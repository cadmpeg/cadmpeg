// SPDX-License-Identifier: Apache-2.0
//! Parameter-preserving sampled carriers for finite procedural domains.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use crate::eval::{
    admission::EvaluationAdmission, decode::pcurve_uv, finite_or_refusal, model_curve_point_by_id,
    model_surface_point_by_id, nurbs_curve_parameter_domain, EvaluationFailure,
};
use crate::geometry::{
    nurbs::{NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis},
    pcurve::PcurveGeometry,
    ProceduralCurveDefinition, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry,
};
use crate::ids::{CurveId, SurfaceId};
use crate::index::{ModelIndex, StandardIndex};
use crate::math::{Point2, Point3};
use crate::CadIr;
use cadmpeg_core::convert::f64_from_index;
use cadmpeg_core::decode::{ResourceLimit, WorkBudget};

/// Largest patch count along one parameter axis.
const MAX_PATCHES: usize = 1024;
/// Stored approximation poles one export may write.
const MAX_POLES: usize = 1 << 20;
/// Point evaluations one export may spend on sampling and residual probes.
const MAX_SAMPLES: usize = 1 << 23;
/// Model evaluation work one export may spend.
const MAX_EVALUATION_WORK: usize = 1 << 26;
/// Share of the model linear tolerance that one sampled carrier may use.
const TOLERANCE_FRACTION: f64 = 0.25;
/// Relative slack for a trim parameter at the end of a sampled domain.
const DOMAIN_SLACK_RELATIVE: f64 = 1.0e-9;
/// Probe fractions inside each patch. The end fractions lie on knot lines.
const PROBES: [f64; 5] = [0.0, 0.125, 0.5, 0.875, 1.0];
/// Initial patch count along a sampled parameter axis.
const SEED_PATCHES: usize = 4;

/// A point evaluation: `Some` point, or `None` when the carrier states no
/// point there or a budget ran out.
type Evaluate<'f> = dyn FnMut(f64, f64) -> Option<Point3> + 'f;

struct FitEvaluation {
    work: WorkBudget,
    refusal: Cell<Option<ResourceLimit>>,
}

/// Lazily sampled carriers and the export-wide sampling budget.
///
/// Helix curves and helix surfaces are not sampled here: the writer fits
/// them through `crate::eval::helix_nurbs`. An evaluation without a value or
/// an exhausted work slice leaves the carrier unwritten. A resource refusal
/// is retained for the caller.
pub struct Approximator<'a> {
    ir: &'a CadIr,
    index: Option<ModelIndex<'a>>,
    evaluation: FitEvaluation,
    samples: usize,
    poles: usize,
    curves: HashMap<String, Option<SolvedCurveGeometry>>,
    failed_surfaces: HashSet<String>,
}

impl<'a> Approximator<'a> {
    /// Create an approximation budget for this document.
    pub fn new(ir: &'a CadIr) -> Self {
        Self {
            ir,
            index: None,
            evaluation: FitEvaluation {
                work: WorkBudget::new(MAX_EVALUATION_WORK),
                refusal: Cell::new(None),
            },
            samples: MAX_SAMPLES,
            poles: MAX_POLES,
            curves: HashMap::new(),
            failed_surfaces: HashSet::new(),
        }
    }

    /// The sampled residual limit in model length units.
    pub fn residual_limit(&self) -> f64 {
        self.ir.tolerances.linear.get() * TOLERANCE_FRACTION
    }

    /// The first resource refusal encountered while fitting.
    pub fn resource_limit(&self) -> Option<ResourceLimit> {
        self.evaluation.refusal.get()
    }

    /// A sampled B-spline carrier for a procedural surface. A surface whose
    /// fit failed is not sampled again.
    pub fn surface(&mut self, surface: &str) -> Option<SolvedSurfaceGeometry> {
        if self.failed_surfaces.contains(surface) {
            return None;
        }
        let sampled = self.sample_surface(surface);
        if sampled.is_none() {
            self.failed_surfaces.insert(surface.to_owned());
        }
        sampled
    }

    fn sample_surface(&mut self, surface: &str) -> Option<SolvedSurfaceGeometry> {
        let ir = self.ir;
        let tolerance = self.residual_limit();
        let index = &*self
            .index
            .get_or_insert_with(|| ModelIndex::new_model_only(ir));
        let procedural = index.procedural_surface_for_surface(surface, StandardIndex)?;
        let owner = &index.surfaces(surface, StandardIndex)?.id;
        let (ranges, seeds) = match procedural.definition() {
            ProceduralSurfaceDefinition::Helix { .. } => return None,
            ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
                let a = curve_domain(ir, index, first)?;
                let b = curve_domain(ir, index, second)?;
                (
                    [[a[0].max(b[0]), a[1].min(b[1])], [0.0, 1.0]],
                    [SEED_PATCHES, 1],
                )
            }
            ProceduralSurfaceDefinition::Sum(construction) => (
                [
                    curve_domain(ir, index, construction.first())?,
                    curve_domain(ir, index, construction.second())?,
                ],
                [SEED_PATCHES; 2],
            ),
            _ => (face_domain(ir, index, owner)?, [SEED_PATCHES; 2]),
        };
        let evaluation = &self.evaluation;
        let mut evaluate = |u: f64, v: f64| {
            keep_refusal(
                &evaluation.refusal,
                EvaluationAdmission::Standard.within_work_slice(&evaluation.work, |admission| {
                    model_surface_point_by_id(admission, index, owner, u, v)
                }),
            )
            .map(crate::features::FinitePoint3::get)
        };
        if !trims_meet_vertices(
            ir,
            index,
            owner,
            ranges,
            tolerance,
            evaluation,
            &mut evaluate,
        ) {
            return None;
        }
        let grid = sample(
            ranges,
            seeds,
            false,
            tolerance,
            &mut self.samples,
            self.poles,
            &mut evaluate,
        )?;
        let count = grid.rows.len().checked_mul(grid.rows.first()?.len())?;
        let surface = NurbsSurface::new(
            NurbsSurfaceAxis::new(3, knots(ranges[0], grid.patches[0]), false),
            NurbsSurfaceAxis::new(3, knots(ranges[1], grid.patches[1]), false),
            NurbsPoleGrid::Polynomial { rows: grid.rows },
            false,
        )
        .ok()?;
        self.poles = self.poles.checked_sub(count)?;
        Some(SolvedSurfaceGeometry::Nurbs(surface))
    }

    /// A sampled B-spline carrier for a procedural curve, sampled once.
    pub fn curve(&mut self, curve: &str) -> Option<SolvedCurveGeometry> {
        if let Some(sampled) = self.curves.get(curve) {
            return sampled.clone();
        }
        let sampled = self.sample_curve(curve);
        self.curves.insert(curve.to_owned(), sampled.clone());
        sampled
    }

    fn sample_curve(&mut self, curve: &str) -> Option<SolvedCurveGeometry> {
        let ir = self.ir;
        let tolerance = self.residual_limit();
        let index = &*self
            .index
            .get_or_insert_with(|| ModelIndex::new_model_only(ir));
        let procedural = index
            .procedural_curves_for_curve(curve, StandardIndex)?
            .first()?;
        if matches!(procedural.definition(), ProceduralCurveDefinition::Helix(_)) {
            return None;
        }
        let owner = &index.curves(curve, StandardIndex)?.id;
        let range = curve_domain(ir, index, owner)?;
        let slack = DOMAIN_SLACK_RELATIVE * (range[1] - range[0]).abs().max(1.0);
        let edges_inside = ir
            .model
            .edges
            .iter()
            .filter(|edge| edge.curve() == Some(owner))
            .filter_map(|edge| edge.param_range().map(crate::units::FiniteVector::get))
            .all(|[start, end]| {
                start.min(end) >= range[0] - slack && start.max(end) <= range[1] + slack
            });
        if !edges_inside {
            return None;
        }
        let evaluation = &self.evaluation;
        let mut evaluate = |u: f64, _: f64| {
            keep_refusal(
                &evaluation.refusal,
                EvaluationAdmission::Standard.within_work_slice(&evaluation.work, |admission| {
                    model_curve_point_by_id(admission, index, owner, u)
                }),
            )
            .map(crate::features::FinitePoint3::get)
        };
        let grid = sample(
            [range, [0.0, 1.0]],
            [SEED_PATCHES, 1],
            true,
            tolerance,
            &mut self.samples,
            self.poles,
            &mut evaluate,
        )?;
        let points: Vec<_> = grid
            .rows
            .into_iter()
            .filter_map(|row| row.first().copied())
            .collect();
        let count = points.len();
        let curve = NurbsCurve::new(
            3,
            knots(range, grid.patches[0]),
            NurbsPoles3::Polynomial { points },
            false,
        )
        .ok()?;
        self.poles = self.poles.checked_sub(count)?;
        Some(SolvedCurveGeometry::Nurbs(curve))
    }
}

fn keep_refusal<T, R>(
    refusal: &Cell<Option<ResourceLimit>>,
    result: Result<T, EvaluationFailure<R>>,
) -> Option<T> {
    match finite_or_refusal(result) {
        Ok(value) => value,
        Err(limit) => {
            if refusal.get().is_none() {
                refusal.set(Some(limit));
            }
            None
        }
    }
}

/// Whether every trim endpoint of the faces on `surface` lies in the sampled
/// domain and maps onto one of its edge vertices. A pcurve the evaluator
/// cannot place at an endpoint, of any kind, fails the check.
fn trims_meet_vertices(
    ir: &CadIr,
    index: &ModelIndex<'_>,
    surface: &SurfaceId,
    ranges: [[f64; 2]; 2],
    residual: f64,
    evaluation: &FitEvaluation,
    evaluate: &mut Evaluate<'_>,
) -> bool {
    let slack = ranges.map(|range| DOMAIN_SLACK_RELATIVE * (range[1] - range[0]).abs().max(1.0));
    let inside = |uv: Point2| {
        uv.u >= ranges[0][0] - slack[0]
            && uv.u <= ranges[0][1] + slack[0]
            && uv.v >= ranges[1][0] - slack[1]
            && uv.v <= ranges[1][1] + slack[1]
    };
    let vertex = |id: &str| {
        let vertex = index.vertices(id, StandardIndex)?;
        let point = index.points(vertex.point.as_str(), StandardIndex)?;
        Some((
            point.position().get(),
            vertex
                .tolerance
                .map_or(0.0, crate::scalar::PositiveReal::get),
        ))
    };
    for face in ir
        .model
        .faces
        .iter()
        .filter(|face| &face.surface == surface)
    {
        for loop_id in &face.loops {
            let Some(loop_) = index.loops(loop_id.as_str(), StandardIndex) else {
                return false;
            };
            for coedge_id in loop_.coedges() {
                let Some(coedge) = index.coedges(coedge_id.as_str(), StandardIndex) else {
                    return false;
                };
                let Some(edge) = index.edges(coedge.edge.as_str(), StandardIndex) else {
                    return false;
                };
                let (Some((start, start_tolerance)), Some((end, end_tolerance))) =
                    (vertex(edge.start.as_str()), vertex(edge.end.as_str()))
                else {
                    return false;
                };
                let allowance = [
                    start_tolerance,
                    end_tolerance,
                    edge.tolerance.map_or(0.0, crate::scalar::PositiveReal::get),
                ]
                .into_iter()
                .fold(ir.tolerances.linear.get(), f64::max)
                    + residual;
                for use_ in &coedge.pcurves {
                    let Some(range) = use_.parameter_range else {
                        continue;
                    };
                    let Some(pcurve) = index.pcurves(use_.pcurve.as_str(), StandardIndex) else {
                        return false;
                    };
                    for t in range.endpoints() {
                        let uv = keep_refusal(
                            &evaluation.refusal,
                            EvaluationAdmission::Standard
                                .within_work_slice(&evaluation.work, |admission| {
                                    pcurve_uv(admission, &pcurve.geometry, t)
                                }),
                        );
                        let Some(uv) = uv else {
                            return false;
                        };
                        let uv = uv.get();
                        if !inside(uv) {
                            return false;
                        }
                        let Some(point) = evaluate(uv.u, uv.v) else {
                            return false;
                        };
                        if point.distance(start).min(point.distance(end)) > allowance {
                            return false;
                        }
                    }
                }
            }
        }
    }
    true
}

/// The bounding box of the trim pcurves of the faces on `surface`.
fn face_domain(ir: &CadIr, index: &ModelIndex<'_>, surface: &SurfaceId) -> Option<[[f64; 2]; 2]> {
    let mut bounds: Option<[[f64; 2]; 2]> = None;
    for face in ir
        .model
        .faces
        .iter()
        .filter(|face| &face.surface == surface)
    {
        for loop_id in &face.loops {
            let loop_ = index.loops(loop_id.as_str(), StandardIndex)?;
            for coedge_id in loop_.coedges() {
                let coedge = index.coedges(coedge_id.as_str(), StandardIndex)?;
                if coedge.pcurves.is_empty() {
                    return None;
                }
                for use_ in &coedge.pcurves {
                    let pcurve = index.pcurves(use_.pcurve.as_str(), StandardIndex)?;
                    let points = match &pcurve.geometry {
                        PcurveGeometry::Line(line) => {
                            let range = use_.parameter_range?.endpoints();
                            let origin = line.origin().get();
                            let direction = *line.direction().as_raw();
                            range
                                .map(|t| {
                                    Point2::new(
                                        origin.u + t * direction.u,
                                        origin.v + t * direction.v,
                                    )
                                })
                                .to_vec()
                        }
                        PcurveGeometry::Nurbs { nurbs } => {
                            if nurbs
                                .pole_rows()
                                .weights()
                                .is_some_and(|weights| weights.iter().any(|w| *w <= 0.0))
                            {
                                return None;
                            }
                            nurbs.pole_rows().raw_points()
                        }
                        _ => return None,
                    };
                    for point in points {
                        if !point.u.is_finite() || !point.v.is_finite() {
                            return None;
                        }
                        match &mut bounds {
                            Some([u, v]) => {
                                u[0] = u[0].min(point.u);
                                u[1] = u[1].max(point.u);
                                v[0] = v[0].min(point.v);
                                v[1] = v[1].max(point.v);
                            }
                            None => bounds = Some([[point.u, point.u], [point.v, point.v]]),
                        }
                    }
                }
            }
        }
    }
    bounds
}

/// The parameter domain of a rail curve: its stated range, its solved
/// B-spline domain, or the hull of its edge ranges.
fn curve_domain(ir: &CadIr, index: &ModelIndex<'_>, curve: &CurveId) -> Option<[f64; 2]> {
    let carrier = index.curves(curve.as_str(), StandardIndex)?;
    if let Some(range) = carrier.parameter_range {
        return Some(range.endpoints());
    }
    if let Some(SolvedCurveGeometry::Nurbs(nurbs)) = carrier.geometry.solved() {
        return nurbs_curve_parameter_domain(nurbs)
            .map(crate::topology::IncreasingParameterInterval::endpoints);
    }
    if let Some(ProceduralCurveDefinition::SurfaceCurve {
        family: crate::geometry::SurfaceCurveFamily::Parametric { context, .. },
    }) = index
        .procedural_curves_for_curve(curve.as_str(), StandardIndex)
        .and_then(|curves| curves.first())
        .map(|curve| curve.definition())
    {
        return Some(context.parameter_range().endpoints());
    }
    ir.model
        .edges
        .iter()
        .filter(|edge| edge.curve() == Some(curve))
        .filter_map(|edge| edge.param_range().map(crate::units::FiniteVector::get))
        .reduce(|a, b| [a[0].min(b[0]), a[1].max(b[1])])
}

struct Grid {
    rows: Vec<Vec<Point3>>,
    patches: [usize; 2],
}

/// A grid count as a real; counts stay far below 2^53.
fn real(count: usize) -> f64 {
    f64_from_index(count).unwrap_or(f64::NAN)
}

fn parameter(range: [f64; 2], fraction: f64) -> f64 {
    range[0] * (1.0 - fraction) + range[1] * fraction
}

/// Clamped knots of a piecewise cubic Bézier chain over `range`.
fn knots(range: [f64; 2], patches: usize) -> Vec<f64> {
    let mut knots = vec![range[0]];
    for i in 0..=patches {
        let knot = parameter(range, real(i) / real(patches));
        knots.extend([knot; 3]);
    }
    knots.push(range[1]);
    knots
}

fn blend(points: [Point3; 4], weights: [f64; 4]) -> Point3 {
    let lane = |coordinate: fn(Point3) -> f64| {
        points
            .into_iter()
            .zip(weights)
            .map(|(point, weight)| coordinate(point) * weight)
            .sum()
    };
    Point3::new(lane(|p| p.x), lane(|p| p.y), lane(|p| p.z))
}

/// Replace each run of samples at fractions 0, 1/3, 2/3 and 1 by the Bézier
/// poles of the cubic that interpolates them.
fn controls(points: &mut [Point3]) {
    let mut start = 0;
    while let Some(&[p0, p1, p2, p3]) = points.get(start..start + 4) {
        let p = [p0, p1, p2, p3];
        if let Some(slot) = points.get_mut(start + 1) {
            *slot = blend(p, [-5.0 / 6.0, 3.0, -1.5, 1.0 / 3.0]);
        }
        if let Some(slot) = points.get_mut(start + 2) {
            *slot = blend(p, [1.0 / 3.0, -1.5, 3.0, -5.0 / 6.0]);
        }
        start += 3;
    }
}

fn bezier(points: [Point3; 4], t: f64) -> Point3 {
    let s = 1.0 - t;
    blend(
        points,
        [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t],
    )
}

fn four(row: &[Point3], start: usize) -> Option<[Point3; 4]> {
    match *row.get(start..start + 4)? {
        [a, b, c, d] => Some([a, b, c, d]),
        _ => None,
    }
}

/// Point of the Bézier patch whose first pole is `(i, j)`.
fn patch(rows: &[Vec<Point3>], i: usize, j: usize, u: f64, v: f64, curve: bool) -> Option<Point3> {
    let mut columns = [Point3::new(0.0, 0.0, 0.0); 4];
    for (k, column) in columns.iter_mut().enumerate() {
        let row = rows.get(i + k)?;
        *column = if curve {
            *row.first()?
        } else {
            bezier(four(row, j)?, v)
        };
    }
    Some(bezier(columns, u))
}

/// Sample a cubic Bézier-chain carrier over `ranges`, and refine each axis
/// whose probes miss `tolerance`.
fn sample(
    ranges: [[f64; 2]; 2],
    seeds: [usize; 2],
    curve: bool,
    tolerance: f64,
    samples: &mut usize,
    poles_left: usize,
    evaluate: &mut Evaluate<'_>,
) -> Option<Grid> {
    if !tolerance.is_finite()
        || tolerance <= 0.0
        || ranges
            .iter()
            .any(|r| !r[0].is_finite() || !r[1].is_finite() || r[0] >= r[1])
    {
        return None;
    }
    let mut point = |u, v| {
        *samples = samples.checked_sub(1)?;
        evaluate(u, v)
    };
    let mut patches = if curve { [seeds[0], 1] } else { seeds };
    while patches[0] <= MAX_PATCHES && patches[1] <= MAX_PATCHES {
        let width = 3 * patches[0] + 1;
        let height = if curve { 1 } else { 3 * patches[1] + 1 };
        if width
            .checked_mul(height)
            .is_none_or(|count| count > poles_left)
        {
            return None;
        }
        let mut rows = Vec::with_capacity(width);
        for row_index in 0..width {
            let u = parameter(ranges[0], real(row_index) / real(3 * patches[0]));
            let mut row = Vec::with_capacity(height);
            for column in 0..height {
                let v = parameter(ranges[1], real(column) / real(3 * patches[1]));
                row.push(point(u, v)?);
            }
            if !curve {
                controls(&mut row);
            }
            rows.push(row);
        }
        for column_index in 0..height {
            let mut column: Vec<_> = rows
                .iter()
                .filter_map(|row| row.get(column_index).copied())
                .collect();
            controls(&mut column);
            for (row, pole) in rows.iter_mut().zip(column) {
                if let Some(slot) = row.get_mut(column_index) {
                    *slot = pole;
                }
            }
        }
        let v_probes: &[f64] = if curve { &PROBES[..1] } else { &PROBES };
        let mut refine = [false; 2];
        let mut interior = false;
        for u_patch in 0..patches[0] {
            for v_patch in 0..if curve { 1 } else { patches[1] } {
                for along_u in PROBES {
                    for &along_v in v_probes {
                        let u = (real(u_patch) + along_u) / real(patches[0]);
                        let v = (real(v_patch) + along_v) / real(patches[1]);
                        let exact = point(parameter(ranges[0], u), parameter(ranges[1], v))?;
                        let fit = patch(&rows, 3 * u_patch, 3 * v_patch, along_u, along_v, curve)?;
                        let residual = fit.distance(exact);
                        if residual.is_finite() && residual <= tolerance {
                            continue;
                        }
                        let on_v_knot = curve || along_v == 0.0 || along_v == 1.0;
                        let on_u_knot = along_u == 0.0 || along_u == 1.0;
                        refine[0] |= on_v_knot;
                        refine[1] |= on_u_knot;
                        interior |= !on_v_knot && !on_u_knot;
                    }
                }
            }
        }
        if !refine[0] && !refine[1] {
            if !interior {
                return Some(Grid { rows, patches });
            }
            refine = [true, !curve];
        }
        for (count, grow) in patches.iter_mut().zip(refine) {
            if grow {
                *count *= 2;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
