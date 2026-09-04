// SPDX-License-Identifier: Apache-2.0
//! Sketch profile connectivity, intersection, and containment.

use super::super::analytic::nurbs_intrinsic_parameter_range;
use super::super::holes::ExtrusionSpan;
use super::super::uniqueness::exactly_one;
use super::nurbs::{oriented_sketch_nurbs_curve, sketch_nurbs_curve, sketch_nurbs_pcurve};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, PcurveGeometry};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchGeometry, SketchId};

const EPS_ENDPOINT_AGREEMENT: f64 = 1.0e-9;
const EPS_PARAMETER_SCALE: f64 = 1.0e-12;
const EPS_FULL_TURN: f64 = 1.0e-12;
const EPS_AREA: f64 = 1.0e-12;
const EPS_GEOMETRY_AGREEMENT: f64 = 1.0e-9;

pub(in super::super) fn sketch_geometry_endpoints(
    geometry: &SketchGeometry,
) -> Option<([f64; 2], [f64; 2])> {
    match geometry {
        SketchGeometry::Line { start, end } => Some(([start.u, start.v], [end.u, end.v])),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some((
            [
                center.u + radius.0 * start_angle.0.cos(),
                center.v + radius.0 * start_angle.0.sin(),
            ],
            [
                center.u + radius.0 * end_angle.0.cos(),
                center.v + radius.0 * end_angle.0.sin(),
            ],
        )),
        SketchGeometry::Circle { center, radius }
            if center.u.is_finite()
                && center.v.is_finite()
                && radius.0.is_finite()
                && radius.0 > 0.0 =>
        {
            let seam = [center.u + radius.0, center.v];
            Some((seam, seam))
        }
        SketchGeometry::Nurbs { .. } => {
            let nurbs = sketch_nurbs_curve(geometry)?;
            let [lower, upper] = nurbs_intrinsic_parameter_range(&nurbs)?;
            let carrier = CurveGeometry::Nurbs(nurbs);
            let first = cadmpeg_ir::eval::curve_point(&carrier, lower)?;
            let last = cadmpeg_ir::eval::curve_point(&carrier, upper)?;
            [first.x, first.y]
                .into_iter()
                .chain([last.x, last.y])
                .all(f64::is_finite)
                .then_some(([first.x, first.y], [last.x, last.y]))
        }
        _ => None,
    }
}

pub(in super::super) fn connected_sketch_profile_vertices(
    ir: &CadIr,
    sketch_id: &SketchId,
) -> Vec<(usize, Vec<[f64; 2]>)> {
    let Some(sketch) = exactly_one(
        ir.model
            .sketches
            .iter()
            .filter(|sketch| sketch.id == *sketch_id),
    ) else {
        return Vec::new();
    };
    sketch
        .profiles
        .iter()
        .enumerate()
        .filter_map(|(profile_index, profile)| {
            (!profile.is_empty()).then_some(())?;
            let uses = profile
                .iter()
                .map(|entity_use| {
                    let geometry = exactly_one(ir.model.sketch_entities.iter().filter(|entity| {
                        entity.sketch == *sketch_id && entity.id() == &entity_use.entity
                    }))
                    .map(|entity| &entity.geometry)?;
                    let (mut start, mut end) = sketch_geometry_endpoints(geometry)?;
                    if entity_use.reversed {
                        std::mem::swap(&mut start, &mut end);
                    }
                    Some((start, end))
                })
                .collect::<Option<Vec<_>>>()?;
            let scale = uses
                .iter()
                .flat_map(|(start, end)| start.iter().chain(end))
                .map(|coordinate| coordinate.abs())
                .fold(1.0, f64::max);
            uses.windows(2)
                .all(|adjacent| {
                    let end = adjacent[0].1;
                    let next = adjacent[1].0;
                    (end[0] - next[0]).hypot(end[1] - next[1]) <= EPS_ENDPOINT_AGREEMENT * scale
                })
                .then(|| {
                    let mut vertices = uses.iter().map(|(start, _)| *start).collect::<Vec<_>>();
                    let first = uses[0].0;
                    let terminal = uses.last().expect("profile is not empty").1;
                    if (terminal[0] - first[0]).hypot(terminal[1] - first[1])
                        > EPS_ENDPOINT_AGREEMENT * scale
                    {
                        vertices.push(terminal);
                    }
                    (profile_index, vertices)
                })
        })
        .collect()
}

pub(in super::super) fn oriented_arc_parameterization(
    reversed: bool,
    start: f64,
    end: f64,
) -> (f64, [f64; 2]) {
    let (axis_sign, raw_start, raw_end) = if reversed {
        (-1.0, -end, -start)
    } else {
        (1.0, start, end)
    };
    let raw_span = raw_end - raw_start;
    let full_turn = raw_span.is_finite()
        && (raw_span.abs() - std::f64::consts::TAU).abs()
            <= EPS_PARAMETER_SCALE * raw_span.abs().max(std::f64::consts::TAU);
    let start = raw_start.rem_euclid(std::f64::consts::TAU);
    let mut end = raw_end.rem_euclid(std::f64::consts::TAU);
    if end < start || (full_turn && (end - start).abs() <= EPS_FULL_TURN) {
        end += std::f64::consts::TAU;
    }
    (axis_sign, [start, end])
}

pub(in super::super) fn forward_arc_sweep(start: f64, end: f64) -> f64 {
    let raw_span = end - start;
    if raw_span.is_finite()
        && (raw_span - std::f64::consts::TAU).abs()
            <= EPS_PARAMETER_SCALE * raw_span.abs().max(std::f64::consts::TAU)
    {
        std::f64::consts::TAU
    } else {
        raw_span.rem_euclid(std::f64::consts::TAU)
    }
}

pub(in super::super) fn line_pcurve(start: [f64; 2], end: [f64; 2]) -> PcurveGeometry {
    PcurveGeometry::Line {
        origin: Point2::new(start[0], start[1]),
        direction: Point2::new(end[0] - start[0], end[1] - start[1]),
    }
}

pub(in super::super) fn circular_pcurve(
    center: [f64; 2],
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> PcurveGeometry {
    let segment_count = ((end_angle - start_angle).abs() / std::f64::consts::FRAC_PI_2)
        .ceil()
        .max(1.0) as usize;
    let step = (end_angle - start_angle) / segment_count as f64;
    let mut control_points = Vec::with_capacity(2 * segment_count + 1);
    let mut weights = Vec::with_capacity(2 * segment_count + 1);
    for segment in 0..segment_count {
        let first = start_angle + segment as f64 * step;
        let second = first + step;
        let middle = 0.5 * (first + second);
        let middle_weight = (0.5 * step).cos();
        if segment == 0 {
            control_points.push(Point2::new(
                center[0] + radius * first.cos(),
                center[1] + radius * first.sin(),
            ));
            weights.push(1.0);
        }
        control_points.push(Point2::new(
            center[0] + radius * middle.cos() / middle_weight,
            center[1] + radius * middle.sin() / middle_weight,
        ));
        weights.push(middle_weight);
        control_points.push(Point2::new(
            center[0] + radius * second.cos(),
            center[1] + radius * second.sin(),
        ));
        weights.push(1.0);
    }
    let mut knots = vec![0.0; 3];
    for boundary in 1..segment_count {
        knots.extend([boundary as f64 / segment_count as f64; 2]);
    }
    knots.extend([1.0; 3]);
    cadmpeg_ir::geometry::PcurveNurbs::new(2, knots, control_points, Some(weights), false)
        .map(|nurbs| PcurveGeometry::Nurbs { nurbs })
        .unwrap_or_else(|_| line_pcurve(center, center))
}

pub(in super::super) fn extrusion_cap_pcurve(
    geometry: &SketchGeometry,
    reversed: bool,
    start: [f64; 2],
    end: [f64; 2],
) -> PcurveGeometry {
    match geometry {
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let [start_angle, end_angle] = if reversed {
                [end_angle.0, start_angle.0]
            } else {
                [start_angle.0, end_angle.0]
            };
            circular_pcurve([center.u, center.v], radius.0, start_angle, end_angle)
        }
        SketchGeometry::Circle { center, radius } => {
            let [start_angle, end_angle] = oriented_full_turn_angles(reversed);
            circular_pcurve([center.u, center.v], radius.0, start_angle, end_angle)
        }
        SketchGeometry::Nurbs { .. } => {
            sketch_nurbs_pcurve(geometry, reversed).unwrap_or_else(|| line_pcurve(start, end))
        }
        _ => line_pcurve(start, end),
    }
}

pub(in super::super) fn extrusion_side_uvs(
    geometry: &SketchGeometry,
    reversed: bool,
    start: [f64; 2],
    end: [f64; 2],
    span: ExtrusionSpan,
) -> [[[f64; 2]; 2]; 4] {
    if matches!(geometry, SketchGeometry::Nurbs { .. }) {
        if let Some(nurbs) = oriented_sketch_nurbs_curve(geometry, reversed) {
            if let Some([lower, upper]) = nurbs_intrinsic_parameter_range(&nurbs) {
                return [
                    [[lower, 0.0], [upper, 0.0]],
                    [[upper, 0.0], [upper, 1.0]],
                    [[lower, 1.0], [upper, 1.0]],
                    [[lower, 0.0], [lower, 1.0]],
                ];
            }
        }
    }
    let [first, second] = match geometry {
        SketchGeometry::Arc {
            start_angle,
            end_angle,
            ..
        } if reversed => [end_angle.0, start_angle.0],
        SketchGeometry::Arc {
            start_angle,
            end_angle,
            ..
        } => [start_angle.0, end_angle.0],
        SketchGeometry::Circle { .. } => oriented_full_turn_angles(reversed),
        _ => [0.0, (end[0] - start[0]).hypot(end[1] - start[1])],
    };
    [
        [[first, span.lower], [second, span.lower]],
        [[second, span.lower], [second, span.upper]],
        [[first, span.upper], [second, span.upper]],
        [[first, span.lower], [first, span.upper]],
    ]
}

pub(in super::super) fn extrusion_profile_signed_area(
    profile: &[(SketchGeometry, bool, [f64; 2], [f64; 2])],
) -> Option<f64> {
    let mut area_twice = 0.0;
    for (geometry, reversed, start, end) in profile {
        let contribution = match geometry {
            SketchGeometry::Nurbs { .. } => nurbs_profile_signed_area_twice(geometry, *reversed)?,
            SketchGeometry::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let forward_sweep = forward_arc_sweep(start_angle.0, end_angle.0);
                let sweep = if *reversed {
                    -forward_sweep
                } else {
                    forward_sweep
                };
                center.u.mul_add(
                    end[1] - start[1],
                    -(center.v * (end[0] - start[0])) + radius.0 * radius.0 * sweep,
                )
            }
            SketchGeometry::Circle { center, radius } => {
                let sweep = if *reversed {
                    -std::f64::consts::TAU
                } else {
                    std::f64::consts::TAU
                };
                center.u.mul_add(
                    end[1] - start[1],
                    -(center.v * (end[0] - start[0])) + radius.0 * radius.0 * sweep,
                )
            }
            _ => start[0].mul_add(end[1], -(start[1] * end[0])),
        };
        area_twice += contribution;
    }
    let scale = profile
        .iter()
        .flat_map(|(_, _, start, end)| start.iter().chain(end))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (area_twice.abs() > EPS_AREA * scale * scale).then_some(0.5 * area_twice)
}

pub(in super::super) type ExtrusionProfile = Vec<(SketchGeometry, bool, [f64; 2], [f64; 2])>;

pub(in super::super) fn resolved_sketch_profiles(
    ir: &CadIr,
    sketch_id: &SketchId,
    minimum_entity_count: usize,
) -> Option<Vec<ExtrusionProfile>> {
    let sketch = exactly_one(
        ir.model
            .sketches
            .iter()
            .filter(|sketch| sketch.id == *sketch_id),
    )?;
    (!sketch.profiles.is_empty()).then_some(())?;
    let mut profiles = Vec::new();
    for profile in &sketch.profiles {
        let mut geometries = Vec::new();
        for entity_use in profile {
            let entity = exactly_one(ir.model.sketch_entities.iter().filter(|entity| {
                entity.sketch == *sketch_id && entity.id() == &entity_use.entity
            }))?;
            let (mut start, mut end) = sketch_geometry_endpoints(&entity.geometry)?;
            if entity_use.reversed {
                std::mem::swap(&mut start, &mut end);
            }
            geometries.push((entity.geometry.clone(), entity_use.reversed, start, end));
        }
        (geometries.len() >= minimum_entity_count).then_some(())?;
        let scale = geometries
            .iter()
            .flat_map(|(_, _, start, end)| start.iter().chain(end))
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        geometries
            .iter()
            .enumerate()
            .all(|(index, (_, _, _, end))| {
                let next = geometries[(index + 1) % geometries.len()].2;
                (end[0] - next[0]).hypot(end[1] - next[1]) <= EPS_ENDPOINT_AGREEMENT * scale
            })
            .then_some(())?;
        profiles.push(geometries);
    }
    Some(profiles)
}

#[cfg(test)]
mod tests;

pub(in super::super) fn profile_arc(
    segment: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
) -> Option<([f64; 2], f64, f64, f64)> {
    let (center, radius, start, forward_delta) = match &segment.0 {
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => (
            [center.u, center.v],
            radius.0,
            (segment.2[1] - center.v).atan2(segment.2[0] - center.u),
            forward_arc_sweep(start_angle.0, end_angle.0),
        ),
        SketchGeometry::Circle { center, radius } => {
            ([center.u, center.v], radius.0, 0.0, std::f64::consts::TAU)
        }
        _ => return None,
    };
    let delta = if segment.1 {
        -forward_delta
    } else {
        forward_delta
    };
    Some((center, radius, start, delta))
}

pub(in super::super) fn oriented_full_turn_angles(reversed: bool) -> [f64; 2] {
    if reversed {
        [std::f64::consts::TAU, 0.0]
    } else {
        [0.0, std::f64::consts::TAU]
    }
}

pub(in super::super) fn segments_intersect(
    first: [[f64; 2]; 2],
    second: [[f64; 2]; 2],
    tolerance: f64,
) -> bool {
    let orient = |a: [f64; 2], b: [f64; 2], point: [f64; 2]| {
        (b[0] - a[0]).mul_add(point[1] - a[1], -((b[1] - a[1]) * (point[0] - a[0])))
    };
    let on_segment = |segment: [[f64; 2]; 2], point: [f64; 2]| {
        point[0] >= segment[0][0].min(segment[1][0]) - tolerance
            && point[0] <= segment[0][0].max(segment[1][0]) + tolerance
            && point[1] >= segment[0][1].min(segment[1][1]) - tolerance
            && point[1] <= segment[0][1].max(segment[1][1]) + tolerance
    };
    let orientations = [
        orient(first[0], first[1], second[0]),
        orient(first[0], first[1], second[1]),
        orient(second[0], second[1], first[0]),
        orient(second[0], second[1], first[1]),
    ];
    let first_length = (first[1][0] - first[0][0]).hypot(first[1][1] - first[0][1]);
    let second_length = (second[1][0] - second[0][0]).hypot(second[1][1] - second[0][1]);
    let first_cross_tolerance = tolerance * first_length.max(1.0);
    let second_cross_tolerance = tolerance * second_length.max(1.0);
    let opposite = |left: f64, right: f64, cross_tolerance: f64| {
        (left > cross_tolerance && right < -cross_tolerance)
            || (left < -cross_tolerance && right > cross_tolerance)
    };
    if opposite(orientations[0], orientations[1], first_cross_tolerance)
        && opposite(orientations[2], orientations[3], second_cross_tolerance)
    {
        return true;
    }
    (orientations[0].abs() <= first_cross_tolerance && on_segment(first, second[0]))
        || (orientations[1].abs() <= first_cross_tolerance && on_segment(first, second[1]))
        || (orientations[2].abs() <= second_cross_tolerance && on_segment(second, first[0]))
        || (orientations[3].abs() <= second_cross_tolerance && on_segment(second, first[1]))
}

pub(in super::super) fn point_on_profile_arc(
    point: [f64; 2],
    arc: ([f64; 2], f64, f64, f64),
    tolerance: f64,
) -> bool {
    let (center, radius, start, delta) = arc;
    let relative = [point[0] - center[0], point[1] - center[1]];
    let distance = relative[0].hypot(relative[1]);
    if (distance - radius).abs() > tolerance {
        return false;
    }
    let angle = relative[1].atan2(relative[0]);
    let travel = if delta >= 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU)
    } else {
        (start - angle).rem_euclid(std::f64::consts::TAU)
    };
    travel <= delta.abs() + tolerance / radius.max(1.0)
}

pub(in super::super) fn line_arc_intersect(
    line: [[f64; 2]; 2],
    arc: ([f64; 2], f64, f64, f64),
    tolerance: f64,
) -> bool {
    let direction = [line[1][0] - line[0][0], line[1][1] - line[0][1]];
    let relative = [line[0][0] - arc.0[0], line[0][1] - arc.0[1]];
    let a = direction[0].mul_add(direction[0], direction[1] * direction[1]);
    let b = 2.0 * direction[0].mul_add(relative[0], direction[1] * relative[1]);
    let c = relative[0].mul_add(relative[0], relative[1] * relative[1]) - arc.1 * arc.1;
    let discriminant = b.mul_add(b, -(4.0 * a * c));
    if a <= tolerance * tolerance || discriminant < -tolerance * tolerance {
        return false;
    }
    let root = discriminant.max(0.0).sqrt();
    [-root, root].into_iter().any(|signed_root| {
        let parameter = (-b + signed_root) / (2.0 * a);
        parameter >= -tolerance
            && parameter <= 1.0 + tolerance
            && point_on_profile_arc(
                [
                    line[0][0] + parameter * direction[0],
                    line[0][1] + parameter * direction[1],
                ],
                arc,
                tolerance,
            )
    })
}

pub(in super::super) fn arcs_intersect(
    first: ([f64; 2], f64, f64, f64),
    second: ([f64; 2], f64, f64, f64),
    tolerance: f64,
) -> bool {
    let displacement = [second.0[0] - first.0[0], second.0[1] - first.0[1]];
    let distance = displacement[0].hypot(displacement[1]);
    if distance <= tolerance && (first.1 - second.1).abs() <= tolerance {
        let endpoints = |arc: ([f64; 2], f64, f64, f64)| {
            [
                [
                    arc.0[0] + arc.1 * arc.2.cos(),
                    arc.0[1] + arc.1 * arc.2.sin(),
                ],
                [
                    arc.0[0] + arc.1 * (arc.2 + arc.3).cos(),
                    arc.0[1] + arc.1 * (arc.2 + arc.3).sin(),
                ],
            ]
        };
        return endpoints(first)
            .into_iter()
            .any(|point| point_on_profile_arc(point, second, tolerance))
            || endpoints(second)
                .into_iter()
                .any(|point| point_on_profile_arc(point, first, tolerance));
    }
    if distance <= tolerance
        || distance > first.1 + second.1 + tolerance
        || distance < (first.1 - second.1).abs() - tolerance
    {
        return false;
    }
    let along = (first.1 * first.1 - second.1 * second.1 + distance * distance) / (2.0 * distance);
    let height_squared = first.1 * first.1 - along * along;
    if height_squared < -tolerance * tolerance {
        return false;
    }
    let base = [
        first.0[0] + along * displacement[0] / distance,
        first.0[1] + along * displacement[1] / distance,
    ];
    let height = height_squared.max(0.0).sqrt();
    let offset = [
        -height * displacement[1] / distance,
        height * displacement[0] / distance,
    ];
    [-1.0, 1.0].into_iter().any(|sign| {
        let point = [base[0] + sign * offset[0], base[1] + sign * offset[1]];
        point_on_profile_arc(point, first, tolerance)
            && point_on_profile_arc(point, second, tolerance)
    })
}

pub(in super::super) fn planar_point_segment_distance(
    point: [f64; 2],
    segment: [[f64; 2]; 2],
) -> f64 {
    let direction = [segment[1][0] - segment[0][0], segment[1][1] - segment[0][1]];
    let relative = [point[0] - segment[0][0], point[1] - segment[0][1]];
    let length_squared = direction[0].mul_add(direction[0], direction[1] * direction[1]);
    if length_squared == 0.0 {
        return relative[0].hypot(relative[1]);
    }
    let parameter = (relative[0].mul_add(direction[0], relative[1] * direction[1])
        / length_squared)
        .clamp(0.0, 1.0);
    let nearest = [
        segment[0][0] + parameter * direction[0],
        segment[0][1] + parameter * direction[1],
    ];
    (point[0] - nearest[0]).hypot(point[1] - nearest[1])
}

pub(in super::super) const NURBS_AREA_GAUSS_NODES: [f64; 8] = [
    -0.960_289_856_497_536_3,
    -0.796_666_477_413_626_7,
    -0.525_532_409_916_329,
    -0.183_434_642_495_649_8,
    0.183_434_642_495_649_8,
    0.525_532_409_916_329,
    0.796_666_477_413_626_7,
    0.960_289_856_497_536_3,
];
pub(in super::super) const NURBS_AREA_GAUSS_WEIGHTS: [f64; 8] = [
    0.101_228_536_290_376_3,
    0.222_381_034_453_374_5,
    0.313_706_645_877_887_3,
    0.362_683_783_378_362,
    0.362_683_783_378_362,
    0.313_706_645_877_887_3,
    0.222_381_034_453_374_5,
    0.101_228_536_290_376_3,
];

pub(in super::super) struct NurbsProfileSpan<'a> {
    pub(in super::super) carrier: &'a CurveGeometry,
    pub(in super::super) start: f64,
    pub(in super::super) end: f64,
    pub(in super::super) start_point: [f64; 2],
    pub(in super::super) end_point: [f64; 2],
    pub(in super::super) tolerance: f64,
    pub(in super::super) depth: usize,
}

pub(in super::super) fn append_nurbs_profile_span(
    span: &NurbsProfileSpan<'_>,
    points: &mut Vec<[f64; 2]>,
) -> Option<()> {
    const MAX_DEPTH: usize = 24;
    const MAX_POINTS: usize = 262_145;
    (span.start.is_finite() && span.end.is_finite() && span.start < span.end).then_some(())?;
    let middle = span.start + (span.end - span.start) * 0.5;
    if middle == span.start || middle == span.end {
        (points.len() < MAX_POINTS).then_some(())?;
        points.push(span.end_point);
        return Some(());
    }
    let first_quarter = span.start + (span.end - span.start) * 0.25;
    let third_quarter = span.start + (span.end - span.start) * 0.75;
    let middle_point = cadmpeg_ir::eval::curve_point(span.carrier, middle)?;
    let first_quarter_point = cadmpeg_ir::eval::curve_point(span.carrier, first_quarter)?;
    let third_quarter_point = cadmpeg_ir::eval::curve_point(span.carrier, third_quarter)?;
    let middle_point = [middle_point.x, middle_point.y];
    let first_quarter_point = [first_quarter_point.x, first_quarter_point.y];
    let third_quarter_point = [third_quarter_point.x, third_quarter_point.y];
    let chord = [span.start_point, span.end_point];
    let flatness = planar_point_segment_distance(first_quarter_point, chord)
        .max(planar_point_segment_distance(middle_point, chord))
        .max(planar_point_segment_distance(third_quarter_point, chord));
    (flatness.is_finite() && span.tolerance.is_finite() && span.tolerance > 0.0).then_some(())?;
    if flatness <= span.tolerance {
        (points.len() < MAX_POINTS).then_some(())?;
        points.push(span.end_point);
        return Some(());
    }
    (span.depth < MAX_DEPTH).then_some(())?;
    append_nurbs_profile_span(
        &NurbsProfileSpan {
            carrier: span.carrier,
            start: span.start,
            end: middle,
            start_point: span.start_point,
            end_point: middle_point,
            tolerance: span.tolerance,
            depth: span.depth + 1,
        },
        points,
    )?;
    append_nurbs_profile_span(
        &NurbsProfileSpan {
            carrier: span.carrier,
            start: middle,
            end: span.end,
            start_point: middle_point,
            end_point: span.end_point,
            tolerance: span.tolerance,
            depth: span.depth + 1,
        },
        points,
    )
}

pub(in super::super) fn nurbs_profile_polyline(
    nurbs: &NurbsCurve,
    tolerance: f64,
) -> Option<Vec<[f64; 2]>> {
    let [lower, upper] = nurbs_intrinsic_parameter_range(nurbs)?;
    let carrier = CurveGeometry::Nurbs(nurbs.clone());
    let first = cadmpeg_ir::eval::curve_point(&carrier, lower)?;
    let first = [first.x, first.y];
    let mut points = vec![first];
    for pair in nurbs.knots().windows(2) {
        let start = pair[0].max(lower);
        let end = pair[1].min(upper);
        if start >= end {
            continue;
        }
        let start_point = cadmpeg_ir::eval::curve_point(&carrier, start)?;
        let end_point = cadmpeg_ir::eval::curve_point(&carrier, end)?;
        let start_point = [start_point.x, start_point.y];
        let end_point = [end_point.x, end_point.y];
        if points.last().copied() != Some(start_point) {
            points.push(start_point);
        }
        append_nurbs_profile_span(
            &NurbsProfileSpan {
                carrier: &carrier,
                start,
                end,
                start_point,
                end_point,
                tolerance,
                depth: 0,
            },
            &mut points,
        )?;
    }
    (points.len() >= 2 && points.iter().flatten().all(|value| value.is_finite())).then_some(points)
}

pub(in super::super) fn profile_nurbs_polyline(
    segment: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    tolerance: f64,
) -> Option<Vec<[f64; 2]>> {
    let nurbs = oriented_sketch_nurbs_curve(&segment.0, segment.1)?;
    nurbs_profile_polyline(&nurbs, tolerance)
}

pub(in super::super) fn nurbs_profile_signed_area_twice(
    geometry: &SketchGeometry,
    reversed: bool,
) -> Option<f64> {
    let nurbs = oriented_sketch_nurbs_curve(geometry, reversed)?;
    let [lower, upper] = nurbs_intrinsic_parameter_range(&nurbs)?;
    let carrier = CurveGeometry::Nurbs(nurbs.clone());
    let mut area_twice = 0.0;
    for pair in nurbs.knots().windows(2) {
        let start = pair[0].max(lower);
        let end = pair[1].min(upper);
        if start >= end {
            continue;
        }
        let middle = 0.5 * (start + end);
        let half_width = 0.5 * (end - start);
        for (node, weight) in NURBS_AREA_GAUSS_NODES
            .into_iter()
            .zip(NURBS_AREA_GAUSS_WEIGHTS)
        {
            let parameter = middle + half_width * node;
            let point = cadmpeg_ir::eval::curve_point(&carrier, parameter)?;
            let tangent = cadmpeg_ir::eval::curve_tangent(&carrier, parameter)?;
            area_twice += weight * (point.x * tangent.y - point.y * tangent.x) * half_width;
        }
    }
    area_twice.is_finite().then_some(area_twice)
}

pub(in super::super) fn polylines_intersect(
    first: &[[f64; 2]],
    second: &[[f64; 2]],
    tolerance: f64,
) -> bool {
    first.windows(2).any(|first_segment| {
        second.windows(2).any(|second_segment| {
            segments_intersect(
                [first_segment[0], first_segment[1]],
                [second_segment[0], second_segment[1]],
                tolerance,
            )
        })
    })
}

pub(in super::super) fn profile_segments_intersect(
    first: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    second: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    tolerance: f64,
) -> bool {
    let first_nurbs = matches!(first.0, SketchGeometry::Nurbs { .. });
    let second_nurbs = matches!(second.0, SketchGeometry::Nurbs { .. });
    if first_nurbs || second_nurbs {
        if first_nurbs {
            if let Some(arc) = profile_arc(second) {
                return profile_nurbs_polyline(first, tolerance).is_some_and(|polyline| {
                    polyline
                        .windows(2)
                        .any(|segment| line_arc_intersect([segment[0], segment[1]], arc, tolerance))
                });
            }
        }
        if second_nurbs {
            if let Some(arc) = profile_arc(first) {
                return profile_nurbs_polyline(second, tolerance).is_some_and(|polyline| {
                    polyline
                        .windows(2)
                        .any(|segment| line_arc_intersect([segment[0], segment[1]], arc, tolerance))
                });
            }
        }
        let Some(first_polyline) = (if first_nurbs {
            profile_nurbs_polyline(first, tolerance)
        } else {
            Some(vec![first.2, first.3])
        }) else {
            return true;
        };
        let Some(second_polyline) = (if second_nurbs {
            profile_nurbs_polyline(second, tolerance)
        } else {
            Some(vec![second.2, second.3])
        }) else {
            return true;
        };
        return polylines_intersect(&first_polyline, &second_polyline, tolerance);
    }
    match (profile_arc(first), profile_arc(second)) {
        (None, None) => segments_intersect([first.2, first.3], [second.2, second.3], tolerance),
        (None, Some(arc)) => line_arc_intersect([first.2, first.3], arc, tolerance),
        (Some(arc), None) => line_arc_intersect([second.2, second.3], arc, tolerance),
        (Some(first), Some(second)) => arcs_intersect(first, second, tolerance),
    }
}

pub(in super::super) fn profile_strictly_contains(
    profile: &ExtrusionProfile,
    point: [f64; 2],
) -> bool {
    let scale = profile
        .iter()
        .flat_map(|(_, _, start, end)| start.iter().chain(end))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let tolerance = EPS_GEOMETRY_AGREEMENT * scale;
    let mut winding = 0.0;
    for segment in profile {
        let mut accumulate = |first: [f64; 2], second: [f64; 2]| {
            let first = [first[0] - point[0], first[1] - point[1]];
            let second = [second[0] - point[0], second[1] - point[1]];
            winding += first[0]
                .mul_add(second[1], -(first[1] * second[0]))
                .atan2(first[0].mul_add(second[0], first[1] * second[1]));
        };
        if matches!(segment.0, SketchGeometry::Nurbs { .. }) {
            let Some(polyline) = profile_nurbs_polyline(segment, tolerance) else {
                return false;
            };
            for pair in polyline.windows(2) {
                accumulate(pair[0], pair[1]);
            }
        } else if let Some((center, radius, start, delta)) = profile_arc(segment) {
            let pieces = (delta.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
            for piece in 0..pieces {
                let first = start + delta * piece as f64 / pieces as f64;
                let second = start + delta * (piece + 1) as f64 / pieces as f64;
                accumulate(
                    [
                        center[0] + radius * first.cos(),
                        center[1] + radius * first.sin(),
                    ],
                    [
                        center[0] + radius * second.cos(),
                        center[1] + radius * second.sin(),
                    ],
                );
            }
        } else {
            accumulate(segment.2, segment.3);
        }
    }
    winding.abs() > std::f64::consts::PI
}

pub(in super::super) fn ordered_extrusion_profiles(
    mut profiles: Vec<ExtrusionProfile>,
) -> Option<(Vec<ExtrusionProfile>, f64)> {
    let scale = profiles
        .iter()
        .flatten()
        .flat_map(|(_, _, start, end)| start.iter().chain(end))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let tolerance = EPS_GEOMETRY_AGREEMENT * scale;
    for profile in &profiles {
        for first in 0..profile.len() {
            for second in first + 1..profile.len() {
                if second == first + 1 || (first == 0 && second + 1 == profile.len()) {
                    continue;
                }
                if profile_segments_intersect(&profile[first], &profile[second], tolerance) {
                    return None;
                }
            }
        }
    }
    for first in 0..profiles.len() {
        for second in first + 1..profiles.len() {
            for first_segment in &profiles[first] {
                for second_segment in &profiles[second] {
                    if profile_segments_intersect(first_segment, second_segment, tolerance) {
                        return None;
                    }
                }
            }
        }
    }
    let outer = profiles
        .iter()
        .enumerate()
        .filter(|(candidate, profile)| {
            profiles.iter().enumerate().all(|(index, inner)| {
                index == *candidate || profile_strictly_contains(profile, inner[0].2)
            })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [outer] = outer.as_slice() else {
        return None;
    };
    for first in 0..profiles.len() {
        if first == *outer {
            continue;
        }
        for second in first + 1..profiles.len() {
            if second == *outer {
                continue;
            }
            if profile_strictly_contains(&profiles[first], profiles[second][0].2)
                || profile_strictly_contains(&profiles[second], profiles[first][0].2)
            {
                return None;
            }
        }
    }
    let outer_area = extrusion_profile_signed_area(&profiles[*outer])?;
    if profiles.iter().enumerate().any(|(index, profile)| {
        index != *outer
            && extrusion_profile_signed_area(profile)
                .is_none_or(|area| area.is_sign_positive() == outer_area.is_sign_positive())
    }) {
        return None;
    }
    profiles.swap(0, *outer);
    Some((profiles, outer_area))
}
