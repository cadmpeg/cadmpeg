// SPDX-License-Identifier: Apache-2.0
//! Geometric consistency checks: evaluated carrier geometry must land on the
//! topology it supports.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;
use crate::eval::{curve_point, pcurve_uv, surface_point};
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

/// Embedded support pcurves must map through their surfaces onto the solved
/// procedural curve at both ends of the construction interval.
pub(super) fn check_procedural_support_consistency(ir: &CadIr, findings: &mut Vec<Finding>) {
    let curves = ir
        .model
        .curves
        .iter()
        .map(|curve| (curve.id.0.as_str(), &curve.geometry))
        .collect::<HashMap<_, _>>();
    let surfaces = ir
        .model
        .surfaces
        .iter()
        .map(|surface| (surface.id.0.as_str(), &surface.geometry))
        .collect::<HashMap<_, _>>();
    for procedural in &ir.model.procedural_curves {
        let (context, third) = match &procedural.definition {
            crate::geometry::ProceduralCurveDefinition::Law { context, .. }
            | crate::geometry::ProceduralCurveDefinition::Intersection { context, .. }
            | crate::geometry::ProceduralCurveDefinition::SurfaceCurve { context, .. }
            | crate::geometry::ProceduralCurveDefinition::Silhouette { context, .. }
            | crate::geometry::ProceduralCurveDefinition::SurfaceOffset { context, .. }
            | crate::geometry::ProceduralCurveDefinition::Spring { context, .. }
            | crate::geometry::ProceduralCurveDefinition::Projection { context, .. }
            | crate::geometry::ProceduralCurveDefinition::TwoSidedOffset { context, .. } => {
                (context, None)
            }
            crate::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection {
                context,
                third,
                ..
            } => (context, Some(third)),
            _ => continue,
        };
        let Some(curve) = curves.get(procedural.curve.0.as_str()) else {
            continue;
        };
        let solved = context
            .parameter_range
            .map(|parameter| curve_point(curve, parameter));
        let [Some(solved_start), Some(solved_end)] = solved else {
            continue;
        };
        let bound = allowance(&[procedural.cache_fit_tolerance]);
        for (side_index, side) in context.sides.iter().chain(third).enumerate() {
            let (Some(surface_id), Some(pcurve)) = (&side.surface, &side.pcurve) else {
                continue;
            };
            let Some(surface) = surfaces.get(surface_id.0.as_str()) else {
                continue;
            };
            let support = context.parameter_range.map(|parameter| {
                side.pcurve_parameter(context.parameter_range, parameter)
                    .and_then(|parameter| pcurve_uv(pcurve, parameter))
                    .and_then(|uv| surface_point(surface, uv.u, uv.v))
            });
            let [Some(support_start), Some(support_end)] = support else {
                continue;
            };
            let mismatch =
                distance(solved_start, support_start).max(distance(solved_end, support_end));
            if !mismatch.is_finite() || mismatch > bound {
                findings.push(Finding {
                    check: Check::GeometricConsistency,
                    severity: Severity::Error,
                    message: format!(
                        "procedural support side {side_index} misses the solved curve endpoints by \
                         {mismatch:.6}"
                    ),
                    entity: Some(procedural.id.0.clone()),
                });
            }
        }
    }
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
}

/// A coedge's pcurve, mapped through its face's surface, must land on the
/// owning edge's vertex positions at the pcurve's parameter extremes. The
/// pcurve's parameter direction is independent of the edge sense, so either
/// endpoint assignment satisfies the check.
pub(super) fn check_pcurve_surface_consistency(ir: &CadIr, findings: &mut Vec<Finding>) {
    let surfaces = ir
        .model
        .surfaces
        .iter()
        .map(|surface| (surface.id.0.as_str(), &surface.geometry))
        .collect::<HashMap<_, _>>();
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
        let Some(geometry) = surfaces.get(face.surface.0.as_str()) else {
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
            surface_point(geometry, uv0.u, uv0.v),
            surface_point(geometry, uv1.u, uv1.v),
        ) else {
            continue;
        };
        let bound = allowance(&[edge.tolerance, *start_tol, *end_tol, face.tolerance]);
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

#[cfg(test)]
mod tests {
    use super::check_procedural_support_consistency;
    use crate::document::CadIr;
    use crate::geometry::{
        Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, PcurveGeometry,
        ProceduralCurve, ProceduralCurveDefinition, Surface, SurfaceCurveFamily, SurfaceGeometry,
    };
    use crate::ids::{CurveId, ProceduralCurveId, SurfaceId};
    use crate::math::{Point2, Point3, Vector3};
    use crate::units::Units;

    fn mapped_surface_curve(mapping: [f64; 2]) -> CadIr {
        let mut ir = CadIr::empty(Units::default());
        let curve = CurveId("curve".to_string());
        let surface = SurfaceId("surface".to_string());
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Line {
                origin: Point3::new(2.0, 0.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: ProceduralCurveId("surface-curve".to_string()),
            curve,
            definition: ProceduralCurveDefinition::SurfaceCurve {
                family: SurfaceCurveFamily::Parametric,
                context: IntcurveSupportContext {
                    sides: [
                        IntcurveSupportSide {
                            surface: Some(surface),
                            pcurve: Some(PcurveGeometry::Line {
                                origin: Point2::new(0.0, 0.0),
                                direction: Point2::new(1.0, 0.0),
                            }),
                            pcurve_parameter_range: Some(mapping),
                        },
                        IntcurveSupportSide {
                            surface: None,
                            pcurve: None,
                            pcurve_parameter_range: None,
                        },
                    ],
                    parameter_range: [0.0, 1.0],
                    discontinuities: std::array::from_fn(|_| Vec::new()),
                },
            },
            cache_fit_tolerance: None,
        });
        ir
    }

    #[test]
    fn procedural_support_endpoints_honor_the_per_side_parameter_mapping() {
        let mut findings = Vec::new();
        check_procedural_support_consistency(&mapped_surface_curve([2.0, 3.0]), &mut findings);
        assert!(findings.is_empty());

        check_procedural_support_consistency(&mapped_surface_curve([3.0, 2.0]), &mut findings);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("support side 0"));
    }
}
