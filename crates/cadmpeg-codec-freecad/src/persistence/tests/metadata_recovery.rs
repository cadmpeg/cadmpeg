// SPDX-License-Identifier: Apache-2.0
//! Independent metadata admission and bounded property indexing.

use std::fmt::Write as _;

#[test]
fn missing_and_legacy_counts_retain_the_actual_population_and_report_losses() {
    for (schema, declarations, data, record) in [
        ("4", "Objects", "ObjectData", "Object"),
        ("2", "Features", "FeatureData", "Feature"),
    ] {
        let xml = format!(
            r#"<Document SchemaVersion="{schema}"><{declarations}><{record} name="A" type="App::Feature"/></{declarations}><{data} Count="0"><{record} name="A"><Properties Count="0"/></{record}></{data}></Document>"#
        );
        let graph = crate::test_support::with_service_context(xml.as_bytes(), |ctx| {
            super::parse(xml.as_bytes(), schema, ctx)
        })
        .unwrap();
        assert_eq!(graph.objects.len(), 1);
        assert_eq!(graph.losses.len(), 2);
    }
}

#[test]
fn redundant_counts_do_not_own_xml_population() {
    for declared in ["0", "99", "bad", "999999999999999999999999999999"] {
        let document = format!(
            r#"<Document SchemaVersion="4"><Properties Count="{declared}" TransientCount="{declared}"><Property name="Note" type="App::PropertyString"><String value="note"/></Property><_Property name="Temporary" type="App::PropertyString"/></Properties><Objects Count="{declared}"><Object type="Part::Feature" name="Body"/></Objects><ObjectData Count="{declared}"><Object name="Body"><Properties Count="0"/></Object></ObjectData></Document>"#
        );
        let graph = crate::test_support::with_service_context(document.as_bytes(), |ctx| {
            super::parse(document.as_bytes(), "4", ctx)
        })
        .expect("actual framed counts");
        assert_eq!(graph.objects.len(), 1);
        assert_eq!(graph.properties.len(), 2);
        assert_eq!(graph.losses.len(), 4);
    }
}

#[test]
fn dependency_and_extension_counts_do_not_own_framed_records() {
    for count in [
        "",
        " Count=\"0\"",
        " Count=\"99\"",
        " Count=\"bad\"",
        " Count=\"999999999999999999999999999999\"",
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="A"{count}><Dep Name="B"/></ObjectDeps><ObjectDeps Name="B" Count="0"/><Object name="A" type="App::Feature"/><Object name="B" type="App::Feature"/></Objects><ObjectData Count="2"><Object name="A"><Extensions{count}><Extension name="Extra" type="Vendor::Extension"/></Extensions></Object><Object name="B"/></ObjectData></Document>"#
        );
        let graph = crate::test_support::with_service_context(document.as_bytes(), |ctx| {
            super::parse(document.as_bytes(), "4", ctx)
        })
        .expect("independently framed dependencies and extensions");
        assert_eq!(graph.objects.len(), 2);
        assert_eq!(graph.objects[0].dependencies, ["fcstd:native:object#B"]);
        assert_eq!(graph.extensions.len(), 1);
        assert_eq!(graph.extensions[0].name, "Extra");
        assert_eq!(graph.losses.len(), 2);
    }
}

#[test]
fn malformed_unused_links_remain_explicitly_source_only() {
    for payload in ["<String value=\"\"/>", "", "<Link Object=\"Body\"/>"] {
        let document = format!(
            r#"<Document SchemaVersion="4"><Properties Count="1"><Property name="Unused" type="App::PropertyLink">{payload}</Property></Properties><Objects Count="0"/><ObjectData Count="0"/></Document>"#
        );
        let graph = crate::test_support::with_service_context(document.as_bytes(), |ctx| {
            super::parse(document.as_bytes(), "4", ctx)
        })
        .expect("source-only link");
        let decoded = cadmpeg_ir::Codec::decode(
            &crate::FcstdCodec,
            &mut std::io::Cursor::new(crate::test_support::test_archive::archive(&document)),
            &cadmpeg_ir::DecodeOptions::default(),
        )
        .expect("source-only property with its fidelity");
        let level = if payload.is_empty() {
            "freecad.property"
        } else {
            "freecad.property_value"
        };
        let given_up: Vec<_> = decoded
            .source_fidelity()
            .parts()
            .iter()
            .filter(|(_, part)| {
                part.level().as_str() == level
                    && matches!(
                        part.state(),
                        cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                    )
            })
            .collect();
        assert_eq!(given_up.len(), 1);
        let (id, part) = given_up[0];
        let unreadable: Vec<_> = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .collect();
        assert_eq!(unreadable.len(), 1);
        assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
        assert!(unreadable[0].message.contains(level));
        let range = usize::try_from(part.offset()).unwrap()
            ..usize::try_from(part.offset() + part.byte_len()).unwrap();
        assert_eq!(
            crate::test_support::source_fidelity::bytes(decoded.source_fidelity(), id.as_str()),
            document[range].as_bytes()
        );
        if payload.is_empty() {
            assert!(graph.properties.is_empty());
        } else {
            let property = &graph.properties[0];
            assert!(property.body.is_none());
            assert!(property.links().is_empty());
            assert!(property.values().is_empty());
            assert!(
                document[graph.property_sources[&property.id].record_range()].contains(payload)
            );
            let wire = serde_json::to_value(property).expect("serialize");
            assert!(wire.get("payload_error").is_none());
            assert_eq!(
                serde_json::from_value::<crate::native::PropertyRecord>(wire)
                    .expect("deserialize source-only property"),
                *property
            );
        }
        assert!(graph
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "record.unreadable"
                && loss.message.contains(level)));
    }
}

#[test]
fn many_unique_object_names_decode() {
    let count = 16_000;
    let mut document = format!(r#"<Document SchemaVersion="4"><Objects Count="{count}">"#);
    for index in 0..count {
        write!(
            document,
            r#"<Object name="Object{index}" type="App::Feature"/>"#
        )
        .unwrap();
    }
    write!(document, r#"</Objects><ObjectData Count="{count}">"#).unwrap();
    for index in 0..count {
        write!(document, r#"<Object name="Object{index}"/>"#).unwrap();
    }
    document.push_str("</ObjectData></Document>");
    let graph = crate::test_support::with_service_context(document.as_bytes(), |ctx| {
        super::parse(document.as_bytes(), "4", ctx).unwrap()
    });
    assert_eq!(graph.objects.len(), count);
    assert!(graph
        .objects
        .iter()
        .all(|object| graph.object_sources.iter().any(|(id, _)| id == object.id())));
}

#[test]
fn many_unique_properties_decode() {
    let count = 16_000;
    let mut document = format!(r#"<Document SchemaVersion="4"><Properties Count="{count}">"#);
    for index in 0..count {
        write!(
            document,
            r#"<Property name="P{index}" type="App::PropertyString"><String value=""/></Property>"#
        )
        .unwrap();
    }
    document.push_str("</Properties><Objects Count=\"0\"/><ObjectData Count=\"0\"/></Document>");
    let graph = crate::test_support::with_service_context(document.as_bytes(), |ctx| {
        super::parse(document.as_bytes(), "4", ctx).expect("unique property names")
    });
    assert_eq!(graph.properties.len(), count);
}
