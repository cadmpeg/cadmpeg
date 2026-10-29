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
    let missing_order_error = crate::writer::restore_entry_order(
        &mut [entry.record.clone()],
        &std::collections::HashMap::new(),
    )
    .expect_err("output order cannot be inferred from identity");
    assert!(missing_order_error
        .to_string()
        .contains("no retained restoration order"));
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
    // #209 section2.3 W2: the current neutral projection owns this native lane.
    let canonical_label = edited
        .model
        .attributes
        .iter()
        .find(|attribute| {
            attribute.target == cadmpeg_ir::attributes::AttributeTarget::Document
                && attribute.name == "Label"
        })
        .expect("canonical document Label");
    let [Some(cadmpeg_ir::attributes::AttributeValue::String(expected_label))] =
        canonical_label.values.as_slice()
    else {
        panic!("the authored fixture has one canonical string Label")
    };
    let expected_label = expected_label.clone();
    assert_ne!(expected_label, "edited & verified");
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
    let edited_properties = edited
        .native
        .namespace("fcstd")
        .expect("native namespace")
        .arena_as::<PropertyRecord>("properties")
        .expect("typed properties");
    let edited_native_label = edited_properties
        .iter()
        .find(|property| {
            property.owner == crate::native::native_id("document", "0") && property.name == "Label"
        })
        .expect("edited native document Label");
    assert_eq!(
        edited_native_label.values()[0]
            .attributes
            .get("value")
            .map(String::as_str),
        Some("edited & verified")
    );

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
        Some(expected_label.as_str())
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

#[test]
fn canonical_document_label_edits_regenerate_the_native_property() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");
    let mut edited = decoded.ir().clone();
    let canonical_label = edited
        .model
        .attributes
        .iter_mut()
        .find(|attribute| {
            attribute.target == cadmpeg_ir::attributes::AttributeTarget::Document
                && attribute.name == "Label"
        })
        .expect("canonical document Label");
    assert_ne!(
        canonical_label.values,
        vec![Some(cadmpeg_ir::attributes::AttributeValue::String(
            "edited & verified".into()
        ))]
    );
    canonical_label.values = vec![Some(cadmpeg_ir::attributes::AttributeValue::String(
        "edited & verified".into(),
    ))];
    let mut encoded = Vec::new();
    let report = FcstdCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("encode canonical edit");
    assert!(matches!(
        report.write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    let round_trip = FcstdCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("decode canonical output");
    let namespace = round_trip
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace");
    let properties = namespace
        .arena_as::<PropertyRecord>("properties")
        .expect("typed properties");
    let output_label = properties
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
    let canonical_label = round_trip
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| {
            attribute.target == cadmpeg_ir::attributes::AttributeTarget::Document
                && attribute.name == "Label"
        })
        .expect("decoded canonical Label");
    assert_eq!(
        canonical_label.values,
        vec![Some(cadmpeg_ir::attributes::AttributeValue::String(
            "edited & verified".into()
        ))]
    );
    assert_eq!(
        round_trip.ir().model.bodies.len(),
        decoded.ir().model.bodies.len()
    );
    assert!(crate::test_support::validate_native(round_trip.ir()).is_empty());
}

#[test]
fn retaining_writes_preserve_physical_restoration_order_after_cadir_finalization() {
    for schema in ["2", "3", "4"] {
        let (objects_tag, data_tag) = if schema == "2" {
            ("Features", "FeatureData")
        } else {
            ("Objects", "ObjectData")
        };
        let xml = format!(
            r#"<Document SchemaVersion="{schema}" FileVersion="1"><Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="original"/></Property></Properties><{objects_tag} Count="0"/><{data_tag} Count="0"/></Document>"#
        );
        let source = archive_entries(&[
            ("Document.xml", xml.as_bytes()),
            ("ZFirst.brp", b"first"),
            ("ASecond.brp", b"second"),
            ("GuiDocument.xml", br#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><Camera settings=""/></Document>"#),
            ("ZGui.bin", b"gui"),
            ("AOpaque.bin", b"opaque"),
        ]);
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("required invariant");
        for edit in [false, true] {
            let mut ir = decoded.ir().clone();
            ir.finalize();
            if edit {
                FcstdCodec
                    .set_property_value_attribute(
                        &mut ir,
                        crate::FcstdPropertyOwner::Document,
                        "Label",
                        0,
                        "value",
                        "edited",
                    )
                    .expect("required invariant");
            }
            let mut output = Vec::new();
            FcstdCodec
                .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
                .and_then(|plan| plan.write_to(&mut output))
                .expect("required invariant");
            let mut archive =
                zip::ZipArchive::new(Cursor::new(output)).expect("required invariant");
            let names = (0..archive.len())
                .map(|index| {
                    archive
                        .by_index(index)
                        .expect("required invariant")
                        .name()
                        .expect("required invariant")
                        .into_owned()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                names,
                [
                    "Document.xml",
                    "ZFirst.brp",
                    "ASecond.brp",
                    "GuiDocument.xml",
                    "ZGui.bin",
                    "AOpaque.bin"
                ],
                "schema={schema}, edit={edit}"
            );
        }
    }
}

#[test]
fn typed_input_edits_persist_owner_recompute_state_without_touching_outputs_or_noops() {
    for (status, edited, touched) in [
        (0_u64, "15", true),
        (0, "10", false),
        (1 << 7, "15", false),
        (1 << 11, "15", true),
        (1 << 23, "15", false),
        (1 << 27, "15", false),
    ] {
        for declaration in [
            r#"<Object type="Part::Box" name="Box"/>"#,
            r#"<Object type="Part::Box" name="Box" Touched="0"/>"#,
        ] {
            let xml = format!(
                r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1">{declaration}</Objects><ObjectData Count="1"><Object name="Box"><Properties Count="1"><Property name="Length" type="App::PropertyLength" status="{status}"><Float value="10"/></Property></Properties></Object></ObjectData></Document>"#
            );
            let decoded = FcstdCodec
                .decode(
                    &mut Cursor::new(crate::test_support::test_archive::archive(&xml)),
                    &DecodeOptions::default(),
                )
                .expect("decode Box");
            let mut ir = decoded.ir().clone();
            FcstdCodec
                .set_property_value_attribute(
                    &mut ir,
                    crate::FcstdPropertyOwner::Object("Box"),
                    "Length",
                    0,
                    "value",
                    edited,
                )
                .expect("edit Length");
            let objects = ir
                .native
                .namespace("fcstd")
                .expect("namespace")
                .arena_as::<crate::native::ObjectRecord>("objects")
                .expect("objects");
            assert_eq!(
                objects[0].attributes.get("Touched").map(String::as_str) == Some("1"),
                touched
            );
            let mut bytes = Vec::new();
            FcstdCodec
                .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
                .and_then(|plan| plan.write_to(&mut bytes))
                .expect("write edit");
            let output = FcstdCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .expect("decode edit");
            let objects = output
                .ir()
                .native
                .namespace("fcstd")
                .expect("namespace")
                .arena_as::<crate::native::ObjectRecord>("objects")
                .expect("objects");
            assert_eq!(
                objects[0].attributes.get("Touched").map(String::as_str) == Some("1"),
                touched
            );
        }
    }
}

#[test]
fn schema_two_input_edits_refuse_an_ignored_recompute_marker_but_labels_remain_writable() {
    let xml = r#"<Document SchemaVersion="2" FileVersion="1"><Features Count="1"><Feature type="Part::Box" name="Box"/></Features><FeatureData Count="1"><Feature name="Box"><Properties Count="2"><Property name="Length" type="App::PropertyLength"><Float value="10"/></Property><Property name="Label" type="App::PropertyString"><String value="Box"/></Property></Properties></Feature></FeatureData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(xml)),
            &DecodeOptions::default(),
        )
        .expect("schema two Box");
    for (property, value, writable) in [("Length", "15", false), ("Label", "Renamed", true)] {
        let mut ir = decoded.ir().clone();
        FcstdCodec
            .set_property_value_attribute(
                &mut ir,
                crate::FcstdPropertyOwner::Object("Box"),
                property,
                0,
                "value",
                value,
            )
            .expect("edit property");
        let plan = FcstdCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit);
        if writable {
            assert!(plan.is_ok());
        } else {
            assert!(plan
                .expect_err("schema two has no recompute marker")
                .to_string()
                .contains("pending-recompute marker"));
        }
    }
}

#[test]
fn checked_native_input_record_edits_also_schedule_owner_recomputation() {
    let xml = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Box" name="Box"/></Objects><ObjectData Count="1"><Object name="Box"><Properties Count="1"><Property name="Length" type="App::PropertyLength"><Float value="10"/></Property></Properties></Object></ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(xml)),
            &DecodeOptions::default(),
        )
        .expect("Box");
    let mut ir = decoded.ir().clone();
    let namespace = ir.native.namespace_mut("fcstd");
    let mut properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    properties[0].values_mut().expect("values")[0]
        .attributes
        .insert("value".to_owned(), "15".to_owned());
    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "properties",
            &properties,
        )
        .expect("replace values");
    let mut bytes = Vec::new();
    FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("write native edit");
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode edit");
    let objects = output
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    assert_eq!(
        objects[0].attributes.get("Touched").map(String::as_str),
        Some("1")
    );
}
