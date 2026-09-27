// SPDX-License-Identifier: Apache-2.0
//! Attachment-frame transfer unit tests.

use crate::test_support::test_archive::{archive, assert_valid_document};
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn diagnostic_property(type_name: &str, values: Vec<crate::native::ValueRecord>) -> crate::native::PropertyRecord {
    crate::native::PropertyRecord {
        id: "fcstd:native:property#Attachment:MapMode".into(),
        owner: "fcstd:native:object#Attachment".into(),
        name: "MapMode".into(),
        type_name: type_name.into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted { values, links: Vec::new(),
            side_entries: Vec::new(), dynamic: None },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("test XML span"),
    }
}

fn enum_value(tag: &str, value: Option<&str>) -> crate::native::ValueRecord {
    crate::native::ValueRecord {
        tag: tag.into(), order: 0,
        attributes: value.map(|value| std::collections::BTreeMap::from([
            ("value".into(), value.into())])).unwrap_or_default(),
        text: None, raw_xml: "<Integer/>".into(),
    }
}

#[test]
fn attachment_support_type_error_refuses_at_retained_limit() {
    let property = diagnostic_property("App::PropertyEnumeration", Vec::new());
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD attachment support type error",
        |ctx| super::support_links(ctx, &property));
}

#[test]
fn attachment_support_value_error_refuses_at_retained_limit() {
    let property = diagnostic_property("App::PropertyLinkSubList", Vec::new());
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD attachment support value error",
        |ctx| super::support_links(ctx, &property));
}

#[test]
fn attachment_map_mode_type_error_refuses_at_retained_limit() {
    let property = diagnostic_property("App::PropertyString", Vec::new());
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD attachment map-mode type error",
        |ctx| super::map_mode_value(ctx, &property));
}

#[test]
fn attachment_map_mode_value_error_refuses_at_retained_limit() {
    let property = diagnostic_property("App::PropertyEnumeration", Vec::new());
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD attachment map-mode value error",
        |ctx| super::map_mode_value(ctx, &property));
}

#[test]
fn attachment_map_mode_tag_error_refuses_at_retained_limit() {
    let property = diagnostic_property("App::PropertyEnumeration", vec![enum_value("String", None)]);
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD attachment map-mode tag error",
        |ctx| super::map_mode_value(ctx, &property));
}

#[test]
fn attachment_map_mode_missing_index_refuses_at_retained_limit() {
    let property = diagnostic_property("App::PropertyEnumeration", vec![enum_value("Integer", None)]);
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD attachment missing map-mode index",
        |ctx| super::map_mode_value(ctx, &property));
}

#[test]
fn attachment_map_mode_invalid_index_refuses_at_retained_limit() {
    let property = diagnostic_property("App::PropertyEnumeration", vec![enum_value("Integer", Some("bad-index"))]);
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD attachment invalid map-mode index",
        |ctx| super::map_mode_value(ctx, &property));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test input");
    let error = super::map_mode_value(&ctx, &property).expect_err("invalid index");
    assert_eq!(error.to_string(),
        "malformed container: attachment property fcstd:native:property#Attachment:MapMode: map_mode \"bad-index\" is not an index");
}

#[test]
fn attachment_map_mode_outer_error_refuses_at_retained_limit() {
    let property = diagnostic_property("App::PropertyEnumeration", vec![enum_value("Integer", Some("999"))]);
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD attachment map-mode error",
        |ctx| super::map_mode_value(ctx, &property));
}

#[test]
fn attachment_owner_lookup_refuses_on_collection_limit() {
    let property = crate::native::PropertyRecord {
        id: "property".into(),
        owner: "object".into(),
        name: "AttachmentSupport".into(),
        type_name: "App::PropertyLinkSubList".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within input policy");
    assert!(matches!(super::transfer(&ctx, &[], &[property]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD attachment owner lookup"));
}

#[test]
fn attachment_identity_refuses_at_retained_limit() {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Sketch".into(),
        name: "Sketch".into(),
        type_name: "Sketcher::SketchObject".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let property = crate::native::PropertyRecord {
        id: "fcstd:native:property#Sketch:MapMode".into(),
        owner: object.id.clone(),
        name: "MapMode".into(),
        type_name: "App::PropertyEnumeration".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: vec![crate::native::ValueRecord {
                tag: "Integer".into(),
                order: 0,
                attributes: std::collections::BTreeMap::from([("value".into(), "5".into())]),
                text: None,
                raw_xml: "<Integer value=\"5\"/>".into(),
            }],
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_retained_bytes = crate::native::native_id("attachment", &object.name).len() as u64 - 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(super::transfer(&ctx, &[object], &[property]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD native identity"));
}

#[test]
fn retains_support_attachment_and_distinct_offset_frame() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="PartDesign::Feature" name="Support" id="1"/>
 <Object type="Sketcher::SketchObject" name="Sketch" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Support"><Properties Count="0"/></Object>
 <Object name="Sketch"><Properties Count="5">
  <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
  <Property name="AttachmentSupport" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="Support" sub="Face1"/></LinkSubList></Property>
  <Property name="MapMode" type="App::PropertyEnumeration"><Integer value="5"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="AttachmentOffset" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("attachment");
    let namespace = result.ir().native.namespace("fcstd").expect("native");
    let attachments = namespace
        .arena_as::<crate::native::AttachmentRecord>("attachments")
        .expect("attachments");
    assert_eq!(attachments.len(), 1);
    assert_eq!(
        attachments[0].map_mode.map(|mode| mode.to_string()),
        Some("5".to_owned())
    );
    assert_eq!(
        attachments[0].supports[0]
            .as_ref()
            .expect("support")
            .object(),
        Some("fcstd:native:object#Support")
    );
    assert_eq!(
        attachments[0].supports[0]
            .as_ref()
            .expect("support")
            .subelements(),
        ["Face1"]
    );
    assert_eq!(
        attachments[0].placement().expect("placement").rows()[0][3],
        10.0
    );
    assert_eq!(attachments[0].offset().expect("offset").rows()[0][3], 2.0);
    assert_eq!(attachments[0].effective_frame()[0][3], 12.0);
    let sketch = result.ir().model.sketches.first().expect("sketch");
    assert_eq!(
        sketch
            .resolved_placement()
            .expect("resolved sketch placement")
            .0
            .x,
        10.0
    );
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn rejects_ambiguous_attachment_carriers() {
    for property in [
        r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="1"/><PropertyPlacement Px="2"/></Property>"#,
        r#"<Property name="MapMode" type="App::PropertyString"><String value="FlatFace"/><String value="Deformed"/></Property>"#,
        r#"<Property name="AttachmentSupport" type="App::PropertyLinkSub"><LinkSub value="Support" count="1"><Sub value="Face1"/></LinkSub></Property>"#,
        r#"<Property name="MapMode" type="App::PropertyEnumeration"><Integer value="55"/></Property>"#,
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">{property}</Properties></Object></ObjectData>
</Document>"#
        );
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            ),
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::Malformed(_)
            ))
        ));
    }
}

#[test]
fn rejects_noncanonical_map_mode_value_grammar() {
    for property in [
        r#"<Property name="MapMode" type="App::PropertyEnumeration"><String value="FlatFace"/></Property>"#,
        r#"<Property name="MapMode" type="App::PropertyEnumeration"><Integer value="5"/><CustomEnumList count="1"><Enum value="FlatFace"/></CustomEnumList></Property>"#,
        r#"<Property name="MapMode" type="App::PropertyEnumeration"><Integer/></Property>"#,
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">{property}</Properties></Object></ObjectData>
</Document>"#
        );
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            ),
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::Malformed(_)
            ))
        ));
    }
}

#[test]
fn rejects_invalid_attachment_placement_values() {
    for property in [
        r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0"/></Property>"#,
        r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="NaN"/></Property>"#,
        r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="0"/></Property>"#,
        r#"<Property name="Placement" type="App::PropertyString"/>"#,
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">{property}</Properties></Object></ObjectData>
</Document>"#
        );
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            ),
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::Malformed(_)
            ))
        ));
    }
}

#[test]
fn map_mode_admission_checks_indices_and_preserves_decimal_wire() {
    for index in 0..super::MAP_MODE_NAMES.len() {
        let expected = index.to_string();
        let mode = super::MapModeIndex::try_new(index).expect("valid table index");
        assert_eq!(mode.to_string(), expected);
        assert_eq!(
            super::MapModeIndex::try_from(expected.as_str()).expect("text index"),
            mode
        );
        let wire = serde_json::json!(expected);
        assert_eq!(serde_json::to_value(mode).expect("serialize index"), wire);
        assert_eq!(
            serde_json::from_value::<super::MapModeIndex>(wire).expect("deserialize index"),
            mode
        );
    }
    for index in [super::MAP_MODE_NAMES.len(), 255, 256, usize::MAX] {
        assert!(super::MapModeIndex::try_new(index).is_err());
    }
    let record = crate::native::AttachmentRecord::try_new(
        "attachment".to_owned(),
        "object".to_owned(),
        Vec::new(),
        None,
        None,
        None,
    )
    .expect("attachment without a map mode");
    let wire = serde_json::to_value(record).expect("serialize attachment");
    for value in [
        "not-an-index".to_owned(),
        "-1".to_owned(),
        String::new(),
        super::MAP_MODE_NAMES.len().to_string(),
        "256".to_owned(),
    ] {
        assert!(super::MapModeIndex::try_from(value.as_str()).is_err());
        let mut invalid = wire.clone();
        invalid["map_mode"] = serde_json::json!(value);
        let error = serde_json::from_value::<crate::native::AttachmentRecord>(invalid)
            .expect_err("invalid map mode at the record wire boundary");
        assert!(error.to_string().contains("map_mode"));
    }
}

/// Every admissible index writes the same JSON text through a writer-backed
/// serializer as through the value serializer: the digits, quoted, unescaped.
#[test]
fn map_mode_writes_the_same_text_through_a_writer() {
    for index in 0..super::MAP_MODE_NAMES.len() {
        let mode = super::MapModeIndex::try_new(index).expect("valid table index");
        assert_eq!(
            serde_json::to_string(&mode).expect("write index"),
            format!("\"{index}\"")
        );
    }
}
