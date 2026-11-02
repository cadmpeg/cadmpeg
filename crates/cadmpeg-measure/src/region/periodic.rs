// SPDX-License-Identifier: Apache-2.0
//! Nesting on a domain periodic in `u` and unbounded in `v`: cylinders and
//! cones.
//!
//! A loop with zero net shift is contractible; a loop that wraps once around
//! the axis is not. With `m` non-contractible loops the region is:
//!
//! - `m = 2`: the band between them; contractible loops are holes in it;
//! - `m = 1` on a cone: the part between the loop and the apex, which is a
//!   member of the region;
//! - `m = 0`: the planar rule in the strip, with winding numbers taken over
//!   lifts of each loop by whole periods.
//!
//! Every other configuration is unbounded or self-contradictory.

use cadmpeg_ir::topology::{Face, FaceLoops};

use super::lifts::Lifts;
use super::{
    classification, degenerate, invalid, ring_loops, winding_number, LoopRegion, LoopShape, Region,
};
use crate::boundary::{LoopKind, ResolvedFace};
use crate::canonical::surface::Chart;
use crate::failure::{Measure, MeasureFailure, Outcome};
use crate::reason::UnavailableReason;
use crate::report::{Classification, LoopRole, RegionRule};

/// Lifts of a contractible loop tried by the strip winding test.
const STRIP_LIFTS: [f64; 5] = [-2.0, -1.0, 0.0, 1.0, 2.0];

/// One ring loop of the face with its polyline.
struct Ring<'a> {
    index: usize,
    shape: &'a LoopShape,
    /// The net shift in `u`.
    turns: i32,
}

impl Ring<'_> {
    /// The midpoint of the loop's longest polyline segment: a point of the
    /// loop away from the vertices where other loops meet it.
    fn sample(&self) -> Option<[f64; 2]> {
        let points = &self.shape.points;
        let segments = if self.turns == 0 {
            points.len()
        } else {
            points.len().checked_sub(1)?
        };
        (0..segments)
            .map(|index| (points[index], points[(index + 1) % points.len()]))
            .max_by(|(a, b), (c, d)| {
                let first = (b[0] - a[0]).hypot(b[1] - a[1]);
                let second = (d[0] - c[0]).hypot(d[1] - c[1]);
                first.total_cmp(&second)
            })
            .map(|(a, b)| [a[0].midpoint(b[0]), a[1].midpoint(b[1])])
    }
}

/// The number of times the downward vertical ray from `point` crosses the
/// polyline, with `u` taken modulo `period`: once per lift of the ray in a
/// segment's `u` span below the segment. A non-contractible polyline is an
/// open chain whose last point repeats its first one period on; a
/// contractible polyline is closed. A segment whose lifts
/// [`Lifts::periodic`] refuses refuses the count.
pub(super) fn crossings_below(
    points: &[[f64; 2]],
    closed: bool,
    point: [f64; 2],
    period: f64,
) -> Outcome<usize> {
    let segments = if closed {
        points.len()
    } else {
        match points.len().checked_sub(1) {
            Some(segments) => segments,
            None => return Ok(0),
        }
    };
    let mut crossings = 0;
    for index in 0..segments {
        let from = points[index];
        let to = points[(index + 1) % points.len()];
        if from[0] == to[0] {
            continue;
        }
        let (low, high) = if from[0] < to[0] {
            (from[0], to[0])
        } else {
            (to[0], from[0])
        };
        for lifted in Lifts::periodic(point[0], [low, high], period)? {
            let fraction = (lifted - from[0]) / (to[0] - from[0]);
            let height = from[1] + fraction * (to[1] - from[1]);
            if height < point[1] {
                crossings += 1;
            }
        }
    }
    Ok(crossings)
}

/// Whether a contractible loop winds around `point` at some lift.
fn encloses(shape: &LoopShape, point: [f64; 2], period: f64) -> bool {
    STRIP_LIFTS
        .iter()
        .any(|k| winding_number(&shape.points, [point[0] + k * period, point[1]]) != 0)
}

/// Whether `point` lies above a non-contractible loop.
fn above(ring: &Ring<'_>, point: [f64; 2], period: f64) -> Outcome<bool> {
    Ok(crossings_below(&ring.shape.points, false, point, period)? % 2 == 1)
}

/// The face-left factor of a loop: `+1` when its traversal already runs in
/// `direction` (`+1` for `+u`, `−1` for `−u`), `−1` otherwise.
fn factor(turns: i32, direction: i32) -> f64 {
    if turns.signum() == direction {
        1.0
    } else {
        -1.0
    }
}

/// The region of a face on a cylinder or cone.
pub(super) fn classify(
    face: &Face,
    chart: &Chart,
    resolved: &ResolvedFace<'_>,
    shapes: &[Option<LoopShape>],
    v_tolerance: f64,
) -> Measure<Region> {
    let Some(period) = chart.periods()[0] else {
        return Err(invalid("the carrier is not periodic in u"));
    };
    let mut contractible: Vec<Ring<'_>> = Vec::new();
    let mut wrapping: Vec<Ring<'_>> = Vec::new();
    for (index, resolved_loop) in ring_loops(resolved)? {
        let Some(Some(shape)) = shapes.get(index) else {
            return Err(invalid("a loop has no parameter-plane polyline"));
        };
        let [turns, v_turns] = resolved_loop.net_shift;
        if v_turns != 0 || turns.abs() > 1 {
            return Err(invalid("a loop wraps the axis more than once"));
        }
        let ring = Ring {
            index,
            shape,
            turns,
        };
        if turns == 0 {
            if degenerate(shape) {
                return Err(MeasureFailure::unavailable(
                    UnavailableReason::BoundaryLoopDegenerate,
                    Some(resolved.loops[index].face_loop.id.as_str()),
                    "a ring loop encloses no area relative to its own size",
                ));
            }
            contractible.push(ring);
        } else {
            wrapping.push(ring);
        }
    }
    if contractible.is_empty() && wrapping.is_empty() {
        return Err(MeasureFailure::unavailable(
            UnavailableReason::BoundaryUnboundedFace,
            Some(face.id.as_str()),
            "a face on an unbounded cylinder or cone has no boundary loop",
        ));
    }
    let mut loops = vec![
        LoopRegion {
            role: LoopRole::Vertex,
            factor: 0.0,
        };
        resolved.loops.len()
    ];
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for ring in contractible.iter().chain(&wrapping) {
        for point in &ring.shape.points {
            low = low.min(point[1]);
            high = high.max(point[1]);
        }
    }
    let apex = chart.apex();
    if wrapping.len() != 1
        && apex.is_some_and(|(_, apex)| {
            resolved.loops.iter().any(|resolution| {
                resolution.outcome.as_ref().is_ok_and(|resolved_loop| {
                    matches!(resolved_loop.kind, LoopKind::Vertex(point) if point == apex)
                })
            })
        })
    {
        return Err(invalid("a loop at the apex and no one loop reaching it"));
    }
    let (base, outer, apex_member) = match wrapping.as_slice() {
        [] => {
            let outer = strip_outer(&contractible, period)?;
            (low, Some(outer), None)
        }
        [ring] => {
            let Some((apex_v, _)) = apex else {
                return Err(invalid(
                    "one loop wraps the axis and no apex closes the region",
                ));
            };
            let ring_low = ring
                .shape
                .points
                .iter()
                .map(|p| p[1])
                .fold(f64::INFINITY, f64::min);
            let ring_high = ring
                .shape
                .points
                .iter()
                .map(|p| p[1])
                .fold(f64::NEG_INFINITY, f64::max);
            let direction = if ring_low >= apex_v - v_tolerance {
                -1
            } else if ring_high <= apex_v + v_tolerance {
                1
            } else {
                return Err(invalid("the loop around the axis crosses the apex"));
            };
            loops[ring.index] = LoopRegion {
                role: LoopRole::Outer,
                factor: factor(ring.turns, direction),
            };
            for hole in &contractible {
                let Some(point) = hole.sample() else {
                    return Err(invalid("a loop has no sample point"));
                };
                let inside = if direction < 0 {
                    point[1] > apex_v && !above(ring, point, period)?
                } else {
                    point[1] < apex_v && above(ring, point, period)?
                };
                if !inside {
                    return Err(invalid("a hole lies outside the region"));
                }
            }
            (apex_v, Some(ring.index), Some(true))
        }
        [first, second] => {
            let (Some(first_point), Some(second_point)) = (first.sample(), second.sample()) else {
                return Err(invalid("a loop has no sample point"));
            };
            let (lower, upper) = match (
                above(first, second_point, period)?,
                above(second, first_point, period)?,
            ) {
                (true, false) => (first, second),
                (false, true) => (second, first),
                _ => return Err(invalid("the two loops around the axis cross")),
            };
            loops[lower.index] = LoopRegion {
                role: LoopRole::BandLower,
                factor: factor(lower.turns, 1),
            };
            loops[upper.index] = LoopRegion {
                role: LoopRole::BandUpper,
                factor: factor(upper.turns, -1),
            };
            for hole in &contractible {
                let Some(point) = hole.sample() else {
                    return Err(invalid("a loop has no sample point"));
                };
                if !above(lower, point, period)? || above(upper, point, period)? {
                    return Err(invalid("a hole lies outside the band"));
                }
            }
            (low, None, None)
        }
        _ => return Err(invalid("more than two loops wrap the axis")),
    };
    for (position, hole) in contractible.iter().enumerate() {
        if Some(hole.index) == outer {
            continue;
        }
        for other in contractible.iter().skip(position + 1) {
            if Some(other.index) == outer {
                continue;
            }
            let contains = |a: &Ring<'_>, b: &Ring<'_>| {
                b.sample()
                    .is_some_and(|point| encloses(a.shape, point, period))
            };
            if contains(hole, other) || contains(other, hole) {
                return Err(invalid("one hole contains another"));
            }
        }
    }
    for ring in &contractible {
        if Some(ring.index) == outer && wrapping.is_empty() {
            loops[ring.index] = LoopRegion {
                role: LoopRole::Outer,
                factor: if ring.shape.integral.value > 0.0 {
                    1.0
                } else {
                    -1.0
                },
            };
        } else {
            loops[ring.index] = LoopRegion {
                role: LoopRole::Inner,
                factor: if ring.shape.integral.value > 0.0 {
                    -1.0
                } else {
                    1.0
                },
            };
        }
    }
    let apex = match (apex, apex_member) {
        (Some((_, point)), Some(true)) => Some(point),
        (Some((apex_v, point)), _)
            if apex_v >= low - v_tolerance && apex_v <= high + v_tolerance =>
        {
            Some(point)
        }
        _ => None,
    };
    let classification = match (&face.loops, wrapping.as_slice(), outer) {
        (FaceLoops::Unspecified { .. }, _, _) => Classification::Unspecified,
        (FaceLoops::Classified { outer: stored, .. }, [_, _], _) => {
            if wrapping
                .iter()
                .any(|ring| resolved.loops[ring.index].face_loop.id == *stored)
            {
                Classification::Consistent
            } else {
                Classification::Conflict
            }
        }
        (_, _, Some(outer)) => classification(face, resolved, outer),
        (_, _, None) => Classification::Unspecified,
    };
    Ok(Region {
        rule: RegionRule::Nesting,
        loops,
        base,
        classification,
        apex,
        exterior: 0.0,
        closed: None,
    })
}

/// The outer loop of contractible loops in the strip: the one loop that
/// winds around a point of every other loop at some lift.
fn strip_outer(rings: &[Ring<'_>], period: f64) -> Measure<usize> {
    let outers: Vec<usize> = rings
        .iter()
        .filter(|ring| {
            rings
                .iter()
                .filter(|other| other.index != ring.index)
                .all(|other| {
                    other
                        .sample()
                        .is_some_and(|point| encloses(ring.shape, point, period))
                })
        })
        .map(|ring| ring.index)
        .collect();
    match outers.as_slice() {
        [outer] => Ok(*outer),
        _ => Err(invalid("no single loop encloses every other loop")),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::crossings_below;
    use crate::reason::UnavailableReason;

    /// The upper bound on the time of one refusal; a walk over the span
    /// would take hours or never end.
    const PROMPT: Duration = Duration::from_secs(1);

    fn refusal(result: crate::failure::Outcome<usize>) -> UnavailableReason {
        match result {
            Ok(count) => panic!("expected a refusal, got {count} crossings"),
            Err(missing) => missing.reason,
        }
    }

    #[test]
    fn a_wrapping_polyline_is_crossed_once_from_above() {
        let tau = std::f64::consts::TAU;
        let circle = [[0.0, 1.0], [tau / 2.0, 1.0], [tau, 1.0]];
        assert_eq!(crossings_below(&circle, false, [1.0, 2.0], tau), Ok(1));
        assert_eq!(crossings_below(&circle, false, [1.0, 0.0], tau), Ok(0));
        assert_eq!(
            crossings_below(&circle, false, [1.0 + 3.0 * tau, 2.0], tau),
            Ok(1)
        );
        assert_eq!(crossings_below(&circle, false, [-1.0, 2.0], tau), Ok(1));
    }

    #[test]
    fn a_closed_polyline_is_crossed_twice_from_above() {
        let tau = std::f64::consts::TAU;
        let square = [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]];
        assert_eq!(crossings_below(&square, true, [1.5, 3.0], tau), Ok(2));
        assert_eq!(crossings_below(&square, true, [1.5, 1.5], tau), Ok(1));
        assert_eq!(crossings_below(&square, true, [1.5 + tau, 1.5], tau), Ok(1));
    }

    #[test]
    fn a_segment_of_a_trillion_periods_is_refused_promptly() {
        let tau = std::f64::consts::TAU;
        let segment = [[0.0, 0.0], [1.0e12, 1.0]];
        let started = Instant::now();
        assert_eq!(
            refusal(crossings_below(&segment, false, [1.0, 2.0], tau)),
            UnavailableReason::EvaluationWorkExhausted
        );
        assert!(started.elapsed() < PROMPT);
    }

    #[test]
    fn a_segment_where_one_period_is_lost_is_refused_promptly() {
        let tau = std::f64::consts::TAU;
        let start = 2.0_f64.powi(57);
        assert_eq!(start + tau, start);
        let segment = [[start, 0.0], [start + 1024.0, 1.0]];
        let started = Instant::now();
        assert_eq!(
            refusal(crossings_below(&segment, false, [start + 32.0, 2.0], tau)),
            UnavailableReason::BoundaryPeriodUnresolved
        );
        assert!(started.elapsed() < PROMPT);
    }
}
