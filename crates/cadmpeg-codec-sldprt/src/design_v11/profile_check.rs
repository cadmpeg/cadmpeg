// SPDX-License-Identifier: Apache-2.0
//! Whether the closed regions of a whole-sketch extrude's `Sketch` pick are
//! the profile the decoder resolved from the stored records (§6 "Sources";
//! §6.1 "Profiles and regions").
//!
//! A `Sketch` pick takes every closed region of its sketch, even-odd. The
//! stored profile loops are the contours `SolidWorks` extruded. The two differ
//! where the sketch closes curves the loops leave out, or where a loop curve
//! closes no region of the sketch. The check only reports: the pick stays
//! the `Sketch` pick.
//!
//! A curve closes where a closed chain of the sketch's non-construction
//! curves runs through it: a circle always, and a line or an arc where it is
//! no bridge of the graph whose nodes are curve ends joined within the
//! linear resolution. A dangling curve is a bridge and closes nothing. A
//! curve that crosses another mid-span is not joined to it there.

use std::collections::{HashMap, HashSet};
use std::f64::consts::TAU;

use cadmpeg_core::convert::truncate_f64_to_i64;
use cadmpeg_ir::design::ops::sketch::{EntityDefinition, Planar, PlanarCurve, SketchEntity};
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::sketches::{SketchEntityId, SketchEntityUse, SketchId};

use crate::brep::graph::LINEAR_RESOLUTION_MM;

#[cfg(test)]
mod tests;

/// The chords an arc of one full turn is sampled with.
const ARC_CHORDS_PER_TURN: f64 = 64.0;

/// The parts a line is cut into to sample its inner points.
const LINE_PARTS: u32 = 8;

/// The most point-against-edge tests one check makes before it stops telling
/// how the regions differ.
const CLASSIFY_BUDGET: usize = 1 << 24;

type Point = [f64; 2];

/// How a `Sketch` pick's regions differ from the stored profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Difference {
    /// The sketch closes a region outside the stored profile, which the pick
    /// extrudes and `SolidWorks` did not.
    Extra,
    /// A curve of the stored profile closes no region of the sketch, so the
    /// pick leaves out a region `SolidWorks` extruded.
    Missing,
    /// A loop inside the stored profile that touches none of its curves and
    /// that it leaves out: even-odd, the pick cuts a hole there.
    Hole,
    /// Curves inside the stored profile that join its curves and only split
    /// its regions: the same solid with internal edges.
    InternalEdge,
    /// Curves the stored profile leaves out close regions in a way the check
    /// does not tell.
    Other,
}

impl Difference {
    pub(super) fn text(self) -> &'static str {
        match self {
            Self::Extra => "extra region outside the stored profile",
            Self::Missing => "missing region, a stored profile curve closes no region",
            Self::Hole => "missing region, a loop inside the stored profile is a hole",
            Self::InternalEdge => "only internal edges inside the stored profile, the same solid",
            Self::Other => "curves outside the stored profile close regions",
        }
    }
}

/// The curve of each non-construction entity of `sketch` other than a
/// point, or `None` where one states no curve.
pub(super) fn curves<'a>(
    sketch: &SketchId,
    entities: &'a [SketchEntity<Planar>],
) -> Option<HashMap<&'a SketchEntityId, &'a PlanarCurve>> {
    let mut curves = HashMap::new();
    for entity in entities
        .iter()
        .filter(|entity| &entity.sketch == sketch && entity.construction != Some(true))
    {
        let curve = match &entity.definition {
            EntityDefinition::Free(curve) => curve,
            EntityDefinition::Defined { solved, .. } => solved.as_ref()?,
        };
        if !matches!(curve, PlanarCurve::Point(_)) {
            curves.insert(&entity.id, curve);
        }
    }
    Some(curves)
}

/// How the closed regions of the sketch whose curves are `curves` differ
/// from the stored profile `loops`, in order. Empty where they agree, where
/// no loop is stored, or where the sketch holds a curve other than a line,
/// an arc or a circle.
pub(super) fn differences(
    loops: &[Vec<SketchEntityUse>],
    curves: &HashMap<&SketchEntityId, &PlanarCurve>,
) -> Vec<Difference> {
    if loops.is_empty() {
        return Vec::new();
    }
    let Some(graph) = Graph::new(curves) else {
        return Vec::new();
    };
    let stored = loops
        .iter()
        .flatten()
        .map(|usage| &usage.entity)
        .collect::<HashSet<_>>();
    let mut found = Vec::new();
    if stored.iter().any(|entity| !graph.closing.contains(entity)) {
        found.push(Difference::Missing);
    }
    let mut extra = graph
        .closing
        .iter()
        .copied()
        .filter(|entity| !stored.contains(entity))
        .collect::<Vec<_>>();
    if !extra.is_empty() {
        extra.sort();
        found.extend(classify(loops, curves, &graph, &stored, &extra));
    }
    found.sort();
    found.dedup();
    found
}

/// The sketch's curves as a graph of their ends.
struct Graph<'a> {
    /// The end joints of each line and arc.
    ends: HashMap<&'a SketchEntityId, [usize; 2]>,
    joints: usize,
    /// The curves a closed chain runs through.
    closing: HashSet<&'a SketchEntityId>,
}

impl<'a> Graph<'a> {
    /// `None` where a curve is no line, arc or circle, an arc has no sweep,
    /// or an end lies too far out to join.
    fn new(curves: &HashMap<&'a SketchEntityId, &PlanarCurve>) -> Option<Self> {
        let mut sorted = curves
            .iter()
            .map(|(entity, curve)| (*entity, *curve))
            .collect::<Vec<_>>();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        let mut joints = Joints::default();
        let mut ends = HashMap::new();
        let mut closing = HashSet::new();
        let mut edges = Vec::new();
        for (entity, curve) in sorted {
            let [start, end] = match curve {
                PlanarCurve::Circle { .. } => {
                    closing.insert(entity);
                    continue;
                }
                PlanarCurve::Line { start, end } => [at(start), at(end)],
                PlanarCurve::Arc { .. } => arc_ends(curve)?,
                _ => return None,
            };
            let pair = [joints.at(start)?, joints.at(end)?];
            ends.insert(entity, pair);
            if pair[0] != pair[1] {
                edges.push((entity, pair));
            } else if matches!(curve, PlanarCurve::Arc { .. }) {
                closing.insert(entity);
            }
        }
        let bridge = bridges(&joints.points, &edges);
        closing.extend(
            edges
                .iter()
                .zip(bridge)
                .filter(|(_, bridge)| !bridge)
                .map(|((entity, _), _)| *entity),
        );
        Some(Self {
            ends,
            joints: joints.points.len(),
            closing,
        })
    }
}

/// Curve ends merged within the linear resolution, found through a grid of
/// cells that size.
#[derive(Default)]
struct Joints {
    cells: HashMap<[i64; 2], Vec<usize>>,
    points: Vec<Point>,
}

impl Joints {
    fn at(&mut self, point: Point) -> Option<usize> {
        let cell = |value: f64| truncate_f64_to_i64((value / LINEAR_RESOLUTION_MM).floor());
        let cell = [cell(point[0])?, cell(point[1])?];
        for du in -1..=1_i64 {
            for dv in -1..=1_i64 {
                let (Some(u), Some(v)) = (cell[0].checked_add(du), cell[1].checked_add(dv)) else {
                    continue;
                };
                if let Some(found) = self.cells.get(&[u, v]) {
                    if let Some(&joint) = found
                        .iter()
                        .find(|&&joint| meets(self.points[joint], point))
                    {
                        return Some(joint);
                    }
                }
            }
        }
        let joint = self.points.len();
        self.points.push(point);
        self.cells.entry(cell).or_default().push(joint);
        Some(joint)
    }
}

/// Whether each edge between distinct joints is a bridge: an edge no cycle
/// runs through (Tarjan's low-link, iterative).
fn bridges(joints: &[Point], edges: &[(&SketchEntityId, [usize; 2])]) -> Vec<bool> {
    let mut adjacent = vec![Vec::new(); joints.len()];
    for (index, (_, [a, b])) in edges.iter().enumerate() {
        adjacent[*a].push((*b, index));
        adjacent[*b].push((*a, index));
    }
    let mut found = vec![usize::MAX; joints.len()];
    let mut low = vec![0; joints.len()];
    let mut bridge = vec![false; edges.len()];
    let mut time = 0;
    for root in 0..joints.len() {
        if found[root] != usize::MAX {
            continue;
        }
        found[root] = time;
        low[root] = time;
        time += 1;
        // Each frame: the joint, the edge it was reached by, the next
        // adjacency to visit.
        let mut stack = vec![(root, usize::MAX, 0_usize)];
        while let Some(frame) = stack.last_mut() {
            let (joint, through) = (frame.0, frame.1);
            if let Some(&(next, edge)) = adjacent[joint].get(frame.2) {
                frame.2 += 1;
                if edge == through {
                    continue;
                }
                if found[next] == usize::MAX {
                    found[next] = time;
                    low[next] = time;
                    time += 1;
                    stack.push((next, edge, 0));
                } else {
                    low[joint] = low[joint].min(found[next]);
                }
            } else {
                stack.pop();
                if let Some(&(parent, _, _)) = stack.last() {
                    low[parent] = low[parent].min(low[joint]);
                    if low[joint] > found[parent] {
                        bridge[through] = true;
                    }
                }
            }
        }
    }
    bridge
}

/// How the closing curves `extra`, which the stored profile leaves out,
/// change its regions.
fn classify(
    loops: &[Vec<SketchEntityUse>],
    curves: &HashMap<&SketchEntityId, &PlanarCurve>,
    graph: &Graph<'_>,
    stored: &HashSet<&SketchEntityId>,
    extra: &[&SketchEntityId],
) -> Vec<Difference> {
    let Some(rings) = loops
        .iter()
        .map(|uses| ring(uses, curves))
        .collect::<Option<Vec<_>>>()
    else {
        return vec![Difference::Other];
    };
    let Some(sampled) = extra
        .iter()
        .map(|entity| Some((*entity, inner_points(curves.get(entity)?)?)))
        .collect::<Option<Vec<_>>>()
    else {
        return vec![Difference::Other];
    };
    let tests = sampled
        .iter()
        .map(|(_, points)| points.len())
        .sum::<usize>()
        .checked_mul(rings.iter().map(Vec::len).sum());
    if tests.is_none_or(|tests| tests > CLASSIFY_BUDGET) {
        return vec![Difference::Other];
    }
    let mut parent = (0..graph.joints).collect::<Vec<_>>();
    for entity in extra {
        if let Some(&[a, b]) = graph.ends.get(entity) {
            let (a, b) = (root(&mut parent, a), root(&mut parent, b));
            parent[a] = b;
        }
    }
    let stored_joints = stored
        .iter()
        .filter_map(|entity| graph.ends.get(entity))
        .flatten()
        .copied()
        .collect::<HashSet<_>>();
    // Each component, keyed by its root joint, or past the joints for a
    // curve without ends: whether it reaches outside the stored profile,
    // and its joints.
    let mut components = HashMap::<usize, (bool, HashSet<usize>)>::new();
    for (index, (entity, points)) in sampled.iter().enumerate() {
        let ends = graph.ends.get(entity);
        let key = match ends {
            Some(&[a, _]) => root(&mut parent, a),
            None => graph.joints + index,
        };
        let component = components.entry(key).or_default();
        component.0 |= points.iter().any(|&point| outside(&rings, point));
        component.1.extend(ends.into_iter().flatten().copied());
    }
    components
        .into_values()
        .map(|(outside, joints)| {
            if outside {
                return Difference::Extra;
            }
            match joints.intersection(&stored_joints).count() {
                0 => Difference::Hole,
                1 => Difference::Other,
                _ => Difference::InternalEdge,
            }
        })
        .collect()
}

fn root(parent: &mut [usize], mut joint: usize) -> usize {
    while parent[joint] != joint {
        parent[joint] = parent[parent[joint]];
        joint = parent[joint];
    }
    joint
}

/// Whether `point` lies off every ring and inside an even number of them:
/// outside the stored profile, even-odd.
fn outside(rings: &[Vec<Point>], point: Point) -> bool {
    !rings.iter().any(|ring| on_boundary(ring, point))
        && rings.iter().filter(|ring| contains(ring, point)).count() % 2 == 0
}

/// The closed polygon a stored loop traces, or `None` where a use names a
/// curve the sketch does not close as profile geometry, a circle is joined
/// to other curves, or consecutive uses do not meet.
fn ring(
    uses: &[SketchEntityUse],
    curves: &HashMap<&SketchEntityId, &PlanarCurve>,
) -> Option<Vec<Point>> {
    let mut points: Vec<Point> = Vec::new();
    for usage in uses {
        let curve = curves.get(&usage.entity)?;
        let mut traced = match curve {
            PlanarCurve::Circle { .. } if uses.len() == 1 => trace(curve)?,
            PlanarCurve::Line { start, end } => vec![at(start), at(end)],
            PlanarCurve::Arc { .. } => trace(curve)?,
            _ => return None,
        };
        if usage.reversed {
            traced.reverse();
        }
        if let Some(&last) = points.last() {
            if !meets(last, traced[0]) {
                return None;
            }
            points.pop();
        }
        points.extend(traced);
    }
    if points.len() < 2 || !meets(points[0], points[points.len() - 1]) {
        return None;
    }
    points.pop();
    Some(points)
}

/// The points of `curve` strictly between its ends, or around it for a
/// circle.
fn inner_points(curve: &PlanarCurve) -> Option<Vec<Point>> {
    Some(match curve {
        PlanarCurve::Line { start, end } => {
            let [start, end] = [at(start), at(end)];
            (1..LINE_PARTS)
                .map(|part| {
                    let t = f64::from(part) / f64::from(LINE_PARTS);
                    [
                        start[0] + t * (end[0] - start[0]),
                        start[1] + t * (end[1] - start[1]),
                    ]
                })
                .collect()
        }
        PlanarCurve::Arc { .. } => {
            let points = trace(curve)?;
            points[1..points.len() - 1].to_vec()
        }
        PlanarCurve::Circle { .. } => trace(curve)?,
        _ => return None,
    })
}

/// The points an arc passes from its start to its end, counterclockwise, or
/// one turn of a circle.
fn trace(curve: &PlanarCurve) -> Option<Vec<Point>> {
    let (center, radius, start, sweep) = match curve {
        PlanarCurve::Arc {
            center,
            radius,
            start,
            end,
        } => (center, radius, start.get(), sweep(start.get(), end.get())?),
        PlanarCurve::Circle { center, radius } => (center, radius, 0.0, TAU),
        _ => return None,
    };
    let (center, radius) = (at(center), radius.get());
    let chords = ((sweep / TAU) * ARC_CHORDS_PER_TURN).ceil().max(2.0);
    let mut points = Vec::new();
    let mut step = 0.0;
    while step <= chords {
        let (sin, cos) = (start + sweep * step / chords).portable_sin_cos();
        points.push([center[0] + radius * cos, center[1] + radius * sin]);
        step += 1.0;
    }
    Some(points)
}

fn arc_ends(curve: &PlanarCurve) -> Option<[Point; 2]> {
    let PlanarCurve::Arc {
        center,
        radius,
        start,
        end,
    } = curve
    else {
        return None;
    };
    sweep(start.get(), end.get())?;
    let (center, radius) = (at(center), radius.get());
    let point = |angle: f64| {
        let (sin, cos) = angle.portable_sin_cos();
        [center[0] + radius * cos, center[1] + radius * sin]
    };
    Some([point(start.get()), point(end.get())])
}

/// The counterclockwise sweep from `start` to `end`, `None` where it is
/// zero.
fn sweep(start: f64, end: f64) -> Option<f64> {
    let sweep = (end - start).rem_euclid(TAU);
    (sweep != 0.0).then_some(sweep)
}

fn at(point: &cadmpeg_ir::units::FinitePoint2) -> Point {
    let point = point.get();
    [point.u, point.v]
}

fn edges(ring: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    (0..ring.len()).map(|at| (ring[at], ring[(at + 1) % ring.len()]))
}

fn on_boundary(ring: &[Point], point: Point) -> bool {
    edges(ring).any(|(a, b)| {
        let along = [b[0] - a[0], b[1] - a[1]];
        let length = along[0] * along[0] + along[1] * along[1];
        let t = if length > 0.0 {
            (((point[0] - a[0]) * along[0] + (point[1] - a[1]) * along[1]) / length).clamp(0.0, 1.0)
        } else {
            0.0
        };
        meets(point, [a[0] + t * along[0], a[1] + t * along[1]])
    })
}

/// Whether `point` lies inside `ring`, by the crossings of a ray along +u.
fn contains(ring: &[Point], point: Point) -> bool {
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
    (a[0] - b[0]).portable_hypot(a[1] - b[1]) <= LINEAR_RESOLUTION_MM
}
