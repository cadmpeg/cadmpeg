// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]
use super::exact_surface_trim_operation;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;
use std::io::{Cursor, Write};
use zip::CompressionMethod;

fn surface_trim_selection_and_cell_table() -> (Vec<u8>, DesignParameterScope) {
    surface_trim_selection_and_cell_table_with_base(0)
}

fn surface_trim_selection_and_cell_table_with_base(base: u32) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"344", base + 811);
    bytes.extend_from_slice(&[0; 11]);
    bytes.push(1);
    bytes.extend_from_slice(&(base + 814).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"257", base + 811);
    bytes.extend_from_slice(&[0; 10]);
    indexed_header(&mut bytes, *b"358", base + 812);
    bytes.extend_from_slice(&[0; 10]);
    indexed_header(&mut bytes, *b"272", base + 813);
    bytes.extend_from_slice(&[0; 10]);
    indexed_header(&mut bytes, *b"344", base + 814);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&123u64.to_le_bytes());
    indexed_header(&mut bytes, *b"288", base + 815);
    indexed_header(&mut bytes, *b"271", base + 816);
    indexed_header(&mut bytes, *b"325", base + 817);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for (record_index, ordinal) in [(819u32, 1u64), (820, 4)] {
        bytes.push(1);
        bytes.extend_from_slice(&(base + record_index).to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&ordinal.to_le_bytes());
    }
    bytes.extend_from_slice(&5u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    indexed_header(&mut bytes, *b"257", base + 817);
    indexed_header(&mut bytes, *b"286", base + 819);
    indexed_header(&mut bytes, *b"351", base + 820);

    let mut scope = DesignParameterScope::empty(
        &format!(
            "f3d:Design/BulkStream.dat:design-parameter-scope#{}",
            base + 800
        ),
        crate::records::feature::scope::DesignFeatureKind::SurfaceTrim,
        base + 800,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                base + 801,
                base + 804,
                base + 808,
                base + 811,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

#[test]
fn surface_trim_output_carries_its_scope_identifier() {
    const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let (bytes, mut scope) = surface_trim_selection_and_cell_table();
    scope.id = format!(
        "{}:design-parameter-scope#800",
        crate::ids::native_scope(ENTRY)
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(ENTRY, stored).unwrap();
    zip.write_all(&bytes).unwrap();
    let metadata = crate::test_support::zip_test::indexed_fixture_metastream(
        &bytes,
        &[811, 812, 813, 814, 815, 816, 817, 819, 820],
        &[811, 817],
    );
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&metadata).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut parts =
            cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, &ctx);
        let operations =
            super::decode_surface_trim_operations(&ctx, &mut parts, scan, &[scope.clone()])
                .unwrap();
        assert_eq!(operations.len(), 1);
        assert_eq!(
            operations[0].id,
            format!(
                "{}:design-surface-trim-operation#{}",
                crate::ids::native_scope(ENTRY),
                scope.byte_offset().expect("decoded scope offset"),
            ),
        );
    });
}

#[test]
fn surface_trim_decodes_selection_chain_and_cell_table() {
    let (bytes, scope) = surface_trim_selection_and_cell_table();
    let operation = exact_surface_trim_operation(
        &bytes,
        &crate::design::test_support::test_primary_frames(&bytes),
        &scope,
    )
    .expect("exact SurfaceTrim cell carrier");

    assert_eq!(operation.selection_record_index, 811);
    assert_eq!(operation.selection_next_record_index, 815);
    assert_eq!(
        operation
            .chain_records
            .iter()
            .map(|record| (
                record.record_index,
                record.class_tag.as_str(),
                record.frame_length
            ))
            .collect::<Vec<_>>(),
        vec![(815, "288", 11), (816, "271", 11)]
    );
    assert_eq!(operation.cell_table_record_index, 817);
    assert_eq!(operation.cell_table_class_tag.as_str(), "325");
    assert_eq!(operation.cell_table_paired_class_tag.as_str(), "257");
    assert_eq!(operation.cell_entries().len(), 2);
    assert_eq!(
        operation
            .cell_entries()
            .iter()
            .map(|entry| (entry.record_index, entry.ordinal))
            .collect::<Vec<_>>(),
        vec![(819, 1), (820, 4)]
    );
    assert_eq!(operation.trailing_value, 5);
    assert_eq!(
        operation.trailing_zero_offset,
        operation.trailing_value_offset + 4
    );
}

#[test]
fn surface_trim_rejects_cell_ordinal_outside_partition() {
    let (mut bytes, scope) = surface_trim_selection_and_cell_table();
    let table_start = bytes
        .windows(11)
        .position(|window| window == [3, 0, 0, 0, b'3', b'2', b'5', 0x31, 3, 0, 0])
        .expect("cell table header");
    let ordinal = table_start + 25 + 11;
    assert_eq!(
        bytes[ordinal..ordinal + 8],
        1u64.to_le_bytes(),
        "first cell ordinal"
    );
    bytes[ordinal..ordinal + 8].copy_from_slice(&6u64.to_le_bytes());
    assert!(exact_surface_trim_operation(
        &bytes,
        &crate::design::test_support::test_primary_frames(&bytes),
        &scope
    )
    .is_none());
}

#[test]
fn surface_trim_rejects_nonzero_cell_table_tail() {
    let (mut bytes, scope) = surface_trim_selection_and_cell_table();
    let table_start = bytes
        .windows(11)
        .position(|window| window == [3, 0, 0, 0, b'3', b'2', b'5', 0x31, 3, 0, 0])
        .expect("cell table header");
    let tail_zero = table_start + 67;
    bytes[tail_zero] = 1;
    assert!(exact_surface_trim_operation(
        &bytes,
        &crate::design::test_support::test_primary_frames(&bytes),
        &scope
    )
    .is_none());
}

#[test]
fn damaged_cell_table_gives_up_only_its_record_and_keeps_sibling() {
    const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let (mut damaged, mut damaged_scope) = surface_trim_selection_and_cell_table_with_base(0);
    let (good, mut good_scope) = surface_trim_selection_and_cell_table_with_base(1000);
    for scope in [&mut damaged_scope, &mut good_scope] {
        scope.id = format!(
            "{}:design-parameter-scope#{}",
            crate::ids::native_scope(ENTRY),
            scope.record_index
        );
    }
    let mut paired = Vec::new();
    indexed_header(&mut paired, *b"257", 817);
    let paired_at = damaged
        .windows(paired.len())
        .position(|window| window == paired)
        .unwrap();
    let tail = paired_at - std::mem::size_of::<u32>();
    damaged[tail] = 1;
    let good_start = damaged.len();
    damaged.extend_from_slice(&good);
    let records = [
        811, 812, 813, 814, 815, 816, 817, 819, 820, 1811, 1812, 1813, 1814, 1815, 1816, 1817,
        1819, 1820,
    ];
    let meta = crate::test_support::zip_test::indexed_fixture_metastream(
        &damaged,
        &records,
        &[811, 817, 1811, 1817],
    );
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(ENTRY, stored).unwrap();
    zip.write_all(&damaged).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&meta).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    let mut parts =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, &ctx);
    let operations = super::decode_surface_trim_operations(
        &ctx,
        &mut parts,
        &scan,
        &[damaged_scope.clone(), good_scope, damaged_scope],
    )
    .unwrap();
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].cell_table_record_index, 1817);
    let (_, native, fidelity, losses) = parts.into_parts();
    assert_eq!(
        native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
        1
    );
    assert!(fidelity
        .unrepresented()
        .contains(&cadmpeg_ir::document::ArenaClass::Design));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    let location = losses[0].provenance.as_ref().unwrap();
    assert_eq!(location.stream_name(), Some(ENTRY));
    assert!(location.offset < u64::try_from(good_start).unwrap());
    assert!(location.byte_len().is_some_and(|length| length > 0));
}
