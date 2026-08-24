// SPDX-License-Identifier: Apache-2.0
//! Section geometry conversion from live and saved section entities.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::features::{Angle, Length};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntityUse, SketchGeometry, SketchId};

use super::super::sketch_ids::sketch_entity_id;
use super::super::sketch_transfer::{
    section_saved_entity, semantic_saved_section_entities, unique_saved_section_internal_ids,
};
use super::radii::trim_segment_id;
use super::skamp::section_line_entity_fixed_coordinate;

const EPS_POINT_NONZERO: f64 = 1.0e-12;
const EPS_RADIUS_AGREEMENT: f64 = 1.0e-9;
const EPS_DENOMINATOR_NONZERO: f64 = 1.0e-12;
const EPS_FRAME_ORTHONORMAL: f64 = 1.0e-9;
const EPS_PARAMETER_AGREEMENT: f64 = 1.0e-9;
const EPS_PARAMETER_FULL_TURN: f64 = 1.0e-9;
const EPS_ANGLE_FULL_TURN: f64 = 1.0e-12;

pub(crate) fn section_line_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    let start = points.get(&segment.point_ids[0])?;
    let end = points.get(&segment.point_ids[1])?;
    let scale = start
        .iter()
        .chain(end)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (((end[0] - start[0]) / scale).hypot((end[1] - start[1]) / scale) > EPS_POINT_NONZERO)
        .then_some(())?;
    Some(SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
        end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
    })
}

pub(crate) fn section_point_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Point).then_some(())?;
    let position = points.get(&segment.point_ids[0])?;
    Some(SketchGeometry::Point {
        position: cadmpeg_ir::math::Point2::new(position[0], position[1]),
    })
}

pub(crate) fn section_arc_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Arc && segment.arc_orientation == Some(0))
        .then_some(())?;
    let center = points.get(&segment.center_id?)?;
    let first = points.get(&segment.point_ids[0])?;
    let second = points.get(&segment.point_ids[1])?;
    let offset = |point: &[f64; 2]| [point[0] - center[0], point[1] - center[1]];
    let first_offset = offset(first);
    let second_offset = offset(second);
    let first_radius = first_offset[0].hypot(first_offset[1]);
    let second_radius = second_offset[0].hypot(second_offset[1]);
    let scale = first_radius.max(second_radius).max(1.0);
    if first_radius <= EPS_POINT_NONZERO
        || (first_radius - second_radius).abs() > EPS_RADIUS_AGREEMENT * scale
    {
        return None;
    }
    let start = second_offset[1].atan2(second_offset[0]);
    let mut end = first_offset[1].atan2(first_offset[0]);
    while end <= start {
        end += std::f64::consts::TAU;
    }
    Some(SketchGeometry::Arc {
        center: cadmpeg_ir::math::Point2::new(center[0], center[1]),
        radius: Length(first_radius),
        start_angle: Angle(start),
        end_angle: Angle(end),
    })
}

pub(crate) fn section_circle_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    radii: &BTreeMap<u32, f64>,
    segment: &crate::feature::FeatureCircleSegment,
) -> Option<SketchGeometry> {
    let center = points.get(&segment.center_id)?;
    let radius = *radii.get(&segment.radius_ref)?;
    Some(SketchGeometry::Circle {
        center: Point2::new(center[0], center[1]),
        radius: Length(radius),
    })
}

pub(crate) fn section_point_row_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeaturePointSegment,
) -> Option<SketchGeometry> {
    let point = points.get(&segment.point_id)?;
    Some(SketchGeometry::Point {
        position: Point2::new(point[0], point[1]),
    })
}

pub(crate) fn section_centered_line_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureCenteredLineSegment,
) -> Option<SketchGeometry> {
    let start = points.get(&0)?;
    let end = points.get(&1)?;
    let center = points.get(&segment.center_id)?;
    let scale = start
        .iter()
        .chain(end)
        .chain(center)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    ((end[0] - start[0]).hypot(end[1] - start[1]) > EPS_POINT_NONZERO * scale).then_some(())?;
    ((start[0] + end[0] - 2.0 * center[0]).hypot(start[1] + end[1] - 2.0 * center[1])
        <= EPS_RADIUS_AGREEMENT * scale)
        .then_some(())?;
    Some(SketchGeometry::Line {
        start: Point2::new(start[0], start[1]),
        end: Point2::new(end[0], end[1]),
    })
}

pub(crate) fn section_reference_line_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureReferenceLineSegment,
) -> Option<SketchGeometry> {
    let [Some(start_id), Some(end_id)] = segment.point_ids else {
        return None;
    };
    let start = points.get(&start_id)?;
    let end = points.get(&end_id)?;
    let scale = start
        .iter()
        .chain(end)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let direction = [end[0] - start[0], end[1] - start[1]];
    (direction[0].hypot(direction[1]) > EPS_POINT_NONZERO * scale).then_some(())?;
    Some(SketchGeometry::ReferenceLine {
        origin: Point2::new(start[0], start[1]),
        direction: Point2::new(direction[0], direction[1]),
    })
}

pub(crate) fn resolved_section_reference_line_geometry(
    definition: &crate::feature::FeatureDefinition,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureReferenceLineSegment,
) -> Option<SketchGeometry> {
    if let Some(geometry) = section_reference_line_geometry(points, segment) {
        return Some(geometry);
    }
    let [Some(start_id), Some(end_id)] = segment.point_ids else {
        return None;
    };
    let fixed_coordinate = section_line_entity_fixed_coordinate(definition, segment.external_id)?;
    let [Some(first), Some(second)] =
        [start_id, end_id].map(|point| variable_points.get(&point)?[fixed_coordinate])
    else {
        return None;
    };
    let scale = first.abs().max(second.abs()).max(1.0);
    ((first - second).abs() <= EPS_PARAMETER_AGREEMENT * scale).then(|| {
        if fixed_coordinate == 0 {
            SketchGeometry::ReferenceLine {
                origin: Point2::new(first, 0.0),
                direction: Point2::new(0.0, 1.0),
            }
        } else {
            SketchGeometry::ReferenceLine {
                origin: Point2::new(0.0, first),
                direction: Point2::new(1.0, 0.0),
            }
        }
    })
}

pub(crate) fn section_segment_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    section_line_geometry(points, segment)
        .or_else(|| section_arc_geometry(points, segment))
        .or_else(|| section_point_geometry(points, segment))
}

pub(crate) fn saved_section_line_geometry(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    let order_table = definition.order_table.as_ref()?;
    let internal_id = order_table
        .internal_id(segment.external_id)
        .or_else(|| {
            let segment_table = definition.segments.as_ref()?;
            segment_table.is_complete().then_some(())?;
            let segments = &segment_table.rows;
            let position = segments
                .iter()
                .position(|candidate| candidate.external_id == segment.external_id)?;
            let previous = segments[..position]
                .iter()
                .rev()
                .find_map(|candidate| order_table.internal_id(candidate.external_id))?;
            let next = segments[position + 1..]
                .iter()
                .find_map(|candidate| order_table.internal_id(candidate.external_id))?;
            let internal_id = previous.checked_add(1)?;
            (next == internal_id.checked_add(1)?
                && semantic_saved_section_entities(definition).any(|entity| {
                    matches!(entity, crate::feature::FeatureSavedEntity::Line(line) if line.entity_id == internal_id)
                }))
            .then_some(internal_id)
        })
        .or_else(|| {
            order_table.is_complete().then_some(())?;
            let trimmed = definition.trim_entities.as_ref()?;
            (trimmed.has_complete_bucket_frame() && trimmed.has_unique_external_ids())
                .then_some(())?;
            let segment_table = definition.segments.as_ref()?;
            segment_table.is_complete().then_some(())?;
            let trimmed_external_ids = trimmed
                .rows
                .iter()
                .filter_map(|row| trim_segment_id(definition, row))
                .collect::<BTreeSet<_>>();
            let ordered_external_ids = order_table
                .rows
                .iter()
                .map(|row| row.external_id)
                .collect::<BTreeSet<_>>();
            let ordered_internal_ids = order_table
                .rows
                .iter()
                .map(|row| row.internal_id)
                .collect::<BTreeSet<_>>();
            let segment_ids = segment_table
                .rows
                .iter()
                .filter(|candidate| {
                    candidate.kind == crate::feature::FeatureSegmentKind::Line
                        && trimmed_external_ids.contains(&candidate.external_id)
                        && !ordered_external_ids.contains(&candidate.external_id)
                })
                .map(|candidate| candidate.external_id)
                .collect::<Vec<_>>();
            let saved_ids = semantic_saved_section_entities(definition)
                .filter_map(|entity| match entity {
                    crate::feature::FeatureSavedEntity::Line(line)
                        if !ordered_internal_ids.contains(&line.entity_id) =>
                    {
                        Some(line.entity_id)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            match (segment_ids.as_slice(), saved_ids.as_slice()) {
                ([external_id], [internal_id]) if *external_id == segment.external_id => {
                    Some(*internal_id)
                }
                _ => None,
            }
        });
    let internal_id = internal_id?;
    unique_saved_section_internal_ids(definition)
        .contains(&internal_id)
        .then_some(())?;
    let line = semantic_saved_section_entities(definition).find_map(|entity| match entity {
        crate::feature::FeatureSavedEntity::Line(line) if line.entity_id == internal_id => {
            Some(line)
        }
        _ => None,
    })?;
    let [[Some(start_u), Some(start_v), _], [Some(end_u), Some(end_v), _]] = line.endpoints else {
        return None;
    };
    Some(SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(start_u, start_v),
        end: cadmpeg_ir::math::Point2::new(end_u, end_v),
    })
}

pub(crate) fn saved_section_arc_record<'a>(
    definition: &'a crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<&'a crate::feature::FeatureSavedArc> {
    (segment.kind == crate::feature::FeatureSegmentKind::Arc && segment.arc_orientation == Some(0))
        .then_some(())?;
    let internal_id = definition
        .order_table
        .as_ref()?
        .internal_id(segment.external_id)?;
    unique_saved_section_internal_ids(definition)
        .contains(&internal_id)
        .then_some(())?;
    semantic_saved_section_entities(definition).find_map(|entity| match entity {
        crate::feature::FeatureSavedEntity::Arc(arc) if arc.entity_id == internal_id => Some(arc),
        _ => None,
    })
}

pub(crate) fn saved_section_arc_carrier(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<([f64; 2], f64)> {
    let arc = saved_section_arc_record(definition, segment)?;
    let [center_u, center_v, _] = arc.center;
    if let ([Some(center_u), Some(center_v)], Some(radius)) = (
        [center_u, center_v],
        arc.radius.filter(|radius| *radius > EPS_POINT_NONZERO),
    ) {
        return Some(([center_u, center_v], radius));
    }
    let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] = arc.endpoints
    else {
        return None;
    };
    let scale = [first_u, first_v, second_u, second_v]
        .into_iter()
        .map(f64::abs)
        .fold(1.0, f64::max);
    let [center_u, center_v] = match [center_u, center_v] {
        [Some(u), Some(v)] => [u, v],
        [Some(u), None] => {
            let denominator = 2.0 * (second_v - first_v);
            if denominator.abs() <= EPS_DENOMINATOR_NONZERO * scale {
                return None;
            }
            let v = ((second_u - u).mul_add(
                second_u - u,
                second_v * second_v - (first_u - u) * (first_u - u) - first_v * first_v,
            )) / denominator;
            [u, v]
        }
        [None, Some(v)] => {
            let denominator = 2.0 * (second_u - first_u);
            if denominator.abs() <= EPS_DENOMINATOR_NONZERO * scale {
                return None;
            }
            let u = ((second_v - v).mul_add(
                second_v - v,
                second_u * second_u - (first_v - v) * (first_v - v) - first_u * first_u,
            )) / denominator;
            [u, v]
        }
        [None, None] => return None,
    };
    let first_radius = (first_u - center_u).hypot(first_v - center_v);
    let second_radius = (second_u - center_u).hypot(second_v - center_v);
    let radial_scale = first_radius.max(second_radius).max(1.0);
    if first_radius <= EPS_POINT_NONZERO
        || (first_radius - second_radius).abs() > EPS_RADIUS_AGREEMENT * radial_scale
        || arc.radius.is_some_and(|stored| {
            (stored - first_radius).abs() > EPS_RADIUS_AGREEMENT * stored.max(first_radius).max(1.0)
        })
    {
        return None;
    }
    let radius = arc.radius.unwrap_or(first_radius);
    Some(([center_u, center_v], radius))
}

pub(crate) fn saved_section_arc_geometry(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    let arc = saved_section_arc_record(definition, segment)?;
    let ([center_u, center_v], radius) = saved_section_arc_carrier(definition, segment)?;
    let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] = arc.endpoints
    else {
        return None;
    };
    let first = [first_u - center_u, first_v - center_v];
    let second = [second_u - center_u, second_v - center_v];
    let first_radius = first[0].hypot(first[1]);
    let second_radius = second[0].hypot(second[1]);
    let scale = radius.max(first_radius).max(second_radius).max(1.0);
    if (first_radius - radius).abs() > EPS_RADIUS_AGREEMENT * scale
        || (second_radius - radius).abs() > EPS_RADIUS_AGREEMENT * scale
    {
        return None;
    }
    let start = second[1].atan2(second[0]);
    let mut end = first[1].atan2(first[0]);
    while end <= start {
        end += std::f64::consts::TAU;
    }
    Some(SketchGeometry::Arc {
        center: cadmpeg_ir::math::Point2::new(center_u, center_v),
        radius: Length(radius),
        start_angle: Angle(start),
        end_angle: Angle(end),
    })
}

pub(crate) fn saved_section_segment_point_coordinates(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<Vec<(u32, [f64; 2])>> {
    match segment.kind {
        crate::feature::FeatureSegmentKind::Line => {
            let geometry = saved_section_line_geometry(definition, segment)?;
            let [start, end] = saved_geometry_endpoints(&geometry)?;
            Some(vec![
                (segment.point_ids[0], start),
                (segment.point_ids[1], end),
            ])
        }
        crate::feature::FeatureSegmentKind::Arc => {
            let SketchGeometry::Arc { center, .. } =
                saved_section_arc_geometry(definition, segment)?
            else {
                unreachable!();
            };
            let arc = saved_section_arc_record(definition, segment)?;
            let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] =
                arc.endpoints
            else {
                return None;
            };
            Some(vec![
                (segment.point_ids[0], [first_u, first_v]),
                (segment.point_ids[1], [second_u, second_v]),
                (segment.center_id?, [center.u, center.v]),
            ])
        }
        crate::feature::FeatureSegmentKind::Point => None,
    }
}

pub(crate) fn saved_section_circle_values(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureCircleSegment,
) -> Option<([f64; 2], f64)> {
    let segments = definition.segments.as_ref()?;
    (segments.is_complete() && segments.external_id_count(segment.external_id) == 1)
        .then_some(())?;
    let entity = section_saved_entity(definition, segment.external_id)?;
    let (_, geometry, _) = saved_section_entity_geometry(entity)?;
    let SketchGeometry::Circle { center, radius } = geometry else {
        return None;
    };
    Some(([center.u, center.v], radius.0))
}

pub(crate) fn saved_section_entity_geometry(
    entity: &crate::feature::FeatureSavedEntity,
) -> Option<(u32, SketchGeometry, usize)> {
    match entity {
        crate::feature::FeatureSavedEntity::Line(line) => {
            let [[Some(start_u), Some(start_v), _], [Some(end_u), Some(end_v), _]] = line.endpoints
            else {
                return None;
            };
            Some((
                line.entity_id,
                SketchGeometry::Line {
                    start: Point2::new(start_u, start_v),
                    end: Point2::new(end_u, end_v),
                },
                line.offset,
            ))
        }
        crate::feature::FeatureSavedEntity::Arc(arc) => {
            let ([Some(center_u), Some(center_v)], Some(radius)) = (
                [arc.center[0], arc.center[1]],
                arc.radius.filter(|radius| *radius > EPS_POINT_NONZERO),
            ) else {
                return None;
            };
            let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] =
                arc.endpoints
            else {
                return None;
            };
            let first = [first_u - center_u, first_v - center_v];
            let second = [second_u - center_u, second_v - center_v];
            let scale = radius
                .max(first[0].hypot(first[1]))
                .max(second[0].hypot(second[1]))
                .max(1.0);
            if (first[0].hypot(first[1]) - radius).abs() > EPS_RADIUS_AGREEMENT * scale
                || (second[0].hypot(second[1]) - radius).abs() > EPS_RADIUS_AGREEMENT * scale
            {
                return None;
            }
            let start_angle = second[1].atan2(second[0]);
            let mut end_angle = first[1].atan2(first[0]);
            while end_angle <= start_angle {
                end_angle += std::f64::consts::TAU;
            }
            Some((
                arc.entity_id,
                SketchGeometry::Arc {
                    center: Point2::new(center_u, center_v),
                    radius: Length(radius),
                    start_angle: Angle(start_angle),
                    end_angle: Angle(end_angle),
                },
                arc.offset,
            ))
        }
        crate::feature::FeatureSavedEntity::Circle(circle) => {
            let ([Some(center_u), Some(center_v)], Some(radius)) = (
                [circle.center[0], circle.center[1]],
                circle.radius.filter(|radius| *radius > EPS_POINT_NONZERO),
            ) else {
                return None;
            };
            Some((
                circle.entity_id,
                SketchGeometry::Circle {
                    center: Point2::new(center_u, center_v),
                    radius: Length(radius),
                },
                circle.offset,
            ))
        }
        crate::feature::FeatureSavedEntity::Conic(conic) => {
            let (Some(frame), [Some(first_radius), Some(second_radius)]) =
                (conic.local_system, conic.coefficients)
            else {
                return None;
            };
            let first_axis = [frame[0], frame[1]];
            let second_axis = [frame[3], frame[4]];
            let first_length = first_axis[0].hypot(first_axis[1]);
            let second_length = second_axis[0].hypot(second_axis[1]);
            let scale = first_length.max(second_length).max(1.0);
            if !frame.into_iter().all(f64::is_finite)
                || first_radius <= EPS_POINT_NONZERO
                || second_radius <= EPS_POINT_NONZERO
                || (first_length - 1.0).abs() > EPS_FRAME_ORTHONORMAL * scale
                || (second_length - 1.0).abs() > EPS_FRAME_ORTHONORMAL * scale
                || (first_axis[0] * second_axis[0] + first_axis[1] * second_axis[1]).abs()
                    > EPS_FRAME_ORTHONORMAL
                || (first_axis[0] * second_axis[1] - first_axis[1] * second_axis[0] - 1.0).abs()
                    > EPS_FRAME_ORTHONORMAL
                || frame[2].abs() > EPS_FRAME_ORTHONORMAL
                || frame[5].abs() > EPS_FRAME_ORTHONORMAL
                || frame[6].abs() > EPS_FRAME_ORTHONORMAL
                || frame[7].abs() > EPS_FRAME_ORTHONORMAL
                || (frame[8] - 1.0).abs() > EPS_FRAME_ORTHONORMAL
                || frame[11].abs() > EPS_FRAME_ORTHONORMAL
            {
                return None;
            }
            let (major_axis, major_radius, minor_radius, parameter_shift) =
                if first_radius >= second_radius {
                    (first_axis, first_radius, second_radius, 0.0)
                } else {
                    (
                        second_axis,
                        second_radius,
                        first_radius,
                        -std::f64::consts::FRAC_PI_2,
                    )
                };
            let coincident_endpoints = conic.endpoints.iter().flatten().all(Option::is_some)
                && conic.endpoints[0]
                    .into_iter()
                    .zip(conic.endpoints[1])
                    .all(|(first, second)| {
                        let (Some(first), Some(second)) = (first, second) else {
                            return false;
                        };
                        let scale = first.abs().max(second.abs()).max(1.0);
                        (first - second).abs() <= EPS_PARAMETER_AGREEMENT * scale
                    });
            let (start_angle, end_angle) = match conic.parameters {
                [Some(start), Some(end)]
                    if start.is_finite()
                        && end.is_finite()
                        && (end - start - std::f64::consts::TAU).abs()
                            <= EPS_PARAMETER_FULL_TURN =>
                {
                    (None, None)
                }
                [Some(start), Some(end)] if start.is_finite() && end > start => (
                    Some(Angle(start + parameter_shift)),
                    Some(Angle(end + parameter_shift)),
                ),
                [Some(start), None]
                    if start.is_finite()
                        && start.abs() <= EPS_PARAMETER_FULL_TURN
                        && coincident_endpoints =>
                {
                    (None, None)
                }
                _ => return None,
            };
            Some((
                conic.entity_id,
                SketchGeometry::Ellipse {
                    center: Point2::new(frame[9], frame[10]),
                    major_angle: Angle(major_axis[1].atan2(major_axis[0])),
                    major_radius: Length(major_radius),
                    minor_radius: Length(minor_radius),
                    start_angle,
                    end_angle,
                },
                conic.offset,
            ))
        }
        crate::feature::FeatureSavedEntity::Spline(_)
        | crate::feature::FeatureSavedEntity::Dummy(_) => None,
    }
}

pub(crate) fn is_full_circle_geometry(geometry: &SketchGeometry) -> bool {
    matches!(geometry, SketchGeometry::Circle { .. })
        || matches!(
            geometry,
            SketchGeometry::Arc {
                start_angle,
                end_angle,
                ..
                } if (end_angle.0 - start_angle.0 - std::f64::consts::TAU).abs()
                    <= EPS_ANGLE_FULL_TURN
        )
}

pub(crate) fn saved_geometry_endpoints(geometry: &SketchGeometry) -> Option<[[f64; 2]; 2]> {
    match geometry {
        SketchGeometry::Line { start, end } => Some([[start.u, start.v], [end.u, end.v]]),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } if !is_full_circle_geometry(geometry) => Some([
            [
                center.u + radius.0 * start_angle.0.cos(),
                center.v + radius.0 * start_angle.0.sin(),
            ],
            [
                center.u + radius.0 * end_angle.0.cos(),
                center.v + radius.0 * end_angle.0.sin(),
            ],
        ]),
        SketchGeometry::Nurbs { control_points, .. } => {
            let first = control_points.first()?;
            let last = control_points.last()?;
            Some([[first.u, first.v], [last.u, last.v]])
        }
        _ => None,
    }
}

pub(crate) fn saved_section_missing_line_geometry(
    definition: &crate::feature::FeatureDefinition,
) -> Option<(usize, SketchGeometry)> {
    let order = definition.order_table.as_ref()?;
    order.is_complete().then_some(())?;
    let segments = definition.segments.as_ref()?;
    segments.is_complete().then_some(())?;
    let trim = definition.trim_entities.as_ref()?;
    (trim.has_complete_bucket_frame() && trim.has_unique_external_ids()).then_some(())?;
    let trimmed_external_ids = trim
        .rows
        .iter()
        .filter_map(|row| trim_segment_id(definition, row))
        .collect::<BTreeSet<_>>();
    let missing = segments
        .rows
        .iter()
        .filter(|candidate| {
            candidate.kind == crate::feature::FeatureSegmentKind::Line
                && order.internal_id(candidate.external_id).is_none()
                && trimmed_external_ids.contains(&candidate.external_id)
        })
        .collect::<Vec<_>>();
    let [missing] = missing.as_slice() else {
        return None;
    };
    let fixed_coordinate = match missing.vertical_horizontal {
        Some(0) => 0,
        Some(1) => 1,
        _ => return None,
    };

    let geometries = semantic_saved_section_entities(definition)
        .filter_map(saved_section_entity_geometry)
        .filter(|(internal_id, _, _)| order.rows.iter().any(|row| row.internal_id == *internal_id))
        .collect::<Vec<_>>();
    let ordered_ids = order
        .rows
        .iter()
        .map(|row| row.internal_id)
        .collect::<BTreeSet<_>>();
    let geometry_ids = geometries
        .iter()
        .map(|(internal_id, _, _)| *internal_id)
        .collect::<BTreeSet<_>>();
    (ordered_ids.len() == order.rows.len()
        && geometry_ids.len() == geometries.len()
        && geometry_ids == ordered_ids)
        .then_some(())?;
    let endpoints = geometries
        .iter()
        .filter_map(|(_, geometry, _)| saved_geometry_endpoints(geometry))
        .flatten()
        .collect::<Vec<_>>();
    (endpoints.len() == 2 * geometries.len()).then_some(())?;
    let mate_counts = endpoints
        .iter()
        .enumerate()
        .map(|(index, endpoint)| {
            endpoints
                .iter()
                .enumerate()
                .filter(|(candidate_index, candidate)| {
                    *candidate_index != index && saved_points_coincide(*endpoint, **candidate)
                })
                .count()
        })
        .collect::<Vec<_>>();
    (mate_counts.iter().filter(|count| **count == 0).count() == 2
        && mate_counts.iter().all(|count| *count <= 1))
    .then_some(())?;
    let open = endpoints
        .iter()
        .zip(mate_counts)
        .filter(|(_, count)| *count == 0)
        .map(|(endpoint, _)| *endpoint)
        .collect::<Vec<_>>();
    let [start, end] = open.as_slice() else {
        return None;
    };
    let scale = start
        .iter()
        .chain(end)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    ((start[fixed_coordinate] - end[fixed_coordinate]).abs() <= EPS_PARAMETER_AGREEMENT * scale)
        .then_some(())?;
    Some((
        missing.offset,
        SketchGeometry::Line {
            start: Point2::new(start[0], start[1]),
            end: Point2::new(end[0], end[1]),
        },
    ))
}

pub(crate) fn saved_points_coincide(first: [f64; 2], second: [f64; 2]) -> bool {
    let scale = first
        .into_iter()
        .chain(second)
        .map(f64::abs)
        .fold(1.0, f64::max);
    first
        .into_iter()
        .zip(second)
        .all(|(left, right)| (left - right).abs() <= EPS_PARAMETER_AGREEMENT * scale)
}

pub(crate) fn saved_profile_chains(
    sketch: &SketchId,
    geometries: &[(u32, SketchGeometry)],
) -> Vec<Vec<SketchEntityUse>> {
    let mut profiles = geometries
        .iter()
        .filter(|(_, geometry)| is_full_circle_geometry(geometry))
        .map(|(external_id, _)| {
            vec![SketchEntityUse {
                entity: sketch_entity_id(sketch, external_id),
                reversed: false,
            }]
        })
        .collect::<Vec<_>>();
    let rows = geometries
        .iter()
        .filter_map(|(external_id, geometry)| {
            Some((*external_id, saved_geometry_endpoints(geometry)?))
        })
        .collect::<Vec<_>>();
    let mut mates = vec![[None; 2]; rows.len()];
    for (row_index, (_, endpoints)) in rows.iter().enumerate() {
        for endpoint_index in 0..2 {
            let matches = rows
                .iter()
                .enumerate()
                .flat_map(|(candidate_row, (_, candidate_endpoints))| {
                    (0..2).map(move |candidate_endpoint| {
                        (candidate_row, candidate_endpoint, candidate_endpoints)
                    })
                })
                .filter(|(candidate_row, candidate_endpoint, candidate_endpoints)| {
                    (*candidate_row != row_index || *candidate_endpoint != endpoint_index)
                        && saved_points_coincide(
                            endpoints[endpoint_index],
                            candidate_endpoints[*candidate_endpoint],
                        )
                })
                .map(|(candidate_row, candidate_endpoint, _)| (candidate_row, candidate_endpoint))
                .collect::<Vec<_>>();
            if let [mate] = matches.as_slice() {
                mates[row_index][endpoint_index] = Some(*mate);
            }
        }
    }
    let mut remaining = (0..rows.len()).collect::<BTreeSet<_>>();
    while let Some(seed) = remaining
        .iter()
        .min_by_key(|index| rows[**index].0)
        .copied()
    {
        if mates[seed].iter().any(Option::is_none) {
            remaining.remove(&seed);
            continue;
        }
        let mut uses = Vec::new();
        let mut used = BTreeSet::new();
        let mut row = seed;
        let mut reversed = false;
        loop {
            if !used.insert(row) {
                break;
            }
            uses.push(SketchEntityUse {
                entity: sketch_entity_id(sketch, rows[row].0),
                reversed,
            });
            let outgoing = usize::from(!reversed);
            let Some((next_row, next_endpoint)) = mates[row][outgoing] else {
                break;
            };
            row = next_row;
            reversed = next_endpoint == 1;
            if row == seed {
                if !reversed {
                    profiles.push(uses);
                }
                break;
            }
        }
        remaining.retain(|index| !used.contains(index));
    }
    profiles
}

pub(crate) fn resolved_section_segment_geometry(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    let missing_line = saved_section_missing_line_geometry(definition);
    resolved_section_segment_geometry_with_missing_line(
        definition,
        points,
        segment,
        missing_line.as_ref(),
    )
}

pub(crate) fn resolved_section_segment_geometry_with_missing_line(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
    missing_line: Option<&(usize, SketchGeometry)>,
) -> Option<SketchGeometry> {
    let stored = section_segment_geometry(points, segment);
    let saved = saved_section_line_geometry(definition, segment)
        .or_else(|| saved_section_arc_geometry(definition, segment))
        .or_else(|| {
            missing_line
                .filter(|(offset, _)| *offset == segment.offset)
                .map(|(_, geometry)| geometry.clone())
        });
    match (stored, saved) {
        (Some(stored), Some(saved)) => {
            let agree = match (&stored, &saved) {
                (
                    SketchGeometry::Line {
                        start: stored_start,
                        end: stored_end,
                    },
                    SketchGeometry::Line {
                        start: saved_start,
                        end: saved_end,
                    },
                ) => {
                    saved_points_coincide(
                        [stored_start.u, stored_start.v],
                        [saved_start.u, saved_start.v],
                    ) && saved_points_coincide(
                        [stored_end.u, stored_end.v],
                        [saved_end.u, saved_end.v],
                    )
                }
                (
                    SketchGeometry::Arc {
                        center: stored_center,
                        radius: stored_radius,
                        ..
                    },
                    SketchGeometry::Arc {
                        center: saved_center,
                        radius: saved_radius,
                        ..
                    },
                ) => {
                    let radius_scale = stored_radius.0.max(saved_radius.0).max(1.0);
                    saved_points_coincide(
                        [stored_center.u, stored_center.v],
                        [saved_center.u, saved_center.v],
                    ) && (stored_radius.0 - saved_radius.0).abs()
                        <= EPS_RADIUS_AGREEMENT * radius_scale
                        && saved_geometry_endpoints(&stored)
                            .zip(saved_geometry_endpoints(&saved))
                            .is_some_and(|(stored, saved)| {
                                stored
                                    .into_iter()
                                    .zip(saved)
                                    .all(|(stored, saved)| saved_points_coincide(stored, saved))
                            })
                }
                _ => false,
            };
            agree.then_some(stored)
        }
        (Some(geometry), None) | (None, Some(geometry)) => Some(geometry),
        (None, None) => None,
    }
}
