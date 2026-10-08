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
