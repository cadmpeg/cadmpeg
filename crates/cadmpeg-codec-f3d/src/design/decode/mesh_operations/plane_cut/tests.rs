use super::*;

fn reference(target: u32) -> [u8; 11] {
    let mut bytes = [0; 11];
    bytes[0] = 1;
    bytes[1..5].copy_from_slice(&target.to_le_bytes());
    bytes
}

fn scope(flagged: bool) -> Vec<u8> {
    let mut bytes = vec![0; 21];
    if flagged {
        bytes[20] = 1;
        bytes.extend_from_slice(&43u32.to_le_bytes());
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&16u32.to_le_bytes());
    for value in [
        0.0f64, -1.0, 0.0, 2.5, 1.0, 0.0, 0.0, -3.25, 0.0, 0.0, 2.0, 7.5, 0.0, 0.0, 0.0, 1.0,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.push(1);
    for target in [81, 82, 83, 84, 85] {
        bytes.extend_from_slice(&reference(target));
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&reference(81));
    bytes.push(0);
    bytes
}

#[test]
fn both_prefixes_preserve_plane_bits_and_distinct_input_output_roles() {
    for flagged in [false, true] {
        let bytes = scope(flagged);
        let cut = read_scope(View::over_retained(&bytes));
        assert_eq!(cut.flag, Some(u8::from(flagged)));
        assert_eq!(cut.prefix_word, flagged.then_some(43));
        assert_eq!(cut.mode, Some(1));
        assert_eq!(cut.matrix_count, Some(16));
        assert_eq!(cut.matrix[7], Some((-3.25f64).to_bits()));
        assert_eq!(cut.fill, Some(2));
        assert_eq!(cut.side, Some(1));
        assert_eq!(
            cut.references.map(|value| value.unwrap()),
            [
                PlaneCutReference::Local(81),
                PlaneCutReference::Local(82),
                PlaneCutReference::Local(83),
                PlaneCutReference::Local(84),
                PlaneCutReference::Local(85),
                PlaneCutReference::Local(81)
            ]
        );
        assert_eq!(cut.tail_word, Some(0));
        assert_eq!(cut.tail_byte, Some(0));
        assert_eq!(cut.framing, None);
    }
}

#[test]
fn every_truncation_retains_only_complete_preceding_fields() {
    for flagged in [false, true] {
        let bytes = scope(flagged);
        let shift = if flagged { 4 } else { 0 };
        for end in 0..bytes.len() {
            let cut = read_scope(View::over_retained(&bytes[..end]));
            assert!(cut.framing.is_some(), "end {end}");
            assert_eq!(cut.mode, (end >= 25 + shift).then_some(1));
            assert_eq!(cut.matrix_count, (end >= 29 + shift).then_some(16));
            for (index, value) in cut.matrix.iter().enumerate() {
                assert_eq!(value.is_some(), end >= 37 + shift + index * 8);
            }
            assert_eq!(cut.fill, (end >= 161 + shift).then_some(2));
            assert_eq!(cut.side, (end >= 162 + shift).then_some(1));
            for (index, boundary) in [173, 184, 195, 206, 217, 232].into_iter().enumerate() {
                assert_eq!(cut.references[index].is_some(), end >= boundary + shift);
            }
            assert_eq!(cut.tail_word, (end >= 221 + shift).then_some(0));
            assert_eq!(cut.tail_byte, None);
        }
    }
}

#[test]
fn semantic_invalidity_preserves_later_fields_and_all_numeric_bits() {
    let mut bytes = scope(false);
    bytes[11] = 6;
    bytes[21..25].copy_from_slice(&97u32.to_le_bytes());
    bytes[29..37].copy_from_slice(&0x7ff8_0000_0000_0123u64.to_le_bytes());
    bytes[157..161].copy_from_slice(&98u32.to_le_bytes());
    bytes[161] = 99;
    let cut = read_scope(View::over_retained(&bytes));
    assert_eq!(cut.mode, Some(97));
    assert_eq!(cut.fill, Some(98));
    assert_eq!(cut.side, Some(99));
    assert_eq!(cut.matrix[0], Some(0x7ff8_0000_0000_0123));
    assert_eq!(cut.references[5], Some(PlaneCutReference::Local(81)));
    assert_eq!(cut.framing, None);
    bytes[25..29].copy_from_slice(&17u32.to_le_bytes());
    let cut = read_scope(View::over_retained(&bytes));
    assert_eq!(cut.matrix_count, Some(17));
    assert_eq!(cut.matrix, [None; 16]);
    assert_eq!(cut.fill, None);
}

#[test]
fn null_typed_and_other_references_keep_their_envelopes_and_tail() {
    let mut bytes = scope(false);
    bytes.splice(173..184, [0]);
    let cut = read_scope(View::over_retained(&bytes));
    assert_eq!(cut.references[1], Some(PlaneCutReference::Null));
    assert_eq!(cut.references[2], Some(PlaneCutReference::Local(83)));
    assert_eq!(cut.tail_byte, Some(0));
    let mut bytes = reference(91)[..9].to_vec();
    bytes.extend_from_slice(&36u32.to_le_bytes());
    bytes.extend_from_slice(b"22975A91-16AD-4CC9-A5E4-B4F8B4C27881");
    bytes.extend_from_slice(&[0, 0]);
    let mut cursor = Cursor(View::over_retained(&bytes));
    assert_eq!(
        cursor.reference().unwrap(),
        PlaneCutReference::TypedLocal {
            target: 91,
            inline_type: "22975A91-16AD-4CC9-A5E4-B4F8B4C27881"
        }
    );
    let bytes = reference(0);
    let mut cursor = Cursor(View::over_retained(&bytes));
    assert_eq!(
        cursor.reference().unwrap(),
        PlaneCutReference::Other(&bytes)
    );
    let mut bytes = scope(false);
    bytes[20] = 7;
    let cut = read_scope(View::over_retained(&bytes));
    assert_eq!(cut.flag, Some(7));
    assert_eq!(cut.mode, None);
}

#[test]
fn admission_checks_guid_base_module_version_identity_bounds_and_uniqueness() {
    use crate::metastream::{MetaStream, RecordIndexEntry};
    use crate::records::entity_header::{BaseTypeGuid, SegmentTypeData};
    use crate::records::identity::ReferenceRun;
    let mut bytes = scope(false);
    bytes[..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"256");
    bytes[7..11].copy_from_slice(&17u32.to_le_bytes());
    let meta = MetaStream {
        types: vec![SegmentTypeData {
            byte_offset: 0,
            type_guid: "E3EB3760-4362-48D6-ABF3-698B0C12C5A9"
                .to_owned()
                .try_into()
                .unwrap(),
            type_guid_offset: 0,
            base_type_guid: BaseTypeGuid::Guid {
                value: "90B8D2C6-05BB-4D0F-89EF-83AAC46CA1FF"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                offset: 0,
            },
            version: 1,
            version_offset: 0,
            module: "Fusion".into(),
            entities: ReferenceRun::unlocated(vec![17]),
        }],
        records: vec![RecordIndexEntry {
            entity_id: 17,
            bulk_offset: 0,
        }],
        secondary_records: Vec::new(),
    };
    for case in 0..9 {
        let mut meta = meta.clone();
        match case {
            1 => meta.types[0].version = 2,
            2 => meta.types[0].module = "Other".into(),
            3 => {
                meta.types[0].type_guid = "00000000-0000-0000-0000-000000000000"
                    .to_owned()
                    .try_into()
                    .unwrap();
            }
            4 => {
                meta.types[0].base_type_guid = BaseTypeGuid::Guid {
                    value: "00000000-0000-0000-0000-000000000000"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    offset: 0,
                }
            }
            5 => meta.types[0].entities = ReferenceRun::unlocated(vec![18]),
            6 => meta.records.push(meta.records[0]),
            7 => meta.records[0].bulk_offset = 1,
            8 => meta.records[0].entity_id = 18,
            _ => (),
        }
        let records =
            MeshOperationRecords::index(View::over_retained(&bytes), std::sync::Arc::new(meta));
        assert_eq!(
            records.plane_cut_scope(17).is_ok(),
            case == 0,
            "case {case}"
        );
    }
}
