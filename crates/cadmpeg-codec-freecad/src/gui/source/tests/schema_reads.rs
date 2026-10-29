// SPDX-License-Identifier: Apache-2.0
//! Exact source schema producers, retained values, and failed-transfer rollback.

use super::*;

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
        let entries = std::collections::BTreeMap::default();
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
    }
}

#[test]
fn rejected_provider_transfer_keeps_source_only_behavior_without_speculative_schema_reads() {
    let bytes = b"<Document SchemaVersion='1'><ViewProviderData Count='1'/></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = parse(&ctx, root).unwrap();
    let entries = std::collections::BTreeMap::default();
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
}
