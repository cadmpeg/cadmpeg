// SPDX-License-Identifier: Apache-2.0
//! Borrowed history states, reached uniqueness and original refusal.

use super::limits::change_state;
use crate::history::{
    effective_scope_previous_history_state_id, history_state_pair, linked_previous_state_id,
    unique_history_state_in, unique_history_state_pair_for_link, StatePairSearch,
};
use crate::history_records::{AsmDeltaState, AsmHistoricalTransition, AsmHistory};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn history(states: Vec<AsmDeltaState>) -> AsmHistory {
    AsmHistory {
        id: "synthetic:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states,
    }
}

fn policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy
}

#[test]
fn borrowed_state_query_preserves_native_value_and_free_known_exhaustion() {
    let present = history(vec![change_state(7)]);
    let empty = history(Vec::new());
    crate::test_support::with_decode_policy(&policy(1), |ctx| {
        let state = unique_history_state_in(ctx, &present, 7).unwrap().unwrap();
        assert!(std::ptr::eq(state, &present.states[0]));
        assert_eq!(state.id, present.states[0].id);
        assert_eq!(state.parent, present.states[0].parent);
        assert!(unique_history_state_in(ctx, &empty, 7).unwrap().is_none());
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "after native state")
        else { panic!("one reached state"); };
        assert_eq!(original.used, 1);
        assert_eq!(original.additional, 1);
        let Err(CodecError::ResourceLimit(sticky)) = unique_history_state_in(ctx, &empty, 7)
        else { panic!("empty query keeps original refusal"); };
        assert_eq!(sticky, original);
    });
}

#[test]
fn borrowed_state_query_stops_at_second_match_before_unread_tail() {
    let history = history(vec![change_state(7), change_state(7), change_state(99)]);
    crate::test_support::with_decode_policy(&policy(2), |ctx| {
        assert!(unique_history_state_in(ctx, &history, 7).unwrap().is_none());
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "after duplicate state")
        else { panic!("two reached state values"); };
        assert_eq!(original.used, 2);
        assert_eq!(original.additional, 1);
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    });
}

#[test]
fn same_identity_pair_reuses_unique_native_state_without_second_lookup() {
    let history = history(vec![change_state(7)]);
    // One reached state and one actual reachability-chain step.
    crate::test_support::with_decode_policy(&policy(2), |ctx| {
        let (state, previous) = history_state_pair(ctx, &history, 7, 7, false).unwrap().unwrap();
        assert!(std::ptr::eq(state, &history.states[0]));
        assert!(std::ptr::eq(previous, state));
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "after same native pair")
        else { panic!("no duplicate state query"); };
        assert_eq!(original.used, 2);
        assert_eq!(original.additional, 1);
    });
}

#[test]
fn repeated_native_pair_stops_before_unread_history_tail() {
    let histories = [
        history(vec![change_state(7)]),
        history(vec![change_state(7)]),
        history(vec![change_state(7), change_state(99)]),
    ];
    // Each reached history: source, its one state, its reachability step.
    crate::test_support::with_decode_policy(&policy(6), |ctx| {
        assert!(matches!(
            unique_history_state_pair_for_link(ctx, &histories, 7, 7, false).unwrap(),
            StatePairSearch::Ambiguous
        ));
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "after repeated pair")
        else { panic!("two reached native pairs"); };
        assert_eq!(original.used, 6);
        assert_eq!(original.additional, 1);
    });
}

#[test]
fn predecessor_keeps_raw_unique_ambiguity_derived_fallback_and_disagreement() {
    let mut current = change_state(7);
    current.next_ref = Some(9);
    let unique = history(vec![current.clone(), change_state(9)]);
    let mut repeated = change_state(99);
    repeated.node_index = 9;
    let ambiguous = history(vec![current.clone(), change_state(9), repeated]);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(linked_previous_state_id(ctx, &unique, &current).unwrap(), Some(9));
        assert_eq!(linked_previous_state_id(ctx, &ambiguous, &current).unwrap(), None);
        current.transition = Some(AsmHistoricalTransition {
            previous_state_id: Some(4), records: Default::default(), topology: Default::default(),
        });
        assert_eq!(linked_previous_state_id(ctx, &ambiguous, &current).unwrap(), Some(4));
        assert_eq!(linked_previous_state_id(ctx, &unique, &current).unwrap(), None);
    });
}

#[test]
fn explicit_and_absent_scope_predecessors_are_free_and_keep_original_refusal() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut explicit = DesignParameterScope::empty("synthetic:scope#1", DesignFeatureKind::WorkPoint, 1);
    explicit.try_edit(|draft| {
        draft.history_state_id = Some(7);
        draft.previous_history_state_id = Some(1);
        draft.layout_fixture_tail();
    }).unwrap();
    let absent = DesignParameterScope::empty("synthetic:scope#2", DesignFeatureKind::WorkPoint, 2);
    let history = history(vec![change_state(7), change_state(7)]);
    crate::test_support::with_decode_policy(&policy(0), |ctx| {
        assert_eq!(effective_scope_previous_history_state_id(ctx, &explicit, std::slice::from_ref(&history)).unwrap(), Some(1));
        assert_eq!(effective_scope_previous_history_state_id(ctx, &absent, &[]).unwrap(), None);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original predecessor refusal")
        else { panic!("original predecessor refusal"); };
        for scope in [&explicit, &absent] {
            let Err(CodecError::ResourceLimit(sticky)) = effective_scope_previous_history_state_id(ctx, scope, &[])
            else { panic!("field-only predecessor keeps original refusal"); };
            assert_eq!(sticky, original);
        }
    });
}
