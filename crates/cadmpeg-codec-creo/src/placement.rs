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
use crate::vecmath::{add, cross, dot, normalize, scale};

const EPS_FRAME_AGREEMENT: f64 = 1.0e-9;
const EPS_VECTOR_NONZERO: f64 = 1.0e-12;
const EPS_FRAME_DETERMINANT: f64 = 1.0e-9;
const EPS_FRAME_ORTHO: f64 = 1.0e-12;
const EPS_PLANE_SEPARATION: f64 = 1.0e-12;
const EPS_RADIUS_NONZERO: f64 = 1.0e-12;
const EPS_SPAN_AGREEMENT: f64 = 1.0e-9;
const EPS_AXIS_ALIGNMENT: f64 = 1.0e-12;
const EPS_FRAME_DENOMINATOR: f64 = 1.0e-12;

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

type PlaneEquation = ([f64; 3], f64);

#[derive(Debug, Clone, Copy, PartialEq)]
struct SectionFrameCandidate {
    reference_id: u32,
    sketch: PlaneEquation,
    reference: PlaneEquation,
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
    for (_, entry) in entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .flat_map(|table| table.entries.iter().map(move |entry| (table, entry)))
        .filter(|(table, entry)| {
            entry.class_id == 200 && table.surface_ids.contains(&entry.entity_id)
        })
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
    let close = |left: f64, right: f64| {
        (left - right).abs() <= EPS_FRAME_AGREEMENT * left.abs().max(right.abs()).max(1.0)
    };
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
                && table
                    .entries
                    .iter()
                    .all(|entry| table.surface_ids.contains(&entry.entity_id))
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
        (length.is_finite() && length > EPS_VECTOR_NONZERO).then_some(())?;
        let local_normal = [direction[1] / length, -direction[0] / length];
        let local_offset = local_normal[0].mul_add(start[0], local_normal[1] * start[1]);
        let (model_normal, model_offset) = generated_plane_equation(entry)?;
        let magnitude = dot(model_normal, model_normal).sqrt();
        (magnitude.is_finite() && magnitude > EPS_VECTOR_NONZERO).then_some(())?;
        sides.push((
            local_normal,
            local_offset,
            scale(model_normal, magnitude.recip()),
            model_offset / magnitude,
        ));
    }

    let close = |left: f64, right: f64| {
        (left - right).abs() <= EPS_FRAME_AGREEMENT * left.abs().max(right.abs()).max(1.0)
    };
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
            if determinant.abs() <= EPS_FRAME_DETERMINANT {
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
    let model_planes = model_planes
        .iter()
        .filter(|plane| plane.surface_id == id)
        .collect::<Vec<_>>();
    let model_equation = match model_planes.as_slice() {
        [plane] => plane
            .normal
            .zip(plane.origin)
            .map(|(normal, origin)| (normal, dot(normal, origin))),
        _ => None,
    };
    let outline_planes = outline_planes
        .iter()
        .filter(|plane| plane.surface_id == id)
        .collect::<Vec<_>>();
    let outline_equation = match outline_planes.as_slice() {
        [plane] => Some((plane.normal, dot(plane.normal, plane.origin))),
        _ => None,
    };
    // The datum-geometry and model-surface identifiers are separate namespaces;
    // a numeric collision supplies no rule for choosing between their equations.
    if datums.len() == 1 && (model_equation.is_some() || outline_equation.is_some()) {
        return None;
    }
    if let [datum] = datums.as_slice() {
        return Some((datum.normal, datum.offset));
    }
    if !datums.is_empty() {
        return None;
    }
    if let Some(equation) = model_equation {
        return Some(equation);
    }
    if model_planes.len() > 1 {
        return None;
    }
    outline_equation
}

fn definition_local_plane_equation(definition: &FeatureDefinition) -> Option<([f64; 3], f64)> {
    let values = unique_complete_local_system(definition)?;
    let raw_normal: [f64; 3] = values[6..9].try_into().ok()?;
    let normal = normalize(raw_normal)?;
    let origin: [f64; 3] = values[9..12].try_into().ok()?;
    Some((normal, dot(normal, origin)))
}

pub(crate) fn unique_complete_local_system(definition: &FeatureDefinition) -> Option<[f64; 12]> {
    let mut frames = definition
        .parameter_frames
        .iter()
        .filter(|frame| frame.kind == FeatureParameterFrameKind::LocalSystem)
        .filter_map(|frame| frame.decoded_values.as_deref())
        .filter_map(|values| <&[f64; 12]>::try_from(values).ok());
    let values = *frames.next()?;
    frames.next().is_none().then_some(values)
}

fn reference_flip_for_reference(
    section: &crate::feature::FeatureSection3d,
    reference_id: Option<u32>,
) -> Option<BinaryFlag> {
    if section.reference_plane_rows.is_empty() {
        return section.orientation.reference_flip;
    }
    let reference_id = reference_id?;
    section
        .reference_plane_rows
        .iter()
        .find(|row| row.plane_entity_id == reference_id)
        .and_then(|row| row.reference_flip)
}

fn definition_local_frame_transform(
    definition: &FeatureDefinition,
    section: &crate::feature::FeatureSection3d,
) -> Option<FeatureSectionTransform> {
    let feature_id = definition.owner_feature_id?;
    let values = unique_complete_local_system(definition)?;
    let mut reference_axis = normalize(values[0..3].try_into().ok()?)?;
    let mut normal = normalize(values[6..9].try_into().ok()?)?;
    (dot(reference_axis, normal).abs() <= EPS_FRAME_ORTHO).then_some(())?;
    let origin: [f64; 3] = values[9..12].try_into().ok()?;
    if section.sketch_plane_flip == Some(BinaryFlag::Set) {
        normal = scale(normal, -1.0);
    }
    if section.orientation.section_flip == Some(BinaryFlag::Set) {
        normal = scale(normal, -1.0);
    }
    if reference_flip_for_reference(section, None) == Some(BinaryFlag::Set) {
        reference_axis = scale(reference_axis, -1.0);
    }
    let u_axis = cross(reference_axis, normal);
    ((dot(u_axis, u_axis) - 1.0).abs() <= EPS_FRAME_ORTHO).then_some(FeatureSectionTransform {
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
                .filter(|(normal, _)| dot(*normal, reference_normal).abs() <= EPS_FRAME_ORTHO)
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
    (magnitude > EPS_VECTOR_NONZERO).then_some(())?;
    let direction = scale(direction, magnitude.recip());
    let normal = cross(direction, transform.normal);
    let magnitude = dot(normal, normal).sqrt();
    (magnitude > EPS_VECTOR_NONZERO).then_some(())?;
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
    ((cosine - 1.0).abs() <= EPS_AXIS_ALIGNMENT
        && (first.1 - second_offset).abs() > EPS_PLANE_SEPARATION * scale)
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
            ((cap_alignment - 1.0).abs() <= EPS_AXIS_ALIGNMENT
                && reference_alignment <= EPS_AXIS_ALIGNMENT)
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
    (separation > EPS_PLANE_SEPARATION * scale).then_some(*candidate)
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
    let radius = circle
        .radius
        .filter(|radius| *radius > EPS_RADIUS_NONZERO)?;
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
        && spans[0] > EPS_VECTOR_NONZERO
        && (spans[0] - spans[1]).abs() <= EPS_SPAN_AGREEMENT * tolerance_scale
        && (0.5 * spans[0] - radius).abs() <= EPS_SPAN_AGREEMENT * tolerance_scale)
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
        let mut candidates = Vec::<SectionFrameCandidate>::new();
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
                    if dot(sketch.0, reference.0).abs() < 1.0 - EPS_AXIS_ALIGNMENT
                        && !candidates.iter().any(|candidate| {
                            candidate.sketch == sketch && candidate.reference == reference
                        })
                    {
                        candidates.push(SectionFrameCandidate {
                            reference_id,
                            sketch,
                            reference,
                        });
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
                    if dot(sketch.0, reference.0).abs() < 1.0 - EPS_AXIS_ALIGNMENT
                        && !candidates.iter().any(|candidate| {
                            candidate.sketch == sketch && candidate.reference == reference
                        })
                    {
                        candidates.push(SectionFrameCandidate {
                            reference_id,
                            sketch,
                            reference,
                        });
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
        let [candidate] = candidates.as_slice() else {
            continue;
        };
        let (mut sketch_normal, mut sketch_offset) = candidate.sketch;
        let (mut reference_normal, mut reference_offset) = candidate.reference;
        if section.sketch_plane_flip == Some(BinaryFlag::Set) {
            sketch_normal = scale(sketch_normal, -1.0);
            sketch_offset = -sketch_offset;
        }
        if section.orientation.section_flip == Some(BinaryFlag::Set) {
            sketch_normal = scale(sketch_normal, -1.0);
            sketch_offset = -sketch_offset;
        }
        if reference_flip_for_reference(section, Some(candidate.reference_id))
            == Some(BinaryFlag::Set)
        {
            reference_normal = scale(reference_normal, -1.0);
            reference_offset = -reference_offset;
        }
        let normal = sketch_normal;
        let cosine = dot(normal, reference_normal);
        let denominator = 1.0 - cosine * cosine;
        if denominator <= EPS_FRAME_DENOMINATOR {
            continue;
        }
        let reference_axis = scale(
            add(reference_normal, scale(normal, -cosine)),
            denominator.sqrt().recip(),
        );
        let u_axis = cross(reference_axis, normal);
        if (dot(u_axis, u_axis) - 1.0).abs() > EPS_FRAME_ORTHO {
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
mod tests;
