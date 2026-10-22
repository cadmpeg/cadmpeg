// SPDX-License-Identifier: Apache-2.0
//! Kept property identity and status fields use their actual source producer.

use super::*;

#[test]
fn entity_spelled_property_identity_and_status_flow_into_kept_fields_in_a_nonzero_window() {
    for (raw_name, name, raw_status, status) in
        [("A&amp;B", "A&B", "007", 7), ("A&amp;C", "A&C", "008", 8)]
    {
        let xml = document(&format!("<Property name='{raw_name}' type='Vendor::Futu&#x72;e' status='{raw_status}'><String value='opaque-state'/></Property>"), 1);
        let bytes = format!("prefix{xml}suffix");
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes.as_bytes(), &arena, &policy).unwrap();
        let view = root.child(6, 6 + xml.len()).unwrap();
        let dom = roxmltree::Document::parse(std::str::from_utf8(view.window()).unwrap()).unwrap();
        let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
        let property = &graph.properties[0];
        assert_eq!(property.name, name);
        assert_eq!(property.type_name, "Vendor::Future");
        assert_eq!(property.status, Some(status));
        assert_eq!(
            property.id,
            crate::native::native_child_id(
                "property",
                &crate::native::native_id("object", "Owner"),
                name
            )
        );
        let wire = serde_json::to_value(property).unwrap();
        assert_eq!(wire["name"], name);
        assert_eq!(wire["type_name"], "Vendor::Future");
        assert_eq!(wire["status"], status);
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].id(), property.id);
        assert_eq!(
            captures[0].reads()["file"],
            [
                attribute_range(&bytes, "name", raw_name),
                attribute_range(&bytes, "type", "Vendor::Futu&#x72;e"),
                attribute_range(&bytes, "status", raw_status)
            ]
        );
    }
}

#[test]
fn invalid_status_and_unreadable_payload_bytes_do_not_gain_identity_read_credit() {
    for raw in ["bad", "-1", "18446744073709551616"] {
        let xml = document(&format!("<Property name='Invalid' type='App::PropertyBool' status='{raw}'><Bool value='maybe'/></Property>"), 1);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, view) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
        let dom = roxmltree::Document::parse(&xml).unwrap();
        let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
        assert_eq!(graph.properties[0].status, None);
        assert!(matches!(
            graph.properties[0].body,
            crate::native::PropertyBody::Unreadable(_)
        ));
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(captures.len(), 1);
        assert_eq!(
            captures[0].reads()["file"],
            identity_ranges(&xml, "Invalid", "App::PropertyBool")
        );
    }
}

#[test]
fn a_transient_declaration_reads_its_kept_identity_and_status_without_a_payload() {
    let xml = "<Document SchemaVersion='4'><Objects Count='0'/><ObjectData Count='0'/><Properties Count='0' TransientCount='1'><_Property name='Preview' type='App::PropertyString' status='152'/></Properties></Document>";
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, view) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
    let dom = roxmltree::Document::parse(xml).unwrap();
    let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
    let property = &graph.properties[0];
    assert_eq!(property.name, "Preview");
    assert_eq!(property.type_name, "App::PropertyString");
    assert_eq!(property.status, Some(152));
    assert!(property.is_transient());
    assert!(property.values().is_empty());
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    let mut expected = identity_ranges(xml, "Preview", "App::PropertyString");
    expected.push(attribute_range(xml, "status", "152"));
    assert_eq!(captures[0].reads()["file"], expected);
}
