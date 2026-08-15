// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::history_records::{
    AsmDeltaState, AsmHistoricalEntityDelta, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory,
};
use crate::records::DesignParameterScope;

fn history_state(state_id: i64, previous_state_id: Option<i64>) -> AsmDeltaState {
    AsmDeltaState {
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
        topology: Some(AsmHistoricalTopology::default()),
        transition: previous_state_id.map(|previous_state_id| AsmHistoricalTransition {
            previous_state_id: Some(previous_state_id),
            records: AsmHistoricalEntityDelta::default(),
            topology: AsmHistoricalTopologyDelta::default(),
        }),
    }
}

fn history(states: Vec<AsmDeltaState>) -> AsmHistory {
    AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        stream_size: None,
        history_entry_count: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states,
    }
}

fn scope(
    record_index: u32,
    byte_offset: u64,
    state_id: i64,
    previous_state_id: i64,
) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:stream:design-parameter-scope#{byte_offset}"),
        "Chamfer",
        record_index,
    );
    scope.byte_offset = byte_offset;
    scope.history_state_id = Some(state_id);
    scope.previous_history_state_id = Some(previous_state_id);
    scope
}

#[test]
fn retains_the_unique_history_bound_scope_envelope() {
    let mut scopes = vec![scope(42, 200, 9, 8), scope(42, 100, 7, 6)];
    let histories = [history(vec![
        history_state(7, Some(6)),
        history_state(6, None),
    ])];

    admit_history_bound_scope_variants(&mut scopes, &histories).expect("unique envelope");

    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].byte_offset, 100);
}

#[test]
fn refuses_duplicate_scope_envelopes_without_one_history_binding() {
    let mut scopes = vec![scope(42, 100, 7, 6), scope(42, 200, 9, 8)];
    let histories = [history(vec![
        history_state(7, Some(6)),
        history_state(9, Some(8)),
    ])];

    assert!(admit_history_bound_scope_variants(&mut scopes, &histories).is_err());
}
