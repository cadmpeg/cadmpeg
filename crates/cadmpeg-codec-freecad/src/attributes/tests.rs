// SPDX-License-Identifier: Apache-2.0
//! Native enumeration choices do not compete with the selected neutral value.

use super::*;
use crate::native::{PropertyFamily, RetainedXml, ValueRecord};
use std::collections::BTreeMap;

#[test]
fn enumeration_projects_selected_index_and_preserves_native_choices() {
    let value = |tag: &str, parent, scalar: &str| ValueRecord {
        tag: tag.into(),
        parent,
        order: 0,
        attributes: BTreeMap::from([("value".into(), scalar.into())]),
        decoded_json: None,
        text: None,
        raw_xml: String::new(),
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
        id: "source:ScaleType".into(),
        owner: "source:View".into(),
        name: "ScaleType".into(),
        type_name: "App::PropertyEnumeration".into(),
        family: PropertyFamily::Enumeration,
        status: None,
        order: 0,
        body: PropertyBody::Persisted {
            values: records,
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        xml: RetainedXml::from_text("<Property/>".into(), 0).unwrap(),
    };
    let before = property.clone();
    assert_eq!(semantic_text(&property).as_deref(), Some("1"));
    assert_eq!(property, before);
    if let PropertyBody::Persisted { values, .. } = &mut property.body {
        values[2].parent = None;
    }
    assert_eq!(
        semantic_text(&property),
        None,
        "an unexpected root Enum is not grammar metadata"
    );
}

#[test]
fn complex_links_reference_their_complete_ordered_typed_property() {
    let owner = "fcstd:native:object#Owner";
    let link = |document: Option<&str>, object: &str, subelements: &[&str]| {
        crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
            document: document.map(str::to_owned),
            document_attribute: document.map(|_| "file".into()),
            object: Some(object.into()),
            subelements: subelements.iter().map(|value| (*value).into()).collect(),
        })
        .unwrap()
    };
    let property = PropertyRecord {
        id: "fcstd:native:property#Owner:Support".into(),
        owner: owner.into(),
        name: "Support".into(),
        type_name: "App::PropertyXLinkSubList".into(),
        family: PropertyFamily::Link,
        status: None,
        order: 0,
        body: PropertyBody::Persisted {
            values: Vec::new(),
            links: vec![
                link(None, "fcstd:native:object#Target", &["Face3", "Face1"]),
                None,
                link(Some("linked.FCStd"), "External", &["Edge9"]),
            ],
            side_entries: Vec::new(),
            dynamic: None,
        },
        xml: RetainedXml::from_text("<Property/>".into(), 0).unwrap(),
    };
    let object = ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            owner.into(),
            "Owner".into(),
        )
        .unwrap(),
        type_name: "PartDesign::ShapeBinder".into(),
        persistent_id: None,
        view_type: None,
        attributes: BTreeMap::new(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let mut ir = CadIr::empty();
    ir.native
        .namespace_mut("fcstd")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "properties",
            std::slice::from_ref(&property),
        )
        .unwrap();
    assert!(
        transfer(&mut ir, &[object], std::slice::from_ref(&property))
            .unwrap()
            .is_empty()
    );
    assert_eq!(ir.model.attributes.len(), 1);
    assert_eq!(
        ir.model.attributes[0].target,
        AttributeTarget::Source {
            source_id: owner.into()
        }
    );
    assert_eq!(
        ir.model.attributes[0].values,
        vec![AttributeValue::Reference(property.id.clone())]
    );
    let stored = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<PropertyRecord>("properties")
        .unwrap();
    assert_eq!(
        stored[0].links(),
        property.links(),
        "external names, selectors, null targets and their order remain typed content"
    );
    assert_eq!(
        serde_json::to_value(&stored[0]).unwrap(),
        serde_json::to_value(&property).unwrap()
    );
}
