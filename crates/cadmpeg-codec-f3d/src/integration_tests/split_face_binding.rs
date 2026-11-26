// SPDX-License-Identifier: Apache-2.0

use crate::design::feature_project::{
    project_parameter_design_with_edge_identities, ProjectInputs,
};
use crate::history::face_selections::{bind_feature_face_selections, FeatureFaceSelectionInputs};
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::features::direct_edit::SplitFaceTool;
use cadmpeg_ir::features::evaluation::FeatureInputTopology;
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

fn group(record: u32, member: u32, ordinal: u32, role: u64) -> DesignConstructionOperandGroup {
    json::from_value(serde_json::json!({
        "id": format!("f3d:Design/BulkStream.dat:group#{record}"),
        "scope_record_index": 42, "scope_reference_ordinal": ordinal,
        "record_index": record, "byte_offset": 1000, "class_tag": "256",
        "members": [member], "member_offsets": [1026],
        "frame": {"member_count_offset": 1021, "opaque_index": 1,
            "opaque_index_offset": 1072, "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076, "variant": false},
        "role": role, "role_offset": 1054,
        "paired_class_tag": "257", "paired_byte_offset": 1125
    }))
    .unwrap()
}

fn state(state_id: i64, next_ref: Option<i64>) -> crate::test_support::history::State {
    (
        AsmDeltaState {
            id: format!("f3d:history:state#{state_id}"),
            parent: "f3d:history".into(),
            byte_offset: 0,
            state_id,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref,
            node_index: state_id,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: None,
        },
        Some(AsmHistoricalTopology {
            faces: vec![7, 8],
            ..Default::default()
        }),
        Vec::new(),
    )
}

#[test]
fn split_face_binding_requires_target_membership_through_final_admission() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#42",
        DesignFeatureKind::SplitFace,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 256;
            draft.paired_byte_offset = 256;
            draft.history_state_id = Some(8);
            draft.previous_history_state_id = Some(7);
            draft.reference_members = ReferenceRun::unlocated(vec![100, 110, 200, 210]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let tool = group(100, 110, 0, 0x0000_0009_0000_0000);
    let mut tool_operand = crate::test_support::history::bounded_face_operand(Vec::new());
    tool_operand.scope_reference_ordinal = 0;
    tool_operand.resolved_face_slots = vec![7];
    let mut histories = vec![crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![state(8, Some(7)), state(7, None)],
    )];
    let timelines =
        crate::design::test_support::synthetic_feature_timelines(std::slice::from_ref(&scope));
    let mut projected = crate::test_support::with_decode_context(|ctx| {
        project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                scopes: std::slice::from_ref(&scope),
                timelines: &timelines,
                construction_groups: std::slice::from_ref(&tool),
                face_operands: std::slice::from_ref(&tool_operand),
                histories: &histories,
                ..Default::default()
            },
        )
    })
    .unwrap();
    assert!(projected
        .projection_losses
        .iter()
        .any(|loss| loss.code.local_code() == "feature.split-face-groups-unresolved"));
    let feature_id = projected.features[0].id.clone();
    let prefix = crate::ids::feature_input_prefix(&feature_id, 7);
    let mut input_topologies = vec![FeatureInputTopology {
        face_owners: Vec::new(),
        id: crate::ids::feature_input_topology_id(&feature_id, 7),
        input_of: feature_id,
        bodies: Vec::new().try_into().unwrap(),
        faces: vec![
            crate::ids::history_input_face_id(&prefix, 7),
            crate::ids::history_input_face_id(&prefix, 8),
        ]
        .try_into()
        .unwrap(),
        edges: Vec::new().try_into().unwrap(),
        vertices: Vec::new().try_into().unwrap(),
        edge_geometry: Vec::new(),
        native_ref: None,
    }];
    let bind = |features: &mut [_],
                input_topologies: &mut [_],
                groups: &[_],
                operands: &[_],
                histories: &mut [_]| {
        bind_feature_face_selections(
            features,
            input_topologies,
            FeatureFaceSelectionInputs {
                scopes: std::slice::from_ref(&scope),
                groups,
                operands,
                entity_operands: &[],
                body_recipe_operands: &[],
                histories,
                scope_histories: &std::collections::HashMap::default(),
            },
        );
    };
    bind(
        &mut projected.features,
        &mut input_topologies,
        std::slice::from_ref(&tool),
        std::slice::from_ref(&tool_operand),
        &mut histories,
    );
    let mut losses = Vec::new();
    crate::history::face_admission::admit_feature_input_faces(
        &mut projected.features,
        &input_topologies,
        &[],
        &mut losses,
    )
    .unwrap();
    assert!(matches!(projected.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::SplitFace {
            targets: FaceSelection::Native(native),
            tool: SplitFaceTool::Faces(FaceSelection::Historical { faces, .. }), ..
        }) if native == &scope.id && faces.as_slice() == [crate::ids::history_input_face_id(&prefix, 7)]));
    assert!(losses.iter().any(|loss| loss.code.local_code()
        == "feature.split-face-selection-unresolved"
        && loss.message.contains("target")));

    let target = group(200, 210, 2, 0x0000_0010_0000_0000);
    let mut draft = tool_operand.into_draft();
    draft.id = "f3d:Design/BulkStream.dat:design-face-operand#210".into();
    draft.record_index = 210;
    draft.scope_reference_ordinal = 2;
    draft.recipe_record_index = 213;
    draft.recipe_id = "f3d:Design/BulkStream.dat:construction-recipe#213".into();
    draft.next_record_index = 214;
    draft.group.as_mut().unwrap().group_record_index = 200;
    draft.resolved_face_slots = vec![8];
    let target_operand = crate::records::topology::face::DesignFaceOperand::try_new(draft).unwrap();
    bind(
        &mut projected.features,
        &mut input_topologies,
        &[tool, target],
        &[target_operand],
        &mut histories,
    );
    assert!(matches!(projected.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::SplitFace {
            targets: FaceSelection::Historical { faces, .. }, ..
        }) if faces.as_slice() == [crate::ids::history_input_face_id(&prefix, 8)]));
    let mut losses = Vec::new();
    crate::history::face_admission::admit_feature_input_faces(
        &mut projected.features,
        &input_topologies,
        &[],
        &mut losses,
    )
    .unwrap();
    assert!(!losses.iter().any(|loss| loss.code.local_code()
        == "feature.split-face-selection-unresolved"
        && loss.message.contains("target")));
}
