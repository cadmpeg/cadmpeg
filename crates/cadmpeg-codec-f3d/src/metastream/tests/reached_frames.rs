// SPDX-License-Identifier: Apache-2.0
//! Reached primary/secondary frame sources and original refusals.

use super::{one_primary_frame, MetaStream, RecordIndexEntry};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn work_before_invalid_frame(meta: &MetaStream, expected: &str) -> u64 {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(matches!(super::super::primary_record_frames(ctx, meta, 14),
            Err(CodecError::Malformed(message)) if message == expected));
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(limit)) =
            ctx.charge_work(u64::MAX, "measure F3D rejected primary frame")
        else { panic!("actual frame work measurement"); };
        limit.used
    })
}

#[test]
fn invalid_first_primary_frame_does_not_visit_unread_index_tail() {
    let mut short = one_primary_frame(false);
    short.records[0].bulk_offset = 15;
    let mut long = short.clone();
    // The first row has the same invalid start15/end14 in both inputs.
    long.records.extend((8..4104).map(|entity_id| RecordIndexEntry {
        entity_id, bulk_offset: 14,
    }));
    let message = "F3D primary record extents are not nonempty and strictly increasing within the BulkStream";
    assert_eq!(work_before_invalid_frame(&short, message), work_before_invalid_frame(&long, message));
}

#[test]
fn invalid_first_secondary_frame_does_not_visit_unread_index_tail() {
    let mut short = one_primary_frame(true);
    short.secondary_records[0].bulk_offset = 0;
    let mut long = short.clone();
    long.secondary_records.extend((8..4104).map(|entity_id| RecordIndexEntry {
        entity_id, bulk_offset: 7,
    }));
    let message = "F3D secondary record offset is not strictly inside its primary record";
    assert_eq!(work_before_invalid_frame(&short, message), work_before_invalid_frame(&long, message));
}

#[test]
fn known_empty_primary_frame_sources_are_free_and_keep_original_fuse() {
    let empty = MetaStream { types: Vec::new(), records: Vec::new(), secondary_records: Vec::new() };
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(super::super::primary_record_frames(ctx, &empty, 0).unwrap().is_empty());
        let Err(CodecError::ResourceLimit(original)) =
            super::super::primary_record_frames(ctx, &one_primary_frame(false), 14)
        else { panic!("actual frame source refusal"); };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "frame F3D primary records");
        assert_eq!(original.used, 0);
        assert_eq!(original.additional, 1);
        let Err(CodecError::ResourceLimit(sticky)) =
            super::super::primary_record_frames(ctx, &empty, 0)
        else { panic!("sticky frame refusal"); };
        assert_eq!(sticky, original);
    });
}

#[test]
fn reached_secondary_source_keeps_active_original_refusal() {
    let meta = one_primary_frame(true);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let frames = super::super::primary_record_frames(ctx, &meta, 14).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!((frames[0].entity_id, frames[0].start, frames[0].member_end, frames[0].end), (7, 0, 7, 14));
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "index F3D secondary entities", Some(1));
        let Err(CodecError::ResourceLimit(original)) =
            super::super::primary_record_frames(ctx, &meta, 14)
        else { panic!("actual secondary source refusal"); };
        assert_eq!(original.additional, 1);
        assert_eq!(original.operation, "index F3D secondary entities");
        let Err(CodecError::ResourceLimit(sticky)) =
            super::super::primary_record_frames(ctx, &meta, 14)
        else { panic!("sticky secondary source refusal"); };
        assert_eq!(sticky, original);
    });
}
