// SPDX-License-Identifier: Apache-2.0
//! Source-only XML fields retain exact bytes without claiming semantic reads.

use super::{install_layouts, xml_layout_payload};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::SourceFidelity;
use std::collections::BTreeMap;

#[test]
fn xml_payload_uses_the_real_record_extent_and_preserves_exact_source_text() {
    let bytes =
        b"prefix <Extension name='A' type='Vendor::Ext'>\n  exact &amp; text\n</Extension> suffix";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
    let root = ctx.name_stream(root, "Document.xml").unwrap();
    let source = root.child(7, bytes.len() - 7).unwrap();
    let mut payloads = BTreeMap::new();
    let id = "fcstd:native:extension#A";
    xml_layout_payload(&ctx, source, id, 0..source.window().len(), &mut payloads).unwrap();
    let mut fidelity = SourceFidelity::default();
    install_layouts(&ctx, &mut fidelity, payloads).unwrap();
    let layout = &fidelity.record_layout()[id];
    assert_eq!(layout.stream(), "Document.xml");
    assert_eq!(layout.offset(), 7);
    assert_eq!(layout.byte_len(), u64::try_from(bytes.len() - 14).unwrap());
    assert!(layout.reads().is_empty());
    assert!(layout.pointers().is_empty());
    assert_eq!(layout.payloads().len(), 1);
    assert_eq!(
        layout.payloads()["raw_xml"].as_ref(),
        &bytes[7..bytes.len() - 7]
    );
    assert!(fidelity.retained_records().is_empty());
}

#[test]
fn detached_xml_bytes_cannot_claim_a_source_record_layout() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"<Extension/>", &arena, &DecodePolicy::default()).unwrap();
    let detached = View::over_retained(b"<Extension/>");
    let mut payloads = BTreeMap::new();
    assert!(xml_layout_payload(
        &ctx,
        detached,
        "fcstd:native:extension#A",
        0..12,
        &mut payloads,
    )
    .is_err());
    assert!(payloads.is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn duplicate_xml_owner_does_not_overwrite_the_first_payload() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"<A/><B/>", &arena, &DecodePolicy::default()).unwrap();
    let mut payloads = BTreeMap::new();
    let id = "fcstd:native:extension#A";
    xml_layout_payload(&ctx, root, id, 0..4, &mut payloads).unwrap();
    assert!(xml_layout_payload(&ctx, root, id, 4..8, &mut payloads).is_err());
    assert_eq!(payloads[id].as_ref(), b"<A/>");
    let mut fidelity = SourceFidelity::default();
    install_layouts(&ctx, &mut fidelity, payloads).unwrap();
    assert_eq!(fidelity.record_layout().len(), 1);
    assert_eq!(
        fidelity.record_layout()[id].payloads()["raw_xml"].as_ref(),
        b"<A/>"
    );
}

#[test]
fn extension_parser_preserves_the_source_range_separate_from_native_fields() {
    let xml = "<Document SchemaVersion='4'><Objects Count='1'><Object type='App::Feature' name='A' id='1'/></Objects><ObjectData Count='1'><Object name='A'><Extensions Count='1'><Extension name='Vendor' type='Vendor::Extension'><Data value='a &amp; b'/></Extension></Extensions><Properties Count='0'/></Object></ObjectData></Document>";
    let document = roxmltree::Document::parse(xml).unwrap();
    let graph =
        crate::persistence::parse_document(&document, crate::persistence::Vocabulary::Objects)
            .unwrap();
    assert_eq!(graph.extensions.len(), 1);
    assert_eq!(graph.extension_sources.len(), 1);
    let record = &graph.extensions[0];
    let (id, range) = &graph.extension_sources[0];
    assert_eq!(id, &record.id);
    assert_eq!(
        &xml[range.clone()],
        "<Extension name='Vendor' type='Vendor::Extension'><Data value='a &amp; b'/></Extension>"
    );
    let native = serde_json::to_value(record).unwrap();
    assert_eq!(native["name"], "Vendor");
    assert_eq!(native["type_name"], "Vendor::Extension");
    for name in ["raw_xml", "byte_start", "byte_end", "sha256", "length"] {
        assert!(native.get(name).is_none(), "{name}");
    }
    assert_eq!(
        serde_json::from_value::<crate::native::ExtensionRecord>(native).unwrap(),
        *record
    );
}

#[test]
fn gui_only_provider_uses_its_actual_xml_owner_and_keeps_typed_values() {
    let xml = b"<GuiDocument SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='GUI only' expanded='1'><Properties Count='0'/></ViewProvider></ViewProviderData></GuiDocument>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::default()).unwrap();
    let root = ctx.name_stream(root, "GuiDocument.xml").unwrap();
    let entries = BTreeMap::from([("GuiDocument.xml".into(), root)]);
    let mut ir = cadmpeg_ir::CadIr::empty();
    let graph = crate::gui::transfer(
        &ctx,
        &mut ir,
        root.window(),
        &crate::gui::GuiSources {
            entries: &entries,
            objects: &[],
            properties: &[],
            payloads: &[],
            element_maps: &[],
            requires_alpha_conversion: false,
        },
    )
    .unwrap();
    assert!(!graph.untyped);
    assert_eq!(graph.providers.len(), 1);
    assert_eq!(graph.provider_sources.len(), 1);
    let provider = &graph.providers[0];
    assert_eq!(provider.name, "GUI only");
    assert_eq!(provider.object, None);
    assert_eq!(provider.expanded, Some(true));
    let (id, range) = &graph.provider_sources[0];
    assert_eq!(id, &provider.id);
    let mut payloads = BTreeMap::new();
    xml_layout_payload(&ctx, root, id, range.clone(), &mut payloads).unwrap();
    let mut fidelity = SourceFidelity::default();
    install_layouts(&ctx, &mut fidelity, payloads).unwrap();
    assert_eq!(
        fidelity.record_layout()[id].payloads()["raw_xml"].as_ref(),
        b"<ViewProvider name='GUI only' expanded='1'><Properties Count='0'/></ViewProvider>"
    );
    let native = serde_json::to_value(provider).unwrap();
    assert_eq!(native["name"], "GUI only");
    assert_eq!(native["expanded"], true);
    for name in ["raw_xml", "byte_start", "byte_end", "sha256", "length"] {
        assert!(native.get(name).is_none(), "{name}");
    }
}
