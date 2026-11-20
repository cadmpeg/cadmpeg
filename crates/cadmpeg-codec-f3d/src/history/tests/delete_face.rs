// SPDX-License-Identifier: Apache-2.0
use crate::history::bind_feature_face_selections;
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_ir::features::{
    FaceSelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureInputTopology,
    FeatureOperation,
};

const SCOPE_ID: &str = "f3d:Design/BulkStream.dat:scope#42";
const GROUP_ID: &str = "f3d:Design/BulkStream.dat:group#43";

fn state(
    state_id: i64,
    faces: Vec<i64>,
    next_ref: Option<i64>,
) -> crate::test_support::history::State {
    (
        AsmDeltaState {
            id: format!("history:state#{state_id}"),
            parent: "history".into(),
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
            faces,
            ..AsmHistoricalTopology::default()
        }),
        Vec::new(),
    )
}

fn plate_history(after: Vec<i64>) -> AsmHistory {
    crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![state(2, after, Some(1)), state(1, vec![10, 20], None)],
    )
}

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(SCOPE_ID, DesignFeatureKind::DeleteFace, 42);
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn group(scope_record_index: u32, members: &[u32]) -> DesignConstructionOperandGroup {
    let member_offsets: Vec<u64> = (0..u64::try_from(members.len()).unwrap())
        .map(|ordinal| ordinal * 11)
        .collect();
    serde_json::from_value(serde_json::json!({
        "id": GROUP_ID,
        "byte_offset": 0,
        "class_tag": "261",
        "record_index": 43,
        "scope_record_index": scope_record_index,
        "scope_reference_ordinal": 0,
        "members": members,
        "member_offsets": member_offsets,
        "role": 68_719_476_736_u64,
        "role_offset": 0,
        "paired_byte_offset": 200,
        "paired_class_tag": "266",
        "frame": {
            "member_count_offset": 0,
            "opaque_index": 1,
            "opaque_index_offset": 18,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 22,
            "trailing_flags": [],
            "trailing_record_indices": [],
            "trailing_record_offsets": [],
            "variant": false
        }
    }))
    .unwrap()
}

fn bind(
    group: DesignConstructionOperandGroup,
    heal: bool,
    initial: FaceSelection,
    history: AsmHistory,
) -> FaceSelection {
    bind_groups(&[group], heal, initial, history)
}

fn bind_groups(
    groups: &[DesignConstructionOperandGroup],
    heal: bool,
    initial: FaceSelection,
    history: AsmHistory,
) -> FaceSelection {
    bind_groups_with_scope(groups, heal, initial, history, scope())
}

fn bind_groups_with_scope(
    groups: &[DesignConstructionOperandGroup],
    heal: bool,
    initial: FaceSelection,
    history: AsmHistory,
    scope: DesignParameterScope,
) -> FaceSelection {
    bind_groups_with_scope_and_losses(groups, heal, initial, history, scope).0
}

fn bind_groups_with_scope_and_losses(
    groups: &[DesignConstructionOperandGroup],
    heal: bool,
    initial: FaceSelection,
    history: AsmHistory,
    scope: DesignParameterScope,
) -> (FaceSelection, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let feature_id = FeatureId::mint("f3d:test:feature#42").expect("identity grammar");
    let mut feature = Feature {
        product_definition: None,
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::DeleteFace {
                faces: initial,
                heal,
            },
        )),
        native_ref: Some(SCOPE_ID.into()),
    };
    let mut input_topologies = vec![FeatureInputTopology {
        id: crate::ids::feature_input_topology_id(&feature_id, 1),
        input_of: feature_id,
        bodies: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        faces: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        edges: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        vertices: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        edge_geometry: Vec::new(),
        native_ref: None,
    }];
    let losses = bind_feature_face_selections(
        std::slice::from_mut(&mut feature),
        &mut input_topologies,
        crate::history::FeatureFaceSelectionInputs {
            scopes: &[scope],
            groups,
            operands: &[],
            entity_operands: &[],
            body_recipe_operands: &[],
            histories: &mut [history],
            scope_histories: &std::collections::HashMap::new(),
        },
    );
    let FeatureDefinition::Operation(FeatureOperation::DeleteFace { faces, .. }) =
        feature.evaluation.definition()
    else {
        panic!("DeleteFace definition");
    };
    (faces.clone(), losses)
}

#[test]
fn healed_delete_face_binds_across_intermediate_history_states() {
    let history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![
            state(4, vec![20], Some(3)),
            state(3, vec![10, 20], Some(2)),
            state(2, vec![10, 20], Some(1)),
            state(1, vec![10, 20], None),
        ],
    );
    let mut scope = scope();
    scope
        .try_edit(|draft| draft.history_state_id = Some(4))
        .unwrap();
    let bound = bind_groups_with_scope(&[group(42, &[100])], true, native(), history, scope);
    let FaceSelection::Historical { state, faces, .. } = bound else {
        panic!("historical face selection");
    };
    let feature_id = FeatureId::mint("f3d:test:feature#42").unwrap();
    let prefix = crate::ids::feature_input_prefix(&feature_id, 1);
    assert_eq!(state, crate::ids::history_input_state_id(&prefix));
    assert_eq!(
        faces.as_slice(),
        [crate::ids::history_input_face_id(&prefix, 10)]
    );
}

#[test]
fn an_intermediate_healing_deletion_prevents_the_member_count_proof() {
    let history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![
            state(3, vec![30], Some(2)),
            state(2, vec![10, 30], Some(1)),
            state(1, vec![10, 20, 30], None),
        ],
    );
    let mut scope = scope();
    scope
        .try_edit(|draft| draft.history_state_id = Some(3))
        .unwrap();
    assert_eq!(
        bind_groups_with_scope(&[group(42, &[100])], true, native(), history, scope),
        native(),
    );
}

fn native() -> FaceSelection {
    FaceSelection::Native(GROUP_ID.into())
}

#[test]
fn healed_delete_face_binds_to_the_face_its_transition_removes() {
    let bound = bind(group(42, &[100]), true, native(), plate_history(vec![20]));
    let feature_id = FeatureId::mint("f3d:test:feature#42").expect("identity grammar");
    let prefix = crate::ids::feature_input_prefix(&feature_id, 1);
    let FaceSelection::Historical { state, faces, .. } = bound else {
        panic!("DeleteFace stays native");
    };
    assert_eq!(state, crate::ids::feature_input_topology_id(&feature_id, 1));
    assert_eq!(
        faces.as_slice(),
        [crate::ids::history_input_face_id(
            &prefix,
            cadmpeg_ir::ids::IdentityKey::from(10)
        )]
    );
}

#[test]
fn unhealed_delete_face_does_not_bind_from_its_transition() {
    assert_eq!(
        bind(group(42, &[100]), false, native(), plate_history(vec![20])),
        native()
    );
}

#[test]
fn delete_face_stays_native_when_healing_removes_a_further_face() {
    assert_eq!(
        bind(group(42, &[100]), true, native(), plate_history(Vec::new())),
        native()
    );
}

#[test]
fn delete_face_group_of_another_scope_does_not_bind() {
    assert_eq!(
        bind(group(99, &[100]), true, native(), plate_history(vec![20])),
        native()
    );
}

#[test]
fn delete_face_group_id_must_name_one_group() {
    assert_eq!(
        bind_groups(
            &[group(42, &[100]), group(42, &[100])],
            true,
            native(),
            plate_history(vec![20])
        ),
        native()
    );
}

#[test]
fn delete_face_keeps_a_selection_that_is_already_bound() {
    let bound = FaceSelection::Resolved {
        faces: vec![cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#20").expect("identity grammar")],
        native: GROUP_ID.into(),
    };
    assert_eq!(
        bind(
            group(42, &[100]),
            true,
            bound.clone(),
            plate_history(vec![20])
        ),
        bound
    );
}

#[test]
fn delete_face_member_order_assumption_keeps_direct_and_interval_selections() {
    for intermediate in [false, true] {
        for members in [&[100][..], &[100, 101][..]] {
            let after = if members.len() == 1 {
                vec![20]
            } else {
                Vec::new()
            };
            let mut states = vec![state(2, after, Some(1)), state(1, vec![10, 20], None)];
            let mut scope = scope();
            if intermediate {
                if members.len() >= 2 {
                    states = vec![
                        state(3, Vec::new(), Some(2)),
                        state(2, vec![10], Some(1)),
                        state(1, vec![10, 20], None),
                    ];
                } else {
                    states.insert(0, state(3, vec![20], Some(2)));
                }
                scope
                    .try_edit(|draft| draft.history_state_id = Some(3))
                    .unwrap();
            }
            let history =
                crate::test_support::history::history("f3d:history".into(), 0, None, false, states);
            let (bound, losses) = bind_groups_with_scope_and_losses(
                &[group(42, members)],
                true,
                native(),
                history,
                scope,
            );
            let FaceSelection::Historical { faces, .. } = bound else {
                panic!("historical face selection");
            };
            let feature_id = FeatureId::mint("f3d:test:feature#42").unwrap();
            let prefix = crate::ids::feature_input_prefix(&feature_id, 1);
            let expected: Vec<_> = [10, 20]
                .into_iter()
                .take(members.len())
                .map(|slot| crate::ids::history_input_face_id(&prefix, slot))
                .collect();
            assert_eq!(faces.as_slice(), expected);
            assert_eq!(losses.len(), usize::from(members.len() >= 2));
            if let Some(loss) = losses.first() {
                assert_eq!(*loss, crate::loss::F3dLossCode::HistoryDeleteFaceMemberOrderAssumed.note(
                    format!("DeleteFace group {GROUP_ID} assigns selected faces to members in stable-slot order"),
                ));
            }
        }
    }
}
