// SPDX-License-Identifier: Apache-2.0
//! Stream-local carrier backing and consuming source tables.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use super::IndexedCurve;
use crate::brep::{blend, offset, sweep, CurveCarrier, SurfaceCarrier};
use crate::brep::intersection::IntersectionCarrier;

/// Geometry and optional support-UV backing have separate construction scopes.
/// Neither scope includes a table shell or discarded parameter lane.
enum CurveBacking<'ctx> {
    Exact { _geometry: ScopedReservation<'ctx> },
    Derived {
        _geometry: ScopedReservation<'ctx>,
        _support_uv: ScopedReservation<'ctx>,
    },
}

pub(in crate::brep) struct StreamCurve<'ctx> {
    pub(in crate::brep::index) value: IndexedCurve,
    _backing: CurveBacking<'ctx>,
}

impl<'ctx> StreamCurve<'ctx> {
    pub(in crate::brep) fn exact(value: CurveCarrier, geometry: ScopedReservation<'ctx>) -> Self {
        Self { value: IndexedCurve::Exact(value), _backing: CurveBacking::Exact { _geometry: geometry } }
    }

    pub(in crate::brep) fn derived(
        value: IntersectionCarrier,
        geometry: ScopedReservation<'ctx>,
        support_uv: ScopedReservation<'ctx>,
    ) -> Self {
        Self {
            value: IndexedCurve::Derived(value),
            _backing: CurveBacking::Derived { _geometry: geometry, _support_uv: support_uv },
        }
    }
}

pub(in crate::brep) struct StreamSurface<'ctx> {
    pub(in crate::brep::index) value: SurfaceCarrier,
    _geometry: ScopedReservation<'ctx>,
}

impl<'ctx> StreamSurface<'ctx> {
    pub(in crate::brep) fn new(value: SurfaceCarrier, geometry: ScopedReservation<'ctx>) -> Self {
        Self { value, _geometry: geometry }
    }
}

pub(in crate::brep) struct StreamLoss<'ctx> {
    pub(in crate::brep::index) value: LossNote,
    _message: ScopedReservation<'ctx>,
}

impl<'ctx> StreamLoss<'ctx> {
    pub(in crate::brep) fn new(value: LossNote, message: ScopedReservation<'ctx>) -> Self {
        Self { value, _message: message }
    }
}

/// Only typed map-node storage is created under this shell.
pub(in crate::brep) struct CarrierTable<'ctx, V> {
    pub(in crate::brep::index) entries: BTreeMap<u16, V>,
    shell: ScopedReservation<'ctx>,
}

impl<'ctx, V> CarrierTable<'ctx, V> {
    pub(in crate::brep::index) fn empty(ctx: &'ctx DecodeContext<'_>, operation: &'static str) -> Result<Self, CodecError> {
        Ok(Self { entries: BTreeMap::new(), shell: ctx.reserve_scoped(0, operation)? })
    }

    pub(in crate::brep::index) fn replace(
        &mut self,
        ctx: &DecodeContext<'_>,
        attr: u16,
        value: V,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.shell.with_storage(|| ctx.insert_btree_map(&mut self.entries, attr, value, operation))?;
        Ok(())
    }

    pub(in crate::brep::index) fn keep_first(
        &mut self,
        ctx: &DecodeContext<'_>,
        attr: u16,
        value: V,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.shell.with_storage(|| {
            ctx.entry_btree_map(&mut self.entries, attr, operation)?.or_insert(value);
            Ok(())
        })
    }

    fn into_entries(self) -> CarrierEntries<'ctx, V> {
        CarrierEntries { entries: self.entries.into_iter(), _shell: self.shell }
    }

    pub(in crate::brep::index) fn replace_from(
        &mut self, ctx: &DecodeContext<'_>, source: Self, source_operation: &'static str, target_operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(limit) = ctx.resource_refusal() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let mut source = source.into_entries();
        while source.entries.len() != 0 {
            let Some((attr, value)) = ctx.next_charged(&mut source.entries, source_operation)? else {
                break;
            };
            self.replace(ctx, attr, value, target_operation)?;
        }
        Ok(())
    }

    fn merge_missing(
        &mut self,
        ctx: &DecodeContext<'_>,
        source: Self,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        // The owner retains the source shell through the actual IntoIter,
        // including an early resource refusal. Intact child owners move;
        // occupied-source children drop. No drained shell is transferred.
        if let Some(limit) = ctx.resource_refusal() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let mut source = source.into_entries();
        while source.entries.len() != 0 {
            let Some((attr, value)) = ctx.next_charged(&mut source.entries, operation)? else {
                break;
            };
            self.keep_first(ctx, attr, value, operation)?;
        }
        Ok(())
    }
}

struct CarrierEntries<'ctx, V> {
    entries: std::collections::btree_map::IntoIter<u16, V>,
    _shell: ScopedReservation<'ctx>,
}

pub(in crate::brep) struct CarrierLosses<'ctx> {
    pub(in crate::brep::index) entries: Vec<StreamLoss<'ctx>>,
    shell: ScopedReservation<'ctx>,
}

impl<'ctx> CarrierLosses<'ctx> {
    pub(in crate::brep::index) fn empty(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self { entries: Vec::new(), shell: ctx.reserve_scoped(0, "hold SLDPRT lane refusals")? })
    }

    pub(in crate::brep::index) fn push(
        &mut self, ctx: &DecodeContext<'_>, value: StreamLoss<'ctx>, operation: &'static str,
    ) -> Result<(), CodecError> {
        self.shell.with_storage(|| ctx.push_vec(&mut self.entries, value, operation))
    }

    fn append(&mut self, ctx: &DecodeContext<'_>, mut source: Self) -> Result<(), CodecError> {
        self.shell.with_storage(|| {
            ctx.append_vec(&mut self.entries, &mut source.entries, "merge SLDPRT lane refusals")
        })
        // Source Vec and its shell still share one owner on Err/unwind.
    }
}

pub(in crate::brep) struct StreamCarrierIndex<'ctx> {
    pub(in crate::brep::index) curves: CarrierTable<'ctx, StreamCurve<'ctx>>,
    pub(in crate::brep::index) surfaces: CarrierTable<'ctx, StreamSurface<'ctx>>,
    pub(in crate::brep::index) sweeps: CarrierTable<'ctx, sweep::SweepCarrier>,
    pub(in crate::brep::index) blends: CarrierTable<'ctx, blend::BlendCarrier>,
    pub(in crate::brep::index) offsets: CarrierTable<'ctx, offset::OffsetCarrier>,
    pub(in crate::brep::index) blend_support_pairs: CarrierTable<'ctx, blend::SupportPairCarrier>,
    pub(in crate::brep::index) lane_refusals: CarrierLosses<'ctx>,
}

impl<'ctx> StreamCarrierIndex<'ctx> {
    pub(in crate::brep) fn empty(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            curves: CarrierTable::empty(ctx, "hold SLDPRT curve carrier table")?,
            surfaces: CarrierTable::empty(ctx, "hold SLDPRT surface carrier table")?,
            sweeps: CarrierTable::empty(ctx, "hold SLDPRT sweep carrier table")?,
            blends: CarrierTable::empty(ctx, "hold SLDPRT blend carrier table")?,
            offsets: CarrierTable::empty(ctx, "hold SLDPRT offset carrier table")?,
            blend_support_pairs: CarrierTable::empty(ctx, "hold SLDPRT blend support table")?,
            lane_refusals: CarrierLosses::empty(ctx)?,
        })
    }

    pub(in crate::brep) fn merge_missing(&mut self, ctx: &DecodeContext<'_>, source: Self) -> Result<(), CodecError> {
        self.curves.merge_missing(ctx, source.curves, "merge SLDPRT curve carriers")?;
        self.surfaces.merge_missing(ctx, source.surfaces, "merge SLDPRT surface carriers")?;
        self.sweeps.merge_missing(ctx, source.sweeps, "merge SLDPRT sweep carriers")?;
        self.blends.merge_missing(ctx, source.blends, "merge SLDPRT blend carriers")?;
        self.offsets.merge_missing(ctx, source.offsets, "merge SLDPRT offset carriers")?;
        self.blend_support_pairs.merge_missing(ctx, source.blend_support_pairs, "merge SLDPRT blend support pairs")?;
        self.lane_refusals.append(ctx, source.lane_refusals)
    }
}

#[cfg(test)]
mod tests;
