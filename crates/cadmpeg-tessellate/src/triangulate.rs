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

fn visible(points: &[Uv], from: usize, to: usize, rings: &[&[usize]]) -> bool {
    let (a, b) = (points[from], points[to]);
    for ring in rings {
        for at in 0..ring.len() {
            let (c, d) = (ring[at], ring[(at + 1) % ring.len()]);
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
            .max_by(|a, b| points[*a.1][0].total_cmp(&points[*b.1][0]))
            .map_or(0, |(position, _)| position)
    };
    holes.sort_by(|a, b| {
        let x = |ring: &Vec<usize>| points[ring[rightmost(ring)]][0];
        x(b).total_cmp(&x(a))
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
            gap(a).total_cmp(&gap(b))
        });
        let others: Vec<&[usize]> = holes[at + 1..].iter().map(Vec::as_slice).collect();
        let mut rings: Vec<&[usize]> = vec![outer.as_slice(), hole.as_slice()];
        rings.extend(others);
        let target = candidates
            .into_iter()
            .find(|position| visible(points, from, outer[*position], &rings))
            .ok_or_else(|| Refusal::new("an inner loop has no bridge to the outer loop"))?;
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
            if ring
                .iter()
                .all(|p| *p == a || *p == b || *p == c || !inside(points, a, b, c, *p))
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
