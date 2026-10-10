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

fn duplicate_lookup_work(count: usize) -> u64 {
    // One root key needs one comparison. A 64-key B-tree has at most two
    // levels; each admits at most eleven comparisons of a four-byte u32.
    let comparisons = match count { 1 => 1, 64 => 22, _ => panic!("unsupported fixture size") };
    comparisons * u64::try_from(std::mem::size_of::<u32>()).unwrap()
}

fn duplicate_source_boundary(count: usize, visited: usize, before_lookup: bool) {
    let keys: BTreeSet<_> = (1..=u32::try_from(count).unwrap()).collect();
    let before = keys.clone();
    let lookup = duplicate_lookup_work(count);
    let work = u64::try_from(visited).unwrap() * (1 + lookup) + u64::from(before_lookup);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let initial = outcome(&ctx, keys.clone());
    // Construct owned replay inputs before fusing the session. Their backing
    // uses the existing fixture reservation; they perform no measured visits.
    let replays: Vec<_> = (0..64).flat_map(|_| [outcome(&ctx, keys.clone()),
        outcome(&ctx, BTreeSet::new())]).collect();
    let mut decoded = keys;
    let mut decoded_storage = ctx.reserve_scoped(0, "test trusted target set").unwrap();
    let mut losses = Vec::new();
    let mut edges = Vec::new();
    let mut vertices = Vec::new();
    let first = match initial.merge_into(&mut decoded, &mut decoded_storage,
        &mut losses, &mut edges, &mut vertices, &ctx) {
        Err(CodecError::ResourceLimit(first)) => first,
        other => panic!("expected duplicate source work refusal: {other:?}"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, if before_lookup { "iges merged decoded sequences" }
        else { "iges copious merged sequences" });
    assert_eq!((first.limit, first.used, first.additional),
        (work, work, if before_lookup { lookup } else { 1 }));
    assert_eq!(decoded, before);
    for replay in replays {
        assert!(matches!(replay.merge_into(&mut decoded, &mut decoded_storage,
            &mut losses, &mut edges, &mut vertices, &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
        assert_eq!(decoded, before);
    }
    assert!(losses.is_empty() && edges.is_empty() && vertices.is_empty());
    drop(decoded);
    drop(decoded_storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn copious_duplicate_merge_refuses_first_and_last_actual_source_visits() {
    for count in [1, 64] {
        for visited in [0, count - 1] {
            duplicate_source_boundary(count, visited, false);
        }
    }
}

#[test]
fn copious_duplicate_merge_refuses_first_and_last_existing_key_lookups() {
    for count in [1, 64] {
        for visited in [0, count - 1] {
            duplicate_source_boundary(count, visited, true);
        }
    }
}

#[test]
fn copious_duplicate_merge_accepts_exact_whole_source_without_new_storage() {
    for count in [1, 64] {
        let mut decoded: BTreeSet<_> = (1..=u32::try_from(count).unwrap()).collect();
        let before = decoded.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(count).unwrap() * (1 + duplicate_lookup_work(count));
        policy.limits.max_collection_items = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let source = outcome(&ctx, decoded.clone());
        let mut decoded_storage = ctx.reserve_scoped(0, "test trusted target set").unwrap();
        source.merge_into(&mut decoded, &mut decoded_storage,
            &mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &ctx).unwrap();
        assert_eq!(decoded, before);
        drop(decoded);
        drop(decoded_storage);
        // The source iterator and its reservation both ended. The target
        // was a trusted prebuilt input, so no measured backing remains.
        let allowance = 16 * 1024 * 1024;
        let free = ctx.reserve_scoped(allowance, "test duplicate source backing released").unwrap();
        drop(free);
        ctx.finish_session().unwrap();
    }
}
