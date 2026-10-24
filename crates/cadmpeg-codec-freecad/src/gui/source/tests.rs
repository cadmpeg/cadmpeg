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
        source.document().unwrap().input_text().as_ptr(),
        root.window().as_ptr()
    );
    assert!(source.framing().unwrap().supported());
    let available = ctx.retained_bytes_available();
    let entries = Default::default();
    let sources = GuiSources {
        entries: &entries,
        objects: &[],
        properties: &[],
        payloads: &[],
        topology_occurrences: &[],
        requires_alpha_conversion: false,
    };
    let mut ir = CadIr::empty();
    let graph = source.transfer(&ctx, &mut ir, &sources).unwrap();
    // The existing DOM is reused; the only newly retained storage is the
    // genuine kept-schema producer's record/capture state, measured separately.
    let capture_cost = schema_reads::capture_cost(bytes);
    assert_eq!(ctx.retained_bytes_available(), available - capture_cost);
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
        let (legacy_ctx, _) =
            DecodeContext::from_root_bytes(bytes, &legacy_arena, &DecodePolicy::service()).unwrap();
        let entries = Default::default();
        let sources = GuiSources {
            entries: &entries,
            objects: &[],
            properties: &[],
            payloads: &[],
            topology_occurrences: &[],
            requires_alpha_conversion: false,
        };
        let mut ir = CadIr::empty();
        let mut original_ir = CadIr::empty();
        let graph = source.transfer(&ctx, &mut ir, &sources).unwrap();
        let original =
            super::super::transfer(&legacy_ctx, &mut original_ir, bytes, &sources).unwrap();
        assert_eq!(graph_value(&graph), graph_value(&original));
        assert_eq!(ir.model, original_ir.model);
        assert_eq!(graph.provider_sources, original.provider_sources);
        match graph.documents[0].schema_version.as_deref() {
            Some(value) => schema_reads::assert_capture(&ctx, root, "SchemaVersion", value),
            None => assert!(ctx.take_record_captures().unwrap().is_empty()),
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
    assert!(matches!(
        parse(&ctx, root),
        Err(CodecError::ResourceLimit(_))
    ));
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(_))
    ));
}

mod schema_reads;
