// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

pub(crate) use super::{parameter_owner_frame, parameter_record};
pub(crate) use crate::design::decode::operands::{
    assign_extrude_face_roles, decode_fillet_radius_groups,
};
pub(crate) use crate::design::decode::parameters::{
    parse_design_parameter_record as parse_design_parameter, parse_parameter_owner,
};
pub(crate) use crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment;
pub(crate) use crate::design::decode::scopes::axial_assembly::bind_joint_origin_frames_from_assemblies;
pub(crate) use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
pub(crate) use crate::design::decode::scopes::pattern::exact_circular_pattern_construction_with_owners;
pub(crate) use crate::design::decode::scopes::pattern::exact_rectangular_pattern_construction;
pub(crate) use crate::design::decode::scopes::pattern::select_circular_pattern_axis;
pub(crate) use crate::design::decode::sketch::IndexedRecordOffsets;
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
pub(crate) use crate::design::face_resolve::resolved_body_recipe_shape;
pub(crate) use crate::design::feature_project::{
    project_extrude, project_parameter_design, project_parameter_design_with_edge_identities,
    untyped_parameter_unit_count,
};
pub(crate) use crate::design::geometry::MAX_ARRANGEMENT_WALK_WORK;
pub(crate) use crate::design::profile_select::bind_extrude_profile_selections;
pub(crate) use crate::ids::{
    neutral_parameter_id_parts, neutral_sketch_id, neutral_spatial_sketch_id,
};
pub(crate) use crate::records::{
    decal::DesignRecordHeader,
    dimensions::{
        DesignDimensionAnnotationFrame, DesignDimensionAnnotationOperand, DesignDimensionLocus,
        DesignDimensionLocusGroup, DesignDimensionLocusPair, DesignDimensionRecipeRecord,
    },
    entity_header::DesignFeatureTimeline,
    feature::{
        assembly::DesignAssemblyAlignment,
        body_ops::DesignScaleOperation,
        coil::{DesignCoilExtent, DesignCoilSection, DesignCoilSectionPlacement},
        extrude::{
            DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
            DesignExtrudeTargetOrdinal,
        },
        fixed_parameters::{
            DesignFixedExtrudeDistance, DesignFixedExtrudeParameters, DesignFixedExtrudeScalar,
        },
        hole::DesignHoleConstruction,
        path_features::DesignPathFeatureConstruction,
        patterns::DesignCircularPatternConstruction,
        scope::{DesignParameterScope, DesignScopePayload},
        surface_ops::{
            DesignSurfaceExtendMethod, DesignSurfaceExtendOperation, DesignSurfaceStitchOperation,
        },
        thread::{DesignThreadConstruction, DesignThreadForm},
    },
    parameters::DesignParameterCompanion,
    recipes::{ConstructionRecipe, ConstructionRecipeKind},
    sketch_geometry::{SketchCurveGeometry, SketchCurveIdentity, SketchPoint},
    sketch_placement::DesignSketchPlacement,
    sketch_relations::{SketchConstraintKind, SketchRelation, SketchRelationOperand},
    topology::{
        body_recipe::DesignBodyRecipeOperand, body_recipe::DesignBodyRecipeReference,
        body_recipe::DesignOperandOwner, construction::DesignConstructionOperandGroup,
        extrude_selection::DesignExtrudeFaceRole, extrude_selection::DesignExtrudeSelectionGroup,
        face::DesignFaceOperand, sketch_profile::DesignSketchProfileOperand,
    },
};
pub(crate) use crate::test_support::lp_utf16;
pub(crate) use cadmpeg_core::decode::WorkBudget;
pub(crate) use cadmpeg_ir::features::{
    FaceSelection, Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId,
    ParameterValue, ProfileRef,
};
pub(crate) use cadmpeg_ir::ids::FaceId;
pub(crate) use cadmpeg_ir::math::{Point2, Point3, Vector3};
pub(crate) use cadmpeg_ir::scalar::{Angle, Length};
pub(crate) use cadmpeg_ir::sketches::{
    SketchAxis, SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchId, SketchLocus, SketchNativeOperand, SpatialSketch,
    SpatialSketchConstraintDefinitionInput, SpatialSketchEntity, SpatialSketchEntityId,
    SpatialSketchEntityUse, SpatialSketchGeometry, SpatialSketchId, SpatialSketchProfile,
};
pub(crate) use std::collections::{BTreeMap, HashMap};
