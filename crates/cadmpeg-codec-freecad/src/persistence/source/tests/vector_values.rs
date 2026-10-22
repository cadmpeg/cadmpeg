// SPDX-License-Identifier: Apache-2.0
//! Kept finite vector components consume only their genuine source value spans.

use super::*;
use cadmpeg_ir::attributes::AttributeValue;

#[test]
fn complete_vector_components_flow_into_kept_values_and_neutral_vectors_with_exact_reads() {
    for (raw_x, x) in [("1.25", 1.25), ("-6.5", -6.5)] {
        let xml = document(&format!("<Property name='ExtraVector' type='App::PropertyVector'><PropertyVector valueX='{raw_x}' valueY='&#45;2' valueZ='3' Vendor='opaque-extra'/></Property>"), 1);
        let bytes = format!("prefix{xml}suffix");
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes.as_bytes(), &arena, &policy).unwrap();
        let view = root
            .child("prefix".len(), "prefix".len() + xml.len())
            .unwrap();
        let dom = roxmltree::Document::parse(std::str::from_utf8(view.window()).unwrap()).unwrap();
        let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
        let property = &graph.properties[0];
        assert_eq!(property.name, "ExtraVector");
        assert_eq!(property.values().len(), 1);
        let value = &property.values()[0];
        assert_eq!(value.attributes["valueX"], raw_x);
        assert_eq!(value.attributes["valueY"], "-2");
        assert_eq!(value.attributes["valueZ"], "3");
        assert_eq!(value.attributes["Vendor"], "opaque-extra");
        let projected = crate::attributes::values(property).unwrap();
        let [AttributeValue::Vector(coordinates)] = projected.as_slice() else {
            panic!("one finite vector: {projected:?}")
        };
        assert_eq!(
            coordinates
                .iter()
                .map(|coordinate| coordinate.get())
                .collect::<Vec<_>>(),
            [x, -2.0, 3.0]
        );
        let extent = ctx.capture_part_extent(view).unwrap();
        let captures = property_captures(&ctx, &graph);
        let [capture] = captures.as_slice() else {
            panic!("one property capture: {captures:?}")
        };
        assert_eq!(capture.id(), property.id);
        let property_start = bytes.find("<Property name='ExtraVector'").unwrap();
        let property_end = bytes.find("</Property>").unwrap() + "</Property>".len();
        assert_eq!(capture.stream(), extent.stream());
        assert_eq!(capture.offset(), u64_from_index(property_start));
        assert_eq!(
            capture.byte_len(),
            u64_from_index(property_end - property_start)
        );
        assert_eq!(
            capture.reads(),
            &std::collections::BTreeMap::from([(
                "file".into(),
                vec![
                    attribute_range(&bytes, "name", "ExtraVector"),
                    attribute_range(&bytes, "type", "App::PropertyVector"),
                    attribute_range(&bytes, "valueX", raw_x),
                    attribute_range(&bytes, "valueY", "&#45;2"),
                    attribute_range(&bytes, "valueZ", "3"),
                ]
            )])
        );
    }
}

#[test]
fn partial_nonfinite_nested_multiple_vendor_and_overwritten_vectors_get_no_component_credit() {
    for (type_name, carrier, value_count, kept_text) in [
        ("App::PropertyVector", "<PropertyVector valueX='1' valueY='2'/>", 1, "1"),
        ("App::PropertyVector", "<PropertyVector valueX='bad' valueY='2' valueZ='3'/>", 1, "bad"),
        ("App::PropertyVector", "<PropertyVector valueX='NaN' valueY='2' valueZ='3'/>", 1, "NaN"),
        ("App::PropertyVector", "<PropertyVector valueX='inf' valueY='2' valueZ='3'/>", 1, "inf"),
        ("App::PropertyVector", "<PropertyVector valueX='1' valueY='2' valueZ='3'><Vendor value='opaque'/></PropertyVector>", 2, "opaque"),
        ("App::PropertyVector", "<PropertyVector valueX='1' valueY='2' valueZ='3'/><PropertyVector valueX='4' valueY='5' valueZ='6'/>", 2, "6"),
        ("Vendor::PropertyVector", "<PropertyVector valueX='1' valueY='2' valueZ='3'/>", 1, "3"),
        ("App::PropertyVector", "<Vector valueX='1' valueY='2' valueZ='3'/>", 1, "3"),
        ("App::PropertyVector", "<PropertyVector xmlns:v='urn:vendor' valueX='1' v:valueX='9' valueY='2' valueZ='3'/>", 1, "9"),
    ] {
        let xml = document(&format!("<Property name='Uncredited' type='{type_name}'>{carrier}</Property>"), 1);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, view) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
        let dom = roxmltree::Document::parse(&xml).unwrap();
        let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
        let property = &graph.properties[0];
        assert!(matches!(property.body, crate::native::PropertyBody::Persisted { .. }));
        assert_eq!(property.values().len(), value_count, "{carrier}");
        assert!(property.values().iter().any(|value| value.attributes.values().any(|text| text == kept_text)), "original native field stays: {carrier}");
        let captures = property_captures(&ctx, &graph);
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].reads(), &std::collections::BTreeMap::from([("file".into(), identity_ranges(&xml, "Uncredited", type_name))]), "{carrier}");
    }
}

#[test]
fn discarding_a_real_vector_property_producer_rolls_back_all_component_reads() {
    let xml = "<Property name='Vector' type='App::PropertyVector'><PropertyVector valueX='1' valueY='2' valueZ='3'/></Property>";
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, view) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
    let dom = roxmltree::Document::parse(xml).unwrap();
    let source = Source::new(&ctx, view, &dom).unwrap();
    let mut losses = Vec::new();
    let result: Result<(), CodecError> =
        source.record("discarded-vector", dom.root_element(), || {
            let property = crate::persistence::parse_persisted_property(
                dom.root_element(),
                "Owner",
                0,
                &mut losses,
                Some(&source),
                false,
            )?;
            assert_eq!(property.values()[0].attributes["valueX"], "1");
            assert!(crate::attributes::values(&property).is_some());
            Err(CodecError::malformed("discard the actual vector producer"))
        });
    assert!(result.is_err());
    assert!(losses.is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
