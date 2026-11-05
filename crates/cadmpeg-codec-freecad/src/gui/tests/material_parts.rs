// SPDX-License-Identifier: Apache-2.0
//! Version-3 material-list split parts (#224).

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartOutput;

use super::super::{decode_material_v3, frame_material_v3};

fn property() -> crate::native::GuiPropertyRecord {
    crate::native::GuiPropertyRecord {
        id: "fcstd:native:gui-property#provider:ShapeAppearance".into(),
        owner: "fcstd:native:gui-view-provider#provider".into(),
        name: "ShapeAppearance".into(),
        type_name: "App::PropertyMaterialList".into(),
        status: None,
        order: 0,
        values: Vec::new(),
        side_entries: vec!["member.bin".into()],
    }
}

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
    let framing = frame_material_v3(root, "property").expect("healthy framing");
    assert!(framing.cut.is_none());
    assert_eq!(framing.declared, 2);
    let framed = framing.records;
    assert_eq!(framed.len(), 2);
    assert_eq!(framed[0].fixed, (4, 28));
    assert_eq!(framed[1].fixed, (28, 52));
    assert_eq!(
        framed[0].strings,
        [Some((52, 57)), Some((57, 62)), Some((62, 67))]
    );
    assert_eq!(
        framed[1].strings,
        [Some((67, 72)), Some((72, 77)), Some((77, 82))]
    );
}

#[test]
fn version_three_assigns_strings_in_record_order() {
    let bytes = member(&[zeros(), zeros()], &["a", "b", "c", "d", "e", "f"]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut stage = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd, &ctx);
    let materials = decode_material_v3(
        &ctx,
        &mut stage,
        root,
        "member.bin",
        &property(),
        false,
        true,
    )
    .expect("healthy list");
    assert_eq!(materials.declared, 2);
    let materials: Vec<_> = materials
        .slots
        .iter()
        .map(|material| material.as_ref().unwrap())
        .collect();
    assert_eq!(
        [
            &materials[0].image,
            &materials[0].image_path,
            &materials[0].uuid
        ],
        [
            &Some("a".to_owned()),
            &Some("b".to_owned()),
            &Some("c".to_owned())
        ]
    );
    assert_eq!(
        [
            &materials[1].image,
            &materials[1].image_path,
            &materials[1].uuid
        ],
        [
            &Some("d".to_owned()),
            &Some("e".to_owned()),
            &Some("f".to_owned())
        ]
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
    let materials = decode_material_v3(
        &ctx,
        &mut stage,
        root,
        "member.bin",
        &property(),
        false,
        true,
    )
    .expect("partial list");
    assert_eq!(materials.declared, 2);
    let materials: Vec<_> = materials
        .slots
        .iter()
        .map(|material| material.as_ref().unwrap())
        .collect();
    assert_eq!(materials[0].image.as_deref(), Some("a"));
    assert_eq!(materials[0].image_path, None);
    assert_eq!(materials[0].uuid.as_deref(), Some("c"));
    assert_eq!(materials[1].image.as_deref(), Some("d"));
    assert_eq!(materials[1].image_path.as_deref(), Some("e"));
    assert_eq!(materials[1].uuid.as_deref(), Some("f"));
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
    let framing = frame_material_v3(root, "ShapeAppearance").expect("healthy framing");
    assert!(framing.cut.is_none());
    assert_eq!(framing.declared, 8);
    let framed = framing.records;
    assert_eq!(framed.len(), 8);
    assert_eq!(framed.first().expect("first").fixed.0, 4);
    assert_eq!(framed.last().expect("last").fixed.1, 196);
    for material in &framed {
        for slot in &material.strings {
            let slot = slot.expect("string extent");
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

#[test]
fn damaged_material_leaves_its_slot_empty_and_keeps_the_next_material() {
    let mut bytes = member(&[zeros(), zeros()], &["a", "b", "c", "d", "e", "f"]);
    bytes[20..24].copy_from_slice(&f32::NAN.to_le_bytes());
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut stage = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd, &ctx);
    let materials = decode_material_v3(
        &ctx,
        &mut stage,
        root,
        "member.bin",
        &property(),
        false,
        true,
    )
    .unwrap();
    assert_eq!(materials.declared, 2);
    assert!(materials.slots[0].is_none());
    let next = materials.slots[1]
        .as_ref()
        .expect("second material remains readable");
    assert_eq!(next.diffuse, 0x5566_7788);
    assert_eq!(next.image.as_deref(), Some("d"));
    let (_, _, fidelity, losses) = stage.into_parts();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert_eq!(
        crate::test_support::source_fidelity::given_up(&fidelity).len(),
        1
    );
}

#[test]
fn truncated_fixed_section_preserves_complete_prefix_and_parent_link() {
    let mut bytes = member(&[zeros(), zeros()], &["a", "b", "c", "d", "e", "f"]);
    bytes.truncate(4 + 24 + 8);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut stage = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd, &ctx);
    let decoded = decode_material_v3(
        &ctx,
        &mut stage,
        root,
        "member.bin",
        &property(),
        false,
        true,
    )
    .unwrap();
    assert_eq!(decoded.declared, 2);
    assert_eq!(decoded.slots.len(), 1);
    let held = decoded.slots[0].as_ref().unwrap();
    assert_eq!(held.diffuse, 0x5566_7788);
    assert!(held.uuid.is_none());
    let (_, native, fidelity, losses) = stage.into_parts();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("freecad.material_list_material"));
    assert!(losses[0].dropped_fields.iter().any(|field| field.record
        == "fcstd:appearance:shape-material#provider:1"
        && field.field == "asset_guid"));
    let stubs = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .unwrap();
    assert_eq!(stubs.len(), 1);
    assert!(stubs[0]
        .links
        .iter()
        .any(|link| link.as_str() == property().id));
    let given_up = crate::test_support::source_fidelity::given_up(&fidelity);
    assert_eq!(given_up.len(), 1);
    assert_eq!(given_up[0].1.data(), Some(&bytes[28..]));
}

#[test]
fn truncated_string_tail_does_not_invent_later_string_extents() {
    let mut bytes = member(&[zeros(), zeros()], &["a", "b", "c", "d", "e", "f"]);
    bytes.truncate(58);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut stage = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd, &ctx);
    let decoded = decode_material_v3(
        &ctx,
        &mut stage,
        root,
        "member.bin",
        &property(),
        false,
        true,
    )
    .unwrap();
    assert_eq!(decoded.declared, 2);
    assert_eq!(
        decoded.slots[0].as_ref().unwrap().image.as_deref(),
        Some("a")
    );
    assert!(decoded.slots[0].as_ref().unwrap().image_path.is_none());
    assert!(decoded.slots[0].as_ref().unwrap().uuid.is_none());
    assert!(decoded.slots[1].as_ref().unwrap().image.is_none());
    let (_, _, fidelity, losses) = stage.into_parts();
    assert_eq!(losses.len(), 1);
    assert_eq!(
        crate::test_support::source_fidelity::given_up(&fidelity).len(),
        1
    );
}

#[test]
fn damaged_uuid_names_the_derived_appearance_field_and_property_parent() {
    let mut bytes = member(&[zeros()], &["a", "b", "c"]);
    bytes[42] = 0xff;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut stage = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd, &ctx);
    let decoded = decode_material_v3(
        &ctx,
        &mut stage,
        root,
        "member.bin",
        &property(),
        false,
        true,
    )
    .unwrap();
    assert_eq!(decoded.slots[0].as_ref().unwrap().uuid, None);
    let (_, native, _, losses) = stage.into_parts();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].dropped_fields.iter().any(|field| field.record
        == "fcstd:appearance:shape-material#provider:1"
        && field.field == "asset_guid"));
    let stubs = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .unwrap();
    assert!(stubs[0]
        .links
        .iter()
        .any(|link| link.as_str() == property().id));
}
