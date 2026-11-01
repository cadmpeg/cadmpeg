// SPDX-License-Identifier: Apache-2.0
//! A B-spline cache for a constant-radius rolling-ball blend that
//! `blend_surface::solve` does not solve: a support pair, a nested blend, or a
//! polyline spine chart (`sldprt.md` §7.4). Sections are exact rational arcs
//! about the solved ball centre, joined along the spine by cubic Hermite
//! pieces within [`BLEND_FIT_TOLERANCE_MM`].

use std::cell::{Cell, RefCell};

use cadmpeg_core::convert::f64_from_index;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis, WeightedPole3};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::NonZeroReal;

/// Departure of a fitted section from the exact section: the linear
/// resolution, 1e-8 m.
const BLEND_FIT_TOLERANCE_MM: f64 = 1.0e-5;
/// Departure a span at [`MAX_SPLIT_DEPTH`] may keep.
const BLEND_FIT_LIMIT_MM: f64 = 1.0e-3;
/// Share of the face's spine interval added beyond each end.
const DOMAIN_MARGIN_FRACTION: f64 = 0.05;
/// Spans the fit starts from, shared among the pieces of a polyline chart.
const SEED_SPANS: usize = 4;
/// Midpoint splits allowed below one seed span.
const MAX_SPLIT_DEPTH: usize = 12;
/// Spans a fitted cache may hold.
const MAX_SPANS: usize = 1 << 10;
/// Relative step of the central difference along the spine.
const DIFFERENCE_STEP_FRACTION: f64 = 1.0e-4;
/// Section parameters at which a span's departure is checked.
const CHECK_FRACTIONS: [f64; 3] = [0.25, 0.5, 0.75];
/// Allowed departure of a contact from the ball's radius, relative to it.
const CONTACT_RADIUS_RELATIVE: f64 = 1.0e-6;
/// Smallest `π − θ` of a section arc; a half-turn section has no minor arc.
const MIN_HALF_TURN_GAP: f64 = 1.0e-6;
/// Samples per span when a vertex is inverted onto a NURBS spine.
const SPINE_SAMPLES: usize = 64;
/// Golden-section steps when a vertex is inverted onto a NURBS spine.
const SPINE_SECTION_STEPS: usize = 40;
/// Samples of the local window around the last inverted spine parameter.
const LOCAL_SPINE_SAMPLES: usize = 16;

/// The fitted cache of one blend.
pub(super) struct BlendFit {
    pub(super) surface: NurbsSurface,
    /// Largest departure from the exact sections the fit accepted, and no
    /// less than [`BLEND_FIT_TOLERANCE_MM`].
    pub(super) fit_tolerance: f64,
}

/// One homogeneous section: the three weighted controls of its arc as
/// `[x·w, y·w, z·w, w]`.
type Section = [[f64; 4]; 3];

/// A solved exact spine, keyed by the indices of its side candidates.
type ExactSpineEntry = ((usize, usize), Option<std::rc::Rc<ExactSpine>>);

/// A constant-radius rolling ball: its spine, the candidate support
/// surfaces of each side (one surface, or the two members of a support
/// pair), and its radius.
pub(super) struct BlendGeometry<'a, 'ctx> {
    ctx: &'a DecodeContext<'ctx>,
    spine: &'a CurveGeometry,
    sides: [Vec<Side<'a, 'ctx>>; 2],
    radius: f64,
    refusal: RefCell<Option<CodecError>>,
    /// Centre solves spent, against [`MAX_CENTER_SOLVES`].
    center_solves: Cell<usize>,
    /// The spine parameter of the last point inverted, which seeds the next.
    last_parameter: RefCell<Option<f64>>,
    /// The exact spine of each choice of side candidates, once solved over a
    /// stored spine chart.
    exact_spines: RefCell<Vec<ExactSpineEntry>>,
    /// The stored spine's points and parameters when it is a polyline.
    polyline: Option<SpinePolyline>,
}

/// A stored spine chart that is a polyline: one polynomial degree-one piece
/// between each two consecutive points, at increasing parameters.
struct SpinePolyline {
    parameters: Vec<f64>,
    points: Vec<Point3>,
}

impl SpinePolyline {
    /// The polyline of a clamped, polynomial, degree-one spine chart.
    fn of(spine: &CurveGeometry) -> Option<Self> {
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = spine else {
            return None;
        };
        let cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } = nurbs.pole_rows()
        else {
            return None;
        };
        let knots = nurbs.knots().as_slice();
        if nurbs.degree() != 1 || nurbs.periodic() || knots.len() != points.len() + 2 {
            return None;
        }
        let parameters = knots.get(1..=points.len())?.to_vec();
        if parameters.windows(2).any(|pair| pair[0] > pair[1]) {
            return None;
        }
        Some(Self {
            parameters,
            points: points.iter().map(|point| point.get()).collect(),
        })
    }

    /// The point at `s`, or `None` outside the parameters.
    fn point(&self, s: f64) -> Option<Point3> {
        let (&lower, &upper) = (self.parameters.first()?, self.parameters.last()?);
        if !(lower..=upper).contains(&s) {
            return None;
        }
        let index = self
            .parameters
            .partition_point(|parameter| *parameter <= s)
            .clamp(1, self.parameters.len().max(2) - 1)
            - 1;
        let (from, to) = (
            *self.parameters.get(index)?,
            *self.parameters.get(index + 1)?,
        );
        let (start, end) = (*self.points.get(index)?, *self.points.get(index + 1)?);
        let span = to - from;
        if !(span > 0.0 && span.is_finite()) {
            return Some(end);
        }
        let fraction = (s - from) / span;
        Some(Point3::new(
            start.x + (end.x - start.x) * fraction,
            start.y + (end.y - start.y) * fraction,
            start.z + (end.z - start.z) * fraction,
        ))
    }
}

/// One exact ball center at a spine parameter, with its derivative.
type SpineNode = (f64, Point3, cadmpeg_ir::math::Vector3);

/// An exact spine: runs of cubic Hermite pieces through exact ball centers
/// at increasing spine parameters, with the centers' derivatives. A run ends
/// where the centers stop solving, and the next starts where they solve
/// again.
struct ExactSpine {
    runs: Vec<Vec<SpineNode>>,
}

impl ExactSpine {
    fn point(&self, s: f64) -> Option<Point3> {
        self.runs.iter().find_map(|run| hermite_spine_point(run, s))
    }
}

/// The point at `s` of the Hermite pieces through `nodes`, or `None`
/// outside them.
fn hermite_spine_point(nodes: &[SpineNode], s: f64) -> Option<Point3> {
    let first = nodes.first()?;
    let last = nodes.last()?;
    if !(first.0..=last.0).contains(&s) {
        return None;
    }
    let index = nodes.partition_point(|node| node.0 <= s).checked_sub(1)?;
    let start = nodes.get(index)?;
    let end = nodes.get(index + 1).unwrap_or(start);
    let span = end.0 - start.0;
    if span <= 0.0 {
        return Some(start.1);
    }
    let f = (s - start.0) / span;
    let (h00, h10, h01, h11) = (
        2.0 * f * f * f - 3.0 * f * f + 1.0,
        f * f * f - 2.0 * f * f + f,
        -2.0 * f * f * f + 3.0 * f * f,
        f * f * f - f * f,
    );
    let lane =
        |a: f64, da: f64, b: f64, db: f64| h00 * a + h10 * span * da + h01 * b + h11 * span * db;
    Some(Point3::new(
        lane(start.1.x, start.2.x, end.1.x, end.2.x),
        lane(start.1.y, start.2.y, end.1.y, end.2.y),
        lane(start.1.z, start.2.z, end.1.z, end.2.z),
    ))
}

/// One support candidate of a blend side: an analytic surface, another
/// rolling-ball blend the ball rolls against, or a support pair's convex
/// edge.
pub(super) enum Side<'a, 'ctx> {
    Surface(&'a SurfaceGeometry),
    Blend(std::rc::Rc<BlendGeometry<'a, 'ctx>>),
    Wedge(Wedge<'a>),
}

/// The material bounded by two analytic surfaces that meet at a convex edge
/// (`sldprt.md` §7.4). The ball touches one surface where its foot there
/// lies on the material side of the other, and their common edge elsewhere.
pub(super) struct Wedge<'a> {
    members: [&'a SurfaceGeometry; 2],
    /// Sign of the side of each member's natural normal the ball is on.
    outside: [f64; 2],
}

impl<'a> Wedge<'a> {
    /// The wedge of `members`, its outside per member taken from the side of
    /// the spine farthest from it over the face's spine interval. `None` when
    /// a member is not analytic or the spine touches it.
    ///
    /// # Errors
    ///
    /// Propagates an evaluator's resource refusal.
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        spine: &CurveGeometry,
        members: [&'a SurfaceGeometry; 2],
        vertices: &[Point3],
    ) -> Result<Option<Self>, CodecError> {
        if !members.iter().all(|member| analytic(member)) {
            return Ok(None);
        }
        let samples = spine_samples(ctx, spine, face_interval(ctx, spine, vertices)?)?;
        let mut outside = [0.0; 2];
        for (slot, member) in members.iter().enumerate() {
            let mut farthest: f64 = 0.0;
            for sample in &samples {
                let Some((foot, normal)) =
                    super::evaluation::analytic_surface_foot(member, *sample)
                else {
                    continue;
                };
                let distance = sample.vector_from(foot).dot(normal);
                if distance.abs() > farthest.abs() {
                    farthest = distance;
                }
            }
            if !(farthest.is_finite() && farthest != 0.0) {
                return Ok(None);
            }
            outside[slot] = farthest.signum();
        }
        Ok(Some(Self { members, outside }))
    }
}

/// Whether a NURBS curve ends where it starts, so that its seam has two
/// parameters.
fn closed_nurbs(nurbs: &cadmpeg_ir::geometry::nurbs::NurbsCurve) -> bool {
    nurbs.periodic()
        || matches!(
            nurbs.pole_rows(),
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points }
                if points.first() == points.last()
        )
}

/// The spine interval spanned by a blend face's `vertices`, or `None` when
/// there are none or they do not invert onto the spine.
fn face_interval(
    ctx: &DecodeContext<'_>,
    spine: &CurveGeometry,
    vertices: &[Point3],
) -> Result<Option<[f64; 2]>, CodecError> {
    if vertices.is_empty() {
        return Ok(None);
    }
    let fitter = BlendGeometry::with_sides(ctx, spine, [Vec::new(), Vec::new()], 0.0);
    let range = fitter.range(vertices);
    fitter.finish(range)
}

/// Stored-spine points: over `interval`, its ends, midpoint and inner knots;
/// else every knot and knot midpoint; else the point at zero.
fn spine_samples(
    ctx: &DecodeContext<'_>,
    spine: &CurveGeometry,
    interval: Option<[f64; 2]>,
) -> Result<Vec<Point3>, CodecError> {
    let knots = match spine {
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            let Some(domain) = cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs) else {
                return Ok(Vec::new());
            };
            let [lower, upper] = domain.endpoints();
            let mut knots: Vec<f64> = nurbs.knots().as_slice().to_vec();
            knots.dedup();
            knots.retain(|knot| (lower..=upper).contains(knot));
            Some(knots)
        }
        _ => None,
    };
    let parameters = match (interval, knots) {
        (Some([low, high]), knots) => {
            let mut parameters = vec![low, low.midpoint(high), high];
            parameters.extend(
                knots
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|knot| (low..=high).contains(knot)),
            );
            parameters
        }
        (None, Some(knots)) => {
            let mut parameters = knots.clone();
            parameters.extend(knots.windows(2).map(|pair| pair[0].midpoint(pair[1])));
            parameters
        }
        (None, None) => vec![0.0],
    };
    let mut points = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        if let Some(point) = super::evaluation::curve_point(ctx, spine, parameter)? {
            points.push(point.get());
        }
    }
    Ok(points)
}

/// Fit the cache of a blend whose spine and supports are given, over the
/// spine interval of `vertices`, or `None` when a support is not analytic,
/// the spine has no evaluable interval there, or a section has no minor arc.
///
/// # Errors
///
/// Propagates an evaluator's resource refusal.
pub(super) fn fit(
    ctx: &DecodeContext<'_>,
    spine: &CurveGeometry,
    supports: [Side<'_, '_>; 2],
    signed_radius: f64,
    vertices: &[Point3],
) -> Result<Option<BlendFit>, CodecError> {
    let [first, second] = supports;
    let fitter =
        BlendGeometry::with_sides(ctx, spine, [vec![first], vec![second]], signed_radius.abs());
    let fitted = fitter.fit(vertices);
    fitter.finish(fitted)
}

/// Distance between successive center iterates at which a center has
/// converged.
const CENTER_STEP_TOLERANCE_MM: f64 = 1.0e-11;
/// Newton iterations allowed for one center.
const MAX_CENTER_STEPS: usize = 32;
/// Centre solves one use of a blend may spend.
const MAX_CENTER_SOLVES: usize = 1 << 20;
/// Smallest `|n1 × n2|²` of two transversal support offsets.
const MIN_CENTER_TRANSVERSALITY: f64 = 1.0e-8;
/// Separation of two parallel support offsets at which they coincide, so a
/// center on one lies on both.
const COINCIDENT_OFFSETS_MM: f64 = 1.0e-7;
/// Distance between successive iterates at which a point of a wedge's edge
/// has converged.
const EDGE_STEP_TOLERANCE_MM: f64 = 1.0e-11;
/// Iterations allowed for one point of a wedge's edge.
const MAX_EDGE_STEPS: usize = 32;
/// Smallest `|n1 × n2|²` of a wedge's two members at its edge.
const MIN_EDGE_TRANSVERSALITY: f64 = 1.0e-8;

/// Share of a NURBS spine's domain searched on each side of the last
/// inverted parameter.
const LOCAL_SEARCH_FRACTION: f64 = 0.1;
/// Departure of the cached exact spine from solved centers.
const SPINE_CACHE_TOLERANCE_MM: f64 = 1.0e-7;
/// Midpoint splits allowed below one stored spine segment.
const MAX_SPINE_CACHE_DEPTH: usize = 10;
/// Spine parameter step of the central difference along a contact track.
const TRACK_DIFFERENCE_STEP: f64 = 1.0e-7;
/// Newton steps allowed when a point is inverted onto a track or the spine.
const MAX_NEAREST_STEPS: usize = 16;
/// Model-space length of a final Newton step at which an inversion has
/// converged.
const NEAREST_STEP_TOLERANCE_MM: f64 = 1.0e-10;

/// Spine stations at which a support pair's members are compared.
const PAIR_STATIONS: usize = 5;

/// The member of a support pair that a blend payload names as its one
/// support: the one whose distance from the spine, at every station of the
/// face's spine interval, is within a quarter of the radius of it while the
/// other member's is beyond half of it. `None` when no member qualifies.
///
/// # Errors
///
/// Propagates an evaluator's resource refusal.
pub(super) fn pair_member(
    ctx: &DecodeContext<'_>,
    spine: &CurveGeometry,
    members: [&SurfaceGeometry; 2],
    signed_radius: f64,
    vertices: &[Point3],
) -> Result<Option<usize>, CodecError> {
    let fitter = BlendGeometry::with_sides(
        ctx,
        spine,
        [
            vec![Side::Surface(members[0])],
            vec![Side::Surface(members[1])],
        ],
        signed_radius.abs(),
    );
    let member = fitter.pair_member(vertices);
    fitter.finish(member)
}

impl<'a, 'ctx> BlendGeometry<'a, 'ctx> {
    /// The rolling ball of `spine` between the candidate surfaces of each
    /// side, or `None` when a candidate is not analytic.
    pub(super) fn new(
        ctx: &'a DecodeContext<'ctx>,
        spine: &'a CurveGeometry,
        sides: [Vec<Side<'a, 'ctx>>; 2],
        signed_radius: f64,
    ) -> Option<Self> {
        let geometry = Self::with_sides(ctx, spine, sides, signed_radius.abs());
        (geometry.analytic() && geometry.radius > 0.0 && geometry.radius.is_finite())
            .then_some(geometry)
    }

    fn with_sides(
        ctx: &'a DecodeContext<'ctx>,
        spine: &'a CurveGeometry,
        sides: [Vec<Side<'a, 'ctx>>; 2],
        radius: f64,
    ) -> Self {
        Self {
            ctx,
            spine,
            sides,
            radius,
            refusal: RefCell::new(None),
            center_solves: Cell::new(0),
            last_parameter: RefCell::new(None),
            exact_spines: RefCell::new(Vec::new()),
            polyline: SpinePolyline::of(spine),
        }
    }

    /// `value`, or the first refusal an evaluation met.
    fn finish<T>(self, value: T) -> Result<T, CodecError> {
        match self.refusal.into_inner() {
            Some(error) => Err(error),
            None => Ok(value),
        }
    }

    /// Start a new use: forget the seed of the next spine inversion and the
    /// centre solves spent.
    pub(super) fn reset_seed(&self) {
        *self.last_parameter.borrow_mut() = None;
        self.center_solves.set(0);
    }

    /// The first resource refusal an evaluation met.
    pub(super) fn take_refusal(&self) -> Option<CodecError> {
        self.refusal.borrow_mut().take()
    }

    fn analytic(&self) -> bool {
        self.sides.iter().all(|side| {
            !side.is_empty()
                && side.iter().all(|candidate| match candidate {
                    Side::Surface(surface) => analytic(surface),
                    Side::Blend(_) | Side::Wedge(_) => true,
                })
        })
    }

    /// The index of the candidate of side `side` the ball at the stored spine
    /// point `point` rolls on: the one whose distance from it is nearest the
    /// radius.
    fn member(&self, side: usize, point: Point3) -> Option<usize> {
        let candidates = self.sides.get(side)?;
        if candidates.len() == 1 {
            return Some(0);
        }
        let mut best: Option<(f64, usize)> = None;
        for (index, surface) in candidates.iter().enumerate() {
            let Some((foot, _)) = self.side_normal(surface, point) else {
                continue;
            };
            let gap = (foot.distance(point) - self.radius).abs();
            if best.as_ref().is_none_or(|(best_gap, _)| gap < *best_gap) {
                best = Some((gap, index));
            }
        }
        best.map(|(_, index)| index)
    }

    /// The candidates at `members`, one index per side.
    fn candidates(&self, members: [usize; 2]) -> Option<[&Side<'a, 'ctx>; 2]> {
        Some([
            self.sides.first()?.get(members[0])?,
            self.sides.get(1)?.get(members[1])?,
        ])
    }

    /// The candidate point of side `side` nearest `point` whose distance from
    /// it is nearest the radius, and the unit natural normal there.
    fn side_foot(&self, side: usize, point: Point3) -> Option<(Point3, cadmpeg_ir::math::Vector3)> {
        let mut best: Option<(f64, (Point3, cadmpeg_ir::math::Vector3))> = None;
        for surface in self.sides.get(side)? {
            let Some(foot) = self.side_normal(surface, point) else {
                continue;
            };
            let gap = (foot.0.distance(point) - self.radius).abs();
            if best.as_ref().is_none_or(|(best_gap, _)| gap < *best_gap) {
                best = Some((gap, foot));
            }
        }
        best.map(|(_, foot)| foot)
    }

    /// The spine parameter nearest `point`, searched near the last inverted
    /// point when there is one.
    fn nearest_spine_parameter(&self, point: Point3) -> Option<f64> {
        let seed = *self.last_parameter.borrow();
        let parameter = match (seed, self.spine) {
            (Some(seed), CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))) => {
                let [lower, upper] =
                    cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs)?.endpoints();
                let window = (upper - lower) * LOCAL_SEARCH_FRACTION;
                let found = self.golden(
                    [(seed - window).max(lower), (seed + window).min(upper)],
                    point,
                )?;
                // The seam of a closed spine has two parameters, so a window
                // past either end continues from the other end.
                let beyond = (seed + window - upper, lower + window - seed);
                let wrapped = if !closed_nurbs(nurbs) {
                    None
                } else if beyond.0 > 0.0 {
                    Some([lower, lower + beyond.0])
                } else if beyond.1 > 0.0 {
                    Some([upper - beyond.1, upper])
                } else {
                    None
                };
                let gap = |s: f64| Some(self.spine_point(s)?.distance(point));
                match wrapped {
                    Some(range) => {
                        let across = self.golden(range, point)?;
                        if gap(across)? < gap(found)? {
                            across
                        } else {
                            found
                        }
                    }
                    None => found,
                }
            }
            _ => self.spine_parameter(point, seed)?,
        };
        *self.last_parameter.borrow_mut() = Some(parameter);
        Some(parameter)
    }

    /// The spine parameter in `[a, b]` nearest `point`, by sampling and then
    /// golden-section search.
    fn golden(&self, [a, b]: [f64; 2], point: Point3) -> Option<f64> {
        let step = (b - a) / f64_from_index(LOCAL_SPINE_SAMPLES)?;
        let mut best = (f64::INFINITY, a);
        for index in 0..=LOCAL_SPINE_SAMPLES {
            let s = a + step * f64_from_index(index)?;
            let gap = self.spine_point(s)?.distance(point);
            if gap < best.0 {
                best = (gap, s);
            }
        }
        self.refine_nearest([(best.1 - step).max(a), (best.1 + step).min(b)], point)
    }

    /// The spine parameter in `[a, b]` nearest `point` by golden-section search.
    fn refine_nearest(&self, [mut a, mut b]: [f64; 2], point: Point3) -> Option<f64> {
        let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
        for _ in 0..SPINE_SECTION_STEPS {
            let left = b - ratio * (b - a);
            let right = a + ratio * (b - a);
            if self.spine_point(left)?.distance(point) <= self.spine_point(right)?.distance(point) {
                b = right;
            } else {
                a = left;
            }
        }
        Some(a.midpoint(b))
    }

    /// The parameter where `curve` is nearest `point`, by Newton steps on
    /// `t·(p − c)` with a secant once the sign changes, inside the stored
    /// spine's domain.
    fn nearest_on(
        &self,
        point: Point3,
        curve: impl Fn(f64) -> Option<Point3>,
    ) -> Option<(f64, Point3, cadmpeg_ir::math::Vector3)> {
        let mut s = self.nearest_spine_parameter(point)?;
        let [lower, upper] = match self.spine {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs)?.endpoints()
            }
            _ => [f64::NEG_INFINITY, f64::INFINITY],
        };
        let mut previous: Option<(f64, f64)> = None;
        let step = TRACK_DIFFERENCE_STEP;
        for _ in 0..MAX_NEAREST_STEPS {
            let at = curve(s)?;
            let (ahead, behind) = ((s + step).min(upper), (s - step).max(lower));
            let (after, before) = (curve(ahead)?, curve(behind)?);
            let derivative = after.vector_from(before);
            let width = ahead - behind;
            if !(width > 0.0 && width.is_finite()) {
                return None;
            }
            let derivative = cadmpeg_ir::math::Vector3::new(
                derivative.x / width,
                derivative.y / width,
                derivative.z / width,
            );
            let speed = derivative.dot(derivative);
            if !(speed > 0.0 && speed.is_finite()) {
                return None;
            }
            let offset = point.vector_from(at);
            let projected = derivative.dot(offset) / speed;
            let outward = (s >= upper && projected > 0.0) || (s <= lower && projected < 0.0);
            if outward || (projected * speed.sqrt()).abs() <= NEAREST_STEP_TOLERANCE_MM {
                *self.last_parameter.borrow_mut() = Some(s);
                return Some((s, at, derivative));
            }
            let delta = match previous {
                Some((last_s, last_projected))
                    if projected.is_sign_negative() != last_projected.is_sign_negative() =>
                {
                    let secant = projected * (s - last_s) / (last_projected - projected);
                    if secant.is_finite() {
                        secant
                    } else {
                        projected
                    }
                }
                _ => {
                    // `t'·(p − c)` from the second difference of the samples.
                    let curving = if s - step >= lower && s + step <= upper {
                        (after.vector_from(at).dot(offset) + before.vector_from(at).dot(offset))
                            / (step * step)
                    } else {
                        0.0
                    };
                    let denominator = speed - curving;
                    let newton = derivative.dot(offset) / denominator;
                    if denominator > 0.0 && newton.is_finite() {
                        newton
                    } else {
                        projected
                    }
                }
            };
            previous = Some((s, projected));
            s = (s + delta).clamp(lower, upper);
        }
        None
    }

    /// The point of the blend's pipe surface nearest `point` and the unit
    /// normal there, pointing away from the spine.
    pub(super) fn pipe_foot(&self, point: Point3) -> Option<(Point3, cadmpeg_ir::math::Vector3)> {
        let (_, center, _) = self.nearest_on(point, |s| self.center(s))?;
        let offset = point.vector_from(center);
        let length = offset.norm();
        if !(length > 0.0 && length.is_finite()) {
            return None;
        }
        let normal =
            cadmpeg_ir::math::Vector3::new(offset.x / length, offset.y / length, offset.z / length);
        let foot = Point3::new(
            center.x + normal.x * self.radius,
            center.y + normal.y * self.radius,
            center.z + normal.z * self.radius,
        );
        Some((foot, normal))
    }

    /// The candidate index of each side the ball at spine parameter `s` rolls on.
    fn members_at(&self, s: f64) -> Option<[usize; 2]> {
        let point = self.spine_point(s)?;
        Some([self.member(0, point)?, self.member(1, point)?])
    }

    /// The point of the contact track of side `side` nearest `point`, and the
    /// track's unit tangent there in the spine's direction.
    pub(super) fn track_point(
        &self,
        side: usize,
        point: Point3,
    ) -> Option<(Point3, cadmpeg_ir::math::Vector3)> {
        let track = |s: f64| {
            let members = self.members_at(s)?;
            let center = self.center_with(s, members)?;
            self.side_normal(self.candidates(members)?.get(side).copied()?, center)
                .map(|(foot, _)| foot)
        };
        let (_, contact, tangent) = self.nearest_on(point, track)?;
        let length = tangent.norm();
        (length > 0.0 && length.is_finite()).then(|| {
            (
                contact,
                cadmpeg_ir::math::Vector3::new(
                    tangent.x / length,
                    tangent.y / length,
                    tangent.z / length,
                ),
            )
        })
    }

    fn pair_member(&self, vertices: &[Point3]) -> Option<usize> {
        let [low, high] = self.range(vertices)?;
        let mut qualifies = [true, true];
        for index in 0..PAIR_STATIONS {
            let s =
                low + (high - low) * f64_from_index(index)? / f64_from_index(PAIR_STATIONS - 1)?;
            let center = self.spine_point(s)?;
            let gaps = [
                self.side_normal(&self.sides[0][0], center)?
                    .0
                    .distance(center)
                    - self.radius,
                self.side_normal(&self.sides[1][0], center)?
                    .0
                    .distance(center)
                    - self.radius,
            ];
            for member in 0..2 {
                let other = 1 - member;
                if !(gaps[member].abs() <= self.radius / 4.0
                    && gaps[other].abs() > self.radius / 2.0)
                {
                    qualifies[member] = false;
                }
            }
        }
        match qualifies {
            [true, false] => Some(0),
            [false, true] => Some(1),
            _ => None,
        }
    }

    /// The point of a side candidate nearest `point` and the unit normal
    /// there.
    fn side_normal(
        &self,
        candidate: &Side<'a, 'ctx>,
        point: Point3,
    ) -> Option<(Point3, cadmpeg_ir::math::Vector3)> {
        match candidate {
            Side::Surface(surface) => super::evaluation::analytic_surface_foot(surface, point),
            Side::Blend(blend) => {
                let foot = blend.pipe_foot(point);
                if let Some(limit) = blend.take_refusal() {
                    self.refusal.borrow_mut().get_or_insert(limit);
                }
                foot
            }
            Side::Wedge(wedge) => Self::wedge_foot(wedge, point),
        }
    }

    /// The point of a wedge nearest `point` and the unit normal there: a
    /// member's foot when `point` is outside that member and the foot is on
    /// the material side of the other, else the nearest point of their edge
    /// with the normal toward `point`.
    fn wedge_foot(wedge: &Wedge<'_>, point: Point3) -> Option<(Point3, cadmpeg_ir::math::Vector3)> {
        for slot in 0..2 {
            let (foot, normal) =
                super::evaluation::analytic_surface_foot(wedge.members[slot], point)?;
            if wedge.outside[slot] * point.vector_from(foot).dot(normal) <= 0.0 {
                continue;
            }
            let other = 1 - slot;
            let (other_foot, other_normal) =
                super::evaluation::analytic_surface_foot(wedge.members[other], foot)?;
            if wedge.outside[other] * foot.vector_from(other_foot).dot(other_normal) <= 0.0 {
                return Some((foot, normal));
            }
        }
        let edge = Self::edge_point(wedge, point)?;
        let offset = point.vector_from(edge);
        let length = offset.norm();
        (length > 0.0 && length.is_finite()).then(|| {
            (
                edge,
                cadmpeg_ir::math::Vector3::new(
                    offset.x / length,
                    offset.y / length,
                    offset.z / length,
                ),
            )
        })
    }

    /// The point of a wedge's edge nearest `point`: the point on both
    /// members' tangent planes nearest `point`, until it lies on both
    /// members.
    fn edge_point(wedge: &Wedge<'_>, point: Point3) -> Option<Point3> {
        let mut current = point;
        for _ in 0..MAX_EDGE_STEPS {
            let (a, na) = super::evaluation::analytic_surface_foot(wedge.members[0], current)?;
            let (b, nb) = super::evaluation::analytic_surface_foot(wedge.members[1], current)?;
            let cross = na.cross(nb);
            if cross.dot(cross) < MIN_EDGE_TRANSVERSALITY {
                return None;
            }
            let (aa, ab, bb) = (na.dot(na), na.dot(nb), nb.dot(nb));
            let ra = na.dot(a.vector_from(point));
            let rb = nb.dot(b.vector_from(point));
            let determinant = aa * bb - ab * ab;
            let la = (ra * bb - rb * ab) / determinant;
            let lb = (rb * aa - ra * ab) / determinant;
            let next = Point3::new(
                point.x + la * na.x + lb * nb.x,
                point.y + la * na.y + lb * nb.y,
                point.z + la * na.z + lb * nb.z,
            );
            if !(next.x.is_finite() && next.y.is_finite() && next.z.is_finite()) {
                return None;
            }
            let step = next.distance(current);
            current = next;
            if step <= EDGE_STEP_TOLERANCE_MM.max(super::evaluation::rounding_scale(current)) {
                return Some(current);
            }
        }
        None
    }

    fn fit(&self, vertices: &[Point3]) -> Option<BlendFit> {
        if !(self.radius > 0.0 && self.radius.is_finite()) {
            return None;
        }
        if !self.analytic() {
            return None;
        }
        let [low, high] = self.range(vertices)?;
        let margin = (high - low) * DOMAIN_MARGIN_FRACTION;
        let [low, high] = self.clamp([low - margin, high + margin]);
        if high.partial_cmp(&low) != Some(std::cmp::Ordering::Greater) {
            return None;
        }
        // A polyline chart's speed jumps at its knots: fit each piece alone.
        let mut breaks = vec![low];
        if let Some(polyline) = &self.polyline {
            breaks.extend(
                polyline
                    .parameters
                    .iter()
                    .copied()
                    .filter(|knot| low < *knot && *knot < high),
            );
        }
        breaks.push(high);
        let pieces = breaks.len() - 1;
        let seeds = SEED_SPANS.div_ceil(pieces).max(1);
        let mut fitted = Vec::new();
        let mut fit_tolerance = BLEND_FIT_TOLERANCE_MM;
        for piece in breaks.windows(2) {
            let [from, to] = [piece[0], piece[1]];
            let mut nodes = Vec::with_capacity(seeds + 1);
            for index in 0..=seeds {
                let s = from + (to - from) * f64_from_index(index)? / f64_from_index(seeds)?;
                nodes.push((s, self.section(s)?, self.derivative(s, [from, to])?));
            }
            fitted.push(nodes[0]);
            for pair in nodes.windows(2) {
                self.split(
                    pair[0],
                    pair[1],
                    [from, to],
                    0,
                    &mut fitted,
                    &mut fit_tolerance,
                )?;
                if fitted.len() > MAX_SPANS {
                    return None;
                }
            }
        }
        let surface = Self::surface(&fitted)?;
        Some(BlendFit {
            surface,
            fit_tolerance,
        })
    }

    /// The spine interval spanned by the face's vertices.
    fn range(&self, vertices: &[Point3]) -> Option<[f64; 2]> {
        let mut parameters = Vec::with_capacity(vertices.len());
        let mut reference = None;
        for vertex in vertices {
            let parameter = self.spine_parameter(*vertex, reference)?;
            reference.get_or_insert(parameter);
            parameters.push(parameter);
        }
        let low = parameters.iter().copied().fold(f64::INFINITY, f64::min);
        let high = parameters.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (low.is_finite() && high.is_finite()).then_some([low, high])
    }

    /// The interval clamped to the spine's own domain, where it has one.
    fn clamp(&self, [low, high]: [f64; 2]) -> [f64; 2] {
        match self.spine {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                match cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs) {
                    Some(domain) => {
                        let [lower, upper] = domain.endpoints();
                        [low.max(lower), high.min(upper)]
                    }
                    None => [low, high],
                }
            }
            _ => [low, high],
        }
    }

    /// The spine parameter nearest `point`. A periodic parameter is taken on
    /// the turn nearest `reference`.
    fn spine_parameter(&self, point: Point3, reference: Option<f64>) -> Option<f64> {
        let tau = std::f64::consts::TAU;
        let near = |angle: f64| match reference {
            Some(reference) => angle + ((reference - angle) / tau).round() * tau,
            None => angle,
        };
        match self.spine {
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) => {
                let origin = line.origin().get();
                Some(point.vector_from(origin).dot(*line.direction().as_raw()))
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) => {
                let offset = point.vector_from(circle.center().get());
                let frame = circle.frame();
                let x = offset.dot(*frame.reference().as_raw());
                let y = offset.dot(*frame.binormal().as_raw());
                Some(near(y.atan2(x)))
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse)) => {
                let offset = point.vector_from(ellipse.center().get());
                let frame = ellipse.frame();
                let x = offset.dot(*frame.reference().as_raw()) / ellipse.major_radius().get();
                let y = offset.dot(*frame.binormal().as_raw()) / ellipse.minor_radius().get();
                Some(near(y.atan2(x)))
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                let [lower, upper] =
                    cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs)?.endpoints();
                let mut breaks: Vec<f64> = nurbs.knots().as_slice().to_vec();
                breaks.dedup();
                breaks.retain(|knot| (lower..=upper).contains(knot));
                let mut best = (f64::INFINITY, lower, lower, upper);
                for pair in breaks.windows(2) {
                    let step = (pair[1] - pair[0]) / f64_from_index(SPINE_SAMPLES)?;
                    for index in 0..=SPINE_SAMPLES {
                        let s = pair[0] + step * f64_from_index(index)?;
                        let gap = self.spine_point(s)?.distance(point);
                        if gap < best.0 {
                            best = (gap, s, (s - step).max(lower), (s + step).min(upper));
                        }
                    }
                }
                let (_, _, a, b) = best;
                self.refine_nearest([a, b], point)
            }
            _ => None,
        }
    }

    /// The ball center at spine parameter `s`: the stored spine point moved
    /// onto the intersection of the two supports' offsets by the radius, on
    /// the side of each support the spine point lies.
    fn center(&self, s: f64) -> Option<Point3> {
        self.center_with(s, self.members_at(s)?)
    }

    /// The ball center at spine parameter `s` rolling on the given support of
    /// each side.
    fn center_with(&self, s: f64, members: [usize; 2]) -> Option<Point3> {
        if let Some(spine) = self.exact_spine(members) {
            if let Some(point) = spine.point(s) {
                return Some(point);
            }
        }
        self.solve_center(s, self.candidates(members)?)
    }

    /// The exact spine of a choice of side candidates over a stored spine
    /// chart's domain, solved once; `None` for an exact stored spine or a
    /// spine that does not solve.
    fn exact_spine(&self, members: [usize; 2]) -> Option<std::rc::Rc<ExactSpine>> {
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = self.spine else {
            return None;
        };
        if nurbs.degree() != 1 {
            return None;
        }
        let key = (members[0], members[1]);
        if let Some((_, spine)) = self.exact_spines.borrow().iter().find(|(k, _)| *k == key) {
            return spine.clone();
        }
        let solved = self
            .solve_spine(nurbs, self.candidates(members)?)
            .map(std::rc::Rc::new);
        self.exact_spines.borrow_mut().push((key, solved.clone()));
        solved
    }

    fn solve_spine(
        &self,
        nurbs: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
        members: [&Side<'a, 'ctx>; 2],
    ) -> Option<ExactSpine> {
        let [lower, upper] = cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs)?.endpoints();
        let mut knots: Vec<f64> = nurbs.knots().as_slice().to_vec();
        knots.dedup();
        knots.retain(|knot| (lower..=upper).contains(knot));
        let node = |s: f64| -> Option<(f64, Point3, cadmpeg_ir::math::Vector3)> {
            let step = TRACK_DIFFERENCE_STEP;
            let (before, after) = ((s - step).max(lower), (s + step).min(upper));
            let difference = self
                .solve_center(after, members)?
                .vector_from(self.solve_center(before, members)?);
            let span = after - before;
            Some((
                s,
                self.solve_center(s, members)?,
                cadmpeg_ir::math::Vector3::new(
                    difference.x / span,
                    difference.y / span,
                    difference.z / span,
                ),
            ))
        };
        let mut runs = Vec::new();
        let mut run: Vec<SpineNode> = Vec::new();
        for knot in knots {
            let Some(end) = node(knot) else {
                if run.len() > 1 {
                    runs.push(std::mem::take(&mut run));
                }
                run.clear();
                continue;
            };
            if run.is_empty() || self.extend_spine(&mut run, end, members, &node).is_none() {
                if run.len() > 1 {
                    runs.push(std::mem::take(&mut run));
                }
                run = vec![end];
            }
        }
        if run.len() > 1 {
            runs.push(run);
        }
        (!runs.is_empty()).then_some(ExactSpine { runs })
    }

    /// Extend `run` to `end` by Hermite pieces within
    /// [`SPINE_CACHE_TOLERANCE_MM`] of solved centers, or `None` when a
    /// center between them does not solve; the pieces already admitted stay.
    fn extend_spine(
        &self,
        run: &mut Vec<SpineNode>,
        end: SpineNode,
        members: [&Side<'a, 'ctx>; 2],
        node: &impl Fn(f64) -> Option<SpineNode>,
    ) -> Option<()> {
        let mut stack = vec![(end, 0_usize)];
        while let Some((end, depth)) = stack.pop() {
            let start = *run.last()?;
            let middle = start.0.midpoint(end.0);
            let exact = self.solve_center(middle, members)?;
            if hermite_spine_point(&[start, end], middle)?.distance(exact)
                <= SPINE_CACHE_TOLERANCE_MM
            {
                run.push(end);
            } else if depth >= MAX_SPINE_CACHE_DEPTH {
                return None;
            } else {
                stack.push((end, depth + 1));
                stack.push((node(middle)?, depth + 1));
            }
        }
        Some(())
    }

    /// The ball center at spine parameter `s` solved from the stored spine
    /// point by Newton steps on the two offsets.
    fn solve_center(&self, s: f64, members: [&Side<'a, 'ctx>; 2]) -> Option<Point3> {
        let solves = self.center_solves.get() + 1;
        self.center_solves.set(solves);
        if solves > MAX_CENTER_SOLVES {
            return None;
        }
        let mut current = self.spine_point(s)?;
        for _ in 0..MAX_CENTER_STEPS {
            let mut planes = [(current, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 0.0)); 2];
            for slot in 0..2 {
                let (foot, normal) = self.side_normal(members[slot], current)?;
                let side = if current.vector_from(foot).dot(normal) < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let offset = Point3::new(
                    foot.x + normal.x * side * self.radius,
                    foot.y + normal.y * side * self.radius,
                    foot.z + normal.z * side * self.radius,
                );
                planes[slot] = (offset, normal);
            }
            let [(a, na), (b, nb)] = planes;
            let cross = na.cross(nb);
            let (aa, ab, bb) = (na.dot(na), na.dot(nb), nb.dot(nb));
            let ra = na.dot(a.vector_from(current));
            let rb = nb.dot(b.vector_from(current));
            let next = if cross.dot(cross) < MIN_CENTER_TRANSVERSALITY {
                // Where the supports touch, their offsets are one plane: the
                // center moves onto it along its normal and keeps its
                // position along the spine.
                let (ua, ub) = (ra / aa.sqrt(), rb / bb.sqrt());
                let flip = if ab < 0.0 { -1.0 } else { 1.0 };
                let gap = ua - flip * ub;
                if !(gap.abs() <= COINCIDENT_OFFSETS_MM && gap.is_finite()) {
                    return None;
                }
                if ua.abs().max(ub.abs()) <= COINCIDENT_OFFSETS_MM {
                    return Some(current);
                }
                let along = ua.midpoint(flip * ub) / aa.sqrt();
                Point3::new(
                    current.x + along * na.x,
                    current.y + along * na.y,
                    current.z + along * na.z,
                )
            } else {
                let determinant = aa * bb - ab * ab;
                let la = (ra * bb - rb * ab) / determinant;
                let lb = (rb * aa - ra * ab) / determinant;
                Point3::new(
                    current.x + la * na.x + lb * nb.x,
                    current.y + la * na.y + lb * nb.y,
                    current.z + la * na.z + lb * nb.z,
                )
            };
            if !(next.x.is_finite() && next.y.is_finite() && next.z.is_finite()) {
                return None;
            }
            let step = next.distance(current);
            current = next;
            if step <= CENTER_STEP_TOLERANCE_MM.max(super::evaluation::rounding_scale(current)) {
                return Some(current);
            }
        }
        None
    }

    fn spine_point(&self, s: f64) -> Option<Point3> {
        if let Some(polyline) = &self.polyline {
            return polyline.point(s);
        }
        match super::evaluation::curve_point(self.ctx, self.spine, s) {
            Ok(point) => point.map(cadmpeg_ir::features::FinitePoint3::get),
            Err(error) => {
                self.refusal.borrow_mut().get_or_insert(error);
                None
            }
        }
    }

    /// The exact homogeneous section at spine parameter `s`.
    fn section(&self, s: f64) -> Option<Section> {
        let center = self.center(s)?;
        let first = self.contact(0, center)?;
        let second = self.contact(1, center)?;
        let a = first.vector_from(center);
        let b = second.vector_from(center);
        let cosine = (a.dot(b) / (self.radius * self.radius)).clamp(-1.0, 1.0);
        let angle = cosine.acos();
        if std::f64::consts::PI - angle < MIN_HALF_TURN_GAP {
            return None;
        }
        let middle = Point3::new(
            center.x + (a.x + b.x) / (1.0 + cosine),
            center.y + (a.y + b.y) / (1.0 + cosine),
            center.z + (a.z + b.z) / (1.0 + cosine),
        );
        let weight = (angle / 2.0).cos();
        Some([
            [first.x, first.y, first.z, 1.0],
            [
                middle.x * weight,
                middle.y * weight,
                middle.z * weight,
                weight,
            ],
            [second.x, second.y, second.z, 1.0],
        ])
    }

    /// The point of side `side` nearest the ball center, which lies one
    /// radius from it.
    fn contact(&self, side: usize, center: Point3) -> Option<Point3> {
        let (contact, _) = self.side_foot(side, center)?;
        ((contact.distance(center) - self.radius).abs() <= CONTACT_RADIUS_RELATIVE * self.radius)
            .then_some(contact)
    }

    /// The derivative of the homogeneous section along the spine, by central
    /// difference inside `domain`.
    fn derivative(&self, s: f64, [low, high]: [f64; 2]) -> Option<Section> {
        let step = (high - low) * DIFFERENCE_STEP_FRACTION;
        let (before, after) = ((s - step).max(low), (s + step).min(high));
        let (a, b) = (self.section(before)?, self.section(after)?);
        let span = after - before;
        let mut derivative = [[0.0; 4]; 3];
        for row in 0..3 {
            for lane in 0..4 {
                derivative[row][lane] = (b[row][lane] - a[row][lane]) / span;
            }
        }
        Some(derivative)
    }

    /// Append the fitted nodes after `start` up to and including `end`, and
    /// raise `largest` to each departure accepted.
    fn split(
        &self,
        start: (f64, Section, Section),
        end: (f64, Section, Section),
        domain: [f64; 2],
        depth: usize,
        out: &mut Vec<(f64, Section, Section)>,
        largest: &mut f64,
    ) -> Option<()> {
        let s = start.0.midpoint(end.0);
        let exact = self.section(s)?;
        let fitted = hermite(start, end, 0.5);
        let mut departure: f64 = 0.0;
        for t in CHECK_FRACTIONS {
            departure = departure.max(arc_point(&fitted, t).distance(arc_point(&exact, t)));
        }
        if departure <= BLEND_FIT_TOLERANCE_MM
            || (depth >= MAX_SPLIT_DEPTH && departure <= BLEND_FIT_LIMIT_MM)
        {
            *largest = largest.max(departure);
            out.push(end);
            return Some(());
        }
        if depth >= MAX_SPLIT_DEPTH || out.len() >= MAX_SPANS {
            return None;
        }
        let middle = (s, exact, self.derivative(s, domain)?);
        self.split(start, middle, domain, depth + 1, out, largest)?;
        self.split(middle, end, domain, depth + 1, out, largest)
    }

    /// The rational surface of the fitted nodes: cubic along the spine with
    /// triple interior knots, quadratic across from the first support to the
    /// second; `u` is arc length of the contact midpoints in millimetres.
    fn surface(nodes: &[(f64, Section, Section)]) -> Option<NurbsSurface> {
        let normal_reversed = normal_toward_center(nodes)?;
        let first = nodes.first()?;
        let last = nodes.last()?;
        let midpoint = |section: &Section| {
            Point3::new(
                section[0][0].midpoint(section[2][0]),
                section[0][1].midpoint(section[2][1]),
                section[0][2].midpoint(section[2][2]),
            )
        };
        let length: f64 = nodes
            .windows(2)
            .map(|pair| midpoint(&pair[1].1).distance(midpoint(&pair[0].1)))
            .sum();
        let extent = last.0 - first.0;
        let speed = length / extent;
        let speed = if speed > 0.0 && speed.is_finite() {
            speed
        } else {
            1.0
        };
        let knot = |s: f64| (s - first.0) * speed;
        let mut u_knots = vec![knot(first.0); 4];
        let mut rows: Vec<[[f64; 4]; 3]> = vec![first.1];
        for pair in nodes.windows(2) {
            let span = pair[1].0 - pair[0].0;
            if span == 0.0 {
                continue;
            }
            let mut ahead = pair[0].1;
            let mut behind = pair[1].1;
            for row in 0..3 {
                for lane in 0..4 {
                    ahead[row][lane] += pair[0].2[row][lane] * span / 3.0;
                    behind[row][lane] -= pair[1].2[row][lane] * span / 3.0;
                }
            }
            rows.extend([ahead, behind, pair[1].1]);
            if pair[1].0 < last.0 {
                u_knots.extend([knot(pair[1].0); 3]);
            }
        }
        u_knots.extend([knot(last.0); 4]);
        let mut grid = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut weighted = Vec::with_capacity(3);
            for control in row {
                let weight = control[3];
                if !(weight > 0.0 && weight.is_finite()) {
                    return None;
                }
                weighted.push(WeightedPole3 {
                    point: Point3::new(
                        control[0] / weight,
                        control[1] / weight,
                        control[2] / weight,
                    ),
                    weight: NonZeroReal::new(weight)?,
                });
            }
            grid.push(weighted);
        }
        NurbsSurface::new(
            NurbsSurfaceAxis::new(3, u_knots, false),
            NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
            NurbsPoleGrid::Rational { rows: grid },
            normal_reversed,
        )
        .ok()
    }
}

/// Whether `∂u × ∂v` of the fitted sections points toward the ball center,
/// read at the widest section, or `None` when that section has no side.
fn normal_toward_center(nodes: &[(f64, Section, Section)]) -> Option<bool> {
    let point = |row: [f64; 4]| Point3::new(row[0] / row[3], row[1] / row[3], row[2] / row[3]);
    let (_, section, derivative) = nodes.iter().max_by(|left, right| {
        let width = |section: &Section| point(section[2]).distance(point(section[0]));
        width(&left.1).total_cmp(&width(&right.1))
    })?;
    let [first, middle, last] = section.map(point);
    let weight = section[0][3];
    let motion = cadmpeg_ir::math::Vector3::new(
        (derivative[0][0] - first.x * derivative[0][3]) / weight,
        (derivative[0][1] - first.y * derivative[0][3]) / weight,
        (derivative[0][2] - first.z * derivative[0][3]) / weight,
    );
    let chord_middle = Point3::new(
        first.x.midpoint(last.x),
        first.y.midpoint(last.y),
        first.z.midpoint(last.z),
    );
    let side = motion
        .cross(middle.vector_from(first))
        .dot(middle.vector_from(chord_middle));
    (side.is_finite() && side != 0.0).then_some(side < 0.0)
}

/// The homogeneous section of the Hermite piece between two nodes at
/// fraction `f`.
fn hermite(start: (f64, Section, Section), end: (f64, Section, Section), f: f64) -> Section {
    let span = end.0 - start.0;
    let (h00, h10, h01, h11) = (
        2.0 * f * f * f - 3.0 * f * f + 1.0,
        f * f * f - 2.0 * f * f + f,
        -2.0 * f * f * f + 3.0 * f * f,
        f * f * f - f * f,
    );
    let mut out = [[0.0; 4]; 3];
    for (row, out_row) in out.iter_mut().enumerate() {
        for (lane, value) in out_row.iter_mut().enumerate() {
            *value = h00 * start.1[row][lane]
                + h10 * span * start.2[row][lane]
                + h01 * end.1[row][lane]
                + h11 * span * end.2[row][lane];
        }
    }
    out
}

/// The point of a homogeneous rational quadratic section at `t`.
fn arc_point(section: &Section, t: f64) -> Point3 {
    let basis = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
    let mut sum = [0.0; 4];
    for (control, weight) in section.iter().zip(basis) {
        for lane in 0..4 {
            sum[lane] += control[lane] * weight;
        }
    }
    Point3::new(sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3])
}

const fn analytic(surface: &SurfaceGeometry) -> bool {
    matches!(
        surface,
        SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Plane(_)
                | SolvedSurfaceGeometry::Cylinder(_)
                | SolvedSurfaceGeometry::Cone(_)
                | SolvedSurfaceGeometry::Sphere(_)
                | SolvedSurfaceGeometry::Torus(_)
        )
    )
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::analytic::{LineCurve, PlaneSurface};
    use cadmpeg_ir::geometry::{
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::units::UnitVector3;

    use super::Side;

    fn plane(origin: Point3, normal: Vector3, reference: Vector3) -> SurfaceGeometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(origin, normal, reference).unwrap(),
        ))
    }

    /// The spine parameter nearest (2, 0, 0) from a seed just below the end of
    /// a square path whose last pole is `last`.
    fn parameter_past_the_seam(last: Point3) -> f64 {
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(10.0, 10.0, 0.0),
            Point3::new(0.0, 10.0, 0.0),
            last,
        ];
        let spine = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 10.0, 20.0, 30.0, 40.0, 40.0],
                points.to_vec(),
                None,
                false,
            )
            .unwrap(),
        ));
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let ball = super::BlendGeometry::with_sides(&ctx, &spine, [Vec::new(), Vec::new()], 1.0);
        *ball.last_parameter.borrow_mut() = Some(39.9);
        ball.nearest_spine_parameter(Point3::new(2.0, 0.0, 0.0))
            .unwrap()
    }

    #[test]
    fn a_spine_inversion_crosses_the_seam_of_a_closed_spine() {
        let parameter = parameter_past_the_seam(Point3::new(0.0, 0.0, 0.0));
        assert!((parameter - 2.0).abs() < 1.0e-6, "{parameter}");
    }

    #[test]
    fn a_spine_inversion_stays_in_its_window_on_an_open_spine() {
        let parameter = parameter_past_the_seam(Point3::new(0.0, 1.0, 0.0));
        assert!(parameter > 35.0, "{parameter}");
    }

    /// A line spine through `origin` along `direction`.
    fn line(origin: Point3, direction: Vector3) -> CurveGeometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::new(
            FinitePoint3::new(origin).unwrap(),
            UnitVector3::normalized(direction).unwrap(),
        )))
    }

    #[test]
    fn a_pair_member_is_the_one_the_stored_spine_lies_at_the_radius_from() {
        // A unit ball rolls along y on the floor z = 0 with its center on
        // x = 1, z = 1. The pair's other member, the wall x = 1.05, passes
        // through the ball: a ball solved against both members would touch
        // each at the radius, so only the stored spine tells them apart.
        let floor = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let wall = plane(
            Point3::new(1.05, 0.0, 0.0),
            Vector3::new(-1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        );
        let spine = line(Point3::new(1.0, 0.0, 1.0), Vector3::new(0.0, 1.0, 0.0));
        let vertices = [Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 2.0, 0.0)];
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let first = super::pair_member(&ctx, &spine, [&floor, &wall], 1.0, &vertices).unwrap();
        let second = super::pair_member(&ctx, &spine, [&wall, &floor], 1.0, &vertices).unwrap();
        assert_eq!((first, second), (Some(0), Some(1)));
    }

    #[test]
    fn a_fitted_blend_normal_points_away_from_the_ball_center() {
        // A unit ball between the floor z = 0 and the wall x = 0 has its
        // center on x = 1, z = 1. Neither the spine's direction nor the
        // order of the supports changes the blend's normal.
        let floor = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let wall = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        );
        let vertices = [
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(0.0, 2.0, 1.0),
        ];
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for direction in [1.0, -1.0] {
            let spine = line(
                Point3::new(1.0, 0.0, 1.0),
                Vector3::new(0.0, direction, 0.0),
            );
            for supports in [[&floor, &wall], [&wall, &floor]] {
                let fitted = super::fit(&ctx, &spine, supports.map(Side::Surface), 1.0, &vertices)
                    .unwrap()
                    .unwrap()
                    .surface;
                let u = fitted.u_knots().as_slice();
                let middle = u[0].midpoint(u[u.len() - 1]);
                let geometry = SolvedSurfaceGeometry::Nurbs(fitted.clone());
                let partials =
                    cadmpeg_ir::eval::surface_partials_solved(&ctx, &geometry, middle, 0.5)
                        .unwrap()
                        .into_raw();
                let mut normal = partials.du.cross(partials.dv);
                if fitted.normal_reversed() {
                    normal = Vector3::new(-normal.x, -normal.y, -normal.z);
                }
                let point = partials.point;
                let center = Point3::new(1.0, point.y, 1.0);
                assert!(normal.dot(point.vector_from(center)) > 0.0);
            }
        }
    }

    #[test]
    fn a_wedge_is_touched_on_a_face_or_on_its_convex_edge() {
        // The material is x <= 0, z <= 0, with the convex edge along y. A
        // ball above the floor and over the material touches the floor, one
        // beside the wall touches the wall, and one beyond both touches the
        // edge.
        let floor = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let wall = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        );
        let spine = line(Point3::new(1.0, 0.0, 1.0), Vector3::new(0.0, 1.0, 0.0));
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let geometry = super::BlendGeometry::new(
            &ctx,
            &spine,
            [vec![Side::Surface(&floor)], vec![Side::Surface(&wall)]],
            1.0,
        )
        .unwrap();
        let wedge = Side::Wedge(
            super::Wedge::new(&ctx, &spine, [&floor, &wall], &[])
                .unwrap()
                .unwrap(),
        );
        for (point, foot) in [
            (Point3::new(-0.5, 2.0, 2.0), Point3::new(-0.5, 2.0, 0.0)),
            (Point3::new(2.0, 2.0, -0.5), Point3::new(0.0, 2.0, -0.5)),
            (Point3::new(1.0, 2.0, 1.0), Point3::new(0.0, 2.0, 0.0)),
        ] {
            let (got, normal) = geometry.side_normal(&wedge, point).unwrap();
            assert!(got.distance(foot) < 1.0e-9, "{got:?} {foot:?}");
            let toward = point.vector_from(foot);
            let alignment = normal.cross(toward).norm();
            assert!(alignment < 1.0e-9 * toward.norm(), "{normal:?}");
        }
    }

    /// A polyline spine through `points` at `parameters`.
    fn polyline(parameters: &[f64], points: &[Point3]) -> CurveGeometry {
        let mut knots = vec![parameters[0]];
        knots.extend_from_slice(parameters);
        knots.push(parameters[parameters.len() - 1]);
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            cadmpeg_ir::geometry::nurbs::NurbsCurve::new(
                1,
                knots,
                cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial {
                    points: points.to_vec(),
                },
                false,
            )
            .unwrap(),
        ))
    }

    #[test]
    fn a_polyline_spine_reads_as_its_stored_chart() {
        let spine = polyline(
            &[0.0, 1.0, 3.0],
            &[
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 2.0, 0.0),
                Point3::new(1.0, 2.0, 4.0),
            ],
        );
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let chart = super::SpinePolyline::of(&spine).unwrap();
        for s in [0.0, 0.25, 1.0, 2.0, 3.0] {
            let stored = super::super::evaluation::curve_point(&ctx, &spine, s)
                .unwrap()
                .unwrap()
                .get();
            assert!(chart.point(s).unwrap().distance(stored) < 1.0e-12, "{s}");
        }
        for s in [-0.5, 3.5] {
            assert!(chart.point(s).is_none());
        }
    }

    #[test]
    fn a_ball_where_its_supports_touch_rests_on_both() {
        // The floor z = 0 and the plane through it facing down touch
        // everywhere: a unit ball on both has its center on z = 1, where the
        // stored spine puts it along the floor. Two parallel planes apart
        // hold no ball of the radius.
        let floor = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let under = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let above = plane(
            Point3::new(0.0, 0.0, 0.5),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let spine = line(Point3::new(0.3, 0.0, 1.2), Vector3::new(0.0, 1.0, 0.0));
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for other in [&floor, &under] {
            let geometry = super::BlendGeometry::new(
                &ctx,
                &spine,
                [vec![Side::Surface(&floor)], vec![Side::Surface(other)]],
                1.0,
            )
            .unwrap();
            let [first, second] = [&geometry.sides[0][0], &geometry.sides[1][0]];
            let center = geometry.solve_center(2.0, [first, second]).unwrap();
            assert!(
                center.distance(Point3::new(0.3, 2.0, 1.0)) < 1.0e-9,
                "{center:?}"
            );
        }
        let geometry = super::BlendGeometry::new(
            &ctx,
            &spine,
            [vec![Side::Surface(&floor)], vec![Side::Surface(&above)]],
            1.0,
        )
        .unwrap();
        let [first, second] = [&geometry.sides[0][0], &geometry.sides[1][0]];
        assert!(geometry.solve_center(2.0, [first, second]).is_none());
    }

    #[test]
    fn a_point_beyond_a_spine_end_is_nearest_that_end() {
        // A unit ball between the floor z = 0 and the wall x = 0 rolls along
        // y from 0 to 2. The pipe point nearest a point past y = 2 lies on
        // the end section's sphere.
        let floor = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let wall = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        );
        let spine = polyline(
            &[0.0, 2.0],
            &[Point3::new(1.0, 0.0, 1.0), Point3::new(1.0, 2.0, 1.0)],
        );
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let geometry = super::BlendGeometry::new(
            &ctx,
            &spine,
            [vec![Side::Surface(&floor)], vec![Side::Surface(&wall)]],
            1.0,
        )
        .unwrap();
        let (foot, _) = geometry.pipe_foot(Point3::new(1.0, 2.5, 3.0)).unwrap();
        let end = Point3::new(1.0, 2.0, 1.0);
        let toward = Vector3::new(0.0, 0.5, 2.0);
        let length = toward.norm();
        let expected = Point3::new(
            end.x + toward.x / length,
            end.y + toward.y / length,
            end.z + toward.z / length,
        );
        assert!(foot.distance(expected) < 1.0e-9, "{foot:?}");
    }

    #[test]
    fn a_fitted_blend_runs_along_its_length_in_millimetres() {
        // A unit ball between the floor z = 0 and the wall x = 0 rolls along
        // y, on a spine chart whose parameter advances 0.0005 per millimetre.
        // The face spans y from 0 to 2, and its surface a twentieth more on
        // each side: 2.2 mm.
        let floor = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let wall = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        );
        let spine = polyline(
            &[0.0, 0.002],
            &[Point3::new(1.0, -1.0, 1.0), Point3::new(1.0, 3.0, 1.0)],
        );
        let vertices = [
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(0.0, 2.0, 1.0),
        ];
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let fitted = super::fit(
            &ctx,
            &spine,
            [Side::Surface(&floor), Side::Surface(&wall)],
            1.0,
            &vertices,
        )
        .unwrap()
        .unwrap();
        // Every section of a ball between two planes is exact on the first
        // spans, so the fit tolerance stays at its floor.
        assert_eq!(fitted.fit_tolerance, super::BLEND_FIT_TOLERANCE_MM);
        let u = fitted.surface.u_knots().as_slice();
        assert!(u[0].abs() < 1.0e-9, "{u:?}");
        assert!((u[u.len() - 1] - 2.2).abs() < 1.0e-6, "{u:?}");
    }

    #[test]
    fn a_fitted_blend_breaks_where_its_spine_chart_turns() {
        // A unit ball between the floor z = 0 and the wall x = 0 rolls along
        // y. The stored spine runs on the ball's center line but advances
        // 2000 mm per unit of its parameter before its middle knot and 1000
        // after it, so the sections are linear in the parameter on each side
        // of the knot and a cubic piece through either side is exact. With a
        // node at the knot the fit keeps its seed spans: two on each side.
        let floor = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let wall = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        );
        let spine = polyline(
            &[0.0, 0.001, 0.003],
            &[
                Point3::new(1.0, -1.0, 1.0),
                Point3::new(1.0, 1.0, 1.0),
                Point3::new(1.0, 3.0, 1.0),
            ],
        );
        let vertices = [
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(0.0, 2.0, 1.0),
        ];
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let fitted = super::fit(
            &ctx,
            &spine,
            [Side::Surface(&floor), Side::Surface(&wall)],
            1.0,
            &vertices,
        )
        .unwrap()
        .unwrap()
        .surface;
        let mut knots = fitted.u_knots().as_slice().to_vec();
        knots.dedup();
        assert_eq!(knots.len(), 5, "{knots:?}");
    }

    #[test]
    fn a_wedge_takes_its_outside_from_the_spine_over_its_face() {
        // The floor z = 0 and the wall x = 0 meet along y. Over the face,
        // from y = 0 to 2, the stored spine runs at x = 0.3, z = 0.95, outside
        // both. Its chart runs on 60 mm beyond the face to x = -1.15, behind
        // the wall, farther from it than anywhere over the face.
        let floor = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let wall = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        );
        let spine = polyline(
            &[0.0, 0.06, 0.063],
            &[
                Point3::new(-1.15, -60.0, 0.95),
                Point3::new(0.3, 0.0, 0.95),
                Point3::new(0.3, 3.0, 0.95),
            ],
        );
        let vertices = [Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 2.0, 0.0)];
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let wedge = super::Wedge::new(&ctx, &spine, [&floor, &wall], &vertices)
            .unwrap()
            .unwrap();
        assert_eq!(wedge.outside, [1.0, 1.0]);
    }

    #[test]
    fn a_point_far_off_a_curved_pipe_finds_its_nearest_section() {
        // A unit ball rolls on the floor z = 0 around the outside of the
        // cylinder of radius 5 about z, so its center runs on the circle of
        // radius 6 at z = 1. From a point 5 inside that circle, which curves
        // toward the point, the pipe's nearest point lies at radius 5 on the
        // point's own bearing.
        let floor = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let wall = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .unwrap(),
        ));
        let steps = 9;
        let (parameters, points): (Vec<f64>, Vec<Point3>) = (0..=steps)
            .map(|index| {
                let angle = std::f64::consts::FRAC_PI_2 * f64::from(index) / f64::from(steps);
                (
                    angle * 1.0e-3,
                    Point3::new(6.0 * angle.cos(), 6.0 * angle.sin(), 1.0),
                )
            })
            .unzip();
        let spine = polyline(&parameters, &points);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let geometry = super::BlendGeometry::new(
            &ctx,
            &spine,
            [vec![Side::Surface(&floor)], vec![Side::Surface(&wall)]],
            1.0,
        )
        .unwrap();
        let bearing = 0.7_f64;
        let point = Point3::new(bearing.cos(), bearing.sin(), 1.0);
        let (foot, _) = geometry.pipe_foot(point).unwrap();
        let expected = Point3::new(5.0 * bearing.cos(), 5.0 * bearing.sin(), 1.0);
        // The solved spine interpolates the centers between solved nodes.
        assert!(foot.distance(expected) < 1.0e-5, "{foot:?}");
    }

    #[test]
    fn an_exact_spine_answers_only_inside_its_runs() {
        let node = |s: f64| (s, Point3::new(s, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
        let spine = super::ExactSpine {
            runs: vec![vec![node(0.0), node(1.0)], vec![node(2.0), node(3.0)]],
        };
        assert!((spine.point(0.5).unwrap().x - 0.5).abs() < 1.0e-12);
        assert!((spine.point(2.5).unwrap().x - 2.5).abs() < 1.0e-12);
        assert!(spine.point(1.5).is_none());
        assert!(spine.point(3.5).is_none());
    }

    #[test]
    fn a_track_point_is_the_contact_of_its_side_nearest_the_point() {
        // A unit ball between the floor z = 0 and the wall x = 0 rolls along
        // y. It touches the floor on the line x = 1, z = 0, and the wall on
        // the line x = 0, z = 1. Both tracks run along +y.
        let floor = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let wall = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        );
        let spine = polyline(
            &[0.0, 2.0],
            &[Point3::new(1.0, 0.0, 1.0), Point3::new(1.0, 2.0, 1.0)],
        );
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let geometry = super::BlendGeometry::new(
            &ctx,
            &spine,
            [vec![Side::Surface(&floor)], vec![Side::Surface(&wall)]],
            1.0,
        )
        .unwrap();
        for (side, near, contact) in [
            (0, Point3::new(1.3, 1.0, 0.2), Point3::new(1.0, 1.0, 0.0)),
            (1, Point3::new(0.2, 1.0, 1.3), Point3::new(0.0, 1.0, 1.0)),
        ] {
            let (point, tangent) = geometry.track_point(side, near).unwrap();
            assert!(point.distance(contact) < 1.0e-9, "{side}: {point:?}");
            assert!(
                tangent.cross(Vector3::new(0.0, 1.0, 0.0)).norm() < 1.0e-9,
                "{side}: {tangent:?}"
            );
            assert!((tangent.y - 1.0).abs() < 1.0e-9, "{side}: {tangent:?}");
        }
    }
}
