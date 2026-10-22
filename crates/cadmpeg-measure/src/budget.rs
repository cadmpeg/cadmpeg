// SPDX-License-Identifier: Apache-2.0
//! The work allowance of one measurement pass and the slices each item draws
//! from it.
//!
//! One `measure` call owns one pass allowance that scales with the document:
//! `max(MEASURE_PASS_WORK_FLOOR, k × (faces + edges))`. Each face and each
//! edge draws a capped slice that charges the pass as it works. An item whose
//! slice runs out reports its remaining quantities as
//! `evaluation.work-exhausted`; the measurement continues.

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
pub const MAX_MEASURE_PASS_WORK_PER_ITEM: usize = 1 << 22;

/// Pass allowance per face and edge of the document under the service
/// profile.
pub const MAX_MEASURE_PASS_WORK_PER_ITEM_SERVICE: usize = 1 << 18;

/// One face slice: traversal, resolver, regions, quadrature, extents, box and
/// flux of one face.
const MAX_FACE_MEASURE_WORK: usize = 1 << 26;

/// One edge slice: length and box of one edge.
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
}

/// The work allowance of one measurement pass.
pub(crate) struct PassAllowance {
    allowance: usize,
    work: WorkBudget,
    starved: Cell<bool>,
}

impl PassAllowance {
    /// The pass allowance of `allowance` work units.
    pub(crate) fn new(allowance: usize) -> Self {
        Self {
            allowance,
            work: WorkBudget::new(allowance),
            starved: Cell::new(false),
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

    /// Whether an item ran out because the pass could not pay its slice.
    pub(crate) fn exhausted(&self) -> bool {
        self.starved.get()
    }

    pub(crate) fn face_slice(&self) -> ItemBudget<'_> {
        self.slice(MAX_FACE_MEASURE_WORK)
    }

    pub(crate) fn edge_slice(&self) -> ItemBudget<'_> {
        self.slice(MAX_EDGE_MEASURE_WORK)
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
    /// The item's work slice, for an evaluator that charges it directly.
    pub(crate) const fn slice(&self) -> &WorkBudget {
        &self.work
    }

    /// Charge `units` of work, or report the item's slice exhausted.
    pub(crate) fn charge(&self, units: usize) -> Measure<()> {
        if self.work.charge_by(units) {
            Ok(())
        } else {
            Err(self.exhausted())
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

    fn exhausted(&self) -> MeasureFailure {
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
