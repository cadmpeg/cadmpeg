// SPDX-License-Identifier: Apache-2.0
//! Typed references survive identity edits independently of source literal names.

use crate::native::{
    LinkTarget, LinkTargetWire, PropertyBody, PropertyFamily, PropertyRecord, ValueRecord,
    ValueSlots,
};
use std::collections::{BTreeMap, HashMap};

#[test]
fn sparse_reference_output_uses_current_targets_and_preserves_source_literals_and_nulls() {
    let value = |order, link_index, attrs: &[(&str, &str)]| ValueRecord {
        item_index: None,
        order,
        parent: (order != 0).then_some(0),
        tag: if order == 0 { "XLinkSubList" } else { "XLink" }.into(),
        attributes: attrs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect::<BTreeMap<_, _>>(),
        text: None,
        decoded_json: None,
        link_index,
    };
    let target = |document: Option<&str>, object: &str| {
        LinkTarget::optional_from_wire(LinkTargetWire {
            document: document.map(str::to_owned),
            document_attribute: Some("file".into()),
            object: Some(object.into()),
            subelements: vec![],
        })
        .unwrap()
    };
    let property = PropertyRecord {
        id: "property".into(),
        owner: "owner".into(),
        name: "Links".into(),
        type_name: "App::PropertyXLinkSubList".into(),
        family: PropertyFamily::Link,
        status: None,
        dynamic: None,
        order: 0,
        body: Some(PropertyBody::Persisted {
            values: ValueSlots::new(
                vec![
                    value(0, None, &[("count", "4")]),
                    value(1, Some(0), &[("name", "")]),
                    value(
                        3,
                        Some(2),
                        &[
                            ("name", "fcstd:native:object#literal-name"),
                            ("vendor", "kept"),
                        ],
                    ),
                    value(
                        4,
                        Some(3),
                        &[("name", "Remote"), ("file", "External.FCStd")],
                    ),
                ],
                5,
            )
            .unwrap(),
            links: vec![
                None,
                None,
                target(None, "arbitrary-current-id"),
                target(Some("External.FCStd"), "Remote"),
            ],
            side_entries: vec![],
        }),
    };
    let original = property.clone();
    super::super::validate(&property).unwrap();
    let xml = super::super::native_named(
        &property,
        &HashMap::from([("arbitrary-current-id".into(), "CurrentName".into())]),
    )
    .unwrap()
    .unwrap();
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let list = parsed.root_element().first_element_child().unwrap();
    assert_eq!(list.attribute("count"), Some("3"));
    let children: Vec<_> = list
        .children()
        .filter(roxmltree::Node::is_element)
        .collect();
    assert_eq!(children[0].attribute("name"), Some(""));
    assert_eq!(children[1].attribute("name"), Some("CurrentName"));
    assert_eq!(children[1].attribute("vendor"), Some("kept"));
    assert_eq!(children[2].attribute("name"), Some("Remote"));
    assert_eq!(children[2].attribute("file"), Some("External.FCStd"));
    assert_eq!(
        serde_json::to_value(&property).unwrap(),
        serde_json::to_value(original).unwrap()
    );
}
