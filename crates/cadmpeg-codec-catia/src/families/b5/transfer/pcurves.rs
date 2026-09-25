// SPDX-License-Identifier: Apache-2.0
//! Pcurve-layer transfer: surface-chart curve lowering, orientation solving,
//! and the pcurve emit pass.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::curve_point;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    pcurve::{Pcurve, PcurveGeometry},
    CurveGeometry, ProceduralCurveDefinition, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{FiniteReal, PositiveLength, PositiveReal};
use cadmpeg_ir::units::{FiniteVector, UnitVector3};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::super::graph::{
    edge_pcurve_parameters, evaluate_pcurve, pcurve_nurbs_knots, B5Graph, B5Pcurve,
    B5SphereGreatCirclePcurve, B5Surface,
};
use super::super::vecmath::{add, components, coordinates, cross, scale};
use super::edges::ordered_subrange;
use super::{
    annotate, dot, point3, subtract, vector, CurvePlan, HelixPlan, TransferPlan, POINT_TOLERANCE,
};
use crate::analytic::signed_reference_frame;
use crate::math::{distance, unit_vector};

const EPS_PCURVE_RESIDUAL: f64 = 1.0e-9;
const EPS_PCURVE_PARAMETER: f64 = 1.0e-12;

pub(super) fn sphere_great_circle_geometry(
    pcurve: &B5SphereGreatCirclePcurve,
    surface: &B5Surface,
) -> Option<CurveGeometry> {
    let B5Surface::Sphere {
        center,
        frame,
        direction_y,
        radius,
        construction_radius,
        ..
    } = surface
    else {
        return None;
    };
    let chart_scale = pcurve.chart_scale.get();
    if chart_scale != construction_radius.get() {
        return None;
    }
    let (sphere_axis, direction_x, direction_y) = (
        components(frame.axis()),
        components(frame.reference()),
        components(direction_y),
    );
    let phase = pcurve.chart_shift.get() / chart_scale + pcurve.phase.get();
    let slope = pcurve.slope.get();
    let plane_axis = unit_vector(add(
        scale(sphere_axis, 1.0),
        add(
            scale(direction_x, -slope * phase.cos()),
            scale(direction_y, -slope * phase.sin()),
        ),
    ))?;
    let ref_direction = add(
        scale(direction_x, -phase.sin()),
        scale(direction_y, phase.cos()),
    );
    Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            center.get(),
            vector(plane_axis),
            vector(ref_direction),
            radius.get(),
        )
        .ok()?,
    )))
}

pub(super) fn sphere_great_circle_pcurve(
    pcurve: &B5SphereGreatCirclePcurve,
) -> Option<(PcurveGeometry, [FiniteReal; 2])> {
    let chart_scale = pcurve.chart_scale.get();
    Some((
        PcurveGeometry::SphericalGreatCircle(
            cadmpeg_ir::geometry::pcurve::SphericalGreatCirclePcurve::try_new(
                0.0,
                chart_scale.recip(),
                pcurve.chart_shift.get() / chart_scale + pcurve.phase.get(),
                pcurve.slope.get(),
            )
            .ok()?,
        ),
        pcurve.u_bounds.finite_endpoints(),
    ))
}

pub(super) fn oriented_line_plan(
    geometry: &CurveGeometry,
    edge_start: [f64; 3],
    edge_end: [f64; 3],
) -> Option<CurvePlan> {
    let CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) = geometry else {
        return None;
    };
    let line_origin = line_curve.origin();
    let mut line_direction = line_curve.direction();
    let origin = coordinates(line_origin);
    let direction = components(&line_direction);
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
        line_direction = line_direction.reversed();
        range = [-range[0], -range[1]];
    }
    Some(CurvePlan {
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::new(line_origin, line_direction),
        )),
        parameter_range: Some(range),
        edge_tolerance: if residual > EPS_PCURVE_RESIDUAL {
            Some(cadmpeg_ir::scalar::PositiveReal::new(
                residual + EPS_PCURVE_RESIDUAL,
            )?)
        } else {
            None
        },
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
    let scale = scale.get();
    if scale == 0.0 {
        return None;
    }
    if pcurve
        .weights
        .as_ref()
        .is_some_and(|weights| weights.len() != pcurve.control_points.len())
    {
        return None;
    }
    let endpoints = endpoint_parameters.map(|parameter| evaluate_pcurve(pcurve, parameter));
    let [Some(start_uv), Some(end_uv)] = endpoints else {
        return None;
    };
    let angles = [start_uv[dimension] / scale, end_uv[dimension] / scale];
    let delta = angles[1] - angles[0];
    if !delta.is_finite()
        || delta == 0.0
        || delta.abs() > std::f64::consts::TAU + EPS_PCURVE_RESIDUAL
    {
        return None;
    }
    let direction = delta.signum();
    if pcurve.control_points.windows(2).any(|points| {
        direction * (points[1][dimension] - points[0][dimension]) / scale < -EPS_PCURVE_PARAMETER
    }) {
        return None;
    }

    let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) = geometry else {
        return None;
    };
    let mut circle_curve = *circle_curve;
    let oriented_angles = if delta < 0.0 {
        circle_curve.reverse_parameterization();
        [-angles[0], -angles[1]]
    } else {
        angles
    };
    let parameter_range = crate::nurbs::canonical_periodic_range(oriented_angles)?;
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve));
    let evaluated = parameter_range.map(|parameter| curve_point(&geometry, parameter).ok());
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
        edge_tolerance: if residual > EPS_PCURVE_RESIDUAL {
            Some(cadmpeg_ir::scalar::PositiveReal::new(
                residual + EPS_PCURVE_RESIDUAL,
            )?)
        } else {
            None
        },
        cache_fit_tolerance: None,
    })
}

fn isoparametric_angle_coordinate(
    pcurve: &B5Pcurve,
    surface: &B5Surface,
) -> Option<(usize, FiniteReal)> {
    match surface {
        B5Surface::Cylinder { angular_scale, .. }
            if constant_coordinate(&pcurve.control_points, 1).is_some() =>
        {
            Some((0, *angular_scale))
        }
        B5Surface::Cone { angular_scale, .. }
            if constant_coordinate(&pcurve.control_points, 1).is_some() =>
        {
            Some((0, (*angular_scale).into()))
        }
        B5Surface::Torus { minor_scale, .. }
            if constant_coordinate(&pcurve.control_points, 0).is_some() =>
        {
            Some((1, (*minor_scale).into()))
        }
        B5Surface::Torus { major_scale, .. }
            if constant_coordinate(&pcurve.control_points, 1).is_some() =>
        {
            Some((0, (*major_scale).into()))
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
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(mut curve)) = geometry else {
        return None;
    };
    let degree = usize::try_from(curve.degree()).ok()?;
    let domain_start = *curve.knots().get(degree)?;
    let domain_end = *curve
        .knots()
        .len()
        .checked_sub(degree + 1)
        .and_then(|index| curve.knots().get(index))?;
    let mut range = endpoint_parameters;
    if range[0] > range[1] {
        let sum = domain_start + domain_end;
        curve.reverse_parameterization();
        curve
            .edit_knots(|knots| {
                for knot in knots {
                    *knot += sum;
                }
            })
            .ok()?;
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
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve));
    let start = curve_point(&geometry, range[0]).ok()?;
    let end = curve_point(&geometry, range[1]).ok()?;
    let residual = distance([start.x, start.y, start.z], edge_start)
        .max(distance([end.x, end.y, end.z], edge_end));
    if residual > POINT_TOLERANCE {
        return None;
    }
    Some(CurvePlan {
        geometry,
        parameter_range: Some(range),
        edge_tolerance: if residual > EPS_PCURVE_RESIDUAL {
            Some(cadmpeg_ir::scalar::PositiveReal::new(
                residual + EPS_PCURVE_RESIDUAL,
            )?)
        } else {
            None
        },
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
    if pcurve
        .weights
        .as_ref()
        .is_some_and(|weights| weights.len() != pcurve.control_points.len())
    {
        return None;
    }
    if !pcurve
        .control_points
        .windows(2)
        .all(|pair| pair[0][varying_dimension] <= pair[1][varying_dimension])
        && !pcurve
            .control_points
            .windows(2)
            .all(|pair| pair[0][varying_dimension] >= pair[1][varying_dimension])
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
        B5Surface::Cylinder { angular_scale, .. } => {
            Point2::new(point[0] / angular_scale.get(), point[1])
        }
        B5Surface::Cone {
            frame,
            direction_y,
            half_angle,
            slant_range,
            angular_scale,
            ..
        } => Point2::new(
            dot(
                cross(components(frame.reference()), components(direction_y)),
                components(frame.axis()),
            )
            .signum()
                * point[0]
                / angular_scale.get(),
            (point[1] - slant_range.lower()) * half_angle.get().cos(),
        ),
        B5Surface::Torus {
            major_scale,
            minor_scale,
            ..
        } => Point2::new(point[0] / major_scale.get(), point[1] / minor_scale.get()),
        _ => Point2::new(point[0], point[1]),
    }
}

pub(super) fn lifted_curve_geometry(
    pcurve: &B5Pcurve,
    surface: &B5Surface,
) -> Option<CurveGeometry> {
    let knots = pcurve_nurbs_knots(pcurve)?
        .into_iter()
        .map(FiniteReal::get)
        .collect::<Vec<_>>();
    match surface {
        B5Surface::UnresolvedNurbs { .. }
        | B5Surface::Unknown { .. }
        | B5Surface::RollingBall { .. }
        | B5Surface::Sphere { .. } => None,
        B5Surface::Plane {
            origin,
            frame,
            direction_v,
            ..
        } => {
            let (origin, direction_u) = (coordinates(*origin), components(frame.reference()));
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    pcurve.degree,
                    knots,
                    pcurve
                        .control_points
                        .iter()
                        .map(|uv| {
                            point3(add(
                                origin,
                                add(scale(direction_u, uv[0]), scale(direction_v.get(), uv[1])),
                            ))
                        })
                        .collect(),
                    pcurve
                        .weights
                        .as_ref()
                        .map(|weights| weights.iter().copied().map(PositiveReal::get).collect()),
                    false,
                )
                .ok()?,
            )))
        }
        B5Surface::Cylinder {
            origin,
            frame,
            radius,
            angular_scale,
            ..
        } if constant_coordinate(&pcurve.control_points, 0).is_some() => {
            let first = pcurve.control_points.first()?;
            let line_origin = cylinder_point(
                coordinates(*origin),
                components(frame.reference()),
                components(frame.axis()),
                radius.get(),
                angular_scale.get(),
                first.get(),
            );
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::new(
                    FinitePoint3::new(point3(line_origin))?,
                    *frame.axis(),
                ),
            )))
        }
        B5Surface::Cone {
            apex,
            frame,
            direction_y,
            half_angle,
            angular_scale,
            ..
        } if constant_coordinate(&pcurve.control_points, 0).is_some() => {
            let [u, _] = pcurve.control_points.first()?.get();
            let angle = u / angular_scale.get();
            let radial = add(
                scale(components(frame.reference()), angle.cos()),
                scale(components(direction_y), angle.sin()),
            );
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::new(
                    *apex,
                    UnitVector3::new(vector(add(
                        scale(components(frame.axis()), half_angle.get().cos()),
                        scale(radial, half_angle.get().sin()),
                    )))?,
                ),
            )))
        }
        B5Surface::Torus {
            center,
            frame,
            direction_y,
            major_radius,
            minor_radius,
            major_scale,
            ..
        } if constant_coordinate(&pcurve.control_points, 0).is_some() => {
            let u = pcurve.control_points.first()?[0];
            let angle = u / major_scale.get();
            let radial = add(
                scale(components(frame.reference()), angle.cos()),
                scale(direction_y.get(), angle.sin()),
            );
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                    point3(add(coordinates(*center), scale(radial, major_radius.get()))),
                    vector(cross(radial, components(frame.axis()))),
                    vector(radial),
                    minor_radius.get(),
                )
                .ok()?,
            )))
        }
        B5Surface::Torus {
            center,
            frame,
            major_radius,
            minor_radius,
            minor_scale,
            ..
        } => {
            let v = constant_coordinate(&pcurve.control_points, 1)?;
            let angle = v / minor_scale.get();
            let signed_radius = major_radius.get() + minor_radius.get() * angle.cos();
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::new(
                    FinitePoint3::new(point3(add(
                        coordinates(*center),
                        scale(components(frame.axis()), minor_radius.get() * angle.sin()),
                    )))?,
                    signed_reference_frame(*frame, signed_radius),
                    PositiveLength::new(signed_radius.abs())?,
                ),
            )))
        }
        B5Surface::Cone {
            apex,
            frame,
            half_angle,
            ..
        } => {
            let slant = constant_coordinate(&pcurve.control_points, 1)?;
            let radius = slant * half_angle.get().sin();
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::new(
                    FinitePoint3::new(point3(add(
                        coordinates(*apex),
                        scale(components(frame.axis()), slant * half_angle.get().cos()),
                    )))?,
                    signed_reference_frame(*frame, radius),
                    PositiveLength::new(radius.abs())?,
                ),
            )))
        }
        B5Surface::Cylinder {
            origin,
            frame,
            radius,
            ..
        } => {
            let v = constant_coordinate(&pcurve.control_points, 1)?;
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::new(
                    FinitePoint3::new(point3(add(
                        coordinates(*origin),
                        scale(components(frame.axis()), v),
                    )))?,
                    *frame,
                    *radius,
                ),
            )))
        }
        B5Surface::Nurbs(surface) => nurbs_isocurve(pcurve, surface)
            .map(SolvedCurveGeometry::Nurbs)
            .map(CurveGeometry::Solved),
        B5Surface::Revolution { .. } => None,
    }
}

pub(super) fn nurbs_isocurve(pcurve: &B5Pcurve, surface: &NurbsSurface) -> Option<NurbsCurve> {
    if let Some(u) = constant_coordinate(&pcurve.control_points, 0) {
        cadmpeg_ir::eval::nurbs_surface_isocurve(
            surface,
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
            u,
        )
    } else if let Some(v) = constant_coordinate(&pcurve.control_points, 1) {
        cadmpeg_ir::eval::nurbs_surface_isocurve(
            surface,
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::V,
            v,
        )
    } else {
        None
    }
}

fn constant_coordinate(points: &[FiniteVector<2>], dimension: usize) -> Option<f64> {
    let value = points.first()?[dimension];
    points
        .iter()
        .all(|point| point[dimension] == value)
        .then_some(value)
}

pub(super) fn cylinder_point(
    origin: [f64; 3],
    reference_x: [f64; 3],
    axis: [f64; 3],
    radius: f64,
    angular_scale: f64,
    uv: [f64; 2],
) -> [f64; 3] {
    let reference_y = cross(axis, reference_x);
    let angle = uv[0] / angular_scale;
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
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<HelixPlan> {
    const FIT_TOLERANCE: PositiveReal = match PositiveReal::new(1e-4) {
        Some(tolerance) => tolerance,
        None => panic!("the helix cache fit tolerance must be positive and finite"),
    };

    let B5Surface::Cylinder {
        origin,
        frame,
        radius,
        angular_scale,
        ..
    } = surface
    else {
        return None;
    };
    let (origin, reference_x, axis, radius) = (
        coordinates(*origin),
        components(frame.reference()),
        components(frame.axis()),
        radius.get(),
    );
    if pcurve.degree != 1 || pcurve.control_points.len() != 2 {
        return None;
    }
    let endpoints = endpoint_parameters.map(|parameter| evaluate_pcurve(pcurve, parameter));
    let [Some(first), Some(second)] = endpoints else {
        return None;
    };
    let endpoints = [first, second];
    let lifted = endpoints
        .map(|uv| cylinder_point(origin, reference_x, axis, radius, angular_scale.get(), uv));
    let forward_error = distance(lifted[0], edge_start).max(distance(lifted[1], edge_end));
    if !forward_error.is_finite() || forward_error > POINT_TOLERANCE {
        return None;
    }
    let angles = endpoints.map(|point| point[0] / angular_scale.get());
    let delta_angle = angles[1] - angles[0];
    let delta_height = endpoints[1][1] - endpoints[0][1];
    if delta_angle == 0.0 || delta_height == 0.0 {
        return None;
    }
    let reference_y = cross(axis, reference_x);
    let radial = add(
        scale(reference_x, angles[0].cos()),
        scale(reference_y, angles[0].sin()),
    );
    let tangent = cross(axis, radial);
    let sweep = delta_angle.abs();
    let definition = ProceduralCurveDefinition::Helix(
        cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
            [0.0, sweep],
            cadmpeg_ir::geometry::HelixFrame {
                center: point3(add(origin, scale(axis, endpoints[0][1]))),
                major: vector(scale(radial, radius)),
                minor: vector(scale(tangent, radius * delta_angle.signum())),
                pitch: vector(scale(
                    axis,
                    delta_height / sweep * 2.0 * std::f64::consts::PI,
                )),
                axis: vector(axis),
            },
            0.0,
            None,
        )
        .ok()?,
    );
    let cache = crate::nurbs::circular_helix_cache(
        &definition,
        FIT_TOLERANCE,
        refusal,
        "b5 helix edge construction",
    )?;
    let cache_points = cache.curve.control_points();
    let cache_start = cache_points.first()?;
    let cache_end = cache_points.last()?;
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

/// Emitted pcurve carriers and intervals indexed by native loop and member.
pub(super) type PcurveUses = HashMap<(u32, usize), (PcurveId, [FiniteReal; 2])>;

/// Emit distinct pcurve occurrences grouped by native parameter range,
/// returning each emitted carrier and its forward interval by
/// `(loop_id, member_index)`.
pub(super) fn emit_pcurves(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    graph: &B5Graph,
    plan: &TransferPlan,
) -> Result<PcurveUses, cadmpeg_core::CodecError> {
    let pcurve_plan = &plan.pcurve_plan;
    let mut occurrence_groups =
        BTreeMap::<u32, BTreeMap<[u64; 2], ([FiniteReal; 2], Vec<(u32, usize)>)>>::new();
    for loop_ in graph.loops.values() {
        for (index, member) in loop_.members.iter().enumerate() {
            let object_id = member.pcurve;
            let edge_id = member.edge;
            let Some((_, _, native_range)) = pcurve_plan.get(&object_id) else {
                continue;
            };
            let parameter_range = edge_pcurve_parameters(graph, edge_id, object_id)
                .and_then(|parameters| ordered_subrange(parameters, *native_range))
                .unwrap_or(*native_range)
                .map(|parameter| {
                    if parameter.get() == 0.0 {
                        FiniteReal::ZERO
                    } else {
                        parameter
                    }
                });
            occurrence_groups
                .entry(object_id)
                .or_default()
                .entry(parameter_range.map(|parameter| parameter.get().to_bits()))
                .or_insert_with(|| (parameter_range, Vec::new()))
                .1
                .push((loop_.object_id, index));
        }
    }
    let mut pcurve_uses = HashMap::new();
    for (object_id, ranges) in occurrence_groups {
        let (geometry, cylinder_reparameterized, _) = &pcurve_plan[&object_id];
        let range_count = ranges.len();
        for (rank, (parameter_range, occurrences)) in ranges.into_values().enumerate() {
            let key = cadmpeg_ir::ids::IdentityKey::from(object_id);
            let key = if range_count == 1 {
                key
            } else {
                key.then(cadmpeg_ir::identity_key!("@")).then(rank)
            };
            let id = PcurveId::compose(
                &cadmpeg_ir::identity_namespace!("catia", "b5", "pcurve"),
                key,
            );
            annotate(
                annotations,
                &id,
                "object_stream_b5_03",
                "21_pcurve",
                Exactness::ByteExact,
            );
            if *cylinder_reparameterized {
                annotations
                    .derived(&id, "geometry.control_points")
                    .map_err(cadmpeg_core::CodecError::malformed)?;
            }
            if graph
                .pcurves
                .get(&object_id)
                .and_then(|pcurve| pcurve.parameter_range)
                != Some(parameter_range)
            {
                annotations
                    .derived(&id, "parameter_range")
                    .map_err(cadmpeg_core::CodecError::malformed)?;
            }
            for occurrence in occurrences {
                pcurve_uses.insert(occurrence, (id.clone(), parameter_range));
            }
            ir.model.pcurves.push(Pcurve {
                id,
                geometry: geometry.clone(),
                metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                    None,
                    Some(cadmpeg_ir::units::FiniteVector::from(parameter_range)),
                    None,
                ),
            });
        }
    }
    Ok(pcurve_uses)
}
