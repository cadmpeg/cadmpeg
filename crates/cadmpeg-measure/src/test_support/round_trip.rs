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
//! their dev-dependencies. Failure messages state only the caller's label,
//! fixed phrases, the quantity names and the cause codes that this crate
//! defines. They never print a value or an identifier that a decoded model
//! supplies.

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::CadIr;

use crate::compare::{compare, CompareOptions, Comparison, Row, Verdict};
use crate::report::MeasureReport;
use crate::{MeasureInput, MeasureOptions, Measurer, Selection};

/// Measures every body, face and edge of `ir` in model space under the
/// service policy.
///
/// # Panics
///
/// Panics when the measurement pass fails or its session is refused.
#[must_use]
pub fn measured(label: &str, ir: &CadIr) -> MeasureReport {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0)
        .unwrap_or_else(|_| panic!("{label}: the measurement context is refused"));
    let input = MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let report = Measurer::new(input, MeasureOptions::default())
        .measure(&ctx, &Selection::default())
        .unwrap_or_else(|_| panic!("{label}: the measurement pass fails"));
    assert!(
        ctx.finish_session().is_ok(),
        "{label}: the measurement session is refused"
    );
    report
}

/// Measures `before` and `after`, compares them without entity identities
/// and asserts the verdict is [`Verdict::Agree`] over at least one body pair.
///
/// Returns the comparison and both reports.
///
/// # Panics
///
/// Panics when a measurement fails, when the comparison exceeds its pair
/// allowance, when it pairs no body, or when its verdict is not
/// [`Verdict::Agree`]. The message names the disagreeing quantities, the
/// sides with an unpaired or unmeasured body and the inconclusive causes.
pub fn assert_measures_agree(
    label: &str,
    before: &CadIr,
    after: &CadIr,
) -> (Comparison, MeasureReport, MeasureReport) {
    let before_report = measured(label, before);
    let after_report = measured(label, after);
    let comparison = compare(
        before,
        &before_report,
        after,
        &after_report,
        &CompareOptions::default(),
    )
    .unwrap_or_else(|_| panic!("{label}: the comparison exceeds its pair allowance"));
    let disagreements = disagreements(&comparison);
    assert!(
        comparison.verdict() == Verdict::Agree,
        "{label}: the round trip does not keep its measured geometry; {disagreements}"
    );
    assert!(
        !comparison.pairs().is_empty(),
        "{label}: the comparison pairs no body, so it checks no geometry"
    );
    (comparison, before_report, after_report)
}

/// Whether `ir` compared with itself gives [`Verdict::Agree`] over at least
/// one body pair.
///
/// A document whose bodies measure no comparable quantity, or whose
/// occurrences place its bodies, leaves every comparison inconclusive. A
/// round-trip test selects its fixtures with this check before it calls
/// [`assert_measures_agree`].
///
/// # Panics
///
/// Panics when the measurement fails or the comparison exceeds its pair
/// allowance.
#[must_use]
pub fn measures_decide(label: &str, ir: &CadIr) -> bool {
    let report = measured(label, ir);
    let comparison = compare(ir, &report, ir, &report, &CompareOptions::default())
        .unwrap_or_else(|_| panic!("{label}: the comparison exceeds its pair allowance"));
    comparison.verdict() == Verdict::Agree && !comparison.pairs().is_empty()
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
