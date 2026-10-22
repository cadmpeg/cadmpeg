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
    assert_eq!(payloads[id]["raw_xml"].as_ref(), b"<A/>");
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

#[test]
fn python_and_nested_value_xml_move_to_their_real_property_layouts() {
    let xml = "<Document SchemaVersion='4'><Objects Count='1'><Object type='App::FeaturePython' name='A' id='1'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='2'><Property name='Proxy' type='App::PropertyPythonObject'><PythonObject class='Inert'>  opaque AP8A &amp; payload  </PythonObject></Property><Property name='Names' type='App::PropertyStringList'><StringList count='2'><String value='a &amp; b'/><String> Ω </String></StringList></Property></Properties></Object></ObjectData></Document>";
    let mut bytes = b"prefix!".to_vec();
    bytes.extend_from_slice(xml.as_bytes());
    bytes.extend_from_slice(b"suffix!");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let view = ctx
        .register_named_slice(
            root,
            cadmpeg_core::decode::ByteRange {
                start: 7,
                end: 7 + u64::try_from(xml.len()).unwrap(),
            },
            "Document.xml",
        )
        .unwrap();
    let document = roxmltree::Document::parse(std::str::from_utf8(view.window()).unwrap()).unwrap();
    let graph =
        crate::persistence::parse_document(&document, crate::persistence::Vocabulary::Objects)
            .unwrap();
    assert_eq!(graph.properties.len(), 2);
    assert_eq!(graph.value_sources.len(), 2);
    let mut pending = BTreeMap::new();
    for source in &graph.value_sources {
        super::value_layout_payloads(&ctx, view, source, &mut pending).unwrap();
    }
    let mut fidelity = SourceFidelity::default();
    install_layouts(&ctx, &mut fidelity, pending).unwrap();
    let proxy = graph
        .properties
        .iter()
        .find(|property| property.name == "Proxy")
        .unwrap();
    let names = graph
        .properties
        .iter()
        .find(|property| property.name == "Names")
        .unwrap();
    assert_eq!(
        proxy.values()[0].text.as_deref(),
        Some("  opaque AP8A & payload  ")
    );
    assert_eq!(names.values()[2].text.as_deref(), Some(" Ω "));
    for property in [&proxy, &names] {
        let layout = &fidelity.record_layout()[&property.id];
        assert_eq!(layout.stream(), "Document.xml");
        assert_eq!(layout.offset(), property.xml.start());
        assert_eq!(layout.byte_len(), property.xml.end() - property.xml.start());
        assert!(layout.reads().is_empty());
        assert_eq!(layout.payloads().len(), property.values().len());
        let wire = serde_json::to_value(property).unwrap();
        for value in wire["values"].as_array().unwrap() {
            assert!(value.get("raw_xml").is_none());
        }
    }
    assert_eq!(
        fidelity.record_layout()[&proxy.id].payloads()["values.0.raw_xml"].as_ref(),
        b"<PythonObject class='Inert'>  opaque AP8A &amp; payload  </PythonObject>"
    );
    assert_eq!(
        fidelity.record_layout()[&names.id].payloads()["values.0.raw_xml"].as_ref(),
        "<StringList count='2'><String value='a &amp; b'/><String> Ω </String></StringList>"
            .as_bytes()
    );
    assert_eq!(
        fidelity.record_layout()[&names.id].payloads()["values.1.raw_xml"].as_ref(),
        b"<String value='a &amp; b'/>"
    );
    assert_eq!(
        fidelity.record_layout()[&names.id].payloads()["values.2.raw_xml"].as_ref(),
        "<String> Ω </String>".as_bytes()
    );
    assert!(fidelity.retained_records().is_empty());
    assert!(ctx
        .source_streams()
        .unwrap()
        .iter()
        .any(|stream| stream.name() == "Document.xml"));
}

#[test]
fn gui_state_and_provider_property_values_keep_separate_source_owners() {
    let xml = b"<GuiDocument SchemaVersion='1'><Layout><Panel name='authored'/></Layout><ViewProviderData Count='1'><ViewProvider name='GUI only'><Properties Count='1'><Property name='Visibility' type='App::PropertyBool'><Bool value='1'/></Property></Properties></ViewProvider></ViewProviderData></GuiDocument>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::service()).unwrap();
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
    assert_eq!(graph.value_sources.len(), 2);
    assert_eq!(graph.documents[0].states[0].kind, "Layout");
    let mut pending = BTreeMap::new();
    for source in &graph.value_sources {
        super::value_layout_payloads(&ctx, root, source, &mut pending).unwrap();
    }
    let mut fidelity = SourceFidelity::default();
    install_layouts(&ctx, &mut fidelity, pending).unwrap();
    let state = &graph.documents[0].states[0];
    let property = &graph.properties[0];
    assert_ne!(state.id, property.id);
    assert_eq!(
        fidelity.record_layout()[&state.id].payloads()["values.0.raw_xml"].as_ref(),
        b"<Panel name='authored'/>"
    );
    assert_eq!(
        fidelity.record_layout()[&property.id].payloads()["values.0.raw_xml"].as_ref(),
        b"<Bool value='1'/>"
    );
    assert!(fidelity
        .record_layout()
        .values()
        .all(|layout| layout.stream() == "GuiDocument.xml" && layout.reads().is_empty()));
}

#[test]
fn value_ranges_cannot_bind_to_a_different_source_element_or_value_order() {
    let xml = "<Property><Bool value='1'/></Property>";
    let document = roxmltree::Document::parse(xml).unwrap();
    let value = crate::native::ValueRecord {
        tag: "String".into(),
        order: 0,
        parent: None,
        attributes: BTreeMap::new(),
        decoded_json: None,
        text: None,
    };
    let span = crate::native::RetainedXml::from_source(xml, 0).unwrap();
    assert!(super::value_sources(
        &document,
        std::iter::once(("test:property#A", &span, std::slice::from_ref(&value)))
    )
    .is_err());
    let shifted = crate::native::RetainedXml::from_source(xml, 1).unwrap();
    assert!(super::value_sources(
        &document,
        std::iter::once(("test:property#A", &shifted, std::slice::from_ref(&value)))
    )
    .is_err());
}
