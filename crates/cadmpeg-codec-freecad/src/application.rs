// SPDX-License-Identifier: Apache-2.0
//! Application-domain census and inert-payload classification.

use std::collections::HashMap;

use cadmpeg_ir::wire::hash::sha256_hex;

use crate::native::{
    ApplicationPayloadRecord, ApplicationPropertyRecord, ApplicationRecord, EntryRecord,
    ObjectRecord, PropertyFamily, PropertyRecord,
};

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Vec<ApplicationRecord> {
    let by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    let entries = entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry))
        .collect::<HashMap<_, _>>();
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
            let data = object
                .raw_xml
                .as_deref()
                .unwrap_or_default()
                .as_bytes()
                .to_vec();
            let byte_start = object.byte_start.unwrap_or_default();
            let byte_end = object.byte_end.unwrap_or(byte_start);
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
                order: object.order,
                byte_start,
                byte_end,
                byte_len: data.len() as u64,
                sha256: sha256_hex(&data),
                data,
                property_records: owned
                    .iter()
                    .map(|property| {
                        let data = property.raw_xml.as_bytes().to_vec();
                        ApplicationPropertyRecord {
                            id: crate::native::native_child_id(
                                "application-property",
                                &object.id,
                                &property.name,
                            ),
                            object: object.id.clone(),
                            property: property.id.clone(),
                            type_name: property.type_name.clone(),
                            family: property.family,
                            order: property.order,
                            links: property.links.clone(),
                            byte_start: property.byte_start,
                            byte_end: property.byte_end,
                            byte_len: data.len() as u64,
                            sha256: sha256_hex(&data),
                            data,
                            payloads: property
                                .side_entries
                                .iter()
                                .filter_map(|name| entries.get(name.as_str()))
                                .map(|entry| ApplicationPayloadRecord {
                                    entry: entry.id.clone(),
                                    name: entry.name.clone(),
                                    byte_len: entry.byte_len,
                                    sha256: entry.sha256.clone(),
                                    data: entry.data.clone(),
                                })
                                .collect(),
                            inert: property.family == PropertyFamily::PythonObject
                                || property.type_name.contains("PropertyPythonObject"),
                        }
                    })
                    .collect(),
            }
        })
        .collect()
}
