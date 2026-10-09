// SPDX-License-Identifier: Apache-2.0
//! Generated lane placement backing survives independently of its axis index.

use crate::resolved_features::grid::GridCoordinate;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::holes::HolePlacement;
use std::collections::BTreeMap;

const COLLECT: &str = "collect SLDPRT generated hole placements";
const OUTPUT: &str = "retain SLDPRT generated hole placements";

pub(super) struct GeneratedHolePlacements<'ctx> {
    placements: Vec<HolePlacement>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx> GeneratedHolePlacements<'ctx> {
    pub(super) fn from_axes(
        ctx: &'ctx DecodeContext<'_>,
        source_storage: ScopedReservation<'ctx>,
        axes: BTreeMap<[GridCoordinate; 6], HolePlacement>,
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, OUTPUT)?;
        let placements = storage.with_storage(|| ctx.collect_vec(axes.into_values(), COLLECT))?;
        // The consumed map's iterator and backing have died inside collect_vec.
        drop(source_storage);
        Ok(Self { placements, storage })
    }

    pub(super) fn placements(&self) -> &[HolePlacement] { &self.placements }

    pub(super) fn into_output(self) -> Result<Vec<HolePlacement>, CodecError> {
        self.storage.commit_value(self.placements)
    }
}

#[cfg(test)]
mod tests;
