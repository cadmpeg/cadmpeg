// SPDX-License-Identifier: Apache-2.0
//! The `Document.xml` patch and the archive repack: property edits, value
//! serialization, entry preservation, and the checks that refuse an edit no
//! retained span can carry.

use super::super::target::retained_baseline;
use super::super::*;
use crate::test_support::*;
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
        raw_xml: format!(
            r#"<Property name="Values" type="App::PropertyStringList">{raw_value}{raw_value}</Property>"#
        ),
        byte_start: 0,
        byte_end: 0,
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
    let namespace = declaration_edit
        .native
        .namespace_mut("fcstd", std::num::NonZeroU32::MIN);
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
    let namespace = stale_entry
        .native
        .namespace_mut("fcstd", std::num::NonZeroU32::MIN);
    let mut entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    entries
        .iter_mut()
        .find(|entry| entry.name != "Document.xml")
        .expect("side entry")
        .data
        .push(0);
    namespace
        .set_arena("entries", &entries)
        .expect("replace entries");
    let error = FcstdCodec
        .plan(EncodeInput::new(&stale_entry, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("stale entry metadata must fail");
    assert!(error.to_string().contains("stale length or digest"));
}
