// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use crate::test_support::test_cfb::legacy_cfb_with_two_streams;
use crate::test_support::test_om::offset_only_indexed_om_section_with_index_values;
use crate::test_support::test_om::segment_index_payload;
use crate::test_support::test_om::size_framed_om_section_with_repeated_operations;
use crate::test_support::test_prt::append_rmfastload_table;
use crate::test_support::test_prt::prt_with_indexed_om_section;
use crate::test_support::test_prt::prt_with_named_payloads;
use crate::test_support::test_prt::rmfastload_prt;
use crate::test_support::test_prt::single_part_prt;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_core::decode::InspectOptions;

use crate::container;
use crate::container::{test_modern_layout, Container, ContainerLayout, DirEntry, Region};
use crate::NxCodec;

#[test]
fn ug_part_segment_index_uses_row_one_self_boundary() {
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
    let container = container::scan_bytes(file).unwrap();
    let (_, index) = container.segment_index().expect("segment index");
    assert_eq!(index.rows().count() * 12 + index.padding.len(), 28);
    assert_eq!(index.rows().count(), 2);
    assert_eq!(index.row(0).expect("first validated row").type_code, 7);
    assert_eq!(index.row(0).expect("first validated row").subtype_code, 9);
    assert_eq!(index.row(0).expect("first validated row").value, 11);
    assert_eq!(index.row(1).expect("second validated row").type_code, 1);
    assert_eq!(index.row(1).expect("second validated row").subtype_code, 1);
    assert_eq!(index.row(1).expect("second validated row").value, 28);
    assert_eq!(index.padding, &[0xaa, 0xbb, 0xcc, 0xdd]);
}

#[test]
fn container_parses_header_and_directory() {
    let c = container::scan_bytes(single_part_prt()).unwrap();
    assert_eq!(c.layout.version(), 0x06);
    let ContainerLayout::Modern {
        file_tag,
        footer_fingerprint,
        ..
    } = c.layout
    else {
        panic!("SPLMSSTR input must have modern layout facts");
    };
    assert_eq!(c.entry_count(Region::Header), 1);
    assert_eq!(file_tag, 0x33_22_11);
    assert_eq!(c.entry_count(Region::Footer), 0);
    assert_eq!(footer_fingerprint, [0; 4]);
    assert!(c
        .entries
        .iter()
        .any(|e| e.name == "/Root/UG_PART/UG_PART" && e.file_span().is_some()));
}

#[test]
fn legacy_scan_admits_two_stream_directory() {
    let file = legacy_cfb_with_two_streams();
    crate::test_support::with_decode_context_over(
        &file,
        |_| {},
        |ctx| {
            assert!(
                container::scan_legacy(ctx, cadmpeg_core::decode::View::over_retained(&file))
                    .is_ok()
            );
        },
    );
}

#[test]
fn container_bounded_entry_tail_stops_at_the_next_stream() {
    let payload = [1, 2, 3, 4, 5, 6];
    let container = Container {
        data: payload.as_slice().into(),
        physical_size: cadmpeg_core::decode::u64_from_index(payload.len()),
        layout: ContainerLayout::LegacyCfb { version: 0 },
        entries: vec![
            DirEntry {
                name: "/Root/first".into(),
                region: Region::Header,
                body: crate::container::DirEntryBody::File { offset: 0, len: 3 },
            },
            DirEntry {
                name: "/Root/second".into(),
                region: Region::Header,
                body: crate::container::DirEntryBody::File { offset: 3, len: 3 },
            },
        ],
        fastload_table: None,
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    assert_eq!(container.bounded_entry_bytes(1, 2), Some(&payload[1..3]));
    assert_eq!(container.bounded_entry_bytes(1, 3), None);
    assert_eq!(container.bounded_entry_bytes(3, 3), Some(&payload[3..6]));
    assert_eq!(container.bounded_entry_tail(1), Some(&payload[1..3]));
    assert_eq!(container.bounded_entry_tail(4), Some(&payload[4..6]));
    assert_eq!(container.bounded_entry_tail(6), None);
}

#[test]
fn container_cached_operation_labels_preserve_section_materialization() {
    let payload = size_framed_om_section_with_repeated_operations(2);
    let container = Container {
        data: payload.as_slice().into(),
        physical_size: cadmpeg_core::decode::u64_from_index(payload.len()),
        layout: test_modern_layout(0),
        entries: vec![DirEntry {
            name: "/Root/om".into(),
            region: Region::Header,
            body: crate::container::DirEntryBody::File {
                offset: 0,
                len: cadmpeg_core::decode::u64_from_index(payload.len()),
            },
        }],
        fastload_table: None,
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let direct = crate::om::sections(&payload).unwrap();
    let cached = container.om_sections().unwrap();
    assert_eq!(cached.len(), direct.len());
    assert!(container.om_section_cache.get().is_some());
    for ((entry, section), expected) in cached.iter().zip(direct.iter()) {
        assert_eq!(entry.name, "/Root/om");
        assert_eq!(section, expected);
        assert_eq!(section.operation_labels(), expected.operation_labels());
        assert_eq!(
            section.operation_records_with_label_ordinals(),
            expected.operation_records_with_label_ordinals()
        );
    }
    let repeated = container.om_sections().unwrap();
    assert_eq!(repeated, cached);
    assert!(std::sync::Arc::ptr_eq(
        &cached[0].1.types,
        &repeated[0].1.types
    ));
}

#[test]
fn container_caches_owned_section_layouts() {
    let payload = size_framed_om_section_with_repeated_operations(2);
    let payload_len = cadmpeg_core::decode::u64_from_index(payload.len());
    let mut file = vec![0xaa; 17];
    file.extend_from_slice(&payload);
    let physical_size = cadmpeg_core::decode::u64_from_index(file.len());
    let container = Container {
        data: file.into(),
        physical_size,
        layout: test_modern_layout(0),
        entries: vec![DirEntry {
            name: "/Root/om".into(),
            region: Region::Header,
            body: crate::container::DirEntryBody::File {
                offset: 17,
                len: payload_len,
            },
        }],
        fastload_table: None,
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let first = container.om_sections().unwrap();
    let second = container.om_sections().unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(second, first);
    assert_eq!(
        first[0].1,
        crate::om::sections(&container.data[17..]).unwrap()[0]
    );
    assert!(container.om_section_cache.get().is_some_and(|cache| {
        matches!(
            cache,
            container::FramedSectionCache::Owned { layouts } if layouts.len() == 1
        )
    }));
}

#[test]
fn container_reuses_materialized_indexed_sections_for_borrowed_input() {
    let file = prt_with_indexed_om_section();
    let container = container::scan_bytes(file.as_slice()).unwrap();
    let first = container.indexed_om_sections().unwrap();
    let second = container.indexed_om_sections().unwrap();
    assert!(!first.is_empty());
    assert_eq!(first, second);
    assert!(std::sync::Arc::ptr_eq(
        &first[0].1.types,
        &second[0].1.types
    ));
    match (&first[0].1.store, &second[0].1.store) {
        (
            crate::om::IndexedStore::Fixed {
                records: first_records,
            },
            crate::om::IndexedStore::Fixed {
                records: second_records,
            },
        ) => assert!(std::sync::Arc::ptr_eq(first_records, second_records)),
        (
            crate::om::IndexedStore::OffsetOnly {
                records: first_records,
                ..
            },
            crate::om::IndexedStore::OffsetOnly {
                records: second_records,
                ..
            },
        ) => assert!(std::sync::Arc::ptr_eq(first_records, second_records)),
        _ => panic!("indexed section store kind changed between cache hits"),
    }
}

#[test]
fn container_reuses_borrowed_offset_store_block_index() {
    let section = offset_only_indexed_om_section_with_index_values();
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", section)]);
    let container = container::scan_bytes(file.as_slice()).unwrap();
    let _ = container.indexed_om_sections().unwrap();
    let first = container
        .cached_offset_data_block_bytes()
        .expect("borrowed indexed sections cache their offset-store blocks");
    let second = container
        .cached_offset_data_block_bytes()
        .expect("cached offset-store blocks remain available");

    assert!(!first.is_empty());
    assert!(first.contains_key("nx:om-data-blocks-0:block#0"));
    assert!(std::ptr::eq(first, second));
}

#[test]
fn container_counts_admitted_entries_in_each_region() {
    let mut file = single_part_prt();
    file.truncate(file.len() - 8);
    file.extend_from_slice(&1_u32.to_le_bytes());
    file.extend_from_slice(&6_u32.to_le_bytes());
    file.extend_from_slice(b"/Root/");
    file.extend_from_slice(&[0; 16]);
    file.extend_from_slice(&[0; 4]);
    let container = container::scan_bytes(file).expect("one entry in each counted region");
    assert_eq!(container.entry_count(Region::Header), 1);
    assert_eq!(container.entry_count(Region::Footer), 1);
    assert_eq!(container.entries.len(), 2);
}

#[test]
fn service_profile_admits_both_directory_regions() {
    let mut file = single_part_prt();
    file.truncate(file.len() - 8);
    file.extend_from_slice(&1_u32.to_le_bytes());
    file.extend_from_slice(&6_u32.to_le_bytes());
    file.extend_from_slice(b"/Root/");
    file.extend_from_slice(&[0; 16]);
    file.extend_from_slice(&[0; 4]);

    let container = container::scan_bytes(file.as_slice()).expect("service directory admission");
    assert_eq!(container.entry_count(Region::Header), 1);
    assert_eq!(container.entry_count(Region::Footer), 1);
}

#[test]
fn container_rejects_incomplete_counted_directories() {
    let mut header = single_part_prt();
    header[0x1f..0x23].copy_from_slice(&2_u32.to_le_bytes());
    assert!(container::scan_bytes(header).is_err());

    let mut footer = single_part_prt();
    let footer_offset = usize::try_from(u64::from_le_bytes([
        footer[0x11],
        footer[0x12],
        footer[0x13],
        footer[0x14],
        footer[0x15],
        footer[0x16],
        0,
        0,
    ]))
    .expect("synthetic footer offset");
    footer[footer_offset + 6..footer_offset + 10].copy_from_slice(&1_u32.to_le_bytes());
    assert!(container::scan_bytes(footer).is_err());
}

#[test]
fn container_rejects_trailing_or_overlapping_footer_data() {
    let mut trailing = single_part_prt();
    trailing.push(0);
    assert!(container::scan_bytes(trailing).is_err());

    let mut overlap = single_part_prt();
    let name_len = usize::try_from(u32::from_le_bytes(
        overlap[0x23..0x27]
            .try_into()
            .expect("synthetic name length"),
    ))
    .expect("synthetic name length fits usize");
    let span = 0x27 + name_len;
    let offset = u64::from_le_bytes(
        overlap[span..span + 8]
            .try_into()
            .expect("synthetic file offset"),
    );
    let footer_offset = u64::from_le_bytes([
        overlap[0x11],
        overlap[0x12],
        overlap[0x13],
        overlap[0x14],
        overlap[0x15],
        overlap[0x16],
        0,
        0,
    ]);
    overlap[span + 8..span + 16].copy_from_slice(&(footer_offset - offset + 1).to_le_bytes());
    assert!(container::scan_bytes(overlap).is_err());
}

#[test]
fn container_rejects_footer_offset_beyond_the_file_image() {
    let mut bytes = single_part_prt();
    bytes[0x11..0x17].copy_from_slice(&[0xff; 6]);
    let error = container::scan_bytes(bytes).expect_err("required invariant");
    assert_eq!(
        error.to_string(),
        "malformed container: FOOTER offset exceeds the file image"
    );
}

#[test]
fn container_reads_rmfastload_active_ids() {
    let container = container::scan_bytes(rmfastload_prt()).unwrap();
    let (entry, table) = container
        .rmfastload_object_id_table()
        .expect("RMFastLoad object-id table");
    assert_eq!(entry.name, "/Root/FastLoad/RMFastLoad");
    assert_eq!(table.registry_offset, 0);
    assert_eq!(table.count_offset, b"UGS::Solid::Topol".len());
    assert_eq!(table.object_ids.count().to_le_bytes(), 50u32.to_le_bytes());
    assert_eq!(
        table.object_ids.as_slice().to_vec(),
        (1..=50).collect::<Vec<_>>()
    );
    assert_eq!(table.member_offset(0), table.count_offset + 4);
    assert_eq!(
        table.object_ids.as_slice()[0].to_le_bytes(),
        1u32.to_le_bytes()
    );
    assert_eq!(table.member_offset(49), table.count_offset + 4 + 49 * 4);
    assert_eq!(
        table.object_ids.as_slice()[49].to_le_bytes(),
        50u32.to_le_bytes()
    );
    let (_, same_table) = container
        .rmfastload_object_id_table()
        .expect("shared RMFastLoad table");
    assert!(std::ptr::eq(table, same_table));
}

#[test]
fn container_reads_rmfastload_table_from_product_boundary_without_range_floor() {
    let mut payload = b"UGS::Solid::Topol".to_vec();
    append_rmfastload_table(&mut payload, [0, u32::MAX, 7]);
    let file = prt_with_named_payloads(&[("/Root/FastLoad/RMFastLoad", payload)]);
    let container = container::scan_bytes(file).unwrap();
    let (_, table) = container
        .rmfastload_object_id_table()
        .expect("product-bounded RMFastLoad table");
    assert_eq!(table.object_ids.as_slice().len(), 3);
    assert_eq!(table.object_ids.as_slice().to_vec(), [0, u32::MAX, 7]);
}

#[test]
fn fuzz_oom_splmsstr_header_is_rejected_without_count_allocation() {
    // libFuzzer artifact: SPLMSSTR + HEADER with a footer offset past EOF and a
    // directory count that would request >2 GiB if taken as a Vec capacity.
    let bytes: &[u8] = &[
        0x53, 0x50, 0x4c, 0x4d, 0x53, 0x53, 0x54, 0x52, 0x26, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff,
        0xff, 0xff, 0x0a, 0x00, 0x06, 0x00, 0xff, 0xff, 0x90, 0xff, 0x48, 0x45, 0x41, 0x44, 0x45,
        0x52, 0x20, 0x00, 0x00, 0x6f, 0x2f, 0xf9, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x04, 0x00, 0x04,
    ];
    assert!(container::scan_bytes(bytes.to_vec()).is_err());
    let _ = cadmpeg_test_support::detection::confidence(&NxCodec, bytes);
    let _probe = NxCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default());
    let _probe = NxCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default());
}

#[test]
fn container_bounds_rmfastload_table_at_its_first_product_record() {
    let mut payload = b"UGS::Solid::Topol".to_vec();
    append_rmfastload_table(&mut payload, [1, 2, 3]);
    append_rmfastload_table(&mut payload, [4, 5]);
    let file = prt_with_named_payloads(&[("/Root/FastLoad/RMFastLoad", payload)]);
    let container = container::scan_bytes(file).unwrap();
    let (_, table) = container
        .rmfastload_object_id_table()
        .expect("first product-bounded table");
    assert_eq!(table.object_ids.as_slice().to_vec(), [1, 2, 3]);
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
fn overlapping_extref_candidates_are_rejected() {
    let mut bytes = vec![b'A'; 32_775];
    for ordinal in 1..=127 {
        let marker = ordinal * 256;
        bytes[marker] = 1;
        bytes[marker + 1..marker + 5].copy_from_slice(&1_u32.to_le_bytes());
        bytes[marker + 5..marker + 7]
            .copy_from_slice(&u16::try_from(32_768 - marker).unwrap().to_le_bytes());
    }
    *bytes.last_mut().unwrap() = 0;
    assert_eq!(
        super::locate_extref_string_table(&bytes),
        super::ExtrefStringTableSearch::Absent
    );
}

#[test]
fn overlapping_extref_candidates_skip_the_search_at_its_work_bound() {
    // Every eighth byte starts a one-entry table whose string runs to the
    // payload end, so the candidates declare about 2^28 string bytes.
    let mut bytes = vec![b'A'; 65_543];
    for marker in (8..bytes.len() - 8).step_by(8) {
        bytes[marker] = 1;
        bytes[marker + 1..marker + 5].copy_from_slice(&1_u32.to_le_bytes());
        let length = u16::try_from(bytes.len() - marker - 7).unwrap();
        bytes[marker + 5..marker + 7].copy_from_slice(&length.to_le_bytes());
    }
    *bytes.last_mut().unwrap() = 0;
    assert_eq!(
        super::locate_extref_string_table(&bytes),
        super::ExtrefStringTableSearch::Bounded
    );

    let file = prt_with_named_payloads(&[("/Root/ExternalReferences", bytes)]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let bounded = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::NxLossCode::SourceScanBounded.kind())
        .collect::<Vec<_>>();
    assert_eq!(bounded.len(), 1);
    assert!(bounded[0].message.contains("/Root/ExternalReferences"));
    assert!(result.ir().source.as_ref().is_some_and(|source| !source
        .attributes
        .keys()
        .any(|key| key.as_str().starts_with("external_reference"))));
}

#[test]
fn external_reference_materialization_copies_each_string() {
    let text = "A".repeat(1000);
    let mut bytes = vec![1];
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(1000u16.to_le_bytes());
    bytes.extend(text.as_bytes());
    let (_, values) = super::parse_extref_string_table(&bytes).unwrap();
    assert_eq!(values, [(7, text)]);
}
