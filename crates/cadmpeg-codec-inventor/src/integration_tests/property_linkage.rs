// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::native::{PropertyRecord, PropertyValueKind};
use cadmpeg_ir::attributes::AttributeValue;

fn padded(mut bytes: Vec<u8>) -> Vec<u8> {
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    bytes
}

fn text(value: &str) -> Vec<u8> {
    let units = value.encode_utf16().chain([0]).collect::<Vec<_>>();
    let mut bytes = 31_u32.to_le_bytes().to_vec();
    bytes.extend(u32::try_from(units.len()).unwrap().to_le_bytes());
    for unit in units {
        bytes.extend(unit.to_le_bytes());
    }
    padded(bytes)
}

fn source() -> Vec<u8> {
    let mut dictionary = 2_u32.to_le_bytes().to_vec();
    for (id, name) in [(2_u32, "CustomName"), (3, "CustomCount")] {
        let units = name.encode_utf16().chain([0]).collect::<Vec<_>>();
        dictionary.extend(id.to_le_bytes());
        dictionary.extend(u32::try_from(units.len()).unwrap().to_le_bytes());
        for unit in units {
            dictionary.extend(unit.to_le_bytes());
        }
        dictionary = padded(dictionary);
    }
    let properties = [
        (0_u32, dictionary),
        (
            1,
            [
                2_u16.to_le_bytes(),
                0_u16.to_le_bytes(),
                1200_u16.to_le_bytes(),
                0_u16.to_le_bytes(),
            ]
            .concat(),
        ),
        (2, text("Équipement")),
        (3, [3_i32.to_le_bytes(), (-17_i32).to_le_bytes()].concat()),
    ];
    let mut section = vec![0; 8 + properties.len() * 8];
    section[4..8].copy_from_slice(&u32::try_from(properties.len()).unwrap().to_le_bytes());
    for (index, (id, value)) in properties.iter().enumerate() {
        section[8 + index * 8..12 + index * 8].copy_from_slice(&id.to_le_bytes());
        let offset = u32::try_from(section.len()).unwrap();
        section[12 + index * 8..16 + index * 8].copy_from_slice(&offset.to_le_bytes());
        section.extend(value);
    }
    let length = u32::try_from(section.len()).unwrap();
    section[..4].copy_from_slice(&length.to_le_bytes());
    let mut stream = vec![0; 48];
    stream[..2].copy_from_slice(&0xfffe_u16.to_le_bytes());
    stream[2..4].copy_from_slice(&1_u16.to_le_bytes());
    stream[24..28].copy_from_slice(&1_u32.to_le_bytes());
    stream[28..44].copy_from_slice(&[0x42; 16]);
    stream[44..48].copy_from_slice(&48_u32.to_le_bytes());
    stream.extend(section);
    crate::test_support::test_fixtures::fixture_with_property_set_peers(&stream, &stream)
}

fn decode() -> cadmpeg_ir::DecodeResult {
    InventorCodec
        .decode(
            &mut std::io::Cursor::new(source()),
            &DecodeOptions::default(),
        )
        .unwrap()
}

#[test]
fn decoded_iproperties_keep_exact_native_values_and_each_neutral_attribute_owns_its_native_ref() {
    let decoded = decode();
    let native = decoded.ir().native.namespace("inventor").unwrap();
    let properties = native.arena_as::<PropertyRecord>("properties").unwrap();
    assert_eq!(properties.len(), 8);
    assert_eq!(decoded.ir().model.attributes.len(), 4);
    let mut links = std::collections::BTreeSet::new();
    for attribute in &decoded.ir().model.attributes {
        let link = attribute
            .native_ref
            .as_deref()
            .expect("native property identity");
        assert!(links.insert(link));
        let property = properties
            .iter()
            .find(|property| property.id == link)
            .unwrap();
        assert_eq!(property.name.as_deref(), Some(attribute.name.as_str()));
        match attribute.name.as_str() {
            "CustomName" => {
                assert_eq!(property.scalar_value.as_deref(), Some("Équipement"));
                assert!(matches!(
                    property.value_kind,
                    PropertyValueKind::String { type_code: 31 }
                ));
                assert_eq!(
                    attribute.values,
                    [Some(AttributeValue::String("Équipement".into()))]
                );
            }
            "CustomCount" => {
                assert_eq!(property.scalar_value.as_deref(), Some("-17"));
                assert!(matches!(
                    property.value_kind,
                    PropertyValueKind::Signed { type_code: 3 }
                ));
                assert_eq!(attribute.values, [Some(AttributeValue::Integer(-17))]);
            }
            name => panic!("unexpected projected property {name}"),
        }
    }
    for record in &native.arenas()["properties"] {
        let fields = record.fields().unwrap();
        assert!(!fields.contains_key("neutral_attribute"));
        assert!(fields.contains_key("name"));
        assert!(fields.contains_key("scalar_value"));
    }
}

#[test]
fn source_free_native_property_admission_does_not_depend_on_the_neutral_attribute_value() {
    let (mut ir, _, _) = decode().into_parts();
    ir.source = None;
    let before = ir.native.clone();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(
        &arena,
        &DecodePolicy::service(),
        cadmpeg_core::decode::DecodeDepth::Full,
    );
    let findings = InventorCodec.validate_native(&ctx, &ir, None).unwrap();
    ir.model.attributes[0].values = vec![Some(AttributeValue::String(
        "Edited current attribute".into(),
    ))];
    assert_eq!(
        InventorCodec.validate_native(&ctx, &ir, None).unwrap(),
        findings
    );
    assert_eq!(ir.native, before);
    let attribute_id = ir.model.attributes[0].id.to_string();
    ir.model.attributes[0].native_ref = Some("inventor:property:missing#0".into());
    let checked = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(checked.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(attribute_id.as_str())
            && finding.message == "native_ref `inventor:property:missing#0` does not resolve"
    }));
}

#[test]
fn malformed_kept_native_property_fields_still_fail_typed_native_admission() {
    let (mut ir, _, _) = decode().into_parts();
    ir.source = None;
    let records = ir
        .native
        .namespace_mut("inventor")
        .arenas_mut()
        .get_mut("properties")
        .unwrap();
    let record = records
        .iter_mut()
        .find(|record| record.fields().unwrap()["name"] == "CustomName")
        .unwrap();
    let mut fields = record.fields().unwrap();
    fields.insert("name".into(), serde_json::json!(17));
    *record = cadmpeg_ir::native::NativeRecord::new(
        cadmpeg_ir::ids::Identity::try_from(record.id()).unwrap(),
        fields,
    )
    .unwrap();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(
        &arena,
        &DecodePolicy::service(),
        cadmpeg_core::decode::DecodeDepth::Full,
    );
    let findings = InventorCodec.validate_native(&ctx, &ir, None).unwrap();
    assert!(findings.iter().any(|finding| finding
        .message
        .starts_with("Inventor native arenas are invalid:")));
}
