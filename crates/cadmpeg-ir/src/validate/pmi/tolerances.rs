// SPDX-License-Identifier: Apache-2.0
//! A PMI dimension's tolerance (part 10, §5.12, §8): a `Deviations` has its
//! `lower` below its `upper`, in `Tolerance::Deviations` and in a fit's
//! `deviations`, and a fit's `mate` has the other `zone` than its `fit`.

use super::error;
use crate::design::parameter::{Deviations, FitDeviations, Tolerance};
use crate::pmi::PmiDimension;
use crate::report::check::Finding;

pub(super) fn check_dimension_tolerance(
    annotation: &str,
    dimension: &PmiDimension,
    findings: &mut Vec<Finding>,
) {
    let Some(tolerance) = &dimension.tolerance else {
        return;
    };
    let deviations = match tolerance {
        Tolerance::Deviations(deviations) => Some(deviations),
        Tolerance::Fit {
            fit,
            mate,
            deviations,
        } => {
            if mate.as_ref().is_some_and(|mate| mate.zone == fit.zone) {
                error(
                    findings,
                    annotation,
                    format_args!("dimension's fit and its mate state the same zone"),
                );
            }
            deviations.as_ref().map(|deviations| match deviations {
                FitDeviations::Cell(deviations)
                | FitDeviations::Own(deviations)
                | FitDeviations::Unchecked(deviations) => deviations,
            })
        }
        Tolerance::General | Tolerance::Basic | Tolerance::SingleLimit(_) => None,
    };
    if let Some(Deviations { lower, upper }) = deviations {
        if lower.get() >= upper.get() {
            error(
                findings,
                annotation,
                format_args!(
                    "dimension's lower deviation {} is not below its upper deviation {}",
                    lower.get(),
                    upper.get()
                ),
            );
        }
    }
}
