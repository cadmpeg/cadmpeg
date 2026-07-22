// SPDX-License-Identifier: Apache-2.0
//! Relation, decode, and projection unit tests for the design modules.

use crate::design::configurations::{
    bind_configuration_parameter_overrides, bind_configuration_suppressed_features,
    project_configurations, unresolved_configuration_member_count,
    unresolved_configuration_parameter_override_count, unresolved_configuration_rule_count,
    unresolved_configuration_suppressed_feature_count, validate_configuration_payload,
};
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
    parse_body_recipe_operand, parse_construction_operand_group,
    parse_construction_operand_identity, parse_edge_operand, parse_entity_selection_operand,
    parse_extrude_selection_group, parse_extrude_selection_member, parse_face_operand,
    parse_sketch_profile, FaceRecipeProgramKind,
};
use crate::design::decode::parameters::{
    bind_parameter_companion_payloads, design_parameter_prefix, parse_design_parameter,
    parse_parameter_companion, parse_parameter_owner,
};
use crate::design::decode::scopes::{
    exact_base_feature_construction, exact_direct_face_operation, exact_fixed_chamfer_parameters,
    exact_fixed_extrude_parameters, exact_fixed_fillet_parameters, exact_path_feature_construction,
    exact_scale_operation, exact_solid_primitive, exact_surface_stitch_operation,
    exact_work_plane_frame, exact_work_point_position, parse_parameter_scope,
};
use crate::design::decode::sketch::{
    bind_sketch_graph, decode_constraint_kinds, decode_pattern_definition, identity_matrix,
    next_indexed_record_offset, next_indexed_record_offset_with_index, parse_genesis_entity_header,
    parse_settled_entity_header, parse_sketch_placement_candidates, parse_sketch_relation,
    parse_sketch_surface,
};
use crate::design::dimensions::{
    bind_dimension_loci, directional_point_dimension, exact_atomic_constraint,
    exact_counted_dimension_relation, exact_counted_offset, exact_offset_constraint,
    expression_identifiers, indirect_angular_lines, null_locus_dimension_definition,
    offset_parameter_factor, point_lies_on_sketch_geometry, project_dimension_constraints,
    project_spatial_dimension_constraints, radial_dimension_definition,
    remove_dimension_frame_relations, repeated_linear_dimension,
    spatial_parallel_line_distance_matches, two_locus_distance_dimension,
    unresolved_parameter_expression_dependency_count,
};
use crate::design::edge_resolve::{
    feature_input_topology_id, partial_historical_edge_selection,
    resolved_edge_candidate_intersection,
};
use crate::design::face_resolve::resolved_face_group;
use crate::design::feature_project::{
    project_extrude, project_parameter_design, untyped_parameter_unit_count,
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
    ConstructionRecipe, ConstructionRecipeKind, DesignCoilExtent, DesignCoilSection,
    DesignCoilSectionPlacement, DesignConfiguration, DesignConfigurationKind,
    DesignConstructionOperandGroup, DesignConstructionOperandIdentity,
    DesignConstructionPersistentIdentity, DesignDimensionLocus, DesignDimensionLocusGroup,
    DesignDimensionLocusPair, DesignDimensionRecipeRecord, DesignDirectFaceOperation,
    DesignEdgeIdentityOperand, DesignEntityHeader, DesignExtrudeExtent, DesignExtrudeFaceRole,
    DesignExtrudeOperandRole, DesignExtrudeOperation, DesignExtrudeSelectionGroup,
    DesignExtrudeStart, DesignFixedChamferParameters, DesignFixedExtrudeParameters,
    DesignFixedFilletParameters, DesignObjectKind, DesignParameter, DesignParameterCompanion,
    DesignParameterKind, DesignParameterOwner, DesignParameterScope, DesignPathFeatureConstruction,
    DesignRecipeReference, DesignRecordHeader, DesignScaleOperation, DesignSketchPlacement,
    DesignSketchProfileOperand, DesignSolidPrimitive, DesignSurfaceStitchOperation,
    LostEdgeReference, PersistentSubentityTag, SketchConstraintKind, SketchCurveGeometry,
    SketchCurveIdentity, SketchPoint, SketchRelation, SketchRelationOperand, SketchSurface,
};
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::features::{
    Angle, DesignParameter as NeutralParameter, FaceSelection, Feature, FeatureDefinition,
    FeatureId, Length, ParameterId, ParameterValue, ProfileRef, SketchProfileRegion,
};
use cadmpeg_ir::ids::{EdgeId, FaceId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchAxis, SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchEntityUse,
    SketchGeometry, SketchId, SpatialSketch, SpatialSketchConstraintDefinition,
};
use std::collections::{BTreeMap, HashMap, HashSet};

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
    assert_eq!(
        design_feature_family("Réseau C"),
        Some(DesignFeatureFamily::CircularPattern)
    );
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
        design_feature_family("SurfacePatch"),
        Some(DesignFeatureFamily::SurfacePatch)
    );
    assert_eq!(
        design_feature_family("BoundaryFill"),
        Some(DesignFeatureFamily::BoundaryFill)
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
}

#[test]
fn configuration_identity_is_stable_across_table_order_and_delimiter_names() {
    let table = |entry_name: &str, variant_name: &str| DesignConfiguration {
        id: format!("f3d:configuration:entry#{entry_name}"),
        entry_name: entry_name.into(),
        kind: DesignConfigurationKind::Table,
        payload: serde_json::json!({"configurations": {variant_name: {}}}),
    };
    let first = table("asset/a#b.dsgcfg", "c");
    let second = table("asset/a.dsgcfg", "b#c");
    let first_id = first.id.clone();

    let forward = project_configurations(&[first.clone(), second.clone()]);
    let reversed = project_configurations(&[second, first]);
    let forward_ids = forward
        .iter()
        .map(|configuration| configuration.id.clone())
        .collect::<HashSet<_>>();
    let reversed_ids = reversed
        .iter()
        .map(|configuration| configuration.id.clone())
        .collect::<HashSet<_>>();

    assert_eq!(forward_ids, reversed_ids);
    assert_eq!(forward_ids.len(), 2);
    assert_ne!(forward[0].id, forward[1].id);
    assert_eq!(forward[0].native_ref.as_deref(), Some(first_id.as_str()));
}

#[test]
fn configuration_parameter_overrides_require_scalar_values() {
    let scalar_parameters = serde_json::json!({
        "configurations": {
            "variant": {
                "parameters": {
                    "string": "25 mm",
                    "number": 2.5,
                    "boolean": true,
                    "null": null
                }
            }
        }
    });
    assert!(validate_configuration_payload(
        "table.dsgcfg",
        DesignConfigurationKind::Table,
        &scalar_parameters,
    )
    .is_ok());

    for value in [
        serde_json::json!(["25 mm"]),
        serde_json::json!({"value": "25 mm"}),
    ] {
        let payload = serde_json::json!({
            "configurations": {"variant": {"parameters": {"width": value}}}
        });
        assert!(validate_configuration_payload(
            "table.dsgcfg",
            DesignConfigurationKind::Table,
            &payload,
        )
        .is_err());
    }
}

#[test]
fn configuration_unknown_members_are_counted_at_each_semantic_level() {
    let native = [
        DesignConfiguration {
            id: "f3d:configuration:entry#table.dsgcfg".into(),
            entry_name: "table.dsgcfg".into(),
            kind: DesignConfigurationKind::Table,
            payload: serde_json::json!({
                "active": "variant",
                "table_unknown": 1,
                "configurations": {
                    "variant": {
                        "parameters": {},
                        "suppressed": [],
                        "material": "steel",
                        "variant_unknown": true
                    }
                }
            }),
        },
        DesignConfiguration {
            id: "f3d:configuration:entry#rule.dsgcfgrule".into(),
            entry_name: "rule.dsgcfgrule".into(),
            kind: DesignConfigurationKind::Rule,
            payload: serde_json::json!({
                "when": "width > 20 mm",
                "activate": "variant",
                "rule_unknown": null
            }),
        },
    ];
    assert_eq!(unresolved_configuration_member_count(&native), 3);
}

#[test]
fn configuration_rules_bind_only_one_named_variant() {
    let table = |entry_name: &str, variant_name: &str| DesignConfiguration {
        id: format!("f3d:configuration:entry#{entry_name}"),
        entry_name: entry_name.into(),
        kind: DesignConfigurationKind::Table,
        payload: serde_json::json!({"configurations": {variant_name: {}}}),
    };
    let rule = DesignConfiguration {
        id: "f3d:configuration:entry#rule.dsgcfgrule".into(),
        entry_name: "rule.dsgcfgrule".into(),
        kind: DesignConfigurationKind::Rule,
        payload: serde_json::json!({"when": "width > 20 mm", "activate": "wide"}),
    };
    let native = [table("table.dsgcfg", "wide"), rule.clone()];
    let projected = project_configurations(&native);
    assert_eq!(
        projected[0].properties["activation_rule:rule.dsgcfgrule"],
        "width > 20 mm"
    );
    assert_eq!(unresolved_configuration_rule_count(&native, &projected), 0);

    let ambiguous = [
        table("first.dsgcfg", "wide"),
        table("second.dsgcfg", "wide"),
        rule,
    ];
    let projected = project_configurations(&ambiguous);
    assert!(projected
        .iter()
        .all(|configuration| configuration.properties.is_empty()));
    assert_eq!(
        unresolved_configuration_rule_count(&ambiguous, &projected),
        1
    );
}

#[test]
fn configuration_parameter_overrides_bind_only_unique_parameter_names() {
    let table = DesignConfiguration {
        id: "f3d:configuration:entry#table.dsgcfg".into(),
        entry_name: "table.dsgcfg".into(),
        kind: DesignConfigurationKind::Table,
        payload: serde_json::json!({
            "configurations": {"wide": {"parameters": {"width": "25 mm"}}}
        }),
    };
    let parameter = NeutralParameter {
        id: ParameterId("f3d:model:parameter#width".into()),
        owner: None,
        ordinal: 0,
        name: "width".into(),
        expression: "10 mm".into(),
        display: None,
        value: None,
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let mut projected = project_configurations(&[table]);
    bind_configuration_parameter_overrides(&mut projected, std::slice::from_ref(&parameter));
    assert_eq!(projected[0].parameter_overrides[&parameter.id], "25 mm");
    assert!(projected[0].properties.is_empty());
    assert_eq!(
        unresolved_configuration_parameter_override_count(&projected),
        0
    );

    let duplicate = NeutralParameter {
        id: ParameterId("f3d:model:parameter#other-width".into()),
        ..parameter.clone()
    };
    let mut ambiguous = project_configurations(&[DesignConfiguration {
        id: "f3d:configuration:entry#other.dsgcfg".into(),
        entry_name: "other.dsgcfg".into(),
        kind: DesignConfigurationKind::Table,
        payload: serde_json::json!({
            "configurations": {"wide": {"parameters": {"width": "25 mm"}}}
        }),
    }]);
    bind_configuration_parameter_overrides(&mut ambiguous, &[parameter, duplicate]);
    assert!(ambiguous[0].parameter_overrides.is_empty());
    assert_eq!(
        unresolved_configuration_parameter_override_count(&ambiguous),
        1
    );
}

#[test]
fn configuration_suppression_binds_only_unique_feature_names() {
    let table = DesignConfiguration {
        id: "f3d:configuration:entry#table.dsgcfg".into(),
        entry_name: "table.dsgcfg".into(),
        kind: DesignConfigurationKind::Table,
        payload: serde_json::json!({
            "configurations": {"alternate": {"suppressed": ["Fillet 1"]}}
        }),
    };
    let feature = Feature {
        id: FeatureId("f3d:model:feature#fillet-1".into()),
        ordinal: 0,
        name: Some("Fillet 1".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "Fillet".into(),
            parameters: BTreeMap::new(),
            properties: BTreeMap::new(),
        },
        native_ref: None,
    };
    let mut projected = project_configurations(&[table]);
    bind_configuration_suppressed_features(&mut projected, std::slice::from_ref(&feature));
    assert_eq!(projected[0].suppressed_features, [feature.id.clone()]);
    assert!(projected[0].properties.is_empty());
    assert_eq!(
        unresolved_configuration_suppressed_feature_count(&projected),
        0
    );

    let duplicate = Feature {
        id: FeatureId("f3d:model:feature#other-fillet-1".into()),
        ..feature.clone()
    };
    let mut ambiguous = project_configurations(&[DesignConfiguration {
        id: "f3d:configuration:entry#other.dsgcfg".into(),
        entry_name: "other.dsgcfg".into(),
        kind: DesignConfigurationKind::Table,
        payload: serde_json::json!({
            "configurations": {"alternate": {"suppressed": ["Fillet 1"]}}
        }),
    }]);
    bind_configuration_suppressed_features(&mut ambiguous, &[feature, duplicate]);
    assert!(ambiguous[0].suppressed_features.is_empty());
    assert_eq!(
        unresolved_configuration_suppressed_feature_count(&ambiguous),
        1
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
fn parameter_identity_uses_stream_and_native_source_ordinal() {
    let first = neutral_parameter_id_parts("Design/A:12", 3);
    let same = neutral_parameter_id_parts("Design/A:12", 3);
    let different_stream = neutral_parameter_id_parts("Design/A", 123);
    let different_ordinal = neutral_parameter_id_parts("Design/A:12", 4);

    assert_eq!(first, same);
    assert_ne!(first, different_stream);
    assert_ne!(first, different_ordinal);
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
fn historical_point_inside_unique_closed_line_profile_selects_region() {
    let sketch_id = SketchId("sketch".into());
    let mut entities = Vec::new();
    let mut profile = Vec::new();
    for (ordinal, (start, end)) in [
        (Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)),
        (Point2::new(4.0, 0.0), Point2::new(4.0, 3.0)),
        (Point2::new(4.0, 3.0), Point2::new(0.0, 3.0)),
        (Point2::new(0.0, 3.0), Point2::new(0.0, 0.0)),
    ]
    .into_iter()
    .enumerate()
    {
        let id = SketchEntityId(format!("line-{ordinal}"));
        profile.push(SketchEntityUse {
            entity: id.clone(),
            reversed: false,
        });
        entities.push(SketchEntity {
            id,
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line { start, end },
        });
    }
    let circle_id = SketchEntityId("unrelated-circle".into());
    let profiles = vec![
        profile,
        vec![SketchEntityUse {
            entity: circle_id.clone(),
            reversed: false,
        }],
    ];
    entities.push(SketchEntity {
        id: circle_id,
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(20.0, 20.0),
            radius: Length(1.0),
        },
    });
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(10.0, 20.0, 5.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles,
        native_ref: None,
    };

    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(12.0, 21.0, 12.0)], 1.0e-6,),
        Some(SketchProfileRegion::Loops {
            outer: 0,
            holes: Vec::new(),
        })
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(15.0, 21.0, 12.0)], 1.0e-6,),
        None
    );

    let mut incomplete = sketch.clone();
    let ellipse = SketchEntityId("unsupported-ellipse".into());
    incomplete.profiles.push(vec![SketchEntityUse {
        entity: ellipse.clone(),
        reversed: false,
    }]);
    entities.push(SketchEntity {
        id: ellipse,
        sketch: incomplete.id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Ellipse {
            center: Point2::new(30.0, 30.0),
            major_angle: Angle(0.0),
            major_radius: Length(2.0),
            minor_radius: Length(1.0),
            start_angle: None,
            end_angle: None,
        },
    });
    assert_eq!(
        region_containing_points(
            &incomplete,
            &entities,
            &[Point3::new(12.0, 21.0, 12.0)],
            1.0e-6,
        ),
        None
    );
}

#[test]
fn nested_line_profiles_resolve_atomic_regions_and_immediate_holes() {
    let sketch_id = SketchId("sketch".into());
    let mut entities = Vec::new();
    let mut profiles = Vec::new();
    for (profile_index, (minimum, maximum)) in [
        (Point2::new(0.0, 0.0), Point2::new(10.0, 10.0)),
        (Point2::new(2.0, 2.0), Point2::new(8.0, 8.0)),
        (Point2::new(4.0, 4.0), Point2::new(6.0, 6.0)),
    ]
    .into_iter()
    .enumerate()
    {
        let corners = [
            minimum,
            Point2::new(maximum.u, minimum.v),
            maximum,
            Point2::new(minimum.u, maximum.v),
        ];
        let mut profile = Vec::new();
        for edge_index in 0..corners.len() {
            let id = SketchEntityId(format!("line-{profile_index}-{edge_index}"));
            profile.push(SketchEntityUse {
                entity: id.clone(),
                reversed: false,
            });
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: corners[edge_index],
                    end: corners[(edge_index + 1) % corners.len()],
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

    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(1.0, 1.0, 0.0)], 1.0e-6,),
        Some(SketchProfileRegion::Loops {
            outer: 0,
            holes: vec![1],
        })
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(3.0, 3.0, 0.0)], 1.0e-6,),
        Some(SketchProfileRegion::Loops {
            outer: 1,
            holes: vec![2],
        })
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(5.0, 5.0, 0.0)], 1.0e-6,),
        Some(SketchProfileRegion::Loops {
            outer: 2,
            holes: Vec::new(),
        })
    );
    assert_eq!(
        region_containing_points(
            &sketch,
            &entities,
            &[Point3::new(0.0, 5.0, 0.0), Point3::new(2.0, 5.0, 0.0)],
            1.0e-6,
        ),
        Some(SketchProfileRegion::Loops {
            outer: 0,
            holes: vec![1],
        })
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(2.0, 5.0, 0.0)], 1.0e-6),
        None
    );
}

#[test]
fn nonperiodic_nurbs_boundary_resolves_atomic_region() {
    let sketch_id = SketchId("sketch".into());
    let definitions = [
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
        SketchGeometry::Line {
            start: Point2::new(10.0, 0.0),
            end: Point2::new(10.0, 10.0),
        },
        SketchGeometry::Nurbs {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point2::new(10.0, 10.0),
                Point2::new(5.0, 12.0),
                Point2::new(0.0, 10.0),
            ],
            weights: Some(vec![1.0, 0.75, 1.0]),
            periodic: false,
        },
        SketchGeometry::Line {
            start: Point2::new(0.0, 10.0),
            end: Point2::new(0.0, 0.0),
        },
    ];
    let mut entities = Vec::new();
    let outer = definitions
        .into_iter()
        .enumerate()
        .map(|(index, geometry)| {
            let id = SketchEntityId(format!("outer-{index}"));
            entities.push(SketchEntity {
                id: id.clone(),
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry,
            });
            SketchEntityUse {
                entity: id,
                reversed: false,
            }
        })
        .collect::<Vec<_>>();
    let corners = [
        Point2::new(3.0, 3.0),
        Point2::new(7.0, 3.0),
        Point2::new(7.0, 7.0),
        Point2::new(3.0, 7.0),
    ];
    let inner = (0..corners.len())
        .map(|index| {
            let id = SketchEntityId(format!("inner-{index}"));
            entities.push(SketchEntity {
                id: id.clone(),
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: corners[index],
                    end: corners[(index + 1) % corners.len()],
                },
            });
            SketchEntityUse {
                entity: id,
                reversed: false,
            }
        })
        .collect::<Vec<_>>();
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![outer, inner],
        native_ref: None,
    };

    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(1.0, 1.0, 0.0)], 1.0e-6),
        Some(SketchProfileRegion::Loops {
            outer: 0,
            holes: vec![1],
        })
    );
}

#[test]
fn coincident_circle_arc_arrangement_resolves_trimmed_faces() {
    use cadmpeg_ir::features::{SketchProfileBoundaryUse, SketchProfileRegion};

    let sketch_id = SketchId("sketch".into());
    let line_id = SketchEntityId("diameter".into());
    let arc_id = SketchEntityId("left-arc".into());
    let circle_id = SketchEntityId("circle".into());
    let entity = |id, geometry| SketchEntity {
        id,
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let entities = vec![
        entity(
            line_id.clone(),
            SketchGeometry::Line {
                start: Point2::new(0.0, -1.0),
                end: Point2::new(0.0, 1.0),
            },
        ),
        entity(
            arc_id.clone(),
            SketchGeometry::Arc {
                center: Point2::new(0.0, 0.0),
                radius: Length(1.0),
                start_angle: Angle(std::f64::consts::FRAC_PI_2),
                end_angle: Angle(3.0 * std::f64::consts::FRAC_PI_2),
            },
        ),
        entity(
            circle_id.clone(),
            SketchGeometry::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length(1.0),
            },
        ),
    ];
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![
            vec![
                SketchEntityUse {
                    entity: line_id.clone(),
                    reversed: false,
                },
                SketchEntityUse {
                    entity: arc_id.clone(),
                    reversed: false,
                },
            ],
            vec![SketchEntityUse {
                entity: circle_id,
                reversed: false,
            }],
        ],
        native_ref: None,
    };

    let faces = crate::design::geometry::sketch_arrangement_faces(&sketch, &entities, 1.0e-7)
        .expect("endpoint arrangement faces");
    assert_eq!(faces.len(), 2);
    let selected = crate::design::geometry::arrangement_region_containing_points(
        &sketch,
        &entities,
        &[
            Point2::new(0.0, -1.0),
            Point2::new(0.0, 1.0),
            Point2::new(-1.0, 0.0),
        ],
        1.0e-7,
    )
    .expect("left half-disk arrangement face");
    let SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    } = selected
    else {
        panic!("arrangement selection must emit a trimmed boundary")
    };
    assert!(hole_boundaries.is_empty());
    assert_eq!(outer_boundary.len(), 2);
    assert!(outer_boundary.iter().any(|use_| use_.entity == line_id));
    assert!(outer_boundary.iter().any(|use_| use_.entity == arc_id));
    assert!(outer_boundary.iter().all(|use_| matches!(
        use_,
        SketchProfileBoundaryUse {
            parameter_range: [start, end],
            ..
        } if start != end
    )));
}

#[test]
fn analytic_arrangement_intersections_include_hidden_second_crossing() {
    let line = crate::design::geometry::ProfileBoundarySegment::Line {
        start: Point2::new(-2.0, 0.0),
        end: Point2::new(2.0, 0.0),
    };
    let circle = crate::design::geometry::ProfileBoundarySegment::Arc {
        center: Point2::new(0.0, 0.0),
        radius: 1.0,
        start_angle: 0.0,
        end_angle: std::f64::consts::TAU,
    };

    assert_eq!(
        crate::design::geometry::analytic_segment_intersections(&line, &circle)
            .expect("analytic intersection family")
            .len(),
        2
    );
}

#[test]
fn polygon_and_circle_boundaries_resolve_one_atomic_region() {
    let sketch_id = SketchId("sketch".into());
    let corners = [
        Point2::new(-5.0, -5.0),
        Point2::new(5.0, -5.0),
        Point2::new(5.0, 5.0),
        Point2::new(-5.0, 5.0),
    ];
    let mut entities = Vec::new();
    let mut outer = Vec::new();
    for index in 0..corners.len() {
        let id = SketchEntityId(format!("line-{index}"));
        outer.push(SketchEntityUse {
            entity: id.clone(),
            reversed: false,
        });
        entities.push(SketchEntity {
            id,
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: corners[index],
                end: corners[(index + 1) % corners.len()],
            },
        });
    }
    let circle = SketchEntityId("circle".into());
    entities.push(SketchEntity {
        id: circle.clone(),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
        },
    });
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![
            outer,
            vec![SketchEntityUse {
                entity: circle,
                reversed: false,
            }],
        ],
        native_ref: None,
    };
    let expected = SketchProfileRegion::Loops {
        outer: 0,
        holes: vec![1],
    };

    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(4.0, 0.0, 0.0)], 1.0e-6,),
        Some(expected.clone())
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(0.0, 0.0, 0.0)], 1.0e-6,),
        Some(SketchProfileRegion::Loops {
            outer: 1,
            holes: Vec::new(),
        })
    );
}

#[test]
fn circular_arc_loop_uses_analytic_containment_and_distance() {
    let segments = vec![
        crate::design::geometry::ProfileBoundarySegment::Line {
            start: Point2::new(0.0, -2.0),
            end: Point2::new(0.0, 2.0),
        },
        crate::design::geometry::ProfileBoundarySegment::Arc {
            center: Point2::new(0.0, 0.0),
            radius: 2.0,
            start_angle: std::f64::consts::FRAC_PI_2,
            end_angle: 3.0 * std::f64::consts::FRAC_PI_2,
        },
    ];
    let boundary = crate::design::geometry::ProfileBoundary::CircularArcLoop(segments);
    let hole = crate::design::geometry::ProfileBoundary::Circle {
        center: Point2::new(-1.0, 0.0),
        radius: 0.5,
    };

    assert!(boundary.contains_point(Point2::new(-1.0, 0.0)));
    assert!(!boundary.contains_point(Point2::new(1.0, 0.0)));
    assert!(!boundary.contains_point(Point2::new(-1.0, -2.0)));
    assert!(!boundary.contains_point(Point2::new(-1.0, 2.0)));
    assert!(boundary.strictly_contains(&hole));
}

#[test]
fn polygon_and_arc_loop_containment_requires_disjoint_boundaries() {
    let polygon = crate::design::geometry::ProfileBoundary::Polygon(vec![
        Point2::new(-2.0, -2.0),
        Point2::new(2.0, -2.0),
        Point2::new(2.0, 2.0),
        Point2::new(-2.0, 2.0),
    ]);
    let arc_loop = crate::design::geometry::ProfileBoundary::CircularArcLoop(vec![
        crate::design::geometry::ProfileBoundarySegment::Line {
            start: Point2::new(-1.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
        crate::design::geometry::ProfileBoundarySegment::Arc {
            center: Point2::new(0.0, 0.0),
            radius: 1.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::PI,
        },
    ]);

    assert!(polygon.strictly_contains(&arc_loop));
    assert!(!arc_loop.strictly_contains(&polygon));

    let crossing = crate::design::geometry::ProfileBoundary::CircularArcLoop(vec![
        crate::design::geometry::ProfileBoundarySegment::Line {
            start: Point2::new(-3.0, 0.0),
            end: Point2::new(3.0, 0.0),
        },
        crate::design::geometry::ProfileBoundarySegment::Arc {
            center: Point2::new(0.0, 0.0),
            radius: 3.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::PI,
        },
    ]);
    assert!(!polygon.strictly_contains(&crossing));
    assert!(!crossing.strictly_contains(&polygon));
}

#[test]
fn arc_loop_containment_rejects_crossing_and_touching_segments() {
    let d_loop = |center_u: f64, radius: f64| {
        crate::design::geometry::ProfileBoundary::CircularArcLoop(vec![
            crate::design::geometry::ProfileBoundarySegment::Line {
                start: Point2::new(center_u, -radius),
                end: Point2::new(center_u, radius),
            },
            crate::design::geometry::ProfileBoundarySegment::Arc {
                center: Point2::new(center_u, 0.0),
                radius,
                start_angle: std::f64::consts::FRAC_PI_2,
                end_angle: 3.0 * std::f64::consts::FRAC_PI_2,
            },
        ])
    };
    let outer = d_loop(0.0, 2.0);
    let inner = d_loop(-0.5, 0.5);
    let crossing = d_loop(-1.5, 1.0);
    let touching = d_loop(-1.0, 1.0);

    assert!(outer.strictly_contains(&inner));
    assert!(!inner.strictly_contains(&outer));
    assert!(!outer.strictly_contains(&crossing));
    assert!(!outer.strictly_contains(&touching));
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
        crate::design::profile_select::transition_inserted_profile_selection([
            Some(region.clone()),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0, 1])),
        ]),
        Some(region.clone())
    );
    assert_eq!(
        crate::design::profile_select::transition_inserted_profile_selection([
            Some(region.clone()),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![2])),
        ]),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![2]))
    );
    assert_eq!(
        crate::design::profile_select::transition_inserted_profile_selection([
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Regions(Vec::new())),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
        ]),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1]))
    );
    assert_eq!(
        crate::design::profile_select::transition_inserted_profile_selection([Some(region)]),
        Some(
            crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
                SketchProfileRegion::Loops {
                    outer: 0,
                    holes: vec![1],
                },
            ])
        )
    );
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
    out.extend_from_slice(&design_parameter_prefix(source_kind).to_le_bytes());
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
    assert_eq!(tangency.prefix_value, 6);
    assert_eq!(tangency.unit, None);
    assert_eq!(tangency.name, "d81");
    assert_eq!(tangency.evaluated_value, 1.0);

    let mut invalid_tangency =
        parameter_record(Some(24409), "1", "TangencyWeight", Some(""), "d81", 1.0);
    invalid_tangency[22..30].copy_from_slice(&0u64.to_le_bytes());
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
            .prefix_value,
        6
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

#[test]
fn parameter_owner_frame_has_repeated_scope_and_both_record_orders() {
    let parsed = parse_parameter_owner(&parameter_owner_frame()).unwrap();
    assert_eq!(parsed.record_index, 44);
    assert_eq!(parsed.scope_record_index, 12);
    assert_eq!(parsed.local_ordinal, 2);
    assert_eq!(parsed.evaluated_value, 6.0);
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.owned_ordinal, 9);
    assert_eq!(parsed.variant, 1);
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
    assert_eq!(
        crate::design::decode::dimension_frames::recipe_reference_candidate_faces(
            &references[0],
            &[
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
                    id: "wrong-selector".into(),
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
            ],
        ),
        [FaceId("face-b".into())]
    );
    assert_eq!(
        crate::design::decode::dimension_frames::recipe_reference_candidate_edges(
            &references[0],
            &[PersistentSubentityTag {
                id: "matching-edge".into(),
                target: AttributeTarget::Edge(EdgeId("edge-b".into())),
                selector: 1,
                token: "13".into(),
                design_references: vec![331],
                ordinal: 0,
            }],
        ),
        [EdgeId("edge-b".into())]
    );
    assert_eq!(
        crate::design::decode::dimension_frames::recipe_reference_alternate_selector_faces(
            &references[0],
            &[PersistentSubentityTag {
                id: "alternate-face".into(),
                target: AttributeTarget::Face(FaceId("face-c".into())),
                selector: 2,
                token: "13".into(),
                design_references: vec![331],
                ordinal: 0,
            }],
        ),
        [FaceId("face-c".into())]
    );
    assert_eq!(
        crate::design::decode::dimension_frames::recipe_reference_alternate_selector_edges(
            &references[0],
            &[PersistentSubentityTag {
                id: "alternate-edge".into(),
                target: AttributeTarget::Edge(EdgeId("edge-c".into())),
                selector: 2,
                token: "13".into(),
                design_references: vec![331],
                ordinal: 0,
            }],
        ),
        [EdgeId("edge-c".into())]
    );
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
        variant: 0,
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
fn radial_dimensions_require_one_exact_circular_measurement() {
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
        member_roles: Vec::new(),
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
    let scope = parse_parameter_scope(&bytes, &header).expect("WorkPoint scope");
    assert_eq!(
        exact_work_point_position(&bytes, &scope),
        Some(([1.25, -2.5, 3.75], position_at as u64))
    );
    bytes[point_at + 66..point_at + 70].copy_from_slice(&1u32.to_le_bytes());
    bytes[point_at + 94..point_at + 98].copy_from_slice(&1u32.to_le_bytes());
    bytes.drain(point_at + 197..point_at + 208);
    assert_eq!(
        exact_work_point_position(&bytes, &scope),
        Some(([1.25, -2.5, 3.75], position_at as u64))
    );
}

#[test]
fn move_matrix_decomposes_to_translation_and_axis_angle() {
    let angle = std::f64::consts::PI / 3.0;
    let transform = [
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

    let mut scope = parse_parameter_scope(&bytes, &header).unwrap();
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
    let discovered = crate::design::decode::scopes::parameter_scope_candidate_headers(&bytes)
        .into_iter()
        .filter_map(|header| parse_parameter_scope(&bytes, &header))
        .collect::<Vec<_>>();
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].record_index, 12);

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
    let copy = parse_parameter_scope(&copy_scope, &header)
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
    let generic_scope =
        parse_parameter_scope(&generic_references, &header).expect("generic-table Sketch scope");
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
    let decoded = exact_work_plane_frame(&bytes, &scope).expect("exact WorkPlane frame");
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
    let decoded = exact_work_plane_frame(&bytes, &extended_scope)
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
    let decoded = exact_work_plane_frame(&bytes, &direct_scope).expect("direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (direct_at + 66) as u64);
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
    let decoded = crate::design::decode::scopes::exact_move_operation(&bytes, &move_scope)
        .expect("class-368 Move frame");
    assert_eq!(decoded.transform, move_transform);
    assert_eq!(decoded.transform_offset, (move_at + 48) as u64);
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
        exact_solid_primitive(&bytes, &sphere_scope),
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
        exact_solid_primitive(&bytes, &torus_scope),
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
        exact_direct_face_operation(&bytes, &offset_scope),
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
        exact_direct_face_operation(&bytes, &offset_scope),
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
        exact_direct_face_operation(&bytes, &thicken_scope),
        Some(DesignDirectFaceOperation::Thicken {
            signed_thickness: -1.0,
            thickness_record_index: 74,
            ..
        })
    ));
    thicken_scope.direct_face_operation = exact_direct_face_operation(&bytes, &thicken_scope);
    let thicken_group = DesignConstructionOperandGroup {
        id: "thicken-group".into(),
        scope_record_index: thicken_scope.record_index,
        scope_reference_ordinal: 0,
        record_index: 200,
        byte_offset: 0,
        class_tag: "264".into(),
        member_count_offset: 0,
        members: vec![201],
        lost_edge_references: Vec::new(),
        member_offsets: vec![0],
        identity_record_index: 202,
        identity_record_offset: 0,
        role: 0x0000_0005_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 0,
        opaque_index: 1,
        opaque_index_offset: 0,
        opaque_scalar: 0.0,
        opaque_scalar_offset: 0,
        variant: false,
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
    shell_scope.direct_face_operation = exact_direct_face_operation(&bytes, &shell_scope);
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
    offset_scope.direct_face_operation = exact_direct_face_operation(&bytes, &offset_scope);
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
    bytes[thicken_at + 47] = 0;
    assert_eq!(exact_direct_face_operation(&bytes, &thicken_scope), None);

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
    extrude_scope.extrude_extent = Some(DesignExtrudeExtent::OneSidedDistance);
    extrude_scope.reference_members = vec![50, 75, 76, 51];
    assert_eq!(
        exact_fixed_extrude_parameters(&bytes, &extrude_scope),
        Some(DesignFixedExtrudeParameters {
            along_distance: -2.0,
            along_distance_record_index: 75,
            along_distance_offset: (bytes.len() - 2 * 115 + 40) as u64,
            taper_angle: 0.0,
            taper_angle_record_index: 76,
            taper_angle_offset: (bytes.len() - 115 + 40) as u64,
        })
    );
    extrude_scope.reference_members.push(75);
    assert_eq!(exact_fixed_extrude_parameters(&bytes, &extrude_scope), None);

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
        exact_fixed_fillet_parameters(&bytes, &fillet_scope),
        Some(DesignFixedFilletParameters {
            tangency_weight: 1.0,
            tangency_weight_record_index: 77,
            tangency_weight_offset: (fillet_start + 40) as u64,
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
        })
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
        exact_fixed_chamfer_parameters(&bytes, &chamfer_scope),
        Some(DesignFixedChamferParameters {
            distance: 0.04,
            distance_record_index: 86,
            distance_offset: (chamfer_scalar_start + 40) as u64,
        })
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
    let revolve_construction = exact_path_feature_construction(&bytes, &revolve_scope);
    assert_eq!(
        revolve_construction,
        Some(DesignPathFeatureConstruction::Revolve {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: (revolve_start + 25) as u64,
            angle: 3.5,
            angle_record_index: 1_779,
            angle_offset: (revolve_scalar_start + 40) as u64,
            opposite_angle_record_index: 1_780,
            opposite_angle_offset: (revolve_scalar_start + 116 + 40) as u64,
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
        crate::design::feature_project::project_fixed_revolve(
            &revolve_scope,
            &[revolve_profile, revolve_axis],
            &[],
        ),
        None
    );

    let loft_start = bytes.len();
    let mut loft = vec![0; 376];
    loft[29..33].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&loft);
    let mut loft_scope = scope.clone();
    loft_scope.byte_offset = loft_start as u64;
    loft_scope.kind = "Loft".into();
    loft_scope.frame_length = 376;
    assert_eq!(
        exact_path_feature_construction(&bytes, &loft_scope),
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
    let mut point = loft_group(0, 0x5_0000_0000);
    point.members = vec![10];
    let profile = loft_group(1, 0x43_0000_0000);
    let mut boundary = loft_group(2, 0x5_0000_0000);
    boundary.members = vec![20, 21, 22];
    assert!(matches!(
        crate::design::feature_project::project_fixed_loft(
            &loft_scope,
            &[point, profile, boundary],
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

    let sweep_start = bytes.len();
    let mut sweep = vec![0; 499];
    sweep[25..29].copy_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&sweep);
    let sweep_values: [f64; 6] = [0.8, 0.0, 1.0, 1.0, 6.632251157578453, 0.0];
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
        exact_path_feature_construction(&bytes, &sweep_scope),
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
            &[scope],
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
        recipe_index: 0,
        record_index: 303,
    };
    bind_parameter_companion_payloads(
        std::slice::from_mut(&mut companion),
        std::slice::from_ref(&parameter),
        &[],
        &[],
        &[],
        std::slice::from_ref(&recipe),
        &HashMap::from([("f3d:native".into(), 100)]),
    );
    assert_eq!(companion.payload_byte_offset, 58);
    assert_eq!(companion.payload_byte_length, 7);
    assert_eq!(companion.owned_recipe_ids, [recipe.id]);
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

    let scope = parse_parameter_scope(&bytes, &header).expect("localized Sketch scope");
    assert_eq!(scope.kind, "Esquisse");
    assert_eq!(scope.reference_members, [55, 56]);
    assert!(scope.entity_id.is_none());
}

#[test]
fn extrude_scope_discriminators_follow_optional_indexed_reference() {
    let scope = |kind: &str,
                 operation: u32,
                 extent: (u32, u32),
                 direction_reversed: bool,
                 start: u8,
                 conditional_reference: bool| {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"301");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.resize(100, 0);
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        let operation_offset = if conditional_reference {
            bytes[25] = 1;
            bytes[26..30].copy_from_slice(&77u32.to_le_bytes());
            38
        } else {
            28
        };
        bytes[operation_offset..operation_offset + 4].copy_from_slice(&operation.to_le_bytes());
        bytes[operation_offset + 4..operation_offset + 8].copy_from_slice(&extent.0.to_le_bytes());
        bytes[operation_offset + 8..operation_offset + 12].copy_from_slice(&extent.1.to_le_bytes());
        bytes[operation_offset + 12] = u8::from(direction_reversed);
        bytes[operation_offset + 13] = 1;
        bytes[operation_offset + 14] = start;
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&55u32.to_le_bytes());
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
        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: 12,
            class_tag: "301".into(),
            byte_offset: 0,
        };
        parse_parameter_scope(&bytes, &header).unwrap()
    };

    let direct = scope("Extrude", 1, (1, 2), false, 0, false);
    assert_eq!(direct.extrude_operation, Some(DesignExtrudeOperation::Join));
    assert_eq!(direct.extrude_operation_offset, Some(28));
    assert_eq!(
        direct.extrude_extent,
        Some(DesignExtrudeExtent::OneSidedDistance)
    );
    assert_eq!(direct.extrude_extent_offsets, Some([32, 36]));
    assert_eq!(direct.extrude_direction_reversed, Some(false));
    assert_eq!(direct.extrude_direction_reversed_offset, Some(40));
    assert_eq!(direct.extrude_start, Some(DesignExtrudeStart::ProfilePlane));
    assert_eq!(direct.extrude_start_offset, Some(42));
    let shifted = scope("Extrude", 3, (2, 0), false, 1, true);
    assert_eq!(
        shifted.extrude_operation,
        Some(DesignExtrudeOperation::Intersect)
    );
    assert_eq!(shifted.extrude_operation_offset, Some(38));
    assert_eq!(
        shifted.extrude_extent,
        Some(DesignExtrudeExtent::TwoSidedDistance)
    );
    assert_eq!(shifted.extrude_extent_offsets, Some([42, 46]));
    assert_eq!(
        shifted.extrude_start,
        Some(DesignExtrudeStart::OffsetProfilePlane)
    );
    assert_eq!(shifted.extrude_start_offset, Some(52));
    let to_face = scope("Extrusion", 2, (1, 1), true, 2, false);
    assert_eq!(to_face.kind, "Extrusion");
    assert_eq!(
        to_face.extrude_extent,
        Some(DesignExtrudeExtent::OneSidedToFace)
    );
    assert_eq!(to_face.extrude_direction_reversed, Some(true));
    assert_eq!(to_face.extrude_start, Some(DesignExtrudeStart::FromFace));
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

    let scope = parse_parameter_scope(&bytes, &header).expect("Coil scope");
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
        object_kind: Some(DesignObjectKind::Sketch),
        record_reference: Some(200),
        record_reference_offset: Some(1010),
        declared_reference_count: Some(0),
        reference_indices: Vec::new(),
        reference_offsets: Vec::new(),
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
    };

    let profile = parse_sketch_profile(&bytes, "f3d:Design/BulkStream.dat", 4, &header, &[entity])
        .expect("sketch-profile operand");
    assert_eq!(profile.scope_reference_ordinal, 4);
    assert_eq!(profile.entity_suffix, 172);
    assert_eq!(profile.entity_id, "0_172");
    assert_eq!(profile.paired_byte_offset, paired_at as u64);
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
fn edge_flange_scope_binds_each_edge_and_aggregate_operand() {
    fn reference(bytes: &mut [u8], at: usize, record_index: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }

    let references = [
        101, 102, 103, 111, 112, 113, 121, 122, 123, 131, 132, 133, 140, 141, 150, 151, 152, 153,
        160, 170,
    ];
    let mut bytes = vec![0; 814];
    bytes[30..34].copy_from_slice(&4u32.to_le_bytes());
    let common = 133;
    bytes[common..common + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&4u32.to_le_bytes());
    for (ordinal, record_index) in [101, 111, 121, 131].into_iter().enumerate() {
        reference(&mut bytes, common + 8 + ordinal * 11, record_index);
    }
    reference(&mut bytes, common + 52, 170);
    bytes[common + 63..common + 67].copy_from_slice(&2u32.to_le_bytes());
    reference(&mut bytes, common + 67, 141);
    reference(&mut bytes, common + 78, 140);
    bytes[common + 89..common + 93].copy_from_slice(&4u32.to_le_bytes());
    bytes[237..245].copy_from_slice(&0.25f64.to_le_bytes());
    bytes[251..255].copy_from_slice(&5u32.to_le_bytes());

    let operation =
        crate::design::decode::scopes::exact_edge_flange_operation(&bytes, 0, 814, &references)
            .expect("fixed EdgeFlange operation");
    assert_eq!(operation.edge_wrapper_record_indices, [101, 111, 121, 131]);
    assert_eq!(operation.edge_group_record_indices, [102, 112, 122, 132]);
    assert_eq!(operation.edge_operand_record_indices, [103, 113, 123, 133]);
    assert_eq!(operation.aggregate_group_record_index, 150);
    assert_eq!(
        operation.aggregate_operand_record_indices,
        [151, 152, 153, 160]
    );
    assert_eq!(operation.height_owner_record_index, 140);
    assert_eq!(operation.angle_owner_record_index, 141);
    assert_eq!(operation.bend_radius, 0.25);
    assert_eq!(operation.bend_radius_offset, 237);
}

#[test]
fn hem_scope_binds_parameters_edge_groups_and_rule_radius() {
    fn reference(bytes: &mut [u8], at: usize, record_index: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }

    let references = [201, 202, 203, 204, 205, 206, 207, 208];
    let mut bytes = vec![0; 494];
    bytes[85..89].copy_from_slice(&3u32.to_le_bytes());
    bytes[89..93].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 93, 203);
    reference(&mut bytes, 104, 208);
    bytes[115..119].copy_from_slice(&1u32.to_le_bytes());
    bytes[121..125].copy_from_slice(&4u32.to_le_bytes());
    reference(&mut bytes, 127, 201);
    reference(&mut bytes, 138, 202);
    bytes[156..164].copy_from_slice(&0.25f64.to_le_bytes());

    let operation = crate::design::decode::scopes::exact_hem_operation(&bytes, 0, 494, &references)
        .expect("fixed Hem operation");
    assert_eq!(operation.edge_wrapper_record_index, 203);
    assert_eq!(operation.edge_group_record_index, 204);
    assert_eq!(operation.edge_operand_record_index, 205);
    assert_eq!(operation.aggregate_group_record_index, 206);
    assert_eq!(operation.aggregate_operand_record_index, 207);
    assert_eq!(operation.gap_owner_record_index, 201);
    assert_eq!(operation.length_owner_record_index, 202);
    assert_eq!(operation.bend_radius, 0.25);
    assert_eq!(operation.bend_radius_offset, 156);
}

#[test]
fn extrude_operand_group_has_an_exact_counted_frame() {
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
        extrude_operation: None,
        extrude_operation_offset: None,
        extrude_extent: None,
        extrude_extent_offsets: None,
        extrude_direction_reversed: None,
        extrude_direction_reversed_offset: None,
        extrude_start: None,
        extrude_start_offset: None,
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
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        extrude_profile: None,
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
        .expect("counted Extrude operand group");
    assert_eq!(group.member_count_offset, 21);
    assert_eq!(group.members, [200, 201]);
    assert_eq!(group.member_offsets, [26, 37]);
    assert_eq!(group.identity_record_index, 300);
    assert_eq!(group.role, 0x0000_0008_0000_0000);
    assert_eq!(group.extrude_role, Some(DesignExtrudeOperandRole::Bodies));
    assert_eq!(group.opaque_index, 180);
    assert_eq!(group.opaque_scalar, 0.125);
    assert!(group.variant);
    assert_eq!(group.paired_byte_offset, paired_at as u64);

    bytes.drain(paired_at - 3..paired_at);
    let compact = parse_construction_operand_group(&bytes, &scope, 0, &record)
        .expect("compact counted operand group");
    assert_eq!(compact.members, [200, 201]);
    assert_eq!(compact.role, 0x0000_0008_0000_0000);
    assert_eq!(compact.paired_byte_offset, (paired_at - 3) as u64);

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
        exact_surface_stitch_operation(&bytes, 12, &[100, 200, 300, 301]),
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
        member_count_offset: 1021,
        members: vec![200],
        lost_edge_references: Vec::new(),
        member_offsets: vec![1026],
        identity_record_index: 300,
        identity_record_offset: 1043,
        role: 0x0000_0008_0000_0000,
        extrude_role: Some(DesignExtrudeOperandRole::Bodies),
        extrude_face_role: None,
        role_offset: 1053,
        opaque_index: 180,
        opaque_index_offset: 1071,
        opaque_scalar: 0.125,
        opaque_scalar_offset: 1075,
        variant: false,
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
fn nested_entity_selection_member_retains_both_identity_values() {
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
        member_count_offset: 921,
        members: vec![100],
        lost_edge_references: Vec::new(),
        member_offsets: vec![926],
        identity_record_index: 200,
        identity_record_offset: 943,
        role: 0x0000_0005_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 953,
        opaque_index: 1,
        opaque_index_offset: 971,
        opaque_scalar: 0.0,
        opaque_scalar_offset: 975,
        variant: false,
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
    assert_eq!(operand.secondary_identity, 183);
    assert_eq!(operand.identity_record_offset, identity_at as u64);
    assert_eq!(operand.next_byte_offset, next_at as u64);
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
        member_count_offset: 921,
        members: vec![100],
        lost_edge_references: Vec::new(),
        member_offsets: vec![926],
        identity_record_index: 200,
        identity_record_offset: 943,
        role: 0x0000_0005_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 953,
        opaque_index: 1,
        opaque_index_offset: 971,
        opaque_scalar: 0.0,
        opaque_scalar_offset: 975,
        variant: false,
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
        recipe_index: 0,
        record_index: 0,
    };

    let operand = parse_body_recipe_operand(&bytes, &group, 0, &record, &recipe)
        .expect("body recipe operand");
    assert_eq!(operand.references.len(), 2);
    assert_eq!(operand.references[0].design_reference, 2265);
    assert_eq!(operand.references[0].form, 3);
    assert_eq!(operand.references[1].design_reference, 2266);
    assert_eq!(operand.references[1].form, 32);
    assert_eq!(operand.nested_record_index, 103);
    assert_eq!(operand.recipe_id, recipe.id);
    assert_eq!(operand.next_byte_offset, next_at as u64);
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
        extrude_operation: None,
        extrude_operation_offset: None,
        extrude_extent: None,
        extrude_extent_offsets: None,
        extrude_direction_reversed: None,
        extrude_direction_reversed_offset: None,
        extrude_start: None,
        extrude_start_offset: None,
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
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        extrude_profile: None,
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
        extrude_operation: None,
        extrude_operation_offset: None,
        extrude_extent: None,
        extrude_extent_offsets: None,
        extrude_direction_reversed: None,
        extrude_direction_reversed_offset: None,
        extrude_start: None,
        extrude_start_offset: None,
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
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        extrude_profile: None,
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
        member_count_offset: 921,
        members: vec![100],
        lost_edge_references: vec!["f3d:Design/BulkStream.dat:lost-edge#1".into()],
        member_offsets: vec![926],
        identity_record_index: 91,
        identity_record_offset: 950,
        role: 0x0000_0008_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 960,
        opaque_index: 1,
        opaque_index_offset: 968,
        opaque_scalar: 0.0,
        opaque_scalar_offset: 972,
        variant: false,
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
        &[PersistentSubentityTag {
            id: "f3d:asm:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(FaceId("f3d:brep:entity#50".into())),
            selector: 1,
            token: "3".into(),
            design_references: vec![303],
            ordinal: 0,
        }],
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
        id: "dimension-recipe".into(),
        companion_record_index: 1,
        recipe_ordinal: 0,
        recipe_id: "recipe".into(),
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
                id: "f3d:asm:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(FaceId("f3d:brep:entity#50".into())),
                selector: 1,
                token: "3".into(),
                design_references: vec![303],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:asm:persistent-subentity-tag#2".into(),
                target: AttributeTarget::Face(FaceId("f3d:brep:entity#51".into())),
                selector: 1,
                token: "4".into(),
                design_references: vec![303],
                ordinal: 1,
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
    let group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
        scope_record_index: face_scope.record_index,
        scope_reference_ordinal: 0,
        record_index: 90,
        byte_offset: 900,
        class_tag: "306".into(),
        member_count_offset: 920,
        members: vec![operand.record_index],
        lost_edge_references: Vec::new(),
        member_offsets: vec![924],
        identity_record_index: 91,
        identity_record_offset: 935,
        role: 0x0000_0011_0000_0000,
        extrude_role: Some(DesignExtrudeOperandRole::Faces),
        extrude_face_role: Some(DesignExtrudeFaceRole::Termination),
        role_offset: 946,
        opaque_index: 1,
        opaque_index_offset: 954,
        opaque_scalar: 0.0,
        opaque_scalar_offset: 958,
        variant: false,
        paired_class_tag: "259".into(),
        paired_byte_offset: 980,
    };
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&operand)),
        Some(FaceSelection::Resolved { faces, native })
            if faces == [FaceId("f3d:brep:entity#51".into())] && native == group.id
    ));
    operand
        .unreferenced_candidate_faces
        .push(FaceId("f3d:brep:entity#50".into()));
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
    fn placement_frame(
        record_index: u32,
        length: usize,
        transform: Option<[[f64; 4]; 4]>,
    ) -> Vec<u8> {
        let mut bytes = vec![0; length];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"356");
        bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
        if let Some(transform) = transform {
            for (ordinal, value) in transform.into_iter().flatten().enumerate() {
                let at = 55 + ordinal * 8;
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"259");
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes
    }

    let compact =
        parse_sketch_placement_candidates(&placement_frame(185, 201, None), 177, "0_172", 172, 185);
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
    let explicit = parse_sketch_placement_candidates(
        &placement_frame(1773, 329, Some(transform)),
        1765,
        "0_1761",
        1761,
        1773,
    );
    assert_eq!(explicit.len(), 1);
    assert_eq!(explicit[0].frame_length, 329);
    assert_eq!(explicit[0].transform, transform);
    assert_eq!(explicit[0].transform_offset, Some(55));
}

#[test]
fn entity_genesis_placement_decodes_compact_and_explicit_frames() {
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

    let compact = parse_sketch_placement_candidates(
        &genesis_frame(214, 213, 1, None),
        206,
        "0_201",
        201,
        214,
    );
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
    let explicit = parse_sketch_placement_candidates(
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
    assert!(parse_sketch_placement_candidates(
        &genesis_frame(214, 213, 0, None),
        206,
        "0_201",
        201,
        214
    )
    .is_empty());
    assert!(parse_sketch_placement_candidates(
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
    assert!(parse_sketch_placement_candidates(&workplane_like, 206, "0_201", 201, 214).is_empty());
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
        object_kind: Some(DesignObjectKind::Sketch),
        record_reference: None,
        record_reference_offset: None,
        declared_reference_count: None,
        reference_indices: Vec::new(),
        reference_offsets: Vec::new(),
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
    };
    let placement = crate::design::decode::sketch::parse_member_run_head_placement(&bytes, &entity)
        .expect("feature-owned sketch placement");
    assert_eq!(placement.record_index, 200);
    assert_eq!(placement.byte_offset, head_at as u64);
    assert_eq!(placement.paired_byte_offset, paired_at as u64);
    assert_eq!(placement.transform, identity_matrix());
    assert!(placement.member_run_head);
    assert_eq!(placement.scope_record_index, None);

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
    let compact = crate::design::decode::sketch::parse_member_run_head_placement(&bytes, &entity)
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
        object_kind: Some(DesignObjectKind::Sketch),
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
        member_roles: Vec::new(),
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
        member_roles: Vec::new(),
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
        member_roles: vec![3, 5, 1, 1],
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
fn rectangular_pattern_instances_require_exact_translated_geometry() {
    let source = SketchGeometry::Line {
        start: Point2::new(1.0, 2.0),
        end: Point2::new(4.0, 6.0),
    };
    let translated = SketchGeometry::Line {
        start: Point2::new(11.0, -1.0),
        end: Point2::new(14.0, 3.0),
    };
    assert!(
        crate::design::constraints::translated_sketch_geometry_matches(
            &source,
            &translated,
            Point2::new(10.0, -3.0),
        )
    );
    let reversed = SketchGeometry::Line {
        start: Point2::new(14.0, 3.0),
        end: Point2::new(11.0, -1.0),
    };
    assert!(
        !crate::design::constraints::translated_sketch_geometry_matches(
            &source,
            &reversed,
            Point2::new(10.0, -3.0),
        )
    );
    let resized = SketchGeometry::Circle {
        center: Point2::new(12.0, 0.0),
        radius: cadmpeg_ir::features::Length(3.1),
    };
    assert!(
        !crate::design::constraints::translated_sketch_geometry_matches(
            &SketchGeometry::Circle {
                center: Point2::new(2.0, 3.0),
                radius: cadmpeg_ir::features::Length(3.0),
            },
            &resized,
            Point2::new(10.0, -3.0),
        )
    );
}

#[test]
fn rectangular_pattern_derives_spacing_from_internal_span_scalars() {
    let entity = |id: &str, u| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(u, 4.0),
        },
    };
    let seed = entity("generated:point#seed", 2.0);
    let second = entity("generated:point#second", 17.0);
    let third = entity("generated:point#third", 32.0);
    let relation = SketchRelation {
        id: "f3d:native:sketch-relation#rectangular".into(),
        record_index: 10,
        class_tag: "300".into(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 1,
        owner_entity_id: "0_1".into(),
        auxiliary_references: vec![20, 21, 22, 23],
        auxiliary_reference_offsets: Vec::new(),
        members: vec![1, 2, 3],
        resolved_members: Vec::new(),
        member_offsets: Vec::new(),
        owner_reference_offset: 0,
        state: 0x2000_0000,
        constraint_kinds: vec![SketchConstraintKind::RectangularPattern],
        unknown_constraint_bits: 0,
        member_roles: vec![1, 0, 0],
        entity_genesis: None,
        pattern: Some(crate::records::SketchPatternDefinition::Rectangular {
            directions: [
                crate::records::SketchPatternDirection {
                    count_parameter: 20,
                    distance_parameter: 21,
                    evaluated_count: 3,
                    direction: [1.0, 0.0, 0.0],
                    evaluated_distance: 3.0,
                },
                crate::records::SketchPatternDirection {
                    count_parameter: 22,
                    distance_parameter: 23,
                    evaluated_count: 1,
                    direction: [0.0, 1.0, 0.0],
                    evaluated_distance: 0.0,
                },
            ],
        }),
        return_members: vec![1, 2, 3],
        resolved_return_members: Vec::new(),
        return_member_offsets: Vec::new(),
        raw_bytes: Vec::new(),
    };
    let Some(SketchConstraintDefinition::RectangularPattern {
        directions,
        instances,
    }) = crate::design::constraints::exact_rectangular_pattern(
        &relation,
        "native",
        &[],
        &[&seed, &second, &third],
    )
    else {
        panic!("rectangular pattern did not resolve");
    };
    assert_eq!(directions[0].spacing.0, 15.0);
    assert_eq!(directions[1].spacing.0, 0.0);
    assert_eq!(directions[0].span_parameter, None);
    assert_eq!(directions[0].count_parameter, None);
    assert_eq!(
        instances
            .iter()
            .map(|instance| instance.indices)
            .collect::<Vec<_>>(),
        [[0, 0], [1, 0], [2, 0]]
    );
}

#[test]
fn circular_pattern_resolves_full_and_partial_instance_distributions() {
    let entity = |id: &str, geometry| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("generated:sketch#0".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let center = entity(
        "generated:point#center",
        SketchGeometry::Point {
            position: Point2::new(2.0, -3.0),
        },
    );
    let circle = |id: &str, angle: f64| {
        entity(
            id,
            SketchGeometry::Circle {
                center: Point2::new(2.0 + 5.0 * angle.cos(), -3.0 + 5.0 * angle.sin()),
                radius: cadmpeg_ir::features::Length(0.75),
            },
        )
    };
    let seed = circle("generated:circle#seed", 0.0);
    let middle = circle("generated:circle#middle", std::f64::consts::FRAC_PI_2);
    let last = circle("generated:circle#last", std::f64::consts::PI);
    let relation = |angle| SketchRelation {
        id: "f3d:native:sketch-relation#circular".into(),
        record_index: 10,
        class_tag: "300".into(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 1,
        owner_entity_id: "0_1".into(),
        auxiliary_references: vec![20, 21],
        auxiliary_reference_offsets: Vec::new(),
        members: vec![1, 2, 3, 4],
        resolved_members: Vec::new(),
        member_offsets: Vec::new(),
        owner_reference_offset: 0,
        state: 0x1000_0000,
        constraint_kinds: vec![SketchConstraintKind::CircularPattern],
        unknown_constraint_bits: 0,
        member_roles: vec![1, 1, 0, 0],
        entity_genesis: None,
        pattern: Some(crate::records::SketchPatternDefinition::Circular {
            angle_parameter: 20,
            count_parameter: 21,
            evaluated_angle: angle,
            evaluated_count: 3,
        }),
        return_members: vec![2, 3, 4, 1],
        resolved_return_members: Vec::new(),
        return_member_offsets: Vec::new(),
        raw_bytes: Vec::new(),
    };
    let members = [&center, &seed, &middle, &last];
    let returned = [&seed, &middle, &last, &center];
    let Some(SketchConstraintDefinition::CircularPattern {
        center: actual_center,
        angle,
        count,
        instances,
        ..
    }) = crate::design::constraints::exact_circular_pattern(
        &relation(std::f64::consts::PI),
        "native",
        &[],
        &members,
        &returned,
    )
    else {
        panic!("partial circular pattern did not resolve");
    };
    assert_eq!(actual_center, center.id);
    assert_eq!(angle.0, std::f64::consts::PI);
    assert_eq!(count, 3);
    assert_eq!(
        instances
            .iter()
            .map(|instance| instance.angle.0)
            .collect::<Vec<_>>(),
        [0.0, std::f64::consts::FRAC_PI_2, std::f64::consts::PI]
    );

    let full_middle = circle("generated:circle#full-middle", std::f64::consts::TAU / 3.0);
    let full_last = circle(
        "generated:circle#full-last",
        2.0 * std::f64::consts::TAU / 3.0,
    );
    let full_members = [&center, &seed, &full_middle, &full_last];
    let full_returned = [&seed, &full_middle, &full_last, &center];
    assert!(matches!(
        crate::design::constraints::exact_circular_pattern(
            &relation(std::f64::consts::TAU),
            "native",
            &[],
            &full_members,
            &full_returned,
        ),
        Some(SketchConstraintDefinition::CircularPattern { ref instances, .. })
            if crate::design::constraints::scalar_close(instances[1].angle.0, std::f64::consts::TAU / 3.0)
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
        prefix_value: 0,
        prefix_value_offset: 0,
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
        variant: 0,
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
        &[],
    );
    assert_eq!(spatial_constraints.len(), 1, "{spatial_constraints:#?}");
    assert!(matches!(
        &spatial_constraints[0],
        cadmpeg_ir::sketches::SpatialSketchConstraint {
            sketch: actual_sketch,
            definition: SpatialSketchConstraintDefinition::Native {
                parameter: Some(actual_parameter),
                ..
            },
            ..
        } if actual_sketch == &spatial_sketch.id
            && actual_parameter == &neutral_parameter_id_parts(stream, 4)
    ));

    let mut zero_parameter = parameter;
    zero_parameter.evaluated_value = 0.0;
    let mut duplicate_pair = pair;
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

    let mut group_owner = owner;
    group_owner.companion_record_index = group.companion_record_index;
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
        variant: 0,
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
        prefix_value: 0,
        prefix_value_offset: 0,
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
        variant: 0,
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
    assert_eq!(
        projected_parameter.0,
        format!("f3d:model:parameter#{}:{stream}4", stream.len())
    );
    assert_eq!(measurements.len(), 2);
    assert!(measurements.iter().all(|measurement| matches!(
        measurement,
        cadmpeg_ir::sketches::SketchDistanceMeasurement::Distance { .. }
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
                entities,
                parameter: Some(actual_parameter),
                operands,
            },
            native_ref: Some(native_ref),
            ..
        }] if native_kind == "Linear Dimension-4"
            && entities.is_empty()
            && actual_parameter.0 == format!("f3d:model:parameter#{}:{stream}4", stream.len())
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

    let mut empty_companion = companion;
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
    assert!(matches!(
        retained.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::Native { operands, .. },
            ..
        }] if matches!(operands.as_slice(), [cadmpeg_ir::sketches::SketchNativeOperand {
            native_field: Some(field),
            ..
        }] if field == "companion")
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
    let entities = vec![
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
        object_kind: Some(DesignObjectKind::Sketch),
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
        member_roles: Vec::new(),
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
        extrude_operation: Some(DesignExtrudeOperation::NewBody),
        extrude_operation_offset: Some(128),
        extrude_extent: Some(DesignExtrudeExtent::OneSidedDistance),
        extrude_extent_offsets: Some([132, 136]),
        extrude_direction_reversed: Some(false),
        extrude_direction_reversed_offset: Some(140),
        extrude_start: Some(DesignExtrudeStart::ProfilePlane),
        extrude_start_offset: Some(142),
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
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        extrude_profile: None,
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
        variant: 0,
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
        extrude_operation: None,
        extrude_operation_offset: None,
        extrude_extent: None,
        extrude_extent_offsets: None,
        extrude_direction_reversed: None,
        extrude_direction_reversed_offset: None,
        extrude_start: None,
        extrude_start_offset: None,
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
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        extrude_profile: None,
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
        Angle, BooleanOp, Extent, ExtrudeDirection, ExtrudeStart, FaceSelection, ProfileRef,
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
        extrude_operation: Some(DesignExtrudeOperation::NewBody),
        extrude_operation_offset: Some(128),
        extrude_extent: Some(DesignExtrudeExtent::OneSidedDistance),
        extrude_extent_offsets: Some([132, 136]),
        extrude_direction_reversed: Some(false),
        extrude_direction_reversed_offset: Some(140),
        extrude_start: Some(DesignExtrudeStart::ProfilePlane),
        extrude_start_offset: Some(142),
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
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
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
    )
    .expect("typed blind Extrude");
    assert!(matches!(
        &blind,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(profile),
            direction: ExtrudeDirection::ProfileNormal,
            extent: Extent::Blind { length: Length(5.5) },
            op: BooleanOp::NewBody,
            draft: Some(Angle(0.2)),
            ..
        } if profile == &neutral_sketch_id(&placement)
    ));
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
        },
    );
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Native(ref native),
            ..
        } if native == &selection.id
    ));
    scope.extrude_direction_reversed = Some(true);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
    )
    .is_none());
    scope.extrude_direction_reversed = Some(false);
    let unsupported = parameter("UnclassifiedControl", "mm", 1.0);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &unsupported)],
        &[],
        &[],
        std::slice::from_ref(&placement),
    )
    .is_none());
    let side_two_taper = parameter("Side2TaperAngle", "deg", -0.3);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &side_two_taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
    )
    .is_none());
    let invalid_taper = parameter("TaperAngle", "native-unit", 0.2);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &invalid_taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
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
    sketch_scope.extrude_operation = None;
    sketch_scope.extrude_extent = None;
    sketch_scope.extrude_start = None;
    sketch_scope.extrude_profile = None;
    let scopes = [sketch_scope, scope.clone()];
    let (mut features, _) = project_parameter_design(
        &[owned_along],
        &[owner],
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

    let body_group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#101".into(),
        scope_record_index: 12,
        scope_reference_ordinal: 1,
        record_index: 101,
        byte_offset: 1000,
        class_tag: "332".into(),
        member_count_offset: 1021,
        members: vec![200],
        lost_edge_references: Vec::new(),
        member_offsets: vec![1026],
        identity_record_index: 300,
        identity_record_offset: 1044,
        role: 0x0000_0008_0000_0000,
        extrude_role: Some(DesignExtrudeOperandRole::Bodies),
        extrude_face_role: None,
        role_offset: 1054,
        opaque_index: 180,
        opaque_index_offset: 1072,
        opaque_scalar: 0.125,
        opaque_scalar_offset: 1076,
        variant: false,
        paired_class_tag: "259".into(),
        paired_byte_offset: 1125,
    };
    scope.extrude_operation = Some(DesignExtrudeOperation::Join);
    let target_body = project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        std::slice::from_ref(&body_group),
        &[],
        std::slice::from_ref(&placement),
    )
    .expect("typed target-body Extrude");
    assert!(matches!(
        target_body,
        FeatureDefinition::Extrude {
            op: BooleanOp::Join,
            ..
        }
    ));

    let mut profile_group = body_group.clone();
    profile_group.id = "f3d:Design/BulkStream.dat:operand-group#104".into();
    profile_group.record_index = 104;
    profile_group.extrude_role = Some(DesignExtrudeOperandRole::Profile);
    profile_group.role = 0x0000_0041_0000_0000;
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[body_group.clone(), profile_group.clone()],
        &[],
        std::slice::from_ref(&placement),
    )
    .is_none());
    profile_group.members = vec![100];
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[body_group.clone(), profile_group.clone()],
        &[],
        std::slice::from_ref(&placement),
    )
    .is_some());
    let mut native_profile_scope = scope.clone();
    native_profile_scope.extrude_profile = None;
    let reversed_native_profile = project_extrude(
        &native_profile_scope,
        &[(0, &parameter("AlongDistance", "mm", -0.2)), (1, &taper)],
        &[body_group.clone(), profile_group.clone()],
        &[],
        &[],
    )
    .expect("typed reversed Extrude with a native profile");
    assert!(matches!(
        reversed_native_profile,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Native(ref native),
            direction: ExtrudeDirection::ReversedProfileNormal,
            extent: Extent::Blind {
                length: Length(2.0)
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
    scope.extrude_start = Some(DesignExtrudeStart::FromFace);
    assign_extrude_face_roles(&scope, &mut ordered_faces);
    assert_eq!(
        ordered_faces.map(|group| group.extrude_face_role),
        [
            Some(DesignExtrudeFaceRole::Start),
            Some(DesignExtrudeFaceRole::Termination)
        ]
    );
    scope.extrude_start = Some(DesignExtrudeStart::ProfilePlane);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[body_group.clone(), face_group.clone()],
        &[],
        std::slice::from_ref(&placement),
    )
    .is_none());

    let profile_offset = parameter("ProfileOffset", "mm", 0.1);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &profile_offset)],
        std::slice::from_ref(&body_group),
        &[],
        std::slice::from_ref(&placement),
    )
    .is_none());
    scope.extrude_start = Some(DesignExtrudeStart::OffsetProfilePlane);
    let offset_start = project_extrude(
        &scope,
        &[(0, &along), (1, &profile_offset)],
        std::slice::from_ref(&body_group),
        &[],
        std::slice::from_ref(&placement),
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
    scope.extrude_start = Some(DesignExtrudeStart::ProfilePlane);

    scope.extrude_operation = Some(DesignExtrudeOperation::NewBody);
    let against = parameter("AgainstDistance", "mm", -0.05);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &against)],
        &[],
        &[],
        std::slice::from_ref(&placement),
    )
    .is_none());
    scope.extrude_extent = Some(DesignExtrudeExtent::TwoSidedDistance);
    let two_sided = project_extrude(
        &scope,
        &[(0, &along), (1, &against), (2, &side_two_taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
    )
    .expect("typed two-sided Extrude");
    assert!(matches!(
        two_sided,
        FeatureDefinition::Extrude {
            extent: Extent::TwoSided {
                first: Length(5.5),
                second: Length(0.5),
            },
            second_draft: Some(Angle(-0.3)),
            ..
        }
    ));
    scope.extrude_direction_reversed = Some(true);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &against), (2, &side_two_taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
    )
    .is_none());
    scope.extrude_direction_reversed = Some(false);

    scope.extrude_extent = Some(DesignExtrudeExtent::OneSidedDistance);
    let reversed_along = parameter("AlongDistance", "mm", -0.6);
    let reversed = project_extrude(
        &scope,
        &[(0, &reversed_along)],
        &[],
        &[],
        std::slice::from_ref(&placement),
    )
    .expect("typed reversed Extrude");
    assert!(matches!(
        reversed,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ReversedProfileNormal,
            extent: Extent::Blind {
                length: Length(6.0)
            },
            ..
        }
    ));

    scope.extrude_operation = Some(DesignExtrudeOperation::Join);
    scope.extrude_extent = Some(DesignExtrudeExtent::OneSidedToFace);
    scope.extrude_direction_reversed = Some(true);
    face_group.extrude_face_role = Some(DesignExtrudeFaceRole::Termination);
    let side_offset = parameter("Side1Offset", "mm", 0.025);
    let to_face = project_extrude(
        &scope,
        &[(0, &side_offset), (1, &taper)],
        &[body_group.clone(), face_group.clone()],
        &[],
        std::slice::from_ref(&placement),
    )
    .expect("typed reversed to-face Extrude");
    assert!(matches!(
        to_face,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ReversedProfileNormal,
            extent: Extent::ToFace {
                face: FaceSelection::Native(ref id),
                offset: Some(Length(0.25)),
            },
            ..
        } if id == &face_group.id
    ));

    scope.extrude_start = Some(DesignExtrudeStart::FromFace);
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

    scope.extrude_operation = Some(DesignExtrudeOperation::NewBody);
    scope.extrude_extent = Some(DesignExtrudeExtent::TwoSidedDistance);
    scope.extrude_direction_reversed = Some(false);
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
    )
    .expect("typed selected-face-start two-sided Extrude");
    assert!(matches!(
        from_face_two_sided,
        FeatureDefinition::Extrude {
            start: ExtrudeStart::FromFace {
                face: FaceSelection::Native(ref id),
                offset: None,
            },
            extent: Extent::TwoSided {
                first: Length(5.5),
                second: Length(0.5),
            },
            ..
        } if id == &start_group.id
    ));
}

#[test]
fn edge_treatments_project_typed_dimensions_and_native_selections() {
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
        extrude_operation: None,
        extrude_operation_offset: None,
        extrude_extent: None,
        extrude_extent_offsets: None,
        extrude_direction_reversed: None,
        extrude_direction_reversed_offset: None,
        extrude_start: None,
        extrude_start_offset: None,
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
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        extrude_profile: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: byte_offset + 200,
    };
    let scopes = [scope(12, 100, "Fillet"), scope(22, 400, "Chamfer")];
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
            member_count_offset: 1_021 + u64::from(scope_reference_ordinal),
            members: vec![record_index + 100],
            lost_edge_references: Vec::new(),
            member_offsets: vec![1_026 + u64::from(scope_reference_ordinal)],
            identity_record_index: record_index + 1,
            identity_record_offset: 1_050 + u64::from(scope_reference_ordinal),
            role: 0x0000_0008_0000_0000,
            extrude_role: None,
            extrude_face_role: None,
            role_offset: 1_060 + u64::from(scope_reference_ordinal),
            opaque_index: 100,
            opaque_index_offset: 1_068 + u64::from(scope_reference_ordinal),
            opaque_scalar: 0.5,
            opaque_scalar_offset: 1_072 + u64::from(scope_reference_ordinal),
            variant: false,
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
        None
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
        extrude_operation: None,
        extrude_operation_offset: None,
        extrude_extent: None,
        extrude_extent_offsets: None,
        extrude_direction_reversed: None,
        extrude_direction_reversed_offset: None,
        extrude_start: None,
        extrude_start_offset: None,
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
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        extrude_profile: None,
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
        member_count_offset: 1021 + u64::from(ordinal) * 200,
        member_offsets: (0..members.len())
            .map(|index| 1026 + u64::from(ordinal) * 200 + index as u64 * 11)
            .collect(),
        members,
        lost_edge_references: Vec::new(),
        identity_record_index: 300 + ordinal,
        identity_record_offset: 1100 + u64::from(ordinal) * 200,
        role: 0x0000_0008_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 1110 + u64::from(ordinal) * 200,
        opaque_index: 100,
        opaque_index_offset: 1128 + u64::from(ordinal) * 200,
        opaque_scalar: 0.5,
        opaque_scalar_offset: 1132 + u64::from(ordinal) * 200,
        variant: false,
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
        tangency_weight: 1.0,
        tangency_weight_record_index: 10,
        tangency_weight_offset: 100,
        radii: vec![0.5],
        radius_record_indexes: vec![20],
        radius_offsets: vec![200],
        intermediate_parameters: Vec::new(),
        intermediate_parameter_record_indexes: Vec::new(),
        intermediate_parameter_offsets: Vec::new(),
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
    let mut patch_group = group(100, 0, vec![200]);
    patch_group.role = 0x0000_0004_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(&patch_scope, std::slice::from_ref(&patch_group)),
        Some(FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::Native(ref native)
            ),
            support_faces: cadmpeg_ir::features::FaceSelection::Faces(ref faces),
            continuity: None,
            merge_result: None,
        }) if native == &patch_group.id && faces.is_empty()
    ));

    patch_scope.frame_length = 398;
    patch_scope.reference_members = vec![100, 200, 300, 101, 201, 301, 102];
    let mut second_patch_group = group(101, 3, vec![201]);
    second_patch_group.role = 0x0000_0004_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &patch_scope,
            &[patch_group.clone(), second_patch_group]
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
    patch_group.role = 0x0000_0041_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(&patch_scope, std::slice::from_ref(&patch_group)),
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
        extrude_operation: None,
        extrude_operation_offset: None,
        extrude_extent: None,
        extrude_extent_offsets: None,
        extrude_direction_reversed: None,
        extrude_direction_reversed_offset: None,
        extrude_start: None,
        extrude_start_offset: None,
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
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        extrude_profile: None,
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
        extrude_operation: None,
        extrude_operation_offset: None,
        extrude_extent: None,
        extrude_extent_offsets: None,
        extrude_direction_reversed: None,
        extrude_direction_reversed_offset: None,
        extrude_start: None,
        extrude_start_offset: None,
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
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        extrude_profile: None,
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
        variant: 0,
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
    let mut record = vec![0u8; 127];
    record[0..4].copy_from_slice(&3u32.to_le_bytes());
    record[4..7].copy_from_slice(b"286");
    record[7..11].copy_from_slice(&1239u32.to_le_bytes());
    record[19] = 1;
    record[20..24].copy_from_slice(&3u32.to_le_bytes());
    for (marker, reference) in [(24, 1224u32), (39, 1228), (54, 1236), (69, 0), (74, 1041)] {
        record[marker] = 1;
        record[marker + 1..marker + 5].copy_from_slice(&reference.to_le_bytes());
    }
    record[35..39].copy_from_slice(&3u32.to_le_bytes());
    record[50..54].copy_from_slice(&1u32.to_le_bytes());
    record[85..93].copy_from_slice(&4u64.to_le_bytes());
    record[93..97].copy_from_slice(&3u32.to_le_bytes());
    for (marker, reference) in [(97, 1224u32), (108, 1228), (119, 1236)] {
        record[marker] = 1;
        record[marker + 1..marker + 5].copy_from_slice(&reference.to_le_bytes());
    }
    let mut bytes = record.clone();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"277");
    bytes.extend_from_slice(&1240u32.to_le_bytes());

    assert_eq!(next_indexed_record_offset(&bytes, 11), Some(127));
    let parsed = parse_sketch_relation(&record, &HashSet::from([1041])).unwrap();
    assert_eq!(parsed.members, [1224, 1228, 1236]);
    assert_eq!(parsed.member_roles, [3, 1, 0]);
    assert_eq!(parsed.auxiliary_references, [0]);
    assert_eq!(parsed.owner_reference, 1041);
    assert_eq!(parsed.state, 4);
    assert_eq!(parsed.state_offset, 85);
    assert_eq!(parsed.entity_genesis, None);
    assert_eq!(parsed.return_members, [1224, 1228, 1236]);
    assert_eq!(parsed.parsed_end, 124);
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
    let record = genesis_relation_record(
        &[(2394, 0), (2403, 0), (2404, 0)],
        2,
        &auxiliary,
        1425,
        0x100_0000_0000,
        &[2403, 2404],
    );
    let parsed = parse_sketch_relation(&record, &HashSet::from([1425])).unwrap();
    assert_eq!(parsed.members, [2394, 2403, 2404]);
    assert_eq!(parsed.member_roles, [0, 0, 0]);
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
    let parsed = parse_sketch_relation(&record, &HashSet::from([201])).unwrap();
    assert_eq!(parsed.members, [237, 304]);
    assert_eq!(parsed.member_roles, [1, 0]);
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

#[test]
fn sketch_text_record_decodes_typed_content_and_metrics() {
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
    bytes.extend_from_slice(&3u32.to_le_bytes());
    push_ascii(&mut bytes, "EntityGenesis");
    push_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
    bytes.extend_from_slice(&4u64.to_le_bytes());
    push_ascii(&mut bytes, "textex_tag");
    push_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
    bytes.extend_from_slice(&109u64.to_le_bytes());
    push_ascii(&mut bytes, "txt_tag_base");
    push_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
    bytes.extend_from_slice(&305u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    bytes.extend_from_slice(&0.0f64.to_le_bytes());
    bytes.extend_from_slice(&0.0f32.to_le_bytes());
    bytes.extend_from_slice(&1.0f32.to_le_bytes());
    push_utf16(&mut bytes, "Arial");
    bytes.push(0);
    bytes.extend_from_slice(&0.8f64.to_le_bytes());
    push_reference(&mut bytes, 307);
    bytes.extend_from_slice(&[0; 6]);
    bytes.push(1);
    bytes.extend_from_slice(&[0; 6]);
    push_utf16(&mut bytes, "path text");
    push_reference(&mut bytes, 310);
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&[0; 16]);
    push_reference(&mut bytes, 201);
    bytes.extend_from_slice(&[0; 6]);

    let text = crate::design::decode::sketch::decode_sketch_text_record(
        &bytes,
        "Design/BulkStream.dat",
        "329".into(),
        304,
        7,
    )
    .expect("sketch text record");
    assert_eq!(text.record_index, 304);
    assert_eq!(text.owner_reference, 201);
    assert_eq!(text.entity_genesis, 4);
    assert_eq!(text.persistent_id, 109);
    assert_eq!(text.base_id, 305);
    assert_eq!(text.text, "path text");
    assert_eq!(text.font_family, "Arial");
    assert_eq!(text.height, 10.0);
    assert_eq!(text.width_factor, 0.8);
    assert_eq!(text.first_reference, 307);
    assert_eq!(text.second_reference, 310);
}

#[test]
fn text_path_relation_projects_typed_entities_and_scaled_glyph_placements() {
    use cadmpeg_ir::features::Length;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchGeometry, SketchId,
    };

    let sketch = SketchId("sketch".into());
    let path = SketchEntity {
        id: SketchEntityId("path".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
    };
    let text = SketchEntity {
        id: SketchEntityId("text".into()),
        sketch,
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Text {
            text: "A".into(),
            font_family: "Arial".into(),
            height: Length(10.0),
            width_factor: 0.8,
        },
    };
    let mut glyph = [[0.0; 4]; 4];
    for ordinal in 0..4 {
        glyph[ordinal][ordinal] = 1.0;
    }
    glyph[0][3] = 0.5;
    let relation = SketchRelation {
        id: "f3d:Design/BulkStream.dat:sketch-relation#3".into(),
        record_index: 3,
        class_tag: "413".into(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 1,
        owner_entity_id: String::new(),
        auxiliary_references: vec![2],
        auxiliary_reference_offsets: Vec::new(),
        members: vec![1, 2],
        resolved_members: Vec::new(),
        member_offsets: Vec::new(),
        owner_reference_offset: 0,
        state: 0x200_0000_0000,
        constraint_kinds: vec![SketchConstraintKind::TextPath],
        unknown_constraint_bits: 0,
        member_roles: vec![1, 0],
        entity_genesis: Some(2),
        pattern: Some(crate::records::SketchPatternDefinition::TextPath {
            text_reference: 2,
            glyph_transforms: vec![glyph],
        }),
        return_members: vec![1],
        resolved_return_members: Vec::new(),
        return_member_offsets: Vec::new(),
        raw_bytes: Vec::new(),
    };
    let projected = std::collections::HashMap::from([(("scope", 1), &path), (("scope", 2), &text)]);
    let definition =
        crate::design::constraints::exact_text_relation(&relation, "scope", &projected)
            .expect("typed text path");
    assert!(matches!(
        definition,
        SketchConstraintDefinition::TextPath {
            text: ref text_id,
            path: ref path_id,
            ref glyph_transforms,
        } if text_id == &text.id
            && path_id == &path.id
            && glyph_transforms[0].rows[0][3] == 5.0
    ));
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
    let parsed = parse_sketch_relation(&record, &HashSet::from([201])).unwrap();
    assert_eq!(parsed.member_roles, [1, 1, 0, 0]);
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
    let parsed = parse_sketch_relation(&record, &HashSet::from([201])).unwrap();
    assert_eq!(parsed.member_roles, [3, 1, 0, 0]);
    assert_eq!(parsed.auxiliary_references, [0, 464, 470, 467, 473]);
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

    let mut compact_auxiliary = Vec::new();
    compact_auxiliary.extend_from_slice(&3u32.to_le_bytes());
    push_reference(&mut compact_auxiliary, 464);
    compact_auxiliary.extend_from_slice(&[0u8; 6]);
    for value in [1.0f64, 0.0, 0.0, 3.0] {
        compact_auxiliary.extend_from_slice(&value.to_le_bytes());
    }
    push_reference(&mut compact_auxiliary, 470);
    compact_auxiliary.extend_from_slice(&[0u8; 6]);
    compact_auxiliary.extend_from_slice(&1u32.to_le_bytes());
    push_reference(&mut compact_auxiliary, 467);
    compact_auxiliary.extend_from_slice(&[0u8; 6]);
    for value in [0.0f64, 1.0, 0.0, 0.5] {
        compact_auxiliary.extend_from_slice(&value.to_le_bytes());
    }
    push_reference(&mut compact_auxiliary, 473);
    compact_auxiliary.extend_from_slice(&[0u8; 6]);
    let compact_record = genesis_relation_record(
        &[(352, 3), (353, 1), (442, 0), (445, 0)],
        2,
        &compact_auxiliary,
        201,
        0x2000_0000,
        &[353, 352, 442, 445],
    );
    let compact = parse_sketch_relation(&compact_record, &HashSet::from([201])).unwrap();
    assert_eq!(compact.auxiliary_references, [464, 470, 467, 473]);
    assert_eq!(
        decode_pattern_definition(&compact_record, &compact),
        decode_pattern_definition(&record, &parsed),
    );
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
        extrude_operation: None,
        extrude_operation_offset: None,
        extrude_extent: None,
        extrude_extent_offsets: None,
        extrude_direction_reversed: None,
        extrude_direction_reversed_offset: None,
        extrude_start: None,
        extrude_start_offset: None,
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
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        extrude_profile: None,
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
