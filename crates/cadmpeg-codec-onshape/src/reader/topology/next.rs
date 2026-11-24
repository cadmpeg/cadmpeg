// SPDX-License-Identifier: Apache-2.0
//! Exact `UP_TO_NEXT` target proof: the first stored planar input face that every
//! ray of a closed convex line profile meets. Face footprints are line boundaries;
//! no sample, area or bounding box decides coverage.
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::document::Model;
use serde_json::{json, Value};

use super::super::{features, number, rows, text, Mapping};
use super::{cross, sub, unit, vector, State};

const ROUNDOFF_FACTOR: f64 = 128.0;
const WORK_LIMIT: usize = 1_000_000;

type Point = [f64; 2];
type Segment = (Point, Point);

/// Takes `cost` units from the one work budget of a proof.
fn charge(work: &mut usize, cost: Option<usize>) -> Mapping<()> {
    let cost = cost.ok_or("the exact extent work count exceeds the limit")?;
    *work = work
        .checked_sub(cost)
        .ok_or("the exact extent work budget is exhausted")?;
    Ok(())
}

/// Work of the quadratic profile checks: chain ordering and convexity.
fn profile_cost(corners: usize) -> Option<usize> {
    corners.checked_mul(corners)?.checked_mul(2)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

fn along([x, y, z]: [f64; 3], amount: f64, [dx, dy, dz]: [f64; 3]) -> [f64; 3] {
    [x + amount * dx, y + amount * dy, z + amount * dz]
}

fn roundoff(points: &[[f64; 3]]) -> f64 {
    ROUNDOFF_FACTOR * f64::EPSILON * points.iter().flatten().map(|v| v.abs()).fold(1.0, f64::max)
}

fn dot2(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn cross2(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn sub2(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}

fn gap(a: Point, b: Point) -> f64 {
    let [x, y] = sub2(a, b);
    x.hypot(y)
}

fn distance_to(point: Point, (a, b): Segment) -> f64 {
    let edge = sub2(b, a);
    let length = dot2(edge, edge);
    let t = if length > 0.0 {
        (dot2(sub2(point, a), edge) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    gap(point, [a[0] + t * edge[0], a[1] + t * edge[1]])
}

/// True when the segments intersect or lie within `tolerance` of each other.
fn touches(first: Segment, second: Segment, tolerance: f64) -> bool {
    let (a, b) = first;
    let (c, d) = second;
    let side = |p: Point, q: Point, r: Point| cross2(sub2(q, p), sub2(r, p));
    if side(a, b, c) * side(a, b, d) < 0.0 && side(c, d, a) * side(c, d, b) < 0.0 {
        return true;
    }
    [c, d]
        .into_iter()
        .any(|p| distance_to(p, first) <= tolerance)
        || [a, b]
            .into_iter()
            .any(|p| distance_to(p, second) <= tolerance)
}

/// Lower and upper bound of each coordinate: `[min x, max x, min y, max y]`.
fn bounds<'a>(points: impl IntoIterator<Item = &'a Point>) -> [f64; 4] {
    points.into_iter().fold(
        [
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ],
        |[x0, x1, y0, y1], p| [x0.min(p[0]), x1.max(p[0]), y0.min(p[1]), y1.max(p[1])],
    )
}

fn scale(segments: &[Segment]) -> f64 {
    ROUNDOFF_FACTOR
        * f64::EPSILON
        * segments
            .iter()
            .flat_map(|(a, b)| [a, b])
            .flatten()
            .map(|v| v.abs())
            .fold(1.0, f64::max)
}

/// Orders segments into one closed chain: each member's start meets the previous end.
/// Returns each member's index and whether it runs against its stored direction.
fn chain(segments: &[Segment]) -> Mapping<Vec<(usize, bool)>> {
    let tolerance = scale(segments);
    let mut remaining: Vec<usize> = (0..segments.len()).collect();
    let first = remaining.pop().ok_or("a stored boundary has no curves")?;
    let (start, mut end) = segments
        .get(first)
        .copied()
        .ok_or("a stored boundary curve is missing")?;
    let mut members = vec![(first, false)];
    while gap(start, end) > tolerance {
        let mut matches = Vec::new();
        for (position, &index) in remaining.iter().enumerate() {
            let (a, b) = segments
                .get(index)
                .copied()
                .ok_or("a stored boundary curve is missing")?;
            if gap(end, a) <= tolerance {
                matches.push((position, false));
            }
            if gap(end, b) <= tolerance {
                matches.push((position, true));
            }
        }
        let [(position, reversed)] = matches.as_slice() else {
            return Err("the stored boundary is open or contradictory".into());
        };
        let index = remaining.remove(*position);
        let (a, b) = segments
            .get(index)
            .copied()
            .ok_or("a stored boundary curve is missing")?;
        end = if *reversed { a } else { b };
        members.push((index, *reversed));
    }
    if !remaining.is_empty() {
        return Err("a stored boundary has disconnected curves".into());
    }
    Ok(members)
}

/// A closed polygon in chain order.
#[derive(Debug)]
struct Ring(Vec<Point>);

impl Ring {
    fn new(segments: &[Segment], order: &[(usize, bool)]) -> Mapping<Self> {
        order
            .iter()
            .map(|&(index, reversed)| {
                segments
                    .get(index)
                    .map(|&(a, b)| if reversed { b } else { a })
                    .ok_or_else(|| "a stored boundary curve is missing".to_owned())
            })
            .collect::<Mapping<Vec<_>>>()
            .map(Self)
    }

    fn edges(&self) -> impl Iterator<Item = Segment> + '_ {
        self.0
            .iter()
            .copied()
            .zip(self.0.iter().copied().cycle().skip(1))
    }

    fn tolerance(&self) -> f64 {
        ROUNDOFF_FACTOR
            * f64::EPSILON
            * self.0.iter().flatten().map(|v| v.abs()).fold(1.0, f64::max)
    }

    /// Even-odd containment; callers apply it only to points off every boundary.
    fn contains(&self, [x, y]: Point) -> bool {
        self.edges()
            .filter(|&([_, ay], [_, by])| (ay > y) != (by > y))
            .filter(|&([ax, ay], [bx, by])| x < ax + (y - ay) / (by - ay) * (bx - ax))
            .count()
            % 2
            == 1
    }

    fn first(&self) -> Mapping<Point> {
        self.0
            .first()
            .copied()
            .ok_or_else(|| "a stored boundary is empty".into())
    }

    fn touches(&self, other: &Self) -> bool {
        let tolerance = self.tolerance().max(other.tolerance());
        self.edges()
            .any(|a| other.edges().any(|b| touches(a, b, tolerance)))
    }

    /// A simple polygon: at least three nondegenerate edges, none meeting another
    /// except at a shared vertex.
    fn admit(&self) -> Mapping<()> {
        let tolerance = self.tolerance();
        let edges: Vec<Segment> = self.edges().collect();
        if edges.len() < 3 {
            return Err("a stored boundary needs at least three edges".into());
        }
        for (i, &first) in edges.iter().enumerate() {
            if gap(first.0, first.1) <= tolerance {
                return Err("a stored boundary has a degenerate edge".into());
            }
            for (j, &second) in edges.iter().enumerate().skip(i + 1) {
                let bad = if j == i + 1 {
                    distance_to(first.0, second) <= tolerance
                        || distance_to(second.1, first) <= tolerance
                } else if i == 0 && j + 1 == edges.len() {
                    distance_to(first.1, second) <= tolerance
                        || distance_to(second.0, first) <= tolerance
                } else {
                    touches(first, second, tolerance)
                };
                if bad {
                    return Err("a stored boundary overlaps or crosses itself".into());
                }
            }
        }
        Ok(())
    }
}

/// A face's outer boundary and holes, each admitted as a simple polygon.
#[derive(Debug)]
struct Outline {
    outer: Ring,
    holes: Vec<Ring>,
}

impl Outline {
    fn new(loops: Vec<(bool, Ring)>) -> Mapping<Self> {
        let mut outer = None;
        let mut holes = Vec::new();
        for (is_outer, ring) in loops {
            ring.admit()?;
            if is_outer {
                if outer.replace(ring).is_some() {
                    return Err("the stored target repeats its outer boundary".into());
                }
            } else {
                holes.push(ring);
            }
        }
        let outer = outer.ok_or("the stored target has no outer boundary")?;
        for (i, hole) in holes.iter().enumerate() {
            if outer.touches(hole) || !outer.contains(hole.first()?) {
                return Err("a stored target hole lies outside its outer boundary".into());
            }
            for other in holes.iter().take(i) {
                if hole.touches(other)
                    || hole.contains(other.first()?)
                    || other.contains(hole.first()?)
                {
                    return Err("the stored target hole boundaries touch or nest".into());
                }
            }
        }
        Ok(Self { outer, holes })
    }

    /// Closed-region overlap with a profile. A touch counts as overlap.
    fn overlaps(&self, profile: &Ring) -> Mapping<bool> {
        if std::iter::once(&self.outer)
            .chain(&self.holes)
            .any(|ring| ring.touches(profile))
            || profile.contains(self.outer.first()?)
        {
            return Ok(true);
        }
        let point = profile.first()?;
        Ok(self.outer.contains(point) && !self.holes.iter().any(|hole| hole.contains(point)))
    }

    /// Whether the closed convex profile lies on the face's material.
    fn covers(&self, profile: &Ring) -> Mapping<bool> {
        let tolerance = self.outer.tolerance().max(profile.tolerance());
        let mut convex = true;
        let mut covered = true;
        for (start, end) in self.outer.edges() {
            let normal = [start[1] - end[1], end[0] - start[0]];
            let offset = dot2(start, normal);
            let side = |point: &Point| dot2(*point, normal) - offset;
            let (low, high) = self
                .outer
                .0
                .iter()
                .map(side)
                .fold((0.0_f64, 0.0_f64), |(l, h), v| (l.min(v), h.max(v)));
            let limit = tolerance * normal[0].hypot(normal[1]);
            if low >= -limit && high > limit {
                covered &= profile.0.iter().all(|point| side(point) >= -limit);
            } else if high <= limit && low < -limit {
                covered &= profile.0.iter().all(|point| side(point) <= limit);
            } else {
                convex = false;
            }
        }
        if convex {
            if !covered {
                return Ok(false);
            }
        } else if self.outer.touches(profile) || !self.outer.contains(profile.first()?) {
            return Ok(false);
        }
        for hole in &self.holes {
            if hole.touches(profile)
                || profile.contains(hole.first()?)
                || hole.contains(profile.first()?)
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Signed travel to a plane as an affine function of a profile point.
#[derive(Clone, Copy)]
struct Affine {
    constant: f64,
    axis: Point,
}

impl Affine {
    /// The least and greatest value over the profile.
    fn range(self, profile: &Ring) -> [f64; 2] {
        profile
            .0
            .iter()
            .map(|&point| self.constant + dot2(self.axis, point))
            .fold([f64::INFINITY, f64::NEG_INFINITY], |[l, h], v| {
                [l.min(v), h.max(v)]
            })
    }

    fn minus(self, other: Self) -> Self {
        Self {
            constant: self.constant - other.constant,
            axis: sub2(self.axis, other.axis),
        }
    }
}

struct Face {
    id: String,
    /// Travel to the plane; absent when the plane is parallel to the travel.
    time: Option<Affine>,
    /// Signed offset of a parallel plane across the profile.
    parallel_side: [f64; 2],
    /// The face's footprint in the profile frame, for a plane met by the travel.
    outline: Option<Outline>,
    /// Footprint bounds `[min u, max u, min v, max v]`.
    bounds: [f64; 4],
}

/// The sketch frame of the profile: origin, axes and travel direction.
struct Frame {
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    travel: [f64; 3],
}

impl Frame {
    fn project(&self, point: [f64; 3]) -> Point {
        let offset = sub(point, self.origin);
        [dot(offset, self.x), dot(offset, self.y)]
    }
}

struct Profile {
    frame: Frame,
    ring: Ring,
    /// The profile corners and their stored ends in world coordinates, in chain order.
    corners: Vec<[f64; 3]>,
    ends: Vec<[f64; 3]>,
    normal: [f64; 3],
}

fn read_profile(
    feature: &Value,
    profile: &Value,
    model: &Model,
    work: &mut usize,
) -> Mapping<Profile> {
    if text(profile, "kind")? != "sketch" {
        return Err("needs one closed source sketch boundary".into());
    }
    let sketch_id = text(profile, "value")?;
    let sketch = model
        .sketches
        .iter()
        .find(|sketch| sketch.id.as_str() == sketch_id)
        .ok_or("source sketch is missing")?;
    let wire = json::to_value(sketch).map_err(|_| "source sketch is unreadable")?;
    let frame = wire
        .pointer("/placement/frame")
        .ok_or("source frame is missing")?;
    let origin = vector(frame.get("origin").ok_or("source origin is missing")?, 1.0)?;
    let normal = unit(vector(
        frame.get("normal").ok_or("source normal is missing")?,
        1.0,
    )?)?;
    let x = unit(vector(
        frame.get("u_axis").ok_or("source x axis is missing")?,
        1.0,
    )?)?;
    let y = cross(normal, x);
    let reversed = features::enabled(feature, "oppositeDirection")?;
    let travel = normal.map(|v| if reversed { -v } else { v });
    let mut segments = Vec::new();
    for entity in model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch.as_str() == sketch_id && !entity.construction)
    {
        let wire = json::to_value(entity).map_err(|_| "source curve is unreadable")?;
        let geometry = wire.get("geometry").ok_or("source geometry is missing")?;
        if text(geometry, "kind")? != "line" {
            return Err("needs stored straight source edges".into());
        }
        let local = |key| -> Mapping<Point> {
            let p = geometry
                .get(key)
                .ok_or("stored source endpoint is missing")?;
            Ok([number(p, "u")?, number(p, "v")?])
        };
        segments.push((local("start")?, local("end")?));
    }
    charge(work, profile_cost(segments.len()))?;
    let order = chain(&segments)?;
    let ring = Ring::new(&segments, &order)?;
    let lift = |[u, v]: Point| along(along(origin, u, x), v, y);
    let mut corners = Vec::new();
    let mut ends = Vec::new();
    for &(index, flipped) in &order {
        let (a, b) = segments
            .get(index)
            .copied()
            .ok_or("a source curve is missing")?;
        let (a, b) = if flipped { (b, a) } else { (a, b) };
        corners.push(lift(a));
        ends.push(lift(b));
    }
    Ok(Profile {
        frame: Frame {
            origin,
            x,
            y,
            travel,
        },
        ring,
        corners,
        ends,
        normal,
    })
}

/// A closed boundary of stored endpoints that bends one way only.
fn convex(points: &[[f64; 3]], ends: &[[f64; 3]], normal: [f64; 3]) -> Mapping<()> {
    if points.len() < 3 {
        return Err("needs a bounded closed convex boundary".into());
    }
    let tolerance = roundoff(points);
    let mut sense = None;
    for (index, start) in points.iter().enumerate() {
        let next = points
            .get((index + 1) % points.len())
            .ok_or("boundary endpoint is missing")?;
        let end = ends.get(index).ok_or("boundary endpoint is missing")?;
        if sub(*next, *end).into_iter().any(|v| v.abs() > tolerance) {
            return Err("needs a closed boundary of stored endpoints".into());
        }
        let direction = unit(sub(*end, *start))?;
        for candidate in points {
            let side = dot(cross(direction, sub(*candidate, *start)), normal);
            if !side.is_finite() {
                return Err("boundary exceeds the finite range".into());
            }
            if side.abs() > tolerance {
                let positive = side > 0.0;
                if sense.is_some_and(|sense| sense != positive) {
                    return Err("needs a convex boundary of stored lines".into());
                }
                sense = Some(positive);
            }
        }
    }
    if sense.is_none() {
        return Err("needs a nondegenerate closed boundary".into());
    }
    Ok(())
}

/// One stored input face as a plane, its travel function and its line outline.
fn read_face(
    id: &str,
    face: &Value,
    state: &State,
    profile: &Profile,
    work: &mut usize,
) -> Mapping<Face> {
    let frame = &profile.frame;
    let surface = face
        .get("surface")
        .ok_or("a stored target surface is missing")?;
    if text(surface, "type")? != "PLANE" {
        return Err("a stored input surface has no supported analytic intersection yet".into());
    }
    let origin = vector(
        surface
            .get("origin")
            .ok_or("a stored target plane origin is missing")?,
        1000.0,
    )?;
    let normal = unit(vector(
        surface
            .get("normal")
            .ok_or("a stored target plane normal is missing")?,
        1.0,
    )?)?;
    let denominator = dot(normal, frame.travel);
    if denominator != 0.0 && denominator.abs() <= ROUNDOFF_FACTOR * f64::EPSILON {
        return Err("a stored input plane is nearly parallel to the travel direction; its intersection is unresolved".into());
    }
    let signed = Affine {
        constant: dot(normal, sub(origin, frame.origin)),
        axis: [-dot(normal, frame.x), -dot(normal, frame.y)],
    };
    let time = (denominator != 0.0).then(|| Affine {
        constant: signed.constant / denominator,
        axis: signed.axis.map(|a| a / denominator),
    });
    // An algebraic chart for the stored loops; never a sketch attachment frame.
    let reference = if normal[0].abs() < normal[1].abs() {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let face_x = unit(cross(normal, reference))?;
    let face_y = cross(normal, face_x);
    let chart = |point: [f64; 3]| {
        let offset = sub(point, origin);
        [dot(offset, face_x), dot(offset, face_y)]
    };
    let loops = rows(face, "loops")?;
    let count = loops.iter().try_fold(0usize, |total, lp| {
        total
            .checked_add(rows(lp, "coedges")?.len())
            .ok_or_else(|| "the exact extent boundary count exceeds the limit".to_owned())
    })?;
    let cost = count
        .checked_mul(count)
        .and_then(|cost| cost.checked_mul(3))
        .and_then(|cost| {
            count
                .checked_mul(profile.ring.0.len())
                .and_then(|n| n.checked_mul(4))
                .and_then(|n| cost.checked_add(n))
        })
        .ok_or("the exact extent work count exceeds the limit")?;
    *work = work
        .checked_sub(cost)
        .ok_or("the exact extent work budget is exhausted")?;
    let mut charted = Vec::new();
    let mut footprint = Vec::new();
    for lp in loops {
        let is_outer = lp
            .get("isOuter")
            .and_then(Value::as_bool)
            .ok_or("the stored target loop has no outer/hole role")?;
        let mut chart_segments = Vec::new();
        let mut footprint_segments = Vec::new();
        for coedge in rows(lp, "coedges")? {
            let edge = state
                .edges
                .get(text(coedge, "edgeId")?)
                .ok_or("a stored target boundary edge is missing")?;
            let carrier = edge
                .get("curve")
                .ok_or("a stored target boundary carrier is missing")?;
            if text(carrier, "type")? != "LINE" {
                return Err(
                    "a stored target boundary needs a supported exact curve intersection".into(),
                );
            }
            let geometry = edge
                .get("geometry")
                .ok_or("a stored target boundary interval is missing")?;
            let start = vector(
                geometry
                    .get("startPoint")
                    .ok_or("a stored target boundary start is missing")?,
                1000.0,
            )?;
            let end = vector(
                geometry
                    .get("endPoint")
                    .ok_or("a stored target boundary end is missing")?,
                1000.0,
            )?;
            let line_origin = vector(
                carrier
                    .get("origin")
                    .ok_or("a stored line origin is missing")?,
                1000.0,
            )?;
            let line_direction = unit(vector(
                carrier
                    .get("direction")
                    .ok_or("a stored line direction is missing")?,
                1.0,
            )?)?;
            let tolerance = roundoff(&[start, end, line_origin, origin]);
            for endpoint in [start, end] {
                if dot(normal, sub(endpoint, origin)).abs() > tolerance
                    || cross(sub(endpoint, line_origin), line_direction)
                        .into_iter()
                        .any(|residual| residual.abs() > tolerance)
                {
                    return Err(
                        "a stored target boundary contradicts its plane or line carrier".into(),
                    );
                }
            }
            chart_segments.push((chart(start), chart(end)));
            footprint_segments.push((frame.project(start), frame.project(end)));
        }
        *work = work
            .checked_sub(chart_segments.len())
            .ok_or("the exact extent work budget is exhausted")?;
        let order = chain(&chart_segments)?;
        charted.push((is_outer, Ring::new(&chart_segments, &order)?));
        footprint.push((is_outer, Ring::new(&footprint_segments, &order)?));
    }
    if charted.is_empty() {
        return Err("the stored target face has no closed boundaries".into());
    }
    // Admission of the stored loops uses the face chart, valid for every plane.
    Outline::new(charted)?;
    let bounds = bounds(footprint.iter().flat_map(|(_, ring)| &ring.0));
    let outline = if time.is_some() {
        Some(Outline::new(footprint)?)
    } else {
        None
    };
    Ok(Face {
        id: id.to_owned(),
        time,
        parallel_side: signed.range(&profile.ring),
        outline,
        bounds,
    })
}

pub(super) fn prove(
    state: &State,
    feature: &Value,
    profile: &Value,
    model: &Model,
) -> Mapping<Value> {
    if !matches!(
        features::value(feature, "operationType"),
        Some("ADD" | "REMOVE")
    ) {
        return Err("the stored operation does not select an existing target body".into());
    }
    let mut work = WORK_LIMIT;
    let profile = read_profile(feature, profile, model, &mut work)?;
    convex(&profile.corners, &profile.ends, profile.normal)?;
    let reach = bounds(&profile.ring.0);
    let tolerance =
        ROUNDOFF_FACTOR * f64::EPSILON * reach.iter().map(|v| v.abs()).fold(1.0_f64, f64::max);
    let mut faces = Vec::new();
    let mut sheets = Vec::new();
    for (id, face) in &state.faces {
        let sheet = !state.solid_faces.contains(id);
        if sheet && !state.other_faces.contains(id) {
            // The body is consumed by a composite and is not a live part.
            continue;
        }
        let face = read_face(id, face, state, &profile, &mut work)?;
        if face.bounds[1] < reach[0] - tolerance
            || face.bounds[0] > reach[1] + tolerance
            || face.bounds[3] < reach[2] - tolerance
            || face.bounds[2] > reach[3] + tolerance
        {
            continue;
        }
        if let Some(outline) = &face.outline {
            if !outline.overlaps(&profile.ring)? {
                continue;
            }
        }
        if sheet {
            // The next face ends on a part; a live body of another type is not a part.
            sheets.push(face);
        } else {
            faces.push(face);
        }
    }
    charge(
        &mut work,
        faces
            .len()
            .checked_mul(faces.len())
            .and_then(|n| n.checked_mul(profile.ring.0.len()))
            .and_then(|n| n.checked_mul(2)),
    )?;
    for candidate in &faces {
        let (Some(time), Some(outline)) = (candidate.time, &candidate.outline) else {
            continue;
        };
        if time.range(&profile.ring)[0] <= tolerance || !outline.covers(&profile.ring)? {
            continue;
        }
        let blocks = |other: &Face| match other.time {
            Some(other_time) => {
                other_time.range(&profile.ring)[1] > tolerance
                    && other_time.minus(time).range(&profile.ring)[0] < -tolerance
            }
            None => other.parallel_side[0] < -tolerance && other.parallel_side[1] > tolerance,
        };
        for other in &sheets {
            charge(&mut work, Some(1))?;
            if blocks(other) {
                return Err(
                    "a live non-solid body face lies before the covering face on the travel".into(),
                );
            }
        }
        let mut first = true;
        for other in &faces {
            charge(&mut work, Some(1))?;
            if blocks(other) {
                first = false;
                break;
            }
        }
        if first {
            return Ok(
                json!({"kind":"historical","value":{"state":state.row.get("id"),"native":"Onshape UP_TO_NEXT","faces":[candidate.id]}}),
            );
        }
    }
    Err(
        "no stored input face proves closed-boundary coverage and the first positive intersection"
            .into(),
    )
}

#[cfg(test)]
mod tests;
