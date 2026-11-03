// SPDX-License-Identifier: Apache-2.0
//! Version-3 material-list split parts (#224).

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartOutput;

use super::super::{decode_material_v3, frame_material_v3};

/// Build a version-3 member: count, fixed records, then length-prefixed strings.
fn member(records: &[([u32; 4], [f32; 2])], strings: &[&str]) -> Vec<u8> {
    let mut bytes = (records.len() as u32).to_le_bytes().to_vec();
    for (colors, scalars) in records {
        for color in *colors {
            bytes.extend_from_slice(&color.to_le_bytes());
        }
        for scalar in *scalars {
            bytes.extend_from_slice(&scalar.to_le_bytes());
        }
    }
    for string in strings {
        bytes.extend_from_slice(&(string.len() as u32).to_le_bytes());
        bytes.extend_from_slice(string.as_bytes());
    }
    bytes
}

fn zeros() -> ([u32; 4], [f32; 2]) {
    (
        [0x1122_3344, 0x5566_7788, 0x99aa_bbcc, 0xddee_ff00],
        [0.5, 0.25],
    )
}

#[test]
fn version_three_frames_records_then_strings() {
    let bytes = member(&[zeros(), zeros()], &["a", "b", "c", "d", "e", "f"]);
    let arena = DecodeArena::new();
    let (_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (framed, cut) = frame_material_v3(root, "property").expect("healthy framing");
    assert!(cut.is_none());
    assert_eq!(framed.len(), 2);
    assert_eq!(framed[0].fixed, (4, 28));
    assert_eq!(framed[1].fixed, (28, 52));
    assert_eq!(
        framed[0].strings.as_ref().expect("strings"),
        &[(52, 57), (57, 62), (62, 67)]
    );
    assert_eq!(
        framed[1].strings.as_ref().expect("strings"),
        &[(67, 72), (72, 77), (77, 82)]
    );
}

#[test]
fn version_three_assigns_strings_in_record_order() {
    let bytes = member(&[zeros(), zeros()], &["a", "b", "c", "d", "e", "f"]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut stage = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd, &ctx);
    let materials = decode_material_v3(&ctx, &mut stage, root, "member.bin", "property", false)
        .expect("healthy list");
    assert_eq!(materials.len(), 2);
    assert_eq!(
        [
            &materials[0].image,
            &materials[0].image_path,
            &materials[0].uuid
        ],
        [&"a".to_owned(), &"b".to_owned(), &"c".to_owned()]
    );
    assert_eq!(
        [
            &materials[1].image,
            &materials[1].image_path,
            &materials[1].uuid
        ],
        [&"d".to_owned(), &"e".to_owned(), &"f".to_owned()]
    );
    assert_eq!(materials[0].diffuse, 0x5566_7788);
    let (_, _, fidelity, losses) = stage.into_parts();
    assert!(losses.is_empty());
    assert!(crate::test_support::source_fidelity::given_up(&fidelity).is_empty());
}

#[test]
fn damaged_string_gives_up_only_its_field() {
    let mut bytes = member(&[zeros(), zeros()], &["a", "b", "c", "d", "e", "f"]);
    // String 1 (material 0 image path) keeps its length word so later
    // extents stay computable, but its content is not UTF-8.
    bytes[57 + 4] = 0xff;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut stage = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd, &ctx);
    let materials = decode_material_v3(&ctx, &mut stage, root, "member.bin", "property", false)
        .expect("partial list");
    assert_eq!(materials.len(), 2);
    assert_eq!(materials[0].image, "a");
    assert_eq!(materials[0].image_path, "");
    assert_eq!(materials[0].uuid, "c");
    assert_eq!(materials[1].image, "d");
    assert_eq!(materials[1].image_path, "e");
    assert_eq!(materials[1].uuid, "f");
    let (_, _, fidelity, losses) = stage.into_parts();
    assert_eq!(losses.len(), 1);
    let loss = &losses[0];
    assert_eq!(loss.code.namespace(), "decode");
    assert_eq!(loss.code.local_code(), "record.unreadable");
    assert!(loss.message.contains("part freecad.material_list_string"));
    let given_up = crate::test_support::source_fidelity::given_up(&fidelity);
    assert_eq!(given_up.len(), 1);
}

#[test]
fn gui_appearance_decodes_eight_materials_without_give_ups() {
    use std::io::Cursor;

    use cadmpeg_ir::{Codec, DecodeOptions};

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/freecad_fcstd/fixtures/gui_appearance.FCStd");
    let bytes = std::fs::read(path).expect("public appearance fixture");
    let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).expect("readable fixture");
    let mut member = None;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("entry");
        let mut payload = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut payload).expect("inflate entry");
        if payload.len() == 292 && payload.first().is_some_and(|first| *first == 8) {
            member = Some(payload);
        }
    }
    let member = member.expect("eight-material version-3 member");
    let arena = DecodeArena::new();
    let (_ctx, root) =
        DecodeContext::from_root_bytes(&member, &arena, &DecodePolicy::service()).unwrap();
    let (framed, cut) = frame_material_v3(root, "ShapeAppearance").expect("healthy framing");
    assert!(cut.is_none());
    assert_eq!(framed.len(), 8);
    assert_eq!(framed.first().expect("first").fixed.0, 4);
    assert_eq!(framed.last().expect("last").fixed.1, 196);
    for material in &framed {
        for slot in material.strings.as_ref().expect("strings") {
            assert!((196..=292).contains(&slot.0));
            assert!((196..=292).contains(&slot.1));
        }
    }
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("appearance fixture");
    assert_eq!(
        decoded
            .ir()
            .model
            .appearances
            .iter()
            .filter(|appearance| appearance.schema.as_deref() == Some("FCStd ShapeAppearance"))
            .count(),
        8
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code.namespace() == "decode" && loss.code.local_code() == "record.unreadable"
    }));
}
