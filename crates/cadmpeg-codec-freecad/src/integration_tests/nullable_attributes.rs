// SPDX-License-Identifier: Apache-2.0
//! Attribute holes follow the target property's scalar or ordered-list grammar.

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};
use std::io::Cursor;

fn document_attribute(values: Vec<Option<AttributeValue>>) -> SourceAttribute {
    SourceAttribute {
        id: "test:model:attribute#Meta".try_into().unwrap(),
        native_ref: None,
        target: AttributeTarget::Document,
        name: "Meta".into(),
        values,
    }
}

fn write(ir: &CadIr) -> (Vec<u8>, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let plan = crate::FcstdCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .unwrap();
    let losses = plan.report().losses.clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    (bytes, losses)
}

fn omission<'a>(
    losses: &'a [cadmpeg_ir::report::loss::LossNote],
    id: &str,
) -> &'a cadmpeg_ir::report::loss::LossNote {
    let code = crate::loss::FreecadLossCode::ExportAttributeNotTransferred
        .note("")
        .code;
    let matching = losses
        .iter()
        .filter(|loss| loss.code == code && loss.message.contains(id))
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1);
    matching[0]
}

#[test]
fn ordered_string_holes_keep_peers_in_order_and_name_source_positions() {
    let mut ir = CadIr::empty();
    let attribute = document_attribute(vec![
        Some(AttributeValue::String("first".into())),
        None,
        Some(AttributeValue::String("last".into())),
        None,
    ]);
    let id = attribute.id.to_string();
    ir.model.attributes.push(attribute);
    let (bytes, losses) = write(&ir);
    assert!(omission(&losses, &id)
        .message
        .contains("unavailable value positions [1, 3]"));
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let current = decoded
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "Meta")
        .unwrap();
    assert_eq!(
        current.values,
        vec![
            Some(AttributeValue::String("first".into())),
            Some(AttributeValue::String("last".into()))
        ]
    );
}

#[test]
fn scalar_hole_drops_the_whole_attribute_without_resurrecting_a_peer_value() {
    let mut ir = CadIr::empty();
    let attribute = document_attribute(vec![
        Some(AttributeValue::Integer(7)),
        None,
        Some(AttributeValue::Integer(9)),
    ]);
    let id = attribute.id.to_string();
    ir.model.attributes.push(attribute);
    let (bytes, losses) = write(&ir);
    assert!(omission(&losses, &id)
        .message
        .contains("unavailable value positions [1]"));
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "Meta"));
}

#[test]
fn native_string_list_all_holes_writes_empty_content_instead_of_native_values() {
    let source = r#"<Document SchemaVersion="4" FileVersion="1"><Properties Count="1"><Property name="Meta" type="App::PropertyStringList"><StringList count="2"><String value="old-first"/><String value="old-last"/></StringList></Property></Properties><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let bytes = crate::test_support::test_archive::archive(source);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let attribute = ir
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "Meta")
        .unwrap();
    assert_eq!(
        attribute.values,
        vec![
            Some(AttributeValue::String("old-first".into())),
            Some(AttributeValue::String("old-last".into()))
        ]
    );
    attribute.values = vec![None, None];
    let id = attribute.id.to_string();
    let (bytes, losses) = write(&ir);
    assert!(omission(&losses, &id)
        .message
        .contains("unavailable value positions [0, 1]"));
    let current = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let namespace = current.ir().native.namespace("fcstd").unwrap();
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let property = properties
        .iter()
        .find(|property| property.name == "Meta")
        .unwrap();
    assert_eq!(property.type_name, "App::PropertyStringList");
    let values = property.values();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].tag, "StringList");
    assert_eq!(
        values[0].attributes,
        std::collections::BTreeMap::from([("count".into(), "0".into())])
    );
}

#[test]
fn native_scalar_string_with_a_hole_cannot_be_promoted_to_a_string_list() {
    let source = r#"<Document SchemaVersion="4" FileVersion="1"><Properties Count="1"><Property name="Meta" type="App::PropertyString"><String value="old"/></Property></Properties><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let bytes = crate::test_support::test_archive::archive(source);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let attribute = ir
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "Meta")
        .unwrap();
    assert_eq!(
        attribute.values,
        vec![Some(AttributeValue::String("old".into()))]
    );
    assert!(attribute.native_ref.is_some());
    attribute.id = "test:model:attribute#reminted-scalar".try_into().unwrap();
    attribute.values = vec![Some(AttributeValue::String("current".into())), None];
    let id = attribute.id.to_string();
    let (bytes, losses) = write(&ir);
    assert!(omission(&losses, &id)
        .message
        .contains("unavailable value positions [1]"));
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "Meta"));
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert!(!properties.iter().any(|property| property.name == "Meta"));
}
