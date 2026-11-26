// SPDX-License-Identifier: Apache-2.0
//! Trimmed profile regions as v11 boundary uses.
//!
//! A trimmed region stores each boundary use as a parameter interval of one
//! sketch curve. The v11 region names a whole entity or the `index`-th of the
//! `count` pieces into which the other non-construction curves of the sketch
//! cut it. This module cuts each used curve, maps every interval onto whole
//! pieces, and orients the rings.

use std::cell::{OnceCell, RefCell};
use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::rc::Rc;

use cadmpeg_ir::design::selection::{BoundaryUse, Fragment, SketchRegion};
use cadmpeg_ir::features::profiles::SketchProfileBoundaryUse;
use cadmpeg_ir::features::selections::NonEmptyMembers;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometryDefinition, SketchId};

use super::Projected;
use crate::feature::profile::EPS_PROFILE_WIRE_POINT_MM;

/// Largest distance, in millimetres, between a point and a curve for the
/// point to lie on it: one kernel linear tolerance, as for profile wires.
const EPS_CUT_MM: f64 = EPS_PROFILE_WIRE_POINT_MM;
/// Largest distance, in millimetres, at which two points on one curve are the
/// same point: a cut point and an end of the curve, two cut points, and the
/// end of a stored interval and the cut point or curve end it is matched to.
/// A stored end is a wire vertex within one tolerance of the curve, and a cut
/// point is within one tolerance of the same crossing.
const EPS_USE_END_MM: f64 = 2.0 * EPS_CUT_MM;
/// Largest gap, in millimetres, between the end of one boundary use and the
/// start of the next in a ring: two matched ends and one cut point.
const EPS_RING_GAP_MM: f64 = 4.0 * EPS_CUT_MM;
/// Sine of the angle between two lines below which they have no single
/// crossing. Their overlap is found by the ends of the lines.
const EPS_PARALLEL_SINE: f64 = 1.0e-12;
/// Smallest area, in square millimetres, of a ring with an orientation.
const EPS_RING_AREA_MM2: f64 = 1.0e-10;
/// Most non-construction curves of one sketch that a trimmed region is cut
/// against. The cut work of one sketch, built once per decode, is at most the
/// square of this number.
const MAX_SKETCH_CURVES: usize = 2048;
const UNREAD_CURVE: &str = "the sketch holds a curve that is not a line, arc, or circle";

/// One non-construction curve in sketch millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Curve {
    Line {
        start: Point2,
        end: Point2,
    },
    /// Counterclockwise from `start` through `sweep`, in radians.
    Arc {
        center: Point2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
    Circle {
        center: Point2,
        radius: f64,
    },
}

/// An axis-aligned box in sketch millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Bounds {
    low: Point2,
    high: Point2,
}

impl Bounds {
    fn of_points(points: impl IntoIterator<Item = Point2>) -> Option<Self> {
        points.into_iter().fold(None, |found, point| {
            Some(match found {
                None => Self {
                    low: point,
                    high: point,
                },
                Some(Self { low, high }) => Self {
                    low: Point2::new(low.u.min(point.u), low.v.min(point.v)),
                    high: Point2::new(high.u.max(point.u), high.v.max(point.v)),
                },
            })
        })
    }

    fn about(center: Point2, along_u: f64, along_v: f64) -> Self {
        Self {
            low: Point2::new(center.u - along_u, center.v - along_v),
            high: Point2::new(center.u + along_u, center.v + along_v),
        }
    }

    /// Whether the boxes share a point once each is grown by `grow`.
    fn meets(self, other: Self, grow: f64) -> bool {
        self.low.u - grow <= other.high.u + grow
            && other.low.u - grow <= self.high.u + grow
            && self.low.v - grow <= other.high.v + grow
            && other.low.v - grow <= self.high.v + grow
    }
}

enum Kind {
    Curve(Curve),
    /// An isolated point bounds no region and cuts no curve.
    Point,
    /// A curve that is not read as a line, arc, or circle, with a box that
    /// holds it where one is known.
    Unread(Option<Bounds>),
}

impl Curve {
    fn of(entity: &SketchEntity) -> Kind {
        match entity.geometry.definition() {
            SketchGeometryDefinition::Point { .. } => Kind::Point,
            SketchGeometryDefinition::Line { start, end } => {
                let (start, end) = (start.get(), end.get());
                if distance(start, end) > EPS_CUT_MM {
                    Kind::Curve(Self::Line { start, end })
                } else {
                    Kind::Unread(Bounds::of_points([start, end]))
                }
            }
            SketchGeometryDefinition::Circle { center, radius } => Kind::Curve(Self::Circle {
                center: center.get(),
                radius: radius.get(),
            }),
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let sweep = end_angle.get() - start_angle.get();
                // An arc runs counterclockwise from its start angle to its end
                // angle, as the sweep `end - start` in `(0, TAU)`. An arc whose
                // end angle is below its start angle may be stored by a source
                // that wraps the end past one turn; this reader does not take
                // it for a curve, and `feature/profile.rs` `LoopCurve` makes
                // the same assumption.
                if sweep > 0.0 && sweep < TAU {
                    Kind::Curve(Self::Arc {
                        center: center.get(),
                        radius: radius.get(),
                        start: start_angle.get(),
                        sweep,
                    })
                } else {
                    Kind::Unread(None)
                }
            }
            SketchGeometryDefinition::Ellipse {
                center,
                major_angle,
                radii,
                ..
            } => {
                // The box of the whole ellipse holds every bounded part of it.
                let (major, minor) = (radii.major().get(), radii.minor().get());
                let (sin, cos) = major_angle.get().sin_cos();
                Kind::Unread(Some(Bounds::about(
                    center.get(),
                    (major * cos).hypot(minor * sin),
                    (major * sin).hypot(minor * cos),
                )))
            }
            SketchGeometryDefinition::Nurbs { curve } => Kind::Unread(Bounds::of_points(
                curve
                    .control_points()
                    .into_iter()
                    .map(cadmpeg_ir::units::FinitePoint2::get),
            )),
            _ => Kind::Unread(None),
        }
    }

    /// A box that holds the curve.
    fn bounds(self) -> Bounds {
        match self {
            Self::Line { start, end } => Bounds {
                low: Point2::new(start.u.min(end.u), start.v.min(end.v)),
                high: Point2::new(start.u.max(end.u), start.v.max(end.v)),
            },
            // The box of the whole circle holds every arc of it.
            Self::Arc { center, radius, .. } | Self::Circle { center, radius } => {
                Bounds::about(center, radius, radius)
            }
        }
    }

    fn closed(self) -> bool {
        matches!(self, Self::Circle { .. })
    }

    /// Millimetres along the curve for one unit of its parameter.
    fn scale(self) -> f64 {
        match self {
            Self::Line { start, end } => distance(start, end),
            Self::Arc { radius, .. } | Self::Circle { radius, .. } => radius,
        }
    }

    /// The parameter interval of the whole curve in its stored direction: a
    /// line from 0 to 1, an arc or circle by angle.
    fn domain(self) -> [f64; 2] {
        match self {
            Self::Line { .. } => [0.0, 1.0],
            Self::Arc { start, sweep, .. } => [start, start + sweep],
            Self::Circle { .. } => [0.0, TAU],
        }
    }

    fn point_at(self, parameter: f64) -> Point2 {
        match self {
            Self::Line { start, end } => Point2::new(
                parameter.mul_add(end.u - start.u, start.u),
                parameter.mul_add(end.v - start.v, start.v),
            ),
            Self::Arc { center, radius, .. } | Self::Circle { center, radius } => {
                polar(center, radius, parameter)
            }
        }
    }

    /// The ends of an open curve.
    fn ends(self) -> Option<[Point2; 2]> {
        let [low, high] = self.domain();
        (!self.closed()).then(|| [self.point_at(low), self.point_at(high)])
    }

    /// The parameter of a point on the curve, or none where the point is
    /// farther than the tolerance from it.
    fn parameter(self, point: Point2) -> Option<f64> {
        match self {
            Self::Line { start, end } => {
                let length = distance(start, end);
                let along = (point.u - start.u)
                    .mul_add(end.u - start.u, (point.v - start.v) * (end.v - start.v))
                    / (length * length);
                let off = ((point.u - start.u) * (end.v - start.v)
                    - (point.v - start.v) * (end.u - start.u))
                    .abs()
                    / length;
                let slack = EPS_CUT_MM / length;
                (off <= EPS_CUT_MM && (-slack..=1.0 + slack).contains(&along))
                    .then(|| along.clamp(0.0, 1.0))
            }
            Self::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                on_circle(point, center, radius)?;
                let slack = EPS_CUT_MM / radius;
                let offset = angle_about(point, center, start);
                // A point a hair below the start angle wraps to just under a
                // full turn.
                let offset = if offset > TAU - slack { 0.0 } else { offset };
                (offset <= sweep + slack).then(|| start + offset.min(sweep))
            }
            Self::Circle { center, radius } => {
                on_circle(point, center, radius)?;
                let angle = angle_about(point, center, 0.0);
                Some(if angle > TAU - EPS_USE_END_MM / radius {
                    0.0
                } else {
                    angle
                })
            }
        }
    }

    /// The points where the carriers of two curves meet. A point may lie off
    /// either curve.
    fn carrier_points(self, other: Self) -> Vec<Point2> {
        match (self, other) {
            (Self::Line { .. }, Self::Line { .. }) => line_line(self, other),
            (
                Self::Line { start, end },
                Self::Arc { center, radius, .. } | Self::Circle { center, radius },
            )
            | (
                Self::Arc { center, radius, .. } | Self::Circle { center, radius },
                Self::Line { start, end },
            ) => line_circle(start, end, center, radius),
            (
                Self::Arc { center, radius, .. } | Self::Circle { center, radius },
                Self::Arc {
                    center: other_center,
                    radius: other_radius,
                    ..
                }
                | Self::Circle {
                    center: other_center,
                    radius: other_radius,
                },
            ) => circle_circle(center, radius, other_center, other_radius),
        }
    }

    /// Twice the signed area under the traversal from `from` to `to`, as the
    /// integral of `u dv - v du`.
    fn doubled_area(self, from: f64, to: f64) -> f64 {
        match self {
            Self::Line { .. } => {
                let (a, b) = (self.point_at(from), self.point_at(to));
                a.u * b.v - b.u * a.v
            }
            Self::Arc { center, radius, .. } | Self::Circle { center, radius } => {
                radius * radius * (to - from)
                    + radius
                        * (center.u * (to.sin() - from.sin()) - center.v * (to.cos() - from.cos()))
            }
        }
    }
}

fn polar(center: Point2, radius: f64, angle: f64) -> Point2 {
    Point2::new(
        radius.mul_add(angle.cos(), center.u),
        radius.mul_add(angle.sin(), center.v),
    )
}

fn distance(left: Point2, right: Point2) -> f64 {
    (left.u - right.u).hypot(left.v - right.v)
}

fn on_circle(point: Point2, center: Point2, radius: f64) -> Option<()> {
    ((distance(point, center) - radius).abs() <= EPS_CUT_MM).then_some(())
}

/// The counterclockwise angle in `[0, TAU)` from `base` to the direction of
/// `point` about `center`.
fn angle_about(point: Point2, center: Point2, base: f64) -> f64 {
    ((point.v - center.v).atan2(point.u - center.u) - base).rem_euclid(TAU)
}

fn line_line(first: Curve, second: Curve) -> Vec<Point2> {
    let (
        Curve::Line {
            start: a,
            end: a_end,
        },
        Curve::Line {
            start: b,
            end: b_end,
        },
    ) = (first, second)
    else {
        return Vec::new();
    };
    let (d1, d2) = (
        (a_end.u - a.u, a_end.v - a.v),
        (b_end.u - b.u, b_end.v - b.v),
    );
    let cross = d1.0 * d2.1 - d1.1 * d2.0;
    if cross.abs() <= EPS_PARALLEL_SINE * d1.0.hypot(d1.1) * d2.0.hypot(d2.1) {
        return Vec::new();
    }
    let along = ((b.u - a.u) * d2.1 - (b.v - a.v) * d2.0) / cross;
    vec![Point2::new(
        along.mul_add(d1.0, a.u),
        along.mul_add(d1.1, a.v),
    )]
}

fn line_circle(start: Point2, end: Point2, center: Point2, radius: f64) -> Vec<Point2> {
    let (du, dv) = (end.u - start.u, end.v - start.v);
    let length_squared = du * du + dv * dv;
    let foot = ((center.u - start.u) * du + (center.v - start.v) * dv) / length_squared;
    let nearest = Point2::new(foot.mul_add(du, start.u), foot.mul_add(dv, start.v));
    let gap = distance(nearest, center);
    if gap > radius + EPS_CUT_MM {
        return Vec::new();
    }
    if (gap - radius).abs() <= EPS_CUT_MM {
        return vec![nearest];
    }
    let half = (radius * radius - gap * gap).sqrt() / length_squared.sqrt();
    [foot - half, foot + half]
        .map(|at| Point2::new(at.mul_add(du, start.u), at.mul_add(dv, start.v)))
        .to_vec()
}

fn circle_circle(first: Point2, radius: f64, second: Point2, other: f64) -> Vec<Point2> {
    let between = distance(first, second);
    if between <= EPS_CUT_MM || between > radius + other + EPS_CUT_MM {
        return Vec::new();
    }
    let inside = (radius - other).abs();
    if between < inside - EPS_CUT_MM {
        return Vec::new();
    }
    let (du, dv) = (
        (second.u - first.u) / between,
        (second.v - first.v) / between,
    );
    if (between - (radius + other)).abs() <= EPS_CUT_MM {
        return vec![Point2::new(
            radius.mul_add(du, first.u),
            radius.mul_add(dv, first.v),
        )];
    }
    if (between - inside).abs() <= EPS_CUT_MM {
        let reach = if radius > other { radius } else { -radius };
        return vec![Point2::new(
            reach.mul_add(du, first.u),
            reach.mul_add(dv, first.v),
        )];
    }
    let along = (radius * radius - other * other + between * between) / (2.0 * between);
    let half = (radius * radius - along * along).max(0.0).sqrt();
    let base = Point2::new(along.mul_add(du, first.u), along.mul_add(dv, first.v));
    vec![
        Point2::new(base.u - half * dv, base.v + half * du),
        Point2::new(base.u + half * dv, base.v - half * du),
    ]
}

/// What the entities of a sketch are for cutting.
enum Slot {
    /// A non-construction curve, by position in the curve list.
    Curve(usize),
    Construction,
    Point,
    /// A non-construction curve that is not read.
    Unread,
}

/// The non-construction curves of one sketch and the cut points found on
/// them so far.
pub(super) struct Arrangement {
    curves: Vec<Curve>,
    /// The boxes of the non-construction curves that are not read, `None`
    /// where no box is known.
    unread: Vec<Option<Bounds>>,
    slots: BTreeMap<SketchEntityId, Slot>,
    /// The cut parameters of each curve, found when the curve is first used.
    cuts: Vec<OnceCell<Projected<Vec<f64>>>>,
}

/// The arrangements of the sketches of one decode, each built when a trimmed
/// region of its sketch first needs it.
#[derive(Default)]
pub(in crate::design_v11::operations) struct Arrangements {
    by_sketch: RefCell<BTreeMap<SketchId, Projected<Rc<Arrangement>>>>,
}

impl Arrangements {
    pub(super) fn get(
        &self,
        entities: &[SketchEntity],
        sketch: &SketchId,
    ) -> Projected<Rc<Arrangement>> {
        if let Some(found) = self.by_sketch.borrow().get(sketch) {
            return found.clone();
        }
        let built = Arrangement::new(entities, sketch).map(Rc::new);
        self.by_sketch
            .borrow_mut()
            .insert(sketch.clone(), built.clone());
        built
    }
}

/// One piece of a curve, or the whole curve, covered by a stored interval.
#[derive(Debug, Clone, PartialEq)]
struct Covered {
    fragment: Option<Fragment>,
    /// Parameters in ascending order; the end of a circle piece may exceed
    /// one turn.
    from: f64,
    to: f64,
}

/// One boundary use of a ring in traversal order.
struct Step {
    entity: SketchEntityId,
    curve: usize,
    fragment: Option<Fragment>,
    /// Parameters of the traversal start and end.
    from: f64,
    to: f64,
}

impl Arrangement {
    pub(super) fn new(entities: &[SketchEntity], sketch: &SketchId) -> Projected<Self> {
        let mut curves = Vec::new();
        let mut unread = Vec::new();
        let mut slots = BTreeMap::new();
        for entity in entities.iter().filter(|entity| &entity.sketch == sketch) {
            let slot = match Curve::of(entity) {
                _ if entity.construction => Slot::Construction,
                Kind::Point => Slot::Point,
                Kind::Curve(curve) => {
                    admit(curves.len() + unread.len())?;
                    curves.push(curve);
                    Slot::Curve(curves.len() - 1)
                }
                Kind::Unread(bounds) => {
                    admit(curves.len() + unread.len())?;
                    unread.push(bounds);
                    Slot::Unread
                }
            };
            slots.insert(entity.id().clone(), slot);
        }
        let cuts = curves.iter().map(|_| OnceCell::new()).collect();
        Ok(Self {
            curves,
            unread,
            slots,
            cuts,
        })
    }

    /// The cut parameters of one curve in ascending order, without
    /// duplicates and without its own ends. A curve that is not read cuts the
    /// curve unless its box is clear of the curve's box.
    fn cuts(&self, index: usize) -> Projected<&[f64]> {
        self.cuts[index]
            .get_or_init(|| self.find_cuts(index))
            .as_ref()
            .map(Vec::as_slice)
            .map_err(|reason| *reason)
    }

    fn find_cuts(&self, index: usize) -> Projected<Vec<f64>> {
        let curve = self.curves[index];
        let around = curve.bounds();
        if self
            .unread
            .iter()
            .any(|bounds| bounds.is_none_or(|bounds| bounds.meets(around, EPS_USE_END_MM)))
        {
            return Err(UNREAD_CURVE);
        }
        let own_ends = curve.ends();
        let mut found = Vec::new();
        for (at, other) in self.curves.iter().enumerate() {
            if at == index {
                continue;
            }
            let crossings = curve
                .carrier_points(*other)
                .into_iter()
                .filter(|point| other.parameter(*point).is_some());
            let touches = other.ends().into_iter().flatten();
            for point in crossings.chain(touches) {
                let Some(parameter) = curve.parameter(point) else {
                    continue;
                };
                if own_ends.is_some_and(|ends| {
                    ends.iter()
                        .any(|end| distance(*end, point) <= EPS_USE_END_MM)
                }) {
                    continue;
                }
                found.push(parameter);
            }
        }
        found.sort_by(f64::total_cmp);
        let scale = curve.scale();
        let mut cuts: Vec<f64> = Vec::new();
        for parameter in found {
            if cuts
                .last()
                .is_none_or(|last| (parameter - last) * scale > EPS_USE_END_MM)
            {
                cuts.push(parameter);
            }
        }
        // A circle cut at its parameter 0 may also hold a parameter just
        // below one turn.
        if curve.closed()
            && cuts.len() > 1
            && (cuts[0] + TAU - cuts[cuts.len() - 1]) * scale <= EPS_USE_END_MM
        {
            cuts.pop();
        }
        Ok(cuts)
    }

    /// The pieces of one curve that a stored interval covers, in ascending
    /// parameter order.
    fn cover(&self, index: usize, interval: [f64; 2]) -> Projected<Vec<Covered>> {
        let curve = self.curves[index];
        let scale = curve.scale();
        let [low, high] = [interval[0].min(interval[1]), interval[0].max(interval[1])];
        let cuts = self.cuts(index)?;
        let unmatched = "a trimmed boundary use does not start and end at cut points";
        if curve.closed() {
            return cover_circle(cuts, low, high, scale);
        }
        let [first, last] = curve.domain();
        // The bounds are the start, the cuts, and the end.
        let count = cuts.len() + 2;
        let bound = |at: usize| match at {
            0 => first,
            _ if at == count - 1 => last,
            _ => cuts[at - 1],
        };
        let nearest = |parameter: f64| {
            (0..count)
                .map(|at| (at, (bound(at) - parameter).abs() * scale))
                .filter(|(_, gap)| *gap <= EPS_USE_END_MM)
                .min_by(|left, right| left.1.total_cmp(&right.1))
                .map(|(at, _)| at)
        };
        let start = nearest(low).ok_or(unmatched)?;
        let end = nearest(high).ok_or(unmatched)?;
        if end <= start {
            return Err(unmatched);
        }
        if start == 0 && end == count - 1 {
            return Ok(vec![Covered {
                fragment: None,
                from: first,
                to: last,
            }]);
        }
        let pieces = u32::try_from(count - 1).map_err(|_| "the cut count overflows")?;
        (start..end)
            .map(|piece| {
                Ok(Covered {
                    fragment: Some(Fragment {
                        index: u32::try_from(piece).map_err(|_| "the piece index overflows")?,
                        count: pieces,
                    }),
                    from: bound(piece),
                    to: bound(piece + 1),
                })
            })
            .collect()
    }

    fn ring(&self, ring: &[SketchProfileBoundaryUse], hole: bool) -> Projected<Vec<BoundaryUse>> {
        let mut steps = Vec::new();
        for usage in ring {
            let index = match self.slots.get(&usage.entity) {
                Some(Slot::Curve(index)) => *index,
                Some(Slot::Construction) => {
                    return Err("a trimmed boundary use names a construction entity")
                }
                Some(Slot::Point) => return Err("a trimmed boundary use names a point"),
                Some(Slot::Unread) => return Err(UNREAD_CURVE),
                None => return Err("a trimmed boundary use names an entity outside its sketch"),
            };
            let mut covered = self.cover(index, interval(usage))?;
            if usage.reversed {
                covered.reverse();
            }
            for piece in covered {
                let (from, to) = if usage.reversed {
                    (piece.to, piece.from)
                } else {
                    (piece.from, piece.to)
                };
                steps.push(Step {
                    entity: usage.entity.clone(),
                    curve: index,
                    fragment: piece.fragment,
                    from,
                    to,
                });
            }
        }
        for (step, next) in steps.iter().zip(steps.iter().cycle().skip(1)) {
            let end = self.curves[step.curve].point_at(step.to);
            let start = self.curves[next.curve].point_at(next.from);
            if distance(end, start) > EPS_RING_GAP_MM {
                return Err("a trimmed boundary ring does not close");
            }
        }
        let doubled = steps
            .iter()
            .map(|step| self.curves[step.curve].doubled_area(step.from, step.to))
            .sum::<f64>();
        if doubled.abs() <= 2.0 * EPS_RING_AREA_MM2 {
            return Err("a trimmed boundary ring encloses no area");
        }
        // The topology convention: an outer ring runs counterclockwise about
        // the sketch normal, a hole ring clockwise.
        if (doubled < 0.0) != hole {
            steps.reverse();
            for step in &mut steps {
                std::mem::swap(&mut step.from, &mut step.to);
            }
        }
        Ok(steps
            .into_iter()
            .map(|step| BoundaryUse {
                entity: step.entity,
                fragment: step.fragment,
            })
            .collect())
    }

    /// The region of a trimmed outer ring and its hole rings. A region that
    /// any of its uses cannot be mapped for is refused whole.
    pub(super) fn region(
        &self,
        outer: &[SketchProfileBoundaryUse],
        holes: &[NonEmptyMembers<SketchProfileBoundaryUse>],
    ) -> Projected<SketchRegion> {
        Ok(SketchRegion {
            outer: self.ring(outer, false)?,
            holes: holes
                .iter()
                .map(|hole| self.ring(hole, true))
                .collect::<Projected<_>>()?,
        })
    }
}

/// Whether one more curve may join a sketch that holds `held`.
fn admit(held: usize) -> Projected<()> {
    if held >= MAX_SKETCH_CURVES {
        return Err("the sketch holds more curves than a trimmed profile is cut against");
    }
    Ok(())
}

fn interval(usage: &SketchProfileBoundaryUse) -> [f64; 2] {
    usage.parameter_range.endpoints()
}

/// The pieces of a circle that a stored interval covers. `cuts` holds the
/// cut angles in `[0, TAU)` in ascending order.
fn cover_circle(cuts: &[f64], low: f64, high: f64, radius: f64) -> Projected<Vec<Covered>> {
    let span = high - low;
    if span * radius >= TAU * radius - EPS_USE_END_MM {
        return Ok(vec![Covered {
            fragment: None,
            from: 0.0,
            to: TAU,
        }]);
    }
    let unmatched = "a trimmed boundary use does not start and end at cut points";
    let count = cuts.len();
    if count < 2 {
        return Err(unmatched);
    }
    // Millimetres along the circle between two angles, the short way round.
    let around = |left: f64, right: f64| {
        let gap = (left - right).rem_euclid(TAU);
        gap.min(TAU - gap) * radius
    };
    let nearest = |parameter: f64| {
        cuts.iter()
            .enumerate()
            .map(|(at, cut)| (at, around(*cut, parameter)))
            .filter(|(_, gap)| *gap <= EPS_USE_END_MM)
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(at, _)| at)
    };
    let first = nearest(low).ok_or(unmatched)?;
    let last = nearest(high).ok_or(unmatched)?;
    let pieces = (last + count - first) % count;
    if pieces == 0 {
        return Err(unmatched);
    }
    // The pieces run at most once past the last cut, so `at` is below twice
    // the cut count.
    let unrolled = |at: usize| cuts[at % count] + if at >= count { TAU } else { 0.0 };
    if (unrolled(first + pieces) - unrolled(first) - span).abs() * radius > EPS_USE_END_MM {
        return Err(unmatched);
    }
    let count = u32::try_from(count).map_err(|_| "the cut count overflows")?;
    (first..first + pieces)
        .map(|piece| {
            Ok(Covered {
                fragment: Some(Fragment {
                    index: u32::try_from(piece % cuts.len())
                        .map_err(|_| "the piece index overflows")?,
                    count,
                }),
                from: unrolled(piece),
                to: unrolled(piece + 1),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
