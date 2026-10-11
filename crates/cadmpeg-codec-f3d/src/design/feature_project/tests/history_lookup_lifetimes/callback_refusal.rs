// SPDX-License-Identifier: Apache-2.0
//! Predecessor selection preserves the original callback refusal.

use super::predecessor_chain;
use crate::design::feature_project::{ScopeHistoryGraph, ScopeHistoryPredecessor};
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_core::CodecError;

#[test]
fn predecessor_visitor_ordinary_error_preserves_first_scope_and_unread_chain() {
    let scopes = predecessor_chain();
    crate::test_support::with_decode_policy(&DecodePolicy::service(), |ctx| {
        let (graph, _graph_storage) =
            ScopeHistoryGraph::new(ctx, &scopes, &[], &[], &[], &[]).unwrap();
        let visits = std::cell::Cell::new(0);
        let result = graph.predecessor(ctx, &scopes[7], |candidate| {
            visits.set(visits.get() + 1);
            assert!(std::ptr::eq(candidate, &raw const scopes[6]));
            Err(CodecError::malformed("ordinary predecessor visitor error"))
        });
        assert!(
            matches!(result, Err(CodecError::Malformed(message)) if message == "ordinary predecessor visitor error")
        );
        assert_eq!(visits.get(), 1);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn predecessor_visitor_preserves_actual_selected_scope_and_chain_order() {
    let scopes = predecessor_chain();
    let original_scopes = scopes.clone();
    crate::test_support::with_decode_policy(&DecodePolicy::service(), |ctx| {
        let (graph, _graph_storage) =
            ScopeHistoryGraph::new(ctx, &scopes, &[], &[], &[], &[]).unwrap();
        let visited = std::cell::RefCell::new(Vec::new());
        let result = graph
            .predecessor(ctx, &scopes[7], |candidate| {
                visited.borrow_mut().push(candidate.record_index);
                Ok(candidate.record_index == 4)
            })
            .unwrap();
        let ScopeHistoryPredecessor::Scope(selected) = result else {
            panic!("selected predecessor");
        };
        assert!(std::ptr::eq(selected, &raw const scopes[3]));
        assert_eq!(selected.id, scopes[3].id);
        assert_eq!(*visited.borrow(), [7, 6, 5, 4]);
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(scopes, original_scopes);
}
