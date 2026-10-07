// SPDX-License-Identifier: Apache-2.0
//! Application-domain census and inert-payload classification.

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
    let (records, _storage) = ctx
        .with_scoped_storage("FreeCAD application wire records", || {
            wire_records(ctx, objects, properties, entries)
        })?;
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
    let mut expected = expected.into_iter();
    while let Some(record) = ctx.next_charged(&mut expected, "FreeCAD expected applications")? {
        let (actual, _actual_storage) =
            ctx.with_scoped_storage("FreeCAD actual application record", || {
                ctx.next_charged(&mut actual, "FreeCAD actual application record")?
                    .transpose()
            })?;
        let Some(actual) = actual else {
            return Ok(false);
        };
        if actual != serde_json::to_value(record)? {
            return Ok(false);
        }
    }
    let (tail, _tail_storage) =
        ctx.with_scoped_storage("FreeCAD actual application tail", || {
            ctx.next_charged(&mut actual, "FreeCAD actual application tail")?
                .transpose()
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
    let (mut by_owner, _owner_storage) = ctx.collect_scoped_btree_groups(
        properties
            .iter()
            .map(|property| (property.owner.as_str(), property)),
        "FreeCAD application owner properties",
    )?;
    let (entry_index, _entry_storage) = ctx.collect_scoped_btree_map(
        entries.iter().map(|entry| (entry.name(), entry)),
        "FreeCAD application entry lookup",
    )?;
    let mut records = ctx.collection_vec(objects.len(), "FreeCAD application records")?;
    for object in ctx.admit_iter(objects, "FreeCAD application objects")? {
        let mut owned = ctx
            .remove_btree_map(
                &mut by_owner,
                object.id().as_str(),
                "FreeCAD application owner lookup",
            )?
            .unwrap_or_default();
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
        let mut side_entries = Vec::new();
        let mut property_records =
            ctx.collection_vec(owned.len(), "FreeCAD application property records")?;
        let mut inert_payload = false;
        for property in ctx.admit_iter(owned, "FreeCAD application properties")? {
            property_ids.push(property.id.as_str());
            let data = property.xml.text().as_bytes();
            let mut payloads = Vec::new();
            for name in ctx.admit_iter(
                property.side_entries(),
                "FreeCAD application side-entry names",
            )? {
                ctx.push_vec(
                    &mut side_entries,
                    name.as_str(),
                    "FreeCAD application side entries",
                )?;
                if let Some(entry) = ctx.get_btree_map(
                    &entry_index,
                    name.as_str(),
                    "FreeCAD application payload lookup",
                )? {
                    ctx.push_vec(
                        &mut payloads,
                        ApplicationPayloadWire {
                            entry: entry.id(),
                            name: entry.name(),
                            byte_len: entry.byte_len(),
                            sha256: entry.sha256(),
                            data: entry.data(),
                        },
                        "FreeCAD application payloads",
                    )?;
                }
            }
            let inert = is_inert(ctx, property)?;
            inert_payload |= inert;
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
                inert,
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
            domain: ctx
                .split_once(&object.type_name, "::", "FreeCAD application domain")?
                .map_or("Unqualified", |(domain, _)| domain),
            properties: property_ids,
            dependencies: &object.dependencies,
            side_entries,
            inert_payload,
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

fn is_inert(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    property: &PropertyRecord,
) -> Result<bool, CodecError> {
    Ok(property.family == PropertyFamily::PythonObject
        || ctx.contains_text(
            &property.type_name,
            "PropertyPythonObject",
            "FreeCAD application inert payload",
        )?)
}

#[cfg(test)]
mod tests;
