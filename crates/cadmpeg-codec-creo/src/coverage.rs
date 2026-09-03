// SPDX-License-Identifier: Apache-2.0
//! Statically declared decode-coverage measures.

use cadmpeg_ir::{CoverageKey, HexByteCoverageKey, IndexedCoverageKey};

pub(crate) const VISIBLE_CURVE_TYPE_ROW_COUNT: HexByteCoverageKey =
    HexByteCoverageKey::new("visible_curve_type_", "_row_count");
pub(crate) const TRANSFERRED_VISIBLE_CURVE_TYPE_ROW_COUNT: HexByteCoverageKey =
    HexByteCoverageKey::new("transferred_visible_curve_type_", "_row_count");
pub(crate) const RETAINED_UNKNOWN_VISIBLE_CURVE_TYPE_ROW_COUNT: HexByteCoverageKey =
    HexByteCoverageKey::new("retained_unknown_visible_curve_type_", "_row_count");
pub(crate) const TRANSFERRED_NATIVE_FEATURE_SKAMP_TYPE_CONSTRAINT_COUNT: IndexedCoverageKey =
    IndexedCoverageKey::decimal(
        "transferred_native_feature_skamp_type_",
        "_constraint_count",
    );
pub(crate) const ACTIVE_NATIVE_FEATURE_SKAMP_TYPE_CONSTRAINT_COUNT: IndexedCoverageKey =
    IndexedCoverageKey::decimal("active_native_feature_skamp_type_", "_constraint_count");
pub(crate) const TRANSFERRED_NATIVE_FEATURE_RELATION_TYPE_CONSTRAINT_COUNT: IndexedCoverageKey =
    IndexedCoverageKey::decimal(
        "transferred_native_feature_relation_type_",
        "_constraint_count",
    );
pub(crate) const ACTIVE_NATIVE_FEATURE_RELATION_TYPE_CONSTRAINT_COUNT: IndexedCoverageKey =
    IndexedCoverageKey::decimal("active_native_feature_relation_type_", "_constraint_count");

#[derive(Clone, Copy)]
pub(crate) struct SurfaceFamilyCoverageKeys {
    pub(crate) visible: CoverageKey,
    pub(crate) transferred: CoverageKey,
    pub(crate) untransferred: CoverageKey,
    pub(crate) retained_unknown: CoverageKey,
}

pub(crate) const fn surface_family_keys(
    kind: crate::surface::SurfaceKind,
) -> SurfaceFamilyCoverageKeys {
    match kind {
        crate::surface::SurfaceKind::Plane => SurfaceFamilyCoverageKeys {
            visible: VISIBLE_PLANE_SURFACE_ROW_COUNT,
            transferred: TRANSFERRED_VISIBLE_PLANE_SURFACE_ROW_COUNT,
            untransferred: UNTRANSFERRED_VISIBLE_PLANE_SURFACE_ROW_COUNT,
            retained_unknown: RETAINED_UNKNOWN_VISIBLE_PLANE_SURFACE_ROW_COUNT,
        },
        crate::surface::SurfaceKind::Cylinder => SurfaceFamilyCoverageKeys {
            visible: VISIBLE_CYLINDER_SURFACE_ROW_COUNT,
            transferred: TRANSFERRED_VISIBLE_CYLINDER_SURFACE_ROW_COUNT,
            untransferred: UNTRANSFERRED_VISIBLE_CYLINDER_SURFACE_ROW_COUNT,
            retained_unknown: RETAINED_UNKNOWN_VISIBLE_CYLINDER_SURFACE_ROW_COUNT,
        },
        crate::surface::SurfaceKind::Cone => SurfaceFamilyCoverageKeys {
            visible: VISIBLE_CONE_SURFACE_ROW_COUNT,
            transferred: TRANSFERRED_VISIBLE_CONE_SURFACE_ROW_COUNT,
            untransferred: UNTRANSFERRED_VISIBLE_CONE_SURFACE_ROW_COUNT,
            retained_unknown: RETAINED_UNKNOWN_VISIBLE_CONE_SURFACE_ROW_COUNT,
        },
        crate::surface::SurfaceKind::TorusOrSphere => SurfaceFamilyCoverageKeys {
            visible: VISIBLE_TORUS_OR_SPHERE_SURFACE_ROW_COUNT,
            transferred: TRANSFERRED_VISIBLE_TORUS_OR_SPHERE_SURFACE_ROW_COUNT,
            untransferred: UNTRANSFERRED_VISIBLE_TORUS_OR_SPHERE_SURFACE_ROW_COUNT,
            retained_unknown: RETAINED_UNKNOWN_VISIBLE_TORUS_OR_SPHERE_SURFACE_ROW_COUNT,
        },
        crate::surface::SurfaceKind::Spline => SurfaceFamilyCoverageKeys {
            visible: VISIBLE_SPLINE_SURFACE_ROW_COUNT,
            transferred: TRANSFERRED_VISIBLE_SPLINE_SURFACE_ROW_COUNT,
            untransferred: UNTRANSFERRED_VISIBLE_SPLINE_SURFACE_ROW_COUNT,
            retained_unknown: RETAINED_UNKNOWN_VISIBLE_SPLINE_SURFACE_ROW_COUNT,
        },
        crate::surface::SurfaceKind::Fillet => SurfaceFamilyCoverageKeys {
            visible: VISIBLE_FILLET_SURFACE_ROW_COUNT,
            transferred: TRANSFERRED_VISIBLE_FILLET_SURFACE_ROW_COUNT,
            untransferred: UNTRANSFERRED_VISIBLE_FILLET_SURFACE_ROW_COUNT,
            retained_unknown: RETAINED_UNKNOWN_VISIBLE_FILLET_SURFACE_ROW_COUNT,
        },
        crate::surface::SurfaceKind::Extrusion => SurfaceFamilyCoverageKeys {
            visible: VISIBLE_EXTRUSION_SURFACE_ROW_COUNT,
            transferred: TRANSFERRED_VISIBLE_EXTRUSION_SURFACE_ROW_COUNT,
            untransferred: UNTRANSFERRED_VISIBLE_EXTRUSION_SURFACE_ROW_COUNT,
            retained_unknown: RETAINED_UNKNOWN_VISIBLE_EXTRUSION_SURFACE_ROW_COUNT,
        },
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SketchSegmentCoverageKeys {
    pub(crate) decoded: CoverageKey,
    pub(crate) resolved: CoverageKey,
    pub(crate) unresolved: CoverageKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchSegmentFamily {
    Line,
    Arc,
    Point,
    Circle,
    CenteredLine,
    ReferenceLine,
    BoundedCurve,
    Conic,
    Opaque,
}

impl SketchSegmentFamily {
    pub(crate) const ALL: [Self; 9] = [
        Self::Line,
        Self::Arc,
        Self::Point,
        Self::Circle,
        Self::CenteredLine,
        Self::ReferenceLine,
        Self::BoundedCurve,
        Self::Conic,
        Self::Opaque,
    ];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Line => 0,
            Self::Arc => 1,
            Self::Point => 2,
            Self::Circle => 3,
            Self::CenteredLine => 4,
            Self::ReferenceLine => 5,
            Self::BoundedCurve => 6,
            Self::Conic => 7,
            Self::Opaque => 8,
        }
    }
}

pub(crate) const fn sketch_segment_keys(family: SketchSegmentFamily) -> SketchSegmentCoverageKeys {
    match family {
        SketchSegmentFamily::Line => SketchSegmentCoverageKeys {
            decoded: DECODED_FEATURE_LINE_SEGMENT_COUNT,
            resolved: RESOLVED_FEATURE_LINE_SEGMENT_GEOMETRY_COUNT,
            unresolved: UNRESOLVED_FEATURE_LINE_SEGMENT_GEOMETRY_COUNT,
        },
        SketchSegmentFamily::Arc => SketchSegmentCoverageKeys {
            decoded: DECODED_FEATURE_ARC_SEGMENT_COUNT,
            resolved: RESOLVED_FEATURE_ARC_SEGMENT_GEOMETRY_COUNT,
            unresolved: UNRESOLVED_FEATURE_ARC_SEGMENT_GEOMETRY_COUNT,
        },
        SketchSegmentFamily::Point => SketchSegmentCoverageKeys {
            decoded: DECODED_FEATURE_POINT_SEGMENT_COUNT,
            resolved: RESOLVED_FEATURE_POINT_SEGMENT_GEOMETRY_COUNT,
            unresolved: UNRESOLVED_FEATURE_POINT_SEGMENT_GEOMETRY_COUNT,
        },
        SketchSegmentFamily::Circle => SketchSegmentCoverageKeys {
            decoded: DECODED_FEATURE_CIRCLE_SEGMENT_COUNT,
            resolved: RESOLVED_FEATURE_CIRCLE_SEGMENT_GEOMETRY_COUNT,
            unresolved: UNRESOLVED_FEATURE_CIRCLE_SEGMENT_GEOMETRY_COUNT,
        },
        SketchSegmentFamily::CenteredLine => SketchSegmentCoverageKeys {
            decoded: DECODED_FEATURE_CENTERED_LINE_SEGMENT_COUNT,
            resolved: RESOLVED_FEATURE_CENTERED_LINE_SEGMENT_GEOMETRY_COUNT,
            unresolved: UNRESOLVED_FEATURE_CENTERED_LINE_SEGMENT_GEOMETRY_COUNT,
        },
        SketchSegmentFamily::ReferenceLine => SketchSegmentCoverageKeys {
            decoded: DECODED_FEATURE_REFERENCE_LINE_SEGMENT_COUNT,
            resolved: RESOLVED_FEATURE_REFERENCE_LINE_SEGMENT_GEOMETRY_COUNT,
            unresolved: UNRESOLVED_FEATURE_REFERENCE_LINE_SEGMENT_GEOMETRY_COUNT,
        },
        SketchSegmentFamily::BoundedCurve => SketchSegmentCoverageKeys {
            decoded: DECODED_FEATURE_BOUNDED_CURVE_SEGMENT_COUNT,
            resolved: RESOLVED_FEATURE_BOUNDED_CURVE_SEGMENT_GEOMETRY_COUNT,
            unresolved: UNRESOLVED_FEATURE_BOUNDED_CURVE_SEGMENT_GEOMETRY_COUNT,
        },
        SketchSegmentFamily::Conic => SketchSegmentCoverageKeys {
            decoded: DECODED_FEATURE_CONIC_SEGMENT_COUNT,
            resolved: RESOLVED_FEATURE_CONIC_SEGMENT_GEOMETRY_COUNT,
            unresolved: UNRESOLVED_FEATURE_CONIC_SEGMENT_GEOMETRY_COUNT,
        },
        SketchSegmentFamily::Opaque => SketchSegmentCoverageKeys {
            decoded: DECODED_FEATURE_OPAQUE_SEGMENT_COUNT,
            resolved: RESOLVED_FEATURE_OPAQUE_SEGMENT_GEOMETRY_COUNT,
            unresolved: UNRESOLVED_FEATURE_OPAQUE_SEGMENT_GEOMETRY_COUNT,
        },
    }
}

pub(crate) const ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT: CoverageKey =
    CoverageKey::new("active_curve_expression_assignment_count");
pub(crate) const CONDITIONAL_CURVE_EXPRESSION_ASSIGNMENT_COUNT: CoverageKey =
    CoverageKey::new("conditional_curve_expression_assignment_count");
pub(crate) const DECODED_ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_active_curve_expression_assignment_count");
pub(crate) const DECODED_ACTIVE_CURVE_EXPRESSION_FUNCTION_WRITE_ASSIGNMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_active_curve_expression_function_write_assignment_count");
pub(crate) const DECODED_ACTIVE_CURVE_EXPRESSION_SCOPED_SYMBOL_ASSIGNMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_active_curve_expression_scoped_symbol_assignment_count");
pub(crate) const DECODED_ACTIVE_CURVE_EXPRESSION_SIMULTANEOUS_EQUATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_active_curve_expression_simultaneous_equation_count");
pub(crate) const DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_ASSIGNMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_active_curve_expression_solve_assignment_count");
pub(crate) const DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_BLOCK_COUNT: CoverageKey =
    CoverageKey::new("decoded_active_curve_expression_solve_block_count");
pub(crate) const DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_VARIABLE_COUNT: CoverageKey =
    CoverageKey::new("decoded_active_curve_expression_solve_variable_count");
pub(crate) const DECODED_ACTIVE_CURVE_EXPRESSION_SYSTEM_SYMBOL_ASSIGNMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_active_curve_expression_system_symbol_assignment_count");
pub(crate) const DECODED_ACTIVE_CURVE_EXPRESSION_TABLE_CELL_ASSIGNMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_active_curve_expression_table_cell_assignment_count");
pub(crate) const DECODED_CONFIGURATION_DRIVER_TABLE_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_configuration_driver_table_reference_count");
pub(crate) const DECODED_LEGACY_CONFIGURATION_DRIVER_TABLE_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_configuration_driver_table_count");
pub(crate) const DECODED_LEGACY_CONFIGURATION_ITEM_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_configuration_item_count");
pub(crate) const DECODED_LEGACY_CONFIGURATION_INSTANCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_configuration_instance_count");
pub(crate) const DECODED_FEATURE_BOUNDED_CURVE_SEGMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_bounded_curve_segment_count");
pub(crate) const DECODED_FEATURE_CENTERED_LINE_SEGMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_centered_line_segment_count");
pub(crate) const DECODED_FEATURE_CIRCLE_SEGMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_circle_segment_count");
pub(crate) const DECODED_FEATURE_DIMENSION_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_dimension_count");
pub(crate) const DECODED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_dimension_driven_coordinate_variable_count");
pub(crate) const DECODED_FEATURE_DIMENSION_DRIVEN_GUESS_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_dimension_driven_guess_count");
pub(crate) const DECODED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_dimension_driven_other_variable_count");
pub(crate) const DECODED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_dimension_driven_variable_count");
pub(crate) const DECODED_FEATURE_LINE_SEGMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_line_segment_count");
pub(crate) const DECODED_FEATURE_LOOP_HISTORY_ENTRY_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_loop_history_entry_count");
pub(crate) const DECODED_FEATURE_POINT_SEGMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_point_segment_count");
pub(crate) const DECODED_FEATURE_REFERENCE_LINE_SEGMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_reference_line_segment_count");
pub(crate) const DECODED_FEATURE_RELATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_relation_count");
pub(crate) const DECODED_FEATURE_RELATION_TRIPLE_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_relation_triple_count");
pub(crate) const DECODED_FEATURE_SEGMENT_ROW_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_segment_row_count");
pub(crate) const DECODED_FEATURE_SKAMP_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_skamp_count");
pub(crate) const DECODED_FEATURE_SOLVER_VARIABLE_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_solver_variable_count");
pub(crate) const DECODED_FEATURE_SURFACE_REPLAY_ASSOCIATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_surface_replay_association_count");
pub(crate) const DECODED_TYPE26_FIVE_COORDINATE_ENVELOPE_COUNT: CoverageKey =
    CoverageKey::new("decoded_type26_five_coordinate_envelope_count");
pub(crate) const DECODED_TYPE26_REPLAYED_MINOR_RADIUS_COUNT: CoverageKey =
    CoverageKey::new("decoded_type26_replayed_minor_radius_count");
pub(crate) const DECODED_TYPE26_SPLIT_COORDINATE_ENVELOPE_COUNT: CoverageKey =
    CoverageKey::new("decoded_type26_split_coordinate_envelope_count");
pub(crate) const EVALUATED_ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT: CoverageKey =
    CoverageKey::new("evaluated_active_curve_expression_assignment_count");
pub(crate) const EVALUATED_ACTIVE_CURVE_EXPRESSION_SOLVE_BLOCK_COUNT: CoverageKey =
    CoverageKey::new("evaluated_active_curve_expression_solve_block_count");
pub(crate) const EVALUATED_ACTIVE_CURVE_EXPRESSION_SOLVE_VARIABLE_COUNT: CoverageKey =
    CoverageKey::new("evaluated_active_curve_expression_solve_variable_count");
pub(crate) const INACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT: CoverageKey =
    CoverageKey::new("inactive_curve_expression_assignment_count");
pub(crate) const MALFORMED_FEATURE_RELATION_TABLE_COUNT: CoverageKey =
    CoverageKey::new("malformed_feature_relation_table_count");
pub(crate) const MISSING_FEATURE_RELATION_ROW_COUNT: CoverageKey =
    CoverageKey::new("missing_feature_relation_row_count");
pub(crate) const MISSING_FEATURE_RELATION_TRIPLE_ROW_COUNT: CoverageKey =
    CoverageKey::new("missing_feature_relation_triple_row_count");
pub(crate) const MISSING_FEATURE_SEGMENT_ROW_COUNT: CoverageKey =
    CoverageKey::new("missing_feature_segment_row_count");
pub(crate) const MISSING_FEATURE_SKAMP_ROW_COUNT: CoverageKey =
    CoverageKey::new("missing_feature_skamp_row_count");
pub(crate) const MISSING_FEATURE_SOLVER_VARIABLE_COUNT: CoverageKey =
    CoverageKey::new("missing_feature_solver_variable_count");
pub(crate) const PROHIBITED_ACTIVE_CURVE_EXPRESSION_KIND_COUNT: CoverageKey =
    CoverageKey::new("prohibited_active_curve_expression_kind_count");
pub(crate) const PROHIBITED_ACTIVE_CURVE_EXPRESSION_RECORD_COUNT: CoverageKey =
    CoverageKey::new("prohibited_active_curve_expression_record_count");
pub(crate) const RESOLVED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT: CoverageKey =
    CoverageKey::new("resolved_feature_dimension_driven_coordinate_variable_count");
pub(crate) const RESOLVED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT: CoverageKey =
    CoverageKey::new("resolved_feature_dimension_driven_other_variable_count");
pub(crate) const RESOLVED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT: CoverageKey =
    CoverageKey::new("resolved_feature_dimension_driven_variable_count");
pub(crate) const RESOLVED_FEATURE_DIMENSION_VALUE_COUNT: CoverageKey =
    CoverageKey::new("resolved_feature_dimension_value_count");
pub(crate) const RESOLVED_FEATURE_LINE_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("resolved_feature_line_segment_geometry_count");
pub(crate) const RESOLVED_FEATURE_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("resolved_feature_segment_geometry_count");
pub(crate) const RETAINED_UNKNOWN_VISIBLE_CURVE_ROW_COUNT: CoverageKey =
    CoverageKey::new("retained_unknown_visible_curve_row_count");
pub(crate) const RETAINED_UNKNOWN_VISIBLE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("retained_unknown_visible_surface_row_count");
pub(crate) const TRANSFERRED_CHAMFER_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_chamfer_feature_count");
pub(crate) const TRANSFERRED_CONFIGURATION_DRIVER_TABLE_COUNT: CoverageKey =
    CoverageKey::new("transferred_configuration_driver_table_count");
pub(crate) const TRANSFERRED_CURVE_EXPRESSION_PARAMETER_COUNT: CoverageKey =
    CoverageKey::new("transferred_curve_expression_parameter_count");
pub(crate) const TRANSFERRED_DRAFT_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_draft_feature_count");
pub(crate) const TRANSFERRED_EXPLICITLY_UNRESOLVED_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_explicitly_unresolved_feature_count");
pub(crate) const TRANSFERRED_EXTRUDE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_extrude_feature_count");
pub(crate) const TRANSFERRED_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_count");
pub(crate) const TRANSFERRED_FEATURE_RESULT_EDGE_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_result_edge_count");
pub(crate) const TRANSFERRED_FEATURE_RESULT_TOPOLOGY_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_result_topology_count");
pub(crate) const TRANSFERRED_FEATURE_DIMENSION_PARAMETER_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_dimension_parameter_count");
pub(crate) const TRANSFERRED_FILLET_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_fillet_feature_count");
pub(crate) const TRANSFERRED_FIRST_INSTANCE_PROTOTYPE_SURFACE_COUNT: CoverageKey =
    CoverageKey::new("transferred_first_instance_prototype_surface_count");
pub(crate) const TRANSFERRED_LEGACY_ASCII_SURFACE_CARRIER_COUNT: CoverageKey =
    CoverageKey::new("transferred_legacy_ascii_surface_carrier_count");
pub(crate) const TRANSFERRED_GEOMETRY_GENERATOR_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_geometry_generator_feature_count");
pub(crate) const TRANSFERRED_HOLE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_hole_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_CHAMFER_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_chamfer_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_DRAFT_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_draft_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_EXTRUDE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_extrude_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_EXTRUDE_START_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_extrude_start_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_EXTRUDE_TERMINATION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_extrude_termination_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_FILLET_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_fillet_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_HOLE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_hole_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_KNIT_SURFACE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_knit_surface_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_OTHER_CONSTRUCTION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_other_construction_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_PATTERN_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_pattern_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_REVOLVE_EXTENT_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_revolve_extent_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_REVOLVE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_revolve_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_SURFACE_OPERATION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_surface_operation_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_SWEEP_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_sweep_feature_count");
pub(crate) const TRANSFERRED_KNIT_SURFACE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_knit_surface_feature_count");
pub(crate) const TRANSFERRED_NATIVE_AXIS_HELIX_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_axis_helix_feature_count");
pub(crate) const TRANSFERRED_NATIVE_CHAMFER_EDGE_SELECTION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_chamfer_edge_selection_feature_count");
pub(crate) const TRANSFERRED_NATIVE_DRAFT_FACE_SELECTION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_draft_face_selection_feature_count");
pub(crate) const TRANSFERRED_NATIVE_DRAFT_NEUTRAL_PLANE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_draft_neutral_plane_feature_count");
pub(crate) const TRANSFERRED_NATIVE_EXTRUDE_PROFILE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_extrude_profile_feature_count");
pub(crate) const TRANSFERRED_NATIVE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_feature_count");
pub(crate) const TRANSFERRED_NATIVE_FILLET_EDGE_SELECTION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_fillet_edge_selection_feature_count");
pub(crate) const TRANSFERRED_NATIVE_HOLE_FACE_SELECTION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_hole_face_selection_feature_count");
pub(crate) const TRANSFERRED_NATIVE_HOLE_PROFILE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_hole_profile_feature_count");
pub(crate) const TRANSFERRED_NATIVE_REVOLVE_PROFILE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_revolve_profile_feature_count");
pub(crate) const TRANSFERRED_PAIRED_ENVELOPE_SPHERE_COUNT: CoverageKey =
    CoverageKey::new("transferred_paired_envelope_sphere_count");
pub(crate) const TRANSFERRED_PATTERN_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_pattern_feature_count");
pub(crate) const TRANSFERRED_POSITIONAL_CYLINDER_COUNT: CoverageKey =
    CoverageKey::new("transferred_positional_cylinder_count");
pub(crate) const TRANSFERRED_POSITIONAL_TORUS_COUNT: CoverageKey =
    CoverageKey::new("transferred_positional_torus_count");
pub(crate) const TRANSFERRED_REFERENCE_ELLIPSE_COUNT: CoverageKey =
    CoverageKey::new("transferred_reference_ellipse_count");
pub(crate) const TRANSFERRED_REVOLVE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_revolve_feature_count");
pub(crate) const TRANSFERRED_TYPED_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_typed_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_BOUNDARY_SURFACE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_boundary_surface_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_CHAMFER_EDGE_SELECTION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_chamfer_edge_selection_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_CHAMFER_SPEC_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_chamfer_spec_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_DATUM_COORDINATE_SYSTEM_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_datum_coordinate_system_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_DATUM_PLANE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_datum_plane_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_EXTRUDE_BOOLEAN_OPERATION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_extrude_boolean_operation_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_EXTRUDE_PROFILE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_extrude_profile_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_FILLET_EDGE_SELECTION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_fillet_edge_selection_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_FILLET_RADIUS_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_fillet_radius_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_FILLET_RADIUS_WITH_GENERATED_SURFACE_FEATURE_COUNT:
    CoverageKey =
    CoverageKey::new("transferred_unresolved_fillet_radius_with_generated_surface_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_FILLET_RADIUS_WITHOUT_GENERATED_SURFACE_FEATURE_COUNT:
    CoverageKey = CoverageKey::new(
    "transferred_unresolved_fillet_radius_without_generated_surface_feature_count",
);
pub(crate) const TRANSFERRED_UNRESOLVED_HOLE_FACE_SELECTION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_hole_face_selection_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_HOLE_PROFILE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_hole_profile_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_KNIT_SURFACE_FACES_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_knit_surface_faces_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_KNIT_SURFACE_MERGE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_knit_surface_merge_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_KNIT_SURFACE_SOLID_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_knit_surface_solid_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_PATTERN_SEED_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_pattern_seed_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_PATTERN_TRANSFORM_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_pattern_transform_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_REVOLVE_AXIS_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_revolve_axis_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_REVOLVE_BOOLEAN_OPERATION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_revolve_boolean_operation_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_REVOLVE_PROFILE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_revolve_profile_feature_count");
pub(crate) const TRANSFERRED_VARIABLE_RADIUS_FILLET_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_variable_radius_fillet_feature_count");
pub(crate) const TRANSFERRED_VISIBLE_PLANE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("transferred_visible_plane_surface_row_count");
pub(crate) const UNRESOLVED_ACTIVE_CURVE_EXPRESSION_SOLVE_CONTROL_COUNT: CoverageKey =
    CoverageKey::new("unresolved_active_curve_expression_solve_control_count");
pub(crate) const UNRESOLVED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_dimension_driven_coordinate_variable_count");
pub(crate) const UNRESOLVED_FEATURE_DIMENSION_DRIVEN_GUESS_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_dimension_driven_guess_count");
pub(crate) const UNRESOLVED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_dimension_driven_other_variable_count");
pub(crate) const UNRESOLVED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_dimension_driven_variable_count");
pub(crate) const UNRESOLVED_FEATURE_DIMENSION_VALUE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_dimension_value_count");
pub(crate) const UNRESOLVED_FEATURE_LINE_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_line_segment_geometry_count");
pub(crate) const UNRESOLVED_FEATURE_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_segment_geometry_count");
pub(crate) const UNTRANSFERRED_VISIBLE_CURVE_ROW_COUNT: CoverageKey =
    CoverageKey::new("untransferred_visible_curve_row_count");
pub(crate) const UNTRANSFERRED_VISIBLE_CYLINDER_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("untransferred_visible_cylinder_surface_row_count");
pub(crate) const UNTRANSFERRED_VISIBLE_PLANE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("untransferred_visible_plane_surface_row_count");
pub(crate) const UNTRANSFERRED_VISIBLE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("untransferred_visible_surface_row_count");
pub(crate) const VISIBLE_PLANE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("visible_plane_surface_row_count");

pub(crate) const ACTIVE_FEATURE_EQUATION_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("active_feature_equation_constraint_count");
pub(crate) const ACTIVE_FEATURE_RELATION_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("active_feature_relation_constraint_count");
pub(crate) const ACTIVE_FEATURE_SKAMP_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("active_feature_skamp_constraint_count");
pub(crate) const ACTIVE_NATIVE_FEATURE_EQUATION_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("active_native_feature_equation_constraint_count");
pub(crate) const ACTIVE_NATIVE_FEATURE_RELATION_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("active_native_feature_relation_constraint_count");
pub(crate) const ACTIVE_NATIVE_FEATURE_SKAMP_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("active_native_feature_skamp_constraint_count");
pub(crate) const ACTIVE_TYPED_FEATURE_EQUATION_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("active_typed_feature_equation_constraint_count");
pub(crate) const ACTIVE_TYPED_FEATURE_RELATION_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("active_typed_feature_relation_constraint_count");
pub(crate) const ACTIVE_TYPED_FEATURE_SKAMP_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("active_typed_feature_skamp_constraint_count");
pub(crate) const AMBIGUOUS_VISIBLE_CURVE_ROW_COUNT: CoverageKey =
    CoverageKey::new("ambiguous_visible_curve_row_count");
pub(crate) const AMBIGUOUS_VISIBLE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("ambiguous_visible_surface_row_count");
pub(crate) const AXIAL_INTERVAL_CORNER_ENVELOPE_COUNT: CoverageKey =
    CoverageKey::new("axial_interval_corner_envelope_count");
pub(crate) const AXIAL_INTERVAL_CORNER_SOLVED_CARRIER_COUNT: CoverageKey =
    CoverageKey::new("axial_interval_corner_solved_carrier_count");
pub(crate) const BREP_ADMITTED_COMPONENT_COUNT: CoverageKey =
    CoverageKey::new("brep_admitted_component_count");
pub(crate) const BREP_ADMITTED_FACE_COUNT: CoverageKey =
    CoverageKey::new("brep_admitted_face_count");
pub(crate) const BREP_BODY_COUNT_MISMATCH_COUNT: CoverageKey =
    CoverageKey::new("brep_body_count_mismatch_count");
pub(crate) const BREP_BOUNDARY_CURVE_COUNT: CoverageKey =
    CoverageKey::new("brep_boundary_curve_count");
pub(crate) const BREP_BOUNDARY_CURVE_MISSING_INCIDENCE_COUNT: CoverageKey =
    CoverageKey::new("brep_boundary_curve_missing_incidence_count");
pub(crate) const BREP_BOUNDARY_CURVE_UNSOLVED_VERTEX_COUNT: CoverageKey =
    CoverageKey::new("brep_boundary_curve_unsolved_vertex_count");
pub(crate) const BREP_CANDIDATE_FACE_COUNT: CoverageKey =
    CoverageKey::new("brep_candidate_face_count");
pub(crate) const BREP_EMITTED_FACE_COUNT: CoverageKey = CoverageKey::new("brep_emitted_face_count");
pub(crate) const BREP_EMPTY_COMPONENT_COUNT: CoverageKey =
    CoverageKey::new("brep_empty_component_count");
pub(crate) const BREP_LEGACY_BODY_OWNERSHIP_AMBIGUOUS_COUNT: CoverageKey =
    CoverageKey::new("brep_legacy_body_ownership_ambiguous_count");
pub(crate) const BREP_LEGACY_NONVISIBLE_FACE_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("brep_legacy_nonvisible_face_reference_count");
pub(crate) const BREP_PCURVE_ACCEPTED_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_accepted_record_count");
pub(crate) const BREP_PCURVE_CARRIER_REJECTED_PATH_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_carrier_rejected_path_count");
pub(crate) const BREP_PCURVE_CARRIER_REJECTED_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_carrier_rejected_record_count");
pub(crate) const BREP_PCURVE_CARRIER_UNKNOWN_MISSING_CARRIER_PATH_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_carrier_unknown_missing_carrier_path_count");
pub(crate) const BREP_PCURVE_CARRIER_UNKNOWN_MISSING_SURFACE_PATH_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_carrier_unknown_missing_surface_path_count");
pub(crate) const BREP_PCURVE_CARRIER_UNKNOWN_PARALLEL_PLANE_PATH_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_carrier_unknown_parallel_plane_path_count");
pub(crate) const BREP_PCURVE_CARRIER_UNKNOWN_PATH_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_carrier_unknown_path_count");
pub(crate) const BREP_PCURVE_CARRIER_UNKNOWN_UNSUPPORTED_PAIR_PATH_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_carrier_unknown_unsupported_pair_path_count");
pub(crate) const BREP_PCURVE_CARRIER_UNKNOWN_UNSUPPORTED_PATH_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_carrier_unknown_unsupported_path_count");
pub(crate) const BREP_PCURVE_CARRIER_VALIDATED_PATH_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_carrier_validated_path_count");
pub(crate) const BREP_PCURVE_COMPLETE_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_complete_record_count");
pub(crate) const BREP_PCURVE_CONFLICTING_CURVE_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_conflicting_curve_count");
pub(crate) const BREP_PCURVE_INACTIVE_PATH_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_inactive_path_count");
pub(crate) const BREP_PCURVE_INACTIVE_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_inactive_record_count");
pub(crate) const BREP_PCURVE_INCONSISTENT_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_inconsistent_record_count");
pub(crate) const BREP_PCURVE_MAPPED_PATH_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_mapped_path_count");
pub(crate) const BREP_PCURVE_MISSING_SURFACE_PATH_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_missing_surface_path_count");
pub(crate) const BREP_PCURVE_PARTIAL_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_partial_record_count");
pub(crate) const BREP_PCURVE_PATH_COUNT: CoverageKey = CoverageKey::new("brep_pcurve_path_count");
pub(crate) const BREP_PCURVE_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_record_count");
pub(crate) const BREP_PCURVE_TOPOLOGY_MISMATCH_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_topology_mismatch_record_count");
pub(crate) const BREP_PCURVE_TWO_CHART_COMPLETE_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_two_chart_complete_record_count");
pub(crate) const BREP_PCURVE_TWO_CHART_MAPPED_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_two_chart_mapped_record_count");
pub(crate) const BREP_PCURVE_TWO_CHART_MISSING_SURFACE_PATH_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_two_chart_missing_surface_path_count");
pub(crate) const BREP_PCURVE_TWO_CHART_NO_SAMPLE_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_two_chart_no_sample_record_count");
pub(crate) const BREP_PCURVE_TWO_CHART_PARTIAL_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_two_chart_partial_record_count");
pub(crate) const BREP_PCURVE_TWO_CHART_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_two_chart_record_count");
pub(crate) const BREP_PCURVE_TWO_CHART_SURFACE_MISMATCH_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_two_chart_surface_mismatch_record_count");
pub(crate) const BREP_PCURVE_TWO_CHART_UNEVALUABLE_PATH_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_two_chart_unevaluable_path_count");
pub(crate) const BREP_PCURVE_TWO_CHART_UNMAPPED_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_two_chart_unmapped_record_count");
pub(crate) const BREP_PCURVE_UNEVALUABLE_PATH_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_unevaluable_path_count");
pub(crate) const BREP_PCURVE_UNMAPPED_RECORD_COUNT: CoverageKey =
    CoverageKey::new("brep_pcurve_unmapped_record_count");
pub(crate) const BREP_REJECTED_FACE_COUNT: CoverageKey =
    CoverageKey::new("brep_rejected_face_count");
pub(crate) const BREP_SELECTED_BODY_COUNT: CoverageKey =
    CoverageKey::new("brep_selected_body_count");
pub(crate) const BREP_VERTEX_ANALYTIC_DOMAIN_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_analytic_domain_count");
pub(crate) const BREP_VERTEX_CARRIER_AMBIGUOUS_CANDIDATE_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_carrier_ambiguous_candidate_count");
pub(crate) const BREP_VERTEX_CARRIER_INCIDENT_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_carrier_incident_count");
pub(crate) const BREP_VERTEX_CARRIER_NO_GEOMETRIC_CANDIDATE_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_carrier_no_geometric_candidate_count");
pub(crate) const BREP_VERTEX_CARRIER_NO_VALID_CANDIDATE_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_carrier_no_valid_candidate_count");
pub(crate) const BREP_VERTEX_CARRIER_PAIR_INTERSECTION_CANDIDATE_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_carrier_pair_intersection_candidate_count");
pub(crate) const BREP_VERTEX_CARRIER_POINT_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_carrier_point_count");
pub(crate) const BREP_VERTEX_CARRIER_TRIPLE_INTERSECTION_CANDIDATE_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_carrier_triple_intersection_candidate_count");
pub(crate) const BREP_VERTEX_CARRIER_VALID_INTERSECTION_CANDIDATE_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_carrier_valid_intersection_candidate_count");
pub(crate) const BREP_VERTEX_CARRIER_ZERO_CANDIDATE_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_carrier_zero_candidate_count");
pub(crate) const BREP_VERTEX_COMPLETE_PCURVE_ENDPOINT_EVIDENCE_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_complete_pcurve_endpoint_evidence_count");
pub(crate) const BREP_VERTEX_DIRECTED_ENDPOINT_ASSIGNMENT_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_directed_endpoint_assignment_count");
pub(crate) const BREP_VERTEX_DIRECTED_ENDPOINT_CONFLICT_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_directed_endpoint_conflict_count");
pub(crate) const BREP_VERTEX_NURBS_ENDPOINT_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_nurbs_endpoint_constraint_count");
pub(crate) const BREP_VERTEX_PCURVE_AMBIGUOUS_ENDPOINT_VERTEX_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_pcurve_ambiguous_endpoint_vertex_count");
pub(crate) const BREP_VERTEX_PCURVE_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_pcurve_constraint_count");
pub(crate) const BREP_VERTEX_PCURVE_ENDPOINT_EVIDENCE_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_pcurve_endpoint_evidence_count");
pub(crate) const BREP_VERTEX_PCURVE_FIXED_ENDPOINT_CONFLICT_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_pcurve_fixed_endpoint_conflict_count");
pub(crate) const BREP_VERTEX_SOLVED_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_solved_count");
pub(crate) const BREP_VERTEX_TOPOLOGICAL_COUNT: CoverageKey =
    CoverageKey::new("brep_vertex_topological_count");
pub(crate) const CONFLICTING_PRIMITIVE_TRIANGLE_STRIP_REPRESENTATION_COUNT: CoverageKey =
    CoverageKey::new("conflicting_primitive_triangle_strip_representation_count");
pub(crate) const DECODED_BOUND_PROTOTYPE_PCURVE_COUNT: CoverageKey =
    CoverageKey::new("decoded_bound_prototype_pcurve_count");
pub(crate) const DECODED_CROSS_SECTION_CURVE_PROTOTYPE_COUNT: CoverageKey =
    CoverageKey::new("decoded_cross_section_curve_prototype_count");
pub(crate) const DECODED_CROSS_SECTION_CURVE_ROW_COUNT: CoverageKey =
    CoverageKey::new("decoded_cross_section_curve_row_count");
pub(crate) const DECODED_CROSS_SECTION_OUTLINE_PLANE_COUNT: CoverageKey =
    CoverageKey::new("decoded_cross_section_outline_plane_count");
pub(crate) const DECODED_CROSS_SECTION_PLANE_ENVELOPE_COUNT: CoverageKey =
    CoverageKey::new("decoded_cross_section_plane_envelope_count");
pub(crate) const DECODED_CROSS_SECTION_PLANE_LOCAL_SYSTEM_COUNT: CoverageKey =
    CoverageKey::new("decoded_cross_section_plane_local_system_count");
pub(crate) const DECODED_CROSS_SECTION_SURFACE_PARAMETER_RECORD_COUNT: CoverageKey =
    CoverageKey::new("decoded_cross_section_surface_parameter_record_count");
pub(crate) const DECODED_CROSS_SECTION_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("decoded_cross_section_surface_row_count");
pub(crate) const DECODED_CURVE_EXPRESSION_RECORD_COUNT: CoverageKey =
    CoverageKey::new("decoded_curve_expression_record_count");
pub(crate) const DECODED_CURVE_PARAMETER_RECORD_COUNT: CoverageKey =
    CoverageKey::new("decoded_curve_parameter_record_count");
pub(crate) const DECODED_CURVE_PROTOTYPE_COUNT: CoverageKey =
    CoverageKey::new("decoded_curve_prototype_count");
pub(crate) const DECODED_CURVE_PROTOTYPE_TOPOLOGY_COUNT: CoverageKey =
    CoverageKey::new("decoded_curve_prototype_topology_count");
pub(crate) const DECODED_CURVE_TOPOLOGY_ROW_COUNT: CoverageKey =
    CoverageKey::new("decoded_curve_topology_row_count");
pub(crate) const DECODED_DATUM_PLANE_COUNT: CoverageKey =
    CoverageKey::new("decoded_datum_plane_count");
pub(crate) const DECODED_FACE_COMPONENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_face_component_count");
pub(crate) const DECODED_FC05_CIRCLE_COUNT: CoverageKey =
    CoverageKey::new("decoded_fc05_circle_count");
pub(crate) const DECODED_FC05_CYLINDER_CAP_PAIR_COUNT: CoverageKey =
    CoverageKey::new("decoded_fc05_cylinder_cap_pair_count");
pub(crate) const DECODED_FC_CURVE_COORDINATE_RECORD_COUNT: CoverageKey =
    CoverageKey::new("decoded_fc_curve_coordinate_record_count");
pub(crate) const DECODED_FEATURE_AFFECTED_ID_ARRAY_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_affected_id_array_count");
pub(crate) const DECODED_FEATURE_CHOICE_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_choice_count");
pub(crate) const DECODED_FEATURE_CHOICE_FIELD_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_choice_field_count");
pub(crate) const DECODED_FEATURE_CONIC_SEGMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_conic_segment_count");
pub(crate) const DECODED_FEATURE_COUNT: CoverageKey = CoverageKey::new("decoded_feature_count");
pub(crate) const DECODED_FEATURE_DEFINITION_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_definition_count");
pub(crate) const DECODED_FEATURE_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_entity_count");
pub(crate) const DECODED_FEATURE_ENTITY_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_entity_reference_count");
pub(crate) const DECODED_FEATURE_ENTITY_TABLE_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_entity_table_count");
pub(crate) const DECODED_FEATURE_EQUATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_equation_count");
pub(crate) const DECODED_FEATURE_EQUATION_TABLE_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_equation_table_count");
pub(crate) const DECODED_FEATURE_GEOMETRY_TABLE_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_geometry_table_count");
pub(crate) const DECODED_FEATURE_LOOP_RESTORE_DIRECTION_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_loop_restore_direction_count");
pub(crate) const DECODED_FEATURE_OPAQUE_SEGMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_opaque_segment_count");
pub(crate) const DECODED_FEATURE_OPERATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_operation_count");
pub(crate) const DECODED_FEATURE_OPERATION_STATE_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_operation_state_count");
pub(crate) const DECODED_FEATURE_ORDER_ENTRY_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_order_entry_count");
pub(crate) const DECODED_FEATURE_OUTLINE_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_outline_count");
pub(crate) const DECODED_FEATURE_PLACEMENT_INSTRUCTION_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_placement_instruction_count");
pub(crate) const DECODED_FEATURE_REPLAY_AFFECTED_ID_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_replay_affected_id_count");
pub(crate) const DECODED_FEATURE_REVOLUTION_EXTENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_revolution_extent_count");
pub(crate) const DECODED_FEATURE_ROW_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_row_count");
pub(crate) const DECODED_FEATURE_SAVED_CONIC_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_saved_conic_count");
pub(crate) const DECODED_FEATURE_SAVED_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_saved_entity_count");
pub(crate) const DECODED_FEATURE_SECTION_POINT_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_section_point_count");
pub(crate) const DECODED_FEATURE_SECTION_TRANSFORM_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_section_transform_count");
pub(crate) const DECODED_FEATURE_TRIM_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_trim_entity_count");
pub(crate) const DECODED_FEATURE_TRIM_VERTEX_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_trim_vertex_count");
pub(crate) const DECODED_HALF_EDGE_COUNT: CoverageKey = CoverageKey::new("decoded_half_edge_count");
pub(crate) const DECODED_LEGACY_INTEGER_ARRAY_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_integer_array_count");
pub(crate) const DECODED_LEGACY_INTEGER_ELEMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_integer_element_count");
pub(crate) const DECODED_LEGACY_INTEGER_SCALAR_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_integer_scalar_count");
pub(crate) const DECODED_LEGACY_OBJECT_ARRAY_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_object_array_count");
pub(crate) const DECODED_LEGACY_OBJECT_ARROW_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_object_arrow_count");
pub(crate) const DECODED_LEGACY_OBJECT_INLINE_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_object_inline_count");
pub(crate) const DECODED_LEGACY_OBJECT_NULL_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_object_null_count");
pub(crate) const DECODED_LEGACY_PRINCIPAL_UNIT_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_principal_unit_count");
pub(crate) const DECODED_LEGACY_REAL_ARRAY_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_real_array_count");
pub(crate) const DECODED_LEGACY_REAL_ELEMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_real_element_count");
pub(crate) const DECODED_LEGACY_REAL_SCALAR_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_real_scalar_count");
pub(crate) const DECODED_LEGACY_STRING_ARRAY_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_string_array_count");
pub(crate) const DECODED_LEGACY_STRING_ELEMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_string_element_count");
pub(crate) const DECODED_LEGACY_STRING_SCALAR_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_string_scalar_count");
pub(crate) const DECODED_LEGACY_TORUS_OR_SPHERE_CARRIER_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_torus_or_sphere_carrier_count");
pub(crate) const DECODED_LOOP_COUNT: CoverageKey = CoverageKey::new("decoded_loop_count");
pub(crate) const DECODED_NAMED_SURFACE_PROTOTYPE_COUNT: CoverageKey =
    CoverageKey::new("decoded_named_surface_prototype_count");
pub(crate) const DECODED_OUTLINE_PLANE_COUNT: CoverageKey =
    CoverageKey::new("decoded_outline_plane_count");
pub(crate) const DECODED_PCURVE_COUNT: CoverageKey = CoverageKey::new("decoded_pcurve_count");
pub(crate) const DECODED_PLANE_ENVELOPE_COUNT: CoverageKey =
    CoverageKey::new("decoded_plane_envelope_count");
pub(crate) const DECODED_PLANE_LOCAL_SYSTEM_COUNT: CoverageKey =
    CoverageKey::new("decoded_plane_local_system_count");
pub(crate) const DECODED_POSITIONAL_EXTRUSION_DIRECTION_COUNT: CoverageKey =
    CoverageKey::new("decoded_positional_extrusion_direction_count");
pub(crate) const DECODED_POSITIONAL_FRAME_PLANE_COUNT: CoverageKey =
    CoverageKey::new("decoded_positional_frame_plane_count");
pub(crate) const DECODED_PRIMITIVE_TRIANGLE_STRIP_COUNT: CoverageKey =
    CoverageKey::new("decoded_primitive_triangle_strip_count");
pub(crate) const DECODED_PROTOTYPE_PCURVE_COUNT: CoverageKey =
    CoverageKey::new("decoded_prototype_pcurve_count");
pub(crate) const DECODED_REFERENCE_CIRCLE_COUNT: CoverageKey =
    CoverageKey::new("decoded_reference_circle_count");
pub(crate) const DECODED_REFERENCE_CONIC_COUNT: CoverageKey =
    CoverageKey::new("decoded_reference_conic_count");
pub(crate) const DECODED_REFERENCE_LINE_COUNT: CoverageKey =
    CoverageKey::new("decoded_reference_line_count");
pub(crate) const DECODED_SURFACE_MERGE_REPLAY_AFFECTED_ID_COUNT: CoverageKey =
    CoverageKey::new("decoded_surface_merge_replay_affected_id_count");
pub(crate) const DECODED_SURFACE_PARAMETER_RECORD_COUNT: CoverageKey =
    CoverageKey::new("decoded_surface_parameter_record_count");
pub(crate) const DECODED_SURFACE_PROTOTYPE_COUNT: CoverageKey =
    CoverageKey::new("decoded_surface_prototype_count");
pub(crate) const DECODED_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("decoded_surface_row_count");
pub(crate) const DECODED_TABULATED_CYLINDER_CONTROL_POINT_SET_COUNT: CoverageKey =
    CoverageKey::new("decoded_tabulated_cylinder_control_point_set_count");
pub(crate) const DECODED_TABULATED_CYLINDER_CURVE_REPLAY_COUNT: CoverageKey =
    CoverageKey::new("decoded_tabulated_cylinder_curve_replay_count");
pub(crate) const DECODED_TOPOLOGICAL_VERTEX_COUNT: CoverageKey =
    CoverageKey::new("decoded_topological_vertex_count");
pub(crate) const DECODED_TORUS_OUTLINE_EXTENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_torus_outline_extent_count");
pub(crate) const DECODED_TORUS_RADIUS_OVERRIDE_COUNT: CoverageKey =
    CoverageKey::new("decoded_torus_radius_override_count");
pub(crate) const DECODED_TWO_CHART_PCURVE_COUNT: CoverageKey =
    CoverageKey::new("decoded_two_chart_pcurve_count");
pub(crate) const DECODED_TYPE24_ROUND_EDGE_ENVELOPE_COUNT: CoverageKey =
    CoverageKey::new("decoded_type24_round_edge_envelope_count");
pub(crate) const INCOMPLETE_LEGACY_OBJECT_ARRAY_COUNT: CoverageKey =
    CoverageKey::new("incomplete_legacy_object_array_count");
pub(crate) const INCOMPLETE_LEGACY_STRING_ARRAY_COUNT: CoverageKey =
    CoverageKey::new("incomplete_legacy_string_array_count");
pub(crate) const RECONCILED_SUPPORT_APEX_CONE_PARAMETER_BRANCH_COUNT: CoverageKey =
    CoverageKey::new("reconciled_support_apex_cone_parameter_branch_count");
pub(crate) const ROUND_EDGE_CARRIER_VALIDATION_FAILURE_COUNT: CoverageKey =
    CoverageKey::new("round_edge_carrier_validation_failure_count");
pub(crate) const ROUND_EDGE_COMPLETE_ENVELOPE_COUNT: CoverageKey =
    CoverageKey::new("round_edge_complete_envelope_count");
pub(crate) const ROUND_EDGE_ENDPOINT_INCIDENCE_MISMATCH_COUNT: CoverageKey =
    CoverageKey::new("round_edge_endpoint_incidence_mismatch_count");
pub(crate) const ROUND_EDGE_MISSING_SUPPORT_PLANE_COUNT: CoverageKey =
    CoverageKey::new("round_edge_missing_support_plane_count");
pub(crate) const ROUND_EDGE_NO_PERPENDICULAR_SUPPORT_PAIR_COUNT: CoverageKey =
    CoverageKey::new("round_edge_no_perpendicular_support_pair_count");
pub(crate) const ROUND_EDGE_NONUNIQUE_RADIUS_COUNT: CoverageKey =
    CoverageKey::new("round_edge_nonunique_radius_count");
pub(crate) const ROUND_EDGE_RADIUS_PROJECTION_MISMATCH_COUNT: CoverageKey =
    CoverageKey::new("round_edge_radius_projection_mismatch_count");
pub(crate) const ROUND_EDGE_REPLAY_CONFLICT_COUNT: CoverageKey =
    CoverageKey::new("round_edge_replay_conflict_count");
pub(crate) const ROUND_EDGE_SOLVED_CARRIER_COUNT: CoverageKey =
    CoverageKey::new("round_edge_solved_carrier_count");
pub(crate) const ROUND_EDGE_UNSOLVED_CARRIER_COUNT: CoverageKey =
    CoverageKey::new("round_edge_unsolved_carrier_count");
pub(crate) const TRANSFERRED_ACTIVE_DATUM_CYLINDER_COUNT: CoverageKey =
    CoverageKey::new("transferred_active_datum_cylinder_count");
pub(crate) const TRANSFERRED_ANALYTIC_PCURVE_CARRIER_COUNT: CoverageKey =
    CoverageKey::new("transferred_analytic_pcurve_carrier_count");
pub(crate) const TRANSFERRED_CIRCULAR_SWEEP_CYLINDER_COUNT: CoverageKey =
    CoverageKey::new("transferred_circular_sweep_cylinder_count");
pub(crate) const TRANSFERRED_CONSTRAINED_SLOT_FILLET_CYLINDER_COUNT: CoverageKey =
    CoverageKey::new("transferred_constrained_slot_fillet_cylinder_count");
pub(crate) const TRANSFERRED_CROSS_SECTION_PLANE_COUNT: CoverageKey =
    CoverageKey::new("transferred_cross_section_plane_count");
pub(crate) const TRANSFERRED_EXTRUSION_PLANE_BOUNDARY_CURVE_COUNT: CoverageKey =
    CoverageKey::new("transferred_extrusion_plane_boundary_curve_count");
pub(crate) const TRANSFERRED_EXTRUSION_PLANE_SECTION_GENERATOR_CURVE_COUNT: CoverageKey =
    CoverageKey::new("transferred_extrusion_plane_section_generator_curve_count");
pub(crate) const TRANSFERRED_FEATURE_CIRCULAR_EXTRUSION_BREP_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_circular_extrusion_brep_count");
pub(crate) const TRANSFERRED_FEATURE_EQUATION_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_equation_constraint_count");
pub(crate) const TRANSFERRED_FEATURE_EXTRUSION_BREP_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_extrusion_brep_count");
pub(crate) const TRANSFERRED_FEATURE_EXTRUSION_SURFACE_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_extrusion_surface_count");
pub(crate) const TRANSFERRED_FEATURE_EXTRUSION_VERTEX_ORBIT_CURVE_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_extrusion_vertex_orbit_curve_count");
pub(crate) const TRANSFERRED_FEATURE_RELATION_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_relation_constraint_count");
pub(crate) const TRANSFERRED_FEATURE_REVOLUTION_BREP_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_revolution_brep_count");
pub(crate) const TRANSFERRED_FEATURE_REVOLUTION_SURFACE_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_revolution_surface_count");
pub(crate) const TRANSFERRED_FEATURE_REVOLUTION_VERTEX_ORBIT_CURVE_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_revolution_vertex_orbit_curve_count");
pub(crate) const TRANSFERRED_FEATURE_SKAMP_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_skamp_constraint_count");
pub(crate) const TRANSFERRED_HOLE_CYLINDER_COUNT: CoverageKey =
    CoverageKey::new("transferred_hole_cylinder_count");
pub(crate) const TRANSFERRED_NATIVE_FEATURE_EQUATION_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_feature_equation_constraint_count");
pub(crate) const TRANSFERRED_NATIVE_FEATURE_RELATION_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_feature_relation_constraint_count");
pub(crate) const TRANSFERRED_NATIVE_FEATURE_SKAMP_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_feature_skamp_constraint_count");
pub(crate) const TRANSFERRED_NATIVE_TOPOLOGICAL_EDGE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_topological_edge_count");
pub(crate) const TRANSFERRED_PART_PRODUCT_COUNT: CoverageKey =
    CoverageKey::new("transferred_part_product_count");
pub(crate) const TRANSFERRED_POSITIONAL_CONE_COUNT: CoverageKey =
    CoverageKey::new("transferred_positional_cone_count");
pub(crate) const TRANSFERRED_POSITIONAL_LINE_EXTRUSION_PLANE_COUNT: CoverageKey =
    CoverageKey::new("transferred_positional_line_extrusion_plane_count");
pub(crate) const TRANSFERRED_POSITIONAL_SPLINE_REPLAY_COUNT: CoverageKey =
    CoverageKey::new("transferred_positional_spline_replay_count");
pub(crate) const TRANSFERRED_ROUND_EDGE_CARRIER_COUNT: CoverageKey =
    CoverageKey::new("transferred_round_edge_carrier_count");
pub(crate) const TRANSFERRED_ROWLESS_ROUND_CYLINDER_COUNT: CoverageKey =
    CoverageKey::new("transferred_rowless_round_cylinder_count");
pub(crate) const TRANSFERRED_SAVED_SPLINE_CURVE_COUNT: CoverageKey =
    CoverageKey::new("transferred_saved_spline_curve_count");
pub(crate) const TRANSFERRED_SHARED_EXTRUSION_GENERATOR_CURVE_COUNT: CoverageKey =
    CoverageKey::new("transferred_shared_extrusion_generator_curve_count");
pub(crate) const TRANSFERRED_SPLIT_OUTLINE_CYLINDER_COUNT: CoverageKey =
    CoverageKey::new("transferred_split_outline_cylinder_count");
pub(crate) const TRANSFERRED_TABULATED_CYLINDER_SPLINE_EXTRUSION_COUNT: CoverageKey =
    CoverageKey::new("transferred_tabulated_cylinder_spline_extrusion_count");
pub(crate) const TRANSFERRED_TOPOLOGICAL_POINT_COUNT: CoverageKey =
    CoverageKey::new("transferred_topological_point_count");
pub(crate) const TRANSFERRED_TOPOLOGY_BOUND_PLANE_SURFACE_COUNT: CoverageKey =
    CoverageKey::new("transferred_topology_bound_plane_surface_count");
pub(crate) const TRANSFERRED_TYPED_FEATURE_EQUATION_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("transferred_typed_feature_equation_constraint_count");
pub(crate) const TRANSFERRED_TYPED_FEATURE_RELATION_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("transferred_typed_feature_relation_constraint_count");
pub(crate) const TRANSFERRED_TYPED_FEATURE_SKAMP_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("transferred_typed_feature_skamp_constraint_count");
pub(crate) const TRANSFERRED_VISIBLE_CURVE_ROW_COUNT: CoverageKey =
    CoverageKey::new("transferred_visible_curve_row_count");
pub(crate) const TRANSFERRED_VISIBLE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("transferred_visible_surface_row_count");
pub(crate) const UNDECODED_LEGACY_STRING_ENCODING_COUNT: CoverageKey =
    CoverageKey::new("undecoded_legacy_string_encoding_count");
pub(crate) const UNIQUE_VISIBLE_CURVE_ROW_COUNT: CoverageKey =
    CoverageKey::new("unique_visible_curve_row_count");
pub(crate) const UNIQUE_VISIBLE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("unique_visible_surface_row_count");
pub(crate) const UNRESOLVED_LEGACY_INTEGER_VALUE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_legacy_integer_value_count");
pub(crate) const UNRESOLVED_LEGACY_OBJECT_VALUE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_legacy_object_value_count");
pub(crate) const UNRESOLVED_LEGACY_REAL_VALUE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_legacy_real_value_count");
pub(crate) const UNRESOLVED_LEGACY_STRING_VALUE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_legacy_string_value_count");

pub(crate) const TRANSFERRED_EXPLICITLY_UNRESOLVED_DRAFT_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_explicitly_unresolved_draft_feature_count");
pub(crate) const TRANSFERRED_FILLED_SURFACE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_filled_surface_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_FILLED_SURFACE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_filled_surface_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_HOLE_TERMINATION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_hole_termination_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_RECOGNIZED_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_recognized_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_SECTION_SHAPE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_section_shape_feature_count");
pub(crate) const TRANSFERRED_INCOMPLETE_THICKEN_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_incomplete_thicken_feature_count");
pub(crate) const TRANSFERRED_NATIVE_KNIT_SURFACE_FACES_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_knit_surface_faces_feature_count");
pub(crate) const TRANSFERRED_SECTION_SHAPE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_section_shape_feature_count");
pub(crate) const TRANSFERRED_THICKEN_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_thicken_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_DRAFT_ANGLE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_draft_angle_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_DRAFT_DIRECTION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_draft_direction_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_DRAFT_FACE_SELECTION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_draft_face_selection_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_DRAFT_NEUTRAL_PLANE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_draft_neutral_plane_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_DRAFT_OUTWARD_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_draft_outward_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_FILLED_SURFACE_BOUNDARY_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_filled_surface_boundary_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_FILLED_SURFACE_CONTINUITY_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_filled_surface_continuity_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_FILLED_SURFACE_MERGE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_filled_surface_merge_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_FILLED_SURFACE_SUPPORT_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_filled_surface_support_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_HOLE_DIAMETER_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_hole_diameter_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_HOLE_DIRECTION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_hole_direction_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_HOLE_KIND_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_hole_kind_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_HOLE_LOCATION_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_hole_location_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_THICKEN_FACES_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_thicken_faces_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_THICKEN_SIDE_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_thicken_side_feature_count");
pub(crate) const TRANSFERRED_UNRESOLVED_THICKEN_THICKNESS_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_unresolved_thicken_thickness_feature_count");

pub(crate) const BREP_REJECTED_FACE_AMBIGUOUS_BOUNDARY_CURVE_COUNT: CoverageKey =
    CoverageKey::new("brep_rejected_face_ambiguous_boundary_curve_count");
pub(crate) const BREP_REJECTED_FACE_AMBIGUOUS_SURFACE_CARRIER_COUNT: CoverageKey =
    CoverageKey::new("brep_rejected_face_ambiguous_surface_carrier_count");
pub(crate) const BREP_REJECTED_FACE_LOOP_ORDERING_COUNT: CoverageKey =
    CoverageKey::new("brep_rejected_face_loop_ordering_count");
pub(crate) const BREP_REJECTED_FACE_MISSING_LOOPS_COUNT: CoverageKey =
    CoverageKey::new("brep_rejected_face_missing_loops_count");
pub(crate) const BREP_REJECTED_FACE_MISSING_ORIENTATION_COUNT: CoverageKey =
    CoverageKey::new("brep_rejected_face_missing_orientation_count");
pub(crate) const BREP_REJECTED_FACE_MISSING_SURFACE_CARRIER_COUNT: CoverageKey =
    CoverageKey::new("brep_rejected_face_missing_surface_carrier_count");
pub(crate) const BREP_REJECTED_FACE_TWO_EDGE_PARAMETER_PROOF_COUNT: CoverageKey =
    CoverageKey::new("brep_rejected_face_two_edge_parameter_proof_count");
pub(crate) const BREP_REJECTED_FACE_UNRESOLVED_BOUNDARY_VERTICES_COUNT: CoverageKey =
    CoverageKey::new("brep_rejected_face_unresolved_boundary_vertices_count");
pub(crate) const BREP_SELECTED_BODY_COUNT_UNRESOLVED: CoverageKey =
    CoverageKey::new("brep_selected_body_count_unresolved");
pub(crate) const DECODED_LEGACY_TYPE_11_ARRAY_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_11_array_count");
pub(crate) const DECODED_LEGACY_TYPE_11_ELEMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_11_element_count");
pub(crate) const DECODED_LEGACY_TYPE_11_SCALAR_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_11_scalar_count");
pub(crate) const DECODED_LEGACY_TYPE_3_SCALAR_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_3_scalar_count");
pub(crate) const DECODED_LEGACY_TYPE_4_SCALAR_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_4_scalar_count");
pub(crate) const DECODED_LEGACY_TYPE_5_ARRAY_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_5_array_count");
pub(crate) const DECODED_LEGACY_TYPE_5_ELEMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_5_element_count");
pub(crate) const DECODED_LEGACY_TYPE_5_SCALAR_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_5_scalar_count");
pub(crate) const DECODED_LEGACY_TYPE_6_ARRAY_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_6_array_count");
pub(crate) const DECODED_LEGACY_TYPE_6_ELEMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_6_element_count");
pub(crate) const DECODED_LEGACY_TYPE_6_SCALAR_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_6_scalar_count");
pub(crate) const DECODED_LEGACY_TYPE_7_ARRAY_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_7_array_count");
pub(crate) const DECODED_LEGACY_TYPE_7_ELEMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_7_element_count");
pub(crate) const DECODED_LEGACY_TYPE_7_SCALAR_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_7_scalar_count");
pub(crate) const DECODED_LEGACY_TYPE_9_ARRAY_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_9_array_count");
pub(crate) const DECODED_LEGACY_TYPE_9_ELEMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_9_element_count");
pub(crate) const DECODED_LEGACY_TYPE_9_SCALAR_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_type_9_scalar_count");
pub(crate) const UNDECODED_LEGACY_TYPE_3_ENCODING_COUNT: CoverageKey =
    CoverageKey::new("undecoded_legacy_type_3_encoding_count");
pub(crate) const UNDECODED_LEGACY_TYPE_4_ENCODING_COUNT: CoverageKey =
    CoverageKey::new("undecoded_legacy_type_4_encoding_count");
pub(crate) const UNRESOLVED_LEGACY_TYPE_11_VALUE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_legacy_type_11_value_count");
pub(crate) const UNRESOLVED_LEGACY_TYPE_3_VALUE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_legacy_type_3_value_count");
pub(crate) const UNRESOLVED_LEGACY_TYPE_4_VALUE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_legacy_type_4_value_count");
pub(crate) const UNRESOLVED_LEGACY_TYPE_5_VALUE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_legacy_type_5_value_count");
pub(crate) const UNRESOLVED_LEGACY_TYPE_6_VALUE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_legacy_type_6_value_count");
pub(crate) const UNRESOLVED_LEGACY_TYPE_7_VALUE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_legacy_type_7_value_count");
pub(crate) const UNRESOLVED_LEGACY_TYPE_9_VALUE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_legacy_type_9_value_count");

pub(crate) const DECODED_FEATURE_ARC_SEGMENT_COUNT: CoverageKey =
    CoverageKey::new("decoded_feature_arc_segment_count");
pub(crate) const RESOLVED_FEATURE_ARC_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("resolved_feature_arc_segment_geometry_count");
pub(crate) const RESOLVED_FEATURE_BOUNDED_CURVE_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("resolved_feature_bounded_curve_segment_geometry_count");
pub(crate) const RESOLVED_FEATURE_CENTERED_LINE_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("resolved_feature_centered_line_segment_geometry_count");
pub(crate) const RESOLVED_FEATURE_CIRCLE_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("resolved_feature_circle_segment_geometry_count");
pub(crate) const RESOLVED_FEATURE_CONIC_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("resolved_feature_conic_segment_geometry_count");
pub(crate) const RESOLVED_FEATURE_OPAQUE_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("resolved_feature_opaque_segment_geometry_count");
pub(crate) const RESOLVED_FEATURE_POINT_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("resolved_feature_point_segment_geometry_count");
pub(crate) const RESOLVED_FEATURE_REFERENCE_LINE_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("resolved_feature_reference_line_segment_geometry_count");
pub(crate) const RETAINED_UNKNOWN_VISIBLE_CONE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("retained_unknown_visible_cone_surface_row_count");
pub(crate) const RETAINED_UNKNOWN_VISIBLE_CYLINDER_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("retained_unknown_visible_cylinder_surface_row_count");
pub(crate) const RETAINED_UNKNOWN_VISIBLE_EXTRUSION_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("retained_unknown_visible_extrusion_surface_row_count");
pub(crate) const RETAINED_UNKNOWN_VISIBLE_FILLET_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("retained_unknown_visible_fillet_surface_row_count");
pub(crate) const RETAINED_UNKNOWN_VISIBLE_PLANE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("retained_unknown_visible_plane_surface_row_count");
pub(crate) const RETAINED_UNKNOWN_VISIBLE_SPLINE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("retained_unknown_visible_spline_surface_row_count");
pub(crate) const RETAINED_UNKNOWN_VISIBLE_TORUS_OR_SPHERE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("retained_unknown_visible_torus_or_sphere_surface_row_count");
pub(crate) const TRANSFERRED_VISIBLE_CONE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("transferred_visible_cone_surface_row_count");
pub(crate) const TRANSFERRED_VISIBLE_CYLINDER_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("transferred_visible_cylinder_surface_row_count");
pub(crate) const TRANSFERRED_VISIBLE_EXTRUSION_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("transferred_visible_extrusion_surface_row_count");
pub(crate) const TRANSFERRED_VISIBLE_FILLET_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("transferred_visible_fillet_surface_row_count");
pub(crate) const TRANSFERRED_VISIBLE_SPLINE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("transferred_visible_spline_surface_row_count");
pub(crate) const TRANSFERRED_VISIBLE_TORUS_OR_SPHERE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("transferred_visible_torus_or_sphere_surface_row_count");
pub(crate) const UNRESOLVED_FEATURE_ARC_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_arc_segment_geometry_count");
pub(crate) const UNRESOLVED_FEATURE_BOUNDED_CURVE_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_bounded_curve_segment_geometry_count");
pub(crate) const UNRESOLVED_FEATURE_CENTERED_LINE_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_centered_line_segment_geometry_count");
pub(crate) const UNRESOLVED_FEATURE_CIRCLE_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_circle_segment_geometry_count");
pub(crate) const UNRESOLVED_FEATURE_CONIC_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_conic_segment_geometry_count");
pub(crate) const UNRESOLVED_FEATURE_OPAQUE_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_opaque_segment_geometry_count");
pub(crate) const UNRESOLVED_FEATURE_POINT_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_point_segment_geometry_count");
pub(crate) const UNRESOLVED_FEATURE_REFERENCE_LINE_SEGMENT_GEOMETRY_COUNT: CoverageKey =
    CoverageKey::new("unresolved_feature_reference_line_segment_geometry_count");
pub(crate) const UNTRANSFERRED_VISIBLE_CONE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("untransferred_visible_cone_surface_row_count");
pub(crate) const UNTRANSFERRED_VISIBLE_EXTRUSION_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("untransferred_visible_extrusion_surface_row_count");
pub(crate) const UNTRANSFERRED_VISIBLE_FILLET_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("untransferred_visible_fillet_surface_row_count");
pub(crate) const UNTRANSFERRED_VISIBLE_SPLINE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("untransferred_visible_spline_surface_row_count");
pub(crate) const UNTRANSFERRED_VISIBLE_TORUS_OR_SPHERE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("untransferred_visible_torus_or_sphere_surface_row_count");
pub(crate) const VISIBLE_CONE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("visible_cone_surface_row_count");
pub(crate) const VISIBLE_CYLINDER_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("visible_cylinder_surface_row_count");
pub(crate) const VISIBLE_EXTRUSION_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("visible_extrusion_surface_row_count");
pub(crate) const VISIBLE_FILLET_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("visible_fillet_surface_row_count");
pub(crate) const VISIBLE_SPLINE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("visible_spline_surface_row_count");
pub(crate) const VISIBLE_TORUS_OR_SPHERE_SURFACE_ROW_COUNT: CoverageKey =
    CoverageKey::new("visible_torus_or_sphere_surface_row_count");

#[cfg(test)]
pub(crate) const ALL: &[CoverageKey] = &[
    DECODED_FEATURE_ARC_SEGMENT_COUNT,
    RESOLVED_FEATURE_ARC_SEGMENT_GEOMETRY_COUNT,
    RESOLVED_FEATURE_BOUNDED_CURVE_SEGMENT_GEOMETRY_COUNT,
    RESOLVED_FEATURE_CENTERED_LINE_SEGMENT_GEOMETRY_COUNT,
    RESOLVED_FEATURE_CIRCLE_SEGMENT_GEOMETRY_COUNT,
    RESOLVED_FEATURE_CONIC_SEGMENT_GEOMETRY_COUNT,
    RESOLVED_FEATURE_OPAQUE_SEGMENT_GEOMETRY_COUNT,
    RESOLVED_FEATURE_POINT_SEGMENT_GEOMETRY_COUNT,
    RESOLVED_FEATURE_REFERENCE_LINE_SEGMENT_GEOMETRY_COUNT,
    RETAINED_UNKNOWN_VISIBLE_CONE_SURFACE_ROW_COUNT,
    RETAINED_UNKNOWN_VISIBLE_CYLINDER_SURFACE_ROW_COUNT,
    RETAINED_UNKNOWN_VISIBLE_EXTRUSION_SURFACE_ROW_COUNT,
    RETAINED_UNKNOWN_VISIBLE_FILLET_SURFACE_ROW_COUNT,
    RETAINED_UNKNOWN_VISIBLE_PLANE_SURFACE_ROW_COUNT,
    RETAINED_UNKNOWN_VISIBLE_SPLINE_SURFACE_ROW_COUNT,
    RETAINED_UNKNOWN_VISIBLE_TORUS_OR_SPHERE_SURFACE_ROW_COUNT,
    TRANSFERRED_VISIBLE_CONE_SURFACE_ROW_COUNT,
    TRANSFERRED_VISIBLE_CYLINDER_SURFACE_ROW_COUNT,
    TRANSFERRED_VISIBLE_EXTRUSION_SURFACE_ROW_COUNT,
    TRANSFERRED_VISIBLE_FILLET_SURFACE_ROW_COUNT,
    TRANSFERRED_VISIBLE_SPLINE_SURFACE_ROW_COUNT,
    TRANSFERRED_VISIBLE_TORUS_OR_SPHERE_SURFACE_ROW_COUNT,
    UNRESOLVED_FEATURE_ARC_SEGMENT_GEOMETRY_COUNT,
    UNRESOLVED_FEATURE_BOUNDED_CURVE_SEGMENT_GEOMETRY_COUNT,
    UNRESOLVED_FEATURE_CENTERED_LINE_SEGMENT_GEOMETRY_COUNT,
    UNRESOLVED_FEATURE_CIRCLE_SEGMENT_GEOMETRY_COUNT,
    UNRESOLVED_FEATURE_CONIC_SEGMENT_GEOMETRY_COUNT,
    UNRESOLVED_FEATURE_OPAQUE_SEGMENT_GEOMETRY_COUNT,
    UNRESOLVED_FEATURE_POINT_SEGMENT_GEOMETRY_COUNT,
    UNRESOLVED_FEATURE_REFERENCE_LINE_SEGMENT_GEOMETRY_COUNT,
    UNTRANSFERRED_VISIBLE_CONE_SURFACE_ROW_COUNT,
    UNTRANSFERRED_VISIBLE_EXTRUSION_SURFACE_ROW_COUNT,
    UNTRANSFERRED_VISIBLE_FILLET_SURFACE_ROW_COUNT,
    UNTRANSFERRED_VISIBLE_SPLINE_SURFACE_ROW_COUNT,
    UNTRANSFERRED_VISIBLE_TORUS_OR_SPHERE_SURFACE_ROW_COUNT,
    VISIBLE_CONE_SURFACE_ROW_COUNT,
    VISIBLE_CYLINDER_SURFACE_ROW_COUNT,
    VISIBLE_EXTRUSION_SURFACE_ROW_COUNT,
    VISIBLE_FILLET_SURFACE_ROW_COUNT,
    VISIBLE_SPLINE_SURFACE_ROW_COUNT,
    VISIBLE_TORUS_OR_SPHERE_SURFACE_ROW_COUNT,
    BREP_REJECTED_FACE_AMBIGUOUS_BOUNDARY_CURVE_COUNT,
    BREP_REJECTED_FACE_AMBIGUOUS_SURFACE_CARRIER_COUNT,
    BREP_REJECTED_FACE_LOOP_ORDERING_COUNT,
    BREP_REJECTED_FACE_MISSING_LOOPS_COUNT,
    BREP_REJECTED_FACE_MISSING_ORIENTATION_COUNT,
    BREP_REJECTED_FACE_MISSING_SURFACE_CARRIER_COUNT,
    BREP_REJECTED_FACE_TWO_EDGE_PARAMETER_PROOF_COUNT,
    BREP_REJECTED_FACE_UNRESOLVED_BOUNDARY_VERTICES_COUNT,
    BREP_SELECTED_BODY_COUNT_UNRESOLVED,
    DECODED_LEGACY_TYPE_11_ARRAY_COUNT,
    DECODED_LEGACY_TYPE_11_ELEMENT_COUNT,
    DECODED_LEGACY_TYPE_11_SCALAR_COUNT,
    DECODED_LEGACY_TYPE_3_SCALAR_COUNT,
    DECODED_LEGACY_TYPE_4_SCALAR_COUNT,
    DECODED_LEGACY_TYPE_5_ARRAY_COUNT,
    DECODED_LEGACY_TYPE_5_ELEMENT_COUNT,
    DECODED_LEGACY_TYPE_5_SCALAR_COUNT,
    DECODED_LEGACY_TYPE_6_ARRAY_COUNT,
    DECODED_LEGACY_TYPE_6_ELEMENT_COUNT,
    DECODED_LEGACY_TYPE_6_SCALAR_COUNT,
    DECODED_LEGACY_TYPE_7_ARRAY_COUNT,
    DECODED_LEGACY_TYPE_7_ELEMENT_COUNT,
    DECODED_LEGACY_TYPE_7_SCALAR_COUNT,
    DECODED_LEGACY_TYPE_9_ARRAY_COUNT,
    DECODED_LEGACY_TYPE_9_ELEMENT_COUNT,
    DECODED_LEGACY_TYPE_9_SCALAR_COUNT,
    UNDECODED_LEGACY_TYPE_3_ENCODING_COUNT,
    UNDECODED_LEGACY_TYPE_4_ENCODING_COUNT,
    UNRESOLVED_LEGACY_TYPE_11_VALUE_COUNT,
    UNRESOLVED_LEGACY_TYPE_3_VALUE_COUNT,
    UNRESOLVED_LEGACY_TYPE_4_VALUE_COUNT,
    UNRESOLVED_LEGACY_TYPE_5_VALUE_COUNT,
    UNRESOLVED_LEGACY_TYPE_6_VALUE_COUNT,
    UNRESOLVED_LEGACY_TYPE_7_VALUE_COUNT,
    UNRESOLVED_LEGACY_TYPE_9_VALUE_COUNT,
    TRANSFERRED_EXPLICITLY_UNRESOLVED_DRAFT_FEATURE_COUNT,
    TRANSFERRED_FILLED_SURFACE_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_FILLED_SURFACE_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_HOLE_TERMINATION_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_RECOGNIZED_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_SECTION_SHAPE_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_THICKEN_FEATURE_COUNT,
    TRANSFERRED_NATIVE_KNIT_SURFACE_FACES_FEATURE_COUNT,
    TRANSFERRED_SECTION_SHAPE_FEATURE_COUNT,
    TRANSFERRED_THICKEN_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_DRAFT_ANGLE_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_DRAFT_DIRECTION_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_DRAFT_FACE_SELECTION_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_DRAFT_NEUTRAL_PLANE_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_DRAFT_OUTWARD_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_FILLED_SURFACE_BOUNDARY_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_FILLED_SURFACE_CONTINUITY_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_FILLED_SURFACE_MERGE_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_FILLED_SURFACE_SUPPORT_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_HOLE_DIAMETER_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_HOLE_DIRECTION_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_HOLE_KIND_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_HOLE_LOCATION_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_THICKEN_FACES_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_THICKEN_SIDE_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_THICKEN_THICKNESS_FEATURE_COUNT,
    ACTIVE_FEATURE_EQUATION_CONSTRAINT_COUNT,
    ACTIVE_FEATURE_RELATION_CONSTRAINT_COUNT,
    ACTIVE_FEATURE_SKAMP_CONSTRAINT_COUNT,
    ACTIVE_NATIVE_FEATURE_EQUATION_CONSTRAINT_COUNT,
    ACTIVE_NATIVE_FEATURE_RELATION_CONSTRAINT_COUNT,
    ACTIVE_NATIVE_FEATURE_SKAMP_CONSTRAINT_COUNT,
    ACTIVE_TYPED_FEATURE_EQUATION_CONSTRAINT_COUNT,
    ACTIVE_TYPED_FEATURE_RELATION_CONSTRAINT_COUNT,
    ACTIVE_TYPED_FEATURE_SKAMP_CONSTRAINT_COUNT,
    AMBIGUOUS_VISIBLE_CURVE_ROW_COUNT,
    AMBIGUOUS_VISIBLE_SURFACE_ROW_COUNT,
    AXIAL_INTERVAL_CORNER_ENVELOPE_COUNT,
    AXIAL_INTERVAL_CORNER_SOLVED_CARRIER_COUNT,
    BREP_ADMITTED_COMPONENT_COUNT,
    BREP_ADMITTED_FACE_COUNT,
    BREP_BODY_COUNT_MISMATCH_COUNT,
    BREP_BOUNDARY_CURVE_COUNT,
    BREP_BOUNDARY_CURVE_MISSING_INCIDENCE_COUNT,
    BREP_BOUNDARY_CURVE_UNSOLVED_VERTEX_COUNT,
    BREP_CANDIDATE_FACE_COUNT,
    BREP_EMITTED_FACE_COUNT,
    BREP_EMPTY_COMPONENT_COUNT,
    BREP_LEGACY_BODY_OWNERSHIP_AMBIGUOUS_COUNT,
    BREP_LEGACY_NONVISIBLE_FACE_REFERENCE_COUNT,
    BREP_PCURVE_ACCEPTED_RECORD_COUNT,
    BREP_PCURVE_CARRIER_REJECTED_PATH_COUNT,
    BREP_PCURVE_CARRIER_REJECTED_RECORD_COUNT,
    BREP_PCURVE_CARRIER_UNKNOWN_MISSING_CARRIER_PATH_COUNT,
    BREP_PCURVE_CARRIER_UNKNOWN_MISSING_SURFACE_PATH_COUNT,
    BREP_PCURVE_CARRIER_UNKNOWN_PARALLEL_PLANE_PATH_COUNT,
    BREP_PCURVE_CARRIER_UNKNOWN_PATH_COUNT,
    BREP_PCURVE_CARRIER_UNKNOWN_UNSUPPORTED_PAIR_PATH_COUNT,
    BREP_PCURVE_CARRIER_UNKNOWN_UNSUPPORTED_PATH_COUNT,
    BREP_PCURVE_CARRIER_VALIDATED_PATH_COUNT,
    BREP_PCURVE_COMPLETE_RECORD_COUNT,
    BREP_PCURVE_CONFLICTING_CURVE_COUNT,
    BREP_PCURVE_INACTIVE_PATH_COUNT,
    BREP_PCURVE_INACTIVE_RECORD_COUNT,
    BREP_PCURVE_INCONSISTENT_RECORD_COUNT,
    BREP_PCURVE_MAPPED_PATH_COUNT,
    BREP_PCURVE_MISSING_SURFACE_PATH_COUNT,
    BREP_PCURVE_PARTIAL_RECORD_COUNT,
    BREP_PCURVE_PATH_COUNT,
    BREP_PCURVE_RECORD_COUNT,
    BREP_PCURVE_TOPOLOGY_MISMATCH_RECORD_COUNT,
    BREP_PCURVE_TWO_CHART_COMPLETE_RECORD_COUNT,
    BREP_PCURVE_TWO_CHART_MAPPED_RECORD_COUNT,
    BREP_PCURVE_TWO_CHART_MISSING_SURFACE_PATH_COUNT,
    BREP_PCURVE_TWO_CHART_NO_SAMPLE_RECORD_COUNT,
    BREP_PCURVE_TWO_CHART_PARTIAL_RECORD_COUNT,
    BREP_PCURVE_TWO_CHART_RECORD_COUNT,
    BREP_PCURVE_TWO_CHART_SURFACE_MISMATCH_RECORD_COUNT,
    BREP_PCURVE_TWO_CHART_UNEVALUABLE_PATH_COUNT,
    BREP_PCURVE_TWO_CHART_UNMAPPED_RECORD_COUNT,
    BREP_PCURVE_UNEVALUABLE_PATH_COUNT,
    BREP_PCURVE_UNMAPPED_RECORD_COUNT,
    BREP_REJECTED_FACE_COUNT,
    BREP_SELECTED_BODY_COUNT,
    BREP_VERTEX_ANALYTIC_DOMAIN_COUNT,
    BREP_VERTEX_CARRIER_AMBIGUOUS_CANDIDATE_COUNT,
    BREP_VERTEX_CARRIER_INCIDENT_COUNT,
    BREP_VERTEX_CARRIER_NO_GEOMETRIC_CANDIDATE_COUNT,
    BREP_VERTEX_CARRIER_NO_VALID_CANDIDATE_COUNT,
    BREP_VERTEX_CARRIER_PAIR_INTERSECTION_CANDIDATE_COUNT,
    BREP_VERTEX_CARRIER_POINT_COUNT,
    BREP_VERTEX_CARRIER_TRIPLE_INTERSECTION_CANDIDATE_COUNT,
    BREP_VERTEX_CARRIER_VALID_INTERSECTION_CANDIDATE_COUNT,
    BREP_VERTEX_CARRIER_ZERO_CANDIDATE_COUNT,
    BREP_VERTEX_COMPLETE_PCURVE_ENDPOINT_EVIDENCE_COUNT,
    BREP_VERTEX_DIRECTED_ENDPOINT_ASSIGNMENT_COUNT,
    BREP_VERTEX_DIRECTED_ENDPOINT_CONFLICT_COUNT,
    BREP_VERTEX_NURBS_ENDPOINT_CONSTRAINT_COUNT,
    BREP_VERTEX_PCURVE_AMBIGUOUS_ENDPOINT_VERTEX_COUNT,
    BREP_VERTEX_PCURVE_CONSTRAINT_COUNT,
    BREP_VERTEX_PCURVE_ENDPOINT_EVIDENCE_COUNT,
    BREP_VERTEX_PCURVE_FIXED_ENDPOINT_CONFLICT_COUNT,
    BREP_VERTEX_SOLVED_COUNT,
    BREP_VERTEX_TOPOLOGICAL_COUNT,
    CONFLICTING_PRIMITIVE_TRIANGLE_STRIP_REPRESENTATION_COUNT,
    DECODED_BOUND_PROTOTYPE_PCURVE_COUNT,
    DECODED_CROSS_SECTION_CURVE_PROTOTYPE_COUNT,
    DECODED_CROSS_SECTION_CURVE_ROW_COUNT,
    DECODED_CROSS_SECTION_OUTLINE_PLANE_COUNT,
    DECODED_CROSS_SECTION_PLANE_ENVELOPE_COUNT,
    DECODED_CROSS_SECTION_PLANE_LOCAL_SYSTEM_COUNT,
    DECODED_CROSS_SECTION_SURFACE_PARAMETER_RECORD_COUNT,
    DECODED_CROSS_SECTION_SURFACE_ROW_COUNT,
    DECODED_CURVE_EXPRESSION_RECORD_COUNT,
    DECODED_CURVE_PARAMETER_RECORD_COUNT,
    DECODED_CURVE_PROTOTYPE_COUNT,
    DECODED_CURVE_PROTOTYPE_TOPOLOGY_COUNT,
    DECODED_CURVE_TOPOLOGY_ROW_COUNT,
    DECODED_DATUM_PLANE_COUNT,
    DECODED_FACE_COMPONENT_COUNT,
    DECODED_FC05_CIRCLE_COUNT,
    DECODED_FC05_CYLINDER_CAP_PAIR_COUNT,
    DECODED_FC_CURVE_COORDINATE_RECORD_COUNT,
    DECODED_FEATURE_AFFECTED_ID_ARRAY_COUNT,
    DECODED_FEATURE_CHOICE_COUNT,
    DECODED_FEATURE_CHOICE_FIELD_COUNT,
    DECODED_FEATURE_CONIC_SEGMENT_COUNT,
    DECODED_FEATURE_COUNT,
    DECODED_FEATURE_DEFINITION_COUNT,
    DECODED_FEATURE_ENTITY_COUNT,
    DECODED_FEATURE_ENTITY_REFERENCE_COUNT,
    DECODED_FEATURE_ENTITY_TABLE_COUNT,
    DECODED_FEATURE_EQUATION_COUNT,
    DECODED_FEATURE_EQUATION_TABLE_COUNT,
    DECODED_FEATURE_GEOMETRY_TABLE_COUNT,
    DECODED_FEATURE_LOOP_RESTORE_DIRECTION_COUNT,
    DECODED_FEATURE_OPAQUE_SEGMENT_COUNT,
    DECODED_FEATURE_OPERATION_COUNT,
    DECODED_FEATURE_OPERATION_STATE_COUNT,
    DECODED_FEATURE_ORDER_ENTRY_COUNT,
    DECODED_FEATURE_OUTLINE_COUNT,
    DECODED_FEATURE_PLACEMENT_INSTRUCTION_COUNT,
    DECODED_FEATURE_REPLAY_AFFECTED_ID_COUNT,
    DECODED_FEATURE_REVOLUTION_EXTENT_COUNT,
    DECODED_FEATURE_ROW_COUNT,
    DECODED_FEATURE_SAVED_CONIC_COUNT,
    DECODED_FEATURE_SAVED_ENTITY_COUNT,
    DECODED_FEATURE_SECTION_POINT_COUNT,
    DECODED_FEATURE_SECTION_TRANSFORM_COUNT,
    DECODED_FEATURE_TRIM_ENTITY_COUNT,
    DECODED_FEATURE_TRIM_VERTEX_COUNT,
    DECODED_HALF_EDGE_COUNT,
    DECODED_LEGACY_INTEGER_ARRAY_COUNT,
    DECODED_LEGACY_INTEGER_ELEMENT_COUNT,
    DECODED_LEGACY_INTEGER_SCALAR_COUNT,
    DECODED_LEGACY_OBJECT_ARRAY_COUNT,
    DECODED_LEGACY_OBJECT_ARROW_COUNT,
    DECODED_LEGACY_OBJECT_INLINE_COUNT,
    DECODED_LEGACY_OBJECT_NULL_COUNT,
    DECODED_LEGACY_PRINCIPAL_UNIT_COUNT,
    DECODED_LEGACY_REAL_ARRAY_COUNT,
    DECODED_LEGACY_REAL_ELEMENT_COUNT,
    DECODED_LEGACY_REAL_SCALAR_COUNT,
    DECODED_LEGACY_STRING_ARRAY_COUNT,
    DECODED_LEGACY_STRING_ELEMENT_COUNT,
    DECODED_LEGACY_STRING_SCALAR_COUNT,
    DECODED_LEGACY_TORUS_OR_SPHERE_CARRIER_COUNT,
    DECODED_LOOP_COUNT,
    DECODED_NAMED_SURFACE_PROTOTYPE_COUNT,
    DECODED_OUTLINE_PLANE_COUNT,
    DECODED_PCURVE_COUNT,
    DECODED_PLANE_ENVELOPE_COUNT,
    DECODED_PLANE_LOCAL_SYSTEM_COUNT,
    DECODED_POSITIONAL_EXTRUSION_DIRECTION_COUNT,
    DECODED_POSITIONAL_FRAME_PLANE_COUNT,
    DECODED_PRIMITIVE_TRIANGLE_STRIP_COUNT,
    DECODED_PROTOTYPE_PCURVE_COUNT,
    DECODED_REFERENCE_CIRCLE_COUNT,
    DECODED_REFERENCE_CONIC_COUNT,
    DECODED_REFERENCE_LINE_COUNT,
    DECODED_SURFACE_MERGE_REPLAY_AFFECTED_ID_COUNT,
    DECODED_SURFACE_PARAMETER_RECORD_COUNT,
    DECODED_SURFACE_PROTOTYPE_COUNT,
    DECODED_SURFACE_ROW_COUNT,
    DECODED_TABULATED_CYLINDER_CONTROL_POINT_SET_COUNT,
    DECODED_TABULATED_CYLINDER_CURVE_REPLAY_COUNT,
    DECODED_TOPOLOGICAL_VERTEX_COUNT,
    DECODED_TORUS_OUTLINE_EXTENT_COUNT,
    DECODED_TORUS_RADIUS_OVERRIDE_COUNT,
    DECODED_TWO_CHART_PCURVE_COUNT,
    DECODED_TYPE24_ROUND_EDGE_ENVELOPE_COUNT,
    INCOMPLETE_LEGACY_OBJECT_ARRAY_COUNT,
    INCOMPLETE_LEGACY_STRING_ARRAY_COUNT,
    RECONCILED_SUPPORT_APEX_CONE_PARAMETER_BRANCH_COUNT,
    ROUND_EDGE_CARRIER_VALIDATION_FAILURE_COUNT,
    ROUND_EDGE_COMPLETE_ENVELOPE_COUNT,
    ROUND_EDGE_ENDPOINT_INCIDENCE_MISMATCH_COUNT,
    ROUND_EDGE_MISSING_SUPPORT_PLANE_COUNT,
    ROUND_EDGE_NO_PERPENDICULAR_SUPPORT_PAIR_COUNT,
    ROUND_EDGE_NONUNIQUE_RADIUS_COUNT,
    ROUND_EDGE_RADIUS_PROJECTION_MISMATCH_COUNT,
    ROUND_EDGE_REPLAY_CONFLICT_COUNT,
    ROUND_EDGE_SOLVED_CARRIER_COUNT,
    ROUND_EDGE_UNSOLVED_CARRIER_COUNT,
    TRANSFERRED_ACTIVE_DATUM_CYLINDER_COUNT,
    TRANSFERRED_ANALYTIC_PCURVE_CARRIER_COUNT,
    TRANSFERRED_CIRCULAR_SWEEP_CYLINDER_COUNT,
    TRANSFERRED_CONSTRAINED_SLOT_FILLET_CYLINDER_COUNT,
    TRANSFERRED_CROSS_SECTION_PLANE_COUNT,
    TRANSFERRED_EXTRUSION_PLANE_BOUNDARY_CURVE_COUNT,
    TRANSFERRED_EXTRUSION_PLANE_SECTION_GENERATOR_CURVE_COUNT,
    TRANSFERRED_FEATURE_CIRCULAR_EXTRUSION_BREP_COUNT,
    TRANSFERRED_FEATURE_EQUATION_CONSTRAINT_COUNT,
    TRANSFERRED_FEATURE_EXTRUSION_BREP_COUNT,
    TRANSFERRED_FEATURE_EXTRUSION_SURFACE_COUNT,
    TRANSFERRED_FEATURE_EXTRUSION_VERTEX_ORBIT_CURVE_COUNT,
    TRANSFERRED_FEATURE_RELATION_CONSTRAINT_COUNT,
    TRANSFERRED_FEATURE_REVOLUTION_BREP_COUNT,
    TRANSFERRED_FEATURE_REVOLUTION_SURFACE_COUNT,
    TRANSFERRED_FEATURE_REVOLUTION_VERTEX_ORBIT_CURVE_COUNT,
    TRANSFERRED_FEATURE_SKAMP_CONSTRAINT_COUNT,
    TRANSFERRED_HOLE_CYLINDER_COUNT,
    TRANSFERRED_NATIVE_FEATURE_EQUATION_CONSTRAINT_COUNT,
    TRANSFERRED_NATIVE_FEATURE_RELATION_CONSTRAINT_COUNT,
    TRANSFERRED_NATIVE_FEATURE_SKAMP_CONSTRAINT_COUNT,
    TRANSFERRED_NATIVE_TOPOLOGICAL_EDGE_COUNT,
    TRANSFERRED_PART_PRODUCT_COUNT,
    TRANSFERRED_POSITIONAL_CONE_COUNT,
    TRANSFERRED_POSITIONAL_LINE_EXTRUSION_PLANE_COUNT,
    TRANSFERRED_POSITIONAL_SPLINE_REPLAY_COUNT,
    TRANSFERRED_ROUND_EDGE_CARRIER_COUNT,
    TRANSFERRED_ROWLESS_ROUND_CYLINDER_COUNT,
    TRANSFERRED_SAVED_SPLINE_CURVE_COUNT,
    TRANSFERRED_SHARED_EXTRUSION_GENERATOR_CURVE_COUNT,
    TRANSFERRED_SPLIT_OUTLINE_CYLINDER_COUNT,
    TRANSFERRED_TABULATED_CYLINDER_SPLINE_EXTRUSION_COUNT,
    TRANSFERRED_TOPOLOGICAL_POINT_COUNT,
    TRANSFERRED_TOPOLOGY_BOUND_PLANE_SURFACE_COUNT,
    TRANSFERRED_TYPED_FEATURE_EQUATION_CONSTRAINT_COUNT,
    TRANSFERRED_TYPED_FEATURE_RELATION_CONSTRAINT_COUNT,
    TRANSFERRED_TYPED_FEATURE_SKAMP_CONSTRAINT_COUNT,
    TRANSFERRED_VISIBLE_CURVE_ROW_COUNT,
    TRANSFERRED_VISIBLE_SURFACE_ROW_COUNT,
    UNDECODED_LEGACY_STRING_ENCODING_COUNT,
    UNIQUE_VISIBLE_CURVE_ROW_COUNT,
    UNIQUE_VISIBLE_SURFACE_ROW_COUNT,
    UNRESOLVED_LEGACY_INTEGER_VALUE_COUNT,
    UNRESOLVED_LEGACY_OBJECT_VALUE_COUNT,
    UNRESOLVED_LEGACY_REAL_VALUE_COUNT,
    UNRESOLVED_LEGACY_STRING_VALUE_COUNT,
    ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
    CONDITIONAL_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
    DECODED_ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
    DECODED_ACTIVE_CURVE_EXPRESSION_FUNCTION_WRITE_ASSIGNMENT_COUNT,
    DECODED_ACTIVE_CURVE_EXPRESSION_SCOPED_SYMBOL_ASSIGNMENT_COUNT,
    DECODED_ACTIVE_CURVE_EXPRESSION_SIMULTANEOUS_EQUATION_COUNT,
    DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_ASSIGNMENT_COUNT,
    DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_BLOCK_COUNT,
    DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_VARIABLE_COUNT,
    DECODED_ACTIVE_CURVE_EXPRESSION_SYSTEM_SYMBOL_ASSIGNMENT_COUNT,
    DECODED_ACTIVE_CURVE_EXPRESSION_TABLE_CELL_ASSIGNMENT_COUNT,
    DECODED_CONFIGURATION_DRIVER_TABLE_REFERENCE_COUNT,
    DECODED_LEGACY_CONFIGURATION_DRIVER_TABLE_COUNT,
    DECODED_LEGACY_CONFIGURATION_ITEM_COUNT,
    DECODED_LEGACY_CONFIGURATION_INSTANCE_COUNT,
    DECODED_FEATURE_BOUNDED_CURVE_SEGMENT_COUNT,
    DECODED_FEATURE_CENTERED_LINE_SEGMENT_COUNT,
    DECODED_FEATURE_CIRCLE_SEGMENT_COUNT,
    DECODED_FEATURE_DIMENSION_COUNT,
    DECODED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT,
    DECODED_FEATURE_DIMENSION_DRIVEN_GUESS_COUNT,
    DECODED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT,
    DECODED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT,
    DECODED_FEATURE_LINE_SEGMENT_COUNT,
    DECODED_FEATURE_LOOP_HISTORY_ENTRY_COUNT,
    DECODED_FEATURE_POINT_SEGMENT_COUNT,
    DECODED_FEATURE_REFERENCE_LINE_SEGMENT_COUNT,
    DECODED_FEATURE_RELATION_COUNT,
    DECODED_FEATURE_RELATION_TRIPLE_COUNT,
    DECODED_FEATURE_SEGMENT_ROW_COUNT,
    DECODED_FEATURE_SKAMP_COUNT,
    DECODED_FEATURE_SOLVER_VARIABLE_COUNT,
    DECODED_FEATURE_SURFACE_REPLAY_ASSOCIATION_COUNT,
    DECODED_TYPE26_FIVE_COORDINATE_ENVELOPE_COUNT,
    DECODED_TYPE26_REPLAYED_MINOR_RADIUS_COUNT,
    DECODED_TYPE26_SPLIT_COORDINATE_ENVELOPE_COUNT,
    EVALUATED_ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
    EVALUATED_ACTIVE_CURVE_EXPRESSION_SOLVE_BLOCK_COUNT,
    EVALUATED_ACTIVE_CURVE_EXPRESSION_SOLVE_VARIABLE_COUNT,
    INACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
    MALFORMED_FEATURE_RELATION_TABLE_COUNT,
    MISSING_FEATURE_RELATION_ROW_COUNT,
    MISSING_FEATURE_RELATION_TRIPLE_ROW_COUNT,
    MISSING_FEATURE_SEGMENT_ROW_COUNT,
    MISSING_FEATURE_SKAMP_ROW_COUNT,
    MISSING_FEATURE_SOLVER_VARIABLE_COUNT,
    PROHIBITED_ACTIVE_CURVE_EXPRESSION_KIND_COUNT,
    PROHIBITED_ACTIVE_CURVE_EXPRESSION_RECORD_COUNT,
    RESOLVED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT,
    RESOLVED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT,
    RESOLVED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT,
    RESOLVED_FEATURE_DIMENSION_VALUE_COUNT,
    RESOLVED_FEATURE_LINE_SEGMENT_GEOMETRY_COUNT,
    RESOLVED_FEATURE_SEGMENT_GEOMETRY_COUNT,
    RETAINED_UNKNOWN_VISIBLE_CURVE_ROW_COUNT,
    RETAINED_UNKNOWN_VISIBLE_SURFACE_ROW_COUNT,
    TRANSFERRED_CHAMFER_FEATURE_COUNT,
    TRANSFERRED_CONFIGURATION_DRIVER_TABLE_COUNT,
    TRANSFERRED_CURVE_EXPRESSION_PARAMETER_COUNT,
    TRANSFERRED_DRAFT_FEATURE_COUNT,
    TRANSFERRED_EXPLICITLY_UNRESOLVED_FEATURE_COUNT,
    TRANSFERRED_EXTRUDE_FEATURE_COUNT,
    TRANSFERRED_FEATURE_COUNT,
    TRANSFERRED_FEATURE_RESULT_EDGE_COUNT,
    TRANSFERRED_FEATURE_RESULT_TOPOLOGY_COUNT,
    TRANSFERRED_FEATURE_DIMENSION_PARAMETER_COUNT,
    TRANSFERRED_FILLET_FEATURE_COUNT,
    TRANSFERRED_FIRST_INSTANCE_PROTOTYPE_SURFACE_COUNT,
    TRANSFERRED_GEOMETRY_GENERATOR_FEATURE_COUNT,
    TRANSFERRED_HOLE_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_CHAMFER_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_DRAFT_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_EXTRUDE_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_EXTRUDE_START_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_EXTRUDE_TERMINATION_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_FILLET_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_HOLE_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_KNIT_SURFACE_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_OTHER_CONSTRUCTION_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_PATTERN_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_REVOLVE_EXTENT_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_REVOLVE_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_SURFACE_OPERATION_FEATURE_COUNT,
    TRANSFERRED_INCOMPLETE_SWEEP_FEATURE_COUNT,
    TRANSFERRED_KNIT_SURFACE_FEATURE_COUNT,
    TRANSFERRED_NATIVE_AXIS_HELIX_FEATURE_COUNT,
    TRANSFERRED_NATIVE_CHAMFER_EDGE_SELECTION_FEATURE_COUNT,
    TRANSFERRED_NATIVE_DRAFT_FACE_SELECTION_FEATURE_COUNT,
    TRANSFERRED_NATIVE_DRAFT_NEUTRAL_PLANE_FEATURE_COUNT,
    TRANSFERRED_NATIVE_EXTRUDE_PROFILE_FEATURE_COUNT,
    TRANSFERRED_NATIVE_FEATURE_COUNT,
    TRANSFERRED_NATIVE_FILLET_EDGE_SELECTION_FEATURE_COUNT,
    TRANSFERRED_NATIVE_HOLE_FACE_SELECTION_FEATURE_COUNT,
    TRANSFERRED_NATIVE_HOLE_PROFILE_FEATURE_COUNT,
    TRANSFERRED_NATIVE_REVOLVE_PROFILE_FEATURE_COUNT,
    TRANSFERRED_PAIRED_ENVELOPE_SPHERE_COUNT,
    TRANSFERRED_PATTERN_FEATURE_COUNT,
    TRANSFERRED_POSITIONAL_CYLINDER_COUNT,
    TRANSFERRED_POSITIONAL_TORUS_COUNT,
    TRANSFERRED_REFERENCE_ELLIPSE_COUNT,
    TRANSFERRED_REVOLVE_FEATURE_COUNT,
    TRANSFERRED_TYPED_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_BOUNDARY_SURFACE_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_CHAMFER_EDGE_SELECTION_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_CHAMFER_SPEC_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_DATUM_COORDINATE_SYSTEM_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_DATUM_PLANE_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_EXTRUDE_BOOLEAN_OPERATION_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_EXTRUDE_PROFILE_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_FILLET_EDGE_SELECTION_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_FILLET_RADIUS_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_FILLET_RADIUS_WITH_GENERATED_SURFACE_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_FILLET_RADIUS_WITHOUT_GENERATED_SURFACE_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_HOLE_FACE_SELECTION_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_HOLE_PROFILE_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_KNIT_SURFACE_FACES_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_KNIT_SURFACE_MERGE_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_KNIT_SURFACE_SOLID_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_PATTERN_SEED_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_PATTERN_TRANSFORM_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_REVOLVE_AXIS_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_REVOLVE_BOOLEAN_OPERATION_FEATURE_COUNT,
    TRANSFERRED_UNRESOLVED_REVOLVE_PROFILE_FEATURE_COUNT,
    TRANSFERRED_VARIABLE_RADIUS_FILLET_FEATURE_COUNT,
    TRANSFERRED_VISIBLE_PLANE_SURFACE_ROW_COUNT,
    UNRESOLVED_ACTIVE_CURVE_EXPRESSION_SOLVE_CONTROL_COUNT,
    UNRESOLVED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT,
    UNRESOLVED_FEATURE_DIMENSION_DRIVEN_GUESS_COUNT,
    UNRESOLVED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT,
    UNRESOLVED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT,
    UNRESOLVED_FEATURE_DIMENSION_VALUE_COUNT,
    UNRESOLVED_FEATURE_LINE_SEGMENT_GEOMETRY_COUNT,
    UNRESOLVED_FEATURE_SEGMENT_GEOMETRY_COUNT,
    UNTRANSFERRED_VISIBLE_CURVE_ROW_COUNT,
    UNTRANSFERRED_VISIBLE_CYLINDER_SURFACE_ROW_COUNT,
    UNTRANSFERRED_VISIBLE_PLANE_SURFACE_ROW_COUNT,
    UNTRANSFERRED_VISIBLE_SURFACE_ROW_COUNT,
    VISIBLE_PLANE_SURFACE_ROW_COUNT,
];

#[cfg(test)]
mod tests {
    use super::ALL;
    use std::collections::BTreeSet;

    #[test]
    fn coverage_keys_are_unique() {
        let unique = ALL
            .iter()
            .map(cadmpeg_ir::CoverageKey::as_str)
            .collect::<BTreeSet<_>>();
        assert_eq!(unique.len(), ALL.len());
    }
}
