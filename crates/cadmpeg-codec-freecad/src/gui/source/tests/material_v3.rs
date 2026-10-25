// SPDX-License-Identifier: Apache-2.0
//! The published v3 layout stores fixed records before independently recoverable strings.

use crate::native::entry_contents::EntryContents;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MaterialStringField {
    Image,
    ImagePath,
    Uuid,
}
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn payload() -> Vec<u8> {
    let mut bytes = 2_u32.to_le_bytes().to_vec();
    for _ in 0..2 {
        for color in [0x11223344_u32, 0x55667788, 0x99aabbcc, 0xddeeff00] {
            bytes.extend_from_slice(&color.to_le_bytes());
        }
        bytes.extend_from_slice(&0.5_f32.to_le_bytes());
        bytes.extend_from_slice(&0.25_f32.to_le_bytes());
    }
    for text in ["a", "b", "c", "d", "e", "f"] {
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(text.as_bytes());
    }
    bytes
}

fn decode(payload: &[u8]) -> cadmpeg_ir::codec::DecodeResult {
    let document = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='A' type='Part::Feature'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='0'/></Object></ObjectData></Document>";
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='1'><Property name='ShapeAppearance' type='App::PropertyMaterialList'><MaterialList file='materials' version='3'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("GuiDocument.xml", gui),
        ("materials", payload),
    ]);
    let result = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    crate::fidelity::b1::assert_classified_spans(&result);
    result
}

fn strings(result: &cadmpeg_ir::codec::DecodeResult) -> Vec<(usize, MaterialStringField, String)> {
    let entries = result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let values = entries
        .iter()
        .find(|entry| entry.name() == "materials")
        .unwrap()
        .contents()
        .iter()
        .find_map(|content| match content {
            EntryContents::Material { values, .. } => Some(values),
            _ => None,
        })
        .unwrap();
    assert_eq!(values.iter().filter(|slot| slot.fixed.is_some()).count(), 2);
    let mut held = Vec::new();
    for (order, slot) in values.iter().enumerate() {
        for (field, value) in [
            (MaterialStringField::Image, &slot.image),
            (MaterialStringField::ImagePath, &slot.image_path),
            (MaterialStringField::Uuid, &slot.uuid),
        ] {
            if let Some(value) = value {
                held.push((order, field, value.clone()));
            }
        }
    }
    held
}

#[test]
fn two_materials_keep_all_six_strings_in_material_and_field_order() {
    let result = decode(&payload());
    assert!(result
        .source_fidelity()
        .parts()
        .values()
        .all(|part| part.state() != &(PartState::GivenUp {})));
    assert_eq!(
        strings(&result),
        vec![
            (0, MaterialStringField::Image, "a".into()),
            (0, MaterialStringField::ImagePath, "b".into()),
            (0, MaterialStringField::Uuid, "c".into()),
            (1, MaterialStringField::Image, "d".into()),
            (1, MaterialStringField::ImagePath, "e".into()),
            (1, MaterialStringField::Uuid, "f".into()),
        ]
    );
    let parts = result.source_fidelity().parts();
    let mut fixed = parts
        .values()
        .filter(|part| part.level().as_str() == "freecad.material_list_material")
        .map(|part| (part.offset(), part.byte_len()))
        .collect::<Vec<_>>();
    fixed.sort_unstable();
    assert_eq!(fixed, [(4, 24), (28, 24)]);
    let mut text = parts
        .values()
        .filter(|part| part.level().as_str() == "freecad.material_list_string")
        .map(|part| {
            assert_eq!(
                parts[part.parent().unwrap()].level().as_str(),
                "freecad.material_list_entry"
            );
            assert_eq!(part.framing_bytes(), [part.offset()..part.offset() + 4]);
            (part.offset(), part.byte_len())
        })
        .collect::<Vec<_>>();
    text.sort_unstable();
    assert_eq!(text, [(52, 5), (57, 5), (62, 5), (67, 5), (72, 5), (77, 5)]);
    let plan = crate::FcstdCodec
        .plan(
            EncodeInput::new(result.ir(), None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &output,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    assert_eq!(archive.open(&ctx, "materials").unwrap().window(), payload());
}

#[test]
fn zeroed_second_string_loses_only_the_first_material_image_path() {
    let mut bytes = payload();
    bytes[61] = 0;
    let result = decode(&bytes);
    assert_eq!(
        strings(&result),
        vec![
            (0, MaterialStringField::Image, "a".into()),
            (0, MaterialStringField::Uuid, "c".into()),
            (1, MaterialStringField::Image, "d".into()),
            (1, MaterialStringField::ImagePath, "e".into()),
            (1, MaterialStringField::Uuid, "f".into()),
        ]
    );
    let failed = result
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &(PartState::GivenUp {}))
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level().as_str(), "freecad.material_list_string");
    assert_eq!((failed[0].offset(), failed[0].byte_len()), (57, 5));
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
    let source = losses[0].provenance.as_ref().unwrap();
    assert_eq!(source.stream_name(), Some(failed[0].stream()));
    assert_eq!((source.offset, source.byte_len()), (57, Some(5)));
}
