// SPDX-License-Identifier: Apache-2.0
//! Kept integer/Boolean property flags use actual returned source values.

use super::*;

#[test]
fn valid_flag_changes_and_entities_feed_kept_fields_with_exact_nonzero_source_reads() {
    for (attr, ro, hide, flags, read_only, hidden) in [
        ("-8", "True", "0", -8, true, false),
        ("&#49;2", "0", "TRUE", 12, false, true),
        ("9223372036854775807", "FALSE", "1", i64::MAX, false, true),
    ] {
        let xml = document(&format!("<Property name='Flags' type='App::PropertyFloat' group='Custom' attr='{attr}' ro='{ro}' hide='{hide}' Vendor='opaque-extra'><Float value='7.5'/></Property>"), 1);
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
            panic!("kept flags")
        };
        assert_eq!(metadata.group, "Custom");
        assert_eq!(metadata.attributes, Some(flags));
        assert_eq!(metadata.read_only, Some(read_only));
        assert_eq!(metadata.hidden, Some(hidden));
        let captures = property_captures(&ctx, &graph);
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].id(), property.id);
        assert_eq!(
            captures[0].reads(),
            &std::collections::BTreeMap::from([(
                "file".into(),
                vec![
                    attribute_range(&bytes, "name", "Flags"),
                    attribute_range(&bytes, "type", "App::PropertyFloat"),
                    attribute_range(&bytes, "group", "Custom"),
                    attribute_range(&bytes, "attr", attr),
                    attribute_range(&bytes, "ro", ro),
                    attribute_range(&bytes, "hide", hide),
                    attribute_range(&bytes, "value", "7.5")
                ]
            )])
        );
    }
}

#[test]
fn invalid_overflow_and_namespace_flag_values_preserve_legacy_fields_without_read_credit() {
    for (attrs, flags, read_only, hidden) in [
        (
            "attr='9223372036854775808' ro='maybe' hide='tRuE'",
            None,
            Some(false),
            Some(false),
        ),
        (
            "attr='bad' ro='' hide='TrUe'",
            None,
            Some(false),
            Some(false),
        ),
        (
            "xmlns:v='urn:vendor' v:attr='7' v:ro='1' v:hide='0'",
            None,
            None,
            None,
        ),
    ] {
        let xml = document(&format!("<Property name='Flags' type='App::PropertyFloat' group='Custom' {attrs}><Float value='7.5'/></Property>"), 1);
        let arena = DecodeArena::new();
        let (ctx, view) =
            DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service())
                .unwrap();
        let dom = roxmltree::Document::parse(&xml).unwrap();
        let pure = crate::persistence::parse_document(&dom, Vocabulary::Objects).unwrap();
        let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
        assert_eq!(graph.properties, pure.properties);
        assert!(matches!(
            &graph.properties[0].body,
            Some(crate::native::PropertyBody::Persisted { .. })
        ));
        let Some(metadata) = &graph.properties[0].dynamic else {
            panic!("kept fallback flags")
        };
        assert_eq!(metadata.attributes, flags);
        assert_eq!(metadata.read_only, read_only);
        assert_eq!(metadata.hidden, hidden);
        let captures = property_captures(&ctx, &graph);
        assert_eq!(captures.len(), 1);
        assert_eq!(
            captures[0].reads(),
            &std::collections::BTreeMap::from([(
                "file".into(),
                vec![
                    attribute_range(&xml, "name", "Flags"),
                    attribute_range(&xml, "type", "App::PropertyFloat"),
                    attribute_range(&xml, "group", "Custom"),
                    attribute_range(&xml, "value", "7.5")
                ]
            )])
        );
    }
}

#[test]
fn absent_metadata_or_unreadable_body_cannot_credit_otherwise_valid_flag_bytes() {
    for (group, value, readable) in [("", "7.5", true), ("group='Custom'", "invalid", false)] {
        let xml = document(&format!("<Property name='Flags' type='App::PropertyFloat' {group} attr='7' ro='1' hide='0'><Float value='{value}'/></Property>"), 1);
        let arena = DecodeArena::new();
        let (ctx, view) =
            DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service())
                .unwrap();
        let dom = roxmltree::Document::parse(&xml).unwrap();
        let pure = crate::persistence::parse_document(&dom, Vocabulary::Objects).unwrap();
        let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
        assert_eq!(graph.properties, pure.properties);
        if readable {
            assert!(matches!(
                &graph.properties[0].body,
                Some(crate::native::PropertyBody::Persisted { .. })
            ));
            assert!(graph.properties[0].dynamic.is_none());
        } else {
            assert!(matches!(
                &graph.properties[0].body,
                Some(crate::native::PropertyBody::Unreadable(_))
            ));
        }
        let captures = property_captures(&ctx, &graph);
        assert_eq!(captures.len(), 1);
        let mut expected = identity_ranges(&xml, "Flags", "App::PropertyFloat");
        if readable {
            expected.push(attribute_range(&xml, "value", value));
        }
        assert_eq!(
            captures[0].reads(),
            &std::collections::BTreeMap::from([("file".into(), expected)])
        );
    }
}

#[test]
fn discarded_actual_flag_producer_rolls_back_numeric_boolean_and_other_field_reads() {
    let xml = "<Property name='Flags' type='App::PropertyFloat' group='Custom' attr='12' ro='1' hide='0'><Float value='7.5'/></Property>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let dom = roxmltree::Document::parse(xml).unwrap();
    let source = Source::new(&ctx, view, &dom).unwrap();
    let mut losses = Vec::new();
    let result: Result<(), CodecError> =
        source.record("discarded-flags", dom.root_element(), || {
            let property = crate::persistence::parse_persisted_property(
                dom.root_element(),
                "Owner",
                0,
                &mut losses,
                Some(&source),
                false,
            )?;
            assert!(matches!(
                &property.body,
                Some(crate::native::PropertyBody::Persisted { .. })
            ));
            let Some(metadata) = &property.dynamic else {
                panic!("actual kept flags")
            };
            assert_eq!(metadata.attributes, Some(12));
            assert_eq!(metadata.read_only, Some(true));
            assert_eq!(metadata.hidden, Some(false));
            Err(CodecError::malformed("discard actual property flags"))
        });
    assert!(result.is_err());
    assert!(losses.is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
