// SPDX-License-Identifier: Apache-2.0
//! Decode-module projection and completeness unit tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use super::super::{
    apply_appearance_base_colors, bind_mesh_feature_definitions,
    container_only_dimension_parameters, design_projection_gaps, face_selection_is_resolved,
    feature_definition_is_incomplete, incomplete_feature_families, mesh_attribute_channels,
    mesh_texture_assignments, report_design_projection_gaps, unresolved_dimension_companion_count,
    DesignProjectionGaps, MeshProjection,
};
use crate::loss::F3dLossCode;
use crate::native::F3dNative;
use crate::records::{
    DesignBodyBinding, DesignDimensionLocusPair, DesignDimensionNullLocusPair,
    DesignDimensionRecipeRecord, DesignFeatureTimeline, DesignParameter, DesignParameterCompanion,
    DesignParameterOwner, DesignParameterScope, DesignSketchPlacement,
    LostEdgeReference, SketchCurveIdentity, SketchPoint, SketchRelation, SketchRelationKind,
};

#[test]
fn active_face_substitutions_have_a_distinct_loss_note() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    let mut native = F3dNative::default();
    native.design_face_operands.push(
        serde_json::from_value(serde_json::json!({
            "id": "f3d:test:face-operand#200",
            "scope_record_index": 100,
            "scope_reference_ordinal": 0,
            "record_index": 200,
            "byte_offset": 0,
            "class_tag": "346",
            "paired_byte_offset": 325,
            "paired_class_tag": "262",
            "recipe_record_index": 201,
            "recipe_record_byte_offset": 0,
            "recipe_id": "f3d:test:recipe#201",
            "recipe_prefix_offset": 0,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_kind": "bounded_face",
            "recipe_program_offset": 0,
            "recipe_program": [],
            "recipe_node_offsets": [],
            "recipe_nodes": [],
            "resolved_active_face": "f3d:brep:entity#30",
            "next_record_index": 202,
            "next_byte_offset": 100
        }))
        .expect("active face operand"),
    );
    let mut report = cadmpeg_ir::codec::DecodeBody {
        geometry_transferred: true,
        coverage: cadmpeg_ir::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };

    report_design_projection_gaps(&mut report, &ir, &native);

    let loss = report
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::FeatureFaceSelectionActiveSubstituted.kind())
        .expect("active-face substitution loss");
    assert_eq!(loss.message, "1 legacy face operand(s) use a current active-BREP face because no unique preceding-state face slot resolved.");
}

#[test]
fn mesh_feature_binds_tessellations_in_design_body_order() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

    let scope_id = "f3d:Design/BulkStream.dat:design-parameter-scope#10";
    let mut scope = DesignParameterScope::empty(
        scope_id,
        crate::records::DesignFeatureKind::BaseMeshFeature,
        10,
    );
    // The feature's owning entity reference is distinct from its scope index.
    scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![221]);
    let mut features = vec![Feature {
        id: FeatureId("feature:mesh-import".into()),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: Some("Base Mesh Feature".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "Base Mesh Feature".into(),
            parameters: std::collections::BTreeMap::new(),
        },
        native_ref: Some(scope_id.into()),
    }];
    let projection = MeshProjection {
        count: 2,
        tessellations_by_scope: std::collections::HashMap::from([(
            ("f3d:Design/BulkStream.dat".into(), 10),
            vec!["tessellation:z-body".into(), "tessellation:a-body".into()],
        )]),
    };

    bind_mesh_feature_definitions(&mut features, &[scope], &projection);

    assert_eq!(
        features[0].definition,
        FeatureDefinition::MeshImport {
            tessellations: vec!["tessellation:z-body".into(), "tessellation:a-body".into(),],
        }
    );
    assert!(!feature_definition_is_incomplete(&features[0].definition));
}

#[test]
fn mesh_texture_ids_resolve_through_design_table_order() {
    use cadmpeg_ir::assets::AssetId;
    use cadmpeg_ir::tessellation::TessellationTextureAssignment;

    let first = AssetId("asset:first".into());
    let second = AssetId("asset:second".into());
    let textures = [
        ("resource:first".into(), first.clone()),
        ("resource:second".into(), second.clone()),
        ("resource:third".into(), first.clone()),
    ];
    assert_eq!(
        mesh_texture_assignments(Some(&[0, 2, 1, 3, 2]), &textures, 5)
            .expect("texture assignments"),
        vec![
            TessellationTextureAssignment {
                source_id: Some("resource:first".into()),
                texture: first,
                triangles: vec![2],
            },
            TessellationTextureAssignment {
                source_id: Some("resource:second".into()),
                texture: second,
                triangles: vec![1, 4],
            },
            TessellationTextureAssignment {
                source_id: Some("resource:third".into()),
                texture: AssetId("asset:first".into()),
                triangles: vec![3],
            },
        ]
    );
    assert!(matches!(
        mesh_texture_assignments(
            Some(&[2]),
            &[("resource:only".into(), AssetId("asset:only".into()))],
            1,
        ),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn indexed_mesh_channels_project_default_and_override_selectors() {
    let attribute = crate::paramesh::MeshAttribute {
        role: 4,
        resource_guid: None,
        authored_name: None,
        groups: Vec::new(),
        element_code: 4,
        domain: crate::paramesh::MeshAttributeDomain::Corner,
        item_size: Some(1),
        values: vec![10, 11, 12, 13, 14],
        indices: Some(vec![0, 2]),
        triangle_values: None,
    };
    let mut unresolved = std::collections::BTreeMap::new();
    let channels = mesh_attribute_channels(&[attribute], 3, &[[0, 1, 2]], &mut unresolved);

    assert!(unresolved.is_empty());
    assert_eq!(channels.len(), 1);
    assert_eq!(
        channels[0].domain(),
        cadmpeg_ir::tessellation::TessellationChannelDomain::Corner
    );
    assert_eq!(channels[0].count(), 5);
    assert_eq!(channels[0].indices(), [3, 1, 4]);
    assert_eq!(channels[0].data(), [10, 11, 12, 13, 14]);
}

#[test]
fn presentation_timeline_objects_are_not_incomplete_modeling_features() {
    let native = |kind: &str| cadmpeg_ir::features::FeatureDefinition::Native {
        kind: kind.into(),
        parameters: std::collections::BTreeMap::new(),
    };

    assert!(!feature_definition_is_incomplete(&native("Canvas")));
    assert!(!feature_definition_is_incomplete(&native("Decal")));
    assert!(feature_definition_is_incomplete(&native("Fillet")));
}

#[test]
fn full_round_fillet_with_automatic_sides_is_complete() {
    use cadmpeg_ir::features::{
        FaceSelection, Feature, FeatureDefinition, FeatureId, FullRoundFilletGroup,
        FullRoundSideSelection,
    };

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId("feature:full-round".into()),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: Some("Fillet".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::FullRoundFillet {
            groups: vec![FullRoundFilletGroup {
                center_faces: FaceSelection::Resolved {
                    faces: vec!["face:center".into()],
                    native: "native:center-group".into(),
                },
                side_one_faces: FullRoundSideSelection::Automatic,
                side_two_faces: FullRoundSideSelection::Automatic,
            }],
        },
        native_ref: None,
    });

    assert!(!feature_definition_is_incomplete(
        &ir.model.features[0].definition
    ));
    assert_eq!(
        design_projection_gaps(&ir, &F3dNative::default()).incomplete_features,
        0
    );
}

#[test]
fn extrude_completeness_requires_resolved_profile_start_and_termination() {
    let extrude =
        |profile: serde_json::Value, start: serde_json::Value, termination: serde_json::Value| {
            serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(serde_json::json!({
                "definition": "extrude",
                "profile": profile,
                "start": start,
                "extent": {
                    "kind": "one_sided",
                    "side": {"termination": termination}
                },
                "solid": true,
                "op": "new_body"
            }))
            .expect("Extrude definition")
        };
    let sketch_profile = serde_json::json!({
        "kind": "sketch_profiles",
        "value": {"sketch": "sketch:1", "profiles": [0]}
    });
    let profile_start = serde_json::json!({"kind": "profile_plane"});
    let blind = serde_json::json!({"kind": "blind", "length": 10.0});

    assert!(!feature_definition_is_incomplete(&extrude(
        sketch_profile.clone(),
        profile_start.clone(),
        blind.clone(),
    )));
    assert!(feature_definition_is_incomplete(&extrude(
        serde_json::json!({"kind": "native", "value": "native:profile"}),
        profile_start.clone(),
        blind.clone(),
    )));
    assert!(feature_definition_is_incomplete(&extrude(
        sketch_profile.clone(),
        serde_json::json!({"kind": "unresolved"}),
        blind,
    )));
    assert!(feature_definition_is_incomplete(&extrude(
        sketch_profile,
        profile_start,
        serde_json::json!({"kind": "to_face", "face": {"kind": "unresolved"}}),
    )));
}

#[test]
fn hole_completeness_requires_support_placement_size_and_extent() {
    let complete: cadmpeg_ir::features::FeatureDefinition =
        serde_json::from_value(serde_json::json!({
            "definition": "hole",
            "face": {"kind": "faces", "value": ["face:support"]},
            "placements": [{
                "kind": "directed",
                "position": {"x": 1.0, "y": 2.0, "z": 3.0},
                "direction": {"x": 0.0, "y": 0.0, "z": -1.0}
            }],
            "kind": {"kind": "simple_drilled", "drill_point_angle": 2.0},
            "diameter": 5.0,
            "extent": {"kind": "blind", "length": 10.0}
        }))
        .expect("complete Hole definition");
    assert!(!feature_definition_is_incomplete(&complete));

    let mut missing_placement = complete.clone();
    let cadmpeg_ir::features::FeatureDefinition::Hole { placements, .. } = &mut missing_placement
    else {
        panic!("Hole definition");
    };
    *placements = None;
    assert!(feature_definition_is_incomplete(&missing_placement));

    let mut native_support = complete.clone();
    let cadmpeg_ir::features::FeatureDefinition::Hole { face, .. } = &mut native_support else {
        panic!("Hole definition");
    };
    *face = Some(cadmpeg_ir::features::FaceSelection::Native(
        "native:support".into(),
    ));
    assert!(feature_definition_is_incomplete(&native_support));

    let mut missing_extent = complete;
    let cadmpeg_ir::features::FeatureDefinition::Hole { extent, .. } = &mut missing_extent else {
        panic!("Hole definition");
    };
    *extent = None;
    assert!(feature_definition_is_incomplete(&missing_extent));
}

#[test]
fn face_selection_resolution_accepts_complete_generated_and_partial_members() {
    use cadmpeg_ir::features::{FaceSelection, FeatureId, GeneratedFaceRef};
    use cadmpeg_ir::ids::{FeatureInputTopologyId, HistoricalFaceId};

    assert!(face_selection_is_resolved(&FaceSelection::Generated {
        faces: vec![GeneratedFaceRef {
            feature: FeatureId("feature:source".into()),
            local_id: "face:1".into(),
        }],
        native: "native:generated-face".into(),
    }));
    assert!(face_selection_is_resolved(
        &FaceSelection::HistoricalPartial {
            state: FeatureInputTopologyId::mint("state:1").expect("identity grammar"),
            faces: vec![HistoricalFaceId::mint("face:1").expect("identity grammar")],
            unresolved: Vec::new(),
            native: "native:historical-face".into(),
        }
    ));
    assert!(!face_selection_is_resolved(
        &FaceSelection::HistoricalPartial {
            state: FeatureInputTopologyId::mint("state:1").expect("identity grammar"),
            faces: vec![HistoricalFaceId::mint("face:1").expect("identity grammar")],
            unresolved: vec!["native:missing-face".into()],
            native: "native:historical-face".into(),
        }
    ));
}

#[test]
fn filled_surface_completeness_requires_boundary_conditions_support_and_merge() {
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, PathRef, SurfaceBoundary, SurfaceContinuity,
    };
    use cadmpeg_ir::ids::{EdgeId, FaceId};

    let surface = |support_faces, continuity, merge_result| FeatureDefinition::FilledSurface {
        boundary: SurfaceBoundary::Path(PathRef::Edges(vec![
            EdgeId::mint("edge:1").expect("identity grammar")
        ])),
        support_faces,
        continuity: cadmpeg_ir::features::FilledSurfaceContinuityState::uniform(continuity),
        merge_result,
    };

    assert!(!feature_definition_is_incomplete(&surface(
        FaceSelection::Faces(Vec::new()),
        SurfaceContinuity::Contact,
        Some(false),
    )));
    assert!(feature_definition_is_incomplete(&surface(
        FaceSelection::Faces(Vec::new()),
        SurfaceContinuity::Contact,
        None,
    )));
    assert!(feature_definition_is_incomplete(&surface(
        FaceSelection::Faces(Vec::new()),
        SurfaceContinuity::Tangent,
        Some(true),
    )));
    assert!(!feature_definition_is_incomplete(&surface(
        FaceSelection::Faces(vec![FaceId::mint("face:support").expect("identity grammar")]),
        SurfaceContinuity::Curvature,
        Some(true),
    )));
}

#[test]
fn sheet_metal_completeness_requires_neutral_profiles_and_edges() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("sheet-metal definition")
    };
    let base_flange = |profile| {
        definition(serde_json::json!({
            "definition": "sheet_metal_base_flange",
            "profile": profile,
            "thickness": 2.5,
            "side": "forward"
        }))
    };
    let edge_flange = |edges| {
        definition(serde_json::json!({
            "definition": "sheet_metal_edge_flange",
            "edges": edges,
            "height": {"kind": "distance", "value": 25.0},
            "angle": std::f64::consts::FRAC_PI_2,
            "height_datum": "outer_faces",
            "bend_position": "inside",
            "width": {"kind": "full_edge"},
            "bend_radius": 2.5
        }))
    };
    let hem = |edges| {
        definition(serde_json::json!({
            "definition": "sheet_metal_hem",
            "edges": edges,
            "form": {"kind": "flat", "value": {"length": 10.0}},
            "direction": "forward",
            "bend_radius": 2.5
        }))
    };

    assert!(!feature_definition_is_incomplete(&base_flange(
        serde_json::json!({"kind": "sketch", "value": "sketch:1"}),
    )));
    assert!(feature_definition_is_incomplete(&base_flange(
        serde_json::json!({"kind": "native", "value": "native:profile"}),
    )));
    assert!(!feature_definition_is_incomplete(&edge_flange(
        serde_json::json!({"kind": "edges", "value": ["edge:1"]}),
    )));
    assert!(feature_definition_is_incomplete(&edge_flange(
        serde_json::json!({"kind": "native", "value": "native:edges"}),
    )));
    assert!(!feature_definition_is_incomplete(&hem(
        serde_json::json!({"kind": "edges", "value": ["edge:1"]}),
    )));
    assert!(feature_definition_is_incomplete(&hem(
        serde_json::json!({"kind": "native", "value": "native:edges"}),
    )));
}

#[test]
fn selected_face_and_edge_features_require_neutral_operands() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("selected feature definition")
    };
    let fillet = |edges| {
        definition(serde_json::json!({
            "definition": "fillet",
            "groups": [{
                "edges": edges,
                "radius": {"kind": "constant", "radius": 5.0},
                "tangency_weight": 1.0
            }]
        }))
    };

    assert!(!feature_definition_is_incomplete(&fillet(
        serde_json::json!({"kind": "edges", "value": ["edge:1"]}),
    )));
    assert!(feature_definition_is_incomplete(&fillet(
        serde_json::json!({"kind": "native", "value": "native:edges"}),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "delete_face",
            "faces": {"kind": "faces", "value": ["face:1"]},
            "heal": true
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "delete_face",
            "faces": {"kind": "native", "value": "native:faces"},
            "heal": true
        }),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "offset_surface",
            "faces": {"kind": "faces", "value": ["face:1"]},
            "distance": 2.0
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "offset_surface",
            "faces": {"kind": "faces", "value": ["face:1"]}
        }),
    )));
}

#[test]
fn form_and_primitive_completeness_requires_construction_payloads() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("construction definition")
    };

    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({"definition": "form", "cages": ["subd:1"]}),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({"definition": "form", "cages": []}),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "block",
            "dimensions": [30.0, 40.0, 20.0],
            "placement": [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0]
            ],
            "op": "join"
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "block",
            "dimensions": [30.0, 40.0, 20.0],
            "op": "join"
        }),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "primitive",
            "solid": {
                "kind": "cylinder",
                "radius": 15.0,
                "height": 7.0,
                "angle": std::f64::consts::TAU
            },
            "op": "join"
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "primitive",
            "solid": {
                "kind": "cylinder",
                "radius": 15.0,
                "height": 7.0,
                "angle": std::f64::consts::TAU
            },
            "op": "unresolved"
        }),
    )));
}

#[test]
fn profile_and_boolean_features_require_resolved_operation_inputs() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("profile or Boolean definition")
    };

    let sweep = definition(serde_json::json!({
        "definition": "sweep",
        "section": {
            "kind": "profile",
            "value": {"kind": "sketch", "value": "sketch:section"}
        },
        "path": {"kind": "edges", "value": ["edge:path"]},
        "mode": {"mode": "solid", "op": "join"}
    }));
    assert!(!feature_definition_is_incomplete(&sweep));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "sweep",
            "section": {
                "kind": "profile",
                "value": {"kind": "native", "value": "native:section"}
            },
            "path": {"kind": "edges", "value": ["edge:path"]},
            "mode": {"mode": "solid", "op": "join"}
        }),
    )));

    let chamfer = definition(serde_json::json!({
        "definition": "chamfer",
        "groups": [{
            "edges": {"kind": "edges", "value": ["edge:1"]},
            "spec": {"kind": "distance", "distance": 2.0}
        }]
    }));
    assert!(!feature_definition_is_incomplete(&chamfer));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "chamfer",
            "groups": [{
                "edges": {"kind": "native", "value": "native:edges"},
                "spec": {"kind": "distance", "distance": 2.0}
            }]
        }),
    )));

    let combine = definition(serde_json::json!({
        "definition": "combine",
        "target": {"kind": "bodies", "value": ["body:target"]},
        "tools": {"kind": "bodies", "value": ["body:tool"]},
        "op": "cut"
    }));
    assert!(!feature_definition_is_incomplete(&combine));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "combine",
            "target": {"kind": "native", "value": "native:target"},
            "tools": {"kind": "bodies", "value": ["body:tool"]},
            "op": "cut"
        }),
    )));

    let revolve = definition(serde_json::json!({
        "definition": "revolve",
        "construction": {
            "profile": {"kind": "sketch", "value": "sketch:profile"},
            "axis": {
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "direction": {"x": 0.0, "y": 0.0, "z": 1.0}
            },
            "extent": {
                "kind": "one_sided",
                "termination": {"kind": "angle", "angle": std::f64::consts::PI}
            }
        },
        "op": "new_body"
    }));
    assert!(!feature_definition_is_incomplete(&revolve));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "revolve",
            "construction": {
                "profile": {"kind": "native", "value": "native:profile"},
                "axis": {
                    "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                    "direction": {"x": 0.0, "y": 0.0, "z": 1.0}
                },
                "extent": {
                    "kind": "one_sided",
                    "termination": {"kind": "angle", "angle": std::f64::consts::PI}
                }
            },
            "op": "new_body"
        }),
    )));
}

#[test]
fn datum_point_completeness_requires_a_resolved_construction_rule() {
    let definition = |construction: Option<serde_json::Value>| {
        let mut value = serde_json::json!({
            "definition": "datum_point",
            "position": {"x": 1.0, "y": 2.0, "z": 3.0}
        });
        if let Some(construction) = construction {
            value
                .as_object_mut()
                .expect("DatumPoint object")
                .insert("construction".into(), construction);
        }
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("DatumPoint definition")
    };

    assert!(!feature_definition_is_incomplete(&definition(Some(
        serde_json::json!({
            "kind": "circle_center",
            "edge": {"kind": "edges", "value": ["edge:1"]}
        }),
    ))));
    assert!(feature_definition_is_incomplete(&definition(None)));
    assert!(feature_definition_is_incomplete(&definition(Some(
        serde_json::json!({
            "kind": "distance_on_edge",
            "edge": {"kind": "native", "value": "native:edge"},
            "fraction": 0.5
        }),
    ))));
}

#[test]
fn datum_plane_completeness_accepts_direct_frames_and_resolved_construction() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("datum-plane definition")
    };

    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "datum_plane",
            "origin": {"x": 0.0, "y": 0.0, "z": 5.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
        }),
    )));
    let three_point = |point: serde_json::Value| {
        serde_json::json!({
            "definition": "datum_three_point_plane",
            "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
            "points": [point.clone(), point.clone(), point]
        })
    };
    assert!(!feature_definition_is_incomplete(&definition(three_point(
        serde_json::json!({
            "kind": "historical",
            "value": {
                "state": "state:1",
                "vertex": "vertex:1",
                "native": "native:1"
            }
        }),
    ))));
    assert!(feature_definition_is_incomplete(&definition(three_point(
        serde_json::json!({"kind": "native", "value": "native:1"}),
    ))));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "datum_principal_plane",
            "plane": "top"
        }),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "datum_offset_plane",
            "reference": "feature:plane",
            "distance": 5.0
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "datum_offset_plane",
            "distance": 5.0
        }),
    )));
}

#[test]
fn coil_completeness_requires_neutral_placement_and_boolean_targets() {
    use cadmpeg_ir::features::{
        Angle, BodySelection, CoilConstruction, CoilExtent, CoilPlacement, CoilResult, CoilSection,
        CoilSectionPlacement, FeatureDefinition, Length,
    };
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::math::{Point3, Vector3};

    let construction = CoilConstruction {
        placement: CoilPlacement::Explicit {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            radial: Vector3::new(1.0, 0.0, 0.0),
        },
        diameter: Length(10.0),
        extent: CoilExtent::RevolutionsHeight {
            revolutions: 2.0,
            height: Length(5.0),
        },
        section: CoilSection::Circular {
            diameter: Length(1.0),
        },
        section_placement: CoilSectionPlacement::Center,
        clockwise: false,
        taper: Angle(0.0),
    };
    let definition = |construction, result| FeatureDefinition::Coil {
        construction,
        result,
    };

    assert!(!feature_definition_is_incomplete(&definition(
        construction.clone(),
        CoilResult::NewBody,
    )));

    let mut native_placement = construction.clone();
    native_placement.placement = CoilPlacement::Native {
        native_ref: "native:placement".into(),
    };
    assert!(feature_definition_is_incomplete(&definition(
        native_placement,
        CoilResult::NewBody,
    )));

    let native_target = definition(
        construction.clone(),
        CoilResult::Boolean {
            operation: cadmpeg_ir::features::BooleanKind::Join,
            targets: BodySelection::Native("native:target".into()),
        },
    );
    assert!(feature_definition_is_incomplete(&native_target));

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(cadmpeg_ir::features::Feature {
        id: cadmpeg_ir::features::FeatureId("feature:coil".into()),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("CoilPrimitive".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: native_target,
        native_ref: None,
    });
    let gaps = design_projection_gaps(&ir, &F3dNative::default());
    assert_eq!(gaps.incomplete_features, 1);
    assert_eq!(gaps.body_selections, 1);

    assert!(!feature_definition_is_incomplete(&definition(
        construction,
        CoilResult::Boolean {
            operation: cadmpeg_ir::features::BooleanKind::Cut,
            targets: BodySelection::Bodies(vec![BodyId::mint("body:1").expect("identity grammar")]),
        },
    )));
}

#[test]
fn draft_completeness_requires_material_side() {
    let complete: cadmpeg_ir::features::FeatureDefinition =
        serde_json::from_value(serde_json::json!({
            "definition": "draft",
            "faces": {"kind": "faces", "value": ["face:drafted"]},
            "neutral_plane": {"kind": "faces", "value": ["face:neutral"]},
            "pull_direction": null,
            "angle": 0.1,
            "outward": true
        }))
        .expect("complete neutral-plane Draft");
    assert!(!feature_definition_is_incomplete(&complete));

    let mut incomplete = complete;
    let cadmpeg_ir::features::FeatureDefinition::Draft { outward, .. } = &mut incomplete else {
        panic!("Draft definition");
    };
    *outward = None;
    assert!(feature_definition_is_incomplete(&incomplete));
}

#[test]
fn loft_completeness_and_gap_counts_require_resolved_sections_and_paths() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

    let resolved: FeatureDefinition = serde_json::from_value(serde_json::json!({
        "definition": "loft",
        "sections": [
            {
                "kind": "spatial_sketch_profiles",
                "value": {"sketch": "spatial-sketch", "profiles": [2, 3]}
            },
            {
                "kind": "spatial_sketch_profiles",
                "value": {"sketch": "spatial-sketch", "profiles": [1, 4]}
            }
        ],
        "guides": [{
            "kind": "spatial_sketch_curves",
            "value": {"sketch": "spatial-sketch", "curves": ["curve"]}
        }],
        "op": "join"
    }))
    .expect("resolved Loft definition");
    assert!(!feature_definition_is_incomplete(&resolved));

    let unresolved: FeatureDefinition = serde_json::from_value(serde_json::json!({
        "definition": "loft",
        "sections": [
            {"kind": "native", "value": "native:profile"},
            {
                "kind": "spatial_sketch_profiles",
                "value": {"sketch": "spatial-sketch", "profiles": [1, 4]}
            }
        ],
        "guides": [{"kind": "native", "value": "native:guide"}],
        "op": "join"
    }))
    .expect("unresolved Loft definition");
    assert!(feature_definition_is_incomplete(&unresolved));

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId("feature:loft".into()),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: Some("Loft".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: unresolved,
        native_ref: None,
    });

    let gaps = design_projection_gaps(&ir, &F3dNative::default());
    assert_eq!(gaps.incomplete_features, 1);
    assert_eq!(gaps.profile_selections, 1);
    assert_eq!(gaps.path_selections, 1);
}

#[test]
fn incomplete_feature_families_are_counted_by_source_operation() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let feature = |id: &str, source_tag: Option<&str>, kind: &str| Feature {
        id: FeatureId(id.into()),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: source_tag.map(str::to_owned),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: kind.into(),
            parameters: std::collections::BTreeMap::new(),
        },
        native_ref: None,
    };
    ir.model
        .features
        .push(feature("feature:1", Some("EdgeFlange"), "native-a"));
    ir.model
        .features
        .push(feature("feature:2", Some("EdgeFlange"), "native-b"));
    ir.model.features.push(feature("feature:3", None, "Hem"));
    ir.model
        .features
        .push(feature("feature:4", Some("Canvas"), "Canvas"));

    assert_eq!(
        incomplete_feature_families(&ir),
        std::collections::BTreeMap::from([("EdgeFlange", 2), ("Hem", 1)])
    );
}

#[test]
fn body_copy_features_require_resolved_body_selection() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};
    use cadmpeg_ir::ids::BodyId;

    let resolved = BodySelection::Resolved {
        bodies: vec![BodyId::mint("body:result").expect("identity grammar")],
        native: "native:body-selection".into(),
    };
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::BaseFeature {
            bodies: resolved.clone(),
        }
    ));
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::InsertBodies { bodies: resolved }
    ));

    let unresolved = BodySelection::Native("native:body-selection".into());
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::BaseFeature { bodies: unresolved }
    ));
}

#[test]
fn split_body_requires_resolved_target_and_tool_selections() {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition};
    use cadmpeg_ir::ids::{BodyId, FaceId};

    let resolved_target = BodySelection::Resolved {
        bodies: vec![BodyId::mint("body:target").expect("identity grammar")],
        native: "native:target".into(),
    };
    let resolved_tool = FaceSelection::Resolved {
        faces: vec![FaceId::mint("face:tool").expect("identity grammar")],
        native: "native:tool".into(),
    };
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::SplitBody {
            targets: resolved_target.clone(),
            tools: resolved_tool,
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::SplitBody {
            targets: resolved_target.clone(),
            tools: FaceSelection::Native("native:tool".into()),
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::SplitBody {
            targets: BodySelection::Native("native:target".into()),
            tools: FaceSelection::Resolved {
                faces: vec![FaceId::mint("face:tool").expect("identity grammar")],
                native: "native:tool".into(),
            },
        }
    ));
}

#[test]
fn design_projection_gaps_count_unresolved_body_map_pairs() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    let mut native = F3dNative::default();
    native.design_body_bindings.push(DesignBodyBinding {
        id: "f3d:design:body-binding#0".into(),
        stream: "Design/BulkStream.dat".into(),
        pair_count: 1,
        pair_ordinal: 0,
        asm_body_key: 0,
        asm_body_key_offset: 0,
        entity_suffix: 1,
        entity_suffix_offset: 8,
        blob_name: "BREP.snapshot.smb".into(),
        blob_name_offset: 16,
        body: None,
    });

    assert_eq!(
        design_projection_gaps(&ir, &native).unresolved_body_bindings,
        1
    );
}

#[test]
fn design_projection_gaps_count_cosmetic_thread_faces() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    for (ordinal, face) in [
        serde_json::json!({"kind": "native", "value": "native:thread-face"}),
        serde_json::json!({"kind": "unresolved"}),
        serde_json::json!({
            "kind": "historical",
            "value": {
                "state": "feature-input",
                "faces": ["historical:face"],
                "native": "native:thread-group"
            }
        }),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(
            serde_json::from_value(serde_json::json!({
                "id": format!("thread-{ordinal}"),
                "ordinal": ordinal,
                "definition": {
                    "definition": "cosmetic_thread",
                    "face": face,
                    "diameter": 2.5
                }
            }))
            .expect("CosmeticThread feature"),
        );
    }

    assert_eq!(
        design_projection_gaps(&ir, &F3dNative::default()).face_selections,
        2
    );
}

#[test]
fn design_projection_gaps_count_each_retained_selection_family() {
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchGeometry, SketchId,
    };

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId("constraint".into()),
        sketch: SketchId("sketch".into()),
        definition: SketchConstraintDefinition::Native {
            native_kind: "dimension".into(),
            native_state: None,
            native_flags: None,
            native_properties: std::collections::BTreeMap::new(),
            entities: Vec::new(),
            parameter: None,
            operands: Vec::new(),
        },
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: Some("native:sketch-relation".into()),
    });
    let mut native_dimension = ir.model.sketch_constraints[0].clone();
    native_dimension.id = SketchConstraintId("dimension".into());
    native_dimension.native_ref = Some("native:dimension-companion".into());
    ir.model.sketch_constraints.push(native_dimension);
    ir.model.features.push(
        serde_json::from_value(serde_json::json!({
            "id": "extrude",
            "ordinal": 0,
            "definition": {
                "definition": "extrude",
                "profile": {
                    "kind": "sketch_selection",
                    "value": {"sketch": "sketch", "selections": ["native:profile"]}
                },
                "start": {"kind": "profile_plane"},
                "extent": {
                    "kind": "one_sided",
                    "side": {
                        "termination": {
                            "kind": "to_face",
                            "face": {"kind": "native", "value": "native:face"}
                        }
                    }
                },
                "op": "cut"
            }
        }))
        .expect("Extrude feature"),
    );
    ir.model.features.push(
        serde_json::from_value(serde_json::json!({
            "id": "sweep",
            "ordinal": 1,
            "definition": {
                "definition": "sweep",
                "section": {
                    "kind": "profile",
                    "value": {"kind": "native", "value": "native:sweep-profile"}
                },
                "path": {"kind": "native", "value": "native:sweep-path"},
                "mode": {"mode": "solid", "op": "cut"}
            }
        }))
        .expect("Sweep feature"),
    );
    ir.model.features.push(
        serde_json::from_value(serde_json::json!({
            "id": "fillet",
            "ordinal": 2,
            "definition": {
                "definition": "fillet",
                "groups": [
                    {
                        "edges": {"kind": "native", "value": "native:edges"},
                        "radius": {"kind": "constant", "radius": 1.0}
                    },
                    {
                        "edges": {"kind": "unresolved"},
                        "radius": {"kind": "constant", "radius": 2.0}
                    },
                    {
                        "edges": {
                            "kind": "historical_partial",
                            "value": {
                                "state": "history-input",
                                "edges": [],
                                "unresolved": [
                                    "native:edge-operand#1",
                                    "f3d:test:lost-edge-reference#2"
                                ],
                                "native": "native:partial-edges"
                            }
                        },
                        "radius": {"kind": "constant", "radius": 3.0}
                    }
                ]
            }
        }))
        .expect("Fillet feature"),
    );
    ir.model.features.push(
        serde_json::from_value(serde_json::json!({
            "id": "suppressed-fillet",
            "ordinal": 2,
            "suppressed": true,
            "definition": {
                "definition": "fillet",
                "groups": [{
                    "edges": {"kind": "native", "value": "native:suppressed-edges"},
                    "radius": {"kind": "constant", "radius": 3.0}
                }]
            }
        }))
        .expect("suppressed Fillet feature"),
    );
    ir.model.features.push(
        serde_json::from_value(serde_json::json!({
            "id": "native-feature",
            "ordinal": 3,
            "definition": {
                "definition": "native",
                "kind": "unsupported",
                "parameters": {},
                "properties": {}
            }
        }))
        .expect("native feature"),
    );
    ir.model.features.push(
        serde_json::from_value(serde_json::json!({
            "id": "unresolved-pattern",
            "ordinal": 4,
            "definition": {
                "definition": "pattern",
                "seeds": [],
                "pattern": {
                    "kind": "unresolved",
                    "form": "circular"
                }
            }
        }))
        .expect("unresolved pattern feature"),
    );

    let mut native = F3dNative::default();
    native.design_sketch_placements.push(DesignSketchPlacement {
        frame: crate::records::DesignSketchFrame::new(0, crate::records::DesignSketchFrameForm::ScopeCompact).unwrap(),

        id: "native:sketch-placement".into(),
        scope_record_index: Some(10),
        entity_id: crate::records::DesignEntityId::try_from("Sketch_1".to_owned()).expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::DesignClassTag::try_from("000".to_owned()).unwrap(),
        record_index: 10,

        paired_class_tag: crate::records::DesignClassTag::try_from("001".to_owned()).unwrap(),

    });
    native.sketch_points.push(SketchPoint {
        id: "native:sketch-point".into(),
        record_index: 11,
        owner_reference: Some(1),
        class_tag: "000".into(),
        byte_offset: 0,
        coordinate_offset: 0,
        entity_genesis: None,
        record_form: crate::records::SketchPointRecordForm::version11(
            1,
            crate::records::SketchPointClosure::Selector0State0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(0.0, 0.0),
        depth: 0.0,
        companion: None,
    });
    native.sketch_curve_identities.push(SketchCurveIdentity {
        id: "native:sketch-curve".into(),
        record_index: 12,
        owner_reference: Some(1),
        class_tag: "000".into(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: 1,
        secondary_id: 2,
        geometry: None,
    });
    native.lost_edge_references.push(LostEdgeReference::new("f3d:test:lost-edge-reference#2".into(), 0, "000".into(), 0, "001".into(), 1).expect("valid lost-edge record layout"));
    native.sketch_relations.push(SketchRelation {
        id: "native:sketch-relation".into(),
        record_index: 1,
        class_tag: "000".into(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 1,
        owner_entity_id: "0_1".into(),
        auxiliary_references: crate::records::ReferenceRun::Unlocated(Vec::new()),
        rectangular_counted_reference_count: None,
        members: Vec::new(),
        owner_reference_offset: 0,
        state: 0,
        entity_genesis: None,
        kind: SketchRelationKind::Unpatterned,
        return_members: Vec::new(),
        raw_bytes: Vec::new(),
    });
    native.design_parameters.push(DesignParameter {
        id: "f3d:test:design-parameter#2".into(),
        byte_offset: 0,
        class_tag: "000".into(),
        record_index: 2,
        source_ordinal: 2,
        source: crate::records::DesignParameterSource::new("Linear Dimension-2".into(), Some(3), Some(crate::records::Located { value: crate::records::DesignParameterDiscriminator::Code0, offset: 0 })).unwrap(),
        expression: "1 mm".into(),
        expression_offset: 0,
        source_kind_offset: 0,

        unit: Some(crate::records::RecordedValue { value: "native-unit".into(), offset: Some(0) }),
        name: "d2".into(),
        name_offset: 0,
        evaluated_value: 0.1,
        evaluated_value_offset: 0,
    });
    native.design_parameter_scopes.push(DesignParameterScope {
        id: "native:unprojected-scope".into(),
        byte_offset: 0,
        class_tag: "000".into(),
        record_index: 3,
        frame_length: 1,
        kind_offset: 0,
        feature_ordinal: std::num::NonZeroU32::MIN,
        feature_ordinal_offset: 0,
        history_state_id: None,
        previous_history_state_id: None,
        previous_history_state_id_offset: None,
        reference_count_offset: 0,
        reference_members: crate::records::ReferenceRun::from_columns(Vec::new(), Vec::new(), "reference_members").unwrap(),
        payload: crate::records::DesignFeatureKind::try_from("Unsupported".to_owned()).expect("native family name").into(),
        unclosed_construction_operand_groups: Vec::new(),
        paired_class_tag: "001".into(),
        paired_byte_offset: 1,
    });
    assert_eq!(
        design_projection_gaps(&ir, &native),
        DesignProjectionGaps {
            unresolved_body_bindings: 0,
            incomplete_features: 6,
            native_reference_images: 0,
            native_decals: 0,
            unprojected_feature_scopes: 1,
            unprojected_parameters: 1,
            unresolved_parameter_owners: 1,
            untyped_parameter_units: 1,
            unresolved_expression_dependencies: 0,
            unprojected_history_dependencies: 0,
            ambiguous_history_dependencies: 0,
            native_sketch_relations: 1,
            native_dimensions: 1,
            unprojected_sketch_placements: 1,
            unprojected_sketch_points: 1,
            unprojected_sketch_curves: 1,
            unprojected_sketch_surfaces: 0,
            unprojected_sketch_texts: 0,
            unprojected_sketch_relations: 0,
            unprojected_dimensions: 0,
            profile_selections: 2,
            path_selections: 1,
            face_selections: 1,
            active_face_substitutions: 0,
            body_selections: 0,
            partially_resolved_face_members: 0,
            native_edge_selections: 2,
            partially_resolved_edge_members: 1,
            unresolved_edge_selections: 1,
            unrepaired_lost_edge_references: 1,
        }
    );

    native.design_construction_operand_groups.push(
        serde_json::from_value(serde_json::json!({
            "id": "native:partial-edges",
            "scope_record_index": 1,
            "scope_reference_ordinal": 0,
            "record_index": 2,
            "byte_offset": 0,
            "class_tag": "300",
            "members": [3],
            "lost_edge_references": ["f3d:test:lost-edge-reference#2"],
            "member_offsets": [0],
            "frame": {
                "member_count_offset": 0,
                "opaque_index": 1,
                "opaque_index_offset": 0,
                "opaque_scalar": 1.0,
                "opaque_scalar_offset": 0,
                "variant": false
            },
            "role": 0x10_0000_0000u64,
            "role_offset": 0,
            "paired_class_tag": "258",
            "paired_byte_offset": 0
        }))
        .expect("lost-reference construction group"),
    );
    let cadmpeg_ir::features::FeatureDefinition::Fillet { groups } =
        &mut ir.model.features[2].definition
    else {
        unreachable!();
    };
    groups[2].edges = cadmpeg_ir::features::EdgeSelection::Historical {
        state: cadmpeg_ir::ids::FeatureInputTopologyId::mint("history-input")
            .expect("identity grammar"),
        edges: vec![
            cadmpeg_ir::ids::HistoricalEdgeId::mint("history-edge").expect("identity grammar")
        ],
        native: "native:partial-edges".into(),
    };
    assert_eq!(
        design_projection_gaps(&ir, &native).unrepaired_lost_edge_references,
        0
    );

    native.sketch_points[0].owner_reference = None;
    native.sketch_curve_identities[0].owner_reference = None;
    let ownerless = design_projection_gaps(&ir, &native);
    assert_eq!(ownerless.unprojected_sketch_points, 0);
    assert_eq!(ownerless.unprojected_sketch_curves, 0);
    native.sketch_points[0].owner_reference = Some(1);
    native.sketch_curve_identities[0].owner_reference = Some(1);

    ir.model.sketches.push(Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: Some("native:sketch-placement".into()),
    });
    for (id, native_ref) in [
        ("point", "native:sketch-point"),
        ("curve", "native:sketch-curve"),
    ] {
        ir.model.sketch_entities.push(
            SketchEntity::new(
                SketchEntityId(id.into()),
                SketchId("sketch".into()),
                SketchGeometry::Point {
                    position: Point2::new(0.0, 0.0),
                },
            )
            .with_native_ref(Some(native_ref.into())),
        );
    }
    let gaps = design_projection_gaps(&ir, &native);
    assert_eq!(gaps.unprojected_sketch_placements, 0);
    assert_eq!(gaps.unprojected_sketch_points, 0);
    assert_eq!(gaps.unprojected_sketch_curves, 0);

    ir.model.parameters.push(
        serde_json::from_value(serde_json::json!({
            "id": "parameter-2",
            "ordinal": 2,
            "name": "d2",
            "expression": "1 mm",
            "native_ref": "f3d:test:design-parameter#2"
        }))
        .expect("Design parameter"),
    );
    assert_eq!(
        design_projection_gaps(&ir, &native).unprojected_parameters,
        0
    );
}

#[test]
fn design_projection_gaps_require_unique_scope_state_dependencies() {
    let scope = |record_index, current, previous| DesignParameterScope {
        id: format!("f3d:native:scope#{record_index}"),
        byte_offset: u64::from(record_index),
        class_tag: "000".into(),
        record_index,
        frame_length: 1,
        kind_offset: 0,
        feature_ordinal: std::num::NonZeroU32::new(record_index).expect("nonzero ordinal"),
        feature_ordinal_offset: 0,
        history_state_id: current,
        previous_history_state_id: previous,
        previous_history_state_id_offset: None,
        reference_count_offset: 0,
        reference_members: crate::records::ReferenceRun::from_columns(Vec::new(), Vec::new(), "reference_members").unwrap(),
        payload: crate::records::DesignFeatureKind::try_from("Unsupported".to_owned()).expect("native family name").into(),
        unclosed_construction_operand_groups: Vec::new(),
        paired_class_tag: "001".into(),
        paired_byte_offset: u64::from(record_index) + 1,
    };
    let mut native = F3dNative::default();
    native.design_parameter_scopes = vec![
        scope(1, Some(10), None),
        scope(2, Some(11), Some(10)),
        scope(3, Some(20), None),
        scope(4, Some(20), None),
        scope(5, Some(21), Some(20)),
    ];
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features = native
        .design_parameter_scopes
        .iter()
        .map(|scope| {
            serde_json::from_value(serde_json::json!({
                "id": format!("feature-{}", scope.record_index),
                "ordinal": scope.record_index,
                "definition": {
                    "definition": "native",
                    "kind": "Unsupported",
                    "parameters": {},
                    "properties": {}
                },
                "native_ref": scope.id
            }))
            .expect("native feature")
        })
        .collect();

    let gaps = design_projection_gaps(&ir, &native);
    assert_eq!(gaps.unprojected_feature_scopes, 0);
    assert_eq!(gaps.unprojected_history_dependencies, 1);
    assert_eq!(gaps.ambiguous_history_dependencies, 1);

    let predecessor = ir.model.features[0].id.clone();
    ir.model.features[1].dependencies.push(predecessor);
    let gaps = design_projection_gaps(&ir, &native);
    assert_eq!(gaps.unprojected_history_dependencies, 0);
    assert_eq!(gaps.ambiguous_history_dependencies, 1);
}

#[test]
fn design_projection_gaps_accept_a_dependency_collapsed_through_an_internal_scope() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut predecessor = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        crate::records::DesignFeatureKind::Extrude,
        100,
    );
    predecessor.history_state_id = Some(7);
    let mut internal = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#150"),
        crate::records::DesignFeatureKind::BaseFeature,
        150,
    );
    internal.history_state_id = Some(8);
    internal.previous_history_state_id = Some(7);
    let mut successor = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#200"),
        crate::records::DesignFeatureKind::Fillet,
        200,
    );
    successor.history_state_id = Some(9);
    successor.previous_history_state_id = Some(8);
    let scopes = vec![successor, internal, predecessor];
    let timeline = DesignFeatureTimeline {
        id: crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        byte_offset: 0,
        class_tag: "256".into(),
        record_index: 1,
        source_ordinal: 0,
        frame_length: 0,
        context_record_index: 2,
        context_record_index_offset: 0,
        item_count_offset: 0,
        items: vec![crate::records::Located { value: 100, offset: 0 }, crate::records::Located { value: 200, offset: 0 }],
    };
    let (features, _) =
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            &crate::design::feature_project::ProjectInputs {
                native: &[],
                owners: &[],
                scopes: &scopes,
                timelines: std::slice::from_ref(&timeline),
                construction_groups: &[],
                fillet_radius_groups: &[],
                edge_operands: &[],
                edge_identity_operands: &[],
                edge_treatment_vertex_operands: &[],
                entity_selection_operands: &[],
                curve_identities: &[],
                face_operands: &[],
                body_recipe_operands: &[],
                legacy_loft_body_carriers: &[],
                placements: &[],
                body_bindings: &[],
                component_naming_spaces: &[],
                histories: &[],
            },
        )
        .expect("timeline projection through one internal scope");
    let mut native = F3dNative::default();
    native.design_parameter_scopes = scopes;
    native.design_feature_timelines = vec![timeline];
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features = features;

    let gaps = design_projection_gaps(&ir, &native);
    assert_eq!(gaps.unprojected_feature_scopes, 0);
    assert_eq!(gaps.unprojected_history_dependencies, 0);
    assert_eq!(gaps.ambiguous_history_dependencies, 0);
}

#[test]
fn payload_bearing_dimension_companion_uses_the_governing_dimension_frame() {
    let stream = "f3d:test/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let mut native = F3dNative::default();
    native.design_parameters.push(DesignParameter {
        id: format!("{stream}:design-parameter#10"),
        byte_offset: 0,
        class_tag: "305".into(),
        record_index: 10,
        source_ordinal: 0,
        source: crate::records::DesignParameterSource::new("Linear Dimension-2".into(), Some(20), Some(crate::records::Located { value: crate::records::DesignParameterDiscriminator::Code0, offset: 22 })).unwrap(),
        expression: "5 mm".into(),
        expression_offset: 40,
        source_kind_offset: 60,

        unit: Some(crate::records::RecordedValue { value: "mm".into(), offset: Some(90) }),
        name: "d1".into(),
        name_offset: 100,
        evaluated_value: 0.5,
        evaluated_value_offset: 110,
    });
    native.design_parameter_owners.push(DesignParameterOwner {
        id: format!("{stream}:design-parameter-owner#20"),
        byte_offset: 120,
        frame_length: 104,
        class_tag: "292".into(),
        record_index: 20,
        scope_record_index: 1,
        local_ordinal: 0,
        evaluated_value: 0.5,
        evaluated_value_offset: 160,
        parameter_record_index: 10,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 30,
    });
    native
        .design_parameter_companions
        .push(DesignParameterCompanion {
            id: format!("{stream}:design-parameter-companion#30"),
            byte_offset: 220,
            class_tag: "408".into(),
            record_index: 30,
            owner_record_index: 20,
            timestamp_micros: std::num::NonZeroU64::new(1).unwrap(),
            timestamp_micros_offset: 262,
            payload_byte_offset: 278,
            payload_byte_length: 100,
            owned_recipe_ids: Vec::new(),
        });
    assert_eq!(unresolved_dimension_companion_count(&native, &ir), 1);
    ir.model.sketch_constraints.push(
        serde_json::from_value(serde_json::json!({
            "id": "f3d:model:sketch-constraint#dimension",
            "sketch": "f3d:model:sketch#1",
            "definition": {
                "kind": "distance",
                "entities": [],
                "parameter": "f3d:model:parameter#1"
            },
            "native_ref": format!("{stream}:design-parameter-companion#30")
        }))
        .expect("neutral dimension constraint"),
    );
    assert_eq!(unresolved_dimension_companion_count(&native, &ir), 0);
    ir.model.sketch_constraints.clear();

    let mut recipe_backed = native.clone();
    recipe_backed
        .design_dimension_recipe_records
        .push(DesignDimensionRecipeRecord {
            id: format!("{stream}:design-dimension-recipe-record#31"),
            companion_record_index: 30,
            recipe_ordinal: 0,
            recipe_id: format!("{stream}:construction-recipe#31"),
            recipe_kind: crate::records::ConstructionRecipeKind::Edge,
            byte_offset: 278,
            class_tag: "423".into(),
            record_index: 31,
            frame_length: 100,
            prefix_offset: 300,
            prefix_bytes: Vec::new(),
            references: Vec::new(),
            program_offset: 320,
            program: vec![-1],
            matching_edge_operand_ids: Vec::new(),
        });
    assert_eq!(unresolved_dimension_companion_count(&recipe_backed, &ir), 0);

    native
        .design_dimension_locus_pairs
        .push(DesignDimensionLocusPair {
            id: format!("{stream}:design-dimension-locus-pair#278"),
            companion_record_index: 99,
            governing_companion_record_index: 30,
            byte_offset: 278,
            class_tag: "423".into(),
            record_index: 31,
            frame_length: 100,
            opaque_index: 0,
            opaque_index_offset: 300,
            first_geometry_record_index: 40,
            first_geometry_reference_offset: 305,
            first_role: 1,
            first_role_offset: 315,
            second_geometry_record_index: 41,
            second_geometry_reference_offset: 320,
            second_role: 2,
            second_role_offset: 330,
            paired_class_tag: "259".into(),
            paired_byte_offset: 378,
        });
    assert_eq!(unresolved_dimension_companion_count(&native, &ir), 0);
    assert!(container_only_dimension_parameters(&native).is_empty());
    native.design_dimension_locus_pairs[0].companion_record_index = 30;
    native.design_dimension_locus_pairs[0].governing_companion_record_index = 99;
    assert_eq!(unresolved_dimension_companion_count(&native, &ir), 0);
    assert_eq!(container_only_dimension_parameters(&native).len(), 1);

    native.design_dimension_locus_pairs.clear();
    native
        .design_dimension_null_locus_pairs
        .push(DesignDimensionNullLocusPair {
            id: format!("{stream}:design-dimension-null-locus-pair#278"),
            companion_record_index: 99,
            governing_companion_record_index: 30,
            byte_offset: 278,
            class_tag: "423".into(),
            record_index: 31,
            frame_length: 100,
            null_reference_offset: 300,
            null_role: 14,
            null_role_offset: 305,
            geometry_record_index: 40,
            geometry_reference_offset: 310,
            geometry_role: 3,
            geometry_role_offset: 320,
            paired_class_tag: "259".into(),
            paired_byte_offset: 378,
        });
    assert_eq!(unresolved_dimension_companion_count(&native, &ir), 0);
    native.design_dimension_null_locus_pairs[0].companion_record_index = 30;
    native.design_dimension_null_locus_pairs[0].governing_companion_record_index = 99;
    assert_eq!(unresolved_dimension_companion_count(&native, &ir), 0);
}

#[test]
fn appearance_base_colors_fill_only_uncolored_unambiguous_targets() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;
    use cadmpeg_ir::topology::Color;

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let body = ir.model.bodies[0].id.clone();
    let first_face = ir.model.faces[0].id.clone();
    let second_face = ir.model.faces[1].id.clone();
    let direct = Color {
        r: 0.9,
        g: 0.8,
        b: 0.7,
        a: 1.0,
    };
    let material = Color {
        r: 0.1,
        g: 0.2,
        b: 0.3,
        a: 1.0,
    };
    ir.model.bodies[0].color = Some(direct);
    ir.model.appearances.push(Appearance {
        id: AppearanceId::mint("f3d:appearance#material").expect("identity grammar"),
        name: None,
        asset_guid: None,
        library_id: None,
        textures: Vec::new(),
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(material),
        properties: Default::default(),
    });
    let binding = |id: &str, target| AppearanceBinding {
        id: id.into(),
        target,
        appearance: AppearanceId::mint("f3d:appearance#material").expect("identity grammar"),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: Default::default(),
    };
    ir.model.appearance_bindings = vec![
        binding("body", AppearanceTarget::Body(body)),
        binding("face", AppearanceTarget::Face(first_face)),
        binding("ambiguous-a", AppearanceTarget::Face(second_face.clone())),
        binding("ambiguous-b", AppearanceTarget::Face(second_face)),
    ];

    apply_appearance_base_colors(&mut ir);
    assert_eq!(ir.model.bodies[0].color, Some(direct));
    assert_eq!(ir.model.faces[0].color, Some(material));
    assert_eq!(ir.model.faces[1].color, None);
}
