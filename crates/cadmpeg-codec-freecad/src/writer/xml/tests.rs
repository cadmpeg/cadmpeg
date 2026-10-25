// SPDX-License-Identifier: Apache-2.0

use super::Object;

#[test]
fn unknown_emission_is_not_an_empty_witness_and_generic_mutation_invalidates_it() {
    let mut object = Object::new("Group".into(), "App::DocumentObjectGroup", "Group");
    assert!(!object.geometry_empty());
    object.mark_grouping_only();
    assert!(object.geometry_empty());
    object.set_text("Label", "Renamed");
    object.set_boolean("Suppressed", false);
    object.set_projection_text("CadmpegFeatureOrdinal", "0");
    object.set_projection_boolean("CadmpegHasFeatureSourceTag", false);
    object.set_projection_texts("CadmpegParameterOrdinals", &["length=0".into()]);
    assert!(object.geometry_empty());
    object.set("Shape", "independently emitted property".into());
    assert!(!object.geometry_empty());
    object.mark_geometry_written();
    assert!(!object.geometry_empty());
}

#[test]
fn changed_runtime_or_extension_and_mutable_property_access_invalidate_grouping_proof() {
    let group = Object::new("Group".into(), "App::DocumentObjectGroup", "Group");
    let mut changed = group.clone();
    changed.mark_grouping_only();
    changed.kind = "Part::Box".into();
    assert!(!changed.geometry_empty());
    let mut extended = group.clone();
    extended.mark_grouping_only();
    extended.extensions.push("independent extension".into());
    assert!(!extended.geometry_empty());
    let mut edited = group;
    edited.mark_grouping_only();
    edited
        .properties
        .get_mut("Label")
        .unwrap()
        .push_str(" edit");
    assert!(!edited.geometry_empty());
    let mut bypassed = edited;
    bypassed.mark_grouping_only();
    bypassed
        .properties
        .insert("Shape".into(), "new geometry".into());
    assert!(!bypassed.geometry_empty());
}

#[test]
fn partial_link_list_recounts_held_members_and_preserves_authored_null() {
    use crate::native::{PropertyBody, PropertyFamily, PropertyRecord, ValueRecord, ValueSlots};
    use std::collections::BTreeMap;
    let value = |tag: &str, order, parent, attributes| ValueRecord {
        item_index: None,
        link_index: (tag == "Link").then(|| order - 1),
        tag: tag.into(),
        order,
        parent,
        attributes,
        decoded_json: None,
        text: None,
    };
    let mut property = PropertyRecord {
        id: "partial-links".into(),
        owner: "owner".into(),
        name: "Group".into(),
        type_name: "App::PropertyLinkList".into(),
        family: PropertyFamily::Link,
        status: None,
        dynamic: None,
        order: 0,
        body: Some(PropertyBody::Persisted {
            values: ValueSlots::new(
                vec![
                    value(
                        "LinkList",
                        0,
                        None,
                        BTreeMap::from([("count".into(), "3".into())]),
                    ),
                    value(
                        "Link",
                        1,
                        Some(0),
                        BTreeMap::from([("value".into(), "".into())]),
                    ),
                    value(
                        "Link",
                        3,
                        Some(0),
                        BTreeMap::from([("value".into(), "B".into())]),
                    ),
                ],
                4,
            )
            .unwrap(),
            links: vec![
                None,
                None,
                crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
                    document: None,
                    document_attribute: None,
                    object: Some(crate::native::native_id("object", "B")),
                    subelements: vec![],
                })
                .unwrap(),
            ],
            side_entries: vec![],
        }),
    };
    assert!(super::has_omitted_value_parts(&property));
    let xml = super::native(&property).unwrap().unwrap();
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let list = parsed.root_element().first_element_child().unwrap();
    assert_eq!(list.attribute("count"), Some("2"));
    assert_eq!(list.children().filter(|node| node.is_element()).count(), 2);
    assert_eq!(
        list.first_element_child().unwrap().attribute("value"),
        Some("")
    );
    super::validate(&property).unwrap();
    if let Some(PropertyBody::Persisted { values, .. }) = &mut property.body {
        let mut held = values.to_vec();
        held[2].order = 2;
        for value in &mut held {
            value.link_index = None;
        }
        held[0].attributes.insert("count".into(), "2".into());
        *values = held.into();
    }
    if let Some(PropertyBody::Persisted { links, .. }) = &mut property.body {
        links.remove(1);
    }
    assert!(!super::has_omitted_value_parts(&property));
    super::validate(&property).unwrap();
}

#[test]
fn identity_only_unknown_properties_are_omitted_while_registered_bodyless_values_are_writable() {
    use crate::native::{PropertyBody, PropertyFamily, PropertyRecord};
    let mut property = PropertyRecord {
        id: crate::native::native_id("property", "Opaque"),
        owner: crate::native::native_id("document", "0"),
        name: "Opaque".into(),
        type_name: "Vendor::Unregistered".into(),
        family: PropertyFamily::Unknown,
        status: None,
        dynamic: None,
        body: Some(PropertyBody::Persisted {
            values: Vec::new().into(),
            links: Vec::new(),
            side_entries: Vec::new(),
        }),
        order: 0,
    };
    assert!(super::native(&property).unwrap().is_none());
    for type_name in ["Part::PropertyShapeHistory", "Part::PropertyShapeCache"] {
        property.type_name = type_name.into();
        let xml = super::native(&property).unwrap().unwrap();
        assert!(xml.contains(type_name));
        assert!(!xml.contains("<Value"));
    }
}
