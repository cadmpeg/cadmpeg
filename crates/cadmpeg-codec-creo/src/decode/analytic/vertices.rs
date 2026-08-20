// SPDX-License-Identifier: Apache-2.0
//! Line/conic intersections and solved topological vertices.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::{Point3, Vector3};

use crate::container::ContainerScan;

use super::super::sketch::normalized;
use super::super::surfaces::curve_contains_points;

use super::super::uniqueness::exactly_one;
use super::edges::{nonperiodic_nurbs_endpoint_points, planar_conic_equation, PlanarConicEquation};
use super::equations::{
    common_plane_conic_parameters, cross, dot, plane_intersection_line, CarrierEquation,
    PlaneConicEquation, PlaneEquation,
};
use super::pcurves::{
    directed_pcurve_points, pcurve_edge_endpoint_evidence_with_diagnostics,
    solve_pcurve_vertex_domains, PcurveEndpointDiagnostics,
};
use super::planes::solve_carriers_with_diagnostics;

const EPS_AGREE: f64 = 1e-9;
const EPS_NEAR_ZERO: f64 = 1e-12;

fn unique_model_curve<'a>(ir: &'a CadIr, id: &CurveId) -> Option<&'a Curve> {
    exactly_one(ir.model.curves.iter().filter(|curve| &curve.id == id))
}

pub fn model_points_agree(first: [f64; 3], second: [f64; 3]) -> bool {
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

pub fn line_line_intersection(first: &CurveGeometry, second: &CurveGeometry) -> Option<[f64; 3]> {
    let (
        CurveGeometry::Line {
            origin: first_origin,
            direction: first_direction,
        },
        CurveGeometry::Line {
            origin: second_origin,
            direction: second_direction,
        },
    ) = (first, second)
    else {
        return None;
    };
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

pub fn line_conic_intersections(line: &CurveGeometry, conic: &CurveGeometry) -> Vec<[f64; 3]> {
    let CurveGeometry::Line { origin, direction } = line else {
        return Vec::new();
    };
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
    let Some(direction) = normalized([direction.x, direction.y, direction.z]) else {
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
    let line_quadratic = quadratic[0].mul_add(
        local_direction[0].powi(2),
        quadratic[1] * local_direction[1].powi(2),
    );
    let line_linear =
        2.0 * quadratic[0].mul_add(
            local_origin[0] * local_direction[0],
            quadratic[1] * local_origin[1] * local_direction[1],
        ) + linear[0].mul_add(local_direction[0], linear[1] * local_direction[1]);
    let line_constant = quadratic[0].mul_add(
        local_origin[0].powi(2),
        quadratic[1] * local_origin[1].powi(2),
    ) + linear[0].mul_add(local_origin[0], linear[1] * local_origin[1])
        + constant;
    let coefficient_scale = line_linear
        .abs()
        .max((line_quadratic * line_constant).abs().sqrt())
        .max(1.0);
    let coefficient_tolerance = 1e-14 * coefficient_scale;
    if !line_quadratic.is_finite() || !line_linear.is_finite() || !line_constant.is_finite() {
        return Vec::new();
    }
    if line_quadratic.abs() <= coefficient_tolerance {
        if line_linear.abs() <= coefficient_tolerance {
            return Vec::new();
        }
        let parameter = -line_constant / line_linear;
        let point = std::array::from_fn(|coordinate| {
            direction[coordinate].mul_add(parameter, origin[coordinate])
        });
        return curve_contains_points(conic, [point, point])
            .then_some(point)
            .into_iter()
            .collect();
    }
    let discriminant = line_linear.mul_add(line_linear, -4.0 * line_quadratic * line_constant);
    let tolerance = EPS_NEAR_ZERO * coefficient_scale * coefficient_scale;
    if !discriminant.is_finite() || discriminant < -tolerance {
        return Vec::new();
    }
    let root = discriminant.max(0.0).sqrt();
    let first_parameter = -line_linear / (2.0 * line_quadratic);
    let first = std::array::from_fn(|coordinate| {
        direction[coordinate].mul_add(first_parameter, origin[coordinate])
    });
    if root <= EPS_AGREE * coefficient_scale {
        return curve_contains_points(conic, [first, first])
            .then_some(first)
            .into_iter()
            .collect();
    }
    let root_product = -0.5 * (line_linear + root.copysign(line_linear));
    let first_parameter = root_product / line_quadratic;
    let second_parameter = line_constant / root_product;
    let first = std::array::from_fn(|coordinate| {
        direction[coordinate].mul_add(first_parameter, origin[coordinate])
    });
    let second = std::array::from_fn(|coordinate| {
        direction[coordinate].mul_add(second_parameter, origin[coordinate])
    });
    [first, second]
        .into_iter()
        .filter(|point| curve_contains_points(conic, [*point, *point]))
        .collect()
}

pub fn restrict_planar_conic_to_chart(
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
    PlaneConicEquation {
        uu: conic.quadratic[0].mul_add(x[1].powi(2), conic.quadratic[1] * y[1].powi(2)),
        uv: 2.0 * conic.quadratic[0].mul_add(x[1] * x[2], conic.quadratic[1] * y[1] * y[2]),
        vv: conic.quadratic[0].mul_add(x[2].powi(2), conic.quadratic[1] * y[2].powi(2)),
        u: 2.0 * conic.quadratic[0].mul_add(x[0] * x[1], conic.quadratic[1] * y[0] * y[1])
            + conic.linear[0].mul_add(x[1], conic.linear[1] * y[1]),
        v: 2.0 * conic.quadratic[0].mul_add(x[0] * x[2], conic.quadratic[1] * y[0] * y[2])
            + conic.linear[0].mul_add(x[2], conic.linear[1] * y[2]),
        constant: conic.quadratic[0].mul_add(x[0].powi(2), conic.quadratic[1] * y[0].powi(2))
            + conic.linear[0].mul_add(x[0], conic.linear[1] * y[0])
            + conic.constant,
    }
}

pub fn conic_conic_intersections(first: &CurveGeometry, second: &CurveGeometry) -> Vec<[f64; 3]> {
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
        let line = CurveGeometry::Line {
            origin: Point3::new(origin[0], origin[1], origin[2]),
            direction: Vector3::new(direction[0], direction[1], direction[2]),
        };
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

pub fn incident_analytic_vertex_domain(curves: &[&CurveGeometry]) -> Vec<[f64; 3]> {
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

#[derive(Debug, Default, Clone, PartialEq)]
pub struct TopologicalVertexSolveDiagnostics {
    pub topological_vertices: usize,
    pub carrier_incident_vertices: usize,
    pub carrier_pair_candidates: usize,
    pub carrier_triple_candidates: usize,
    pub carrier_valid_candidates: usize,
    pub carrier_zero_candidate_vertices: usize,
    pub carrier_ambiguous_candidate_vertices: usize,
    pub carrier_points: usize,
    pub pcurve: PcurveEndpointDiagnostics,
    pub pcurve_constraints: usize,
    pub directed_endpoint_assignments: usize,
    pub directed_endpoint_conflicts: usize,
    pub nurbs_endpoint_constraints: usize,
    pub analytic_domain_vertices: usize,
    pub solved_vertices: usize,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SolvedTopologicalVertices {
    pub points: BTreeMap<u32, [f64; 3]>,
    pub diagnostics: TopologicalVertexSolveDiagnostics,
}

pub fn solve_topological_vertices(
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
        let incident_carriers = face_ids
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
            0 => diagnostics.carrier_zero_candidate_vertices += 1,
            1 => {
                if let Some(point) = point {
                    carrier_points.insert(vertex.id, point);
                }
            }
            _ => diagnostics.carrier_ambiguous_candidate_vertices += 1,
        }
    }
    diagnostics.carrier_points = carrier_points.len();
    let (endpoint_evidence, pcurve_diagnostics) =
        pcurve_edge_endpoint_evidence_with_diagnostics(scan, ir);
    diagnostics.pcurve = pcurve_diagnostics;
    let edge_endpoints = endpoint_evidence
        .into_iter()
        .map(|(curve_id, evidence)| (curve_id, evidence.points))
        .collect::<BTreeMap<_, _>>();
    let edge_vertices =
        crate::topology::edge_vertex_pairs(&scan.topology.half_edge_vertex_incidence);
    let mut fixed_points = carrier_points
        .into_iter()
        .map(|(vertex, point)| (vertex, Some(point)))
        .collect::<BTreeMap<_, _>>();
    let mut constraints = Vec::new();
    for row in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        let Some(points) = edge_endpoints.get(&row.id).copied() else {
            continue;
        };
        let Some(vertices) = edge_vertices.get(&row.id).copied() else {
            continue;
        };
        diagnostics.pcurve_constraints += 1;
        constraints.push((vertices, points));
        if let Some(ordered) = directed_pcurve_points(row.directions, points) {
            for (vertex, point) in vertices.into_iter().zip(ordered) {
                diagnostics.directed_endpoint_assignments += 1;
                match fixed_points.entry(vertex) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(Some(point));
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        if entry
                            .get()
                            .is_none_or(|known| !model_points_agree(known, point))
                        {
                            diagnostics.directed_endpoint_conflicts += 1;
                            entry.insert(None);
                        }
                    }
                }
            }
        }
    }
    for row in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        let Some(vertices) = edge_vertices.get(&row.id).copied() else {
            continue;
        };
        let id = CurveId(format!("creo:visibgeom:curve#{}", row.id));
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
    let analytic_curves = crate::topology::uniquely_identified_rows(&scan.curves.topology_rows)
        .into_iter()
        .filter_map(|row| {
            let id = CurveId(format!("creo:visibgeom:curve#{}", row.id));
            let geometry = &unique_model_curve(ir, &id)?.geometry;
            let evaluable = matches!(
                geometry,
                CurveGeometry::Line { .. }
                    | CurveGeometry::Circle { .. }
                    | CurveGeometry::Ellipse { .. }
                    | CurveGeometry::Parabola { .. }
                    | CurveGeometry::Hyperbola { .. }
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
    let points = solve_pcurve_vertex_domains(
        &constraints,
        &fixed_points,
        &analytic_domains,
        &incident_curves,
    );
    diagnostics.solved_vertices = points.len();
    SolvedTopologicalVertices {
        points,
        diagnostics,
    }
}

pub fn solved_topological_vertices(
    scan: &ContainerScan,
    ir: &CadIr,
    carriers: &BTreeMap<u32, CarrierEquation>,
    nurbs_endpoint_witnesses: &BTreeSet<CurveId>,
) -> BTreeMap<u32, [f64; 3]> {
    solve_topological_vertices(scan, ir, carriers, nurbs_endpoint_witnesses).points
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::units::Units;

    #[test]
    fn unique_model_curve_rejects_duplicate_ids() {
        let id = CurveId("creo:visibgeom:curve#7".to_string());
        let mut ir = CadIr::empty(Units::default());
        ir.model.curves.extend([
            Curve {
                id: id.clone(),
                geometry: CurveGeometry::Line {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    direction: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            },
            Curve {
                id: id.clone(),
                geometry: CurveGeometry::Line {
                    origin: Point3::new(0.0, 1.0, 0.0),
                    direction: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            },
        ]);

        assert!(unique_model_curve(&ir, &id).is_none());
    }
}
