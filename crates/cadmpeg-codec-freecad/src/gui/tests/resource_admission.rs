// SPDX-License-Identifier: Apache-2.0
//! GUI XML admission and retained-copy tests.

use crate::test_support::test_archive::archive_entries;
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn y4_2_decode_refuses_unadmitted_gui_text_copy() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><Camera settings=""/></Document>"#;
    let bytes = archive_entries(&[("Document.xml", document), ("GuiDocument.xml", gui)]);
    FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("service profile admits the GUI state");

    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = (document.len() + gui.len()) as u64;
    let error = FcstdCodec
        .decode(&mut Cursor::new(bytes), &options)
        .expect_err("GUI text copy must be admitted");
    assert!(
        matches!(
            &error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && limit.operation.starts_with("FCStd GUI ")
        ),
        "{error:?}"
    );
}

#[test]
fn y4_2_gui_xml_tree_is_admitted_before_allocation() {
    let xml = br#"<Document SchemaVersion="1"><Camera settings=""/></Document>"#;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(xml, &arena, &policy)
        .expect("service GUI context");
    super::super::transfer(
        &ctx,
        &mut cadmpeg_ir::CadIr::empty(),
        xml,
        &std::collections::BTreeMap::new(),
        &[],
        &[],
        &[],
        &[],
        false,
    )
    .expect("service profile admits the GUI document");

    let mut limited = cadmpeg_core::decode::DecodePolicy::service();
    limited.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(xml, &arena, &limited)
        .expect("limited GUI context");
    let error = super::super::transfer(
        &ctx,
        &mut cadmpeg_ir::CadIr::empty(),
        xml,
        &std::collections::BTreeMap::new(),
        &[],
        &[],
        &[],
        &[],
        false,
    )
    .err()
    .expect("GUI XML node count must be charged before parsing");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "FCStd GUI XML node tree"
    ));
}

fn assert_gui_state_service(xml: &str) {
    let document = roxmltree::Document::parse(xml).expect("GUI state XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
            .expect("service GUI state context");
    super::super::gui_state(&ctx, xml, 0, document.root_element())
        .expect("service profile admits the GUI state");
}

fn assert_gui_provider_service(xml: &str) {
    let document = roxmltree::Document::parse(xml).expect("GUI provider XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
            .expect("service GUI provider context");
    let mut providers = Vec::new();
    let mut properties = Vec::new();
    super::super::append_native_provider(
        &ctx,
        xml,
        document.root_element(),
        0,
        None,
        &mut providers,
        &mut properties,
    )
    .expect("service profile admits the GUI provider");
}

#[test]
fn y4_2_gui_state_xml_copy_refuses_at_the_retained_byte_limit() {
    let xml = "<Camera/>";
    assert_gui_state_service(xml);
    let node = roxmltree::Document::parse(xml).expect("GUI state XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = "Camera".len() as u64;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
            .expect("GUI state context");
    let error = super::super::gui_state(&ctx, xml, 0, node.root_element())
        .expect_err("GUI state raw XML copy must be charged");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "FCStd GUI state XML"
    ));
}

#[test]
fn y4_2_gui_state_values_are_admitted_before_allocation() {
    let xml = "<Camera><Settings/></Camera>";
    assert_gui_state_service(xml);
    let document = roxmltree::Document::parse(xml).expect("GUI state XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
            .expect("GUI state context");
    let error = super::super::gui_state(&ctx, xml, 0, document.root_element())
        .expect_err("GUI state values must be charged before allocation");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "FCStd GUI state values"
    ));
}

#[test]
fn y4_2_gui_provider_xml_copy_refuses_at_the_retained_byte_limit() {
    let xml = "<ViewProvider name=\"P\"><Properties Count=\"0\"/></ViewProvider>";
    assert_gui_provider_service(xml);
    let document = roxmltree::Document::parse(xml).expect("GUI provider XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
            .expect("GUI provider context");
    let mut providers = Vec::new();
    let mut properties = Vec::new();
    let error = super::super::append_native_provider(
        &ctx,
        xml,
        document.root_element(),
        0,
        None,
        &mut providers,
        &mut properties,
    )
    .expect_err("GUI provider raw XML copy must be charged");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "FCStd GUI provider XML"
    ));
}

#[test]
fn y4_2_gui_property_xml_copy_refuses_at_the_retained_byte_limit() {
    let xml = "<ViewProvider name=\"P\"><Properties Count=\"1\"><Property name=\"P\" type=\"T\"/></Properties></ViewProvider>";
    assert_gui_provider_service(xml);
    let document = roxmltree::Document::parse(xml).expect("GUI provider XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 1 + xml.len() as u64 + 1 + 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
            .expect("GUI property context");
    let mut providers = Vec::new();
    let mut properties = Vec::new();
    let error = super::super::append_native_provider(
        &ctx,
        xml,
        document.root_element(),
        0,
        None,
        &mut providers,
        &mut properties,
    )
    .expect_err("GUI property raw XML copy must be charged");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "FCStd GUI property XML"
    ));
}

#[test]
fn y4_2_gui_property_values_are_admitted_before_allocation() {
    let xml = "<ViewProvider name=\"P\"><Properties Count=\"1\"><Property name=\"P\" type=\"T\"><X/></Property></Properties></ViewProvider>";
    assert_gui_provider_service(xml);
    let document = roxmltree::Document::parse(xml).expect("GUI provider XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
            .expect("GUI property context");
    let mut providers = Vec::new();
    let mut properties = Vec::new();
    let error = super::super::append_native_provider(
        &ctx,
        xml,
        document.root_element(),
        0,
        None,
        &mut providers,
        &mut properties,
    )
    .expect_err("GUI property values must be charged before allocation");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "FCStd GUI property values"
    ));
}
