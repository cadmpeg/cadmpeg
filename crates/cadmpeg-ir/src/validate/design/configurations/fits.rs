// SPDX-License-Identifier: Apache-2.0
//! A design parameter's fit (part 10, §5.12): its `mate` has the other `zone`
//! than its `fit`.

use crate::pmi::LimitsAndFits;
use crate::report::check::Finding;

pub(super) fn check_mate_zone(
    parameter: &str,
    fit: &LimitsAndFits,
    mate: Option<&LimitsAndFits>,
    findings: &mut Vec<Finding>,
) {
    if mate.is_some_and(|mate| mate.zone == fit.zone) {
        super::super::error(
            findings,
            parameter,
            crate::rule!(
                "design.parameter-fit-mate-state-same-zone",
                "parameter's fit and its mate state the same zone"
            ),
        );
    }
}

#[cfg(test)]
mod tests;
