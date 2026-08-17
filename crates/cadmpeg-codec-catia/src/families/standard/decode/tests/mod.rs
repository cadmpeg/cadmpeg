pub(super) use crate::assemble::{
    attach_free_vertices, circle_parameter_range_from_surface_branch, ordered_range,
    rational_pcurve_arc,
};
pub(super) use crate::families::standard::decode::{
    analytic_surface_uv, bind_ordered_standard_curve_branches,
    bind_ordered_standard_curve_branches_for_group, bind_standard_curve_branch_group,
    build_standard_edge_curve, circle_axis_from_endpoints,
    circular_ranges_are_nonoverlapping_or_coincident, combine_propagated_endpoint_pairs,
    corroborate_successor_endpoint_points, emit_standard_topology, include_native_endpoint_pairs,
    intersection_line_direction, merge_native_endpoint_evidence, native_support_circle_param_range,
    plane_intersection_line, point_on_standard_face, point_on_surface,
    resolve_standard_endpoint_pairs, resolve_standard_limit_curve_binding,
    retry_rejected_mesh_solution, same_cone_generator_pair, standard_circle_endpoint_candidates,
    standard_circle_param_range, standard_curve_branch_assignment_is_ranked,
    standard_curve_branch_candidates_after_partial_assignment, standard_curve_branch_groups,
    standard_endpoint_pair_supports_topology, standard_limit_curve_bindings,
    standard_limit_curve_point_parameter, standard_line_pair_solution_is_simple,
    standard_native_support_endpoint_pair, standard_object_evidence_from_streams,
    standard_pcurve_geometry, standard_plane_normals_from_face_frames, standard_spline_line,
    standard_successor_endpoint_pairs, standard_successor_endpoint_points,
    standard_surface_evidence, unique_native_identity_points, witness_arc_end, StandardEdgeSupport,
    StandardSurfaceProcedure,
};

pub(super) use crate::families::b5::graph::{B5Graph, B5Profile, B5Surface};
pub(super) use crate::test_support::{append_b5_record, b5_closed_triangle_stream, le_f64};

pub(super) use crate::families::standard::records::{
    StandardCurveGeometry, StandardCurveSupport, StandardFaceBounds, StandardSurfaceRecord,
    SurfacePrefix,
};

pub(super) use cadmpeg_ir::document::CadIr;
pub(super) use cadmpeg_ir::eval::{pcurve_uv, surface_point};
pub(super) use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
    RollingBallJetDerivative, RollingBallJetSite, Surface, SurfaceGeometry,
};
pub(super) use cadmpeg_ir::ids::{FaceId, PointId, ShellId, SurfaceId, VertexId};
pub(super) use cadmpeg_ir::math::{Point2, Point3, Vector3};
pub(super) use cadmpeg_ir::topology::{Face, Point, Sense, Vertex};
pub(super) use cadmpeg_ir::units::Units;

pub(super) use cadmpeg_core::decode::WorkBudget;
pub(super) use cadmpeg_ir::AnnotationBuilder;
pub(super) use std::cell::Cell;
pub(super) use std::collections::BTreeMap;
pub(super) use std::collections::{HashMap, HashSet};

mod binding;
mod evidence;
mod transfer;
