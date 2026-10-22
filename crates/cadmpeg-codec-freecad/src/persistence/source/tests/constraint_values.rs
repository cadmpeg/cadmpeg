// SPDX-License-Identifier: Apache-2.0
//! Kept numeric constraint facts consume their original value spans independently.

use super::*;
use cadmpeg_ir::attributes::AttributeValue;

#[test]
fn each_numeric_constraint_attribute_flows_to_kept_native_facts_with_exact_source_reads() {
    for (type_name, tag, scalar, min, max, step, expected_min) in [
        (
            "App::PropertyIntegerConstraint",
            "Integer",
            "-7",
            "&#50;0",
            "-2",
            "-1",
            "20",
        ),
        (
            "App::PropertyIntegerConstraint",
            "Integer",
            "-7",
            "41",
            "-2",
            "-1",
            "41",
        ),
        (
            "App::PropertyPercent",
            "Integer",
            "-7",
            "&#50;0",
            "-2",
            "-1",
            "20",
        ),
        (
            "App::PropertyFloatConstraint",
            "Float",
            "1.5",
            "3&#x2e;5",
            "-2.25",
            "-0.125",
            "3.5",
        ),
        (
            "App::PropertyFloatConstraint",
            "Float",
            "1.5",
            "-9.25",
            "-2.25",
            "-0.125",
            "-9.25",
        ),
    ] {
        let xml = document(&format!("<Property name='Constrained' type='{type_name}'><{tag} value='{scalar}' min='{min}' max='{max}' step='{step}' Vendor='opaque-extra'/></Property>"), 1);
        let bytes = format!("prefix{xml}suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes.as_bytes(), &arena, &DecodePolicy::service())
                .unwrap();
        let view = root
            .child("prefix".len(), "prefix".len() + xml.len())
            .unwrap();
        let dom = roxmltree::Document::parse(std::str::from_utf8(view.window()).unwrap()).unwrap();
        let pure = crate::persistence::parse_document(&dom, Vocabulary::Objects).unwrap();
        let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
        assert_eq!(graph.properties, pure.properties);
        let property = &graph.properties[0];
        assert_eq!(property.values().len(), 1);
        let fields = &property.values()[0].attributes;
        assert_eq!(fields["value"], scalar);
        assert_eq!(fields["min"], expected_min);
        assert_eq!(fields["max"], max);
        assert_eq!(fields["step"], step);
        assert_eq!(fields["Vendor"], "opaque-extra");
        let projected = crate::attributes::values(property).unwrap();
        assert_eq!(projected.len(), 1);
        match &projected[0] {
            AttributeValue::Integer(value) => assert_eq!(*value, -7),
            AttributeValue::Float(value) => assert_eq!(value.get(), 1.5),
            value => panic!("one original scalar: {value:?}"),
        }
        let extent = ctx.capture_part_extent(view).unwrap();
        let captures = property_captures(&ctx, &graph);
        let [capture] = captures.as_slice() else {
            panic!("one property: {captures:?}")
        };
        assert_eq!(capture.id(), property.id);
        assert_eq!(capture.stream(), extent.stream());
        let property_start = bytes.find("<Property name='Constrained'").unwrap();
        let property_end = bytes.find("</Property>").unwrap() + "</Property>".len();
        assert_eq!(capture.offset(), u64_from_index(property_start));
        assert_eq!(
            capture.byte_len(),
            u64_from_index(property_end - property_start)
        );
        let mut expected = identity_ranges(&bytes, "Constrained", type_name);
        for (key, raw) in [
            ("value", scalar),
            ("min", min),
            ("max", max),
            ("step", step),
        ] {
            expected.push(attribute_range(&bytes, key, raw));
        }
        assert_eq!(
            capture.reads(),
            &std::collections::BTreeMap::from([("file".into(), expected)])
        );
    }
}

#[test]
fn invalid_or_namespace_overwritten_bounds_stay_uncredited_while_independent_numeric_bounds_flow() {
    for (type_name, tag, attrs, credited) in [
        (
            "App::PropertyFloatConstraint",
            "Float",
            "min='NaN' max='inf' step='0'",
            vec![("step", "0")],
        ),
        (
            "App::PropertyIntegerConstraint",
            "Integer",
            "min='9223372036854775808' max='3' step='bad'",
            vec![("max", "3")],
        ),
        (
            "App::PropertyPercent",
            "Integer",
            "min='bad' step='2'",
            vec![("step", "2")],
        ),
        (
            "App::PropertyFloatConstraint",
            "Float",
            "xmlns:v='urn:vendor' min='1' v:min='overwritten' max='4'",
            vec![("max", "4")],
        ),
        (
            "App::PropertyFloat",
            "Float",
            "min='1' max='4' step='2'",
            Vec::new(),
        ),
    ] {
        let xml = document(
            &format!(
                "<Property name='Partial' type='{type_name}'><{tag} value='2' {attrs}/></Property>"
            ),
            1,
        );
        let arena = DecodeArena::new();
        let (ctx, view) =
            DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service())
                .unwrap();
        let dom = roxmltree::Document::parse(&xml).unwrap();
        let pure = crate::persistence::parse_document(&dom, Vocabulary::Objects).unwrap();
        let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
        assert_eq!(graph.properties, pure.properties);
        assert_eq!(graph.properties[0].values()[0].attributes["value"], "2");
        let captures = property_captures(&ctx, &graph);
        let [capture] = captures.as_slice() else {
            panic!("one property: {captures:?}")
        };
        let mut expected = identity_ranges(&xml, "Partial", type_name);
        expected.push(attribute_range(&xml, "value", "2"));
        expected.extend(
            credited
                .into_iter()
                .map(|(key, raw)| attribute_range(&xml, key, raw)),
        );
        assert_eq!(
            capture.reads(),
            &std::collections::BTreeMap::from([("file".into(), expected)]),
            "{type_name} {attrs}"
        );
    }
}

#[test]
fn unsupported_or_unreadable_constraint_carriers_get_no_numeric_bound_read_credit() {
    for (type_name, body, scalar_credit) in [
        (
            "Vendor::PropertyFloatConstraint",
            "<Float value='2' min='1' max='4' step='2'/>",
            false,
        ),
        (
            "App::PropertyFloatConstraint",
            "<Float value='bad' min='1' max='4' step='2'/>",
            false,
        ),
        (
            "App::PropertyIntegerConstraint",
            "<Integer value='2' min='1'><Vendor/></Integer>",
            true,
        ),
        (
            "App::PropertyPercent",
            "<Integer value='2' min='1'/><Integer value='3' max='4'/>",
            false,
        ),
    ] {
        let xml = document(
            &format!("<Property name='Uncredited' type='{type_name}'>{body}</Property>"),
            1,
        );
        let arena = DecodeArena::new();
        let (ctx, view) =
            DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service())
                .unwrap();
        let dom = roxmltree::Document::parse(&xml).unwrap();
        let pure = crate::persistence::parse_document(&dom, Vocabulary::Objects).unwrap();
        let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
        assert_eq!(graph.properties, pure.properties);
        let captures = property_captures(&ctx, &graph);
        let [capture] = captures.as_slice() else {
            panic!("one property: {captures:?}")
        };
        let mut expected = identity_ranges(&xml, "Uncredited", type_name);
        if scalar_credit {
            assert_eq!(graph.properties[0].values()[0].attributes["value"], "2");
            expected.push(attribute_range(&xml, "value", "2"));
        }
        assert_eq!(
            capture.reads(),
            &std::collections::BTreeMap::from([("file".into(), expected)]),
            "{type_name} {body}"
        );
    }
}

#[test]
fn discarded_actual_constraint_producers_rollback_value_bound_and_identity_reads() {
    let xml = "<Property name='Constraint' type='App::PropertyFloatConstraint'><Float value='2' min='1' max='4' step='0.5'/></Property>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let dom = roxmltree::Document::parse(xml).unwrap();
    let source = Source::new(&ctx, view, &dom).unwrap();
    let mut losses = Vec::new();
    let result: Result<(), CodecError> =
        source.record("discarded-constraint", dom.root_element(), || {
            let property = crate::persistence::parse_persisted_property(
                dom.root_element(),
                "Owner",
                0,
                &mut losses,
                Some(&source),
            )?;
            assert_eq!(property.values()[0].attributes["min"], "1");
            assert_eq!(property.values()[0].attributes["max"], "4");
            assert_eq!(property.values()[0].attributes["step"], "0.5");
            assert!(crate::attributes::values(&property).is_some());
            Err(CodecError::malformed(
                "discard the actual constraint producer",
            ))
        });
    assert!(result.is_err());
    assert!(losses.is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
