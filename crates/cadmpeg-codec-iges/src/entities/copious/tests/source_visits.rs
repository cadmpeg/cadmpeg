// SPDX-License-Identifier: Apache-2.0

use super::super::CopiousProjectionOutcome;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeSet;

fn outcome(decoded: BTreeSet<u32>) -> CopiousProjectionOutcome {
    CopiousProjectionOutcome { decoded, losses: Vec::new(), wire_edges: Vec::new(), free_vertices: Vec::new() }
}

fn boundary(work: u64, additional: u64, operation: &'static str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut decoded = BTreeSet::new();
    let first = match outcome(BTreeSet::from([1, 3, 5])).merge_into(
        &mut decoded, &mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &ctx,
    ) {
        Err(CodecError::ResourceLimit(first)) => first,
        other => panic!("expected actual merged source/allocation refusal: {other:?}"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!((first.limit, first.used, first.additional), (work, work, additional));
    assert!(decoded.is_empty());
    for replay in [BTreeSet::from([1, 3, 5]), BTreeSet::new()] {
        assert!(matches!(outcome(replay).merge_into(
            &mut decoded, &mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn copious_merge_source_refuses_one_visit_before_any_decoded_insertion() {
    boundary(0, 1, "iges copious merged sequences");
}

#[test]
fn copious_merge_node_work_refuses_after_one_visit_without_admitting_the_tail() {
    // An empty u32 set adds one node: three admitted node passes.
    let alignment = std::mem::align_of::<u32>()
        .max(std::mem::align_of::<usize>());
    let node_bytes = 11 * std::mem::size_of::<u32>()
        + 16 * std::mem::size_of::<usize>() + 2 * alignment;
    boundary(1, u64::try_from(3 * node_bytes).unwrap(), "iges merged decoded sequences");
}

#[test]
fn empty_copious_merge_executes_no_source_steps() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    outcome(BTreeSet::new()).merge_into(
        &mut BTreeSet::new(), &mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &ctx,
    ).unwrap();
    ctx.finish_session().unwrap();
}
