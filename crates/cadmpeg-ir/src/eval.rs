// SPDX-License-Identifier: Apache-2.0
//! Point evaluation of geometry carriers.
//!
//! Evaluators map carrier parameters to model-space (or parameter-space)
//! points using the carriers' own parameterizations: conic parameters are
//! angles from the reference/major direction, line parameters are signed
//! distances along the unit direction, and B-splines evaluate by Cox–de Boor
//! over their stored knot vectors. [`model_surface_point`] resolves construction-
//! backed carriers that require other model entities. Carriers without a typed
//! parameterization ([`SolvedCurveGeometry::Unknown`], [`SolvedCurveGeometry::Composite`],
//! [`SolvedSurfaceGeometry::Unknown`]) have no value.
//! [`model_curve_point_by_id`] resolves construction-backed curves whose
//! parameterization is established by model entities.

use cadmpeg_core::convert::f64_from_index;
use std::borrow::Cow;
use std::cmp::Ordering;

use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::nurbs::bezier::{homogeneous_spans, positive_controls};
use crate::geometry::nurbs::bounds::speed_bound_by;
use crate::geometry::nurbs::scoped::ScopedRows;
use crate::geometry::{
    nurbs::{NurbsCurve, NurbsPoleGrid, NurbsSurface, SurfaceParameterAxis},
    pcurve::PcurveGeometry,
    CurveGeometry, LawExpression, LawFormula, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    SweepSurfaceLayout,
};
use crate::math::solve::least_squares_step;
use crate::math::sum::{scaled_ratio_products, ExactSignedSum};
use crate::math::{product_quotient, scaled_sinh_cosh};
use crate::math::{Point2, Point3, Vector3};
use crate::scalar::{
    ExtendedReal, FiniteReal, Length, NonNegativeLength, NonNegativeReal, NonZeroLength,
    NonZeroReal, PositiveReal, SegmentPosition, UnitCosine,
};
use crate::topology::{IncreasingParameterInterval, ParameterInterval};
use crate::transform::Transform;
use crate::units::{FinitePoint2, FiniteVector, UnitVector3};
use crate::CadIr;
use cadmpeg_core::decode::{
    u64_from_index, DecodeContext, ResourceLimit, ScopedReservation, WorkBudget,
};
use cadmpeg_core::CodecError;

/// Resource policies for geometry evaluation.
pub mod admission;

/// Evaluation under the caller decode resource limits.
pub mod decode;

mod basis;
mod bezier;
mod depth;
mod model_surface_point;
mod polyline;
mod priority_queue;
mod rational;
mod sketch_offset;
mod sweep_law;
mod surface_request;
#[cfg(test)]
mod test_support;
use basis::fill_bspline_basis;
use depth::{ModelEvaluationDepthGuard, ModelEvaluationIdentity};
use polyline::polyline_point;
use priority_queue::PriorityQueue;
use rational::{finite_lanes, Homogeneous};
use sketch_offset::{clamped_nurbs_pcurve_endpoint_frames, fitted_nurbs_offset_candidate};
use surface_request::{HigherPartials, RequestedJet, SurfaceRequest};

const DEFAULT_NURBS_SURFACE_INVERSION_WORK: usize = 1_000_000;

const EPS_EVAL_SPATIAL_POINTS_ARE_REFLECTIONS_E12: f64 = 1.0e-12;
const EPS_EVAL_SPATIAL_POINTS_ARE_REFLECTIONS_E9: f64 = 1.0e-9;
const EPS_EVAL_REFINE_NURBS_SURFACE_PARAMETERS_E12: f64 = 1.0e-12;
const EPS_EVAL_MODEL_CURVE_PARAMETER_NEAR_POINT_WITH_TOLERANCE_E12: f64 = 1.0e-12;
const EPS_EVAL_SWEEP_PROFILE_FRAME_ALIGNMENT_E9: f64 = 1.0e-9;

/// Test whether two model-space points are reflections across a line carrier.
///
/// The line is unbounded for the reflection operation but its two stored
/// endpoints must define a finite, nondegenerate direction.
pub fn spatial_points_are_reflections(
    first: Point3,
    second: Point3,
    axis_start: Point3,
    axis_end: Point3,
) -> bool {
    let axis = Vector3::new(
        axis_end.x - axis_start.x,
        axis_end.y - axis_start.y,
        axis_end.z - axis_start.z,
    );
    let axis_length = axis.norm();
    if !axis_length.is_finite() || axis_length <= EPS_EVAL_SPATIAL_POINTS_ARE_REFLECTIONS_E12 {
        return false;
    }
    let midpoint = Point3::new(
        first.x.midpoint(second.x),
        first.y.midpoint(second.y),
        first.z.midpoint(second.z),
    );
    let from_axis = Vector3::new(
        midpoint.x - axis_start.x,
        midpoint.y - axis_start.y,
        midpoint.z - axis_start.z,
    );
    let separation = Vector3::new(second.x - first.x, second.y - first.y, second.z - first.z);
    let scale = 1.0
        + axis_length
            .max(from_axis.norm())
            .max(separation.norm())
            .max(first.x.abs())
            .max(first.y.abs())
            .max(first.z.abs())
            .max(second.x.abs())
            .max(second.y.abs())
            .max(second.z.abs());
    if !scale.is_finite() {
        return false;
    }
    let Some(axis) = FiniteVector3::new(axis).and_then(FiniteVector3::unit_nonzero) else {
        return false;
    };
    let scaled =
        |vector: Vector3| Vector3::new(vector.x / scale, vector.y / scale, vector.z / scale);
    axis.cross(scaled(from_axis)).norm() <= EPS_EVAL_SPATIAL_POINTS_ARE_REFLECTIONS_E9
        && axis.dot(scaled(separation)).abs() <= EPS_EVAL_SPATIAL_POINTS_ARE_REFLECTIONS_E9
}

/// Recover native parameters for an analytic surface point. The parameters
/// are absent when a projection of the point is not finite, and on a cone at
/// its apex and where its angle is undefined.
pub fn analytic_surface_parameters_solved(
    geometry: &SolvedSurfaceGeometry,
    point: Point3,
) -> Option<FinitePoint2> {
    let components = |origin: Point3, axis: Vector3, reference: Vector3| {
        let project = |direction: Vector3| {
            crate::math::sum::finite_dot(
                [point.x, point.y, point.z, -origin.x, -origin.y, -origin.z],
                [
                    direction.x,
                    direction.y,
                    direction.z,
                    direction.x,
                    direction.y,
                    direction.z,
                ],
            )
            .ok()
        };
        let transverse = axis.cross(reference);
        (project(reference), project(transverse), project(axis))
    };
    let finite = ExtendedReal::from_finite;
    match geometry {
        SolvedSurfaceGeometry::Plane(plane_surface) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            let (u, v, _) = components(origin, *normal, *u_axis);
            Some(FinitePoint2::from_coordinates(u?, v?))
        }
        SolvedSurfaceGeometry::Cylinder(cylinder_surface) => {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let ref_direction = cylinder_surface.frame().reference().as_raw();
            let radius = NonZeroLength::from(cylinder_surface.radius());
            let (x, y, v) = components(origin, *axis, *ref_direction);
            let angle = finite(y?).over(radius).atan2(finite(x?).over(radius));
            Some(FinitePoint2::from_coordinates(angle, v?))
        }
        SolvedSurfaceGeometry::Cone(cone_surface) => {
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let ref_direction = cone_surface.frame().reference().as_raw();
            let radius = cone_surface.radius().get();
            let ratio = cone_surface.ratio().get();
            let half_angle = cone_surface.half_angle().get();
            let (x, y, v) = components(origin, *axis, *ref_direction);
            let (x, y, v) = (x?.get(), y?.get(), v?);
            let local_radius = radius + v.get() * half_angle.tan();
            if local_radius == 0.0 {
                return None;
            }
            // A section radius whose product with the ratio underflows to
            // zero leaves 0 / 0 at y = 0.
            let angle = FiniteReal::new((y / (local_radius * ratio)).atan2(x / local_radius))?;
            Some(FinitePoint2::from_coordinates(angle, v))
        }
        SolvedSurfaceGeometry::Sphere(sphere_surface) => {
            let center = sphere_surface.center().get();
            let axis = sphere_surface.frame().axis().as_raw();
            let ref_direction = sphere_surface.frame().reference().as_raw();
            let (x, y, z) = components(center, *axis, *ref_direction);
            let (x, y, z) = (x?, y?, z?);
            Some(FinitePoint2::from_coordinates(
                y.atan2(x),
                finite(z).atan2(ExtendedReal::hypot(x, y)),
            ))
        }
        SolvedSurfaceGeometry::Torus(torus_surface) => {
            let center = torus_surface.center().get();
            let axis = torus_surface.frame().axis().as_raw();
            let ref_direction = torus_surface.frame().reference().as_raw();
            let major_radius = torus_surface.major_radius();
            let minor_radius = torus_surface.minor_radius();
            let (x, y, z) = components(center, *axis, *ref_direction);
            let (x, y, z) = (x?, y?, z?);
            Some(FinitePoint2::from_coordinates(
                y.atan2(x),
                finite(z).over(minor_radius).atan2(
                    ExtendedReal::hypot(x, y)
                        .minus(major_radius.into())
                        .over(minor_radius),
                ),
            ))
        }
        _ => None,
    }
}

#[derive(Debug)]
struct RationalBezierSurfacePatch<'session> {
    u_domain: IncreasingParameterInterval,
    v_domain: IncreasingParameterInterval,
    u_degree: usize,
    v_degree: usize,
    controls: Vec<[f64; 4]>,
    _scratch: ScopedReservation<'session>,
}

#[derive(Debug)]
struct SurfacePatches<'ctx> {
    // Rows are ordered by u span, then by v span. Each u span has v_span_count rows.
    rows: Vec<RationalBezierSurfacePatch<'ctx>>,
    v_span_count: usize,
    _storage: ScopedReservation<'ctx>,
}

impl SurfacePatches<'_> {
    fn patch_at_parameters(
        &self,
        ctx: &DecodeContext<'_>,
        parameters: FinitePoint2,
    ) -> Result<Option<&RationalBezierSurfacePatch<'_>>, ResourceLimit> {
        let u_start = ctx.partition_point_limit(
            &self.rows,
            |patch| Ok(patch.u_domain.upper() < parameters.u),
            "IR surface segment patch search",
        )?;
        let Some(u_end) = u_start.checked_add(self.v_span_count) else {
            return Ok(None);
        };
        let Some(row) = self.rows.get(u_start..u_end) else {
            return Ok(None);
        };
        let v_index = ctx.partition_point_limit(
            row,
            |patch| Ok(patch.v_domain.upper() < parameters.v),
            "IR surface segment patch search",
        )?;
        Ok(row.get(v_index).filter(|patch| {
            patch.u_domain.lower() <= parameters.u
                && parameters.u <= patch.u_domain.upper()
                && patch.v_domain.lower() <= parameters.v
                && parameters.v <= patch.v_domain.upper()
        }))
    }
}

struct SurfacePatchQueueEntry<'session> {
    lower_bound: f64,
    diameter: f64,
    sequence: usize,
    patch: RationalBezierSurfacePatch<'session>,
}

impl PartialEq for SurfacePatchQueueEntry<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.sequence == other.sequence
    }
}

impl Eq for SurfacePatchQueueEntry<'_> {}

impl PartialOrd for SurfacePatchQueueEntry<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SurfacePatchQueueEntry<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // The queue is a max-heap. Reverse the lower-bound order so the patch
        // with the strongest minimum-distance promise is examined first.
        other
            .lower_bound
            .total_cmp(&self.lower_bound)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

fn rational_surface_patches_with_budget<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    surface: &NurbsSurface,
    budget: &WorkBudget<'_>,
) -> Result<Option<SurfacePatches<'ctx>>, ResourceLimit> {
    if !budget.charge() {
        ctx.charge_work_limit(0, "IR surface extraction completion")?;
        return Ok(None);
    }
    let Some(u_degree) = usize::try_from(surface.u_degree()).ok() else {
        return Ok(None);
    };
    let Some(v_degree) = usize::try_from(surface.v_degree()).ok() else {
        return Ok(None);
    };
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let Some(control_count) = u_count.checked_mul(v_count) else {
        return Ok(None);
    };
    if u_degree >= u_count || v_degree >= v_count {
        return Ok(None);
    }
    let Some(patch_control_count) = (u_degree + 1).checked_mul(v_degree + 1) else {
        return Ok(None);
    };
    let (_point_storage, mut points) = {
        let mut values = Vec::new();
        let reservation =
            ctx.reserve_temporary_vec(&mut values, control_count, "IR surface control points")?;
        (reservation, values)
    };
    let (_weight_storage, weights) = match surface.pole_grid() {
        NurbsPoleGrid::Polynomial { rows } => {
            for row in ctx.admit_iter(rows, "IR surface control row visit")? {
                points.extend(
                    ctx.admit_iter(row, "IR surface control point copy")?
                        .copied(),
                );
            }
            (None, None)
        }
        NurbsPoleGrid::Rational { rows } => {
            let storage;
            let mut weights = Vec::new();
            storage = ctx.reserve_temporary_vec(
                &mut weights,
                control_count,
                "IR surface control weights",
            )?;
            for row in ctx.admit_iter(rows, "IR surface control row visit")? {
                for pole in ctx.admit_iter(row, "IR surface control point copy")? {
                    points.push(pole.point);
                    ctx.charge_work_limit(1, "IR surface control weight copy")?;
                    weights.push(pole.weight.get());
                }
            }
            (Some(storage), Some(weights))
        }
    };
    let Some(homogeneous_controls) =
        positive_controls(ctx, &points, weights.as_deref(), "Bezier positive controls")?
    else {
        return Ok(None);
    };
    // Every row and column has the same active knot intervals.
    let Some(u_domains) = surface.u_knots().active_spans(ctx, u_degree, u_count)? else {
        return Ok(None);
    };
    let Some(v_domains) = surface.v_knots().active_spans(ctx, v_degree, v_count)? else {
        return Ok(None);
    };
    let (_u_span_storage, mut u_spans_by_v) = {
        let mut values = Vec::new();
        let reservation = ctx.reserve_temporary_vec(&mut values, v_count, "IR surface u spans")?;
        (reservation, values)
    };
    for v in 0..v_count {
        let (_row_storage, mut controls) = {
            let mut values = Vec::new();
            let reservation =
                ctx.reserve_temporary_vec(&mut values, u_count, "IR surface u row")?;
            (reservation, values)
        };
        controls.extend(
            ctx.admit_iter(0..u_count, "IR surface u row copy")?
                .map(|u| homogeneous_controls[u * v_count + v]),
        );
        let Some(spans) = homogeneous_spans(ctx, u_degree, surface.u_knots(), &controls)? else {
            return Ok(None);
        };
        ctx.charge_work_limit(1, "IR surface u span append")?;
        u_spans_by_v.push(spans);
    }
    if !ctx.all_by_limit(
        &u_spans_by_v,
        |spans| Ok(spans.len() == u_domains.len()),
        "IR surface u span count scan",
    )? {
        return Ok(None);
    }
    let storage;
    let mut patches = Vec::new();
    let Some(patch_count) = u_domains.len().checked_mul(v_domains.len()) else {
        return Ok(None);
    };
    storage = ctx.reserve_temporary_vec(&mut patches, patch_count, "IR surface patches")?;
    for (u_span, &u_domain) in u_domains.iter().enumerate() {
        ctx.charge_work_limit(1, "IR surface u domain visit")?;
        let (_v_span_storage, mut v_spans_by_u) = {
            let mut values = Vec::new();
            let reservation =
                ctx.reserve_temporary_vec(&mut values, u_degree + 1, "IR surface v spans")?;
            (reservation, values)
        };
        for u_control in 0..=u_degree {
            let (_row_storage, mut controls) = {
                let mut values = Vec::new();
                let reservation =
                    ctx.reserve_temporary_vec(&mut values, v_count, "IR surface v row")?;
                (reservation, values)
            };
            controls.extend(
                ctx.admit_iter(&u_spans_by_v, "IR surface v row copy")?
                    .map(|spans| spans[u_span].controls[u_control]),
            );
            let Some(spans) = homogeneous_spans(ctx, v_degree, surface.v_knots(), &controls)?
            else {
                return Ok(None);
            };
            ctx.charge_work_limit(1, "IR surface v span append")?;
            v_spans_by_u.push(spans);
        }
        if !ctx.all_by_limit(
            &v_spans_by_u,
            |spans| Ok(spans.len() == v_domains.len()),
            "IR surface v span count scan",
        )? {
            return Ok(None);
        }
        for (v_span, &v_domain) in ctx
            .admit_iter(&*v_domains, "IR surface v domain visit")?
            .enumerate()
        {
            let (control_scratch, mut controls) = {
                let mut values = Vec::new();
                let reservation = ctx.reserve_temporary_vec(
                    &mut values,
                    patch_control_count,
                    "IR surface patch controls",
                )?;
                (reservation, values)
            };
            for spans in ctx.admit_iter(&v_spans_by_u, "IR surface patch row visit")? {
                controls.extend(
                    ctx.admit_iter(&*spans[v_span].controls, "IR surface patch control copy")?
                        .copied(),
                );
            }
            ctx.charge_work_limit(1, "IR surface patch append")?;
            patches.push(RationalBezierSurfacePatch {
                u_domain,
                v_domain,
                u_degree,
                v_degree,
                controls,
                _scratch: control_scratch,
            });
        }
    }
    Ok((!patches.is_empty()).then_some(SurfacePatches {
        rows: patches,
        v_span_count: v_domains.len(),
        _storage: storage,
    }))
}

fn rational_surface_residual_patches<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    surface: &NurbsSurface,
    point: Point3,
    budget: &WorkBudget<'_>,
) -> Result<Option<SurfacePatches<'ctx>>, ResourceLimit> {
    if !point.is_finite() {
        return Ok(None);
    }
    let Some(mut patches) = rational_surface_patches_with_budget(ctx, surface, budget)? else {
        return Ok(None);
    };
    for patch in &mut patches.rows {
        if !budget.charge() {
            ctx.charge_work_limit(0, "IR surface residual completion")?;
            return Ok(None);
        }
        for control in ctx.admit_iter(&mut patch.controls, "IR surface residual control")? {
            for (axis, coordinate) in [point.x, point.y, point.z].into_iter().enumerate() {
                control[axis] -= control[3] * coordinate;
            }
        }
    }
    Ok(Some(patches))
}

/// Each temporary row and output control is bounded by the admitted patch control count.
fn rational_patch_parameter_segment<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    patch: &RationalBezierSurfacePatch<'_>,
    start: FinitePoint2,
    end: FinitePoint2,
) -> Result<Option<ScopedRows<'ctx, [f64; 4]>>, ResourceLimit> {
    let normalize = |value: FiniteReal, domain: IncreasingParameterInterval| {
        let [lower, upper] = domain.finite_endpoints();
        match value.segment_position(lower, upper) {
            SegmentPosition::Within(fraction) => Some(fraction.get()),
            SegmentPosition::Outside | SegmentPosition::Degenerate => None,
        }
    };
    let [start_u, start_v] = start.coordinates();
    let [end_u, end_v] = end.coordinates();
    let Some(u_range) = normalize(start_u, patch.u_domain).zip(normalize(end_u, patch.u_domain))
    else {
        return Ok(None);
    };
    let Some(v_range) = normalize(start_v, patch.v_domain).zip(normalize(end_v, patch.v_domain))
    else {
        return Ok(None);
    };
    let (_u_line_storage, mut u_lines) = {
        let mut values = Vec::new();
        let reservation = ctx.reserve_temporary_vec(
            &mut values,
            patch.v_degree + 1,
            "IR rational surface u lines",
        )?;
        (reservation, values)
    };
    for v in 0..=patch.v_degree {
        let (_control_storage, mut controls) = {
            let mut values = Vec::new();
            let reservation = ctx.reserve_temporary_vec(
                &mut values,
                patch.u_degree + 1,
                "IR rational surface u row",
            )?;
            (reservation, values)
        };
        controls.extend(
            ctx.admit_iter(0..=patch.u_degree, "IR rational surface u row copy")?
                .map(|u| patch.controls[u * (patch.v_degree + 1) + v]),
        );
        let Some(line) = bezier::restrict_homogeneous_bezier(ctx, &controls, u_range.0, u_range.1)?
        else {
            return Ok(None);
        };
        u_lines.push(line);
    }
    let (_restricted_storage, mut restricted) = {
        let mut values = Vec::new();
        let reservation = ctx.reserve_temporary_vec(
            &mut values,
            patch.u_degree + 1,
            "IR rational surface restricted rows",
        )?;
        (reservation, values)
    };
    let Some(first_line) = u_lines.first() else {
        return Ok(None);
    };
    for (u, _) in first_line.iter().enumerate().take(patch.u_degree + 1) {
        let (_control_storage, mut controls) = {
            let mut values = Vec::new();
            let reservation = ctx.reserve_temporary_vec(
                &mut values,
                patch.v_degree + 1,
                "IR rational surface v row",
            )?;
            (reservation, values)
        };
        controls.extend(
            ctx.admit_iter(&u_lines, "IR rational surface v row copy")?
                .map(|row| row[u]),
        );
        let Some(line) = bezier::restrict_homogeneous_bezier(ctx, &controls, v_range.0, v_range.1)?
        else {
            return Ok(None);
        };
        restricted.push(line);
    }
    let degree = patch.u_degree + patch.v_degree;
    let Some(count) = degree.checked_add(1) else {
        return Ok(None);
    };
    let (diagonal_storage, mut diagonal) = {
        let mut values = Vec::new();
        let reservation =
            ctx.reserve_temporary_vec(&mut values, count, "IR rational surface diagonal")?;
        (reservation, values)
    };
    diagonal.extend(
        ctx.admit_iter(0..count, "IR rational surface diagonal fill")?
            .map(|_| [0.0; 4]),
    );
    let mut u = 0;
    if !ctx.all_by_limit(
        &restricted,
        |row| {
            let mut v = 0;
            let valid = ctx.all_by_limit(
                row,
                |control| {
                    let index = u + v;
                    let (Some(u_factor), Some(v_factor), Some(denominator)) = (
                        bezier::binomial_coefficient(ctx, patch.u_degree, u)?,
                        bezier::binomial_coefficient(ctx, patch.v_degree, v)?,
                        bezier::binomial_coefficient(ctx, degree, index)?,
                    ) else {
                        return Ok(false);
                    };
                    let factor = u_factor * v_factor / denominator;
                    for axis in 0..4 {
                        diagonal[index][axis] += factor * control[axis];
                    }
                    v += 1;
                    Ok(true)
                },
                "IR rational surface diagonal coefficient",
            )?;
            u += 1;
            Ok(valid)
        },
        "IR rational surface diagonal row visit",
    )? {
        return Ok(None);
    }
    if !ctx.all_by_limit(
        &diagonal,
        |control| Ok(control.iter().all(|value| value.is_finite())),
        "IR rational surface diagonal finite scan",
    )? {
        return Ok(None);
    }
    Ok(Some(ScopedRows::new(diagonal, diagonal_storage)))
}

/// Conservatively bound the separation between a NURBS surface image of a
/// linear parameter segment and a model-space chord with the same parameter.
///
/// The segment is split at every surface knot. Each rational Bézier piece is
/// restricted exactly to the parameter line, and its positive-weight residual
/// control hull bounds the complete piece rather than selected samples.
pub fn nurbs_surface_parameter_segment_chord_bound(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    parameters: [Point2; 2],
    chord: [Point3; 2],
) -> Result<Option<f64>, CodecError> {
    let budget = ctx.work_budget(u64_from_index(DEFAULT_NURBS_SURFACE_INVERSION_WORK));
    nurbs_surface_parameter_segment_chord_bound_with_budget(
        ctx, surface, parameters, chord, &budget,
    )
}

/// Bound a surface segment with scratch charged to the work slice's session.
pub fn nurbs_surface_parameter_segment_chord_bound_with_budget(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    parameters: [Point2; 2],
    chord: [Point3; 2],
    budget: &WorkBudget<'_>,
) -> Result<Option<f64>, CodecError> {
    let _depth = ctx.enter_nested("IR surface segment depth")?;
    let result = (|| -> Result<Option<f64>, CodecError> {
        let [Some(first), Some(last)] = parameters.map(FinitePoint2::new) else {
            return Ok(None);
        };
        if chord.iter().any(|point| !point.is_finite()) {
            return Ok(None);
        }
        let Some(patches) = rational_surface_patches_with_budget(ctx, surface, budget)? else {
            return Ok(None);
        };
        let [first_u, first_v] = first.coordinates();
        let [last_u, last_v] = last.coordinates();
        let mut split_storage = ctx.reserve_scoped(0, "IR rational surface segment splits")?;
        let mut splits = Vec::new();
        let Some(split_capacity) = patches
            .rows
            .len()
            .checked_mul(4)
            .and_then(|count| count.checked_add(2))
        else {
            return Err(ctx.refuse_codec_limit(
                "IR rational surface segment splits",
                u64::MAX - 1,
                u64::MAX,
            ));
        };
        ctx.reserve_scoped_vec(
            &mut split_storage,
            &mut splits,
            split_capacity,
            "IR rational surface segment splits",
        )?;
        splits.extend([0.0, 1.0]);
        if !ctx.all_by(
            &patches.rows,
            |patch| {
                let [u_lower, u_upper] = patch.u_domain.finite_endpoints();
                let [v_lower, v_upper] = patch.v_domain.finite_endpoints();
                for (boundary, start, end) in [
                    (u_lower, first_u, last_u),
                    (u_upper, first_u, last_u),
                    (v_lower, first_v, last_v),
                    (v_upper, first_v, last_v),
                ] {
                    if start.get().min(end.get()) < boundary.get()
                        && boundary.get() < start.get().max(end.get())
                    {
                        let Some(parameter) =
                            finite_or_refusal(difference_quotient(boundary, start, end, start))?
                                .map(FiniteReal::get)
                        else {
                            return Ok(false);
                        };
                        if 0.0 < parameter && parameter < 1.0 {
                            splits.push(parameter);
                        }
                    }
                }
                Ok(true)
            },
            "IR surface segment boundary scan",
        )? {
            return Ok(None);
        }
        // Equal finite split parameters are indistinguishable before deduplication.
        ctx.sort_unstable_by(
            &mut splits,
            |value| value,
            f64::total_cmp,
            "IR surface segment split sort",
        )?;
        ctx.charge_work(
            u64_from_index(splits.len()),
            "IR surface segment split deduplication",
        )?;
        splits.dedup();
        let mut bound = 0.0_f64;
        if !ctx.all_by(
            splits.windows(2),
            |range| {
                let middle = 0.5 * (range[0] + range[1]);
                let parameter_point = |parameter: f64| {
                    Some(FinitePoint2::from_coordinates(
                        crate::math::interpolate(first.u, last.u, parameter)?,
                        crate::math::interpolate(first.v, last.v, parameter)?,
                    ))
                };
                let Some(midpoint) = parameter_point(middle) else {
                    return Ok(false);
                };
                let Some(patch) = patches.patch_at_parameters(ctx, midpoint)? else {
                    return Ok(false);
                };
                let Some(start) = parameter_point(range[0]) else {
                    return Ok(false);
                };
                let Some(end) = parameter_point(range[1]) else {
                    return Ok(false);
                };
                let Some(controls) = rational_patch_parameter_segment(ctx, patch, start, end)?
                else {
                    return Ok(false);
                };
                let Some(piece_bound) = bezier::rational_curve_chord_bound(
                    ctx,
                    &controls,
                    [
                        bezier::point_on_chord(chord, range[0]),
                        bezier::point_on_chord(chord, range[1]),
                    ],
                )?
                else {
                    return Ok(false);
                };
                bound = bound.max(piece_bound);
                Ok(true)
            },
            "IR surface segment interval scan",
        )? {
            return Ok(None);
        }
        Ok(Some(bound))
    })();
    ctx.charge_work(0, "IR surface segment completion")?;
    if budget.exhausted() {
        let limit = u64_from_index(budget.consumed());
        let requested = limit.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("IR surface segment work", u64::MAX - 1, u64::MAX)
        })?;
        return Err(ctx.refuse_codec_limit("IR surface segment work", limit, requested));
    }
    result
}

fn rational_patch_distance_bounds_with_budget(
    ctx: &DecodeContext<'_>,
    patch: &RationalBezierSurfacePatch<'_>,
    budget: &WorkBudget<'_>,
) -> Result<Option<(f64, f64)>, ResourceLimit> {
    if !budget.charge() {
        ctx.charge_work_limit(0, "IR surface distance bounds completion")?;
        return Ok(None);
    }
    let mut minimum = [f64::INFINITY; 3];
    let mut maximum = [f64::NEG_INFINITY; 3];
    if !ctx.all_by_limit(
        &patch.controls,
        |control| {
            if !control[3].is_finite() || control[3] <= 0.0 {
                return Ok(false);
            }
            for axis in 0..3 {
                let coordinate = control[axis] / control[3];
                if !coordinate.is_finite() {
                    return Ok(false);
                }
                minimum[axis] = minimum[axis].min(coordinate);
                maximum[axis] = maximum[axis].max(coordinate);
            }
            Ok(true)
        },
        "IR surface distance control scan",
    )? {
        return Ok(None);
    }
    let lower = (0..3)
        .map(|axis| {
            if minimum[axis] > 0.0 {
                minimum[axis]
            } else if maximum[axis] < 0.0 {
                -maximum[axis]
            } else {
                0.0
            }
        })
        .fold(0.0_f64, f64::hypot);
    let diameter = (0..3)
        .map(|axis| maximum[axis] - minimum[axis])
        .fold(0.0_f64, f64::hypot);
    Ok((lower.is_finite() && diameter.is_finite()).then_some((lower, diameter)))
}

fn split_rational_surface_patch<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    patch: &RationalBezierSurfacePatch<'_>,
    split_u: bool,
    budget: &WorkBudget<'_>,
) -> Result<Option<[RationalBezierSurfacePatch<'ctx>; 2]>, ResourceLimit> {
    let (degree, line_count) = if split_u {
        (patch.u_degree, patch.v_degree + 1)
    } else {
        (patch.v_degree, patch.u_degree + 1)
    };
    if !budget.charge() {
        ctx.charge_work_limit(0, "IR surface split completion")?;
        return Ok(None);
    }
    let (_first_line_storage, mut first_lines) = {
        let mut values = Vec::new();
        let reservation =
            ctx.reserve_temporary_vec(&mut values, line_count, "IR surface patch first lines")?;
        (reservation, values)
    };
    let (_second_line_storage, mut second_lines) = {
        let mut values = Vec::new();
        let reservation =
            ctx.reserve_temporary_vec(&mut values, line_count, "IR surface patch second lines")?;
        (reservation, values)
    };
    for line in 0..line_count {
        let (_row_storage, mut controls) = {
            let mut values = Vec::new();
            let reservation =
                ctx.reserve_temporary_vec(&mut values, degree + 1, "IR surface patch split line")?;
            (reservation, values)
        };
        if split_u {
            controls.extend(
                ctx.admit_iter(0..=degree, "IR surface patch split line copy")?
                    .map(|index| patch.controls[index * (patch.v_degree + 1) + line]),
            );
        } else {
            let row =
                &patch.controls[line * (patch.v_degree + 1)..(line + 1) * (patch.v_degree + 1)];
            controls.extend(
                ctx.admit_iter(row, "IR surface patch split line copy")?
                    .copied(),
            );
        }
        let Some(split) = bezier::split_homogeneous_bezier_midpoint(ctx, &controls)? else {
            return Ok(None);
        };
        let [first, second] = split.into_polygons()?;
        ctx.charge_work_limit(2, "IR surface split polygon append")?;
        first_lines.push(first);
        second_lines.push(second);
    }
    let assemble = |lines: &[ScopedRows<'ctx, [f64; 4]>]| -> Result<(Vec<[f64; 4]>, ScopedReservation<'ctx>), ResourceLimit> {
        let (reservation, mut controls) = {
            let mut values = Vec::new();
            let reservation = ctx.reserve_temporary_vec(&mut values, patch.controls.len(), "IR surface patch assembled controls")?;
            (reservation, values)
        };
        if split_u {
            for u in ctx.admit_iter(0..=patch.u_degree, "IR surface patch assembled row visit")? {
                controls.extend(ctx.admit_iter(lines, "IR surface patch assembled copy")?.map(|line| line[u]));
            }
        } else {
            for line in ctx.admit_iter(lines, "IR surface patch assembled row visit")? {
                controls.extend(ctx.admit_iter(&**line, "IR surface patch assembled copy")?.copied());
            }
        }
        Ok((controls, reservation))
    };
    let (first_u, second_u, first_v, second_v) = if split_u {
        let Some((first_u, second_u)) = patch.u_domain.split_at_midpoint() else {
            return Ok(None);
        };
        (first_u, second_u, patch.v_domain, patch.v_domain)
    } else {
        let Some((first_v, second_v)) = patch.v_domain.split_at_midpoint() else {
            return Ok(None);
        };
        (patch.u_domain, patch.u_domain, first_v, second_v)
    };
    let first = assemble(&first_lines)?;
    let second = assemble(&second_lines)?;
    Ok(Some([
        RationalBezierSurfacePatch {
            u_domain: first_u,
            v_domain: first_v,
            u_degree: patch.u_degree,
            v_degree: patch.v_degree,
            controls: first.0,
            _scratch: first.1,
        },
        RationalBezierSurfacePatch {
            u_domain: second_u,
            v_domain: second_v,
            u_degree: patch.u_degree,
            v_degree: patch.v_degree,
            controls: second.0,
            _scratch: second.1,
        },
    ]))
}

fn refine_nurbs_surface_parameters(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    point: Point3,
    start: FinitePoint2,
    u_domain: ParameterInterval,
    v_domain: ParameterInterval,
    budget: &WorkBudget<'_>,
) -> Result<Option<FinitePoint2>, ResourceLimit> {
    let distance = |position: Point3| position.distance(point);
    let [start_u, start_v] = start.coordinates();
    let mut parameters = FinitePoint2::from_coordinates(
        u_domain.project(ExtendedReal::from_finite(start_u)),
        v_domain.project(ExtendedReal::from_finite(start_v)),
    );
    for _ in 0..32 {
        let Some(partials) = finite_or_refusal(
            crate::eval::admission::EvaluationAdmission::Decode(ctx).within_work_slice(
                budget,
                |admission| {
                    crate::eval::nurbs_surface_partials(
                        admission,
                        surface,
                        parameters.u,
                        parameters.v,
                    )
                },
            ),
        )?
        else {
            return Ok(None);
        };
        let position = partials.point;
        let residual = Vector3::new(
            position.x - point.x,
            position.y - point.y,
            position.z - point.z,
        );
        let Some((step_u, step_v)) = least_squares_step(partials.du, partials.dv, residual) else {
            break;
        };
        let current_distance = distance(position.get());
        let [u, v] = parameters.coordinates();
        let mut scale = FiniteReal::ONE;
        let mut accepted = None;
        for _ in 0..16 {
            let candidate = FinitePoint2::from_coordinates(
                u_domain.project(ExtendedReal::stepped(u, scale, step_u)),
                v_domain.project(ExtendedReal::stepped(v, scale, step_v)),
            );
            let Some(candidate_position) = finite_or_refusal(
                crate::eval::admission::EvaluationAdmission::Decode(ctx).within_work_slice(
                    budget,
                    |admission| {
                        crate::eval::decode::nurbs_surface_point(
                            admission,
                            surface,
                            candidate.u,
                            candidate.v,
                        )
                    },
                ),
            )?
            else {
                return Ok(None);
            };
            if distance(candidate_position.get()) <= current_distance {
                accepted = Some(candidate);
                break;
            }
            scale = scale.halved();
        }
        let Some(candidate) = accepted else {
            break;
        };
        parameters = candidate;
        if scale.get() * step_u.get().abs()
            <= EPS_EVAL_REFINE_NURBS_SURFACE_PARAMETERS_E12 * (1.0 + parameters.u.abs())
            && scale.get() * step_v.get().abs()
                <= EPS_EVAL_REFINE_NURBS_SURFACE_PARAMETERS_E12 * (1.0 + parameters.v.abs())
        {
            break;
        }
    }
    Ok(Some(parameters))
}

fn nurbs_surface_evaluation_cost(surface: &NurbsSurface) -> Option<usize> {
    let (u_support, v_support) = nurbs_surface_support_sizes(surface)?;
    let control_work = u_support.checked_mul(v_support)?;
    control_work
        .checked_add(u_support.checked_mul(u_support)?)?
        .checked_add(v_support.checked_mul(v_support)?)
}

fn complete_nurbs_surface_starts<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<FinitePoint2>,
    fit_tolerance: Option<f64>,
    budget: &WorkBudget<'_>,
) -> Result<Option<ScopedRows<'ctx, FinitePoint2>>, ResourceLimit> {
    const MAX_PATCHES: usize = 1_000_000;

    let Some(patches) = rational_surface_residual_patches(ctx, surface, point, budget)? else {
        return Ok(None);
    };
    let mut coordinate_scale = 1.0_f64;
    if !ctx.all_by_limit(
        &patches.rows,
        |patch| {
            ctx.all_by_limit(
                &patch.controls,
                |control| {
                    let weight = control[3];
                    if !weight.is_finite() || weight <= 0.0 {
                        return Ok(false);
                    }
                    for coordinate in &control[..3] {
                        let coordinate = (coordinate / weight).abs();
                        if !coordinate.is_finite() {
                            return Ok(false);
                        }
                        coordinate_scale = coordinate_scale.max(coordinate);
                    }
                    Ok(true)
                },
                "IR surface inverse coordinate scale scan",
            )
        },
        "IR surface inverse patch scale scan",
    )? {
        return Ok(None);
    }
    let requested_tolerance = match fit_tolerance {
        Some(tolerance) if tolerance.is_finite() && tolerance >= 0.0 => tolerance,
        Some(_) => return Ok(None),
        None => 0.0,
    };
    let distance_tolerance = requested_tolerance.max(256.0 * f64::EPSILON * coordinate_scale);
    let distance_at = |parameters: FinitePoint2| -> Result<Option<f64>, ResourceLimit> {
        let position = match crate::eval::admission::EvaluationAdmission::Decode(ctx)
            .within_work_slice(budget, |admission| {
                crate::eval::decode::nurbs_surface_point(
                    admission,
                    surface,
                    parameters.u,
                    parameters.v,
                )
            }) {
            Ok(position) => position,
            Err(EvaluationFailure::ResourceLimit(limit)) => return Err(limit),
            Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(_)) => return Ok(None),
        };
        let distance = (position.x - point.x)
            .hypot(position.y - point.y)
            .hypot(position.z - point.z);
        Ok(distance.is_finite().then_some(distance))
    };
    let center = |patch: &RationalBezierSurfacePatch<'_>| {
        let [u_start, u_end] = patch.u_domain.finite_endpoints();
        let [v_start, v_end] = patch.v_domain.finite_endpoints();
        FinitePoint2::from_coordinates(u_start.midpoint(u_end), v_start.midpoint(v_end))
    };
    let Some(u_degree) = usize::try_from(surface.u_degree()).ok() else {
        return Ok(None);
    };
    let Some(v_degree) = usize::try_from(surface.v_degree()).ok() else {
        return Ok(None);
    };
    let Some(surface_u_domain) = surface.u_knots().span(u_degree, surface.u_count()) else {
        return Ok(None);
    };
    let Some(surface_v_domain) = surface.v_knots().span(v_degree, surface.v_count()) else {
        return Ok(None);
    };
    let refined_upper =
        |start, u_domain, v_domain| -> Result<Option<(FinitePoint2, f64)>, ResourceLimit> {
            let parameters = refine_nurbs_surface_parameters(
                ctx, surface, point, start, u_domain, v_domain, budget,
            )?
            .unwrap_or(start);
            Ok(distance_at(parameters)?.map(|distance| (parameters, distance)))
        };
    let mut best_distance = f64::INFINITY;
    let mut upper_scratch = ctx.reserve_scoped_limit(0, "IR surface upper parameters")?;
    let mut best_upper_parameters = Vec::new();
    {
        let mut consider_upper =
            |(parameters, distance): (FinitePoint2, f64)| -> Result<(), ResourceLimit> {
                if !best_distance.is_finite() {
                    best_distance = distance;
                    ctx.reserve_scoped_vec_limit(
                        &mut upper_scratch,
                        &mut best_upper_parameters,
                        1,
                        "IR surface upper parameters",
                    )?;
                    ctx.charge_work_limit(1, "IR surface upper parameter append")?;
                    best_upper_parameters.push(parameters);
                    return Ok(());
                }
                let tolerance = 128.0
                    * f64::EPSILON
                    * distance
                        .abs()
                        .max(best_distance.abs())
                        .max(distance_tolerance);
                if distance < best_distance && best_distance - distance > tolerance {
                    best_distance = distance;
                    best_upper_parameters.clear();
                }
                if (distance - best_distance).abs() <= tolerance {
                    ctx.reserve_scoped_vec_limit(
                        &mut upper_scratch,
                        &mut best_upper_parameters,
                        1,
                        "IR surface upper parameters",
                    )?;
                    ctx.charge_work_limit(1, "IR surface upper parameter append")?;
                    best_upper_parameters.push(parameters);
                }
                Ok(())
            };
        if let Some(seed) = seed {
            if let Some(candidate) = refined_upper(seed, surface_u_domain, surface_v_domain)? {
                consider_upper(candidate)?;
            }
        }
        if !ctx.all_by_limit(
            &patches.rows,
            |patch| {
                let Some(candidate) =
                    refined_upper(center(patch), patch.u_domain.into(), patch.v_domain.into())?
                else {
                    return Ok(false);
                };
                consider_upper(candidate)?;
                Ok(true)
            },
            "IR surface upper patch visit",
        )? {
            return Ok(None);
        }
    }
    if !best_distance.is_finite() {
        return Ok(None);
    }
    // A tolerance-bounded inverse needs a constructive fitting parameter, not
    // a proof of the global minimum. Every upper candidate is surface-evaluated.
    if fit_tolerance.is_some() && best_distance <= distance_tolerance {
        return Ok((!best_upper_parameters.is_empty())
            .then_some(ScopedRows::new(best_upper_parameters, upper_scratch)));
    }
    let mut queue = PriorityQueue::new(ctx)?;
    let mut sequence = 0usize;
    let SurfacePatches {
        rows,
        _storage: patch_storage,
        ..
    } = patches;
    for patch in rows {
        ctx.charge_work_limit(1, "IR surface queue patch visit")?;
        let Some((lower_bound, diameter)) =
            rational_patch_distance_bounds_with_budget(ctx, &patch, budget)?
        else {
            return Ok(None);
        };
        queue.push(SurfacePatchQueueEntry {
            lower_bound,
            diameter,
            sequence,
            patch,
        })?;
        sequence += 1;
    }
    drop(patch_storage);
    let mut terminal_scratch = ctx.reserve_scoped_limit(0, "IR surface terminal parameters")?;
    let mut terminal = Vec::<(FinitePoint2, f64)>::new();
    let mut examined = 0usize;
    while let Some(entry) = queue.pop()? {
        examined += 1;
        if examined > MAX_PATCHES || !budget.charge() {
            ctx.charge_work_limit(0, "IR surface search completion")?;
            return Ok(None);
        }
        let SurfacePatchQueueEntry {
            lower_bound,
            diameter,
            patch,
            ..
        } = entry;
        let comparison_tolerance = 128.0
            * f64::EPSILON
            * lower_bound
                .abs()
                .max(best_distance.abs())
                .max(distance_tolerance);
        if lower_bound > best_distance + comparison_tolerance {
            break;
        }
        let parameters = center(&patch);
        let Some((upper_parameters, center_distance)) =
            refined_upper(parameters, patch.u_domain.into(), patch.v_domain.into())?
        else {
            return Ok(None);
        };
        if fit_tolerance.is_some() && center_distance <= distance_tolerance {
            let (reservation, mut starts) = {
                let mut values = Vec::new();
                let reservation =
                    ctx.reserve_temporary_vec(&mut values, 1, "IR surface parameter start")?;
                (reservation, values)
            };
            ctx.charge_work_limit(1, "IR surface parameter start copy")?;
            starts.push(upper_parameters);
            return Ok(Some(ScopedRows::new(starts, reservation)));
        }
        let upper_tolerance = 128.0
            * f64::EPSILON
            * center_distance
                .abs()
                .max(best_distance.abs())
                .max(distance_tolerance);
        if center_distance < best_distance && best_distance - center_distance > upper_tolerance {
            best_distance = center_distance;
            best_upper_parameters.clear();
        }
        if (center_distance - best_distance).abs() <= upper_tolerance {
            ctx.reserve_scoped_vec_limit(
                &mut upper_scratch,
                &mut best_upper_parameters,
                1,
                "IR surface upper parameters",
            )?;
            ctx.charge_work_limit(1, "IR surface upper parameter append")?;
            best_upper_parameters.push(upper_parameters);
        }
        let indivisible = parameters.u == patch.u_domain.lower()
            || parameters.u == patch.u_domain.upper()
            || parameters.v == patch.v_domain.lower()
            || parameters.v == patch.v_domain.upper();
        if diameter <= distance_tolerance
            || center_distance - lower_bound <= distance_tolerance
            || indivisible
        {
            ctx.reserve_scoped_vec_limit(
                &mut terminal_scratch,
                &mut terminal,
                1,
                "IR surface terminal parameters",
            )?;
            ctx.charge_work_limit(1, "IR surface terminal parameter append")?;
            terminal.push((upper_parameters, lower_bound));
            continue;
        }
        let control = |u: usize, v: usize| {
            let homogeneous = patch.controls[u * (patch.v_degree + 1) + v];
            [
                homogeneous[0] / homogeneous[3],
                homogeneous[1] / homogeneous[3],
                homogeneous[2] / homogeneous[3],
            ]
        };
        let mut u_variation = 0.0_f64;
        for u in ctx.admit_iter(0..patch.u_degree, "IR surface u variation row visit")? {
            for v in ctx.admit_iter(0..=patch.v_degree, "IR surface u variation scan")? {
                let first = control(u, v);
                let second = control(u + 1, v);
                let variation = (0..3)
                    .map(|axis| second[axis] - first[axis])
                    .fold(0.0_f64, f64::hypot);
                u_variation = u_variation.max(variation);
            }
        }
        let mut v_variation = 0.0_f64;
        for u in ctx.admit_iter(0..=patch.u_degree, "IR surface v variation row visit")? {
            for v in ctx.admit_iter(0..patch.v_degree, "IR surface v variation scan")? {
                let first = control(u, v);
                let second = control(u, v + 1);
                let variation = (0..3)
                    .map(|axis| second[axis] - first[axis])
                    .fold(0.0_f64, f64::hypot);
                v_variation = v_variation.max(variation);
            }
        }
        let Some(children) =
            split_rational_surface_patch(ctx, &patch, u_variation >= v_variation, budget)?
        else {
            return Ok(None);
        };
        for patch in children {
            let Some((lower_bound, diameter)) =
                rational_patch_distance_bounds_with_budget(ctx, &patch, budget)?
            else {
                return Ok(None);
            };
            queue.push(SurfacePatchQueueEntry {
                lower_bound,
                diameter,
                sequence,
                patch,
            })?;
            sequence += 1;
        }
    }
    let final_tolerance = 128.0 * f64::EPSILON * best_distance.abs().max(distance_tolerance);
    let Some(start_count) = terminal.len().checked_add(best_upper_parameters.len()) else {
        return Ok(None);
    };
    let (start_scratch, mut starts) = {
        let mut values = Vec::new();
        let reservation =
            ctx.reserve_temporary_vec(&mut values, start_count, "IR surface parameter starts")?;
        (reservation, values)
    };
    for &(parameters, lower) in ctx.admit_iter(&terminal, "IR surface terminal parameter scan")? {
        if lower <= best_distance + final_tolerance {
            ctx.charge_work_limit(1, "IR surface terminal parameter copy")?;
            starts.push(parameters);
        }
    }
    starts.extend(
        ctx.admit_iter(&best_upper_parameters, "IR surface upper parameter copy")?
            .copied(),
    );
    Ok((!starts.is_empty()).then_some(ScopedRows::new(starts, start_scratch)))
}

fn solve_nurbs_surface_parameter(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
    budget: &WorkBudget<'_>,
) -> Result<Option<(FinitePoint2, f64)>, ResourceLimit> {
    let seed = seed.and_then(FinitePoint2::new);
    let Some(u_degree) = usize::try_from(surface.u_degree()).ok() else {
        return Ok(None);
    };
    let Some(v_degree) = usize::try_from(surface.v_degree()).ok() else {
        return Ok(None);
    };
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let Some(u_domain) = surface.u_knots().span(u_degree, u_count) else {
        return Ok(None);
    };
    let Some(v_domain) = surface.v_knots().span(v_degree, v_count) else {
        return Ok(None);
    };
    if u_domain.endpoints()[0] >= u_domain.endpoints()[1]
        || v_domain.endpoints()[0] >= v_domain.endpoints()[1]
    {
        return Ok(None);
    }
    if let (Some(seed), Some(tolerance)) = (seed, fit_tolerance) {
        let [seed_u, seed_v] = seed.coordinates();
        let parameters = FinitePoint2::from_coordinates(
            u_domain.project(ExtendedReal::from_finite(seed_u)),
            v_domain.project(ExtendedReal::from_finite(seed_v)),
        );
        let Some(position) = finite_or_refusal(
            crate::eval::admission::EvaluationAdmission::Decode(ctx).within_work_slice(
                budget,
                |admission| {
                    crate::eval::decode::nurbs_surface_point(
                        admission,
                        surface,
                        parameters.u,
                        parameters.v,
                    )
                },
            ),
        )?
        else {
            return Ok(None);
        };
        let distance = (position.x - point.x)
            .hypot(position.y - point.y)
            .hypot(position.z - point.z);
        if distance.is_finite() && distance <= tolerance {
            return Ok(Some((parameters, distance)));
        }
        if let Some(refined) = refine_nurbs_surface_parameters(
            ctx, surface, point, parameters, u_domain, v_domain, budget,
        )? {
            let Some(position) = finite_or_refusal(
                crate::eval::admission::EvaluationAdmission::Decode(ctx).within_work_slice(
                    budget,
                    |admission| {
                        crate::eval::decode::nurbs_surface_point(
                            admission, surface, refined.u, refined.v,
                        )
                    },
                ),
            )?
            else {
                return Ok(None);
            };
            let distance = (position.x - point.x)
                .hypot(position.y - point.y)
                .hypot(position.z - point.z);
            if distance.is_finite() && distance <= tolerance {
                return Ok(Some((refined, distance)));
            }
        }
    }
    let Some(starts) =
        complete_nurbs_surface_starts(ctx, surface, point, seed, fit_tolerance, budget)?
    else {
        return Ok(None);
    };
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    let mut best_seed_distance = f64::INFINITY;
    for &start in ctx.admit_iter(&*starts, "IR surface inverse start visit")? {
        let Some(parameters) = refine_nurbs_surface_parameters(
            ctx, surface, point, start, u_domain, v_domain, budget,
        )?
        else {
            continue;
        };
        let Some(position) = finite_or_refusal(
            crate::eval::admission::EvaluationAdmission::Decode(ctx).within_work_slice(
                budget,
                |admission| {
                    crate::eval::decode::nurbs_surface_point(
                        admission,
                        surface,
                        parameters.u,
                        parameters.v,
                    )
                },
            ),
        )?
        else {
            continue;
        };
        let distance = (position.x - point.x)
            .hypot(position.y - point.y)
            .hypot(position.z - point.z);
        if !distance.is_finite() {
            continue;
        }
        // Quartering before subtraction keeps the tie metric finite even when
        // parameters span the full finite range. Its common scale preserves order.
        let seed_distance = seed.map_or(
            parameters.u.abs() * 0.25 + parameters.v.abs() * 0.25,
            |seed| (parameters.u * 0.25 - seed.u * 0.25).hypot(parameters.v * 0.25 - seed.v * 0.25),
        );
        let same_point = (distance - best_distance).abs()
            <= f64::EPSILON * 64.0 * distance.abs().max(best_distance.abs()).max(1.0);
        if best.is_none()
            || distance < best_distance && !same_point
            || same_point && seed_distance < best_seed_distance
        {
            best = Some(parameters);
            best_distance = distance;
            best_seed_distance = seed_distance;
        }
    }
    Ok(best.map(|parameters| (parameters, best_distance)))
}

/// Find a globally closest parameter pair on a finite NURBS surface within a
/// caller-owned work slice.
pub fn nurbs_surface_closest_parameter_with_budget(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    budget: &WorkBudget<'_>,
) -> Result<Option<FinitePoint2>, ResourceLimit> {
    let result = solve_nurbs_surface_parameter(ctx, surface, point, seed, None, budget)?;
    ctx.charge_work_limit(0, "IR surface closest parameter completion")?;
    Ok(result.map(|(parameters, _)| parameters))
}

/// Find a bounded local parameter candidate on a finite NURBS surface.
///
/// A supplied seed selects the local Newton branch. Without a seed, a fixed
/// parameter grid supplies the initial branch. The result is a constructive
/// candidate only; callers must forward-evaluate it and apply their own
/// residual bound. This operation does not prove that the candidate is
/// globally closest.
pub fn nurbs_surface_parameter_near_point<'ctx, 'arena: 'ctx>(
    admission: impl Into<admission::EvaluationAdmission<'ctx, 'arena>>,
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
) -> Result<Option<FinitePoint2>, ResourceLimit> {
    const COARSE_GRID: usize = 8;
    const COARSE_GRID_F64: f64 = 8.0;
    const MAX_ITERATIONS: usize = 24;
    const MAX_LINE_SEARCH_STEPS: usize = 12;
    let admission = admission.into();
    admission.work(0, "IR surface inverse boundary")?;

    if !point.is_finite() {
        return Ok(None);
    }
    let Some(u_degree) = usize::try_from(surface.u_degree()).ok() else {
        return Ok(None);
    };
    let Some(v_degree) = usize::try_from(surface.v_degree()).ok() else {
        return Ok(None);
    };
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let Some(u_domain) = surface.u_knots().span(u_degree, u_count) else {
        return Ok(None);
    };
    let Some(v_domain) = surface.v_knots().span(v_degree, v_count) else {
        return Ok(None);
    };
    let [u_start, u_end] = u_domain.endpoints();
    let [v_start, v_end] = v_domain.endpoints();
    if u_start >= u_end || v_start >= v_end {
        return Ok(None);
    }
    let mut parameters = match seed.and_then(FinitePoint2::new) {
        Some(seed) => {
            let [seed_u, seed_v] = seed.coordinates();
            FinitePoint2::from_coordinates(
                u_domain.project(ExtendedReal::from_finite(seed_u)),
                v_domain.project(ExtendedReal::from_finite(seed_v)),
            )
        }
        None => {
            let mut best = None;
            for u_index in 0..=COARSE_GRID {
                let Some(u_index) = f64_from_index(u_index) else {
                    return Ok(None);
                };
                let Some(u) = crate::math::interpolate(u_start, u_end, u_index / COARSE_GRID_F64)
                else {
                    return Ok(None);
                };
                for v_index in 0..=COARSE_GRID {
                    let Some(v_index) = f64_from_index(v_index) else {
                        return Ok(None);
                    };
                    let Some(v) =
                        crate::math::interpolate(v_start, v_end, v_index / COARSE_GRID_F64)
                    else {
                        return Ok(None);
                    };
                    let Some(candidate) =
                        finite_or_refusal(crate::eval::decode::nurbs_surface_point(
                            admission,
                            surface,
                            u.get(),
                            v.get(),
                        ))?
                    else {
                        return Ok(None);
                    };
                    let distance = candidate.distance(point);
                    if best.is_none_or(|(_, best_distance)| distance < best_distance) {
                        best = Some((FinitePoint2::from_coordinates(u, v), distance));
                    }
                }
            }
            let Some((best, _)) = best else {
                return Ok(None);
            };
            best
        }
    };
    let distance = |left: Point3| left.distance(point);
    for _ in 0..MAX_ITERATIONS {
        let Some(partials) = finite_or_refusal(nurbs_surface_partials(
            admission,
            surface,
            parameters.u,
            parameters.v,
        ))?
        else {
            return Ok(None);
        };
        let current_distance = distance(partials.point.get());
        if current_distance == 0.0 {
            return Ok(Some(parameters));
        }
        let residual = Vector3::new(
            partials.point.x - point.x,
            partials.point.y - point.y,
            partials.point.z - point.z,
        );
        let Some((step_u, step_v)) = least_squares_step(partials.du, partials.dv, residual) else {
            break;
        };
        let [u, v] = parameters.coordinates();
        let mut scale = FiniteReal::ONE;
        let mut accepted = false;
        for _ in 0..MAX_LINE_SEARCH_STEPS {
            let candidate = FinitePoint2::from_coordinates(
                u_domain.project(ExtendedReal::stepped(u, scale, step_u)),
                v_domain.project(ExtendedReal::stepped(v, scale, step_v)),
            );
            let Some(candidate_point) =
                finite_or_refusal(crate::eval::decode::nurbs_surface_point(
                    admission,
                    surface,
                    candidate.u,
                    candidate.v,
                ))?
            else {
                return Ok(None);
            };
            if distance(candidate_point.get()) < current_distance {
                parameters = candidate;
                accepted = true;
                break;
            }
            scale = scale.halved();
        }
        if !accepted {
            break;
        }
    }
    Ok(Some(parameters))
}

/// Find a NURBS surface parameter pair whose image is within `tolerance` of
/// `point`. The result is forward-evaluated before it is returned.
///
/// When multiple fitting pairs exist, `seed` selects the nearest parameter
/// branch. Without a seed, the pair nearest the parameter-space origin wins.
pub fn nurbs_surface_parameter_within_tolerance(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    tolerance: f64,
) -> Result<Option<FinitePoint2>, ResourceLimit> {
    let budget = ctx.work_budget(u64_from_index(DEFAULT_NURBS_SURFACE_INVERSION_WORK));
    nurbs_surface_parameter_within_tolerance_with_budget(
        ctx, surface, point, seed, tolerance, &budget,
    )
}

/// Find a NURBS surface parameter pair within `tolerance` using a
/// caller-owned work slice. A negative or non-finite tolerance finds nothing.
pub fn nurbs_surface_parameter_within_tolerance_with_budget(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    tolerance: f64,
    budget: &WorkBudget<'_>,
) -> Result<Option<FinitePoint2>, ResourceLimit> {
    ctx.charge_work_limit(0, "IR surface tolerance parameter boundary")?;
    let Some(tolerance) = NonNegativeReal::new(tolerance) else {
        return Ok(None);
    };
    nurbs_surface_parameter_within_nonnegative_tolerance_with_budget(
        ctx, surface, point, seed, tolerance, budget,
    )
}

/// Find a NURBS surface parameter pair within an admitted `tolerance` using
/// a caller-owned work slice.
pub fn nurbs_surface_parameter_within_nonnegative_tolerance_with_budget(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    tolerance: NonNegativeReal,
    budget: &WorkBudget<'_>,
) -> Result<Option<FinitePoint2>, ResourceLimit> {
    let tolerance = tolerance.get();
    let result = solve_nurbs_surface_parameter(ctx, surface, point, seed, Some(tolerance), budget)?;
    ctx.charge_work_limit(0, "IR surface tolerance parameter completion")?;
    let Some((parameters, distance)) = result else {
        return Ok(None);
    };
    Ok((distance.is_finite() && distance <= tolerance).then_some(parameters))
}

/// `base + Σ factorᵢ · directionᵢ` in model space.
fn offset(base: Point3, terms: &[(f64, Vector3)]) -> Point3 {
    let coordinate = |base: f64, component: fn(&Vector3) -> f64| match crate::math::sum::product_sum(
        std::iter::once(Some([1.0, base])).chain(
            terms
                .iter()
                .map(|(factor, vector)| Some([*factor, component(vector)])),
        ),
    ) {
        crate::math::sum::ProductSum::Value(value) => {
            value.finite().map_or(f64::NAN, FiniteReal::get)
        }
        crate::math::sum::ProductSum::Zero => 0.0,
        crate::math::sum::ProductSum::Undefined => f64::NAN,
    };
    Point3::new(
        coordinate(base.x, |v| v.x),
        coordinate(base.y, |v| v.y),
        coordinate(base.z, |v| v.z),
    )
}

/// Evaluate a NURBS curve with a caller-owned basis buffer of `degree + 1`
/// values. The caller admits the buffer before creating it.
pub fn nurbs_curve_point_at_with_basis(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    t: f64,
    basis: &mut [f64],
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    ctx.charge_work_limit(0, "IR caller basis curve evaluation boundary")?;
    let poles = curve.pole_rows();
    let degree = usize::try_from(curve.degree()).map_err(|_| EvaluationFailure::NoValue)?;
    if Some(basis.len()) != degree.checked_add(1) {
        return Err(EvaluationFailure::NoValue);
    }
    let at = FiniteReal::new(t).ok_or(EvaluationFailure::NoValue)?;
    let span = basis::bspline_span(ctx, curve.knots(), degree, poles.count(), at.get())?
        .ok_or(EvaluationFailure::NoValue)?;
    fill_bspline_basis(ctx, curve.knots(), degree, span, at.get(), basis)?
        .ok_or(EvaluationFailure::NonFinite(UNREACHED_POINT))?;
    let scratch = decode::Scratch::new(ctx);
    scratch.settle(nurbs_curve_point_from_basis(
        &scratch,
        basis,
        span,
        |index| poles.point_at(index),
        |index| poles.weight_at(index),
    ))
}

/// The point at `t` of a possibly-rational B-spline over `count` poles that
/// `pole` hands out admitted, or why it has no finite point there.
///
/// A basis that leaves the finite range reaches no coordinate, and each
/// reads NaN; a projection that overflows carries each coordinate it
/// reached. A knot vector or pole list that states no span at `t`, and a zero
/// weight sum, have no value.
fn nurbs_curve_point_evaluation(
    scratch: &decode::Scratch<'_, '_>,
    degree: u32,
    knots: &[f64],
    count: usize,
    pole: impl Fn(usize) -> Option<FinitePoint3>,
    weight: impl Fn(usize) -> Option<f64>,
    t: FiniteReal,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    scratch.settle(nurbs_curve_point_unsettled(
        scratch, degree, knots, count, pole, weight, t,
    ))
}

fn nurbs_curve_point_unsettled(
    scratch: &decode::Scratch<'_, '_>,
    degree: u32,
    knots: &[f64],
    count: usize,
    pole: impl Fn(usize) -> Option<FinitePoint3>,
    weight: impl Fn(usize) -> Option<f64>,
    t: FiniteReal,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let t = t.get();
    let no_value = EvaluationFailure::NoValue;
    let unreached = EvaluationFailure::NonFinite(UNREACHED_POINT);
    let degree = usize::try_from(degree).map_err(|_| no_value)?;
    let span = scratch
        .admit(basis::bspline_span(
            scratch.admission,
            knots,
            degree,
            count,
            t,
        ))
        .flatten()
        .ok_or(no_value)?;
    // At a finite parameter over finite knots, the basis is absent or not
    // finite only where one of its terms left the finite range.
    let basis = basis::bspline_basis(scratch, knots, degree, span, t).ok_or(unreached)?;
    nurbs_curve_point_from_basis(scratch, &basis, span, pole, weight)
}

fn nurbs_curve_point_from_basis(
    scratch: &decode::Scratch<'_, '_>,
    basis: &[f64],
    span: usize,
    pole: impl Fn(usize) -> Option<FinitePoint3>,
    weight: impl Fn(usize) -> Option<f64>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let no_value = EvaluationFailure::NoValue;
    let unreached = EvaluationFailure::NonFinite(UNREACHED_POINT);
    if !basis::all_finite(scratch, basis).ok_or_else(|| scratch.failure(unreached))? {
        return Err(unreached);
    }
    let first = span
        .checked_add(1)
        .and_then(|value| value.checked_sub(basis.len()))
        .ok_or(no_value)?;
    let base = homogeneous_curve_sum(scratch, basis, pole, weight, first).ok_or(no_value)?;
    let [x, y, z] = finite_lanes(base.project(base, &[]).ok_or(no_value)?)
        .map_err(|[x, y, z]| EvaluationFailure::NonFinite(Point3::new(x, y, z)))?;
    Ok(FinitePoint3::from_coordinates(x, y, z))
}

/// The homogeneous sum of `poles`, the first of which is global pole `first`,
/// blended by `values`.
fn homogeneous_curve_sum(
    scratch: &decode::Scratch<'_, '_>,
    values: &[f64],
    pole: impl Fn(usize) -> Option<FinitePoint3>,
    weight: impl Fn(usize) -> Option<f64>,
    first: usize,
) -> Option<Homogeneous> {
    scratch.admit(Homogeneous::sum(
        scratch,
        values.iter().copied().enumerate().map(|(local, basis)| {
            Some((
                [basis, 1.0],
                weight(first + local).unwrap_or(1.0),
                pole(first + local)?,
            ))
        }),
    ))?
}

/// Effective knot domain of a structurally evaluable NURBS curve.
pub fn nurbs_curve_parameter_domain(
    curve: &NurbsCurve,
) -> Option<crate::topology::IncreasingParameterInterval> {
    nurbs_pcurve_parameter_domain(curve.degree(), curve.knots(), curve.pole_count())
}

/// Effective knot domain shared by model-space and parameter-space NURBS
/// carriers. The full knot vector contains multiplicity and extrapolation
/// knots; only the interval between the degree-th knot and the control-pole
/// count-th knot is evaluable.
pub fn nurbs_pcurve_parameter_domain(
    degree: u32,
    knots: &[f64],
    control_point_count: usize,
) -> Option<crate::topology::IncreasingParameterInterval> {
    let degree = usize::try_from(degree).ok()?;
    if control_point_count <= degree
        || knots.len() < control_point_count.checked_add(degree)?.checked_add(1)?
    {
        return None;
    }
    let lower = *knots.get(degree)?;
    let upper = *knots.get(control_point_count)?;
    crate::topology::IncreasingParameterInterval::new([lower, upper])
}

const ADJACENT_PAIR: std::num::NonZeroUsize = match std::num::NonZeroUsize::new(2) {
    Some(width) => width,
    None => panic!("adjacent row window width must be nonzero"),
};
const NURBS_SEARCH_MAX_INTERVALS: usize = 512;
const MODEL_CURVE_PARAMETER_SEARCH_MAX_NEWTON_ITERATIONS: usize = 12;

#[derive(Clone, Copy)]
struct NurbsSearchWindow<'a> {
    domain: ParameterInterval,
    boundaries: &'a [FiniteReal],
}

/// Find a parameter witness whose NURBS curve point lies within `tolerance` of
/// `point`, searching finite knot spans in proximity to `seed`.
///
/// Interval rejection uses a rational-curve speed bound, so skipped intervals
/// cannot contain an admissible witness. The returned parameter is always
/// forward-evaluated within `tolerance`; `None` also covers malformed input or
/// exhaustion of the bounded certified search.
pub fn nurbs_curve_parameter_near_point(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    point: Point3,
    tolerance: f64,
    seed: f64,
) -> Result<Option<FiniteReal>, CodecError> {
    ctx.charge_work_limit(0, "IR NURBS curve inverse boundary")?;
    let Some(tolerance) = NonNegativeLength::new(tolerance) else {
        return Ok(None);
    };
    let Some(seed) = FiniteReal::new(seed) else {
        return Ok(None);
    };
    nurbs_curve_parameter_near_point_with_nonnegative_tolerance(ctx, curve, point, tolerance, seed)
}

/// [`nurbs_curve_parameter_near_point`] with an admitted tolerance and seed.
fn nurbs_curve_parameter_near_point_with_nonnegative_tolerance(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    point: Point3,
    tolerance: NonNegativeLength,
    seed: FiniteReal,
) -> Result<Option<FiniteReal>, CodecError> {
    let _depth = ctx.enter_nested("IR NURBS curve inversion depth")?;
    let scratch = decode::Scratch::new(ctx);
    let result = (|| {
        let tolerance = tolerance.get();
        let Some(degree) = usize::try_from(curve.degree()).ok() else {
            return Ok(None);
        };
        let count = curve.pole_count();
        let Some(domain) = nurbs_curve_parameter_domain(curve).map(ParameterInterval::from) else {
            return Ok(None);
        };
        if degree == 0 || !point.is_finite() {
            return Ok(None);
        }
        let mut source_storage = ctx.reserve_scoped(0, "IR curve inversion source scratch")?;
        let Some(weights) = validated_nurbs_curve_weights(ctx, &mut source_storage, curve)? else {
            return Ok(None);
        };
        let Some(speed_bound) =
            nurbs_curve_speed_bound_about(ctx, curve, point)?.map(FiniteReal::get)
        else {
            return Ok(None);
        };
        let mut poles = Vec::new();
        ctx.reserve_scoped_vec(
            &mut source_storage,
            &mut poles,
            count,
            "IR curve inversion controls",
        )?;
        for index in ctx.admit_iter(0..count, "IR curve inversion controls copy")? {
            let Some(pole) = curve.pole_rows().point_at(index) else {
                return Ok(None);
            };
            poles.push(pole);
        }
        let distance = |parameter: FiniteReal| {
            let position = finite_or_refusal(nurbs_curve_point_evaluation(
                &scratch,
                curve.degree(),
                curve.knots(),
                poles.len(),
                |index| poles.get(index).copied(),
                |index| {
                    weights
                        .values()
                        .and_then(|weights| weights.get(index).copied())
                },
                parameter,
            ))?;
            Ok(position.map(|position| position.distance(point)))
        };
        let seed = domain.project(ExtendedReal::from_finite(seed));
        let mut boundaries = Vec::new();
        ctx.reserve_scoped_vec(
            &mut source_storage,
            &mut boundaries,
            count - degree + 1,
            "IR curve inversion boundaries",
        )?;
        for index in ctx.admit_iter(degree..=count, "IR curve inversion boundary copy")? {
            let Some(boundary) = curve.knots().finite_knot(index) else {
                return Ok(None);
            };
            boundaries.push(boundary);
        }
        match nearest_boundary_witness(ctx, &boundaries, seed, tolerance, distance)? {
            BoundaryWitness::Found(parameter) => return Ok(Some(parameter)),
            BoundaryWitness::Invalid => return Ok(None),
            BoundaryWitness::NoMatch => {}
        }
        if let Some(parameter) = nurbs_curve_parameter_near_point_newton(
            ctx,
            curve,
            (&poles, weights.values()),
            point,
            tolerance,
            seed,
            NurbsSearchWindow {
                domain,
                boundaries: &boundaries,
            },
        )? {
            return Ok(Some(parameter));
        }
        let mut intervals = bounded_nearest_intervals(ctx, &boundaries, seed)?;
        let mut examined = 0usize;
        while let Some([start, end]) = intervals.0.pop() {
            ctx.charge_work(1, "IR curve inversion interval scan")?;
            examined += 1;
            if examined > NURBS_SEARCH_MAX_INTERVALS {
                return Ok(None);
            }
            let middle = start.midpoint(end);
            let Some(middle_distance) = distance(middle)? else {
                return Ok(None);
            };
            if middle_distance <= tolerance {
                return Ok(Some(middle));
            }
            let (start_value, middle_value, end_value) = (start.get(), middle.get(), end.get());
            if middle_distance
                - speed_bound * (end_value - middle_value).max(middle_value - start_value)
                > tolerance
                || middle == start
                || middle == end
            {
                continue;
            }
            let halves = [[start, middle], [middle, end]];
            let nearer = usize::from(
                interval_distance_to_parameter(halves[1], seed)
                    < interval_distance_to_parameter(halves[0], seed),
            );
            ctx.charge_work(2, "IR curve inversion interval split")?;
            ctx.push_scoped_vec(
                &mut intervals.1,
                &mut intervals.0,
                halves[1 - nearer],
                "IR curve inversion search intervals",
            )?;
            ctx.push_scoped_vec(
                &mut intervals.1,
                &mut intervals.0,
                halves[nearer],
                "IR curve inversion search intervals",
            )?;
        }
        Ok(None)
    })();
    scratch.unless_refused()?;
    result
}

fn nurbs_curve_parameter_near_point_newton(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    lanes: (&[FinitePoint3], Option<&[f64]>),
    point: Point3,
    tolerance: f64,
    seed: FiniteReal,
    search: NurbsSearchWindow<'_>,
) -> Result<Option<FiniteReal>, CodecError> {
    let (poles, weights) = lanes;
    let scratch = decode::Scratch::new(ctx);
    let result = (|| {
        let window =
            parameter_interval_containing(ctx, search.boundaries, seed)?.unwrap_or(search.domain);
        let mut parameter = window.project(ExtendedReal::from_finite(seed));
        for _ in 0..MODEL_CURVE_PARAMETER_SEARCH_MAX_NEWTON_ITERATIONS {
            let Some(position) = finite_or_refusal(nurbs_curve_point_evaluation(
                &scratch,
                curve.degree(),
                curve.knots(),
                poles.len(),
                |index| poles.get(index).copied(),
                |index| weights.and_then(|weights| weights.get(index).copied()),
                parameter,
            ))?
            else {
                return Ok(None);
            };
            let residual = Vector3::new(
                position.x - point.x,
                position.y - point.y,
                position.z - point.z,
            );
            if residual.norm() <= tolerance {
                return Ok(Some(parameter));
            }
            let Some(tangent) = finite_or_refusal(nurbs_curve_derivative(
                &scratch,
                curve.degree(),
                curve.knots(),
                poles,
                weights,
                parameter,
                CurveDerivative::First,
            ))?
            else {
                return Ok(None);
            };
            let tangent = tangent.get();
            let denominator = tangent.dot(tangent);
            if !denominator.is_finite() || denominator <= 0.0 {
                return Ok(None);
            }
            let Some(next) = FiniteReal::new(parameter.get() - residual.dot(tangent) / denominator)
            else {
                return Ok(None);
            };
            let next = window.project(ExtendedReal::from_finite(next));
            if next == parameter {
                return Ok(None);
            }
            parameter = next;
        }
        Ok(None)
    })();
    scratch.unless_refused()?;
    result
}

/// Global model-space speed bound for a structurally valid rational NURBS
/// curve over its effective knot domain.
pub fn nurbs_curve_speed_bound(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
) -> Result<Option<FiniteReal>, ResourceLimit> {
    if nurbs_curve_parameter_domain(curve).is_none() {
        return Ok(None);
    }
    nurbs_curve_speed_bound_about(ctx, curve, Point3::new(0.0, 0.0, 0.0))
}

enum ValidatedNurbsWeights {
    Unit,
    Rational(Vec<f64>),
}

impl ValidatedNurbsWeights {
    fn values(&self) -> Option<&[f64]> {
        match self {
            Self::Unit => None,
            Self::Rational(values) => Some(values),
        }
    }
}

/// A rational weight copy has at most one value per admitted control pole.
fn validated_nurbs_curve_weights(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    curve: &NurbsCurve,
) -> Result<Option<ValidatedNurbsWeights>, CodecError> {
    if nurbs_curve_parameter_domain(curve).is_none() {
        return Ok(None);
    }
    let points = match curve.pole_rows() {
        crate::geometry::nurbs::NurbsPoles3::Polynomial { .. } => {
            return Ok(Some(ValidatedNurbsWeights::Unit));
        }
        crate::geometry::nurbs::NurbsPoles3::Rational { points } => points,
    };
    let mut weights = Vec::new();
    ctx.reserve_scoped_vec(
        storage,
        &mut weights,
        curve.pole_count(),
        "IR curve inversion weights",
    )?;
    if !ctx.all_by_limit(
        points,
        |pole| {
            let weight = pole.weight.get();
            if weight <= 0.0 {
                return Ok(false);
            }
            weights.push(weight);
            Ok(true)
        },
        "IR curve inversion weight scan",
    )? {
        return Ok(None);
    }
    Ok(Some(ValidatedNurbsWeights::Rational(weights)))
}

fn nurbs_curve_speed_bound_about(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    origin: Point3,
) -> Result<Option<FiniteReal>, ResourceLimit> {
    speed_bound_by(
        ctx,
        curve.degree(),
        curve.knots(),
        curve.pole_count(),
        |index| {
            curve
                .pole_rows()
                .point_at(index)
                .map(|point| point.get().into())
        },
        |index| curve.pole_rows().weight_at(index).unwrap_or(1.0),
        origin.into(),
    )
}

fn interval_distance_to_parameter(interval: [FiniteReal; 2], parameter: FiniteReal) -> f64 {
    let [lower, upper] = interval.map(FiniteReal::get);
    let parameter = parameter.get();
    if parameter < lower {
        lower - parameter
    } else if parameter > upper {
        parameter - upper
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug)]
struct SearchInterval {
    bounds: [FiniteReal; 2],
    distance: f64,
}

impl PartialEq for SearchInterval {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for SearchInterval {}

impl PartialOrd for SearchInterval {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SearchInterval {
    fn cmp(&self, other: &Self) -> Ordering {
        self.distance
            .total_cmp(&other.distance)
            .then_with(|| self.bounds[0].get().total_cmp(&other.bounds[0].get()))
            .then_with(|| self.bounds[1].get().total_cmp(&other.bounds[1].get()))
    }
}

/// Retain only the nearest knot intervals that the bounded search can visit.
fn bounded_nearest_intervals<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    boundaries: &[FiniteReal],
    seed: FiniteReal,
) -> Result<
    (
        Vec<[FiniteReal; 2]>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let mut nearest = PriorityQueue::new(ctx)?;
    for pair in ctx
        .admit_iter(boundaries, "IR curve inversion interval visit")?
        .windows(ADJACENT_PAIR)
    {
        if pair[0] >= pair[1] {
            continue;
        }
        let candidate = SearchInterval {
            bounds: [pair[0], pair[1]],
            distance: interval_distance_to_parameter([pair[0], pair[1]], seed),
        };
        if nearest.len() < NURBS_SEARCH_MAX_INTERVALS {
            nearest.push(candidate)?;
        } else if let Some(farthest) = nearest.peek()? {
            ctx.charge_work(1, "IR curve inversion candidate comparison")?;
            if candidate < *farthest {
                let _farthest = nearest.replace_max(candidate)?;
            }
        }
    }
    let mut storage = ctx.reserve_scoped(0, "IR curve inversion intervals")?;
    let mut result = Vec::new();
    ctx.reserve_scoped_vec(
        &mut storage,
        &mut result,
        nearest.len(),
        "IR curve inversion intervals",
    )?;
    // Popping the max-heap emits the same descending total order used by the
    // interval stack, which visits its final entry first.
    while let Some(interval) = nearest.pop()? {
        ctx.charge_work(1, "IR curve inversion interval copy")?;
        result.push(interval.bounds);
    }
    Ok((result, storage))
}

struct PcurveIntervals<'ctx> {
    intervals: Vec<[f64; 2]>,
    truncated: bool,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

/// Retain the final valid knot intervals without materializing the full partition.
fn bounded_tail_intervals<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    boundaries: &[f64],
) -> Result<PcurveIntervals<'ctx>, ResourceLimit> {
    let mut storage = ctx.reserve_scoped_limit(0, "IR pcurve search intervals")?;
    let mut intervals = Vec::new();
    ctx.reserve_scoped_vec_limit(
        &mut storage,
        &mut intervals,
        boundaries.len().min(NURBS_SEARCH_MAX_INTERVALS),
        "IR pcurve search intervals",
    )?;
    let mut truncated = false;
    for pair in boundaries.windows(2).rev() {
        ctx.charge_work_limit(1, "IR pcurve search interval scan")?;
        if pair[0] < pair[1] {
            if intervals.len() == NURBS_SEARCH_MAX_INTERVALS {
                truncated = true;
                break;
            }
            ctx.charge_work_limit(1, "IR pcurve search interval copy")?;
            intervals.push([pair[0], pair[1]]);
        }
    }
    ctx.charge_work_limit(
        u64_from_index(intervals.len() / 2) * 2,
        "IR pcurve search interval reversal",
    )?;
    intervals.reverse();
    Ok(PcurveIntervals {
        intervals,
        truncated,
        storage,
    })
}

#[derive(Debug, PartialEq)]
enum BoundaryWitness {
    Invalid,
    NoMatch,
    Found(FiniteReal),
}

/// Find the nearest admissible distinct boundary without cloning and sorting knots.
fn nearest_boundary_witness<F>(
    ctx: &DecodeContext<'_>,
    boundaries: &[FiniteReal],
    seed: FiniteReal,
    tolerance: f64,
    mut distance: F,
) -> Result<BoundaryWitness, ResourceLimit>
where
    F: FnMut(FiniteReal) -> Result<Option<f64>, ResourceLimit>,
{
    ctx.charge_work_limit(0, "IR curve inversion boundary witness scan")?;
    let mut previous_boundary = None;
    let mut nearest = None;
    let mut nearest_seed_distance = f64::INFINITY;
    if !ctx.all_by_limit(
        boundaries,
        |&parameter| {
            if previous_boundary == Some(parameter) {
                return Ok(true);
            }
            previous_boundary = Some(parameter);
            let seed_distance = (parameter.get() - seed.get()).abs();
            if seed_distance >= nearest_seed_distance {
                return Ok(true);
            }
            let Some(candidate_distance) = distance(parameter)? else {
                return Ok(false);
            };
            if candidate_distance <= tolerance {
                nearest = Some(parameter);
                nearest_seed_distance = seed_distance;
            }
            Ok(true)
        },
        "IR curve inversion boundary witness scan",
    )? {
        return Ok(BoundaryWitness::Invalid);
    }
    Ok(nearest.map_or(BoundaryWitness::NoMatch, BoundaryWitness::Found))
}

fn parameter_interval_containing(
    ctx: &DecodeContext<'_>,
    boundaries: &[FiniteReal],
    parameter: FiniteReal,
) -> Result<Option<ParameterInterval>, ResourceLimit> {
    ctx.charge_work_limit(0, "IR curve inversion Newton interval scan")?;
    for pair in boundaries.windows(2) {
        ctx.charge_work_limit(1, "IR curve inversion Newton interval scan")?;
        if let Some(interval) = IncreasingParameterInterval::between(pair[0], pair[1])
            .filter(|_| parameter >= pair[0] && parameter <= pair[1])
            .map(ParameterInterval::from)
        {
            return Ok(Some(interval));
        }
    }
    Ok(None)
}

/// Map a NURBS parameter onto its evaluable knot branch.
///
/// Periodic parameters retain their serialized phase outside this operation
/// and are interpreted modulo the positive knot-domain period.
pub fn map_nurbs_curve_parameter(curve: &NurbsCurve, parameter: FiniteReal) -> Option<FiniteReal> {
    let domain = nurbs_curve_parameter_domain(curve)?;
    let [lower, upper] = domain.endpoints();
    if !curve.periodic() && !(lower..=upper).contains(&parameter.get()) {
        return None;
    }
    let mapped = periodic_parameter(
        curve.knots(),
        usize::try_from(curve.degree()).ok()?,
        curve.pole_count(),
        curve.periodic(),
        parameter,
    )?;
    Some(if curve.periodic() && mapped.get() == upper {
        domain.finite_endpoints()[0]
    } else {
        mapped
    })
}

/// Evaluate a possibly-rational B-spline curve over 2D `(u, v)` poles, or
/// report why it has no finite point there.
///
/// A parameter that is not finite, a pole that is not finite, a knot vector
/// or pole list that states no span at `t`, and a zero weight sum have no
/// value. A basis that leaves the finite range reaches no coordinate, and
/// each reads NaN; a projection that overflows carries each coordinate it
/// reached.
pub fn nurbs_pcurve_uv(
    ctx: &DecodeContext<'_>,
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    t: f64,
) -> Result<FinitePoint2, EvaluationFailure<Point2>> {
    ctx.charge_work_limit(0, "IR raw NURBS pcurve evaluation")?;
    let scratch = decode::Scratch::new(ctx);
    let result = FiniteReal::new(t)
        .ok_or(EvaluationFailure::NoValue)
        .and_then(|t| {
            nurbs_curve_point_evaluation(
                &scratch,
                degree,
                knots,
                control_points.len(),
                |index| FinitePoint2::new(*control_points.get(index)?).map(planar_pole),
                |index| weights.and_then(|weights| weights.get(index).copied()),
                t,
            )
            .map(|point| {
                let [u, v, _] = point.coordinates();
                FinitePoint2::from_coordinates(u, v)
            })
            .map_err(|failure| failure.map(|point| Point2::new(point.x, point.y)))
        });
    scratch
        .finish_evaluation(result)
        .map_err(EvaluationFailure::ResourceLimit)?
}

/// Return the signed endpoint-frame offset between two fitted sketch NURBS.
///
/// Both curves must be nonperiodic and clamped. Their corresponding endpoint
/// tangents must be parallel, and both result endpoints must have the same
/// normal displacement from the source. The result curve can have the opposite
/// stored traversal and can use a different degree or knot vector. This checks
/// the boundary-frame invariant of a fitted offset relation; it does not assert
/// pointwise equality between independently fitted interior parameterizations.
pub fn fitted_nurbs_offset_frame_distance(
    ctx: &DecodeContext<'_>,
    source: &crate::sketches::SketchGeometry,
    result: &crate::sketches::SketchGeometry,
    linear_tolerance: f64,
) -> Result<Option<FiniteReal>, ResourceLimit> {
    use crate::sketches::SketchGeometryDefinition;

    ctx.charge_work_limit(0, "IR fitted NURBS offset boundary")?;

    if !linear_tolerance.is_finite() || linear_tolerance < 0.0 {
        return Ok(None);
    }
    let (
        SketchGeometryDefinition::Nurbs { curve: source },
        SketchGeometryDefinition::Nurbs { curve: result },
    ) = (source.definition(), result.definition())
    else {
        return Ok(None);
    };
    if source.periodic() || result.periodic() {
        return Ok(None);
    }
    let Some(source_frames) = clamped_nurbs_pcurve_endpoint_frames(ctx, source)? else {
        return Ok(None);
    };
    let Some(result_frames) = clamped_nurbs_pcurve_endpoint_frames(ctx, result)? else {
        return Ok(None);
    };
    let same = fitted_nurbs_offset_candidate(source_frames, result_frames, linear_tolerance);
    let reversed = fitted_nurbs_offset_candidate(
        source_frames,
        [
            (
                result_frames[1].0,
                Point2::new(-result_frames[1].1.u, -result_frames[1].1.v),
            ),
            (
                result_frames[0].0,
                Point2::new(-result_frames[0].1.u, -result_frames[0].1.v),
            ),
        ],
        linear_tolerance,
    );
    Ok(match (same, reversed) {
        (Some(distance), None) | (None, Some(distance)) => Some(distance),
        _ => None,
    })
}

struct PcurveDifferential {
    point: FinitePoint2,
    /// The first derivative, or why it has no finite value.
    tangent: Result<FinitePoint2, EvaluationFailure<Point2>>,
    acceleration: Option<FinitePoint2>,
}

/// The parameter-plane pole `(u, v)` as the model-space pole `(u, v, 0)`.
fn planar_pole(pole: FinitePoint2) -> FinitePoint3 {
    let [u, v] = pole.coordinates();
    FinitePoint3::from_coordinates(u, v, FiniteReal::ZERO)
}

/// A parameter-plane value from its two coordinate evaluations: finite when
/// both are, absent when either is, and otherwise non-finite with the value
/// each coordinate reached.
fn planar_value(
    u: Result<FiniteReal, EvaluationFailure<f64>>,
    v: Result<FiniteReal, EvaluationFailure<f64>>,
) -> Result<FinitePoint2, EvaluationFailure<Point2>> {
    let reached = |coordinate: Result<FiniteReal, EvaluationFailure<f64>>| match coordinate {
        Ok(value) => Ok(Some(value.get())),
        Err(failure) => failure.non_finite(),
    };
    match (u, v) {
        (Ok(u), Ok(v)) => Ok(FinitePoint2::from_coordinates(u, v)),
        (u, v) => match (reached(u)?, reached(v)?) {
            (Some(u), Some(v)) => Err(EvaluationFailure::NonFinite(Point2::new(u, v))),
            _ => Err(EvaluationFailure::NoValue),
        },
    }
}

/// [`nurbs_pcurve_differential_with`] over raw `(u, v)` poles, each admitted
/// as the span that supports `t` reads it.
#[cfg(test)]
fn nurbs_pcurve_differential(
    ctx: &DecodeContext<'_>,
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    t: f64,
) -> Result<PcurveDifferential, EvaluationFailure<Point2>> {
    ctx.charge_work_limit(0, "IR raw NURBS pcurve evaluation")?;
    let scratch = decode::Scratch::new(ctx);
    let result = FiniteReal::new(t)
        .ok_or(EvaluationFailure::NoValue)
        .and_then(|t| {
            nurbs_pcurve_differential_with(
                &scratch,
                degree,
                knots,
                control_points.len(),
                |index| FinitePoint2::new(*control_points.get(index)?).map(planar_pole),
                weights,
                t,
            )
        });
    scratch
        .finish_evaluation(result)
        .map_err(EvaluationFailure::ResourceLimit)?
}

/// The point and first two derivatives at `t` of a possibly-rational
/// B-spline over `count` planar poles that `pole` hands out admitted and
/// placed in the model-space plane `z = 0`.
///
/// A point that overflows is non-finite and carries the value each
/// coordinate reached; a basis that leaves the finite range reaches no
/// coordinate, and each reads NaN. The rational quotient rule forms the
/// derivatives from the finite point, so a curve without one has no
/// derivatives here.
fn nurbs_pcurve_differential_with(
    scratch: &decode::Scratch<'_, '_>,
    degree: u32,
    knots: &[f64],
    count: usize,
    pole: impl Fn(usize) -> Option<FinitePoint3>,
    weights: Option<&[f64]>,
    t: FiniteReal,
) -> Result<PcurveDifferential, EvaluationFailure<Point2>> {
    scratch.settle(nurbs_pcurve_differential_unsettled(
        scratch, degree, knots, count, pole, weights, t,
    ))
}

fn nurbs_pcurve_differential_unsettled(
    scratch: &decode::Scratch<'_, '_>,
    degree: u32,
    knots: &[f64],
    count: usize,
    pole: impl Fn(usize) -> Option<FinitePoint3>,
    weights: Option<&[f64]>,
    t: FiniteReal,
) -> Result<PcurveDifferential, EvaluationFailure<Point2>> {
    let t = t.get();
    let unreached = EvaluationFailure::NonFinite(Point2::new(f64::NAN, f64::NAN));
    let degree = usize::try_from(degree).map_err(|_| EvaluationFailure::NoValue)?;
    let span = scratch
        .admit(basis::bspline_span(
            scratch.admission,
            knots,
            degree,
            count,
            t,
        ))
        .flatten()
        .ok_or(EvaluationFailure::NoValue)?;
    // At a finite parameter over finite knots, the basis is absent or not
    // finite only where one of its terms left the finite range.
    let basis = basis::bspline_basis(scratch, knots, degree, span, t).ok_or(unreached)?;
    if !basis::all_finite(scratch, &basis).ok_or_else(|| scratch.failure(unreached))? {
        return Err(unreached);
    }
    let sum = |values: &[f64]| {
        homogeneous_curve_sum(
            scratch,
            values,
            &pole,
            |index| weights.and_then(|weights| weights.get(index).copied()),
            span - degree,
        )
    };
    let base = sum(&basis).ok_or(EvaluationFailure::NoValue)?;
    let point = finite_lanes(base.project(base, &[]).ok_or(EvaluationFailure::NoValue)?)
        .map_err(|[u, v, _]| EvaluationFailure::NonFinite(Point2::new(u, v)))?;
    let uv = |p: [FiniteReal; 3]| FinitePoint2::from_coordinates(p[0], p[1]);
    let point_only = |tangent| PcurveDifferential {
        point: uv(point),
        tangent: Err(tangent),
        acceleration: None,
    };
    // The derivative basis is absent only where a term of the lower basis
    // left the finite range.
    let Some(mut first_basis) = basis::bspline_basis_derivative(scratch, knots, degree, span, t)
    else {
        return Ok(point_only(unreached));
    };
    let mut second_basis = basis::bspline_basis_second_derivative(scratch, knots, degree, span, t);
    let scale = if basis::all_finite(scratch, &first_basis)
        .ok_or_else(|| scratch.failure(unreached))?
        && match &second_basis {
            Some(values) => {
                basis::all_finite(scratch, values).ok_or_else(|| scratch.failure(unreached))?
            }
            None => true,
        } {
        PositiveReal::ONE
    } else {
        let width = knots[span + 1] - knots[span];
        let Some(scale) = PositiveReal::new(width) else {
            // A span of zero width has no derivative; the derivative over a
            // width outside the finite range is not reached.
            return Ok(point_only(if width.is_finite() {
                EvaluationFailure::NoValue
            } else {
                unreached
            }));
        };
        if degree == 1 {
            let poles = [
                pole(span - 1).ok_or(EvaluationFailure::NoValue)?,
                pole(span).ok_or(EvaluationFailure::NoValue)?,
            ];
            let local_weights = weights.map(|weights| {
                [
                    weights.get(span - 1).copied().unwrap_or(1.0),
                    weights.get(span).copied().unwrap_or(1.0),
                ]
            });
            let derivative = |second| {
                linear_nurbs_derivative(
                    &basis,
                    &poles,
                    local_weights.as_ref().map(<[f64; 2]>::as_slice),
                    1,
                    scale.get(),
                    second,
                )
            };
            // The quotient form divides by the span itself, so each lane is
            // the derivative's coordinate or the signed infinity it reached.
            let lane = |lane: Result<FiniteReal, f64>| lane.map_err(EvaluationFailure::NonFinite);
            return Ok(PcurveDifferential {
                point: uv(point),
                tangent: derivative(false).map_or(Err(EvaluationFailure::NoValue), |[x, y, _]| {
                    planar_value(lane(x), lane(y))
                }),
                acceleration: derivative(true)
                    .and_then(|lanes| finite_lanes(lanes).ok())
                    .map(uv),
            });
        }
        let Some(scaled) =
            basis::bspline_basis_scaled_derivatives(scratch, knots, degree, span, t, scale)
        else {
            return Ok(point_only(unreached));
        };
        first_basis = scaled.first;
        second_basis = Some(Cow::Owned(scaled.second));
        scale
    };
    let first_sum = sum(&first_basis);
    let first_lanes = first_sum.and_then(|sum| sum.project(base, &[(sum, point)]));
    let first = first_lanes.and_then(|lanes| finite_lanes(lanes).ok());
    let second = first_sum.zip(first).and_then(|(first_sum, first)| {
        let second_sum = sum(second_basis.as_ref()?)?;
        finite_lanes(second_sum.project(
            base,
            &[(second_sum, point), (first_sum, first), (first_sum, first)],
        )?)
        .ok()
    });
    let unscale_twice = |value: FiniteReal| -> Result<Option<FiniteReal>, ResourceLimit> {
        if scale.get() == 1.0 {
            Ok(Some(value))
        } else {
            let Some(value) = finite_or_refusal(difference_quotient(
                value,
                FiniteReal::ZERO,
                scale.into(),
                FiniteReal::ZERO,
            ))?
            else {
                return Ok(None);
            };
            finite_or_refusal(difference_quotient(
                value,
                FiniteReal::ZERO,
                scale.into(),
                FiniteReal::ZERO,
            ))
        }
    };
    // A derivative lane over the span is the coordinate over the scale, or
    // the signed infinity its quotient reached; the positive scale leaves an
    // infinity unchanged.
    let tangent_lane = |lane: Result<FiniteReal, f64>| match lane {
        Ok(value) if scale.get() == 1.0 => Ok(value),
        Ok(value) => difference_quotient(value, FiniteReal::ZERO, scale.into(), FiniteReal::ZERO),
        Err(reached) => Err(EvaluationFailure::NonFinite(reached)),
    };
    let acceleration = match second {
        Some(value) => match (unscale_twice(value[0])?, unscale_twice(value[1])?) {
            (Some(u), Some(v)) => Some(FinitePoint2::from_coordinates(u, v)),
            _ => None,
        },
        None => None,
    };
    Ok(PcurveDifferential {
        point: uv(point),
        // A derivative sum or projection is absent only where the derivative
        // basis left the finite range.
        tangent: first_lanes.map_or(Err(unreached), |[x, y, _]| {
            planar_value(tangent_lane(x), tangent_lane(y))
        }),
        acceleration,
    })
}

/// Return whether a point lies within `tolerance` of a nonperiodic NURBS
/// pcurve, using evaluated witnesses and Lipschitz-bounded interval rejection.
///
/// Positive rational weights make both the homogeneous curve and its
/// derivative convex combinations of their control polygons. Their norms
/// therefore bound Euclidean curve speed after the quotient rule. The search
/// accepts only an evaluated curve point within tolerance; intervals whose
/// midpoint distance minus the maximum possible travel exceeds tolerance are
/// discarded. `None` denotes invalid input or exhaustion of the bounded search.
pub fn nurbs_pcurve_contains_point(
    ctx: &DecodeContext<'_>,
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    point: Point2,
    tolerance: f64,
) -> Result<Option<bool>, ResourceLimit> {
    ctx.charge_work_limit(0, "IR NURBS pcurve containment boundary")?;
    let Some(degree_usize) = usize::try_from(degree).ok() else {
        return Ok(None);
    };
    let count = control_points.len();
    let Some(required_knots) = count
        .checked_add(degree_usize)
        .and_then(|value| value.checked_add(1))
    else {
        return Ok(None);
    };
    if degree_usize == 0
        || count <= degree_usize
        || knots.len() < required_knots
        || !tolerance.is_finite()
        || tolerance < 0.0
        || !point.is_finite()
    {
        return Ok(None);
    }
    let weights = match weights {
        Some(weights) if weights.len() == count => Some(weights),
        Some(_) => return Ok(None),
        None => None,
    };
    let Some(speed_bound) = speed_bound_by(
        ctx,
        degree,
        knots,
        control_points.len(),
        |index| control_points.get(index).map(|point| [point.u, point.v]),
        |index| weights.map_or(1.0, |weights| weights[index]),
        [point.u, point.v],
    )?
    .map(FiniteReal::get) else {
        return Ok(None);
    };

    let domain = [knots[degree_usize], knots[count]];
    if domain[0] > domain[1] {
        return Ok(None);
    }
    let mut search = bounded_tail_intervals(ctx, &knots[degree_usize..=count])?;
    if search.intervals.is_empty() {
        ctx.charge_work_limit(1, "IR pcurve search interval copy")?;
        ctx.reserve_scoped_vec_limit(
            &mut search.storage,
            &mut search.intervals,
            1,
            "IR pcurve search intervals",
        )?;
        search.intervals.push(domain);
    }
    let mut examined = 0usize;
    while !search.intervals.is_empty() {
        ctx.charge_work_limit(1, "IR pcurve containment interval visit")?;
        let Some([start, end]) = search.intervals.pop() else {
            break;
        };
        examined += 1;
        if examined > NURBS_SEARCH_MAX_INTERVALS {
            return Ok(None);
        }
        let middle = start.midpoint(end);
        let curve_uv = match nurbs_pcurve_uv(
            ctx,
            degree,
            knots,
            control_points,
            weights,
            middle,
        ) {
            Ok(value) => Point2::from(value),
            Err(EvaluationFailure::ResourceLimit(limit)) => return Err(limit),
            Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(_)) => return Ok(None),
        };
        let distance = (curve_uv.u - point.u).hypot(curve_uv.v - point.v);
        if distance <= tolerance {
            return Ok(Some(true));
        }
        let travel_bound = speed_bound * (end - middle).max(middle - start);
        if distance - travel_bound > tolerance {
            continue;
        }
        if middle == start || middle == end {
            continue;
        }
        ctx.reserve_scoped_vec_limit(
            &mut search.storage,
            &mut search.intervals,
            2,
            "IR pcurve search subdivisions",
        )?;
        ctx.charge_work_limit(2, "IR pcurve search subdivision copy")?;
        search.intervals.push([start, middle]);
        search.intervals.push([middle, end]);
    }
    Ok((!search.truncated).then_some(false))
}

/// A tensor-product NURBS surface at a parameter: its spans, its bases and
/// its homogeneous base sum, with its finite point.
struct NurbsSurfaceLocal<'a> {
    surface: &'a NurbsSurface,
    degrees: [usize; 2],
    spans: [usize; 2],
    parameters: [f64; 2],
    bases: [decode::SupportValues<f64>; 2],
    base: Homogeneous,
    point: [FiniteReal; 3],
}

/// The first partials of a NURBS surface at a parameter: the derivative
/// bases, the homogeneous derivative sums and the finite lanes.
struct NurbsSurfaceFirstPartials {
    bases: [Vec<f64>; 2],
    sums: [Homogeneous; 2],
    lanes: [[FiniteReal; 3]; 2],
}

/// The second derivative bases, homogeneous sums and projected lanes.
/// The requested third order reuses all of this actual local state.
struct NurbsSurfaceSecondPartials {
    bases: [Cow<'static, [f64]>; 2],
    sums: [Homogeneous; 3],
    lanes: [[FiniteReal; 3]; 3],
}

/// The homogeneous sum of a NURBS surface's poles local to `spans`, blended
/// by `u_values` along `u` and `v_values` along `v`. A missing pole and a
/// value that is not finite leave no sum.
fn nurbs_local_sum(
    scratch: &decode::Scratch<'_, '_>,
    surface: &NurbsSurface,
    [u_degree, v_degree]: [usize; 2],
    [u_span, v_span]: [usize; 2],
    u_values: &[f64],
    v_values: &[f64],
) -> Option<Homogeneous> {
    scratch.admit(Homogeneous::sum(
        scratch,
        u_values
            .iter()
            .copied()
            .enumerate()
            .flat_map(|(i, u_value)| {
                v_values
                    .iter()
                    .copied()
                    .enumerate()
                    .map(move |(j, v_value)| {
                        let (pole_u, pole_v) = (u_span - u_degree + i, v_span - v_degree + j);
                        Some((
                            [u_value, v_value],
                            surface.weight(pole_u, pole_v).map_or(1.0, NonZeroReal::get),
                            surface.pole(pole_u, pole_v)?,
                        ))
                    })
            }),
    ))?
}

impl NurbsSurfaceLocal<'_> {
    /// The homogeneous sum of the local poles blended by `u_values` along
    /// `u` and `v_values` along `v`.
    fn sum(
        &self,
        scratch: &decode::Scratch<'_, '_>,
        u_values: &[f64],
        v_values: &[f64],
    ) -> Option<Homogeneous> {
        nurbs_local_sum(
            scratch,
            self.surface,
            self.degrees,
            self.spans,
            u_values,
            v_values,
        )
    }

    /// A third-only homogeneous derivative sum over the exact pole window.
    fn derivative_sum(
        &self,
        scratch: &decode::Scratch<'_, '_>,
        u_values: &[f64],
        v_values: &[f64],
    ) -> Option<Homogeneous> {
        let count = u_values.len().checked_mul(v_values.len())?;
        scratch.admit(Homogeneous::derivative_sum(scratch, (0..count).map(|local| {
            let (i, j) = (local / v_values.len(), local % v_values.len());
            let (pole_u, pole_v) = (self.spans[0] - self.degrees[0] + i, self.spans[1] - self.degrees[1] + j);
            Some((
                [u_values[i], v_values[j]],
                self.surface.weight(pole_u, pole_v).map_or(1.0, NonZeroReal::get),
                self.surface.pole(pole_u, pole_v)?,
            ))
        })))?
    }

    /// The first partials, or why they have none. At the finite point over
    /// finite knots, a derivative basis that is absent, a sum that is absent
    /// and a projection that is absent or overflows each left the finite
    /// range.
    fn first(
        &self,
        scratch: &decode::Scratch<'_, '_>,
    ) -> Result<NurbsSurfaceFirstPartials, EvaluationFailure<()>> {
        scratch.settle((|| {
            let non_finite = EvaluationFailure::NonFinite(());
            let knots = [self.surface.u_knots(), self.surface.v_knots()];

            let derivative = |axis: usize| {
                basis::bspline_basis_derivative(
                    scratch,
                    knots[axis],
                    self.degrees[axis],
                    self.spans[axis],
                    self.parameters[axis],
                )
                .ok_or_else(|| scratch.failure(non_finite))
            };
            let bases = [derivative(0)?, derivative(1)?];
            let u = self
                .sum(scratch, &bases[0], &self.bases[1])
                .ok_or(non_finite)?;
            let v = self
                .sum(scratch, &self.bases[0], &bases[1])
                .ok_or(non_finite)?;
            let lane = |sum: Homogeneous| {
                finite_lanes(
                    sum.project(self.base, &[(sum, self.point)])
                        .ok_or(non_finite)?,
                )
                .map_err(|_| non_finite)
            };
            let lanes = [lane(u)?, lane(v)?];
            Ok(NurbsSurfaceFirstPartials {
                bases,
                sums: [u, v],
                lanes,
            })
        })())
    }

    /// The second partials over `first`, or why they have none, in the terms
    /// of [`Self::first`].
    fn second(
        &self,
        scratch: &decode::Scratch<'_, '_>,
        first: &NurbsSurfaceFirstPartials,
    ) -> Result<NurbsSurfaceSecondPartials, EvaluationFailure<()>> {
        scratch.settle((|| {
            let non_finite = EvaluationFailure::NonFinite(());
            let knots = [self.surface.u_knots(), self.surface.v_knots()];

            let second = |axis: usize| {
                basis::bspline_basis_second_derivative(
                    scratch,
                    knots[axis],
                    self.degrees[axis],
                    self.spans[axis],
                    self.parameters[axis],
                )
                .ok_or_else(|| scratch.failure(non_finite))
            };
            let [u_second, v_second] = [second(0)?, second(1)?];
            let [u, v] = first.sums;
            let [du, dv] = first.lanes;
            let uu = self
                .sum(scratch, &u_second, &self.bases[1])
                .ok_or(non_finite)?;
            let uv = self
                .sum(scratch, &first.bases[0], &first.bases[1])
                .ok_or(non_finite)?;
            let vv = self
                .sum(scratch, &self.bases[0], &v_second)
                .ok_or(non_finite)?;
            let lane = |sum: Homogeneous, corrections: &[(Homogeneous, [FiniteReal; 3])]| {
                finite_lanes(sum.project(self.base, corrections).ok_or(non_finite)?)
                    .map_err(|_| non_finite)
            };
            let lanes = [
                lane(uu, &[(uu, self.point), (u, du), (u, du)])?,
                lane(uv, &[(uv, self.point), (u, dv), (v, du)])?,
                lane(vv, &[(vv, self.point), (v, dv), (v, dv)])?,
            ];
            Ok(NurbsSurfaceSecondPartials {
                bases: [u_second, v_second], sums: [uu, uv, vv], lanes,
            })
        })())
    }
    /// Differentiate H=w*S three times. Polynomial homogeneous derivatives
    /// above the stored degree are zero; rational quotient corrections remain.
    fn third(
        &self,
        scratch: &decode::Scratch<'_, '_>,
        first: &NurbsSurfaceFirstPartials,
        second: &NurbsSurfaceSecondPartials,
    ) -> Result<[[FiniteReal; 3]; 4], EvaluationFailure<()>> {
        scratch.settle((|| {
            scratch.admission.independent_cost(nurbs_surface_third_evaluation_cost(self.degrees))?;
            let non_finite = EvaluationFailure::NonFinite(());
            let knots = [self.surface.u_knots(), self.surface.v_knots()];
            let third = |axis: usize| basis::bspline_basis_third_derivative(
                scratch, knots[axis], self.degrees[axis], self.spans[axis], self.parameters[axis],
            ).ok_or_else(|| scratch.failure(non_finite));
            let bases = [third(0)?, third(1)?];
            let sum = |active, u: &[f64], v: &[f64]| if active {
                self.derivative_sum(scratch, u, v).ok_or(non_finite)
            } else { Ok(Homogeneous::zero()) };
            let [u_degree, v_degree] = self.degrees;
            let uuu = sum(u_degree >= 3, &bases[0], &self.bases[1])?;
            let uuv = sum(u_degree >= 2 && v_degree >= 1, &second.bases[0], &first.bases[1])?;
            let uvv = sum(u_degree >= 1 && v_degree >= 2, &first.bases[0], &second.bases[1])?;
            let vvv = sum(v_degree >= 3, &self.bases[0], &bases[1])?;
            let [u, v] = first.sums;
            let [uu, uv, vv] = second.sums;
            let [du, dv] = first.lanes;
            let [duu, duv, dvv] = second.lanes;
            let lane = |sum: Homogeneous, corrections: &[(Homogeneous, [FiniteReal; 3])]| {
                finite_lanes(sum.project(self.base, corrections).ok_or(non_finite)?).map_err(|_| non_finite)
            };
            Ok([
                lane(uuu, &[(uuu, self.point), (uu, du), (uu, du), (uu, du), (u, duu), (u, duu), (u, duu)])?,
                lane(uuv, &[(uuv, self.point), (uu, dv), (uv, du), (uv, du), (u, duv), (u, duv), (v, duu)])?,
                lane(uvv, &[(uvv, self.point), (vv, du), (uv, dv), (uv, dv), (v, duv), (v, duv), (u, dvv)])?,
                lane(vvv, &[(vvv, self.point), (vv, dv), (vv, dv), (vv, dv), (v, dvv), (v, dvv), (v, dvv)])?,
            ])
        })())
    }
}

/// Extra independent work for the actual third recurrence rows and homogeneous
/// pole traversals. Existing point/first/second independent laws are unchanged.
fn nurbs_surface_third_evaluation_cost([u_degree, v_degree]: [usize; 2]) -> Option<usize> {
    let basis_work = |degree: usize| {
        if degree < 3 { return Some(0); }
        let base = degree - 3;
        let base_work = if base <= 1 { 0 } else {
            // Heap initialization: q+1. Cox-de Boor: first write, triangular
            // blends, then one saved value per row.
            base.checked_add(1)?.checked_add(1)?
                .checked_add(base.checked_mul(base.checked_add(1)?)?.checked_div(2)?)?
                .checked_add(base)?
        };
        base_work.checked_add(degree.checked_mul(3)?)
    };
    let supports = u_degree.checked_add(1)?.checked_mul(v_degree.checked_add(1)?)?;
    let sums = usize::from(u_degree >= 3)
        + usize::from(u_degree >= 2 && v_degree >= 1)
        + usize::from(u_degree >= 1 && v_degree >= 2)
        + usize::from(v_degree >= 3);
    let visits = if supports > 2 { supports.checked_mul(sums)? } else { 0 };
    basis_work(u_degree)?.checked_add(basis_work(v_degree)?)?.checked_add(visits)
}

/// A tensor-product NURBS surface at `(u, v)`, or why it has no finite
/// point there.
///
/// A basis that leaves the finite range reaches no coordinate, and each
/// coordinate reads NaN; a projection that overflows carries each coordinate
/// it reached. A parameter that is not finite, a knot vector or pole net that
/// states no span at the parameter, and a zero weight sum have no value.
fn nurbs_surface_local<'a>(
    scratch: &decode::Scratch<'_, '_>,
    surface: &'a NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<NurbsSurfaceLocal<'a>, EvaluationFailure<Point3>> {
    scratch.settle(nurbs_surface_local_unsettled(scratch, surface, u_at, v_at))
}

fn nurbs_surface_local_unsettled<'a>(
    scratch: &decode::Scratch<'_, '_>,
    surface: &'a NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<NurbsSurfaceLocal<'a>, EvaluationFailure<Point3>> {
    let no_value = EvaluationFailure::NoValue;
    let unreached = EvaluationFailure::NonFinite(Point3::new(f64::NAN, f64::NAN, f64::NAN));
    let u_degree = usize::try_from(surface.u_degree()).map_err(|_| no_value)?;
    let v_degree = usize::try_from(surface.v_degree()).map_err(|_| no_value)?;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let u_at = periodic_parameter(
        surface.u_knots(),
        u_degree,
        u_count,
        surface.u_periodic(),
        FiniteReal::new(u_at).ok_or(no_value)?,
    )
    .ok_or(no_value)?
    .get();
    let v_at = periodic_parameter(
        surface.v_knots(),
        v_degree,
        v_count,
        surface.v_periodic(),
        FiniteReal::new(v_at).ok_or(no_value)?,
    )
    .ok_or(no_value)?
    .get();
    let u_span = scratch
        .admit(basis::bspline_span(
            scratch.admission,
            surface.u_knots(),
            u_degree,
            u_count,
            u_at,
        ))
        .flatten()
        .ok_or(no_value)?;
    let v_span = scratch
        .admit(basis::bspline_span(
            scratch.admission,
            surface.v_knots(),
            v_degree,
            v_count,
            v_at,
        ))
        .flatten()
        .ok_or(no_value)?;
    // At a finite parameter over finite knots, the basis is absent or not
    // finite only where one of its terms left the finite range.
    let u_basis = basis::bspline_basis(scratch, surface.u_knots(), u_degree, u_span, u_at)
        .ok_or(unreached)?;
    let v_basis = basis::bspline_basis(scratch, surface.v_knots(), v_degree, v_span, v_at)
        .ok_or(unreached)?;
    if !basis::all_finite(scratch, &u_basis).ok_or_else(|| scratch.failure(unreached))?
        || !basis::all_finite(scratch, &v_basis).ok_or_else(|| scratch.failure(unreached))?
    {
        return Err(unreached);
    }
    let degrees = [u_degree, v_degree];
    let spans = [u_span, v_span];
    let base =
        nurbs_local_sum(scratch, surface, degrees, spans, &u_basis, &v_basis).ok_or(no_value)?;
    let point = finite_lanes(base.project(base, &[]).ok_or(no_value)?)
        .map_err(|[x, y, z]| EvaluationFailure::NonFinite(Point3::new(x, y, z)))?;
    Ok(NurbsSurfaceLocal {
        surface,
        degrees,
        spans,
        parameters: [u_at, v_at],
        bases: [u_basis, v_basis],
        base,
        point,
    })
}

/// A NURBS surface's point and first partials at `(u, v)`, the partials with
/// their own outcome, or why the point has none.
fn nurbs_surface_first_order(
    scratch: &decode::Scratch<'_, '_>,
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    scratch
        .admission
        .independent_cost(nurbs_surface_partials_evaluation_cost(surface))?;
    let result = (|| {
        let local = nurbs_surface_local(scratch, surface, u_at, v_at)?;
        let [x, y, z] = local.point;
        Ok(SurfaceFirstOrder {
            point: FinitePoint3::from_coordinates(x, y, z),
            first: local
                .first(scratch)
                .map(|first| first.lanes.map(finite_vector)),
        })
    })();
    scratch.settle(result)
}

/// A NURBS surface's point with its first and second partials at `(u, v)`,
/// each order with its own outcome, or why the point has none.
fn nurbs_surface_requested_jet(
    scratch: &decode::Scratch<'_, '_>,
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
    request: SurfaceRequest,
) -> Result<RequestedJet, EvaluationFailure<Point3>> {
    scratch.admission.independent_cost(nurbs_surface_partials_evaluation_cost(surface))?;
    let result = (|| {
        let local = nurbs_surface_local(scratch, surface, u_at, v_at)?;
        let [x, y, z] = local.point;
        let first = local.first(scratch);
        let second = if request.needs_second() {
            first.as_ref().map_err(|failure| *failure)
                .and_then(|first| local.second(scratch, first))
        } else { Err(EvaluationFailure::NoValue) };
        let third = if request.needs_third() {
            first.as_ref().map_err(|failure| *failure).and_then(|first| {
                second.as_ref().map_err(|failure| *failure)
                    .and_then(|second| local.third(scratch, first, second))
            }).map(|lanes| lanes.map(finite_vector))
        } else { Err(EvaluationFailure::NoValue) };
        Ok(RequestedJet {
            jet: SurfaceJet {
                point: FinitePoint3::from_coordinates(x, y, z),
                first: first.map(|first| first.lanes.map(finite_vector)),
                second: second.map(|second| second.lanes.map(finite_vector)),
            },
            higher: HigherPartials::Third(third),
        })
    })();
    scratch.settle(result)
}

/// The vector of three finite lanes.
fn finite_vector([x, y, z]: [FiniteReal; 3]) -> FiniteVector3 {
    FiniteVector3::from_components(x, y, z)
}

/// The parametric direction a surface isoline holds fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolineDirection {
    /// `u` is fixed; the curve runs along `v` in the surface's `v` parameter.
    ConstantU,
    /// `v` is fixed; the curve runs along `u` in the surface's `u` parameter.
    ConstantV,
}

/// The isoline of `surface` at `at` in `direction`, as an exact NURBS curve.
///
/// A tensor-product surface restricted to a constant parameter in one direction
/// is a NURBS curve of the free direction's degree over the free direction's
/// knot vector, whose poles are the fixed direction's pole rows blended by the
/// basis at `at`. The result is exact, not a fit; its parameter is the
/// surface's own parameter in the free direction.
pub fn nurbs_surface_isoline<'ctx, 'arena: 'ctx>(
    admission: impl Into<admission::EvaluationAdmission<'ctx, 'arena>>,
    surface: &NurbsSurface,
    direction: IsolineDirection,
    at: f64,
) -> Result<Option<NurbsCurve>, ResourceLimit> {
    let fixed_axis = match direction {
        IsolineDirection::ConstantU => SurfaceParameterAxis::U,
        IsolineDirection::ConstantV => SurfaceParameterAxis::V,
    };
    nurbs_surface_isocurve(admission, surface, fixed_axis, at)
}

/// Extract the exact rational NURBS curve obtained by fixing one parameter of
/// a tensor-product NURBS surface.
pub fn nurbs_surface_isocurve<'ctx, 'arena: 'ctx>(
    admission: impl Into<admission::EvaluationAdmission<'ctx, 'arena>>,
    surface: &NurbsSurface,
    fixed_axis: SurfaceParameterAxis,
    fixed_parameter: f64,
) -> Result<Option<NurbsCurve>, ResourceLimit> {
    let scratch = decode::Scratch::new(admission);
    let result = (|| {
        if scratch.work(0, "IR surface isoline evaluation").is_none() {
            return Ok(None);
        }
        let Some(u_degree) = usize::try_from(surface.u_degree()).ok() else {
            return Ok(None);
        };
        let Some(v_degree) = usize::try_from(surface.v_degree()).ok() else {
            return Ok(None);
        };
        let u_count = surface.u_count();
        let v_count = surface.v_count();
        let (fixed_degree, fixed_count, fixed_knots, fixed_periodic) = match fixed_axis {
            SurfaceParameterAxis::U => (u_degree, u_count, surface.u_knots(), surface.u_periodic()),
            SurfaceParameterAxis::V => (v_degree, v_count, surface.v_knots(), surface.v_periodic()),
        };
        let Some(fixed_parameter) = FiniteReal::new(fixed_parameter).and_then(|parameter| {
            periodic_parameter(
                fixed_knots,
                fixed_degree,
                fixed_count,
                fixed_periodic,
                parameter,
            )
        }) else {
            return Ok(None);
        };
        let fixed_parameter = fixed_parameter.get();
        let Some(fixed_span) = basis::bspline_span(
            scratch.admission,
            fixed_knots,
            fixed_degree,
            fixed_count,
            fixed_parameter,
        )?
        else {
            return Ok(None);
        };

        let Some(fixed_basis) = basis::bspline_basis(
            &scratch,
            fixed_knots,
            fixed_degree,
            fixed_span,
            fixed_parameter,
        ) else {
            scratch.unless_refused()?;
            return Ok(None);
        };
        let varying_count = match fixed_axis {
            SurfaceParameterAxis::U => v_count,
            SurfaceParameterAxis::V => u_count,
        };
        let rational = surface.weight(0, 0).is_some();
        let Some(mut controls) =
            scratch.temporary_vec(varying_count, "IR surface isoline controls")
        else {
            return Ok(None);
        };
        let mut sums = Vec::new();
        if rational
            && scratch
                .reserve(&mut sums, varying_count, "IR surface isoline sums")
                .is_none()
        {
            return Ok(None);
        }
        for varying in 0..varying_count {
            if scratch.work(1, "IR surface isoline pole visit").is_none() {
                return Ok(None);
            }
            let Some(sum) = Homogeneous::sum(
                &scratch,
                fixed_basis
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(local, basis)| {
                        let fixed = fixed_span - fixed_degree + local;
                        let (pole_u, pole_v) = match fixed_axis {
                            SurfaceParameterAxis::U => (fixed, varying),
                            SurfaceParameterAxis::V => (varying, fixed),
                        };
                        Some((
                            [basis, 1.0],
                            surface.weight(pole_u, pole_v).map_or(1.0, NonZeroReal::get),
                            surface.pole(pole_u, pole_v)?,
                        ))
                    }),
            )?
            else {
                return Ok(None);
            };
            let Some(projected) = sum.project(sum, &[]) else {
                return Ok(None);
            };
            let Ok([x, y, z]) = finite_lanes(projected) else {
                return Ok(None);
            };
            if scratch
                .work(
                    std::mem::size_of::<Point3>(),
                    "IR surface isoline point copy",
                )
                .is_none()
            {
                return Ok(None);
            }
            controls.0.push(FinitePoint3::from_coordinates(x, y, z).get());
            if rational {
                if scratch
                    .work(
                        std::mem::size_of::<Homogeneous>(),
                        "IR surface isoline sum copy",
                    )
                    .is_none()
                {
                    return Ok(None);
                }
                sums.push(sum);
            }
        }
        let (degree, knots, periodic) = match fixed_axis {
            SurfaceParameterAxis::U => (
                surface.v_degree(),
                surface.v_knots().as_slice(),
                surface.v_periodic(),
            ),
            SurfaceParameterAxis::V => (
                surface.u_degree(),
                surface.u_knots().as_slice(),
                surface.u_periodic(),
            ),
        };
        let Some(admitted_knots) = scratch.retained_copy(knots, "IR surface isoline knots") else {
            return Ok(None);
        };
        let weights = if rational {
            let Some(weights) = Homogeneous::weights(&scratch, &sums)? else {
                return Ok(None);
            };
            Some(weights)
        } else {
            None
        };
        let curve = match scratch.admission.context() {
            Some(ctx) => NurbsCurve::from_lanes(
                ctx,
                degree,
                admitted_knots,
                controls.0,
                weights,
                periodic,
            )
            .map_err(crate::geometry::nurbs::NurbsError::from)
            .and_then(|curve| curve),
            None => (|| {
                use crate::geometry::nurbs::{
                    admit_weight, build_curve, pair_curve_lanes, StandardNurbsAdmission,
                };
                let poles = pair_curve_lanes(
                    &StandardNurbsAdmission,
                    controls.0,
                    weights,
                    &mut None,
                    |index, weight| admit_weight(&StandardNurbsAdmission, "poles", index, weight),
                )?;
                build_curve(
                    &StandardNurbsAdmission,
                    degree,
                    admitted_knots,
                    poles,
                    periodic,
                )
            })(),
        };
        match curve {
            Ok(curve) => Ok(Some(curve)),
            Err(crate::geometry::nurbs::NurbsError::ResourceLimit(limit)) => Err(limit),
            Err(_) => Ok(None),
        }
    })();
    scratch.unless_refused()?;
    result
}

/// Why an evaluator has no finite value at its input.
///
/// [`NoValue`](Self::NoValue) states that there is no value: the input is
/// not a finite parameter, the carrier states no value there, or a step the
/// value needs is undefined, such as the angle of a polar chart at its
/// origin.
///
/// [`NonFinite`](Self::NonFinite) states that the evaluation left the finite
/// range, and carries the value it reached. A coordinate that no step
/// reached is NaN. Where the value an evaluator returns includes
/// derivatives, as a surface's partials do, a derivative that leaves the
/// finite range leaves the evaluation there, and the point it carries can be
/// finite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EvaluationFailure<R> {
    /// There is no value at the input.
    NoValue,
    /// The evaluation left the finite range and reached this value.
    NonFinite(R),
    /// Scratch storage for admitted geometry was refused.
    ResourceLimit(ResourceLimit),
}

impl<R> EvaluationFailure<R> {
    /// The value a non-finite evaluation reached, absent for an input with no
    /// value.
    pub fn non_finite(self) -> Result<Option<R>, ResourceLimit> {
        match self {
            Self::NoValue => Ok(None),
            Self::NonFinite(value) => Ok(Some(value)),
            Self::ResourceLimit(limit) => Err(limit),
        }
    }

    /// The same failure with the reached value mapped by `reach`.
    pub fn map<S>(self, reach: impl FnOnce(R) -> S) -> EvaluationFailure<S> {
        match self {
            Self::NoValue => EvaluationFailure::NoValue,
            Self::NonFinite(value) => EvaluationFailure::NonFinite(reach(value)),
            Self::ResourceLimit(limit) => EvaluationFailure::ResourceLimit(limit),
        }
    }
}

impl<R> From<ResourceLimit> for EvaluationFailure<R> {
    fn from(limit: ResourceLimit) -> Self {
        Self::ResourceLimit(limit)
    }
}

/// Keep an undefined or non-finite candidate absent and preserve a resource refusal.
pub fn finite_or_refusal<T, R>(
    evaluation: Result<T, EvaluationFailure<R>>,
) -> Result<Option<T>, ResourceLimit> {
    match evaluation {
        Ok(value) => Ok(Some(value)),
        Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(_)) => Ok(None),
        Err(EvaluationFailure::ResourceLimit(limit)) => Err(limit),
    }
}

/// The point of an evaluation that left the finite range before it reached
/// any coordinate.
const UNREACHED_POINT: Point3 = Point3 {
    x: f64::NAN,
    y: f64::NAN,
    z: f64::NAN,
};

/// Admit an evaluated model-space point, or report the non-finite point.
fn admit_point(point: Point3) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    FinitePoint3::new(point).ok_or(EvaluationFailure::NonFinite(point))
}

/// Admit an evaluated parameter-space value, or report the non-finite value.
fn admit_parameter_point(point: Point2) -> Result<FinitePoint2, EvaluationFailure<Point2>> {
    FinitePoint2::new(point).ok_or(EvaluationFailure::NonFinite(point))
}

/// Point and first partial derivatives of a surface in its stored
/// parameterization. An evaluator that admits every lane returns the
/// `SurfacePartials<FinitePoint3, FiniteVector3>` instantiation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfacePartials<P = Point3, V = Vector3> {
    /// Surface point at `(u, v)`.
    pub point: P,
    /// First partial derivative with respect to `u`.
    pub du: V,
    /// First partial derivative with respect to `v`.
    pub dv: V,
}

impl SurfacePartials<FinitePoint3, FiniteVector3> {
    /// The partials with raw lanes, for a reader that computes with them.
    #[must_use]
    pub fn into_raw(self) -> SurfacePartials {
        SurfacePartials {
            point: self.point.get(),
            du: self.du.get(),
            dv: self.dv.get(),
        }
    }
}

/// Point, first partials, and second partials of a surface in its stored
/// parameterization. An evaluator that admits every lane returns the
/// `SurfaceSecondPartials<FinitePoint3, FiniteVector3>` instantiation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceSecondPartials<P = Point3, V = Vector3> {
    /// Surface point at `(u, v)`.
    pub point: P,
    /// First partial derivative with respect to `u`.
    pub du: V,
    /// First partial derivative with respect to `v`.
    pub dv: V,
    /// Second partial derivative with respect to `u`.
    pub duu: V,
    /// Mixed partial derivative.
    pub duv: V,
    /// Second partial derivative with respect to `v`.
    pub dvv: V,
}

impl SurfaceSecondPartials<FinitePoint3, FiniteVector3> {
    /// The partials with raw lanes, for a reader that computes with them.
    #[must_use]
    pub fn into_raw(self) -> SurfaceSecondPartials {
        SurfaceSecondPartials {
            point: self.point.get(),
            du: self.du.get(),
            dv: self.dv.get(),
            duu: self.duu.get(),
            duv: self.duv.get(),
            dvv: self.dvv.get(),
        }
    }
}

/// A surface's finite point with its first and second partials, each order
/// with its own outcome: an order that has no value there, or that left the
/// finite range, leaves the point and the other order as they are. A reader
/// fails on the orders it reads.
#[derive(Clone, Copy)]
struct SurfaceJet {
    point: FinitePoint3,
    first: Result<[FiniteVector3; 2], EvaluationFailure<()>>,
    second: Result<[FiniteVector3; 3], EvaluationFailure<()>>,
}

impl SurfaceJet {
    /// The jet an arm forms at a finite point from raw lanes: an order with a
    /// lane outside the finite range left it.
    fn formed(
        point: FinitePoint3,
        first: Result<[Vector3; 2], EvaluationFailure<()>>,
        second: Result<[Vector3; 3], EvaluationFailure<()>>,
    ) -> Self {
        Self {
            point,
            first: first.and_then(admit_lanes),
            second: second.and_then(admit_lanes),
        }
    }

    /// An order's failure carrying the finite point.
    fn at_point(self, failure: EvaluationFailure<()>) -> EvaluationFailure<Point3> {
        failure.map(|()| self.point.get())
    }

    /// The point with the first partials.
    fn first_order(self) -> SurfaceFirstOrder {
        SurfaceFirstOrder {
            point: self.point,
            first: self.first,
        }
    }

    /// The point with both orders, or the failure of the first order that has
    /// none, carrying the finite point.
    fn second_partials(
        self,
    ) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
        let [du, dv] = self.first.map_err(|failure| self.at_point(failure))?;
        let [duu, duv, dvv] = self.second.map_err(|failure| self.at_point(failure))?;
        Ok(SurfaceSecondPartials {
            point: self.point,
            du,
            dv,
            duu,
            duv,
            dvv,
        })
    }
}

/// A surface's finite point with its first partials, which state their own
/// outcome.
#[derive(Clone, Copy)]
struct SurfaceFirstOrder {
    point: FinitePoint3,
    first: Result<[FiniteVector3; 2], EvaluationFailure<()>>,
}

impl SurfaceFirstOrder {
    /// The point and first partials, or the first partials' failure carrying
    /// the finite point.
    fn partials(
        self,
    ) -> Result<SurfacePartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
        let [du, dv] = self
            .first
            .map_err(|failure| failure.map(|()| self.point.get()))?;
        Ok(SurfacePartials {
            point: self.point,
            du,
            dv,
        })
    }
}

/// Admit the lanes of one partial order together: a lane outside the finite
/// range leaves the order there.
fn admit_lanes<const N: usize>(
    lanes: [Vector3; N],
) -> Result<[FiniteVector3; N], EvaluationFailure<()>> {
    FiniteVector3::array(lanes).ok_or(EvaluationFailure::NonFinite(()))
}

/// Evaluate a tensor-product NURBS surface and its exact rational first
/// partials at `(u, v)`, or report why they have no finite value there.
///
/// The point fails as [`decode::nurbs_surface_point`] states. At a finite point,
/// first partials outside the finite range leave the evaluation there,
/// carrying the point.
pub fn nurbs_surface_partials<'ctx, 'arena: 'ctx>(
    admission: impl Into<admission::EvaluationAdmission<'ctx, 'arena>>,
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<SurfacePartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    let scratch = decode::Scratch::new(admission);
    let result = (|| nurbs_surface_first_order(&scratch, surface, u_at, v_at)?.partials())();
    scratch.settle(result)
}

/// Evaluate a tensor-product NURBS surface and its exact rational first and
/// second partials at `(u, v)`, or report why they have no finite value
/// there.
///
/// The point fails as [`decode::nurbs_surface_point`] states. At a finite point,
/// partials outside the finite range leave the evaluation there, carrying
/// the point.
pub fn nurbs_surface_second_partials<'ctx, 'arena: 'ctx>(
    admission: impl Into<admission::EvaluationAdmission<'ctx, 'arena>>,
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    let scratch = decode::Scratch::new(admission);
    let result = (|| nurbs_surface_requested_jet(&scratch, surface, u_at, v_at, SurfaceRequest::Second)?.jet.second_partials())();
    scratch.settle(result)
}

fn nurbs_surface_partials_evaluation_cost(surface: &NurbsSurface) -> Option<usize> {
    let (u_support, v_support) = nurbs_surface_support_sizes(surface)?;
    let control_work = u_support.checked_mul(v_support)?;
    let u_basis_work = u_support.checked_mul(u_support)?.checked_mul(3)?;
    let v_basis_work = v_support.checked_mul(v_support)?.checked_mul(3)?;
    control_work
        .checked_add(u_basis_work)?
        .checked_add(v_basis_work)
}

fn nurbs_surface_support_sizes(surface: &NurbsSurface) -> Option<(usize, usize)> {
    Some((
        usize::try_from(surface.u_degree()).ok()?.checked_add(1)?,
        usize::try_from(surface.v_degree()).ok()?.checked_add(1)?,
    ))
}

fn periodic_parameter(
    knots: &[f64],
    degree: usize,
    count: usize,
    periodic: bool,
    parameter: FiniteReal,
) -> Option<FiniteReal> {
    let start = *knots.get(degree)?;
    let end = *knots.get(count)?;
    if !periodic || (start..=end).contains(&parameter.get()) {
        return Some(parameter);
    }
    crate::math::wrap_parameter(parameter.get(), start, end)
}

/// Evaluate the exact first derivative of a directly stored curve, or report
/// why it has no finite value.
///
/// A parameter that is not finite, a degenerate carrier, a polyline
/// parameter outside its segments or at a vertex whose segments disagree,
/// and a NURBS span of zero width have no derivative. A derivative that
/// leaves the finite range is non-finite; it carries no value.
pub fn curve_tangent_solved<'ctx, 'arena: 'ctx>(
    admission: impl Into<admission::EvaluationAdmission<'ctx, 'arena>>,
    geometry: &SolvedCurveGeometry,
    t: f64,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    let scratch = decode::Scratch::new(admission);
    let result = curve_derivative_evaluation(&scratch, geometry, t, CurveDerivative::First);
    scratch.settle(result)
}

/// Evaluate the exact second derivative of a directly stored curve, or report
/// why it has no finite value, as [`curve_tangent_solved`] states.
pub fn curve_second_derivative_solved<'ctx, 'arena: 'ctx>(
    admission: impl Into<admission::EvaluationAdmission<'ctx, 'arena>>,
    geometry: &SolvedCurveGeometry,
    t: f64,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    let scratch = decode::Scratch::new(admission);
    let result = curve_derivative_evaluation(&scratch, geometry, t, CurveDerivative::Second);
    scratch.settle(result)
}

fn nurbs_curve_evaluation_cost(curve: &NurbsCurve) -> Option<usize> {
    let support = usize::try_from(curve.degree()).ok()?.checked_add(1)?;
    support.checked_mul(support).filter(|cost| *cost > 0)
}

fn nurbs_curve_derivative_evaluation_cost(
    curve: &NurbsCurve,
    basis_levels: usize,
) -> Option<usize> {
    nurbs_curve_evaluation_cost(curve)?.checked_mul(basis_levels)
}

/// The order of a curve derivative.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CurveDerivative {
    First,
    Second,
}

/// A placed carrier's derivative from its basis derivative. A derivative the
/// placement leaves outside the finite range is non-finite.
fn placed_derivative(
    transform: Transform,
    basis: Result<FiniteVector3, EvaluationFailure<()>>,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    transform
        .apply_vector(basis?.get())
        .ok_or(EvaluationFailure::NonFinite(()))
}

/// Admit a derivative an arm computes raw: a component outside the finite
/// range leaves the derivative there.
fn admit_derivative(derivative: Vector3) -> Result<FiniteVector3, EvaluationFailure<()>> {
    FiniteVector3::new(derivative).ok_or(EvaluationFailure::NonFinite(()))
}

/// The exact derivative of order `order` of a directly stored curve at `t`,
/// or why it has none there.
///
/// A parameter that is not finite, a degenerate carrier, a polyline
/// parameter outside its segments or at a vertex whose segments disagree,
/// and a NURBS span of zero width have no derivative. A derivative that
/// leaves the finite range is non-finite; it carries no value, because its
/// readers carry the point instead.
///
/// The descent is bounded by [`PlacedCurve`](crate::geometry::PlacedCurve)
/// construction; no arm follows an arena id.
fn curve_derivative_evaluation(
    scratch: &decode::Scratch<'_, '_>,
    geometry: &SolvedCurveGeometry,
    t: f64,
    order: CurveDerivative,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    match geometry {
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let levels = if order == CurveDerivative::First {
                2
            } else {
                3
            };
            scratch
                .admission
                .independent_cost(nurbs_curve_derivative_evaluation_cost(nurbs, levels))?;
        }
        SolvedCurveGeometry::Polyline(polyline) => scratch
            .admission
            .independent_cost(Some(polyline.point_count()))?,
        SolvedCurveGeometry::Transformed(_) => scratch.admission.independent_cost(Some(1))?,
        _ => {}
    }
    scratch.settle(curve_derivative_unsettled(scratch, geometry, t, order))
}

fn curve_derivative_unsettled(
    scratch: &decode::Scratch<'_, '_>,
    geometry: &SolvedCurveGeometry,
    t: f64,
    order: CurveDerivative,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    let _depth = scratch.enter().ok_or(EvaluationFailure::NoValue)?;
    let parameter = FiniteReal::new(t).ok_or(EvaluationFailure::NoValue)?;
    let t = parameter.get();
    let second = order == CurveDerivative::Second;
    match geometry {
        SolvedCurveGeometry::Line(line_curve) => Ok(if second {
            FiniteVector3::ZERO
        } else {
            FiniteVector3::from(line_curve.direction())
        }),
        SolvedCurveGeometry::Circle(circle_curve) => {
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius().get();
            admit_derivative(vector_sum(&if second {
                [
                    (-radius * t.cos(), *ref_direction),
                    (-radius * t.sin(), axis.cross(*ref_direction)),
                ]
            } else {
                [
                    (-radius * t.sin(), *ref_direction),
                    (radius * t.cos(), axis.cross(*ref_direction)),
                ]
            }))
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            admit_derivative(vector_sum(&if second {
                [
                    (-major_radius * t.cos(), *major_direction),
                    (-minor_radius * t.sin(), axis.cross(*major_direction)),
                ]
            } else {
                [
                    (-major_radius * t.sin(), *major_direction),
                    (minor_radius * t.cos(), axis.cross(*major_direction)),
                ]
            }))
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let axis = parabola_curve.frame().axis().as_raw();
            let major_direction = parabola_curve.frame().reference().as_raw();
            let focal_distance = parabola_curve.focal_distance().get();
            // The products of finite factors are absent only where they
            // overflow.
            let product = |value: Option<FiniteReal>| {
                value
                    .map(FiniteReal::get)
                    .ok_or(EvaluationFailure::NonFinite(()))
            };
            let curvature = product(product_quotient([2.0, focal_distance], []))?;
            admit_derivative(if second {
                vector_sum(&[(curvature, *major_direction)])
            } else {
                vector_sum(&[
                    (
                        product(product_quotient([2.0, focal_distance, t], []))?,
                        *major_direction,
                    ),
                    (curvature, axis.cross(*major_direction)),
                ])
            })
        }
        SolvedCurveGeometry::Hyperbola(hyperbola_curve) => {
            let axis = hyperbola_curve.frame().axis().as_raw();
            let major_direction = hyperbola_curve.frame().reference().as_raw();
            let major_radius = hyperbola_curve.major_radius().magnitude();
            let minor_radius = Length::from(hyperbola_curve.minor_radius()).magnitude();
            // A product the derivative reads outside the finite range leaves
            // the derivative there.
            let [major_sinh, major_cosh] =
                sinh_cosh_lanes(scaled_sinh_cosh(major_radius, parameter));
            let [minor_sinh, minor_cosh] =
                sinh_cosh_lanes(scaled_sinh_cosh(minor_radius, parameter));
            let (major, minor) = if second {
                (major_cosh, minor_sinh)
            } else {
                (major_sinh, minor_cosh)
            };
            let (Ok(major), Ok(minor)) = (major, minor) else {
                return Err(EvaluationFailure::NonFinite(()));
            };
            admit_derivative(vector_sum(&[
                (major.get(), *major_direction),
                (minor.get(), axis.cross(*major_direction)),
            ]))
        }
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let parameter =
                map_nurbs_curve_parameter(nurbs, parameter).ok_or(EvaluationFailure::NoValue)?;
            let poles = nurbs.pole_rows();
            let points = scratch
                .collect(
                    (0..poles.count()).map(|index| poles.point_at(index)),
                    "IR NURBS derivative points",
                    "IR NURBS derivative points work",
                )
                .ok_or(EvaluationFailure::NoValue)?;
            let weights = match poles {
                crate::geometry::nurbs::NurbsPoles3::Polynomial { .. } => None,
                crate::geometry::nurbs::NurbsPoles3::Rational { points } => Some(
                    scratch
                        .collect(
                            points.iter().map(|pole| Some(pole.weight.get())),
                            "IR NURBS derivative weights",
                            "IR NURBS derivative weights work",
                        )
                        .ok_or(EvaluationFailure::NoValue)?,
                ),
            };
            nurbs_curve_derivative(
                scratch,
                nurbs.degree(),
                nurbs.knots(),
                &points,
                weights.as_deref(),
                parameter,
                order,
            )
        }
        SolvedCurveGeometry::Polyline(polyline) => {
            let tangent = polyline::polyline_tangent(
                scratch.admission,
                polyline.point_count(),
                |index| polyline.point_at(index),
                |index| polyline.parameter_at(index),
                t,
            )?;
            Ok(if second { FiniteVector3::ZERO } else { tangent })
        }
        SolvedCurveGeometry::Transformed(placed) => {
            scratch
                .work(1, "placed geometry evaluation step")
                .ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
            placed_derivative(
                *placed.transform(),
                curve_derivative_evaluation(scratch, placed.basis(), t, order),
            )
        }
        SolvedCurveGeometry::Degenerate(_)
        | SolvedCurveGeometry::Composite { .. }
        | SolvedCurveGeometry::Unknown { .. } => Err(EvaluationFailure::NoValue),
    }
}

/// The products `scale * sinh(parameter)` and `scale * cosh(parameter)` of a
/// [`scaled_sinh_cosh`] pair, each finite or the plain product it reached. A
/// pair outside the finite range carries both plain products, so one of
/// them can be finite.
fn sinh_cosh_lanes(
    pair: Result<(FiniteReal, FiniteReal), (f64, f64)>,
) -> [Result<FiniteReal, f64>; 2] {
    match pair {
        Ok((sinh, cosh)) => [Ok(sinh), Ok(cosh)],
        Err((sinh, cosh)) => [
            FiniteReal::new(sinh).ok_or(sinh),
            FiniteReal::new(cosh).ok_or(cosh),
        ],
    }
}

/// The derivative of order `order` at `t` of a possibly-rational B-spline
/// curve, or why it has none there.
///
/// At a finite parameter over finite knots, a basis that is absent or not
/// finite, and a derivative basis the span's scale cannot bring into range,
/// left the finite range, as did a point or derivative projection that
/// overflows. A knot vector or pole list that states no span at `t`, a zero
/// weight sum, and a span of zero width have no value.
fn nurbs_curve_derivative(
    scratch: &decode::Scratch<'_, '_>,
    degree: u32,
    knots: &[f64],
    control_points: &[FinitePoint3],
    weights: Option<&[f64]>,
    t: FiniteReal,
    order: CurveDerivative,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    scratch.settle(nurbs_curve_derivative_unsettled(
        scratch,
        degree,
        knots,
        control_points,
        weights,
        t,
        order,
    ))
}

fn nurbs_curve_derivative_unsettled(
    scratch: &decode::Scratch<'_, '_>,
    degree: u32,
    knots: &[f64],
    control_points: &[FinitePoint3],
    weights: Option<&[f64]>,
    t: FiniteReal,
    order: CurveDerivative,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    let t = t.get();
    let no_value = EvaluationFailure::NoValue;
    let non_finite = EvaluationFailure::NonFinite(());
    let second = order == CurveDerivative::Second;
    let degree = usize::try_from(degree).map_err(|_| no_value)?;
    let span = scratch
        .admit(basis::bspline_span(
            scratch.admission,
            knots,
            degree,
            control_points.len(),
            t,
        ))
        .flatten()
        .ok_or(no_value)?;
    let basis = basis::bspline_basis(scratch, knots, degree, span, t).ok_or(non_finite)?;
    if !basis::all_finite(scratch, &basis).ok_or_else(|| scratch.failure(non_finite))? {
        return Err(non_finite);
    }
    let mut first_basis =
        basis::bspline_basis_derivative(scratch, knots, degree, span, t).ok_or(non_finite)?;
    let mut second_basis = if second {
        basis::bspline_basis_second_derivative(scratch, knots, degree, span, t).ok_or(non_finite)?
    } else {
        Cow::Borrowed(&[][..])
    };
    let scale = if basis::all_finite(scratch, &first_basis)
        .ok_or_else(|| scratch.failure(non_finite))?
        && basis::all_finite(scratch, &second_basis).ok_or_else(|| scratch.failure(non_finite))?
    {
        PositiveReal::ONE
    } else {
        let width = knots[span + 1] - knots[span];
        let scale = PositiveReal::new(width).ok_or(if width.is_finite() {
            no_value
        } else {
            non_finite
        })?;
        if degree == 1 {
            let [x, y, z] = finite_lanes(
                linear_nurbs_derivative(&basis, control_points, weights, span, scale.get(), second)
                    .ok_or(no_value)?,
            )
            .map_err(|_| non_finite)?;
            return Ok(FiniteVector3::from_components(x, y, z));
        }
        let scaled =
            basis::bspline_basis_scaled_derivatives(scratch, knots, degree, span, t, scale)
                .ok_or(non_finite)?;
        first_basis = scaled.first;
        if second {
            second_basis = Cow::Owned(scaled.second);
        }
        scale
    };
    // A sum is absent only where one of its derivative basis terms is not
    // finite.
    let sum = |values: &[f64]| {
        homogeneous_curve_sum(
            scratch,
            values,
            |index| control_points.get(index).copied(),
            |index| weights.and_then(|weights| weights.get(index).copied()),
            span - degree,
        )
        .ok_or(non_finite)
    };
    let base = sum(&basis)?;
    let first_sum = sum(&first_basis)?;
    let point = finite_lanes(base.project(base, &[]).ok_or(no_value)?).map_err(|_| non_finite)?;
    let first = finite_lanes(
        first_sum
            .project(base, &[(first_sum, point)])
            .ok_or(no_value)?,
    )
    .map_err(|_| non_finite)?;
    let (lanes, divisions) = if second {
        let second_sum = sum(&second_basis)?;
        let lanes = finite_lanes(
            second_sum
                .project(
                    base,
                    &[(second_sum, point), (first_sum, first), (first_sum, first)],
                )
                .ok_or(no_value)?,
        )
        .map_err(|_| non_finite)?;
        (lanes, 2)
    } else {
        (first, 1)
    };
    // Each derivative lane over the span divides by the positive scale once
    // per order; a quotient that overflows leaves the derivative there.
    let unscale = |value: FiniteReal| {
        if scale.get() == 1.0 {
            return Ok(value);
        }
        (0..divisions).try_fold(value, |value, _| {
            difference_quotient(value, FiniteReal::ZERO, scale.into(), FiniteReal::ZERO)
                .map_err(|failure| failure.map(|_| ()))
        })
    };
    let [x, y, z] = lanes;
    Ok(FiniteVector3::from_components(
        unscale(x)?,
        unscale(y)?,
        unscale(z)?,
    ))
}

/// The degree-one rational derivative has a two-pole quotient form. Form its
/// numerator as exact products before dividing by the span and homogeneous
/// weight; neither derivative basis coefficient needs to fit in `f64`.
///
/// Each lane is the derivative's coordinate, or the signed infinity of a
/// coordinate whose quotient overflows.
fn linear_nurbs_derivative(
    basis: &[f64],
    control_points: &[FinitePoint3],
    weights: Option<&[f64]>,
    span: usize,
    width: f64,
    second: bool,
) -> Option<[Result<FiniteReal, f64>; 3]> {
    use crate::math::sum::{product_sum, scaled_finite, ProductSum};
    let start = span.checked_sub(1)?;
    let first = *control_points.get(start)?;
    let last = *control_points.get(span)?;
    let weight0 = weights
        .and_then(|weights| weights.get(start))
        .copied()
        .unwrap_or(1.0);
    let weight1 = weights
        .and_then(|weights| weights.get(span))
        .copied()
        .unwrap_or(1.0);
    let ProductSum::Value(base_weight) =
        product_sum([Some([basis[0], weight0]), Some([basis[1], weight1])].into_iter())
    else {
        return None;
    };
    let width = scaled_finite(width)?;
    let coordinate = |left: f64, right: f64| {
        let mut sum = ExactSignedSum::default();
        if second {
            // -w0*w1*(w1-w0)*(right-left); the factor 2 is an exponent shift.
            sum.add_factors([-weight0, weight1, weight1, right]);
            sum.add_factors([weight0, weight1, weight1, left]);
            sum.add_factors([weight0, weight1, weight0, right]);
            sum.add_factors([-weight0, weight1, weight0, left]);
            sum.finish().map_or(Ok(FiniteReal::ZERO), |numerator| {
                numerator.quotient_by_factors(
                    [base_weight, base_weight, base_weight, width, width],
                    true,
                )
            })
        } else {
            sum.add_factors([weight0, weight1, right]);
            sum.add_factors([-weight0, weight1, left]);
            sum.finish().map_or(Ok(FiniteReal::ZERO), |numerator| {
                numerator.quotient_by_factors([base_weight, base_weight, width], false)
            })
        }
    };
    Some([
        coordinate(first.x, last.x),
        coordinate(first.y, last.y),
        coordinate(first.z, last.z),
    ])
}

/// Evaluate a curve carrier selected by arena id, including supported
/// procedural constructions, or report why it has no finite point there.
pub fn model_curve_point_by_id(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    admission.within_model(|admission| {
        model_curve_point_by_id_inner(admission, index, curve_id, parameter)
    })
}

/// The orders a construction reads from a model curve.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ModelCurveRequest {
    Point,
    First,
    Second,
}

impl ModelCurveRequest {
    fn for_surface_partials(request: SurfaceRequest) -> Self {
        if request.needs_second() { Self::Second } else { Self::First }
    }
}

/// A model curve's finite point with its tangent and acceleration, each
/// derivative with its own outcome: a derivative that has no value there, or
/// that left the finite range, leaves the point and the other derivative as
/// they are. A reader fails on the derivatives it reads.
#[derive(Clone, Copy)]
struct ModelCurveDifferential {
    point: FinitePoint3,
    tangent: Result<FiniteVector3, EvaluationFailure<()>>,
    acceleration: Result<FiniteVector3, EvaluationFailure<()>>,
}

impl ModelCurveDifferential {
    /// The tangent, or its failure carrying the finite point, for a reader
    /// that fails on it.
    fn tangent(&self) -> Result<FiniteVector3, EvaluationFailure<Point3>> {
        self.tangent
            .map_err(|failure| failure.map(|()| self.point.get()))
    }
}

/// The differential of a curve whose point is `point` and whose derivatives
/// are evaluated separately, each once the point is finite.
fn differential_at(
    point: Result<FinitePoint3, EvaluationFailure<Point3>>,
    tangent: impl FnOnce() -> Result<FiniteVector3, EvaluationFailure<()>>,
    acceleration: impl FnOnce() -> Result<FiniteVector3, EvaluationFailure<()>>,
) -> Result<ModelCurveDifferential, EvaluationFailure<Point3>> {
    let point = point?;
    let tangent = tangent();
    if let Err(EvaluationFailure::ResourceLimit(limit)) = tangent {
        return Err(EvaluationFailure::ResourceLimit(limit));
    }
    let acceleration = acceleration();
    if let Err(EvaluationFailure::ResourceLimit(limit)) = acceleration {
        return Err(EvaluationFailure::ResourceLimit(limit));
    }
    Ok(ModelCurveDifferential {
        point,
        tangent,
        acceleration,
    })
}

/// Evaluate the native helix path and its exact angle derivatives.
///
/// The stored angular interval is the domain of the path parameter. The
/// pitch and apex terms advance by the fraction of one full revolution from
/// the interval's lower bound, while the major and minor vectors define the
/// radial frame at the stored angle.
///
/// A parameter outside the interval and a zero axis have no value. The point
/// reads no derivative, so a tangent or acceleration outside the finite
/// range is that derivative's own failure.
fn helix_differential(
    definition: &ProceduralCurveDefinition,
    parameter: f64,
) -> Result<ModelCurveDifferential, EvaluationFailure<Point3>> {
    let ProceduralCurveDefinition::Helix(helix_payload) = definition else {
        return Err(EvaluationFailure::NoValue);
    };
    let [start, end] = helix_payload.angle_range().finite_components();
    let center = helix_payload.center().get();
    let major = helix_payload.major().get();
    let minor = helix_payload.minor().get();
    let pitch = helix_payload.pitch().get();
    let apex_factor = helix_payload.apex_factor().get();
    let finite_parameter = FiniteReal::new(parameter).ok_or(EvaluationFailure::NoValue)?;
    if finite_parameter < start || finite_parameter > end {
        return Err(EvaluationFailure::NoValue);
    }
    let inverse_revolution = 1.0 / std::f64::consts::TAU;
    let revolution_fraction = finite_parameter.turns_from(start).get();
    let radial_scale = 1.0 + apex_factor * revolution_fraction;
    let radial = vector_sum(&[(parameter.cos(), major), (parameter.sin(), minor)]);
    let radial_first = vector_sum(&[(-parameter.sin(), major), (parameter.cos(), minor)]);
    let point = offset(
        center,
        &[(radial_scale, radial), (revolution_fraction, pitch)],
    );
    let scale_first = apex_factor * inverse_revolution;
    let tangent = vector_sum(&[
        (radial_scale, radial_first),
        (scale_first, radial),
        (inverse_revolution, pitch),
    ]);
    let acceleration = vector_sum(&[(-radial_scale, radial), (2.0 * scale_first, radial_first)]);
    Ok(ModelCurveDifferential {
        point: admit_point(point)?,
        tangent: admit_derivative(tangent),
        acceleration: admit_derivative(acceleration),
    })
}

fn model_curve_differential_by_id(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
    request: ModelCurveRequest,
) -> Result<ModelCurveDifferential, EvaluationFailure<Point3>> {
    admission.within_model(|admission| {
        model_curve_differential_by_id_inner(admission, index, curve_id, parameter, request)
    })
}

/// The point, tangent and acceleration of a model curve at `parameter`, or
/// why its point has no finite value there. At a finite point each
/// derivative states its own outcome: no value where the curve has no
/// derivative there, non-finite where the derivative left the finite range.
fn model_curve_differential_by_id_inner(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
    request: ModelCurveRequest,
) -> Result<ModelCurveDifferential, EvaluationFailure<Point3>> {
    let budget = admission.work_slice();
    let scratch = decode::Scratch::new(admission);
    let result = (|| {
        let depth_guard =
            ModelEvaluationDepthGuard::enter(budget).map_err(EvaluationFailure::ResourceLimit)?;
        if !parameter.is_finite() {
            return Err(EvaluationFailure::NoValue);
        }
        let curve = index
            .curves(curve_id.as_str(), admission)
            .map_err(EvaluationFailure::ResourceLimit)?
            .ok_or(EvaluationFailure::NoValue)?;
        admission.model_step()?;
        if !depth_guard.bind(
            ModelEvaluationIdentity::Curve(std::ptr::from_ref(curve)),
            admission,
        ) {
            return Err(EvaluationFailure::NoValue);
        }
        if let Some(procedural) = index
            .procedural_curves_for_curve(curve_id.as_str(), admission)
            .map_err(EvaluationFailure::ResourceLimit)?
            .and_then(|procedurals| procedurals.first().copied())
        {
            match procedural.definition() {
                ProceduralCurveDefinition::Replica { source, transform } => {
                    let differential =
                        model_curve_differential_by_id_inner(admission, index, source, parameter, request)
                            .map_err(|failure| {
                            failure.map(|point| {
                                transform
                                    .apply_point_reaching(point)
                                    .map_or_else(|point| point, FinitePoint3::get)
                            })
                        })?;
                    let point = transform
                        .apply_point_reaching(differential.point.get())
                        .map_err(EvaluationFailure::NonFinite)?;
                    return Ok(ModelCurveDifferential {
                        point,
                        tangent: placed_derivative(*transform, differential.tangent),
                        acceleration: placed_derivative(*transform, differential.acceleration),
                    });
                }
                ProceduralCurveDefinition::Subset(definition_payload) => {
                    let source = definition_payload.source();
                    let sense = definition_payload.sense();
                    let source_parameter = subset_source_parameter(
                        *definition_payload.parameter_range(),
                        *sense,
                        parameter,
                    )?;
                    let differential = model_curve_differential_by_id_inner(
                        admission,
                        index,
                        source,
                        source_parameter,
                        request,
                    )?;
                    return Ok(ModelCurveDifferential {
                        point: differential.point,
                        tangent: differential.tangent.map(|tangent| {
                            if *sense {
                                tangent
                            } else {
                                tangent.negated()
                            }
                        }),
                        acceleration: differential.acceleration,
                    });
                }
                ProceduralCurveDefinition::Helix(_) => {
                    return helix_differential(procedural.definition(), parameter);
                }
                _ => {}
            }
        }
        let solved = if let Some(cache) = curve.geometry.solved_cache() {
            cache
        } else {
            match &curve.geometry {
                CurveGeometry::Solved(solved) => solved,
                CurveGeometry::Procedural { .. } => return Err(EvaluationFailure::NoValue),
            }
        };
        let point = crate::eval::decode::curve_point_solved(admission, solved, parameter);
        if request == ModelCurveRequest::Point {
            return point.map(|point| ModelCurveDifferential {
                point,
                tangent: Err(EvaluationFailure::NoValue),
                acceleration: Err(EvaluationFailure::NoValue),
            });
        }
        differential_at(
            point,
            || curve_derivative_evaluation(&scratch, solved, parameter, CurveDerivative::First),
            || if request == ModelCurveRequest::Second {
                curve_derivative_evaluation(&scratch, solved, parameter, CurveDerivative::Second)
            } else { Err(EvaluationFailure::NoValue) },
        )
    })();
    scratch.settle(result)
}

/// Map a nonnegative local subset parameter into its ordered source range.
fn subset_source_parameter(
    interval: ParameterInterval,
    sense: bool,
    parameter: f64,
) -> Result<f64, EvaluationFailure<Point3>> {
    let interval =
        IncreasingParameterInterval::new(interval.endpoints()).ok_or(EvaluationFailure::NoValue)?;
    if !parameter.is_finite() || parameter < 0.0 {
        return Err(EvaluationFailure::NoValue);
    }
    if interval
        .scaled_span()
        .finite()
        .is_ok_and(|span| parameter > span.get())
    {
        return Err(EvaluationFailure::NoValue);
    }
    let [start, end] = interval.endpoints();
    Ok(if sense {
        start + parameter
    } else {
        end - parameter
    })
}

/// The admitted revolution axis scaled by the reciprocal of its length. The
/// admission holds the length within `1e-9` of one without rescaling, and the
/// rotation needs unit length to rounding.
fn unit_length_axis(direction: UnitVector3) -> Vector3 {
    let direction = *direction.as_raw();
    scale_vector(direction, 1.0 / direction.norm())
}

fn rotate_vector_about_axis(vector: Vector3, axis: Vector3, angle: f64) -> Vector3 {
    let cosine = angle.cos();
    let sine = angle.sin();
    vector_sum(&[
        (cosine, vector),
        (sine, axis.cross(vector)),
        (axis.dot(vector) * (1.0 - cosine), axis),
    ])
}

/// `point` rotated by `angle` about the unit `axis` through `axis_origin`.
fn revolved_point(point: Point3, axis_origin: Point3, axis: Vector3, angle: f64) -> Point3 {
    offset(
        axis_origin,
        &[(
            1.0,
            rotate_vector_about_axis(point_displacement(point, axis_origin), axis, angle),
        )],
    )
}

/// The point of a directrix revolved about an axis, or why it has none. A
/// directrix point outside the finite range leaves the surface there, at the
/// point its revolution reaches.
fn model_axis_revolution_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    directrix: &crate::ids::CurveId,
    axis_origin: Point3,
    axis_direction: UnitVector3,
    angle: f64,
    parameter: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    if !angle.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let axis = unit_length_axis(axis_direction);
    let point = model_curve_point_by_id(admission, index, directrix, parameter)
        .map_err(|failure| failure.map(|point| revolved_point(point, axis_origin, axis, angle)))?;
    admit_point(revolved_point(point.get(), axis_origin, axis, angle))
}

/// The jet of a directrix revolved about an axis, or why its point has none.
/// A directrix point outside the finite range leaves the surface there, at
/// the point its revolution reaches. The first partials read the directrix
/// tangent; the second read its acceleration as well.
fn model_axis_revolution_jet(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    directrix: &crate::ids::CurveId,
    axis_origin: Point3,
    axis_direction: UnitVector3,
    angle: f64,
    parameter: f64,
    request: SurfaceRequest,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    if !angle.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let axis = unit_length_axis(axis_direction);
    let differential = model_curve_differential_by_id_inner(admission, index, directrix, parameter, ModelCurveRequest::for_surface_partials(request))
        .map_err(|failure| failure.map(|point| revolved_point(point, axis_origin, axis, angle)))?;
    let rotated = rotate_vector_about_axis(
        point_displacement(differential.point.get(), axis_origin),
        axis,
        angle,
    );
    let point = admit_point(offset(axis_origin, &[(1.0, rotated)]))?;
    let du = axis.cross(rotated);
    let rotated_tangent = differential
        .tangent
        .map(|tangent| rotate_vector_about_axis(tangent.get(), axis, angle));
    let rotated_acceleration = differential
        .acceleration
        .map(|acceleration| rotate_vector_about_axis(acceleration.get(), axis, angle));
    Ok(SurfaceJet::formed(
        point,
        rotated_tangent.map(|tangent| [du, tangent]),
        rotated_tangent
            .and_then(|tangent| Ok([axis.cross(du), axis.cross(tangent), rotated_acceleration?])),
    ))
}

/// Map a construction-space directrix parameter to the carrier curve and
/// return the carrier derivative with respect to the construction parameter.
///
/// IGES line entities use a normalized surface interval while the neutral
/// line carrier uses signed distance. Other curve carriers retain their native
/// parameterization. A line used by more than one edge is only unambiguous
/// when every retained edge range agrees, unless the construction stores its
/// neutral carrier interval explicitly.
fn record_u_interval(
    record_bounds: Option<crate::geometry::RecordBounds>,
) -> Option<[FiniteReal; 2]> {
    let [Some(start), Some(end), _, _] = record_bounds?.finite_values() else {
        return None;
    };
    Some([start, end])
}

/// The descent is bounded by [`PlacedCurve`](crate::geometry::PlacedCurve)
/// construction; no arm follows an arena id.
fn is_line_geometry(geometry: &SolvedCurveGeometry) -> bool {
    match geometry {
        SolvedCurveGeometry::Line(_) => true,
        SolvedCurveGeometry::Transformed(placed) => is_line_geometry(placed.basis()),
        _ => false,
    }
}

/// A construction parameter mapped onto its carrier curve.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ConstructionParameter {
    /// The carrier parameter.
    parameter: FiniteReal,
    /// The carrier parameter's derivative with respect to the construction
    /// parameter, or why it has no finite value. A line carrier forms it
    /// apart from the parameter, which does not need it.
    derivative: Result<FiniteReal, EvaluationFailure<()>>,
}

/// The carrier parameter of a construction parameter and its derivative, or
/// why there is no carrier parameter. A parameter that is not finite or lies
/// outside its interval, an interval that is empty or reversed, and a line
/// carrier whose edge ranges disagree have no value. A width or mapped
/// parameter that overflows, and a derivative the mapping reads, leave the
/// finite range; the evaluation reaches no coordinate there.
fn construction_curve_parameter(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    directrix: &crate::ids::CurveId,
    parameter: f64,
    surface_interval: Option<[FiniteReal; 2]>,
    carrier_interval: Option<[FiniteReal; 2]>,
    reversed: bool,
) -> Result<ConstructionParameter, EvaluationFailure<()>> {
    let no_value = EvaluationFailure::NoValue;
    let non_finite = EvaluationFailure::NonFinite(());
    let overflow = |value: f64| FiniteReal::new(value).ok_or(non_finite);
    let parameter = FiniteReal::new(parameter).ok_or(no_value)?;
    // The surface width is admitted positive where it is formed, and only
    // the arms that hold a surface interval form it.
    let positive_width = |start: FiniteReal, end: FiniteReal| {
        let width = overflow(end.get() - start.get())?;
        if width.get() > 0.0 {
            Ok(width)
        } else {
            Err(no_value)
        }
    };
    let (parameter, surface_derivative, surface_width) = match (surface_interval, carrier_interval)
    {
        (Some([surface_start, surface_end]), Some([carrier_start, carrier_end])) => {
            let surface_width = positive_width(surface_start, surface_end)?;
            let carrier_width = carrier_end.get() - carrier_start.get();
            if carrier_width <= 0.0
                || parameter.get() < carrier_start.get()
                || parameter.get() > carrier_end.get()
            {
                return Err(no_value);
            }
            let carrier_width = overflow(carrier_width)?;
            let derivative = overflow(surface_width.get() / carrier_width.get())?;
            let source_parameter = overflow(
                (parameter.get() - carrier_start.get())
                    .mul_add(derivative.get(), surface_start.get()),
            )?;
            (source_parameter, derivative, Some(surface_width))
        }
        (Some([surface_start, surface_end]), None) => {
            let surface_width = positive_width(surface_start, surface_end)?;
            if parameter.get() < surface_start.get() || parameter.get() > surface_end.get() {
                return Err(no_value);
            }
            (parameter, FiniteReal::ONE, Some(surface_width))
        }
        (None, Some([carrier_start, carrier_end])) => {
            if carrier_start.get() >= carrier_end.get()
                || parameter.get() < carrier_start.get()
                || parameter.get() > carrier_end.get()
            {
                return Err(no_value);
            }
            (parameter, FiniteReal::ONE, None)
        }
        (None, None) => (parameter, FiniteReal::ONE, None),
    };
    let curve = index
        .curves(directrix.as_str(), admission)
        .map_err(EvaluationFailure::ResourceLimit)?
        .ok_or(no_value)?;
    let directed = |parameter: FiniteReal, derivative: FiniteReal| {
        Ok(if reversed {
            ConstructionParameter {
                parameter: parameter.negated(),
                derivative: Ok(derivative.negated()),
            }
        } else {
            ConstructionParameter {
                parameter,
                derivative: Ok(derivative),
            }
        })
    };
    let (Some([surface_start, surface_end]), Some(surface_width)) =
        (surface_interval, surface_width)
    else {
        return directed(parameter, surface_derivative);
    };
    if !curve.geometry.solved().is_some_and(is_line_geometry) {
        return directed(parameter, surface_derivative);
    }
    let line_interval = if let Some(carrier_interval) = carrier_interval {
        carrier_interval
    } else {
        let mut interval = None;
        let mut edges = index.ir().model.edges.iter();
        while edges.len() != 0 {
            admission.independent_cost(Some(1))?;
            admission
                .work(1, "construction directrix edge scan")
                .map_err(EvaluationFailure::ResourceLimit)?;
            let Some(edge) = edges.next() else {
                break;
            };
            let Some(curve) = edge.curve() else {
                continue;
            };
            if !crate::ids::comparison::equal(
                &admission,
                curve.as_str(),
                directrix.as_str(),
                "construction directrix identity comparison",
            )
            .map_err(EvaluationFailure::ResourceLimit)? {
                continue;
            }
            let Some(range) = edge.param_range() else {
                continue;
            };
            if let Some(first) = interval {
                if range != first {
                    return Err(no_value);
                }
            } else {
                interval = Some(range);
            }
        }
        interval.ok_or(no_value)?.finite_components()
    };
    let [curve_start, curve_end] = line_interval;
    let curve_width = positive_width(curve_start, curve_end)?;
    // An absent product is one that no finite value holds.
    let derivative = scaled_ratio_products(curve_width, surface_width, [surface_derivative])
        .map(|[derivative]| {
            if reversed {
                derivative.negated()
            } else {
                derivative
            }
        })
        .ok_or(non_finite);
    let fraction = if reversed {
        (surface_end.get() - parameter.get()) / surface_width.get()
    } else {
        (parameter.get() - surface_start.get()) / surface_width.get()
    };
    let parameter = overflow(curve_start.get() + fraction * curve_width.get())?;
    Ok(ConstructionParameter {
        parameter,
        derivative,
    })
}

/// The point of a native extrusion: the directrix point at the carrier
/// parameter, displaced by `v` along the extrusion direction, or why it has
/// none. The point fails on no derivative. A `v` that is not finite has no
/// value; a directrix point outside the finite range leaves the surface
/// there, at the point its displacement reaches.
fn model_native_extrusion_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::ExtrusionSurfaceConstruction,
    carrier_interval: Option<[FiniteReal; 2]>,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    if !v.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let directrix = construction.directrix();
    let direction = construction.direction().get();
    let carrier = construction_curve_parameter(
        admission,
        index,
        directrix,
        u,
        construction
            .parameter_interval()
            .map(FiniteVector::finite_components),
        carrier_interval,
        extrusion_directrix_reversed(construction.revision_form()),
    )
    .map_err(|failure| failure.map(|()| UNREACHED_POINT))?;
    let point = model_curve_differential_by_id_inner(
        admission, index, directrix, carrier.parameter.get(), ModelCurveRequest::Point,
    ).map_err(|failure| failure.map(|point| offset(point, &[(v, direction)])))?.point;
    admit_point(offset(point.get(), &[(v, direction)]))
}

// The native and carrier parameter intervals are independent serialized semantics;
// The revision reversal affects the derivative mapping. Recursive evaluation
// shares the selected admission policy.
fn model_native_extrusion_jet(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::ExtrusionSurfaceConstruction,
    carrier_interval: Option<[FiniteReal; 2]>,
    u: f64,
    v: f64,
    request: SurfaceRequest,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    if !v.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let directrix = construction.directrix();
    let direction = construction.direction().get();
    let carrier = construction_curve_parameter(
        admission,
        index,
        directrix,
        u,
        construction
            .parameter_interval()
            .map(FiniteVector::finite_components),
        carrier_interval,
        extrusion_directrix_reversed(construction.revision_form()),
    )
    .map_err(|failure| failure.map(|()| UNREACHED_POINT))?;
    // A directrix point that leaves the finite range leaves the surface
    // there, at the point its extrusion reaches.
    let differential =
        model_curve_differential_by_id_inner(admission, index, directrix, carrier.parameter.get(), ModelCurveRequest::for_surface_partials(request))
            .map_err(|failure| failure.map(|point| offset(point, &[(v, direction)])))?;
    let point = admit_point(offset(differential.point.get(), &[(v, direction)]))?;
    let derivative = carrier.derivative;
    // A product outside the finite range reaches its signed infinity; a
    // component that is not finite reaches no product.
    let squared_derivative_component =
        |derivative: FiniteReal, component| match crate::math::sum::product_sum(std::iter::once(
            Some([derivative.get(), derivative.get(), component]),
        )) {
            crate::math::sum::ProductSum::Zero => 0.0,
            crate::math::sum::ProductSum::Value(value) => value
                .finite()
                .map_or_else(|reached| reached, FiniteReal::get),
            crate::math::sum::ProductSum::Undefined => f64::NAN,
        };
    Ok(SurfaceJet {
        point,
        first: derivative.and_then(|derivative| {
            Ok([
                admit_derivative(scale_vector(differential.tangent?.get(), derivative.get()))?,
                *construction.direction(),
            ])
        }),
        second: derivative.and_then(|derivative| {
            let acceleration = differential.acceleration?.get();
            Ok([
                admit_derivative(Vector3::new(
                    squared_derivative_component(derivative, acceleration.x),
                    squared_derivative_component(derivative, acceleration.y),
                    squared_derivative_component(derivative, acceleration.z),
                ))?,
                FiniteVector3::ZERO,
                FiniteVector3::ZERO,
            ])
        }),
    })
}

fn extrusion_directrix_reversed(
    revision_form: Option<&crate::geometry::RevisionSurfaceForm<Vec<bool>, FiniteReal>>,
) -> bool {
    revision_form
        .and_then(|form| form.flags.first())
        .copied()
        .unwrap_or(false)
}

/// The angle of a native revolution at its angular parameter, and the
/// angle's derivative, or why there is none. An angle that overflows reaches
/// no coordinate.
fn native_revolution_angle(
    construction: &crate::geometry::surface_payloads::RevolutionSurfaceConstruction,
    angular_parameter: FiniteReal,
) -> Result<(f64, f64), EvaluationFailure<Point3>> {
    let unreached = EvaluationFailure::NonFinite(UNREACHED_POINT);
    let (angle, angular_derivative) = match construction.angular_parameter_interval() {
        None => (angular_parameter.get(), 1.0),
        Some(parameter_interval) => {
            let angular_interval = construction.angular_interval();
            let angle = angular_interval
                .map_from(parameter_interval, angular_parameter, false)
                .map_err(|_| unreached)?
                .get();
            let angular_derivative = angular_interval
                .scaled_span()
                .quotient(parameter_interval.scaled_span())
                .map_or_else(|overflow| overflow, FiniteReal::get);
            (angle, angular_derivative)
        }
    };
    Ok((angle, angular_derivative))
}

/// The directrix and angular parameters of a native revolution at `(u, v)`.
/// An angular parameter that is not finite has no value.
fn native_revolution_parameters(
    construction: &crate::geometry::surface_payloads::RevolutionSurfaceConstruction,
    u: f64,
    v: f64,
) -> Result<(f64, FiniteReal), EvaluationFailure<Point3>> {
    let (directrix_parameter, angular_parameter) = if *construction.transposed() {
        (v, u)
    } else {
        (u, v)
    };
    Ok((
        directrix_parameter,
        FiniteReal::new(angular_parameter).ok_or(EvaluationFailure::NoValue)?,
    ))
}

/// The directrix parameter of a native revolution on its carrier curve, or
/// why there is none.
fn native_revolution_carrier(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::RevolutionSurfaceConstruction,
    carrier_interval: Option<[FiniteReal; 2]>,
    directrix_parameter: f64,
) -> Result<ConstructionParameter, EvaluationFailure<Point3>> {
    construction_curve_parameter(
        admission,
        index,
        construction.directrix(),
        directrix_parameter,
        construction
            .parameter_interval()
            .map(crate::topology::IncreasingParameterInterval::finite_endpoints),
        carrier_interval,
        false,
    )
    .map_err(|failure| failure.map(|()| UNREACHED_POINT))
}

/// The point of a native revolution, or why it has none: the directrix point
/// at the carrier parameter revolved by the mapped angle. The point fails on
/// no derivative.
fn model_native_revolution_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::RevolutionSurfaceConstruction,
    carrier_interval: Option<[FiniteReal; 2]>,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let (directrix_parameter, angular_parameter) =
        native_revolution_parameters(construction, u, v)?;
    let carrier = native_revolution_carrier(
        admission,
        index,
        construction,
        carrier_interval,
        directrix_parameter,
    )?;
    let (angle, _) = native_revolution_angle(construction, angular_parameter)?;
    let axis_origin = construction.axis_origin().get();
    let axis = unit_length_axis(construction.axis_direction());
    let point = model_curve_differential_by_id_inner(
        admission,
        index,
        construction.directrix(),
        carrier.parameter.get(),
        ModelCurveRequest::Point,
    )
    .map_err(|failure| failure.map(|point| revolved_point(point, axis_origin, axis, angle)))?.point;
    admit_point(revolved_point(point.get(), axis_origin, axis, angle))
}

/// The jet of a native revolution, or why its point has none. Each partial
/// order reads the carrier parameter's derivative besides the axis
/// revolution's order.
fn model_native_revolution_jet(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::RevolutionSurfaceConstruction,
    carrier_interval: Option<[FiniteReal; 2]>,
    u: f64,
    v: f64,
    request: SurfaceRequest,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    let (directrix_parameter, angular_parameter) =
        native_revolution_parameters(construction, u, v)?;
    let carrier = native_revolution_carrier(
        admission,
        index,
        construction,
        carrier_interval,
        directrix_parameter,
    )?;
    let (angle, angular_derivative) = native_revolution_angle(construction, angular_parameter)?;
    let jet = model_axis_revolution_jet(
        admission,
        index,
        construction.directrix(),
        construction.axis_origin().get(),
        construction.axis_direction(),
        angle,
        carrier.parameter.get(),
        request,
    )?;
    let derivative = carrier.derivative.map(FiniteReal::get);
    let transposed = *construction.transposed();
    let first = derivative.and_then(|derivative| {
        let [du, dv] = FiniteVector3::raw_array(jet.first?);
        Ok(if transposed {
            [
                scale_vector(du, angular_derivative),
                scale_vector(dv, derivative),
            ]
        } else {
            [
                scale_vector(dv, derivative),
                scale_vector(du, angular_derivative),
            ]
        })
    });
    let second = derivative.and_then(|derivative| {
        let [duu, duv, dvv] = FiniteVector3::raw_array(jet.second?);
        Ok(if transposed {
            [
                scale_vector(duu, angular_derivative * angular_derivative),
                scale_vector(duv, derivative * angular_derivative),
                scale_vector(dvv, derivative * derivative),
            ]
        } else {
            [
                scale_vector(dvv, derivative * derivative),
                scale_vector(duv, derivative * angular_derivative),
                scale_vector(duu, angular_derivative * angular_derivative),
            ]
        })
    });
    Ok(SurfaceJet::formed(jet.point, first, second))
}

fn model_curve_point_by_id_inner(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let budget = admission.work_slice();
    let depth_guard =
        ModelEvaluationDepthGuard::enter(budget).map_err(EvaluationFailure::ResourceLimit)?;
    let curve = index
        .curves(curve_id.as_str(), admission)
        .map_err(EvaluationFailure::ResourceLimit)?
        .ok_or(EvaluationFailure::NoValue)?;
    admission.model_step()?;
    if !depth_guard.bind(
        ModelEvaluationIdentity::Curve(std::ptr::from_ref(curve)),
        admission,
    ) {
        return Err(EvaluationFailure::NoValue);
    }
    let Some(procedural) = index
        .procedural_curves_for_curve(curve_id.as_str(), admission)
        .map_err(EvaluationFailure::ResourceLimit)?
        .and_then(|procedurals| procedurals.first().copied())
    else {
        return crate::eval::decode::curve_point(admission, &curve.geometry, parameter);
    };
    match procedural.definition() {
        ProceduralCurveDefinition::Replica { source, transform } => placed_point(
            *transform,
            model_curve_point_by_id_inner(admission, index, source, parameter),
        ),
        ProceduralCurveDefinition::Subset(definition_payload) => {
            let source = definition_payload.source();
            let source_parameter = subset_source_parameter(
                *definition_payload.parameter_range(),
                *definition_payload.sense(),
                parameter,
            )?;
            model_curve_point_by_id_inner(admission, index, source, source_parameter)
        }
        ProceduralCurveDefinition::Helix(_) => {
            helix_differential(procedural.definition(), parameter)
                .map(|differential| differential.point)
        }
        ProceduralCurveDefinition::TolerantIntersection {
            construction: intersection,
            parameterization: Some(parameterization),
            ..
        } => {
            let supports = intersection.supports();
            let tolerance = intersection.tolerance().get();

            let parameter_range = parameterization.parameter_range().endpoints();
            if !parameter.is_finite()
                || parameter < parameter_range[0]
                || parameter > parameter_range[1]
            {
                return Err(EvaluationFailure::NoValue);
            }
            let evaluate_side = |side: usize| {
                // A non-finite offset-pcurve point is evaluated on its support
                // as a finite one is.
                let uv = match crate::eval::decode::pcurve_uv(
                    admission,
                    &parameterization.pcurves[side],
                    parameter,
                ) {
                    Ok(uv) => uv.get(),
                    Err(EvaluationFailure::NonFinite(uv)) => uv,
                    Err(EvaluationFailure::NoValue) => return Err(EvaluationFailure::NoValue),
                    Err(EvaluationFailure::ResourceLimit(limit)) => {
                        return Err(EvaluationFailure::ResourceLimit(limit))
                    }
                };
                model_surface_point_by_id(admission, index, &supports[side], uv.u, uv.v)
            };
            let first = evaluate_side(0);
            if let Err(EvaluationFailure::ResourceLimit(limit)) = first {
                return Err(EvaluationFailure::ResourceLimit(limit));
            }
            let points = [first, evaluate_side(1)];
            // A side with no value leaves no point. A side outside the finite
            // range leaves the evaluation there, carrying the first side's
            // point as far as it was reached. Two finite points farther apart
            // than the tolerance, including a separation that overflows, have
            // no intersection point.
            let reached = |side: Result<FinitePoint3, EvaluationFailure<Point3>>| match side {
                Ok(point) => Ok(Some(point.get())),
                Err(failure) => failure.non_finite(),
            };
            match points {
                [Ok(first), Ok(second)] => {
                    let separation = first.distance(second.get());
                    (separation.is_finite() && separation <= tolerance)
                        .then_some(first)
                        .ok_or(EvaluationFailure::NoValue)
                }
                [first, second] => match (reached(first)?, reached(second)?) {
                    (Some(first), Some(_)) => Err(EvaluationFailure::NonFinite(first)),
                    _ => Err(EvaluationFailure::NoValue),
                },
            }
        }
        _ => {
            if let Some(cache) = curve.geometry.solved_cache() {
                crate::eval::decode::curve_point_solved(admission, cache, parameter)
            } else if matches!(&curve.geometry, CurveGeometry::Procedural { .. }) {
                Err(EvaluationFailure::NoValue)
            } else {
                crate::eval::decode::curve_point(admission, &curve.geometry, parameter)
            }
        }
    }
}

/// Invert a model curve using a caller-owned lookup index.
///
/// Batch callers must reuse one index so carrier inversion remains linear in
/// the document population rather than rebuilding the index for every edge.
pub fn model_curve_parameter_near_point_in_index(
    ctx: &DecodeContext<'_>,
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    point: Point3,
    seed: f64,
) -> Result<Option<FiniteReal>, CodecError> {
    model_curve_parameter_near_point_with_tolerance(
        ctx,
        index,
        curve_id,
        point,
        seed,
        NonNegativeLength::from(index.ir().tolerances.linear),
    )
}

/// Invert a model curve using a caller-owned lookup index and tolerance.
///
/// The tolerance controls the forward validation of every candidate returned
/// by the inversion. Callers that admit an evaluated geometric residual above
/// the document default must pass that same admission bound here.
pub fn model_curve_parameter_near_point_in_index_with_tolerance(
    ctx: &DecodeContext<'_>,
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    point: Point3,
    seed: f64,
    tolerance: f64,
) -> Result<Option<FiniteReal>, CodecError> {
    ctx.charge_work_limit(0, "IR model curve tolerance inverse boundary")?;
    let Some(tolerance) = NonNegativeLength::new(tolerance) else {
        return Ok(None);
    };
    model_curve_parameter_near_point_with_tolerance(ctx, index, curve_id, point, seed, tolerance)
}

/// Invert a model curve with an admitted tolerance.
fn model_curve_parameter_near_point_with_tolerance(
    ctx: &DecodeContext<'_>,
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    point: Point3,
    seed: f64,
    tolerance: NonNegativeLength,
) -> Result<Option<FiniteReal>, CodecError> {
    let _depth = ctx.enter_nested("IR model curve inversion depth")?;
    let Some(curve) = index.curves(curve_id.as_str(), ctx)? else {
        return Ok(None);
    };
    if let Some(procedural) = index
        .procedural_curves_for_curve(curve_id.as_str(), ctx)?
        .and_then(|procedurals| procedurals.first().copied())
    {
        match procedural.definition() {
            ProceduralCurveDefinition::Replica { source, transform } => {
                let Some((basis_point, tolerance_scale)) = inverse_affine_point(*transform, point)
                else {
                    return Ok(None);
                };
                // The scale is a finite norm, so the admission refuses only an
                // overflowed product.
                let Some(basis_tolerance) =
                    NonNegativeLength::new(tolerance.get() * tolerance_scale)
                else {
                    return Ok(None);
                };
                return model_curve_parameter_near_point_with_tolerance(
                    ctx,
                    index,
                    source,
                    basis_point,
                    seed,
                    basis_tolerance,
                );
            }
            ProceduralCurveDefinition::Subset(definition_payload) => {
                let source = definition_payload.source();
                let [start, end] = definition_payload.parameter_range().endpoints();
                let sense = definition_payload.sense();
                {
                    let Some(interval) = IncreasingParameterInterval::new([start, end]) else {
                        return Ok(None);
                    };
                    let span = interval.scaled_span().finite();
                    if !seed.is_finite() || seed < 0.0 || span.is_ok_and(|span| seed > span.get()) {
                        return Ok(None);
                    }
                    let source_seed = if *sense { start + seed } else { end - seed };
                    let Some(source_parameter) = model_curve_parameter_near_point_with_tolerance(
                        ctx,
                        index,
                        source,
                        point,
                        source_seed,
                        tolerance,
                    )?
                    else {
                        return Ok(None);
                    };
                    let source_parameter = source_parameter.get();
                    let Some(parameter) = FiniteReal::new(if *sense {
                        source_parameter - start
                    } else {
                        end - source_parameter
                    }) else {
                        return Ok(None);
                    };
                    let evaluated = finite_or_refusal(model_curve_point_by_id(
                        crate::eval::admission::EvaluationAdmission::Decode(ctx),
                        index,
                        curve_id,
                        parameter.get(),
                    ))?;
                    return Ok((parameter.get() >= 0.0
                        && span.map_or(true, |span| parameter <= span)
                        && evaluated.is_some_and(|evaluated| {
                            evaluated.distance(point) <= tolerance.get()
                        }))
                    .then_some(parameter));
                }
            }
            ProceduralCurveDefinition::Helix(_) => {
                return helix_parameter_near_point(
                    ctx,
                    point,
                    seed,
                    tolerance,
                    procedural.definition(),
                );
            }
            _ => {}
        }
    }
    if let Some(cache) = curve.geometry.solved_cache() {
        let Some(seed) = FiniteReal::new(seed) else {
            return Ok(None);
        };
        return direct_curve_parameter_near_point(ctx, cache, point, seed, tolerance);
    }
    if !matches!(&curve.geometry, CurveGeometry::Procedural { .. }) {
        let Some(seed) = FiniteReal::new(seed) else {
            return Ok(None);
        };
        let Some(geometry) = curve.geometry.solved() else {
            return Ok(None);
        };
        return direct_curve_parameter_near_point(ctx, geometry, point, seed, tolerance);
    }
    let Some(construction) = curve.geometry.procedural_construction() else {
        return Ok(None);
    };
    let Some(procedural) = index.procedural_curves(construction.as_str(), ctx)? else {
        return Ok(None);
    };
    let crate::geometry::ProceduralCurveDefinition::TolerantIntersection {
        construction: intersection,
        parameterization: Some(parameterization),
        ..
    } = procedural.definition()
    else {
        return Ok(None);
    };
    let supports = intersection.supports();
    // The intersection tolerance bounds model-space distances, so it is a
    // length.
    let admitted_tolerance = NonNegativeLength::from_assigned_real(intersection.tolerance());
    let tolerance = admitted_tolerance.get();

    let range = parameterization.parameter_range().endpoints();
    let finite_range = parameterization.parameter_range().finite_endpoints();
    if !seed.is_finite() || seed < range[0] || seed > range[1] {
        return Ok(None);
    }
    let mut best_candidate: Option<FiniteReal> = None;
    for (support_id, pcurve) in supports.iter().zip(&parameterization.pcurves) {
        let Some(surface) = index.surfaces(support_id.as_str(), ctx)? else {
            continue;
        };
        let PcurveGeometry::Line(line_pcurve) = pcurve else {
            continue;
        };
        let origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
        let parameter = match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => {
                let Some(base) = finite_or_refusal(model_surface_point_by_id(
                    crate::eval::admission::EvaluationAdmission::Decode(ctx),
                    index,
                    support_id,
                    origin.u,
                    origin.v,
                ))?
                else {
                    continue;
                };
                let Some(next) = finite_or_refusal(model_surface_point_by_id(
                    crate::eval::admission::EvaluationAdmission::Decode(ctx),
                    index,
                    support_id,
                    origin.u + direction.u,
                    origin.v + direction.v,
                ))?
                else {
                    continue;
                };
                let tangent = Vector3::new(next.x - base.x, next.y - base.y, next.z - base.z);
                let offset = Vector3::new(point.x - base.x, point.y - base.y, point.z - base.z);
                let denominator = tangent.dot(tangent);
                (denominator.is_finite() && denominator > 0.0)
                    .then(|| offset.dot(tangent) / denominator)
            }
            SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(_)
                | SolvedSurfaceGeometry::Cone(_)
                | SolvedSurfaceGeometry::Sphere(_)
                | SolvedSurfaceGeometry::Torus(_),
            ) => analytic_surface_parameters(&surface.geometry, point)
                .map(Point2::from)
                .and_then(|mut uv| {
                    if direction.v == 0.0 && direction.u != 0.0 {
                        let expected = origin.u + direction.u * seed;
                        uv.u += ((expected - uv.u) / std::f64::consts::TAU).round()
                            * std::f64::consts::TAU;
                        Some((uv.u - origin.u) / direction.u)
                    } else if direction.u == 0.0
                        && direction.v != 0.0
                        && matches!(
                            surface.geometry.solved(),
                            Some(SolvedSurfaceGeometry::Torus(_))
                        )
                    {
                        let expected = origin.v + direction.v * seed;
                        uv.v += ((expected - uv.v) / std::f64::consts::TAU).round()
                            * std::f64::consts::TAU;
                        Some((uv.v - origin.v) / direction.v)
                    } else if direction.u == 0.0 && direction.v != 0.0 {
                        Some((uv.v - origin.v) / direction.v)
                    } else {
                        None
                    }
                }),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) => {
                let (fixed_axis, fixed_parameter, varying_origin, varying_scale) =
                    if direction.u == 0.0 && direction.v != 0.0 {
                        (SurfaceParameterAxis::U, origin.u, origin.v, direction.v)
                    } else if direction.v == 0.0 && direction.u != 0.0 {
                        (SurfaceParameterAxis::V, origin.v, origin.u, direction.u)
                    } else {
                        continue;
                    };
                let Some(isocurve) =
                    nurbs_surface_isocurve(ctx, surface, fixed_axis, fixed_parameter)?
                else {
                    continue;
                };
                let Some(isocurve_seed) = FiniteReal::new(varying_origin + varying_scale * seed)
                else {
                    continue;
                };
                nurbs_curve_parameter_near_point_with_nonnegative_tolerance(
                    ctx,
                    &isocurve,
                    point,
                    admitted_tolerance,
                    isocurve_seed,
                )?
                .map(|parameter| (parameter.get() - varying_origin) / varying_scale)
            }
            _ => continue,
        };
        // A parameter that is not finite lies outside the finite range, so
        // it has no candidate.
        let Some(mut parameter) = parameter.and_then(FiniteReal::new) else {
            continue;
        };
        let endpoint_tolerance = EPS_EVAL_MODEL_CURVE_PARAMETER_NEAR_POINT_WITH_TOLERANCE_E12
            * (1.0 + range[0].abs().max(range[1].abs()));
        let value = parameter.get();
        if value < range[0] && range[0] - value <= endpoint_tolerance {
            parameter = finite_range[0];
        } else if value > range[1] && value - range[1] <= endpoint_tolerance {
            parameter = finite_range[1];
        } else if value < range[0] || value > range[1] {
            continue;
        }
        let Some(evaluated) = finite_or_refusal(model_curve_point_by_id(
            crate::eval::admission::EvaluationAdmission::Decode(ctx),
            index,
            curve_id,
            parameter.get(),
        ))?
        else {
            continue;
        };
        let distance = evaluated.distance(point);
        if distance.is_finite()
            && distance <= tolerance
            && best_candidate.is_none_or(|best| {
                (parameter.get() - seed)
                    .abs()
                    .total_cmp(&(best.get() - seed).abs())
                    == Ordering::Less
            })
        {
            best_candidate = Some(parameter);
        }
    }
    Ok(best_candidate)
}

/// Find a helix parameter near a caller-selected seed by bounded Newton
/// refinement of the squared model-space distance.
fn helix_parameter_near_point(
    ctx: &DecodeContext<'_>,
    target: Point3,
    seed: f64,
    tolerance: NonNegativeLength,
    definition: &ProceduralCurveDefinition,
) -> Result<Option<FiniteReal>, CodecError> {
    let _depth = ctx.enter_nested("IR helix inverse evaluation")?;
    let ProceduralCurveDefinition::Helix(helix_payload) = definition else {
        return Ok(None);
    };
    let [start, end] = helix_payload.angle_range().finite_components();
    let tolerance = tolerance.get();

    // A reversed angle range holds no seed.
    let Some(angles) = ParameterInterval::ordered(start, end) else {
        return Ok(None);
    };
    let Some(seed) = FiniteReal::new(seed) else {
        return Ok(None);
    };
    if seed < start || seed > end || !target.is_finite() {
        return Ok(None);
    }

    let mut parameter = seed;
    for _ in 0..MODEL_CURVE_PARAMETER_SEARCH_MAX_NEWTON_ITERATIONS {
        let Some(differential) =
            finite_or_refusal(helix_differential(definition, parameter.get()))?
        else {
            return Ok(None);
        };
        let residual = Vector3::new(
            differential.point.x - target.x,
            differential.point.y - target.y,
            differential.point.z - target.z,
        );
        let distance = residual.norm();
        if distance.is_finite() && distance <= tolerance {
            return Ok(Some(parameter));
        }
        let Some(tangent) = finite_or_refusal(differential.tangent)? else {
            return Ok(None);
        };
        let tangent = tangent.get();
        let denominator = tangent.dot(tangent);
        if !denominator.is_finite() || denominator <= 0.0 {
            break;
        }
        let Some(next) = FiniteReal::new(parameter.get() - residual.dot(tangent) / denominator)
        else {
            break;
        };
        let next = angles.project(ExtendedReal::from_finite(next));
        if next == parameter {
            break;
        }
        parameter = next;
    }

    let Some(differential) = finite_or_refusal(helix_differential(definition, parameter.get()))?
    else {
        return Ok(None);
    };
    let distance = Vector3::new(
        differential.point.x - target.x,
        differential.point.y - target.y,
        differential.point.z - target.z,
    )
    .norm();
    Ok((distance.is_finite() && distance <= tolerance).then_some(parameter))
}

/// Invert a direct curve carrier near a caller-selected parameter seed.
pub(crate) fn curve_parameter_near_point(
    ctx: &DecodeContext<'_>,
    geometry: &CurveGeometry,
    point: Point3,
    seed: f64,
    tolerance: f64,
) -> Result<Option<FiniteReal>, CodecError> {
    ctx.charge_work_limit(0, "IR direct curve inverse boundary")?;
    let Some(geometry) = geometry.solved() else {
        return Ok(None);
    };
    let Some(seed) = FiniteReal::new(seed) else {
        return Ok(None);
    };
    let Some(tolerance) = NonNegativeLength::new(tolerance) else {
        return Ok(None);
    };
    direct_curve_parameter_near_point(ctx, geometry, point, seed, tolerance)
}

fn direct_curve_parameter_near_point(
    ctx: &DecodeContext<'_>,
    geometry: &SolvedCurveGeometry,
    point: Point3,
    seed: FiniteReal,
    admitted_tolerance: NonNegativeLength,
) -> Result<Option<FiniteReal>, CodecError> {
    let _depth = ctx.enter_nested("IR direct curve inversion depth")?;
    let result = (|| -> Option<Result<FiniteReal, CodecError>> {
        let tolerance = admitted_tolerance.get();
        let components = |origin: Point3, axis: Vector3, reference: Vector3| -> (f64, f64, f64) {
            let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
            let transverse = axis.cross(reference);
            (delta.dot(reference), delta.dot(transverse), delta.dot(axis))
        };
        let parameter = match geometry {
            SolvedCurveGeometry::Line(line_curve) => {
                let origin = line_curve.origin().get();
                let direction = *line_curve.direction().as_raw();
                let delta =
                    Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
                FiniteReal::new(delta.dot(direction) / direction.dot(direction))?
            }
            SolvedCurveGeometry::Circle(circle_curve) => {
                let center = circle_curve.center().get();
                let axis = circle_curve.frame().axis().as_raw();
                let ref_direction = circle_curve.frame().reference().as_raw();
                let radius = circle_curve.radius().get();
                let (x, y, _) = components(center, *axis, *ref_direction);
                let canonical = (y / radius).atan2(x / radius);
                FiniteReal::new(
                    canonical
                        + ((seed.get() - canonical) / std::f64::consts::TAU).round()
                            * std::f64::consts::TAU,
                )?
            }
            SolvedCurveGeometry::Ellipse(ellipse_curve) => {
                let center = ellipse_curve.center().get();
                let axis = ellipse_curve.frame().axis().as_raw();
                let major_direction = ellipse_curve.frame().reference().as_raw();
                let major_radius = ellipse_curve.major_radius().get();
                let minor_radius = ellipse_curve.minor_radius().get();
                let (x, y, _) = components(center, *axis, *major_direction);
                let canonical = (y / minor_radius).atan2(x / major_radius);
                FiniteReal::new(
                    canonical
                        + ((seed.get() - canonical) / std::f64::consts::TAU).round()
                            * std::f64::consts::TAU,
                )?
            }
            SolvedCurveGeometry::Parabola(parabola_curve) => {
                let vertex = parabola_curve.vertex().get();
                let axis = parabola_curve.frame().axis().as_raw();
                let major_direction = parabola_curve.frame().reference().as_raw();
                let focal_distance = parabola_curve.focal_distance().magnitude();
                let (_, transverse, _) = components(vertex, *axis, *major_direction);
                crate::math::multiply_divide(
                    FiniteReal::new(transverse)?,
                    FiniteReal::HALF,
                    focal_distance,
                )?
            }
            SolvedCurveGeometry::Hyperbola(hyperbola_curve) => {
                let center = hyperbola_curve.center().get();
                let axis = hyperbola_curve.frame().axis().as_raw();
                let major_direction = hyperbola_curve.frame().reference().as_raw();
                let minor_radius = hyperbola_curve.minor_radius().get();
                let (_, transverse, _) = components(center, *axis, *major_direction);
                FiniteReal::new((transverse / minor_radius).asinh())?
            }
            SolvedCurveGeometry::Nurbs(curve) => {
                match nurbs_curve_parameter_near_point_with_nonnegative_tolerance(
                    ctx,
                    curve,
                    point,
                    admitted_tolerance,
                    seed,
                ) {
                    Ok(Some(parameter)) => parameter,
                    Ok(None) => return None,
                    Err(limit) => return Some(Err(limit)),
                }
            }
            SolvedCurveGeometry::Polyline(polyline) => {
                match polyline::parameter_near_point(ctx, polyline, point, tolerance, seed) {
                    Ok(Some(parameter)) => parameter,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                }
            }
            SolvedCurveGeometry::Transformed(placed) => {
                let (basis_point, tolerance_scale) =
                    inverse_affine_point(*placed.transform(), point)?;
                // The scale is a finite norm, so the admission refuses only an
                // overflowed product.
                let basis_tolerance = NonNegativeLength::new(tolerance * tolerance_scale)?;
                match direct_curve_parameter_near_point(
                    ctx,
                    placed.basis(),
                    basis_point,
                    seed,
                    basis_tolerance,
                ) {
                    Ok(Some(parameter)) => parameter,
                    Ok(None) => return None,
                    Err(limit) => return Some(Err(limit)),
                }
            }
            SolvedCurveGeometry::Degenerate(degenerate_curve) => {
                let stored = degenerate_curve.point().get();
                let error = (stored.x - point.x)
                    .hypot(stored.y - point.y)
                    .hypot(stored.z - point.z);
                (error.is_finite() && error <= tolerance).then_some(seed)?
            }
            SolvedCurveGeometry::Composite { .. } | SolvedCurveGeometry::Unknown { .. } => {
                return None
            }
        };
        let evaluated = match crate::eval::decode::outer_refusal(
            crate::eval::decode::curve_point_solved(ctx, geometry, parameter.get()),
        )
        .and_then(finite_or_refusal)
        {
            Ok(Some(point)) => point,
            Ok(None) => return None,
            Err(limit) => return Some(Err(limit.into())),
        };
        let error = evaluated.distance(point);
        (error.is_finite() && error <= tolerance).then_some(Ok(parameter))
    })();
    result.transpose()
}

fn inverse_affine_point(transform: Transform, point: Point3) -> Option<(Point3, f64)> {
    let inverse = transform.try_inverse_affine().ok()?;
    let coordinates = inverse.apply_point(point)?;
    let tolerance_scale = inverse
        .affine_rows()
        .iter()
        .flat_map(|row| &row[..3])
        .fold(0.0_f64, |length, &value| length.hypot(value));
    tolerance_scale
        .is_finite()
        .then_some((coordinates.get(), tolerance_scale))
}

fn curve_point_evaluation(
    scratch: &decode::Scratch<'_, '_>,
    geometry: &SolvedCurveGeometry,
    t: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    match geometry {
        SolvedCurveGeometry::Nurbs(nurbs) => scratch
            .admission
            .independent_cost(nurbs_curve_evaluation_cost(nurbs))?,
        SolvedCurveGeometry::Polyline(polyline) => scratch
            .admission
            .independent_cost(Some(polyline.point_count()))?,
        SolvedCurveGeometry::Transformed(_) => scratch.admission.independent_cost(Some(1))?,
        _ => {}
    }
    let _depth = scratch.enter().ok_or(EvaluationFailure::NoValue)?;
    let parameter = || FiniteReal::new(t).ok_or(EvaluationFailure::NoValue);
    match geometry {
        SolvedCurveGeometry::Line(line_curve) => {
            let t = parameter()?.get();
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            admit_point(offset(origin, &[(t, direction)]))
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let t = parameter()?.get();
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius().get();
            admit_point(offset(
                center,
                &[
                    (radius * t.cos(), *ref_direction),
                    (radius * t.sin(), axis.cross(*ref_direction)),
                ],
            ))
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let t = parameter()?.get();
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            admit_point(offset(
                center,
                &[
                    (major_radius * t.cos(), *major_direction),
                    (minor_radius * t.sin(), axis.cross(*major_direction)),
                ],
            ))
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let t = parameter()?.get();
            let vertex = parabola_curve.vertex().get();
            let axis = parabola_curve.frame().axis().as_raw();
            let major_direction = parabola_curve.frame().reference().as_raw();
            let focal_distance = parabola_curve.focal_distance().get();
            // The products of finite factors are absent only where they
            // overflow.
            let (Some(axial), Some(transverse)) = (
                product_quotient([focal_distance, t, t], []),
                product_quotient([2.0, focal_distance, t], []),
            ) else {
                return Err(EvaluationFailure::NonFinite(UNREACHED_POINT));
            };
            admit_point(offset(
                vertex,
                &[
                    (axial.get(), *major_direction),
                    (transverse.get(), axis.cross(*major_direction)),
                ],
            ))
        }
        SolvedCurveGeometry::Hyperbola(hyperbola_curve) => {
            let parameter = parameter()?;
            let center = hyperbola_curve.center().get();
            let axis = hyperbola_curve.frame().axis().as_raw();
            let major_direction = hyperbola_curve.frame().reference().as_raw();
            let major_radius = hyperbola_curve.major_radius().magnitude();
            let minor_radius = Length::from(hyperbola_curve.minor_radius()).magnitude();
            // The point reads the major cosine and the minor sine products
            // only; one outside the finite range carries its plain value.
            let [_, major_cosh] = sinh_cosh_lanes(scaled_sinh_cosh(major_radius, parameter));
            let [minor_sinh, _] = sinh_cosh_lanes(scaled_sinh_cosh(minor_radius, parameter));
            match (major_cosh, minor_sinh) {
                (Ok(major_cosh), Ok(minor_sinh)) => admit_point(offset(
                    center,
                    &[
                        (major_cosh.get(), *major_direction),
                        (minor_sinh.get(), axis.cross(*major_direction)),
                    ],
                )),
                (major_cosh, minor_sinh) => {
                    let plain = |lane: Result<FiniteReal, f64>| {
                        lane.map_or_else(|plain| plain, FiniteReal::get)
                    };
                    Err(EvaluationFailure::NonFinite(offset(
                        center,
                        &[
                            (plain(major_cosh), *major_direction),
                            (plain(minor_sinh), axis.cross(*major_direction)),
                        ],
                    )))
                }
            }
        }
        SolvedCurveGeometry::Degenerate(degenerate_curve) => Ok(degenerate_curve.point()),
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let parameter =
                map_nurbs_curve_parameter(nurbs, parameter()?).ok_or(EvaluationFailure::NoValue)?;
            let poles = nurbs.pole_rows();
            nurbs_curve_point_evaluation(
                scratch,
                nurbs.degree(),
                nurbs.knots(),
                poles.count(),
                |index| poles.point_at(index),
                |index| poles.weight_at(index),
                parameter,
            )
        }
        SolvedCurveGeometry::Polyline(polyline) => polyline_point(
            scratch.admission,
            polyline.point_count(),
            |index| polyline.point_at(index),
            |index| polyline.parameter_at(index),
            t,
        ),
        SolvedCurveGeometry::Transformed(placed) => {
            scratch
                .work(1, "placed geometry evaluation step")
                .ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
            placed_point(
                *placed.transform(),
                curve_point_evaluation(scratch, placed.basis(), t),
            )
        }
        SolvedCurveGeometry::Composite { .. } | SolvedCurveGeometry::Unknown { .. } => {
            Err(EvaluationFailure::NoValue)
        }
    }
}

fn surface_point_evaluation(
    scratch: &decode::Scratch<'_, '_>,
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    match geometry {
        SolvedSurfaceGeometry::Nurbs(nurbs) => scratch
            .admission
            .independent_cost(nurbs_surface_evaluation_cost(nurbs))?,
        SolvedSurfaceGeometry::Transformed(_) => scratch.admission.independent_cost(Some(1))?,
        _ => {}
    }
    let _depth = scratch.enter().ok_or(EvaluationFailure::NoValue)?;
    match geometry {
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            nurbs_surface_local(scratch, nurbs, u, v).map(|local| {
                let [point_x, point_y, point_z] = local.point;
                FinitePoint3::from_coordinates(point_x, point_y, point_z)
            })
        }
        SolvedSurfaceGeometry::Transformed(placed) => {
            scratch
                .work(1, "placed geometry evaluation step")
                .ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
            placed_point(
                *placed.transform(),
                surface_point_evaluation(scratch, placed.basis(), u, v),
            )
        }
        _ => analytic_surface_second_partials(geometry, u, v)
            .map_or(Err(EvaluationFailure::NoValue), |partials| {
                admit_point(partials.point)
            }),
    }
}

/// A placed carrier's point from its basis point. A basis point outside the
/// finite range carries the point the placement reaches from it, finite or
/// not.
fn placed_point(
    transform: Transform,
    basis: Result<FinitePoint3, EvaluationFailure<Point3>>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    match basis {
        Ok(point) => transform
            .apply_point_reaching(point.get())
            .map_err(EvaluationFailure::NonFinite),
        Err(EvaluationFailure::NonFinite(point)) => Err(EvaluationFailure::NonFinite(
            transform
                .apply_point_reaching(point)
                .map_or_else(|point| point, FinitePoint3::get),
        )),
        Err(EvaluationFailure::NoValue) => Err(EvaluationFailure::NoValue),
        Err(EvaluationFailure::ResourceLimit(limit)) => {
            Err(EvaluationFailure::ResourceLimit(limit))
        }
    }
}

const ROLLING_BALL_JET_RADIUS_TOLERANCE: f64 = 1e-8;

/// Evaluate a quintic rolling-ball jet at spine parameter `t` and arc
/// fraction `s`.
///
/// The jet stores value, first-derivative, and second-derivative rows for the
/// two limiting points, centre, and opening angle. Each scalar channel is
/// interpolated with the unique quintic Hermite polynomial on its knot span.
/// The interpolated limiting points then define the circular section whose
/// fixed radius is taken from the first stored station.
///
/// A definition that is not a quintic jet with triple interior knots, a
/// parameter that is not finite or lies outside the jet, stations whose
/// radii disagree, and a section whose first limiting direction vanishes or
/// whose second is parallel to it have no point. A knot span, interpolated
/// channel or section direction that overflows reaches no coordinate; a
/// point outside the finite range carries the point reached.
pub fn rolling_ball_jet_point(
    definition: &ProceduralSurfaceDefinition,
    t: f64,
    s: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    rolling_ball_jet_point_admitted(admission::EvaluationAdmission::Standard, definition, t, s)
}

fn rolling_ball_jet_point_admitted(
    admission: admission::EvaluationAdmission<'_, '_>,
    definition: &ProceduralSurfaceDefinition,
    t: f64,
    s: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    admission.work(0, "rolling-ball jet boundary")?;
    let no_value = EvaluationFailure::NoValue;
    let unreached = EvaluationFailure::NonFinite(UNREACHED_POINT);
    let ProceduralSurfaceDefinition::RollingBallJet(jet) = definition else {
        return Err(no_value);
    };
    let degree = jet.degree();
    let stations = jet.stations();
    if degree != 5 {
        return Err(no_value);
    }
    let mut interior = stations[1..stations.len() - 1].iter();
    while interior.len() != 0 {
        admission.independent_cost(Some(1))?;
        admission.work(1, "rolling-ball jet multiplicity scan")?;
        let Some(station) = interior.next() else {
            break;
        };
        if station.multiplicity != 3 {
            return Err(no_value);
        }
    }
    if !t.is_finite() || !s.is_finite() || !(0.0..=1.0).contains(&s) {
        return Err(no_value);
    }
    let radius = stations[0]
        .site
        .first_limit
        .distance(stations[0].site.center.get());
    let mut radius_stations = stations.iter();
    while radius_stations.len() != 0 {
        admission.independent_cost(Some(1))?;
        admission.work(1, "rolling-ball jet radius scan")?;
        let Some(station) = radius_stations.next() else {
            break;
        };
        let site = &station.site;
        let first_radius = site.first_limit.distance(site.center.get());
        let second_radius = site.second_limit.distance(site.center.get());
        if second_radius <= 0.0
            || (first_radius - radius).abs()
                > ROLLING_BALL_JET_RADIUS_TOLERANCE * first_radius.abs().max(radius.abs()).max(1.0)
        {
            return Err(no_value);
        }
    }
    let mut span = None;
    let mut pairs = stations.windows(2).enumerate();
    while pairs.len() != 0 {
        admission.independent_cost(Some(1))?;
        admission.work(1, "rolling-ball jet span scan")?;
        let Some((index, pair)) = pairs.next() else {
            break;
        };
        if t >= pair[0].knot.get() && t <= pair[1].knot.get() {
            span = Some(index);
            break;
        }
    }
    let span = span.ok_or(no_value)?;
    let interval =
        IncreasingParameterInterval::between(stations[span].knot, stations[span + 1].knot)
            .ok_or(no_value)?;
    let (span_factor, doubled) = interval.span_factors();
    let span_factors = [span_factor, if doubled { 2.0 } else { 1.0 }];
    let fraction = match FiniteReal::new(t)
        .ok_or(no_value)?
        .segment_position(stations[span].knot, stations[span + 1].knot)
    {
        SegmentPosition::Within(fraction) => fraction.get(),
        SegmentPosition::Outside | SegmentPosition::Degenerate => return Err(no_value),
    };
    let first = &stations[span].site;
    let second = &stations[span + 1].site;
    let first_limit = rolling_ball_jet_interpolate_point(
        [first.first_limit.get(), second.first_limit.get()],
        [
            first.first_derivative.first_limit.get(),
            second.first_derivative.first_limit.get(),
        ],
        [
            first.second_derivative.first_limit.get(),
            second.second_derivative.first_limit.get(),
        ],
        fraction,
        span_factors,
    );
    let second_limit = rolling_ball_jet_interpolate_point(
        [first.second_limit.get(), second.second_limit.get()],
        [
            first.first_derivative.second_limit.get(),
            second.first_derivative.second_limit.get(),
        ],
        [
            first.second_derivative.second_limit.get(),
            second.second_derivative.second_limit.get(),
        ],
        fraction,
        span_factors,
    );
    let center = rolling_ball_jet_interpolate_point(
        [first.center.get(), second.center.get()],
        [
            first.first_derivative.center.get(),
            second.first_derivative.center.get(),
        ],
        [
            first.second_derivative.center.get(),
            second.second_derivative.center.get(),
        ],
        fraction,
        span_factors,
    );
    let angle = rolling_ball_jet_interpolate_scalar(
        [first.angle.get(), second.angle.get()],
        [
            first.first_derivative.angle.get(),
            second.first_derivative.angle.get(),
        ],
        [
            first.second_derivative.angle.get(),
            second.second_derivative.angle.get(),
        ],
        fraction,
        span_factors,
    );
    // An interpolated channel reads NaN only where its value overflowed.
    if !angle.is_finite() {
        return Err(unreached);
    }
    let first_radius = first_limit.vector_from(center);
    let first_direction = first_radius.scale(1.0 / radius);
    let first_direction_squared = first_direction.dot(first_direction);
    if !first_direction_squared.is_finite() {
        return Err(unreached);
    }
    if first_direction_squared <= f64::EPSILON {
        return Err(no_value);
    }
    let second_radius = second_limit.vector_from(center);
    let second_direction = FiniteVector3::new(
        second_radius
            - first_direction.scale(second_radius.dot(first_direction) / first_direction_squared),
    )
    .ok_or(unreached)?
    .unit_nonzero()
    .ok_or(no_value)?;
    let radial =
        first_direction.scale((s * angle).cos()) + second_direction.scale((s * angle).sin());
    admit_point(center.translated(radial, radius))
}

fn rolling_ball_jet_interpolate_point(
    values: [Point3; 2],
    first_derivatives: [Vector3; 2],
    second_derivatives: [Vector3; 2],
    fraction: f64,
    span_factors: [f64; 2],
) -> Point3 {
    Point3::new(
        rolling_ball_jet_interpolate_scalar(
            [values[0].x, values[1].x],
            [first_derivatives[0].x, first_derivatives[1].x],
            [second_derivatives[0].x, second_derivatives[1].x],
            fraction,
            span_factors,
        ),
        rolling_ball_jet_interpolate_scalar(
            [values[0].y, values[1].y],
            [first_derivatives[0].y, first_derivatives[1].y],
            [second_derivatives[0].y, second_derivatives[1].y],
            fraction,
            span_factors,
        ),
        rolling_ball_jet_interpolate_scalar(
            [values[0].z, values[1].z],
            [first_derivatives[0].z, first_derivatives[1].z],
            [second_derivatives[0].z, second_derivatives[1].z],
            fraction,
            span_factors,
        ),
    )
}

fn rolling_ball_jet_interpolate_scalar(
    values: [f64; 2],
    first_derivatives: [f64; 2],
    second_derivatives: [f64; 2],
    fraction: f64,
    [span_factor, multiplier]: [f64; 2],
) -> f64 {
    let s2 = fraction * fraction;
    let s3 = s2 * fraction;
    let s4 = s3 * fraction;
    let s5 = s4 * fraction;
    let h00 = 1.0 - 10.0 * s3 + 15.0 * s4 - 6.0 * s5;
    let h10 = fraction - 6.0 * s3 + 8.0 * s4 - 3.0 * s5;
    let h20 = 0.5 * s2 - 1.5 * s3 + 1.5 * s4 - 0.5 * s5;
    let h01 = 10.0 * s3 - 15.0 * s4 + 6.0 * s5;
    let h11 = -4.0 * s3 + 7.0 * s4 - 3.0 * s5;
    let h21 = 0.5 * s3 - s4 + 0.5 * s5;
    match crate::math::sum::product_sum(
        [
            Some([values[0], h00, 1.0, 1.0]),
            Some([first_derivatives[0], span_factor, h10, multiplier]),
            Some([
                second_derivatives[0],
                span_factor,
                span_factor,
                h20 * multiplier * multiplier,
            ]),
            Some([values[1], h01, 1.0, 1.0]),
            Some([first_derivatives[1], span_factor, h11, multiplier]),
            Some([
                second_derivatives[1],
                span_factor,
                span_factor,
                h21 * multiplier * multiplier,
            ]),
        ]
        .into_iter(),
    ) {
        crate::math::sum::ProductSum::Zero => 0.0,
        crate::math::sum::ProductSum::Value(value) => {
            value.finite().map_or(f64::NAN, FiniteReal::get)
        }
        crate::math::sum::ProductSum::Undefined => f64::NAN,
    }
}

/// Evaluate a directly stored surface and its exact first partial
/// derivatives, or report why they have no finite value.
///
/// The point fails as [`decode::surface_point_solved`] states. At a finite point,
/// first partials outside the finite range leave the evaluation there,
/// carrying the point.
pub fn surface_partials_solved<'ctx, 'arena: 'ctx>(
    admission: impl Into<admission::EvaluationAdmission<'ctx, 'arena>>,
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<SurfacePartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    let scratch = decode::Scratch::new(admission);
    let result = (|| surface_first_order_solved(&scratch, geometry, u, v)?.partials())();
    scratch.settle(result)
}

/// The raw point and exact first and second partials of an analytic surface
/// at `(u, v)`, or none for a carrier that is not analytic. The lanes are
/// formed from finite frames and can leave the finite range.
fn analytic_surface_second_partials(
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
) -> Option<SurfaceSecondPartials> {
    let zero = Vector3::new(0.0, 0.0, 0.0);
    match geometry {
        SolvedSurfaceGeometry::Plane(plane_surface) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            let v_axis = normal.cross(*u_axis);
            Some(SurfaceSecondPartials {
                point: offset(origin, &[(u, *u_axis), (v, v_axis)]),
                du: *u_axis,
                dv: v_axis,
                duu: zero,
                duv: zero,
                dvv: zero,
            })
        }
        SolvedSurfaceGeometry::Cylinder(cylinder_surface) => {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let ref_direction = cylinder_surface.frame().reference().as_raw();
            let radius = cylinder_surface.radius().get();
            let transverse = axis.cross(*ref_direction);
            let cosine = u.cos();
            let sine = u.sin();
            Some(SurfaceSecondPartials {
                point: offset(
                    origin,
                    &[
                        (radius * cosine, *ref_direction),
                        (radius * sine, transverse),
                        (v, *axis),
                    ],
                ),
                du: vector_sum(&[
                    (-radius * sine, *ref_direction),
                    (radius * cosine, transverse),
                ]),
                dv: *axis,
                duu: vector_sum(&[
                    (-radius * cosine, *ref_direction),
                    (-radius * sine, transverse),
                ]),
                duv: zero,
                dvv: zero,
            })
        }
        SolvedSurfaceGeometry::Cone(cone_surface) => {
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let ref_direction = cone_surface.frame().reference().as_raw();
            let radius = cone_surface.radius().get();
            let ratio = cone_surface.ratio().get();
            let half_angle = cone_surface.half_angle().get();
            let transverse = axis.cross(*ref_direction);
            let cosine = u.cos();
            let sine = u.sin();
            let radial_slope = half_angle.tan();
            let local_radius = radius + v * radial_slope;
            Some(SurfaceSecondPartials {
                point: offset(
                    origin,
                    &[
                        (local_radius * cosine, *ref_direction),
                        (local_radius * ratio * sine, transverse),
                        (v, *axis),
                    ],
                ),
                du: vector_sum(&[
                    (-local_radius * sine, *ref_direction),
                    (local_radius * ratio * cosine, transverse),
                ]),
                dv: vector_sum(&[
                    (radial_slope * cosine, *ref_direction),
                    (radial_slope * ratio * sine, transverse),
                    (1.0, *axis),
                ]),
                duu: vector_sum(&[
                    (-local_radius * cosine, *ref_direction),
                    (-local_radius * ratio * sine, transverse),
                ]),
                duv: vector_sum(&[
                    (-radial_slope * sine, *ref_direction),
                    (radial_slope * ratio * cosine, transverse),
                ]),
                dvv: zero,
            })
        }
        SolvedSurfaceGeometry::Sphere(sphere_surface) => {
            let center = sphere_surface.center().get();
            let axis = sphere_surface.frame().axis().as_raw();
            let ref_direction = sphere_surface.frame().reference().as_raw();
            let radius = sphere_surface.radius().get();
            let transverse = axis.cross(*ref_direction);
            let u_cosine = u.cos();
            let u_sine = u.sin();
            let v_cosine = v.cos();
            let v_sine = v.sin();
            Some(SurfaceSecondPartials {
                point: offset(
                    center,
                    &[
                        (radius * v_cosine * u_cosine, *ref_direction),
                        (radius * v_cosine * u_sine, transverse),
                        (radius * v_sine, *axis),
                    ],
                ),
                du: vector_sum(&[
                    (-radius * v_cosine * u_sine, *ref_direction),
                    (radius * v_cosine * u_cosine, transverse),
                ]),
                dv: vector_sum(&[
                    (-radius * v_sine * u_cosine, *ref_direction),
                    (-radius * v_sine * u_sine, transverse),
                    (radius * v_cosine, *axis),
                ]),
                duu: vector_sum(&[
                    (-radius * v_cosine * u_cosine, *ref_direction),
                    (-radius * v_cosine * u_sine, transverse),
                ]),
                duv: vector_sum(&[
                    (radius * v_sine * u_sine, *ref_direction),
                    (-radius * v_sine * u_cosine, transverse),
                ]),
                dvv: vector_sum(&[
                    (-radius * v_cosine * u_cosine, *ref_direction),
                    (-radius * v_cosine * u_sine, transverse),
                    (-radius * v_sine, *axis),
                ]),
            })
        }
        SolvedSurfaceGeometry::Torus(torus_surface) => {
            let center = torus_surface.center().get();
            let axis = torus_surface.frame().axis().as_raw();
            let ref_direction = torus_surface.frame().reference().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            let transverse = axis.cross(*ref_direction);
            let u_cosine = u.cos();
            let u_sine = u.sin();
            let v_cosine = v.cos();
            let v_sine = v.sin();
            let ring = major_radius + minor_radius * v_cosine;
            Some(SurfaceSecondPartials {
                point: offset(
                    center,
                    &[
                        (ring * u_cosine, *ref_direction),
                        (ring * u_sine, transverse),
                        (minor_radius * v_sine, *axis),
                    ],
                ),
                du: vector_sum(&[
                    (-ring * u_sine, *ref_direction),
                    (ring * u_cosine, transverse),
                ]),
                dv: vector_sum(&[
                    (-minor_radius * v_sine * u_cosine, *ref_direction),
                    (-minor_radius * v_sine * u_sine, transverse),
                    (minor_radius * v_cosine, *axis),
                ]),
                duu: vector_sum(&[
                    (-ring * u_cosine, *ref_direction),
                    (-ring * u_sine, transverse),
                ]),
                duv: vector_sum(&[
                    (minor_radius * v_sine * u_sine, *ref_direction),
                    (-minor_radius * v_sine * u_cosine, transverse),
                ]),
                dvv: vector_sum(&[
                    (-minor_radius * v_cosine * u_cosine, *ref_direction),
                    (-minor_radius * v_cosine * u_sine, transverse),
                    (-minor_radius * v_sine, *axis),
                ]),
            })
        }
        SolvedSurfaceGeometry::Nurbs(_)
        | SolvedSurfaceGeometry::Transformed(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Unknown { .. } => None,
    }
}

/// The point with the first and second partials of a directly stored
/// surface at `(u, v)`, each partial order with its own outcome, or why the
/// point has none. Within a work slice, NURBS carriers charge their partials
/// work and placed carriers each placement; a refused charge leaves no
/// value.
///
/// The descent is bounded by [`PlacedSurface`](crate::geometry::PlacedSurface)
/// construction; no arm follows an arena id.
fn surface_requested_jet_solved(
    scratch: &decode::Scratch<'_, '_>,
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
    request: SurfaceRequest,
) -> Result<RequestedJet, EvaluationFailure<Point3>> {
    scratch.unless_refused().map_err(EvaluationFailure::ResourceLimit)?;
    match geometry {
        SolvedSurfaceGeometry::Nurbs(nurbs) => nurbs_surface_requested_jet(scratch, nurbs, u, v, request),
        SolvedSurfaceGeometry::Transformed(placed) => {
            scratch.admission.independent_cost(Some(1))?;
            scratch.work(1, "placed surface partial step")
                .ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
            let _depth = scratch.enter().ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
            let transform = *placed.transform();
            let basis = surface_requested_jet_solved(scratch, placed.basis(), u, v, request)
                .map_err(|failure| failure.map(|point| placed_reach(transform, point)))?;
            Ok(RequestedJet { jet: placed_jet(transform, Ok(basis.jet))?, higher: basis.higher.placed(transform) })
        }
        _ => {
            let partials = analytic_surface_second_partials(geometry, u, v).ok_or(EvaluationFailure::NoValue)?;
            let jet = SurfaceJet::formed(admit_point(partials.point)?,
                Ok([partials.du, partials.dv]), Ok([partials.duu, partials.duv, partials.dvv]));
            let higher = if matches!(geometry, SolvedSurfaceGeometry::Plane(_)) {
                HigherPartials::Affine
            } else if request.needs_third() {
                HigherPartials::Third(surface_request::differentials::analytic_third(geometry, u, v, jet))
            } else { HigherPartials::Third(Err(EvaluationFailure::NoValue)) };
            Ok(RequestedJet { jet, higher })
        }
    }
}

/// The point with the first partials of a directly stored surface at
/// `(u, v)`, the partials with their own outcome, or why the point has none,
/// in the terms of [`surface_requested_jet_solved`].
///
/// The descent is bounded by [`PlacedSurface`](crate::geometry::PlacedSurface)
/// construction; no arm follows an arena id.
fn surface_first_order_solved(
    scratch: &decode::Scratch<'_, '_>,
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    scratch
        .unless_refused()
        .map_err(EvaluationFailure::ResourceLimit)?;
    match geometry {
        SolvedSurfaceGeometry::Nurbs(nurbs) => nurbs_surface_first_order(scratch, nurbs, u, v),
        SolvedSurfaceGeometry::Transformed(placed) => {
            scratch.admission.independent_cost(Some(1))?;
            scratch
                .work(1, "placed surface partial step")
                .ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
            let _depth = scratch
                .enter()
                .ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
            let transform = *placed.transform();
            let basis = surface_first_order_solved(scratch, placed.basis(), u, v)
                .map_err(|failure| failure.map(|point| placed_reach(transform, point)))?;
            Ok(SurfaceFirstOrder {
                point: transform
                    .apply_point_reaching(basis.point.get())
                    .map_err(EvaluationFailure::NonFinite)?,
                first: basis
                    .first
                    .and_then(|vectors| placed_vectors(transform, vectors)),
            })
        }
        _ => {
            let partials = analytic_surface_second_partials(geometry, u, v)
                .ok_or(EvaluationFailure::NoValue)?;
            Ok(SurfaceFirstOrder {
                point: admit_point(partials.point)?,
                first: admit_lanes([partials.du, partials.dv]),
            })
        }
    }
}

/// The point a placement reaches from a basis point outside the finite
/// range, finite or not.
fn placed_reach(transform: Transform, point: Point3) -> Point3 {
    transform
        .apply_point_reaching(point)
        .map_or_else(|point| point, FinitePoint3::get)
}

/// Vectors under a placement's linear part: a vector the placement leaves
/// outside the finite range leaves them there.
fn placed_vectors<const N: usize>(
    transform: Transform,
    vectors: [FiniteVector3; N],
) -> Result<[FiniteVector3; N], EvaluationFailure<()>> {
    let mut placed = vectors;
    for vector in &mut placed {
        *vector = transform
            .apply_vector(vector.get())
            .ok_or(EvaluationFailure::NonFinite(()))?;
    }
    Ok(placed)
}

/// A placed carrier's jet from its basis jet: the placed point, and each
/// order under the placement's linear part. A basis point outside the finite
/// range carries the point the placement reaches from it.
fn placed_jet(
    transform: Transform,
    basis: Result<SurfaceJet, EvaluationFailure<Point3>>,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    let basis = basis.map_err(|failure| failure.map(|point| placed_reach(transform, point)))?;
    Ok(SurfaceJet {
        point: transform
            .apply_point_reaching(basis.point.get())
            .map_err(EvaluationFailure::NonFinite)?,
        first: basis
            .first
            .and_then(|vectors| placed_vectors(transform, vectors)),
        second: basis
            .second
            .and_then(|vectors| placed_vectors(transform, vectors)),
    })
}

/// Evaluate a directly stored surface and its exact first and second partial
/// derivatives, or report why they have no finite value.
///
/// The point fails as [`decode::surface_point_solved`] states. At a finite point,
/// partials outside the finite range leave the evaluation there, carrying
/// the point.
pub fn surface_second_partials_solved<'ctx, 'arena: 'ctx>(
    admission: impl Into<admission::EvaluationAdmission<'ctx, 'arena>>,
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    let scratch = decode::Scratch::new(admission);
    let result = (|| surface_requested_jet_solved(&scratch, geometry, u, v, SurfaceRequest::Second)?.jet.second_partials())();
    scratch.settle(result)
}

/// Evaluate a surface carrier with access to construction and child-carrier
/// arenas in `ir`.
pub fn model_surface_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    ir: &CadIr,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    admission.within_model(|admission| model_surface_point_inner(admission, ir, geometry, u, v))
}

fn model_surface_point_inner(
    admission: admission::EvaluationAdmission<'_, '_>,
    ir: &CadIr,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let budget = admission.work_slice();
    let _depth =
        ModelEvaluationDepthGuard::enter(budget).map_err(EvaluationFailure::ResourceLimit)?;
    if let Some(cache) = geometry.solved_cache() {
        return crate::eval::decode::surface_point_solved(admission, cache, u, v);
    }
    let Some(construction) = geometry.procedural_construction() else {
        return crate::eval::decode::surface_point(admission, geometry, u, v);
    };
    let mut procedural = None;
    for candidate in &ir.model.procedural_surfaces {
        admission
            .work(1, "model surface construction scan")
            .map_err(EvaluationFailure::ResourceLimit)?;
        let matches = crate::ids::comparison::equal(
            &admission,
            candidate.id.as_str(),
            construction.as_str(),
            "model surface construction identity",
        )
        .map_err(EvaluationFailure::ResourceLimit)?;
        if matches {
            procedural = Some(candidate);
            break;
        }
    }
    let procedural = procedural.ok_or(EvaluationFailure::NoValue)?;
    let carrier_interval = record_u_interval(procedural.record_bounds());
    let decoded;
    let standard;
    let index = match admission.context() {
        Some(ctx) => {
            decoded = crate::index::ModelIndex::build(ir, ctx)
                .map_err(EvaluationFailure::ResourceLimit)?;
            &*decoded
        }
        None => {
            standard = crate::index::ModelIndex::build(ir, crate::index::StandardIndex);
            &standard
        }
    };
    match procedural.definition() {
        ProceduralSurfaceDefinition::Extrusion(definition_payload) => model_native_extrusion_point(
            admission,
            index,
            definition_payload,
            carrier_interval,
            u,
            v,
        ),
        ProceduralSurfaceDefinition::LinearSweep(definition_payload) => {
            model_linear_sweep_point(admission, index, definition_payload, u, v)
        }
        ProceduralSurfaceDefinition::Revolution(definition_payload) => {
            model_native_revolution_point(
                admission,
                index,
                definition_payload,
                carrier_interval,
                u,
                v,
            )
        }
        ProceduralSurfaceDefinition::AxisRevolution(definition_payload) => {
            model_axis_revolution_point(
                admission,
                index,
                definition_payload.directrix(),
                definition_payload.axis_origin().get(),
                definition_payload.axis_direction(),
                u,
                v,
            )
        }
        ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
            model_ruled_surface_point(admission, index, first, second, u, v)
        }
        ProceduralSurfaceDefinition::Sum(definition_payload) => {
            model_sum_surface_point(admission, index, definition_payload, u, v)
        }
        ProceduralSurfaceDefinition::Sweep(definition_payload) => {
            let construction = definition_payload
                .native()
                .as_deref()
                .ok_or(EvaluationFailure::NoValue)?;
            cacheless_law_sweep_point(
                admission,
                index,
                definition_payload.profile(),
                definition_payload.spine(),
                construction,
                u,
                v,
            )
            .and_then(admit_point)
        }
        ProceduralSurfaceDefinition::VariableBlend(definition_payload) => {
            cacheless_variable_blend_point(admission, index, definition_payload, u, v)
                .and_then(admit_point)
        }
        ProceduralSurfaceDefinition::Blend(definition_payload) => {
            cacheless_constant_rolling_ball_point(admission, index, definition_payload, u, v)
                .map_err(|failure| failure.map(|()| UNREACHED_POINT))
                .and_then(admit_point)
        }
        ProceduralSurfaceDefinition::RollingBallJet(_) => {
            rolling_ball_jet_point_admitted(admission, procedural.definition(), u, v)
        }
        _ => Err(EvaluationFailure::NoValue),
    }
}

/// The point of a linear sweep: the directrix point displaced by `v` along
/// the sweep direction. A `v` that is not finite has no value; a directrix
/// point outside the finite range leaves the surface there, at the point
/// its displacement reaches.
fn model_linear_sweep_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::LinearSweepSurfaceConstruction,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    if !v.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let directrix = construction.directrix();
    let direction = construction.direction().get();
    let point = model_curve_point_by_id(admission, index, directrix, u)
        .map_err(|failure| failure.map(|point| offset(point, &[(v, direction)])))?;
    admit_point(offset(point.get(), &[(v, direction)]))
}

/// The jet of a linear sweep, or why its point has none. The first partials
/// read the directrix tangent, and the second its acceleration.
fn model_linear_sweep_jet(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::LinearSweepSurfaceConstruction,
    u: f64,
    v: f64,
    request: SurfaceRequest,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    if !v.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let directrix = construction.directrix();
    let direction = *construction.direction().finite();
    let differential = model_curve_differential_by_id_inner(admission, index, directrix, u, ModelCurveRequest::for_surface_partials(request))
        .map_err(|failure| failure.map(|point| offset(point, &[(v, direction.get())])))?;
    Ok(SurfaceJet {
        point: admit_point(offset(differential.point.get(), &[(v, direction.get())]))?,
        first: differential.tangent.map(|tangent| [tangent, direction]),
        second: differential
            .acceleration
            .map(|acceleration| [acceleration, FiniteVector3::ZERO, FiniteVector3::ZERO]),
    })
}

/// The profile differential scaled about `frame_point`. A scaled point
/// outside the finite range is non-finite; each scaled derivative states its
/// own outcome.
fn scale_sweep_profile(
    profile: ModelCurveDifferential,
    frame_point: Point3,
    scale: Vector3,
) -> Result<ModelCurveDifferential, EvaluationFailure<Point3>> {
    let scaled =
        |vector: Vector3| Vector3::new(vector.x * scale.x, vector.y * scale.y, vector.z * scale.z);
    let point = offset(
        frame_point,
        &[(
            1.0,
            scaled(point_displacement(profile.point.get(), frame_point)),
        )],
    );
    Ok(ModelCurveDifferential {
        point: admit_point(point)?,
        tangent: profile
            .tangent
            .and_then(|tangent| admit_derivative(scaled(tangent.get()))),
        acceleration: profile
            .acceleration
            .and_then(|acceleration| admit_derivative(scaled(acceleration.get()))),
    })
}

fn unit_domain_sweep_formula(
    admission: admission::EvaluationAdmission<'_, '_>,
    name: &str,
) -> Result<bool, EvaluationFailure<()>> {
    let Some(bounds) = name
        .strip_prefix("DOMAIN(VEC(1,0,0),")
        .and_then(|name| name.strip_suffix(')'))
    else {
        return Ok(false);
    };
    let Some([lower, upper]) = sweep_law::sweep_number_fields::<2>(admission, bounds)? else {
        return Ok(false);
    };
    Ok(lower.is_finite() && upper.is_finite() && lower < upper)
}

fn sweep_rail_transform(
    admission: admission::EvaluationAdmission<'_, '_>,
    formula: &LawFormula<FiniteReal, FiniteVector3, FinitePoint3>,
) -> Result<Option<Transform>, EvaluationFailure<()>> {
    let scratch = decode::Scratch::new(admission);
    let result = (|| {
        let (name, variables) = match formula {
            LawFormula::Null {} => return Ok(Some(Transform::identity())),
            LawFormula::Named { name, variables } => (name, variables),
        };
        let name = sweep_law::compact_sweep_text(&scratch, name.as_str())?;
        if variables.is_empty() {
            return Ok(
                unit_domain_sweep_formula(admission, &name)?.then_some(Transform::identity())
            );
        }
        let Some(inner) = name
            .strip_prefix("ROTATE(")
            .and_then(|name| name.strip_suffix(",TRANS1)"))
        else {
            return Ok(None);
        };
        if !unit_domain_sweep_formula(admission, inner)? {
            return Ok(None);
        }
        let [LawExpression::TransformVec {
            vectors,
            scale,
            flags,
        }] = variables.as_slice()
        else {
            return Ok(None);
        };
        if scale.get() != 1.0
            || *flags != [true, false, false]
            || vectors[3].get() != Vector3::new(0.0, 0.0, 0.0)
        {
            return Ok(None);
        }
        let [x, y, z] = [vectors[0], vectors[1], vectors[2]].map(FiniteVector3::components);
        let zero = FiniteReal::ZERO;
        let transform = Transform::from_finite_rows([
            [x[0], y[0], z[0], zero],
            [x[1], y[1], z[1], zero],
            [x[2], y[2], z[2], zero],
        ]);
        Ok(transform.is_proper_rigid().then_some(transform))
    })();
    scratch.settle(result)
}

/// The origin of a straight sweep path: a line's origin, or the start of a
/// two-pole linear NURBS. Any other spine has no straight origin.
fn straight_sweep_path_origin(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    spine: &crate::ids::CurveId,
) -> Result<Point3, EvaluationFailure<()>> {
    let curve = index
        .curves(spine.as_str(), admission)
        .map_err(EvaluationFailure::ResourceLimit)?
        .ok_or(EvaluationFailure::NoValue)?;
    match &curve.geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            Ok(line_curve.origin().get())
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
            if nurbs.degree() == 1 && nurbs.pole_rows().count() == 2 && !nurbs.periodic() =>
        {
            let [start, _] = nurbs_curve_parameter_domain(nurbs)
                .ok_or(EvaluationFailure::NoValue)?
                .endpoints();
            crate::eval::decode::curve_point(admission, &curve.geometry, start)
                .map(FinitePoint3::get)
                .map_err(|failure| failure.map(|_| ()))
        }
        _ => Err(EvaluationFailure::NoValue),
    }
}

fn point_displacement(point: Point3, origin: Point3) -> Vector3 {
    Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z)
}

fn sweep_tail_interval_contains(interval: [Option<FiniteReal>; 2], parameter: FiniteReal) -> bool {
    interval[0].is_none_or(|lower| parameter >= lower)
        && interval[1].is_none_or(|upper| parameter <= upper)
}

/// The unit direction of a nonzero `vector`, none for the zero vector, and
/// the unit direction's derivative given the vector's derivative, with its
/// own outcome: a vector derivative without a value leaves it without one,
/// and a quotient that overflows leaves it outside the finite range.
fn unit_vector_with_derivative(
    vector: FiniteVector3,
    derivative: Result<FiniteVector3, EvaluationFailure<()>>,
) -> Option<(Vector3, Result<Vector3, EvaluationFailure<()>>)> {
    let (unit, cubed_length) = vector.unit_with_cubed_length()?;
    let vector = vector.get();
    let components = [vector.x, vector.y, vector.z];
    let denominator = crate::math::sum::ScaledValue::product_of_nonzero(cubed_length)?;
    let unit_derivative = derivative.and_then(|derivative| {
        let derivative = derivative.get();
        let derivatives = [derivative.x, derivative.y, derivative.z];
        // (|v|² d - v(v·d)) / |v|³. Exact products keep parallel derivatives
        // zero and postpone range checks until the normalized result is
        // formed.
        let component = |index: usize| {
            let mut numerator = ExactSignedSum::default();
            for other in 0..3 {
                if other != index {
                    numerator.add_factors([
                        components[other],
                        components[other],
                        derivatives[index],
                    ]);
                    numerator.add_factors([
                        -components[index],
                        components[other],
                        derivatives[other],
                    ]);
                }
            }
            numerator.finish().map_or(Ok(0.0), |value| {
                value
                    .quotient(denominator)
                    .map(FiniteReal::get)
                    .map_err(|_| EvaluationFailure::NonFinite(()))
            })
        };
        Ok(Vector3::new(component(0)?, component(1)?, component(2)?))
    });
    Some((unit, unit_derivative))
}

/// Whether the profile frame runs against the spine tangent. A frame
/// vector or spine tangent that is zero or not aligned states no direction;
/// one whose length overflows still has a direction.
fn sweep_profile_reversed(
    profile_frame: Option<(FinitePoint3, FiniteVector3)>,
    spine_tangent: Vector3,
) -> Result<bool, EvaluationFailure<()>> {
    let Some((_, frame_vector)) = profile_frame else {
        return Ok(false);
    };
    let unit = |vector: Vector3| {
        let finite = FiniteVector3::new(vector).ok_or(EvaluationFailure::NonFinite(()))?;
        if vector.norm() <= f64::EPSILON {
            return Err(EvaluationFailure::NoValue);
        }
        finite.unit_nonzero().ok_or(EvaluationFailure::NoValue)
    };
    let frame_vector = unit(frame_vector.get())?;
    let spine_tangent = unit(spine_tangent)?;
    let alignment = frame_vector.dot(spine_tangent);
    ((alignment.abs() - 1.0).abs() <= EPS_EVAL_SWEEP_PROFILE_FRAME_ALIGNMENT_E9)
        .then_some(alignment < 0.0)
        .ok_or(EvaluationFailure::NoValue)
}

/// Evaluate a sweep profile in its native domain and scale its derivatives.
/// Out-of-range parameters and reverse mappings without a native domain have
/// no value; a mapped parameter outside finite range reaches no coordinate.
fn sweep_profile_differential(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    profile: &crate::ids::CurveId,
    profile_range: [FiniteReal; 2],
    reversed: bool,
    parameter: FiniteReal,
) -> Result<ModelCurveDifferential, EvaluationFailure<Point3>> {
    let no_value = EvaluationFailure::NoValue;
    let unreached = EvaluationFailure::NonFinite(UNREACHED_POINT);
    if !sweep_tail_interval_contains([Some(profile_range[0]), Some(profile_range[1])], parameter) {
        return Err(no_value);
    }
    let profile_interval =
        IncreasingParameterInterval::new(FiniteReal::raw_array(profile_range)).ok_or(no_value)?;
    let curve = index
        .curves(profile.as_str(), admission)
        .map_err(EvaluationFailure::ResourceLimit)?
        .ok_or(no_value)?;
    let (native_parameter, parameter_scale) = match &curve.geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            let native_interval = nurbs_curve_parameter_domain(nurbs).ok_or(no_value)?;
            let native_parameter = native_interval
                .map_from(profile_interval, parameter, reversed)
                .map_err(|_| unreached)?
                .get();
            let scale = native_interval
                .scaled_span()
                .quotient(profile_interval.scaled_span())
                .map_or_else(|overflow| overflow, FiniteReal::get);
            (native_parameter, if reversed { -scale } else { scale })
        }
        _ if !reversed => (parameter.get(), 1.0),
        _ => return Err(no_value),
    };
    let differential = model_curve_differential_by_id(admission, index, profile, native_parameter, ModelCurveRequest::Second)?;
    Ok(ModelCurveDifferential {
        point: differential.point,
        tangent: differential
            .tangent
            .and_then(|tangent| admit_derivative(scale_vector(tangent.get(), parameter_scale))),
        acceleration: differential.acceleration.and_then(|acceleration| {
            admit_derivative(scale_vector(
                acceleration.get(),
                parameter_scale * parameter_scale,
            ))
        }),
    })
}

/// The spine of a law-driven sweep at a spine parameter: its point, its
/// tangent, which the section frame reads, and its acceleration with its
/// own outcome.
#[derive(Clone, Copy)]
struct SweepSpine {
    point: FinitePoint3,
    tangent: FiniteVector3,
    acceleration: Result<FiniteVector3, EvaluationFailure<()>>,
}

/// The placed profile differential, the spine, the law value and derivative,
/// and the straight path origin of a law-driven sweep at `(u, v)`, or why
/// there are none. A sweep outside the law-driven straight form the
/// evaluator reads, and a parameter outside its intervals, have no value.
/// The section frame reads the spine tangent, so a spine tangent without a
/// value fails the sweep, carrying the spine point.
fn cacheless_law_sweep_differentials(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    profile: &crate::ids::CurveId,
    spine: &crate::ids::CurveId,
    construction: &crate::geometry::SweepSurfaceConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Result<
    (
        ModelCurveDifferential,
        SweepSpine,
        sweep_law::ScalarSweepDifferential,
        Point3,
    ),
    EvaluationFailure<Point3>,
> {
    let no_value = EvaluationFailure::NoValue;
    let unreached = |failure: EvaluationFailure<()>| failure.map(|()| UNREACHED_POINT);
    let form = construction.cache.form().ok_or(no_value)?;
    let parameterization = form.cache.parameterization().ok_or(no_value)?;
    let path_origin = straight_sweep_path_origin(admission, index, spine).map_err(unreached)?;
    let SweepSurfaceLayout::LawDriven {
        profile_range,
        profile_frame,
        origin,
        first_law,
        first_range,
        path_mode,
        second_law,
        formula,
        formula_mode,
        trailing_flag,
        ..
    } = &construction.layout
    else {
        return Err(no_value);
    };
    let rail_transform = sweep_rail_transform(admission, formula)
        .map_err(unreached)?
        .ok_or(no_value)?;
    let scale = sweep_law::sweep_scale(admission, second_law)
        .map_err(unreached)?
        .ok_or(no_value)?;
    let (Some(u), Some(v)) = (FiniteReal::new(u), FiniteReal::new(v)) else {
        return Err(no_value);
    };
    if *path_mode != 1
        || *formula_mode != 0
        || *trailing_flag
        || !sweep_tail_interval_contains(parameterization.u_interval, u)
        || !sweep_tail_interval_contains(parameterization.v_interval, v)
        || !sweep_tail_interval_contains([Some(first_range[0]), Some(first_range[1])], v)
    {
        return Err(no_value);
    }
    let spine = model_curve_differential_by_id(admission, index, spine, v.get(), ModelCurveRequest::Second)?;
    let spine = SweepSpine {
        point: spine.point,
        tangent: spine.tangent()?,
        acceleration: spine.acceleration,
    };
    let reversed =
        sweep_profile_reversed(*profile_frame, spine.tangent.get()).map_err(unreached)?;
    let profile =
        sweep_profile_differential(admission, index, profile, *profile_range, reversed, u)?;
    let frame_point = profile_frame.map_or(*origin, |(point, _)| point).get();
    let profile = scale_sweep_profile(profile, frame_point, scale)?;
    let profile = ModelCurveDifferential {
        point: rail_transform
            .apply_point_reaching(profile.point.get())
            .map_err(EvaluationFailure::NonFinite)?,
        tangent: placed_derivative(rail_transform, profile.tangent),
        acceleration: placed_derivative(rail_transform, profile.acceleration),
    };
    let law =
        sweep_law::scalar_sweep_law_differential(admission, first_law, v).map_err(unreached)?;
    Ok((profile, spine, law, path_origin))
}

/// The raw point of a law-driven sweep, or why it has none: the profile
/// point displaced along the spine and by the law along the section normal.
/// The normal reads the profile and spine tangents; the point reads no
/// acceleration and no law derivative.
fn cacheless_law_sweep_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    profile: &crate::ids::CurveId,
    spine: &crate::ids::CurveId,
    construction: &crate::geometry::SweepSurfaceConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Result<Point3, EvaluationFailure<Point3>> {
    let (profile, spine, law, path_origin) =
        cacheless_law_sweep_differentials(admission, index, profile, spine, construction, u, v)?;
    let profile_tangent = profile
        .tangent()?
        .unit_nonzero()
        .ok_or(EvaluationFailure::NoValue)?;
    let spine_tangent = spine
        .tangent
        .unit_nonzero()
        .ok_or(EvaluationFailure::NoValue)?;
    let normal = profile_tangent.cross(spine_tangent);
    Ok(offset(
        profile.point.get(),
        &[
            (1.0, point_displacement(spine.point.get(), path_origin)),
            (law.value.get(), normal),
        ],
    ))
}

/// The point and first partials of a law-driven sweep, the partials with
/// their own outcome, or why the point has none. The partials read the
/// profile and spine accelerations and the law derivative besides what the
/// point reads.
fn cacheless_law_sweep_first_order(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    profile: &crate::ids::CurveId,
    spine: &crate::ids::CurveId,
    construction: &crate::geometry::SweepSurfaceConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let (profile, spine, law, path_origin) =
        cacheless_law_sweep_differentials(admission, index, profile, spine, construction, u, v)?;
    let profile_tangent = profile.tangent()?;
    let (profile_unit, profile_unit_derivative) =
        unit_vector_with_derivative(profile_tangent, profile.acceleration)
            .ok_or(EvaluationFailure::NoValue)?;
    let (spine_unit, spine_unit_derivative) =
        unit_vector_with_derivative(spine.tangent, spine.acceleration)
            .ok_or(EvaluationFailure::NoValue)?;
    let normal = profile_unit.cross(spine_unit);
    let point = admit_point(offset(
        profile.point.get(),
        &[
            (1.0, point_displacement(spine.point.get(), path_origin)),
            (law.value.get(), normal),
        ],
    ))?;
    let first = (|| {
        let normal_u =
            profile_unit_derivative?.cross(spine_unit) + profile_unit.cross(spine_unit_derivative?);
        admit_lanes([
            profile_tangent.get() + scale_vector(normal_u, law.value.get()),
            spine.tangent.get() + scale_vector(normal, law.derivative?.get()),
        ])
    })();
    Ok(SurfaceFirstOrder { point, first })
}

/// The unit direction of a vector formed from finite values: a component
/// outside the finite range leaves the evaluation there, and a vector within
/// [`f64::EPSILON`] of zero has no direction.
fn unit_direction(vector: Vector3) -> Result<Vector3, EvaluationFailure<()>> {
    if !vector.is_finite() {
        return Err(EvaluationFailure::NonFinite(()));
    }
    vector.unit().ok_or(EvaluationFailure::NoValue)
}

/// The unit cross direction of finite partials. Normalizing each partial
/// before crossing retains the direction when their raw cross overflows.
fn unit_cross_direction(
    first: FiniteVector3,
    second: FiniteVector3,
) -> Result<Vector3, EvaluationFailure<()>> {
    let first = first.unit_nonzero().ok_or(EvaluationFailure::NoValue)?;
    let second = second.unit_nonzero().ok_or(EvaluationFailure::NoValue)?;
    first.cross(second).unit().ok_or(EvaluationFailure::NoValue)
}

/// The contact track of a blend side at a parameter: the support's point
/// with its first partials, the side pcurve's tangent, and the derivative of
/// the support's unit normal along the track, each derivative with its own
/// outcome.
#[derive(Clone, Copy)]
struct ContactTrack {
    support: SurfaceFirstOrder,
    uv_tangent: Result<FinitePoint2, EvaluationFailure<()>>,
    normal_derivative: Result<Vector3, EvaluationFailure<()>>,
}

impl ContactTrack {
    /// The support point.
    fn point(&self) -> Point3 {
        self.support.point.get()
    }

    /// The support's unit normal: a degenerate normal has no direction.
    fn normal(&self) -> Result<Vector3, EvaluationFailure<()>> {
        let [du, dv] = self.support.first?;
        let cross = du.get().cross(dv.get());
        if cross.is_finite() {
            unit_direction(cross)
        } else {
            unit_cross_direction(du, dv)
        }
    }

    /// The track tangent: the pcurve tangent carried by the support's first
    /// partials.
    fn tangent(&self) -> Result<Vector3, EvaluationFailure<()>> {
        let uv_tangent = self.uv_tangent?;
        let [du, dv] = self.support.first?;
        Ok(vector_sum(&[
            (uv_tangent.u, du.get()),
            (uv_tangent.v, dv.get()),
        ]))
    }
}

/// The contact track of a blend side at `parameter`, or why its support
/// point has none. A side without a support or pcurve, and a pcurve without
/// a point there, have no value. The track's derivatives state their own
/// outcomes.
fn variable_blend_contact_track(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    side: &crate::geometry::RollingBallSide<
        crate::ids::SurfaceId,
        crate::ids::CurveId,
        PcurveGeometry,
        FiniteReal,
        FinitePoint3,
    >,
    parameter: f64,
) -> Result<ContactTrack, EvaluationFailure<()>> {
    let no_value = EvaluationFailure::NoValue;
    let surface = &side.surface.as_ref().ok_or(no_value)?.surface;
    let pcurve = side.pcurve.as_ref().ok_or(no_value)?;
    // A non-finite offset-pcurve point is evaluated on its support as a
    // finite one is.
    let uv = match crate::eval::decode::pcurve_uv(admission, pcurve, parameter) {
        Ok(uv) => uv.get(),
        Err(EvaluationFailure::NonFinite(uv)) => uv,
        Err(EvaluationFailure::NoValue) => return Err(no_value),
        Err(EvaluationFailure::ResourceLimit(limit)) => {
            return Err(EvaluationFailure::ResourceLimit(limit))
        }
    };
    let support = model_surface_first_order_by_id(admission, index, surface, uv.u, uv.v)
        .map_err(|failure| failure.map(|_| ()))?;
    let uv_tangent =
        pcurve_tangent(admission, pcurve, parameter).map_err(|failure| failure.map(|_| ()));
    if let Err(EvaluationFailure::ResourceLimit(limit)) = uv_tangent {
        return Err(EvaluationFailure::ResourceLimit(limit));
    }
    let normal_derivative = uv_tangent.and_then(|uv_tangent| {
        let support = model_surface_second_partials_by_id(admission, index, surface, uv.u, uv.v)
            .map_err(|failure| failure.map(|_| ()))?;
        let [du, dv, duu, duv, dvv] = FiniteVector3::raw_array([
            support.du,
            support.dv,
            support.duu,
            support.duv,
            support.dvv,
        ]);
        let du_along = vector_sum(&[(uv_tangent.u, duu), (uv_tangent.v, duv)]);
        let dv_along = vector_sum(&[(uv_tangent.u, duv), (uv_tangent.v, dvv)]);
        let normal = admit_derivative(du.cross(dv))?;
        let normal_derivative = admit_derivative(du_along.cross(dv) + du.cross(dv_along));
        let (_, derivative) = unit_vector_with_derivative(normal, normal_derivative)
            .ok_or(EvaluationFailure::NoValue)?;
        derivative
    });
    if let Err(EvaluationFailure::ResourceLimit(limit)) = normal_derivative {
        return Err(EvaluationFailure::ResourceLimit(limit));
    }
    Ok(ContactTrack {
        support,
        uv_tangent,
        normal_derivative,
    })
}

fn cacheless_variable_blend_domain_contains(
    payload: &crate::geometry::surface_payloads::VariableBlendSurfacePayload,
    u: FiniteReal,
    v: FiniteReal,
) -> bool {
    let construction = payload.construction();
    let exact_construction = matches!(
        construction.cache,
        crate::geometry::VariableBlendCache::Parameterization { .. }
            | crate::geometry::VariableBlendCache::Stale {}
    );
    exact_construction
        && (0.0..=1.0).contains(&u.get())
        && sweep_tail_interval_contains(payload.slice_range().endpoints(), v)
        && construction.cache.parameterization().is_none_or(|tail| {
            sweep_tail_interval_contains(tail.u_interval, u)
                && sweep_tail_interval_contains(tail.v_interval, v)
        })
}

/// The finite blend parameters `(u, v)`, or no value where either is not
/// finite.
fn blend_parameters(u: f64, v: f64) -> Result<(FiniteReal, FiniteReal), EvaluationFailure<()>> {
    match (FiniteReal::new(u), FiniteReal::new(v)) {
        (Some(u), Some(v)) => Ok((u, v)),
        _ => Err(EvaluationFailure::NoValue),
    }
}

fn variable_blend_has_current_cache(
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
) -> bool {
    construction.cache.shape_prefix() > 0
        && matches!(
            construction.cache,
            crate::geometry::VariableBlendCache::Current { .. }
        )
}

fn sweep_has_current_cache(
    construction: &crate::geometry::SweepSurfaceConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
) -> bool {
    construction
        .cache
        .form()
        .is_some_and(|form| revision_surface_tail_has_current_cache(&form.cache))
}

fn revision_surface_tail_has_current_cache<P>(
    cache: &crate::geometry::RevisionCacheForm<P>,
) -> bool {
    matches!(
        cache,
        crate::geometry::RevisionCacheForm::SolvedCache { .. }
    )
}

fn variable_blend_is_zero_radius(
    admission: admission::EvaluationAdmission<'_, '_>,
    value: &crate::geometry::VariableBlendValue<FiniteReal, FiniteVector3, FinitePoint3>,
) -> Result<bool, EvaluationFailure<()>> {
    admission.within_model(|admission| {
        let _depth = ModelEvaluationDepthGuard::enter(admission.work_slice())
            .map_err(EvaluationFailure::ResourceLimit)?;
        admission.model_step()?;
        let zero = match &value.payload {
            crate::geometry::VariableBlendValuePayload::TwoEnds {
                parameters: [first_parameter, second_parameter],
                radii: [first_radius, second_radius],
                ..
            } => {
                first_parameter != second_parameter
                    && first_radius.get() == 0.0
                    && second_radius.get() == 0.0
            }
            crate::geometry::VariableBlendValuePayload::Constant { radius, nested, .. } => {
                radius.get() == 0.0 && variable_blend_is_zero_radius(admission, nested)?
            }
            _ => false,
        };
        Ok(zero)
    })
}

/// The two contact tracks of a zero-radius rounded chamfer at `(u, v)`, or
/// why there are none: a blend outside that form or its domain has no
/// value.
fn cacheless_ruled_variable_blend_tracks(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    payload: &crate::geometry::surface_payloads::VariableBlendSurfacePayload,
    u: f64,
    v: f64,
) -> Result<[ContactTrack; 2], EvaluationFailure<()>> {
    let construction = payload.construction();
    let no_value = EvaluationFailure::NoValue;
    let Some(crate::geometry::VariableBlendCrossSection::RoundedChamfer { radius }) =
        construction.cross_section.as_ref()
    else {
        return Err(no_value);
    };
    let (finite_u, finite_v) = blend_parameters(u, v)?;
    if !cacheless_variable_blend_domain_contains(payload, finite_u, finite_v) {
        return Err(no_value);
    }
    if let Some(radius) = radius.as_deref() {
        if !variable_blend_is_zero_radius(admission, radius)? {
            return Err(no_value);
        }
    }
    Ok([
        variable_blend_contact_track(admission, index, &construction.sides[0], v)?,
        variable_blend_contact_track(admission, index, &construction.sides[1], v)?,
    ])
}

/// The point of a zero-radius rounded chamfer: its chord between the two
/// contact points at fraction `u`. It reads the contact points only.
fn cacheless_ruled_variable_blend_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    payload: &crate::geometry::surface_payloads::VariableBlendSurfacePayload,
    u: f64,
    v: f64,
) -> Result<Point3, EvaluationFailure<()>> {
    let [first, second] = cacheless_ruled_variable_blend_tracks(admission, index, payload, u, v)?;
    Ok(offset(
        first.point(),
        &[(u, point_displacement(second.point(), first.point()))],
    ))
}

/// The point and first partials of a zero-radius rounded chamfer, the first
/// partials reading the track tangents, or why its point has none.
fn cacheless_ruled_variable_blend_first_order(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    payload: &crate::geometry::surface_payloads::VariableBlendSurfacePayload,
    u: f64,
    v: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let [first, second] = cacheless_ruled_variable_blend_tracks(admission, index, payload, u, v)
        .map_err(|failure| failure.map(|()| UNREACHED_POINT))?;
    let chord = point_displacement(second.point(), first.point());
    let point = admit_point(offset(first.point(), &[(u, chord)]))?;
    let tangents = first
        .tangent()
        .and_then(|first| Ok([first, second.tangent()?]));
    Ok(SurfaceFirstOrder {
        point,
        first: tangents
            .map(|[first, second]| [chord, vector_sum(&[(1.0 - u, first), (u, second)])])
            .and_then(admit_lanes),
    })
}

/// A variable blend radius at `parameter`, or why it has none. Two equal
/// end parameters, a radius function without a value there, and a payload
/// the evaluator does not read have no value; an interpolation or radius
/// function that overflows leaves the evaluation outside the finite range.
fn variable_blend_radius(
    admission: admission::EvaluationAdmission<'_, '_>,
    value: &crate::geometry::VariableBlendValue<FiniteReal, FiniteVector3, FinitePoint3>,
    parameter: f64,
) -> Result<FiniteReal, EvaluationFailure<()>> {
    let _depth = ModelEvaluationDepthGuard::enter(admission.work_slice())
        .map_err(EvaluationFailure::ResourceLimit)?;
    admission.model_step()?;
    match &value.payload {
        crate::geometry::VariableBlendValuePayload::TwoEnds {
            parameters, radii, ..
        } => {
            let [first_parameter, second_parameter] = FiniteReal::raw_array(*parameters);
            let [first_radius, second_radius] = FiniteReal::raw_array(*radii);
            let width = second_parameter - first_parameter;
            if width == 0.0 {
                return Err(EvaluationFailure::NoValue);
            }
            let fraction = (parameter - first_parameter) / width;
            FiniteReal::new(first_radius + fraction * (second_radius - first_radius))
                .ok_or(EvaluationFailure::NonFinite(()))
        }
        crate::geometry::VariableBlendValuePayload::Constant { nested, .. } => {
            variable_blend_radius(admission, nested, parameter)
        }
        crate::geometry::VariableBlendValuePayload::Functional { function, .. }
        | crate::geometry::VariableBlendValuePayload::Interpolated { function, .. } => {
            let [radius, _] = crate::eval::decode::pcurve_uv(admission, function, parameter)
                .map_err(|failure| failure.map(|_| ()))?
                .coordinates();
            Ok(radius)
        }
        _ => Err(EvaluationFailure::NoValue),
    }
}

/// The derivative of a variable blend radius at `parameter`, or why it has
/// none, in the terms of [`variable_blend_radius`].
fn variable_blend_radius_derivative(
    admission: admission::EvaluationAdmission<'_, '_>,
    value: &crate::geometry::VariableBlendValue<FiniteReal, FiniteVector3, FinitePoint3>,
    parameter: f64,
) -> Result<FiniteReal, EvaluationFailure<()>> {
    let _depth = ModelEvaluationDepthGuard::enter(admission.work_slice())
        .map_err(EvaluationFailure::ResourceLimit)?;
    admission.model_step()?;
    match &value.payload {
        crate::geometry::VariableBlendValuePayload::TwoEnds {
            parameters, radii, ..
        } => {
            let [first_parameter, second_parameter] = FiniteReal::raw_array(*parameters);
            let [first_radius, second_radius] = FiniteReal::raw_array(*radii);
            let width = second_parameter - first_parameter;
            if width == 0.0 {
                return Err(EvaluationFailure::NoValue);
            }
            sweep_law::law_real((second_radius - first_radius) / width)
        }
        crate::geometry::VariableBlendValuePayload::Constant { nested, .. } => {
            variable_blend_radius_derivative(admission, nested, parameter)
        }
        crate::geometry::VariableBlendValuePayload::Functional { function, .. }
        | crate::geometry::VariableBlendValuePayload::Interpolated { function, .. } => {
            let [derivative, _] = pcurve_tangent(admission, function, parameter)
                .map_err(|failure| failure.map(|_| ()))?
                .coordinates();
            Ok(derivative)
        }
        _ => Err(EvaluationFailure::NoValue),
    }
}

/// The point at fraction `u` along the minor arc of radius `radius` about
/// `center` from `first` to `second`. Radii that vanish or are parallel
/// state no arc; a radius outside the finite range leaves the evaluation
/// there.
fn minor_circular_arc_point(
    center: Point3,
    first: Point3,
    second: Point3,
    radius: f64,
    u: f64,
) -> Result<Point3, EvaluationFailure<()>> {
    if u == 0.0 {
        return Ok(first);
    }
    if u == 1.0 {
        return Ok(second);
    }
    let first_radius = unit_direction(Vector3::new(
        first.x - center.x,
        first.y - center.y,
        first.z - center.z,
    ))?;
    let second_radius = unit_direction(Vector3::new(
        second.x - center.x,
        second.y - center.y,
        second.z - center.z,
    ))?;
    let axis = unit_direction(first_radius.cross(second_radius))?;
    let angle = first_radius
        .cross(second_radius)
        .norm()
        .atan2(first_radius.dot(second_radius));
    let section_angle = u * angle;
    let radial = vector_sum(&[
        (section_angle.cos(), first_radius),
        (section_angle.sin(), axis.cross(first_radius)),
    ]);
    Ok(offset(center, &[(radius, radial)]))
}

/// Whether a variable blend takes the circular cross section with a single
/// radius over its domain at `(u, v)`.
fn circular_variable_blend_applies(
    payload: &crate::geometry::surface_payloads::VariableBlendSurfacePayload,
    u: FiniteReal,
    v: FiniteReal,
) -> bool {
    let construction = payload.construction();
    cacheless_variable_blend_domain_contains(payload, u, v)
        && construction.radii.is_single()
        && matches!(
            construction.cross_section,
            None | Some(crate::geometry::VariableBlendCrossSection::Circular {})
        )
}

/// The two contact tracks of a circular variable blend at `(u, v)`, or why
/// there are none: a blend outside that form or its domain has no value.
fn circular_variable_blend_tracks(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    payload: &crate::geometry::surface_payloads::VariableBlendSurfacePayload,
    u: f64,
    v: f64,
) -> Result<[ContactTrack; 2], EvaluationFailure<()>> {
    let construction = payload.construction();
    let (finite_u, finite_v) = blend_parameters(u, v)?;
    if !circular_variable_blend_applies(payload, finite_u, finite_v) {
        return Err(EvaluationFailure::NoValue);
    }
    Ok([
        variable_blend_contact_track(admission, index, &construction.sides[0], v)?,
        variable_blend_contact_track(admission, index, &construction.sides[1], v)?,
    ])
}

fn cacheless_circular_variable_blend_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    payload: &crate::geometry::surface_payloads::VariableBlendSurfacePayload,
    u: f64,
    v: f64,
) -> Result<Point3, EvaluationFailure<()>> {
    let tracks = circular_variable_blend_tracks(admission, index, payload, u, v)?;
    if u == 0.0 {
        return Ok(tracks[0].point());
    }
    if u == 1.0 {
        return Ok(tracks[1].point());
    }
    let section = cacheless_circular_variable_blend_section(
        admission,
        index,
        payload.construction(),
        v,
        tracks,
    )?;
    minor_circular_arc_point(
        section.center,
        section.first.point(),
        section.second.point(),
        section.radius,
        u,
    )
}

struct CircularVariableBlendSection {
    center: Point3,
    signs: [f64; 2],
    first: ContactTrack,
    second: ContactTrack,
    normals: [Vector3; 2],
    radius: f64,
    radius_derivative: Result<f64, EvaluationFailure<()>>,
    tolerance: f64,
}

/// The circular section of a variable blend at `v` over its contact tracks:
/// the common center of the two normal offsets by the radius, or why there
/// is none. The section reads the contact points and normals and the radius;
/// the radius derivative states its own outcome.
fn cacheless_circular_variable_blend_section(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    v: f64,
    [first, second]: [ContactTrack; 2],
) -> Result<CircularVariableBlendSection, EvaluationFailure<()>> {
    let no_value = EvaluationFailure::NoValue;
    let signed_radius = variable_blend_radius(admission, construction.radii.first(), v)?.get();
    let radius = signed_radius.abs();
    if radius <= f64::EPSILON {
        return Err(no_value);
    }
    let radius_derivative =
        variable_blend_radius_derivative(admission, construction.radii.first(), v)
            .map(|derivative| derivative.get() * signed_radius.signum());
    if let Err(EvaluationFailure::ResourceLimit(limit)) = radius_derivative {
        return Err(EvaluationFailure::ResourceLimit(limit));
    }
    let normals = [first.normal()?, second.normal()?];
    let [first_point, second_point] = [first.point(), second.point()];
    let scale = radius
        .max(first_point.x.abs())
        .max(first_point.y.abs())
        .max(first_point.z.abs())
        .max(second_point.x.abs())
        .max(second_point.y.abs())
        .max(second_point.z.abs());
    let tolerance = index
        .ir()
        .tolerances
        .linear
        .get()
        .max(256.0 * f64::EPSILON * scale.max(1.0));

    let candidate = |first_sign: f64, second_sign: f64| {
        let first_center = offset(first_point, &[(first_sign * radius, normals[0])]);
        let second_center = offset(second_point, &[(second_sign * radius, normals[1])]);
        let residual = point_displacement(second_center, first_center).norm();
        residual.is_finite().then(|| {
            (
                Point3::new(
                    first_center.x.midpoint(second_center.x),
                    first_center.y.midpoint(second_center.y),
                    first_center.z.midpoint(second_center.z),
                ),
                [first_sign, second_sign],
                residual,
            )
        })
    };
    let mut best: Option<(Point3, [f64; 2], f64)> = None;
    let mut second_best_residual = f64::INFINITY;
    for (first_sign, second_sign) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
        let Some(next) = candidate(first_sign, second_sign) else {
            continue;
        };
        match best {
            Some(current) if next.2 < current.2 => {
                second_best_residual = current.2;
                best = Some(next);
            }
            Some(_) => second_best_residual = second_best_residual.min(next.2),
            None => best = Some(next),
        }
    }
    let (center, signs, residual) = best.ok_or(no_value)?;
    if residual > tolerance || second_best_residual <= tolerance {
        return Err(no_value);
    }
    Ok(CircularVariableBlendSection {
        center,
        signs,
        first,
        second,
        normals,
        radius,
        radius_derivative,
        tolerance,
    })
}

fn cacheless_constant_rolling_ball_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    payload: &crate::geometry::surface_payloads::BlendSurfacePayload,
    u: f64,
    v: f64,
) -> Result<Point3, EvaluationFailure<()>> {
    let section = cacheless_constant_rolling_ball_section(admission, index, payload, u, v)?;
    minor_circular_arc_point(
        section.center,
        section.first.point(),
        section.second.point(),
        section.radius,
        u,
    )
}

struct ConstantRollingBallSection {
    center: Point3,
    center_tangent: Result<Vector3, EvaluationFailure<()>>,
    first: ContactTrack,
    second: ContactTrack,
    radius: f64,
}

/// The section of a constant rolling ball at `(u, v)`, or why there is none:
/// its spine point as the center of the two contact points. The section
/// reads the contact points and the spine point; the spine tangent states
/// its own outcome.
fn cacheless_constant_rolling_ball_section(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    payload: &crate::geometry::surface_payloads::BlendSurfacePayload,
    u: f64,
    v: f64,
) -> Result<ConstantRollingBallSection, EvaluationFailure<()>> {
    let no_value = EvaluationFailure::NoValue;
    let native = payload.native().ok_or(no_value)?;
    let native_ranges = payload.native_ranges().ok_or(no_value)?;
    let crate::geometry::BlendRadiusLaw::Constant { signed_radius } = payload.radius() else {
        return Err(no_value);
    };
    let signed_radius = signed_radius.get();
    let (finite_u, finite_v) = blend_parameters(u, v)?;
    if !matches!(
        native.cache,
        crate::geometry::RevisionCacheForm::Parameterization(_)
    ) || native.third.is_some()
        || *payload.cross_section() != crate::geometry::BlendCrossSection::Circular
        || !(0.0..=1.0).contains(&u)
        || !sweep_tail_interval_contains(native.slice_range, finite_v)
        || !sweep_tail_interval_contains(native_ranges[0].endpoints(), finite_u)
        || !sweep_tail_interval_contains(native_ranges[1].endpoints(), finite_v)
        || !native.cache.parameterization().is_some_and(|tail| {
            sweep_tail_interval_contains(tail.u_interval, finite_u)
                && sweep_tail_interval_contains(tail.v_interval, finite_v)
        })
    {
        return Err(no_value);
    }
    let radius = signed_radius.abs();
    if radius <= f64::EPSILON {
        return Err(no_value);
    }
    for (support, side) in payload.supports().iter().zip(native.sides.iter()) {
        if support.as_ref().is_some_and(|support| {
            side.surface
                .as_ref()
                .is_some_and(|surface| surface.surface != support.surface)
        }) {
            return Err(no_value);
        }
    }
    let first = variable_blend_contact_track(admission, index, &native.sides[0], v)?;
    let second = variable_blend_contact_track(admission, index, &native.sides[1], v)?;
    let center = model_curve_point_by_id(admission, index, &native.slice, v)
        .map_err(|failure| failure.map(|_| ()))?;
    let center_tangent = model_curve_differential_by_id(admission, index, &native.slice, v, ModelCurveRequest::Second)
        .map_err(|failure| failure.map(|_| ()))
        .and_then(|differential| differential.tangent)
        .map(FiniteVector3::get);
    if let Err(EvaluationFailure::ResourceLimit(limit)) = center_tangent {
        return Err(EvaluationFailure::ResourceLimit(limit));
    }
    let [first_point, second_point] = [first.point(), second.point()];
    let tolerance = index.ir().tolerances.linear.get().max(
        256.0
            * f64::EPSILON
            * radius
                .max(first_point.x.abs())
                .max(first_point.y.abs())
                .max(first_point.z.abs())
                .max(second_point.x.abs())
                .max(second_point.y.abs())
                .max(second_point.z.abs())
                .max(1.0),
    );
    let radius_error = |point: Point3| {
        (Vector3::new(point.x - center.x, point.y - center.y, point.z - center.z).norm() - radius)
            .abs()
    };
    if native
        .offsets
        .iter()
        .any(|offset| (offset.get() - signed_radius).abs() > tolerance)
        || radius_error(first_point) > tolerance
        || radius_error(second_point) > tolerance
    {
        return Err(no_value);
    }
    Ok(ConstantRollingBallSection {
        center: center.get(),
        center_tangent,
        first,
        second,
        radius,
    })
}

/// The point and first partials of a circular variable blend, the first
/// partials reading the radius derivative, the normal derivatives and the
/// tangents of both tracks, or why its point has none.
fn cacheless_circular_variable_blend_first_order(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    payload: &crate::geometry::surface_payloads::VariableBlendSurfacePayload,
    u: f64,
    v: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let unreached = |failure: EvaluationFailure<()>| failure.map(|()| UNREACHED_POINT);
    let tracks =
        circular_variable_blend_tracks(admission, index, payload, u, v).map_err(unreached)?;
    let section = cacheless_circular_variable_blend_section(
        admission,
        index,
        payload.construction(),
        v,
        tracks,
    )
    .map_err(unreached)?;
    let center_tangent = (|| {
        let radius_derivative = section.radius_derivative?;
        let center_tangent = |track: &ContactTrack,
                              sign: f64,
                              normal: Vector3|
         -> Result<Vector3, EvaluationFailure<()>> {
            Ok(vector_sum(&[
                (1.0, track.tangent()?),
                (sign * radius_derivative, normal),
                (sign * section.radius, track.normal_derivative?),
            ]))
        };
        let first_center_tangent =
            center_tangent(&section.first, section.signs[0], section.normals[0])?;
        let second_center_tangent =
            center_tangent(&section.second, section.signs[1], section.normals[1])?;
        if vector_sum(&[(1.0, second_center_tangent), (-1.0, first_center_tangent)]).norm()
            > section.tolerance
        {
            return Err(EvaluationFailure::NoValue);
        }
        Ok(scale_vector(
            vector_sum(&[(1.0, first_center_tangent), (1.0, second_center_tangent)]),
            0.5,
        ))
    })();
    circular_arc_first_order(
        section.center,
        center_tangent,
        &section.first,
        &section.second,
        section.radius,
        section.radius_derivative,
        u,
    )
}

fn cacheless_constant_rolling_ball_first_order(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    payload: &crate::geometry::surface_payloads::BlendSurfacePayload,
    u: f64,
    v: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let section = cacheless_constant_rolling_ball_section(admission, index, payload, u, v)
        .map_err(|failure| failure.map(|()| UNREACHED_POINT))?;
    constant_rolling_ball_first_order(&section, u)
}

fn constant_rolling_ball_first_order(
    section: &ConstantRollingBallSection,
    u: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    circular_arc_first_order(
        section.center,
        section.center_tangent,
        &section.first,
        &section.second,
        section.radius,
        Ok(0.0),
        u,
    )
}

/// The point at fraction `u` of the circular arc about `center` from the
/// first contact point to the second, with the arc's first partials, or why
/// its point has none. Contact points whose offsets from the center overflow
/// reach no coordinate; offsets that vanish or are parallel state no arc.
/// The first partials read the center tangent, the track tangents and the
/// radius derivative.
fn circular_arc_first_order(
    center: Point3,
    center_tangent: Result<Vector3, EvaluationFailure<()>>,
    first: &ContactTrack,
    second: &ContactTrack,
    radius: f64,
    radius_derivative: Result<f64, EvaluationFailure<()>>,
    u: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let unreached = |failure: EvaluationFailure<()>| failure.map(|()| UNREACHED_POINT);
    let delta_derivative = |track: &ContactTrack| {
        let center_tangent = center_tangent?;
        admit_derivative(vector_sum(&[
            (1.0, track.tangent()?),
            (-1.0, center_tangent),
        ]))
    };
    let radius_direction = |track: &ContactTrack| {
        let delta = admit_derivative(point_displacement(track.point(), center))?;
        unit_vector_with_derivative(delta, delta_derivative(track))
            .ok_or(EvaluationFailure::NoValue)
    };
    let (first_radius, first_radius_v) = radius_direction(first).map_err(unreached)?;
    let (second_radius, second_radius_v) = radius_direction(second).map_err(unreached)?;
    let first_unit = UnitVector3::new(first_radius).ok_or(EvaluationFailure::NoValue)?;
    let second_unit = UnitVector3::new(second_radius).ok_or(EvaluationFailure::NoValue)?;
    let cosine = UnitCosine::between(first_unit, second_unit).get();
    let cross = first_unit.finite_cross(second_unit);
    let sine = cross.get().norm();
    let axis = cross.unit_nonzero().ok_or(EvaluationFailure::NoValue)?;
    let angle = sine.atan2(cosine);
    let transverse = axis.cross(first_radius);
    let (section_sine, section_cosine) = (u * angle).sin_cos();
    let radial = vector_sum(&[(section_cosine, first_radius), (section_sine, transverse)]);
    let angular_direction =
        vector_sum(&[(-section_sine, first_radius), (section_cosine, transverse)]);
    let point = admit_point(offset(center, &[(radius, radial)]))?;
    let first_order = (|| {
        let first_radius_v = first_radius_v?;
        let second_radius_v = second_radius_v?;
        let center_tangent = center_tangent?;
        let radius_derivative = radius_derivative?;
        let cosine_v = first_radius_v.dot(second_radius) + first_radius.dot(second_radius_v);
        let cross_v = vector_sum(&[
            (1.0, first_radius_v.cross(second_radius)),
            (1.0, first_radius.cross(second_radius_v)),
        ]);
        let sine_v = axis.dot(cross_v);
        let angle_v = cosine * sine_v - sine * cosine_v;
        let axis_v = vector_sum(&[(1.0, cross_v), (-sine_v, axis)]).scale(1.0 / sine);
        let transverse_v = vector_sum(&[
            (1.0, axis_v.cross(first_radius)),
            (1.0, axis.cross(first_radius_v)),
        ]);
        let radial_v = vector_sum(&[
            (section_cosine, first_radius_v),
            (section_sine, transverse_v),
            (u * angle_v, angular_direction),
        ]);
        admit_lanes([
            angular_direction.scale(radius * angle),
            vector_sum(&[
                (1.0, center_tangent),
                (radius_derivative, radial),
                (radius, radial_v),
            ]),
        ])
    })();
    Ok(SurfaceFirstOrder {
        point,
        first: first_order,
    })
}

/// The point of a variable blend: the zero-radius rounded chamfer's, or the
/// circular section's.
fn cacheless_variable_blend_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    payload: &crate::geometry::surface_payloads::VariableBlendSurfacePayload,
    u: f64,
    v: f64,
) -> Result<Point3, EvaluationFailure<Point3>> {
    match cacheless_ruled_variable_blend_point(admission, index, payload, u, v) {
        Ok(point) => Ok(point),
        Err(EvaluationFailure::ResourceLimit(limit)) => {
            Err(EvaluationFailure::ResourceLimit(limit))
        }
        // The routes need different cross sections, so at most one reaches
        // past its structure: its failure outside the finite range is the
        // evaluation's.
        Err(ruled) => cacheless_circular_variable_blend_point(admission, index, payload, u, v)
            .map_err(|circular| match ruled {
                EvaluationFailure::NonFinite(()) => ruled,
                EvaluationFailure::NoValue => circular,
                EvaluationFailure::ResourceLimit(limit) => EvaluationFailure::ResourceLimit(limit),
            })
            .map_err(|failure| failure.map(|()| UNREACHED_POINT)),
    }
}

/// The point and first partials of a variable blend: the zero-radius rounded
/// chamfer's, or the circular section's.
fn cacheless_variable_blend_first_order(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    payload: &crate::geometry::surface_payloads::VariableBlendSurfacePayload,
    u: f64,
    v: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    match cacheless_ruled_variable_blend_first_order(admission, index, payload, u, v) {
        Ok(order) => Ok(order),
        Err(EvaluationFailure::ResourceLimit(limit)) => {
            Err(EvaluationFailure::ResourceLimit(limit))
        }
        // The routes need different cross sections, so at most one reaches
        // past its structure: its failure outside the finite range is the
        // evaluation's.
        Err(ruled) => cacheless_circular_variable_blend_first_order(
            admission, index, payload, u, v,
        )
        .map_err(|circular| match ruled {
            EvaluationFailure::NonFinite(_) => ruled,
            EvaluationFailure::NoValue => circular,
            EvaluationFailure::ResourceLimit(limit) => EvaluationFailure::ResourceLimit(limit),
        }),
    }
}

/// The point of a ruled surface. Curve points are evaluated in rail order;
/// the second is not reached after an original resource refusal on the first.
fn model_ruled_surface_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    first: &crate::ids::CurveId,
    second: &crate::ids::CurveId,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    if !v.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let first = model_curve_differential_by_id(admission, index, first, u, ModelCurveRequest::Point).map(|differential| differential.point);
    if let Err(EvaluationFailure::ResourceLimit(limit)) = first {
        return Err(EvaluationFailure::ResourceLimit(limit));
    }
    combine_curve_points(first, model_curve_differential_by_id(admission, index, second, u, ModelCurveRequest::Point).map(|differential| differential.point),
        |first, second| offset(first, &[(v, point_displacement(second, first))]))
}

/// The point of a sum surface, with the stored scalar addition order.
fn model_sum_surface_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::SumSurfaceConstruction,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let first = model_curve_differential_by_id(admission, index, construction.first(), u, ModelCurveRequest::Point).map(|differential| differential.point);
    if let Err(EvaluationFailure::ResourceLimit(limit)) = first {
        return Err(EvaluationFailure::ResourceLimit(limit));
    }
    let basepoint = *construction.basepoint();
    combine_curve_points(first, model_curve_differential_by_id(admission, index, construction.second(), v, ModelCurveRequest::Point).map(|differential| differential.point),
        |first, second| Point3::new(
            first.x + second.x - basepoint.x,
            first.y + second.y - basepoint.y,
            first.z + second.z - basepoint.z,
        ))
}

/// Combine two point outcomes. Absence wins over a reached non-finite point;
/// resource errors retain rail order, and only a finite pair is admitted anew.
fn combine_curve_points(
    first: Result<FinitePoint3, EvaluationFailure<Point3>>,
    second: Result<FinitePoint3, EvaluationFailure<Point3>>,
    combine: impl Fn(Point3, Point3) -> Point3,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let reached = |side: Result<FinitePoint3, EvaluationFailure<Point3>>| match side {
        Ok(point) => Ok(Some(point.get())),
        Err(failure) => failure.non_finite(),
    };
    match (first, second) {
        (Ok(first), Ok(second)) => admit_point(combine(first.get(), second.get())),
        (first, second) => Err(match (reached(first)?, reached(second)?) {
            (Some(first), Some(second)) => EvaluationFailure::NonFinite(combine(first, second)),
            _ => EvaluationFailure::NoValue,
        }),
    }
}

/// The jet of a ruled surface between two model curves, or why its point
/// has none. The point reads both curve points only; the first partials read
/// both tangents, and the second both accelerations as well.
fn model_ruled_surface_jet(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    first: &crate::ids::CurveId,
    second: &crate::ids::CurveId,
    u: f64,
    v: f64,
    request: SurfaceRequest,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    if !v.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let rule =
        |first: Point3, second: Point3| offset(first, &[(v, point_displacement(second, first))]);
    let first = model_curve_differential_by_id(admission, index, first, u, ModelCurveRequest::for_surface_partials(request));
    if let Err(EvaluationFailure::ResourceLimit(limit)) = first {
        return Err(EvaluationFailure::ResourceLimit(limit));
    }
    let (first, second) = curve_pair(
        first,
        model_curve_differential_by_id(admission, index, second, u, ModelCurveRequest::for_surface_partials(request)),
        rule,
    )?;
    let point = admit_point(rule(first.point.get(), second.point.get()))?;
    let blend = |first: Vector3, second: Vector3| vector_sum(&[(1.0 - v, first), (v, second)]);
    let tangents = first
        .tangent
        .and_then(|first| Ok([first.get(), second.tangent?.get()]));
    Ok(SurfaceJet {
        point,
        first: tangents.and_then(|[first_tangent, second_tangent]| {
            admit_lanes([
                blend(first_tangent, second_tangent),
                point_displacement(second.point.get(), first.point.get()),
            ])
        }),
        second: tangents.and_then(|[first_tangent, second_tangent]| {
            let [acceleration, twist] = admit_lanes([
                blend(first.acceleration?.get(), second.acceleration?.get()),
                vector_sum(&[(-1.0, first_tangent), (1.0, second_tangent)]),
            ])?;
            Ok([acceleration, twist, FiniteVector3::ZERO])
        }),
    })
}

/// The jet of a sum surface of two model curves, or why its point has none.
/// The point reads both curve points only; the first partials read both
/// tangents, and the second both accelerations.
fn model_sum_surface_jet(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::SumSurfaceConstruction,
    u: f64,
    v: f64,
    request: SurfaceRequest,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    let basepoint = *construction.basepoint();
    let sum = |first: Point3, second: Point3| {
        Point3::new(
            first.x + second.x - basepoint.x,
            first.y + second.y - basepoint.y,
            first.z + second.z - basepoint.z,
        )
    };
    let first = model_curve_differential_by_id(admission, index, construction.first(), u, ModelCurveRequest::for_surface_partials(request));
    if let Err(EvaluationFailure::ResourceLimit(limit)) = first {
        return Err(EvaluationFailure::ResourceLimit(limit));
    }
    let (first, second) = curve_pair(
        first,
        model_curve_differential_by_id(admission, index, construction.second(), v, ModelCurveRequest::for_surface_partials(request)),
        sum,
    )?;
    let point = admit_point(sum(first.point.get(), second.point.get()))?;
    Ok(SurfaceJet {
        point,
        first: first.tangent.and_then(|first| Ok([first, second.tangent?])),
        second: first
            .acceleration
            .and_then(|first| Ok([first, FiniteVector3::ZERO, second.acceleration?])),
    })
}

/// The two curve differentials a surface arm combines. A curve with no value
/// leaves the surface without one; otherwise a curve outside the finite
/// range leaves the surface there, at the point `combine` reaches from the
/// two curve points.
fn curve_pair(
    first: Result<ModelCurveDifferential, EvaluationFailure<Point3>>,
    second: Result<ModelCurveDifferential, EvaluationFailure<Point3>>,
    combine: impl Fn(Point3, Point3) -> Point3,
) -> Result<(ModelCurveDifferential, ModelCurveDifferential), EvaluationFailure<Point3>> {
    let reached = |side: Result<ModelCurveDifferential, EvaluationFailure<Point3>>| match side {
        Ok(differential) => Ok(Some(differential.point.get())),
        Err(failure) => failure.non_finite(),
    };
    match (first, second) {
        (Ok(first), Ok(second)) => Ok((first, second)),
        (first, second) => Err(match (reached(first)?, reached(second)?) {
            (Some(first), Some(second)) => EvaluationFailure::NonFinite(combine(first, second)),
            _ => EvaluationFailure::NoValue,
        }),
    }
}

/// Evaluate a surface carrier selected by arena id.
pub fn model_surface_point_by_id(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    admission.within_model(|admission| {
        model_surface_point::model_surface_point_by_id_inner(admission, index, surface, u, v)
    })
}

/// Evaluate an arena-selected direct, trimmed, or uniform-offset surface and
/// its exact first partial derivatives, or report why they have no finite
/// value.
///
/// Subsets map the support parameterization through a linear local domain;
/// offsets follow the support's oriented normal. The recursive carrier walk
/// preserves both contracts before evaluating the final point and partials.
/// The point fails as [`model_surface_point_by_id`]'s arms state; at a finite
/// point, first partials without a value, or outside the finite range, fail
/// the evaluation, carrying the point.
pub fn model_surface_partials_by_id(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Result<SurfacePartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    admission.within_model(|admission| {
        model_surface_first_order_by_id(admission, index, surface, u, v)
            .and_then(SurfaceFirstOrder::partials)
    })
}

/// The point and first partials of an arena surface, the first partials
/// with their own outcome, or why the point has none.
///
/// A cacheless blend or sweep whose point and first partials both have
/// values is evaluated with the selected admission policy.
/// Otherwise one with a current cache falls back to the cache: the cache's
/// complete evaluation wins, then an evaluation with a point, the cacheless
/// one first; of two failures, the cacheless one outside the finite range
/// wins.
fn model_surface_first_order_by_id(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let budget = admission.work_slice();
    let _depth =
        ModelEvaluationDepthGuard::enter(budget).map_err(EvaluationFailure::ResourceLimit)?;
    let cacheless = match index
        .procedural_surface_for_surface(surface.as_str(), admission)
        .map_err(EvaluationFailure::ResourceLimit)?
        .map(crate::geometry::ProceduralSurface::definition)
    {
        Some(ProceduralSurfaceDefinition::Blend(definition_payload)) => {
            definition_payload.native().map(|native| {
                (
                    cacheless_constant_rolling_ball_first_order(
                        admission,
                        index,
                        definition_payload,
                        u,
                        v,
                    ),
                    revision_surface_tail_has_current_cache(&native.cache),
                )
            })
        }
        Some(ProceduralSurfaceDefinition::VariableBlend(definition_payload)) => {
            let construction = definition_payload.construction();
            Some((
                cacheless_variable_blend_first_order(admission, index, definition_payload, u, v),
                variable_blend_has_current_cache(construction),
            ))
        }
        Some(ProceduralSurfaceDefinition::Sweep(definition_payload)) => {
            definition_payload.native().as_deref().map(|construction| {
                (
                    cacheless_law_sweep_first_order(
                        admission,
                        index,
                        definition_payload.profile(),
                        definition_payload.spine(),
                        construction,
                        u,
                        v,
                    ),
                    sweep_has_current_cache(construction),
                )
            })
        }
        _ => None,
    };
    let cached =
        || surface_request::model_jet(admission, index, surface, u, v, SurfaceRequest::First).map(SurfaceJet::first_order);
    let complete = |order: &Result<SurfaceFirstOrder, EvaluationFailure<Point3>>| match order {
        Ok(order) => match &order.first {
            Ok(_) => Ok(true),
            Err(EvaluationFailure::ResourceLimit(limit)) => {
                Err(EvaluationFailure::ResourceLimit(*limit))
            }
            Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(())) => Ok(false),
        },
        Err(EvaluationFailure::ResourceLimit(limit)) => {
            Err(EvaluationFailure::ResourceLimit(*limit))
        }
        Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(_)) => Ok(false),
    };
    let Some((cacheless, has_current_cache)) = cacheless else {
        return cached();
    };
    if complete(&cacheless)? || !has_current_cache {
        return cacheless;
    }
    let cached = cached();
    if complete(&cached)? {
        return cached;
    }
    match (cacheless, cached) {
        (Ok(order), _) | (Err(_), Ok(order)) => Ok(order),
        (Err(cacheless), Err(cached)) => Err(match cacheless {
            EvaluationFailure::NonFinite(_) => cacheless,
            EvaluationFailure::NoValue => cached,
            EvaluationFailure::ResourceLimit(limit) => EvaluationFailure::ResourceLimit(limit),
        }),
    }
}

fn model_surface_second_partials_by_id(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    admission.within_model(|admission| {
        surface_request::model_jet(admission, index, surface, u, v, SurfaceRequest::Second)
            .and_then(SurfaceJet::second_partials)
    })
}

fn subset_support_parameters_with_derivatives(
    u: f64,
    v: f64,
    parameter_ranges: [[FiniteReal; 2]; 2],
    u_sense: Option<bool>,
    v_sense: Option<bool>,
) -> Option<(f64, f64, f64, f64)> {
    let (support_u, u_derivative) = subset_parameter(parameter_ranges[0], u, u_sense)?;
    let (support_v, v_derivative) = subset_parameter(parameter_ranges[1], v, v_sense)?;
    Some((support_u, support_v, u_derivative, v_derivative))
}

/// Map a subset parameter onto its support: the support parameter and the
/// derivative sign. The range endpoints are finite and distinct, so the span
/// is not zero. A sense against the range direction runs the support
/// parameter away from the range, where it can leave the finite range; the
/// support evaluation then reports that evaluation.
fn subset_parameter(
    range: [FiniteReal; 2],
    parameter: f64,
    sense: Option<bool>,
) -> Option<(f64, f64)> {
    let [start, end] = range.map(FiniteReal::get);
    let span = (end - start).abs();
    if !parameter.is_finite() || parameter < 0.0 || parameter > span {
        return None;
    }
    let agrees = sense.unwrap_or(end >= start);
    let derivative = if agrees { 1.0 } else { -1.0 };
    Some((start + derivative * parameter, derivative))
}

/// `(end - start) / (domain_end - domain_start)` over exact differences. An
/// empty domain has no quotient; a quotient that overflows is non-finite and
/// carries its signed infinity.
fn difference_quotient(
    end: FiniteReal,
    start: FiniteReal,
    domain_end: FiniteReal,
    domain_start: FiniteReal,
) -> Result<FiniteReal, EvaluationFailure<f64>> {
    let mut denominator = ExactSignedSum::default();
    denominator.add_product(domain_end.get(), 1.0);
    denominator.add_product(domain_start.get(), -1.0);
    let denominator = denominator.finish().ok_or(EvaluationFailure::NoValue)?;
    let mut numerator = ExactSignedSum::default();
    numerator.add_product(end.get(), 1.0);
    numerator.add_product(start.get(), -1.0);
    numerator.finish().map_or(Ok(FiniteReal::ZERO), |value| {
        value
            .quotient(denominator)
            .map_err(EvaluationFailure::NonFinite)
    })
}

fn scale_vector(vector: Vector3, factor: f64) -> Vector3 {
    Vector3::new(vector.x * factor, vector.y * factor, vector.z * factor)
}

fn vector_sum(terms: &[(f64, Vector3)]) -> Vector3 {
    let point = offset(Point3::new(0.0, 0.0, 0.0), terms);
    Vector3::new(point.x, point.y, point.z)
}

/// Evaluate the exact first derivative of a directly stored pcurve.
///
/// The derivative is finite only where the carrier's evaluation is: an
/// evaluation that leaves the finite range reports
/// [`EvaluationFailure::NonFinite`] with the derivative it reached, finite
/// or not. A parameter that is not finite and a structure that states no
/// derivative report [`EvaluationFailure::NoValue`].
pub fn pcurve_tangent<'ctx, 'arena: 'ctx>(
    admission: impl Into<admission::EvaluationAdmission<'ctx, 'arena>>,
    geometry: &PcurveGeometry,
    t: f64,
) -> Result<FinitePoint2, EvaluationFailure<Point2>> {
    let scratch = decode::Scratch::new(admission);
    let result = (|| {
        let t = FiniteReal::new(t).ok_or(EvaluationFailure::NoValue)?;
        let evaluated =
            pcurve_uv_differential(&scratch, geometry, t).ok_or(EvaluationFailure::NoValue)?;
        if let Some(limit) = evaluated.resource {
            return Err(EvaluationFailure::ResourceLimit(limit));
        }
        evaluated.tangent
    })();
    scratch.settle(result)
}

/// The angle of a polar chart point and its first two derivatives, without
/// squaring coordinates in the finite f64 range. A chart point at the origin
/// has no angle.
///
/// The first derivative is absent without a chart tangent. A chart tangent
/// that left the finite range, and a quotient that overflows, make it
/// non-finite; a derivative that no step reached is NaN.
fn polar_angle_differential(
    point: FinitePoint2,
    tangent: Result<FinitePoint2, EvaluationFailure<Point2>>,
    acceleration: Option<FinitePoint2>,
) -> Result<Option<PolarAngle>, ResourceLimit> {
    let mut radius = ExactSignedSum::default();
    radius.add_product(point.u, point.u);
    radius.add_product(point.v, point.v);
    let Some(radius) = radius.finish() else {
        return Ok(None);
    };
    let first = match tangent {
        Ok(tangent) => {
            let mut cross = ExactSignedSum::default();
            cross.add_product(point.u, tangent.v);
            cross.add_product(-point.v, tangent.u);
            cross.finish().map_or(Ok(FiniteReal::ZERO), |cross| {
                cross.quotient(radius).map_err(EvaluationFailure::NonFinite)
            })
        }
        Err(EvaluationFailure::NonFinite(_)) => Err(EvaluationFailure::NonFinite(f64::NAN)),
        Err(EvaluationFailure::NoValue) => Err(EvaluationFailure::NoValue),
        Err(EvaluationFailure::ResourceLimit(limit)) => {
            Err(EvaluationFailure::ResourceLimit(limit))
        }
    };
    let second = finite_or_refusal(tangent)?
        .zip(acceleration)
        .zip(finite_or_refusal(first)?)
        .and_then(|((tangent, acceleration), first)| {
            let mut dot = ExactSignedSum::default();
            dot.add_product(point.u, tangent.u);
            dot.add_product(point.v, tangent.v);
            let mut numerator = ExactSignedSum::default();
            numerator.add_product(point.u, acceleration.v);
            numerator.add_product(-point.v, acceleration.u);
            numerator.add_scaled_product(dot.finish(), first.negated())?;
            numerator.add_scaled_product(dot.finish(), first.negated())?;
            numerator
                .finish()
                .map_or(Some(FiniteReal::ZERO), |value| value.quotient(radius).ok())
        });
    let [u, v] = point.coordinates();
    Ok(Some(PolarAngle {
        angle: v.atan2(u),
        first,
        second,
    }))
}

/// The angle of a polar chart point and its first two derivatives.
struct PolarAngle {
    angle: FiniteReal,
    /// The first derivative, or why it has no finite value.
    first: Result<FiniteReal, EvaluationFailure<f64>>,
    /// The second derivative, where it is finite.
    second: Option<FiniteReal>,
}

/// A computed second derivative or a carrier that states none.
#[derive(Clone, Copy)]
enum PcurveAcceleration {
    Finite(FinitePoint2),
    NonFinite,
    Unstated,
}

impl From<Option<FinitePoint2>> for PcurveAcceleration {
    fn from(value: Option<FinitePoint2>) -> Self {
        value.map_or(Self::NonFinite, Self::Finite)
    }
}

impl PcurveAcceleration {
    fn finite(self) -> Option<FinitePoint2> {
        match self {
            Self::Finite(value) => Some(value),
            Self::NonFinite | Self::Unstated => None,
        }
    }
}

/// A pcurve carrier's point and first two derivatives at a parameter.
struct PcurveEvaluation {
    /// The point, or the point an evaluation that left the finite range
    /// reached.
    point: Result<FinitePoint2, Point2>,
    /// The first derivative, or why it has no finite value.
    tangent: Result<FinitePoint2, EvaluationFailure<Point2>>,
    /// The stated finite/non-finite derivative, or an unstated derivative.
    acceleration: PcurveAcceleration,
    /// Refusal of scratch needed by the evaluated carrier.
    resource: Option<ResourceLimit>,
}

impl PcurveEvaluation {
    /// A carrier whose point and derivatives are evaluated together: the
    /// point is finite only where the whole evaluation is. An evaluation that
    /// left the finite range carries the point and tangent it reached and no
    /// acceleration.
    fn evaluated(
        point: Point2,
        tangent: Result<FinitePoint2, EvaluationFailure<Point2>>,
        acceleration: PcurveAcceleration,
    ) -> Self {
        if let Err(EvaluationFailure::ResourceLimit(limit)) = tangent {
            return Self::resource(limit);
        }
        match FinitePoint2::new(point) {
            Some(point) => Self {
                point: Ok(point),
                tangent,
                acceleration,
                resource: None,
            },
            None => match reached_value(tangent) {
                Ok(tangent) => Self::left_finite_range(point, tangent),
                Err(limit) => Self::resource(limit),
            },
        }
    }

    /// An evaluation that left the finite range, with the point and tangent
    /// it reached. Either can be finite: the evaluation also leaves the range
    /// where the other, or a quantity formed with them, does.
    fn left_finite_range(point: Point2, tangent: Point2) -> Self {
        Self {
            point: Err(point),
            tangent: Err(EvaluationFailure::NonFinite(tangent)),
            acceleration: PcurveAcceleration::NonFinite,
            resource: None,
        }
    }

    fn resource(limit: ResourceLimit) -> Self {
        Self {
            point: Err(Point2::new(f64::NAN, f64::NAN)),
            tangent: Err(EvaluationFailure::ResourceLimit(limit)),
            acceleration: PcurveAcceleration::NonFinite,
            resource: Some(limit),
        }
    }
}

impl From<PcurveDifferential> for PcurveEvaluation {
    fn from(differential: PcurveDifferential) -> Self {
        if let Err(EvaluationFailure::ResourceLimit(limit)) = differential.tangent {
            return Self::resource(limit);
        }
        Self {
            point: Ok(differential.point),
            tangent: differential.tangent,
            acceleration: differential.acceleration.into(),
            resource: None,
        }
    }
}

/// The value an evaluation reached: the finite value, the non-finite value,
/// or NaN in each coordinate where no step reached one.
fn reached_value(
    value: Result<FinitePoint2, EvaluationFailure<Point2>>,
) -> Result<Point2, ResourceLimit> {
    match value {
        Ok(value) => Ok(value.get()),
        Err(EvaluationFailure::NonFinite(value)) => Ok(value),
        Err(EvaluationFailure::NoValue) => Ok(Point2::new(f64::NAN, f64::NAN)),
        Err(EvaluationFailure::ResourceLimit(limit)) => Err(limit),
    }
}

/// Evaluate a pcurve carrier's point and its first two derivatives at `t`.
/// `None` states that the carrier has no value at `t`.
///
/// Each carrier frame enters the scratch admission's recursion policy.
/// Nested placed, trimmed and offset carriers retain the same policy and
/// preserve the original session refusal.
fn pcurve_uv_differential(
    scratch: &decode::Scratch<'_, '_>,
    geometry: &PcurveGeometry,
    parameter: FiniteReal,
) -> Option<PcurveEvaluation> {
    let evaluated = pcurve_uv_unsettled(scratch, geometry, parameter);
    match scratch.refused() {
        Some(limit) => Some(PcurveEvaluation::resource(limit)),
        None => evaluated,
    }
}

fn pcurve_uv_unsettled(
    scratch: &decode::Scratch<'_, '_>,
    geometry: &PcurveGeometry,
    parameter: FiniteReal,
) -> Option<PcurveEvaluation> {
    let _depth = scratch.enter()?;
    let t = parameter.get();
    let pair = match geometry {
        PcurveGeometry::Line(line_pcurve) => {
            let origin = line_pcurve.origin().as_raw();
            let direction = line_pcurve.direction().as_raw();
            (
                Point2::new(origin.u + t * direction.u, origin.v + t * direction.v),
                *direction,
                Point2::new(0.0, 0.0),
            )
        }
        PcurveGeometry::Circle(circle_pcurve) => {
            let center = circle_pcurve.center();
            let x_axis = circle_pcurve.x_axis();
            let y_axis = circle_pcurve.y_axis();
            let radius = circle_pcurve.radius();
            let cosine = t.cos();
            let sine = t.sin();
            (
                offset2(
                    center.get(),
                    &[
                        (radius.get() * cosine, x_axis.get()),
                        (radius.get() * sine, y_axis.get()),
                    ],
                ),
                Point2::new(
                    radius.get() * (-sine * x_axis.u + cosine * y_axis.u),
                    radius.get() * (-sine * x_axis.v + cosine * y_axis.v),
                ),
                Point2::new(
                    -radius.get() * (cosine * x_axis.u + sine * y_axis.u),
                    -radius.get() * (cosine * x_axis.v + sine * y_axis.v),
                ),
            )
        }
        PcurveGeometry::Ellipse(ellipse_pcurve) => {
            let center = ellipse_pcurve.center();
            let x_axis = ellipse_pcurve.x_axis();
            let y_axis = ellipse_pcurve.y_axis();
            let major_radius = ellipse_pcurve.major_radius();
            let minor_radius = ellipse_pcurve.minor_radius();
            let cosine = t.cos();
            let sine = t.sin();
            (
                offset2(
                    center.get(),
                    &[
                        (major_radius.get() * cosine, x_axis.get()),
                        (minor_radius.get() * sine, y_axis.get()),
                    ],
                ),
                Point2::new(
                    -major_radius.get() * sine * x_axis.u + minor_radius.get() * cosine * y_axis.u,
                    -major_radius.get() * sine * x_axis.v + minor_radius.get() * cosine * y_axis.v,
                ),
                Point2::new(
                    -major_radius.get() * cosine * x_axis.u - minor_radius.get() * sine * y_axis.u,
                    -major_radius.get() * cosine * x_axis.v - minor_radius.get() * sine * y_axis.v,
                ),
            )
        }
        PcurveGeometry::Harmonic(harmonic_pcurve) => {
            let center = harmonic_pcurve.center();
            let cosine = harmonic_pcurve.cosine();
            let sine = harmonic_pcurve.sine();
            let cosine_parameter = t.cos();
            let sine_parameter = t.sin();
            (
                offset2(
                    center.get(),
                    &[
                        (cosine_parameter, cosine.get()),
                        (sine_parameter, sine.get()),
                    ],
                ),
                Point2::new(
                    -sine_parameter * cosine.u + cosine_parameter * sine.u,
                    -sine_parameter * cosine.v + cosine_parameter * sine.v,
                ),
                Point2::new(
                    -cosine_parameter * cosine.u - sine_parameter * sine.u,
                    -cosine_parameter * cosine.v - sine_parameter * sine.v,
                ),
            )
        }
        PcurveGeometry::Parabola(parabola) => {
            let vertex = parabola.vertex();
            let x = parabola.x_axis();
            let y = parabola.y_axis();
            let focal = parabola.focal_distance();
            // `t * t` and `4 * focal` are not negative, so an axial quotient
            // that overflows reaches positive infinity.
            let axial =
                product_quotient([t, t], [4.0, focal.get()]).map_or(f64::INFINITY, FiniteReal::get);
            let derivative = |axis| {
                product_quotient([t, axis], [2.0, focal.get()]).map_or(f64::NAN, FiniteReal::get)
            };
            let second = |axis| {
                product_quotient([axis], [2.0, focal.get()]).map_or(f64::NAN, FiniteReal::get)
            };
            (
                offset2(vertex.get(), &[(axial, x.get()), (t, y.get())]),
                Point2::new(derivative(x.u) + y.u, derivative(x.v) + y.v),
                Point2::new(second(x.u), second(x.v)),
            )
        }
        PcurveGeometry::Hyperbola(hyperbola) => {
            let x = hyperbola.x_axis();
            let y = hyperbola.y_axis();
            let reached = |pair: Result<(FiniteReal, FiniteReal), (f64, f64)>| {
                pair.map_or_else(|plain| plain, |(sinh, cosh)| (sinh.get(), cosh.get()))
            };
            let (major_sinh, major_cosh) =
                reached(scaled_sinh_cosh(hyperbola.major_radius().into(), parameter));
            let (minor_sinh, minor_cosh) =
                reached(scaled_sinh_cosh(hyperbola.minor_radius().into(), parameter));
            let zero = Point2::new(0.0, 0.0);
            let point = offset2(
                hyperbola.center().get(),
                &[(major_cosh, x.get()), (minor_sinh, y.get())],
            );
            let tangent = offset2(zero, &[(major_sinh, x.get()), (minor_cosh, y.get())]);
            let acceleration = offset2(zero, &[(major_cosh, x.get()), (minor_sinh, y.get())]);
            return Some(PcurveEvaluation::evaluated(
                point,
                FinitePoint2::new(tangent).ok_or(EvaluationFailure::NonFinite(tangent)),
                FinitePoint2::new(acceleration).into(),
            ));
        }
        PcurveGeometry::Hyperbolic(hyperbolic) => {
            let [cosine_u, cosine_v] = hyperbolic.cosine().coordinates();
            let [sine_u, sine_v] = hyperbolic.sine().coordinates();
            let [center_u, center_v] = hyperbolic.center().coordinates();
            // Each coordinate and its two derivatives read their own lanes.
            let coordinate = |center: FiniteReal, cosine: FiniteReal, sine: FiniteReal| {
                let cosine = scaled_sinh_cosh(cosine, parameter);
                let sine = scaled_sinh_cosh(sine, parameter);
                let reached = |pair: Result<(FiniteReal, FiniteReal), (f64, f64)>| {
                    pair.map_or_else(|plain| plain, |(sinh, cosh)| (sinh.get(), cosh.get()))
                };
                let (cosine_sinh, cosine_cosh) = reached(cosine);
                let (sine_sinh, sine_cosh) = reached(sine);
                (
                    crate::math::sum::finite_dot([1.0; 3], [center.get(), cosine_cosh, sine_sinh]),
                    crate::math::sum::finite_dot([1.0; 2], [cosine_sinh, sine_cosh]),
                    crate::math::sum::finite_dot([1.0; 2], [cosine_cosh, sine_sinh]),
                )
            };
            let u = coordinate(center_u, cosine_u, sine_u);
            let v = coordinate(center_v, cosine_v, sine_v);
            let reached =
                |value: Result<FiniteReal, f64>| value.map_or_else(|plain| plain, FiniteReal::get);
            let lane = |value: Result<FiniteReal, f64>| value.map_err(EvaluationFailure::NonFinite);
            let (Ok(point_u), Ok(point_v)) = (u.0, v.0) else {
                return Some(PcurveEvaluation::left_finite_range(
                    Point2::new(reached(u.0), reached(v.0)),
                    Point2::new(reached(u.1), reached(v.1)),
                ));
            };
            return Some(PcurveEvaluation {
                point: Ok(FinitePoint2::from_coordinates(point_u, point_v)),
                tangent: planar_value(lane(u.1), lane(v.1)),
                acceleration: u
                    .2
                    .ok()
                    .zip(v.2.ok())
                    .map(|(u, v)| FinitePoint2::from_coordinates(u, v)).into(),
                resource: None,
            });
        }
        PcurveGeometry::PolarHarmonic(polar_harmonic_pcurve) => {
            let radial_center = polar_harmonic_pcurve.radial_center();
            let radial_cos = polar_harmonic_pcurve.radial_cos();
            let radial_sin = polar_harmonic_pcurve.radial_sin();
            let axial_origin = polar_harmonic_pcurve.axial_origin().get();
            let axial_cos = polar_harmonic_pcurve.axial_cos().get();
            let axial_sin = polar_harmonic_pcurve.axial_sin().get();
            let cosine = t.cos();
            let sine = t.sin();
            let x = radial_center.u + radial_cos.u * cosine + radial_sin.u * sine;
            let y = radial_center.v + radial_cos.v * cosine + radial_sin.v * sine;
            let dx = -radial_cos.u * sine + radial_sin.u * cosine;
            let dy = -radial_cos.v * sine + radial_sin.v * cosine;
            let ddx = -radial_cos.u * cosine - radial_sin.u * sine;
            let ddy = -radial_cos.v * cosine - radial_sin.v * sine;
            let axial = axial_origin + axial_cos * cosine + axial_sin * sine;
            let axial_derivative = -axial_cos * sine + axial_sin * cosine;
            let Some(radial) = FinitePoint2::new(Point2::new(x, y)) else {
                // The angle of a radial point outside the finite range is
                // not reached.
                return Some(PcurveEvaluation::left_finite_range(
                    Point2::new(f64::NAN, axial),
                    Point2::new(f64::NAN, axial_derivative),
                ));
            };
            let angle = match polar_angle_differential(
                radial,
                admit_parameter_point(Point2::new(dx, dy)),
                FinitePoint2::new(Point2::new(ddx, ddy)),
            ) {
                Ok(Some(angle)) => angle,
                Ok(None) => return None,
                Err(limit) => return Some(PcurveEvaluation::resource(limit)),
            };
            let PolarAngle {
                angle,
                first,
                second,
            } = angle;
            let Some(finite_axial) = FiniteReal::new(axial) else {
                let first = match first {
                    Ok(value) => value.get(),
                    Err(EvaluationFailure::NonFinite(value)) => value,
                    Err(EvaluationFailure::NoValue) => f64::NAN,
                    Err(EvaluationFailure::ResourceLimit(limit)) => {
                        return Some(PcurveEvaluation::resource(limit))
                    }
                };
                return Some(PcurveEvaluation::left_finite_range(
                    Point2::new(angle.get(), axial),
                    Point2::new(first, axial_derivative),
                ));
            };
            let axial_lane =
                |value: f64| FiniteReal::new(value).ok_or(EvaluationFailure::NonFinite(value));
            return Some(PcurveEvaluation {
                point: Ok(FinitePoint2::from_coordinates(angle, finite_axial)),
                tangent: planar_value(first, axial_lane(axial_derivative)),
                acceleration: second.and_then(|second| {
                    let axial = FiniteReal::new(-axial_cos * cosine - axial_sin * sine)?;
                    Some(FinitePoint2::from_coordinates(second, axial))
                }).into(),
                resource: None,
            });
        }
        PcurveGeometry::PolarNurbs { nurbs } => {
            let poles = nurbs.pole_rows();
            let weights = match poles {
                crate::geometry::pcurve::PolarNurbsPoles::Polynomial { .. } => None,
                crate::geometry::pcurve::PolarNurbsPoles::Rational { poles } => {
                    Some(scratch.collect(
                        poles.iter().map(|pole| Some(pole.weight.get())),
                        "IR polar NURBS weights",
                        "IR polar NURBS weights work",
                    )?)
                }
            };
            let radial_at = |index: usize| match poles {
                crate::geometry::pcurve::PolarNurbsPoles::Polynomial { poles } => {
                    poles.get(index).map(|pole| pole.radial)
                }
                crate::geometry::pcurve::PolarNurbsPoles::Rational { poles } => {
                    poles.get(index).map(|pole| pole.radial)
                }
            };
            let axial_at = |index: usize| match poles {
                crate::geometry::pcurve::PolarNurbsPoles::Polynomial { poles } => {
                    poles.get(index).map(|pole| pole.axial)
                }
                crate::geometry::pcurve::PolarNurbsPoles::Rational { poles } => {
                    poles.get(index).map(|pole| pole.axial)
                }
            };
            let radial = nurbs_pcurve_differential_with(
                scratch,
                nurbs.degree(),
                nurbs.knots(),
                poles.count(),
                |index| radial_at(index).map(planar_pole),
                weights.as_deref(),
                parameter,
            );
            let axial = nurbs_pcurve_differential_with(
                scratch,
                nurbs.degree(),
                nurbs.knots(),
                poles.count(),
                |index| {
                    let axial = axial_at(index)?;
                    Some(FinitePoint3::from_coordinates(
                        axial,
                        FiniteReal::ZERO,
                        FiniteReal::ZERO,
                    ))
                },
                weights.as_deref(),
                parameter,
            );
            if let Err(EvaluationFailure::ResourceLimit(limit)) = &radial {
                return Some(PcurveEvaluation::resource(*limit));
            }
            if let Err(EvaluationFailure::ResourceLimit(limit)) = &axial {
                return Some(PcurveEvaluation::resource(*limit));
            }
            let (radial, axial) = match (radial, axial) {
                (Ok(radial), Ok(axial)) => (radial, axial),
                (Err(EvaluationFailure::NoValue), _) | (_, Err(EvaluationFailure::NoValue)) => {
                    return None;
                }
                // The angle of a radial point outside the finite range is
                // not reached, and the derivatives of a spline point outside
                // it are not formed.
                (radial, axial) => {
                    let axial = match axial {
                        Ok(axial) => axial.point.get().u,
                        Err(EvaluationFailure::NonFinite(axial)) => axial.u,
                        Err(EvaluationFailure::NoValue) => f64::NAN,
                        Err(EvaluationFailure::ResourceLimit(limit)) => {
                            return Some(PcurveEvaluation::resource(limit))
                        }
                    };
                    // A radial point at the origin has no angle.
                    let angle = match radial {
                        Ok(radial) => {
                            match polar_angle_differential(radial.point, radial.tangent, None) {
                                Ok(Some(angle)) => angle.angle.get(),
                                Ok(None) => return None,
                                Err(limit) => return Some(PcurveEvaluation::resource(limit)),
                            }
                        }
                        Err(_) => f64::NAN,
                    };
                    return Some(PcurveEvaluation::left_finite_range(
                        Point2::new(angle, axial),
                        Point2::new(f64::NAN, f64::NAN),
                    ));
                }
            };
            let angle =
                match polar_angle_differential(radial.point, radial.tangent, radial.acceleration) {
                    Ok(Some(angle)) => angle,
                    Ok(None) => return None,
                    Err(limit) => return Some(PcurveEvaluation::resource(limit)),
                };
            let PolarAngle {
                angle,
                first,
                second,
            } = angle;
            let axial_lane = |value: Result<FinitePoint2, EvaluationFailure<Point2>>| match value {
                Ok(value) => Ok(value.coordinates()[0]),
                Err(failure) => Err(match failure {
                    EvaluationFailure::NoValue => EvaluationFailure::NoValue,
                    EvaluationFailure::NonFinite(value) => EvaluationFailure::NonFinite(value.u),
                    EvaluationFailure::ResourceLimit(limit) => {
                        EvaluationFailure::ResourceLimit(limit)
                    }
                }),
            };
            return Some(PcurveEvaluation {
                point: Ok(FinitePoint2::from_coordinates(
                    angle,
                    axial.point.coordinates()[0],
                )),
                tangent: planar_value(first, axial_lane(axial.tangent)),
                acceleration: second.zip(axial.acceleration).map(|(second, axial)| {
                    FinitePoint2::from_coordinates(second, axial.coordinates()[0])
                }).into(),
                resource: None,
            });
        }
        PcurveGeometry::SphericalGreatCircle(spherical_great_circle_pcurve) => {
            let azimuth_origin = spherical_great_circle_pcurve.azimuth_origin().get();
            let admitted_rate = spherical_great_circle_pcurve.azimuth_rate();
            let azimuth_rate = admitted_rate.get();
            let plane_phase = spherical_great_circle_pcurve.plane_phase().get();
            let plane_slope = spherical_great_circle_pcurve.plane_slope().get();
            let raw_azimuth = azimuth_origin + azimuth_rate * t;
            let Some(azimuth) = FiniteReal::new(raw_azimuth) else {
                // The latitude of an azimuth outside the finite range is not
                // reached.
                return Some(PcurveEvaluation::left_finite_range(
                    Point2::new(raw_azimuth, f64::NAN),
                    Point2::new(azimuth_rate, f64::NAN),
                ));
            };
            let phase = azimuth.get() - plane_phase;
            let cosine = phase.cos();
            let sine = phase.sin();
            let scale = plane_slope.abs().max(1.0);
            let slope = plane_slope / scale;
            // The chart and its derivatives are finite exactly when the phase
            // is: a phase outside the finite range reaches no latitude.
            let Some([chart_u, chart_v, tangent_v, acceleration_v]) =
                FiniteReal::array([1.0 / scale, slope * cosine, -slope * sine, -slope * cosine])
            else {
                return Some(PcurveEvaluation::left_finite_range(
                    Point2::new(azimuth.get(), f64::NAN),
                    Point2::new(azimuth_rate, f64::NAN),
                ));
            };
            let angle = match polar_angle_differential(
                FinitePoint2::from_coordinates(chart_u, chart_v),
                Ok(FinitePoint2::from_coordinates(FiniteReal::ZERO, tangent_v)),
                Some(FinitePoint2::from_coordinates(
                    FiniteReal::ZERO,
                    acceleration_v,
                )),
            ) {
                Ok(Some(angle)) => angle,
                Ok(None) => return None,
                Err(limit) => return Some(PcurveEvaluation::resource(limit)),
            };
            let PolarAngle {
                angle: latitude,
                first,
                second,
            } = angle;
            let latitude_rate = first.and_then(|first| {
                let mut sum = ExactSignedSum::default();
                sum.add_product(first.get(), azimuth_rate);
                sum.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                    value.finite().map_err(EvaluationFailure::NonFinite)
                })
            });
            let acceleration = second
                .and_then(|second| {
                    let mut sum = ExactSignedSum::default();
                    sum.add_factors([second.get(), azimuth_rate, azimuth_rate]);
                    sum.finish()
                        .map_or(Some(FiniteReal::ZERO), |value| value.finite().ok())
                })
                .map(|value| FinitePoint2::from_coordinates(FiniteReal::ZERO, value));
            return Some(PcurveEvaluation {
                point: Ok(FinitePoint2::from_coordinates(azimuth, latitude)),
                tangent: planar_value(Ok(admitted_rate), latitude_rate),
                acceleration: acceleration.into(),
                resource: None,
            });
        }
        PcurveGeometry::Nurbs { nurbs } => {
            let poles = nurbs.pole_rows();
            let weights = match poles {
                crate::geometry::pcurve::PcurveNurbsPoles::Polynomial { .. } => None,
                crate::geometry::pcurve::PcurveNurbsPoles::Rational { points } => {
                    Some(scratch.collect(
                        points.iter().map(|pole| Some(pole.weight.get())),
                        "IR NURBS pcurve weights",
                        "IR NURBS pcurve weights work",
                    )?)
                }
            };
            return match nurbs_pcurve_differential_with(
                scratch,
                nurbs.degree(),
                nurbs.knots(),
                poles.count(),
                |index| match poles {
                    crate::geometry::pcurve::PcurveNurbsPoles::Polynomial { points } => {
                        points.get(index).copied().map(planar_pole)
                    }
                    crate::geometry::pcurve::PcurveNurbsPoles::Rational { points } => {
                        points.get(index).map(|pole| planar_pole(pole.point))
                    }
                },
                weights.as_deref(),
                parameter,
            ) {
                Ok(differential) => Some(PcurveEvaluation::from(differential)),
                // The quotient rule forms the derivatives from the finite
                // point, so a point outside the finite range reaches none.
                Err(EvaluationFailure::NonFinite(point)) => Some(
                    PcurveEvaluation::left_finite_range(point, Point2::new(f64::NAN, f64::NAN)),
                ),
                Err(EvaluationFailure::NoValue) => None,
                Err(EvaluationFailure::ResourceLimit(limit)) => {
                    Some(PcurveEvaluation::resource(limit))
                }
            };
        }
        PcurveGeometry::Transformed(placed) => {
            let transform = placed.transform();
            let basis = pcurve_uv_differential(scratch, placed.basis(), parameter)?;
            if basis.resource.is_some() {
                return Some(basis);
            }
            let point =
                transform.apply_point(basis.point.map_or_else(|point| point, FinitePoint2::get));
            let tangent = match basis.tangent {
                Ok(tangent) => admit_parameter_point(transform.apply_vector(tangent.get())),
                Err(EvaluationFailure::NonFinite(tangent)) => Err(EvaluationFailure::NonFinite(
                    transform.apply_vector(tangent),
                )),
                Err(EvaluationFailure::NoValue) => Err(EvaluationFailure::NoValue),
                Err(EvaluationFailure::ResourceLimit(limit)) => {
                    Err(EvaluationFailure::ResourceLimit(limit))
                }
            };
            let acceleration = basis
                .acceleration
                .finite()
                .map(|acceleration| transform.apply_vector(acceleration.get()));
            // The placement evaluates its derivatives with its point: a basis
            // tangent or acceleration the placement carries outside the finite
            // range leaves the evaluation there.
            let basis_tangent_finite = match basis.tangent {
                Ok(_) => true,
                Err(EvaluationFailure::ResourceLimit(limit)) => {
                    return Some(PcurveEvaluation::resource(limit));
                }
                Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(_)) => false,
            };
            let tangent_left_finite_range = match tangent {
                Ok(_) => false,
                Err(EvaluationFailure::ResourceLimit(limit)) => {
                    return Some(PcurveEvaluation::resource(limit));
                }
                Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(_)) => true,
            };
            let derivative_left_range = basis_tangent_finite && tangent_left_finite_range
                || acceleration.is_some_and(|acceleration| !acceleration.is_finite());
            if basis.point.is_err() || derivative_left_range {
                let tangent = match reached_value(tangent) {
                    Ok(value) => value,
                    Err(limit) => return Some(PcurveEvaluation::resource(limit)),
                };
                return Some(PcurveEvaluation::left_finite_range(point, tangent));
            }
            let acceleration = match basis.acceleration {
                PcurveAcceleration::Unstated => PcurveAcceleration::Unstated,
                PcurveAcceleration::Finite(_) | PcurveAcceleration::NonFinite => {
                    acceleration.and_then(FinitePoint2::new).into()
                }
            };
            return Some(PcurveEvaluation::evaluated(point, tangent, acceleration));
        }
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let basis = trimmed_pcurve.basis();
            return pcurve_uv_differential(scratch, basis, parameter);
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            let distance = offset_pcurve.distance();
            let basis = offset_pcurve.basis();
            let basis = pcurve_uv_differential(scratch, basis, parameter)?;
            if basis.resource.is_some() {
                return Some(basis);
            }
            let tangent = match basis.tangent {
                Ok(tangent) => tangent,
                // The offset direction is the unit normal of the basis
                // tangent, which a tangent outside the finite range does not
                // reach; a basis without a tangent states no direction.
                Err(EvaluationFailure::NonFinite(_)) => {
                    return Some(PcurveEvaluation::left_finite_range(
                        Point2::new(f64::NAN, f64::NAN),
                        Point2::new(f64::NAN, f64::NAN),
                    ));
                }
                Err(EvaluationFailure::NoValue) if basis.point.is_err() => {
                    return Some(PcurveEvaluation::left_finite_range(
                        Point2::new(f64::NAN, f64::NAN),
                        Point2::new(f64::NAN, f64::NAN),
                    ));
                }
                Err(EvaluationFailure::NoValue) => return None,
                Err(EvaluationFailure::ResourceLimit(limit)) => {
                    return Some(PcurveEvaluation::resource(limit))
                }
            };
            let speed = tangent.u.hypot(tangent.v);
            if speed == 0.0 {
                return None;
            }
            if !speed.is_finite() {
                // The unit normal of a tangent whose length overflows is not
                // reached.
                return Some(PcurveEvaluation::left_finite_range(
                    Point2::new(f64::NAN, f64::NAN),
                    Point2::new(f64::NAN, f64::NAN),
                ));
            }
            let unit = Point2::new(tangent.u / speed, tangent.v / speed);
            // The offset of a non-finite basis point is the non-finite point
            // it reaches.
            let basis_point = basis.point.map_or_else(|point| point, FinitePoint2::get);
            let point = Point2::new(
                basis_point.u - distance.get() * unit.v,
                basis_point.v + distance.get() * unit.u,
            );
            let tangent = basis.acceleration.finite().map(|acceleration| {
                let tangential_acceleration = unit.u * acceleration.u + unit.v * acceleration.v;
                let unit_derivative = Point2::new(
                    (acceleration.u - tangential_acceleration * unit.u) / speed,
                    (acceleration.v - tangential_acceleration * unit.v) / speed,
                );
                Point2::new(
                    tangent.u - distance.get() * unit_derivative.v,
                    tangent.v + distance.get() * unit_derivative.u,
                )
            });
            // The offset tangent reads the basis acceleration. A basis that
            // states one and has none at a finite point and tangent has an
            // acceleration outside the finite range, which the offset tangent
            // does not reach; an offset basis states none.
            let tangent = match tangent {
                Some(tangent) => admit_parameter_point(tangent),
                None if matches!(basis.acceleration, PcurveAcceleration::NonFinite) => Err(
                    EvaluationFailure::NonFinite(Point2::new(f64::NAN, f64::NAN)),
                ),
                None => Err(EvaluationFailure::NoValue),
            };
            return Some(PcurveEvaluation {
                point: FinitePoint2::new(point).ok_or(point),
                tangent,
                acceleration: PcurveAcceleration::Unstated,
                resource: None,
            });
        }
    };
    Some(PcurveEvaluation::evaluated(
        pair.0,
        admit_parameter_point(pair.1),
        FinitePoint2::new(pair.2).into(),
    ))
}

fn offset2(base: Point2, terms: &[(f64, Point2)]) -> Point2 {
    let coordinate = |base: f64, component: fn(&Point2) -> f64| match crate::math::sum::product_sum(
        std::iter::once(Some([1.0, base])).chain(
            terms
                .iter()
                .map(|(factor, vector)| Some([*factor, component(vector)])),
        ),
    ) {
        crate::math::sum::ProductSum::Value(value) => {
            value.finite().map_or(f64::NAN, FiniteReal::get)
        }
        crate::math::sum::ProductSum::Zero => 0.0,
        crate::math::sum::ProductSum::Undefined => f64::NAN,
    };
    Point2::new(coordinate(base.u, |p| p.u), coordinate(base.v, |p| p.v))
}

#[cfg(test)]
mod tests;

/// Evaluate the exact second derivative of a stored curve carrier, or report
/// why it has no finite value, as [`curve_tangent_solved`] states. A
/// procedural carrier without a solved cache has no value here.
pub fn curve_second_derivative<'ctx, 'arena: 'ctx>(
    admission: impl Into<admission::EvaluationAdmission<'ctx, 'arena>>,
    geometry: &CurveGeometry,
    t: f64,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    let scratch = decode::Scratch::new(admission);
    let result = (|| {
        curve_derivative_evaluation(
            &scratch,
            geometry.solved().ok_or(EvaluationFailure::NoValue)?,
            t,
            CurveDerivative::Second,
        )
    })();
    scratch.settle(result)
}

/// Evaluate the first partial derivatives of a surface carrier, or report
/// why they have no finite value, as [`surface_partials_solved`] states. A
/// procedural carrier without a solved cache has no value here.
pub fn surface_partials<'ctx, 'arena: 'ctx>(
    admission: impl Into<admission::EvaluationAdmission<'ctx, 'arena>>,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<SurfacePartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    let scratch = decode::Scratch::new(admission);
    let result = (|| {
        surface_first_order_solved(
            &scratch,
            geometry.solved().ok_or(EvaluationFailure::NoValue)?,
            u,
            v,
        )?
        .partials()
    })();
    scratch.settle(result)
}

/// Evaluate the second partial derivatives of a surface carrier, or report
/// why they have no finite value, as [`surface_second_partials_solved`]
/// states. A procedural carrier without a solved cache has no value here.
pub fn surface_second_partials<'ctx, 'arena: 'ctx>(
    admission: impl Into<admission::EvaluationAdmission<'ctx, 'arena>>,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    let scratch = decode::Scratch::new(admission);
    let result = (|| {
        surface_requested_jet_solved(
            &scratch,
            geometry.solved().ok_or(EvaluationFailure::NoValue)?,
            u,
            v,
            SurfaceRequest::Second,
        )?
        .jet.second_partials()
    })();
    scratch.settle(result)
}

/// Analytic surface parameters of the point on a surface carrier.
pub fn analytic_surface_parameters(
    geometry: &SurfaceGeometry,
    point: Point3,
) -> Option<FinitePoint2> {
    analytic_surface_parameters_solved(geometry.solved()?, point)
}

/// [`surface_first_order_solved`] of a surface carrier's solved geometry. A
/// procedural carrier without a solved cache has no value here.
fn surface_first_order<'ctx, 'arena: 'ctx>(
    admission: impl Into<admission::EvaluationAdmission<'ctx, 'arena>>,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let scratch = decode::Scratch::new(admission);
    let result = (|| {
        surface_first_order_solved(
            &scratch,
            geometry.solved().ok_or(EvaluationFailure::NoValue)?,
            u,
            v,
        )
    })();
    scratch.settle(result)
}

#[cfg(test)]
mod numerical_range_tests;
