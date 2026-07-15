// SPDX-License-Identifier: Apache-2.0
//! Model-space frames resolved from feature-section datum references.

use crate::datum::DatumPlane;
use crate::feature::{
    AffectedIdKind, BinaryFlag, FeatureAffectedIds, FeatureDefinition, FeatureEntityTable,
    FeatureGeometryTable, FeatureGeometryTableKind, FeatureSegmentKind,
};
use crate::surface::{
    OutlinePlane, PlaneEnvelope, PlaneEnvelopeRecord, PlaneLocalSystem, SurfaceKind, SurfaceRow,
};

/// A feature's right-handed section-to-model rigid frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSectionTransform {
    /// Owning `feat_defs_<id>` record identifier.
    pub definition_id: u32,
    /// Unique modeling feature identifier inside the definition, when present.
    pub feature_id: Option<u32>,
    /// Model-space point corresponding to section coordinate `[0, 0, 0]`.
    pub origin: [f64; 3],
    /// Model-space direction of increasing section `u`.
    pub u_axis: [f64; 3],
    /// Model-space direction of increasing section `v`.
    pub v_axis: [f64; 3],
    /// Model-space normal of the section plane.
    pub normal: [f64; 3],
    /// Byte offset of the source `gsec3d_ptr` record.
    pub offset: usize,
}

pub(crate) struct PlacementSources<'a> {
    pub datums: &'a [DatumPlane],
    pub surface_rows: &'a [SurfaceRow],
    pub model_planes: &'a [PlaneLocalSystem],
    pub outline_planes: &'a [OutlinePlane],
    pub plane_envelopes: &'a [PlaneEnvelopeRecord],
    pub geometry_tables: &'a [FeatureGeometryTable],
    pub affected_ids: &'a [FeatureAffectedIds],
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0].mul_add(right[0], left[1].mul_add(right[1], left[2] * right[2]))
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1].mul_add(right[2], -(left[2] * right[1])),
        left[2].mul_add(right[0], -(left[0] * right[2])),
        left[0].mul_add(right[1], -(left[1] * right[0])),
    ]
}

fn add(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn scale(vector: [f64; 3], factor: f64) -> [f64; 3] {
    vector.map(|value| value * factor)
}

fn plane_equation(
    id: u32,
    datums: &[DatumPlane],
    model_planes: &[PlaneLocalSystem],
    outline_planes: &[OutlinePlane],
) -> Option<([f64; 3], f64)> {
    datums
        .iter()
        .find(|datum| datum.id == id)
        .map(|datum| (datum.normal, datum.offset))
        .or_else(|| {
            let plane = model_planes.iter().find(|plane| plane.surface_id == id)?;
            let normal = plane.normal?;
            let origin = plane.origin?;
            Some((normal, dot(normal, origin)))
        })
        .or_else(|| {
            let plane = outline_planes.iter().find(|plane| plane.surface_id == id)?;
            Some((plane.normal, dot(plane.normal, plane.origin)))
        })
}

fn generated_datum_plane_equation(
    sketch_id: u32,
    reference_id: u32,
    reference_normal: [f64; 3],
    sources: &PlacementSources<'_>,
) -> Option<([f64; 3], f64)> {
    let datum_ids = sources
        .geometry_tables
        .iter()
        .filter(|table| table.kind == FeatureGeometryTableKind::DatumIds)
        .filter_map(|table| table.entry_ids.as_ref())
        .flatten()
        .filter(|id| **id == sketch_id)
        .count();
    (datum_ids == 1).then_some(())?;
    let reference_feature = sources
        .datums
        .iter()
        .find(|datum| datum.id == reference_id)
        .map(|datum| datum.feature_id)
        .or_else(|| {
            sources
                .surface_rows
                .iter()
                .find(|row| row.id == reference_id && row.kind == SurfaceKind::Plane)
                .map(|row| row.feature_id)
        })?;
    let candidates = sources
        .affected_ids
        .iter()
        .filter(|record| {
            record.kind == AffectedIdKind::Parents && record.ids.contains(&reference_feature)
        })
        .filter_map(|parents| {
            let other = parents
                .ids
                .iter()
                .filter(|parent| **parent != reference_feature)
                .collect::<Vec<_>>();
            let [other] = other.as_slice() else {
                return None;
            };
            let equations = sources
                .datums
                .iter()
                .filter(|datum| datum.feature_id == **other)
                .map(|datum| (datum.normal, datum.offset))
                .chain(
                    sources
                        .surface_rows
                        .iter()
                        .filter(|row| row.feature_id == **other && row.kind == SurfaceKind::Plane)
                        .filter_map(|row| {
                            plane_equation(
                                row.id,
                                sources.datums,
                                sources.model_planes,
                                sources.outline_planes,
                            )
                        }),
                )
                .chain(
                    sources
                        .surface_rows
                        .iter()
                        .filter(|row| row.feature_id == **other && row.kind == SurfaceKind::Plane)
                        .flat_map(|row| {
                            sources
                                .plane_envelopes
                                .iter()
                                .filter(move |record| record.surface_id == row.id)
                        })
                        .flat_map(|record| {
                            let corners = match &record.envelope {
                                PlaneEnvelope::Standard { corners_3d, .. }
                                | PlaneEnvelope::Compact { corners_3d, .. } => corners_3d,
                            };
                            (0..3).filter_map(move |axis| {
                                if record.corner_coordinate_equal[axis] != Some(true) {
                                    return None;
                                }
                                let coordinate = corners[0][axis]?;
                                let mut normal = [0.0; 3];
                                normal[axis] = 1.0;
                                Some((normal, coordinate))
                            })
                        }),
                )
                .filter(|(normal, _)| dot(*normal, reference_normal).abs() <= 1e-12)
                .fold(Vec::<([f64; 3], f64)>::new(), |mut unique, equation| {
                    if !unique.contains(&equation) {
                        unique.push(equation);
                    }
                    unique
                });
            let [equation] = equations.as_slice() else {
                return None;
            };
            Some(*equation)
        })
        .collect::<Vec<_>>();
    let [equation] = candidates.as_slice() else {
        return None;
    };
    Some(*equation)
}

fn feature_generated_plane_equation(
    id: u32,
    definitions: &[FeatureDefinition],
    transforms: &[FeatureSectionTransform],
    sources: &PlacementSources<'_>,
) -> Option<([f64; 3], f64)> {
    let feature_id = sources
        .surface_rows
        .iter()
        .find(|row| row.id == id && row.kind == SurfaceKind::Plane)?
        .feature_id;
    let transforms = transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let [transform] = transforms.as_slice() else {
        return None;
    };
    let definition = definitions
        .iter()
        .find(|definition| definition.id == transform.definition_id)?;
    let segments = definition.segments.as_ref()?;
    let segments = segments
        .rows
        .iter()
        .filter(|segment| segment.external_id == id && segment.kind == FeatureSegmentKind::Line)
        .collect::<Vec<_>>();
    let [segment] = segments.as_slice() else {
        return None;
    };
    let variables = definition.variables.as_ref()?;
    let point = |point_id| {
        let point = variables
            .points
            .iter()
            .find(|point| point.point_id == point_id)?;
        Some([point.u?, point.v?])
    };
    let start = point(segment.point_ids[0])?;
    let end = point(segment.point_ids[1])?;
    let place = |point: [f64; 2]| {
        std::array::from_fn(|axis| {
            transform.origin[axis]
                + point[0] * transform.u_axis[axis]
                + point[1] * transform.v_axis[axis]
        })
    };
    let start = place(start);
    let end = place(end);
    let direction = std::array::from_fn(|axis| end[axis] - start[axis]);
    let magnitude = dot(direction, direction).sqrt();
    (magnitude > 1e-12).then_some(())?;
    let direction = scale(direction, magnitude.recip());
    let normal = cross(direction, transform.normal);
    let magnitude = dot(normal, normal).sqrt();
    (magnitude > 1e-12).then_some(())?;
    let normal = scale(normal, magnitude.recip());
    Some((normal, dot(normal, start)))
}

fn generated_cap_pair_plane_equation(
    table: &FeatureEntityTable,
    sources: &PlacementSources<'_>,
) -> Option<([f64; 3], f64)> {
    let [first, second, ..] = table.entries.as_slice() else {
        return None;
    };
    if [first.class_id, second.class_id] != [204, 203] {
        return None;
    }
    let first = plane_equation(
        first.entity_id,
        sources.datums,
        sources.model_planes,
        sources.outline_planes,
    )?;
    let second = plane_equation(
        second.entity_id,
        sources.datums,
        sources.model_planes,
        sources.outline_planes,
    )?;
    let oriented_cosine = dot(first.0, second.0);
    let cosine = oriented_cosine.abs();
    let second_offset = if oriented_cosine.is_sign_negative() {
        -second.1
    } else {
        second.1
    };
    let scale = first.1.abs().max(second.1.abs()).max(1.0);
    ((cosine - 1.0).abs() <= 1e-12 && (first.1 - second_offset).abs() > 1e-12 * scale)
        .then_some(first)
}

fn generated_section_cap_plane_equation(
    sketch_id: u32,
    feature_id: u32,
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Option<([f64; 3], f64)> {
    let datum_tables = sources
        .geometry_tables
        .iter()
        .filter(|table| {
            table.feature_id == feature_id
                && table.kind == FeatureGeometryTableKind::DatumIds
                && table.entry_ids.as_deref() == Some(&[sketch_id])
        })
        .collect::<Vec<_>>();
    let [_] = datum_tables.as_slice() else {
        return None;
    };
    let equations = entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .filter_map(|table| generated_cap_pair_plane_equation(table, sources))
        .collect::<Vec<_>>();
    let [equation] = equations.as_slice() else {
        return None;
    };
    Some(*equation)
}

/// Resolve feature frames whose sketch and orientation references reduce to
/// two perpendicular model-space datum planes.
pub(crate) fn resolve(
    definitions: &[FeatureDefinition],
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Vec<FeatureSectionTransform> {
    let mut result = Vec::new();
    for definition in definitions {
        let Some(section) = &definition.section_3d else {
            continue;
        };
        let Some(sketch_id) = section.sketch_plane_entity_id else {
            continue;
        };
        let mut reference_ids = section
            .reference_plane_datum_geometry_id
            .map_or_else(|| section.reference_plane_entity_ids.clone(), |id| vec![id]);
        reference_ids.sort_unstable();
        reference_ids.dedup();
        let direct_sketch = plane_equation(
            sketch_id,
            sources.datums,
            sources.model_planes,
            sources.outline_planes,
        )
        .or_else(|| {
            generated_section_cap_plane_equation(
                sketch_id,
                definition.owner_feature_id?,
                sources,
                entity_tables,
            )
        });
        let mut candidates = Vec::new();
        for reference_id in reference_ids {
            let direct_reference = plane_equation(
                reference_id,
                sources.datums,
                sources.model_planes,
                sources.outline_planes,
            );
            if let Some(sketch) = direct_sketch {
                let reference = direct_reference
                    .or_else(|| {
                        generated_datum_plane_equation(reference_id, sketch_id, sketch.0, sources)
                    })
                    .or_else(|| {
                        feature_generated_plane_equation(
                            reference_id,
                            definitions,
                            &result,
                            sources,
                        )
                    });
                if let Some(reference) = reference {
                    if dot(sketch.0, reference.0).abs() < 1.0 - 1e-12
                        && !candidates.contains(&(sketch, reference))
                    {
                        candidates.push((sketch, reference));
                    }
                }
            } else if let Some(reference) = direct_reference {
                if let Some(sketch) =
                    generated_datum_plane_equation(sketch_id, reference_id, reference.0, sources)
                {
                    if dot(sketch.0, reference.0).abs() < 1.0 - 1e-12
                        && !candidates.contains(&(sketch, reference))
                    {
                        candidates.push((sketch, reference));
                    }
                }
            }
        }
        let [(sketch, reference)] = candidates.as_slice() else {
            continue;
        };
        let (mut sketch_normal, mut sketch_offset) = *sketch;
        let (mut reference_normal, mut reference_offset) = *reference;
        if section.sketch_plane_flip == Some(BinaryFlag::Set) {
            sketch_normal = scale(sketch_normal, -1.0);
            sketch_offset = -sketch_offset;
        }
        if section.orientation.section_flip == Some(BinaryFlag::Set) {
            sketch_normal = scale(sketch_normal, -1.0);
            sketch_offset = -sketch_offset;
        }
        if section.orientation.reference_flip == Some(BinaryFlag::Set) {
            reference_normal = scale(reference_normal, -1.0);
            reference_offset = -reference_offset;
        }
        let normal = sketch_normal;
        let cosine = dot(normal, reference_normal);
        let denominator = 1.0 - cosine * cosine;
        if denominator <= 1e-12 {
            continue;
        }
        let u_axis = scale(
            add(reference_normal, scale(normal, -cosine)),
            denominator.sqrt().recip(),
        );
        let v_axis = cross(normal, u_axis);
        if (dot(v_axis, v_axis) - 1.0).abs() > 1e-12 {
            continue;
        }
        let sketch_factor = (sketch_offset - cosine * reference_offset) / denominator;
        let reference_factor = (reference_offset - cosine * sketch_offset) / denominator;
        result.push(FeatureSectionTransform {
            definition_id: definition.id,
            feature_id: definition.owner_feature_id,
            origin: add(
                scale(sketch_normal, sketch_factor),
                scale(reference_normal, reference_factor),
            ),
            u_axis,
            v_axis,
            normal,
            offset: section.offset,
        });
    }
    result.sort_by_key(|transform| transform.offset);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::{
        FeatureSection3d, FeatureSectionOrientation, FeatureSectionPoint, FeatureSegment,
        FeatureSegmentTable, FeatureVariableTable,
    };

    fn datum(id: u32, normal: [f64; 3], offset: f64) -> DatumPlane {
        DatumPlane {
            id,
            feature_id: id.saturating_sub(1),
            normal,
            offset,
            corners: [[Some(0.0); 3]; 2],
            offset_in_payload: usize::try_from(id).expect("fixture id fits usize"),
        }
    }

    #[test]
    fn resolves_perpendicular_datum_frame() {
        let definition = FeatureDefinition {
            id: 42,
            owner_feature_id: Some(42),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: Some(FeatureSection3d {
                sketch_plane_entity_id: Some(2),
                sketch_plane_flip: Some(BinaryFlag::Clear),
                reference_plane_entity_ids: vec![3, 4],
                reference_plane_datum_geometry_id: Some(4),
                orientation: FeatureSectionOrientation::default(),
                dimension_ids: Vec::new(),
                offset: 100,
            }),
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 90,
        };
        assert_eq!(
            resolve(
                &[definition],
                &PlacementSources {
                    datums: &[
                        datum(2, [1.0, 0.0, 0.0], 2.0),
                        datum(3, [1.0, 0.0, 0.0], 1.0),
                        datum(4, [0.0, 0.0, 1.0], 3.0),
                    ],
                    surface_rows: &[],
                    model_planes: &[],
                    outline_planes: &[],
                    plane_envelopes: &[],
                    geometry_tables: &[],
                    affected_ids: &[],
                },
                &[],
            ),
            vec![FeatureSectionTransform {
                definition_id: 42,
                feature_id: Some(42),
                origin: [2.0, 0.0, 3.0],
                u_axis: [0.0, 0.0, 1.0],
                v_axis: [0.0, -1.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                offset: 100,
            }]
        );
    }

    #[test]
    fn resolves_generated_section_from_declared_cap_pair() {
        let definition = FeatureDefinition {
            id: 917,
            owner_feature_id: Some(40),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: Some(FeatureSection3d {
                sketch_plane_entity_id: Some(42),
                sketch_plane_flip: None,
                reference_plane_entity_ids: vec![191],
                reference_plane_datum_geometry_id: Some(2),
                orientation: FeatureSectionOrientation::default(),
                dimension_ids: Vec::new(),
                offset: 100,
            }),
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 90,
        };
        let rows = [43, 92].map(|id| SurfaceRow {
            id,
            type_byte: 0x22,
            kind: SurfaceKind::Plane,
            feature_id: 40,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture id fits usize"),
        });
        let outlines = [
            OutlinePlane {
                surface_id: 43,
                origin: [0.0, 0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                u_axis: [1.0, 0.0, 0.0],
                offset: 43,
            },
            OutlinePlane {
                surface_id: 92,
                origin: [0.0, 38.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                u_axis: [1.0, 0.0, 0.0],
                offset: 92,
            },
        ];
        let geometry_tables = [FeatureGeometryTable {
            feature_id: 40,
            kind: FeatureGeometryTableKind::DatumIds,
            count: 1,
            entity_class: 1,
            entry_ids: Some(vec![42]),
            offset: 80,
        }];
        let entries = [(43, 204), (92, 203)].map(|(entity_id, class_id)| {
            crate::feature::FeatureEntityTableEntry {
                entity_id,
                class_id,
                source_entity_id: None,
                prefixed: false,
                offset: usize::try_from(entity_id).expect("fixture id fits usize"),
                end_offset: usize::try_from(entity_id + 1).expect("fixture id fits usize"),
            }
        });
        let entity_tables = [
            FeatureEntityTable {
                feature_id: Some(40),
                entry_ids: vec![700],
                entries: vec![crate::feature::FeatureEntityTableEntry {
                    entity_id: 700,
                    class_id: 7,
                    source_entity_id: None,
                    prefixed: false,
                    offset: 60,
                    end_offset: 61,
                }],
                surface_ids: Vec::new(),
                non_surface_entity_ids: vec![700],
                offset: 50,
            },
            FeatureEntityTable {
                feature_id: Some(40),
                entry_ids: vec![43, 92],
                entries: entries.to_vec(),
                surface_ids: vec![43, 92],
                non_surface_entity_ids: Vec::new(),
                offset: 70,
            },
        ];

        assert_eq!(
            resolve(
                &[definition],
                &PlacementSources {
                    datums: &[
                        datum(2, [1.0, 0.0, 0.0], 0.0),
                        datum(191, [1.0, 0.0, 0.0], 8.0),
                    ],
                    surface_rows: &rows,
                    model_planes: &[],
                    outline_planes: &outlines,
                    plane_envelopes: &[],
                    geometry_tables: &geometry_tables,
                    affected_ids: &[],
                },
                &entity_tables,
            ),
            vec![FeatureSectionTransform {
                definition_id: 917,
                feature_id: Some(40),
                origin: [0.0, 0.0, 0.0],
                u_axis: [1.0, 0.0, 0.0],
                v_axis: [0.0, 0.0, -1.0],
                normal: [0.0, 1.0, 0.0],
                offset: 100,
            }]
        );
    }

    #[test]
    fn resolves_oblique_reference_from_an_earlier_extruded_line() {
        let source = FeatureDefinition {
            id: 917,
            owner_feature_id: Some(40),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(FeatureVariableTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                points: vec![
                    FeatureSectionPoint {
                        point_id: 8,
                        u: Some(0.0),
                        v: Some(0.0),
                    },
                    FeatureSectionPoint {
                        point_id: 9,
                        u: Some(0.0),
                        v: Some(1.0),
                    },
                ],
                offset: 10,
            }),
            segments: Some(FeatureSegmentTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![FeatureSegment {
                    kind: FeatureSegmentKind::Line,
                    directions: [None; 3],
                    point_ids: [8, 9],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: None,
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 43,
                    offset: 20,
                }],
                offset: 20,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: Some(FeatureSection3d {
                sketch_plane_entity_id: Some(2),
                sketch_plane_flip: None,
                reference_plane_entity_ids: vec![4],
                reference_plane_datum_geometry_id: Some(4),
                orientation: FeatureSectionOrientation::default(),
                dimension_ids: Vec::new(),
                offset: 30,
            }),
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 5,
        };
        let dependent = FeatureDefinition {
            id: 579,
            owner_feature_id: Some(579),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: Some(FeatureSection3d {
                sketch_plane_entity_id: Some(799),
                sketch_plane_flip: None,
                reference_plane_entity_ids: vec![43],
                reference_plane_datum_geometry_id: None,
                orientation: FeatureSectionOrientation::default(),
                dimension_ids: Vec::new(),
                offset: 40,
            }),
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 35,
        };
        let generated_plane = SurfaceRow {
            id: 43,
            type_byte: 0x22,
            kind: SurfaceKind::Plane,
            feature_id: 40,
            reversed: false,
            boundary_type: 1,
            next_surface: 0,
            offset: 50,
        };

        let transforms = resolve(
            &[source, dependent],
            &PlacementSources {
                datums: &[
                    datum(2, [1.0, 0.0, 0.0], 0.0),
                    datum(4, [0.0, 0.0, 1.0], 0.0),
                    datum(799, [0.0, 1.0, 0.0], 1.0),
                ],
                surface_rows: &[generated_plane],
                model_planes: &[],
                outline_planes: &[],
                plane_envelopes: &[],
                geometry_tables: &[],
                affected_ids: &[],
            },
            &[],
        );

        assert_eq!(transforms.len(), 2);
        assert_eq!(transforms[1].definition_id, 579);
        assert_eq!(transforms[1].feature_id, Some(579));
        assert_eq!(transforms[1].origin, [0.0, 1.0, 0.0]);
        assert_eq!(transforms[1].u_axis, [0.0, 0.0, 1.0]);
        assert_eq!(transforms[1].normal, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn resolves_orientation_from_an_outline_plane_carrier() {
        let definition = FeatureDefinition {
            id: 42,
            owner_feature_id: Some(42),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: Some(FeatureSection3d {
                sketch_plane_entity_id: Some(2),
                sketch_plane_flip: Some(BinaryFlag::Clear),
                reference_plane_entity_ids: vec![4],
                reference_plane_datum_geometry_id: Some(4),
                orientation: FeatureSectionOrientation::default(),
                dimension_ids: Vec::new(),
                offset: 100,
            }),
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 90,
        };
        let reference = OutlinePlane {
            surface_id: 4,
            origin: [0.0, 0.0, 3.0],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 70,
        };

        let transforms = resolve(
            &[definition],
            &PlacementSources {
                datums: &[datum(2, [1.0, 0.0, 0.0], 2.0)],
                surface_rows: &[],
                model_planes: &[],
                outline_planes: &[reference],
                plane_envelopes: &[],
                geometry_tables: &[],
                affected_ids: &[],
            },
            &[],
        );
        assert_eq!(transforms.len(), 1);
        assert_eq!(transforms[0].origin, [2.0, 0.0, 3.0]);
        assert_eq!(transforms[0].u_axis, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn resolves_generated_sketch_datum_from_unique_parent_relation() {
        let definition = FeatureDefinition {
            id: 80,
            owner_feature_id: Some(40),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: Some(FeatureSection3d {
                sketch_plane_entity_id: Some(42),
                sketch_plane_flip: None,
                reference_plane_entity_ids: vec![90],
                reference_plane_datum_geometry_id: Some(2),
                orientation: FeatureSectionOrientation {
                    section_flip: Some(BinaryFlag::Set),
                    reference_type: Some(5),
                    segment_id: None,
                    reference_flip: Some(BinaryFlag::Clear),
                },
                dimension_ids: Vec::new(),
                offset: 100,
            }),
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 90,
        };
        let geometry_table = FeatureGeometryTable {
            feature_id: 40,
            kind: FeatureGeometryTableKind::DatumIds,
            count: 1,
            entity_class: 87,
            entry_ids: Some(vec![42]),
            offset: 20,
        };
        let parents = FeatureAffectedIds {
            feature_id: 11,
            kind: AffectedIdKind::Parents,
            ids: vec![1, 3],
            offset: 40,
        };
        let transforms = resolve(
            &[definition],
            &PlacementSources {
                datums: &[
                    datum(2, [1.0, 0.0, 0.0], 0.0),
                    datum(4, [0.0, 1.0, 0.0], 0.0),
                ],
                surface_rows: &[],
                model_planes: &[],
                outline_planes: &[],
                plane_envelopes: &[],
                geometry_tables: &[geometry_table],
                affected_ids: &[parents],
            },
            &[],
        );
        assert_eq!(transforms.len(), 1);
        assert_eq!(transforms[0].normal, [0.0, -1.0, 0.0]);
        assert_eq!(transforms[0].u_axis, [1.0, 0.0, 0.0]);
        assert_eq!(transforms[0].v_axis, [-0.0, 0.0, 1.0]);
    }

    #[test]
    fn resolves_generated_plane_from_contextually_unambiguous_envelope_axis() {
        let definition = FeatureDefinition {
            id: 80,
            owner_feature_id: Some(40),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: Some(FeatureSection3d {
                sketch_plane_entity_id: Some(42),
                sketch_plane_flip: None,
                reference_plane_entity_ids: vec![90],
                reference_plane_datum_geometry_id: Some(2),
                orientation: FeatureSectionOrientation::default(),
                dimension_ids: Vec::new(),
                offset: 100,
            }),
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 90,
        };
        let geometry_table = FeatureGeometryTable {
            feature_id: 40,
            kind: FeatureGeometryTableKind::DatumIds,
            count: 1,
            entity_class: 87,
            entry_ids: Some(vec![42]),
            offset: 20,
        };
        let parents = FeatureAffectedIds {
            feature_id: 40,
            kind: AffectedIdKind::Parents,
            ids: vec![1, 3],
            offset: 40,
        };
        let row = SurfaceRow {
            id: 7,
            type_byte: 0x22,
            kind: SurfaceKind::Plane,
            feature_id: 3,
            reversed: false,
            boundary_type: 1,
            next_surface: 0,
            offset: 50,
        };
        let envelope = PlaneEnvelopeRecord {
            surface_id: 7,
            body: Vec::new(),
            envelope: PlaneEnvelope::Standard {
                bounds_2d: [[Some(0.0); 2]; 2],
                corners_3d: [
                    [Some(0.0), Some(-1.0), Some(3.0)],
                    [Some(0.0), Some(1.0), Some(3.0)],
                ],
            },
            corner_coordinate_equal: [Some(true), Some(false), Some(true)],
            row_offset: 50,
            offset: 60,
        };

        let transforms = resolve(
            &[definition],
            &PlacementSources {
                datums: &[datum(2, [1.0, 0.0, 0.0], 0.0)],
                surface_rows: &[row],
                model_planes: &[],
                outline_planes: &[],
                plane_envelopes: &[envelope],
                geometry_tables: &[geometry_table],
                affected_ids: &[parents],
            },
            &[],
        );
        assert_eq!(transforms.len(), 1);
        assert_eq!(transforms[0].origin, [0.0, 0.0, 3.0]);
        assert_eq!(transforms[0].normal, [0.0, 0.0, 1.0]);
    }
}
