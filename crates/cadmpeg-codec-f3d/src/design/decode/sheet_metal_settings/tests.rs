// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn local_references_distinguish_null_present_and_malformed_frames() {
    assert_eq!(local(&mut View::over_retained(&[0])), Ok(None));
    let mut present = vec![1];
    present.extend_from_slice(&51u64.to_le_bytes());
    present.extend_from_slice(&[0, 0]);
    assert_eq!(local(&mut View::over_retained(&present)), Ok(Some(51)));
    for length in 0..present.len() {
        assert_eq!(local(&mut View::over_retained(&present[..length])), Err(()));
    }
    present[10] = 1;
    assert_eq!(local(&mut View::over_retained(&present)), Err(()));
    assert_eq!(local(&mut View::over_retained(&[2])), Err(()));
}

fn fixture(version: u32, rule_reference_tail: &[u8]) -> (Vec<u8>, PrimaryRecordFrame) {
    fn header(bytes: &mut Vec<u8>, tag: [u8; 3]) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&tag);
        bytes.extend_from_slice(&50u32.to_le_bytes());
    }
    fn reference(bytes: &mut Vec<u8>, index: u64) {
        bytes.push(1);
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    let mut bytes = Vec::new();
    header(&mut bytes, *b"256");
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(if version == 2 { &[0; 7] } else { &[0; 9] });
    reference(&mut bytes, 51);
    bytes.extend_from_slice(rule_reference_tail);
    reference(&mut bytes, 52);
    bytes.push(0);
    reference(&mut bytes, 53);
    let member_end = bytes.len();
    header(&mut bytes, *b"257");
    bytes.extend_from_slice(&[0; 8]);
    reference(&mut bytes, 54);
    let end = bytes.len();
    (
        bytes,
        PrimaryRecordFrame {
            entity_id: 50,
            start: 0,
            member_end,
            end,
        },
    )
}

#[test]
fn settings_frames_require_exact_boundary_version_flags_and_typed_links() {
    let primary = HashSet::from([50, 51, 52, 53, 54]);
    let types = HashMap::from([(51, (RULE_GUID, 0)), (52, (PRESENTATION_GUID, 0))]);
    for (version, tail) in [(2, &[0; 4][..]), (3, &[1, 0, 0][..]), (3, &[0, 0][..])] {
        let (bytes, frame) = fixture(version, tail);
        let registrations: Vec<_> =
            [(SETTINGS_GUID, version), (PAIRED_GUID, 0)]
                .into_iter()
                .enumerate()
                .map(
                    |(index, (guid, version))| crate::records::entity_header::SegmentTypeData {
                        byte_offset: u64_from_index(index),
                        type_guid: guid.to_owned().try_into().unwrap(),
                        type_guid_offset: 4,
                        base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                        version,
                        version_offset: 44,
                        module: "Fusion".into(),
                        entities: crate::records::identity::ReferenceRun::unlocated(
                            if index == 0 { vec![50] } else { Vec::new() },
                        ),
                    },
                )
                .collect();
        let fields =
            parse_settings(&bytes, frame, version, &primary, &types, &registrations).unwrap();
        assert_eq!(fields.mask, 4);
        assert_eq!(fields.inputs.version(), version);
        assert_eq!(
            fields.inputs.rule_reference_tail_width(),
            u64_from_index(tail.len())
        );
        if version == 3 {
            assert_eq!(
                fields.inputs,
                DesignSheetMetalSettingsInputs::V3 {
                    slots: [None; 9],
                    rule_reference_tail: if tail == [0, 0] {
                        V3RuleReferenceTail::ZeroPair
                    } else {
                        V3RuleReferenceTail::OneZeroPair
                    },
                }
            );
        }
        assert_eq!(
            (
                fields.rule,
                fields.presentation,
                fields.owner,
                fields.context
            ),
            (51, 52, 53, 54)
        );
        for offset in [
            4,
            frame.member_end + 4,
            11,
            25,
            25 + if version == 2 { 7 } else { 9 } + 11,
            frame.member_end + 11,
            frame.end - 1,
        ] {
            let mut invalid = bytes.clone();
            invalid[offset] = 2;
            assert!(
                parse_settings(&invalid, frame, version, &primary, &types, &registrations)
                    .is_none()
            );
        }
        assert!(parse_settings(
            &bytes,
            PrimaryRecordFrame {
                member_end: frame.member_end + 1,
                ..frame
            },
            version,
            &primary,
            &types,
            &registrations
        )
        .is_none());
        assert!(parse_settings(
            &bytes,
            frame,
            version,
            &HashSet::from([50, 51, 52, 53]),
            &types,
            &registrations
        )
        .is_none());
        let mut wrong_class = bytes.clone();
        wrong_class[6] = b'7';
        assert!(parse_settings(
            &wrong_class,
            frame,
            version,
            &primary,
            &types,
            &registrations
        )
        .is_none());
        let mut wrong_paired_class = bytes.clone();
        wrong_paired_class[frame.member_end + 6] = b'6';
        assert!(parse_settings(
            &wrong_paired_class,
            frame,
            version,
            &primary,
            &types,
            &registrations
        )
        .is_none());
        let wrong_types = HashMap::from([(51, (PRESENTATION_GUID, 0)), (52, (RULE_GUID, 0))]);
        assert!(parse_settings(
            &bytes,
            frame,
            version,
            &primary,
            &wrong_types,
            &registrations
        )
        .is_none());
        assert!(parse_settings(&bytes, frame, 4, &primary, &types, &registrations).is_none());
    }
}

#[test]
fn settings_decoder_skips_unregistered_bulk_without_requesting_a_metastream() {
    use std::io::{Cursor, Write};
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&[]).unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&bytes, |scan| {
        assert!(super::decode_settings(scan, &mut Vec::new())
            .unwrap()
            .is_empty());
    });
}

#[test]
fn settings_name_every_linked_record_for_header_retention() {
    let settings: crate::records::sheet_metal_settings::DesignSheetMetalSettings =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:Design1/BulkStream.dat:design-sheet-metal-settings#20", "record_index": 50,
            "byte_offset": 20, "paired_byte_offset": 100, "mask": 4,
            "inputs": {"version": "v2", "slots": [null, 55, null, null, null, null, null]},
            "rule_record_index": 51, "presentation_record_index": 52, "owner_record_index": 53,
            "context_record_index": 54,
        }))
        .unwrap();
    let indices = super::linked_record_indices(&[settings]);
    assert_eq!(
        indices,
        [50, 55, 51, 52, 53, 54].map(|index| ("f3d:Design1/BulkStream.dat".to_owned(), index))
    );
}

#[test]
fn rule_thickness_is_the_first_entry_named_thickness() {
    let rule = |name: &str, settings: Option<&[u64]>| {
        let mut bytes = vec![0xaa; 7];
        let start = bytes.len();
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(b"285");
        bytes.extend_from_slice(&273_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        if let Some(settings) = settings {
            bytes.extend_from_slice(&u32::try_from(settings.len()).unwrap().to_le_bytes());
            for target in settings {
                bytes.push(1);
                bytes.extend_from_slice(&target.to_le_bytes());
                bytes.extend_from_slice(&[0, 0]);
            }
        }
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
        for unit in name.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let thickness_offset = bytes.len();
        bytes.extend_from_slice(&0.25_f64.to_le_bytes());
        (bytes, start, thickness_offset)
    };
    let types = HashMap::from([(17, (SETTINGS_GUID, 2)), (18, (SETTINGS_GUID, 3))]);
    for (version, settings) in [(1, None), (0, Some([17, 18].as_slice()))] {
        let (bytes, start, thickness_offset) = rule("Thickness", settings);
        let root = View::over_retained(&bytes);
        let (thickness, offset) =
            rule_thickness(root.child(start, bytes.len()).unwrap(), version, &types).unwrap();
        assert_eq!(
            thickness,
            cadmpeg_ir::scalar::PositiveReal::new(0.25).unwrap()
        );
        assert_eq!(offset, u64_from_index(thickness_offset));
        let short = root.child(start, bytes.len() - 1).unwrap();
        assert!(rule_thickness(short, version, &types).is_none());
        let (other, start, _) = rule("KFactor00", settings);
        let record = View::over_retained(&other)
            .child(start, other.len())
            .unwrap();
        assert!(rule_thickness(record, version, &types).is_none());
    }
    let (bytes, start, _) = rule("Thickness", Some(&[17, 20]));
    let record = View::over_retained(&bytes)
        .child(start, bytes.len())
        .unwrap();
    assert!(rule_thickness(record, 0, &types).is_none());
}

#[test]
fn unreadable_settings_keep_the_sibling_settings() {
    use std::io::{Cursor, Write};
    for (version, tail, reference_prefix) in [(2, &[0; 4][..], false), (3, &[0; 2][..], true)] {
        let (mut first, mut first_frame) = fixture(version, tail);
        let (mut second, second_frame) = fixture(version, tail);
        second[7..11].copy_from_slice(&150u32.to_le_bytes());
        second[second_frame.member_end + 7..second_frame.member_end + 11]
            .copy_from_slice(&150u32.to_le_bytes());
        if reference_prefix {
            let mut prefix = vec![1];
            prefix.extend_from_slice(&51u64.to_le_bytes());
            prefix.extend_from_slice(&[0; 14]);
            prefix.push(1);
            prefix.extend_from_slice(&55u64.to_le_bytes());
            prefix.extend_from_slice(&[0; 4]);
            let prefix_start =
                crate::layout::indexed_design_record_header::LEN + settings_prefix::OVERRIDE_MASK;
            let replaced_end = prefix_start + 4 + 9 + 11 + 2;
            let added = prefix.len() - (replaced_end - prefix_start);
            first.splice(prefix_start..replaced_end, prefix);
            first_frame.member_end += added;
            first_frame.end += added;
        } else {
            first[11] = 2;
        }
        let second_start = first.len();
        first.extend_from_slice(&second);
        let mut primary = vec![(50, 0), (150, u64_from_index(second_start))];
        for entity in [51u64, 52, 53, 54]
            .into_iter()
            .chain(reference_prefix.then_some(55))
        {
            primary.push((entity, u64_from_index(first.len())));
            first.extend_from_slice(&3u32.to_le_bytes());
            first.extend_from_slice(b"258");
            first.extend_from_slice(&u32::try_from(entity).unwrap().to_le_bytes());
        }
        let mut meta = crate::test_support::streams_test::design_metastream_with_records(
            &[
                (SETTINGS_GUID, "", version, "Fusion", &[50, 150]),
                (PAIRED_GUID, "", 0, "Fusion", &[]),
                (RULE_GUID, "", 0, "Fusion", &[51]),
                (PRESENTATION_GUID, "", 0, "Fusion", &[52]),
            ],
            &primary,
        );
        let secondary_at = meta.len() - 20;
        let mut secondary = 2u32.to_le_bytes().to_vec();
        for (entity, offset) in [
            (50u64, first_frame.member_end),
            (150, second_start + second_frame.member_end),
        ] {
            secondary.extend_from_slice(&entity.to_le_bytes());
            secondary.extend_from_slice(&u64_from_index(offset).to_le_bytes());
        }
        meta.splice(secondary_at..secondary_at + 4, secondary);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(&first).unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(&meta).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        crate::test_support::zip_test::with_scan(&bytes, |scan| {
            let mut losses = Vec::new();
            let settings = super::decode_settings(scan, &mut losses).unwrap();
            assert_eq!(settings.len(), 1);
            assert_eq!(settings[0].record_index, 150);
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code,
                crate::loss::F3dLossCode::SheetMetalSettingsUndecoded.kind()
            );
        });
    }
}
