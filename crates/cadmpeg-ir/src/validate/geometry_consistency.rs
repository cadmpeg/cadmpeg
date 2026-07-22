// SPDX-License-Identifier: Apache-2.0
//! Geometric consistency checks: evaluated carrier geometry must land on the
//! topology it supports.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;
use crate::eval::{curve_point, pcurve_uv, surface_point};
use crate::geometry::PcurveGeometry;
use crate::math::Point3;
use crate::topology::Sense;

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
        let bound = allowance(&[edge.tolerance, *start_tol, *end_tol]);
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
    let edges = ir
        .model
        .edges
        .iter()
        .map(|edge| (edge.id.0.as_str(), edge))
        .collect::<HashMap<_, _>>();
    for coedge in &ir.model.coedges {
        let Some([start_t, end_t]) = coedge.use_curve_parameter_range else {
            continue;
        };
        let Some(geometry) = coedge
            .use_curve
            .as_ref()
            .and_then(|id| curves.get(id.0.as_str()))
        else {
            continue;
        };
        let Some(edge) = edges.get(coedge.edge.0.as_str()) else {
            continue;
        };
        let (first_vertex, last_vertex) = match coedge.sense {
            Sense::Forward => (&edge.start, &edge.end),
            Sense::Reversed => (&edge.end, &edge.start),
        };
        let (Some((start, start_tol)), Some((end, end_tol))) = (
            vertices.get(first_vertex.0.as_str()),
            vertices.get(last_vertex.0.as_str()),
        ) else {
            continue;
        };
        let (Some(at_start), Some(at_end)) =
            (curve_point(geometry, start_t), curve_point(geometry, end_t))
        else {
            continue;
        };
        let bound = allowance(&[edge.tolerance, *start_tol, *end_tol]);
        let mismatch = distance(at_start, *start).max(distance(at_end, *end));
        if !mismatch.is_finite() || mismatch > bound {
            findings.push(Finding {
                check: Check::GeometricConsistency,
                severity: Severity::Error,
                message: format!(
                    "coedge use-curve endpoints miss the traversal vertices by {mismatch:.6}"
                ),
                entity: Some(coedge.id.0.clone()),
            });
        }
    }
}

/// A coedge's pcurve, mapped through its face's surface, must land on the
/// owning edge's vertex positions over the edge's parameter interval. Pcurve
/// parameter sign and direction are independent of edge sense, so either sign
/// and either endpoint assignment satisfy the check.
pub(super) fn check_pcurve_surface_consistency(ir: &CadIr, findings: &mut Vec<Finding>) {
    let surfaces = ir
        .model
        .surfaces
        .iter()
        .map(|surface| (surface.id.0.as_str(), &surface.geometry))
        .collect::<HashMap<_, _>>();
    let procedurally_parameterized_surfaces = ir
        .model
        .procedural_surfaces
        .iter()
        .filter(|surface| {
            !matches!(
                surface.definition,
                crate::geometry::ProceduralSurfaceDefinition::Exact { .. }
            )
        })
        .map(|surface| surface.surface.0.as_str())
        .collect::<HashSet<_>>();
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
        let Some((first_use, last_use)) = coedge.pcurves.first().zip(coedge.pcurves.last()) else {
            continue;
        };
        let (Some(first), Some(last)) = (
            pcurves.get(first_use.pcurve.0.as_str()),
            pcurves.get(last_use.pcurve.0.as_str()),
        ) else {
            continue;
        };
        let Some(face) = loops
            .get(coedge.owner_loop.0.as_str())
            .and_then(|lp| faces.get(lp.face.0.as_str()))
        else {
            continue;
        };
        let Some(geometry) = surfaces.get(face.surface.0.as_str()) else {
            continue;
        };
        // A procedural construction defines its own UV space. Its solved
        // surface is a model-space cache, not the carrier of that UV
        // parameterization, so mapping the pcurve through the cache is not a
        // valid consistency test.
        if procedurally_parameterized_surfaces.contains(face.surface.0.as_str()) {
            continue;
        }
        let Some(edge) = edges.get(coedge.edge.0.as_str()) else {
            continue;
        };
        let (Some((start, start_tol)), Some((end, end_tol))) = (
            vertices.get(edge.start.0.as_str()),
            vertices.get(edge.end.0.as_str()),
        ) else {
            continue;
        };
        // A single parameter-space image is checked over its candidate
        // intervals, honoring an opposite-sign parameterization and a stored
        // range. Multiple images are checked from the first image's start
        // extreme to the last image's end extreme.
        let intervals = if coedge.pcurves.len() == 1 {
            pcurve_parameter_ranges(&first.geometry, first_use.parameter_range, edge.param_range)
        } else {
            match (
                first_use
                    .parameter_range
                    .or_else(|| pcurve_parameter_extremes(&first.geometry)),
                last_use
                    .parameter_range
                    .or_else(|| pcurve_parameter_extremes(&last.geometry)),
            ) {
                (Some([t0, _]), Some([_, t1])) => Some(vec![[t0, t1]]),
                _ => None,
            }
        };
        let Some(intervals) = intervals else {
            continue;
        };
        let bound = allowance(&[edge.tolerance, *start_tol, *end_tol, face.tolerance]);
        let Some(mismatch) = intervals
            .into_iter()
            .filter_map(|[t0, t1]| {
                let (uv0, uv1) = (
                    pcurve_uv(&first.geometry, t0)?,
                    pcurve_uv(&last.geometry, t1)?,
                );
                let (p0, p1) = (
                    surface_point(geometry, uv0.u, uv0.v)?,
                    surface_point(geometry, uv1.u, uv1.v)?,
                );
                let forward = distance(p0, *start).max(distance(p1, *end));
                let reversed = distance(p0, *end).max(distance(p1, *start));
                Some(forward.min(reversed))
            })
            .reduce(f64::min)
        else {
            continue;
        };
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

/// Candidate pcurve intervals for an edge. Native pcurves can parameterize the
/// same edge with the opposite sign, and a stored use interval can wrap a
/// periodic pcurve's seam, so no single interval is authoritative. The stored
/// range, the edge interval (in either sign), and the pcurve's intrinsic
/// parameter extremes are all candidates; the check takes the closest image.
/// An unbounded line without a stored range or edge interval is skipped.
fn pcurve_parameter_ranges(
    geometry: &PcurveGeometry,
    pcurve_range: Option<[f64; 2]>,
    edge_range: Option<[f64; 2]>,
) -> Option<Vec<[f64; 2]>> {
    let mut ranges = Vec::with_capacity(4);
    if let Some(range) = pcurve_range {
        ranges.push(range);
    }
    if let Some([start, end]) = edge_range {
        ranges.extend([[start, end], [-start, -end]]);
    }
    if let Some(extremes) = pcurve_parameter_extremes(geometry) {
        ranges.push(extremes);
    }
    (!ranges.is_empty()).then_some(ranges)
}

/// The parameter extremes over which a pcurve is checked: the NURBS knot
/// extremes, a trimmed range, or an offset basis's extremes. A line, conic,
/// parabola, or hyperbola pcurve without a stored range has no intrinsic extent
/// and is skipped.
fn pcurve_parameter_extremes(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        PcurveGeometry::Nurbs { knots, .. } => Some([*knots.first()?, *knots.last()?]),
        PcurveGeometry::Trimmed {
            parameter_range, ..
        } => Some(*parameter_range),
        PcurveGeometry::Offset { basis, .. } => pcurve_parameter_extremes(basis),
        PcurveGeometry::Line { .. }
        | PcurveGeometry::Circle { .. }
        | PcurveGeometry::Ellipse { .. }
        | PcurveGeometry::Parabola { .. }
        | PcurveGeometry::Hyperbola { .. } => None,
    }
}
