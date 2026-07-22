// SPDX-License-Identifier: Apache-2.0
//! Geometric consistency checks: evaluated carrier geometry must land on the
//! topology it supports.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;
use crate::eval::{curve_point, pcurve_uv, ModelSurfaceEvaluator};
use crate::geometry::PcurveGeometry;
use crate::math::Point3;

/// Maximum distance, in the document's length unit, between an evaluated
/// carrier point and the vertex position it must coincide with. Exact carriers
/// agree to rational-weight rounding (well under `1e-3` mm); real mismatches
/// observed from decoder defects start orders of magnitude above this bound.
const COINCIDENCE_TOLERANCE: f64 = 0.01;

fn distance(a: Point3, b: Point3) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// The per-entity coincidence allowance: the shared bound widened by any
/// stored edge/vertex tolerances.
fn allowance(tolerances: &[Option<f64>]) -> f64 {
    tolerances
        .iter()
        .flatten()
        .copied()
        .fold(COINCIDENCE_TOLERANCE, f64::max)
}

fn vertex_positions(ir: &CadIr) -> HashMap<&str, (Point3, Option<f64>)> {
    let points = ir
        .model
        .points
        .iter()
        .map(|point| (point.id.0.as_str(), point.position))
        .collect::<HashMap<_, _>>();
    ir.model
        .vertices
        .iter()
        .filter_map(|vertex| {
            let position = points.get(vertex.point.0.as_str())?;
            Some((vertex.id.0.as_str(), (*position, vertex.tolerance)))
        })
        .collect()
}

/// An edge's curve evaluated at its parameter range must land on the edge's
/// start and end vertex positions.
pub(super) fn check_edge_endpoint_consistency(ir: &CadIr, findings: &mut Vec<Finding>) {
    let curves = ir
        .model
        .curves
        .iter()
        .map(|curve| (curve.id.0.as_str(), &curve.geometry))
        .collect::<HashMap<_, _>>();
    let cache_tolerances = ir
        .model
        .procedural_curves
        .iter()
        .filter_map(|curve| {
            curve
                .cache_fit_tolerance
                .map(|tolerance| (curve.curve.0.as_str(), tolerance))
        })
        .collect::<HashMap<_, _>>();
    let vertices = vertex_positions(ir);
    for edge in &ir.model.edges {
        let Some([start_t, end_t]) = edge.param_range else {
            continue;
        };
        let Some(geometry) = edge.curve.as_ref().and_then(|id| curves.get(id.0.as_str())) else {
            continue;
        };
        let (Some((start, start_tol)), Some((end, end_tol))) = (
            vertices.get(edge.start.0.as_str()),
            vertices.get(edge.end.0.as_str()),
        ) else {
            continue;
        };
        let (Some(at_start), Some(at_end)) =
            (curve_point(geometry, start_t), curve_point(geometry, end_t))
        else {
            continue;
        };
        let cache_tolerance = edge
            .curve
            .as_ref()
            .and_then(|curve| cache_tolerances.get(curve.0.as_str()))
            .copied();
        let bound = allowance(&[edge.tolerance, *start_tol, *end_tol, cache_tolerance]);
        let mismatch = distance(at_start, *start).max(distance(at_end, *end));
        if !mismatch.is_finite() || mismatch > bound {
            findings.push(Finding {
                check: Check::GeometricConsistency,
                severity: Severity::Error,
                message: format!(
                    "edge curve endpoints miss the edge's vertex positions by {mismatch:.6}"
                ),
                entity: Some(edge.id.0.clone()),
            });
        }
    }
}

/// A coedge's pcurve, mapped through its face's surface, must land on the
/// owning edge's vertex positions at the pcurve's parameter extremes. The
/// pcurve's parameter direction is independent of the edge sense, so either
/// endpoint assignment satisfies the check.
pub(super) fn check_pcurve_surface_consistency(ir: &CadIr, findings: &mut Vec<Finding>) {
    let surface_evaluator = ModelSurfaceEvaluator::new(ir);
    let pcurves = ir
        .model
        .pcurves
        .iter()
        .map(|pcurve| (pcurve.id.0.as_str(), pcurve))
        .collect::<HashMap<_, _>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|edge| (edge.id.0.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let faces = ir
        .model
        .faces
        .iter()
        .map(|face| (face.id.0.as_str(), face))
        .collect::<HashMap<_, _>>();
    let loops = ir
        .model
        .loops
        .iter()
        .map(|lp| (lp.id.0.as_str(), lp))
        .collect::<HashMap<_, _>>();
    let vertices = vertex_positions(ir);

    for coedge in &ir.model.coedges {
        let Some((first, last)) = coedge.pcurves.first().zip(coedge.pcurves.last()) else {
            continue;
        };
        let (Some(first), Some(last)) = (
            pcurves.get(first.pcurve.0.as_str()),
            pcurves.get(last.pcurve.0.as_str()),
        ) else {
            continue;
        };
        let Some(face) = loops
            .get(coedge.owner_loop.0.as_str())
            .and_then(|lp| faces.get(lp.face.0.as_str()))
        else {
            continue;
        };
        let Some(edge) = edges.get(coedge.edge.0.as_str()) else {
            continue;
        };
        let (Some((start, start_tol)), Some((end, end_tol))) = (
            vertices.get(edge.start.0.as_str()),
            vertices.get(edge.end.0.as_str()),
        ) else {
            continue;
        };
        let (Some([t0, _]), Some([_, t1])) = (
            pcurve_parameter_extremes(first),
            pcurve_parameter_extremes(last),
        ) else {
            continue;
        };
        let (Some(uv0), Some(uv1)) = (
            pcurve_uv(&first.geometry, t0),
            pcurve_uv(&last.geometry, t1),
        ) else {
            continue;
        };
        let (Some(p0), Some(p1)) = (
            surface_evaluator.point(&face.surface, uv0.u, uv0.v),
            surface_evaluator.point(&face.surface, uv1.u, uv1.v),
        ) else {
            continue;
        };
        let bound = allowance(&[
            edge.tolerance,
            *start_tol,
            *end_tol,
            face.tolerance,
            first
                .fit_tolerance
                .into_iter()
                .chain(last.fit_tolerance)
                .reduce(f64::max),
        ]);
        let forward = distance(p0, *start).max(distance(p1, *end));
        let reversed = distance(p0, *end).max(distance(p1, *start));
        let mismatch = forward.min(reversed);
        if !mismatch.is_finite() || mismatch > bound {
            findings.push(Finding {
                check: Check::GeometricConsistency,
                severity: Severity::Error,
                message: format!(
                    "pcurve mapped through the face surface misses the edge's vertex positions \
                     by {mismatch:.6}"
                ),
                entity: Some(coedge.id.0.clone()),
            });
        }
    }
}

/// The parameter extremes over which a pcurve is checked. Ordinary NURBS
/// carriers use their knot domain because some native range fields are
/// independent metadata. Polar NURBS carriers use their explicit trim range,
/// falling back to the knot domain. Other analytic carriers have no intrinsic
/// finite extent here.
fn pcurve_parameter_extremes(pcurve: &crate::geometry::Pcurve) -> Option<[f64; 2]> {
    match &pcurve.geometry {
        PcurveGeometry::PolarNurbs { knots, .. } => pcurve
            .parameter_range
            .or_else(|| Some([*knots.first()?, *knots.last()?])),
        geometry => pcurve_geometry_parameter_extremes(geometry),
    }
}

fn pcurve_geometry_parameter_extremes(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        PcurveGeometry::Nurbs { knots, .. } | PcurveGeometry::PolarNurbs { knots, .. } => {
            Some([*knots.first()?, *knots.last()?])
        }
        PcurveGeometry::Trimmed {
            parameter_range, ..
        } => Some(*parameter_range),
        PcurveGeometry::Offset { basis, .. } => pcurve_geometry_parameter_extremes(basis),
        PcurveGeometry::Line { .. }
        | PcurveGeometry::Circle { .. }
        | PcurveGeometry::Ellipse { .. }
        | PcurveGeometry::Parabola { .. }
        | PcurveGeometry::Hyperbola { .. }
        | PcurveGeometry::PolarHarmonic { .. } => None,
    }
}
