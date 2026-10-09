// SPDX-License-Identifier: Apache-2.0

use super::super::primary_record_frames;
use super::{one_primary_frame, MetaStream, RecordIndexEntry};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

#[test]
fn metastream_index_scratch_releases_after_success_with_native_frames_live() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let frames = primary_record_frames(&ctx, &one_primary_frame(true), 14).unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!((frames[0].entity_id, frames[0].start, frames[0].member_end, frames[0].end), (7, 0, 7, 14));
    drop(ctx.reserve_scoped(1024, "after primary entity index destruction").unwrap());
    assert_eq!((frames[0].entity_id, frames[0].start, frames[0].member_end, frames[0].end), (7, 0, 7, 14));
    drop(frames);
    ctx.finish_session().unwrap();
}

#[test]
fn metastream_index_scratch_releases_after_duplicate_primary_and_secondary() {
    for secondary in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 1024;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut metadata = one_primary_frame(secondary);
        if secondary {
            metadata.secondary_records.push(RecordIndexEntry { entity_id: 7, bulk_offset: 8 });
        } else {
            metadata.records.push(RecordIndexEntry { entity_id: 7, bulk_offset: 8 });
        }
        let error = primary_record_frames(&ctx, &metadata, 14).unwrap_err();
        let CodecError::Malformed(message) = error else { panic!("a repeated native entity must remain malformed"); };
        assert_eq!(message, if secondary { "F3D secondary record index repeats an entity ID" } else { "F3D primary record index repeats an entity ID" });
        drop(ctx.reserve_scoped(1024, "after rejected primary entity indexes").unwrap());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn metastream_index_empty_frame_has_no_scratch_or_retained_allocation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let metadata = MetaStream { types: Vec::new(), records: Vec::new(), secondary_records: Vec::new() };
    assert!(primary_record_frames(&ctx, &metadata, 0).unwrap().is_empty());
    ctx.finish_session().unwrap();
}
