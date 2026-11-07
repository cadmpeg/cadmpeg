// SPDX-License-Identifier: Apache-2.0
//! Native enumeration choices do not compete with the selected neutral value.

use super::*;
use crate::native::{PropertyFamily, ValueRecord};
use std::collections::BTreeMap;

#[test]
fn standalone_extrusion_lengths_belong_to_the_extent() {
    use cadmpeg_ir::{Codec, DecodeOptions};
    let xml = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="3"><Object name="Profile" type="Part::Feature"/><Object name="Extrusion" type="Part::Extrusion"/><Object name="Other" type="Part::Feature"/></Objects><ObjectData Count="3"><Object name="Profile"><Properties Count="0"/></Object><Object name="Extrusion"><Properties Count="4"><Property name="Base" type="App::PropertyLink"><Link value="Profile"/></Property><Property name="Dir" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property><Property name="LengthFwd" type="App::PropertyLength"><Float value="5"/></Property><Property name="LengthRev" type="App::PropertyLength"><Float value="2"/></Property></Properties></Object><Object name="Other"><Properties Count="1"><Property name="LengthFwd" type="App::PropertyLength"><Float value="3"/></Property></Properties></Object></ObjectData></Document>"#;
    let decoded = crate::FcstdCodec
        .decode(
            &mut std::io::Cursor::new(crate::test_support::test_archive::archive(xml)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Extrusion"))
        .unwrap();
    assert!(matches!(
        feature.evaluation.definition().operation(),
        FeatureOperation::Extrude { .. }
    ));
    let attributes = &decoded.ir().model.attributes;
    assert!(!attributes
        .iter()
        .any(
            |attribute| attribute.native_ref.as_deref().is_some_and(|id| id
                .contains("Extrusion:LengthFwd")
                || id.contains("Extrusion:LengthRev"))
        ));
    assert!(attributes.iter().any(|attribute| attribute
        .native_ref
        .as_deref()
        .is_some_and(|id| id.contains("Other:LengthFwd"))));
}

#[test]
fn enumeration_projects_selected_index_and_preserves_native_choices() {
    let value = |tag: &str, parent, scalar: &str| ValueRecord {
        tag: tag.into(),
        parent,
        order: 0,
        attributes: BTreeMap::from([("value".into(), scalar.into())]),
        decoded_json: None,
        text: None,
    };
    let mut records = vec![
        value("Integer", None, "1"),
        ValueRecord {
            attributes: BTreeMap::from([("count".into(), "2".into())]),
            ..value("CustomEnumList", None, "")
        },
        value("Enum", Some(1), "Page"),
        value("Enum", Some(1), "Custom"),
    ];
    for (index, value) in records.iter_mut().enumerate() {
        value.order = index;
    }
    let mut property = PropertyRecord {
        dynamic: None,
        id: "source:ScaleType".into(),
        owner: "source:View".into(),
        name: "ScaleType".into(),
        type_name: "App::PropertyEnumeration".into(),
        family: PropertyFamily::Enumeration,
        status: None,
        order: 0,
        body: Some(PropertyBody::Persisted {
            values: records,
            links: Vec::new(),
            side_entries: Vec::new(),
        }),
    };
    let before = property.clone();
    assert_eq!(semantic_text(&property).as_deref(), Some("1"));
    assert_eq!(property, before);
    if let Some(PropertyBody::Persisted { values, .. }) = &mut property.body {
        values[2].parent = None;
    }
    assert_eq!(
        semantic_text(&property),
        None,
        "an unexpected root Enum is not grammar metadata"
    );
}

#[test]
fn empty_lists_project_as_empty_attributes_only_when_their_grammar_matches() {
    for (kind, tag, key, held) in [
        ("App::PropertyStringList", "StringList", "count", "0"),
        (
            "TechDraw::PropertyCenterLineList",
            "CenterLineList",
            "count",
            "0",
        ),
        ("App::PropertyBoolList", "BoolList", "value", ""),
    ] {
        let mut property = PropertyRecord {
            dynamic: None,
            id: "source:empty".into(),
            owner: "source:owner".into(),
            name: "empty".into(),
            type_name: kind.into(),
            family: PropertyFamily::List,
            status: None,
            order: 0,
            body: Some(PropertyBody::Persisted {
                values: vec![ValueRecord {
                    tag: tag.into(),
                    parent: None,
                    order: 0,
                    attributes: BTreeMap::from([(key.into(), held.into())]),
                    decoded_json: None,
                    text: None,
                }],
                links: Vec::new(),
                side_entries: Vec::new(),
            }),
        };
        assert_eq!(values(&property), Some(Vec::new()));
        if kind == "App::PropertyBoolList" {
            let Some(PropertyBody::Persisted { values: rows, .. }) = &mut property.body else {
                unreachable!()
            };
            rows[0].attributes.insert("value".into(), "01".into());
            assert_eq!(
                values(&property),
                Some(vec![
                    Some(AttributeValue::Boolean(false)),
                    Some(AttributeValue::Boolean(true))
                ])
            );
            let Some(PropertyBody::Persisted { values: rows, .. }) = &mut property.body else {
                unreachable!()
            };
            rows[0].attributes.insert("value".into(), "2".into());
            assert_eq!(values(&property), None);
        }
        property.type_name = "Vendor::Other".into();
        assert_eq!(values(&property), None);
    }
}

#[test]
fn unconsumed_expression_engine_projects_each_authored_expression() {
    let row = |tag: &str, parent, attributes: &[(&str, &str)]| ValueRecord {
        tag: tag.into(),
        parent,
        order: usize::from(parent.is_some()),
        attributes: attributes
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
        decoded_json: None,
        text: None,
    };
    let mut property = PropertyRecord {
        dynamic: None,
        id: "source:engine".into(),
        owner: "source:owner".into(),
        name: "ExpressionEngine".into(),
        type_name: "App::PropertyExpressionEngine".into(),
        family: PropertyFamily::Expression,
        status: None,
        order: 0,
        body: Some(PropertyBody::Persisted {
            values: vec![
                row("ExpressionEngine", None, &[("count", "1")]),
                row(
                    "Expression",
                    Some(0),
                    &[
                        ("path", "Length"),
                        ("expression", "Sheet.Width * 2"),
                        ("comment", "note"),
                    ],
                ),
            ],
            links: Vec::new(),
            side_entries: Vec::new(),
        }),
    };
    let Some(Some(AttributeValue::Expression(expression))) =
        values(&property).and_then(|mut rows| rows.pop())
    else {
        panic!("expression row must project")
    };
    assert_eq!(expression.path, "Length");
    assert_eq!(
        expression.expression,
        cadmpeg_ir::features::Expression::text("Sheet.Width * 2")
    );
    assert_eq!(expression.comment.as_deref(), Some("note"));
    let Some(PropertyBody::Persisted { values: rows, .. }) = &mut property.body else {
        unreachable!()
    };
    rows[0].attributes.insert("count".into(), "2".into());
    assert_eq!(values(&property), None);
}
