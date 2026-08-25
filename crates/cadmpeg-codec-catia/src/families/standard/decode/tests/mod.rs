pub(super) use crate::assemble::{
    attach_free_vertices, circle_parameter_range_from_surface_branch, ordered_range,
    rational_pcurve_arc,
};
pub(super) use crate::families::standard::decode::{
    analytic_surface_uv, associate_standard_freeform_e5_rolling_ball_jets,
    associate_standard_freeform_e5_surfaces, build_standard_edge_curve, circle_axis_from_endpoints,
    circular_range_choices_have_simple_selection, circular_ranges_are_nonoverlapping_or_coincident,
    combine_propagated_endpoint_pairs, corroborate_successor_endpoint_points,
    emit_standard_topology, include_native_endpoint_pairs, intersection_line_direction,
    invariant_face_carrier_bindings, merge_derived_endpoint_pair, merge_native_endpoint_evidence,
    merge_standard_edge_vertex_references, native_support_circle_param_range,
    owner_contains_face_bounds, owner_matches_a5_carrier, plane_intersection_line,
    point_on_standard_face, point_on_surface, resolve_standard_endpoint_pairs,
    resolve_standard_limit_curve_binding, retry_rejected_mesh_solution, same_cone_generator_pair,
    standard_analytic_curve_parameter_range, standard_circle_endpoint_candidates,
    standard_circle_param_range, standard_edge_identity_is_admitted,
    standard_endpoint_pair_supports_topology, standard_face_point_membership,
    standard_limit_curve_bindings, standard_limit_curve_point_parameter,
    standard_line_pair_solution_is_simple, standard_line_pair_solution_is_simple_cached,
    standard_native_support_endpoint_pair, standard_object_evidence_from_streams,
    standard_oriented_analytic_curve_parameter_range, standard_oriented_native_support_pcurves,
    standard_pcurve_geometry, standard_plane_normals_from_face_frames,
    standard_serialized_endpoint_pairs, standard_shared_boundary_group_domains,
    standard_shared_nurbs_boundary_pair_options, standard_spline_line,
    standard_successor_endpoint_pairs, standard_successor_endpoint_points,
    standard_surface_evidence, unique_native_identity_points, witness_arc_end, StandardEdgeSupport,
    StandardRollingBallSource, StandardSurfaceProcedure, CYLINDER_PLANE_CONIC_TOLERANCE,
    PERPENDICULAR_CYLINDER_CONIC_TOLERANCE, SPHERE_SECTION_ENDPOINT_TOLERANCE,
};

pub(super) use crate::families::b2::records::B2OwnerNumericTail;
pub(super) use crate::families::b5::graph::{B5Graph, B5Profile, B5Surface};
pub(super) use crate::test_support::{
    append_b5_record, append_e5_record, b5_closed_triangle_stream, e5_d8_rolling_ball_stream,
    e5_torus_stream, le_f64,
};

pub(super) use crate::families::standard::records::{
    StandardCurveGeometry, StandardCurveSupport, StandardFaceBounds, StandardSurfaceRecord,
    SurfacePrefix,
};

pub(super) use cadmpeg_ir::document::CadIr;
pub(super) use cadmpeg_ir::eval::{curve_point, pcurve_uv, surface_point};
pub(super) use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
    RollingBallJetDerivative, RollingBallJetSite, Surface, SurfaceGeometry,
};
pub(super) use cadmpeg_ir::ids::{FaceId, PointId, ShellId, SurfaceId, VertexId};
pub(super) use cadmpeg_ir::math::{Point2, Point3, Vector3};
pub(super) use cadmpeg_ir::topology::{Face, Point, Sense, Vertex};
pub(super) use cadmpeg_ir::units::Units;

pub(super) use cadmpeg_ir::AnnotationBuilder;
pub(super) use std::cell::Cell;
pub(super) use std::collections::BTreeMap;
pub(super) use std::collections::{HashMap, HashSet};

mod binding;
mod evidence;
mod transfer;
