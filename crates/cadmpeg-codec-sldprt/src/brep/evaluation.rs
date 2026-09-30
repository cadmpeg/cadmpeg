// SPDX-License-Identifier: Apache-2.0
//! Caller admission for stored carrier evaluation scratch.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};

fn admit_nurbs_point<'ctx>(ctx: &'ctx DecodeContext<'_>, curve: &NurbsCurve) -> Result<ScopedReservation<'ctx>, CodecError> {
    const OPERATION: &str = "evaluate SLDPRT NURBS curve basis";
    let count = u64::from(curve.degree()).checked_add(1)
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let work = count.checked_mul(count).and_then(|work| work.checked_mul(64))
        .and_then(|work| work.checked_add(u64_from_index(curve.knots().len())))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, OPERATION)?;
    let bytes = count.checked_mul(u64_from_index(std::mem::size_of::<f64>()))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.reserve_scoped(bytes, OPERATION)
}

fn admit_solved_point<'ctx>(ctx: &'ctx DecodeContext<'_>, curve: &SolvedCurveGeometry) -> Result<ScopedReservation<'ctx>, CodecError> {
    const OPERATION: &str = "evaluate SLDPRT stored curve";
    ctx.charge_work(64, OPERATION)?;
    match curve {
        SolvedCurveGeometry::Nurbs(curve) => admit_nurbs_point(ctx, curve),
        SolvedCurveGeometry::Transformed(placed) => {
            let _depth = ctx.enter_nested(OPERATION)?;
            admit_solved_point(ctx, placed.basis())
        }
        SolvedCurveGeometry::Polyline(curve) => {
            ctx.charge_work(u64_from_index(curve.point_count()).checked_mul(64)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
            ctx.reserve_scoped(0, OPERATION)
        }
        _ => ctx.reserve_scoped(0, OPERATION),
    }
}

pub(super) fn curve_point(ctx: &DecodeContext<'_>, curve: &CurveGeometry, parameter: f64) -> Result<Option<FinitePoint3>, CodecError> {
    let Some(solved) = curve.solved() else { return Ok(None); };
    let _scratch = admit_solved_point(ctx, solved)?;
    Ok(cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::curve_point(curve, parameter))?)
}

pub(super) fn nurbs_curve_point(ctx: &DecodeContext<'_>, curve: &NurbsCurve, parameter: f64) -> Result<Option<FinitePoint3>, CodecError> {
    let _scratch = admit_nurbs_point(ctx, curve)?;
    Ok(cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::nurbs_curve_point_at(curve, parameter))?)
}

fn admit_nurbs_surface<'ctx>(
    ctx: &'ctx DecodeContext<'_>, surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    evaluations: u64, basis_lanes: u64, operation: &'static str,
) -> Result<ScopedReservation<'ctx>, CodecError> {
    let u = u64::from(surface.u_degree()).checked_add(1)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    let v = u64::from(surface.v_degree()).checked_add(1)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    let work = u.checked_mul(u).and_then(|work| work.checked_add(v.checked_mul(v)?))
        .and_then(|work| work.checked_add(u.checked_mul(v)?))
        .and_then(|work| work.checked_mul(64))
        .and_then(|work| work.checked_add(u64_from_index(surface.u_knots().len())))
        .and_then(|work| work.checked_add(u64_from_index(surface.v_knots().len())))
        .and_then(|work| work.checked_mul(evaluations))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, operation)?;
    let bytes = u.checked_add(v).and_then(|count| count.checked_mul(basis_lanes))
        .and_then(|count| count.checked_mul(u64_from_index(std::mem::size_of::<f64>())))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.reserve_scoped(bytes, operation)
}

pub(crate) fn nurbs_surface_point(
    ctx: &DecodeContext<'_>, surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface, u: f64, v: f64,
) -> Result<Option<FinitePoint3>, CodecError> {
    let _scratch = admit_nurbs_surface(ctx, surface, 1, 1, "evaluate SLDPRT NURBS surface basis")?;
    Ok(cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::nurbs_surface_point(surface, u, v))?)
}

pub(crate) fn nurbs_surface_parameter_near_point(
    ctx: &DecodeContext<'_>, surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    point: cadmpeg_ir::math::Point3, seed: Option<cadmpeg_ir::math::Point2>,
) -> Result<Option<cadmpeg_ir::units::FinitePoint2>, CodecError> {
    // The inverse evaluates at most 81 grid points, 24 first partials and
    // 288 line-search points. A partial costs at most three point evaluations.
    // Three basis lanes cover both bases, both derivatives and a lower basis.
    let _scratch = admit_nurbs_surface(ctx, surface, 441, 3, "project SLDPRT NURBS surface point")?;
    Ok(cadmpeg_ir::eval::nurbs_surface_parameter_near_point(surface, point, seed)?)
}

pub(crate) fn nurbs_surface_partials(
    ctx: &DecodeContext<'_>, surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface, u: f64, v: f64,
) -> Result<Option<cadmpeg_ir::eval::SurfacePartials<FinitePoint3, cadmpeg_ir::features::FiniteVector3>>, CodecError> {
    let _scratch = admit_nurbs_surface(ctx, surface, 3, 3, "evaluate SLDPRT NURBS surface partials")?;
    Ok(cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::nurbs_surface_partials(surface, u, v))?)
}

fn admit_solved_surface<'ctx>(
    ctx: &'ctx DecodeContext<'_>, surface: &cadmpeg_ir::geometry::SolvedSurfaceGeometry,
) -> Result<ScopedReservation<'ctx>, CodecError> {
    const OPERATION: &str = "evaluate SLDPRT stored surface";
    ctx.charge_work(64, OPERATION)?;
    match surface {
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(surface) => admit_nurbs_surface(ctx, surface, 1, 1, OPERATION),
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Transformed(placed) => {
            let _depth = ctx.enter_nested(OPERATION)?;
            admit_solved_surface(ctx, placed.basis())
        }
        _ => ctx.reserve_scoped(0, OPERATION),
    }
}

pub(crate) fn surface_point(
    ctx: &DecodeContext<'_>, surface: &cadmpeg_ir::geometry::SurfaceGeometry, u: f64, v: f64,
) -> Result<Option<FinitePoint3>, CodecError> {
    let Some(solved) = surface.solved() else { return Ok(None); };
    let _scratch = admit_solved_surface(ctx, solved)?;
    Ok(cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::surface_point(surface, u, v))?)
}

pub(super) fn admit_nurbs_isocurve<'ctx>(
    ctx: &'ctx DecodeContext<'_>, surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    fixed_axis: cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis,
) -> Result<ScopedReservation<'ctx>, CodecError> {
    const OPERATION: &str = "evaluate SLDPRT NURBS isocurve";
    let (degree, count) = match fixed_axis {
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U => (surface.u_degree(), surface.v_count()),
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::V => (surface.v_degree(), surface.u_count()),
    };
    let support = u64::from(degree).checked_add(1)
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let work = u64_from_index(count).checked_add(support)
        .and_then(|count| count.checked_mul(support))
        .and_then(|work| work.checked_mul(64))
        .and_then(|work| work.checked_add(u64_from_index(surface.u_knots().len())))
        .and_then(|work| work.checked_add(u64_from_index(surface.v_knots().len())))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, OPERATION)?;
    let bytes = cadmpeg_ir::eval::nurbs_surface_isocurve_scratch_bytes(surface, fixed_axis)?;
    ctx.reserve_scoped(u64_from_index(bytes), OPERATION)
}

const SURFACE_SOLVER_LOCAL_WORK: u64 = 1_000_000;

fn surface_solver_budget<'ctx>(
    ctx: &'ctx DecodeContext<'_>, surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    operation: &'static str,
) -> Result<cadmpeg_core::decode::WorkBudget<'ctx>, CodecError> {
    let (_, extraction_work) = cadmpeg_ir::eval::nurbs_surface_patch_workspace(surface)?;
    ctx.charge_work(extraction_work, operation)?;
    // One normalized evaluator unit also pays for fixed arithmetic and a
    // complete scan of both knot vectors. Its local ceiling stays unchanged.
    let scale = 64_u64.checked_add(u64_from_index(surface.u_knots().len()))
        .and_then(|scale| scale.checked_add(u64_from_index(surface.v_knots().len())))
        .and_then(std::num::NonZeroU64::new)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    Ok(ctx.work_budget(SURFACE_SOLVER_LOCAL_WORK).with_session_work_scale(scale))
}

pub(super) fn nurbs_surface_parameter_within_tolerance(
    ctx: &DecodeContext<'_>, surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    point: cadmpeg_ir::math::Point3, seed: Option<cadmpeg_ir::math::Point2>, tolerance: f64,
) -> Result<Option<cadmpeg_ir::units::FinitePoint2>, CodecError> {
    const OPERATION: &str = "invert SLDPRT NURBS surface globally";
    let budget = surface_solver_budget(ctx, surface, OPERATION)?;
    let result = cadmpeg_ir::eval::nurbs_surface_parameter_within_tolerance_with_budget(
        surface, point, seed, tolerance, &budget,
    );
    // A zero charge observes the session's original sticky refusal.
    ctx.charge_work(0, OPERATION)?;
    if budget.exhausted() {
        return Err(ctx.refuse_codec_limit(OPERATION, SURFACE_SOLVER_LOCAL_WORK, SURFACE_SOLVER_LOCAL_WORK + 1));
    }
    Ok(result?)
}

pub(super) fn nurbs_surface_parameter_segment_chord_bound(
    ctx: &DecodeContext<'_>, surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    parameters: [cadmpeg_ir::math::Point2; 2], chord: [cadmpeg_ir::math::Point3; 2],
) -> Result<Option<f64>, CodecError> {
    const OPERATION: &str = "certify SLDPRT NURBS surface chord";
    let budget = surface_solver_budget(ctx, surface, OPERATION)?;
    let u = u64::from(surface.u_degree()).checked_add(1)
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let v = u64::from(surface.v_degree()).checked_add(1)
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let patches = u64_from_index(surface.u_count()).checked_mul(u64_from_index(surface.v_count()))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let splits = patches.checked_mul(4).and_then(|count| count.checked_add(2))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    // Each split can search all patches. Restricting a tensor-product line
    // costs at most its squared support times the combined degrees; the
    // square of the split count also covers the in-place sort's comparisons.
    let work = u.checked_mul(v).and_then(|support| support.checked_mul(support))
        .and_then(|work| work.checked_mul(u.checked_add(v)?))
        .and_then(|work| work.checked_mul(64))
        .and_then(|work| work.checked_add(patches))
        .and_then(|work| work.checked_mul(splits))
        .and_then(|work| work.checked_add(splits.checked_mul(splits)?.checked_mul(64)?))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, OPERATION)?;
    let result = cadmpeg_ir::eval::nurbs_surface_parameter_segment_chord_bound_with_budget(
        surface, parameters, chord, &budget,
    );
    ctx.charge_work(0, OPERATION)?;
    if budget.exhausted() {
        return Err(ctx.refuse_codec_limit(OPERATION, SURFACE_SOLVER_LOCAL_WORK, SURFACE_SOLVER_LOCAL_WORK + 1));
    }
    Ok(result?)
}
