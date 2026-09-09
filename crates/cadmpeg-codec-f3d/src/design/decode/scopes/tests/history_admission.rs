// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::history_records::{
    AsmDeltaState, AsmHistoricalEntityDelta, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory,
};
use crate::records::feature::DesignParameterScope;

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
        topology_cache: crate::history_records::AsmTopologyCache::Complete(
            AsmHistoricalTopology::default(),
        ),
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
        preamble: None,
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
        crate::records::feature::DesignFeatureKind::Chamfer,
        record_index,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = byte_offset;
            draft.history_state_id = Some(state_id);
            draft.previous_history_state_id = Some(previous_state_id);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
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
    assert_eq!(scopes[0].byte_offset(), 100);
}

#[test]
fn refuses_duplicate_scope_envelopes_without_one_history_binding() {
    let mut scopes = vec![scope(42, 100, 7, 6), scope(42, 200, 9, 8)];
    scopes[1].feature_ordinal = std::num::NonZeroU32::new(2).expect("nonzero ordinal");
    let histories = [history(vec![
        history_state(7, Some(6)),
        history_state(9, Some(8)),
    ])];

    assert!(admit_history_bound_scope_variants(&mut scopes, &histories).is_err());
}

#[test]
fn retains_later_equivalent_scope_envelope_without_history_binding() {
    let mut older = scope(42, 100, 7, 6);
    older.class_tag = crate::records::DesignClassTag::try_from("392".to_owned()).unwrap();
    older
        .try_edit(|draft| {
            draft.frame_length = 260;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    older.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
    older
        .try_edit(|draft| {
            draft.reference_members = crate::records::ReferenceRun::from_columns(
                vec![101, 102, 103],
                vec![110, 120, 130],
                "reference_members",
            )
            .unwrap();
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    older.paired_class_tag = crate::records::DesignClassTag::try_from("262".to_owned()).unwrap();
    let mut newer = older.clone();
    newer.id = "f3d:stream:design-parameter-scope#200".into();
    newer
        .try_edit(|draft| {
            draft.byte_offset = 200;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    newer.class_tag = crate::records::DesignClassTag::try_from("404".to_owned()).unwrap();
    newer
        .try_edit(|draft| {
            draft.frame_length = 340;
            draft.history_state_id = Some(9);
            draft.reference_members = crate::records::ReferenceRun::from_columns(
                draft.reference_members.values().copied().collect(),
                vec![210, 220, 230],
                "reference_members",
            )
            .unwrap();
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    newer.paired_class_tag = crate::records::DesignClassTag::try_from("258".to_owned()).unwrap();

    let mut scopes = vec![older, newer];
    admit_history_bound_scope_variants(&mut scopes, &[]).expect("equivalent envelope");

    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].byte_offset(), 200);
}
