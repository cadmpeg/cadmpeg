// SPDX-License-Identifier: Apache-2.0
//! Borrowed queries over retained standalone and scoped stream carriers.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::stream::StreamCarrierIndex;
use super::IndexedCurve;
#[cfg(test)]
use super::CarrierIndex;
use crate::brep::{blend, offset, sweep, SurfaceCarrier};

#[derive(Clone, Copy)]
pub(in crate::brep) enum CarrierView<'a, 'ctx> {
    #[cfg(test)]
    Retained(&'a CarrierIndex),
    Stream(&'a StreamCarrierIndex<'ctx>),
}

impl CarrierView<'_, '_> {
    pub(in crate::brep) fn curve(
        &self, ctx: &DecodeContext<'_>, attr: u16,
    ) -> Result<Option<&IndexedCurve>, CodecError> {
        match self {
            #[cfg(test)]
            Self::Retained(index) => index.curve(ctx, attr),
            Self::Stream(index) => Ok(ctx.get_btree_map(&index.curves.entries, &attr,
                "lookup SLDPRT curve carrier")?.map(|value| &value.value)),
        }
    }

    pub(in crate::brep) fn surface(
        &self, ctx: &DecodeContext<'_>, attr: u16,
    ) -> Result<Option<&SurfaceCarrier>, CodecError> {
        match self {
            #[cfg(test)]
            Self::Retained(index) => index.surface(ctx, attr),
            Self::Stream(index) => Ok(ctx.get_btree_map(&index.surfaces.entries, &attr,
                "lookup SLDPRT surface carrier")?.map(|value| &value.value)),
        }
    }

    pub(in crate::brep) fn curve_attrs(&self, ctx: &DecodeContext<'_>) -> Result<BTreeSet<u16>, CodecError> {
        match self {
            #[cfg(test)]
            Self::Retained(index) => index.curve_attrs(ctx),
            Self::Stream(index) => ctx.collect_btree_set(index.curves.entries.keys().copied(),
                "collect SLDPRT curve attributes"),
        }
    }

    pub(in crate::brep) fn sweep(
        &self, ctx: &DecodeContext<'_>, attr: u16,
    ) -> Result<Option<&sweep::SweepCarrier>, CodecError> {
        match self {
            #[cfg(test)]
            Self::Retained(index) => index.sweep(ctx, attr),
            Self::Stream(index) => ctx.get_btree_map(&index.sweeps.entries, &attr,
                "lookup SLDPRT sweep carrier"),
        }
    }

    pub(in crate::brep) fn blend(
        &self, ctx: &DecodeContext<'_>, attr: u16,
    ) -> Result<Option<&blend::BlendCarrier>, CodecError> {
        match self {
            #[cfg(test)]
            Self::Retained(index) => index.blend(ctx, attr),
            Self::Stream(index) => ctx.get_btree_map(&index.blends.entries, &attr,
                "lookup SLDPRT blend carrier"),
        }
    }

    pub(in crate::brep) fn offset(
        &self, ctx: &DecodeContext<'_>, attr: u16,
    ) -> Result<Option<&offset::OffsetCarrier>, CodecError> {
        match self {
            #[cfg(test)]
            Self::Retained(index) => index.offset(ctx, attr),
            Self::Stream(index) => ctx.get_btree_map(&index.offsets.entries, &attr,
                "lookup SLDPRT offset carrier"),
        }
    }

    pub(in crate::brep) fn blend_support_pair(
        &self, ctx: &DecodeContext<'_>, attr: u16,
    ) -> Result<Option<&blend::SupportPairCarrier>, CodecError> {
        match self {
            #[cfg(test)]
            Self::Retained(index) => index.blend_support_pair(ctx, attr),
            Self::Stream(index) => ctx.get_btree_map(&index.blend_support_pairs.entries, &attr,
                "lookup SLDPRT blend_support_pair carrier"),
        }
    }
    /// Clone the final loss output at the graph's original publication point.
    /// All output slots and copied messages survive together; source messages
    /// retain their own live owners until the carrier index is destroyed.
    pub(in crate::brep) fn clone_lane_refusals(
        &self, ctx: &DecodeContext<'_>,
    ) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, CodecError> {
        const OPERATION: &str = "retain Parasolid carrier losses";
        let (values, output_storage) = ctx.with_scoped_storage(OPERATION, || {
            match self {
                #[cfg(test)]
                Self::Retained(index) => ctx.try_collect_vec(
                    index.lane_refusals.iter().map(|note| note.try_clone_for_decode(ctx, OPERATION)),
                    OPERATION,
                ),
                Self::Stream(index) => ctx.try_collect_vec(
                    index.lane_refusals.entries.iter().map(|note| note.value.try_clone_for_decode(ctx, OPERATION)),
                    OPERATION,
                ),
            }
        })?;
        output_storage.commit_value(values)
    }

}
