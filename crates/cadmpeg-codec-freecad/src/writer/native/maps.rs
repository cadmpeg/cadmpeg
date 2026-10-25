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

mod legacy;
mod relocate;

fn hex(value: i64) -> String {
    if value < 0 {
        format!("-{:x}", value.unsigned_abs())
    } else {
        format!("{value:x}")
    }
}

pub(crate) fn write(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    objects: &mut [Object],
    names: &HashMap<String, String>,
    properties: &[PropertyRecord],
    native_only: bool,
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
                cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
                    "string table {} omits its unavailable source payload; no entry count is invented",
                    table.id()
                )),
            );
            super::super::dispositions::record(
                outcomes,
                "string_tables",
                &table.id(),
                cadmpeg_ir::codec::write::NativeDisposition::Dropped,
            );
            continue;
        };
        let (Some(save_all), Some(threshold)) = (table.save_all, table.threshold) else {
            let missing = [
                ("save_all", table.save_all.is_none()),
                ("threshold", table.threshold.is_none()),
            ]
            .into_iter()
            .filter_map(|(field, missing)| missing.then_some(field))
            .collect::<Vec<_>>();
            losses.push(cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
                "string table {} omits its payload because source metadata fields {missing:?} were not decoded", table.id()
            )));
            super::super::dispositions::record(
                outcomes,
                "string_tables",
                &table.id(),
                cadmpeg_ir::codec::write::NativeDisposition::Dropped,
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
        table_indices.insert(table.id(), output_index);
        let absent = entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| entry.is_none().then_some(index))
            .collect::<Vec<_>>();
        if !absent.is_empty() {
            losses.push(
                cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
                    "string table {} omits undecoded source record positions {absent:?}",
                    table.id()
                )),
            );
        }
        let mut content = String::new();
        for entry in entries.iter().flatten() {
            content.push_str(&hex(entry.string_id));
            content.push('.');
            let flags = format!("{:x}", entry.flags);
            content.push_str(&flags);
            for component in &entry.components {
                content.push('.');
                content.push_str(&hex(*component));
            }
            content.push(' ');
            if entry.flags & 8 == 0 {
                content.push_str(&entry.payload.matches('\n').count().to_string());
                content.push(':');
            }
            content.push_str(&entry.payload);
            content.push('\n');
        }
        let file = format!("Cadmpeg.StringHasher.{output_index}.txt");
        let stream = format!(
            "StringTableStart v1 {}\n{content}",
            entries.iter().flatten().count()
        );
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
                table_indices.remove(&table.id());
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
            table_indices.remove(&table.id());
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
    let maps = namespace.arena_as::<ElementMapRecord>("element_maps")?;
    let payloads = if native_only {
        crate::brep::records::payloads_untracked(namespace)?
    } else {
        Vec::new()
    };
    let mut node_ids = maps
        .iter()
        .flat_map(|map| map.maps.iter().map(|node| node.map_id))
        .collect::<std::collections::BTreeSet<_>>();
    for original in maps {
        let map = &original;
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
        let hasher = map
            .string_table
            .as_ref()
            .and_then(|id| table_indices.get(id))
            .copied();
        if map.string_table.is_some() && hasher.is_none() {
            losses.push(
                cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
                    "element map {} omits its payload because its required string table was omitted",
                    map.id
                )),
            );
            super::super::dispositions::record(
                outcomes,
                "element_maps",
                &map.id,
                cadmpeg_ir::codec::write::NativeDisposition::Dropped,
            );
            continue;
        }
        let payloads = payloads
            .iter()
            .filter(|payload| payload.property == original.property)
            .collect::<Vec<_>>();
        let indices = if let [payload] = payloads.as_slice() {
            crate::topology_transfer::canonical_indices::for_payload(ctx, payload)?
        } else {
            Vec::new()
        };
        let (map, disposition) = relocate::apply(
            &original,
            indices
                .into_iter()
                .map(|(kind, index)| (kind, index, index)),
            losses,
            &mut node_ids,
        )?;
        let disposition = if native_only
            && disposition == cadmpeg_ir::codec::write::NativeDisposition::Regenerated
        {
            cadmpeg_ir::codec::write::NativeDisposition::Written
        } else {
            disposition
        };
        if let Some(rows) = &map.legacy_records {
            let mut updated = shape.clone();
            if !legacy::part_attributes(&mut updated, &map.version, hasher)? {
                losses.push(
                    FreecadLossCode::ExportNativePropertyNotTransferred
                        .note(format!("element map {} has no emitted Part value", map.id)),
                );
                continue;
            }
            let source_rows = original.legacy_records.as_deref().ok_or_else(|| {
                CodecError::malformed("relocated legacy map has no canonical legacy rows")
            })?;
            if let Some(carrier) =
                legacy::carrier(&map.id, rows, source_rows, archive_entries, losses)?
            {
                *shape = updated.replacen("</Property>", &format!("{carrier}</Property>"), 1);
                super::super::dispositions::record(outcomes, "element_maps", &map.id, disposition);
            } else {
                super::super::dispositions::record(
                    outcomes,
                    "element_maps",
                    &map.id,
                    cadmpeg_ir::codec::write::NativeDisposition::Dropped,
                );
            }
            continue;
        }
        let absent = missing_positions(&original);
        if !absent.is_empty() {
            losses.push(cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
                "element map {} omits undecoded source positions {absent:?}; held sparse name chains use exact supported child-map associations where possible", map.id
            )));
        }
        let postfix_indices = map
            .postfixes
            .iter()
            .enumerate()
            .filter_map(|(index, postfix)| postfix.as_ref().map(|postfix| (index + 1, postfix)))
            .enumerate()
            .map(|(new, (old, _))| (old, new + 1))
            .collect::<BTreeMap<_, _>>();
        let map_indices = map
            .maps
            .indexed()
            .enumerate()
            .map(|(new, (old, _))| (old, new + 1))
            .collect::<BTreeMap<_, _>>();
        let mut content = format!(
            "{} PostfixCount {} {} MapCount {}\n",
            map.map_id,
            map.postfixes.iter().flatten().count(),
            map.postfixes
                .iter()
                .flatten()
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
            map.maps.iter().count()
        );
        for (index, (_, node)) in map.maps.indexed().enumerate() {
            content.push_str("ElementMap ");
            content.push_str(&(index + 1).to_string());
            content.push(' ');
            content.push_str(&node.map_id.to_string());
            content.push(' ');
            content.push_str(
                &node
                    .groups
                    .iter()
                    .flatten()
                    .filter(|group| group.names.iter().all(Option::is_some))
                    .count()
                    .to_string(),
            );
            content.push('\n');
            for group in node
                .groups
                .iter()
                .flatten()
                .filter(|group| group.names.iter().all(Option::is_some))
            {
                content.push_str(&group.indexed_name);
                content.push_str(" ChildCount ");
                content.push_str(&group.children.iter().flatten().count().to_string());
                content.push(' ');
                for child in group.children.iter().flatten() {
                    let mut fields = child
                        .split_ascii_whitespace()
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    if let Some(index) = fields
                        .get(4)
                        .and_then(|value| value.parse::<usize>().ok())
                        .filter(|index| *index != 0)
                    {
                        let mapped = map_indices.get(&index).ok_or_else(|| {
                            CodecError::InvalidInput(
                                "element-map child names an absent map node".into(),
                            )
                        })?;
                        fields[4] = mapped.to_string();
                    }
                    content.push_str(&fields.join(" "));
                    content.push(' ');
                }
                content.push_str(" NameCount ");
                content.push_str(&group.names.len().to_string());
                content.push('\n');
                for chain in group.names.iter().flatten() {
                    for name in chain {
                        let mut fields = name
                            .encoded
                            .split('.')
                            .map(str::to_owned)
                            .collect::<Vec<_>>();
                        let postfix_position = if let Some(prefix) = fields[0].strip_prefix(':') {
                            let original = usize::from_str_radix(prefix, 16).map_err(|_| {
                                CodecError::InvalidInput(
                                    "invalid element-map dictionary index".into(),
                                )
                            })?;
                            fields[0] = format!(
                                ":{:x}",
                                postfix_indices.get(&original).ok_or_else(|| {
                                    CodecError::InvalidInput(
                                        "mapped name refers to an absent dictionary entry".into(),
                                    )
                                })?
                            );
                            2
                        } else {
                            1
                        };
                        let original = fields
                            .get(postfix_position)
                            .and_then(|value| usize::from_str_radix(value, 16).ok())
                            .ok_or_else(|| {
                                CodecError::InvalidInput("mapped name has no postfix index".into())
                            })?;
                        if original != 0 {
                            fields[postfix_position] = format!(
                                "{:x}",
                                postfix_indices.get(&original).ok_or_else(|| {
                                    CodecError::InvalidInput(
                                        "mapped name refers to an absent postfix".into(),
                                    )
                                })?
                            );
                        }
                        content.push_str(&fields.join("."));
                        content.push(' ');
                    }
                    content.push_str("0\n");
                }
            }
            content.push_str("EndMap\n");
        }
        if !legacy::part_attributes(shape, &map.version, hasher)? {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred
                    .note(format!("element map {} has no emitted Part value", map.id)),
            );
            continue;
        }
        let count = map
            .declared_count
            .map(|count| format!(" count=\"{count}\""))
            .unwrap_or_default();
        let map_xml = format!(
            "<ElementMap new=\"1\" count=\"0\"/><ElementMap2{count}>{}</ElementMap2>",
            xml::escaped(&content)
        );
        *shape = shape.replacen("</Property>", &format!("{map_xml}</Property>"), 1);
        super::super::dispositions::record(outcomes, "element_maps", &map.id, disposition);
    }
    Ok(dictionary)
}

fn missing_positions(map: &ElementMapRecord) -> Vec<String> {
    let mut absent = Vec::new();
    for (index, postfix) in map.postfixes.iter().enumerate() {
        if postfix.is_none() {
            absent.push(format!("postfix[{index}]"));
        }
    }
    let admitted = map
        .maps
        .indexed()
        .map(|(index, _)| index)
        .collect::<std::collections::BTreeSet<_>>();
    for index in 1..=map.maps.slot_count() {
        if !admitted.contains(&index) {
            absent.push(format!("node[{index}]"));
        }
    }
    for (node_index, node) in map.maps.indexed() {
        for (group_index, group) in node.groups.iter().enumerate() {
            let path = format!("node[{node_index}].group[{group_index}]");
            let Some(group) = group else {
                absent.push(path);
                continue;
            };
            for (index, child) in group.children.iter().enumerate() {
                if child.is_none() {
                    absent.push(format!("{path}.child[{index}]"));
                }
            }
            for (index, chain) in group.names.iter().enumerate() {
                if chain.is_none() {
                    absent.push(format!("{path}.name_chain[{index}]"));
                }
            }
        }
    }
    absent
}

#[cfg(test)]
mod tests;
