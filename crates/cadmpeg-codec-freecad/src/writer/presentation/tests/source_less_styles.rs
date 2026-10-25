// SPDX-License-Identifier: Apache-2.0
//! Real source-less GUI values and unbound material slots retain their typed owners.

use crate::native::entry_contents::EntryContents;
use crate::native::GuiPropertyRecord;
use crate::test_support::test_archive::archive_entries;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn source() -> Vec<u8> {
    let document=b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object type='App::FeaturePython' name='A'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='0'/></Object></ObjectData></Document>";
    let gui=b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='5'><Property name='Text' type='App::PropertyString' status='17'><String value='original'/></Property><Property name='Number' type='App::PropertyFloat'><Float value='2.5'/></Property><Property name='ShapeColor' type='App::PropertyColor'><PropertyColor value='287454016'/></Property><Property name='ShapeAppearance' type='App::PropertyMaterialList'><MaterialList file='materials' version='3'/></Property><Property name='DiffuseColor' type='App::PropertyColorList'><ColorList file='colors'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let mut materials = 2_u32.to_le_bytes().to_vec();
    for _ in 0..2 {
        for color in [0x11223344_u32, 0x556677aa, 0x8899aacc, 0xddeeff00] {
            materials.extend(color.to_le_bytes());
        }
        materials.extend(0.5_f32.to_le_bytes());
        materials.extend(0.25_f32.to_le_bytes());
    }
    for index in 0..2 {
        for text in [
            format!("image{index}"),
            format!("path{index}"),
            format!("uuid{index}"),
        ] {
            materials.extend(u32::try_from(text.len()).unwrap().to_le_bytes());
            materials.extend(text.as_bytes());
        }
    }
    let mut colors = 2_u32.to_le_bytes().to_vec();
    colors.extend(0x11223344_u32.to_le_bytes());
    colors.extend(0x55667788_u32.to_le_bytes());
    archive_entries(&[
        ("Document.xml", document),
        ("GuiDocument.xml", gui),
        ("materials", &materials),
        ("colors", &colors),
    ])
}
fn decode(bytes: &[u8]) -> cadmpeg_ir::codec::DecodeResult {
    crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}
fn roundtrip(ir: &cadmpeg_ir::CadIr) -> cadmpeg_ir::codec::DecodeResult {
    let plan = crate::FcstdCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let result = decode(&bytes);
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    assert!(
        result
            .source_fidelity()
            .parts()
            .values()
            .all(|part| !matches!(
                part.state(),
                cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
            )),
        "{:#?}",
        result.report().losses
    );
    result
}

#[test]
fn source_less_gui_scalars_and_unbound_materials_survive_without_a_generic_view_cache() {
    let before = decode(&source());
    assert!(before.ir().model.appearance_bindings.is_empty());
    assert!(before
        .ir()
        .model
        .view_presentations
        .iter()
        .all(|view| view.properties.is_empty()));
    let mut current = before.ir().clone();
    let namespace = current.native.namespace_mut("fcstd");
    let mut properties = namespace
        .arena_as::<GuiPropertyRecord>("gui_properties")
        .unwrap();
    properties
        .iter_mut()
        .find(|property| property.name == "Text")
        .unwrap()
        .values[0]
        .attributes
        .insert("value".into(), "current text".into());
    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "gui_properties",
            &properties,
        )
        .unwrap();
    let after = roundtrip(&current);
    let namespace = after.ir().native.namespace("fcstd").unwrap();
    let properties = namespace
        .arena_as::<GuiPropertyRecord>("gui_properties")
        .unwrap();
    let text = properties
        .iter()
        .find(|property| property.name == "Text")
        .unwrap();
    assert_eq!(text.status, Some(17));
    assert_eq!(text.values[0].attributes["value"], "current text");
    assert_eq!(
        properties
            .iter()
            .find(|property| property.name == "Number")
            .unwrap()
            .values[0]
            .attributes["value"],
        "2.5"
    );
    assert_eq!(
        properties
            .iter()
            .find(|property| property.name == "ShapeColor")
            .unwrap()
            .values[0]
            .attributes["value"],
        "287454016"
    );
    assert!(!properties
        .iter()
        .any(|property| property.name == "Transparency"));
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    assert_eq!(
        entries
            .iter()
            .flat_map(crate::native::EntryRecord::contents)
            .filter_map(|contents| match contents {
                EntryContents::Material { values, .. } =>
                    Some(values.iter().filter(|slot| slot.fixed.is_some()).count()),
                _ => None,
            })
            .sum::<usize>(),
        2
    );
    assert_eq!(
        entries
            .iter()
            .flat_map(crate::native::EntryRecord::contents)
            .filter_map(|contents| match contents {
                EntryContents::Color { values } => Some(values.iter().flatten().count()),
                _ => None,
            })
            .sum::<usize>(),
        2
    );
}

#[test]
fn current_unbound_material_and_color_assets_update_their_proven_native_slots() {
    let before = decode(&source());
    let mut current = before.ir().clone();
    let entries = current
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let material = entries
        .iter()
        .find(|entry| entry.name() == "materials")
        .unwrap();
    let asset = current
        .model
        .appearances
        .iter_mut()
        .find(|asset| {
            asset.native_ref.as_deref() == Some(material.id())
                && asset.asset_guid.as_deref() == Some("uuid0")
        })
        .unwrap();
    asset.base_color = Some(
        cadmpeg_ir::topology::Color::new(200.0 / 255.0, 40.0 / 255.0, 90.0 / 255.0, 0.75).unwrap(),
    );
    asset.properties.insert(
        cadmpeg_core::nonblank_literal!("shininess"),
        cadmpeg_ir::scalar::FiniteReal::new(0.875).unwrap(),
    );
    asset.asset_guid = Some("current-uuid".into());
    let color = entries
        .iter()
        .find(|entry| entry.name() == "colors")
        .unwrap();
    current
        .model
        .appearances
        .iter_mut()
        .find(|asset| {
            asset.native_ref.as_deref() == Some(color.id())
                && asset.base_color
                    == Some(cadmpeg_ir::topology::Color::from_rgba8(
                        0x11, 0x22, 0x33, 0x44,
                    ))
        })
        .unwrap()
        .base_color = Some(cadmpeg_ir::topology::Color::from_rgba8(10, 20, 30, 77));
    let after = roundtrip(&current);
    let entries = after
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let materials = entries
        .iter()
        .flat_map(crate::native::EntryRecord::contents)
        .find_map(|content| match content {
            EntryContents::Material { values, .. } => Some(values),
            _ => None,
        })
        .unwrap();
    let material = materials[0].fixed.as_ref().unwrap();
    assert_eq!(material.diffuse, 0xc8285aaa);
    assert_eq!(material.shininess.get(), 0.875);
    assert!(materials
        .iter()
        .any(|slot| slot.uuid.as_deref() == Some("current-uuid")));
    assert!(entries.iter().flat_map(crate::native::EntryRecord::contents).any(|content|matches!(content,EntryContents::Color {values} if values.iter().flatten().any(|value|*value==0x0a141e4d))));
}

#[test]
fn deleting_unbound_current_assets_drops_only_their_proven_native_list_slots() {
    let before = decode(&source());
    let mut current = before.ir().clone();
    let entries = current
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let material = entries
        .iter()
        .find(|entry| entry.name() == "materials")
        .unwrap();
    let color = entries
        .iter()
        .find(|entry| entry.name() == "colors")
        .unwrap();
    let mut deleted = std::collections::BTreeSet::new();
    current.model.appearances.retain(|asset| {
        let keep = !(asset.native_ref.as_deref() == Some(material.id())
            && asset.asset_guid.as_deref() == Some("uuid0"))
            && !(asset.native_ref.as_deref() == Some(color.id())
                && asset.base_color
                    == Some(cadmpeg_ir::topology::Color::from_rgba8(
                        0x11, 0x22, 0x33, 0x44,
                    )));
        if !keep {
            deleted.insert(asset.id.clone());
        }
        keep
    });
    current.model.attributes.retain(|attribute| {
        !(matches!(attribute.values.as_slice(), [cadmpeg_ir::attributes::AttributeValue::Map(_)])
            && matches!(&attribute.target,
                cadmpeg_ir::attributes::AttributeTarget::Record { arena, id }
                if arena.as_str() == "appearances" && deleted.iter().any(|deleted| deleted.as_str() == id)))
    });
    let after = roundtrip(&current);
    let entries = after
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let materials = entries
        .iter()
        .flat_map(crate::native::EntryRecord::contents)
        .find_map(|content| match content {
            EntryContents::Material { values, .. } => Some(values),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        materials.iter().filter(|slot| slot.fixed.is_some()).count(),
        1
    );
    assert_eq!(
        entries
            .iter()
            .flat_map(crate::native::EntryRecord::contents)
            .filter_map(|content| match content {
                EntryContents::Color { values } => Some(values.iter().flatten().count()),
                _ => None,
            })
            .sum::<usize>(),
        1
    );
    assert!(materials
        .iter()
        .any(|slot| slot.uuid.as_deref() == Some("uuid1")));
    assert!(!materials
        .iter()
        .any(|slot| slot.uuid.as_deref() == Some("uuid0")));
}
