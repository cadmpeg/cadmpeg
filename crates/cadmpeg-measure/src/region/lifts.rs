// SPDX-License-Identifier: Apache-2.0
//! Lifts of a parameter value by whole periods over a span of a loop
//! polyline.
//!
//! The parameter values of a loop are not reduced to the chart domain, so
//! one polyline segment can cover any number of periods. A walk over the
//! lifts in a span counts them before it starts and refuses a span whose
//! values a shift by one period does not change.

use crate::failure::{Missing, Outcome};
use crate::reason::UnavailableReason;

/// Whole periods that one span of a loop polyline segment may cover.
pub(super) const MAX_SPAN_LIFTS: usize = 1 << 16;

fn refused(reason: UnavailableReason, message: &str) -> Missing {
    Missing {
        reason,
        entity: None,
        message: message.to_owned(),
    }
}

/// Refuse a non-finite value or period, and values at whose magnitude a
/// shift by `period` is lost.
pub(super) fn resolved(values: &[f64], period: f64) -> Outcome<()> {
    if !(period.is_finite() && period > 0.0) || values.iter().any(|value| !value.is_finite()) {
        return Err(refused(
            UnavailableReason::EvaluationNonFinite,
            "a loop's parameter values left the finite range",
        ));
    }
    let magnitude = values
        .iter()
        .fold(0.0_f64, |most, value| most.max(value.abs()));
    if magnitude + period == magnitude {
        return Err(refused(
            UnavailableReason::BoundaryPeriodUnresolved,
            "a loop's parameter values are too large for a shift by one period to change them",
        ));
    }
    Ok(())
}

/// The number of whole periods `count`, refused above [`MAX_SPAN_LIFTS`]
/// and when it is not a number.
pub(super) fn bounded(count: f64) -> Outcome<usize> {
    let count = if count < 0.0 { 0.0 } else { count };
    match cadmpeg_core::convert::truncate_f64_to_usize(count) {
        Some(count) if count <= MAX_SPAN_LIFTS => Ok(count),
        _ => Err(refused(
            UnavailableReason::EvaluationWorkExhausted,
            "a loop segment covers more whole periods than the lift cap",
        )),
    }
}

/// The lifts `x0 + k·period`, for whole `k`, that lie in `[low, high)`, in
/// increasing order. The walk starts one period below the first multiple
/// and ends one period above the last, so that rounding of the end
/// multiples drops no lift.
#[derive(Debug, Clone)]
pub(super) struct Lifts {
    /// The lift at the first multiple `⌈(low − x0)/period⌉`.
    start: f64,
    period: f64,
    /// The next offset from `start`, in periods.
    step: f64,
    /// The offsets left to visit.
    left: usize,
    low: f64,
    high: f64,
}

impl Lifts {
    /// The lifts of `x0` by whole multiples of `period` in `[low, high)`.
    /// The span holds `⌊(high − x0)/period⌋ − ⌈(low − x0)/period⌉ + 1`
    /// multiples, at most [`MAX_SPAN_LIFTS`].
    pub(super) fn periodic(x0: f64, [low, high]: [f64; 2], period: f64) -> Outcome<Self> {
        resolved(&[x0, low, high], period)?;
        let first = ((low - x0) / period).ceil();
        let last = ((high - x0) / period).floor();
        let count = bounded(last - first + 1.0)?;
        Ok(Self {
            start: x0 + first * period,
            period,
            step: -1.0,
            left: count + 2,
            low,
            high,
        })
    }

    /// `x0` alone when it lies in `[low, high)`: the lifts of a value whose
    /// direction is not periodic.
    pub(super) const fn fixed(x0: f64, [low, high]: [f64; 2]) -> Self {
        Self {
            start: x0,
            period: 0.0,
            step: 0.0,
            left: 1,
            low,
            high,
        }
    }
}

impl Iterator for Lifts {
    type Item = f64;

    fn next(&mut self) -> Option<f64> {
        while let Some(left) = self.left.checked_sub(1) {
            self.left = left;
            let lift = self.start + self.step * self.period;
            self.step += 1.0;
            if lift >= self.low && lift < self.high {
                return Some(lift);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;
    use std::time::{Duration, Instant};

    use super::{bounded, resolved, Lifts, MAX_SPAN_LIFTS};
    use crate::reason::UnavailableReason;

    /// The upper bound on the time of one refusal; a walk over the span
    /// would take hours.
    const PROMPT: Duration = Duration::from_secs(1);

    fn reason(result: Result<Lifts, crate::failure::Missing>) -> UnavailableReason {
        match result {
            Ok(lifts) => panic!("expected a refusal, got {lifts:?}"),
            Err(missing) => missing.reason,
        }
    }

    #[test]
    fn every_lift_in_the_half_open_span_is_visited_once() {
        let lifts: Vec<f64> = Lifts::periodic(1.0, [0.0, 3.0 * TAU], TAU)
            .expect("bounded span")
            .collect();
        assert_eq!(lifts, [1.0, 1.0 + TAU, 1.0 + 2.0 * TAU]);
        let below: Vec<f64> = Lifts::periodic(1.0 - 5.0 * TAU, [0.0, TAU], TAU)
            .expect("bounded span")
            .collect();
        assert_eq!(below.len(), 1);
        assert!((below[0] - 1.0).abs() <= 8.0 * f64::EPSILON * TAU);
    }

    #[test]
    fn a_lift_at_the_low_end_is_in_and_one_at_the_high_end_is_out() {
        let at_low: Vec<f64> = Lifts::periodic(0.0, [0.0, 1.0], TAU)
            .expect("bounded span")
            .collect();
        assert_eq!(at_low, [0.0]);
        let at_high: Vec<f64> = Lifts::periodic(TAU, [1.0, TAU], TAU)
            .expect("bounded span")
            .collect();
        assert_eq!(at_high, Vec::<f64>::new());
    }

    #[test]
    fn a_value_without_a_period_is_its_only_lift() {
        assert_eq!(Lifts::fixed(0.5, [0.0, 1.0]).collect::<Vec<_>>(), [0.5]);
        assert_eq!(Lifts::fixed(1.0, [0.0, 1.0]).count(), 0);
    }

    #[test]
    fn a_span_of_the_cap_is_walked_and_one_period_more_is_refused() {
        let cap = cadmpeg_core::convert::f64_from_index(MAX_SPAN_LIFTS).expect("exact cap");
        let at_cap = Lifts::periodic(0.5, [0.0, (cap - 0.5) * TAU], TAU).expect("at the cap");
        assert_eq!(at_cap.count(), MAX_SPAN_LIFTS);
        let above = Lifts::periodic(0.5, [0.0, (cap + 0.5) * TAU], TAU);
        assert_eq!(reason(above), UnavailableReason::EvaluationWorkExhausted);
        assert_eq!(bounded(cap), Ok(MAX_SPAN_LIFTS));
        assert!(bounded(cap + 1.0).is_err());
        assert!(bounded(f64::INFINITY).is_err());
        assert!(bounded(f64::NAN).is_err());
    }

    #[test]
    fn a_span_of_a_trillion_is_refused_promptly() {
        let started = Instant::now();
        let refused = Lifts::periodic(1.0, [0.0, 1.0e12], TAU);
        assert_eq!(reason(refused), UnavailableReason::EvaluationWorkExhausted);
        assert!(started.elapsed() < PROMPT);
    }

    #[test]
    fn a_span_where_one_period_is_lost_is_refused_promptly() {
        let start = 2.0_f64.powi(57);
        assert_eq!(start + TAU, start);
        let started = Instant::now();
        let refused = Lifts::periodic(start + 32.0, [start, start + 1024.0], TAU);
        assert_eq!(reason(refused), UnavailableReason::BoundaryPeriodUnresolved);
        assert!(started.elapsed() < PROMPT);
    }

    #[test]
    fn non_finite_values_are_refused() {
        for (x0, span) in [
            (f64::NAN, [0.0, 1.0]),
            (0.0, [0.0, f64::INFINITY]),
            (0.0, [f64::NEG_INFINITY, 0.0]),
        ] {
            let refused = Lifts::periodic(x0, span, TAU);
            assert_eq!(reason(refused), UnavailableReason::EvaluationNonFinite);
        }
        assert!(resolved(&[0.0], f64::NAN).is_err());
        assert!(resolved(&[0.0], 0.0).is_err());
        assert!(resolved(&[1.0e12, -1.0e12], TAU).is_ok());
    }
}
