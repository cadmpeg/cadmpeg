// SPDX-License-Identifier: Apache-2.0
//! Actual kept dynamic-property strings consume only their original value spans.

use super::*;

#[test]
fn changed_group_and_documentation_strings_feed_kept_fields_with_exact_nonzero_source_reads() {
    for (raw_group, group, raw_doc, doc) in [
        (
            "Custom&amp;Parameters",
            "Custom&Parameters",
            " Ω &amp; β ",
            " Ω & β ",
        ),
        ("Second Ω", "Second Ω", "different &#x3b4;", "different δ"),
    ] {
        let xml = document(&format!("<Property name='Input' type='App::PropertyFloat' group='{raw_group}' doc='{raw_doc}' attr='12' ro='true' hide='false' Vendor='opaque-extra'><Float value='7.5'/></Property>"), 1);
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
        assert!(matches!(
            &property.body,
            Some(crate::native::PropertyBody::Persisted { .. })
        ));
        let Some(metadata) = &property.dynamic else {
            panic!("actual kept dynamic metadata: {property:?}")
        };
        assert_eq!(metadata.group, group);
        assert_eq!(metadata.documentation.as_deref(), Some(doc));
        assert_eq!(metadata.attributes, Some(12));
        assert_eq!(metadata.read_only, Some(true));
        assert_eq!(metadata.hidden, Some(false));
        assert_eq!(property.values()[0].attributes["value"], "7.5");
        let extent = ctx.capture_part_extent(view).unwrap();
        let captures = property_captures(&ctx, &graph);
        let [capture] = captures.as_slice() else {
            panic!("one kept property: {captures:?}")
        };
        assert_eq!(capture.id(), property.id);
        assert_eq!(capture.stream(), extent.stream());
        assert_eq!(
            capture.reads(),
            &normalized_map(&std::collections::BTreeMap::from([(
                "file".into(),
                [
                    attribute_reads(&bytes, "name", "Input"),
                    attribute_reads(&bytes, "type", "App::PropertyFloat"),
                    attribute_reads(&bytes, "group", raw_group),
                    attribute_reads(&bytes, "doc", raw_doc),
                    attribute_reads(&bytes, "attr", "12"),
                    attribute_reads(&bytes, "ro", "true"),
                    attribute_reads(&bytes, "hide", "false"),
                    attribute_reads(&bytes, "value", "7.5")
                ]
                .concat()
            )]))
        );
    }
}

#[test]
fn absent_group_namespace_lookalikes_and_unreadable_bodies_keep_strings_uncredited() {
    for (attrs, payload, readable) in [
        ("doc='not-kept'", "<Float value='7.5'/>", true),
        (
            "xmlns:v='urn:vendor' v:group='not-kept' doc='not-kept'",
            "<Float value='7.5'/>",
            true,
        ),
        (
            "group='discarded-group' doc='discarded-doc'",
            "<Float value='invalid'/>",
            false,
        ),
    ] {
        let xml = document(
            &format!("<Property name='P' type='App::PropertyFloat' {attrs}>{payload}</Property>"),
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
        let property = &graph.properties[0];
        if readable {
            assert!(matches!(
                &property.body,
                Some(crate::native::PropertyBody::Persisted { .. })
            ));
            assert!(property.dynamic.is_none());
            assert_eq!(property.values()[0].attributes["value"], "7.5");
        } else {
            assert!(matches!(&property.body, None));
            assert!(property.values().is_empty());
            // The value part failed; independent property metadata remains held (#215).
            let metadata = property.dynamic.as_ref().unwrap();
            assert_eq!(metadata.group, "discarded-group");
            assert_eq!(metadata.documentation.as_deref(), Some("discarded-doc"));
        }
        let captures = property_captures(&ctx, &graph);
        assert_eq!(captures.len(), 1);
        let mut expected = identity_ranges(&xml, "P", "App::PropertyFloat");
        if readable {
            expected.extend(attribute_reads(&xml, "value", "7.5"));
        } else {
            expected.extend(attribute_reads(&xml, "group", "discarded-group"));
            expected.extend(attribute_reads(&xml, "doc", "discarded-doc"));
        }
        assert_eq!(
            captures[0].reads(),
            &normalized_map(&std::collections::BTreeMap::from([(
                "file".into(),
                expected
            )]))
        );
    }
}

#[test]
fn abandoned_actual_dynamic_property_producer_rolls_back_group_doc_and_value_reads() {
    let xml = "<Property name='P' type='App::PropertyFloat' group='Custom' doc='kept only on success'><Float value='7.5'/></Property>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let mut losses = Vec::new();
    let result: Result<(), CodecError> = ctx.record("discarded-dynamic-property", view, || {
        let property = parse_property(&ctx, view, &mut losses)?;
        assert!(matches!(
            &property.body,
            Some(crate::native::PropertyBody::Persisted { .. })
        ));
        let Some(metadata) = &property.dynamic else {
            panic!("kept metadata")
        };
        assert_eq!(metadata.group, "Custom");
        assert_eq!(
            metadata.documentation.as_deref(),
            Some("kept only on success")
        );
        Err(CodecError::malformed(
            "discard this actual kept-field producer",
        ))
    });
    assert!(result.is_err());
    assert!(losses.is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

mod flags;
