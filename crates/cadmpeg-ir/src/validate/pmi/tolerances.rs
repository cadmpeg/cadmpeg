// SPDX-License-Identifier: Apache-2.0
//! A PMI dimension's tolerance (part 10, §5.12, §8): a `Deviations` has its
//! `lower` below its `upper`, in `Tolerance::Deviations` and in a fit's
//! `deviations`, a fit's `mate` has the other `zone` than its `fit`, and
//! what the dimension states goes with its quantity and nominal.

use super::error;
use super::nominals::{nominal_quantity, BoundParameters};
use crate::design::dimension::Dimension;
use crate::design::parameter::{Deviations, FitDeviations, LimitSide, Tolerance};
use crate::pmi::{LimitsAndFits, PmiDimension, PmiQuantity};
use crate::report::check::Finding;
use crate::report::check::Ruled;
use crate::scalar::FiniteReal;

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
                    crate::rule!(
                        "pmi.dimension-fit-mate-state-same-zone",
                        "dimension's fit and its mate state the same zone"
                    ),
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
                crate::rule!(
                    "pmi.dimension-lower-deviation-not-below-upper",
                    "dimension's lower deviation {} is not below its upper deviation {}",
                    lower.get(),
                    upper.get()
                ),
            );
        }
    }
}

/// What a PMI dimension states beside its nominal (part 10, §8): a fit's
/// designations are capital letters and a grade; a tolerance goes with a
/// length or angle, and a fit with a length; `Basic` and `SingleLimit` go
/// with a stated nominal, and `decimals` with a length or angle nominal; and a tolerance, compared in the dimension's sign,
/// or `decimals` equal to the bound parameter's is a second spelling of it
/// (D60, D61).
pub(super) fn check_dimension_statements(
    parameters: &BoundParameters<'_>,
    annotation: &str,
    dimension: &PmiDimension,
    findings: &mut Vec<Finding>,
) {
    let nominal = nominal_quantity(parameters, dimension);
    let quantity = dimension.dimension.quantity().or(nominal);
    if let Some(tolerance) = &dimension.tolerance {
        if quantity.is_some_and(|quantity| quantity == PmiQuantity::Ratio) {
            error(
                findings,
                annotation,
                crate::rule!(
                    "pmi.dimension-states-tolerance-quantity-no-length",
                    "dimension states a tolerance, but its quantity is no length or angle"
                ),
            );
        }
        match tolerance {
            Tolerance::Fit { fit, mate, .. } => {
                if quantity.is_some_and(|quantity| quantity != PmiQuantity::Length) {
                    error(
                        findings,
                        annotation,
                        crate::rule!(
                            "pmi.dimension-states-fit-quantity-no-length",
                            "dimension states a fit, but its quantity is no length"
                        ),
                    );
                }
                for (what, designation) in
                    std::iter::once(("fit", fit)).chain(mate.iter().map(|mate| ("mate", mate)))
                {
                    check_designation(annotation, what, designation, findings);
                }
            }
            Tolerance::Basic | Tolerance::SingleLimit(_) if dimension.nominal.is_none() => error(
                findings,
                annotation,
                crate::rule!(
                    "pmi.dimension-states-basic-single-limit-tolerance",
                    "dimension states a basic or single-limit tolerance and no nominal"
                ),
            ),
            Tolerance::Deviations(_)
            | Tolerance::General
            | Tolerance::Basic
            | Tolerance::SingleLimit(_) => {}
        }
    }
    if dimension.decimals.is_some()
        && (dimension.nominal.is_none() || nominal == Some(PmiQuantity::Ratio))
    {
        error(
            findings,
            annotation,
            crate::rule!(
                "pmi.dimension-states-decimals-no-nominal-length",
                "dimension states `decimals` and no nominal with a length or angle quantity"
            ),
        );
    }
    let bound = match &dimension.nominal {
        Some(Dimension::Parameter(id)) => parameters.design(id.as_str()).map(|p| (p, false)),
        Some(Dimension::Negated(id)) => parameters.design(id.as_str()).map(|p| (p, true)),
        Some(Dimension::Literal(_)) | None => None,
    };
    if let Some((parameter, negated)) = bound {
        let in_dimension_sign = if negated {
            parameter.tolerance.as_ref().and_then(mirrored)
        } else {
            parameter.tolerance.clone()
        };
        if in_dimension_sign.is_some() && dimension.tolerance == in_dimension_sign {
            warning(
                findings,
                annotation,
                crate::rule!(
                    "pmi.dimension-restates-tolerance-parameter-nominal-binds",
                    "dimension restates the tolerance of the parameter its nominal binds, {} (D61)",
                    parameter.id.as_str()
                ),
            );
        }
        if parameter.decimals.is_some() && dimension.decimals == parameter.decimals {
            warning(
                findings,
                annotation,
                crate::rule!(
                    "pmi.dimension-restates-decimals-parameter-nominal-binds",
                    "dimension restates the decimals of the parameter its nominal binds, {} (D60)",
                    parameter.id.as_str()
                ),
            );
        }
    }
}

/// A tolerance in the opposite sign: its lower and upper exchanged and
/// negated (part 10, D61). A fit keeps its designations.
fn mirrored(tolerance: &Tolerance) -> Option<Tolerance> {
    let flip = |deviations: &Deviations| {
        Some(Deviations {
            lower: FiniteReal::new(-deviations.upper.get())?,
            upper: FiniteReal::new(-deviations.lower.get())?,
        })
    };
    Some(match tolerance {
        Tolerance::Deviations(deviations) => Tolerance::Deviations(flip(deviations)?),
        Tolerance::Fit {
            fit,
            mate,
            deviations,
        } => Tolerance::Fit {
            fit: fit.clone(),
            mate: mate.clone(),
            deviations: match deviations {
                Some(FitDeviations::Cell(stated)) => Some(FitDeviations::Cell(flip(stated)?)),
                Some(FitDeviations::Own(stated)) => Some(FitDeviations::Own(flip(stated)?)),
                Some(FitDeviations::Unchecked(stated)) => {
                    Some(FitDeviations::Unchecked(flip(stated)?))
                }
                None => None,
            },
        },
        Tolerance::SingleLimit(LimitSide::Minimum) => Tolerance::SingleLimit(LimitSide::Maximum),
        Tolerance::SingleLimit(LimitSide::Maximum) => Tolerance::SingleLimit(LimitSide::Minimum),
        Tolerance::General | Tolerance::Basic => tolerance.clone(),
    })
}

/// A designation's `form_variance` holds capital letters only, and its
/// `grade` is not empty.
fn check_designation(
    annotation: &str,
    what: &str,
    designation: &LimitsAndFits,
    findings: &mut Vec<Finding>,
) {
    let letters = &designation.form_variance;
    if letters.is_empty() || !letters.chars().all(|letter| letter.is_ascii_uppercase()) {
        error(
            findings,
            annotation,
            crate::rule!(
                "pmi.dimension-designation-not-capital-letters-only",
                "dimension's {what} designation {letters:?} is not capital letters only"
            ),
        );
    }
    if designation.grade.is_empty() {
        error(
            findings,
            annotation,
            crate::rule!(
                "pmi.dimension-designation-states-empty-grade",
                "dimension's {what} designation states an empty grade"
            ),
        );
    }
}

fn warning(findings: &mut Vec<Finding>, annotation: &str, message: Ruled<'_>) {
    crate::validate::record_finding(
        findings,
        crate::report::check::Check::Pmi,
        crate::report::Severity::Warning,
        Some(annotation),
        message,
    );
}
