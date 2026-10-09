// SPDX-License-Identifier: Apache-2.0

use super::super::CopiousProjectionOutcome;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeSet;

fn node_bytes() -> usize {
    let alignment = std::mem::align_of::<u32>()
        .max(std::mem::align_of::<usize>());
    11 * std::mem::size_of::<u32>()
        + 16 * std::mem::size_of::<usize>() + 2 * alignment
}

fn outcome<'ctx>(ctx: &'ctx DecodeContext<'_>, decoded: BTreeSet<u32>) -> CopiousProjectionOutcome<'ctx> {
    // These are prebuilt unit-test inputs. Hold their container backing in
    // the caller's session without charging decode traversal or item creation.
    let nodes = if decoded.is_empty() { 0 } else { (decoded.len() - 1) / 5 + 1 };
    let decoded_storage = ctx.reserve_scoped(u64::try_from(nodes * node_bytes()).unwrap(),
        "test copious decoded input backing").unwrap();
    CopiousProjectionOutcome {
        decoded, decoded_storage,
        losses: Vec::new(),
        loss_slots_storage: ctx.reserve_scoped(0, "test copious loss input backing").unwrap(),
        wire_edges: Vec::new(),
        wire_slots_storage: ctx.reserve_scoped(0, "test copious wire input backing").unwrap(),
        free_vertices: Vec::new(),
        free_vertex_slots_storage: ctx.reserve_scoped(0, "test copious free-vertex input backing").unwrap(),
    }
}

fn boundary(work: u64, additional: u64, operation: &'static str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let initial = outcome(&ctx, BTreeSet::from([1, 3, 5]));
    let replays = [outcome(&ctx, BTreeSet::from([1, 3, 5])), outcome(&ctx, BTreeSet::new())];
    let mut decoded_storage = ctx.reserve_scoped(0, "test merged decoded storage").unwrap();
    let mut decoded = BTreeSet::new();
    let first = match initial.merge_into(
        &mut decoded, &mut decoded_storage, &mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &ctx,
    ) {
        Err(CodecError::ResourceLimit(first)) => first,
        other => panic!("expected actual merged source/allocation refusal: {other:?}"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!((first.limit, first.used, first.additional), (work, work, additional));
    assert!(decoded.is_empty());
    for replay in replays {
        assert!(matches!(replay.merge_into(
            &mut decoded, &mut decoded_storage, &mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    drop(decoded);
    drop(decoded_storage);
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
    boundary(1, u64::try_from(3 * node_bytes()).unwrap(), "iges merged decoded sequences");
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
    let mut decoded_storage = ctx.reserve_scoped(0, "test merged decoded storage").unwrap();
    outcome(&ctx, BTreeSet::new()).merge_into(
        &mut BTreeSet::new(), &mut decoded_storage, &mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &ctx,
    ).unwrap();
    drop(decoded_storage);
    ctx.finish_session().unwrap();
}
