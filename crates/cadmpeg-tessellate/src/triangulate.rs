// SPDX-License-Identifier: Apache-2.0
//! Triangulation of a polygon with holes by ear clipping.
//!
//! Each hole is joined to the outer ring by a bridge to a visible vertex, so
//! the region becomes one ring that repeats the bridge ends. Ears are then
//! clipped until three vertices remain.

use crate::face::Refusal;
use crate::plane::{cross2, signed_area, Uv};

/// The rings of one region by vertex index: the outer ring and its holes.
pub(crate) struct Rings<'a> {
    pub(crate) points: &'a [Uv],
    pub(crate) outer: Vec<usize>,
    pub(crate) holes: Vec<Vec<usize>>,
}

fn ring_area(points: &[Uv], ring: &[usize]) -> f64 {
    signed_area(&ring.iter().map(|at| points[*at]).collect::<Vec<_>>())
}

/// Whether the segments `a`–`b` and `c`–`d` cross at a point interior to
/// both.
fn segments_cross(a: Uv, b: Uv, c: Uv, d: Uv) -> bool {
    let d1 = cross2(a, b, c);
    let d2 = cross2(a, b, d);
    let d3 = cross2(c, d, a);
    let d4 = cross2(c, d, b);
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// Whether `p` lies on the segment `a`–`b` strictly between its ends.
fn on_segment(a: Uv, b: Uv, p: Uv) -> bool {
    if p == a || p == b || cross2(a, b, p) != 0.0 {
        return false;
    }
    let along = (p[0] - a[0]) * (b[0] - a[0]) + (p[1] - a[1]) * (b[1] - a[1]);
    let length = (b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2);
    along > 0.0 && along < length
}

/// Whether the bridge `from`–`to` crosses no ring segment and passes
/// through no ring vertex, which would make the joined ring touch itself.
fn visible(points: &[Uv], from: usize, to: usize, rings: &[&[usize]]) -> bool {
    let (a, b) = (points[from], points[to]);
    for ring in rings {
        for at in 0..ring.len() {
            let (c, d) = (ring[at], ring[(at + 1) % ring.len()]);
            if on_segment(a, b, points[c]) {
                return false;
            }
            if [c, d].contains(&from) || [c, d].contains(&to) {
                continue;
            }
            if segments_cross(a, b, points[c], points[d]) {
                return false;
            }
        }
    }
    true
}

/// Whether `point` lies in the wedge at `at` that turns counterclockwise
/// from the ray toward `next` to the ray toward `previous`: the interior of
/// a counterclockwise ring at that vertex.
fn in_wedge(at: Uv, next: Uv, previous: Uv, point: Uv) -> bool {
    let convex = cross2(at, next, previous) >= 0.0;
    let after_next = cross2(at, next, point) > 0.0;
    let before_previous = cross2(at, point, previous) > 0.0;
    if convex {
        after_next && before_previous
    } else {
        after_next || before_previous
    }
}

/// Joins each hole to `outer` with a bridge.
fn bridge(
    points: &[Uv],
    mut outer: Vec<usize>,
    mut holes: Vec<Vec<usize>>,
) -> Result<Vec<usize>, Refusal> {
    // Holes farthest to the right join first, so later bridges see them.
    let rightmost = |ring: &[usize]| -> usize {
        ring.iter()
            .enumerate()
            .max_by(|a, b| {
                points[*a.1][0]
                    .total_cmp(&points[*b.1][0])
                    .then(a.1.cmp(b.1))
            })
            .map_or(0, |(position, _)| position)
    };
    holes.sort_by(|a, b| {
        let x = |ring: &Vec<usize>| points[ring[rightmost(ring)]][0];
        x(b).total_cmp(&x(a)).then(a[0].cmp(&b[0]))
    });
    for at in 0..holes.len() {
        let hole = holes[at].clone();
        let from_position = rightmost(&hole);
        let from = hole[from_position];
        let mut candidates: Vec<usize> = (0..outer.len()).collect();
        candidates.sort_by(|a, b| {
            let gap = |position: &usize| {
                let p = points[outer[*position]];
                let q = points[from];
                (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)
            };
            gap(a).total_cmp(&gap(b)).then(a.cmp(b))
        });
        let others: Vec<&[usize]> = holes[at + 1..].iter().map(Vec::as_slice).collect();
        let mut rings: Vec<&[usize]> = vec![outer.as_slice(), hole.as_slice()];
        rings.extend(others);
        let target = candidates
            .into_iter()
            .find(|position| visible(points, from, outer[*position], &rings))
            .ok_or_else(|| Refusal::new("an inner loop has no bridge to the outer loop"))?;
        // A vertex that earlier bridges repeat appears once per visit; the
        // bridge joins the visit whose interior wedge holds its direction.
        let target = (0..outer.len())
            .filter(|position| outer[*position] == outer[target])
            .find(|position| {
                let count = outer.len();
                let at = points[outer[*position]];
                let next = points[outer[(position + 1) % count]];
                let previous = points[outer[(position + count - 1) % count]];
                in_wedge(at, next, previous, points[from])
            })
            .unwrap_or(target);
        let mut joined = outer[..=target].to_vec();
        joined.extend(hole[from_position..].iter().copied());
        joined.extend(hole[..=from_position].iter().copied());
        joined.extend(outer[target..].iter().copied());
        outer = joined;
    }
    Ok(outer)
}

fn inside(points: &[Uv], a: usize, b: usize, c: usize, p: usize) -> bool {
    let (pa, pb, pc, pp) = (points[a], points[b], points[c], points[p]);
    if pp == pa || pp == pb || pp == pc {
        return false;
    }
    cross2(pa, pb, pp) >= 0.0 && cross2(pb, pc, pp) >= 0.0 && cross2(pc, pa, pp) >= 0.0
}

/// The counterclockwise triangles of a region.
pub(crate) fn triangulate(rings: Rings<'_>) -> Result<Vec<[usize; 3]>, Refusal> {
    let points = rings.points;
    let mut outer = rings.outer;
    if ring_area(points, &outer) < 0.0 {
        outer.reverse();
    }
    let holes: Vec<Vec<usize>> = rings
        .holes
        .into_iter()
        .filter(|hole| hole.len() >= 3)
        .map(|mut hole| {
            if ring_area(points, &hole) > 0.0 {
                hole.reverse();
            }
            hole
        })
        .collect();
    let mut ring = bridge(points, outer, holes)?;
    ring.dedup_by(|a, b| points[*a] == points[*b]);
    while ring.len() > 3 && points[ring[0]] == points[ring[ring.len() - 1]] {
        ring.pop();
    }
    let mut triangles = Vec::new();
    while ring.len() > 3 {
        let count = ring.len();
        let mut best: Option<(usize, f64)> = None;
        let mut clipped = None;
        for at in 0..count {
            let (a, b, c) = (
                ring[(at + count - 1) % count],
                ring[at],
                ring[(at + 1) % count],
            );
            let turn = cross2(points[a], points[b], points[c]);
            if turn <= 0.0 {
                continue;
            }
            if best.is_none_or(|(_, largest)| turn > largest) {
                best = Some((at, turn));
            }
            // A bridged ring touches itself at its bridge ends, so an ear's
            // diagonal must also stay clear of the rest of the ring.
            if ring
                .iter()
                .all(|p| *p == a || *p == b || *p == c || !inside(points, a, b, c, *p))
                && (0..count).all(|k| {
                    let (start, end) = (ring[k], ring[(k + 1) % count]);
                    [start, end].contains(&a)
                        || [start, end].contains(&c)
                        || !segments_cross(points[a], points[c], points[start], points[end])
                })
            {
                clipped = Some(at);
                break;
            }
        }
        // A ring with no clean ear is degenerate: clip its widest convex
        // corner, or its first corner, so that clipping ends.
        let at = clipped.or(best.map(|(at, _)| at)).unwrap_or(0);
        let (a, b, c) = (
            ring[(at + count - 1) % count],
            ring[at],
            ring[(at + 1) % count],
        );
        if cross2(points[a], points[b], points[c]) > 0.0 {
            triangles.push([a, b, c]);
        }
        ring.remove(at);
    }
    if ring.len() == 3 && cross2(points[ring[0]], points[ring[1]], points[ring[2]]) > 0.0 {
        triangles.push([ring[0], ring[1], ring[2]]);
    }
    Ok(triangles)
}

#[cfg(test)]
mod tests;
