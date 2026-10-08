// SPDX-License-Identifier: Apache-2.0
//! Pcurve-layer transfer: surface-chart curve lowering, orientation solving,
//! and the pcurve emit pass.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    pcurve::{Pcurve, PcurveGeometry},
    CurveGeometry, ProceduralCurveDefinition, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{FiniteReal, PositiveLength, PositiveReal};
use cadmpeg_ir::units::{FiniteVector, OrthonormalFrame3, UnitVector3};
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
    let frame =
        OrthonormalFrame3::from_units(plane_axis, UnitVector3::new(vector(ref_direction))?)?;
    Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::new(*center, frame, *radius),
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurve: &B5Pcurve,
    surface: &B5Surface,
    geometry: &CurveGeometry,
    endpoint_parameters: [f64; 2],
    edge_start: [f64; 3],
    edge_end: [f64; 3],
) -> Result<Option<CurvePlan>, cadmpeg_core::CodecError> {
    let Some((dimension, scale)) = isoparametric_angle_coordinate(ctx, pcurve, surface)? else {
        return Ok(None);
    };
    let scale = scale.get();
    if scale == 0.0 {
        return Ok(None);
    }
    if pcurve
        .weights
        .as_ref()
        .is_some_and(|weights| weights.len() != pcurve.control_points.len())
    {
        return Ok(None);
    }
    let Some(start_uv) = evaluate_pcurve(ctx, pcurve, endpoint_parameters[0])? else {
        return Ok(None);
    };
    let Some(end_uv) = evaluate_pcurve(ctx, pcurve, endpoint_parameters[1])? else {
        return Ok(None);
    };
    let angles = [start_uv[dimension] / scale, end_uv[dimension] / scale];
    let delta = angles[1] - angles[0];
    if !delta.is_finite()
        || delta == 0.0
        || delta.abs() > std::f64::consts::TAU + EPS_PCURVE_RESIDUAL
    {
        return Ok(None);
    }
    let direction = delta.signum();
    if ctx.any_by(
        pcurve.control_points.windows(2),
        |points| {
            Ok(
                direction * (points[1][dimension] - points[0][dimension]) / scale
                    < -EPS_PCURVE_PARAMETER,
            )
        },
        "catia_b5_circle_pcurve_direction_scan",
    )? {
        return Ok(None);
    }
    let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) = geometry else {
        return Ok(None);
    };
    let mut circle_curve = *circle_curve;
    let oriented_angles = if delta < 0.0 {
        circle_curve.reverse_parameterization();
        [-angles[0], -angles[1]]
    } else {
        angles
    };
    let Some(parameter_range) = crate::nurbs::canonical_periodic_range(oriented_angles) else {
        return Ok(None);
    };
    edge_fitted_plan(
        ctx,
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)),
        parameter_range,
        edge_start,
        edge_end,
    )
}

/// The plan for `geometry` over `parameter_range` when its ends meet the edge
/// endpoints, with an edge tolerance covering a residual above the pcurve
/// residual.
fn edge_fitted_plan(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: CurveGeometry,
    parameter_range: [f64; 2],
    edge_start: [f64; 3],
    edge_end: [f64; 3],
) -> Result<Option<CurvePlan>, cadmpeg_core::CodecError> {
    let point = |parameter| -> Result<_, cadmpeg_core::CodecError> {
        Ok(cadmpeg_ir::eval::finite_or_refusal(
            cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::curve_point(
                ctx, &geometry, parameter,
            ))?,
        )?)
    };
    let Some(start) = point(parameter_range[0])? else {
        return Ok(None);
    };
    let Some(end) = point(parameter_range[1])? else {
        return Ok(None);
    };
    let residual = distance([start.x, start.y, start.z], edge_start)
        .max(distance([end.x, end.y, end.z], edge_end));
    if residual > POINT_TOLERANCE {
        return Ok(None);
    }
    let edge_tolerance = if residual > EPS_PCURVE_RESIDUAL {
        let Some(tolerance) = cadmpeg_ir::scalar::PositiveReal::new(residual + EPS_PCURVE_RESIDUAL)
        else {
            return Ok(None);
        };
        Some(tolerance)
    } else {
        None
    };
    Ok(Some(CurvePlan {
        geometry,
        parameter_range: Some(parameter_range),
        edge_tolerance,
        cache_fit_tolerance: None,
    }))
}

fn isoparametric_angle_coordinate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurve: &B5Pcurve,
    surface: &B5Surface,
) -> Result<Option<(usize, FiniteReal)>, cadmpeg_core::CodecError> {
    let result = match surface {
        B5Surface::Cylinder { angular_scale, .. } => {
            constant_coordinate(ctx, &pcurve.control_points, 1)?
                .is_some()
                .then_some((0, *angular_scale))
        }
        B5Surface::Cone { angular_scale, .. } => {
            constant_coordinate(ctx, &pcurve.control_points, 1)?
                .is_some()
                .then_some((0, (*angular_scale).into()))
        }
        B5Surface::Torus {
            major_scale,
            minor_scale,
            ..
        } => {
            if constant_coordinate(ctx, &pcurve.control_points, 0)?.is_some() {
                Some((1, (*minor_scale).into()))
            } else if constant_coordinate(ctx, &pcurve.control_points, 1)?.is_some() {
                Some((0, (*major_scale).into()))
            } else {
                None
            }
        }
        _ => None,
    };
    Ok(result)
}

pub(super) fn oriented_nurbs_range(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &CurveGeometry,
    endpoint_parameters: [f64; 2],
    edge_start: [f64; 3],
    edge_end: [f64; 3],
) -> Result<Option<CurvePlan>, cadmpeg_core::CodecError> {
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = geometry else {
        return Ok(None);
    };
    let Ok(degree) = usize::try_from(curve.degree()) else {
        return Ok(None);
    };
    let knots = curve.knots();
    let (Some(&domain_start), Some(&domain_end)) = (
        knots.get(degree),
        knots
            .len()
            .checked_sub(degree + 1)
            .and_then(|index| knots.get(index)),
    ) else {
        return Ok(None);
    };
    let mut curve = curve.try_clone_for_decode(ctx, "catia_b5_oriented_nurbs_curve")?;
    let mut range = endpoint_parameters;
    if range[0] > range[1] {
        let sum = domain_start + domain_end;
        curve.reverse_parameterization(ctx)?;
        // The knot edit charges one step per knot.
        if curve
            .edit_knots(ctx, |knots| {
                for knot in knots {
                    *knot += sum;
                }
            })?
            .is_err()
        {
            return Ok(None);
        }
        range = [sum - range[0], sum - range[1]];
    }
    if !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] >= range[1]
        || range[0] < domain_start
        || range[1] > domain_end
    {
        return Ok(None);
    }
    edge_fitted_plan(
        ctx,
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
        range,
        edge_start,
        edge_end,
    )
}

pub(super) fn isocurve_endpoint_parameters(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurve: &B5Pcurve,
    endpoint_parameters: [f64; 2],
) -> Result<Option<[f64; 2]>, cadmpeg_core::CodecError> {
    let varying_dimension = if constant_coordinate(ctx, &pcurve.control_points, 0)?.is_some() {
        1
    } else if constant_coordinate(ctx, &pcurve.control_points, 1)?.is_some() {
        0
    } else {
        return Ok(None);
    };
    if pcurve
        .weights
        .as_ref()
        .is_some_and(|weights| weights.len() != pcurve.control_points.len())
    {
        return Ok(None);
    }
    if !ctx.all_by(
        pcurve.control_points.windows(2),
        |pair| Ok(pair[0][varying_dimension] <= pair[1][varying_dimension]),
        "catia_b5_isocurve_increasing_parameter_scan",
    )? && !ctx.all_by(
        pcurve.control_points.windows(2),
        |pair| Ok(pair[0][varying_dimension] >= pair[1][varying_dimension]),
        "catia_b5_isocurve_decreasing_parameter_scan",
    )? {
        return Ok(None);
    }
    let Some(start) = evaluate_pcurve(ctx, pcurve, endpoint_parameters[0])? else {
        return Ok(None);
    };
    let Some(end) = evaluate_pcurve(ctx, pcurve, endpoint_parameters[1])? else {
        return Ok(None);
    };
    Ok(Some([start[varying_dimension], end[varying_dimension]]))
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurve: &B5Pcurve,
    surface: &B5Surface,
) -> Result<Option<CurveGeometry>, cadmpeg_core::CodecError> {
    let Some(native_knots) = pcurve_nurbs_knots(ctx, pcurve)? else {
        return Ok(None);
    };
    if let B5Surface::Plane {
        origin,
        frame,
        direction_v,
        ..
    } = surface
    {
        let knots = ctx.collect_vec(
            native_knots.into_iter().map(FiniteReal::get),
            "catia_b5_lifted_plane_knots",
        )?;
        let (origin, direction_u) = (coordinates(*origin), components(frame.reference()));
        let points = ctx.collect_vec(
            pcurve.control_points.iter().map(|uv| {
                point3(add(
                    origin,
                    add(scale(direction_u, uv[0]), scale(direction_v.get(), uv[1])),
                ))
            }),
            "catia_b5_lifted_plane_points",
        )?;
        let weights = pcurve
            .weights
            .as_ref()
            .map(|weights| {
                ctx.collect_vec(
                    weights.iter().copied().map(PositiveReal::get),
                    "catia_b5_lifted_plane_weights",
                )
            })
            .transpose()?;
        return Ok(cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
            ctx,
            pcurve.degree,
            knots,
            points,
            weights,
            false,
        )?
        .ok()
        .map(SolvedCurveGeometry::Nurbs)
        .map(CurveGeometry::Solved));
    }
    if let B5Surface::Nurbs(surface) = surface {
        return Ok(nurbs_isocurve(ctx, pcurve, surface)?
            .map(SolvedCurveGeometry::Nurbs)
            .map(CurveGeometry::Solved));
    }
    let coordinate = match surface {
        B5Surface::Cylinder { .. } | B5Surface::Cone { .. } | B5Surface::Torus { .. } => {
            let u = constant_coordinate(ctx, &pcurve.control_points, 0)?;
            if let Some(u) = u {
                Some((0, u))
            } else {
                constant_coordinate(ctx, &pcurve.control_points, 1)?.map(|v| (1, v))
            }
        }
        _ => None,
    };
    Ok((|| -> Option<CurveGeometry> {
        match surface {
            B5Surface::UnresolvedNurbs { .. }
            | B5Surface::Unknown { .. }
            | B5Surface::RollingBall { .. }
            | B5Surface::Sphere { .. }
            | B5Surface::Nurbs(_) => None,
            B5Surface::Plane { .. } => None,
            B5Surface::Cylinder {
                origin,
                frame,
                radius,
                angular_scale,
                ..
            } if coordinate.is_some_and(|(dimension, _)| dimension == 0) => {
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
            } if coordinate.is_some_and(|(dimension, _)| dimension == 0) => {
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
            } if coordinate.is_some_and(|(dimension, _)| dimension == 0) => {
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
                let (_, v) = coordinate?;
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
                let (_, slant) = coordinate?;
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
                let (_, v) = coordinate?;
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
            B5Surface::Revolution { .. } => None,
        }
    })())
}

pub(super) fn nurbs_isocurve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurve: &B5Pcurve,
    surface: &NurbsSurface,
) -> Result<Option<NurbsCurve>, cadmpeg_core::CodecError> {
    use cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis;
    let fixed = if let Some(u) = constant_coordinate(ctx, &pcurve.control_points, 0)? {
        (SurfaceParameterAxis::U, u)
    } else if let Some(v) = constant_coordinate(ctx, &pcurve.control_points, 1)? {
        (SurfaceParameterAxis::V, v)
    } else {
        return Ok(None);
    };
    cadmpeg_ir::eval::nurbs_surface_isocurve(ctx, surface, fixed.0, fixed.1).map_err(Into::into)
}

fn constant_coordinate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    points: &[FiniteVector<2>],
    dimension: usize,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let Some(value) = points.first().map(|point| point[dimension]) else {
        return Ok(None);
    };
    Ok(ctx
        .all_by(
            points,
            |point| Ok(point[dimension] == value),
            "catia_b5_pcurve_constant_coordinate_scan",
        )?
        .then_some(value))
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurve: &B5Pcurve,
    surface: &B5Surface,
    endpoint_parameters: [f64; 2],
    edge_start: [f64; 3],
    edge_end: [f64; 3],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<HelixPlan>, cadmpeg_core::CodecError> {
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
        return Ok(None);
    };
    let (origin, reference_x, axis, radius) = (
        coordinates(*origin),
        components(frame.reference()),
        components(frame.axis()),
        radius.get(),
    );
    if pcurve.degree != 1 || pcurve.control_points.len() != 2 {
        return Ok(None);
    }
    let Some(first) = evaluate_pcurve(ctx, pcurve, endpoint_parameters[0])? else {
        return Ok(None);
    };
    let Some(second) = evaluate_pcurve(ctx, pcurve, endpoint_parameters[1])? else {
        return Ok(None);
    };
    let Some((definition, sweep)) = (|| -> Option<_> {
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
        Some((definition, sweep))
    })() else {
        return Ok(None);
    };
    let Some(cache) = crate::nurbs::circular_helix_cache(
        ctx,
        &definition,
        FIT_TOLERANCE,
        refusal,
        "b5 helix edge construction",
    )?
    else {
        return Ok(None);
    };
    let poles = cache.curve.pole_rows();
    let Some(cache_start) = poles.point_at(0) else {
        return Ok(None);
    };
    let Some(last) = poles.count().checked_sub(1) else {
        return Ok(None);
    };
    let Some(cache_end) = poles.point_at(last) else {
        return Ok(None);
    };
    if distance([cache_start.x, cache_start.y, cache_start.z], edge_start) > POINT_TOLERANCE
        || distance([cache_end.x, cache_end.y, cache_end.z], edge_end) > POINT_TOLERANCE
    {
        return Ok(None);
    }
    Ok(Some(HelixPlan {
        definition,
        cache: cache.curve,
        parameter_range: [0.0, sweep],
        fit_tolerance: cache.fit_tolerance,
    }))
}

/// Emitted pcurve carriers and intervals indexed by native loop and member.
pub(super) type PcurveUses = HashMap<(u32, usize), (PcurveId, [FiniteReal; 2])>;

/// Emit distinct pcurve occurrences grouped by native parameter range,
/// returning each emitted carrier and its forward interval by
/// `(loop_id, member_index)`.
pub(super) fn emit_pcurves(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    graph: &B5Graph,
    plan: &TransferPlan,
    admission: &mut crate::families::FamilyEntityAdmission<'_, '_>,
) -> Result<PcurveUses, cadmpeg_core::CodecError> {
    const LOOKUP: &str = "catia_b5_emit_pcurve_plan_lookup";
    let pcurve_plan = &plan.pcurve_plan;
    let mut occurrence_groups =
        BTreeMap::<u32, BTreeMap<[u64; 2], ([FiniteReal; 2], Vec<(u32, usize)>)>>::new();
    for loop_ in admission
        .context()
        .admit_iter(&graph.loops, "catia_b5_pcurve_occurrence_loop_scan")?
        .map(|(_, loop_)| loop_)
    {
        for (index, member) in admission
            .context()
            .admit_iter(&loop_.members, "catia_b5_pcurve_occurrence_member_scan")?
            .enumerate()
        {
            let object_id = member.pcurve;
            let edge_id = member.edge;
            let Some((_, _, native_range)) =
                admission
                    .context()
                    .get_btree_map(pcurve_plan, &object_id, LOOKUP)?
            else {
                continue;
            };
            let parameter_range =
                edge_pcurve_parameters(admission.context(), graph, edge_id, object_id)?
                    .and_then(|parameters| ordered_subrange(parameters, *native_range))
                    .unwrap_or(*native_range)
                    .map(|parameter| {
                        if parameter.get() == 0.0 {
                            FiniteReal::ZERO
                        } else {
                            parameter
                        }
                    });
            let ranges = admission
                .context()
                .entry_btree_map(
                    &mut occurrence_groups,
                    object_id,
                    "catia_b5_pcurve_occurrence_objects",
                )?
                .or_default();
            let key = parameter_range.map(|parameter| parameter.get().to_bits());
            let occurrences = &mut admission
                .context()
                .entry_btree_map(ranges, key, "catia_b5_pcurve_occurrence_ranges")?
                .or_insert_with(|| (parameter_range, Vec::new()))
                .1;
            admission.context().push_vec(
                occurrences,
                (loop_.object_id, index),
                "catia_b5_pcurve_occurrences",
            )?;
        }
    }
    let mut pcurve_uses = HashMap::new();
    for (object_id, ranges) in admission
        .context()
        .admit_iter(&occurrence_groups, "catia_b5_pcurve_emission_object_scan")?
        .map(|(object_id, ranges)| (*object_id, ranges))
    {
        let (geometry, cylinder_reparameterized, _) = admission
            .context()
            .get_btree_map(pcurve_plan, &object_id, LOOKUP)?
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("grouped B5 pcurve without plan"))?;
        let range_count = ranges.len();
        for (rank, (parameter_range, occurrences)) in admission
            .context()
            .admit_iter(ranges, "catia_b5_pcurve_emission_range_scan")?
            .map(|(_, (range, occurrences))| (*range, occurrences))
            .enumerate()
        {
            let id = if range_count == 1 {
                admission.context().format_retained(
                    format_args!("catia:b5:pcurve#{object_id}"),
                    "catia_b5_emitted_pcurve_id",
                )?
            } else {
                admission.context().format_retained(
                    format_args!("catia:b5:pcurve#{object_id}@{rank}"),
                    "catia_b5_emitted_pcurve_id",
                )?
            };
            let id = PcurveId::mint(id).map_err(cadmpeg_core::CodecError::malformed)?;
            annotate(
                admission.context(),
                annotations,
                &id,
                "object_stream_b5_03",
                "21_pcurve",
                Exactness::ByteExact,
            )?;
            if *cylinder_reparameterized {
                crate::resource::derived_annotation(
                    admission.context(),
                    annotations,
                    id.as_str(),
                    "geometry.control_points",
                )?;
            }
            if admission
                .context()
                .get_btree_map(&graph.pcurves, &object_id, LOOKUP)?
                .and_then(|pcurve| pcurve.parameter_range)
                != Some(parameter_range)
            {
                crate::resource::derived_annotation(
                    admission.context(),
                    annotations,
                    id.as_str(),
                    "parameter_range",
                )?;
            }
            for &occurrence in admission
                .context()
                .admit_iter(occurrences, "catia_b5_pcurve_emission_occurrence_scan")?
            {
                let use_id =
                    id.try_clone_for_decode(admission.context(), "catia_b5_pcurve_use_id")?;
                admission.context().insert_hash_map(
                    &mut pcurve_uses,
                    occurrence,
                    (use_id, parameter_range),
                    "catia_b5_pcurve_uses",
                )?;
            }
            let geometry = geometry
                .try_clone_for_decode(admission.context(), "catia_b5_emitted_pcurve_geometry")?;
            admission.reserve_entity(&mut ir.model.pcurves, "catia_b5_emit_pcurves")?;
            ir.model.pcurves.push(Pcurve {
                id,
                geometry,
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
