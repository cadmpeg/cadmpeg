// SPDX-License-Identifier: Apache-2.0
//! Original-source framing without a second parse or semantic read credit.

use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

use super::*;

fn text<'a>(views: &[View<'a>]) -> Vec<&'a [u8]> {
    views.iter().map(|view| view.window()).collect()
}

fn contains(views: &[View<'_>], range: std::ops::Range<usize>) -> bool {
    views
        .iter()
        .any(|view| view.start() < range.end && range.start < view.end())
}

#[test]
fn existing_gui_tree_exports_only_original_markup_from_a_nonzero_source_window() {
    let xml = "<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='1'><Property name='Label' type='App::PropertyString'><String value='a > b'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let mut bytes = b"prefix".to_vec();
    let start = bytes.len();
    bytes.extend_from_slice(xml.as_bytes());
    let end = bytes.len();
    bytes.extend_from_slice(b"suffix");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let source = root.child(start, end).unwrap();
    let tree = ctx
        .parse_xml(
            std::str::from_utf8(source.window()).unwrap(),
            "existing GUI tree",
        )
        .unwrap();
    let framing = ctx
        .record("navigation", source, || frame(&ctx, source, &tree, true))
        .unwrap();
    assert!(framing.supported());
    assert_eq!(
        text(framing.framing()),
        [
            b"<Document".as_slice(),
            b">",
            b"</Document>",
            b"<ViewProviderData",
            b">",
            b"</ViewProviderData>",
            b"<ViewProvider",
            b">",
            b"</ViewProvider>",
            b"<Properties",
            b">",
            b"</Properties>",
            b"<Property",
            b">",
            b"</Property>",
            b"<String",
            b"/>",
        ]
    );
    assert!(framing.insignificant().is_empty());
    for view in framing.framing() {
        assert_eq!(
            view.location(),
            root.child(view.start(), view.end()).unwrap().location()
        );
        assert_eq!(
            view.window().as_ptr(),
            root.child(view.start(), view.end())
                .unwrap()
                .window()
                .as_ptr()
        );
        assert!(start <= view.start() && view.end() <= end);
    }
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert!(captures[0].reads().is_empty());
}

#[test]
fn namespace_aliases_and_unicode_names_keep_their_actual_source_qnames() {
    let bytes = "<p:Document xmlns:p='urn:f' xmlns:q='urn:f'><q:状态 settings='quoted > text'/></p:Document>".as_bytes();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
    let tree = ctx
        .parse_xml(
            std::str::from_utf8(root.window()).unwrap(),
            "namespaced GUI",
        )
        .unwrap();
    let framing = frame(&ctx, root, &tree, true).unwrap();
    assert!(framing.supported());
    assert_eq!(
        text(framing.framing()),
        [
            b"<p:Document".as_slice(),
            b">",
            b"</p:Document>",
            "<q:状态".as_bytes(),
            b"/>"
        ]
    );
    assert!(framing.insignificant().is_empty());
}

#[test]
fn unknown_intervening_elements_do_not_promote_a_nested_known_provider() {
    let bytes = b"<Document><ViewProviderData><Vendor><ViewProvider name='A'/></Vendor></ViewProviderData></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
    let tree = ctx
        .parse_xml(std::str::from_utf8(root.window()).unwrap(), "GUI tree")
        .unwrap();
    let framing = frame(&ctx, root, &tree, true).unwrap();
    assert!(!framing.supported());
    assert_eq!(
        text(framing.framing()),
        [
            b"<Document".as_slice(),
            b">",
            b"</Document>",
            b"<ViewProviderData",
            b">",
            b"</ViewProviderData>"
        ]
    );
}

#[test]
fn only_plain_inter_element_whitespace_is_insignificant() {
    let xml = "<Document>\n <ViewProviderData>\n <ViewProvider name='A'>\n <Properties>\n <Property name='Text' type='App::PropertyString'><String>  </String></Property>\n <Property name='CData' type='App::PropertyString'><String><![CDATA[  ]]></String></Property>\n <Property name='Entity' type='App::PropertyString'><String>&#32;</String></Property>\n <Property name='Mixed' type='App::PropertyString'><String> <Item/> </String></Property>\n </Properties>\n </ViewProvider>\n </ViewProviderData>\n</Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::default()).unwrap();
    let tree = ctx.parse_xml(xml, "existing GUI tree").unwrap();
    let framing = frame(&ctx, root, &tree, true).unwrap();
    assert!(framing.supported());
    assert!(!framing.insignificant().is_empty());
    for view in framing.insignificant() {
        assert!(view.window().iter().all(|byte| xml_space(*byte)));
    }
    for protected in ["  </String>", "<![CDATA[  ]]>", "&#32;", " <Item/> "] {
        let start = xml.find(protected).unwrap();
        assert!(
            !contains(framing.insignificant(), start..start + protected.len()),
            "{protected}"
        );
    }
}

#[test]
fn comments_processing_instructions_and_declarations_are_not_element_framing() {
    let xml = "<?xml version='1.0'?><!-- before --><?outside data?><Document><!-- inside --><?inside data?><Camera settings='source'/></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::default()).unwrap();
    let tree = ctx.parse_xml(xml, "existing GUI tree").unwrap();
    let framing = frame(&ctx, root, &tree, true).unwrap();
    assert!(framing.supported());
    assert_eq!(
        text(framing.framing()),
        [
            b"<Document".as_slice(),
            b">",
            b"</Document>",
            b"<Camera",
            b"/>"
        ]
    );
    assert!(framing.insignificant().is_empty());
}

#[test]
fn document_vocabulary_uses_the_same_original_tree_navigation() {
    for (objects, object, data) in [
        ("Objects", "Object", "ObjectData"),
        ("Features", "Feature", "FeatureData"),
    ] {
        let xml = format!("<Document SchemaVersion='4'><{objects}><{object} name='A' type='App::Feature'/></{objects}><{data}><{object} name='A'><Properties><Property name='Label' type='App::PropertyString'><String value='A'/></Property></Properties></{object}></{data}></Document>");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::default())
                .unwrap();
        let tree = ctx.parse_xml(&xml, "existing Document tree").unwrap();
        let framing = frame(&ctx, root, &tree, false).unwrap();
        assert!(framing.supported());
        assert_eq!(framing.framing().len(), 22);
        assert!(framing.insignificant().is_empty());
        assert!(framing
            .framing()
            .iter()
            .any(|view| view.window() == b"<Property"));
    }
}

#[test]
fn foreign_detached_and_mismatched_tree_sources_cannot_claim_framing() {
    let bytes = b"<Document/>";
    let arena = DecodeArena::new();
    let other_arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
    let (other, _) =
        DecodeContext::from_root_bytes(bytes, &other_arena, &DecodePolicy::default()).unwrap();
    let tree = ctx
        .parse_xml(std::str::from_utf8(root.window()).unwrap(), "original tree")
        .unwrap();
    assert!(frame(&other, root, &tree, true).is_err());
    assert!(frame(&ctx, View::over_retained(bytes), &tree, true).is_err());
    let (mismatch, source) =
        DecodeContext::from_root_bytes(b"<Xocument/>", &arena, &DecodePolicy::default()).unwrap();
    assert!(frame(&mismatch, source, &tree, true).is_err());
}

#[test]
fn xml_framing_uses_the_same_retained_budget_and_preserves_the_session_fuse() {
    let bytes = b"<Document><Camera/></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
    let tree = ctx
        .parse_xml(std::str::from_utf8(root.window()).unwrap(), "original tree")
        .unwrap();
    ctx.retain_bytes(ctx.retained_bytes_available(), "exhaust framing storage")
        .unwrap();
    assert!(matches!(
        frame(&ctx, root, &tree, true),
        Err(CodecError::ResourceLimit(_))
    ));
    assert!(ctx.finish_session().is_err());
}

#[test]
fn equal_bytes_in_a_different_dom_buffer_do_not_claim_the_original_view() {
    let bytes = b"<Document><Camera settings='same'/></Document>";
    let duplicate = bytes.to_vec();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
    let different_tree = ctx
        .parse_xml(
            std::str::from_utf8(&duplicate).unwrap(),
            "separate equal DOM",
        )
        .unwrap();
    assert_eq!(different_tree.input_text().as_bytes(), root.window());
    assert_ne!(different_tree.input_text().as_ptr(), root.window().as_ptr());
    assert!(frame(&ctx, root, &different_tree, true).is_err());
    let original_tree = ctx
        .parse_xml(
            std::str::from_utf8(root.window()).unwrap(),
            "original source DOM",
        )
        .unwrap();
    assert!(frame(&ctx, root, &original_tree, true).unwrap().supported());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
