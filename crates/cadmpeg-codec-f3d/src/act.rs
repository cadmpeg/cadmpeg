// SPDX-License-Identifier: Apache-2.0
//! Fusion ACT entity table and change-version channel groups.

use std::collections::BTreeMap;

use crate::bytes::{is_guid_hyphenated, lp_ascii_strict, lp_utf16_bounded};
use crate::records::{ActEntity, ActGuid, ActRootComponent};
use cadmpeg_ir::codec::{CodecError, ReadSeek};

use crate::container::{role, ContainerScan};

pub struct DecodedAct {
    pub entities: Vec<ActEntity>,
    pub guids: Vec<ActGuid>,
    pub root_components: Vec<ActRootComponent>,
}

pub fn decode(_reader: &mut dyn ReadSeek, scan: &ContainerScan) -> Result<DecodedAct, CodecError> {
    let mut entities = Vec::new();
    let mut guids = Vec::new();
    let mut root_components = Vec::new();
    for entry in scan.entries.iter().filter(|entry| {
        entry.role == role::BULKSTREAM && entry.name.contains("FusionACTSegmentType")
    }) {
        let bytes = scan.entry_bytes(&entry.name)?;
        let (table, stream_guids) = decode_table(bytes);
        let groups = decode_channel_groups(bytes);
        root_components.extend(decode_root_components(bytes, &entry.name));
        let mut by_key: BTreeMap<(u32, String), ActEntity> = BTreeMap::new();
        for item in table {
            by_key.insert(
                (item.record_index, item.entity_id.clone()),
                ActEntity {
                    id: format!("f3d:{}:act-entity#{}", entry.name, item.record_index),
                    record_index: item.record_index,
                    table_record_index_offset: Some(item.record_index_offset as u64),
                    channel_record_index_offset: None,
                    entity_id: item.entity_id,
                    table_entity_id_offset: Some(item.entity_id_offset as u64),
                    channel_entity_id_offset: None,
                    in_table: true,
                    channel_class_tag: None,
                    channels: BTreeMap::new(),
                    channel_guid_offsets: BTreeMap::new(),
                },
            );
        }
        for group in groups {
            let key = (group.record_index, group.entity_id.clone());
            let entity = by_key.entry(key).or_insert_with(|| ActEntity {
                id: format!("f3d:{}:act-entity#{}", entry.name, group.record_index),
                record_index: group.record_index,
                table_record_index_offset: None,
                channel_record_index_offset: Some(group.record_index_offset as u64),
                entity_id: group.entity_id.clone(),
                table_entity_id_offset: None,
                channel_entity_id_offset: Some(group.entity_id_offset as u64),
                in_table: false,
                channel_class_tag: None,
                channels: BTreeMap::new(),
                channel_guid_offsets: BTreeMap::new(),
            });
            entity.channel_class_tag = Some(group.class_tag);
            entity.channels = group.channels;
            entity.channel_record_index_offset = Some(group.record_index_offset as u64);
            entity.channel_entity_id_offset = Some(group.entity_id_offset as u64);
            entity.channel_guid_offsets = group.guid_offsets;
        }
        entities.extend(by_key.into_values());
        guids.extend(
            stream_guids
                .into_iter()
                .enumerate()
                .map(|(ordinal, (guid, offset))| ActGuid {
                    id: format!("f3d:{}:act-guid#{offset}", entry.name),
                    byte_offset: offset as u64,
                    guid_offset: (offset + 4) as u64,
                    ordinal: ordinal as u32,
                    guid,
                }),
        );
    }
    Ok(DecodedAct {
        entities,
        guids,
        root_components,
    })
}

fn decode_root_components(bytes: &[u8], stream: &str) -> Vec<ActRootComponent> {
    let mut out = Vec::new();
    let mut position = 0usize;
    while position + 4 <= bytes.len() {
        let Some((class_tag, after_tag)) = lp_ascii_strict(bytes, position, 1..=128) else {
            position += 1;
            continue;
        };
        if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
            position += 1;
            continue;
        }
        let Some(record_raw) = bytes.get(after_tag..after_tag + 4) else {
            break;
        };
        if bytes.get(after_tag + 4..after_tag + 14) != Some(&[0; 10]) {
            position += 1;
            continue;
        }
        let cursor = after_tag + 14;
        let instance_root_record_offset = cursor + 1;
        let Some((instance_root_record, cursor)) = marker_ref(bytes, cursor, 6) else {
            position += 1;
            continue;
        };
        let entity_id_offset = cursor + 4;
        let Some((entity_id, cursor)) = lp_utf16_bounded(bytes, cursor, 0..=1024) else {
            position += 1;
            continue;
        };
        let Some((flag, cursor)) = marker_ref(bytes, cursor, 5) else {
            position += 1;
            continue;
        };
        if flag != 3 {
            position += 1;
            continue;
        }
        let registry_flag_offset = cursor + 1;
        let Some((selector, cursor)) = marker_ref(bytes, cursor, 0) else {
            position += 1;
            continue;
        };
        if selector > 1 {
            position += 1;
            continue;
        }
        let display_name_offset = cursor + 4;
        let Some((display_name, cursor)) = lp_utf16_bounded(bytes, cursor, 0..=1024) else {
            position += 1;
            continue;
        };
        let mut components_marker = cursor;
        while bytes.get(components_marker) == Some(&0) && components_marker - cursor < 8 {
            components_marker += 1;
        }
        if components_marker == cursor {
            position += 1;
            continue;
        }
        let Some((components_root_record, end)) = marker_value(bytes, components_marker) else {
            position += 1;
            continue;
        };
        out.push(ActRootComponent {
            id: format!("f3d:{stream}:act-root-component#{position}"),
            byte_offset: position as u64,
            record_index: u32::from_le_bytes(record_raw.try_into().expect(
                "invariant: record_raw is a 4-byte slice from bytes.get(range) of length 4",
            )),
            record_index_offset: after_tag as u64,
            class_tag,
            instance_root_record,
            instance_root_record_offset: instance_root_record_offset as u64,
            components_root_record,
            components_root_record_offset: (components_marker + 1) as u64,
            registry_flag: selector,
            registry_flag_offset: registry_flag_offset as u64,
            entity_id,
            entity_id_offset: entity_id_offset as u64,
            display_name,
            display_name_offset: display_name_offset as u64,
        });
        position = end;
    }
    out
}

fn marker_ref(bytes: &[u8], position: usize, zero_count: usize) -> Option<(u32, usize)> {
    if bytes.get(position) != Some(&1) {
        return None;
    }
    let value = u32::from_le_bytes(bytes.get(position + 1..position + 5)?.try_into().ok()?);
    let end = position + 5 + zero_count;
    bytes
        .get(position + 5..end)?
        .iter()
        .all(|byte| *byte == 0)
        .then_some((value, end))
}

fn marker_value(bytes: &[u8], position: usize) -> Option<(u32, usize)> {
    if bytes.get(position) != Some(&1) {
        return None;
    }
    Some((
        u32::from_le_bytes(bytes.get(position + 1..position + 5)?.try_into().ok()?),
        position + 5,
    ))
}

struct TableEntry {
    record_index: u32,
    record_index_offset: usize,
    entity_id: String,
    entity_id_offset: usize,
}

fn decode_table(bytes: &[u8]) -> (Vec<TableEntry>, Vec<(String, usize)>) {
    let Some(name_at) = bytes.windows(8).position(|window| window == b"ACTTable") else {
        return (Vec::new(), Vec::new());
    };
    let mut cursor = name_at + 8;
    if bytes.get(cursor..cursor + 2) != Some(&[0, 0]) {
        return (Vec::new(), Vec::new());
    }
    cursor += 2;
    let Some(count_raw) = bytes.get(cursor..cursor + 4) else {
        return (Vec::new(), Vec::new());
    };
    let count = u32::from_le_bytes(
        count_raw
            .try_into()
            .expect("invariant: count_raw is a 4-byte slice from bytes.get(range) of length 4"),
    ) as usize;
    if count > 100_000 {
        return (Vec::new(), Vec::new());
    }
    cursor += 4;
    let mut indexed = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(cursor) != Some(&1) {
            return (Vec::new(), Vec::new());
        }
        let Some(index_raw) = bytes.get(cursor + 1..cursor + 5) else {
            return (Vec::new(), Vec::new());
        };
        if bytes.get(cursor + 5..cursor + 11) != Some(&[0; 6]) {
            return (Vec::new(), Vec::new());
        }
        let entity_id_offset = cursor + 15;
        let Some((entity_id, end)) = lp_utf16_bounded(bytes, cursor + 11, 0..=1024) else {
            return (Vec::new(), Vec::new());
        };
        indexed.push((
            u32::from_le_bytes(index_raw.try_into().expect(
                "invariant: index_raw is a 4-byte slice from bytes.get(range) of length 4",
            )),
            cursor + 1,
            entity_id,
            entity_id_offset,
        ));
        cursor = end;
    }
    let mut guids = Vec::new();
    while cursor + 4 <= bytes.len() {
        if let Some((guid, end)) =
            lp_utf16_bounded(bytes, cursor, 0..=1024).filter(|(value, _)| is_guid_hyphenated(value))
        {
            guids.push((guid, cursor));
            cursor = end;
        } else {
            cursor += 1;
        }
    }
    let entries = indexed
        .into_iter()
        .map(
            |(record_index, record_index_offset, entity_id, entity_id_offset)| TableEntry {
                record_index,
                record_index_offset,
                entity_id,
                entity_id_offset,
            },
        )
        .collect();
    (entries, guids)
}

struct ChannelGroup {
    record_index: u32,
    record_index_offset: usize,
    entity_id: String,
    entity_id_offset: usize,
    class_tag: String,
    channels: BTreeMap<String, String>,
    guid_offsets: BTreeMap<String, u64>,
}

fn decode_channel_groups(bytes: &[u8]) -> Vec<ChannelGroup> {
    let mut out = Vec::new();
    let mut position = 0usize;
    while position + 4 <= bytes.len() {
        let Some((class_tag, after_tag)) = lp_ascii_strict(bytes, position, 1..=128) else {
            position += 1;
            continue;
        };
        if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
            position += 1;
            continue;
        }
        let Some(index_raw) = bytes.get(after_tag..after_tag + 4) else {
            break;
        };
        if bytes.get(after_tag + 4..after_tag + 14) != Some(&[0; 10]) {
            position += 1;
            continue;
        }
        let Some(count_raw) = bytes.get(after_tag + 14..after_tag + 18) else {
            break;
        };
        let count = u32::from_le_bytes(
            count_raw
                .try_into()
                .expect("invariant: count_raw is a 4-byte slice from bytes.get(range) of length 4"),
        ) as usize;
        if !(1..=8).contains(&count) {
            position += 1;
            continue;
        }
        let mut cursor = after_tag + 18;
        let mut channels = BTreeMap::new();
        let mut guid_offsets = BTreeMap::new();
        for _ in 0..count {
            let Some((name, after_name)) = lp_ascii_strict(bytes, cursor, 1..=128) else {
                channels.clear();
                break;
            };
            let Some((guid, after_guid)) = lp_utf16_bounded(bytes, after_name, 0..=1024)
                .filter(|(v, _)| is_guid_hyphenated(v))
            else {
                channels.clear();
                break;
            };
            channels.insert(name, guid);
            guid_offsets.insert(
                channels
                    .last_key_value()
                    .expect("inserted channel")
                    .0
                    .clone(),
                (after_name + 4) as u64,
            );
            cursor = after_guid;
        }
        if !channels.is_empty() {
            if let Some((entity_id, end)) = lp_utf16_bounded(bytes, cursor, 0..=1024) {
                out.push(ChannelGroup {
                    record_index: u32::from_le_bytes(index_raw.try_into().expect(
                        "invariant: index_raw is a 4-byte slice from bytes.get(range) of length 4",
                    )),
                    record_index_offset: after_tag,
                    entity_id,
                    entity_id_offset: cursor + 4,
                    class_tag,
                    channels,
                    guid_offsets,
                });
                position = end;
                continue;
            }
        }
        position += 1;
    }
    out
}
