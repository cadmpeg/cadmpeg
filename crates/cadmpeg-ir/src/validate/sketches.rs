// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for sketches.
#![allow(clippy::wildcard_imports)]

use super::*;
use crate::geometry::knots_nondecreasing;
use crate::sketches::{
    SketchConstraintDefinition as Constraint, SketchDistancePair, SketchGeometry, SketchLocus,
    SpatialSketchConstraintDefinition as SpatialConstraint, SpatialSketchGeometry,
};
use std::collections::{HashMap, HashSet};

const EPS_SKETCH_VALIDATION_GEOMETRY: f64 = 1.0e-9;
const EPS_SKETCH_VALIDATION_EXACT_GEOMETRY: f64 = 1.0e-12;

const EPS_EQUAL_DISTANCE: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_COORDINATE_VALUE: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_DISTANCE_VALUE: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_POLAR_ANGLE: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_POLAR_ZERO: f64 = EPS_SKETCH_VALIDATION_EXACT_GEOMETRY;
const SPATIAL_LINE_DEGENERACY_EPSILON: f64 = EPS_SKETCH_VALIDATION_EXACT_GEOMETRY;
const EPS_SKETCHES_VALID_SPATIAL_CIRCLE_FRAME_E9: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E9: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E12: f64 = EPS_SKETCH_VALIDATION_EXACT_GEOMETRY;
const EPS_SKETCHES_SPATIAL_PARALLEL_LINE_DISTANCE_E12: f64 = EPS_SKETCH_VALIDATION_EXACT_GEOMETRY;
const EPS_SKETCHES_SPATIAL_PARALLEL_LINE_DISTANCE_E9: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_SKETCHES_SPATIAL_LENGTH_PARAMETER_MATCHES_E9: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_SKETCHES_CHECK_SKETCHES_E9: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_SKETCHES_CHECK_SKETCHES_E12: f64 = EPS_SKETCH_VALIDATION_EXACT_GEOMETRY;
const EPS_SKETCHES_PLANAR_PARALLEL_LINE_DISTANCE_E12: f64 = EPS_SKETCH_VALIDATION_EXACT_GEOMETRY;
const EPS_SKETCHES_PLANAR_PARALLEL_LINE_DISTANCE_E9: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;

fn finding(findings: &mut Vec<Finding>, check: Check, id: &str, message: &str) {
    findings.push(Finding {
        check,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(id.into()),
    });
}

fn finite2(point: crate::math::Point2) -> bool {
    point.u.is_finite() && point.v.is_finite()
}

fn finite3(point: crate::math::Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn valid_spatial_circle_frame(
    normal: crate::math::Vector3,
    reference: crate::math::Vector3,
) -> bool {
    let normal_length = normal.norm();
    let reference_length = reference.norm();
    normal_length.is_finite()
        && reference_length.is_finite()
        && (normal_length - 1.0).abs() <= EPS_SKETCHES_VALID_SPATIAL_CIRCLE_FRAME_E9
        && (reference_length - 1.0).abs() <= EPS_SKETCHES_VALID_SPATIAL_CIRCLE_FRAME_E9
        && (normal.x * reference.x + normal.y * reference.y + normal.z * reference.z).abs()
            <= EPS_SKETCHES_VALID_SPATIAL_CIRCLE_FRAME_E9
}

fn spatial_oriented_endpoints(
    geometry: &SpatialSketchGeometry,
    reversed: bool,
) -> Option<(crate::math::Point3, crate::math::Point3)> {
    let endpoints = match geometry {
        SpatialSketchGeometry::Line { start, end } => (*start, *end),
        SpatialSketchGeometry::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        } => {
            let transverse = crate::math::Vector3::new(
                normal.y * reference_direction.z - normal.z * reference_direction.y,
                normal.z * reference_direction.x - normal.x * reference_direction.z,
                normal.x * reference_direction.y - normal.y * reference_direction.x,
            );
            let at = |angle: f64| {
                crate::math::Point3::new(
                    center.x
                        + radius.0
                            * (reference_direction.x * angle.cos() + transverse.x * angle.sin()),
                    center.y
                        + radius.0
                            * (reference_direction.y * angle.cos() + transverse.y * angle.sin()),
                    center.z
                        + radius.0
                            * (reference_direction.z * angle.cos() + transverse.z * angle.sin()),
                )
            };
            (at(start_angle.0), at(end_angle.0))
        }
        SpatialSketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic: false,
        } => {
            let degree_index = usize::try_from(*degree).ok()?;
            let start = *knots.get(degree_index)?;
            let end = *knots.get(knots.len().checked_sub(degree_index + 1)?)?;
            (
                crate::eval::nurbs_curve_point(
                    *degree,
                    knots,
                    control_points,
                    weights.as_deref(),
                    start,
                )?,
                crate::eval::nurbs_curve_point(
                    *degree,
                    knots,
                    control_points,
                    weights.as_deref(),
                    end,
                )?,
            )
        }
        _ => return None,
    };
    Some(if reversed {
        (endpoints.1, endpoints.0)
    } else {
        endpoints
    })
}

const EPS_FULL_CIRCLE_OFFSET: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_OFFSET_SWEEP: f64 = EPS_SKETCH_VALIDATION_EXACT_GEOMETRY;

fn sketch_curve_offset_matches(
    source: &SketchGeometry,
    result: &SketchGeometry,
    expected: f64,
    linear_tolerance: f64,
) -> bool {
    if let (
        SketchGeometry::Circle {
            center: source_center,
            radius: source_radius,
        },
        SketchGeometry::Circle {
            center: result_center,
            radius: result_radius,
        },
    ) = (source, result)
    {
        let scale = 1.0
            + source_center
                .u
                .abs()
                .max(source_center.v.abs())
                .max(result_center.u.abs())
                .max(result_center.v.abs())
                .max(source_radius.0.abs())
                .max(result_radius.0.abs())
                .max(expected.abs());
        return expected.is_finite()
            && source_radius.0.is_finite()
            && result_radius.0.is_finite()
            && source_radius.0 > 0.0
            && result_radius.0 > 0.0
            && (source_center.u - result_center.u).abs() <= EPS_FULL_CIRCLE_OFFSET * scale
            && (source_center.v - result_center.v).abs() <= EPS_FULL_CIRCLE_OFFSET * scale
            && (source_radius.0 - result_radius.0 - expected).abs()
                <= EPS_FULL_CIRCLE_OFFSET * scale;
    }

    if let (
        SketchGeometry::Circle {
            center: source_center,
            radius: source_radius,
        },
        SketchGeometry::Arc {
            center: result_center,
            radius: result_radius,
            start_angle: result_start,
            end_angle: result_end,
        },
    ) = (source, result)
    {
        let scale = 1.0
            + source_center
                .u
                .abs()
                .max(source_center.v.abs())
                .max(result_center.u.abs())
                .max(result_center.v.abs())
                .max(source_radius.0.abs())
                .max(result_radius.0.abs())
                .max(expected.abs());
        let result_sweep = result_end.0 - result_start.0;
        return expected.is_finite()
            && source_radius.0.is_finite()
            && result_radius.0.is_finite()
            && source_radius.0 > 0.0
            && result_radius.0 > 0.0
            && result_sweep.abs() > EPS_OFFSET_SWEEP
            && (source_center.u - result_center.u).abs() <= EPS_FULL_CIRCLE_OFFSET * scale
            && (source_center.v - result_center.v).abs() <= EPS_FULL_CIRCLE_OFFSET * scale
            && (source_radius.0 - result_radius.0 - expected).abs()
                <= EPS_FULL_CIRCLE_OFFSET * scale;
    }

    if let (
        SketchGeometry::Arc {
            center: source_center,
            radius: source_radius,
            start_angle: source_start,
            end_angle: source_end,
        },
        SketchGeometry::Circle {
            center: result_center,
            radius: result_radius,
        },
    ) = (source, result)
    {
        let scale = 1.0
            + source_center
                .u
                .abs()
                .max(source_center.v.abs())
                .max(result_center.u.abs())
                .max(result_center.v.abs())
                .max(source_radius.0.abs())
                .max(result_radius.0.abs())
                .max(expected.abs());
        let source_sweep = source_end.0 - source_start.0;
        return expected.is_finite()
            && source_radius.0.is_finite()
            && result_radius.0.is_finite()
            && source_radius.0 > 0.0
            && result_radius.0 > 0.0
            && source_sweep.abs() > EPS_OFFSET_SWEEP
            && (source_center.u - result_center.u).abs() <= EPS_FULL_CIRCLE_OFFSET * scale
            && (source_center.v - result_center.v).abs() <= EPS_FULL_CIRCLE_OFFSET * scale
            && (source_sweep.signum() * (source_radius.0 - result_radius.0) - expected).abs()
                <= EPS_FULL_CIRCLE_OFFSET * scale;
    }

    if let (
        SketchGeometry::Arc {
            center: source_center,
            radius: source_radius,
            start_angle: source_start,
            end_angle: source_end,
        },
        SketchGeometry::Arc {
            center: result_center,
            radius: result_radius,
            start_angle: result_start,
            end_angle: result_end,
        },
    ) = (source, result)
    {
        let scale = 1.0
            + source_center
                .u
                .abs()
                .max(source_center.v.abs())
                .max(result_center.u.abs())
                .max(result_center.v.abs())
                .max(source_radius.0)
                .max(result_radius.0)
                .max(expected.abs());
        let source_sweep = source_end.0 - source_start.0;
        let result_sweep = result_end.0 - result_start.0;
        let angle_in_sweep = |angle: f64, start: f64, end: f64| {
            let sweep = end - start;
            if sweep.abs() >= std::f64::consts::TAU - EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E9 {
                return true;
            }
            if sweep.is_sign_positive() {
                (angle - start).rem_euclid(std::f64::consts::TAU)
                    <= sweep + EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E9
            } else {
                (start - angle).rem_euclid(std::f64::consts::TAU)
                    <= -sweep + EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E9
            }
        };
        let angular_overlap = [source_start.0, source_end.0]
            .into_iter()
            .any(|angle| angle_in_sweep(angle, result_start.0, result_end.0))
            || [result_start.0, result_end.0]
                .into_iter()
                .any(|angle| angle_in_sweep(angle, source_start.0, source_end.0));
        return source_radius.0 > 0.0
            && result_radius.0 > 0.0
            && source_sweep.abs() > EPS_OFFSET_SWEEP
            && result_sweep.abs() > EPS_OFFSET_SWEEP
            && source_sweep.signum() == result_sweep.signum()
            && angular_overlap
            && (source_center.u - result_center.u).abs() <= EPS_SKETCH_VALIDATION_GEOMETRY * scale
            && (source_center.v - result_center.v).abs() <= EPS_SKETCH_VALIDATION_GEOMETRY * scale
            && (source_sweep.signum() * (source_radius.0 - result_radius.0) - expected).abs()
                <= EPS_SKETCH_VALIDATION_GEOMETRY * scale;
    }

    if let Some(distance) =
        crate::eval::fitted_nurbs_offset_frame_distance(source, result, linear_tolerance)
    {
        let scale = 1.0 + distance.abs().max(expected.abs());
        return expected.is_finite()
            && (distance - expected).abs()
                <= linear_tolerance.max(EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E9 * scale);
    }

    let (
        SketchGeometry::Line {
            start: source_start,
            end: source_end,
        },
        SketchGeometry::Line {
            start: result_start,
            end: result_end,
        },
    ) = (source, result)
    else {
        return false;
    };
    let source_du = source_end.u - source_start.u;
    let source_dv = source_end.v - source_start.v;
    let result_du = result_end.u - result_start.u;
    let result_dv = result_end.v - result_start.v;
    let source_length = source_du.hypot(source_dv);
    let result_length = result_du.hypot(result_dv);
    if source_length <= EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E12
        || result_length <= EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E12
    {
        return false;
    }
    let scale = 1.0 + expected.abs();
    let parallel = (source_du * result_dv - source_dv * result_du).abs()
        <= EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E9 * source_length * result_length;
    let normal_u = -source_dv / source_length;
    let normal_v = source_du / source_length;
    let distance_at = |point: &crate::math::Point2| {
        (point.u - source_start.u) * normal_u + (point.v - source_start.v) * normal_v
    };
    parallel
        && (distance_at(result_start) - expected).abs()
            <= EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E9 * scale
        && (distance_at(result_end) - expected).abs()
            <= EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E9 * scale
}

fn spatial_parallel_line_distance(
    first: &SpatialSketchGeometry,
    second: &SpatialSketchGeometry,
) -> Option<f64> {
    let (
        SpatialSketchGeometry::Line {
            start: first_start,
            end: first_end,
        },
        SpatialSketchGeometry::Line {
            start: second_start,
            end: second_end,
        },
    ) = (first, second)
    else {
        return None;
    };
    let first_direction = crate::math::Vector3::new(
        first_end.x - first_start.x,
        first_end.y - first_start.y,
        first_end.z - first_start.z,
    );
    let second_direction = crate::math::Vector3::new(
        second_end.x - second_start.x,
        second_end.y - second_start.y,
        second_end.z - second_start.z,
    );
    let first_length = first_direction.norm();
    let second_length = second_direction.norm();
    let cross = crate::math::Vector3::new(
        first_direction.y * second_direction.z - first_direction.z * second_direction.y,
        first_direction.z * second_direction.x - first_direction.x * second_direction.z,
        first_direction.x * second_direction.y - first_direction.y * second_direction.x,
    );
    if first_length <= EPS_SKETCHES_SPATIAL_PARALLEL_LINE_DISTANCE_E12
        || second_length <= EPS_SKETCHES_SPATIAL_PARALLEL_LINE_DISTANCE_E12
        || cross.norm()
            > EPS_SKETCHES_SPATIAL_PARALLEL_LINE_DISTANCE_E9 * first_length * second_length
    {
        return None;
    }
    let offset = crate::math::Vector3::new(
        second_start.x - first_start.x,
        second_start.y - first_start.y,
        second_start.z - first_start.z,
    );
    Some(
        crate::math::Vector3::new(
            offset.y * first_direction.z - offset.z * first_direction.y,
            offset.z * first_direction.x - offset.x * first_direction.z,
            offset.x * first_direction.y - offset.y * first_direction.x,
        )
        .norm()
            / first_length,
    )
}

fn spatial_line_length(geometry: &SpatialSketchGeometry) -> Option<f64> {
    let SpatialSketchGeometry::Line { start, end } = geometry else {
        return None;
    };
    Some((end.x - start.x).hypot((end.y - start.y).hypot(end.z - start.z)))
}

fn spatial_point_line_distance(
    point: &SpatialSketchGeometry,
    line: &SpatialSketchGeometry,
) -> Option<f64> {
    let (SpatialSketchGeometry::Point { position }, SpatialSketchGeometry::Line { start, end }) =
        (point, line)
    else {
        return None;
    };
    let direction = crate::math::Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
    let length = direction.norm();
    if !length.is_finite() || length <= SPATIAL_LINE_DEGENERACY_EPSILON {
        return None;
    }
    let offset = crate::math::Vector3::new(
        position.x - start.x,
        position.y - start.y,
        position.z - start.z,
    );
    Some(
        crate::math::Vector3::new(
            offset.y * direction.z - offset.z * direction.y,
            offset.z * direction.x - offset.x * direction.z,
            offset.x * direction.y - offset.y * direction.x,
        )
        .norm()
            / length,
    )
}

fn spatial_parallel_line_span_distance(
    first: &SpatialSketchGeometry,
    second: &SpatialSketchGeometry,
    linear_tolerance: f64,
) -> Option<f64> {
    let distance = spatial_parallel_line_distance(first, second)?;
    let (
        SpatialSketchGeometry::Line {
            start: first_start,
            end: first_end,
        },
        SpatialSketchGeometry::Line {
            start: second_start,
            end: second_end,
        },
    ) = (first, second)
    else {
        unreachable!("parallel line distance requires line geometry")
    };
    let direction = crate::math::Vector3::new(
        first_end.x - first_start.x,
        first_end.y - first_start.y,
        first_end.z - first_start.z,
    );
    let length = direction.norm();
    let project = |point: crate::math::Point3| {
        (point.x * direction.x + point.y * direction.y + point.z * direction.z) / length
    };
    let first_interval = [project(*first_start), project(*first_end)];
    let second_interval = [project(*second_start), project(*second_end)];
    let first_min = first_interval[0].min(first_interval[1]);
    let first_max = first_interval[0].max(first_interval[1]);
    let second_min = second_interval[0].min(second_interval[1]);
    let second_max = second_interval[0].max(second_interval[1]);
    (first_min.max(second_min) <= first_max.min(second_max) + linear_tolerance).then_some(distance)
}

fn spatial_length_parameter_matches(
    measured: Option<f64>,
    parameter: &crate::features::ParameterId,
    parameter_values: &HashMap<
        &crate::features::ParameterId,
        &Option<crate::features::ParameterValue>,
    >,
) -> bool {
    let expected = match parameter_values.get(parameter) {
        Some(Some(crate::features::ParameterValue::Length(length))) => length.0.abs(),
        _ => return false,
    };
    measured.is_some_and(|measured| {
        let scale = 1.0 + measured.abs().max(expected.abs());
        (measured - expected).abs() <= EPS_SKETCHES_SPATIAL_LENGTH_PARAMETER_MATCHES_E9 * scale
    })
}

pub(super) fn check_sketches(ir: &CadIr, findings: &mut Vec<Finding>) {
    let entity_geometry = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| (&entity.id, &entity.geometry))
        .collect::<HashMap<_, _>>();
    for sketch in &ir.model.sketches {
        let Some((origin, normal_axis, u_axis)) = sketch.resolved_placement() else {
            continue;
        };
        let normal = normal_axis.norm();
        let u_norm = u_axis.norm();
        let dot = normal_axis.x * u_axis.x + normal_axis.y * u_axis.y + normal_axis.z * u_axis.z;
        if !normal.is_finite() || normal <= 0.0 || !u_norm.is_finite() || u_norm <= 0.0 {
            finding(
                findings,
                Check::Bounds,
                &sketch.id.0,
                "sketch plane has a degenerate axis",
            );
        } else if dot.abs() > EPS_SKETCHES_CHECK_SKETCHES_E9 * normal * u_norm {
            finding(
                findings,
                Check::GeometricConsistency,
                &sketch.id.0,
                "sketch plane axes are not perpendicular",
            );
        }
        if !origin.x.is_finite() || !origin.y.is_finite() || !origin.z.is_finite() {
            finding(
                findings,
                Check::Bounds,
                &sketch.id.0,
                "sketch origin is not finite",
            );
        }
        if sketch.profiles.iter().any(Vec::is_empty) {
            finding(
                findings,
                Check::Counts,
                &sketch.id.0,
                "sketch contains an empty profile",
            );
        }
        for profile in &sketch.profiles {
            for adjacent in profile.windows(2) {
                let Some(left) = entity_geometry
                    .get(&adjacent[0].entity)
                    .and_then(|geometry| oriented_endpoints(geometry, adjacent[0].reversed))
                else {
                    continue;
                };
                let Some(right) = entity_geometry
                    .get(&adjacent[1].entity)
                    .and_then(|geometry| oriented_endpoints(geometry, adjacent[1].reversed))
                else {
                    continue;
                };
                if distance2(left.1, right.0) > ir.tolerances.linear {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &sketch.id.0,
                        "sketch profile has disconnected consecutive entities",
                    );
                }
            }
        }
    }

    for entity in &ir.model.sketch_entities {
        let id = &entity.id.0;
        match &entity.geometry {
            SketchGeometry::Point { position } => {
                if !finite2(*position) {
                    finding(findings, Check::Bounds, id, "sketch point is not finite");
                }
            }
            SketchGeometry::Line { start, end } => {
                if !finite2(*start) || !finite2(*end) {
                    finding(findings, Check::Bounds, id, "sketch line is not finite");
                }
            }
            SketchGeometry::ReferenceLine { origin, direction } => {
                if !finite2(*origin)
                    || !finite2(*direction)
                    || direction.u.hypot(direction.v) <= f64::EPSILON
                {
                    finding(findings, Check::Bounds, id, "invalid sketch reference line");
                }
            }
            SketchGeometry::Circle { center, radius }
            | SketchGeometry::Arc { center, radius, .. } => {
                if !finite2(*center) || nonpositive(radius.0) {
                    finding(
                        findings,
                        Check::Bounds,
                        id,
                        "invalid circular sketch geometry",
                    );
                }
                if let SketchGeometry::Arc {
                    start_angle,
                    end_angle,
                    ..
                } = &entity.geometry
                {
                    if !start_angle.0.is_finite() || !end_angle.0.is_finite() {
                        finding(
                            findings,
                            Check::ParameterDomain,
                            id,
                            "arc angle is not finite",
                        );
                    }
                }
            }
            SketchGeometry::Ellipse {
                center,
                major_angle,
                major_radius,
                minor_radius,
                bounds,
            } => {
                if !finite2(*center)
                    || !major_angle.0.is_finite()
                    || nonpositive(major_radius.0)
                    || nonpositive(minor_radius.0)
                    || major_radius.0 < minor_radius.0
                {
                    finding(findings, Check::Bounds, id, "invalid sketch ellipse");
                }
                if bounds.iter().flatten().any(|angle| !angle.0.is_finite()) {
                    finding(
                        findings,
                        Check::ParameterDomain,
                        id,
                        "invalid elliptical arc parameters",
                    );
                }
            }
            SketchGeometry::Hyperbola {
                center,
                major_angle,
                major_radius,
                minor_radius,
                bounds,
            } => {
                if !finite2(*center)
                    || !major_angle.0.is_finite()
                    || nonpositive(major_radius.0)
                    || nonpositive(minor_radius.0)
                {
                    finding(findings, Check::Bounds, id, "invalid sketch hyperbola");
                }
                if bounds.iter().flatten().any(|value| !value.is_finite()) {
                    finding(
                        findings,
                        Check::ParameterDomain,
                        id,
                        "invalid hyperbolic arc parameters",
                    );
                }
            }
            SketchGeometry::Parabola {
                vertex,
                axis_angle,
                focal_length,
                bounds,
            } => {
                if !finite2(*vertex) || !axis_angle.0.is_finite() || nonpositive(focal_length.0) {
                    finding(findings, Check::Bounds, id, "invalid sketch parabola");
                }
                if bounds.iter().flatten().any(|value| !value.is_finite()) {
                    finding(
                        findings,
                        Check::ParameterDomain,
                        id,
                        "invalid parabolic arc parameters",
                    );
                }
            }
            SketchGeometry::Nurbs {
                degree,
                knots,
                control_points,
                weights,
                ..
            } => {
                let expected = control_points.len().checked_add(*degree as usize + 1);
                if *degree == 0
                    || control_points.len() <= *degree as usize
                    || expected != Some(knots.len())
                    || knots.iter().any(|value| !value.is_finite())
                    || !knots_nondecreasing(knots)
                    || control_points.iter().any(|point| !finite2(*point))
                    || weights.as_ref().is_some_and(|weights| {
                        weights.len() != control_points.len()
                            || weights.iter().any(|weight| nonpositive(*weight))
                    })
                {
                    finding(findings, Check::ParameterDomain, id, "invalid sketch NURBS");
                }
            }
            SketchGeometry::Text {
                text,
                font_family,
                font_weight,
                height,
                width_factor,
                anchor,
                rotation,
                ..
            } => {
                if text.is_empty()
                    || font_family.is_empty()
                    || !matches!(font_weight, 400 | 500 | 750)
                    || nonpositive(height.0)
                    || width_factor.is_some_and(nonpositive)
                    || anchor.is_some_and(|anchor| !finite2(anchor))
                    || rotation.is_some_and(|rotation| !rotation.0.is_finite())
                {
                    finding(findings, Check::Bounds, id, "invalid sketch text");
                }
            }
            SketchGeometry::ExternalReference { object, .. } => {
                if object.is_empty() {
                    finding(
                        findings,
                        Check::ReferentialIntegrity,
                        id,
                        "empty external sketch reference",
                    );
                }
            }
            SketchGeometry::Native { native_kind } => {
                if native_kind.is_empty() {
                    finding(findings, Check::Counts, id, "empty native sketch kind");
                }
            }
        }
    }

    let spatial_sketches = ir
        .model
        .spatial_sketches
        .iter()
        .map(|sketch| &sketch.id)
        .collect::<HashSet<_>>();
    let spatial_geometry = ir
        .model
        .spatial_sketch_entities
        .iter()
        .map(|entity| (&entity.id, (&entity.sketch, &entity.geometry)))
        .collect::<HashMap<_, _>>();
    for sketch in &ir.model.spatial_sketches {
        for profile in &sketch.profiles {
            let normal_length = profile.normal.norm();
            let u_length = profile.u_axis.norm();
            let dot = profile.normal.x * profile.u_axis.x
                + profile.normal.y * profile.u_axis.y
                + profile.normal.z * profile.u_axis.z;
            if !finite3(profile.origin)
                || (normal_length - 1.0).abs() > EPS_SKETCHES_CHECK_SKETCHES_E9
                || (u_length - 1.0).abs() > EPS_SKETCHES_CHECK_SKETCHES_E9
                || dot.abs() > EPS_SKETCHES_CHECK_SKETCHES_E9
            {
                finding(
                    findings,
                    Check::GeometricConsistency,
                    &sketch.id.0,
                    "invalid spatial sketch profile plane",
                );
            }
            let unique = profile
                .boundary
                .iter()
                .map(|use_| &use_.entity)
                .collect::<HashSet<_>>();
            if profile.boundary.is_empty() || unique.len() != profile.boundary.len() {
                finding(
                    findings,
                    Check::Counts,
                    &sketch.id.0,
                    "spatial sketch profile boundary is empty or repeats an entity",
                );
            }
            for use_ in &profile.boundary {
                if spatial_geometry.get(&use_.entity).map(|(owner, _)| *owner) != Some(&sketch.id) {
                    finding(
                        findings,
                        Check::ReferentialIntegrity,
                        &sketch.id.0,
                        "spatial sketch profile entity does not belong to its sketch",
                    );
                }
            }
            if profile.boundary.len() == 1 {
                if !matches!(
                    spatial_geometry.get(&profile.boundary[0].entity),
                    Some((_, SpatialSketchGeometry::Circle { .. }))
                ) {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &sketch.id.0,
                        "single-entity spatial sketch profile is not a full circle",
                    );
                }
            } else {
                for index in 0..profile.boundary.len() {
                    let left = &profile.boundary[index];
                    let right = &profile.boundary[(index + 1) % profile.boundary.len()];
                    let endpoints = spatial_geometry
                        .get(&left.entity)
                        .and_then(|(_, geometry)| {
                            spatial_oriented_endpoints(geometry, left.reversed)
                        })
                        .zip(
                            spatial_geometry
                                .get(&right.entity)
                                .and_then(|(_, geometry)| {
                                    spatial_oriented_endpoints(geometry, right.reversed)
                                }),
                        );
                    if endpoints.is_some_and(|(left, right)| {
                        (left.1.x - right.0.x)
                            .hypot(left.1.y - right.0.y)
                            .hypot(left.1.z - right.0.z)
                            > ir.tolerances.linear
                    }) {
                        finding(
                            findings,
                            Check::GeometricConsistency,
                            &sketch.id.0,
                            "spatial sketch profile has disconnected consecutive entities",
                        );
                    }
                }
            }
        }
    }
    for entity in &ir.model.spatial_sketch_entities {
        let id = &entity.id.0;
        if !spatial_sketches.contains(&entity.sketch) {
            finding(
                findings,
                Check::ReferentialIntegrity,
                id,
                "spatial sketch entity references a missing spatial sketch",
            );
        }
        match &entity.geometry {
            SpatialSketchGeometry::Point { position } => {
                if !finite3(*position) {
                    finding(
                        findings,
                        Check::Bounds,
                        id,
                        "non-finite spatial sketch point",
                    );
                }
            }
            SpatialSketchGeometry::Line { start, end } => {
                let distance = (end.x - start.x)
                    .hypot(end.y - start.y)
                    .hypot(end.z - start.z);
                if !finite3(*start) || !finite3(*end) || distance <= EPS_SKETCHES_CHECK_SKETCHES_E12
                {
                    finding(findings, Check::Bounds, id, "invalid spatial sketch line");
                }
            }
            SpatialSketchGeometry::Circle {
                center,
                normal,
                reference_direction,
                radius,
            }
            | SpatialSketchGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                ..
            } => {
                if !finite3(*center)
                    || nonpositive(radius.0)
                    || !valid_spatial_circle_frame(*normal, *reference_direction)
                {
                    finding(
                        findings,
                        Check::Bounds,
                        id,
                        "invalid spatial circular sketch geometry",
                    );
                }
                if let SpatialSketchGeometry::Arc {
                    start_angle,
                    end_angle,
                    ..
                } = &entity.geometry
                {
                    if !start_angle.0.is_finite()
                        || !end_angle.0.is_finite()
                        || start_angle == end_angle
                    {
                        finding(
                            findings,
                            Check::ParameterDomain,
                            id,
                            "invalid spatial sketch arc interval",
                        );
                    }
                }
            }
            SpatialSketchGeometry::Nurbs {
                degree,
                knots,
                control_points,
                weights,
                ..
            } => {
                let expected = control_points.len().checked_add(*degree as usize + 1);
                if *degree == 0
                    || control_points.len() <= *degree as usize
                    || expected != Some(knots.len())
                    || knots.iter().any(|value| !value.is_finite())
                    || !knots_nondecreasing(knots)
                    || control_points.iter().any(|point| !finite3(*point))
                    || weights.as_ref().is_some_and(|weights| {
                        weights.len() != control_points.len()
                            || weights.iter().any(|weight| nonpositive(*weight))
                    })
                {
                    finding(
                        findings,
                        Check::ParameterDomain,
                        id,
                        "invalid spatial sketch NURBS",
                    );
                }
            }
            SpatialSketchGeometry::NurbsSurface {
                u_degree,
                v_degree,
                u_knots,
                v_knots,
                control_points,
            } => {
                let u_count = control_points.len();
                let v_count = control_points.first().map_or(0, Vec::len);
                let expected_u_knots = usize::try_from(*u_degree)
                    .ok()
                    .and_then(|degree| u_count.checked_add(degree)?.checked_add(1));
                let expected_v_knots = usize::try_from(*v_degree)
                    .ok()
                    .and_then(|degree| v_count.checked_add(degree)?.checked_add(1));
                if *u_degree == 0
                    || *v_degree == 0
                    || u_count <= *u_degree as usize
                    || v_count <= *v_degree as usize
                    || control_points.iter().any(|row| row.len() != v_count)
                    || expected_u_knots != Some(u_knots.len())
                    || expected_v_knots != Some(v_knots.len())
                    || u_knots.iter().any(|value| !value.is_finite())
                    || v_knots.iter().any(|value| !value.is_finite())
                    || !knots_nondecreasing(u_knots)
                    || !knots_nondecreasing(v_knots)
                    || control_points
                        .iter()
                        .flatten()
                        .any(|point| !finite3(*point))
                {
                    finding(
                        findings,
                        Check::ParameterDomain,
                        id,
                        "invalid spatial sketch NURBS surface",
                    );
                }
            }
            SpatialSketchGeometry::Native { native_kind } => {
                if native_kind.is_empty() {
                    finding(
                        findings,
                        Check::Counts,
                        id,
                        "empty native spatial sketch kind",
                    );
                }
            }
        }
    }

    let spatial_entities = ir
        .model
        .spatial_sketch_entities
        .iter()
        .map(|entity| (entity.id.clone(), entity.sketch.clone()))
        .collect::<HashMap<_, _>>();
    let spatial_geometry = ir
        .model
        .spatial_sketch_entities
        .iter()
        .map(|entity| (&entity.id, &entity.geometry))
        .collect::<HashMap<_, _>>();
    let parameter_values = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, &parameter.value))
        .collect::<HashMap<_, _>>();
    for constraint in &ir.model.spatial_sketch_constraints {
        if !spatial_sketches.contains(&constraint.sketch) {
            finding(
                findings,
                Check::ReferentialIntegrity,
                &constraint.id.0,
                "spatial constraint references a missing spatial sketch",
            );
        }
        let entities = match &constraint.definition {
            SpatialConstraint::Native { .. } => Vec::new(),
            SpatialConstraint::SplineGroup { entities } => entities.clone(),
            SpatialConstraint::Coincident { first, second }
            | SpatialConstraint::Tangent { first, second }
            | SpatialConstraint::PointDistance { first, second, .. }
            | SpatialConstraint::ParallelLineDistance { first, second, .. } => {
                vec![first.clone(), second.clone()]
            }
            SpatialConstraint::PointLineDistance { point, line, .. } => {
                vec![point.clone(), line.clone()]
            }
            SpatialConstraint::LineLength { entity, .. } => vec![entity.clone()],
            SpatialConstraint::RepeatedLineLength { entities, .. } => entities.clone(),
            SpatialConstraint::RepeatedParallelLineDistance { pairs, .. } => pairs
                .iter()
                .flat_map(|pair| [pair.first.clone(), pair.second.clone()])
                .collect(),
            SpatialConstraint::ParallelLineSetDistance { first, second, .. } => {
                first.iter().chain(second).cloned().collect()
            }
            SpatialConstraint::Offset {
                sources, results, ..
            } => sources.iter().chain(results).cloned().collect(),
            SpatialConstraint::Symmetric {
                first,
                second,
                axis,
            } => vec![first.clone(), second.clone(), axis.clone()],
            SpatialConstraint::Midpoint { point, entity } => {
                vec![point.clone(), entity.clone()]
            }
            SpatialConstraint::PointOnSurface { point, surface } => {
                vec![point.clone(), surface.clone()]
            }
            SpatialConstraint::ParallelToDirection { entity, .. } => vec![entity.clone()],
        };
        let distinct = entities.iter().collect::<HashSet<_>>();
        let valid_arity = match &constraint.definition {
            SpatialConstraint::Native { .. } => true,
            SpatialConstraint::ParallelToDirection { .. }
            | SpatialConstraint::LineLength { .. } => entities.len() == 1,
            SpatialConstraint::RepeatedLineLength { .. } => entities.len() >= 2,
            SpatialConstraint::RepeatedParallelLineDistance { pairs, .. } => pairs.len() >= 2,
            SpatialConstraint::ParallelLineSetDistance { first, second, .. } => {
                !first.is_empty() && !second.is_empty() && (first.len() > 1 || second.len() > 1)
            }
            SpatialConstraint::Offset {
                sources,
                results,
                normal,
                distance,
                parameter: _,
            } => {
                !sources.is_empty()
                    && !results.is_empty()
                    && (normal.norm() - 1.0).abs() <= EPS_SKETCHES_CHECK_SKETCHES_E9
                    && distance.0.is_finite()
                    && distance.0 > 0.0
            }
            _ => entities.len() >= 2,
        };
        if !valid_arity || distinct.len() != entities.len() {
            finding(
                findings,
                Check::Counts,
                &constraint.id.0,
                "invalid spatial constraint arity",
            );
        }
        for entity in &entities {
            if spatial_entities.get(entity) != Some(&constraint.sketch) {
                finding(
                    findings,
                    Check::ReferentialIntegrity,
                    &constraint.id.0,
                    "spatial constraint member does not belong to its sketch",
                );
            }
        }
        match &constraint.definition {
            SpatialConstraint::Native { .. } => {}
            SpatialConstraint::Coincident { first, second }
                if !matches!(
                    spatial_geometry.get(first),
                    Some(SpatialSketchGeometry::Point { .. })
                ) || !matches!(
                    spatial_geometry.get(second),
                    Some(SpatialSketchGeometry::Point { .. })
                ) =>
            {
                finding(
                    findings,
                    Check::ReferentialIntegrity,
                    &constraint.id.0,
                    "spatial coincidence requires two points",
                );
            }
            SpatialConstraint::Symmetric {
                first,
                second,
                axis,
            } => {
                let solved = match (
                    spatial_geometry.get(first),
                    spatial_geometry.get(second),
                    spatial_geometry.get(axis),
                ) {
                    (
                        Some(SpatialSketchGeometry::Point { position: first }),
                        Some(SpatialSketchGeometry::Point { position: second }),
                        Some(SpatialSketchGeometry::Line { start, end }),
                    ) => crate::eval::spatial_points_are_reflections(*first, *second, *start, *end),
                    _ => false,
                };
                if !solved {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "spatial symmetry requires two points reflected across a nondegenerate line",
                    );
                }
            }
            SpatialConstraint::Midpoint { point, entity }
                if !matches!(
                    spatial_geometry.get(point),
                    Some(SpatialSketchGeometry::Point { .. })
                ) || !matches!(
                    spatial_geometry.get(entity),
                    Some(SpatialSketchGeometry::Line { .. })
                ) =>
            {
                finding(
                    findings,
                    Check::ReferentialIntegrity,
                    &constraint.id.0,
                    "spatial midpoint requires a point and line",
                );
            }
            SpatialConstraint::PointOnSurface { point, surface }
                if !matches!(
                    spatial_geometry.get(point),
                    Some(SpatialSketchGeometry::Point { .. })
                ) || !matches!(
                    spatial_geometry.get(surface),
                    Some(SpatialSketchGeometry::NurbsSurface { .. })
                ) =>
            {
                finding(
                    findings,
                    Check::ReferentialIntegrity,
                    &constraint.id.0,
                    "spatial point-on-surface requires a point and surface",
                );
            }
            SpatialConstraint::Tangent { first, second }
                if !matches!(
                    spatial_geometry.get(first),
                    Some(
                        SpatialSketchGeometry::Line { .. }
                            | SpatialSketchGeometry::Circle { .. }
                            | SpatialSketchGeometry::Arc { .. }
                            | SpatialSketchGeometry::Nurbs { .. }
                    )
                ) || !matches!(
                    spatial_geometry.get(second),
                    Some(
                        SpatialSketchGeometry::Line { .. }
                            | SpatialSketchGeometry::Circle { .. }
                            | SpatialSketchGeometry::Arc { .. }
                            | SpatialSketchGeometry::Nurbs { .. }
                    )
                ) =>
            {
                finding(
                    findings,
                    Check::ReferentialIntegrity,
                    &constraint.id.0,
                    "spatial tangent requires two curves",
                );
            }
            SpatialConstraint::ParallelLineDistance {
                first,
                second,
                parameter,
            } => {
                let measured = spatial_geometry.get(first).and_then(|first| {
                    spatial_geometry
                        .get(second)
                        .and_then(|second| spatial_parallel_line_distance(first, second))
                });
                let expected = match parameter_values.get(parameter) {
                    Some(Some(crate::features::ParameterValue::Length(length))) => {
                        Some(length.0.abs())
                    }
                    _ => None,
                };
                let matches = measured.zip(expected).is_some_and(|(measured, expected)| {
                    let scale = 1.0 + measured.max(expected);
                    (measured - expected).abs() <= EPS_SKETCHES_CHECK_SKETCHES_E9 * scale
                });
                if !matches {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "spatial distance requires parallel lines separated by its length parameter",
                    );
                }
            }
            SpatialConstraint::RepeatedParallelLineDistance { pairs, parameter } => {
                let matches = pairs.iter().all(|pair| {
                    let measured = spatial_geometry.get(&pair.first).and_then(|first| {
                        spatial_geometry
                            .get(&pair.second)
                            .and_then(|second| spatial_parallel_line_distance(first, second))
                    });
                    spatial_length_parameter_matches(measured, parameter, &parameter_values)
                });
                if !matches {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "repeated spatial distance requires disjoint parallel-line pairs matching one length parameter",
                    );
                }
            }
            SpatialConstraint::PointDistance {
                first,
                second,
                parameter,
            } => {
                let measured = match (spatial_geometry.get(first), spatial_geometry.get(second)) {
                    (
                        Some(SpatialSketchGeometry::Point { position: first }),
                        Some(SpatialSketchGeometry::Point { position: second }),
                    ) => Some(
                        ((second.x - first.x).powi(2)
                            + (second.y - first.y).powi(2)
                            + (second.z - first.z).powi(2))
                        .sqrt(),
                    ),
                    _ => None,
                };
                let expected = match parameter_values.get(parameter) {
                    Some(Some(crate::features::ParameterValue::Length(length))) => {
                        Some(length.0.abs())
                    }
                    _ => None,
                };
                let matches = measured.zip(expected).is_some_and(|(measured, expected)| {
                    let scale = 1.0 + measured.max(expected);
                    (measured - expected).abs() <= EPS_SKETCHES_CHECK_SKETCHES_E9 * scale
                });
                if !matches {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "spatial point distance requires two points separated by its length parameter",
                    );
                }
            }
            SpatialConstraint::PointLineDistance {
                point,
                line,
                parameter,
            } => {
                if !matches!(
                    spatial_geometry.get(point),
                    Some(SpatialSketchGeometry::Point { .. })
                ) || !matches!(
                    spatial_geometry.get(line),
                    Some(SpatialSketchGeometry::Line { .. })
                ) {
                    finding(
                        findings,
                        Check::ReferentialIntegrity,
                        &constraint.id.0,
                        "spatial point-line distance requires a point and line",
                    );
                    continue;
                }
                let measured = spatial_geometry.get(point).and_then(|point| {
                    spatial_geometry
                        .get(line)
                        .and_then(|line| spatial_point_line_distance(point, line))
                });
                if !spatial_length_parameter_matches(measured, parameter, &parameter_values) {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "spatial point-line distance requires a point-to-line distance matching its length parameter",
                    );
                }
            }
            SpatialConstraint::LineLength { entity, parameter } => {
                let measured = spatial_geometry
                    .get(entity)
                    .and_then(|geometry| spatial_line_length(geometry));
                if !spatial_length_parameter_matches(measured, parameter, &parameter_values) {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "spatial line length requires a line matching its length parameter",
                    );
                }
            }
            SpatialConstraint::RepeatedLineLength {
                entities,
                parameter,
            } => {
                let measured = entities
                    .iter()
                    .map(|entity| {
                        spatial_geometry
                            .get(entity)
                            .and_then(|geometry| spatial_line_length(geometry))
                    })
                    .collect::<Option<Vec<_>>>();
                let matches = measured.is_some_and(|measured| {
                    measured.iter().all(|measured| {
                        spatial_length_parameter_matches(
                            Some(*measured),
                            parameter,
                            &parameter_values,
                        )
                    })
                });
                if !matches {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "repeated spatial line length requires distinct lines matching one length parameter",
                    );
                }
            }
            SpatialConstraint::ParallelLineSetDistance {
                first,
                second,
                parameter,
            } => {
                let first_geometry = first
                    .iter()
                    .filter_map(|entity| spatial_geometry.get(entity).copied())
                    .collect::<Vec<_>>();
                let second_geometry = second
                    .iter()
                    .filter_map(|entity| spatial_geometry.get(entity).copied())
                    .collect::<Vec<_>>();
                let tolerance = ir.tolerances.linear;
                let first_collinear = first_geometry.first().is_some_and(|reference| {
                    first_geometry.iter().all(|candidate| {
                        spatial_parallel_line_distance(reference, candidate)
                            .is_some_and(|distance| distance <= tolerance)
                    })
                });
                let second_collinear = second_geometry.first().is_some_and(|reference| {
                    second_geometry.iter().all(|candidate| {
                        spatial_parallel_line_distance(reference, candidate)
                            .is_some_and(|distance| distance <= tolerance)
                    })
                });
                let measured = first_geometry.iter().find_map(|first| {
                    second_geometry.iter().find_map(|second| {
                        spatial_parallel_line_span_distance(first, second, tolerance)
                    })
                });
                let matches = first_geometry.len() == first.len()
                    && second_geometry.len() == second.len()
                    && first_collinear
                    && second_collinear
                    && spatial_length_parameter_matches(measured, parameter, &parameter_values);
                if !matches {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "spatial parallel-line-set distance requires collinear carriers with overlapping spans separated by its length parameter",
                    );
                }
            }
            SpatialConstraint::Offset {
                sources,
                results,
                distance,
                parameter,
                ..
            } => {
                let curves_match = sources.iter().chain(results).all(|entity| {
                    spatial_geometry.get(entity).is_some_and(|geometry| {
                        matches!(
                            geometry,
                            SpatialSketchGeometry::Line { .. }
                                | SpatialSketchGeometry::Circle { .. }
                                | SpatialSketchGeometry::Arc { .. }
                                | SpatialSketchGeometry::Nurbs { .. }
                        )
                    })
                });
                let parameter_matches = match parameter {
                    None => true,
                    Some(parameter) => match parameter_values.get(&parameter.id) {
                        Some(Some(crate::features::ParameterValue::Length(value))) => {
                            let expected = if parameter.negated { -value.0 } else { value.0 };
                            let scale = 1.0 + expected.abs().max(distance.0);
                            (expected - distance.0).abs() <= EPS_SKETCHES_CHECK_SKETCHES_E9 * scale
                        }
                        _ => false,
                    },
                };
                if !curves_match {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "spatial offset source and result members must be curves",
                    );
                }
                if !parameter_matches {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "spatial offset distance does not match its parameter",
                    );
                }
            }
            SpatialConstraint::ParallelToDirection { entity, direction } => {
                let direction_norm = direction.norm();
                let Some(SpatialSketchGeometry::Line { start, end }) = spatial_geometry.get(entity)
                else {
                    finding(
                        findings,
                        Check::ReferentialIntegrity,
                        &constraint.id.0,
                        "spatial directional constraint requires a line",
                    );
                    continue;
                };
                let line =
                    crate::math::Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
                let line_norm = line.norm();
                let cross = crate::math::Vector3::new(
                    line.y * direction.z - line.z * direction.y,
                    line.z * direction.x - line.x * direction.z,
                    line.x * direction.y - line.y * direction.x,
                );
                if !direction_norm.is_finite()
                    || (direction_norm - 1.0).abs() > EPS_SKETCHES_CHECK_SKETCHES_E9
                    || !line_norm.is_finite()
                    || line_norm <= EPS_SKETCHES_CHECK_SKETCHES_E12
                    || cross.norm() > EPS_SKETCHES_CHECK_SKETCHES_E9 * line_norm
                {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "spatial line is not parallel to its constraint direction",
                    );
                }
            }
            _ => {}
        }
    }

    let geometry = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| (&entity.id, &entity.geometry))
        .collect::<HashMap<_, _>>();
    for constraint in &ir.model.sketch_constraints {
        if constraint
            .label_distance
            .iter()
            .chain(&constraint.label_position)
            .any(|value| !value.is_finite())
        {
            finding(
                findings,
                Check::Bounds,
                &constraint.id.0,
                "sketch constraint label placement is not finite",
            );
        }
        let valid = match &constraint.definition {
            Constraint::Coincident { entities } => entities.len() >= 2,
            Constraint::SplineGroup { entities } => entities.len() >= 2,
            Constraint::RectangularPattern { pattern } => {
                let directions = pattern.directions();
                let mut entities = HashSet::new();
                let dot = directions[0].direction[0] * directions[1].direction[0]
                    + directions[0].direction[1] * directions[1].direction[1];
                dot.abs() <= EPS_SKETCHES_CHECK_SKETCHES_E9
                    && directions.iter().all(|direction| {
                        let length = direction.direction[0].hypot(direction.direction[1]);
                        direction.spacing.0.is_finite()
                            && direction.direction.iter().all(|value| value.is_finite())
                            && (length - 1.0).abs() <= EPS_SKETCHES_CHECK_SKETCHES_E9
                    })
                    && pattern.rows().iter().flatten().all(|instance| {
                        instance
                            .entities
                            .iter()
                            .all(|entity| entities.insert(entity))
                    })
            }
            Constraint::CircularPattern { pattern } => {
                let instances = pattern.instances();
                let mut entities = HashSet::new();
                pattern.angle().0.is_finite()
                    && instances
                        .first()
                        .is_some_and(|instance| instance.angle.0 == 0.0)
                    && !instances
                        .iter()
                        .flat_map(|instance| &instance.entities)
                        .any(|entity| entity == pattern.center())
                    && instances.iter().all(|instance| {
                        instance.angle.0.is_finite()
                            && instance
                                .entities
                                .iter()
                                .all(|entity| entities.insert(entity))
                    })
            }
            Constraint::TextFrame { text, frame } => {
                matches!(geometry.get(text), Some(SketchGeometry::Text { .. }))
                    && !frame.is_empty()
                    && frame.iter().all(|entity| {
                        entity != text
                            && geometry.get(entity).is_some_and(|geometry| {
                                !matches!(geometry, SketchGeometry::Text { .. })
                            })
                    })
            }
            Constraint::TextPath {
                text,
                path,
                glyph_transforms,
            } => {
                matches!(geometry.get(text), Some(SketchGeometry::Text { .. }))
                    && text != path
                    && geometry.get(path).is_some_and(|geometry| {
                        !matches!(
                            geometry,
                            SketchGeometry::Point { .. } | SketchGeometry::Text { .. }
                        )
                    })
                    && !glyph_transforms.is_empty()
                    && glyph_transforms
                        .iter()
                        .all(crate::transform::Transform::is_affine)
            }
            Constraint::CoincidentLoci { loci } => loci.len() >= 2,
            Constraint::Distance { entities, .. } => !entities.is_empty(),
            Constraint::EqualDistance { first, second } => {
                let measured_distance = |pair: &SketchDistancePair| {
                    let first = sketch_locus_point(&pair.first, &geometry)?;
                    let second = sketch_locus_point(&pair.second, &geometry)?;
                    Some(distance2(first, second))
                };
                measured_distance(first)
                    .zip(measured_distance(second))
                    .is_none_or(|(first, second)| {
                        (first - second).abs()
                            <= ir
                                .tolerances
                                .linear
                                .max(EPS_EQUAL_DISTANCE * (1.0 + first.abs().max(second.abs())))
                    })
            }
            Constraint::DistanceLociValue {
                first,
                second,
                distance,
                parameter,
            } => {
                let measured_points =
                    sketch_locus_point(first, &geometry).zip(sketch_locus_point(second, &geometry));
                let distance_matches = measured_points.as_ref().is_none_or(|(first, second)| {
                    let measured = distance2(*first, *second);
                    (measured - distance.0).abs()
                        <= ir
                            .tolerances
                            .linear
                            .max(EPS_DISTANCE_VALUE * (1.0 + measured.abs().max(distance.0)))
                });
                let parameter_matches = parameter.as_ref().is_none_or(|parameter| {
                    let Some(Some(crate::features::ParameterValue::Length(value))) =
                        parameter_values.get(parameter)
                    else {
                        return false;
                    };
                    let expected = value.0.abs();
                    (expected - distance.0).abs()
                        <= ir
                            .tolerances
                            .linear
                            .max(EPS_DISTANCE_VALUE * (1.0 + expected.max(distance.0)))
                });
                distance.0.is_finite() && distance.0 >= 0.0 && distance_matches && parameter_matches
            }
            Constraint::PointCoordinateValues { point, values } => {
                let coordinate_matches = sketch_locus_point(point, &geometry).is_none_or(|point| {
                    [point.u, point.v]
                        .into_iter()
                        .zip(values)
                        .all(|(measured, expected)| {
                            expected.0.is_finite()
                                && (measured - expected.0).abs()
                                    <= ir.tolerances.linear.max(
                                        EPS_COORDINATE_VALUE
                                            * (1.0 + measured.abs().max(expected.0.abs())),
                                    )
                        })
                });
                values.iter().all(|value| value.0.is_finite()) && coordinate_matches
            }
            Constraint::MidpointCoordinate {
                first,
                second,
                axis,
                value,
            } => {
                let coordinate_matches = sketch_locus_point(first, &geometry)
                    .zip(sketch_locus_point(second, &geometry))
                    .is_none_or(|(first, second)| {
                        let measured = match axis {
                            crate::sketches::SketchCoordinateAxis::U => {
                                f64::midpoint(first.u, second.u)
                            }
                            crate::sketches::SketchCoordinateAxis::V => {
                                f64::midpoint(first.v, second.v)
                            }
                        };
                        (measured - value.0).abs()
                            <= ir.tolerances.linear.max(
                                EPS_COORDINATE_VALUE * (1.0 + measured.abs().max(value.0.abs())),
                            )
                    });
                value.0.is_finite() && coordinate_matches
            }
            Constraint::PolarDistance {
                first,
                second,
                distance,
                angle,
                distance_parameter,
            } => {
                let measured_points =
                    sketch_locus_point(first, &geometry).zip(sketch_locus_point(second, &geometry));
                let distance_matches = measured_points.as_ref().is_none_or(|(first, second)| {
                    let measured = distance2(*first, *second);
                    (measured - distance.0).abs()
                        <= ir
                            .tolerances
                            .linear
                            .max(EPS_POLAR_ANGLE * (1.0 + measured.abs().max(distance.0)))
                });
                let angle_matches = match (distance.0 <= EPS_POLAR_ZERO, angle.as_ref()) {
                    (true, None) => true,
                    (false, Some(angle)) => {
                        angle.0.is_finite()
                            && measured_points.as_ref().is_none_or(|(first, second)| {
                                let measured = (second.v - first.v).atan2(second.u - first.u);
                                let difference =
                                    (angle.0 - measured).rem_euclid(std::f64::consts::TAU);
                                difference.min(std::f64::consts::TAU - difference)
                                    <= EPS_POLAR_ANGLE
                            })
                    }
                    _ => false,
                };
                let parameter_matches = distance_parameter.as_ref().is_none_or(|parameter| {
                    let Some(Some(crate::features::ParameterValue::Length(value))) =
                        parameter_values.get(parameter)
                    else {
                        return false;
                    };
                    let expected = value.0.abs();
                    (expected - distance.0).abs()
                        <= ir
                            .tolerances
                            .linear
                            .max(EPS_POLAR_ANGLE * (1.0 + expected.max(distance.0)))
                });
                distance.0.is_finite()
                    && distance.0 >= 0.0
                    && distance_matches
                    && angle_matches
                    && parameter_matches
            }
            Constraint::AngleDifference {
                first,
                second,
                difference,
                value,
            } => {
                first.variable_type == 4
                    && second.variable_type == 4
                    && difference.variable_type == 0
                    && value.0.is_finite()
                    && (0.0..=std::f64::consts::PI).contains(&value.0)
            }
            Constraint::ScalarEquality { first, second } => {
                first.variable_type == 6 && second.variable_type == 6 && first.key != second.key
            }
            Constraint::RepeatedDistance { measurements, .. } => {
                let mut entities = HashSet::new();
                !measurements.is_empty()
                    && measurements.iter().all(|measurement| {
                        use crate::sketches::SketchDistanceMeasurement as Measurement;
                        let (first, second) = match measurement {
                            Measurement::Distance { first, second }
                            | Measurement::Horizontal { first, second }
                            | Measurement::Vertical { first, second } => (first, second),
                        };
                        let first = locus_entity(first);
                        let second = locus_entity(second);
                        first != second
                            && entities.insert(first.clone())
                            && entities.insert(second.clone())
                    })
            }
            Constraint::RepeatedLength { entities, .. } => {
                let distinct = entities.iter().collect::<HashSet<_>>();
                let lengths = entities
                    .iter()
                    .filter_map(|entity| match geometry.get(entity) {
                        Some(SketchGeometry::Line { start, end }) => {
                            Some((end.u - start.u).hypot(end.v - start.v))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                entities.len() >= 2
                    && distinct.len() == entities.len()
                    && lengths.len() == entities.len()
                    && lengths[1..].iter().all(|length| {
                        (length - lengths[0]).abs()
                            <= ir.tolerances.linear.max(
                                EPS_SKETCHES_CHECK_SKETCHES_E9
                                    * (1.0 + length.abs().max(lengths[0].abs())),
                            )
                    })
            }
            Constraint::ParallelLineSetDistance {
                first,
                second,
                parameter,
            } => {
                let distinct = first.iter().chain(second).collect::<HashSet<_>>();
                let first_geometry = first
                    .iter()
                    .filter_map(|entity| geometry.get(entity).copied())
                    .collect::<Vec<_>>();
                let second_geometry = second
                    .iter()
                    .filter_map(|entity| geometry.get(entity).copied())
                    .collect::<Vec<_>>();
                let tolerance = ir.tolerances.linear;
                let first_collinear = first_geometry.first().is_some_and(|reference| {
                    first_geometry.iter().all(|candidate| {
                        planar_parallel_line_distance(reference, candidate)
                            .is_some_and(|distance| distance <= tolerance)
                    })
                });
                let second_collinear = second_geometry.first().is_some_and(|reference| {
                    second_geometry.iter().all(|candidate| {
                        planar_parallel_line_distance(reference, candidate)
                            .is_some_and(|distance| distance <= tolerance)
                    })
                });
                let measured = first_geometry.iter().find_map(|first| {
                    second_geometry.iter().find_map(|second| {
                        planar_parallel_line_span_distance(first, second, tolerance)
                    })
                });
                let expected = match parameter_values.get(parameter) {
                    Some(Some(crate::features::ParameterValue::Length(length))) => {
                        Some(length.0.abs())
                    }
                    _ => None,
                };
                let measurement_matches =
                    measured.zip(expected).is_some_and(|(measured, expected)| {
                        (measured - expected).abs()
                            <= tolerance.max(
                                EPS_SKETCHES_CHECK_SKETCHES_E9
                                    * (1.0 + measured.abs().max(expected.abs())),
                            )
                    });
                !first.is_empty()
                    && !second.is_empty()
                    && (first.len() > 1 || second.len() > 1)
                    && distinct.len() == first.len() + second.len()
                    && first_geometry.len() == first.len()
                    && second_geometry.len() == second.len()
                    && first_collinear
                    && second_collinear
                    && measurement_matches
            }
            Constraint::RepeatedRadius { entities, .. }
            | Constraint::RepeatedDiameter { entities, .. } => {
                let distinct = entities.iter().collect::<HashSet<_>>();
                let radii = entities
                    .iter()
                    .filter_map(|entity| match geometry.get(entity) {
                        Some(
                            SketchGeometry::Circle { radius, .. }
                            | SketchGeometry::Arc { radius, .. },
                        ) => Some(radius.0),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                entities.len() >= 2
                    && distinct.len() == entities.len()
                    && radii.len() == entities.len()
                    && radii[1..].iter().all(|radius| {
                        (radius - radii[0]).abs()
                            <= ir.tolerances.linear.max(
                                EPS_SKETCHES_CHECK_SKETCHES_E9
                                    * (1.0 + radius.abs().max(radii[0].abs())),
                            )
                    })
            }
            Constraint::Offset {
                pairs,
                distance,
                parameter: _,
            } => {
                let mut sources = HashSet::new();
                let mut results = HashSet::new();
                !pairs.is_empty()
                    && pairs.iter().all(|pair| {
                        pair.source != pair.result
                            && sources.insert(&pair.source)
                            && results.insert(&pair.result)
                    })
                    && distance.0.is_finite()
                    && distance.0 > 0.0
            }
            Constraint::ProjectedCopy { source, result } => source != result,
            Constraint::Group { elements } | Constraint::Text { elements, .. } => {
                !elements.is_empty()
            }
            Constraint::Native {
                native_kind,
                entities,
                operands,
                ..
            } => {
                !native_kind.is_empty()
                    && (!entities.is_empty() || !operands.is_empty())
                    && operands.iter().all(|operand| {
                        !operand.native_kind.is_empty()
                            && operand
                                .native_field
                                .as_ref()
                                .is_none_or(|field| !field.is_empty())
                            && (operand.native_role.is_none() || operand.native_field.is_some())
                    })
            }
            _ => true,
        };
        if !valid {
            finding(
                findings,
                Check::Counts,
                &constraint.id.0,
                "invalid sketch constraint arity",
            );
        }
        if let Constraint::PointOnObject { point: _, entity } = &constraint.definition {
            if geometry
                .get(entity)
                .is_some_and(|geometry| matches!(geometry, SketchGeometry::Point { .. }))
            {
                finding(
                    findings,
                    Check::GeometricConsistency,
                    &constraint.id.0,
                    "point-on-object support is itself a point",
                );
            }
        }
        for locus in constraint_loci(&constraint.definition) {
            let Some(entity_geometry) = geometry.get(locus_entity(locus)) else {
                continue;
            };
            let valid = match locus {
                SketchLocus::Entity(_) => true,
                SketchLocus::Start(_) | SketchLocus::End(_) => !matches!(
                    entity_geometry,
                    SketchGeometry::Point { .. } | SketchGeometry::Circle { .. }
                ),
                SketchLocus::Center(_) => matches!(
                    entity_geometry,
                    SketchGeometry::Circle { .. }
                        | SketchGeometry::Arc { .. }
                        | SketchGeometry::Ellipse { .. }
                        | SketchGeometry::ExternalReference { .. }
                        | SketchGeometry::Native { .. }
                ),
            };
            if !valid {
                finding(
                    findings,
                    Check::GeometricConsistency,
                    &constraint.id.0,
                    "sketch constraint locus is incompatible with its entity",
                );
            }
        }
        if let Constraint::Offset {
            pairs, distance, ..
        } = &constraint.definition
        {
            for pair in pairs {
                let valid = entity_geometry
                    .get(&pair.source)
                    .zip(entity_geometry.get(&pair.result))
                    .is_none_or(|(source, result)| {
                        let expected = if pair.source_reversed {
                            -distance.0
                        } else {
                            distance.0
                        };
                        sketch_curve_offset_matches(source, result, expected, ir.tolerances.linear)
                    });
                if !valid {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "sketch offset pair does not match its oriented distance",
                    );
                }
            }
        }
        if let Constraint::ProjectedCopy { source, result } = &constraint.definition {
            let valid = entity_geometry
                .get(source)
                .zip(entity_geometry.get(result))
                .is_none_or(|(source, result)| source == result);
            if !valid {
                finding(
                    findings,
                    Check::GeometricConsistency,
                    &constraint.id.0,
                    "projected-copy entities do not have identical geometry",
                );
            }
        }
    }
}

fn distance2(left: crate::math::Point2, right: crate::math::Point2) -> f64 {
    (left.u - right.u).hypot(left.v - right.v)
}

fn planar_parallel_line_distance(first: &SketchGeometry, second: &SketchGeometry) -> Option<f64> {
    let (
        SketchGeometry::Line {
            start: first_start,
            end: first_end,
        },
        SketchGeometry::Line {
            start: second_start,
            end: second_end,
        },
    ) = (first, second)
    else {
        return None;
    };
    let first_direction =
        crate::math::Point2::new(first_end.u - first_start.u, first_end.v - first_start.v);
    let second_direction =
        crate::math::Point2::new(second_end.u - second_start.u, second_end.v - second_start.v);
    let first_length = first_direction.u.hypot(first_direction.v);
    let second_length = second_direction.u.hypot(second_direction.v);
    if first_length <= EPS_SKETCHES_PLANAR_PARALLEL_LINE_DISTANCE_E12
        || second_length <= EPS_SKETCHES_PLANAR_PARALLEL_LINE_DISTANCE_E12
    {
        return None;
    }
    let cross = first_direction.u * second_direction.v - first_direction.v * second_direction.u;
    if cross.abs() > EPS_SKETCHES_PLANAR_PARALLEL_LINE_DISTANCE_E9 * first_length * second_length {
        return None;
    }
    let offset = crate::math::Point2::new(
        second_start.u - first_start.u,
        second_start.v - first_start.v,
    );
    Some((offset.u * first_direction.v - offset.v * first_direction.u).abs() / first_length)
}

fn planar_parallel_line_span_distance(
    first: &SketchGeometry,
    second: &SketchGeometry,
    linear_tolerance: f64,
) -> Option<f64> {
    let distance = planar_parallel_line_distance(first, second)?;
    let (
        SketchGeometry::Line {
            start: first_start,
            end: first_end,
        },
        SketchGeometry::Line {
            start: second_start,
            end: second_end,
        },
    ) = (first, second)
    else {
        unreachable!("parallel line distance requires line geometry")
    };
    let direction =
        crate::math::Point2::new(first_end.u - first_start.u, first_end.v - first_start.v);
    let length = direction.u.hypot(direction.v);
    let project =
        |point: crate::math::Point2| (point.u * direction.u + point.v * direction.v) / length;
    let first_interval = [project(*first_start), project(*first_end)];
    let second_interval = [project(*second_start), project(*second_end)];
    let first_min = first_interval[0].min(first_interval[1]);
    let first_max = first_interval[0].max(first_interval[1]);
    let second_min = second_interval[0].min(second_interval[1]);
    let second_max = second_interval[0].max(second_interval[1]);
    (first_min.max(second_min) <= first_max.min(second_max) + linear_tolerance).then_some(distance)
}

fn oriented_endpoints(
    geometry: &SketchGeometry,
    reversed: bool,
) -> Option<(crate::math::Point2, crate::math::Point2)> {
    let endpoints = match geometry {
        SketchGeometry::Line { start, end } => (*start, *end),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => (
            circular_point(*center, radius.0, start_angle.0),
            circular_point(*center, radius.0, end_angle.0),
        ),
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds: Some([start, end]),
        } => (
            ellipse_point(
                *center,
                major_angle.0,
                major_radius.0,
                minor_radius.0,
                start.0,
            ),
            ellipse_point(
                *center,
                major_angle.0,
                major_radius.0,
                minor_radius.0,
                end.0,
            ),
        ),
        SketchGeometry::Nurbs {
            control_points,
            periodic: false,
            ..
        } if control_points.len() >= 2 => {
            (control_points[0], control_points[control_points.len() - 1])
        }
        _ => return None,
    };
    Some(if reversed {
        (endpoints.1, endpoints.0)
    } else {
        endpoints
    })
}

fn circular_point(center: crate::math::Point2, radius: f64, angle: f64) -> crate::math::Point2 {
    crate::math::Point2::new(
        center.u + radius * angle.cos(),
        center.v + radius * angle.sin(),
    )
}

fn ellipse_point(
    center: crate::math::Point2,
    angle: f64,
    major: f64,
    minor: f64,
    parameter: f64,
) -> crate::math::Point2 {
    crate::math::Point2::new(
        center.u + angle.cos() * major * parameter.cos() - angle.sin() * minor * parameter.sin(),
        center.v + angle.sin() * major * parameter.cos() + angle.cos() * minor * parameter.sin(),
    )
}

fn locus_entity(locus: &SketchLocus) -> &crate::sketches::SketchEntityId {
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity,
    }
}

fn sketch_locus_point(
    locus: &SketchLocus,
    geometry: &HashMap<&crate::sketches::SketchEntityId, &SketchGeometry>,
) -> Option<crate::math::Point2> {
    let entity_geometry = geometry.get(locus_entity(locus))?;
    match locus {
        SketchLocus::Entity(_) => match entity_geometry {
            SketchGeometry::Point { position } => Some(*position),
            _ => None,
        },
        SketchLocus::Start(_) | SketchLocus::End(_) => {
            let (start, end) = oriented_endpoints(entity_geometry, false)?;
            Some(if matches!(locus, SketchLocus::Start(_)) {
                start
            } else {
                end
            })
        }
        SketchLocus::Center(_) => match entity_geometry {
            SketchGeometry::Circle { center, .. }
            | SketchGeometry::Arc { center, .. }
            | SketchGeometry::Ellipse { center, .. }
            | SketchGeometry::Hyperbola { center, .. } => Some(*center),
            SketchGeometry::Parabola { vertex, .. } => Some(*vertex),
            _ => None,
        },
    }
}

fn constraint_loci(definition: &Constraint) -> Vec<&SketchLocus> {
    match definition {
        Constraint::CoincidentLoci { loci } => loci.iter().collect(),
        Constraint::Midpoint { point, .. }
        | Constraint::PointOnObject { point, .. }
        | Constraint::PointCoordinateValues { point, .. } => vec![point],
        Constraint::Symmetric { first, second, .. } => vec![first, second],
        Constraint::DistanceLoci { first, second, .. }
        | Constraint::DistanceLociValue { first, second, .. }
        | Constraint::MidpointCoordinate { first, second, .. }
        | Constraint::PolarDistance { first, second, .. }
        | Constraint::HorizontalDistance { first, second, .. }
        | Constraint::VerticalDistance { first, second, .. } => vec![first, second],
        Constraint::EqualDistance { first, second } => {
            [&first.first, &first.second, &second.first, &second.second]
                .into_iter()
                .collect()
        }
        Constraint::RepeatedDistance { measurements, .. } => measurements
            .iter()
            .flat_map(|measurement| {
                use crate::sketches::SketchDistanceMeasurement as Measurement;
                let (first, second) = match measurement {
                    Measurement::Distance { first, second }
                    | Measurement::Horizontal { first, second }
                    | Measurement::Vertical { first, second } => (first, second),
                };
                [first, second]
            })
            .collect(),
        Constraint::SnellsLaw {
            incident,
            refracted,
            ..
        } => vec![incident, refracted],
        Constraint::Group { elements } | Constraint::Text { elements, .. } => {
            elements.iter().collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
