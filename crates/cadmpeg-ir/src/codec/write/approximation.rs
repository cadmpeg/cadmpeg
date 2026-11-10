// SPDX-License-Identifier: Apache-2.0
//! Parameter-preserving sampled carriers for finite procedural domains.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use crate::eval::{
    admission::EvaluationAdmission, decode::pcurve_uv, exact_blend_cap_point,
    exact_blend_rail_point, exact_intersection_point, finite_or_refusal, model_curve_point_by_id,
    model_surface_point_by_id, nurbs_curve_parameter_domain, spine_rolling_ball_domain,
    EvaluationFailure,
};
use crate::geometry::{
    nurbs::{NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis},
    pcurve::PcurveGeometry,
    ProceduralCurveDefinition, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SurfaceGeometry,
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
const MAX_EVALUATION_WORK: usize = 1 << 31;
/// Model evaluation work one export may spend on exact curves, which can fall back to their charts.
const MAX_EXACT_CURVE_WORK: usize = 1 << 26;
/// Share of the model linear tolerance that one sampled carrier may use.
const TOLERANCE_FRACTION: f64 = 0.25;
/// Relative slack for a trim parameter at the end of a sampled domain.
const DOMAIN_SLACK_RELATIVE: f64 = 1.0e-9;
/// Probe fractions inside each patch. The end fractions lie on knot lines.
const PROBES: [f64; 5] = [0.0, 0.125, 0.5, 0.875, 1.0];
/// Initial patch count along a sampled parameter axis.
const SEED_PATCHES: usize = 4;
/// Residual limit for a blend with a free-form support, in model length units. A
/// piecewise support makes the blend only piecewise smooth, so the strict limit
/// would spend the export's whole evaluation budget without being reached.
const FREE_FORM_BLEND_RESIDUAL: f64 = 1.0e-3;
/// Residual limit for a blend on analytic supports, in model length units. The
/// patch count along each direction grows with the inverse fourth root of the
/// limit, and the strict limit makes a single blend face megabytes long.
const ANALYTIC_BLEND_RESIDUAL: f64 = 1.0e-4;
/// Intervals sampled along each edge of a blend face without trim pcurves.
const EDGE_DOMAIN_SAMPLES: usize = 16;

/// A point evaluation: `Some` point, or `None` when the carrier states no
/// point there or a budget ran out.
type Evaluate<'f> = dyn FnMut(f64, f64) -> Option<Point3> + 'f;

struct FitEvaluation {
    work: WorkBudget,
    // Separate from `work`, because an exact curve can fall back to its chart while a
    // surface that runs out of work drops its face from the shell.
    exact_curve_work: WorkBudget,
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
    free_form_blends: usize,
    analytic_blends: usize,
    free_form_curves: usize,
    exact_intersections: HashMap<String, Option<SolvedCurveGeometry>>,
}

impl<'a> Approximator<'a> {
    /// Create an approximation budget for this document.
    pub fn new(ir: &'a CadIr) -> Self {
        Self {
            ir,
            index: None,
            evaluation: FitEvaluation {
                work: WorkBudget::new(MAX_EVALUATION_WORK),
                exact_curve_work: WorkBudget::new(MAX_EXACT_CURVE_WORK),
                refusal: Cell::new(None),
            },
            samples: MAX_SAMPLES,
            poles: MAX_POLES,
            curves: HashMap::new(),
            failed_surfaces: HashSet::new(),
            free_form_blends: 0,
            analytic_blends: 0,
            free_form_curves: 0,
            exact_intersections: HashMap::new(),
        }
    }

    /// The sampled residual limit in model length units.
    pub fn residual_limit(&self) -> f64 {
        self.ir.tolerances.linear.get() * TOLERANCE_FRACTION
    }

    /// The number of sampled blends with a free-form support, and their residual
    /// limit in model length units, which differs from [`Self::residual_limit`].
    pub fn free_form_blend_fits(&self) -> (usize, f64) {
        (self.free_form_blends, FREE_FORM_BLEND_RESIDUAL)
    }

    /// The number of sampled blends on analytic supports, and their residual limit
    /// in model length units, which differs from [`Self::residual_limit`].
    pub fn analytic_blend_fits(&self) -> (usize, f64) {
        (self.analytic_blends, ANALYTIC_BLEND_RESIDUAL)
    }

    /// The number of fitted curves that lie on a free-form surface, and their residual
    /// limit in model length units, which differs from [`Self::residual_limit`].
    pub fn free_form_curve_fits(&self) -> (usize, f64) {
        (self.free_form_curves, FREE_FORM_BLEND_RESIDUAL)
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
        let strict_tolerance = self.residual_limit();
        let index = &*self
            .index
            .get_or_insert_with(|| ModelIndex::new_model_only(ir));
        let procedural = index.procedural_surface_for_surface(surface, StandardIndex)?;
        let owner = &index.surfaces(surface, StandardIndex)?.id;
        let free_form_blend = blend_has_free_form_support(index, procedural.definition());
        let analytic_blend = !free_form_blend && is_spine_blend(procedural.definition());
        let tolerance = if free_form_blend {
            FREE_FORM_BLEND_RESIDUAL
        } else if analytic_blend {
            ANALYTIC_BLEND_RESIDUAL
        } else {
            strict_tolerance
        };
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
            definition => {
                let evaluation = &self.evaluation;
                let by_spine = || spine_blend_face_domain(ir, index, owner, definition, evaluation);
                // Pcurves of closed rails can start past the seam of a closed spine, and a band
                // that stops short of its seam has no surface for the two rails to bound.
                let ranges = if has_closed_rail(ir, index, owner) {
                    by_spine().or_else(|| face_domain(ir, index, owner))
                } else {
                    face_domain(ir, index, owner).or_else(by_spine)
                };
                (ranges?, [SEED_PATCHES; 2])
            }
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
        self.free_form_blends += usize::from(free_form_blend);
        self.analytic_blends += usize::from(analytic_blend);
        Some(SolvedSurfaceGeometry::Nurbs(surface))
    }

    /// A B-spline carrier on the exact intersection of two solved surfaces, for
    /// an intersection curve whose stored carrier is a coarse degree-one chart. It
    /// keeps the chart's parameter interval, so edge ranges and pcurves stay valid.
    pub fn exact_intersection(&mut self, curve: &str) -> Option<SolvedCurveGeometry> {
        if let Some(fitted) = self.exact_intersections.get(curve) {
            return fitted.clone();
        }
        let fitted = self.fit_exact_intersection(curve);
        self.exact_intersections
            .insert(curve.to_owned(), fitted.clone());
        fitted
    }

    /// A degree-one intersection chart with its ends on the vertices of its edges,
    /// for a curve with no exact carrier.
    pub fn chart_on_edge_vertices(&mut self, curve: &str) -> Option<SolvedCurveGeometry> {
        let ir = self.ir;
        let index = &*self
            .index
            .get_or_insert_with(|| ModelIndex::new_model_only(ir));
        let procedural = index
            .procedural_curves_for_curve(curve, StandardIndex)?
            .first()?;
        let ProceduralCurveDefinition::Intersection { cache, .. } = procedural.definition() else {
            return None;
        };
        let owner = index.curves(curve, StandardIndex)?;
        let SolvedCurveGeometry::Nurbs(chart) = owner.geometry.solved()? else {
            return None;
        };
        (chart.degree() == 1).then_some(())?;
        chart_ending_on_edge_vertices(ir, index, &owner.id, chart, cache.as_ref())
            .map(SolvedCurveGeometry::Nurbs)
    }

    fn fit_exact_intersection(&mut self, curve: &str) -> Option<SolvedCurveGeometry> {
        let ir = self.ir;
        let strict_limit = self.residual_limit();
        let index = &*self
            .index
            .get_or_insert_with(|| ModelIndex::new_model_only(ir));
        let procedural = index
            .procedural_curves_for_curve(curve, StandardIndex)?
            .first()?;
        let ProceduralCurveDefinition::Intersection { context, cache, .. } =
            procedural.definition()
        else {
            return None;
        };
        let owner = &index.curves(curve, StandardIndex)?.id;
        let SolvedCurveGeometry::Nurbs(chart) =
            index.curves(curve, StandardIndex)?.geometry.solved()?
        else {
            return None;
        };
        if chart.degree() != 1 {
            return None;
        }
        let chart = &chart_ending_on_edge_vertices(ir, index, owner, chart, cache.as_ref())?;
        let carrier = exact_carrier(index, context)?;
        let free_form = carrier.has_free_form_surface();
        let tolerance = if free_form {
            FREE_FORM_BLEND_RESIDUAL.max(strict_limit)
        } else {
            strict_limit
        };
        let range = nurbs_curve_parameter_domain(chart)
            .map(crate::topology::IncreasingParameterInterval::endpoints)?;
        let evaluation = &self.evaluation;
        let mut evaluate = |u: f64, _: f64| {
            keep_refusal(
                &evaluation.refusal,
                EvaluationAdmission::Standard.within_work_slice(
                    &evaluation.exact_curve_work,
                    |admission| match carrier {
                        ExactCarrier::Intersection(supports) => {
                            exact_intersection_point(admission, supports, chart, u)
                        }
                        ExactCarrier::BlendRail {
                            supports,
                            side,
                            radius,
                        } => exact_blend_rail_point(admission, supports, side, radius, chart, u),
                        ExactCarrier::BlendCap {
                            supports,
                            radius,
                            other,
                        } => exact_blend_cap_point(admission, supports, radius, other, chart, u),
                    },
                ),
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
        let fitted = NurbsCurve::new(
            3,
            knots(range, grid.patches[0]),
            NurbsPoles3::Polynomial { points },
            false,
        )
        .ok()?;
        self.poles = self.poles.checked_sub(count)?;
        self.free_form_curves += usize::from(free_form);
        Some(SolvedCurveGeometry::Nurbs(fitted))
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
        if let Some(exact) = spline_through_plane(index, procedural.definition(), range, evaluation)
            .or_else(|| isocurve_of_spline_surface(index, procedural.definition(), evaluation))
            .or_else(|| isocurve_of_linear_sweep(index, procedural.definition()))
        {
            return Some(SolvedCurveGeometry::Nurbs(exact));
        }
        let mut evaluate = |u: f64, _: f64| {
            keep_refusal(
                &evaluation.refusal,
                EvaluationAdmission::Standard.within_work_slice(&evaluation.work, |admission| {
                    model_curve_point_by_id(admission, index, owner, u)
                }),
            )
            .map(crate::features::FinitePoint3::get)
        };
        // A uniform fit cannot converge across a corner that falls between its samples, so each
        // smooth stretch is fitted alone and the stretches share a pole at the corner.
        let mut breaks = vec![range[0]];
        breaks.extend(curve_corner_parameters(procedural.definition(), range));
        breaks.push(range[1]);
        let mut points: Vec<Point3> = Vec::new();
        let mut joined_knots: Vec<f64> = Vec::new();
        for stretch in breaks.windows(2) {
            let stretch = [*stretch.first()?, *stretch.get(1)?];
            let grid = sample(
                [stretch, [0.0, 1.0]],
                [SEED_PATCHES, 1],
                true,
                tolerance,
                &mut self.samples,
                self.poles.checked_sub(points.len())?,
                &mut evaluate,
            )?;
            let poles = grid.rows.into_iter().filter_map(|row| row.first().copied());
            let stretch_knots = knots(stretch, grid.patches[0]);
            if points.is_empty() {
                points.extend(poles);
                joined_knots.extend(stretch_knots);
            } else {
                points.extend(poles.skip(1));
                joined_knots.pop();
                joined_knots.extend(stretch_knots.into_iter().skip(4));
            }
        }
        let count = points.len();
        let curve =
            NurbsCurve::new(3, joined_knots, NurbsPoles3::Polynomial { points }, false).ok()?;
        self.poles = self.poles.checked_sub(count)?;
        Some(SolvedCurveGeometry::Nurbs(curve))
    }
}

/// The exact spline of a parameter-space spline on a plane. A plane is affine in its parameters,
/// so the curve keeps its knots, including the corners that a uniform sampled fit cannot resolve.
fn spline_through_plane(
    index: &ModelIndex<'_>,
    definition: &ProceduralCurveDefinition,
    range: [f64; 2],
    evaluation: &FitEvaluation,
) -> Option<NurbsCurve> {
    let ProceduralCurveDefinition::SurfaceCurve {
        family:
            crate::geometry::SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
    } = definition
    else {
        return None;
    };
    let [side, other] = context.sides();
    if other.surface.is_some() || other.pcurve.is_some() {
        return None;
    }
    let surface = side.surface.as_ref()?;
    let carrier = index.surfaces(surface.as_str(), StandardIndex)?;
    if !matches!(
        carrier.geometry.solved(),
        Some(SolvedSurfaceGeometry::Plane(_))
    ) {
        return None;
    }
    let PcurveGeometry::Nurbs { nurbs } = &side.pcurve.as_ref()?.geometry else {
        return None;
    };
    let knots = nurbs.knots().as_slice();
    if knots.first() != Some(&range[0]) || knots.last() != Some(&range[1]) {
        return None;
    }
    let on_plane = |u: f64, v: f64| {
        keep_refusal(
            &evaluation.refusal,
            EvaluationAdmission::Standard.within_work_slice(&evaluation.work, |admission| {
                model_surface_point_by_id(admission, index, surface, u, v)
            }),
        )
        .map(crate::features::FinitePoint3::get)
    };
    let origin = on_plane(0.0, 0.0)?;
    let along_u = on_plane(1.0, 0.0)?;
    let along_v = on_plane(0.0, 1.0)?;
    let points = nurbs
        .pole_rows()
        .raw_points()
        .into_iter()
        .map(|pole| {
            Point3::new(
                origin.x + (along_u.x - origin.x) * pole.u + (along_v.x - origin.x) * pole.v,
                origin.y + (along_u.y - origin.y) * pole.u + (along_v.y - origin.y) * pole.v,
                origin.z + (along_u.z - origin.z) * pole.u + (along_v.z - origin.z) * pole.v,
            )
        })
        .collect();
    NurbsCurve::from_lanes(
        nurbs.degree(),
        knots.to_vec(),
        points,
        nurbs.pole_rows().weights(),
        nurbs.periodic(),
    )
    .ok()
}

/// The parameters inside `range` where the parameter-space spline of a surface curve has only
/// position continuity: its interior knots repeated as often as its degree.
fn curve_corner_parameters(definition: &ProceduralCurveDefinition, range: [f64; 2]) -> Vec<f64> {
    let ProceduralCurveDefinition::SurfaceCurve {
        family: crate::geometry::SurfaceCurveFamily::Parametric { context, .. },
    } = definition
    else {
        return Vec::new();
    };
    let [side, _] = context.sides();
    let Some(PcurveGeometry::Nurbs { nurbs }) = side.pcurve.as_ref().map(|pcurve| &pcurve.geometry)
    else {
        return Vec::new();
    };
    let Ok(degree) = usize::try_from(nurbs.degree()) else {
        return Vec::new();
    };
    nurbs
        .knots()
        .as_slice()
        .chunk_by(|a, b| a == b)
        .filter(|run| run.len() >= degree)
        .filter_map(|run| run.first().copied())
        .filter(|knot| *knot > range[0] && *knot < range[1])
        .collect()
}

/// The exact isocurve of a straight parameter-space line that crosses a whole spline surface at a
/// constant parameter. Its knots can be far too dense for a uniform sampled fit to resolve.
fn isocurve_of_spline_surface(
    index: &ModelIndex<'_>,
    definition: &ProceduralCurveDefinition,
    evaluation: &FitEvaluation,
) -> Option<NurbsCurve> {
    let ProceduralCurveDefinition::SurfaceCurve {
        family:
            crate::geometry::SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
    } = definition
    else {
        return None;
    };
    let [side, other] = context.sides();
    if other.surface.is_some() || other.pcurve.is_some() {
        return None;
    }
    let carrier = index.surfaces(side.surface.as_ref()?.as_str(), StandardIndex)?;
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) = carrier.geometry.solved() else {
        return None;
    };
    let PcurveGeometry::Nurbs { nurbs } = &side.pcurve.as_ref()?.geometry else {
        return None;
    };
    let ([first, last], 1) = (
        <[Point2; 2]>::try_from(nurbs.pole_rows().raw_points()).ok()?,
        nurbs.degree(),
    ) else {
        return None;
    };
    let domain = |knots: &[f64]| Some((*knots.first()?, *knots.last()?));
    // The isocurve keeps the surface's own knots, so it has the parameters of this line only
    // when the line covers the same parameter interval. A line that runs backward takes the
    // reversed isocurve.
    let line_domain = domain(nurbs.knots().as_slice())?;
    let crossing = |domain: (f64, f64), a: f64, b: f64| {
        (line_domain == domain)
            .then(|| LineCrossing::between(domain, a, b))
            .flatten()
    };
    let u_domain = domain(surface.u_knots().as_slice())?;
    let v_domain = domain(surface.v_knots().as_slice())?;
    let (axis, at, crossing) =
        if let Some(crossing) = crossing(u_domain, first.u, last.u).filter(|_| first.v == last.v) {
            (
                crate::geometry::nurbs::SurfaceParameterAxis::V,
                first.v,
                crossing,
            )
        } else {
            (
                crate::geometry::nurbs::SurfaceParameterAxis::U,
                first.u,
                crossing(v_domain, first.v, last.v).filter(|_| first.u == last.u)?,
            )
        };
    match crate::eval::nurbs_surface_isocurve(EvaluationAdmission::Standard, surface, axis, at) {
        Ok(curve) => crossing.orient(curve?, line_domain),
        Err(limit) => {
            if evaluation.refusal.get().is_none() {
                evaluation.refusal.set(Some(limit));
            }
            None
        }
    }
}

/// The direction in which a straight parameter-space line crosses a whole parameter interval.
#[derive(Clone, Copy)]
enum LineCrossing {
    Forward,
    Backward,
}

impl LineCrossing {
    fn between(domain: (f64, f64), from: f64, to: f64) -> Option<Self> {
        if from == domain.0 && to == domain.1 {
            Some(Self::Forward)
        } else if from == domain.1 && to == domain.0 {
            Some(Self::Backward)
        } else {
            None
        }
    }

    /// `curve` in the direction of the line, over the same parameter interval.
    fn orient(self, mut curve: NurbsCurve, domain: (f64, f64)) -> Option<NurbsCurve> {
        if matches!(self, Self::Backward) {
            curve.reverse_parameterization_in_range(
                crate::scalar::FiniteReal::new(domain.0)?,
                crate::scalar::FiniteReal::new(domain.1)?,
            )?;
        }
        Some(curve)
    }
}

/// The directrix of a linear sweep, moved along the sweep, for a straight parameter-space line
/// that crosses the whole sweep at a constant sweep parameter. Its knots can be far too dense for
/// a uniform sampled fit to resolve.
fn isocurve_of_linear_sweep(
    index: &ModelIndex<'_>,
    definition: &ProceduralCurveDefinition,
) -> Option<NurbsCurve> {
    let ProceduralCurveDefinition::SurfaceCurve {
        family:
            crate::geometry::SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
    } = definition
    else {
        return None;
    };
    let [side, other] = context.sides();
    if other.surface.is_some() || other.pcurve.is_some() {
        return None;
    }
    let procedural =
        index.procedural_surface_for_surface(side.surface.as_ref()?.as_str(), StandardIndex)?;
    let ProceduralSurfaceDefinition::LinearSweep(sweep) = procedural.definition() else {
        return None;
    };
    let PcurveGeometry::Nurbs { nurbs } = &side.pcurve.as_ref()?.geometry else {
        return None;
    };
    let ([first, last], 1) = (
        <[Point2; 2]>::try_from(nurbs.pole_rows().raw_points()).ok()?,
        nurbs.degree(),
    ) else {
        return None;
    };
    let directrix = index.curves(sweep.directrix().as_str(), StandardIndex)?;
    let Some(SolvedCurveGeometry::Nurbs(directrix)) = directrix.geometry.solved() else {
        return None;
    };
    let knots = directrix.knots().as_slice();
    let line_knots = nurbs.knots().as_slice();
    // The moved directrix keeps its own knots, so it has the parameters of this line only when
    // the line covers the same parameter interval. A line that runs backward takes the reversed
    // directrix.
    let domain = (*knots.first()?, *knots.last()?);
    if first.v != last.v || (*line_knots.first()?, *line_knots.last()?) != domain {
        return None;
    }
    let crossing = LineCrossing::between(domain, first.u, last.u)?;
    let direction = sweep.direction().get();
    let points = directrix
        .pole_rows()
        .raw_points()
        .into_iter()
        .map(|pole| {
            Point3::new(
                pole.x + direction.x * first.v,
                pole.y + direction.y * first.v,
                pole.z + direction.z * first.v,
            )
        })
        .collect();
    NurbsCurve::from_lanes(
        directrix.degree(),
        knots.to_vec(),
        points,
        directrix.pole_rows().weights(),
        directrix.periodic(),
    )
    .ok()
    .and_then(|moved| crossing.orient(moved, domain))
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

/// How to evaluate an intersection curve exactly.
#[derive(Clone, Copy)]
enum ExactCarrier<'ir> {
    /// A transversal intersection of two solved surfaces.
    Intersection([&'ir SurfaceGeometry; 2]),
    /// The contact of a constant blend with its solved support `side`.
    BlendRail {
        supports: [&'ir SurfaceGeometry; 2],
        side: usize,
        radius: f64,
    },
    /// A constant blend's crossing with a solved surface that is not a support.
    BlendCap {
        supports: [&'ir SurfaceGeometry; 2],
        radius: f64,
        other: &'ir SurfaceGeometry,
    },
}

impl ExactCarrier<'_> {
    fn has_free_form_surface(&self) -> bool {
        let is_free_form = |surface: &&SurfaceGeometry| {
            matches!(surface.solved(), Some(SolvedSurfaceGeometry::Nurbs(_)))
        };
        match self {
            Self::Intersection(supports) | Self::BlendRail { supports, .. } => {
                supports.iter().any(&is_free_form)
            }
            Self::BlendCap {
                supports, other, ..
            } => is_free_form(other) || supports.iter().any(&is_free_form),
        }
    }
}

fn exact_carrier<'ir>(
    index: &ModelIndex<'ir>,
    context: &crate::geometry::IntcurveSupportContext,
) -> Option<ExactCarrier<'ir>> {
    let [first, second] = context.sides();
    let solved = |surface: &SurfaceId| {
        let geometry = &index.surfaces(surface.as_str(), StandardIndex)?.geometry;
        geometry.solved().map(|_| geometry)
    };
    let first = first.surface.as_ref()?;
    let second = second.surface.as_ref()?;
    if let (Some(first), Some(second)) = (solved(first), solved(second)) {
        return Some(ExactCarrier::Intersection([first, second]));
    }
    let (blend, other) = if solved(first).is_none() {
        (first, second)
    } else {
        (second, first)
    };
    let ProceduralSurfaceDefinition::Blend(payload) = index
        .procedural_surface_for_surface(blend.as_str(), StandardIndex)?
        .definition()
    else {
        return None;
    };
    let crate::geometry::BlendRadiusLaw::Constant { signed_radius } = payload.radius() else {
        return None;
    };
    let [support_a, support_b] = payload.supports();
    let support_a = &support_a.as_ref()?.surface;
    let support_b = &support_b.as_ref()?.surface;
    let supports = [solved(support_a)?, solved(support_b)?];
    let other_geometry = solved(other)?;
    let radius = signed_radius.get().abs();
    Some(
        match supports
            .iter()
            .position(|support| support.solved() == other_geometry.solved())
        {
            Some(side) => ExactCarrier::BlendRail {
                supports,
                side,
                radius,
            },
            None => ExactCarrier::BlendCap {
                supports,
                radius,
                other: other_geometry,
            },
        },
    )
}

/// Copy a degree-one chart with each end moved onto a vertex of an edge on the
/// curve when the vertex lies within the chart's fit tolerance. A chart can stop
/// short of a tangential end of its curve, and a fitted carrier must reach the
/// vertices of its edges.
fn chart_ending_on_edge_vertices(
    ir: &CadIr,
    index: &ModelIndex<'_>,
    curve: &CurveId,
    chart: &NurbsCurve,
    cache: Option<&crate::geometry::LegacyCache>,
) -> Option<NurbsCurve> {
    let Some(tolerance) = cache.map(|cache| cache.fit_tolerance.get()) else {
        return Some(chart.clone());
    };
    let vertices = ir
        .model
        .edges
        .iter()
        .filter(|edge| edge.curve() == Some(curve))
        .flat_map(|edge| [&edge.start, &edge.end])
        .filter_map(|vertex| {
            let vertex = index.vertices(vertex.as_str(), StandardIndex)?;
            Some(
                index
                    .points(vertex.point.as_str(), StandardIndex)?
                    .position()
                    .get(),
            )
        })
        .collect::<Vec<_>>();
    let last = chart.pole_rows().raw_points().len().checked_sub(1)?;
    let mut pole = 0;
    chart.map_control_points(|point| {
        let ordinal = pole;
        pole += 1;
        if ordinal != 0 && ordinal != last {
            return point;
        }
        vertices
            .iter()
            .copied()
            .filter(|vertex| vertex.distance(point) <= tolerance)
            .min_by(|a, b| a.distance(point).total_cmp(&b.distance(point)))
            .unwrap_or(point)
    })
}

/// Whether a surface is a blend without a stored section cache.
fn is_spine_blend(definition: &ProceduralSurfaceDefinition) -> bool {
    matches!(definition, ProceduralSurfaceDefinition::Blend(payload) if payload.native().is_none())
}

/// Whether a blend without a stored section cache rests on a NURBS surface.
fn blend_has_free_form_support(
    index: &ModelIndex<'_>,
    definition: &ProceduralSurfaceDefinition,
) -> bool {
    let ProceduralSurfaceDefinition::Blend(payload) = definition else {
        return false;
    };
    payload.native().is_none()
        && payload.supports().iter().flatten().any(|support| {
            index
                .surfaces(support.surface.as_str(), StandardIndex)
                .and_then(|surface| surface.geometry.solved())
                .is_some_and(|solved| matches!(solved, SolvedSurfaceGeometry::Nurbs(_)))
        })
}

fn has_closed_rail(ir: &CadIr, index: &ModelIndex<'_>, surface: &SurfaceId) -> bool {
    ir.model
        .faces
        .iter()
        .filter(|face| &face.surface == surface)
        .flat_map(|face| &face.loops)
        .filter_map(|loop_id| index.loops(loop_id.as_str(), StandardIndex))
        .flat_map(crate::topology::Loop::coedges)
        .filter_map(|coedge| index.coedges(coedge.as_str(), StandardIndex))
        .filter_map(|coedge| index.edges(coedge.edge.as_str(), StandardIndex))
        .any(|edge| edge.start == edge.end)
}

/// The domain of a blend from sample points along the edges of its faces.
fn spine_blend_face_domain(
    ir: &CadIr,
    index: &ModelIndex<'_>,
    surface: &SurfaceId,
    definition: &ProceduralSurfaceDefinition,
    evaluation: &FitEvaluation,
) -> Option<[[f64; 2]; 2]> {
    let ProceduralSurfaceDefinition::Blend(payload) = definition else {
        return None;
    };
    if payload.native().is_some() {
        return None;
    }
    let mut points = Vec::new();
    let mut whole_spine = false;
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
                let edge = index.edges(coedge.edge.as_str(), StandardIndex)?;
                whole_spine |= edge.start == edge.end;
                for vertex in [&edge.start, &edge.end] {
                    let vertex = index.vertices(vertex.as_str(), StandardIndex)?;
                    points.push(
                        index
                            .points(vertex.point.as_str(), StandardIndex)?
                            .position()
                            .get(),
                    );
                }
                let Some((curve, range)) = edge.curve().and_then(|curve| {
                    let range = edge
                        .param_range()
                        .map(crate::units::FiniteVector::get)
                        .or_else(|| curve_domain(ir, index, curve))?;
                    Some((curve, range))
                }) else {
                    continue;
                };
                for step in 0..=EDGE_DOMAIN_SAMPLES {
                    let fraction = real(step) / real(EDGE_DOMAIN_SAMPLES);
                    let point = keep_refusal(
                        &evaluation.refusal,
                        EvaluationAdmission::Standard.within_work_slice(
                            &evaluation.work,
                            |admission| {
                                model_curve_point_by_id(
                                    admission,
                                    index,
                                    curve,
                                    parameter(range, fraction),
                                )
                            },
                        ),
                    )?;
                    points.push(point.get());
                }
            }
        }
    }
    keep_refusal(
        &evaluation.refusal,
        EvaluationAdmission::Standard.within_work_slice(&evaluation.work, |admission| {
            spine_rolling_ball_domain(admission, index, payload, &points, whole_spine)
        }),
    )?
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
