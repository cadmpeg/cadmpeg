// SPDX-License-Identifier: Apache-2.0
//! The work allowance of one measurement pass and the slices each item draws
//! from it.
//!
//! One `measure` call owns one pass allowance that scales with the document:
//! `max(MEASURE_PASS_WORK_FLOOR, k × (faces + edges))`. Each face and each
//! edge draws a capped slice that charges the pass as it works. An item whose
//! slice runs out reports its remaining quantities as
//! `evaluation.work-exhausted`; the measurement continues.
//!
//! The check findings pass draws the same allowance and slices. An item
//! whose slice runs out is not checked.

use std::cell::Cell;

use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::EvaluationFailure;

use crate::failure::{Measure, MeasureFailure};
use crate::reason::UnavailableReason;

/// The smallest pass allowance: one full face slice.
pub const MEASURE_PASS_WORK_FLOOR: usize = 1 << 26;

/// Pass allowance per face and edge of the document under the desktop
/// profile.
// resource-cap: MAX_MEASURE_PASS_WORK_PER_ITEM = 4
pub const MAX_MEASURE_PASS_WORK_PER_ITEM: usize = 1 << 22;

/// Pass allowance per face and edge of the document under the service
/// profile.
// resource-cap: MAX_MEASURE_PASS_WORK_PER_ITEM_SERVICE = 4
pub const MAX_MEASURE_PASS_WORK_PER_ITEM_SERVICE: usize = 1 << 18;

/// The smallest pair allowance of an ID-free comparison, in body pair
/// evaluations.
const COMPARE_PAIR_WORK_FLOOR: usize = 1 << 20;

/// Pair evaluations of an ID-free comparison per body of both documents
/// under the desktop profile.
// resource-cap: MAX_COMPARE_PAIR_WORK_PER_BODY = N
const MAX_COMPARE_PAIR_WORK_PER_BODY: usize = 1 << 12;

/// Pair evaluations of an ID-free comparison per body of both documents
/// under the service profile.
// resource-cap: MAX_COMPARE_PAIR_WORK_PER_BODY_SERVICE = N
const MAX_COMPARE_PAIR_WORK_PER_BODY_SERVICE: usize = 1 << 8;

/// One face slice: traversal, resolver, regions, quadrature, extents, box and
/// flux of one face.
// resource-cap: MAX_FACE_MEASURE_WORK = 4
const MAX_FACE_MEASURE_WORK: usize = 1 << 26;

/// One edge slice: length and box of one edge.
// resource-cap: MAX_EDGE_MEASURE_WORK = 4
const MAX_EDGE_MEASURE_WORK: usize = 1 << 22;

/// The work profile of a measurement pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkProfile {
    /// Generous allowance for interactive use.
    #[default]
    Desktop,
    /// Tight allowance for unattended use.
    Service,
}

impl WorkProfile {
    /// The pass allowance for a document of `items` faces and edges, absent
    /// when it does not fit a `usize`.
    #[must_use]
    pub fn pass_allowance(self, items: usize) -> Option<usize> {
        let per_item = match self {
            Self::Desktop => MAX_MEASURE_PASS_WORK_PER_ITEM,
            Self::Service => MAX_MEASURE_PASS_WORK_PER_ITEM_SERVICE,
        };
        items
            .checked_mul(per_item)
            .map(|scaled| scaled.max(MEASURE_PASS_WORK_FLOOR))
    }

    /// The pair allowance of an ID-free comparison of `bodies` bodies of
    /// both documents, absent when it does not fit a `usize`.
    #[must_use]
    pub fn pair_allowance(self, bodies: usize) -> Option<usize> {
        let per_body = match self {
            Self::Desktop => MAX_COMPARE_PAIR_WORK_PER_BODY,
            Self::Service => MAX_COMPARE_PAIR_WORK_PER_BODY_SERVICE,
        };
        bodies
            .checked_mul(per_body)
            .map(|scaled| scaled.max(COMPARE_PAIR_WORK_FLOOR))
    }
}

/// The work allowance of one measurement pass.
pub(crate) struct PassAllowance {
    allowance: usize,
    work: WorkBudget,
    starved: Cell<bool>,
    face_limit: usize,
    edge_limit: usize,
}

impl PassAllowance {
    /// The measurement pass allowance of `allowance` work units.
    pub(crate) fn new(allowance: usize) -> Self {
        Self {
            allowance,
            work: WorkBudget::new(allowance),
            starved: Cell::new(false),
            face_limit: MAX_FACE_MEASURE_WORK,
            edge_limit: MAX_EDGE_MEASURE_WORK,
        }
    }

    /// The allowance in work units.
    pub(crate) const fn allowance(&self) -> usize {
        self.allowance
    }

    /// Work units the pass has used.
    pub(crate) fn used(&self) -> usize {
        self.work.consumed()
    }

    /// Work units still available in this pass.
    pub(crate) fn remaining(&self) -> usize {
        self.work.remaining()
    }

    /// Per-face slice cap of this pass.
    pub(crate) const fn face_limit(&self) -> usize {
        self.face_limit
    }

    /// Per-edge slice cap of this pass.
    pub(crate) const fn edge_limit(&self) -> usize {
        self.edge_limit
    }

    /// Build a standalone pass allowance with explicit item caps, used inside
    /// parallel worker threads before committing total work back to the parent pass.
    pub(crate) fn with_limits(allowance: usize, face_limit: usize, edge_limit: usize) -> Self {
        Self {
            allowance,
            work: WorkBudget::new(allowance),
            starved: Cell::new(false),
            face_limit,
            edge_limit,
        }
    }

    /// Charge work already executed by a parallel chunk against this pass.
    pub(crate) fn charge_committed(&self, units: usize) -> bool {
        self.work.charge_by(units)
    }

    /// Whether an item ran out because the pass could not pay its slice.
    pub(crate) fn exhausted(&self) -> bool {
        self.starved.get()
    }

    pub(crate) fn face_slice(&self) -> ItemBudget<'_> {
        self.slice(self.face_limit)
    }

    pub(crate) fn edge_slice(&self) -> ItemBudget<'_> {
        self.slice(self.edge_limit)
    }

    fn slice(&self, limit: usize) -> ItemBudget<'_> {
        ItemBudget {
            work: self.work.charging_slice(limit),
            truncated: self.work.remaining() < limit,
            pass: self,
        }
    }
}

/// The capped work slice of one face or edge.
pub(crate) struct ItemBudget<'pass> {
    work: WorkBudget,
    truncated: bool,
    pass: &'pass PassAllowance,
}

impl ItemBudget<'_> {
    /// A local allowance charged to this item.
    pub(crate) fn local_slice(&self, limit: usize) -> ItemBudget<'_> {
        ItemBudget {
            work: self.work.charging_slice(limit),
            truncated: self.truncated && self.work.remaining() < limit,
            pass: self.pass,
        }
    }

    /// The item's work slice, for an evaluator that charges it directly.
    pub(crate) const fn slice(&self) -> &WorkBudget {
        &self.work
    }

    /// Work units a charge can still use.
    pub(crate) fn remaining(&self) -> usize {
        self.work.remaining()
    }

    /// Whether the slice refused a charge.
    pub(crate) fn exhausted(&self) -> bool {
        self.work.exhausted()
    }

    /// Charge `units` of work, or report the item's slice exhausted.
    pub(crate) fn charge(&self, units: usize) -> Measure<()> {
        if self.work.charge_by(units) {
            Ok(())
        } else {
            Err(self.exhaustion())
        }
    }

    /// Run one evaluator call under the item slice. A session refusal behind
    /// it propagates.
    pub(crate) fn evaluate<T, R>(
        &self,
        ctx: &DecodeContext<'_>,
        run: impl for<'slice> FnOnce(
            EvaluationAdmission<'slice, 'slice>,
        ) -> Result<T, EvaluationFailure<R>>,
    ) -> Measure<T> {
        let result = EvaluationAdmission::Decode(ctx).within_work_slice(&self.work, run);
        if self.work.exhausted() {
            self.note_exhaustion();
        }
        result.map_err(MeasureFailure::from)
    }

    fn note_exhaustion(&self) {
        if self.truncated {
            self.pass.starved.set(true);
        }
    }

    /// The failure of a step that the item's slice stopped.
    pub(crate) fn exhaustion(&self) -> MeasureFailure {
        self.note_exhaustion();
        MeasureFailure::unavailable(
            UnavailableReason::EvaluationWorkExhausted,
            None,
            "the item's work slice ran out",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PassAllowance, WorkProfile, MAX_FACE_MEASURE_WORK, MAX_MEASURE_PASS_WORK_PER_ITEM,
        MAX_MEASURE_PASS_WORK_PER_ITEM_SERVICE, MEASURE_PASS_WORK_FLOOR,
    };
    use crate::failure::MeasureFailure;
    use crate::reason::UnavailableReason;

    #[test]
    fn pass_allowance_scales_with_items_above_the_floor() {
        for (profile, per_item) in [
            (WorkProfile::Desktop, MAX_MEASURE_PASS_WORK_PER_ITEM),
            (WorkProfile::Service, MAX_MEASURE_PASS_WORK_PER_ITEM_SERVICE),
        ] {
            assert_eq!(profile.pass_allowance(0), Some(MEASURE_PASS_WORK_FLOOR));
            assert_eq!(
                profile.pass_allowance(3),
                Some(MEASURE_PASS_WORK_FLOOR.max(3 * per_item))
            );
            assert_eq!(profile.pass_allowance(1 << 20), Some((1 << 20) * per_item));
        }
    }

    #[test]
    fn pair_allowance_scales_with_bodies_above_the_floor() {
        use super::{
            COMPARE_PAIR_WORK_FLOOR, MAX_COMPARE_PAIR_WORK_PER_BODY,
            MAX_COMPARE_PAIR_WORK_PER_BODY_SERVICE,
        };
        for (profile, per_body) in [
            (WorkProfile::Desktop, MAX_COMPARE_PAIR_WORK_PER_BODY),
            (WorkProfile::Service, MAX_COMPARE_PAIR_WORK_PER_BODY_SERVICE),
        ] {
            assert_eq!(profile.pair_allowance(2), Some(COMPARE_PAIR_WORK_FLOOR));
            assert_eq!(profile.pair_allowance(1 << 16), Some((1 << 16) * per_body));
            assert_eq!(profile.pair_allowance(usize::MAX), None);
        }
    }

    #[test]
    fn a_local_slice_charges_the_item_and_preserves_its_remaining_work() {
        let pass = PassAllowance::new(WorkProfile::Desktop.pass_allowance(1).unwrap());
        let face = pass.face_slice();
        let local = face.local_slice(10);
        local.charge(5).unwrap();
        assert!(local.charge(6).is_err());
        assert_eq!(pass.used(), 5);
        face.charge(10).unwrap();
        assert!(!face.exhausted());
        assert!(!pass.exhausted());
    }

    #[test]
    fn a_face_slice_charges_the_pass() {
        let pass = PassAllowance::new(WorkProfile::Desktop.pass_allowance(1).unwrap());
        let face = pass.face_slice();
        face.charge(1000).unwrap();
        assert_eq!(pass.used(), 1000);
        assert!(!pass.exhausted());
    }

    #[test]
    fn an_empty_pass_gives_work_exhausted() {
        let pass = PassAllowance::new(WorkProfile::Desktop.pass_allowance(0).unwrap());
        let first = pass.face_slice();
        first.charge(MAX_FACE_MEASURE_WORK).unwrap();
        let second = pass.face_slice();
        match second.charge(1) {
            Err(MeasureFailure::Unavailable { reason, .. }) => {
                assert_eq!(reason, UnavailableReason::EvaluationWorkExhausted);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(pass.exhausted());
    }
}
