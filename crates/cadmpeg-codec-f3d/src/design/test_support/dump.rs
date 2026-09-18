// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

pub(crate) use super::{
    parameter_owner_frame, parameter_record, push_genesis_block, push_reference,
};
pub(crate) use crate::design::decode::dimension_frames::companion_owned_interval;
pub(crate) use crate::design::decode::operands::{
    assign_extrude_face_roles, bind_edge_operand_candidates, bind_extrude_selection_geometry,
    bind_extrude_selection_identities, bind_face_operand_candidates, bind_lost_edge_groups,
    construction_operand_group_is_retained, decode_fillet_radius_groups, face_recipe_program_kind,
    parse_body_recipe_operand, parse_construction_operand_dual_transform,
    parse_construction_operand_flag, parse_construction_operand_group,
    parse_construction_operand_identity, parse_construction_operand_path,
    parse_construction_operand_transform, parse_construction_tracking_path, parse_edge_operand,
    parse_entity_selection_operand, parse_extrude_selection_group, parse_extrude_selection_member,
    parse_face_operand, parse_sketch_profile, parse_vertex_recipe, ConstructionOperandGroupParse,
    FaceRecipeProgramKind,
};
pub(crate) use crate::design::decode::parameters::{
    bind_parameter_companion_payloads, parse_design_parameter_record as parse_design_parameter,
    parse_parameter_owner,
};
pub(crate) use crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment;
pub(crate) use crate::design::decode::scopes::axial_assembly::bind_axial_assembly_operand_targets;
pub(crate) use crate::design::decode::scopes::axial_assembly::bind_joint_origin_frames_from_assemblies;
pub(crate) use crate::design::decode::scopes::base_feature::exact_base_feature_construction;
pub(crate) use crate::design::decode::scopes::combine::exact_combine_operation;
pub(crate) use crate::design::decode::scopes::direct_face::exact_direct_face_operation;
pub(crate) use crate::design::decode::scopes::direct_face::exact_scale_operation;
pub(crate) use crate::design::decode::scopes::draft::exact_draft_operation_with_owners;
pub(crate) use crate::design::decode::scopes::fixed_parameters::exact_fixed_chamfer_parameters;
pub(crate) use crate::design::decode::scopes::fixed_parameters::exact_fixed_extrude_parameters;
pub(crate) use crate::design::decode::scopes::fixed_parameters::exact_fixed_fillet_parameters;
pub(crate) use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
pub(crate) use crate::design::decode::scopes::path_feature::exact_path_feature_construction;
pub(crate) use crate::design::decode::scopes::pattern::exact_circular_pattern_construction_with_owners;
pub(crate) use crate::design::decode::scopes::pattern::exact_rectangular_pattern_construction;
pub(crate) use crate::design::decode::scopes::pattern::select_circular_pattern_axis;
pub(crate) use crate::design::decode::scopes::surfaces::exact_ruled_surface_operation;
pub(crate) use crate::design::decode::scopes::surfaces::exact_surface_extend_operation;
pub(crate) use crate::design::decode::scopes::surfaces::exact_surface_offset_operation;
pub(crate) use crate::design::decode::scopes::surfaces::exact_surface_stitch_operation;
pub(crate) use crate::design::decode::scopes::thread::exact_thread_construction;
pub(crate) use crate::design::decode::scopes::thread::parse_thread_payload;
pub(crate) use crate::design::decode::scopes::work_geometry::exact_joint_origin_frame;
pub(crate) use crate::design::decode::scopes::work_geometry::exact_work_axis_construction;
pub(crate) use crate::design::decode::scopes::work_geometry::exact_work_plane_frame;
pub(crate) use crate::design::decode::sketch::{
    bind_sketch_graph, decode_pattern_definition, next_indexed_record_offset,
    next_indexed_record_offset_with_index, parse_classed_sketch_relation,
    parse_genesis_entity_header, parse_settled_entity_header, parse_sketch_placement_candidates,
    parse_sketch_surface, IndexedRecordOffsets, SketchRelationClass,
};
pub(crate) use crate::design::dimensions::{
    bind_dimension_loci, counted_role_relation, directional_point_dimension,
    exact_atomic_constraint, exact_counted_dimension_relation, exact_counted_offset,
    exact_offset_constraint, expression_identifiers, indirect_angular_lines,
    offset_parameter_factor, owner_scoped_angular_dimension_definition,
    owner_scoped_line_length_dimension_definition, owner_scoped_radial_dimension_definition,
    preceding_incident_angular_dimension_definition, radial_dimension_definition,
    radial_extension_annotation_group, radial_locus_dimension_definition,
    repeated_linear_dimension, spatial_counted_offset_dimension_definition,
    spatial_parallel_line_distance_matches, spatial_point_distance_matches,
    two_locus_distance_dimension, unique_point_class_dimension_definition,
    unresolved_parameter_expression_dependency_count,
};
pub(crate) use crate::design::edge_resolve::feature_input_topology_id;
pub(crate) use crate::design::face_resolve::{
    resolved_body_recipe_shape, resolved_face_group, resolved_historical_split_face_target_group,
};
pub(crate) use crate::design::feature_project::{
    project_combine, project_extrude, project_parameter_design,
    project_parameter_design_with_edge_identities, project_split, untyped_parameter_unit_count,
};
pub(crate) use crate::design::geometry::MAX_ARRANGEMENT_WALK_WORK;
pub(crate) use crate::design::profile_select::{
    bind_extrude_profile_selections, resolved_extrude_profile_selection,
};
pub(crate) use crate::design::sketch_project::project_sketch_design;
pub(crate) use crate::ids::{
    neutral_parameter_id_parts, neutral_sketch_curve_id, neutral_sketch_id,
    neutral_sketch_point_id, neutral_spatial_sketch_id,
};
pub(crate) use crate::records::{
    decal::DesignRecordHeader,
    dimensions::{
        DesignDimensionAnnotationFrame, DesignDimensionAnnotationOperand, DesignDimensionLocus,
        DesignDimensionLocusGroup, DesignDimensionLocusPair, DesignDimensionRecipeRecord,
        DesignRecipeReference,
    },
    entity_header::{DesignEntityHeader, DesignFeatureTimeline, DESIGN_MODULE_SKETCH},
    feature::{
        assembly::{
            DesignAssemblyAlignment, DesignAssemblyAxialOperandTarget, DesignAssemblyLimitKind,
            DesignAssemblyOperandFrame,
        },
        assembly_features::DesignComponentInsertConstruction,
        base_feature::DesignBaseFeatureConstruction,
        body_ops::DesignScaleOperation,
        coil::{DesignCoilExtent, DesignCoilSection, DesignCoilSectionPlacement},
        combine::{DesignCombineBodySelection, DesignCombineForm, DesignCombineOperation},
        direct_face::{DesignDirectFaceOperation, DesignDraftOperation},
        extrude::{
            DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
            DesignExtrudeTargetOrdinal,
        },
        fixed_parameters::{
            DesignFixedChamferParameters, DesignFixedExtrudeDistance, DesignFixedExtrudeParameters,
            DesignFixedExtrudeScalar, DesignFixedFilletParameters,
        },
        hole::DesignHoleConstruction,
        path_features::DesignPathFeatureConstruction,
        patterns::DesignCircularPatternConstruction,
        primitives::DesignSolidPrimitive,
        scope::{DesignParameterScope, DesignScopePayload},
        surface_ops::{
            DesignRuledSurfaceCorner, DesignRuledSurfaceMethod, DesignSurfaceExtendMethod,
            DesignSurfaceExtendOperation, DesignSurfaceOffsetOperation, DesignSurfaceOffsetSupport,
            DesignSurfaceStitchOperation,
        },
        thread::{DesignThreadConstruction, DesignThreadForm},
    },
    parameters::{DesignParameterCompanion, DesignParameterOwner},
    recipes::{ConstructionRecipe, ConstructionRecipeKind},
    references::LostEdgeReference,
    sketch_geometry::{SketchCurveGeometry, SketchCurveIdentity, SketchPoint},
    sketch_links::PersistentSubentityTag,
    sketch_placement::DesignSketchPlacement,
    sketch_relations::{SketchConstraintKind, SketchRelation, SketchRelationOperand},
    topology::{
        body_recipe::DesignBodyRecipeOperand, body_recipe::DesignBodyRecipeReference,
        body_recipe::DesignOperandOwner, construction::DesignConstructionOperandGroup,
        construction::DesignConstructionOperandIdentity,
        construction::DesignConstructionPersistentIdentity,
        edge_identity::DesignEdgeIdentityOperand, edge_recipe::DesignTopologyRecipeSide,
        extrude_selection::DesignExtrudeFaceRole, extrude_selection::DesignExtrudeOperandRole,
        extrude_selection::DesignExtrudeSelectionGroup, face::DesignFaceOperand,
        face::DesignFaceRecipeNode, face::DesignFaceRecipeStructure,
        sketch_profile::DesignSketchProfileOperand,
    },
};
pub(crate) use crate::test_support::lp_utf16;
pub(crate) use cadmpeg_core::decode::WorkBudget;
pub(crate) use cadmpeg_ir::attributes::AttributeTarget;
pub(crate) use cadmpeg_ir::features::{
    FaceSelection, Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId,
    ParameterValue, ProfileRef,
};
pub(crate) use cadmpeg_ir::ids::FaceId;
pub(crate) use cadmpeg_ir::math::{Point2, Point3, Vector3};
pub(crate) use cadmpeg_ir::scalar::{Angle, Length};
pub(crate) use cadmpeg_ir::sketches::{
    Sketch, SketchAxis, SketchConstraintDefinitionInput, SketchEntity, SketchEntityId,
    SketchEntityUse, SketchGeometry, SketchId, SketchLocus, SketchNativeOperand, SpatialSketch,
    SpatialSketchConstraintDefinitionInput, SpatialSketchEntity, SpatialSketchEntityId,
    SpatialSketchEntityUse, SpatialSketchGeometry, SpatialSketchId, SpatialSketchProfile,
};
pub(crate) use std::collections::{BTreeMap, HashMap};

pub(crate) use crate::design::decode::scopes::solid_primitive::exact_solid_primitive;
