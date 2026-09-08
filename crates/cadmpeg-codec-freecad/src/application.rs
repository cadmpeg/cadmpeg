// SPDX-License-Identifier: Apache-2.0
//! Application-domain census and inert-payload classification.

use std::collections::HashMap;

use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::Serialize;

use crate::native::{EntryRecord, LinkTarget, ObjectRecord, PropertyFamily, PropertyRecord};

/// Write the legacy census from authoritative object, property, and entry records.
pub(crate) fn install(
    namespace: &mut NativeNamespace,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<(), NativeConvertError> {
    namespace.set_arena("applications", &wire_records(objects, properties, entries))
}

/// Check the persisted census against the same authoritative records used for writing.
pub(crate) fn matches_native(
    namespace: &NativeNamespace,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<bool, NativeConvertError> {
    let mut expected = wire_records(objects, properties, entries);
    expected.sort_by(|left, right| left.id.cmp(&right.id));
    let mut actual = namespace.arena_iter_as::<serde_json::Value>("applications");
    for record in expected {
        let Some(actual) = actual.next() else {
            return Ok(false);
        };
        if actual? != serde_json::to_value(record)? {
            return Ok(false);
        }
    }
    Ok(actual.next().is_none())
}

// These are serialization views, not independent preservation records. Payload bytes
// borrow their authoritative entry; absent object data remains an Option until this
// legacy wire boundary emits its empty-data and zero-offset representation.
#[derive(Serialize)]
struct ApplicationRecordWire<'a> {
    id: String,
    object: &'a str,
    type_name: &'a str,
    domain: &'a str,
    properties: Vec<&'a str>,
    dependencies: &'a [String],
    side_entries: Vec<&'a str>,
    inert_payload: bool,
    order: usize,
    byte_start: u64,
    byte_end: u64,
    byte_len: u64,
    sha256: String,
    data: &'a [u8],
    property_records: Vec<ApplicationPropertyWire<'a>>,
}

#[derive(Serialize)]
struct ApplicationPropertyWire<'a> {
    id: String,
    object: &'a str,
    property: &'a str,
    type_name: &'a str,
    family: PropertyFamily,
    order: usize,
    links: &'a [LinkTarget],
    byte_start: u64,
    byte_end: u64,
    byte_len: u64,
    sha256: String,
    data: &'a [u8],
    payloads: Vec<ApplicationPayloadWire<'a>>,
    inert: bool,
}

#[derive(Serialize)]
struct ApplicationPayloadWire<'a> {
    entry: &'a str,
    name: &'a str,
    byte_len: u64,
    sha256: String,
    data: &'a [u8],
}

fn wire_records<'a>(
    objects: &'a [ObjectRecord],
    properties: &'a [PropertyRecord],
    entries: &'a [EntryRecord],
) -> Vec<ApplicationRecordWire<'a>> {
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        by_owner.entry(&property.owner).or_default().push(property);
    }
    let entries = entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry))
        .collect::<HashMap<_, _>>();
    objects
        .iter()
        .map(|object| {
            let mut owned = by_owner.remove(object.id.as_str()).unwrap_or_default();
            owned.sort_by_key(|property| (property.xml.start(), property.xml.end()));
            let data = object
                .data
                .as_ref()
                .map_or(&[][..], |data| data.text().as_bytes());
            ApplicationRecordWire {
                id: crate::native::native_id("application", &object.name),
                object: &object.id,
                type_name: &object.type_name,
                domain: object
                    .type_name
                    .split_once("::")
                    .map_or("Unqualified", |(domain, _)| domain),
                properties: owned.iter().map(|property| property.id.as_str()).collect(),
                dependencies: &object.dependencies,
                side_entries: owned
                    .iter()
                    .flat_map(|property| property.side_entries().iter().map(String::as_str))
                    .collect(),
                inert_payload: owned.iter().any(|property| is_inert(property)),
                order: object.order,
                byte_start: object.data.as_ref().map_or(0, |data| data.start()),
                byte_end: object.data.as_ref().map_or(0, |data| data.end()),
                byte_len: data.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(data),
                data,
                property_records: owned
                    .into_iter()
                    .map(|property| {
                        let data = property.xml.text().as_bytes();
                        ApplicationPropertyWire {
                            id: crate::native::native_child_id(
                                "application-property",
                                &object.id,
                                &property.name,
                            ),
                            object: &object.id,
                            property: &property.id,
                            type_name: &property.type_name,
                            family: property.family,
                            order: property.order,
                            links: property.links(),
                            byte_start: property.xml.start(),
                            byte_end: property.xml.end(),
                            byte_len: data.len() as u64,
                            sha256: cadmpeg_ir::hash::sha256_hex(data),
                            data,
                            payloads: property
                                .side_entries()
                                .iter()
                                .filter_map(|name| entries.get(name.as_str()))
                                .map(|entry| ApplicationPayloadWire {
                                    entry: &entry.id,
                                    name: &entry.name,
                                    byte_len: entry.byte_len(),
                                    sha256: entry.sha256(),
                                    data: &entry.data,
                                })
                                .collect(),
                            inert: is_inert(property),
                        }
                    })
                    .collect(),
            }
        })
        .collect()
}

fn is_inert(property: &PropertyRecord) -> bool {
    property.family == PropertyFamily::PythonObject
        || property.type_name.contains("PropertyPythonObject")
}

#[cfg(test)]
mod tests;
