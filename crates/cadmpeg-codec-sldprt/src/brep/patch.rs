// SPDX-License-Identifier: Apache-2.0
//! Native curve lookup and shape-preserving patch writes.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};

use super::index::scan::scan_stream_carriers;
use super::index::view::CarrierView;
use super::spline::patch::patch_nurbs_curve;

/// Return the typed curve carried by one stream-local attribute.
pub(crate) fn curve_by_attr(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    attr: u16,
) -> Result<Option<CurveGeometry>, cadmpeg_core::CodecError> {
    let carriers = scan_stream_carriers(ctx, body)?;
    let view = CarrierView::Stream(&carriers);
    view
        .curve(ctx, attr)?
        .map(|indexed| {
            indexed
                .carrier()
                .geometry
                .try_clone_for_decode(ctx, "SLDPRT patch curve geometry copy")
        })
        .transpose()
}

/// Replace the scalar run of one compact analytic carrier.
pub(crate) fn patch_compact_values(
    ctx: &DecodeContext<'_>,
    body: &mut [u8],
    attr: u16,
    values: &[f64],
) -> Result<bool, cadmpeg_core::CodecError> {
    let carriers = scan_stream_carriers(ctx, body)?;
    let view = CarrierView::Stream(&carriers);
    let Some(indexed) = view.curve(ctx, attr)? else {
        return Ok(false);
    };
    let carrier = indexed.carrier();
    let Some(size) = values.len().checked_mul(8) else {
        return Ok(false);
    };
    let Some(start) = carrier.end.checked_sub(size) else {
        return Ok(false);
    };
    let Some(bytes) = body.get_mut(start..carrier.end) else {
        return Ok(false);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(size),
        "patch SLDPRT compact values",
    )?;
    for (slot, value) in bytes.chunks_exact_mut(8).zip(values) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
    Ok(true)
}

/// Patch one stream-local NURBS curve without changing its storage shape.
pub(crate) fn patch_nurbs_by_attr(
    ctx: &DecodeContext<'_>,
    body: &mut [u8],
    attr: u16,
    new: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<bool, cadmpeg_core::CodecError> {
    let carriers = scan_stream_carriers(ctx, body)?;
    let view = CarrierView::Stream(&carriers);
    let Some(indexed) = view.curve(ctx, attr)? else {
        return Ok(false);
    };
    let carrier = indexed.carrier();
    let Some(SolvedCurveGeometry::Nurbs(old)) = carrier.geometry.solved() else {
        return Ok(false);
    };
    Ok(patch_nurbs_curve(ctx, body, carrier.offset, old, new, 0.001)?.is_some())
}

