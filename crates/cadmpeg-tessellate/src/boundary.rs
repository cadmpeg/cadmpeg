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
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::index::{ModelIndex, StandardIndex};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Coedge, Edge, Sense};

use crate::face::Refusal;
use crate::plane::{distance, dot, mid3, sub, Uv};
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
fn natural_domain(geometry: &SolvedCurveGeometry) -> Option<([f64; 2], bool)> {
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
        finite_or_refusal(
            EvaluationAdmission::Standard.within_work_slice(self.work, |admission| {
                model_curve_point_by_id(admission, self.index, curve, t)
            }),
        )
        .map(|point| point.map(cadmpeg_ir::features::FinitePoint3::get))
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
        let start = self.vertex_point(&edge.start)?;
        let end = self.vertex_point(&edge.end)?;
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
        if edge.start == edge.end {
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
        let start = self.vertex_point(&edge.start)?;
        let end = self.vertex_point(&edge.end)?;
        let samples = match edge.curve() {
            None => EdgeSamples {
                params: Vec::new(),
                points: vec![start, end],
                degenerate: true,
            },
            Some(curve) => self.sample(edge, curve, start, end)?,
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
                let turns = (span.abs() / (TAU / 8.0)).ceil();
                cadmpeg_core::convert::truncate_f64_to_usize(turns).ok_or_else(|| {
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
        let turn = 2.0 * crate::plane::angle(sub(on, a.1), sub(b.1, on));
        if depth < MAX_DEPTH && (sag > self.chord || turn > self.angle) {
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
            let mut lane = Vec::new();
            let mut seed = previous;
            for point in &points {
                let found = carrier.invert(*point, seed)?.ok_or_else(|| {
                    Refusal::new(format!(
                        "a sample of edge {} does not invert onto its face",
                        edge.id
                    ))
                })?;
                lane.push(found);
                seed = Some(found);
            }
            uv = Some(lane);
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
    settle(carrier, &mut traced)?;
    let mut closure = closure_of(&traced);
    unwind_at_pole(carrier, &mut traced, closure)?;
    closure = closure_of(&traced);
    if closure == [0.0, 0.0] {
        open_at_pole(carrier, &mut traced, left)?;
    }
    closure = closure_of(&traced);
    Ok(TracedLoop {
        coedges: traced,
        closure,
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
        cadmpeg_ir::geometry::DirectedParameterRange::endpoints,
    );
    let span = last - first;
    let mut out = Vec::new();
    for (t, point) in params.iter().zip(points) {
        let fraction = if span == 0.0 { 0.0 } else { (t - first) / span };
        let s = range[0] + (range[1] - range[0]) * fraction;
        let uv = finite_or_refusal(
            EvaluationAdmission::Standard.within_work_slice(sampler.work, |admission| {
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
