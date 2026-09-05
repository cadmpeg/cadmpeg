// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_core::decode::InspectOptions;

use crate::container;
use crate::container::{Container, ContainerLayout, DirEntry, Region, TEST_MODERN_LAYOUT};
use crate::test_support::*;
use crate::NxCodec;

#[test]
fn ug_part_segment_index_uses_row_one_self_boundary() {
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
    let container = container::scan_bytes(file).unwrap();
    let (_, index) = container.segment_index().expect("segment index");
    assert_eq!(index.byte_len, 28);
    assert_eq!(index.rows.len(), 2);
    assert_eq!(index.rows[0].type_code, 7);
    assert_eq!(index.rows[0].subtype_code, 9);
    assert_eq!(index.rows[0].value, 11);
    assert_eq!(index.rows[1].type_code, 1);
    assert_eq!(index.rows[1].subtype_code, 1);
    assert_eq!(index.rows[1].value, 28);
    assert_eq!(index.padding, &[0xaa, 0xbb, 0xcc, 0xdd]);
}

#[test]
fn container_parses_header_and_directory() {
    let c = container::scan_bytes(single_part_prt()).unwrap();
    assert_eq!(c.version, 0x06);
    assert_eq!(c.header_entry_count, 1);
    let ContainerLayout::Modern {
        file_tag,
        footer_entry_count,
        footer_fingerprint,
        ..
    } = c.layout
    else {
        panic!("SPLMSSTR input must have modern layout facts");
    };
    assert_eq!(file_tag, 0x33_22_11);
    assert_eq!(footer_entry_count, 0);
    assert_eq!(footer_fingerprint, [0; 4]);
    assert!(c
        .entries
        .iter()
        .any(|e| e.name == "/Root/UG_PART/UG_PART" && e.file_span.is_some()));
}

#[test]
fn container_bounded_entry_tail_stops_at_the_next_stream() {
    let payload = [1, 2, 3, 4, 5, 6];
    let container = Container {
        data: payload.as_slice().into(),
        version: 0,
        header_entry_count: 2,
        physical_size: payload.len() as u64,
        layout: ContainerLayout::LegacyCfb,
        entries: vec![
            DirEntry {
                name: "/Root/first".into(),
                region: Region::Header,
                file_span: Some((0, 3)),
            },
            DirEntry {
                name: "/Root/second".into(),
                region: Region::Header,
                file_span: Some((3, 3)),
            },
        ],
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_operation_label_layouts: std::sync::OnceLock::new(),
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
        version: 0,
        header_entry_count: 1,
        physical_size: payload.len() as u64,
        layout: TEST_MODERN_LAYOUT,
        entries: vec![DirEntry {
            name: "/Root/om".into(),
            region: Region::Header,
            file_span: Some((0, payload.len() as u64)),
        }],
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_operation_label_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let direct = crate::om::sections(&payload);
    let cached = container.om_sections();
    assert_eq!(cached.len(), direct.len());
    assert!(container.om_operation_label_layouts.get().is_some());
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
    let repeated = container.om_sections();
    assert_eq!(repeated, cached);
    assert!(std::sync::Arc::ptr_eq(
        &cached[0].1.types,
        &repeated[0].1.types
    ));
}

#[test]
fn container_caches_owned_section_layouts() {
    let payload = size_framed_om_section_with_repeated_operations(2);
    let payload_len = payload.len() as u64;
    let mut file = vec![0xaa; 17];
    file.extend_from_slice(&payload);
    let physical_size = file.len() as u64;
    let container = Container {
        data: file.into(),
        version: 0,
        header_entry_count: 1,
        physical_size,
        layout: TEST_MODERN_LAYOUT,
        entries: vec![DirEntry {
            name: "/Root/om".into(),
            region: Region::Header,
            file_span: Some((17, payload_len)),
        }],
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_operation_label_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let first = container.om_sections();
    let second = container.om_sections();
    assert_eq!(first.len(), 1);
    assert_eq!(second, first);
    assert_eq!(first[0].1, crate::om::sections(&container.data[17..])[0]);
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
    let first = container.indexed_om_sections();
    let second = container.indexed_om_sections();
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
    let _ = container.indexed_om_sections();
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
    assert_eq!(table.raw_count, 50u32.to_le_bytes());
    assert_eq!(
        table
            .object_ids
            .iter()
            .map(|object_id| object_id.value)
            .collect::<Vec<_>>(),
        (1..=50).collect::<Vec<_>>()
    );
    assert_eq!(table.object_ids[0].offset, table.count_offset + 4);
    assert_eq!(table.object_ids[0].raw, 1u32.to_le_bytes());
    assert_eq!(table.object_ids[49].offset, table.count_offset + 4 + 49 * 4);
    assert_eq!(table.object_ids[49].raw, 50u32.to_le_bytes());
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
    assert_eq!(table.object_ids.len(), 3);
    assert_eq!(
        table
            .object_ids
            .iter()
            .map(|object_id| object_id.value)
            .collect::<Vec<_>>(),
        [0, u32::MAX, 7]
    );
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
    let _ = NxCodec.detect(bytes);
    let _ = NxCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default());
    let _ = NxCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default());
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
    assert_eq!(
        table
            .object_ids
            .iter()
            .map(|object_id| object_id.value)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
}
