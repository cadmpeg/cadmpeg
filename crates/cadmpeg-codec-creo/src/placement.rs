// SPDX-License-Identifier: Apache-2.0
//! Model-space frames resolved from feature-section datum references.

use crate::datum::DatumPlane;
use crate::feature::{
    AffectedIdKind, BinaryFlag, FeatureAffectedIds, FeatureDefinition, FeatureEntityTable,
    FeatureGeometryTable, FeatureGeometryTableKind,
};
use crate::surface::{OutlinePlane, PlaneLocalSystem};

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
    datums: &[DatumPlane],
    geometry_tables: &[FeatureGeometryTable],
    entity_tables: &[FeatureEntityTable],
    affected_ids: &[FeatureAffectedIds],
) -> Option<([f64; 3], f64)> {
    let datum_ids = geometry_tables
        .iter()
        .filter(|table| table.kind == FeatureGeometryTableKind::DatumIds)
        .filter_map(|table| table.entry_ids.as_ref())
        .flatten()
        .filter(|id| **id == sketch_id)
        .count();
    (datum_ids == 1).then_some(())?;
    let generators = entity_tables
        .iter()
        .filter(|table| table.entry_ids.contains(&sketch_id))
        .filter_map(|table| table.feature_id)
        .collect::<std::collections::BTreeSet<_>>();
    let generators = generators.into_iter().collect::<Vec<_>>();
    let [generator] = generators.as_slice() else {
        return None;
    };
    let reference_feature = datums
        .iter()
        .find(|datum| datum.id == reference_id)?
        .feature_id;
    let parent_rows = affected_ids
        .iter()
        .filter(|record| record.feature_id == *generator && record.kind == AffectedIdKind::Parents)
        .collect::<Vec<_>>();
    let [parents] = parent_rows.as_slice() else {
        return None;
    };
    let candidates = parents
        .ids
        .iter()
        .filter(|parent| **parent != reference_feature)
        .filter_map(|parent| datums.iter().find(|datum| datum.feature_id == *parent))
        .collect::<Vec<_>>();
    let [datum] = candidates.as_slice() else {
        return None;
    };
    Some((datum.normal, datum.offset))
}

/// Resolve feature frames whose sketch and orientation references reduce to
/// two perpendicular model-space datum planes.
pub fn resolve(
    definitions: &[FeatureDefinition],
    datums: &[DatumPlane],
    model_planes: &[PlaneLocalSystem],
    outline_planes: &[OutlinePlane],
    geometry_tables: &[FeatureGeometryTable],
    entity_tables: &[FeatureEntityTable],
    affected_ids: &[FeatureAffectedIds],
) -> Vec<FeatureSectionTransform> {
    let mut result = Vec::new();
    for definition in definitions {
        let Some(section) = &definition.section_3d else {
            continue;
        };
        let Some(sketch_id) = section.sketch_plane_entity_id else {
            continue;
        };
        let Some(reference_id) = section.reference_plane_datum_geometry_id else {
            continue;
        };
        let Some((mut sketch_normal, mut sketch_offset)) =
            plane_equation(sketch_id, datums, model_planes, outline_planes).or_else(|| {
                generated_datum_plane_equation(
                    sketch_id,
                    reference_id,
                    datums,
                    geometry_tables,
                    entity_tables,
                    affected_ids,
                )
            })
        else {
            continue;
        };
        let Some((mut reference_normal, mut reference_offset)) =
            plane_equation(reference_id, datums, model_planes, outline_planes)
        else {
            continue;
        };
        if dot(sketch_normal, reference_normal).abs() > 1e-12 {
            continue;
        }
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
        let u_axis = reference_normal;
        let v_axis = cross(normal, u_axis);
        if (dot(v_axis, v_axis) - 1.0).abs() > 1e-12 {
            continue;
        }
        result.push(FeatureSectionTransform {
            definition_id: definition.id,
            feature_id: definition.owner_feature_id,
            origin: add(
                scale(sketch_normal, sketch_offset),
                scale(reference_normal, reference_offset),
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
    use crate::feature::{FeatureSection3d, FeatureSectionOrientation};

    fn datum(id: u32, normal: [f64; 3], offset: f64) -> DatumPlane {
        DatumPlane {
            id,
            feature_id: id.saturating_sub(1),
            normal,
            offset,
            corners: [[0.0; 3]; 2],
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
        assert_eq!(
            resolve(
                &[definition],
                &[
                    datum(2, [1.0, 0.0, 0.0], 2.0),
                    datum(4, [0.0, 0.0, 1.0], 3.0),
                ],
                &[],
                &[],
                &[],
                &[],
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
            &[datum(2, [1.0, 0.0, 0.0], 2.0)],
            &[],
            &[reference],
            &[],
            &[],
            &[],
        );
        assert_eq!(transforms.len(), 1);
        assert_eq!(transforms[0].origin, [2.0, 0.0, 3.0]);
        assert_eq!(transforms[0].u_axis, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn resolves_generated_sketch_datum_from_the_other_parent() {
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
        let entity_table = FeatureEntityTable {
            feature_id: Some(41),
            entry_ids: vec![42],
            surface_ids: vec![42],
            non_surface_entity_ids: Vec::new(),
            offset: 30,
        };
        let parents = FeatureAffectedIds {
            feature_id: 41,
            kind: AffectedIdKind::Parents,
            ids: vec![1, 3],
            offset: 40,
        };
        let transforms = resolve(
            &[definition],
            &[
                datum(2, [1.0, 0.0, 0.0], 0.0),
                datum(4, [0.0, 1.0, 0.0], 0.0),
            ],
            &[],
            &[],
            &[geometry_table],
            &[entity_table],
            &[parents],
        );
        assert_eq!(transforms.len(), 1);
        assert_eq!(transforms[0].normal, [0.0, -1.0, 0.0]);
        assert_eq!(transforms[0].u_axis, [1.0, 0.0, 0.0]);
        assert_eq!(transforms[0].v_axis, [-0.0, 0.0, 1.0]);
    }
}
