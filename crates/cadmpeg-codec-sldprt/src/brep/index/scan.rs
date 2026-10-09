// SPDX-License-Identifier: Apache-2.0
//! Ordered stream-local carrier construction and concrete analytic publication.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::publication::{CurveSink, FixedSink, IntersectionSink, LossSink, SurfaceSink};
use super::stream::{CarrierTable, StreamCarrierIndex, StreamCurve, StreamSurface};
use super::view::CarrierView;
#[cfg(test)]
use super::CarrierIndex;
use crate::brep::{blend, intersection, offset, parse_carrier, spline, subset, sweep, Carrier};

pub(in crate::brep::index) enum AnalyticSink<'out, 'ctx> {
    #[cfg(test)]
    Retained(&'out mut CarrierIndex),
    Stream(&'out mut StreamCarrierIndex<'ctx>),
}

pub(in crate::brep::index) fn scan_analytic_carriers<'ctx>(
    ctx: &'ctx DecodeContext<'_>, body: &[u8], out: &mut AnalyticSink<'_, 'ctx>,
) -> Result<(), CodecError> {
    // Two fixed-width analytic framings at most are parsed at each start.
    let starts = 0..body.len().checked_sub(1).map_or(0, |end| end);
    for i in ctx.admit_iter(starts, "scan SLDPRT analytic carriers")? {
        let Some(carrier) = parse_carrier(body, i) else { continue; };
        match out {
            #[cfg(test)]
            AnalyticSink::Retained(out) => out.insert(ctx, carrier)?,
            AnalyticSink::Stream(out) => match carrier {
                // parse_carrier accepts only fixed analytic geometry. These
                // zero-byte child owners never wrap an external NURBS value.
                Carrier::Curve(value) => out.curves.replace(ctx, value.attr,
                    StreamCurve::exact(value, ctx.reserve_scoped(0, "hold SLDPRT analytic curve")?),
                    "index SLDPRT curve carriers")?,
                Carrier::Surface(value) => out.surfaces.replace(ctx, value.attr,
                    StreamSurface::new(value, ctx.reserve_scoped(0, "hold SLDPRT analytic surface")?),
                    "index SLDPRT surface carriers")?,
            },
        }
    }
    Ok(())
}

pub(in crate::brep) fn scan_stream_carriers<'ctx>(
    ctx: &'ctx DecodeContext<'_>, body: &[u8],
) -> Result<StreamCarrierIndex<'ctx>, CodecError> {
    let mut out = StreamCarrierIndex::empty(ctx)?;
    scan_analytic_carriers(ctx, body, &mut AnalyticSink::Stream(&mut out))?;
    // Spline winner tables start empty. An analytic with the same attr must
    // not suppress descriptor discovery or the first valid spline attempt.
    let mut curves = CarrierTable::empty(ctx, "hold SLDPRT spline curve source")?;
    spline::scan_curve_carriers_into(ctx, body,
        &mut LossSink::Stream(&mut out.lane_refusals), &mut CurveSink::Stream(&mut curves))?;
    out.curves.replace_from(ctx, curves, "index SLDPRT spline curve carriers", "index SLDPRT curve carriers")?;
    let mut surfaces = CarrierTable::empty(ctx, "hold SLDPRT spline surface source")?;
    spline::scan_surface_carriers_into(ctx, body,
        &mut LossSink::Stream(&mut out.lane_refusals), &mut SurfaceSink::Stream(&mut surfaces))?;
    out.surfaces.replace_from(ctx, surfaces, "index SLDPRT spline surface carriers", "index SLDPRT surface carriers")?;
    // The complete subset run evaluates against the pre-subset index.
    // Publication then replaces in occurrence order, keeping the source shell
    // alive through any early failure of the actual consuming iterator.
    let mut subsets = subset::scan_scoped(ctx, body, CarrierView::Stream(&out))?.into_entries();
    while let Some(value) = subsets.next(ctx, "index SLDPRT subset carriers")? {
        out.curves.replace(ctx, value.value.carrier().attr, value, "index SLDPRT curve carriers")?;
    }
    drop(subsets);
    sweep::scan_into(ctx, body, &mut FixedSink::Stream(&mut out.sweeps))?;
    blend::scan_into(ctx, body, &mut FixedSink::Stream(&mut out.blends),
        &mut FixedSink::Stream(&mut out.blend_support_pairs))?;
    offset::scan_into(ctx, body, &mut FixedSink::Stream(&mut out.offsets))?;
    intersection::scan_intersection_carriers_into(ctx, body,
        &mut LossSink::Stream(&mut out.lane_refusals), &mut IntersectionSink::Stream(&mut out.curves))?;
    Ok(out)
}
