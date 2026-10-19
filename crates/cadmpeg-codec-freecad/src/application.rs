// SPDX-License-Identifier: Apache-2.0
//! Application-domain census and inert-payload classification.

use std::collections::HashMap;

use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::Serialize;

use crate::native::{EntryRecord, LinkTarget, ObjectRecord, PropertyFamily, PropertyRecord};

/// Write the legacy census from authoritative object, property, and entry records.
pub(crate) fn install(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    namespace: &mut NativeNamespace,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<(), NativeConvertError> {
    let records = wire_records(objects, properties, entries);
    namespace.set_arena(ctx, "applications", &records)
}

/// Check the persisted census against the same authoritative records used for writing.
pub(crate) fn matches_native(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    namespace: &NativeNamespace,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<bool, NativeConvertError> {
    let mut expected = wire_records(objects, properties, entries);
    expected.sort_by(|left, right| left.id.cmp(&right.id));
    let mut actual = namespace.arenas().get("applications").into_iter().flatten();
    for record in expected {
        let Some(actual) = actual.next() else {
            return Ok(false);
        };
        if !actual.matches_typed_for_decode(ctx, &record)? {
            return Ok(false);
        }
    }
    Ok(actual.next().is_none())
}

// Census records refer to authoritative bytes through identities, ranges and digests.
// Absent object data uses zero offsets and the empty-byte digest.
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
    links: &'a [Option<LinkTarget>],
    byte_start: u64,
    byte_end: u64,
    byte_len: u64,
    sha256: String,
    payloads: Vec<ApplicationPayloadWire<'a>>,
    inert: bool,
}

#[derive(Serialize)]
struct ApplicationPayloadWire<'a> {
    entry: &'a str,
    name: &'a str,
    byte_len: u64,
    sha256: &'a str,
}

fn wire_records<'a>(
    objects: &'a [ObjectRecord],
    properties: &'a [PropertyRecord],
    entries: &'a [EntryRecord],
) -> Vec<ApplicationRecordWire<'a>> {
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        by_owner
            .entry(property.owner.as_str())
            .or_default()
            .push(property);
    }
    let mut entry_index = HashMap::new();
    for entry in entries {
        entry_index.insert(entry.name(), entry);
    }
    let mut records = Vec::new();
    for object in objects {
        let mut owned = by_owner.remove(object.id().as_str()).unwrap_or_default();
        owned.sort_by(|left, right| {
            (left.xml.start(), left.xml.end()).cmp(&(right.xml.start(), right.xml.end()))
        });
        let data = object
            .data
            .as_ref()
            .map_or(&[][..], |data| data.text().as_bytes());
        let property_ids = owned.iter().map(|property| property.id.as_str()).collect();
        let side_entries = owned
            .iter()
            .flat_map(|property| property.side_entries().iter().map(String::as_str))
            .collect();
        let mut property_records = Vec::new();
        for property in owned {
            let data = property.xml.text().as_bytes();
            let mut payloads = Vec::new();
            for name in property.side_entries() {
                if let Some(entry) = entry_index.get(name.as_str()) {
                    payloads.push(ApplicationPayloadWire {
                        entry: entry.id(),
                        name: entry.name(),
                        byte_len: entry.byte_len(),
                        sha256: entry.sha256(),
                    });
                }
            }
            property_records.push(ApplicationPropertyWire {
                id: crate::native::native_child_id(
                    "application-property",
                    object.id(),
                    &property.name,
                ),
                object: object.id(),
                property: &property.id,
                type_name: &property.type_name,
                family: property.family,
                order: property.order,
                links: property.links(),
                byte_start: property.xml.start(),
                byte_end: property.xml.end(),
                byte_len: cadmpeg_core::decode::u64_from_index(data.len()),
                sha256: cadmpeg_ir::hash::sha256_hex(data),
                payloads,
                inert: is_inert(property),
            });
        }
        records.push(ApplicationRecordWire {
            id: crate::native::native_id("application", object.name()),
            object: object.id(),
            type_name: &object.type_name,
            domain: object
                .type_name
                .split_once("::")
                .map_or("Unqualified", |(domain, _)| domain),
            properties: property_ids,
            dependencies: &object.dependencies,
            side_entries,
            inert_payload: property_records.iter().any(|property| property.inert),
            order: object.order,
            byte_start: object
                .data
                .as_ref()
                .map_or(0, crate::native::RetainedXml::start),
            byte_end: object
                .data
                .as_ref()
                .map_or(0, crate::native::RetainedXml::end),
            byte_len: cadmpeg_core::decode::u64_from_index(data.len()),
            sha256: cadmpeg_ir::hash::sha256_hex(data),
            property_records,
        });
    }
    records
}

fn is_inert(property: &PropertyRecord) -> bool {
    property.family == PropertyFamily::PythonObject
        || property.type_name.contains("PropertyPythonObject")
}

#[cfg(test)]
mod tests;
