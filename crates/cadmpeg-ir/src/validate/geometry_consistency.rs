// SPDX-License-Identifier: Apache-2.0
//! Geometric consistency checks: evaluated carrier geometry must land on the
//! topology it supports.
#![allow(clippy::wildcard_imports)]

use super::*;
use crate::eval::{
    curve_parameter_near_point, curve_point, model_curve_point_by_id, model_surface_partials_by_id,
    model_surface_point_by_id, pcurve_tangent, pcurve_uv,
};
use crate::geometry::{PcurveGeometry, SurfaceGeometry};
use crate::math::{Point3, Vector3};
use crate::topology::Sense;

use crate::units::COINCIDENCE_TOLERANCE;

fn distance(a: Point3, b: Point3) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// The coincidence allowance combines the document-wide uncertainty with any
/// stored edge, vertex, face, or carrier tolerances.
fn allowance(document_tolerance: f64, tolerances: &[Option<f64>]) -> f64 {
    let document_tolerance = if document_tolerance.is_finite() && document_tolerance > 0.0 {
        document_tolerance
    } else {
        0.0
    };
    tolerances
        .iter()
        .flatten()
        .copied()
        .fold(COINCIDENCE_TOLERANCE.max(document_tolerance), f64::max)
}

/// Two independently evaluated procedural carriers can each consume the
/// baseline coincidence allowance. The solved cache's explicit fit tolerance
/// widens that allowance when it is larger.
fn procedural_support_allowance(document_tolerance: f64, cache_fit_tolerance: Option<f64>) -> f64 {
    COINCIDENCE_TOLERANCE + allowance(document_tolerance, &[cache_fit_tolerance])
}

/// Embedded support pcurves must map through their surfaces onto the curve
/// they constrain at both ends of the construction interval.
pub(super) fn check_procedural_support_consistency(ir: &CadIr, findings: &mut Vec<Finding>) {
    let index = crate::index::ModelIndex::new(ir);
    let curves = ir
        .model
        .curves
        .iter()
        .map(|curve| (curve.id.0.as_str(), &curve.geometry))
        .collect::<HashMap<_, _>>();
    for procedural in &ir.model.procedural_curves {
        if let crate::geometry::ProceduralCurveDefinition::TolerantIntersection {
            endpoints,
            tolerance,
            parameterization: Some(parameterization),
            ..
        } = procedural.definition()
        {
            let evaluated = parameterization
                .parameter_range
                .map(|parameter| model_curve_point_by_id(&index, &procedural.curve, parameter));
            let [Some(start), Some(end)] = evaluated else {
                findings.push(Finding {
                    check: Check::GeometricConsistency,
                    severity: Severity::Error,
                    message: "charted tolerant intersection does not evaluate at both endpoints"
                        .into(),
                    entity: Some(procedural.id.0.clone()),
                });
                continue;
            };
            let mismatch = distance(start, endpoints[0]).max(distance(end, endpoints[1]));
            if !mismatch.is_finite() || mismatch > *tolerance {
                findings.push(Finding {
                    check: Check::GeometricConsistency,
                    severity: Severity::Error,
                    message: format!(
                        "charted tolerant intersection misses its endpoint witnesses by \
                         {mismatch:.6}"
                    ),
                    entity: Some(procedural.id.0.clone()),
                });
            }
            continue;
        }
        if let crate::geometry::ProceduralCurveDefinition::SurfaceOffset {
            context,
            base,
            base_endpoints,
            distance: offset,
            ..
        } = procedural.definition()
        {
            let Some(solved) = curves.get(procedural.curve.0.as_str()) else {
                continue;
            };
            let solved = context
                .parameter_range
                .map(|parameter| curve_point(solved, parameter));
            let [Some(solved_start), Some(solved_end)] = solved else {
                continue;
            };
            let bound = procedural_support_allowance(
                ir.tolerances.linear,
                procedural.cache_fit_tolerance(),
            );
            let Some(base) = curves.get(base.0.as_str()) else {
                continue;
            };
            let base = base_endpoints
                .map(|parameter| parameter.and_then(|parameter| curve_point(base, parameter)));
            let [Some(base_start), Some(base_end)] = base else {
                check_support_sides(
                    context,
                    None,
                    SupportEndpointContract::Offset {
                        endpoints: [solved_start, solved_end],
                        distance: offset.abs(),
                    },
                    &index,
                    bound,
                    &procedural.id.0,
                    findings,
                );
                continue;
            };
            let offset_mismatch = (distance(solved_start, base_start) - offset.abs())
                .abs()
                .max((distance(solved_end, base_end) - offset.abs()).abs());
            if !offset_mismatch.is_finite() || offset_mismatch > bound {
                findings.push(Finding {
                    check: Check::GeometricConsistency,
                    severity: Severity::Error,
                    message: format!(
                        "surface-offset solved curve misses its base offset distance by \
                         {offset_mismatch:.6}"
                    ),
                    entity: Some(procedural.id.0.clone()),
                });
            }
            check_support_sides(
                context,
                None,
                SupportEndpointContract::Coincident([base_start, base_end]),
                &index,
                bound,
                &procedural.id.0,
                findings,
            );
            continue;
        }
        let (context, third) = match procedural.definition() {
            crate::geometry::ProceduralCurveDefinition::Law { context, .. }
            | crate::geometry::ProceduralCurveDefinition::Intersection { context, .. }
            | crate::geometry::ProceduralCurveDefinition::SurfaceCurve { context, .. }
            | crate::geometry::ProceduralCurveDefinition::Silhouette { context, .. }
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
        let bound =
            procedural_support_allowance(ir.tolerances.linear, procedural.cache_fit_tolerance());
        check_support_sides(
            context,
            third,
            SupportEndpointContract::Coincident([solved_start, solved_end]),
            &index,
            bound,
            &procedural.id.0,
            findings,
        );
    }
}

#[derive(Clone, Copy)]
enum SupportEndpointContract {
    Coincident([Point3; 2]),
    Offset {
        endpoints: [Point3; 2],
        distance: f64,
    },
}

fn check_support_sides(
    context: &crate::geometry::IntcurveSupportContext,
    third: Option<&crate::geometry::IntcurveSupportSide>,
    contract: SupportEndpointContract,
    index: &crate::index::ModelIndex<'_>,
    bound: f64,
    entity: &str,
    findings: &mut Vec<Finding>,
) {
    let (constrained, expected_distance) = match contract {
        SupportEndpointContract::Coincident(endpoints) => (endpoints, None),
        SupportEndpointContract::Offset {
            endpoints,
            distance,
        } => (endpoints, Some(distance)),
    };
    for (side_index, side) in context.sides.iter().chain(third).enumerate() {
        let (Some(surface_id), Some(pcurve)) = (&side.surface, &side.pcurve) else {
            continue;
        };
        let support = context.parameter_range.map(|parameter| {
            side.pcurve_parameter(context.parameter_range, parameter)
                .and_then(|parameter| pcurve_uv(pcurve, parameter))
                .and_then(|uv| model_surface_point_by_id(index, surface_id, uv.u, uv.v))
        });
        let [Some(support_start), Some(support_end)] = support else {
            continue;
        };
        let endpoint_mismatch = |constrained, support| {
            let distance = distance(constrained, support);
            expected_distance.map_or(distance, |expected| (distance - expected).abs())
        };
        let mismatch = endpoint_mismatch(constrained[0], support_start)
            .max(endpoint_mismatch(constrained[1], support_end));
        if !mismatch.is_finite() || mismatch > bound {
            findings.push(Finding {
                check: Check::GeometricConsistency,
                severity: Severity::Error,
                message: format!(
                    "procedural support side {side_index} misses its endpoint distance contract by \
                     {mismatch:.6}"
                ),
                entity: Some(entity.to_owned()),
            });
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
/// start and end vertex positions within the topology tolerances or the
/// evaluated curve cache's fit tolerance.
pub(super) fn check_edge_endpoint_consistency(ir: &CadIr, findings: &mut Vec<Finding>) {
    let curves = ir
        .model
        .curves
        .iter()
        .map(|curve| (curve.id.0.as_str(), &curve.geometry))
        .collect::<HashMap<_, _>>();
    let curve_cache_tolerances = ir
        .model
        .procedural_curves
        .iter()
        .map(|curve| {
            (
                curve.curve.0.as_str(),
                curve
                    .cache_fit_tolerance()
                    .filter(|value| value.is_finite()),
            )
        })
        .collect::<HashMap<_, _>>();
    let vertices = vertex_positions(ir);
    for edge in &ir.model.edges {
        let Some([start_t, end_t]) = edge.param_range else {
            continue;
        };
        let Some((curve_id, geometry)) = edge
            .curve
            .as_ref()
            .and_then(|id| curves.get(id.0.as_str()).map(|geometry| (id, geometry)))
        else {
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
        let bound = allowance(
            ir.tolerances.linear,
            &[
                edge.tolerance,
                *start_tol,
                *end_tol,
                curve_cache_tolerances
                    .get(curve_id.0.as_str())
                    .copied()
                    .flatten(),
            ],
        );
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
        let Some((curve_id, geometry)) = coedge
            .use_curve
            .as_ref()
            .and_then(|id| curves.get(id.0.as_str()).map(|geometry| (id, geometry)))
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
        let bound = allowance(
            ir.tolerances.linear,
            &[
                edge.tolerance,
                *start_tol,
                *end_tol,
                curve_cache_tolerances
                    .get(curve_id.0.as_str())
                    .copied()
                    .flatten(),
            ],
        );
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
/// owning edge's vertex positions over the edge's parameter interval within
/// the topology tolerances or the evaluated pcurve carriers' fit tolerances.
/// Pcurve parameter sign and direction are independent of edge sense, so
/// either sign and either endpoint assignment satisfy the check.
pub(super) fn check_pcurve_surface_consistency(ir: &CadIr, findings: &mut Vec<Finding>) {
    let index = crate::index::ModelIndex::new(ir);
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
    let procedurally_parameterized_surfaces = ir
        .model
        .procedural_surfaces
        .iter()
        .filter(|surface| {
            !matches!(
                surface.definition(),
                crate::geometry::ProceduralSurfaceDefinition::Subset { .. }
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
        let curve_geometry = edge
            .curve
            .as_ref()
            .and_then(|curve| curves.get(curve.0.as_str()).copied());
        let bound = allowance(
            ir.tolerances.linear,
            &[
                edge.tolerance,
                *start_tol,
                *end_tol,
                face.tolerance,
                first.fit_tolerance,
                last.fit_tolerance,
            ],
        );
        // Recovering an occurrence interval is a topological operation. A
        // carrier fit tolerance may qualify the final image, but must not let
        // inverse recovery move an explicitly ranged occurrence onto a
        // different, merely nearby part of the carrier.
        let recovery_bound = allowance(
            ir.tolerances.linear,
            &[edge.tolerance, *start_tol, *end_tol, face.tolerance],
        );
        // A malformed STEP export can retain a stale TRIMMED_CURVE interval
        // even though its carrier still reaches the edge vertices on another
        // interval. Keep the declared interval as a candidate, but also solve
        // the mapped carrier against the topology endpoints whenever the
        // carrier can provide such a witness.
        let surface_context = SurfacePcurveContext {
            index: &index,
            surface_id: &face.surface,
            geometry,
        };
        let recovered = edge_pcurve_parameter_ranges(
            &surface_context,
            curve_geometry,
            *start,
            *end,
            first,
            last,
            recovery_bound,
        )
        .unwrap_or_default();
        let declared = if coedge.pcurves.len() == 1 {
            pcurve_parameter_ranges(first, first_use.parameter_range, edge.param_range)
        } else {
            match (
                first_use
                    .parameter_range
                    .or(first.parameter_range)
                    .or_else(|| pcurve_parameter_extremes(first)),
                last_use
                    .parameter_range
                    .or(last.parameter_range)
                    .or_else(|| pcurve_parameter_extremes(last)),
            ) {
                (Some([t0, _]), Some([_, t1])) => Some(vec![[t0, t1]]),
                _ => None,
            }
        }
        .unwrap_or_default();
        let intervals = declared.into_iter().chain(recovered).collect::<Vec<_>>();
        let intervals = (!intervals.is_empty()).then_some(intervals);
        let Some(intervals) = intervals else {
            continue;
        };
        let Some(mismatch) = intervals
            .into_iter()
            .filter_map(|[t0, t1]| {
                let (uv0, uv1) = (
                    pcurve_uv(&first.geometry, t0)?,
                    pcurve_uv(&last.geometry, t1)?,
                );
                let (p0, p1) = (
                    model_surface_point_by_id(&index, &face.surface, uv0.u, uv0.v)?,
                    model_surface_point_by_id(&index, &face.surface, uv1.u, uv1.v)?,
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
/// range and the edge interval (in either sign) are candidates; the check takes
/// the closest image. An untrimmed carrier domain is not an edge interval: the
/// STEP edge may select any sub-interval of that carrier through its vertices.
/// Such an interval is recovered independently from the shared 3D curve by
/// `edge_pcurve_parameter_ranges`.
fn pcurve_parameter_ranges(
    pcurve: &crate::geometry::Pcurve,
    pcurve_range: Option<[f64; 2]>,
    edge_range: Option<[f64; 2]>,
) -> Option<Vec<[f64; 2]>> {
    let mut ranges = Vec::with_capacity(4);
    if let Some(range) = pcurve_range.or(pcurve.parameter_range) {
        ranges.push(range);
    }
    if let Some([start, end]) = edge_range {
        ranges.extend([[start, end], [-start, -end]]);
    }
    ranges.extend(pcurve_parameter_extremes(pcurve));
    if !ranges.is_empty() {
        if let Some(domain) = pcurve_parameter_domain(&pcurve.geometry) {
            ranges.push(domain);
        }
    }
    (!ranges.is_empty()).then_some(ranges)
}

struct SurfacePcurveContext<'index, 'model> {
    index: &'index crate::index::ModelIndex<'model>,
    surface_id: &'index crate::ids::SurfaceId,
    geometry: &'index SurfaceGeometry,
}

/// Recover an edge interval from its mapped pcurve when the STEP topology does
/// not carry a usable parameter range. A declared range remains a candidate,
/// but malformed exports can retain a stale range while the carrier still
/// reaches the edge vertices on another interval. The 3D and surface-space
/// carriers can use different native parameterizations, so solve the mapped
/// pcurve at each vertex instead of copying a parameter from one carrier to
/// the other. `tolerance` is the topology allowance used for this inverse;
/// carrier fit tolerances are applied only after recovery. A direct conic
/// solve remains as a fallback for surfaces without a usable mapped inverse.
/// Several seeds preserve the correct branch for periodic carriers.
fn edge_pcurve_parameter_ranges(
    context: &SurfacePcurveContext<'_, '_>,
    curve_geometry: Option<&crate::geometry::CurveGeometry>,
    start: Point3,
    end: Point3,
    first: &crate::geometry::Pcurve,
    last: &crate::geometry::Pcurve,
    tolerance: f64,
) -> Option<Vec<[f64; 2]>> {
    let start_parameters = pcurve_parameter_seeds_on_surface(context, first)
        .into_iter()
        .filter_map(|seed| {
            mapped_pcurve_parameter_near_point(context, &first.geometry, start, seed, tolerance)
        });
    let start_parameters = unique_finite(start_parameters);
    let end_parameters = pcurve_parameter_seeds_on_surface(context, last)
        .into_iter()
        .filter_map(|seed| {
            mapped_pcurve_parameter_near_point(context, &last.geometry, end, seed, tolerance)
        });
    let end_parameters = unique_finite(end_parameters);
    let ranges = start_parameters
        .iter()
        .copied()
        .flat_map(|start| end_parameters.iter().copied().map(move |end| [start, end]))
        .collect::<Vec<_>>();
    if !ranges.is_empty() {
        return Some(ranges);
    }

    let curve_geometry = curve_geometry?;
    if !matches!(
        curve_geometry,
        crate::geometry::CurveGeometry::Circle { .. }
            | crate::geometry::CurveGeometry::Ellipse { .. }
            | crate::geometry::CurveGeometry::Parabola { .. }
            | crate::geometry::CurveGeometry::Hyperbola { .. }
    ) {
        return None;
    }
    let seeds = pcurve_parameter_seeds_on_surface(context, first)
        .into_iter()
        .chain(pcurve_parameter_seeds_on_surface(context, last))
        .collect::<Vec<_>>();
    let start_parameters = seeds
        .iter()
        .filter_map(|&seed| curve_parameter_near_point(curve_geometry, start, seed, tolerance));
    let start_parameters = unique_finite(start_parameters);
    let end_parameters = seeds
        .iter()
        .filter_map(|&seed| curve_parameter_near_point(curve_geometry, end, seed, tolerance));
    let end_parameters = unique_finite(end_parameters);
    let ranges = start_parameters
        .into_iter()
        .flat_map(|start| end_parameters.iter().copied().map(move |end| [start, end]))
        .collect::<Vec<_>>();
    (!ranges.is_empty()).then_some(ranges)
}

/// Find a pcurve parameter whose mapped surface point is near a topology
/// vertex. Newton steps use the pcurve tangent pushed through the surface
/// partials; a short backtracking search keeps the iteration on the selected
/// branch of a periodic or rational carrier.
fn mapped_pcurve_parameter_near_point(
    context: &SurfacePcurveContext<'_, '_>,
    pcurve_geometry: &PcurveGeometry,
    target: Point3,
    seed: f64,
    tolerance: f64,
) -> Option<f64> {
    if !seed.is_finite() || !tolerance.is_finite() || tolerance < 0.0 {
        return None;
    }
    let domain = pcurve_parameter_domain(pcurve_geometry);
    let clamp_to_domain =
        |parameter: f64| domain.map_or(parameter, |[lower, upper]| parameter.clamp(lower, upper));
    let evaluate = |parameter: f64| {
        let uv = pcurve_uv(pcurve_geometry, parameter)?;
        let point = model_surface_point_by_id(context.index, context.surface_id, uv.u, uv.v)?;
        let tangent_uv = pcurve_tangent(pcurve_geometry, parameter)?;
        let partials = model_surface_partials_by_id(context.index, context.surface_id, uv.u, uv.v)?;
        let tangent = Vector3::new(
            partials.du.x * tangent_uv.u + partials.dv.x * tangent_uv.v,
            partials.du.y * tangent_uv.u + partials.dv.y * tangent_uv.v,
            partials.du.z * tangent_uv.u + partials.dv.z * tangent_uv.v,
        );
        Some((point, tangent))
    };
    let mismatch = |point: Point3| distance(point, target);
    let mut parameter = clamp_to_domain(seed);
    for _ in 0..32 {
        let (point, tangent) = evaluate(parameter)?;
        let error = mismatch(point);
        if error.is_finite() && error <= tolerance {
            return Some(parameter);
        }
        let denominator = tangent.dot(tangent);
        if !denominator.is_finite() || denominator <= f64::EPSILON {
            return None;
        }
        let residual = point.vector_from(target);
        let step = residual.dot(tangent) / denominator;
        if !step.is_finite() {
            return None;
        }
        let mut candidate = clamp_to_domain(parameter - step);
        let mut candidate_error = evaluate(candidate).map(|(point, _)| mismatch(point))?;
        for _ in 0..12 {
            if candidate_error <= error {
                break;
            }
            candidate = clamp_to_domain(0.5 * (candidate + parameter));
            candidate_error = evaluate(candidate).map(|(point, _)| mismatch(point))?;
        }
        if candidate == parameter || !candidate_error.is_finite() || candidate_error >= error {
            return None;
        }
        parameter = candidate;
    }
    None
}

fn unique_finite(values: impl IntoIterator<Item = f64>) -> Vec<f64> {
    let mut unique = Vec::new();
    for value in values {
        if value.is_finite() && !unique.contains(&value) {
            unique.push(value);
        }
    }
    unique
}

fn pcurve_parameter_seeds(pcurve: &crate::geometry::Pcurve) -> Vec<f64> {
    let mut seeds = vec![0.0];
    if let Some(range) = pcurve.parameter_range {
        seeds.extend(range);
    }
    if let Some([start, end]) = pcurve_parameter_domain(&pcurve.geometry) {
        seeds.extend([start, start + (end - start) * 0.5, end]);
    }
    unique_finite(seeds)
}

fn pcurve_parameter_seeds_on_surface(
    context: &SurfacePcurveContext<'_, '_>,
    pcurve: &crate::geometry::Pcurve,
) -> Vec<f64> {
    let mut seeds = pcurve_parameter_seeds(pcurve);
    let Some((origin, direction)) = pcurve.geometry.line_parameters() else {
        return seeds;
    };
    let Some([[u_lower, u_upper], [v_lower, v_upper]]) = surface_parameter_domains(context) else {
        return seeds;
    };
    for boundary in [u_lower, (u_lower + u_upper) * 0.5, u_upper] {
        if direction.u != 0.0 {
            seeds.push((boundary - origin.u) / direction.u);
        }
    }
    for boundary in [v_lower, (v_lower + v_upper) * 0.5, v_upper] {
        if direction.v != 0.0 {
            seeds.push((boundary - origin.v) / direction.v);
        }
    }
    unique_finite(seeds)
}

fn surface_parameter_domains(context: &SurfacePcurveContext<'_, '_>) -> Option<[[f64; 2]; 2]> {
    if let Some(crate::geometry::ProceduralSurfaceDefinition::Subset {
        parameter_ranges, ..
    }) = context
        .index
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.surface == *context.surface_id)
        .map(|procedural| procedural.definition())
    {
        let [[u_start, u_end], [v_start, v_end]] = *parameter_ranges;
        let u_span = (u_end - u_start).abs();
        let v_span = (v_end - v_start).abs();
        if u_span.is_finite() && u_span > 0.0 && v_span.is_finite() && v_span > 0.0 {
            return Some([[0.0, u_span], [0.0, v_span]]);
        }
    }
    match context.geometry {
        SurfaceGeometry::Nurbs(surface) => {
            let u_count = usize::try_from(surface.u_count).ok()?;
            let v_count = usize::try_from(surface.v_count).ok()?;
            Some([
                nurbs_parameter_domain(surface.u_degree, &surface.u_knots, u_count)?,
                nurbs_parameter_domain(surface.v_degree, &surface.v_knots, v_count)?,
            ])
        }
        SurfaceGeometry::Transformed { basis, .. } => {
            surface_parameter_domains(&SurfacePcurveContext {
                index: context.index,
                surface_id: context.surface_id,
                geometry: basis,
            })
        }
        SurfaceGeometry::Plane { .. }
        | SurfaceGeometry::Cylinder { .. }
        | SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Sphere { .. }
        | SurfaceGeometry::Torus { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

/// Explicit trim metadata, if the pcurve carrier itself supplies it. A raw
/// NURBS knot domain is deliberately excluded: it bounds the carrier, not the
/// edge occurrence.
fn pcurve_parameter_extremes(pcurve: &crate::geometry::Pcurve) -> Option<[f64; 2]> {
    pcurve
        .parameter_range
        .or_else(|| pcurve_geometry_trim_range(&pcurve.geometry))
}

fn pcurve_geometry_trim_range(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        PcurveGeometry::Trimmed {
            parameter_range, ..
        } => Some(*parameter_range),
        PcurveGeometry::Offset { basis, .. } => pcurve_geometry_trim_range(basis),
        PcurveGeometry::Transformed { basis, .. } => pcurve_geometry_trim_range(basis),
        PcurveGeometry::Line { .. }
        | PcurveGeometry::Circle { .. }
        | PcurveGeometry::Ellipse { .. }
        | PcurveGeometry::Harmonic { .. }
        | PcurveGeometry::Parabola { .. }
        | PcurveGeometry::Hyperbola { .. }
        | PcurveGeometry::Hyperbolic { .. }
        | PcurveGeometry::PolarHarmonic { .. }
        | PcurveGeometry::PolarNurbs { .. }
        | PcurveGeometry::Nurbs { .. }
        | PcurveGeometry::SphericalGreatCircle { .. } => None,
    }
}

fn pcurve_parameter_domain(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            ..
        } => nurbs_parameter_domain(*degree, knots, control_points.len()),
        PcurveGeometry::PolarNurbs {
            degree,
            knots,
            radial_control_points,
            ..
        } => nurbs_parameter_domain(*degree, knots, radial_control_points.len()),
        PcurveGeometry::Trimmed {
            parameter_range,
            basis,
            ..
        } => {
            if parameter_range[0] < parameter_range[1] {
                Some(*parameter_range)
            } else {
                pcurve_parameter_domain(basis)
            }
        }
        PcurveGeometry::Offset { basis, .. } => pcurve_parameter_domain(basis),
        PcurveGeometry::Transformed { basis, .. } => pcurve_parameter_domain(basis),
        PcurveGeometry::Line { .. }
        | PcurveGeometry::Circle { .. }
        | PcurveGeometry::Ellipse { .. }
        | PcurveGeometry::Harmonic { .. }
        | PcurveGeometry::Parabola { .. }
        | PcurveGeometry::Hyperbola { .. }
        | PcurveGeometry::Hyperbolic { .. }
        | PcurveGeometry::PolarHarmonic { .. }
        | PcurveGeometry::SphericalGreatCircle { .. } => None,
    }
}

fn nurbs_parameter_domain(
    degree: u32,
    knots: &[f64],
    control_point_count: usize,
) -> Option<[f64; 2]> {
    let degree = usize::try_from(degree).ok()?;
    if control_point_count <= degree
        || knots.len() < control_point_count.checked_add(degree)?.checked_add(1)?
    {
        return None;
    }
    let start = *knots.get(degree)?;
    let end = *knots.get(control_point_count)?;
    (start.is_finite() && end.is_finite() && start < end).then_some([start, end])
}

#[cfg(test)]
mod tests;
