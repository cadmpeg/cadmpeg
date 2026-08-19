// SPDX-License-Identifier: Apache-2.0
//! Document report for pcurve relations that the finite witness admits.
//!
//! The B-rep walker collects the located relations as facts. This module owns
//! the presentation policy: how many relations the one document warning names,
//! and the text of that warning.

use cadmpeg_ir::report::LossNote;

use crate::loss::StepLossCode;

/// Number of admitted relations that the document admission warning names.
const PCURVE_UNPROVED_NOTE_EXEMPLARS: usize = 8;

/// One admitted pcurve relation, by the three source records that locate it.
pub(super) struct PcurveAdmission {
    pub(super) curve: u64,
    pub(super) surface: u64,
    pub(super) coedge_use: u64,
}

/// The one document warning, or `None` when no relation is admitted.
///
/// Every admitted relation shares one class of unproved invariant, so the
/// warning gives the count, names the first [`PCURVE_UNPROVED_NOTE_EXEMPLARS`]
/// relations in decode order, and gives the number it does not name.
pub(super) fn pcurve_admission_note(admissions: &[PcurveAdmission]) -> Option<LossNote> {
    if admissions.is_empty() {
        return None;
    }
    let count = admissions.len();
    let named = admissions
        .iter()
        .take(PCURVE_UNPROVED_NOTE_EXEMPLARS)
        .map(|admission| {
            format!(
                "curve #{} on surface #{} at coedge use #{}",
                admission.curve, admission.surface, admission.coedge_use
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let unnamed = count.saturating_sub(PCURVE_UNPROVED_NOTE_EXEMPLARS);
    let more = if unnamed == 0 {
        String::new()
    } else {
        format!(", and {unnamed} more")
    };
    Some(StepLossCode::PcurveGlobalFidelityUnproved.note(format!(
        "a finite endpoint and locus witness admits {count} pcurve relation(s); global model-space point-set equality and direction are unproved: {named}{more}"
    )))
}

#[cfg(test)]
mod tests;
