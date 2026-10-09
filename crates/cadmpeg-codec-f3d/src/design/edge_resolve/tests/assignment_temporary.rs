// SPDX-License-Identifier: Apache-2.0

use super::super::unique_bipartite_assignment;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const SCRATCH_BYTES: u64 = 4096;

#[test]
fn edge_assignment_temporary_rejected_candidates_retain_nothing_and_refund_actual_backing() {
    for candidates in [vec![vec![17, 18], vec![17, 18]], vec![vec![17], vec![17]],
        vec![vec![17], Vec::new()]] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = SCRATCH_BYTES;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(unique_bipartite_assignment(&candidates, &ctx).unwrap().is_none());
        drop(ctx.reserve_scoped(SCRATCH_BYTES, "after rejected assignment destruction").unwrap());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn edge_assignment_temporary_unique_native_order_retains_only_actual_selected_slots() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let selected_bytes = u64::try_from(2 * std::mem::size_of::<i64>()).unwrap();
    policy.limits.max_retained_bytes = selected_bytes;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = unique_bipartite_assignment(&[vec![18, 17, 17], vec![18]], &ctx).unwrap();
    assert_eq!(result, Some(vec![17, 18]));
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "after successful assignment scratch destruction").unwrap());
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_retained(1, "selected assignment retained extent")
    else { panic!("only selected backing exhausts retained bytes"); };
    assert_eq!((original.dimension, original.used, original.additional),
        (ResourceDimension::RetainedBytes, selected_bytes, 1));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn edge_assignment_temporary_selected_handoff_one_below_keeps_original_retained_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let selected_bytes = u64::try_from(2 * std::mem::size_of::<i64>()).unwrap();
    policy.limits.max_retained_bytes = selected_bytes - 1;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(original)) =
        unique_bipartite_assignment(&[vec![17], vec![18]], &ctx)
    else { panic!("actual selected handoff exceeds retained capacity by one"); };
    assert_eq!((original.dimension, original.operation, original.used, original.additional),
        (ResourceDimension::RetainedBytes, "f3d edge assignment", 0, selected_bytes));
    assert!(matches!(unique_bipartite_assignment(&[], &ctx),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn edge_assignment_temporary_empty_input_uses_no_positive_resource_then_keeps_original_fuse() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(unique_bipartite_assignment(&[], &ctx).unwrap().is_none());
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original empty assignment refusal")
    else { panic!("zero work refuses actual positive request"); };
    assert!(matches!(unique_bipartite_assignment(&[], &ctx),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}
