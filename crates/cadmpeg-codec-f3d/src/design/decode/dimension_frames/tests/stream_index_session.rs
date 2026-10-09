// SPDX-License-Identifier: Apache-2.0

use super::super::StreamIndexRuns;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

#[test]
fn stream_index_session_preserves_sorted_distinct_native_keys_and_releases_backing() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4096;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let storage;
    let (runs, held) = ctx.with_scoped_storage("test native stream index owner", || {
        StreamIndexRuns::new(&ctx, vec![
            ("f3d:z", 4), ("f3d:a", 9), ("f3d:a", 2), ("f3d:a", 9),
        ], "test native stream index owner")
    }).unwrap();
    storage = held;
    assert_eq!(runs.of("f3d:a").unwrap(), [2, 9]);
    assert_eq!(runs.of("f3d:z").unwrap(), [4]);
    assert!(runs.of("f3d:missing").unwrap().is_empty());
    drop(runs);
    drop(storage);
    drop(ctx.reserve_scoped(4096, "after native stream index destruction").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn stream_index_session_keeps_original_fuse_for_present_missing_and_empty_runs() {
    for keys in [vec![("f3d:a", 9)], Vec::new()] {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (other, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let storage;
        let (runs, held) = ctx.with_scoped_storage("test refused native stream index", || {
            StreamIndexRuns::new(&ctx, keys, "test refused native stream index")
        }).unwrap();
        storage = held;
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(
            policy.limits.max_work_units + 1, "original native stream index session refusal",
        ) else { panic!("creating session must refuse excess work"); };
        for query in ["f3d:a", "f3d:missing"] {
            assert!(matches!(runs.of(query),
                Err(CodecError::ResourceLimit(later)) if later == original));
            assert_eq!(ctx.resource_refusal(), Some(original.clone()));
            assert!(other.resource_refusal().is_none());
            other.charge_work(0, "unrelated stream index session remains healthy").unwrap();
        }
        drop(runs);
        drop(storage);
        other.finish_session().unwrap();
    }
}

#[test]
fn stream_index_session_empty_runs_are_free_then_observe_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let runs = StreamIndexRuns::new(&ctx, Vec::new(), "test empty native stream index").unwrap();
    assert!(runs.of("f3d:missing").unwrap().is_empty());
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original empty stream index refusal")
    else { panic!("creating zero-work session must refuse excess work"); };
    assert!(matches!(runs.of("f3d:missing"),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(StreamIndexRuns::new(&ctx, Vec::new(), "test empty refused construction"),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(runs);
}
