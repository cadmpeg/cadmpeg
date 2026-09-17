// SPDX-License-Identifier: Apache-2.0
//! The named readers every optional feature-record key is declared with.
//!
//! The declarations live beside their records rather than among them: the
//! record file is at the production-size limit `docs/source-policy.md` states.

use super::{
    ConstructionRecipeSelector, DesignAssemblyAlignment, DesignBaseFeatureConstruction,
    DesignBaseFlangeOperation, DesignCircularPatternConstruction, DesignCoilExtent,
    DesignCoilPlacement, DesignCoilSection, DesignCoilSectionPlacement, DesignCoilTransform,
    DesignCombineExternalBodyIdentity, DesignCombineOperation, DesignComponentInsertConstruction,
    DesignComponentPatternOccurrencesWire, DesignCopyPasteBodiesOperation,
    DesignCopyPasteComponentOperation, DesignDerivedInstanceConstruction,
    DesignDirectFaceOperation, DesignDraftOperation, DesignEdgeFlangeOperation,
    DesignExtrudeOperation, DesignExtrudePrologue, DesignFixedChamferParameters,
    DesignFixedExtrudeDistance, DesignFixedExtrudeParameters, DesignFixedExtrudeScalar,
    DesignFixedFilletParameters, DesignFixedFilletScalar, DesignHemOperation,
    DesignHoleConstruction, DesignHoleFaceSelection, DesignMirrorConstruction,
    DesignMirrorScopeTolerance, DesignMoveOperation, DesignPathFeatureConstruction,
    DesignRectangularPatternConstruction, DesignRectangularPatternInstances, DesignRelaxedGuidText,
    DesignRuledSurfaceOperation, DesignScaleOperation, DesignSketchProfileOperand,
    DesignSolidPrimitive, DesignSurfaceExtendOperation, DesignSurfaceOffsetOperation,
    DesignSurfaceStitchOperation, DesignThreadConstruction, DesignWorkAxisConstruction,
    DesignWorkAxisSource, DesignWorkPlaneConstruction, DesignWorkPointConstruction,
    DesignWorkPointInputCarrier, Point3, SketchPlacementMatrix, Vector3,
};

cadmpeg_core::named_optional_field!(pub(super) deserialize_operation_prefix_marker, u8, "operation_prefix_marker");
cadmpeg_core::named_optional_field!(pub(super) deserialize_operation_prefix_marker_offset, u64, "operation_prefix_marker_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_secondary_identity, u64, "secondary_identity");
cadmpeg_core::named_optional_field!(pub(super) deserialize_curve_secondary_identity, u64, "curve_secondary_identity");
cadmpeg_core::named_optional_field!(pub(super) deserialize_design_id, String, "design_id");
cadmpeg_core::named_optional_field!(pub(super) deserialize_design_selector, ConstructionRecipeSelector, "design_selector");
cadmpeg_core::named_optional_field!(pub(super) deserialize_transform_offset, u64, "transform_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_transform, SketchPlacementMatrix, "transform");
cadmpeg_core::named_optional_field!(pub(super) deserialize_along_distance, DesignFixedExtrudeDistance, "along_distance");
cadmpeg_core::named_optional_field!(pub(super) deserialize_taper_angle, DesignFixedExtrudeScalar, "taper_angle");
cadmpeg_core::named_optional_field!(pub(super) deserialize_tangency_weight, DesignFixedFilletScalar, "tangency_weight");
cadmpeg_core::named_optional_field!(pub(super) deserialize_instances, DesignRectangularPatternInstances, "instances");
cadmpeg_core::named_optional_field!(pub(super) deserialize_component_occurrences, DesignComponentPatternOccurrencesWire, "component_occurrences");
cadmpeg_core::named_optional_field!(pub(super) deserialize_occurrence_identity, u64, "occurrence_identity");
cadmpeg_core::named_optional_field!(pub(super) deserialize_carrier_transform_offset, u64, "carrier_transform_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_stitch_tolerance_record_index, u32, "stitch_tolerance_record_index");
cadmpeg_core::named_optional_field!(pub(super) deserialize_stitch_tolerance_scope, DesignMirrorScopeTolerance, "stitch_tolerance_scope");
cadmpeg_core::named_optional_field!(pub(super) deserialize_seed_feature_scope_record_index, u32, "seed_feature_scope_record_index");
cadmpeg_core::named_optional_field!(pub(super) deserialize_seed_feature_reference_offset, u64, "seed_feature_reference_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_plane_scope_record_index, u32, "plane_scope_record_index");
cadmpeg_core::named_optional_field!(pub(super) deserialize_plane_reference_offset, u64, "plane_reference_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_plane_selection_record_index, u32, "plane_selection_record_index");
cadmpeg_core::named_optional_field!(pub(super) deserialize_plane_origin, Point3, "plane_origin");
cadmpeg_core::named_optional_field!(pub(super) deserialize_plane_normal, Vector3, "plane_normal");
cadmpeg_core::named_optional_field!(pub(super) deserialize_repeated_marker_offset, u64, "repeated_marker_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_opposite_angle_record_index, u32, "opposite_angle_record_index");
cadmpeg_core::named_optional_field!(pub(super) deserialize_opposite_angle_offset, u64, "opposite_angle_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_external_property_key, DesignRelaxedGuidText, "external_property_key");
cadmpeg_core::named_optional_field!(pub(super) deserialize_external_property_key_offset, u64, "external_property_key_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_external_version_urn, String, "external_version_urn");
cadmpeg_core::named_optional_field!(pub(super) deserialize_external_version_urn_offset, u64, "external_version_urn_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_external_identity, DesignCombineExternalBodyIdentity, "external_identity");
cadmpeg_core::named_optional_field!(pub(super) deserialize_trailing_reference_record_index, u32, "trailing_reference_record_index");
cadmpeg_core::named_optional_field!(pub(super) deserialize_trailing_reference_offset, u64, "trailing_reference_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_source, DesignWorkAxisSource, "source");
cadmpeg_core::named_optional_field!(pub(super) deserialize_carrier, Box<DesignWorkPointInputCarrier>, "carrier");
cadmpeg_core::named_optional_field!(pub(super) deserialize_recipe_state_id, i64, "recipe_state_id");
cadmpeg_core::named_optional_field!(pub(super) deserialize_resolved_vertex_slot, i64, "resolved_vertex_slot");
cadmpeg_core::named_optional_field!(pub(super) deserialize_tangent_point_data, [f64; 3], "tangent_point_data");
cadmpeg_core::named_optional_field!(pub(super) deserialize_tangent_point_data_prefix, u8, "tangent_point_data_prefix");
cadmpeg_core::named_optional_field!(pub(super) deserialize_tangent_point_data_offset, u64, "tangent_point_data_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_face_selection, DesignHoleFaceSelection, "face_selection");
cadmpeg_core::named_optional_field!(pub(super) deserialize_secondary_identity_offset, u64, "secondary_identity_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_curve_secondary_identity_offset, u64, "curve_secondary_identity_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_history_state_id, i64, "history_state_id");
cadmpeg_core::named_optional_field!(pub(super) deserialize_previous_history_state_id, i64, "previous_history_state_id");
cadmpeg_core::named_optional_field!(pub(super) deserialize_solid_primitive, DesignSolidPrimitive, "solid_primitive");
cadmpeg_core::named_optional_field!(pub(super) deserialize_direct_face_operation, DesignDirectFaceOperation, "direct_face_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_move_operation, DesignMoveOperation, "move_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_scale_operation, DesignScaleOperation, "scale_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_surface_stitch_operation, DesignSurfaceStitchOperation, "surface_stitch_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_surface_extend_operation, DesignSurfaceExtendOperation, "surface_extend_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_surface_offset_operation, DesignSurfaceOffsetOperation, "surface_offset_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_ruled_surface_operation, DesignRuledSurfaceOperation, "ruled_surface_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_edge_flange_operation, DesignEdgeFlangeOperation, "edge_flange_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_hem_operation, DesignHemOperation, "hem_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_fixed_fillet_parameters, DesignFixedFilletParameters, "fixed_fillet_parameters");
cadmpeg_core::named_optional_field!(pub(super) deserialize_fixed_chamfer_parameters, DesignFixedChamferParameters, "fixed_chamfer_parameters");
cadmpeg_core::named_optional_field!(pub(super) deserialize_combine_operation, DesignCombineOperation, "combine_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_thread_construction, DesignThreadConstruction, "thread_construction");
cadmpeg_core::named_optional_field!(pub(super) deserialize_draft_operation, DesignDraftOperation, "draft_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_circular_pattern_construction, DesignCircularPatternConstruction, "circular_pattern_construction");
cadmpeg_core::named_optional_field!(pub(super) deserialize_rectangular_pattern_construction, DesignRectangularPatternConstruction, "rectangular_pattern_construction");
cadmpeg_core::named_optional_field!(pub(super) deserialize_assembly_alignment, DesignAssemblyAlignment, "assembly_alignment");
cadmpeg_core::named_optional_field!(pub(super) deserialize_component_insert_construction, DesignComponentInsertConstruction, "component_insert_construction");
cadmpeg_core::named_optional_field!(pub(super) deserialize_derived_instance_construction, DesignDerivedInstanceConstruction, "derived_instance_construction");
cadmpeg_core::named_optional_field!(pub(super) deserialize_copy_paste_component_operation, DesignCopyPasteComponentOperation, "copy_paste_component_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_mirror_construction, DesignMirrorConstruction, "mirror_construction");
cadmpeg_core::named_optional_field!(pub(super) deserialize_copy_paste_bodies_operation, DesignCopyPasteBodiesOperation, "copy_paste_bodies_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_base_feature_construction, DesignBaseFeatureConstruction, "base_feature_construction");
cadmpeg_core::named_optional_field!(pub(super) deserialize_work_axis_construction, DesignWorkAxisConstruction, "work_axis_construction");
cadmpeg_core::named_optional_field!(pub(super) deserialize_work_point_construction, DesignWorkPointConstruction, "work_point_construction");
cadmpeg_core::named_optional_field!(pub(super) deserialize_hole_construction, DesignHoleConstruction, "hole_construction");
cadmpeg_core::named_optional_field!(pub(super) deserialize_work_plane_transform, SketchPlacementMatrix, "work_plane_transform");
cadmpeg_core::named_optional_field!(pub(super) deserialize_work_plane_transform_offset, u64, "work_plane_transform_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_work_plane_reference, u32, "work_plane_reference");
cadmpeg_core::named_optional_field!(pub(super) deserialize_work_plane_reference_offset, u64, "work_plane_reference_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_work_plane_construction, DesignWorkPlaneConstruction, "work_plane_construction");
cadmpeg_core::named_optional_field!(pub(super) deserialize_joint_origin_transform, SketchPlacementMatrix, "joint_origin_transform");
cadmpeg_core::named_optional_field!(pub(super) deserialize_joint_origin_transform_offset, u64, "joint_origin_transform_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_joint_origin_reference, u32, "joint_origin_reference");
cadmpeg_core::named_optional_field!(pub(super) deserialize_joint_origin_reference_offset, u64, "joint_origin_reference_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_entity_id, String, "entity_id");
cadmpeg_core::named_optional_field!(pub(super) deserialize_entity_suffix, u64, "entity_suffix");
cadmpeg_core::named_optional_field!(pub(super) deserialize_entity_reference_offset, u64, "entity_reference_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_base_flange_operation, DesignBaseFlangeOperation, "base_flange_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_base_flange_profile, DesignSketchProfileOperand, "base_flange_profile");
cadmpeg_core::named_optional_field!(pub(super) deserialize_extrude_prologue, DesignExtrudePrologue, "extrude_prologue");
cadmpeg_core::named_optional_field!(pub(super) deserialize_fixed_extrude_parameters, DesignFixedExtrudeParameters, "fixed_extrude_parameters");
cadmpeg_core::named_optional_field!(pub(super) deserialize_extrude_profile, DesignSketchProfileOperand, "extrude_profile");
cadmpeg_core::named_optional_field!(pub(super) deserialize_path_feature_construction, DesignPathFeatureConstruction, "path_feature_construction");
cadmpeg_core::named_optional_field!(pub(super) deserialize_sweep_profile, DesignSketchProfileOperand, "sweep_profile");
cadmpeg_core::named_optional_field!(pub(super) deserialize_coil_operation, DesignExtrudeOperation, "coil_operation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_coil_operation_offset, u64, "coil_operation_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_coil_extent, DesignCoilExtent, "coil_extent");
cadmpeg_core::named_optional_field!(pub(super) deserialize_coil_extent_offset, u64, "coil_extent_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_coil_section, DesignCoilSection, "coil_section");
cadmpeg_core::named_optional_field!(pub(super) deserialize_coil_section_offset, u64, "coil_section_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_coil_section_placement, DesignCoilSectionPlacement, "coil_section_placement");
cadmpeg_core::named_optional_field!(pub(super) deserialize_coil_section_placement_offset, u64, "coil_section_placement_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_coil_clockwise, bool, "coil_clockwise");
cadmpeg_core::named_optional_field!(pub(super) deserialize_coil_clockwise_offset, u64, "coil_clockwise_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_coil_placement, DesignCoilPlacement, "coil_placement");
cadmpeg_core::named_optional_field!(pub(super) deserialize_coil_transform, DesignCoilTransform, "coil_transform");
cadmpeg_core::named_optional_field!(pub(super) deserialize_center_position, [f64; 3], "center_position");
cadmpeg_core::named_optional_field!(pub(super) deserialize_center_position_offset, u64, "center_position_offset");
