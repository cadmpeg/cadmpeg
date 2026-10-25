// SPDX-License-Identifier: Apache-2.0
//! Single-parse source handoff and preservation of GUI transfer behavior.

use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

use super::*;

fn graph_value(graph: &Graph) -> serde_json::Value {
    serde_json::to_value((
        &graph.documents,
        &graph.providers,
        &graph.properties,
        &graph.losses,
        graph.untyped,
    ))
    .expect("GUI graph wire values")
}

#[test]
fn original_dom_survives_until_transfer_without_another_parse_or_read_credit() {
    let bytes = b"<Document SchemaVersion='1'><ViewProviderData Count='0'/></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = parse(&ctx, root).unwrap();
    assert_eq!(source.view().location(), root.location());
    assert_eq!(
        source.scanner(&ctx).unwrap().view().window().as_ptr(),
        root.window().as_ptr()
    );
    assert_eq!(source.scanner(&ctx).unwrap().elements().len(), 2);
    let entries = Default::default();
    let sources = GuiSources {
        entries: &entries,
        entry_owners: None,
        entry_interpretations: None,
        gui_entry_interpretations: None,
        declared_entries: None,
        declared_objects: None,
        admitted_objects: None,
        entry_parents: None,
        objects: &[],
        object_sources: None,
        properties: &[],
        payloads: &[],
        element_maps: &[],
        topology_occurrences: &[],
        requires_alpha_conversion: false,
    };
    let mut ir = CadIr::empty();
    let mut output =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let graph = source
        .transfer(&ctx, &mut ir, &sources, &mut output)
        .unwrap();
    let levels = output
        .fidelity()
        .parts()
        .values()
        .map(|part| part.level().as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        levels,
        std::collections::BTreeSet::from([
            "freecad.gui_document_xml",
            "freecad.gui_document",
            "freecad.view_provider_data"
        ])
    );
    assert_eq!(output.fidelity().parts().len(), 3);
    assert!(!graph.untyped);
    assert_eq!(graph.documents.len(), 1);
    assert_eq!(graph.documents[0].schema_version.as_deref(), Some("1"));
    assert!(graph.losses.is_empty());
    schema_reads::assert_capture(&ctx, root, "SchemaVersion", "1");
}

#[test]
fn same_dom_transfer_preserves_native_graph_model_and_existing_source_only_errors() {
    for bytes in [
        b"<Document SchemaVersion='1'><ViewProviderData Count='0'/></Document>".as_slice(),
        b"<Document SchemaVersion='future'><ViewProviderData Count='0'/></Document>",
        b"<Document><ViewProviderData></Document>",
        b"\xff",
    ] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
        let source = parse(&ctx, root).unwrap();
        let legacy_arena = DecodeArena::new();
        let (legacy_ctx, legacy_root) =
            DecodeContext::from_root_bytes(bytes, &legacy_arena, &DecodePolicy::service()).unwrap();
        let entries = Default::default();
        let sources = GuiSources {
            entries: &entries,
            entry_owners: None,
            entry_interpretations: None,
            gui_entry_interpretations: None,
            declared_entries: None,
            declared_objects: None,
            admitted_objects: None,
            entry_parents: None,
            objects: &[],
            object_sources: None,
            properties: &[],
            payloads: &[],
            element_maps: &[],
            topology_occurrences: &[],
            requires_alpha_conversion: false,
        };
        let mut ir = CadIr::empty();
        let mut original_ir = CadIr::empty();
        let graph = source
            .transfer(
                &ctx,
                &mut ir,
                &sources,
                &mut cadmpeg_ir::source_fidelity::parts::PartOutput::new(
                    cadmpeg_ir::CodecFormat::Fcstd,
                ),
            )
            .unwrap();
        let legacy_entries =
            std::collections::BTreeMap::from([("GuiDocument.xml".to_owned(), legacy_root)]);
        let legacy_sources = GuiSources {
            entries: &legacy_entries,
            entry_owners: None,
            entry_interpretations: None,
            gui_entry_interpretations: None,
            declared_entries: None,
            declared_objects: None,
            admitted_objects: None,
            entry_parents: None,
            objects: &[],
            object_sources: None,
            properties: &[],
            payloads: &[],
            element_maps: &[],
            topology_occurrences: &[],
            requires_alpha_conversion: false,
        };
        let original =
            super::super::transfer(&legacy_ctx, &mut original_ir, bytes, &legacy_sources).unwrap();
        assert_eq!(graph_value(&graph), graph_value(&original));
        assert_eq!(ir.model, original_ir.model);
        assert_eq!(graph.provider_sources, original.provider_sources);
        match graph
            .documents
            .first()
            .and_then(|document| document.schema_version.as_deref())
        {
            Some(value) => schema_reads::assert_capture(&ctx, root, "SchemaVersion", value),
            None => {
                let captures = ctx.take_record_captures().unwrap();
                if graph.documents.is_empty() {
                    assert!(captures.is_empty());
                } else {
                    assert_eq!(captures.len(), 1);
                    assert_eq!(captures[0].id(), "fcstd:gui:document#0");
                    assert!(captures[0].reads().is_empty());
                }
            }
        }
    }
}

#[test]
fn foreign_source_is_refused_before_parsing() {
    let bytes = b"<Document/>";
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let foreign_arena = DecodeArena::new();
    let (_foreign_ctx, foreign) =
        DecodeContext::from_root_bytes(bytes, &foreign_arena, &DecodePolicy::service()).unwrap();
    assert!(parse(&ctx, foreign).is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn recursion_limit_escapes_source_only_recovery_and_keeps_the_session_fused() {
    let bytes = b"<Document><ViewProviderData><ViewProvider/></ViewProviderData></Document>";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let source = parse(&ctx, root).unwrap();
    let entries = Default::default();
    let sources = GuiSources {
        entries: &entries,
        entry_owners: None,
        entry_interpretations: None,
        gui_entry_interpretations: None,
        declared_entries: None,
        declared_objects: None,
        admitted_objects: None,
        entry_parents: None,
        objects: &[],
        object_sources: None,
        properties: &[],
        payloads: &[],
        element_maps: &[],
        topology_occurrences: &[],
        requires_alpha_conversion: false,
    };
    let mut output =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    // #215: the recovering scanner is navigation only. The first nested actual
    // part reaches the transaction depth limit without fusing the session.
    let graph = source.decode(&ctx, &sources, &mut output).unwrap();
    assert!(graph.documents.is_empty());
    let failed = output
        .fidelity()
        .parts()
        .values()
        .filter(|part| {
            matches!(
                part.state(),
                cadmpeg_ir::source_fidelity::parts::PartState::GivenUp { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level().as_str(), "freecad.gui_document");
    assert!(ctx.finish_session().is_ok());
}

mod schema_reads;

mod recovery;

mod material_v3;

mod appearance_reads;

mod allocation;
