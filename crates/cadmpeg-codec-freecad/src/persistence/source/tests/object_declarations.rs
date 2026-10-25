// SPDX-License-Identifier: Apache-2.0
//! Real declaration captures and matched data identity reads build kept objects.

use super::*;

#[test]
fn changed_object_identity_and_type_use_the_declared_extent_and_actual_matched_data_name() {
    for (raw_name, data_name, name, type_name) in [
        ("A&amp;B", "A&#38;B", "A&B", "App::Feature"),
        ("C", "C", "C", "Vendor::Feature"),
    ] {
        let declaration = format!("<Object name='{raw_name}' type='{type_name}' id='7' ViewType='Gui::Vendor' Vendor='opaque-extra'/>");
        let data = format!("<Object name='{data_name}'><Properties Count='1'><Property name='Length' type='App::PropertyFloat'><Float value='3.5'/></Property></Properties></Object>");
        let xml = format!("<Document SchemaVersion='4'><Objects Count='1'>{declaration}</Objects><ObjectData Count='1'>{data}</ObjectData></Document>");
        let bytes = format!("prefix{xml}suffix");
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes.as_bytes(), &arena, &policy).unwrap();
        let view = root.child(6, 6 + xml.len()).unwrap();
        let dom = roxmltree::Document::parse(std::str::from_utf8(view.window()).unwrap()).unwrap();
        let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
        assert_eq!(graph.objects.len(), 1);
        let object = &graph.objects[0];
        assert_eq!(object.name(), name);
        assert_eq!(object.type_name, type_name);
        assert_eq!(object.id(), &crate::native::native_id("object", name));
        assert_eq!(object.persistent_id, Some(7));
        assert_eq!(object.view_type.as_deref(), Some("Gui::Vendor"));
        assert_eq!(object.attributes["Vendor"], "opaque-extra");
        let wire = serde_json::to_value(object).unwrap();
        assert_eq!(wire["name"], name);
        assert_eq!(wire["type_name"], type_name);
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(captures.len(), 2);
        let capture = captures
            .iter()
            .find(|capture| capture.id() == object.id().as_str())
            .unwrap();
        assert_eq!(
            capture.offset(),
            u64_from_index(bytes.find(&declaration).unwrap())
        );
        assert_eq!(capture.byte_len(), u64_from_index(declaration.len()));
        let declaration_start = bytes.find(&declaration).unwrap();
        let data_start = bytes.find(&data).unwrap();
        let data_name_start = data_start + "<Object name='".len();
        let mut expected = Vec::new();
        for (key, raw) in [
            ("name", raw_name),
            ("type", type_name),
            ("id", "7"),
            ("ViewType", "Gui::Vendor"),
            ("Vendor", "opaque-extra"),
        ] {
            expected.extend(
                attribute_reads(&declaration, key, raw)
                    .into_iter()
                    .map(|range| {
                        range.start + u64_from_index(declaration_start)
                            ..range.end + u64_from_index(declaration_start)
                    }),
            );
        }
        expected.extend(
            attribute_reads(&data, "name", data_name)
                .into_iter()
                .map(|range| {
                    range.start + u64_from_index(data_start)..range.end + u64_from_index(data_start)
                }),
        );
        assert_eq!(capture.reads()["file"], merge_ranges(expected));
        assert!(u64_from_index(data_name_start) >= capture.offset() + capture.byte_len());
        let property = &graph.properties[0];
        let property_capture = captures
            .iter()
            .find(|capture| capture.id() == property.id)
            .unwrap();
        let mut expected = identity_ranges(&bytes, "Length", "App::PropertyFloat");
        expected.extend(attribute_reads(&bytes, "value", "3.5"));
        assert_eq!(property_capture.reads()["file"], merge_ranges(expected));
        assert_eq!(
            graph.object_sources[0].1,
            xml.find(&data).unwrap()..xml.find(&data).unwrap() + data.len()
        );
    }
}

#[test]
fn matched_object_data_order_does_not_replace_declaration_order_or_identity() {
    let xml = "<Document SchemaVersion='4'><Objects Count='2'><Object name='B' type='App::Feature'/><Object name='A' type='App::Feature'/></Objects><ObjectData Count='2'><Object name='A'><Properties Count='0'/></Object><Object name='B'><Properties Count='0'/></Object></ObjectData></Document>";
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, view) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
    let dom = roxmltree::Document::parse(xml).unwrap();
    let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
    assert_eq!(
        graph
            .objects
            .iter()
            .map(|object| object.name().as_str())
            .collect::<Vec<_>>(),
        ["B", "A"]
    );
    assert_eq!(
        graph
            .objects
            .iter()
            .map(|object| object.order)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 2);
    for object in &graph.objects {
        let capture = captures
            .iter()
            .find(|capture| capture.id() == object.id().as_str())
            .unwrap();
        let declaration = format!("<Object name='{}' type='App::Feature'/>", object.name());
        assert_eq!(
            capture.offset(),
            u64_from_index(xml.find(&declaration).unwrap())
        );
        assert_eq!(capture.byte_len(), u64_from_index(declaration.len()));
        let name_start = xml.rfind(&format!("name='{}'", object.name())).unwrap() + "name='".len();
        assert!(capture.reads()["file"]
            .iter()
            .any(|range| range.start <= u64_from_index(name_start)
                && range.end >= u64_from_index(name_start + object.name().len())));
    }
}

#[test]
fn mismatched_object_data_rejects_the_graph_and_discards_partial_source_captures() {
    let xml = "<Document SchemaVersion='4'><Objects Count='1'><Object name='A' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='B'><Properties Count='0'/></Object></ObjectData></Document>";
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, view) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
    let dom = roxmltree::Document::parse(xml).unwrap();
    let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects)
        .expect("the unmatched data part is bounded");
    assert_eq!(graph.objects.len(), 1);
    assert_eq!(graph.objects[0].name(), "A");
    assert!(graph.object_sources.is_empty());
    assert!(graph.properties.is_empty());
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), graph.objects[0].id());
    let losses: Vec<_> = graph
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(losses[0].message.contains("freecad.object_data"));
    let data = "<Object name='B'><Properties Count='0'/></Object>";
    assert!(losses[0]
        .message
        .contains(&format!("offset={}", xml.find(data).unwrap())));
    assert!(losses[0]
        .message
        .contains(&format!("length={}", data.len())));
}
