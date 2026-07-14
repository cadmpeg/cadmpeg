// SPDX-License-Identifier: Apache-2.0
//! Application-domain census and inert-payload classification.

use std::collections::HashMap;

use crate::native::{ApplicationRecord, ObjectRecord, PropertyFamily, PropertyRecord};

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Vec<ApplicationRecord> {
    let by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    objects
        .iter()
        .map(|object| {
            let mut owned = by_owner
                .get(object.id.as_str())
                .cloned()
                .unwrap_or_default();
            owned.sort_by_key(|property| (property.byte_start, property.byte_end));
            let domain = object
                .type_name
                .split_once("::")
                .map_or("Unqualified", |(domain, _)| domain);
            ApplicationRecord {
                id: crate::native::native_id("application", &object.name),
                object: object.id.clone(),
                type_name: object.type_name.clone(),
                domain: domain.to_owned(),
                properties: owned.iter().map(|property| property.id.clone()).collect(),
                dependencies: object.dependencies.clone(),
                side_entries: owned
                    .iter()
                    .flat_map(|property| &property.side_entries)
                    .cloned()
                    .collect(),
                inert_payload: owned.iter().any(|property| {
                    property.family == PropertyFamily::PythonObject
                        || property.type_name.contains("PropertyPythonObject")
                }),
            }
        })
        .collect()
}
