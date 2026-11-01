// SPDX-License-Identifier: Apache-2.0
//! The trimmed parameter domain of a face as polygons with holes.
//!
//! Every loop is oriented so that the face lies on its left. A loop that
//! closes in the parameter plane bounds a polygon or a hole. A loop that
//! closes only after a whole period of one parameter wraps around the
//! carrier; two such loops bound a strip, and one bounds the strip between
//! it and a pole or a domain end. A strip is cut open along a seam that no
//! hole crosses.

use cadmpeg_ir::math::Point3;

use crate::face::Refusal;
use crate::plane::{contains, signed_area, Uv};

/// Ratio to a period, or to the extent of a loop, below which a closure
/// displacement is zero.
const CLOSURE_RATIO: f64 = 1e-6;
/// Ratio to a period cell's area within which a ring encloses the whole
/// cell.
const CELL_RATIO: f64 = 1e-6;

/// One polygon vertex.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Node {
    pub(crate) uv: Uv,
    /// The model point of a boundary sample.
    pub(crate) point: Option<Point3>,
    /// The segment from this node to the next one is not a face boundary: a
    /// seam cut or a pole line, which refinement may split.
    pub(crate) open: bool,
}

/// One polygon with holes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Region {
    pub(crate) outer: Vec<Node>,
    pub(crate) holes: Vec<Vec<Node>>,
}

/// A loop as a ring of nodes with its closure displacement.
#[derive(Debug, Clone)]
pub(crate) struct Ring {
    pub(crate) nodes: Vec<Node>,
    pub(crate) closure: Uv,
}

/// The carrier facts the domain needs.
#[derive(Debug, Clone, Default)]
pub(crate) struct Frame {
    pub(crate) periods: [Option<f64>; 2],
    pub(crate) domain: Option<[[f64; 2]; 2]>,
    /// Singular points of the face, as poles and apexes, with their
    /// parameters; a wrapping strip with one loop runs to one of them or to
    /// a domain end.
    pub(crate) poles: Vec<(Uv, Option<Point3>)>,
}

enum Kind {
    Closed,
    Wrap { axis: usize },
}

fn uvs(nodes: &[Node]) -> Vec<Uv> {
    nodes.iter().map(|node| node.uv).collect()
}

fn kind(ring: &Ring, frame: &Frame) -> Result<Kind, Refusal> {
    let points = uvs(&ring.nodes);
    let extent = (0..2)
        .map(|axis| {
            let low = points
                .iter()
                .map(|uv| uv[axis])
                .fold(f64::INFINITY, f64::min);
            let high = points
                .iter()
                .map(|uv| uv[axis])
                .fold(f64::NEG_INFINITY, f64::max);
            high - low
        })
        .fold(1.0, f64::max);
    let zero = |value: f64, scale: f64| value.abs() <= CLOSURE_RATIO * scale;
    if zero(ring.closure[0], extent) && zero(ring.closure[1], extent) {
        return Ok(Kind::Closed);
    }
    for axis in 0..2 {
        let Some(period) = frame.periods[axis] else {
            continue;
        };
        let turns = (ring.closure[axis] / period).round();
        if turns != 0.0
            && zero(ring.closure[axis] - turns * period, period)
            && zero(ring.closure[1 - axis], extent)
        {
            return Ok(Kind::Wrap { axis });
        }
    }
    Err(Refusal::new(format!(
        "a loop does not close in the parameter plane: it ends ({}, {}) from its start",
        ring.closure[0], ring.closure[1]
    )))
}

/// Maps a point into the frame of a strip that wraps along `axis`: the first
/// coordinate runs along the wrap, and the face lies above a rising loop.
fn to_strip(axis: usize, uv: Uv) -> Uv {
    if axis == 0 {
        uv
    } else {
        [uv[1], -uv[0]]
    }
}

fn from_strip(axis: usize, q: Uv) -> Uv {
    if axis == 0 {
        q
    } else {
        [-q[1], q[0]]
    }
}

/// `nodes` in the opposite direction, each segment flag moved with its
/// segment.
pub(crate) fn reversed(nodes: &[Node]) -> Vec<Node> {
    let count = nodes.len();
    (0..count)
        .map(|at| {
            let source = count - 1 - at;
            Node {
                open: nodes[(source + count - 1) % count].open,
                ..nodes[source]
            }
        })
        .collect()
}

/// The regions of a face, from its oriented rings.
pub(crate) fn regions(rings: Vec<Ring>, frame: &Frame) -> Result<Vec<Region>, Refusal> {
    let mut closed = Vec::new();
    let mut wraps: Vec<(usize, Ring)> = Vec::new();
    for ring in rings {
        if ring.nodes.len() < 2 {
            continue;
        }
        match kind(&ring, frame)? {
            Kind::Closed => closed.push(ring.nodes),
            Kind::Wrap { axis } => wraps.push((axis, ring)),
        }
    }
    let mut outers = Vec::new();
    let mut holes = Vec::new();
    // A ring around a whole period cell of a surface closed in both
    // parameters bounds the whole surface, whichever way it runs.
    let cell = match frame.periods {
        [Some(u), Some(v)] => Some(u * v),
        _ => None,
    };
    for mut nodes in closed {
        if nodes.len() < 3 {
            continue;
        }
        // Twice the signed area, as `signed_area` returns it.
        let area = signed_area(&uvs(&nodes));
        let whole = cell.is_some_and(|cell| (area.abs() / 2.0 - cell).abs() <= CELL_RATIO * cell);
        if whole && area < 0.0 {
            nodes = reversed(&nodes);
        }
        if whole || area > 0.0 {
            outers.push(nodes);
        } else {
            holes.push(nodes);
        }
    }
    if let Some((axis, _)) = wraps.first() {
        let axis = *axis;
        if wraps.iter().any(|(other, _)| *other != axis) {
            return Err(Refusal::new(
                "loops wrap around both parameters of the surface",
            ));
        }
        let rings = wraps.into_iter().map(|(_, ring)| ring).collect();
        outers.extend(strips(rings, axis, frame, &holes)?);
    }
    if outers.is_empty() {
        let Some(domain) = frame.domain else {
            return Err(Refusal::new(
                "the face has no outer loop and its surface no bounded domain",
            ));
        };
        outers.push(rectangle(domain, frame, &holes));
    }
    assign(outers, holes, frame)
}

/// The whole parameter domain as one polygon whose sides are open.
fn rectangle(domain: [[f64; 2]; 2], frame: &Frame, holes: &[Vec<Node>]) -> Vec<Node> {
    let mut low = [domain[0][0], domain[1][0]];
    for (axis, value) in low.iter_mut().enumerate() {
        if let Some(period) = frame.periods[axis] {
            if let Some(first) = holes.first().and_then(|hole| hole.first()) {
                // A seam through the middle of the first hole's opposite side.
                *value = first.uv[axis] - period / 2.0;
            }
        }
    }
    let high = [
        low[0] + (domain[0][1] - domain[0][0]),
        low[1] + (domain[1][1] - domain[1][0]),
    ];
    [
        [low[0], low[1]],
        [high[0], low[1]],
        [high[0], high[1]],
        [low[0], high[1]],
    ]
    .into_iter()
    .map(|uv| Node {
        uv,
        point: None,
        open: true,
    })
    .collect()
}

/// Gives each hole to the outer polygon that contains it, moving it by whole
/// periods where that places it inside.
fn assign(
    outers: Vec<Vec<Node>>,
    holes: Vec<Vec<Node>>,
    frame: &Frame,
) -> Result<Vec<Region>, Refusal> {
    let mut regions: Vec<Region> = outers
        .into_iter()
        .map(|outer| Region {
            outer,
            holes: Vec::new(),
        })
        .collect();
    let shifts = |axis: usize| -> Vec<f64> {
        frame.periods[axis].map_or_else(|| vec![0.0], |period| vec![0.0, -period, period])
    };
    for hole in holes {
        let Some(probe) = hole.first().map(|node| node.uv) else {
            continue;
        };
        let mut placed = false;
        'search: for du in shifts(0) {
            for dv in shifts(1) {
                let moved = [probe[0] + du, probe[1] + dv];
                if let Some(region) = regions
                    .iter_mut()
                    .find(|region| contains(&uvs(&region.outer), moved))
                {
                    let mut hole = hole.clone();
                    for node in &mut hole {
                        node.uv = [node.uv[0] + du, node.uv[1] + dv];
                    }
                    region.holes.push(hole);
                    placed = true;
                    break 'search;
                }
            }
        }
        if !placed {
            return Err(Refusal::new("an inner loop lies outside every outer loop"));
        }
    }
    Ok(regions)
}

/// The strips between wrapping rings that run along `axis`.
fn strips(
    rings: Vec<Ring>,
    axis: usize,
    frame: &Frame,
    holes: &[Vec<Node>],
) -> Result<Vec<Vec<Node>>, Refusal> {
    let period = frame.periods[axis]
        .ok_or_else(|| Refusal::new("a loop wraps around a parameter that has no period"))?;
    // In the strip frame each ring rises (+x) with the face above it, or
    // falls with the face below it.
    let mut entries: Vec<(f64, bool, Vec<Node>)> = Vec::new();
    for ring in rings {
        let nodes: Vec<Node> = ring
            .nodes
            .iter()
            .map(|node| Node {
                uv: to_strip(axis, node.uv),
                ..*node
            })
            .collect();
        let rising = to_strip(axis, ring.closure)[0] > 0.0;
        let count = f64::from(
            u32::try_from(nodes.len()).map_err(|_| Refusal::new("a loop has too many samples"))?,
        );
        let mean = nodes.iter().map(|node| node.uv[1]).sum::<f64>() / count;
        entries.push((mean, rising, nodes));
    }
    entries.sort_by(|a, b| a.0.total_cmp(&b.0));
    let hole_ranges: Vec<[f64; 2]> = holes
        .iter()
        .map(|hole| {
            let xs = hole.iter().map(|node| to_strip(axis, node.uv)[0]);
            let low = xs.clone().fold(f64::INFINITY, f64::min);
            let high = xs.fold(f64::NEG_INFINITY, f64::max);
            [low, high]
        })
        .collect();
    let other = 1 - axis;
    let mut bounds: Vec<(f64, Option<Point3>)> = frame
        .poles
        .iter()
        .map(|(uv, point)| (to_strip(axis, *uv)[1], *point))
        .collect();
    if let (Some(domain), None) = (frame.domain, frame.periods[other]) {
        for end in domain[other] {
            let mut probe = [0.0; 2];
            probe[other] = end;
            bounds.push((to_strip(axis, probe)[1], None));
        }
    }
    let mut out = Vec::new();
    let mut at = 0;
    while at < entries.len() {
        let (mean, rising, ref lower) = entries[at];
        if rising {
            let upper = entries
                .get(at + 1)
                .filter(|entry| !entry.1)
                .map(|entry| entry.2.clone());
            let upper = match upper {
                Some(upper) => {
                    at += 2;
                    Upper::Ring(upper)
                }
                None => {
                    at += 1;
                    let bound = bounds
                        .iter()
                        .filter(|(value, _)| *value > mean)
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                        .copied()
                        .ok_or_else(|| {
                            Refusal::new("a wrapping loop bounds a strip with no far side")
                        })?;
                    Upper::Bound(bound)
                }
            };
            out.push(strip(lower.clone(), upper, period, &hole_ranges)?);
        } else {
            // A falling ring with nothing below: the strip runs down to a
            // bound. Turning the frame half around makes it rising.
            at += 1;
            let bound = bounds
                .iter()
                .filter(|(value, _)| *value < mean)
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .copied()
                .ok_or_else(|| Refusal::new("a wrapping loop bounds a strip with no far side"))?;
            let turned: Vec<Node> = lower
                .iter()
                .map(|node| Node {
                    uv: [-node.uv[0], -node.uv[1]],
                    ..*node
                })
                .collect();
            let ranges: Vec<[f64; 2]> = hole_ranges
                .iter()
                .map(|[low, high]| [-high, -low])
                .collect();
            let polygon = strip(turned, Upper::Bound((-bound.0, bound.1)), period, &ranges)?;
            out.push(
                polygon
                    .into_iter()
                    .map(|node| Node {
                        uv: [-node.uv[0], -node.uv[1]],
                        ..node
                    })
                    .collect(),
            );
        }
    }
    Ok(out
        .into_iter()
        .map(|polygon| {
            polygon
                .into_iter()
                .map(|node| Node {
                    uv: from_strip(axis, node.uv),
                    ..node
                })
                .collect()
        })
        .collect())
}

enum Upper {
    Ring(Vec<Node>),
    Bound((f64, Option<Point3>)),
}

/// Whether the seam at `x` crosses a hole.
fn crosses(x: f64, period: f64, ranges: &[[f64; 2]]) -> bool {
    ranges.iter().any(|[low, high]| {
        let turns = ((x - low) / period).floor();
        let moved = x - turns * period;
        moved > *low && moved < *high
    })
}

/// The polygon between a rising ring and the ring or bound above it, in the
/// strip frame, cut along a seam.
fn strip(
    lower: Vec<Node>,
    upper: Upper,
    period: f64,
    holes: &[[f64; 2]],
) -> Result<Vec<Node>, Refusal> {
    let start = (0..lower.len())
        .find(|at| !crosses(lower[*at].uv[0], period, holes))
        .ok_or_else(|| Refusal::new("every seam of a strip crosses a hole"))?;
    let mut lower = lower;
    lower.rotate_left(start);
    // The rotated ring is continuous only after the wrapped part moves up a
    // period.
    let first_x = lower[0].uv[0];
    let mut unrolled = Vec::new();
    let mut offset = 0.0;
    for (position, node) in lower.iter().enumerate() {
        if position > 0
            && node.uv[0] + offset
                < unrolled.last().map_or(first_x, |last: &Node| last.uv[0]) - period / 2.0
        {
            offset += period;
        }
        unrolled.push(Node {
            uv: [node.uv[0] + offset, node.uv[1]],
            ..*node
        });
    }
    let x0 = first_x;
    let mut end = lower[0];
    end.uv[0] = x0 + period;
    end.open = true;
    unrolled.push(end);
    match upper {
        Upper::Ring(mut ring) => {
            let target = x0 + period;
            let nearest = (0..ring.len())
                .min_by(|a, b| {
                    let gap = |at: usize| {
                        let x = ring[at].uv[0];
                        (x - target - ((x - target) / period).round() * period).abs()
                    };
                    gap(*a).total_cmp(&gap(*b))
                })
                .ok_or_else(|| Refusal::new("a wrapping loop has no samples"))?;
            ring.rotate_left(nearest);
            let shift = ((target - ring[0].uv[0]) / period).round() * period;
            let mut offset = shift;
            let mut previous = ring[0].uv[0] + shift;
            for (position, node) in ring.iter().enumerate() {
                let mut x = node.uv[0] + offset;
                if position > 0 && x > previous + period / 2.0 {
                    offset -= period;
                    x -= period;
                }
                previous = x;
                unrolled.push(Node {
                    uv: [x, node.uv[1]],
                    ..*node
                });
            }
            let mut last = ring[0];
            last.uv[0] = ring[0].uv[0] + shift - period;
            last.open = true;
            unrolled.push(last);
        }
        Upper::Bound((at, point)) => {
            for x in [x0 + period, x0] {
                unrolled.push(Node {
                    uv: [x, at],
                    point,
                    open: true,
                });
            }
        }
    }
    Ok(unrolled)
}

#[cfg(test)]
mod tests;
