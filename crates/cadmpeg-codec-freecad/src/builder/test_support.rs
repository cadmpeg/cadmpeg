// SPDX-License-Identifier: Apache-2.0
//! Fixed linked-part and side-entry state used by builder and pipeline tests.

use std::collections::BTreeMap;

use super::{FcstdDocumentBuilder, FcstdPropertyValue, Property};

/// Attach the declared `Box` to `Part` and install the fixture side entry.
pub(crate) fn attach_part_fixture(builder: &mut FcstdDocumentBuilder, payload: &[u8]) {
    let part = builder
        .objects
        .iter_mut()
        .find(|object| object.name == "Part")
        .expect("fixture declares Part");
    part.dependencies.push("Box".to_owned());
    part.properties.push(Property {
        name: "Group".to_owned(),
        type_name: "App::PropertyLinkList".to_owned(),
        values: vec![FcstdPropertyValue {
            tag: "LinkList".to_owned(),
            attributes: BTreeMap::from([("count".to_owned(), "1".to_owned())]),
            text: None,
            children: vec![FcstdPropertyValue::attribute("Link", "value", "Box")],
        }],
    });
    builder
        .side_entries
        .push(("Payload.bin".to_owned(), payload.to_vec()));
}
