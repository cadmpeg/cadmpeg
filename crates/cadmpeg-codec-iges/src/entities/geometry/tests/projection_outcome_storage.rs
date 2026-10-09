// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::collections::BTreeSet;

#[test]
fn projection_outcome_releases_decoded_and_loss_slot_storage_after_merge() {
    let alignment = std::mem::align_of::<u32>()
        .max(std::mem::align_of::<()>())
        .max(std::mem::align_of::<usize>());
    let tree_node_bytes = u64::try_from(
        11 * (std::mem::size_of::<u32>() + std::mem::size_of::<()>())
            + 16 * std::mem::size_of::<usize>()
            + 2 * alignment,
    )
    .unwrap();
    let loss_note_size = std::mem::size_of::<cadmpeg_ir::report::loss::LossNote>();
    let loss_note_bytes = u64::try_from(loss_note_size).unwrap();
    assert!((2..=1024).contains(&loss_note_size));
    // The source loss vector is scoped; the accumulator vector is retained.
    // Core's amortized reservation gives the source four slots at minimum.
    let source_storage = tree_node_bytes * 2 + loss_note_bytes * 4;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = source_storage;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut decoded_storage = ctx.reserve_scoped(0, "test decoded storage").unwrap();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();

    super::projection_outcome(&ctx, 1, true, true)
        .unwrap()
        .merge_into(&mut decoded, &mut decoded_storage, &mut losses, &ctx)
        .unwrap();
    super::projection_outcome(&ctx, 3, true, true)
        .unwrap()
        .merge_into(&mut decoded, &mut decoded_storage, &mut losses, &ctx)
        .unwrap();

    assert_eq!(decoded, BTreeSet::from([1, 3]));
    assert_eq!(losses.len(), 2);
    assert_eq!(
        losses[0].message,
        "IGES entity type 124 form 0 was not projected: test projection loss"
    );
    assert_eq!(
        losses[0]
            .provenance
            .as_ref()
            .and_then(|value| value.tag.as_deref()),
        Some("directory_entry:D1")
    );
    assert_eq!(
        losses[1]
            .provenance
            .as_ref()
            .and_then(|value| value.tag.as_deref()),
        Some("directory_entry:D3")
    );

    drop(decoded);
    drop(decoded_storage);
    let released = ctx.reserve_scoped(source_storage, "test released projection storage").unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}

fn decoded_node_bytes() -> u64 {
    u64::try_from(11 * std::mem::size_of::<u32>()
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<u32>().max(std::mem::align_of::<usize>())).unwrap()
}

fn source_outcome<'ctx>(ctx: &'ctx DecodeContext<'_>, decoded: BTreeSet<u32>)
    -> super::super::ProjectionOutcome<'ctx> {
    // Prebuilt unit inputs own one real tree node and no loss slots. Their
    // creation is outside decode traversal, but their backing stays live.
    let bytes = if decoded.is_empty() { 0 } else { decoded_node_bytes() };
    super::super::ProjectionOutcome {
        decoded,
        decoded_storage: ctx.reserve_scoped(bytes, "test projection source node").unwrap(),
        losses: Vec::new(),
        loss_slots_storage: ctx.reserve_scoped(0, "test projection loss slots").unwrap(),
    }
}

fn source_refusal(work: u64, additional: u64, operation: &'static str) {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let initial = source_outcome(&ctx, BTreeSet::from([1, 3, 5]));
    let replays = [source_outcome(&ctx, BTreeSet::from([1, 3, 5])),
        source_outcome(&ctx, BTreeSet::new())];
    let mut storage = ctx.reserve_scoped(0, "test projection target node").unwrap();
    let mut decoded = BTreeSet::new();
    let Err(CodecError::ResourceLimit(first)) = initial.merge_into(
        &mut decoded, &mut storage, &mut Vec::new(), &ctx) else {
        panic!("expected projection source or target-node refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!((first.limit, first.used, first.additional), (work, work, additional));
    assert!(decoded.is_empty());
    for replay in replays {
        assert!(matches!(replay.merge_into(&mut decoded, &mut storage, &mut Vec::new(), &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    drop(decoded);
    drop(storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn projection_merge_refuses_one_source_visit_before_insertion() {
    source_refusal(0, 1, "iges merged decoded traversal");
}

#[test]
fn projection_merge_refuses_target_node_work_after_one_source_visit() {
    source_refusal(1, 3 * decoded_node_bytes(), "iges merged decoded sequences");
}

#[test]
fn projection_merge_target_node_refuses_while_actual_source_backing_is_live() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    let node = decoded_node_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 2 * node - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let initial = source_outcome(&ctx, BTreeSet::from([1, 3, 5]));
    let mut storage = ctx.reserve_scoped(0, "test projection target node").unwrap();
    let mut decoded = BTreeSet::new();
    let Err(CodecError::ResourceLimit(first)) = initial.merge_into(
        &mut decoded, &mut storage, &mut Vec::new(), &ctx) else {
        panic!("expected simultaneous source and target-node backing refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "iges merged decoded sequences");
    assert_eq!((first.limit, first.used, first.additional), (2 * node - 1, node, node));
    assert!(decoded.is_empty());
    drop(decoded);
    drop(storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn empty_projection_merge_has_no_work_or_backing_admission() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx.reserve_scoped(0, "test projection target node").unwrap();
    let mut decoded = BTreeSet::new();
    source_outcome(&ctx, BTreeSet::new()).merge_into(
        &mut decoded, &mut storage, &mut Vec::new(), &ctx).unwrap();
    assert!(decoded.is_empty());
    drop(decoded);
    drop(storage);
    ctx.finish_session().unwrap();
}

#[test]
fn projection_merge_accepts_exact_source_and_target_work_and_releases_backing() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    let node = decoded_node_bytes();
    // Three source visits. New target nodes use three passes at length0
    // and one pass at lengths1/2; each insertion makes two key lookups.
    let work = 3 + 5 * node + 2 * (1 + 2) * u64::try_from(std::mem::size_of::<u32>()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 3;
    policy.limits.max_materialized_bytes = 2 * node;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let initial = source_outcome(&ctx, BTreeSet::from([1, 3, 5]));
    let mut storage = ctx.reserve_scoped(0, "test projection target node").unwrap();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    initial.merge_into(&mut decoded, &mut storage, &mut losses, &ctx).unwrap();
    assert_eq!(decoded, BTreeSet::from([1, 3, 5]));
    assert!(losses.is_empty());
    let source_released = ctx.reserve_scoped(node, "test projection source backing released").unwrap();
    drop(source_released);
    drop(decoded);
    drop(storage);
    let target_released = ctx.reserve_scoped(2 * node, "test projection target backing released").unwrap();
    drop(target_released);
    let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test projection exact work") else {
        panic!("expected the complete merge to use its exact admitted work");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "test projection exact work");
    assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}
