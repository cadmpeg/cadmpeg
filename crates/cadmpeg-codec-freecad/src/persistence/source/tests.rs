// SPDX-License-Identifier: Apache-2.0
//! Literal source ranges, committed scalar producers, and opaque negatives.

use super::*;
use crate::persistence::{parse_document_source, Vocabulary};
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodePolicy};

fn document(properties: &str, count: usize) -> String {
    format!("<Document SchemaVersion='4'><Objects Count='1'><Object name='Owner' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Owner'><Properties Count='{count}'>{properties}</Properties></Object></ObjectData></Document>")
}

fn attribute_range(input: &str, name: &str, raw: &str) -> std::ops::Range<u64> {
    let start = input.find(&format!("{name}='{raw}'")).unwrap() + name.len() + "='".len();
    u64_from_index(start)..u64_from_index(start + raw.len())
}

fn identity_ranges(input: &str, name: &str, type_name: &str) -> Vec<std::ops::Range<u64>> {
    vec![
        attribute_range(input, "name", name),
        attribute_range(input, "type", type_name),
    ]
}

fn property_captures(
    ctx: &DecodeContext<'_>,
    graph: &crate::persistence::Graph,
) -> Vec<cadmpeg_core::decode::RecordCapture> {
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), graph.objects.len() + graph.properties.len());
    captures
        .into_iter()
        .filter(|capture| {
            graph
                .properties
                .iter()
                .any(|property| capture.id() == property.id)
        })
        .collect()
}

#[test]
fn kept_scalar_values_read_only_the_original_value_bytes_in_a_nonzero_source_window() {
    for float in ["1.25", "2.25"] {
        let xml = document(&format!("<Property name='Distance' type='App::PropertyFloat'><Float unit='mm' value='{float}'/></Property><Property name='Count' type='App::PropertyInteger'><Integer value='-7'/></Property><Property name='Visible' type='App::PropertyBool'><Bool value='True'/></Property>"), 3);
        let bytes = format!("prefix{xml}suffix");
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes.as_bytes(), &arena, &policy).unwrap();
        let source = root.child(6, 6 + xml.len()).unwrap();
        let dom =
            roxmltree::Document::parse(std::str::from_utf8(source.window()).unwrap()).unwrap();
        let graph = parse_document_source(&ctx, source, &dom, Vocabulary::Objects).unwrap();
        let captures = property_captures(&ctx, &graph);
        assert_eq!(captures.len(), 3);
        for (name, literal) in [("Distance", float), ("Count", "-7"), ("Visible", "True")] {
            let property = graph
                .properties
                .iter()
                .find(|property| property.name == name)
                .unwrap();
            assert_eq!(property.values()[0].attributes["value"], literal);
            let wire = serde_json::to_value(property).unwrap();
            assert_eq!(wire["values"][0]["attributes"]["value"], literal);
            let capture = captures
                .iter()
                .find(|capture| capture.id() == property.id)
                .unwrap();
            let start = bytes.find(&format!("value='{literal}'")).unwrap() + "value='".len();
            let type_name = match name {
                "Distance" => "App::PropertyFloat",
                "Count" => "App::PropertyInteger",
                "Visible" => "App::PropertyBool",
                _ => unreachable!(),
            };
            let mut expected = identity_ranges(&bytes, name, type_name);
            expected.push(u64_from_index(start)..u64_from_index(start + literal.len()));
            assert_eq!(capture.reads()["file"], expected);
            assert_eq!(capture.reads().len(), 1);
        }
    }
}

#[test]
fn decoded_scalar_entities_credit_the_exact_original_entity_spelling_and_no_markup() {
    let xml = document(
        "<Property name='Distance' type='App::PropertyFloat'><Float value='1&#x2e;5'/></Property>",
        1,
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, source) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
    let dom = roxmltree::Document::parse(&xml).unwrap();
    let graph = parse_document_source(&ctx, source, &dom, Vocabulary::Objects).unwrap();
    assert_eq!(graph.properties[0].values()[0].attributes["value"], "1.5");
    let captures = property_captures(&ctx, &graph);
    let start = xml.find("1&#x2e;5").unwrap();
    assert_eq!(captures.len(), 1);
    let mut expected = identity_ranges(&xml, "Distance", "App::PropertyFloat");
    expected.push(u64_from_index(start)..u64_from_index(start + "1&#x2e;5".len()));
    assert_eq!(captures[0].reads()["file"], expected);
}

#[test]
fn unreadable_and_opaque_property_values_receive_no_scalar_read_credit() {
    let xml = document("<Property name='Invalid' type='App::PropertyBool'><Bool value='maybe'/></Property><Property name='Proxy' type='App::PropertyPythonObject'><Python value='AAAB-pickle'/></Property><Property name='Vendor' type='Vendor::PropertyInteger'><Integer value='7'/></Property>", 3);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, source) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
    let dom = roxmltree::Document::parse(&xml).unwrap();
    let graph = parse_document_source(&ctx, source, &dom, Vocabulary::Objects).unwrap();
    assert!(matches!(
        graph.properties[0].body,
        crate::native::PropertyBody::Unreadable(_)
    ));
    assert_eq!(
        graph.properties[1].values()[0].attributes["value"],
        "AAAB-pickle"
    );
    assert_eq!(graph.properties[2].values()[0].attributes["value"], "7");
    let captures = property_captures(&ctx, &graph);
    assert_eq!(captures.len(), 3);
    for (name, type_name) in [
        ("Invalid", "App::PropertyBool"),
        ("Proxy", "App::PropertyPythonObject"),
        ("Vendor", "Vendor::PropertyInteger"),
    ] {
        let property = graph
            .properties
            .iter()
            .find(|property| property.name == name)
            .unwrap();
        assert_eq!(property.type_name, type_name);
        let capture = captures
            .iter()
            .find(|capture| capture.id() == property.id)
            .unwrap();
        assert_eq!(
            capture.reads()["file"],
            identity_ranges(&xml, name, type_name)
        );
    }
}

#[test]
fn a_discarded_scalar_producer_rolls_back_its_read_and_record() {
    let xml = "<Float value='19.5'/>";
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, view) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
    let dom = roxmltree::Document::parse(xml).unwrap();
    let source = Source::new(&ctx, view, &dom).unwrap();
    let node = dom.root_element();
    let attribute = node.attributes().next().unwrap();
    let result: Result<(), CodecError> =
        source.record("fcstd:native:property#discarded", node, || {
            assert_eq!(source.attribute(&attribute)?, "19.5");
            Err(CodecError::malformed("discard this producer"))
        });
    assert!(result.is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn a_dom_from_another_buffer_cannot_attribute_reads_to_equal_source_bytes() {
    let xml = document(
        "<Property name='Count' type='App::PropertyInteger'><Integer value='7'/></Property>",
        1,
    );
    let other = xml.clone();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, source) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
    let dom = roxmltree::Document::parse(&other).unwrap();
    assert!(parse_document_source(&ctx, source, &dom, Vocabulary::Objects).is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn nested_scalar_lookalikes_do_not_get_the_direct_scalar_producers_read_credit() {
    let xml = document("<Property name='Distance' type='App::PropertyFloat'><Float value='12.5'><Float value='999'/></Float></Property>", 1);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, view) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
    let dom = roxmltree::Document::parse(&xml).unwrap();
    let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
    assert_eq!(graph.properties[0].values()[0].attributes["value"], "12.5");
    assert_eq!(graph.properties[0].values()[1].attributes["value"], "999");
    let captures = property_captures(&ctx, &graph);
    let start = xml.find("12.5").unwrap();
    assert_eq!(captures.len(), 1);
    let mut expected = identity_ranges(&xml, "Distance", "App::PropertyFloat");
    expected.push(u64_from_index(start)..u64_from_index(start + 4));
    assert_eq!(captures[0].reads()["file"], expected);
}

mod object_declarations;
mod property_identity;
mod string_values;

mod vector_values;

mod dynamic_metadata;

mod vector_families;

mod constraint_values;
