// SPDX-License-Identifier: Apache-2.0

use super::super::PrimaryFrames;
use crate::metastream::{MetaStream, PrimaryRecordFrame, RecordIndexEntry};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn primary_frame_session_preserves_native_extents_and_missing_lookup() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let metadata = MetaStream {
        types: Vec::new(),
        records: vec![
            RecordIndexEntry { entity_id: 9, bulk_offset: 0 },
            RecordIndexEntry { entity_id: 2, bulk_offset: 40 },
        ],
        secondary_records: vec![RecordIndexEntry { entity_id: 9, bulk_offset: 25 }],
    };
    let index = PrimaryFrames::index(&ctx, &metadata, 70).unwrap();
    assert_eq!(index.frame(9).unwrap(), Some(PrimaryRecordFrame {
        entity_id: 9, start: 0, member_end: 25, end: 40,
    }));
    assert_eq!(index.frame(2).unwrap(), Some(PrimaryRecordFrame {
        entity_id: 2, start: 40, member_end: 70, end: 70,
    }));
    assert_eq!(index.frame(8).unwrap(), None);
    assert_eq!(metadata.records[0].entity_id, 9);
    assert_eq!(metadata.records[1].entity_id, 2);
    drop(index);
    drop(ctx.reserve_scoped(1024, "after primary frame index destruction").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn primary_frame_session_preserves_original_refusal_on_present_missing_and_empty() {
    for records in [
        vec![RecordIndexEntry { entity_id: 9, bulk_offset: 0 }],
        Vec::new(),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1024;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (other, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let metadata = MetaStream { types: Vec::new(), records, secondary_records: Vec::new() };
        let index = PrimaryFrames::index(&ctx, &metadata, 40).unwrap();
        let Err(CodecError::ResourceLimit(original)) =
            ctx.charge_work(1025, "original primary frame session refusal") else {
                panic!("the original session must refuse work beyond its ceiling");
            };
        for entity in [9, 8] {
            let Err(CodecError::ResourceLimit(later)) = index.frame(entity) else {
                panic!("the primary index must keep its creating session refusal");
            };
            assert_eq!(later, original);
            assert_eq!(later.dimension, ResourceDimension::WorkUnits);
            assert_eq!(ctx.resource_refusal(), Some(original.clone()));
            assert_eq!(other.resource_refusal(), None);
            other.charge_work(0, "unrelated primary frame session remains healthy").unwrap();
        }
        drop(index);
        other.finish_session().unwrap();
    }
}

#[test]
fn primary_frame_session_empty_index_releases_scratch_without_native_payload() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let metadata = MetaStream { types: Vec::new(), records: Vec::new(), secondary_records: Vec::new() };
    let index = PrimaryFrames::index(&ctx, &metadata, 0).unwrap();
    assert_eq!(index.frame(9).unwrap(), None);
    drop(index);
    ctx.finish_session().unwrap();
}
