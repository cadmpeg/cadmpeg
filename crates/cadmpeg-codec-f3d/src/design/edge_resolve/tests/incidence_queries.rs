// SPDX-License-Identifier: Apache-2.0
use crate::design::edge_resolve::unique_incidence_edge_shared_by_reference_faces;
use crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn selector(edges: Vec<i64>) -> DesignEdgeRecipeSelectorContext {
    DesignEdgeRecipeSelectorContext {
        selector: 0,
        clauses: Vec::new(),
        incidence_matching_edge_slots: edges,
        boundary_count_matching_edge_slots: Vec::new(),
    }
}

fn incidence_refusal(operation: &'static str) {
    let selectors = [selector(vec![11])];
    let references = [&[10, 11, 11][..], &[11, 10][..], &[11, 12][..]];
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| unique_incidence_edge_shared_by_reference_faces(ctx, &selectors, references),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn incidence_selector_source_refuses_work_limit() {
    incidence_refusal("scan F3D incidence selectors");
}

#[test]
fn incidence_candidate_source_refuses_work_limit() {
    incidence_refusal("scan F3D incidence edge candidates");
}

#[test]
fn incidence_reference_source_refuses_work_limit() {
    incidence_refusal("scan F3D incidence reference sets");
}

#[test]
fn incidence_reference_membership_refuses_work_limit() {
    incidence_refusal("find F3D incidence reference edge");
}

#[test]
fn incidence_prior_reference_source_refuses_work_limit() {
    incidence_refusal("find F3D duplicate reference edge set");
}

#[test]
fn incidence_prior_member_source_refuses_work_limit() {
    incidence_refusal("scan F3D prior reference edges");
}

#[test]
fn incidence_prior_membership_refuses_work_limit() {
    incidence_refusal("compare F3D reference edge sets");
}

#[test]
fn incidence_current_member_source_refuses_work_limit() {
    incidence_refusal("scan F3D reference edges");
}

#[test]
fn incidence_current_membership_refuses_work_limit() {
    incidence_refusal("compare F3D reference edge sets");
}

#[test]
fn incidence_ignores_repeated_edges_and_equivalent_reference_sets() {
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            unique_incidence_edge_shared_by_reference_faces(ctx, 
                &[selector(vec![11, 11])],
                [&[10, 11, 11][..], &[11, 10][..], &[11, 12][..]])
            .unwrap(),
            Some(11)
        );
    });
}

#[test]
fn incidence_rejects_two_distinct_qualifying_edges() {
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            unique_incidence_edge_shared_by_reference_faces(ctx, 
                &[selector(vec![11, 12])],
                [&[10, 11][..], &[11, 12][..], &[12, 13][..]])
            .unwrap(),
            None
        );
    });
}

#[test]
fn incidence_proof_uses_only_borrowed_storage() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            unique_incidence_edge_shared_by_reference_faces(ctx, 
                &[selector(vec![11])],
                [&[10, 11][..], &[11, 12][..]])
            .unwrap(),
            Some(11)
        );
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn incidence_proof_leaves_reference_tail_unvisited() {
    let tail_visited = std::cell::Cell::new(false);
    let references = [&[10, 11][..], &[11, 12][..], &[11][..]];
    let source = references
        .into_iter()
        .enumerate()
        .map(|(index, reference)| {
            if index == 2 {
                tail_visited.set(true);
            }
            reference
        });
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            unique_incidence_edge_shared_by_reference_faces(ctx, &[selector(vec![11])], source)
                .unwrap(),
            Some(11)
        );
    });
    assert!(!tail_visited.get());
}

#[test]
fn empty_incidence_selector_leaves_references_unvisited() {
    let references = [&[11][..]; 128];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            unique_incidence_edge_shared_by_reference_faces(ctx, 
                &[selector(Vec::new())],
                references)
            .unwrap(),
            None
        );
        assert!(ctx.resource_refusal().is_none());
    });
}
