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
    assert_eq!(captures.len(), 1);
    let mut expected = identity_ranges(&xml, "Label", "App::PropertyString");
    expected.extend(attribute_reads(&xml, "value", "Ω &amp; β"));
    expected.extend(attribute_reads(&xml, "extra", "uninterpreted"));
    assert_eq!(captures[0].reads()["file"], merge_ranges(expected));
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
        expected.push(value_tag_read(&xml, "String"));
        assert_eq!(captures[0].reads()["file"], merge_ranges(expected));
    }
}

#[test]
fn unsupported_string_carriers_and_opaque_types_do_not_receive_string_read_credit() {
    for (case, property) in [
        "<Property name='P' type='App::PropertyString'><String><![CDATA[opaque]]></String></Property>",
        "<Property name='P' type='App::PropertyString'><String>first<![CDATA[second]]>last</String></Property>",
        "<Property name='P' type='App::PropertyString'><String value='first'/><String value='second'/></Property>",
        "<Property name='P' type='App::PropertyString'><String value='outer'><String value='inner'/></String></Property>",
        "<Property name='P' type='Vendor::PropertyString'><String value='opaque'/></Property>",
        "<Property name='P' type='App::PropertyPythonObject'><Python value='opaque'/></Property>",
    ].into_iter().enumerate() {
        let xml = document(property, 1);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, view) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).unwrap();
        let dom = roxmltree::Document::parse(&xml).unwrap();
        let graph = parse_document_source(&ctx, view, &dom, Vocabulary::Objects).unwrap();
        // CDATA is valid String content; opaque/unknown payloads remain identity-only (#214/#224).
        let property = &graph.properties[0];
        assert_eq!(property.values().len(), usize::from(case != 4));
        let mut expected = identity_ranges(&xml, "P", &property.type_name);
        match case {
            0 | 1 => {
                expected.push(value_tag_read(&xml, "String"));
                let chunks: &[&str] = if case == 0 { &["opaque"] } else { &["first", "second", "last"] };
                for chunk in chunks {
                    let start = xml.find(chunk).unwrap();
                    expected.push(u64_from_index(start)..u64_from_index(start + chunk.len()));
                }
                assert_eq!(property.values()[0].text.as_deref(), Some(if case == 0 { "opaque" } else { "firstsecondlast" }));
            }
            2 | 3 => {
                let value = if case == 2 { "first" } else { "outer" };
                expected.extend(attribute_reads(&xml, "value", value));
                assert_eq!(property.values()[0].attributes["value"], value);
            }
            4 => assert!(property.values().is_empty()),
            5 => { expected.push(value_tag_read(&xml, "Python")); assert!(property.values()[0].attributes.is_empty()); assert!(property.values()[0].text.is_none()); }
            _ => unreachable!(),
        }
        assert_eq!(graph.losses.iter().filter(|loss| loss.code.local_code() == "record.unreadable").count(), usize::from(matches!(case, 2 | 3)));
        let captures = property_captures(&ctx, &graph);
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].reads()["file"], merge_ranges(expected), "{property:?}");
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
        merge_ranges(
            [
                identity_ranges(&xml, "P", "App::PropertyString"),
                attribute_reads(&xml, "value", "")
            ]
            .concat()
        )
    );
}
