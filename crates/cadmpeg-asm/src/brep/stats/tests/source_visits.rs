// SPDX-License-Identifier: Apache-2.0

use super::super::Stats;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn part() -> Stats {
    Stats {
        missing_face_surface_kinds: BTreeMap::from([
            ("cone".into(), 1), ("plane".into(), 2), ("sphere".into(), 3),
        ]),
        ..Stats::default()
    }
}

#[test]
fn stats_kind_source_refuses_one_visit_before_any_entry_admission() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut target = Stats::default();
    let first = match target.merge(&ctx, part()) {
        Err(CodecError::ResourceLimit(first)) => first,
        other => panic!("expected kind source refusal: {other:?}"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "ASM merge loss kind traversal");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    assert!(target.missing_face_surface_kinds.is_empty());
    assert!(matches!(target.merge(&ctx, Stats::default()),
        Err(CodecError::ResourceLimit(last)) if last == first));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn stats_kind_node_work_refuses_after_one_source_visit_without_admitting_the_tail() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut target = Stats::default();
    let first = match target.merge(&ctx, part()) {
        Err(CodecError::ResourceLimit(first)) => first,
        other => panic!("expected actual kind node-work refusal: {other:?}"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "ASM merge loss kinds");
    // An empty tree adds one node: one shift pass plus two split passes.
    let alignment = std::mem::align_of::<String>()
        .max(std::mem::align_of::<usize>());
    let node_bytes = 11 * (std::mem::size_of::<String>() + std::mem::size_of::<usize>())
        + 16 * std::mem::size_of::<usize>() + 2 * alignment;
    let node_work = u64::try_from(3 * node_bytes).unwrap();
    assert_eq!((first.limit, first.used, first.additional), (1, 1, node_work));
    assert!(target.missing_face_surface_kinds.is_empty());
    assert!(matches!(target.merge(&ctx, Stats::default()),
        Err(CodecError::ResourceLimit(last)) if last == first));
}
