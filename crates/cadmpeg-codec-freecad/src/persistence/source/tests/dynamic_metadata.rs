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
        let crate::native::PropertyBody::Persisted {
            dynamic: Some(metadata),
            ..
        } = &property.body
        else {
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
            &std::collections::BTreeMap::from([(
                "file".into(),
                vec![
                    attribute_range(&bytes, "name", "Input"),
                    attribute_range(&bytes, "type", "App::PropertyFloat"),
                    attribute_range(&bytes, "group", raw_group),
                    attribute_range(&bytes, "doc", raw_doc),
                    attribute_range(&bytes, "attr", "12"),
                    attribute_range(&bytes, "ro", "true"),
                    attribute_range(&bytes, "hide", "false"),
                    attribute_range(&bytes, "value", "7.5")
                ]
            )])
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
                crate::native::PropertyBody::Persisted { dynamic: None, .. }
            ));
            assert_eq!(property.values()[0].attributes["value"], "7.5");
        } else {
            assert!(matches!(
                &property.body,
                crate::native::PropertyBody::Unreadable(_)
            ));
            assert!(property.values().is_empty());
        }
        let captures = property_captures(&ctx, &graph);
        assert_eq!(captures.len(), 1);
        let mut expected = identity_ranges(&xml, "P", "App::PropertyFloat");
        if readable {
            expected.push(attribute_range(&xml, "value", "7.5"));
        }
        assert_eq!(
            captures[0].reads(),
            &std::collections::BTreeMap::from([("file".into(), expected)])
        );
    }
}

#[test]
fn abandoned_actual_dynamic_property_producer_rolls_back_group_doc_and_value_reads() {
    let xml = "<Property name='P' type='App::PropertyFloat' group='Custom' doc='kept only on success'><Float value='7.5'/></Property>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let dom = roxmltree::Document::parse(xml).unwrap();
    let source = Source::new(&ctx, view, &dom).unwrap();
    let mut losses = Vec::new();
    let result: Result<(), CodecError> =
        source.record("discarded-dynamic-property", dom.root_element(), || {
            let property = crate::persistence::parse_persisted_property(
                dom.root_element(),
                "Owner",
                0,
                &mut losses,
                Some(&source),
            )?;
            let crate::native::PropertyBody::Persisted {
                dynamic: Some(metadata),
                ..
            } = &property.body
            else {
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
