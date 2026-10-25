// SPDX-License-Identifier: Apache-2.0
//! Entry ownership does not supply a hidden list slot or provider identity.
use super::*;
use crate::test_support::test_archive::archive_entries;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn source(aliases: bool, material: bool) -> cadmpeg_ir::CadIr {
    source_version(aliases, material, 3)
}
fn source_version(aliases: bool, material: bool, version: u32) -> cadmpeg_ir::CadIr {
    let document = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='2'><Object type='App::Feature' name='A'/><Object type='App::Feature' name='B'/></Objects><ObjectData Count='2'><Object name='A'><Properties Count='0'/></Object><Object name='B'><Properties Count='0'/></Object></ObjectData></Document>";
    let (name, kind, value) = if material {
        (
            "ShapeAppearance",
            "App::PropertyMaterialList",
            format!("<MaterialList version='{version}' file='palette.bin'/>"),
        )
    } else {
        (
            "DiffuseColor",
            "App::PropertyColorList",
            "<ColorList file='palette.bin'/>".into(),
        )
    };
    let provider = |object: &str| {
        format!("<ViewProvider name='{object}'><Properties Count='1'><Property name='{name}' type='{kind}'>{value}</Property></Properties></ViewProvider>")
    };
    let providers = if aliases {
        format!("{}{}", provider("A"), provider("B"))
    } else {
        provider("A")
    };
    let gui = format!("<Document SchemaVersion='1'><ViewProviderData Count='{}'>{providers}</ViewProviderData></Document>", if aliases { 2 } else { 1 });
    let mut palette = 2_u32.to_le_bytes().to_vec();
    for diffuse in [0x1020_30ff_u32, 0x4050_60ff] {
        if material {
            for color in [0x1111_11ff, diffuse, 0x2222_22ff, 0x3333_33ff] {
                palette.extend_from_slice(&color.to_le_bytes());
            }
            palette.extend_from_slice(&0.5_f32.to_le_bytes());
            palette.extend_from_slice(&0.0_f32.to_le_bytes());
        } else {
            palette.extend_from_slice(&diffuse.to_le_bytes());
        }
    }
    if material && version == 3 {
        palette.extend_from_slice(&[0_u8; 24]);
    }
    let bytes = archive_entries(&[
        ("Document.xml", document),
        ("GuiDocument.xml", gui.as_bytes()),
        ("palette.bin", &palette),
    ]);
    crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone()
}

fn records(ir: &cadmpeg_ir::CadIr) -> (crate::native::GuiPropertyRecord, EntryRecord) {
    let namespace = ir.native.namespace("fcstd").unwrap();
    let property = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .unwrap()
        .remove(0);
    let entry = namespace
        .arena_as::<EntryRecord>("entries")
        .unwrap()
        .into_iter()
        .find(|entry| entry.name() == "palette.bin")
        .unwrap();
    (property, entry)
}

#[test]
fn current_color_order_is_independent_of_appearance_id_spelling() {
    let mut ir = source(false, false);
    assert_eq!(ir.model.appearances.len(), 2);
    assert!(ir.model.appearance_bindings.is_empty());
    ir.model.appearances[0].id =
        cadmpeg_ir::ids::AppearanceId::mint("authored:model:appearance#z").unwrap();
    ir.model.appearances[1].id =
        cadmpeg_ir::ids::AppearanceId::mint("authored:model:appearance#a").unwrap();
    let (property, entry) = records(&ir);
    let mut current = entry.contents()[0].clone();
    let mut losses = Vec::new();
    assert!(rewrite(&ir, &property, &entry, &mut current, &mut losses).unwrap());
    assert_eq!(
        current,
        EntryContents::Color {
            values: vec![Some(0x1020_30ff), Some(0x4050_60ff)]
        }
    );
    assert!(losses.is_empty());
    let plan = crate::FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let (_, actual) = records(output.ir());
    assert_eq!(actual.contents()[0], current);
}

#[test]
fn shared_entry_without_bindings_does_not_duplicate_its_assets() {
    let ir = source(true, false);
    assert_eq!(ir.model.appearances.len(), 4);
    assert!(ir.model.appearance_bindings.is_empty());
    let (property, entry) = records(&ir);
    let original = entry.contents()[0].clone();
    let mut current = original.clone();
    let mut losses = Vec::new();
    assert!(!rewrite(&ir, &property, &entry, &mut current, &mut losses).unwrap());
    assert_eq!(current, original);
    // The final presentation writer reports omission only after giving actual
    // topology bindings an opportunity to supply a current list.
    assert!(losses.is_empty());
    let plan = crate::FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "source.undecoded-omitted"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.appearances.is_empty());
}

#[test]
fn current_material_order_survives_arbitrary_asset_ids_and_map_targets() {
    let mut ir = source(false, true);
    assert_eq!(ir.model.appearances.len(), 2);
    assert!(ir.model.appearance_bindings.is_empty());
    for (index, spelling) in ["authored:model:appearance#z", "authored:model:appearance#a"]
        .into_iter()
        .enumerate()
    {
        let previous = ir.model.appearances[index].id.clone();
        let current = cadmpeg_ir::ids::AppearanceId::mint(spelling).unwrap();
        ir.model.appearances[index].id = current.clone();
        for attribute in &mut ir.model.attributes {
            if let cadmpeg_ir::attributes::AttributeTarget::Record { arena, id } =
                &mut attribute.target
            {
                if arena.as_str() == "appearances" && id.as_str() == previous.as_str() {
                    *id = current.as_str().to_owned();
                }
            }
        }
    }
    let (property, entry) = records(&ir);
    let mut current = entry.contents()[0].clone();
    let mut losses = Vec::new();
    assert!(rewrite(&ir, &property, &entry, &mut current, &mut losses).unwrap());
    assert_eq!(current, entry.contents()[0]);
    let plan = crate::FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let (_, actual) = records(output.ir());
    assert_eq!(actual.contents()[0], current);
}

#[test]
fn authored_material_map_keys_have_named_omissions_without_losing_known_fields() {
    let mut ir = source(false, true);
    let appearance = ir.model.appearances[0].id.clone();
    let attribute = ir.model.attributes.iter_mut().find(|attribute| {
        matches!(&attribute.target, cadmpeg_ir::attributes::AttributeTarget::Record { arena, id } if arena.as_str() == "appearances" && id == appearance.as_str())
    }).unwrap();
    let attribute_id = attribute.id.to_string();
    let cadmpeg_ir::attributes::AttributeValue::Map(values) = &mut attribute.values[0] else {
        panic!("material strings are a Map");
    };
    values.push(cadmpeg_ir::attributes::AttributeMapEntry {
        key: "authored-extra".into(),
        value: "retain-known-fields".into(),
    });
    let (property, entry) = records(&ir);
    let mut losses = Vec::new();
    let actual = material(
        &ir,
        &ir.model.appearances[0],
        &property.name,
        3,
        &mut losses,
    )
    .unwrap()
    .unwrap();
    let EntryContents::Material { values, .. } = &entry.contents()[0] else {
        panic!("material entry");
    };
    assert_eq!(actual, values[0]);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains(&attribute_id));
    assert!(losses[0].message.contains("authored-extra"));
}

fn author_strings(ir: &mut cadmpeg_ir::CadIr, index: usize, keys: &[(&str, &str)]) -> String {
    let (_, entry) = records(ir);
    let arena = cadmpeg_ir::document::ArenaName::ALL
        .iter()
        .copied()
        .find(|arena| arena.as_str() == "appearances")
        .unwrap();
    let id = cadmpeg_ir::ids::AttributeId::mint(format!("authored:model:attribute#strings{index}"))
        .unwrap();
    let name = id.to_string();
    ir.model
        .attributes
        .push(cadmpeg_ir::attributes::SourceAttribute {
            id,
            native_ref: Some(entry.id().to_owned()),
            target: cadmpeg_ir::attributes::AttributeTarget::Record {
                arena,
                id: ir.model.appearances[index].id.as_str().to_owned(),
            },
            name: "ShapeAppearance".into(),
            values: vec![cadmpeg_ir::attributes::AttributeValue::Map(
                keys.iter()
                    .map(|(key, value)| cadmpeg_ir::attributes::AttributeMapEntry {
                        key: (*key).into(),
                        value: (*value).into(),
                    })
                    .collect(),
            )],
        });
    name
}
fn encoded_material(
    ir: &cadmpeg_ir::CadIr,
) -> (crate::native::GuiPropertyRecord, EntryRecord, Vec<LossNote>) {
    let plan = crate::FcstdCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .unwrap();
    let losses = plan.report().losses.clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let (property, entry) = records(decoded.ir());
    (property, entry, losses)
}
#[test]
fn legacy_material_with_three_authored_strings_promotes_member_and_xml_together() {
    let mut ir = source_version(false, true, 2);
    assert!(ir.model.attributes.is_empty());
    author_strings(
        &mut ir,
        0,
        &[("image", "first"), ("image_path", "/first"), ("uuid", "")],
    );
    author_strings(
        &mut ir,
        1,
        &[("image", "second"), ("image_path", "/second"), ("uuid", "")],
    );
    let (property, entry, losses) = encoded_material(&ir);
    assert_eq!(
        property.values[0]
            .attributes
            .get("version")
            .map(String::as_str),
        Some("3")
    );
    let EntryContents::Material { version, values } = &entry.contents()[0] else {
        panic!("material entry");
    };
    assert_eq!(*version, 3);
    assert_eq!(
        values
            .iter()
            .map(|value| value.image.as_deref())
            .collect::<Vec<_>>(),
        [Some("first"), Some("second")]
    );
    assert_eq!(
        values
            .iter()
            .map(|value| value.image_path.as_deref())
            .collect::<Vec<_>>(),
        [Some("/first"), Some("/second")]
    );
    assert!(values.iter().all(|value| value.uuid.as_deref() == Some("")));
    assert!(!losses
        .iter()
        .any(|loss| loss.message.contains("legacy known absence")));
}
#[test]
fn partial_authored_legacy_strings_report_the_actual_added_empty_channels() {
    let mut ir = source_version(false, true, 1);
    let attribute = author_strings(&mut ir, 0, &[("image", "current-image")]);
    let (property, entry, losses) = encoded_material(&ir);
    assert_eq!(
        property.values[0]
            .attributes
            .get("version")
            .map(String::as_str),
        Some("3")
    );
    let EntryContents::Material { version, values } = &entry.contents()[0] else {
        panic!("material entry");
    };
    assert_eq!(*version, 3);
    assert_eq!(values[0].image.as_deref(), Some("current-image"));
    assert_eq!(values[0].image_path.as_deref(), Some(""));
    assert_eq!(values[0].uuid.as_deref(), Some(""));
    let note = losses
        .iter()
        .find(|loss| {
            loss.message.contains(&attribute) && loss.message.contains("legacy known absence")
        })
        .unwrap();
    assert!(note.message.contains("image_path"));
    assert!(note.message.contains("uuid"));
}

#[test]
fn authored_nul_material_string_omits_only_its_unbound_material() {
    let mut ir = source(false, true);
    let (_, entry) = records(&ir);
    let EntryContents::Material { values, .. } = &entry.contents()[0] else {
        panic!("material entry");
    };
    let healthy = values[1].clone();
    let first = ir.model.appearances[0].id.clone();
    let attribute=ir.model.attributes.iter_mut().find(|attribute|matches!(&attribute.target,cadmpeg_ir::attributes::AttributeTarget::Record{arena,id} if arena.as_str()=="appearances" && id==first.as_str())).unwrap();
    let id = attribute.id.to_string();
    let cadmpeg_ir::attributes::AttributeValue::Map(values) = &mut attribute.values[0] else {
        panic!("material Map");
    };
    values
        .iter_mut()
        .find(|value| value.key == "image")
        .unwrap()
        .value = "bad\0image".into();
    let (_, entry, losses) = encoded_material(&ir);
    let EntryContents::Material { values, .. } = &entry.contents()[0] else {
        panic!("material entry");
    };
    assert_eq!(values.as_slice(), [healthy].as_slice());
    assert!(losses.iter().any(|loss| loss.message.contains(&id)
        && loss.message.contains("image")
        && loss.message.contains("NUL")));
}

#[test]
fn deleting_all_material_maps_and_guids_keeps_numeric_materials_in_version_two() {
    let mut ir = source(false, true);
    let (_, original) = records(&ir);
    let EntryContents::Material {
        values: original_values,
        ..
    } = &original.contents()[0]
    else {
        panic!("material entry");
    };
    let fixed = original_values
        .iter()
        .map(|value| value.fixed.clone())
        .collect::<Vec<_>>();
    assert_eq!(ir.model.attributes.len(), 2);
    ir.model.attributes.clear();
    for appearance in &mut ir.model.appearances {
        appearance.asset_guid = None;
    }
    let (property, entry, losses) = encoded_material(&ir);
    assert_eq!(
        property.values[0]
            .attributes
            .get("version")
            .map(String::as_str),
        Some("2")
    );
    let EntryContents::Material { version, values } = &entry.contents()[0] else {
        panic!("material entry");
    };
    assert_eq!(*version, 2);
    assert_eq!(
        values
            .iter()
            .map(|value| value.fixed.clone())
            .collect::<Vec<_>>(),
        fixed
    );
    assert!(values
        .iter()
        .all(|value| value.image.is_none() && value.image_path.is_none() && value.uuid.is_none()));
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains(original.id())
            && loss.message.contains("version2")
            && loss.message.contains("string-channel")));
}
