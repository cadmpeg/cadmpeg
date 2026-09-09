// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use crate::container;
use crate::parasolid::{self, StreamKind};
use crate::test_support::*;

#[test]
fn legacy_stream_boundaries_require_complete_transmit_headers() {
    let mut bytes = b"prefix".to_vec();
    bytes.extend_from_slice(b"PS\x00\x00not a header");
    let first = bytes.len();
    let first_description = b": TRANSMIT FILE (partition) created by test";
    bytes.extend_from_slice(b"PS");
    bytes.extend_from_slice(&(first_description.len() as u32).to_be_bytes());
    bytes.extend_from_slice(first_description);
    bytes.extend_from_slice(b"payload PS\x00\x00not a header");
    let second = bytes.len();
    let second_description = b": TRANSMIT FILE (deltas) created by test";
    bytes.extend_from_slice(b"PS");
    bytes.extend_from_slice(&(second_description.len() as u32).to_be_bytes());
    bytes.extend_from_slice(second_description);

    assert_eq!(super::legacy_stream_start(&bytes, 0), Some(first));
    assert_eq!(super::legacy_stream_start(&bytes, first + 4), Some(second));
    assert_eq!(super::legacy_stream_start(&bytes, second + 4), None);
}

#[test]
fn legacy_short_sections_are_bounded_by_complete_transmit_headers() {
    let mut bytes = Vec::new();
    let first_description = b": TRANSMIT FILE (partition)";
    bytes.extend_from_slice(b"PS");
    bytes.extend_from_slice(&(first_description.len() as u32).to_be_bytes());
    bytes.extend_from_slice(first_description);
    let second = bytes.len();
    let second_description = b": TRANSMIT FILE (deltas)";
    bytes.extend_from_slice(b"PS");
    bytes.extend_from_slice(&(second_description.len() as u32).to_be_bytes());
    bytes.extend_from_slice(second_description);
    bytes.extend_from_slice(&[0; 64]);

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let streams = super::extract_legacy_streams(&ctx, root).unwrap();

    assert_eq!(streams.len(), 2);
    assert_eq!(streams[0].file_offset, 0);
    assert_eq!(streams[1].file_offset, second);
}

#[test]
fn parasolid_entity_51_records_retain_layout_selected_references() {
    let mut bytes = vec![0, 0x51];
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&0x21u16.to_be_bytes());
    for reference in 3..=8u16 {
        bytes.extend_from_slice(&reference.to_be_bytes());
    }
    bytes.extend_from_slice(&[0xaa, 0xbb]);

    let records = crate::parasolid::entity_51_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 0);
    assert_eq!(records[0].byte_len, 26);
    assert_eq!(u32::from(records[0].xmt), 10);
    assert_eq!(records[0].sequence.get(), 2);
    assert_eq!(records[0].definition_xmt, 0x21);
    assert_eq!(records[0].leading_references, [3, 4, 5, 6, 7]);
    assert_eq!(records[0].trailing_references.values(), [8]);
    assert_eq!(
        crate::parasolid::entity_51_record_at(&bytes, 0),
        Some(records[0].clone())
    );
    assert!(crate::parasolid::entity_51_record_at(&bytes[..25], 0).is_none());
}

#[test]
fn parasolid_entity_51_definition_uses_extended_xmt_framing() {
    let mut bytes = vec![0, 0x51];
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&(-7_233i16).to_be_bytes());
    bytes.extend_from_slice(&1u16.to_be_bytes());
    for reference in 3..=8u16 {
        bytes.extend_from_slice(&reference.to_be_bytes());
    }

    let record = crate::parasolid::entity_51_record_at(&bytes, 0).unwrap();
    assert_eq!(record.definition_xmt, 40_000);
    assert_eq!(record.byte_len, 28);
    assert!(crate::parasolid::entity_51_record_at(&bytes[..27], 0).is_none());
}

#[test]
fn parasolid_entity_51_reference_count_is_five_plus_flags() {
    for flags in 1..=0x20u32 {
        let mut direct = vec![0, 0x51];
        direct.extend_from_slice(&flags.to_be_bytes());
        direct.extend_from_slice(&10u16.to_be_bytes());
        direct.extend_from_slice(&2u32.to_be_bytes());
        direct.extend_from_slice(&0x21u16.to_be_bytes());
        for reference in 0..flags + 5 {
            direct.extend_from_slice(&(reference as u16 + 3).to_be_bytes());
        }
        direct.extend_from_slice(&[0xaa, 0xbb]);

        let record = crate::parasolid::entity_51_record_at(&direct, 0).unwrap();
        assert_eq!(record.leading_references.len(), 5);
        assert_eq!(record.trailing_references.values().len(), flags as usize);
        assert_eq!(record.byte_len, direct.len() - 2);
        assert!(crate::parasolid::entity_51_record_at(&direct[..direct.len() - 3], 0).is_none());

        let mut prefixed = vec![0, 0x51];
        prefixed.extend_from_slice(&flags.to_be_bytes());
        prefixed.extend_from_slice(&10u16.to_be_bytes());
        prefixed.extend_from_slice(&2u32.to_be_bytes());
        prefixed.extend_from_slice(&0x21u16.to_be_bytes());
        for reference in 0..flags + 5 {
            prefixed.push(u8::from(reference % 2 == 0));
            prefixed.extend_from_slice(&(reference as u16 + 3).to_be_bytes());
        }
        prefixed.push(0);
        prefixed.extend_from_slice(&[0xaa, 0xbb]);

        let record = crate::parasolid::entity_51_record_at(&prefixed, 0).unwrap();
        assert_eq!(record.leading_references.len(), 5);
        assert_eq!(record.trailing_references.values().len(), flags as usize);
        assert_eq!(record.byte_len, prefixed.len() - 2);
        assert!(
            crate::parasolid::entity_51_record_at(&prefixed[..prefixed.len() - 3], 0).is_none()
        );
    }
}

#[test]
fn parasolid_entity_51_rejects_nonzero_upper_flag_bytes() {
    let mut bytes = vec![0, 0x51];
    bytes.extend_from_slice(&0x0100_0001u32.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&0x21u16.to_be_bytes());
    for reference in 3..=8u16 {
        bytes.extend_from_slice(&reference.to_be_bytes());
    }

    assert!(crate::parasolid::entity_51_record_at(&bytes, 0).is_none());
}

#[test]
fn parasolid_field_names_require_a_complete_nonempty_reference_lane() {
    let bytes = [
        0xaa, 0x00, 0x63, 0x00, 0x00, 0x00, 0x03, 0x00, 0x19, 0x00, 0x1c, 0x00, 0x1d, 0x00, 0x1e,
        0xbb,
    ];
    let records = crate::parasolid::field_names_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].byte_len, 14);
    assert_eq!(u32::from(records[0].xmt), 25);
    assert_eq!(
        records[0]
            .name_xmts
            .as_slice()
            .iter()
            .copied()
            .map(u32::from)
            .collect::<Vec<_>>(),
        [28, 29, 30]
    );
    assert!(crate::parasolid::field_names_record_at(&bytes[..14], 1).is_none());

    let empty = [0x00, 0x63, 0, 0, 0, 0, 0, 25];
    assert!(crate::parasolid::field_names_records(&empty).is_empty());
}

#[test]
fn parasolid_field_name_scan_does_not_admit_nested_counted_candidates() {
    let mut bytes = vec![0x00, 0x63];
    bytes.extend_from_slice(&6u32.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());
    bytes.extend_from_slice(&[
        0x00, 0x63, 0xff, 0x00, 0x00, 0x00, 0x01, 0x00, 0x20, 0x00, 0x30, 0x00, 0x00, 0x40,
    ]);

    let records = crate::parasolid::field_names_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 0);
    assert_eq!(
        records[0]
            .name_xmts
            .as_slice()
            .iter()
            .copied()
            .map(u32::from)
            .collect::<Vec<_>>(),
        [99, 256, 256, 8192, 12288, 64]
    );
}

#[test]
fn partition_values_require_a_unique_entity_reference() {
    let mut bytes = parasolid_entity_records_stream();
    let owned = crate::parasolid::referenced_value_record_offsets(&bytes);
    assert_eq!(owned.len(), 3);

    bytes.extend_from_slice(&[0, 98]);
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&200u16.to_be_bytes());
    bytes.extend_from_slice(&[0, b'N', 0, b'X']);

    assert_eq!(
        crate::parasolid::referenced_value_record_offsets(&bytes),
        owned
    );
}

#[test]
fn partition_character_values_can_be_owned_by_a_field_name_list() {
    let mut bytes = parasolid_entity_records_stream();
    let definition_offset = crate::parasolid::attribute_definitions(&bytes)[0].offset;
    bytes[definition_offset + 24..definition_offset + 26].copy_from_slice(&34u16.to_be_bytes());
    bytes.extend_from_slice(&[0, 0x63]);
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&34u16.to_be_bytes());
    bytes.extend_from_slice(&37u16.to_be_bytes());
    let unicode_offset = bytes.len();
    bytes.extend_from_slice(&[0, 98]);
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&37u16.to_be_bytes());
    bytes.extend_from_slice(&[0, b'N', 0, b'X']);

    assert!(crate::parasolid::referenced_value_record_offsets(&bytes).contains(&unicode_offset));
}

#[test]
fn parasolid_extraction_classifies_partition_and_schema() {
    let f = single_part_prt();
    let streams = extract_streams(&f);
    let part = streams
        .iter()
        .find(|s| s.kind() == StreamKind::Partition)
        .expect("a partition stream");
    assert_eq!(part.schema(), Some("SCH_TEST_1_9999"));
    assert!(part.inflated.starts_with(b"PS\x00\x00"));
}

#[test]
fn external_reference_string_table_is_end_anchored() {
    let table = b"prefix\x01\x02\x00\x00\x00\x09\x00child.prt\x0c\x00nested/b.prt";
    let (_, strings) = crate::container::parse_extref_string_table(table).expect("string table");
    assert_eq!(
        strings
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>(),
        ["child.prt", "nested/b.prt"]
    );

    let mut trailed = table.to_vec();
    trailed.push(0);
    assert!(crate::container::parse_extref_string_table(&trailed).is_none());
    assert!(crate::container::parse_extref_string_table(b"\x01\xff\xff\xff\xff").is_none());
}

#[test]
fn external_reference_record_parser_accepts_sorted_repeated_handles() {
    let mut payload = b"EXTREFSTREAM".to_vec();
    payload.extend_from_slice(&3u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.push(0);
    payload.extend_from_slice(&6u32.to_le_bytes());
    payload.extend_from_slice(&41u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(payload.len(), 41);
    payload.extend_from_slice(&[1, 0, 0, 0]);
    payload.extend_from_slice(&2u16.to_be_bytes());
    payload.push(1);
    for value in [8u32, 11, 12, 4] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[1, 5]);
    for handle in [0x1020_3040u32, 0x2030_4050, 0x2030_4050, 0x2030_4050] {
        payload.push(0xe0);
        payload.extend_from_slice(&handle.to_be_bytes());
    }
    payload.push(5);
    payload.extend_from_slice(b"\x01\x01\x00\x00\x00\x09\x00child.prt");

    let records = crate::container::parse_extref_records(&payload);
    let indexed = crate::container::parse_extref_record_index(&payload).expect("record index");
    assert_eq!(indexed.len(), 1);
    assert_eq!(indexed[0].record_id, 6);
    assert_eq!(indexed[0].offset, 41);
    assert_eq!(indexed[0].byte_len, 46);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].record_id, 6);
    assert_eq!(records[0].declared_count, 2);
    assert_eq!(records[0].id_slots, [8, 11, 12, 4]);
    assert_eq!(
        records[0].handles.values(),
        [0x1020_3040, 0x2030_4050, 0x2030_4050]
    );
    assert!(records[0].handles.closing_duplicate());
    assert_eq!(records[0].tail_byte_len, 0);

    let duplicate = payload
        .windows(5)
        .rposition(|window| window == [0xe0, 0x20, 0x30, 0x40, 0x50])
        .expect("closing duplicate");
    payload[duplicate + 1] = 0x10;
    assert!(crate::container::parse_extref_records(&payload).is_empty());
    assert_eq!(
        crate::container::parse_extref_record_index(&payload)
            .expect("opaque indexed record")
            .len(),
        1
    );
}

#[test]
fn external_reference_empty_record_parser_requires_the_complete_form() {
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1]),
        Some(false)
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1, 1]),
        Some(true)
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1, 0]),
        None
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0]),
        None
    );
}

#[test]
fn external_reference_tail_pairs_require_adjacent_complete_tokens() {
    let bytes = [
        0xff, 0xe0, 0x12, 0x34, 0x56, 0x78, 0xca, 0xbc, 0xde, 0xf0, 0xe0, 0x00, 0x00, 0x00, 0x01,
        0x00,
    ];
    assert_eq!(
        crate::container::parse_extref_reference_pairs(&bytes),
        vec![(
            1,
            0x1234_5678,
            crate::om::reference_value::Tagged28::try_from(0x0abc_def0).unwrap()
        )]
    );
    assert!(crate::container::parse_extref_reference_pairs(&bytes[10..]).is_empty());
}

#[test]
fn extraction_uses_ug_part_bounds_and_all_standard_zlib_headers() {
    let part = zlib_compress_at_level(&partition_stream(), 6);
    assert_eq!(&part[..2], b"\x78\x9c");

    let mut decoy_stream = partition_stream();
    let schema = b"SCH_TEST_1_9999";
    let decoy = b"SCH_FAKE_1_9999";
    let pos = decoy_stream
        .windows(schema.len())
        .position(|w| w == schema)
        .unwrap();
    decoy_stream[pos..pos + schema.len()].copy_from_slice(decoy);
    let decoy = zlib_compress(&decoy_stream);

    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", part),
        ("/Root/FastLoad/JT", decoy),
    ]);

    let streams = extract_streams(&file);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].schema(), Some("SCH_TEST_1_9999"));
}

#[test]
fn extraction_accepts_short_complete_zlib_members_in_ug_part() {
    let inflated = b"PS\0\0SCH_X";
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", zlib_compress(inflated))]);

    let streams = extract_streams(&file);

    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].inflated, inflated);
    assert_eq!(streams[0].kind(), StreamKind::Plain);
    assert_eq!(streams[0].schema(), Some("SCH_X"));
}

#[test]
fn extraction_rejects_zlib_members_with_invalid_integrity_trailers() {
    let compressed = zlib_compress(&partition_stream());
    let mut corrupt = compressed.clone();
    *corrupt.last_mut().expect("zlib integrity trailer") ^= 0x01;
    let corrupt = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", corrupt)]);
    assert!(extract_streams(&corrupt).is_empty());

    let truncated = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        compressed[..compressed.len() - 1].to_vec(),
    )]);
    assert!(extract_streams(&truncated).is_empty());

    let mut indexed = segment_stream_payload();
    *indexed.last_mut().expect("indexed zlib integrity trailer") ^= 0x01;
    let indexed = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", indexed)]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&indexed, &arena, &policy)
            .expect("bounded test input");
    let container = container::scan_bytes(indexed.clone()).expect("test SPLMSSTR container");
    assert!(parasolid::extract_streams(&ctx, root, &container).is_err());
}

#[test]
fn extraction_refuses_inflated_stream_copy_when_retained_budget_is_exhausted() {
    let file = prt_with_partition(&partition_stream());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_retained_bytes = 1;
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&file, &arena, &policy)
        .expect("bounded test input");
    let container = container::scan_bytes(file.clone()).expect("test SPLMSSTR container");

    assert!(matches!(
        parasolid::extract_streams(&ctx, root, &container),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain NX inflated stream"
    ));
}

#[test]
fn extraction_uses_ordered_segment_wrappers_in_indexed_payloads() {
    let decoy = zlib_compress(
        b"PS\0\0 (partition) SCH_DECOY_1_9999 unindexed payload with more than sixty-four inflated bytes........",
    );
    let real = zlib_compress(
        b"PS\0\0 (deltas) SCH_REAL_1_9999 indexed payload with more than sixty-four inflated bytes..........",
    );
    let mut payload = Vec::new();
    for word in [0_u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend_from_slice(&decoy);
    let wrapper_offset = payload.len();
    payload[0..4].copy_from_slice(
        &u32::try_from(wrapper_offset)
            .expect("synthetic wrapper offset")
            .to_le_bytes(),
    );
    payload.extend_from_slice(&0x8000_0000_u32.to_le_bytes());
    payload.extend_from_slice(&0_u32.to_le_bytes());
    payload.extend_from_slice(&real);

    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
    let streams = extract_streams(&file);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].kind(), StreamKind::Deltas);
    assert_eq!(streams[0].schema(), Some("SCH_REAL_1_9999"));
}

#[test]
fn extraction_falls_back_to_unindexed_structural_streams_when_index_has_no_parasolid() {
    let decoy = zlib_compress(
        b"PS\0\0 (partition) SCH_DECOY_1_9999 unindexed text without structural records",
    );
    let real = zlib_compress(&parasolid_group_partition_stream());
    let indexed_preview = zlib_compress(b"preview payload");
    let mut payload = Vec::new();
    for word in [0_u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend_from_slice(&decoy);
    payload.extend_from_slice(&real);
    let wrapper_offset = payload.len();
    payload[0..4].copy_from_slice(
        &u32::try_from(wrapper_offset)
            .expect("synthetic wrapper offset")
            .to_le_bytes(),
    );
    payload.extend_from_slice(&0x8000_0000_u32.to_le_bytes());
    payload.extend_from_slice(&0_u32.to_le_bytes());
    payload.extend_from_slice(&indexed_preview);

    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
    let streams = extract_streams(&file);

    assert_eq!(streams.len(), 2);
    assert!(streams.iter().any(|stream| {
        stream.kind() == StreamKind::Partition && stream.schema() == Some("SCH_TEST_1_9999")
    }));
    assert!(streams
        .iter()
        .all(|stream| { stream.schema() != Some("SCH_DECOY_1_9999") }));
}

#[test]
fn legal_owner_flags_carry_binary_values_and_exact_layout_length() {
    use crate::parasolid::LegalOwnerFlags;

    let flags = LegalOwnerFlags::try_from(&[1; 14][..]).unwrap();
    assert_eq!(flags.as_slice(), &[true; 14]);
    assert_eq!(&flags.padded()[..14], &[1; 14]);
    assert_eq!(&flags.padded()[14..], &[0; 2]);
    assert!(LegalOwnerFlags::try_from(&[0; 15][..]).is_err());
    assert!(LegalOwnerFlags::try_from(&[2; 16][..]).is_err());
}

#[test]
fn counted_value_identities_require_nonempty_payloads() {
    for tag in [0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x62] {
        let bytes = [0, tag, 0, 0, 0, 0, 0, 17, 0];
        assert!(
            crate::parasolid::value_records::entity_value_record_identity_at(&bytes, 0).is_none()
        );
    }
}
