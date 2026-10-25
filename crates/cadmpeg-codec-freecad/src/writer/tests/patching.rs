// SPDX-License-Identifier: Apache-2.0
//! The `Document.xml` patch and the archive repack: property edits, value
//! serialization, entry preservation, and the checks that refuse an edit no
//! retained span can carry.

use super::super::target::retained_baseline;
use crate::native::{EntryRecord, PropertyRecord, ValueRecord};
use crate::test_support::test_archive::{archive_entries, CORE_DESIGN_PRODUCT};
use crate::writer::validate_entry_names;
use crate::FcstdCodec;
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn unsafe_entry_names_refuse_construction_and_duplicates_remain_writer_limits() {
    assert!(crate::test_support::source_fidelity::EntryFixture::try_new(
        "test:native:entry#unsafe".into(),
        "../Document.xml".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        Vec::new()
    )
    .is_err());
    let entry = crate::test_support::entry_fixture(
        "test:native:entry#document".into(),
        "Document.xml".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        Vec::new(),
    );
    let duplicate_error = validate_entry_names(&[entry.record.clone(), entry.record])
        .expect_err("duplicate output paths are refused");
    assert!(matches!(
        duplicate_error,
        cadmpeg_core::CodecError::Malformed(_)
    ));
}

#[test]
fn x65_backslash_entry_is_refused_by_constructor_reader_and_native_record() {
    let error = crate::test_support::source_fidelity::EntryFixture::try_new(
        "test:native:entry#backslash".into(),
        r"..\outside".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        Vec::new(),
    )
    .expect_err("constructor refuses backslash");
    assert!(error.to_string().contains("unsafe ZIP entry path"));
    let wire = serde_json::json!({"id": "test:native:entry#backslash", "name": r"..\outside", "role": "auxiliary", "byte_len": 0, "sha256": cadmpeg_ir::hash::sha256_hex(&[]), "referenced_by": []});
    let mut semantic_wire = wire.clone();
    assert!(serde_json::from_value::<crate::native::EntryRecord>(wire).is_err());
    semantic_wire.as_object_mut().unwrap().remove("byte_len");
    semantic_wire.as_object_mut().unwrap().remove("sha256");
    let error = serde_json::from_value::<crate::native::EntryRecord>(semantic_wire)
        .expect_err("native entry refuses backslash");
    assert!(error.to_string().contains("unsafe ZIP entry path"));
    let xml = b"<Document SchemaVersion=\"4\" FileVersion=\"1\"/>";
    let bytes = archive_entries(&[("Document.xml", xml), (r"..\outside", b"payload")]);
    let error = FcstdCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect_err("reader refuses backslash");
    assert!(error.to_string().contains("unsafe ZIP entry path"));
}

#[test]
fn property_edits_use_value_order_when_raw_xml_is_identical() {
    let mut values = (0..2)
        .map(|order| ValueRecord {
            parent: None,
            tag: "String".into(),
            order,
            attributes: [("value".into(), "same".into())].into(),
            decoded_json: None,
            text: None,
        })
        .collect::<Vec<_>>();
    values[1]
        .attributes
        .insert("value".into(), "changed".into());
    let property = PropertyRecord {
        dynamic: None,
        id: "test:property#values".into(),
        owner: "test:object#owner".into(),
        name: "Values".into(),
        type_name: "App::PropertyStringList".into(),
        family: crate::native::PropertyFamily::List,
        status: None,
        body: Some(crate::native::PropertyBody::Persisted {
            values,
            links: Vec::new(),
            side_entries: Vec::new(),
        }),
        order: 0,
    };
    let output = crate::writer::xml::native(&property).unwrap().unwrap();
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
        parent: None,
        tag: "String".into(),
        order: 0,
        attributes: [("value".into(), "a\tb\nc\rd".into())].into(),
        decoded_json: None,
        text: Some("a\tb\nc\rd".into()),
    };
    let property = PropertyRecord {
        dynamic: None,
        id: "test:property#whitespace".into(),
        owner: "test:object#owner".into(),
        name: "Text".into(),
        type_name: "App::PropertyString".into(),
        family: crate::native::PropertyFamily::String,
        status: None,
        order: 0,
        body: Some(crate::native::PropertyBody::Persisted {
            values: vec![value],
            links: Vec::new(),
            side_entries: Vec::new(),
        }),
    };
    let serialized = crate::writer::xml::native(&property).unwrap().unwrap();
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
    assert!(matches!(
        report.write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
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
    assert_eq!(
        round_trip.ir().model.bodies.len(),
        decoded.ir().model.bodies.len()
    );
    assert!(crate::test_support::validate_native(round_trip.ir()).is_empty());
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
    assert!(crate::test_support::validate_native(&edited).is_empty());
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
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "properties",
            &properties,
        )
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
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
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
    let resolution = retained_baseline(decoded.ir(), decoded.source_fidelity(), source_dialect)
        .expect("the decoded schema-4 baseline is preserved");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let ctx = cadmpeg_core::decode::DecodeContext::new(
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
        cadmpeg_core::decode::DecodeDepth::Full,
    );
    crate::writer::write_seekable(&ctx, &mut streamed, &resolution).expect("seekable writer");

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
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "objects",
            &objects,
        )
        .expect("replace objects");
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&declaration_edit, Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect("typed declaration edit is synthesized");
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));

    let (stale_entry, _, mut fidelity) = decoded.into_parts();
    let namespace = stale_entry
        .native
        .namespace("fcstd")
        .expect("FCStd namespace");
    let mut entries = namespace
        .arena_as::<serde_json::Value>("entries")
        .expect("entries");
    let entry = entries
        .iter_mut()
        .find(|entry| entry["name"] != "Document.xml")
        .expect("side entry");
    // Retired native evidence is rejected; stale source-only layout evidence
    // cannot authorize reuse of the original archive.
    entry["sha256"] = serde_json::json!(cadmpeg_ir::hash::sha256_hex(b"different layout"));
    assert!(serde_json::from_value::<EntryRecord>(entry.clone()).is_err());
    fidelity.metadata.insert(
        cadmpeg_ir::hash::DOCUMENT_FINGERPRINT_METADATA_KEY.into(),
        cadmpeg_ir::hash::sha256_hex(b"different layout"),
    );
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&stale_entry, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .expect("stale layout evidence falls back to the current typed content");
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("write current typed content");
    let reread = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read synthesized output");
    assert_eq!(
        reread.ir().model.bodies.len(),
        stale_entry.model.bodies.len()
    );
}
