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
        let (graph, plan) =
            super::super::transfer_schema_one(&ctx, &ir, GUI, &xml, Some("1"), Some(1), &sources)
                .expect("native copies are not retained output");
        drop(probe);
        assert_eq!(graph.documents[0].attributes["extra"], "native attribute");
        assert_eq!(graph.documents[0].states[0].kind, "Camera");
        assert_eq!(graph.providers[0].name, "Unknown");
        assert_eq!(
            graph.properties[0].values[0].attributes["value"],
            "native value"
        );
        assert!(graph._native_storage.is_some());
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
            Some(1),
            &super::super::GuiSources {
                entries: &std::collections::BTreeMap::new(),
                objects: &[],
                properties: &[],
                payloads: &[],
                element_maps: &[],
                requires_alpha_conversion: false,
            },
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
            Some(1),
            &super::super::GuiSources {
                entries: &std::collections::BTreeMap::new(),
                objects: &[],
                properties: &[],
                payloads: &[],
                element_maps: &[],
                requires_alpha_conversion: false,
            },
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
