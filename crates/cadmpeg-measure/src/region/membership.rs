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
//! their period.

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
/// lie in `[low, high)`.
fn lifts(x0: f64, [low, high]: [f64; 2], period: Option<f64>) -> Vec<f64> {
    let Some(period) = period else {
        return if x0 >= low && x0 < high {
            vec![x0]
        } else {
            Vec::new()
        };
    };
    let mut found = Vec::new();
    let mut x = x0 + ((low - x0) / period).ceil() * period;
    while x < high {
        if x >= low {
            found.push(x);
        }
        x += period;
    }
    found
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
    fn along_v(&self, u0: f64, from: f64, to: f64) -> i64 {
        let direction: i64 = if to >= from { 1 } else { -1 };
        let mut count = 0;
        for polyline in &self.loops {
            for (a, b) in polyline.segments() {
                if a[0] == b[0] {
                    continue;
                }
                let side: i64 = if b[0] > a[0] { 1 } else { -1 };
                for x in lifts(u0, [a[0].min(b[0]), a[0].max(b[0])], self.periods[0]) {
                    let height = a[1] + (x - a[0]) / (b[0] - a[0]) * (b[1] - a[1]);
                    if on_leg(height, from, to, self.periods[1]) {
                        count += side * direction;
                    }
                }
            }
        }
        count
    }

    /// Signed crossings of the leg `v = v0` from `u = from` to `u = to`.
    fn along_u(&self, v0: f64, from: f64, to: f64) -> i64 {
        let direction: i64 = if to >= from { 1 } else { -1 };
        let mut count = 0;
        for polyline in &self.loops {
            for (a, b) in polyline.segments() {
                if a[1] == b[1] {
                    continue;
                }
                let side: i64 = if b[1] > a[1] { -1 } else { 1 };
                for y in lifts(v0, [a[1].min(b[1]), a[1].max(b[1])], self.periods[1]) {
                    let across = a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                    if on_leg(across, from, to, self.periods[0]) {
                        count += side * direction;
                    }
                }
            }
        }
        count
    }

    /// The nearest lift of `value` to `from` in a periodic direction.
    fn nearest(&self, axis: usize, from: f64, value: f64) -> f64 {
        match self.periods[axis] {
            Some(period) => value + ((from - value) / period).round() * period,
            None => value,
        }
    }

    /// The region indicator at `target`.
    pub(crate) fn at(&self, target: [f64; 2]) -> i64 {
        let [u, v] = self.reference;
        let v_target = self.nearest(1, v, target[1]);
        let u_target = self.nearest(0, u, target[0]);
        1 + self.along_v(u, v, v_target) + self.along_u(v_target, u, u_target)
    }

    /// The indicator above every loop point (`toward = 1`) or below every
    /// loop point (`toward = −1`) on a domain whose `v` is not periodic.
    pub(crate) fn beyond(&self, toward: f64) -> i64 {
        let [u, v] = self.reference;
        1 + self.along_v(u, v, toward * f64::INFINITY)
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::{Indicator, Oriented};

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
        assert_eq!(square.at([1.5, 1.5]), 1);
        assert_eq!(square.at([1.5 + TAU, 1.5]), 1);
        assert_eq!(square.at([3.0, 1.5]), 0);
        assert_eq!(square.at([1.5, 3.0]), 0);
        assert_eq!(square.beyond(1.0), 0);
        assert_eq!(square.beyond(-1.0), 0);
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
        assert_eq!(cap.beyond(1.0), 1);
        assert_eq!(cap.beyond(-1.0), 0);
        assert_eq!(cap.at([4.0, 1.0]), 1);
        assert_eq!(cap.at([4.0, 0.0]), 0);
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
        assert_eq!(band.at([2.0, 0.0]), 1);
        assert_eq!(band.at([2.0, 0.0 + TAU]), 1);
        assert_eq!(band.at([2.0, 2.0]), 0);
        assert_eq!(band.at([2.0, -2.0]), 0);
        assert_eq!(band.at([2.0, 3.0]), 0);
    }
}
