// SPDX-License-Identifier: Apache-2.0
//! Application-domain census and inert-payload classification.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::{Deserialize, Serialize};

mod identities;
pub(crate) type ApplicationRecord = ApplicationRecordWire<'static>;

use crate::native::{EntryRecord, LinkTarget, ObjectRecord, PropertyFamily, PropertyRecord};

#[cfg(test)]
pub(crate) fn install(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    namespace: &mut NativeNamespace,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<(), NativeConvertError> {
    let records = wire_records(objects, properties, entries, None)?;
    namespace.set_arena(ctx, "applications", &records)
}

pub(crate) fn install_source<'source>(
    ctx: &DecodeContext<'source>,
    namespace: &mut NativeNamespace,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
    sources: &BTreeMap<String, crate::persistence::object_parts::ObjectSource<'source>>,
    entry_owners: &crate::container::parts::EntryOwners<'source>,
) -> Result<(), NativeConvertError> {
    let records = wire_records(
        objects,
        properties,
        entries,
        Some((ctx, sources, entry_owners)),
    )?;
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
    let mut expected = wire_records(objects, properties, entries, None)?;
    let actual = namespace.arena_as_for_decode::<ApplicationRecord>(ctx, "applications")?;
    if expected.len() != actual.len() {
        return Ok(false);
    }
    let mut by_object = ctx.alloc_filled(objects.len(), 0_usize, "application object lookup")?;
    for (index, position) in by_object.iter_mut().enumerate() {
        *position = index;
    }
    by_object.sort_unstable_by(|left, right| objects[*left].id().cmp(objects[*right].id()));
    if by_object
        .windows(2)
        .any(|pair| objects[pair[0]].id() == objects[pair[1]].id())
    {
        return Ok(false);
    }
    let Some(rows) = namespace.arenas().get("applications") else {
        return Ok(actual.is_empty());
    };
    let mut matched = std::collections::BTreeSet::new();
    let mut identities = std::collections::BTreeSet::new();
    for (actual, row) in actual.iter().zip(rows) {
        if cadmpeg_ir::ids::Identity::new(actual.id.clone()).is_err()
            || !identities.insert(actual.id.as_str())
            || !matched.insert(actual.object.as_ref())
        {
            return Ok(false);
        }
        let Ok(position) = by_object
            .binary_search_by(|index| objects[*index].id().as_str().cmp(actual.object.as_ref()))
        else {
            return Ok(false);
        };
        let expected = &mut expected[by_object[position]];
        expected.id.clone_from(&actual.id);
        if expected.property_records.len() != actual.property_records.len() {
            return Ok(false);
        }
        for (expected, current) in expected
            .property_records
            .iter_mut()
            .zip(&actual.property_records)
        {
            if cadmpeg_ir::ids::Identity::new(current.id.clone()).is_err()
                || !identities.insert(current.id.as_str())
            {
                return Ok(false);
            }
            expected.id.clone_from(&current.id);
        }
        if row.id() != actual.id || !row.matches_typed_for_decode(ctx, expected)? {
            return Ok(false);
        }
    }
    Ok(true)
}

// Census records describe authoritative typed objects and their owned properties.
// Source producers construct each census owner from its original declaration field.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApplicationRecordWire<'a> {
    id: String,
    object: Cow<'a, str>,
    type_name: Cow<'a, str>,
    domain: Cow<'a, str>,
    properties: Vec<Cow<'a, str>>,
    dependencies: Cow<'a, [String]>,
    side_entries: Vec<Cow<'a, str>>,
    inert_payload: bool,
    order: usize,
    property_records: Vec<ApplicationPropertyWire<'a>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ApplicationPropertyWire<'a> {
    id: String,
    object: Cow<'a, str>,
    property: Cow<'a, str>,
    type_name: Cow<'a, str>,
    family: PropertyFamily,
    order: usize,
    links: Cow<'a, [Option<LinkTarget>]>,
    payloads: Vec<ApplicationPayloadWire<'a>>,
    inert: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ApplicationPayloadWire<'a> {
    entry: Cow<'a, str>,
    name: Cow<'a, str>,
}

fn wire_records<'a, 'source>(
    objects: &'a [ObjectRecord],
    properties: &'a [PropertyRecord],
    entries: &'a [EntryRecord],
    producer: Option<(
        &DecodeContext<'source>,
        &BTreeMap<String, crate::persistence::object_parts::ObjectSource<'source>>,
        &crate::container::parts::EntryOwners<'source>,
    )>,
) -> Result<Vec<ApplicationRecordWire<'a>>, CodecError> {
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
        let id = crate::native::native_id("application", object.name());
        let construct = |actual_object: Cow<'a, str>,
                         dependencies: Cow<'a, [String]>|
         -> Result<ApplicationRecordWire<'a>, CodecError> {
            let entry_name = |name: &'a str| -> Result<Cow<'a, str>, CodecError> {
                let Some((ctx, _, owners)) = producer else {
                    return Ok(Cow::Borrowed(name));
                };
                let actual = owners
                    .get(name)
                    .ok_or_else(|| {
                        CodecError::malformed(
                            "application side entry has no original header name source",
                        )
                    })?
                    .read_name(ctx)?;
                if actual != name {
                    return Err(CodecError::malformed(
                        "application side entry differs from its original source",
                    ));
                }
                Ok(Cow::Owned(actual))
            };
            // Transient declarations and persisted values have independent source
            // order domains. Equal ordinals use the typed property identity so the
            // census is identical before and after native arenas sort their rows.
            owned.sort_by(|left, right| (left.order, &left.id).cmp(&(right.order, &right.id)));
            let property_ids = owned
                .iter()
                .map(|property| Cow::Borrowed(property.id.as_str()))
                .collect();
            let side_entries = owned
                .iter()
                .flat_map(|property| property.side_entries().iter().map(String::as_str))
                .map(entry_name)
                .collect::<Result<Vec<_>, _>>()?;
            let mut property_records = Vec::new();
            for property in owned {
                let mut payloads = Vec::new();
                for name in property.side_entries() {
                    if let Some(entry) = entry_index.get(name.as_str()) {
                        payloads.push(ApplicationPayloadWire {
                            entry: Cow::Borrowed(entry.id()),
                            name: entry_name(entry.name())?,
                        });
                    }
                }
                property_records.push(ApplicationPropertyWire {
                    id: crate::native::native_child_id(
                        "application-property",
                        object.id(),
                        &property.name,
                    ),
                    object: actual_object.clone(),
                    property: Cow::Borrowed(&property.id),
                    type_name: Cow::Borrowed(&property.type_name),
                    family: property.family,
                    order: property.order,
                    links: Cow::Borrowed(property.links()),
                    payloads,
                    inert: is_inert(property),
                });
            }
            Ok(ApplicationRecordWire {
                id: id.clone(),
                object: actual_object,
                type_name: Cow::Borrowed(&object.type_name),
                domain: Cow::Borrowed(
                    object
                        .type_name
                        .split_once("::")
                        .map_or("Unqualified", |(domain, _)| domain),
                ),
                properties: property_ids,
                dependencies,
                side_entries,
                inert_payload: property_records.iter().any(|property| property.inert),
                order: object.order,
                property_records,
            })
        };
        let record = match producer {
            Some((ctx, sources, _)) => {
                let source = sources.get(object.id()).ok_or_else(|| {
                    CodecError::malformed(
                        "application census owner has no original source authority",
                    )
                })?;
                source.record(ctx, &id, |actual| {
                    let dependencies = source.dependencies(ctx, &object.dependencies)?;
                    construct(Cow::Owned(actual), Cow::Owned(dependencies))
                })?
            }
            None => construct(
                Cow::Borrowed(object.id()),
                Cow::Borrowed(&object.dependencies),
            )?,
        };
        records.push(record);
    }
    Ok(records)
}

fn is_inert(property: &PropertyRecord) -> bool {
    property.family == PropertyFamily::PythonObject
        || property.type_name.contains("PropertyPythonObject")
}

#[cfg(test)]
mod tests;
