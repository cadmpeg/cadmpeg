// SPDX-License-Identifier: Apache-2.0
//! Native enumeration choices do not compete with the selected neutral value.

use super::*;
use crate::native::{PropertyFamily, ValueRecord};
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
    let _fidelity =
        crate::test_support::source_fidelity::xml_layout_fixture(&property.id, "<Property/>");
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
