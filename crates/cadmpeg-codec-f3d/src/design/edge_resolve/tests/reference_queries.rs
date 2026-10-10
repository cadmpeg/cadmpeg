// SPDX-License-Identifier: Apache-2.0
use super::recipe_edge_operand;
use crate::design::edge_resolve::{
    edge_assignment_candidates, edge_group_assignment_candidates,
    transition_chain_is_supported_by_recipe, EdgeAssignmentCandidates,
};
use crate::design::edge_references::edge_operand_reference_edge_sets;
use crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn selector(edges: &[i64]) -> DesignEdgeRecipeSelectorContext {
    DesignEdgeRecipeSelectorContext {
        selector: 0,
        clauses: Vec::new(),
        incidence_matching_edge_slots: edges.to_vec(),
        boundary_count_matching_edge_slots: Vec::new(),
    }
}

fn corroboration_refusal(operation: &'static str) {
    let selectors = [selector(&[17, 18, 18]), selector(&[18, 19])];
    let references = [None, Some(&[18, 20][..])];
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| edge_assignment_candidates(&selectors, references, ctx),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn assignment_selector_kind_refuses_work_limit() {
    corroboration_refusal("select F3D edge assignment evidence");
}

#[test]
fn corroborated_selector_source_refuses_work_limit() {
    corroboration_refusal("scan F3D corroborated candidate selectors");
}

#[test]
fn corroborated_selector_membership_refuses_work_limit() {
    corroboration_refusal("find F3D corroborated selector candidate");
}

#[test]
fn corroborated_reference_source_refuses_work_limit() {
    corroboration_refusal("scan F3D corroborated shared sets");
}

#[test]
fn corroborated_reference_membership_refuses_work_limit() {
    corroboration_refusal("find F3D corroborated shared candidate");
}

#[test]
fn invalid_local_ordinals_remain_visible_to_reference_admission() {
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.terminal_reference_edge_slots = vec![vec![17], vec![]];
    operand.local_topology_references = Some(
        [99, 2, 1]
            .map(|value| std::num::NonZeroU32::new(value).unwrap())
            .to_vec(),
    );
    assert_eq!(
        edge_operand_reference_edge_sets(&operand).collect::<Vec<_>>(),
        vec![None, Some(&[][..]), Some(&[17][..])]
    );
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "scan F3D assignment reference sources",
        1,
        |ctx| {
            edge_group_assignment_candidates(&[], edge_operand_reference_edge_sets(&operand), ctx)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "scan F3D assignment reference sources"));
}

#[test]
fn reference_pair_query_leaves_tail_unvisited() {
    let visited = std::cell::Cell::new(0);
    let references = [
        None,
        Some(&[][..]),
        Some(&[17, 18][..]),
        Some(&[18, 19][..]),
        Some(&[20][..]),
    ];
    crate::test_support::with_decode_context(|ctx| {
        let result = edge_group_assignment_candidates(
            &[],
            references.into_iter().inspect(|_| {
                visited.set(visited.get() + 1);
            }),
            ctx,
        )
        .unwrap();
        assert_eq!(result, Some(EdgeAssignmentCandidates::Edges(vec![18])));
    });
    assert_eq!(visited.get(), 4);
}

#[test]
fn group_skips_empty_references_but_direct_assignment_rejects_them() {
    crate::test_support::with_decode_context(|ctx| {
        let selectors = [selector(&[18])];
        assert_eq!(
            edge_group_assignment_candidates(&selectors, [Some(&[][..]), Some(&[18][..])], ctx)
                .unwrap(),
            Some(EdgeAssignmentCandidates::Edges(vec![18]))
        );
        assert_eq!(
            edge_assignment_candidates(&selectors, [Some(&[][..]), Some(&[18][..])], ctx).unwrap(),
            None
        );
        assert_eq!(
            edge_assignment_candidates(&selectors, [None, Some(&[18][..])], ctx).unwrap(),
            Some(vec![18])
        );
    });
}

#[test]
fn rejected_reference_candidates_release_scratch_without_retained_bytes() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1 << 20;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            edge_group_assignment_candidates(&[], [Some(&[17][..]), Some(&[18][..])], ctx).unwrap(),
            None
        );
        assert_eq!(
            edge_assignment_candidates(&[selector(&[17])], [Some(&[18][..])], ctx).unwrap(),
            None
        );
        drop(
            ctx.reserve_scoped(
                policy.limits.max_materialized_bytes,
                "f3d reference scratch release",
            )
            .unwrap(),
        );
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn accepted_reference_candidates_refuse_retained_copy() {
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::RetainedBytes,
        "hold F3D assignment reference candidates",
        0,
        |ctx| edge_group_assignment_candidates(&[], [Some(&[17, 18][..]), Some(&[18][..])], ctx),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "hold F3D assignment reference candidates"));
}

#[test]
fn accepted_corroborated_candidates_refuse_retained_copy() {
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::RetainedBytes,
        "hold F3D corroborated edge candidates",
        0,
        |ctx| edge_assignment_candidates(&[selector(&[17, 18])], [Some(&[18][..])], ctx),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "hold F3D corroborated edge candidates"));
}

fn transition_refusal(operation: &'static str) {
    let mut operand = recipe_edge_operand(10, &[17, 18], &[18]);
    operand.resolved_edge_slot = Some(18);
    operand.terminal_reference_edge_slots = vec![vec![18, 19]];
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| transition_chain_is_supported_by_recipe(&[18], 1, &[&operand], ctx),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn transition_operand_source_refuses_work_limit() {
    transition_refusal("scan F3D transition recipe operands");
}

#[test]
fn transition_deleted_edge_membership_refuses_work_limit() {
    transition_refusal("find F3D transition deleted recipe edge");
}

#[test]
fn transition_deleted_edge_query_refuses_work_limit() {
    transition_refusal("scan F3D transition deleted recipe edges");
}

#[test]
fn transition_chain_membership_refuses_work_limit() {
    transition_refusal("find F3D transition deleted chain edge");
}

#[test]
fn transition_boundary_source_refuses_work_limit() {
    transition_refusal("scan F3D transition boundary edges");
}

#[test]
fn transition_reference_source_refuses_work_limit() {
    transition_refusal("scan F3D transition reference sources");
}

#[test]
fn transition_reference_edge_source_refuses_work_limit() {
    transition_refusal("scan F3D transition reference edges");
}

#[test]
fn transition_recipe_membership_refuses_work_limit() {
    transition_refusal("find F3D transition covered edge");
}

#[test]
fn transition_union_is_scratch_and_preserves_duplicates_and_references() {
    let mut operand = recipe_edge_operand(10, &[17, 17], &[]);
    operand.terminal_reference_edge_slots = vec![vec![18, 18]];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1 << 20;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(transition_chain_is_supported_by_recipe(&[17, 18], 1, &[&operand], ctx).unwrap());
        assert!(!transition_chain_is_supported_by_recipe(&[17, 19], 1, &[&operand], ctx).unwrap());
        drop(
            ctx.reserve_scoped(
                policy.limits.max_materialized_bytes,
                "f3d transition scratch release",
            )
            .unwrap(),
        );
        assert!(ctx.resource_refusal().is_none());
    });
}
