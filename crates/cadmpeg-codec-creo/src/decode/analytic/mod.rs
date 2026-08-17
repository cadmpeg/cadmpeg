// SPDX-License-Identifier: Apache-2.0
//! Analytic carriers, plane reconciliation, vertices, edge parameters, and pcurves.

mod carriers;
mod edges;
mod equations;
mod pcurve_geometry;
mod pcurves;
mod planes;
mod vertices;

// Preserve the former analytic.rs `pub(super)` surface for sibling modules.
// Named re-exports that siblings do not import still need unused_imports.
#[allow(unused_imports)]
pub(super) use carriers::{
    face_boundary_plane, geometry_section_record, native_face_orientations, ordered_face_loops,
    ordered_planar_face_loops, placed_carriers, polygon_strictly_contains, projected_loop_polygon,
    retain_unresolved_visible_carriers, rowless_round_face_orientations,
    transfer_topology_bound_planes,
};
#[allow(unused_imports)]
pub(super) use edges::{
    degree_one_nurbs_point_parameter, exact_line_edge_parameter_range,
    full_periodic_conic_edge_parameter_range, full_periodic_nurbs_edge_parameter_range,
    nonperiodic_conic_edge_parameter_range, nonperiodic_conic_frame, nonperiodic_conic_parameter,
    nonperiodic_nurbs_edge_parameter_range, nonperiodic_nurbs_endpoint_points,
    nurbs_control_extent, nurbs_intrinsic_parameter_range, orient_line_edge_carrier,
    orient_nonperiodic_nurbs_edge_carrier, periodic_conic_edge_parameter_range,
    periodic_conic_frame, planar_conic_equation, point_pair_alignments, NonperiodicConicFamily,
    NonperiodicConicFrame, PeriodicConicFrame, PlanarConicEquation,
};
#[allow(unused_imports)]
pub(super) use equations::{
    carrier_quadric, circle_parameters, circular_cone, common_plane_conic_parameters,
    conic_resultant, cross, dot, intersect_plane_with_circle, intersect_plane_with_two_quadrics,
    intersect_two_planes_with_quadric, intersect_two_planes_with_torus, matrix_vector,
    outer_product, plane_cone_conic, plane_conic_value, plane_intersection_line,
    polynomial_product, polynomial_value, quadratic_real_roots, real_polynomial_roots,
    refine_plane_conic_intersection, restrict_quadric_to_plane, solve_planes, CarrierEquation,
    ConeEquation, CylinderEquation, PlaneConicEquation, PlaneEquation, QuadricEquation,
    SphereEquation, TorusEquation, QUARTIC_RESULTANT_PERMUTATIONS,
};
#[allow(unused_imports)]
pub(super) use pcurve_geometry::{
    meridian_circle_pcurve, ruled_generator_line_pcurve, stored_unit_vector,
    surface_of_revolution_parallel_pcurve,
};
#[allow(unused_imports)]
pub(super) use pcurves::{
    directed_pcurve_points, linear_pcurve_carrier, mapped_pcurve_endpoints, native_pcurve_midpoint,
    oriented_native_pcurve_endpoints, pcurve_backed_periodic_conic_parameter_range,
    pcurve_edge_endpoint_evidence, pcurve_edge_endpoints, planar_curve_pcurve,
    solve_pcurve_vertex_domains, transfer_analytic_pcurve_carriers, unique_oriented_native_pcurve,
    NativePcurveCandidates, PcurveEndpointEvidence, PcurveVertexConstraint,
};
#[allow(unused_imports)]
pub(super) use planes::{
    agreed_plane, agreed_plane_surface, agreed_topology_bound_plane, analytic_boundary_line,
    analytic_curve_plane, canonical_plane, envelope_reconciled_plane_candidate,
    frame_bound_outline_plane_candidate, held_coordinate_plane, is_axis_aligned,
    placed_plane_surfaces, placed_planes, plane_candidates, point_on_carrier,
    reconciled_model_plane, solve_carriers, tangent_plane_sphere_point, tangent_sphere_point,
    topology_bound_line_plane, topology_bound_plane, valid_positive_nurbs_curve, BoundaryLine,
    PlaneCandidate, PlaneChart,
};
#[allow(unused_imports)]
pub(super) use vertices::{
    conic_conic_intersections, incident_analytic_vertex_domain, line_conic_intersections,
    line_line_intersection, model_points_agree, restrict_planar_conic_to_chart,
    solved_topological_vertices,
};
