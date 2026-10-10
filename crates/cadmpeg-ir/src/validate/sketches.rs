// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for sketches.

use super::record_finding;
use super::scans::{all, find_map};
use super::scratch::Scratch;
use crate::document::CadIr;
use crate::index::identities::BorrowedIdentities;
use crate::report::check::{Check, Finding};
use crate::sketches::{
    SketchConstraintDefinitionInput as Constraint, SketchDistancePair, SketchEntityKindRestriction,
    SketchGeometry, SketchGeometryDefinition, SketchLocus,
    SpatialSketchConstraintDefinitionInput as SpatialConstraint, SpatialSketchGeometry,
    SpatialSketchGeometryDefinition,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

mod equality;

const EPS_SKETCH_VALIDATION_GEOMETRY: f64 = 1.0e-9;
const EPS_SKETCH_VALIDATION_EXACT_GEOMETRY: f64 = 1.0e-12;

const EPS_EQUAL_DISTANCE: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_COORDINATE_VALUE: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_DISTANCE_VALUE: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_POLAR_ANGLE: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const SPATIAL_LINE_DEGENERACY_EPSILON: f64 = EPS_SKETCH_VALIDATION_EXACT_GEOMETRY;
const EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E9: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E12: f64 = EPS_SKETCH_VALIDATION_EXACT_GEOMETRY;
const EPS_SKETCHES_SPATIAL_PARALLEL_LINE_DISTANCE_E12: f64 = EPS_SKETCH_VALIDATION_EXACT_GEOMETRY;
const EPS_SKETCHES_SPATIAL_PARALLEL_LINE_DISTANCE_E9: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_SKETCHES_SPATIAL_LENGTH_PARAMETER_MATCHES_E9: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_SKETCHES_CHECK_SKETCHES_E9: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_SKETCHES_CHECK_SKETCHES_E12: f64 = EPS_SKETCH_VALIDATION_EXACT_GEOMETRY;
const EPS_SKETCHES_PLANAR_PARALLEL_LINE_DISTANCE_E12: f64 = EPS_SKETCH_VALIDATION_EXACT_GEOMETRY;
const EPS_SKETCHES_PLANAR_PARALLEL_LINE_DISTANCE_E9: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;

fn spatial_oriented_endpoints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &SpatialSketchGeometry,
    reversed: bool,
) -> Result<Option<(crate::math::Point3, crate::math::Point3)>, cadmpeg_core::decode::ResourceLimit>
{
    let endpoints = match geometry.definition() {
        SpatialSketchGeometryDefinition::Line { start, end } => (start.get(), end.get()),
        SpatialSketchGeometryDefinition::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        } => {
            let normal = normal.as_raw();
            let reference_direction = reference_direction.as_raw();
            let transverse = crate::math::Vector3::new(
                normal.y * reference_direction.z - normal.z * reference_direction.y,
                normal.z * reference_direction.x - normal.x * reference_direction.z,
                normal.x * reference_direction.y - normal.y * reference_direction.x,
            );
            let at = |angle: f64| {
                crate::math::Point3::new(
                    center.x
                        + radius.get()
                            * (reference_direction.x * angle.cos() + transverse.x * angle.sin()),
                    center.y
                        + radius.get()
                            * (reference_direction.y * angle.cos() + transverse.y * angle.sin()),
                    center.z
                        + radius.get()
                            * (reference_direction.z * angle.cos() + transverse.z * angle.sin()),
                )
            };
            (at(start_angle.get()), at(end_angle.get()))
        }
        SpatialSketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => {
            let start = curve.knots()[cadmpeg_core::decode::index_from_u32(curve.degree())];
            let end = curve.knots()[curve.pole_count()];
            let Some(start_point) =
                crate::eval::finite_or_refusal(crate::eval::decode::nurbs_curve_point_at(
                    crate::eval::admission::EvaluationAdmission::Decode(ctx),
                    curve,
                    start,
                ))?
            else {
                return Ok(None);
            };
            let Some(end_point) =
                crate::eval::finite_or_refusal(crate::eval::decode::nurbs_curve_point_at(
                    crate::eval::admission::EvaluationAdmission::Decode(ctx),
                    curve,
                    end,
                ))?
            else {
                return Ok(None);
            };
            (start_point.get(), end_point.get())
        }
        _ => return Ok(None),
    };
    Ok(Some(if reversed {
        (endpoints.1, endpoints.0)
    } else {
        endpoints
    }))
}

const EPS_FULL_CIRCLE_OFFSET: f64 = EPS_SKETCH_VALIDATION_GEOMETRY;
const EPS_OFFSET_SWEEP: f64 = EPS_SKETCH_VALIDATION_EXACT_GEOMETRY;

fn sketch_curve_offset_matches(
    ctx: &DecodeContext<'_>,
    source: &SketchGeometry,
    result: &SketchGeometry,
    expected: f64,
    linear_tolerance: f64,
) -> Result<bool, CodecError> {
    if let (
        SketchGeometryDefinition::Circle {
            center: source_center,
            radius: source_radius,
        },
        SketchGeometryDefinition::Circle {
            center: result_center,
            radius: result_radius,
        },
    ) = (source.definition(), result.definition())
    {
        let scale = 1.0
            + source_center
                .u
                .abs()
                .max(source_center.v.abs())
                .max(result_center.u.abs())
                .max(result_center.v.abs())
                .max(source_radius.get().abs())
                .max(result_radius.get().abs())
                .max(expected.abs());
        return Ok(expected.is_finite()
            && (source_center.u - result_center.u).abs() <= EPS_FULL_CIRCLE_OFFSET * scale
            && (source_center.v - result_center.v).abs() <= EPS_FULL_CIRCLE_OFFSET * scale
            && (source_radius.get() - result_radius.get() - expected).abs()
                <= EPS_FULL_CIRCLE_OFFSET * scale);
    }

    if let (
        SketchGeometryDefinition::Circle {
            center: source_center,
            radius: source_radius,
        },
        SketchGeometryDefinition::Arc {
            center: result_center,
            radius: result_radius,
            start_angle: result_start,
            end_angle: result_end,
        },
    ) = (source.definition(), result.definition())
    {
        let scale = 1.0
            + source_center
                .u
                .abs()
                .max(source_center.v.abs())
                .max(result_center.u.abs())
                .max(result_center.v.abs())
                .max(source_radius.get().abs())
                .max(result_radius.get().abs())
                .max(expected.abs());
        let result_sweep = result_end.get() - result_start.get();
        return Ok(expected.is_finite()
            && result_sweep.abs() > EPS_OFFSET_SWEEP
            && (source_center.u - result_center.u).abs() <= EPS_FULL_CIRCLE_OFFSET * scale
            && (source_center.v - result_center.v).abs() <= EPS_FULL_CIRCLE_OFFSET * scale
            && (source_radius.get() - result_radius.get() - expected).abs()
                <= EPS_FULL_CIRCLE_OFFSET * scale);
    }

    if let (
        SketchGeometryDefinition::Arc {
            center: source_center,
            radius: source_radius,
            start_angle: source_start,
            end_angle: source_end,
        },
        SketchGeometryDefinition::Circle {
            center: result_center,
            radius: result_radius,
        },
    ) = (source.definition(), result.definition())
    {
        let scale = 1.0
            + source_center
                .u
                .abs()
                .max(source_center.v.abs())
                .max(result_center.u.abs())
                .max(result_center.v.abs())
                .max(source_radius.get().abs())
                .max(result_radius.get().abs())
                .max(expected.abs());
        let source_sweep = source_end.get() - source_start.get();
        return Ok(expected.is_finite()
            && source_sweep.abs() > EPS_OFFSET_SWEEP
            && (source_center.u - result_center.u).abs() <= EPS_FULL_CIRCLE_OFFSET * scale
            && (source_center.v - result_center.v).abs() <= EPS_FULL_CIRCLE_OFFSET * scale
            && (source_sweep.signum() * (source_radius.get() - result_radius.get()) - expected)
                .abs()
                <= EPS_FULL_CIRCLE_OFFSET * scale);
    }

    if let (
        SketchGeometryDefinition::Arc {
            center: source_center,
            radius: source_radius,
            start_angle: source_start,
            end_angle: source_end,
        },
        SketchGeometryDefinition::Arc {
            center: result_center,
            radius: result_radius,
            start_angle: result_start,
            end_angle: result_end,
        },
    ) = (source.definition(), result.definition())
    {
        let scale = 1.0
            + source_center
                .u
                .abs()
                .max(source_center.v.abs())
                .max(result_center.u.abs())
                .max(result_center.v.abs())
                .max(source_radius.get())
                .max(result_radius.get())
                .max(expected.abs());
        let source_sweep = source_end.get() - source_start.get();
        let result_sweep = result_end.get() - result_start.get();
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
        let angular_overlap = [source_start.get(), source_end.get()]
            .into_iter()
            .any(|angle| angle_in_sweep(angle, result_start.get(), result_end.get()))
            || [result_start.get(), result_end.get()]
                .into_iter()
                .any(|angle| angle_in_sweep(angle, source_start.get(), source_end.get()));
        return Ok(source_sweep.abs() > EPS_OFFSET_SWEEP
            && result_sweep.abs() > EPS_OFFSET_SWEEP
            && source_sweep.signum() == result_sweep.signum()
            && angular_overlap
            && (source_center.u - result_center.u).abs()
                <= EPS_SKETCH_VALIDATION_GEOMETRY * scale
            && (source_center.v - result_center.v).abs()
                <= EPS_SKETCH_VALIDATION_GEOMETRY * scale
            && (source_sweep.signum() * (source_radius.get() - result_radius.get()) - expected)
                .abs()
                <= EPS_SKETCH_VALIDATION_GEOMETRY * scale);
    }

    if let Some(distance) =
        crate::eval::fitted_nurbs_offset_frame_distance(ctx, source, result, linear_tolerance)?
    {
        let distance = distance.get();
        let scale = 1.0 + distance.abs().max(expected.abs());
        return Ok(expected.is_finite()
            && (distance - expected).abs()
                <= linear_tolerance.max(EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E9 * scale));
    }

    let (
        SketchGeometryDefinition::Line {
            start: source_start,
            end: source_end,
        },
        SketchGeometryDefinition::Line {
            start: result_start,
            end: result_end,
        },
    ) = (source.definition(), result.definition())
    else {
        return Ok(false);
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
        return Ok(false);
    }
    let scale = 1.0 + expected.abs();
    let parallel = (source_du * result_dv - source_dv * result_du).abs()
        <= EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E9 * source_length * result_length;
    let normal_u = -source_dv / source_length;
    let normal_v = source_du / source_length;
    let distance_at = |point: &crate::math::Point2| {
        (point.u - source_start.u) * normal_u + (point.v - source_start.v) * normal_v
    };
    Ok(parallel
        && (distance_at(result_start) - expected).abs()
            <= EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E9 * scale
        && (distance_at(result_end) - expected).abs()
            <= EPS_SKETCHES_SKETCH_CURVE_OFFSET_MATCHES_E9 * scale)
}

struct SpatialParallelLines {
    first: [crate::math::Point3; 2],
    second: [crate::math::Point3; 2],
    distance: f64,
}

fn spatial_parallel_line_distance(
    first: &SpatialSketchGeometry,
    second: &SpatialSketchGeometry,
) -> Option<f64> {
    spatial_parallel_lines(first, second).map(|lines| lines.distance)
}

fn spatial_parallel_lines(
    first: &SpatialSketchGeometry,
    second: &SpatialSketchGeometry,
) -> Option<SpatialParallelLines> {
    let (
        SpatialSketchGeometryDefinition::Line {
            start: first_start,
            end: first_end,
        },
        SpatialSketchGeometryDefinition::Line {
            start: second_start,
            end: second_end,
        },
    ) = (first.definition(), second.definition())
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
    Some(SpatialParallelLines {
        first: [first_start.get(), first_end.get()],
        second: [second_start.get(), second_end.get()],
        distance: crate::math::Vector3::new(
            offset.y * first_direction.z - offset.z * first_direction.y,
            offset.z * first_direction.x - offset.x * first_direction.z,
            offset.x * first_direction.y - offset.y * first_direction.x,
        )
        .norm()
            / first_length,
    })
}

fn spatial_line_length(geometry: &SpatialSketchGeometry) -> Option<f64> {
    let SpatialSketchGeometryDefinition::Line { start, end } = geometry.definition() else {
        return None;
    };
    Some((end.x - start.x).hypot((end.y - start.y).hypot(end.z - start.z)))
}

fn spatial_point_line_distance(
    point: &SpatialSketchGeometry,
    line: &SpatialSketchGeometry,
) -> Option<f64> {
    let (
        SpatialSketchGeometryDefinition::Point { position },
        SpatialSketchGeometryDefinition::Line { start, end },
    ) = (point.definition(), line.definition())
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
    let lines = spatial_parallel_lines(first, second)?;
    let [first_start, first_end] = lines.first;
    let [second_start, second_end] = lines.second;
    let direction = crate::math::Vector3::new(
        first_end.x - first_start.x,
        first_end.y - first_start.y,
        first_end.z - first_start.z,
    );
    let length = direction.norm();
    let project = |point: crate::math::Point3| {
        (point.x * direction.x + point.y * direction.y + point.z * direction.z) / length
    };
    let first_interval = [project(first_start), project(first_end)];
    let second_interval = [project(second_start), project(second_end)];
    let first_min = first_interval[0].min(first_interval[1]);
    let first_max = first_interval[0].max(first_interval[1]);
    let second_min = second_interval[0].min(second_interval[1]);
    let second_max = second_interval[0].max(second_interval[1]);
    let lower = first_min.max(second_min);
    let upper = first_max.min(second_max);
    (lower <= upper || lower - upper <= linear_tolerance).then_some(lines.distance)
}

fn spatial_length_parameter_matches(
    ctx: &DecodeContext<'_>,
    measured: Option<f64>,
    parameter: &crate::features::ParameterId,
    parameter_values: &BorrowedIdentities<'_, '_, &Option<crate::features::ParameterValue>>,
) -> Result<bool, CodecError> {
    let expected = match parameter_values.get(ctx, parameter.as_str())? {
        Some(Some(crate::features::ParameterValue::Length(length))) => length.get().abs(),
        _ => return Ok(false),
    };
    Ok(measured.is_some_and(|measured| {
        let scale = 1.0 + measured.abs().max(expected.abs());
        (measured - expected).abs() <= EPS_SKETCHES_SPATIAL_LENGTH_PARAMETER_MATCHES_E9 * scale
    }))
}

fn same_spatial_owner(
    ctx: &DecodeContext<'_>,
    owner: Option<&crate::sketches::SpatialSketchId>,
    expected: &crate::sketches::SpatialSketchId,
) -> Result<bool, CodecError> {
    let Some(owner) = owner else {
        return Ok(false);
    };
    equality::text_equal(
        ctx,
        owner.as_str(),
        expected.as_str(),
        "compare spatial sketch owner",
    )
}

pub(super) fn check_sketches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), cadmpeg_core::CodecError> {
    let geometry = BorrowedIdentities::build(ctx, |add| {
        for entity in ctx.admit_iter(&ir.model.sketch_entities, "sketch entity identity scan")? {
            add(entity.id().as_str(), &entity.geometry)?;
        }
        Ok(())
    })?;
    for sketch in &ir.model.sketches {
        ctx.charge_work(1, "planar sketch scan")?;
        if sketch.resolved_placement().is_none() {
            continue;
        }
        for profile in ctx.admit_iter(sketch.profiles.as_slice(), "sketch profile scan")? {
            for adjacent in profile.windows(2) {
                ctx.charge_work(1, "sketch profile adjacency scan")?;
                let Some(left) = geometry
                    .get(ctx, adjacent[0].entity.as_str())?
                    .and_then(|geometry| oriented_endpoints(geometry, adjacent[0].reversed))
                else {
                    continue;
                };
                let Some(right) = geometry
                    .get(ctx, adjacent[1].entity.as_str())?
                    .and_then(|geometry| oriented_endpoints(geometry, adjacent[1].reversed))
                else {
                    continue;
                };
                if distance2(left.1, right.0) > ir.tolerances.linear.get() {
                    record_finding(
                        ctx,
                        findings,
                        Check::GeometricConsistency,
                        crate::report::Severity::Error,
                        Some(sketch.id.as_str()),
                        format_args!("{}", "sketch profile has disconnected consecutive entities"),
                    )?;
                }
            }
        }
    }

    let spatial_sketches = BorrowedIdentities::build(ctx, |add| {
        for sketch in ctx.admit_iter(&ir.model.spatial_sketches, "spatial sketch identity scan")? {
            add(sketch.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let spatial_geometry = BorrowedIdentities::build(ctx, |add| {
        for entity in ctx.admit_iter(
            &ir.model.spatial_sketch_entities,
            "spatial sketch geometry identity scan",
        )? {
            add(entity.id().as_str(), (&entity.sketch, &entity.geometry))?;
        }
        Ok(())
    })?;
    for sketch in &ir.model.spatial_sketches {
        ctx.charge_work(1, "spatial sketch scan")?;
        for profile in &sketch.profiles {
            ctx.charge_work(1, "sketch profile scan")?;
            for use_ in profile.boundary() {
                ctx.charge_work(1, "spatial profile member scan")?;
                if !same_spatial_owner(
                    ctx,
                    spatial_geometry
                        .get(ctx, use_.entity.as_str())?
                        .map(|(owner, _)| *owner),
                    &sketch.id,
                )? {
                    record_finding(
                        ctx,
                        findings,
                        Check::ReferentialIntegrity,
                        crate::report::Severity::Error,
                        Some(sketch.id.as_str()),
                        format_args!(
                            "{}",
                            "spatial sketch profile entity does not belong to its sketch"
                        ),
                    )?;
                }
            }
            if profile.boundary().len() == 1 {
                if !matches!(
                    spatial_geometry
                        .get(ctx, profile.boundary()[0].entity.as_str())?
                        .map(|(sketch, geometry)| (sketch, geometry.definition())),
                    Some((_, SpatialSketchGeometryDefinition::Circle { .. }))
                ) {
                    record_finding(
                        ctx,
                        findings,
                        Check::GeometricConsistency,
                        crate::report::Severity::Error,
                        Some(sketch.id.as_str()),
                        format_args!(
                            "{}",
                            "single-entity spatial sketch profile is not a full circle"
                        ),
                    )?;
                }
            } else {
                for index in 0..profile.boundary().len() {
                    ctx.charge_work(1, "spatial profile adjacency scan")?;
                    let left = &profile.boundary()[index];
                    let right = &profile.boundary()[(index + 1) % profile.boundary().len()];
                    let left_endpoints = match spatial_geometry.get(ctx, left.entity.as_str())? {
                        Some((_, geometry)) => {
                            spatial_oriented_endpoints(ctx, geometry, left.reversed)?
                        }
                        None => None,
                    };
                    let right_endpoints = match spatial_geometry.get(ctx, right.entity.as_str())? {
                        Some((_, geometry)) => {
                            spatial_oriented_endpoints(ctx, geometry, right.reversed)?
                        }
                        None => None,
                    };
                    let endpoints = left_endpoints.zip(right_endpoints);
                    if endpoints.is_some_and(|(left, right)| {
                        (left.1.x - right.0.x)
                            .hypot(left.1.y - right.0.y)
                            .hypot(left.1.z - right.0.z)
                            > ir.tolerances.linear.get()
                    }) {
                        record_finding(
                            ctx,
                            findings,
                            Check::GeometricConsistency,
                            crate::report::Severity::Error,
                            Some(sketch.id.as_str()),
                            format_args!(
                                "{}",
                                "spatial sketch profile has disconnected consecutive entities"
                            ),
                        )?;
                    }
                }
            }
        }
    }
    for entity in &ir.model.spatial_sketch_entities {
        ctx.charge_work(1, "spatial sketch entity scan")?;
        let id = entity.id().as_str();
        if !spatial_sketches.contains(ctx, entity.sketch.as_str())? {
            record_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                crate::report::Severity::Error,
                Some(id),
                format_args!(
                    "{}",
                    "spatial sketch entity references a missing spatial sketch"
                ),
            )?;
        }
        if let SpatialSketchGeometryDefinition::NurbsSurface { surface } =
            entity.geometry.definition()
        {
            if surface.u_degree() == 0 || surface.v_degree() == 0 {
                record_finding(
                    ctx,
                    findings,
                    Check::ParameterDomain,
                    crate::report::Severity::Error,
                    Some(id),
                    format_args!("{}", "invalid spatial sketch NURBS surface"),
                )?;
            }
        }
    }

    let parameter_values = BorrowedIdentities::build(ctx, |add| {
        for parameter in ctx.admit_iter(&ir.model.parameters, "sketch parameter identity scan")? {
            add(parameter.id.as_str(), &parameter.value)?;
        }
        Ok(())
    })?;
    for constraint in &ir.model.spatial_sketch_constraints {
        ctx.charge_work(1, "spatial constraint scan")?;
        if !spatial_sketches.contains(ctx, constraint.sketch.as_str())? {
            record_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                crate::report::Severity::Error,
                Some(constraint.id.as_str()),
                format_args!(
                    "{}",
                    "spatial constraint references a missing spatial sketch"
                ),
            )?;
        }
        visit_spatial_constraint_entities(ctx, constraint.definition.kind(), |entity| {
            if !same_spatial_owner(
                ctx,
                spatial_geometry
                    .get(ctx, entity.as_str())?
                    .map(|(owner, _)| *owner),
                &constraint.sketch,
            )? {
                record_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(constraint.id.as_str()),
                    format_args!(
                        "{}",
                        "spatial constraint member does not belong to its sketch"
                    ),
                )?;
            }
            Ok(())
        })?;
        match constraint.definition.kind() {
            SpatialConstraint::Native { .. } => {}
            SpatialConstraint::Coincident { first, second }
                if !matches!(
                    spatial_geometry
                        .get(ctx, first.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                    Some(SpatialSketchGeometryDefinition::Point { .. })
                ) || !matches!(
                    spatial_geometry
                        .get(ctx, second.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                    Some(SpatialSketchGeometryDefinition::Point { .. })
                ) =>
            {
                record_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(constraint.id.as_str()),
                    format_args!("{}", "spatial coincidence requires two points"),
                )?;
            }
            SpatialConstraint::Symmetric {
                first,
                second,
                axis,
            } => {
                let solved = match (
                    spatial_geometry
                        .get(ctx, first.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                    spatial_geometry
                        .get(ctx, second.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                    spatial_geometry
                        .get(ctx, axis.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                ) {
                    (
                        Some(SpatialSketchGeometryDefinition::Point { position: first }),
                        Some(SpatialSketchGeometryDefinition::Point { position: second }),
                        Some(SpatialSketchGeometryDefinition::Line { start, end }),
                    ) => crate::eval::spatial_points_are_reflections(
                        first.get(),
                        second.get(),
                        start.get(),
                        end.get(),
                    ),
                    _ => false,
                };
                if !solved {
                    record_finding(ctx, findings, Check::GeometricConsistency, crate::report::Severity::Error, Some(constraint.id.as_str()), format_args!("{}", "spatial symmetry requires two points reflected across a nondegenerate line"))?;
                }
            }
            SpatialConstraint::Midpoint { point, entity }
                if !matches!(
                    spatial_geometry
                        .get(ctx, point.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                    Some(SpatialSketchGeometryDefinition::Point { .. })
                ) || !matches!(
                    spatial_geometry
                        .get(ctx, entity.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                    Some(SpatialSketchGeometryDefinition::Line { .. })
                ) =>
            {
                record_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(constraint.id.as_str()),
                    format_args!("{}", "spatial midpoint requires a point and line"),
                )?;
            }
            SpatialConstraint::PointOnSurface { point, surface }
                if !matches!(
                    spatial_geometry
                        .get(ctx, point.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                    Some(SpatialSketchGeometryDefinition::Point { .. })
                ) || !matches!(
                    spatial_geometry
                        .get(ctx, surface.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                    Some(SpatialSketchGeometryDefinition::NurbsSurface { .. })
                ) =>
            {
                record_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(constraint.id.as_str()),
                    format_args!(
                        "{}",
                        "spatial point-on-surface requires a point and surface"
                    ),
                )?;
            }
            SpatialConstraint::Tangent { first, second }
                if !matches!(
                    spatial_geometry
                        .get(ctx, first.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                    Some(
                        SpatialSketchGeometryDefinition::Line { .. }
                            | SpatialSketchGeometryDefinition::Circle { .. }
                            | SpatialSketchGeometryDefinition::Arc { .. }
                            | SpatialSketchGeometryDefinition::Nurbs { .. }
                    )
                ) || !matches!(
                    spatial_geometry
                        .get(ctx, second.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                    Some(
                        SpatialSketchGeometryDefinition::Line { .. }
                            | SpatialSketchGeometryDefinition::Circle { .. }
                            | SpatialSketchGeometryDefinition::Arc { .. }
                            | SpatialSketchGeometryDefinition::Nurbs { .. }
                    )
                ) =>
            {
                record_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(constraint.id.as_str()),
                    format_args!("{}", "spatial tangent requires two curves"),
                )?;
            }
            SpatialConstraint::ParallelLineDistance {
                first,
                second,
                parameter,
            } => {
                let measured = match spatial_geometry
                    .get(ctx, first.as_str())?
                    .map(|(_, geometry)| geometry)
                {
                    Some(first) => spatial_geometry
                        .get(ctx, second.as_str())?
                        .map(|(_, geometry)| geometry)
                        .and_then(|second| spatial_parallel_line_distance(first, second)),
                    None => None,
                };
                let expected = match parameter_values.get(ctx, parameter.as_str())? {
                    Some(Some(crate::features::ParameterValue::Length(length))) => {
                        Some(length.get().abs())
                    }
                    _ => None,
                };
                let matches = measured.zip(expected).is_some_and(|(measured, expected)| {
                    let scale = 1.0 + measured.max(expected);
                    measured.is_finite()
                        && (measured - expected).abs() <= EPS_SKETCHES_CHECK_SKETCHES_E9 * scale
                });
                if !matches {
                    record_finding(ctx, findings, Check::GeometricConsistency, crate::report::Severity::Error, Some(constraint.id.as_str()), format_args!("{}", "spatial distance requires parallel lines separated by its length parameter"))?;
                }
            }
            SpatialConstraint::RepeatedParallelLineDistance { pairs, parameter } => {
                let matches = all(ctx, pairs, |pair| {
                    let measured = match spatial_geometry
                        .get(ctx, pair.first.as_str())?
                        .map(|(_, geometry)| geometry)
                    {
                        Some(first) => spatial_geometry
                            .get(ctx, pair.second.as_str())?
                            .map(|(_, geometry)| geometry)
                            .and_then(|second| spatial_parallel_line_distance(first, second)),
                        None => None,
                    };
                    spatial_length_parameter_matches(ctx, measured, parameter, &parameter_values)
                })?;
                if !matches {
                    record_finding(ctx, findings, Check::GeometricConsistency, crate::report::Severity::Error, Some(constraint.id.as_str()), format_args!("{}", "repeated spatial distance requires disjoint parallel-line pairs matching one length parameter"))?;
                }
            }
            SpatialConstraint::PointDistance {
                first,
                second,
                parameter,
            } => {
                let measured = match (
                    spatial_geometry
                        .get(ctx, first.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                    spatial_geometry
                        .get(ctx, second.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                ) {
                    (
                        Some(SpatialSketchGeometryDefinition::Point { position: first }),
                        Some(SpatialSketchGeometryDefinition::Point { position: second }),
                    ) => Some(first.distance(second.get())),
                    _ => None,
                };
                let expected = match parameter_values.get(ctx, parameter.as_str())? {
                    Some(Some(crate::features::ParameterValue::Length(length))) => {
                        Some(length.get().abs())
                    }
                    _ => None,
                };
                let matches = measured.zip(expected).is_some_and(|(measured, expected)| {
                    let scale = 1.0 + measured.max(expected);
                    measured.is_finite()
                        && (measured - expected).abs() <= EPS_SKETCHES_CHECK_SKETCHES_E9 * scale
                });
                if !matches {
                    record_finding(ctx, findings, Check::GeometricConsistency, crate::report::Severity::Error, Some(constraint.id.as_str()), format_args!("{}", "spatial point distance requires two points separated by its length parameter"))?;
                }
            }
            SpatialConstraint::PointLineDistance {
                point,
                line,
                parameter,
            } => {
                if !matches!(
                    spatial_geometry
                        .get(ctx, point.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                    Some(SpatialSketchGeometryDefinition::Point { .. })
                ) || !matches!(
                    spatial_geometry
                        .get(ctx, line.as_str())?
                        .map(|(_, geometry)| geometry)
                        .map(|geometry| geometry.definition()),
                    Some(SpatialSketchGeometryDefinition::Line { .. })
                ) {
                    record_finding(
                        ctx,
                        findings,
                        Check::ReferentialIntegrity,
                        crate::report::Severity::Error,
                        Some(constraint.id.as_str()),
                        format_args!(
                            "{}",
                            "spatial point-line distance requires a point and line"
                        ),
                    )?;
                    continue;
                }
                let measured = match spatial_geometry
                    .get(ctx, point.as_str())?
                    .map(|(_, geometry)| geometry)
                {
                    Some(point) => spatial_geometry
                        .get(ctx, line.as_str())?
                        .map(|(_, geometry)| geometry)
                        .and_then(|line| spatial_point_line_distance(point, line)),
                    None => None,
                };
                if !spatial_length_parameter_matches(ctx, measured, parameter, &parameter_values)? {
                    record_finding(ctx, findings, Check::GeometricConsistency, crate::report::Severity::Error, Some(constraint.id.as_str()), format_args!("{}", "spatial point-line distance requires a point-to-line distance matching its length parameter"))?;
                }
            }
            SpatialConstraint::LineLength { entity, parameter } => {
                let measured = spatial_geometry
                    .get(ctx, entity.as_str())?
                    .map(|(_, geometry)| geometry)
                    .and_then(|geometry| spatial_line_length(geometry));
                if !spatial_length_parameter_matches(ctx, measured, parameter, &parameter_values)? {
                    record_finding(
                        ctx,
                        findings,
                        Check::GeometricConsistency,
                        crate::report::Severity::Error,
                        Some(constraint.id.as_str()),
                        format_args!(
                            "{}",
                            "spatial line length requires a line matching its length parameter"
                        ),
                    )?;
                }
            }
            SpatialConstraint::RepeatedLineLength {
                entities,
                parameter,
            } => {
                let mut measured = Scratch::new(ctx)?;
                let mut complete = true;
                for entity in entities {
                    ctx.charge_work(1, "spatial repeated length scan")?;
                    let Some(length) = spatial_geometry
                        .get(ctx, entity.as_str())?
                        .map(|(_, geometry)| geometry)
                        .and_then(|geometry| spatial_line_length(geometry))
                    else {
                        complete = false;
                        break;
                    };
                    measured.push(length)?;
                }
                let matches = complete
                    && all(ctx, measured.iter(), |measured| {
                        spatial_length_parameter_matches(
                            ctx,
                            Some(*measured),
                            parameter,
                            &parameter_values,
                        )
                    })?;
                if !matches {
                    record_finding(ctx, findings, Check::GeometricConsistency, crate::report::Severity::Error, Some(constraint.id.as_str()), format_args!("{}", "repeated spatial line length requires distinct lines matching one length parameter"))?;
                }
            }
            SpatialConstraint::ParallelLineSetDistance {
                first,
                second,
                parameter,
            } => {
                let first_geometry = Scratch::filter_map(ctx, first, |entity| {
                    Ok(spatial_geometry
                        .get(ctx, entity.as_str())?
                        .map(|(_, geometry)| geometry)
                        .copied())
                })?;
                let second_geometry = Scratch::filter_map(ctx, second, |entity| {
                    Ok(spatial_geometry
                        .get(ctx, entity.as_str())?
                        .map(|(_, geometry)| geometry)
                        .copied())
                })?;
                let tolerance = ir.tolerances.linear.get();
                let first_collinear = match first_geometry.first() {
                    Some(reference) => all(ctx, first_geometry.iter(), |candidate| {
                        Ok(spatial_parallel_line_distance(reference, candidate)
                            .is_some_and(|distance| distance <= tolerance))
                    })?,
                    None => false,
                };
                let second_collinear = match second_geometry.first() {
                    Some(reference) => all(ctx, second_geometry.iter(), |candidate| {
                        Ok(spatial_parallel_line_distance(reference, candidate)
                            .is_some_and(|distance| distance <= tolerance))
                    })?,
                    None => false,
                };
                let measured = find_map(ctx, first_geometry.iter(), |first| {
                    find_map(ctx, second_geometry.iter(), |second| {
                        Ok(spatial_parallel_line_span_distance(
                            first, second, tolerance,
                        ))
                    })
                })?;
                let matches = first_geometry.len() == first.len()
                    && second_geometry.len() == second.len()
                    && first_collinear
                    && second_collinear
                    && spatial_length_parameter_matches(
                        ctx,
                        measured,
                        parameter,
                        &parameter_values,
                    )?;
                if !matches {
                    record_finding(ctx, findings, Check::GeometricConsistency, crate::report::Severity::Error, Some(constraint.id.as_str()), format_args!("{}", "spatial parallel-line-set distance requires collinear carriers with overlapping spans separated by its length parameter"))?;
                }
            }
            SpatialConstraint::Offset {
                sources,
                results,
                distance,
                parameter,
                ..
            } => {
                let curves_match = all(ctx, sources.iter().chain(results), |entity| {
                    Ok(spatial_geometry
                        .get(ctx, entity.as_str())?
                        .map(|(_, geometry)| geometry)
                        .is_some_and(|geometry| {
                            matches!(
                                geometry.definition(),
                                SpatialSketchGeometryDefinition::Line { .. }
                                    | SpatialSketchGeometryDefinition::Circle { .. }
                                    | SpatialSketchGeometryDefinition::Arc { .. }
                                    | SpatialSketchGeometryDefinition::Nurbs { .. }
                            )
                        }))
                })?;
                let parameter_matches = match parameter {
                    None => true,
                    Some(parameter) => match parameter_values.get(ctx, parameter.id.as_str())? {
                        Some(Some(crate::features::ParameterValue::Length(value))) => {
                            let expected = if parameter.negated {
                                -value.get()
                            } else {
                                value.get()
                            };
                            let scale = 1.0 + expected.abs().max(distance.get());
                            (expected - distance.get()).abs()
                                <= EPS_SKETCHES_CHECK_SKETCHES_E9 * scale
                        }
                        _ => false,
                    },
                };
                if !curves_match {
                    record_finding(
                        ctx,
                        findings,
                        Check::GeometricConsistency,
                        crate::report::Severity::Error,
                        Some(constraint.id.as_str()),
                        format_args!(
                            "{}",
                            "spatial offset source and result members must be curves"
                        ),
                    )?;
                }
                if !parameter_matches {
                    record_finding(
                        ctx,
                        findings,
                        Check::GeometricConsistency,
                        crate::report::Severity::Error,
                        Some(constraint.id.as_str()),
                        format_args!("{}", "spatial offset distance does not match its parameter"),
                    )?;
                }
            }
            SpatialConstraint::ParallelToDirection { entity, direction } => {
                let Some(SpatialSketchGeometryDefinition::Line { start, end }) = spatial_geometry
                    .get(ctx, entity.as_str())?
                    .map(|(_, geometry)| geometry)
                    .map(|geometry| geometry.definition())
                else {
                    record_finding(
                        ctx,
                        findings,
                        Check::ReferentialIntegrity,
                        crate::report::Severity::Error,
                        Some(constraint.id.as_str()),
                        format_args!("{}", "spatial directional constraint requires a line"),
                    )?;
                    continue;
                };
                let line =
                    crate::math::Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
                let line_norm = line.norm();
                let direction = direction.as_raw();
                let cross = crate::math::Vector3::new(
                    line.y * direction.z - line.z * direction.y,
                    line.z * direction.x - line.x * direction.z,
                    line.x * direction.y - line.y * direction.x,
                );
                if !line_norm.is_finite()
                    || line_norm <= EPS_SKETCHES_CHECK_SKETCHES_E12
                    || cross.norm() > EPS_SKETCHES_CHECK_SKETCHES_E9 * line_norm
                {
                    record_finding(
                        ctx,
                        findings,
                        Check::GeometricConsistency,
                        crate::report::Severity::Error,
                        Some(constraint.id.as_str()),
                        format_args!(
                            "{}",
                            "spatial line is not parallel to its constraint direction"
                        ),
                    )?;
                }
            }
            SpatialConstraint::Coincident { .. }
            | SpatialConstraint::Midpoint { .. }
            | SpatialConstraint::PointOnSurface { .. }
            | SpatialConstraint::Tangent { .. }
            | SpatialConstraint::SplineGroup { .. } => {}
        }
    }

    for constraint in &ir.model.sketch_constraints {
        ctx.charge_work(1, "planar constraint scan")?;
        let valid = match constraint.definition.kind() {
            Constraint::TextFrame { text, frame } => {
                matches!(
                    geometry
                        .get(ctx, text.as_str())?
                        .map(|geometry| geometry.definition()),
                    Some(SketchGeometryDefinition::Text { .. })
                ) && all(ctx, frame, |entity| {
                    Ok(geometry.get(ctx, entity.as_str())?.is_some_and(|geometry| {
                        !matches!(geometry.definition(), SketchGeometryDefinition::Text { .. })
                    }))
                })?
            }
            Constraint::TextPath { text, path, .. } => {
                matches!(
                    geometry
                        .get(ctx, text.as_str())?
                        .map(|geometry| geometry.definition()),
                    Some(SketchGeometryDefinition::Text { .. })
                ) && geometry.get(ctx, path.as_str())?.is_some_and(|geometry| {
                    !matches!(
                        geometry.definition(),
                        SketchGeometryDefinition::Point { .. }
                            | SketchGeometryDefinition::Text { .. }
                    )
                })
            }
            Constraint::EqualDistance { first, second } => {
                let measured_distance =
                    |pair: &SketchDistancePair| -> Result<Option<f64>, CodecError> {
                        let Some(first) = sketch_locus_point(ctx, &pair.first, &geometry)? else {
                            return Ok(None);
                        };
                        let Some(second) = sketch_locus_point(ctx, &pair.second, &geometry)? else {
                            return Ok(None);
                        };
                        Ok(Some(distance2(first, second)))
                    };
                measured_distance(first)?
                    .zip(measured_distance(second)?)
                    .is_none_or(|(first, second)| {
                        (first - second).abs()
                            <= ir
                                .tolerances
                                .linear
                                .get()
                                .max(EPS_EQUAL_DISTANCE * (1.0 + first.abs().max(second.abs())))
                    })
            }
            Constraint::DistanceLociValue {
                first,
                second,
                distance,
                parameter,
            } => {
                let measured_points = sketch_locus_point(ctx, first, &geometry)?
                    .zip(sketch_locus_point(ctx, second, &geometry)?);
                let distance_matches =
                    measured_points.as_ref().is_none_or(|(first, second)| {
                        let measured = distance2(*first, *second);
                        (measured - distance.get()).abs()
                            <= ir.tolerances.linear.get().max(
                                EPS_DISTANCE_VALUE * (1.0 + measured.abs().max(distance.get())),
                            )
                    });
                let parameter_matches =
                    match parameter.as_ref() {
                        None => true,
                        Some(parameter) => match parameter_values.get(ctx, parameter.as_str())? {
                            Some(Some(crate::features::ParameterValue::Length(value))) => {
                                let expected = value.get().abs();
                                (expected - distance.get()).abs()
                                    <= ir.tolerances.linear.get().max(
                                        EPS_DISTANCE_VALUE * (1.0 + expected.max(distance.get())),
                                    )
                            }
                            _ => false,
                        },
                    };
                distance_matches && parameter_matches
            }
            Constraint::PointCoordinateValues { point, values } => {
                sketch_locus_point(ctx, point, &geometry)?.is_none_or(|point| {
                    [point.u, point.v]
                        .into_iter()
                        .zip(values)
                        .all(|(measured, expected)| {
                            (measured - expected.get()).abs()
                                <= ir.tolerances.linear.get().max(
                                    EPS_COORDINATE_VALUE
                                        * (1.0 + measured.abs().max(expected.get().abs())),
                                )
                        })
                })
            }
            Constraint::MidpointCoordinate {
                first,
                second,
                axis,
                value,
            } => sketch_locus_point(ctx, first, &geometry)?
                .zip(sketch_locus_point(ctx, second, &geometry)?)
                .is_none_or(|(first, second)| {
                    let measured = match axis {
                        crate::sketches::SketchCoordinateAxis::U => {
                            f64::midpoint(first.u, second.u)
                        }
                        crate::sketches::SketchCoordinateAxis::V => {
                            f64::midpoint(first.v, second.v)
                        }
                    };
                    (measured - value.get()).abs()
                        <= ir.tolerances.linear.get().max(
                            EPS_COORDINATE_VALUE * (1.0 + measured.abs().max(value.get().abs())),
                        )
                }),
            Constraint::PolarDistance {
                first,
                second,
                distance,
                angle,
                distance_parameter,
            } => {
                let measured_points = sketch_locus_point(ctx, first, &geometry)?
                    .zip(sketch_locus_point(ctx, second, &geometry)?);
                let distance_matches = measured_points.as_ref().is_none_or(|(first, second)| {
                    let measured = distance2(*first, *second);
                    (measured - distance.get()).abs()
                        <= ir
                            .tolerances
                            .linear
                            .get()
                            .max(EPS_POLAR_ANGLE * (1.0 + measured.abs().max(distance.get())))
                });
                let angle_matches = angle.as_ref().is_none_or(|angle| {
                    measured_points.as_ref().is_none_or(|(first, second)| {
                        let measured = (second.v - first.v).atan2(second.u - first.u);
                        let difference = (angle.get() - measured).rem_euclid(std::f64::consts::TAU);
                        difference.min(std::f64::consts::TAU - difference) <= EPS_POLAR_ANGLE
                    })
                });
                let parameter_matches = match distance_parameter.as_ref() {
                    None => true,
                    Some(parameter) => match parameter_values.get(ctx, parameter.as_str())? {
                        Some(Some(crate::features::ParameterValue::Length(value))) => {
                            let expected = value.get().abs();
                            (expected - distance.get()).abs()
                                <= ir
                                    .tolerances
                                    .linear
                                    .get()
                                    .max(EPS_POLAR_ANGLE * (1.0 + expected.max(distance.get())))
                        }
                        _ => false,
                    },
                };
                distance_matches && angle_matches && parameter_matches
            }
            Constraint::RepeatedLength { entities, .. } => {
                let lengths = Scratch::filter_map(ctx, entities, |entity| {
                    Ok(
                        match geometry
                            .get(ctx, entity.as_str())?
                            .map(|geometry| geometry.definition())
                        {
                            Some(SketchGeometryDefinition::Line { start, end }) => {
                                Some((end.u - start.u).hypot(end.v - start.v))
                            }
                            _ => None,
                        },
                    )
                })?;
                lengths.len() == entities.len()
                    && all(ctx, lengths[1..].iter(), |length| {
                        Ok((length - lengths[0]).abs()
                            <= ir.tolerances.linear.get().max(
                                EPS_SKETCHES_CHECK_SKETCHES_E9
                                    * (1.0 + length.abs().max(lengths[0].abs())),
                            ))
                    })?
            }
            Constraint::ParallelLineSetDistance {
                first,
                second,
                parameter,
            } => {
                let first_geometry = Scratch::filter_map(ctx, first, |entity| {
                    Ok(geometry.get(ctx, entity.as_str())?.copied())
                })?;
                let second_geometry = Scratch::filter_map(ctx, second, |entity| {
                    Ok(geometry.get(ctx, entity.as_str())?.copied())
                })?;
                let tolerance = ir.tolerances.linear.get();
                let first_collinear = match first_geometry.first() {
                    Some(reference) => all(ctx, first_geometry.iter(), |candidate| {
                        Ok(planar_parallel_line_distance(reference, candidate)
                            .is_some_and(|distance| distance.get() <= tolerance))
                    })?,
                    None => false,
                };
                let second_collinear = match second_geometry.first() {
                    Some(reference) => all(ctx, second_geometry.iter(), |candidate| {
                        Ok(planar_parallel_line_distance(reference, candidate)
                            .is_some_and(|distance| distance.get() <= tolerance))
                    })?,
                    None => false,
                };
                let measured = find_map(ctx, first_geometry.iter(), |first| {
                    find_map(ctx, second_geometry.iter(), |second| {
                        Ok(planar_parallel_line_span_distance(first, second, tolerance)
                            .map(crate::scalar::FiniteReal::get))
                    })
                })?;
                let expected = match parameter_values.get(ctx, parameter.as_str())? {
                    Some(Some(crate::features::ParameterValue::Length(length))) => {
                        Some(length.get().abs())
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
                first_geometry.len() == first.len()
                    && second_geometry.len() == second.len()
                    && first_collinear
                    && second_collinear
                    && measurement_matches
            }
            Constraint::RepeatedRadius { entities, .. }
            | Constraint::RepeatedDiameter { entities, .. } => {
                let radii = Scratch::filter_map(ctx, entities, |entity| {
                    Ok(
                        match geometry
                            .get(ctx, entity.as_str())?
                            .map(|geometry| geometry.definition())
                        {
                            Some(
                                SketchGeometryDefinition::Circle { radius, .. }
                                | SketchGeometryDefinition::Arc { radius, .. },
                            ) => Some(radius.get()),
                            _ => None,
                        },
                    )
                })?;
                radii.len() == entities.len()
                    && all(ctx, radii[1..].iter(), |radius| {
                        Ok((radius - radii[0]).abs()
                            <= ir.tolerances.linear.get().max(
                                EPS_SKETCHES_CHECK_SKETCHES_E9
                                    * (1.0 + radius.abs().max(radii[0].abs())),
                            ))
                    })?
            }
            Constraint::Disabled {}
            | Constraint::Coincident { .. }
            | Constraint::Polygon { .. }
            | Constraint::SplineGroup { .. }
            | Constraint::RectangularPattern { .. }
            | Constraint::CircularPattern { .. }
            | Constraint::CoincidentLoci { .. }
            | Constraint::SameCoordinate { .. }
            | Constraint::PointOnObject { .. }
            | Constraint::Midpoint { .. }
            | Constraint::Offset { .. }
            | Constraint::ProjectedCopy { .. }
            | Constraint::AtIntersection { .. }
            | Constraint::Concentric { .. }
            | Constraint::Coradial { .. }
            | Constraint::Collinear { .. }
            | Constraint::Symmetric { .. }
            | Constraint::PointSymmetric { .. }
            | Constraint::Horizontal { .. }
            | Constraint::Vertical { .. }
            | Constraint::Parallel { .. }
            | Constraint::Perpendicular { .. }
            | Constraint::Tangent { .. }
            | Constraint::TangentLoci { .. }
            | Constraint::Curvature { .. }
            | Constraint::Equal { .. }
            | Constraint::Fixed { .. }
            | Constraint::ArcAngle { .. }
            | Constraint::EllipseAngle { .. }
            | Constraint::Distance { .. }
            | Constraint::DistanceLoci { .. }
            | Constraint::AngleDifference { .. }
            | Constraint::ScalarEquality { .. }
            | Constraint::HorizontalDistance { .. }
            | Constraint::VerticalDistance { .. }
            | Constraint::RepeatedDistance { .. }
            | Constraint::Angle { .. }
            | Constraint::AngleToAxis { .. }
            | Constraint::Radius { .. }
            | Constraint::Diameter { .. }
            | Constraint::SnellsLaw { .. }
            | Constraint::Weight { .. }
            | Constraint::InternalAlignment { .. }
            | Constraint::Group { .. }
            | Constraint::Text { .. }
            | Constraint::Native { .. } => true,
        };
        if !valid {
            record_finding(
                ctx,
                findings,
                Check::Counts,
                crate::report::Severity::Error,
                Some(constraint.id.as_str()),
                format_args!("{}", "invalid sketch constraint arity"),
            )?;
        }
        if let Constraint::PointOnObject { point: _, entity } = constraint.definition.kind() {
            if geometry.get(ctx, entity.as_str())?.is_some_and(|geometry| {
                matches!(
                    geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                )
            }) {
                record_finding(
                    ctx,
                    findings,
                    Check::GeometricConsistency,
                    crate::report::Severity::Error,
                    Some(constraint.id.as_str()),
                    format_args!("{}", "point-on-object support is itself a point"),
                )?;
            }
        }
        if let Some(message) =
            constraint_entity_kind_refusal(ctx, constraint.definition.kind(), &geometry)?
        {
            record_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                crate::report::Severity::Error,
                Some(constraint.id.as_str()),
                format_args!("{message}"),
            )?;
        }
        visit_constraint_loci(ctx, constraint.definition.kind(), |locus| {
            let Some(entity_geometry) = geometry.get(ctx, locus_entity(locus).as_str())? else {
                return Ok(());
            };
            let valid = match locus {
                SketchLocus::Entity(_) => true,
                SketchLocus::Start(_) | SketchLocus::End(_) => !matches!(
                    entity_geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                        | SketchGeometryDefinition::Circle { .. }
                ),
                SketchLocus::Center(_) => matches!(
                    entity_geometry.definition(),
                    SketchGeometryDefinition::Circle { .. }
                        | SketchGeometryDefinition::Arc { .. }
                        | SketchGeometryDefinition::Ellipse { .. }
                        | SketchGeometryDefinition::ExternalReference { .. }
                        | SketchGeometryDefinition::Native { .. }
                ),
            };
            if !valid {
                record_finding(
                    ctx,
                    findings,
                    Check::GeometricConsistency,
                    crate::report::Severity::Error,
                    Some(constraint.id.as_str()),
                    format_args!(
                        "{}",
                        "sketch constraint locus is incompatible with its entity"
                    ),
                )?;
            }
            Ok(())
        })?;
        if let Constraint::Offset {
            pairs, distance, ..
        } = constraint.definition.kind()
        {
            for pair in ctx.admit_iter(pairs, "sketch constraint pair scan")? {
                let valid = match geometry
                    .get(ctx, pair.source.as_str())?
                    .zip(geometry.get(ctx, pair.result.as_str())?)
                {
                    Some((source, result)) => {
                        let expected = if pair.source_reversed {
                            -distance.get()
                        } else {
                            distance.get()
                        };
                        sketch_curve_offset_matches(
                            ctx,
                            source,
                            result,
                            expected,
                            ir.tolerances.linear.get(),
                        )?
                    }
                    None => true,
                };
                if !valid {
                    record_finding(
                        ctx,
                        findings,
                        Check::GeometricConsistency,
                        crate::report::Severity::Error,
                        Some(constraint.id.as_str()),
                        format_args!(
                            "{}",
                            "sketch offset pair does not match its oriented distance"
                        ),
                    )?;
                }
            }
        }
        if let Constraint::ProjectedCopy { source, result } = constraint.definition.kind() {
            let valid = match geometry
                .get(ctx, source.as_str())?
                .zip(geometry.get(ctx, result.as_str())?)
            {
                Some((source, result)) => equality::geometry_equal(ctx, source, result)?,
                None => true,
            };
            if !valid {
                record_finding(
                    ctx,
                    findings,
                    Check::GeometricConsistency,
                    crate::report::Severity::Error,
                    Some(constraint.id.as_str()),
                    format_args!(
                        "{}",
                        "projected-copy entities do not have identical geometry"
                    ),
                )?;
            }
        }
    }
    Ok(())
}

/// Refuse a constraint whose restricted entity has a neutral geometry of a kind
/// the constraint does not admit. An absent entity is a referential finding of
/// its own.
fn constraint_entity_kind_refusal(
    ctx: &DecodeContext<'_>,
    definition: &Constraint,
    geometry: &BorrowedIdentities<'_, '_, &SketchGeometry>,
) -> Result<Option<&'static str>, CodecError> {
    let Some((entity, restriction)) = definition.entity_kind_restriction() else {
        return Ok(None);
    };
    let Some(geometry) = geometry.get(ctx, entity.as_str())? else {
        return Ok(None);
    };
    if restriction.admits(geometry.definition()) {
        return Ok(None);
    }
    Ok(Some(match restriction {
        SketchEntityKindRestriction::BoundedCurve => {
            "sketch midpoint constraint references an entity that is not a bounded curve"
        }
        SketchEntityKindRestriction::CircularArc => {
            "sketch arc-angle constraint references an entity that is not a circular arc"
        }
        SketchEntityKindRestriction::BoundedEllipse => {
            "sketch ellipse-angle constraint references an entity that is not a bounded ellipse"
        }
    }))
}

fn distance2(left: crate::math::Point2, right: crate::math::Point2) -> f64 {
    (left.u - right.u).hypot(left.v - right.v)
}

struct PlanarParallelLines {
    first: [crate::math::Point2; 2],
    second: [crate::math::Point2; 2],
    distance: crate::scalar::FiniteReal,
}

fn planar_parallel_line_distance(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<crate::scalar::FiniteReal> {
    planar_parallel_lines(first, second).map(|lines| lines.distance)
}

fn planar_parallel_lines(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<PlanarParallelLines> {
    let (
        SketchGeometryDefinition::Line {
            start: first_start,
            end: first_end,
        },
        SketchGeometryDefinition::Line {
            start: second_start,
            end: second_end,
        },
    ) = (first.definition(), second.definition())
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
    let first_unit = crate::features::FiniteVector3::new(crate::math::Vector3::new(
        first_direction.u,
        first_direction.v,
        0.0,
    ))?
    .unit_nonzero()?;
    let second_unit = crate::features::FiniteVector3::new(crate::math::Vector3::new(
        second_direction.u,
        second_direction.v,
        0.0,
    ))?
    .unit_nonzero()?;
    if first_unit.cross(second_unit).norm() > EPS_SKETCHES_PLANAR_PARALLEL_LINE_DISTANCE_E9 {
        return None;
    }
    let distance = crate::math::sum::finite_dot(
        [
            second_start.u,
            -first_start.u,
            second_start.v,
            -first_start.v,
        ],
        [first_unit.y, first_unit.y, -first_unit.x, -first_unit.x],
    )
    .ok()?
    .abs();
    Some(PlanarParallelLines {
        first: [first_start.get(), first_end.get()],
        second: [second_start.get(), second_end.get()],
        distance,
    })
}

fn planar_parallel_line_span_distance(
    first: &SketchGeometry,
    second: &SketchGeometry,
    linear_tolerance: f64,
) -> Option<crate::scalar::FiniteReal> {
    let lines = planar_parallel_lines(first, second)?;
    let [first_start, first_end] = lines.first;
    let [second_start, second_end] = lines.second;
    let direction =
        crate::math::Point2::new(first_end.u - first_start.u, first_end.v - first_start.v);
    let unit = crate::features::FiniteVector3::new(crate::math::Vector3::new(
        direction.u,
        direction.v,
        0.0,
    ))?
    .unit_nonzero()?;
    let project = |point: crate::math::Point2| {
        crate::math::sum::finite_dot(
            [point.u, -first_start.u, point.v, -first_start.v],
            [unit.x, unit.x, unit.y, unit.y],
        )
        .ok()
    };
    let first_interval = [0.0, project(first_end)?.get()];
    let second_interval = [project(second_start)?.get(), project(second_end)?.get()];
    let first_min = first_interval[0].min(first_interval[1]);
    let first_max = first_interval[0].max(first_interval[1]);
    let second_min = second_interval[0].min(second_interval[1]);
    let second_max = second_interval[0].max(second_interval[1]);
    let lower = first_min.max(second_min);
    let upper = first_max.min(second_max);
    (lower <= upper || lower - upper <= linear_tolerance).then_some(lines.distance)
}

fn oriented_endpoints(
    geometry: &SketchGeometry,
    reversed: bool,
) -> Option<(crate::math::Point2, crate::math::Point2)> {
    let endpoints = match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => (start.get(), end.get()),
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => (
            circular_point(center.get(), radius.get(), start_angle.get()),
            circular_point(center.get(), radius.get(), end_angle.get()),
        ),
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            bounds: Some([start, end]),
        } => (
            ellipse_point(
                center.get(),
                major_angle.get(),
                radii.major().get(),
                radii.minor().get(),
                start.get(),
            ),
            ellipse_point(
                center.get(),
                major_angle.get(),
                radii.major().get(),
                radii.minor().get(),
                end.get(),
            ),
        ),
        SketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => {
            let points = curve.pole_rows();
            (
                points.point_at(0)?.get(),
                points.point_at(points.count().checked_sub(1)?)?.get(),
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

pub(super) fn locus_entity(locus: &SketchLocus) -> &crate::sketches::SketchEntityId {
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity,
    }
}

fn sketch_locus_point(
    ctx: &DecodeContext<'_>,
    locus: &SketchLocus,
    geometry: &BorrowedIdentities<'_, '_, &SketchGeometry>,
) -> Result<Option<crate::math::Point2>, CodecError> {
    let Some(entity_geometry) = geometry.get(ctx, locus_entity(locus).as_str())? else {
        return Ok(None);
    };
    Ok(match locus {
        SketchLocus::Entity(_) => match entity_geometry.definition() {
            SketchGeometryDefinition::Point { position } => Some(position.get()),
            _ => None,
        },
        SketchLocus::Start(_) | SketchLocus::End(_) => {
            let Some((start, end)) = oriented_endpoints(entity_geometry, false) else {
                return Ok(None);
            };
            Some(if matches!(locus, SketchLocus::Start(_)) {
                start
            } else {
                end
            })
        }
        SketchLocus::Center(_) => match entity_geometry.definition() {
            SketchGeometryDefinition::Circle { center, .. }
            | SketchGeometryDefinition::Arc { center, .. }
            | SketchGeometryDefinition::Ellipse { center, .. }
            | SketchGeometryDefinition::Hyperbola { center, .. } => Some(center.get()),
            SketchGeometryDefinition::Parabola { vertex, .. } => Some(vertex.get()),
            _ => None,
        },
    })
}

fn visit_constraint_loci<'definition>(
    ctx: &DecodeContext<'_>,
    definition: &'definition Constraint,
    mut visit: impl FnMut(&'definition SketchLocus) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    use crate::sketches::SketchDistanceMeasurement;
    match definition {
        Constraint::CoincidentLoci { loci: members }
        | Constraint::Group { elements: members }
        | Constraint::Text { elements: members, .. } => {
            let mut loci = members.iter();
            while loci.len() != 0 {
                ctx.charge_work(1, "sketch constraint locus scan")?;
                if let Some(locus) = loci.next() {
                    visit(locus)?;
                }
            }
        }
        Constraint::Midpoint { point, .. }
        | Constraint::PointOnObject { point, .. }
        | Constraint::PointCoordinateValues { point, .. } => visit(point)?,
        Constraint::Symmetric { first, second, .. }
        | Constraint::DistanceLoci { first, second, .. }
        | Constraint::DistanceLociValue { first, second, .. }
        | Constraint::MidpointCoordinate { first, second, .. }
        | Constraint::PolarDistance { first, second, .. }
        | Constraint::HorizontalDistance { first, second, .. }
        | Constraint::VerticalDistance { first, second, .. } => {
            visit(first)?;
            visit(second)?;
        }
        Constraint::EqualDistance { first, second } => {
            for locus in [&first.first, &first.second, &second.first, &second.second] {
                visit(locus)?;
            }
        }
        Constraint::RepeatedDistance { measurements, .. } => {
            let mut measurements = measurements.iter();
            while measurements.len() != 0 {
                ctx.charge_work(1, "sketch distance measurement scan")?;
                if let Some(measurement) = measurements.next() {
                    let (first, second) = match measurement {
                        SketchDistanceMeasurement::Distance { first, second }
                        | SketchDistanceMeasurement::Horizontal { first, second }
                        | SketchDistanceMeasurement::Vertical { first, second } => (first, second),
                    };
                    visit(first)?;
                    visit(second)?;
                }
            }
        }
        Constraint::SnellsLaw { incident, refracted, .. } => {
            visit(incident)?;
            visit(refracted)?;
        }
        _ => {}
    }
    Ok(())
}

fn visit_spatial_constraint_entities<'definition, U, L>(
    ctx: &DecodeContext<'_>,
    definition: &'definition SpatialConstraint<U, L>,
    mut visit: impl FnMut(&'definition crate::sketches::SpatialSketchEntityId) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    match definition {
        SpatialConstraint::Native { .. } => {}
        SpatialConstraint::SplineGroup { entities: members }
        | SpatialConstraint::RepeatedLineLength { entities: members, .. } => {
            let mut members = members.iter();
            while members.len() != 0 {
                ctx.charge_work(1, "spatial constraint member scan")?;
                if let Some(member) = members.next() {
                    visit(member)?;
                }
            }
        }
        SpatialConstraint::Coincident { first, second }
        | SpatialConstraint::Tangent { first, second }
        | SpatialConstraint::PointDistance { first, second, .. }
        | SpatialConstraint::ParallelLineDistance { first, second, .. } => {
            visit(first)?;
            visit(second)?;
        }
        SpatialConstraint::PointLineDistance { point, line, .. } => {
            visit(point)?;
            visit(line)?;
        }
        SpatialConstraint::LineLength { entity, .. }
        | SpatialConstraint::ParallelToDirection { entity, .. } => visit(entity)?,
        SpatialConstraint::RepeatedParallelLineDistance { pairs, .. } => {
            let mut pairs = pairs.iter();
            while pairs.len() != 0 {
                ctx.charge_work(1, "spatial constraint pair scan")?;
                if let Some(pair) = pairs.next() {
                    visit(&pair.first)?;
                    visit(&pair.second)?;
                }
            }
        }
        SpatialConstraint::ParallelLineSetDistance { first, second, .. }
        | SpatialConstraint::Offset { sources: first, results: second, .. } => {
            for members in [first, second] {
                let mut members = members.iter();
                while members.len() != 0 {
                    ctx.charge_work(1, "spatial constraint member scan")?;
                    if let Some(member) = members.next() {
                        visit(member)?;
                    }
                }
            }
        }
        SpatialConstraint::Symmetric { first, second, axis } => {
            visit(first)?;
            visit(second)?;
            visit(axis)?;
        }
        SpatialConstraint::Midpoint { point, entity } => {
            visit(point)?;
            visit(entity)?;
        }
        SpatialConstraint::PointOnSurface { point, surface } => {
            visit(point)?;
            visit(surface)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
