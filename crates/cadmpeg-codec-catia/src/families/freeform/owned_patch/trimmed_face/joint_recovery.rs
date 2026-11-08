// SPDX-License-Identifier: Apache-2.0
//! Match trim occurrences to a common native vertex inside their active ranges.

use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::math::Point3;

use super::{delta, Trim, ROOT_STEPS, TRIM_STEPS};

const ANCHORED_UV_JOIN: f64 = 1.0e-4;

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    let [u, v] = delta(a, b);
    u.hypot(v)
}

fn project(trim: &Trim, uv: [f64; 2]) -> Option<f64> {
    closest(trim, ANCHORED_UV_JOIN, |t| Some(distance(trim.uv(t)?, uv)))
}

fn closest(trim: &Trim, tolerance: f64, at: impl Fn(f64) -> Option<f64>) -> Option<f64> {
    let at = |t| at(t).unwrap_or(f64::INFINITY);
    let samples = (0..=TRIM_STEPS)
        .map(|i| {
            let t = trim.sample(i, true);
            (t, at(t))
        })
        .collect::<Vec<_>>();
    let mut solution = None;
    for (i, &(t, gap)) in samples.iter().enumerate() {
        if !gap.is_finite() {
            continue;
        }
        let previous = i.checked_sub(1).and_then(|j| samples.get(j));
        let next = samples.get(i + 1);
        if previous.is_some_and(|&(_, d)| d <= gap) || next.is_some_and(|&(_, d)| d < gap) {
            continue;
        }
        let mut lo = previous.map_or(t, |&(t, _)| t);
        let mut hi = next.map_or(t, |&(t, _)| t);
        for _ in 0..ROOT_STEPS {
            let a = lo + (hi - lo) / 3.0;
            let b = hi - (hi - lo) / 3.0;
            if at(a) <= at(b) {
                hi = b;
            } else {
                lo = a;
            }
        }
        let interior = lo.midpoint(hi);
        let best = if at(interior) < gap { interior } else { t };
        if at(best) <= tolerance && solution.replace(best).is_some() {
            return None;
        }
    }
    solution
}

pub(super) fn anchored(sides: [&Trim; 2], stations: [Option<f64>; 2]) -> Option<[f64; 2]> {
    match stations {
        [Some(a), Some(b)] => {
            (distance(sides[0].uv(a)?, sides[1].uv(b)?) <= ANCHORED_UV_JOIN).then_some([a, b])
        }
        [Some(a), None] => Some([a, project(sides[1], sides[0].uv(a)?)?]),
        [None, Some(b)] => Some([project(sides[0], sides[1].uv(b)?)?, b]),
        [None, None] => None,
    }
}

const CROSSING_STEPS: usize = 12;
const DERIVATIVE_FRACTION: f64 = 1.0e-5;
const PARAMETER_MERGE: f64 = 1.0e-5;

pub(super) fn crossing(sides: [&Trim; 2], stations: [Option<f64>; 2]) -> Option<[f64; 2]> {
    let [a, b] = sides;
    let sample = |trim: &Trim| {
        (0..=TRIM_STEPS)
            .map(|i| {
                let t = trim.sample(i, true);
                Some((t, trim.uv(t)?))
            })
            .collect::<Option<Vec<_>>>()
    };
    let a_samples = sample(a)?;
    let b_samples = sample(b)?;
    let mut solution: Option<[f64; 2]> = None;
    let mut collect = |candidate: [f64; 2]| {
        for ((side, known), t) in sides.into_iter().zip(stations).zip(candidate) {
            if let Some(known) = known {
                if distance(side.uv(known)?, side.uv(t)?) > ANCHORED_UV_JOIN {
                    return Some(());
                }
            }
        }
        if let Some(prior) = solution {
            if !candidate
                .into_iter()
                .zip(prior)
                .zip(sides)
                .all(|((x, y), side)| {
                    (x - y).abs() <= PARAMETER_MERGE * (side.range[1] - side.range[0])
                })
            {
                return None;
            }
        } else {
            solution = Some(candidate);
        }
        Some(())
    };
    for (i, side) in sides.into_iter().enumerate() {
        for t in side.range {
            let other = sides[1 - i];
            if let Some(q) = project(other, side.uv(t)?) {
                let mut candidate = [q, q];
                candidate[i] = t;
                collect(candidate)?;
            }
        }
    }
    for a_segment in a_samples.windows(2) {
        let da = delta(a_segment[0].1, a_segment[1].1);
        for b_segment in b_samples.windows(2) {
            let db = delta(b_segment[0].1, b_segment[1].1);
            let Some([s, t]) = line_intersection(delta(a_segment[0].1, b_segment[0].1), da, db)
            else {
                continue;
            };
            if !(0.0..=1.0).contains(&s) || !(0.0..=1.0).contains(&t) {
                continue;
            }
            let initial = [
                (a_segment[1].0 - a_segment[0].0).mul_add(s, a_segment[0].0),
                (b_segment[1].0 - b_segment[0].0).mul_add(t, b_segment[0].0),
            ];
            if let Some(candidate) = refine(sides, initial) {
                collect(candidate)?;
            }
        }
    }
    solution
}

fn line_intersection(offset: [f64; 2], a: [f64; 2], b: [f64; 2]) -> Option<[f64; 2]> {
    let cross = super::det(a, b);
    (cross.abs() > super::PARALLEL_SINE * a[0].hypot(a[1]) * b[0].hypot(b[1]))
        .then(|| [super::det(offset, b) / cross, super::det(offset, a) / cross])
}

fn refine(sides: [&Trim; 2], mut pair: [f64; 2]) -> Option<[f64; 2]> {
    let tangent = |side: &Trim, t: f64| {
        let [lo, hi] = side.range;
        let step = (hi - lo) * DERIVATIVE_FRACTION;
        let a = (t - step).max(lo);
        let b = (t + step).min(hi);
        let direction = delta(side.uv(a)?, side.uv(b)?);
        Some(direction.map(|d| d / (b - a)))
    };
    for _ in 0..CROSSING_STEPS {
        let offset = delta(sides[0].uv(pair[0])?, sides[1].uv(pair[1])?);
        if offset[0].hypot(offset[1]) <= super::UV_JOIN {
            return Some(pair);
        }
        let corrections = line_intersection(
            offset,
            tangent(sides[0], pair[0])?,
            tangent(sides[1], pair[1])?,
        )?;
        for ((t, shift), side) in pair.iter_mut().zip(corrections).zip(sides) {
            *t = (*t + shift).clamp(side.range[0], side.range[1]);
        }
    }
    None
}

const KNOWN_VERTEX_JOIN_MM: f64 = 0.0004;

pub(super) fn extended_join(
    sides: [&Trim; 2],
    ends: [[[f64; 2]; 2]; 2],
    stations: [Option<f64>; 2],
    world: impl Fn([f64; 2]) -> Option<Point3>,
) -> Option<[f64; 2]> {
    let mut solution: Option<[f64; 2]> = None;
    for (i, side) in sides.into_iter().enumerate() {
        if !matches!(side.pcurve, PcurveGeometry::Line(_)) {
            continue;
        }
        let [a, b] = ends[i];
        let direction = delta(a, b);
        let length_squared = direction[0].mul_add(direction[0], direction[1] * direction[1]);
        if length_squared <= 0.0 {
            continue;
        }
        let other = sides[1 - i];
        for t in stations[1 - i].into_iter().chain(other.range) {
            let offset = delta(a, other.uv(t)?);
            let q = side.range[0]
                + (side.range[1] - side.range[0])
                    * direction[0].mul_add(offset[0], direction[1] * offset[1])
                    / length_squared;
            if q >= side.range[0] && q <= side.range[1] {
                continue;
            }
            let mut pair = [t, t];
            pair[i] = q;
            if connected_in_world(sides, pair, &world, KNOWN_VERTEX_JOIN_MM).is_none() {
                continue;
            }
            if let Some(prior) = solution {
                if !sides.iter().enumerate().all(|(i, side)| {
                    (pair[i] - prior[i]).abs() <= PARAMETER_MERGE * (side.range[1] - side.range[0])
                }) {
                    return None;
                }
            } else {
                solution = Some(pair);
            }
        }
    }
    solution
}

fn world_projection(
    trim: &Trim,
    vertex: Point3,
    world: &impl Fn([f64; 2]) -> Option<Point3>,
    tolerance: f64,
) -> Option<f64> {
    closest(trim, tolerance, |t| {
        Some(world(trim.uv(t)?)?.distance(vertex))
    })
}

fn connected_in_world(
    sides: [&Trim; 2],
    pair: [f64; 2],
    world: &impl Fn([f64; 2]) -> Option<Point3>,
    tolerance: f64,
) -> Option<[f64; 2]> {
    let a = sides[0].uv(pair[0])?;
    let b = sides[1].uv(pair[1])?;
    let origin = world(a)?;
    for fraction in [0.25, 0.5, 0.75, 1.0] {
        let uv =
            std::array::from_fn(|axis| super::interval_parameter([a[axis], b[axis]], fraction));
        if world(uv)?.distance(origin) > tolerance {
            return None;
        }
    }
    Some(pair)
}

pub(super) fn known_vertex(
    sides: [&Trim; 2],
    vertex: Point3,
    world: impl Fn([f64; 2]) -> Option<Point3>,
    recover: bool,
) -> Option<[f64; 2]> {
    const WITNESSED_JOIN_MM: f64 = 0.00065;
    let project = |trim: &Trim| {
        let project = |trim: &Trim| world_projection(trim, vertex, &world, WITNESSED_JOIN_MM);
        project(trim).or_else(|| {
            if !recover {
                return None;
            }
            for range in [trim.mapped_range, trim.spline_range].into_iter().flatten() {
                if range != trim.range {
                    let mut extended = trim.clone();
                    extended.range = range;
                    if let Some(parameter) = project(&extended) {
                        return Some(parameter);
                    }
                }
            }
            None
        })
    };
    let pair = [project(sides[0])?, project(sides[1])?];
    let continuity = if recover {
        super::GAP_MM
    } else {
        WITNESSED_JOIN_MM
    };
    connected_in_world(sides, pair, &world, continuity)
}

pub(super) fn world_join(
    sides: [&Trim; 2],
    stations: [Option<f64>; 2],
    world: impl Fn([f64; 2]) -> Option<Point3>,
) -> Option<[f64; 2]> {
    world_join_with_tolerance(sides, stations, &world, KNOWN_VERTEX_JOIN_MM)
}

fn world_join_with_tolerance(
    sides: [&Trim; 2],
    stations: [Option<f64>; 2],
    world: &impl Fn([f64; 2]) -> Option<Point3>,
    tolerance: f64,
) -> Option<[f64; 2]> {
    let [a, b] = sides;
    let pair = match stations {
        [Some(s), Some(t)] => [s, t],
        [Some(s), None] => [s, world_projection(b, world(a.uv(s)?)?, world, tolerance)?],
        [None, Some(t)] => [world_projection(a, world(b.uv(t)?)?, world, tolerance)?, t],
        [None, None] => {
            let mut candidate: Option<[f64; 2]> = None;
            for (i, side) in sides.iter().enumerate() {
                for t in side.range {
                    let other = sides[1 - i];
                    let Some(point) = side.uv(t).and_then(world) else {
                        continue;
                    };
                    let Some(q) = world_projection(other, point, world, tolerance) else {
                        continue;
                    };
                    let mut pair = [q, q];
                    pair[i] = t;
                    if connected_in_world(sides, pair, world, tolerance).is_none() {
                        continue;
                    }
                    if let Some(prior) = candidate {
                        if !sides.iter().enumerate().all(|(i, side)| {
                            (pair[i] - prior[i]).abs()
                                <= PARAMETER_MERGE * (side.range[1] - side.range[0])
                        }) {
                            return None;
                        }
                    } else {
                        candidate = Some(pair);
                    }
                }
            }
            candidate?
        }
    };
    connected_in_world(sides, pair, world, tolerance)
}

pub(super) fn extended_splines(
    sides: [&Trim; 2],
    stations: [Option<f64>; 2],
    world: impl Fn([f64; 2]) -> Option<Point3>,
) -> Option<[f64; 2]> {
    const FITTED_JOIN_MM: f64 = 0.00095;
    let mut solution = None;
    for (i, side) in sides.into_iter().enumerate() {
        if side.mapped_range == Some(side.range) && sides[1 - i].spline_range.is_some() {
            continue;
        }
        let Some(range) = side.spline_range.filter(|range| *range != side.range) else {
            continue;
        };
        let mut extended = side.clone();
        extended.range = range;
        let mut pair = sides;
        pair[i] = &extended;
        let candidate = crossing(pair, stations)
            .or_else(|| crossing(pair, [None; 2]))
            .or_else(|| world_join_with_tolerance(pair, stations, &world, KNOWN_VERTEX_JOIN_MM))
            .or_else(|| world_join_with_tolerance(pair, [None; 2], &world, KNOWN_VERTEX_JOIN_MM));
        if let Some(candidate) = candidate {
            if let Some(prior) = solution {
                let agree = sides.into_iter().zip(candidate).zip(prior).all(
                    |((side, candidate), prior)| {
                        side.uv(candidate)
                            .zip(side.uv(prior))
                            .is_some_and(|(a, b)| distance(a, b) <= ANCHORED_UV_JOIN)
                    },
                );
                if !agree {
                    return None;
                }
            } else {
                solution = Some(candidate);
            }
        }
    }
    solution
        .or_else(|| world_join_with_tolerance(sides, stations, &world, FITTED_JOIN_MM))
        .or_else(|| world_join_with_tolerance(sides, [None; 2], &world, FITTED_JOIN_MM))
}
