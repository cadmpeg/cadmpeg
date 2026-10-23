// SPDX-License-Identifier: Apache-2.0
//! Source-less document builder unit tests.

use crate::FcstdCodec;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn builds_and_writes_a_source_less_typed_application_graph() {
    let mut builder = crate::FcstdDocumentBuilder::new("source-less & portable");
    builder
        .add_object("Box", "Part::Box")
        .expect("add object")
        .add_property(
            "Box",
            "Label",
            "App::PropertyString",
            vec![crate::FcstdPropertyValue::attribute(
                "String",
                "value",
                "Generated Box",
            )],
        )
        .expect("add label")
        .add_property(
            "Box",
            "Length",
            "App::PropertyLength",
            vec![crate::FcstdPropertyValue::attribute(
                "Float", "value", "12.5",
            )],
        )
        .expect("add length")
        .add_property(
            "Box",
            "Width",
            "App::PropertyLength",
            vec![crate::FcstdPropertyValue::attribute("Float", "value", "7")],
        )
        .expect("add width")
        .add_property(
            "Box",
            "Height",
            "App::PropertyLength",
            vec![crate::FcstdPropertyValue::attribute("Float", "value", "3")],
        )
        .expect("add height")
        .add_object("Part", "App::Part")
        .expect("add part");
    crate::builder::test_support::attach_part_fixture(&mut builder, b"extension payload");
    let mut ir = builder.build().expect("build source-less graph");
    assert!(crate::test_support::validate_native(&ir).is_empty());
    crate::mutation::replace_entry(&mut ir, "Payload.bin", b"edited payload".to_vec())
        .expect("replace side entry");

    let mut encoded = Vec::new();
    FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("write graph");
    let round_trip = FcstdCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("decode generated file");
    let namespace = round_trip
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    assert_eq!(objects.len(), 2);
    assert_eq!(objects[0].name(), "Box");
    assert_eq!(objects[0].type_name, "Part::Box");
    assert_eq!(objects[1].dependencies, vec![objects[0].id().clone()]);
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.name() == "Payload.bin")
            .map(crate::native::EntryRecord::data),
        Some(b"edited payload".as_slice())
    );
}

#[test]
fn source_less_payloads_follow_request_order_before_gui_payloads() {
    let mut builder = super::FcstdDocumentBuilder::new("ordered payloads");
    for (object, payload) in [("ZFirst", "ZFirst.bin"), ("ASecond", "ASecond.bin")] {
        builder
            .add_object(object, "App::FeaturePython")
            .expect("required invariant");
        builder
            .add_property(
                object,
                "Attachment",
                "App::PropertyFileIncluded",
                vec![super::FcstdPropertyValue::attribute(
                    "FileIncluded",
                    "file",
                    payload,
                )],
            )
            .expect("required invariant");
    }
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><Camera settings=""/><Property><ColorList file="Colors.bin"/></Property></Document>"#;
    for (name, bytes) in [
        ("Colors.bin", b"colors".as_slice()),
        ("GuiDocument.xml", gui.as_slice()),
        ("ASecond.bin", b"second".as_slice()),
        ("ZFirst.bin", b"first".as_slice()),
        ("Opaque.bin", b"opaque".as_slice()),
    ] {
        builder
            .add_side_entry(name, bytes.to_vec())
            .expect("required invariant");
    }
    let document = builder.document_xml();
    let order = builder
        .restoration_entries(&document)
        .expect("required invariant");
    assert_eq!(
        order
            .iter()
            .map(|entry| entry.0.as_str())
            .collect::<Vec<_>>(),
        [
            "ZFirst.bin",
            "ASecond.bin",
            "GuiDocument.xml",
            "Colors.bin",
            "Opaque.bin"
        ]
    );
    let bytes = builder.clone().archive_bytes().expect("required invariant");
    assert_eq!(bytes, builder.archive_bytes().expect("required invariant"));
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("required invariant");
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
            "ZFirst.bin",
            "ASecond.bin",
            "GuiDocument.xml",
            "Colors.bin",
            "Opaque.bin"
        ]
    );
}

#[test]
fn source_less_shared_payload_requests_require_distinct_members() {
    let mut builder = super::FcstdDocumentBuilder::new("shared payload");
    for object in ["First", "Second"] {
        builder
            .add_object(object, "App::FeaturePython")
            .expect("required invariant");
        builder
            .add_property(
                object,
                "Attachment",
                "App::PropertyFileIncluded",
                vec![super::FcstdPropertyValue::attribute(
                    "FileIncluded",
                    "file",
                    "Shared.bin",
                )],
            )
            .expect("required invariant");
    }
    builder
        .add_side_entry("Shared.bin", b"shared".to_vec())
        .expect("required invariant");
    let error = builder
        .archive_bytes()
        .expect_err("refused restoration schedule");
    assert!(error.to_string().contains("multiple restoration requests"));
}

#[test]
fn source_less_payload_names_and_missing_requests_are_rejected() {
    let mut builder = super::FcstdDocumentBuilder::new("missing payload");
    for name in ["Document.xml", "../Escape.bin", "/Absolute.bin"] {
        assert!(builder.add_side_entry(name, Vec::new()).is_err());
    }
    builder
        .add_side_entry("Unique.bin", Vec::new())
        .expect("required invariant");
    assert!(builder.add_side_entry("Unique.bin", Vec::new()).is_err());
    builder
        .add_object("Owner", "App::FeaturePython")
        .expect("required invariant");
    builder
        .add_property(
            "Owner",
            "Attachment",
            "App::PropertyFileIncluded",
            vec![super::FcstdPropertyValue::attribute(
                "FileIncluded",
                "file",
                "Missing.bin",
            )],
        )
        .expect("required invariant");
    assert!(builder
        .archive_bytes()
        .expect_err("refused restoration schedule")
        .to_string()
        .contains("missing side entry Missing.bin"));
}

#[test]
fn source_less_gui_external_links_do_not_register_archive_requests() {
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1"><Property name="Remote" type="App::PropertyXLink"><XLink file="External.FCStd" name="Body"/></Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    let mut builder = super::FcstdDocumentBuilder::new("external GUI link");
    builder
        .add_object("Model", "Part::Feature")
        .expect("object");
    builder
        .add_side_entry("GuiDocument.xml", gui.to_vec())
        .expect("GUI");
    builder
        .clone()
        .build()
        .expect("external link needs no archive payload");
    builder
        .add_side_entry("Opaque.bin", b"opaque".to_vec())
        .expect("opaque");
    builder
        .add_side_entry("External.FCStd", b"unrelated".to_vec())
        .expect("unrelated payload");
    let document = builder.document_xml();
    let entries = builder
        .restoration_entries(&document)
        .expect("request schedule");
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.0.as_str())
            .collect::<Vec<_>>(),
        ["GuiDocument.xml", "Opaque.bin", "External.FCStd"]
    );
}
