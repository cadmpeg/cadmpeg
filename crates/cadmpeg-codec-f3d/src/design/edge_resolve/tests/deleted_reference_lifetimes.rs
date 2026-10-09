// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use super::recipe_edge_operand;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension, refusal_probe::RefusalProbe};

fn reference(edges: &[i64]) -> crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
    serde_json::from_value(serde_json::json!({
        "reference_ordinal": 0, "result_faces": [], "result_shared_edge_slots": [],
        "preceding_faces": [], "shared_edge_slots": [], "changed_shared_edge_slots": [],
        "changed_reference_edge_slots": edges,
    })).unwrap()
}

fn operand(record: u32, references: &[i64], deleted: &[i64]) -> DesignEdgeOperand {
    let mut operand = recipe_edge_operand(record, deleted, deleted);
    operand.recipe_reference_contexts = vec![reference(references)];
    operand
}

fn probe_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = u64::MAX;
    policy
}

fn assert_retained_assignment(ctx: &DecodeContext<'_>, count: usize) {
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_retained(u64::MAX, "measure actual deleted reference output")
    else { panic!("retained output measurement"); };
    assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(original.used, u64::try_from(count * std::mem::size_of::<i64>()).unwrap());
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn edge_deleted_reference_lifetime_rejected_arrays_refund_actual_overlap() {
    let operand = operand(10, &[17], &[18]);
    // Two four-slot parent vectors and two four-slot scalar vectors remain
    // alive through the shared-reference proof; no output is retained.
    let overlap = 4 * u64::try_from(2 * std::mem::size_of::<Vec<i64>>() + 2 * std::mem::size_of::<i64>()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = overlap;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(deleted_reference_edge_group_candidates(&[&operand], ctx).unwrap(), None);
        }
        assert_retained_assignment(ctx, 0);
    });
    policy.limits.max_materialized_bytes = overlap - 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = deleted_reference_edge_group_candidates(&[&operand], ctx)
        else { panic!("actual overlapping parent backing must refuse one below"); };
        assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(original.operation, "f3d deleted reference boundary set");
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn edge_deleted_reference_lifetime_success_keeps_only_native_assignment() {
    let first = operand(10, &[17, 17], &[17]);
    let second = operand(11, &[18, 18], &[18]);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(deleted_reference_edge_group_candidates(&[&first, &second], ctx).unwrap(), Some(vec![17, 18]));
        assert_retained_assignment(ctx, 2);
    });
}

#[test]
fn edge_deleted_reference_lifetime_empty_context_prefix_is_paid() {
    let mut operand = recipe_edge_operand(10, &[], &[17]);
    operand.recipe_reference_contexts = vec![reference(&[]); 1024];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = deleted_reference_edge_group_candidates(&[&operand], ctx)
        else { panic!("third raw context must refuse after the paid operand"); };
        assert_eq!(original.operation, "scan F3D deleted reference contexts");
        assert_eq!((original.used, original.additional), (3, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(matches!(deleted_reference_edge_group_candidates(&[], ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

#[test]
fn edge_deleted_reference_lifetime_operand_refuses_before_backing() {
    let operand = operand(10, &[17], &[17]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = deleted_reference_edge_group_candidates(&[&operand], ctx)
        else { panic!("first source step must refuse before vector backing"); };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "scan F3D deleted reference operands");
        assert_eq!((original.used, original.additional), (0, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn edge_deleted_reference_lifetime_every_outer_scan_keeps_original_refusal() {
    let operand = operand(10, &[17, 17], &[17]);
    for operation in ["scan F3D deleted reference operands", "scan F3D deleted reference contexts", "scan F3D deleted reference edges", "scan F3D deleted reference boundary edges"] {
        crate::test_support::with_decode_policy(&probe_policy(), |ctx| {
            assert_eq!(deleted_reference_edge_group_candidates(&[&operand], ctx).unwrap(), Some(vec![17]));
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            let Err(CodecError::ResourceLimit(original)) = deleted_reference_edge_group_candidates(&[&operand], ctx)
            else { panic!("reached outer source operation must refuse"); };
            assert_eq!(original.operation, operation);
            assert_eq!(ctx.resource_refusal(), Some(original.clone()));
            assert!(matches!(deleted_reference_edge_group_candidates(&[], ctx), Err(CodecError::ResourceLimit(later)) if later == original));
        });
    }
}

#[test]
fn edge_deleted_reference_lifetime_disjoint_shared_proof_needs_no_backing() {
    let references = [vec![17]];
    let deleted = [vec![18]];
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(unique_deleted_reference_assignment(&references, &deleted, ctx).unwrap(), None);
        }
        assert_retained_assignment(ctx, 0);
    });
}

#[test]
fn edge_deleted_reference_lifetime_empty_and_length_gates_are_free_and_sticky() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(deleted_reference_edge_group_candidates(&[], ctx).unwrap(), None);
        assert_eq!(unique_deleted_reference_assignment(&[], &[], ctx).unwrap(), None);
        assert_eq!(unique_deleted_reference_assignment(&[vec![17]], &[], ctx).unwrap(), None);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original empty deleted proof refusal")
        else { panic!("zero ceiling refuses one unit"); };
        assert!(matches!(deleted_reference_edge_group_candidates(&[], ctx), Err(CodecError::ResourceLimit(later)) if later == original));
        assert!(matches!(unique_deleted_reference_assignment(&[], &[], ctx), Err(CodecError::ResourceLimit(later)) if later == original));
        assert!(matches!(unique_deleted_reference_assignment(&[vec![17]], &[], ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

#[test]
fn edge_deleted_reference_lifetime_shared_success_preserves_native_order() {
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(unique_deleted_reference_assignment(&[vec![16, 17, 18], vec![19, 20, 21]], &[vec![17, 20, 22], vec![17, 20, 22]], ctx).unwrap(), Some(vec![17, 20]));
        assert_retained_assignment(ctx, 2);
    });
}

#[test]
fn edge_deleted_reference_lifetime_ambiguous_shared_proof_retains_nothing() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(unique_deleted_reference_assignment(&[vec![17, 18], vec![17, 18]], &[vec![17, 18], vec![17, 18]], ctx).unwrap(), None);
        }
        assert_retained_assignment(ctx, 0);
    });
}

#[test]
fn edge_deleted_reference_lifetime_shared_scan_steps_keep_original_refusal() {
    for operation in ["scan F3D deleted reference pairs", "scan F3D shared deleted reference edges"] {
        crate::test_support::with_decode_policy(&probe_policy(), |ctx| {
            assert_eq!(unique_deleted_reference_assignment(&[vec![17]], &[vec![17]], ctx).unwrap(), Some(vec![17]));
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            let Err(CodecError::ResourceLimit(original)) = unique_deleted_reference_assignment(&[vec![17]], &[vec![17]], ctx)
            else { panic!("reached shared source operation must refuse"); };
            assert_eq!(original.operation, operation);
            assert_eq!(ctx.resource_refusal(), Some(original.clone()));
            assert!(matches!(unique_deleted_reference_assignment(&[], &[], ctx), Err(CodecError::ResourceLimit(later)) if later == original));
        });
    }
}

#[test]
fn edge_deleted_reference_lifetime_missing_deleted_side_skips_reference_suffix() {
    let mut operand = operand(10, &[17], &[]);
    operand.recipe_reference_contexts.extend(vec![reference(&[18]); 1024]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(deleted_reference_edge_group_candidates(&[&operand], ctx).unwrap(), None);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "measure missing deleted side work")
        else { panic!("only the actual operand was visited"); };
        assert_eq!((original.used, original.additional), (1, 1));
        assert!(matches!(deleted_reference_edge_group_candidates(&[], ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

#[test]
fn edge_deleted_reference_lifetime_empty_reference_proof_skips_parent_backing() {
    let mut operand = operand(10, &[], &[17]);
    operand.recipe_reference_contexts = vec![reference(&[]); 3];
    let mut policy = DecodePolicy::service();
    // Each call visits one operand and its three contexts. The absent
    // reference candidate makes parent arrays and the deleted scan unused.
    policy.limits.max_work_units = 2 * (1 + 3);
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(deleted_reference_edge_group_candidates(&[&operand], ctx).unwrap(), None);
        }
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "measure empty deleted reference proof work")
        else { panic!("only reached operands and contexts were visited"); };
        assert_eq!((original.used, original.additional), (8, 1));
        assert!(matches!(deleted_reference_edge_group_candidates(&[], ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}
