// SPDX-License-Identifier: Apache-2.0
//! Source-only XML fields retain exact bytes.

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
    install_layouts(&ctx, &mut fidelity, payloads, BTreeMap::new(), None).unwrap();
    let layout = &fidelity.record_layout()[id];
    assert_eq!(layout.stream(), "Document.xml");
    assert_eq!(layout.offset(), 7);
    assert_eq!(layout.byte_len(), u64::try_from(bytes.len() - 14).unwrap());
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
    install_layouts(&ctx, &mut fidelity, payloads, BTreeMap::new(), None).unwrap();
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
            entry_parents: None,
            entry_owners: None,
            entry_interpretations: None,
            gui_entry_interpretations: None,
            declared_entries: None,
            declared_objects: None,
            admitted_objects: None,
            objects: &[],
            object_sources: None,
            properties: &[],
            payloads: &[],
            topology_occurrences: &[],
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
    super::captured_xml_layout_payload(&ctx, root, id, range.clone(), &mut payloads).unwrap();
    let mut fidelity = SourceFidelity::default();
    install_layouts(&ctx, &mut fidelity, payloads, BTreeMap::new(), None).unwrap();
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
    let graph = crate::persistence::parse_document_source(
        &ctx,
        view,
        &document,
        crate::persistence::Vocabulary::Objects,
    )
    .unwrap();
    assert_eq!(graph.properties.len(), 2);
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
    assert!(proxy.body.is_none());
    assert_eq!(names.values().len(), 2);
    assert_eq!(names.values()[0].attributes["count"], "2");
    assert_eq!(names.values()[1].attributes["value"], "a & b");
    let wire = serde_json::to_value(names).unwrap();
    assert_eq!(wire["values"].as_array().unwrap().len(), 3);
    assert!(wire["values"][2].is_null());
    let expected_failed = [
        "<PythonObject class='Inert'>  opaque AP8A &amp; payload  </PythonObject>",
        "<String> Ω </String>",
    ];
    assert_eq!(graph.losses.len(), expected_failed.len());
    for raw in expected_failed {
        let start = u64::try_from(xml.find(raw).unwrap()).unwrap();
        let loss = graph
            .losses
            .iter()
            .find(|loss| {
                loss.provenance.as_ref().is_some_and(|source| {
                    source.offset == start
                        && source.byte_len() == Some(u64::try_from(raw.len()).unwrap())
                })
            })
            .unwrap();
        assert_eq!(loss.code.local_code(), "record.unreadable");
    }
    let mut payloads = BTreeMap::new();
    let mut pointers = BTreeMap::new();
    for property in &graph.properties {
        super::captured_property_layout_payloads(
            &ctx,
            view,
            &graph.property_sources[&property.id],
            property,
            &mut payloads,
            &mut pointers,
        )
        .unwrap();
    }
    let mut fidelity = SourceFidelity::default();
    install_layouts(&ctx, &mut fidelity, payloads, pointers, None).unwrap();
    let proxy_layout = &fidelity.record_layout()[&proxy.id];
    assert!(proxy_layout.payloads().is_empty());
    let failed = xml.find(expected_failed[0]).unwrap();
    assert!(proxy_layout.reads()["Document.xml"].iter().all(|range| {
        range.end <= u64::try_from(failed).unwrap()
            || range.start >= u64::try_from(failed + expected_failed[0].len()).unwrap()
    }));
    let layout = &fidelity.record_layout()[&names.id];
    assert_eq!(layout.stream(), "Document.xml");
    assert_eq!(
        layout.offset(),
        u64::try_from(graph.property_sources[&names.id].record_range().start).unwrap()
    );
    assert_eq!(
        layout.byte_len(),
        u64::try_from(graph.property_sources[&names.id].record_range().len()).unwrap()
    );
    assert_eq!(layout.payloads().len(), 3);
    assert_eq!(
        layout.payloads()["values.0.raw_xml"].as_ref(),
        "<StringList count='2'><String value='a &amp; b'/><String> Ω </String></StringList>"
            .as_bytes()
    );
    assert_eq!(
        layout.payloads()["values.1.raw_xml"].as_ref(),
        b"<String value='a &amp; b'/>"
    );
    assert!(!layout.payloads().contains_key("values.2.raw_xml"));
    assert!(fidelity.retained_records().is_empty());
    for property in [&proxy, &names] {
        let wire = serde_json::to_value(property).unwrap();
        for value in wire["values"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|value| !value.is_null())
        {
            assert!(value.get("raw_xml").is_none());
        }
    }
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
            entry_parents: None,
            entry_owners: None,
            entry_interpretations: None,
            gui_entry_interpretations: None,
            declared_entries: None,
            declared_objects: None,
            admitted_objects: None,
            objects: &[],
            object_sources: None,
            properties: &[],
            payloads: &[],
            topology_occurrences: &[],
            requires_alpha_conversion: false,
        },
    )
    .unwrap();
    assert!(!graph.untyped);
    assert_eq!(graph.value_sources.len(), 2);
    assert_eq!(graph.documents[0].states[0].kind, "Layout");
    let mut pending = BTreeMap::new();
    let mut pointers = BTreeMap::new();
    super::gui_layout_payloads(&ctx, &entries, &graph, &mut pending, &mut pointers).unwrap();
    let mut fidelity = SourceFidelity::default();
    install_layouts(&ctx, &mut fidelity, pending, pointers, None).unwrap();
    let state = &graph.documents[0].states[0];
    let property = &graph.properties[0];
    assert_ne!(state.id, property.id);
    assert_eq!(
        fidelity.record_layout()[&graph.documents[0].id].payloads()["states.0.values.0.raw_xml"]
            .as_ref(),
        b"<Panel name='authored'/>"
    );
    assert_eq!(
        fidelity.record_layout()[&property.id].payloads()["values.0.raw_xml"].as_ref(),
        b"<Bool value='1'/>"
    );
    assert!(fidelity
        .record_layout()
        .values()
        .all(|layout| layout.stream() == "GuiDocument.xml"));
}

#[test]
fn value_ranges_cannot_bind_to_a_different_source_element_or_value_order() {
    let xml = "<Property><Bool value='1'/></Property>";
    assert!(super::ValueSourceRanges::new_slots(
        "test:property#A".into(),
        0..xml.len(),
        vec![(1, 10..27), (0, 10..27)],
    )
    .is_err());
    assert!(super::ValueSourceRanges::new_slots(
        "test:property#A".into(),
        10..27,
        vec![(0, 0..xml.len())],
    )
    .is_err());
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    ctx.capture_part_extent(root).unwrap();
    assert!(super::relative_xml_extent(root, 1..xml.len() + 1).is_err());
}

#[test]
fn gui_state_xml_and_offsets_leave_native_records_without_losing_empty_or_nested_states() {
    let xml = "<GuiDocument SchemaVersion='1'><Layout><Panel name='a &amp; b'> Ω </Panel></Layout><EmptyState flag='0'/><ViewProviderData Count='0'/></GuiDocument>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "GuiDocument.xml").unwrap();
    let entries = BTreeMap::from([("GuiDocument.xml".into(), root)]);
    let sources = crate::entry_sources::EntrySources::new(&ctx, &entries).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let graph = crate::gui::transfer(
        &ctx,
        &mut ir,
        root.window(),
        &crate::gui::GuiSources {
            entries: &entries,
            entry_parents: None,
            entry_owners: None,
            entry_interpretations: None,
            gui_entry_interpretations: None,
            declared_entries: None,
            declared_objects: None,
            admitted_objects: None,
            objects: &[],
            object_sources: None,
            properties: &[],
            payloads: &[],
            topology_occurrences: &[],
            requires_alpha_conversion: false,
        },
    )
    .unwrap();
    assert!(!graph.untyped);
    assert_eq!(graph.documents[0].states.len(), 2);
    assert_eq!(graph.state_sources.len(), 2);
    let mut payloads = BTreeMap::new();
    let mut pointers = BTreeMap::new();
    super::gui_layout_payloads(&ctx, &entries, &graph, &mut payloads, &mut pointers).unwrap();
    let mut fidelity = SourceFidelity::default();
    install_layouts(&ctx, &mut fidelity, payloads, pointers, None).unwrap();
    let expected = [
        "<Layout><Panel name='a &amp; b'> Ω </Panel></Layout>",
        "<EmptyState flag='0'/>",
    ];
    for (order, (state, raw)) in graph.documents[0].states.iter().zip(expected).enumerate() {
        let start = xml.find(raw).unwrap();
        let end = start + raw.len();
        let layout = &fidelity.record_layout()[&graph.documents[0].id];
        assert!(!fidelity.record_layout().contains_key(&state.id));
        assert_eq!(layout.stream(), "GuiDocument.xml");
        assert_eq!(layout.offset(), 0);
        assert_eq!(layout.byte_len(), u64::try_from(xml.len()).unwrap());
        assert_eq!(
            layout.pointers()[&format!("states.{order}.byte_start")],
            u64::try_from(start).unwrap()
        );
        assert_eq!(
            layout.pointers()[&format!("states.{order}.byte_end")],
            u64::try_from(end).unwrap()
        );
        assert_eq!(
            layout.payloads()[&format!("states.{order}.raw_xml")].as_ref(),
            raw.as_bytes()
        );
        assert!(fidelity.retained_record(&state.id).is_none());
        let native = serde_json::to_value(state).unwrap();
        for field in [
            "raw_xml",
            "byte_start",
            "byte_end",
            "length",
            "sha256",
            "xml",
        ] {
            assert!(native.get(field).is_none(), "{field}");
        }
        assert_eq!(native["kind"], state.kind);
    }
    let nested = &graph.documents[0].states[0];
    assert_eq!(nested.values[0].attributes["name"], "a & b");
    assert_eq!(nested.values[0].text.as_deref(), Some(" Ω "));
    assert_eq!(
        fidelity.record_layout()[&graph.documents[0].id].payloads()["states.0.values.0.raw_xml"]
            .as_ref(),
        "<Panel name='a &amp; b'> Ω </Panel>".as_bytes()
    );
    let empty = &graph.documents[0].states[1];
    assert!(empty.values.is_empty());
    assert_eq!(empty.attributes["flag"], "0");
    assert_eq!(
        fidelity.record_layout()[&graph.documents[0].id]
            .payloads()
            .keys()
            .filter(|key| key.starts_with("states.1."))
            .count(),
        1
    );
    let entry = crate::test_support::entry_fixture(
        "fcstd:native:entry#GuiDocument.xml".into(),
        "GuiDocument.xml".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        xml.as_bytes().to_vec(),
    );
    let ledger = crate::container::logical_ledger(
        &[entry.record],
        &[],
        &BTreeMap::new(),
        &sources,
        &graph,
        &[],
        &[],
        &[],
    )
    .unwrap();
    for raw in expected {
        let span = ledger.iter().find(|span| span.span.start() == u64::try_from(xml.find(raw).unwrap()).unwrap()
            && matches!(&span.classification, cadmpeg_ir::source_fidelity::freecad::LogicalClassification::Typed { owner } if owner == &graph.documents[0].id)).unwrap();
        assert_eq!(
            span.span.start(),
            u64::try_from(xml.find(raw).unwrap()).unwrap()
        );
        assert_eq!(
            span.span.end() - span.span.start(),
            u64::try_from(raw.len()).unwrap()
        );
    }
}

#[test]
fn repeated_gui_state_capture_cannot_replace_the_first_payload_or_pointer_fields() {
    let xml = "<Layout><Panel name='first'/></Layout>";
    let document = roxmltree::Document::parse(xml).unwrap();
    let state = crate::native::GuiStateRecord {
        id: "fcstd:native:gui-state#0:Layout".into(),
        kind: "Layout".into(),
        attributes: BTreeMap::new(),
        values: vec![crate::native::ValueRecord {
            item_index: None,
            link_index: None,
            tag: "Panel".into(),
            order: 0,
            parent: None,
            attributes: BTreeMap::from([("name".into(), "first".into())]),
            decoded_json: None,
            text: None,
        }]
        .into(),
        side_entries: Vec::new(),
    };
    let source = super::ValueSourceRanges::new(
        state.id.clone(),
        document.root_element().range(),
        document
            .root_element()
            .descendants()
            .filter(|node| node.is_element() && *node != document.root_element())
            .map(|node| node.range())
            .collect(),
    )
    .unwrap();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let document_id = "fcstd:gui:document#0";
    ctx.record(document_id, root, || Ok(())).unwrap();
    let mut payloads = BTreeMap::new();
    let mut pointers = BTreeMap::new();
    super::state_layout_payloads(
        &ctx,
        root,
        &source,
        document_id,
        0,
        &mut payloads,
        &mut pointers,
    )
    .unwrap();
    let expected_payloads = payloads.clone();
    let expected_pointers = pointers.clone();
    assert!(super::state_layout_payloads(
        &ctx,
        root,
        &source,
        document_id,
        0,
        &mut payloads,
        &mut pointers
    )
    .is_err());
    assert_eq!(payloads, expected_payloads);
    assert_eq!(pointers, expected_pointers);
    let mut fidelity = SourceFidelity::default();
    install_layouts(&ctx, &mut fidelity, payloads, pointers, None).unwrap();
    assert_eq!(fidelity.record_layout().len(), 1);
    assert_eq!(
        fidelity.record_layout()[document_id].payloads()["states.0.raw_xml"].as_ref(),
        xml.as_bytes()
    );
}

#[test]
fn gui_property_wrappers_move_to_real_layouts_with_nested_values_empty_payload_and_side_entry() {
    let xml = "<GuiDocument SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='GUI only' expanded='0'><Properties Count='2'><Property name='VendorData' type='Vendor::Payload' status='7'><Vendor file='side.bin'><Leaf name='a &amp; b'> Ω </Leaf></Vendor></Property><Property name='Empty' type='Vendor::Empty'/></Properties></ViewProvider></ViewProviderData></GuiDocument>";
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
            "GuiDocument.xml",
        )
        .unwrap();
    let entries = BTreeMap::from([("GuiDocument.xml".into(), view)]);
    let mut ir = cadmpeg_ir::CadIr::empty();
    let graph = crate::gui::transfer(
        &ctx,
        &mut ir,
        view.window(),
        &crate::gui::GuiSources {
            entries: &entries,
            entry_parents: None,
            entry_owners: None,
            entry_interpretations: None,
            gui_entry_interpretations: None,
            declared_entries: None,
            declared_objects: None,
            admitted_objects: None,
            objects: &[],
            object_sources: None,
            properties: &[],
            payloads: &[],
            topology_occurrences: &[],
            requires_alpha_conversion: false,
        },
    )
    .unwrap();
    assert!(!graph.untyped);
    assert_eq!(graph.properties.len(), 2);
    assert_eq!(graph.property_sources.len(), 2);
    assert!(graph.value_sources.is_empty());
    let mut payloads = BTreeMap::new();
    let mut pointers = BTreeMap::new();
    for source in graph.property_sources.values() {
        super::gui_property_layout_payloads(&ctx, view, source, &mut payloads, &mut pointers)
            .unwrap();
    }
    let mut fidelity = SourceFidelity::default();
    install_layouts(&ctx, &mut fidelity, payloads, pointers, None).unwrap();
    let expected = [
        "<Property name='VendorData' type='Vendor::Payload' status='7'><Vendor file='side.bin'><Leaf name='a &amp; b'> Ω </Leaf></Vendor></Property>",
        "<Property name='Empty' type='Vendor::Empty'/>",
    ];
    for (property, raw) in graph.properties.iter().zip(expected) {
        let start = xml.find(raw).unwrap();
        let layout = &fidelity.record_layout()[&property.id];
        assert_eq!(layout.stream(), "GuiDocument.xml");
        assert_eq!(layout.offset(), u64::try_from(start).unwrap());
        assert_eq!(layout.byte_len(), u64::try_from(raw.len()).unwrap());
        assert_eq!(
            layout.pointers()["byte_start"],
            u64::try_from(start).unwrap()
        );
        assert_eq!(
            layout.pointers()["byte_end"],
            u64::try_from(start + raw.len()).unwrap()
        );
        assert_eq!(layout.payloads()["raw_xml"].as_ref(), raw.as_bytes());
        assert!(fidelity.retained_record(&property.id).is_none());
        let native = serde_json::to_value(property).unwrap();
        for field in [
            "xml",
            "raw_xml",
            "byte_start",
            "byte_end",
            "length",
            "sha256",
        ] {
            assert!(native.get(field).is_none(), "{field}");
        }
        assert_eq!(
            serde_json::from_value::<crate::native::GuiPropertyRecord>(native).unwrap(),
            *property
        );
    }
    let property = &graph.properties[0];
    assert_eq!(property.owner, graph.providers[0].id);
    assert_eq!(property.name, "VendorData");
    assert_eq!(property.type_name, "Vendor::Payload");
    assert_eq!(property.status, Some(7));
    assert_eq!(property.order, 0);
    assert!(property.side_entries.is_empty());
    assert!(property.values.is_empty());
    assert!(fidelity.record_layout()[&property.id]
        .reads()
        .get("GuiDocument.xml")
        .is_some_and(|ranges| {
            let payload_start = u64::try_from(xml.find("<Vendor file=").unwrap()).unwrap();
            let payload_end =
                u64::try_from(xml.find("</Vendor>").unwrap() + "</Vendor>".len()).unwrap();
            ranges
                .iter()
                .all(|range| range.end <= payload_start || range.start >= payload_end)
        }));
    assert_eq!(fidelity.record_layout()[&property.id].payloads().len(), 1);
    assert_eq!(fidelity.record_layout()[&property.id].pointers().len(), 2);
    assert!(fidelity.retained_records().is_empty());
    let empty = &graph.properties[1];
    assert!(empty.values.is_empty());
    assert!(empty.side_entries.is_empty());
    assert_eq!(empty.order, 1);
    assert_eq!(fidelity.record_layout()[&empty.id].payloads().len(), 1);
    let entry = crate::test_support::entry_fixture(
        "fcstd:native:entry#GuiDocument.xml".into(),
        "GuiDocument.xml".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        xml.as_bytes().to_vec(),
    );
    let sources = crate::entry_sources::EntrySources::new(&ctx, &entries).unwrap();
    let ledger = crate::container::logical_ledger(
        &[entry.record],
        &[],
        &BTreeMap::new(),
        &sources,
        &graph,
        &[],
        &[],
        &[],
    )
    .unwrap();
    for (property, raw) in graph.properties.iter().zip(expected) {
        let span = ledger.iter().find(|span| matches!(&span.classification, cadmpeg_ir::source_fidelity::freecad::LogicalClassification::NamedOpaque { owner } if owner == &property.id)).unwrap();
        assert_eq!(
            span.span.start(),
            u64::try_from(xml.find(raw).unwrap()).unwrap()
        );
        assert_eq!(
            span.span.end() - span.span.start(),
            u64::try_from(raw.len()).unwrap()
        );
    }
}

#[test]
fn object_xml_and_extent_leave_native_records_without_losing_typed_declarations() {
    let xml = "<Document SchemaVersion='4'><Objects Count='2'><Object type='Vendor::Feature' name='A' id='7' ViewType='Gui::Vendor' Touched='1'/><Object type='App::Feature' name='B' id='8'/></Objects><ObjectData Count='2'><Object name='A'><Properties Count='0'/></Object><Object name='B'><Properties Count='1'><Property name='Label' type='App::PropertyString'><String value='Ω &amp; label'/></Property></Properties></Object></ObjectData></Document>";
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
    let graph = crate::persistence::parse_document_source(
        &ctx,
        view,
        &document,
        crate::persistence::Vocabulary::Objects,
    )
    .unwrap();
    assert_eq!(graph.objects.len(), 2);
    assert_eq!(graph.object_sources.len(), 2);
    let mut payloads = BTreeMap::new();
    let mut pointers = BTreeMap::new();
    for (id, range) in &graph.object_sources {
        super::object_layout_payloads(&ctx, view, id, range.clone(), &mut payloads, &mut pointers)
            .unwrap();
    }
    let mut fidelity = SourceFidelity::default();
    install_layouts(&ctx, &mut fidelity, payloads, pointers, None).unwrap();
    let expected = [
        "<Object name='A'><Properties Count='0'/></Object>",
        "<Object name='B'><Properties Count='1'><Property name='Label' type='App::PropertyString'><String value='Ω &amp; label'/></Property></Properties></Object>",
    ];
    for (object, raw) in graph.objects.iter().zip(expected) {
        let start = xml.find(raw).unwrap();
        let layout = &fidelity.record_layout()[object.id()];
        assert_eq!(layout.stream(), "Document.xml");
        let declaration = document
            .descendants()
            .find(|node| {
                node.has_tag_name("Object")
                    && node.attribute("name") == Some(object.name())
                    && node.attribute("type").is_some()
            })
            .unwrap()
            .range();
        assert_eq!(layout.offset(), u64::try_from(declaration.start).unwrap());
        assert_eq!(layout.byte_len(), u64::try_from(declaration.len()).unwrap());
        assert_eq!(
            layout.pointers()["byte_start"],
            u64::try_from(start).unwrap()
        );
        assert_eq!(
            layout.pointers()["byte_end"],
            u64::try_from(start + raw.len()).unwrap()
        );
        assert_eq!(layout.payloads()["raw_xml"].as_ref(), raw.as_bytes());
        assert_eq!(layout.payloads().len(), 1);
        assert_eq!(layout.pointers().len(), 2);
        let native = serde_json::to_value(object).unwrap();
        for field in [
            "data",
            "data_evidence",
            "raw_xml",
            "byte_start",
            "byte_end",
            "byte_len",
            "length",
            "sha256",
        ] {
            assert!(native.get(field).is_none(), "{field}");
        }
        assert_eq!(
            serde_json::from_value::<crate::native::ObjectRecord>(native).unwrap(),
            *object
        );
    }
    assert!(fidelity.retained_records().is_empty());
    let object = &graph.objects[0];
    assert_eq!(object.id(), "fcstd:native:object#A");
    assert_eq!(object.name(), "A");
    assert_eq!(object.type_name, "Vendor::Feature");
    assert_eq!(object.persistent_id, Some(7));
    assert_eq!(object.view_type.as_deref(), Some("Gui::Vendor"));
    assert_eq!(object.attributes["Touched"], "1");
    assert_eq!(object.order, 0);
    assert_eq!(
        graph.properties[0].values()[0].attributes["value"],
        "Ω & label"
    );
    let mut namespace = cadmpeg_ir::native::NativeNamespace::default();
    crate::application::install(&ctx, &mut namespace, &graph.objects, &graph.properties, &[])
        .unwrap();
    let applications = namespace
        .arena_as::<serde_json::Value>("applications")
        .unwrap();
    assert_eq!(applications.len(), 2);
    for (application, object) in applications.iter().zip(&graph.objects) {
        assert_eq!(application["object"], object.id().as_str());
        assert_eq!(application["type_name"], object.type_name);
        for field in ["byte_start", "byte_end", "byte_len", "sha256"] {
            assert!(application.get(field).is_none(), "{field}");
        }
    }
    assert!(crate::application::matches_native(
        &ctx,
        &namespace,
        &graph.objects,
        &graph.properties,
        &[]
    )
    .unwrap());
}

#[test]
fn complete_typed_gui_properties_keep_exact_layouts_and_real_side_entry_views() {
    let raw = [
        "<Property name='Names' type='App::PropertyStringList' status='7'><StringList count='2'><String value='Ω &amp; β'/><String value='authored'/></StringList></Property>",
        "<Property name='Included' type='App::PropertyFileIncluded'><FileIncluded file='side.bin'/></Property>",
        "<Property name='Empty' type='App::PropertyStringList'><StringList count='0'/></Property>",
    ];
    let xml = format!("<GuiDocument SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='GUI only' expanded='0'><Properties Count='3'>{}</Properties></ViewProvider></ViewProviderData></GuiDocument>", raw.join(""));
    let side = b"authored inert side entry";
    let mut bytes = b"prefix!".to_vec();
    bytes.extend_from_slice(xml.as_bytes());
    bytes.extend_from_slice(b"suffix!");
    let side_start = bytes.len();
    bytes.extend_from_slice(side);
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
            "GuiDocument.xml",
        )
        .unwrap();
    let side_view = ctx
        .register_named_slice(
            root,
            cadmpeg_core::decode::ByteRange {
                start: u64::try_from(side_start).unwrap(),
                end: u64::try_from(bytes.len()).unwrap(),
            },
            "side.bin",
        )
        .unwrap();
    let entries = BTreeMap::from([
        ("GuiDocument.xml".into(), view),
        ("side.bin".into(), side_view),
    ]);
    let mut ir = cadmpeg_ir::CadIr::empty();
    let graph = crate::gui::transfer(
        &ctx,
        &mut ir,
        view.window(),
        &crate::gui::GuiSources {
            entries: &entries,
            entry_parents: None,
            entry_owners: None,
            entry_interpretations: None,
            gui_entry_interpretations: None,
            declared_entries: None,
            declared_objects: None,
            admitted_objects: None,
            objects: &[],
            object_sources: None,
            properties: &[],
            payloads: &[],
            topology_occurrences: &[],
            requires_alpha_conversion: false,
        },
    )
    .unwrap();
    assert!(!graph.untyped);
    assert!(graph.losses.is_empty());
    assert_eq!(graph.properties.len(), 3);
    assert_eq!(graph.property_sources.len(), 3);
    assert_eq!(entries["side.bin"].window(), side);
    let mut payloads = BTreeMap::new();
    let mut pointers = BTreeMap::new();
    for source in graph.property_sources.values() {
        super::gui_property_layout_payloads(&ctx, view, source, &mut payloads, &mut pointers)
            .unwrap();
    }
    let mut fidelity = SourceFidelity::default();
    install_layouts(&ctx, &mut fidelity, payloads, pointers, None).unwrap();
    for (property, source) in graph.properties.iter().zip(raw) {
        let start = xml.find(source).unwrap();
        let layout = &fidelity.record_layout()[&property.id];
        assert_eq!(layout.stream(), "GuiDocument.xml");
        assert_eq!(layout.offset(), u64::try_from(start).unwrap());
        assert_eq!(layout.byte_len(), u64::try_from(source.len()).unwrap());
        assert_eq!(
            layout.pointers()["byte_start"],
            u64::try_from(start).unwrap()
        );
        assert_eq!(
            layout.pointers()["byte_end"],
            u64::try_from(start + source.len()).unwrap()
        );
        assert_eq!(layout.payloads()["raw_xml"].as_ref(), source.as_bytes());
        assert_eq!(property.owner, graph.providers[0].id);
        let native = serde_json::to_value(property).unwrap();
        for field in [
            "xml",
            "raw_xml",
            "byte_start",
            "byte_end",
            "length",
            "sha256",
        ] {
            assert!(native.get(field).is_none(), "{field}");
        }
        assert_eq!(
            serde_json::from_value::<crate::native::GuiPropertyRecord>(native).unwrap(),
            *property
        );
    }
    assert_eq!(graph.properties[0].status, Some(7));
    assert_eq!(graph.properties[0].values[1].parent, Some(0));
    assert_eq!(graph.properties[0].values[1].attributes["value"], "Ω & β");
    assert_eq!(
        fidelity.record_layout()[&graph.properties[0].id].payloads()["values.1.raw_xml"].as_ref(),
        "<String value='Ω &amp; β'/>".as_bytes()
    );
    assert!(graph.properties[1].side_entries.is_empty());
    assert_eq!(graph.properties[1].values[0].attributes["file"], "side.bin");
    assert_eq!(
        fidelity.record_layout()[&graph.properties[1].id].payloads()["values.0.raw_xml"].as_ref(),
        b"<FileIncluded file='side.bin'/>"
    );
    assert_eq!(graph.properties[2].values[0].attributes["count"], "0");
    assert_eq!(graph.properties[2].values.len(), 1);
    assert_eq!(
        fidelity.record_layout()[&graph.properties[2].id].payloads()["values.0.raw_xml"].as_ref(),
        b"<StringList count='0'/>"
    );
    assert!(fidelity.retained_records().is_empty());
}

#[test]
fn complete_property_xml_and_positions_live_only_on_the_original_record_layouts() {
    let xml = "<Document SchemaVersion='4'><Objects Count='1'><Object name='A' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='2' TransientCount='1'><_Property name='Preview' type='App::PropertyString' status='152'/><Property name='Label' type='App::PropertyString'><String value='a &amp; Ω'/></Property><Property name='Empty' type='App::PropertyString'><String value=''/></Property></Properties></Object></ObjectData></Document>";
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
    let tree = roxmltree::Document::parse(std::str::from_utf8(view.window()).unwrap()).unwrap();
    let graph = crate::persistence::parse_document_source(
        &ctx,
        view,
        &tree,
        crate::persistence::Vocabulary::Objects,
    )
    .unwrap();
    assert_eq!(graph.properties.len(), 3);
    assert_eq!(graph.property_sources.len(), 3);
    assert!(graph.losses.is_empty());
    let sources = graph
        .property_sources
        .iter()
        .map(|(id, ranges)| {
            (
                id.clone(),
                super::relative_xml_extent(view, ranges.record_range()).unwrap(),
            )
        })
        .collect();
    let reader = crate::property_reader::PropertyReader::fixtures(&ctx, sources, root).unwrap();
    let mut payloads = BTreeMap::new();
    let mut pointers = BTreeMap::new();
    for property in &graph.properties {
        let source = &graph.property_sources[&property.id];
        let range = source.record_range();
        assert_eq!(reader.property_xml(property).unwrap().as_ref(), &xml[range]);
        super::property_layout_payloads(&ctx, view, source, &mut payloads, &mut pointers).unwrap();
    }
    let mut fidelity = SourceFidelity::default();
    install_layouts(&ctx, &mut fidelity, payloads, pointers, None).unwrap();
    assert!(fidelity.retained_records().is_empty());
    assert_eq!(
        fidelity.record_layout().len(),
        graph.properties.len() + graph.objects.len()
    );
    for property in &graph.properties {
        let range = graph.property_sources[&property.id].record_range();
        let layout = &fidelity.record_layout()[&property.id];
        assert_eq!(layout.stream(), "Document.xml");
        assert_eq!(layout.offset(), u64::try_from(range.start).unwrap());
        assert_eq!(layout.byte_len(), u64::try_from(range.len()).unwrap());
        assert_eq!(
            layout.pointers()["byte_start"],
            u64::try_from(range.start).unwrap()
        );
        assert_eq!(
            layout.pointers()["byte_end"],
            u64::try_from(range.end).unwrap()
        );
        assert_eq!(
            layout.payloads()["raw_xml"].as_ref(),
            &xml.as_bytes()[range]
        );
        assert_eq!(layout.payloads().len(), property.values().len() + 1);
        let native = serde_json::to_value(property).unwrap();
        for field in [
            "xml",
            "raw_xml",
            "length",
            "sha256",
            "byte_start",
            "byte_end",
        ] {
            assert!(native.get(field).is_none(), "{field}");
        }
        assert_eq!(
            serde_json::from_value::<crate::native::PropertyRecord>(native).unwrap(),
            *property
        );
    }
    assert_eq!(
        graph
            .properties
            .iter()
            .find(|property| property.name == "Label")
            .unwrap()
            .values()[0]
            .attributes["value"],
        "a & Ω"
    );
}

#[test]
fn application_payloads_link_real_entry_extents_without_an_extra_source_copy() {
    use cadmpeg_ir::{Codec, DecodeOptions};
    let document = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Owner' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Owner'><Properties Count='1'><Property name='Included' type='App::PropertyFileIncluded'><FileIncluded file='shared.bin'/></Property></Properties></Object></ObjectData></Document>";
    let payload = b"source bytes shared by application references";
    let archive = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("shared.bin", payload),
    ]);
    let decoded = crate::FcstdCodec
        .decode(
            &mut std::io::Cursor::new(&archive),
            &DecodeOptions::default(),
        )
        .unwrap();
    let fidelity = decoded.source_fidelity();
    let id = crate::native::native_id("entry", "shared.bin");
    let layout = &fidelity.record_layout()[&id];
    assert!(!layout.stream().is_empty());
    assert_ne!(layout.stream(), "shared.bin");
    assert!(layout.payloads().is_empty());
    assert_eq!(layout.pointers().len(), 5);
    assert_eq!(
        layout.pointers()["byte_len"],
        u64::try_from(payload.len()).unwrap()
    );
    // #212 requires the original archive once, rather than an expanded entry copy.
    assert!(fidelity.retained_record(&id).is_none());
    let source = crate::test_support::source_fidelity::entry_bytes(fidelity, &id);
    assert_eq!(source, payload);
    assert_eq!(
        cadmpeg_ir::hash::sha256_hex(&source),
        cadmpeg_ir::hash::sha256_hex(payload)
    );
    let source_arena = cadmpeg_core::decode::DecodeArena::new();
    let source_policy = cadmpeg_core::decode::DecodePolicy::service();
    let (source_context, source_root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &archive,
        &source_arena,
        &source_policy,
    )
    .unwrap();
    let source_archive =
        cadmpeg_container::ArchiveSnapshot::new(&source_context, source_root).unwrap();
    let frames = source_archive.member_frames(&source_context).unwrap();
    let (_, frame) = source_archive
        .entries()
        .iter()
        .zip(&frames)
        .find(|(entry, _)| entry.name == "shared.bin")
        .unwrap();
    let capture = source_context
        .capture_part_extent(frame.unwrap().header())
        .unwrap();
    assert_eq!(layout.stream(), capture.stream());
    assert_eq!(layout.offset(), capture.offset());
    assert_eq!(layout.byte_len(), capture.byte_len());
    assert_eq!(
        layout.pointers(),
        &crate::source_archive::entry_pointers(
            source_archive.entry("shared.bin").unwrap(),
            u64::try_from(payload.len()).unwrap(),
        )
        .unwrap(),
    );
    assert_eq!(
        fidelity
            .retained_record(super::SOURCE_IMAGE)
            .unwrap()
            .data(),
        Some(archive.as_slice())
    );
    assert_eq!(
        fidelity
            .retained_records()
            .values()
            .filter(|record| record.data() == Some(archive.as_slice()))
            .count(),
        1
    );
    let applications = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<serde_json::Value>("applications")
        .unwrap();
    let link = &applications[0]["property_records"][0]["payloads"][0];
    assert_eq!(link["entry"], id);
    assert_eq!(link["name"], "shared.bin");
    for field in ["sha256", "byte_len", "data"] {
        assert!(link.get(field).is_none(), "{field}");
    }
}
