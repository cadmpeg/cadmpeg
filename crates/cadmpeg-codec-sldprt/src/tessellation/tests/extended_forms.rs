// SPDX-License-Identifier: Apache-2.0
use super::class;
use crate::container::{Block, BlockName, PayloadFamily, Section};
use crate::tessellation::{section_display_faces, summary_for_faces, DisplayFace, Summary};
use crate::test_support::tessellation::descriptor;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn record(word: u32, abc: [u32; 3], normals: &[[f32; 3]]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in [1_u32, 1, 1, 0, 0, word, 0, 0, 0, 0] {
        out.extend(value.to_le_bytes());
    }
    out.extend(descriptor(4, 8, 1, &3_u32.to_le_bytes()));
    let positions = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    out.extend(descriptor(12, 100, 3, &positions));
    let normal_bytes = normals
        .iter()
        .flatten()
        .copied()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    out.extend(descriptor(
        12,
        100,
        u32::try_from(normals.len()).unwrap(),
        &normal_bytes,
    ));
    out.extend(descriptor(4, 8, 4, &[0; 16]));
    out.extend(descriptor(4, 8, 1, &4_u32.to_le_bytes()));
    out.extend(descriptor(1, 8, 4, &[0; 4]));
    let mut metadata = [0_u8; 128];
    for (offset, value) in [
        (92, abc[0]),
        (96, 1),
        (100, 0x1020_3040),
        (104, abc[1]),
        (108, abc[2]),
    ] {
        metadata[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    if abc[1] == 1 {
        metadata[112..116].copy_from_slice(&0.5_f32.to_le_bytes());
        metadata[116..120].copy_from_slice(&1.0_f32.to_le_bytes());
    }
    out.extend(metadata);
    out
}

fn varying() -> [[f32; 3]; 3] {
    [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]
}

fn decode(records: &[Vec<u8>]) -> (Vec<DisplayFace>, Summary) {
    let mut payload = Vec::new();
    class(&mut payload, "uoTempFaceTessData_c", &[]);
    for record in records {
        payload.extend(record);
    }
    let block = Block {
        offset: 0,
        type_id: 0x41,
        comp_sz: 0,
        section: BlockName::Named(cadmpeg_ir::stream_name!("Contents/DisplayLists")),
        family: PayloadFamily::Tessellation,
        payload,
        ps_streams: Vec::new(),
        ps_probe_capped: false,
    };
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&block.payload, &arena, &DecodePolicy::service()).unwrap();
    let faces = section_display_faces(&ctx, Section::Block(&block)).expect("valid geometry");
    let summary = summary_for_faces(&block.payload, &faces);
    (faces, summary)
}

fn set_metadata(record: &mut [u8], offset: usize, bytes: &[u8]) {
    let start = record.len() - 128 + offset;
    record[start..start + bytes.len()].copy_from_slice(bytes);
}

fn assert_opaque(record: Vec<u8>) {
    let (faces, summary) = decode(&[record]);
    assert_eq!(faces.len(), 1);
    assert_eq!(summary.vertices, 3);
    assert_eq!(summary.triangles, 1);
    assert_eq!(summary.extended_forms, [0; 4]);
    assert_eq!(summary.opaque_extended_tables, 1);
    assert_eq!(faces[0].mesh.channels.len(), 6);
}

#[test]
fn four_extended_forms_require_matching_metadata_and_normal_bits() {
    let records = vec![
        record(0x0020_1296, [0, 0, 0], &varying()),
        record(0x0020_1a96, [0, 0, 1], &varying()),
        record(0x0020_1b96, [1, 0, 1], &[[0.0, 0.0, 1.0]; 3]),
        record(0x0020_1e96, [0, 1, 1], &varying()),
    ];
    let (faces, summary) = decode(&records);
    assert_eq!(faces.len(), 4);
    assert_eq!(summary.extended_forms, [1; 4]);
    assert_eq!(summary.opaque_extended_tables, 0);
    assert_eq!(summary.triangles, 4);
    for face in faces {
        assert_eq!(face.mesh.channels.len(), 6);
        assert_eq!(face.mesh.channels[2].data().len(), 36);
    }
}

#[test]
fn independent_metadata_word_does_not_select_the_form() {
    for value in [0_u32, 1, u32::MAX] {
        let mut bytes = record(0x0020_1a96, [0, 0, 1], &varying());
        set_metadata(&mut bytes, 100, &value.to_le_bytes());
        assert_eq!(decode(&[bytes]).1.extended_forms, [0, 1, 0, 0]);
    }
}

#[test]
fn unknown_forms_and_inconsistent_cells_preserve_geometry() {
    assert_opaque(record(0x0020_ffff, [0, 0, 1], &varying()));
    for abc in [
        [2, 0, 1],
        [0, 2, 1],
        [0, 0, 2],
        [1, 1, 1],
        [1, 0, 0],
        [0, 1, 0],
    ] {
        assert_opaque(record(0x0020_1a96, abc, &varying()));
    }
    assert_opaque(record(0x0020_1296, [0, 0, 1], &varying()));
    assert_opaque(record(0x0020_1b96, [1, 0, 1], &varying()));
    assert_opaque(record(0x0020_1a96, [0, 0, 1], &[[0.0, 0.0, 1.0]; 3]));
    for (offset, value) in [(96, 0_u32), (112, 1), (124, 0x8000_0000)] {
        let mut bytes = record(0x0020_1a96, [0, 0, 1], &varying());
        set_metadata(&mut bytes, offset, &value.to_le_bytes());
        assert_opaque(bytes);
    }
    let mut bytes = record(0x0020_1a96, [0, 0, 1], &varying());
    bytes.truncate(bytes.len() - 1);
    assert_opaque(bytes);
}

#[test]
fn extended_vector_form_requires_a_positive_finite_scalar_and_unit_vector() {
    for (offset, value) in [
        (112, 0.0_f32),
        (112, -1.0),
        (112, f32::NAN),
        (112, f32::INFINITY),
        (116, 0.5),
        (116, f32::NAN),
        (120, f32::INFINITY),
    ] {
        let mut bytes = record(0x0020_1e96, [0, 1, 1], &varying());
        set_metadata(&mut bytes, offset, &value.to_le_bytes());
        assert_opaque(bytes);
    }
}

#[test]
fn normal_constancy_compares_signed_zero_bits() {
    let normals = [[0.0, 0.0, 1.0], [-0.0, 0.0, 1.0], [0.0, 0.0, 1.0]];
    assert_eq!(
        decode(&[record(0x0020_1a96, [0, 0, 1], &normals)])
            .1
            .extended_forms,
        [0, 1, 0, 0]
    );
    assert_opaque(record(0x0020_1b96, [1, 0, 1], &normals));
}

#[test]
fn truncated_metadata_cannot_borrow_the_next_table_header() {
    let mut first = record(0x0020_1e96, [0, 1, 1], &varying());
    first.truncate(first.len() - 8);
    let second = record(0x0020_1a96, [0, 0, 1], &varying());
    let (faces, summary) = decode(&[first, second]);
    assert_eq!(faces.len(), 2);
    assert_eq!(summary.extended_forms, [0, 1, 0, 0]);
    assert_eq!(summary.opaque_extended_tables, 1);
    assert_eq!(summary.triangles, 2);
}

#[test]
fn absent_normal_lane_preserves_unshaded_geometry_and_raw_channel() {
    let (faces, summary) = decode(&[record(0x0020_1a96, [0, 0, 1], &[])]);
    assert_eq!(faces.len(), 1);
    assert_eq!(summary.vertices, 3);
    assert_eq!(summary.triangles, 1);
    assert_eq!(summary.extended_forms, [0; 4]);
    assert_eq!(summary.opaque_extended_tables, 1);
    assert!(faces[0].mesh.channels[2].data().is_empty());
}

#[test]
fn contradictory_later_header_still_bounds_the_previous_metadata() {
    let mut first = record(0x0020_1e96, [0, 1, 1], &varying());
    first.truncate(first.len() - 8);
    let mut second = record(0x0020_1a96, [0, 0, 1], &varying());
    second[..4].copy_from_slice(&2_u32.to_le_bytes());
    let (faces, summary) = decode(&[first, second]);
    assert_eq!(faces.len(), 2);
    assert_eq!(summary.triangles, 2);
    assert_eq!(summary.extended_forms, [0; 4]);
    assert_eq!(summary.opaque_extended_tables, 2);
}
