// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use super::recipe_edge_operand;
use cadmpeg_core::decode::{refusal_probe::RefusalProbe, DecodePolicy, ResourceDimension};

fn resolved(record: u32, edge: i64) -> DesignEdgeOperand {
    let mut operand = recipe_edge_operand(record, &[], &[]);
    operand.resolved_edge_slot = Some(edge);
    operand
}

fn no_storage_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy
}

fn assert_retained(ctx: &DecodeContext<'_>, count: usize) {
    let Err(CodecError::ResourceLimit(original)) =
        ctx.charge_retained(u64::MAX, "measure unique edge assignment output")
    else {
        panic!("retained measurement must refuse");
    };
    assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(
        original.used,
        u64::try_from(count * std::mem::size_of::<i64>()).unwrap()
    );
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn edge_unique_lifetime_group_success_retains_only_native_assignment() {
    let first = resolved(10, 17);
    let second = resolved(11, 18);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            unique_edge_group_assignment(&[&first, &second], ctx).unwrap(),
            Some(vec![17, 18])
        );
        assert_retained(ctx, 2);
    });
}

#[test]
fn edge_unique_lifetime_group_rejection_refunds_actual_candidates() {
    let first = resolved(10, 17);
    let second = resolved(11, 17);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(
                unique_edge_group_assignment(&[&first, &second], ctx).unwrap(),
                None
            );
        }
        assert_retained(ctx, 0);
    });
}

#[test]
fn edge_unique_lifetime_resolved_singleton_backing_is_admitted_before_copy() {
    let operand = resolved(10, 17);
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::try_from(std::mem::size_of::<i64>()).unwrap() - 1;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            unique_edge_group_assignment(&[&operand], ctx)
        else {
            panic!("the actual one-slot vector must refuse one below");
        };
        assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(original.operation, "f3d unique resolved edge candidate");
        assert_eq!(
            (original.used, original.additional),
            (0, u64::try_from(std::mem::size_of::<i64>()).unwrap())
        );
    });
}

#[test]
fn edge_unique_lifetime_group_context_prefix_is_paid_without_backing() {
    let operands = vec![recipe_edge_operand(10, &[], &[]); 1024];
    let references: Vec<_> = operands.iter().collect();
    crate::test_support::with_decode_policy(&no_storage_policy(3), |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            unique_edge_group_assignment(&references, ctx)
        else {
            panic!("the fourth actual operand must refuse");
        };
        assert_eq!(original.operation, "scan F3D unique edge group operands");
        assert_eq!((original.used, original.additional), (3, 1));
    });
}

#[test]
fn edge_unique_lifetime_context_prefix_is_paid_without_backing() {
    let sets: Vec<_> = (0..1024)
        .map(|_| EdgeAssignmentCandidates::Context)
        .collect();
    crate::test_support::with_decode_policy(&no_storage_policy(3), |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            unique_edge_assignment_with_context(&sets, ctx)
        else {
            panic!("the fourth actual context set must refuse");
        };
        assert_eq!(original.operation, "f3d edge normalized source");
        assert_eq!((original.used, original.additional), (3, 1));
    });
}

#[test]
fn edge_unique_lifetime_contextual_success_preserves_selected_native_order() {
    let sets = [
        EdgeAssignmentCandidates::Edges(vec![17, 18]),
        EdgeAssignmentCandidates::Context,
        EdgeAssignmentCandidates::Edges(vec![18]),
    ];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            unique_edge_assignment_with_context(&sets, ctx).unwrap(),
            Some(vec![17, 18])
        );
        assert_retained(ctx, 2);
    });
}

#[test]
fn edge_unique_lifetime_contextual_ambiguity_retains_nothing() {
    let sets = [
        EdgeAssignmentCandidates::Edges(vec![17, 18]),
        EdgeAssignmentCandidates::Context,
        EdgeAssignmentCandidates::Edges(vec![17, 18]),
    ];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(
                unique_edge_assignment_with_context(&sets, ctx).unwrap(),
                None
            );
        }
        assert_retained(ctx, 0);
    });
}

#[test]
fn edge_unique_lifetime_empty_candidate_skips_parent_and_suffix() {
    let mut sets = vec![EdgeAssignmentCandidates::Edges(Vec::new())];
    sets.extend((0..1024).map(|_| EdgeAssignmentCandidates::Edges(vec![17])));
    crate::test_support::with_decode_policy(&no_storage_policy(1), |ctx| {
        assert_eq!(
            unique_edge_assignment_with_context(&sets, ctx).unwrap(),
            None
        );
        let Err(CodecError::ResourceLimit(original)) =
            ctx.charge_work(1, "measure empty unique candidate prefix")
        else {
            panic!("one reached candidate set exhausts the ceiling");
        };
        assert_eq!((original.used, original.additional), (1, 1));
    });
}

#[test]
fn edge_unique_lifetime_each_reached_source_keeps_original_refusal() {
    let operand = resolved(10, 17);
    let sets = [EdgeAssignmentCandidates::Edges(vec![17])];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = u64::MAX;
    for operation in [
        "scan F3D unique edge group operands",
        "f3d edge normalized source",
        "f3d unique edge assignment candidate",
    ] {
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let group_route = operation == "scan F3D unique edge group operands";
            let run = || {
                if group_route {
                    unique_edge_group_assignment(&[&operand], ctx)
                } else {
                    unique_edge_assignment_with_context(&sets, ctx)
                }
            };
            assert_eq!(run().unwrap(), Some(vec![17]));
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            let Err(CodecError::ResourceLimit(original)) = run() else {
                panic!("reached unique assignment source must refuse");
            };
            assert_eq!(original.operation, operation);
            assert_eq!(ctx.resource_refusal(), Some(original));
        });
    }
}
