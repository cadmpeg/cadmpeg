// SPDX-License-Identifier: Apache-2.0
//! The session context, model index and work slice one item measures under.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::index::ModelIndex;

use crate::budget::ItemBudget;
use crate::failure::Measure;

/// What a measurement computes for each face and edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Work {
    /// Every quantity of a row.
    Row,
    /// What orientation findings read: boundaries, regions and orientation
    /// readings, edge lengths, and, for a face that needs fluxes, its area
    /// readings and fluxes. Carrier parameters, boxes and extents are not
    /// computed, and the rows are never reported.
    Orientation,
}

/// What one item's evaluator calls run under.
#[derive(Clone, Copy)]
pub(crate) struct Eval<'a, 'ir> {
    pub(crate) ctx: &'a DecodeContext<'a>,
    pub(crate) index: &'a ModelIndex<'ir>,
    pub(crate) work: &'a ItemBudget<'a>,
}

impl Eval<'_, '_> {
    /// Charge `units` of work to the item slice.
    pub(crate) fn charge(&self, units: usize) -> Measure<()> {
        self.work.charge(units)
    }
}
