// SPDX-License-Identifier: Apache-2.0
//! Source-less document builder unit tests.

use crate::FcstdCodec;
use cadmpeg_ir::codec::EncodeInput;
use cadmpeg_ir::codec::TargetRequest;
use cadmpeg_ir::{Codec, DecodeOptions, Encoder};
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
        .expect("add part")
        .add_dependency("Part", "Box")
        .expect("add dependency")
        .add_property(
            "Part",
            "Group",
            "App::PropertyLinkList",
            vec![crate::FcstdPropertyValue::empty("LinkList")
                .with_attribute("count", "1")
                .with_child(crate::FcstdPropertyValue::attribute("Link", "value", "Box"))],
        )
        .expect("add group")
        .add_side_entry("Payload.bin", b"extension payload".to_vec())
        .expect("add payload");
    let mut ir = builder.build().expect("build source-less graph");
    assert!(crate::validate_native(&ir).is_empty());
    FcstdCodec
        .replace_side_entry(&mut ir, "Payload.bin", b"edited payload".to_vec())
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
    assert_eq!(objects[0].name, "Box");
    assert_eq!(objects[0].type_name, "Part::Box");
    assert_eq!(objects[1].dependencies, vec![objects[0].id.clone()]);
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.name == "Payload.bin")
            .map(|entry| entry.data.as_slice()),
        Some(b"edited payload".as_slice())
    );
}
