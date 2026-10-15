// SPDX-License-Identifier: Apache-2.0
//! History binding bound unit tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::u64_from_index;

use crate::history::{bind_historical_entity_versions, TableBindingCost};
use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmHistoryRecord,
    AsmHistoryRecordFraming,
};

/// One state of an insert-only chain whose board inserts `inserted`.
fn insert_only_state(node_index: i64, next_ref: Option<i64>, inserted: &[i64]) -> AsmDeltaState {
    let state_id = format!("state-{node_index}");
    let board_id = format!("board-{node_index}");
    let offset = u64::try_from(node_index).unwrap();
    AsmDeltaState {
        id: state_id.clone(),
        parent: "history".into(),
        byte_offset: offset,
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
            byte_offset: offset,
            owner_ref: 0,
            number: 2,
            changes: inserted
                .iter()
                .enumerate()
                .map(|(index, new_ref)| AsmEntityChange {
                    id: format!("change-{node_index}-{index}"),
                    parent: board_id.clone(),
                    byte_offset: u64_from_index(index),
                    kind: AsmEntityChangeKind::Insert { new: *new_ref },
                })
                .collect(),
        }],
        records: vec![AsmHistoryRecord {
            id: format!("record-{node_index}"),
            parent: state_id,
            revision_id: None,
            byte_offset: offset,
            framing: AsmHistoryRecordFraming::Framed {
                index: 0,
                name: "End-of-ASM-History-Section".into(),
                entity_references: Vec::new(),
            },
            raw_bytes: vec![0x11].into(),
        }],
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: None,
    }
}

fn insert_only_chain() -> Vec<AsmDeltaState> {
    vec![
        insert_only_state(0, Some(1), &[1]),
        insert_only_state(1, Some(2), &[2]),
        insert_only_state(2, None, &[3]),
    ]
}

fn state_records(states: &[AsmDeltaState]) -> u64 {
    states
        .iter()
        .map(|state| u64_from_index(state.entity_versions.len()))
        .sum()
}

#[test]
fn history_binding_skips_a_chain_past_its_state_record_bound() {
    let mut unbounded = insert_only_chain();
    assert_eq!(
        bind_historical_entity_versions(&mut unbounded, u64::MAX).unwrap(),
        TableBindingCost::Admitted
    );
    let entries = state_records(&unbounded);
    assert!(entries > 0);

    let mut at_bound = insert_only_chain();
    assert_eq!(
        bind_historical_entity_versions(&mut at_bound, entries).unwrap(),
        TableBindingCost::Admitted
    );
    assert_eq!(at_bound, unbounded);

    let mut past_bound = insert_only_chain();
    assert_eq!(
        bind_historical_entity_versions(&mut past_bound, entries - 1).unwrap(),
        TableBindingCost::Exceeded
    );
    assert!(past_bound
        .iter()
        .all(|state| state.entity_versions.is_empty()));
}
