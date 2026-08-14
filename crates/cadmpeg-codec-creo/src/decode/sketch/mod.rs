// SPDX-License-Identifier: Apache-2.0
//! Section geometry conversion and sketch-table coordinate, radius, and trim solvers.

#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn section_line_geometry(
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
    (((end[0] - start[0]) / scale).hypot((end[1] - start[1]) / scale) > 1e-12).then_some(())?;
    Some(SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
        end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
    })
}

pub(super) fn section_point_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Point).then_some(())?;
    let position = points.get(&segment.point_ids[0])?;
    Some(SketchGeometry::Point {
        position: cadmpeg_ir::math::Point2::new(position[0], position[1]),
    })
}

pub(super) fn section_arc_geometry(
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
    if first_radius <= 1e-12 || (first_radius - second_radius).abs() > 1e-9 * scale {
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

pub(super) fn section_circle_geometry(
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

pub(super) fn section_point_row_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeaturePointSegment,
) -> Option<SketchGeometry> {
    let point = points.get(&segment.point_id)?;
    Some(SketchGeometry::Point {
        position: Point2::new(point[0], point[1]),
    })
}

pub(super) fn section_centered_line_geometry(
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
    ((end[0] - start[0]).hypot(end[1] - start[1]) > 1e-12 * scale).then_some(())?;
    ((start[0] + end[0] - 2.0 * center[0]).hypot(start[1] + end[1] - 2.0 * center[1])
        <= 1e-9 * scale)
        .then_some(())?;
    Some(SketchGeometry::Line {
        start: Point2::new(start[0], start[1]),
        end: Point2::new(end[0], end[1]),
    })
}

pub(super) fn section_reference_line_geometry(
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
    (direction[0].hypot(direction[1]) > 1e-12 * scale).then_some(())?;
    Some(SketchGeometry::ReferenceLine {
        origin: Point2::new(start[0], start[1]),
        direction: Point2::new(direction[0], direction[1]),
    })
}

pub(super) fn resolved_section_reference_line_geometry(
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
    ((first - second).abs() <= 1e-9 * scale).then(|| {
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

pub(super) fn section_segment_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    section_line_geometry(points, segment)
        .or_else(|| section_arc_geometry(points, segment))
        .or_else(|| section_point_geometry(points, segment))
}

pub(super) fn saved_section_line_geometry(
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

pub(super) fn saved_section_arc_record<'a>(
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

pub(super) fn saved_section_arc_carrier(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<([f64; 2], f64)> {
    let arc = saved_section_arc_record(definition, segment)?;
    let [center_u, center_v, _] = arc.center;
    if let ([Some(center_u), Some(center_v)], Some(radius)) = (
        [center_u, center_v],
        arc.radius.filter(|radius| *radius > 1e-12),
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
            if denominator.abs() <= 1e-12 * scale {
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
            if denominator.abs() <= 1e-12 * scale {
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
    if first_radius <= 1e-12
        || (first_radius - second_radius).abs() > 1e-9 * radial_scale
        || arc.radius.is_some_and(|stored| {
            (stored - first_radius).abs() > 1e-9 * stored.max(first_radius).max(1.0)
        })
    {
        return None;
    }
    let radius = arc.radius.unwrap_or(first_radius);
    Some(([center_u, center_v], radius))
}

pub(super) fn saved_section_arc_geometry(
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
    if (first_radius - radius).abs() > 1e-9 * scale || (second_radius - radius).abs() > 1e-9 * scale
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

pub(super) fn saved_section_segment_point_coordinates(
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

pub(super) fn saved_section_circle_values(
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

pub(super) fn saved_section_entity_geometry(
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
                arc.radius.filter(|radius| *radius > 1e-12),
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
            if (first[0].hypot(first[1]) - radius).abs() > 1e-9 * scale
                || (second[0].hypot(second[1]) - radius).abs() > 1e-9 * scale
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
                circle.radius.filter(|radius| *radius > 1e-12),
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
                || first_radius <= 1e-12
                || second_radius <= 1e-12
                || (first_length - 1.0).abs() > 1e-9 * scale
                || (second_length - 1.0).abs() > 1e-9 * scale
                || (first_axis[0] * second_axis[0] + first_axis[1] * second_axis[1]).abs() > 1e-9
                || (first_axis[0] * second_axis[1] - first_axis[1] * second_axis[0] - 1.0).abs()
                    > 1e-9
                || frame[2].abs() > 1e-9
                || frame[5].abs() > 1e-9
                || frame[6].abs() > 1e-9
                || frame[7].abs() > 1e-9
                || (frame[8] - 1.0).abs() > 1e-9
                || frame[11].abs() > 1e-9
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
                        (first - second).abs() <= 1e-9 * scale
                    });
            let (start_angle, end_angle) = match conic.parameters {
                [Some(start), Some(end)]
                    if start.is_finite()
                        && end.is_finite()
                        && (end - start - std::f64::consts::TAU).abs() <= 1e-9 =>
                {
                    (None, None)
                }
                [Some(start), Some(end)] if start.is_finite() && end > start => (
                    Some(Angle(start + parameter_shift)),
                    Some(Angle(end + parameter_shift)),
                ),
                [Some(start), None]
                    if start.is_finite() && start.abs() <= 1e-9 && coincident_endpoints =>
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

pub(super) fn is_full_circle_geometry(geometry: &SketchGeometry) -> bool {
    matches!(geometry, SketchGeometry::Circle { .. })
        || matches!(
            geometry,
            SketchGeometry::Arc {
                start_angle,
                end_angle,
                ..
            } if (end_angle.0 - start_angle.0 - std::f64::consts::TAU).abs() <= 1e-12
        )
}

pub(super) fn saved_geometry_endpoints(geometry: &SketchGeometry) -> Option<[[f64; 2]; 2]> {
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

pub(super) fn saved_section_missing_line_geometry(
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
    ((start[fixed_coordinate] - end[fixed_coordinate]).abs() <= 1e-9 * scale).then_some(())?;
    Some((
        missing.offset,
        SketchGeometry::Line {
            start: Point2::new(start[0], start[1]),
            end: Point2::new(end[0], end[1]),
        },
    ))
}

pub(super) fn saved_points_coincide(first: [f64; 2], second: [f64; 2]) -> bool {
    let scale = first
        .into_iter()
        .chain(second)
        .map(f64::abs)
        .fold(1.0, f64::max);
    first
        .into_iter()
        .zip(second)
        .all(|(left, right)| (left - right).abs() <= 1e-9 * scale)
}

pub(super) fn saved_profile_chains(
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

pub(super) fn resolved_section_segment_geometry(
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

pub(super) fn resolved_section_segment_geometry_with_missing_line(
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
                    ) && (stored_radius.0 - saved_radius.0).abs() <= 1e-9 * radius_scale
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

pub(crate) fn resolved_section_coordinates(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, [Option<f64>; 2]> {
    let (points, ambiguous_point_ids) = match &definition.variables {
        Some(variables) if variables.is_complete() => variables.reconciled_points(),
        Some(_) => return BTreeMap::new(),
        None => (BTreeMap::new(), BTreeSet::new()),
    };
    let mut segment_counts = BTreeMap::new();
    for segment in definition.segments.iter().flat_map(|table| &table.rows) {
        *segment_counts.entry(segment.external_id).or_insert(0usize) += 1;
    }
    let mut saved_segment_points = definition
        .segments
        .iter()
        .filter(|table| table.is_complete())
        .flat_map(|table| {
            table
                .rows
                .iter()
                .filter(|segment| table.external_id_count(segment.external_id) == 1)
        })
        .filter(|segment| {
            segment
                .point_ids
                .iter()
                .all(|point_id| !ambiguous_point_ids.contains(point_id))
        })
        .filter_map(|segment| saved_section_segment_point_coordinates(definition, segment))
        .flatten()
        .collect::<Vec<_>>();
    saved_segment_points.extend(
        definition
            .segments
            .iter()
            .filter(|table| table.is_complete())
            .flat_map(|table| &table.circle_rows)
            .filter_map(|segment| {
                (!ambiguous_point_ids.contains(&segment.center_id)).then_some(())?;
                let (center, _) = saved_section_circle_values(definition, segment)?;
                Some((segment.center_id, center))
            }),
    );
    let segments = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Line)
        .filter(|segment| segment_counts[&segment.external_id] == 1)
        .filter(|segment| {
            segment
                .point_ids
                .iter()
                .all(|point_id| !ambiguous_point_ids.contains(point_id))
        })
        .collect::<Vec<_>>();
    let coincident_points = active_complete_section_skamps(definition)
        .filter_map(|skamp| {
            let [first, second] = skamp.items.as_slice() else {
                return None;
            };
            let pair = match skamp.kind {
                0 => Some([
                    section_skamp_selected_point(definition, first)?,
                    section_skamp_selected_point(definition, second)?,
                ]),
                3 => {
                    let first_point = section_skamp_point_entity_id(definition, first);
                    let second_point = section_skamp_point_entity_id(definition, second);
                    match (first_point, second_point) {
                        (Some(first), Some(second)) => Some([
                            SectionPointSource::Point(first),
                            SectionPointSource::Point(second),
                        ]),
                        (Some(point), None) => Some([
                            SectionPointSource::Point(point),
                            section_skamp_selected_point(definition, second)?,
                        ]),
                        (None, Some(point)) => Some([
                            section_skamp_selected_point(definition, first)?,
                            SectionPointSource::Point(point),
                        ]),
                        _ => None,
                    }
                }
                _ => None,
            }?;
            (pair
                .iter()
                .any(|point| matches!(point, SectionPointSource::Point(_)))
                && pair.iter().all(|point| match point {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                }))
            .then_some(pair)
        })
        .collect::<Vec<_>>();
    let same_coordinate_points = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_same_coordinate_sources(definition, skamp))
        .filter(|(pair, _)| {
            pair.iter()
                .any(|point| matches!(point, SectionPointSource::Point(_)))
                && pair.iter().all(|point| match point {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                })
        })
        .collect::<Vec<_>>();
    let point_on_line_coordinates = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_point_on_line(definition, skamp))
        .filter(|(first, second, _)| {
            !ambiguous_point_ids.contains(first) && !ambiguous_point_ids.contains(second)
        })
        .collect::<Vec<_>>();
    let saved_point_on_line_coordinates = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_saved_point_on_line(definition, skamp))
        .filter(|(point_id, _, _)| !ambiguous_point_ids.contains(point_id))
        .collect::<Vec<_>>();
    let line_midpoint_constraints = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_line_midpoint_sources(definition, skamp))
        .filter(|(point_ids, point)| {
            point_ids
                .iter()
                .all(|point_id| !ambiguous_point_ids.contains(point_id))
                && match point {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                }
        })
        .collect::<Vec<_>>();
    let symmetric_point_constraints = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_axis_symmetry(definition, skamp))
        .filter(|(axis, first, second, _)| {
            [first, second]
                .into_iter()
                .any(|point| matches!(point, SectionPointSource::Point(_)))
                && [first, second].into_iter().all(|point| match point {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                })
                && match axis {
                    SectionSymmetryAxis::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionSymmetryAxis::Value(_) => true,
                }
        })
        .collect::<Vec<_>>();
    let point_symmetric_constraints = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_point_symmetry(definition, skamp))
        .filter(|(center, first, second)| {
            !ambiguous_point_ids.contains(center)
                && [first, second].into_iter().all(|point| match point {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                })
        })
        .collect::<Vec<_>>();
    let auxiliary_constraints =
        section_equation_auxiliary_constraints(definition, &ambiguous_point_ids);
    let mut auxiliary_scalar_values = section_equation_scalar_seed_values(definition);
    let linear_dimension_candidates = definition
        .relations
        .iter()
        .filter(|table| feature_relation_table_complete(table))
        .flat_map(|table| &table.rows)
        .filter_map(|relation| {
            if section_solver_relation_is_disabled(definition, relation.relation_id) {
                return None;
            }
            if relation.relation_type != 0 {
                return None;
            }
            let vectors = relation.operand_vectors?;
            if !section_linear_distance_vectors(vectors) {
                return None;
            }
            let [Some(first), Some(second), _, _] = vectors[0] else {
                return None;
            };
            let coordinate = section_linear_distance_coordinate(
                definition,
                &segments,
                first,
                second,
                &points,
                &saved_segment_points,
                &ambiguous_point_ids,
            )?;
            let magnitude = section_relation_length_dimension(definition, relation)?
                .value
                .filter(|value| value.is_finite() && *value >= 0.0)?;
            matches!(relation.sign, 0 | 1 | 0xf6).then_some((
                first,
                second,
                coordinate,
                magnitude,
                relation.sign,
            ))
        })
        .collect::<Vec<_>>();
    let signed_dimension_candidates = linear_dimension_candidates
        .iter()
        .filter_map(|&(first, second, coordinate, magnitude, sign)| {
            let delta = match sign {
                1 => magnitude,
                0xf6 => -magnitude,
                _ => return None,
            };
            Some((first, second, coordinate, delta))
        })
        .collect::<Vec<_>>();
    let mut unsigned_dimension_candidates = linear_dimension_candidates
        .iter()
        .filter_map(|&(first, second, coordinate, magnitude, sign)| {
            (sign == 0).then_some((first, second, coordinate, magnitude))
        })
        .collect::<Vec<_>>();
    unsigned_dimension_candidates.extend(
        section_equation_unsigned_coordinate_distances(definition, &ambiguous_point_ids)
            .into_iter()
            .map(|constraint| {
                (
                    constraint.first,
                    constraint.second,
                    constraint.coordinate,
                    constraint.value,
                )
            }),
    );
    let radial_constraints =
        section_equation_radial_constraints(definition, &points, &ambiguous_point_ids);
    let equal_length_constraints =
        section_equation_equal_length_constraints(definition, &ambiguous_point_ids);
    let mut signed_dimensions = BTreeMap::<(u32, u32, usize), Option<f64>>::new();
    for (first, second, coordinate, delta) in signed_dimension_candidates {
        let (key, canonical_delta) = if first <= second {
            ((first, second, coordinate), delta)
        } else {
            ((second, first, coordinate), -delta)
        };
        signed_dimensions
            .entry(key)
            .and_modify(|stored| {
                if stored.is_some_and(|stored| stored != canonical_delta) {
                    *stored = None;
                }
            })
            .or_insert(Some(canonical_delta));
    }
    let signed_dimensions = signed_dimensions
        .into_iter()
        .filter_map(|((first, second, coordinate), delta)| {
            Some((first, second, coordinate, delta?))
        })
        .collect::<Vec<_>>();
    let mut equations = Vec::new();
    for (&point_id, coordinates) in &points {
        for (coordinate, value) in coordinates.iter().copied().enumerate() {
            if let Some(value) = value {
                equations.push(SectionCoordinateEquation::point_value(
                    point_id, coordinate, value,
                ));
            }
        }
    }
    for &(point_id, coordinates) in &saved_segment_points {
        for (coordinate, value) in coordinates.into_iter().enumerate() {
            equations.push(SectionCoordinateEquation::point_value(
                point_id, coordinate, value,
            ));
        }
    }
    for segment in &segments {
        if let Some(coordinate) = section_line_fixed_coordinate(definition, segment) {
            equations.push(SectionCoordinateEquation::point_difference(
                segment.point_ids[0],
                segment.point_ids[1],
                coordinate,
                0.0,
            ));
        }
    }
    for &(first, second, coordinate, delta) in &signed_dimensions {
        equations.push(SectionCoordinateEquation::point_difference(
            first, second, coordinate, delta,
        ));
    }
    for &[first, second] in &coincident_points {
        for coordinate in 0..2 {
            equations.push(SectionCoordinateEquation::source_difference(
                first, second, coordinate, 0.0,
            ));
        }
    }
    for (first, second, coordinate) in
        section_equation_coordinate_equalities(definition, &ambiguous_point_ids)
    {
        equations.push(SectionCoordinateEquation::point_difference(
            first, second, coordinate, 0.0,
        ));
    }
    for (target, first, second) in
        section_equation_point_on_line_constraints(definition, &ambiguous_point_ids)
    {
        let (
            Some([Some(first_u), Some(first_v)]),
            Some([Some(second_u), Some(second_v)]),
            Some(target_coordinates),
        ) = (points.get(&first), points.get(&second), points.get(&target))
        else {
            continue;
        };
        let [target_u, target_v] = *target_coordinates;
        if target_u.is_some() == target_v.is_some() {
            continue;
        }
        let delta_u = second_u - first_u;
        let delta_v = second_v - first_v;
        let mut equation = SectionCoordinateEquation::default();
        equation.add_point(target, 0, -delta_v);
        equation.add_point(target, 1, delta_u);
        equation.rhs = delta_u * first_v - delta_v * first_u;
        let missing_coefficient = if target_u.is_none() {
            delta_v.abs()
        } else {
            delta_u.abs()
        };
        if missing_coefficient > 1e-12 {
            equations.push(equation);
        }
    }
    for constraint in &radial_constraints {
        if let Some(offset) = constraint.offset() {
            equations.push(SectionCoordinateEquation::point_difference(
                constraint.first,
                constraint.second,
                0,
                offset[0],
            ));
            equations.push(SectionCoordinateEquation::point_difference(
                constraint.first,
                constraint.second,
                1,
                offset[1],
            ));
        }
    }
    for &([first, second], coordinate) in &same_coordinate_points {
        equations.push(SectionCoordinateEquation::source_difference(
            first, second, coordinate, 0.0,
        ));
    }
    for &(first, second, coordinate) in &point_on_line_coordinates {
        equations.push(SectionCoordinateEquation::point_difference(
            first, second, coordinate, 0.0,
        ));
    }
    for &(point, coordinate, value) in &saved_point_on_line_coordinates {
        equations.push(SectionCoordinateEquation::point_value(
            point, coordinate, value,
        ));
    }
    for &(point_ids, point) in &line_midpoint_constraints {
        for coordinate in 0..2 {
            let mut equation = SectionCoordinateEquation::default();
            equation.add_point(point_ids[0], coordinate, 1.0);
            equation.add_point(point_ids[1], coordinate, 1.0);
            equation.add_source(point, coordinate, -2.0);
            equations.push(equation);
        }
    }
    for &(axis, first, second, fixed_coordinate) in &symmetric_point_constraints {
        let parallel_coordinate = 1usize.saturating_sub(fixed_coordinate);
        equations.push(SectionCoordinateEquation::source_difference(
            first,
            second,
            parallel_coordinate,
            0.0,
        ));
        let mut equation = SectionCoordinateEquation::default();
        equation.add_source(first, fixed_coordinate, 1.0);
        equation.add_source(second, fixed_coordinate, 1.0);
        match axis {
            SectionSymmetryAxis::Point(point_id) => {
                equation.add_point(point_id, fixed_coordinate, -2.0);
            }
            SectionSymmetryAxis::Value(value) => equation.rhs += 2.0 * value,
        }
        equations.push(equation);
    }
    for &(center, first, second) in &point_symmetric_constraints {
        for coordinate in 0..2 {
            let mut equation = SectionCoordinateEquation::default();
            equation.add_source(first, coordinate, 1.0);
            equation.add_source(second, coordinate, 1.0);
            equation.add_point(center, coordinate, -2.0);
            equations.push(equation);
        }
    }
    let stored_coordinates = points
        .iter()
        .flat_map(|(&point, coordinates)| {
            coordinates
                .iter()
                .copied()
                .enumerate()
                .filter_map(move |(coordinate, value)| Some(((point, coordinate), value?)))
        })
        .collect();
    append_section_equation_auxiliary_coordinate_constraints(
        &auxiliary_constraints,
        &auxiliary_scalar_values,
        &stored_coordinates,
        &mut equations,
    );
    let unsigned_coordinates = solve_unsigned_dimension_coordinates(
        &equations,
        &stored_coordinates,
        &unsigned_dimension_candidates,
    );
    for ((point, coordinate), value) in unsigned_coordinates {
        equations.push(SectionCoordinateEquation::point_value(
            point, coordinate, value,
        ));
    }
    let mut solved_coordinates =
        solve_section_coordinate_equations(&equations, &stored_coordinates);
    for _ in 0..equal_length_constraints.len() {
        let equal_length_values =
            section_equal_length_coordinate_values(&equal_length_constraints, &solved_coordinates);
        let mut added = false;
        for (variable, value) in equal_length_values {
            let Some(value) = value else {
                continue;
            };
            equations.push(SectionCoordinateEquation::point_value(
                variable.0, variable.1, value,
            ));
            added = true;
        }
        if !added {
            break;
        }
        solved_coordinates = solve_section_coordinate_equations(&equations, &stored_coordinates);
    }
    for constraint in
        section_equation_radial_constraints(definition, &solved_coordinates, &ambiguous_point_ids)
    {
        if let Some(offset) = constraint.offset() {
            equations.push(SectionCoordinateEquation::point_difference(
                constraint.first,
                constraint.second,
                0,
                offset[0],
            ));
            equations.push(SectionCoordinateEquation::point_difference(
                constraint.first,
                constraint.second,
                1,
                offset[1],
            ));
        }
    }
    let second_unsigned_coordinates = solve_unsigned_dimension_coordinates(
        &equations,
        &stored_coordinates,
        &unsigned_dimension_candidates,
    );
    for ((point, coordinate), value) in second_unsigned_coordinates {
        equations.push(SectionCoordinateEquation::point_value(
            point, coordinate, value,
        ));
    }
    let solved_coordinates = solve_section_coordinate_equations(&equations, &stored_coordinates);
    for (variable, value) in
        section_equation_scalar_values_from_coordinates(definition, &solved_coordinates)
    {
        merge_scalar_value_candidate(&mut auxiliary_scalar_values, variable, value);
    }
    append_section_equation_auxiliary_coordinate_constraints(
        &auxiliary_constraints,
        &auxiliary_scalar_values,
        &stored_coordinates,
        &mut equations,
    );
    let solved_coordinates = solve_section_coordinate_equations(&equations, &stored_coordinates);
    let arc_midpoint_constraints = active_complete_section_skamps(definition)
        .filter_map(|skamp| {
            section_skamp_arc_midpoint_source(definition, skamp, &solved_coordinates)
        })
        .filter_map(|(point, midpoint)| match point {
            SectionPointSource::Point(point_id) if !ambiguous_point_ids.contains(&point_id) => {
                Some((point_id, midpoint))
            }
            SectionPointSource::Point(_) | SectionPointSource::Value(_) => None,
        })
        .collect::<Vec<_>>();
    for &(point_id, midpoint) in &arc_midpoint_constraints {
        for (coordinate, value) in midpoint.into_iter().enumerate() {
            equations.push(SectionCoordinateEquation::point_value(
                point_id, coordinate, value,
            ));
        }
    }
    solve_section_coordinate_equations(&equations, &stored_coordinates)
}

pub(super) fn section_linear_distance_coordinate(
    definition: &crate::feature::FeatureDefinition,
    segments: &[&crate::feature::FeatureSegment],
    first: u32,
    second: u32,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    saved_segment_points: &[(u32, [f64; 2])],
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Option<usize> {
    let matching_segments = segments
        .iter()
        .copied()
        .filter(|segment| {
            segment.point_ids == [first, second] || segment.point_ids == [second, first]
        })
        .collect::<Vec<_>>();
    if let [segment] = matching_segments.as_slice() {
        if let Some(fixed_coordinate) = section_line_fixed_coordinate(definition, segment) {
            return 1usize.checked_sub(fixed_coordinate);
        }
    }
    if matching_segments.len() > 1 {
        return None;
    }
    let table = definition.segments.as_ref()?;
    let has_unique_incident_entity = |point_id| {
        table.rows.iter().any(|segment| {
            segment.point_ids.contains(&point_id)
                && table.external_id_count(segment.external_id) == 1
        }) || table.point_rows.iter().any(|segment| {
            segment.point_id == point_id && table.external_id_count(segment.external_id) == 1
        })
    };
    has_unique_incident_entity(first).then_some(())?;
    has_unique_incident_entity(second).then_some(())?;
    let point_coordinate = |point_id: u32, coordinate: usize| -> Option<f64> {
        if ambiguous_point_ids.contains(&point_id) {
            return None;
        }
        let mut values = Vec::new();
        if let Some(value) = coordinates
            .get(&point_id)
            .and_then(|point| point[coordinate])
        {
            value.is_finite().then_some(())?;
            values.push(value);
        }
        for &(_, point) in saved_segment_points
            .iter()
            .filter(|(saved_point_id, _)| *saved_point_id == point_id)
        {
            let value = point[coordinate];
            value.is_finite().then_some(())?;
            values.push(value);
        }
        let first = values.first().copied()?;
        let scale = values.iter().map(|value| value.abs()).fold(1.0, f64::max);
        values
            .iter()
            .all(|value| (*value - first).abs() <= 1e-9 * scale)
            .then_some(first)
    };
    let equal_coordinate = |coordinate: usize| -> Option<bool> {
        let first = point_coordinate(first, coordinate)?;
        let second = point_coordinate(second, coordinate)?;
        let scale = first.abs().max(second.abs()).max(1.0);
        Some((first - second).abs() <= 1e-9 * scale)
    };
    let equal_u = equal_coordinate(0);
    let equal_v = equal_coordinate(1);
    if equal_u == Some(true) && equal_v != Some(true) {
        return Some(1);
    }
    if equal_v == Some(true) && equal_u != Some(true) {
        return Some(0);
    }
    None
}

pub(crate) fn resolved_section_points(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, [f64; 2]> {
    resolved_section_coordinates(definition)
        .into_iter()
        .filter_map(|(point, [u, v])| Some((point, [u?, v?])))
        .collect()
}

pub(super) fn section_equation_coordinate_equalities(
    definition: &crate::feature::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<(u32, u32, usize)> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
        .filter_map(|equation| {
            let (first, second, auxiliary) = match equation.function_id {
                2 if equation.arguments.len() == 2 => {
                    let [Some(first), Some(second)] = equation.arguments.as_slice() else {
                        return None;
                    };
                    (*first, *second, None)
                }
                13 if equation.arguments.len() == 3 => {
                    let [Some(first), Some(second), Some(auxiliary)] =
                        equation.arguments.as_slice()
                    else {
                        return None;
                    };
                    (*first, *second, Some(*auxiliary))
                }
                _ => return None,
            };
            let first = variables.rows.get(usize::try_from(first).ok()?)?;
            let second = variables.rows.get(usize::try_from(second).ok()?)?;
            if let Some(auxiliary) = auxiliary {
                let auxiliary = variables.rows.get(usize::try_from(auxiliary).ok()?)?;
                if auxiliary.variable_type != 7 || auxiliary.value != Some(0.0) {
                    return None;
                }
            }
            if first.variable_type != second.variable_type
                || !matches!(first.variable_type, 1 | 2)
                || auxiliary.is_some() && first.variable_type != 2
                || ambiguous_point_ids.contains(&first.key)
                || ambiguous_point_ids.contains(&second.key)
                || first.key == second.key
            {
                return None;
            }
            Some((first.key, second.key, usize::from(first.variable_type == 2)))
        })
        .collect()
}

pub(super) type SectionScalarVariable = (u32, u32);

#[derive(Clone, Copy)]
pub(super) struct SectionEquationMidpointConstraint {
    pub(super) first: SectionCoordinateVariable,
    pub(super) second: SectionCoordinateVariable,
    pub(super) result: SectionScalarVariable,
}

#[derive(Clone, Copy)]
pub(super) struct SectionEquationPointBinding {
    pub(super) point: u32,
    pub(super) coordinates: [SectionScalarVariable; 2],
}

#[derive(Default)]
pub(super) struct SectionEquationAuxiliaryConstraints {
    pub(super) midpoints: Vec<SectionEquationMidpointConstraint>,
    pub(super) point_bindings: Vec<SectionEquationPointBinding>,
}

pub(super) fn section_equation_auxiliary_constraints(
    definition: &crate::feature::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> SectionEquationAuxiliaryConstraints {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return SectionEquationAuxiliaryConstraints::default();
    };
    let Some(equations) =
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return SectionEquationAuxiliaryConstraints::default();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return SectionEquationAuxiliaryConstraints::default();
    };
    if declared_count != equations.rows.len() + 1 {
        return SectionEquationAuxiliaryConstraints::default();
    }

    let row = |ordinal: Option<u32>| {
        usize::try_from(ordinal?)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    let mut constraints = SectionEquationAuxiliaryConstraints::default();
    for equation in equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
    {
        match (equation.function_id, equation.arguments.as_slice()) {
            (42, [Some(first), Some(second), Some(result)]) => {
                let (Some(first), Some(second), Some(result)) =
                    (row(Some(*first)), row(Some(*second)), row(Some(*result)))
                else {
                    continue;
                };
                if first.variable_type != second.variable_type
                    || !matches!(first.variable_type, 1 | 2)
                    || result.variable_type != 6
                    || ambiguous_point_ids.contains(&first.key)
                    || ambiguous_point_ids.contains(&second.key)
                {
                    continue;
                }
                let coordinate = usize::from(first.variable_type == 2);
                constraints
                    .midpoints
                    .push(SectionEquationMidpointConstraint {
                        first: (first.key, coordinate),
                        second: (second.key, coordinate),
                        result: (result.variable_type, result.key),
                    });
            }
            (31, [Some(first_u), Some(first_v), Some(second_u), Some(second_v)]) => {
                let (Some(first_u), Some(first_v), Some(second_u), Some(second_v)) = (
                    row(Some(*first_u)),
                    row(Some(*first_v)),
                    row(Some(*second_u)),
                    row(Some(*second_v)),
                ) else {
                    continue;
                };
                if first_u.variable_type != 1
                    || first_v.variable_type != 2
                    || first_u.key != first_v.key
                    || second_u.variable_type != 6
                    || second_v.variable_type != 6
                    || second_u.key == second_v.key
                    || ambiguous_point_ids.contains(&first_u.key)
                {
                    continue;
                }
                constraints
                    .point_bindings
                    .push(SectionEquationPointBinding {
                        point: first_u.key,
                        coordinates: [
                            (second_u.variable_type, second_u.key),
                            (second_v.variable_type, second_v.key),
                        ],
                    });
            }
            _ => {}
        }
    }
    constraints
}

pub(super) fn merge_scalar_value_candidate(
    values: &mut BTreeMap<SectionScalarVariable, Option<f64>>,
    variable: SectionScalarVariable,
    value: f64,
) {
    if !value.is_finite() {
        return;
    }
    match values.entry(variable) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(Some(value));
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            let Some(stored) = *entry.get() else {
                return;
            };
            if !approximately_equal(stored, value) {
                *entry.get_mut() = None;
            }
        }
    }
}

pub(super) fn section_equation_scalar_seed_values(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<SectionScalarVariable, Option<f64>> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return BTreeMap::new();
    };
    let mut values = BTreeMap::new();
    for row in &variables.rows {
        if matches!(row.variable_type, 1 | 2) {
            continue;
        }
        let variable = (row.variable_type, row.key);
        match row.value {
            Some(value) if value.is_finite() => {
                merge_scalar_value_candidate(&mut values, variable, value);
            }
            Some(_) => {
                values.insert(variable, None);
            }
            None => {}
        }
    }
    for (variable, value) in section_equation_scalar_equalities(definition) {
        merge_scalar_value_candidate(&mut values, variable, value);
    }
    values
}

pub(super) fn append_section_equation_auxiliary_coordinate_constraints(
    constraints: &SectionEquationAuxiliaryConstraints,
    scalar_values: &BTreeMap<SectionScalarVariable, Option<f64>>,
    stored_coordinates: &BTreeMap<SectionCoordinateVariable, f64>,
    equations: &mut Vec<SectionCoordinateEquation>,
) {
    for constraint in &constraints.midpoints {
        let Some(Some(value)) = scalar_values.get(&constraint.result) else {
            continue;
        };
        if stored_coordinates
            .get(&constraint.first)
            .zip(stored_coordinates.get(&constraint.second))
            .is_some_and(|(first, second)| {
                !approximately_equal(f64::midpoint(*first, *second), *value)
            })
        {
            continue;
        }
        let mut equation = SectionCoordinateEquation::default();
        equation.add_point(constraint.first.0, constraint.first.1, 1.0);
        equation.add_point(constraint.second.0, constraint.second.1, 1.0);
        equation.rhs = 2.0 * value;
        equations.push(equation);
    }
    for constraint in &constraints.point_bindings {
        let mut values = [None; 2];
        let mut underdetermined = false;
        let mut invalid = false;
        for (coordinate, variable) in constraint.coordinates.into_iter().enumerate() {
            match scalar_values.get(&variable) {
                Some(Some(value)) => {
                    if stored_coordinates
                        .get(&(constraint.point, coordinate))
                        .is_some_and(|stored| !approximately_equal(*stored, *value))
                    {
                        invalid = true;
                        break;
                    }
                    values[coordinate] = Some(*value);
                }
                Some(None) => {
                    invalid = true;
                    break;
                }
                None => {
                    underdetermined |=
                        !stored_coordinates.contains_key(&(constraint.point, coordinate));
                }
            }
        }
        if invalid || underdetermined {
            continue;
        }
        for (coordinate, value) in values
            .into_iter()
            .enumerate()
            .filter_map(|(coordinate, value)| Some((coordinate, value?)))
        {
            equations.push(SectionCoordinateEquation::point_value(
                constraint.point,
                coordinate,
                value,
            ));
        }
    }
}

pub(super) fn section_equation_scalar_values_from_coordinates(
    definition: &crate::feature::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
) -> BTreeMap<SectionScalarVariable, f64> {
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .map_or_else(BTreeSet::new, |variables| variables.reconciled_points().1);
    let constraints = section_equation_auxiliary_constraints(definition, &ambiguous_point_ids);
    let seed_values = section_equation_scalar_seed_values(definition);
    let mut derived = BTreeMap::<SectionScalarVariable, Option<f64>>::new();
    let compatible = |variable: SectionScalarVariable, value: f64| {
        !seed_values.contains_key(&variable)
            || seed_values[&variable].is_some_and(|stored| approximately_equal(stored, value))
    };
    for constraint in constraints.midpoints {
        let (Some(Some(first)), Some(Some(second))) = (
            coordinates
                .get(&constraint.first.0)
                .map(|point| point[constraint.first.1]),
            coordinates
                .get(&constraint.second.0)
                .map(|point| point[constraint.second.1]),
        ) else {
            continue;
        };
        let value = f64::midpoint(first, second);
        if compatible(constraint.result, value) {
            merge_scalar_value_candidate(&mut derived, constraint.result, value);
        }
    }
    for constraint in constraints.point_bindings {
        let Some(point) = coordinates.get(&constraint.point) else {
            continue;
        };
        let mut invalid = false;
        let mut candidates = Vec::new();
        for (coordinate, variable) in constraint.coordinates.into_iter().enumerate() {
            let Some(value) = point[coordinate] else {
                continue;
            };
            if !compatible(variable, value) {
                invalid = true;
                break;
            }
            if !seed_values.contains_key(&variable) {
                candidates.push((variable, value));
            }
        }
        if !invalid {
            for (variable, value) in candidates {
                merge_scalar_value_candidate(&mut derived, variable, value);
            }
        }
    }
    derived
        .into_iter()
        .filter_map(|(variable, value)| Some((variable, value?)))
        .collect()
}

pub(super) fn section_equation_scalar_equality_components(
    definition: &crate::feature::FeatureDefinition,
) -> Vec<BTreeSet<SectionScalarVariable>> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }

    let mut adjacency = BTreeMap::<SectionScalarVariable, BTreeSet<SectionScalarVariable>>::new();
    for equation in equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
    {
        let (first, second, selector) = match (equation.function_id, equation.arguments.as_slice())
        {
            (2, [Some(first), Some(second)]) => (*first, *second, None),
            (5, [Some(first), Some(second), Some(selector)]) => (*first, *second, Some(*selector)),
            _ => continue,
        };
        let Some(first) = usize::try_from(first)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let Some(second) = usize::try_from(second)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        if let Some(selector) = selector {
            let Some(selector) = usize::try_from(selector)
                .ok()
                .and_then(|ordinal| variables.rows.get(ordinal))
            else {
                continue;
            };
            if first.variable_type != 6
                || second.variable_type != 6
                || selector.variable_type != 5
                || selector.value != Some(0.0)
            {
                continue;
            }
        }
        if first.variable_type != second.variable_type
            || matches!(first.variable_type, 1 | 2)
            || first.key == second.key
        {
            continue;
        }
        let first = (first.variable_type, first.key);
        let second = (second.variable_type, second.key);
        adjacency.entry(first).or_default().insert(second);
        adjacency.entry(second).or_default().insert(first);
    }

    let mut remaining = adjacency.keys().copied().collect::<BTreeSet<_>>();
    let mut components = Vec::new();
    while let Some(seed) = remaining.pop_first() {
        let mut component = BTreeSet::from([seed]);
        let mut pending = std::collections::VecDeque::from([seed]);
        while let Some(variable) = pending.pop_front() {
            for neighbor in adjacency
                .get(&variable)
                .into_iter()
                .flat_map(|neighbors| neighbors.iter())
                .copied()
            {
                if component.insert(neighbor) {
                    remaining.remove(&neighbor);
                    pending.push_back(neighbor);
                }
            }
        }
        components.push(component);
    }
    components
}

pub(super) fn section_equation_scalar_equalities(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<SectionScalarVariable, f64> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return BTreeMap::new();
    };
    let mut values = BTreeMap::<SectionScalarVariable, Vec<f64>>::new();
    let mut invalid = BTreeSet::<SectionScalarVariable>::new();
    for row in &variables.rows {
        if matches!(row.variable_type, 1 | 2) {
            continue;
        }
        let variable = (row.variable_type, row.key);
        match row.value {
            Some(value) if value.is_finite() => values.entry(variable).or_default().push(value),
            Some(_) => {
                invalid.insert(variable);
            }
            None => {}
        }
    }

    let mut resolved = BTreeMap::new();
    for component in section_equation_scalar_equality_components(definition) {
        if component.iter().any(|variable| invalid.contains(variable)) {
            continue;
        }
        let component_values = component
            .iter()
            .flat_map(|variable| values.get(variable).into_iter().flatten().copied())
            .collect::<Vec<_>>();
        let Some(first) = component_values.first().copied() else {
            continue;
        };
        let scale = component_values
            .iter()
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        if component_values
            .iter()
            .any(|value| (*value - first).abs() > 1e-9 * scale)
        {
            continue;
        }
        resolved.extend(component.into_iter().map(|variable| (variable, first)));
    }
    resolved
}

#[derive(Clone, Copy)]
pub(super) struct SectionRadialConstraint {
    pub(super) first: u32,
    pub(super) second: u32,
    pub(super) radius: (u32, u32),
    pub(super) angle: (u32, u32),
    pub(super) radius_value: Option<f64>,
    pub(super) angle_value: Option<f64>,
}

impl SectionRadialConstraint {
    pub(super) fn offset(self) -> Option<[f64; 2]> {
        let radius = self.radius_value?;
        if radius.abs() <= 1e-12 {
            return Some([0.0; 2]);
        }
        let angle = self.angle_value?;
        Some([radius * angle.cos(), radius * angle.sin()])
    }
}

pub(super) fn section_equation_radial_constraints(
    definition: &crate::feature::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionRadialConstraint> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
        .filter(|equation| equation.function_id == 0 && equation.arguments.len() == 6)
        .filter_map(|equation| {
            let [
                Some(first_u),
                Some(first_v),
                Some(second_u),
                Some(second_v),
                Some(radius),
                Some(angle),
            ] = equation.arguments.as_slice()
            else {
                return None;
            };
            let first_u = variables.rows.get(usize::try_from(*first_u).ok()?)?;
            let first_v = variables.rows.get(usize::try_from(*first_v).ok()?)?;
            let second_u = variables.rows.get(usize::try_from(*second_u).ok()?)?;
            let second_v = variables.rows.get(usize::try_from(*second_v).ok()?)?;
            let radius = variables.rows.get(usize::try_from(*radius).ok()?)?;
            let angle = variables.rows.get(usize::try_from(*angle).ok()?)?;
            if first_u.variable_type != 1
                || first_v.variable_type != 2
                || second_u.variable_type != 1
                || second_v.variable_type != 2
                || first_u.key != first_v.key
                || second_u.key != second_v.key
                || first_u.key == second_u.key
                || !matches!(radius.variable_type, 0 | 3)
                || !matches!(angle.variable_type, 4 | 6)
                || ambiguous_point_ids.contains(&first_u.key)
                || ambiguous_point_ids.contains(&second_u.key)
            {
                return None;
            }
            let mut radius_value = match radius.value {
                Some(value) if value.is_finite() && value >= 0.0 => Some(value),
                Some(_) => return None,
                None => None,
            };
            let mut angle_value = match angle.value {
                Some(value) if value.is_finite() => Some(value),
                Some(_) => return None,
                None => None,
            };
            let first_point = coordinates.get(&first_u.key).and_then(|point| {
                Some([point[0]?, point[1]?])
            });
            let second_point = coordinates.get(&second_u.key).and_then(|point| {
                Some([point[0]?, point[1]?])
            });
            if let (Some(first), Some(second)) = (first_point, second_point) {
                if !first.into_iter().chain(second).all(f64::is_finite) {
                    return None;
                }
                let delta = [second[0] - first[0], second[1] - first[1]];
                let distance = delta[0].hypot(delta[1]);
                let scale = distance
                    .abs()
                    .max(radius_value.unwrap_or(0.0).abs())
                    .max(1.0);
                if radius_value.is_some_and(|value| (value - distance).abs() > 1e-9 * scale) {
                    return None;
                }
                radius_value.get_or_insert(distance);
                if distance > 1e-12 {
                    let derived_angle = delta[1].atan2(delta[0]);
                    if angle_value.is_some_and(|value| {
                        let difference = (value - derived_angle).rem_euclid(std::f64::consts::TAU);
                        difference.min(std::f64::consts::TAU - difference) > 1e-9
                    }) {
                        return None;
                    }
                    angle_value.get_or_insert(derived_angle);
                }
            }
            Some(SectionRadialConstraint {
                first: first_u.key,
                second: second_u.key,
                radius: (radius.variable_type, radius.key),
                angle: (angle.variable_type, angle.key),
                radius_value,
                angle_value,
            })
        })
        .collect()
}

pub(crate) fn resolved_section_scalar_values(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<(u32, u32), f64> {
    let coordinates = resolved_section_coordinates(definition);
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .map_or_else(BTreeSet::new, |variables| variables.reconciled_points().1);
    let mut values = BTreeMap::<(u32, u32), Option<f64>>::new();
    for (variable, value) in section_equation_scalar_equalities(definition) {
        values.insert(variable, Some(value));
    }
    for (variable, value) in
        section_equation_scalar_values_from_coordinates(definition, &coordinates)
    {
        merge_scalar_value_candidate(&mut values, variable, value);
    }
    for (variable, value) in
        section_equation_function_six_distance_values(definition, &coordinates, &BTreeSet::new())
    {
        merge_scalar_value_candidate(&mut values, variable, value);
    }
    for (variable, value) in section_equation_function_forty_three_axis_distance_values(
        definition,
        &coordinates,
        &BTreeSet::new(),
    ) {
        merge_scalar_value_candidate(&mut values, variable, value);
    }
    for (variable, value) in section_equation_function_sixteen_angle_difference_values(definition) {
        merge_scalar_value_candidate(&mut values, variable, value);
    }
    for constraint in
        section_equation_unsigned_coordinate_distances(definition, &ambiguous_point_ids)
    {
        merge_scalar_value_candidate(&mut values, constraint.scalar, constraint.value);
    }
    for constraint in section_equation_radius_dimensions(definition) {
        merge_scalar_value_candidate(&mut values, constraint.scalar, constraint.value);
    }
    for constraint in
        section_equation_radial_constraints(definition, &coordinates, &BTreeSet::new())
    {
        for (variable, value) in [
            (constraint.radius, constraint.radius_value),
            (constraint.angle, constraint.angle_value),
        ] {
            let Some(value) = value else {
                continue;
            };
            merge_scalar_value_candidate(&mut values, variable, value);
        }
    }
    values
        .into_iter()
        .filter_map(|(variable, value)| Some((variable, value?)))
        .collect()
}

pub(super) fn section_equation_function_sixteen_angle_difference_values(
    definition: &crate::feature::FeatureDefinition,
) -> Vec<(SectionScalarVariable, f64)> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    let row = |ordinal: u32| {
        usize::try_from(ordinal)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
        .filter_map(|equation| {
            if equation.function_id != 16 || equation.arguments.len() != 4 {
                return None;
            }
            let [Some(first), Some(second), Some(difference), Some(selector)] =
                equation.arguments.as_slice()
            else {
                return None;
            };
            let (Some(first), Some(second), Some(difference), Some(selector)) =
                (row(*first), row(*second), row(*difference), row(*selector))
            else {
                return None;
            };
            if first.variable_type != 4
                || second.variable_type != 4
                || difference.variable_type != 0
                || selector.variable_type != 5
                || selector.value != Some(0.0)
            {
                return None;
            }
            let (Some(first), Some(second)) = (first.value, second.value) else {
                return None;
            };
            if !first.is_finite() || !second.is_finite() || first < second {
                return None;
            }
            let value = first - second;
            if !value.is_finite() || value > std::f64::consts::PI {
                return None;
            }
            if difference.value.is_some_and(|stored| {
                !stored.is_finite() || stored < 0.0 || !approximately_equal(stored, value)
            }) {
                return None;
            }
            Some(((difference.variable_type, difference.key), value))
        })
        .collect()
}

pub(super) fn section_equation_function_six_distance_values(
    definition: &crate::feature::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<(SectionScalarVariable, f64)> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    let row = |ordinal: Option<u32>| {
        usize::try_from(ordinal?)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
        .filter_map(|equation| {
            if equation.function_id != 6 {
                return None;
            }
            let [Some(first_u), Some(first_v), Some(second_u), Some(second_v), Some(radius)] =
                equation.arguments.as_slice()
            else {
                return None;
            };
            let (Some(first_u), Some(first_v), Some(second_u), Some(second_v), Some(radius)) = (
                row(Some(*first_u)),
                row(Some(*first_v)),
                row(Some(*second_u)),
                row(Some(*second_v)),
                row(Some(*radius)),
            ) else {
                return None;
            };
            if first_u.variable_type != 1
                || first_v.variable_type != 2
                || first_u.key != first_v.key
                || second_u.variable_type != 1
                || second_v.variable_type != 2
                || second_u.key != second_v.key
                || first_u.key == second_u.key
                || radius.variable_type != 3
                || ambiguous_point_ids.contains(&first_u.key)
                || ambiguous_point_ids.contains(&second_u.key)
            {
                return None;
            }
            let first = coordinates
                .get(&first_u.key)
                .and_then(|point| Some([point[0]?, point[1]?]))?;
            let second = coordinates
                .get(&second_u.key)
                .and_then(|point| Some([point[0]?, point[1]?]))?;
            let delta = [second[0] - first[0], second[1] - first[1]];
            let distance = delta[0].hypot(delta[1]);
            if !distance.is_finite() || distance <= 0.0 {
                return None;
            }
            if radius.value.is_some_and(|stored| {
                !stored.is_finite()
                    || stored <= 0.0
                    || (stored - distance).abs() > 1e-9 * stored.abs().max(distance).max(1.0)
            }) {
                return None;
            }
            Some(((radius.variable_type, radius.key), distance))
        })
        .collect()
}

pub(super) fn section_equation_function_forty_three_axis_distance_values(
    definition: &crate::feature::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<(SectionScalarVariable, f64)> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    let row = |ordinal: u32| {
        usize::try_from(ordinal)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
        .filter_map(|equation| {
            if equation.function_id != 43 || equation.arguments.len() != 8 {
                return None;
            }
            let [
                Some(first_u),
                Some(first_v),
                Some(second_u),
                Some(second_v),
                Some(first_auxiliary),
                Some(second_auxiliary),
                Some(distance),
                Some(final_auxiliary),
            ] = equation.arguments.as_slice()
            else {
                return None;
            };
            let (Some(first_u), Some(first_v), Some(second_u), Some(second_v)) = (
                row(*first_u),
                row(*first_v),
                row(*second_u),
                row(*second_v),
            ) else {
                return None;
            };
            let (Some(first_auxiliary), Some(second_auxiliary), Some(distance), Some(final_auxiliary)) =
                (
                    row(*first_auxiliary),
                    row(*second_auxiliary),
                    row(*distance),
                    row(*final_auxiliary),
                )
            else {
                return None;
            };
            if first_u.variable_type != 1
                || first_v.variable_type != 2
                || first_u.key != first_v.key
                || second_u.variable_type != 1
                || second_v.variable_type != 2
                || second_u.key != second_v.key
                || first_u.key == second_u.key
                || !matches!(first_auxiliary.variable_type, 4 | 5)
                || !matches!(second_auxiliary.variable_type, 4 | 5)
                || distance.variable_type != 0
                || final_auxiliary.variable_type != 5
                || ambiguous_point_ids.contains(&first_u.key)
                || ambiguous_point_ids.contains(&second_u.key)
                || [first_auxiliary, second_auxiliary, final_auxiliary]
                    .into_iter()
                    .any(|row| {
                        row.value.is_some_and(|value| {
                            !value.is_finite()
                                || row.variable_type == 5 && value.abs() > 1e-12
                        })
                    })
            {
                return None;
            }
            let first = coordinates
                .get(&first_u.key)
                .and_then(|point| Some([point[0]?, point[1]?]))?;
            let second = coordinates
                .get(&second_u.key)
                .and_then(|point| Some([point[0]?, point[1]?]))?;
            let deltas = [
                (second[0] - first[0]).abs(),
                (second[1] - first[1]).abs(),
            ];
            if !deltas.into_iter().all(f64::is_finite) {
                return None;
            }
            let matches_distance = |value: f64| {
                deltas.iter().filter_map(move |delta| {
                    let scale = value.abs().max(delta.abs()).max(1.0);
                    ((*delta - value).abs() <= 1e-9 * scale).then_some(*delta)
                })
            };
            let value = if let Some(stored) = distance.value {
                if !stored.is_finite() || stored < 0.0 {
                    return None;
                }
                let mut matches = matches_distance(stored);
                let value = matches.next()?;
                matches.next().is_none().then_some(value)?
            } else {
                let mut nonzero = deltas
                    .iter()
                    .filter_map(|delta| (*delta > 1e-12).then_some(*delta));
                let value = nonzero.next()?;
                nonzero.next().is_none().then_some(value)?
            };
            Some(((distance.variable_type, distance.key), value))
        })
        .collect()
}

#[derive(Clone, Copy)]
pub(super) struct SectionUnsignedCoordinateDistance {
    pub(super) first: u32,
    pub(super) second: u32,
    pub(super) coordinate: usize,
    pub(super) scalar: SectionScalarVariable,
    pub(super) value: f64,
}

#[derive(Clone, Copy)]
pub(super) struct SectionRadiusDimension {
    pub(super) radius: u32,
    pub(super) scalar: SectionScalarVariable,
    pub(super) value: f64,
}

pub(super) fn section_equation_dimension_scalar_value(
    scalar: &crate::feature::FeatureVariableRow,
    dimension_value: f64,
    strictly_positive: bool,
) -> Option<f64> {
    let valid = |value: f64| {
        value.is_finite()
            && (strictly_positive && value > 0.0 || !strictly_positive && value >= 0.0)
    };
    if !valid(dimension_value) {
        return None;
    }
    match scalar.value {
        Some(value) if valid(value) && approximately_equal(value, dimension_value) => {
            Some(dimension_value)
        }
        None if scalar.dimension_driven => Some(dimension_value),
        _ => None,
    }
}

pub(super) fn section_equation_unsigned_coordinate_distances(
    definition: &crate::feature::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionUnsignedCoordinateDistance> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(dimensions) = definition
        .dimensions
        .as_ref()
        .filter(|table| feature_dimension_table_complete(table))
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
        .filter(|equation| equation.function_id == 3 && equation.arguments.len() == 3)
        .filter_map(|equation| {
            let [Some(first), Some(second), Some(dimension)] = equation.arguments.as_slice() else {
                return None;
            };
            let first = variables.rows.get(usize::try_from(*first).ok()?)?;
            let second = variables.rows.get(usize::try_from(*second).ok()?)?;
            let dimension = variables.rows.get(usize::try_from(*dimension).ok()?)?;
            if first.variable_type != second.variable_type
                || !matches!(first.variable_type, 1 | 2)
                || dimension.variable_type != 0
                || ambiguous_point_ids.contains(&first.key)
                || ambiguous_point_ids.contains(&second.key)
                || first.key == second.key
            {
                return None;
            }
            let dimension_row = dimensions.rows.get(usize::try_from(dimension.key).ok()?)?;
            if dimension_row.value_unit != crate::feature::DimensionUnit::Millimeters
                || !matches!(dimension_row.dimension_type, 1..=5)
            {
                return None;
            }
            let value =
                section_equation_dimension_scalar_value(dimension, dimension_row.value?, false)?;
            Some(SectionUnsignedCoordinateDistance {
                first: first.key,
                second: second.key,
                coordinate: usize::from(first.variable_type == 2),
                scalar: (dimension.variable_type, dimension.key),
                value,
            })
        })
        .collect()
}

pub(super) fn section_equation_radius_dimensions(
    definition: &crate::feature::FeatureDefinition,
) -> Vec<SectionRadiusDimension> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(dimensions) = definition
        .dimensions
        .as_ref()
        .filter(|table| feature_dimension_table_complete(table))
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
        .filter(|equation| equation.function_id == 2 && equation.arguments.len() == 2)
        .filter_map(|equation| {
            let [Some(first), Some(second)] = equation.arguments.as_slice() else {
                return None;
            };
            let first = variables.rows.get(usize::try_from(*first).ok()?)?;
            let second = variables.rows.get(usize::try_from(*second).ok()?)?;
            let (radius, scalar) = match (first.variable_type, second.variable_type) {
                (3, 0) => (first, second),
                (0, 3) => (second, first),
                _ => return None,
            };
            let dimension = dimensions.rows.get(usize::try_from(scalar.key).ok()?)?;
            let dimension_value = dimension.value?;
            if dimension.dimension_type != 3
                || dimension.value_unit != crate::feature::DimensionUnit::Millimeters
                || radius.value.is_some_and(|value| {
                    !value.is_finite()
                        || value <= 0.0
                        || (value - dimension_value).abs()
                            > 1e-9 * value.abs().max(dimension_value.abs()).max(1.0)
                })
            {
                return None;
            }
            let value = section_equation_dimension_scalar_value(scalar, dimension_value, true)?;
            Some(SectionRadiusDimension {
                radius: radius.key,
                scalar: (scalar.variable_type, scalar.key),
                value,
            })
        })
        .collect()
}

pub(super) fn section_equation_point_on_line_constraints(
    definition: &crate::feature::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<(u32, u32, u32)> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
        .filter(|equation| equation.function_id == 35 && equation.arguments.len() == 9)
        .filter_map(|equation| {
            let [
                Some(target_u),
                Some(target_v),
                Some(first_u),
                Some(first_v),
                Some(second_u),
                Some(second_v),
                Some(line_parameter),
                Some(first_zero),
                Some(second_zero),
            ] = equation.arguments.as_slice()
            else {
                return None;
            };
            let target_u = variables.rows.get(usize::try_from(*target_u).ok()?)?;
            let target_v = variables.rows.get(usize::try_from(*target_v).ok()?)?;
            let first_u = variables.rows.get(usize::try_from(*first_u).ok()?)?;
            let first_v = variables.rows.get(usize::try_from(*first_v).ok()?)?;
            let second_u = variables.rows.get(usize::try_from(*second_u).ok()?)?;
            let second_v = variables.rows.get(usize::try_from(*second_v).ok()?)?;
            let line_parameter = variables
                .rows
                .get(usize::try_from(*line_parameter).ok()?)?;
            let first_zero = variables.rows.get(usize::try_from(*first_zero).ok()?)?;
            let second_zero = variables.rows.get(usize::try_from(*second_zero).ok()?)?;
            if target_u.variable_type != 1
                || target_v.variable_type != 2
                || first_u.variable_type != 1
                || first_v.variable_type != 2
                || second_u.variable_type != 1
                || second_v.variable_type != 2
                || target_u.key != target_v.key
                || first_u.key != first_v.key
                || second_u.key != second_v.key
                || target_u.key == first_u.key
                || target_u.key == second_u.key
                || first_u.key == second_u.key
                || line_parameter.variable_type != 4
                || first_zero.variable_type != 5
                || second_zero.variable_type != 5
                || first_zero.value != Some(0.0)
                || second_zero.value != Some(0.0)
                || ambiguous_point_ids.contains(&target_u.key)
                || ambiguous_point_ids.contains(&first_u.key)
                || ambiguous_point_ids.contains(&second_u.key)
            {
                return None;
            }
            Some((target_u.key, first_u.key, second_u.key))
        })
        .collect()
}

#[derive(Clone, Copy)]
pub(super) struct SectionEqualLengthConstraint {
    pub(super) first: [u32; 2],
    pub(super) second: [u32; 2],
}

pub(super) fn section_equation_equal_length_constraints(
    definition: &crate::feature::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionEqualLengthConstraint> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
        .filter(|equation| equation.function_id == 33 && equation.arguments.len() == 9)
        .filter_map(|equation| {
            let mut rows = Vec::with_capacity(equation.arguments.len());
            for ordinal in &equation.arguments {
                rows.push(variables.rows.get(usize::try_from((*ordinal)?).ok()?)?);
            }
            let [first_u, first_v, second_u, second_v, third_u, third_v, fourth_u, fourth_v, auxiliary] =
                rows.as_slice()
            else {
                return None;
            };
            if first_u.variable_type != 1
                || first_v.variable_type != 2
                || second_u.variable_type != 1
                || second_v.variable_type != 2
                || third_u.variable_type != 1
                || third_v.variable_type != 2
                || fourth_u.variable_type != 1
                || fourth_v.variable_type != 2
                || auxiliary.variable_type != 7
                || auxiliary.value != Some(0.0)
                || first_u.key != first_v.key
                || second_u.key != second_v.key
                || third_u.key != third_v.key
                || fourth_u.key != fourth_v.key
                || first_u.key == second_u.key
                || third_u.key == fourth_u.key
                || [first_u.key, second_u.key, third_u.key, fourth_u.key]
                    .into_iter()
                    .any(|point_id| ambiguous_point_ids.contains(&point_id))
            {
                return None;
            }
            Some(SectionEqualLengthConstraint {
                first: [first_u.key, second_u.key],
                second: [third_u.key, fourth_u.key],
            })
        })
        .collect()
}

pub(super) type SectionCoordinateVariable = (u32, usize);

#[derive(Clone, Default)]
pub(super) struct SectionCoordinateEquation {
    pub(super) terms: BTreeMap<SectionCoordinateVariable, f64>,
    pub(super) rhs: f64,
}

impl SectionCoordinateEquation {
    pub(super) fn point_value(point: u32, coordinate: usize, value: f64) -> Self {
        let mut equation = Self::default();
        equation.add_point(point, coordinate, 1.0);
        equation.rhs = value;
        equation
    }

    pub(super) fn point_difference(first: u32, second: u32, coordinate: usize, delta: f64) -> Self {
        let mut equation = Self::default();
        equation.add_point(first, coordinate, -1.0);
        equation.add_point(second, coordinate, 1.0);
        equation.rhs = delta;
        equation
    }

    pub(super) fn source_difference(
        first: SectionPointSource,
        second: SectionPointSource,
        coordinate: usize,
        delta: f64,
    ) -> Self {
        let mut equation = Self::default();
        equation.add_source(first, coordinate, -1.0);
        equation.add_source(second, coordinate, 1.0);
        equation.rhs += delta;
        equation
    }

    pub(super) fn add_point(&mut self, point: u32, coordinate: usize, coefficient: f64) {
        *self.terms.entry((point, coordinate)).or_default() += coefficient;
    }

    pub(super) fn add_source(
        &mut self,
        source: SectionPointSource,
        coordinate: usize,
        coefficient: f64,
    ) {
        match source {
            SectionPointSource::Point(point) => self.add_point(point, coordinate, coefficient),
            SectionPointSource::Value(value) => self.rhs -= coefficient * value[coordinate],
        }
    }
}

pub(super) fn solve_unsigned_dimension_coordinates(
    equations: &[SectionCoordinateEquation],
    stored_coordinates: &BTreeMap<SectionCoordinateVariable, f64>,
    distances: &[(u32, u32, usize, f64)],
) -> BTreeMap<SectionCoordinateVariable, f64> {
    const MAX_SIGNED_BRANCHES: usize = 4096;
    if distances.is_empty() {
        return BTreeMap::new();
    }

    let variables = equations
        .iter()
        .flat_map(|equation| equation.terms.keys().copied())
        .chain(
            distances
                .iter()
                .flat_map(|&(first, second, coordinate, _)| {
                    [(first, coordinate), (second, coordinate)]
                }),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let indices = variables
        .iter()
        .enumerate()
        .map(|(index, variable)| (*variable, index))
        .collect::<BTreeMap<_, _>>();
    let mut adjacency = vec![BTreeSet::new(); variables.len()];
    let connect = |members: Vec<usize>, adjacency: &mut [BTreeSet<usize>]| {
        for &first in &members {
            adjacency[first].extend(members.iter().copied().filter(|second| *second != first));
        }
    };
    for equation in equations {
        connect(
            equation
                .terms
                .keys()
                .filter_map(|variable| indices.get(variable).copied())
                .collect(),
            &mut adjacency,
        );
    }
    for &(first, second, coordinate, _) in distances {
        connect(
            [
                indices[&(first, coordinate)],
                indices[&(second, coordinate)],
            ]
            .into_iter()
            .collect(),
            &mut adjacency,
        );
    }

    let mut remaining = (0..variables.len()).collect::<BTreeSet<_>>();
    let mut resolved = BTreeMap::new();
    while let Some(seed) = remaining.pop_first() {
        let mut component = BTreeSet::from([seed]);
        let mut pending = std::collections::VecDeque::from([seed]);
        while let Some(variable) = pending.pop_front() {
            for &neighbor in &adjacency[variable] {
                if component.insert(neighbor) {
                    remaining.remove(&neighbor);
                    pending.push_back(neighbor);
                }
            }
        }
        let component_distances = distances
            .iter()
            .copied()
            .filter(|&(first, second, coordinate, _)| {
                component.contains(&indices[&(first, coordinate)])
                    && component.contains(&indices[&(second, coordinate)])
            })
            .collect::<Vec<_>>();
        if component_distances.is_empty()
            || component_distances.len() >= usize::BITS as usize
            || (1usize << component_distances.len()) > MAX_SIGNED_BRANCHES
        {
            continue;
        }
        let component_equations = equations
            .iter()
            .filter(|equation| {
                equation
                    .terms
                    .keys()
                    .any(|variable| component.contains(&indices[variable]))
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut solutions = Vec::new();
        for signs in 0..(1usize << component_distances.len()) {
            let mut branched = component_equations.clone();
            for (index, &(first, second, coordinate, magnitude)) in
                component_distances.iter().enumerate()
            {
                let delta = if signs & (1usize << index) == 0 {
                    magnitude
                } else {
                    -magnitude
                };
                branched.push(SectionCoordinateEquation::point_difference(
                    first, second, coordinate, delta,
                ));
            }
            let candidate = solve_section_coordinate_equations(&branched, stored_coordinates);
            let mut values = stored_coordinates.clone();
            for (point, coordinates) in &candidate {
                for (coordinate, value) in coordinates.iter().copied().enumerate() {
                    if let Some(value) = value {
                        values.insert((*point, coordinate), value);
                    }
                }
            }
            let valid = component_equations.iter().all(|equation| {
                let Some(lhs) = equation
                    .terms
                    .iter()
                    .try_fold(0.0, |lhs, (variable, coefficient)| {
                        Some(lhs + values.get(variable)? * coefficient)
                    })
                else {
                    return true;
                };
                let scale = lhs.abs().max(equation.rhs.abs()).max(1.0);
                (lhs - equation.rhs).abs() <= 1e-9 * scale
            }) && component_distances.iter().all(
                |&(first, second, coordinate, magnitude)| {
                    let Some(first) = values.get(&(first, coordinate)).copied() else {
                        return false;
                    };
                    let Some(second) = values.get(&(second, coordinate)).copied() else {
                        return false;
                    };
                    let scale = first.abs().max(second.abs()).max(magnitude).max(1.0);
                    ((second - first).abs() - magnitude).abs() <= 1e-9 * scale
                },
            );
            if valid {
                let mut candidate_values = BTreeMap::new();
                for (point, coordinates) in candidate {
                    for (coordinate, value) in coordinates.into_iter().enumerate() {
                        let variable = (point, coordinate);
                        if let (Some(global), Some(value)) = (indices.get(&variable), value) {
                            if component.contains(global)
                                && !stored_coordinates.contains_key(&variable)
                            {
                                candidate_values.insert(variable, value);
                            }
                        }
                    }
                }
                solutions.push(candidate_values);
            }
        }
        for &global in &component {
            let variable = variables[global];
            let Some(value) = solutions
                .first()
                .and_then(|solution| solution.get(&variable))
                .copied()
            else {
                continue;
            };
            let scale = value.abs().max(1.0);
            if solutions.iter().all(|solution| {
                solution
                    .get(&variable)
                    .is_some_and(|candidate| (*candidate - value).abs() <= 1e-9 * scale)
            }) {
                resolved.insert(variable, value);
            }
        }
    }
    resolved
}

pub(super) fn section_equal_length_coordinate_values(
    constraints: &[SectionEqualLengthConstraint],
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
) -> BTreeMap<SectionCoordinateVariable, Option<f64>> {
    let mut candidates = BTreeMap::<SectionCoordinateVariable, Option<f64>>::new();
    for constraint in constraints {
        let variables = constraint
            .first
            .into_iter()
            .chain(constraint.second)
            .flat_map(|point| [(point, 0), (point, 1)])
            .collect::<BTreeSet<_>>();
        let missing = variables
            .iter()
            .copied()
            .filter(|variable| {
                coordinates
                    .get(&variable.0)
                    .and_then(|point| point[variable.1])
                    .is_none()
            })
            .collect::<Vec<_>>();
        let [missing] = missing.as_slice() else {
            continue;
        };

        let component = |first: u32, second: u32, coordinate: usize| -> Option<(f64, f64)> {
            let value = |point: u32| {
                if (point, coordinate) == *missing {
                    Some((1.0, 0.0))
                } else {
                    coordinates
                        .get(&point)
                        .and_then(|coordinates| coordinates.get(coordinate).copied().flatten())
                        .map(|value| (0.0, value))
                }
            };
            let (first_coefficient, first_value) = value(first)?;
            let (second_coefficient, second_value) = value(second)?;
            Some((
                second_coefficient - first_coefficient,
                second_value - first_value,
            ))
        };
        let Some((first_u_coefficient, first_u_value)) =
            component(constraint.first[0], constraint.first[1], 0)
        else {
            continue;
        };
        let Some((first_v_coefficient, first_v_value)) =
            component(constraint.first[0], constraint.first[1], 1)
        else {
            continue;
        };
        let Some((second_u_coefficient, second_u_value)) =
            component(constraint.second[0], constraint.second[1], 0)
        else {
            continue;
        };
        let Some((second_v_coefficient, second_v_value)) =
            component(constraint.second[0], constraint.second[1], 1)
        else {
            continue;
        };

        let square = |coefficient: f64, value: f64| {
            (
                coefficient * coefficient,
                2.0 * coefficient * value,
                value * value,
            )
        };
        let first_u = square(first_u_coefficient, first_u_value);
        let first_v = square(first_v_coefficient, first_v_value);
        let second_u = square(second_u_coefficient, second_u_value);
        let second_v = square(second_v_coefficient, second_v_value);
        let quadratic = (
            second_u.0 + second_v.0 - first_u.0 - first_v.0,
            second_u.1 + second_v.1 - first_u.1 - first_v.1,
            second_u.2 + second_v.2 - first_u.2 - first_v.2,
        );
        let roots = quadratic_roots(quadratic);
        let [value] = roots.as_slice() else {
            continue;
        };
        candidates
            .entry(*missing)
            .and_modify(|candidate| {
                if candidate.is_some_and(|candidate| !approximately_equal(candidate, *value)) {
                    *candidate = None;
                }
            })
            .or_insert(Some(*value));
    }
    candidates
}

pub(super) fn quadratic_roots((quadratic, linear, constant): (f64, f64, f64)) -> Vec<f64> {
    let scale = quadratic
        .abs()
        .max(linear.abs())
        .max(constant.abs())
        .max(1.0);
    let tolerance = 1e-12 * scale;
    let mut roots = if quadratic.abs() <= tolerance {
        if linear.abs() <= tolerance {
            Vec::new()
        } else {
            vec![-constant / linear]
        }
    } else {
        let discriminant = linear * linear - 4.0 * quadratic * constant;
        let discriminant_tolerance =
            1e-12 * (linear * linear + (4.0 * quadratic * constant).abs()).max(1.0);
        if discriminant < -discriminant_tolerance {
            Vec::new()
        } else if discriminant.abs() <= discriminant_tolerance {
            vec![-linear / (2.0 * quadratic)]
        } else {
            let root = discriminant.sqrt();
            vec![
                (-linear - root) / (2.0 * quadratic),
                (-linear + root) / (2.0 * quadratic),
            ]
        }
    };
    roots.retain(|root| {
        root.is_finite()
            && (quadratic * root * root + linear * root + constant).abs()
                <= 1e-9
                    * (quadratic * root * root)
                        .abs()
                        .max((linear * root).abs())
                        .max(constant.abs())
                        .max(1.0)
    });
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|first, second| approximately_equal(*first, *second));
    roots
}

pub(super) fn approximately_equal(first: f64, second: f64) -> bool {
    let scale = first.abs().max(second.abs()).max(1.0);
    (first - second).abs() <= 1e-9 * scale
}

pub(super) fn solve_section_coordinate_equations(
    equations: &[SectionCoordinateEquation],
    stored_coordinates: &BTreeMap<SectionCoordinateVariable, f64>,
) -> BTreeMap<u32, [Option<f64>; 2]> {
    let variables = equations
        .iter()
        .flat_map(|equation| equation.terms.keys().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let indices = variables
        .iter()
        .enumerate()
        .map(|(index, variable)| (*variable, index))
        .collect::<BTreeMap<_, _>>();
    let mut adjacency = vec![BTreeSet::new(); variables.len()];
    let mut variable_equations = vec![BTreeSet::new(); variables.len()];
    for (equation_index, equation) in equations.iter().enumerate() {
        let members = equation
            .terms
            .keys()
            .filter_map(|variable| indices.get(variable).copied())
            .collect::<Vec<_>>();
        for &first in &members {
            adjacency[first].extend(members.iter().copied().filter(|second| *second != first));
            variable_equations[first].insert(equation_index);
        }
    }
    let mut solved = BTreeMap::<SectionCoordinateVariable, f64>::new();
    let mut remaining = (0..variables.len()).collect::<BTreeSet<_>>();
    while let Some(seed) = remaining.pop_first() {
        let mut component = BTreeSet::from([seed]);
        let mut pending = std::collections::VecDeque::from([seed]);
        while let Some(variable) = pending.pop_front() {
            for &neighbor in &adjacency[variable] {
                if component.insert(neighbor) {
                    remaining.remove(&neighbor);
                    pending.push_back(neighbor);
                }
            }
        }
        let columns = component.iter().copied().collect::<Vec<_>>();
        let local_columns = columns
            .iter()
            .enumerate()
            .map(|(local, global)| (*global, local))
            .collect::<BTreeMap<_, _>>();
        let component_equations = component
            .iter()
            .flat_map(|variable| variable_equations[*variable].iter().copied())
            .collect::<BTreeSet<_>>();
        let mut matrix = component_equations
            .into_iter()
            .map(|equation_index| &equations[equation_index])
            .map(|equation| {
                let mut row = SectionLinearRow {
                    coefficients: BTreeMap::new(),
                    rhs: equation.rhs,
                };
                for (variable, coefficient) in &equation.terms {
                    let global = indices[variable];
                    if *coefficient != 0.0 {
                        row.coefficients
                            .insert(local_columns[&global], *coefficient);
                    }
                }
                row
            })
            .collect::<Vec<_>>();
        let Some(component_solution) = uniquely_solved_linear_variables(&mut matrix, columns.len())
        else {
            for global in columns {
                let variable = variables[global];
                if let Some(value) = stored_coordinates.get(&variable) {
                    solved.insert(variable, *value);
                }
            }
            continue;
        };
        for (local, value) in component_solution {
            solved.insert(variables[columns[local]], value);
        }
    }
    let mut points = BTreeMap::<u32, [Option<f64>; 2]>::new();
    for ((point, coordinate), value) in solved {
        points.entry(point).or_insert([None; 2])[coordinate] = Some(value);
    }
    points
}

pub(super) struct SectionLinearRow {
    pub(super) coefficients: BTreeMap<usize, f64>,
    pub(super) rhs: f64,
}

pub(super) fn uniquely_solved_linear_variables(
    matrix: &mut [SectionLinearRow],
    variable_count: usize,
) -> Option<Vec<(usize, f64)>> {
    let coefficient_scale = matrix
        .iter()
        .flat_map(|row| row.coefficients.values())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let rhs_scale = matrix.iter().map(|row| row.rhs.abs()).fold(1.0, f64::max);
    let coefficient_tolerance = 1e-12 * coefficient_scale;
    let residual_tolerance = 1e-9 * rhs_scale;
    let mut pivot_rows = BTreeMap::new();
    let mut pivot_row = 0;
    for column in 0..variable_count {
        let Some(selected) = (pivot_row..matrix.len()).max_by(|&first, &second| {
            matrix[first]
                .coefficients
                .get(&column)
                .copied()
                .unwrap_or(0.0)
                .abs()
                .total_cmp(
                    &matrix[second]
                        .coefficients
                        .get(&column)
                        .copied()
                        .unwrap_or(0.0)
                        .abs(),
                )
        }) else {
            break;
        };
        let divisor = matrix[selected]
            .coefficients
            .get(&column)
            .copied()
            .unwrap_or(0.0);
        if divisor.abs() <= coefficient_tolerance {
            continue;
        }
        matrix.swap(pivot_row, selected);
        for value in matrix[pivot_row].coefficients.values_mut() {
            *value /= divisor;
        }
        matrix[pivot_row].rhs /= divisor;
        let pivot_coefficients = matrix[pivot_row].coefficients.clone();
        let pivot_rhs = matrix[pivot_row].rhs;
        for (row, target) in matrix.iter_mut().enumerate() {
            if row == pivot_row {
                continue;
            }
            let factor = target.coefficients.get(&column).copied().unwrap_or(0.0);
            if factor.abs() <= coefficient_tolerance {
                continue;
            }
            for (&index, &pivot_value) in &pivot_coefficients {
                let value = target.coefficients.entry(index).or_default();
                *value -= factor * pivot_value;
                if value.abs() <= coefficient_tolerance {
                    target.coefficients.remove(&index);
                }
            }
            target.rhs -= factor * pivot_rhs;
        }
        pivot_rows.insert(column, pivot_row);
        pivot_row += 1;
    }
    if matrix
        .iter()
        .any(|row| row.coefficients.is_empty() && row.rhs.abs() > residual_tolerance)
    {
        return None;
    }
    let free_columns = (0..variable_count)
        .filter(|column| !pivot_rows.contains_key(column))
        .collect::<Vec<_>>();
    Some(
        pivot_rows
            .into_iter()
            .filter_map(|(column, row)| {
                free_columns
                    .iter()
                    .all(|free| !matrix[row].coefficients.contains_key(free))
                    .then_some((column, matrix[row].rhs))
            })
            .collect(),
    )
}

pub(super) fn section_line_fixed_coordinate(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<usize> {
    let segment = unique_section_skamp_segment(definition, segment.external_id)?;
    (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    section_line_entity_fixed_coordinate(definition, segment.external_id)
}

pub(super) fn section_line_entity_fixed_coordinate(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> Option<usize> {
    let mut adjacency = BTreeMap::<u32, Vec<(u32, usize)>>::new();
    for skamp in active_complete_section_skamps(definition) {
        let (parity, first, second) = match (skamp.kind, skamp.items.as_slice()) {
            (5 | 7, [first, second]) if first.sense == 0 && second.sense == 0 => {
                ((skamp.kind == 5) as usize, first, second)
            }
            _ => continue,
        };
        if !section_skamp_is_line(definition, first) || !section_skamp_is_line(definition, second) {
            continue;
        }
        adjacency
            .entry(first.entity_id)
            .or_default()
            .push((second.entity_id, parity));
        adjacency
            .entry(second.entity_id)
            .or_default()
            .push((first.entity_id, parity));
    }
    let mut parities = BTreeMap::from([(entity_id, 0usize)]);
    let mut pending = std::collections::VecDeque::from([entity_id]);
    while let Some(entity_id) = pending.pop_front() {
        let parity = parities[&entity_id];
        for &(neighbor, edge_parity) in adjacency.get(&entity_id).into_iter().flatten() {
            let neighbor_parity = parity ^ edge_parity;
            match parities.get(&neighbor) {
                Some(stored) if *stored != neighbor_parity => return None,
                Some(_) => {}
                None => {
                    parities.insert(neighbor, neighbor_parity);
                    pending.push_back(neighbor);
                }
            }
        }
    }
    let mut coordinates = BTreeSet::new();
    for (entity_id, parity) in parities {
        coordinates.extend(
            section_line_direct_fixed_coordinates(definition, entity_id)
                .into_iter()
                .map(|coordinate| coordinate ^ parity),
        );
    }
    coordinates
        .first()
        .copied()
        .filter(|_| coordinates.len() == 1)
}

pub(super) fn section_line_direct_fixed_coordinates(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> BTreeSet<usize> {
    let mut coordinates = unique_section_skamp_segment(definition, entity_id)
        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Line)
        .and_then(|segment| segment.vertical_horizontal)
        .and_then(|selector| match selector {
            0 => Some(0),
            1 => Some(1),
            _ => None,
        })
        .into_iter()
        .collect::<BTreeSet<_>>();
    coordinates.extend(
        unique_reference_line_segment(definition, entity_id)
            .and_then(|segment| segment.vertical_horizontal)
            .and_then(|selector| match selector {
                0 => Some(0),
                1 => Some(1),
                _ => None,
            }),
    );
    coordinates.extend(
        active_complete_section_skamps(definition).filter_map(|skamp| {
            match (skamp.kind, skamp.items.as_slice()) {
                (1, [item]) if item.sense == 0 && item.entity_id == entity_id => Some(1),
                (2, [item]) if item.sense == 0 && item.entity_id == entity_id => Some(0),
                _ => None,
            }
        }),
    );
    if let Some(crate::feature::FeatureSavedEntity::Line(line)) =
        section_saved_entity(definition, entity_id)
    {
        let [[Some(x0), Some(y0), _], [Some(x1), Some(y1), _]] = line.endpoints else {
            return coordinates;
        };
        let scale = [x0, y0, x1, y1]
            .into_iter()
            .map(f64::abs)
            .fold(1.0, f64::max);
        let tolerance = 1e-9 * scale;
        match [(x0 - x1).abs() <= tolerance, (y0 - y1).abs() <= tolerance] {
            [true, false] => {
                coordinates.insert(0);
            }
            [false, true] => {
                coordinates.insert(1);
            }
            _ => {}
        }
    }
    coordinates
}

pub(super) fn section_skamp_point_on_line(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<(u32, u32, usize)> {
    let [first, second] = skamp.items.as_slice() else {
        return None;
    };
    let pair = match skamp.kind {
        3 => [(first, second), (second, first)]
            .into_iter()
            .find_map(|(line_item, point_item)| {
                let line = unique_section_skamp_segment(definition, line_item.entity_id)?;
                (line_item.sense == 0 && line.kind == crate::feature::FeatureSegmentKind::Line)
                    .then_some((
                        line,
                        section_skamp_selected_point_id(definition, point_item)?,
                    ))
            }),
        9 => [(first, second), (second, first)]
            .into_iter()
            .find_map(|(line_item, point_item)| {
                let line = unique_section_skamp_segment(definition, line_item.entity_id)?;
                let point = unique_section_skamp_segment(definition, point_item.entity_id)?;
                (line_item.sense == 0
                    && point_item.sense == 0
                    && line.kind == crate::feature::FeatureSegmentKind::Line
                    && point.kind == crate::feature::FeatureSegmentKind::Point)
                    .then_some((line, point.point_ids[0]))
            }),
        _ => None,
    }?;
    let coordinate = section_line_fixed_coordinate(definition, pair.0)?;
    Some((pair.0.point_ids[0], pair.1, coordinate))
}

pub(super) fn section_skamp_saved_point_on_line(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<(u32, usize, f64)> {
    let [first, second] = skamp.items.as_slice() else {
        return None;
    };
    let (line_item, point_id) = match skamp.kind {
        3 => [(first, second), (second, first)]
            .into_iter()
            .find_map(|(line_item, point_item)| {
                if line_item.sense != 0 {
                    return None;
                }
                Some((
                    line_item,
                    section_skamp_selected_point_id(definition, point_item)?,
                ))
            }),
        9 => [(first, second), (second, first)]
            .into_iter()
            .find_map(|(line_item, point_item)| {
                if line_item.sense != 0
                    || point_item.sense != 0
                    || !section_skamp_is_point(definition, point_item)
                {
                    return None;
                }
                Some((
                    line_item,
                    unique_section_skamp_segment(definition, point_item.entity_id)?.point_ids[0],
                ))
            }),
        _ => None,
    }?;
    if definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .any(|segment| segment.external_id == line_item.entity_id)
    {
        return None;
    }
    let crate::feature::FeatureSavedEntity::Line(line) =
        section_saved_entity(definition, line_item.entity_id)?
    else {
        return None;
    };
    let coordinate = section_line_entity_fixed_coordinate(definition, line_item.entity_id)?;
    Some((
        point_id,
        coordinate,
        saved_line_fixed_coordinate_value(line, coordinate)?,
    ))
}

#[derive(Clone, Copy)]
pub(super) enum SectionSymmetryAxis {
    Point(u32),
    Value(f64),
}

pub(super) fn section_skamp_axis_symmetry(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<(
    SectionSymmetryAxis,
    SectionPointSource,
    SectionPointSource,
    usize,
)> {
    let (14, [axis_item, first_item, second_item]) = (skamp.kind, skamp.items.as_slice()) else {
        return None;
    };
    (axis_item.sense == 0 && section_skamp_is_line(definition, axis_item)).then_some(())?;
    let coordinate = section_line_entity_fixed_coordinate(definition, axis_item.entity_id)?;
    let axis = if let Some(segment) = unique_section_skamp_segment(definition, axis_item.entity_id)
    {
        SectionSymmetryAxis::Point(segment.point_ids[0])
    } else {
        let crate::feature::FeatureSavedEntity::Line(line) =
            section_saved_entity(definition, axis_item.entity_id)?
        else {
            return None;
        };
        SectionSymmetryAxis::Value(saved_line_fixed_coordinate_value(line, coordinate)?)
    };
    Some((
        axis,
        section_skamp_selected_point(definition, first_item)?,
        section_skamp_selected_point(definition, second_item)?,
        coordinate,
    ))
}

pub(super) fn section_skamp_point_symmetry(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<(u32, SectionPointSource, SectionPointSource)> {
    let (14, [center, first, second]) = (skamp.kind, skamp.items.as_slice()) else {
        return None;
    };
    Some((
        section_skamp_point_entity_id(definition, center)?,
        section_skamp_selected_point(definition, first)?,
        section_skamp_selected_point(definition, second)?,
    ))
}

pub(super) fn saved_line_fixed_coordinate_value(
    line: &crate::feature::FeatureSavedLine,
    coordinate: usize,
) -> Option<f64> {
    let [Some(first), Some(second)] =
        [line.endpoints[0][coordinate], line.endpoints[1][coordinate]]
    else {
        return None;
    };
    let scale = first.abs().max(second.abs()).max(1.0);
    ((first - second).abs() <= 1e-9 * scale).then_some(first)
}

#[derive(Clone, Copy)]
pub(super) enum SectionPointSource {
    Point(u32),
    Value([f64; 2]),
}

pub(super) fn unique_section_skamp_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureSegment> {
    definition.segments.as_ref()?.segment(external_id)
}

pub(super) fn unique_decoded_section_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureSegment> {
    let segments = definition.segments.as_ref()?;
    let segment = segments
        .rows
        .iter()
        .find(|segment| segment.external_id == external_id)?;
    (segments.external_id_count(external_id) == 1).then_some(segment)
}

pub(super) fn section_segment_rows(
    definition: &crate::feature::FeatureDefinition,
) -> &[crate::feature::FeatureSegment] {
    definition
        .segments
        .as_ref()
        .map_or(&[], |table| table.rows.as_slice())
}

pub(super) fn complete_section_segment_rows(
    definition: &crate::feature::FeatureDefinition,
) -> &[crate::feature::FeatureSegment] {
    definition
        .segments
        .as_ref()
        .filter(|table| table.is_complete())
        .map_or(&[], |table| table.rows.as_slice())
}

pub(super) fn section_skamp_point_entity_id(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<u32> {
    if let Some(point) = unique_point_segment(definition, item.entity_id) {
        return (item.sense == 0).then_some(point.point_id);
    }
    let segment = unique_section_skamp_segment(definition, item.entity_id)?;
    (item.sense == 0 && segment.kind == crate::feature::FeatureSegmentKind::Point)
        .then_some(segment.point_ids[0])
}

pub(super) fn section_skamp_selected_point_id(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<u32> {
    if let Some(segment) = unique_centered_line_segment(definition, item.entity_id) {
        return match item.sense {
            2 => Some(0),
            3 => Some(1),
            4 => Some(segment.center_id),
            _ => None,
        };
    }
    if let Some(point) = unique_point_segment(definition, item.entity_id) {
        return matches!(item.sense, 0 | 4).then_some(point.point_id);
    }
    if let Some(circle) = unique_circle_segment(definition, item.entity_id) {
        return (item.sense == 4).then_some(circle.center_id);
    }
    let segment = unique_section_skamp_segment(definition, item.entity_id)?;
    if segment.kind == crate::feature::FeatureSegmentKind::Point {
        return matches!(item.sense, 0 | 4).then_some(segment.point_ids[0]);
    }
    match item.sense {
        2 => Some(segment.point_ids[0]),
        3 => Some(segment.point_ids[1]),
        4 => segment.center_id,
        _ => None,
    }
}

pub(super) fn section_skamp_selected_point(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SectionPointSource> {
    section_skamp_selected_point_id(definition, item)
        .map(SectionPointSource::Point)
        .or_else(|| saved_section_point(definition, item).map(SectionPointSource::Value))
}

pub(super) fn saved_section_point(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<[f64; 2]> {
    if definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .any(|segment| segment.external_id == item.entity_id)
    {
        return None;
    }
    let coordinates = match (
        section_saved_entity(definition, item.entity_id)?,
        item.sense,
    ) {
        (crate::feature::FeatureSavedEntity::Line(line), 2) => line.endpoints[0],
        (crate::feature::FeatureSavedEntity::Line(line), 3) => line.endpoints[1],
        (crate::feature::FeatureSavedEntity::Arc(arc), 2) => arc.endpoints[0],
        (crate::feature::FeatureSavedEntity::Arc(arc), 3) => arc.endpoints[1],
        (crate::feature::FeatureSavedEntity::Arc(arc), 4) => arc.center,
        (crate::feature::FeatureSavedEntity::Circle(circle), 4) => circle.center,
        (crate::feature::FeatureSavedEntity::Conic(conic), 4) => {
            let frame = conic.local_system?;
            [Some(frame[9]), Some(frame[10]), Some(frame[11])]
        }
        _ => return None,
    };
    let [Some(u), Some(v), _] = coordinates else {
        return None;
    };
    (u.is_finite() && v.is_finite()).then_some([u, v])
}

pub(crate) fn resolved_section_radii(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, f64> {
    let mut candidates = BTreeMap::<u32, Vec<f64>>::new();
    for segment in definition
        .segments
        .iter()
        .filter(|table| table.is_complete())
        .flat_map(|table| &table.circle_rows)
    {
        if let Some((_, radius)) = saved_section_circle_values(definition, segment) {
            candidates
                .entry(segment.radius_ref)
                .or_default()
                .push(radius);
        }
    }
    for row in definition
        .variables
        .iter()
        .filter(|table| table.is_complete())
        .flat_map(|table| &table.rows)
    {
        if row.variable_type == 3 {
            if let Some(value) = row.value.filter(|value| value.is_finite() && *value > 0.0) {
                candidates.entry(row.key).or_default().push(value);
            }
        }
    }
    let radial_coordinates = resolved_section_coordinates(definition);
    for constraint in
        section_equation_radial_constraints(definition, &radial_coordinates, &BTreeSet::new())
    {
        if constraint.radius.0 == 3 {
            if let Some(value) = constraint
                .radius_value
                .filter(|value| value.is_finite() && *value > 0.0)
            {
                candidates
                    .entry(constraint.radius.1)
                    .or_default()
                    .push(value);
            }
        }
    }
    for (variable, value) in section_equation_function_six_distance_values(
        definition,
        &radial_coordinates,
        &BTreeSet::new(),
    ) {
        if variable.0 == 3 && value.is_finite() && value > 0.0 {
            candidates.entry(variable.1).or_default().push(value);
        }
    }
    for constraint in section_equation_radius_dimensions(definition) {
        candidates
            .entry(constraint.radius)
            .or_default()
            .push(constraint.value);
    }
    for relation in definition
        .relations
        .iter()
        .filter(|table| feature_relation_table_complete(table))
        .flat_map(|table| &table.rows)
    {
        if section_solver_relation_is_disabled(definition, relation.relation_id) {
            continue;
        }
        if relation.relation_type == 5 && relation.sign == 1 {
            let Some(_) = section_type5_radius_arc(definition, relation) else {
                continue;
            };
            let Some(dimension) = section_relation_length_dimension(definition, relation) else {
                continue;
            };
            let Some(value) = dimension
                .value
                .filter(|value| value.is_finite() && *value > 0.0)
            else {
                continue;
            };
            let radius = match dimension.dimension_type {
                4 => value / 2.0,
                _ => value,
            };
            candidates
                .entry(relation.dimension_id)
                .or_default()
                .push(radius);
            continue;
        }
        if relation.relation_type != 14 || relation.sign != 1 {
            continue;
        }
        let Some(vectors) = relation.operand_vectors else {
            continue;
        };
        let [Some(radius_id), Some(0), Some(0), Some(0)] = vectors[0] else {
            continue;
        };
        if vectors[1] != [Some(0); 4] || vectors[2] != [Some(15), Some(0), Some(0), Some(0)] {
            continue;
        }
        let Some(dimension) = section_relation_length_dimension(definition, relation) else {
            continue;
        };
        let Some(value) = dimension
            .value
            .filter(|value| value.is_finite() && *value > 0.0)
        else {
            continue;
        };
        let value = if dimension.dimension_type == 4 {
            value / 2.0
        } else {
            value
        };
        candidates.entry(radius_id).or_default().push(value);
    }
    if let Some(dimensions) = definition
        .dimensions
        .as_ref()
        .filter(|dimensions| feature_dimension_table_complete(dimensions))
    {
        for circle in definition
            .segments
            .iter()
            .flat_map(|segments| &segments.circle_rows)
            .filter(|segment| {
                unique_circle_segment(definition, segment.external_id)
                    .is_some_and(|candidate| candidate == *segment)
            })
        {
            let radius_id = circle.radius_ref;
            let Some(dimension) = dimensions
                .rows
                .get(usize::try_from(radius_id).unwrap_or(usize::MAX))
            else {
                continue;
            };
            let Some(value) = dimension
                .value
                .filter(|value| value.is_finite() && *value > 0.0)
            else {
                continue;
            };
            let radius = match dimension.dimension_type {
                3 => value,
                4 => value / 2.0,
                _ => continue,
            };
            candidates.entry(radius_id).or_default().push(radius);
        }
    }
    let points = resolved_section_points(definition);
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
    {
        if unique_section_skamp_segment(definition, segment.external_id) != Some(segment) {
            continue;
        }
        let Some(radius_id) = segment.radius_ref else {
            continue;
        };
        let Some(center) = segment.center_id.and_then(|id| points.get(&id)) else {
            continue;
        };
        let endpoint_radii = segment
            .point_ids
            .iter()
            .filter_map(|id| points.get(id))
            .map(|point| (point[0] - center[0]).hypot(point[1] - center[1]))
            .filter(|radius| radius.is_finite() && *radius > 1e-12)
            .collect::<Vec<_>>();
        let Some(radius) = endpoint_radii.first().copied() else {
            continue;
        };
        let scale = endpoint_radii
            .iter()
            .copied()
            .fold(radius.max(1.0), f64::max);
        if endpoint_radii
            .iter()
            .all(|candidate| (*candidate - radius).abs() <= 1e-9 * scale)
        {
            candidates.entry(radius_id).or_default().push(radius);
        }
    }
    let mut adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    let mut invalid_scalar_radius_ids = BTreeSet::new();
    if let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    {
        for component in section_equation_scalar_equality_components(definition) {
            let radius_ids = component
                .iter()
                .filter_map(|&(variable_type, radius_id)| (variable_type == 3).then_some(radius_id))
                .collect::<Vec<_>>();
            if radius_ids.len() != component.len() {
                continue;
            }
            let invalid = component.iter().any(|&(variable_type, radius_id)| {
                variables.rows.iter().any(|row| {
                    row.variable_type == variable_type
                        && row.key == radius_id
                        && row
                            .value
                            .is_some_and(|value| !value.is_finite() || value <= 0.0)
                })
            });
            if invalid {
                invalid_scalar_radius_ids.extend(radius_ids);
                continue;
            }
            for pair in radius_ids.windows(2) {
                let [first, second] = pair else {
                    unreachable!();
                };
                adjacency.entry(*first).or_default().insert(*second);
                adjacency.entry(*second).or_default().insert(*first);
            }
        }
    }
    for skamp in active_complete_section_skamps(definition) {
        let [first, second] = skamp.items.as_slice() else {
            continue;
        };
        if skamp.kind != 6 || first.sense != 0 || second.sense != 0 {
            continue;
        }
        let Some(first_radius) = section_skamp_radius_source(definition, first) else {
            continue;
        };
        let Some(second_radius) = section_skamp_radius_source(definition, second) else {
            continue;
        };
        match (first_radius, second_radius) {
            (SectionRadiusSource::Reference(first), SectionRadiusSource::Reference(second)) => {
                adjacency.entry(first).or_default().insert(second);
                adjacency.entry(second).or_default().insert(first);
            }
            (SectionRadiusSource::Reference(reference), SectionRadiusSource::Value(value))
            | (SectionRadiusSource::Value(value), SectionRadiusSource::Reference(reference)) => {
                candidates.entry(reference).or_default().push(value);
            }
            (SectionRadiusSource::Value(_), SectionRadiusSource::Value(_)) => {}
        }
    }
    let mut remaining = candidates
        .keys()
        .chain(adjacency.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut radii = BTreeMap::new();
    while let Some(seed) = remaining.first().copied() {
        let mut component = BTreeSet::from([seed]);
        let mut pending = std::collections::VecDeque::from([seed]);
        while let Some(radius_id) = pending.pop_front() {
            for neighbor in adjacency.get(&radius_id).into_iter().flatten() {
                if component.insert(*neighbor) {
                    pending.push_back(*neighbor);
                }
            }
        }
        if component
            .iter()
            .any(|radius_id| invalid_scalar_radius_ids.contains(radius_id))
        {
            remaining.retain(|radius_id| !component.contains(radius_id));
            continue;
        }
        let values = component
            .iter()
            .flat_map(|radius_id| candidates.get(radius_id).into_iter().flatten())
            .copied()
            .collect::<Vec<_>>();
        if let Some(value) = values.first().copied() {
            let scale = values.iter().copied().fold(value.max(1.0), f64::max);
            if !values
                .iter()
                .all(|candidate| (*candidate - value).abs() <= 1e-9 * scale)
            {
                remaining.retain(|radius_id| !component.contains(radius_id));
                continue;
            }
            radii.extend(component.iter().map(|radius_id| (*radius_id, value)));
        }
        remaining.retain(|radius_id| !component.contains(radius_id));
    }
    radii
}

pub(super) fn section_relation_length_dimension<'a>(
    definition: &'a crate::feature::FeatureDefinition,
    relation: &crate::feature::FeatureRelation,
) -> Option<&'a crate::feature::FeatureDimension> {
    let dimension = definition
        .dimensions
        .as_ref()
        .filter(|table| feature_dimension_table_complete(table))?
        .rows
        .get(usize::try_from(relation.dimension_id).ok()?)?;
    (dimension.value_unit == crate::feature::DimensionUnit::Millimeters
        && matches!(dimension.dimension_type, 1..=5))
    .then_some(dimension)
}

pub(super) fn section_type5_radius_arc<'a>(
    definition: &'a crate::feature::FeatureDefinition,
    relation: &crate::feature::FeatureRelation,
) -> Option<&'a crate::feature::FeatureSegment> {
    (relation.relation_type == 5 && relation.sign == 1).then_some(())?;
    section_relation_length_dimension(definition, relation)?;
    let vectors = relation.operand_vectors?;
    let [Some(first_point), Some(0), Some(second_point), Some(0)] = vectors[0] else {
        return None;
    };
    let [Some(center), Some(10), Some(0), Some(1)] = vectors[1] else {
        return None;
    };
    if vectors[2] != [Some(16), Some(15), Some(0), Some(0)] {
        return None;
    }
    let unique_entities = unique_section_segment_external_ids(definition);
    let matching = section_segment_rows(definition)
        .iter()
        .filter(|segment| {
            segment.kind == crate::feature::FeatureSegmentKind::Arc
                && segment.radius_ref == Some(relation.dimension_id)
                && segment.center_id == Some(center)
                && (segment.point_ids == [first_point, second_point]
                    || segment.point_ids == [second_point, first_point])
                && unique_entities.contains(&segment.external_id)
        })
        .collect::<Vec<_>>();
    let [segment] = matching.as_slice() else {
        return None;
    };
    Some(segment)
}

#[derive(Clone, Copy)]
pub(super) enum SectionRadiusSource {
    Reference(u32),
    Value(f64),
}

pub(super) fn section_skamp_radius_source(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SectionRadiusSource> {
    if let Some(circle) = unique_circle_segment(definition, item.entity_id) {
        return Some(SectionRadiusSource::Reference(circle.radius_ref));
    }
    if let Some(segment) = unique_section_skamp_segment(definition, item.entity_id) {
        return (segment.kind == crate::feature::FeatureSegmentKind::Arc)
            .then_some(segment.radius_ref)
            .flatten()
            .map(SectionRadiusSource::Reference);
    }
    if definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .any(|segment| segment.external_id == item.entity_id)
    {
        return None;
    }
    let radius = match section_saved_entity(definition, item.entity_id)? {
        crate::feature::FeatureSavedEntity::Arc(arc) => arc.radius,
        crate::feature::FeatureSavedEntity::Circle(circle) => circle.radius,
        _ => None,
    }?;
    (radius.is_finite() && radius > 0.0).then_some(SectionRadiusSource::Value(radius))
}

pub(super) fn section_arc_carrier(
    radii: &BTreeMap<u32, f64>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<([f64; 2], f64)> {
    (segment.kind == crate::feature::FeatureSegmentKind::Arc).then_some(())?;
    let center = *points.get(&segment.center_id?)?;
    let radius = *radii.get(&segment.radius_ref?)?;
    Some((center, radius))
}

#[derive(Clone)]
pub(super) struct SectionIntersectionCarrier {
    pub(super) geometry: SketchGeometry,
}

pub(super) fn section_axis_line_carrier_with_points(
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    let fixed_coordinate = match segment.directions {
        [Some(0), _, _] => 0,
        [_, Some(0), _] => 1,
        _ => return None,
    };
    section_fixed_coordinate_line_carrier(variable_points, segment, fixed_coordinate)
}

pub(super) fn section_fixed_coordinate_line_carrier(
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::FeatureSegment,
    fixed_coordinate: usize,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Line && fixed_coordinate < 2)
        .then_some(())?;
    let endpoint = |id| variable_points.get(&id);
    let [first, second] = segment.point_ids.map(endpoint);
    let (Some(first), Some(second)) = (first, second) else {
        return None;
    };
    let (Some(first), Some(second)) = (first[fixed_coordinate], second[fixed_coordinate]) else {
        return None;
    };
    let scale = first.abs().max(second.abs()).max(1.0);
    ((first - second).abs() <= 1e-9 * scale).then(|| {
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

pub(super) fn section_proven_axis_line_carrier(
    definition: &crate::feature::FeatureDefinition,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    if let Some(geometry) = section_axis_line_carrier_with_points(variable_points, segment) {
        Some(geometry)
    } else {
        section_fixed_coordinate_line_carrier(
            variable_points,
            segment,
            section_line_entity_fixed_coordinate(definition, segment.external_id)?,
        )
    }
}

pub(super) fn section_axis_reference_line_geometry(
    definition: &crate::feature::FeatureDefinition,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    if !section_degenerate_axis_line(definition, segment) {
        return section_proven_axis_line_carrier(definition, variable_points, segment);
    }
    let fixed_coordinate = usize::try_from(segment.vertical_horizontal?).ok()?;
    let values = segment
        .point_ids
        .iter()
        .filter_map(|point| {
            variable_points
                .get(point)?
                .get(fixed_coordinate)
                .copied()
                .flatten()
        })
        .collect::<Vec<_>>();
    let expected_value_count = if segment.point_ids[0] == segment.point_ids[1] {
        1
    } else {
        2
    };
    (values.len() == expected_value_count).then_some(())?;
    let value = *values.first()?;
    let scale = values
        .iter()
        .copied()
        .map(f64::abs)
        .fold(value.abs().max(1.0), f64::max);
    values
        .iter()
        .all(|candidate| (*candidate - value).abs() <= 1e-9 * scale)
        .then_some(())?;
    let (origin, direction) = if fixed_coordinate == 0 {
        (Point2::new(value, 0.0), Point2::new(0.0, 1.0))
    } else {
        (Point2::new(0.0, value), Point2::new(1.0, 0.0))
    };
    Some(SketchGeometry::ReferenceLine { origin, direction })
}

pub(super) fn section_segment_intersection_carrier_with_missing_line(
    definition: &crate::feature::FeatureDefinition,
    radii: &BTreeMap<u32, f64>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
    missing_line: Option<&(usize, SketchGeometry)>,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Option<SectionIntersectionCarrier> {
    if let Some(geometry) = resolved_section_segment_geometry_with_missing_line(
        definition,
        points,
        segment,
        missing_line,
    ) {
        return Some(SectionIntersectionCarrier { geometry });
    }
    if let Some(geometry) = section_proven_axis_line_carrier(definition, variable_points, segment) {
        return Some(SectionIntersectionCarrier { geometry });
    }
    let ([center_u, center_v], radius) = section_arc_carrier(radii, points, segment)
        .or_else(|| saved_section_arc_carrier(definition, segment))?;
    Some(SectionIntersectionCarrier {
        geometry: SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(center_u, center_v),
            radius: Length(radius),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::TAU),
        },
    })
}

pub(super) fn trim_segment_id(
    definition: &crate::feature::FeatureDefinition,
    row: &crate::feature::FeatureTrimEntity,
) -> Option<u32> {
    let trim_table = definition.trim_entities.as_ref()?;
    (trim_table.has_complete_bucket_frame() && trim_table.has_unique_external_ids())
        .then_some(())?;
    let Some(segment_table) = &definition.segments else {
        return Some(row.external_id);
    };
    segment_table.is_complete().then_some(())?;
    let segments = &segment_table.rows;
    let trim_rows = &trim_table.rows;
    let matching_segment_count = segments
        .iter()
        .filter(|segment| segment.external_id == row.external_id)
        .count();
    let matching_trim_count = trim_rows
        .iter()
        .filter(|trim| trim.external_id == row.external_id)
        .count();
    if matching_segment_count == 1 && matching_trim_count == 1 {
        return Some(row.external_id);
    }
    if matching_segment_count != 0 || matching_trim_count != 1 {
        return None;
    }
    let unmatched_segments = segments
        .iter()
        .filter(|segment| {
            !trim_rows
                .iter()
                .any(|trim| trim.external_id == segment.external_id)
        })
        .map(|segment| segment.external_id)
        .collect::<Vec<_>>();
    let unmatched_rows = trim_rows
        .iter()
        .filter(|trim| {
            !segments
                .iter()
                .any(|segment| segment.external_id == trim.external_id)
        })
        .collect::<Vec<_>>();
    match (unmatched_segments.as_slice(), unmatched_rows.as_slice()) {
        ([segment_id], [unmatched]) if std::ptr::eq(*unmatched, row) => Some(*segment_id),
        _ => None,
    }
}

pub(super) fn section_line_origin_direction(geometry: &SketchGeometry) -> Option<(Point2, Point2)> {
    match geometry {
        SketchGeometry::Line { start, end } => {
            Some((*start, Point2::new(end.u - start.u, end.v - start.v)))
        }
        SketchGeometry::ReferenceLine { origin, direction } => Some((*origin, *direction)),
        _ => None,
    }
}

pub(super) fn intersect_section_lines(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<[f64; 2]> {
    let (first_origin, first_direction) = section_line_origin_direction(first)?;
    let (second_origin, second_direction) = section_line_origin_direction(second)?;
    let first_end = Point2::new(
        first_origin.u + first_direction.u,
        first_origin.v + first_direction.v,
    );
    let second_end = Point2::new(
        second_origin.u + second_direction.u,
        second_origin.v + second_direction.v,
    );
    let denominator = (first_origin.u - first_end.u).mul_add(
        second_origin.v - second_end.v,
        -(first_origin.v - first_end.v) * (second_origin.u - second_end.u),
    );
    let scale = (first_origin.u - first_end.u)
        .abs()
        .max((first_origin.v - first_end.v).abs())
        .max((second_origin.u - second_end.u).abs())
        .max((second_origin.v - second_end.v).abs())
        .max(1.0);
    if denominator.abs() <= 1e-12 * scale * scale {
        return None;
    }
    let first_cross = first_origin
        .u
        .mul_add(first_end.v, -(first_origin.v * first_end.u));
    let second_cross = second_origin
        .u
        .mul_add(second_end.v, -(second_origin.v * second_end.u));
    Some([
        first_cross.mul_add(
            second_origin.u - second_end.u,
            -(first_origin.u - first_end.u) * second_cross,
        ) / denominator,
        first_cross.mul_add(
            second_origin.v - second_end.v,
            -(first_origin.v - first_end.v) * second_cross,
        ) / denominator,
    ])
}

pub(super) fn intersect_section_line_arc(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<[f64; 2]> {
    let (
        (line @ SketchGeometry::Line { .. }, arc @ SketchGeometry::Arc { .. })
        | (arc @ SketchGeometry::Arc { .. }, line @ SketchGeometry::Line { .. }),
    ) = ((first, second),)
    else {
        return None;
    };
    let SketchGeometry::Line { start, end } = line else {
        return None;
    };
    let SketchGeometry::Arc { center, radius, .. } = arc else {
        return None;
    };
    let direction = [end.u - start.u, end.v - start.v];
    let length = direction[0].hypot(direction[1]);
    if length <= 1e-12 || radius.0 <= 1e-12 {
        return None;
    }
    let direction = direction.map(|value| value / length);
    let relative = [start.u - center.u, start.v - center.v];
    let projection = -(relative[0] * direction[0] + relative[1] * direction[1]);
    let closest = [
        start.u + projection * direction[0],
        start.v + projection * direction[1],
    ];
    let distance_squared = (closest[0] - center.u).mul_add(
        closest[0] - center.u,
        (closest[1] - center.v) * (closest[1] - center.v),
    );
    let radial_squared = radius.0 * radius.0;
    let scale = radial_squared.max(1.0);
    if distance_squared > radial_squared + 1e-10 * scale {
        return None;
    }
    let travel = (radial_squared - distance_squared).max(0.0).sqrt();
    let candidates = [
        [
            closest[0] + travel * direction[0],
            closest[1] + travel * direction[1],
        ],
        [
            closest[0] - travel * direction[0],
            closest[1] - travel * direction[1],
        ],
    ];
    if travel <= 1e-10 * radius.0.max(1.0) {
        let parameter = projection / length;
        return (-1e-10..=1.0 + 1e-10)
            .contains(&parameter)
            .then_some(candidates[0]);
    }
    let parameters = [
        (projection + travel) / length,
        (projection - travel) / length,
    ];
    let inside = parameters
        .into_iter()
        .enumerate()
        .filter(|(_, parameter)| (-1e-10..=1.0 + 1e-10).contains(parameter))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [index] = inside.as_slice() else {
        return None;
    };
    Some(candidates[*index])
}

pub(super) fn intersect_tangent_section_arcs(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<[f64; 2]> {
    let (
        SketchGeometry::Arc {
            center: first_center,
            radius: first_radius,
            ..
        },
        SketchGeometry::Arc {
            center: second_center,
            radius: second_radius,
            ..
        },
    ) = (first, second)
    else {
        return None;
    };
    if first_radius.0 <= 1e-12 || second_radius.0 <= 1e-12 {
        return None;
    }
    let delta = [
        second_center.u - first_center.u,
        second_center.v - first_center.v,
    ];
    let distance = delta[0].hypot(delta[1]);
    let scale = distance.max(first_radius.0).max(second_radius.0).max(1.0);
    if distance <= 1e-12 * scale {
        return None;
    }
    let offset = (first_radius
        .0
        .mul_add(first_radius.0, -(second_radius.0 * second_radius.0))
        + distance * distance)
        / (2.0 * distance);
    let height_squared = first_radius.0.mul_add(first_radius.0, -(offset * offset));
    if height_squared.abs() > 1e-9 * scale * scale {
        return None;
    }
    Some([
        first_center.u + offset * delta[0] / distance,
        first_center.v + offset * delta[1] / distance,
    ])
}

pub(super) fn intersect_section_carriers(
    first: &SectionIntersectionCarrier,
    second: &SectionIntersectionCarrier,
) -> Option<[f64; 2]> {
    let line_arc_is_bounded = matches!(
        (&first.geometry, &second.geometry),
        (SketchGeometry::Line { .. }, SketchGeometry::Arc { .. })
            | (SketchGeometry::Arc { .. }, SketchGeometry::Line { .. })
    );
    intersect_section_lines(&first.geometry, &second.geometry)
        .or_else(|| {
            line_arc_is_bounded
                .then(|| intersect_section_line_arc(&first.geometry, &second.geometry))
                .flatten()
        })
        .or_else(|| intersect_tangent_section_arcs(&first.geometry, &second.geometry))
}

pub(super) fn intersect_incident_section_carriers(
    carriers: &[SectionIntersectionCarrier],
) -> Option<[f64; 2]> {
    (carriers.len() >= 2).then_some(())?;
    let mut candidates = Vec::new();
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
            candidates.push((
                0,
                intersect_section_carriers(&carriers[first], &carriers[second])?,
            ));
        }
    }
    let (coordinates, ambiguous) = reconciled_section_coordinates(candidates);
    ambiguous.is_empty().then_some(())?;
    coordinates.get(&0).copied()
}

pub(super) fn resolved_trim_vertex_coordinates(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
) -> BTreeMap<u32, [f64; 2]> {
    let Some(segments) = &definition.segments else {
        return BTreeMap::new();
    };
    let radii = resolved_section_radii(definition);
    let missing_line = saved_section_missing_line_geometry(definition);
    let variable_points = definition
        .variables
        .as_ref()
        .map(|variables| variables.reconciled_points().0)
        .unwrap_or_default();
    let mut seen_vertex_ids = BTreeSet::new();
    let duplicate_vertex_ids = definition
        .trim_vertices
        .iter()
        .filter(|table| table.has_complete_bucket_frame())
        .flat_map(|table| &table.rows)
        .filter_map(|vertex| {
            (!seen_vertex_ids.insert(vertex.vertex_id)).then_some(vertex.vertex_id)
        })
        .collect::<BTreeSet<_>>();
    let mut coordinate_candidates = definition
        .trim_vertices
        .iter()
        .filter(|table| table.has_complete_bucket_frame())
        .flat_map(|table| &table.rows)
        .filter_map(|vertex| Some((vertex.vertex_id, vertex.section_coordinates?)))
        .collect::<Vec<_>>();
    for trim in definition
        .trim_entities
        .iter()
        .flat_map(|table| &table.rows)
    {
        let Some(external_id) = trim_segment_id(definition, trim) else {
            continue;
        };
        let Some(segment) = segments.segment(external_id) else {
            continue;
        };
        let Some(([center_u, center_v], radius)) = saved_section_arc_carrier(definition, segment)
        else {
            continue;
        };
        let Some(arc) = saved_section_arc_record(definition, segment) else {
            continue;
        };
        for (vertex, endpoint) in trim.vertices.into_iter().zip(arc.endpoints) {
            let [Some(u), Some(v), _] = endpoint else {
                continue;
            };
            let candidate = [u, v];
            let candidate_radius = (u - center_u).hypot(v - center_v);
            let radial_scale = radius.max(candidate_radius).max(1.0);
            if (candidate_radius - radius).abs() > 1e-9 * radial_scale {
                continue;
            }
            coordinate_candidates.push((vertex, candidate));
        }
    }
    let mut incident = BTreeMap::<u32, Vec<u32>>::new();
    for entity in definition
        .trim_entities
        .iter()
        .flat_map(|table| &table.rows)
    {
        let Some(external_id) = trim_segment_id(definition, entity) else {
            continue;
        };
        for vertex in entity.vertices {
            incident.entry(vertex).or_default().push(external_id);
        }
    }
    let explicit_incident = definition
        .trim_vertices
        .as_ref()
        .filter(|table| table.has_complete_bucket_frame())
        .map(|table| {
            let mut result = BTreeMap::<u32, Vec<u32>>::new();
            for vertex in &table.rows {
                let mut resolved = Vec::new();
                for entity_id in &vertex.entities {
                    let matches = definition
                        .trim_entities
                        .iter()
                        .flat_map(|table| &table.rows)
                        .filter(|entity| entity.external_id == *entity_id)
                        .collect::<Vec<_>>();
                    let external_id = match matches.as_slice() {
                        [entity] => trim_segment_id(definition, entity),
                        [] => segments
                            .segment(*entity_id)
                            .map(|segment| segment.external_id),
                        _ => None,
                    };
                    if let Some(external_id) = external_id {
                        resolved.push(external_id);
                    }
                }
                resolved.sort_unstable();
                if resolved.len() == vertex.entities.len() {
                    result.entry(vertex.vertex_id).or_default().extend(resolved);
                }
            }
            result
        });
    if let Some(explicit) = &explicit_incident {
        for (vertex, entities) in explicit {
            if entities.len() < 2 || entities.windows(2).any(|pair| pair[0] == pair[1]) {
                continue;
            }
            let mut derived = incident.get(vertex).cloned().unwrap_or_default();
            derived.sort_unstable();
            derived.dedup();
            if derived
                .iter()
                .any(|external_id| !entities.contains(external_id))
            {
                continue;
            }
            incident.insert(*vertex, entities.clone());
            let common_points = entities
                .iter()
                .filter_map(|external_id| segments.segment(*external_id))
                .map(|segment| segment.point_ids.into_iter().collect::<BTreeSet<_>>())
                .reduce(|common, points| common.intersection(&points).copied().collect());
            let Some(common_points) = common_points else {
                continue;
            };
            let common_points = common_points.into_iter().collect::<Vec<_>>();
            let [point_id] = common_points.as_slice() else {
                continue;
            };
            if let Some(coordinate) = points.get(point_id) {
                coordinate_candidates.push((*vertex, *coordinate));
            }
        }
    }
    let intersection_carriers = incident
        .values()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|external_id| {
            let segment = segments.segment(external_id)?;
            let carrier = section_segment_intersection_carrier_with_missing_line(
                definition,
                &radii,
                points,
                segment,
                missing_line.as_ref(),
                &variable_points,
            )?;
            Some((external_id, carrier))
        })
        .collect::<BTreeMap<_, _>>();
    for (vertex, mut entities) in incident {
        entities.sort_unstable();
        if entities.len() < 2 || entities.windows(2).any(|pair| pair[0] == pair[1]) {
            continue;
        }
        if explicit_incident
            .as_ref()
            .is_some_and(|explicit| explicit.get(&vertex) != Some(&entities))
        {
            continue;
        }
        let carriers = entities
            .iter()
            .map(|external_id| intersection_carriers.get(external_id).cloned())
            .collect::<Option<Vec<_>>>();
        let Some(carriers) = carriers else {
            continue;
        };
        if let Some(coordinate) = intersect_incident_section_carriers(&carriers) {
            coordinate_candidates.push((vertex, coordinate));
        }
    }
    let (mut coordinates, mut ambiguous_vertices) =
        reconciled_section_coordinates(coordinate_candidates);
    ambiguous_vertices.extend(duplicate_vertex_ids);
    coordinates.retain(|vertex, _| !ambiguous_vertices.contains(vertex));
    loop {
        let mut additions = Vec::new();
        for trim in definition
            .trim_entities
            .iter()
            .flat_map(|table| &table.rows)
        {
            let Some(external_id) = trim_segment_id(definition, trim) else {
                continue;
            };
            let Some(segment) = segments.segment(external_id) else {
                continue;
            };
            let Some(SketchGeometry::Line { start, end }) =
                resolved_section_segment_geometry_with_missing_line(
                    definition,
                    points,
                    segment,
                    missing_line.as_ref(),
                )
            else {
                continue;
            };
            let stored = [[start.u, start.v], [end.u, end.v]];
            let known = trim
                .vertices
                .map(|vertex| coordinates.get(&vertex).copied());
            let (known_point, missing_index) = match known {
                [Some(point), None] => (point, 1),
                [None, Some(point)] => (point, 0),
                _ => continue,
            };
            let distances =
                stored.map(|point| (point[0] - known_point[0]).hypot(point[1] - known_point[1]));
            let scale = stored
                .iter()
                .flatten()
                .map(|value| value.abs())
                .fold(1.0, f64::max);
            let matched = if distances[0] <= 1e-9 * scale && distances[1] > 1e-9 * scale {
                0
            } else if distances[1] <= 1e-9 * scale && distances[0] > 1e-9 * scale {
                1
            } else {
                continue;
            };
            additions.push((trim.vertices[missing_index], stored[1 - matched]));
        }
        let (additions, conflicts) = reconciled_section_coordinates(additions);
        ambiguous_vertices.extend(conflicts);
        let mut changed = false;
        for (vertex, coordinate) in additions {
            if ambiguous_vertices.contains(&vertex) {
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = coordinates.entry(vertex) {
                entry.insert(coordinate);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    coordinates
}

pub(super) fn reconciled_section_coordinates(
    candidates: impl IntoIterator<Item = (u32, [f64; 2])>,
) -> (BTreeMap<u32, [f64; 2]>, BTreeSet<u32>) {
    let mut grouped = BTreeMap::<u32, Vec<[f64; 2]>>::new();
    for (vertex, coordinate) in candidates {
        grouped.entry(vertex).or_default().push(coordinate);
    }
    let mut coordinates = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for (vertex, values) in grouped {
        let first = values[0];
        let scale = values
            .iter()
            .flatten()
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        if values.iter().all(|candidate| {
            (candidate[0] - first[0]).hypot(candidate[1] - first[1]) <= 1e-9 * scale
        }) {
            coordinates.insert(vertex, first);
        } else {
            ambiguous.insert(vertex);
        }
    }
    (coordinates, ambiguous)
}

pub(super) fn trimmed_section_segment_geometry_with_missing_line(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    trim_vertices: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
    missing_line: Option<&(usize, SketchGeometry)>,
) -> Option<SketchGeometry> {
    let trim = definition
        .trim_entities
        .as_ref()?
        .rows
        .iter()
        .find(|row| trim_segment_id(definition, row) == Some(segment.external_id))?;
    let start = trim_vertices.get(&trim.vertices[0])?;
    let end = trim_vertices.get(&trim.vertices[1])?;
    if let Some(SketchGeometry::Line {
        start: carrier_start,
        end: carrier_end,
    }) = resolved_section_segment_geometry_with_missing_line(
        definition,
        points,
        segment,
        missing_line,
    ) {
        let scale = [
            carrier_start.u,
            carrier_start.v,
            carrier_end.u,
            carrier_end.v,
            start[0],
            start[1],
            end[0],
            end[1],
        ]
        .into_iter()
        .map(f64::abs)
        .fold(1.0, f64::max);
        let direction = [
            carrier_end.u / scale - carrier_start.u / scale,
            carrier_end.v / scale - carrier_start.v / scale,
        ];
        let direction_norm = direction[0].hypot(direction[1]);
        if direction_norm <= 1e-12
            || [start, end].into_iter().any(|point| {
                let offset = [
                    point[0] / scale - carrier_start.u / scale,
                    point[1] / scale - carrier_start.v / scale,
                ];
                (offset[0] * direction[1] - offset[1] * direction[0]).abs() > 1e-9 * direction_norm
            })
        {
            return None;
        }
    } else if let Some(([center_u, center_v], radius)) =
        section_arc_carrier(&resolved_section_radii(definition), points, segment)
            .or_else(|| saved_section_arc_carrier(definition, segment))
    {
        let first = [start[0] - center_u, start[1] - center_v];
        let second = [end[0] - center_u, end[1] - center_v];
        let first_radius = first[0].hypot(first[1]);
        let second_radius = second[0].hypot(second[1]);
        let scale = radius.max(first_radius).max(second_radius).max(1.0);
        if (first_radius - radius).abs() > 1e-9 * scale
            || (second_radius - radius).abs() > 1e-9 * scale
        {
            return None;
        }
        let start_angle = second[1].atan2(second[0]);
        let mut end_angle = first[1].atan2(first[0]);
        while end_angle <= start_angle {
            end_angle += std::f64::consts::TAU;
        }
        return Some(SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(center_u, center_v),
            radius: Length(radius),
            start_angle: Angle(start_angle),
            end_angle: Angle(end_angle),
        });
    } else {
        let scale = start
            .iter()
            .chain(end)
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        let orientation_matches = match section_line_fixed_coordinate(definition, segment) {
            Some(0) => (start[0] - end[0]).abs() <= 1e-9 * scale,
            Some(1) => (start[1] - end[1]).abs() <= 1e-9 * scale,
            _ => false,
        };
        orientation_matches.then_some(())?;
    }
    Some(SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
        end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
    })
}

pub(super) fn section_point_in_model(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 2],
) -> [f64; 3] {
    std::array::from_fn(|axis| {
        transform.origin[axis]
            + point[0] * transform.u_axis[axis]
            + point[1] * transform.v_axis[axis]
    })
}

pub(super) fn section_xyz_in_model(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 3],
) -> [f64; 3] {
    std::array::from_fn(|axis| {
        transform.origin[axis]
            + point[0] * transform.u_axis[axis]
            + point[1] * transform.v_axis[axis]
            + point[2] * transform.normal[axis]
    })
}

pub(super) fn normalized(vector: [f64; 3]) -> Option<[f64; 3]> {
    let magnitude = vector
        .iter()
        .fold(0.0_f64, |norm, value| norm.hypot(*value));
    (magnitude.is_finite() && magnitude > 1e-12).then(|| vector.map(|value| value / magnitude))
}

#[cfg(test)]
mod tests;
