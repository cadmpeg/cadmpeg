// SPDX-License-Identifier: Apache-2.0
//! Whether the closed regions of a whole-sketch extrude's `Sketch` pick are
//! the profile the decoder resolved from the stored records (§6 "Sources",
//! "Loops"; §6.1 "Profiles and regions").
//!
//! A `SolidWorks` extrude states no loop rule, so its `Sketch` pick takes the
//! regions its sketch's closed loops bound alternately. The stored profile
//! loops are the contours `SolidWorks` extruded. The two differ where the
//! sketch closes curves the loops leave out, or where a loop curve closes no
//! region of the sketch. The check only reports: the pick stays the `Sketch`
//! pick.
//!
//! The sketch's non-construction curves are cut where they touch or cross,
//! into pieces whose ends are joints merged within the linear resolution. A
//! piece that lies along another piece between the same joints repeats it
//! and bounds nothing of its own. A piece closes where a closed chain of
//! pieces runs through it: a circle no curve touches always, and another
//! piece where it is no bridge of the graph of joints. A dangling curve is a
//! bridge and closes nothing.

use std::collections::{HashMap, HashSet};
use std::f64::consts::{PI, TAU};

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

/// The most pairs of curves one check tests for contacts.
const PAIR_BUDGET: usize = 1 << 24;

type Point = [f64; 2];

/// How a `Sketch` pick's regions differ from the stored profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Difference {
    /// The sketch closes a region outside the stored profile, which the pick
    /// extrudes.
    Extra,
    /// A curve of the stored profile closes no region of the sketch, so the
    /// pick leaves out a region of the stored profile.
    Missing,
    /// Curves inside the stored profile that touch none of its curves close a
    /// loop it leaves out: the pick cuts a hole there.
    Hole,
    /// Curves inside the stored profile that join its curves and only split
    /// its regions: the same solid with internal edges.
    InternalEdge,
    /// A curve outside the stored loops lies along a curve of them.
    Duplicate,
    /// Curves outside the stored loops, or parts of them past a contact,
    /// close no region, so the pick leaves them out.
    Dangling,
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
            Self::Duplicate => "duplicate curve, a curve repeats a stored profile curve",
            Self::Dangling => "dangling curves or curve ends, which close no region",
            Self::Other => "curves outside the stored profile close regions",
        }
    }
}

/// Why the check did not compare a `Sketch` pick's regions with the stored
/// profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Unchecked {
    /// The sketch holds no curve other than points and construction curves,
    /// so the pick takes no region.
    NoProfileCurve,
    /// The decoder resolved no profile loop for the sketch.
    NoStoredLoop,
    /// A curve is no line, arc or circle.
    CurveKind,
    /// A curve states no solved form, an arc has no sweep, or a point lies
    /// too far out to join.
    Unplaced,
    /// The sketch holds more curves than the check tests.
    Budget,
}

impl Unchecked {
    pub(super) fn text(self) -> &'static str {
        match self {
            Self::NoProfileCurve => "the sketch holds no profile curve",
            Self::NoStoredLoop => "no stored profile loop",
            Self::CurveKind => "a curve other than a line, an arc or a circle",
            Self::Unplaced => "a curve the check cannot place",
            Self::Budget => "more curves than the check compares",
        }
    }
}

/// The curve of each non-construction entity of `sketch` other than a
/// point.
pub(super) fn curves<'a>(
    sketch: &SketchId,
    entities: &'a [SketchEntity<Planar>],
) -> Result<HashMap<&'a SketchEntityId, &'a PlanarCurve>, Unchecked> {
    let mut curves = HashMap::new();
    for entity in entities
        .iter()
        .filter(|entity| &entity.sketch == sketch && entity.construction != Some(true))
    {
        let curve = match &entity.definition {
            EntityDefinition::Free(curve) => curve,
            EntityDefinition::Defined { solved, .. } => {
                solved.as_ref().ok_or(Unchecked::Unplaced)?
            }
        };
        if !matches!(curve, PlanarCurve::Point(_)) {
            curves.insert(&entity.id, curve);
        }
    }
    Ok(curves)
}

/// How the closed regions of the sketch whose curves are `curves` differ
/// from the stored profile `loops`, in order; empty where they agree.
pub(super) fn check(
    loops: &[Vec<SketchEntityUse>],
    curves: &HashMap<&SketchEntityId, &PlanarCurve>,
) -> Result<Vec<Difference>, Unchecked> {
    if curves.is_empty() {
        return Err(Unchecked::NoProfileCurve);
    }
    if loops.is_empty() {
        return Err(Unchecked::NoStoredLoop);
    }
    let stored = loops
        .iter()
        .flatten()
        .map(|usage| &usage.entity)
        .collect::<HashSet<_>>();
    let arrangement = Arrangement::new(curves, |entity| stored.contains(entity))?;
    let closing = arrangement.closing(|_| true);
    let is_stored = |entity: usize| stored.contains(arrangement.ids[entity]);
    let mut closes = vec![false; arrangement.ids.len()];
    let mut found = Vec::new();
    for (piece, closing) in arrangement.pieces.iter().zip(&closing) {
        closes[piece.entity] |= *closing;
        if let Some(twin) = piece.repeats {
            if !is_stored(piece.entity) && is_stored(arrangement.pieces[twin].entity) {
                found.push(Difference::Duplicate);
            }
        }
    }
    if stored.iter().any(|entity| {
        arrangement
            .index
            .get(entity)
            .is_none_or(|&index| !closes[index])
    }) {
        found.push(Difference::Missing);
    }
    if arrangement
        .pieces
        .iter()
        .zip(&closing)
        .any(|(piece, closing)| !closing && piece.repeats.is_none() && !is_stored(piece.entity))
    {
        found.push(Difference::Dangling);
    }
    let extra = (0..arrangement.pieces.len())
        .filter(|&piece| closing[piece] && !is_stored(arrangement.pieces[piece].entity))
        .collect::<Vec<_>>();
    if !extra.is_empty() {
        found.extend(classify(loops, &arrangement, &is_stored, &extra));
    }
    found.sort();
    found.dedup();
    Ok(found)
}

/// The curves of `state_zero` that the curves of `live` close through:
/// without them, a curve of `live` that closes in the whole sketch would
/// close nothing. Empty where the check cannot place the curves.
pub(super) fn needed<'a>(
    live: &HashMap<&'a SketchEntityId, &'a PlanarCurve>,
    state_zero: &HashMap<&'a SketchEntityId, &'a PlanarCurve>,
) -> HashSet<&'a SketchEntityId> {
    if state_zero.is_empty() {
        return HashSet::new();
    }
    let mut all = live.clone();
    all.extend(state_zero.iter().map(|(entity, curve)| (*entity, *curve)));
    let Ok(arrangement) = Arrangement::new(&all, |entity| live.contains_key(entity)) else {
        return HashSet::new();
    };
    let is_live = arrangement
        .ids
        .iter()
        .map(|entity| live.contains_key(entity))
        .collect::<Vec<_>>();
    let live_closing = |present: &[bool]| {
        let closing = arrangement.closing(|entity| present[entity]);
        let mut closes = vec![false; arrangement.ids.len()];
        for (piece, closing) in arrangement.pieces.iter().zip(closing) {
            closes[piece.entity] |= closing && is_live[piece.entity];
        }
        closes
    };
    let target = live_closing(&vec![true; arrangement.ids.len()]);
    let keeps = |closes: &[bool]| target.iter().zip(closes).all(|(want, has)| !want || *has);
    if keeps(&live_closing(&is_live)) {
        return HashSet::new();
    }
    if state_zero
        .len()
        .checked_mul(arrangement.pieces.len())
        .is_none_or(|trials| trials > CLASSIFY_BUDGET)
    {
        return HashSet::new();
    }
    let mut present = vec![true; arrangement.ids.len()];
    for entity in (0..arrangement.ids.len()).filter(|&entity| !is_live[entity]) {
        present[entity] = false;
        if !keeps(&live_closing(&present)) {
            present[entity] = true;
        }
    }
    (0..arrangement.ids.len())
        .filter(|&entity| !is_live[entity] && present[entity])
        .map(|entity| arrangement.ids[entity])
        .collect()
}

/// A line, an arc or a circle of the sketch.
#[derive(Debug, Clone, Copy)]
enum Shape {
    Line([Point; 2]),
    /// Counterclockwise from `start` through `sweep`.
    Arc {
        center: Point,
        radius: f64,
        start: f64,
        sweep: f64,
    },
    Circle {
        center: Point,
        radius: f64,
    },
}

impl Shape {
    fn new(curve: &PlanarCurve) -> Result<Self, Unchecked> {
        Ok(match curve {
            PlanarCurve::Line { start, end } => Self::Line([at(start), at(end)]),
            PlanarCurve::Arc {
                center,
                radius,
                start,
                end,
            } => Self::Arc {
                center: at(center),
                radius: radius.get(),
                start: start.get(),
                sweep: sweep(start.get(), end.get()).ok_or(Unchecked::Unplaced)?,
            },
            PlanarCurve::Circle { center, radius } => Self::Circle {
                center: at(center),
                radius: radius.get(),
            },
            _ => return Err(Unchecked::CurveKind),
        })
    }

    /// The point `fraction` of the way along.
    fn at(&self, fraction: f64) -> Point {
        match *self {
            Self::Line([a, b]) => [
                a[0] + fraction * (b[0] - a[0]),
                a[1] + fraction * (b[1] - a[1]),
            ],
            Self::Arc {
                center,
                radius,
                start,
                sweep,
            } => polar(center, radius, start + fraction * sweep),
            Self::Circle { center, radius } => polar(center, radius, fraction * TAU),
        }
    }

    fn ends(&self) -> Option<[Point; 2]> {
        match self {
            Self::Circle { .. } => None,
            _ => Some([self.at(0.0), self.at(1.0)]),
        }
    }

    /// How far along `point`'s nearest place on the curve lies.
    fn fraction(&self, point: Point) -> f64 {
        match *self {
            Self::Line([a, b]) => {
                let along = [b[0] - a[0], b[1] - a[1]];
                let length = along[0] * along[0] + along[1] * along[1];
                if length > 0.0 {
                    (((point[0] - a[0]) * along[0] + (point[1] - a[1]) * along[1]) / length)
                        .clamp(0.0, 1.0)
                } else {
                    0.0
                }
            }
            Self::Arc {
                center,
                start,
                sweep,
                ..
            } => (turn(center, point, start) / sweep).min(1.0),
            Self::Circle { center, .. } => turn(center, point, 0.0) / TAU,
        }
    }

    fn distance(&self, point: Point) -> f64 {
        match *self {
            Self::Line(_) => {
                let nearest = self.at(self.fraction(point));
                (point[0] - nearest[0]).portable_hypot(point[1] - nearest[1])
            }
            Self::Arc {
                center,
                start,
                sweep,
                ..
            } if turn(center, point, start) > sweep => {
                let [a, b] = [self.at(0.0), self.at(1.0)];
                (point[0] - a[0])
                    .portable_hypot(point[1] - a[1])
                    .min((point[0] - b[0]).portable_hypot(point[1] - b[1]))
            }
            Self::Arc { center, radius, .. } | Self::Circle { center, radius } => {
                ((point[0] - center[0]).portable_hypot(point[1] - center[1]) - radius).abs()
            }
        }
    }

    /// The part between `from` and `to` of the way along; past `1.0` for a
    /// circle's part across its start.
    fn part(&self, from: f64, to: f64) -> Self {
        match *self {
            Self::Line(_) => Self::Line([self.at(from), self.at(to)]),
            Self::Arc {
                center,
                radius,
                start,
                sweep,
            } => Self::Arc {
                center,
                radius,
                start: start + from * sweep,
                sweep: (to - from) * sweep,
            },
            Self::Circle { center, radius } => Self::Arc {
                center,
                radius,
                start: from * TAU,
                sweep: (to - from) * TAU,
            },
        }
    }

    /// The points the curve passes from its start to its end, or one turn
    /// of a circle.
    fn trace(&self) -> Vec<Point> {
        let turns = match *self {
            Self::Line([a, b]) => return vec![a, b],
            Self::Arc { sweep, .. } => sweep / TAU,
            Self::Circle { .. } => 1.0,
        };
        let chords = (turns * ARC_CHORDS_PER_TURN).ceil().max(2.0);
        let mut points = Vec::new();
        let mut step = 0.0;
        while step <= chords {
            points.push(self.at(step / chords));
            step += 1.0;
        }
        points
    }

    /// The points strictly between the curve's ends, or around a circle.
    fn samples(&self) -> Vec<Point> {
        match self {
            Self::Line(_) => (1..LINE_PARTS)
                .map(|part| self.at(f64::from(part) / f64::from(LINE_PARTS)))
                .collect(),
            Self::Arc { .. } => {
                let points = self.trace();
                points[1..points.len() - 1].to_vec()
            }
            Self::Circle { .. } => self.trace(),
        }
    }

    /// The box around the curve, grown by the linear resolution.
    fn bounds(&self) -> [Point; 2] {
        let [low, high] = match *self {
            Self::Line([a, b]) => [
                [a[0].min(b[0]), a[1].min(b[1])],
                [a[0].max(b[0]), a[1].max(b[1])],
            ],
            Self::Arc { center, radius, .. } | Self::Circle { center, radius } => [
                [center[0] - radius, center[1] - radius],
                [center[0] + radius, center[1] + radius],
            ],
        };
        [
            [low[0] - LINEAR_RESOLUTION_MM, low[1] - LINEAR_RESOLUTION_MM],
            [
                high[0] + LINEAR_RESOLUTION_MM,
                high[1] + LINEAR_RESOLUTION_MM,
            ],
        ]
    }

    /// Whether `point` lies on the curve away from its ends.
    fn inside_span(&self, point: Point) -> bool {
        self.distance(point) <= LINEAR_RESOLUTION_MM
            && self
                .ends()
                .is_none_or(|ends| !ends.iter().any(|&end| meets(end, point)))
    }
}

/// The points where the lines and circles that carry `a` and `b` meet.
fn crossings(a: &Shape, b: &Shape) -> Vec<Point> {
    match (*a, *b) {
        (Shape::Line(first), Shape::Line(second)) => {
            let along = [first[1][0] - first[0][0], first[1][1] - first[0][1]];
            let length = along[0].portable_hypot(along[1]);
            if length == 0.0 {
                return Vec::new();
            }
            let side = |point: Point| {
                (along[0] * (point[1] - first[0][1]) - along[1] * (point[0] - first[0][0])) / length
            };
            let (from, to) = (side(second[0]), side(second[1]));
            if (from - to).abs() <= LINEAR_RESOLUTION_MM {
                return Vec::new();
            }
            vec![Shape::Line(second).at(from / (from - to))]
        }
        (Shape::Line(line), circle) | (circle, Shape::Line(line)) => {
            let (center, radius) = carrier(circle);
            let along = [line[1][0] - line[0][0], line[1][1] - line[0][1]];
            let length = along[0].portable_hypot(along[1]);
            if length == 0.0 {
                return Vec::new();
            }
            let unit = [along[0] / length, along[1] / length];
            let offset = [center[0] - line[0][0], center[1] - line[0][1]];
            let foot = offset[0] * unit[0] + offset[1] * unit[1];
            let apart = (offset[0] * unit[1] - offset[1] * unit[0]).abs();
            let point = |at: f64| [line[0][0] + at * unit[0], line[0][1] + at * unit[1]];
            if (apart - radius).abs() <= LINEAR_RESOLUTION_MM {
                vec![point(foot)]
            } else if apart > radius {
                Vec::new()
            } else {
                let half = (radius * radius - apart * apart).sqrt();
                vec![point(foot - half), point(foot + half)]
            }
        }
        (first, second) => {
            let ((c1, r1), (c2, r2)) = (carrier(first), carrier(second));
            let apart = (c2[0] - c1[0]).portable_hypot(c2[1] - c1[1]);
            if apart == 0.0
                || apart > r1 + r2 + LINEAR_RESOLUTION_MM
                || apart < (r1 - r2).abs() - LINEAR_RESOLUTION_MM
            {
                return Vec::new();
            }
            let unit = [(c2[0] - c1[0]) / apart, (c2[1] - c1[1]) / apart];
            let along = (apart * apart + r1 * r1 - r2 * r2) / (2.0 * apart);
            let base = [c1[0] + along * unit[0], c1[1] + along * unit[1]];
            if apart >= r1 + r2 - LINEAR_RESOLUTION_MM
                || apart <= (r1 - r2).abs() + LINEAR_RESOLUTION_MM
            {
                return vec![base];
            }
            let half = (r1 * r1 - along * along).max(0.0).sqrt();
            vec![
                [base[0] - half * unit[1], base[1] + half * unit[0]],
                [base[0] + half * unit[1], base[1] - half * unit[0]],
            ]
        }
    }
}

fn carrier(shape: Shape) -> (Point, f64) {
    match shape {
        Shape::Arc { center, radius, .. } | Shape::Circle { center, radius } => (center, radius),
        Shape::Line([a, _]) => (a, 0.0),
    }
}

/// A place where a curve is cut: how far along, and the contact point that
/// both curves' pieces end at.
#[derive(Debug, Clone, Copy)]
struct Cut {
    fraction: f64,
    point: Point,
}

/// A piece of a curve between two cuts or ends.
struct Piece {
    /// The index of its curve.
    entity: usize,
    shape: Shape,
    /// Its end joints; none for a circle no curve touches.
    joints: Option<[usize; 2]>,
    /// The earlier piece it lies along, between the same joints.
    repeats: Option<usize>,
}

/// The sketch's curves cut at their contacts.
struct Arrangement<'a> {
    /// The curves' entities, in id order.
    ids: Vec<&'a SketchEntityId>,
    index: HashMap<&'a SketchEntityId, usize>,
    /// Each curve whole.
    shapes: Vec<Shape>,
    pieces: Vec<Piece>,
    joints: Vec<Point>,
}

impl<'a> Arrangement<'a> {
    /// `first` names the curves a piece that lies along another keeps.
    fn new(
        curves: &HashMap<&'a SketchEntityId, &PlanarCurve>,
        first: impl Fn(&SketchEntityId) -> bool,
    ) -> Result<Self, Unchecked> {
        let mut ids = curves.keys().copied().collect::<Vec<_>>();
        ids.sort();
        let shapes = ids
            .iter()
            .map(|entity| Shape::new(curves[entity]))
            .collect::<Result<Vec<_>, _>>()?;
        let count = shapes.len();
        if count
            .checked_mul(count.max(1) - 1)
            .is_none_or(|twice| twice / 2 > PAIR_BUDGET)
        {
            return Err(Unchecked::Budget);
        }
        let mut joints = Joints::default();
        let mut pieces = Vec::new();
        for (entity, (shape, cuts)) in shapes.iter().zip(contacts(&shapes)).enumerate() {
            let mut bounds = Vec::with_capacity(cuts.len() + 2);
            if let Some([start, end]) = shape.ends() {
                bounds.push(Cut {
                    fraction: 0.0,
                    point: start,
                });
                bounds.extend(cuts);
                bounds.push(Cut {
                    fraction: 1.0,
                    point: end,
                });
            } else if let Some(&opening) = cuts.first() {
                bounds.extend(cuts);
                bounds.push(Cut {
                    fraction: opening.fraction + 1.0,
                    point: opening.point,
                });
            } else {
                pieces.push(Piece {
                    entity,
                    shape: *shape,
                    joints: None,
                    repeats: None,
                });
                continue;
            }
            for pair in bounds.windows(2) {
                pieces.push(Piece {
                    entity,
                    shape: shape.part(pair[0].fraction, pair[1].fraction),
                    joints: Some([
                        joints.at(pair[0].point).ok_or(Unchecked::Unplaced)?,
                        joints.at(pair[1].point).ok_or(Unchecked::Unplaced)?,
                    ]),
                    repeats: None,
                });
            }
        }
        let firsts = ids.iter().map(|&entity| first(entity)).collect::<Vec<_>>();
        mark_repeats(&mut pieces, &firsts);
        let index = ids
            .iter()
            .enumerate()
            .map(|(index, entity)| (*entity, index))
            .collect();
        Ok(Self {
            ids,
            index,
            shapes,
            pieces,
            joints: joints.points,
        })
    }

    /// Whether each piece closes in the sketch of the curves `include` names.
    fn closing(&self, include: impl Fn(usize) -> bool) -> Vec<bool> {
        let mut closing = vec![false; self.pieces.len()];
        let mut edges = Vec::new();
        for (index, piece) in self.pieces.iter().enumerate() {
            if piece.repeats.is_some() || !include(piece.entity) {
                continue;
            }
            match piece.joints {
                None => closing[index] = true,
                Some([a, b]) if a == b => {
                    closing[index] = matches!(piece.shape, Shape::Arc { sweep, .. } if sweep > PI);
                }
                Some(pair) => edges.push((index, pair)),
            }
        }
        for ((index, _), bridge) in edges.iter().zip(bridges(&self.joints, &edges)) {
            closing[*index] = !bridge;
        }
        closing
    }
}

/// The cuts of each shape where another touches or crosses it away from its
/// ends, in order along it.
fn contacts(shapes: &[Shape]) -> Vec<Vec<Cut>> {
    let bounds = shapes.iter().map(Shape::bounds).collect::<Vec<_>>();
    let mut cuts = vec![Vec::new(); shapes.len()];
    for first in 0..shapes.len() {
        for second in first + 1..shapes.len() {
            let ([low, high], [other_low, other_high]) = (bounds[first], bounds[second]);
            if low[0] > other_high[0]
                || other_low[0] > high[0]
                || low[1] > other_high[1]
                || other_low[1] > high[1]
            {
                continue;
            }
            for (on, from) in [(first, second), (second, first)] {
                for end in shapes[from].ends().into_iter().flatten() {
                    if shapes[on].inside_span(end) {
                        cuts[on].push(Cut {
                            fraction: shapes[on].fraction(end),
                            point: end,
                        });
                    }
                }
            }
            for point in crossings(&shapes[first], &shapes[second]) {
                if shapes[first].inside_span(point) && shapes[second].inside_span(point) {
                    for on in [first, second] {
                        cuts[on].push(Cut {
                            fraction: shapes[on].fraction(point),
                            point,
                        });
                    }
                }
            }
        }
    }
    for cuts in &mut cuts {
        cuts.sort_by(|a, b| a.fraction.total_cmp(&b.fraction));
        let mut kept: Vec<Cut> = Vec::with_capacity(cuts.len());
        for cut in cuts.drain(..) {
            if kept.last().is_none_or(|last| !meets(last.point, cut.point)) {
                kept.push(cut);
            }
        }
        if kept.len() > 1 && meets(kept[0].point, kept[kept.len() - 1].point) {
            kept.pop();
        }
        *cuts = kept;
    }
    cuts
}

/// Marks each piece that lies along an earlier piece between the same
/// joints, the pieces of the curves `first` marks before the others.
fn mark_repeats(pieces: &mut [Piece], first: &[bool]) {
    let mut groups = HashMap::<Option<[usize; 2]>, Vec<usize>>::new();
    for (index, piece) in pieces.iter().enumerate() {
        let key = piece.joints.map(|[a, b]| [a.min(b), a.max(b)]);
        groups.entry(key).or_default().push(index);
    }
    for mut group in groups.into_values() {
        if group.len() < 2 {
            continue;
        }
        group.sort_by_key(|&index| (!first[pieces[index].entity], index));
        let mut kept: Vec<usize> = Vec::new();
        for index in group {
            let twin = kept
                .iter()
                .copied()
                .find(|&earlier| along(&pieces[index].shape, &pieces[earlier].shape));
            match twin {
                Some(twin) => pieces[index].repeats = Some(twin),
                None => kept.push(index),
            }
        }
    }
}

/// Whether `shape` lies along `other`.
fn along(shape: &Shape, other: &Shape) -> bool {
    match (*shape, *other) {
        (
            Shape::Circle { center, radius },
            Shape::Circle {
                center: c,
                radius: r,
            },
        ) => meets(center, c) && (radius - r).abs() <= LINEAR_RESOLUTION_MM,
        (Shape::Circle { .. }, _) | (_, Shape::Circle { .. }) => false,
        _ => shape
            .samples()
            .into_iter()
            .all(|point| other.distance(point) <= LINEAR_RESOLUTION_MM),
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
fn bridges(joints: &[Point], edges: &[(usize, [usize; 2])]) -> Vec<bool> {
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

/// How the closing pieces `extra`, whose curves the stored profile leaves
/// out, change its regions.
fn classify(
    loops: &[Vec<SketchEntityUse>],
    arrangement: &Arrangement<'_>,
    is_stored: &impl Fn(usize) -> bool,
    extra: &[usize],
) -> Vec<Difference> {
    let Some(rings) = loops
        .iter()
        .map(|uses| ring(uses, arrangement))
        .collect::<Option<Vec<_>>>()
    else {
        return vec![Difference::Other];
    };
    let stored_shapes = arrangement
        .shapes
        .iter()
        .enumerate()
        .filter(|(entity, _)| is_stored(*entity))
        .map(|(_, shape)| *shape)
        .collect::<Vec<_>>();
    let sampled = extra
        .iter()
        .map(|&piece| (piece, arrangement.pieces[piece].shape.samples()))
        .collect::<Vec<_>>();
    let tests = sampled
        .iter()
        .map(|(_, points)| points.len())
        .sum::<usize>()
        .checked_mul(rings.iter().map(Vec::len).sum::<usize>() + stored_shapes.len());
    if tests.is_none_or(|tests| tests > CLASSIFY_BUDGET) {
        return vec![Difference::Other];
    }
    let mut parent = (0..arrangement.joints.len()).collect::<Vec<_>>();
    for &piece in extra {
        if let Some([a, b]) = arrangement.pieces[piece].joints {
            let (a, b) = (root(&mut parent, a), root(&mut parent, b));
            parent[a] = b;
        }
    }
    let stored_joints = arrangement
        .pieces
        .iter()
        .filter(|piece| piece.repeats.is_none() && is_stored(piece.entity))
        .filter_map(|piece| piece.joints)
        .flatten()
        .collect::<HashSet<_>>();
    // Each component, keyed by its root joint, or past the joints for a
    // piece without ends: whether it reaches outside the stored profile,
    // and its joints.
    let mut components = HashMap::<usize, (bool, HashSet<usize>)>::new();
    for (piece, points) in &sampled {
        let joints = arrangement.pieces[*piece].joints;
        let key = match joints {
            Some([a, _]) => root(&mut parent, a),
            None => arrangement.joints.len() + piece,
        };
        let component = components.entry(key).or_default();
        component.0 |= points.iter().any(|&point| {
            !stored_shapes
                .iter()
                .any(|shape| shape.distance(point) <= LINEAR_RESOLUTION_MM)
                && outside(&rings, point)
        });
        component.1.extend(joints.into_iter().flatten());
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
/// outside the stored profile, alternately.
fn outside(rings: &[Vec<Point>], point: Point) -> bool {
    !rings.iter().any(|ring| on_boundary(ring, point))
        && rings.iter().filter(|ring| contains(ring, point)).count() % 2 == 0
}

/// The closed polygon a stored loop traces, or `None` where a use names a
/// curve the sketch does not hold as profile geometry, a circle is joined
/// to other curves, or consecutive uses do not meet.
fn ring(uses: &[SketchEntityUse], arrangement: &Arrangement<'_>) -> Option<Vec<Point>> {
    let mut points: Vec<Point> = Vec::new();
    for usage in uses {
        let shape = arrangement.shapes[*arrangement.index.get(&usage.entity)?];
        if matches!(shape, Shape::Circle { .. }) && uses.len() != 1 {
            return None;
        }
        let mut traced = shape.trace();
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

/// The counterclockwise sweep from `start` to `end`, `None` where it is
/// zero.
fn sweep(start: f64, end: f64) -> Option<f64> {
    let sweep = (end - start).rem_euclid(TAU);
    (sweep != 0.0).then_some(sweep)
}

/// The counterclockwise angle from `start` to the direction of `point` from
/// `center`.
fn turn(center: Point, point: Point, start: f64) -> f64 {
    ((point[1] - center[1]).portable_atan2(point[0] - center[0]) - start).rem_euclid(TAU)
}

fn polar(center: Point, radius: f64, angle: f64) -> Point {
    let (sin, cos) = angle.portable_sin_cos();
    [center[0] + radius * cos, center[1] + radius * sin]
}

fn at(point: &cadmpeg_ir::units::FinitePoint2) -> Point {
    let point = point.get();
    [point.u, point.v]
}

fn edges(ring: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    (0..ring.len()).map(|at| (ring[at], ring[(at + 1) % ring.len()]))
}

fn on_boundary(ring: &[Point], point: Point) -> bool {
    edges(ring).any(|(a, b)| Shape::Line([a, b]).distance(point) <= LINEAR_RESOLUTION_MM)
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
