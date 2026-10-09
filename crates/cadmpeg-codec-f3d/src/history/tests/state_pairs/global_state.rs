// SPDX-License-Identifier: Apache-2.0
//! Global state ambiguity and reached-source admission.

use super::limits::change_state;
use crate::history::{effective_scope_previous_history_state_id, unique_history_state};
use crate::history_records::{AsmHistoricalTransition, AsmHistory};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn history(id: &str, state_ids: &[i64]) -> AsmHistory {
    AsmHistory {
        id: id.into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: state_ids.iter().map(|&state_id| {
            let mut state = change_state(state_id);
            state.id = format!("{id}:state#{state_id}");
            state.parent = id.into();
            state
        }).collect(),
    }
}

#[test]
fn global_history_state_rejects_repetition_within_or_across_eligible_histories() {
    for histories in [
        vec![history("repeated", &[7, 7]), history("single", &[7])],
        vec![history("single", &[7]), history("repeated", &[7, 7])],
        vec![history("first", &[7]), history("second", &[7])],
        vec![history("repeated", &[7, 7])],
    ] {
        crate::test_support::with_decode_context(|ctx| {
            assert!(unique_history_state(ctx, &histories, 7).unwrap().is_none());
        });
    }
    let histories = [history("absent", &[8, 8]), history("single", &[7])];
    crate::test_support::with_decode_context(|ctx| {
        let (found_history, state) = unique_history_state(ctx, &histories, 7).unwrap().unwrap();
        assert_eq!(found_history.id, "single");
        assert_eq!(state.id, "single:state#7");
        assert_eq!(state.parent, found_history.id);
        assert_eq!(state.state_id, 7);
        assert!(unique_history_state(ctx, &histories, 9).unwrap().is_none());
    });
}

#[test]
fn repeated_global_history_state_has_no_derived_scope_predecessor() {
    let mut single = history("single", &[7]);
    single.states[0].transition = Some(AsmHistoricalTransition {
        previous_state_id: Some(1), records: Default::default(), topology: Default::default(),
    });
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:scope#1", crate::records::feature::scope::DesignFeatureKind::WorkPoint, 1,
    );
    scope.try_edit(|draft| draft.history_state_id = Some(7)).unwrap();
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(effective_scope_previous_history_state_id(ctx, &scope, std::slice::from_ref(&single)).unwrap(), Some(1));
        let histories = [history("repeated", &[7, 7]), single];
        assert_eq!(effective_scope_previous_history_state_id(ctx, &scope, &histories).unwrap(), None);
    });
}

#[test]
fn global_history_ambiguity_stops_before_unread_history_and_state_tails() {
    let histories = [history("repeated", &[7, 7, 8, 9]), history("tail", &[7, 8, 9])];
    let mut policy = DecodePolicy::service();
    // One history source step and the two matching state source steps.
    policy.limits.max_work_units = 3;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(unique_history_state(ctx, &histories, 7).unwrap().is_none());
        assert!(matches!(ctx.charge_work(1, "after global ambiguity"), Err(CodecError::ResourceLimit(limit)) if limit.used == 3 && limit.additional == 1));
    });
}

#[test]
fn global_history_state_source_refusal_is_active_and_sticky() {
    let histories = [history("single", &[7])];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    for operation in ["find F3D history state", "find F3D ASM history state"] {
        crate::test_support::with_decode_policy(&policy, |ctx| {
            assert_eq!(unique_history_state(ctx, &histories, 7).unwrap().unwrap().1.state_id, 7);
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, Some(1));
            let CodecError::ResourceLimit(original) = unique_history_state(ctx, &histories, 7).unwrap_err() else { panic!("global state refusal"); };
            assert_eq!(original.operation, operation);
            assert_eq!(original.additional, 1);
            let CodecError::ResourceLimit(sticky) = unique_history_state(ctx, &[], 7).unwrap_err() else { panic!("sticky state refusal"); };
            assert_eq!(sticky, original);
        });
    }
}

#[test]
fn explicit_scope_predecessor_does_not_query_ambiguous_global_state() {
    let histories = [history("repeated", &[7, 7])];
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:scope#1", crate::records::feature::scope::DesignFeatureKind::WorkPoint, 1,
    );
    scope.try_edit(|draft| {
        draft.history_state_id = Some(7);
        draft.previous_history_state_id = Some(1);
        draft.layout_fixture_tail();
    }).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(effective_scope_previous_history_state_id(ctx, &scope, &histories).unwrap(), Some(1));
    });
}
