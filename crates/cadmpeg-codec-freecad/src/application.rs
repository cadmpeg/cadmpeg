// SPDX-License-Identifier: Apache-2.0
//! Application-domain census and inert-payload classification.

use cadmpeg_core::decode::ScopedReservation;
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::Serialize;
use std::collections::BTreeMap;

use crate::native::{EntryRecord, LinkTarget, ObjectRecord, PropertyFamily, PropertyRecord};

/// Write the legacy census from authoritative object, property, and entry records.
pub(crate) fn install(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    namespace: &mut NativeNamespace,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<(), NativeConvertError> {
    let records = ctx
        .with_scoped_storage("FreeCAD application wire records", || {
            wire_records(ctx, objects, properties, entries)
        })?;
    let _storage = records.1;
    let records = records.0;
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
    let expected = ctx
        .with_scoped_storage("FreeCAD expected application records", || {
            wire_records(ctx, objects, properties, entries)
        })?;
    let _expected_storage = expected.1;
    let mut expected = expected.0;
    ctx.stable_sort_by(
        &mut expected,
        |value| &value.id,
        Ord::cmp,
        "FreeCAD application records sort",
    )?;
    let mut actual = namespace.arena_iter_as_for_decode::<serde_json::Value>(ctx, "applications");
    let mut expected = expected.into_iter();
    while expected.len() != 0 {
        if actual.size_hint().1 == Some(0) {
            return Ok(false);
        }
        let Some(record) = ctx.next_charged(&mut expected, "FreeCAD expected applications")? else {
            break;
        };
        let record_result =
            ctx.with_scoped_storage("FreeCAD actual application record", || {
                ctx.next_charged(&mut actual, "FreeCAD actual application record")?
                    .transpose()
            })?;
        let _actual_storage = record_result.1;
        let actual = record_result.0;
        let Some(actual) = actual else {
            return Ok(false);
        };
        if actual != serde_json::to_value(record)? {
            return Ok(false);
        }
    }
    if actual.size_hint().1 == Some(0) {
        return Ok(true);
    }
    let tail =
        ctx.with_scoped_storage("FreeCAD actual application tail", || {
            ctx.next_charged(&mut actual, "FreeCAD actual application tail")?
                .transpose()
        })?;
    let _tail_storage = tail.1;
    let tail = tail.0;
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

struct OwnerProperties<'records, 'context> {
    properties: Vec<&'records PropertyRecord>,
    storage: ScopedReservation<'context>,
}

fn wire_records<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    objects: &'a [ObjectRecord],
    properties: &'a [PropertyRecord],
    entries: &'a [EntryRecord],
) -> Result<Vec<ApplicationRecordWire<'a>>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if objects.is_empty() {
        return Ok(Vec::new());
    }
    const OWNER_PROPERTIES: &str = "FreeCAD application owner properties";
    let mut owner_storage = ctx.reserve_scoped(0, OWNER_PROPERTIES)?;
    let mut by_owner: BTreeMap<&str, Option<OwnerProperties<'_, '_>>> = BTreeMap::new();
    let mut source_properties = properties.iter();
    while source_properties.len() != 0 {
        let Some(property) = ctx.next_charged(&mut source_properties, OWNER_PROPERTIES)? else {
            break;
        };
        let slot = owner_storage.with_storage(|| {
            ctx.entry_btree_map(&mut by_owner, property.owner.as_str(), OWNER_PROPERTIES)
                .map(std::collections::btree_map::Entry::or_default)
        })?;
        let group = match slot {
            Some(group) => group,
            slot @ None => slot.insert(OwnerProperties {
                properties: Vec::new(),
                storage: ctx.reserve_scoped(0, OWNER_PROPERTIES)?,
            }),
        };
        group.storage.with_storage(|| ctx.push_vec(&mut group.properties, property, OWNER_PROPERTIES))?;
    }
    let mut entry_storage = None;
    let mut entry_index: Option<BTreeMap<&str, &EntryRecord>> = None;
    let mut records = ctx.collection_vec(objects.len(), "FreeCAD application records")?;
    let mut object_iter = objects.iter();
    while object_iter.len() != 0 {
        let Some(object) = ctx.next_charged(&mut object_iter, "FreeCAD application objects")? else {
            break;
        };
        let owned = ctx
            .get_mut_btree_map(
                &mut by_owner,
                object.id().as_str(),
                "FreeCAD application owner lookup",
            )?
            .and_then(Option::take);
        let mut owned = match owned {
            Some(group) => group,
            None => OwnerProperties {
                properties: Vec::new(),
                storage: ctx.reserve_scoped(0, OWNER_PROPERTIES)?,
            },
        };
        ctx.stable_sort_by_key(
            &mut owned.properties,
            |value| (value.xml.start(), value.xml.end()),
            Ord::cmp,
            "FreeCAD application owner properties sort",
        )?;
        let data = object
            .data
            .as_ref()
            .map_or(&[][..], |data| data.text().as_bytes());
        let mut property_ids =
            ctx.collection_vec(owned.properties.len(), "FreeCAD application property IDs")?;
        let mut side_entries = Vec::new();
        let mut property_records =
            ctx.collection_vec(owned.properties.len(), "FreeCAD application property records")?;
        let mut inert_payload = false;
        let mut property_iter = owned.properties.into_iter();
        while property_iter.len() != 0 {
            let Some(property) = ctx.next_charged(&mut property_iter, "FreeCAD application properties")? else {
                break;
            };
            property_ids.push(property.id.as_str());
            let data = property.xml.text().as_bytes();
            let mut payloads = Vec::new();
            let mut entry_iter = property.side_entries().iter();
            while entry_iter.len() != 0 {
                let Some(name) = ctx.next_charged(&mut entry_iter, "FreeCAD application side-entry names")? else {
                    break;
                };
                let entry_index = match &mut entry_index {
                    Some(index) => index,
                    slot @ None => {
                        let (index, storage) = ctx.collect_scoped_btree_map(
                            entries.iter().map(|entry| (entry.name(), entry)),
                            "FreeCAD application entry lookup",
                        )?;
                        entry_storage = Some(storage);
                        slot.insert(index)
                    }
                };
                ctx.push_vec(
                    &mut side_entries,
                    name.as_str(),
                    "FreeCAD application side entries",
                )?;
                if let Some(entry) = ctx.get_btree_map(
                    entry_index,
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
        drop(property_iter);
        drop(owned.storage);
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
                .position_by(
                    object.type_name.as_bytes().windows(b"::".len()),
                    |window| Ok(window == b"::"),
                    "FreeCAD application domain",
                )?
                .map_or("Unqualified", |separator| &object.type_name[..separator]),
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
    drop(entry_index);
    drop(entry_storage);
    drop(by_owner);
    drop(owner_storage);
    Ok(records)
}

fn is_inert(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    property: &PropertyRecord,
) -> Result<bool, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    Ok(property.family == PropertyFamily::PythonObject
        || ctx.position_by(
            property.type_name.as_bytes().windows(b"PropertyPythonObject".len()),
            |window| Ok(window == b"PropertyPythonObject"),
            "FreeCAD application inert payload",
        )?.is_some())
}

#[cfg(test)]
mod tests;
