// SPDX-License-Identifier: Apache-2.0
//! Historical edge-operand binding tests.
#![allow(clippy::unwrap_used)]

use super::super::*;

#[test]
fn sole_transition_deletion_does_not_supply_operand_identity() {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalEntityDelta, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
        AsmHistoricalTransition, AsmHistory,
    };

    let stream = "f3d:Design/BulkStream.dat";
    let mut scope = crate::records::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        "Fillet",
        10,
    );
    scope.history_state_id = Some(2);
    scope.previous_history_state_id = Some(1);
    let mut operand: crate::records::DesignEdgeOperand =
        serde_json::from_value(serde_json::json!({
            "id": format!("{stream}:design-edge-operand#20"),
            "scope_record_index": 10,
            "scope_reference_ordinal": 0,
            "record_index": 20,
            "byte_offset": 0,
            "class_tag": "297",
            "paired_byte_offset": 0,
            "paired_class_tag": "259",
            "recipe_record_index": 23,
            "recipe_record_byte_offset": 0,
            "recipe_id": format!("{stream}:construction-recipe#23"),
            "recipe_prefix_offset": 0,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_program_offset": 0,
            "recipe_program": [],
            "changed_boundary_edge_slots": [],
            "deleted_boundary_edge_slots": [],
            "next_record_index": 24,
            "next_byte_offset": 0
        }))
        .expect("edge operand");
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
        topology: Some(topology),
        transition,
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
        Some(AsmHistoricalTransition {
            previous_state_id: Some(1),
            records: AsmHistoricalEntityDelta::default(),
            topology: AsmHistoricalTopologyDelta {
                edges: AsmHistoricalEntityDelta {
                    deleted: vec![17],
                    ..Default::default()
                },
                ..Default::default()
            },
        }),
    );
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![previous, current],
    };
    let scope_histories = HashMap::from([(scope.id.clone(), history.id.clone())]);

    bind_edge_operand_history_candidates(
        std::slice::from_mut(&mut operand),
        std::slice::from_ref(&scope),
        &[],
        std::slice::from_ref(&history),
        &scope_histories,
    );

    assert_eq!(operand.recipe_state_id, Some(1));
    assert_eq!(operand.resolved_edge_slot, None);
}
