// SPDX-License-Identifier: Apache-2.0
//! Face boundaries: shared edge samples, and each loop as a polyline in the
//! parameter plane of its face.

use std::collections::HashMap;
use std::f64::consts::TAU;
use std::rc::Rc;

use cadmpeg_core::convert::f64_from_index;
use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::pcurve_uv;
use cadmpeg_ir::eval::{finite_or_refusal, model_curve_point_by_id, nurbs_curve_parameter_domain};
use cadmpeg_ir::geometry::{PlacedCurve, SolvedCurveGeometry};
use cadmpeg_ir::ids::{CurveId, EdgeId};
use cadmpeg_ir::index::{ModelIndex, StandardIndex};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Coedge, Edge, Sense};

use crate::count::{ceil_count, exceeds_limit};
use crate::face::Refusal;
use crate::plane::{distance, dot, length, mid3, sub, Uv};
use crate::surface::Carrier;

/// Seed segments of a curve of a kind with no seeding rule.
const SEEDS: usize = 8;
/// Largest seed segment count of one edge.
const MAX_SEEDS: usize = 256;
/// Ratio of a loop's parameter area to its squared extent below which the
/// loop has no area.
const ZERO_AREA_RATIO: f64 = 1e-9;
/// Deepest bisection of one edge segment.
const MAX_DEPTH: u32 = 18;
/// Largest sample count of one edge.
const MAX_EDGE_SAMPLES: usize = 1 << 17;
/// Ternary-search steps of a curve-parameter projection.
const NEAREST_STEPS: usize = 100;
/// Samples of the coarse search of a curve-parameter projection.
const NEAREST_SAMPLES: u8 = 64;
/// Ratio of a pcurve point's distance from its edge point to the surface
/// size above which the pcurve is not used for that coedge.
const PCURVE_MISMATCH: f64 = 1e-3;
/// Ratio of the distance between a loop's end and start points to the
/// surface size below which they are one point.
const JUNCTION_POINT_RATIO: f64 = 1e-9;
/// Ratio of a loop's closing parameter gap to its parameter extent below
/// which the gap lies between two coedge traces that meet at one vertex.
const JUNCTION_GAP_RATIO: f64 = 1e-2;
/// Samples of a parameter segment tested for lying on a pole.
const POLE_LINE_SAMPLES: u8 = 8;
/// Ratio of the parameter gap between the traces of two consecutive
/// coedges to their parameter scale below which they meet at one point.
const MEET_RATIO: f64 = 1e-7;
/// Ratio of the across-pole displacement of a loop's closure to its
/// along-pole displacement below which the closure runs along a pole line.
const POLE_LINE_SLOPE: f64 = 1e-3;
/// Factor by which a first coedge traced again from the loop's end must
/// narrow its widest gap from the surface to replace the first trace.
const RESEED_GAIN: f64 = 0.5;
/// Ratio of a half chord's length to the chord tolerance below which the
/// half chord has no direction for the turn test.
const TURN_LENGTH_RATIO: f64 = 1e-6;

/// The samples of one edge, from its start vertex to its end vertex.
#[derive(Debug)]
pub(crate) struct EdgeSamples {
    /// Curve parameters, increasing; empty for an edge with no curve.
    pub(crate) params: Vec<f64>,
    pub(crate) points: Vec<Point3>,
    /// The edge has no curve: its samples are its two vertices.
    pub(crate) degenerate: bool,
}

/// Edge samples by edge identity, shared by the faces of one model.
#[derive(Debug, Default)]
pub(crate) struct EdgeCache {
    edges: HashMap<String, Rc<EdgeSamples>>,
}

/// The sampling state of one model.
pub(crate) struct Sampler<'t, 'ir> {
    pub(crate) index: &'t ModelIndex<'ir>,
    pub(crate) work: &'t WorkBudget,
    /// The admission each evaluation of a boundary runs under.
    pub(crate) admission: EvaluationAdmission<'ir, 'ir>,
    pub(crate) chord: f64,
    pub(crate) angle: f64,
}

fn resource(limit: impl std::fmt::Debug) -> Refusal {
    Refusal::new(format!(
        "an evaluation exceeded a resource limit: {limit:?}"
    ))
}

/// The solved form of a curve, looking through placements.
fn basis(geometry: &SolvedCurveGeometry) -> &SolvedCurveGeometry {
    match geometry {
        SolvedCurveGeometry::Transformed(placed) => basis(placed.basis()),
        other => other,
    }
}

/// The natural parameter domain of a curve and whether it is periodic.
pub(crate) fn natural_domain(geometry: &SolvedCurveGeometry) -> Option<([f64; 2], bool)> {
    match geometry {
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => {
            Some(([0.0, TAU], true))
        }
        SolvedCurveGeometry::Nurbs(nurbs) => {
            nurbs_curve_parameter_domain(nurbs).map(|domain| (domain.endpoints(), nurbs.periodic()))
        }
        SolvedCurveGeometry::Transformed(placed) => natural_domain(PlacedCurve::basis(placed)),
        _ => None,
    }
}

impl Sampler<'_, '_> {
    pub(crate) fn curve_point(&self, curve: &CurveId, t: f64) -> Result<Option<Point3>, Refusal> {
        finite_or_refusal(self.admission.within_work_slice(self.work, |admission| {
            model_curve_point_by_id(admission, self.index, curve, t)
        }))
        .map(|point| point.map(cadmpeg_ir::features::geometry::FinitePoint3::get))
        .map_err(resource)
    }

    fn vertex_point(&self, vertex: &cadmpeg_ir::ids::VertexId) -> Result<Point3, Refusal> {
        let vertex = self
            .index
            .vertices(vertex.as_str(), StandardIndex)
            .ok_or_else(|| Refusal::new(format!("the vertex {vertex} is not in the model")))?;
        let point = self
            .index
            .points(vertex.point.as_str(), StandardIndex)
            .ok_or_else(|| {
                Refusal::new(format!("the point {} is not in the model", vertex.point))
            })?;
        Ok(point.position().get())
    }

    /// The parameter in `domain` of the curve point nearest `point`.
    fn nearest(
        &self,
        curve: &CurveId,
        point: Point3,
        domain: [f64; 2],
    ) -> Result<Option<f64>, Refusal> {
        let gap = |t: f64| -> Result<Option<f64>, Refusal> {
            Ok(self.curve_point(curve, t)?.map(|on| distance(on, point)))
        };
        let span = domain[1] - domain[0];
        let step = span / f64::from(NEAREST_SAMPLES);
        let mut best: Option<(f64, f64)> = None;
        for sample in 0..=NEAREST_SAMPLES {
            let t = domain[0] + step * f64::from(sample);
            if let Some(value) = gap(t)? {
                if best.is_none_or(|(_, nearest)| value < nearest) {
                    best = Some((t, value));
                }
            }
        }
        let Some((t, _)) = best else {
            return Ok(None);
        };
        let (mut low, mut high) = (t - step, t + step);
        for _ in 0..NEAREST_STEPS {
            let first = low + (high - low) / 3.0;
            let second = high - (high - low) / 3.0;
            match (gap(first)?, gap(second)?) {
                (Some(a), Some(b)) if a <= b => high = second,
                (Some(_), Some(_)) => low = first,
                _ => break,
            }
        }
        Ok(Some(f64::midpoint(low, high)))
    }

    /// The parameter range of an edge, from the model or from its vertices.
    fn range(&self, edge: &Edge, curve: &CurveId) -> Result<[f64; 2], Refusal> {
        if let Some(range) = edge.param_range() {
            return Ok(range.get());
        }
        let geometry = self
            .index
            .curves(curve.as_str(), StandardIndex)
            .and_then(|curve| curve.geometry.solved());
        let Some(ends) = &edge.ends else {
            return geometry
                .and_then(natural_domain)
                .map(|(domain, _)| domain)
                .ok_or_else(|| {
                    Refusal::new(format!("the edge {} has no parameter range", edge.id))
                });
        };
        let start = self.vertex_point(&ends.start)?;
        let end = self.vertex_point(&ends.end)?;
        if let Some(SolvedCurveGeometry::Line(line)) = geometry {
            let origin = line.origin().get();
            let direction = cadmpeg_ir::math::Vector3::from(line.direction());
            let along = |point: Point3| dot(sub(point, origin), direction);
            return Ok([along(start), along(end)]);
        }
        let (domain, periodic) = geometry
            .and_then(natural_domain)
            .ok_or_else(|| Refusal::new(format!("the edge {} has no parameter range", edge.id)))?;
        let first = self
            .nearest(curve, start, domain)?
            .ok_or_else(|| Refusal::new(format!("the edge {} start is off its curve", edge.id)))?;
        if ends.start == ends.end {
            return Ok(if periodic {
                [first, first + (domain[1] - domain[0])]
            } else {
                domain
            });
        }
        let mut last = self
            .nearest(curve, end, domain)?
            .ok_or_else(|| Refusal::new(format!("the edge {} end is off its curve", edge.id)))?;
        if periodic && last <= first {
            last += domain[1] - domain[0];
        }
        Ok([first, last])
    }

    /// The samples of `edge`, from the cache or sampled to the chord and
    /// angle limits.
    pub(crate) fn edge(
        &self,
        cache: &mut EdgeCache,
        edge: &Edge,
    ) -> Result<Rc<EdgeSamples>, Refusal> {
        if let Some(samples) = cache.edges.get(edge.id.as_str()) {
            return Ok(Rc::clone(samples));
        }
        let samples = match (edge.curve(), &edge.ends) {
            (None, Some(ends)) => EdgeSamples {
                params: Vec::new(),
                points: vec![
                    self.vertex_point(&ends.start)?,
                    self.vertex_point(&ends.end)?,
                ],
                degenerate: true,
            },
            (None, None) => {
                return Err(Refusal::new(format!(
                    "the edge {} has neither a curve nor a vertex",
                    edge.id
                )));
            }
            (Some(curve), Some(ends)) => self.sample(
                edge,
                curve,
                self.vertex_point(&ends.start)?,
                self.vertex_point(&ends.end)?,
            )?,
            (Some(curve), None) => {
                let [low, _] = self.range(edge, curve)?;
                let seam = self.curve_point(curve, low)?.ok_or_else(|| {
                    Refusal::new(format!(
                        "the curve of edge {} has no point at {low}",
                        edge.id
                    ))
                })?;
                self.sample(edge, curve, seam, seam)?
            }
        };
        let samples = Rc::new(samples);
        cache
            .edges
            .insert(edge.id.as_str().to_owned(), Rc::clone(&samples));
        Ok(samples)
    }

    fn sample(
        &self,
        edge: &Edge,
        curve: &CurveId,
        start: Point3,
        end: Point3,
    ) -> Result<EdgeSamples, Refusal> {
        let [low, high] = self.range(edge, curve)?;
        let span = high - low;
        let point = |t: f64| -> Result<Point3, Refusal> {
            self.curve_point(curve, t)?.ok_or_else(|| {
                Refusal::new(format!("the curve of edge {} has no point at {t}", edge.id))
            })
        };
        // Seeds keep a closed or wiggly curve from collapsing into one chord
        // whose midpoint happens to lie on it.
        let geometry = self
            .index
            .curves(curve.as_str(), StandardIndex)
            .and_then(|curve| curve.geometry.solved());
        let seeds = match geometry.map(basis) {
            Some(SolvedCurveGeometry::Line(_)) => 1,
            Some(SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_)) => {
                ceil_count(span.abs() / (TAU / 8.0)).ok_or_else(|| {
                    Refusal::new(format!("the edge {} has an unbounded range", edge.id))
                })?
            }
            Some(SolvedCurveGeometry::Nurbs(nurbs)) => {
                let knots = nurbs.knots().as_slice();
                let spans = knots
                    .windows(2)
                    .filter(|pair| pair[0] < pair[1] && pair[0] < high && pair[1] > low)
                    .count();
                2 * spans
            }
            _ => SEEDS,
        }
        .clamp(1, MAX_SEEDS);
        let seeds_f = f64_from_index(seeds).ok_or_else(|| Refusal::new("too many edge seeds"))?;
        let mut params = vec![low];
        let mut points = vec![start];
        for seed in 0..seeds {
            let from = low + span * f64_from_index(seed).unwrap_or_default() / seeds_f;
            let to = low + span * f64_from_index(seed + 1).unwrap_or_default() / seeds_f;
            let a = if seed == 0 { start } else { point(from)? };
            let b = if seed + 1 == seeds { end } else { point(to)? };
            self.bisect(&point, (from, a), (to, b), 0, &mut params, &mut points)?;
            if params.len() > MAX_EDGE_SAMPLES {
                return Err(Refusal::new(format!(
                    "the edge {} needs too many samples",
                    edge.id
                )));
            }
        }
        if params.len() == 2 && !matches!(geometry.map(basis), Some(SolvedCurveGeometry::Line(_))) {
            // A curved edge has at least one interior sample.
            let middle = low + span / 2.0;
            params.insert(1, middle);
            points.insert(1, point(middle)?);
        }
        Ok(EdgeSamples {
            params,
            points,
            degenerate: false,
        })
    }

    /// Appends the samples after `a` up to and including `b`.
    fn bisect(
        &self,
        point: &dyn Fn(f64) -> Result<Point3, Refusal>,
        a: (f64, Point3),
        b: (f64, Point3),
        depth: u32,
        params: &mut Vec<f64>,
        points: &mut Vec<Point3>,
    ) -> Result<(), Refusal> {
        let middle = f64::midpoint(a.0, b.0);
        let on = point(middle)?;
        let chord_mid = mid3(a.1, b.1);
        let sag = distance(on, chord_mid);
        // The two half chords turn by half the turn of the curve between the
        // ends.
        let (first, second) = (sub(on, a.1), sub(b.1, on));
        let floor = TURN_LENGTH_RATIO * self.chord;
        let turn = if length(first) > floor && length(second) > floor {
            2.0 * crate::plane::angle(first, second)
        } else {
            0.0
        };
        if depth < MAX_DEPTH && (exceeds_limit(sag, self.chord) || exceeds_limit(turn, self.angle))
        {
            self.bisect(point, a, (middle, on), depth + 1, params, points)?;
            self.bisect(point, (middle, on), b, depth + 1, params, points)?;
        } else {
            params.push(b.0);
            points.push(b.1);
        }
        Ok(())
    }
}

/// One coedge traced in the parameter plane, in traversal order.
#[derive(Debug, Clone)]
pub(crate) struct Traced<'ir> {
    pub(crate) coedge: &'ir Coedge,
    pub(crate) uv: Vec<Uv>,
    pub(crate) points: Vec<Point3>,
    /// Curve parameters in traversal order; empty for a degenerate edge.
    pub(crate) params: Vec<f64>,
    pub(crate) degenerate: bool,
    /// The trace comes from the coedge's pcurve.
    pub(crate) from_pcurve: bool,
}

/// A loop in the parameter plane.
#[derive(Debug, Clone)]
pub(crate) struct TracedLoop<'ir> {
    pub(crate) coedges: Vec<Traced<'ir>>,
    /// The parameter displacement from the loop's start to its end.
    pub(crate) closure: Uv,
    /// The loop ends and starts on two branches of one pole and has no
    /// regular junction to start from; it closes along the pole line.
    pub(crate) closes_at_pole: bool,
}

/// The directed samples of `coedge`.
fn directed(samples: &EdgeSamples, coedge: &Coedge) -> (Vec<f64>, Vec<Point3>) {
    let mut params = samples.params.clone();
    let mut points = samples.points.clone();
    if coedge.sense == Sense::Reversed {
        params.reverse();
        points.reverse();
    }
    (params, points)
}

/// Traces a ring loop of a face on `carrier`.
pub(crate) fn trace<'ir>(
    sampler: &Sampler<'_, 'ir>,
    cache: &mut EdgeCache,
    carrier: &Carrier<'_, 'ir>,
    coedges: &[&'ir Coedge],
    left: bool,
) -> Result<TracedLoop<'ir>, Refusal> {
    let index = sampler.index;
    let mut traced = Vec::new();
    let mut previous: Option<Uv> = None;
    for coedge in coedges {
        let edge = index
            .edges(coedge.edge.as_str(), StandardIndex)
            .ok_or_else(|| Refusal::new(format!("the edge {} is not in the model", coedge.edge)))?;
        let samples = sampler.edge(cache, edge)?;
        let (params, points) = directed(&samples, coedge);
        let mut uv = pcurve_trace(sampler, carrier, coedge, &params, &points)?;
        let from_pcurve = uv.is_some();
        if uv.is_none() {
            uv = Some(invert_lane(carrier, &points, previous, &coedge.edge)?);
        }
        let uv = uv.unwrap_or_default();
        previous = uv.last().copied();
        traced.push(Traced {
            coedge,
            uv,
            points,
            params,
            degenerate: samples.degenerate,
            from_pcurve,
        });
    }
    // The loop's first sample has no seed and can land on another turn of
    // a surface wound many times; the loop's end seeds its first coedge
    // again, and the trace that lies nearer its samples stands.
    let end = traced.last().and_then(|last| last.uv.last()).copied();
    if let (true, Some(end), Some(first)) = (traced.len() > 1, end, traced.first()) {
        if !first.from_pcurve && !first.degenerate {
            let again = invert_lane(carrier, &first.points, Some(end), &first.coedge.edge)?;
            if widest_gap(carrier, &again, &first.points)?
                < RESEED_GAIN * widest_gap(carrier, &first.uv, &first.points)?
            {
                traced[0].uv = again;
            }
        }
    }
    settle(carrier, &mut traced)?;
    let mut closure = closure_of(&traced);
    unwind_at_pole(carrier, &mut traced, closure)?;
    closure = closure_of(&traced);
    if closure == [0.0, 0.0] {
        open_at_pole(carrier, &mut traced, left)?;
    }
    closure = closure_of(&traced);
    let mut closes_at_pole = false;
    if closure != [0.0, 0.0] && pole_closure(carrier, &traced, closure, sampler.chord)? {
        // A loop that starts on a pole restarts at a junction off it, so
        // that its pole crossing joins two coedges inside the loop.
        if let Some(start) = regular_junction(carrier, &traced)? {
            traced.rotate_left(start);
            closure = closure_of(&traced);
        } else {
            closes_at_pole = true;
            closure = [0.0, 0.0];
            // Every junction lies on a pole, as for a lune between two
            // meridians: the loop takes the side of the seam its direction
            // selects.
            let polygon: Vec<Uv> = traced
                .iter()
                .flat_map(|coedge| coedge.uv.iter().copied())
                .collect();
            if (crate::plane::signed_area(&polygon) > 0.0) != left {
                shift_at_pole(carrier, &mut traced, left)?;
            }
        }
    }
    if closure != [0.0, 0.0] {
        if let Some(snapped) = junction_closure(&traced, closure, carrier.size, carrier.periods) {
            closure = snapped;
        }
    }
    Ok(TracedLoop {
        coedges: traced,
        closure,
        closes_at_pole,
    })
}

/// The parameters of each of `points` in turn, each projection seeded by the
/// one before and the first by `seed`.
fn invert_lane(
    carrier: &Carrier<'_, '_>,
    points: &[Point3],
    seed: Option<Uv>,
    edge: &EdgeId,
) -> Result<Vec<Uv>, Refusal> {
    let mut lane = Vec::with_capacity(points.len());
    let mut seed = seed;
    for point in points {
        let found = carrier.invert(*point, seed)?.ok_or_else(|| {
            Refusal::new(format!(
                "a sample of edge {edge} does not invert onto its face"
            ))
        })?;
        lane.push(found);
        seed = Some(found);
    }
    Ok(lane)
}

/// The largest distance between a sample and the surface point at its
/// parameters.
fn widest_gap(carrier: &Carrier<'_, '_>, uv: &[Uv], points: &[Point3]) -> Result<f64, Refusal> {
    let mut widest = 0.0_f64;
    for (at, point) in uv.iter().zip(points) {
        let gap = carrier
            .point(*at)?
            .map_or(f64::INFINITY, |on| distance(on, *point));
        widest = widest.max(gap);
    }
    Ok(widest)
}

/// The closure of a loop that ends `closure` from its start at its start
/// point, when the displacement is a whole number of periods, zero on an
/// axis with no period, up to a gap between the traces of two coedges that
/// meet there: small against the loop's parameter extent.
fn junction_closure(
    traced: &[Traced<'_>],
    closure: Uv,
    size: f64,
    periods: [Option<f64>; 2],
) -> Option<Uv> {
    let first = traced.first().and_then(|first| first.points.first())?;
    let last = traced.last().and_then(|last| last.points.last())?;
    if distance(*first, *last) > JUNCTION_POINT_RATIO * size.max(1.0) {
        return None;
    }
    let mut extent = [0.0_f64; 2];
    for (axis, span) in extent.iter_mut().enumerate() {
        let values = traced
            .iter()
            .flat_map(|coedge| coedge.uv.iter().map(move |uv| uv[axis]));
        let (low, high) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| {
            (low.min(value), high.max(value))
        });
        *span = high - low;
    }
    let mut snapped = [0.0; 2];
    for axis in 0..2 {
        if let Some(period) = periods[axis] {
            snapped[axis] = (closure[axis] / period).round() * period;
        }
        let gap = closure[axis] - snapped[axis];
        if gap.abs() > JUNCTION_GAP_RATIO * extent[axis].max(extent[1 - axis]) {
            return None;
        }
    }
    Some(snapped)
}

/// The first coedge after the first that starts where its predecessor ends,
/// at a point where neither surface parameter is singular.
fn regular_junction(
    carrier: &Carrier<'_, '_>,
    traced: &[Traced<'_>],
) -> Result<Option<usize>, Refusal> {
    for at in 1..traced.len() {
        let (Some(end), Some(start)) = (
            traced[at - 1].uv.last().copied(),
            traced[at].uv.first().copied(),
        ) else {
            continue;
        };
        let scale = 1.0 + start[0].abs() + start[1].abs();
        if (end[0] - start[0]).abs().max((end[1] - start[1]).abs()) > MEET_RATIO * scale {
            continue;
        }
        if !carrier.singular(start, 0)? && !carrier.singular(start, 1)? {
            return Ok(Some(at));
        }
    }
    Ok(None)
}

/// Whether a loop whose trace ends `closure` from its start meets its start
/// along a pole line: the displacement runs along one parameter, and the
/// surface stays within `chord` of the loop's start point across it.
fn pole_closure(
    carrier: &Carrier<'_, '_>,
    traced: &[Traced<'_>],
    closure: Uv,
    chord: f64,
) -> Result<bool, Refusal> {
    let (Some(first), Some(point)) = (
        traced.first().and_then(|first| first.uv.first()).copied(),
        traced
            .first()
            .and_then(|first| first.points.first())
            .copied(),
    ) else {
        return Ok(false);
    };
    if !along_one_parameter(closure) {
        return Ok(false);
    }
    for step in 1..POLE_LINE_SAMPLES {
        let fraction = f64::from(step) / f64::from(POLE_LINE_SAMPLES);
        let uv = [
            first[0] + closure[0] * fraction,
            first[1] + closure[1] * fraction,
        ];
        match carrier.point(uv)? {
            Some(on) if distance(on, point) <= chord => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

/// Whether `displacement` runs along one surface parameter.
fn along_one_parameter(displacement: Uv) -> bool {
    (0..2).any(|axis| {
        displacement[axis] != 0.0
            && displacement[1 - axis].abs() <= POLE_LINE_SLOPE * displacement[axis].abs()
    })
}

fn closure_of(traced: &[Traced<'_>]) -> Uv {
    let first = traced.first().and_then(|first| first.uv.first()).copied();
    let last = traced.last().and_then(|last| last.uv.last()).copied();
    match (first, last) {
        (Some(first), Some(last)) => [last[0] - first[0], last[1] - first[1]],
        _ => [0.0, 0.0],
    }
}

/// A loop that passes through a pole of a periodic parameter and ends a
/// whole period from its start is a seam-cut loop: its parameter jumps at
/// the pole. The coedges after the pole move back by that period, so that
/// the loop closes and its two seam uses lie a period apart.
fn unwind_at_pole(
    carrier: &Carrier<'_, '_>,
    traced: &mut [Traced<'_>],
    closure: Uv,
) -> Result<(), Refusal> {
    for axis in 0..2 {
        let Some(period) = carrier.periods[axis] else {
            continue;
        };
        let turns = (closure[axis] / period).round();
        if turns == 0.0 {
            continue;
        }
        let mut pole = None;
        for (at, coedge) in traced.iter().enumerate().skip(1) {
            if let Some(junction) = coedge.uv.first() {
                if carrier.singular(*junction, axis)? {
                    pole = Some(at);
                    break;
                }
            }
        }
        let Some(pole) = pole else {
            continue;
        };
        for coedge in &mut traced[pole..] {
            for uv in &mut coedge.uv {
                uv[axis] -= turns * period;
            }
        }
    }
    Ok(())
}

/// A closed loop with no area that passes through a pole runs out and back
/// along a seam, as a sphere bounded by one meridian does. The coedges after
/// the pole move by the period whose sign makes the loop run with the face
/// on its left.
fn open_at_pole(
    carrier: &Carrier<'_, '_>,
    traced: &mut [Traced<'_>],
    left: bool,
) -> Result<(), Refusal> {
    let polygon: Vec<Uv> = traced
        .iter()
        .flat_map(|coedge| coedge.uv.iter().copied())
        .collect();
    let extent = polygon
        .iter()
        .flat_map(|uv| uv.iter().map(|value| value.abs()))
        .fold(1.0, f64::max);
    if crate::plane::signed_area(&polygon).abs() > ZERO_AREA_RATIO * extent * extent {
        return Ok(());
    }
    shift_at_pole(carrier, traced, left)
}

/// Moves the coedges after the first pole junction of a periodic parameter
/// by the period whose sign makes the loop run with the face on its left.
fn shift_at_pole(
    carrier: &Carrier<'_, '_>,
    traced: &mut [Traced<'_>],
    left: bool,
) -> Result<(), Refusal> {
    for axis in 0..2 {
        let Some(period) = carrier.periods[axis] else {
            continue;
        };
        for at in 1..traced.len() {
            let Some(junction) = traced[at].uv.first().copied() else {
                continue;
            };
            if !carrier.singular(junction, axis)? {
                continue;
            }
            for sign in [1.0, -1.0] {
                let shifted: Vec<Uv> = traced
                    .iter()
                    .enumerate()
                    .flat_map(|(position, coedge)| {
                        coedge.uv.iter().map(move |uv| {
                            let mut uv = *uv;
                            if position >= at {
                                uv[axis] += sign * period;
                            }
                            uv
                        })
                    })
                    .collect();
                let area = crate::plane::signed_area(&shifted);
                if (area > 0.0) == left && area != 0.0 {
                    for coedge in &mut traced[at..] {
                        for uv in &mut coedge.uv {
                            uv[axis] += sign * period;
                        }
                    }
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

/// The pcurve image of a coedge's samples, or `None` when it has no single
/// pcurve use that lies on its edge.
fn pcurve_trace(
    sampler: &Sampler<'_, '_>,
    carrier: &Carrier<'_, '_>,
    coedge: &Coedge,
    params: &[f64],
    points: &[Point3],
) -> Result<Option<Vec<Uv>>, Refusal> {
    let [use_] = coedge.pcurves.as_slice() else {
        return Ok(None);
    };
    let Some(pcurve) = sampler.index.pcurves(use_.pcurve.as_str(), StandardIndex) else {
        return Ok(None);
    };
    let (Some(first), Some(last)) = (params.first(), params.last()) else {
        return Ok(None);
    };
    let range = use_.parameter_range.map_or(
        [*first, *last],
        cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange::endpoints,
    );
    let span = last - first;
    let mut out = Vec::new();
    for (t, point) in params.iter().zip(points) {
        let fraction = if span == 0.0 { 0.0 } else { (t - first) / span };
        let s = range[0] + (range[1] - range[0]) * fraction;
        let uv = finite_or_refusal(
            sampler
                .admission
                .within_work_slice(sampler.work, |admission| {
                    pcurve_uv(admission, &pcurve.geometry, s)
                }),
        )
        .map_err(resource)?;
        let Some(uv) = uv else {
            return Ok(None);
        };
        let uv = [uv.get().u, uv.get().v];
        let Some(on) = carrier.point(uv)? else {
            return Ok(None);
        };
        if distance(on, *point) > PCURVE_MISMATCH * carrier.size.max(1.0) {
            return Ok(None);
        }
        out.push(uv);
    }
    Ok(Some(out))
}

/// Unwraps periodic parameters along the loop and places singular and
/// degenerate samples.
fn settle(carrier: &Carrier<'_, '_>, traced: &mut [Traced<'_>]) -> Result<(), Refusal> {
    // Singular axes per sample.
    let mut singular: Vec<Vec<[bool; 2]>> = Vec::new();
    for coedge in traced.iter() {
        let mut lane = Vec::new();
        for uv in &coedge.uv {
            lane.push([carrier.singular(*uv, 0)?, carrier.singular(*uv, 1)?]);
        }
        singular.push(lane);
    }
    // Continuity: each sample takes the branch nearest the last sample whose
    // parameter is defined. A pcurve trace moves as a whole.
    let mut reference: [Option<f64>; 2] = [None, None];
    for (coedge, flags) in traced.iter_mut().zip(&singular) {
        for axis in 0..2 {
            if coedge.from_pcurve {
                let anchor = coedge
                    .uv
                    .iter()
                    .zip(flags)
                    .find(|(_, flag)| !flag[axis])
                    .map(|(uv, _)| uv[axis]);
                if let (Some(anchor), Some(reference)) = (anchor, reference[axis]) {
                    let moved = carrier.nearest_branch(axis, anchor, reference);
                    let shift = moved - anchor;
                    for uv in &mut coedge.uv {
                        uv[axis] += shift;
                    }
                }
                if let Some((uv, _)) = coedge
                    .uv
                    .iter()
                    .zip(flags)
                    .rev()
                    .find(|(_, flag)| !flag[axis])
                {
                    reference[axis] = Some(uv[axis]);
                }
                continue;
            }
            for (uv, flag) in coedge.uv.iter_mut().zip(flags) {
                if flag[axis] || coedge.degenerate {
                    continue;
                }
                if let Some(reference) = reference[axis] {
                    uv[axis] = carrier.nearest_branch(axis, uv[axis], reference);
                }
                reference[axis] = Some(uv[axis]);
            }
        }
    }
    // A singular parameter takes the value of its nearest regular sample in
    // the same coedge.
    for (coedge, flags) in traced.iter_mut().zip(&singular) {
        for axis in 0..2 {
            let regular: Vec<usize> = (0..coedge.uv.len())
                .filter(|at| !flags[*at][axis])
                .collect();
            if regular.is_empty() {
                continue;
            }
            for (at, flag) in flags.iter().enumerate().take(coedge.uv.len()) {
                if flag[axis] {
                    let nearest = regular
                        .iter()
                        .min_by_key(|candidate| candidate.abs_diff(at))
                        .copied()
                        .unwrap_or(at);
                    coedge.uv[at][axis] = coedge.uv[nearest][axis];
                }
            }
        }
    }
    // A degenerate coedge runs between the ends of its neighbours.
    let count = traced.len();
    for at in 0..count {
        if !traced[at].degenerate {
            continue;
        }
        let before = traced[(at + count - 1) % count].uv.last().copied();
        let after = traced[(at + 1) % count].uv.first().copied();
        let coedge = &mut traced[at];
        if let (Some(before), Some(after)) = (before, after) {
            let start = coedge.uv.first().copied().unwrap_or(before);
            coedge.uv = vec![
                [before[0], carrier.nearest_branch(1, start[1], before[1])],
                [after[0], carrier.nearest_branch(1, start[1], after[1])],
            ];
            let point = coedge.points.first().copied();
            if let Some(point) = point {
                coedge.points = vec![point, point];
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
