// SPDX-License-Identifier: Apache-2.0
//! Typed persistent-name dictionaries and maps, independent of source bytes.

use super::super::xml::{self, Object};
use crate::loss::FreecadLossCode;
use crate::native::element_map::ElementMapRecord;
use crate::native::{PropertyRecord, StringTableRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{BTreeMap, HashMap};

fn hex(value: i64) -> String {
    if value < 0 {
        format!("-{:x}", value.unsigned_abs())
    } else {
        format!("{value:x}")
    }
}

pub(crate) fn write(
    ir: &CadIr,
    objects: &mut [Object],
    names: &HashMap<String, String>,
    properties: &[PropertyRecord],
    archive_entries: &mut BTreeMap<String, Vec<u8>>,
    losses: &mut Vec<LossNote>,
    outcomes: &mut cadmpeg_ir::codec::write::NativeDispositions,
) -> Result<String, CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(String::new());
    };
    let mut tables = namespace.arena_as::<StringTableRecord>("string_tables")?;
    tables.sort_by_key(|table| {
        table
            .owner_property
            .as_ref()
            .and_then(|id| properties.iter().find(|property| property.id == *id))
            .map_or_else(
                || (0, None, String::new(), table.index),
                |property| {
                    (
                        1,
                        objects
                            .iter()
                            .position(|object| names.get(&property.owner) == Some(&object.name)),
                        property.name.clone(),
                        table.index,
                    )
                },
            )
    });
    let mut dictionary = String::new();
    let mut table_indices = BTreeMap::new();
    let mut next_hasher = 1_usize;
    for table in tables {
        let Some(entries) = table.entries() else {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred
                    .note(format!("string table {} has no typed entries", table.id())),
            );
            continue;
        };
        let Some(save_all) = table.save_all else {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                    "string table {} has no admitted save-all policy",
                    table.id()
                )),
            );
            continue;
        };
        let Some(threshold) = table.threshold else {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                    "string table {} has no admitted threshold",
                    table.id()
                )),
            );
            continue;
        };
        if table.owner_property.as_ref().is_some_and(|id| {
            !properties.iter().any(|property| {
                property.id == *id
                    && names
                        .get(&property.owner)
                        .is_some_and(|name| objects.iter().any(|object| object.name == *name))
            })
        }) {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred
                    .note(format!("string table {} has an omitted owner", table.id())),
            );
            continue;
        }
        let output_index = if table.owner_property.is_none() {
            0
        } else {
            let index = next_hasher;
            next_hasher = next_hasher.checked_add(1).ok_or_else(|| {
                CodecError::InvalidInput("FCStd hasher count exceeds address space".into())
            })?;
            index
        };
        table_indices.insert(table.index, output_index);
        let mut content = String::new();
        for entry in entries {
            content.push_str(&format!("{}.{:x}", hex(entry.string_id), entry.flags));
            for component in &entry.components {
                content.push('.');
                content.push_str(&hex(*component));
            }
            content.push(' ');
            if entry.flags & 8 == 0 {
                content.push_str(&format!("{}:", entry.payload.matches('\n').count()));
            }
            content.push_str(&entry.payload);
            content.push('\n');
        }
        let file = format!("Cadmpeg.StringHasher.{output_index}.txt");
        let stream = format!("StringTableStart v1 {}\n{content}", entries.len());
        let carrier = format!("<StringHasher saveall=\"{}\" threshold=\"{threshold}\" count=\"0\" new=\"1\"/><StringHasher2 file=\"{file}\"/>", u8::from(save_all));
        if let Some(owner) = &table.owner_property {
            let property = properties
                .iter()
                .find(|property| property.id == *owner)
                .ok_or_else(|| CodecError::malformed("admitted hasher owner disappeared"))?;
            let shape = names
                .get(&property.owner)
                .and_then(|name| objects.iter_mut().find(|object| object.name == *name))
                .and_then(|object| object.properties.get_mut(&property.name));
            let Some(shape) = shape else {
                table_indices.remove(&table.index);
                next_hasher = output_index;
                losses.push(
                    FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                        "string table {} has no emitted owner property",
                        table.id()
                    )),
                );
                continue;
            };
            *shape = shape.replacen("</Property>", &format!("{carrier}</Property>"), 1);
        } else if dictionary.is_empty() {
            dictionary = carrier;
        } else {
            table_indices.remove(&table.index);
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                    "string table {} is a second root hasher with no output owner",
                    table.id()
                )),
            );
            continue;
        }
        archive_entries.insert(file, stream.into_bytes());
        super::super::dispositions::record(
            outcomes,
            "string_tables",
            &table.id(),
            cadmpeg_ir::codec::write::NativeDisposition::Written,
        );
    }
    for map in namespace.arena_as::<ElementMapRecord>("element_maps")? {
        let Some(property) = properties
            .iter()
            .find(|property| property.id == map.property)
        else {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred
                    .note(format!("element map {} has no typed property", map.id)),
            );
            continue;
        };
        let Some(object) = names
            .get(&property.owner)
            .and_then(|name| objects.iter_mut().find(|object| object.name == *name))
        else {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred
                    .note(format!("element map {} has an omitted owner", map.id)),
            );
            continue;
        };
        let Some(shape) = object.properties.get_mut(&property.name) else {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                    "element map {} has an omitted shape property",
                    map.id
                )),
            );
            continue;
        };
        if map
            .maps
            .iter()
            .flat_map(|node| &node.groups)
            .flat_map(|group| &group.names)
            .flatten()
            .flat_map(|name| &name.topology_ids)
            .any(|id| !names.contains_key(id))
        {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred
                    .note(format!("element map {} refers to omitted topology", map.id)),
            );
            continue;
        }
        let hasher = map
            .hasher_index
            .and_then(|index| table_indices.get(&index))
            .copied();
        if map.hasher_index.is_some() && hasher.is_none() {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                    "element map {} refers to an omitted string table",
                    map.id
                )),
            );
            continue;
        }
        let mut content = format!(
            "{} PostfixCount {} {} MapCount {}\n",
            map.map_id,
            map.postfixes.len(),
            map.postfixes.join(" "),
            map.maps.iter().len()
        );
        for (index, node) in map.maps.iter().enumerate() {
            content.push_str(&format!(
                "ElementMap {} {} {}",
                index + 1,
                node.map_id,
                node.groups.len()
            ));
            content.push('\n');
            for group in &node.groups {
                content.push_str(&format!(
                    "{} ChildCount {} {} NameCount {}",
                    group.indexed_name,
                    group.children.len(),
                    group.children.join(" "),
                    group.names.len()
                ));
                content.push('\n');
                for chain in &group.names {
                    for name in chain {
                        content.push_str(&name.encoded);
                        content.push(' ');
                    }
                    content.push_str("0\n");
                }
            }
            content.push_str("EndMap\n");
        }
        let part_attributes = format!(
            "<Part ElementMap=\"{}\"{} ",
            xml::escaped(&map.version),
            hasher.map_or_else(String::new, |index| format!(" HasherIndex=\"{index}\""))
        );
        *shape = shape.replacen("<Part ", &part_attributes, 1);
        let map_xml = format!(
            "<ElementMap new=\"1\" count=\"0\"/><ElementMap2 count=\"{}\">{}</ElementMap2>",
            map.declared_count.unwrap_or(0),
            xml::escaped(&content)
        );
        *shape = shape.replacen("</Property>", &format!("{map_xml}</Property>"), 1);
        super::super::dispositions::record(
            outcomes,
            "element_maps",
            &map.id,
            cadmpeg_ir::codec::write::NativeDisposition::Written,
        );
    }
    Ok(dictionary)
}
