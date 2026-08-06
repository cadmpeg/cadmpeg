// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]
//! Relation, decode, and projection unit tests for the design modules.

use crate::design::constraints::project_sketch_constraints;
use crate::design::decode::body::body_bound_candidates;
use crate::design::decode::dimension_frames::{
    companion_owned_interval, contiguous_i32_program, find_dimension_locus_groups,
    find_dimension_locus_pair, find_dimension_null_locus_pair, indexed_record_containing,
    parse_dimension_annotation_frame, parse_dimension_locus_group, parse_dimension_locus_pair,
    parse_dimension_null_locus_pair, recipe_record_prefix,
};
use crate::design::decode::operands::{
    assign_extrude_face_roles, bind_edge_operand_candidates, bind_extrude_selection_geometry,
    bind_extrude_selection_identities, bind_face_operand_candidates, bind_lost_edge_groups,
    decode_fillet_radius_groups, face_recipe_program_kind, has_typed_edge_treatment_group,
    parse_body_recipe_operand, parse_construction_operand_dual_transform,
    parse_construction_operand_flag, parse_construction_operand_group,
    parse_construction_operand_identity, parse_construction_operand_path,
    parse_construction_operand_transform, parse_construction_tracking_path, parse_edge_operand,
    parse_entity_selection_operand, parse_extrude_selection_group, parse_extrude_selection_member,
    parse_face_operand, parse_sketch_profile, ConstructionOperandGroupParse, FaceRecipeProgramKind,
};
use crate::design::decode::parameters::{
    bind_parameter_companion_payloads, design_parameter_discriminator, parse_design_parameter,
    parse_parameter_companion, parse_parameter_owner,
};
use crate::design::decode::scopes::{
    bind_joint_origin_frames_from_assemblies, exact_assembly_alignment,
    exact_base_feature_construction, exact_circular_pattern_construction_with_owners,
    exact_combine_operation, exact_component_insert_construction, exact_direct_face_operation,
    exact_draft_operation_with_owners, exact_fixed_chamfer_parameters,
    exact_fixed_extrude_parameters, exact_fixed_fillet_parameters, exact_joint_origin_frame,
    exact_path_feature_construction, exact_rectangular_pattern_construction,
    exact_ruled_surface_operation, exact_scale_operation, exact_solid_primitive,
    exact_surface_extend_operation, exact_surface_offset_operation, exact_surface_stitch_operation,
    exact_work_axis_construction, exact_work_plane_frame, exact_work_point_position,
    parse_parameter_scope, parse_thread_payload,
};
use crate::design::decode::sketch::{
    bind_sketch_graph, decode_constraint_kinds, decode_pattern_definition, identity_matrix,
    next_indexed_record_offset, next_indexed_record_offset_with_index,
    parse_classed_sketch_relation, parse_genesis_entity_header, parse_settled_entity_header,
    parse_sketch_placement_candidates, parse_sketch_surface, IndexedRecordOffsets,
    SketchRelationClass,
};
use crate::design::dimensions::{
    bind_dimension_loci, counted_role_relation, directional_point_dimension,
    exact_atomic_constraint, exact_counted_dimension_relation, exact_counted_offset,
    exact_offset_constraint, expression_identifiers, indirect_angular_lines,
    null_locus_dimension_definition, offset_parameter_factor,
    owner_scoped_angular_dimension_definition, owner_scoped_line_length_dimension_definition,
    owner_scoped_radial_dimension_definition, point_lies_on_sketch_geometry,
    radial_dimension_definition, radial_locus_dimension_definition,
    remove_dimension_frame_relations, repeated_linear_dimension,
    spatial_parallel_line_distance_matches, spatial_point_distance_matches,
    two_locus_distance_dimension, unique_point_class_dimension_definition,
    unresolved_parameter_expression_dependency_count,
};

fn project_dimension_constraints(
    inputs: &crate::design::dimensions::DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
) -> Vec<cadmpeg_ir::sketches::SketchConstraint> {
    crate::design::dimensions::project_dimension_constraints(inputs, spatial_sketches, 1.0e-6)
}

fn project_spatial_dimension_constraints(
    inputs: &crate::design::dimensions::DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    spatial_entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Vec<cadmpeg_ir::sketches::SpatialSketchConstraint> {
    crate::design::dimensions::project_spatial_dimension_constraints(
        inputs,
        spatial_sketches,
        spatial_entities,
        1.0e-6,
    )
}
use crate::design::edge_resolve::{
    feature_input_topology_id, partial_historical_edge_selection,
    resolved_edge_candidate_intersection, resolved_edge_candidate_intersection_with_deleted_proofs,
};
use crate::design::face_resolve::resolved_face_group;
use crate::design::feature_project::{
    project_combine, project_extrude, project_parameter_design, project_split,
    untyped_parameter_unit_count,
};
use crate::design::geometry::{
    closed_sketch_profiles, point_on_sketch_entity, region_containing_points,
    sketch_entity_endpoints,
};

use crate::design::profile_select::{
    bind_extrude_profile_selections, historical_profile_face_candidates,
    resolved_extrude_profile_selection,
};
use crate::design::sketch_project::{
    project_sketch_design, project_spatial_sketch_constraints, project_spatial_sketch_design,
};
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::{
    neutral_dimension_constraint_id, neutral_sketch_curve_id, neutral_sketch_id,
    neutral_sketch_point_id, neutral_spatial_sketch_id,
};
use crate::ids::{neutral_feature_id_parts, neutral_parameter_id_parts};

use crate::records::{
    ConstructionRecipe, ConstructionRecipeKind, DesignBodyRecipeOperand,
    DesignBodyRecipeOperandOwner, DesignBodyRecipeReference, DesignCircularPatternConstruction,
    DesignCoilExtent, DesignCoilSection, DesignCoilSectionPlacement, DesignCombineOperation,
    DesignConstructionOperandGroup, DesignConstructionOperandIdentity,
    DesignConstructionPersistentIdentity, DesignDimensionAnnotationFrame,
    DesignDimensionAnnotationOperand, DesignDimensionLocus, DesignDimensionLocusGroup,
    DesignDimensionLocusPair, DesignDimensionRecipeRecord, DesignDirectFaceOperation,
    DesignDraftOperation, DesignEdgeIdentityOperand, DesignEntityHeader, DesignExtrudeExtent,
    DesignExtrudeFaceRole, DesignExtrudeOperandRole, DesignExtrudeOperation, DesignExtrudePrologue,
    DesignExtrudeSelectionGroup, DesignExtrudeStart, DesignFaceOperand, DesignFaceRecipeNode,
    DesignFaceRecipeStructure, DesignFixedChamferParameters, DesignFixedExtrudeDistance,
    DesignFixedExtrudeParameters, DesignFixedExtrudeScalar, DesignFixedFilletParameters,
    DesignParameter, DesignParameterCompanion, DesignParameterKind, DesignParameterOwner,
    DesignParameterScope, DesignPathFeatureConstruction, DesignRecipeReference, DesignRecordHeader,
    DesignRuledSurfaceCorner, DesignRuledSurfaceMethod, DesignScaleOperation,
    DesignSketchPlacement, DesignSketchProfileOperand, DesignSolidPrimitive,
    DesignSurfaceExtendMethod, DesignSurfaceExtendOperation, DesignSurfaceOffsetOperation,
    DesignSurfaceOffsetSupport, DesignSurfaceStitchOperation, DesignThreadConstruction,
    DesignTopologyRecipeSide, LostEdgeReference, PersistentSubentityTag, SketchConstraintKind,
    SketchCurveGeometry, SketchCurveIdentity, SketchPoint, SketchRelation, SketchRelationOperand,
    SketchSurface, DESIGN_MODULE_SKETCH,
};

use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::features::{
    Angle, FaceSelection, Feature, FeatureDefinition, FeatureId, Length, ParameterId,
    ParameterValue, ProfileRef, SketchProfileRegion,
};

use cadmpeg_ir::ids::{EdgeId, FaceId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchAxis, SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchEntityUse,
    SketchGeometry, SketchId, SketchLocus, SpatialSketch, SpatialSketchConstraintDefinition,
};
use std::collections::{BTreeMap, HashMap, HashSet};

fn set_extrude_operation(scope: &mut DesignParameterScope, operation: DesignExtrudeOperation) {
    let Some(
        DesignExtrudePrologue::LegacyDistance {
            operation: value, ..
        }
        | DesignExtrudePrologue::ReferenceAware {
            operation: value, ..
        }
        | DesignExtrudePrologue::LegacyShifted {
            operation: value, ..
        },
    ) = scope.extrude_prologue.as_mut()
    else {
        panic!("test scope must carry an Extrude prologue");
    };
    *value = operation;
}

fn set_extrude_extent(scope: &mut DesignParameterScope, extent: DesignExtrudeExtent) {
    let Some(prologue) = scope.extrude_prologue.as_mut() else {
        panic!("test scope must carry an Extrude prologue");
    };
    match prologue {
        DesignExtrudePrologue::LegacyDistance { .. } => {
            assert_eq!(extent, DesignExtrudeExtent::OneSidedDistance);
        }
        DesignExtrudePrologue::ReferenceAware {
            extent: value,
            extent_discriminators,
            ..
        } => {
            *value = extent;
            *extent_discriminators = match extent {
                DesignExtrudeExtent::OneSidedToFace => [1, 1],
                DesignExtrudeExtent::OneSidedDistance => [1, 2],
                DesignExtrudeExtent::TwoSidedDistance => [2, 0],
                DesignExtrudeExtent::SymmetricDistance => [3, 2],
                DesignExtrudeExtent::SymmetricThroughAll => {
                    panic!("reference-aware test prologue does not decode this extent")
                }
                DesignExtrudeExtent::OneSidedThroughNext
                | DesignExtrudeExtent::OneSidedThroughAll => {
                    panic!("reference-aware test prologue does not decode this extent")
                }
            };
        }
        DesignExtrudePrologue::LegacyShifted {
            extent: value,
            direction_face_extend_values,
            side_extent_discriminators,
            ..
        } => {
            *value = Some(extent);
            (*direction_face_extend_values, *side_extent_discriminators) = match extent {
                DesignExtrudeExtent::OneSidedDistance => ([1, 0], [1, 0]),
                DesignExtrudeExtent::OneSidedToFace => ([1, 0], [2, 0]),
                DesignExtrudeExtent::OneSidedThroughNext => ([1, 0], [3, 0]),
                DesignExtrudeExtent::OneSidedThroughAll => ([1, 0], [4, 0]),
                DesignExtrudeExtent::TwoSidedDistance => ([2, 0], [1, 1]),
                DesignExtrudeExtent::SymmetricDistance => ([3, 0], [1, 0]),
                DesignExtrudeExtent::SymmetricThroughAll => ([3, 0], [4, 4]),
            };
        }
    }
}

fn set_extrude_direction_reversed(scope: &mut DesignParameterScope, reversed: bool) {
    let Some(
        DesignExtrudePrologue::LegacyDistance {
            direction_reversed, ..
        }
        | DesignExtrudePrologue::ReferenceAware {
            direction_reversed, ..
        }
        | DesignExtrudePrologue::LegacyShifted {
            direction_reversed, ..
        },
    ) = scope.extrude_prologue.as_mut()
    else {
        panic!("test scope must carry an Extrude prologue");
    };
    *direction_reversed = reversed;
}

fn set_extrude_start(scope: &mut DesignParameterScope, start: DesignExtrudeStart) {
    let Some(
        DesignExtrudePrologue::ReferenceAware { start: value, .. }
        | DesignExtrudePrologue::LegacyShifted { start: value, .. },
    ) = scope.extrude_prologue.as_mut()
    else {
        panic!("test scope must carry an Extrude prologue");
    };
    *value = start;
}

#[test]
fn spatial_line_distance_requires_parallel_geometry_and_exact_value() {
    use cadmpeg_ir::sketches::SpatialSketchGeometry::Line;

    let first = Line {
        start: Point3::new(0.0, 0.0, 0.0),
        end: Point3::new(0.0, 10.0, 0.0),
    };
    let second = Line {
        start: Point3::new(3.0, 0.0, 4.0),
        end: Point3::new(3.0, -5.0, 4.0),
    };
    let crossing = Line {
        start: Point3::new(0.0, 0.0, 0.0),
        end: Point3::new(1.0, 0.0, 0.0),
    };

    assert!(spatial_parallel_line_distance_matches(&first, &second, 5.0));
    assert!(!spatial_parallel_line_distance_matches(
        &first, &second, 4.0
    ));
    assert!(!spatial_parallel_line_distance_matches(
        &first, &crossing, 0.0
    ));
}

#[test]
fn spatial_point_distance_requires_point_geometry_and_exact_value() {
    use cadmpeg_ir::sketches::SpatialSketchGeometry::{Line, Point};

    let first = Point {
        position: Point3::new(1.0, 2.0, 3.0),
    };
    let second = Point {
        position: Point3::new(4.0, 6.0, 3.0),
    };
    let line = Line {
        start: Point3::new(1.0, 2.0, 3.0),
        end: Point3::new(4.0, 6.0, 3.0),
    };

    assert!(spatial_point_distance_matches(&first, &second, 5.0));
    assert!(!spatial_point_distance_matches(&first, &second, 4.0));
    assert!(!spatial_point_distance_matches(&first, &line, 5.0));
}

#[test]
fn sketch_surface_parser_recovers_tensor_product_grid() {
    let mut payload = vec![0; 315];
    payload[20] = 1;
    payload[21..25].copy_from_slice(&2u32.to_le_bytes());
    payload[25..29].copy_from_slice(&13u32.to_le_bytes());
    payload[29..42].copy_from_slice(b"EntityGenesis");
    payload[42..46].copy_from_slice(&23u32.to_le_bytes());
    payload[46..69].copy_from_slice(b"IntrinsicMetaTypeuint64");
    payload[69..77].copy_from_slice(&17u64.to_le_bytes());
    payload[77..81].copy_from_slice(&11u32.to_le_bytes());
    payload[81..92].copy_from_slice(b"surface_tag");
    payload[92..96].copy_from_slice(&23u32.to_le_bytes());
    payload[96..119].copy_from_slice(b"IntrinsicMetaTypeuint64");
    payload[119..127].copy_from_slice(&29u64.to_le_bytes());
    payload[127..131].copy_from_slice(&4u32.to_le_bytes());
    let coordinates = [
        0.0f64, 0.0, 0.0, 0.0, 2.0, 0.0, 3.0, 0.0, 0.0, 3.0, 2.0, 1.0,
    ];
    for (index, coordinate) in coordinates.into_iter().enumerate() {
        let at = 131 + index * 8;
        payload[at..at + 8].copy_from_slice(&coordinate.to_le_bytes());
    }
    let degrees_at = 131 + coordinates.len() * 8;
    payload[degrees_at..degrees_at + 4].copy_from_slice(&1u32.to_le_bytes());
    payload[degrees_at + 4..degrees_at + 8].copy_from_slice(&1u32.to_le_bytes());
    payload[degrees_at + 8..degrees_at + 12].copy_from_slice(&4u32.to_le_bytes());
    let mut at = degrees_at + 12;
    for knot in [0.0f64, 0.0, 1.0, 1.0] {
        payload[at..at + 8].copy_from_slice(&knot.to_le_bytes());
        at += 8;
    }
    payload[at..at + 4].copy_from_slice(&4u32.to_le_bytes());
    at += 4;
    for knot in [0.0f64, 0.0, 1.0, 1.0] {
        payload[at..at + 8].copy_from_slice(&knot.to_le_bytes());
        at += 8;
    }
    payload[at..at + 4].copy_from_slice(&2u32.to_le_bytes());
    payload[at + 4..at + 8].copy_from_slice(&2u32.to_le_bytes());

    let surface = parse_sketch_surface(&payload).expect("canonical surface payload");
    assert_eq!(surface.entity_genesis, Some(17));
    assert_eq!(surface.persistent_id, 29);
    assert_eq!((surface.u_degree, surface.v_degree), (1, 1));
    assert_eq!(surface.u_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_points.len(), 2);
    assert_eq!(surface.control_points[0].len(), 2);
    assert_eq!(surface.control_points[1][1], Point3::new(30.0, 20.0, 10.0));
}

#[test]
fn feature_family_tokens_are_localized() {
    assert_eq!(
        design_feature_family("Esquisse"),
        Some(DesignFeatureFamily::Sketch)
    );
    assert_eq!(
        design_feature_family("Extrusion"),
        Some(DesignFeatureFamily::Extrude)
    );
    assert_eq!(
        design_feature_family("Extrusão"),
        Some(DesignFeatureFamily::Extrude)
    );
    for token in ["Skizze", "Esboço"] {
        assert_eq!(
            design_feature_family(token),
            Some(DesignFeatureFamily::Sketch)
        );
    }
    assert_eq!(
        design_feature_family("Congé"),
        Some(DesignFeatureFamily::Fillet)
    );
    for token in ["Abrundung", "Arredondamento"] {
        assert_eq!(
            design_feature_family(token),
            Some(DesignFeatureFamily::Fillet)
        );
        assert!(has_typed_edge_treatment_group(token));
    }
    assert_eq!(
        design_feature_family("Chanfrein"),
        Some(DesignFeatureFamily::Chamfer)
    );
    for token in ["C-Pattern", "Réseau C"] {
        assert_eq!(
            design_feature_family(token),
            Some(DesignFeatureFamily::CircularPattern)
        );
    }
    assert_eq!(
        design_feature_family("Symétrie miroir"),
        Some(DesignFeatureFamily::Mirror)
    );
    assert_eq!(
        design_feature_family("DécalerLesFaces"),
        Some(DesignFeatureFamily::OffsetFaces)
    );
    assert_eq!(
        design_feature_family("Schale"),
        Some(DesignFeatureFamily::Shell)
    );
    assert_eq!(
        design_feature_family("SpirePrimitive"),
        Some(DesignFeatureFamily::Coil)
    );
    assert_eq!(
        design_feature_family("Hem"),
        Some(DesignFeatureFamily::SheetMetalHem)
    );
    assert!(crate::design::decode::operands::has_edge_recipe_operands(
        "Hem"
    ));
    assert_eq!(
        design_feature_family("SurfacePatch"),
        Some(DesignFeatureFamily::SurfacePatch)
    );
    assert!(crate::design::decode::operands::has_edge_recipe_operands(
        "SurfacePatch"
    ));
    assert_eq!(
        design_feature_family("SurfaceRuled"),
        Some(DesignFeatureFamily::SurfaceRuled)
    );
    assert_eq!(
        design_feature_family("BoundaryFill"),
        Some(DesignFeatureFamily::BoundaryFill)
    );
    assert_eq!(
        design_feature_family("Hole"),
        Some(DesignFeatureFamily::Hole)
    );
    assert_eq!(
        design_feature_family("Split"),
        Some(DesignFeatureFamily::Split)
    );
    assert_eq!(
        design_feature_family("Loft"),
        Some(DesignFeatureFamily::Loft)
    );
    assert_eq!(
        design_feature_family("Sweep"),
        Some(DesignFeatureFamily::Sweep)
    );
    assert_eq!(
        design_feature_family("Pipe"),
        Some(DesignFeatureFamily::Pipe)
    );
}

#[test]
fn partial_historical_edge_selection_retains_proofs_and_unresolved_operands() {
    use cadmpeg_ir::features::EdgeSelection;
    use cadmpeg_ir::ids::FeatureInputTopologyId;

    let state = FeatureInputTopologyId("f3d:history-input:state#feature".into());
    let selection = partial_historical_edge_selection(
        [
            ("operand-a", Some(17)),
            ("operand-b", None),
            ("operand-c", Some(17)),
        ],
        41,
        "feature",
        state.clone(),
        "group",
    )
    .expect("mixed proof state");
    assert_eq!(
        selection,
        EdgeSelection::HistoricalPartial {
            state,
            edges: vec![cadmpeg_ir::ids::HistoricalEdgeId(
                "f3d:history-input:edge#7:feature:41:17".into()
            )],
            unresolved: vec!["operand-b".into()],
            native: "group".into(),
        }
    );
    assert!(partial_historical_edge_selection(
        [("operand-a", Some(17)), ("operand-b", Some(18))],
        41,
        "feature",
        FeatureInputTopologyId("state".into()),
        "group",
    )
    .is_none());
    assert_eq!(
        partial_historical_edge_selection(
            [("operand-a", None), ("operand-b", None)],
            41,
            "feature",
            FeatureInputTopologyId("state".into()),
            "group",
        ),
        None
    );
}

#[test]
fn loft_path_preserves_complete_historical_edge_selection() {
    use cadmpeg_ir::features::{EdgeSelection, PathRef};
    use cadmpeg_ir::ids::{FeatureInputTopologyId, HistoricalEdgeId};

    let state = FeatureInputTopologyId("f3d:history-input:state#feature".into());
    let edge = HistoricalEdgeId("f3d:history-input:edge#7:feature:41:17".into());
    assert_eq!(
        crate::design::feature_project::loft_path_from_edge_selection(
            "group",
            EdgeSelection::Historical {
                state: state.clone(),
                edges: vec![edge.clone()],
                native: "selection".into(),
            },
        ),
        PathRef::HistoricalEdges {
            state: state.clone(),
            edges: vec![edge.clone()],
            native: "selection".into(),
        }
    );
    assert_eq!(
        crate::design::feature_project::loft_path_from_edge_selection(
            "group",
            EdgeSelection::HistoricalPartial {
                state,
                edges: vec![edge],
                unresolved: vec!["operand".into()],
                native: "selection".into(),
            },
        ),
        PathRef::Native("group".into())
    );
}

#[test]
fn feature_identity_uses_stream_family_ordinal_and_scope_record() {
    let first = neutral_feature_id_parts("Design/A:B", "Kind:12", 3, 41);
    let same = neutral_feature_id_parts("Design/A:B", "Kind:12", 3, 41);
    let different_stream = neutral_feature_id_parts("Design/A", "B:Kind:12", 3, 41);
    let different_family = neutral_feature_id_parts("Design/A:B", "Kind", 123, 41);
    let different_scope = neutral_feature_id_parts("Design/A:B", "Kind:12", 3, 42);

    assert_eq!(first, same);
    assert_ne!(first, different_stream);
    assert_ne!(first, different_family);
    assert_ne!(first, different_scope);

    let localized = neutral_feature_id_parts("Design Name", "Symétrie miroir", 1, 41);
    let literal_escape = neutral_feature_id_parts("Design%20Name", "Symétrie%20miroir", 1, 41);
    assert!(!localized.0.chars().any(char::is_whitespace));
    assert!(localized.0.contains("Design%20Name"));
    assert!(localized.0.contains("Symétrie%20miroir"));
    assert_ne!(localized, literal_escape);
    assert!(!feature_input_topology_id(&localized, 2)
        .0
        .chars()
        .any(char::is_whitespace));
}

#[test]
fn parameter_identity_uses_stream_and_native_record_index() {
    let first = neutral_parameter_id_parts("Design/A:12", 3);
    let same = neutral_parameter_id_parts("Design/A:12", 3);
    let different_stream = neutral_parameter_id_parts("Design/A", 123);
    let different_record = neutral_parameter_id_parts("Design/A:12", 4);

    assert_eq!(first, same);
    assert_ne!(first, different_stream);
    assert_ne!(first, different_record);
}

#[test]
fn parameter_identity_distinguishes_repeated_source_ordinals() {
    let mut first = parse_design_parameter(&parameter_record(
        Some(40),
        "1 cm",
        "AlongDistance",
        Some("cm"),
        "d9",
        1.0,
    ))
    .expect("first parameter");
    first.id = "f3d:Design/A:design-parameter#100".into();

    let mut second = first.clone();
    second.id = "f3d:Design/A:design-parameter#200".into();
    second.record_index = first.record_index + 1;

    assert_eq!(first.source_ordinal, second.source_ordinal);
    assert_ne!(
        crate::ids::neutral_parameter_id(&first),
        crate::ids::neutral_parameter_id(&second)
    );
}

#[test]
fn sketch_geometry_identity_uses_owner_and_native_persistent_ids() {
    use cadmpeg_ir::sketches::{SketchId, SpatialSketchId};

    let sketch = SketchId("f3d:model:sketch#Design/A@10".into());
    let other_sketch = SketchId("f3d:model:sketch#Design/A@11".into());
    let point = neutral_sketch_point_id(&sketch, 42);
    let same_point = neutral_sketch_point_id(&sketch, 42);
    let curve = neutral_sketch_curve_id(&sketch, 42, 0);
    let same_curve = neutral_sketch_curve_id(&sketch, 42, 0);

    assert_eq!(point, same_point);
    assert_eq!(curve, same_curve);
    assert_ne!(point, curve);
    assert_ne!(curve, neutral_sketch_curve_id(&sketch, 42, 1));
    assert_ne!(point, neutral_sketch_point_id(&other_sketch, 42));
    assert_ne!(curve, neutral_sketch_curve_id(&other_sketch, 42, 0));

    let spatial = SpatialSketchId("f3d:model:spatial-sketch#Design/A@10".into());
    let other_spatial = SpatialSketchId("f3d:model:spatial-sketch#Design/A@11".into());
    assert_ne!(
        crate::ids::neutral_spatial_sketch_point_id(&spatial, 42),
        crate::ids::neutral_spatial_sketch_point_id(&other_spatial, 42)
    );
    assert_ne!(
        crate::ids::neutral_spatial_sketch_curve_id(&spatial, 42, 0),
        crate::ids::neutral_spatial_sketch_curve_id(&other_spatial, 42, 0)
    );
}

#[test]
fn governing_dimension_identity_uses_parameter_identity() {
    let parameter = cadmpeg_ir::features::ParameterId("f3d:model:parameter#Design/A:12".into());
    let relocated = neutral_dimension_constraint_id(&parameter, "pair");
    let same = neutral_dimension_constraint_id(&parameter, "pair");
    let other_form = neutral_dimension_constraint_id(&parameter, "null-pair");
    let other_parameter = neutral_dimension_constraint_id(
        &cadmpeg_ir::features::ParameterId("parameter:Design/A".into()),
        "12:pair",
    );

    assert_eq!(relocated, same);
    assert_ne!(relocated, other_form);
    assert_ne!(relocated, other_parameter);
    assert_eq!(relocated.0.matches('#').count(), 1);
}

#[test]
fn historical_points_on_profile_boundaries_are_ambiguous() {
    let sketch_id = SketchId("sketch".into());
    let entity_id = SketchEntityId("line".into());
    let mut sketch = Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(10.0, 20.0, 5.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![vec![SketchEntityUse {
            entity: entity_id.clone(),
            reversed: false,
        }]],
        native_ref: None,
    };
    let entity = SketchEntity {
        id: entity_id,
        sketch: sketch_id,
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        },
    };
    let point = Point3::new(11.0, 20.0, 9.0);
    assert_eq!(
        region_containing_points(&sketch, std::slice::from_ref(&entity), &[point], 1.0e-6),
        None
    );
    assert_eq!(
        crate::design::profile_select::selection_containing_points(
            &sketch,
            std::slice::from_ref(&entity),
            &[point],
            1.0e-6,
        ),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0]))
    );

    let mut branched_sketch = sketch.clone();
    let start_branch_id = SketchEntityId("start-branch".into());
    let end_branch_id = SketchEntityId("end-branch".into());
    branched_sketch.profiles.extend([
        vec![SketchEntityUse {
            entity: start_branch_id.clone(),
            reversed: false,
        }],
        vec![SketchEntityUse {
            entity: end_branch_id.clone(),
            reversed: false,
        }],
    ]);
    let branch_entity = |id, start, end| SketchEntity {
        id,
        sketch: branched_sketch.id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let branched_entities = [
        entity.clone(),
        branch_entity(
            start_branch_id,
            Point2::new(0.0, 0.0),
            Point2::new(0.0, 1.0),
        ),
        branch_entity(end_branch_id, Point2::new(2.0, 0.0), Point2::new(2.0, 1.0)),
    ];
    let endpoints = [Point3::new(10.0, 20.0, 5.0), Point3::new(12.0, 20.0, 5.0)];
    assert_eq!(
        crate::design::profile_select::selection_containing_points(
            &branched_sketch,
            &branched_entities,
            &endpoints,
            1.0e-6,
        ),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0]))
    );

    sketch.profiles.push(sketch.profiles[0].clone());
    assert_eq!(
        region_containing_points(&sketch, std::slice::from_ref(&entity), &[point], 1.0e-6),
        None
    );
    assert_eq!(
        crate::design::profile_select::selection_containing_points(
            &sketch,
            std::slice::from_ref(&entity),
            &[point],
            1.0e-6,
        ),
        None
    );
}

#[test]
fn historical_selection_preserves_first_member_region_order() {
    let region = |outer| SketchProfileRegion::Loops {
        outer,
        holes: Vec::new(),
    };
    assert_eq!(
        crate::design::profile_select::ordered_unique_profile_selections([
            Some(crate::design::profile_select::ResolvedProfileSelection::Regions(vec![region(3)])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Regions(vec![region(1)])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Regions(vec![region(3)])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Regions(vec![region(2)])),
        ]),
        Some(
            crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
                region(3),
                region(1),
                region(2),
            ])
        )
    );
    assert_eq!(
        crate::design::profile_select::ordered_unique_profile_selections([
            Some(crate::design::profile_select::ResolvedProfileSelection::Regions(vec![region(3)])),
            None,
        ]),
        None
    );
}

#[test]
fn multiple_extrude_profile_groups_merge_only_exact_same_kind_selections() {
    let sketch = SketchId("f3d:model:sketch#multi-profile".into());
    let loops = [
        ProfileRef::SketchProfiles {
            sketch: sketch.clone(),
            profiles: vec![3, 1],
        },
        ProfileRef::SketchProfiles {
            sketch: sketch.clone(),
            profiles: vec![1, 2],
        },
    ];
    assert_eq!(
        crate::design::profile_select::merge_resolved_profile_selections(&sketch, &loops),
        Some(ProfileRef::SketchProfiles {
            sketch: sketch.clone(),
            profiles: vec![3, 1, 2],
        })
    );

    let regions = [
        ProfileRef::SketchRegions {
            sketch: sketch.clone(),
            regions: vec![SketchProfileRegion::Loops {
                outer: 4,
                holes: vec![5],
            }],
        },
        ProfileRef::SketchRegions {
            sketch: sketch.clone(),
            regions: vec![SketchProfileRegion::Loops {
                outer: 2,
                holes: Vec::new(),
            }],
        },
    ];
    assert_eq!(
        crate::design::profile_select::merge_resolved_profile_selections(&sketch, &regions),
        Some(ProfileRef::SketchRegions {
            sketch: sketch.clone(),
            regions: vec![
                SketchProfileRegion::Loops {
                    outer: 4,
                    holes: vec![5],
                },
                SketchProfileRegion::Loops {
                    outer: 2,
                    holes: Vec::new(),
                },
            ],
        })
    );

    assert_eq!(
        crate::design::profile_select::merge_resolved_profile_selections(
            &sketch,
            &[loops[0].clone(), regions[0].clone()]
        ),
        None
    );
    assert_eq!(
        crate::design::profile_select::merge_resolved_profile_selections(
            &sketch,
            &[
                loops[0].clone(),
                ProfileRef::SketchSelection {
                    sketch: sketch.clone(),
                    selections: vec!["native-group".into()],
                },
            ]
        ),
        None
    );
}

#[test]
fn historical_edge_positions_require_a_complete_state_chain() {
    let mut topology = crate::history_records::AsmHistoricalTopology {
        edges: vec![7],
        vertices: vec![8, 9],
        points: vec![18, 19],
        edge_vertices: vec![crate::history_records::AsmHistoricalEdge {
            edge: 7,
            start_vertex: 8,
            end_vertex: 9,
        }],
        vertex_points: vec![
            crate::history_records::AsmHistoricalCarrierBinding {
                entity: 8,
                carrier: 18,
            },
            crate::history_records::AsmHistoricalCarrierBinding {
                entity: 9,
                carrier: 19,
            },
        ],
        point_positions: vec![
            crate::history_records::AsmHistoricalPoint {
                point: 18,
                position: Point3::new(1.0, 2.0, 3.0),
            },
            crate::history_records::AsmHistoricalPoint {
                point: 19,
                position: Point3::new(4.0, 5.0, 6.0),
            },
        ],
        ..crate::history_records::AsmHistoricalTopology::default()
    };
    assert_eq!(
        crate::design::geometry::historical_entity_positions(
            crate::records::AsmHistoricalEntityKind::Edge,
            7,
            &topology,
        ),
        Some(vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0),])
    );
    topology.point_positions.pop();
    assert_eq!(
        crate::design::geometry::historical_entity_positions(
            crate::records::AsmHistoricalEntityKind::Edge,
            7,
            &topology,
        ),
        None
    );
}

#[test]
fn historical_region_faces_follow_complete_ownership_hierarchy() {
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    use crate::records::AsmHistoricalEntityKind;

    let topology = AsmHistoricalTopology {
        body_regions: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: vec![2],
        }],
        region_shells: vec![AsmHistoricalRelation {
            owner_ref: 2,
            member_refs: vec![3, 4],
        }],
        shell_faces: vec![
            AsmHistoricalRelation {
                owner_ref: 3,
                member_refs: vec![7, 5],
            },
            AsmHistoricalRelation {
                owner_ref: 4,
                member_refs: vec![6, 7],
            },
        ],
        ..AsmHistoricalTopology::default()
    };

    assert_eq!(
        crate::design::geometry::historical_owned_faces(
            AsmHistoricalEntityKind::Body,
            1,
            &topology
        ),
        Some(vec![5, 6, 7])
    );
    assert_eq!(
        crate::design::geometry::historical_owned_faces(
            AsmHistoricalEntityKind::Region,
            2,
            &topology
        ),
        Some(vec![5, 6, 7])
    );
    assert_eq!(
        crate::design::geometry::historical_owned_faces(
            AsmHistoricalEntityKind::Shell,
            3,
            &topology
        ),
        Some(vec![5, 7])
    );
}

#[test]
fn historical_profile_members_resolve_through_topology_ownership() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalOptionalCarrierBinding,
        AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use crate::records::AsmHistoricalEntityKind;

    let topology = AsmHistoricalTopology {
        faces: vec![10, 20],
        loops: vec![11, 21],
        coedges: vec![12, 22],
        edges: vec![30],
        surfaces: vec![40],
        pcurves: vec![50],
        face_loops: vec![
            AsmHistoricalRelation {
                owner_ref: 10,
                member_refs: vec![11],
            },
            AsmHistoricalRelation {
                owner_ref: 20,
                member_refs: vec![21],
            },
        ],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 12,
                owner_loop: 11,
                edge: 30,
                previous: 12,
                next: 12,
                radial_next: 22,
            },
            AsmHistoricalCoedge {
                coedge: 22,
                owner_loop: 21,
                edge: 30,
                previous: 22,
                next: 22,
                radial_next: 12,
            },
        ],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 40,
        }],
        coedge_pcurves: vec![AsmHistoricalOptionalCarrierBinding {
            entity: 12,
            carrier: Some(50),
        }],
        ..AsmHistoricalTopology::default()
    };

    assert_eq!(
        historical_profile_face_candidates(Some(AsmHistoricalEntityKind::Pcurve), 50, &topology,),
        HashSet::from([10])
    );
    assert_eq!(
        historical_profile_face_candidates(Some(AsmHistoricalEntityKind::Surface), 40, &topology,),
        HashSet::from([10])
    );
    assert_eq!(
        historical_profile_face_candidates(Some(AsmHistoricalEntityKind::Edge), 30, &topology,),
        HashSet::from([10, 20])
    );
}

#[test]
fn historical_face_points_require_complete_boundary_topology() {
    let mut topology = crate::history_records::AsmHistoricalTopology {
        faces: vec![10],
        loops: vec![11],
        coedges: vec![12, 13, 14],
        edges: vec![20, 21, 22],
        vertices: vec![30, 31, 32],
        points: vec![40, 41, 42],
        face_loops: vec![crate::history_records::AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![11],
        }],
        loop_coedges: vec![crate::history_records::AsmHistoricalRelation {
            owner_ref: 11,
            member_refs: vec![12, 13, 14],
        }],
        coedge_topology: vec![
            crate::history_records::AsmHistoricalCoedge {
                coedge: 12,
                owner_loop: 11,
                edge: 20,
                next: 13,
                previous: 14,
                radial_next: 12,
            },
            crate::history_records::AsmHistoricalCoedge {
                coedge: 13,
                owner_loop: 11,
                edge: 21,
                next: 14,
                previous: 12,
                radial_next: 13,
            },
            crate::history_records::AsmHistoricalCoedge {
                coedge: 14,
                owner_loop: 11,
                edge: 22,
                next: 12,
                previous: 13,
                radial_next: 14,
            },
        ],
        edge_vertices: vec![
            crate::history_records::AsmHistoricalEdge {
                edge: 20,
                start_vertex: 30,
                end_vertex: 31,
            },
            crate::history_records::AsmHistoricalEdge {
                edge: 21,
                start_vertex: 31,
                end_vertex: 32,
            },
            crate::history_records::AsmHistoricalEdge {
                edge: 22,
                start_vertex: 32,
                end_vertex: 30,
            },
        ],
        vertex_points: vec![
            crate::history_records::AsmHistoricalCarrierBinding {
                entity: 30,
                carrier: 40,
            },
            crate::history_records::AsmHistoricalCarrierBinding {
                entity: 31,
                carrier: 41,
            },
            crate::history_records::AsmHistoricalCarrierBinding {
                entity: 32,
                carrier: 42,
            },
        ],
        point_positions: vec![
            crate::history_records::AsmHistoricalPoint {
                point: 40,
                position: Point3::new(0.0, 0.0, 0.0),
            },
            crate::history_records::AsmHistoricalPoint {
                point: 41,
                position: Point3::new(2.0, 0.0, 0.0),
            },
            crate::history_records::AsmHistoricalPoint {
                point: 42,
                position: Point3::new(0.0, 1.0, 0.0),
            },
        ],
        ..crate::history_records::AsmHistoricalTopology::default()
    };
    assert_eq!(
        crate::design::profile_select::historical_face_points(10, &topology),
        Some(vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ])
    );

    topology.point_positions.pop();
    assert_eq!(
        crate::design::profile_select::historical_face_points(10, &topology),
        None
    );
}

#[test]
fn inserted_cylinder_selects_its_exact_circular_sketch_profile() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder, AsmHistoricalEdge,
        AsmHistoricalPoint, AsmHistoricalRelation, AsmHistoricalTopology,
    };

    let sketch_id = SketchId("sketch".into());
    let circle_id = SketchEntityId("circle".into());
    let circle = SketchEntity {
        id: circle_id.clone(),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
        },
    };
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![vec![SketchEntityUse {
            entity: circle_id,
            reversed: false,
        }]],
        native_ref: None,
    };
    let topology = AsmHistoricalTopology {
        faces: vec![10],
        loops: vec![11],
        coedges: vec![12, 13, 14],
        edges: vec![20, 21, 22],
        vertices: vec![30, 31, 32],
        points: vec![40, 41, 42],
        surfaces: vec![50],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![11],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 11,
            member_refs: vec![12, 13, 14],
        }],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 12,
                owner_loop: 11,
                edge: 20,
                next: 13,
                previous: 14,
                radial_next: 12,
            },
            AsmHistoricalCoedge {
                coedge: 13,
                owner_loop: 11,
                edge: 21,
                next: 14,
                previous: 12,
                radial_next: 13,
            },
            AsmHistoricalCoedge {
                coedge: 14,
                owner_loop: 11,
                edge: 22,
                next: 12,
                previous: 13,
                radial_next: 14,
            },
        ],
        edge_vertices: vec![
            AsmHistoricalEdge {
                edge: 20,
                start_vertex: 30,
                end_vertex: 31,
            },
            AsmHistoricalEdge {
                edge: 21,
                start_vertex: 31,
                end_vertex: 32,
            },
            AsmHistoricalEdge {
                edge: 22,
                start_vertex: 32,
                end_vertex: 30,
            },
        ],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 50,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 30,
                carrier: 40,
            },
            AsmHistoricalCarrierBinding {
                entity: 31,
                carrier: 41,
            },
            AsmHistoricalCarrierBinding {
                entity: 32,
                carrier: 42,
            },
        ],
        point_positions: vec![
            AsmHistoricalPoint {
                point: 40,
                position: Point3::new(2.0, 0.0, 0.0),
            },
            AsmHistoricalPoint {
                point: 41,
                position: Point3::new(0.0, 2.0, 1.0),
            },
            AsmHistoricalPoint {
                point: 42,
                position: Point3::new(-2.0, 0.0, 0.0),
            },
        ],
        surface_cylinders: vec![AsmHistoricalCylinder {
            surface: 50,
            origin: Point3::new(0.0, 0.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            radius: 2.0,
        }],
        ..AsmHistoricalTopology::default()
    };

    assert_eq!(
        crate::design::profile_select::inserted_cylindrical_profile_selection(
            &sketch,
            std::slice::from_ref(&circle),
            &topology,
            10,
            1.0e-6,
            1.0e-9,
        ),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0]))
    );
    let mut tilted = topology;
    tilted.surface_cylinders[0].axis = Vector3::new(0.0, 1.0, 0.0);
    assert_eq!(
        crate::design::profile_select::inserted_cylindrical_profile_selection(
            &sketch,
            std::slice::from_ref(&circle),
            &tilted,
            10,
            1.0e-6,
            1.0e-9,
        ),
        None
    );
}

#[test]
fn deleted_profile_family_requires_one_complete_multi_face_carrier() {
    use crate::history_records::{AsmHistoricalCarrierBinding, AsmHistoricalTopology};

    let topology = AsmHistoricalTopology {
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 200,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    assert_eq!(
        crate::design::profile_select::unique_multi_face_deleted_carrier_family(
            &[20, 11, 10],
            &topology
        ),
        Some(vec![10, 11])
    );
    assert_eq!(
        crate::design::profile_select::unique_multi_face_deleted_carrier_family(
            &[10, 10],
            &topology
        ),
        None
    );

    let mut ambiguous = topology.clone();
    ambiguous.face_surfaces.extend([
        AsmHistoricalCarrierBinding {
            entity: 30,
            carrier: 300,
        },
        AsmHistoricalCarrierBinding {
            entity: 31,
            carrier: 300,
        },
    ]);
    assert_eq!(
        crate::design::profile_select::unique_multi_face_deleted_carrier_family(
            &[10, 11, 30, 31],
            &ambiguous
        ),
        None
    );

    let mut incomplete = topology;
    incomplete
        .face_surfaces
        .retain(|binding| binding.entity != 20);
    assert_eq!(
        crate::design::profile_select::unique_multi_face_deleted_carrier_family(
            &[10, 11, 20],
            &incomplete
        ),
        None
    );
}

#[test]
fn transition_profile_prefers_consistent_side_loops_and_combines_cap_boundaries() {
    use cadmpeg_ir::features::SketchProfileRegion;

    let sketch_id = SketchId("sketch".into());
    let mut profiles = Vec::new();
    let mut entities = Vec::new();
    for (profile_index, corners) in [
        [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]],
        [[6.0, 0.0], [8.0, 0.0], [8.0, 2.0], [6.0, 2.0]],
        [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]],
        [[3.0, 1.0], [5.0, 1.0], [5.0, 3.0], [3.0, 3.0]],
    ]
    .into_iter()
    .enumerate()
    {
        let mut profile = Vec::new();
        for edge_index in 0..corners.len() {
            let id = SketchEntityId(format!("profile-{profile_index}-edge-{edge_index}"));
            profile.push(SketchEntityUse {
                entity: id.clone(),
                reversed: false,
            });
            let [start_u, start_v] = corners[edge_index];
            let [end_u, end_v] = corners[(edge_index + 1) % corners.len()];
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: Point2::new(start_u, start_v),
                    end: Point2::new(end_u, end_v),
                },
            });
        }
        profiles.push(profile);
    }
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles,
        native_ref: None,
    };
    let transition_selection = |selections| {
        crate::design::profile_select::transition_inserted_profile_selection(
            &sketch, &entities, 1.0e-6, selections,
        )
    };

    assert_eq!(
        crate::design::profile_select::unique_resolved_selection([Some(3), Some(3), Some(3)]),
        Some(3)
    );
    assert_eq!(
        crate::design::profile_select::unique_resolved_selection([Some(3), None, Some(3)]),
        Some(3)
    );
    assert_eq!(
        crate::design::profile_select::unique_resolved_selection([Some(3), Some(4)]),
        None
    );
    assert_eq!(
        crate::design::profile_select::unique_resolved_selection(std::iter::empty::<Option<u32>>()),
        None
    );
    assert_eq!(
        crate::design::profile_select::unique_resolved_selection([None::<u32>, None]),
        None
    );
    let region = crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
        SketchProfileRegion::Loops {
            outer: 0,
            holes: vec![1],
        },
    ]);
    assert_eq!(
        transition_selection(vec![
            Some(region.clone()),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0, 1])),
        ]),
        Some(region.clone())
    );
    assert_eq!(
        transition_selection(vec![
            Some(region.clone()),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![2])),
        ]),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![2]))
    );
    assert_eq!(
        transition_selection(vec![
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Regions(Vec::new())),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
        ]),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1]))
    );
    assert_eq!(
        transition_selection(vec![Some(region)]),
        Some(
            crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
                SketchProfileRegion::Loops {
                    outer: 0,
                    holes: vec![1],
                },
            ])
        )
    );
    assert_eq!(
        transition_selection(vec![
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
            None,
        ]),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0, 1]))
    );
    assert_eq!(
        transition_selection(vec![
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![2])),
        ]),
        None
    );
    assert_eq!(
        transition_selection(vec![
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![3])),
        ]),
        None
    );
    assert_eq!(transition_selection(vec![None]), None);
}

#[test]
fn historical_point_membership_respects_conic_domains_and_nurbs_endpoints() {
    let sketch = SketchId("sketch".into());
    let entity = |geometry| SketchEntity {
        id: SketchEntityId("curve".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let arc = entity(SketchGeometry::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length(2.0),
        start_angle: cadmpeg_ir::features::Angle(0.0),
        end_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
    });
    assert!(point_on_sketch_entity(Point2::new(0.0, 2.0), &arc, 1.0e-6));
    assert!(!point_on_sketch_entity(
        Point2::new(-2.0, 0.0),
        &arc,
        1.0e-6
    ));
    let clockwise_arc = entity(SketchGeometry::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length(2.0),
        start_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
        end_angle: cadmpeg_ir::features::Angle(0.0),
    });
    assert!(point_lies_on_sketch_geometry(
        Point2::new(std::f64::consts::SQRT_2, std::f64::consts::SQRT_2),
        &clockwise_arc.geometry
    ));
    assert!(!point_lies_on_sketch_geometry(
        Point2::new(-2.0, 0.0),
        &clockwise_arc.geometry
    ));

    let ellipse = entity(SketchGeometry::Ellipse {
        center: Point2::new(1.0, -1.0),
        major_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
        major_radius: Length(4.0),
        minor_radius: Length(2.0),
        start_angle: Some(cadmpeg_ir::features::Angle(0.0)),
        end_angle: Some(cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2)),
    });
    assert!(point_on_sketch_entity(
        Point2::new(-1.0, -1.0),
        &ellipse,
        1.0e-6
    ));
    assert!(!point_on_sketch_entity(
        Point2::new(3.0, -1.0),
        &ellipse,
        1.0e-6
    ));
    assert!(!point_on_sketch_entity(
        Point2::new(-1.0, -0.9),
        &ellipse,
        1.0e-6
    ));

    let nurbs = entity(SketchGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point2::new(1.0, 2.0),
            Point2::new(2.0, 4.0),
            Point2::new(3.0, 2.0),
        ],
        weights: Some(vec![1.0, 0.5, 1.0]),
        periodic: false,
    });
    assert!(point_on_sketch_entity(
        Point2::new(3.0, 2.0),
        &nurbs,
        1.0e-6
    ));
    assert!(!point_on_sketch_entity(
        Point2::new(2.0, 4.0),
        &nurbs,
        1.0e-6
    ));
    let SketchGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        ..
    } = &nurbs.geometry
    else {
        unreachable!()
    };
    let interior = cadmpeg_ir::eval::nurbs_pcurve_uv(
        *degree,
        knots,
        control_points,
        weights.as_deref(),
        0.375,
    )
    .unwrap();
    assert!(point_on_sketch_entity(interior, &nurbs, 1.0e-9));
}

fn lp_utf16(out: &mut Vec<u8>, value: &str) {
    let units = value.encode_utf16().collect::<Vec<_>>();
    out.extend_from_slice(&(units.len() as u32).to_le_bytes());
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

fn parameter_record(
    owner: Option<u32>,
    expression: &str,
    source_kind: &str,
    unit: Option<&str>,
    name: &str,
    evaluated_value: f64,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"305");
    out.extend_from_slice(&71u32.to_le_bytes());
    out.extend_from_slice(&[0; 11]);
    out.extend_from_slice(&design_parameter_discriminator(source_kind).to_le_bytes());
    out.push(0);
    out.extend_from_slice(&9u32.to_le_bytes());
    match owner {
        Some(owner) => {
            out.push(1);
            out.extend_from_slice(&owner.to_le_bytes());
            out.extend_from_slice(&[0; 6]);
        }
        None => out.push(0),
    }
    lp_utf16(&mut out, expression);
    out.extend_from_slice(if owner.is_some() {
        &[0; 9]
    } else {
        &[0, 0, 0, 0, 0, 0, 0, 0, 1]
    });
    lp_utf16(&mut out, source_kind);
    out.extend_from_slice(&0u32.to_le_bytes());
    if let Some(unit) = unit {
        lp_utf16(&mut out, unit);
    }
    lp_utf16(&mut out, name);
    out.extend_from_slice(&evaluated_value.to_le_bytes());
    out.extend_from_slice(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out
}

fn compact_owned_parameter_record(
    owner_record_index: u32,
    source_ordinal: u32,
    expression: &str,
    source_kind: &str,
    unit: Option<&str>,
    name: &str,
    evaluated_value: f64,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"328");
    out.extend_from_slice(&(owner_record_index + 1).to_le_bytes());
    out.extend_from_slice(&[0; 15]);
    out.extend_from_slice(&source_ordinal.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&owner_record_index.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    lp_utf16(&mut out, expression);
    out.extend_from_slice(&[0; 5]);
    lp_utf16(&mut out, source_kind);
    if let Some(unit) = unit {
        lp_utf16(&mut out, unit);
    } else {
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    lp_utf16(&mut out, name);
    out.extend_from_slice(&evaluated_value.to_le_bytes());
    out.extend_from_slice(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out
}

#[test]
fn compact_owned_design_parameter_has_no_family_discriminator() {
    let bytes =
        compact_owned_parameter_record(6653, 99, "82.00 mm", "Diameter", Some("mm"), "d99", 8.2);
    let parameter = parse_design_parameter(&bytes).expect("compact owned parameter");
    assert_eq!(parameter.record_index, 6654);
    assert_eq!(parameter.owner_record_index, Some(6653));
    assert_eq!(parameter.source_ordinal, 99);
    assert_eq!(parameter.family_discriminator, None);
    assert_eq!(parameter.family_discriminator_offset, None);
    assert_eq!(parameter.expression, "82.00 mm");
    assert_eq!(parameter.source_kind, "Diameter");
    assert_eq!(parameter.unit.as_deref(), Some("mm"));
    assert_eq!(parameter.name, "d99");
    assert_eq!(parameter.evaluated_value, 8.2);
}

#[test]
fn legacy_owned_design_parameter_uses_the_compact_identity_prefix() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"296");
    bytes.extend_from_slice(&439_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(&5_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&437_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    lp_utf16(&mut bytes, "0.00 mm");
    bytes.extend_from_slice(&[0; 5]);
    lp_utf16(&mut bytes, "OffsetX");
    lp_utf16(&mut bytes, "mm");
    lp_utf16(&mut bytes, "d5");
    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 18, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

    let parameter = parse_design_parameter(&bytes).expect("legacy owned parameter");
    assert_eq!(parameter.record_index, 439);
    assert_eq!(parameter.owner_record_index, Some(437));
    assert_eq!(parameter.source_ordinal, 5);
    assert_eq!(parameter.source_kind, "OffsetX");
    assert_eq!(parameter.unit.as_deref(), Some("mm"));
    assert_eq!(parameter.name, "d5");
    assert_eq!(parameter.evaluated_value, 0.0);
}

#[test]
fn body_bound_candidate_has_one_marker_and_six_ordered_f64_values() {
    let values: [f64; 6] = [4.0, 6.0, 1.5, -1.0, 0.0, -0.25];
    let mut bytes = vec![1];
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let candidates = body_bound_candidates(&bytes, 0, bytes.len()).collect::<Vec<_>>();
    assert_eq!(candidates, [(0, values)]);

    bytes[0] = 0;
    assert!(body_bound_candidates(&bytes, 0, bytes.len())
        .next()
        .is_none());
}

#[test]
fn parameter_variants_have_exact_string_and_scalar_boundaries() {
    let user = parse_design_parameter(&parameter_record(
        None,
        "60 mm",
        "User Parameter",
        Some("mm"),
        "Width",
        6.0,
    ))
    .unwrap();
    assert_eq!(user.kind, DesignParameterKind::User);
    assert_eq!(user.owner_record_index, None);
    assert_eq!(user.unit.as_deref(), Some("mm"));
    assert_eq!(user.evaluated_value, 6.0);

    let feature = parse_design_parameter(&parameter_record(
        Some(44),
        "Width / 2",
        "AlongDistance",
        Some("mm"),
        "d12",
        3.0,
    ))
    .unwrap();
    assert_eq!(feature.kind, DesignParameterKind::Feature);
    assert_eq!(feature.owner_record_index, Some(44));
    assert_eq!(feature.expression, "Width / 2");

    let boolean = parse_design_parameter(&parameter_record(
        None,
        "1",
        "User Parameter",
        None,
        "OnOff",
        1.0,
    ))
    .unwrap();
    assert_eq!(boolean.unit, None);
    assert_eq!(boolean.name, "OnOff");

    let mut tangency = parameter_record(Some(24409), "1", "TangencyWeight", Some(""), "d81", 1.0);
    tangency[22..30].copy_from_slice(&6u64.to_le_bytes());
    let tangency = parse_design_parameter(&tangency).expect("prefixed unitless parameter");
    assert_eq!(tangency.family_discriminator, Some(6));
    assert_eq!(tangency.unit, None);
    assert_eq!(tangency.name, "d81");
    assert_eq!(tangency.evaluated_value, 1.0);

    let mut earlier_tangency =
        parameter_record(Some(24409), "1", "TangencyWeight", Some(""), "d81", 1.0);
    earlier_tangency[22..30].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(
        parse_design_parameter(&earlier_tangency)
            .expect("earlier tangency parameter")
            .family_discriminator,
        Some(0)
    );

    for discriminator in [3u64, 4] {
        let mut earlier_distance = parameter_record(
            Some(44),
            "Width / 2",
            "AlongDistance",
            Some("mm"),
            "d12",
            3.0,
        );
        earlier_distance[22..30].copy_from_slice(&discriminator.to_le_bytes());
        assert_eq!(
            parse_design_parameter(&earlier_distance)
                .expect("earlier feature parameter")
                .family_discriminator,
            Some(discriminator)
        );
    }

    let mut invalid_tangency = earlier_tangency;
    invalid_tangency[22..30].copy_from_slice(&5u64.to_le_bytes());
    assert!(parse_design_parameter(&invalid_tangency).is_none());

    let mut revised_distance = parameter_record(
        Some(44),
        "Width / 2",
        "AlongDistance",
        Some("mm"),
        "d12",
        3.0,
    );
    revised_distance[22..30].copy_from_slice(&6u64.to_le_bytes());
    let tail = revised_distance.len() - 12;
    revised_distance[tail + 2] = 16;
    assert_eq!(
        parse_design_parameter(&revised_distance)
            .expect("revision-six feature parameter")
            .family_discriminator,
        Some(6)
    );

    let mut invalid_distance = revised_distance.clone();
    invalid_distance[22..30].copy_from_slice(&7u64.to_le_bytes());
    assert!(parse_design_parameter(&invalid_distance).is_none());

    revised_distance[tail + 2] = 19;
    assert!(parse_design_parameter(&revised_distance).is_none());

    let mut sheet_metal =
        parameter_record(Some(301), "50.00 mm", "FlangeHeight", Some("mm"), "d2", 5.0);
    sheet_metal[22..30].copy_from_slice(&6u64.to_le_bytes());
    let (_, expression_end) =
        crate::bytes::lp_utf16_bounded(&sheet_metal, 46, 1..=256).expect("sheet-metal expression");
    sheet_metal.insert(expression_end + 9, 0);
    let tail = sheet_metal.len() - 12;
    sheet_metal[tail + 2] = 16;
    let sheet_metal = parse_design_parameter(&sheet_metal)
        .expect("sheet-metal parameter with ten-byte expression trailer");
    assert_eq!(sheet_metal.source_kind, "FlangeHeight");
    assert_eq!(sheet_metal.owner_record_index, Some(301));
    assert_eq!(sheet_metal.evaluated_value, 5.0);
}

#[test]
fn parameter_record_rejects_noncanonical_tail() {
    let mut record = parameter_record(
        Some(44),
        "45 deg",
        "TaperAngle",
        Some("deg"),
        "d13",
        std::f64::consts::FRAC_PI_4,
    );
    *record.last_mut().unwrap() = 1;
    assert!(parse_design_parameter(&record).is_none());
}

fn parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 104];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"292");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&2u32.to_le_bytes());
    frame[40..48].copy_from_slice(&6.0f64.to_le_bytes());
    frame[48] = 1;
    frame[49..53].copy_from_slice(&45u32.to_le_bytes());
    frame[59..63].copy_from_slice(&9u32.to_le_bytes());
    frame[67] = 1;
    frame[68..72].copy_from_slice(&12u32.to_le_bytes());
    frame[78] = 1;
    frame[79] = 1;
    frame[81] = 1;
    frame[82..86].copy_from_slice(&46u32.to_le_bytes());
    frame[93] = 1;
    frame[94..98].copy_from_slice(&12u32.to_le_bytes());
    frame
}

fn compact_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 103];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"406");
    frame[7..11].copy_from_slice(&6653u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&6644u32.to_le_bytes());
    frame[35..39].copy_from_slice(&0u32.to_le_bytes());
    frame[40..48].copy_from_slice(&8.2f64.to_le_bytes());
    frame[48] = 1;
    frame[49..53].copy_from_slice(&6654u32.to_le_bytes());
    frame[59..63].copy_from_slice(&4u32.to_le_bytes());
    frame[67] = 1;
    frame[68..72].copy_from_slice(&6644u32.to_le_bytes());
    frame[80] = 1;
    frame[81..85].copy_from_slice(&6655u32.to_le_bytes());
    frame[92] = 1;
    frame[93..97].copy_from_slice(&6644u32.to_le_bytes());
    frame
}

fn counted_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 101];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"316");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&2u32.to_le_bytes());
    frame[40] = 1;
    frame[41..45].copy_from_slice(&6u32.to_le_bytes());
    frame[45] = 1;
    frame[46..50].copy_from_slice(&45u32.to_le_bytes());
    frame[56..60].copy_from_slice(&9u32.to_le_bytes());
    frame[64] = 1;
    frame[65..69].copy_from_slice(&12u32.to_le_bytes());
    frame[75] = 1;
    frame[76] = 1;
    frame[78] = 1;
    frame[79..83].copy_from_slice(&46u32.to_le_bytes());
    frame[90] = 1;
    frame[91..95].copy_from_slice(&12u32.to_le_bytes());
    frame
}

fn compact_counted_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 99];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"457");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&2u32.to_le_bytes());
    frame[40..44].copy_from_slice(&6u32.to_le_bytes());
    frame[44] = 1;
    frame[45..49].copy_from_slice(&45u32.to_le_bytes());
    frame[55..59].copy_from_slice(&9u32.to_le_bytes());
    frame[63] = 1;
    frame[64..68].copy_from_slice(&12u32.to_le_bytes());
    frame[76] = 1;
    frame[77..81].copy_from_slice(&46u32.to_le_bytes());
    frame[88] = 1;
    frame[89..93].copy_from_slice(&12u32.to_le_bytes());
    frame
}

fn tagged_scalar_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 107];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"406");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&2u32.to_le_bytes());
    frame[39] = 1;
    frame[44..52].copy_from_slice(&6.0f64.to_le_bytes());
    frame[52] = 1;
    frame[53..57].copy_from_slice(&45u32.to_le_bytes());
    frame[63..67].copy_from_slice(&9u32.to_le_bytes());
    frame[71] = 1;
    frame[72..76].copy_from_slice(&12u32.to_le_bytes());
    frame[84] = 1;
    frame[85..89].copy_from_slice(&46u32.to_le_bytes());
    frame[96] = 1;
    frame[97..101].copy_from_slice(&12u32.to_le_bytes());
    frame
}

fn tagged_scalar_variant_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 108];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"299");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&1u32.to_le_bytes());
    frame[39] = 1;
    frame[44..52].copy_from_slice(&0.8f64.to_le_bytes());
    frame[52] = 1;
    frame[53..57].copy_from_slice(&45u32.to_le_bytes());
    frame[63..67].copy_from_slice(&73u32.to_le_bytes());
    frame[71] = 1;
    frame[72..76].copy_from_slice(&12u32.to_le_bytes());
    frame[82] = 1;
    frame[85] = 1;
    frame[86..90].copy_from_slice(&46u32.to_le_bytes());
    frame[97] = 1;
    frame[98..102].copy_from_slice(&12u32.to_le_bytes());
    frame
}

#[test]
fn parameter_owner_frame_has_repeated_scope_and_both_record_orders() {
    let parsed = parse_parameter_owner(&parameter_owner_frame()).unwrap();
    assert_eq!(parsed.record_index, 44);
    assert_eq!(parsed.scope_record_index, 12);
    assert_eq!(parsed.local_ordinal, 2);
    assert_eq!(parsed.evaluated_value, 6.0);
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.owned_ordinal, 9);
    assert_eq!(parsed.variant, Some(1));
    assert_eq!(parsed.companion_record_index, 46);

    let mut parameter_first = parameter_owner_frame();
    parameter_first[49..53].copy_from_slice(&43u32.to_le_bytes());
    parameter_first[82..86].copy_from_slice(&45u32.to_le_bytes());
    let parsed = parse_parameter_owner(&parameter_first).expect("parameter-first owner frame");
    assert_eq!(parsed.parameter_record_index, 43);
    assert_eq!(parsed.record_index, 44);
    assert_eq!(parsed.companion_record_index, 45);

    let mut malformed = parameter_owner_frame();
    malformed[94..98].copy_from_slice(&13u32.to_le_bytes());
    assert!(parse_parameter_owner(&malformed).is_none());
}

/// A parameter-owner frame length outside the six layouts carries no known
/// field positions, so it declines. The evaluated-value offset therefore never
/// reaches a length the layout match did not accept, and the three lengths that
/// take its default all hold the value at 40.
#[test]
fn a_parameter_owner_frame_of_an_unlisted_length_declines() {
    for build in [
        parameter_owner_frame as fn() -> Vec<u8>,
        compact_parameter_owner_frame,
        counted_parameter_owner_frame,
        compact_counted_parameter_owner_frame,
        tagged_scalar_parameter_owner_frame,
    ] {
        let frame = build();
        assert!(parse_parameter_owner(&frame).is_some());
        let mut longer = frame.clone();
        longer.push(0);
        assert!(parse_parameter_owner(&longer).is_none());
        assert!(parse_parameter_owner(&frame[..frame.len() - 1]).is_none());
    }

    assert_eq!(
        parse_parameter_owner(&parameter_owner_frame())
            .expect("owner frame")
            .evaluated_value_offset,
        40
    );
    assert_eq!(
        parse_parameter_owner(&compact_parameter_owner_frame())
            .expect("compact owner frame")
            .evaluated_value_offset,
        40
    );
}

#[test]
fn compact_parameter_owner_omits_the_variant_slot() {
    let parsed =
        parse_parameter_owner(&compact_parameter_owner_frame()).expect("compact parameter owner");
    assert_eq!(parsed.record_index, 6653);
    assert_eq!(parsed.scope_record_index, 6644);
    assert_eq!(parsed.parameter_record_index, 6654);
    assert_eq!(parsed.companion_record_index, 6655);
    assert_eq!(parsed.owned_ordinal, 4);
    assert_eq!(parsed.variant, None);
    assert_eq!(parsed.evaluated_value, 8.2);
}

#[test]
fn counted_parameter_owner_uses_typed_u32_scalar() {
    let parsed =
        parse_parameter_owner(&counted_parameter_owner_frame()).expect("counted parameter owner");
    assert_eq!(parsed.evaluated_value, 6.0);
    assert_eq!(parsed.evaluated_value_offset, 41);
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.companion_record_index, 46);
}

#[test]
fn compact_counted_parameter_owner_omits_type_and_variant_markers() {
    let mut frame = compact_counted_parameter_owner_frame();
    frame[45..49].copy_from_slice(&46u32.to_le_bytes());
    frame[77..81].copy_from_slice(&45u32.to_le_bytes());
    let parsed = parse_parameter_owner(&frame).expect("compact counted parameter owner");
    assert_eq!(parsed.evaluated_value, 6.0);
    assert_eq!(parsed.evaluated_value_offset, 40);
    assert_eq!(parsed.parameter_record_index, 46);
    assert_eq!(parsed.variant, None);
    assert_eq!(parsed.companion_record_index, 45);
}

#[test]
fn tagged_scalar_parameter_owner_carries_a_scalar_type_prefix() {
    let parsed = parse_parameter_owner(&tagged_scalar_parameter_owner_frame())
        .expect("tagged scalar parameter owner");
    assert_eq!(parsed.evaluated_value, 6.0);
    assert_eq!(parsed.evaluated_value_offset, 44);
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.variant, None);
    assert_eq!(parsed.companion_record_index, 46);
}

#[test]
fn tagged_scalar_parameter_owner_can_carry_a_variant_slot() {
    let parsed = parse_parameter_owner(&tagged_scalar_variant_parameter_owner_frame())
        .expect("tagged scalar variant parameter owner");
    assert_eq!(parsed.evaluated_value, 0.8);
    assert_eq!(parsed.evaluated_value_offset, 44);
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.owned_ordinal, 73);
    assert_eq!(parsed.variant, Some(0));
    assert_eq!(parsed.companion_record_index, 46);
}

#[test]
fn parameter_companion_prefix_has_owner_backlink_and_timestamp() {
    let mut prefix = vec![0; 58];
    prefix[0..4].copy_from_slice(&3u32.to_le_bytes());
    prefix[4..7].copy_from_slice(b"408");
    prefix[7..11].copy_from_slice(&46u32.to_le_bytes());
    prefix[31] = 1;
    prefix[32..36].copy_from_slice(&44u32.to_le_bytes());
    prefix[42..50].copy_from_slice(&1_678_000_000_000_000u64.to_le_bytes());

    let parsed = parse_parameter_companion(&prefix).unwrap();
    assert_eq!(parsed.record_index, 46);
    assert_eq!(parsed.owner_record_index, 44);
    assert_eq!(parsed.timestamp_micros, 1_678_000_000_000_000);
    assert_eq!(parsed.timestamp_micros_offset, 42);

    prefix[32..36].copy_from_slice(&45u32.to_le_bytes());
    assert_eq!(
        parse_parameter_companion(&prefix)
            .unwrap()
            .owner_record_index,
        45
    );
    prefix[42..50].fill(0);
    assert!(parse_parameter_companion(&prefix).is_none());
}

#[test]
fn dimension_recipe_uses_its_immediate_indexed_record_boundary() {
    let mut bytes = vec![0xaa; 5];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"415");
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 17]);
    let recipe_offset = bytes.len();
    bytes.extend_from_slice(b"edge_recipe_data");
    bytes.extend_from_slice(&[0; 13]);
    let next_offset = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"423");
    bytes.extend_from_slice(&41u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);

    assert_eq!(
        indexed_record_containing(&bytes, 5, bytes.len(), recipe_offset),
        Some((5, "415".into(), 40, next_offset))
    );
    assert_eq!(
        indexed_record_containing(&bytes, 5, bytes.len(), next_offset + 11),
        Some((next_offset, "423".into(), 41, bytes.len()))
    );
    assert_eq!(indexed_record_containing(&bytes, 6, bytes.len(), 7), None);
    assert_eq!(
        contiguous_i32_program(&[u8::MAX; 8], 0, 8),
        Some(vec![-1, -1])
    );
    assert_eq!(contiguous_i32_program(&[0; 7], 0, 7), None);

    let mut framed = vec![0; 11];
    framed.extend_from_slice(&[7, 8, 9]);
    framed.extend_from_slice(&16u32.to_le_bytes());
    let family_name_offset = framed.len();
    framed.extend_from_slice(b"edge_recipe_data");
    assert_eq!(
        recipe_record_prefix(&framed, 0, family_name_offset, 16),
        Some((11, vec![7, 8, 9]))
    );
    framed[14..18].copy_from_slice(&15u32.to_le_bytes());
    assert_eq!(
        recipe_record_prefix(&framed, 0, family_name_offset, 16),
        None
    );
}

#[test]
fn dimension_recipe_decodes_ordered_persistent_reference_entries() {
    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&3u32.to_le_bytes());
    prefix.extend_from_slice(&4u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&2u32.to_le_bytes());
    let first_token_at = prefix.len();
    prefix.extend_from_slice(b"13");
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    let first_reference_at = prefix.len();
    prefix.extend_from_slice(&331u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());

    prefix.extend_from_slice(&2u32.to_le_bytes());
    let second_token_at = prefix.len();
    prefix.extend_from_slice(&[b'9', 0, 0, 0]);
    prefix.push(0);
    prefix.extend_from_slice(&2u32.to_le_bytes());
    let second_reference_at = prefix.len();
    prefix.extend_from_slice(&303u32.to_le_bytes());
    let third_reference_at = prefix.len();
    prefix.extend_from_slice(&304u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let references =
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000);
    assert_eq!(references.len(), 3);
    assert_eq!(references[0].selector, 1);
    assert_eq!(references[0].selector_offset, 1_022);
    assert_eq!(references[0].token, "13");
    assert_eq!(references[0].token_offset, 1_000 + first_token_at as u64);
    assert_eq!(references[0].design_reference, 331);
    assert_eq!(
        references[0].design_reference_offset,
        1_000 + first_reference_at as u64
    );
    assert_eq!(references[1].selector, 2);
    assert_eq!(references[1].selector_offset, 1_048);
    assert_eq!(references[1].token, "9");
    assert_eq!(references[1].token_offset, 1_000 + second_token_at as u64);
    assert_eq!(references[1].design_reference, 303);
    assert_eq!(
        references[1].design_reference_offset,
        1_000 + second_reference_at as u64
    );
    assert_eq!(references[2].selector, 2);
    assert_eq!(references[2].token, "9");
    assert_eq!(references[2].design_reference, 304);
    assert_eq!(
        references[2].design_reference_offset,
        1_000 + third_reference_at as u64
    );
    let suffix_at = prefix.len() - 4;
    prefix.splice(
        suffix_at..,
        [1u32, 1, 0, 0, 2, 401, 402, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes),
    );
    assert_eq!(
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000),
        references
    );
    prefix.extend_from_slice(&[0; 2]);
    assert_eq!(
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000),
        references
    );
    let tags = [
        PersistentSubentityTag {
            id: "matching".into(),
            target: AttributeTarget::Face(FaceId("face-b".into())),
            selector: 1,
            token: "13".into(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "other".into(),
            target: AttributeTarget::Face(FaceId("face-a".into())),
            selector: 1,
            token: "13".into(),
            design_references: vec![999],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "alternate-face".into(),
            target: AttributeTarget::Face(FaceId("face-c".into())),
            selector: 2,
            token: "13".into(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "matching-edge".into(),
            target: AttributeTarget::Edge(EdgeId("edge-b".into())),
            selector: 1,
            token: "13".into(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "alternate-edge".into(),
            target: AttributeTarget::Edge(EdgeId("edge-c".into())),
            selector: 2,
            token: "13".into(),
            design_references: vec![331],
            ordinal: 0,
        },
    ];
    let mut bound = references[0].clone();
    crate::design::decode::dimension_frames::bind_recipe_reference_candidates(
        &mut bound, &tags, None,
    );
    assert_eq!(bound.candidate_faces, [FaceId("face-b".into())]);
    assert_eq!(bound.candidate_edges, [EdgeId("edge-b".into())]);
    assert_eq!(bound.alternate_selector_faces, [FaceId("face-c".into())]);
    assert_eq!(bound.alternate_selector_edges, [EdgeId("edge-c".into())]);
    let stream_tags = [
        PersistentSubentityTag {
            id: "f3d:xref/A/occurrence-0/design:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(FaceId("face-a".into())),
            selector: 1,
            token: "13".into(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "f3d:xref/B/occurrence-0/design:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(FaceId("face-b".into())),
            selector: 1,
            token: "13".into(),
            design_references: vec![331],
            ordinal: 0,
        },
    ];
    crate::design::decode::dimension_frames::bind_recipe_reference_candidates(
        &mut bound,
        &stream_tags,
        Some("f3d:xref/A/occurrence-0/Asset/Design1/BulkStream.dat:dimension-recipe#1"),
    );
    assert_eq!(bound.candidate_faces, [FaceId("face-a".into())]);
}

#[test]
fn dimension_locus_pair_resolves_two_typed_geometry_records() {
    let mut bytes = vec![0; 80];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"277");
    bytes[7..11].copy_from_slice(&233u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&3u32.to_le_bytes());
    bytes[24] = 1;
    bytes[35..39].copy_from_slice(&4u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&192u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&0u32.to_le_bytes());
    bytes[54] = 1;
    bytes[55..59].copy_from_slice(&194u32.to_le_bytes());
    bytes[65..69].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"273");
    bytes.extend_from_slice(&233u32.to_le_bytes());

    let mut pair = parse_dimension_locus_pair(&bytes, 0, 228, &HashSet::from([192, 194]))
        .expect("paired dimension locus frame");
    pair.id = "f3d:Design/BulkStream.dat:design-dimension-locus-pair#0".into();
    assert_eq!(pair.companion_record_index, 228);
    assert_eq!(pair.record_index, 233);
    assert_eq!(pair.frame_length, 80);
    assert_eq!(pair.first_geometry_record_index, 192);
    assert_eq!(pair.first_role, 0);
    assert_eq!(pair.second_geometry_record_index, 194);
    assert_eq!(pair.second_role, 1);
    assert_eq!(pair.paired_class_tag, "273");
    let mut parameter = parse_design_parameter(&parameter_record(
        Some(300),
        "40 mm",
        "Linear Dimension-3",
        Some("mm"),
        "d3",
        4.0,
    ))
    .unwrap();
    parameter.id = "f3d:Design/BulkStream.dat:design-parameter#301".into();
    parameter.record_index = 301;
    let owner = DesignParameterOwner {
        id: "f3d:Design/BulkStream.dat:design-parameter-owner#300".into(),
        byte_offset: pair.paired_byte_offset + 59,
        class_tag: "292".into(),
        record_index: 300,
        scope_record_index: 10,
        local_ordinal: 0,
        evaluated_value: 4.0,
        evaluated_value_offset: pair.paired_byte_offset + 99,
        parameter_record_index: 301,
        owned_ordinal: 3,
        variant: Some(0),
        companion_record_index: 302,
    };
    assert_eq!(
        crate::design::decode::dimension_frames::following_dimension_companion_record_index(
            &pair.id,
            pair.paired_byte_offset,
            std::slice::from_ref(&owner),
            std::slice::from_ref(&parameter),
        ),
        Some(302)
    );
    assert_eq!(
        crate::design::decode::dimension_frames::following_dimension_companion_record_index(
            &pair.id,
            pair.paired_byte_offset,
            &[owner.clone(), owner],
            std::slice::from_ref(&parameter),
        ),
        None
    );

    let mut nested = Vec::new();
    nested.extend_from_slice(&3u32.to_le_bytes());
    nested.extend_from_slice(b"341");
    nested.extend_from_slice(&229u32.to_le_bytes());
    nested.extend_from_slice(&bytes);
    let nested_end = nested.len();
    let nested = find_dimension_locus_pair(&nested, 0, nested_end, 228, &HashSet::from([192, 194]))
        .expect("nested paired dimension locus frame");
    assert_eq!(nested.byte_offset, 11);
    assert_eq!(nested.paired_byte_offset, 91);

    let mut competing = bytes.clone();
    competing.extend_from_slice(&bytes);
    assert!(find_dimension_locus_pair(
        &competing,
        0,
        competing.len(),
        228,
        &HashSet::from([192, 194]),
    )
    .is_none());
}

#[test]
fn dimension_null_locus_pair_preserves_null_and_typed_roles() {
    let mut bytes = vec![0; 74];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"277");
    bytes[7..11].copy_from_slice(&1394u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24] = 1;
    bytes[35..39].copy_from_slice(&10u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&1109u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"273");
    bytes.extend_from_slice(&1394u32.to_le_bytes());

    let pair = parse_dimension_null_locus_pair(&bytes, 0, 1290, &HashSet::from([1109]))
        .expect("null-locus dimension frame");
    assert_eq!(pair.companion_record_index, 1290);
    assert_eq!(pair.governing_companion_record_index, 1290);
    assert_eq!(pair.record_index, 1394);
    assert_eq!(pair.frame_length, 74);
    assert_eq!(pair.null_role, 10);
    assert_eq!(pair.geometry_record_index, 1109);
    assert_eq!(pair.geometry_role, 7);
    assert_eq!(pair.paired_class_tag, "273");

    assert!(parse_dimension_null_locus_pair(&bytes, 0, 1290, &HashSet::from([1110]),).is_none());

    let mut nested = Vec::new();
    nested.extend_from_slice(&3u32.to_le_bytes());
    nested.extend_from_slice(b"341");
    nested.extend_from_slice(&229u32.to_le_bytes());
    nested.extend_from_slice(&bytes);
    let nested_end = nested.len();
    let nested =
        find_dimension_null_locus_pair(&nested, 0, nested_end, 1290, &HashSet::from([1109]))
            .expect("null-locus frame following another indexed frame");
    assert_eq!(nested.byte_offset, 11);
    assert_eq!(nested.paired_byte_offset, 85);

    let mut axis_pair = pair;
    axis_pair.null_role = 14;
    axis_pair.geometry_role = 3;
    let entity = SketchEntity {
        id: SketchEntityId("f3d:model:sketch-entity#line".into()),
        sketch: SketchId("f3d:model:sketch#axis-angle".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 1.0),
        },
    };
    let parameter = cadmpeg_ir::features::ParameterId("f3d:model:parameter#angle".into());
    assert!(matches!(
        null_locus_dimension_definition(
            &axis_pair,
            &entity,
            "Angular Dimension-2",
            std::f64::consts::FRAC_PI_4,
            parameter.clone(),
        ),
        Some(SketchConstraintDefinition::AngleToAxis {
            entity: ref actual_entity,
            axis: SketchAxis::Horizontal,
            parameter: ref actual_parameter,
        }) if actual_entity == &entity.id && actual_parameter == &parameter
    ));
    assert!(null_locus_dimension_definition(
        &axis_pair,
        &entity,
        "Angular Dimension-2",
        0.5,
        parameter.clone(),
    )
    .is_none());
    axis_pair.null_role = 13;
    assert!(null_locus_dimension_definition(
        &axis_pair,
        &entity,
        "Angular Dimension-2",
        std::f64::consts::FRAC_PI_4,
        parameter,
    )
    .is_none());
}

#[test]
fn owner_scoped_radial_dimensions_preserve_repeated_measurements() {
    let mut entity = SketchEntity {
        id: SketchEntityId("f3d:model:sketch-entity#circle".into()),
        sketch: SketchId("f3d:model:sketch#radial".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(2.0, 3.0),
            radius: Length(5.0),
        },
    };
    let radius_parameter = cadmpeg_ir::features::ParameterId("parameter#radius".into());
    assert!(matches!(
        radial_dimension_definition(
            &entity,
            "Radius Dimension-2",
            0.5,
            radius_parameter.clone(),
        ),
        Some(SketchConstraintDefinition::Radius { entity: ref actual, parameter: ref p })
            if actual == &entity.id && p == &radius_parameter
    ));
    assert!(matches!(
        radial_dimension_definition(
            &entity,
            "Radial Dimension-3",
            0.5,
            radius_parameter.clone(),
        ),
        Some(SketchConstraintDefinition::Radius { entity: ref actual, .. })
            if actual == &entity.id
    ));
    let diameter_parameter = cadmpeg_ir::features::ParameterId("parameter#diameter".into());
    assert!(matches!(
        radial_dimension_definition(
            &entity,
            "Diameter Dimension-2",
            1.0,
            diameter_parameter.clone(),
        ),
        Some(SketchConstraintDefinition::Diameter { entity: ref actual, parameter: ref p })
            if actual == &entity.id && p == &diameter_parameter
    ));
    assert!(radial_dimension_definition(
        &entity,
        "Diameter Dimension-2",
        0.5,
        diameter_parameter.clone(),
    )
    .is_none());
    let parameter = parse_design_parameter(&parameter_record(
        Some(1),
        "10 mm",
        "Diameter Dimension-2",
        Some("mm"),
        "d1",
        1.0,
    ))
    .expect("diameter parameter");
    assert!(matches!(
        owner_scoped_radial_dimension_definition(
            std::slice::from_ref(&entity),
            &entity.sketch,
            &parameter,
            &diameter_parameter,
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::Diameter {
            entity: ref actual,
            ..
        }) if actual == &entity.id
    ));
    let mut duplicate = entity.clone();
    duplicate.id = SketchEntityId("f3d:model:sketch-entity#duplicate-circle".into());
    let SketchGeometry::Circle { radius, .. } = &mut duplicate.geometry else {
        unreachable!("test entity is circular")
    };
    radius.0 += 5.0e-7;
    assert!(matches!(
        owner_scoped_radial_dimension_definition(
            &[entity.clone(), duplicate.clone()],
            &entity.sketch,
            &parameter,
            &diameter_parameter,
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::RepeatedDiameter {
            entities,
            parameter,
        }) if entities == vec![entity.id.clone(), duplicate.id.clone()]
            && parameter == diameter_parameter
    ));

    let radial_parameter = parse_design_parameter(&parameter_record(
        Some(1),
        "5 mm",
        "Radial Dimension-2",
        Some("mm"),
        "d2",
        0.5,
    ))
    .expect("radial parameter");
    assert!(matches!(
        owner_scoped_radial_dimension_definition(
            &[entity.clone(), duplicate.clone()],
            &entity.sketch,
            &radial_parameter,
            &radius_parameter,
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::RepeatedRadius {
            entities,
            parameter,
        }) if entities == vec![entity.id.clone(), duplicate.id]
            && parameter == radius_parameter
    ));

    entity.geometry = SketchGeometry::Arc {
        center: Point2::new(2.0, 3.0),
        radius: Length(5.0),
        start_angle: cadmpeg_ir::features::Angle(0.0),
        end_angle: cadmpeg_ir::features::Angle(1.0),
    };
    assert!(
        radial_dimension_definition(&entity, "Diameter Dimension", 1.0, diameter_parameter,)
            .is_some()
    );
    entity.geometry = SketchGeometry::Ellipse {
        center: Point2::new(2.0, 3.0),
        major_angle: cadmpeg_ir::features::Angle(0.0),
        major_radius: Length(5.0),
        minor_radius: Length(3.0),
        start_angle: None,
        end_angle: None,
    };
    assert!(
        radial_dimension_definition(&entity, "Radius Dimension-2", 0.5, radius_parameter,)
            .is_none()
    );
}

#[test]
fn owner_scoped_line_lengths_preserve_repeated_entities() {
    let sketch = SketchId("f3d:model:sketch#line-length".into());
    let line = |name: &str, v: f64, length: f64| SketchEntity {
        id: SketchEntityId(format!("f3d:model:sketch-entity#{name}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, v),
            end: Point2::new(length, v),
        },
    };
    let first = line("first", 0.0, 4.0);
    let second = line("second", 2.0, 4.0 + 5.0e-7);
    let parameter = parse_design_parameter(&parameter_record(
        Some(1),
        "4 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d1",
        0.4,
    ))
    .expect("linear parameter");
    let parameter_id = cadmpeg_ir::features::ParameterId("parameter#line-length".into());

    assert!(matches!(
        owner_scoped_line_length_dimension_definition(
            std::slice::from_ref(&first),
            &sketch,
            &parameter,
            &parameter_id,
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Start(ref entity),
            second: SketchLocus::End(ref other),
            parameter: ref actual_parameter,
        }) if entity == &first.id && other == &first.id && actual_parameter == &parameter_id
    ));
    assert!(matches!(
        owner_scoped_line_length_dimension_definition(
            &[first.clone(), second.clone()],
            &sketch,
            &parameter,
            &parameter_id,
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::RepeatedLength {
            entities,
            parameter,
        }) if entities == vec![first.id, second.id]
            && parameter == parameter_id
    ));
}

#[test]
fn owner_scoped_angular_dimension_requires_one_matching_line_pair() {
    let sketch = SketchId("f3d:model:sketch#angular".into());
    let line = |name: &str, angle: f64| SketchEntity {
        id: SketchEntityId(format!("f3d:model:sketch-entity#{name}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(angle.cos(), angle.sin()),
        },
    };
    let horizontal = line("horizontal", 0.0);
    let sloped = line("sloped", std::f64::consts::FRAC_PI_6);
    let vertical = line("vertical", std::f64::consts::FRAC_PI_2);
    let parameter = parse_design_parameter(&parameter_record(
        Some(1),
        "30 deg",
        "Angular Dimension-2",
        Some("deg"),
        "d1",
        std::f64::consts::FRAC_PI_6,
    ))
    .expect("angular parameter");
    let parameter_id = cadmpeg_ir::features::ParameterId("parameter#angle".into());

    assert!(matches!(
        owner_scoped_angular_dimension_definition(
            &[horizontal.clone(), sloped.clone(), vertical.clone()],
            &sketch,
            &parameter,
            &parameter_id,
        ),
        Some(SketchConstraintDefinition::Angle {
            first,
            second,
            parameter,
        }) if first == horizontal.id && second == sloped.id && parameter == parameter_id
    ));

    let other_sloped = line("other-sloped", -std::f64::consts::FRAC_PI_6);
    assert!(owner_scoped_angular_dimension_definition(
        &[horizontal, sloped, vertical, other_sloped],
        &sketch,
        &parameter,
        &parameter_id,
    )
    .is_none());
}

#[test]
fn owner_scoped_point_dimensions_quotient_coincident_identities() {
    let sketch = SketchId("f3d:model:sketch#point-classes".into());
    let point = |name: &str, u: f64, v: f64| SketchEntity {
        id: SketchEntityId(format!("f3d:model:sketch-entity#{name}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(u, v),
        },
    };
    let lower = point("lower", -53.0, -20.875);
    let lower_duplicate = point("lower-duplicate", -53.0, -20.875 + 5.0e-7);
    let upper = point("upper", -53.0, -7.875);
    let parameter = parse_design_parameter(&parameter_record(
        Some(1),
        "13 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d19",
        1.3,
    ))
    .expect("linear parameter");
    let parameter_id = cadmpeg_ir::features::ParameterId("parameter#point-classes".into());

    assert!(matches!(
        unique_point_class_dimension_definition(
            &[lower.clone(), lower_duplicate, upper.clone()],
            &sketch,
            &parameter,
            &parameter_id,
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::VerticalDistance {
            first: SketchLocus::Entity(first),
            second: SketchLocus::Entity(second),
            parameter,
        }) if first == lower.id && second == upper.id && parameter == parameter_id
    ));

    let another_upper = point("another-upper", -40.0, -7.875);
    assert!(unique_point_class_dimension_definition(
        &[lower, upper, another_upper],
        &sketch,
        &parameter,
        &parameter_id,
        1.0e-6,
    )
    .is_none());
}

#[test]
fn radial_locus_groups_use_direct_curves_then_unique_center_witnesses() {
    let sketch = SketchId("f3d:model:sketch#radial-loci".into());
    let point = |id: &str, u, v| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(u, v),
        },
    };
    let circle = |id: &str, u, v, radius| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(u, v),
            radius: Length(radius),
        },
    };
    let center = point("center", 2.0, 3.0);
    let annotation = point("annotation", 7.0, 3.0);
    let measured = circle("measured", 2.0, 3.0, 5.0);
    let other_center = circle("other-center", 20.0, 30.0, 5.0);
    let other_radius = circle("other-radius", 2.0, 3.0, 7.0);
    let all = [
        center.clone(),
        annotation.clone(),
        measured.clone(),
        other_center,
        other_radius,
    ];
    let parameter = cadmpeg_ir::features::ParameterId("parameter#radial-loci".into());

    assert!(matches!(
        radial_locus_dimension_definition(
            &[&measured, &annotation],
            &all,
            "Radial Dimension-2",
            0.5,
            &parameter,
        ),
        Some(SketchConstraintDefinition::Radius { entity, .. }) if entity == measured.id
    ));
    let repeated = circle("repeated", 12.0, 3.0, 5.0);
    assert!(matches!(
        radial_locus_dimension_definition(
            &[&measured, &annotation, &repeated],
            &all,
            "Diameter Dimension-3",
            1.0,
            &parameter,
        ),
        Some(SketchConstraintDefinition::RepeatedDiameter { entities, parameter: actual })
            if entities == vec![measured.id.clone(), repeated.id] && actual == parameter
    ));
    assert!(matches!(
        radial_locus_dimension_definition(
            &[&center],
            &all,
            "Diameter Dimension-2",
            1.0,
            &parameter,
        ),
        Some(SketchConstraintDefinition::Diameter { entity, .. }) if entity == measured.id
    ));
}

#[test]
fn dimension_locus_group_preserves_roles_owner_state_and_return_order() {
    let mut bytes = vec![0; 101];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"286");
    bytes[7..11].copy_from_slice(&249u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24] = 1;
    bytes[25..29].copy_from_slice(&175u32.to_le_bytes());
    bytes[35..39].copy_from_slice(&2u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&217u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&1u32.to_le_bytes());
    bytes[55] = 1;
    bytes[56..60].copy_from_slice(&172u32.to_le_bytes());
    bytes[66..70].copy_from_slice(&1u32.to_le_bytes());
    bytes[74..78].copy_from_slice(&2u32.to_le_bytes());
    bytes[78] = 1;
    bytes[79..83].copy_from_slice(&217u32.to_le_bytes());
    bytes[89] = 1;
    bytes[90..94].copy_from_slice(&175u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"314");
    bytes.extend_from_slice(&250u32.to_le_bytes());

    let group = parse_dimension_locus_group(
        &bytes,
        0,
        240,
        &HashSet::from([175, 217]),
        &HashSet::from([172]),
    )
    .expect("counted dimension locus frame");
    assert_eq!(group.companion_record_index, 240);
    assert_eq!(group.record_index, 249);
    assert_eq!(group.frame_length, 101);
    assert_eq!(group.owner_reference, 172);
    assert_eq!(group.owner_role, 1);
    assert_eq!(group.state, 0);
    assert_eq!(group.loci[0].geometry_record_index, 175);
    assert_eq!(group.loci[0].role, 2);
    assert_eq!(group.loci[1].geometry_record_index, 217);
    assert_eq!(group.loci[1].role, 1);
    assert_eq!(group.return_members, [217, 175]);
    assert_eq!(group.next_class_tag, "314");
    assert_eq!(group.next_record_index, 250);

    let relation_at = |stream: &str, byte_offset| SketchRelation {
        id: format!("f3d:{stream}:sketch-relation#{byte_offset}"),
        record_index: 249,
        class_tag: "286".into(),
        byte_offset,
        state_offset: 66,
        owner_reference: 172,
        owner_entity_id: "0_172".into(),
        auxiliary_references: Vec::new(),
        auxiliary_reference_offsets: Vec::new(),
        members: vec![175, 217],
        resolved_members: Vec::new(),
        member_offsets: vec![25, 40],
        owner_reference_offset: 56,
        state: 0,
        constraint_kinds: vec![SketchConstraintKind::Coincident],
        unknown_constraint_bits: 0,
        member_relation_ordinals: Vec::new(),
        entity_genesis: None,
        pattern: None,
        return_members: vec![217, 175],
        resolved_return_members: Vec::new(),
        return_member_offsets: vec![79, 90],
        raw_bytes: bytes[..101].to_vec(),
    };
    let mut relations = vec![relation_at("native", 0), relation_at("other", 0)];
    let mut group = group;
    group.id = "f3d:native:design-dimension-locus-group#0".into();
    remove_dimension_frame_relations(&mut relations, &[], &[group], &[]);
    assert_eq!(relations.len(), 1);
    assert!(relations[0].id.starts_with("f3d:other:"));

    let body = bytes[11..101].to_vec();
    bytes.extend_from_slice(&body);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"315");
    bytes.extend_from_slice(&251u32.to_le_bytes());
    let groups = find_dimension_locus_groups(
        &bytes,
        0,
        bytes.len(),
        240,
        &HashSet::from([175, 217]),
        &HashSet::from([172]),
    );
    assert_eq!(
        groups
            .iter()
            .map(|group| group.record_index)
            .collect::<Vec<_>>(),
        [249, 250]
    );
}

#[test]
fn dimension_annotation_frame_links_nullable_loci_to_governing_owner() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"298");
    bytes.extend_from_slice(&388u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    for (reference, role) in [(0u32, 6u32), (354, 2), (376, 3)] {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&role.to_le_bytes());
    }
    push_genesis_block(&mut bytes, 0x202);
    let annotation_byte_offset = bytes.len();
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    push_reference(&mut bytes, 390);
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for reference in [376u32, 354] {
        push_reference(&mut bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&[0; 4]);
    let paired_byte_offset = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"287");
    bytes.extend_from_slice(&388u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    push_reference(&mut bytes, 201);
    bytes.extend_from_slice(&[0; 6]);
    bytes.resize(paired_byte_offset + 59, 0);

    let frame = parse_dimension_annotation_frame(
        &bytes,
        0,
        Some(383),
        &HashMap::from([(390, 391)]),
        &HashSet::from([354, 376]),
        &HashSet::from([201]),
    )
    .expect("annotated dimension frame");
    assert_eq!(frame.companion_record_index, Some(383));
    assert_eq!(frame.governing_companion_record_index, 391);
    assert_eq!(frame.entity_genesis, 0x202);
    assert_eq!(frame.annotation_byte_offset, annotation_byte_offset as u64);
    assert_eq!(frame.annotation_bytes, [0xaa, 0xbb, 0xcc]);
    assert_eq!(frame.operands[0].geometry_record_index, 0);
    assert_eq!(frame.return_members, [376, 354]);
    assert_eq!(frame.paired_byte_offset, paired_byte_offset as u64);
    assert_eq!(frame.owner_reference, 201);

    let leading = parse_dimension_annotation_frame(
        &bytes,
        0,
        None,
        &HashMap::from([(390, 391)]),
        &HashSet::from([354, 376]),
        &HashSet::from([201]),
    )
    .expect("scope-prefix dimension frame");
    assert_eq!(leading.companion_record_index, None);
    assert_eq!(leading.governing_owner_record_index, 390);
}

#[test]
fn work_point_direct_record_carries_model_space_position() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"427");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "WorkPoint");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);

    let point_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 27]);
    let position_at = bytes.len();
    for value in [1.25, -2.5, 3.75] {
        bytes.extend_from_slice(&f64::to_le_bytes(value));
    }
    bytes.extend_from_slice(&7u32.to_le_bytes());
    for _ in 0..3 {
        bytes.extend_from_slice(&f64::to_le_bytes(-1.0));
    }
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for target in [56u32, 57] {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(target).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.resize(point_at + 208, 0);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&55u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "427".into(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("WorkPoint scope");
    let frame = exact_work_point_position(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    assert_eq!(frame.position, [1.25, -2.5, 3.75]);
    assert_eq!(frame.position_offset, position_at as u64);
    assert_eq!(frame.reference_type, 7);
    assert_eq!(frame.input_record_indices, [56, 57]);
    bytes[point_at + 66..point_at + 70].copy_from_slice(&1u32.to_le_bytes());
    bytes[point_at + 94..point_at + 98].copy_from_slice(&1u32.to_le_bytes());
    bytes.drain(point_at + 197..point_at + 208);
    let frame = exact_work_point_position(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    assert_eq!(frame.position, [1.25, -2.5, 3.75]);
    assert_eq!(frame.position_offset, position_at as u64);
    assert_eq!(frame.reference_type, 1);
    assert_eq!(frame.input_record_indices, [56]);
}

#[test]
fn work_point_input_count_frames_the_rule_inputs() {
    // The counted input run is framed by its serialized count. The rule
    // selector is retained independently and does not impose a fixed arity.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"427");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "WorkPoint");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);

    let point_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 27]);
    let position_at = bytes.len();
    for value in [4.0, 5.0, 6.0] {
        bytes.extend_from_slice(&f64::to_le_bytes(value));
    }
    bytes.extend_from_slice(&18u32.to_le_bytes());
    for _ in 0..3 {
        bytes.extend_from_slice(&f64::to_le_bytes(-1.0));
    }
    let count_at = bytes.len();
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for target in [56u32, 57] {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(target).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.resize(point_at + 208, 0);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&55u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "427".into(),
        byte_offset: 0,
    };
    let records = IndexedRecordOffsets::build(&bytes);
    let scope = parse_parameter_scope(&bytes, &records, &header).expect("WorkPoint scope");
    let frame = exact_work_point_position(&bytes, &records, &scope, &HashMap::new())
        .expect("work point frame");
    assert_eq!(frame.reference_type, 18);
    assert_eq!(frame.input_record_indices, [56, 57]);

    bytes[count_at..count_at + 4].copy_from_slice(&1u32.to_le_bytes());
    let records = IndexedRecordOffsets::build(&bytes);
    let frame = exact_work_point_position(&bytes, &records, &scope, &HashMap::new())
        .expect("work point frame");
    assert_eq!(frame.reference_type, 18);
    assert_eq!(frame.input_record_indices, [56]);

    // A rule above the values the shipped range check admitted still names a
    // coordinate when its input arity agrees.
    bytes[position_at + 24..position_at + 28].copy_from_slice(&64u32.to_le_bytes());
    let records = IndexedRecordOffsets::build(&bytes);
    let frame = exact_work_point_position(&bytes, &records, &scope, &HashMap::new())
        .expect("work point frame");
    assert_eq!(frame.position, [4.0, 5.0, 6.0]);
    assert_eq!(frame.reference_type, 64);
    assert_eq!(frame.input_record_indices, [56]);
}

#[test]
fn move_matrix_decomposes_to_translation_and_axis_angle() {
    let angle = std::f64::consts::PI / 3.0;
    let transform: [[f64; 4]; 4] = [
        [angle.cos(), 0.0, angle.sin(), -14.0],
        [0.0, 1.0, 0.0, 2.0],
        [-angle.sin(), 0.0, angle.cos(), 9.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let rotation = crate::design::feature_project::matrix_axis_angle(&transform)
        .expect("nonidentity rotation");
    assert!((rotation.angle.0 - angle).abs() <= 1.0e-12);
    assert!((rotation.direction.x - 0.0).abs() <= 1.0e-12);
    assert!((rotation.direction.y - 1.0).abs() <= 1.0e-12);
    assert!((rotation.direction.z - 0.0).abs() <= 1.0e-12);
    assert_eq!(
        crate::design::feature_project::matrix_axis_angle(
            &crate::design::decode::sketch::identity_matrix()
        ),
        None
    );
}

#[test]
fn parameter_scope_parses_named_variable_tail() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"378");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Draft");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    lp_utf16(&mut bytes, "draft-name");
    bytes.extend_from_slice(&[0; 7]);

    bytes.push(1);
    bytes.push(0x4e);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.push(1);
    bytes.push(0x4d);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 0, 0]);
    bytes.push(1);
    bytes.push(0x4c);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);

    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "378".into(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("named variable-tail scope");
    assert_eq!(scope.kind, "Draft");
    assert_eq!(scope.feature_ordinal, 1);
    assert_eq!(scope.history_state_id, Some(7));
    assert_eq!(scope.previous_history_state_id, None);
    assert_eq!(scope.previous_history_state_id_offset, 0);
    assert_eq!(scope.reference_members, [55]);
    assert_eq!(scope.frame_length, paired_at as u64);

    let mut owner_scope = scope.clone();
    owner_scope.reference_members = vec![327, 330, 55, 56, 57, 58];
    let owners = vec![
        DesignParameterOwner {
            id: "f3d:test:owner#327".into(),
            byte_offset: 0,
            class_tag: "272".into(),
            record_index: 327,
            scope_record_index: 12,
            local_ordinal: 0,
            evaluated_value: 0.0,
            evaluated_value_offset: 111,
            parameter_record_index: 326,
            owned_ordinal: 3,
            variant: Some(0),
            companion_record_index: 328,
        },
        DesignParameterOwner {
            id: "f3d:test:owner#330".into(),
            byte_offset: 0,
            class_tag: "272".into(),
            record_index: 330,
            scope_record_index: 12,
            local_ordinal: 1,
            evaluated_value: 0.0,
            evaluated_value_offset: 222,
            parameter_record_index: 329,
            owned_ordinal: 4,
            variant: Some(0),
            companion_record_index: 331,
        },
    ];
    let operation = exact_draft_operation_with_owners(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &owner_scope,
        &owners,
    )
    .expect("owner-lane Draft operation");
    assert_eq!(operation.angle, 0.0);
    assert_eq!(operation.angle_record_index, 327);
    assert_eq!(operation.opposite_angle_record_index, 330);
    assert_eq!(operation.angle_offset, 111);
    assert_eq!(operation.opposite_angle_offset, 222);
}

#[test]
fn parameter_scope_parses_named_tail_with_empty_label() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"378");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "CylinderPrimitive");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);

    bytes.push(1);
    bytes.push(0x0f);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.push(1);
    bytes.push(0x0e);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 0, 0]);
    bytes.push(1);
    bytes.push(0x0d);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);

    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "378".into(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("empty-label named scope");
    assert_eq!(scope.kind, "CylinderPrimitive");
    assert_eq!(scope.frame_length, paired_at as u64);
    assert_eq!(scope.previous_history_state_id, None);
    assert_eq!(scope.previous_history_state_id_offset, 0);
}

#[test]
fn parameter_scope_uses_same_index_pair_and_fixed_kind_tail() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"301");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    let reference_count_at = bytes.len();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    let reference_at = bytes.len();
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Sketch");
    let feature_ordinal_at = bytes.len();
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "301".into(),
        byte_offset: 0,
    };

    let mut scope =
        parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header).unwrap();
    assert_eq!(scope.kind, "Sketch");
    assert_eq!(scope.feature_ordinal, 1);
    assert_eq!(scope.feature_ordinal_offset, feature_ordinal_at as u64);
    assert_eq!(scope.history_state_id, Some(7));
    assert_eq!(scope.previous_history_state_id, Some(2));
    assert_eq!(scope.reference_count_offset, reference_count_at as u64);
    assert_eq!(scope.reference_members, [55]);
    assert_eq!(scope.reference_member_offsets, [reference_at as u64]);
    assert_eq!(scope.frame_length, paired_at as u64);
    assert_eq!(scope.paired_class_tag, "261");
    assert_eq!(scope.paired_byte_offset, paired_at as u64);
    let discovered = crate::design::decode::scopes::parameter_scope_candidate_headers(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
    )
    .into_iter()
    .filter_map(|header| {
        parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
    })
    .collect::<Vec<_>>();
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].record_index, 12);

    let mut compact_tail = bytes.clone();
    compact_tail.remove(paired_at - 1);
    let compact = parse_parameter_scope(
        &compact_tail,
        &IndexedRecordOffsets::build(&compact_tail),
        &header,
    )
    .expect("scope with compact fixed tail");
    assert_eq!(compact.kind, "Sketch");
    assert_eq!(compact.frame_length, paired_at as u64 - 1);
    assert_eq!(compact.previous_history_state_id, Some(2));
    assert!(
        !crate::design::decode::scopes::parameter_scope_tail_length_is_valid("CopyPasteBodies", 78,)
    );

    for tail_length in [72, 76] {
        let mut legacy = bytes[..feature_ordinal_at].to_vec();
        let mut tail = vec![0; tail_length];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[30..34].copy_from_slice(&2u32.to_le_bytes());
        legacy.extend_from_slice(&tail);
        legacy.extend_from_slice(&3u32.to_le_bytes());
        legacy.extend_from_slice(b"261");
        legacy.extend_from_slice(&12u32.to_le_bytes());
        let decoded =
            parse_parameter_scope(&legacy, &IndexedRecordOffsets::build(&legacy), &header)
                .expect("scope with legacy fixed tail");
        assert_eq!(decoded.kind, "Sketch");
        assert_eq!(decoded.previous_history_state_id, Some(2));
        assert_eq!(
            decoded.previous_history_state_id_offset,
            (feature_ordinal_at + 30) as u64
        );
    }

    let mut extended_tail = bytes[..feature_ordinal_at].to_vec();
    let mut tail = [0; 87];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[41..45].copy_from_slice(&3u32.to_le_bytes());
    extended_tail.extend_from_slice(&tail);
    extended_tail.extend_from_slice(&3u32.to_le_bytes());
    extended_tail.extend_from_slice(b"261");
    extended_tail.extend_from_slice(&12u32.to_le_bytes());
    let extended = parse_parameter_scope(
        &extended_tail,
        &IndexedRecordOffsets::build(&extended_tail),
        &header,
    )
    .expect("scope with extended fixed tail");
    assert_eq!(extended.previous_history_state_id, Some(3));
    assert_eq!(
        extended.previous_history_state_id_offset,
        (feature_ordinal_at + 41) as u64
    );

    let mut copy_scope = Vec::new();
    copy_scope.extend_from_slice(&3u32.to_le_bytes());
    copy_scope.extend_from_slice(b"316");
    copy_scope.extend_from_slice(&12u32.to_le_bytes());
    copy_scope.extend_from_slice(&[0; 10]);
    copy_scope.extend_from_slice(&1u32.to_le_bytes());
    copy_scope.push(1);
    copy_scope.extend_from_slice(&55u32.to_le_bytes());
    copy_scope.extend_from_slice(&[0; 6]);
    copy_scope.extend_from_slice(&u32::MAX.to_le_bytes());
    lp_utf16(&mut copy_scope, "CopyPasteBodies");
    let copy_feature_ordinal_at = copy_scope.len();
    let mut copy_tail = [0; 110];
    copy_tail[0..4].copy_from_slice(&2u32.to_le_bytes());
    copy_tail[53..57].copy_from_slice(&u32::MAX.to_le_bytes());
    copy_scope.extend_from_slice(&copy_tail);
    let copy_paired_at = copy_scope.len();
    copy_scope.extend_from_slice(&3u32.to_le_bytes());
    copy_scope.extend_from_slice(b"259");
    copy_scope.extend_from_slice(&12u32.to_le_bytes());
    let copy = parse_parameter_scope(
        &copy_scope,
        &IndexedRecordOffsets::build(&copy_scope),
        &header,
    )
    .expect("CopyPasteBodies scope with extended tail");
    assert_eq!(copy.kind, "CopyPasteBodies");
    assert_eq!(copy.feature_ordinal, 2);
    assert_eq!(copy.feature_ordinal_offset, copy_feature_ordinal_at as u64);
    assert_eq!(copy.history_state_id, None);
    assert_eq!(copy.previous_history_state_id, None);
    assert_eq!(
        copy.previous_history_state_id_offset,
        (copy_feature_ordinal_at + 53) as u64
    );
    assert_eq!(copy.frame_length, copy_paired_at as u64);

    let mut operation_bytes = vec![0; 80];
    operation_bytes[29] = 1;
    operation_bytes[30..34].copy_from_slice(&55u32.to_le_bytes());
    operation_bytes[34..40].fill(0);
    operation_bytes[40] = 1;
    operation_bytes[41..45].copy_from_slice(&44u32.to_le_bytes());
    operation_bytes[45..51].fill(0);
    let body_group_at = operation_bytes.len();
    operation_bytes.extend_from_slice(&3u32.to_le_bytes());
    operation_bytes.extend_from_slice(b"264");
    operation_bytes.extend_from_slice(&55u32.to_le_bytes());
    operation_bytes.extend_from_slice(&[0; 10]);
    operation_bytes.extend_from_slice(&1u32.to_le_bytes());
    operation_bytes.push(1);
    operation_bytes.extend_from_slice(&66u32.to_le_bytes());
    operation_bytes.extend_from_slice(&[0; 6]);
    let relation_at = operation_bytes.len();
    operation_bytes.extend_from_slice(&3u32.to_le_bytes());
    operation_bytes.extend_from_slice(b"314");
    operation_bytes.extend_from_slice(&44u32.to_le_bytes());
    operation_bytes.extend_from_slice(&[0; 8]);
    operation_bytes.push(1);
    operation_bytes.extend_from_slice(&2u32.to_le_bytes());
    for suffix in [1206, 1215] {
        operation_bytes.push(1);
        operation_bytes.extend_from_slice(&u32::to_le_bytes(suffix));
        operation_bytes.extend_from_slice(&[0; 10]);
    }
    let mut operation_scope = copy.clone();
    operation_scope.byte_offset = 0;
    operation_scope.paired_byte_offset = 60;
    operation_scope.reference_members = vec![55, 66];
    let operation = crate::design::decode::scopes::exact_copy_paste_bodies_operation(
        &operation_bytes,
        &IndexedRecordOffsets::build(&operation_bytes),
        &operation_scope,
    )
    .expect("single-body CopyPasteBodies relation");
    assert_eq!(operation.body_group_record_index, 55);
    assert_eq!(operation.body_group_byte_offset, body_group_at as u64);
    assert_eq!(operation.body_operand_record_indices, [66]);
    assert_eq!(operation.relation_record_index, 44);
    assert_eq!(operation.relation_byte_offset, relation_at as u64);
    assert_eq!(operation.source_body_entity_suffixes, [1206]);
    assert_eq!(operation.copied_body_entity_suffixes, [1215]);

    // A Sketch scope may also carry the generic ordered reference table
    // used by `EntityGenesis`-form streams; the table then has more than
    // one member and the entity join happens by unique suffix match.
    let mut generic_reference = vec![1];
    generic_reference.extend_from_slice(&56u32.to_le_bytes());
    generic_reference.extend_from_slice(&[0; 6]);
    let mut generic_references = bytes.clone();
    generic_references[reference_count_at..reference_count_at + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    generic_references.splice(reference_at + 10..reference_at + 10, generic_reference);
    let generic_scope = parse_parameter_scope(
        &generic_references,
        &IndexedRecordOffsets::build(&generic_references),
        &header,
    )
    .expect("generic-table Sketch scope");
    assert_eq!(generic_scope.kind, "Sketch");
    assert_eq!(generic_scope.reference_members, [55, 56]);

    let work_plane_at = bytes.len();
    let mut work_plane = vec![0; 362];
    work_plane[0..4].copy_from_slice(&3u32.to_le_bytes());
    work_plane[4..7].copy_from_slice(b"293");
    work_plane[7..11].copy_from_slice(&55u32.to_le_bytes());
    work_plane[55] = 1;
    work_plane[57] = 1;
    work_plane[58..62].copy_from_slice(&99u32.to_le_bytes());
    let transform: [[f64; 4]; 4] = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 76 + ordinal * 8;
        work_plane[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    work_plane.extend_from_slice(&3u32.to_le_bytes());
    work_plane.extend_from_slice(b"261");
    work_plane.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&work_plane);
    let decoded = exact_work_plane_frame(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
        .expect("exact WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (work_plane_at + 76) as u64);
    assert_eq!(decoded.reference, Some((99, (work_plane_at + 58) as u64)));

    let extended_at = bytes.len();
    let mut extended = vec![0; 373];
    extended[0..4].copy_from_slice(&3u32.to_le_bytes());
    extended[4..7].copy_from_slice(b"263");
    extended[7..11].copy_from_slice(&57u32.to_le_bytes());
    extended[55..58].copy_from_slice(&[1, 0, 1]);
    extended[58..62].copy_from_slice(&100u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 76 + ordinal * 8;
        extended[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    extended.extend_from_slice(&3u32.to_le_bytes());
    extended.extend_from_slice(b"261");
    extended.extend_from_slice(&57u32.to_le_bytes());
    bytes.extend_from_slice(&extended);
    let mut extended_scope = scope.clone();
    extended_scope.reference_members = vec![57];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &extended_scope,
    )
    .expect("extended referenced WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (extended_at + 76) as u64);
    assert_eq!(decoded.reference, Some((100, (extended_at + 58) as u64)));

    let direct_at = bytes.len();
    let mut direct = vec![0; 352];
    direct[0..4].copy_from_slice(&3u32.to_le_bytes());
    direct[4..7].copy_from_slice(b"293");
    direct[7..11].copy_from_slice(&56u32.to_le_bytes());
    direct[55] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 66 + ordinal * 8;
        direct[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    direct.extend_from_slice(&3u32.to_le_bytes());
    direct.extend_from_slice(b"261");
    direct.extend_from_slice(&56u32.to_le_bytes());
    bytes.extend_from_slice(&direct);
    let mut direct_scope = scope.clone();
    direct_scope.reference_members = vec![56];
    let decoded =
        exact_work_plane_frame(&bytes, &IndexedRecordOffsets::build(&bytes), &direct_scope)
            .expect("direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (direct_at + 66) as u64);
    assert_eq!(decoded.reference, None);

    let extended_direct_at = bytes.len();
    let mut extended_direct = vec![0; 363];
    extended_direct[0..4].copy_from_slice(&3u32.to_le_bytes());
    extended_direct[4..7].copy_from_slice(b"289");
    extended_direct[7..11].copy_from_slice(&61u32.to_le_bytes());
    extended_direct[55] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 66 + ordinal * 8;
        extended_direct[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    extended_direct.extend_from_slice(&3u32.to_le_bytes());
    extended_direct.extend_from_slice(b"258");
    extended_direct.extend_from_slice(&61u32.to_le_bytes());
    bytes.extend_from_slice(&extended_direct);
    let mut extended_direct_scope = scope.clone();
    extended_direct_scope.reference_members = vec![61];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &extended_direct_scope,
    )
    .expect("extended direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (extended_direct_at + 66) as u64);
    assert_eq!(decoded.reference, None);

    let large_direct_at = bytes.len();
    let mut large_direct = vec![0; 374];
    large_direct[0..4].copy_from_slice(&3u32.to_le_bytes());
    large_direct[4..7].copy_from_slice(b"267");
    large_direct[7..11].copy_from_slice(&62u32.to_le_bytes());
    large_direct[55] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 66 + ordinal * 8;
        large_direct[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    large_direct.extend_from_slice(&3u32.to_le_bytes());
    large_direct.extend_from_slice(b"258");
    large_direct.extend_from_slice(&62u32.to_le_bytes());
    bytes.extend_from_slice(&large_direct);
    let mut large_direct_scope = scope.clone();
    large_direct_scope.reference_members = vec![62];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &large_direct_scope,
    )
    .expect("large direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (large_direct_at + 66) as u64);
    assert_eq!(decoded.reference, None);

    let mut axis_bytes = vec![0; 232];
    axis_bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    axis_bytes[4..7].copy_from_slice(b"701");
    axis_bytes[7..11].copy_from_slice(&100u32.to_le_bytes());
    axis_bytes[21..25].copy_from_slice(&8u32.to_le_bytes());
    let axis_values = [1.0_f64, 2.0, 3.0, 0.0, -3.0, 4.0, 0.0, 0.0];
    for (ordinal, value) in axis_values.into_iter().enumerate() {
        let at = 25 + ordinal * 8;
        axis_bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    axis_bytes[118..122].copy_from_slice(&2u32.to_le_bytes());
    for (ordinal, record_index) in [102_u32, 104].into_iter().enumerate() {
        let at = 122 + ordinal * 11;
        axis_bytes[at] = 1;
        axis_bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }
    axis_bytes.extend_from_slice(&3u32.to_le_bytes());
    axis_bytes.extend_from_slice(b"258");
    axis_bytes.extend_from_slice(&100u32.to_le_bytes());
    for (record_index, point) in [(102_u32, [1.0_f64, 2.0, 3.0]), (104, [1.0, -1.0, 7.0])] {
        let start = axis_bytes.len();
        axis_bytes.resize(start + 197, 0);
        axis_bytes[start..start + 4].copy_from_slice(&3u32.to_le_bytes());
        axis_bytes[start + 4..start + 7].copy_from_slice(b"702");
        axis_bytes[start + 7..start + 11].copy_from_slice(&record_index.to_le_bytes());
        for (ordinal, value) in point.into_iter().enumerate() {
            let at = start + 42 + ordinal * 8;
            axis_bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        axis_bytes.extend_from_slice(&3u32.to_le_bytes());
        axis_bytes.extend_from_slice(b"258");
        axis_bytes.extend_from_slice(&record_index.to_le_bytes());
    }
    let mut axis_scope = scope.clone();
    axis_scope.id = "f3d:native:parameter-scope#55".into();
    axis_scope.kind = "WorkAxis".into();
    axis_scope.reference_members = vec![100, 101, 102, 103, 104];
    let construction = exact_work_axis_construction(
        &axis_bytes,
        &IndexedRecordOffsets::build(&axis_bytes),
        &axis_scope,
    )
    .expect("exact two-point WorkAxis construction");
    assert_eq!(construction.origin, [1.0, 2.0, 3.0]);
    assert_eq!(construction.displacement, [0.0, -3.0, 4.0]);
    assert_eq!(construction.origin_offset, 25);
    assert_eq!(construction.displacement_offset, 49);
    assert_eq!(construction.point_record_indices, [102, 104]);
    axis_scope.work_axis_construction = Some(construction);
    let (axis_features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&axis_scope),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        axis_features.as_slice(),
        [Feature {
            definition: FeatureDefinition::DatumAxis { origin, direction },
            ..
        }] if *origin == Point3::new(10.0, 20.0, 30.0)
            && *direction == Vector3::new(0.0, -0.6, 0.8)
    ));

    let compact_at = bytes.len();
    let mut compact = vec![0; 321];
    compact[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact[4..7].copy_from_slice(b"293");
    compact[7..11].copy_from_slice(&58u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact.extend_from_slice(&3u32.to_le_bytes());
    compact.extend_from_slice(b"261");
    compact.extend_from_slice(&58u32.to_le_bytes());
    bytes.extend_from_slice(&compact);
    let mut compact_scope = scope.clone();
    compact_scope.reference_members = vec![58];
    let decoded =
        exact_work_plane_frame(&bytes, &IndexedRecordOffsets::build(&bytes), &compact_scope)
            .expect("compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (compact_at + 49) as u64);
    assert_eq!(decoded.reference, None);

    let compact_431_at = bytes.len();
    let mut compact_431 = vec![0; 325];
    compact_431[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_431[4..7].copy_from_slice(b"431");
    compact_431[7..11].copy_from_slice(&67u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact_431[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_431.extend_from_slice(&3u32.to_le_bytes());
    compact_431.extend_from_slice(b"257");
    compact_431.extend_from_slice(&67u32.to_le_bytes());
    bytes.extend_from_slice(&compact_431);
    let mut compact_431_scope = scope.clone();
    compact_431_scope.reference_members = vec![67];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_431_scope,
    )
    .expect("class-431 compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (compact_431_at + 49) as u64);
    assert_eq!(decoded.reference, None);

    let compact_364_at = bytes.len();
    let mut compact_364 = vec![0; 321];
    compact_364[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_364[4..7].copy_from_slice(b"364");
    compact_364[7..11].copy_from_slice(&65u32.to_le_bytes());
    compact_364[46] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact_364[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_364.extend_from_slice(&3u32.to_le_bytes());
    compact_364.extend_from_slice(b"264");
    compact_364.extend_from_slice(&65u32.to_le_bytes());
    bytes.extend_from_slice(&compact_364);
    let mut compact_364_scope = scope.clone();
    compact_364_scope.reference_members = vec![65];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_364_scope,
    )
    .expect("class-364 marked compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (compact_364_at + 49) as u64);
    assert_eq!(decoded.reference, None);

    let compact_364_variant_at = bytes.len();
    let mut compact_364_variant = vec![0; 321];
    compact_364_variant[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_364_variant[4..7].copy_from_slice(b"364");
    compact_364_variant[7..11].copy_from_slice(&66u32.to_le_bytes());
    compact_364_variant[45..49].copy_from_slice(&[0xcc, 0xcd, 0, 0]);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact_364_variant[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_364_variant.extend_from_slice(&3u32.to_le_bytes());
    compact_364_variant.extend_from_slice(b"264");
    compact_364_variant.extend_from_slice(&66u32.to_le_bytes());
    bytes.extend_from_slice(&compact_364_variant);
    let mut compact_364_variant_scope = scope.clone();
    compact_364_variant_scope.reference_members = vec![66];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_364_variant_scope,
    )
    .expect("class-364 compact direct WorkPlane frame variant");
    assert_eq!(decoded.transform, transform);
    assert_eq!(
        decoded.transform_offset,
        (compact_364_variant_at + 49) as u64
    );
    assert_eq!(decoded.reference, None);

    let compact_450_at = bytes.len();
    let mut compact_450 = vec![0; 326];
    compact_450[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_450[4..7].copy_from_slice(b"450");
    compact_450[7..11].copy_from_slice(&59u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 50 + ordinal * 8;
        compact_450[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_450.extend_from_slice(&3u32.to_le_bytes());
    compact_450.extend_from_slice(b"259");
    compact_450.extend_from_slice(&59u32.to_le_bytes());
    bytes.extend_from_slice(&compact_450);
    let mut compact_450_scope = scope.clone();
    compact_450_scope.reference_members = vec![59];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_450_scope,
    )
    .expect("class-450 compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (compact_450_at + 50) as u64);
    assert_eq!(decoded.reference, None);

    let compact_409_short_at = bytes.len();
    let mut compact_409_short = compact_450.clone();
    compact_409_short[4..7].copy_from_slice(b"409");
    compact_409_short[7..11].copy_from_slice(&64u32.to_le_bytes());
    compact_409_short[330..333].copy_from_slice(b"258");
    compact_409_short[333..337].copy_from_slice(&64u32.to_le_bytes());
    bytes.extend_from_slice(&compact_409_short);
    let mut compact_409_short_scope = scope.clone();
    compact_409_short_scope.reference_members = vec![64];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_409_short_scope,
    )
    .expect("short class-409 compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (compact_409_short_at + 50) as u64);
    assert_eq!(decoded.reference, None);

    let compact_409_at = bytes.len();
    let mut compact_409 = vec![0; 337];
    compact_409[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_409[4..7].copy_from_slice(b"409");
    compact_409[7..11].copy_from_slice(&63u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 50 + ordinal * 8;
        compact_409[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_409.extend_from_slice(&3u32.to_le_bytes());
    compact_409.extend_from_slice(b"258");
    compact_409.extend_from_slice(&63u32.to_le_bytes());
    bytes.extend_from_slice(&compact_409);
    let mut compact_409_scope = scope.clone();
    compact_409_scope.reference_members = vec![63];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_409_scope,
    )
    .expect("class-409 compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (compact_409_at + 50) as u64);
    assert_eq!(decoded.reference, None);

    let joint_origin_at = bytes.len();
    let mut joint_origin = vec![0; 336];
    joint_origin[0..4].copy_from_slice(&3u32.to_le_bytes());
    joint_origin[4..7].copy_from_slice(b"450");
    joint_origin[7..11].copy_from_slice(&60u32.to_le_bytes());
    joint_origin[45] = 1;
    joint_origin[46..50].copy_from_slice(&61u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 60 + ordinal * 8;
        joint_origin[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    joint_origin.extend_from_slice(&3u32.to_le_bytes());
    joint_origin.extend_from_slice(b"259");
    joint_origin.extend_from_slice(&60u32.to_le_bytes());
    bytes.extend_from_slice(&joint_origin);
    let mut joint_origin_scope = scope.clone();
    joint_origin_scope.kind = "JointOrigin".into();
    joint_origin_scope.reference_members = vec![60];
    let decoded = exact_joint_origin_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &joint_origin_scope,
    )
    .expect("exact JointOrigin frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (joint_origin_at + 60) as u64);
    assert_eq!(decoded.reference, Some((61, (joint_origin_at + 46) as u64)));

    let compact_joint_origin_at = bytes.len();
    let mut compact_joint_origin = vec![0; 385];
    compact_joint_origin[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_joint_origin[4..7].copy_from_slice(b"364");
    compact_joint_origin[7..11].copy_from_slice(&67u32.to_le_bytes());
    compact_joint_origin[45..49].copy_from_slice(&[1, 1, 0, 0]);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact_joint_origin[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_joint_origin.extend_from_slice(&3u32.to_le_bytes());
    compact_joint_origin.extend_from_slice(b"264");
    compact_joint_origin.extend_from_slice(&67u32.to_le_bytes());
    bytes.extend_from_slice(&compact_joint_origin);
    let mut compact_joint_origin_scope = scope.clone();
    compact_joint_origin_scope.kind = "JointOrigin".into();
    compact_joint_origin_scope.reference_members = vec![67];
    let decoded = exact_joint_origin_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_joint_origin_scope,
    )
    .expect("exact compact JointOrigin frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(
        decoded.transform_offset,
        (compact_joint_origin_at + 49) as u64
    );
    assert_eq!(decoded.reference, None);

    let move_at = bytes.len();
    let mut move_frame = vec![0; 254];
    move_frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    move_frame[4..7].copy_from_slice(b"368");
    move_frame[7..11].copy_from_slice(&90u32.to_le_bytes());
    move_frame[43..47].copy_from_slice(&5u32.to_le_bytes());
    let mut move_transform = identity_matrix();
    move_transform[1][3] = 15.0;
    for (ordinal, value) in move_transform.into_iter().flatten().enumerate() {
        let at = 48 + ordinal * 8;
        move_frame[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    move_frame.extend_from_slice(&3u32.to_le_bytes());
    move_frame.extend_from_slice(b"265");
    move_frame.extend_from_slice(&90u32.to_le_bytes());
    bytes.extend_from_slice(&move_frame);
    let mut move_scope = scope.clone();
    move_scope.kind = "Move".into();
    move_scope.reference_members = vec![90];
    let decoded = crate::design::decode::scopes::exact_move_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &move_scope,
    )
    .expect("class-368 Move frame");
    assert_eq!(decoded.transform, move_transform);
    assert_eq!(decoded.transform_offset, (move_at + 48) as u64);
    assert_eq!(decoded.form, 5);

    let compact_move_at = bytes.len();
    let mut compact_move = vec![0; 253];
    compact_move[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_move[4..7].copy_from_slice(b"296");
    compact_move[7..11].copy_from_slice(&91u32.to_le_bytes());
    compact_move[43..47].copy_from_slice(&1u32.to_le_bytes());
    for (ordinal, value) in move_transform.into_iter().flatten().enumerate() {
        let at = 48 + ordinal * 8;
        compact_move[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_move.extend_from_slice(&3u32.to_le_bytes());
    compact_move.extend_from_slice(b"265");
    compact_move.extend_from_slice(&91u32.to_le_bytes());
    bytes.extend_from_slice(&compact_move);
    let mut compact_move_scope = scope.clone();
    compact_move_scope.kind = "Move".into();
    compact_move_scope.reference_members = vec![91];
    let decoded = crate::design::decode::scopes::exact_move_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_move_scope,
    )
    .expect("class-296 Move frame");
    assert_eq!(decoded.transform, move_transform);
    assert_eq!(decoded.transform_offset, (compact_move_at + 48) as u64);
    assert_eq!(decoded.transform_record_index, 91);
    assert_eq!(decoded.form, 1);
    assert_eq!(decoded.form_offset, (compact_move_at + 43) as u64);
    bytes[compact_move_at + 4..compact_move_at + 7].copy_from_slice(b"362");
    bytes[compact_move_at + 43..compact_move_at + 47].copy_from_slice(&5u32.to_le_bytes());
    let decoded = crate::design::decode::scopes::exact_move_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_move_scope,
    )
    .expect("class-362 Move frame");
    assert_eq!(decoded.transform, move_transform);
    assert_eq!(decoded.form, 5);

    let scale_at = bytes.len();
    let mut scale = vec![0; 317];
    scale[20..24].copy_from_slice(&1u32.to_le_bytes());
    scale[25..33].copy_from_slice(&1.5f64.to_le_bytes());
    for (offset, record_index) in [(33, 105u32), (44, 101), (68, 102)] {
        scale[offset] = 1;
        scale[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
    }
    scale[55..59].copy_from_slice(&1u32.to_le_bytes());
    scale[60..64].copy_from_slice(&1u32.to_le_bytes());
    scale[64..68].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&scale);
    let mut scale_scope = scope.clone();
    scale_scope.byte_offset = scale_at as u64;
    scale_scope.kind = "Maßstab".into();
    scale_scope.frame_length = 317;
    scale_scope.reference_members = vec![101, 102, 103, 104, 105];
    assert_eq!(
        exact_scale_operation(&bytes, &scale_scope),
        Some(DesignScaleOperation {
            body_group_record_index: 102,
            center_record_index: 101,
            uniform_factor: 1.5,
            uniform_factor_offset: (scale_at + 25) as u64,
        })
    );

    let sphere_at = bytes.len();
    let mut sphere = vec![0; 462];
    sphere[0..4].copy_from_slice(&3u32.to_le_bytes());
    sphere[4..7].copy_from_slice(b"302");
    sphere[7..11].copy_from_slice(&80u32.to_le_bytes());
    sphere[25..29].copy_from_slice(&4u32.to_le_bytes());
    sphere[29] = 1;
    sphere[30] = 1;
    sphere[41] = 1;
    sphere[42..46].copy_from_slice(&70u32.to_le_bytes());
    sphere[52] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 64 + ordinal * 8;
        sphere[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&sphere);
    let mut diameter = vec![0; 104];
    diameter[0..4].copy_from_slice(&3u32.to_le_bytes());
    diameter[4..7].copy_from_slice(b"277");
    diameter[7..11].copy_from_slice(&70u32.to_le_bytes());
    diameter[40..48].copy_from_slice(&8.0f64.to_le_bytes());
    diameter.extend_from_slice(&3u32.to_le_bytes());
    diameter.extend_from_slice(b"261");
    diameter.extend_from_slice(&70u32.to_le_bytes());
    bytes.extend_from_slice(&diameter);
    let mut sphere_scope = scope.clone();
    sphere_scope.byte_offset = sphere_at as u64;
    sphere_scope.kind = "SpherePrimitive".into();
    sphere_scope.frame_length = 462;
    assert!(matches!(
        exact_solid_primitive(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &sphere_scope,
            &[],
        ),
        Some(DesignSolidPrimitive::Sphere {
            diameter: 8.0,
            diameter_record_index: 70,
            operation: DesignExtrudeOperation::NewBody,
            ..
        })
    ));

    let torus_at = bytes.len();
    let mut torus = vec![0; 486];
    torus[0..4].copy_from_slice(&3u32.to_le_bytes());
    torus[4..7].copy_from_slice(b"305");
    torus[7..11].copy_from_slice(&81u32.to_le_bytes());
    torus[25..29].copy_from_slice(&4u32.to_le_bytes());
    torus[29] = 1;
    torus[30] = 1;
    torus[31..35].copy_from_slice(&71u32.to_le_bytes());
    torus[41] = 1;
    torus[52] = 1;
    torus[53..57].copy_from_slice(&72u32.to_le_bytes());
    torus[63] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 75 + ordinal * 8;
        torus[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&torus);
    for (record_index, value) in [(71u32, 15.0f64), (72, 4.0)] {
        let mut diameter = vec![0; 104];
        diameter[0..4].copy_from_slice(&3u32.to_le_bytes());
        diameter[4..7].copy_from_slice(b"277");
        diameter[7..11].copy_from_slice(&record_index.to_le_bytes());
        diameter[40..48].copy_from_slice(&value.to_le_bytes());
        diameter.extend_from_slice(&3u32.to_le_bytes());
        diameter.extend_from_slice(b"261");
        diameter.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&diameter);
    }
    let mut torus_scope = scope.clone();
    torus_scope.byte_offset = torus_at as u64;
    torus_scope.kind = "TorusPrimitive".into();
    torus_scope.frame_length = 486;
    assert!(matches!(
        exact_solid_primitive(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &torus_scope,
            &[],
        ),
        Some(DesignSolidPrimitive::Torus {
            major_diameter: 15.0,
            minor_diameter: 4.0,
            operation: DesignExtrudeOperation::NewBody,
            ..
        })
    ));

    let offset_at = bytes.len();
    let mut offset = vec![0; 286];
    offset[25] = 1;
    offset[26..30].copy_from_slice(&73u32.to_le_bytes());
    bytes.extend_from_slice(&offset);
    let mut distance = vec![0; 104];
    distance[0..4].copy_from_slice(&3u32.to_le_bytes());
    distance[4..7].copy_from_slice(b"277");
    distance[7..11].copy_from_slice(&73u32.to_le_bytes());
    distance[40..48].copy_from_slice(&(-0.5f64).to_le_bytes());
    distance.extend_from_slice(&3u32.to_le_bytes());
    distance.extend_from_slice(b"261");
    distance.extend_from_slice(&73u32.to_le_bytes());
    bytes.extend_from_slice(&distance);
    let mut offset_scope = scope.clone();
    offset_scope.byte_offset = offset_at as u64;
    offset_scope.kind = "OffsetFaces".into();
    offset_scope.frame_length = 286;
    offset_scope.reference_members = vec![1, 2, 3, 73];
    assert!(matches!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &offset_scope),
        Some(DesignDirectFaceOperation::OffsetFaces {
            distance: -0.5,
            distance_record_index: 73,
            ..
        })
    ));

    let compact_offset_at = bytes.len();
    let mut compact_offset = vec![0; 275];
    compact_offset[25] = 1;
    compact_offset[26..30].copy_from_slice(&1_777u32.to_le_bytes());
    bytes.extend_from_slice(&compact_offset);
    let mut compact_distance = vec![0; 105];
    compact_distance[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_distance[4..7].copy_from_slice(b"312");
    compact_distance[7..11].copy_from_slice(&1_777u32.to_le_bytes());
    compact_distance[24] = 1;
    compact_distance[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    compact_distance[40..48].copy_from_slice(&0.254f64.to_le_bytes());
    compact_distance.extend_from_slice(&3u32.to_le_bytes());
    compact_distance.extend_from_slice(b"259");
    compact_distance.extend_from_slice(&1_777u32.to_le_bytes());
    bytes.extend_from_slice(&compact_distance);
    offset_scope.byte_offset = compact_offset_at as u64;
    offset_scope.frame_length = 275;
    offset_scope.reference_members = vec![1, 2, 1_777];
    assert!(matches!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &offset_scope),
        Some(DesignDirectFaceOperation::OffsetFaces {
            distance: 0.254,
            distance_record_index: 1_777,
            ..
        })
    ));

    let thicken_at = bytes.len();
    let mut thicken = vec![0; 301];
    thicken[47] = 1;
    thicken[48..52].copy_from_slice(&74u32.to_le_bytes());
    bytes.extend_from_slice(&thicken);
    let mut thickness = vec![0; 104];
    thickness[0..4].copy_from_slice(&3u32.to_le_bytes());
    thickness[4..7].copy_from_slice(b"277");
    thickness[7..11].copy_from_slice(&74u32.to_le_bytes());
    thickness[40..48].copy_from_slice(&(-1.0f64).to_le_bytes());
    thickness.extend_from_slice(&3u32.to_le_bytes());
    thickness.extend_from_slice(b"261");
    thickness.extend_from_slice(&74u32.to_le_bytes());
    bytes.extend_from_slice(&thickness);
    let mut thicken_scope = scope.clone();
    thicken_scope.byte_offset = thicken_at as u64;
    thicken_scope.kind = "Thicken".into();
    thicken_scope.frame_length = 301;
    thicken_scope.reference_members = vec![1, 2, 74];
    assert!(matches!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &thicken_scope),
        Some(DesignDirectFaceOperation::Thicken {
            signed_thickness: -1.0,
            thickness_record_index: 74,
            ..
        })
    ));
    thicken_scope.frame_length = 295;
    assert_eq!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &thicken_scope),
        None
    );
    let compact_thicken_at = bytes.len();
    let mut compact_thicken = vec![0; 295];
    compact_thicken[45] = 1;
    compact_thicken[46] = 1;
    compact_thicken[47..51].copy_from_slice(&74u32.to_le_bytes());
    bytes.extend_from_slice(&compact_thicken);
    thicken_scope.byte_offset = compact_thicken_at as u64;
    assert!(matches!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &thicken_scope),
        Some(DesignDirectFaceOperation::Thicken {
            signed_thickness: -1.0,
            thickness_record_index: 74,
            ..
        })
    ));
    let shifted_thicken_at = bytes.len();
    let mut shifted_thicken = vec![0; 312];
    shifted_thicken[34] = 1;
    shifted_thicken[35..39].copy_from_slice(&200u32.to_le_bytes());
    shifted_thicken[46..48].copy_from_slice(&[1, 1]);
    shifted_thicken[48..52].copy_from_slice(&74u32.to_le_bytes());
    bytes.extend_from_slice(&shifted_thicken);
    let shifted_thicken_scope = DesignParameterScope {
        byte_offset: shifted_thicken_at as u64,
        frame_length: 312,
        reference_members: vec![74, 200, 201, 202],
        ..thicken_scope.clone()
    };
    assert!(matches!(
        exact_direct_face_operation(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &shifted_thicken_scope,
        ),
        Some(DesignDirectFaceOperation::Thicken {
            signed_thickness: -1.0,
            thickness_record_index: 74,
            ..
        })
    ));
    thicken_scope.direct_face_operation =
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &thicken_scope);
    let thicken_group = DesignConstructionOperandGroup {
        id: "thicken-group".into(),
        scope_record_index: thicken_scope.record_index,
        scope_reference_ordinal: 0,
        record_index: 200,
        byte_offset: 0,
        class_tag: "264".into(),
        members: vec![201],
        lost_edge_references: Vec::new(),
        member_offsets: vec![0],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![202],
            trailing_record_offsets: vec![0],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role: 0x0000_0005_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 0,

        paired_class_tag: "264".into(),
        paired_byte_offset: 0,
    };
    assert!(matches!(
        crate::design::feature_project::project_thicken(&thicken_scope, &[], std::slice::from_ref(&thicken_group)),
        Some(cadmpeg_ir::features::FeatureDefinition::Thicken {
            faces: cadmpeg_ir::features::FaceSelection::Native(native),
            thickness: Some(cadmpeg_ir::features::Length(10.0)),
            side: Some(cadmpeg_ir::features::ThickenSide::Reverse),
        }) if native == "thicken-group"
    ));
    let mut bounded_face_thicken_group = thicken_group.clone();
    bounded_face_thicken_group.role = 0x0000_0012_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_thicken(
            &thicken_scope,
            &[],
            std::slice::from_ref(&bounded_face_thicken_group)
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Thicken {
            faces: cadmpeg_ir::features::FaceSelection::Native(native),
            ..
        }) if native == "thicken-group"
    ));
    let shell_at = bytes.len();
    let mut shell = vec![0; 278];
    shell[25] = 1;
    shell[27] = 1;
    shell[28..32].copy_from_slice(&1_778u32.to_le_bytes());
    shell[51..55].copy_from_slice(&1u32.to_le_bytes());
    shell[55] = 1;
    shell[56..60].copy_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&shell);
    let mut shell_thickness = vec![0; 105];
    shell_thickness[0..4].copy_from_slice(&3u32.to_le_bytes());
    shell_thickness[4..7].copy_from_slice(b"321");
    shell_thickness[7..11].copy_from_slice(&1_778u32.to_le_bytes());
    shell_thickness[24] = 1;
    shell_thickness[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    shell_thickness[40..48].copy_from_slice(&0.5f64.to_le_bytes());
    shell_thickness.extend_from_slice(&3u32.to_le_bytes());
    shell_thickness.extend_from_slice(b"265");
    shell_thickness.extend_from_slice(&1_778u32.to_le_bytes());
    bytes.extend_from_slice(&shell_thickness);
    let mut shell_scope = scope.clone();
    shell_scope.byte_offset = shell_at as u64;
    shell_scope.kind = "Shell".into();
    shell_scope.frame_length = 278;
    shell_scope.reference_members = vec![200, 201, 1_778];
    shell_scope.direct_face_operation =
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &shell_scope);
    assert!(matches!(
        shell_scope.direct_face_operation,
        Some(DesignDirectFaceOperation::Shell {
            thickness: 0.5,
            thickness_record_index: 1_778,
            outward: true,
            ..
        })
    ));
    let mut shell_group = thicken_group.clone();
    shell_group.id = "shell-group".into();
    shell_group.scope_record_index = shell_scope.record_index;
    shell_group.role = 0x0000_0010_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_shell(&shell_scope, &[], std::slice::from_ref(&shell_group)),
        Some(cadmpeg_ir::features::FeatureDefinition::Shell {
            removed_faces: cadmpeg_ir::features::FaceSelection::Native(native),
            thickness: Some(cadmpeg_ir::features::Length(5.0)),
            outward: Some(true),
            ..
        }) if native == "shell-group"
    ));
    let compact_shell_at = bytes.len();
    let mut compact_shell = vec![0; 268];
    compact_shell[21] = 1;
    compact_shell[22] = 1;
    compact_shell[23..27].copy_from_slice(&9_000u32.to_le_bytes());
    compact_shell[42..46].copy_from_slice(&1u32.to_le_bytes());
    compact_shell[46] = 1;
    compact_shell[47..51].copy_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&compact_shell);
    let mut compact_shell_thickness = vec![0; 103];
    compact_shell_thickness[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_shell_thickness[4..7].copy_from_slice(b"354");
    compact_shell_thickness[7..11].copy_from_slice(&9_000u32.to_le_bytes());
    compact_shell_thickness[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    compact_shell_thickness[24] = 1;
    compact_shell_thickness[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    compact_shell_thickness[40..48].copy_from_slice(&0.25f64.to_le_bytes());
    compact_shell_thickness[48] = 1;
    compact_shell_thickness[49..53].copy_from_slice(&9_001u32.to_le_bytes());
    compact_shell_thickness[59..63].copy_from_slice(&10u32.to_le_bytes());
    compact_shell_thickness[67] = 1;
    compact_shell_thickness[68..72].copy_from_slice(&scope.record_index.to_le_bytes());
    compact_shell_thickness[80] = 1;
    compact_shell_thickness[81..85].copy_from_slice(&9_002u32.to_le_bytes());
    compact_shell_thickness[92] = 1;
    compact_shell_thickness[93..97].copy_from_slice(&scope.record_index.to_le_bytes());
    compact_shell_thickness.extend_from_slice(&3u32.to_le_bytes());
    compact_shell_thickness.extend_from_slice(b"258");
    compact_shell_thickness.extend_from_slice(&9_000u32.to_le_bytes());
    bytes.extend_from_slice(&compact_shell_thickness);
    let mut compact_shell_scope = DesignParameterScope {
        byte_offset: compact_shell_at as u64,
        frame_length: 268,
        reference_members: vec![200, 201, 9_000],
        ..shell_scope.clone()
    };
    assert!(matches!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &compact_shell_scope),
        Some(DesignDirectFaceOperation::Shell {
            thickness: 0.25,
            thickness_record_index: 9_000,
            outward: true,
            outward_offset,
            ..
        }) if outward_offset == (compact_shell_at + 21) as u64
    ));
    let shifted_shell_at = bytes.len();
    let mut shifted_shell = vec![0; 278];
    shifted_shell[20] = 1;
    shifted_shell[25] = 1;
    shifted_shell[27] = 1;
    shifted_shell[28..32].copy_from_slice(&9_000u32.to_le_bytes());
    shifted_shell[51..55].copy_from_slice(&1u32.to_le_bytes());
    shifted_shell[55] = 1;
    shifted_shell[56..60].copy_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&shifted_shell);
    let shifted_shell_scope = DesignParameterScope {
        byte_offset: shifted_shell_at as u64,
        frame_length: 278,
        reference_members: vec![9_000, 200, 201],
        ..shell_scope.clone()
    };
    assert!(matches!(
        exact_direct_face_operation(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &shifted_shell_scope,
        ),
        Some(DesignDirectFaceOperation::Shell {
            thickness: 0.25,
            thickness_record_index: 9_000,
            outward: false,
            outward_offset,
            ..
        }) if outward_offset == (shifted_shell_at + 21) as u64
    ));
    compact_shell_scope.direct_face_operation = exact_direct_face_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_shell_scope,
    );
    shell_group.role = 0x0000_0004_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_shell(
            &compact_shell_scope,
            &[],
            std::slice::from_ref(&shell_group)
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Shell {
            bodies: Some(cadmpeg_ir::features::BodySelection::Native(body)),
            removed_faces: cadmpeg_ir::features::FaceSelection::Faces(removed),
            thickness: Some(cadmpeg_ir::features::Length(2.5)),
            outward: Some(true),
            ..
        }) if body == "shell-group" && removed.is_empty()
    ));
    offset_scope.direct_face_operation =
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &offset_scope);
    let mut offset_group = thicken_group.clone();
    offset_group.id = "offset-group".into();
    offset_group.scope_record_index = offset_scope.record_index;
    offset_group.role = 0x0000_0010_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_offset_faces(
            &offset_scope,
            &[],
            &[],
            std::slice::from_ref(&offset_group)
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::MoveFace {
            faces: cadmpeg_ir::features::FaceSelection::Native(native),
            motion: cadmpeg_ir::features::FaceMotion::Offset {
                distance: cadmpeg_ir::features::Length(2.54)
            },
        }) if native == "offset-group"
    ));
    bytes[compact_thicken_at + 46] = 0;
    assert_eq!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &thicken_scope),
        None
    );

    for (record_index, ordinal, value) in [(75u32, 0u8, -2.0f64), (76, 1, 0.0)] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut extrude_scope = scope.clone();
    extrude_scope.kind = "Extrude".into();
    extrude_scope.extrude_prologue = Some(DesignExtrudePrologue::ReferenceAware {
        reference: None,
        operation: DesignExtrudeOperation::NewBody,
        operation_offset: 28,
        extent_discriminators: [1, 2],
        extent: DesignExtrudeExtent::OneSidedDistance,
        extent_discriminator_offsets: [32, 36],
        direction_reversed: false,
        direction_reversed_offset: 40,
        solid_operation: true,
        solid_operation_offset: 41,
        start: DesignExtrudeStart::ProfilePlane,
        start_offset: 42,
    });
    extrude_scope.reference_members = vec![50, 75, 76, 51];
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &extrude_scope
        ),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(
                DesignFixedExtrudeScalar {
                    value: -2.0,
                    record_index: 75,
                    value_offset: (bytes.len() - 2 * 115 + 40) as u64,
                },
            )),
            taper_angle: Some(DesignFixedExtrudeScalar {
                value: 0.0,
                record_index: 76,
                value_offset: (bytes.len() - 115 + 40) as u64,
            }),
        })
    );
    extrude_scope.reference_members = vec![50, 75, 51];
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &extrude_scope
        ),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(
                DesignFixedExtrudeScalar {
                    value: -2.0,
                    record_index: 75,
                    value_offset: (bytes.len() - 2 * 115 + 40) as u64,
                },
            )),
            taper_angle: None,
        })
    );
    extrude_scope.reference_members = vec![50, 75, 76, 51];
    extrude_scope.reference_members.push(75);
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &extrude_scope
        ),
        None
    );

    let extend_distance_at = bytes.len();
    let extend_distance_record_index = 400u32;
    let extend_boundary_record_index = 500u32;
    let extend_edge_record_indices = [503u32, 507u32];
    let mut extend_distance = vec![0; 104];
    extend_distance[0..4].copy_from_slice(&3u32.to_le_bytes());
    extend_distance[4..7].copy_from_slice(b"299");
    extend_distance[7..11].copy_from_slice(&extend_distance_record_index.to_le_bytes());
    extend_distance[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    extend_distance[24] = 1;
    extend_distance[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    extend_distance[35] = 0;
    extend_distance[40..48].copy_from_slice(&0.04f64.to_le_bytes());
    extend_distance[48] = 1;
    extend_distance[49..53].copy_from_slice(&(extend_distance_record_index - 1).to_le_bytes());
    extend_distance[59..63].copy_from_slice(&1016u32.to_le_bytes());
    extend_distance[67] = 1;
    extend_distance[68..72].copy_from_slice(&scope.record_index.to_le_bytes());
    extend_distance[78..81].copy_from_slice(&[1, 0, 0]);
    extend_distance[81] = 1;
    extend_distance[82..86].copy_from_slice(&(extend_distance_record_index + 1).to_le_bytes());
    extend_distance[93] = 1;
    extend_distance[94..98].copy_from_slice(&scope.record_index.to_le_bytes());
    extend_distance.extend_from_slice(&3u32.to_le_bytes());
    extend_distance.extend_from_slice(b"258");
    extend_distance.extend_from_slice(&extend_distance_record_index.to_le_bytes());
    bytes.extend_from_slice(&extend_distance);

    let extend_boundary_at = bytes.len();
    let extend_boundary_tail = 25 + extend_edge_record_indices.len() * 11;
    let mut extend_boundary = vec![0; 113 + extend_edge_record_indices.len() * 11];
    extend_boundary[0..4].copy_from_slice(&3u32.to_le_bytes());
    extend_boundary[4..7].copy_from_slice(b"290");
    extend_boundary[7..11].copy_from_slice(&extend_boundary_record_index.to_le_bytes());
    extend_boundary[21..25]
        .copy_from_slice(&(extend_edge_record_indices.len() as u32).to_le_bytes());
    for (ordinal, record_index) in extend_edge_record_indices.iter().enumerate() {
        let at = 25 + ordinal * 11;
        extend_boundary[at] = 1;
        extend_boundary[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }
    extend_boundary[extend_boundary_tail + 2..extend_boundary_tail + 6]
        .copy_from_slice(&1u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 6] = 1;
    extend_boundary[extend_boundary_tail + 7..extend_boundary_tail + 11]
        .copy_from_slice(&900u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 21..extend_boundary_tail + 25]
        .copy_from_slice(&8u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 35..extend_boundary_tail + 39]
        .copy_from_slice(&210u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 39..extend_boundary_tail + 47]
        .copy_from_slice(&1.0e-6f64.to_le_bytes());
    extend_boundary[extend_boundary_tail + 47..extend_boundary_tail + 51]
        .copy_from_slice(&210u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 51] = 1;
    extend_boundary[extend_boundary_tail + 52..extend_boundary_tail + 56]
        .copy_from_slice(&(extend_boundary_record_index + 2).to_le_bytes());
    extend_boundary[extend_boundary_tail + 62..extend_boundary_tail + 65]
        .copy_from_slice(&[1, 0, 0]);
    extend_boundary[extend_boundary_tail + 65] = 1;
    extend_boundary[extend_boundary_tail + 66..extend_boundary_tail + 70]
        .copy_from_slice(&(extend_boundary_record_index + 1).to_le_bytes());
    extend_boundary[extend_boundary_tail + 77] = 1;
    extend_boundary[extend_boundary_tail + 78..extend_boundary_tail + 82]
        .copy_from_slice(&scope.record_index.to_le_bytes());
    extend_boundary.extend_from_slice(&3u32.to_le_bytes());
    extend_boundary.extend_from_slice(b"258");
    extend_boundary.extend_from_slice(&extend_boundary_record_index.to_le_bytes());
    bytes.extend_from_slice(&extend_boundary);

    let mut extend_scope = scope.clone();
    extend_scope.id = "f3d:native:parameter-scope#12".into();
    extend_scope.kind = "SurfaceExtend".into();
    extend_scope.reference_members = vec![
        extend_distance_record_index,
        extend_boundary_record_index,
        extend_edge_record_indices[0],
        extend_edge_record_indices[1],
    ];
    let operation =
        exact_surface_extend_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &extend_scope)
            .expect("exact SurfaceExtend construction");
    assert_eq!(
        operation,
        DesignSurfaceExtendOperation {
            distance: 0.04,
            distance_offset: (extend_distance_at + 40) as u64,
            distance_record_index: extend_distance_record_index,
            method: DesignSurfaceExtendMethod::Tangent,
            method_offset: (extend_boundary_at + extend_boundary_tail + 2) as u64,
            boundary_record_index: extend_boundary_record_index,
            boundary_reference_record_index: 900,
            boundary_reference_offset: (extend_boundary_at + extend_boundary_tail + 6) as u64,
            edge_record_indices: extend_edge_record_indices.to_vec(),
            tolerance: 1.0e-6,
            tolerance_offset: (extend_boundary_at + extend_boundary_tail + 39) as u64,
        }
    );
    extend_scope.surface_extend_operation = Some(operation);
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&extend_scope),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features.as_slice(),
        [Feature {
            definition: FeatureDefinition::ExtendSurface {
                faces: FaceSelection::Native(native),
                distance: Some(Length(distance)),
                method: cadmpeg_ir::features::SurfaceExtension::Linear,
            },
            ..
        }] if native.ends_with(":design-record#500") && *distance == 0.4
    ));

    bytes[extend_distance_at + 40..extend_distance_at + 48]
        .copy_from_slice(&(-0.4f64).to_le_bytes());
    bytes[extend_boundary_at + extend_boundary_tail + 21
        ..extend_boundary_at + extend_boundary_tail + 25]
        .copy_from_slice(&65u32.to_le_bytes());
    extend_scope.kind = "SurfaceOffset".into();
    extend_scope.surface_extend_operation = None;
    let operation =
        exact_surface_offset_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &extend_scope)
            .expect("exact SurfaceOffset construction");
    assert_eq!(
        operation,
        DesignSurfaceOffsetOperation {
            distance: -0.4,
            distance_offset: (extend_distance_at + 40) as u64,
            distance_record_index: extend_distance_record_index,
            support: DesignSurfaceOffsetSupport::BoundaryCarrier {
                boundary_mode: 1,
                boundary_mode_offset: (extend_boundary_at + extend_boundary_tail + 2) as u64,
                boundary_record_index: extend_boundary_record_index,
                boundary_reference_record_index: 900,
                boundary_reference_offset: (extend_boundary_at + extend_boundary_tail + 6) as u64,
                edge_record_indices: extend_edge_record_indices.to_vec(),
                tolerance: 1.0e-6,
                tolerance_offset: (extend_boundary_at + extend_boundary_tail + 39) as u64,
            },
        }
    );
    extend_scope.surface_offset_operation = Some(operation);
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&extend_scope),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features.as_slice(),
        [Feature {
            definition: FeatureDefinition::OffsetSurface {
                faces: FaceSelection::Native(native),
                distance: Some(Length(distance)),
            },
            ..
        }] if native.ends_with(":design-record#500") && *distance == -4.0
    ));

    let grouped_record_index = 600u32;
    let grouped_member_record_index = 601u32;
    let mut grouped = Vec::new();
    grouped.extend_from_slice(&3u32.to_le_bytes());
    grouped.extend_from_slice(b"282");
    grouped.extend_from_slice(&grouped_record_index.to_le_bytes());
    grouped.extend_from_slice(&[0; 10]);
    grouped.extend_from_slice(&1u32.to_le_bytes());
    grouped.push(1);
    grouped.extend_from_slice(&grouped_member_record_index.to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.extend_from_slice(&[0; 2]);
    grouped.extend_from_slice(&1u32.to_le_bytes());
    grouped.push(1);
    grouped.extend_from_slice(&(grouped_record_index + 2).to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.extend_from_slice(&0x0000_0041_0000_0000u64.to_le_bytes());
    grouped.extend_from_slice(&[0; 10]);
    grouped.extend_from_slice(&252u32.to_le_bytes());
    grouped.extend_from_slice(&0.0001f64.to_le_bytes());
    grouped.extend_from_slice(&252u32.to_le_bytes());
    grouped.push(1);
    grouped.extend_from_slice(&(grouped_record_index + 2).to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.extend_from_slice(&[1, 1, 0, 1]);
    grouped.extend_from_slice(&(grouped_record_index + 1).to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.push(0);
    grouped.push(1);
    grouped.extend_from_slice(&extend_scope.record_index.to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.extend_from_slice(&3u32.to_le_bytes());
    grouped.extend_from_slice(b"260");
    grouped.extend_from_slice(&grouped_record_index.to_le_bytes());
    bytes.extend_from_slice(&grouped);
    let mut grouped_scope = extend_scope.clone();
    grouped_scope.reference_members = vec![
        extend_distance_record_index,
        grouped_record_index,
        grouped_member_record_index,
    ];
    let grouped_operation = exact_surface_offset_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &grouped_scope,
    )
    .expect("exact grouped SurfaceOffset construction");
    assert_eq!(
        grouped_operation,
        DesignSurfaceOffsetOperation {
            distance: -0.4,
            distance_offset: (extend_distance_at + 40) as u64,
            distance_record_index: extend_distance_record_index,
            support: DesignSurfaceOffsetSupport::FaceGroups {
                group_record_indices: vec![grouped_record_index],
            },
        }
    );

    bytes[extend_boundary_at + 21..extend_boundary_at + 25]
        .copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        exact_surface_offset_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &extend_scope,),
        None
    );

    let embedded_default_at = bytes.len();
    for (record_index, ordinal) in [(273u32, 0u8), (274, 1)] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let embedded_distance_at = bytes.len();
    let embedded_distance_record_index = 275u32;
    let mut embedded_distance = vec![0; 100];
    embedded_distance[0..4].copy_from_slice(&3u32.to_le_bytes());
    embedded_distance[4..7].copy_from_slice(b"314");
    embedded_distance[7..11].copy_from_slice(&embedded_distance_record_index.to_le_bytes());
    embedded_distance[21] = 1;
    embedded_distance[22..26].copy_from_slice(&scope.record_index.to_le_bytes());
    embedded_distance[32..36].copy_from_slice(&1u32.to_le_bytes());
    embedded_distance[36] = 1;
    embedded_distance[37..41].copy_from_slice(&999u32.to_le_bytes());
    embedded_distance[47..51].copy_from_slice(&210u32.to_le_bytes());
    embedded_distance[51..59].copy_from_slice(&0.25f64.to_le_bytes());
    embedded_distance[59..63].copy_from_slice(&210u32.to_le_bytes());
    embedded_distance[63] = 1;
    embedded_distance[64..68].copy_from_slice(&(embedded_distance_record_index + 2).to_le_bytes());
    embedded_distance[74] = 1;
    embedded_distance[77] = 1;
    embedded_distance[78..82].copy_from_slice(&(embedded_distance_record_index + 1).to_le_bytes());
    embedded_distance[89] = 1;
    embedded_distance[90..94].copy_from_slice(&scope.record_index.to_le_bytes());
    embedded_distance.extend_from_slice(&3u32.to_le_bytes());
    embedded_distance.extend_from_slice(b"258");
    embedded_distance.extend_from_slice(&embedded_distance_record_index.to_le_bytes());
    bytes.extend_from_slice(&embedded_distance);
    extrude_scope.reference_members = vec![50, 273, 274, embedded_distance_record_index, 51];
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &extrude_scope
        ),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::DistanceConstruction(
                DesignFixedExtrudeScalar {
                    value: 0.25,
                    record_index: embedded_distance_record_index,
                    value_offset: (embedded_distance_at + 51) as u64,
                },
            )),
            taper_angle: Some(DesignFixedExtrudeScalar {
                value: 0.0,
                record_index: 274,
                value_offset: (embedded_default_at + 115 + 40) as u64,
            }),
        })
    );
    extrude_scope.reference_members.insert(2, 273);
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &extrude_scope
        ),
        None
    );

    let draft_start = bytes.len();
    for (record_index, ordinal, value) in [(175u32, 0u8, 0.4f64), (176, 1, 0.0)] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut draft_scope = scope.clone();
    draft_scope.kind = "Draft".into();
    draft_scope.frame_length = 361;
    draft_scope.reference_members = vec![175, 176, 181, 182, 186, 190, 193];
    let expected = Some(DesignDraftOperation {
        angle: 0.4,
        angle_record_index: 175,
        angle_offset: (draft_start + 40) as u64,
        opposite_angle_record_index: 176,
        opposite_angle_offset: (draft_start + 155) as u64,
    });
    assert_eq!(
        exact_draft_operation_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &draft_scope,
            &[],
        ),
        expected
    );

    // The ordered reference table is in record-index order, so the scalar lanes
    // hold no fixed position in it. Their local ordinals order them, and moving
    // them within the table must not change the recovered operation.
    draft_scope.reference_members = vec![181, 182, 186, 190, 193, 175, 176];
    assert_eq!(
        exact_draft_operation_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &draft_scope,
            &[],
        ),
        expected
    );

    // A table that reaches only one of the two lanes has no complete operation.
    draft_scope.reference_members = vec![175, 181, 182, 186, 190, 193];
    assert_eq!(
        exact_draft_operation_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &draft_scope,
            &[],
        ),
        None
    );

    // Fewer than six references cannot carry the two lanes plus both groups.
    draft_scope.reference_members = vec![175, 176, 181, 182, 186];
    assert_eq!(
        exact_draft_operation_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &draft_scope,
            &[],
        ),
        None
    );
    draft_scope.reference_members = vec![175, 176, 181, 182, 186, 190, 193];

    let fillet_start = bytes.len();
    for (record_index, ordinal, value) in [
        (77u32, 0u8, 1.0f64),
        (78, 1, 0.0),
        (79, 2, 0.65),
        (87, 3, 0.4),
        (88, 4, 0.2),
    ] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut fillet_scope = scope.clone();
    fillet_scope.kind = "Fillet".into();
    fillet_scope.reference_members = vec![77, 50, 78, 79, 87, 88];
    assert_eq!(
        exact_fixed_fillet_parameters(&bytes, &IndexedRecordOffsets::build(&bytes), &fillet_scope),
        Some(DesignFixedFilletParameters {
            groups: vec![crate::records::DesignFixedFilletGroup {
                tangency_weight: Some(crate::records::DesignFixedFilletTangencyWeight {
                    value: 1.0,
                    record_index: 77,
                    value_offset: (fillet_start + 40) as u64,
                }),
                radii: vec![0.0, 0.65, 0.4],
                radius_record_indexes: vec![78, 79, 87],
                radius_offsets: vec![
                    (fillet_start + 115 + 40) as u64,
                    (fillet_start + 230 + 40) as u64,
                    (fillet_start + 345 + 40) as u64,
                ],
                intermediate_parameters: vec![0.2],
                intermediate_parameter_record_indexes: vec![88],
                intermediate_parameter_offsets: vec![(fillet_start + 460 + 40) as u64],
            }],
        })
    );
    fillet_scope.reference_members = vec![50, 77];
    assert_eq!(
        exact_fixed_fillet_parameters(&bytes, &IndexedRecordOffsets::build(&bytes), &fillet_scope),
        Some(DesignFixedFilletParameters {
            groups: vec![crate::records::DesignFixedFilletGroup {
                tangency_weight: None,
                radii: vec![1.0],
                radius_record_indexes: vec![77],
                radius_offsets: vec![(fillet_start + 40) as u64],
                intermediate_parameters: Vec::new(),
                intermediate_parameter_record_indexes: Vec::new(),
                intermediate_parameter_offsets: Vec::new(),
            }],
        })
    );

    let dynamic_scalar_at = bytes.len();
    let mut dynamic_scalar = vec![0; 103];
    dynamic_scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
    dynamic_scalar[4..7].copy_from_slice(b"406");
    dynamic_scalar[7..11].copy_from_slice(&89u32.to_le_bytes());
    dynamic_scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    dynamic_scalar[24] = 1;
    dynamic_scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    dynamic_scalar[40..48].copy_from_slice(&0.5f64.to_le_bytes());
    dynamic_scalar[48] = 1;
    dynamic_scalar[49..53].copy_from_slice(&90u32.to_le_bytes());
    dynamic_scalar[67] = 1;
    dynamic_scalar[68..72].copy_from_slice(&scope.record_index.to_le_bytes());
    dynamic_scalar[80] = 1;
    dynamic_scalar[81..85].copy_from_slice(&91u32.to_le_bytes());
    dynamic_scalar[92] = 1;
    dynamic_scalar[93..97].copy_from_slice(&scope.record_index.to_le_bytes());
    bytes.extend_from_slice(&dynamic_scalar);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&89u32.to_le_bytes());
    fillet_scope.reference_members = vec![89];
    assert_eq!(
        exact_fixed_fillet_parameters(&bytes, &IndexedRecordOffsets::build(&bytes), &fillet_scope),
        Some(DesignFixedFilletParameters {
            groups: vec![crate::records::DesignFixedFilletGroup {
                tangency_weight: None,
                radii: vec![0.5],
                radius_record_indexes: vec![89],
                radius_offsets: vec![(dynamic_scalar_at + 40) as u64],
                intermediate_parameters: Vec::new(),
                intermediate_parameter_record_indexes: Vec::new(),
                intermediate_parameter_offsets: Vec::new(),
            }],
        })
    );

    let second_group_at = bytes.len();
    for (record_index, ordinal, value) in [
        (92u32, 0u8, 1.0f64),
        (93, 1, 0.5),
        (94, 2, 0.75),
        (95, 3, 0.25),
    ] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"406");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"259");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    fillet_scope.reference_members = vec![92, 93, 94, 95];
    let fixed =
        exact_fixed_fillet_parameters(&bytes, &IndexedRecordOffsets::build(&bytes), &fillet_scope)
            .expect("two constant-radius Fillet scalar groups");
    assert_eq!(fixed.groups.len(), 2);
    assert_eq!(fixed.groups[0].radii, [0.5]);
    assert_eq!(fixed.groups[1].radii, [0.25]);
    assert_eq!(
        fixed.groups[1]
            .tangency_weight
            .as_ref()
            .map(|weight| (weight.value, weight.value_offset)),
        Some((0.75, (second_group_at + 2 * 115 + 40) as u64))
    );

    let chamfer_scalar_start = bytes.len();
    let mut chamfer_scalar = vec![0; 104];
    chamfer_scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
    chamfer_scalar[4..7].copy_from_slice(b"277");
    chamfer_scalar[7..11].copy_from_slice(&86u32.to_le_bytes());
    chamfer_scalar[24] = 1;
    chamfer_scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    chamfer_scalar[35] = 0;
    chamfer_scalar[40..48].copy_from_slice(&0.04f64.to_le_bytes());
    chamfer_scalar.extend_from_slice(&3u32.to_le_bytes());
    chamfer_scalar.extend_from_slice(b"261");
    chamfer_scalar.extend_from_slice(&86u32.to_le_bytes());
    bytes.extend_from_slice(&chamfer_scalar);
    let mut chamfer_scope = scope.clone();
    chamfer_scope.kind = "Chamfer".into();
    chamfer_scope.reference_members = vec![86];
    assert_eq!(
        exact_fixed_chamfer_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &chamfer_scope,
            &[],
        ),
        Some(DesignFixedChamferParameters::EqualDistance {
            distance: crate::records::DesignFixedChamferDistance {
                value: 0.04,
                record_index: 86,
                value_offset: (chamfer_scalar_start + 40) as u64,
            },
        })
    );
    let second_chamfer_scalar_start = bytes.len();
    let mut second_chamfer_scalar = chamfer_scalar[..104].to_vec();
    second_chamfer_scalar[7..11].copy_from_slice(&96u32.to_le_bytes());
    second_chamfer_scalar[35] = 1;
    second_chamfer_scalar[40..48].copy_from_slice(&0.08f64.to_le_bytes());
    second_chamfer_scalar.extend_from_slice(&3u32.to_le_bytes());
    second_chamfer_scalar.extend_from_slice(b"261");
    second_chamfer_scalar.extend_from_slice(&96u32.to_le_bytes());
    bytes.extend_from_slice(&second_chamfer_scalar);
    chamfer_scope.reference_members = vec![86, 96];
    assert_eq!(
        exact_fixed_chamfer_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &chamfer_scope,
            &[],
        ),
        Some(DesignFixedChamferParameters::TwoDistances {
            first: crate::records::DesignFixedChamferDistance {
                value: 0.04,
                record_index: 86,
                value_offset: (chamfer_scalar_start + 40) as u64,
            },
            second: crate::records::DesignFixedChamferDistance {
                value: 0.08,
                record_index: 96,
                value_offset: (second_chamfer_scalar_start + 40) as u64,
            },
        })
    );
    chamfer_scope.id = "f3d:Design/BulkStream.dat:scope#12".into();
    let indexed_owner = DesignParameterOwner {
        id: "f3d:Design/BulkStream.dat:parameter-owner#97".into(),
        byte_offset: 0,
        class_tag: "292".into(),
        record_index: 97,
        scope_record_index: chamfer_scope.record_index,
        local_ordinal: 0,
        evaluated_value: 0.04,
        evaluated_value_offset: 0,
        parameter_record_index: 98,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 99,
    };
    assert_eq!(
        exact_fixed_chamfer_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &chamfer_scope,
            std::slice::from_ref(&indexed_owner),
        ),
        None
    );

    let revolve_start = bytes.len();
    let mut revolve = vec![0; 386];
    revolve[25..29].copy_from_slice(&4u32.to_le_bytes());
    revolve[29..33].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&revolve);
    let revolve_scalar_start = bytes.len();
    for (record_index, ordinal, value) in [(1_779u32, 0u8, 3.5f64), (1_780, 1, 0.0)] {
        let mut scalar = vec![0; 105];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"321");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"265");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut revolve_scope = scope.clone();
    revolve_scope.byte_offset = revolve_start as u64;
    revolve_scope.kind = "Revolve".into();
    revolve_scope.frame_length = 386;
    revolve_scope.reference_members = vec![200, 201, 202, 203, 1_779, 1_780, 204];
    let revolve_construction = exact_path_feature_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &revolve_scope,
        &[],
    );
    assert_eq!(
        revolve_construction,
        Some(DesignPathFeatureConstruction::Revolve {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: (revolve_start + 25) as u64,
            angle: 3.5,
            angle_record_index: 1_779,
            angle_offset: (revolve_scalar_start + 40) as u64,
            opposite_angle_record_index: Some(1_780),
            opposite_angle_offset: Some((revolve_scalar_start + 116 + 40) as u64),
        })
    );

    let indexed_revolve_start = bytes.len();
    let indexed_angle_record_index = 1_790u32;
    let mut indexed_revolve = vec![0; 377];
    indexed_revolve[21..25].copy_from_slice(&2u32.to_le_bytes());
    indexed_revolve[25..29].copy_from_slice(&2u32.to_le_bytes());
    indexed_revolve[30..34].copy_from_slice(&1u32.to_le_bytes());
    indexed_revolve[34] = 1;
    indexed_revolve[35..43].copy_from_slice(&u64::from(indexed_angle_record_index).to_le_bytes());
    bytes.extend_from_slice(&indexed_revolve);
    let mut indexed_revolve_scope = revolve_scope.clone();
    indexed_revolve_scope.byte_offset = indexed_revolve_start as u64;
    indexed_revolve_scope.class_tag = "407".into();
    indexed_revolve_scope.paired_class_tag = "258".into();
    indexed_revolve_scope.frame_length = 377;
    indexed_revolve_scope.reference_members = vec![200, 201, 202, 203, 204, 205, 1_790, 1_791];
    let indexed_angle = DesignParameterOwner {
        id: indexed_revolve_scope.id.clone(),
        byte_offset: 0,
        class_tag: "372".into(),
        record_index: indexed_angle_record_index,
        scope_record_index: indexed_revolve_scope.record_index,
        local_ordinal: 0,
        evaluated_value: std::f64::consts::TAU,
        evaluated_value_offset: 45,
        parameter_record_index: 1_792,
        owned_ordinal: 8,
        variant: None,
        companion_record_index: 1_793,
    };
    let indexed_revolve_construction = exact_path_feature_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &indexed_revolve_scope,
        std::slice::from_ref(&indexed_angle),
    );
    assert_eq!(
        indexed_revolve_construction,
        Some(DesignPathFeatureConstruction::Revolve {
            operation: DesignExtrudeOperation::Cut,
            operation_offset: (indexed_revolve_start + 21) as u64,
            angle: std::f64::consts::TAU,
            angle_record_index: indexed_angle_record_index,
            angle_offset: 45,
            opposite_angle_record_index: None,
            opposite_angle_offset: None,
        })
    );
    let legacy_revolve_start = bytes.len();
    let legacy_angle_record_index = 1_800u32;
    let mut legacy_revolve = vec![0; 359];
    legacy_revolve[20] = 1;
    legacy_revolve[25..29].copy_from_slice(&4u32.to_le_bytes());
    legacy_revolve[29..33].copy_from_slice(&2u32.to_le_bytes());
    legacy_revolve[34..38].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&legacy_revolve);
    let mut legacy_revolve_scope = revolve_scope.clone();
    legacy_revolve_scope.byte_offset = legacy_revolve_start as u64;
    legacy_revolve_scope.class_tag = "409".into();
    legacy_revolve_scope.paired_class_tag = "257".into();
    legacy_revolve_scope.frame_length = 359;
    legacy_revolve_scope.reference_members =
        vec![200, 201, 202, 203, legacy_angle_record_index, 204];
    let legacy_angle = DesignParameterOwner {
        id: legacy_revolve_scope.id.clone(),
        byte_offset: 0,
        class_tag: "372".into(),
        record_index: legacy_angle_record_index,
        scope_record_index: legacy_revolve_scope.record_index,
        local_ordinal: 0,
        evaluated_value: std::f64::consts::TAU,
        evaluated_value_offset: 55,
        parameter_record_index: 1_801,
        owned_ordinal: 8,
        variant: None,
        companion_record_index: 1_802,
    };
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &legacy_revolve_scope,
            std::slice::from_ref(&legacy_angle),
        ),
        Some(DesignPathFeatureConstruction::Revolve {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: (legacy_revolve_start + 25) as u64,
            angle: std::f64::consts::TAU,
            angle_record_index: legacy_angle_record_index,
            angle_offset: 55,
            opposite_angle_record_index: None,
            opposite_angle_offset: None,
        })
    );
    revolve_scope.id = "stream:scope".into();
    revolve_scope.path_feature_construction = revolve_construction;
    let mut revolve_profile = thicken_group.clone();
    revolve_profile.id = "stream:profile".into();
    revolve_profile.scope_record_index = revolve_scope.record_index;
    revolve_profile.role = 0x0000_0041_0000_0000;
    let mut revolve_axis = revolve_profile.clone();
    revolve_axis.id = "stream:axis".into();
    revolve_axis.role = 0x0000_0021_0000_0000;
    assert_eq!(
        crate::design::feature_project::project_fixed_revolve_with_entities(
            &revolve_scope,
            &[revolve_profile, revolve_axis],
            &[],
            &[],
            &[],
            &[],
        ),
        None
    );

    indexed_revolve_scope.id = "stream:indexed-revolve".into();
    indexed_revolve_scope.path_feature_construction = indexed_revolve_construction;
    let mut indexed_profile = thicken_group.clone();
    indexed_profile.id = "stream:indexed-profile".into();
    indexed_profile.scope_record_index = indexed_revolve_scope.record_index;
    indexed_profile.role = 0x0000_0041_0000_0000;
    let mut indexed_axis = indexed_profile.clone();
    indexed_axis.id = "stream:indexed-axis".into();
    indexed_axis.record_index = 899;
    indexed_axis.members = vec![900];
    indexed_axis.role = 0x0000_0021_0000_0000;
    let mut indexed_bodies = indexed_profile.clone();
    indexed_bodies.id = "stream:indexed-bodies".into();
    indexed_bodies.record_index = 901;
    indexed_bodies.role = 0x0000_0004_0000_0000;
    let mut axis_selection = crate::records::DesignEntitySelectionOperand {
        id: "stream:indexed-axis-selection".into(),
        scope_record_index: indexed_revolve_scope.record_index,
        group_record_index: indexed_axis.record_index,
        group_member_ordinal: 0,
        record_index: 900,
        byte_offset: 0,
        class_tag: "377".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        identity_record_index: 902,
        identity_record_offset: 0,
        primary_identity: 100,
        primary_identity_offset: 0,
        secondary_identity: Some(104),
        secondary_identity_offset: Some(0),
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: 903,
        next_byte_offset: 0,
    };
    let axis_placement = DesignSketchPlacement {
        member_run_head: false,
        id: "stream:indexed-axis-placement".into(),
        scope_record_index: Some(10),
        entity_id: "Sketch_100".into(),
        entity_suffix: 100,
        byte_offset: 0,
        class_tag: "305".into(),
        record_index: 904,
        frame_length: 201,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: Some(0),
        paired_class_tag: "258".into(),
        paired_byte_offset: 0,
    };
    let axis_curve = SketchCurveIdentity {
        id: "stream:indexed-axis-curve".into(),
        record_index: 905,
        owner_reference: Some(100),
        class_tag: "450".into(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: 104,
        secondary_id: 0,
        geometry: Some(SketchCurveGeometry::Line {
            start: Point3::new(1.0, 2.0, 3.0),
            end: Point3::new(1.0, -3.0, 3.0),
            direction: Vector3::new(0.0, -1.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }),
    };
    let projected = crate::design::feature_project::project_fixed_revolve_with_entities(
        &indexed_revolve_scope,
        &[
            indexed_profile.clone(),
            indexed_axis.clone(),
            indexed_bodies.clone(),
        ],
        &[],
        std::slice::from_ref(&axis_selection),
        &[axis_placement],
        &[axis_curve],
    );
    assert!(matches!(
        projected,
        Some(FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                axis: Some(cadmpeg_ir::features::RevolutionAxis { origin, direction }),
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
        }) if origin == Point3::new(1.0, 2.0, 3.0)
            && direction == Vector3::new(0.0, -1.0, 0.0)
    ));
    axis_selection.secondary_identity = None;
    axis_selection.historical_face_candidates =
        vec![crate::records::DesignEntitySelectionFaceCandidate {
            history_id: "history".into(),
            historical_entity_kind: crate::records::AsmHistoricalEntityKind::Face,
            historical_entity_ref: 40,
            historical_state_ids: vec![1],
            face_slot: 40,
        }];
    let historical_definition =
        crate::design::feature_project::project_fixed_revolve_with_entities(
            &indexed_revolve_scope,
            &[
                indexed_profile.clone(),
                indexed_axis.clone(),
                indexed_bodies,
            ],
            &[],
            std::slice::from_ref(&axis_selection),
            &[],
            &[],
        )
        .unwrap();
    assert!(matches!(
        historical_definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction { axis: None, .. },
            ..
        }
    ));
    let mut feature = cadmpeg_ir::features::Feature {
        id: crate::ids::neutral_feature_id(&indexed_revolve_scope),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: historical_definition,
        native_ref: Some(indexed_revolve_scope.id.clone()),
    };
    let surface_id = cadmpeg_ir::ids::SurfaceId("surface:53".into());
    crate::design::feature_project::bind_revolve_face_axes(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&indexed_revolve_scope),
        &[indexed_profile, indexed_axis],
        std::slice::from_ref(&axis_selection),
        &[cadmpeg_ir::topology::Face {
            id: cadmpeg_ir::ids::FaceId("f3d:brep:entity#40".into()),
            shell: cadmpeg_ir::ids::ShellId("shell:1".into()),
            surface: surface_id.clone(),
            sense: cadmpeg_ir::topology::Sense::Forward,
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        }],
        &[cadmpeg_ir::geometry::Surface {
            id: surface_id,
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Plane {
                origin: Point3::new(4.0, 5.0, 6.0),
                normal: Vector3::new(0.0, 0.0, -2.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        }],
    );
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                axis: Some(cadmpeg_ir::features::RevolutionAxis { origin, direction }),
                ..
            },
            ..
        } if origin == Point3::new(4.0, 5.0, 6.0)
            && direction == Vector3::new(0.0, 0.0, -1.0)
    ));

    let loft_start = bytes.len();
    let mut loft = vec![0; 376];
    loft[29..33].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&loft);
    let mut loft_scope = scope.clone();
    loft_scope.byte_offset = loft_start as u64;
    loft_scope.kind = "Loft".into();
    loft_scope.frame_length = 376;
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &loft_scope,
            &[],
        ),
        Some(DesignPathFeatureConstruction::Loft {
            operation: DesignExtrudeOperation::Join,
            operation_offset: (loft_start + 29) as u64,
        })
    );
    loft_scope.id = "stream:loft-scope".into();
    loft_scope.path_feature_construction = Some(DesignPathFeatureConstruction::Loft {
        operation: DesignExtrudeOperation::NewBody,
        operation_offset: (loft_start + 29) as u64,
    });
    let loft_group = |ordinal: u32, role: u64| {
        let mut group = thicken_group.clone();
        group.id = format!("stream:loft-group-{ordinal}");
        group.scope_record_index = loft_scope.record_index;
        group.scope_reference_ordinal = ordinal;
        group.role = role;
        group
    };
    let role_41 = [loft_group(0, 0x41_0000_0000), loft_group(1, 0x41_0000_0000)];
    assert!(matches!(
        crate::design::feature_project::project_fixed_loft(&loft_scope, &role_41, &[], &[], &[]),
        Some(cadmpeg_ir::features::FeatureDefinition::Loft { sections, guides, .. })
            if sections.len() == 2 && guides.is_empty()
    ));
    let guided_role_41 = [
        loft_group(0, 0x41_0000_0000),
        loft_group(1, 0x41_0000_0000),
        loft_group(2, 0x41_0000_0000),
        loft_group(3, 0x5_0000_0000),
    ];
    assert!(matches!(
        crate::design::feature_project::project_fixed_loft(
            &loft_scope,
            &guided_role_41,
            &[],
            &[],
            &[],
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Loft { sections, guides, .. })
            if sections.len() == 3 && guides.len() == 1
    ));
    let role_shape = |groups: &[DesignConstructionOperandGroup]| {
        groups
            .iter()
            .map(|group| (group.role, group.members.len()))
            .collect::<Vec<_>>()
    };
    assert!(crate::validate::loft_operand_roles_are_valid(
        DesignExtrudeOperation::NewBody,
        &role_shape(&guided_role_41),
    ));
    loft_scope.path_feature_construction = Some(DesignPathFeatureConstruction::Loft {
        operation: DesignExtrudeOperation::Cut,
        operation_offset: (loft_start + 29) as u64,
    });
    let cut = [
        loft_group(0, 0x4_0000_0000),
        loft_group(1, 0x41_0000_0000),
        loft_group(2, 0x43_0000_0000),
    ];
    assert!(matches!(
        crate::design::feature_project::project_fixed_loft(&loft_scope, &cut, &[], &[], &[]),
        Some(cadmpeg_ir::features::FeatureDefinition::Loft {
            sections,
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }) if sections.len() == 2
    ));
    assert!(crate::validate::loft_operand_roles_are_valid(
        DesignExtrudeOperation::Cut,
        &role_shape(&cut),
    ));
    loft_scope.path_feature_construction = Some(DesignPathFeatureConstruction::Loft {
        operation: DesignExtrudeOperation::NewBody,
        operation_offset: (loft_start + 29) as u64,
    });
    let role_5 = [
        loft_group(0, 0x5_0000_0000),
        loft_group(1, 0x5_0000_0000),
        loft_group(2, 0x5_0000_0000),
    ];
    assert!(matches!(
        crate::design::feature_project::project_fixed_loft(&loft_scope, &role_5, &[], &[], &[]),
        Some(cadmpeg_ir::features::FeatureDefinition::Loft { sections, guides, .. })
            if sections.len() == 3 && guides.is_empty()
    ));
    let centered = [
        loft_group(0, 0x43_0000_0000),
        loft_group(1, 0x43_0000_0000),
        loft_group(2, 0x7_0000_0000),
    ];
    assert!(matches!(
        crate::design::feature_project::project_fixed_loft(&loft_scope, &centered, &[], &[], &[]),
        Some(cadmpeg_ir::features::FeatureDefinition::Loft {
            sections,
            guides,
            centerline: Some(cadmpeg_ir::features::PathRef::Native(centerline)),
            ..
        }) if sections.len() == 2 && guides.is_empty() && centerline == "stream:loft-group-2"
    ));
    let mixed = [
        loft_group(0, 0x43_0000_0000),
        loft_group(1, 0x43_0000_0000),
        loft_group(2, 0x5_0000_0000),
        loft_group(3, 0x7_0000_0000),
    ];
    assert_eq!(
        crate::design::feature_project::project_fixed_loft(&loft_scope, &mixed, &[], &[], &[]),
        None
    );
    assert!(!crate::validate::loft_operand_roles_are_valid(
        DesignExtrudeOperation::NewBody,
        &role_shape(&mixed),
    ));
    let mut point = loft_group(0, 0x5_0000_0000);
    point.members = vec![10];
    let profile = loft_group(1, 0x43_0000_0000);
    let mut boundary = loft_group(2, 0x5_0000_0000);
    boundary.members = vec![20, 21, 22];
    assert!(matches!(
        crate::design::feature_project::project_fixed_loft(
            &loft_scope,
            &[point.clone(), profile.clone(), boundary.clone()],
            &[],
            &[],
            &[],
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Loft {
            sections,
            guides,
            centerline: None,
            ..
        }) if matches!(sections.as_slice(), [
            cadmpeg_ir::features::LoftSection::Point(
                cadmpeg_ir::features::LoftPointSection::Native(_)
            ),
            cadmpeg_ir::features::LoftSection::Profile(_),
            cadmpeg_ir::features::LoftSection::Profile(_),
        ]) && guides.is_empty()
    ));
    assert!(crate::validate::loft_operand_roles_are_valid(
        DesignExtrudeOperation::NewBody,
        &role_shape(&[point, profile, boundary]),
    ));

    let sweep_start = bytes.len();
    let mut sweep = vec![0; 499];
    sweep[25..29].copy_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&sweep);
    let sweep_values: [f64; 6] = [0.8, 0.0, 1.0, 1.0, 6.632_251_157_578_453, 0.0];
    let sweep_scalar_start = bytes.len();
    for (ordinal, value) in sweep_values.into_iter().enumerate() {
        let record_index = 80 + ordinal as u32;
        let mut scalar = vec![0; 100];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal as u8;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut sweep_scope = scope.clone();
    sweep_scope.byte_offset = sweep_start as u64;
    sweep_scope.kind = "Sweep".into();
    sweep_scope.frame_length = 499;
    sweep_scope.reference_members = (80..86).collect();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &sweep_scope,
            &[],
        ),
        Some(DesignPathFeatureConstruction::Sweep {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: (sweep_start + 25) as u64,
            values: sweep_values,
            record_indexes: [80, 81, 82, 83, 84, 85],
            value_offsets: std::array::from_fn(|ordinal| {
                (sweep_scalar_start + ordinal * 111 + 40) as u64
            }),
        })
    );
    sweep_scope.id = "stream:sweep-scope".into();
    sweep_scope.path_feature_construction = exact_path_feature_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &sweep_scope,
        &[],
    );
    let sweep_group = |ordinal: u32, role: u64| {
        let mut group = thicken_group.clone();
        group.id = format!("stream:sweep-group-{ordinal}");
        group.scope_record_index = sweep_scope.record_index;
        group.scope_reference_ordinal = ordinal;
        group.role = role;
        group
    };
    let profile = sweep_group(0, 0x41_0000_0000);
    let path = sweep_group(1, 0x5_0000_0000);
    let body = sweep_group(2, 0x4_0000_0000);
    assert!(matches!(
        crate::design::feature_project::project_fixed_sweep(
            &sweep_scope,
            &[profile.clone(), path.clone()],
            &[],
            &[],
            &[],
            &[],
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Sweep {
            path_extent: Some(cadmpeg_ir::features::SweepPathExtent {
                along_fraction: 0.8,
                against_fraction: 0.0,
            }),
            twist: Some(cadmpeg_ir::features::Angle(6.632_251_157_578_453)),
            taper: None,
            ..
        })
    ));
    let rail = sweep_group(2, 0x5_0000_0000);
    sweep_scope.path_feature_construction = Some(DesignPathFeatureConstruction::Sweep {
        operation: DesignExtrudeOperation::NewBody,
        operation_offset: (sweep_start + 25) as u64,
        values: [0.0, 1.0, 0.0, 1.0, 0.0, 0.0],
        record_indexes: [80, 81, 82, 83, 84, 85],
        value_offsets: std::array::from_fn(|ordinal| {
            (sweep_scalar_start + ordinal * 111 + 40) as u64
        }),
    });
    assert!(matches!(
        crate::design::feature_project::project_fixed_sweep(
            &sweep_scope,
            &[profile.clone(), path.clone(), rail],
            &[],
            &[],
            &[],
            &[],
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Sweep {
            path: Some(cadmpeg_ir::features::PathRef::Native(path)),
            path_extent: Some(cadmpeg_ir::features::SweepPathExtent {
                along_fraction: 0.0,
                against_fraction: 1.0,
            }),
            guide_rail: Some(cadmpeg_ir::features::SweepGuideRail {
                path: cadmpeg_ir::features::PathRef::Native(rail),
                extent: cadmpeg_ir::features::SweepPathExtent {
                    along_fraction: 0.0,
                    against_fraction: 1.0,
                },
            }),
            ..
        }) if path == "stream:sweep-group-1" && rail == "stream:sweep-group-2"
    ));
    let complete_sweep_values = [1.0, 1.0, 1.0, 1.0, sweep_values[4], 0.0];
    sweep_scope.path_feature_construction = Some(DesignPathFeatureConstruction::Sweep {
        operation: DesignExtrudeOperation::NewBody,
        operation_offset: (sweep_start + 25) as u64,
        values: complete_sweep_values,
        record_indexes: [80, 81, 82, 83, 84, 85],
        value_offsets: std::array::from_fn(|ordinal| {
            (sweep_scalar_start + ordinal * 111 + 40) as u64
        }),
    });
    assert!(matches!(
        crate::design::feature_project::project_fixed_sweep(
            &sweep_scope,
            &[profile.clone(), path.clone()],
            &[],
            &[],
            &[],
            &[],
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Sweep {
            mode: cadmpeg_ir::features::SweepMode::Unresolved,
            ..
        })
    ));
    assert_eq!(
        crate::design::feature_project::project_fixed_sweep(
            &sweep_scope,
            &[profile.clone(), path.clone(), body.clone()],
            &[],
            &[],
            &[],
            &[],
        ),
        None
    );
    sweep_scope.sweep_profile = Some(crate::records::DesignSketchProfileOperand {
        scope_reference_ordinal: 3,
        record_index: 2795,
        byte_offset: 32_000,
        class_tag: "312".into(),
        asset_id: "asset".into(),
        asset_id_offset: 32_040,
        entity_id: "0_2718".into(),
        entity_suffix: 2718,
        entity_reference_offset: 32_080,
        paired_class_tag: "258".into(),
        paired_byte_offset: 32_180,
    });
    let mut selected_profile = profile.clone();
    selected_profile.members = vec![2788];
    let mut profile_carrier = profile.clone();
    profile_carrier.id = "stream:sweep-profile-carrier".into();
    profile_carrier.scope_reference_ordinal = 3;
    profile_carrier.members = vec![2795];
    let mut guide_surface = sweep_group(4, 0x11_0000_0000);
    guide_surface.id = "stream:sweep-guide-surface".into();
    let entity_selection = crate::records::DesignEntitySelectionOperand {
        id: "stream:sweep-profile-selection".into(),
        scope_record_index: sweep_scope.record_index,
        group_record_index: selected_profile.record_index,
        group_member_ordinal: 0,
        record_index: 2788,
        byte_offset: 31_000,
        class_tag: "310".into(),
        asset_id: "asset".into(),
        asset_id_offset: 31_040,
        context_id: "context".into(),
        context_id_offset: 31_080,
        identity_record_index: 2791,
        identity_record_offset: 31_180,
        primary_identity: 2718,
        primary_identity_offset: 31_200,
        secondary_identity: Some(164),
        secondary_identity_offset: Some(31_208),
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: profile_carrier.record_index,
        next_byte_offset: profile_carrier.byte_offset,
    };
    assert!(matches!(
        crate::design::feature_project::project_fixed_sweep(
            &sweep_scope,
            &[selected_profile, profile_carrier, path.clone(), guide_surface],
            &[],
            &[],
            &[entity_selection],
            &[],
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(
                cadmpeg_ir::features::ProfileRef::Native(profile)
            ),
            orientation: Some(cadmpeg_ir::features::SweepOrientation::GuideSurface {
                faces: cadmpeg_ir::features::FaceSelection::Native(faces),
            }),
            guide_rail: None,
            ..
        }) if profile == "stream:sweep-group-0" && faces == "stream:sweep-guide-surface"
    ));
    sweep_scope.sweep_profile = None;
    sweep_scope.path_feature_construction = Some(DesignPathFeatureConstruction::Sweep {
        operation: DesignExtrudeOperation::Cut,
        operation_offset: (sweep_start + 25) as u64,
        values: complete_sweep_values,
        record_indexes: [80, 81, 82, 83, 84, 85],
        value_offsets: std::array::from_fn(|ordinal| {
            (sweep_scalar_start + ordinal * 111 + 40) as u64
        }),
    });
    assert!(matches!(
        crate::design::feature_project::project_fixed_sweep(
            &sweep_scope,
            &[profile, path, body],
            &[],
            &[],
            &[],
            &[],
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Sweep {
            mode: cadmpeg_ir::features::SweepMode::Solid {
                op: cadmpeg_ir::features::BooleanOp::Cut
            },
            ..
        })
    ));

    let pipe_start = bytes.len();
    let mut pipe = vec![0; 464];
    pipe[25..29].copy_from_slice(&4u32.to_le_bytes());
    pipe[29] = 1;
    pipe[30] = 1;
    bytes.extend_from_slice(&pipe);
    let pipe_values: [f64; 4] = [1.0, 1.0, 0.6, 0.15];
    let pipe_scalar_start = bytes.len();
    for (ordinal, value) in pipe_values.into_iter().enumerate() {
        let record_index = 170 + ordinal as u32;
        let mut scalar = vec![0; 100];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal as u8;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut pipe_scope = scope.clone();
    pipe_scope.byte_offset = pipe_start as u64;
    pipe_scope.kind = "Pipe".into();
    pipe_scope.frame_length = 464;
    pipe_scope.reference_members = (170..174).collect();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &pipe_scope,
            &[],
        ),
        Some(DesignPathFeatureConstruction::Pipe {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: (pipe_start + 25) as u64,
            section_shape: 1,
            section_shape_offset: (pipe_start + 29) as u64,
            filled: true,
            filled_offset: (pipe_start + 30) as u64,
            values: pipe_values,
            record_indexes: [170, 171, 172, 173],
            value_offsets: std::array::from_fn(|ordinal| {
                (pipe_scalar_start + ordinal * 111 + 40) as u64
            }),
        })
    );

    let mut companion = DesignParameterCompanion {
        id: "f3d:native:parameter-companion#11".into(),
        byte_offset: 0,
        class_tag: "300".into(),
        record_index: 11,
        owner_record_index: 10,
        timestamp_micros: 1,
        timestamp_micros_offset: 42,
        payload_byte_offset: 58,
        payload_byte_length: 0,
        owned_recipe_ids: Vec::new(),
    };
    scope.id = "f3d:native:parameter-scope#12".into();
    scope.byte_offset = 58;
    assert_eq!(
        companion_owned_interval(
            &companion,
            std::iter::empty(),
            &[],
            &[scope.clone()],
            &[],
            100,
        ),
        Some((58, 58))
    );
    scope.byte_offset = 80;
    assert_eq!(
        companion_owned_interval(
            &companion,
            std::iter::empty(),
            &[],
            &[scope.clone()],
            &[],
            100,
        ),
        Some((58, 80))
    );
    scope.byte_offset = 90;
    let foreign_header = DesignRecordHeader {
        id: "f3d:native:record-header#55".into(),
        record_index: 55,
        class_tag: "301".into(),
        byte_offset: 70,
    };
    assert_eq!(
        companion_owned_interval(
            &companion,
            std::iter::empty(),
            &[],
            &[scope.clone()],
            &[foreign_header],
            100,
        ),
        Some((58, 70))
    );

    let mut parameter = parse_design_parameter(&parameter_record(
        None,
        "1",
        "User Parameter",
        None,
        "p",
        1.0,
    ))
    .expect("generated parameter");
    parameter.id = "f3d:native:design-parameter#65".into();
    parameter.byte_offset = 65;
    assert_eq!(
        companion_owned_interval(&companion, std::iter::once(&parameter), &[], &[], &[], 100,),
        Some((58, 65))
    );
    let recipe = ConstructionRecipe {
        id: "f3d:native:construction-recipe#60".into(),
        byte_offset: 60,
        record_index_offset: None,
        kind: ConstructionRecipeKind::Edge,
        design_id: None,
        design_id_offset: None,
        design_selector: None,
        recipe_index: 0,
        record_index: 303,
    };
    bind_parameter_companion_payloads(
        std::slice::from_mut(&mut companion),
        std::slice::from_ref(&parameter),
        &[],
        &[],
        &[],
        &[],
        std::slice::from_ref(&recipe),
        &HashMap::from([("f3d:native".into(), 100)]),
    );
    assert_eq!(companion.payload_byte_offset, 58);
    assert_eq!(companion.payload_byte_length, 7);
    assert_eq!(companion.owned_recipe_ids, [recipe.id]);

    companion.payload_byte_length = 0;
    companion.owned_recipe_ids.clear();
    scope.entity_id = Some("Sketch_99".into());
    scope.entity_suffix = Some(99);
    let entity = crate::records::DesignEntityHeader {
        id: "f3d:native:design-entity-header#70".into(),
        byte_offset: 70,
        entity_suffix: 99,
        entity_id: "Sketch_99".into(),
        class_tag: "366".into(),
        optional_slot_present: false,
        module: Some("MSketch".into()),
        record_reference: None,
        record_reference_offset: None,
        declared_reference_count: None,
        reference_indices: Vec::new(),
        reference_offsets: Vec::new(),
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
    };
    bind_parameter_companion_payloads(
        std::slice::from_mut(&mut companion),
        &[],
        &[],
        std::slice::from_ref(&scope),
        std::slice::from_ref(&entity),
        &[],
        &[],
        &HashMap::from([("f3d:native".into(), 100)]),
    );
    assert_eq!(companion.payload_byte_offset, 58);
    assert_eq!(companion.payload_byte_length, 12);
}

#[test]
fn named_solid_primitives_bind_ordered_parameter_owners() {
    fn owner(
        scope_record_index: u32,
        record_index: u32,
        local_ordinal: u32,
        value: f64,
    ) -> DesignParameterOwner {
        DesignParameterOwner {
            id: format!("f3d:Design/BulkStream.dat:owner#{record_index}"),
            byte_offset: u64::from(record_index),
            class_tag: "272".into(),
            record_index,
            scope_record_index,
            local_ordinal,
            evaluated_value: value,
            evaluated_value_offset: u64::from(record_index) + 100,
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: None,
            companion_record_index: record_index + 2,
        }
    }

    let mut bytes = vec![0; 100];
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24] = 0;
    bytes[25] = 1;
    let mut box_scope =
        DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#12", "BoxPrimitive", 12);
    box_scope.frame_length = bytes.len() as u64;
    box_scope.reference_members = vec![20, 21, 22, 23, 24];
    let box_owners = vec![
        owner(12, 20, 0, 3.0),
        owner(12, 21, 1, 4.0),
        owner(12, 22, 2, 2.0),
        owner(12, 23, 3, 0.5),
        owner(12, 24, 4, -0.25),
    ];
    let records = IndexedRecordOffsets::build(&bytes);
    assert!(matches!(
        exact_solid_primitive(&bytes, &records, &box_scope, &box_owners),
        Some(DesignSolidPrimitive::Box {
            length: 3.0,
            width: 4.0,
            height: 2.0,
            offset_x: 0.5,
            offset_y: -0.25,
            operation: DesignExtrudeOperation::Join,
            operation_offset: 20,
            ..
        })
    ));

    bytes[20..24].copy_from_slice(&4u32.to_le_bytes());
    let mut cylinder_scope = box_scope;
    cylinder_scope.kind = "CylinderPrimitive".into();
    cylinder_scope.record_index = 13;
    cylinder_scope.reference_members = vec![30, 31];
    let cylinder_owners = vec![owner(13, 30, 0, 0.7), owner(13, 31, 1, 3.0)];
    assert!(matches!(
        exact_solid_primitive(&bytes, &records, &cylinder_scope, &cylinder_owners,),
        Some(DesignSolidPrimitive::Cylinder {
            height: 0.7,
            diameter: 3.0,
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 20,
            ..
        })
    ));
}

#[test]
fn combine_scope_projects_ordered_target_tools_and_retention() {
    fn indexed_header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }
    fn operation_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, b"283", record_index);
        bytes.extend_from_slice(&[0; 9]);
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(b"DcFeatureOperationIdFlag");
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&7u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, b"259", record_index);
    }
    fn target_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, b"283", record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, b"259", record_index);
    }
    fn selection_record(bytes: &mut Vec<u8>, record_index: u32, suffix: u8) {
        indexed_header(bytes, b"389", record_index);
        lp_utf16(
            bytes,
            &format!("00000000-0000-0000-0000-0000000000{suffix:02x}"),
        );
        lp_utf16(
            bytes,
            &format!("10000000-0000-0000-0000-0000000000{suffix:02x}"),
        );
        indexed_header(bytes, b"306", record_index);
    }

    let scope_record_index = 90;
    let references = [91u32, 92, 93, 94, 95, 96];
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, b"382", scope_record_index);
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0);
    bytes.push(1);
    bytes.extend_from_slice(&[0; 7]);
    bytes.resize(64, 0);
    bytes.extend_from_slice(&(references.len() as u32).to_le_bytes());
    for reference in references {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&17u32.to_le_bytes());
    lp_utf16(&mut bytes, "Combine");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&2u32.to_le_bytes());
    tail[31..35].copy_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    indexed_header(&mut bytes, b"259", scope_record_index);
    for (ordinal, pair) in references.chunks_exact(2).enumerate() {
        if ordinal == 2 {
            target_record(&mut bytes, pair[0], pair[1]);
        } else {
            operation_record(&mut bytes, pair[0], pair[1]);
        }
        selection_record(&mut bytes, pair[1], u8::try_from(pair[1]).unwrap());
    }

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: scope_record_index,
        class_tag: "382".into(),
        byte_offset: 0,
    };
    let mut scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("Combine scope");
    let operation = exact_combine_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
        .expect("Combine construction");
    assert_eq!(
        operation,
        DesignCombineOperation {
            operation: DesignExtrudeOperation::Join,
            operation_offset: 20,
            keep_tools: true,
            keep_tools_offset: 25,
            body_selection_record_indexes: vec![96, 92, 94],
        }
    );
    scope.combine_operation = Some(operation);
    assert_eq!(
        project_combine(&scope, "Design1/BulkStream.dat"),
        Some(cadmpeg_ir::features::FeatureDefinition::Combine {
            target: cadmpeg_ir::features::BodySelection::Native(
                "Design1/BulkStream.dat:design-record#96".into(),
            ),
            tools: cadmpeg_ir::features::BodySelection::NativeSet(vec![
                "Design1/BulkStream.dat:design-record#92".into(),
                "Design1/BulkStream.dat:design-record#94".into(),
            ]),
            op: cadmpeg_ir::features::BooleanOp::Join,
            keep_tools: true,
        })
    );

    let mut compact_bytes = bytes.clone();
    compact_bytes[4..7].copy_from_slice(b"387");
    compact_bytes[11..21].fill(0);
    compact_bytes[21..25].copy_from_slice(&1u32.to_le_bytes());
    compact_bytes[25] = 0;
    compact_bytes[26..29].fill(0);
    compact_bytes[29..31].copy_from_slice(&[1, 0]);
    compact_bytes[31..35].copy_from_slice(&1u32.to_le_bytes());
    compact_bytes[35] = 1;
    compact_bytes[36..44].copy_from_slice(&200u64.to_le_bytes());
    compact_bytes[44] = 0;
    let mut compact_scope = scope.clone();
    compact_scope.class_tag = "387".into();
    compact_scope.paired_class_tag = "258".into();
    compact_scope.frame_length = 328;
    let compact = exact_combine_operation(
        &compact_bytes,
        &IndexedRecordOffsets::build(&compact_bytes),
        &compact_scope,
    )
    .expect("compact Combine construction");
    assert_eq!(compact.operation, DesignExtrudeOperation::Join);
    assert_eq!(compact.operation_offset, 21);
    assert!(!compact.keep_tools);
    assert_eq!(compact.body_selection_record_indexes, [96, 92, 94]);
}

#[test]
fn thread_scope_decodes_standard_size_and_face_group() {
    let mut bytes = vec![0; 148];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"335");
    bytes[7..11].copy_from_slice(&987u32.to_le_bytes());
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[1, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x67, 0]);
    let mut payload = Vec::new();
    lp_utf16(&mut payload, "M30x3.5");
    lp_utf16(&mut payload, "30.0");
    lp_utf16(&mut payload, "ISO Metric profile");
    assert_eq!(payload.len(), 70);
    bytes[38..108].copy_from_slice(&payload);
    bytes[108..113].copy_from_slice(&[0, 1, 0, 0, 0]);
    bytes[113..121].copy_from_slice(&2.97345f64.to_le_bytes());
    bytes[121..129].copy_from_slice(&2.5732f64.to_le_bytes());
    bytes[129] = 1;
    bytes[130..138].copy_from_slice(&0.35f64.to_le_bytes());
    bytes[138..146].copy_from_slice(&2.7568f64.to_le_bytes());
    bytes[146..148].copy_from_slice(&[0, 1]);

    assert_eq!(
        parse_thread_payload(&bytes, 0, 988),
        Some(DesignThreadConstruction {
            designation: "M30x3.5".into(),
            nominal_size: 30.0,
            profile: "ISO Metric profile".into(),
            major_diameter: 2.97345,
            minor_diameter: 2.5732,
            pitch: 0.35,
            pitch_diameter: 2.7568,
            face_group_record_index: 988,
        })
    );
}

#[test]
fn localized_sketch_scope_retains_its_generic_reference_table() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"301");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for record_index in [55u32, 56] {
        bytes.push(1);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Esquisse");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "301".into(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("localized Sketch scope");
    assert_eq!(scope.kind, "Esquisse");
    assert_eq!(scope.reference_members, [55, 56]);
    assert!(scope.entity_id.is_none());
}

#[test]
fn extrude_scope_discriminators_follow_optional_indexed_reference() {
    let scope = |kind: &str,
                 operation: u32,
                 extent: (u32, u32),
                 direction_reversed: u8,
                 structural_constant: u8,
                 start: u8,
                 reference_padding: Option<usize>,
                 legacy_side_extents: Option<((u32, u32), bool)>,
                 legacy_reference_count_offset: Option<usize>| {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"301");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.resize(120, 0);
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        let operation_offset = if legacy_side_extents.is_some() {
            27
        } else if let Some(reference_padding) = reference_padding {
            bytes[25] = 1;
            bytes[26..30].copy_from_slice(&77u32.to_le_bytes());
            30 + reference_padding
        } else {
            28
        };
        bytes[operation_offset..operation_offset + 4].copy_from_slice(&operation.to_le_bytes());
        bytes[operation_offset + 4..operation_offset + 8].copy_from_slice(&extent.0.to_le_bytes());
        bytes[operation_offset + 8..operation_offset + 12].copy_from_slice(&extent.1.to_le_bytes());
        bytes[operation_offset + 12] = direction_reversed;
        bytes[operation_offset + 13] = structural_constant;
        bytes[operation_offset + 14] = start;
        if legacy_side_extents.is_some() {
            let reference_count_offset = legacy_reference_count_offset.unwrap_or_else(|| {
                if legacy_side_extents.is_some_and(|(_, widened)| widened) || extent.0 == 2 {
                    272
                } else {
                    252
                }
            });
            bytes.resize(reference_count_offset, 0);
        }
        if legacy_side_extents.is_some_and(|(_, widened)| widened)
            || legacy_side_extents.is_some() && extent.0 == 2
        {
            for reference_at in [139, 159, 182] {
                bytes[reference_at] = 1;
                bytes[reference_at + 1..reference_at + 5].copy_from_slice(&55u32.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&reference_padding.map_or(55, |_| 77_u32).to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        lp_utf16(&mut bytes, kind);
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"261");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        if let Some((side_extents, widened)) = legacy_side_extents {
            if widened && extent.0 != 2 {
                bytes[106..110].copy_from_slice(&1u32.to_le_bytes());
                bytes[110..114].copy_from_slice(&0u32.to_le_bytes());
            }
            let (first_extent_at, second_extent_at) = if legacy_reference_count_offset == Some(294)
            {
                (116, 129)
            } else if extent.0 == 2 {
                (155, 178)
            } else if widened {
                (116, if side_extents.0 == 2 { 268 } else { 130 })
            } else {
                (106, if side_extents.0 == 2 { 116 } else { 110 })
            };
            bytes[first_extent_at..first_extent_at + 4]
                .copy_from_slice(&side_extents.0.to_le_bytes());
            bytes[second_extent_at..second_extent_at + 4]
                .copy_from_slice(&side_extents.1.to_le_bytes());
        }
        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: 12,
            class_tag: "301".into(),
            byte_offset: 0,
        };
        parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header).unwrap()
    };

    let direct = scope("Extrude", 1, (1, 2), 0, 1, 0, None, None, None);
    assert_eq!(
        direct.extrude_prologue,
        Some(DesignExtrudePrologue::ReferenceAware {
            reference: None,
            operation: DesignExtrudeOperation::Join,
            operation_offset: 28,
            extent_discriminators: [1, 2],
            extent: DesignExtrudeExtent::OneSidedDistance,
            extent_discriminator_offsets: [32, 36],
            direction_reversed: false,
            direction_reversed_offset: 40,
            solid_operation: true,
            solid_operation_offset: 41,
            start: DesignExtrudeStart::ProfilePlane,
            start_offset: 42,
        })
    );
    let referenced = scope("Extrude", 3, (2, 0), 0, 1, 1, Some(8), None, None);
    assert_eq!(
        referenced.extrude_prologue,
        Some(DesignExtrudePrologue::ReferenceAware {
            reference: Some(crate::records::DesignExtrudePrologueReference {
                record_index: 77,
                record_index_offset: 26,
                trailing_zero_count: 8,
            }),
            operation: DesignExtrudeOperation::Intersect,
            operation_offset: 38,
            extent_discriminators: [2, 0],
            extent: DesignExtrudeExtent::TwoSidedDistance,
            extent_discriminator_offsets: [42, 46],
            direction_reversed: false,
            direction_reversed_offset: 50,
            solid_operation: true,
            solid_operation_offset: 51,
            start: DesignExtrudeStart::OffsetProfilePlane,
            start_offset: 52,
        })
    );
    let compact_reference = scope("Extrude", 2, (1, 2), 0, 1, 2, Some(7), None, None);
    let Some(DesignExtrudePrologue::ReferenceAware {
        reference: Some(reference),
        operation_offset,
        ..
    }) = compact_reference.extrude_prologue
    else {
        panic!("compact referenced Extrude prologue");
    };
    assert_eq!(reference.trailing_zero_count, 7);
    assert_eq!(operation_offset, 37);

    let to_face = scope("Extrusion", 2, (1, 1), 1, 1, 2, None, None, None);
    assert_eq!(to_face.kind, "Extrusion");
    let Some(prologue) = to_face.extrude_prologue else {
        panic!("to-face Extrude prologue");
    };
    assert_eq!(prologue.extent(), Some(DesignExtrudeExtent::OneSidedToFace));
    assert!(prologue.direction_reversed());
    assert_eq!(prologue.start(), DesignExtrudeStart::FromFace);

    let shifted_distance = scope(
        "Extrude",
        4,
        (1, 2),
        0,
        1,
        0,
        None,
        Some(((1, 0), false)),
        None,
    );
    assert_eq!(
        shifted_distance
            .extrude_prologue
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::OneSidedDistance)
    );
    let shifted_symmetric = scope(
        "Extrude",
        4,
        (3, 2),
        0,
        1,
        0,
        None,
        Some(((1, 0), true)),
        None,
    );
    assert_eq!(
        shifted_symmetric
            .extrude_prologue
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::SymmetricDistance)
    );
    assert!(matches!(
        shifted_symmetric.extrude_prologue,
        Some(DesignExtrudePrologue::LegacyShifted {
            side_extent_discriminator_offsets: [116, 130],
            ..
        })
    ));
    let shifted_two_sided = scope(
        "Extrude",
        2,
        (2, 0),
        0,
        1,
        0,
        None,
        Some(((1, 1), false)),
        None,
    );
    assert_eq!(
        shifted_two_sided
            .extrude_prologue
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::TwoSidedDistance)
    );

    let shifted_through_all = scope(
        "Extrude",
        2,
        (1, 0),
        1,
        1,
        0,
        None,
        Some(((4, 0), false)),
        None,
    );
    assert_eq!(
        shifted_through_all
            .extrude_prologue
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::OneSidedThroughAll)
    );
    let shifted_to_face = scope(
        "Extrude",
        2,
        (1, 1),
        1,
        1,
        0,
        None,
        Some(((2, 0), true)),
        None,
    );
    assert_eq!(
        shifted_to_face
            .extrude_prologue
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::OneSidedToFace)
    );
    assert!(matches!(
        shifted_to_face.extrude_prologue,
        Some(DesignExtrudePrologue::LegacyShifted {
            side_extent_discriminator_offsets: [116, 268],
            ..
        })
    ));

    for reference_count_offset in [262, 263] {
        let shifted_compact_to_face = scope(
            "Extrude",
            2,
            (1, 1),
            1,
            1,
            0,
            None,
            Some(((2, 0), false)),
            Some(reference_count_offset),
        );
        assert!(matches!(
            shifted_compact_to_face.extrude_prologue,
            Some(DesignExtrudePrologue::LegacyShifted {
                extent: Some(DesignExtrudeExtent::OneSidedToFace),
                side_extent_discriminator_offsets: [106, offset],
                ..
            }) if offset == u64::try_from(reference_count_offset - 4).unwrap()
        ));
    }

    let shifted_symmetric_through_all = scope(
        "Extrude",
        2,
        (3, 0),
        0,
        1,
        0,
        None,
        Some(((4, 4), true)),
        Some(294),
    );
    assert!(matches!(
        shifted_symmetric_through_all.extrude_prologue,
        Some(DesignExtrudePrologue::LegacyShifted {
            extent: Some(DesignExtrudeExtent::SymmetricThroughAll),
            side_extent_discriminator_offsets: [116, 129],
            ..
        })
    ));

    let invalid_absent_first_side = scope(
        "Extrude",
        2,
        (3, 0),
        0,
        1,
        0,
        None,
        Some(((0, 0), false)),
        None,
    );
    assert_eq!(invalid_absent_first_side.extrude_prologue, None);

    let unrecognized = scope("Extrude", 2, (3, 0), 0, 1, 0, None, None, None);
    assert_eq!(unrecognized.kind, "Extrude");
    assert_eq!(unrecognized.extrude_prologue, None);
    assert_eq!(
        scope(
            "Extrude",
            2,
            (3, 0),
            2,
            1,
            0,
            None,
            Some(((1, 0), false)),
            None,
        )
        .extrude_prologue,
        None
    );
    let sheet = scope(
        "Extrude",
        2,
        (3, 0),
        0,
        0,
        0,
        None,
        Some(((1, 0), false)),
        None,
    )
    .extrude_prologue
    .expect("sheet Extrude prologue");
    assert!(!sheet.solid_operation());
    assert_eq!(
        scope(
            "Extrude",
            2,
            (3, 0),
            0,
            1,
            3,
            None,
            Some(((1, 0), false)),
            None,
        )
        .extrude_prologue,
        None
    );
}

#[test]
fn legacy_distance_extrude_scope_decodes_nullable_prefix_forms() {
    let scope = |prefix_present: bool, operation: u32, geometry_kind: u32| {
        let reference_count_offset = if prefix_present { 212 } else { 208 };
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"376");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.resize(reference_count_offset, 0);
        let operation_offset = if prefix_present {
            bytes[20] = 1;
            bytes[21..25].copy_from_slice(&0u32.to_le_bytes());
            25
        } else {
            21
        };
        bytes[operation_offset..operation_offset + 4].copy_from_slice(&operation.to_le_bytes());
        bytes[operation_offset + 4..operation_offset + 8].copy_from_slice(&2u32.to_le_bytes());
        bytes[operation_offset + 8] = 1;
        bytes[operation_offset + 9..operation_offset + 13]
            .copy_from_slice(&geometry_kind.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&55u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        lp_utf16(&mut bytes, "Extrude");
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"261");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: 12,
            class_tag: "376".into(),
            byte_offset: 0,
        };
        parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header).unwrap()
    };

    assert_eq!(
        scope(false, 1, 1).extrude_prologue,
        Some(DesignExtrudePrologue::LegacyDistance {
            prefix_value: None,
            prefix_value_offset: None,
            operation: DesignExtrudeOperation::Join,
            operation_offset: 21,
            extent_discriminator: 2,
            extent_discriminator_offset: 25,
            direction_reversed: true,
            direction_reversed_offset: 29,
            geometry_kind: 1,
            geometry_kind_offset: 30,
        })
    );
    assert_eq!(
        scope(true, 4, 0).extrude_prologue,
        Some(DesignExtrudePrologue::LegacyDistance {
            prefix_value: Some(0),
            prefix_value_offset: Some(21),
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 25,
            extent_discriminator: 2,
            extent_discriminator_offset: 29,
            direction_reversed: true,
            direction_reversed_offset: 33,
            geometry_kind: 0,
            geometry_kind_offset: 34,
        })
    );
}

#[test]
fn coil_scope_discriminators_use_the_fixed_scope_prologue() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"301");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.resize(120, 0);
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24] = 1;
    bytes[26..30].copy_from_slice(&2u32.to_le_bytes());
    bytes[30..34].copy_from_slice(&3u32.to_le_bytes());
    bytes[92..96].copy_from_slice(&2u32.to_le_bytes());
    bytes[107..111].copy_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "SpirePrimitive");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "301".into(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("Coil scope");
    assert_eq!(scope.coil_operation, Some(DesignExtrudeOperation::Cut));
    assert_eq!(scope.coil_operation_offset, Some(20));
    assert_eq!(scope.coil_extent, Some(DesignCoilExtent::HeightPitch));
    assert_eq!(scope.coil_extent_offset, Some(30));
    assert_eq!(
        scope.coil_section,
        Some(DesignCoilSection::ExternalTriangle)
    );
    assert_eq!(scope.coil_section_offset, Some(92));
    assert_eq!(
        scope.coil_section_placement,
        Some(DesignCoilSectionPlacement::Inside)
    );
    assert_eq!(scope.coil_section_placement_offset, Some(107));
    assert_eq!(scope.coil_clockwise, Some(true));
    assert_eq!(scope.coil_clockwise_offset, Some(24));
}

#[test]
fn compact_coil_scope_uses_its_own_closed_discriminators() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"353");
    bytes.extend_from_slice(&6644u32.to_le_bytes());
    bytes.resize(120, 0);
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24] = 0;
    bytes[26..30].copy_from_slice(&4u32.to_le_bytes());
    bytes[30..34].copy_from_slice(&1u32.to_le_bytes());
    bytes[92..96].copy_from_slice(&1u32.to_le_bytes());
    bytes[107..111].copy_from_slice(&1u32.to_le_bytes());
    let references: [u32; 8] = [6645, 6650, 6653, 6656, 6659, 6662, 6665, 6668];
    bytes.extend_from_slice(&(references.len() as u32).to_le_bytes());
    for reference in references {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&310u32.to_le_bytes());
    lp_utf16(&mut bytes, "CoilPrimitive");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&309u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&6644u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 6644,
        class_tag: "353".into(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("compact Coil scope");
    assert_eq!(scope.coil_operation, Some(DesignExtrudeOperation::NewBody));
    assert_eq!(scope.coil_extent, Some(DesignCoilExtent::RevolutionsHeight));
    assert_eq!(scope.coil_section, Some(DesignCoilSection::Circular));
    assert_eq!(
        scope.coil_section_placement,
        Some(DesignCoilSectionPlacement::Inside)
    );
    assert_eq!(scope.coil_clockwise, Some(false));

    for (placement_code, placement) in [
        (1u32, DesignCoilSectionPlacement::Inside),
        (2u32, DesignCoilSectionPlacement::Center),
        (3u32, DesignCoilSectionPlacement::Outside),
    ] {
        for (section_code, section) in [
            (1u32, DesignCoilSection::Circular),
            (2u32, DesignCoilSection::Square),
            (3u32, DesignCoilSection::ExternalTriangle),
            (4u32, DesignCoilSection::InternalTriangle),
        ] {
            bytes[92..96].copy_from_slice(&placement_code.to_le_bytes());
            bytes[107..111].copy_from_slice(&section_code.to_le_bytes());
            let parsed =
                parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
                    .expect("compact Coil scope");
            assert_eq!(parsed.coil_section, Some(section));
            assert_eq!(parsed.coil_section_placement, Some(placement));
        }
    }

    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    let unsupported = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("unsupported Coil operation remains a native scope");
    assert!(unsupported.coil_operation.is_none());
}

#[test]
fn compact_coil_new_body_scope_accepts_unlinked_state_trailer() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"338");
    bytes.extend_from_slice(&6644u32.to_le_bytes());
    bytes.resize(228, 0);
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24] = 0;
    bytes[26..30].copy_from_slice(&4u32.to_le_bytes());
    bytes[30..34].copy_from_slice(&1u32.to_le_bytes());
    bytes[92..96].copy_from_slice(&1u32.to_le_bytes());
    bytes[107..111].copy_from_slice(&1u32.to_le_bytes());
    let references: [u32; 8] = [6645, 6650, 6653, 6656, 6659, 6662, 6665, 6668];
    bytes.extend_from_slice(&(references.len() as u32).to_le_bytes());
    for reference in references {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    lp_utf16(&mut bytes, "CoilPrimitive");
    let mut tail = [0; 88];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&6644u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 6644,
        class_tag: "338".into(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("compact Coil new-body scope");
    assert_eq!(scope.frame_length, 442);
    assert_eq!(scope.kind, "CoilPrimitive");
    assert_eq!(scope.coil_operation, Some(DesignExtrudeOperation::NewBody));
    assert_eq!(scope.history_state_id, Some(3));
    assert_eq!(scope.previous_history_state_id, None);
    assert_eq!(scope.previous_history_state_id_offset, 0);
}

#[test]
fn long_coil_scope_discriminators_use_the_ten_reference_envelope() {
    let scope = |frame_length: usize, operation: u32| {
        let reference_members: [u32; 10] =
            [1001, 1002, 1003, 1004, 1005, 1006, 1007, 1008, 1009, 1010];
        let kind = "CoilPrimitive";
        let kind_length = 4 + kind.encode_utf16().count() * 2;
        let kind_at = frame_length - 78 - kind_length;
        let reference_count_at = kind_at - 4 - 4 - reference_members.len() * 11;
        let mut bytes = vec![0; reference_count_at];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"345");
        bytes[7..11].copy_from_slice(&331u32.to_le_bytes());
        bytes[22..26].copy_from_slice(&operation.to_le_bytes());
        bytes[26..30].copy_from_slice(&1u32.to_le_bytes());
        for (offset, target) in [(30usize, 1005u32), (41, 1009)] {
            bytes[offset] = 1;
            bytes[offset + 1..offset + 5].copy_from_slice(&target.to_le_bytes());
        }
        if frame_length == 578 {
            let matrix: [f64; 16] = [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ];
            for (ordinal, value) in matrix.into_iter().enumerate() {
                bytes[77 + ordinal * 8..85 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&(reference_members.len() as u32).to_le_bytes());
        for reference in reference_members {
            bytes.push(1);
            bytes.extend_from_slice(&reference.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        bytes.extend_from_slice(&310u32.to_le_bytes());
        lp_utf16(&mut bytes, kind);
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"259");
        bytes.extend_from_slice(&331u32.to_le_bytes());
        assert_eq!(bytes.len(), frame_length + 11);
        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: 331,
            class_tag: "345".into(),
            byte_offset: 0,
        };
        parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
            .expect("long Coil scope")
    };

    let boolean = scope(450, 1);
    assert_eq!(boolean.coil_operation, Some(DesignExtrudeOperation::Join));
    assert_eq!(boolean.coil_operation_offset, Some(22));
    assert_eq!(boolean.coil_extent, None);
    assert_eq!(boolean.coil_section, Some(DesignCoilSection::Circular));
    assert_eq!(boolean.coil_section_offset, None);
    assert_eq!(
        boolean.coil_section_placement,
        Some(DesignCoilSectionPlacement::Inside)
    );
    assert_eq!(boolean.coil_section_placement_offset, None);
    assert_eq!(boolean.coil_clockwise, Some(false));
    assert_eq!(boolean.coil_clockwise_offset, None);

    let new_body = scope(578, 2);
    assert_eq!(
        new_body.coil_operation,
        Some(DesignExtrudeOperation::NewBody)
    );
    assert_eq!(new_body.coil_operation_offset, Some(22));
}

#[test]
fn sketch_profile_frame_resolves_its_decimal_entity_suffix() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"308");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&103u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b");
    lp_utf16(&mut bytes, "172");
    let tail_at = bytes.len();
    bytes.extend_from_slice(&[0; 94]);
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "308".into(),
        record_index: 100,
    };
    let entity = DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:entity#172".into(),
        byte_offset: 1000,
        entity_suffix: 172,
        entity_id: "0_172".into(),
        class_tag: "269".into(),
        optional_slot_present: false,
        module: Some(DESIGN_MODULE_SKETCH.to_owned()),
        record_reference: Some(200),
        record_reference_offset: Some(1010),
        declared_reference_count: Some(0),
        reference_indices: Vec::new(),
        reference_offsets: Vec::new(),
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
    };

    let profile = parse_sketch_profile(
        &bytes,
        "f3d:Design/BulkStream.dat",
        4,
        &header,
        std::slice::from_ref(&entity),
    )
    .expect("sketch-profile operand");
    assert_eq!(profile.scope_reference_ordinal, 4);
    assert_eq!(profile.entity_suffix, 172);
    assert_eq!(profile.entity_id, "0_172");
    assert_eq!(profile.paired_byte_offset, paired_at as u64);

    bytes.truncate(paired_at - 94);
    bytes[4..7].copy_from_slice(b"319");
    let mut compact_tail = vec![0; 93];
    compact_tail[0] = 1;
    compact_tail[8..12].copy_from_slice(&1u32.to_le_bytes());
    compact_tail[12] = 1;
    compact_tail[13..17].copy_from_slice(&500u32.to_le_bytes());
    compact_tail[41..45].copy_from_slice(&99u32.to_le_bytes());
    compact_tail[53..57].copy_from_slice(&99u32.to_le_bytes());
    compact_tail[57] = 1;
    compact_tail[58..62].copy_from_slice(&102u32.to_le_bytes());
    compact_tail[70] = 1;
    compact_tail[71..75].copy_from_slice(&101u32.to_le_bytes());
    compact_tail[82] = 1;
    compact_tail[83..87].copy_from_slice(&777u32.to_le_bytes());
    bytes.extend_from_slice(&compact_tail);
    let compact_paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"258");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    let compact_header = DesignRecordHeader {
        class_tag: "319".into(),
        ..header
    };
    let compact = parse_sketch_profile(
        &bytes,
        "f3d:Design/BulkStream.dat",
        2,
        &compact_header,
        std::slice::from_ref(&entity),
    )
    .expect("compact sketch-profile operand");
    assert_eq!(compact.scope_reference_ordinal, 2);
    assert_eq!(compact.paired_byte_offset, compact_paired_at as u64);

    bytes.truncate(tail_at);
    let mut omitted_ordinal_tail = vec![0; 89];
    omitted_ordinal_tail[0] = 1;
    omitted_ordinal_tail[8..12].copy_from_slice(&1u32.to_le_bytes());
    omitted_ordinal_tail[12] = 1;
    omitted_ordinal_tail[13..17].copy_from_slice(&500u32.to_le_bytes());
    omitted_ordinal_tail[41..45].copy_from_slice(&99u32.to_le_bytes());
    omitted_ordinal_tail[53] = 1;
    omitted_ordinal_tail[54..58].copy_from_slice(&102u32.to_le_bytes());
    omitted_ordinal_tail[66] = 1;
    omitted_ordinal_tail[67..71].copy_from_slice(&101u32.to_le_bytes());
    omitted_ordinal_tail[78] = 1;
    omitted_ordinal_tail[79..83].copy_from_slice(&777u32.to_le_bytes());
    bytes.extend_from_slice(&omitted_ordinal_tail);
    let omitted_paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"258");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    let omitted = parse_sketch_profile(
        &bytes,
        "f3d:Design/BulkStream.dat",
        2,
        &compact_header,
        std::slice::from_ref(&entity),
    )
    .expect("omitted-ordinal sketch-profile operand");
    assert_eq!(omitted.paired_byte_offset, omitted_paired_at as u64);
}

#[test]
fn base_flange_scope_has_exact_profile_and_thickness_fields() {
    let mut bytes = vec![0; 416];
    bytes[73..77].copy_from_slice(&1u32.to_le_bytes());
    bytes[81] = 1;
    bytes[82..86].copy_from_slice(&266u32.to_le_bytes());
    bytes[92..96].copy_from_slice(&1u32.to_le_bytes());
    bytes[112] = 1;
    bytes[113..117].copy_from_slice(&263u32.to_le_bytes());
    bytes[123..131].copy_from_slice(&0.25f64.to_le_bytes());
    bytes[141..145].copy_from_slice(&1u32.to_le_bytes());
    bytes[145] = 1;
    bytes[146..150].copy_from_slice(&256u32.to_le_bytes());

    let operation = crate::design::decode::scopes::exact_base_flange_operation(
        &bytes,
        0,
        416,
        &[256, 259, 263, 266],
    )
    .expect("fixed BaseFlange operation");
    assert_eq!(operation.thickness, 0.25);
    assert_eq!(operation.thickness_offset, 123);
    assert_eq!(operation.profile_group_record_index, 256);
    assert_eq!(operation.profile_record_index, 259);
    assert_eq!(operation.thickness_record_index, 263);
    assert_eq!(operation.settings_record_index, 266);

    bytes[123..131].copy_from_slice(&0.0f64.to_le_bytes());
    assert!(crate::design::decode::scopes::exact_base_flange_operation(
        &bytes,
        0,
        416,
        &[256, 259, 263, 266]
    )
    .is_none());
}

#[test]
fn edge_flange_scope_resolves_every_role_from_its_marked_slot() {
    // The ordered reference table is in record-index order, so the fixture
    // deliberately lists the settings record before the edge and aggregate
    // groups: a reader that assigned roles by table position would mis-bind it.
    let references = [201, 204, 207, 218, 221, 240, 243, 251, 254];
    let frame = edge_flange_frame(&EdgeFlangeFixture {
        header_shift: 0,
        width_count: 1,
        result_count: 2,
        bend_position: 2,
        height_datum: 1,
        reference_side: 4,
        bend_radius: 0.25,
        wrapper: 201,
        settings: 207,
        angle_owner: 218,
        height_owner: 204,
        aggregate_group: 240,
        edge_group: 251,
    });

    let operation = crate::design::decode::scopes::exact_edge_flange_operation(
        &frame.bytes,
        0,
        frame.paired_at,
        &references,
    )
    .expect("fixed EdgeFlange operation");
    assert_eq!(operation.edge_wrapper_record_indices, [201]);
    assert_eq!(operation.edge_group_record_indices, [251]);
    assert_eq!(operation.edge_operand_record_indices, [254]);
    assert_eq!(operation.aggregate_group_record_index, 240);
    assert_eq!(operation.aggregate_operand_record_indices, [243]);
    assert_eq!(operation.height_owner_record_index, 204);
    assert_eq!(operation.angle_owner_record_index, 218);
    assert_eq!(operation.settings_record_index, 207);
    assert_eq!(operation.bend_radius, 0.25);
    assert_eq!(operation.bend_radius_offset, frame.bend_radius_offset);
    assert_eq!(
        operation.bend_position,
        crate::records::DesignBendPosition::Inside
    );
    assert_eq!(
        operation.height_datum,
        crate::records::DesignSheetMetalHeightDatum::InnerFaces
    );
    // The one table entry no slot claims is the width-distance owner, which
    // makes this the symmetric edge-width mode.
    assert_eq!(operation.width_distance_owner_record_indices, [221]);
    assert_eq!(
        operation.edge_width_mode(),
        crate::records::DesignEdgeWidthMode::Symmetric
    );
}

#[test]
fn edge_flange_scope_reads_the_shifted_header_form() {
    // The optional four-byte header member is not announced, so the same frame
    // written four bytes later must still read through reference agreement.
    let references = [201, 204, 207, 218, 240, 243, 251, 254];
    for header_shift in [0usize, 4] {
        let frame = edge_flange_frame(&EdgeFlangeFixture {
            header_shift,
            width_count: 0,
            result_count: 1,
            bend_position: 3,
            height_datum: 2,
            reference_side: 4,
            bend_radius: 0.5,
            wrapper: 201,
            settings: 207,
            angle_owner: 218,
            height_owner: 204,
            aggregate_group: 240,
            edge_group: 251,
        });

        let operation = crate::design::decode::scopes::exact_edge_flange_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            &references,
        )
        .expect("fixed EdgeFlange operation");
        assert_eq!(
            operation.bend_position,
            crate::records::DesignBendPosition::Adjacent
        );
        assert_eq!(
            operation.height_datum,
            crate::records::DesignSheetMetalHeightDatum::OuterFaces
        );
        assert_eq!(
            operation.edge_width_mode(),
            crate::records::DesignEdgeWidthMode::FullEdge
        );
        assert!(operation.width_distance_owner_record_indices.is_empty());
    }
}

#[test]
fn edge_flange_scope_refuses_a_frame_whose_group_operand_is_absent() {
    // Record 254 is the edge group's operand. Without it the table has no entry
    // three after the group, so the frame is refused rather than half-bound.
    let references = [201, 204, 207, 218, 240, 243, 251, 255];
    let frame = edge_flange_frame(&EdgeFlangeFixture {
        header_shift: 0,
        width_count: 0,
        result_count: 1,
        bend_position: 1,
        height_datum: 2,
        reference_side: 4,
        bend_radius: 0.25,
        wrapper: 201,
        settings: 207,
        angle_owner: 218,
        height_owner: 204,
        aggregate_group: 240,
        edge_group: 251,
    });

    assert!(crate::design::decode::scopes::exact_edge_flange_operation(
        &frame.bytes,
        0,
        frame.paired_at,
        &references,
    )
    .is_none());
}

#[test]
fn edge_flange_scope_reads_the_single_edge_to_object_form() {
    use crate::records::DesignEdgeFlangeHeightExtent;

    let references = [201, 204, 207, 218, 221, 224, 240, 243, 251, 254, 270];
    for header_shift in [0usize, 4] {
        let frame = edge_flange_to_object_frame(header_shift);
        let operation = crate::design::decode::scopes::exact_edge_flange_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            &references,
        )
        .expect("fixed to-object EdgeFlange operation");
        assert_eq!(
            operation.width_distance_owner_record_indices,
            Vec::<u32>::new()
        );
        assert_eq!(operation.edge_group_record_indices, [251]);
        assert_eq!(operation.edge_operand_record_indices, [254]);
        assert_eq!(
            operation.height_extent,
            DesignEdgeFlangeHeightExtent::ToObject {
                target_group_record_index: 221,
                target_operand_record_index: 224,
                offset_owner_record_index: 270,
                reference_record_indices: [469, 470],
            }
        );
    }
}

#[test]
fn edge_flange_scope_refuses_a_to_object_frame_with_a_table_reference_pair() {
    let mut frame = edge_flange_to_object_frame(0);
    frame.bytes[85 + 109 + 1..85 + 109 + 5].copy_from_slice(&270u32.to_le_bytes());
    assert!(crate::design::decode::scopes::exact_edge_flange_operation(
        &frame.bytes,
        0,
        frame.paired_at,
        &[201, 204, 207, 218, 221, 224, 240, 243, 251, 254, 270]
    )
    .is_none());
}

/// Field values written into a synthetic single-edge `EdgeFlange` frame.
struct EdgeFlangeFixture {
    header_shift: usize,
    /// Width-distance parameter owners the edge-width mode adds to the table.
    width_count: usize,
    result_count: usize,
    bend_position: u32,
    height_datum: u32,
    reference_side: u32,
    bend_radius: f64,
    wrapper: u32,
    settings: u32,
    angle_owner: u32,
    height_owner: u32,
    aggregate_group: u32,
    edge_group: u32,
}

/// A synthetic frame plus the offsets the reader is expected to derive.
struct EdgeFlangeFrame {
    bytes: Vec<u8>,
    paired_at: usize,
    bend_radius_offset: u64,
}

/// Build a single-edge `EdgeFlange` frame from the settled fixed-section layout.
///
/// Every offset is computed from the layout rather than counted by hand, so the
/// fixture stays correct when a field width changes.
fn edge_flange_frame(fixture: &EdgeFlangeFixture) -> EdgeFlangeFrame {
    fn reference(bytes: &mut [u8], at: usize, record_index: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }

    let common = 85 + fixture.header_shift;
    let wrapper_at = common + 8;
    let settings_at = wrapper_at + 11;
    let datum_at = settings_at + 11;
    let angle_at = datum_at + 4;
    let height_at = angle_at + 11;
    let side_at = height_at + 11;
    let radius_at = side_at + 15;
    let result_count_at = radius_at + 14;
    let aggregate_at = radius_at + 22 + fixture.result_count * 15;
    let edge_group_at = aggregate_at + 27;
    let paired_at =
        493 + fixture.result_count * 15 + fixture.width_count * 11 + fixture.header_shift;

    let mut bytes = vec![0; paired_at.max(edge_group_at + 11)];
    bytes[common..common + 4].copy_from_slice(&fixture.bend_position.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, wrapper_at, fixture.wrapper);
    reference(&mut bytes, settings_at, fixture.settings);
    bytes[datum_at..datum_at + 4].copy_from_slice(&fixture.height_datum.to_le_bytes());
    reference(&mut bytes, angle_at, fixture.angle_owner);
    reference(&mut bytes, height_at, fixture.height_owner);
    bytes[side_at..side_at + 4].copy_from_slice(&fixture.reference_side.to_le_bytes());
    bytes[radius_at..radius_at + 8].copy_from_slice(&fixture.bend_radius.to_le_bytes());
    let result_count = u32::try_from(fixture.result_count).expect("result count fits u32");
    bytes[result_count_at..result_count_at + 4].copy_from_slice(&result_count.to_le_bytes());
    reference(&mut bytes, aggregate_at, fixture.aggregate_group);
    reference(&mut bytes, edge_group_at, fixture.edge_group);

    EdgeFlangeFrame {
        bytes,
        paired_at,
        bend_radius_offset: u64::try_from(radius_at).expect("radius offset fits u64"),
    }
}

fn edge_flange_to_object_frame(header_shift: usize) -> EdgeFlangeFrame {
    fn reference(bytes: &mut [u8], at: usize, record_index: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }

    let common = 85 + header_shift;
    let wrapper_at = common + 8;
    let settings_at = wrapper_at + 11;
    let datum_at = settings_at + 11;
    let angle_at = datum_at + 4;
    let height_at = angle_at + 11;
    let side_at = height_at + 11;
    let radius_at = side_at + 15;
    let target_group_at = common + 94;
    let target_reference_one_at = common + 109;
    let target_reference_two_at = common + 124;
    let aggregate_at = common + 143;
    let edge_group_at = common + 170;
    let paired_at = 576 + header_shift;
    let mut bytes = vec![0; paired_at];

    bytes[common..common + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, wrapper_at, 201);
    reference(&mut bytes, settings_at, 207);
    bytes[datum_at..datum_at + 4].copy_from_slice(&2u32.to_le_bytes());
    reference(&mut bytes, angle_at, 218);
    reference(&mut bytes, height_at, 204);
    bytes[side_at..side_at + 4].copy_from_slice(&4u32.to_le_bytes());
    bytes[radius_at..radius_at + 8].copy_from_slice(&0.25f64.to_le_bytes());
    bytes[radius_at + 14..radius_at + 18].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, target_group_at, 221);
    bytes[common + 105..common + 109].copy_from_slice(&2u32.to_le_bytes());
    reference(&mut bytes, target_reference_one_at, 469);
    bytes[common + 120..common + 124].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, target_reference_two_at, 470);
    bytes[common + 139..common + 143].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, aggregate_at, 240);
    bytes[common + 166..common + 170].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, edge_group_at, 251);

    EdgeFlangeFrame {
        bytes,
        paired_at,
        bend_radius_offset: u64::try_from(radius_at).expect("radius offset fits u64"),
    }
}

#[test]
fn edge_flange_scope_projects_a_typed_two_sided_neutral_flange() {
    use crate::records::{
        DesignBendPosition, DesignEdgeFlangeOperation, DesignParameterKind, DesignParameterScope,
        DesignSheetMetalHeightDatum,
    };
    use cadmpeg_ir::features::{
        FeatureDefinition, SheetMetalBendPosition, SheetMetalFlangeWidth, SheetMetalHeightDatum,
    };

    let stream = "f3d:FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#900"),
        "EdgeFlange",
        382,
    );
    scope.reference_members = vec![383, 385, 388, 393, 396, 399, 402, 404, 407, 411];
    scope.edge_flange_operation = Some(DesignEdgeFlangeOperation {
        edge_wrapper_record_indices: vec![383],
        edge_group_record_indices: vec![385],
        edge_operand_record_indices: vec![388],
        aggregate_group_record_index: 404,
        aggregate_operand_record_indices: vec![407],
        height_owner_record_index: 399,
        height_extent: crate::records::DesignEdgeFlangeHeightExtent::Distance,
        angle_owner_record_index: 402,
        width_distance_owner_record_indices: vec![393, 396],
        settings_record_index: 411,
        bend_radius: 0.25,
        bend_radius_offset: 156,
        reference_side_code: 4,
        height_datum: DesignSheetMetalHeightDatum::InnerFaces,
        bend_position: DesignBendPosition::Adjacent,
    });

    let owner =
        |record_index: u32, parameter_record_index: u32| crate::records::DesignParameterOwner {
            id: format!("{stream}:design-parameter-owner#{record_index}"),
            byte_offset: 0,
            class_tag: "000".into(),
            record_index,
            scope_record_index: 382,
            local_ordinal: 0,
            evaluated_value: 0.0,
            evaluated_value_offset: 0,
            parameter_record_index,
            owned_ordinal: 0,
            variant: None,
            companion_record_index: 0,
        };
    let parameter = |record_index: u32, source_kind: &str, unit: &str, evaluated_value: f64| {
        crate::records::DesignParameter {
            id: format!("{stream}:design-parameter#{record_index}"),
            byte_offset: 0,
            class_tag: "000".into(),
            record_index,
            family_discriminator: None,
            family_discriminator_offset: None,
            source_ordinal: 0,
            owner_record_index: None,
            expression: String::new(),
            expression_offset: 0,
            source_kind: source_kind.into(),
            source_kind_offset: 0,
            kind: DesignParameterKind::Dimension,
            unit: Some(unit.into()),
            unit_offset: None,
            name: source_kind.into(),
            name_offset: 0,
            evaluated_value,
            evaluated_value_offset: 0,
        }
    };
    let owners = [
        owner(393, 392),
        owner(396, 395),
        owner(399, 398),
        owner(402, 401),
    ];
    // Stored lengths are centimetres and stored angles are radians.
    let parameters = [
        parameter(392, "EdgeWidth_1", "mm", 3.0),
        parameter(395, "EdgeWidth_2", "mm", 1.5),
        parameter(398, "FlangeHeight", "mm", 2.5),
        parameter(401, "FlangeAngle", "deg", std::f64::consts::FRAC_PI_2),
    ];
    let group = crate::records::DesignConstructionOperandGroup {
        id: format!("{stream}:design-construction-operand-group#385"),
        scope_record_index: 382,
        scope_reference_ordinal: 1,
        record_index: 385,
        byte_offset: 0,
        class_tag: "000".into(),
        members: vec![388],
        lost_edge_references: Vec::new(),
        member_offsets: vec![0],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 0,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role: 0x0000_0008_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 0,
        paired_class_tag: "000".into(),
        paired_byte_offset: 0,
    };

    let inputs = crate::design::feature_project::ProjectInputs {
        native: &parameters,
        owners: &owners,
        scopes: &[],
        construction_groups: std::slice::from_ref(&group),
        fillet_radius_groups: &[],
        edge_operands: &[],
        edge_identity_operands: &[],
        entity_selection_operands: &[],
        curve_identities: &[],
        face_operands: &[],
        body_recipe_operands: &[],
        placements: &[],
        body_bindings: &[],
        histories: &[],
    };
    let definition = crate::design::feature_project::project_edge_flange(&scope, &inputs)
        .expect("typed EdgeFlange definition");

    let FeatureDefinition::SheetMetalEdgeFlange {
        height,
        angle,
        height_datum,
        bend_position,
        width,
        bend_radius,
        ..
    } = definition
    else {
        panic!("expected a sheet-metal edge flange");
    };
    let cadmpeg_ir::features::SheetMetalFlangeHeight::Distance(height) = height else {
        panic!("expected a distance flange height");
    };
    assert!((height.0 - 25.0).abs() < 1e-12);
    assert!((angle.0 - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert_eq!(height_datum, SheetMetalHeightDatum::InnerFaces);
    assert_eq!(bend_position, SheetMetalBendPosition::Adjacent);
    assert!((bend_radius.0 - 2.5).abs() < 1e-12);
    let SheetMetalFlangeWidth::TwoSides { first, second } = width else {
        panic!("expected a two-sided flange width");
    };
    assert!((first.0 - 30.0).abs() < 1e-12);
    assert!((second.0 - 15.0).abs() < 1e-12);
}

#[test]
fn edge_flange_scope_projects_a_to_object_height_to_a_work_plane() {
    use crate::records::{
        DesignBendPosition, DesignEdgeFlangeHeightExtent, DesignEdgeFlangeOperation,
        DesignParameterKind, DesignParameterScope, DesignSheetMetalHeightDatum,
    };
    use cadmpeg_ir::features::{
        FeatureDefinition, SheetMetalFlangeHeight, SheetMetalFlangeHeightTarget,
    };

    let stream = "f3d:FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#910"),
        "EdgeFlange",
        382,
    );
    scope.edge_flange_operation = Some(DesignEdgeFlangeOperation {
        edge_wrapper_record_indices: vec![383],
        edge_group_record_indices: vec![385],
        edge_operand_record_indices: vec![388],
        aggregate_group_record_index: 404,
        aggregate_operand_record_indices: vec![407],
        height_owner_record_index: 399,
        height_extent: DesignEdgeFlangeHeightExtent::ToObject {
            target_group_record_index: 421,
            target_operand_record_index: 424,
            offset_owner_record_index: 430,
            reference_record_indices: [469, 470],
        },
        angle_owner_record_index: 402,
        width_distance_owner_record_indices: Vec::new(),
        settings_record_index: 411,
        bend_radius: 0.25,
        bend_radius_offset: 156,
        reference_side_code: 4,
        height_datum: DesignSheetMetalHeightDatum::OuterFaces,
        bend_position: DesignBendPosition::Inside,
    });

    let owner =
        |record_index: u32, parameter_record_index: u32| crate::records::DesignParameterOwner {
            id: format!("{stream}:design-parameter-owner#{record_index}"),
            byte_offset: 0,
            class_tag: "000".into(),
            record_index,
            scope_record_index: 382,
            local_ordinal: 0,
            evaluated_value: 0.0,
            evaluated_value_offset: 0,
            parameter_record_index,
            owned_ordinal: 0,
            variant: None,
            companion_record_index: 0,
        };
    let parameter = |record_index: u32, source_kind: &str, unit: &str, evaluated_value: f64| {
        crate::records::DesignParameter {
            id: format!("{stream}:design-parameter#{record_index}"),
            byte_offset: 0,
            class_tag: "000".into(),
            record_index,
            family_discriminator: None,
            family_discriminator_offset: None,
            source_ordinal: 0,
            owner_record_index: None,
            expression: String::new(),
            expression_offset: 0,
            source_kind: source_kind.into(),
            source_kind_offset: 0,
            kind: DesignParameterKind::Dimension,
            unit: Some(unit.into()),
            unit_offset: None,
            name: source_kind.into(),
            name_offset: 0,
            evaluated_value,
            evaluated_value_offset: 0,
        }
    };
    let owners = [owner(399, 398), owner(402, 401), owner(430, 429)];
    let parameters = [
        parameter(398, "FlangeHeight", "mm", 2.5),
        parameter(401, "FlangeAngle", "deg", std::f64::consts::FRAC_PI_2),
        parameter(429, "ToObjectOffset", "mm", 1.5),
    ];

    let mut edge_group = crate::records::DesignConstructionOperandGroup {
        id: format!("{stream}:design-construction-operand-group#385"),
        scope_record_index: 382,
        scope_reference_ordinal: 1,
        record_index: 385,
        byte_offset: 0,
        class_tag: "000".into(),
        members: vec![388],
        lost_edge_references: Vec::new(),
        member_offsets: vec![0],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 0,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role: 0x0000_0008_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 0,
        paired_class_tag: "000".into(),
        paired_byte_offset: 0,
    };
    let mut target_group = edge_group.clone();
    target_group.id = format!("{stream}:design-construction-operand-group#421");
    target_group.scope_reference_ordinal = 2;
    target_group.record_index = 421;
    target_group.members = vec![424];
    target_group.role = 0x0000_0021_0000_0000;
    edge_group.member_offsets = vec![0];

    let target_selection = crate::records::DesignEntitySelectionOperand {
        id: format!("{stream}:design-entity-selection-operand#424"),
        scope_record_index: 382,
        group_record_index: 421,
        group_member_ordinal: 0,
        record_index: 424,
        byte_offset: 0,
        class_tag: "377".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        identity_record_index: 427,
        identity_record_offset: 0,
        primary_identity: 319,
        primary_identity_offset: 0,
        secondary_identity: None,
        secondary_identity_offset: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: 428,
        next_byte_offset: 0,
    };
    let mut target_scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#920"),
        "WorkPlane",
        320,
    );
    target_scope.work_plane_transform = Some([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);

    let groups = [edge_group, target_group];
    let target_scopes = [target_scope.clone()];
    let target_selections = [target_selection];
    let inputs = crate::design::feature_project::ProjectInputs {
        native: &parameters,
        owners: &owners,
        scopes: &target_scopes,
        construction_groups: &groups,
        fillet_radius_groups: &[],
        edge_operands: &[],
        edge_identity_operands: &[],
        entity_selection_operands: &target_selections,
        curve_identities: &[],
        face_operands: &[],
        body_recipe_operands: &[],
        placements: &[],
        body_bindings: &[],
        histories: &[],
    };
    let definition = crate::design::feature_project::project_edge_flange(&scope, &inputs)
        .expect("typed to-object EdgeFlange definition");
    let FeatureDefinition::SheetMetalEdgeFlange { height, .. } = definition else {
        panic!("expected a sheet-metal edge flange");
    };
    let SheetMetalFlangeHeight::ToObject { target, offset } = height else {
        panic!("expected a to-object flange height");
    };
    assert_eq!(
        target,
        SheetMetalFlangeHeightTarget::Feature(crate::ids::neutral_feature_id(&target_scope))
    );
    assert_eq!(offset.0, 15.0);
}

#[test]
fn edge_flange_scope_without_a_width_parameter_keeps_its_native_form() {
    use crate::records::{
        DesignBendPosition, DesignEdgeFlangeOperation, DesignParameterScope,
        DesignSheetMetalHeightDatum,
    };

    let stream = "f3d:FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#901"),
        "EdgeFlange",
        317,
    );
    scope.reference_members = vec![318, 320, 323, 328, 331, 334, 336, 339, 343];
    scope.edge_flange_operation = Some(DesignEdgeFlangeOperation {
        edge_wrapper_record_indices: vec![318],
        edge_group_record_indices: vec![320],
        edge_operand_record_indices: vec![323],
        aggregate_group_record_index: 336,
        aggregate_operand_record_indices: vec![339],
        height_owner_record_index: 331,
        height_extent: crate::records::DesignEdgeFlangeHeightExtent::Distance,
        angle_owner_record_index: 334,
        width_distance_owner_record_indices: vec![328],
        settings_record_index: 343,
        bend_radius: 0.25,
        bend_radius_offset: 156,
        reference_side_code: 4,
        height_datum: DesignSheetMetalHeightDatum::OuterFaces,
        bend_position: DesignBendPosition::Inside,
    });

    // The symmetric mode needs one `EdgeWidth` parameter. Without it the scope
    // has no complete width, so no partial neutral flange is reported.
    let inputs = crate::design::feature_project::ProjectInputs {
        native: &[],
        owners: &[],
        scopes: &[],
        construction_groups: &[],
        fillet_radius_groups: &[],
        edge_operands: &[],
        edge_identity_operands: &[],
        entity_selection_operands: &[],
        curve_identities: &[],
        face_operands: &[],
        body_recipe_operands: &[],
        placements: &[],
        body_bindings: &[],
        histories: &[],
    };
    assert!(crate::design::feature_project::project_edge_flange(&scope, &inputs).is_none());
}

#[test]
fn surface_patch_continuity_needs_every_boundary_to_agree() {
    use crate::records::{DesignParameterScope, DesignPatchContinuity, DesignSurfacePatchBoundary};
    use cadmpeg_ir::features::SurfaceContinuity;

    let boundary = |continuity: DesignPatchContinuity| DesignSurfacePatchBoundary {
        scope_reference_ordinal: 0,
        record_index: 0,
        is_seed_selection: false,
        continuity,
        flip: 2,
        scale: -1.0,
        model_reference: 0,
    };
    let scope_with = |boundaries: Vec<DesignSurfacePatchBoundary>| {
        let mut scope = DesignParameterScope::empty("f3d:test:scope#1", "SurfacePatch", 1);
        scope.surface_patch_boundaries = boundaries;
        scope
    };

    for (code, expected) in [
        (DesignPatchContinuity::Connected, SurfaceContinuity::Contact),
        (DesignPatchContinuity::Tangent, SurfaceContinuity::Tangent),
        (
            DesignPatchContinuity::Curvature,
            SurfaceContinuity::Curvature,
        ),
    ] {
        let scope = scope_with(vec![boundary(code), boundary(code)]);
        assert_eq!(
            crate::design::feature_project::surface_patch_continuity(&scope),
            Some(expected)
        );
    }

    // A patch whose boundaries impose different conditions has no single neutral
    // continuity, and one with no boundary record has none to report.
    let mixed = scope_with(vec![
        boundary(DesignPatchContinuity::Tangent),
        boundary(DesignPatchContinuity::Connected),
    ]);
    assert_eq!(
        crate::design::feature_project::surface_patch_boundary_continuities(&mixed),
        vec![SurfaceContinuity::Tangent, SurfaceContinuity::Contact]
    );
    assert!(crate::design::feature_project::surface_patch_continuity(&mixed).is_none());
    assert!(
        crate::design::feature_project::surface_patch_continuity(&scope_with(Vec::new())).is_none()
    );
    assert!(
        crate::design::feature_project::surface_patch_continuity(&scope_with(vec![boundary(
            DesignPatchContinuity::Unknown(9)
        )]))
        .is_none()
    );
    assert!(
        crate::design::feature_project::surface_patch_boundary_continuities(&scope_with(vec![
            boundary(DesignPatchContinuity::Unknown(9))
        ]))
        .is_empty()
    );
}

#[test]
fn surface_patch_projection_accepts_boundary_groups_at_either_reference_endpoint() {
    use crate::records::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame, DesignParameterScope,
        DesignPatchContinuity, DesignSurfacePatchBoundary,
    };
    use cadmpeg_ir::features::{FeatureDefinition, SurfaceContinuity};

    let mut scope = DesignParameterScope::empty("f3d:test:scope#1", "SurfacePatch", 1);
    scope.frame_length = 442;
    scope.reference_members = vec![900, 100, 101, 102, 110, 111, 112, 120, 121, 122];
    scope.surface_patch_boundaries = vec![
        DesignSurfacePatchBoundary {
            scope_reference_ordinal: 3,
            record_index: 102,
            is_seed_selection: false,
            continuity: DesignPatchContinuity::Connected,
            flip: 2,
            scale: -1.0,
            model_reference: 100,
        },
        DesignSurfacePatchBoundary {
            scope_reference_ordinal: 6,
            record_index: 112,
            is_seed_selection: true,
            continuity: DesignPatchContinuity::Connected,
            flip: 2,
            scale: -1.0,
            model_reference: 110,
        },
        DesignSurfacePatchBoundary {
            scope_reference_ordinal: 9,
            record_index: 122,
            is_seed_selection: false,
            continuity: DesignPatchContinuity::Connected,
            flip: 2,
            scale: -1.0,
            model_reference: 120,
        },
    ];
    let group = |record_index, ordinal, member| DesignConstructionOperandGroup {
        id: format!("f3d:test:construction-group#{record_index}"),
        scope_record_index: scope.record_index,
        scope_reference_ordinal: ordinal,
        record_index,
        byte_offset: 0,
        class_tag: "277".into(),
        members: vec![member],
        lost_edge_references: Vec::new(),
        member_offsets: vec![0],
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role: 0x0000_0004_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 0,
        paired_class_tag: "260".into(),
        paired_byte_offset: 0,
    };
    let shifted_groups = [group(100, 1, 101), group(110, 4, 111), group(120, 7, 121)];
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &scope,
            &shifted_groups,
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface {
            continuity: Some(SurfaceContinuity::Contact),
            ref boundary_continuities,
            ..
        }) if boundary_continuities == &[
            SurfaceContinuity::Contact,
            SurfaceContinuity::Contact,
            SurfaceContinuity::Contact,
        ]
    ));

    scope.reference_members = vec![100, 101, 102, 110, 111, 112, 120, 121, 122, 900];
    for (boundary, ordinal) in scope.surface_patch_boundaries.iter_mut().zip([2_u32, 5, 8]) {
        boundary.scope_reference_ordinal = ordinal;
    }
    let endpoint_groups = [group(100, 0, 101), group(110, 3, 111), group(120, 6, 121)];
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &scope,
            &endpoint_groups,
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface {
            continuity: Some(SurfaceContinuity::Contact),
            ref boundary_continuities,
            ..
        }) if boundary_continuities == &[
            SurfaceContinuity::Contact,
            SurfaceContinuity::Contact,
            SurfaceContinuity::Contact,
        ]
    ));
}

#[test]
fn hem_scope_projects_each_decoded_owner_layout() {
    use crate::records::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame, DesignHemOperation,
        DesignHemParameterOwners, DesignParameter, DesignParameterKind, DesignParameterOwner,
        DesignParameterScope,
    };
    use cadmpeg_ir::features::{FeatureDefinition, SheetMetalHemDirection, SheetMetalHemForm};

    let stream = "f3d:FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let owner = |scope_record_index: u32,
                 record_index: u32,
                 parameter_record_index: u32|
     -> DesignParameterOwner {
        DesignParameterOwner {
            id: format!("{stream}:design-parameter-owner#{record_index}"),
            byte_offset: 0,
            class_tag: "000".into(),
            record_index,
            scope_record_index,
            local_ordinal: 0,
            evaluated_value: 0.0,
            evaluated_value_offset: 0,
            parameter_record_index,
            owned_ordinal: 0,
            variant: None,
            companion_record_index: 0,
        }
    };
    let parameter =
        |record_index: u32, source_kind: &str, unit: &str, value: f64| DesignParameter {
            id: format!("{stream}:design-parameter#{record_index}"),
            byte_offset: 0,
            class_tag: "000".into(),
            record_index,
            family_discriminator: None,
            family_discriminator_offset: None,
            source_ordinal: 0,
            owner_record_index: None,
            expression: String::new(),
            expression_offset: 0,
            source_kind: source_kind.into(),
            source_kind_offset: 0,
            kind: DesignParameterKind::Dimension,
            unit: Some(unit.into()),
            unit_offset: None,
            name: source_kind.into(),
            name_offset: 0,
            evaluated_value: value,
            evaluated_value_offset: 0,
        };
    let group = |scope_record_index: u32, record_index: u32, member: u32, role: u64| {
        DesignConstructionOperandGroup {
            id: format!("{stream}:design-construction-operand-group#{record_index}"),
            scope_record_index,
            scope_reference_ordinal: 0,
            record_index,
            byte_offset: 0,
            class_tag: "000".into(),
            members: vec![member],
            lost_edge_references: Vec::new(),
            member_offsets: vec![0],
            frame: DesignConstructionOperandGroupFrame {
                member_count_offset: 0,
                auxiliary_record_indices: Vec::new(),
                auxiliary_record_offsets: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_record_indices: Vec::new(),
                trailing_record_offsets: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 0,
                opaque_index_offset: 0,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 0,
                variant: false,
            },
            role,
            extrude_role: None,
            extrude_face_role: None,
            role_offset: 0,
            paired_class_tag: "000".into(),
            paired_byte_offset: 0,
        }
    };
    let operation = |parameter_owners| DesignHemOperation {
        edge_wrapper_record_index: 708,
        edge_group_record_index: 710,
        edge_operand_record_index: 713,
        aggregate_group_record_index: 717,
        aggregate_operand_record_index: 720,
        parameter_owners,
        settings_record_index: 724,
        bend_radius: 0.25,
        bend_radius_offset: 100,
        form_code: 3,
        direction_code: 1,
        direction_reversal_byte: 0,
        reference_side_code: 4,
    };
    let project = |record_index: u32,
                   operation: DesignHemOperation,
                   owners: Vec<DesignParameterOwner>,
                   parameters: Vec<DesignParameter>| {
        let mut scope = DesignParameterScope::empty(
            &format!("{stream}:design-parameter-scope#{record_index}"),
            "Hem",
            record_index,
        );
        scope.hem_operation = Some(operation);
        let groups = vec![
            group(record_index, 710, 713, 0x0000_0008_0000_0000),
            group(record_index, 717, 720, 0x0000_0043_0000_0000),
        ];
        let inputs = crate::design::feature_project::ProjectInputs {
            native: &parameters,
            owners: &owners,
            scopes: &[],
            construction_groups: &groups,
            fillet_radius_groups: &[],
            edge_operands: &[],
            edge_identity_operands: &[],
            entity_selection_operands: &[],
            curve_identities: &[],
            face_operands: &[],
            body_recipe_operands: &[],
            placements: &[],
            body_bindings: &[],
            histories: &[],
        };
        crate::design::feature_project::project_hem(&scope, &inputs).expect("typed Hem definition")
    };

    let gap_length = project(
        900,
        operation(DesignHemParameterOwners::GapLength {
            gap_owner_record_index: 901,
            length_owner_record_index: 902,
        }),
        vec![owner(900, 901, 903), owner(900, 902, 904)],
        vec![
            parameter(903, "HemGap", "mm", 0.02),
            parameter(904, "HemLength", "mm", 10.0),
        ],
    );
    let rolled = project(
        910,
        operation(DesignHemParameterOwners::RadiusAngle {
            radius_owner_record_index: 911,
            angle_owner_record_index: 912,
        }),
        vec![owner(910, 911, 913), owner(910, 912, 914)],
        vec![
            parameter(913, "HemRadius", "mm", 0.5),
            parameter(914, "HemAngle", "deg", std::f64::consts::FRAC_PI_2),
        ],
    );
    let teardrop = project(
        920,
        operation(DesignHemParameterOwners::GapLengthRadius {
            gap_owner_record_index: 921,
            length_owner_record_index: 922,
            radius_owner_record_index: 923,
        }),
        vec![
            owner(920, 921, 924),
            owner(920, 922, 925),
            owner(920, 923, 926),
        ],
        vec![
            parameter(924, "HemGap", "mm", 0.25),
            parameter(925, "HemLength", "mm", 10.0),
            parameter(926, "HemRadius", "mm", 0.5),
        ],
    );

    let FeatureDefinition::SheetMetalHem {
        form,
        direction,
        bend_radius,
        ..
    } = gap_length
    else {
        panic!("expected a gap-length Hem");
    };
    assert_eq!(
        form,
        SheetMetalHemForm::GapLength {
            gap: cadmpeg_ir::features::Length(0.2),
            length: cadmpeg_ir::features::Length(100.0),
        }
    );
    assert_eq!(direction, SheetMetalHemDirection::Unresolved);
    assert_eq!(bend_radius, cadmpeg_ir::features::Length(2.5));

    let FeatureDefinition::SheetMetalHem { form, .. } = rolled else {
        panic!("expected a rolled Hem");
    };
    assert_eq!(
        form,
        SheetMetalHemForm::Rolled {
            radius: cadmpeg_ir::features::Length(5.0),
            angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
        }
    );

    let FeatureDefinition::SheetMetalHem { form, .. } = teardrop else {
        panic!("expected a teardrop Hem");
    };
    assert_eq!(
        form,
        SheetMetalHemForm::Teardrop {
            gap: cadmpeg_ir::features::Length(2.5),
            length: cadmpeg_ir::features::Length(100.0),
            radius: cadmpeg_ir::features::Length(5.0),
        }
    );
}

#[test]
fn hem_scope_binds_parameters_edge_groups_and_rule_radius() {
    // The table deliberately places the groups before the owners so a reader
    // assigning roles by table position cannot pass, and it is exercised under
    // both header shifts because the shift is not announced by any member.
    let references = [240, 243, 251, 254, 301, 304, 308, 311];
    for header_shift in [0usize, 4] {
        let frame = hem_frame(&HemFixture {
            header_shift,
            wrapper: 308,
            settings: 311,
            gap_owner: 301,
            length_owner: 304,
            aggregate_group: 240,
            edge_group: 251,
            bend_radius: 0.25,
        });

        let operation = crate::design::decode::scopes::exact_hem_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            &references,
        )
        .expect("fixed Hem operation");
        assert_eq!(operation.edge_wrapper_record_index, 308);
        assert_eq!(operation.settings_record_index, 311);
        assert_eq!(
            operation.parameter_owners,
            crate::records::DesignHemParameterOwners::GapLength {
                gap_owner_record_index: 301,
                length_owner_record_index: 304,
            }
        );
        assert_eq!(operation.aggregate_group_record_index, 240);
        assert_eq!(operation.aggregate_operand_record_index, 243);
        assert_eq!(operation.edge_group_record_index, 251);
        assert_eq!(operation.edge_operand_record_index, 254);
        assert_eq!(operation.bend_radius, 0.25);
        assert_eq!(operation.bend_radius_offset, frame.bend_radius_offset);
        // These four values are retained, not interpreted: each holds one value
        // across every readable hem form and direction state (DR-09A).
        assert_eq!(operation.form_code, 3);
        assert_eq!(operation.direction_code, 1);
        assert_eq!(operation.direction_reversal_byte, 0);
        assert_eq!(operation.reference_side_code, 4);
    }
}

#[test]
fn hem_scope_refuses_a_frame_whose_owner_slot_is_absent() {
    // The rolled form places its owner references at other offsets, so the
    // gap-and-length reader must refuse a frame whose owner slot does not agree.
    let references = [240, 243, 251, 254, 301, 304, 308, 311];
    let mut frame = hem_frame(&HemFixture {
        header_shift: 0,
        wrapper: 308,
        settings: 311,
        gap_owner: 301,
        length_owner: 304,
        aggregate_group: 240,
        edge_group: 251,
        bend_radius: 0.25,
    });
    // Move the length-owner reference one byte later, as the rolled form does.
    let at = 85 + 53;
    frame.bytes[at..at + 11].fill(0);
    frame.bytes[at + 1] = 1;
    frame.bytes[at + 2..at + 6].copy_from_slice(&304u32.to_le_bytes());
    assert!(crate::design::decode::scopes::exact_hem_operation(
        &frame.bytes,
        0,
        frame.paired_at,
        &references,
    )
    .is_none());
}

#[test]
fn hem_scope_reads_the_rolled_owner_layout() {
    let references = [708, 717, 720, 724, 775, 788, 790, 793];
    let frame = rolled_hem_frame();
    let operation = crate::design::decode::scopes::exact_hem_operation(
        &frame.bytes,
        0,
        frame.paired_at,
        &references,
    )
    .expect("rolled Hem operation");
    assert_eq!(
        operation.parameter_owners,
        crate::records::DesignHemParameterOwners::RadiusAngle {
            radius_owner_record_index: 775,
            angle_owner_record_index: 788,
        }
    );
    assert_eq!(operation.bend_radius, 0.25);
    assert_eq!(operation.bend_radius_offset, 160);
}

#[test]
fn hem_scope_reads_the_teardrop_owner_layout() {
    let references = [703, 706, 708, 717, 720, 724, 775, 777, 780];
    let frame = teardrop_hem_frame();
    let operation = crate::design::decode::scopes::exact_hem_operation(
        &frame.bytes,
        0,
        frame.paired_at,
        &references,
    )
    .expect("teardrop Hem operation");
    assert_eq!(
        operation.parameter_owners,
        crate::records::DesignHemParameterOwners::GapLengthRadius {
            gap_owner_record_index: 703,
            length_owner_record_index: 706,
            radius_owner_record_index: 775,
        }
    );
    assert_eq!(operation.bend_radius, 0.25);
    assert_eq!(operation.bend_radius_offset, 170);
}

/// Field values written into a synthetic gap-and-length `Hem` frame.
struct HemFixture {
    header_shift: usize,
    wrapper: u32,
    settings: u32,
    gap_owner: u32,
    length_owner: u32,
    aggregate_group: u32,
    edge_group: u32,
    bend_radius: f64,
}

/// A synthetic frame plus the offsets the reader is expected to derive.
struct HemFrame {
    bytes: Vec<u8>,
    paired_at: usize,
    bend_radius_offset: u64,
}

/// Build a gap-and-length `Hem` frame from the settled fixed-section layout.
///
/// Every offset is computed from the layout rather than counted by hand.
fn hem_frame(fixture: &HemFixture) -> HemFrame {
    fn reference(bytes: &mut [u8], at: usize, record_index: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }

    let common = 85 + fixture.header_shift;
    let paired_at = 494 + fixture.header_shift;
    let mut bytes = vec![0; paired_at];
    bytes[common..common + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, common + 8, fixture.wrapper);
    reference(&mut bytes, common + 19, fixture.settings);
    bytes[common + 30..common + 34].copy_from_slice(&1u32.to_le_bytes());
    bytes[common + 36..common + 40].copy_from_slice(&4u32.to_le_bytes());
    reference(&mut bytes, common + 42, fixture.gap_owner);
    reference(&mut bytes, common + 53, fixture.length_owner);
    let radius_at = common + 71;
    bytes[radius_at..radius_at + 8].copy_from_slice(&fixture.bend_radius.to_le_bytes());
    reference(&mut bytes, common + 108, fixture.aggregate_group);
    reference(&mut bytes, common + 135, fixture.edge_group);

    HemFrame {
        bytes,
        paired_at,
        bend_radius_offset: u64::try_from(radius_at).expect("radius offset fits u64"),
    }
}

/// Build the rolled `Hem` frame. Its header shift is four bytes and its owner
/// slots are thirteen bytes apart.
fn rolled_hem_frame() -> HemFrame {
    fn reference(bytes: &mut [u8], at: usize, record_index: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }

    let common = 89;
    let paired_at = 498;
    let mut bytes = vec![0; paired_at];
    bytes[common..common + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, common + 8, 708);
    reference(&mut bytes, common + 19, 724);
    reference(&mut bytes, common + 41, 788);
    reference(&mut bytes, common + 54, 775);
    bytes[common + 71..common + 79].copy_from_slice(&0.25f64.to_le_bytes());
    reference(&mut bytes, common + 108, 717);
    reference(&mut bytes, common + 135, 790);
    HemFrame {
        bytes,
        paired_at,
        bend_radius_offset: 160,
    }
}

/// Build the teardrop `Hem` frame. The third parameter owner shifts the group
/// slots by ten bytes and moves the fixed rule radius to offset eighty-one.
fn teardrop_hem_frame() -> HemFrame {
    fn reference(bytes: &mut [u8], at: usize, record_index: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }

    let common = 89;
    let paired_at = 519;
    let mut bytes = vec![0; paired_at];
    bytes[common..common + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, common + 8, 708);
    reference(&mut bytes, common + 19, 724);
    reference(&mut bytes, common + 42, 703);
    reference(&mut bytes, common + 53, 706);
    reference(&mut bytes, common + 64, 775);
    bytes[common + 81..common + 89].copy_from_slice(&0.25f64.to_le_bytes());
    reference(&mut bytes, common + 118, 717);
    reference(&mut bytes, common + 145, 777);
    HemFrame {
        bytes,
        paired_at,
        bend_radius_offset: 170,
    }
}

#[test]
fn construction_operand_groups_have_exact_counted_and_direct_frames() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    let scope = DesignParameterScope {
        id: "f3d:Design/BulkStream.dat:scope#12".into(),
        byte_offset: 1000,
        class_tag: "301".into(),
        record_index: 12,
        frame_length: 200,
        kind: "Extrude".into(),
        kind_offset: 1100,
        extrude_prologue: None,
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: None,
        history_state_id_offset: 0,
        previous_history_state_id: None,
        previous_history_state_id_offset: 0,
        reference_count_offset: 1080,
        reference_members: vec![100, 200, 201],
        reference_member_offsets: vec![1085, 1096, 1107],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        unclosed_construction_operand_groups: Vec::new(),
        work_point_reference_type: None,
        work_point_input_record_indices: Vec::new(),
        extrude_profile: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: 1200,
    };
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "332".into(),
        record_index: 100,
    };
    let mut bytes = Vec::new();
    header(&mut bytes, *b"332", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for member in [200u32, 201] {
        bytes.push(1);
        bytes.extend_from_slice(&member.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&300u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&0x0000_0008_0000_0000u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&180u32.to_le_bytes());
    bytes.extend_from_slice(&0.125f64.to_le_bytes());
    bytes.extend_from_slice(&180u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&102u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&[1, 1, 0, 1]);
    bytes.extend_from_slice(&101u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);
    bytes.push(1);
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    let paired_at = bytes.len();
    header(&mut bytes, *b"259", 100);

    let group = parse_construction_operand_group(&bytes, &scope, 0, &record)
        .complete()
        .expect("counted Extrude operand group");
    assert_eq!(group.members, [200, 201]);
    assert_eq!(group.member_offsets, [26, 37]);
    assert_eq!(group.role, 0x0000_0008_0000_0000);
    assert_eq!(group.extrude_role, Some(DesignExtrudeOperandRole::Bodies));
    assert_eq!(group.frame.member_count_offset, 21);
    assert!(group.frame.auxiliary_record_indices.is_empty());
    assert_eq!(group.frame.trailing_record_indices, [300]);
    assert_eq!(group.frame.opaque_index, 180);
    assert_eq!(group.frame.opaque_scalar, 0.125);
    assert!(group.frame.variant);
    assert_eq!(group.paired_byte_offset, paired_at as u64);

    let mut whole_body_bytes = bytes.clone();
    whole_body_bytes[group.role_offset as usize..group.role_offset as usize + 8]
        .copy_from_slice(&0x0000_0004_0000_0000u64.to_le_bytes());
    let whole_body = parse_construction_operand_group(&whole_body_bytes, &scope, 0, &record)
        .complete()
        .expect("counted Extrude whole-body group");
    assert_eq!(whole_body.role, 0x0000_0004_0000_0000);
    assert_eq!(
        whole_body.extrude_role,
        Some(DesignExtrudeOperandRole::Bodies)
    );

    let mut flagged = bytes[..11].to_vec();
    flagged.extend_from_slice(&[0; 9]);
    flagged.push(1);
    flagged.extend_from_slice(&1u32.to_le_bytes());
    for value in [
        b"DcFeatureOperationIdFlag".as_slice(),
        b"IntrinsicMetaTypeuint64".as_slice(),
    ] {
        flagged.extend_from_slice(&u32::try_from(value.len()).unwrap().to_le_bytes());
        flagged.extend_from_slice(value);
    }
    flagged.extend_from_slice(&445u64.to_le_bytes());
    let flagged_count_at = flagged.len();
    flagged.extend_from_slice(&bytes[21..]);
    let flagged = parse_construction_operand_group(&flagged, &scope, 0, &record)
        .complete()
        .expect("operation-flagged counted operand group");
    assert_eq!(flagged.frame.member_count_offset, flagged_count_at as u64);
    assert_eq!(flagged.members, [200, 201]);
    assert_eq!(flagged.role, 0x0000_0008_0000_0000);

    let mut start_face_bytes = bytes.clone();
    start_face_bytes[group.role_offset as usize..group.role_offset as usize + 8]
        .copy_from_slice(&0x0000_0005_0000_0000u64.to_le_bytes());
    let retained_role_five =
        parse_construction_operand_group(&start_face_bytes, &scope, 0, &record)
            .complete()
            .expect("counted Extrude retained role-five group");
    assert_eq!(retained_role_five.extrude_role, None);

    let mut from_face_scope = scope.clone();
    from_face_scope.extrude_prologue = Some(DesignExtrudePrologue::ReferenceAware {
        reference: None,
        operation: DesignExtrudeOperation::Cut,
        operation_offset: 1028,
        extent_discriminators: [1, 2],
        extent: DesignExtrudeExtent::OneSidedDistance,
        extent_discriminator_offsets: [1032, 1036],
        direction_reversed: false,
        direction_reversed_offset: 1040,
        solid_operation: true,
        solid_operation_offset: 1041,
        start: DesignExtrudeStart::FromFace,
        start_offset: 1042,
    });
    let start_face =
        parse_construction_operand_group(&start_face_bytes, &from_face_scope, 0, &record)
            .complete()
            .expect("counted Extrude start-face group");
    assert_eq!(start_face.role, 0x0000_0005_0000_0000);
    assert_eq!(
        start_face.extrude_role,
        Some(DesignExtrudeOperandRole::Faces)
    );

    let tail_at = 11 + 10 + 4 + 2 * 11;
    let mut flagless = bytes[..tail_at + 62].to_vec();
    flagless.extend_from_slice(&[0; 2]);
    flagless.push(1);
    flagless.extend_from_slice(&101u32.to_le_bytes());
    flagless.extend_from_slice(&[0; 7]);
    flagless.push(1);
    flagless.extend_from_slice(&12u32.to_le_bytes());
    flagless.extend_from_slice(&[0; 6]);
    let flagless_paired_at = flagless.len();
    header(&mut flagless, *b"259", 100);
    let flagless = parse_construction_operand_group(&flagless, &scope, 0, &record)
        .complete()
        .expect("flagless counted operand group");
    assert_eq!(flagless.members, [200, 201]);
    assert_eq!(flagless.role, 0x0000_0008_0000_0000);
    assert!(!flagless.frame.variant);
    assert_eq!(
        flagless.paired_byte_offset,
        u64::try_from(flagless_paired_at).unwrap()
    );

    // A corrupt member count larger than the remaining bytes can supply must
    // fail the parse without reaching the allocator.
    let mut bombed = bytes.clone();
    bombed[21..25].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        parse_construction_operand_group(&bombed, &scope, 0, &record),
        ConstructionOperandGroupParse::NotAGroup
    ));

    // A record that opens the grammar but whose tail names another record is a
    // group this reader cannot read, not a reference member that is not a group.
    let mut truncated = bytes.clone();
    let tail_at = truncated.len() - 40;
    truncated[tail_at..].fill(0x5a);
    assert!(matches!(
        parse_construction_operand_group(&truncated, &scope, 0, &record),
        ConstructionOperandGroupParse::Unclosed
    ));

    // Both optional references after the member run are present and the counted
    // identity run is empty: the shape a fixed-offset reader cannot reach.
    let mut auxiliary = Vec::new();
    header(&mut auxiliary, *b"283", 100);
    auxiliary.extend_from_slice(&[0; 10]);
    auxiliary.extend_from_slice(&1u32.to_le_bytes());
    for record_index in [109u32, 103, 106] {
        auxiliary.push(1);
        auxiliary.extend_from_slice(&record_index.to_le_bytes());
        auxiliary.extend_from_slice(&[0; 6]);
    }
    auxiliary.extend_from_slice(&0u32.to_le_bytes());
    auxiliary.extend_from_slice(&0x0000_0011_0000_0000u64.to_le_bytes());
    auxiliary.extend_from_slice(&[0; 10]);
    auxiliary.extend_from_slice(&31_003u32.to_le_bytes());
    auxiliary.extend_from_slice(&0.25f64.to_le_bytes());
    auxiliary.extend_from_slice(&31_003u32.to_le_bytes());
    auxiliary.push(1);
    auxiliary.extend_from_slice(&102u32.to_le_bytes());
    auxiliary.extend_from_slice(&[0; 6]);
    auxiliary.extend_from_slice(&[0; 2]);
    auxiliary.push(1);
    auxiliary.extend_from_slice(&101u32.to_le_bytes());
    auxiliary.extend_from_slice(&[0; 7]);
    auxiliary.push(1);
    auxiliary.extend_from_slice(&scope.record_index.to_le_bytes());
    auxiliary.extend_from_slice(&[0; 6]);
    let auxiliary_paired_at = auxiliary.len();
    header(&mut auxiliary, *b"259", 100);
    let auxiliary_record = DesignRecordHeader {
        class_tag: "283".into(),
        ..record.clone()
    };
    let auxiliary = parse_construction_operand_group(&auxiliary, &scope, 0, &auxiliary_record)
        .complete()
        .expect("Extrude face group carrying both optional references");
    assert_eq!(auxiliary.members, [109]);
    assert_eq!(auxiliary.member_offsets, [26]);
    assert_eq!(auxiliary.frame.auxiliary_record_indices, [103, 106]);
    assert_eq!(auxiliary.frame.auxiliary_record_offsets, [37, 48]);
    assert!(auxiliary.frame.trailing_record_indices.is_empty());
    assert_eq!(auxiliary.role, 0x0000_0011_0000_0000);
    assert_eq!(
        auxiliary.extrude_role,
        Some(DesignExtrudeOperandRole::Faces)
    );
    assert_eq!(auxiliary.paired_byte_offset, auxiliary_paired_at as u64);

    let mut split_scope = scope.clone();
    split_scope.kind = "SplitFace".into();
    split_scope.frame_length = 334;
    split_scope.reference_members = vec![100, 200, 201, 400, 500];
    split_scope.reference_member_offsets = vec![1085, 1096, 1107, 1118, 1129];
    let mut tool_group = group.clone();
    tool_group.id = "f3d:Design/BulkStream.dat:operand-group#100".into();
    tool_group.role = 0x0000_0021_0000_0000;
    let mut target_group = group.clone();
    target_group.id = "f3d:Design/BulkStream.dat:operand-group#400".into();
    target_group.record_index = 400;
    target_group.scope_reference_ordinal = 3;
    target_group.members = vec![500];
    target_group.member_offsets = vec![1129];
    target_group.role = 0x0000_0010_0000_0000;
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&split_scope),
        &[tool_group, target_group],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::SplitFace {
            targets: cadmpeg_ir::features::FaceSelection::Native(targets),
            tool: cadmpeg_ir::features::SplitFaceTool::Path(
                cadmpeg_ir::features::PathRef::Native(tool),
            ),
        } if targets.ends_with("#400") && tool.ends_with("#100")
    ));

    let mut split_body_scope = scope.clone();
    split_body_scope.kind = "Split".into();
    split_body_scope.frame_length = 325;
    split_body_scope.reference_members = vec![100, 200, 400, 500];
    split_body_scope.reference_member_offsets = vec![1085, 1096, 1107, 1118];
    let mut split_tool_group = group.clone();
    split_tool_group.id = "f3d:Design/BulkStream.dat:operand-group#100".into();
    split_tool_group.record_index = 100;
    split_tool_group.scope_reference_ordinal = 0;
    split_tool_group.members = vec![200];
    split_tool_group.member_offsets = vec![1096];
    split_tool_group.role = 0x0000_0009_0000_0000;
    let mut split_target_group = group.clone();
    split_target_group.id = "f3d:Design/BulkStream.dat:operand-group#400".into();
    split_target_group.record_index = 400;
    split_target_group.scope_reference_ordinal = 2;
    split_target_group.members = vec![500];
    split_target_group.member_offsets = vec![1118];
    split_target_group.role = 0x0000_0004_0000_0000;
    let split_tool = DesignFaceOperand {
        id: "f3d:Design/BulkStream.dat:face-operand#200".into(),
        scope_record_index: split_body_scope.record_index,
        scope_reference_ordinal: 1,
        group_record_index: Some(100),
        group_member_ordinal: Some(0),
        record_index: 200,
        byte_offset: 1200,
        class_tag: "297".into(),
        paired_byte_offset: 1400,
        paired_class_tag: "259".into(),
        recipe_record_index: 203,
        recipe_record_byte_offset: 1300,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#1300".into(),
        recipe_prefix_offset: 1311,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: ConstructionRecipeKind::Face,
        recipe_program_offset: 1350,
        recipe_program: vec![0, -1],
        recipe_node_offsets: Vec::new(),
        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        next_record_index: 204,
        next_byte_offset: 1411,
    };
    let split_groups = [split_target_group.clone(), split_tool_group.clone()];
    assert!(matches!(
        project_split(
            &split_body_scope,
            &split_groups,
            std::slice::from_ref(&split_tool)
        ),
        Some(FeatureDefinition::SplitBody {
            targets: cadmpeg_ir::features::BodySelection::Native(ref targets),
            tools: cadmpeg_ir::features::FaceSelection::Native(ref tool),
        }) if targets.ends_with("#400") && tool.ends_with("#200")
    ));

    let mut multiple_targets_scope = split_body_scope.clone();
    multiple_targets_scope.frame_length = 358;
    multiple_targets_scope.reference_members = vec![100, 200, 400, 500, 501];
    let mut multiple_targets = split_target_group.clone();
    multiple_targets.members = vec![500, 501];
    assert!(matches!(
        project_split(
            &multiple_targets_scope,
            &[split_tool_group.clone(), multiple_targets],
            std::slice::from_ref(&split_tool)
        ),
        Some(FeatureDefinition::SplitBody { .. })
    ));

    let mut construction_tool_scope = split_body_scope.clone();
    construction_tool_scope.frame_length = 347;
    construction_tool_scope.reference_members = vec![100, 200, 201, 400, 500];
    let mut construction_tool = split_tool_group.clone();
    construction_tool.role = 0x0000_0021_0000_0000;
    construction_tool.members = vec![200, 201];
    split_target_group.scope_reference_ordinal = 3;
    assert!(matches!(
        project_split(
            &construction_tool_scope,
            &[split_target_group.clone(), construction_tool],
            &[]
        ),
        Some(FeatureDefinition::SplitBody {
            tools: cadmpeg_ir::features::FaceSelection::Native(ref tool),
            ..
        }) if tool.ends_with("#100")
    ));
    split_target_group.scope_reference_ordinal = 2;

    let mut invalid_groups = Vec::new();
    invalid_groups.push(vec![split_target_group.clone()]);
    let mut oversized_tool = split_tool_group.clone();
    oversized_tool.members = vec![200, 201, 202, 203];
    invalid_groups.push(vec![oversized_tool, split_target_group.clone()]);
    for mutate in 0..4 {
        let mut tool = split_tool_group.clone();
        match mutate {
            0 => tool.scope_reference_ordinal = 1,
            1 => tool.record_index = 101,
            2 => tool.role = 0x0000_0008_0000_0000,
            3 => tool.members = vec![201],
            _ => unreachable!(),
        }
        invalid_groups.push(vec![tool, split_target_group.clone()]);
    }
    for mutate in 0..4 {
        let mut target = split_target_group.clone();
        match mutate {
            0 => target.scope_reference_ordinal = 3,
            1 => target.record_index = 401,
            2 => target.role = 0x0000_0005_0000_0000,
            3 => target.members = vec![501],
            _ => unreachable!(),
        }
        invalid_groups.push(vec![split_tool_group.clone(), target]);
    }
    assert!(invalid_groups.iter().all(|groups| project_split(
        &split_body_scope,
        groups,
        std::slice::from_ref(&split_tool)
    )
    .is_none()));
    let mut nonterminal_tool = split_tool;
    nonterminal_tool.recipe_program = vec![0, -1, 2];
    assert!(project_split(
        &split_body_scope,
        &split_groups,
        std::slice::from_ref(&nonterminal_tool)
    )
    .is_none());

    let mut delete_scope = scope.clone();
    delete_scope.kind = "DeleteFace".into();
    delete_scope.frame_length = 258;
    delete_scope.kind_offset = 1161;
    delete_scope.reference_members = vec![100, 200];
    delete_scope.reference_member_offsets = vec![1085, 1096];
    let mut delete_group = group.clone();
    delete_group.id = "f3d:Design/BulkStream.dat:operand-group#100".into();
    delete_group.members = vec![200];
    delete_group.member_offsets = vec![1096];
    delete_group.role = 0x0000_0010_0000_0000;
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&delete_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        &[],
        &[],
    );
    assert_eq!(
        features[0].definition,
        FeatureDefinition::DeleteFace {
            faces: cadmpeg_ir::features::FaceSelection::Native(delete_group.id.clone()),
            heal: true,
        }
    );
    delete_scope.frame_length = 263;
    delete_scope.kind_offset = 1165;
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&delete_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::DeleteFace { heal: true, .. }
    ));
    delete_scope.frame_length += 1;
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&delete_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Native { ref kind, .. } if kind == "DeleteFace"
    ));

    let mut surface_scope = delete_scope.clone();
    let reference_bytes = 11 * surface_scope.reference_members.len() as u64;
    surface_scope.kind = "SurfaceDeleteFace".into();
    surface_scope.frame_length = 250 + reference_bytes;
    surface_scope.kind_offset = surface_scope.byte_offset + 140 + reference_bytes;
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&surface_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        &[],
        &[],
    );
    assert_eq!(
        features[0].definition,
        FeatureDefinition::DeleteFace {
            faces: cadmpeg_ir::features::FaceSelection::Native(delete_group.id.clone()),
            heal: false,
        }
    );
    surface_scope.frame_length = 251 + reference_bytes;
    surface_scope.kind_offset = surface_scope.byte_offset + 139 + reference_bytes;
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&surface_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::DeleteFace { heal: false, .. }
    ));
    surface_scope.frame_length = 236 + reference_bytes;
    surface_scope.kind_offset = surface_scope.byte_offset + 139 + reference_bytes;
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&surface_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Native { ref kind, .. } if kind == "SurfaceDeleteFace"
    ));

    let mut remove_scope = scope.clone();
    remove_scope.kind = "RemoveBody".into();
    let mut remove_group = group;
    remove_group.id = "f3d:Design/BulkStream.dat:operand-group#100".into();
    remove_group.role = 0x0000_0004_0000_0000;
    assert_eq!(
        crate::design::feature_project::project_remove_body(
            &remove_scope,
            std::slice::from_ref(&remove_group)
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::DeleteBody {
            bodies: cadmpeg_ir::features::BodySelection::Native(remove_group.id.clone()),
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
        })
    );

    let mut stitch_scope = scope;
    stitch_scope.kind = "SurfaceStitch".into();
    stitch_scope.reference_members = vec![100, 200, 300, 301];
    stitch_scope.surface_stitch_operation = Some(DesignSurfaceStitchOperation {
        gap_tolerance: 0.01,
        gap_tolerance_offset: 40,
        tolerance_record_index: 300,
        settings_record_index: 301,
    });
    let mut stitch_group = remove_group;
    stitch_group.members = vec![200];
    stitch_group.role = 0x0000_0005_0000_0000;
    assert_eq!(
        crate::design::feature_project::project_surface_stitch(
            &stitch_scope,
            std::slice::from_ref(&stitch_group)
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::KnitSurface {
            faces: cadmpeg_ir::features::FaceSelection::Native(stitch_scope.id),
            merge_entities: Some(true),
            create_solid: Some(true),
            gap_tolerance: Some(cadmpeg_ir::features::Length(0.1)),
        })
    );
}

#[test]
fn construction_operand_trailing_transform_has_exact_affine_frame() {
    let record_index = 300u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"339");
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);
    let transform = [
        [0.0_f64, -1.0, 0.0, 12.5],
        [1.0, 0.0, 0.0, -4.0],
        [0.0, 0.0, 1.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[1, 0]);
    let following_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"432");
    bytes.extend_from_slice(&(record_index + 1).to_le_bytes());
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#300".into(),
        byte_offset: 0,
        class_tag: "339".into(),
        record_index,
    };

    let parsed = parse_construction_operand_transform(&bytes, &header)
        .expect("exact construction-operand transform");
    assert_eq!(parsed.transform, transform);
    assert_eq!(parsed.transform_offset, 22);
    assert_eq!(parsed.following_record_index, 301);
    assert_eq!(parsed.following_byte_offset, following_at as u64);
    assert_eq!(parsed.following_class_tag, "432");

    bytes[150] = 0;
    assert!(parse_construction_operand_transform(&bytes, &header).is_none());

    let secondary = [
        [1.0_f64, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut dual = bytes[..21].to_vec();
    for value in transform.into_iter().flatten() {
        dual.extend_from_slice(&value.to_le_bytes());
    }
    for value in secondary.into_iter().flatten() {
        dual.extend_from_slice(&value.to_le_bytes());
    }
    dual.push(0);
    let dual_following_at = dual.len();
    dual.extend_from_slice(&3u32.to_le_bytes());
    dual.extend_from_slice(b"432");
    dual.extend_from_slice(&(record_index + 1).to_le_bytes());
    let parsed = parse_construction_operand_dual_transform(&dual, &header)
        .expect("exact dual construction-operand transform");
    assert_eq!(parsed.first_transform, transform);
    assert_eq!(parsed.first_transform_offset, 21);
    assert_eq!(parsed.second_transform, secondary);
    assert_eq!(parsed.second_transform_offset, 149);
    assert_eq!(dual_following_at, 278);
}

#[test]
fn construction_operand_trailing_flag_has_exact_compact_frame() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"374");
    bytes.extend_from_slice(&33602u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 1, 0]);
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#33602".into(),
        byte_offset: 0,
        class_tag: "374".into(),
        record_index: 33602,
    };

    let flag = parse_construction_operand_flag(&bytes, &header).expect("compact trailing flag");
    assert!(flag.value);
    assert_eq!(flag.value_offset, 22);

    bytes[22] = 2;
    assert!(parse_construction_operand_flag(&bytes, &header).is_none());
}

#[test]
fn construction_operand_auxiliary_paths_decode_transform_and_compact_frames() {
    fn header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    fn reference(bytes: &mut Vec<u8>, record_index: u32) {
        bytes.push(1);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }

    let scope_record_index = 40u32;
    let record_index = 100u32;
    let transform = [
        [0.0_f64, -1.0, 0.0, 12.5],
        [1.0, 0.0, 0.0, -4.0],
        [0.0, 0.0, 1.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut expanded = Vec::new();
    header(&mut expanded, b"304", record_index);
    expanded.extend_from_slice(&[0; 10]);
    expanded.push(1);
    expanded.extend_from_slice(&174u64.to_le_bytes());
    expanded.extend_from_slice(&[0; 3]);
    for value in transform.into_iter().flatten() {
        expanded.extend_from_slice(&value.to_le_bytes());
    }
    expanded.push(0);
    reference(&mut expanded, scope_record_index);
    reference(&mut expanded, record_index + 2);
    expanded.extend_from_slice(&[0; 6]);
    let expanded_following_at = expanded.len();
    header(&mut expanded, b"390", record_index + 1);
    let expanded_header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "304".into(),
        record_index,
    };
    let expanded = parse_construction_operand_path(&expanded, scope_record_index, &expanded_header)
        .expect("expanded selection path");
    assert_eq!(expanded.entity_ref, 174);
    assert_eq!(expanded.transform, Some(transform));
    assert_eq!(expanded.transform_offset, Some(33));
    assert_eq!(expanded.compact_variant, None);
    assert_eq!(expanded.scope_record_index_offset, 163);
    assert_eq!(expanded.nested_record_index, 102);
    assert_eq!(expanded.nested_record_index_offset, 174);
    assert_eq!(expanded.following_record_index, 101);
    assert_eq!(expanded.following_byte_offset, expanded_following_at as u64);

    let mut compact = Vec::new();
    header(&mut compact, b"304", record_index);
    compact.extend_from_slice(&[0; 10]);
    compact.push(1);
    compact.extend_from_slice(&18_064u64.to_le_bytes());
    compact.extend_from_slice(&[0, 0, 1, 0]);
    reference(&mut compact, scope_record_index);
    reference(&mut compact, record_index + 2);
    compact.extend_from_slice(&[0; 6]);
    let compact_following_at = compact.len();
    header(&mut compact, b"390", record_index + 1);
    let compact = parse_construction_operand_path(&compact, scope_record_index, &expanded_header)
        .expect("compact selection path");
    assert_eq!(compact.entity_ref, 18_064);
    assert_eq!(compact.transform, None);
    assert_eq!(compact.compact_variant, Some(true));
    assert_eq!(compact.scope_record_index_offset, 35);
    assert_eq!(compact.nested_record_index_offset, 46);
    assert_eq!(compact.following_byte_offset, compact_following_at as u64);
}

#[test]
fn construction_tracking_path_decodes_absent_and_present_related_identities() {
    fn header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    fn tracking_path(first: Option<u64>, second: Option<u64>) -> Vec<u8> {
        let wrapper_record_index = 300u32;
        let mut bytes = Vec::new();
        header(&mut bytes, b"361", wrapper_record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(wrapper_record_index + 1).to_le_bytes());
        bytes.extend_from_slice(&[0; 3]);
        header(&mut bytes, b"363", wrapper_record_index + 1);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&268u64.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(-1i32).to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        for identity in [first, second] {
            bytes.extend_from_slice(&u32::from(identity.is_some()).to_le_bytes());
            if let Some(identity) = identity {
                bytes.extend_from_slice(&identity.to_le_bytes());
            }
        }
        header(&mut bytes, b"301", wrapper_record_index + 2);
        bytes
    }

    let absent = tracking_path(None, None);
    let absent = parse_construction_tracking_path(&absent, 0, 300, "361")
        .expect("tracking path without related identities");
    assert_eq!(absent.carrier_record_index, 301);
    assert_eq!(absent.carrier_byte_offset, 33);
    assert_eq!(absent.primary_identity, 268);
    assert_eq!(absent.primary_identity_offset, 70);
    assert_eq!(absent.selector, -1);
    assert_eq!(absent.kind, 3);
    assert_eq!(absent.first_related_identity, None);
    assert_eq!(absent.second_related_identity, None);
    assert_eq!(absent.following_record_index, 302);
    assert_eq!(absent.following_byte_offset, 114);

    let present = tracking_path(Some(113), Some(119));
    let present = parse_construction_tracking_path(&present, 0, 300, "361")
        .expect("tracking path with related identities");
    assert_eq!(present.first_related_identity, Some(113));
    assert_eq!(present.first_related_identity_offset, Some(110));
    assert_eq!(present.second_related_identity, Some(119));
    assert_eq!(present.second_related_identity_offset, Some(122));
    assert_eq!(present.following_byte_offset, 130);
}

#[test]
fn ruled_surface_operation_reads_mode_parameters_and_ordered_edge_groups() {
    let mut bytes = vec![0; 366];
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    let reference = |bytes: &mut [u8], at: usize, record_index: u32| {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    };
    bytes[27] = 1;
    reference(&mut bytes, 28, 12);
    reference(&mut bytes, 39, 11);
    bytes[54..58].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 58, 13);
    bytes[73..77].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 77, 99);
    bytes[92..96].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 96, 15);
    bytes[107..111].copy_from_slice(&36u32.to_le_bytes());
    for (ordinal, byte) in b"00000000-0000-0000-0000-000000000000".iter().enumerate() {
        bytes[111 + ordinal * 2] = *byte;
    }
    bytes[186..190].copy_from_slice(&6u32.to_le_bytes());

    let operation = exact_ruled_surface_operation(&bytes, 0, 366, 186, &[11, 12, 13, 14, 15, 16])
        .expect("exact SurfaceRuled operation");
    assert_eq!(operation.method, DesignRuledSurfaceMethod::Normal);
    assert_eq!(operation.method_offset, 20);
    assert_eq!(operation.corner, DesignRuledSurfaceCorner::Rounded);
    assert_eq!(operation.corner_offset, 50);
    assert!(operation.alternate_face);
    assert_eq!(operation.alternate_face_offset, 27);
    assert_eq!(operation.angle_owner_record_index, 12);
    assert_eq!(operation.distance_owner_record_index, 11);
    assert_eq!(operation.edge_group_record_indices, [13, 15]);
    assert_eq!(operation.auxiliary_record_indices, [99]);
    assert_eq!(operation.direction_entity_id, None);

    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    for (ordinal, byte) in b"01234567-89ab-cdef-0123-456789abcdef".iter().enumerate() {
        bytes[111 + ordinal * 2] = *byte;
    }
    let operation = exact_ruled_surface_operation(&bytes, 0, 366, 186, &[11, 12, 13, 14, 15, 16])
        .expect("directed SurfaceRuled operation");
    assert_eq!(operation.method, DesignRuledSurfaceMethod::Direction);
    assert_eq!(
        operation.direction_entity_id.as_deref(),
        Some("01234567-89ab-cdef-0123-456789abcdef")
    );
}

#[test]
fn surface_stitch_tolerance_uses_its_fixed_scope_owned_frame() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    let mut bytes = Vec::new();
    header(&mut bytes, *b"308", 300);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&[1, 1, 0, 0, 0]);
    bytes.push(1);
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);
    bytes.extend_from_slice(&0.01f64.to_le_bytes());
    bytes.resize(104, 0);
    header(&mut bytes, *b"258", 300);
    bytes.extend_from_slice(&[0; 20]);
    header(&mut bytes, *b"331", 301);
    bytes.extend_from_slice(&[0; 20]);
    header(&mut bytes, *b"258", 301);

    assert_eq!(
        exact_surface_stitch_operation(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            12,
            &[100, 200, 300, 301]
        ),
        Some(DesignSurfaceStitchOperation {
            gap_tolerance: 0.01,
            gap_tolerance_offset: 40,
            tolerance_record_index: 300,
            settings_record_index: 301,
        })
    );
}

#[test]
fn extrude_operand_identity_walks_shared_wrapper_grammar_to_a_fixed_leaf() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    let group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#100".into(),
        scope_record_index: 12,
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 1000,
        class_tag: "332".into(),
        members: vec![200],
        lost_edge_references: Vec::new(),
        member_offsets: vec![1026],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 1021,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![300],
            trailing_record_offsets: vec![1043],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 180,
            opaque_index_offset: 1071,
            opaque_scalar: 0.125,
            opaque_scalar_offset: 1075,
            variant: false,
        },
        role: 0x0000_0008_0000_0000,
        extrude_role: Some(DesignExtrudeOperandRole::Bodies),
        extrude_face_role: None,
        role_offset: 1053,

        paired_class_tag: "259".into(),
        paired_byte_offset: 1124,
    };
    let wrapper_header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#300".into(),
        byte_offset: 0,
        class_tag: "326".into(),
        record_index: 300,
    };
    let mut bytes = Vec::new();
    header(&mut bytes, *b"326", 300);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 1, 0]);
    header(&mut bytes, *b"326", 305);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 1, 0]);
    header(&mut bytes, *b"324", 400);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&586u64.to_le_bytes());
    lp_utf16(&mut bytes, "df9087bd-02a6-4a3f-a132-7e69990f323c");
    lp_utf16(&mut bytes, "0b2382d1-caaf-4eb9-b40d-a6322a7ed829");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    header(&mut bytes, *b"301", 900);

    let identity = parse_construction_operand_identity(&bytes, &group, &wrapper_header)
        .expect("identity chain");
    assert_eq!(identity.wrapper_record_indices, [300, 305]);
    assert_eq!(identity.wrapper_byte_offsets, [0, 24]);
    assert_eq!(identity.following_record_index, 400);
    assert_eq!(identity.following_byte_offset, 48);
    let persistent = identity
        .persistent_identity
        .as_ref()
        .expect("fixed persistent identity leaf");
    assert_eq!(persistent.local_id, 586);
    assert_eq!(persistent.next_record_index, 900);
    assert_eq!(persistent.next_byte_offset, 238);

    let mut expanded_bytes = bytes[..233].to_vec();
    expanded_bytes.extend_from_slice(&[0; 4]);
    expanded_bytes.push(1);
    expanded_bytes.extend_from_slice(&900u32.to_le_bytes());
    expanded_bytes.extend_from_slice(&[0; 6]);
    header(&mut expanded_bytes, *b"301", 900);
    let expanded = parse_construction_operand_identity(&expanded_bytes, &group, &wrapper_header)
        .expect("identity chain with expanded tail reference");
    let persistent = expanded
        .persistent_identity
        .expect("expanded persistent identity leaf");
    assert_eq!(persistent.tail_slot_offset, 233);
    assert_eq!(persistent.next_record_index, 900);
    assert_eq!(persistent.next_byte_offset, 248);

    let mut bound_group = group;
    let mut terminating_identity = identity;
    terminating_identity.id =
        "f3d:Design/BulkStream.dat:design-construction-operand-identity#200".into();
    terminating_identity.wrapper_byte_offsets[0] = 200;
    bind_lost_edge_groups(
        std::slice::from_mut(&mut bound_group),
        std::slice::from_ref(&terminating_identity),
        &[LostEdgeReference {
            id: "f3d:Design/BulkStream.dat:lost-edge-reference#152".into(),
            record_byte_offset: 152,
            class_tag_offset: 156,
            class_tag: "419".into(),
            record_index: 299,
            record_index_offset: 159,
            byte_offset: 181,
            next_byte_offset: 200,
            next_class_tag: "326".into(),
            next_record_index: 300,
        }],
    )
    .expect("lost-edge run terminates at the group identity");
    assert_eq!(
        bound_group.lost_edge_references,
        ["f3d:Design/BulkStream.dat:lost-edge-reference#152"]
    );
}

#[test]
fn nested_entity_selection_member_retains_compact_and_expanded_identities() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    let group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
        scope_record_index: 80,
        scope_reference_ordinal: 0,
        record_index: 90,
        byte_offset: 900,
        class_tag: "269".into(),
        members: vec![100],
        lost_edge_references: Vec::new(),
        member_offsets: vec![926],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 921,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![200],
            trailing_record_offsets: vec![943],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 971,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 975,
            variant: false,
        },
        role: 0x0000_0005_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 953,

        paired_class_tag: "265".into(),
        paired_byte_offset: 1024,
    };
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "333".into(),
        record_index: 100,
    };
    let mut bytes = Vec::new();
    header(&mut bytes, *b"333", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&103u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "53aa8ab4-194a-434b-bd52-8c6d761dc147");
    lp_utf16(&mut bytes, "8e685642-4d68-4909-96d0-0dd4437491b6");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&[1, 0, 0]);
    header(&mut bytes, *b"265", 100);
    header(&mut bytes, *b"301", 101);
    header(&mut bytes, *b"446", 102);
    let identity_at = bytes.len();
    header(&mut bytes, *b"429", 103);
    bytes.extend_from_slice(&[0; 18]);
    bytes.extend_from_slice(&1331u64.to_le_bytes());
    bytes.extend_from_slice(&183u64.to_le_bytes());
    let next_at = bytes.len();
    header(&mut bytes, *b"311", 104);

    let operand = parse_entity_selection_operand(&bytes, &group, 0, &record)
        .expect("nested entity-selection frame");
    assert_eq!(operand.primary_identity, 1331);
    assert_eq!(operand.secondary_identity, Some(183));
    assert_eq!(operand.identity_record_offset, identity_at as u64);
    assert_eq!(operand.next_byte_offset, next_at as u64);

    let mut compact = bytes[..identity_at].to_vec();
    header(&mut compact, *b"429", 103);
    compact.extend_from_slice(&[0; 10]);
    compact.extend_from_slice(&1331u64.to_le_bytes());
    let compact_next_at = compact.len();
    header(&mut compact, *b"311", 109);
    let compact_operand = parse_entity_selection_operand(&compact, &group, 0, &record)
        .expect("compact nested entity-selection frame");
    assert_eq!(compact_operand.primary_identity, 1331);
    assert_eq!(compact_operand.secondary_identity, None);
    assert_eq!(compact_operand.identity_record_offset, identity_at as u64);
    assert_eq!(compact_operand.next_record_index, 109);
    assert_eq!(compact_operand.next_byte_offset, compact_next_at as u64);
}

#[test]
fn body_recipe_operand_decodes_counted_reference_table() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    let group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
        scope_record_index: 80,
        scope_reference_ordinal: 0,
        record_index: 90,
        byte_offset: 900,
        class_tag: "269".into(),
        members: vec![100],
        lost_edge_references: Vec::new(),
        member_offsets: vec![926],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 921,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![200],
            trailing_record_offsets: vec![943],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 971,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 975,
            variant: false,
        },
        role: 0x0000_0005_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 953,

        paired_class_tag: "265".into(),
        paired_byte_offset: 1024,
    };
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "365".into(),
        record_index: 100,
    };
    let mut bytes = Vec::new();
    header(&mut bytes, *b"365", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&2265u64.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&2266u64.to_le_bytes());
    bytes.extend_from_slice(&32u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&103u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "53aa8ab4-194a-434b-bd52-8c6d761dc147");
    lp_utf16(&mut bytes, "8e685642-4d68-4909-96d0-0dd4437491b6");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    header(&mut bytes, *b"259", 100);
    header(&mut bytes, *b"283", 101);
    header(&mut bytes, *b"463", 102);
    header(&mut bytes, *b"452", 103);
    let recipe_at = bytes.len();
    bytes.extend_from_slice(b"body_recipe_data");
    let next_at = bytes.len();
    header(&mut bytes, *b"311", 104);
    let recipe = ConstructionRecipe {
        id: format!("f3d:Design/BulkStream.dat:construction-recipe#{recipe_at}"),
        byte_offset: recipe_at as u64,
        record_index_offset: None,
        kind: ConstructionRecipeKind::Body,
        design_id: Some("2265".into()),
        design_id_offset: None,
        design_selector: Some(crate::records::ConstructionRecipeSelector {
            value: 1,
            byte_offset: 0,
        }),
        recipe_index: 0,
        record_index: 0,
    };

    let mut operand = parse_body_recipe_operand(&bytes, &group, 0, &record, &recipe)
        .expect("body recipe operand");
    assert_eq!(operand.references.len(), 2);
    assert_eq!(operand.references[0].design_reference, 2265);
    assert_eq!(operand.references[0].form, 3);
    assert_eq!(operand.references[1].design_reference, 2266);
    assert_eq!(operand.references[1].form, 32);
    assert_eq!(
        operand.owner,
        crate::records::DesignBodyRecipeOperandOwner::Group {
            group_record_index: 90,
            group_member_ordinal: 0,
        }
    );
    assert_eq!(operand.nested_record_index, 103);
    assert_eq!(operand.recipe_id, recipe.id);
    assert_eq!(operand.next_byte_offset, next_at as u64);
    operand.id = "f3d:Design/BulkStream.dat:body-recipe-operand#0".into();
    crate::design::decode::operands::bind_body_recipe_operand_candidates(
        std::slice::from_mut(&mut operand),
        std::slice::from_ref(&recipe),
        &[
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(FaceId("same-stream".into())),
                selector: 1,
                token: String::new(),
                design_references: vec![2265],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#2".into(),
                target: AttributeTarget::Face(FaceId("other-selector".into())),
                selector: 2,
                token: String::new(),
                design_references: vec![2265, 2266],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:xref/Other/occurrence-0/design:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(FaceId("other-stream".into())),
                selector: 0,
                token: String::new(),
                design_references: vec![2265],
                ordinal: 0,
            },
        ],
    );
    assert_eq!(
        operand.references[0].candidate_faces,
        [FaceId("same-stream".into())]
    );

    let mut nested = Vec::new();
    header(&mut nested, *b"302", 1);
    header(&mut nested, *b"305", 11);
    bytes.splice(next_at..next_at, nested.iter().copied());
    let operand = parse_body_recipe_operand(&bytes, &group, 0, &record, &recipe)
        .expect("body recipe operand with nested recipe records");
    assert_eq!(operand.next_byte_offset, (next_at + nested.len()) as u64);
}

#[test]
fn extrude_selection_group_and_members_have_exact_counted_frames() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    let scope = DesignParameterScope {
        id: "f3d:Design/BulkStream.dat:scope#12".into(),
        byte_offset: 1000,
        class_tag: "301".into(),
        record_index: 12,
        frame_length: 200,
        kind: "Extrude".into(),
        kind_offset: 1100,
        extrude_prologue: None,
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: None,
        history_state_id_offset: 0,
        previous_history_state_id: None,
        previous_history_state_id_offset: 0,
        reference_count_offset: 1080,
        reference_members: vec![100],
        reference_member_offsets: vec![1085],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        unclosed_construction_operand_groups: Vec::new(),
        work_point_reference_type: None,
        work_point_input_record_indices: Vec::new(),
        extrude_profile: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: 1200,
    };
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "331".into(),
        record_index: 100,
    };
    let mut group_bytes = Vec::new();
    header(&mut group_bytes, *b"331", 100);
    group_bytes.extend_from_slice(&[0; 10]);
    group_bytes.push(1);
    group_bytes.extend_from_slice(&12u32.to_le_bytes());
    group_bytes.extend_from_slice(&[0; 6]);
    group_bytes.extend_from_slice(&2u32.to_le_bytes());
    for member in [200u32, 201] {
        group_bytes.push(1);
        group_bytes.extend_from_slice(&member.to_le_bytes());
        group_bytes.extend_from_slice(&[0; 6]);
    }
    group_bytes.extend_from_slice(&180u32.to_le_bytes());
    group_bytes.extend_from_slice(&0.25f64.to_le_bytes());
    group_bytes.extend_from_slice(&180u32.to_le_bytes());
    group_bytes.push(1);
    group_bytes.extend_from_slice(&102u32.to_le_bytes());
    group_bytes.extend_from_slice(&[0; 6]);
    group_bytes.extend_from_slice(&[1, 1, 0, 1]);
    group_bytes.extend_from_slice(&101u32.to_le_bytes());
    group_bytes.extend_from_slice(&[0; 7]);
    group_bytes.push(1);
    group_bytes.extend_from_slice(&12u32.to_le_bytes());
    group_bytes.extend_from_slice(&[0; 6]);
    let paired_at = group_bytes.len();
    header(&mut group_bytes, *b"259", 100);

    let mut group = parse_extrude_selection_group(&group_bytes, &scope, 0, &record)
        .expect("counted Extrude selection group");
    assert_eq!(group.members, [200, 201]);
    assert_eq!(group.opaque_index, 180);
    assert_eq!(group.opaque_scalar, 0.25);
    assert!(group.variant);
    assert_eq!(group.paired_byte_offset, paired_at as u64);

    let member_record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#200".into(),
        byte_offset: 0,
        class_tag: "290".into(),
        record_index: 200,
    };
    let mut member_bytes = Vec::new();
    header(&mut member_bytes, *b"290", 200);
    member_bytes.extend_from_slice(&[0; 10]);
    member_bytes.extend_from_slice(&586u64.to_le_bytes());
    lp_utf16(&mut member_bytes, "df9087bd-02a6-4a3f-a132-7e69990f323c");
    lp_utf16(&mut member_bytes, "0b2382d1-caaf-4eb9-b40d-a6322a7ed829");
    member_bytes.extend_from_slice(&2u32.to_le_bytes());
    member_bytes.extend_from_slice(&[0; 5]);
    header(&mut member_bytes, *b"290", 201);

    let mut member = parse_extrude_selection_member(&member_bytes, &group, 0, &member_record)
        .expect("fixed Extrude selection member");
    assert_eq!(member.local_id, 586);
    assert_eq!(member.next_byte_offset, 190);
    assert_eq!(member.next_record_index, 201);
    assert!(!member.tail_slot_present);
    assert_eq!(member.tail_slot_offset, 185);

    member_bytes[185] = 1;
    let member_with_slot = parse_extrude_selection_member(&member_bytes, &group, 0, &member_record)
        .expect("Extrude selection member with present tail slot");
    assert!(member_with_slot.tail_slot_present);
    assert_eq!(member_with_slot.tail_slot_offset, 185);

    let terminal_member =
        parse_extrude_selection_member(&member_bytes[..190], &group, 0, &member_record)
            .expect("terminal fixed Extrude selection member");
    assert_eq!(terminal_member.next_byte_offset, 190);
    assert_eq!(terminal_member.next_record_index, 0);

    let mut edge_identity_bytes = Vec::new();
    header(&mut edge_identity_bytes, *b"278", 5887);
    edge_identity_bytes.extend_from_slice(&[0; 12]);
    edge_identity_bytes.push(1);
    edge_identity_bytes.extend_from_slice(&5890u32.to_le_bytes());
    edge_identity_bytes.extend_from_slice(&[0; 6]);
    edge_identity_bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(
        &mut edge_identity_bytes,
        "ad3001bb-a0fc-44c2-9b7a-c8b8fb70bfc0",
    );
    lp_utf16(
        &mut edge_identity_bytes,
        "1d8b67fc-c638-4af3-b13d-776dce4f472d",
    );
    let edge_identity =
        crate::design::decode::operands::parse_edge_identity_member(&edge_identity_bytes, 0)
            .expect("fixed edge-treatment selection identity");
    assert_eq!(edge_identity.local_id, 5890);
    assert!(!edge_identity.compact_layout);
    assert_eq!(edge_identity.local_id_offset, 24);
    assert_eq!(edge_identity.asset_id_offset, 42);
    assert_eq!(edge_identity.context_id_offset, 118);

    edge_identity_bytes.remove(22);
    let compact_edge_identity =
        crate::design::decode::operands::parse_edge_identity_member(&edge_identity_bytes, 0)
            .expect("compact fixed edge-treatment selection identity");
    assert!(compact_edge_identity.compact_layout);
    assert_eq!(compact_edge_identity.local_id, 5890);
    assert_eq!(compact_edge_identity.local_id_offset, 23);
    assert_eq!(compact_edge_identity.asset_id_offset, 41);
    assert_eq!(compact_edge_identity.context_id_offset, 117);

    edge_identity_bytes.remove(21);
    let shortest_edge_identity =
        crate::design::decode::operands::parse_edge_identity_member(&edge_identity_bytes, 0)
            .expect("short compact edge-treatment selection identity");
    assert!(shortest_edge_identity.compact_layout);
    assert_eq!(shortest_edge_identity.local_id, 5890);
    assert_eq!(shortest_edge_identity.local_id_offset, 22);
    assert_eq!(shortest_edge_identity.asset_id_offset, 40);
    assert_eq!(shortest_edge_identity.context_id_offset, 116);

    group.id = "f3d:Design/BulkStream.dat:selection-group#100".into();
    member.id = "f3d:Design/BulkStream.dat:selection-member#200".into();
    let identity = DesignConstructionOperandIdentity {
        id: "f3d:Design/BulkStream.dat:operand-identity#50".into(),
        group_record_index: 50,
        wrapper_record_indices: vec![150],
        wrapper_byte_offsets: vec![50],
        wrapper_class_tags: vec!["289".into()],
        following_record_index: 200,
        following_byte_offset: 0,
        following_class_tag: "290".into(),
        tracking_path: None,
        persistent_identity: Some(DesignConstructionPersistentIdentity {
            local_id: 586,
            local_id_offset: 21,
            asset_id: "df9087bd-02a6-4a3f-a132-7e69990f323c".into(),
            asset_id_offset: 33,
            context_id: "0b2382d1-caaf-4eb9-b40d-a6322a7ed829".into(),
            context_id_offset: 113,
            tail_slot_present: false,
            tail_slot_offset: 185,
            next_record_index: 201,
            next_byte_offset: 190,
        }),
    };
    bind_extrude_selection_identities(
        std::slice::from_mut(&mut member),
        std::slice::from_ref(&identity),
    );
    assert_eq!(member.operand_identity_ids, [identity.id]);
    let mut owning_scope = scope;
    owning_scope.extrude_profile = Some(DesignSketchProfileOperand {
        scope_reference_ordinal: 1,
        record_index: 300,
        byte_offset: 3000,
        class_tag: "308".into(),
        asset_id: "df9087bd-02a6-4a3f-a132-7e69990f323c".into(),
        asset_id_offset: 3040,
        entity_id: "0_172".into(),
        entity_suffix: 172,
        entity_reference_offset: 3120,
        paired_class_tag: "259".into(),
        paired_byte_offset: 3200,
    });
    let curve = SketchCurveIdentity {
        id: "f3d:Design/BulkStream.dat:sketch-curve#400".into(),
        record_index: 400,
        owner_reference: Some(172),
        class_tag: "270".into(),
        byte_offset: 4000,
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: 586,
        secondary_id: 0,
        geometry: None,
    };
    bind_extrude_selection_geometry(
        std::slice::from_mut(&mut member),
        std::slice::from_ref(&group),
        std::slice::from_ref(&owning_scope),
        &[],
        &[curve],
    );
    assert!(matches!(
        member.resolved_geometry,
        Some(SketchRelationOperand::Curve {
            record_index: 400,
            primary_id: 586,
            secondary_id: 0,
        })
    ));

    group.members.truncate(1);
    let sketch_id = SketchId("f3d:model:sketch#172".into());
    let sketch = Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![vec![SketchEntityUse {
            entity: neutral_sketch_curve_id(&sketch_id, 586, 0),
            reversed: false,
        }]],
        native_ref: None,
    };
    assert!(matches!(
        resolved_extrude_profile_selection(
            &sketch_id,
            &group,
            std::slice::from_ref(&member),
            &sketch,
            crate::design::profile_select::ExtrudeProfileResolution {
                entities: &[],
                spatial_sketches: &[],
                spatial_entities: &[],
                histories: &[],
                linear_tolerance: 1.0e-6,
                angular_tolerance: 1.0e-9,
            },
            None,
            None,
        ),
        cadmpeg_ir::features::ProfileRef::SketchProfiles {
            sketch: ref actual_sketch,
            ref profiles,
        } if actual_sketch == &sketch_id && profiles == &[0]
    ));
    let mut point_member = member.clone();
    point_member.id = "f3d:Design/BulkStream.dat:selection-member#201".into();
    point_member.record_index = 201;
    point_member.group_member_ordinal = 1;
    point_member.local_id = 587;
    point_member.resolved_geometry = Some(SketchRelationOperand::Point {
        record_index: 401,
        persistent_id: 587,
    });
    group.members.push(201);
    let mut sketch = sketch;
    let second_profile_id = SketchEntityId("second-profile".into());
    sketch.profiles.push(vec![SketchEntityUse {
        entity: second_profile_id.clone(),
        reversed: false,
    }]);
    let point_entity = SketchEntity {
        id: neutral_sketch_point_id(&sketch_id, 587),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(0.5, 1.0),
        },
    };
    let line_entity = SketchEntity {
        id: neutral_sketch_curve_id(&sketch_id, 586, 0),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    };
    let second_profile_entity = SketchEntity {
        id: second_profile_id,
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 1.0),
            end: Point2::new(1.0, 1.0),
        },
    };
    let profile_entities = [line_entity, second_profile_entity, point_entity];
    assert!(matches!(
        resolved_extrude_profile_selection(
            &sketch_id,
            &group,
            &[member.clone(), point_member],
            &sketch,
            crate::design::profile_select::ExtrudeProfileResolution {
                entities: &profile_entities,
                spatial_sketches: &[],
                spatial_entities: &[],
                histories: &[],
                linear_tolerance: 1.0e-6,
                angular_tolerance: 1.0e-9,
            },
            None,
            None,
        ),
        cadmpeg_ir::features::ProfileRef::SketchProfiles {
            sketch: ref actual_sketch,
            ref profiles,
        } if actual_sketch == &sketch_id && profiles == &[0, 1]
    ));
    member.resolved_geometry = None;
    assert!(matches!(
        resolved_extrude_profile_selection(
            &sketch_id,
            &group,
            std::slice::from_ref(&member),
            &sketch,
            crate::design::profile_select::ExtrudeProfileResolution {
                entities: &[],
                spatial_sketches: &[],
                spatial_entities: &[],
                histories: &[],
                linear_tolerance: 1.0e-6,
                angular_tolerance: 1.0e-9,
            },
            None,
            None,
        ),
        cadmpeg_ir::features::ProfileRef::SketchSelection {
            sketch: ref actual_sketch,
            selections: ref actual_selections,
        } if actual_sketch == &sketch_id && actual_selections == &[group.id.clone()]
    ));
    let mut single_profile_sketch = sketch.clone();
    single_profile_sketch.profiles.truncate(1);
    assert!(matches!(
        resolved_extrude_profile_selection(
            &sketch_id,
            &group,
            std::slice::from_ref(&member),
            &single_profile_sketch,
            crate::design::profile_select::ExtrudeProfileResolution {
                entities: &[],
                spatial_sketches: &[],
                spatial_entities: &[],
                histories: &[],
                linear_tolerance: 1.0e-6,
                angular_tolerance: 1.0e-9,
            },
            None,
            None,
        ),
        cadmpeg_ir::features::ProfileRef::SketchProfiles {
            sketch: ref actual_sketch,
            ref profiles,
        } if actual_sketch == &sketch_id && profiles == &[0]
    ));
}

#[test]
fn topology_operands_follow_consecutive_nested_records_to_their_recipes() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) -> u64 {
        let offset = u64::try_from(bytes.len()).expect("generated frame length fits u64");
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        offset
    }

    let mut bytes = Vec::new();
    header(&mut bytes, *b"306", 100);
    let paired_at = header(&mut bytes, *b"259", 100);
    header(&mut bytes, *b"408", 101);
    header(&mut bytes, *b"414", 102);
    let recipe_record_at = header(&mut bytes, *b"423", 103);
    // A recipe prefix can contain header-shaped scalar bytes. Only the exact
    // N+4 record closes the N through N+3 operand envelope.
    header(&mut bytes, *b"122", 0);
    let recipe_name_at = bytes.len() + 4;
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(b"edge_recipe_data");
    for value in [-1i32, -1, 2, 0, -1, 1, -1, 7] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let next_at = header(&mut bytes, *b"306", 104);
    let scope = DesignParameterScope {
        id: "f3d:Design/BulkStream.dat:scope#1".into(),
        byte_offset: 1000,
        class_tag: "301".into(),
        record_index: 1,
        frame_length: 200,
        kind: "Fillet".into(),
        kind_offset: 1100,
        extrude_prologue: None,
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: None,
        history_state_id_offset: 0,
        previous_history_state_id: None,
        previous_history_state_id_offset: 0,
        reference_count_offset: 1080,
        reference_members: vec![100],
        reference_member_offsets: vec![1085],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        unclosed_construction_operand_groups: Vec::new(),
        work_point_reference_type: None,
        work_point_input_record_indices: Vec::new(),
        extrude_profile: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: 1200,
    };
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "306".into(),
        record_index: 100,
    };
    let recipe = ConstructionRecipe {
        id: "f3d:Design/BulkStream.dat:construction-recipe#60".into(),
        byte_offset: recipe_name_at as u64,
        record_index_offset: Some(recipe_record_at + 8),
        kind: ConstructionRecipeKind::Edge,
        design_id: None,
        design_id_offset: None,
        design_selector: None,
        recipe_index: 7,
        record_index: 303,
    };

    let mut edge_operand =
        parse_edge_operand(&bytes, &scope, 0, &record, std::slice::from_ref(&recipe))
            .expect("edge recipe operand");
    assert_eq!(edge_operand.record_index, 100);
    assert_eq!(edge_operand.paired_byte_offset, paired_at);
    assert_eq!(edge_operand.recipe_record_index, 103);
    assert_eq!(edge_operand.recipe_record_byte_offset, recipe_record_at);
    assert_eq!(edge_operand.recipe_id, recipe.id);
    assert_eq!(edge_operand.resolved_edge_slot, None);
    edge_operand.terminal_reference_edge_slots = vec![vec![17], vec![18, 19]];
    assert_eq!(
        crate::design::edge_resolve::edge_operand_reference_edge_sets(&edge_operand),
        vec![&[17][..], &[18, 19][..]]
    );
    let reference_context = |reference_ordinal, changed_reference_edge_slots| {
        crate::records::DesignEdgeRecipeReferenceContext {
            reference_ordinal,
            result_faces: Vec::new(),
            result_face_boundaries: Vec::new(),
            result_shared_edge_slots: Vec::new(),
            preceding_faces: Vec::new(),
            preceding_face_boundaries: Vec::new(),
            preceding_support_face_slots: Vec::new(),
            preceding_support_face_boundaries: Vec::new(),
            shared_edge_slots: Vec::new(),
            changed_shared_edge_slots: Vec::new(),
            changed_reference_edge_slots,
        }
    };
    edge_operand.recipe_reference_contexts = vec![
        reference_context(0, vec![17]),
        reference_context(1, vec![18, 19]),
    ];
    edge_operand.local_topology_references = Some(vec![
        std::num::NonZeroU32::new(2).unwrap(),
        std::num::NonZeroU32::new(1).unwrap(),
        std::num::NonZeroU32::new(2).unwrap(),
    ]);
    assert_eq!(
        crate::design::edge_resolve::edge_operand_reference_edge_sets(&edge_operand),
        vec![&[18, 19][..], &[17][..], &[18, 19][..]]
    );
    edge_operand.recipe_reference_contexts = vec![
        reference_context(0, Vec::new()),
        reference_context(1, vec![17]),
    ];
    let mut second_changed_operand = edge_operand.clone();
    second_changed_operand.recipe_reference_contexts = vec![
        reference_context(0, Vec::new()),
        reference_context(1, vec![18]),
    ];
    assert_eq!(
        crate::design::edge_resolve::changed_reference_edge_group_candidates(&[
            &edge_operand,
            &second_changed_operand,
        ]),
        Some(vec![17, 18])
    );
    second_changed_operand.recipe_reference_contexts[0].changed_reference_edge_slots = vec![17];
    assert_eq!(
        crate::design::edge_resolve::changed_reference_edge_group_candidates(&[
            &edge_operand,
            &second_changed_operand,
        ]),
        None
    );
    edge_operand.recipe_reference_contexts.clear();
    edge_operand.local_topology_references = None;
    edge_operand.terminal_reference_edge_slots.clear();
    edge_operand.resolved_edge_slot = Some(17);
    assert_eq!(
        crate::design::edge_resolve::resolved_edge_operand(&edge_operand),
        Some(17)
    );
    edge_operand.resolved_edge_slot = None;
    edge_operand.changed_boundary_edge_slots = vec![17, 18];
    edge_operand.deleted_boundary_edge_slots = vec![17, 18];
    edge_operand.treatment_radius_candidates = vec![
        crate::records::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 17,
            radius: 3.0,
        },
        crate::records::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 18,
            radius: 3.0,
        },
    ];
    let second_operand = edge_operand.clone();
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&edge_operand, &second_operand],
            3.0
        ),
        Some(vec![17, 18])
    );
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&edge_operand, &second_operand],
            4.0
        ),
        None
    );
    let mut chain_left = edge_operand.clone();
    chain_left.treatment_radius_candidates.push(
        crate::records::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 19,
            radius: 3.0,
        },
    );
    let mut chain_right = edge_operand.clone();
    chain_right.treatment_radius_candidates = vec![
        crate::records::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 19,
            radius: 3.0,
        },
        crate::records::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 20,
            radius: 3.0,
        },
    ];
    chain_right.deleted_boundary_edge_slots = vec![19, 20];
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&chain_left, &chain_right],
            3.0
        ),
        Some(vec![17, 18, 19, 20])
    );
    let mut context_operand = edge_operand.clone();
    context_operand.treatment_radius_candidates.clear();
    context_operand.changed_boundary_edge_slots = vec![16, 17];
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&edge_operand, &context_operand],
            3.0
        ),
        Some(vec![17, 18])
    );
    context_operand.changed_boundary_edge_slots = vec![15, 16];
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&edge_operand, &context_operand],
            3.0
        ),
        None
    );
    let mut resolved_operand = edge_operand.clone();
    resolved_operand.id = "resolved".into();
    resolved_operand.resolved_edge_slot = Some(17);
    let mut proven_operand = edge_operand.clone();
    proven_operand.resolved_edge_slot = Some(17);
    let recovered_group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
        scope_record_index: 1,
        scope_reference_ordinal: 0,
        record_index: 90,
        byte_offset: 900,
        class_tag: "288".into(),
        members: vec![100],
        lost_edge_references: vec!["f3d:Design/BulkStream.dat:lost-edge#1".into()],
        member_offsets: vec![926],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 921,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![91],
            trailing_record_offsets: vec![950],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 968,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 972,
            variant: false,
        },
        role: 0x0000_0008_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 960,

        paired_class_tag: "259".into(),
        paired_byte_offset: 1_000,
    };
    let recovered = crate::design::edge_resolve::resolved_edge_group(
        &recovered_group,
        std::slice::from_ref(&recovered_group),
        std::slice::from_ref(&proven_operand),
        &[],
        Some(8),
        &cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into()),
        None,
    );
    assert!(matches!(
        recovered,
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges == [cadmpeg_ir::ids::HistoricalEdgeId(
                "f3d:history-input:edge#6:fillet:8:17".into()
            )]
    ));
    let mut terminal_group = recovered_group.clone();
    terminal_group.lost_edge_references.clear();
    terminal_group.members = vec![100, 104];
    let mut terminal_resolved = proven_operand.clone();
    terminal_resolved.recipe_state_id = Some(8);
    let mut terminal_unresolved = proven_operand.clone();
    terminal_unresolved.id = "f3d:Design/BulkStream.dat:edge-operand#104".into();
    terminal_unresolved.record_index = 104;
    terminal_unresolved.recipe_state_id = Some(8);
    terminal_unresolved.resolved_edge_slot = None;
    terminal_unresolved.changed_boundary_edge_slots.clear();
    terminal_unresolved.deleted_boundary_edge_slots.clear();
    terminal_unresolved.treatment_radius_candidates.clear();
    terminal_unresolved.recipe_selectors = vec![crate::records::DesignEdgeRecipeSelectorContext {
        selector: 0,
        clause_entries: vec![None, None],
        clause_triplet_edge_slots: vec![None, None],
        incidence_matching_edge_slots: vec![18, 19],
        unique_incidence_edge_slot: None,
        boundary_count_matching_edge_slots: vec![18, 19],
    }];
    let terminal = crate::design::edge_resolve::resolved_edge_group(
        &terminal_group,
        std::slice::from_ref(&terminal_group),
        &[terminal_resolved, terminal_unresolved.clone()],
        &[],
        None,
        &cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into()),
        None,
    );
    assert!(
        matches!(
        terminal,
        cadmpeg_ir::features::EdgeSelection::HistoricalPartial {
            ref edges,
            ref unresolved,
            ..
        } if edges == &[cadmpeg_ir::ids::HistoricalEdgeId(
            "f3d:history-input:edge#6:fillet:8:17".into()
        )] && unresolved == &["f3d:Design/BulkStream.dat:edge-operand#104"]
        ),
        "{terminal:?}"
    );
    let identity = |record_index, ordinal, edge| DesignEdgeIdentityOperand {
        id: format!("f3d:Design/BulkStream.dat:edge-identity#{record_index}"),
        scope_record_index: 1,
        group_record_index: 90,
        group_member_ordinal: ordinal,
        record_index,
        byte_offset: u64::from(record_index),
        class_tag: "297".into(),
        compact_layout: false,
        local_id: u64::from(record_index),
        local_id_offset: 0,
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        historical_entity_kind: None,
        historical_entity_ref: None,
        historical_state_ids: Vec::new(),
        treatment_radius_candidates: Vec::new(),
        transition_edge_candidates: Vec::new(),
        resolved_edge_slots: Vec::new(),
        resolved_edge_slot: edge,
        resolution_identity_id: None,
    };
    let mut recipe_unresolved = proven_operand.clone();
    recipe_unresolved.resolved_edge_slot = None;
    recipe_unresolved.recipe_state_id = Some(8);
    recipe_unresolved.changed_boundary_edge_slots.clear();
    let merged = crate::design::edge_resolve::resolved_edge_group(
        &terminal_group,
        std::slice::from_ref(&terminal_group),
        &[recipe_unresolved.clone(), terminal_unresolved.clone()],
        &[identity(100, 0, Some(17)), identity(104, 1, None)],
        Some(8),
        &cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into()),
        None,
    );
    assert!(matches!(
        merged,
        cadmpeg_ir::features::EdgeSelection::HistoricalPartial {
            ref edges,
            ref unresolved,
            ..
        } if edges == &[cadmpeg_ir::ids::HistoricalEdgeId(
            "f3d:history-input:edge#6:fillet:8:17".into()
        )] && unresolved == &["f3d:Design/BulkStream.dat:edge-operand#104"]
    ));
    let complete = crate::design::edge_resolve::resolved_edge_group(
        &terminal_group,
        std::slice::from_ref(&terminal_group),
        &[recipe_unresolved.clone(), terminal_unresolved],
        &[identity(100, 0, Some(17)), identity(104, 1, Some(18))],
        Some(8),
        &cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into()),
        None,
    );
    assert!(matches!(
        complete,
        cadmpeg_ir::features::EdgeSelection::Historical { ref edges, .. }
            if edges == &[
                cadmpeg_ir::ids::HistoricalEdgeId(
                    "f3d:history-input:edge#6:fillet:8:17".into()
                ),
                cadmpeg_ir::ids::HistoricalEdgeId(
                    "f3d:history-input:edge#6:fillet:8:18".into()
                ),
            ]
    ));
    let mut first_rule = identity(100, 0, None);
    first_rule.resolved_edge_slots = vec![17, 18];
    let mut second_rule = identity(104, 1, None);
    second_rule.resolved_edge_slots = vec![18, 19];
    let face_rules = crate::design::edge_resolve::resolved_edge_group(
        &terminal_group,
        std::slice::from_ref(&terminal_group),
        &[recipe_unresolved.clone()],
        &[first_rule, second_rule],
        Some(8),
        &cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into()),
        None,
    );
    assert!(matches!(
        face_rules,
        cadmpeg_ir::features::EdgeSelection::Historical { ref edges, .. }
            if edges == &[
                cadmpeg_ir::ids::HistoricalEdgeId(
                    "f3d:history-input:edge#6:fillet:8:17".into()
                ),
                cadmpeg_ir::ids::HistoricalEdgeId(
                    "f3d:history-input:edge#6:fillet:8:18".into()
                ),
                cadmpeg_ir::ids::HistoricalEdgeId(
                    "f3d:history-input:edge#6:fillet:8:19".into()
                ),
            ]
    ));
    let mut chain_group = terminal_group.clone();
    chain_group.members = vec![100];
    let mut chain_recipe = recipe_unresolved.clone();
    chain_recipe.changed_boundary_edge_slots = vec![17, 18];
    let mut chain_identity = identity(100, 0, None);
    chain_identity.transition_edge_candidates = vec![18, 17];
    let chain = crate::design::edge_resolve::resolved_edge_group(
        &chain_group,
        std::slice::from_ref(&chain_group),
        &[chain_recipe],
        &[chain_identity],
        Some(8),
        &cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into()),
        None,
    );
    assert!(matches!(
        chain,
        cadmpeg_ir::features::EdgeSelection::Historical { ref edges, .. }
            if edges == &[
                cadmpeg_ir::ids::HistoricalEdgeId(
                    "f3d:history-input:edge#6:fillet:8:17".into()
                ),
                cadmpeg_ir::ids::HistoricalEdgeId(
                    "f3d:history-input:edge#6:fillet:8:18".into()
                ),
            ]
    ));
    assert_eq!(
        edge_operand.recipe_program_offset,
        recipe_name_at as u64 + 16
    );
    assert_eq!(edge_operand.recipe_program, [-1, -1, 2, 0, -1, 1, -1, 7]);
    assert!(edge_operand.recipe_structure.is_none());
    let structured = crate::design::decode::operands::edge_recipe_structure(&[
        -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 0, -1, 2, -1, 1, -1, 0, 1, 1, 5, 4, 4, 4, 4, 3, 4, -1,
        3, 0, -1, 1, -1, 3, -1, 0, 1, 2, 5, 3, 3, 3, 1, 1, 1, -1,
    ])
    .expect("standard two-side recipe structure");
    assert_eq!(structured.root, 2);
    assert_eq!(structured.sides[0].field_count.get(), 3);
    assert_eq!(structured.sides[0].header_value, 0);
    assert_eq!(structured.sides[0].scalars, [2, 1]);
    assert_eq!(structured.sides[0].payload_entry_count, 1);
    assert_eq!(structured.sides[0].entries[0].selector, 1);
    assert_eq!(structured.sides[0].entries[0].boundary_edge_count.get(), 5);
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0]
            .outer
            .get(),
        4
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0].middle,
        4
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0].vertex_ordinal,
        3
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0].incident_edge_ordinal,
        Some(3)
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0].incident_side,
        Some(crate::records::DesignTopologyIncidentSide::Following)
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[1]
            .outer
            .get(),
        4
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[1].middle,
        3
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[1].incident_edge_ordinal,
        Some(2)
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[1].incident_side,
        Some(crate::records::DesignTopologyIncidentSide::Preceding)
    );
    assert_eq!(structured.sides[1].field_count.get(), 3);
    assert_eq!(structured.sides[1].header_value, 0);
    assert_eq!(structured.sides[1].scalars, [1, 3]);
    assert_eq!(structured.sides[1].payload_entry_count, 1);
    assert_eq!(structured.sides[1].entries[0].selector, 2);
    assert_eq!(structured.sides[1].entries[0].boundary_edge_count.get(), 5);
    assert_eq!(
        structured.sides[1].entries[0].topology_triplets[0]
            .outer
            .get(),
        3
    );
    assert_eq!(
        structured.sides[1].entries[0].topology_triplets[0].middle,
        3
    );
    assert_eq!(
        structured.sides[1].entries[0].topology_triplets[1]
            .outer
            .get(),
        1
    );
    assert_eq!(
        structured.sides[1].entries[0].topology_triplets[1].middle,
        1
    );
    assert_eq!(
        crate::design::decode::operands::edge_recipe_local_topology_references(&structured, 3),
        Some(
            [2, 1, 1, 3]
                .into_iter()
                .map(|value| std::num::NonZeroU32::new(value).unwrap())
                .collect()
        )
    );
    assert!(
        crate::design::decode::operands::edge_recipe_local_topology_references(&structured, 2)
            .is_none()
    );
    let mut referenced_headers = structured.clone();
    referenced_headers.sides[0].header_value = 2;
    referenced_headers.sides[1].header_value = 3;
    assert_eq!(
        crate::design::decode::operands::edge_recipe_local_topology_references(
            &referenced_headers,
            3
        ),
        Some(
            [2, 2, 1, 3, 1, 3]
                .into_iter()
                .map(|value| std::num::NonZeroU32::new(value).unwrap())
                .collect()
        )
    );
    let wrap =
        crate::design::decode::operands::edge_recipe_entries(&[1, 5, 1, 0, 1, 1, 1, 1]).unwrap();
    assert_eq!(wrap[0].topology_triplets[0].vertex_ordinal, 0);
    assert_eq!(wrap[0].topology_triplets[0].incident_edge_ordinal, Some(4));
    assert_eq!(wrap[0].common_incident_edge_ordinal, None);
    assert_eq!(
        wrap[0].topology_triplets[0].incident_side,
        Some(crate::records::DesignTopologyIncidentSide::Preceding)
    );
    let common =
        crate::design::decode::operands::edge_recipe_entries(&[1, 5, 1, 1, 1, 1, 1, 1]).unwrap();
    assert_eq!(common[0].common_incident_edge_ordinal, Some(0));
    let underived =
        crate::design::decode::operands::edge_recipe_entries(&[0, 6, 6, 4, 6, 1, 1, 1]).unwrap();
    assert_eq!(underived[0].topology_triplets[0].vertex_ordinal, 5);
    assert_eq!(
        underived[0].topology_triplets[0].incident_edge_ordinal,
        None
    );
    assert_eq!(underived[0].topology_triplets[0].incident_side, None);
    assert_eq!(
        crate::design::decode::operands::edge_recipe_entries(&[3, 5, 1, 1, 1, 2, 1, 2]).unwrap()[0]
            .selector,
        3
    );
    assert!(
        crate::design::decode::operands::edge_recipe_entries(&[-1, 5, 1, 1, 1, 2, 1, 2]).is_none()
    );
    assert!(
        crate::design::decode::operands::edge_recipe_entries(&[1, 5, 6, 5, 6, 2, 1, 2]).is_none()
    );
    assert!(crate::design::decode::operands::edge_recipe_entries(&[
        1, 5, 1, 1, 1, 2, 1, 2, 1, 5, 2, 1, 2, 3, 2, 3,
    ])
    .is_none());
    assert!(crate::design::decode::operands::edge_recipe_entries(&[
        2, 5, 1, 1, 1, 2, 1, 2, 1, 5, 2, 1, 2, 3, 2, 3,
    ])
    .is_none());
    let extended = crate::design::decode::operands::edge_recipe_structure(&[
        -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 2, -1, 1, -1, 0, -1, 0, 0, -1, 4, 3, -1, 0, -1, 1, -1,
        4, -1, 0, 0, -1,
    ])
    .expect("recipe structure with a third scalar on its second side");
    assert_eq!(extended.sides[0].scalars, [1, 0]);
    assert_eq!(extended.sides[1].scalars, [0, 1, 4]);
    assert_eq!(extended.sides[1].field_count.get(), 4);
    assert!(extended.sides[0].entries.is_empty());
    assert!(extended.sides[1].entries.is_empty());
    let zero_delimited = crate::design::decode::operands::edge_recipe_structure(&[
        -1, -1, 2, 0, 0, 1, 0, 2, -1, 3, 1, 0, 0, 0, 2, 0, 0, 0, -1, 4, 1, 0, 3, 0, 4, 0, 0, 0, 0,
        1, 2, 3, 2, 1, 2, 1, 1, 1, -1,
    ])
    .expect("recipe structure with zero-delimited side fields");
    assert_eq!(zero_delimited.root, 2);
    assert_eq!(zero_delimited.sides[0].field_count.get(), 3);
    assert_eq!(zero_delimited.sides[0].header_value, 1);
    assert_eq!(zero_delimited.sides[0].scalars, [0, 2]);
    assert!(zero_delimited.sides[0].entries.is_empty());
    assert_eq!(zero_delimited.sides[1].field_count.get(), 4);
    assert_eq!(zero_delimited.sides[1].scalars, [3, 4, 0]);
    assert_eq!(zero_delimited.sides[1].entries.len(), 1);
    assert_eq!(zero_delimited.sides[1].entries[0].selector, 2);
    assert_eq!(
        zero_delimited.sides[1].entries[0].boundary_edge_count.get(),
        3
    );
    let mixed_delimiters = crate::design::decode::operands::edge_recipe_structure(&[
        -1, -1, 2, 0, 0, 1, -1, 2, -1, 3, 2, 0, 1, -1, 0, 0, 0, 0, -1, 3, 0, 0, 1, -1, 3, 0, 0, 0,
        -1,
    ])
    .expect("recipe structure with field-local delimiters");
    assert_eq!(mixed_delimiters.root, 2);
    assert_eq!(mixed_delimiters.sides[0].header_value, 2);
    assert_eq!(mixed_delimiters.sides[0].scalars, [1, 0]);
    assert_eq!(mixed_delimiters.sides[1].header_value, 0);
    assert_eq!(mixed_delimiters.sides[1].scalars, [1, 3]);
    let revolution_axis = crate::design::decode::operands::edge_recipe_structure(&[
        -1, -1, 2, 0, 0, 1, 0, 2, -1, 3, 0, 0, 2, -1, 1, 0, 0, 1, 1, 7, 1, 1, 1, 4, 4, 4, -1, 3, 0,
        0, 1, 0, 3, 0, 0, 0, 0,
    ])
    .expect("revolution-axis edge recipe structure");
    assert_eq!(revolution_axis.sides[0].scalars, [2, 1]);
    assert_eq!(revolution_axis.sides[0].entries.len(), 1);
    assert!(revolution_axis.sides[1].entries.is_empty());
    let variable_scalars = crate::design::decode::operands::edge_recipe_structure(&[
        -1, -1, 2, 0, -1, 1, -1, 2, -1, 5, 1, -1, 0, -1, 2, -1, 3, -1, 4, -1, 0, 0, -1, 3, 0, -1,
        1, -1, 2, -1, 0, 0, -1,
    ])
    .expect("recipe structure with four scalar fields");
    assert_eq!(variable_scalars.sides[0].field_count.get(), 5);
    assert_eq!(variable_scalars.sides[0].scalars, [0, 2, 3, 4]);
    let extended_payload = crate::design::decode::operands::edge_recipe_structure(&[
        -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 1, -1, 0, -1, 2, -1, 2, 3, -1, 0, 0, -1, 4, -1, 0, 0,
        -1, 1, 0, 4, 1, 1, 1, 2, 2, 2, -1, 3, 0, -1, 1, -1, 2, -1, 0, 0, -1,
    ])
    .expect("recipe structure with an extended payload field program");
    assert_eq!(
        extended_payload.sides[0].payload_prefix,
        [2, 3, -1, 0, 0, -1, 4, -1, 0, 0, -1]
    );
    assert_eq!(extended_payload.sides[0].entries.len(), 1);
    let face = crate::design::decode::operands::face_recipe_structure(&[
        0, -1, 1, -1, 2, -1, 3, 0, -1, 2, -1, 1, -1, 0, 0, -1, 3, 0, -1, 1, -1, 3, -1, 0, 0, -1,
    ])
    .expect("face node topology recipe structure");
    assert_eq!(face.root, 0);
    assert_eq!(face.prelude, [1, 2]);
    assert_eq!(face.sides[0].field_count.get(), 3);
    assert_eq!(face.sides[0].header_value, 0);
    assert_eq!(face.sides[0].scalars, [2, 1]);
    assert_eq!(face.sides[1].field_count.get(), 3);
    assert_eq!(face.sides[1].header_value, 0);
    assert_eq!(face.sides[1].scalars, [1, 3]);
    let zero_delimited_face = crate::design::decode::operands::face_recipe_structure(&[
        0, 0, 1, 0, 2, -1, 3, 0, 0, 2, 0, 1, 0, 0, 0, -1, 3, 0, 0, 1, 0, 3, 0, 0, 0, -1,
    ])
    .expect("zero-delimited face node topology recipe structure");
    assert_eq!(zero_delimited_face, face);
    assert_eq!(edge_operand.next_record_index, 104);
    assert_eq!(edge_operand.next_byte_offset, next_at);
    bind_edge_operand_candidates(
        std::slice::from_mut(&mut edge_operand),
        std::slice::from_ref(&recipe),
        &[
            PersistentSubentityTag {
                id: "f3d:asm:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(FaceId("f3d:brep:entity#50".into())),
                selector: 1,
                token: "3".into(),
                design_references: vec![303],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:xref/other/occurrence-0/design:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(FaceId("f3d:brep:entity#xref".into())),
                selector: 1,
                token: "3".into(),
                design_references: vec![303],
                ordinal: 0,
            },
        ],
    );
    assert_eq!(
        edge_operand.candidate_faces,
        [FaceId("f3d:brep:entity#50".into())]
    );
    let mut local_recipe = recipe.clone();
    local_recipe.record_index = -1335;
    bind_edge_operand_candidates(
        std::slice::from_mut(&mut edge_operand),
        std::slice::from_ref(&local_recipe),
        &[PersistentSubentityTag {
            id: "f3d:asm:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(FaceId("f3d:brep:entity#50".into())),
            selector: 1,
            token: "3".into(),
            design_references: vec![303],
            ordinal: 0,
        }],
    );
    assert!(edge_operand.candidate_faces.is_empty());
    let mut embedded_program = vec![99];
    embedded_program.extend_from_slice(&edge_operand.recipe_program[7..]);
    embedded_program.push(88);
    let dimension_recipe = DesignDimensionRecipeRecord {
        id: "f3d:Design/BulkStream.dat:dimension-recipe#1".into(),
        companion_record_index: 1,
        recipe_ordinal: 0,
        recipe_id: "recipe".into(),
        recipe_kind: ConstructionRecipeKind::Edge,
        byte_offset: 0,
        class_tag: "423".into(),
        record_index: 1,
        frame_length: 4,
        prefix_offset: 0,
        prefix_bytes: vec![1],
        references: Vec::new(),
        program_offset: 0,
        program: embedded_program,
        matching_edge_operand_ids: Vec::new(),
    };
    assert_eq!(
        crate::design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
            &dimension_recipe,
            std::slice::from_ref(&edge_operand),
        ),
        [edge_operand.id.clone()]
    );
    let mut other_stream_operand = edge_operand.clone();
    other_stream_operand.id = "f3d:Other/BulkStream.dat:edge-operand#100".into();
    assert_eq!(
        crate::design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
            &dimension_recipe,
            &[edge_operand.clone(), other_stream_operand],
        ),
        [edge_operand.id.clone()]
    );

    let mut face_bytes = Vec::new();
    header(&mut face_bytes, *b"306", 100);
    let face_paired_at = header(&mut face_bytes, *b"259", 100);
    header(&mut face_bytes, *b"408", 101);
    header(&mut face_bytes, *b"414", 102);
    let face_recipe_record_at = header(&mut face_bytes, *b"423", 103);
    let face_recipe_name_at = face_bytes.len() + 4;
    face_bytes.extend_from_slice(&24u32.to_le_bytes());
    face_bytes.extend_from_slice(b"bounded_face_recipe_data");
    for value in [0i32, -1, 4, -1, -1, 2, 7, -1, -1, 2, 8, -1, -1, 2, 9] {
        face_bytes.extend_from_slice(&value.to_le_bytes());
    }
    let face_next_at = header(&mut face_bytes, *b"306", 104);
    let mut face_scope = scope;
    face_scope.kind = "Extrude".into();
    let mut face_recipe = recipe;
    face_recipe.kind = ConstructionRecipeKind::BoundedFace;
    face_recipe.design_id = Some("303".into());
    face_recipe.byte_offset = face_recipe_name_at as u64;
    face_recipe.record_index_offset = Some(face_recipe_record_at + 8);
    let mut operand = parse_face_operand(
        &face_bytes,
        &face_scope,
        0,
        None,
        None,
        &record,
        std::slice::from_ref(&face_recipe),
    )
    .expect("face recipe operand");
    assert_eq!(operand.record_index, 100);
    assert_eq!(operand.paired_byte_offset, face_paired_at);
    assert_eq!(operand.recipe_record_index, 103);
    assert_eq!(operand.recipe_kind, ConstructionRecipeKind::BoundedFace);
    assert_eq!(operand.recipe_id, face_recipe.id);
    assert!(operand.resolved_face_slots.is_empty());
    assert_eq!(
        operand.recipe_program_offset,
        face_recipe_name_at as u64 + 24
    );
    assert_eq!(operand.recipe_program[0..3], [0, -1, 4]);
    let face_program_at = face_recipe_name_at + 24;
    face_bytes[face_program_at + 4..face_program_at + 8].copy_from_slice(&0i32.to_le_bytes());
    let zero_prelude = parse_face_operand(
        &face_bytes,
        &face_scope,
        0,
        None,
        None,
        &record,
        std::slice::from_ref(&face_recipe),
    )
    .expect("zero-prelude face recipe operand");
    assert_eq!(zero_prelude.recipe_program[0..3], [0, 0, 4]);
    assert_eq!(
        face_recipe_program_kind(&zero_prelude.recipe_program),
        Some(FaceRecipeProgramKind::Counted { header_value: 4 })
    );
    assert_eq!(
        operand.recipe_node_offsets,
        [
            face_recipe_name_at as u64 + 36,
            face_recipe_name_at as u64 + 52,
            face_recipe_name_at as u64 + 68,
        ]
    );
    assert_eq!(operand.recipe_nodes.len(), 3);
    assert_eq!(
        operand.recipe_nodes[0].byte_offset,
        face_recipe_name_at as u64 + 36
    );
    assert_eq!(
        operand.recipe_nodes[0].end_byte_offset,
        face_recipe_name_at as u64 + 52
    );
    assert_eq!(operand.recipe_nodes[0].program, [-1, -1, 2, 7]);
    assert_eq!(operand.next_record_index, 104);
    assert_eq!(operand.next_byte_offset, face_next_at);

    let mut prelude_bytes = face_bytes.clone();
    let prelude_words = [4i32, 5, 6, 7];
    let prelude_bytes_at = prelude_words
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>();
    prelude_bytes.splice(
        face_program_at + 12..face_program_at + 12,
        prelude_bytes_at.iter().copied(),
    );
    let prelude = parse_face_operand(
        &prelude_bytes,
        &face_scope,
        0,
        None,
        None,
        &record,
        std::slice::from_ref(&face_recipe),
    )
    .expect("face recipe operand with counted prelude");
    assert_eq!(prelude.recipe_program[0..7], [0, 0, 4, 4, 5, 6, 7]);
    assert_eq!(
        prelude.recipe_node_offsets[0],
        prelude.recipe_program_offset + 28
    );
    assert_eq!(prelude.recipe_nodes[0].program, [-1, -1, 2, 7]);

    let enclosing_limit = header(&mut face_bytes, *b"306", 105);
    let bounded = parse_face_operand(
        &face_bytes,
        &face_scope,
        0,
        None,
        Some(enclosing_limit),
        &record,
        std::slice::from_ref(&face_recipe),
    )
    .expect("face recipe bounded before its enclosing member limit");
    assert_eq!(bounded.next_record_index, 104);
    assert_eq!(bounded.next_byte_offset, face_next_at);

    let mut compact_bytes = Vec::new();
    header(&mut compact_bytes, *b"306", 100);
    header(&mut compact_bytes, *b"259", 100);
    header(&mut compact_bytes, *b"408", 101);
    header(&mut compact_bytes, *b"414", 102);
    let compact_record_at = header(&mut compact_bytes, *b"423", 103);
    let compact_name_at = compact_bytes.len() + 4;
    compact_bytes.extend_from_slice(&24u32.to_le_bytes());
    compact_bytes.extend_from_slice(b"bounded_face_recipe_data");
    for value in [0i32, -1, 4, 1, -1, 1, 0, -1] {
        compact_bytes.extend_from_slice(&value.to_le_bytes());
    }
    header(&mut compact_bytes, *b"306", 104);
    let mut compact_recipe = face_recipe.clone();
    compact_recipe.byte_offset = compact_name_at as u64;
    compact_recipe.record_index_offset = Some(compact_record_at + 8);
    let compact = parse_face_operand(
        &compact_bytes,
        &face_scope,
        0,
        None,
        None,
        &record,
        std::slice::from_ref(&compact_recipe),
    )
    .expect("compact face recipe operand");
    assert_eq!(compact.recipe_program, [0, -1, 4, 1, -1, 1, 0, -1]);
    assert!(compact.recipe_nodes.is_empty());

    let terminal_program_at = compact_name_at + 24;
    compact_bytes.truncate(terminal_program_at);
    for value in [0i32, -1] {
        compact_bytes.extend_from_slice(&value.to_le_bytes());
    }
    header(&mut compact_bytes, *b"306", 104);
    let terminal = parse_face_operand(
        &compact_bytes,
        &face_scope,
        0,
        None,
        None,
        &record,
        std::slice::from_ref(&compact_recipe),
    )
    .expect("terminal face recipe operand");
    assert_eq!(terminal.recipe_program, [0, -1]);
    assert!(terminal.recipe_nodes.is_empty());
    assert_eq!(
        face_recipe_program_kind(&terminal.recipe_program),
        Some(FaceRecipeProgramKind::Terminal)
    );
    assert_eq!(face_recipe_program_kind(&[0, 1, 4]), None);
    assert_eq!(face_recipe_program_kind(&[0, -1, 0]), None);
    operand.recipe_references.push(DesignRecipeReference {
        selector: 1,
        selector_offset: 1_101,
        token: "3".into(),
        token_offset: 1,
        design_reference: 303,
        design_reference_offset: 2,
        candidate_faces: Vec::new(),
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    });
    bind_face_operand_candidates(
        std::slice::from_mut(&mut operand),
        std::slice::from_ref(&face_recipe),
        &[
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(FaceId("f3d:brep:entity#50".into())),
                selector: 1,
                token: "3".into(),
                design_references: vec![303],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#2".into(),
                target: AttributeTarget::Face(FaceId("f3d:brep:entity#51".into())),
                selector: 1,
                token: "4".into(),
                design_references: vec![303],
                ordinal: 1,
            },
            PersistentSubentityTag {
                id: "f3d:xref/other/occurrence-0/design:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(FaceId("f3d:brep:entity#xref".into())),
                selector: 1,
                token: "3".into(),
                design_references: vec![303],
                ordinal: 0,
            },
        ],
    );
    assert_eq!(
        operand.candidate_faces,
        [
            FaceId("f3d:brep:entity#50".into()),
            FaceId("f3d:brep:entity#51".into())
        ]
    );
    assert_eq!(
        operand.unreferenced_candidate_faces,
        [FaceId("f3d:brep:entity#51".into())]
    );
    let mut direct_face = operand.clone();
    direct_face.recipe_kind = ConstructionRecipeKind::Face;
    direct_face.recipe_references = vec![DesignRecipeReference {
        selector: 1,
        selector_offset: 1_201,
        token: "3".into(),
        token_offset: 1_202,
        design_reference: 303,
        design_reference_offset: 1_203,
        candidate_faces: vec![FaceId("f3d:brep:entity#50".into())],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    }];
    direct_face.alternate_selector_candidate_faces.clear();
    direct_face.resolved_face_slots.clear();
    let group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
        scope_record_index: face_scope.record_index,
        scope_reference_ordinal: 0,
        record_index: 90,
        byte_offset: 900,
        class_tag: "306".into(),
        members: vec![operand.record_index],
        lost_edge_references: Vec::new(),
        member_offsets: vec![924],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 920,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![91],
            trailing_record_offsets: vec![935],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 954,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 958,
            variant: false,
        },
        role: 0x0000_0011_0000_0000,
        extrude_role: Some(DesignExtrudeOperandRole::Faces),
        extrude_face_role: Some(DesignExtrudeFaceRole::Termination),
        role_offset: 946,

        paired_class_tag: "259".into(),
        paired_byte_offset: 980,
    };
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&direct_face)),
        Some(FaceSelection::Resolved { faces, native })
            if faces == [FaceId("f3d:brep:entity#50".into())] && native == group.id
    ));
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&operand)),
        Some(FaceSelection::Resolved { faces, native })
            if faces == [FaceId("f3d:brep:entity#51".into())] && native == group.id
    ));
    operand
        .unreferenced_candidate_faces
        .push(FaceId("f3d:brep:entity#50".into()));
    assert!(resolved_face_group(&group, std::slice::from_ref(&operand)).is_none());
    operand.recipe_program = vec![0, -1, 1];
    operand.recipe_kind = ConstructionRecipeKind::BoundedFace;
    operand.recipe_nodes.clear();
    operand.recipe_nodes.push(DesignFaceRecipeNode {
        byte_offset: 1_200,
        end_byte_offset: 1_300,
        program: Vec::new(),
        recipe_structure: Some(DesignFaceRecipeStructure {
            root: 0,
            prelude: [0, 2],
            sides: [
                DesignTopologyRecipeSide {
                    field_count: std::num::NonZeroU32::new(3).unwrap(),
                    header_value: 0,
                    scalars: vec![0, 1],
                    payload_prefix: vec![0],
                    payload_entry_count: 0,
                    entries: Vec::new(),
                },
                DesignTopologyRecipeSide {
                    field_count: std::num::NonZeroU32::new(3).unwrap(),
                    header_value: 1,
                    scalars: vec![1, 0],
                    payload_prefix: vec![0],
                    payload_entry_count: 0,
                    entries: Vec::new(),
                },
            ],
        }),
    });
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&operand)),
        Some(FaceSelection::Resolved { faces, native })
            if faces == operand.unreferenced_candidate_faces && native == group.id
    ));
    operand.recipe_nodes[0].recipe_structure = None;
    assert!(resolved_face_group(&group, std::slice::from_ref(&operand)).is_none());
    operand.preceding_candidate_faces = vec![FaceId("f3d:brep:entity#50".into())];
    assert_eq!(
        crate::design::face_resolve::resolve_face_operand_history_candidates(&operand),
        Some(50)
    );
    operand.resolved_face_slots = vec![50];
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&operand)),
        Some(FaceSelection::Resolved { faces, native })
            if faces == [FaceId("f3d:brep:entity#50".into())] && native == group.id
    ));
    let mut historical_face_scope = face_scope.clone();
    historical_face_scope.previous_history_state_id = Some(49);
    assert!(matches!(
        crate::design::feature_project::direct_face_selection(
            &historical_face_scope,
            std::slice::from_ref(&operand)
        ),
        Some(FaceSelection::Historical { state, faces, native })
            if state == feature_input_topology_id(&crate::ids::neutral_feature_id(&historical_face_scope), 49)
                && faces.len() == 1
                && faces[0].0.ends_with(":49:50")
                && native == historical_face_scope.id
    ));
    operand.resolved_face_slots.clear();
    assert!(crate::design::face_resolve::retain_face_operand_resolution(
        &group,
        std::slice::from_mut(&mut operand),
        &FaceId("f3d:brep:entity#50".into()),
    ));
    assert_eq!(operand.resolved_face_slots, [50]);
    operand.resolved_face_slots.clear();
    operand.alternate_selector_candidate_faces = vec![
        FaceId("f3d:brep:entity#50".into()),
        FaceId("f3d:brep:entity#51".into()),
    ];
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&operand)),
        Some(FaceSelection::Resolved { faces, native })
            if faces == operand.alternate_selector_candidate_faces && native == group.id
    ));
    operand.alternate_selector_candidate_faces.clear();
    operand.resolved_face_slots = vec![50];
    let mut ambiguous = [operand.clone(), operand];
    assert!(
        !crate::design::face_resolve::retain_face_operand_resolution(
            &group,
            &mut ambiguous,
            &FaceId("f3d:brep:entity#50".into()),
        )
    );
}

#[test]
fn bounded_face_record_identity_is_not_a_second_design_id() {
    let mut bytes = Vec::new();
    for _ in 0..2 {
        let mut prefix = [0u8; 27];
        prefix[11..15].copy_from_slice(&309i32.to_le_bytes());
        prefix[23..27].copy_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(&prefix);
        bytes.extend_from_slice(b"bounded_face_recipe_data");
        bytes.extend_from_slice(&(-1i64).to_le_bytes());
    }
    let mut recipes = Vec::new();
    crate::design::decode::body::decode_stream(&bytes, "Design/BulkStream.dat", &mut recipes);
    assert_eq!(recipes.len(), 2);
    assert!(recipes.iter().all(|recipe| recipe.record_index == 309));
    assert!(recipes.iter().all(|recipe| recipe.design_id.is_none()));
    assert_eq!(recipes[0].recipe_index, 0);
    assert_eq!(recipes[1].recipe_index, 1);

    let mut body = Vec::new();
    body.extend_from_slice(&4u32.to_le_bytes());
    body.extend_from_slice(b"2265");
    body.extend_from_slice(&3u32.to_le_bytes());
    body.extend_from_slice(&[0; 12]);
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(b"body_recipe_data");
    let mut recipes = Vec::new();
    crate::design::decode::body::decode_stream(&body, "Design/BulkStream.dat", &mut recipes);
    assert_eq!(recipes.len(), 1);
    assert_eq!(recipes[0].design_id.as_deref(), Some("2265"));
    assert_eq!(recipes[0].design_id_offset, Some(4));
    assert_eq!(
        recipes[0].design_selector,
        Some(crate::records::ConstructionRecipeSelector {
            value: 3,
            byte_offset: 8,
        })
    );
}

#[test]
fn selected_face_start_requires_unique_sketch_plane_coincidence() {
    use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
    use cadmpeg_ir::topology::{Face, Sense};

    let sketch = Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 2.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let face = |id: &str, surface: &str| Face {
        id: FaceId(id.into()),
        shell: ShellId("shell".into()),
        surface: SurfaceId(surface.into()),
        sense: Sense::Forward,
        loops: Vec::new(),
        name: None,
        color: None,
        tolerance: None,
    };
    let plane = |id: &str, origin: Point3, normal: Vector3| Surface {
        id: SurfaceId(id.into()),
        geometry: SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    let faces = [
        face("coincident", "surface-coincident"),
        face("offset", "surface-offset"),
        face("tilted", "surface-tilted"),
    ];
    let surfaces = [
        plane(
            "surface-coincident",
            Point3::new(5.0, -3.0, 2.0),
            Vector3::new(0.0, 0.0, -2.0),
        ),
        plane(
            "surface-offset",
            Point3::new(0.0, 0.0, 2.1),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        plane(
            "surface-tilted",
            Point3::new(0.0, 0.0, 2.0),
            Vector3::new(0.0, 1.0, 0.0),
        ),
    ];

    assert!(crate::design::face_resolve::face_coincident_with_sketch(
        &faces[0].id,
        &sketch,
        &faces,
        &surfaces,
        1.0e-6,
        1.0e-10,
    ));
    for candidate in &faces[1..] {
        assert!(!crate::design::face_resolve::face_coincident_with_sketch(
            &candidate.id,
            &sketch,
            &faces,
            &surfaces,
            1.0e-6,
            1.0e-10,
        ));
    }
}

#[test]
fn sketch_placement_decodes_compact_identity_and_explicit_affine_frame() {
    fn candidates(
        bytes: &[u8],
        scope_record_index: u32,
        entity_id: &str,
        entity_suffix: u64,
        record_index: u32,
    ) -> Vec<DesignSketchPlacement> {
        let records = IndexedRecordOffsets::build(bytes);
        parse_sketch_placement_candidates(
            bytes,
            scope_record_index,
            entity_id,
            entity_suffix,
            record_index,
            &records,
        )
    }

    fn placement_frame(
        record_index: u32,
        length: usize,
        transform_offset: usize,
        transform: Option<[[f64; 4]; 4]>,
    ) -> Vec<u8> {
        let mut bytes = vec![0; length];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"356");
        bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
        if let Some(transform) = transform {
            for (ordinal, value) in transform.into_iter().flatten().enumerate() {
                let at = transform_offset + ordinal * 8;
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"259");
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes
    }

    let compact = candidates(&placement_frame(185, 201, 55, None), 177, "0_172", 172, 185);
    assert_eq!(compact.len(), 1);
    assert_eq!(compact[0].frame_length, 201);
    assert_eq!(compact[0].transform, identity_matrix());
    assert_eq!(compact[0].transform_offset, None);

    let transform = [
        [0.0, 0.0, 1.0, 12.0],
        [1.0, 0.0, 0.0, 34.0],
        [0.0, 1.0, 0.0, 56.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let explicit = candidates(
        &placement_frame(1773, 329, 55, Some(transform)),
        1765,
        "0_1761",
        1761,
        1773,
    );
    assert_eq!(explicit.len(), 1);
    assert_eq!(explicit[0].frame_length, 329);
    assert_eq!(explicit[0].transform, transform);
    assert_eq!(explicit[0].transform_offset, Some(55));

    for length in [305, 325] {
        let legacy = candidates(
            &placement_frame(1773, length, 48, Some(transform)),
            1765,
            "0_1761",
            1761,
            1773,
        );
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0].frame_length, length as u64);
        assert_eq!(legacy[0].transform, transform);
        assert_eq!(legacy[0].transform_offset, Some(48));
    }
}

#[test]
fn entity_genesis_placement_decodes_compact_and_explicit_frames() {
    fn candidates(
        bytes: &[u8],
        scope_record_index: u32,
        entity_id: &str,
        entity_suffix: u64,
        record_index: u32,
    ) -> Vec<DesignSketchPlacement> {
        let records = IndexedRecordOffsets::build(bytes);
        parse_sketch_placement_candidates(
            bytes,
            scope_record_index,
            entity_id,
            entity_suffix,
            record_index,
            &records,
        )
    }

    fn genesis_frame(
        record_index: u32,
        length: usize,
        form_byte: u8,
        transform: Option<[[f64; 4]; 4]>,
    ) -> Vec<u8> {
        let mut bytes = vec![0; length];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"293");
        bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
        bytes[55] = 1;
        bytes[65] = form_byte;
        if let Some(transform) = transform {
            for (ordinal, value) in transform.into_iter().flatten().enumerate() {
                let at = 66 + ordinal * 8;
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"261");
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes
    }

    let compact = candidates(&genesis_frame(214, 213, 1, None), 206, "0_201", 201, 214);
    assert_eq!(compact.len(), 1);
    assert_eq!(compact[0].frame_length, 213);
    assert_eq!(compact[0].transform, identity_matrix());
    assert_eq!(compact[0].transform_offset, None);

    let transform = [
        [0.0, 0.0, 1.0, 26.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let explicit = candidates(
        &genesis_frame(3060, 341, 0, Some(transform)),
        3052,
        "0_3048",
        3048,
        3060,
    );
    assert_eq!(explicit.len(), 1);
    assert_eq!(explicit[0].frame_length, 341);
    assert_eq!(explicit[0].transform, transform);
    assert_eq!(explicit[0].transform_offset, Some(66));

    // A mismatched form byte fails both lengths.
    assert!(candidates(&genesis_frame(214, 213, 0, None), 206, "0_201", 201, 214).is_empty());
    assert!(candidates(
        &genesis_frame(3060, 341, 1, Some(transform)),
        3052,
        "0_3048",
        3048,
        3060,
    )
    .is_empty());

    // The WorkPlane sibling of this record class carries a marked record
    // reference inside the zero run and must not decode as a placement.
    let mut workplane_like = genesis_frame(214, 213, 1, None);
    workplane_like[57] = 1;
    workplane_like[58..62].copy_from_slice(&788u32.to_le_bytes());
    assert!(candidates(&workplane_like, 206, "0_201", 201, 214).is_empty());
}

#[test]
fn entity_genesis_placement_origin_scales_to_neutral_units() {
    let placement = |frame_length: u64| DesignSketchPlacement {
        member_run_head: false,
        id: "f3d:native:design-sketch-placement#0".into(),
        scope_record_index: Some(10),
        entity_id: "0_100".into(),
        entity_suffix: 100,
        byte_offset: 0,
        class_tag: "293".into(),
        record_index: 11,
        frame_length,
        transform: [
            [0.0, 0.0, 1.0, 26.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: Some(66),
        paired_class_tag: "261".into(),
        paired_byte_offset: 341,
    };
    let point = SketchPoint {
        id: "f3d:native:sketch-point#0".into(),
        record_index: 20,
        owner_reference: Some(100),
        class_tag: "256".into(),
        byte_offset: 0,
        coordinate_offset: 141,
        entity_genesis: Some(2),
        persistent_id: 20,
        paired_reference: 0,
        coordinates: Point2::new(120.0, 30.0),
        raw_bytes: Vec::new(),
    };

    // The `EntityGenesis`-flavor frame stores its origin in centimetres
    // while the sketch records carry ten-times-centimetre values; the
    // projected sketch origin scales by ten to stay commensurate.
    let (sketches, entities) =
        project_sketch_design(&[placement(341)], &[point.clone()], &[], &[], 1.0e-6);
    assert_eq!(sketches.len(), 1);
    assert_eq!(
        sketches[0].resolved_placement(),
        Some((
            Point3::new(260.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ))
    );
    assert!(matches!(
        entities[0].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Point { position }
            if position == Point2::new(120.0, 30.0)
    ));

    // The settled explicit frame keeps its stored origin unscaled.
    let (sketches, _) = project_sketch_design(&[placement(329)], &[point], &[], &[], 1.0e-6);
    assert_eq!(
        sketches[0]
            .resolved_placement()
            .map(|(origin, _, _)| origin),
        Some(Point3::new(26.0, 0.0, 0.0))
    );
}

#[test]
fn feature_owned_sketch_placement_follows_member_run_head_reference() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"281");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.resize(40, 0);

    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.resize(80, 0);

    let head_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"283");
    bytes.extend_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);
    for value in identity_matrix().into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0, 1]);
    bytes.resize(
        head_at + crate::design::decode::sketch::MEMBER_RUN_HEAD_FRAME,
        0,
    );

    let entity = DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:design-entity-header#0".into(),
        byte_offset: 0,
        entity_suffix: 100,
        entity_id: "0_100".into(),
        class_tag: "281".into(),
        optional_slot_present: false,
        module: Some(DESIGN_MODULE_SKETCH.to_owned()),
        record_reference: None,
        record_reference_offset: None,
        declared_reference_count: None,
        reference_indices: Vec::new(),
        reference_offsets: Vec::new(),
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
    };
    let records = IndexedRecordOffsets::build(&bytes);
    let placement =
        crate::design::decode::sketch::parse_member_run_head_placement(&bytes, &entity, &records)
            .expect("feature-owned sketch placement");
    assert_eq!(placement.record_index, 200);
    assert_eq!(placement.byte_offset, head_at as u64);
    assert_eq!(placement.paired_byte_offset, paired_at as u64);
    assert_eq!(placement.transform, identity_matrix());
    assert!(placement.member_run_head);
    assert_eq!(placement.scope_record_index, None);
    assert_eq!(
        crate::design::decode::sketch::parse_legacy_sketch_container_members(
            &bytes, 0, 100, &records,
        ),
        Some((Vec::new(), Vec::new()))
    );

    bytes.truncate(head_at);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"283");
    bytes.extend_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 0, 1]);
    bytes.extend_from_slice(&173u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"284");
    bytes.extend_from_slice(&201u32.to_le_bytes());
    let records = IndexedRecordOffsets::build(&bytes);
    let compact =
        crate::design::decode::sketch::parse_member_run_head_placement(&bytes, &entity, &records)
            .expect("compact identity sketch placement");
    assert_eq!(compact.frame_length, 34);
    assert_eq!(compact.transform, identity_matrix());
    assert_eq!(compact.transform_offset, None);
}

#[test]
fn legacy_sketch_pair_decodes_its_complete_member_run() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"380");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.resize(40, 0);
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"381");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for member in [300u32, 301] {
        bytes.push(1);
        bytes.extend_from_slice(&member.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }

    let (members, offsets) =
        crate::design::decode::sketch::parse_legacy_sketch_member_run(&bytes, 0, 100)
            .expect("legacy sketch member run");
    assert_eq!(members, [300, 301]);
    assert_eq!(offsets, [(paired_at + 46) as u64, (paired_at + 57) as u64]);
}

#[test]
fn legacy_line_orthogonalizes_its_auxiliary_normal() {
    let mut bytes = vec![0u8; 133];
    let values: [f64; 12] = [
        0.5,
        0.875,
        0.0,
        0.0,
        -1.75,
        0.0,
        0.0,
        -1.0,
        0.0,
        -0.000_037,
        0.000_184,
        0.999_999_982,
    ];
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let SketchCurveGeometry::Line {
        direction, normal, ..
    } = crate::design::decode::sketch::decode_line(&bytes).expect("legacy line")
    else {
        panic!("expected line");
    };
    assert!((direction.norm() - 1.0).abs() <= 1.0e-12);
    assert!((normal.norm() - 1.0).abs() <= 1.0e-12);
    assert!(
        (direction.x * normal.x + direction.y * normal.y + direction.z * normal.z).abs() <= 1.0e-12
    );
    assert!(normal.z > 0.0);

    bytes[133 + 7 * 8..133 + 8 * 8].copy_from_slice(&1.0f64.to_le_bytes());
    let SketchCurveGeometry::Line { direction, .. } =
        crate::design::decode::sketch::decode_line(&bytes).expect("reverse-parameterized line")
    else {
        panic!("expected line");
    };
    assert!((direction.y + 1.0).abs() <= 1.0e-12);

    bytes[133 + 6 * 8..133 + 7 * 8].copy_from_slice(&0.6f64.to_le_bytes());
    bytes[133 + 7 * 8..133 + 8 * 8].copy_from_slice(&0.8f64.to_le_bytes());
    let SketchCurveGeometry::Line { direction, .. } =
        crate::design::decode::sketch::decode_line(&bytes)
            .expect("line with stale auxiliary direction")
    else {
        panic!("expected line");
    };
    assert!((direction.x).abs() <= 1.0e-12);
    assert!((direction.y + 1.0).abs() <= 1.0e-12);
}

#[test]
fn spatial_line_with_parallel_auxiliary_normal_retains_its_endpoints() {
    let values: [f64; 12] = [0.0, 3.0, 0.0, 0.0, 0.0, 1.5, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
    let mut bytes = vec![0u8; 133];
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    let SketchCurveGeometry::Line {
        start,
        end,
        direction,
        normal,
    } = crate::design::decode::sketch::decode_line(&bytes).expect("spatial line")
    else {
        panic!("expected line");
    };
    assert_eq!(start, Point3::new(0.0, 30.0, 0.0));
    assert_eq!(end, Point3::new(0.0, 30.0, 15.0));
    assert_eq!(direction, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(normal, Vector3::new(0.0, 1.0, 0.0));
}

#[test]
fn compact_planar_line_uses_its_implicit_normal() {
    let values: [f64; 9] = [0.5, 0.875, 0.0, 0.0, -1.75, 0.0, 0.0, -1.0, 0.0];
    let mut bytes = vec![0u8; 133];
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend_from_slice(&37u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);

    let SketchCurveGeometry::Line {
        start,
        end,
        direction,
        normal,
    } = crate::design::decode::sketch::decode_compact_planar_line(&bytes)
        .expect("compact planar line")
    else {
        panic!("expected line");
    };
    assert_eq!(start, Point3::new(5.0, 8.75, 0.0));
    assert_eq!(end, Point3::new(5.0, -8.75, 0.0));
    assert_eq!(direction, Vector3::new(0.0, -1.0, 0.0));
    assert_eq!(normal, Vector3::new(0.0, 0.0, 1.0));

    let mut referenced = vec![0u8; 133];
    referenced.push(1);
    referenced.extend_from_slice(&42u32.to_le_bytes());
    referenced.extend_from_slice(&[0; 6]);
    referenced.extend_from_slice(&bytes[133..]);
    assert_eq!(
        crate::design::decode::sketch::decode_referenced_analytic(&referenced),
        Some(SketchCurveGeometry::Line {
            start,
            end,
            direction,
            normal,
        })
    );
}

#[test]
fn retained_compact_planar_line_edit_preserves_its_reference_tail() {
    let values: [f64; 9] = [0.5, 0.875, 0.0, 0.0, -1.75, 0.0, 0.0, -1.0, 0.0];
    let mut bytes = Vec::new();
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend_from_slice(&37u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    let tail = bytes[72..].to_vec();
    let geometry = SketchCurveGeometry::Line {
        start: Point3::new(10.0, 20.0, 0.0),
        end: Point3::new(30.0, 40.0, 0.0),
        direction: Vector3::new(
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
            0.0,
        ),
        normal: Vector3::new(0.0, 0.0, 1.0),
    };
    crate::writer::patch::records::patch_sketch_curves(&mut bytes, &[(0, 0, geometry)])
        .expect("compact planar line edit");
    assert_eq!(&bytes[72..], tail);
    assert_eq!(f64::from_le_bytes(bytes[0..8].try_into().unwrap()), 1.0);
    assert_eq!(f64::from_le_bytes(bytes[24..32].try_into().unwrap()), 2.0);
}

#[test]
fn text_frame_line_decodes_after_point_references() {
    let mut bytes = vec![0u8; 52 + 133];
    for reference in [2397u32, 2395] {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        if reference == 2397 {
            bytes.push(0);
        }
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"289");
    bytes.extend_from_slice(&2403u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    for value in [
        -5.75f64, 1.0, 0.0, 5.25, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    let (geometry, end) = crate::design::decode::sketch::decode_text_frame_line(&bytes, 52, 2403)
        .expect("text-frame boundary line");
    assert_eq!(end, bytes.len());
    assert!(matches!(
        geometry,
        SketchCurveGeometry::Line { start, end, .. }
            if start == Point3::new(-57.5, 10.0, 0.0)
                && end == Point3::new(-5.0, 10.0, 0.0)
    ));
}

#[test]
fn legacy_sketch_nurbs_decodes_its_counted_arrays() {
    fn marked_reference(bytes: &mut Vec<u8>, record_index: u32) {
        bytes.push(1);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }

    let mut bytes = vec![0u8; 133];
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"285");
    bytes.extend_from_slice(&1200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1201u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&0.000_01f64.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    marked_reference(&mut bytes, 1202);
    marked_reference(&mut bytes, 1203);
    marked_reference(&mut bytes, 1204);
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0x95, 0xd6, 0x26, 0xe8, 0x0b, 0x2e, 0x11, 0x3e]);
    for (values, capacity) in [
        (vec![0.0f64, 0.0, 0.0, 1.0, 1.0, 1.0], 8u32),
        (vec![1.0f64, 1.0, 1.0], 8),
        (vec![0.0f64, 0.0, 0.0, 0.5, 0.75, 0.0, 1.0, 0.0, 0.0], 8),
    ] {
        let count = u32::try_from(if values.len() == 9 {
            values.len() / 3
        } else {
            values.len()
        })
        .expect("test count");
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&capacity.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes());
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }

    let (geometry, end) =
        crate::design::decode::sketch::decode_legacy_sketch_nurbs(&bytes).expect("legacy NURBS");
    let SketchCurveGeometry::Nurbs {
        degree,
        fit_tolerance,
        knots,
        weights,
        control_points,
        ..
    } = geometry
    else {
        panic!("expected NURBS");
    };
    assert_eq!(end, bytes.len());
    assert_eq!(degree, 2);
    assert_eq!(knots, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(weights, [1.0; 3]);
    assert_eq!(control_points[1], Point3::new(5.0, 7.5, 0.0));
    assert!((fit_tolerance - 0.000_1).abs() <= f64::EPSILON);
}

#[test]
fn sketch_geometry_tail_names_its_owner_container() {
    let mut bytes = vec![0u8; 112];
    bytes.push(1);
    bytes.extend_from_slice(&201u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"301");
    bytes.extend_from_slice(&400u32.to_le_bytes());
    assert_eq!(
        crate::design::decode::sketch::trailing_sketch_owner_reference(&bytes, 112),
        Some(201)
    );

    bytes[117] = 1;
    assert_eq!(
        crate::design::decode::sketch::trailing_sketch_owner_reference(&bytes, 112),
        None
    );

    let mut nested = vec![0u8; 140];
    nested[120..124].copy_from_slice(&3u32.to_le_bytes());
    nested[124..127].copy_from_slice(b"302");
    nested[127..131].copy_from_slice(&500u32.to_le_bytes());
    nested.push(1);
    nested.extend_from_slice(&201u32.to_le_bytes());
    nested.extend_from_slice(&[0; 6]);
    nested.extend_from_slice(&3u32.to_le_bytes());
    nested.extend_from_slice(b"303");
    nested.extend_from_slice(&501u32.to_le_bytes());
    assert_eq!(
        crate::design::decode::sketch::trailing_sketch_owner_reference(&nested, 131),
        Some(201)
    );
}

#[test]
fn sketch_member_run_backfills_relation_free_owners() {
    let mut bytes = vec![0u8; 40];
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 41]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let mut member_offsets = Vec::new();
    member_offsets.push((bytes.len() + 1) as u64);
    bytes.push(1);
    bytes.extend_from_slice(&99u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    for member in [20u32, 21] {
        member_offsets.push((bytes.len() + 1) as u64);
        bytes.push(1);
        bytes.extend_from_slice(&member.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&[0; 8]);
    assert_eq!(
        crate::design::decode::sketch::parse_sketch_member_run(&bytes, 0, 100),
        (vec![99, 20, 21], member_offsets)
    );
    assert_eq!(
        crate::design::decode::sketch::parse_sketch_member_run(&bytes, 0, 101),
        (vec![], vec![])
    );
    assert_eq!(
        crate::design::decode::sketch::parse_sketch_member_run(&bytes, paired_at + 1, 100),
        (vec![], vec![])
    );

    let header = |suffix: u64, members: Vec<u32>| DesignEntityHeader {
        id: format!("f3d:native:design-entity-header#{suffix}"),
        byte_offset: suffix,
        entity_suffix: suffix,
        entity_id: format!("0_{suffix}"),
        class_tag: "281".into(),
        optional_slot_present: false,
        module: Some(DESIGN_MODULE_SKETCH.to_owned()),
        record_reference: None,
        record_reference_offset: None,
        declared_reference_count: None,
        reference_indices: Vec::new(),
        reference_offsets: Vec::new(),
        member_offsets: members.iter().map(|_| 0).collect(),
        member_indices: members,
    };
    let point = |record_index: u32| SketchPoint {
        id: format!("f3d:native:sketch-point#{record_index}"),
        record_index,
        owner_reference: None,
        class_tag: "256".into(),
        byte_offset: u64::from(record_index),
        coordinate_offset: 141,
        entity_genesis: Some(2),
        persistent_id: u64::from(record_index),
        paired_reference: 0,
        coordinates: Point2::new(0.0, 0.0),
        raw_bytes: Vec::new(),
    };

    // Relation-free geometry named by the container's member run binds to
    // that sketch; records the run does not name stay unowned.
    let mut points = [point(20), point(21), point(22)];
    bind_sketch_graph(
        &[header(100, vec![20, 21, 99])],
        &mut points,
        &mut [],
        &mut [],
        &mut [],
    )
    .expect("member-run owners bind");
    assert_eq!(points[0].owner_reference, Some(100));
    assert_eq!(points[1].owner_reference, Some(100));
    assert_eq!(points[2].owner_reference, None);

    // Two sketches claiming one record is a structural conflict.
    let mut points = [point(20)];
    assert!(bind_sketch_graph(
        &[header(100, vec![20]), header(101, vec![20])],
        &mut points,
        &mut [],
        &mut [],
        &mut [],
    )
    .is_err());
}

#[test]
fn unbranched_closed_sketch_components_project_as_ordered_profiles() {
    let sketch = SketchId("f3d:model:sketch#profile".into());
    let line = |id: &str, start: Point2, end: Point2| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let entities = vec![
        line("line-a", Point2::new(0.0, 0.0), Point2::new(2.0, 0.0)),
        line("line-b", Point2::new(2.0, 2.0), Point2::new(2.0, 0.0)),
        line("line-c", Point2::new(2.0, 2.0), Point2::new(0.0, 2.0)),
        line(
            "line-d",
            Point2::new(0.0, 2.0 + 5.0e-7),
            Point2::new(0.0, 0.0),
        ),
        line("open-line", Point2::new(10.0, 0.0), Point2::new(11.0, 0.0)),
        SketchEntity {
            id: SketchEntityId("circle".into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Circle {
                center: Point2::new(20.0, 20.0),
                radius: Length(3.0),
            },
        },
    ];

    let profiles = closed_sketch_profiles(&sketch, &entities, 1.0e-6);
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].len(), 1);
    assert_eq!(profiles[0][0].entity, SketchEntityId("circle".into()));
    assert_eq!(
        profiles[1]
            .iter()
            .map(|entity_use| (entity_use.entity.0.as_str(), entity_use.reversed))
            .collect::<Vec<_>>(),
        [
            ("line-a", false),
            ("line-b", true),
            ("line-c", false),
            ("line-d", false),
        ]
    );
}

#[test]
fn branched_line_graph_projects_each_bounded_face() {
    let sketch = SketchId("f3d:model:sketch#branched-profile".into());
    let line = |id: &str, start: (f64, f64), end: (f64, f64)| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        },
    };
    let entities = vec![
        line("bottom-left", (0.0, 0.0), (1.0, 0.0)),
        line("bottom-right", (1.0, 0.0), (2.0, 0.0)),
        line("right", (2.0, 0.0), (2.0, 1.0)),
        line("top-right", (2.0, 1.0), (1.0, 1.0)),
        line("top-left", (1.0, 1.0), (0.0, 1.0)),
        line("left", (0.0, 1.0), (0.0, 0.0)),
        line("divider", (1.0, 0.0), (1.0, 1.0)),
    ];

    let profiles = closed_sketch_profiles(&sketch, &entities, 1.0e-6);
    assert_eq!(profiles.len(), 2);
    assert!(profiles.iter().all(|profile| profile.len() == 4));
    assert!(profiles.iter().all(|profile| profile
        .iter()
        .any(|entity_use| entity_use.entity.0 == "divider")));
}

#[test]
fn branched_line_graph_with_a_shared_corner_projects_bounded_faces() {
    let sketch = SketchId("f3d:model:sketch#shared-corner-profile".into());
    let line = |id: &str, start: (f64, f64), end: (f64, f64)| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        },
    };
    let entities = vec![
        line("outer-bottom", (0.0, 0.0), (31.0, 0.0)),
        line("outer-right", (31.0, 0.0), (31.0, 47.0)),
        line("outer-top", (31.0, 47.0), (0.0, 47.0)),
        line("outer-left", (0.0, 47.0), (0.0, 0.0)),
        line("inner-top", (0.0, 47.0), (9.0, 47.0)),
        line("inner-right", (9.0, 47.0), (9.0, 41.0)),
        line("inner-bottom", (9.0, 41.0), (0.0, 41.0)),
        line("inner-left", (0.0, 41.0), (0.0, 47.0)),
    ];

    let profiles = closed_sketch_profiles(&sketch, &entities, 1.0e-6);
    assert_eq!(
        profiles
            .iter()
            .flat_map(|profile| profile
                .iter()
                .map(|entity_use| (entity_use.entity.0.as_str(), entity_use.reversed)))
            .collect::<Vec<_>>(),
        [
            ("outer-left", false),
            ("outer-bottom", false),
            ("outer-right", false),
            ("outer-top", false),
            ("inner-top", false),
            ("inner-right", false),
            ("inner-bottom", false),
            ("inner-left", false),
        ]
    );
}

#[test]
fn placed_sketch_projects_signed_normal_and_nonclamped_curves() {
    let placement = DesignSketchPlacement {
        member_run_head: false,
        id: "f3d:native:placement#0".into(),
        scope_record_index: Some(177),
        entity_id: "0_172".into(),
        entity_suffix: 172,
        byte_offset: 100,
        class_tag: "356".into(),
        record_index: 185,
        frame_length: 329,
        transform: [
            [0.0, 0.0, 1.0, 10.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 1.0, 0.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: Some(155),
        paired_class_tag: "259".into(),
        paired_byte_offset: 429,
    };
    let point = SketchPoint {
        id: "f3d:native:point#175".into(),
        record_index: 175,
        owner_reference: Some(172),
        class_tag: "300".into(),
        byte_offset: 400,
        coordinate_offset: 89,
        entity_genesis: None,
        persistent_id: 10,
        paired_reference: 0,
        coordinates: Point2::new(2.5, 4.0),
        raw_bytes: Vec::new(),
    };
    let line = SketchCurveIdentity {
        id: "f3d:native:curve#217".into(),
        record_index: 217,
        owner_reference: Some(172),
        class_tag: "301".into(),
        byte_offset: 500,
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: 20,
        secondary_id: 0,
        geometry: Some(SketchCurveGeometry::Line {
            start: Point3::new(1.0, 2.0, 0.0),
            end: Point3::new(4.0, 6.0, 0.0),
            direction: Vector3::new(0.6, 0.8, 0.0),
            normal: Vector3::new(0.0, 0.0, -1.0),
        }),
    };
    let clockwise_arc = SketchCurveIdentity {
        id: "f3d:native:curve#220".into(),
        record_index: 220,
        owner_reference: Some(172),
        class_tag: "305".into(),
        byte_offset: 800,
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: 22,
        secondary_id: 0,
        geometry: Some(SketchCurveGeometry::Arc {
            center: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, -1.0),
            reference_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::FRAC_PI_2,
        }),
    };
    let nonclamped_nurbs = SketchCurveIdentity {
        id: "f3d:native:curve#218".into(),
        record_index: 218,
        owner_reference: Some(172),
        class_tag: "303".into(),
        byte_offset: 700,
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: 21,
        secondary_id: 0,
        geometry: Some(SketchCurveGeometry::Nurbs {
            carrier_reference: None,
            subtype_class_tag: "304".into(),
            subtype_record_index: 219,
            degree: 2,
            fit_tolerance: 1.0e-6,
            scalar_width: 8,
            knots: vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            weights: Vec::new(),
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(2.0, 2.0, 0.0),
                Point3::new(4.0, 2.0, 0.0),
            ],
        }),
    };

    let placements = vec![placement];
    let points = vec![point];
    let curves = vec![line, nonclamped_nurbs, clockwise_arc];
    let (sketches, entities) = project_sketch_design(&placements, &points, &curves, &[], 1.0e-6);
    assert_eq!(sketches.len(), 1);
    assert_eq!(
        sketches[0].resolved_placement(),
        Some((
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ))
    );
    assert_eq!(entities.len(), 4);
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SketchGeometry::Point { position } if position == Point2::new(2.5, 4.0)
    )));
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SketchGeometry::Line { start, end }
            if start == Point2::new(1.0, 2.0) && end == Point2::new(4.0, 6.0)
    )));
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SketchGeometry::Arc { start_angle, end_angle, .. }
            if start_angle.0 == 0.0
                && end_angle.0 == -std::f64::consts::FRAC_PI_2
    )));
    let nurbs = entities
        .iter()
        .find(|entity| entity.native_ref.as_deref() == Some("f3d:native:curve#218"))
        .expect("non-clamped NURBS projects");
    let endpoints = sketch_entity_endpoints(nurbs).expect("non-clamped NURBS endpoints");
    assert_eq!(endpoints, [Point2::new(1.0, 0.0), Point2::new(3.0, 2.0)]);
    assert!(point_on_sketch_entity(Point2::new(2.0, 1.0), nurbs, 1.0e-9));
    assert!(point_lies_on_sketch_geometry(
        Point2::new(2.0, 1.0),
        &nurbs.geometry
    ));

    let relation = |record_index, member, operand| SketchRelation {
        id: format!("f3d:native:relation#{record_index}"),
        record_index,
        class_tag: "302".into(),
        byte_offset: 600,
        state_offset: 70,
        owner_reference: 172,
        owner_entity_id: "0_172".into(),
        auxiliary_references: Vec::new(),
        auxiliary_reference_offsets: Vec::new(),
        members: vec![member],
        resolved_members: vec![operand],
        member_offsets: vec![25],
        owner_reference_offset: 55,
        state: 0x40,
        constraint_kinds: vec![SketchConstraintKind::Horizontal],
        unknown_constraint_bits: 0,
        member_relation_ordinals: Vec::new(),
        entity_genesis: None,
        pattern: None,
        return_members: vec![member],
        resolved_return_members: Vec::new(),
        return_member_offsets: vec![80],
        raw_bytes: Vec::new(),
    };
    let mut curve_point_coincidence = relation(
        702,
        217,
        SketchRelationOperand::Curve {
            record_index: 217,
            primary_id: 20,
            secondary_id: 0,
        },
    );
    curve_point_coincidence.members.push(175);
    curve_point_coincidence
        .resolved_members
        .push(SketchRelationOperand::Point {
            record_index: 175,
            persistent_id: 10,
        });
    curve_point_coincidence.member_offsets.push(40);
    curve_point_coincidence.state = 1;
    curve_point_coincidence.constraint_kinds = vec![SketchConstraintKind::Coincident];
    let mut midpoint = curve_point_coincidence.clone();
    midpoint.record_index = 703;
    midpoint.id = "f3d:native:relation#703".into();
    midpoint.state = 0x10;
    midpoint.constraint_kinds = vec![SketchConstraintKind::Parallel];
    let mut curvature = curve_point_coincidence.clone();
    curvature.record_index = 704;
    curvature.id = "f3d:native:relation#704".into();
    curvature.state = 0x200;
    curvature.constraint_kinds = vec![SketchConstraintKind::Curvature];
    let mut horizontal_point = relation(
        701,
        175,
        SketchRelationOperand::Point {
            record_index: 175,
            persistent_id: 10,
        },
    );
    horizontal_point.auxiliary_references = vec![999];
    horizontal_point.return_members = vec![175, 175];
    horizontal_point.state = 0x8000_0040;
    horizontal_point.unknown_constraint_bits = 0x8000_0000;
    let constraints = project_sketch_constraints(
        &placements,
        &[],
        &points,
        &curves,
        &[],
        &[
            relation(
                700,
                217,
                SketchRelationOperand::Curve {
                    record_index: 217,
                    primary_id: 20,
                    secondary_id: 0,
                },
            ),
            horizontal_point,
            curve_point_coincidence,
            midpoint,
            curvature,
        ],
        &entities,
    );
    assert!(matches!(
        constraints[0].definition,
        SketchConstraintDefinition::Horizontal { .. }
    ));
    assert!(matches!(
        constraints[1].definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            native_state: Some(0x8000_0040),
            native_flags: None,
            ref entities,
            ref operands,
            ..
        } if native_kind == "horizontal+unknown_bits"
            && entities.len() == 3
            && entities.iter().all(|entity| entity == &entities[0])
            && operands.iter().map(|operand| (operand.native_field.as_deref(), operand.native_kind.as_str(), operand.object_index)).collect::<Vec<_>>()
                == [
                    (Some("member"), "point", 175),
                    (Some("auxiliary"), "record", 999),
                    (Some("return"), "point", 175),
                    (Some("return"), "point", 175),
                ]
    ));
    assert!(matches!(
        constraints[2].definition,
        SketchConstraintDefinition::Coincident { ref entities } if entities.len() == 2
    ));
    assert!(matches!(
        constraints[3].definition,
        SketchConstraintDefinition::Midpoint { .. }
    ));
    assert!(matches!(
        constraints[4].definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            ref entities,
            ..
        } if native_kind == "curvature" && entities.len() == 3
    ));
    let line = entities
        .iter()
        .find(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
        .unwrap();
    let point = entities
        .iter()
        .find(|entity| matches!(entity.geometry, SketchGeometry::Point { .. }))
        .unwrap();
    let mut other_point = point.clone();
    other_point.id = SketchEntityId("generated:point#other".into());
    assert!(matches!(
        exact_atomic_constraint(SketchConstraintKind::Horizontal, &[point, &other_point]),
        Some(SketchConstraintDefinition::HorizontalLoci { .. })
    ));
    assert!(matches!(
        exact_atomic_constraint(SketchConstraintKind::Vertical, &[point, &other_point]),
        Some(SketchConstraintDefinition::VerticalLoci { .. })
    ));
    assert!(exact_atomic_constraint(SketchConstraintKind::Horizontal, &[point, point]).is_none());
    assert!(matches!(
        exact_atomic_constraint(SketchConstraintKind::Midpoint, &[line, point]),
        Some(SketchConstraintDefinition::Midpoint { .. })
    ));
    for kind in [
        SketchConstraintKind::Tangent,
        SketchConstraintKind::Curvature,
        SketchConstraintKind::Equal,
    ] {
        assert!(exact_atomic_constraint(kind, &[line, point]).is_none());
    }
    let mut other_line = line.clone();
    other_line.id = SketchEntityId("generated:line#other".into());
    assert!(matches!(
        exact_atomic_constraint(SketchConstraintKind::Tangent, &[line, &other_line]),
        Some(SketchConstraintDefinition::Tangent { .. })
    ));
    assert!(matches!(
        exact_atomic_constraint(SketchConstraintKind::Curvature, &[line, &other_line]),
        Some(SketchConstraintDefinition::Curvature { .. })
    ));
    assert!(matches!(
        exact_atomic_constraint(SketchConstraintKind::Equal, &[line, &other_line]),
        Some(SketchConstraintDefinition::Equal { .. })
    ));
    for kind in [
        SketchConstraintKind::Colinear,
        SketchConstraintKind::EqualLength,
        SketchConstraintKind::Parallel,
        SketchConstraintKind::Perpendicular,
        SketchConstraintKind::Tangent,
        SketchConstraintKind::Curvature,
        SketchConstraintKind::Equal,
    ] {
        assert!(exact_atomic_constraint(kind, &[line, line]).is_none());
    }
}

#[test]
fn nonplanar_sketch_curves_project_in_model_space() {
    use cadmpeg_ir::sketches::SpatialSketchGeometry;

    let placement = DesignSketchPlacement {
        member_run_head: false,
        id: "f3d:Design/BulkStream.dat:placement#100".into(),
        scope_record_index: None,
        entity_id: "Sketch_42".into(),
        entity_suffix: 42,
        byte_offset: 100,
        class_tag: "300".into(),
        record_index: 100,
        frame_length: 329,
        transform: [
            [0.0, 0.0, 1.0, 10.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 1.0, 0.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: Some(155),
        paired_class_tag: "259".into(),
        paired_byte_offset: 429,
    };
    let curve = |record_index, primary_id, geometry| SketchCurveIdentity {
        id: format!("f3d:Design/BulkStream.dat:curve#{record_index}"),
        record_index,
        owner_reference: Some(42),
        class_tag: "301".into(),
        byte_offset: u64::from(record_index),
        geometry_offset: 100,
        entity_genesis: None,
        primary_id,
        secondary_id: 0,
        geometry: Some(geometry),
    };
    let mut curves = vec![
        curve(
            101,
            1,
            SketchCurveGeometry::Line {
                start: Point3::new(1.0, 2.0, 3.0),
                end: Point3::new(4.0, 5.0, 6.0),
                direction: Vector3::new(1.0, 1.0, 1.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
            },
        ),
        curve(
            102,
            2,
            SketchCurveGeometry::Arc {
                center: Point3::new(1.0, 2.0, 3.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                reference_direction: Vector3::new(0.0, 1.0, 0.0),
                radius: 2.0,
                start_angle: 0.0,
                end_angle: std::f64::consts::TAU,
            },
        ),
        curve(
            108,
            7,
            SketchCurveGeometry::Line {
                start: Point3::new(1.0, 2.0, 0.0),
                end: Point3::new(4.0, 2.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
        ),
    ];
    curves.push(SketchCurveIdentity {
        id: "f3d:Design/BulkStream.dat:curve#103".into(),
        record_index: 103,
        owner_reference: Some(42),
        class_tag: "301".into(),
        byte_offset: 103,
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: 3,
        secondary_id: 0,
        geometry: None,
    });
    curves.push(curve(
        104,
        4,
        SketchCurveGeometry::Nurbs {
            carrier_reference: None,
            subtype_class_tag: "302".into(),
            subtype_record_index: 104,
            degree: 1,
            fit_tolerance: 1.0e-8,
            scalar_width: 4,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            weights: vec![1.0, 1.0],
            control_points: vec![Point3::new(2.0, 3.0, 4.0), Point3::new(5.0, 6.0, 7.0)],
        },
    ));
    let relation = SketchRelation {
        id: "f3d:Design/BulkStream.dat:relation#105".into(),
        record_index: 105,
        class_tag: "303".into(),
        byte_offset: 105,
        state_offset: 0,
        owner_reference: 42,
        owner_entity_id: "Sketch_42".into(),
        auxiliary_references: Vec::new(),
        auxiliary_reference_offsets: Vec::new(),
        members: vec![103, 104],
        resolved_members: Vec::new(),
        member_offsets: Vec::new(),
        owner_reference_offset: 0,
        state: 0x8000_0000,
        constraint_kinds: vec![SketchConstraintKind::SplineGroup],
        unknown_constraint_bits: 0,
        member_relation_ordinals: Vec::new(),
        entity_genesis: None,
        pattern: None,
        return_members: vec![103, 104],
        resolved_return_members: Vec::new(),
        return_member_offsets: Vec::new(),
        raw_bytes: Vec::new(),
    };
    let mut point_bytes = vec![0; 24];
    point_bytes[16..24].copy_from_slice(&0.45f64.to_le_bytes());
    let point = SketchPoint {
        id: "f3d:Design/BulkStream.dat:point#106".into(),
        record_index: 106,
        owner_reference: Some(42),
        class_tag: "305".into(),
        byte_offset: 106,
        coordinate_offset: 0,
        entity_genesis: None,
        persistent_id: 5,
        paired_reference: 0,
        coordinates: Point2::new(2.5, 3.5),
        raw_bytes: point_bytes,
    };
    let mut midpoint_relation = relation.clone();
    midpoint_relation.id = "f3d:Design/BulkStream.dat:relation#106".into();
    midpoint_relation.record_index = 106;
    midpoint_relation.state = 0x1000;
    midpoint_relation.constraint_kinds = vec![SketchConstraintKind::Midpoint];
    midpoint_relation.members = vec![106, 101];
    midpoint_relation.return_members = vec![101, 106];
    let mut coincident_point = point.clone();
    coincident_point.id = "f3d:Design/BulkStream.dat:point#107".into();
    coincident_point.record_index = 107;
    coincident_point.byte_offset = 107;
    coincident_point.persistent_id = 6;
    let mut coincident_relation = relation.clone();
    coincident_relation.id = "f3d:Design/BulkStream.dat:relation#107".into();
    coincident_relation.record_index = 107;
    coincident_relation.state = 0x40;
    coincident_relation.constraint_kinds = vec![SketchConstraintKind::Coincident];
    coincident_relation.members = vec![106, 107];
    coincident_relation.return_members = vec![106, 107];
    let mut horizontal_relation = relation.clone();
    horizontal_relation.id = "f3d:Design/BulkStream.dat:relation#108".into();
    horizontal_relation.record_index = 108;
    horizontal_relation.state = 0x40;
    horizontal_relation.constraint_kinds = vec![SketchConstraintKind::Horizontal];
    horizontal_relation.members = vec![108];
    horizontal_relation.return_members = vec![108];
    let surface = SketchSurface {
        id: "f3d:Design/BulkStream.dat:surface#109".into(),
        record_index: 109,
        owner_reference: Some(42),
        class_tag: "306".into(),
        byte_offset: 109,
        entity_genesis: None,
        persistent_id: 8,
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![
            vec![Point3::new(1.0, 2.0, 0.0), Point3::new(1.0, 5.0, 0.0)],
            vec![Point3::new(4.0, 2.0, 0.0), Point3::new(4.0, 5.0, 0.0)],
        ],
    };
    let mut point_on_surface_relation = relation.clone();
    point_on_surface_relation.id = "f3d:Design/BulkStream.dat:relation#109".into();
    point_on_surface_relation.record_index = 109;
    point_on_surface_relation.state = 1;
    point_on_surface_relation.constraint_kinds = vec![SketchConstraintKind::Coincident];
    point_on_surface_relation.members = vec![106, 109];
    point_on_surface_relation.return_members = vec![106, 109];

    let points = [point, coincident_point];
    let relations = [
        relation,
        midpoint_relation,
        coincident_relation,
        horizontal_relation,
        point_on_surface_relation,
    ];
    let (planar_sketches, planar_entities) =
        project_sketch_design(&[placement.clone()], &points, &curves, &[], 1.0e-6);
    assert!(planar_sketches.is_empty());
    assert!(planar_entities.is_empty());
    let surfaces = [surface];
    let (sketches, entities) = project_spatial_sketch_design(
        &[placement.clone()],
        &points,
        &curves,
        &surfaces,
        &relations,
        1.0e-6,
    );
    assert_eq!(sketches.len(), 1);
    assert_eq!(entities.len(), 8);
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SpatialSketchGeometry::Line { start, end }
            if start == Point3::new(13.0, 21.0, 32.0)
                && end == Point3::new(16.0, 24.0, 35.0)
    )));
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SpatialSketchGeometry::Line { start, end }
            if start == Point3::new(14.0, 22.0, 33.0)
                && end == Point3::new(17.0, 25.0, 36.0)
    )));
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SpatialSketchGeometry::Line { start, end }
            if start == Point3::new(10.0, 21.0, 32.0)
                && end == Point3::new(10.0, 24.0, 32.0)
    )));
    let constraints = project_spatial_sketch_constraints(
        &[placement],
        &relations,
        &points,
        &curves,
        &surfaces,
        &entities,
    );
    assert!(matches!(
        constraints.first(),
        Some(cadmpeg_ir::sketches::SpatialSketchConstraint {
            definition: cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::SplineGroup { entities },
            ..
        }) if entities.len() == 2
    ));
    assert!(matches!(
        constraints.get(1),
        Some(cadmpeg_ir::sketches::SpatialSketchConstraint {
            definition: cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::Midpoint { .. },
            ..
        })
    ));
    assert!(matches!(
        constraints.get(2),
        Some(cadmpeg_ir::sketches::SpatialSketchConstraint {
            definition: cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::Coincident { .. },
            ..
        })
    ));
    assert!(matches!(
        constraints.get(3),
        Some(cadmpeg_ir::sketches::SpatialSketchConstraint {
            definition: cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::ParallelToDirection {
                entity,
                direction,
            },
            ..
        }) if entity == &crate::ids::neutral_spatial_sketch_curve_id(
            &sketches[0].id,
            7,
            0,
        ) && direction == &Vector3::new(0.0, 1.0, 0.0)
    ));
    assert!(matches!(
        constraints.get(4),
        Some(cadmpeg_ir::sketches::SpatialSketchConstraint {
            definition: cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::PointOnSurface { .. },
            ..
        })
    ));
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SpatialSketchGeometry::Circle {
            center,
            normal,
            reference_direction,
            radius: Length(2.0),
        } if center == Point3::new(13.0, 21.0, 32.0)
            && normal == Vector3::new(0.0, 1.0, 0.0)
            && reference_direction == Vector3::new(0.0, 0.0, 1.0)
    )));
}

#[test]
fn three_member_symmetry_states_project_unique_reflection_axis() {
    let entity = |id: &str, geometry: SketchGeometry| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let first = entity(
        "generated:point#left",
        SketchGeometry::Point {
            position: Point2::new(-2.0, 3.0),
        },
    );
    let axis_entity = entity(
        "generated:line#axis",
        SketchGeometry::Line {
            start: Point2::new(0.0, -5.0),
            end: Point2::new(0.0, 5.0),
        },
    );
    let second = entity(
        "generated:point#right",
        SketchGeometry::Point {
            position: Point2::new(2.0, 3.0),
        },
    );

    for kind in [
        SketchConstraintKind::Concentric,
        SketchConstraintKind::Symmetry,
    ] {
        let definition = exact_atomic_constraint(kind, &[&first, &axis_entity, &second]).unwrap();
        assert!(matches!(
            definition,
            SketchConstraintDefinition::Symmetric {
                first: cadmpeg_ir::sketches::SketchLocus::Entity(ref first_id),
                second: cadmpeg_ir::sketches::SketchLocus::Entity(ref second_id),
                axis: ref axis_id,
            } if first_id == &first.id
                && second_id == &second.id
                && axis_id == &axis_entity.id
        ));
    }

    let off_axis = entity(
        "generated:line#off-axis",
        SketchGeometry::Line {
            start: Point2::new(1.0, -5.0),
            end: Point2::new(1.0, 5.0),
        },
    );
    assert!(exact_atomic_constraint(
        SketchConstraintKind::Concentric,
        &[&first, &off_axis, &second],
    )
    .is_none());
    let on_axis = entity(
        "generated:point#on-axis",
        SketchGeometry::Point {
            position: Point2::new(0.0, 3.0),
        },
    );
    for kind in [
        SketchConstraintKind::Concentric,
        SketchConstraintKind::Symmetry,
    ] {
        assert!(exact_atomic_constraint(kind, &[&on_axis, &axis_entity, &on_axis]).is_none());
    }
}

#[test]
fn coincident_relation_projects_one_unique_shared_locus_per_member() {
    let entity = |id: &str, geometry: SketchGeometry| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let line = entity(
        "generated:line#0",
        SketchGeometry::Line {
            start: Point2::new(1.0, 2.0),
            end: Point2::new(4.0, 2.0),
        },
    );
    let point = entity(
        "generated:point#0",
        SketchGeometry::Point {
            position: Point2::new(1.0, 2.0),
        },
    );
    assert_eq!(
        crate::design::dimensions::exact_coincident_loci(&[&line, &point]),
        Some(SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                cadmpeg_ir::sketches::SketchLocus::Start(line.id.clone()),
                cadmpeg_ir::sketches::SketchLocus::Entity(point.id.clone()),
            ],
        })
    );

    let degenerate = entity(
        "generated:line#degenerate",
        SketchGeometry::Line {
            start: Point2::new(1.0, 2.0),
            end: Point2::new(1.0, 2.0),
        },
    );
    assert!(crate::design::dimensions::exact_coincident_loci(&[&degenerate, &point]).is_none());
    assert!(crate::design::dimensions::exact_coincident_loci(&[&line, &line]).is_none());
    assert!(exact_atomic_constraint(SketchConstraintKind::Coincident, &[&line, &line]).is_none());
}

#[test]
fn polygon_constraint_requires_three_distinct_resolved_members() {
    let entity = |id: &str| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    };
    let first = entity("generated:point#0");
    let second = entity("generated:point#1");
    let third = entity("generated:point#2");
    assert_eq!(
        exact_atomic_constraint(SketchConstraintKind::Polygon, &[&first, &second, &third]),
        Some(SketchConstraintDefinition::Polygon {
            entities: vec![first.id.clone(), second.id.clone(), third.id.clone()]
        })
    );
    assert!(exact_atomic_constraint(SketchConstraintKind::Polygon, &[&first, &second]).is_none());
    assert!(
        exact_atomic_constraint(SketchConstraintKind::Polygon, &[&first, &second, &first])
            .is_none()
    );
}

#[test]
fn aggregate_offset_relation_projects_ordered_oriented_pairs() {
    let entity = |id: &str, geometry: SketchGeometry| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let source_horizontal = entity(
        "generated:line#source-horizontal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
    );
    let result_horizontal = entity(
        "generated:line#result-horizontal",
        SketchGeometry::Line {
            start: Point2::new(2.0, -2.0),
            end: Point2::new(8.0, -2.0),
        },
    );
    let source_vertical = entity(
        "generated:line#source-vertical",
        SketchGeometry::Line {
            start: Point2::new(0.0, 10.0),
            end: Point2::new(0.0, 0.0),
        },
    );
    let result_vertical = entity(
        "generated:line#result-vertical",
        SketchGeometry::Line {
            start: Point2::new(2.0, 2.0),
            end: Point2::new(2.0, 8.0),
        },
    );
    let curve = |record_index, secondary_id| SketchRelationOperand::Curve {
        record_index,
        primary_id: u64::from(record_index),
        secondary_id,
    };
    let relation = SketchRelation {
        id: "f3d:native:sketch-relation#0".into(),
        record_index: 10,
        class_tag: "300".into(),
        byte_offset: 0,
        state_offset: 100,
        owner_reference: 1,
        owner_entity_id: "0_1".into(),
        auxiliary_references: vec![0],
        auxiliary_reference_offsets: vec![80],
        members: vec![1, 2, 3, 4],
        resolved_members: Vec::new(),
        member_offsets: vec![25, 40, 55, 70],
        owner_reference_offset: 90,
        state: 0x20_0000_0000,
        constraint_kinds: vec![SketchConstraintKind::Offset],
        unknown_constraint_bits: 0,
        member_relation_ordinals: vec![3, 5, 1, 1],
        entity_genesis: None,
        pattern: None,
        return_members: vec![1, 3, 2, 4],
        resolved_return_members: vec![curve(1, 10), curve(3, 30), curve(2, 20), curve(4, 40)],
        return_member_offsets: vec![120, 131, 142, 153],
        raw_bytes: Vec::new(),
    };
    let projected = HashMap::from([
        (("native", 1), &source_horizontal),
        (("native", 2), &source_vertical),
        (("native", 3), &result_horizontal),
        (("native", 4), &result_vertical),
    ]);

    let definition = exact_offset_constraint(&relation, "native", &projected).unwrap();
    let SketchConstraintDefinition::Offset {
        pairs,
        distance,
        parameter,
        parameter_factor,
    } = definition
    else {
        panic!("expected neutral offset constraint")
    };
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].source, source_horizontal.id);
    assert_eq!(pairs[0].result, result_horizontal.id);
    assert_eq!(pairs[1].source, source_vertical.id);
    assert_eq!(pairs[1].result, result_vertical.id);
    assert!((distance.0 - 2.0).abs() <= 1.0e-9);
    assert!(pairs[0].source_reversed);
    assert!(!pairs[1].source_reversed);
    assert_eq!(parameter, None);
    assert_eq!(parameter_factor, None);

    let mut repeated_pair = relation;
    repeated_pair.return_members.extend([1, 3]);
    repeated_pair
        .resolved_return_members
        .extend([curve(1, 10), curve(3, 30)]);
    assert!(exact_offset_constraint(&repeated_pair, "native", &projected).is_none());
}

#[test]
fn single_curve_annotation_projects_parameterized_offset() {
    let stream = "f3d:Design/BulkStream.dat";
    let sketch = SketchId("generated:sketch#offset".into());
    let source_curve_id = format!("{stream}:sketch-curve#10");
    let result_curve_id = format!("{stream}:sketch-curve#11");
    let curve = |id: String, record_index, primary_id, secondary_id| SketchCurveIdentity {
        id,
        record_index,
        owner_reference: Some(100),
        class_tag: "262".into(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id,
        secondary_id,
        geometry: None,
    };
    let source_curve = curve(source_curve_id.clone(), 10, 20, 0);
    let result_curve = curve(result_curve_id.clone(), 11, 21, 7);
    let entity = |id: &str, native_ref: String, start, end| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(native_ref),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let source = entity(
        "source",
        source_curve_id,
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
    );
    let result = entity(
        "result",
        result_curve_id,
        Point2::new(0.0, -2.0),
        Point2::new(10.0, -2.0),
    );
    let parameter = DesignParameter {
        id: format!("{stream}:design-parameter#12"),
        byte_offset: 0,
        class_tag: "305".into(),
        record_index: 12,
        family_discriminator: Some(6),
        family_discriminator_offset: Some(0),
        source_ordinal: 0,
        owner_record_index: Some(13),
        expression: "2 mm".into(),
        expression_offset: 0,
        source_kind: "Linear Dimension-2".into(),
        source_kind_offset: 0,
        kind: DesignParameterKind::Dimension,
        unit: Some("mm".into()),
        unit_offset: Some(0),
        name: "d1".into(),
        name_offset: 0,
        evaluated_value: 0.2,
        evaluated_value_offset: 0,
    };
    let frame = DesignDimensionAnnotationFrame {
        id: format!("{stream}:design-dimension-annotation-frame#14"),
        companion_record_index: Some(15),
        governing_companion_record_index: 15,
        byte_offset: 0,
        class_tag: "256".into(),
        record_index: 14,
        frame_length: 100,
        operands: vec![
            DesignDimensionAnnotationOperand {
                geometry_record_index: 0,
                geometry_reference_offset: 0,
                role: 3,
                role_offset: 0,
            },
            DesignDimensionAnnotationOperand {
                geometry_record_index: 10,
                geometry_reference_offset: 0,
                role: 2,
                role_offset: 0,
            },
        ],
        entity_genesis: 0x80,
        annotation_bytes: Vec::new(),
        annotation_byte_offset: 0,
        governing_owner_record_index: 13,
        governing_owner_reference_offset: 0,
        return_members: vec![10],
        return_member_offsets: vec![0],
        paired_class_tag: "256".into(),
        paired_byte_offset: 0,
        owner_reference: 100,
        owner_reference_offset: 0,
    };
    let parameter_id = ParameterId("generated:parameter#offset".into());
    let projected = HashMap::from([((stream, 10), &source), ((stream, 11), &result)]);

    let definition = crate::design::dimensions::annotation_offset_dimension_definition(
        &frame,
        &parameter,
        &parameter_id,
        stream,
        &[source_curve, result_curve],
        &projected,
        1.0e-6,
    )
    .expect("single-curve annotation offset");
    assert!(matches!(
        definition,
        SketchConstraintDefinition::Offset {
            pairs,
            distance: Length(distance),
            parameter: Some(actual_parameter),
            parameter_factor: Some(1.0),
        } if pairs.as_slice() == [cadmpeg_ir::sketches::SketchOffsetPair {
            source: source.id.clone(),
            result: result.id.clone(),
            source_reversed: true,
        }] && (distance - 2.0).abs() <= 1.0e-9
            && actual_parameter == parameter_id
    ));

    let duplicate_curve_id = format!("{stream}:sketch-curve#12");
    let duplicate_curve = curve(duplicate_curve_id.clone(), 12, 22, 8);
    let duplicate = entity(
        "duplicate",
        duplicate_curve_id,
        Point2::new(2.0, -2.0),
        Point2::new(8.0, -2.0),
    );
    let projected_with_duplicate = HashMap::from([
        ((stream, 10), &source),
        ((stream, 11), &result),
        ((stream, 12), &duplicate),
    ]);
    assert!(
        crate::design::dimensions::annotation_offset_dimension_definition(
            &frame,
            &parameter,
            &parameter_id,
            stream,
            &[
                curve(format!("{stream}:sketch-curve#10"), 10, 20, 0),
                curve(format!("{stream}:sketch-curve#11"), 11, 21, 7),
                duplicate_curve
            ],
            &projected_with_duplicate,
            1.0e-6,
        )
        .is_none()
    );
}

#[test]
fn angular_point_operand_selects_unique_incident_line_by_value() {
    let entity = |id: &str, geometry: SketchGeometry| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let point = entity(
        "generated:point#vertex",
        SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    );
    let explicit = entity(
        "generated:line#explicit",
        SketchGeometry::Line {
            start: Point2::new(2.0, -2.0),
            end: Point2::new(2.0, 2.0),
        },
    );
    let diagonal = entity(
        "generated:line#diagonal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 2.0),
        },
    );
    let horizontal = entity(
        "generated:line#horizontal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        },
    );
    let projected = HashMap::from([
        (("native", 1), &point),
        (("native", 2), &explicit),
        (("native", 3), &diagonal),
        (("native", 4), &horizontal),
    ]);

    let lines = indirect_angular_lines(
        "native",
        &[&point, &explicit],
        std::f64::consts::FRAC_PI_4,
        &projected,
    )
    .unwrap();
    assert_eq!(lines, (diagonal.id.clone(), explicit.id.clone()));
    let supplementary = indirect_angular_lines(
        "native",
        &[&point, &explicit],
        3.0 * std::f64::consts::FRAC_PI_4,
        &projected,
    )
    .unwrap();
    assert_eq!(supplementary, lines);
}

#[test]
fn parallel_group_binds_one_common_axis_angle() {
    let line = |id: &str, end: Point2| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end,
        },
    };
    let first = line("generated:line#first", Point2::new(1.0, 1.0));
    let second = line("generated:line#second", Point2::new(-2.0, -2.0));
    let mismatch = line("generated:line#mismatch", Point2::new(1.0, 0.0));
    let crossed = line("generated:line#crossed", Point2::new(1.0, -1.0));
    let parameter = parse_design_parameter(&parameter_record(
        Some(44),
        "45 deg",
        "Angular Dimension-2",
        Some("deg"),
        "d1",
        std::f64::consts::FRAC_PI_4,
    ))
    .expect("generated angular dimension is canonical");
    let parameter_id = ParameterId("generated:parameter#axis-angle".into());

    assert!(matches!(
        crate::design::dimensions::parallel_group_axis_angle_definition(
            &[&first, &second],
            &parameter,
            &parameter_id,
        ),
        Some(SketchConstraintDefinition::AngleToAxis {
            entity,
            axis: SketchAxis::Horizontal,
            parameter,
        }) if entity == first.id && parameter == parameter_id
    ));
    assert!(
        crate::design::dimensions::parallel_group_axis_angle_definition(
            &[&first, &mismatch],
            &parameter,
            &parameter_id,
        )
        .is_none()
    );
    assert!(
        crate::design::dimensions::parallel_group_axis_angle_definition(
            &[&first, &crossed],
            &parameter,
            &parameter_id,
        )
        .is_none()
    );
}

#[test]
fn dimension_proofs_require_the_evaluated_measurement() {
    let dimension = |source_kind: &str, unit: &str| {
        parse_design_parameter(&parameter_record(
            Some(44),
            "value",
            source_kind,
            Some(unit),
            "d1",
            1.0,
        ))
        .expect("generated dimension parameter is canonical")
    };
    assert!(crate::design::feature_project::design_dimension_unit(
        &dimension("Linear Dimension-2", "mm")
    ));
    assert!(crate::design::feature_project::design_dimension_unit(
        &dimension("Radial Dimension-3", "mm")
    ));
    assert!(!crate::design::feature_project::design_dimension_unit(
        &dimension("Linear Dimension-2", "deg")
    ));
    assert!(crate::design::feature_project::design_dimension_unit(
        &dimension("Angular Dimension-2", "rad")
    ));
    assert!(!crate::design::feature_project::design_dimension_unit(
        &dimension("Angular Dimension-2", "mm")
    ));
    assert!(!crate::design::feature_project::design_dimension_unit(
        &dimension("Radius Dimension-2", "native-unit")
    ));

    let entity = |id: &str, geometry: SketchGeometry| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let first = entity(
        "generated:point#0",
        SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    );
    let second = entity(
        "generated:point#1",
        SketchGeometry::Point {
            position: Point2::new(40.0, 0.0),
        },
    );
    let parameter = cadmpeg_ir::features::ParameterId("generated:parameter#0".into());
    assert!(crate::design::dimensions::directional_point_dimension(
        &[&first, &second],
        10.0,
        parameter.clone(),
    )
    .is_none());
    assert!(matches!(
        crate::design::dimensions::directional_point_dimension(&[&first, &second], 40.0, parameter),
        Some(SketchConstraintDefinition::HorizontalDistance { .. })
    ));

    let horizontal = entity(
        "generated:line#horizontal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
    );
    let diagonal = entity(
        "generated:line#diagonal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 10.0),
        },
    );
    assert!(!crate::design::dimensions::parallel_line_separation(
        &horizontal,
        &diagonal,
        2.0,
    ));
    assert!(!crate::design::dimensions::line_angle_matches(
        &horizontal.geometry,
        &diagonal.geometry,
        std::f64::consts::FRAC_PI_6,
    ));
    assert!(crate::design::dimensions::line_angle_matches(
        &horizontal.geometry,
        &diagonal.geometry,
        std::f64::consts::FRAC_PI_4,
    ));
    let vertical = entity(
        "generated:line#vertical",
        SketchGeometry::Line {
            start: Point2::new(0.0, -10.0),
            end: Point2::new(0.0, 10.0),
        },
    );
    let offset_point = entity(
        "generated:point#offset",
        SketchGeometry::Point {
            position: Point2::new(2.0, 4.0),
        },
    );
    assert!(crate::design::dimensions::point_line_separation(
        &offset_point,
        &vertical,
        2.0
    ));
    assert!(!crate::design::dimensions::point_line_separation(
        &vertical,
        &offset_point,
        3.0
    ));

    let inner_circle = entity(
        "generated:circle#inner",
        SketchGeometry::Circle {
            center: Point2::new(3.0, -2.0),
            radius: cadmpeg_ir::features::Length(4.0),
        },
    );
    let outer_circle = entity(
        "generated:circle#outer",
        SketchGeometry::Circle {
            center: Point2::new(3.0, -2.0),
            radius: cadmpeg_ir::features::Length(4.25),
        },
    );
    assert!(crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &outer_circle,
        0.25,
    ));
    assert!(!crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &outer_circle,
        0.5,
    ));
    let displaced_circle = entity(
        "generated:circle#displaced",
        SketchGeometry::Circle {
            center: Point2::new(3.001, -2.0),
            radius: cadmpeg_ir::features::Length(4.25),
        },
    );
    assert!(!crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &displaced_circle,
        0.25,
    ));
}

#[test]
fn counted_linear_graph_selects_one_parameter_backed_direction() {
    let entity = |id: &str, position| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let first = entity("generated:point#first", Point2::new(4.0, 16.0));
    let second = entity("generated:point#second", Point2::new(4.0, 14.0));
    let parameter = cadmpeg_ir::features::ParameterId("generated:parameter#distance".into());

    let definition =
        directional_point_dimension(&[&first, &second], 2.0, parameter.clone()).unwrap();
    assert!(matches!(
        definition,
        SketchConstraintDefinition::VerticalDistance {
            first: cadmpeg_ir::sketches::SketchLocus::Entity(ref first_id),
            second: cadmpeg_ir::sketches::SketchLocus::Entity(ref second_id),
            parameter: ref parameter_id,
        } if first_id == &first.id && second_id == &second.id && parameter_id == &parameter
    ));
    assert!(directional_point_dimension(&[&first, &second], 3.0, parameter).is_none());

    let diagonal = entity("generated:point#diagonal", Point2::new(7.0, 14.0));
    assert!(matches!(
        directional_point_dimension(
            &[&first, &diagonal],
            3.0,
            cadmpeg_ir::features::ParameterId("generated:parameter#horizontal".into()),
        ),
        Some(SketchConstraintDefinition::HorizontalDistance { .. })
    ));
    let square = entity("generated:point#square", Point2::new(6.0, 18.0));
    assert!(directional_point_dimension(
        &[&first, &square],
        2.0,
        cadmpeg_ir::features::ParameterId("generated:parameter#ambiguous".into()),
    )
    .is_none());
}

#[test]
fn unclassified_two_locus_linear_group_is_parameter_backed_distance() {
    let entity = |id: &str, geometry| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let point = entity(
        "generated:point#dimension",
        SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    );
    let line = entity(
        "generated:line#dimension",
        SketchGeometry::Line {
            start: Point2::new(-10.0, 0.0),
            end: Point2::new(-50.0, 0.0),
        },
    );
    let parameter = cadmpeg_ir::features::ParameterId("generated:parameter#distance".into());

    assert!(exact_counted_dimension_relation(&[&point, &line]).is_none());
    assert!(matches!(
        two_locus_distance_dimension(&[&point, &line], parameter.clone()),
        Some(SketchConstraintDefinition::Distance {
            ref entities,
            parameter: ref actual_parameter,
        }) if entities == &[point.id, line.id] && actual_parameter == &parameter
    ));
}

#[test]
fn counted_linear_graph_projects_exact_auxiliary_relations() {
    let entity = |id: &str, geometry| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let horizontal = entity(
        "generated:line#horizontal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
    );
    let vertical = entity(
        "generated:line#vertical",
        SketchGeometry::Line {
            start: Point2::new(0.0, -2.0),
            end: Point2::new(0.0, 2.0),
        },
    );
    let parallel = entity(
        "generated:line#parallel",
        SketchGeometry::Line {
            start: Point2::new(0.0, 2.0),
            end: Point2::new(10.0, 2.0),
        },
    );
    let point = entity(
        "generated:point#on-line",
        SketchGeometry::Point {
            position: Point2::new(4.0, 0.0),
        },
    );
    let duplicate_point = entity(
        "generated:point#duplicate",
        SketchGeometry::Point {
            position: Point2::new(4.0, 0.0),
        },
    );
    let arc = entity(
        "generated:arc#bounded",
        SketchGeometry::Arc {
            center: Point2::new(3.0, 0.0),
            radius: cadmpeg_ir::features::Length(1.0),
            start_angle: cadmpeg_ir::features::Angle(0.0),
            end_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
        },
    );
    let arc_start = entity(
        "generated:point#arc-start",
        SketchGeometry::Point {
            position: Point2::new(4.0, 0.0),
        },
    );
    let outside_arc = entity(
        "generated:point#outside-arc",
        SketchGeometry::Point {
            position: Point2::new(2.0, 0.0),
        },
    );

    assert!(matches!(
        exact_counted_dimension_relation(&[&horizontal, &vertical]),
        Some(SketchConstraintDefinition::Perpendicular { .. })
    ));
    assert!(matches!(
        exact_counted_dimension_relation(&[&horizontal, &parallel]),
        Some(SketchConstraintDefinition::Parallel { .. })
    ));
    assert!(matches!(
        exact_counted_dimension_relation(&[&horizontal, &point]),
        Some(SketchConstraintDefinition::Coincident { .. })
    ));
    assert!(matches!(
        exact_counted_dimension_relation(&[&point, &duplicate_point]),
        Some(SketchConstraintDefinition::Coincident { .. })
    ));
    assert!(matches!(
        exact_counted_dimension_relation(&[&arc_start, &arc]),
        Some(SketchConstraintDefinition::Coincident { .. })
    ));
    assert!(exact_counted_dimension_relation(&[&outside_arc, &arc]).is_none());
}

#[test]
fn exact_pair_suppresses_counted_frames_in_its_containing_companion() {
    let stream = "f3d:A";
    let placement = DesignSketchPlacement {
        member_run_head: false,
        id: format!("{stream}:design-sketch-placement#0"),
        scope_record_index: Some(10),
        entity_id: "0_100".into(),
        entity_suffix: 100,
        byte_offset: 0,
        class_tag: "356".into(),
        record_index: 11,
        frame_length: 201,
        transform: identity_matrix(),
        transform_offset: None,
        paired_class_tag: "259".into(),
        paired_byte_offset: 201,
    };
    let parameter = DesignParameter {
        id: format!("{stream}:design-parameter#20"),
        byte_offset: 0,
        class_tag: "305".into(),
        record_index: 20,
        family_discriminator: Some(0),
        family_discriminator_offset: Some(0),
        source_ordinal: 4,
        owner_record_index: Some(21),
        expression: "2 mm".into(),
        expression_offset: 0,
        source_kind: "Linear Dimension-4".into(),
        source_kind_offset: 0,
        kind: DesignParameterKind::Dimension,
        unit: Some("mm".into()),
        unit_offset: Some(0),
        name: "d4".into(),
        name_offset: 0,
        evaluated_value: 0.2,
        evaluated_value_offset: 0,
    };
    let owner = DesignParameterOwner {
        id: format!("{stream}:design-parameter-owner#21"),
        byte_offset: 0,
        class_tag: "292".into(),
        record_index: 21,
        scope_record_index: 10,
        local_ordinal: 0,
        evaluated_value: 0.2,
        evaluated_value_offset: 0,
        parameter_record_index: 20,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 22,
    };
    let companion = DesignParameterCompanion {
        id: format!("{stream}:design-parameter-companion#22"),
        byte_offset: 0,
        class_tag: "408".into(),
        record_index: 22,
        owner_record_index: 21,
        timestamp_micros: 1,
        timestamp_micros_offset: 42,
        payload_byte_offset: 58,
        payload_byte_length: 0,
        owned_recipe_ids: Vec::new(),
    };
    let pair = DesignDimensionLocusPair {
        id: format!("{stream}:design-dimension-locus-pair#30"),
        companion_record_index: 99,
        governing_companion_record_index: 22,
        byte_offset: 30,
        class_tag: "277".into(),
        record_index: 30,
        frame_length: 100,
        opaque_index: 0,
        opaque_index_offset: 65,
        first_geometry_record_index: 40,
        first_geometry_reference_offset: 70,
        first_role: 7,
        first_role_offset: 80,
        second_geometry_record_index: 41,
        second_geometry_reference_offset: 85,
        second_role: 8,
        second_role_offset: 95,
        paired_class_tag: "273".into(),
        paired_byte_offset: 130,
    };
    let group = DesignDimensionLocusGroup {
        id: format!("{stream}:design-dimension-locus-group#140"),
        companion_record_index: 99,
        byte_offset: 140,
        class_tag: "277".into(),
        record_index: 31,
        frame_length: 100,
        loci: vec![DesignDimensionLocus {
            geometry_record_index: 40,
            geometry_reference_offset: 170,
            role: 0,
            role_offset: 180,
        }],
        owner_reference: 100,
        owner_reference_offset: 185,
        owner_role: 0,
        owner_role_offset: 195,
        state: 0,
        state_offset: 199,
        constraint_kinds: Vec::new(),
        unknown_constraint_bits: 0,
        return_members: vec![40],
        return_member_offsets: vec![210],
        next_class_tag: "273".into(),
        next_record_index: 32,
        next_byte_offset: 240,
    };
    let point = |record_index, y| SketchPoint {
        id: format!("{stream}:sketch-point#{record_index}"),
        record_index,
        owner_reference: Some(100),
        class_tag: "300".into(),
        byte_offset: 0,
        coordinate_offset: 0,
        entity_genesis: None,
        persistent_id: u64::from(record_index),
        paired_reference: 0,
        coordinates: Point2::new(0.0, y),
        raw_bytes: Vec::new(),
    };
    let points = [point(40, 0.0), point(41, 2.0)];
    let sketch = neutral_sketch_id(&placement);
    let entities = points
        .iter()
        .map(|point| SketchEntity {
            id: SketchEntityId(format!("point-{}", point.record_index)),
            sketch: sketch.clone(),
            construction: false,
            native_ref: Some(point.id.clone()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: point.coordinates,
            },
        })
        .collect::<Vec<_>>();

    let constraints = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: std::slice::from_ref(&pair),
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );

    assert_eq!(constraints.len(), 1);
    assert!(matches!(
        constraints[0].definition,
        SketchConstraintDefinition::VerticalDistance { .. }
    ));

    let spatial_sketch = SpatialSketch {
        id: neutral_spatial_sketch_id(&placement),
        name: None,
        configuration: None,
        profiles: Vec::new(),
        native_ref: Some(placement.id.clone()),
    };
    let spatial_entities = points
        .iter()
        .map(|point| cadmpeg_ir::sketches::SpatialSketchEntity {
            id: cadmpeg_ir::sketches::SpatialSketchEntityId(format!(
                "spatial-point-{}",
                point.record_index
            )),
            sketch: spatial_sketch.id.clone(),
            construction: false,
            native_ref: Some(point.id.clone()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: cadmpeg_ir::sketches::SpatialSketchGeometry::Point {
                position: Point3::new(0.0, point.coordinates.v, 0.0),
            },
        })
        .collect::<Vec<_>>();
    assert!(project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: std::slice::from_ref(&pair),
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        std::slice::from_ref(&spatial_sketch),
    )
    .is_empty());
    let spatial_constraints = project_spatial_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: std::slice::from_ref(&pair),
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &[],
        },
        std::slice::from_ref(&spatial_sketch),
        &spatial_entities,
    );
    assert_eq!(spatial_constraints.len(), 1, "{spatial_constraints:#?}");
    assert!(matches!(
        &spatial_constraints[0],
        cadmpeg_ir::sketches::SpatialSketchConstraint {
            sketch: actual_sketch,
            definition: SpatialSketchConstraintDefinition::PointDistance {
                first,
                second,
                parameter: actual_parameter,
            },
            ..
        } if actual_sketch == &spatial_sketch.id
            && actual_parameter == &neutral_parameter_id_parts(stream, 20)
            && first == &spatial_entities[0].id
            && second == &spatial_entities[1].id
    ));

    let axis_record = SketchCurveIdentity {
        id: format!("{stream}:sketch-curve#42"),
        record_index: 42,
        owner_reference: Some(100),
        class_tag: "300".into(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: 42,
        secondary_id: 0,
        geometry: None,
    };
    let axis_entity = cadmpeg_ir::sketches::SpatialSketchEntity {
        id: cadmpeg_ir::sketches::SpatialSketchEntityId("spatial-axis".into()),
        sketch: spatial_sketch.id.clone(),
        construction: true,
        native_ref: Some(axis_record.id.clone()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: cadmpeg_ir::sketches::SpatialSketchGeometry::Line {
            start: Point3::new(-1.0, 1.0, 0.0),
            end: Point3::new(1.0, 1.0, 0.0),
        },
    };
    let symmetry_group = DesignDimensionLocusGroup {
        id: format!("{stream}:design-dimension-locus-group#31"),
        companion_record_index: 22,
        byte_offset: 0,
        class_tag: "277".into(),
        record_index: 31,
        frame_length: 100,
        loci: vec![
            DesignDimensionLocus {
                geometry_record_index: 42,
                geometry_reference_offset: 0,
                role: 5,
                role_offset: 0,
            },
            DesignDimensionLocus {
                geometry_record_index: 40,
                geometry_reference_offset: 0,
                role: 0,
                role_offset: 0,
            },
            DesignDimensionLocus {
                geometry_record_index: 41,
                geometry_reference_offset: 0,
                role: 0,
                role_offset: 0,
            },
        ],
        owner_reference: 100,
        owner_reference_offset: 0,
        owner_role: 0x400,
        owner_role_offset: 0,
        state: 0,
        state_offset: 0,
        constraint_kinds: Vec::new(),
        unknown_constraint_bits: 0,
        return_members: vec![40, 41, 42],
        return_member_offsets: vec![0, 0, 0],
        next_class_tag: "273".into(),
        next_record_index: 32,
        next_byte_offset: 0,
    };
    let mut symmetry_parameter = parameter.clone();
    symmetry_parameter.source_kind = "Linear Dimension-6".into();
    let mut symmetry_entities = spatial_entities.clone();
    symmetry_entities.push(axis_entity.clone());
    let symmetry_constraints = project_spatial_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&symmetry_parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: std::slice::from_ref(&symmetry_group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &points,
            curves: std::slice::from_ref(&axis_record),
            entities: &[],
        },
        std::slice::from_ref(&spatial_sketch),
        &symmetry_entities,
    );
    assert_eq!(symmetry_constraints.len(), 2, "{symmetry_constraints:#?}");
    assert!(symmetry_constraints.iter().any(|constraint| matches!(
        &constraint.definition,
        SpatialSketchConstraintDefinition::Symmetric {
            first,
            second,
            axis,
        } if first == &spatial_entities[0].id
            && second == &spatial_entities[1].id
            && axis == &axis_entity.id
    )));
    assert!(symmetry_constraints.iter().any(|constraint| matches!(
        constraint.definition,
        SpatialSketchConstraintDefinition::Native {
            parameter: Some(_),
            ..
        }
    )));

    let mut zero_parameter = parameter;
    zero_parameter.evaluated_value = 0.0;
    let mut duplicate_pair = pair.clone();
    duplicate_pair.second_geometry_record_index = duplicate_pair.first_geometry_record_index;
    let duplicate = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&zero_parameter),
            owners: std::slice::from_ref(&owner),
            pairs: std::slice::from_ref(&duplicate_pair),
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: &[],
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert_eq!(duplicate.len(), 1);
    assert!(matches!(
        duplicate[0].definition,
        SketchConstraintDefinition::Native { ref operands, .. }
            if operands.iter().map(|operand| (operand.native_field.as_deref(), operand.native_role, operand.object_index)).collect::<Vec<_>>()
                == [
                    (Some("first_locus"), Some(7), 40),
                    (Some("second_locus"), Some(8), 40),
                ]
    ));

    let mut group_owner = owner.clone();
    group_owner.companion_record_index = group.companion_record_index;
    let combined = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&zero_parameter),
            owners: &[owner, group_owner.clone()],
            pairs: std::slice::from_ref(&pair),
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: &[],
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert_eq!(combined.len(), 2);

    let grouped = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&zero_parameter),
            owners: std::slice::from_ref(&group_owner),
            pairs: &[],
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: &[],
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert!(matches!(
        grouped.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::Native {
                native_state: Some(0),
                native_flags: None,
                operands,
                ..
            },
            ..
        }] if operands.iter().map(|operand| (operand.native_field.as_deref(), operand.native_role, operand.object_index)).collect::<Vec<_>>()
            == [
                (Some("locus"), Some(0), 40),
                (Some("owner"), Some(0), 100),
                (Some("return"), None, 40),
            ]
    ));

    let mut indirect_group = group;
    indirect_group.owner_reference = 999;
    let indirect = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&zero_parameter),
            owners: std::slice::from_ref(&group_owner),
            pairs: &[],
            groups: std::slice::from_ref(&indirect_group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: &[],
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert_eq!(indirect.len(), 1);
    assert_eq!(indirect[0].sketch, sketch);
}

#[test]
fn counted_offset_return_run_pairs_sources_and_results() {
    let entity = |id: &str, start, end| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let bottom = entity(
        "generated:line#bottom",
        Point2::new(10.0, 0.0),
        Point2::new(0.0, 0.0),
    );
    let top = entity(
        "generated:line#top",
        Point2::new(0.0, 10.0),
        Point2::new(10.0, 10.0),
    );
    let inset_top = entity(
        "generated:line#inset-top",
        Point2::new(2.0, 8.0),
        Point2::new(8.0, 8.0),
    );
    let inset_bottom = entity(
        "generated:line#inset-bottom",
        Point2::new(8.0, 2.0),
        Point2::new(2.0, 2.0),
    );

    let entities = HashMap::from([(1, &bottom), (2, &top), (3, &inset_top), (4, &inset_bottom)]);
    let definition =
        exact_counted_offset(&[(1, 3), (2, 2), (3, 0), (4, 0)], &[1, 4, 2, 3], &entities)
            .expect("counted offset graph");
    let SketchConstraintDefinition::Offset {
        pairs,
        distance,
        parameter,
        parameter_factor,
    } = definition
    else {
        panic!("expected offset")
    };
    assert_eq!(pairs[0].source, bottom.id);
    assert_eq!(pairs[0].result, inset_bottom.id);
    assert_eq!(pairs[1].source, top.id);
    assert_eq!(pairs[1].result, inset_top.id);
    assert!((distance.0 - 2.0).abs() <= 1.0e-9);
    assert!(pairs.iter().all(|pair| pair.source_reversed));
    assert_eq!(parameter, None);
    assert_eq!(parameter_factor, None);
}

#[test]
fn counted_offset_accepts_concentric_arcs_with_the_same_sweep() {
    let arc = |id: &str, radius| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Arc {
            center: Point2::new(3.0, -4.0),
            radius: Length(radius),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::FRAC_PI_2),
        },
    };
    let source = arc("generated:arc#source", 2.0);
    let result = arc("generated:arc#result", 5.0);
    let entities = HashMap::from([(1, &source), (2, &result)]);

    let definition =
        exact_counted_offset(&[(1, 7), (2, 0)], &[1, 2], &entities).expect("concentric arc offset");
    assert!(matches!(
        definition,
        SketchConstraintDefinition::Offset {
            pairs,
            distance: Length(distance),
            ..
        } if pairs.len() == 1
            && pairs[0].source == source.id
            && pairs[0].result == result.id
            && pairs[0].source_reversed
            && (distance - 3.0).abs() <= 1.0e-9
    ));

    let mut mismatched = result;
    mismatched.geometry = SketchGeometry::Arc {
        center: Point2::new(3.0, -4.0),
        radius: Length(5.0),
        start_angle: Angle(0.0),
        end_angle: Angle(std::f64::consts::PI),
    };
    let entities = HashMap::from([(1, &source), (2, &mismatched)]);
    assert!(exact_counted_offset(&[(1, 7), (2, 0)], &[1, 2], &entities).is_none());
}

#[test]
fn counted_roles_require_matching_solved_geometry() {
    let line = |id: &str, start, end| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let horizontal = line(
        "generated:line#horizontal",
        Point2::new(-2.0, 3.0),
        Point2::new(5.0, 3.0),
    );
    let vertical = line(
        "generated:line#vertical",
        Point2::new(4.0, -1.0),
        Point2::new(4.0, 8.0),
    );

    assert!(matches!(
        counted_role_relation(&[&horizontal], 0x40),
        Some(SketchConstraintDefinition::Horizontal { entity })
            if entity == horizontal.id
    ));
    assert!(matches!(
        counted_role_relation(&[&vertical], 0x80),
        Some(SketchConstraintDefinition::Vertical { entity })
            if entity == vertical.id
    ));
    assert!(counted_role_relation(&[&horizontal], 0x80).is_none());
    assert!(counted_role_relation(&[&horizontal, &vertical], 0x40).is_none());

    let arc = cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId("generated:arc#tangent".into()),
        sketch: horizontal.sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Arc {
            center: Point2::new(-2.0, 2.0),
            radius: Length(1.0),
            start_angle: Angle(std::f64::consts::FRAC_PI_2),
            end_angle: Angle(std::f64::consts::PI),
        },
    };
    assert!(matches!(
        counted_role_relation(&[&arc, &horizontal], 0x100),
        Some(SketchConstraintDefinition::Tangent { first, second })
            if first == arc.id && second == horizontal.id
    ));

    let mut equal_arc = arc.clone();
    equal_arc.id = SketchEntityId("generated:arc#equal".into());
    assert!(matches!(
        counted_role_relation(&[&arc, &equal_arc], 0x800),
        Some(SketchConstraintDefinition::Equal { first, second })
            if first == arc.id && second == equal_arc.id
    ));
    if let SketchGeometry::Arc { radius, .. } = &mut equal_arc.geometry {
        *radius = Length(2.0);
    }
    assert!(counted_role_relation(&[&arc, &equal_arc], 0x800).is_none());
}

#[test]
fn offset_parameter_factor_preserves_curve_direction() {
    assert_eq!(offset_parameter_factor(2.0, 2.0), Some(1.0));
    assert_eq!(offset_parameter_factor(2.0, -2.0), Some(-1.0));
    assert_eq!(offset_parameter_factor(-2.0, 2.0), None);
    assert_eq!(offset_parameter_factor(2.0, 3.0), None);
    assert_eq!(offset_parameter_factor(f64::NAN, 2.0), None);
}

#[test]
fn paired_dimensions_bind_geometry_with_stream_local_record_indices() {
    let placement = |stream: &str, suffix| DesignSketchPlacement {
        member_run_head: false,
        id: format!("f3d:{stream}:design-sketch-placement#0"),
        scope_record_index: Some(10),
        entity_id: format!("0_{suffix}"),
        entity_suffix: suffix,
        byte_offset: 0,
        class_tag: "356".into(),
        record_index: 11,
        frame_length: 201,
        transform: identity_matrix(),
        transform_offset: None,
        paired_class_tag: "259".into(),
        paired_byte_offset: 201,
    };
    let owner = |stream: &str| DesignParameterOwner {
        id: format!("f3d:{stream}:design-parameter-owner#0"),
        byte_offset: 0,
        class_tag: "305".into(),
        record_index: 9,
        scope_record_index: 10,
        local_ordinal: 0,
        evaluated_value: 1.0,
        evaluated_value_offset: 40,
        parameter_record_index: 11,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 12,
    };
    let pair = |stream: &str| DesignDimensionLocusPair {
        id: format!("f3d:{stream}:design-dimension-locus-pair#0"),
        companion_record_index: 12,
        governing_companion_record_index: 12,
        byte_offset: 0,
        class_tag: "277".into(),
        record_index: 13,
        frame_length: 100,
        opaque_index: 0,
        opaque_index_offset: 35,
        first_geometry_record_index: 20,
        first_geometry_reference_offset: 40,
        first_role: 0,
        first_role_offset: 50,
        second_geometry_record_index: 21,
        second_geometry_reference_offset: 55,
        second_role: 0,
        second_role_offset: 65,
        paired_class_tag: "273".into(),
        paired_byte_offset: 100,
    };
    let point = |stream: &str, record_index| SketchPoint {
        id: format!("f3d:{stream}:sketch-point#{record_index}"),
        record_index,
        owner_reference: None,
        class_tag: "300".into(),
        byte_offset: 0,
        coordinate_offset: 89,
        entity_genesis: None,
        persistent_id: u64::from(record_index),
        paired_reference: 0,
        coordinates: Point2::new(0.0, 0.0),
        raw_bytes: Vec::new(),
    };
    let mut points = vec![
        point("A", 20),
        point("A", 21),
        point("B", 20),
        point("B", 21),
    ];

    bind_dimension_loci(
        &[placement("A", 100), placement("B", 200)],
        &[owner("A"), owner("B")],
        &[pair("A"), pair("B")],
        &[],
        &[],
        &[],
        &mut points,
        &mut [],
    )
    .unwrap();
    assert_eq!(
        points
            .iter()
            .map(|point| point.owner_reference)
            .collect::<Vec<_>>(),
        [Some(100), Some(100), Some(200), Some(200)]
    );
}

#[test]
fn recipe_backed_dimension_projects_disjoint_repeated_distance() {
    let stream = "f3d:A";
    let placement = DesignSketchPlacement {
        member_run_head: false,
        id: format!("{stream}:design-sketch-placement#0"),
        scope_record_index: Some(10),
        entity_id: "0_100".into(),
        entity_suffix: 100,
        byte_offset: 0,
        class_tag: "356".into(),
        record_index: 11,
        frame_length: 201,
        transform: identity_matrix(),
        transform_offset: None,
        paired_class_tag: "259".into(),
        paired_byte_offset: 201,
    };
    let parameter = DesignParameter {
        id: format!("{stream}:design-parameter#20"),
        byte_offset: 0,
        class_tag: "305".into(),
        record_index: 20,
        family_discriminator: Some(0),
        family_discriminator_offset: Some(0),
        source_ordinal: 4,
        owner_record_index: Some(21),
        expression: "thickness".into(),
        expression_offset: 0,
        source_kind: "Linear Dimension-4".into(),
        source_kind_offset: 0,
        kind: DesignParameterKind::Dimension,
        unit: Some("mm".into()),
        unit_offset: Some(0),
        name: "d4".into(),
        name_offset: 0,
        evaluated_value: 0.2,
        evaluated_value_offset: 0,
    };
    let owner = DesignParameterOwner {
        id: format!("{stream}:design-parameter-owner#21"),
        byte_offset: 0,
        class_tag: "292".into(),
        record_index: 21,
        scope_record_index: 10,
        local_ordinal: 0,
        evaluated_value: 0.2,
        evaluated_value_offset: 0,
        parameter_record_index: 20,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 22,
    };
    let companion = DesignParameterCompanion {
        id: format!("{stream}:design-parameter-companion#22"),
        byte_offset: 0,
        class_tag: "408".into(),
        record_index: 22,
        owner_record_index: 21,
        timestamp_micros: 1,
        timestamp_micros_offset: 0,
        payload_byte_offset: 58,
        payload_byte_length: 200,
        owned_recipe_ids: Vec::new(),
    };
    let recipe = |ordinal, record_index| DesignDimensionRecipeRecord {
        id: format!("{stream}:design-dimension-recipe-record#{record_index}"),
        companion_record_index: 22,
        recipe_ordinal: ordinal,
        recipe_id: format!("{stream}:construction-recipe#{record_index}"),
        recipe_kind: ConstructionRecipeKind::Edge,
        byte_offset: 0,
        class_tag: "423".into(),
        record_index,
        frame_length: 10,
        prefix_offset: 0,
        prefix_bytes: Vec::new(),
        references: Vec::new(),
        program_offset: 0,
        program: vec![-1],
        matching_edge_operand_ids: Vec::new(),
    };
    let sketch = neutral_sketch_id(&placement);
    let line = |name: &str, start, end| SketchEntity {
        id: SketchEntityId(name.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let entities = [
        line("first", Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)),
        line("second", Point2::new(0.0, 2.0), Point2::new(4.0, 2.0)),
        line("third", Point2::new(10.0, 0.0), Point2::new(10.0, 4.0)),
        line("fourth", Point2::new(12.0, 0.0), Point2::new(12.0, 4.0)),
    ];
    let constraints = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[recipe(1, 31), recipe(0, 30)],
            points: &[],
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    let [constraint] = constraints.as_slice() else {
        panic!("expected one recipe-backed dimension")
    };
    let SketchConstraintDefinition::RepeatedDistance {
        measurements,
        parameter: projected_parameter,
        ..
    } = &constraint.definition
    else {
        panic!("expected repeated recipe-backed dimension")
    };
    let expected_parameter = neutral_parameter_id_parts(stream, parameter.record_index).0;
    assert_eq!(&projected_parameter.0, &expected_parameter);
    assert_eq!(measurements.len(), 2);
    assert!(measurements.iter().all(|measurement| matches!(
        measurement,
        cadmpeg_ir::sketches::SketchDistanceMeasurement::Distance { .. }
    )));

    let mut radial_parameter = parameter.clone();
    radial_parameter.source_kind = "Radial Dimension-4".into();
    let circle = SketchEntity {
        id: SketchEntityId("radial-circle".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(20.0, 20.0),
            radius: Length(2.0),
        },
    };
    let mut radial_entities = entities.to_vec();
    radial_entities.push(circle.clone());
    let radial_constraints = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&radial_parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[recipe(0, 30)],
            points: &[],
            curves: &[],
            entities: &radial_entities,
        },
        &[],
    );
    assert!(matches!(
        radial_constraints.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::Radius {
                entity,
                parameter: actual_parameter,
            },
            ..
        }] if entity == &circle.id
            && actual_parameter == &neutral_parameter_id_parts(stream, parameter.record_index)
    ));

    let curves = [40, 41].map(|record_index| SketchCurveIdentity {
        id: format!("{stream}:sketch-curve#{record_index}"),
        record_index,
        owner_reference: Some(100),
        class_tag: "300".into(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: u64::from(record_index),
        secondary_id: 0,
        geometry: None,
    });
    let mut entities_with_refs = entities.clone();
    for (entity, curve) in entities_with_refs.iter_mut().zip(&curves) {
        entity.native_ref = Some(curve.id.clone());
    }
    let relation_group = DesignDimensionLocusGroup {
        id: format!("{stream}:design-dimension-locus-group#40"),
        companion_record_index: 22,
        byte_offset: 0,
        class_tag: "292".into(),
        record_index: 40,
        frame_length: 100,
        loci: vec![
            DesignDimensionLocus {
                geometry_record_index: 40,
                geometry_reference_offset: 0,
                role: 0,
                role_offset: 0,
            },
            DesignDimensionLocus {
                geometry_record_index: 41,
                geometry_reference_offset: 0,
                role: 0,
                role_offset: 0,
            },
        ],
        owner_reference: 100,
        owner_reference_offset: 0,
        owner_role: 0,
        owner_role_offset: 0,
        state: 0,
        state_offset: 0,
        constraint_kinds: vec![SketchConstraintKind::Parallel],
        unknown_constraint_bits: 0,
        return_members: vec![40, 41],
        return_member_offsets: vec![0, 0],
        next_class_tag: "300".into(),
        next_record_index: 42,
        next_byte_offset: 100,
    };
    let with_independent_relation = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: std::slice::from_ref(&relation_group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[recipe(1, 31), recipe(0, 30)],
            points: &[],
            curves: &curves,
            entities: &entities_with_refs,
        },
        &[],
    );
    assert_eq!(with_independent_relation.len(), 2);
    assert!(with_independent_relation.iter().any(|constraint| matches!(
        constraint.definition,
        SketchConstraintDefinition::RepeatedDistance { .. }
    )));
    assert!(with_independent_relation.iter().any(|constraint| matches!(
        constraint.definition,
        SketchConstraintDefinition::Parallel { .. }
    )));
    let mut radial_parameter = parameter.clone();
    radial_parameter.source_kind = "Radial Dimension-2".into();
    let radial_with_independent_relation = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&radial_parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: std::slice::from_ref(&relation_group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &[],
            curves: &curves,
            entities: &entities_with_refs,
        },
        &[],
    );
    assert_eq!(radial_with_independent_relation.len(), 2);
    assert!(radial_with_independent_relation
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Parallel { .. }
        )));
    assert!(radial_with_independent_relation
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Native {
                parameter: Some(_),
                ..
            }
        )));
    let without_dimension_frame = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: std::slice::from_ref(&relation_group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &[],
            curves: &curves,
            entities: &entities_with_refs,
        },
        &[],
    );
    assert_eq!(without_dimension_frame.len(), 2);
    assert!(without_dimension_frame.iter().any(|constraint| matches!(
        constraint.definition,
        SketchConstraintDefinition::Native {
            parameter: Some(_),
            ..
        }
    )));

    let mut incompatible_unit = parameter.clone();
    incompatible_unit.unit = Some("deg".into());
    let constraints = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&incompatible_unit),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[recipe(1, 31), recipe(0, 30)],
            points: &[],
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert!(matches!(
        constraints.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::Native { operands, .. },
            ..
        }] if operands.len() == 2
    ));

    let retained = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &[],
            curves: &[],
            entities: &[],
        },
        &[],
    );
    assert!(matches!(
        retained.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::Native {
                native_kind,
                native_state: None,
                native_flags: None,
                entities,
                parameter: Some(actual_parameter),
                operands,
                ..
            },
            native_ref: Some(native_ref),
            ..
        }] if native_kind == "Linear Dimension-4"
            && entities.is_empty()
            && actual_parameter.0 == expected_parameter
            && native_ref == &companion.id
            && matches!(operands.as_slice(), [cadmpeg_ir::sketches::SketchNativeOperand {
                native_kind,
                native_field: Some(field),
                native_role: None,
                object_index: 22,
                native_ref: Some(operand_ref),
            }] if native_kind == "dimension_companion"
                && field == "companion_payload"
                && operand_ref == &companion.id)
    ));

    let mut radial_parameter = parameter.clone();
    radial_parameter.source_kind = "Radial Dimension-2".into();
    let radial_entity = SketchEntity {
        id: SketchEntityId("circle".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
        },
    };
    let retained = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&radial_parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &[],
            curves: &[],
            entities: std::slice::from_ref(&radial_entity),
        },
        &[],
    );
    assert!(matches!(
        retained.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::Radius {
                entity,
                parameter: actual_parameter,
            },
            native_ref: Some(native_ref),
            ..
        }] if entity == &radial_entity.id
            && actual_parameter.0 == expected_parameter
            && native_ref == &companion.id
    ));

    let mut empty_companion = companion.clone();
    empty_companion.payload_byte_length = 0;
    let retained = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&empty_companion),
            recipe_records: &[],
            points: &[],
            curves: &[],
            entities: &[],
        },
        &[],
    );
    assert!(retained.is_empty());

    let line = SketchEntity {
        id: SketchEntityId("measured-line".into()),
        sketch,
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(3.0, 4.0),
            end: Point2::new(3.0, 6.0),
        },
    };
    let retained = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &[],
            curves: &[],
            entities: std::slice::from_ref(&line),
        },
        &[],
    );
    assert!(matches!(
        retained.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::DistanceLoci {
                first: cadmpeg_ir::sketches::SketchLocus::Start(first),
                second: cadmpeg_ir::sketches::SketchLocus::End(second),
                parameter: actual_parameter,
            },
            ..
        }] if first == &line.id
            && second == &line.id
            && actual_parameter == &neutral_parameter_id_parts(stream, parameter.record_index)
    ));

    let mut second_line = line.clone();
    second_line.id = SketchEntityId("second-measured-line".into());
    let retained = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&empty_companion),
            recipe_records: &[],
            points: &[],
            curves: &[],
            entities: &[line, second_line],
        },
        &[],
    );
    assert!(matches!(
        retained.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::RepeatedLength {
                entities,
                parameter,
            },
            ..
        }] if entities.len() == 2
            && parameter == &neutral_parameter_id_parts(stream, radial_parameter.record_index)
    ));
}

#[test]
fn recipe_dimension_requires_one_axis_aligned_point_pair() {
    let sketch = SketchId("sketch".into());
    let point = |name: &str, u, v| SketchEntity {
        id: SketchEntityId(name.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(u, v),
        },
    };
    let parameter = cadmpeg_ir::features::ParameterId("parameter".into());
    let mut entities = vec![
        point("first", -30.0, 2.0),
        point("second", -30.0, 0.0),
        point("unrelated", 10.0, 10.0),
    ];
    assert!(matches!(
        crate::design::dimensions::recipe_linear_dimension_candidates(
            &entities,
            &sketch,
            2.0,
            &parameter,
        ).as_slice(),
        [SketchConstraintDefinition::VerticalDistance { first, second, parameter: actual }]
            if *first == cadmpeg_ir::sketches::SketchLocus::Entity(SketchEntityId("first".into()))
                && *second == cadmpeg_ir::sketches::SketchLocus::Entity(SketchEntityId("second".into()))
                && *actual == parameter
    ));
    entities.push(point("ambiguous", 10.0, 8.0));
    let candidates = crate::design::dimensions::recipe_linear_dimension_candidates(
        &entities, &sketch, 2.0, &parameter,
    );
    assert_eq!(candidates.len(), 2);
    assert_eq!(
        crate::design::dimensions::recipe_dimension_candidate_entities(&candidates),
        [
            SketchEntityId("first".into()),
            SketchEntityId("second".into()),
            SketchEntityId("unrelated".into()),
            SketchEntityId("ambiguous".into()),
        ]
    );
}

#[test]
fn recipe_dimension_resolves_one_parallel_line_pair() {
    let sketch = SketchId("sketch".into());
    let line = |name: &str, start, end| SketchEntity {
        id: SketchEntityId(name.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let mut entities = vec![
        line("first", Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)),
        line("second", Point2::new(1.0, 2.0), Point2::new(5.0, 2.0)),
        line("unrelated", Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)),
    ];
    assert!(matches!(
        crate::design::dimensions::recipe_linear_dimension_candidates(
            &entities,
            &sketch,
            2.0,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
        ).as_slice(),
        [SketchConstraintDefinition::Distance { entities, .. }]
            if entities.as_slice() == [SketchEntityId("first".into()), SketchEntityId("second".into())]
    ));
    let point = |name: &str, position| SketchEntity {
        id: SketchEntityId(name.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let mut entities_with_endpoints = entities.clone();
    entities_with_endpoints.extend([
        point("first-start", Point2::new(0.0, 0.0)),
        point("first-end", Point2::new(4.0, 0.0)),
        point("second-start", Point2::new(1.0, 2.0)),
        point("second-end", Point2::new(5.0, 2.0)),
    ]);
    assert!(matches!(
        crate::design::dimensions::recipe_linear_dimension_candidates(
            &entities_with_endpoints,
            &sketch,
            2.0,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
        ).as_slice(),
        [SketchConstraintDefinition::Distance { entities, .. }]
            if entities.as_slice() == [SketchEntityId("first".into()), SketchEntityId("second".into())]
    ));

    let parameter = DesignParameter {
        id: "f3d:A:design-parameter#1".into(),
        byte_offset: 0,
        class_tag: "305".into(),
        record_index: 1,
        family_discriminator: Some(0),
        family_discriminator_offset: Some(0),
        source_ordinal: 1,
        owner_record_index: Some(2),
        expression: "2 mm".into(),
        expression_offset: 0,
        source_kind: "Linear Dimension-2".into(),
        source_kind_offset: 0,
        kind: DesignParameterKind::Dimension,
        unit: Some("mm".into()),
        unit_offset: Some(0),
        name: "d1".into(),
        name_offset: 0,
        evaluated_value: 0.2,
        evaluated_value_offset: 0,
    };
    assert!(matches!(
        crate::design::dimensions::unique_parallel_line_dimension_definition(
            &entities,
            &sketch,
            &parameter,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
        ),
        Some(SketchConstraintDefinition::Distance {
            entities,
            ..
        }) if entities.as_slice()
            == [SketchEntityId("first".into()), SketchEntityId("second".into())]
    ));

    let fragment = line(
        "second-fragment",
        Point2::new(7.0, 2.0),
        Point2::new(9.0, 2.0),
    );
    let mut fragmented_entities = entities.clone();
    fragmented_entities.push(fragment);
    fragmented_entities.push(line(
        "disjoint-first",
        Point2::new(20.0, 0.0),
        Point2::new(20.0, 1.0),
    ));
    fragmented_entities.push(line(
        "disjoint-second",
        Point2::new(22.0, 3.0),
        Point2::new(22.0, 4.0),
    ));
    assert!(matches!(
        crate::design::dimensions::owner_scoped_parallel_line_set_dimension_definition(
            &fragmented_entities,
            &sketch,
            &parameter,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::ParallelLineSetDistance {
            first,
            second,
            ..
        }) if first == vec![SketchEntityId("first".into())]
            && second == vec![
                SketchEntityId("second".into()),
                SketchEntityId("second-fragment".into()),
            ]
    ));

    let point = SketchEntity {
        id: SketchEntityId("point".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(0.0, 2.0),
        },
    };
    let mut point_entities = entities.clone();
    point_entities.push(point);
    assert!(matches!(
        crate::design::dimensions::unique_point_line_dimension_definition(
            &point_entities,
            &sketch,
            &parameter,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
        ),
        Some(SketchConstraintDefinition::Distance {
            entities,
            ..
        }) if entities.as_slice()
            == [SketchEntityId("point".into()), SketchEntityId("first".into())]
    ));

    entities.push(line("third", Point2::new(-1.0, 4.0), Point2::new(3.0, 4.0)));
    assert!(
        crate::design::dimensions::unique_parallel_line_dimension_definition(
            &entities,
            &sketch,
            &parameter,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
        )
        .is_none()
    );
    point_entities.push(entities.last().expect("third line").clone());
    assert!(
        crate::design::dimensions::unique_point_line_dimension_definition(
            &point_entities,
            &sketch,
            &parameter,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
        )
        .is_none()
    );
}

#[test]
fn concentric_circle_dimensions_require_disjoint_matching_pairs() {
    let sketch = SketchId("sketch".into());
    let parameter = DesignParameter {
        id: "f3d:A:design-parameter#1".into(),
        byte_offset: 0,
        class_tag: "305".into(),
        record_index: 1,
        family_discriminator: Some(0),
        family_discriminator_offset: Some(0),
        source_ordinal: 1,
        owner_record_index: Some(2),
        expression: "2 mm".into(),
        expression_offset: 0,
        source_kind: "Linear Dimension-2".into(),
        source_kind_offset: 0,
        kind: DesignParameterKind::Dimension,
        unit: Some("mm".into()),
        unit_offset: Some(0),
        name: "d1".into(),
        name_offset: 0,
        evaluated_value: 0.2,
        evaluated_value_offset: 0,
    };
    let circle = |name: &str, center, radius| SketchEntity {
        id: SketchEntityId(name.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center,
            radius: Length(radius),
        },
    };
    let mut circles = vec![
        circle("outer-a", Point2::new(0.0, 0.0), 5.0),
        circle("inner-a", Point2::new(0.0, 0.0), 3.0),
        circle("outer-b", Point2::new(20.0, 0.0), 8.0),
        circle("inner-b", Point2::new(20.0, 0.0), 6.0),
    ];
    let definition = crate::design::dimensions::concentric_circle_dimension_definition(
        &circles,
        &sketch,
        &parameter,
        &cadmpeg_ir::features::ParameterId("parameter".into()),
    )
    .expect("two disjoint concentric pairs");
    assert!(matches!(
        definition,
        SketchConstraintDefinition::RepeatedDistance {
            measurements,
            ..
        } if measurements == vec![
            cadmpeg_ir::sketches::SketchDistanceMeasurement::Distance {
                first: cadmpeg_ir::sketches::SketchLocus::Entity(
                    SketchEntityId("outer-a".into())
                ),
                second: cadmpeg_ir::sketches::SketchLocus::Entity(
                    SketchEntityId("inner-a".into())
                ),
            },
            cadmpeg_ir::sketches::SketchDistanceMeasurement::Distance {
                first: cadmpeg_ir::sketches::SketchLocus::Entity(
                    SketchEntityId("outer-b".into())
                ),
                second: cadmpeg_ir::sketches::SketchLocus::Entity(
                    SketchEntityId("inner-b".into())
                ),
            },
        ]
    ));

    circles.push(circle("overlap", Point2::new(0.0, 0.0), 1.0));
    assert!(
        crate::design::dimensions::concentric_circle_dimension_definition(
            &circles,
            &sketch,
            &parameter,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
        )
        .is_none()
    );
}

#[test]
fn design_streams_scope_sketch_graphs_identities_and_parameter_names() {
    let placement = |stream: &str| DesignSketchPlacement {
        member_run_head: false,
        id: format!("f3d:{stream}:design-sketch-placement#0"),
        scope_record_index: Some(10),
        entity_id: format!("{stream}_100"),
        entity_suffix: 100,
        byte_offset: 0,
        class_tag: "356".into(),
        record_index: 11,
        frame_length: 201,
        transform: identity_matrix(),
        transform_offset: None,
        paired_class_tag: "259".into(),
        paired_byte_offset: 201,
    };
    let header = |stream: &str| DesignEntityHeader {
        id: format!("f3d:{stream}:design-entity-header#0"),
        byte_offset: 0,
        entity_suffix: 100,
        entity_id: format!("{stream}_100"),
        class_tag: "300".into(),
        optional_slot_present: true,
        module: Some(DESIGN_MODULE_SKETCH.to_owned()),
        record_reference: None,
        record_reference_offset: None,
        declared_reference_count: Some(1),
        reference_indices: vec![30],
        reference_offsets: vec![0],
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
    };
    let point = |stream: &str| SketchPoint {
        id: format!("f3d:{stream}:sketch-point#0"),
        record_index: 20,
        owner_reference: None,
        class_tag: "301".into(),
        byte_offset: 0,
        coordinate_offset: 89,
        entity_genesis: None,
        persistent_id: 20,
        paired_reference: 0,
        coordinates: Point2::new(1.0, 2.0),
        raw_bytes: Vec::new(),
    };
    let relation = |stream: &str| SketchRelation {
        id: format!("f3d:{stream}:sketch-relation#30"),
        record_index: 30,
        class_tag: "302".into(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 100,
        owner_entity_id: String::new(),
        auxiliary_references: Vec::new(),
        auxiliary_reference_offsets: Vec::new(),
        members: vec![20],
        resolved_members: Vec::new(),
        member_offsets: vec![0],
        owner_reference_offset: 0,
        state: 0,
        constraint_kinds: vec![SketchConstraintKind::Coincident],
        unknown_constraint_bits: 0,
        member_relation_ordinals: Vec::new(),
        entity_genesis: None,
        pattern: None,
        return_members: vec![20],
        resolved_return_members: Vec::new(),
        return_member_offsets: vec![0],
        raw_bytes: Vec::new(),
    };

    let placements = [placement("A"), placement("B")];
    let mut points = [point("A"), point("B")];
    let mut relations = [relation("A"), relation("B")];
    bind_sketch_graph(
        &[header("A"), header("B")],
        &mut points,
        &mut [],
        &mut [],
        &mut relations,
    )
    .expect("stream-local sketch graphs bind independently");
    assert_eq!(relations[0].owner_entity_id, "A_100");
    assert_eq!(relations[1].owner_entity_id, "B_100");

    let mut overflowing_header = header("A");
    overflowing_header.entity_suffix = u64::from(u32::MAX) + 101;
    overflowing_header.entity_id = "A_overflow".into();
    assert!(bind_sketch_graph(
        &[overflowing_header],
        &mut [point("A")],
        &mut [],
        &mut [],
        &mut [relation("A")],
    )
    .is_err());

    let (mut sketches, mut entities) =
        project_sketch_design(&placements, &points, &[], &[], 1.0e-6);
    let mut constraints =
        project_sketch_constraints(&placements, &[], &points, &[], &[], &relations, &entities);
    assert_eq!(sketches.len(), 2);
    assert_eq!(entities.len(), 2);
    assert_eq!(constraints.len(), 2);
    assert_eq!(
        sketches
            .iter()
            .map(|item| &item.id)
            .collect::<HashSet<_>>()
            .len(),
        2
    );
    assert_eq!(
        entities
            .iter()
            .map(|item| &item.id)
            .collect::<HashSet<_>>()
            .len(),
        2
    );
    assert_eq!(
        constraints
            .iter()
            .map(|item| &item.id)
            .collect::<HashSet<_>>()
            .len(),
        2
    );

    let parameter = |stream: &str, record_index, name: &str, expression: &str| {
        let mut parameter = parse_design_parameter(&parameter_record(
            None,
            expression,
            "User Parameter",
            Some("mm"),
            name,
            1.0,
        ))
        .expect("generated user parameter is canonical");
        parameter.id = format!("f3d:{stream}:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let (_, parameters) = project_parameter_design(
        &[
            parameter("A", 40, "Width", "1 mm"),
            parameter("A", 41, "Half", "Width / 2"),
            parameter("B", 40, "Width", "2 mm"),
        ],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    let half = parameters
        .iter()
        .find(|parameter| parameter.name == "Half")
        .expect("projected Half parameter");
    let a_width = parameters
        .iter()
        .find(|parameter| {
            parameter.name == "Width"
                && parameter.native_ref.as_deref() == Some("f3d:A:parameter#40")
        })
        .expect("projected stream A Width parameter");
    assert_eq!(half.dependencies, std::slice::from_ref(&a_width.id));
    assert_eq!(
        parameters
            .iter()
            .map(|item| &item.id)
            .collect::<HashSet<_>>()
            .len(),
        3
    );

    for sketch in &mut sketches {
        sketch.native_ref = None;
    }
    for entity in &mut entities {
        entity.native_ref = None;
    }
    for constraint in &mut constraints {
        constraint.native_ref = None;
    }
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.sketches = sketches;
    ir.model.sketch_entities = entities;
    ir.model.sketch_constraints = constraints;
    ir.finalize();
    let report = cadmpeg_ir::validate::validate(&ir, Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn user_parameters_project_in_source_order_with_units_and_dependencies() {
    let mut width = parse_design_parameter(&parameter_record(
        None,
        "60 mm",
        "User Parameter",
        Some("mm"),
        "Width",
        6.0,
    ))
    .unwrap();
    width.id = "f3d:native:parameter#width".into();
    width.record_index = 20;
    width.source_ordinal = 4;
    let mut half = parse_design_parameter(&parameter_record(
        None,
        "Width / 2",
        "User Parameter",
        Some("mm"),
        "HalfWidth",
        3.0,
    ))
    .unwrap();
    half.id = "f3d:native:parameter#half".into();
    half.record_index = 21;
    half.source_ordinal = 5;

    let (features, projected) =
        project_parameter_design(&[half, width], &[], &[], &[], &[], &[], &[], &[]);
    assert!(features.is_empty());
    assert_eq!(projected[0].name, "Width");
    assert_eq!(projected[0].owner, None);
    assert_eq!(
        projected[0].value,
        Some(ParameterValue::Length(Length(60.0)))
    );
    assert_eq!(projected[1].dependencies, [projected[0].id.clone()]);
    assert_eq!(
        projected[1].native_ref.as_deref(),
        Some("f3d:native:parameter#half")
    );
}

#[test]
fn parameters_project_all_design_database_unit_tokens() {
    let mut native = ["mm", "cm", "m", "in", "ft", "deg", "rad"]
        .into_iter()
        .enumerate()
        .map(|(ordinal, unit)| {
            let mut parameter = parse_design_parameter(&parameter_record(
                None,
                "value",
                "User Parameter",
                Some(unit),
                &format!("Value{ordinal}"),
                1.25,
            ))
            .expect("generated database-unit parameter");
            parameter.id = format!("f3d:native:parameter#{ordinal}");
            parameter.record_index = u32::try_from(ordinal).unwrap();
            parameter.source_ordinal = u32::try_from(ordinal).unwrap();
            parameter
        })
        .collect::<Vec<_>>();
    native.reverse();
    let mut unclassified = parse_design_parameter(&parameter_record(
        None,
        "value",
        "User Parameter",
        Some("native-unit"),
        "Unclassified",
        2.75,
    ))
    .expect("generated unclassified-unit parameter");
    unclassified.id = "f3d:native:parameter#7".into();
    unclassified.record_index = 7;
    unclassified.source_ordinal = 7;
    native.push(unclassified);

    let (_, projected) = project_parameter_design(&native, &[], &[], &[], &[], &[], &[], &[]);
    for ordinal in 0..5 {
        assert_eq!(
            projected
                .iter()
                .find(|parameter| parameter.name == format!("Value{ordinal}"))
                .and_then(|parameter| parameter.value.clone()),
            Some(ParameterValue::Length(Length(12.5)))
        );
    }
    for ordinal in 5..7 {
        assert_eq!(
            projected
                .iter()
                .find(|parameter| parameter.name == format!("Value{ordinal}"))
                .and_then(|parameter| parameter.value.clone()),
            Some(ParameterValue::Angle(Angle(1.25)))
        );
    }
    let unclassified = projected
        .iter()
        .find(|parameter| parameter.name == "Unclassified")
        .expect("unclassified-unit parameter");
    assert_eq!(unclassified.value, None);
    assert_eq!(
        unclassified.properties.get("unit").map(String::as_str),
        Some("native-unit")
    );
    assert_eq!(
        unclassified
            .properties
            .get("evaluated_scalar")
            .map(String::as_str),
        Some("2.75")
    );
    assert_eq!(untyped_parameter_unit_count(&native), 1);
}

#[test]
fn expression_dependencies_preserve_fusion_parameter_name_symbols() {
    let name = "Width$µ°\"A";
    assert_eq!(
        expression_identifiers(&format!("{name} / 2 + sin(30 deg)")).collect::<Vec<_>>(),
        [name]
    );
    let parameter = |record_index, source_ordinal, expression: &str, name: &str| {
        let mut parameter = parse_design_parameter(&parameter_record(
            None,
            expression,
            "User Parameter",
            Some("mm"),
            name,
            1.0,
        ))
        .expect("generated symbolic-name parameter");
        parameter.id = format!("f3d:native:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = source_ordinal;
        parameter
    };
    let (_, projected) = project_parameter_design(
        &[
            parameter(20, 0, "10 mm", name),
            parameter(21, 1, "1", "sin"),
            parameter(22, 2, "1", "deg"),
            parameter(23, 3, "1", "mm"),
            parameter(24, 4, &format!("{name} / 2 + sin(30 deg) + 10 mm"), "Half"),
            parameter(25, 5, "mm + 1", "BareUnitName"),
        ],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    let source = projected
        .iter()
        .find(|parameter| parameter.name == name)
        .expect("symbolic-name source parameter");
    let half = projected
        .iter()
        .find(|parameter| parameter.name == "Half")
        .expect("dependent parameter");
    assert_eq!(half.dependencies, [source.id.clone()]);
    let millimetres = projected
        .iter()
        .find(|parameter| parameter.name == "mm")
        .expect("bare unit-named parameter");
    let bare_unit_name = projected
        .iter()
        .find(|parameter| parameter.name == "BareUnitName")
        .expect("consumer of bare unit-named parameter");
    assert_eq!(bare_unit_name.dependencies, [millimetres.id.clone()]);
}

#[test]
fn expression_dependency_audit_counts_only_unprojected_same_stream_names() {
    let parameter = |stream: &str, record_index, expression: &str, name: &str| {
        let mut parameter = parse_design_parameter(&parameter_record(
            None,
            expression,
            "User Parameter",
            Some("mm"),
            name,
            1.0,
        ))
        .expect("generated parameter");
        parameter.id = format!("f3d:{stream}:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let native = vec![
        parameter("A", 1, "1 mm", "Width"),
        parameter("A", 2, "Width + External", "Half"),
        parameter("B", 1, "1 mm", "External"),
    ];
    let (_, mut projected) = project_parameter_design(&native, &[], &[], &[], &[], &[], &[], &[]);
    assert_eq!(
        unresolved_parameter_expression_dependency_count(&native, &projected),
        0
    );

    projected
        .iter_mut()
        .find(|parameter| parameter.name == "Half")
        .expect("Half parameter")
        .dependencies
        .clear();
    assert_eq!(
        unresolved_parameter_expression_dependency_count(&native, &projected),
        1
    );
}

#[test]
fn owned_parameter_projects_under_its_real_scope_feature() {
    let mut parameter = parse_design_parameter(&parameter_record(
        Some(44),
        "60 mm",
        "AlongDistance",
        Some("mm"),
        "d12",
        6.0,
    ))
    .unwrap();
    parameter.id = "f3d:native:parameter#45".into();
    parameter.record_index = 45;
    let mut owner = parse_parameter_owner(&parameter_owner_frame()).unwrap();
    owner.id = "f3d:native:parameter-owner#44".into();
    let scope = DesignParameterScope {
        id: "f3d:native:parameter-scope#12".into(),
        byte_offset: 100,
        class_tag: "301".into(),
        record_index: 12,
        frame_length: 200,
        kind: "Extrude".into(),
        kind_offset: 210,
        extrude_prologue: Some(DesignExtrudePrologue::ReferenceAware {
            reference: None,
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 128,
            extent_discriminators: [1, 2],
            extent: DesignExtrudeExtent::OneSidedDistance,
            extent_discriminator_offsets: [132, 136],
            direction_reversed: false,
            direction_reversed_offset: 140,
            solid_operation: true,
            solid_operation_offset: 141,
            start: DesignExtrudeStart::ProfilePlane,
            start_offset: 142,
        }),
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: None,
        history_state_id_offset: 0,
        previous_history_state_id: None,
        previous_history_state_id_offset: 0,
        reference_count_offset: 180,
        reference_members: vec![44, 44],
        reference_member_offsets: vec![185, 196],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        unclosed_construction_operand_groups: Vec::new(),
        work_point_reference_type: None,
        work_point_input_record_indices: Vec::new(),
        extrude_profile: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: 300,
    };

    let (features, parameters) =
        project_parameter_design(&[parameter], &[owner], &[scope], &[], &[], &[], &[], &[]);
    assert_eq!(features.len(), 1);
    assert_eq!(features[0].name.as_deref(), Some("Extrude 1"));
    assert_eq!(features[0].suppressed, Some(true));
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Native { kind, parameters, properties }
            if kind == "Extrude"
                && parameters.get("d12").map(String::as_str) == Some("60 mm")
                && properties.get("reference:0").map(String::as_str) == Some("44")
                && properties.get("reference:1").map(String::as_str) == Some("44")
    ));
    assert_eq!(parameters[0].owner.as_ref(), Some(&features[0].id));
    assert_eq!(parameters[0].ordinal, 2);
    assert_eq!(
        parameters[0]
            .properties
            .get("source_kind")
            .map(String::as_str),
        Some("AlongDistance")
    );
}

#[test]
fn owned_parameter_without_a_projected_scope_stays_native_only() {
    let mut parameter = parse_design_parameter(&parameter_record(
        Some(44),
        "60 mm",
        "AlongDistance",
        Some("mm"),
        "d12",
        6.0,
    ))
    .unwrap();
    parameter.id = "f3d:native:parameter#45".into();
    parameter.record_index = 45;
    let mut owner = parse_parameter_owner(&parameter_owner_frame()).unwrap();
    owner.id = "f3d:native:parameter-owner#44".into();

    let (features, parameters) =
        project_parameter_design(&[parameter], &[owner], &[], &[], &[], &[], &[], &[]);
    assert!(features.is_empty());
    assert!(parameters.is_empty());
}

#[test]
fn parameter_dependencies_resolve_feature_scope_before_document_scope() {
    let parameter = |owner, record_index, expression: &str, name: &str| {
        let mut parameter = parse_design_parameter(&parameter_record(
            owner,
            expression,
            if owner.is_some() {
                "FeatureInput"
            } else {
                "User Parameter"
            },
            Some("mm"),
            name,
            1.0,
        ))
        .unwrap();
        parameter.id = format!("f3d:Design/BulkStream.dat:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let owner = |record_index, parameter_record_index, scope_record_index| DesignParameterOwner {
        id: format!("f3d:Design/BulkStream.dat:owner#{record_index}"),
        byte_offset: 0,
        class_tag: "292".into(),
        record_index,
        scope_record_index,
        local_ordinal: parameter_record_index,
        evaluated_value: 1.0,
        evaluated_value_offset: 0,
        parameter_record_index,
        owned_ordinal: parameter_record_index,
        variant: Some(0),
        companion_record_index: record_index + 1,
    };
    let scope = |record_index| DesignParameterScope {
        id: format!("f3d:Design/BulkStream.dat:scope#{record_index}"),
        byte_offset: u64::from(record_index),
        class_tag: "301".into(),
        record_index,
        frame_length: 100,
        kind: "CustomFeature".into(),
        kind_offset: 0,
        extrude_prologue: None,
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        feature_ordinal: record_index,
        feature_ordinal_offset: 0,
        history_state_id: None,
        history_state_id_offset: 0,
        previous_history_state_id: None,
        previous_history_state_id_offset: 0,
        reference_count_offset: 0,
        reference_members: Vec::new(),
        reference_member_offsets: Vec::new(),
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        unclosed_construction_operand_groups: Vec::new(),
        work_point_reference_type: None,
        work_point_input_record_indices: Vec::new(),
        extrude_profile: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "302".into(),
        paired_byte_offset: u64::from(record_index) + 100,
    };

    let document_width = parameter(None, 20, "60 mm", "Width");
    let local_width = parameter(Some(101), 21, "20 mm", "Width");
    let local_half = parameter(Some(102), 22, "Width / 2", "Half");
    let remote_half = parameter(Some(103), 23, "Width / 2", "Half");
    let owned_depth = parameter(Some(104), 24, "10 mm", "OwnedDepth");
    let document_half = parameter(None, 25, "OwnedDepth / 2", "DocumentHalf");
    let document_forward = parameter(None, 26, "Later / 2", "DocumentForward");
    let document_later = parameter(None, 27, "10 mm", "Later");
    let cycle_a = parameter(None, 28, "CycleB / 2", "CycleA");
    let cycle_b = parameter(None, 29, "CycleA / 2", "CycleB");
    let preceding_shared = parameter(Some(105), 30, "10 mm", "Shared");
    let shared_consumer = parameter(Some(106), 31, "Shared / 2", "SharedHalf");
    let later_shared = parameter(Some(107), 32, "20 mm", "Shared");
    let (_, parameters) = project_parameter_design(
        &[
            document_width,
            local_width,
            local_half,
            remote_half,
            owned_depth,
            document_half,
            document_forward,
            document_later,
            cycle_a,
            cycle_b,
            preceding_shared,
            shared_consumer,
            later_shared,
        ],
        &[
            owner(101, 21, 201),
            owner(102, 22, 201),
            owner(103, 23, 202),
            owner(104, 24, 201),
            owner(105, 30, 201),
            owner(106, 31, 202),
            owner(107, 32, 203),
        ],
        &[scope(201), scope(202), scope(203)],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    let by_name_and_owner = |name: &str, owner_record_index: u32| {
        parameters
            .iter()
            .find(|parameter| {
                parameter.name == name
                    && parameter.native_ref.as_deref()
                        == Some(
                            format!("f3d:Design/BulkStream.dat:parameter#{}", owner_record_index)
                                .as_str(),
                        )
            })
            .unwrap()
    };
    let document = by_name_and_owner("Width", 20);
    let local = by_name_and_owner("Width", 21);
    assert_eq!(
        by_name_and_owner("Half", 22).dependencies,
        [local.id.clone()]
    );
    assert_eq!(
        by_name_and_owner("Half", 23).dependencies,
        [document.id.clone()]
    );
    assert!(by_name_and_owner("DocumentHalf", 25)
        .dependencies
        .is_empty());
    let document_forward = by_name_and_owner("DocumentForward", 26);
    let document_later = by_name_and_owner("Later", 27);
    assert_eq!(document_forward.dependencies, [document_later.id.clone()]);
    assert!(document_later.ordinal < document_forward.ordinal);
    let cycle_a = by_name_and_owner("CycleA", 28);
    let cycle_b = by_name_and_owner("CycleB", 29);
    assert!(cycle_a.dependencies.is_empty());
    assert_eq!(cycle_b.dependencies, [cycle_a.id.clone()]);
    assert!(cycle_a.ordinal < cycle_b.ordinal);
    let preceding_shared = by_name_and_owner("Shared", 30);
    assert_eq!(
        by_name_and_owner("SharedHalf", 31).dependencies,
        [preceding_shared.id.clone()]
    );
}

#[test]
fn extrude_parameters_project_blind_two_sided_and_reversed_extents() {
    use cadmpeg_ir::features::{
        Angle, BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart,
        FaceSelection, ProfileRef, Termination,
    };

    let parameter = |source_kind: &str, unit: &str, value| {
        parse_design_parameter(&parameter_record(
            Some(44),
            "value",
            source_kind,
            Some(unit),
            "d1",
            value,
        ))
        .expect("generated feature parameter is canonical")
    };
    let mut scope = DesignParameterScope {
        id: "f3d:Design/BulkStream.dat:scope#12".into(),
        byte_offset: 100,
        class_tag: "301".into(),
        record_index: 12,
        frame_length: 200,
        kind: "Extrude".into(),
        kind_offset: 210,
        extrude_prologue: Some(DesignExtrudePrologue::ReferenceAware {
            reference: None,
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 128,
            extent_discriminators: [1, 2],
            extent: DesignExtrudeExtent::OneSidedDistance,
            extent_discriminator_offsets: [132, 136],
            direction_reversed: false,
            direction_reversed_offset: 140,
            solid_operation: true,
            solid_operation_offset: 141,
            start: DesignExtrudeStart::ProfilePlane,
            start_offset: 142,
        }),
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: None,
        history_state_id_offset: 0,
        previous_history_state_id: None,
        previous_history_state_id_offset: 0,
        reference_count_offset: 180,
        reference_members: vec![100],
        reference_member_offsets: vec![185],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        unclosed_construction_operand_groups: Vec::new(),
        work_point_reference_type: None,
        work_point_input_record_indices: Vec::new(),
        extrude_profile: Some(DesignSketchProfileOperand {
            scope_reference_ordinal: 0,
            record_index: 100,
            byte_offset: 300,
            class_tag: "308".into(),
            asset_id: "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b".into(),
            asset_id_offset: 330,
            entity_id: "0_172".into(),
            entity_suffix: 172,
            entity_reference_offset: 420,
            paired_class_tag: "259".into(),
            paired_byte_offset: 520,
        }),
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: 300,
    };
    let placement = DesignSketchPlacement {
        member_run_head: false,
        id: "f3d:Design/BulkStream.dat:placement#200".into(),
        scope_record_index: Some(11),
        entity_id: "0_172".into(),
        entity_suffix: 172,
        byte_offset: 600,
        class_tag: "300".into(),
        record_index: 200,
        frame_length: 329,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: Some(655),
        paired_class_tag: "260".into(),
        paired_byte_offset: 929,
    };
    let along = parameter("AlongDistance", "mm", 0.55);
    let taper = parameter("TaperAngle", "deg", 0.2);
    let blind = project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed blind Extrude");
    assert!(matches!(
        &blind,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(profile),
            direction: ExtrudeDirection::ProfileNormal,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind { length: Length(5.5) },
                    draft: Some(Angle(0.2)),
                    offset: None,
                },
            },
            op: BooleanOp::NewBody,
            solid: Some(true),
            ..
        } if profile == &neutral_sketch_id(&placement)
    ));
    let Some(DesignExtrudePrologue::ReferenceAware {
        solid_operation, ..
    }) = scope.extrude_prologue.as_mut()
    else {
        panic!("reference-aware Extrude prologue");
    };
    *solid_operation = false;
    let sheet = project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed sheet Extrude");
    assert!(matches!(
        sheet,
        FeatureDefinition::Extrude {
            solid: Some(false),
            ..
        }
    ));
    scope.extrude_prologue = Some(DesignExtrudePrologue::LegacyShifted {
        operation: DesignExtrudeOperation::NewBody,
        operation_offset: 127,
        direction_face_extend_values: [3, 2],
        side_extent_discriminators: [1, 0],
        side_extent_discriminator_offsets: [206, 210],
        extent: Some(DesignExtrudeExtent::SymmetricDistance),
        direction_face_extend_offsets: [131, 135],
        direction_reversed: false,
        direction_reversed_offset: 139,
        solid_operation: true,
        solid_operation_offset: 140,
        start: DesignExtrudeStart::ProfilePlane,
        start_offset: 141,
    });
    let symmetric = project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed symmetric Extrude");
    assert!(matches!(
        symmetric,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(5.5)
                    },
                    draft: Some(Angle(0.2)),
                    offset: None,
                },
            },
            ..
        }
    ));
    set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedThroughAll);
    set_extrude_direction_reversed(&mut scope, true);
    let through_all = project_extrude(
        &scope,
        &[(1, &taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed through-all Extrude");
    assert!(matches!(
        through_all,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ReversedProfileNormal,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ThroughAll,
                    draft: Some(Angle(0.2)),
                    offset: None,
                },
            },
            ..
        }
    ));
    set_extrude_direction_reversed(&mut scope, false);
    set_extrude_extent(&mut scope, DesignExtrudeExtent::SymmetricThroughAll);
    let symmetric_through_all = project_extrude(
        &scope,
        &[(1, &taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed symmetric through-all Extrude");
    assert!(matches!(
        symmetric_through_all,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ProfileNormal,
            extent: ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: Termination::ThroughAll,
                    draft: Some(Angle(0.2)),
                    offset: None,
                },
            },
            ..
        }
    ));
    set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedDistance);
    let selection = DesignExtrudeSelectionGroup {
        id: "f3d:Design/BulkStream.dat:selection#300".into(),
        scope_record_index: scope.record_index,
        scope_reference_ordinal: 0,
        record_index: 300,
        byte_offset: 700,
        class_tag: "308".into(),
        member_count_offset: 720,
        members: vec![301],
        member_offsets: vec![724],
        opaque_index: 1,
        opaque_index_offset: 735,
        opaque_scalar: 0.0,
        opaque_scalar_offset: 739,
        variant: false,
        paired_class_tag: "259".into(),
        paired_byte_offset: 760,
    };
    let mut feature = Feature {
        id: FeatureId("f3d:model:feature#extrude".into()),
        ordinal: 0,
        name: Some("Extrude".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some("Extrude".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: blind,
        native_ref: Some(scope.id.clone()),
    };
    bind_extrude_profile_selections(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        std::slice::from_ref(&selection),
        &[],
        &[],
        crate::design::profile_select::ExtrudeProfileResolution {
            entities: &[],
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: &[],
            linear_tolerance: 1.0e-6,
            angular_tolerance: 1.0e-9,
        },
    );
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Native(ref native),
            ..
        } if native == &selection.id
    ));
    set_extrude_direction_reversed(&mut scope, true);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    set_extrude_direction_reversed(&mut scope, false);
    let unsupported = parameter("UnclassifiedControl", "mm", 1.0);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &unsupported)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    let side_two_taper = parameter("Side2TaperAngle", "deg", -0.3);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &side_two_taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    let invalid_taper = parameter("TaperAngle", "native-unit", 0.2);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &invalid_taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    let mut owned_along = along.clone();
    owned_along.id = "f3d:Design/BulkStream.dat:parameter#45".into();
    owned_along.record_index = 45;
    owned_along.owner_record_index = Some(44);
    let mut owner = parse_parameter_owner(&parameter_owner_frame())
        .expect("generated parameter owner is canonical");
    owner.id = "f3d:Design/BulkStream.dat:owner#44".into();
    owner.record_index = 44;
    owner.scope_record_index = scope.record_index;
    owner.parameter_record_index = owned_along.record_index;
    let mut sketch_scope = scope.clone();
    sketch_scope.id = "f3d:Design/BulkStream.dat:scope#11".into();
    sketch_scope.record_index = placement
        .scope_record_index
        .expect("test placement carries a scope record index");
    sketch_scope.kind = "Sketch".into();
    sketch_scope.extrude_prologue = None;
    sketch_scope.extrude_profile = None;
    let scopes = [sketch_scope, scope.clone()];
    let (mut features, _) = project_parameter_design(
        std::slice::from_ref(&owned_along),
        std::slice::from_ref(&owner),
        &scopes,
        &[],
        &[],
        &[],
        &[],
        std::slice::from_ref(&placement),
    );
    let sketches = [cadmpeg_ir::sketches::Sketch {
        id: neutral_sketch_id(&placement),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: Some(placement.id.clone()),
    }];
    crate::design::feature_project::bind_sketch_feature_geometry(
        &mut features,
        &scopes,
        std::slice::from_ref(&placement),
        &sketches,
        &[],
    );
    let sketch_feature = features
        .iter()
        .find(|feature| matches!(feature.definition, FeatureDefinition::Sketch { .. }))
        .expect("neutral Sketch feature");
    let extrude_feature = features
        .iter()
        .find(|feature| matches!(feature.definition, FeatureDefinition::Extrude { .. }))
        .expect("neutral Extrude feature");
    assert_eq!(extrude_feature.dependencies, [sketch_feature.id.clone()]);

    let (mut spatial_features, _) = project_parameter_design(
        std::slice::from_ref(&owned_along),
        std::slice::from_ref(&owner),
        &scopes,
        &[],
        &[],
        &[],
        &[],
        std::slice::from_ref(&placement),
    );
    let spatial_sketch = cadmpeg_ir::sketches::SpatialSketch {
        id: neutral_spatial_sketch_id(&placement),
        name: None,
        configuration: None,
        profiles: vec![cadmpeg_ir::sketches::SpatialSketchProfile {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            boundary: Vec::new(),
        }],
        native_ref: Some(placement.id.clone()),
    };
    crate::design::feature_project::bind_sketch_feature_geometry(
        &mut spatial_features,
        &scopes,
        std::slice::from_ref(&placement),
        &[],
        std::slice::from_ref(&spatial_sketch),
    );
    let spatial_feature = spatial_features
        .iter()
        .find(|feature| matches!(feature.definition, FeatureDefinition::SpatialSketch { .. }))
        .expect("neutral spatial Sketch feature");
    let spatial_extrude = spatial_features
        .iter()
        .find(|feature| matches!(feature.definition, FeatureDefinition::Extrude { .. }))
        .expect("spatial-profile Extrude feature");
    assert!(matches!(
        spatial_extrude.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::SpatialSketchProfiles {
                ref sketch,
                ref profiles
            },
            ..
        } if sketch == &spatial_sketch.id && profiles == &[0]
    ));
    assert_eq!(spatial_extrude.dependencies, [spatial_feature.id.clone()]);

    let body_group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#101".into(),
        scope_record_index: 12,
        scope_reference_ordinal: 1,
        record_index: 101,
        byte_offset: 1000,
        class_tag: "332".into(),
        members: vec![200],
        lost_edge_references: Vec::new(),
        member_offsets: vec![1026],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 1021,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![300],
            trailing_record_offsets: vec![1044],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 180,
            opaque_index_offset: 1072,
            opaque_scalar: 0.125,
            opaque_scalar_offset: 1076,
            variant: false,
        },
        role: 0x0000_0008_0000_0000,
        extrude_role: Some(DesignExtrudeOperandRole::Bodies),
        extrude_face_role: None,
        role_offset: 1054,

        paired_class_tag: "259".into(),
        paired_byte_offset: 1125,
    };
    set_extrude_operation(&mut scope, DesignExtrudeOperation::Join);
    let target_body = project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        std::slice::from_ref(&body_group),
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed target-body Extrude");
    assert!(matches!(
        target_body,
        FeatureDefinition::Extrude {
            op: BooleanOp::Join,
            ..
        }
    ));

    set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedToFace);
    let mut target_shape_group = body_group.clone();
    target_shape_group.id = "f3d:Design/BulkStream.dat:operand-group#105".into();
    target_shape_group.record_index = 105;
    target_shape_group.scope_reference_ordinal = 2;
    target_shape_group.members = vec![201];
    target_shape_group.member_offsets = vec![1026];
    target_shape_group.role = 0x0000_0005_0000_0000;
    target_shape_group.extrude_role = None;
    let target_shape_operand = DesignBodyRecipeOperand {
        id: "f3d:Design/BulkStream.dat:body-recipe-operand#201".into(),
        scope_record_index: scope.record_index,
        owner: DesignBodyRecipeOperandOwner::Group {
            group_record_index: target_shape_group.record_index,
            group_member_ordinal: 0,
        },
        record_index: 201,
        byte_offset: 0,
        class_tag: "295".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        references: vec![DesignBodyRecipeReference {
            design_reference: 301,
            design_reference_offset: 0,
            form: 33,
            form_offset: 0,
            candidate_faces: vec![
                FaceId("f3d:brep:entity#12".into()),
                FaceId("f3d:brep:entity#19".into()),
            ],
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 204,
        nested_record_index_offset: 0,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#205".into(),
        resolved_face_slot: None,
        resolved_body_slot: None,
        next_record_index: 205,
        next_byte_offset: 0,
    };
    let target_shape = project_extrude(
        &scope,
        &[(0, &taper)],
        &[body_group.clone(), target_shape_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        std::slice::from_ref(&target_shape_operand),
    )
    .expect("typed target-shape Extrude");
    assert!(matches!(
        target_shape,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ToShape {
                        target: FaceSelection::Resolved { faces, ref native },
                    },
                    ..
                },
            },
            ..
        } if faces == [
            FaceId("f3d:brep:entity#12".into()),
            FaceId("f3d:brep:entity#19".into()),
        ] && native == &target_shape_group.id
    ));

    set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedDistance);
    set_extrude_operation(&mut scope, DesignExtrudeOperation::NewBody);
    let sketch_profile = scope.extrude_profile.clone();
    scope.extrude_profile = None;
    let mut first_profile_group = body_group.clone();
    first_profile_group.id = "f3d:Design/BulkStream.dat:operand-group#102".into();
    first_profile_group.record_index = 102;
    first_profile_group.scope_reference_ordinal = 0;
    first_profile_group.extrude_role = Some(DesignExtrudeOperandRole::Profile);
    let mut second_profile_group = first_profile_group.clone();
    second_profile_group.id = "f3d:Design/BulkStream.dat:operand-group#103".into();
    second_profile_group.record_index = 103;
    second_profile_group.scope_reference_ordinal = 1;
    let multiple_profiles = project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[first_profile_group.clone(), second_profile_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed multi-profile Extrude");
    assert!(matches!(
        multiple_profiles,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Native(ref native),
            op: BooleanOp::NewBody,
            ..
        } if native == &scope.id
    ));
    second_profile_group.scope_reference_ordinal = 0;
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[first_profile_group, second_profile_group],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    scope.extrude_profile = sketch_profile;
    set_extrude_operation(&mut scope, DesignExtrudeOperation::Join);

    let mut profile_group = body_group.clone();
    profile_group.id = "f3d:Design/BulkStream.dat:operand-group#104".into();
    profile_group.record_index = 104;
    profile_group.extrude_role = Some(DesignExtrudeOperandRole::Profile);
    profile_group.role = 0x0000_0041_0000_0000;
    let direct_profile_with_selection_group = project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[body_group.clone(), profile_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("direct sketch profile with a scoped selection group");
    assert!(matches!(
        direct_profile_with_selection_group,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(ref profile),
            ..
        } if profile == &neutral_sketch_id(&placement)
    ));
    scope.fixed_extrude_parameters = Some(DesignFixedExtrudeParameters {
        along_distance: Some(DesignFixedExtrudeDistance::DistanceConstruction(
            DesignFixedExtrudeScalar {
                value: 0.55,
                record_index: 105,
                value_offset: 600,
            },
        )),
        taper_angle: None,
    });
    let zero_side_offset = parameter("Side1Offset", "mm", 0.0);
    let hybrid = project_extrude(
        &scope,
        &[(0, &zero_side_offset), (1, &taper)],
        &[body_group.clone(), profile_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed hybrid fixed-distance Extrude");
    assert!(matches!(
        hybrid,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(5.5)
                    },
                    offset: None,
                    ..
                },
            },
            ..
        }
    ));
    set_extrude_direction_reversed(&mut scope, true);
    let reversed_hybrid = project_extrude(
        &scope,
        &[(0, &zero_side_offset), (1, &taper)],
        &[body_group.clone(), profile_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed reversed hybrid fixed-distance Extrude");
    assert!(matches!(
        reversed_hybrid,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ReversedProfileNormal,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(5.5)
                    },
                    ..
                },
            },
            ..
        }
    ));
    set_extrude_direction_reversed(&mut scope, false);
    scope.fixed_extrude_parameters = None;
    let mut native_profile_scope = scope.clone();
    native_profile_scope.extrude_profile = None;
    let reversed_native_profile = project_extrude(
        &native_profile_scope,
        &[(0, &parameter("AlongDistance", "mm", -0.2)), (1, &taper)],
        &[body_group.clone(), profile_group.clone()],
        &[],
        &[],
        &[],
    )
    .expect("typed reversed Extrude with a native profile");
    assert!(matches!(
        reversed_native_profile,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Native(ref native),
            direction: ExtrudeDirection::ReversedProfileNormal,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(2.0)
                    },
                    ..
                },
            },
            op: BooleanOp::Join,
            ..
        } if native == &profile_group.id
    ));

    let mut face_group = body_group.clone();
    face_group.id = "f3d:Design/BulkStream.dat:operand-group#102".into();
    face_group.extrude_role = Some(DesignExtrudeOperandRole::Faces);
    face_group.role = 0x0000_0011_0000_0000;
    let mut ordered_faces = [face_group.clone(), face_group.clone()];
    set_extrude_start(&mut scope, DesignExtrudeStart::FromFace);
    assign_extrude_face_roles(&scope, &mut ordered_faces);
    assert_eq!(
        ordered_faces.map(|group| group.extrude_face_role),
        [
            Some(DesignExtrudeFaceRole::Start),
            Some(DesignExtrudeFaceRole::Termination)
        ]
    );
    set_extrude_start(&mut scope, DesignExtrudeStart::ProfilePlane);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[body_group.clone(), face_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());

    let profile_offset = parameter("ProfileOffset", "mm", 0.1);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &profile_offset)],
        std::slice::from_ref(&body_group),
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    set_extrude_start(&mut scope, DesignExtrudeStart::OffsetProfilePlane);
    let offset_start = project_extrude(
        &scope,
        &[(0, &along), (1, &profile_offset)],
        std::slice::from_ref(&body_group),
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed offset-profile-plane Extrude");
    assert!(matches!(
        offset_start,
        FeatureDefinition::Extrude {
            start: ExtrudeStart::OffsetProfilePlane {
                offset: Length(1.0)
            },
            ..
        }
    ));
    set_extrude_start(&mut scope, DesignExtrudeStart::ProfilePlane);

    set_extrude_operation(&mut scope, DesignExtrudeOperation::NewBody);
    let against = parameter("AgainstDistance", "mm", -0.05);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &against)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    set_extrude_extent(&mut scope, DesignExtrudeExtent::TwoSidedDistance);
    let two_sided = project_extrude(
        &scope,
        &[(0, &along), (1, &against), (2, &side_two_taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed two-sided Extrude");
    assert!(matches!(
        two_sided,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(5.5)
                    },
                    ..
                },
                second: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(0.5)
                    },
                    draft: Some(Angle(-0.3)),
                    ..
                },
            },
            ..
        }
    ));
    set_extrude_direction_reversed(&mut scope, true);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &against), (2, &side_two_taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    set_extrude_direction_reversed(&mut scope, false);

    set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedDistance);
    let reversed_along = parameter("AlongDistance", "mm", -0.6);
    let reversed = project_extrude(
        &scope,
        &[(0, &reversed_along)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed reversed Extrude");
    assert!(matches!(
        reversed,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ReversedProfileNormal,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(6.0)
                    },
                    ..
                },
            },
            ..
        }
    ));

    set_extrude_operation(&mut scope, DesignExtrudeOperation::Join);
    set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedToFace);
    set_extrude_direction_reversed(&mut scope, true);
    face_group.extrude_face_role = Some(DesignExtrudeFaceRole::Termination);
    let side_offset = parameter("Side1Offset", "mm", 0.025);
    let to_face = project_extrude(
        &scope,
        &[(0, &side_offset), (1, &taper)],
        &[body_group.clone(), face_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed reversed to-face Extrude");
    assert!(matches!(
        to_face,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ReversedProfileNormal,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ToFace {
                        face: FaceSelection::Native(ref id),
                        offset: Some(Length(0.25)),
                    },
                    ..
                },
            },
            ..
        } if id == &face_group.id
    ));

    set_extrude_start(&mut scope, DesignExtrudeStart::FromFace);
    let mut start_group = face_group.clone();
    start_group.id = "f3d:Design/BulkStream.dat:operand-group#103".into();
    start_group.extrude_face_role = Some(DesignExtrudeFaceRole::Start);
    let from_face = project_extrude(
        &scope,
        &[
            (0, &parameter("ProfileOffset", "mm", 0.0)),
            (1, &side_offset),
            (2, &taper),
        ],
        &[body_group, start_group.clone(), face_group],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed selected-face start Extrude");
    assert!(matches!(
        from_face,
        FeatureDefinition::Extrude {
            start: ExtrudeStart::FromFace {
                face: FaceSelection::Native(ref id),
                offset: None,
            },
            ..
        } if id == &start_group.id
    ));

    set_extrude_operation(&mut scope, DesignExtrudeOperation::NewBody);
    set_extrude_extent(&mut scope, DesignExtrudeExtent::TwoSidedDistance);
    set_extrude_direction_reversed(&mut scope, false);
    let from_face_two_sided = project_extrude(
        &scope,
        &[
            (0, &parameter("ProfileOffset", "mm", 0.0)),
            (1, &along),
            (2, &against),
        ],
        &[start_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed selected-face-start two-sided Extrude");
    assert!(matches!(
        from_face_two_sided,
        FeatureDefinition::Extrude {
            start: ExtrudeStart::FromFace {
                face: FaceSelection::Native(ref id),
                offset: None,
            },
            extent: ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: Termination::Blind { length: Length(5.5) },
                    ..
                },
                second: ExtrudeSide {
                    termination: Termination::Blind { length: Length(0.5) },
                    ..
                },
            },
            ..
        } if id == &start_group.id
    ));
}

#[test]
fn edge_treatments_and_holes_project_typed_dimensions_and_native_selections() {
    use cadmpeg_ir::features::{ChamferGroup, ChamferSpec, EdgeSelection, RadiusSpec};

    let parameter = |owner_record_index,
                     record_index,
                     source_kind: &str,
                     name: &str,
                     expression: &str,
                     value| {
        let mut parameter = parse_design_parameter(&parameter_record(
            Some(owner_record_index),
            expression,
            source_kind,
            Some("mm"),
            name,
            value,
        ))
        .expect("generated feature parameter is canonical");
        parameter.id = format!("f3d:native:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let owner = |record_index, scope_record_index, parameter_record_index, local_ordinal| {
        let mut owner = parse_parameter_owner(&parameter_owner_frame())
            .expect("generated parameter owner is canonical");
        owner.id = format!("f3d:native:owner#{record_index}");
        owner.record_index = record_index;
        owner.scope_record_index = scope_record_index;
        owner.parameter_record_index = parameter_record_index;
        owner.companion_record_index = parameter_record_index + 1;
        owner.local_ordinal = local_ordinal;
        owner
    };
    let scope = |record_index, byte_offset, kind: &str| DesignParameterScope {
        id: format!("f3d:native:scope#{record_index}"),
        byte_offset,
        class_tag: "301".into(),
        record_index,
        frame_length: 200,
        kind: kind.into(),
        kind_offset: byte_offset + 100,
        extrude_prologue: None,
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: None,
        history_state_id_offset: 0,
        previous_history_state_id: None,
        previous_history_state_id_offset: 0,
        reference_count_offset: byte_offset + 80,
        reference_members: vec![record_index + 1],
        reference_member_offsets: vec![byte_offset + 85],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        unclosed_construction_operand_groups: Vec::new(),
        work_point_reference_type: None,
        work_point_input_record_indices: Vec::new(),
        extrude_profile: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: byte_offset + 200,
    };
    let scopes = [
        scope(12, 100, "Fillet"),
        scope(22, 400, "Chamfer"),
        scope(32, 700, "Hole"),
    ];
    let (features, _) = project_parameter_design(
        &[
            parameter(44, 45, "Radius", "d1", "5 mm", 0.5),
            parameter(54, 55, "Distance 1", "d2", "1 mm", 0.1),
            parameter(64, 65, "Distance 2", "d3", "2 mm", 0.2),
        ],
        &[
            owner(44, 12, 45, 0),
            owner(54, 22, 55, 0),
            owner(64, 22, 65, 1),
        ],
        &scopes,
        &[],
        &[],
        &[],
        &[],
        &[],
    );

    let fillet = features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("Fillet"))
        .expect("typed fillet");
    let FeatureDefinition::Fillet { groups } = &fillet.definition else {
        panic!("expected typed fillet");
    };
    assert!(matches!(
        groups.as_slice(),
        [cadmpeg_ir::features::FilletGroup {
            edges: EdgeSelection::Native(selection),
            radius: RadiusSpec::Constant { radius },
            tangency_weight: None,
        }] if selection == &scopes[0].id && radius.0 == 5.0
    ));
    let chamfer = features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("Chamfer"))
        .expect("typed chamfer");
    assert!(matches!(
        &chamfer.definition,
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [ChamferGroup {
                edges: EdgeSelection::Native(selection),
                spec: ChamferSpec::TwoDistances { first, second },
            }] if selection == &scopes[1].id && first.0 == 1.0 && second.0 == 2.0)
    ));

    let mut distance_angle_parameters = [
        parameter(54, 55, "Distance", "d2", "1.6 mm", 0.16),
        parameter(
            64,
            65,
            "Rotate Angle",
            "d3",
            "25 deg",
            25.0_f64.to_radians(),
        ),
    ];
    distance_angle_parameters[1].unit = Some("deg".into());
    let (features, _) = project_parameter_design(
        &distance_angle_parameters,
        &[owner(54, 22, 55, 0), owner(64, 22, 65, 1)],
        std::slice::from_ref(&scopes[1]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::DistanceAngle { distance, angle },
                ..
            }] if distance.0 == 1.6 && angle.0 == 25.0_f64.to_radians())
    ));

    let mut hole_parameters = [
        parameter(94, 95, "HoleDepth", "d4", "10 mm", 1.0),
        parameter(104, 105, "HoleDiameter", "d5", "4 mm", 0.4),
        parameter(114, 115, "TipAngle", "d6", "180 deg", std::f64::consts::PI),
    ];
    hole_parameters[2].unit = Some("deg".into());
    let (features, _) = project_parameter_design(
        &hole_parameters,
        &[
            owner(94, 32, 95, 0),
            owner(104, 32, 105, 1),
            owner(114, 32, 115, 2),
        ],
        std::slice::from_ref(&scopes[2]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Hole {
            face: Some(FaceSelection::Native(selection)),
            kind: cadmpeg_ir::features::HoleKind::Simple,
            diameter: Some(Length(4.0)),
            extent: Some(cadmpeg_ir::features::Termination::Blind { length: Length(10.0) }),
            bottom: Some(cadmpeg_ir::features::HoleBottom::Flat),
            ..
        } if selection == &scopes[2].id
    ));

    hole_parameters[2].evaluated_value = 118.0_f64.to_radians();
    let (features, _) = project_parameter_design(
        &hole_parameters,
        &[
            owner(94, 32, 95, 0),
            owner(104, 32, 105, 1),
            owner(114, 32, 115, 2),
        ],
        std::slice::from_ref(&scopes[2]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Hole {
            kind: cadmpeg_ir::features::HoleKind::SimpleDrilled { drill_point_angle },
            bottom: None,
            ..
        } if drill_point_angle.0 == 118.0_f64.to_radians()
    ));

    let mut counterbore_parameters = hole_parameters.to_vec();
    counterbore_parameters.extend([
        parameter(124, 125, "CBDepth", "d7", "3 mm", 0.3),
        parameter(134, 135, "CBDiameter", "d8", "8 mm", 0.8),
    ]);
    let (features, _) = project_parameter_design(
        &counterbore_parameters,
        &[
            owner(94, 32, 95, 0),
            owner(104, 32, 105, 1),
            owner(114, 32, 115, 2),
            owner(124, 32, 125, 3),
            owner(134, 32, 135, 4),
        ],
        std::slice::from_ref(&scopes[2]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Hole {
            kind: cadmpeg_ir::features::HoleKind::CounterboreDrilled {
                diameter: Length(8.0),
                depth: Length(3.0),
                drill_point_angle,
            },
            bottom: None,
            ..
        } if drill_point_angle.0 == 118.0_f64.to_radians()
    ));

    counterbore_parameters[2].evaluated_value = std::f64::consts::PI;
    let (features, _) = project_parameter_design(
        &counterbore_parameters,
        &[
            owner(94, 32, 95, 0),
            owner(104, 32, 105, 1),
            owner(114, 32, 115, 2),
            owner(124, 32, 125, 3),
            owner(134, 32, 135, 4),
        ],
        std::slice::from_ref(&scopes[2]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Hole {
            kind: cadmpeg_ir::features::HoleKind::Counterbore {
                diameter: Length(8.0),
                depth: Length(3.0),
            },
            bottom: Some(cadmpeg_ir::features::HoleBottom::Flat),
            ..
        }
    ));

    let (features, _) = project_parameter_design(
        &[
            parameter(54, 55, "leftDistance", "d2", "1 mm", 0.1),
            parameter(64, 65, "rightDistance", "d3", "2 mm", 0.2),
        ],
        &[owner(54, 22, 55, 0), owner(64, 22, 65, 1)],
        std::slice::from_ref(&scopes[1]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::TwoDistances { first, second },
                ..
            }] if first.0 == 1.0 && second.0 == 2.0)
    ));

    let (features, _) = project_parameter_design(
        &[parameter(54, 55, "leftDistance", "d2", "1 mm", 0.1)],
        &[owner(54, 22, 55, 0)],
        std::slice::from_ref(&scopes[1]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::Distance { distance },
                ..
            }] if distance.0 == 1.0)
    ));

    let (features, _) = project_parameter_design(
        &[
            parameter(44, 45, "Radius", "d1", "5 mm", 0.5),
            parameter(46, 47, "TangencyWeight", "w1", "0.5", 0.5),
        ],
        &[owner(44, 12, 45, 0), owner(46, 12, 47, 1)],
        std::slice::from_ref(&scopes[0]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Native { kind, parameters, .. }
            if kind == "Fillet" && parameters.len() == 2
    ));

    let (features, _) = project_parameter_design(
        &[parameter(44, 45, "Radius", "d1", "0 mm", 0.0)],
        &[owner(44, 12, 45, 0)],
        std::slice::from_ref(&scopes[0]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Native { kind, parameters, .. }
            if kind == "Fillet" && parameters.len() == 1
    ));

    let (features, _) = project_parameter_design(
        &[
            parameter(54, 55, "Distance 1", "d2", "1 mm", 0.1),
            parameter(64, 65, "Distance 2", "d3", "2 mm", 0.2),
            parameter(74, 75, "Distance", "d4", "3 mm", 0.3),
        ],
        &[
            owner(54, 22, 55, 0),
            owner(64, 22, 65, 1),
            owner(74, 22, 75, 2),
        ],
        std::slice::from_ref(&scopes[1]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Native { kind, parameters, .. }
            if kind == "Chamfer" && parameters.len() == 3
    ));

    let (features, _) = project_parameter_design(
        &[
            parameter(54, 55, "Distance 1", "d2", "0 mm", 0.0),
            parameter(64, 65, "Distance 2", "d3", "2 mm", 0.2),
        ],
        &[owner(54, 22, 55, 0), owner(64, 22, 65, 1)],
        std::slice::from_ref(&scopes[1]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Native { kind, parameters, .. }
            if kind == "Chamfer" && parameters.len() == 2
    ));

    let construction_group =
        |record_index, scope_reference_ordinal| DesignConstructionOperandGroup {
            id: format!("f3d:native:construction-group#{record_index}"),
            scope_record_index: 22,
            scope_reference_ordinal,
            record_index,
            byte_offset: 1_000 + u64::from(scope_reference_ordinal),
            class_tag: "288".into(),
            members: vec![record_index + 100],
            lost_edge_references: Vec::new(),
            member_offsets: vec![1_026 + u64::from(scope_reference_ordinal)],
            frame: crate::records::DesignConstructionOperandGroupFrame {
                member_count_offset: 1_021 + u64::from(scope_reference_ordinal),
                auxiliary_record_indices: Vec::new(),
                auxiliary_record_offsets: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_record_indices: vec![record_index + 1],
                trailing_record_offsets: vec![1_050 + u64::from(scope_reference_ordinal)],
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 100,
                opaque_index_offset: 1_068 + u64::from(scope_reference_ordinal),
                opaque_scalar: 0.5,
                opaque_scalar_offset: 1_072 + u64::from(scope_reference_ordinal),
                variant: false,
            },
            role: 0x0000_0008_0000_0000,
            extrude_role: None,
            extrude_face_role: None,
            role_offset: 1_060 + u64::from(scope_reference_ordinal),
            paired_class_tag: "259".into(),
            paired_byte_offset: 1_100 + u64::from(scope_reference_ordinal),
        };
    let mut construction_groups = [construction_group(90, 17), construction_group(80, 4)];
    construction_groups[1]
        .lost_edge_references
        .push("f3d:native:lost-edge-reference#1".into());
    let mut chamfer_scope = scopes[1].clone();
    chamfer_scope.previous_history_state_id = Some(21);
    let (features, _) = project_parameter_design(
        &[
            parameter(74, 75, "Distance", "d5", "2 mm", 0.2),
            parameter(84, 85, "Distance", "d4", "2.5 mm", 0.25),
        ],
        &[owner(74, 22, 75, 1), owner(84, 22, 85, 0)],
        std::slice::from_ref(&chamfer_scope),
        &construction_groups,
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [
                ChamferGroup {
                    edges: EdgeSelection::Unresolved,
                    spec: ChamferSpec::Distance { distance: Length(2.5) },
                },
                ChamferGroup {
                    edges: EdgeSelection::Native(selection),
                    spec: ChamferSpec::Distance { distance: Length(2.0) },
                },
            ] if selection == &construction_groups[0].id)
    ));
}

#[test]
fn edge_recipe_candidate_intersection_must_be_uniquely_corroborated() {
    use crate::records::{
        DesignEdgeRecipeSelectorContext, DesignTopologyIncidentSide, DesignTopologyRecipeEntry,
        DesignTopologyRecipeTriplet,
    };

    let selector = |selector, edges: &[i64]| DesignEdgeRecipeSelectorContext {
        selector,
        clause_entries: vec![None, None],
        clause_triplet_edge_slots: vec![None, None],
        incidence_matching_edge_slots: edges.to_vec(),
        unique_incidence_edge_slot: (edges.len() == 1).then(|| edges[0]),
        boundary_count_matching_edge_slots: Vec::new(),
    };
    let selector_with_counts = |ordinal: i32, incidence: &[i64], counts: &[i64]| {
        let mut context = selector(ordinal, incidence);
        context.boundary_count_matching_edge_slots = counts.to_vec();
        context
    };
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[selector(0, &[17, 18]), selector(1, &[17, 19])],
            [&[17, 20][..], &[15, 17][..]],
        ),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[selector(0, &[17, 18]), selector(1, &[17, 18])],
            [&[17, 18][..]],
        ),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[selector(0, &[17]), selector(1, &[18])],
            [&[17, 18][..]],
        ),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[selector(0, &[17]), selector(1, &[])], [&[17][..]],),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[selector(0, &[17])], [&[][..]]),
        None
    );
    assert_eq!(resolved_edge_candidate_intersection(&[], [&[17][..]]), None);
    // Two reference sets that share no edge name no edge of this operand, so a
    // proof drawn from outside those references cannot select one either.
    assert_eq!(
        resolved_edge_candidate_intersection_with_deleted_proofs(
            &[selector(0, &[17])],
            [&[17][..], &[18][..]],
            &[],
            Some(17),
        ),
        None
    );
    let mut deleted_triplet = selector(0, &[]);
    deleted_triplet.clause_triplet_edge_slots = vec![Some([vec![17, 19], vec![17, 20]]), None];
    assert_eq!(
        resolved_edge_candidate_intersection_with_deleted_proofs(
            &[deleted_triplet],
            [&[18][..], &[19][..]],
            &[17],
            None,
        ),
        Some(17)
    );
    // The same proof stands where the references do share the edge.
    assert_eq!(
        resolved_edge_candidate_intersection_with_deleted_proofs(
            &[selector(0, &[17])],
            [&[17, 20][..], &[17, 21][..]],
            &[],
            Some(17),
        ),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[
                selector_with_counts(0, &[17, 18], &[17, 19]),
                selector_with_counts(1, &[17, 20], &[17, 21]),
            ],
            std::iter::empty::<&[i64]>(),
        ),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[
                selector_with_counts(0, &[17, 18], &[17, 18]),
                selector_with_counts(1, &[17, 18], &[17, 18]),
            ],
            std::iter::empty::<&[i64]>(),
        ),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[
                selector_with_counts(0, &[17], &[18]),
                selector_with_counts(1, &[17], &[18]),
            ],
            std::iter::empty::<&[i64]>(),
        ),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[], [&[17, 18][..], &[17, 19][..]]),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[], [&[][..], &[17, 18][..], &[][..], &[17, 19][..]],),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[], [&[17, 18][..], &[17, 18][..]]),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[selector(0, &[18])], [&[17, 18][..], &[17, 19][..]],),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[
                selector_with_counts(0, &[], &[17, 18]),
                selector_with_counts(1, &[], &[17, 19]),
            ],
            [&[17, 20][..]],
        ),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[selector_with_counts(0, &[17], &[18])],
            [&[17, 18][..]],
        ),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[], &[17, 18])],
            [&[17][..]],
        ),
        Some(vec![17])
    );
    assert_eq!(
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[18], &[17, 18])],
            [&[17, 18][..]],
        ),
        Some(vec![18])
    );
    assert_eq!(
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[18], &[17, 18])],
            [&[17][..]],
        ),
        None
    );
    let assignment_candidates = [
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[], &[17, 18])],
            [&[17, 18][..]],
        )
        .unwrap(),
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[18], &[17, 18])],
            [&[17, 18][..]],
        )
        .unwrap(),
    ];
    assert_eq!(
        crate::design::edge_resolve::unique_bipartite_assignment(&assignment_candidates),
        Some(vec![17, 18])
    );
    let triplet = DesignTopologyRecipeTriplet {
        outer: std::num::NonZeroU32::new(3).unwrap(),
        middle: 2,
        vertex_ordinal: 2,
        incident_edge_ordinal: Some(1),
        incident_side: Some(DesignTopologyIncidentSide::Preceding),
    };
    let mut common = selector(0, &[]);
    common.clause_entries[0] = Some(DesignTopologyRecipeEntry {
        selector: 0,
        boundary_edge_count: std::num::NonZeroU32::new(4).unwrap(),
        topology_triplets: [triplet.clone(), triplet.clone()],
        common_incident_edge_ordinal: Some(1),
    });
    common.clause_triplet_edge_slots[0] = Some([vec![17, 18], vec![17]]);
    assert_eq!(
        resolved_edge_candidate_intersection(&[common.clone()], [&[17, 18][..]]),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[common], [&[][..]]),
        Some(17)
    );
    let mut common = selector(0, &[]);
    common.clause_entries[0] = Some(DesignTopologyRecipeEntry {
        selector: 0,
        boundary_edge_count: std::num::NonZeroU32::new(4).unwrap(),
        topology_triplets: [triplet.clone(), triplet],
        common_incident_edge_ordinal: Some(1),
    });
    common.clause_triplet_edge_slots[0] = Some([vec![17, 18, 19], vec![17, 18]]);
    assert_eq!(
        resolved_edge_candidate_intersection(&[common.clone()], [&[17][..]]),
        Some(17)
    );
    assert_eq!(
        crate::design::edge_resolve::corroborated_deleted_reference_candidate(
            &[common.clone()],
            [&[20][..], &[17][..]],
            &[17, 19],
        ),
        Some(17)
    );
    assert_eq!(
        crate::design::edge_resolve::corroborated_deleted_reference_candidate(
            &[common.clone()],
            [&[17][..], &[18][..]],
            &[17, 18],
        ),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::corroborated_deleted_reference_candidate(
            &[common.clone()],
            [&[17][..]],
            &[19],
        ),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_deleted_triplet_candidate(&[common.clone()], &[17, 20]),
        Some(17)
    );
    assert_eq!(
        crate::design::edge_resolve::resolved_edge_candidate_intersection_with_deleted_proofs(
            &[common.clone()],
            [&[17, 18][..]],
            &[17, 20],
            None,
        ),
        Some(17)
    );
    assert_eq!(
        crate::design::edge_resolve::resolved_edge_candidate_intersection_with_deleted_proofs(
            &[common.clone()],
            [&[18][..]],
            &[17, 20],
            None,
        ),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::resolved_edge_candidate_intersection_with_deleted_proofs(
            &[common.clone()],
            [&[17, 18][..]],
            &[17, 20],
            Some(18),
        ),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_deleted_triplet_candidate(&[common.clone()], &[17, 18]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_deleted_triplet_candidate(&[common.clone()], &[20]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_deleted_triplet_candidate(&[], &[17]),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[common], [&[19][..]]),
        None
    );
    let mut cross_clause = selector(0, &[]);
    cross_clause.clause_triplet_edge_slots =
        vec![Some([vec![18], vec![17, 19]]), Some([vec![20], vec![17]])];
    assert_eq!(
        resolved_edge_candidate_intersection(&[cross_clause.clone()], std::iter::empty::<&[i64]>(),),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[cross_clause.clone()], [&[17, 21][..]],),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[cross_clause.clone()], [&[18][..]]),
        None
    );
    cross_clause.clause_triplet_edge_slots =
        vec![Some([vec![18], vec![17]]), Some([vec![18], vec![17]])];
    assert_eq!(
        resolved_edge_candidate_intersection(&[cross_clause], std::iter::empty::<&[i64]>(),),
        None
    );
}

#[test]
fn edge_group_cardinality_resolves_one_common_deleted_candidate_set() {
    let selector = |candidates: &[i64]| crate::records::DesignEdgeRecipeSelectorContext {
        selector: 0,
        clause_entries: vec![None, None],
        clause_triplet_edge_slots: vec![None, None],
        incidence_matching_edge_slots: Vec::new(),
        unique_incidence_edge_slot: None,
        boundary_count_matching_edge_slots: candidates.to_vec(),
    };
    let first = [selector(&[19, 17, 18])];
    let context = [selector(&[])];
    let last = [selector(&[18, 19, 17])];
    assert_eq!(
        crate::design::edge_resolve::changed_boundary_count_edge_group_candidates([
            first.as_slice(),
            context.as_slice(),
            last.as_slice(),
        ]),
        Some(vec![17, 18, 19])
    );
    assert_eq!(
        crate::design::edge_resolve::changed_boundary_count_edge_group_candidates([
            first.as_slice(),
            last.as_slice(),
        ]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::changed_boundary_count_edge_group_candidates([
            first.as_slice(),
            context.as_slice(),
            &[],
        ]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates([
            (true, &[19, 17, 18, 17][..]),
            (true, &[18, 19, 17][..]),
            (true, &[17, 18, 19][..]),
        ],),
        Some(vec![17, 18, 19])
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates([
            (true, &[17, 18, 19][..]),
            (true, &[17, 18][..]),
            (true, &[17, 18, 19][..]),
        ],),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates([
            (true, &[17, 18, 19][..]),
            (true, &[17, 18, 19][..]),
        ]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates([
            (true, &[17, 18][..]),
            (false, &[][..]),
            (true, &[18, 17][..]),
        ]),
        Some(vec![17, 18])
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates(std::iter::empty::<(
            bool,
            &[i64]
        )>()),
        None
    );
    let deleted = vec![17, 18, 19, 20];
    let groups = vec![
        vec![
            (10, Some(17), deleted.clone()),
            (11, Some(19), deleted.clone()),
        ],
        vec![(12, None, deleted.clone()), (13, None, deleted.clone())],
    ];
    assert_eq!(
        crate::design::edge_resolve::partition_unique_incomplete_edge_group(1, &groups),
        Some(vec![18, 20])
    );
    assert_eq!(
        crate::design::edge_resolve::partition_unique_incomplete_edge_group(0, &groups),
        None
    );
    let mut two_incomplete = groups.clone();
    two_incomplete[0][0].1 = None;
    assert_eq!(
        crate::design::edge_resolve::partition_unique_incomplete_edge_group(1, &two_incomplete),
        None
    );
    let mut duplicate_identity = groups;
    duplicate_identity[1][0].0 = 11;
    assert_eq!(
        crate::design::edge_resolve::partition_unique_incomplete_edge_group(1, &duplicate_identity),
        None
    );
}

#[test]
fn edge_group_ignores_members_without_changed_edge_candidates() {
    assert_eq!(
        crate::design::edge_resolve::context_only_edge_group_candidates([
            (None, &[][..]),
            (Some(17), &[17, 18][..]),
            (Some(17), &[17][..]),
            (None, &[][..]),
        ]),
        Some(vec![17])
    );
    assert_eq!(
        crate::design::edge_resolve::context_only_edge_group_candidates([
            (Some(17), &[17][..]),
            (None, &[18][..]),
        ]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::context_only_edge_group_candidates([(None, &[][..])]),
        None
    );
}

#[test]
fn edge_group_resolves_only_one_perfect_candidate_assignment() {
    assert_eq!(
        crate::design::edge_resolve::edge_group_assignment_candidates(
            &[],
            [&[17, 18][..], &[18, 19][..], &[20][..]],
        ),
        Some(crate::design::edge_resolve::EdgeAssignmentCandidates::Edges(vec![18]))
    );
    assert_eq!(
        crate::design::edge_resolve::edge_group_assignment_candidates(&[], [&[][..], &[18][..]]),
        Some(crate::design::edge_resolve::EdgeAssignmentCandidates::Context)
    );
    assert_eq!(
        crate::design::edge_resolve::edge_group_assignment_candidates(&[], [&[17][..], &[18][..]]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::edge_group_assignment_candidates(&[], [&[17][..]]),
        Some(crate::design::edge_resolve::EdgeAssignmentCandidates::Context)
    );
    assert_eq!(
        crate::design::edge_resolve::unique_bipartite_assignment(&[
            vec![17, 18],
            vec![18, 19],
            vec![19],
        ]),
        Some(vec![17, 18, 19])
    );
    assert_eq!(
        crate::design::edge_resolve::unique_bipartite_assignment(&[vec![17, 18], vec![17, 18]]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_bipartite_assignment(&[vec![17], vec![17]]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_bipartite_assignment(&[vec![17], Vec::new()]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_bipartite_assignment(&[]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_edge_assignment_with_context(&[
            crate::design::edge_resolve::EdgeAssignmentCandidates::Edges(vec![17, 18]),
            crate::design::edge_resolve::EdgeAssignmentCandidates::Context,
            crate::design::edge_resolve::EdgeAssignmentCandidates::Edges(vec![18]),
        ]),
        Some(vec![17, 18])
    );
    assert_eq!(
        crate::design::edge_resolve::unique_edge_assignment_with_context(&[
            crate::design::edge_resolve::EdgeAssignmentCandidates::Context,
            crate::design::edge_resolve::EdgeAssignmentCandidates::Context,
        ]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_deleted_reference_assignment(
            &[vec![16, 17, 18], vec![19, 20, 21]],
            &[vec![17, 20, 22], vec![17, 20, 22]],
        ),
        Some(vec![17, 20])
    );
    assert_eq!(
        crate::design::edge_resolve::unique_deleted_reference_assignment(
            &[vec![16, 17, 18], vec![16, 17, 18]],
            &[vec![17, 18], vec![17, 18]],
        ),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_deleted_reference_assignment(&[vec![17]], &[vec![]],),
        None
    );
}

#[test]
fn variable_fillet_law_orders_endpoint_and_midpoint_parameters() {
    use cadmpeg_ir::features::Length;

    let parameter = |record_index, source_kind: &str, unit, value| {
        let mut parameter = parse_design_parameter(&parameter_record(
            Some(record_index + 100),
            "value",
            source_kind,
            unit,
            "d1",
            value,
        ))
        .expect("variable Fillet parameter");
        parameter.record_index = record_index;
        parameter
    };
    let start = parameter(1, "StartRadius", Some("mm"), 0.0);
    let end = parameter(2, "EndRadius", Some("mm"), 0.0);
    let radius = parameter(3, "MidRadius", Some("mm"), 0.4);
    let position = parameter(4, "MidParams", None, 0.25);
    let weight = parameter(5, "TangencyWeight", None, 0.75);
    let (points, tangency_weight) = crate::design::feature_project::variable_fillet_law(&[
        (0, &start),
        (1, &end),
        (2, &radius),
        (3, &position),
        (4, &weight),
    ])
    .expect("complete variable Fillet law");
    assert_eq!(
        points,
        [
            cadmpeg_ir::features::VariableRadius {
                parameter: 0.0,
                radius: Length(0.0),
            },
            cadmpeg_ir::features::VariableRadius {
                parameter: 0.25,
                radius: Length(4.0),
            },
            cadmpeg_ir::features::VariableRadius {
                parameter: 1.0,
                radius: Length(0.0),
            },
        ]
    );
    assert_eq!(tangency_weight, 0.75);
}

#[test]
fn localized_fillet_radius_parameters_pair_with_counted_edge_groups_in_order() {
    let scope = DesignParameterScope {
        id: "f3d:native:scope#12".into(),
        byte_offset: 100,
        class_tag: "301".into(),
        record_index: 12,
        frame_length: 200,
        kind: "Congé".into(),
        kind_offset: 210,
        extrude_prologue: None,
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: None,
        history_state_id_offset: 0,
        previous_history_state_id: None,
        previous_history_state_id_offset: 0,
        reference_count_offset: 180,
        reference_members: vec![100, 101],
        reference_member_offsets: vec![185, 196],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        unclosed_construction_operand_groups: Vec::new(),
        work_point_reference_type: None,
        work_point_input_record_indices: Vec::new(),
        extrude_profile: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: 300,
    };
    let group = |record_index, ordinal, members: Vec<u32>| DesignConstructionOperandGroup {
        id: format!("f3d:native:construction-group#{record_index}"),
        scope_record_index: 12,
        scope_reference_ordinal: ordinal,
        record_index,
        byte_offset: 1000 + u64::from(ordinal) * 200,
        class_tag: "288".into(),
        member_offsets: (0..members.len())
            .map(|index| 1026 + u64::from(ordinal) * 200 + index as u64 * 11)
            .collect(),
        members,
        lost_edge_references: Vec::new(),
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 1021 + u64::from(ordinal) * 200,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![300 + ordinal],
            trailing_record_offsets: vec![1100 + u64::from(ordinal) * 200],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 100,
            opaque_index_offset: 1128 + u64::from(ordinal) * 200,
            opaque_scalar: 0.5,
            opaque_scalar_offset: 1132 + u64::from(ordinal) * 200,
            variant: false,
        },
        role: 0x0000_0008_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 1110 + u64::from(ordinal) * 200,

        paired_class_tag: "259".into(),
        paired_byte_offset: 1200 + u64::from(ordinal) * 200,
    };
    let mut operand_groups = [group(100, 0, vec![200]), group(101, 1, vec![201, 202])];
    let parameter = |owner_index, record_index, source_kind: &str, unit, value| {
        let mut parameter = parse_design_parameter(&parameter_record(
            Some(owner_index),
            "value",
            source_kind,
            unit,
            "d1",
            value,
        ))
        .expect("canonical localized Fillet parameter");
        parameter.id = format!("f3d:native:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter
    };
    let owner = |record_index, parameter_record_index, local_ordinal| {
        let mut owner = parse_parameter_owner(&parameter_owner_frame()).unwrap();
        owner.id = format!("f3d:native:owner#{record_index}");
        owner.record_index = record_index;
        owner.scope_record_index = 12;
        owner.parameter_record_index = parameter_record_index;
        owner.local_ordinal = local_ordinal;
        owner
    };
    let parameters = [
        parameter(10, 11, "Radius", Some("mm"), 0.5),
        parameter(20, 21, "Radius", Some("mm"), 0.3),
        parameter(30, 31, "TangencyWeight", None, 1.0),
        parameter(40, 41, "TangencyWeight", None, 0.75),
    ];
    let owners = [
        owner(10, 11, 0),
        owner(20, 21, 1),
        owner(30, 31, 2),
        owner(40, 41, 3),
    ];
    let mut indexed_scope = scope.clone();
    indexed_scope.fixed_fillet_parameters = Some(crate::records::DesignFixedFilletParameters {
        groups: vec![crate::records::DesignFixedFilletGroup {
            tangency_weight: Some(crate::records::DesignFixedFilletTangencyWeight {
                value: 1.0,
                record_index: 10,
                value_offset: 100,
            }),
            radii: vec![0.5],
            radius_record_indexes: vec![20],
            radius_offsets: vec![200],
            intermediate_parameters: Vec::new(),
            intermediate_parameter_record_indexes: Vec::new(),
            intermediate_parameter_offsets: Vec::new(),
        }],
    });
    crate::design::decode::operands::disambiguate_fixed_fillet_parameters(
        std::slice::from_mut(&mut indexed_scope),
        &owners,
    );
    assert_eq!(indexed_scope.fixed_fillet_parameters, None);

    let assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups,
        &owners,
        &parameters,
    );
    assert_eq!(assignments.len(), 2);
    assert_eq!(assignments[0].edge_operand_record_indices, [200]);
    assert_eq!(
        assignments[0].law,
        crate::records::DesignFilletRadiusLaw::Constant {
            radius_parameter_record_index: 11,
        }
    );
    assert_eq!(
        assignments[0].tangency_weight_parameter_record_index,
        Some(31)
    );
    assert_eq!(assignments[1].edge_operand_record_indices, [201, 202]);
    assert_eq!(
        assignments[1].law,
        crate::records::DesignFilletRadiusLaw::Constant {
            radius_parameter_record_index: 21,
        }
    );
    assert_eq!(
        assignments[1].tangency_weight_parameter_record_index,
        Some(41)
    );
    let variable_parameters = [
        parameter(50, 51, "StartRadius", Some("mm"), 0.2),
        parameter(60, 61, "EndRadius", Some("mm"), 0.6),
        parameter(70, 71, "MidRadius", Some("mm"), 0.4),
        parameter(80, 81, "MidParams", None, 0.25),
        parameter(90, 91, "TangencyWeight", None, 0.75),
    ];
    let variable_owners = [
        owner(50, 51, 0),
        owner(60, 61, 1),
        owner(70, 71, 2),
        owner(80, 81, 3),
        owner(90, 91, 4),
    ];
    let variable_assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &variable_owners,
        &variable_parameters,
    );
    assert_eq!(variable_assignments.len(), 1);
    assert_eq!(
        variable_assignments[0].law,
        crate::records::DesignFilletRadiusLaw::Variable {
            start_radius_parameter_record_index: 51,
            end_radius_parameter_record_index: 61,
            middle_radius_parameter_record_indices: vec![71],
            middle_parameter_record_indices: vec![81],
        }
    );
    assert_eq!(
        variable_assignments[0].tangency_weight_parameter_record_index,
        Some(91)
    );
    let mut incomplete_parameters = variable_parameters.to_vec();
    incomplete_parameters.push(parameter(100, 101, "UnknownLawInput", None, 1.0));
    let mut incomplete_owners = variable_owners.to_vec();
    incomplete_owners.push(owner(100, 101, 5));
    assert!(decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &incomplete_owners,
        &incomplete_parameters,
    )
    .is_empty());
    let chord_parameters = [
        parameter(110, 111, "TangencyWeight", None, 1.0),
        parameter(120, 121, "ChordLen", Some("in"), 0.25),
    ];
    let chord_owners = [owner(110, 111, 0), owner(120, 121, 1)];
    let chord_assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &chord_owners,
        &chord_parameters,
    );
    assert_eq!(chord_assignments.len(), 1);
    assert_eq!(
        chord_assignments[0].law,
        crate::records::DesignFilletRadiusLaw::Chordal {
            chord_length_parameter_record_index: 121,
        }
    );
    let (chord_features, _) = project_parameter_design(
        &chord_parameters,
        &chord_owners,
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &chord_assignments,
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &chord_features[0].definition,
        FeatureDefinition::Fillet { groups }
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::FilletGroup {
                    radius: cadmpeg_ir::features::RadiusSpec::Chordal {
                        chord_length: cadmpeg_ir::features::Length(2.5),
                    },
                    tangency_weight: Some(1.0),
                    ..
                }]
            )
    ));
    let asymmetric_parameters = [
        parameter(130, 131, "TangencyWeight", None, 1.0),
        parameter(140, 141, "EdgeOffset1", Some("mm"), 0.2),
        parameter(150, 151, "EdgeOffset2", Some("mm"), 0.7),
    ];
    let asymmetric_owners = [owner(130, 131, 0), owner(140, 141, 1), owner(150, 151, 2)];
    let asymmetric_assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &asymmetric_owners,
        &asymmetric_parameters,
    );
    assert_eq!(asymmetric_assignments.len(), 1);
    assert_eq!(
        asymmetric_assignments[0].law,
        crate::records::DesignFilletRadiusLaw::Asymmetric {
            offset_one_parameter_record_index: 141,
            offset_two_parameter_record_index: 151,
        }
    );
    let (asymmetric_features, _) = project_parameter_design(
        &asymmetric_parameters,
        &asymmetric_owners,
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &asymmetric_assignments,
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        &asymmetric_features[0].definition,
        FeatureDefinition::Fillet { groups }
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::FilletGroup {
                    radius: cadmpeg_ir::features::RadiusSpec::Asymmetric {
                        offset_one: cadmpeg_ir::features::Length(2.0),
                        offset_two: cadmpeg_ir::features::Length(7.0),
                    },
                    tangency_weight: Some(1.0),
                    ..
                }]
            )
    ));
    operand_groups[0]
        .lost_edge_references
        .push("f3d:native:lost-edge-reference#1".into());

    let (features, _) = project_parameter_design(
        &parameters,
        &owners,
        std::slice::from_ref(&scope),
        &operand_groups,
        &assignments,
        &[],
        &[],
        &[],
    );
    let FeatureDefinition::Fillet { groups } = &features[0].definition else {
        panic!("expected typed localized Fillet");
    };
    assert_eq!(groups.len(), 2);
    assert!(matches!(
        &groups[0],
        cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(5.0),
            },
            tangency_weight: Some(1.0),
        }
    ));
    assert!(matches!(
        &groups[1],
        cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Native(selection),
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(3.0),
            },
            tangency_weight: Some(0.75),
        } if selection == &operand_groups[1].id
    ));

    let mut patch_scope = scope.clone();
    patch_scope.kind = "SurfacePatch".into();
    patch_scope.frame_length = 354;
    patch_scope.reference_members = vec![100, 200, 300, 301];
    let patch_boundary = |scope_reference_ordinal, record_index, model_reference| {
        crate::records::DesignSurfacePatchBoundary {
            scope_reference_ordinal,
            record_index,
            is_seed_selection: false,
            continuity: crate::records::DesignPatchContinuity::Connected,
            flip: 2,
            scale: -1.0,
            model_reference,
        }
    };
    patch_scope.surface_patch_boundaries = vec![patch_boundary(2, 300, 100)];
    let mut patch_group = group(100, 0, vec![200]);
    patch_group.role = 0x0000_0004_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &patch_scope,
            std::slice::from_ref(&patch_group),
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::Native(ref native)
            ),
            support_faces: cadmpeg_ir::features::FaceSelection::Faces(ref faces),
            continuity: Some(cadmpeg_ir::features::SurfaceContinuity::Contact),
            ref boundary_continuities,
            merge_result: None,
        }) if boundary_continuities
            == &[cadmpeg_ir::features::SurfaceContinuity::Contact]
            && native == &patch_group.id && faces.is_empty()
    ));

    patch_scope.frame_length = 398;
    patch_scope.reference_members = vec![100, 200, 300, 101, 201, 301, 102];
    patch_scope.surface_patch_boundaries =
        vec![patch_boundary(2, 300, 100), patch_boundary(5, 301, 101)];
    let mut second_patch_group = group(101, 3, vec![201]);
    second_patch_group.role = 0x0000_0004_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &patch_scope,
            &[patch_group.clone(), second_patch_group],
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::Native(ref native)
            ),
            ..
        }) if native == &patch_scope.id
    ));

    patch_scope.frame_length = 339;
    patch_scope.reference_members = vec![100, 200, 300];
    patch_scope.surface_patch_boundaries = vec![patch_boundary(2, 300, 100)];
    patch_group.role = 0x0000_0041_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &patch_scope,
            std::slice::from_ref(&patch_group),
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::Native(ref native)
            ),
            ..
        }) if native == &patch_group.id
    ));

    // The earlier scope-envelope generation is fourteen bytes shorter in both
    // forms and projects the same feature from the same reference shape.
    patch_scope.frame_length = 325;
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &patch_scope,
            std::slice::from_ref(&patch_group),
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface { .. })
    ));
    patch_scope.frame_length = 340;
    patch_scope.reference_members = vec![100, 200, 300, 301];
    patch_scope.surface_patch_boundaries = vec![patch_boundary(2, 300, 100)];
    patch_group.role = 0x0000_0004_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &patch_scope,
            std::slice::from_ref(&patch_group),
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface { .. })
    ));
    patch_scope.reference_members = vec![100, 200, 300, 301, 302];
    assert!(crate::design::feature_project::project_surface_patch(
        &patch_scope,
        std::slice::from_ref(&patch_group),
        &[],
        &[],
    )
    .is_none());

    patch_scope.frame_length = 343;
    patch_scope.reference_members = vec![100, 200, 201, 202, 203, 300];
    patch_scope.surface_patch_boundaries.clear();
    patch_group.members = vec![200, 201, 202, 203];
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &patch_scope,
            std::slice::from_ref(&patch_group),
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::Native(ref native)
            ),
            ..
        }) if native == &patch_group.id
    ));

    let mut fill_scope = scope.clone();
    fill_scope.kind = "BoundaryFill".into();
    fill_scope.reference_members = vec![100, 200, 201, 300, 301, 400];
    let mut tools = group(100, 0, vec![200, 201]);
    tools.role = 0x0000_0004_0000_0000;
    let mut cell = group(300, 3, vec![301]);
    cell.role = 0x0000_0005_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_boundary_fill(&fill_scope, &[tools.clone(), cell.clone()]),
        Some(FeatureDefinition::BoundaryFill {
            tools: cadmpeg_ir::features::BodySelection::Native(ref tool_selection),
            cells: ref cell_selections,
        }) if tool_selection == &tools.id
            && cell_selections == &[cadmpeg_ir::features::BodySelection::Native(cell.id)]
    ));
}

#[test]
fn parameter_expressions_project_feature_dependencies() {
    let parameter = |owner_record_index, record_index, name: &str, expression: &str| {
        let mut parameter = parse_design_parameter(&parameter_record(
            Some(owner_record_index),
            expression,
            "AlongDistance",
            Some("mm"),
            name,
            1.0,
        ))
        .expect("generated owned parameter is canonical");
        parameter.id = format!("f3d:native:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let owner = |record_index, scope_record_index, parameter_record_index| {
        let mut owner = parse_parameter_owner(&parameter_owner_frame())
            .expect("generated parameter owner is canonical");
        owner.id = format!("f3d:native:owner#{record_index}");
        owner.record_index = record_index;
        owner.scope_record_index = scope_record_index;
        owner.parameter_record_index = parameter_record_index;
        owner.companion_record_index = parameter_record_index + 1;
        owner
    };
    let scope = |record_index, byte_offset, kind: &str| DesignParameterScope {
        id: format!("f3d:native:scope#{record_index}"),
        byte_offset,
        class_tag: "301".into(),
        record_index,
        frame_length: 200,
        kind: kind.into(),
        kind_offset: byte_offset + 100,
        extrude_prologue: None,
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: None,
        history_state_id_offset: 0,
        previous_history_state_id: None,
        previous_history_state_id_offset: 0,
        reference_count_offset: byte_offset + 80,
        reference_members: vec![record_index + 1],
        reference_member_offsets: vec![byte_offset + 85],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        unclosed_construction_operand_groups: Vec::new(),
        work_point_reference_type: None,
        work_point_input_record_indices: Vec::new(),
        extrude_profile: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: byte_offset + 200,
    };
    let (features, parameters) = project_parameter_design(
        &[
            parameter(44, 45, "Width", "10 mm"),
            parameter(54, 55, "Depth", "Width / 2"),
            parameter(74, 75, "Premature", "Future / 2"),
            parameter(84, 85, "Future", "20 mm"),
        ],
        &[
            owner(44, 12, 45),
            owner(54, 22, 55),
            owner(74, 22, 75),
            owner(84, 32, 85),
        ],
        &[
            scope(12, 100, "Sketch"),
            scope(22, 200, "Extrude"),
            scope(32, 300, "Fillet"),
        ],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    let width = parameters
        .iter()
        .find(|parameter| parameter.name == "Width")
        .expect("Width parameter");
    let depth = parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .expect("Depth parameter");
    assert_eq!(depth.dependencies, std::slice::from_ref(&width.id));
    let premature = parameters
        .iter()
        .find(|parameter| parameter.name == "Premature")
        .expect("Premature parameter");
    assert!(premature.dependencies.is_empty());
    let source = features
        .iter()
        .find(|feature| feature.id == width.owner.clone().expect("Width owner"))
        .expect("source feature");
    let target = features
        .iter()
        .find(|feature| feature.id == depth.owner.clone().expect("Depth owner"))
        .expect("target feature");
    assert_eq!(target.dependencies, std::slice::from_ref(&source.id));
}

#[test]
fn history_state_identity_orders_cross_family_feature_dependencies() {
    let scope = |record_index, byte_offset, kind: &str, current, previous| DesignParameterScope {
        id: format!("f3d:native:scope#{record_index}"),
        byte_offset,
        class_tag: "301".into(),
        record_index,
        frame_length: 200,
        kind: kind.into(),
        kind_offset: byte_offset + 100,
        extrude_prologue: None,
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: current,
        history_state_id_offset: byte_offset + 60,
        previous_history_state_id: previous,
        previous_history_state_id_offset: byte_offset + 120,
        reference_count_offset: byte_offset + 80,
        reference_members: Vec::new(),
        reference_member_offsets: Vec::new(),
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        unclosed_construction_operand_groups: Vec::new(),
        work_point_reference_type: None,
        work_point_input_record_indices: Vec::new(),
        extrude_profile: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: byte_offset + 200,
    };
    let predecessor = scope(12, 200, "Fillet", Some(10), Some(9));
    let successor = scope(22, 100, "Chamfer", Some(11), Some(10));
    let parameter = |owner_record_index, record_index, expression: &str, name: &str| {
        let mut parameter = parse_design_parameter(&parameter_record(
            Some(owner_record_index),
            expression,
            "FeatureInput",
            Some("mm"),
            name,
            1.0,
        ))
        .expect("generated history-ordered parameter");
        parameter.id = format!("f3d:native:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let owner = |record_index, parameter_record_index, scope_record_index| DesignParameterOwner {
        id: format!("f3d:native:owner#{record_index}"),
        byte_offset: 0,
        class_tag: "292".into(),
        record_index,
        scope_record_index,
        local_ordinal: parameter_record_index,
        evaluated_value: 1.0,
        evaluated_value_offset: 0,
        parameter_record_index,
        owned_ordinal: parameter_record_index,
        variant: Some(0),
        companion_record_index: record_index + 1,
    };
    let (features, parameters) = project_parameter_design(
        &[
            parameter(44, 45, "10 mm", "Width"),
            parameter(54, 55, "Width / 2", "Depth"),
        ],
        &[owner(44, 45, 12), owner(54, 55, 22)],
        &[successor, predecessor],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    let predecessor = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("f3d:native:scope#12"))
        .expect("predecessor feature");
    let successor = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("f3d:native:scope#22"))
        .expect("successor feature");
    assert_eq!(successor.dependencies, [predecessor.id.clone()]);
    assert!(predecessor.ordinal < successor.ordinal);
    let width = parameters
        .iter()
        .find(|parameter| parameter.name == "Width")
        .expect("predecessor Width parameter");
    let depth = parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .expect("successor Depth parameter");
    assert_eq!(depth.dependencies, [width.id.clone()]);
}

#[test]
fn variable_width_relation_uses_counted_runs_and_next_record_boundary() {
    // The eleven-byte reference form puts each pair at fifteen bytes: the
    // reference at `marker`, its relation ordinal four bytes later.
    let mut record = vec![0u8; 127];
    record[0..4].copy_from_slice(&3u32.to_le_bytes());
    record[4..7].copy_from_slice(b"286");
    record[7..11].copy_from_slice(&1239u32.to_le_bytes());
    record[19] = 1;
    record[20..24].copy_from_slice(&3u32.to_le_bytes());
    for (marker, reference) in [(24, 1224u32), (39, 1228), (54, 1236)] {
        record[marker] = 1;
        record[marker + 1..marker + 9].copy_from_slice(&u64::from(reference).to_le_bytes());
    }
    record[35..39].copy_from_slice(&3u32.to_le_bytes());
    record[50..54].copy_from_slice(&1u32.to_le_bytes());
    // Offset 69 is the base level's property-block presence byte; the
    // `ParentNode` reference follows it.
    record[70] = 1;
    record[71..79].copy_from_slice(&1041u64.to_le_bytes());
    record[81..89].copy_from_slice(&4u64.to_le_bytes());
    record[89..93].copy_from_slice(&3u32.to_le_bytes());
    for (marker, reference) in [(93, 1224u32), (104, 1228), (115, 1236)] {
        record[marker] = 1;
        record[marker + 1..marker + 9].copy_from_slice(&u64::from(reference).to_le_bytes());
    }
    let mut bytes = record.clone();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"277");
    bytes.extend_from_slice(&1240u32.to_le_bytes());

    assert_eq!(next_indexed_record_offset(&bytes, 11), Some(127));
    let parsed = parse_classed_sketch_relation(&record, SketchRelationClass::Plain).unwrap();
    assert_eq!(parsed.members, [1224, 1228, 1236]);
    assert_eq!(parsed.member_relation_ordinals, [3, 1, 0]);
    assert_eq!(parsed.auxiliary_references, [] as [u32; 0]);
    assert_eq!(parsed.owner_reference, 1041);
    assert_eq!(parsed.state, 4);
    assert_eq!(parsed.state_offset, 81);
    assert_eq!(parsed.entity_genesis, None);
    assert_eq!(parsed.return_members, [1224, 1228, 1236]);
    assert_eq!(parsed.parsed_end, 127);
}

#[test]
fn indexed_record_search_requires_the_expected_identity() {
    let mut bytes = vec![0xaa; 9];
    let decoy = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"278");
    bytes.extend_from_slice(&41u32.to_le_bytes());
    bytes.extend_from_slice(&[0xbb; 7]);
    let expected = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"306");
    bytes.extend_from_slice(&42u32.to_le_bytes());

    assert_eq!(next_indexed_record_offset(&bytes, 0), Some(decoy));
    assert_eq!(
        next_indexed_record_offset_with_index(&bytes, 0, 42),
        Some(expected)
    );
}

fn push_reference(out: &mut Vec<u8>, reference: u32) {
    out.push(1);
    out.extend_from_slice(&reference.to_le_bytes());
}

fn push_genesis_block(out: &mut Vec<u8>, genesis: u64) {
    out.push(1);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&13u32.to_le_bytes());
    out.extend_from_slice(b"EntityGenesis");
    out.extend_from_slice(&23u32.to_le_bytes());
    out.extend_from_slice(b"IntrinsicMetaTypeuint64");
    out.extend_from_slice(&genesis.to_le_bytes());
}

fn genesis_relation_record(
    members: &[(u32, u32)],
    genesis: u64,
    auxiliary: &[u8],
    owner: u32,
    mask: u64,
    returns: &[u32],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"298");
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    out.push(1);
    out.extend_from_slice(&u32::try_from(members.len()).unwrap().to_le_bytes());
    for (reference, role) in members {
        push_reference(&mut out, *reference);
        out.extend_from_slice(&[0u8; 6]);
        out.extend_from_slice(&role.to_le_bytes());
    }
    push_genesis_block(&mut out, genesis);
    out.extend_from_slice(auxiliary);
    push_reference(&mut out, owner);
    out.extend_from_slice(&[0u8; 6]);
    out.extend_from_slice(&mask.to_le_bytes());
    out.extend_from_slice(&u32::try_from(returns.len()).unwrap().to_le_bytes());
    for reference in returns {
        push_reference(&mut out, *reference);
        out.extend_from_slice(&[0u8; 6]);
    }
    out.extend_from_slice(&[0u8; 4]);
    out
}

#[test]
fn genesis_relation_parses_u64_text_frame_mask_and_member_roles() {
    let mut auxiliary = Vec::new();
    push_reference(&mut auxiliary, 2394);
    auxiliary.extend_from_slice(&[0u8; 6]);
    // The second text-frame reference is absent.
    auxiliary.push(0);
    let record = genesis_relation_record(
        &[(2394, 0), (2403, 0), (2404, 0)],
        2,
        &auxiliary,
        1425,
        0x100_0000_0000,
        &[2403, 2404],
    );
    let parsed = parse_classed_sketch_relation(&record, SketchRelationClass::TextFrame).unwrap();
    assert_eq!(parsed.members, [2394, 2403, 2404]);
    assert_eq!(parsed.member_relation_ordinals, [0, 0, 0]);
    assert_eq!(parsed.entity_genesis, Some(2));
    assert_eq!(parsed.auxiliary_references, [2394]);
    assert_eq!(parsed.owner_reference, 1425);
    assert_eq!(parsed.state, 0x100_0000_0000);
    assert_eq!(parsed.return_members, [2403, 2404]);
    assert_eq!(
        decode_constraint_kinds(parsed.state),
        (vec![SketchConstraintKind::TextFrame], 0)
    );
    assert_eq!(
        decode_pattern_definition(&record, &parsed),
        Some(crate::records::SketchPatternDefinition::TextFrame {
            text_reference: 2394
        })
    );
}

#[test]
fn genesis_relation_parses_text_path_glyph_run() {
    let glyphs: [[[f64; 4]; 4]; 2] = [
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, -5.0627],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        [
            [1.0, 0.0, 0.0, 0.6216],
            [0.0, 1.0, 0.0, -5.0627],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    ];
    let mut auxiliary = vec![1u8];
    push_reference(&mut auxiliary, 304);
    auxiliary.extend_from_slice(&[0u8; 6]);
    auxiliary.extend_from_slice(&2u32.to_le_bytes());
    for transform in &glyphs {
        auxiliary.extend_from_slice(&16u32.to_le_bytes());
        for value in transform.iter().flatten() {
            auxiliary.extend_from_slice(&value.to_le_bytes());
        }
    }
    let record = genesis_relation_record(
        &[(237, 1), (304, 0)],
        2,
        &auxiliary,
        201,
        0x200_0000_0000,
        &[237],
    );
    let parsed = parse_classed_sketch_relation(
        &record,
        SketchRelationClass::TextPath { leading_flag: true },
    )
    .unwrap();
    assert_eq!(parsed.members, [237, 304]);
    assert_eq!(parsed.member_relation_ordinals, [1, 0]);
    assert_eq!(parsed.entity_genesis, Some(2));
    assert_eq!(parsed.auxiliary_references, [304]);
    assert_eq!(parsed.owner_reference, 201);
    assert_eq!(parsed.state, 0x200_0000_0000);
    assert_eq!(parsed.return_members, [237]);
    assert_eq!(parsed.text_glyph_transforms.as_deref(), Some(&glyphs[..]));
    assert_eq!(
        decode_constraint_kinds(parsed.state),
        (vec![SketchConstraintKind::TextPath], 0)
    );
    assert_eq!(
        decode_pattern_definition(&record, &parsed),
        Some(crate::records::SketchPatternDefinition::TextPath {
            text_reference: 304,
            glyph_transforms: glyphs.to_vec(),
        })
    );
}

/// Build one sketch-text record: `properties` are the property-block keys in
/// stream order, `slots` says whether each parameter-reference member is
/// written, and `frame` gives the anchor in centimetres and the rotation in
/// radians of a frame text's placement transform, or `None` for path text,
/// which stores no transform.
fn sketch_text_record(
    properties: &[(&str, u64)],
    slots: [Option<u32>; 2],
    frame: Option<(f64, f64, f64)>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    };
    let push_utf16 = |bytes: &mut Vec<u8>, value: &str| {
        let encoded = value.encode_utf16().collect::<Vec<_>>();
        bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
        bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
    };
    push_ascii(&mut bytes, "329");
    bytes.extend_from_slice(&304u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    bytes.push(1);
    bytes.extend_from_slice(&(properties.len() as u32).to_le_bytes());
    for (key, value) in properties {
        push_ascii(&mut bytes, key);
        push_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend_from_slice(&0.8f64.to_le_bytes());
    for component in [0.25f32, 0.5, 0.75, 1.0] {
        bytes.extend_from_slice(&component.to_le_bytes());
    }
    push_utf16(&mut bytes, "Arial");
    bytes.push(0);
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    if let Some(reference) = slots[0] {
        push_reference(&mut bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);
    push_utf16(&mut bytes, "path text");
    if let Some(reference) = slots[1] {
        push_reference(&mut bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&400i32.to_le_bytes());
    bytes.extend_from_slice(&u32::from(frame.is_none()).to_le_bytes());
    bytes.push(u8::from(frame.is_none()));
    if let Some((anchor_u, anchor_v, rotation)) = frame {
        // A planar rigid placement: the 2x2 rotation basis, the anchor in the
        // last column, and the identity's third row and column.
        let (sin, cos) = rotation.sin_cos();
        for element in [
            cos, -sin, 0.0, anchor_u, sin, cos, 0.0, anchor_v, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
            1.0,
        ] {
            bytes.extend_from_slice(&element.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&[0; 30]);
    push_reference(&mut bytes, 201);
    bytes.extend_from_slice(&[0; 6]);
    bytes
}

/// Build the indexed Design form of a `textex_tag` record. Its header carries
/// a u32 record index and a nine-byte zero entity lane, and its class tail ends
/// after the fixed frame suffix and owning-sketch reference.
fn indexed_sketch_text_record(text_type: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    };
    let push_utf16 = |bytes: &mut Vec<u8>, value: &str| {
        let encoded = value.encode_utf16().collect::<Vec<_>>();
        bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
        bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
    };
    let push_padded_reference = |bytes: &mut Vec<u8>, reference: u32| {
        push_reference(bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
    };
    push_ascii(&mut bytes, "287");
    bytes.extend_from_slice(&304u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for (key, value) in [("EntityGenesis", 0u64), ("textex_tag", 117)] {
        push_ascii(&mut bytes, key);
        push_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0);
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    for component in [0.0f32, 0.0, 0.0, 1.0] {
        bytes.extend_from_slice(&component.to_le_bytes());
    }
    push_utf16(&mut bytes, "Arial");
    bytes.push(0);
    bytes.extend_from_slice(&0.6f64.to_le_bytes());
    push_padded_reference(&mut bytes, 319);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);
    push_utf16(&mut bytes, "B6 Probe 47");
    push_padded_reference(&mut bytes, 322);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&400i32.to_le_bytes());
    bytes.extend_from_slice(&text_type.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&256u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1.0f32.to_le_bytes());
    bytes.extend_from_slice(&1.0f32.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    push_padded_reference(&mut bytes, 227);
    bytes
}

#[test]
fn indexed_textex_tag_sketch_text_record_decodes_frame_and_path_types() {
    for text_type in [0, 1] {
        let bytes = indexed_sketch_text_record(text_type);
        let text = decode_sketch_text_at(&bytes, 3).expect("indexed sketch text record");
        assert_eq!(text.record_index, 304);
        assert_eq!(text.owner_reference, 227);
        assert_eq!(text.entity_genesis, Some(0));
        assert_eq!(text.persistent_id, Some(117));
        assert_eq!(text.text, "B6 Probe 47");
        assert_eq!(text.font_family, "Arial");
        assert_eq!(text.font_weight, 400);
        assert_eq!(text.height, 6.0);
        assert_eq!(text.width_factor, Some(1.0));
        assert_eq!(text.horizontal_alignment, Some(3));
        assert_eq!(text.vertical_alignment, Some(3));
        assert_eq!(
            text.color,
            cadmpeg_ir::topology::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }
        );
        assert_eq!(text.anchor, None);
        assert_eq!(text.rotation, None);
        assert_eq!(text.first_reference, Some(319));
        assert_eq!(text.second_reference, Some(322));
        assert_eq!(text.raw_bytes, bytes);
    }
}

/// Decode one sketch-text record at `class_version`, the version its Design
/// `MetaStream` type table gives its class.
fn decode_sketch_text_at(bytes: &[u8], class_version: u32) -> Option<crate::records::SketchText> {
    crate::design::decode::sketch::decode_sketch_text_record(
        bytes,
        "Design/BulkStream.dat",
        "329".into(),
        class_version,
        304,
        7,
    )
}

/// Decode one sketch-text record at the class version that writes an identity
/// key and the wider anchor run.
fn decode_sketch_text(bytes: &[u8]) -> Option<crate::records::SketchText> {
    decode_sketch_text_at(bytes, 4)
}

#[test]
fn sketch_text_record_decodes_typed_content_and_metrics() {
    let bytes = sketch_text_record(
        &[
            ("EntityGenesis", 4),
            ("textex_tag", 109),
            ("txt_tag_base", 305),
        ],
        [Some(307), Some(310)],
        None,
    );
    let text = decode_sketch_text(&bytes).expect("sketch text record");
    assert_eq!(text.record_index, 304);
    assert_eq!(text.owner_reference, 201);
    assert_eq!(text.entity_genesis, Some(4));
    assert_eq!(text.persistent_id, Some(109));
    assert_eq!(text.base_id, Some(305));
    assert_eq!(text.text, "path text");
    assert_eq!(text.font_family, "Arial");
    assert_eq!(text.font_weight, 400);
    // The height is the field after the font family, in centimetres; the width
    // factor is the field before it.
    assert_eq!(text.height, 10.0);
    assert_eq!(text.width_factor, Some(0.8));
    assert_eq!(text.horizontal_alignment, Some(3));
    assert_eq!(text.vertical_alignment, Some(3));
    // The four f32 after the width factor are red, green, blue, and alpha in
    // that order.
    assert_eq!(
        text.color,
        cadmpeg_ir::topology::Color {
            r: 0.25,
            g: 0.5,
            b: 0.75,
            a: 1.0,
        }
    );
    assert_eq!(text.first_reference, Some(307));
    assert_eq!(text.second_reference, Some(310));
}

#[test]
fn sketch_text_record_refuses_a_colour_component_outside_the_unit_range() {
    let bytes = sketch_text_record(&[("textex_tag", 109)], [None, None], None);
    let mut over = bytes.clone();
    let at = bytes
        .windows(4)
        .position(|window| window == 0.5f32.to_le_bytes())
        .expect("green component");
    over[at..at + 4].copy_from_slice(&1.5f32.to_le_bytes());
    assert!(decode_sketch_text(&over).is_none());
    let mut under = bytes;
    under[at..at + 4].copy_from_slice(&(-0.5f32).to_le_bytes());
    assert!(decode_sketch_text(&under).is_none());
}

#[test]
fn sketch_text_record_decodes_without_the_optional_property_keys() {
    let text = decode_sketch_text(&sketch_text_record(
        &[("textex_tag", 109)],
        [None, None],
        None,
    ))
    .expect("sketch text record");
    assert_eq!(text.entity_genesis, None);
    assert_eq!(text.base_id, None);
    assert_eq!(text.persistent_id, Some(109));
    assert_eq!(text.first_reference, None);
    assert_eq!(text.second_reference, None);
    assert_eq!(text.height, 10.0);
    assert_eq!(text.width_factor, Some(0.8));
}

#[test]
fn frame_sketch_text_record_takes_its_anchor_and_rotation_from_the_transform() {
    let rotation = std::f64::consts::FRAC_PI_2;
    let text = decode_sketch_text(&sketch_text_record(
        &[("textex_tag", 109), ("txt_tag_base", 305)],
        [None, None],
        Some((2.175, -0.5, rotation)),
    ))
    .expect("sketch text record");
    assert_eq!(text.base_id, Some(305));
    assert_eq!(text.owner_reference, 201);
    // The anchor is the transform's last column in centimetres and the
    // rotation is the angle of its first basis column.
    assert_eq!(
        text.anchor,
        Some(cadmpeg_ir::math::Point2::new(21.75, -5.0))
    );
    assert!((text.rotation.expect("rotation") - rotation).abs() < 1e-12);
    // Frame text stores 128 more bytes than path text.
    assert_eq!(
        text.raw_bytes.len(),
        sketch_text_record(
            &[("textex_tag", 109), ("txt_tag_base", 305)],
            [None, None],
            None
        )
        .len()
            + 128
    );
}

#[test]
fn path_sketch_text_record_stores_neither_anchor_nor_rotation() {
    let text = decode_sketch_text(&sketch_text_record(
        &[("textex_tag", 109)],
        [None, None],
        None,
    ))
    .expect("sketch text record");
    assert_eq!(text.anchor, None);
    assert_eq!(text.rotation, None);
}

#[test]
fn frame_sketch_text_record_refuses_a_transform_that_is_not_a_planar_rotation() {
    let bytes = sketch_text_record(&[("textex_tag", 109)], [None, None], Some((1.0, 2.0, 0.25)));
    let at = bytes.len() - 128 - 30 - 11;
    // A scaled basis, a third row that is not the identity's, and a bottom row
    // that is not `(0, 0, 0, 1)` each leave the placement.
    for (element, value) in [(0usize, 2.0f64), (10, 0.5), (15, 2.0)] {
        let mut broken = bytes.clone();
        broken[at + element * 8..at + element * 8 + 8].copy_from_slice(&value.to_le_bytes());
        assert!(decode_sketch_text(&broken).is_none());
    }
}

#[test]
fn sketch_text_record_refuses_a_flag_byte_that_does_not_repeat_the_text_type() {
    let bytes = sketch_text_record(&[("textex_tag", 109)], [None, None], None);
    // The flag byte sits between the text-type enum and the transform slot,
    // ahead of the trailing run and the owning-sketch reference.
    let at = bytes.len() - 30 - 11 - 1;
    assert_eq!(bytes[at], 1);
    let mut broken = bytes;
    broken[at] = 0;
    assert!(decode_sketch_text(&broken).is_none());
}

#[test]
fn sketch_text_record_refuses_a_payload_that_does_not_end_on_its_owner() {
    let mut bytes = sketch_text_record(&[("textex_tag", 109)], [None, None], None);
    bytes.push(0);
    assert!(decode_sketch_text(&bytes).is_none());
}

/// Build one sketch-text record in the `txt_tag` identity form: `properties`
/// are the property-block keys in stream order, `frame` is the leading block's
/// reference run, `run` is the counted reference run after the text, `anchor`
/// is the text anchor point in centimetres, and `class_version` selects the
/// width of the run between the anchor and the text string.
fn txt_tag_sketch_text_record_at(
    properties: &[(&str, u64)],
    frame: &[u32],
    run: &[u32],
    anchor: (f64, f64),
    class_version: u32,
) -> Vec<u8> {
    txt_tag_sketch_text_record_at_with_rotation(properties, frame, run, anchor, class_version, 0.0)
}

fn txt_tag_sketch_text_record_at_with_rotation(
    properties: &[(&str, u64)],
    frame: &[u32],
    run: &[u32],
    anchor: (f64, f64),
    class_version: u32,
    rotation: f64,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    };
    let push_utf16 = |bytes: &mut Vec<u8>, value: &str| {
        let encoded = value.encode_utf16().collect::<Vec<_>>();
        bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
        bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
    };
    let push_padded_reference = |bytes: &mut Vec<u8>, reference: u32| {
        push_reference(bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
    };
    push_ascii(&mut bytes, "329");
    bytes.extend_from_slice(&304u64.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    // The leading block: a reference and a u32 per entry.
    bytes.push(1);
    bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
    for reference in frame {
        push_padded_reference(&mut bytes, *reference);
        bytes.extend_from_slice(&[0; 4]);
    }
    bytes.push(1);
    bytes.extend_from_slice(&(properties.len() as u32).to_le_bytes());
    for (key, value) in properties {
        push_ascii(&mut bytes, key);
        push_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&rotation.to_le_bytes());
    // Five bytes separate the rotation from the four f32 RGBA components.
    bytes.extend_from_slice(&[0; 5]);
    for component in [0.0f32, 0.3, 1.0, 1.0] {
        bytes.extend_from_slice(&component.to_le_bytes());
    }
    push_utf16(&mut bytes, "Arial");
    bytes.extend_from_slice(&0.5f64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&anchor.0.to_le_bytes());
    bytes.extend_from_slice(&anchor.1.to_le_bytes());
    bytes.extend_from_slice(&vec![0u8; if class_version < 4 { 10 } else { 11 }]);
    push_utf16(&mut bytes, "sketch text");
    bytes.extend_from_slice(&(run.len() as u32).to_le_bytes());
    for reference in run {
        push_padded_reference(&mut bytes, *reference);
    }
    let mut member_run = [0; 15];
    member_run[3..7].copy_from_slice(&400i32.to_le_bytes());
    bytes.extend_from_slice(&member_run);
    bytes.extend_from_slice(&[0; 30]);
    push_padded_reference(&mut bytes, 201);
    bytes
}

/// Build one `txt_tag` sketch-text record at the class version that writes an
/// identity key and the wider anchor run.
fn txt_tag_sketch_text_record(
    properties: &[(&str, u64)],
    frame: &[u32],
    run: &[u32],
    anchor: (f64, f64),
) -> Vec<u8> {
    txt_tag_sketch_text_record_at(properties, frame, run, anchor, 4)
}

#[test]
fn txt_tag_sketch_text_record_decodes_its_anchor_and_metrics() {
    let text = decode_sketch_text(&txt_tag_sketch_text_record(
        &[("EntityGenesis", 4), ("txt_tag", 115)],
        &[261, 262, 263, 264],
        &[261, 262, 263, 264, 261, 262, 263, 264],
        (0.25, -1.5),
    ))
    .expect("sketch text record");
    assert_eq!(text.record_index, 304);
    assert_eq!(text.owner_reference, 201);
    assert_eq!(text.entity_genesis, Some(4));
    assert_eq!(text.persistent_id, Some(115));
    assert_eq!(text.base_id, None);
    assert_eq!(text.text, "sketch text");
    assert_eq!(text.font_family, "Arial");
    assert_eq!(text.font_weight, 400);
    assert_eq!(text.rotation, Some(0.0));
    assert_eq!(text.height, 5.0);
    // The form stores no width factor, and the anchor is the field pair the
    // other form omits.
    assert_eq!(text.width_factor, None);
    assert_eq!(text.horizontal_alignment, None);
    assert_eq!(text.vertical_alignment, None);
    assert_eq!(text.anchor, Some(cadmpeg_ir::math::Point2::new(2.5, -15.0)));
    // The colour closes the twenty-nine-byte run in the same component order
    // as the other form.
    assert_eq!(
        text.color,
        cadmpeg_ir::topology::Color {
            r: 0.0,
            g: 0.3,
            b: 1.0,
            a: 1.0,
        }
    );
    assert_eq!(text.first_reference, None);
    assert_eq!(text.second_reference, None);
}

#[test]
fn txt_tag_sketch_text_record_decodes_stored_rotation() {
    let stored_rotation = std::f64::consts::TAU - std::f64::consts::FRAC_PI_6;
    let text = decode_sketch_text(&txt_tag_sketch_text_record_at_with_rotation(
        &[("txt_tag", 115)],
        &[261],
        &[261],
        (0.811_473_722_624_350_2, -1.434_008_059_576_836_5),
        4,
        stored_rotation,
    ))
    .expect("rotated txt_tag");
    assert_eq!(text.rotation, Some(stored_rotation));
    assert_eq!(
        text.anchor,
        Some(Point2::new(8.114_737_226_243_502, -14.340_080_595_768_365,))
    );
}

#[test]
fn txt_tag_sketch_text_record_decodes_an_empty_reference_run() {
    let text = decode_sketch_text(&txt_tag_sketch_text_record(
        &[("txt_tag", 115), ("txt_tag_base", 305)],
        &[],
        &[],
        (0.0, 0.0),
    ))
    .expect("sketch text record");
    assert_eq!(text.base_id, Some(305));
    assert_eq!(text.anchor, Some(cadmpeg_ir::math::Point2::new(0.0, 0.0)));
}

#[test]
fn txt_tag_sketch_text_record_refuses_a_payload_that_does_not_end_on_its_owner() {
    let mut bytes = txt_tag_sketch_text_record(&[("txt_tag", 115)], &[261], &[261], (0.0, 0.0));
    bytes.push(0);
    assert!(decode_sketch_text(&bytes).is_none());
}

#[test]
fn sketch_text_record_refuses_a_property_block_without_an_identity_key() {
    assert!(decode_sketch_text(&txt_tag_sketch_text_record(
        &[("txt_tag_base", 305)],
        &[261],
        &[261],
        (0.0, 0.0),
    ))
    .is_none());
    assert!(decode_sketch_text(&sketch_text_record(
        &[("txt_tag_base", 305)],
        [None, None],
        None
    ))
    .is_none());
}

#[test]
fn a_txt_tag_sketch_text_record_below_the_identity_key_version_stores_no_identity() {
    let text = decode_sketch_text_at(
        &txt_tag_sketch_text_record_at(&[("txt_tag_base", 300)], &[261], &[261], (0.25, -1.5), 3),
        3,
    )
    .expect("sketch text record");
    assert_eq!(text.class_version, 3);
    assert_eq!(text.persistent_id, None);
    assert_eq!(text.base_id, Some(300));
    assert_eq!(text.text, "sketch text");
    assert_eq!(text.anchor, Some(cadmpeg_ir::math::Point2::new(2.5, -15.0)));
}

#[test]
fn the_txt_tag_anchor_run_widens_with_the_class_version() {
    // The run between the anchor and the text string is ten bytes below class
    // version 4 and eleven from it, so a record read at the other version's
    // width does not end on its owning-sketch reference.
    for (written, read) in [(3u32, 4u32), (4, 3)] {
        assert!(decode_sketch_text_at(
            &txt_tag_sketch_text_record_at(
                &[("txt_tag", 115)],
                &[261],
                &[261],
                (0.0, 0.0),
                written
            ),
            read,
        )
        .is_none());
    }
}

#[test]
fn genesis_relation_parses_circular_pattern_auxiliary_run() {
    let mut auxiliary = Vec::new();
    push_reference(&mut auxiliary, 336);
    auxiliary.extend_from_slice(&[0u8; 6]);
    push_reference(&mut auxiliary, 333);
    auxiliary.extend_from_slice(&[0u8; 6]);
    auxiliary.extend_from_slice(&std::f64::consts::TAU.to_le_bytes());
    auxiliary.extend_from_slice(&3u32.to_le_bytes());
    auxiliary.extend_from_slice(&[0u8; 9]);
    let record = genesis_relation_record(
        &[(280, 1), (291, 1), (327, 0), (330, 0)],
        2,
        &auxiliary,
        201,
        0x1000_0000,
        &[291, 327, 330, 280],
    );
    let parsed =
        parse_classed_sketch_relation(&record, SketchRelationClass::CircularPattern).unwrap();
    assert_eq!(parsed.member_relation_ordinals, [1, 1, 0, 0]);
    assert_eq!(parsed.auxiliary_references, [336, 333]);
    assert_eq!(parsed.state, 0x1000_0000);
    assert_eq!(
        decode_pattern_definition(&record, &parsed),
        Some(crate::records::SketchPatternDefinition::Circular {
            angle_parameter: 336,
            count_parameter: 333,
            evaluated_angle: std::f64::consts::TAU,
            evaluated_count: 3,
        })
    );
}

#[test]
fn genesis_relation_parses_rectangular_pattern_auxiliary_run() {
    let mut auxiliary = Vec::new();
    push_reference(&mut auxiliary, 0);
    auxiliary.extend_from_slice(&[0u8; 10]);
    auxiliary.extend_from_slice(&3u32.to_le_bytes());
    push_reference(&mut auxiliary, 464);
    auxiliary.extend_from_slice(&[0u8; 6]);
    for value in [1.0f64, 0.0, 0.0, 3.0] {
        auxiliary.extend_from_slice(&value.to_le_bytes());
    }
    push_reference(&mut auxiliary, 470);
    auxiliary.extend_from_slice(&[0u8; 6]);
    auxiliary.extend_from_slice(&1u32.to_le_bytes());
    push_reference(&mut auxiliary, 467);
    auxiliary.extend_from_slice(&[0u8; 6]);
    for value in [0.0f64, 1.0, 0.0, 0.5] {
        auxiliary.extend_from_slice(&value.to_le_bytes());
    }
    push_reference(&mut auxiliary, 473);
    auxiliary.extend_from_slice(&[0u8; 6]);
    let record = genesis_relation_record(
        &[(352, 3), (353, 1), (442, 0), (445, 0)],
        2,
        &auxiliary,
        201,
        0x2000_0000,
        &[353, 352, 442, 445],
    );
    let parsed =
        parse_classed_sketch_relation(&record, SketchRelationClass::RectangularPattern).unwrap();
    assert_eq!(parsed.member_relation_ordinals, [3, 1, 0, 0]);
    assert_eq!(parsed.auxiliary_references, [464, 470, 467, 473]);
    assert_eq!(parsed.state, 0x2000_0000);
    let Some(crate::records::SketchPatternDefinition::Rectangular { directions }) =
        decode_pattern_definition(&record, &parsed)
    else {
        panic!("expected rectangular pattern definition");
    };
    assert_eq!(directions[0].evaluated_count, 3);
    assert_eq!(directions[0].count_parameter, 464);
    assert_eq!(directions[0].direction, [1.0, 0.0, 0.0]);
    assert_eq!(directions[0].evaluated_distance, 3.0);
    assert_eq!(directions[0].distance_parameter, 470);
    assert_eq!(directions[1].evaluated_count, 1);
    assert_eq!(directions[1].count_parameter, 467);
    assert_eq!(directions[1].direction, [0.0, 1.0, 0.0]);
    assert_eq!(directions[1].evaluated_distance, 0.5);
    assert_eq!(directions[1].distance_parameter, 473);
}

#[test]
fn genesis_entity_header_variant_resolves_suffix_and_id() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"281");
    bytes.extend_from_slice(&201u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 10]);
    push_genesis_block(&mut bytes, 4);
    bytes.extend_from_slice(&5u32.to_le_bytes());
    for unit in "0_201".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let (entity_suffix, entity_id, optional_slot_present, end) =
        parse_genesis_entity_header(&bytes, 0).unwrap();
    assert_eq!(entity_suffix, 201);
    assert_eq!(entity_id, "0_201");
    assert!(!optional_slot_present);
    assert_eq!(end, bytes.len());
    assert!(parse_settled_entity_header(&bytes, 0).is_none());
}

#[test]
fn base_feature_scope_decodes_parallel_result_body_runs() {
    let mut bytes = vec![0u8; 375];
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&4u32.to_le_bytes());
    let mut cursor = 24;
    for (value, field) in [
        (101u64, [0, 0, 1, 0, 0, 0]),
        (202, [0; 6]),
        (301, [0; 6]),
        (302, [0, 0, 2, 0, 0, 0]),
    ] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
        bytes[cursor + 9..cursor + 15].copy_from_slice(&field);
        cursor += 15;
    }
    bytes[cursor] = 1;
    cursor += 11;
    bytes[cursor..cursor + 4].copy_from_slice(&2u32.to_le_bytes());
    cursor += 4;
    for reference in [301u32, 302] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5].copy_from_slice(&reference.to_le_bytes());
        cursor += 11;
    }
    cursor += 1;
    bytes[cursor] = 1;
    bytes[cursor + 1..cursor + 9].copy_from_slice(&401u64.to_le_bytes());
    cursor += 15;
    bytes[cursor..cursor + 4].copy_from_slice(&2u32.to_le_bytes());
    cursor += 4;
    for result in [501u32, 502] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5].copy_from_slice(&result.to_le_bytes());
        cursor += 11;
    }
    assert!(cursor <= 171);

    let scope = DesignParameterScope {
        id: "f3d:Design/BulkStream.dat:design-parameter-scope#0".into(),
        byte_offset: 0,
        class_tag: "306".into(),
        record_index: 1,
        frame_length: 375,
        kind: "Base Feature".into(),
        kind_offset: 273,
        extrude_prologue: None,
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: Some(2),
        history_state_id_offset: 0,
        previous_history_state_id: Some(2),
        previous_history_state_id_offset: 0,
        reference_count_offset: 0,
        reference_members: vec![301],
        reference_member_offsets: vec![0],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        unclosed_construction_operand_groups: Vec::new(),
        work_point_reference_type: None,
        work_point_input_record_indices: Vec::new(),
        extrude_profile: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: 375,
    };
    let construction = exact_base_feature_construction(&bytes, &scope)
        .expect("generated Base Feature frame is canonical");
    assert_eq!(construction.body_entity_suffixes, [101, 202]);
    assert_eq!(construction.body_reference_records, [301, 302]);
    assert_eq!(construction.metadata_record, 401);
    assert_eq!(construction.result_records, [501, 502]);
    assert_eq!(construction.body_entity_fields[0], [0, 0, 1, 0, 0, 0]);

    let mut expanded_bytes = Vec::new();
    expanded_bytes.extend_from_slice(&bytes[..84]);
    expanded_bytes.push(1);
    expanded_bytes.extend_from_slice(&[0; 6]);
    expanded_bytes.extend_from_slice(&2u32.to_le_bytes());
    expanded_bytes.extend_from_slice(&bytes[99..131]);
    expanded_bytes.extend_from_slice(&bytes[131..133]);
    expanded_bytes.extend_from_slice(&bytes[137..]);
    expanded_bytes.resize(366, 0);
    let mut expanded_scope = scope.clone();
    expanded_scope.class_tag = "384".into();
    expanded_scope.paired_class_tag = "264".into();
    expanded_scope.frame_length = 366;
    expanded_scope.kind_offset = 265;
    expanded_scope.paired_byte_offset = 366;
    let expanded = exact_base_feature_construction(&expanded_bytes, &expanded_scope)
        .expect("expanded Base Feature frame is canonical");
    assert_eq!(expanded.body_entity_suffixes, [101, 202]);
    assert_eq!(expanded.result_records, [501, 502]);
    assert_eq!(expanded.metadata_field, [0, 0]);

    let mut legacy_compact_bytes = expanded_bytes.clone();
    legacy_compact_bytes[90] = 1;
    legacy_compact_bytes[96..100].copy_from_slice(&101u32.to_le_bytes());
    legacy_compact_bytes[107..111].copy_from_slice(&202u32.to_le_bytes());
    let mut legacy_compact_scope = expanded_scope.clone();
    legacy_compact_scope.class_tag = "420".into();
    legacy_compact_scope.paired_class_tag = "258".into();
    let legacy_compact =
        exact_base_feature_construction(&legacy_compact_bytes, &legacy_compact_scope)
            .expect("legacy compact Base Feature frame is canonical");
    assert_eq!(legacy_compact.body_entity_suffixes, [101, 202]);
    assert_eq!(legacy_compact.body_reference_records, [301, 302]);
    assert_eq!(legacy_compact.result_records, [501, 502]);
    assert_eq!(legacy_compact.metadata_field, [0, 0]);

    legacy_compact_bytes[96..100].copy_from_slice(&301u32.to_le_bytes());
    assert!(
        exact_base_feature_construction(&legacy_compact_bytes, &legacy_compact_scope).is_none()
    );
}

#[test]
fn pattern_constructions_require_exact_scalar_and_operand_frames() {
    fn append_header(bytes: &mut Vec<u8>, record_index: u32) {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(b"999");
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    fn append_transform_record(bytes: &mut Vec<u8>, record_index: u32, translation: [f64; 3]) {
        append_header(bytes, record_index);
        for value in [
            1.0,
            0.0,
            0.0,
            translation[0],
            0.0,
            1.0,
            0.0,
            translation[1],
            0.0,
            0.0,
            1.0,
            translation[2],
            0.0,
            0.0,
            0.0,
            1.0,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }

    let scope_record_index = 10_u32;
    let count_record_index = 20_u32;
    let angle_record_index = 30_u32;
    let axis_record_index = 40_u32;
    let selection_record_index = 43_u32;
    let mut bytes = Vec::new();

    let count_start = bytes.len();
    let mut count = vec![0; 99];
    count[0..4].copy_from_slice(&3_u32.to_le_bytes());
    count[4..7].copy_from_slice(b"357");
    count[7..11].copy_from_slice(&count_record_index.to_le_bytes());
    count[19] = 1;
    count[20..24].copy_from_slice(&1_u32.to_le_bytes());
    count[24] = 1;
    count[25..29].copy_from_slice(&scope_record_index.to_le_bytes());
    count[40..44].copy_from_slice(&25_u32.to_le_bytes());
    count[44] = 1;
    count[45..49].copy_from_slice(&(count_record_index + 2).to_le_bytes());
    count[55..59].copy_from_slice(&99_u32.to_le_bytes());
    count[63] = 1;
    count[64..68].copy_from_slice(&scope_record_index.to_le_bytes());
    count[76] = 1;
    count[77..81].copy_from_slice(&(count_record_index + 1).to_le_bytes());
    count[88] = 1;
    count[89..93].copy_from_slice(&scope_record_index.to_le_bytes());
    count.extend_from_slice(&3_u32.to_le_bytes());
    count.extend_from_slice(b"258");
    count.extend_from_slice(&count_record_index.to_le_bytes());
    bytes.extend_from_slice(&count);

    let angle_start = bytes.len();
    let mut angle = vec![0; 103];
    angle[0..4].copy_from_slice(&3_u32.to_le_bytes());
    angle[4..7].copy_from_slice(b"354");
    angle[7..11].copy_from_slice(&angle_record_index.to_le_bytes());
    angle[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    angle[24] = 1;
    angle[25..29].copy_from_slice(&scope_record_index.to_le_bytes());
    angle[35] = 1;
    angle[40..48].copy_from_slice(&std::f64::consts::TAU.to_le_bytes());
    angle[48] = 1;
    angle[49..53].copy_from_slice(&77_u32.to_le_bytes());
    angle[67] = 1;
    angle[68..72].copy_from_slice(&scope_record_index.to_le_bytes());
    angle[80] = 1;
    angle[81..85].copy_from_slice(&78_u32.to_le_bytes());
    angle[92] = 1;
    angle[93..97].copy_from_slice(&scope_record_index.to_le_bytes());
    angle.extend_from_slice(&3_u32.to_le_bytes());
    angle.extend_from_slice(b"258");
    angle.extend_from_slice(&angle_record_index.to_le_bytes());
    bytes.extend_from_slice(&angle);

    let axis_start = bytes.len();
    let mut axis = vec![0; 195];
    axis[0..4].copy_from_slice(&3_u32.to_le_bytes());
    axis[4..7].copy_from_slice(b"379");
    axis[7..11].copy_from_slice(&axis_record_index.to_le_bytes());
    axis[21..25].copy_from_slice(&8_u32.to_le_bytes());
    for (offset, value) in [1.0_f64, 2.0, 3.0].into_iter().enumerate() {
        axis[25 + offset * 8..33 + offset * 8].copy_from_slice(&value.to_le_bytes());
    }
    axis[49..57].copy_from_slice(&(-1.0_f64).to_le_bytes());
    axis[89..93].copy_from_slice(&9_u32.to_le_bytes());
    axis[93..97].copy_from_slice(&1_u32.to_le_bytes());
    axis[97] = 1;
    axis[98..102].copy_from_slice(&selection_record_index.to_le_bytes());
    axis[110..114].copy_from_slice(&1_u32.to_le_bytes());
    axis[114] = 1;
    axis[115..119].copy_from_slice(&79_u32.to_le_bytes());
    axis[125..133].copy_from_slice(&0x0000_0004_0000_0000_u64.to_le_bytes());
    axis[143..147].copy_from_slice(&99_u32.to_le_bytes());
    axis[147..155].copy_from_slice(&0.5_f64.to_le_bytes());
    axis[155..159].copy_from_slice(&99_u32.to_le_bytes());
    axis[159] = 1;
    axis[160..164].copy_from_slice(&(axis_record_index + 2).to_le_bytes());
    axis[172] = 1;
    axis[173..177].copy_from_slice(&(axis_record_index + 1).to_le_bytes());
    axis[184] = 1;
    axis[185..189].copy_from_slice(&scope_record_index.to_le_bytes());
    axis.extend_from_slice(&3_u32.to_le_bytes());
    axis.extend_from_slice(b"258");
    axis.extend_from_slice(&axis_record_index.to_le_bytes());
    bytes.extend_from_slice(&axis);

    let mut scope = DesignParameterScope {
        id: "f3d:Design/BulkStream.dat:design-parameter-scope#0".into(),
        byte_offset: 0,
        class_tag: "291".into(),
        record_index: scope_record_index,
        frame_length: 329,
        kind: "C-Pattern".into(),
        kind_offset: 0,
        extrude_prologue: None,
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: Some(2),
        history_state_id_offset: 0,
        previous_history_state_id: Some(1),
        previous_history_state_id_offset: 0,
        reference_count_offset: 0,
        reference_members: vec![
            count_record_index,
            angle_record_index,
            axis_record_index,
            selection_record_index,
        ],
        reference_member_offsets: vec![0; 4],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        unclosed_construction_operand_groups: Vec::new(),
        work_point_reference_type: None,
        work_point_input_record_indices: Vec::new(),
        extrude_profile: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "258".into(),
        paired_byte_offset: 329,
    };
    assert_eq!(
        exact_circular_pattern_construction_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &[]
        ),
        Some(DesignCircularPatternConstruction {
            count: 25,
            count_record_index,
            count_offset: (count_start + 40) as u64,
            angle: std::f64::consts::TAU,
            angle_record_index,
            angle_offset: (angle_start + 40) as u64,
            axis: crate::records::DesignCircularPatternAxis::Inline {
                origin: [1.0, 2.0, 3.0],
                origin_offset: (axis_start + 25) as u64,
                direction: [-1.0, 0.0, 0.0],
                direction_offset: (axis_start + 49) as u64,
            },
            axis_record_index,
            selection_record_index,
        })
    );

    bytes[count_start + 4] = b'x';
    bytes[angle_start + 4] = b'x';
    let owner = |record_index, local_ordinal, evaluated_value, evaluated_value_offset| {
        DesignParameterOwner {
            id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{record_index}"),
            byte_offset: 0,
            class_tag: "457".into(),
            record_index,
            scope_record_index,
            local_ordinal,
            evaluated_value,
            evaluated_value_offset,
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: None,
            companion_record_index: record_index + 2,
        }
    };
    let owners = [
        owner(count_record_index, 0, 25.0, 101),
        owner(angle_record_index, 1, std::f64::consts::TAU, 202),
    ];
    let owner_backed = exact_circular_pattern_construction_with_owners(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &owners,
    )
    .unwrap();
    assert_eq!(owner_backed.count, 25);
    assert_eq!(owner_backed.count_offset, 101);
    assert_eq!(owner_backed.angle, std::f64::consts::TAU);
    assert_eq!(owner_backed.angle_offset, 202);
    bytes[count_start + 4] = b'3';
    bytes[angle_start + 4] = b'3';

    bytes[axis_start + 89..axis_start + 93].copy_from_slice(&6_u32.to_le_bytes());
    assert!(exact_circular_pattern_construction_with_owners(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &[]
    )
    .is_some());
    bytes[axis_start + 89..axis_start + 93].fill(0);
    assert_eq!(
        exact_circular_pattern_construction_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &[]
        ),
        None
    );
    bytes[axis_start + 89..axis_start + 93].copy_from_slice(&9_u32.to_le_bytes());

    bytes[axis_start + 57..axis_start + 65].copy_from_slice(&1.0_f64.to_le_bytes());
    assert_eq!(
        exact_circular_pattern_construction_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &[]
        ),
        None
    );
    bytes[axis_start + 57..axis_start + 65].fill(0);
    scope
        .reference_members
        .extend([axis_record_index, selection_record_index]);
    assert_eq!(
        exact_circular_pattern_construction_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &[]
        ),
        None
    );

    scope.kind = "R-Pattern".into();
    let rectangular_owners = [
        owner(50, 0, 3.0, 501),
        owner(51, 1, 1.0, 502),
        owner(52, 2, 10.0, 503),
        owner(53, 3, 0.0, 504),
    ];
    let rectangular = exact_rectangular_pattern_construction(
        &[],
        &IndexedRecordOffsets::build(&[]),
        &scope,
        &rectangular_owners,
    )
    .expect("exact rectangular-pattern scalar lanes");
    assert_eq!(rectangular.u_count, 3);
    assert_eq!(rectangular.v_count, 1);
    assert_eq!(rectangular.u_extent, 10.0);
    assert_eq!(rectangular.v_extent, 0.0);
    assert_eq!(rectangular.owner_record_indices, [50, 51, 52, 53]);
    assert_eq!(rectangular.value_offsets, [501, 502, 503, 504]);
    assert_eq!(rectangular.instances, None);

    append_transform_record(&mut bytes, 100, [2.0, 3.0, 4.0]);
    for record_index in 50..=53 {
        append_header(&mut bytes, record_index);
    }
    append_header(&mut bytes, 110);
    append_transform_record(&mut bytes, 120, [2.0, 3.0, 9.0]);
    append_transform_record(&mut bytes, 130, [2.0, 3.0, 14.0]);
    append_header(&mut bytes, 140);
    scope.reference_members = vec![100, 50, 51, 52, 53, 110, 120, 130, 140];
    let rectangular = exact_rectangular_pattern_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &rectangular_owners,
    )
    .expect("rectangular-pattern placement run");
    let instances = rectangular.instances.expect("exact placement run");
    assert_eq!(instances.record_indices, [100, 120, 130]);
    assert_eq!(
        instances
            .transforms
            .iter()
            .map(|transform| transform[2][3])
            .collect::<Vec<_>>(),
        [4.0, 9.0, 14.0]
    );

    let mut invalid_inactive_spacing = rectangular_owners.clone();
    invalid_inactive_spacing[3].evaluated_value = 1.0;
    assert_eq!(
        exact_rectangular_pattern_construction(
            &[],
            &IndexedRecordOffsets::build(&[]),
            &scope,
            &invalid_inactive_spacing
        ),
        None
    );
    let mut duplicate_lane = rectangular_owners.clone();
    duplicate_lane[3].local_ordinal = 2;
    assert_eq!(
        exact_rectangular_pattern_construction(
            &[],
            &IndexedRecordOffsets::build(&[]),
            &scope,
            &duplicate_lane
        ),
        None
    );
    let mut excess_lane = rectangular_owners.to_vec();
    excess_lane.push(owner(54, 4, 1.0, 505));
    assert_eq!(
        exact_rectangular_pattern_construction(
            &[],
            &IndexedRecordOffsets::build(&[]),
            &scope,
            &excess_lane
        ),
        None
    );

    scope.kind = "Assemble".into();
    scope.reference_members = vec![50, 51, 52, 53];
    let alignment = exact_assembly_alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &rectangular_owners,
    )
    .expect("exact assembly scalar lanes");
    assert_eq!(alignment.angle, 3.0);
    assert_eq!(alignment.offset, [1.0, 10.0, 0.0]);
    assert_eq!(alignment.owner_record_indices, [50, 51, 52, 53]);
    assert_eq!(alignment.value_offsets, [501, 502, 503, 504]);
    assert_eq!(alignment.operand_frames, None);

    let mut placement_and_alignment_owners = rectangular_owners.to_vec();
    placement_and_alignment_owners.extend([
        owner(60, 4, 0.25, 601),
        owner(61, 5, 4.0, 602),
        owner(62, 6, 5.0, 603),
        owner(63, 7, 6.0, 604),
    ]);
    scope.reference_members = vec![50, 51, 52, 53, 60, 61, 62, 63];
    let alignment = exact_assembly_alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &placement_and_alignment_owners,
    )
    .expect("assembly alignment after four placement lanes");
    assert_eq!(alignment.angle, 0.25);
    assert_eq!(alignment.offset, [4.0, 5.0, 6.0]);
    assert_eq!(alignment.owner_record_indices, [60, 61, 62, 63]);

    let mut legacy_alignment_owners = placement_and_alignment_owners.clone();
    legacy_alignment_owners.extend([owner(64, 8, 0.5, 605), owner(65, 9, 2.0, 606)]);
    scope.reference_members = vec![50, 51, 52, 53, 60, 61, 62, 63, 64, 65];
    let alignment = exact_assembly_alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &legacy_alignment_owners,
    )
    .expect("legacy assembly axial alignment lanes");
    assert_eq!(alignment.angle, 0.5);
    assert_eq!(alignment.offset, [0.0, 0.0, 2.0]);
    assert_eq!(alignment.owner_record_indices, [64, 65]);
    assert_eq!(alignment.value_offsets, [605, 606]);
    scope.reference_members = vec![50, 51, 52, 53];

    let mut assembly_bytes = vec![0_u8; 648];
    assembly_bytes[0..4].copy_from_slice(&3_u32.to_le_bytes());
    assembly_bytes[4..7].copy_from_slice(b"273");
    assembly_bytes[7..11].copy_from_slice(&scope_record_index.to_le_bytes());
    assembly_bytes[20] = 1;
    assembly_bytes[25] = 1;
    for (reference_at, transform_at, reference, translation) in [
        (28, 40, 70_u32, [1.0_f64, 2.0, 3.0]),
        (168, 180, 80_u32, [4.0, 5.0, 6.0]),
    ] {
        assembly_bytes[reference_at] = 1;
        assembly_bytes[reference_at + 1..reference_at + 5]
            .copy_from_slice(&reference.to_le_bytes());
        for (ordinal, value) in [
            1.0,
            0.0,
            0.0,
            translation[0],
            0.0,
            1.0,
            0.0,
            translation[1],
            0.0,
            0.0,
            1.0,
            translation[2],
            0.0,
            0.0,
            0.0,
            1.0,
        ]
        .into_iter()
        .enumerate()
        {
            assembly_bytes[transform_at + ordinal * 8..transform_at + ordinal * 8 + 8]
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    assembly_bytes[637..641].copy_from_slice(&3_u32.to_le_bytes());
    assembly_bytes[641..644].copy_from_slice(b"259");
    assembly_bytes[644..648].copy_from_slice(&scope_record_index.to_le_bytes());
    scope.frame_length = 637;
    scope.paired_byte_offset = 637;
    scope.paired_class_tag = "259".into();
    let frames = exact_assembly_alignment(
        &assembly_bytes,
        &IndexedRecordOffsets::build(&assembly_bytes),
        &scope,
        &rectangular_owners,
    )
    .and_then(|alignment| alignment.operand_frames)
    .expect("exact assembly operand frames");
    assert_eq!(
        frames.map(|frame| (
            frame.reference_record_index,
            frame.reference_offset,
            frame.transform_offset,
            [
                frame.transform[0][3],
                frame.transform[1][3],
                frame.transform[2][3]
            ]
        )),
        [
            (70, 29, 40, [1.0, 2.0, 3.0]),
            (80, 169, 180, [4.0, 5.0, 6.0]),
        ]
    );
    let mut legacy_assembly_bytes = vec![0_u8; 633];
    legacy_assembly_bytes[..11].copy_from_slice(&assembly_bytes[..11]);
    for (legacy_reference, legacy_transform, modern_reference, modern_transform) in
        [(24, 36, 28, 40), (164, 176, 168, 180)]
    {
        legacy_assembly_bytes[legacy_reference..legacy_reference + 5]
            .copy_from_slice(&assembly_bytes[modern_reference..modern_reference + 5]);
        legacy_assembly_bytes[legacy_transform..legacy_transform + 128]
            .copy_from_slice(&assembly_bytes[modern_transform..modern_transform + 128]);
    }
    legacy_assembly_bytes.extend_from_slice(&3_u32.to_le_bytes());
    legacy_assembly_bytes.extend_from_slice(b"258");
    legacy_assembly_bytes.extend_from_slice(&scope_record_index.to_le_bytes());
    let legacy_assembly_scope = DesignParameterScope {
        frame_length: 633,
        paired_byte_offset: 633,
        paired_class_tag: "258".into(),
        ..scope.clone()
    };
    let legacy_frames = exact_assembly_alignment(
        &legacy_assembly_bytes,
        &IndexedRecordOffsets::build(&legacy_assembly_bytes),
        &legacy_assembly_scope,
        &rectangular_owners,
    )
    .and_then(|alignment| alignment.operand_frames)
    .expect("compact assembly operand frames");
    assert_eq!(legacy_frames[0].reference_offset, 25);
    assert_eq!(legacy_frames[0].transform_offset, 36);

    let mut axial_assembly_bytes = vec![0_u8; 772];
    axial_assembly_bytes[..11].copy_from_slice(&assembly_bytes[..11]);
    axial_assembly_bytes[20..25].copy_from_slice(&[1, 0, 0, 0, 0]);
    for (axial_reference, axial_transform, modern_reference, modern_transform) in
        [(28, 39, 28, 40), (167, 178, 168, 180)]
    {
        axial_assembly_bytes[axial_reference..axial_reference + 5]
            .copy_from_slice(&assembly_bytes[modern_reference..modern_reference + 5]);
        axial_assembly_bytes[axial_transform..axial_transform + 128]
            .copy_from_slice(&assembly_bytes[modern_transform..modern_transform + 128]);
    }
    axial_assembly_bytes.extend_from_slice(&3_u32.to_le_bytes());
    axial_assembly_bytes.extend_from_slice(b"261");
    axial_assembly_bytes.extend_from_slice(&scope_record_index.to_le_bytes());
    let axial_assembly_scope = DesignParameterScope {
        frame_length: 772,
        paired_byte_offset: 772,
        paired_class_tag: "261".into(),
        reference_members: vec![50, 51, 52, 53, 60, 61, 62, 63, 64, 65],
        ..scope.clone()
    };
    let axial_alignment = exact_assembly_alignment(
        &axial_assembly_bytes,
        &IndexedRecordOffsets::build(&axial_assembly_bytes),
        &axial_assembly_scope,
        &legacy_alignment_owners,
    )
    .expect("legacy assembly alignment and operand frames");
    assert_eq!(axial_alignment.angle, 0.5);
    assert_eq!(axial_alignment.offset, [0.0, 0.0, 2.0]);
    let axial_frames = axial_alignment.operand_frames.as_ref().unwrap();
    assert_eq!(axial_frames[0].reference_offset, 29);
    assert_eq!(axial_frames[0].transform_offset, 39);
    assert_eq!(axial_frames[1].reference_offset, 168);
    assert_eq!(axial_frames[1].transform_offset, 178);

    let mut first_joint_origin = scope.clone();
    first_joint_origin.kind = "JointOrigin".into();
    first_joint_origin.record_index = 70;
    first_joint_origin.reference_members.clear();
    let mut second_joint_origin = first_joint_origin.clone();
    second_joint_origin.record_index = 80;
    let mut linked_assembly = axial_assembly_scope.clone();
    linked_assembly.assembly_alignment = Some(axial_alignment.clone());
    let mut linked_scopes = [linked_assembly, first_joint_origin, second_joint_origin];
    bind_joint_origin_frames_from_assemblies(&axial_assembly_bytes, &mut linked_scopes);
    assert_eq!(linked_scopes[1].joint_origin_transform_offset, Some(39));
    assert_eq!(
        linked_scopes[1].joint_origin_transform,
        Some(axial_frames[0].transform)
    );
    assert_eq!(linked_scopes[2].joint_origin_transform_offset, Some(178));
    assert_eq!(
        linked_scopes[2].joint_origin_transform,
        Some(axial_frames[1].transform)
    );
    assert_eq!(
        linked_scopes[0]
            .assembly_alignment
            .as_ref()
            .and_then(|alignment| alignment.joint_origin_scope_record_index),
        None
    );

    let mut single_frame_bytes = vec![0_u8; 604];
    single_frame_bytes[..11].copy_from_slice(&assembly_bytes[..11]);
    single_frame_bytes[24] = 1;
    single_frame_bytes[25..29].copy_from_slice(&90_u32.to_le_bytes());
    single_frame_bytes[36..164].copy_from_slice(&assembly_bytes[40..168]);
    single_frame_bytes[164] = 1;
    single_frame_bytes[165..169].copy_from_slice(&91_u32.to_le_bytes());
    single_frame_bytes[175..179].copy_from_slice(&1_u32.to_le_bytes());
    let mut single_frame_assembly = scope.clone();
    single_frame_assembly.class_tag = "276".into();
    single_frame_assembly.paired_class_tag = "258".into();
    single_frame_assembly.frame_length = 604;
    single_frame_assembly.paired_byte_offset = 604;
    single_frame_assembly.assembly_alignment = Some(alignment.clone());
    let mut single_frame_joint_origin = scope.clone();
    single_frame_joint_origin.kind = "JointOrigin".into();
    single_frame_joint_origin.record_index = 91;
    single_frame_joint_origin.reference_members.clear();
    let mut single_frame_scopes = [single_frame_assembly, single_frame_joint_origin];
    bind_joint_origin_frames_from_assemblies(&single_frame_bytes, &mut single_frame_scopes);
    assert_eq!(
        single_frame_scopes[1].joint_origin_transform_offset,
        Some(36)
    );
    assert_eq!(
        single_frame_scopes[1].joint_origin_transform,
        Some(axial_frames[0].transform)
    );
    assert_eq!(single_frame_scopes[1].joint_origin_reference, Some(90));
    assert_eq!(
        single_frame_scopes[1].joint_origin_reference_offset,
        Some(25)
    );
    assert_eq!(
        single_frame_scopes[0]
            .assembly_alignment
            .as_ref()
            .and_then(|alignment| alignment.joint_origin_scope_record_index),
        Some(91)
    );

    single_frame_bytes[175..179].copy_from_slice(&2_u32.to_le_bytes());
    let mut invalid_joint_origin = single_frame_scopes[1].clone();
    invalid_joint_origin.joint_origin_transform = None;
    invalid_joint_origin.joint_origin_transform_offset = None;
    invalid_joint_origin.joint_origin_reference = None;
    invalid_joint_origin.joint_origin_reference_offset = None;
    let mut invalid_single_frame_scopes = [single_frame_scopes[0].clone(), invalid_joint_origin];
    bind_joint_origin_frames_from_assemblies(&single_frame_bytes, &mut invalid_single_frame_scopes);
    assert_eq!(invalid_single_frame_scopes[1].joint_origin_transform, None);

    let mut compact_bytes = assembly_bytes[..627].to_vec();
    compact_bytes.extend_from_slice(&3_u32.to_le_bytes());
    compact_bytes.extend_from_slice(b"264");
    compact_bytes.extend_from_slice(&scope_record_index.to_le_bytes());
    let mut compact_scope = scope.clone();
    compact_scope.class_tag = "459".into();
    compact_scope.frame_length = 627;
    compact_scope.paired_byte_offset = 627;
    compact_scope.paired_class_tag = "264".into();
    assert!(exact_assembly_alignment(
        &compact_bytes,
        &IndexedRecordOffsets::build(&compact_bytes),
        &compact_scope,
        &rectangular_owners,
    )
    .is_some_and(|alignment| alignment.operand_frames.is_some()));

    let push_path = |bytes: &mut Vec<u8>, record_index: u32, guids: &[&str]| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(b"329");
        bytes.extend_from_slice(&u64::from(record_index).to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&(guids.len() as u32).to_le_bytes());
        for guid in guids {
            let encoded = guid.encode_utf16().collect::<Vec<_>>();
            bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
            bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
        }
    };
    let push_identity_path =
        |bytes: &mut Vec<u8>, record_index: u32, path: &[&str], identities: &[&str; 4]| {
            bytes.extend_from_slice(&3_u32.to_le_bytes());
            bytes.extend_from_slice(b"390");
            bytes.extend_from_slice(&u64::from(record_index).to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
            bytes.extend_from_slice(&(path.len() as u32).to_le_bytes());
            for guid in path.iter().chain(&identities[..2]) {
                let encoded = guid.encode_utf16().collect::<Vec<_>>();
                bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
                bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
            }
            bytes.extend_from_slice(&2_u64.to_le_bytes());
            for guid in &identities[2..] {
                let encoded = guid.encode_utf16().collect::<Vec<_>>();
                bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
                bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
            }
            bytes.extend_from_slice(&2_u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 8]);
        };
    let identities = [
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
        "cccccccc-cccc-cccc-cccc-cccccccccccc",
        "dddddddd-dddd-dddd-dddd-dddddddddddd",
    ];
    let mut identity_path_bytes = assembly_bytes.clone();
    let first_identity_path_at = identity_path_bytes.len();
    push_identity_path(
        &mut identity_path_bytes,
        65,
        &[
            "11111111-1111-1111-1111-111111111111",
            "22222222-2222-2222-2222-222222222222",
        ],
        &identities,
    );
    let second_identity_path_at = identity_path_bytes.len();
    push_identity_path(
        &mut identity_path_bytes,
        68,
        &["33333333-3333-3333-3333-333333333333"],
        &identities,
    );
    identity_path_bytes.extend_from_slice(&3_u32.to_le_bytes());
    identity_path_bytes.extend_from_slice(b"396");
    identity_path_bytes.extend_from_slice(&70_u32.to_le_bytes());
    let identity_paths = exact_assembly_alignment(
        &identity_path_bytes,
        &IndexedRecordOffsets::build(&identity_path_bytes),
        &scope,
        &rectangular_owners,
    )
    .and_then(|alignment| alignment.operand_paths)
    .expect("identity-qualified assembly occurrence paths");
    assert_eq!(identity_paths[0].class_tag, "390");
    assert_eq!(identity_paths[0].occurrence_guids.len(), 2);
    assert_eq!(identity_paths[0].identity_guids, identities);
    for path_at in [first_identity_path_at, second_identity_path_at] {
        identity_path_bytes[path_at + 4..path_at + 7].copy_from_slice(b"386");
    }
    let compact_identity_paths = exact_assembly_alignment(
        &identity_path_bytes,
        &IndexedRecordOffsets::build(&identity_path_bytes),
        &scope,
        &rectangular_owners,
    )
    .and_then(|alignment| alignment.operand_paths)
    .expect("compact identity-qualified assembly occurrence paths");
    assert!(compact_identity_paths
        .iter()
        .all(|path| path.class_tag == "386"));

    push_path(
        &mut assembly_bytes,
        65,
        &[
            "11111111-1111-1111-1111-111111111111",
            "22222222-2222-2222-2222-222222222222",
        ],
    );
    push_path(
        &mut assembly_bytes,
        68,
        &["33333333-3333-3333-3333-333333333333"],
    );
    assembly_bytes.extend_from_slice(&3_u32.to_le_bytes());
    assembly_bytes.extend_from_slice(b"396");
    assembly_bytes.extend_from_slice(&70_u32.to_le_bytes());
    let paths = exact_assembly_alignment(
        &assembly_bytes,
        &IndexedRecordOffsets::build(&assembly_bytes),
        &scope,
        &rectangular_owners,
    )
    .and_then(|alignment| alignment.operand_paths)
    .expect("exact assembly occurrence paths");
    assert_eq!(
        paths.map(|path| (path.record_index, path.occurrence_guids)),
        [
            (
                65,
                vec![
                    "11111111-1111-1111-1111-111111111111".into(),
                    "22222222-2222-2222-2222-222222222222".into(),
                ],
            ),
            (68, vec!["33333333-3333-3333-3333-333333333333".into()],),
        ]
    );
    assembly_bytes[25] = 0;
    assert!(exact_assembly_alignment(
        &assembly_bytes,
        &IndexedRecordOffsets::build(&assembly_bytes),
        &scope,
        &rectangular_owners
    )
    .and_then(|alignment| alignment.operand_frames)
    .is_some());
    assembly_bytes[25] = 2;
    assert!(exact_assembly_alignment(
        &assembly_bytes,
        &IndexedRecordOffsets::build(&assembly_bytes),
        &scope,
        &rectangular_owners
    )
    .is_some_and(|alignment| alignment.operand_frames.is_none()));

    scope.reference_members.push(99);
    assert_eq!(
        exact_assembly_alignment(
            &assembly_bytes,
            &IndexedRecordOffsets::build(&assembly_bytes),
            &scope,
            &rectangular_owners
        ),
        None
    );
}

#[test]
fn component_insert_scope_joins_its_relation_carrier_role_and_transform() {
    let header = |bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    };
    let transform: [[f64; 4]; 4] = [
        [1.0, 0.0, 0.0, -2.1],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let role = "b2231f72-46dc-40fa-b8e8-10cd208d7df8";
    let mut bytes = Vec::new();
    header(&mut bytes, b"256", 10);
    let role_at = bytes.len();
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend(role.encode_utf16().flat_map(u16::to_le_bytes));
    bytes.extend_from_slice(&[0, 0]);
    let carrier_transform_at = bytes.len();
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let relation_at = bytes.len();
    header(&mut bytes, b"325", 20);
    bytes.extend_from_slice(&[0; 10]);
    for (ordinal, reference) in [10_u32, 11, 30].into_iter().enumerate() {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0, [8, 7, 6][ordinal]));
    }
    assert_eq!(bytes.len(), relation_at + 57);
    header(&mut bytes, b"259", 20);
    let scope_at = bytes.len();
    bytes.resize(scope_at + 399, 0);
    bytes[scope_at..scope_at + 4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[scope_at + 4..scope_at + 7].copy_from_slice(b"451");
    bytes[scope_at + 7..scope_at + 11].copy_from_slice(&30_u32.to_le_bytes());
    bytes[scope_at + 20] = 1;
    bytes[scope_at + 37] = 1;
    bytes[scope_at + 38..scope_at + 42].copy_from_slice(&20_u32.to_le_bytes());
    bytes[scope_at + 48] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = scope_at + 50 + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    header(&mut bytes, b"259", 30);
    let scope = DesignParameterScope {
        id: "f3d:Design/BulkStream.dat:design-parameter-scope#30".into(),
        byte_offset: scope_at as u64,
        class_tag: "451".into(),
        record_index: 30,
        frame_length: 399,
        kind: "Component Insert".into(),
        kind_offset: 0,
        extrude_prologue: None,
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: None,
        history_state_id_offset: 0,
        previous_history_state_id: None,
        previous_history_state_id_offset: 0,
        reference_count_offset: 0,
        reference_members: vec![20],
        reference_member_offsets: vec![scope_at as u64 + 38],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        unclosed_construction_operand_groups: Vec::new(),
        work_point_reference_type: None,
        work_point_input_record_indices: Vec::new(),
        extrude_profile: None,
        sweep_profile: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "259".into(),
        paired_byte_offset: (scope_at + 399) as u64,
    };

    let construction =
        exact_component_insert_construction(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
            .expect("component insert construction");

    assert_eq!(construction.relation_record_index, 20);
    assert_eq!(construction.carrier_record_index, 10);
    assert_eq!(construction.neutron_role, role);
    assert_eq!(construction.neutron_role_offset, (role_at + 4) as u64);
    assert_eq!(construction.transform, transform);
    assert_eq!(construction.transform_offset, (scope_at + 50) as u64);
    assert_eq!(
        construction.carrier_transform_offset,
        carrier_transform_at as u64
    );

    for (frame_length, paired_class_tag, transform_at, relation_at, expanded_prologue) in [
        (381_usize, "261", 49_usize, 38_usize, true),
        (395, "258", 46, 34, false),
    ] {
        let mut legacy = bytes[..scope_at].to_vec();
        legacy.resize(scope_at + frame_length, 0);
        legacy[scope_at..scope_at + 4].copy_from_slice(&3_u32.to_le_bytes());
        legacy[scope_at + 4..scope_at + 7].copy_from_slice(b"451");
        legacy[scope_at + 7..scope_at + 11].copy_from_slice(&30_u32.to_le_bytes());
        if expanded_prologue {
            legacy[scope_at + 20] = 1;
            legacy[scope_at + 37] = 1;
            legacy[scope_at + 48] = 1;
        } else {
            legacy[scope_at + 33] = 1;
        }
        legacy[scope_at + relation_at..scope_at + relation_at + 4]
            .copy_from_slice(&20_u32.to_le_bytes());
        if !expanded_prologue {
            legacy[scope_at + transform_at - 2] = 1;
        }
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            let at = scope_at + transform_at + ordinal * 8;
            legacy[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        header(
            &mut legacy,
            paired_class_tag
                .as_bytes()
                .try_into()
                .expect("three-byte tag"),
            30,
        );
        let legacy_scope = DesignParameterScope {
            frame_length: frame_length as u64,
            paired_class_tag: paired_class_tag.into(),
            paired_byte_offset: (scope_at + frame_length) as u64,
            ..scope.clone()
        };
        let construction = exact_component_insert_construction(
            &legacy,
            &IndexedRecordOffsets::build(&legacy),
            &legacy_scope,
        )
        .unwrap_or_else(|| panic!("{frame_length}-byte component insert construction"));
        assert_eq!(
            construction.transform_offset,
            (scope_at + transform_at) as u64
        );
        assert_eq!(construction.transform, transform);
    }

    let mut expanded = Vec::new();
    header(&mut expanded, b"312", 10);
    let expanded_carrier_transform_at = expanded.len();
    for value in transform.into_iter().flatten() {
        expanded.extend_from_slice(&value.to_le_bytes());
    }
    let expanded_role_at = expanded.len();
    expanded.extend_from_slice(&36_u32.to_le_bytes());
    expanded.extend(role.encode_utf16().flat_map(u16::to_le_bytes));
    expanded.extend_from_slice(&[0, 1, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let expanded_relation_at = expanded.len();
    header(&mut expanded, b"338", 20);
    expanded.resize(expanded_relation_at + 58, 0);
    expanded[expanded_relation_at + 21] = 1;
    expanded[expanded_relation_at + 22..expanded_relation_at + 26]
        .copy_from_slice(&10_u32.to_le_bytes());
    expanded[expanded_relation_at + 32..expanded_relation_at + 35].copy_from_slice(&[1, 0, 0]);
    expanded[expanded_relation_at + 35] = 1;
    expanded[expanded_relation_at + 36..expanded_relation_at + 40]
        .copy_from_slice(&99_u32.to_le_bytes());
    expanded[expanded_relation_at + 47] = 1;
    expanded[expanded_relation_at + 48..expanded_relation_at + 52]
        .copy_from_slice(&30_u32.to_le_bytes());
    let expanded_scope_at = expanded.len();
    header(&mut expanded, b"335", 30);
    expanded.resize(expanded_scope_at + 404, 0);
    expanded[expanded_scope_at + 20] = 1;
    let occurrence_identity = 0x0102_0304_0506_0708_u64;
    expanded[expanded_scope_at + 29..expanded_scope_at + 37]
        .copy_from_slice(&occurrence_identity.to_le_bytes());
    expanded[expanded_scope_at + 41] = 1;
    expanded[expanded_scope_at + 42..expanded_scope_at + 46].copy_from_slice(&20_u32.to_le_bytes());
    expanded[expanded_scope_at + 52..expanded_scope_at + 54].copy_from_slice(&[1, 0]);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = expanded_scope_at + 54 + ordinal * 8;
        expanded[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    header(&mut expanded, b"260", 30);
    let expanded_scope = DesignParameterScope {
        byte_offset: expanded_scope_at as u64,
        class_tag: "335".into(),
        frame_length: 404,
        reference_member_offsets: vec![(expanded_scope_at + 42) as u64],
        paired_class_tag: "260".into(),
        paired_byte_offset: (expanded_scope_at + 404) as u64,
        ..scope.clone()
    };
    let construction = exact_component_insert_construction(
        &expanded,
        &IndexedRecordOffsets::build(&expanded),
        &expanded_scope,
    )
    .expect("404-byte component insert construction");
    assert_eq!(construction.relation_record_index, 20);
    assert_eq!(construction.carrier_record_index, 10);
    assert_eq!(construction.occurrence_identity, Some(occurrence_identity));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(
        construction.neutron_role_offset,
        (expanded_role_at + 4) as u64
    );
    assert_eq!(construction.transform, transform);
    assert_eq!(
        construction.transform_offset,
        (expanded_scope_at + 54) as u64
    );
    assert_eq!(
        construction.carrier_transform_offset,
        expanded_carrier_transform_at as u64
    );

    let push_utf16 = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.encode_utf16().count() as u32).to_le_bytes());
        bytes.extend(value.encode_utf16().flat_map(u16::to_le_bytes));
    };
    let mut legacy = Vec::new();
    header(&mut legacy, b"288", 10);
    legacy.resize(30, 0);
    push_utf16(&mut legacy, "95cc7c78-04aa-4ffc-a36d-a512f02e0dda");
    let legacy_role_at = legacy.len();
    push_utf16(&mut legacy, role);
    legacy.extend_from_slice(&[1, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    push_utf16(&mut legacy, "96e2c767-721c-4c81-bbbc-8cc143d323fb");
    legacy.push(0);
    let asset_identity = "864a8a41-7ed8-4c94-8871-ee9e87ab7648_urn:asset";
    push_utf16(&mut legacy, asset_identity);
    legacy.push(0);
    let legacy_carrier_transform_at = legacy.len();
    for value in transform.into_iter().flatten() {
        legacy.extend_from_slice(&value.to_le_bytes());
    }
    legacy.extend_from_slice(&[0; 4]);
    push_utf16(&mut legacy, asset_identity);
    legacy.extend_from_slice(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let legacy_relation_at = legacy.len();
    header(&mut legacy, b"325", 20);
    legacy.extend_from_slice(&[0; 10]);
    for (ordinal, reference) in [10_u32, 11, 30].into_iter().enumerate() {
        legacy.push(1);
        legacy.extend_from_slice(&reference.to_le_bytes());
        legacy.extend(std::iter::repeat_n(0, [8, 7, 6][ordinal]));
    }
    let legacy_scope_at = legacy.len();
    header(&mut legacy, b"346", 30);
    legacy.resize(legacy_scope_at + 381, 0);
    legacy[legacy_scope_at + 20] = 1;
    legacy[legacy_scope_at + 37] = 1;
    legacy[legacy_scope_at + 38..legacy_scope_at + 42].copy_from_slice(&20_u32.to_le_bytes());
    legacy[legacy_scope_at + 48] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = legacy_scope_at + 49 + ordinal * 8;
        legacy[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    header(&mut legacy, b"261", 30);
    let legacy_scope = DesignParameterScope {
        byte_offset: legacy_scope_at as u64,
        frame_length: 381,
        paired_class_tag: "261".into(),
        paired_byte_offset: (legacy_scope_at + 381) as u64,
        ..scope
    };
    let construction = exact_component_insert_construction(
        &legacy,
        &IndexedRecordOffsets::build(&legacy),
        &legacy_scope,
    )
    .expect("class-288 legacy component insert construction");
    assert_eq!(construction.neutron_role, role);
    assert_eq!(
        construction.neutron_role_offset,
        (legacy_role_at + 4) as u64
    );
    assert_eq!(
        construction.carrier_transform_offset,
        legacy_carrier_transform_at as u64
    );
    assert_eq!(construction.relation_record_index, 20);
    assert_eq!(legacy_relation_at + 57, legacy_scope_at);
}

#[test]
fn repeated_linear_dimension_requires_disjoint_measurement_pairs() {
    use cadmpeg_ir::features::ParameterId;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchDistanceMeasurement as Measurement,
        SketchEntityId, SketchLocus,
    };

    let entity = |name: &str| SketchEntityId(format!("generated:{name}"));
    let parameter = ParameterId("generated:distance".into());
    let horizontal = |first: &str, second: &str| Definition::HorizontalDistance {
        first: SketchLocus::Entity(entity(first)),
        second: SketchLocus::Entity(entity(second)),
        parameter: parameter.clone(),
    };
    let candidates = vec![horizontal("a", "b"), horizontal("c", "d")];
    let Definition::RepeatedDistance {
        measurements,
        parameter: actual,
    } = repeated_linear_dimension(&candidates, parameter.clone()).unwrap()
    else {
        panic!("expected repeated distance")
    };
    assert_eq!(actual, parameter);
    assert!(matches!(
        measurements.as_slice(),
        [
            Measurement::Horizontal { .. },
            Measurement::Horizontal { .. }
        ]
    ));

    let ambiguous = vec![horizontal("a", "b"), horizontal("a", "c")];
    assert!(repeated_linear_dimension(&ambiguous, parameter).is_none());
}

#[test]
fn sweep_recipe_edge_requires_incidence_and_two_reference_faces() {
    use crate::design::edge_resolve::unique_incidence_edge_shared_by_reference_faces;

    let selector = |edges| crate::records::DesignEdgeRecipeSelectorContext {
        selector: 0,
        clause_entries: Vec::new(),
        clause_triplet_edge_slots: Vec::new(),
        incidence_matching_edge_slots: edges,
        unique_incidence_edge_slot: None,
        boundary_count_matching_edge_slots: Vec::new(),
    };
    let selectors = [selector(vec![11, 12]), selector(vec![13])];
    assert_eq!(
        unique_incidence_edge_shared_by_reference_faces(
            &selectors,
            [&[10, 11][..], &[11, 12][..], &[13, 14][..]],
        ),
        Some(11)
    );
    assert_eq!(
        unique_incidence_edge_shared_by_reference_faces(&selectors, [&[11, 13][..], &[11, 13][..]],),
        None
    );
    assert_eq!(
        unique_incidence_edge_shared_by_reference_faces(&[selector(vec![11])], [&[10, 11][..]],),
        None
    );
    assert_eq!(
        unique_incidence_edge_shared_by_reference_faces(
            &[selector(vec![11])],
            [&[10, 11][..], &[10, 11][..]],
        ),
        None
    );
}
