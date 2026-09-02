// SPDX-License-Identifier: Apache-2.0
//! Statically declared decode-coverage measures.

#![cfg_attr(not(test), allow(dead_code))]

use cadmpeg_ir::CoverageKey;

pub(crate) const AMBIGUOUS_FORMULA_PARAMETER_DEPENDENCY_COUNT: CoverageKey =
    CoverageKey::new("ambiguous_formula_parameter_dependency_count");
pub(crate) const ATTACHED_STANDARD_TOPOLOGY_COUNT: CoverageKey =
    CoverageKey::new("attached_standard_topology_count");
pub(crate) const ATTEMPTED_STANDARD_TOPOLOGY_COUNT: CoverageKey =
    CoverageKey::new("attempted_standard_topology_count");
pub(crate) const CLASSIFIED_DESIGN_OBJECT_COUNT: CoverageKey =
    CoverageKey::new("classified_design_object_count");
pub(crate) const DECODED_ATOM_ENTITY_SUFFIX_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_atom_entity_suffix_value_count");
pub(crate) const DECODED_CLASSIFIED_CONSTRAINT_RANGE_SOURCE_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_classified_constraint_range_source_entity_count");
pub(crate) const DECODED_CLASSIFIED_RANGE_INTERVAL_SOURCE_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_classified_range_interval_source_entity_count");
pub(crate) const DECODED_CLASSIFIED_FORMULA_EXPRESSION_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_classified_formula_expression_entity_count");
pub(crate) const DECODED_CLASSIFIED_FORMULA_OUTPUT_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_classified_formula_output_entity_count");
pub(crate) const DECODED_CLASSIFIED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT: CoverageKey =
    CoverageKey::new("decoded_classified_formula_parameter_dependency_candidate_count");
pub(crate) const DECODED_CLASSIFIED_RELATION_PROGRAM_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_classified_relation_program_entity_count");
pub(crate) const DECODED_CLASSIFIED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_classified_relation_program_reference_incidence_count");
pub(crate) const DECODED_CLASSIFIED_RELATION_PROGRAM_REPEATED_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_classified_relation_program_repeated_entity_count");
pub(crate) const DECODED_CLASSIFIED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_classified_schema_configuration_entity_reference_count");
pub(crate) const DECODED_CLASSIFIED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT: CoverageKey =
    CoverageKey::new("decoded_classified_schema_configuration_row_chain_terminal_count");
pub(crate) const DECODED_COMPLETE_SCHEMA_CONFIGURATION_ROW_CHAIN_COUNT: CoverageKey =
    CoverageKey::new("decoded_complete_schema_configuration_row_chain_count");
pub(crate) const DECODED_COMPLEX_CONSTRAINT_RANGE_COUNT: CoverageKey =
    CoverageKey::new("decoded_complex_constraint_range_count");
pub(crate) const DECODED_CONSOLIDATED_CONE_FACE_COUNT: CoverageKey =
    CoverageKey::new("decoded_consolidated_cone_face_count");
pub(crate) const DECODED_CONSOLIDATED_CONE_FACE_PARAMETER_POINT_COUNT: CoverageKey =
    CoverageKey::new("decoded_consolidated_cone_face_parameter_point_count");
pub(crate) const DECODED_CONSOLIDATED_EDGE_RUN_COUNT: CoverageKey =
    CoverageKey::new("decoded_consolidated_edge_run_count");
pub(crate) const DECODED_CONSOLIDATED_EDGE_RUN_ENDPOINT_LOCUS_COUNT: CoverageKey =
    CoverageKey::new("decoded_consolidated_edge_run_endpoint_locus_count");
pub(crate) const DECODED_CONSOLIDATED_EDGE_RUN_SHARED_LOCUS_COUNT: CoverageKey =
    CoverageKey::new("decoded_consolidated_edge_run_shared_locus_count");
pub(crate) const DECODED_CONSOLIDATED_EDGE_RUN_SUPPORT_BINDING_COUNT: CoverageKey =
    CoverageKey::new("decoded_consolidated_edge_run_support_binding_count");
pub(crate) const DECODED_CONSOLIDATED_LINE_PROFILE_COUNT: CoverageKey =
    CoverageKey::new("decoded_consolidated_line_profile_count");
pub(crate) const DECODED_CONSOLIDATED_PLANE_CARRIER_COUNT: CoverageKey =
    CoverageKey::new("decoded_consolidated_plane_carrier_count");
pub(crate) const DECODED_CONSTRAINT_RANGE_COUNT: CoverageKey =
    CoverageKey::new("decoded_constraint_range_count");
pub(crate) const DECODED_CONSTRAINT_RANGE_INCOMING_PAYLOAD_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_constraint_range_incoming_payload_reference_count");
pub(crate) const DECODED_CONSTRAINT_RANGE_INCOMING_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_constraint_range_incoming_reference_count");
pub(crate) const DECODED_CONSTRAINT_RANGE_INCOMING_STORAGE_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_constraint_range_incoming_storage_reference_count");
pub(crate) const DECODED_CONTROL_E8_ENTITY_SUFFIX_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_control_e8_entity_suffix_value_count");
pub(crate) const DECODED_CONTROL_E9_ENTITY_SUFFIX_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_control_e9_entity_suffix_value_count");
pub(crate) const DECODED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_control_entity_suffix_value_count");
pub(crate) const DECODED_DEFINITION_CHAIN_ATOM_COUNT: CoverageKey =
    CoverageKey::new("decoded_definition_chain_atom_count");
pub(crate) const DECODED_DEFINITION_CHAIN_EVALUATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_definition_chain_evaluation_count");
pub(crate) const DECODED_DEFINITION_CHAIN_SCHEMA_SELECTOR_COUNT: CoverageKey =
    CoverageKey::new("decoded_definition_chain_schema_selector_count");
pub(crate) const DECODED_DEFINITION_CHAIN_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_definition_chain_value_count");
pub(crate) const DECODED_DEFINITION_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_definition_value_count");
pub(crate) const DECODED_DESIGN_FIELD_COUNT: CoverageKey =
    CoverageKey::new("decoded_design_field_count");
pub(crate) const DECODED_DESIGN_OBJECT_COUNT: CoverageKey =
    CoverageKey::new("decoded_design_object_count");
pub(crate) const DECODED_DESIGN_OBJECT_OWNER_LINK_COUNT: CoverageKey =
    CoverageKey::new("decoded_design_object_owner_link_count");
pub(crate) const DECODED_DESIGN_OBJECT_RELATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_design_object_relation_count");
pub(crate) const DECODED_DESIGN_REFLEXIVE_FIELD_RELATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_design_reflexive_field_relation_count");
pub(crate) const DECODED_DESIGN_SAME_OBJECT_RELATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_design_same_object_relation_count");
pub(crate) const DECODED_DESIGN_UNOWNED_FIELD_RELATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_design_unowned_field_relation_count");
pub(crate) const DECODED_DIMENSION_CONSTRAINT_RANGE_COUNT: CoverageKey =
    CoverageKey::new("decoded_dimension_constraint_range_count");
pub(crate) const DECODED_DISTINCT_RELATION_PROGRAM_INPUT_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_distinct_relation_program_input_entity_count");
pub(crate) const DECODED_EVALUATED_CONSTRAINT_RANGE_COUNT: CoverageKey =
    CoverageKey::new("decoded_evaluated_constraint_range_count");
pub(crate) const DECODED_EVALUATED_DEFINITION_CHAIN_COUNT: CoverageKey =
    CoverageKey::new("decoded_evaluated_definition_chain_count");
pub(crate) const DECODED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT: CoverageKey =
    CoverageKey::new("decoded_formula_parameter_dependency_candidate_count");
pub(crate) const DECODED_FORMULA_PARAMETER_DEPENDENCY_COUNT: CoverageKey =
    CoverageKey::new("decoded_formula_parameter_dependency_count");
pub(crate) const DECODED_FORMULA_REFERENCED_RELATION_EXPRESSION_COUNT: CoverageKey =
    CoverageKey::new("decoded_formula_referenced_relation_expression_count");
pub(crate) const DECODED_FORMULA_RELATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_formula_relation_count");
pub(crate) const DECODED_INSTANCED_RELATION_EXPRESSION_COUNT: CoverageKey =
    CoverageKey::new("decoded_instanced_relation_expression_count");
pub(crate) const DECODED_LEAD12_RELATION_PROGRAM_INSTANCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_lead12_relation_program_instance_count");
pub(crate) const DECODED_LEAD12_RELATION_PROGRAM_PARAMOUT_CONTEXT_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_lead12_relation_program_paramout_context_entity_count");
pub(crate) const DECODED_LEAD54_RELATION_PROGRAM_INSTANCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_lead54_relation_program_instance_count");
pub(crate) const DECODED_LEGACY_ASYNCHRONOUS_RELATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_asynchronous_relation_count");
pub(crate) const DECODED_LEGACY_E3_ROLE_TAIL_TEXT_FIELD_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_e3_role_tail_text_field_count");
pub(crate) const DECODED_LEGACY_ENTITY_IDENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_entity_identity_count");
pub(crate) const DECODED_LEGACY_ENTITY_RUN_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_entity_run_count");
pub(crate) const DECODED_LEGACY_IDENTITY_LEAD_81_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_identity_lead_81_count");
pub(crate) const DECODED_LEGACY_IDENTITY_LEAD_82_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_identity_lead_82_count");
pub(crate) const DECODED_LEGACY_IDENTITY_LEAD_E5_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_identity_lead_e5_count");
pub(crate) const DECODED_LEGACY_IDENTITY_LEAD_FD_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_identity_lead_fd_count");
pub(crate) const DECODED_LEGACY_INTEGER_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_integer_value_count");
pub(crate) const DECODED_LEGACY_NAMED_INTEGER_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_named_integer_value_count");
pub(crate) const DECODED_LEGACY_NAMED_STRING_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_named_string_value_count");
pub(crate) const DECODED_LEGACY_RELATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_relation_count");
pub(crate) const DECODED_LEGACY_ROLE_FIELD_BINDING_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_role_field_binding_count");
pub(crate) const DECODED_LEGACY_ROLE_SELECTOR_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_role_selector_count");
pub(crate) const DECODED_LEGACY_ROLE_TEXT_FIELD_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_role_text_field_count");
pub(crate) const DECODED_LEGACY_SCHEMA_FIELD_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_schema_field_count");
pub(crate) const DECODED_LEGACY_SELECTED_ROLE_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_selected_role_count");
pub(crate) const DECODED_LEGACY_STRING_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_string_value_count");
pub(crate) const DECODED_LEGACY_SYNCHRONOUS_RELATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_synchronous_relation_count");
pub(crate) const DECODED_LEGACY_SYNCHRONOUS_STATE_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_synchronous_state_count");
pub(crate) const DECODED_LEGACY_TEXT_FIELD_COUNT: CoverageKey =
    CoverageKey::new("decoded_legacy_text_field_count");
pub(crate) const DECODED_MULTI_MEMBER_REFERENCE_SIGNATURE_COHORT_COUNT: CoverageKey =
    CoverageKey::new("decoded_multi_member_reference_signature_cohort_count");
pub(crate) const DECODED_NULL_FORMULA_OUTPUT_COUNT: CoverageKey =
    CoverageKey::new("decoded_null_formula_output_count");
pub(crate) const DECODED_NULL_OBJECT_RECORD_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_null_object_record_reference_count");
pub(crate) const DECODED_NULL_REFERENCE_SIGNATURE_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_null_reference_signature_entity_count");
pub(crate) const DECODED_NULL_RELATION_PROGRAM_INSTANCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_null_relation_program_instance_count");
pub(crate) const DECODED_NULL_RELATION_PROGRAM_OUTPUT_COUNT: CoverageKey =
    CoverageKey::new("decoded_null_relation_program_output_count");
pub(crate) const DECODED_NULL_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_null_relation_program_reference_incidence_count");
pub(crate) const DECODED_NULL_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_null_relation_program_repeated_reference_count");
pub(crate) const DECODED_NULL_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_null_schema_configuration_entity_reference_count");
pub(crate) const DECODED_NULL_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT: CoverageKey =
    CoverageKey::new("decoded_null_schema_configuration_row_chain_terminal_count");
pub(crate) const DECODED_NULL_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT: CoverageKey =
    CoverageKey::new("decoded_null_schema_configuration_row_class_count");
pub(crate) const DECODED_NULL_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT: CoverageKey =
    CoverageKey::new("decoded_null_schema_configuration_row_successor_count");
pub(crate) const DECODED_NUMERIC_ENTITY_VALUE_PACKET_COUNT: CoverageKey =
    CoverageKey::new("decoded_numeric_entity_value_packet_count");
pub(crate) const DECODED_NUMERIC_ENTITY_VALUE_PAIR_COUNT: CoverageKey =
    CoverageKey::new("decoded_numeric_entity_value_pair_count");
pub(crate) const DECODED_OBJECT_GRAPH_COUNT: CoverageKey =
    CoverageKey::new("decoded_object_graph_count");
pub(crate) const DECODED_OBJECT_RECORD_COUNT: CoverageKey =
    CoverageKey::new("decoded_object_record_count");
pub(crate) const DECODED_OPENED_BOOLEAN_PARSER_VERSION_RELATION_EXPRESSION_COUNT: CoverageKey =
    CoverageKey::new("decoded_opened_boolean_parser_version_relation_expression_count");
pub(crate) const DECODED_ORDERED_SCHEMA_CONFIGURATION_ROW_LINK_COUNT: CoverageKey =
    CoverageKey::new("decoded_ordered_schema_configuration_row_link_count");
pub(crate) const DECODED_OTHER_LEAD12_RELATION_PROGRAM_CONTEXT_CLASS_COUNT: CoverageKey =
    CoverageKey::new("decoded_other_lead12_relation_program_context_class_count");
pub(crate) const DECODED_OTHER_RELATION_PROGRAM_INSTANCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_other_relation_program_instance_count");
pub(crate) const DECODED_OWNED_DEFINITION_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_owned_definition_value_count");
pub(crate) const DECODED_PARSER_VERSION_RELATION_EXPRESSION_COUNT: CoverageKey =
    CoverageKey::new("decoded_parser_version_relation_expression_count");
pub(crate) const DECODED_PROGRAM_REFERENCED_RELATION_EXPRESSION_COUNT: CoverageKey =
    CoverageKey::new("decoded_program_referenced_relation_expression_count");
pub(crate) const DECODED_RANGE_INTERVAL_COUNT: CoverageKey =
    CoverageKey::new("decoded_range_interval_count");
pub(crate) const DECODED_RANGE_INTERVAL_FINITE_SLOT_COUNT: CoverageKey =
    CoverageKey::new("decoded_range_interval_finite_slot_count");
pub(crate) const DECODED_RANGE_INTERVAL_INCOMING_PAYLOAD_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_range_interval_incoming_payload_reference_count");
pub(crate) const DECODED_RANGE_INTERVAL_INCOMING_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_range_interval_incoming_reference_count");
pub(crate) const DECODED_RANGE_INTERVAL_INCOMING_STORAGE_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_range_interval_incoming_storage_reference_count");
pub(crate) const DECODED_RANGE_INTERVAL_NO_SLOT_COUNT: CoverageKey =
    CoverageKey::new("decoded_range_interval_no_slot_count");
pub(crate) const DECODED_RANGE_INTERVAL_NOMINAL_COUNT: CoverageKey =
    CoverageKey::new("decoded_range_interval_nominal_count");
pub(crate) const DECODED_RANGE_INTERVAL_UNSET_SLOT_COUNT: CoverageKey =
    CoverageKey::new("decoded_range_interval_unset_slot_count");
pub(crate) const DECODED_REFERENCE_SIGNATURE_COHORT_COUNT: CoverageKey =
    CoverageKey::new("decoded_reference_signature_cohort_count");
pub(crate) const DECODED_REFERENCE_SIGNATURE_COHORT_MEMBER_COUNT: CoverageKey =
    CoverageKey::new("decoded_reference_signature_cohort_member_count");
pub(crate) const DECODED_REFERENCE_SIGNATURE_COUNT: CoverageKey =
    CoverageKey::new("decoded_reference_signature_count");
pub(crate) const DECODED_REFERENCE_SIGNATURE_INSTRUCTION_COUNT: CoverageKey =
    CoverageKey::new("decoded_reference_signature_instruction_count");
pub(crate) const DECODED_REFERENCE_SIGNATURE_PREFIX_ATOM_2_COUNT: CoverageKey =
    CoverageKey::new("decoded_reference_signature_prefix_atom_2_count");
pub(crate) const DECODED_REFERENCE_SIGNATURE_PREFIX_ATOM_35_COUNT: CoverageKey =
    CoverageKey::new("decoded_reference_signature_prefix_atom_35_count");
pub(crate) const DECODED_REFERENCE_SIGNATURE_TOKEN_COUNT: CoverageKey =
    CoverageKey::new("decoded_reference_signature_token_count");
pub(crate) const DECODED_REFERENCED_RELATION_EXPRESSION_COUNT: CoverageKey =
    CoverageKey::new("decoded_referenced_relation_expression_count");
pub(crate) const DECODED_RELATION_EXPRESSION_COUNT: CoverageKey =
    CoverageKey::new("decoded_relation_expression_count");
pub(crate) const DECODED_RELATION_EXPRESSION_PROGRAM_INSTANCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_relation_expression_program_instance_count");
pub(crate) const DECODED_RELATION_PROGRAM_INSTANCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_relation_program_instance_count");
pub(crate) const DECODED_RELATION_PROGRAM_OUTPUT_COUNT: CoverageKey =
    CoverageKey::new("decoded_relation_program_output_count");
pub(crate) const DECODED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT: CoverageKey =
    CoverageKey::new("decoded_relation_program_parameter_dependency_count");
pub(crate) const DECODED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_relation_program_reference_incidence_count");
pub(crate) const DECODED_RESOLVED_FORMULA_OUTPUT_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_formula_output_count");
pub(crate) const DECODED_RESOLVED_FORMULA_PARAMETER_DEPENDENCY_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_formula_parameter_dependency_count");
pub(crate) const DECODED_RESOLVED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_lead12_relation_program_context_entity_count");
pub(crate) const DECODED_RESOLVED_LEAD54_RELATION_PROGRAM_TRAILING_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_lead54_relation_program_trailing_entity_count");
pub(crate) const DECODED_RESOLVED_REFERENCE_SIGNATURE_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_reference_signature_entity_count");
pub(crate) const DECODED_RESOLVED_RELATION_PROGRAM_INPUT_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_relation_program_input_count");
pub(crate) const DECODED_RESOLVED_RELATION_PROGRAM_INPUT_INSTANCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_relation_program_input_instance_count");
pub(crate) const DECODED_RESOLVED_RELATION_PROGRAM_INSTANCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_relation_program_instance_count");
pub(crate) const DECODED_RESOLVED_RELATION_PROGRAM_OUTPUT_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_relation_program_output_count");
pub(crate) const DECODED_RESOLVED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_relation_program_reference_incidence_count");
pub(crate) const DECODED_RESOLVED_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_relation_program_repeated_reference_count");
pub(crate) const DECODED_RESOLVED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_schema_configuration_entity_reference_count");
pub(crate) const DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_schema_configuration_row_chain_terminal_count");
pub(crate) const DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_schema_configuration_row_class_count");
pub(crate) const DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT: CoverageKey =
    CoverageKey::new("decoded_resolved_schema_configuration_row_successor_count");
pub(crate) const DECODED_SCALAR_ENTITY_SUFFIX_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_scalar_entity_suffix_value_count");
pub(crate) const DECODED_SCHEMA_CONFIGURATION_RECORD_COUNT: CoverageKey =
    CoverageKey::new("decoded_schema_configuration_record_count");
pub(crate) const DECODED_SCHEMA_CONFIGURATION_ROW_INTERVENING_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_schema_configuration_row_intervening_entity_count");
pub(crate) const DECODED_SCHEMA_CONFIGURATION_ROW_INTERVENING_SCHEMA_CONFIGURATION_COUNT:
    CoverageKey =
    CoverageKey::new("decoded_schema_configuration_row_intervening_schema_configuration_count");
pub(crate) const DECODED_SCHEMA_CONFIGURATION_ROW_LINK_COUNT: CoverageKey =
    CoverageKey::new("decoded_schema_configuration_row_link_count");
pub(crate) const DECODED_SCHEMA_CONFIGURATION_SELECTOR_COUNT: CoverageKey =
    CoverageKey::new("decoded_schema_configuration_selector_count");
pub(crate) const DECODED_SCHEMA_SELECTED_ATOM_ENTITY_SUFFIX_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_schema_selected_atom_entity_suffix_value_count");
pub(crate) const DECODED_SCHEMA_SELECTED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_schema_selected_control_entity_suffix_value_count");
pub(crate) const DECODED_SCHEMA_SELECTED_ENTITY_SUFFIX_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_schema_selected_entity_suffix_value_count");
pub(crate) const DECODED_SCHEMA_SELECTED_EVALUATION_ENTITY_SUFFIX_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_schema_selected_evaluation_entity_suffix_value_count");
pub(crate) const DECODED_SCHEMA_SELECTED_REFERENCE_SIGNATURE_COHORT_COUNT: CoverageKey =
    CoverageKey::new("decoded_schema_selected_reference_signature_cohort_count");
pub(crate) const DECODED_SCHEMA_SELECTED_SCHEMA_ENTITY_SUFFIX_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_schema_selected_schema_entity_suffix_value_count");
pub(crate) const DECODED_SCHEMA_SELECTED_SEPARATOR_ENTITY_SUFFIX_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_schema_selected_separator_entity_suffix_value_count");
pub(crate) const DECODED_SEPARATOR_ENTITY_SUFFIX_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_separator_entity_suffix_value_count");
pub(crate) const DECODED_STRUCTURALLY_OWNED_DEFINITION_CHAIN_EVALUATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_structurally_owned_definition_chain_evaluation_count");
pub(crate) const DECODED_STRUCTURALLY_OWNED_DEFINITION_CHAIN_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_structurally_owned_definition_chain_value_count");
pub(crate) const DECODED_TYPED_RELATION_EXPRESSION_COUNT: CoverageKey =
    CoverageKey::new("decoded_typed_relation_expression_count");
pub(crate) const DECODED_TYPED_RELATION_PROGRAM_INSTANCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_typed_relation_program_instance_count");
pub(crate) const DECODED_UNASSIGNED_DEFINITION_CHAIN_EVALUATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_unassigned_definition_chain_evaluation_count");
pub(crate) const DECODED_UNASSIGNED_DEFINITION_CHAIN_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_unassigned_definition_chain_value_count");
pub(crate) const DECODED_UNASSIGNED_OBJECT_OWNER_SLOT_COUNT: CoverageKey =
    CoverageKey::new("decoded_unassigned_object_owner_slot_count");
pub(crate) const DECODED_UNRESOLVED_REFERENCE_SIGNATURE_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("decoded_unresolved_reference_signature_entity_count");
pub(crate) const DECODED_UNSET_CONSTRAINT_RANGE_COUNT: CoverageKey =
    CoverageKey::new("decoded_unset_constraint_range_count");
pub(crate) const DECODED_UNSET_DEFINITION_CHAIN_COUNT: CoverageKey =
    CoverageKey::new("decoded_unset_definition_chain_count");
pub(crate) const DECODED_UNSET_ENTITY_SUFFIX_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_unset_entity_suffix_value_count");
pub(crate) const DECODED_WIDE_PREFIX_ENTITY_SUFFIX_VALUE_COUNT: CoverageKey =
    CoverageKey::new("decoded_wide_prefix_entity_suffix_value_count");
pub(crate) const DECODED_ZERO_ENTITY_EDGE_STRIDE_ALLOCATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_edge_stride_allocation_count");
pub(crate) const DECODED_ZERO_ENTITY_EDGE_STRIDE_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_edge_stride_count");
pub(crate) const DECODED_ZERO_ENTITY_EDGE_STRIDE_SURFACE_SUPPORT_REF_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_edge_stride_surface_support_ref_count");
pub(crate) const DECODED_ZERO_ENTITY_EDGE_STRIDE_TOPOLOGY_REF_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_edge_stride_topology_ref_count");
pub(crate) const DECODED_ZERO_ENTITY_FACE_BOUND_SUPPORT_RUN_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_face_bound_support_run_count");
pub(crate) const DECODED_ZERO_ENTITY_FACE_TERMINAL_CONTROL_03_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_face_terminal_control_03_count");
pub(crate) const DECODED_ZERO_ENTITY_FACE_TERMINAL_CONTROL_05_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_face_terminal_control_05_count");
pub(crate) const DECODED_ZERO_ENTITY_FORWARD_LOOP_MEMBER_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_forward_loop_member_count");
pub(crate) const DECODED_ZERO_ENTITY_LOOP_CLASS_41_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_loop_class_41_count");
pub(crate) const DECODED_ZERO_ENTITY_LOOP_CLASS_50_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_loop_class_50_count");
pub(crate) const DECODED_ZERO_ENTITY_LOOP_CLASS_C1_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_loop_class_c1_count");
pub(crate) const DECODED_ZERO_ENTITY_LOOP_RECORD_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_loop_record_count");
pub(crate) const DECODED_ZERO_ENTITY_LOOP_TERMINAL_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_loop_terminal_count");
pub(crate) const DECODED_ZERO_ENTITY_MODEL_MIDPOINT_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_model_midpoint_count");
pub(crate) const DECODED_ZERO_ENTITY_ORIENTED_LOOP_MEMBER_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_oriented_loop_member_count");
pub(crate) const DECODED_ZERO_ENTITY_ORIENTED_USE_ALLOCATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_oriented_use_allocation_count");
pub(crate) const DECODED_ZERO_ENTITY_ORIENTED_USE_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_oriented_use_count");
pub(crate) const DECODED_ZERO_ENTITY_ORIENTED_USE_PAIR_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_oriented_use_pair_count");
pub(crate) const DECODED_ZERO_ENTITY_RECORD_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_record_count");
pub(crate) const DECODED_ZERO_ENTITY_REVERSED_LOOP_MEMBER_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_reversed_loop_member_count");
pub(crate) const DECODED_ZERO_ENTITY_SUPPORT_MODEL_CONSTRUCTION_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_support_model_construction_count");
pub(crate) const DECODED_ZERO_ENTITY_SUPPORT_MODEL_CURVE_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_support_model_curve_count");
pub(crate) const DECODED_ZERO_ENTITY_SUPPORT_OCCURRENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_support_occurrence_count");
pub(crate) const DECODED_ZERO_ENTITY_SUPPORT_PCURVE_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_support_pcurve_count");
pub(crate) const DECODED_ZERO_ENTITY_SUPPORT_RUN_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_support_run_count");
pub(crate) const DECODED_ZERO_ENTITY_UV_ENDPOINT_PAIR_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_uv_endpoint_pair_count");
pub(crate) const DECODED_ZERO_ENTITY_VERTEX_INCIDENCE_ALLOCATION_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_vertex_incidence_allocation_count");
pub(crate) const DECODED_ZERO_ENTITY_VERTEX_INCIDENCE_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_vertex_incidence_count");
pub(crate) const DECODED_ZERO_ENTITY_VERTEX_OWNER_BINDING_COUNT: CoverageKey =
    CoverageKey::new("decoded_zero_entity_vertex_owner_binding_count");
pub(crate) const FULLY_RESOLVED_CONSOLIDATED_EDGE_RUN_COUNT: CoverageKey =
    CoverageKey::new("fully_resolved_consolidated_edge_run_count");
pub(crate) const MODELING_OBJECT_GRAPH_COUNT: CoverageKey =
    CoverageKey::new("modeling_object_graph_count");
pub(crate) const MODELING_OBJECT_RECORD_COUNT: CoverageKey =
    CoverageKey::new("modeling_object_record_count");
pub(crate) const MULTIPLY_REFERENCED_CONSTRAINT_RANGE_COUNT: CoverageKey =
    CoverageKey::new("multiply_referenced_constraint_range_count");
pub(crate) const MULTIPLY_REFERENCED_RANGE_INTERVAL_COUNT: CoverageKey =
    CoverageKey::new("multiply_referenced_range_interval_count");
pub(crate) const PARTIALLY_RESOLVED_CONSOLIDATED_EDGE_RUN_COUNT: CoverageKey =
    CoverageKey::new("partially_resolved_consolidated_edge_run_count");
pub(crate) const REFINED_CONSOLIDATED_ANALYTIC_SURFACE_COUNT: CoverageKey =
    CoverageKey::new("refined_consolidated_analytic_surface_count");
pub(crate) const RESOLVED_OBJECT_STREAM_CLASS_21_PCURVE_SUFFIX_SCALAR_COUNT: CoverageKey =
    CoverageKey::new("resolved_object_stream_class_21_pcurve_suffix_scalar_count");
pub(crate) const RESOLVED_OBJECT_STREAM_EXTENDED_LOOP_METADATA_COUNT: CoverageKey =
    CoverageKey::new("resolved_object_stream_extended_loop_metadata_count");
pub(crate) const RESOLVED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_03_COUNT: CoverageKey =
    CoverageKey::new("resolved_object_stream_face_terminal_control_03_count");
pub(crate) const RESOLVED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_05_COUNT: CoverageKey =
    CoverageKey::new("resolved_object_stream_face_terminal_control_05_count");
pub(crate) const RESOLVED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_05_05_COUNT: CoverageKey =
    CoverageKey::new("resolved_object_stream_loop_framing_controls_05_05_count");
pub(crate) const RESOLVED_OBJECT_STREAM_UNCOUNTED_FACE_COUNT: CoverageKey =
    CoverageKey::new("resolved_object_stream_uncounted_face_count");
pub(crate) const RETAINED_UNSCOPED_OBJECT_GRAPH_COUNT: CoverageKey =
    CoverageKey::new("retained_unscoped_object_graph_count");
pub(crate) const RETAINED_UNSCOPED_OBJECT_RECORD_COUNT: CoverageKey =
    CoverageKey::new("retained_unscoped_object_record_count");
pub(crate) const STANDARD_TOPOLOGY_CURVE_SUPPORT_COUNT: CoverageKey =
    CoverageKey::new("standard_topology_curve_support_count");
pub(crate) const STANDARD_TOPOLOGY_EMPTY_ENDPOINT_DOMAIN_COUNT: CoverageKey =
    CoverageKey::new("standard_topology_empty_endpoint_domain_count");
pub(crate) const STANDARD_TOPOLOGY_FAILURE_AMBIGUOUS_SOLUTION_COUNT: CoverageKey =
    CoverageKey::new("standard_topology_failure_ambiguous_solution_count");
pub(crate) const STANDARD_TOPOLOGY_FAILURE_SEARCH_EXHAUSTED_COUNT: CoverageKey =
    CoverageKey::new("standard_topology_failure_search_exhausted_count");
pub(crate) const STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_BOUNDARY_RECONSTRUCTION_COUNT: CoverageKey = CoverageKey::new("standard_topology_mesh_rejection_endpoint_incidence_boundary_reconstruction_count");
pub(crate) const STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_COUNT: CoverageKey =
    CoverageKey::new("standard_topology_mesh_rejection_endpoint_incidence_count");
pub(crate) const STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_NO_ASSIGNMENT_COUNT:
    CoverageKey =
    CoverageKey::new("standard_topology_mesh_rejection_endpoint_incidence_no_assignment_count");
pub(crate) const STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_CHOICE_PRUNING_COUNT: CoverageKey =
    CoverageKey::new("standard_topology_mesh_rejection_incidence_choice_pruning_count");
pub(crate) const STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_COMPONENT_COMPOSITION_COUNT:
    CoverageKey =
    CoverageKey::new("standard_topology_mesh_rejection_incidence_component_composition_count");
pub(crate) const STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_COMPONENT_DOMAIN_COUNT: CoverageKey =
    CoverageKey::new("standard_topology_mesh_rejection_incidence_component_domain_count");
pub(crate) const STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_FIXED_ASSIGNMENT_COUNT: CoverageKey =
    CoverageKey::new("standard_topology_mesh_rejection_incidence_fixed_assignment_count");
pub(crate) const STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_INPUT_SHAPE_COUNT: CoverageKey =
    CoverageKey::new("standard_topology_mesh_rejection_incidence_input_shape_count");
pub(crate) const STANDARD_TOPOLOGY_MULTIPLE_ENDPOINT_DOMAIN_COUNT: CoverageKey =
    CoverageKey::new("standard_topology_multiple_endpoint_domain_count");
pub(crate) const STANDARD_TOPOLOGY_SINGLETON_ENDPOINT_DOMAIN_COUNT: CoverageKey =
    CoverageKey::new("standard_topology_singleton_endpoint_domain_count");
pub(crate) const TRANSFERRED_CONFIGURATION_COUNT: CoverageKey =
    CoverageKey::new("transferred_configuration_count");
pub(crate) const TRANSFERRED_CONSOLIDATED_LINE_PROFILE_COUNT: CoverageKey =
    CoverageKey::new("transferred_consolidated_line_profile_count");
pub(crate) const TRANSFERRED_CONSOLIDATED_REVOLUTION_COUNT: CoverageKey =
    CoverageKey::new("transferred_consolidated_revolution_count");
pub(crate) const TRANSFERRED_DEFINITION_CHAIN_PARAMETER_COUNT: CoverageKey =
    CoverageKey::new("transferred_definition_chain_parameter_count");
pub(crate) const TRANSFERRED_FEATURE_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_count");
pub(crate) const TRANSFERRED_FEATURE_PARENT_COUNT: CoverageKey =
    CoverageKey::new("transferred_feature_parent_count");
pub(crate) const TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT: CoverageKey =
    CoverageKey::new("transferred_formula_design_record_count");
pub(crate) const TRANSFERRED_LEGACY_FORMULA_COUNT: CoverageKey =
    CoverageKey::new("transferred_legacy_formula_count");
pub(crate) const TRANSFERRED_LEGACY_PARAMETER_COUNT: CoverageKey =
    CoverageKey::new("transferred_legacy_parameter_count");
pub(crate) const TRANSFERRED_LEGACY_SELECTOR_PARAMETER_COUNT: CoverageKey =
    CoverageKey::new("transferred_legacy_selector_parameter_count");
pub(crate) const TRANSFERRED_NATIVE_OPERATION_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_operation_count");
pub(crate) const TRANSFERRED_NATIVE_OPERATION_DEFINITION_CHAIN_VALUE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_operation_definition_chain_value_count");
pub(crate) const TRANSFERRED_NATIVE_OPERATION_DEFINITION_VALUE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_operation_definition_value_count");
pub(crate) const TRANSFERRED_NATIVE_OPERATION_RANGE_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_operation_range_count");
pub(crate) const TRANSFERRED_NATIVE_OPERATION_PARAMETER_COUNT: CoverageKey =
    CoverageKey::new("transferred_native_operation_parameter_count");
pub(crate) const TRANSFERRED_OBJECT_STREAM_FACE_COUNT: CoverageKey =
    CoverageKey::new("transferred_object_stream_face_count");
pub(crate) const TRANSFERRED_OBJECT_STREAM_LOOP_COUNT: CoverageKey =
    CoverageKey::new("transferred_object_stream_loop_count");
pub(crate) const TRANSFERRED_PARAMETER_COUNT: CoverageKey =
    CoverageKey::new("transferred_parameter_count");
pub(crate) const TRANSFERRED_PMI_DIMENSION_COUNT: CoverageKey =
    CoverageKey::new("transferred_pmi_dimension_count");
pub(crate) const TRANSFERRED_RELATION_PROGRAM_INPUT_PARAMETER_COUNT: CoverageKey =
    CoverageKey::new("transferred_relation_program_input_parameter_count");
pub(crate) const TRANSFERRED_SKETCH_CONSTRAINT_COUNT: CoverageKey =
    CoverageKey::new("transferred_sketch_constraint_count");
pub(crate) const TRANSFERRED_SKETCH_COUNT: CoverageKey =
    CoverageKey::new("transferred_sketch_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_WIRE_BODY_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_wire_body_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_OWNED_WIRE_BODY_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_owned_wire_body_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_WIRE_EDGE_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_wire_edge_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_WIRE_LOOP_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_wire_loop_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_PARAMETRIC_SURFACE_CURVE_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_parametric_surface_curve_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_WIRE_POINT_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_wire_point_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_SUPPORT_CURVE_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_support_curve_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_WIRE_VERTEX_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_wire_vertex_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_TOPOLOGY_BODY_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_topology_body_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_TOPOLOGY_COEDGE_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_topology_coedge_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_TOPOLOGY_EDGE_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_topology_edge_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_TOPOLOGY_FACE_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_topology_face_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_TOPOLOGY_LOOP_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_topology_loop_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_TOPOLOGY_POINT_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_topology_point_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_TOPOLOGY_PCURVE_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_topology_pcurve_count");
pub(crate) const TRANSFERRED_ZERO_ENTITY_TOPOLOGY_VERTEX_COUNT: CoverageKey =
    CoverageKey::new("transferred_zero_entity_topology_vertex_count");
pub(crate) const TYPED_OBJECT_STREAM_CLASS_21_PCURVE_SUFFIX_SCALAR_COUNT: CoverageKey =
    CoverageKey::new("typed_object_stream_class_21_pcurve_suffix_scalar_count");
pub(crate) const TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_21_COUNT: CoverageKey =
    CoverageKey::new("typed_object_stream_edge_terminal_control_21_count");
pub(crate) const TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_2A_COUNT: CoverageKey =
    CoverageKey::new("typed_object_stream_edge_terminal_control_2a_count");
pub(crate) const TYPED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_03_COUNT: CoverageKey =
    CoverageKey::new("typed_object_stream_face_terminal_control_03_count");
pub(crate) const TYPED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_05_COUNT: CoverageKey =
    CoverageKey::new("typed_object_stream_face_terminal_control_05_count");
pub(crate) const TYPED_MULTI_SURFACE_OBJECT_STREAM_FACE_COUNT: CoverageKey =
    CoverageKey::new("typed_multi_surface_object_stream_face_count");
pub(crate) const TYPED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_05_05_COUNT: CoverageKey =
    CoverageKey::new("typed_object_stream_loop_framing_controls_05_05_count");
pub(crate) const TYPED_OBJECT_STREAM_PARAMETER_INCIDENCE_COUNT: CoverageKey =
    CoverageKey::new("typed_object_stream_parameter_incidence_count");
pub(crate) const TYPED_OBJECT_STREAM_PARAMETER_INCIDENCE_MEMBER_COUNT: CoverageKey =
    CoverageKey::new("typed_object_stream_parameter_incidence_member_count");
pub(crate) const TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_ROSTER_COUNT: CoverageKey =
    CoverageKey::new("typed_object_stream_vertex_incidence_roster_count");
pub(crate) const TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_ROSTER_MEMBER_COUNT: CoverageKey =
    CoverageKey::new("typed_object_stream_vertex_incidence_roster_member_count");
pub(crate) const TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_TERMINAL_CONTROL_04_COUNT: CoverageKey =
    CoverageKey::new("typed_object_stream_vertex_incidence_terminal_control_04_count");
pub(crate) const TYPED_UNRESOLVED_OBJECT_STREAM_FACE_COUNT: CoverageKey =
    CoverageKey::new("typed_unresolved_object_stream_face_count");
pub(crate) const TYPED_UNRESOLVED_OBJECT_STREAM_LOOP_COUNT: CoverageKey =
    CoverageKey::new("typed_unresolved_object_stream_loop_count");
pub(crate) const UNCLASSIFIED_CONSTRAINT_RANGE_SOURCE_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("unclassified_constraint_range_source_entity_count");
pub(crate) const UNCLASSIFIED_RANGE_INTERVAL_SOURCE_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("unclassified_range_interval_source_entity_count");
pub(crate) const UNCLASSIFIED_FORMULA_EXPRESSION_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("unclassified_formula_expression_entity_count");
pub(crate) const UNCLASSIFIED_FORMULA_OUTPUT_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("unclassified_formula_output_entity_count");
pub(crate) const UNCLASSIFIED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT: CoverageKey =
    CoverageKey::new("unclassified_formula_parameter_dependency_candidate_count");
pub(crate) const UNCLASSIFIED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("unclassified_lead12_relation_program_context_entity_count");
pub(crate) const UNCLASSIFIED_RELATION_PROGRAM_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("unclassified_relation_program_entity_count");
pub(crate) const UNCLASSIFIED_RELATION_PROGRAM_REPEATED_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("unclassified_relation_program_repeated_entity_count");
pub(crate) const UNCLASSIFIED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("unclassified_schema_configuration_entity_reference_count");
pub(crate) const UNIQUELY_REFERENCED_CONSTRAINT_RANGE_COUNT: CoverageKey =
    CoverageKey::new("uniquely_referenced_constraint_range_count");
pub(crate) const UNIQUELY_REFERENCED_RANGE_INTERVAL_COUNT: CoverageKey =
    CoverageKey::new("uniquely_referenced_range_interval_count");
pub(crate) const UNREFERENCED_CONSTRAINT_RANGE_COUNT: CoverageKey =
    CoverageKey::new("unreferenced_constraint_range_count");
pub(crate) const UNREFERENCED_RANGE_INTERVAL_COUNT: CoverageKey =
    CoverageKey::new("unreferenced_range_interval_count");
pub(crate) const UNRESOLVED_CONSOLIDATED_EDGE_RUN_COUNT: CoverageKey =
    CoverageKey::new("unresolved_consolidated_edge_run_count");
pub(crate) const UNRESOLVED_DEFINITION_CHAIN_EVALUATION_OWNER_COUNT: CoverageKey =
    CoverageKey::new("unresolved_definition_chain_evaluation_owner_count");
pub(crate) const UNRESOLVED_DEFINITION_CHAIN_VALUE_OWNER_COUNT: CoverageKey =
    CoverageKey::new("unresolved_definition_chain_value_owner_count");
pub(crate) const UNRESOLVED_DEFINITION_VALUE_OWNER_COUNT: CoverageKey =
    CoverageKey::new("unresolved_definition_value_owner_count");
pub(crate) const UNRESOLVED_DESIGN_OWNER_COUNT: CoverageKey =
    CoverageKey::new("unresolved_design_owner_count");
pub(crate) const UNRESOLVED_DESIGN_RECORD_COUNT: CoverageKey =
    CoverageKey::new("unresolved_design_record_count");
pub(crate) const UNRESOLVED_FORMULA_OUTPUT_COUNT: CoverageKey =
    CoverageKey::new("unresolved_formula_output_count");
pub(crate) const UNRESOLVED_FORMULA_PARAMETER_DEPENDENCY_COUNT: CoverageKey =
    CoverageKey::new("unresolved_formula_parameter_dependency_count");
pub(crate) const UNRESOLVED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("unresolved_lead12_relation_program_context_entity_count");
pub(crate) const UNRESOLVED_LEAD54_RELATION_PROGRAM_TRAILING_ENTITY_COUNT: CoverageKey =
    CoverageKey::new("unresolved_lead54_relation_program_trailing_entity_count");
pub(crate) const UNRESOLVED_OBJECT_RECORD_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_object_record_reference_count");
pub(crate) const UNRESOLVED_RELATION_PROGRAM_INPUT_INSTANCE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_relation_program_input_instance_count");
pub(crate) const UNRESOLVED_RELATION_PROGRAM_INSTANCE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_relation_program_instance_count");
pub(crate) const UNRESOLVED_RELATION_PROGRAM_OUTPUT_COUNT: CoverageKey =
    CoverageKey::new("unresolved_relation_program_output_count");
pub(crate) const UNRESOLVED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT: CoverageKey =
    CoverageKey::new("unresolved_relation_program_parameter_dependency_count");
pub(crate) const UNRESOLVED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_relation_program_reference_incidence_count");
pub(crate) const UNRESOLVED_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_relation_program_repeated_reference_count");
pub(crate) const UNRESOLVED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT: CoverageKey =
    CoverageKey::new("unresolved_schema_configuration_entity_reference_count");
pub(crate) const UNRESOLVED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT: CoverageKey =
    CoverageKey::new("unresolved_schema_configuration_row_chain_terminal_count");
pub(crate) const UNRESOLVED_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT: CoverageKey =
    CoverageKey::new("unresolved_schema_configuration_row_class_count");
pub(crate) const UNRESOLVED_SCHEMA_CONFIGURATION_ROW_ORDER_COUNT: CoverageKey =
    CoverageKey::new("unresolved_schema_configuration_row_order_count");
pub(crate) const UNRESOLVED_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT: CoverageKey =
    CoverageKey::new("unresolved_schema_configuration_row_successor_count");
pub(crate) const UNRESOLVED_STORAGE_RECORD_COUNT: CoverageKey =
    CoverageKey::new("unresolved_storage_record_count");
pub(crate) const UNRESOLVED_UNREFERENCED_RELATION_EXPRESSION_COUNT: CoverageKey =
    CoverageKey::new("unresolved_unreferenced_relation_expression_count");

pub(crate) const ALL: &[CoverageKey] = &[
    AMBIGUOUS_FORMULA_PARAMETER_DEPENDENCY_COUNT,
    ATTACHED_STANDARD_TOPOLOGY_COUNT,
    ATTEMPTED_STANDARD_TOPOLOGY_COUNT,
    CLASSIFIED_DESIGN_OBJECT_COUNT,
    DECODED_ATOM_ENTITY_SUFFIX_VALUE_COUNT,
    DECODED_CLASSIFIED_CONSTRAINT_RANGE_SOURCE_ENTITY_COUNT,
    DECODED_CLASSIFIED_RANGE_INTERVAL_SOURCE_ENTITY_COUNT,
    DECODED_CLASSIFIED_FORMULA_EXPRESSION_ENTITY_COUNT,
    DECODED_CLASSIFIED_FORMULA_OUTPUT_ENTITY_COUNT,
    DECODED_CLASSIFIED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT,
    DECODED_CLASSIFIED_RELATION_PROGRAM_ENTITY_COUNT,
    DECODED_CLASSIFIED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT,
    DECODED_CLASSIFIED_RELATION_PROGRAM_REPEATED_ENTITY_COUNT,
    DECODED_CLASSIFIED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT,
    DECODED_CLASSIFIED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT,
    DECODED_COMPLETE_SCHEMA_CONFIGURATION_ROW_CHAIN_COUNT,
    DECODED_COMPLEX_CONSTRAINT_RANGE_COUNT,
    DECODED_CONSOLIDATED_CONE_FACE_COUNT,
    DECODED_CONSOLIDATED_CONE_FACE_PARAMETER_POINT_COUNT,
    DECODED_CONSOLIDATED_EDGE_RUN_COUNT,
    DECODED_CONSOLIDATED_EDGE_RUN_ENDPOINT_LOCUS_COUNT,
    DECODED_CONSOLIDATED_EDGE_RUN_SHARED_LOCUS_COUNT,
    DECODED_CONSOLIDATED_EDGE_RUN_SUPPORT_BINDING_COUNT,
    DECODED_CONSOLIDATED_LINE_PROFILE_COUNT,
    DECODED_CONSOLIDATED_PLANE_CARRIER_COUNT,
    DECODED_CONSTRAINT_RANGE_COUNT,
    DECODED_CONSTRAINT_RANGE_INCOMING_PAYLOAD_REFERENCE_COUNT,
    DECODED_CONSTRAINT_RANGE_INCOMING_REFERENCE_COUNT,
    DECODED_CONSTRAINT_RANGE_INCOMING_STORAGE_REFERENCE_COUNT,
    DECODED_CONTROL_E8_ENTITY_SUFFIX_VALUE_COUNT,
    DECODED_CONTROL_E9_ENTITY_SUFFIX_VALUE_COUNT,
    DECODED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT,
    DECODED_DEFINITION_CHAIN_ATOM_COUNT,
    DECODED_DEFINITION_CHAIN_EVALUATION_COUNT,
    DECODED_DEFINITION_CHAIN_SCHEMA_SELECTOR_COUNT,
    DECODED_DEFINITION_CHAIN_VALUE_COUNT,
    DECODED_DEFINITION_VALUE_COUNT,
    DECODED_DESIGN_FIELD_COUNT,
    DECODED_DESIGN_OBJECT_COUNT,
    DECODED_DESIGN_OBJECT_OWNER_LINK_COUNT,
    DECODED_DESIGN_OBJECT_RELATION_COUNT,
    DECODED_DESIGN_REFLEXIVE_FIELD_RELATION_COUNT,
    DECODED_DESIGN_SAME_OBJECT_RELATION_COUNT,
    DECODED_DESIGN_UNOWNED_FIELD_RELATION_COUNT,
    DECODED_DIMENSION_CONSTRAINT_RANGE_COUNT,
    DECODED_DISTINCT_RELATION_PROGRAM_INPUT_ENTITY_COUNT,
    DECODED_EVALUATED_CONSTRAINT_RANGE_COUNT,
    DECODED_EVALUATED_DEFINITION_CHAIN_COUNT,
    DECODED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT,
    DECODED_FORMULA_PARAMETER_DEPENDENCY_COUNT,
    DECODED_FORMULA_REFERENCED_RELATION_EXPRESSION_COUNT,
    DECODED_FORMULA_RELATION_COUNT,
    DECODED_INSTANCED_RELATION_EXPRESSION_COUNT,
    DECODED_LEAD12_RELATION_PROGRAM_INSTANCE_COUNT,
    DECODED_LEAD12_RELATION_PROGRAM_PARAMOUT_CONTEXT_ENTITY_COUNT,
    DECODED_LEAD54_RELATION_PROGRAM_INSTANCE_COUNT,
    DECODED_LEGACY_ASYNCHRONOUS_RELATION_COUNT,
    DECODED_LEGACY_E3_ROLE_TAIL_TEXT_FIELD_COUNT,
    DECODED_LEGACY_ENTITY_IDENTITY_COUNT,
    DECODED_LEGACY_ENTITY_RUN_COUNT,
    DECODED_LEGACY_IDENTITY_LEAD_81_COUNT,
    DECODED_LEGACY_IDENTITY_LEAD_82_COUNT,
    DECODED_LEGACY_IDENTITY_LEAD_E5_COUNT,
    DECODED_LEGACY_IDENTITY_LEAD_FD_COUNT,
    DECODED_LEGACY_INTEGER_VALUE_COUNT,
    DECODED_LEGACY_NAMED_INTEGER_VALUE_COUNT,
    DECODED_LEGACY_NAMED_STRING_VALUE_COUNT,
    DECODED_LEGACY_RELATION_COUNT,
    DECODED_LEGACY_ROLE_FIELD_BINDING_COUNT,
    DECODED_LEGACY_ROLE_SELECTOR_COUNT,
    DECODED_LEGACY_ROLE_TEXT_FIELD_COUNT,
    DECODED_LEGACY_SCHEMA_FIELD_COUNT,
    DECODED_LEGACY_SELECTED_ROLE_COUNT,
    DECODED_LEGACY_STRING_VALUE_COUNT,
    DECODED_LEGACY_SYNCHRONOUS_RELATION_COUNT,
    DECODED_LEGACY_SYNCHRONOUS_STATE_COUNT,
    DECODED_LEGACY_TEXT_FIELD_COUNT,
    DECODED_MULTI_MEMBER_REFERENCE_SIGNATURE_COHORT_COUNT,
    DECODED_NULL_FORMULA_OUTPUT_COUNT,
    DECODED_NULL_OBJECT_RECORD_REFERENCE_COUNT,
    DECODED_NULL_REFERENCE_SIGNATURE_ENTITY_COUNT,
    DECODED_NULL_RELATION_PROGRAM_INSTANCE_COUNT,
    DECODED_NULL_RELATION_PROGRAM_OUTPUT_COUNT,
    DECODED_NULL_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT,
    DECODED_NULL_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT,
    DECODED_NULL_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT,
    DECODED_NULL_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT,
    DECODED_NULL_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT,
    DECODED_NULL_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT,
    DECODED_NUMERIC_ENTITY_VALUE_PACKET_COUNT,
    DECODED_NUMERIC_ENTITY_VALUE_PAIR_COUNT,
    DECODED_OBJECT_GRAPH_COUNT,
    DECODED_OBJECT_RECORD_COUNT,
    DECODED_OPENED_BOOLEAN_PARSER_VERSION_RELATION_EXPRESSION_COUNT,
    DECODED_ORDERED_SCHEMA_CONFIGURATION_ROW_LINK_COUNT,
    DECODED_OTHER_LEAD12_RELATION_PROGRAM_CONTEXT_CLASS_COUNT,
    DECODED_OTHER_RELATION_PROGRAM_INSTANCE_COUNT,
    DECODED_OWNED_DEFINITION_VALUE_COUNT,
    DECODED_PARSER_VERSION_RELATION_EXPRESSION_COUNT,
    DECODED_PROGRAM_REFERENCED_RELATION_EXPRESSION_COUNT,
    DECODED_RANGE_INTERVAL_COUNT,
    DECODED_RANGE_INTERVAL_FINITE_SLOT_COUNT,
    DECODED_RANGE_INTERVAL_INCOMING_PAYLOAD_REFERENCE_COUNT,
    DECODED_RANGE_INTERVAL_INCOMING_REFERENCE_COUNT,
    DECODED_RANGE_INTERVAL_INCOMING_STORAGE_REFERENCE_COUNT,
    DECODED_RANGE_INTERVAL_NO_SLOT_COUNT,
    DECODED_RANGE_INTERVAL_NOMINAL_COUNT,
    DECODED_RANGE_INTERVAL_UNSET_SLOT_COUNT,
    DECODED_REFERENCE_SIGNATURE_COHORT_COUNT,
    DECODED_REFERENCE_SIGNATURE_COHORT_MEMBER_COUNT,
    DECODED_REFERENCE_SIGNATURE_COUNT,
    DECODED_REFERENCE_SIGNATURE_INSTRUCTION_COUNT,
    DECODED_REFERENCE_SIGNATURE_PREFIX_ATOM_2_COUNT,
    DECODED_REFERENCE_SIGNATURE_PREFIX_ATOM_35_COUNT,
    DECODED_REFERENCE_SIGNATURE_TOKEN_COUNT,
    DECODED_REFERENCED_RELATION_EXPRESSION_COUNT,
    DECODED_RELATION_EXPRESSION_COUNT,
    DECODED_RELATION_EXPRESSION_PROGRAM_INSTANCE_COUNT,
    DECODED_RELATION_PROGRAM_INSTANCE_COUNT,
    DECODED_RELATION_PROGRAM_OUTPUT_COUNT,
    DECODED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT,
    DECODED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT,
    DECODED_RESOLVED_FORMULA_OUTPUT_COUNT,
    DECODED_RESOLVED_FORMULA_PARAMETER_DEPENDENCY_COUNT,
    DECODED_RESOLVED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT,
    DECODED_RESOLVED_LEAD54_RELATION_PROGRAM_TRAILING_ENTITY_COUNT,
    DECODED_RESOLVED_REFERENCE_SIGNATURE_ENTITY_COUNT,
    DECODED_RESOLVED_RELATION_PROGRAM_INPUT_COUNT,
    DECODED_RESOLVED_RELATION_PROGRAM_INPUT_INSTANCE_COUNT,
    DECODED_RESOLVED_RELATION_PROGRAM_INSTANCE_COUNT,
    DECODED_RESOLVED_RELATION_PROGRAM_OUTPUT_COUNT,
    DECODED_RESOLVED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT,
    DECODED_RESOLVED_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT,
    DECODED_RESOLVED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT,
    DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT,
    DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT,
    DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT,
    DECODED_SCALAR_ENTITY_SUFFIX_VALUE_COUNT,
    DECODED_SCHEMA_CONFIGURATION_RECORD_COUNT,
    DECODED_SCHEMA_CONFIGURATION_ROW_INTERVENING_ENTITY_COUNT,
    DECODED_SCHEMA_CONFIGURATION_ROW_INTERVENING_SCHEMA_CONFIGURATION_COUNT,
    DECODED_SCHEMA_CONFIGURATION_ROW_LINK_COUNT,
    DECODED_SCHEMA_CONFIGURATION_SELECTOR_COUNT,
    DECODED_SCHEMA_SELECTED_ATOM_ENTITY_SUFFIX_VALUE_COUNT,
    DECODED_SCHEMA_SELECTED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT,
    DECODED_SCHEMA_SELECTED_ENTITY_SUFFIX_VALUE_COUNT,
    DECODED_SCHEMA_SELECTED_EVALUATION_ENTITY_SUFFIX_VALUE_COUNT,
    DECODED_SCHEMA_SELECTED_REFERENCE_SIGNATURE_COHORT_COUNT,
    DECODED_SCHEMA_SELECTED_SCHEMA_ENTITY_SUFFIX_VALUE_COUNT,
    DECODED_SCHEMA_SELECTED_SEPARATOR_ENTITY_SUFFIX_VALUE_COUNT,
    DECODED_SEPARATOR_ENTITY_SUFFIX_VALUE_COUNT,
    DECODED_STRUCTURALLY_OWNED_DEFINITION_CHAIN_EVALUATION_COUNT,
    DECODED_STRUCTURALLY_OWNED_DEFINITION_CHAIN_VALUE_COUNT,
    DECODED_TYPED_RELATION_EXPRESSION_COUNT,
    DECODED_TYPED_RELATION_PROGRAM_INSTANCE_COUNT,
    DECODED_UNASSIGNED_DEFINITION_CHAIN_EVALUATION_COUNT,
    DECODED_UNASSIGNED_DEFINITION_CHAIN_VALUE_COUNT,
    DECODED_UNASSIGNED_OBJECT_OWNER_SLOT_COUNT,
    DECODED_UNRESOLVED_REFERENCE_SIGNATURE_ENTITY_COUNT,
    DECODED_UNSET_CONSTRAINT_RANGE_COUNT,
    DECODED_UNSET_DEFINITION_CHAIN_COUNT,
    DECODED_UNSET_ENTITY_SUFFIX_VALUE_COUNT,
    DECODED_WIDE_PREFIX_ENTITY_SUFFIX_VALUE_COUNT,
    DECODED_ZERO_ENTITY_EDGE_STRIDE_ALLOCATION_COUNT,
    DECODED_ZERO_ENTITY_EDGE_STRIDE_COUNT,
    DECODED_ZERO_ENTITY_FACE_BOUND_SUPPORT_RUN_COUNT,
    DECODED_ZERO_ENTITY_FACE_TERMINAL_CONTROL_03_COUNT,
    DECODED_ZERO_ENTITY_FACE_TERMINAL_CONTROL_05_COUNT,
    DECODED_ZERO_ENTITY_FORWARD_LOOP_MEMBER_COUNT,
    DECODED_ZERO_ENTITY_LOOP_CLASS_41_COUNT,
    DECODED_ZERO_ENTITY_LOOP_CLASS_50_COUNT,
    DECODED_ZERO_ENTITY_LOOP_CLASS_C1_COUNT,
    DECODED_ZERO_ENTITY_LOOP_RECORD_COUNT,
    DECODED_ZERO_ENTITY_LOOP_TERMINAL_COUNT,
    DECODED_ZERO_ENTITY_MODEL_MIDPOINT_COUNT,
    DECODED_ZERO_ENTITY_ORIENTED_LOOP_MEMBER_COUNT,
    DECODED_ZERO_ENTITY_ORIENTED_USE_ALLOCATION_COUNT,
    DECODED_ZERO_ENTITY_ORIENTED_USE_COUNT,
    DECODED_ZERO_ENTITY_ORIENTED_USE_PAIR_COUNT,
    DECODED_ZERO_ENTITY_RECORD_COUNT,
    DECODED_ZERO_ENTITY_REVERSED_LOOP_MEMBER_COUNT,
    DECODED_ZERO_ENTITY_SUPPORT_MODEL_CONSTRUCTION_COUNT,
    DECODED_ZERO_ENTITY_SUPPORT_MODEL_CURVE_COUNT,
    DECODED_ZERO_ENTITY_SUPPORT_OCCURRENCE_COUNT,
    DECODED_ZERO_ENTITY_SUPPORT_PCURVE_COUNT,
    DECODED_ZERO_ENTITY_SUPPORT_RUN_COUNT,
    DECODED_ZERO_ENTITY_UV_ENDPOINT_PAIR_COUNT,
    DECODED_ZERO_ENTITY_VERTEX_INCIDENCE_ALLOCATION_COUNT,
    DECODED_ZERO_ENTITY_VERTEX_INCIDENCE_COUNT,
    DECODED_ZERO_ENTITY_VERTEX_OWNER_BINDING_COUNT,
    FULLY_RESOLVED_CONSOLIDATED_EDGE_RUN_COUNT,
    MODELING_OBJECT_GRAPH_COUNT,
    MODELING_OBJECT_RECORD_COUNT,
    MULTIPLY_REFERENCED_CONSTRAINT_RANGE_COUNT,
    MULTIPLY_REFERENCED_RANGE_INTERVAL_COUNT,
    PARTIALLY_RESOLVED_CONSOLIDATED_EDGE_RUN_COUNT,
    REFINED_CONSOLIDATED_ANALYTIC_SURFACE_COUNT,
    RESOLVED_OBJECT_STREAM_CLASS_21_PCURVE_SUFFIX_SCALAR_COUNT,
    RESOLVED_OBJECT_STREAM_EXTENDED_LOOP_METADATA_COUNT,
    RESOLVED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_03_COUNT,
    RESOLVED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_05_COUNT,
    RESOLVED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_05_05_COUNT,
    RESOLVED_OBJECT_STREAM_UNCOUNTED_FACE_COUNT,
    RETAINED_UNSCOPED_OBJECT_GRAPH_COUNT,
    RETAINED_UNSCOPED_OBJECT_RECORD_COUNT,
    STANDARD_TOPOLOGY_CURVE_SUPPORT_COUNT,
    STANDARD_TOPOLOGY_EMPTY_ENDPOINT_DOMAIN_COUNT,
    STANDARD_TOPOLOGY_FAILURE_AMBIGUOUS_SOLUTION_COUNT,
    STANDARD_TOPOLOGY_FAILURE_SEARCH_EXHAUSTED_COUNT,
    STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_BOUNDARY_RECONSTRUCTION_COUNT,
    STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_COUNT,
    STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_NO_ASSIGNMENT_COUNT,
    STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_CHOICE_PRUNING_COUNT,
    STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_COMPONENT_COMPOSITION_COUNT,
    STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_COMPONENT_DOMAIN_COUNT,
    STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_FIXED_ASSIGNMENT_COUNT,
    STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_INPUT_SHAPE_COUNT,
    STANDARD_TOPOLOGY_MULTIPLE_ENDPOINT_DOMAIN_COUNT,
    STANDARD_TOPOLOGY_SINGLETON_ENDPOINT_DOMAIN_COUNT,
    TRANSFERRED_CONFIGURATION_COUNT,
    TRANSFERRED_CONSOLIDATED_LINE_PROFILE_COUNT,
    TRANSFERRED_CONSOLIDATED_REVOLUTION_COUNT,
    TRANSFERRED_DEFINITION_CHAIN_PARAMETER_COUNT,
    TRANSFERRED_FEATURE_COUNT,
    TRANSFERRED_FEATURE_PARENT_COUNT,
    TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT,
    TRANSFERRED_LEGACY_FORMULA_COUNT,
    TRANSFERRED_LEGACY_PARAMETER_COUNT,
    TRANSFERRED_LEGACY_SELECTOR_PARAMETER_COUNT,
    TRANSFERRED_NATIVE_OPERATION_COUNT,
    TRANSFERRED_NATIVE_OPERATION_DEFINITION_CHAIN_VALUE_COUNT,
    TRANSFERRED_NATIVE_OPERATION_DEFINITION_VALUE_COUNT,
    TRANSFERRED_NATIVE_OPERATION_RANGE_COUNT,
    TRANSFERRED_NATIVE_OPERATION_PARAMETER_COUNT,
    TRANSFERRED_OBJECT_STREAM_FACE_COUNT,
    TRANSFERRED_OBJECT_STREAM_LOOP_COUNT,
    TRANSFERRED_PARAMETER_COUNT,
    TRANSFERRED_PMI_DIMENSION_COUNT,
    TRANSFERRED_RELATION_PROGRAM_INPUT_PARAMETER_COUNT,
    TRANSFERRED_SKETCH_CONSTRAINT_COUNT,
    TRANSFERRED_SKETCH_COUNT,
    TRANSFERRED_ZERO_ENTITY_WIRE_BODY_COUNT,
    TRANSFERRED_ZERO_ENTITY_OWNED_WIRE_BODY_COUNT,
    TRANSFERRED_ZERO_ENTITY_WIRE_EDGE_COUNT,
    TRANSFERRED_ZERO_ENTITY_WIRE_LOOP_COUNT,
    TRANSFERRED_ZERO_ENTITY_PARAMETRIC_SURFACE_CURVE_COUNT,
    TRANSFERRED_ZERO_ENTITY_WIRE_POINT_COUNT,
    TRANSFERRED_ZERO_ENTITY_SUPPORT_CURVE_COUNT,
    TRANSFERRED_ZERO_ENTITY_WIRE_VERTEX_COUNT,
    TRANSFERRED_ZERO_ENTITY_TOPOLOGY_BODY_COUNT,
    TRANSFERRED_ZERO_ENTITY_TOPOLOGY_COEDGE_COUNT,
    TRANSFERRED_ZERO_ENTITY_TOPOLOGY_EDGE_COUNT,
    TRANSFERRED_ZERO_ENTITY_TOPOLOGY_FACE_COUNT,
    TRANSFERRED_ZERO_ENTITY_TOPOLOGY_LOOP_COUNT,
    TRANSFERRED_ZERO_ENTITY_TOPOLOGY_POINT_COUNT,
    TRANSFERRED_ZERO_ENTITY_TOPOLOGY_PCURVE_COUNT,
    TRANSFERRED_ZERO_ENTITY_TOPOLOGY_VERTEX_COUNT,
    TYPED_OBJECT_STREAM_CLASS_21_PCURVE_SUFFIX_SCALAR_COUNT,
    TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_21_COUNT,
    TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_2A_COUNT,
    TYPED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_03_COUNT,
    TYPED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_05_COUNT,
    TYPED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_05_05_COUNT,
    TYPED_OBJECT_STREAM_PARAMETER_INCIDENCE_COUNT,
    TYPED_OBJECT_STREAM_PARAMETER_INCIDENCE_MEMBER_COUNT,
    TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_ROSTER_COUNT,
    TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_ROSTER_MEMBER_COUNT,
    TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_TERMINAL_CONTROL_04_COUNT,
    TYPED_UNRESOLVED_OBJECT_STREAM_FACE_COUNT,
    TYPED_UNRESOLVED_OBJECT_STREAM_LOOP_COUNT,
    UNCLASSIFIED_CONSTRAINT_RANGE_SOURCE_ENTITY_COUNT,
    UNCLASSIFIED_RANGE_INTERVAL_SOURCE_ENTITY_COUNT,
    UNCLASSIFIED_FORMULA_EXPRESSION_ENTITY_COUNT,
    UNCLASSIFIED_FORMULA_OUTPUT_ENTITY_COUNT,
    UNCLASSIFIED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT,
    UNCLASSIFIED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT,
    UNCLASSIFIED_RELATION_PROGRAM_ENTITY_COUNT,
    UNCLASSIFIED_RELATION_PROGRAM_REPEATED_ENTITY_COUNT,
    UNCLASSIFIED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT,
    UNIQUELY_REFERENCED_CONSTRAINT_RANGE_COUNT,
    UNIQUELY_REFERENCED_RANGE_INTERVAL_COUNT,
    UNREFERENCED_CONSTRAINT_RANGE_COUNT,
    UNREFERENCED_RANGE_INTERVAL_COUNT,
    UNRESOLVED_CONSOLIDATED_EDGE_RUN_COUNT,
    UNRESOLVED_DEFINITION_CHAIN_EVALUATION_OWNER_COUNT,
    UNRESOLVED_DEFINITION_CHAIN_VALUE_OWNER_COUNT,
    UNRESOLVED_DEFINITION_VALUE_OWNER_COUNT,
    UNRESOLVED_DESIGN_OWNER_COUNT,
    UNRESOLVED_DESIGN_RECORD_COUNT,
    UNRESOLVED_FORMULA_OUTPUT_COUNT,
    UNRESOLVED_FORMULA_PARAMETER_DEPENDENCY_COUNT,
    UNRESOLVED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT,
    UNRESOLVED_LEAD54_RELATION_PROGRAM_TRAILING_ENTITY_COUNT,
    UNRESOLVED_OBJECT_RECORD_REFERENCE_COUNT,
    UNRESOLVED_RELATION_PROGRAM_INPUT_INSTANCE_COUNT,
    UNRESOLVED_RELATION_PROGRAM_INSTANCE_COUNT,
    UNRESOLVED_RELATION_PROGRAM_OUTPUT_COUNT,
    UNRESOLVED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT,
    UNRESOLVED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT,
    UNRESOLVED_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT,
    UNRESOLVED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT,
    UNRESOLVED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT,
    UNRESOLVED_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT,
    UNRESOLVED_SCHEMA_CONFIGURATION_ROW_ORDER_COUNT,
    UNRESOLVED_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT,
    UNRESOLVED_STORAGE_RECORD_COUNT,
    UNRESOLVED_UNREFERENCED_RELATION_EXPRESSION_COUNT,
];

#[cfg(test)]
mod tests {
    use super::ALL;
    use std::collections::BTreeSet;

    #[test]
    fn coverage_keys_are_unique() {
        let unique = ALL.iter().map(|key| key.as_str()).collect::<BTreeSet<_>>();
        assert_eq!(unique.len(), ALL.len());
    }
}
