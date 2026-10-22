// SPDX-License-Identifier: Apache-2.0
//! Kept string fields consume their own source values, without opaque lookalikes.

use super::*;

#[test]
fn a_recognized_string_attribute_reads_its_original_entities_and_keeps_the_decoded_value() {
    let xml = document("<Property name='Label' type='App::PropertyString'><String extra='uninterpreted' value='Ω &amp; β'/></Property>", 1);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, view) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
    let dom = roxmltree::Document::parse(&xml).unwrap();
    let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
    assert_eq!(graph.properties[0].values()[0].attributes["value"], "Ω & β");
    assert_eq!(
        graph.properties[0].values()[0].attributes["extra"],
        "uninterpreted"
    );
    let wire = serde_json::to_value(&graph.properties[0]).unwrap();
    assert_eq!(wire["values"][0]["attributes"]["value"], "Ω & β");
    let captures = property_captures(&ctx, &graph);
    let start = xml.find("Ω &amp; β").unwrap();
    assert_eq!(captures.len(), 1);
    let mut expected = identity_ranges(&xml, "Label", "App::PropertyString");
    expected.push(u64_from_index(start)..u64_from_index(start + "Ω &amp; β".len()));
    assert_eq!(captures[0].reads()["file"], expected);
}

#[test]
fn direct_string_text_reads_exact_original_text_and_its_changes_feed_the_kept_field() {
    for (raw, decoded) in [
        (" Ω &amp; β ", " Ω & β "),
        (" new &amp; text ", " new & text "),
    ] {
        let xml = document(&format!("<Property name='Label' type='App::PropertyString'><String>{raw}</String></Property>"), 1);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, view) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
        let dom = roxmltree::Document::parse(&xml).unwrap();
        let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
        assert_eq!(
            graph.properties[0].values()[0].text.as_deref(),
            Some(decoded)
        );
        let wire = serde_json::to_value(&graph.properties[0]).unwrap();
        assert_eq!(wire["values"][0]["text"], decoded);
        let captures = property_captures(&ctx, &graph);
        let start = xml.find(raw).unwrap();
        assert_eq!(captures.len(), 1);
        let mut expected = identity_ranges(&xml, "Label", "App::PropertyString");
        expected.push(u64_from_index(start)..u64_from_index(start + raw.len()));
        assert_eq!(captures[0].reads()["file"], expected);
    }
}

#[test]
fn unsupported_string_carriers_and_opaque_types_do_not_receive_string_read_credit() {
    for property in [
        "<Property name='P' type='App::PropertyString'><String><![CDATA[opaque]]></String></Property>",
        "<Property name='P' type='App::PropertyString'><String>first<![CDATA[second]]>last</String></Property>",
        "<Property name='P' type='App::PropertyString'><String value='first'/><String value='second'/></Property>",
        "<Property name='P' type='App::PropertyString'><String value='outer'><String value='inner'/></String></Property>",
        "<Property name='P' type='Vendor::PropertyString'><String value='opaque'/></Property>",
        "<Property name='P' type='App::PropertyPythonObject'><Python value='opaque'/></Property>",
    ] {
        let xml = document(property, 1);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, view) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
        let dom = roxmltree::Document::parse(&xml).unwrap();
        let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
        assert!(!graph.properties[0].values().is_empty());
        let captures = property_captures(&ctx, &graph);
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].reads()["file"], identity_ranges(&xml, "P", &graph.properties[0].type_name), "{property}");
    }
}

#[test]
fn an_empty_string_value_remains_present_without_inventing_an_empty_read_interval() {
    let xml = document(
        "<Property name='P' type='App::PropertyString'><String value=''/></Property>",
        1,
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, view) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
    let dom = roxmltree::Document::parse(&xml).unwrap();
    let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
    assert_eq!(graph.properties[0].values()[0].attributes["value"], "");
    let captures = property_captures(&ctx, &graph);
    assert_eq!(captures.len(), 1);
    assert_eq!(
        captures[0].reads()["file"],
        identity_ranges(&xml, "P", "App::PropertyString")
    );
}
