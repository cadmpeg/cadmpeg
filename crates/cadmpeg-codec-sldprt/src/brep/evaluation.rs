// SPDX-License-Identifier: Apache-2.0
//! Caller admission for stored curve evaluation scratch.

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
