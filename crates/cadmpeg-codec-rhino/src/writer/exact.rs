// SPDX-License-Identifier: Apache-2.0
//! Exact rational B-spline forms of the carriers 3DM does not hold.
//!
//! 3DM holds rational NURBS curves and surfaces. A conic curve, an analytic
//! surface, and the clamped piece of a periodic or partly used NURBS carrier
//! each have a rational B-spline that equals it, which this module builds.
//! A conic's rational form is not parameterized by angle: [`ArcAxis`] maps
//! between the analytic parameter and the NURBS one, and a pcurve on an
//! analytic surface is carried through that map ([`composed_pcurve`]).

use std::cmp::Ordering;

use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

/// Relative slack within which a range end counts as a knot-domain end.
const DOMAIN_END_SLACK: f64 = 1e-12;
/// Relative spread within which a pcurve coordinate counts as constant.
const CONSTANT_COORDINATE_SPREAD: f64 = 1e-12;
/// Relative distance within which a rebuilt conic pcurve meets its source.
const REBUILT_PCURVE_TOLERANCE: f64 = 1e-9;
/// Relative distance within which two pcurve pieces meet end to end.
const JOIN_GAP: f64 = 1e-9;
/// Most arcs one parameter range is split into.
const MAX_ARCS: usize = 4096;

fn usize_degree(degree: u32) -> Result<usize, String> {
    usize::try_from(degree).map_err(|_| "the degree exceeds the address space".to_owned())
}

fn index_f64(index: usize) -> Result<f64, String> {
    cadmpeg_core::convert::f64_from_index(index).ok_or_else(|| "an index exceeds f64".to_owned())
}

/// A B-spline curve with homogeneous control points `[x w, y w, z w, w]`.
#[derive(Clone, Debug)]
pub(super) struct Spline {
    degree: usize,
    knots: Vec<f64>,
    poles: Vec<[f64; 4]>,
    rational: bool,
}

impl Spline {
    pub(super) fn from_curve(curve: &NurbsCurve) -> Result<Self, String> {
        let weights = curve.weights();
        let poles = curve
            .control_points()
            .into_iter()
            .enumerate()
            .map(|(index, point)| {
                let point = point.get();
                let weight = weights
                    .as_ref()
                    .and_then(|weights| weights.get(index))
                    .map_or(1.0, |weight| weight.get());
                [point.x * weight, point.y * weight, point.z * weight, weight]
            })
            .collect();
        Ok(Self {
            degree: usize_degree(curve.degree())?,
            knots: curve.knots().to_vec(),
            poles,
            rational: weights.is_some(),
        })
    }

    fn from_pcurve(nurbs: &PcurveNurbs) -> Result<Self, String> {
        let curve = nurbs
            .lift(|point| Point3::new(point.u, point.v, 0.0))
            .map_err(|error| error.to_string())?;
        Self::from_curve(&curve)
    }

    /// The degree-1 spline from `from` to `to` over `domain`.
    fn segment(from: Point3, to: Point3, domain: [f64; 2]) -> Self {
        Self {
            degree: 1,
            knots: vec![domain[0], domain[0], domain[1], domain[1]],
            poles: vec![[from.x, from.y, from.z, 1.0], [to.x, to.y, to.z, 1.0]],
            rational: false,
        }
    }

    /// The knot domain `[knots[p], knots[n]]`.
    pub(super) fn domain(&self) -> [f64; 2] {
        [self.knots[self.degree], self.knots[self.poles.len()]]
    }

    pub(super) fn points(&self) -> Vec<Point3> {
        self.poles
            .iter()
            .map(|pole| Point3::new(pole[0] / pole[3], pole[1] / pole[3], pole[2] / pole[3]))
            .collect()
    }

    fn weights(&self) -> Option<Vec<f64>> {
        self.rational
            .then(|| self.poles.iter().map(|pole| pole[3]).collect())
    }

    pub(super) fn into_curve(self) -> Result<NurbsCurve, String> {
        let points = self.points();
        let weights = self.weights();
        NurbsCurve::from_lanes(
            u32::try_from(self.degree).map_err(|_| "the degree exceeds u32".to_owned())?,
            self.knots,
            points,
            weights,
            false,
        )
        .map_err(|error| error.to_string())
    }

    fn into_pcurve(self) -> Result<PcurveNurbs, String> {
        let points = self
            .points()
            .into_iter()
            .map(|point| Point2::new(point.x, point.y))
            .collect::<Vec<_>>();
        let weights = self.weights();
        PcurveNurbs::from_lanes(
            u32::try_from(self.degree).map_err(|_| "the degree exceeds u32".to_owned())?,
            self.knots,
            points,
            weights,
            false,
        )
        .map_err(|error| error.to_string())
    }

    fn multiplicity(&self, value: f64) -> usize {
        self.knots.iter().filter(|knot| **knot == value).count()
    }

    /// Inserts `value` once (Boehm). `value` lies in the knot domain.
    fn insert(&mut self, value: f64) {
        let degree = self.degree;
        let count = self.poles.len();
        let mut span = degree;
        while span + 1 < count && self.knots[span + 1] <= value {
            span += 1;
        }
        let mut poles = Vec::with_capacity(count + 1);
        for index in 0..=count {
            if index + degree <= span {
                poles.push(self.poles[index]);
            } else if index > span {
                poles.push(self.poles[index - 1]);
            } else {
                let width = self.knots[index + degree] - self.knots[index];
                let alpha = if width > 0.0 {
                    (value - self.knots[index]) / width
                } else {
                    0.0
                };
                let (before, after) = (self.poles[index - 1], self.poles[index]);
                poles.push(std::array::from_fn(|lane| {
                    alpha * after[lane] + (1.0 - alpha) * before[lane]
                }));
            }
        }
        self.poles = poles;
        self.knots.insert(span + 1, value);
    }

    /// The clamped spline that equals this one over `[lo, hi]`, a range
    /// inside the knot domain.
    pub(super) fn clamped(&self, lo: f64, hi: f64) -> Result<Self, String> {
        let [start, end] = self.domain();
        if !(start <= lo && lo < hi && hi <= end) {
            return Err(format!(
                "the range [{lo}, {hi}] is not inside the knot domain [{start}, {end}]"
            ));
        }
        let degree = self.degree;
        let mut spline = self.clone();
        for value in [lo, hi] {
            while spline.multiplicity(value) < degree {
                spline.insert(value);
            }
        }
        let last_lo = spline
            .knots
            .iter()
            .rposition(|knot| *knot == lo)
            .ok_or("the lower end knot is missing")?;
        let first_hi = spline
            .knots
            .iter()
            .position(|knot| *knot == hi)
            .ok_or("the upper end knot is missing")?;
        let first = (last_lo + 1)
            .checked_sub(degree)
            .ok_or("the lower end knot is misplaced")?;
        if first == 0 || first_hi <= first || first_hi > spline.poles.len() {
            return Err("the clamped range has no control points".into());
        }
        let poles = spline.poles[first - 1..first_hi].to_vec();
        let knots = std::iter::repeat_n(lo, degree + 1)
            .chain(spline.knots[first + degree..first_hi].iter().copied())
            .chain(std::iter::repeat_n(hi, degree + 1))
            .collect::<Vec<_>>();
        if knots.len() != poles.len() + degree + 1 {
            return Err("the clamped range has an inconsistent knot vector".into());
        }
        Ok(Self {
            degree,
            knots,
            poles,
            rational: self.rational,
        })
    }

    /// The spline that runs through this clamped one and then `next`, which
    /// is clamped, of the same degree, and starts where this one ends.
    /// `next` is shifted to start at this one's domain end.
    pub(super) fn join(mut self, mut next: Self) -> Result<Self, String> {
        if self.degree != next.degree {
            return Err("the pieces have different degrees".into());
        }
        let end = self.domain()[1];
        let shift = end - next.domain()[0];
        let last = *self.poles.last().ok_or("a piece has no control point")?;
        let first = *next.poles.first().ok_or("a piece has no control point")?;
        let gap = (0..3)
            .map(|lane| (last[lane] / last[3] - first[lane] / first[3]).abs())
            .fold(0.0, f64::max);
        let scale = (0..3)
            .map(|lane| (last[lane] / last[3]).abs())
            .fold(1.0, f64::max);
        if !matches!(
            gap.partial_cmp(&(JOIN_GAP * scale)),
            Some(Ordering::Less | Ordering::Equal)
        ) {
            return Err(format!("the pieces are {gap} apart where they meet"));
        }
        let scale = last[3] / first[3];
        for pole in &mut next.poles {
            for lane in pole.iter_mut() {
                *lane *= scale;
            }
        }
        self.knots.pop();
        self.knots.extend(
            next.knots[self.degree + 1..]
                .iter()
                .map(|knot| knot + shift),
        );
        self.poles.extend(next.poles.into_iter().skip(1));
        self.rational |= next.rational || scale != 1.0;
        Ok(self)
    }

    /// The same spline with its parameter mapped affinely onto `range`.
    fn rebased(mut self, range: [f64; 2]) -> Self {
        let [lo, hi] = self.domain();
        let scale = (range[1] - range[0]) / (hi - lo);
        let last = self.knots.len() - 1;
        for (index, knot) in self.knots.iter_mut().enumerate() {
            *knot = if index <= self.degree {
                range[0]
            } else if index + self.degree >= last {
                range[1]
            } else {
                range[0] + (*knot - lo) * scale
            };
        }
        self
    }

    /// The spline traversed backward over the same domain.
    fn reversed(&self) -> Self {
        let [lo, hi] = self.domain();
        Self {
            degree: self.degree,
            knots: self.knots.iter().rev().map(|knot| lo + hi - knot).collect(),
            poles: self.poles.iter().rev().copied().collect(),
            rational: self.rational,
        }
    }

    /// The clamped spline that equals this one over `range`, with `range`
    /// as its domain. A periodic spline is unwrapped across its seam, over at
    /// most one period.
    pub(super) fn over(&self, periodic: bool, range: [f64; 2]) -> Result<Self, String> {
        let [mut lo, mut hi] = range;
        let [start, end] = self.domain();
        if !periodic {
            let slack = DOMAIN_END_SLACK * (end - start).abs().max(1.0);
            if lo < start - slack || hi > end + slack {
                return Err(format!(
                    "the range [{lo}, {hi}] leaves the knot domain [{start}, {end}]"
                ));
            }
            return Ok(self.clamped(lo.max(start), hi.min(end))?.rebased(range));
        }
        let period = end - start;
        if period.partial_cmp(&0.0) != Some(Ordering::Greater)
            || hi - lo > period * (1.0 + DOMAIN_END_SLACK)
        {
            return Err(format!(
                "the range [{lo}, {hi}] exceeds one period of the periodic curve"
            ));
        }
        let turns = ((lo - start) / period).floor();
        lo -= turns * period;
        hi -= turns * period;
        let slack = DOMAIN_END_SLACK * period.max(1.0);
        if lo >= end - slack {
            lo -= period;
            hi -= period;
        }
        lo = lo.max(start);
        let piece = if hi <= end + slack {
            self.clamped(lo, hi.min(end))?
        } else {
            let first = self.clamped(lo, end)?;
            let rest = (hi - period).min(end);
            if rest <= start + slack {
                first
            } else {
                first.join(self.clamped(start, rest)?)?
            }
        };
        Ok(piece.rebased(range))
    }
}

/// The clamped NURBS curve that equals `curve` over `range`.
pub(super) fn clamped_curve(curve: &NurbsCurve, range: [f64; 2]) -> Result<NurbsCurve, String> {
    Spline::from_curve(curve)?
        .over(curve.periodic(), range)?
        .into_curve()
}

/// One trigonometric or hyperbolic axis written as rational quadratic arcs:
/// the poles of `(cos t, sin t)` (or `(cosh t, sinh t)`) over `[lo, hi]`,
/// with knots at the parameter values that begin and end each arc.
#[derive(Clone, Debug)]
pub(super) struct ArcAxis {
    lo: f64,
    hi: f64,
    spans: usize,
    half: f64,
    hyperbolic: bool,
    pub(super) knots: Vec<f64>,
    /// `(cos, sin)` or `(cosh, sinh)` lanes of each pole, the middle pole of
    /// each arc scaled by its inverse weight.
    pub(super) poles: Vec<[f64; 2]>,
    pub(super) weights: Vec<f64>,
}

impl ArcAxis {
    pub(super) fn new(lo: f64, hi: f64, hyperbolic: bool) -> Result<Self, String> {
        let width = hi - lo;
        let widest = if hyperbolic {
            1.0
        } else {
            std::f64::consts::FRAC_PI_2
        };
        if !(width > 0.0 && width.is_finite()) {
            return Err(format!("the parameter range [{lo}, {hi}] is empty"));
        }
        if !hyperbolic && width > std::f64::consts::TAU * (1.0 + DOMAIN_END_SLACK) {
            return Err(format!(
                "the angular range [{lo}, {hi}] exceeds one revolution"
            ));
        }
        let spans = cadmpeg_core::convert::truncate_f64_to_usize(
            (width / widest * (1.0 - DOMAIN_END_SLACK)).ceil().max(1.0),
        )
        .filter(|spans| *spans <= MAX_ARCS)
        .ok_or_else(|| format!("the parameter range [{lo}, {hi}] needs too many arcs"))?;
        let half = width / (2.0 * index_f64(spans)?);
        let (cos, sin) = trig(hyperbolic);
        let middle_weight = cos(half);
        let break_at = |index: usize| -> Result<f64, String> {
            Ok(if index == spans {
                hi
            } else {
                lo + 2.0 * half * index_f64(index)?
            })
        };
        let mut knots = vec![lo; 3];
        let mut poles = Vec::with_capacity(2 * spans + 1);
        let mut weights = Vec::with_capacity(2 * spans + 1);
        for span in 0..spans {
            let start = break_at(span)?;
            let middle = start + half;
            poles.push([cos(start), sin(start)]);
            weights.push(1.0);
            poles.push([cos(middle) / middle_weight, sin(middle) / middle_weight]);
            weights.push(middle_weight);
            if span + 1 < spans {
                knots.extend([break_at(span + 1)?; 2]);
            }
        }
        poles.push([cos(hi), sin(hi)]);
        weights.push(1.0);
        knots.extend([hi; 3]);
        Ok(Self {
            lo,
            hi,
            spans,
            half,
            hyperbolic,
            knots,
            poles,
            weights,
        })
    }

    /// The start of the arc that holds `value`.
    fn span_start(&self, value: f64) -> f64 {
        let position = ((value - self.lo) / (2.0 * self.half)).floor().max(0.0);
        let last = self.spans - 1;
        let span = cadmpeg_core::convert::truncate_f64_to_usize(position)
            .filter(|span| *span < last)
            .unwrap_or(last);
        // The span index is at most MAX_ARCS, which f64 holds exactly.
        let index = cadmpeg_core::convert::f64_from_index(span).unwrap_or(0.0);
        self.lo + 2.0 * self.half * index
    }

    /// The NURBS parameter at which the arcs reach the analytic parameter
    /// `value`.
    pub(super) fn parameter(&self, value: f64) -> f64 {
        if value <= self.lo {
            return self.lo;
        }
        if value >= self.hi {
            return self.hi;
        }
        let start = self.span_start(value);
        let offset = value - (start + self.half);
        let ratio = if self.hyperbolic {
            (offset / 2.0).tanh() / (self.half / 2.0).tanh()
        } else {
            (offset / 2.0).tan() / (self.half / 2.0).tan()
        };
        start + self.half * (ratio + 1.0)
    }

    /// The analytic parameter the arcs reach at the NURBS parameter `value`.
    #[cfg(test)]
    pub(super) fn analytic(&self, value: f64) -> f64 {
        let start = self.span_start(value);
        let ratio = (value - start) / self.half - 1.0;
        let offset = if self.hyperbolic {
            2.0 * (ratio * (self.half / 2.0).tanh()).atanh()
        } else {
            2.0 * (ratio * (self.half / 2.0).tan()).atan()
        };
        start + self.half + offset
    }
}

/// A real function: `cos`, `sin`, `cosh`, or `sinh`.
type Real = fn(f64) -> f64;

fn trig(hyperbolic: bool) -> (Real, Real) {
    if hyperbolic {
        (f64::cosh, f64::sinh)
    } else {
        (f64::cos, f64::sin)
    }
}

fn add(point: Point3, vector: Vector3, scale: f64) -> Point3 {
    Point3::new(
        point.x + vector.x * scale,
        point.y + vector.y * scale,
        point.z + vector.z * scale,
    )
}

/// The rational NURBS curve that equals `center + x cos t + y sin t`, or
/// `center + x cosh t + y sinh t`, over `range`.
pub(super) fn trigonometric_curve(
    center: Point3,
    x: Vector3,
    y: Vector3,
    hyperbolic: bool,
    range: [f64; 2],
) -> Result<(NurbsCurve, ArcAxis), String> {
    let axis = ArcAxis::new(range[0], range[1], hyperbolic)?;
    let points = axis
        .poles
        .iter()
        .map(|[c, s]| add(add(center, x, *c), y, *s))
        .collect::<Vec<_>>();
    let curve = NurbsCurve::from_lanes(
        2,
        axis.knots.clone(),
        points,
        Some(axis.weights.clone()),
        false,
    )
    .map_err(|error| error.to_string())?;
    Ok((curve, axis))
}

/// The quadratic NURBS curve that equals `vertex + x t² + y t` over
/// `range`, at the same parameter.
pub(super) fn parabolic_curve(
    vertex: Point3,
    x: Vector3,
    y: Vector3,
    range: [f64; 2],
) -> Result<NurbsCurve, String> {
    let [lo, hi] = range;
    if lo.partial_cmp(&hi) != Some(Ordering::Less) {
        return Err(format!("the parameter range [{lo}, {hi}] is empty"));
    }
    let points = vec![
        add(add(vertex, x, lo * lo), y, lo),
        add(add(vertex, x, lo * hi), y, 0.5 * (lo + hi)),
        add(add(vertex, x, hi * hi), y, hi),
    ];
    NurbsCurve::from_lanes(2, vec![lo, lo, lo, hi, hi, hi], points, None, false)
        .map_err(|error| error.to_string())
}

/// The degree-1 NURBS curve from `from` to `to` over `range`.
pub(super) fn segment_curve(
    from: Point3,
    to: Point3,
    range: [f64; 2],
) -> Result<NurbsCurve, String> {
    Spline::segment(from, to, range).into_curve()
}

/// How an analytic surface parameter maps onto its NURBS parameter.
#[derive(Clone, Debug)]
pub(super) enum ParameterMap {
    Identity,
    Arc(ArcAxis),
}

impl ParameterMap {
    pub(super) fn apply(&self, value: f64) -> f64 {
        match self {
            Self::Identity => value,
            Self::Arc(axis) => axis.parameter(value),
        }
    }
}

/// One direction of an analytic surface: its knots, weights, and the
/// `(radial, axial)` lanes of its poles, or the `(cos, sin)` lanes of its
/// angular direction.
struct Direction {
    degree: u32,
    knots: Vec<f64>,
    lanes: Vec<[f64; 2]>,
    weights: Vec<f64>,
    map: ParameterMap,
}

impl Direction {
    fn arc(range: [f64; 2]) -> Result<Self, String> {
        let axis = ArcAxis::new(range[0], range[1], false)?;
        Ok(Self {
            degree: 2,
            knots: axis.knots.clone(),
            lanes: axis.poles.clone(),
            weights: axis.weights.clone(),
            map: ParameterMap::Arc(axis),
        })
    }

    fn linear(range: [f64; 2], lane: impl Fn(f64) -> [f64; 2]) -> Result<Self, String> {
        let [lo, hi] = range;
        if !(lo < hi && (hi - lo).is_finite()) {
            return Err(format!("the parameter range [{lo}, {hi}] is empty"));
        }
        Ok(Self {
            degree: 1,
            knots: vec![lo, lo, hi, hi],
            lanes: vec![lane(lo), lane(hi)],
            weights: vec![1.0, 1.0],
            map: ParameterMap::Identity,
        })
    }

    fn mapped(self, lane: impl Fn([f64; 2]) -> [f64; 2]) -> Self {
        Self {
            lanes: self.lanes.iter().copied().map(lane).collect(),
            ..self
        }
    }
}

/// The rational NURBS patch that equals an analytic surface over a
/// parameter box, with the maps from the analytic to the NURBS parameters.
pub(super) struct AnalyticPatch {
    pub(super) surface: NurbsSurface,
    pub(super) u: ParameterMap,
    pub(super) v: ParameterMap,
}

/// The NURBS patch of the cylinder, cone, sphere, or torus `surface` over
/// `u × v`, in its own parameterization (spec carrier table): the angular
/// directions are rational quadratic arcs and the axial ones degree 1.
pub(super) fn analytic_patch(
    surface: &SolvedSurfaceGeometry,
    u: [f64; 2],
    v: [f64; 2],
) -> Result<AnalyticPatch, String> {
    let (origin, frame, ratio, along) = match surface {
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            let radius = cylinder.radius().get();
            (
                cylinder.origin().get(),
                *cylinder.frame(),
                1.0,
                Direction::linear(v, |value| [radius, value])?,
            )
        }
        SolvedSurfaceGeometry::Cone(cone) => {
            let radius = cone.radius().get();
            let slope = cone.half_angle().get().tan();
            if !slope.is_finite() {
                return Err("the cone half angle has no finite slope".into());
            }
            (
                cone.origin().get(),
                *cone.frame(),
                cone.ratio().get(),
                Direction::linear(v, |value| [radius + value * slope, value])?,
            )
        }
        SolvedSurfaceGeometry::Sphere(sphere) => {
            let radius = sphere.radius().get();
            (
                sphere.center().get(),
                *sphere.frame(),
                1.0,
                Direction::arc(v)?.mapped(|[c, s]| [radius * c, radius * s]),
            )
        }
        SolvedSurfaceGeometry::Torus(torus) => {
            let major = torus.major_radius().get();
            let minor = torus.minor_radius().get();
            (
                torus.center().get(),
                *torus.frame(),
                1.0,
                Direction::arc(v)?.mapped(|[c, s]| [major + minor * c, minor * s]),
            )
        }
        _ => return Err("the surface is not a cylinder, cone, sphere, or torus".into()),
    };
    let around = Direction::arc(u)?;
    let axis = *frame.axis().as_raw();
    let reference = *frame.reference().as_raw();
    let binormal = *frame.binormal().as_raw();
    let mut rows = Vec::with_capacity(around.lanes.len());
    let mut weight_rows = Vec::with_capacity(around.lanes.len());
    for ([c, s], u_weight) in around.lanes.iter().zip(&around.weights) {
        let radial_direction = Vector3::new(
            reference.x * c + binormal.x * ratio * s,
            reference.y * c + binormal.y * ratio * s,
            reference.z * c + binormal.z * ratio * s,
        );
        rows.push(
            along
                .lanes
                .iter()
                .map(|[radial, axial]| add(add(origin, radial_direction, *radial), axis, *axial))
                .collect::<Vec<_>>(),
        );
        weight_rows.push(
            along
                .weights
                .iter()
                .map(|weight| u_weight * weight)
                .collect(),
        );
    }
    let surface = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(around.degree, around.knots, false),
        NurbsSurfaceAxis::new(along.degree, along.knots, false),
        NurbsSurfaceLanes::new(rows, Some(weight_rows)),
        false,
    )
    .map_err(|error| error.to_string())?;
    Ok(AnalyticPatch {
        surface,
        u: around.map,
        v: along.map,
    })
}

/// The bilinear NURBS patch of the plane `origin + u x + v y` over `u × v`,
/// in the plane's own parameters.
pub(super) fn plane_patch(
    origin: Point3,
    x: Vector3,
    y: Vector3,
    u: [f64; 2],
    v: [f64; 2],
) -> Result<NurbsSurface, String> {
    let rows = u
        .iter()
        .map(|u| v.iter().map(|v| add(add(origin, x, *u), y, *v)).collect())
        .collect();
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![u[0], u[0], u[1], u[1]], false),
        NurbsSurfaceAxis::new(1, vec![v[0], v[0], v[1], v[1]], false),
        NurbsSurfaceLanes::new(rows, None),
        false,
    )
    .map_err(|error| error.to_string())
}

/// The clamped sub-patch of `surface` over `u × v`, unwrapping a periodic
/// direction across its seam.
pub(super) fn clamped_surface(
    surface: &NurbsSurface,
    u: [f64; 2],
    v: [f64; 2],
) -> Result<NurbsSurface, String> {
    let grid = surface.control_grid();
    let weights = surface.weights();
    let homogeneous = |row: usize, column: usize| {
        let point = grid[row][column].get();
        let weight = weights
            .as_ref()
            .map_or(1.0, |weights| weights[row][column].get());
        [point.x * weight, point.y * weight, point.z * weight, weight]
    };
    let rational = weights.is_some();
    let u_degree = usize_degree(surface.u_degree())?;
    let v_degree = usize_degree(surface.v_degree())?;
    // Clamp along v in each row, then along u in each column.
    let mut rows = Vec::with_capacity(grid.len());
    let mut v_knots = Vec::new();
    for (row, cells) in grid.iter().enumerate() {
        let spline = Spline {
            degree: v_degree,
            knots: surface.v_knots().to_vec(),
            poles: (0..cells.len())
                .map(|column| homogeneous(row, column))
                .collect(),
            rational,
        }
        .over(surface.v_periodic(), v)?;
        v_knots.clone_from(&spline.knots);
        rows.push(spline.poles);
    }
    let columns = rows.first().map_or(0, Vec::len);
    let mut columns_out = Vec::with_capacity(columns);
    let mut u_knots = Vec::new();
    for column in 0..columns {
        let spline = Spline {
            degree: u_degree,
            knots: surface.u_knots().to_vec(),
            poles: rows.iter().map(|row| row[column]).collect(),
            rational,
        }
        .over(surface.u_periodic(), u)?;
        u_knots.clone_from(&spline.knots);
        columns_out.push(spline.poles);
    }
    let row_count = columns_out.first().map_or(0, Vec::len);
    let points = (0..row_count)
        .map(|row| {
            columns_out
                .iter()
                .map(|column| {
                    let pole = column[row];
                    Point3::new(pole[0] / pole[3], pole[1] / pole[3], pole[2] / pole[3])
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let weight_rows =
        (rational || columns_out.iter().flatten().any(|pole| pole[3] != 1.0)).then(|| {
            (0..row_count)
                .map(|row| columns_out.iter().map(|column| column[row][3]).collect())
                .collect()
        });
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(surface.u_degree(), u_knots, false),
        NurbsSurfaceAxis::new(surface.v_degree(), v_knots, false),
        NurbsSurfaceLanes::new(points, weight_rows),
        surface.normal_reversed(),
    )
    .map_err(|error| error.to_string())
}

/// The point of the pcurve `geometry` at `parameter`.
fn pcurve_point(geometry: &PcurveGeometry, parameter: f64) -> Result<Point2, String> {
    match cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::pcurve_uv(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        geometry,
        parameter,
    )) {
        Ok(Some(point)) => Ok(point.get()),
        Ok(None) => Err(format!("the pcurve has no finite point at {parameter}")),
        Err(limit) => Err(format!(
            "the evaluation of the pcurve at {parameter} is refused: {limit:?}"
        )),
    }
}

/// The clamped 2D spline (in x and y) that equals the pcurve `geometry`
/// over `range`, a range in its own parameter that may run backward.
pub(super) fn pcurve_spline(geometry: &PcurveGeometry, range: [f64; 2]) -> Result<Spline, String> {
    let forward = [range[0].min(range[1]), range[0].max(range[1])];
    if forward[0].partial_cmp(&forward[1]) != Some(Ordering::Less) {
        return Err("the pcurve's parameter interval is empty".into());
    }
    let lift = |point: Point2| Point3::new(point.u, point.v, 0.0);
    let vector = |point: Point2| Vector3::new(point.u, point.v, 0.0);
    let spline = match geometry {
        PcurveGeometry::Line(_) => Spline::segment(
            lift(pcurve_point(geometry, forward[0])?),
            lift(pcurve_point(geometry, forward[1])?),
            forward,
        ),
        PcurveGeometry::Nurbs { nurbs } => {
            Spline::from_pcurve(nurbs)?.over(nurbs.periodic(), forward)?
        }
        PcurveGeometry::Circle(_) | PcurveGeometry::Ellipse(_) | PcurveGeometry::Harmonic(_) => {
            // center + x cos t + y sin t, read from its points at 0, pi/2, pi.
            let at_zero = pcurve_point(geometry, 0.0)?;
            let at_quarter = pcurve_point(geometry, std::f64::consts::FRAC_PI_2)?;
            let at_half = pcurve_point(geometry, std::f64::consts::PI)?;
            let center = Point2::new(0.5 * (at_zero.u + at_half.u), 0.5 * (at_zero.v + at_half.v));
            let x = Point2::new(at_zero.u - center.u, at_zero.v - center.v);
            let y = Point2::new(at_quarter.u - center.u, at_quarter.v - center.v);
            let (curve, _) =
                trigonometric_curve(lift(center), vector(x), vector(y), false, forward)?;
            Spline::from_curve(&curve)?
        }
        PcurveGeometry::Parabola(_) => {
            // vertex + x t^2 + y t, read from its points at -1, 0, 1.
            let before = pcurve_point(geometry, -1.0)?;
            let vertex = pcurve_point(geometry, 0.0)?;
            let after = pcurve_point(geometry, 1.0)?;
            let x = Point2::new(
                0.5 * (after.u + before.u) - vertex.u,
                0.5 * (after.v + before.v) - vertex.v,
            );
            let y = Point2::new(0.5 * (after.u - before.u), 0.5 * (after.v - before.v));
            Spline::from_curve(&parabolic_curve(
                lift(vertex),
                vector(x),
                vector(y),
                forward,
            )?)?
        }
        PcurveGeometry::Hyperbola(_) => {
            // center + x cosh t + y sinh t, read from its points at -1, 0, 1.
            let before = pcurve_point(geometry, -1.0)?;
            let at_zero = pcurve_point(geometry, 0.0)?;
            let after = pcurve_point(geometry, 1.0)?;
            let (cosh, sinh) = (1.0_f64.cosh(), 1.0_f64.sinh());
            let even = Point2::new(0.5 * (after.u + before.u), 0.5 * (after.v + before.v));
            let x = Point2::new(
                (even.u - at_zero.u) / (cosh - 1.0),
                (even.v - at_zero.v) / (cosh - 1.0),
            );
            let center = Point2::new(at_zero.u - x.u, at_zero.v - x.v);
            let y = Point2::new(
                0.5 * (after.u - before.u) / sinh,
                0.5 * (after.v - before.v) / sinh,
            );
            let (curve, _) =
                trigonometric_curve(lift(center), vector(x), vector(y), true, forward)?;
            Spline::from_curve(&curve)?
        }
        _ => return Err("the pcurve kind has no rational form the writer builds".into()),
    };
    // A rebuilt pcurve must agree with the source at its ends and middle.
    if !matches!(geometry, PcurveGeometry::Nurbs { .. }) {
        check_pcurve_ends(geometry, &spline, forward)?;
    }
    Ok(if range[0] > range[1] {
        spline.reversed()
    } else {
        spline
    })
}

fn check_pcurve_ends(
    geometry: &PcurveGeometry,
    spline: &Spline,
    range: [f64; 2],
) -> Result<(), String> {
    let points = spline.points();
    let ends = [(range[0], points.first()), (range[1], points.last())];
    for (parameter, pole) in ends {
        let expected = pcurve_point(geometry, parameter)?;
        let pole = pole.ok_or("the rebuilt pcurve has no control point")?;
        let scale = 1.0 + expected.u.abs().max(expected.v.abs());
        let tolerance = REBUILT_PCURVE_TOLERANCE * scale;
        if (pole.x - expected.u).abs() > tolerance || (pole.y - expected.v).abs() > tolerance {
            return Err("the rebuilt pcurve does not meet its source at its ends".into());
        }
    }
    Ok(())
}

/// The pcurve `spline` carried into the parameters of an analytic surface's
/// NURBS patch by `u` and `v`.
///
/// An angular coordinate that stays constant maps to a constant. Where one
/// angular coordinate varies, monotonically, and the other coordinate is
/// constant, the image is the straight segment between the mapped ends:
/// the same image under another parameter over the same interval. Any other
/// composition has no exact NURBS form.
pub(super) fn composed_pcurve(
    spline: Spline,
    u: &ParameterMap,
    v: &ParameterMap,
) -> Result<PcurveGeometry, String> {
    let points = spline.points();
    let lane = |point: &Point3, axis: usize| if axis == 0 { point.x } else { point.y };
    let constant = |axis: usize| {
        let first = lane(&points[0], axis);
        let scale = 1.0 + first.abs();
        points
            .iter()
            .all(|point| (lane(point, axis) - first).abs() <= CONSTANT_COORDINATE_SPREAD * scale)
            .then_some(first)
    };
    let maps = [u, v];
    let constants = [constant(0), constant(1)];
    let varying_arcs = (0..2)
        .filter(|axis| matches!(maps[*axis], ParameterMap::Arc(_)) && constants[*axis].is_none())
        .collect::<Vec<_>>();
    let mut spline = spline;
    match varying_arcs.as_slice() {
        [] => {
            for axis in 0..2 {
                if let (ParameterMap::Arc(_), Some(value)) = (maps[axis], constants[axis]) {
                    let mapped = maps[axis].apply(value);
                    for pole in &mut spline.poles {
                        pole[axis] = mapped * pole[3];
                    }
                }
            }
        }
        [axis] => {
            let other = 1 - axis;
            if constants[other].is_none() {
                return Err(
                    "the pcurve moves along both parameters of a surface with an angular \
                     parameter, and its image in the NURBS parameters has no exact NURBS form"
                        .into(),
                );
            }
            let values = points
                .iter()
                .map(|point| lane(point, *axis))
                .collect::<Vec<_>>();
            let rising = values.windows(2).all(|pair| pair[1] >= pair[0]);
            let falling = values.windows(2).all(|pair| pair[1] <= pair[0]);
            if !(rising || falling) {
                return Err(
                    "the pcurve turns back along an angular parameter, and its image in the \
                     NURBS parameters has no exact NURBS form the writer builds"
                        .into(),
                );
            }
            let (first, last) = (points[0], points[points.len() - 1]);
            let map =
                |point: Point3| Point3::new(maps[0].apply(point.x), maps[1].apply(point.y), 0.0);
            spline = Spline::segment(map(first), map(last), spline.domain());
        }
        _ => {
            return Err(
                "the pcurve moves along two angular parameters, and its image in the NURBS \
                 parameters has no exact NURBS form"
                    .into(),
            )
        }
    }
    Ok(PcurveGeometry::Nurbs {
        nurbs: spline.into_pcurve()?,
    })
}

/// The pcurve `spline` as NURBS pcurve geometry, unchanged.
pub(super) fn pcurve_geometry(spline: Spline) -> Result<PcurveGeometry, String> {
    Ok(PcurveGeometry::Nurbs {
        nurbs: spline.into_pcurve()?,
    })
}

#[cfg(test)]
mod tests;
