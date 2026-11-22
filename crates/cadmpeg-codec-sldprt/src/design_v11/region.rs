// SPDX-License-Identifier: Apache-2.0
//! The regions of a sketch's profile loops (§6.1, "Profiles and regions").
//!
//! The decoder's profile loops of a sketch are closed chains of its curves
//! with no nesting stated. A loop that an even number of other loops enclose
//! is the outer ring of a region, and a loop that an odd number enclose is a
//! hole of the region of the innermost loop around it. An outer ring runs
//! counterclockwise about the sketch normal and a hole clockwise.

use std::f64::consts::TAU;

use cadmpeg_ir::design::ops::sketch::PlanarCurve;
use cadmpeg_ir::design::selection::{BoundaryUse, SketchRegion};
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::sketches::{SketchEntityId, SketchEntityUse};

use crate::brep::graph::LINEAR_RESOLUTION_MM;

#[cfg(test)]
mod tests;

/// The chords an arc of one full turn is sampled with.
const ARC_CHORDS_PER_TURN: f64 = 64.0;

type Point = [f64; 2];

/// One profile loop as the closed polygon its curves trace.
struct Ring<'a> {
    uses: &'a [SketchEntityUse],
    points: Vec<Point>,
    /// Twice the signed area: positive where the loop runs counterclockwise.
    area: f64,
    bounds: [Point; 2],
}

/// The regions of the profile `loops`, whose non-construction curves
/// `curve` gives. The reason where a loop names a curve `curve` does not
/// give, joins a circle to other curves, is open, or encloses no area, or
/// where two loops cross or coincide.
pub(super) fn regions<'a>(
    loops: &[Vec<SketchEntityUse>],
    curve: impl Fn(&SketchEntityId) -> Option<&'a PlanarCurve>,
) -> Result<Vec<SketchRegion>, &'static str> {
    let rings = loops
        .iter()
        .map(|uses| ring(uses, &curve))
        .collect::<Result<Vec<_>, _>>()?;
    let mut holders = vec![Vec::new(); rings.len()];
    for (at, inner) in rings.iter().enumerate() {
        for (other, outer) in rings.iter().enumerate() {
            if other != at && encloses(outer, inner)? {
                holders[at].push(other);
            }
        }
    }
    let depth = |at: usize| holders[at].len();
    let mut regions = Vec::new();
    let mut outers = Vec::new();
    for (at, ring) in rings.iter().enumerate() {
        if depth(at) % 2 == 0 {
            outers.push(at);
            regions.push(SketchRegion {
                outer: boundary(ring, true),
                holes: Vec::new(),
            });
        }
    }
    for (at, ring) in rings.iter().enumerate() {
        if depth(at) % 2 == 1 {
            let mut innermost = holders[at]
                .iter()
                .copied()
                .filter(|&holder| depth(holder) + 1 == depth(at));
            let (Some(holder), None) = (innermost.next(), innermost.next()) else {
                return Err("its profile loops do not nest");
            };
            let region = outers
                .iter()
                .position(|&outer| outer == holder)
                .ok_or("its profile loops do not nest")?;
            regions[region].holes.push(boundary(ring, false));
        }
    }
    Ok(regions)
}

/// The uses of `ring` in the order that runs it counterclockwise where
/// `outer`, else clockwise.
fn boundary(ring: &Ring<'_>, outer: bool) -> Vec<BoundaryUse> {
    let uses = ring.uses.iter().map(|usage| BoundaryUse {
        entity: usage.entity.clone(),
        fragment: None,
    });
    if (ring.area > 0.0) == outer {
        uses.collect()
    } else {
        uses.rev().collect()
    }
}

fn ring<'a, 'c>(
    uses: &'a [SketchEntityUse],
    curve: &impl Fn(&SketchEntityId) -> Option<&'c PlanarCurve>,
) -> Result<Ring<'a>, &'static str> {
    let mut points = Vec::new();
    let mut length = 0.0;
    for usage in uses {
        let mut traced = trace(
            curve(&usage.entity)
                .ok_or("its profile holds a construction curve or one its sketch does not state")?,
            uses.len() == 1,
        )?;
        if usage.reversed {
            traced.reverse();
        }
        if let Some(&last) = points.last() {
            if !meets(last, traced[0]) {
                return Err("a profile loop is open");
            }
            points.pop();
        }
        length += traced
            .windows(2)
            .map(|pair| distance(pair[0], pair[1]))
            .sum::<f64>();
        points.extend(traced);
    }
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else {
        return Err("a profile loop is empty");
    };
    if !meets(first, last) {
        return Err("a profile loop is open");
    }
    points.pop();
    let area = (0..points.len())
        .map(|at| {
            let [a, b] = [points[at], points[(at + 1) % points.len()]];
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>();
    if area.abs() <= LINEAR_RESOLUTION_MM * length {
        return Err("a profile loop encloses no area");
    }
    let mut bounds = [points[0], points[0]];
    for point in &points {
        for axis in 0..2 {
            bounds[0][axis] = bounds[0][axis].min(point[axis]);
            bounds[1][axis] = bounds[1][axis].max(point[axis]);
        }
    }
    Ok(Ring {
        uses,
        points,
        area,
        bounds,
    })
}

/// The points `curve` passes from its start to its end; a circle, which
/// only a loop of `alone` takes, runs one counterclockwise turn.
fn trace(curve: &PlanarCurve, alone: bool) -> Result<Vec<Point>, &'static str> {
    let at = |point: &cadmpeg_ir::units::FinitePoint2| {
        let point = point.get();
        [point.u, point.v]
    };
    let turn = |center: Point, radius: f64, start: f64, sweep: f64| {
        let chords = ((sweep / TAU) * ARC_CHORDS_PER_TURN).ceil().max(2.0);
        let mut points = Vec::new();
        let mut step = 0.0;
        while step <= chords {
            let (sin, cos) = (start + sweep * step / chords).portable_sin_cos();
            points.push([center[0] + radius * cos, center[1] + radius * sin]);
            step += 1.0;
        }
        points
    };
    Ok(match curve {
        PlanarCurve::Line { start, end } => vec![at(start), at(end)],
        PlanarCurve::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let sweep = (end.get() - start.get()).rem_euclid(TAU);
            if sweep == 0.0 {
                return Err("an arc of its profile has no sweep");
            }
            turn(at(center), radius.get(), start.get(), sweep)
        }
        PlanarCurve::Circle { center, radius } if alone => turn(at(center), radius.get(), 0.0, TAU),
        PlanarCurve::Circle { .. } => return Err("a profile loop joins a circle to other curves"),
        _ => return Err("its profile holds a curve other than a line, an arc or a circle"),
    })
}

/// Whether `outer` encloses `inner`: every point of `inner` off `outer`'s
/// boundary lies inside it. The reason where some lie inside and some
/// outside, or none lies off the boundary.
fn encloses(outer: &Ring<'_>, inner: &Ring<'_>) -> Result<bool, &'static str> {
    let apart = (0..2).any(|axis| {
        inner.bounds[0][axis] > outer.bounds[1][axis] + LINEAR_RESOLUTION_MM
            || inner.bounds[1][axis] < outer.bounds[0][axis] - LINEAR_RESOLUTION_MM
    });
    if apart {
        return Ok(false);
    }
    let (mut inside, mut outside) = (false, false);
    for &point in &inner.points {
        if on_boundary(outer, point) {
            continue;
        }
        if contains(outer, point) {
            inside = true;
        } else {
            outside = true;
        }
        if inside && outside {
            return Err("its profile loops cross");
        }
    }
    if !inside && !outside {
        return Err("two of its profile loops coincide");
    }
    Ok(inside)
}

fn edges<'r>(ring: &'r Ring<'_>) -> impl Iterator<Item = (Point, Point)> + 'r {
    (0..ring.points.len()).map(|at| (ring.points[at], ring.points[(at + 1) % ring.points.len()]))
}

fn on_boundary(ring: &Ring<'_>, point: Point) -> bool {
    edges(ring).any(|(a, b)| {
        let along = [b[0] - a[0], b[1] - a[1]];
        let length = along[0] * along[0] + along[1] * along[1];
        let t = if length > 0.0 {
            (((point[0] - a[0]) * along[0] + (point[1] - a[1]) * along[1]) / length).clamp(0.0, 1.0)
        } else {
            0.0
        };
        distance(point, [a[0] + t * along[0], a[1] + t * along[1]]) <= LINEAR_RESOLUTION_MM
    })
}

/// Whether `point` lies inside `ring`, by the crossings of a ray along +u.
fn contains(ring: &Ring<'_>, point: Point) -> bool {
    edges(ring)
        .filter(|(a, b)| {
            (a[1] > point[1]) != (b[1] > point[1])
                && point[0] < a[0] + (point[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
        })
        .count()
        % 2
        == 1
}

fn meets(a: Point, b: Point) -> bool {
    distance(a, b) <= LINEAR_RESOLUTION_MM
}

fn distance(a: Point, b: Point) -> f64 {
    (a[0] - b[0]).portable_hypot(a[1] - b[1])
}
