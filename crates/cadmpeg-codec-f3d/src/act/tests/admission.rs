// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

#[test]
fn act_table_readers_admit_only_the_first_invalid_row() {
    for (field, width, operation, detail) in [
        (2, 15, "scan F3D ACT table entries", "entry reference"),
        (6, 11, "scan F3D ACT table references", "table reference"),
        (
            10,
            81,
            "scan F3D ACT registry channels",
            "channel-registry name",
        ),
    ] {
        let mut bytes = vec![0; field + 4];
        bytes[field..field + 4].copy_from_slice(&128u32.to_le_bytes());
        bytes.resize(bytes.len() + 128 * width, 0);
        let frame = super::act_table_frame(bytes.len());
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| super::super::decode_table(ctx, &bytes, &frame, 0, "synthetic").map(|_| ()),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1));
        crate::test_support::with_decode_context(|ctx| {
            let Err(error) = super::super::decode_table(ctx, &bytes, &frame, 0, "synthetic") else {
                panic!("invalid first row must fail");
            };
            let expected = format!("invalid F3D ACTTable {detail}: synthetic@0");
            assert!(matches!(error, CodecError::Malformed(message) if message == expected));
        });
    }
}

#[test]
fn act_frame_validation_admits_only_the_first_invalid_extent() {
    use crate::metastream::{MetaStream, RecordIndexEntry};
    let meta = MetaStream {
        types: Vec::new(),
        records: (1..=128)
            .map(|index| RecordIndexEntry {
                entity_id: index,
                bulk_offset: index,
            })
            .collect(),
        secondary_records: Vec::new(),
    };
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "scan F3D ACT record frames",
        0,
        |ctx| super::super::decode_record_frames(ctx, &[], &meta, "synthetic").map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1));
    crate::test_support::with_decode_context(|ctx| {
        assert!(
            matches!(super::super::decode_record_frames(ctx, &[], &meta, "synthetic"), Err(CodecError::Malformed(message)) if message == "F3D ACT record extent is outside its BulkStream: synthetic")
        );
    });
}

#[test]
fn nonroot_component_link_releases_candidate_text() {
    use super::super::{decode_component_link, ComponentLink, RecordFrame};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut bytes = vec![0; 10];
    bytes.push(1);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    crate::test_support::lp_utf16(&mut bytes, "1_1");
    bytes.push(1);
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    bytes.push(1);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    crate::test_support::lp_utf16(&mut bytes, "component display");
    bytes.push(0);
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let frame = RecordFrame {
        start: 0,
        end: bytes.len(),
        record_index: 1,
        record_index_offset: 0,
        payload_offset: 0,
        class_tag: "123".to_owned().try_into().unwrap(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        decode_component_link(&ctx, &bytes, &frame, "synthetic").unwrap(),
        Some(ComponentLink::NonRoot)
    ));
    ctx.finish_session().unwrap();
}

#[test]
fn act_merge_duplicates_do_not_admit_the_unread_tail() {
    for groups in [false, true] {
        let operation = if groups {
            "scan F3D ACT groups for merge"
        } else {
            "scan F3D ACT table entries for merge"
        };
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            1,
            |ctx| {
                let table = if groups {
                    Vec::new()
                } else {
                    (0..128).map(|_| super::table_entry("0_985")).collect()
                };
                if groups {
                    crate::test_support::with_decode_context(|fixture_ctx| {
                        let rows = (0..128)
                            .map(|_| super::channel_group(fixture_ctx, "0_985"))
                            .collect();
                        super::merge_entities(ctx, "synthetic", table, rows).map(|_| ())
                    })
                } else {
                    super::merge_entities(ctx, "synthetic", table, Vec::new()).map(|_| ())
                }
            },
        );
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation && limit.additional == 1)
        );
    }
}

#[test]
fn act_channel_group_invalid_first_entry_does_not_admit_declared_tail() {
    let (mut bytes, frame, _, _) = super::channel_group_frame();
    let count_at = frame.payload_offset + 10;
    bytes[count_at..count_at + 4].copy_from_slice(&8u32.to_le_bytes());
    bytes[count_at + 4..count_at + 8].copy_from_slice(&0u32.to_le_bytes());
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "scan F3D ACT channel group entries",
        0,
        |ctx| super::decode_channel_group(ctx, &bytes, &frame, "synthetic").map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1));
    crate::test_support::with_decode_context(|ctx| {
        assert!(
            super::decode_channel_group(ctx, &bytes, &frame, "synthetic")
                .unwrap()
                .is_none()
        );
    });
}
