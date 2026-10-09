// SPDX-License-Identifier: Apache-2.0
//! Planar position placements retain only their surviving vector backing.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::holes::HolePlacement;
use cadmpeg_ir::features::{FeatureDirection3, FeatureId, FinitePoint3};

const OPERATION: &str = "project SLDPRT hole position sketches";

pub(super) struct PlanarHolePlacements<'source, 'ctx, 'arena> {
    context: &'ctx DecodeContext<'arena>,
    placements: Vec<HolePlacement>,
    dependency: &'source FeatureId,
    storage: ScopedReservation<'ctx>,
}

impl<'source, 'ctx, 'arena> PlanarHolePlacements<'source, 'ctx, 'arena> {
    pub(super) fn new(
        ctx: &'ctx DecodeContext<'arena>,
        dependency: &'source FeatureId,
    ) -> Result<Self, CodecError> {
        Ok(Self { context: ctx, placements: Vec::new(), dependency, storage: ctx.reserve_scoped(0, OPERATION)? })
    }

    pub(super) fn push_axis(
        &mut self,
        origin: FinitePoint3,
        axis: FeatureDirection3,
    ) -> Result<(), CodecError> {
        self.storage.with_storage(|| self.context.push_vec(&mut self.placements, HolePlacement::Axis { origin, axis }, OPERATION))
    }

    pub(super) fn clear(&mut self) -> Result<(), CodecError> {
        self.context.clear_vec(&mut self.placements, OPERATION)
    }

    pub(super) fn placements(&self) -> &[HolePlacement] { &self.placements }

    pub(super) fn into_output(self) -> Result<(Vec<HolePlacement>, &'source FeatureId), CodecError> {
        let placements = self.storage.commit_value(self.placements)?;
        Ok((placements, self.dependency))
    }
}

#[cfg(test)]
mod tests;
