// SPDX-License-Identifier: Apache-2.0
//! Application-domain census and inert-payload classification.

use std::collections::HashMap;

use cadmpeg_core::CodecError;
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
    let records = wire_records(ctx, objects, properties, entries)?;
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
    let (mut expected, _expected_storage) = ctx
        .with_scoped_storage("FreeCAD expected application records", || {
            wire_records(ctx, objects, properties, entries)
        })?;
    ctx.stable_sort_by(
        &mut expected,
        |value| &value.id,
        Ord::cmp,
        "FreeCAD application records sort",
    )?;
    let mut actual = namespace.arena_iter_as_for_decode::<serde_json::Value>(ctx, "applications");
    for record in expected {
        let (actual, _actual_storage) = ctx
            .with_scoped_storage("FreeCAD actual application record", || {
                actual.next().transpose()
            })?;
        let Some(actual) = actual else {
            return Ok(false);
        };
        if actual != serde_json::to_value(record)? {
            return Ok(false);
        }
    }
    let (tail, _tail_storage) = ctx
        .with_scoped_storage("FreeCAD actual application tail", || {
            actual.next().transpose()
        })?;
    Ok(tail.is_none())
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
    links: &'a [Option<LinkTarget>],
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
    sha256: &'a str,
    data: &'a [u8],
}

fn wire_records<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    objects: &'a [ObjectRecord],
    properties: &'a [PropertyRecord],
    entries: &'a [EntryRecord],
) -> Result<Vec<ApplicationRecordWire<'a>>, CodecError> {
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        let owner = property.owner.as_str();
        if !by_owner.contains_key(owner) {
            ctx.reserve_map(&mut by_owner, 1, "FreeCAD application owner lookup")?;
        }
        let owned = by_owner.entry(owner).or_default();
        ctx.reserve_vec(owned, 1, "FreeCAD application owner properties")?;
        owned.push(property);
    }
    let mut entry_index = HashMap::new();
    for entry in entries {
        if !entry_index.contains_key(entry.name()) {
            ctx.reserve_map(&mut entry_index, 1, "FreeCAD application entry lookup")?;
        }
        entry_index.insert(entry.name(), entry);
    }
    let mut records = ctx.collection_vec(objects.len(), "FreeCAD application records")?;
    for object in objects {
        let mut owned = by_owner.remove(object.id().as_str()).unwrap_or_default();
        ctx.stable_sort_by_key(
            &mut owned,
            |value| (value.xml.start(), value.xml.end()),
            Ord::cmp,
            "FreeCAD application owner properties sort",
        )?;
        let data = object
            .data
            .as_ref()
            .map_or(&[][..], |data| data.text().as_bytes());
        let mut property_ids =
            ctx.collection_vec(owned.len(), "FreeCAD application property IDs")?;
        property_ids.extend(owned.iter().map(|property| property.id.as_str()));
        let side_entry_count = owned
            .iter()
            .map(|property| property.side_entries().len())
            .sum();
        let mut side_entries =
            ctx.collection_vec(side_entry_count, "FreeCAD application side entries")?;
        side_entries.extend(
            owned
                .iter()
                .flat_map(|property| property.side_entries().iter().map(String::as_str)),
        );
        let mut property_records =
            ctx.collection_vec(owned.len(), "FreeCAD application property records")?;
        for property in owned {
            let data = property.xml.text().as_bytes();
            let payload_count = property
                .side_entries()
                .iter()
                .filter(|name| entry_index.contains_key(name.as_str()))
                .count();
            let mut payloads = ctx.collection_vec(payload_count, "FreeCAD application payloads")?;
            for name in property.side_entries() {
                if let Some(entry) = entry_index.get(name.as_str()) {
                    payloads.push(ApplicationPayloadWire {
                        entry: entry.id(),
                        name: entry.name(),
                        byte_len: entry.byte_len(),
                        sha256: entry.sha256(),
                        data: entry.data(),
                    });
                }
            }
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(data.len()),
                "FreeCAD application property digest",
            )?;
            ctx.charge_retained(64, "FreeCAD application property digest")?;
            property_records.push(ApplicationPropertyWire {
                id: crate::native::native_child_id_charged(
                    ctx,
                    "application-property",
                    object.id(),
                    &property.name,
                )?,
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
                data,
                payloads,
                inert: is_inert(property),
            });
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(data.len()),
            "FreeCAD application object digest",
        )?;
        ctx.charge_retained(64, "FreeCAD application object digest")?;
        records.push(ApplicationRecordWire {
            id: crate::native::native_id_charged(ctx, "application", object.name())?,
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
            data,
            property_records,
        });
    }
    Ok(records)
}

fn is_inert(property: &PropertyRecord) -> bool {
    property.family == PropertyFamily::PythonObject
        || property.type_name.contains("PropertyPythonObject")
}

#[cfg(test)]
mod tests;
