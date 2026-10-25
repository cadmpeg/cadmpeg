// SPDX-License-Identifier: Apache-2.0
//! Exact source schema producers, retained values, and failed-transfer rollback.

use super::*;
use cadmpeg_core::decode::{u64_from_index, View};
use std::collections::BTreeMap;

pub(super) fn assert_capture(ctx: &DecodeContext<'_>, source: View<'_>, key: &str, raw: &str) {
    let bytes = std::str::from_utf8(source.window()).unwrap();
    assert!(bytes.contains(&format!("{key}='{raw}'")));
    // Only attribute names and encoded value bytes feed typed fields. Quotes,
    // equals signs, and separating whitespace remain source syntax (#214).
    let mut fields = vec![(key, raw)];
    if bytes.contains("Vendor='opaque-extra'") {
        fields.push(("Vendor", "opaque-extra"));
    }
    // ViewProviderData.Count admits its provider collection, but is not a
    // semantic field of the canonical GuiDocument record (#214 ownership).
    let mut reads = Vec::new();
    for (name, encoded) in fields {
        let start = bytes.find(&format!("{name}='{encoded}'")).unwrap();
        let name_start = u64_from_index(source.start() + start);
        reads.push(name_start..name_start + u64_from_index(name.len()));
        let value_start = name_start + u64_from_index(name.len() + "='".len());
        reads.push(value_start..value_start + u64_from_index(encoded.len()));
    }
    let extent = ctx.capture_part_extent(source).unwrap();
    let captures = ctx.take_record_captures().unwrap();
    let [capture] = captures.as_slice() else {
        panic!("one kept GUI document: {captures:?}");
    };
    assert_eq!(capture.id(), "fcstd:gui:document#0");
    assert_eq!(capture.stream(), "file");
    assert_eq!(capture.offset(), extent.offset());
    assert_eq!(capture.byte_len(), u64_from_index(source.window().len()));
    assert_eq!(capture.reads(), &BTreeMap::from([("file".into(), reads)]));
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
        if key == "schemaVersion" {
            // The original strict vocabulary refuses lowercase-only aliases.
            // Preserve that exact input and require source-only recovery.
            assert!(graph.untyped);
            assert!(graph.documents.is_empty());
            assert!(ir.model.presentation_documents.is_empty());
            assert!(graph.losses.is_empty());
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
    assert!(!graph.untyped);
    assert_eq!(graph.documents[0].schema_version.as_deref(), Some("1"));
    assert!(graph.providers.is_empty());
    assert!(graph.losses.is_empty());
    assert_eq!(ir.model.presentation_documents.len(), 1);
    let failed = output
        .fidelity()
        .parts()
        .values()
        .filter(|part| {
            matches!(
                part.state(),
                cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level().as_str(), "freecad.view_provider_data");
    let carrier = b"<ViewProviderData Count='1'/>";
    assert_eq!(
        failed[0].offset(),
        u64_from_index(
            bytes
                .windows(carrier.len())
                .position(|window| window == carrier)
                .unwrap()
        )
    );
    assert_eq!(failed[0].byte_len(), u64_from_index(carrier.len()));
    assert_capture(&ctx, root, "SchemaVersion", "1");
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
    assert!(source
        .transfer(
            &foreign_ctx,
            &mut ir,
            &sources,
            &mut cadmpeg_ir::source_fidelity::parts::PartOutput::new(
                cadmpeg_ir::CodecFormat::Fcstd
            )
        )
        .is_err());
    assert!(ir.model.presentation_documents.is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
    assert!(foreign_ctx.take_record_captures().unwrap().is_empty());
}
