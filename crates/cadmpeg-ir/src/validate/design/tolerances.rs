// SPDX-License-Identifier: Apache-2.0
//! The limits-and-fits designations of a design parameter's tolerance
//! (§5.12, §8): the fit's and the mate's.

use super::records::DesignRecords;
use crate::design::parameter::Tolerance;
use crate::pmi::LimitsAndFits;
use crate::report::check::Finding;

pub(super) fn check_tolerances(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    for parameter in &index.ir.design.parameters {
        if let Some(Tolerance::Fit { fit, mate, .. }) = &parameter.tolerance {
            let id = parameter.id.as_str();
            check_designation(id, "fit", fit, findings);
            if let Some(mate) = mate {
                check_designation(id, "mate", mate, findings);
            }
        }
    }
}

/// A designation's `form_variance` holds capital letters only, and its
/// `grade` is not empty.
fn check_designation(
    entity: &str,
    what: &str,
    designation: &LimitsAndFits,
    findings: &mut Vec<Finding>,
) {
    let letters = &designation.form_variance;
    if letters.is_empty() || !letters.chars().all(|letter| letter.is_ascii_uppercase()) {
        super::error(
            findings,
            entity,
            format_args!("parameter's {what} designation {letters:?} is not capital letters only"),
        );
    }
    if designation.grade.is_empty() {
        super::error(
            findings,
            entity,
            format_args!("parameter's {what} designation states an empty grade"),
        );
    }
}
