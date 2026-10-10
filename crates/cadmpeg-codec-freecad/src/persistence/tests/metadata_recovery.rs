// SPDX-License-Identifier: Apache-2.0
//! Independent metadata admission and bounded property indexing.

use crate::native::PropertyBody;
use std::fmt::Write as _;

#[test]
fn missing_and_legacy_counts_retain_the_actual_population_and_charge_diagnostics() {
    for (schema, declarations, data, record) in [
        ("4", "Objects", "ObjectData", "Object"),
        ("2", "Features", "FeatureData", "Feature"),
    ] {
        let xml = format!(
            r#"<Document SchemaVersion="{schema}"><{declarations}><{record} name="A" type="App::Feature"/></{declarations}><{data} Count="0"><{record} name="A"><Properties Count="0"/></{record}></{data}></Document>"#
        );
        let graph = crate::test_support::with_service_context(xml.as_bytes(), |ctx| {
            super::super::parse_with_context(xml.as_bytes(), schema, ctx)
        })
        .unwrap();
        assert_eq!(graph.objects.len(), 1);
        assert_eq!(graph.losses.len(), 2);
        crate::test_support::assert_retained_refusal_at(
            xml.as_bytes(),
            "FCStd persistence count diagnostic",
            |ctx| super::super::parse_with_context(xml.as_bytes(), schema, ctx),
        );
    }
}

#[test]
fn redundant_counts_do_not_own_xml_population() {
    for declared in ["0", "99", "bad", "999999999999999999999999999999"] {
        let document = format!(
            r#"<Document SchemaVersion="4"><Properties Count="{declared}" TransientCount="{declared}"><Property name="Note" type="App::PropertyString"><String value="note"/></Property><_Property name="Temporary" type="App::PropertyString"/></Properties><Objects Count="{declared}"><Object type="Part::Feature" name="Body"/></Objects><ObjectData Count="{declared}"><Object name="Body"><Properties Count="0"/></Object></ObjectData></Document>"#
        );
        let graph = crate::test_support::with_service_context(document.as_bytes(), |ctx| {
            super::super::parse_with_context(document.as_bytes(), "4", ctx)
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
            super::super::parse_with_context(document.as_bytes(), "4", ctx)
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
            super::super::parse_with_context(document.as_bytes(), "4", ctx)
        })
        .expect("source-only link");
        assert!(matches!(
            graph.properties[0].body,
            PropertyBody::Unreadable(_)
        ));
        assert!(graph.properties[0].links().is_empty());
        assert!(graph.properties[0].values().is_empty());
        assert!(graph.properties[0].xml.text().contains(payload));
        assert!(graph.losses.iter().any(|loss| loss.code
            == crate::loss::FreecadLossCode::PersistencePayloadUnresolved
                .note(String::new())
                .code));
        let wire = serde_json::to_value(&graph.properties[0]).expect("serialize");
        let property: crate::native::PropertyRecord =
            serde_json::from_value(wire).expect("deserialize source-only property");
        assert_eq!(property, graph.properties[0]);
    }
}

#[test]
fn many_unique_object_names_fit_a_linear_work_budget() {
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 250_000_000;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(document.as_bytes(), &arena, &policy)
            .unwrap();
    let graph = super::super::parse_with_context(document.as_bytes(), "4", &ctx).unwrap();
    assert_eq!(graph.objects.len(), count);
    assert!(graph.objects.iter().all(|object| object.data.is_some()));
}

#[test]
fn many_unique_properties_fit_a_linear_work_budget() {
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 250_000_000;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(document.as_bytes(), &arena, &policy)
            .expect("root");
    let graph = super::super::parse_with_context(document.as_bytes(), "4", &ctx)
        .expect("unique-name index stays bounded");
    assert_eq!(graph.properties.len(), count);
}
