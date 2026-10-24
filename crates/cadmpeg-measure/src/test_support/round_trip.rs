// SPDX-License-Identifier: Apache-2.0
//! Measured geometry of a writer round trip, compared without entity
//! identities.
//!
//! A writer round trip keeps its geometry when the source document and the
//! re-decoded document measure the same: [`crate::compare::compare`]
//! pairs every body of one with a body of the other and every compared
//! quantity agrees within its tolerance.
//!
//! Writer crates take this module through the `test-support` feature in
//! their dev-dependencies. Each function returns a [`RoundTripError`] for
//! the caller to `expect`. Its text states only the caller's label, fixed
//! phrases, the quantity names and the cause codes that this crate defines.
//! It never prints a value or an identifier that a decoded model supplies.

use std::collections::BTreeSet;
use std::fmt;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::CadIr;

use crate::compare::{compare, CompareOptions, Comparison, Row, Verdict};
use crate::report::MeasureReport;
use crate::{MeasureInput, MeasureOptions, Measurer, Selection};

/// Why a writer round trip does not show that it keeps its measured
/// geometry. `Display` and `Debug` print the same fixed text.
#[derive(Clone, PartialEq, Eq)]
pub struct RoundTripError {
    label: String,
    failure: RoundTripFailure,
}

/// The step of a round-trip measurement or comparison that fails.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RoundTripFailure {
    /// The decode policy refuses the measurement context.
    ContextRefused,
    /// The measurement pass fails.
    MeasurementFailed,
    /// The measurement session ends over its budget.
    SessionRefused,
    /// The comparison exceeds its pair allowance.
    PairAllowanceExceeded,
    /// The comparison verdict is not [`Verdict::Agree`]. The text names the
    /// disagreeing quantities, the sides with an unpaired or unmeasured body
    /// and the inconclusive cause codes.
    Disagrees(String),
    /// The comparison agrees but pairs no body.
    NoBodyPair,
}

impl RoundTripError {
    fn new(label: &str, failure: RoundTripFailure) -> Self {
        Self {
            label: label.to_owned(),
            failure,
        }
    }

    /// The failing step.
    #[must_use]
    pub const fn failure(&self) -> &RoundTripFailure {
        &self.failure
    }
}

impl fmt::Display for RoundTripError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = &self.label;
        match &self.failure {
            RoundTripFailure::ContextRefused => {
                write!(f, "{label}: the measurement context is refused")
            }
            RoundTripFailure::MeasurementFailed => {
                write!(f, "{label}: the measurement pass fails")
            }
            RoundTripFailure::SessionRefused => {
                write!(f, "{label}: the measurement session is refused")
            }
            RoundTripFailure::PairAllowanceExceeded => {
                write!(f, "{label}: the comparison exceeds its pair allowance")
            }
            RoundTripFailure::Disagrees(detail) => write!(
                f,
                "{label}: the round trip does not keep its measured geometry; {detail}"
            ),
            RoundTripFailure::NoBodyPair => write!(
                f,
                "{label}: the comparison pairs no body, so it checks no geometry"
            ),
        }
    }
}

impl fmt::Debug for RoundTripError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for RoundTripError {}

/// Measures every body, face and edge of `ir` in model space under the
/// service policy.
///
/// # Errors
///
/// Returns [`RoundTripError`] when the measurement pass fails or its
/// session is refused.
pub fn measured(label: &str, ir: &CadIr) -> Result<MeasureReport, RoundTripError> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0)
        .map_err(|_| RoundTripError::new(label, RoundTripFailure::ContextRefused))?;
    let input = MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let report = Measurer::new(input, MeasureOptions::default())
        .measure(&ctx, &Selection::default())
        .map_err(|_| RoundTripError::new(label, RoundTripFailure::MeasurementFailed))?;
    ctx.finish_session()
        .map_err(|_| RoundTripError::new(label, RoundTripFailure::SessionRefused))?;
    Ok(report)
}

/// `before` and `after` compared without entity identities.
fn compared(
    label: &str,
    before: &CadIr,
    before_report: &MeasureReport,
    after: &CadIr,
    after_report: &MeasureReport,
) -> Result<Comparison, RoundTripError> {
    compare(
        before,
        before_report,
        after,
        after_report,
        &CompareOptions::default(),
    )
    .map_err(|_| RoundTripError::new(label, RoundTripFailure::PairAllowanceExceeded))
}

/// Measures `before` and `after` and compares them without entity
/// identities. The round trip keeps its measured geometry when the verdict
/// is [`Verdict::Agree`] over at least one body pair.
///
/// Returns the comparison and both reports.
///
/// # Errors
///
/// Returns [`RoundTripError`] when a measurement fails, when the comparison
/// exceeds its pair allowance, when its verdict is not [`Verdict::Agree`]
/// ([`RoundTripFailure::Disagrees`]), or when it pairs no body.
pub fn measures_agree(
    label: &str,
    before: &CadIr,
    after: &CadIr,
) -> Result<(Comparison, MeasureReport, MeasureReport), RoundTripError> {
    let before_report = measured(label, before)?;
    let after_report = measured(label, after)?;
    let comparison = compared(label, before, &before_report, after, &after_report)?;
    if comparison.verdict() != Verdict::Agree {
        return Err(RoundTripError::new(
            label,
            RoundTripFailure::Disagrees(disagreements(&comparison)),
        ));
    }
    if comparison.pairs().is_empty() {
        return Err(RoundTripError::new(label, RoundTripFailure::NoBodyPair));
    }
    Ok((comparison, before_report, after_report))
}

/// Whether `ir` compared with itself gives [`Verdict::Agree`] over at least
/// one body pair.
///
/// A document whose bodies measure no comparable quantity, or whose
/// occurrences place its bodies, leaves every comparison inconclusive. A
/// round-trip test selects its fixtures with this check before it calls
/// [`measures_agree`].
///
/// # Errors
///
/// Returns [`RoundTripError`] when the measurement fails or the comparison
/// exceeds its pair allowance.
pub fn measures_decide(label: &str, ir: &CadIr) -> Result<bool, RoundTripError> {
    let report = measured(label, ir)?;
    let comparison = compared(label, ir, &report, ir, &report)?;
    Ok(comparison.verdict() == Verdict::Agree && !comparison.pairs().is_empty())
}

/// The quantity names, sides and causes that keep `comparison` from
/// agreeing, as one line.
fn disagreements(comparison: &Comparison) -> String {
    let disagreeing = |rows: &[Row]| -> Vec<String> {
        rows.iter()
            .filter(|row| row.agrees() == Some(false))
            .map(|row| row.quantity().to_owned())
            .collect()
    };
    let mut quantities: BTreeSet<String> = disagreeing(comparison.document())
        .into_iter()
        .map(|name| format!("document.{name}"))
        .collect();
    for pair in comparison.pairs() {
        quantities.extend(disagreeing(pair.quantities()));
    }
    let mut parts = Vec::new();
    if !quantities.is_empty() {
        let names: Vec<String> = quantities.into_iter().collect();
        parts.push(format!("disagreeing quantities: {}", names.join(", ")));
    }
    for (empty, text) in [
        (
            comparison.unmatched_a().is_empty(),
            "a source body has no partner",
        ),
        (
            comparison.unmatched_b().is_empty(),
            "a round-trip body has no partner",
        ),
        (
            comparison.unmeasured_a().is_empty(),
            "a source body has no comparable quantity",
        ),
        (
            comparison.unmeasured_b().is_empty(),
            "a round-trip body has no comparable quantity",
        ),
        (
            comparison.ambiguous().is_empty(),
            "a body class is ambiguous",
        ),
    ] {
        if !empty {
            parts.push(text.to_owned());
        }
    }
    if !comparison.inconclusive_causes().is_empty() {
        let causes: Vec<&str> = comparison
            .inconclusive_causes()
            .iter()
            .map(|cause| cause.code())
            .collect();
        parts.push(format!("inconclusive causes: {}", causes.join(", ")));
    }
    if parts.is_empty() {
        "no disagreeing quantity".to_owned()
    } else {
        parts.join("; ")
    }
}

#[cfg(test)]
mod tests;
