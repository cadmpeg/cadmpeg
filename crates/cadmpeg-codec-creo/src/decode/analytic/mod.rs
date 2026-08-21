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
    geometry_section_record, native_face_orientations, ordered_face_loops,
    ordered_planar_face_loops, placed_carriers, retain_unresolved_visible_carriers,
    rowless_round_face_orientations, transfer_topology_bound_planes,
};
#[allow(unused_imports)]
pub(super) use edges::{
    exact_line_edge_parameter_range, full_periodic_conic_edge_parameter_range,
    full_periodic_nurbs_edge_parameter_range, nonperiodic_conic_edge_parameter_range,
    nonperiodic_conic_parameter, nonperiodic_nurbs_edge_parameter_range,
    nonperiodic_nurbs_endpoint_points, nurbs_intrinsic_parameter_range, orient_line_edge_carrier,
    periodic_conic_edge_parameter_range, periodic_conic_frame, PeriodicConicFrame,
};
#[allow(unused_imports)]
pub(super) use equations::{
    circle_parameters, circular_cone, cross, dot, intersect_plane_with_circle,
    intersect_plane_with_two_quadrics, intersect_two_planes_with_torus, plane_cone_conic,
    quadratic_real_roots, solve_planes, CarrierEquation, ConeEquation, CylinderEquation,
    PlaneEquation, SphereEquation, TorusEquation,
};
pub(super) use pcurve_geometry::{
    meridian_circle_pcurve, ruled_generator_line_pcurve, surface_of_revolution_parallel_pcurve,
};
#[allow(unused_imports)]
pub(super) use pcurves::{
    directed_pcurve_points, linear_pcurve_carrier, mapped_pcurve_endpoints, native_pcurve_midpoint,
    oriented_native_pcurve_endpoints, pcurve_backed_periodic_conic_parameter_range,
    planar_curve_pcurve, solve_pcurve_vertex_domains, transfer_analytic_pcurve_carriers,
    unique_oriented_native_pcurve, NativePcurveCandidates,
};
#[allow(unused_imports)]
pub(super) use planes::{
    agreed_plane, agreed_plane_surface, agreed_topology_bound_plane, analytic_boundary_line,
    analytic_curve_plane, canonical_plane, envelope_reconciled_plane_candidate,
    frame_bound_outline_plane_candidate, held_coordinate_plane, is_axis_aligned,
    placed_plane_surfaces, placed_planes, plane_candidates, point_on_carrier, solve_carriers,
    topology_bound_line_plane, topology_bound_plane, valid_positive_nurbs_curve, BoundaryLine,
    PlaneCandidate, PlaneChart,
};
#[allow(unused_imports)]
pub(super) use vertices::{
    conic_conic_intersections, incident_analytic_vertex_domain, line_conic_intersections,
    line_line_intersection, model_points_agree, solved_topological_vertices,
};
