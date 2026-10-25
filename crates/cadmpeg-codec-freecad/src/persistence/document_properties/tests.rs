// SPDX-License-Identifier: Apache-2.0
//! Every property family uses source element recovery and native ownership.

use super::*;
use crate::native::PropertyFamily;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartState;

#[test]
fn explicit_json_mode_keeps_valid_scalar_values_and_document_projections() {
    for carrier in [
        "json='yes' value='7'",
        "json='yes' encoded='yes' value='Nw=='",
    ] {
        let xml = format!("<Document><Properties Count='1'><Property name='Json' type='App::PropertyPythonObject'><Python {carrier}/></Property></Properties></Document>");
        let arena = DecodeArena::new();
        let (ctx, view) =
            DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service())
                .unwrap();
        let source = std::rc::Rc::new(XmlSource::new(&ctx, view).unwrap());
        let entries = BTreeMap::new();
        let context = crate::element_map::adopted::Context::new(&entries, None);
        let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
        let parsed = decode_scanned(&ctx, &source, 0, &context, &mut output)
            .unwrap()
            .unwrap();
        crate::test_support::source_fidelity::project_document_attributes(
            &ctx,
            view,
            &parsed,
            &mut output,
        );
        let property = &parsed.properties[0];
        assert_eq!(
            property.values()[0].decoded_json.as_ref().unwrap().value,
            serde_json::json!(7)
        );
        assert_eq!(
            output.model().attributes[0].native_ref.as_deref(),
            Some(property.id.as_str())
        );
        assert_eq!(
            output.model().attributes[0].values,
            [cadmpeg_ir::attributes::AttributeValue::Integer(7)]
        );
        assert!(output.losses().is_empty());
        let emitted = crate::writer::xml::native(property).unwrap().unwrap();
        let tree = roxmltree::Document::parse(&emitted).unwrap();
        let python = tree.root_element().first_element_child().unwrap();
        assert_eq!(python.attribute("json"), Some("yes"));
        assert_eq!(python.attribute("encoded"), Some("yes"));
        use base64::Engine;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(python.attribute("value").unwrap())
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&raw).unwrap(),
            serde_json::json!(7)
        );
    }
}

#[test]
fn lost_document_projection_is_named_only_by_its_actual_failed_value_home() {
    for (kind, value, sponsored, projected) in [
        (
            "App::PropertyInteger",
            "<Integer value='bad'/>",
            true,
            false,
        ),
        (
            "App::PropertyStringList",
            "<StringList count='2'><String value='kept'/><String value='&missing;'/></StringList>",
            true,
            false,
        ),
        (
            "App::PropertyInteger",
            "<Integer value='7'><Unexpected/></Integer>",
            false,
            true,
        ),
        (
            "App::PropertyMap",
            "<Map count='1'><Item key='K' value='&missing;'/></Map>",
            true,
            false,
        ),
        (
            "App::PropertyPythonObject",
            "<Python json='yes' value='invalid-json'/>",
            true,
            false,
        ),
        (
            "App::PropertyPythonObject",
            "<Python json='yes' encoded='yes' value='invalid-base64'/>",
            true,
            false,
        ),
    ] {
        let xml = format!("<Document><Properties Count='1'><Property name='Damaged' type='{kind}'>{value}</Property></Properties></Document>");
        let arena = DecodeArena::new();
        let (ctx, view) =
            DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service())
                .unwrap();
        let source = std::rc::Rc::new(XmlSource::new(&ctx, view).unwrap());
        let entries = BTreeMap::new();
        let context = crate::element_map::adopted::Context::new(&entries, None);
        let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
        let parsed = decode_scanned(&ctx, &source, 0, &context, &mut output)
            .unwrap()
            .unwrap();
        crate::test_support::source_fidelity::project_document_attributes(
            &ctx,
            view,
            &parsed,
            &mut output,
        );
        let property = &parsed.properties[0];
        let id = crate::native::model_id("attribute", &property.id, "property");
        assert_eq!(
            output.model().attributes.len(),
            usize::from(projected),
            "{kind}"
        );
        let failures: Vec<_> = output
            .losses()
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .collect();
        assert_eq!(failures.len(), 1, "{kind}");
        assert_eq!(failures[0].message.contains(&id), sponsored, "{kind}");
        let unknowns = output
            .native()
            .namespace("fcstd")
            .unwrap()
            .arena_as::<cadmpeg_ir::unknown::NativeUnknownRecord>("unknowns")
            .unwrap();
        assert_eq!(unknowns.len(), 1);
        assert_eq!(unknowns[0].links.len(), 1);
        assert_eq!(unknowns[0].links[0].as_str(), property.id);
        let failure = output
            .fidelity()
            .parts()
            .values()
            .find(|part| part.state() == &PartState::GivenUp {})
            .unwrap();
        assert!(matches!(
            failure.level().as_str(),
            "freecad.property_value" | "freecad.value_element"
        ));
        let captures = ctx.take_record_captures().unwrap();
        let capture = captures
            .iter()
            .find(|capture| capture.id() == property.id)
            .unwrap();
        assert!(capture.reads().values().flatten().all(|read| {
            read.end <= failure.offset() || read.start >= failure.offset() + failure.byte_len()
        }));
    }
}

#[test]
fn every_property_family_retains_its_readable_value_inside_the_same_part_path() {
    let values = [
        ("App::PropertyFloat", "<Float value='2.5'/>"),
        ("App::PropertyLength", "<Float value='7'/>"),
        ("App::PropertyEnumeration", "<Integer value='1'/>"),
        (
            "App::PropertyVector",
            "<PropertyVector valueX='1' valueY='2' valueZ='3'/>",
        ),
        ("App::PropertyMatrix", "<PropertyMatrix a11='1' a12='0' a13='0' a14='0' a21='0' a22='1' a23='0' a24='0' a31='0' a32='0' a33='1' a34='0' a41='0' a42='0' a43='0' a44='1'/>"),
        ("App::PropertyPlacement", "<PropertyPlacement Px='1' Py='0' Pz='0' Q0='0' Q1='0' Q2='0' Q3='1'/>"),
        ("App::PropertyString", "<String value='kept'/>"),
        ("App::PropertyMap", "<Map count='1'><Item key='K' value='V'/></Map>"),
        (
            "App::PropertyFloatList",
            "<FloatList file=''/>",
        ),
        ("App::PropertyLink", "<Link value='Target'/>"),
        (
            "App::PropertyExpressionEngine",
            "<ExpressionEngine count='1'><Expression path='Length' expression='2'/></ExpressionEngine>",
        ),
        ("App::PropertyFile", "<String value='kept.txt'/>"),
        ("Part::PropertyPartShape", "<Part file=''/>"),
        (
            "App::PropertyPythonObject",
            "<Python json='yes' value='null'/>",
        ),
        (
            "App::PropertyMap",
            "<Map count='1'><Item key='typed' value='kept'/></Map>",
        ),
    ];
    let properties = values
        .iter()
        .enumerate()
        .map(|(index, (kind, value))| {
            format!("<Property name='P{index}' type='{kind}'>{value}</Property>")
        })
        .collect::<String>();
    let xml = format!(
        "<Document><Properties Count='{}'>{properties}</Properties><Objects Count='1'><Object name='Target' type='App::Feature'/></Objects><ObjectData Count='0'/></Document>",
        values.len()
    );
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let source = std::rc::Rc::new(XmlSource::new(&ctx, view).unwrap());
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    crate::persistence::object_parts::decode_scanned(
        &ctx,
        &source,
        0,
        crate::persistence::Vocabulary::Objects,
        &context,
        &mut output,
    )
    .unwrap();
    let parsed = decode_scanned(&ctx, &source, 0, &context, &mut output)
        .unwrap()
        .unwrap();
    assert_eq!(parsed.properties.len(), values.len());
    for (property, (kind, _)) in parsed.properties.iter().zip(values) {
        assert_eq!(property.type_name, kind);
        assert!(property.body.is_some(), "{kind}");
        assert!(!property.values().is_empty(), "{kind}");
        assert!(parsed.sources[&property.id].has_values());
    }
    assert!(!output
        .fidelity()
        .parts()
        .values()
        .any(|part| part.state() == &PartState::GivenUp {}));
    for level in [
        "freecad.properties",
        "freecad.property",
        "freecad.property_value",
        "freecad.value_element",
    ] {
        assert!(output
            .fidelity()
            .parts()
            .values()
            .any(|part| part.level().as_str() == level));
    }
    let captures = ctx.take_record_captures().unwrap();
    let capture = captures
        .iter()
        .find(|capture| capture.id() == parsed.properties[14].id)
        .unwrap();
    let start = xml.find("typed").unwrap() as u64;
    assert!(capture.reads()["file"]
        .iter()
        .any(|range| range.start <= start && range.end >= start + 5));
}

#[test]
fn malformed_nested_value_discards_only_its_element_and_preserves_parent_fields_and_peer() {
    let xml = "<Document><Properties Count='1'><Property name='Tree' type='App::PropertyMap'><Map count='2' own='retained'><Item key='A' value='bad'>&undeclared;</Item><Item key='B' value='kept'/></Map></Property></Properties></Document>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let source = std::rc::Rc::new(XmlSource::new(&ctx, view).unwrap());
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let parsed = decode_scanned(&ctx, &source, 0, &context, &mut output)
        .unwrap()
        .unwrap();
    let property = &parsed.properties[0];
    assert_eq!(property.values().len(), 2);
    assert_eq!(property.values()[0].attributes["own"], "retained");
    assert_eq!(property.values()[1].tag, "Item");
    assert_eq!(property.values()[1].parent, Some(0));
    assert_eq!(property.values()[1].order, 2);
    assert_eq!(property.values()[1].attributes["value"], "kept");
    let wire = serde_json::to_value(property).unwrap();
    assert!(wire["values"][1].is_null());
    assert_eq!(wire["values"].as_array().unwrap().len(), 3);
    let given_up: Vec<_> = output
        .fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .collect();
    assert_eq!(given_up.len(), 1);
    assert_eq!(given_up[0].level().as_str(), "freecad.value_element");
    assert_eq!(
        output
            .losses()
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    assert_eq!(
        output
            .native()
            .namespace("fcstd")
            .unwrap()
            .arena_as::<cadmpeg_ir::unknown::NativeUnknownRecord>("unknowns")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn unknown_and_opaque_python_payloads_have_no_semantic_fields_or_read_credit() {
    for (kind, value) in [
        ("Vendor::Unregistered", "<Value data='opaque-payload'><Payload secret='raw-secret'>raw-body</Payload></Value>"),
        (
            "App::PropertyPythonObject",
            "<Python value='opaque-payload'><Payload secret='raw-secret'>raw-body</Payload></Python>",
        ),
    ] {
        let xml = format!("<Document><Properties Count='1'><Property name='Opaque' type='{kind}'>{value}</Property></Properties></Document>");
        let arena = DecodeArena::new();
        let (ctx, view) =
            DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service())
                .unwrap();
        let source = std::rc::Rc::new(XmlSource::new(&ctx, view).unwrap());
        let entries = BTreeMap::new();
        let context = crate::element_map::adopted::Context::new(&entries, None);
        let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
        let parsed = decode_scanned(&ctx, &source, 0, &context, &mut output)
            .unwrap()
            .unwrap();
        let property = &parsed.properties[0];
        assert_eq!(property.name, "Opaque");
        assert_eq!(property.type_name, kind);
        assert!(property
            .values()
            .iter()
            .all(|value| value.attributes.is_empty()
                && value.text.is_none()
                && value.decoded_json.is_none()));
        if kind == "Vendor::Unregistered" {
            assert!(property.values().is_empty());
        }
        let captures = ctx.take_record_captures().unwrap();
        let capture = captures
            .iter()
            .find(|capture| capture.id() == property.id)
            .unwrap();
        for raw in ["opaque-payload", "raw-secret", "raw-body"] {
            let start = xml.find(raw).unwrap() as u64;
            let end = start + raw.len() as u64;
            assert!(capture.reads()["file"].iter().all(|range| range.end <= start || range.start >= end));
        }
        let wire = serde_json::to_value(property).unwrap();
        assert_eq!(wire["values"].as_array().unwrap().len(), usize::from(kind != "Vendor::Unregistered"));
        assert!(!output.fidelity().parts().values().any(|part| part.level().as_str() == "freecad.value_element"));
        assert!(!output
            .fidelity()
            .parts()
            .values()
            .any(|part| part.state() == &PartState::GivenUp {}));
    }
}

#[test]
fn unreadable_link_list_child_keeps_its_null_position_between_readable_targets() {
    let xml = "<Document><Properties Count='1'><Property name='Links' type='App::PropertyLinkList'><LinkList count='3'><Link value='A'/><Link Value='bad'/><Link value='B'/></LinkList></Property></Properties><Objects Count='2'><Object name='A' type='App::Feature'/><Object name='B' type='App::Feature'/></Objects><ObjectData Count='0'/></Document>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let source = std::rc::Rc::new(XmlSource::new(&ctx, view).unwrap());
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    crate::persistence::object_parts::decode_scanned(
        &ctx,
        &source,
        0,
        crate::persistence::Vocabulary::Objects,
        &context,
        &mut output,
    )
    .unwrap();
    let parsed = decode_scanned(&ctx, &source, 0, &context, &mut output)
        .unwrap()
        .unwrap();
    let property = &parsed.properties[0];
    assert_eq!(property.links().len(), 3);
    assert_eq!(
        property.links()[0].as_ref().unwrap().object(),
        Some(crate::native::native_id("object", "A").as_str())
    );
    assert!(property.links()[1].is_none());
    let wire = serde_json::to_value(property).unwrap();
    assert_eq!(wire["values"].as_array().unwrap().len(), 4);
    assert!(wire["values"][2].is_null());
    assert_eq!(wire["values"][3]["order"], 3);
    assert_eq!(wire["values"][3]["attributes"]["value"], "B");
    assert_eq!(
        property.links()[2].as_ref().unwrap().object(),
        Some(crate::native::native_id("object", "B").as_str())
    );
    assert_eq!(
        property
            .values()
            .iter()
            .map(|value| value.tag.as_str())
            .collect::<Vec<_>>(),
        ["LinkList", "Link", "Link"]
    );
    let failed: Vec<_> = output
        .fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level().as_str(), "freecad.value_element");
    assert_eq!(
        output
            .losses()
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
}

#[test]
fn collection_indentation_has_no_typed_text_or_semantic_read_credit() {
    let xml = "<Document><Properties Count='1'><Property name='Words' type='App::PropertyStringList'><StringList count='1'>\n    <String value='kept'/>\n  </StringList></Property></Properties></Document>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let source = std::rc::Rc::new(XmlSource::new(&ctx, view).unwrap());
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let parsed = decode_scanned(&ctx, &source, 0, &context, &mut output)
        .unwrap()
        .unwrap();
    let property = &parsed.properties[0];
    assert_eq!(property.values().len(), 2);
    assert!(property.values().iter().all(|value| value.text.is_none()));
    let captures = ctx.take_record_captures().unwrap();
    let capture = captures
        .iter()
        .find(|capture| capture.id() == property.id)
        .unwrap();
    for raw in ["\n    ", "\n  "] {
        let start = xml.find(raw).unwrap() as u64;
        let end = start + raw.len() as u64;
        assert!(capture.reads()["file"]
            .iter()
            .all(|range| range.end <= start || range.start >= end));
    }
}

#[test]
fn side_entry_references_require_real_headers_while_readable_filenames_survive() {
    use cadmpeg_ir::{Codec, DecodeOptions};
    use cadmpeg_test_support::zip_fixture::{archive, RawEntry};
    for declared in [false, true] {
        let xml = "<Document SchemaVersion='4'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part file='missing.brp'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>";
        let mut entries = vec![RawEntry::stored("Document.xml", xml.as_bytes())];
        if declared {
            let mut entry = RawEntry::stored("missing.brp", b"unreadable shape payload");
            entry.crc32 ^= 1;
            entries.push(entry);
        }
        let decoded = crate::FcstdCodec
            .decode(
                &mut std::io::Cursor::new(archive(&entries)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let native = decoded.ir().native.namespace("fcstd").unwrap();
        let properties = native
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        let property = properties
            .iter()
            .find(|property| property.name == "Shape")
            .unwrap();
        assert_eq!(property.values()[0].attributes["file"], "missing.brp");
        let expected: Vec<String> = if declared {
            vec!["missing.brp".into()]
        } else {
            vec![]
        };
        assert_eq!(property.side_entries(), expected.as_slice());
        let entry_records = native
            .arena_as::<crate::native::EntryRecord>("entries")
            .unwrap();
        assert_eq!(
            entry_records
                .iter()
                .any(|entry| entry.name() == "missing.brp"),
            declared
        );
        assert!(!decoded
            .source_fidelity()
            .parts()
            .values()
            .any(|part| part.level().as_str() == "freecad.property_value"
                && part.state() == &PartState::GivenUp {}));
        let failed: Vec<_> = decoded
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::GivenUp {})
            .collect();
        assert_eq!(failed.len(), usize::from(declared));
        if declared {
            assert_eq!(failed[0].level().as_str(), "zip.entry");
        }
    }
}

#[test]
fn never_declared_local_targets_fail_only_their_link_leaf_and_external_targets_survive() {
    use cadmpeg_ir::{Codec, DecodeOptions};
    use cadmpeg_test_support::zip_fixture::{archive, RawEntry};
    let xml = "<Document SchemaVersion='4'><Properties Count='3'><Property name='Single' type='App::PropertyLink'><Link value='Missing'/></Property><Property name='List' type='App::PropertyLinkList'><LinkList count='3'><Link value='A'/><Link value='Missing'/><Link value='B'/></LinkList></Property><Property name='External' type='App::PropertyXLink'><XLink file='other.FCStd' name='Missing'/></Property></Properties><Objects Count='2'><Object name='A' type='App::DocumentObjectGroup'/><Object name='B' type='App::DocumentObjectGroup'/></Objects><ObjectData Count='0'/></Document>";
    let decoded = crate::FcstdCodec
        .decode(
            &mut std::io::Cursor::new(archive(&[RawEntry::stored("Document.xml", xml.as_bytes())])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let properties = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let single = properties
        .iter()
        .find(|property| property.name == "Single")
        .unwrap();
    assert!(single.body.is_none());
    let list = properties
        .iter()
        .find(|property| property.name == "List")
        .unwrap();
    assert_eq!(list.links().len(), 3);
    assert!(list.links()[1].is_none());
    assert_eq!(
        list.links()[0].as_ref().unwrap().object(),
        Some(crate::native::native_id("object", "A").as_str())
    );
    assert_eq!(
        list.links()[2].as_ref().unwrap().object(),
        Some(crate::native::native_id("object", "B").as_str())
    );
    assert!(serde_json::to_value(list).unwrap()["values"][2].is_null());
    let external = properties
        .iter()
        .find(|property| property.name == "External")
        .unwrap();
    assert_eq!(
        external.links()[0].as_ref().unwrap().document_name(),
        Some("other.FCStd")
    );
    assert_eq!(
        external.links()[0].as_ref().unwrap().object(),
        Some("Missing")
    );
    let failed: Vec<_> = decoded
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .map(|part| part.level().as_str())
        .collect();
    assert_eq!(failed.len(), 2);
    assert!(failed.contains(&"freecad.property_value"));
    assert!(failed.contains(&"freecad.value_element"));
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
}

#[test]
fn failed_declared_link_targets_leave_null_slots_without_value_read_credit_or_new_failure() {
    let property = "<Property name='List' type='App::PropertyLinkList'><LinkList count='4'><Link value='A'/><Link value='Failed' discarded='unretained'/><Link value=''/><Link value='B'/></LinkList></Property>";
    let single = "<Property name='Single' type='App::PropertyLink'><Link value='Failed' discarded='unretained-scalar'/></Property>";
    let xml = format!("<Document><Properties Count='2'>{property}{single}</Properties><Objects Count='3'><Object name='A' type='App::Feature'/><Object name='Failed'/><Object name='B' type='App::Feature'/></Objects><ObjectData Count='0'/></Document>");
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let source = std::rc::Rc::new(XmlSource::new(&ctx, view).unwrap());
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    crate::persistence::object_parts::decode_scanned(
        &ctx,
        &source,
        0,
        crate::persistence::Vocabulary::Objects,
        &context,
        &mut output,
    )
    .unwrap();
    let parsed = decode_scanned(&ctx, &source, 0, &context, &mut output)
        .unwrap()
        .unwrap();
    let list = parsed
        .properties
        .iter()
        .find(|row| row.name == "List")
        .unwrap();
    assert_eq!(list.links().len(), 4);
    assert_eq!(
        list.links()[0].as_ref().unwrap().object(),
        Some(crate::native::native_id("object", "A").as_str())
    );
    assert!(list.links()[1].is_none());
    assert!(list.links()[2].is_none());
    assert_eq!(
        list.links()[3].as_ref().unwrap().object(),
        Some(crate::native::native_id("object", "B").as_str())
    );
    let wire = serde_json::to_value(list).unwrap();
    assert!(wire["values"][2].is_null());
    assert_eq!(wire["values"][3]["attributes"]["value"], "");
    assert_eq!(wire["values"].as_array().unwrap().len(), 5);
    let single = parsed
        .properties
        .iter()
        .find(|row| row.name == "Single")
        .unwrap();
    assert!(single.body.is_some());
    assert!(single.values().is_empty());
    assert_eq!(single.links(), &[None]);
    assert_eq!(
        serde_json::to_value(single).unwrap()["values"],
        serde_json::json!([null])
    );
    assert!(crate::writer::xml::native(single).unwrap().is_none());
    let captures = ctx.take_record_captures().unwrap();
    for row in [&list, &single] {
        let capture = captures
            .iter()
            .find(|capture| capture.id() == row.id)
            .unwrap();
        let source_property = if row.name == "List" {
            property
        } else {
            "<Property name='Single' type='App::PropertyLink'><Link value='Failed' discarded='unretained-scalar'/></Property>"
        };
        let base = xml.find(source_property).unwrap();
        let mut expected: Vec<std::ops::Range<u64>> = Vec::new();
        for token in if row.name == "List" {
            vec![
                " name='List'",
                " type='App::PropertyLinkList'",
                " count='4'",
                " value='A'",
                " value=''",
                " value='B'",
            ]
        } else {
            vec![" name='Single'", " type='App::PropertyLink'"]
        } {
            let token = token.trim_start();
            let (name, quoted) = token.split_once("='").unwrap();
            let raw = quoted.strip_suffix('\'').unwrap();
            let located = source_property.find(token).unwrap();
            let opening = source_property[..located].rfind('<').unwrap() + 1;
            let end = opening
                + source_property[opening..]
                    .find(|character: char| {
                        character.is_ascii_whitespace() || matches!(character, '/' | '>')
                    })
                    .unwrap();
            let tag = &source_property[opening..end];
            if tag != "Property" {
                expected.push((base + opening) as u64..(base + end) as u64);
            }
            let start = (base + located) as u64;
            expected.push(start..start + name.len() as u64);
            if !raw.is_empty() {
                let value_start = start + name.len() as u64 + 2;
                expected.push(value_start..value_start + raw.len() as u64);
            }
        }
        assert_eq!(capture.reads()["file"], expected);
    }
    let failed: Vec<_> = output
        .fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level().as_str(), "freecad.object_declaration");
}

#[test]
fn spreadsheet_registered_values_recover_individual_cells_and_dimensions_with_original_slots() {
    let xml = "<Document><Properties Count='3'><Property name='cells' type='Spreadsheet::PropertySheet'><Cells Count='3' xlink='1'><XLinks count='0'/><Cell address='A1' content='5' alias='width' rowSpan='0' Vendor='retained'/><Cell address='invalid' content='discarded'/><Cell address='B1' content='=width * 2'/></Cells></Property><Property name='columnWidths' type='Spreadsheet::PropertyColumnWidths'><ColumnInfo Count='2'><Column name='A' width='120'/><Column name='B' width='bad'/></ColumnInfo></Property><Property name='rowHeights' type='Spreadsheet::PropertyRowHeights'><RowInfo Count='1'><Row name='2' height='45'/></RowInfo></Property></Properties></Document>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let source = XmlSource::new(&ctx, view).unwrap();
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let parsed = decode_scanned(&ctx, &source, 0, &context, &mut output)
        .unwrap()
        .unwrap();
    let cells = &parsed.properties[0];
    assert_eq!(cells.family, PropertyFamily::List);
    assert_eq!(cells.values().len(), 4);
    assert_eq!(cells.values()[1].tag, "XLinks");
    assert_eq!(cells.values()[2].attributes["Vendor"], "retained");
    assert_eq!(cells.values()[2].attributes["rowSpan"], "0");
    assert_eq!(cells.values()[3].order, 4);
    assert_eq!(cells.values()[3].attributes["content"], "=width * 2");
    assert!(serde_json::to_value(cells).unwrap()["values"][3].is_null());
    let cell_xml = crate::writer::xml::native(cells).unwrap().unwrap();
    let document = roxmltree::Document::parse(&cell_xml).unwrap();
    assert_eq!(
        document
            .descendants()
            .find(|node| node.has_tag_name("Cells"))
            .unwrap()
            .attribute("Count"),
        Some("2")
    );
    let columns = &parsed.properties[1];
    assert!(serde_json::to_value(columns).unwrap()["values"][2].is_null());
    let column_xml = crate::writer::xml::native(columns).unwrap().unwrap();
    let document = roxmltree::Document::parse(&column_xml).unwrap();
    assert_eq!(
        document
            .descendants()
            .find(|node| node.has_tag_name("ColumnInfo"))
            .unwrap()
            .attribute("Count"),
        Some("1")
    );
    assert_eq!(parsed.properties[2].values()[1].attributes["height"], "45");
    let failures: Vec<_> = output
        .fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .collect();
    assert_eq!(failures.len(), 2);
    assert!(failures
        .iter()
        .all(|part| part.level().as_str() == "freecad.value_element"));
    let captures = ctx.take_record_captures().unwrap();
    let cell_capture = captures
        .iter()
        .find(|capture| capture.id() == cells.id)
        .unwrap();
    let missing = xml.find(" address='invalid' content='discarded'").unwrap() as u64;
    assert!(cell_capture.reads()["file"]
        .iter()
        .all(|range| range.end <= missing
            || range.start >= missing + " address='invalid' content='discarded'".len() as u64));
}

#[test]
fn expression_xlinks_keep_original_metadata_and_explicit_source_link_ordinals_across_missing_targets(
) {
    let xml = "<Document><Properties Count='1'><Property name='cells' type='Spreadsheet::PropertySheet'><Cells Count='1' xlink='1'><XLinks count='3' docs='2' hidden='0 2'><DocMap name='Local' label='Local document' index='0'/><DocMap name='Other' label='Other document' index='2'/><XLink file='' name='A'/><XLink file='' name='Failed'/><XLink file='other.FCStd' name='B'/></XLinks><Cell address='A1' content='=Other.B.Length'/></Cells></Property></Properties><Objects Count='2'><Object name='A' type='App::Feature'/><Object name='Failed'/></Objects><ObjectData Count='0'/></Document>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let source = std::rc::Rc::new(XmlSource::new(&ctx, view).unwrap());
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    crate::persistence::object_parts::decode_scanned(
        &ctx,
        &source,
        0,
        crate::persistence::Vocabulary::Objects,
        &context,
        &mut output,
    )
    .unwrap();
    let parsed = decode_scanned(&ctx, &source, 0, &context, &mut output)
        .unwrap()
        .unwrap();
    let property = &parsed.properties[0];
    assert_eq!(property.links().len(), 3);
    assert_eq!(
        property.links()[0].as_ref().unwrap().object(),
        Some(crate::native::native_id("object", "A").as_str())
    );
    assert!(property.links()[1].is_none());
    assert_eq!(
        property.links()[2].as_ref().unwrap().document_name(),
        Some("other.FCStd")
    );
    assert!(property.side_entries().is_empty());
    let values = property.values();
    let links: Vec<_> = values.iter().filter(|value| value.tag == "XLink").collect();
    assert_eq!(
        links
            .iter()
            .map(|value| value.link_index)
            .collect::<Vec<_>>(),
        [Some(0), Some(2)]
    );
    assert_eq!(
        values
            .iter()
            .find(|value| value.tag == "XLinks")
            .unwrap()
            .attributes["hidden"],
        "0 2"
    );
    assert_eq!(
        values
            .iter()
            .filter(|value| value.tag == "DocMap")
            .map(|value| value.attributes["index"].as_str())
            .collect::<Vec<_>>(),
        ["0", "2"]
    );
    let wire = serde_json::to_value(property).unwrap();
    assert!(wire["values"][5].is_null());
    assert_eq!(wire["values"][4]["link_index"], 0);
    assert_eq!(wire["values"][6]["link_index"], 2);
    let capture = ctx
        .take_record_captures()
        .unwrap()
        .into_iter()
        .find(|capture| capture.id() == property.id)
        .unwrap();
    let start = xml.find(" file='' name='Failed'").unwrap() as u64;
    assert!(capture.reads()["file"]
        .iter()
        .all(|range| range.end <= start
            || range.start >= start + " file='' name='Failed'".len() as u64));
    let failed: Vec<_> = output
        .fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level().as_str(), "freecad.object_declaration");
}

#[test]
fn map_entries_keep_repeated_keys_and_nonascending_source_order() {
    let xml = "<Document><Properties Count='1'><Property name='Meta' type='App::PropertyMap'><Map count='3'><Item key='b' value='2'/><Item key='a' value='1'/><Item key='b' value='3'/></Map></Property></Properties></Document>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let source = std::rc::Rc::new(XmlSource::new(&ctx, view).unwrap());
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let parsed = decode_scanned(&ctx, &source, 0, &context, &mut output)
        .unwrap()
        .unwrap();
    let items = parsed.properties[0]
        .values()
        .iter()
        .filter(|value| value.tag == "Item")
        .map(|value| {
            (
                value.attributes["key"].as_str(),
                value.attributes["value"].as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(items, [("b", "2"), ("a", "1"), ("b", "3")]);
    assert!(output.losses().is_empty());
    assert!(output
        .fidelity()
        .parts()
        .values()
        .all(|part| part.state() == &PartState::Decoded {}));
}

#[test]
fn an_unavailable_link_sub_target_keeps_its_real_child_scope_without_semantic_reads() {
    let sub = "<Sub value='Edge1' shadow=';#18:1;:H1,E.Edge1'/>";
    let bad = format!("<Property name='Unavailable' type='App::PropertyLinkSub'><LinkSub value='Failed' count='1'>{sub}</LinkSub></Property>");
    let good = format!("<Property name='Readable' type='App::PropertyLinkSub'><LinkSub value='A' count='1'>{sub}</LinkSub></Property>");
    let xml = format!("<Document><Properties Count='2'>{bad}{good}</Properties><Objects Count='2'><Object name='A' type='App::Feature'/><Object name='Failed'/></Objects><ObjectData Count='0'/></Document>");
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let source = std::rc::Rc::new(XmlSource::new(&ctx, view).unwrap());
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    crate::persistence::object_parts::decode_scanned(
        &ctx,
        &source,
        0,
        crate::persistence::Vocabulary::Objects,
        &context,
        &mut output,
    )
    .unwrap();
    let parsed = decode_scanned(&ctx, &source, 0, &context, &mut output)
        .unwrap()
        .unwrap();
    let unavailable = parsed
        .properties
        .iter()
        .find(|property| property.name == "Unavailable")
        .unwrap();
    assert_eq!(
        serde_json::to_value(unavailable).unwrap()["values"],
        serde_json::json!([null, null])
    );
    assert_eq!(unavailable.links(), &[None]);
    let readable = parsed
        .properties
        .iter()
        .find(|property| property.name == "Readable")
        .unwrap();
    assert_eq!(readable.values()[1].tag, "Sub");
    assert_eq!(readable.values()[1].attributes["value"], "Edge1");
    assert_eq!(
        readable.values()[1].attributes["shadow"],
        ";#18:1;:H1,E.Edge1"
    );
    let sub_at = xml.find(sub).unwrap() as u64;
    let child = output
        .fidelity()
        .parts()
        .values()
        .find(|part| part.level().as_str() == "freecad.value_element" && part.offset() == sub_at)
        .unwrap();
    assert_eq!(child.byte_len(), sub.len() as u64);
    assert_eq!(child.state(), &(PartState::Decoded {}));
    assert_eq!(
        output.fidelity().parts()[child.parent().unwrap()]
            .level()
            .as_str(),
        "freecad.property_value"
    );
    assert_eq!(
        output
            .losses()
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    let captures = ctx.take_record_captures().unwrap();
    let capture = captures
        .iter()
        .find(|capture| capture.id() == unavailable.id)
        .unwrap();
    let root = (xml.find(&bad).unwrap() + bad.find("<LinkSub").unwrap()) as u64;
    assert!(capture.reads()[capture.stream()]
        .iter()
        .all(|read| read.end <= root || sub_at + sub.len() as u64 <= read.start));
}
