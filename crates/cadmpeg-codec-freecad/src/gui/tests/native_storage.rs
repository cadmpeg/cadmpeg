// SPDX-License-Identifier: Apache-2.0
//! Native GUI storage remains scoped while neutral output remains retained.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const GUI: &str = "<Document SchemaVersion='1' extra='native attribute'><Camera settings=''/><ViewProviderData Count='1'><ViewProvider name='Unknown'><Properties Count='1'><Property name='Label' type='App::PropertyString'><String value='native value'/></Property></Properties></ViewProvider></ViewProviderData></Document>";

#[test]
fn gui_native_graph_copies_are_scoped_and_fields_survive_transfer() {
    let xml = roxmltree::Document::parse(GUI).expect("GUI XML");
    let ir = cadmpeg_ir::CadIr::empty();
    let sources = super::super::GuiSources {
        entries: &std::collections::BTreeMap::new(),
        objects: &[],
        properties: &[],
        payloads: &[],
        element_maps: &[],
        requires_alpha_conversion: false,
    };
    for operation in [
        "FCStd GUI document attribute",
        "FCStd GUI state records",
        "FCStd GUI provider XML",
        "FCStd GUI property XML",
        "FCStd GUI document records",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
        let (graph, plan) = super::super::transfer_schema_one(
            &ctx,
            &ir,
            GUI,
            &xml,
            Some("1"),
            &sources,
            &mut ctx
                .reserve_scoped(0, "test GUI diagnostics")
                .expect("owner"),
        )
        .expect("native copies are not retained output");
        drop(probe);
        assert_eq!(graph.documents[0].attributes["extra"], "native attribute");
        assert_eq!(graph.documents[0].states[0].kind, "Camera");
        assert_eq!(graph.providers[0].name, "Unknown");
        assert_eq!(
            graph.properties[0].values[0].attributes["value"],
            "native value"
        );
        let super::super::Graph {
            _native_storage: native_storage,
            ..
        } = &graph;
        assert!(native_storage.is_some());
        assert_eq!(plan.presentation_documents.len(), 1);
        assert_eq!(plan.view_presentations.len(), 1);
        drop(graph);
        drop(plan);
        let CodecError::ResourceLimit(limit) = ctx
            .reserve_scoped(u64::MAX, "GUI scratch released")
            .expect_err("finite storage cap")
        else {
            panic!("materialized refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(limit.used, 0);
    }
}

#[test]
fn gui_native_graph_requires_materialized_storage() {
    let xml = roxmltree::Document::parse(GUI).expect("GUI XML");
    crate::test_support::materialized_refusal_at("FCStd GUI state records", |ctx| {
        super::super::transfer_schema_one(
            ctx,
            &cadmpeg_ir::CadIr::empty(),
            GUI,
            &xml,
            Some("1"),
            &super::super::GuiSources {
                entries: &std::collections::BTreeMap::new(),
                objects: &[],
                properties: &[],
                payloads: &[],
                element_maps: &[],
                requires_alpha_conversion: false,
            },
            &mut ctx
                .reserve_scoped(0, "test GUI diagnostics")
                .expect("owner"),
        )
        .map(|_| ())
    });
}

#[test]
fn gui_native_graph_keeps_storage_until_records_drop() {
    let live_storage = |keep_graph: bool| {
        let xml = roxmltree::Document::parse(GUI).expect("GUI XML");
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (graph, plan) = super::super::transfer_schema_one(
            &ctx,
            &cadmpeg_ir::CadIr::empty(),
            GUI,
            &xml,
            Some("1"),
            &super::super::GuiSources {
                entries: &std::collections::BTreeMap::new(),
                objects: &[],
                properties: &[],
                payloads: &[],
                element_maps: &[],
                requires_alpha_conversion: false,
            },
            &mut ctx
                .reserve_scoped(0, "test GUI diagnostics")
                .expect("owner"),
        )
        .expect("GUI graph");
        let graph = keep_graph.then_some(graph);
        assert_eq!(plan.presentation_documents.len(), 1);
        let CodecError::ResourceLimit(limit) = ctx
            .reserve_scoped(u64::MAX, "live GUI native storage")
            .expect_err("finite storage cap")
        else {
            panic!("materialized refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        drop(graph);
        drop(plan);
        limit.used
    };
    assert!(live_storage(true) > live_storage(false));
}

#[test]
fn gui_missing_properties_diagnostic_survives_until_fallback_construction() {
    let name = "x".repeat(65_536);
    let text = format!("<Document SchemaVersion='2'><Camera settings=''/><ViewProviderData Count='1'><ViewProvider name='{name}'/></ViewProviderData></Document>");
    assert_recovered_diagnostic_lifetime(&text, &format!("ViewProvider {name} has no Properties"));
}

#[test]
fn gui_link_validation_diagnostic_survives_until_fallback_construction() {
    let text = "<Document SchemaVersion='2'><Camera settings=''/><ViewProviderData Count='1'><ViewProvider name='Unknown'><Properties Count='1'><Property name='References' type='App::PropertyLinkList'><LinkList count='2'><Link value='Target'/><Link/></LinkList></Property></Properties></ViewProvider></ViewProviderData></Document>";
    assert_recovered_diagnostic_lifetime(text, "Link element has no value attribute");
}

fn assert_recovered_diagnostic_lifetime(text: &str, expected_diagnostic: &str) {
    let xml = roxmltree::Document::parse(text).expect("GUI XML");
    let entries = std::collections::BTreeMap::new();
    let sources = super::super::GuiSources {
        entries: &entries,
        objects: &[],
        properties: &[],
        payloads: &[],
        element_maps: &[],
        requires_alpha_conversion: false,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut diagnostic_storage = ctx
        .reserve_scoped(0, "GUI recovered diagnostic")
        .expect("owner");
    let Err(error) = super::super::transfer_schema_one(
        &ctx,
        &cadmpeg_ir::CadIr::empty(),
        text,
        &xml,
        Some("2"),
        &sources,
        &mut diagnostic_storage,
    ) else {
        panic!("missing Properties");
    };
    assert!(matches!(&error, CodecError::Malformed(message)
        if message == expected_diagnostic));
    let probe = RefusalProbe::arm(
        ResourceDimension::MaterializedBytes,
        "FCStd GUI schema loss text",
        None,
    );
    // Observe the production loss constructor while it reads the diagnostic.
    // The native graph has dropped; only diagnostic storage remains live.
    let CodecError::Malformed(message) = &error else {
        panic!("malformed diagnostic");
    };
    let diagnostic_len = cadmpeg_core::decode::u64_from_index(message.len());
    let CodecError::ResourceLimit(limit) = ctx
        .with_scoped_storage("fallback observation", || {
            super::super::schema_failure_loss(&ctx, "2", error, diagnostic_storage)
        })
        .expect_err("fallback allocation boundary")
    else {
        panic!("materialized refusal");
    };
    assert_eq!(limit.used, diagnostic_len);
    assert_eq!(limit.operation, "FCStd GUI schema loss text");
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(probe);

    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut ir = cadmpeg_ir::CadIr::empty();
    let graph =
        super::super::transfer(&ctx, &mut ir, text.as_bytes(), &sources).expect("schema fallback");
    assert!(graph.documents.is_empty());
    assert!(graph.providers.is_empty());
    assert!(graph.properties.is_empty());
    assert_eq!(graph.losses.len(), 1);
    assert_eq!(graph.losses[0].message, format!("GuiDocument.xml could not be decoded with the schema-1 vocabulary; declared schema 2 is the probable cause: malformed container: {expected_diagnostic}"));
    drop(graph);
    assert_eq!(ctx.resource_refusal(), None);
    let CodecError::ResourceLimit(released) = ctx
        .reserve_scoped(u64::MAX, "fallback diagnostics released")
        .expect_err("finite materialized allowance")
    else {
        panic!("materialized refusal");
    };
    assert_eq!(released.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(released.used, 0);
    assert_eq!(ctx.resource_refusal(), Some(released));
    crate::test_support::assert_retained_refusal_at(&[], "FCStd GUI schema loss text", |ctx| {
        super::super::transfer(
            ctx,
            &mut cadmpeg_ir::CadIr::empty(),
            text.as_bytes(),
            &sources,
        )
        .map(|_| ())
    });
}
