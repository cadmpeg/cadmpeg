// SPDX-License-Identifier: Apache-2.0
//! Exact source schema producers, retained values, and failed-transfer rollback.

use super::*;
use cadmpeg_core::decode::{u64_from_index, View};
use std::collections::BTreeMap;

pub(super) fn assert_capture(ctx: &DecodeContext<'_>, source: View<'_>, key: &str, raw: &str) {
    let bytes = std::str::from_utf8(source.window()).unwrap();
    let at = bytes.find(&format!("{key}='{raw}'")).unwrap() + key.len() + "='".len();
    let start = u64_from_index(source.start() + at);
    let extent = ctx.capture_part_extent(source).unwrap();
    let captures = ctx.take_record_captures().unwrap();
    let [capture] = captures.as_slice() else {
        panic!("one kept GUI document: {captures:?}");
    };
    assert_eq!(capture.id(), "fcstd:gui:document#0");
    assert_eq!(capture.stream(), "file");
    assert_eq!(capture.offset(), extent.offset());
    assert_eq!(capture.byte_len(), u64_from_index(source.window().len()));
    assert_eq!(
        capture.reads(),
        &BTreeMap::from([(
            "file".into(),
            vec![start..start + u64_from_index(raw.len())]
        )])
    );
}

pub(super) fn capture_cost(bytes: &[u8]) -> u64 {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    // Early parsing/framing already activates the real tracking SessionSlot.
    // Measure the producer increment after that same state, so its one-time
    // session allocation is neither charged twice nor replaced by a constant.
    let prepared = parse(&ctx, root).unwrap();
    let xml = prepared.document().unwrap();
    let before = ctx.retained_bytes_available();
    let source = crate::persistence::source::Source::new(&ctx, root, xml).unwrap();
    let version = source
        .record("fcstd:gui:document#0", xml.root_element(), || {
            source.required_attribute(xml.root_element(), "SchemaVersion")
        })
        .unwrap();
    assert_eq!(version, "1");
    before - ctx.retained_bytes_available()
}

#[test]
fn kept_schema_value_comes_from_exact_entity_or_alias_bytes_in_nonzero_source_windows() {
    for (key, raw, kept) in [
        ("SchemaVersion", "&#49;", "1"),
        ("schemaVersion", "2", "2"),
        ("SchemaVersion", "future&amp;schema", "future&schema"),
    ] {
        let xml = format!("<Document {key}='{raw}' Vendor='opaque-extra'><ViewProviderData Count='0'/></Document>");
        let bytes = format!("prefix{xml}suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes.as_bytes(), &arena, &DecodePolicy::service())
                .unwrap();
        let view = root
            .child("prefix".len(), "prefix".len() + xml.len())
            .unwrap();
        let source = parse(&ctx, view).unwrap();
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
        if key == "schemaVersion" {
            // The original strict vocabulary refuses lowercase-only aliases.
            // Preserve that exact input and require source-only recovery.
            assert!(graph.untyped);
            assert_eq!(graph.documents.len(), 1);
            assert_eq!(graph.documents[0].schema_version, None);
            assert!(graph.documents[0].attributes.is_empty());
            assert!(ir.model.presentation_documents.is_empty());
            assert_eq!(graph.losses.len(), 1);
            assert_eq!(
                graph.losses[0].code.local_code(),
                "source.gui-metadata-unresolved"
            );
            assert!(graph.losses[0]
                .message
                .contains("unsupported schemaVersion; expected SchemaVersion"));
            assert!(ctx.take_record_captures().unwrap().is_empty());
            continue;
        }
        assert!(!graph.untyped);
        assert_eq!(graph.documents.len(), 1);
        assert_eq!(graph.documents[0].schema_version.as_deref(), Some(kept));
        assert_eq!(graph.documents[0].attributes[key], kept);
        assert_eq!(graph.documents[0].attributes["Vendor"], "opaque-extra");
        assert_eq!(ir.model.presentation_documents.len(), 1);
        assert_eq!(
            ir.model.presentation_documents[0].schema_version,
            (kept == "1").then_some(1)
        );
        assert_capture(&ctx, view, key, raw);
    }
}

#[test]
fn rejected_provider_transfer_keeps_source_only_behavior_without_speculative_schema_reads() {
    let bytes = b"<Document SchemaVersion='1'><ViewProviderData Count='1'/></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = parse(&ctx, root).unwrap();
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
    assert!(graph.untyped);
    assert_eq!(graph.documents[0].schema_version.as_deref(), Some("1"));
    assert_eq!(graph.losses.len(), 1);
    assert_eq!(
        graph.losses[0].code.local_code(),
        "source.gui-metadata-unresolved"
    );
    assert!(graph.losses[0].message.contains("Count=1 but 0 records"));
    assert!(ir.model.presentation_documents.is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn a_foreign_context_cannot_accept_a_preparsed_source_document() {
    let bytes = b"<Document SchemaVersion='1'><ViewProviderData Count='0'/></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = parse(&ctx, root).unwrap();
    let foreign_arena = DecodeArena::new();
    let (foreign_ctx, _) =
        DecodeContext::from_root_bytes(bytes, &foreign_arena, &DecodePolicy::service()).unwrap();
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
    assert!(source.transfer(&foreign_ctx, &mut ir, &sources).is_err());
    assert!(ir.model.presentation_documents.is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
    assert!(foreign_ctx.take_record_captures().unwrap().is_empty());
}
