// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used, unused_imports)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use super::super::*;

#[test]
fn entity_selection_face_proofs_preserve_history_namespaces() {
    let state = |parent: &str, state_id, topology| AsmDeltaState {
        id: format!("{parent}-state-{state_id}"),
        parent: parent.into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        record_table_complete: true,
        topology: Some(topology),
        transition: None,
    };
    let history = |id: &str, state| AsmHistory {
        id: id.into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![state],
    };
    let unrelated = history(
        "unrelated",
        state(
            "unrelated",
            1,
            AsmHistoricalTopology {
                points: vec![18044],
                ..AsmHistoricalTopology::default()
            },
        ),
    );
    let selected = history(
        "selected",
        state(
            "selected",
            2,
            AsmHistoricalTopology {
                faces: vec![30],
                loops: vec![20],
                coedges: vec![10],
                pcurves: vec![18044],
                face_loops: vec![AsmHistoricalRelation {
                    owner_ref: 30,
                    member_refs: vec![20],
                }],
                loop_coedges: vec![AsmHistoricalRelation {
                    owner_ref: 20,
                    member_refs: vec![10],
                }],
                coedge_pcurves: vec![AsmHistoricalOptionalCarrierBinding {
                    entity: 10,
                    carrier: Some(18044),
                }],
                ..AsmHistoricalTopology::default()
            },
        ),
    );

    assert_eq!(
        entity_selection_face_candidates(18044, &[unrelated, selected]),
        [crate::records::DesignEntitySelectionFaceCandidate {
            history_id: "selected".into(),
            historical: crate::records::HistoricalBinding {
                kind: AsmHistoricalEntityKind::Pcurve,
                entity_ref: 18044,
                state_ids: vec![2],
            },
            face_slot: 30,
        }]
    );
}

#[test]
fn hole_face_selection_history_binds_the_unique_persistent_face() {
    let state = AsmDeltaState {
        id: "selected-state-2".into(),
        parent: "selected".into(),
        byte_offset: 0,
        state_id: 2,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 2,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        record_table_complete: true,
        topology: Some(AsmHistoricalTopology {
            faces: vec![30],
            loops: vec![20],
            coedges: vec![10],
            pcurves: vec![18044],
            face_loops: vec![AsmHistoricalRelation {
                owner_ref: 30,
                member_refs: vec![20],
            }],
            loop_coedges: vec![AsmHistoricalRelation {
                owner_ref: 20,
                member_refs: vec![10],
            }],
            coedge_pcurves: vec![AsmHistoricalOptionalCarrierBinding {
                entity: 10,
                carrier: Some(18044),
            }],
            ..AsmHistoricalTopology::default()
        }),
        transition: None,
    };
    let history = AsmHistory {
        id: "selected".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![state],
    };
    let face_selection = crate::records::DesignHoleFaceSelection {
        record_index: 100,
        byte_offset: 0,
        class_tag: "333".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        identity_record_index: 103,
        identity_record_offset: 0,
        primary_identity: 18044,
        primary_identity_offset: 0,
        secondary_identity: None,
        secondary_identity_offset: None,
        curve_secondary_identity: None,
        curve_secondary_identity_offset: None,
        historical_face_candidates: Vec::new(),
        next_record_index: 104,
        next_byte_offset: 0,
    };
    let construction = crate::records::DesignHoleConstruction {
        point_record_index: 55,
        point_record_byte_offset: 0,
        position: [0.0; 3],
        position_offset: 0,
        direction: [0.0, 0.0, 1.0],
        direction_offset: 0,
        point_parameters: [0.0; 2],
        point_parameter_offsets: [0, 0],
        reference_type: 0,
        reference_type_offset: 0,
        tangent_point_data: None,
        tangent_point_data_prefix: None,
        tangent_point_data_offset: None,
        input_record_indices: vec![55],
        input_record_offsets: vec![0],
        face_selection: Some(face_selection),
    };
    let mut scope = crate::records::DesignParameterScope::empty("f3d:scope#42", "Hole", 42);
    scope.set_hole_construction(Some(construction));

    bind_hole_selection_history(std::slice::from_mut(&mut scope), &[history]);

    assert_eq!(
        scope
            .hole_construction()
            .and_then(|construction| construction.face_selection.as_ref())
            .map(|selection| selection.historical_face_candidates.as_slice()),
        Some(
            &[crate::records::DesignEntitySelectionFaceCandidate {
                history_id: "selected".into(),
                historical: crate::records::HistoricalBinding {
                    kind: AsmHistoricalEntityKind::Pcurve,
                    entity_ref: 18044,
                    state_ids: vec![2],
                },
                face_slot: 30,
            }][..]
        )
    );
}

#[test]
fn compact_edge_treatment_deletions_require_exact_cardinality() {
    assert_eq!(
        complete_compact_edge_treatment_deletions(true, Some(2), &[17, 19]),
        [17, 19]
    );
    assert!(complete_compact_edge_treatment_deletions(true, Some(2), &[17, 18, 19]).is_empty());
    assert!(complete_compact_edge_treatment_deletions(false, Some(2), &[17, 19]).is_empty());
    assert!(complete_compact_edge_treatment_deletions(true, None, &[17, 19]).is_empty());
}

#[test]
fn compact_transition_fallback_is_scoped_to_each_operand_group() {
    let scope: crate::records::DesignParameterScope = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:scope",
        "byte_offset": 0,
        "class_tag": "300",
        "record_index": 1,
        "frame_length": 200,
        "kind": "Fillet",
        "kind_offset": 0,
        "feature_ordinal": 1,
        "feature_ordinal_offset": 0,
        "history_state_id": 8,
        "history_state_id_offset": 0,
        "previous_history_state_id": 7,
        "previous_history_state_id_offset": 0,
        "reference_count_offset": 0,
        "reference_members": [],
        "reference_member_offsets": [],
        "paired_class_tag": "261",
        "paired_byte_offset": 200
    }))
    .expect("Fillet scope");
    let identity = |group_record_index, group_member_ordinal, record_index| {
        serde_json::from_value::<DesignEdgeIdentityOperand>(serde_json::json!({
            "id": format!("f3d:test:identity#{record_index}"),
            "scope_record_index": 1,
            "group_record_index": group_record_index,
            "group_member_ordinal": group_member_ordinal,
            "record_index": record_index,
            "byte_offset": 0,
            "class_tag": "277",
            "compact_layout": true,
            "local_id": record_index,
            "local_id_offset": 0,
            "asset_id": "asset",
            "asset_id_offset": 0,
            "context_id": "context",
            "context_id_offset": 0
        }))
        .expect("edge identity")
    };
    let mut operands = vec![identity(2, 0, 10), identity(2, 1, 11), identity(3, 0, 12)];
    let state = |state_id, topology, transition| AsmDeltaState {
        id: format!("history:state#{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        record_table_complete: true,
        topology: Some(topology),
        transition,
    };
    let previous = state(
        7,
        AsmHistoricalTopology {
            edges: vec![17, 19],
            ..AsmHistoricalTopology::default()
        },
        None,
    );
    let current = state(
        8,
        AsmHistoricalTopology::default(),
        Some(AsmHistoricalTransition {
            previous_state_id: Some(7),
            records: AsmHistoricalEntityDelta::default(),
            topology: AsmHistoricalTopologyDelta::default(),
        }),
    );
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![current, previous],
    };

    bind_edge_identity_history(
        &mut operands,
        &[],
        std::slice::from_ref(&scope),
        std::slice::from_ref(&history),
        &HashMap::from([(scope.id.clone(), history.id.clone())]),
    );

    assert_eq!(operands[0].transition_edge_candidates, [17, 19]);
    assert_eq!(operands[1].transition_edge_candidates, [17, 19]);
    assert!(operands[2].transition_edge_candidates.is_empty());
}

#[test]
fn body_selection_proofs_distinguish_stable_and_topology_changing_operations() {
    let state = |state_id, transition| AsmDeltaState {
        id: format!("state-{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        record_table_complete: true,
        topology: Some(AsmHistoricalTopology {
            bodies: vec![7],
            ..AsmHistoricalTopology::default()
        }),
        transition,
    };
    let previous = state(10, None);
    let mut transition = AsmHistoricalTransition {
        previous_state_id: Some(10),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    transition.topology.bodies.updated.push(7);
    let current = state(11, Some(transition.clone()));
    assert_eq!(
        body_revision_without_topology_change(&current),
        Some(TopologyStableBodyRevision::Revised(7))
    );

    transition.topology.points.updated.push(31);
    transition.topology.surfaces.inserted.push(32);
    transition.topology.curves.deleted.push(33);
    transition.topology.pcurves.updated.push(34);
    let carrier_revisions = state(11, Some(transition.clone()));
    assert_eq!(
        body_revision_without_topology_change(&carrier_revisions),
        Some(TopologyStableBodyRevision::Revised(7))
    );

    let mut intermediate_transition = AsmHistoricalTransition {
        previous_state_id: Some(10),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    intermediate_transition.topology.bodies.updated.push(7);
    let intermediate = state(11, Some(intermediate_transition));
    transition.previous_state_id = Some(11);
    let result = state(12, Some(transition.clone()));
    let states = HashMap::from([
        (10, Some(&previous)),
        (11, Some(&intermediate)),
        (12, Some(&result)),
    ]);
    assert_eq!(
        singleton_body_revision_across_state_chain(&result, 10, &states),
        Some(7)
    );

    let topology = |bodies: &[i64]| AsmHistoricalTopology {
        bodies: bodies.to_vec(),
        ..AsmHistoricalTopology::default()
    };
    let mut split_previous = state(20, None);
    split_previous.topology = Some(topology(&[7, 8]));
    let mut split_transition = AsmHistoricalTransition {
        previous_state_id: Some(20),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    split_transition.topology.bodies.updated.push(7);
    split_transition.topology.bodies.inserted.push(9);
    let mut split_result = state(21, Some(split_transition.clone()));
    split_result.topology = Some(topology(&[7, 8, 9]));
    let split_states = HashMap::from([(20, Some(&split_previous)), (21, Some(&split_result))]);
    assert_eq!(
        singleton_revised_input_body_across_state_chain(&split_result, 20, &split_states),
        Some(7)
    );

    split_transition.topology.bodies.updated.push(8);
    let mut ambiguous_split = state(21, Some(split_transition));
    ambiguous_split.topology = Some(topology(&[7, 8, 9]));
    let ambiguous_states =
        HashMap::from([(20, Some(&split_previous)), (21, Some(&ambiguous_split))]);
    assert_eq!(
        singleton_revised_input_body_across_state_chain(&ambiguous_split, 20, &ambiguous_states),
        None
    );

    transition.previous_state_id = Some(10);
    transition.topology.faces.updated.push(19);
    let topology_changing = state(11, Some(transition));
    assert_eq!(
        body_revision_without_topology_change(&topology_changing),
        None
    );
}

#[test]
fn pattern_combine_tool_set_requires_target_membership_and_exact_cardinality() {
    let bodies = [2, 4, 5, 6, 7].into_iter().collect();

    assert_eq!(
        pattern_combine_tool_slots(&bodies, 4, 4),
        Some(vec![2, 5, 6, 7])
    );
    assert_eq!(pattern_combine_tool_slots(&bodies, 3, 5), None);
    assert_eq!(pattern_combine_tool_slots(&bodies, 4, 3), None);
    assert_eq!(pattern_combine_tool_slots(&bodies, 4, 5), None);
    assert_eq!(
        historical_body_slot("f3d:history-input:body#80:escaped-feature:35:2"),
        Some(2)
    );
    assert_eq!(historical_body_slot("f3d:brep:entity#2"), None);
}

#[test]
fn combine_recipe_family_proves_unordered_generated_tools() {
    use crate::records::{
        ConstructionRecipe, ConstructionRecipeKind, ConstructionRecipeSelector,
        DesignBodyRecipeOperand, DesignBodyRecipeReference, DesignOperandOwner,
    };

    let stream = "f3d:Design/BulkStream.dat";
    let recipe = |record_index, design_id: &str, selector| ConstructionRecipe {
        id: format!("{stream}:construction-recipe#{record_index}"),
        byte_offset: 0,
        record_index_offset: None,
        kind: ConstructionRecipeKind::Body,
        design_id: Some(design_id.into()),
        design_id_offset: None,
        design_selector: Some(ConstructionRecipeSelector {
            value: selector,
            byte_offset: 0,
        }),
        recipe_index: 0,
        record_index: 0,
    };
    let recipes = [
        recipe(101, "exact", 6),
        recipe(102, "family", 3),
        recipe(103, "family", 1),
        recipe(104, "family", 2),
    ];
    let operand =
        |record_index, recipe: &ConstructionRecipe, body: Option<i64>, candidates: &[i64]| {
            DesignBodyRecipeOperand {
                id: format!("{stream}:design-body-recipe-operand#{record_index}"),
                scope_record_index: 10,
                owner: DesignOperandOwner::ScopeReference {
                    scope_reference_ordinal: record_index,
                },
                record_index,
                byte_offset: 0,
                class_tag: "389".into(),
                asset_id: "asset".into(),
                asset_id_offset: 0,
                context_id: "context".into(),
                context_id_offset: 0,
                selector_tail: None,
                selector_tail_offset: None,
                references: vec![DesignBodyRecipeReference {
                    design_reference: if recipe.design_id.as_deref() == Some("family") {
                        413
                    } else {
                        409
                    },
                    design_reference_offset: 0,
                    form: 3,
                    form_offset: 0,
                    candidate_faces: Vec::new(),
                    preceding_candidate_faces: Vec::new(),
                    preceding_body_slots: candidates.to_vec(),
                }],
                nested_record_index: 0,
                nested_record_index_offset: 0,
                recipe_id: recipe.id.clone(),
                resolved_face_slot: None,
                resolved_body_state_id: body.map(|_| 317),
                resolved_body_slot: body,
                resolved_body_face_slots: Vec::new(),
                next_record_index: 0,
                next_byte_offset: 0,
            }
        };
    let family = [6, 7, 8];
    let mut operands = vec![
        operand(1, &recipes[0], Some(5), &[]),
        operand(2, &recipes[1], None, &[]),
        operand(3, &recipes[2], None, &family),
        operand(4, &recipes[3], None, &[]),
    ];

    assert_eq!(
        combine_recipe_family_tool_slots(stream, 10, &[1, 2, 3, 4], 317, 1, &operands, &recipes,),
        Some(vec![5, 6, 7, 8])
    );

    operands[1].references[0].preceding_body_slots = vec![6, 7, 9];
    assert!(combine_recipe_family_tool_slots(
        stream,
        10,
        &[1, 2, 3, 4],
        317,
        1,
        &operands,
        &recipes,
    )
    .is_none());

    operands[1].references[0].preceding_body_slots.clear();
    let mut duplicate_selector = recipes.clone();
    duplicate_selector[1].design_selector = duplicate_selector[2].design_selector;
    assert!(combine_recipe_family_tool_slots(
        stream,
        10,
        &[1, 2, 3, 4],
        317,
        1,
        &operands,
        &duplicate_selector,
    )
    .is_none());
}

#[test]
fn combine_external_tools_retain_complete_occurrence_local_identities() {
    use cadmpeg_ir::features::BodySelection;

    let identity = |occurrence_reference| crate::records::DesignCombineExternalBodyIdentity {
        selector_asset_id: "11111111-1111-4111-8111-111111111111".into(),
        selector_asset_id_offset: 0,
        selector_context_id: "22222222-2222-4222-8222-222222222222".into(),
        selector_context_id_offset: 0,
        occurrence_reference,
        occurrence_reference_offset: 0,
        external_body_reference: 700,
        external_body_reference_offset: 0,
        external_segment: 2,
        external_segment_offset: 0,
        external_asset_id: "11111111-1111-4111-8111-111111111111".into(),
        external_asset_id_offset: 0,
        external_link_name: "component-body-link".into(),
        external_link_name_offset: 0,
        external_property_key: None,
        external_property_key_offset: None,
        external_version_urn: None,
        external_version_urn_offset: None,
        tail_values: [0, 0],
        tail_value_offsets: [0, 12],
    };
    let tool = |record_index, occurrence_reference| crate::records::DesignCombineBodySelection {
        record_index,
        external_identity: Some(identity(occurrence_reference)),
    };
    let mut scope = crate::records::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        "Combine",
        10,
    );
    scope.set_combine_operation(Some(crate::records::DesignCombineOperation {
        form: crate::records::DesignCombineForm::ExtendedReference,
        operation: crate::records::DesignExtrudeOperation::Join,
        operation_offset: 0,
        keep_tools: false,
        keep_tools_offset: 0,
        target: crate::records::DesignCombineBodySelection {
            record_index: 11,
            external_identity: None,
        },
        tools: vec![tool(12, 500), tool(13, 501)],
    }));
    let BodySelection::Local { bodies, native } =
        super::super::combine_external_local_tools(&scope).expect("complete local tool identity")
    else {
        panic!("local body selection");
    };
    assert_eq!(bodies.len(), 2);
    assert_ne!(bodies[0], bodies[1]);
    assert_eq!(native, scope.id);

    scope
        .combine_operation_mut()
        .expect("Combine operation")
        .tools[1] = tool(13, 500);
    assert!(super::super::combine_external_local_tools(&scope).is_none());
}

#[test]
fn active_brep_face_namespace_accepts_default_or_matching_named_source() {
    use cadmpeg_ir::ids::FaceId;

    assert!(active_brep_face_matches_source(
        &FaceId::mint("f3d:brep:entity#17").expect("identity grammar"),
        "history"
    ));
    assert!(active_brep_face_matches_source(
        &FaceId::mint("f3d:brep/history/entity#17").expect("identity grammar"),
        "history"
    ));
    assert!(!active_brep_face_matches_source(
        &FaceId::mint("f3d:brep/other/entity#17").expect("identity grammar"),
        "history"
    ));
}

#[test]
fn historical_transition_separates_membership_and_revision_changes() {
    let state = |state_id, versions: &[(i64, i64)], topology| AsmDeltaState {
        id: format!("state-{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: versions
            .iter()
            .map(|&(entity_ref, record_ref)| AsmEntityVersion {
                entity_ref,
                record_ref,
            })
            .collect(),
        record_table_complete: true,
        topology: Some(topology),
        transition: None,
    };
    let previous = state(
        10,
        &[(1, 10), (4, 40), (8, 80)],
        AsmHistoricalTopology {
            bodies: vec![1],
            faces: vec![4],
            edges: vec![8],
            ..AsmHistoricalTopology::default()
        },
    );
    let current = state(
        11,
        &[(1, 11), (2, 2), (4, 40), (7, 70)],
        AsmHistoricalTopology {
            bodies: vec![1, 2],
            faces: vec![4],
            edges: vec![7],
            ..AsmHistoricalTopology::default()
        },
    );

    let transition = historical_transition(&current, Some(&previous)).unwrap();
    assert_eq!(transition.previous_state_id, Some(10));
    assert_eq!(transition.topology.bodies.inserted, [2]);
    assert_eq!(transition.topology.bodies.updated, [1]);
    assert!(transition.topology.faces.updated.is_empty());
    assert_eq!(transition.topology.edges.inserted, [7]);
    assert_eq!(transition.topology.edges.deleted, [8]);
    assert_eq!(transition.records.updated, [1]);
}

#[test]
fn snapshot_ordinals_bind_the_sorted_revision_interval() {
    let history_id = "history".to_string();
    let state_id = "state".to_string();
    let board_id = "board".to_string();
    let mut state = AsmDeltaState {
        id: state_id.clone(),
        parent: history_id,
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: vec![AsmBulletinBoard {
            id: board_id.clone(),
            parent: state_id.clone(),
            byte_offset: 0,
            owner_ref: 0,
            number: 2,
            changes: [7, 5, 6]
                .into_iter()
                .enumerate()
                .map(|(index, old_ref)| AsmEntityChange {
                    id: format!("change-{index}"),
                    parent: board_id.clone(),
                    byte_offset: index as u64,
                    kind: AsmEntityChangeKind::Update {
                        old: old_ref,
                        new: index as i64,
                    },
                })
                .collect(),
        }],
        records: (0..3)
            .map(|index| AsmHistoryRecord {
                id: format!("record-{index}"),
                parent: state_id.clone(),
                revision_id: None,
                index,
                byte_offset: index,
                name: "edge".into(),
                framing_error: None,
                entity_references: Vec::new(),
                raw_bytes: vec![0x11],
            })
            .collect(),
        entity_versions: Vec::new(),
        record_table_complete: false,
        topology: None,
        transition: None,
    };

    bind_snapshot_revision_ids(std::slice::from_mut(&mut state));

    assert_eq!(
        state
            .records
            .iter()
            .map(|record| record.revision_id)
            .collect::<Vec<_>>(),
        [Some(5), Some(6), Some(7)]
    );
}

#[test]
fn insert_only_history_uses_the_active_record_table_as_revisions() {
    let state = |node_index, next_ref, inserted: &[i64]| {
        let state_id = format!("state-{node_index}");
        let board_id = format!("board-{node_index}");
        AsmDeltaState {
            id: state_id.clone(),
            parent: "history".into(),
            byte_offset: node_index as u64,
            state_id: 10 - node_index,
            version_flag: 1,
            state_flag: 0,
            previous_ref: (node_index > 0).then_some(node_index - 1),
            next_ref,
            node_index,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: vec![AsmBulletinBoard {
                id: board_id.clone(),
                parent: state_id.clone(),
                byte_offset: node_index as u64,
                owner_ref: 0,
                number: 2,
                changes: inserted
                    .iter()
                    .enumerate()
                    .map(|(index, new_ref)| AsmEntityChange {
                        id: format!("change-{node_index}-{index}"),
                        parent: board_id.clone(),
                        byte_offset: index as u64,
                        kind: AsmEntityChangeKind::Insert { new: *new_ref },
                    })
                    .collect(),
            }],
            records: vec![AsmHistoryRecord {
                id: format!("record-{node_index}"),
                parent: state_id,
                revision_id: None,
                index: 0,
                byte_offset: node_index as u64,
                name: "End-of-ASM-History-Section".into(),
                framing_error: None,
                entity_references: Vec::new(),
                raw_bytes: vec![0x11],
            }],
            entity_versions: Vec::new(),
            record_table_complete: false,
            topology: None,
            transition: None,
        }
    };
    let mut states = vec![
        state(0, Some(1), &[1]),
        state(1, Some(2), &[2]),
        state(2, None, &[3]),
    ];

    assert_eq!(insert_only_active_record_count(&states), Some(4));
    bind_historical_entity_versions(&mut states);

    assert_eq!(
        states
            .iter()
            .map(|state| state.entity_versions.len())
            .collect::<Vec<_>>(),
        [4, 3, 2]
    );
    assert_eq!(
        states[1].entity_versions,
        [
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 2,
                record_ref: 2,
            },
            AsmEntityVersion {
                entity_ref: 3,
                record_ref: 3,
            },
        ]
    );
}

#[test]
fn insert_only_history_rejects_gaps_and_updates() {
    let mut state = AsmDeltaState {
        id: "state".into(),
        parent: "history".into(),
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: vec![AsmHistoryRecord {
            id: "record".into(),
            parent: "state".into(),
            revision_id: None,
            index: 0,
            byte_offset: 0,
            name: "End-of-ASM-History-Section".into(),
            framing_error: None,
            entity_references: Vec::new(),
            raw_bytes: vec![0x11],
        }],
        entity_versions: Vec::new(),
        record_table_complete: false,
        topology: None,
        transition: None,
    };
    let board = AsmBulletinBoard {
        id: "board".into(),
        parent: state.id.clone(),
        byte_offset: 0,
        owner_ref: 0,
        number: 2,
        changes: vec![
            AsmEntityChange {
                id: "gap-a".into(),
                parent: "board".into(),
                byte_offset: 0,
                kind: AsmEntityChangeKind::Insert { new: 1 },
            },
            AsmEntityChange {
                id: "gap-b".into(),
                parent: "board".into(),
                byte_offset: 0,
                kind: AsmEntityChangeKind::Insert { new: 3 },
            },
        ],
    };
    state.bulletin_boards.push(board);
    assert_eq!(insert_only_active_record_count(&[state.clone()]), None);
    state.bulletin_boards[0].changes[1].kind = AsmEntityChangeKind::Update { old: 2, new: 3 };
    assert_eq!(insert_only_active_record_count(&[state]), None);
}

#[test]
fn materialized_record_table_normalizes_revision_references() {
    let mut archived_bytes = vec![0x0d, 4];
    archived_bytes.extend_from_slice(b"edge");
    archived_bytes.push(0x0c);
    archived_bytes.extend_from_slice(&2i64.to_le_bytes());
    archived_bytes.push(0x11);
    let state_id = "state".to_string();
    let board_id = "board".to_string();
    let state = AsmDeltaState {
        id: state_id.clone(),
        parent: "history".into(),
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: vec![AsmBulletinBoard {
            id: board_id.clone(),
            parent: state_id.clone(),
            byte_offset: 0,
            owner_ref: 0,
            number: 2,
            changes: vec![AsmEntityChange {
                id: "change".into(),
                parent: board_id,
                byte_offset: 0,
                kind: AsmEntityChangeKind::Update { old: 2, new: 1 },
            }],
        }],
        records: vec![AsmHistoryRecord {
            id: "record".into(),
            parent: state_id,
            revision_id: Some(2),
            index: 0,
            byte_offset: 0,
            name: "edge".into(),
            framing_error: None,
            entity_references: vec![2],
            raw_bytes: archived_bytes.clone(),
        }],
        entity_versions: vec![
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 1,
                record_ref: 2,
            },
        ],
        record_table_complete: false,
        topology: None,
        transition: None,
    };
    let active = ["asmheader", "edge"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| cadmpeg_asm::sab::Record {
            index,
            name: name.into(),
            head: name.into(),
            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        })
        .collect::<Vec<_>>();

    let archive =
        historical_record_archive(std::slice::from_ref(&state), &active, &archived_bytes, 8)
            .expect("complete historical record archive");
    let table =
        materialize_record_table(&state, &archive).expect("complete historical RecordTable");

    assert_eq!(table.len(), 2);
    assert_eq!(table[1].index, 1);
    assert_eq!(&*table[1].tokens, [cadmpeg_asm::sab::Token::Ref(1)]);
}

#[test]
fn qualified_history_marker_remains_an_archived_record() {
    let mut archived_bytes = Vec::new();
    for part in ["End", "of", "ASM", "History"] {
        archived_bytes.extend_from_slice(&[0x0e, u8::try_from(part.len()).unwrap()]);
        archived_bytes.extend_from_slice(part.as_bytes());
    }
    archived_bytes.extend_from_slice(&[0x0d, 7]);
    archived_bytes.extend_from_slice(b"Section");
    archived_bytes.extend_from_slice(&[0x0d, 4]);
    archived_bytes.extend_from_slice(b"body");
    archived_bytes.push(0x0c);
    archived_bytes.extend_from_slice(&2i64.to_le_bytes());
    archived_bytes.push(0x11);
    let state_id = "state".to_string();
    let board_id = "board".to_string();
    let state = AsmDeltaState {
        id: state_id.clone(),
        parent: "history".into(),
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: vec![AsmBulletinBoard {
            id: board_id.clone(),
            parent: state_id.clone(),
            byte_offset: 0,
            owner_ref: 0,
            number: 2,
            changes: vec![AsmEntityChange {
                id: "change".into(),
                parent: board_id,
                byte_offset: 0,
                kind: AsmEntityChangeKind::Update { old: 2, new: 1 },
            }],
        }],
        records: vec![AsmHistoryRecord {
            id: "record".into(),
            parent: state_id,
            revision_id: Some(2),
            index: 0,
            byte_offset: 0,
            name: "End-of-ASM-History-Section".into(),
            framing_error: None,
            entity_references: vec![2],
            raw_bytes: archived_bytes.clone(),
        }],
        entity_versions: vec![
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 1,
                record_ref: 2,
            },
        ],
        record_table_complete: false,
        topology: None,
        transition: None,
    };
    let active = ["asmheader", "body"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| cadmpeg_asm::sab::Record {
            index,
            name: name.into(),
            head: name.into(),
            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        })
        .collect::<Vec<_>>();

    let archive =
        historical_record_archive(std::slice::from_ref(&state), &active, &archived_bytes, 8)
            .expect("qualified history marker is an archived record");
    let record = archive
        .records
        .get(&2)
        .expect("marker revision is retained");
    assert_eq!(record.name, "End-of-ASM-History-Section");
    assert_eq!(record.index, 1);
    assert!(record.tokens.contains(&cadmpeg_asm::sab::Token::Ref(1)));
}

#[test]
fn reverse_history_builds_complete_entity_version_maps() {
    let state = |node_index, previous_ref, next_ref, old_ref, new_ref| {
        let board_id = format!("board-{node_index}");
        AsmDeltaState {
            id: format!("state-{node_index}"),
            parent: "history".into(),
            byte_offset: node_index as u64,
            state_id: 10 - node_index,
            version_flag: 1,
            state_flag: 0,
            previous_ref,
            next_ref,
            node_index,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: vec![AsmBulletinBoard {
                id: board_id.clone(),
                parent: format!("state-{node_index}"),
                byte_offset: node_index as u64,
                owner_ref: 0,
                number: 2,
                changes: vec![AsmEntityChange {
                    id: format!("change-{node_index}"),
                    parent: board_id,
                    byte_offset: node_index as u64,
                    kind: match (old_ref, new_ref) {
                        (Some(old), Some(new)) => AsmEntityChangeKind::Update { old, new },
                        (None, Some(new)) => AsmEntityChangeKind::Insert { new },
                        (Some(old), None) => AsmEntityChangeKind::Delete { old },
                        (None, None) => unreachable!(),
                    },
                }],
            }],
            records: Vec::new(),
            entity_versions: Vec::new(),
            record_table_complete: false,
            topology: None,
            transition: None,
        }
    };
    let mut states = vec![
        state(0, None, Some(1), Some(3), Some(1)),
        state(1, Some(0), Some(2), Some(4), Some(1)),
        state(2, Some(1), Some(3), None, Some(2)),
        state(3, Some(2), None, None, Some(1)),
    ];
    states[0].records = [3, 4]
        .map(|revision_id| AsmHistoryRecord {
            id: format!("record-{revision_id}"),
            parent: states[0].id.clone(),
            revision_id: Some(revision_id),
            index: revision_id as u64 - 3,
            byte_offset: 0,
            name: "edge".into(),
            framing_error: None,
            entity_references: Vec::new(),
            raw_bytes: vec![0x11],
        })
        .into();

    bind_historical_entity_versions(&mut states);

    assert_eq!(
        states
            .iter()
            .map(|state| state.entity_versions.len())
            .collect::<Vec<_>>(),
        [3, 3, 3, 2]
    );
    assert_eq!(
        states[1].entity_versions,
        [
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 1,
                record_ref: 3,
            },
            AsmEntityVersion {
                entity_ref: 2,
                record_ref: 2,
            },
        ]
    );
    assert_eq!(states[2].entity_versions[1].record_ref, 4);
}

#[test]
fn profile_face_group_cardinality_requires_one_changed_surface_family() {
    let topology = AsmHistoricalTopology {
        faces: vec![10, 11, 12, 20],
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
                entity: 12,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 200,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let changed = [20, 12, 10, 11].into_iter().collect();
    assert_eq!(
        profile_face_group_cardinality_candidates(&topology, &changed, 3),
        Some(vec![10, 11, 12])
    );
    assert_eq!(
        profile_face_group_cardinality_candidates(&topology, &[20].into_iter().collect(), 1,),
        Some(vec![20])
    );
    assert_eq!(
        profile_face_group_cardinality_candidates(&topology, &[10, 20].into_iter().collect(), 1,),
        None
    );

    let mut ambiguous = topology;
    ambiguous.faces.extend([30, 31, 32]);
    ambiguous
        .face_surfaces
        .extend([30, 31, 32].map(|entity| AsmHistoricalCarrierBinding {
            entity,
            carrier: 300,
        }));
    let changed = [10, 11, 12, 30, 31, 32].into_iter().collect();
    assert_eq!(
        profile_face_group_cardinality_candidates(&ambiguous, &changed, 3),
        None
    );
}

#[test]
fn grouped_face_reference_selects_one_changed_topology_face() {
    use crate::records::DesignFaceOperand;

    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&5u32.to_le_bytes());
    for (token, references) in [
        ("1", [10u32, 20].as_slice()),
        ("2", [30u32].as_slice()),
        ("3", [40u32].as_slice()),
        ("4", [50u32].as_slice()),
        ("5", [60u32].as_slice()),
    ] {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(token.as_bytes());
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(
            &u32::try_from(references.len())
                .expect("synthetic reference count")
                .to_le_bytes(),
        );
        for reference in references {
            prefix.extend_from_slice(&reference.to_le_bytes());
        }
    }
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let mut operand = serde_json::from_value::<DesignFaceOperand>(serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:design-face-operand#1",
        "scope_record_index": 1,
        "scope_reference_ordinal": 0,
        "record_index": 2,
        "byte_offset": 0,
        "class_tag": "277",
        "paired_byte_offset": 0,
        "paired_class_tag": "259",
        "recipe_record_index": 5,
        "recipe_record_byte_offset": 0,
        "recipe_id": "f3d:Design/BulkStream.dat:construction-recipe#5",
        "recipe_prefix_offset": 0,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_kind": "bounded_face",
        "recipe_program_offset": 0,
        "recipe_program": [0],
        "recipe_node_offsets": [],
        "recipe_nodes": [],
        "next_record_index": 6,
        "next_byte_offset": 0
    }))
    .expect("grouped face operand");
    operand.recipe_prefix_bytes = prefix;
    operand.recipe_references = crate::design::decode::dimension_frames::decode_recipe_references(
        &operand.recipe_prefix_bytes,
        0,
    );
    let topology = AsmHistoricalTopology {
        faces: vec![10, 20],
        ..AsmHistoricalTopology::default()
    };

    assert_eq!(
        grouped_reference_face_candidate(&operand, &topology, &HashSet::from([10])),
        Some(
            cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(10))
                .expect("identity grammar")
        )
    );
    assert_eq!(
        grouped_reference_face_candidate(&operand, &topology, &HashSet::from([10, 20])),
        None
    );
    let mut trailing = operand;
    trailing.recipe_prefix_bytes.extend_from_slice(&[0; 4]);
    assert_eq!(
        grouped_reference_face_candidate(&trailing, &topology, &HashSet::from([10])),
        None
    );
}

#[test]
fn nested_extrude_profile_uses_root_cardinality_and_member_order() {
    use crate::records::{DesignConstructionOperandGroup, DesignFaceOperand, DesignParameterScope};
    use cadmpeg_ir::features::ProfileRef;

    let group = |record_index, scope_reference_ordinal, members: Vec<u32>| {
        let member_offsets = vec![0; members.len()];
        serde_json::from_value::<DesignConstructionOperandGroup>(serde_json::json!({
            "id": format!(
                "f3d:Design/BulkStream.dat:design-construction-operand-group#{record_index}"
            ),
            "scope_record_index": 42,
            "scope_reference_ordinal": scope_reference_ordinal,
            "record_index": record_index,
            "byte_offset": 0,
            "class_tag": "267",
            "members": members,
            "member_offsets": member_offsets,
            "frame": {
                "member_count_offset": 0,
                "opaque_index": 1,
                "opaque_index_offset": 0,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 0,
                "variant": false
            },
            "role": 279_172_874_240_u64,
            "extrude_role": "profile",
            "role_offset": 0,
            "paired_class_tag": "260",
            "paired_byte_offset": 0
        }))
        .expect("profile group")
    };
    let paired_prefix = || {
        let mut prefix = vec![0; 10];
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&2u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.push(b'2');
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&305u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.push(b'3');
        prefix.extend_from_slice(&0u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&305u32.to_le_bytes());
        prefix.extend_from_slice(&0u32.to_le_bytes());
        prefix
    };
    let face_operand = |record_index, group_record_index, scope_reference_ordinal| {
        let mut operand = serde_json::from_value::<DesignFaceOperand>(serde_json::json!({
            "id": format!("f3d:Design/BulkStream.dat:design-face-operand#{record_index}"),
            "scope_record_index": 42,
            "scope_reference_ordinal": scope_reference_ordinal,
            "group_record_index": group_record_index,
            "group_member_ordinal": 0,
            "record_index": record_index,
            "byte_offset": 0,
            "class_tag": "297",
            "paired_byte_offset": 0,
            "paired_class_tag": "259",
            "recipe_record_index": record_index + 3,
            "recipe_record_byte_offset": 0,
            "recipe_id": format!("f3d:Design/BulkStream.dat:construction-recipe#{}", record_index + 3),
            "recipe_prefix_offset": 0,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_kind": "bounded_face",
            "recipe_program_offset": 0,
            "recipe_program": [0],
            "recipe_node_offsets": [],
            "recipe_nodes": [],
            "next_record_index": record_index + 4,
            "next_byte_offset": 0
        }))
        .expect("profile face operand");
        operand.recipe_prefix_bytes = paired_prefix();
        operand.recipe_references =
            crate::design::decode::dimension_frames::decode_recipe_references(
                &operand.recipe_prefix_bytes,
                0,
            );
        operand
    };

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#42",
        "Extrude",
        42,
    );
    scope.history_state_id = Some(2);
    scope.previous_history_state_id = Some(1);
    scope.reference_members = vec![100, 110, 111, 120, 121];
    scope.reference_member_offsets = vec![0; scope.reference_members.len()];
    let groups = vec![
        group(100, 0, vec![110, 120]),
        group(110, 1, vec![111]),
        group(120, 3, vec![121]),
    ];
    let mut operands = vec![face_operand(111, 110, 1), face_operand(121, 120, 3)];
    operands[0].candidate_faces =
        vec![
            cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(10))
                .expect("identity grammar"),
        ];
    operands[1].unreferenced_candidate_faces =
        vec![
            cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(11))
                .expect("identity grammar"),
        ];

    let roots = crate::design::face_resolve::extrude_profile_group_roots(&scope, &groups)
        .expect("valid profile hierarchy");
    assert_eq!(
        roots
            .iter()
            .map(|group| group.record_index)
            .collect::<Vec<_>>(),
        [100]
    );
    assert_eq!(
        crate::design::face_resolve::extrude_profile_group_operand_indices(
            roots[0], &groups, &operands,
        )
        .expect("one leaf operand per root member"),
        [0, 1]
    );
    let mut repeated_child = groups.clone();
    repeated_child[0].members.push(110);
    assert!(
        crate::design::face_resolve::extrude_profile_group_roots(&scope, &repeated_child).is_none()
    );

    let previous_topology = AsmHistoricalTopology {
        faces: vec![10, 11, 20],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 1000,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 1001,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 2000,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let state = |state_id, topology, transition| AsmDeltaState {
        id: format!("f3d:history:state#{state_id}"),
        parent: "f3d:history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        record_table_complete: true,
        topology,
        transition,
    };
    let previous = state(1, Some(previous_topology), None);
    let mut transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    transition.topology.faces.deleted = vec![11, 10];
    let current = state(2, None, Some(transition));
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![previous, current],
    };
    let bound_history_id = history.id.clone();
    let mut unrelated_history = history.clone();
    unrelated_history.id = "f3d:other-history".into();
    unrelated_history.states[0]
        .topology
        .as_mut()
        .expect("unrelated preceding topology")
        .faces = vec![30, 31, 40];
    let histories = vec![history, unrelated_history];
    let scope_histories = HashMap::from([(scope.id.clone(), bound_history_id)]);

    bind_profile_face_group_cardinality(
        &mut operands,
        std::slice::from_ref(&scope),
        &groups,
        &histories,
        &scope_histories,
    );
    assert_eq!(operands[0].resolved_face_slots, [10]);
    assert_eq!(operands[1].resolved_face_slots, [11]);
    let profile = crate::design::face_resolve::resolved_extrude_profile_face_group(
        &scope, roots[0], &groups, &operands,
    )
    .expect("resolved root profile");
    let feature = crate::ids::neutral_feature_id(&scope);
    let feature_key = feature
        .0
        .split_once('#')
        .map_or(feature.0.as_str(), |(_, key)| key);
    let prefix = crate::ids::history_input_prefix(feature_key, 1);
    assert!(matches!(
        profile,
        ProfileRef::HistoricalFaces {
            state,
            faces,
            native,
        } if state == crate::design::edge_resolve::feature_input_topology_id(&feature, 1)
            && faces == [
                crate::ids::history_input_face_id(&prefix, 10),
                crate::ids::history_input_face_id(&prefix, 11),
            ]
            && native == [groups[0].id.clone()]
    ));
}

#[test]
fn mirror_plane_candidate_uses_unique_primary_when_persistent_identity_is_absent() {
    let candidate =
        |history_id: &str, face_slot| crate::records::DesignEntitySelectionFaceCandidate {
            history_id: history_id.into(),
            historical: crate::records::HistoricalBinding {
                kind: crate::records::AsmHistoricalEntityKind::Loop,
                entity_ref: face_slot + 100,
                state_ids: vec![2, 1],
            },
            face_slot,
        };
    let primary = candidate("history-a", 10);
    assert_eq!(
        super::super::unique_mirror_plane_candidate(vec![primary.clone()], Vec::new()),
        Some(primary.clone())
    );

    let second_primary = candidate("history-b", 20);
    assert_eq!(
        super::super::unique_mirror_plane_candidate(
            vec![primary.clone(), second_primary.clone()],
            Vec::new(),
        ),
        None
    );
    assert_eq!(
        super::super::unique_mirror_plane_candidate(
            vec![primary, second_primary.clone()],
            vec![second_primary.clone()],
        ),
        Some(second_primary.clone())
    );
    assert_eq!(
        super::super::unique_mirror_plane_candidate(
            vec![candidate("history-a", 10), second_primary.clone()],
            vec![candidate("history-a", 11), second_primary],
        ),
        None
    );
}

#[test]
fn mirror_plane_binding_falls_back_when_identity_has_no_persistent_value() {
    use crate::history_records::AsmHistoricalPlane;
    use cadmpeg_ir::math::{Point3, Vector3};
    let mut scope = crate::records::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#42",
        "Mirror",
        42,
    );
    scope.history_state_id = Some(2);
    scope.previous_history_state_id = Some(1);
    scope.set_mirror_construction(Some(
        serde_json::from_value(serde_json::json!({
            "count": 2, "count_record_index": 11, "count_offset": 0,
            "stitch_tolerance": 0.001, "stitch_tolerance_record_index": 12,
            "stitch_tolerance_offset": 0, "seed_group_record_index": 20,
            "plane_group_record_index": 30, "plane_selection_record_index": 40,
            "plane_origin": null, "plane_normal": null
        }))
        .expect("mirror construction"),
    ));
    let group: crate::records::DesignConstructionOperandGroup =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:Design/BulkStream.dat:group#30", "scope_record_index": 42,
            "scope_reference_ordinal": 0, "record_index": 30, "byte_offset": 0,
            "class_tag": "282", "members": [40], "member_offsets": [0],
            "frame": {"member_count_offset": 0, "opaque_index": 1,
                "opaque_index_offset": 0, "opaque_scalar": 0.0,
                "opaque_scalar_offset": 0, "variant": false},
            "role": 21_474_836_480u64, "role_offset": 0,
            "paired_class_tag": "261", "paired_byte_offset": 0
        }))
        .expect("mirror plane group");
    let mut operand: crate::records::DesignEntitySelectionOperand =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:Design/BulkStream.dat:operand#40", "scope_record_index": 42,
            "group_record_index": 30, "group_member_ordinal": 0, "record_index": 40,
            "byte_offset": 0, "class_tag": "313", "asset_id": "asset",
            "asset_id_offset": 0, "context_id": "context", "context_id_offset": 0,
            "identity_record_index": 41, "identity_record_offset": 0,
            "primary_identity": 10, "primary_identity_offset": 0,
            "next_record_index": 42, "next_byte_offset": 0
        }))
        .expect("mirror plane selection");
    let state = |state_id, topology, transition| crate::history_records::AsmDeltaState {
        id: format!("history:state#{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        record_table_complete: true,
        topology: Some(topology),
        transition,
    };
    let history = crate::history_records::AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![
            state(
                2,
                crate::history_records::AsmHistoricalTopology::default(),
                Some(crate::history_records::AsmHistoricalTransition {
                    previous_state_id: Some(1),
                    records: Default::default(),
                    topology: Default::default(),
                }),
            ),
            state(
                1,
                crate::history_records::AsmHistoricalTopology {
                    faces: vec![10],
                    face_surfaces: vec![crate::history_records::AsmHistoricalCarrierBinding {
                        entity: 10,
                        carrier: 20,
                    }],
                    surface_planes: vec![AsmHistoricalPlane {
                        surface: 20,
                        origin: Point3::new(1.0, 2.0, 3.0),
                        normal: Vector3::new(0.0, 0.0, 1.0),
                    }],
                    ..Default::default()
                },
                None,
            ),
        ],
    };

    bind_mirror_selection_planes(
        std::slice::from_mut(&mut scope),
        std::slice::from_ref(&group),
        std::slice::from_ref(&operand),
        &[],
        &[],
        std::slice::from_ref(&history),
    );

    let construction = scope.mirror_construction().expect("mirror construction");
    assert_eq!(construction.plane_origin, Some(Point3::new(1.0, 2.0, 3.0)));
    assert_eq!(construction.plane_normal, Some(Vector3::new(0.0, 0.0, 1.0)));

    operand.primary_identity = 44;
    bind_mirror_selection_planes(
        std::slice::from_mut(&mut scope),
        std::slice::from_ref(&group),
        std::slice::from_ref(&operand),
        &[],
        &[],
        std::slice::from_ref(&history),
    );

    let construction = scope.mirror_construction().expect("mirror construction");
    assert_eq!(construction.plane_origin, Some(Point3::new(0.0, 0.0, 0.0)));
    assert_eq!(construction.plane_normal, Some(Vector3::new(1.0, 0.0, 0.0)));
}

#[test]
fn design_geometry_origin_plane_ids_use_coordinate_planes() {
    use cadmpeg_ir::math::{Point3, Vector3};

    for (identity, normal) in [
        (42, Vector3::new(0.0, 0.0, 1.0)),
        (43, Vector3::new(0.0, 1.0, 0.0)),
        (44, Vector3::new(1.0, 0.0, 0.0)),
    ] {
        let plane = design_geometry_mirror_plane(identity).expect("origin plane");
        assert_eq!(plane.origin, Point3::new(0.0, 0.0, 0.0));
        assert_eq!(plane.normal, normal);
    }
    assert!(design_geometry_mirror_plane(45).is_none());
}

#[test]
fn historical_loop_plane_requires_coincident_axis_bearing_curves() {
    use crate::history_records::{
        AsmHistoricalCoedge, AsmHistoricalCurveAxis, AsmHistoricalOptionalCarrierBinding,
        AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut topology = AsmHistoricalTopology {
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 5,
            member_refs: vec![6, 7],
        }],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 6,
                owner_loop: 5,
                edge: 10,
                next: 7,
                previous: 7,
                radial_next: 6,
            },
            AsmHistoricalCoedge {
                coedge: 7,
                owner_loop: 5,
                edge: 11,
                next: 6,
                previous: 6,
                radial_next: 7,
            },
        ],
        edge_curves: vec![
            AsmHistoricalOptionalCarrierBinding {
                entity: 10,
                carrier: Some(20),
            },
            AsmHistoricalOptionalCarrierBinding {
                entity: 11,
                carrier: Some(21),
            },
        ],
        curve_axes: vec![
            AsmHistoricalCurveAxis {
                curve: 20,
                origin: Point3::new(1.0, 2.0, 3.0),
                direction: Vector3::new(0.0, 0.0, 1.0),
            },
            AsmHistoricalCurveAxis {
                curve: 21,
                origin: Point3::new(4.0, 5.0, 3.0),
                direction: Vector3::new(0.0, 0.0, -1.0),
            },
        ],
        ..Default::default()
    };
    let plane = historical_loop_plane(5, &topology).expect("coincident loop curve planes");
    assert_eq!(plane.origin, Point3::new(1.0, 2.0, 3.0));
    assert_eq!(plane.normal, Vector3::new(0.0, 0.0, 1.0));

    topology.curve_axes[1].origin.z = 4.0;
    assert!(historical_loop_plane(5, &topology).is_none());
}

#[test]
fn historical_mirror_plane_requires_one_exact_plane_in_the_selected_state() {
    use crate::history_records::AsmHistoricalPlane;
    use cadmpeg_ir::math::{Point3, Vector3};

    let topology = || AsmHistoricalTopology {
        faces: vec![27],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 27,
            carrier: 41,
        }],
        surface_planes: vec![AsmHistoricalPlane {
            surface: 41,
            origin: Point3 {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            normal: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        }],
        ..AsmHistoricalTopology::default()
    };
    let state = |state_id, topology| AsmDeltaState {
        id: format!("history:state#{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        record_table_complete: true,
        topology: Some(topology),
        transition: None,
    };
    let candidate = crate::records::DesignEntitySelectionFaceCandidate {
        history_id: "history".into(),
        historical: crate::records::HistoricalBinding {
            kind: AsmHistoricalEntityKind::Face,
            entity_ref: 69,
            state_ids: vec![2, 1],
        },
        face_slot: 27,
    };
    let mut history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![state(2, topology()), state(1, topology())],
    };

    let plane = historical_mirror_plane(&candidate, 1, std::slice::from_ref(&history))
        .expect("stable selected-face plane");
    assert_eq!(
        plane.origin,
        Point3 {
            x: 1.0,
            y: 2.0,
            z: 3.0
        }
    );
    assert!(historical_mirror_plane(&candidate, 3, std::slice::from_ref(&history)).is_some());
    history.states[0].topology.as_mut().unwrap().surface_planes[0]
        .normal
        .z = -1.0;
    assert!(historical_mirror_plane(&candidate, 1, std::slice::from_ref(&history)).is_some());
    assert!(historical_mirror_plane(&candidate, 3, std::slice::from_ref(&history)).is_some());
    history.states[0].topology.as_mut().unwrap().surface_planes[0]
        .origin
        .z = 4.0;
    assert!(historical_mirror_plane(&candidate, 3, std::slice::from_ref(&history)).is_none());
    let duplicate = history.states[1].topology.as_ref().unwrap().face_surfaces[0].clone();
    history.states[1]
        .topology
        .as_mut()
        .unwrap()
        .face_surfaces
        .push(duplicate);
    assert!(historical_mirror_plane(&candidate, 1, &[history]).is_none());
}
