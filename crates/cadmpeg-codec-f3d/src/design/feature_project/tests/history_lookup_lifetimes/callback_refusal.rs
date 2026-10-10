// SPDX-License-Identifier: Apache-2.0
//! Predecessor selection preserves the original callback refusal.

use super::predecessor_chain;
use crate::design::feature_project::{ScopeHistoryGraph, ScopeHistoryPredecessor};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const ORIGINAL: &str = "original F3D predecessor visitor refusal";

fn assert_callback_refusal(returned: Option<bool>) {
    let scopes = predecessor_chain();
    let original_scopes = scopes.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let graph = ScopeHistoryGraph::new(ctx, &scopes, &[], &[], &[], &[]).unwrap();
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, ORIGINAL, Some(1));
        let visits = std::cell::Cell::new(0);
        let result = graph.predecessor(ctx, &scopes[7], |candidate| {
            // The production signature is Fn; interior mutation records visits.
            visits.set(visits.get() + 1);
            assert!(std::ptr::eq(candidate, &scopes[6]));
            assert_eq!(candidate.id, scopes[6].id);
            assert!(matches!(ctx.charge_work(1, ORIGINAL), Err(CodecError::ResourceLimit(_))));
            returned.ok_or_else(|| CodecError::malformed("ordinary predecessor visitor error"))
        });
        let original = ctx.resource_refusal().expect("visitor stored its refusal");
        assert_eq!(original.operation, ORIGINAL);
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.additional, 1);
        assert!(matches!(result, Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(visits.get(), 1);
        assert!(matches!(graph.predecessor(ctx, &scopes[7], |_| {
            visits.set(visits.get() + 1);
            Ok(true)
        }), Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(visits.get(), 1);
    });
    assert_eq!(scopes, original_scopes);
}

#[test]
fn predecessor_visitor_false_keeps_original_refusal() { assert_callback_refusal(Some(false)); }

#[test]
fn predecessor_visitor_true_keeps_original_refusal() { assert_callback_refusal(Some(true)); }

#[test]
fn predecessor_visitor_ordinary_error_keeps_original_refusal() { assert_callback_refusal(None); }

#[test]
fn predecessor_visitor_ordinary_error_preserves_first_scope_and_unread_chain() {
    let scopes = predecessor_chain();
    crate::test_support::with_decode_policy(&DecodePolicy::service(), |ctx| {
        let graph = ScopeHistoryGraph::new(ctx, &scopes, &[], &[], &[], &[]).unwrap();
        let visits = std::cell::Cell::new(0);
        let result = graph.predecessor(ctx, &scopes[7], |candidate| {
            visits.set(visits.get() + 1);
            assert!(std::ptr::eq(candidate, &scopes[6]));
            Err(CodecError::malformed("ordinary predecessor visitor error"))
        });
        assert!(matches!(result, Err(CodecError::Malformed(message)) if message == "ordinary predecessor visitor error"));
        assert_eq!(visits.get(), 1);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn predecessor_visitor_preserves_actual_selected_scope_and_chain_order() {
    let scopes = predecessor_chain();
    let original_scopes = scopes.clone();
    crate::test_support::with_decode_policy(&DecodePolicy::service(), |ctx| {
        let graph = ScopeHistoryGraph::new(ctx, &scopes, &[], &[], &[], &[]).unwrap();
        let visited = std::cell::RefCell::new(Vec::new());
        let result = graph.predecessor(ctx, &scopes[7], |candidate| {
            visited.borrow_mut().push(candidate.record_index);
            Ok(candidate.record_index == 4)
        }).unwrap();
        let ScopeHistoryPredecessor::Scope(selected) = result else { panic!("selected predecessor"); };
        assert!(std::ptr::eq(selected, &scopes[3]));
        assert_eq!(selected.id, scopes[3].id);
        assert_eq!(*visited.borrow(), [7, 6, 5, 4]);
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(scopes, original_scopes);
}
