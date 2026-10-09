// SPDX-License-Identifier: Apache-2.0
//! Concrete standalone and stream-local spline publication.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use super::stream::{CarrierLosses, CarrierTable, StreamCurve, StreamLoss, StreamSurface};
use crate::brep::{CurveCarrier, SurfaceCarrier};

pub(in crate::brep) enum CurveSink<'out, 'ctx> {
    Retained(&'out mut BTreeMap<u16, CurveCarrier>),
    Stream(&'out mut CarrierTable<'ctx, StreamCurve<'ctx>>),
}

impl<'ctx> CurveSink<'_, 'ctx> {
    pub(in crate::brep) fn contains(&self, ctx: &DecodeContext<'_>, attr: u16) -> Result<bool, CodecError> {
        match self {
            Self::Retained(out) => ctx.contains_key_btree_map(out, &attr, "collect Parasolid curve carriers"),
            Self::Stream(out) => ctx.contains_key_btree_map(&out.entries, &attr, "collect Parasolid curve carriers"),
        }
    }

    pub(in crate::brep) fn publish(
        &mut self, ctx: &DecodeContext<'_>, value: CurveCarrier, geometry: ScopedReservation<'ctx>,
    ) -> Result<(), CodecError> {
        let attr = value.attr;
        match self {
            Self::Retained(out) => {
                let value = geometry.commit_value(value)?;
                ctx.entry_btree_map(out, attr, "collect Parasolid curve carriers")?.or_insert(value);
                Ok(())
            }
            Self::Stream(out) => out.keep_first(ctx, attr, StreamCurve::exact(value, geometry), "collect Parasolid curve carriers"),
        }
    }
}

pub(in crate::brep) enum SurfaceSink<'out, 'ctx> {
    Retained(&'out mut BTreeMap<u16, SurfaceCarrier>),
    Stream(&'out mut CarrierTable<'ctx, StreamSurface<'ctx>>),
}

impl<'ctx> SurfaceSink<'_, 'ctx> {
    pub(in crate::brep) fn contains(&self, ctx: &DecodeContext<'_>, attr: u16) -> Result<bool, CodecError> {
        match self {
            Self::Retained(out) => ctx.contains_key_btree_map(out, &attr, "collect Parasolid surface carriers"),
            Self::Stream(out) => ctx.contains_key_btree_map(&out.entries, &attr, "collect Parasolid surface carriers"),
        }
    }

    pub(in crate::brep) fn publish(
        &mut self, ctx: &DecodeContext<'_>, value: SurfaceCarrier, geometry: ScopedReservation<'ctx>,
    ) -> Result<(), CodecError> {
        let attr = value.attr;
        match self {
            Self::Retained(out) => {
                let value = geometry.commit_value(value)?;
                ctx.entry_btree_map(out, attr, "collect Parasolid surface carriers")?.or_insert(value);
                Ok(())
            }
            Self::Stream(out) => out.keep_first(ctx, attr, StreamSurface::new(value, geometry), "collect Parasolid surface carriers"),
        }
    }
}

pub(in crate::brep) enum LossSink<'out, 'ctx> {
    Retained(&'out mut Vec<LossNote>),
    Stream(&'out mut CarrierLosses<'ctx>),
}

/// The formatted value and its concrete destination. Formatting completes
/// while the IR diagnostic is live; publication follows diagnostic teardown.
pub(in crate::brep) struct PendingLoss<'out, 'ctx> {
    inner: PendingLossInner<'out, 'ctx>,
}

enum PendingLossInner<'out, 'ctx> {
    Retained { value: LossNote, output: &'out mut Vec<LossNote> },
    Stream { value: StreamLoss<'ctx>, output: &'out mut CarrierLosses<'ctx> },
}

impl<'ctx> LossSink<'_, 'ctx> {
    pub(in crate::brep) fn note<'note>(
        &'note mut self,
        ctx: &'ctx DecodeContext<'_>,
        record: impl std::fmt::Display,
    ) -> Result<PendingLoss<'note, 'ctx>, CodecError> {
        let inner = match self {
            Self::Retained(output) => PendingLossInner::Retained {
                value: crate::loss::spline_lane_refusal(ctx, record)?,
                output,
            },
            Self::Stream(output) => {
                let mut storage = ctx.reserve_scoped(0, "hold SLDPRT spline loss message")?;
                let value = storage.with_storage(|| crate::loss::spline_lane_refusal(ctx, record))?;
                PendingLossInner::Stream { value: StreamLoss::new(value, storage), output }
            }
        };
        Ok(PendingLoss { inner })
    }
}

impl PendingLoss<'_, '_> {
    pub(in crate::brep) fn publish(self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        self.publish_at(ctx, "collect Parasolid spline refusals")
    }

    pub(in crate::brep) fn publish_at(self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<(), CodecError> {
        match self.inner {
            PendingLossInner::Retained { value, output } => ctx.push_vec(output, value, operation),
            PendingLossInner::Stream { value, output } => output.push(ctx, value, operation),
        }
    }
}

/// Publication of fixed construction fields. Only the concrete table shell
/// has dynamic backing; the value itself needs no geometry reservation.
pub(in crate::brep) enum FixedSink<'out, 'ctx, V> {
    #[cfg(test)]
    Retained(&'out mut BTreeMap<u16, V>),
    Stream(&'out mut CarrierTable<'ctx, V>),
}

impl<V> FixedSink<'_, '_, V> {
    pub(in crate::brep) fn keep_first(
        &mut self, ctx: &DecodeContext<'_>, attr: u16, value: V, operation: &'static str,
    ) -> Result<(), CodecError> {
        match self {
            #[cfg(test)]
            Self::Retained(out) => {
                ctx.entry_btree_map(out, attr, operation)?.or_insert(value);
                Ok(())
            }
            Self::Stream(out) => out.keep_first(ctx, attr, value, operation),
        }
    }

    pub(in crate::brep) fn replace(
        &mut self, ctx: &DecodeContext<'_>, attr: u16, value: V, operation: &'static str,
    ) -> Result<(), CodecError> {
        match self {
            #[cfg(test)]
            Self::Retained(out) => {
                ctx.insert_btree_map(out, attr, value, operation)?;
                Ok(())
            }
            Self::Stream(out) => out.replace(ctx, attr, value, operation),
        }
    }
}

pub(in crate::brep) enum IntersectionSink<'out, 'ctx> {
    #[cfg(test)]
    Retained(&'out mut BTreeMap<u16, crate::brep::intersection::IntersectionCarrier>),
    Stream(&'out mut CarrierTable<'ctx, StreamCurve<'ctx>>),
}

struct PendingIntersection<'ctx> {
    value: crate::brep::intersection::IntersectionCarrier,
    geometry: ScopedReservation<'ctx>,
    support_uv: ScopedReservation<'ctx>,
}

impl PendingIntersection<'_> {
    #[cfg(test)]
    fn retain(mut self) -> Result<crate::brep::intersection::IntersectionCarrier, CodecError> {
        self.geometry.absorb(&mut self.support_uv)?;
        self.geometry.commit_value(self.value)
    }
}

impl<'ctx> IntersectionSink<'_, 'ctx> {
    pub(in crate::brep) fn publish(
        &mut self, ctx: &DecodeContext<'_>,
        value: crate::brep::intersection::IntersectionCarrier,
        geometry: ScopedReservation<'ctx>, support_uv: ScopedReservation<'ctx>,
    ) -> Result<(), CodecError> {
        let value = PendingIntersection { value, geometry, support_uv };
        let attr = value.value.carrier.attr;
        match self {
            #[cfg(test)]
            Self::Retained(out) => {
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    ctx.entry_btree_map(out, attr, "collect Parasolid intersection carriers")?
                {
                    entry.insert(value.retain()?);
                }
                Ok(())
            }
            Self::Stream(out) => out.keep_first(ctx, attr,
                StreamCurve::derived(value.value, value.geometry, value.support_uv),
                "collect Parasolid intersection carriers"),
        }
    }
}
