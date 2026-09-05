// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

use std::io::{Cursor, Write};

use zip::CompressionMethod;

use crate::test_support::*;

#[test]
fn component_naming_space_binds_component_entity_to_context_uuid() {
    const COMPONENT_TYPE_GUID: &str = "11111111-2222-3333-4444-555555555555";
    const CONTEXT_UUID: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

    fn archive(bulk: &[u8]) -> Vec<u8> {
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        let meta = design_metastream_with_records(
            &[(
                COMPONENT_TYPE_GUID,
                "21F379C8-CAFD-4985-B461-767673A4C502",
                0,
                "Component",
                &[17],
            )],
            &[],
        );
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(bulk).unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(&meta).unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn binding(out: &mut Vec<u8>, component: u64, reserved_len: usize, context_uuid: &str) {
        out.push(1);
        out.extend_from_slice(&component.to_le_bytes());
        out.extend(std::iter::repeat_n(0, reserved_len));
        out.extend_from_slice(&36_u32.to_le_bytes());
        for code_unit in context_uuid.encode_utf16() {
            out.extend_from_slice(&code_unit.to_le_bytes());
        }
    }

    fn typed_binding(out: &mut Vec<u8>, component: u64, context_uuid: &str) {
        out.push(1);
        out.extend_from_slice(&component.to_le_bytes());
        out.extend_from_slice(&(COMPONENT_TYPE_GUID.len() as u32).to_le_bytes());
        out.extend_from_slice(COMPONENT_TYPE_GUID.as_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&36_u32.to_le_bytes());
        for code_unit in context_uuid.encode_utf16() {
            out.extend_from_slice(&code_unit.to_le_bytes());
        }
    }

    for reserved_len in [2, 3] {
        let mut bulk = vec![0xaa, 0xbb];
        let marker = bulk.len();
        binding(&mut bulk, 17, reserved_len, CONTEXT_UUID);
        let decoded = with_scan(&archive(&bulk), |scan| {
            crate::design::decode::meta::decode_component_naming_spaces(scan)
        })
        .expect("component naming space");
        let [space] = decoded.as_slice() else {
            panic!("expected one component naming space");
        };
        assert_eq!(space.component_record_index, 17);
        assert_eq!(space.context_uuid, CONTEXT_UUID);
        assert_eq!(space.byte_offset, marker as u64);
        assert_eq!(
            space.context_uuid_offset,
            (marker + 9 + reserved_len) as u64
        );
    }

    let mut typed = vec![0xaa, 0xbb];
    let typed_marker = typed.len();
    typed_binding(&mut typed, 17, CONTEXT_UUID);
    let decoded = with_scan(&archive(&typed), |scan| {
        crate::design::decode::meta::decode_component_naming_spaces(scan)
    })
    .expect("typed component naming space");
    let [space] = decoded.as_slice() else {
        panic!("expected one typed component naming space");
    };
    assert_eq!(space.component_record_index, 17);
    assert_eq!(space.context_uuid, CONTEXT_UUID);
    assert_eq!(space.byte_offset, typed_marker as u64);

    let mut overlapping_reference = vec![1];
    binding(
        &mut overlapping_reference,
        17,
        2,
        "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb",
    );
    typed_binding(&mut overlapping_reference, 17, CONTEXT_UUID);
    let decoded = with_scan(&archive(&overlapping_reference), |scan| {
        crate::design::decode::meta::decode_component_naming_spaces(scan)
    })
    .expect("typed binding beside an overlapping 01 01 reference");
    let [space] = decoded.as_slice() else {
        panic!("expected one component naming space");
    };
    assert_eq!(space.context_uuid, CONTEXT_UUID);

    let mut conflicting = Vec::new();
    binding(&mut conflicting, 17, 3, CONTEXT_UUID);
    binding(
        &mut conflicting,
        17,
        3,
        "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb",
    );
    let error = with_scan(&archive(&conflicting), |scan| {
        crate::design::decode::meta::decode_component_naming_spaces(scan)
    })
    .expect_err("conflicting component UUIDs must be rejected");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn design_feature_timeline_versions_share_variable_width_local_references() {
    const INLINE_TYPE_GUID: &str = "11111111-2222-3333-4444-555555555555";

    fn lp_ascii(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    fn local_reference(out: &mut Vec<u8>, target: u64, inline_type: bool) {
        out.push(1);
        out.extend_from_slice(&target.to_le_bytes());
        if inline_type {
            lp_ascii(out, INLINE_TYPE_GUID);
        }
        out.extend_from_slice(&[0, 0]);
    }
    fn archive(meta: &[u8], bulk: &[u8]) -> Vec<u8> {
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(bulk).unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(meta).unwrap();
        zip.finish().unwrap().into_inner()
    }

    let mut bulk = Vec::new();
    lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&35_u64.to_le_bytes());
    lp_ascii(&mut bulk, "Timeline");
    bulk.extend_from_slice(&[0, 0]);
    local_reference(&mut bulk, 17, false);
    bulk.extend_from_slice(&2_u32.to_le_bytes());
    local_reference(&mut bulk, 101, false);
    local_reference(&mut bulk, 102, true);
    for version in crate::design::decode::meta::FEATURE_TIMELINE_TYPE_VERSIONS {
        let meta = design_metastream_with_records(
            &[
                (
                    crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
                    crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID,
                    version,
                    "Fusion",
                    &[35],
                ),
                (INLINE_TYPE_GUID, "", 0, "Fusion", &[17, 101, 102]),
            ],
            &[(35, 0)],
        );
        let decoded = with_scan(&archive(&meta, &bulk), |scan| {
            crate::design::decode::meta::decode_feature_timelines(scan)
        })
        .expect("exact feature timeline");
        let [timeline] = decoded.as_slice() else {
            panic!("expected one timeline record");
        };
        assert_eq!(timeline.record_index, 35);
        assert_eq!(timeline.context_record_index, 17);
        assert_eq!(timeline.items.iter().map(|item| item.value).collect::<Vec<_>>(), [101, 102]);
        assert_eq!(timeline.frame_length, bulk.len() as u64);
        for item in &timeline.items
        {
            assert_eq!(
                u64::from_le_bytes(
                    bulk[item.offset as usize..item.offset as usize + 8]
                        .try_into()
                        .expect("timeline target")
                ),
                item.value
            );
        }

        let mut duplicate = bulk.clone();
        let second_offset = timeline.items[1].offset as usize;
        duplicate[second_offset..second_offset + 8].copy_from_slice(&101_u64.to_le_bytes());
        let error = with_scan(&archive(&meta, &duplicate), |scan| {
            crate::design::decode::meta::decode_feature_timelines(scan)
        })
        .expect_err("duplicate timeline items must be rejected");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));

        let mut mismatched_inline_type = bulk.clone();
        let inline_type_at = mismatched_inline_type
            .windows(INLINE_TYPE_GUID.len())
            .position(|window| window == INLINE_TYPE_GUID.as_bytes())
            .expect("inline type GUID");
        mismatched_inline_type[inline_type_at] = b'2';
        let error = with_scan(&archive(&meta, &mismatched_inline_type), |scan| {
            crate::design::decode::meta::decode_feature_timelines(scan)
        })
        .expect_err("an inline type GUID must match the target registration");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }

    let unsupported_meta = design_metastream_with_records(
        &[(
            crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
            crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID,
            4,
            "Fusion",
            &[35],
        )],
        &[(35, 0)],
    );
    let error = with_scan(&archive(&unsupported_meta, &bulk), |scan| {
        crate::design::decode::meta::decode_feature_timelines(scan)
    })
    .expect_err("an unsupported timeline version must not use a known frame speculatively");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));

    for (base_type_guid, module) in [
        ("22222222-3333-4444-5555-666666666666", "Fusion"),
        (
            crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID,
            "Other",
        ),
    ] {
        let incompatible_meta = design_metastream_with_records(
            &[(
                crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
                base_type_guid,
                crate::design::decode::meta::FEATURE_TIMELINE_TYPE_VERSIONS[1],
                module,
                &[35],
            )],
            &[(35, 0)],
        );
        let error = with_scan(&archive(&incompatible_meta, &bulk), |scan| {
            crate::design::decode::meta::decode_feature_timelines(scan)
        })
        .expect_err("incompatible timeline registration metadata must be rejected");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}
