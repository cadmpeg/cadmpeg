// SPDX-License-Identifier: Apache-2.0
//! Native curve lookup and shape-preserving patch writes.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

use super::index::scan::scan_stream_carriers;
use super::index::view::CarrierView;
use super::spline::patch::patch_nurbs_curve;

/// Fixed analytic facts selected from the unchanged source buffer.
#[derive(Clone, Copy)]
pub(crate) enum CompactCurveFamily {
    Circle(cadmpeg_ir::units::OrthonormalFrame3),
    Ellipse(cadmpeg_ir::units::UnitVector3),
    Other,
}

struct CompactCurveSelection {
    end: usize,
    family: CompactCurveFamily,
}

/// Holds exclusive source authority from lookup through one scalar edit.
/// The source index dies after its fixed selection is copied.
pub(crate) struct CompactCurvePatch<'body, 'ctx, 'arena> {
    body: &'body mut [u8],
    context: &'ctx DecodeContext<'arena>,
    selection: Option<CompactCurveSelection>,
}

impl<'body, 'ctx, 'arena> CompactCurvePatch<'body, 'ctx, 'arena> {
    pub(crate) fn new(
        ctx: &'ctx DecodeContext<'arena>, body: &'body mut [u8], attr: u16,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let selection = {
            let carriers = scan_stream_carriers(ctx, body)?;
            let view = CarrierView::Stream(&carriers);
            view.curve(ctx, attr)?.map(|indexed| {
                let carrier = indexed.carrier();
                let family = match carrier.geometry.solved() {
                    Some(SolvedCurveGeometry::Circle(curve)) => CompactCurveFamily::Circle(*curve.frame()),
                    Some(SolvedCurveGeometry::Ellipse(curve)) => CompactCurveFamily::Ellipse(*curve.frame().axis()),
                    _ => CompactCurveFamily::Other,
                };
                CompactCurveSelection { end: carrier.end, family }
            })
        };
        Ok(Self { body, context: ctx, selection })
    }

    pub(crate) fn family(&self) -> Option<CompactCurveFamily> {
        self.selection.as_ref().map(|selection| selection.family)
    }

    /// Consumes the selection before later mutations can change its source.
    pub(crate) fn apply(self, values: &[f64]) -> Result<bool, cadmpeg_core::CodecError> {
        let ctx = self.context;
        if let Some(limit) = ctx.resource_refusal() {
            return Err(cadmpeg_core::CodecError::ResourceLimit(limit));
        }
        let Some(selection) = self.selection else { return Ok(false); };
        let Some(size) = values.len().checked_mul(8) else { return Ok(false); };
        let Some(start) = selection.end.checked_sub(size) else { return Ok(false); };
        let Some(bytes) = self.body.get_mut(start..selection.end) else { return Ok(false); };
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(size), "patch SLDPRT compact values")?;
        for (slot, value) in bytes.chunks_exact_mut(8).zip(values) {
            slot.copy_from_slice(&value.to_be_bytes());
        }
        Ok(true)
    }
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


#[cfg(test)]
mod tests;
