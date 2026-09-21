// SPDX-License-Identifier: Apache-2.0
//! Line/conic intersections and solved topological vertices.

use crate::vecmath::normalize;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::{Point3, Vector3};

use crate::container::ContainerScan;
use crate::decode::quadratic::{real_roots, Coefficient};

use super::super::surfaces::intersection_resolve::curve_contains_points;

use super::super::uniqueness::exactly_one;
use super::edges::{nonperiodic_nurbs_endpoint_points, planar_conic_equation, PlanarConicEquation};
use super::equations::{
    common_plane_conic_parameters, plane_intersection_line, CarrierEquation, PlaneConicEquation,
    PlaneEquation,
};
use super::pcurves::{
    directed_pcurve_points, pcurve_edge_endpoint_evidence_with_carriers,
    solve_pcurve_vertex_domains_with_authoritative_points, PcurveEndpointDiagnostics,
};
use super::planes::{solve_carriers_with_diagnostics, CarrierSolveDiagnostics};
use crate::vecmath::{cross, dot};

const EPS_AGREE: f64 = 1.0e-9;
const EPS_NEAR_ZERO: f64 = 1.0e-12;
const CARRIER_VERTEX_SAMPLE_LIMIT: usize = 8;

fn unique_model_curve<'a>(ir: &'a CadIr, id: &CurveId) -> Option<&'a Curve> {
    exactly_one(ir.model.curves.iter().filter(|curve| &curve.id == id))
}

pub(super) fn model_points_agree(first: [f64; 3], second: [f64; 3]) -> bool {
    let scale = first
        .into_iter()
        .chain(second)
        .map(f64::abs)
        .fold(1.0, f64::max);
    first
        .into_iter()
        .zip(second)
        .all(|(first, second)| (first - second).abs() <= EPS_AGREE * scale)
}

fn pcurve_candidate_agrees_with_fixed_points(
    vertices: [u32; 2],
    points: [[f64; 3]; 2],
    directions: [u8; 2],
    fixed_points: &BTreeMap<u32, [f64; 3]>,
) -> bool {
    let Some(ordered) = directed_pcurve_points(directions, points) else {
        return true;
    };
    vertices.into_iter().zip(ordered).all(|(vertex, point)| {
        fixed_points
            .get(&vertex)
            .is_none_or(|known| model_points_agree(*known, point))
    })
}

fn pcurve_endpoint_is_ambiguous(candidates: &[[f64; 3]]) -> bool {
    candidates.first().is_some_and(|first| {
        candidates
            .iter()
            .skip(1)
            .any(|candidate| !model_points_agree(*first, *candidate))
    })
}

fn line_line_intersection(first: &CurveGeometry, second: &CurveGeometry) -> Option<[f64; 3]> {
    let (
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)),
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve_2)),
    ) = (first, second)
    else {
        return None;
    };
    let first_origin = line_curve.origin().get();
    let first_direction = *line_curve.direction().as_raw();
    let second_origin = line_curve_2.origin().get();
    let second_direction = *line_curve_2.direction().as_raw();
    let first_origin = [first_origin.x, first_origin.y, first_origin.z];
    let second_origin = [second_origin.x, second_origin.y, second_origin.z];
    let first_direction = [first_direction.x, first_direction.y, first_direction.z];
    let second_direction = [second_direction.x, second_direction.y, second_direction.z];
    let relative = std::array::from_fn(|axis| first_origin[axis] - second_origin[axis]);
    let first_squared = dot(first_direction, first_direction);
    let second_squared = dot(second_direction, second_direction);
    let product = dot(first_direction, second_direction);
    let first_relative = dot(first_direction, relative);
    let second_relative = dot(second_direction, relative);
    let denominator = first_squared.mul_add(second_squared, -(product * product));
    if !denominator.is_finite()
        || denominator <= EPS_NEAR_ZERO * first_squared * second_squared
        || first_squared <= 0.0
        || second_squared <= 0.0
    {
        return None;
    }
    let first_parameter =
        product.mul_add(second_relative, -(second_squared * first_relative)) / denominator;
    let second_parameter =
        first_squared.mul_add(second_relative, -(product * first_relative)) / denominator;
    let first_point = std::array::from_fn(|axis| {
        first_direction[axis].mul_add(first_parameter, first_origin[axis])
    });
    let second_point = std::array::from_fn(|axis| {
        second_direction[axis].mul_add(second_parameter, second_origin[axis])
    });
    (first_point
        .iter()
        .chain(second_point.iter())
        .all(|value| value.is_finite())
        && model_points_agree(first_point, second_point))
    .then(|| std::array::from_fn(|axis| f64::midpoint(first_point[axis], second_point[axis])))
}

/// The points where a line meets a conic.
///
/// A line in the conic's plane restricts the conic to a quadratic in the line
/// parameter whose three coefficients are cancelling sums: the quadratic one
/// cancels where the direction is a hyperbola asymptote, the linear one where
/// the line origin is the conic centre, and the constant where the line origin
/// is on the conic. `real_roots` owns the degree and discriminant decisions for
/// that problem, so the coefficients reach it with the magnitudes of their own
/// terms.
///
/// The points are in ascending order of the line parameter, which is the order
/// `real_roots` states its roots in.
fn line_conic_intersections(line: &CurveGeometry, conic: &CurveGeometry) -> Vec<[f64; 3]> {
    let CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) = line else {
        return Vec::new();
    };
    let origin = line_curve.origin().get();
    let direction = *line_curve.direction().as_raw();
    let Some(PlanarConicEquation {
        origin: conic_origin,
        normal,
        x_axis,
        y_axis,
        quadratic,
        linear,
        constant,
        scale: conic_scale,
    }) = planar_conic_equation(conic)
    else {
        return Vec::new();
    };
    let origin = [origin.x, origin.y, origin.z];
    let Some(direction) = normalize([direction.x, direction.y, direction.z]) else {
        return Vec::new();
    };
    let relative = std::array::from_fn(|coordinate| origin[coordinate] - conic_origin[coordinate]);
    let direction_plane = dot(direction, normal);
    let origin_plane = dot(relative, normal);
    let model_scale = origin
        .into_iter()
        .chain(conic_origin)
        .map(f64::abs)
        .fold(conic_scale.max(1.0), f64::max);
    if direction_plane.abs() > EPS_NEAR_ZERO {
        let parameter = -origin_plane / direction_plane;
        let point = std::array::from_fn(|coordinate| {
            direction[coordinate].mul_add(parameter, origin[coordinate])
        });
        return (point.iter().all(|value| value.is_finite())
            && curve_contains_points(conic, [point, point]))
        .then_some(point)
        .into_iter()
        .collect();
    }
    if origin_plane.abs() > EPS_AGREE * model_scale {
        return Vec::new();
    }
    let local_origin = [dot(relative, x_axis), dot(relative, y_axis)];
    let local_direction = [dot(direction, x_axis), dot(direction, y_axis)];
    let line_quadratic = Coefficient::summed(
        quadratic[0].mul_add(
            local_direction[0].powi(2),
            quadratic[1] * local_direction[1].powi(2),
        ),
        quadratic[0].abs() * local_direction[0].powi(2)
            + quadratic[1].abs() * local_direction[1].powi(2),
    );
    let line_linear = Coefficient::summed(
        2.0 * quadratic[0].mul_add(
            local_origin[0] * local_direction[0],
            quadratic[1] * local_origin[1] * local_direction[1],
        ) + linear[0].mul_add(local_direction[0], linear[1] * local_direction[1]),
        2.0 * ((quadratic[0] * local_origin[0] * local_direction[0]).abs()
            + (quadratic[1] * local_origin[1] * local_direction[1]).abs())
            + (linear[0] * local_direction[0]).abs()
            + (linear[1] * local_direction[1]).abs(),
    );
    let line_constant = Coefficient::summed(
        quadratic[0].mul_add(
            local_origin[0].powi(2),
            quadratic[1] * local_origin[1].powi(2),
        ) + linear[0].mul_add(local_origin[0], linear[1] * local_origin[1])
            + constant,
        quadratic[0].abs() * local_origin[0].powi(2)
            + quadratic[1].abs() * local_origin[1].powi(2)
            + (linear[0] * local_origin[0]).abs()
            + (linear[1] * local_origin[1]).abs()
            + constant.abs(),
    );
    real_roots(line_quadratic, line_linear, line_constant)
        .into_iter()
        .map(|parameter| {
            std::array::from_fn(|coordinate| {
                direction[coordinate].mul_add(parameter, origin[coordinate])
            })
        })
        .filter(|point: &[f64; 3]| {
            point.iter().all(|value| value.is_finite())
                && curve_contains_points(conic, [*point, *point])
        })
        .collect()
}

/// The conic in the chart `origin + u * u_axis + v * v_axis`.
///
/// A chart point has the conic-frame coordinates `x[0] + u * x[1] + v * x[2]`
/// and `y[0] + u * y[1] + v * y[2]`, so every coefficient below is a sum of
/// products of the conic's own coefficients with those direction cosines. Each
/// sum can cancel: `uu` and `vv` where a chart axis lies along a hyperbola
/// asymptote, `uv` where the conic is a circle in a rotated chart, and
/// `constant` — the conic's equation at the chart origin — where the chart
/// origin lies on the conic. The rule that states zero inside the rounding
/// error of the terms is therefore applied to all six.
fn restrict_planar_conic_to_chart(
    conic: PlanarConicEquation,
    origin: [f64; 3],
    u_axis: [f64; 3],
    v_axis: [f64; 3],
) -> PlaneConicEquation {
    let offset: [f64; 3] =
        std::array::from_fn(|coordinate| origin[coordinate] - conic.origin[coordinate]);
    let x = [
        dot(offset, conic.x_axis),
        dot(u_axis, conic.x_axis),
        dot(v_axis, conic.x_axis),
    ];
    let y = [
        dot(offset, conic.y_axis),
        dot(u_axis, conic.y_axis),
        dot(v_axis, conic.y_axis),
    ];
    let [first_quadratic, second_quadratic] = conic.quadratic;
    let [first_linear, second_linear] = conic.linear;
    PlaneConicEquation {
        uu: Coefficient::summed(
            first_quadratic.mul_add(x[1].powi(2), second_quadratic * y[1].powi(2)),
            first_quadratic.abs() * x[1].powi(2) + second_quadratic.abs() * y[1].powi(2),
        ),
        uv: Coefficient::summed(
            2.0 * first_quadratic.mul_add(x[1] * x[2], second_quadratic * y[1] * y[2]),
            2.0 * ((first_quadratic * x[1] * x[2]).abs() + (second_quadratic * y[1] * y[2]).abs()),
        ),
        vv: Coefficient::summed(
            first_quadratic.mul_add(x[2].powi(2), second_quadratic * y[2].powi(2)),
            first_quadratic.abs() * x[2].powi(2) + second_quadratic.abs() * y[2].powi(2),
        ),
        u: Coefficient::summed(
            2.0 * first_quadratic.mul_add(x[0] * x[1], second_quadratic * y[0] * y[1])
                + first_linear.mul_add(x[1], second_linear * y[1]),
            2.0 * ((first_quadratic * x[0] * x[1]).abs() + (second_quadratic * y[0] * y[1]).abs())
                + (first_linear * x[1]).abs()
                + (second_linear * y[1]).abs(),
        ),
        v: Coefficient::summed(
            2.0 * first_quadratic.mul_add(x[0] * x[2], second_quadratic * y[0] * y[2])
                + first_linear.mul_add(x[2], second_linear * y[2]),
            2.0 * ((first_quadratic * x[0] * x[2]).abs() + (second_quadratic * y[0] * y[2]).abs())
                + (first_linear * x[2]).abs()
                + (second_linear * y[2]).abs(),
        ),
        constant: Coefficient::summed(
            first_quadratic.mul_add(x[0].powi(2), second_quadratic * y[0].powi(2))
                + first_linear.mul_add(x[0], second_linear * y[0])
                + conic.constant,
            first_quadratic.abs() * x[0].powi(2)
                + second_quadratic.abs() * y[0].powi(2)
                + (first_linear * x[0]).abs()
                + (second_linear * y[0]).abs()
                + conic.constant.abs(),
        ),
    }
}

fn conic_conic_intersections(first: &CurveGeometry, second: &CurveGeometry) -> Vec<[f64; 3]> {
    let Some(first_equation) = planar_conic_equation(first) else {
        return Vec::new();
    };
    let Some(second_equation) = planar_conic_equation(second) else {
        return Vec::new();
    };
    let normal_cross = cross(first_equation.normal, second_equation.normal);
    if dot(normal_cross, normal_cross) > 1e-18 {
        let Some((origin, direction)) = plane_intersection_line(
            PlaneEquation {
                origin: first_equation.origin,
                normal: first_equation.normal,
            },
            PlaneEquation {
                origin: second_equation.origin,
                normal: second_equation.normal,
            },
        ) else {
            return Vec::new();
        };
        let Ok(line) = cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::from(origin),
            Vector3::from(direction),
        ) else {
            return Vec::new();
        };
        let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(line));
        let mut points = line_conic_intersections(&line, first);
        points.retain(|point| curve_contains_points(second, [*point, *point]));
        return points;
    }
    let delta: [f64; 3] = std::array::from_fn(|coordinate| {
        second_equation.origin[coordinate] - first_equation.origin[coordinate]
    });
    let scale = first_equation
        .origin
        .into_iter()
        .chain(second_equation.origin)
        .map(f64::abs)
        .fold(
            first_equation.scale.max(second_equation.scale).max(1.0),
            f64::max,
        );
    if dot(delta, first_equation.normal).abs() > EPS_AGREE * scale {
        return Vec::new();
    }
    let first_chart = restrict_planar_conic_to_chart(
        first_equation,
        first_equation.origin,
        first_equation.x_axis,
        first_equation.y_axis,
    );
    let second_chart = restrict_planar_conic_to_chart(
        second_equation,
        first_equation.origin,
        first_equation.x_axis,
        first_equation.y_axis,
    );
    common_plane_conic_parameters(first_chart, second_chart)
        .into_iter()
        .map(|[u, v]| {
            std::array::from_fn(|coordinate| {
                first_equation.origin[coordinate]
                    + u * first_equation.x_axis[coordinate]
                    + v * first_equation.y_axis[coordinate]
            })
        })
        .filter(|point| {
            curve_contains_points(first, [*point, *point])
                && curve_contains_points(second, [*point, *point])
        })
        .collect()
}

fn incident_analytic_vertex_domain(curves: &[&CurveGeometry]) -> Vec<[f64; 3]> {
    let mut candidates = Vec::new();
    for first in 0..curves.len() {
        for second in first + 1..curves.len() {
            candidates.extend(
                line_line_intersection(curves[first], curves[second])
                    .into_iter()
                    .chain(line_conic_intersections(curves[first], curves[second]))
                    .chain(line_conic_intersections(curves[second], curves[first]))
                    .chain(conic_conic_intersections(curves[first], curves[second])),
            );
        }
    }
    candidates.retain(|point| {
        curves
            .iter()
            .all(|curve| curve_contains_points(curve, [*point, *point]))
    });
    candidates
        .into_iter()
        .fold(Vec::new(), |mut unique, point| {
            if !unique
                .iter()
                .any(|candidate| model_points_agree(*candidate, point))
            {
                unique.push(point);
            }
            unique
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CarrierFailureKind {
    NoGeometricCandidate,
    NoValidCandidate,
}

fn carrier_failure_kind(diagnostics: CarrierSolveDiagnostics) -> CarrierFailureKind {
    if diagnostics.pair_intersections == 0 && diagnostics.triple_intersections == 0 {
        CarrierFailureKind::NoGeometricCandidate
    } else {
        CarrierFailureKind::NoValidCandidate
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::decode) struct CarrierVertexDiagnostic {
    pub(in crate::decode) vertex_id: u32,
    pub(in crate::decode) incident_face_ids: Vec<u32>,
    pub(in crate::decode) carrier_kinds: Vec<&'static str>,
    pub(in crate::decode) pair_intersections: usize,
    pub(in crate::decode) triple_intersections: usize,
    pub(in crate::decode) valid_candidates: usize,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(in crate::decode) struct TopologicalVertexSolveDiagnostics {
    pub(in crate::decode) topological_vertices: usize,
    pub(in crate::decode) carrier_incident_vertices: usize,
    pub(in crate::decode) carrier_pair_candidates: usize,
    pub(in crate::decode) carrier_triple_candidates: usize,
    pub(in crate::decode) carrier_valid_candidates: usize,
    pub(in crate::decode) carrier_ambiguous_candidate_vertices: usize,
    pub(in crate::decode) carrier_no_geometric_candidate_vertices: usize,
    pub(in crate::decode) carrier_no_valid_candidate_vertices: usize,
    pub(in crate::decode) carrier_rejection_samples: Vec<CarrierVertexDiagnostic>,
    pub(in crate::decode) carrier_points: usize,
    pub(in crate::decode) pcurve: PcurveEndpointDiagnostics,
    pub(in crate::decode) pcurve_constraints: usize,
    pub(in crate::decode) pcurve_fixed_endpoint_conflicts: usize,
    pub(in crate::decode) pcurve_ambiguous_endpoint_vertices: usize,
    pub(in crate::decode) directed_endpoint_assignments: usize,
    pub(in crate::decode) directed_endpoint_conflicts: usize,
    pub(in crate::decode) nurbs_endpoint_constraints: usize,
    pub(in crate::decode) analytic_domain_vertices: usize,
    pub(in crate::decode) solved_vertices: usize,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(in crate::decode) struct SolvedTopologicalVertices {
    pub(in crate::decode) points: BTreeMap<u32, [f64; 3]>,
    pub(in crate::decode) diagnostics: TopologicalVertexSolveDiagnostics,
}

pub(in crate::decode) fn solve_topological_vertices(
    scan: &ContainerScan,
    ir: &CadIr,
    carriers: &BTreeMap<u32, CarrierEquation>,
    nurbs_endpoint_witnesses: &BTreeSet<CurveId>,
) -> SolvedTopologicalVertices {
    let mut diagnostics = TopologicalVertexSolveDiagnostics {
        topological_vertices: scan.topology.vertices.len(),
        ..TopologicalVertexSolveDiagnostics::default()
    };
    let vertex_faces =
        crate::topology::vertex_incident_faces(&scan.topology.vertices, &scan.topology.half_edges);
    let mut carrier_points = BTreeMap::new();
    for vertex in &scan.topology.vertices {
        let Some(face_ids) = vertex_faces.get(&vertex.id) else {
            continue;
        };
        let incident_face_ids = face_ids
            .iter()
            .filter(|face_id| carriers.contains_key(face_id))
            .copied()
            .collect::<Vec<_>>();
        let incident_carriers = incident_face_ids
            .iter()
            .filter_map(|face_id| carriers.get(face_id))
            .copied()
            .collect::<Vec<_>>();
        if incident_carriers.is_empty() {
            continue;
        }
        diagnostics.carrier_incident_vertices += 1;
        let (point, carrier_diagnostics) = solve_carriers_with_diagnostics(&incident_carriers);
        diagnostics.carrier_pair_candidates += carrier_diagnostics.pair_intersections;
        diagnostics.carrier_triple_candidates += carrier_diagnostics.triple_intersections;
        diagnostics.carrier_valid_candidates += carrier_diagnostics.valid_candidates;
        match carrier_diagnostics.unique_solutions {
            0 => {
                match carrier_failure_kind(carrier_diagnostics) {
                    CarrierFailureKind::NoGeometricCandidate => {
                        diagnostics.carrier_no_geometric_candidate_vertices += 1;
                    }
                    CarrierFailureKind::NoValidCandidate => {
                        diagnostics.carrier_no_valid_candidate_vertices += 1;
                    }
                }
                if diagnostics.carrier_rejection_samples.len() < CARRIER_VERTEX_SAMPLE_LIMIT {
                    diagnostics
                        .carrier_rejection_samples
                        .push(CarrierVertexDiagnostic {
                            vertex_id: vertex.id,
                            incident_face_ids: incident_face_ids.clone(),
                            carrier_kinds: incident_carriers
                                .iter()
                                .map(CarrierEquation::kind_str)
                                .collect(),
                            pair_intersections: carrier_diagnostics.pair_intersections,
                            triple_intersections: carrier_diagnostics.triple_intersections,
                            valid_candidates: carrier_diagnostics.valid_candidates,
                        });
                }
            }
            1 => {
                if let Some(point) = point {
                    carrier_points.insert(vertex.id, point);
                }
            }
            _ => diagnostics.carrier_ambiguous_candidate_vertices += 1,
        }
    }
    diagnostics.carrier_points = carrier_points.len();
    let edge_start_vertices =
        crate::topology::edge_start_vertex_pairs(&scan.topology.half_edge_vertex_incidence);
    let mut fixed_points = carrier_points;
    let (endpoint_evidence, pcurve_diagnostics) =
        pcurve_edge_endpoint_evidence_with_carriers(scan, ir, carriers);
    diagnostics.pcurve = pcurve_diagnostics;
    let edge_endpoints = endpoint_evidence
        .into_iter()
        .map(|(curve_id, evidence)| {
            (
                curve_id,
                (evidence.points, evidence.complete, evidence.authoritative),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let topology_rows = crate::topology::uniquely_identified_rows(&scan.curves.topology_rows);
    let mut pcurve_constraints = Vec::new();
    let mut pcurve_endpoint_candidates = BTreeMap::<u32, Vec<[f64; 3]>>::new();
    for row in &topology_rows {
        let Some((points, complete, authoritative)) = edge_endpoints.get(&row.id).copied() else {
            continue;
        };
        let Some(vertices) = edge_start_vertices.get(&row.id).copied() else {
            continue;
        };
        if !pcurve_candidate_agrees_with_fixed_points(
            vertices,
            points,
            row.directions,
            &fixed_points,
        ) {
            diagnostics.pcurve_fixed_endpoint_conflicts += 1;
            continue;
        }
        let ordered = directed_pcurve_points(row.directions, points);
        if let Some(ordered) = ordered {
            for (vertex, point) in vertices.into_iter().zip(ordered) {
                pcurve_endpoint_candidates
                    .entry(vertex)
                    .or_default()
                    .push(point);
            }
        }
        pcurve_constraints.push((vertices, points, ordered, complete, authoritative));
    }
    let ambiguous_pcurve_vertices = pcurve_endpoint_candidates
        .iter()
        .filter_map(|(vertex, candidates)| {
            pcurve_endpoint_is_ambiguous(candidates).then_some(*vertex)
        })
        .collect::<BTreeSet<_>>();
    diagnostics.pcurve_ambiguous_endpoint_vertices = ambiguous_pcurve_vertices.len();
    let mut constraints = Vec::new();
    let mut authoritative_points = BTreeMap::new();
    for (vertices, points, ordered, complete, authoritative) in pcurve_constraints {
        if let Some(ordered) = ordered {
            let ambiguous = vertices
                .iter()
                .any(|vertex| ambiguous_pcurve_vertices.contains(vertex));
            if ambiguous && !complete {
                continue;
            }
            diagnostics.pcurve_constraints += 1;
            constraints.push((vertices, points));
            if ambiguous {
                continue;
            }
            for (vertex, point) in vertices.into_iter().zip(ordered) {
                diagnostics.directed_endpoint_assignments += 1;
                fixed_points.entry(vertex).or_insert(point);
                if authoritative && !ambiguous {
                    authoritative_points.entry(vertex).or_insert(point);
                }
            }
        } else {
            diagnostics.pcurve_constraints += 1;
            constraints.push((vertices, points));
        }
    }
    for row in &topology_rows {
        let Some(vertices) = edge_start_vertices.get(&row.id).copied() else {
            continue;
        };
        let id = CurveId::compose(&crate::identity::VISIBGEOM_CURVE, row.id);
        if !nurbs_endpoint_witnesses.contains(&id) {
            continue;
        }
        let Some(geometry) = unique_model_curve(ir, &id) else {
            continue;
        };
        let Some(points) = nonperiodic_nurbs_endpoint_points(&geometry.geometry) else {
            continue;
        };
        diagnostics.nurbs_endpoint_constraints += 1;
        constraints.push((vertices, points));
    }
    // Non-periodic NURBS boundary rows contribute their intrinsic endpoint
    // pair through the witness constraint above. They are not analytic
    // carrier equations for the vertex-domain solver.
    let analytic_curves = topology_rows
        .into_iter()
        .filter_map(|row| {
            let id = CurveId::compose(&crate::identity::VISIBGEOM_CURVE, row.id);
            let geometry = &unique_model_curve(ir, &id)?.geometry;
            let evaluable = matches!(
                geometry,
                CurveGeometry::Solved(
                    SolvedCurveGeometry::Line(_)
                        | SolvedCurveGeometry::Circle(_)
                        | SolvedCurveGeometry::Ellipse(_)
                        | SolvedCurveGeometry::Parabola(_)
                        | SolvedCurveGeometry::Hyperbola(_)
                )
            );
            evaluable.then_some((row.id, geometry))
        })
        .collect::<BTreeMap<_, _>>();
    let incident_curves = scan
        .topology
        .vertices
        .iter()
        .filter_map(|vertex| {
            let curves = vertex
                .half_edges
                .iter()
                .filter_map(|half_edge| analytic_curves.get(&half_edge.curve_id).copied())
                .collect::<Vec<_>>();
            (!curves.is_empty()).then_some((vertex.id, curves))
        })
        .collect::<BTreeMap<_, _>>();
    let analytic_domains = incident_curves
        .iter()
        .filter_map(|(vertex, curves)| {
            let candidates = incident_analytic_vertex_domain(curves);
            (!candidates.is_empty()).then_some((*vertex, candidates))
        })
        .collect::<BTreeMap<_, _>>();
    diagnostics.analytic_domain_vertices = analytic_domains.len();
    let points = solve_pcurve_vertex_domains_with_authoritative_points(
        &constraints,
        &fixed_points,
        &analytic_domains,
        &incident_curves,
        &authoritative_points,
    );
    diagnostics.solved_vertices = points.len();
    SolvedTopologicalVertices {
        points,
        diagnostics,
    }
}

pub(in crate::decode) fn solved_topological_vertices(
    scan: &ContainerScan,
    ir: &CadIr,
    carriers: &BTreeMap<u32, CarrierEquation>,
    nurbs_endpoint_witnesses: &BTreeSet<CurveId>,
) -> BTreeMap<u32, [f64; 3]> {
    solve_topological_vertices(scan, ir, carriers, nurbs_endpoint_witnesses).points
}

#[cfg(test)]
mod topological_tests;

#[cfg(test)]
mod tests {
    use super::super::edges::PlanarConicEquation;
    use super::super::equations::common_plane_conic_parameters;
    use super::super::planes::CarrierSolveDiagnostics;
    use super::{
        carrier_failure_kind, line_conic_intersections, pcurve_endpoint_is_ambiguous,
        restrict_planar_conic_to_chart, unique_model_curve, CarrierFailureKind,
    };
    use crate::vecmath::normalize;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::math::{Point3, Vector3};

    const CHART_ORIGIN: [f64; 3] = [0.0, 0.0, 0.0];
    const CHART_U_AXIS: [f64; 3] = [1.0, 0.0, 0.0];
    const CHART_V_AXIS: [f64; 3] = [0.0, 1.0, 0.0];
    const EPS_TEST_CONIC_RESIDUAL: f64 = 1.0e-6;

    /// A circle of the given radius centred on the chart origin, whose frame is
    /// the chart frame.
    fn chart_circle(radius: f64) -> PlanarConicEquation {
        PlanarConicEquation {
            origin: CHART_ORIGIN,
            normal: [0.0, 0.0, 1.0],
            x_axis: CHART_U_AXIS,
            y_axis: CHART_V_AXIS,
            quadratic: [1.0 / (radius * radius), 1.0 / (radius * radius)],
            linear: [0.0, 0.0],
            constant: -1.0,
            scale: radius,
        }
    }

    /// The conic's own equation at a model point. Every conic here has constant
    /// -1, so the value is dimensionless and zero exactly on the curve.
    fn conic_value(conic: PlanarConicEquation, point: [f64; 3]) -> f64 {
        let offset: [f64; 3] =
            std::array::from_fn(|coordinate| point[coordinate] - conic.origin[coordinate]);
        let x = super::dot(offset, conic.x_axis);
        let y = super::dot(offset, conic.y_axis);
        conic.quadratic[0] * x * x
            + conic.quadratic[1] * y * y
            + conic.linear[0] * x
            + conic.linear[1] * y
            + conic.constant
    }

    /// The model point of a chart parameter pair.
    fn chart_point(parameter: [f64; 2]) -> [f64; 3] {
        std::array::from_fn(|coordinate| {
            CHART_ORIGIN[coordinate]
                + parameter[0] * CHART_U_AXIS[coordinate]
                + parameter[1] * CHART_V_AXIS[coordinate]
        })
    }

    fn stated_parameters(first: PlanarConicEquation, second: PlanarConicEquation) -> Vec<[f64; 2]> {
        common_plane_conic_parameters(
            restrict_planar_conic_to_chart(first, CHART_ORIGIN, CHART_U_AXIS, CHART_V_AXIS),
            restrict_planar_conic_to_chart(second, CHART_ORIGIN, CHART_U_AXIS, CHART_V_AXIS),
        )
    }

    #[test]
    fn numerical_followup_chart_along_a_hyperbola_asymptote_states_no_extra_root() {
        // The chart v axis is the asymptote direction (a, b) of the hyperbola
        // x^2/a^2 - y^2/b^2 = 1, so the exact vv of the restricted conic is
        // zero and the restricted conic is linear in v.
        let semi_axis = 100.0;
        let semi_conjugate_axis = 200.0;
        let hyperbola = PlanarConicEquation {
            origin: [-800.0, -700.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            x_axis: normalize([semi_conjugate_axis, semi_axis, 0.0]).expect("planar unit axis"),
            y_axis: normalize([-semi_axis, semi_conjugate_axis, 0.0]).expect("planar unit axis"),
            quadratic: [
                1.0 / (semi_axis * semi_axis),
                -1.0 / (semi_conjugate_axis * semi_conjugate_axis),
            ],
            linear: [0.0, 0.0],
            constant: -1.0,
            scale: semi_conjugate_axis,
        };
        let circle = chart_circle(1000.0);

        let parameters = stated_parameters(circle, hyperbola);
        assert_eq!(parameters.len(), 2);
        for parameter in parameters {
            let point = chart_point(parameter);
            assert!(conic_value(circle, point).abs() <= EPS_TEST_CONIC_RESIDUAL);
            assert!(conic_value(hyperbola, point).abs() <= EPS_TEST_CONIC_RESIDUAL);
        }

        let chart =
            restrict_planar_conic_to_chart(hyperbola, CHART_ORIGIN, CHART_U_AXIS, CHART_V_AXIS);
        assert_eq!(chart.vv.stated(), 0.0);
    }

    #[test]
    fn numerical_followup_chart_origin_on_the_conic_states_the_intersections() {
        // The ellipse passes through the circle's centre, which is the chart
        // origin, so the exact constant of the restricted conic is zero.
        let semi_major_axis = 1.577_223_779_332_398_7e6;
        let semi_minor_axis = 2.201_651_457_059_877_5e6;
        let heading: f64 = 3.320_901_222_737_607_6;
        let parameter_on_ellipse: f64 = 2.857_871_476_802_07;
        let x_axis = normalize([heading.cos(), heading.sin(), 0.0]).expect("planar unit axis");
        let y_axis = normalize([-heading.sin(), heading.cos(), 0.0]).expect("planar unit axis");
        let ellipse = PlanarConicEquation {
            origin: std::array::from_fn(|coordinate| {
                -(semi_major_axis * parameter_on_ellipse.cos()) * x_axis[coordinate]
                    - (semi_minor_axis * parameter_on_ellipse.sin()) * y_axis[coordinate]
            }),
            normal: [0.0, 0.0, 1.0],
            x_axis,
            y_axis,
            quadratic: [
                1.0 / (semi_major_axis * semi_major_axis),
                1.0 / (semi_minor_axis * semi_minor_axis),
            ],
            linear: [0.0, 0.0],
            constant: -1.0,
            scale: semi_minor_axis,
        };
        let circle = chart_circle(9.629_502_301_664_337e5);

        let parameters = stated_parameters(circle, ellipse);
        assert!(!parameters.is_empty());
        for parameter in parameters {
            let point = chart_point(parameter);
            assert!(conic_value(circle, point).abs() <= EPS_TEST_CONIC_RESIDUAL);
            assert!(conic_value(ellipse, point).abs() <= EPS_TEST_CONIC_RESIDUAL);
        }

        let chart =
            restrict_planar_conic_to_chart(ellipse, CHART_ORIGIN, CHART_U_AXIS, CHART_V_AXIS);
        assert_eq!(chart.constant.stated(), 0.0);
    }

    #[test]
    fn numerical_followup_residual_bound_refuses_a_candidate_off_both_conics() {
        // The hyperbola crosses the circle twice. Beside one of the two the
        // resultant states a further root, and the refinement leaves it 4e-3
        // away with a residual of 2.6e-5 against the circle and 3.8e-5 against
        // the hyperbola, read against conic constants of -1. No coefficient
        // bound and no correction of the refinement reaches that; only a
        // tolerance that grows with the candidate's own magnitude does.
        let heading = std::f64::consts::FRAC_PI_4;
        let hyperbola = PlanarConicEquation {
            origin: [200.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            x_axis: normalize([heading.cos(), heading.sin(), 0.0]).expect("planar unit axis"),
            y_axis: normalize([-heading.sin(), heading.cos(), 0.0]).expect("planar unit axis"),
            quadratic: [1.0 / (20.0 * 20.0), -1.0 / (40.0 * 40.0)],
            linear: [0.0, 0.0],
            constant: -1.0,
            scale: 40.0,
        };
        let circle = chart_circle(100.0);

        let parameters = stated_parameters(circle, hyperbola);

        assert_eq!(parameters.len(), 2);
        for parameter in parameters {
            let point = chart_point(parameter);
            assert!(conic_value(circle, point).abs() <= EPS_TEST_CONIC_RESIDUAL);
            assert!(conic_value(hyperbola, point).abs() <= EPS_TEST_CONIC_RESIDUAL);
        }
    }

    #[test]
    fn numerical_followup_resultant_quartic_term_states_both_branch_crossings() {
        // The circle of radius 10000 crosses each branch of the hyperbola
        // twice, so the resultant is a real quartic in u. Its quartic
        // coefficient is 3.2e-15 of its largest one, and a rule that reads that
        // ratio against a fixed fraction drops it and states the left branch
        // pair alone.
        let hyperbola = PlanarConicEquation {
            origin: [5000.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            x_axis: CHART_U_AXIS,
            y_axis: CHART_V_AXIS,
            quadratic: [1.0 / (20.0 * 20.0), -1.0 / (80.0 * 80.0)],
            linear: [0.0, 0.0],
            constant: -1.0,
            scale: 80.0,
        };
        let circle = chart_circle(10000.0);

        let parameters = stated_parameters(circle, hyperbola);

        assert_eq!(parameters.len(), 4);
        assert_eq!(
            parameters
                .iter()
                .filter(|parameter| parameter[0] > 5000.0)
                .count(),
            2
        );
        assert_eq!(
            parameters
                .iter()
                .filter(|parameter| parameter[0] < 5000.0)
                .count(),
            2
        );
        for parameter in parameters {
            let point = chart_point(parameter);
            assert!(conic_value(circle, point).abs() <= EPS_TEST_CONIC_RESIDUAL);
            assert!(conic_value(hyperbola, point).abs() <= EPS_TEST_CONIC_RESIDUAL);
        }
    }

    #[test]
    fn numerical_followup_tangent_conics_state_the_contact_point() {
        // An ellipse touching a circle from inside, at 0.39 radians off both
        // chart axes. The ellipse's major vertex sits on the circle and the two
        // share their tangent there, so the contact is a double root of the
        // resultant, whose location the rounding of the resultant's own
        // coefficients displaces by the square root of the rounding unit, and
        // where the pair of conics has a singular Jacobian. The pair therefore
        // states no converged step at the contact, and what the refinement
        // leaves is 3.3e-5 from it at a model scale of 2000. Solving for the
        // contact as a contact states it to half an ulp of that scale.
        const HEADING: f64 = 0.39;
        const RADIUS: f64 = 2000.0;
        const SEMI_MAJOR_AXIS: f64 = 1200.0;
        const EPS_TEST_CONTACT: f64 = 1.0e-9;
        let semi_minor_axis = SEMI_MAJOR_AXIS * 0.9_f64.sqrt();
        let x_axis = [HEADING.cos(), HEADING.sin(), 0.0];
        let y_axis = [-HEADING.sin(), HEADING.cos(), 0.0];
        let ellipse = PlanarConicEquation {
            origin: std::array::from_fn(|coordinate| {
                (RADIUS - SEMI_MAJOR_AXIS) * x_axis[coordinate]
            }),
            normal: [0.0, 0.0, 1.0],
            x_axis,
            y_axis,
            quadratic: [
                1.0 / (SEMI_MAJOR_AXIS * SEMI_MAJOR_AXIS),
                1.0 / (semi_minor_axis * semi_minor_axis),
            ],
            linear: [0.0, 0.0],
            constant: -1.0,
            scale: SEMI_MAJOR_AXIS,
        };
        let circle = chart_circle(RADIUS);

        let parameters = stated_parameters(circle, ellipse);

        assert_eq!(parameters.len(), 1);
        let point = chart_point(parameters[0]);
        let contact: [f64; 3] = std::array::from_fn(|coordinate| RADIUS * x_axis[coordinate]);
        for coordinate in 0..3 {
            assert!((point[coordinate] - contact[coordinate]).abs() <= EPS_TEST_CONTACT);
        }
        assert!(conic_value(circle, point).abs() <= EPS_TEST_CONIC_RESIDUAL);
        assert!(conic_value(ellipse, point).abs() <= EPS_TEST_CONIC_RESIDUAL);
    }

    #[test]
    fn numerical_followup_line_tangent_to_a_circle_states_one_point() {
        // The line x = 100 touches the circle of radius 100 at (100, 0, 0), so
        // the exact discriminant of the conic restricted to the line is zero.
        // The line origin is 1000 along the line from the tangency, which puts
        // the linear coefficient at 2e-3 and the rounding of its square above
        // the coefficient scale the solver used to state a repeated root.
        const RADIUS: f64 = 100.0;
        const LINE_OFFSET: f64 = 1000.0;
        const EPS_TEST_TANGENCY: f64 = 1.0e-9;
        let circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                RADIUS,
            )
            .expect("valid CircleCurve fixture"),
        ));
        let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(RADIUS, LINE_OFFSET, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            )
            .expect("valid LineCurve fixture"),
        ));

        let points = line_conic_intersections(&line, &circle);

        assert_eq!(points.len(), 1);
        assert!((points[0][0] - RADIUS).abs() <= EPS_TEST_TANGENCY);
        assert!(points[0][1].abs() <= EPS_TEST_TANGENCY);
        assert!(points[0][2].abs() <= EPS_TEST_TANGENCY);
    }

    #[test]
    fn unique_model_curve_rejects_duplicate_ids() {
        let id = CurveId::mint("creo:visibgeom:curve#7".to_string()).expect("identity grammar");
        let mut ir = CadIr::empty();
        ir.model.curves.extend([
            Curve {
                id: id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid LineCurve fixture"),
                )),
                source_object: None,
            },
            Curve {
                id: id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        Point3::new(0.0, 1.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid LineCurve fixture"),
                )),
                source_object: None,
            },
        ]);

        assert!(unique_model_curve(&ir, &id).is_none());
    }

    #[test]
    fn carrier_failure_kind_distinguishes_generation_from_validation() {
        assert_eq!(
            carrier_failure_kind(CarrierSolveDiagnostics::default()),
            CarrierFailureKind::NoGeometricCandidate
        );
        assert_eq!(
            carrier_failure_kind(CarrierSolveDiagnostics {
                triple_intersections: 1,
                ..CarrierSolveDiagnostics::default()
            }),
            CarrierFailureKind::NoValidCandidate
        );
    }

    #[test]
    fn pcurve_endpoint_ambiguity_requires_distinct_points() {
        const EPS_TEST_POINT_AGREE: f64 = 1.0e-12;
        assert!(!pcurve_endpoint_is_ambiguous(&[[1.0, 2.0, 3.0]]));
        assert!(!pcurve_endpoint_is_ambiguous(&[
            [1.0, 2.0, 3.0],
            [1.0 + EPS_TEST_POINT_AGREE, 2.0, 3.0],
        ]));
        assert!(pcurve_endpoint_is_ambiguous(&[
            [1.0, 2.0, 3.0],
            [1.1, 2.0, 3.0],
        ]));
    }
}
