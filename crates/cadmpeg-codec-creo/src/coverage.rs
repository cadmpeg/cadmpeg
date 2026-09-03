// SPDX-License-Identifier: Apache-2.0
//! Statically declared decode-coverage measures.

#![cfg_attr(not(test), allow(dead_code))]

use cadmpeg_ir::CoverageKey;

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

pub(crate) const ALL: &[CoverageKey] = &[
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
