// SPDX-License-Identifier: Apache-2.0
//! A design parameter's fit (part 10, §5.12): its `mate` has the other `zone`
//! than its `fit`.

use crate::design::parameter::Tolerance;
use crate::document::CadIr;
use crate::report::check::Finding;

pub(super) fn check_fits(ir: &CadIr, findings: &mut Vec<Finding>) {
    for parameter in &ir.design.parameters {
        let Some(Tolerance::Fit {
            fit,
            mate: Some(mate),
            ..
        }) = &parameter.tolerance
        else {
            continue;
        };
        if mate.zone == fit.zone {
            super::error(
                findings,
                parameter.id.as_str(),
                format_args!("parameter's fit and its mate state the same zone"),
            );
        }
    }
}

#[cfg(test)]
mod tests;
