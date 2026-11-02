// SPDX-License-Identifier: Apache-2.0
//! Historical edge-operand binding tests.
#![allow(clippy::unwrap_used)]

use crate::history::bind_edge_operand_history_candidates;
use std::collections::HashMap;

#[test]
fn sole_transition_deletion_does_not_supply_operand_identity() {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};

    let stream = "f3d:Design/BulkStream.dat";
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        crate::records::feature::scope::DesignFeatureKind::Fillet,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut operand: crate::records::topology::edge_identity::DesignEdgeOperand =
        serde_json::from_value(serde_json::json!({
            "id": format!("{stream}:design-edge-operand#20"),
            "scope_record_index": 10,
            "scope_reference_ordinal": 0,
            "record_index": 20,
            "byte_offset": 0,
            "class_tag": "297",
            "paired_byte_offset": 16,
            "paired_class_tag": "259",
            "recipe_record_index": 23,
            "recipe_record_byte_offset": 32,
            "recipe_id": format!("{stream}:construction-recipe#23"),
            "recipe_prefix_offset": 43,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_program_offset": 0,
            "recipe_program": [],
            "changed_boundary_edge_slots": [],
            "deleted_boundary_edge_slots": [],
            "next_record_index": 24,
            "next_byte_offset": 160
        }))
        .expect("edge operand");
    let state = |state_id, topology, next_ref| {
        (
            AsmDeltaState {
                id: format!("f3d:history:state#{state_id}"),
                parent: "f3d:history".into(),
                byte_offset: Some(0),
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
            Some(topology),
            Vec::new(),
        )
    };
    let previous = state(
        1,
        AsmHistoricalTopology {
            edges: vec![17, 99],
            ..AsmHistoricalTopology::default()
        },
        None,
    );
    let current = state(
        2,
        AsmHistoricalTopology {
            edges: vec![99],
            ..AsmHistoricalTopology::default()
        },
        Some(1),
    );
    let history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![previous, current],
    );
    let scope_histories = HashMap::from([(scope.id.clone(), history.id.clone())]);

    bind_edge_operand_history_candidates(
        std::slice::from_mut(&mut operand),
        std::slice::from_ref(&scope),
        &[],
        crate::history::BindingHistories::Shared(std::slice::from_ref(&history)),
        &scope_histories,
    )
    .unwrap();

    assert_eq!(operand.recipe_state_id, Some(1));
    assert_eq!(operand.resolved_edge_slot, None);
}

#[test]
fn primary_reference_edges_bind_in_the_named_input_topology() {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalCoedge, AsmHistoricalPersistentSubentityTag,
        AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
    let program = [
        -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 1, -1, 0, -1, 2, -1, 0, 0, -1, 3, 0, -1, 0, -1, 0, -1,
        0, 0, -1,
    ];
    for kind in [DesignFeatureKind::Fillet, DesignFeatureKind::Chamfer] {
        for (second_edges, truncate, missing, expected) in [
            (vec![31, 32], false, false, Some(31)),
            (vec![30, 31], false, false, None),
            (vec![31, 32], true, false, None),
            (vec![31, 32], false, true, None),
        ] {
            let stream = "f3d:Design/BulkStream.dat";
            let mut scope = DesignParameterScope::empty(
                &format!("{stream}:design-parameter-scope#10"),
                kind.clone(),
                10,
            );
            scope
                .try_edit(|draft| {
                    draft.history_state_id = Some(2);
                    draft.previous_history_state_id = Some(1);
                    draft.layout_fixture_tail();
                })
                .unwrap();
            let mut operand: crate::records::topology::edge_identity::DesignEdgeOperand =
                serde_json::from_value(serde_json::json!({
                    "id": format!("{stream}:design-edge-operand#20"),
                    "scope_record_index": 10,
                    "scope_reference_ordinal": 0,
                    "record_index": 20,
                    "byte_offset": 0,
                    "class_tag": "297",
                    "paired_byte_offset": 16,
                    "paired_class_tag": "259",
                    "recipe_record_index": 23,
                    "recipe_record_byte_offset": 32,
                    "recipe_id": format!("{stream}:construction-recipe#23"),
                    "recipe_prefix_offset": 43,
                    "recipe_prefix_bytes": "",
                    "recipe_references": [
                        {"selector": 1, "selector_offset": 0, "token": "first", "token_offset": 0, "design_reference": 50, "design_reference_offset": 0},
                        {"selector": 1, "selector_offset": 0, "token": "second", "token_offset": 0, "design_reference": 50, "design_reference_offset": 0}
                    ],
                    "recipe_program_offset": 0,
                    "recipe_program": program,
                    "next_record_index": 24,
                    "next_byte_offset": 160
                })).unwrap();
            operand.recipe_structure = crate::design::decode::operands::edge_recipe_structure(
                &cadmpeg_test_support::service_decode_context(),
                &program,
            )
            .unwrap();
            assert!(operand.recipe_structure.is_some());
            let relation = |owner_ref, member_refs| AsmHistoricalRelation {
                owner_ref,
                member_refs,
            };
            let mut topology = AsmHistoricalTopology {
                faces: vec![10, 20],
                loops: vec![100, 200],
                edges: vec![30, 31, 32],
                coedges: vec![300, 301, 302, 303],
                face_loops: vec![relation(10, vec![100]), relation(20, vec![200])],
                loop_coedges: vec![relation(100, vec![300, 301]), relation(200, vec![302, 303])],
                persistent_subentity_tags: [(10, "first"), (20, "second")]
                    .into_iter()
                    .map(|(entity_ref, token)| AsmHistoricalPersistentSubentityTag {
                        entity_kind: AsmHistoricalEntityKind::Face,
                        entity_ref,
                        selector: 1,
                        token: token.into(),
                        design_references: vec![50],
                        ordinal: 0,
                    })
                    .collect(),
                ..Default::default()
            };
            let rows = [
                (300, 100, 30),
                (301, 100, 31),
                (302, 200, second_edges[0]),
                (303, 200, second_edges[1]),
            ];
            for &(coedge, owner_loop, edge) in &rows {
                let neighbour = rows
                    .iter()
                    .find(|row| row.1 == owner_loop && row.0 != coedge)
                    .unwrap()
                    .0;
                let radial_next = rows
                    .iter()
                    .find(|row| row.2 == edge && row.0 != coedge)
                    .map_or(coedge, |row| row.0);
                topology.coedge_topology.push(AsmHistoricalCoedge {
                    coedge,
                    owner_loop,
                    edge,
                    next: neighbour,
                    previous: neighbour,
                    radial_next,
                });
            }
            if truncate {
                topology.coedge_topology.pop();
            }
            let state = |state_id, next_ref, topology| {
                (
                    AsmDeltaState {
                        id: format!("f3d:test:state#{state_id}"),
                        parent: "f3d:test:history".into(),
                        byte_offset: Some(0),
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
                    Some(topology),
                    Vec::new(),
                )
            };
            let states = vec![
                state(2, Some(1), topology.clone()),
                state(1, None, topology),
            ];
            let mut history = crate::test_support::history::history(
                "f3d:test:history".into(),
                0,
                None,
                false,
                states,
            );
            if missing {
                history.states_mut()[1].state_id = 99;
            }
            let recipe = crate::records::recipes::ConstructionRecipe {
                id: operand.recipe_id.clone(),
                byte_offset: Some(44),
                kind: crate::records::recipes::ConstructionRecipeKind::Edge,
                design: None,
                recipe_index: 0,
                record_index: Some(crate::records::identity::RecordedValue {
                    value: 23,
                    offset: Some(32),
                }),
            };
            let owners = HashMap::from([(scope.id.clone(), history.id.clone())]);
            bind_edge_operand_history_candidates(
                std::slice::from_mut(&mut operand),
                &[scope],
                &[recipe],
                crate::history::BindingHistories::Shared(&[history]),
                &owners,
            )
            .unwrap();
            assert!(operand.preceding_candidate_faces.is_empty());
            assert_eq!(operand.resolved_edge_slot, expected);
            if expected.is_some() {
                assert_eq!(operand.recipe_state_id, Some(1));
            }
        }
    }
}
