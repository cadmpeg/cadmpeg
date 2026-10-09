// SPDX-License-Identifier: Apache-2.0
//! Ordered bore-solution keys and placements with separate backing owners.

use super::EPS_HOLE_POSITION;
use crate::resolved_features::grid::GridCoordinate;
use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDirection3, FinitePoint3};
use cadmpeg_ir::features::holes::HolePlacement;
use std::cmp::Ordering;
use std::collections::BTreeMap;

const RETAIN: &str = "retain SLDPRT bore pattern solution";
const OUTPUT: &str = "match SLDPRT marker loci to bore axes";

type Candidate = ([GridCoordinate; 3], FinitePoint3, FeatureDirection3);

struct BoreSolutionKey<'ctx> {
    coordinates: Vec<[GridCoordinate; 6]>,
    _storage: ScopedReservation<'ctx>,
}

impl PartialEq for BoreSolutionKey<'_> {
    fn eq(&self, other: &Self) -> bool { self.coordinates == other.coordinates }
}
impl Eq for BoreSolutionKey<'_> {}
impl PartialOrd for BoreSolutionKey<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl Ord for BoreSolutionKey<'_> {
    fn cmp(&self, other: &Self) -> Ordering { self.coordinates.cmp(&other.coordinates) }
}
impl DecodeCost for BoreSolutionKey<'_> {
    fn decode_cost(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<u64, CodecError> {
        self.coordinates.decode_cost(ctx, operation)
    }
}

pub(super) struct BoreSolution<'ctx> {
    placements: Vec<HolePlacement>,
    storage: ScopedReservation<'ctx>,
}
impl BoreSolution<'_> {
    pub(super) fn into_output(self) -> Result<Vec<HolePlacement>, CodecError> {
        self.storage.commit_value(self.placements)
    }
}

/// Source keys survive replacement independently of their placement values.
/// The table shell survives its consuming iterator on success and early Err.
pub(super) struct BoreSolutions<'ctx, 'arena> {
    context: &'ctx DecodeContext<'arena>,
    entries: BTreeMap<BoreSolutionKey<'ctx>, BoreSolution<'ctx>>,
    storage: ScopedReservation<'ctx>,
}
impl<'ctx, 'arena> BoreSolutions<'ctx, 'arena> {
    pub(super) fn new(context: &'ctx DecodeContext<'arena>) -> Result<Self, CodecError> {
        Ok(Self { context, entries: BTreeMap::new(), storage: context.reserve_scoped(0, RETAIN)? })
    }

    pub(super) fn retain(
        &mut self,
        candidates: &[Candidate],
        indices: impl Iterator<Item = usize>,
    ) -> Result<bool, CodecError> {
        let ctx = self.context;
        let mut key_storage = ctx.reserve_scoped(0, RETAIN)?;
        let mut placement_storage = ctx.reserve_scoped(0, OUTPUT)?;
        let mut coordinates = Vec::new();
        let mut placements = Vec::new();
        let quantize = |value| GridCoordinate::new(value, EPS_HOLE_POSITION);
        for index in indices {
            let (_, origin, axis) = candidates[index];
            placement_storage.with_storage(|| ctx.reserve_vec(&mut placements, 1, RETAIN))?;
            key_storage.with_storage(|| ctx.reserve_vec(&mut coordinates, 1, RETAIN))?;
            placements.push(HolePlacement::Axis { origin, axis });
            coordinates.push([
                quantize(origin.x), quantize(origin.y), quantize(origin.z),
                quantize(axis.x), quantize(axis.y), quantize(axis.z),
            ]);
        }
        let key = BoreSolutionKey { coordinates, _storage: key_storage };
        let value = BoreSolution { placements, storage: placement_storage };
        self.storage.with_storage(|| ctx.insert_btree_map(&mut self.entries, key, value, RETAIN))?;
        Ok(self.entries.len() > 1)
    }

    pub(super) fn into_single(self) -> Result<Option<BoreSolution<'ctx>>, CodecError> {
        if self.entries.len() != 1 { return Ok(None); }
        let ctx = self.context;
        let storage = self.storage;
        let mut source = self.entries.into_iter();
        let Some((key, value)) = ctx.next_charged(&mut source, OUTPUT)? else { return Ok(None); };
        drop(source);
        drop(storage);
        drop(key);
        Ok(Some(value))
    }
}

#[cfg(test)]
mod tests;
