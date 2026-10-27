// SPDX-License-Identifier: Apache-2.0
//! Recover edge carriers from retained coedge geometry for semantic writers.

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{ResourceLimit, WorkBudget};

use super::approximation::Approximator;
use crate::document::CadIr;
use crate::eval::admission::EvaluationAdmission;
use crate::eval::{
    decode, finite_or_refusal, model_curve_point_by_id, model_surface_point_by_id,
    nurbs_pcurve_parameter_domain, EvaluationFailure,
};
use crate::geometry::analytic::CircleCurve;
use crate::geometry::nurbs::{NurbsCurve, NurbsPoles3};
use crate::geometry::pcurve::{Pcurve, PcurveGeometry};
use crate::geometry::{
    Curve, CurveGeometry, DirectedParameterRange, IntcurveSupportContext, IntcurveSupportSide,
    ProceduralCurve, ProceduralCurveDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry,
    SupportPcurve, SurfaceCurveFamily,
};
use crate::ids::{CurveId, EdgeId, ProceduralCurveId, SurfaceId};
use crate::index::{ModelIndex, StandardIndex};
use crate::math::{Point2, Point3, Vector3};
use crate::scalar::FiniteReal;
use crate::topology::{Coedge, Edge, EdgeCarrier, PcurveUse};

const MAX_RECOVERY_WORK: usize = 1 << 26;
const AGREEMENT_PARAMETERS: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

struct RecoveryEvaluation {
    work: WorkBudget,
    refusal: Cell<Option<ResourceLimit>>,
}

impl RecoveryEvaluation {
    fn evaluate<T, R>(
        &self,
        run: impl for<'slice> FnOnce(
            EvaluationAdmission<'slice, 'slice>,
        ) -> Result<T, EvaluationFailure<R>>,
    ) -> Option<T> {
        if self.refusal.get().is_some() {
            return None;
        }
        match finite_or_refusal(EvaluationAdmission::Standard.within_work_slice(&self.work, run)) {
            Ok(value) => value,
            Err(limit) => {
                self.refusal.set(Some(limit));
                None
            }
        }
    }
}

struct PcurveCandidate<'a> {
    geometry: &'a PcurveGeometry,
    surface: SurfaceId,
    range: [f64; 2],
}

/// A working export document and its sampled coedge comparisons.
pub struct EdgeRecovery<'a> {
    /// The original document or a recovered working copy.
    pub document: Cow<'a, CadIr>,
    /// Edges whose missing curves were recovered.
    pub recovered_edges: HashSet<EdgeId>,
    /// Edges whose different coedge curves agreed only at sampled parameters.
    pub sampled_agreements: Vec<EdgeId>,
    /// Edges recovered as sampled B-splines.
    pub sampled_fits: Vec<EdgeId>,
    /// Maximum sampled fit residual in the document's length unit.
    pub residual_limit: f64,
}

enum Recovery {
    Existing {
        curve: CurveId,
        range: [f64; 2],
    },
    Solved {
        geometry: SolvedCurveGeometry,
        range: [f64; 2],
    },
    Procedural {
        geometry: PcurveGeometry,
        surface: SurfaceId,
        range: [f64; 2],
    },
}

/// Borrow the input when no edge carrier can be recovered; otherwise return a
/// working document with carriers derived from its coedge geometry.
///
/// The source document and its retained bytes are not changed.
pub fn recover_edge_curves(ir: &CadIr) -> Result<EdgeRecovery<'_>, ResourceLimit> {
    let residual_limit = Approximator::new(ir).residual_limit();
    if !ir
        .model
        .edges
        .iter()
        .any(|edge| edge.curve().is_none() && edge.start != edge.end)
    {
        return Ok(EdgeRecovery {
            document: Cow::Borrowed(ir),
            recovered_edges: HashSet::new(),
            sampled_agreements: Vec::new(),
            sampled_fits: Vec::new(),
            residual_limit,
        });
    }
    let index = ModelIndex::build(ir);
    let work = RecoveryEvaluation {
        work: WorkBudget::new(MAX_RECOVERY_WORK),
        refusal: Cell::new(None),
    };
    let mut uses: HashMap<&str, Vec<&Coedge>> = HashMap::new();
    for coedge in &ir.model.coedges {
        uses.entry(coedge.edge.as_str()).or_default().push(coedge);
    }
    let mut recovered = Vec::new();
    for (ordinal, edge) in ir.model.edges.iter().enumerate() {
        if edge.curve().is_some() || edge.start == edge.end {
            continue;
        }
        let Some(coedges) = uses.get(edge.id.as_str()) else {
            continue;
        };
        let Some((start, end, tolerance)) = endpoints(edge, &index, ir) else {
            continue;
        };
        if start.distance(end) <= tolerance {
            continue;
        }
        if coedges.iter().all(|coedge| coedge.pcurves.is_empty()) {
            if let Some(recovery) = existing_curve(coedges, start, end, tolerance, &index, &work) {
                recovered.push((ordinal, false, recovery));
            }
            continue;
        }
        let Some((candidate, mut sampled)) =
            pcurve_candidate(coedges, start, end, tolerance, &index, &work)
        else {
            continue;
        };
        if coedges.iter().any(|coedge| coedge.use_curve.is_some()) {
            if !coedge_curves_agree(&candidate, coedges, start, end, tolerance, &index, &work) {
                continue;
            }
            sampled = true;
        }
        if let Some((geometry, range)) = exact_curve(&candidate, &index, &work) {
            recovered.push((ordinal, sampled, Recovery::Solved { geometry, range }));
        } else {
            recovered.push((
                ordinal,
                sampled,
                Recovery::Procedural {
                    geometry: candidate.geometry.clone(),
                    surface: candidate.surface,
                    range: candidate.range,
                },
            ));
        }
    }
    if let Some(limit) = work.refusal.get() {
        return Err(limit);
    }
    if recovered.is_empty() {
        return Ok(EdgeRecovery {
            document: Cow::Borrowed(ir),
            recovered_edges: HashSet::new(),
            sampled_agreements: Vec::new(),
            sampled_fits: Vec::new(),
            residual_limit,
        });
    }
    let mut document = ir.clone();
    let mut generated = HashSet::new();
    let mut counter = 0usize;
    let mut sampled_agreements = Vec::new();
    let mut fitted_candidates = Vec::new();
    for (ordinal, sampled, recovery) in recovered {
        let mut fit_construction = None;
        let (curve, range, geometry) = match recovery {
            Recovery::Existing { curve, range } => (curve, range, None),
            Recovery::Solved { geometry, range } => {
                let Some(id) = unique_id(&index, &mut generated, &mut counter, "curve") else {
                    continue;
                };
                let Ok(id) = CurveId::mint(id) else { continue };
                (id, range, Some(CurveGeometry::Solved(geometry)))
            }
            Recovery::Procedural {
                geometry,
                surface,
                range,
            } => {
                let Some(id) = unique_id(&index, &mut generated, &mut counter, "curve") else {
                    continue;
                };
                let Ok(curve_id) = CurveId::mint(id) else {
                    continue;
                };
                let Some(id) = unique_id(&index, &mut generated, &mut counter, "construction")
                else {
                    continue;
                };
                let Ok(construction_id) = ProceduralCurveId::mint(id) else {
                    continue;
                };
                let Ok(range) = DirectedParameterRange::new(range) else {
                    continue;
                };
                let Ok(context) = IntcurveSupportContext::try_new(
                    [
                        IntcurveSupportSide {
                            surface: Some(surface),
                            pcurve: Some(SupportPcurve::new(geometry, Some(range))),
                        },
                        IntcurveSupportSide {
                            surface: None,
                            pcurve: None,
                        },
                    ],
                    [0.0, 1.0],
                    [Vec::new(), Vec::new(), Vec::new()],
                ) else {
                    continue;
                };
                document.model.procedural_curves.push(ProceduralCurve::new(
                    construction_id.clone(),
                    ProceduralCurveDefinition::SurfaceCurve {
                        family: SurfaceCurveFamily::Parametric {
                            context,
                            tail: None,
                        },
                    },
                ));
                fit_construction = Some(construction_id.clone());
                (
                    curve_id,
                    [0.0, 1.0],
                    Some(CurveGeometry::Procedural {
                        construction: construction_id,
                        cache: None,
                    }),
                )
            }
        };
        if let Some(geometry) = geometry {
            let curve_index = document.model.curves.len();
            document.model.curves.push(Curve {
                id: curve.clone(),
                geometry,
                parameter_range: None,
                source_object: None,
            });
            if let Some(construction) = fit_construction {
                fitted_candidates.push((ordinal, curve_index, curve.clone(), construction));
            }
        }
        if let Ok(carrier) = EdgeCarrier::new(Some(curve), Some(range)) {
            document.model.edges[ordinal].carrier = carrier;
            if sampled {
                sampled_agreements.push(document.model.edges[ordinal].id.clone());
            }
        }
    }
    let (fits, refusal) = {
        let mut approximator = Approximator::new(&document);
        let fits = fitted_candidates
            .iter()
            .map(|(_, _, curve, _)| {
                if approximator.resource_limit().is_some() {
                    None
                } else {
                    approximator.curve(curve.as_str())
                }
            })
            .collect::<Vec<_>>();
        (fits, approximator.resource_limit())
    };
    if let Some(limit) = refusal {
        return Err(limit);
    }
    let mut sampled_fits = Vec::new();
    let mut failed_curves = HashSet::new();
    let mut failed_edges = HashSet::new();
    let mut constructions = HashSet::new();
    for ((ordinal, curve_index, curve_id, construction_id), fit) in
        fitted_candidates.into_iter().zip(fits)
    {
        constructions.insert(construction_id);
        if let (Some(geometry), Some(curve)) = (fit, document.model.curves.get_mut(curve_index)) {
            curve.geometry = CurveGeometry::Solved(geometry);
            sampled_fits.push(document.model.edges[ordinal].id.clone());
        } else {
            document.model.edges[ordinal].carrier = ir.model.edges[ordinal].carrier.clone();
            failed_curves.insert(curve_id);
            failed_edges.insert(document.model.edges[ordinal].id.clone());
        }
    }
    document
        .model
        .curves
        .retain(|curve| !failed_curves.contains(&curve.id));
    document
        .model
        .procedural_curves
        .retain(|curve| !constructions.contains(&curve.id));
    sampled_agreements.retain(|edge| !failed_edges.contains(edge));
    let recovered_edges = document
        .model
        .edges
        .iter()
        .zip(&ir.model.edges)
        .filter_map(|(edge, original)| {
            (original.curve().is_none() && edge.curve().is_some()).then_some(edge.id.clone())
        })
        .collect::<HashSet<_>>();
    for coedge in &mut document.model.coedges {
        if !recovered_edges.contains(&coedge.edge) {
            continue;
        }
        let Some(edge) = index.edges(coedge.edge.as_str(), StandardIndex) else {
            continue;
        };
        let Some((start, end, _)) = endpoints(edge, &index, ir) else {
            continue;
        };
        let Some(face) = index
            .loops(coedge.owner_loop.as_str(), StandardIndex)
            .and_then(|lp| index.faces(lp.face.as_str(), StandardIndex))
        else {
            continue;
        };
        for use_ in &mut coedge.pcurves {
            if use_.parameter_range.is_some() {
                continue;
            }
            let Some(pcurve) = index.pcurves(use_.pcurve.as_str(), StandardIndex) else {
                continue;
            };
            use_.parameter_range = pcurve_range(use_, pcurve, &face.surface, start, end, &index)
                .and_then(|range| DirectedParameterRange::new(range).ok());
        }
    }
    Ok(EdgeRecovery {
        document: Cow::Owned(document),
        recovered_edges,
        sampled_agreements,
        sampled_fits,
        residual_limit,
    })
}

fn unique_id(
    index: &ModelIndex<'_>,
    generated: &mut HashSet<String>,
    counter: &mut usize,
    kind: &str,
) -> Option<String> {
    loop {
        let id = format!("cadmpeg:export:{kind}#{counter}");
        *counter = counter.checked_add(1)?;
        if !index.contains(&id, StandardIndex) && generated.insert(id.clone()) {
            return Some(id);
        }
    }
}

fn endpoints(edge: &Edge, index: &ModelIndex<'_>, ir: &CadIr) -> Option<(Point3, Point3, f64)> {
    let start = index.vertices(edge.start.as_str(), StandardIndex)?;
    let end = index.vertices(edge.end.as_str(), StandardIndex)?;
    let first = index
        .points(start.point.as_str(), StandardIndex)?
        .position()
        .get();
    let last = index
        .points(end.point.as_str(), StandardIndex)?
        .position()
        .get();
    let mut tolerance = ir.tolerances.linear.get();
    for value in [edge.tolerance, start.tolerance, end.tolerance]
        .into_iter()
        .flatten()
    {
        tolerance = tolerance.max(value.get());
    }
    Some((first, last, tolerance))
}

fn existing_curve(
    coedges: &[&Coedge],
    start: Point3,
    end: Point3,
    tolerance: f64,
    index: &ModelIndex<'_>,
    work: &RecoveryEvaluation,
) -> Option<Recovery> {
    let mut uses = coedges
        .iter()
        .filter_map(|coedge| coedge.use_curve.as_ref());
    let first_use = uses.next()?;
    if uses.any(|use_| use_ != first_use) {
        return None;
    }
    let curve = index.curves(first_use.curve.as_str(), StandardIndex)?;
    let range = first_use.parameter_range.endpoints();
    let at = |t| {
        work.evaluate(|admission| model_curve_point_by_id(admission, index, &curve.id, t))
            .map(crate::features::FinitePoint3::get)
    };
    let (first, last) = (at(range[0])?, at(range[1])?);
    let forward = first.distance(start) <= tolerance && last.distance(end) <= tolerance;
    let backward = first.distance(end) <= tolerance && last.distance(start) <= tolerance;
    match (forward, backward) {
        (true, false) => Some(Recovery::Existing {
            curve: first_use.curve.clone(),
            range,
        }),
        (false, true) => {
            let mut geometry = curve.geometry.solved()?.clone();
            match &mut geometry {
                SolvedCurveGeometry::Line(line) => line.reverse_parameterization(),
                SolvedCurveGeometry::Circle(circle) => circle.reverse_parameterization(),
                SolvedCurveGeometry::Ellipse(ellipse) => ellipse.reverse_parameterization(),
                SolvedCurveGeometry::Nurbs(nurbs) => nurbs.reverse_parameterization(),
                _ => return None,
            }
            Some(Recovery::Solved {
                geometry,
                range: [-range[1], -range[0]],
            })
        }
        _ => None,
    }
}

fn candidate_point(
    candidate: &PcurveCandidate<'_>,
    fraction: f64,
    index: &ModelIndex<'_>,
    work: &RecoveryEvaluation,
) -> Option<Point3> {
    let t = candidate.range[0] * (1.0 - fraction) + candidate.range[1] * fraction;
    let uv = work
        .evaluate(|admission| decode::pcurve_uv(admission, candidate.geometry, t))?
        .get();
    work.evaluate(|admission| {
        model_surface_point_by_id(admission, index, &candidate.surface, uv.u, uv.v)
    })
    .map(crate::features::FinitePoint3::get)
}

fn coedge_curves_agree(
    candidate: &PcurveCandidate<'_>,
    coedges: &[&Coedge],
    start: Point3,
    end: Point3,
    tolerance: f64,
    index: &ModelIndex<'_>,
    work: &RecoveryEvaluation,
) -> bool {
    for use_ in coedges
        .iter()
        .filter_map(|coedge| coedge.use_curve.as_ref())
    {
        let Some(curve) = index.curves(use_.curve.as_str(), StandardIndex) else {
            return false;
        };
        let mut range = use_.parameter_range.endpoints();
        let at = |t| {
            work.evaluate(|admission| model_curve_point_by_id(admission, index, &curve.id, t))
                .map(crate::features::FinitePoint3::get)
        };
        let (Some(first), Some(last)) = (at(range[0]), at(range[1])) else {
            return false;
        };
        let forward = first.distance(start) <= tolerance && last.distance(end) <= tolerance;
        let backward = first.distance(end) <= tolerance && last.distance(start) <= tolerance;
        match (forward, backward) {
            (true, false) => {}
            (false, true) => range.swap(0, 1),
            _ => return false,
        }
        for fraction in AGREEMENT_PARAMETERS {
            let parameter = range[0] * (1.0 - fraction) + range[1] * fraction;
            let (Some(source), Some(lifted)) = (
                at(parameter),
                candidate_point(candidate, fraction, index, work),
            ) else {
                return false;
            };
            if source.distance(lifted) > tolerance {
                return false;
            }
        }
    }
    true
}

fn pcurve_candidate<'a>(
    coedges: &[&'a Coedge],
    start: Point3,
    end: Point3,
    tolerance: f64,
    index: &'a ModelIndex<'a>,
    work: &RecoveryEvaluation,
) -> Option<(PcurveCandidate<'a>, bool)> {
    let mut chosen: Option<PcurveCandidate<'_>> = None;
    let mut sampled = false;
    for coedge in coedges {
        if coedge.pcurves.is_empty() {
            continue;
        }
        let [use_] = coedge.pcurves.as_slice() else {
            return None;
        };
        let pcurve = index.pcurves(use_.pcurve.as_str(), StandardIndex)?;
        let lp = index.loops(coedge.owner_loop.as_str(), StandardIndex)?;
        let face = index.faces(lp.face.as_str(), StandardIndex)?;
        let range = pcurve_range(use_, pcurve, &face.surface, start, end, index)?;
        if range[0] == range[1] {
            return None;
        }
        let mut candidate = PcurveCandidate {
            geometry: &pcurve.geometry,
            surface: face.surface.clone(),
            range,
        };
        let first = candidate_point(&candidate, 0.0, index, work)?;
        let last = candidate_point(&candidate, 1.0, index, work)?;
        let forward = first.distance(start) <= tolerance && last.distance(end) <= tolerance;
        let backward = first.distance(end) <= tolerance && last.distance(start) <= tolerance;
        match (forward, backward) {
            (true, false) => {}
            (false, true) => candidate.range.swap(0, 1),
            _ => return None,
        }
        if let Some(previous) = &chosen {
            if previous.surface == candidate.surface
                && previous.geometry == candidate.geometry
                && previous.range == candidate.range
            {
                continue;
            }
            for fraction in AGREEMENT_PARAMETERS {
                if candidate_point(previous, fraction, index, work)?
                    .distance(candidate_point(&candidate, fraction, index, work)?)
                    > tolerance
                {
                    return None;
                }
            }
            sampled = true;
        } else {
            chosen = Some(candidate);
        }
    }
    chosen.map(|candidate| (candidate, sampled))
}

fn pcurve_range(
    use_: &PcurveUse,
    pcurve: &Pcurve,
    surface: &SurfaceId,
    start: Point3,
    end: Point3,
    index: &ModelIndex<'_>,
) -> Option<[f64; 2]> {
    use_.parameter_range
        .map(DirectedParameterRange::endpoints)
        .or_else(|| {
            pcurve
                .parameter_range()
                .map(crate::units::FiniteVector::get)
        })
        .or_else(|| match &pcurve.geometry {
            PcurveGeometry::Nurbs { nurbs } => nurbs_pcurve_parameter_domain(
                nurbs.degree(),
                nurbs.knots(),
                nurbs.pole_rows().count(),
            )
            .map(crate::topology::IncreasingParameterInterval::endpoints),
            _ => None,
        })
        .or_else(|| planar_line_range(&pcurve.geometry, surface, start, end, index))
}

fn planar_line_range(
    geometry: &PcurveGeometry,
    surface: &SurfaceId,
    start: Point3,
    end: Point3,
    index: &ModelIndex<'_>,
) -> Option<[f64; 2]> {
    let PcurveGeometry::Line(line) = geometry else {
        return None;
    };
    let surface = index.surfaces(surface.as_str(), StandardIndex)?;
    let Some(SolvedSurfaceGeometry::Plane(plane)) = surface.geometry.solved() else {
        return None;
    };
    let origin = plane.origin().get();
    let u = *plane.frame().reference().as_raw();
    let v = plane.frame().axis().as_raw().cross(u);
    let line_origin = line.origin().as_raw();
    let direction = line.direction().as_raw();
    let parameter = |point: Point3| {
        let offset = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
        if direction.u.abs() >= direction.v.abs() {
            (offset.dot(u) - line_origin.u) / direction.u
        } else {
            (offset.dot(v) - line_origin.v) / direction.v
        }
    };
    let range = [parameter(start), parameter(end)];
    (range[0].is_finite() && range[1].is_finite() && range[0] != range[1]).then_some(range)
}

fn exact_curve(
    candidate: &PcurveCandidate<'_>,
    index: &ModelIndex<'_>,
    work: &RecoveryEvaluation,
) -> Option<(SolvedCurveGeometry, [f64; 2])> {
    let surface = index.surfaces(candidate.surface.as_str(), StandardIndex)?;
    let crate::geometry::SurfaceGeometry::Solved(solved) = &surface.geometry else {
        return None;
    };
    if let (SolvedSurfaceGeometry::Cylinder(cylinder), PcurveGeometry::Line(line)) =
        (solved, candidate.geometry)
    {
        let direction = line.direction().as_raw();
        if direction.u == 0.0 {
            let points = vec![
                candidate_point(candidate, 0.0, index, work)?,
                candidate_point(candidate, 1.0, index, work)?,
            ];
            let curve = NurbsCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                NurbsPoles3::Polynomial { points },
                false,
            )
            .ok()?;
            return Some((SolvedCurveGeometry::Nurbs(curve), [0.0, 1.0]));
        }
        if direction.v == 0.0 {
            let origin = line.origin().as_raw();
            let start_u = origin.u + direction.u * candidate.range[0];
            let end_u = origin.u + direction.u * candidate.range[1];
            let span = (end_u - start_u).abs();
            if !span.is_finite() || span == 0.0 || span >= std::f64::consts::TAU {
                return None;
            }
            let axis = *cylinder.frame().axis().as_raw();
            let base = cylinder.origin().get();
            let center = Point3::new(
                base.x + origin.v * axis.x,
                base.y + origin.v * axis.y,
                base.z + origin.v * axis.z,
            );
            let start = candidate_point(candidate, 0.0, index, work)?;
            let radius = cylinder.radius().get();
            let reference = Vector3::new(
                (start.x - center.x) / radius,
                (start.y - center.y) / radius,
                (start.z - center.z) / radius,
            );
            let normal = if end_u > start_u {
                axis
            } else {
                Vector3::new(-axis.x, -axis.y, -axis.z)
            };
            let circle = CircleCurve::try_new(center, normal, reference, radius).ok()?;
            return Some((SolvedCurveGeometry::Circle(circle), [0.0, span]));
        }
    }
    let SolvedSurfaceGeometry::Plane(plane) = solved else {
        return None;
    };
    let range = candidate.range;
    let ascending = range[0] < range[1];
    let low = range[0].min(range[1]);
    let high = range[0].max(range[1]);
    let point = |p: Point2| {
        let origin = plane.origin().get();
        let x = plane.frame().reference().as_raw();
        let y = plane.frame().axis().as_raw().cross(*x);
        Point3::new(
            origin.x + p.u * x.x + p.v * y.x,
            origin.y + p.u * x.y + p.v * y.y,
            origin.z + p.u * x.z + p.v * y.z,
        )
    };
    match candidate.geometry {
        PcurveGeometry::Line(_) => {
            let points = vec![
                candidate_point(candidate, 0.0, index, work)?,
                candidate_point(candidate, 1.0, index, work)?,
            ];
            let curve = NurbsCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                NurbsPoles3::Polynomial { points },
                false,
            )
            .ok()?;
            Some((SolvedCurveGeometry::Nurbs(curve), [0.0, 1.0]))
        }
        PcurveGeometry::Nurbs { nurbs } => {
            let domain = nurbs_pcurve_parameter_domain(
                nurbs.degree(),
                nurbs.knots(),
                nurbs.pole_rows().count(),
            )?
            .endpoints();
            if low < domain[0] || high > domain[1] {
                return None;
            }
            let mut curve = nurbs.lift(point).ok()?;
            if !ascending {
                curve.reverse_parameterization_in_range(
                    FiniteReal::new(low)?,
                    FiniteReal::new(high)?,
                )?;
            }
            Some((SolvedCurveGeometry::Nurbs(curve), [low, high]))
        }
        PcurveGeometry::Circle(circle) => {
            let center = point(circle.center().get());
            let origin = plane.origin().get();
            let axis_point = point(circle.x_axis().get());
            let second_axis_point = point(circle.y_axis().get());
            let x = Vector3::new(
                axis_point.x - origin.x,
                axis_point.y - origin.y,
                axis_point.z - origin.z,
            );
            let y = Vector3::new(
                second_axis_point.x - origin.x,
                second_axis_point.y - origin.y,
                second_axis_point.z - origin.z,
            );
            if x.dot(x) != 1.0 || y.dot(y) != 1.0 || x.dot(y) != 0.0 {
                return None;
            }
            let normal = x.cross(y);
            let normal = if ascending {
                normal
            } else {
                Vector3::new(-normal.x, -normal.y, -normal.z)
            };
            let circle = CircleCurve::try_new(center, normal, x, circle.radius().get()).ok()?;
            let range = if ascending {
                [low, high]
            } else {
                [-high, -low]
            };
            Some((SolvedCurveGeometry::Circle(circle), range))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
