// SPDX-License-Identifier: Apache-2.0
use super::recipe_edge_operand;
use crate::design::edge_resolve::{context_only_edge_group_candidates, resolved_edge_operand};
use crate::records::topology::edge_identity::DesignEdgeOperand;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn terminal_operand(first: &[i64], second: &[i64], boundary: &[i64]) -> DesignEdgeOperand {
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_structure = Some(
        crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
            root: 2,
            sides: (0..2)
                .map(
                    |_| crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
                        header_value: 0,
                        scalars: vec![1, 2],
                        payload_prefix: vec![0],
                        entries: Vec::new(),
                    },
                )
                .collect(),
        },
    );
    operand.terminal_reference_edge_slots = vec![first.to_vec(), second.to_vec()];
    operand.terminal_boundary_edge_slots = boundary.to_vec();
    operand
}

fn terminal_refusal(operation: &'static str) {
    let operand = terminal_operand(&[17, 17, 19], &[17, 18], &[99, 17]);
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| resolved_edge_operand(&operand, ctx),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn terminal_reference_source_refuses_work_limit() {
    terminal_refusal("scan F3D terminal reference edges");
}

#[test]
fn terminal_reference_membership_refuses_work_limit() {
    terminal_refusal("find F3D terminal shared reference edge");
}

#[test]
fn terminal_boundary_membership_refuses_work_limit() {
    terminal_refusal("find F3D terminal boundary edge");
}

#[test]
fn terminal_resolution_uses_borrowed_storage_and_accepts_repeated_edges() {
    let operand = terminal_operand(&[17, 17, 19], &[17, 18], &[99, 17]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(resolved_edge_operand(&operand, ctx).unwrap(), Some(17));
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn terminal_resolution_stops_at_second_distinct_common_edge() {
    let mut first = vec![17, 18];
    first.extend(std::iter::repeat_n(99, 10_000));
    let operand = terminal_operand(&first, &[17, 18], &[17, 18]);
    let mut policy = DecodePolicy::service();
    let short = terminal_operand(&[17, 18], &[17, 18], &[17, 18]);
    let reference_work = crate::test_support::with_decode_context(|ctx| {
        assert_eq!(resolved_edge_operand(&short, ctx).unwrap(), None);
        let Err(CodecError::ResourceLimit(usage)) = ctx.charge_work(u64::MAX, "terminal completed work") else { panic!("completed work observer"); };
        usage.used
    });
    policy.limits.max_work_units = reference_work;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(resolved_edge_operand(&operand, ctx).unwrap(), None);
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn terminal_resolution_keeps_boundary_admission_rule() {
    let operand = terminal_operand(&[17, 19], &[17, 18], &[18, 19]);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(resolved_edge_operand(&operand, ctx).unwrap(), None);
    });
}

#[test]
fn context_only_query_stops_before_mapper_error_in_tail() {
    let tail_visited = std::cell::Cell::new(false);
    let members = [(None, &[17][..]), (Some(18), &[18][..])]
        .into_iter()
        .enumerate()
        .map(|(index, member)| {
            if index == 1 {
                tail_visited.set(true);
                return Err(CodecError::malformed("unvisited context tail"));
            }
            Ok(member)
        });
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            context_only_edge_group_candidates(members, ctx).unwrap(),
            None
        );
    });
    assert!(!tail_visited.get());
}

#[test]
fn context_only_source_refuses_work_limit() {
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "scan F3D context-only edge members",
        0,
        |ctx| {
            context_only_edge_group_candidates(
                [(None, &[][..]), (Some(17), &[17][..])].into_iter().map(Ok),
                ctx,
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "scan F3D context-only edge members"));
}

#[test]
fn context_only_membership_refuses_work_limit() {
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "find F3D context-only edge candidate",
        0,
        |ctx| {
            context_only_edge_group_candidates(
                [(Some(17), &[17][..]), (Some(18), &[18][..])]
                    .into_iter()
                    .map(Ok),
                ctx,
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "find F3D context-only edge candidate"));
}

#[test]
fn rejected_context_only_candidates_release_scratch() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1 << 20;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            context_only_edge_group_candidates(
                [(Some(17), &[17][..]), (None, &[18][..])]
                    .into_iter()
                    .map(Ok),
                ctx
            )
            .unwrap(),
            None
        );
        drop(
            ctx.reserve_scoped(
                policy.limits.max_materialized_bytes,
                "f3d context-only scratch release",
            )
            .unwrap(),
        );
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn accepted_context_only_candidates_refuse_retained_copy() {
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::RetainedBytes,
        "hold F3D context-only edge candidates",
        0,
        |ctx| context_only_edge_group_candidates([(Some(17), &[17][..])].into_iter().map(Ok), ctx),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "hold F3D context-only edge candidates"));
}
