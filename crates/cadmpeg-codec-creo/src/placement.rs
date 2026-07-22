// SPDX-License-Identifier: Apache-2.0
//! Model-space frames resolved from feature-section datum references.

use crate::datum::DatumPlane;
use crate::feature::{
    placement_instructions, AffectedIdKind, BinaryFlag, FeatureAffectedIds, FeatureDefinition,
    FeatureEntityTable, FeatureGeometryTable, FeatureGeometryTableKind, FeatureParameterFrameKind,
    FeatureSegmentKind,
};
use crate::surface::{
    unique_surface_row, OutlinePlane, PlaneEnvelope, PlaneEnvelopeRecord, PlaneLocalSystem,
    SurfaceKind, SurfaceParameterRecord, SurfaceRow,
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
    pub surface_parameters: &'a [SurfaceParameterRecord],
    pub geometry_tables: &'a [FeatureGeometryTable],
    pub affected_ids: &'a [FeatureAffectedIds],
}

fn generated_cylinder_section_transform(
    definition: &FeatureDefinition,
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Option<FeatureSectionTransform> {
    let feature_id = definition.owner_feature_id?;
    definition.segments.as_ref()?.is_complete().then_some(())?;
    let points = definition.variables.as_ref()?.reconciled_points();
    points.1.is_empty().then_some(())?;
    let mut correspondences = Vec::<([f64; 2], [f64; 3], [f64; 3], usize)>::new();
    for entry in entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .flat_map(|table| &table.entries)
    {
        let Some(external_id) = entry.source_entity_id else {
            continue;
        };
        let Some(segment) = definition.segments.as_ref()?.segment(external_id) else {
            continue;
        };
        if segment.kind != FeatureSegmentKind::Arc {
            continue;
        }
        let Some(center_id) = segment.center_id else {
            continue;
        };
        let Some([Some(u), Some(v)]) = points.0.get(&center_id).copied() else {
            continue;
        };
        let Some(row) = unique_surface_row(sources.surface_rows, entry.entity_id)
            .filter(|row| row.feature_id == feature_id && row.kind == SurfaceKind::Cylinder)
        else {
            continue;
        };
        let parameters = sources
            .surface_parameters
            .iter()
            .filter(|record| record.surface_id == row.id)
            .collect::<Vec<_>>();
        let [parameters] = parameters.as_slice() else {
            continue;
        };
        let Some(frame) = parameters.positional_cylinder_frame else {
            continue;
        };
        correspondences.push(([u, v], frame.origin, frame.axis, parameters.offset));
    }
    let first = correspondences.first()?;
    let normal = normalize(first.2)?;
    let scale = correspondences
        .iter()
        .flat_map(|(local, model, _, _)| local.iter().chain(model))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let close =
        |left: f64, right: f64| (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0);
    correspondences
        .iter()
        .all(|(_, _, axis, _)| {
            normalize(*axis).is_some_and(|axis| {
                axis.iter()
                    .zip(normal)
                    .all(|(left, right)| close(*left, right))
            })
        })
        .then_some(())?;

    let mut frames = Vec::new();
    for second in correspondences.iter().skip(1) {
        let local = [second.0[0] - first.0[0], second.0[1] - first.0[1]];
        let model = std::array::from_fn::<_, 3, _>(|index| second.1[index] - first.1[index]);
        let local_squared = dot([local[0], local[1], 0.0], [local[0], local[1], 0.0]);
        if local_squared <= 1e-24 * scale * scale
            || !close(dot(model, model), local_squared)
            || !close(dot(model, normal), 0.0)
        {
            continue;
        }
        let normal_cross_model = cross(normal, model);
        let u_axis = std::array::from_fn(|index| {
            (local[0] * model[index] - local[1] * normal_cross_model[index]) / local_squared
        });
        let Some(u_axis) = normalize(u_axis) else {
            continue;
        };
        let v_axis = cross(normal, u_axis);
        let origin = std::array::from_fn(|index| {
            first.1[index] - first.0[0] * u_axis[index] - first.0[1] * v_axis[index]
        });
        frames.push((origin, u_axis, v_axis));
    }
    let valid = frames
        .iter()
        .filter(|candidate| {
            correspondences.iter().all(|(local, model, _, _)| {
                (0..3).all(|index| {
                    close(
                        candidate.0[index]
                            + local[0] * candidate.1[index]
                            + local[1] * candidate.2[index],
                        model[index],
                    )
                })
            })
        })
        .collect::<Vec<_>>();
    let frame = *valid.first()?;
    valid
        .iter()
        .all(|candidate| {
            candidate
                .0
                .iter()
                .chain(candidate.1.iter())
                .chain(candidate.2.iter())
                .zip(frame.0.iter().chain(frame.1.iter()).chain(frame.2.iter()))
                .all(|(left, right)| close(*left, *right))
        })
        .then_some(())?;
    Some(FeatureSectionTransform {
        definition_id: definition.id,
        feature_id: Some(feature_id),
        origin: frame.0,
        u_axis: frame.1,
        v_axis: frame.2,
        normal,
        offset: correspondences.iter().map(|item| item.3).min()?,
    })
}

fn generated_planar_section_transform(
    definition: &FeatureDefinition,
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Option<FeatureSectionTransform> {
    let feature_id = definition.owner_feature_id?;
    let segments = definition.segments.as_ref()?;
    segments.is_complete().then_some(())?;
    let (points, conflicting_points) = definition.variables.as_ref()?.reconciled_points();
    conflicting_points.is_empty().then_some(())?;
    let tables = entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .filter(|table| {
            table.entries.len() >= 4
                && table.entries[0].class_id == 204
                && table.entries[1].class_id == 203
                && table.entries[2..]
                    .iter()
                    .all(|entry| entry.class_id == 200 && entry.source_entity_id.is_some())
        })
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let generated_plane_equation = |entry: &crate::feature::FeatureEntityTableEntry| {
        let mut matches = sources
            .outline_planes
            .iter()
            .filter(|plane| plane.surface_id == entry.entity_id);
        let plane = matches.next()?;
        matches.next().is_none().then_some(())?;
        Some((plane.normal, dot(plane.normal, plane.origin)))
    };
    let caps = [
        generated_plane_equation(&table.entries[0])?,
        generated_plane_equation(&table.entries[1])?,
    ];
    let mut sides = Vec::new();
    for entry in &table.entries[2..] {
        let segment = segments.segment(entry.source_entity_id?)?;
        (segment.kind == FeatureSegmentKind::Line).then_some(())?;
        let point = |point_id| {
            let point = points.get(&point_id)?;
            Some([point[0]?, point[1]?])
        };
        let start = point(segment.point_ids[0])?;
        let end = point(segment.point_ids[1])?;
        let direction = [end[0] - start[0], end[1] - start[1]];
        let length = direction[0].hypot(direction[1]);
        (length.is_finite() && length > 1e-12).then_some(())?;
        let local_normal = [direction[1] / length, -direction[0] / length];
        let local_offset = local_normal[0].mul_add(start[0], local_normal[1] * start[1]);
        let (model_normal, model_offset) = generated_plane_equation(entry)?;
        let magnitude = dot(model_normal, model_normal).sqrt();
        (magnitude.is_finite() && magnitude > 1e-12).then_some(())?;
        sides.push((
            local_normal,
            local_offset,
            scale(model_normal, magnitude.recip()),
            model_offset / magnitude,
        ));
    }

    let close =
        |left: f64, right: f64| (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0);
    let vectors_close = |left: [f64; 3], right: [f64; 3]| {
        left.into_iter()
            .zip(right)
            .all(|(left, right)| close(left, right))
    };
    let mut candidates = Vec::new();
    for first_index in 0..sides.len() {
        for second_index in first_index + 1..sides.len() {
            let first = sides[first_index];
            let second = sides[second_index];
            let determinant = first.0[0].mul_add(second.0[1], -(first.0[1] * second.0[0]));
            if determinant.abs() <= 1e-9 {
                continue;
            }
            for first_sign in [-1.0, 1.0] {
                for second_sign in [-1.0, 1.0] {
                    let first_normal = scale(first.2, first_sign);
                    let second_normal = scale(second.2, second_sign);
                    let u_axis = std::array::from_fn(|axis| {
                        (second.0[1] * first_normal[axis] - first.0[1] * second_normal[axis])
                            / determinant
                    });
                    let v_axis = std::array::from_fn(|axis| {
                        (-second.0[0] * first_normal[axis] + first.0[0] * second_normal[axis])
                            / determinant
                    });
                    if !close(dot(u_axis, u_axis), 1.0)
                        || !close(dot(v_axis, v_axis), 1.0)
                        || !close(dot(u_axis, v_axis), 0.0)
                    {
                        continue;
                    }
                    let normal = cross(u_axis, v_axis);
                    let cap_alignment = dot(normal, caps[0].0);
                    if !close(cap_alignment.abs(), 1.0) {
                        continue;
                    }
                    let cap_offset = if cap_alignment.is_sign_negative() {
                        -caps[0].1
                    } else {
                        caps[0].1
                    };
                    let side_coordinate = |side: &([f64; 2], f64, [f64; 3], f64)| {
                        let predicted = add(scale(u_axis, side.0[0]), scale(v_axis, side.0[1]));
                        let alignment = dot(predicted, side.2);
                        close(alignment.abs(), 1.0).then(|| {
                            let offset = if alignment.is_sign_negative() {
                                -side.3
                            } else {
                                side.3
                            };
                            (predicted, offset - side.1)
                        })
                    };
                    let Some((_, first_coordinate)) = side_coordinate(&first) else {
                        continue;
                    };
                    let Some((_, second_coordinate)) = side_coordinate(&second) else {
                        continue;
                    };
                    let origin_u = (second.0[1] * first_coordinate
                        - first.0[1] * second_coordinate)
                        / determinant;
                    let origin_v = (-second.0[0] * first_coordinate
                        + first.0[0] * second_coordinate)
                        / determinant;
                    let origin = add(
                        add(scale(u_axis, origin_u), scale(v_axis, origin_v)),
                        scale(normal, cap_offset),
                    );
                    if sides.iter().any(|side| {
                        side_coordinate(side).is_none_or(|(predicted, coordinate)| {
                            !close(dot(predicted, origin), coordinate)
                        })
                    }) {
                        continue;
                    }
                    let second_cap_alignment = dot(normal, caps[1].0);
                    if !close(second_cap_alignment.abs(), 1.0) {
                        continue;
                    }
                    let second_cap_offset = if second_cap_alignment.is_sign_negative() {
                        -caps[1].1
                    } else {
                        caps[1].1
                    };
                    if close(second_cap_offset, cap_offset) {
                        continue;
                    }
                    let candidate = (origin, u_axis, v_axis, normal);
                    if !candidates.iter().any(
                        |existing: &([f64; 3], [f64; 3], [f64; 3], [f64; 3])| {
                            vectors_close(existing.0, candidate.0)
                                && vectors_close(existing.1, candidate.1)
                                && vectors_close(existing.2, candidate.2)
                                && vectors_close(existing.3, candidate.3)
                        },
                    ) {
                        candidates.push(candidate);
                    }
                }
            }
        }
    }
    let [(origin, u_axis, v_axis, normal)] = candidates.as_slice() else {
        return None;
    };
    Some(FeatureSectionTransform {
        definition_id: definition.id,
        feature_id: Some(feature_id),
        origin: *origin,
        u_axis: *u_axis,
        v_axis: *v_axis,
        normal: *normal,
        offset: table.offset,
    })
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

fn normalize(vector: [f64; 3]) -> Option<[f64; 3]> {
    let magnitude = vector
        .iter()
        .fold(0.0_f64, |norm, value| norm.hypot(*value));
    (magnitude.is_finite() && magnitude > 1e-12).then(|| scale(vector, magnitude.recip()))
}

fn plane_equation(
    id: u32,
    datums: &[DatumPlane],
    model_planes: &[PlaneLocalSystem],
    outline_planes: &[OutlinePlane],
) -> Option<([f64; 3], f64)> {
    let datums = datums
        .iter()
        .filter(|datum| datum.id == id)
        .collect::<Vec<_>>();
    if let [datum] = datums.as_slice() {
        return Some((datum.normal, datum.offset));
    }
    if !datums.is_empty() {
        return None;
    }
    let model_planes = model_planes
        .iter()
        .filter(|plane| plane.surface_id == id)
        .collect::<Vec<_>>();
    if let [plane] = model_planes.as_slice() {
        let normal = plane.normal?;
        let origin = plane.origin?;
        return Some((normal, dot(normal, origin)));
    }
    if !model_planes.is_empty() {
        return None;
    }
    let outline_planes = outline_planes
        .iter()
        .filter(|plane| plane.surface_id == id)
        .collect::<Vec<_>>();
    let [plane] = outline_planes.as_slice() else {
        return None;
    };
    Some((plane.normal, dot(plane.normal, plane.origin)))
}

fn definition_local_plane_equation(definition: &FeatureDefinition) -> Option<([f64; 3], f64)> {
    let frames = definition
        .parameter_frames
        .iter()
        .filter(|frame| frame.kind == FeatureParameterFrameKind::LocalSystem)
        .collect::<Vec<_>>();
    let [frame] = frames.as_slice() else {
        return None;
    };
    let values: [f64; 12] = frame.decoded_values.clone()?.try_into().ok()?;
    let raw_normal: [f64; 3] = values[6..9].try_into().ok()?;
    let normal = normalize(raw_normal)?;
    let origin: [f64; 3] = values[9..12].try_into().ok()?;
    Some((normal, dot(normal, origin)))
}

fn definition_local_frame_transform(
    definition: &FeatureDefinition,
    section: &crate::feature::FeatureSection3d,
) -> Option<FeatureSectionTransform> {
    let feature_id = definition.owner_feature_id?;
    let frames = definition
        .parameter_frames
        .iter()
        .filter(|frame| frame.kind == FeatureParameterFrameKind::LocalSystem)
        .collect::<Vec<_>>();
    let [frame] = frames.as_slice() else {
        return None;
    };
    let values: [f64; 12] = frame.decoded_values.clone()?.try_into().ok()?;
    let mut reference_axis = normalize(values[0..3].try_into().ok()?)?;
    let mut normal = normalize(values[6..9].try_into().ok()?)?;
    (dot(reference_axis, normal).abs() <= 1e-12).then_some(())?;
    let origin: [f64; 3] = values[9..12].try_into().ok()?;
    if section.sketch_plane_flip == Some(BinaryFlag::Set) {
        normal = scale(normal, -1.0);
    }
    if section.orientation.section_flip == Some(BinaryFlag::Set) {
        normal = scale(normal, -1.0);
    }
    if section.orientation.reference_flip == Some(BinaryFlag::Set) {
        reference_axis = scale(reference_axis, -1.0);
    }
    let u_axis = cross(reference_axis, normal);
    ((dot(u_axis, u_axis) - 1.0).abs() <= 1e-12).then_some(FeatureSectionTransform {
        definition_id: definition.id,
        feature_id: Some(feature_id),
        origin,
        u_axis,
        v_axis: reference_axis,
        normal,
        offset: section.offset,
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
    let datums = sources
        .datums
        .iter()
        .filter(|datum| datum.id == reference_id)
        .collect::<Vec<_>>();
    let reference_feature = match datums.as_slice() {
        [datum] => Some(datum.feature_id),
        [] => unique_surface_row(sources.surface_rows, reference_id)
            .filter(|row| row.kind == SurfaceKind::Plane)
            .map(|row| row.feature_id),
        _ => None,
    }?;
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
    let surface_rows = sources
        .surface_rows
        .iter()
        .filter(|row| row.id == id && row.kind == SurfaceKind::Plane)
        .collect::<Vec<_>>();
    let [surface_row] = surface_rows.as_slice() else {
        return None;
    };
    let feature_id = surface_row.feature_id;
    let transforms = transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let [transform] = transforms.as_slice() else {
        return None;
    };
    let definitions = definitions
        .iter()
        .filter(|definition| definition.id == transform.definition_id)
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        return None;
    };
    let segments = definition.segments.as_ref()?;
    let segment = segments.segment(id)?;
    (segment.kind == FeatureSegmentKind::Line).then_some(())?;
    let variables = definition.variables.as_ref()?;
    let (points, _) = variables.reconciled_points();
    let point = |point_id| {
        let point = points.get(&point_id)?;
        Some([point[0]?, point[1]?])
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

fn zero_offset_standard_section_plane_equation(
    definition: &FeatureDefinition,
    section: &crate::feature::FeatureSection3d,
    reference_id: u32,
    reference: ([f64; 3], f64),
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Option<([f64; 3], f64)> {
    let feature_id = definition.owner_feature_id?;
    let sketch_id = section.sketch_plane_entity_id?;
    let instructions = placement_instructions(definition);
    let instruction = instructions.first()?;
    instructions
        .iter()
        .all(|candidate| {
            candidate.kind == instruction.kind
                && candidate.zero_offset == instruction.zero_offset
                && candidate.dimension_id == instruction.dimension_id
                && candidate.reference_id == instruction.reference_id
                && candidate.geometry1_id == instruction.geometry1_id
                && candidate.geometry2_id == instruction.geometry2_id
                && candidate.member1 == instruction.member1
                && candidate.member2 == instruction.member2
        })
        .then_some(())?;
    (instruction.kind == 20_127
        && instruction.zero_offset
        && instruction.dimension_id.is_none()
        && instruction.reference_id.is_none()
        && instruction.geometry1_id == Some(reference_id)
        && instruction.geometry2_id.is_none()
        && instruction.member1 == 0
        && instruction.member2 == 0)
        .then_some(())?;
    let datum_tables = sources
        .geometry_tables
        .iter()
        .filter(|table| {
            table.feature_id == feature_id
                && table.kind == FeatureGeometryTableKind::DatumIds
                && table.entry_ids.as_deref() == Some(&[sketch_id])
        })
        .count();
    (datum_tables == 1).then_some(())?;
    let tables = entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .filter(|table| {
            table
                .entries
                .iter()
                .map(|entry| entry.class_id)
                .eq([204, 203, 200, 200])
        })
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let cap_id = table.entries[1].entity_id;
    let cap = plane_equation(
        cap_id,
        sources.datums,
        sources.model_planes,
        sources.outline_planes,
    )?;
    let candidates = sources
        .datums
        .iter()
        .filter_map(|datum| {
            let equation = (datum.normal, datum.offset);
            let cap_alignment = dot(equation.0, cap.0).abs();
            let reference_alignment = dot(equation.0, reference.0).abs();
            ((cap_alignment - 1.0).abs() <= 1e-12 && reference_alignment <= 1e-12)
                .then_some(equation)
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    let aligned_cap_offset = if dot(candidate.0, cap.0).is_sign_negative() {
        -cap.1
    } else {
        cap.1
    };
    let separation = (candidate.1 - aligned_cap_offset).abs();
    let scale = candidate.1.abs().max(cap.1.abs()).max(1.0);
    (separation > 1e-12 * scale).then_some(*candidate)
}

fn circular_profile_aligned_origin(
    definition: &FeatureDefinition,
    feature_id: u32,
    sketch_plane: ([f64; 3], f64),
    u_axis: [f64; 3],
    v_axis: [f64; 3],
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Option<[f64; 3]> {
    let tables = entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .filter(|table| {
            table
                .entries
                .iter()
                .map(|entry| entry.class_id)
                .eq([204, 203, 200, 200])
        })
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let profile_external_id = table.entries[2].source_entity_id?;
    let profile_internal_id = definition
        .order_table
        .as_ref()?
        .internal_id(profile_external_id)?;
    let circles = definition
        .saved_section
        .iter()
        .flat_map(|section| &section.entities)
        .filter_map(|entity| match entity {
            crate::feature::FeatureSavedEntity::Circle(circle)
                if circle.entity_id == profile_internal_id =>
            {
                Some(circle)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let [circle] = circles.as_slice() else {
        return None;
    };
    let [Some(center_u), Some(center_v), _] = circle.center else {
        return None;
    };
    let radius = circle.radius.filter(|radius| *radius > 1e-12)?;
    let cap_id = table.entries[1].entity_id;
    let envelopes = sources
        .plane_envelopes
        .iter()
        .filter(|record| record.surface_id == cap_id)
        .collect::<Vec<_>>();
    let [envelope] = envelopes.as_slice() else {
        return None;
    };
    let corners = match &envelope.envelope {
        PlaneEnvelope::Standard { corners_3d, .. } | PlaneEnvelope::Compact { corners_3d, .. } => {
            corners_3d
        }
    };
    let corners = corners
        .iter()
        .map(|corner| Some([corner[0]?, corner[1]?, corner[2]?]))
        .collect::<Option<Vec<_>>>()?;
    let [first, second] = corners.as_slice() else {
        return None;
    };
    let axis = (0..3).find(|axis| envelope.corner_coordinate_equal[*axis] == Some(true))?;
    let radial = (0..3).filter(|index| *index != axis).collect::<Vec<_>>();
    let spans = radial
        .iter()
        .map(|index| (second[*index] - first[*index]).abs())
        .collect::<Vec<_>>();
    let tolerance_scale = spans
        .iter()
        .chain(std::iter::once(&radius))
        .copied()
        .fold(1.0, f64::max);
    (spans.len() == 2
        && spans[0] > 1e-12
        && (spans[0] - spans[1]).abs() <= 1e-9 * tolerance_scale
        && (0.5 * spans[0] - radius).abs() <= 1e-9 * tolerance_scale)
        .then_some(())?;
    let cap_center: [f64; 3] = std::array::from_fn(|index| 0.5 * (first[index] + second[index]));
    let signed_distance = dot(sketch_plane.0, cap_center) - sketch_plane.1;
    let profile_center = add(cap_center, scale(sketch_plane.0, -signed_distance));
    Some(add(
        add(profile_center, scale(u_axis, -center_u)),
        scale(v_axis, -center_v),
    ))
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
        .or_else(|| definition_local_plane_equation(definition))
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
                        .or_else(|| {
                            zero_offset_standard_section_plane_equation(
                                definition,
                                section,
                                reference_id,
                                reference,
                                sources,
                                entity_tables,
                            )
                        })
                {
                    if dot(sketch.0, reference.0).abs() < 1.0 - 1e-12
                        && !candidates.contains(&(sketch, reference))
                    {
                        candidates.push((sketch, reference));
                    }
                }
            }
        }
        if candidates.len() != 1 {
            if let Some(transform) = definition_local_frame_transform(definition, section) {
                result.push(transform);
            }
            continue;
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
        let reference_axis = scale(
            add(reference_normal, scale(normal, -cosine)),
            denominator.sqrt().recip(),
        );
        let u_axis = cross(reference_axis, normal);
        if (dot(u_axis, u_axis) - 1.0).abs() > 1e-12 {
            continue;
        }
        let sketch_factor = (sketch_offset - cosine * reference_offset) / denominator;
        let reference_factor = (reference_offset - cosine * sketch_offset) / denominator;
        let intersection_origin = add(
            scale(sketch_normal, sketch_factor),
            scale(reference_normal, reference_factor),
        );
        let origin = definition
            .owner_feature_id
            .and_then(|feature_id| {
                circular_profile_aligned_origin(
                    definition,
                    feature_id,
                    (sketch_normal, sketch_offset),
                    u_axis,
                    reference_axis,
                    sources,
                    entity_tables,
                )
            })
            .unwrap_or(intersection_origin);
        result.push(FeatureSectionTransform {
            definition_id: definition.id,
            feature_id: definition.owner_feature_id,
            origin,
            u_axis,
            v_axis: reference_axis,
            normal,
            offset: section.offset,
        });
    }
    for definition in definitions {
        if result
            .iter()
            .any(|transform| transform.definition_id == definition.id)
        {
            continue;
        }
        if let Some(transform) =
            generated_cylinder_section_transform(definition, sources, entity_tables)
                .or_else(|| generated_planar_section_transform(definition, sources, entity_tables))
        {
            result.push(transform);
        }
    }
    result.sort_by_key(|transform| transform.offset);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::{
        FeatureEntityTableEntry, FeatureParameterFrame, FeatureSection3d,
        FeatureSectionOrientation, FeatureSectionPoint, FeatureSegment, FeatureSegmentTable,
        FeatureVariableTable,
    };
    use crate::surface::{PositionalCylinderFrame, SurfaceBodyBoundary, SurfaceParameterRecord};

    #[test]
    fn normalization_rejects_overflowed_feature_frame_vectors() {
        assert_eq!(normalize([f64::MAX, f64::MAX, 0.0]), None);
        let normalized = normalize([0.0, 3.0, 4.0]).expect("finite vector");
        assert!(normalized[0].abs() < 1e-12);
        assert!((normalized[1] - 0.6).abs() < 1e-12);
        assert!((normalized[2] - 0.8).abs() < 1e-12);
    }

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

    fn blank_definition() -> FeatureDefinition {
        FeatureDefinition {
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
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        }
    }

    #[test]
    fn unique_local_system_supplies_section_plane_equation() {
        let mut definition = blank_definition();
        definition.parameter_frames = vec![FeatureParameterFrame {
            kind: FeatureParameterFrameKind::LocalSystem,
            body: Vec::new(),
            decoded_values: Some(vec![
                0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 3.0, 4.0, 5.0,
            ]),
            offset: 1,
        }];

        assert_eq!(
            definition_local_plane_equation(&definition),
            Some(([1.0, 0.0, 0.0], 3.0))
        );

        definition.parameter_frames.push(FeatureParameterFrame {
            kind: FeatureParameterFrameKind::LocalSystem,
            body: Vec::new(),
            decoded_values: Some(vec![0.0; 12]),
            offset: 2,
        });
        assert_eq!(definition_local_plane_equation(&definition), None);
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
                    surface_parameters: &[],
                    geometry_tables: &[],
                    affected_ids: &[],
                },
                &[],
            ),
            vec![FeatureSectionTransform {
                definition_id: 42,
                feature_id: Some(42),
                origin: [2.0, 0.0, 3.0],
                u_axis: [0.0, 1.0, 0.0],
                v_axis: [0.0, 0.0, 1.0],
                normal: [1.0, 0.0, 0.0],
                offset: 100,
            }]
        );
    }

    #[test]
    fn resolves_section_from_complete_local_frame_when_references_are_unresolved() {
        let mut definition = blank_definition();
        definition.parameter_frames = vec![FeatureParameterFrame {
            kind: FeatureParameterFrameKind::LocalSystem,
            body: Vec::new(),
            decoded_values: Some(vec![
                0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -3.0, -4.0, 0.0,
            ]),
            offset: 1,
        }];
        definition.section_3d = Some(FeatureSection3d {
            sketch_plane_entity_id: Some(348),
            sketch_plane_flip: Some(BinaryFlag::Clear),
            reference_plane_entity_ids: vec![2, 274],
            reference_plane_datum_geometry_id: None,
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 100,
        });

        assert_eq!(
            resolve(
                &[definition],
                &PlacementSources {
                    datums: &[],
                    surface_rows: &[],
                    model_planes: &[],
                    outline_planes: &[],
                    plane_envelopes: &[],
                    surface_parameters: &[],
                    geometry_tables: &[],
                    affected_ids: &[],
                },
                &[],
            ),
            vec![FeatureSectionTransform {
                definition_id: 42,
                feature_id: Some(42),
                origin: [-3.0, -4.0, 0.0],
                u_axis: [0.0, 1.0, 0.0],
                v_axis: [0.0, 0.0, 1.0],
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
                table_class_id: 80,
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
                table_class_id: 80,
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
                    surface_parameters: &[],
                    geometry_tables: &geometry_tables,
                    affected_ids: &[],
                },
                &entity_tables,
            ),
            vec![FeatureSectionTransform {
                definition_id: 917,
                feature_id: Some(40),
                origin: [0.0, 0.0, 0.0],
                u_axis: [0.0, 0.0, 1.0],
                v_axis: [1.0, 0.0, 0.0],
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
                        u: Some(1.0),
                        v: Some(0.0),
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
                opaque_rows: Vec::new(),
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
            &[source.clone(), dependent.clone()],
            &PlacementSources {
                datums: &[
                    datum(2, [1.0, 0.0, 0.0], 0.0),
                    datum(4, [0.0, 0.0, 1.0], 0.0),
                    datum(799, [0.0, 1.0, 0.0], 1.0),
                ],
                surface_rows: std::slice::from_ref(&generated_plane),
                model_planes: &[],
                outline_planes: &[],
                plane_envelopes: &[],
                surface_parameters: &[],
                geometry_tables: &[],
                affected_ids: &[],
            },
            &[],
        );

        assert_eq!(transforms.len(), 2);
        assert_eq!(transforms[1].definition_id, 579);
        assert_eq!(transforms[1].feature_id, Some(579));
        assert_eq!(transforms[1].origin, [0.0, 1.0, 0.0]);
        assert_eq!(transforms[1].u_axis, [1.0, 0.0, 0.0]);
        assert_eq!(transforms[1].v_axis, [0.0, 0.0, -1.0]);
        assert_eq!(transforms[1].normal, [0.0, 1.0, 0.0]);

        let duplicate_plane = SurfaceRow {
            offset: 51,
            ..generated_plane
        };
        let ambiguous = resolve(
            &[source, dependent],
            &PlacementSources {
                datums: &[
                    datum(2, [1.0, 0.0, 0.0], 0.0),
                    datum(4, [0.0, 0.0, 1.0], 0.0),
                    datum(799, [0.0, 1.0, 0.0], 1.0),
                ],
                surface_rows: &[generated_plane, duplicate_plane],
                model_planes: &[],
                outline_planes: &[],
                plane_envelopes: &[],
                surface_parameters: &[],
                geometry_tables: &[],
                affected_ids: &[],
            },
            &[],
        );
        assert_eq!(ambiguous.len(), 1);
        assert_eq!(ambiguous[0].definition_id, 917);
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
                surface_parameters: &[],
                geometry_tables: &[],
                affected_ids: &[],
            },
            &[],
        );
        assert_eq!(transforms.len(), 1);
        assert_eq!(transforms[0].origin, [2.0, 0.0, 3.0]);
        assert_eq!(transforms[0].u_axis, [0.0, 1.0, 0.0]);
        assert_eq!(transforms[0].v_axis, [0.0, 0.0, 1.0]);
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
                surface_parameters: &[],
                geometry_tables: &[geometry_table],
                affected_ids: &[parents],
            },
            &[],
        );
        assert_eq!(transforms.len(), 1);
        assert_eq!(transforms[0].normal, [0.0, -1.0, 0.0]);
        assert_eq!(transforms[0].u_axis, [0.0, 0.0, -1.0]);
        assert_eq!(transforms[0].v_axis, [1.0, 0.0, 0.0]);
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
            scalar_tokens: Vec::new(),
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
                surface_parameters: &[],
                geometry_tables: &[geometry_table],
                affected_ids: &[parents],
            },
            &[],
        );
        assert_eq!(transforms.len(), 1);
        assert_eq!(transforms[0].origin, [0.0, 0.0, 3.0]);
        assert_eq!(transforms[0].normal, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn resolves_section_frame_from_two_generated_arc_cylinders() {
        let segment = |external_id, center_id| FeatureSegment {
            kind: FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [0; 2],
            center_id: Some(center_id),
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            offset: external_id as usize,
        };
        let definition = FeatureDefinition {
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
                        point_id: 1,
                        u: Some(-12.5),
                        v: Some(0.0),
                    },
                    FeatureSectionPoint {
                        point_id: 2,
                        u: Some(12.5),
                        v: Some(0.0),
                    },
                ],
                offset: 100,
            }),
            segments: Some(FeatureSegmentTable {
                declared_count: 2,
                entity_ref: None,
                rows: vec![segment(252, 1), segment(255, 2)],
                opaque_rows: Vec::new(),
                offset: 110,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 90,
        };
        let rows = [
            SurfaceRow {
                id: 819,
                type_byte: 0x24,
                kind: SurfaceKind::Cylinder,
                feature_id: 40,
                reversed: false,
                boundary_type: 0,
                next_surface: 0,
                offset: 200,
            },
            SurfaceRow {
                id: 822,
                type_byte: 0x24,
                kind: SurfaceKind::Cylinder,
                feature_id: 40,
                reversed: false,
                boundary_type: 0,
                next_surface: 0,
                offset: 220,
            },
        ];
        let parameters = |surface_id, origin, offset| SurfaceParameterRecord {
            surface_id,
            body: Vec::new(),
            scalar_values: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            terminal_scalar_frame: None,
            tabulated_cylinder_frame: None,
            positional_cylinder_frame: Some(PositionalCylinderFrame {
                origin,
                axis: [0.0, 1.0, 0.0],
                ref_direction: [1.0, 0.0, 0.0],
                radius: 0.75,
                length: Some(34.0),
            }),
            split_cylinder_outline_bounds: None,
            positional_cone_frame: None,
            positional_torus_frame: None,
            boundary: SurfaceBodyBoundary::CompoundClose,
            offset,
            body_offset: offset + 1,
        };
        let parameters = [
            parameters(819, [-12.5, 4.0, 0.0], 200),
            parameters(822, [12.5, 4.0, 0.0], 220),
        ];
        let entry = |entity_id, source_entity_id, offset| FeatureEntityTableEntry {
            entity_id,
            class_id: 200,
            source_entity_id: Some(source_entity_id),
            prefixed: false,
            offset,
            end_offset: offset + 1,
        };
        let tables = [FeatureEntityTable {
            feature_id: Some(40),
            table_class_id: 2,
            entry_ids: vec![819, 822],
            entries: vec![entry(819, 252, 300), entry(822, 255, 310)],
            surface_ids: vec![819, 822],
            non_surface_entity_ids: Vec::new(),
            offset: 290,
        }];
        let sources = PlacementSources {
            datums: &[],
            surface_rows: &rows,
            model_planes: &[],
            outline_planes: &[],
            plane_envelopes: &[],
            surface_parameters: &parameters,
            geometry_tables: &[],
            affected_ids: &[],
        };

        assert_eq!(
            generated_cylinder_section_transform(&definition, &sources, &tables),
            Some(FeatureSectionTransform {
                definition_id: 917,
                feature_id: Some(40),
                origin: [0.0, 4.0, 0.0],
                u_axis: [1.0, 0.0, 0.0],
                v_axis: [0.0, 0.0, -1.0],
                normal: [0.0, 1.0, 0.0],
                offset: 200,
            })
        );

        let mut far_divergent = parameters.clone();
        for record in &mut far_divergent {
            record
                .positional_cylinder_frame
                .as_mut()
                .expect("cylinder frame")
                .origin[0] += 1.0e12;
        }
        far_divergent[1]
            .positional_cylinder_frame
            .as_mut()
            .expect("second cylinder frame")
            .axis = [0.1, 0.99_f64.sqrt(), 0.0];
        let divergent_sources = PlacementSources {
            surface_parameters: &far_divergent,
            ..sources
        };
        assert!(
            generated_cylinder_section_transform(&definition, &divergent_sources, &tables)
                .is_none()
        );
    }

    #[test]
    fn resolves_section_frame_from_complete_generated_planar_prism() {
        let line = |external_id, point_ids| FeatureSegment {
            kind: FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids,
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            offset: external_id as usize,
        };
        let definition = FeatureDefinition {
            id: 917,
            owner_feature_id: Some(10),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(FeatureVariableTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                points: vec![
                    FeatureSectionPoint {
                        point_id: 1,
                        u: Some(-20.0),
                        v: Some(-6.0),
                    },
                    FeatureSectionPoint {
                        point_id: 2,
                        u: Some(20.0),
                        v: Some(-6.0),
                    },
                    FeatureSectionPoint {
                        point_id: 3,
                        u: Some(20.0),
                        v: Some(6.0),
                    },
                    FeatureSectionPoint {
                        point_id: 4,
                        u: Some(-20.0),
                        v: Some(6.0),
                    },
                ],
                offset: 100,
            }),
            segments: Some(FeatureSegmentTable {
                declared_count: 4,
                entity_ref: None,
                rows: vec![
                    line(4, [1, 2]),
                    line(5, [2, 3]),
                    line(6, [3, 4]),
                    line(7, [4, 1]),
                ],
                opaque_rows: Vec::new(),
                offset: 110,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 90,
        };
        let outline = |surface_id, origin, normal| OutlinePlane {
            surface_id,
            origin,
            normal,
            u_axis: if normal[0] == 1.0 {
                [0.0, 1.0, 0.0]
            } else {
                [1.0, 0.0, 0.0]
            },
            offset: surface_id as usize,
        };
        let outlines = [
            outline(13, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            outline(18, [0.0, 48.0, 0.0], [0.0, 1.0, 0.0]),
            outline(23, [0.0, 0.0, 6.0], [0.0, 0.0, 1.0]),
            outline(25, [20.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            outline(27, [0.0, 0.0, -6.0], [0.0, 0.0, 1.0]),
            outline(29, [-20.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        ];
        let entry = |entity_id, class_id, source_entity_id| FeatureEntityTableEntry {
            entity_id,
            class_id,
            source_entity_id,
            prefixed: false,
            offset: entity_id as usize,
            end_offset: entity_id as usize + 1,
        };
        let tables = [FeatureEntityTable {
            feature_id: Some(10),
            table_class_id: 79,
            entry_ids: vec![13, 18, 23, 25, 27, 29],
            entries: vec![
                entry(13, 204, None),
                entry(18, 203, None),
                entry(23, 200, Some(4)),
                entry(25, 200, Some(5)),
                entry(27, 200, Some(6)),
                entry(29, 200, Some(7)),
            ],
            surface_ids: vec![13, 18, 23, 25, 27, 29],
            non_surface_entity_ids: Vec::new(),
            offset: 200,
        }];
        let sources = PlacementSources {
            datums: &[],
            surface_rows: &[],
            model_planes: &[],
            outline_planes: &outlines,
            plane_envelopes: &[],
            surface_parameters: &[],
            geometry_tables: &[],
            affected_ids: &[],
        };

        assert_eq!(
            generated_planar_section_transform(&definition, &sources, &tables),
            Some(FeatureSectionTransform {
                definition_id: 917,
                feature_id: Some(10),
                origin: [0.0, 0.0, 0.0],
                u_axis: [1.0, 0.0, 0.0],
                v_axis: [0.0, 0.0, -1.0],
                normal: [-0.0, 1.0, 0.0],
                offset: 200,
            })
        );
    }
}
