// SPDX-License-Identifier: Apache-2.0
//! Pcurve-layer transfer: surface-chart curve lowering, orientation solving,
//! and the pcurve emit pass.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::curve_point;
use cadmpeg_ir::geometry::{
    CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, ProceduralCurveDefinition,
};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::super::graph::{evaluate_pcurve, B5Graph, B5Pcurve, B5Surface};
use super::super::vecmath::{add, cross, scale};
use super::edges::{edge_pcurve_parameters, ordered_subrange};
use super::{
    annotate, distance, dot, expand_knots, length, point3, subtract, vector, CurvePlan, HelixPlan,
    TransferPlan, POINT_TOLERANCE,
};

pub(super) fn oriented_line_plan(
    geometry: &CurveGeometry,
    edge_start: [f64; 3],
    edge_end: [f64; 3],
) -> Option<CurvePlan> {
    let CurveGeometry::Line { origin, direction } = geometry else {
        return None;
    };
    let origin = [origin.x, origin.y, origin.z];
    let mut direction = [direction.x, direction.y, direction.z];
    let direction_length = length(direction);
    if !direction_length.is_finite() || direction_length <= f64::EPSILON {
        return None;
    }
    direction = scale(direction, 1.0 / direction_length);
    let parameter = |point| dot(subtract(point, origin), direction);
    let mut range = [parameter(edge_start), parameter(edge_end)];
    if !range.into_iter().all(f64::is_finite) || range[0] == range[1] {
        return None;
    }
    let projected = range.map(|value| add(origin, scale(direction, value)));
    let residual = distance(projected[0], edge_start).max(distance(projected[1], edge_end));
    if residual > POINT_TOLERANCE {
        return None;
    }
    if range[0] > range[1] {
        direction = scale(direction, -1.0);
        range = [-range[0], -range[1]];
    }
    Some(CurvePlan {
        geometry: CurveGeometry::Line {
            origin: point3(origin),
            direction: vector(direction),
        },
        parameter_range: Some(range),
        edge_tolerance: (residual > 1e-9).then_some(residual + 1e-9),
        cache_fit_tolerance: None,
    })
}

pub(super) fn oriented_circle_plan(
    pcurve: &B5Pcurve,
    surface: &B5Surface,
    geometry: &CurveGeometry,
    endpoint_parameters: [f64; 2],
    edge_start: [f64; 3],
    edge_end: [f64; 3],
) -> Option<CurvePlan> {
    let (dimension, scale) = isoparametric_angle_coordinate(pcurve, surface)?;
    if !scale.is_finite() || scale.abs() <= f64::EPSILON {
        return None;
    }
    if let Some(weights) = &pcurve.weights {
        if weights.len() != pcurve.control_points.len()
            || weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
        {
            return None;
        }
    }
    let endpoints = endpoint_parameters.map(|parameter| evaluate_pcurve(pcurve, parameter));
    let [Some(start_uv), Some(end_uv)] = endpoints else {
        return None;
    };
    let angles = [start_uv[dimension] / scale, end_uv[dimension] / scale];
    let delta = angles[1] - angles[0];
    if !delta.is_finite() || delta.abs() <= 1e-12 || delta.abs() > std::f64::consts::TAU + 1e-9 {
        return None;
    }
    let direction = delta.signum();
    if pcurve
        .control_points
        .windows(2)
        .any(|points| direction * (points[1][dimension] - points[0][dimension]) / scale < -1e-12)
    {
        return None;
    }

    let CurveGeometry::Circle {
        center,
        axis,
        ref_direction,
        radius,
    } = geometry
    else {
        return None;
    };
    if !radius.is_finite() || radius.abs() <= f64::EPSILON {
        return None;
    }
    let mut axis = *axis;
    let mut ref_direction = *ref_direction;
    let radius = if *radius < 0.0 {
        ref_direction = Vector3::new(-ref_direction.x, -ref_direction.y, -ref_direction.z);
        -*radius
    } else {
        *radius
    };
    let oriented_angles = if delta < 0.0 {
        axis = Vector3::new(-axis.x, -axis.y, -axis.z);
        [-angles[0], -angles[1]]
    } else {
        angles
    };
    let parameter_range = crate::nurbs::canonical_periodic_range(oriented_angles)?;
    let geometry = CurveGeometry::Circle {
        center: *center,
        axis,
        ref_direction,
        radius,
    };
    let evaluated = parameter_range.map(|parameter| curve_point(&geometry, parameter));
    let [Some(start), Some(end)] = evaluated else {
        return None;
    };
    let residual = distance([start.x, start.y, start.z], edge_start)
        .max(distance([end.x, end.y, end.z], edge_end));
    if residual > POINT_TOLERANCE {
        return None;
    }
    Some(CurvePlan {
        geometry,
        parameter_range: Some(parameter_range),
        edge_tolerance: (residual > 1e-9).then_some(residual + 1e-9),
        cache_fit_tolerance: None,
    })
}

pub(super) fn isoparametric_angle_coordinate(
    pcurve: &B5Pcurve,
    surface: &B5Surface,
) -> Option<(usize, f64)> {
    match surface {
        B5Surface::Cylinder { radius, .. }
            if constant_coordinate(&pcurve.control_points, 1).is_some() =>
        {
            Some((0, *radius))
        }
        B5Surface::Cone { angular_scale, .. }
            if constant_coordinate(&pcurve.control_points, 1).is_some() =>
        {
            Some((0, *angular_scale))
        }
        B5Surface::Torus { minor_scale, .. }
            if constant_coordinate(&pcurve.control_points, 0).is_some() =>
        {
            Some((1, *minor_scale))
        }
        B5Surface::Torus { major_scale, .. }
            if constant_coordinate(&pcurve.control_points, 1).is_some() =>
        {
            Some((0, *major_scale))
        }
        _ => None,
    }
}

pub(super) fn oriented_nurbs_range(
    geometry: CurveGeometry,
    endpoint_parameters: [f64; 2],
    edge_start: [f64; 3],
    edge_end: [f64; 3],
) -> Option<CurvePlan> {
    let CurveGeometry::Nurbs(mut curve) = geometry else {
        return None;
    };
    let degree = usize::try_from(curve.degree).ok()?;
    let domain_start = *curve.knots.get(degree)?;
    let domain_end = *curve
        .knots
        .len()
        .checked_sub(degree + 1)
        .and_then(|index| curve.knots.get(index))?;
    let mut range = endpoint_parameters;
    if range[0] > range[1] {
        let sum = domain_start + domain_end;
        curve.knots = curve
            .knots
            .into_iter()
            .rev()
            .map(|knot| sum - knot)
            .collect();
        curve.control_points.reverse();
        if let Some(weights) = &mut curve.weights {
            weights.reverse();
        }
        range = [sum - range[0], sum - range[1]];
    }
    if !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] >= range[1]
        || range[0] < domain_start
        || range[1] > domain_end
    {
        return None;
    }
    let geometry = CurveGeometry::Nurbs(curve);
    let start = curve_point(&geometry, range[0])?;
    let end = curve_point(&geometry, range[1])?;
    let residual = distance([start.x, start.y, start.z], edge_start)
        .max(distance([end.x, end.y, end.z], edge_end));
    if residual > POINT_TOLERANCE {
        return None;
    }
    Some(CurvePlan {
        geometry,
        parameter_range: Some(range),
        edge_tolerance: (residual > 1e-9).then_some(residual + 1e-9),
        cache_fit_tolerance: None,
    })
}

pub(super) fn isocurve_endpoint_parameters(
    pcurve: &B5Pcurve,
    endpoint_parameters: [f64; 2],
) -> Option<[f64; 2]> {
    let varying_dimension = if constant_coordinate(&pcurve.control_points, 0).is_some() {
        1
    } else if constant_coordinate(&pcurve.control_points, 1).is_some() {
        0
    } else {
        return None;
    };
    if let Some(weights) = &pcurve.weights {
        if weights.len() != pcurve.control_points.len()
            || weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
        {
            return None;
        }
    }
    if pcurve
        .control_points
        .iter()
        .any(|point| !point[varying_dimension].is_finite())
        || (!pcurve
            .control_points
            .windows(2)
            .all(|pair| pair[0][varying_dimension] <= pair[1][varying_dimension])
            && !pcurve
                .control_points
                .windows(2)
                .all(|pair| pair[0][varying_dimension] >= pair[1][varying_dimension]))
    {
        return None;
    }
    let values = endpoint_parameters
        .map(|parameter| evaluate_pcurve(pcurve, parameter))
        .map(|uv| uv.map(|point| point[varying_dimension]));
    let [Some(start), Some(end)] = values else {
        return None;
    };
    Some([start, end])
}

pub(super) fn neutral_pcurve_point(point: [f64; 2], surface: &B5Surface) -> Point2 {
    match surface {
        B5Surface::Cylinder { radius, .. } => Point2::new(point[0] / radius, point[1]),
        B5Surface::Cone {
            half_angle,
            slant_range,
            angular_scale,
            ..
        } => Point2::new(
            point[0] / angular_scale,
            (point[1] - slant_range[0]) * half_angle.cos(),
        ),
        B5Surface::Torus {
            major_scale,
            minor_scale,
            ..
        } => Point2::new(point[0] / major_scale, point[1] / minor_scale),
        _ => Point2::new(point[0], point[1]),
    }
}

pub(super) fn lifted_curve_geometry(
    pcurve: &B5Pcurve,
    surface: &B5Surface,
) -> Option<CurveGeometry> {
    let knots = expand_knots(&pcurve.distinct_knots, &pcurve.multiplicities)?;
    match surface {
        B5Surface::UnresolvedNurbs { .. }
        | B5Surface::Unknown { .. }
        | B5Surface::RollingBall { .. }
        | B5Surface::Sphere { .. } => None,
        B5Surface::Plane {
            origin,
            direction_u,
            direction_v,
        } => Some(CurveGeometry::Nurbs(NurbsCurve {
            degree: pcurve.degree,
            knots,
            control_points: pcurve
                .control_points
                .iter()
                .map(|uv| {
                    point3(add(
                        *origin,
                        add(scale(*direction_u, uv[0]), scale(*direction_v, uv[1])),
                    ))
                })
                .collect(),
            weights: pcurve.weights.clone(),
            periodic: false,
        })),
        B5Surface::Cylinder {
            origin,
            reference_x,
            axis,
            radius,
        } if constant_coordinate(&pcurve.control_points, 0).is_some() => {
            let first = pcurve.control_points.first()?;
            let line_origin = cylinder_point(*origin, *reference_x, *axis, *radius, *first);
            Some(CurveGeometry::Line {
                origin: point3(line_origin),
                direction: vector(*axis),
            })
        }
        B5Surface::Cone {
            apex,
            direction_x,
            direction_y,
            axis,
            half_angle,
            angular_scale,
            ..
        } if constant_coordinate(&pcurve.control_points, 0).is_some() => {
            let [u, _] = *pcurve.control_points.first()?;
            let angle = u / angular_scale;
            let radial = add(
                scale(*direction_x, angle.cos()),
                scale(*direction_y, angle.sin()),
            );
            Some(CurveGeometry::Line {
                origin: point3(*apex),
                direction: vector(add(
                    scale(*axis, half_angle.cos()),
                    scale(radial, half_angle.sin()),
                )),
            })
        }
        B5Surface::Torus {
            center,
            direction_x,
            direction_y,
            axis,
            major_radius,
            minor_radius,
            major_scale,
            minor_scale,
        } if constant_coordinate(&pcurve.control_points, 0).is_some() => {
            let u = pcurve.control_points.first()?[0];
            let angle = u / major_scale;
            let radial = add(
                scale(*direction_x, angle.cos()),
                scale(*direction_y, angle.sin()),
            );
            Some(CurveGeometry::Circle {
                center: point3(add(*center, scale(radial, *major_radius))),
                axis: vector(cross(radial, *axis)),
                ref_direction: vector(radial),
                radius: *minor_radius,
            })
        }
        B5Surface::Torus {
            center,
            direction_x,
            axis,
            major_radius,
            minor_radius,
            minor_scale,
            ..
        } => {
            let v = constant_coordinate(&pcurve.control_points, 1)?;
            let angle = v / minor_scale;
            let signed_radius = major_radius + minor_radius * angle.cos();
            (signed_radius.abs() > f64::EPSILON).then_some(())?;
            Some(CurveGeometry::Circle {
                center: point3(add(*center, scale(*axis, minor_radius * angle.sin()))),
                axis: vector(*axis),
                ref_direction: vector(scale(*direction_x, signed_radius.signum())),
                radius: signed_radius.abs(),
            })
        }
        B5Surface::Cone {
            apex,
            direction_x,
            axis,
            half_angle,
            ..
        } => {
            let slant = constant_coordinate(&pcurve.control_points, 1)?;
            (slant.abs() > f64::EPSILON).then_some(())?;
            Some(CurveGeometry::Circle {
                center: point3(add(*apex, scale(*axis, slant * half_angle.cos()))),
                axis: vector(*axis),
                ref_direction: vector(*direction_x),
                radius: slant * half_angle.sin(),
            })
        }
        B5Surface::Cylinder {
            origin,
            reference_x,
            axis,
            radius,
        } => {
            let v = constant_coordinate(&pcurve.control_points, 1)?;
            Some(CurveGeometry::Circle {
                center: point3(add(*origin, scale(*axis, v))),
                axis: vector(*axis),
                ref_direction: vector(*reference_x),
                radius: *radius,
            })
        }
        B5Surface::Nurbs(surface) => nurbs_isocurve(pcurve, surface).map(CurveGeometry::Nurbs),
        B5Surface::Revolution { .. } => None,
    }
}

pub(super) fn nurbs_isocurve(pcurve: &B5Pcurve, surface: &NurbsSurface) -> Option<NurbsCurve> {
    if let Some(u) = constant_coordinate(&pcurve.control_points, 0) {
        crate::nurbs::nurbs_surface_isocurve(surface, u, true)
    } else if let Some(v) = constant_coordinate(&pcurve.control_points, 1) {
        crate::nurbs::nurbs_surface_isocurve(surface, v, false)
    } else {
        None
    }
}

pub(super) fn constant_coordinate(points: &[[f64; 2]], dimension: usize) -> Option<f64> {
    let value = points.first()?[dimension];
    points
        .iter()
        .all(|point| (point[dimension] - value).abs() <= 1e-12 * (1.0 + value.abs()))
        .then_some(value)
}

pub(super) fn cylinder_point(
    origin: [f64; 3],
    reference_x: [f64; 3],
    axis: [f64; 3],
    radius: f64,
    uv: [f64; 2],
) -> [f64; 3] {
    let reference_y = cross(axis, reference_x);
    let angle = uv[0] / radius;
    add(
        origin,
        add(
            scale(
                add(
                    scale(reference_x, angle.cos()),
                    scale(reference_y, angle.sin()),
                ),
                radius,
            ),
            scale(axis, uv[1]),
        ),
    )
}

pub(super) fn cylinder_helix(
    pcurve: &B5Pcurve,
    surface: &B5Surface,
    endpoint_parameters: [f64; 2],
    edge_start: [f64; 3],
    edge_end: [f64; 3],
) -> Option<HelixPlan> {
    const FIT_TOLERANCE: f64 = 1e-4;

    let B5Surface::Cylinder {
        origin,
        reference_x,
        axis,
        radius,
    } = surface
    else {
        return None;
    };
    if pcurve.degree != 1 || pcurve.control_points.len() != 2 {
        return None;
    }
    let endpoints = endpoint_parameters.map(|parameter| evaluate_pcurve(pcurve, parameter));
    let [Some(first), Some(second)] = endpoints else {
        return None;
    };
    let mut endpoints = [first, second];
    let lifted = endpoints.map(|uv| cylinder_point(*origin, *reference_x, *axis, *radius, uv));
    let forward_error = distance(lifted[0], edge_start).max(distance(lifted[1], edge_end));
    let reverse_error = distance(lifted[1], edge_start).max(distance(lifted[0], edge_end));
    if (forward_error - reverse_error).abs() <= 1e-12
        || forward_error.min(reverse_error) > POINT_TOLERANCE
    {
        return None;
    }
    if reverse_error < forward_error {
        endpoints.swap(0, 1);
    }
    let angles = [endpoints[0][0] / radius, endpoints[1][0] / radius];
    let delta_angle = angles[1] - angles[0];
    let delta_height = endpoints[1][1] - endpoints[0][1];
    if delta_angle.abs() <= 1e-12 || delta_height.abs() <= 1e-12 {
        return None;
    }
    let reference_y = cross(*axis, *reference_x);
    let radial = add(
        scale(*reference_x, angles[0].cos()),
        scale(reference_y, angles[0].sin()),
    );
    let tangent = cross(*axis, radial);
    let sweep = delta_angle.abs();
    let definition = ProceduralCurveDefinition::Helix {
        angle_range: [0.0, sweep],
        center: point3(add(*origin, scale(*axis, endpoints[0][1]))),
        major: vector(scale(radial, *radius)),
        minor: vector(scale(tangent, radius * delta_angle.signum())),
        pitch: vector(scale(
            *axis,
            delta_height / sweep * 2.0 * std::f64::consts::PI,
        )),
        apex_factor: 0.0,
        axis: vector(*axis),
    };
    let cache = crate::nurbs::circular_helix_cache(&definition, FIT_TOLERANCE)?;
    let cache_start = cache.curve.control_points.first()?;
    let cache_end = cache.curve.control_points.last()?;
    if distance([cache_start.x, cache_start.y, cache_start.z], edge_start) > POINT_TOLERANCE
        || distance([cache_end.x, cache_end.y, cache_end.z], edge_end) > POINT_TOLERANCE
    {
        return None;
    }
    Some(HelixPlan {
        definition,
        cache: cache.curve,
        parameter_range: [0.0, sweep],
        fit_tolerance: cache.fit_tolerance,
    })
}

/// Emit distinct pcurve occurrences grouped by native parameter range,
/// returning the map from `(loop_id, member_index)` to emitted [`PcurveId`].
pub(super) fn emit_pcurves(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    graph: &B5Graph,
    plan: &TransferPlan,
) -> HashMap<(u32, usize), PcurveId> {
    let pcurve_plan = &plan.pcurve_plan;
    let mut occurrence_groups = BTreeMap::<u32, BTreeMap<[u64; 2], Vec<(u32, usize)>>>::new();
    for loop_ in graph.loops.values() {
        for (index, (&object_id, &edge_id)) in loop_.pcurves.iter().zip(&loop_.edges).enumerate() {
            let Some((_, _, native_range)) = pcurve_plan.get(&object_id) else {
                continue;
            };
            let parameter_range = edge_pcurve_parameters(graph, edge_id, object_id)
                .and_then(|parameters| ordered_subrange(parameters, *native_range))
                .unwrap_or(*native_range);
            occurrence_groups
                .entry(object_id)
                .or_default()
                .entry(parameter_range.map(|parameter| {
                    if parameter == 0.0 {
                        0.0f64.to_bits()
                    } else {
                        parameter.to_bits()
                    }
                }))
                .or_default()
                .push((loop_.object_id, index));
        }
    }
    let mut pcurve_ids = HashMap::new();
    for (object_id, ranges) in occurrence_groups {
        let (geometry, cylinder_reparameterized, _) = &pcurve_plan[&object_id];
        let range_count = ranges.len();
        for (rank, (range_bits, occurrences)) in ranges.into_iter().enumerate() {
            let id = if range_count == 1 {
                PcurveId(format!("catia:b5:pcurve#{object_id}"))
            } else {
                PcurveId(format!("catia:b5:pcurve#{object_id}@{rank}"))
            };
            annotate(
                annotations,
                &id,
                "object_stream_b5_03",
                "21_pcurve",
                Exactness::ByteExact,
            );
            if *cylinder_reparameterized {
                annotations.derived(&id, "geometry.control_points");
            }
            annotations.derived(&id, "parameter_range");
            for occurrence in occurrences {
                pcurve_ids.insert(occurrence, id.clone());
            }
            ir.model.pcurves.push(Pcurve {
                id,
                geometry: geometry.clone(),
                wrapper_reversed: None,
                parameter_range: Some(range_bits.map(f64::from_bits)),
                fit_tolerance: None,
                native_tail_flags: None,
            });
        }
    }
    pcurve_ids
}
