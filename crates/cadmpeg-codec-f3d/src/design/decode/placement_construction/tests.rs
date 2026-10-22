// SPDX-License-Identifier: Apache-2.0
use crate::records::placement_construction::DesignPlacementConstruction;
use cadmpeg_core::decode::View;
use cadmpeg_ir::native::bytes::NativeBytes;

#[test]
fn placement_source_frames_preserve_versioned_members_and_absolute_offsets() {
    crate::test_support::with_decode_context(|ctx| {
        for fixture in crate::test_support::placement_construction::FRAMES {
            let fixture: serde_json::Value = serde_json::from_str(fixture).unwrap();
            let raw: NativeBytes<Vec<u8>> =
                serde_json::from_value(fixture["source_bytes"].clone()).unwrap();
            let expected: DesignPlacementConstruction =
                serde_json::from_value(fixture["native"].clone()).unwrap();
            let mut bytes = vec![0; 17];
            bytes.extend_from_slice(&raw);
            let view = View::over_retained(&bytes).child(17, bytes.len()).unwrap();
            let record = super::decode_frame(ctx, expected.id.clone(), view, expected.type_version)
                .unwrap()
                .unwrap();
            assert_eq!(record, expected);
            assert!(super::decode_frame(ctx, record.id.clone(), view, 99)
                .unwrap()
                .is_none());
            let truncated = View::over_retained(&bytes)
                .child(17, usize::try_from(record.fields.ref_type.offset).unwrap())
                .unwrap();
            assert!(
                super::decode_frame(ctx, record.id, truncated, record.type_version)
                    .unwrap()
                    .is_none()
            );
        }
    });
}

#[test]
fn placement_registration_requires_semantic_identity_matching_headers_and_a_paired_bound() {
    use crate::test_support::placement_construction::{metadata, FRAMES};
    let fixture: serde_json::Value = serde_json::from_str(FRAMES[0]).unwrap();
    let raw: NativeBytes<Vec<u8>> =
        serde_json::from_value(fixture["source_bytes"].clone()).unwrap();
    let expected: DesignPlacementConstruction =
        serde_json::from_value(fixture["native"].clone()).unwrap();
    let mut bytes = vec![0; 17];
    bytes.extend_from_slice(&raw);
    bytes[21..24].copy_from_slice(b"256");
    let member_end = bytes.len();
    crate::test_support::indexed_header(
        &mut bytes,
        *b"257",
        u32::try_from(expected.record_index).unwrap(),
    );
    let frame = crate::metastream::PrimaryRecordFrame {
        entity_id: expected.record_index,
        start: 17,
        member_end,
        end: bytes.len(),
    };
    for (version, type_guid, base_guid, secondary, wanted, unparsed) in [
        (14, super::TYPE_GUID, super::BASE_GUID, true, 1, 0),
        (99, super::TYPE_GUID, super::BASE_GUID, true, 0, 1),
        (14, super::BASE_GUID, super::BASE_GUID, true, 0, 0),
        (
            14,
            super::TYPE_GUID,
            "6144EA59-3ACA-4172-9F14-A6531E94D69B",
            true,
            0,
            1,
        ),
        (14, super::TYPE_GUID, super::BASE_GUID, false, 0, 1),
    ] {
        let meta = metadata(frame, version, type_guid, base_guid, secondary);
        let archive = crate::test_support::body_reference_tail_test::archive(&bytes, &meta);
        crate::test_support::with_decode_context(|ctx| {
            crate::test_support::zip_test::with_scan(&archive, |scan| {
                let decoded = super::decode(ctx, scan).unwrap();
                assert_eq!(decoded.records.len(), wanted);
                assert_eq!(decoded.unparsed, unparsed);
            })
        });
    }
    for (offset, corruption) in [
        (21, b"257".as_slice()),
        (24, &[0, 0, 0, 0][..]),
        (member_end, &[2, 0, 0, 0][..]),
        (member_end + 4, b"bad".as_slice()),
        (member_end + 7, &[0, 0, 0, 0][..]),
    ] {
        let mut corrupt = bytes.clone();
        corrupt[offset..offset + corruption.len()].copy_from_slice(corruption);
        let meta = metadata(frame, 14, super::TYPE_GUID, super::BASE_GUID, true);
        let archive = crate::test_support::body_reference_tail_test::archive(&corrupt, &meta);
        crate::test_support::with_decode_context(|ctx| {
            crate::test_support::zip_test::with_scan(&archive, |scan| {
                let decoded = super::decode(ctx, scan).unwrap();
                assert!(decoded.records.is_empty());
                assert_eq!(decoded.unparsed, 1);
            })
        });
    }
}

#[test]
fn placement_reader_refuses_invalid_identity_and_unbounded_reference_counts() {
    let fixture: serde_json::Value =
        serde_json::from_str(crate::test_support::placement_construction::FRAMES[0]).unwrap();
    let raw: NativeBytes<Vec<u8>> =
        serde_json::from_value(fixture["source_bytes"].clone()).unwrap();
    let expected: DesignPlacementConstruction =
        serde_json::from_value(fixture["native"].clone()).unwrap();
    let mut source = vec![0; 17];
    source.extend_from_slice(&raw);
    let identity = usize::try_from(expected.fields.identity.as_ref().unwrap().offset).unwrap();
    let counts = [
        expected
            .fields
            .edge_references
            .as_ref()
            .unwrap()
            .count
            .offset,
        expected.fields.plane_references.count.offset,
        expected.fields.values.count.offset,
    ];
    crate::test_support::with_decode_context(|ctx| {
        let mut invalid = source.clone();
        invalid[identity] = 2;
        let view = View::over_retained(&invalid)
            .child(17, invalid.len())
            .unwrap();
        assert!(
            super::decode_frame(ctx, expected.id.clone(), view, expected.type_version)
                .unwrap()
                .is_none()
        );
        for count in counts {
            let mut invalid = source.clone();
            let offset = usize::try_from(count).unwrap();
            invalid[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            let view = View::over_retained(&invalid)
                .child(17, invalid.len())
                .unwrap();
            assert!(
                super::decode_frame(ctx, expected.id.clone(), view, expected.type_version)
                    .unwrap()
                    .is_none()
            );
        }
    });
}
