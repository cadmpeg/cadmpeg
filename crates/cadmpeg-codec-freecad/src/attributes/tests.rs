// SPDX-License-Identifier: Apache-2.0
//! Native enumeration choices do not compete with the selected neutral value.

use super::*;
use crate::native::{PropertyFamily, ValueRecord};
use std::collections::BTreeMap;

#[test]
fn enumeration_projects_selected_index_and_preserves_native_choices() {
    let value = |tag: &str, parent, scalar: &str| ValueRecord {
        item_index: None,
        link_index: None,
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
            item_index: None,
            link_index: None,
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
            values: records.into(),
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

fn property(kind: &str, records: Vec<ValueRecord>) -> PropertyRecord {
    PropertyRecord {
        dynamic: None,
        id: crate::native::native_child_id(
            "property",
            &crate::native::native_id("document", "0"),
            "Extra",
        ),
        owner: crate::native::native_id("document", "0"),
        name: "Extra".into(),
        type_name: kind.into(),
        family: PropertyFamily::List,
        status: None,
        order: 0,
        body: Some(PropertyBody::Persisted {
            values: records.into(),
            links: Vec::new(),
            side_entries: Vec::new(),
        }),
    }
}

fn value(tag: &str, order: usize, parent: Option<usize>, fields: &[(&str, &str)]) -> ValueRecord {
    ValueRecord {
        item_index: None,
        link_index: None,
        tag: tag.into(),
        order,
        parent,
        attributes: fields
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
        decoded_json: None,
        text: None,
    }
}

#[test]
fn counted_attribute_projection_preserves_empty_lists_and_refuses_missing_positions() {
    let empty = property(
        "App::PropertyStringList",
        vec![value("StringList", 0, None, &[("count", "0")])],
    );
    assert_eq!(values(&empty), Some(Vec::new()));
    let sparse = property(
        "App::PropertyStringList",
        vec![
            value("StringList", 0, None, &[("count", "3")]),
            value("String", 1, Some(0), &[("value", "first")]),
            value("String", 3, Some(0), &[("value", "last")]),
        ],
    );
    assert_eq!(values(&sparse), None);
    assert_eq!(
        serde_json::to_value(&sparse).unwrap()["values"][2],
        serde_json::Value::Null
    );
    let mut scalar = property(
        "App::PropertyFloat",
        vec![value("Float", 0, None, &[("value", "2.5")])],
    );
    if let Some(PropertyBody::Persisted { values, .. }) = &mut scalar.body {
        *values = crate::native::ValueSlots::new(values.to_vec(), 2).unwrap();
    }
    assert_eq!(
        values(&scalar),
        Some(vec![AttributeValue::float(2.5).unwrap()])
    );
}

#[test]
fn admitted_numeric_collections_project_their_semantic_values() {
    let integers = property(
        "App::PropertyIntegerList",
        vec![
            value("IntegerList", 0, None, &[("count", "2")]),
            value("I", 1, Some(0), &[("v", "-3")]),
            value("I", 2, Some(0), &[("v", "8")]),
        ],
    );
    assert_eq!(
        values(&integers),
        Some(vec![
            AttributeValue::Integer(-3),
            AttributeValue::Integer(8)
        ])
    );
    let booleans = property(
        "App::PropertyBoolList",
        vec![value("BoolList", 0, None, &[("value", "101")])],
    );
    assert_eq!(
        values(&booleans),
        Some(vec![
            AttributeValue::Boolean(true),
            AttributeValue::Boolean(false),
            AttributeValue::Boolean(true)
        ])
    );
    let rotation = property(
        "App::PropertyRotation",
        vec![value(
            "PropertyRotation",
            0,
            None,
            &[("Ox", "0"), ("Oy", "1"), ("Oz", "0"), ("A", "90")],
        )],
    );
    assert_eq!(
        values(&rotation),
        Some(vec![AttributeValue::vector([0.0, 1.0, 0.0, 90.0]).unwrap()])
    );
}

#[test]
fn root_property_source_ranges_do_not_suppress_extra_attributes_or_duplicate_existing_projection() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let xml = b"<Document><Properties Count='1'><Property name='Extra' type='App::PropertyString'><String value='authored'/></Property></Properties></Document>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::service()).unwrap();
    let source = crate::xml_framing::XmlSource::new(&ctx, view).unwrap();
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    let mut output =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let roots = crate::persistence::document_properties::decode_scanned(
        &ctx,
        &source,
        0,
        &context,
        &mut output,
    )
    .unwrap()
    .unwrap();
    let mut ir = CadIr::empty();
    let reader = crate::property_reader::PropertyReader::typed(&ctx);
    let bindings = crate::design::ExpressionBindings::default();
    // #224's named projections are born from actual original getters, so the
    // former invented 0..1 range is replaced by this exact authored carrier.
    assert!(transfer_source(
        &ctx,
        &mut ir,
        &[],
        &roots.properties,
        view,
        &roots.sources,
        &reader,
        &bindings
    )
    .unwrap()
    .0
    .is_empty());
    assert_eq!(ir.model.attributes.len(), 1);
    assert_eq!(ir.model.attributes[0].target, AttributeTarget::Document);
    assert_eq!(
        ir.model.attributes[0].values,
        vec![AttributeValue::String("authored".into())]
    );
    assert!(transfer_source(
        &ctx,
        &mut ir,
        &[],
        &roots.properties,
        view,
        &roots.sources,
        &reader,
        &bindings
    )
    .unwrap()
    .0
    .is_empty());
    assert_eq!(ir.model.attributes.len(), 1);
}

#[test]
fn structured_native_properties_keep_keys_and_expressions_without_string_projection() {
    for property in [
        property(
            "App::PropertyMap",
            vec![
                value("Map", 0, None, &[("count", "1")]),
                value(
                    "Item",
                    1,
                    Some(0),
                    &[("key", "authored key"), ("value", "authored value")],
                ),
            ],
        ),
        property(
            "App::PropertyExpressionEngine",
            vec![
                value("ExpressionEngine", 0, None, &[("count", "1")]),
                value(
                    "Expression",
                    1,
                    Some(0),
                    &[
                        ("path", "Length"),
                        ("expression", "Width * 2"),
                        ("comment", "authored comment"),
                    ],
                ),
            ],
        ),
    ] {
        let before = property.clone();
        // #2246376508279 requires typed Map/Expression projections while the
        // original canonical native fields remain unchanged.
        assert!(values(&property).is_some());
        let mut ir = CadIr::empty();
        assert!(transfer(&mut ir, &[], std::slice::from_ref(&property))
            .unwrap()
            .is_empty());
        assert_eq!(ir.model.attributes.len(), 1);
        assert!(matches!(
            ir.model.attributes[0].values.as_slice(),
            [AttributeValue::Map(_)] | [AttributeValue::Expression(_)]
        ));
        assert_eq!(property, before);
        assert_eq!(
            serde_json::from_value::<PropertyRecord>(serde_json::to_value(&property).unwrap())
                .unwrap(),
            before
        );
    }
}

#[test]
fn structured_property_native_fields_have_real_reads_in_the_property_owner() {
    use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::source_fidelity::parts::PartOutput;
    let xml = b"<Document><Properties Count='2'><Property name='Mapping' type='App::PropertyMap'><Map count='1'><Item key='authored-key' value='authored-value'/></Map></Property><Property name='Bindings' type='App::PropertyExpressionEngine'><ExpressionEngine count='1'><Expression path='Length' expression='Width * 2' comment='authored-comment'/></ExpressionEngine></Property></Properties></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "Document.xml").unwrap();
    let source = crate::xml_framing::XmlSource::new(&ctx, root).unwrap();
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let parsed = crate::persistence::document_properties::decode_scanned(
        &ctx,
        &source,
        0,
        &context,
        &mut output,
    )
    .unwrap()
    .unwrap();
    assert!(output.losses().is_empty());
    assert_eq!(parsed.properties.len(), 2);
    let captures = ctx.take_record_captures().unwrap();
    for (property, fields) in parsed.properties.iter().zip([
        vec!["authored-key", "authored-value"],
        vec!["Length", "Width * 2", "authored-comment"],
    ]) {
        assert!(values(property).is_some());
        let capture = captures
            .iter()
            .find(|capture| capture.id() == property.id)
            .unwrap();
        for text in fields {
            let start = xml
                .windows(text.len())
                .position(|bytes| bytes == text.as_bytes())
                .unwrap();
            let expected = u64_from_index(start)..u64_from_index(start + text.len());
            assert!(
                capture.reads()["Document.xml"]
                    .iter()
                    .any(|read| read.start <= expected.start && read.end >= expected.end),
                "{text}"
            );
        }
        let wire = serde_json::to_value(property).unwrap();
        assert_eq!(
            serde_json::from_value::<PropertyRecord>(wire).unwrap(),
            *property
        );
    }
}

#[test]
fn inert_json_scalars_project_without_reencoding_a_structured_value() {
    for (json, expected) in [
        (serde_json::json!(true), AttributeValue::Boolean(true)),
        (serde_json::json!(7), AttributeValue::Integer(7)),
        (serde_json::json!(2.5), AttributeValue::float(2.5).unwrap()),
        (
            serde_json::json!("authored"),
            AttributeValue::String("authored".into()),
        ),
    ] {
        let mut record = value("Python", 0, None, &[]);
        record.decoded_json = Some(json.into());
        let property = property("App::PropertyPythonObject", vec![record]);
        assert_eq!(values(&property), Some(vec![expected]));
    }
    for json in [
        serde_json::json!(null),
        serde_json::json!({"key":"value"}),
        serde_json::json!([1, 2]),
        serde_json::json!(u64::MAX),
    ] {
        let mut record = value("Python", 0, None, &[]);
        record.decoded_json = Some(json.into());
        assert_eq!(
            values(&property("App::PropertyPythonObject", vec![record])),
            None
        );
    }
}

#[test]
fn prospective_document_attribute_identity_requires_a_projectable_family_and_real_owner() {
    let mut scalar = property("App::PropertyFloat", Vec::new());
    scalar.body = None;
    let expected = crate::native::model_id("attribute", &scalar.id, "property");
    assert_eq!(
        document_property_identity_with_json_mode(&scalar, false),
        Some(expected)
    );
    for kind in [
        "App::PropertyPythonObject",
        "Vendor::PropertyFloat",
        "Part::PropertyPartShape",
    ] {
        scalar.type_name = kind.into();
        assert_eq!(
            document_property_identity_with_json_mode(&scalar, false),
            None,
            "{kind}"
        );
    }
    for kind in ["App::PropertyMap", "App::PropertyExpressionEngine"] {
        // These named families have a projection under #2246376508279;
        // actual failed children sponsor the prospective attribute identity.
        scalar.type_name = kind.into();
        assert_eq!(
            document_property_identity_with_json_mode(&scalar, false),
            Some(crate::native::model_id("attribute", &scalar.id, "property"))
        );
    }
    scalar.type_name = "App::PropertyFloat".into();
    scalar.owner = crate::native::native_id("object", "Other");
    assert_eq!(
        document_property_identity_with_json_mode(&scalar, false),
        None
    );
    scalar.owner = crate::native::native_id("document", "0");
    scalar.body = Some(PropertyBody::Transient);
    assert_eq!(
        document_property_identity_with_json_mode(&scalar, false),
        None
    );
}

#[test]
fn authored_empty_link_lists_project_without_inventing_an_authored_null_reference() {
    let empty = property(
        "App::PropertyLinkList",
        vec![value("LinkList", 0, None, &[("count", "0")])],
    );
    assert_eq!(values(&empty), Some(Vec::new()));
    let single_null = property(
        "App::PropertyLink",
        vec![value("Link", 0, None, &[("value", "")])],
    );
    assert_eq!(values(&single_null), None);
}
