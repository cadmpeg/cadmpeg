// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::report::{LossCategory, LossKind, LossTaxonomy};
use cadmpeg_ir::Exactness;

use crate::container;
use crate::parasolid::{self, StreamKind};
use crate::test_support::*;
use crate::NxCodec;

use super::*;

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
    assert_eq!(c.file_tag, 0x33_22_11);
    assert_eq!(c.header_entry_count, 1);
    assert_eq!(c.footer_entry_count, 0);
    assert_eq!(c.footer_fingerprint, [0; 4]);
    assert!(c
        .entries
        .iter()
        .any(|e| e.name == "/Root/UG_PART/UG_PART" && e.file_span.is_some()));
}

#[test]
fn container_cached_operation_labels_preserve_section_materialization() {
    let payload = size_framed_om_section_with_repeated_operations(2);
    let container = Container {
        data: payload.clone().into(),
        version: 0,
        file_tag: 0,
        footer_offset: 0,
        header_entry_count: 1,
        footer_entry_count: 0,
        footer_fingerprint: [0; 4],
        entries: vec![DirEntry {
            name: "/Root/om".into(),
            region: Region::Header,
            file_span: Some((0, payload.len() as u64)),
        }],
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_operation_label_layouts: std::sync::OnceLock::new(),
    };
    let direct = crate::om::sections(&payload);
    let cached = container.om_sections();
    assert_eq!(cached.len(), direct.len());
    assert!(container.om_operation_label_layouts.get().is_some());
    for ((entry, section), expected) in cached.iter().zip(direct.iter()) {
        assert_eq!(entry.name, "/Root/om");
        assert_eq!(section, expected);
        assert_eq!(section.operation_labels(), expected.operation_labels());
        assert_eq!(section.operation_records(), expected.operation_records());
    }
    assert_eq!(container.om_sections(), cached);
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
    assert!(std::sync::Arc::ptr_eq(
        &first[0].1.records,
        &second[0].1.records
    ));
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
