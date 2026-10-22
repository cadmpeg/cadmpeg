// SPDX-License-Identifier: Apache-2.0
//! The registered vector subtypes keep values from their actual source spans.

use super::*;
use cadmpeg_ir::attributes::AttributeValue;

const TYPES: [&str; 3] = [
    "App::PropertyVectorDistance",
    "App::PropertyPosition",
    "App::PropertyDirection",
];

#[test]
fn registered_vector_family_components_feed_native_and_neutral_values_from_exact_source_bytes() {
    for type_name in TYPES {
        for (raw_x, x) in [("1.25", 1.25), ("&#45;6.5", -6.5)] {
            let xml = document(&format!("<Property name='ExtraVector' type='{type_name}'><PropertyVector valueX='{raw_x}' valueY='&#50;' valueZ='3' Vendor='opaque-extra'/></Property>"), 1);
            let bytes = format!("prefix{xml}suffix");
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(bytes.as_bytes(), &arena, &DecodePolicy::service())
                    .unwrap();
            let view = root
                .child("prefix".len(), "prefix".len() + xml.len())
                .unwrap();
            let dom =
                roxmltree::Document::parse(std::str::from_utf8(view.window()).unwrap()).unwrap();
            let pure = crate::persistence::parse_document(&dom, Vocabulary::Objects).unwrap();
            let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
            assert_eq!(graph.properties, pure.properties);
            let property = &graph.properties[0];
            assert_eq!(property.type_name, type_name);
            assert_eq!(property.values().len(), 1);
            let value = &property.values()[0];
            assert_eq!(value.attributes["valueX"], x.to_string());
            assert_eq!(value.attributes["valueY"], "2");
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
                [x, 2.0, 3.0]
            );
            let extent = ctx.capture_part_extent(view).unwrap();
            let captures = property_captures(&ctx, &graph);
            let [capture] = captures.as_slice() else {
                panic!("one property: {captures:?}")
            };
            assert_eq!(capture.id(), property.id);
            assert_eq!(capture.stream(), extent.stream());
            let property_start = bytes.find("<Property name='ExtraVector'").unwrap();
            let property_end = bytes.find("</Property>").unwrap() + "</Property>".len();
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
                        attribute_range(&bytes, "type", type_name),
                        attribute_range(&bytes, "valueX", raw_x),
                        attribute_range(&bytes, "valueY", "&#50;"),
                        attribute_range(&bytes, "valueZ", "3"),
                    ]
                )])
            );
        }
    }
}

#[test]
fn incomplete_nonfinite_nested_namespace_and_vendor_vector_families_keep_no_component_credit() {
    for type_name in TYPES.into_iter().chain(["Vendor::PropertyPosition"]) {
        let carriers: &[&str] = if type_name.starts_with("Vendor::") {
            &["<PropertyVector valueX='1' valueY='2' valueZ='3'/>"]
        } else {
            &[
                "<PropertyVector valueX='1' valueY='2'/>",
                "<PropertyVector valueX='bad' valueY='2' valueZ='3'/>",
                "<PropertyVector valueX='NaN' valueY='2' valueZ='3'/>",
                "<PropertyVector valueX='inf' valueY='2' valueZ='3'/>",
                "<PropertyVector valueX='1' valueY='2' valueZ='3'><Vendor value='opaque'/></PropertyVector>",
                "<PropertyVector valueX='1' valueY='2' valueZ='3'/><PropertyVector valueX='4' valueY='5' valueZ='6'/>",
                "<v:PropertyVector xmlns:v='urn:vendor' valueX='1' valueY='2' valueZ='3'/>",
                "<PropertyVector xmlns:v='urn:vendor' valueX='1' v:valueX='overwritten' valueY='2' valueZ='3'/>",
            ]
        };
        for carrier in carriers {
            let xml = document(
                &format!("<Property name='Uncredited' type='{type_name}'>{carrier}</Property>"),
                1,
            );
            let arena = DecodeArena::new();
            let (ctx, view) =
                DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service())
                    .unwrap();
            let dom = roxmltree::Document::parse(&xml).unwrap();
            let pure = crate::persistence::parse_document(&dom, Vocabulary::Objects).unwrap();
            let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
            assert_eq!(graph.properties, pure.properties, "{type_name}: {carrier}");
            assert!(!graph.properties[0].values().is_empty());
            let captures = property_captures(&ctx, &graph);
            let [capture] = captures.as_slice() else {
                panic!("one property: {captures:?}")
            };
            assert_eq!(
                capture.reads(),
                &std::collections::BTreeMap::from([(
                    "file".into(),
                    identity_ranges(&xml, "Uncredited", type_name)
                )]),
                "{type_name}: {carrier}"
            );
        }
    }
}

#[test]
fn discarding_each_actual_vector_family_producer_rolls_back_its_identity_and_component_reads() {
    for type_name in TYPES {
        let xml = format!("<Property name='Vector' type='{type_name}'><PropertyVector valueX='1' valueY='2' valueZ='3'/></Property>");
        let arena = DecodeArena::new();
        let (ctx, view) =
            DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service())
                .unwrap();
        let dom = roxmltree::Document::parse(&xml).unwrap();
        let source = Source::new(&ctx, view, &dom).unwrap();
        let mut losses = Vec::new();
        let result: Result<(), CodecError> =
            source.record("discarded-vector-family", dom.root_element(), || {
                let property = crate::persistence::parse_persisted_property(
                    dom.root_element(),
                    "Owner",
                    0,
                    &mut losses,
                    Some(&source),
                    false,
                )?;
                assert_eq!(property.type_name, type_name);
                assert_eq!(property.values()[0].attributes["valueX"], "1");
                assert!(crate::attributes::values(&property).is_some());
                Err(CodecError::malformed(
                    "discard the actual vector-family producer",
                ))
            });
        assert!(result.is_err());
        assert!(losses.is_empty());
        assert!(ctx.take_record_captures().unwrap().is_empty());
    }
}
