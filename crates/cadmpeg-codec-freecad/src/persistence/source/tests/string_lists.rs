// SPDX-License-Identifier: Apache-2.0
//! Ordered kept list strings come from their actual value attributes.

use super::*;
use cadmpeg_ir::attributes::AttributeValue;

#[test]
fn changed_ordered_list_strings_flow_to_kept_and_neutral_values_with_exact_nonzero_reads() {
    for (raw_first, first, raw_second, second) in [
        ("First Ω &amp; β", "First Ω & β", "tail", "tail"),
        (
            "changed &#x3b4;",
            "changed δ",
            "other &lt;tail&gt;",
            "other <tail>",
        ),
    ] {
        let xml = document(&format!("<Property name='Words' type='App::PropertyStringList' Vendor='opaque-extra'><StringList count='3'><String value='{raw_first}'/><String value='{raw_second}'/><String value=''/></StringList></Property>"), 1);
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
        assert_eq!(property.values().len(), 4);
        assert_eq!(property.values()[0].tag, "StringList");
        assert_eq!(property.values()[0].attributes["count"], "3");
        assert!(property.values()[0].parent.is_none());
        for (index, expected) in [first, second, ""].into_iter().enumerate() {
            let value = &property.values()[index + 1];
            assert_eq!(value.parent, Some(0));
            assert_eq!(value.order, index + 1);
            assert_eq!(value.tag, "String");
            assert_eq!(value.attributes["value"], expected);
        }
        assert_eq!(
            crate::attributes::values(property).unwrap(),
            vec![
                AttributeValue::String(first.into()),
                AttributeValue::String(second.into()),
                AttributeValue::String(String::new()),
            ]
        );
        let extent = ctx.capture_part_extent(view).unwrap();
        let captures = property_captures(&ctx, &graph);
        let [capture] = captures.as_slice() else {
            panic!("one property: {captures:?}")
        };
        assert_eq!(capture.id(), property.id);
        assert_eq!(capture.stream(), extent.stream());
        let start = bytes.find("<Property name='Words'").unwrap();
        let end = bytes.find("</Property>").unwrap() + "</Property>".len();
        assert_eq!(capture.offset(), u64_from_index(start));
        assert_eq!(capture.byte_len(), u64_from_index(end - start));
        let mut expected = identity_ranges(&bytes, "Words", "App::PropertyStringList");
        expected.extend(attribute_reads(&bytes, "count", "3"));
        expected.extend(attribute_reads(&bytes, "value", raw_first));
        expected.extend(attribute_reads(&bytes, "value", raw_second));
        expected.extend(attribute_reads(&bytes, "value", ""));
        assert_eq!(
            capture.reads(),
            &normalized_map(&std::collections::BTreeMap::from([(
                "file".into(),
                expected
            )]))
        );
    }
}

#[test]
fn invalid_counts_nested_case_namespace_and_vendor_lists_keep_their_original_values_uncredited() {
    for (case, (type_name, body)) in [
        ("App::PropertyStringList", "<StringList count='2'><String value='kept'/></StringList>"),
        ("App::PropertyStringList", "<StringList count='bad'><String value='kept'/></StringList>"),
        ("App::PropertyStringList", "<StringList count='1'><Wrapper><String value='kept'/></Wrapper></StringList>"),
        ("App::PropertyStringList", "<StringList count='1'><String value='kept'><Vendor/></String></StringList>"),
        ("App::PropertyStringList", "<StringList count='1'><String Value='kept'/></StringList>"),
        ("App::PropertyStringList", "<StringList xmlns:v='urn:vendor' count='1'><String value='kept' v:value='overwritten'/></StringList>"),
        ("App::PropertyStringList", "<StringList count='1'><String value='kept' Vendor='opaque'/></StringList>"),
        ("App::PropertyStringList", "<StringList count='1'>unmodeled text<String value='kept'/></StringList>"),
        ("App::PropertyStringList", "<v:StringList xmlns:v='urn:vendor' count='1'><String value='kept'/></v:StringList>"),
        ("App::PropertyStringList", "<StringList count='1'><String value='kept'/></StringList><StringList count='0'/>"),
        ("Vendor::PropertyStringList", "<StringList count='1'><String value='kept'/></StringList>"),
    ].into_iter().enumerate() {
        let xml = document(&format!("<Property name='Uncredited' type='{type_name}'>{body}</Property>"), 1);
        let arena = DecodeArena::new();
        let (ctx, view) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
        let dom = roxmltree::Document::parse(&xml).unwrap();
        let pure = crate::persistence::parse_document(&dom, Vocabulary::Objects).unwrap();
        let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
        assert_eq!(graph.properties, pure.properties, "{body}");
        // #215 retains each readable leaf and gives up only the damaged element.
        let property = &graph.properties[0];
        let (held, failures) = match case {
            0 | 1 | 7 => (0, 1),
            2 | 4 => (1, 1),
            3 | 9 => (2, 1),
            5 | 6 | 8 => (2, 0),
            10 => (0, 0),
            _ => unreachable!(),
        };
        assert_eq!(property.values().len(), held, "{body}");
        assert_eq!(property.body.is_some(), !matches!(case, 0 | 1 | 7));
        assert_eq!(graph.losses.iter().filter(|loss| loss.code.local_code() == "record.unreadable").count(), failures, "{body}");
        let mut expected = identity_ranges(&xml, "Uncredited", type_name);
        if held != 0 { expected.extend(attribute_reads(&xml, "count", "1")); }
        if held == 2 {
            let (key, raw) = if case == 5 { ("v:value", "overwritten") } else { ("value", "kept") };
            expected.extend(attribute_reads(&xml, key, raw));
            assert_eq!(property.values()[1].attributes["value"], raw);
        }
        if case == 6 { expected.extend(attribute_reads(&xml, "Vendor", "opaque")); }
        let captures = property_captures(&ctx, &graph);
        let [capture] = captures.as_slice() else { panic!("one property: {captures:?}") };
        assert_eq!(capture.reads(), &normalized_map(&std::collections::BTreeMap::from([("file".into(), expected)])), "{body}");
    }
}

#[test]
fn discarding_a_real_string_list_producer_rolls_back_each_child_value_and_property_identity() {
    let xml = "<Property name='Words' type='App::PropertyStringList'><StringList count='2'><String value='first'/><String value='second'/></StringList></Property>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let mut losses = Vec::new();
    let result: Result<(), CodecError> = ctx.record("discarded-string-list", view, || {
        let property = parse_property(&ctx, view, &mut losses)?;
        assert_eq!(property.values()[1].attributes["value"], "first");
        assert_eq!(property.values()[2].attributes["value"], "second");
        assert_eq!(
            crate::attributes::values(&property).unwrap(),
            vec![
                AttributeValue::String("first".into()),
                AttributeValue::String("second".into())
            ]
        );
        Err(CodecError::malformed(
            "discard the actual ordered string-list producer",
        ))
    });
    assert!(result.is_err());
    assert!(losses.is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
