// SPDX-License-Identifier: Apache-2.0
//! Membership of parameter points in a region whose loops keep it on their
//! left.
//!
//! The region indicator is 1 at a reference point just left of a loop. A
//! path from the reference point runs along `v` to the target's `v`, then
//! along `u` to the target. Each crossing of a loop segment adds 1 when the
//! path enters the left side of the segment and subtracts 1 when it leaves
//! it. Segments are half-open in the crossing coordinate, so a path through
//! a polyline vertex counts it once. Periodic coordinates are taken modulo
//! their period: a leg crosses a segment at every lift of the leg by whole
//! periods in the segment's span, and a segment whose lifts
//! [`Lifts::periodic`] refuses gives the region no indicator.

use super::lifts::Lifts;
use crate::failure::Outcome;

/// The distance, relative to the largest `|v|` in play, within which a
/// polyline point lies at a path level.
const MEMBERSHIP_LEVEL_RELATIVE: f64 = 1.0e-9;

/// The smallest `|v|` scale of the level tolerance.
const MEMBERSHIP_LEVEL_SCALE_FLOOR: f64 = 1.0;

/// The step to a clear level when no polyline point lies above `v` on a
/// domain whose `v` is not periodic.
const MEMBERSHIP_LEVEL_OPEN_GAP: f64 = 1.0;

/// A loop polyline in the parameter plane, oriented with the region on its
/// left.
#[derive(Debug, Clone)]
pub(crate) struct Oriented {
    pub(crate) points: Vec<[f64; 2]>,
    /// Whether the polyline closes on its first point; a loop with a net
    /// shift is an open chain whose end repeats its start whole periods on.
    pub(crate) closed: bool,
}

impl Oriented {
    /// The polyline's segments.
    pub(crate) fn segments(&self) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
        let closing = match (self.closed, self.points.first(), self.points.last()) {
            (true, Some(first), Some(last)) if self.points.len() > 1 => Some((*last, *first)),
            _ => None,
        };
        self.points
            .windows(2)
            .map(|pair| (pair[0], pair[1]))
            .chain(closing)
    }
}

/// The lifts `x0 + k·period` (every `k`, or `k = 0` without a period) that
/// lie in `span`, half-open at its upper end.
fn lifts(x0: f64, span: [f64; 2], period: Option<f64>) -> Outcome<Lifts> {
    match period {
        Some(period) => Lifts::periodic(x0, span, period),
        None => Ok(Lifts::fixed(x0, span)),
    }
}

/// Whether `value` lies strictly between `from` and `to` along the leg from
/// `from` to `to`, with `value` taken modulo `period`.
fn on_leg(value: f64, from: f64, to: f64, period: Option<f64>) -> bool {
    let along = match period {
        Some(period) if to >= from => from + (value - from).rem_euclid(period),
        Some(period) => from - (from - value).rem_euclid(period),
        None => value,
    };
    if to >= from {
        along > from && along < to
    } else {
        along < from && along > to
    }
}

/// The parameter-plane picture of a region: its oriented loops, the
/// periods of the domain and a reference point inside the region.
#[derive(Debug, Clone)]
pub(crate) struct Indicator {
    pub(crate) loops: Vec<Oriented>,
    pub(crate) periods: [Option<f64>; 2],
    pub(crate) reference: [f64; 2],
}

impl Indicator {
    /// Signed crossings of the leg `u = u0` from `v = from` to `v = to`.
    fn along_v(&self, u0: f64, from: f64, to: f64) -> Outcome<i64> {
        let direction: i64 = if to >= from { 1 } else { -1 };
        let mut count = 0;
        for polyline in &self.loops {
            for (a, b) in polyline.segments() {
                if a[0] == b[0] {
                    continue;
                }
                let side: i64 = if b[0] > a[0] { 1 } else { -1 };
                for x in lifts(u0, [a[0].min(b[0]), a[0].max(b[0])], self.periods[0])? {
                    let height = a[1] + (x - a[0]) / (b[0] - a[0]) * (b[1] - a[1]);
                    if on_leg(height, from, to, self.periods[1]) {
                        count += side * direction;
                    }
                }
            }
        }
        Ok(count)
    }

    /// Signed crossings of the leg `v = v0` from `u = from` to `u = to`.
    fn along_u(&self, v0: f64, from: f64, to: f64) -> Outcome<i64> {
        let direction: i64 = if to >= from { 1 } else { -1 };
        let mut count = 0;
        for polyline in &self.loops {
            for (a, b) in polyline.segments() {
                if a[1] == b[1] {
                    continue;
                }
                let side: i64 = if b[1] > a[1] { -1 } else { 1 };
                for y in lifts(v0, [a[1].min(b[1]), a[1].max(b[1])], self.periods[1])? {
                    let across = a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                    if on_leg(across, from, to, self.periods[0]) {
                        count += side * direction;
                    }
                }
            }
        }
        Ok(count)
    }

    /// The nearest lift of `value` to `from` in a periodic direction.
    fn nearest(&self, axis: usize, from: f64, value: f64) -> f64 {
        match self.periods[axis] {
            Some(period) => value + ((from - value) / period).round() * period,
            None => value,
        }
    }

    /// The region indicator at `target`.
    pub(crate) fn at(&self, target: [f64; 2]) -> Outcome<i64> {
        let [u, v] = self.reference;
        let v_target = self.nearest(1, v, target[1]);
        let u_target = self.nearest(0, u, target[0]);
        let level = self.clear_level(v_target);
        Ok(1 + self.along_v(u, v, level)?
            + self.along_u(level, u, u_target)?
            + self.along_v(u_target, level, v_target)?)
    }

    /// `v`, or a level just above it halfway to the next polyline point
    /// `v` when a polyline point lies at `v`.
    fn clear_level(&self, v: f64) -> f64 {
        let scale = self
            .loops
            .iter()
            .flat_map(|polyline| polyline.points.iter())
            .fold(v.abs(), |scale, point| scale.max(point[1].abs()))
            .max(MEMBERSHIP_LEVEL_SCALE_FLOOR);
        let level_tolerance = MEMBERSHIP_LEVEL_RELATIVE * scale;
        let offsets = self.loops.iter().flat_map(|polyline| {
            polyline.points.iter().map(move |point| {
                let offset = point[1] - v;
                match self.periods[1] {
                    Some(period) => offset.rem_euclid(period),
                    None => offset,
                }
            })
        });
        let mut touches = false;
        let mut next = f64::INFINITY;
        for offset in offsets {
            let wrapped = self.periods[1].map_or(offset, |period| {
                if period - offset <= level_tolerance {
                    offset - period
                } else {
                    offset
                }
            });
            if wrapped.abs() <= level_tolerance {
                touches = true;
            } else if wrapped > 0.0 {
                next = next.min(wrapped);
            }
        }
        if !touches {
            return v;
        }
        let gap = if next.is_finite() {
            next
        } else {
            self.periods[1].unwrap_or(MEMBERSHIP_LEVEL_OPEN_GAP)
        };
        v + gap / 2.0
    }

    /// The indicator above every loop point (`toward = 1`) or below every
    /// loop point (`toward = −1`) on a domain whose `v` is not periodic.
    pub(crate) fn beyond(&self, toward: f64) -> Outcome<i64> {
        let [u, v] = self.reference;
        Ok(1 + self.along_v(u, v, toward * f64::INFINITY)?)
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;
    use std::time::{Duration, Instant};

    use super::{lifts, Indicator, Oriented};
    use crate::reason::UnavailableReason;

    /// The upper bound on the time of one refusal; a walk over the span
    /// would take hours or never end.
    const PROMPT: Duration = Duration::from_secs(1);

    /// The unit square `[1, 2]²` counterclockwise on a `u`-periodic strip.
    fn square() -> Indicator {
        Indicator {
            loops: vec![Oriented {
                points: vec![[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]],
                closed: true,
            }],
            periods: [Some(TAU), None],
            reference: [1.5, 1.0 + 1.0e-6],
        }
    }

    #[test]
    fn points_inside_and_outside_a_square_read_one_and_zero() {
        let square = square();
        assert_eq!(square.at([1.5, 1.5]), Ok(1));
        assert_eq!(square.at([1.5 + TAU, 1.5]), Ok(1));
        assert_eq!(square.at([3.0, 1.5]), Ok(0));
        assert_eq!(square.at([1.5, 3.0]), Ok(0));
        assert_eq!(square.beyond(1.0), Ok(0));
        assert_eq!(square.beyond(-1.0), Ok(0));
    }

    #[test]
    fn a_target_level_with_a_loop_vertex_reads_its_side() {
        let square = square();
        for target in [[3.0, 1.0], [3.0, 2.0], [0.5, 2.0], [0.5, 1.0]] {
            assert_eq!(square.at(target), Ok(0), "{target:?}");
        }
        assert_eq!(square.at([1.5, 2.0 - 1.0e-3]), Ok(1));
    }

    #[test]
    fn a_wrapping_loop_bounds_the_part_above_it() {
        let cap = Indicator {
            loops: vec![Oriented {
                points: vec![[0.0, 0.5], [TAU / 2.0, 0.5], [TAU, 0.5]],
                closed: false,
            }],
            periods: [Some(TAU), None],
            reference: [1.0, 0.5 + 1.0e-6],
        };
        assert_eq!(cap.beyond(1.0), Ok(1));
        assert_eq!(cap.beyond(-1.0), Ok(0));
        assert_eq!(cap.at([4.0, 1.0]), Ok(1));
        assert_eq!(cap.at([4.0, 0.0]), Ok(0));
    }

    #[test]
    fn a_doubly_periodic_band_reads_through_the_period() {
        let band = Indicator {
            loops: vec![
                Oriented {
                    points: vec![[0.0, -1.0], [TAU, -1.0]],
                    closed: false,
                },
                Oriented {
                    points: vec![[TAU, 1.0], [0.0, 1.0]],
                    closed: false,
                },
            ],
            periods: [Some(TAU), Some(TAU)],
            reference: [1.0, -1.0 + 1.0e-6],
        };
        assert_eq!(band.at([2.0, 0.0]), Ok(1));
        assert_eq!(band.at([2.0, 0.0 + TAU]), Ok(1));
        assert_eq!(band.at([2.0, 2.0]), Ok(0));
        assert_eq!(band.at([2.0, -2.0]), Ok(0));
        assert_eq!(band.at([2.0, 3.0]), Ok(0));
    }

    /// The strip `[low, low + span] × [−½, ½]` counterclockwise on the
    /// doubly periodic domain, referenced just inside its left side.
    fn strip(low: f64, span: f64) -> Indicator {
        Indicator {
            loops: vec![Oriented {
                points: vec![
                    [low, -0.5],
                    [low + span, -0.5],
                    [low + span, 0.5],
                    [low, 0.5],
                ],
                closed: true,
            }],
            periods: [Some(TAU), Some(TAU)],
            reference: [low + 1.0, -0.5 + 1.0e-3],
        }
    }

    fn refusal<T: std::fmt::Debug>(result: crate::failure::Outcome<T>) -> UnavailableReason {
        match result {
            Ok(value) => panic!("expected a refusal, got {value:?}"),
            Err(missing) => missing.reason,
        }
    }

    #[test]
    fn a_segment_of_a_trillion_periods_is_refused_promptly() {
        let started = Instant::now();
        assert_eq!(
            refusal(lifts(1.0, [0.0, 1.0e12], Some(TAU))),
            UnavailableReason::EvaluationWorkExhausted
        );
        let strip = strip(0.0, 1.0e12);
        assert_eq!(
            refusal(strip.at([2.0, 0.0])),
            UnavailableReason::EvaluationWorkExhausted
        );
        assert_eq!(
            refusal(strip.beyond(1.0)),
            UnavailableReason::EvaluationWorkExhausted
        );
        assert!(started.elapsed() < PROMPT);
    }

    #[test]
    fn a_segment_where_one_period_is_lost_is_refused_promptly() {
        let low = 2.0_f64.powi(57);
        assert_eq!(low + TAU, low);
        let started = Instant::now();
        assert_eq!(
            refusal(lifts(low + 32.0, [low, low + 1024.0], Some(TAU))),
            UnavailableReason::BoundaryPeriodUnresolved
        );
        assert_eq!(
            refusal(strip(low, 1024.0).at([low + 64.0, 0.0])),
            UnavailableReason::BoundaryPeriodUnresolved
        );
        assert!(started.elapsed() < PROMPT);
    }

    #[test]
    fn a_strip_shorter_than_a_period_reads_inside_and_outside() {
        let strip = strip(0.0, 3.0);
        assert_eq!(strip.at([2.0, 0.0]), Ok(1));
        assert_eq!(strip.at([2.0 + TAU, 0.0]), Ok(1));
        assert_eq!(strip.at([2.0, 2.0]), Ok(0));
        assert_eq!(strip.at([4.0, 0.0]), Ok(0));
    }
}
