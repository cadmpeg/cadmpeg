// SPDX-License-Identifier: Apache-2.0
//! The `Document.xml` patch and the archive repack: property edits, value
//! serialization, entry preservation, and the checks that refuse an edit no
//! retained span can carry.

use super::super::target::retained_baseline;
use crate::native::{PropertyRecord, ValueRecord};
use crate::test_support::test_archive::CORE_DESIGN_PRODUCT;
use crate::writer::{serialize_property, serialize_value};
use crate::FcstdCodec;
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::write::{EncodeInput, TargetRequest};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn property_edits_use_value_order_when_raw_xml_is_identical() {
    let raw_value = r#"<String value="same"/>"#;
    let mut values = (0..2)
        .map(|order| ValueRecord {
            tag: "String".into(),
            order,
            attributes: [("value".into(), "same".into())].into(),
            text: None,
            raw_xml: raw_value.into(),
        })
        .collect::<Vec<_>>();
    values[1]
        .attributes
        .insert("value".into(), "changed".into());
    let property = PropertyRecord {
        id: "test:property#values".into(),
        owner: "test:object#owner".into(),
        name: "Values".into(),
        type_name: "App::PropertyStringList".into(),
        family: crate::native::PropertyFamily::List,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values,
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text(format!(
            r#"<Property name="Values" type="App::PropertyStringList">{raw_value}{raw_value}</Property>"#
        ), 0).unwrap(),
    };
    let output = String::from_utf8(serialize_property(&property).expect("required invariant"))
        .expect("required invariant");
    assert_eq!(output.matches(r#"value="same""#).count(), 1);
    assert_eq!(output.matches(r#"value="changed""#).count(), 1);
    assert!(
        output.find("same").expect("required invariant")
            < output.find("changed").expect("required invariant")
    );
}

#[test]
fn xml_serialization_preserves_normalized_whitespace() {
    let value = ValueRecord {
        tag: "String".into(),
        order: 0,
        attributes: [("value".into(), "a\tb\nc\rd".into())].into(),
        text: Some("a\tb\nc\rd".into()),
        raw_xml: r#"<String value="old">old</String>"#.into(),
    };
    let serialized = serialize_value(&value).expect("required invariant");
    assert!(serialized.contains("a&#9;b&#10;c&#13;d"));
    assert_eq!(serialized.matches("&#9;").count(), 2);
    assert_eq!(serialized.matches("&#10;").count(), 2);
    assert_eq!(serialized.matches("&#13;").count(), 2);
}

#[test]
fn writes_typed_property_edits_and_preserves_other_entries() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");
    let source_entries = decoded
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let mut edited = decoded.ir().clone();
    FcstdCodec
        .set_property_value_attribute(
            &mut edited,
            crate::FcstdPropertyOwner::Document,
            "Label",
            0,
            "value",
            "edited & verified",
        )
        .expect("edit Label");

    let mut encoded = Vec::new();
    let report = FcstdCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("encode edit");
    assert!(report.losses.is_empty());
    let round_trip = FcstdCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("decode output");
    let output_namespace = round_trip
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace");
    let output_properties = output_namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    let output_label = output_properties
        .iter()
        .find(|property| {
            property.owner == crate::native::native_id("document", "0") && property.name == "Label"
        })
        .expect("document Label");
    assert_eq!(
        output_label.values()[0]
            .attributes
            .get("value")
            .map(String::as_str),
        Some("edited & verified")
    );
    let output_entries = output_namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    for source in source_entries
        .iter()
        .filter(|entry| entry.name != "Document.xml")
    {
        let output = output_entries
            .iter()
            .find(|entry| entry.name == source.name)
            .expect("preserved entry");
        assert_eq!(output.data, source.data, "{}", source.name);
    }
    assert!(crate::validate_native(round_trip.ir()).is_empty());
}

#[test]
fn mutation_rejects_link_carrier_edits_without_changing_the_graph() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Owner"/><Object type="Part::Feature" name="Target"/></Objects>
<ObjectData Count="2"><Object name="Owner"><Properties Count="1"><Property name="Support" type="App::PropertyLink"><Link value="Target"/></Property></Properties></Object><Object name="Target"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(document)),
            &DecodeOptions::default(),
        )
        .expect("decode link carrier");
    let before = decoded
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties")
        .iter()
        .find(|property| property.name == "Support")
        .expect("Support property")
        .links()
        .to_vec();
    let mut edited = decoded.ir().clone();
    let error = FcstdCodec
        .set_property_value_attribute(
            &mut edited,
            crate::FcstdPropertyOwner::Object("Owner"),
            "Support",
            0,
            "value",
            "Missing",
        )
        .expect_err("link target mutation must refuse stale graph output");
    assert!(error.to_string().contains("graph-aware serializer"));
    let after = edited
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties")
        .iter()
        .find(|property| property.name == "Support")
        .expect("Support property")
        .links()
        .to_vec();
    assert_eq!(after, before);
    assert!(crate::validate_native(&edited).is_empty());
}

#[test]
fn writer_rejects_direct_link_carrier_graph_drift() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Owner"/><Object type="Part::Feature" name="Target"/></Objects>
<ObjectData Count="2"><Object name="Owner"><Properties Count="1"><Property name="Support" type="App::PropertyLink"><Link value="Target"/></Property></Properties></Object><Object name="Target"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(document)),
            &DecodeOptions::default(),
        )
        .expect("decode link carrier");
    let mut edited = decoded.ir().clone();
    let namespace = edited.native.namespace_mut("fcstd");
    let mut properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    let support = properties
        .iter_mut()
        .find(|property| property.name == "Support")
        .expect("Support property");
    support.values_mut().expect("persisted property")[0]
        .attributes
        .insert("value".into(), "Missing".into());
    namespace
        .set_arena("properties", &properties)
        .expect("replace properties");

    let error = FcstdCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("writer must check the written semantic graph");
    assert!(error.to_string().contains("value or link graph"));
}

#[test]
fn seekable_encoder_matches_the_write_only_fallback() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");
    let mut staged = Vec::new();
    FcstdCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut staged))
        .expect("write-only fallback");
    let mut streamed = Cursor::new(Vec::new());
    let source_dialect = decoded
        .ir()
        .source
        .as_ref()
        .and_then(cadmpeg_ir::SourceMeta::dialect)
        .expect("decoded FCStd source is classified")
        .dialect();
    let resolution = retained_baseline(decoded.ir(), source_dialect)
        .expect("the decoded schema-4 baseline is preserved");
    crate::writer::write_seekable(&mut streamed, &resolution).expect("seekable writer");

    assert_eq!(streamed.into_inner(), staged);
}

#[test]
pub(crate) fn writer_rejects_unserialized_declaration_and_stale_payload_edits() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");

    let mut declaration_edit = decoded.ir().clone();
    let namespace = declaration_edit.native.namespace_mut("fcstd");
    let mut objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    objects[0].type_name = "App::FeaturePython".into();
    namespace
        .set_arena("objects", &objects)
        .expect("replace objects");
    let error = FcstdCodec
        .plan(
            EncodeInput::new(&declaration_edit, None),
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("unserialized declaration edit must fail");
    assert!(error.to_string().contains("declaration edits"));

    let (mut stale_entry, _, _) = decoded.into_parts();
    let namespace = stale_entry.native.namespace_mut("fcstd");
    let mut entries = namespace
        .arena_as::<serde_json::Value>("entries")
        .expect("entries");
    entries
        .iter_mut()
        .find(|entry| entry["name"] != "Document.xml")
        .expect("side entry")["data"]
        .as_array_mut()
        .expect("entry bytes")
        .push(serde_json::json!(0));
    namespace
        .set_arena("entries", &entries)
        .expect("replace entries");
    let error = FcstdCodec
        .plan(EncodeInput::new(&stale_entry, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("stale entry metadata must fail");
    assert!(error
        .to_string()
        .contains("entry byte_len/sha256 disagrees with data"));
}
