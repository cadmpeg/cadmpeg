// SPDX-License-Identifier: Apache-2.0
//! Fusion ACT entity table and change-version channel groups.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::bytes::{is_guid_hyphenated, lp_ascii_strict, lp_utf16_bounded};
use crate::container::{role, ContainerScan};
use crate::metastream::MetaStream;
use crate::records::{
    ActChannelGroup, ActEntity, ActEntityMembership, ActGuid, ActRegistryChannel, ActRootComponent,
    ActTableReference, ActTableRow,
};

pub struct DecodedAct {
    pub entities: Vec<ActEntity>,
    pub guids: Vec<ActGuid>,
    pub registry_channels: Vec<ActRegistryChannel>,
    pub root_components: Vec<ActRootComponent>,
    pub table_references: Vec<ActTableReference>,
    pub non_root_component_links: usize,
}

struct RecordFrame {
    start: usize,
    end: usize,
    record_index: u32,
    record_index_offset: usize,
    payload_offset: usize,
    class_tag: String,
}

fn decode_record_frames(
    bytes: &[u8],
    meta: &MetaStream,
    stream: &str,
) -> Result<Vec<RecordFrame>, CodecError> {
    if meta.records.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "F3D ACT MetaStream has no primary record index: {stream}"
        )));
    }
    if meta
        .records
        .windows(2)
        .any(|pair| pair[0].bulk_offset >= pair[1].bulk_offset)
    {
        return Err(CodecError::malformed(format_args!(
            "F3D ACT primary record offsets are not strictly increasing: {stream}"
        )));
    }

    let mut record_indices = BTreeSet::new();
    meta.records
        .iter()
        .enumerate()
        .map(|(ordinal, record)| {
            let start = usize::try_from(record.bulk_offset).map_err(|_| {
                CodecError::malformed(format_args!(
                    "F3D ACT record offset exceeds usize: {stream}"
                ))
            })?;
            let end = if let Some(next) = meta.records.get(ordinal + 1) {
                usize::try_from(next.bulk_offset).map_err(|_| {
                    CodecError::malformed(format_args!(
                        "F3D ACT record offset exceeds usize: {stream}"
                    ))
                })?
            } else {
                bytes.len()
            };
            if start >= end || end > bytes.len() {
                return Err(CodecError::malformed(format_args!(
                    "F3D ACT record extent is outside its BulkStream: {stream}"
                )));
            }
            let expected_index = u32::try_from(record.entity_id).map_err(|_| {
                CodecError::malformed(format_args!("F3D ACT record index exceeds u32: {stream}"))
            })?;
            if !record_indices.insert(expected_index) {
                return Err(CodecError::malformed(format_args!(
                    "duplicate F3D ACT primary record index {expected_index}: {stream}"
                )));
            }
            let (class_tag, after_tag) = lp_ascii_strict(bytes, start, 3..=3)
                .filter(|(tag, _)| tag.bytes().all(|byte| byte.is_ascii_digit()))
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "F3D ACT record lacks a dynamic class tag: {stream}@{start}"
                    ))
                })?;
            let payload_offset = after_tag.checked_add(4).ok_or_else(|| {
                CodecError::malformed(format_args!("F3D ACT record header overflows: {stream}"))
            })?;
            if payload_offset > end || View::u32_le_at(bytes, after_tag) != Some(expected_index) {
                return Err(CodecError::malformed(format_args!(
                    "F3D ACT record header conflicts with its MetaStream index: {stream}@{start}"
                )));
            }
            let class_index = class_tag
                .parse::<usize>()
                .ok()
                .and_then(|tag| tag.checked_sub(256))
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "F3D ACT class tag is outside the dynamic registry: {stream}@{start}"
                    ))
                })?;
            if !meta
                .types
                .get(class_index)
                .is_some_and(|record_type| record_type.entity_ids.contains(&record.entity_id))
            {
                return Err(CodecError::malformed(format_args!(
                    "F3D ACT class tag conflicts with its MetaStream type: {stream}@{start}"
                )));
            }
            Ok(RecordFrame {
                start,
                end,
                record_index: expected_index,
                record_index_offset: after_tag,
                payload_offset,
                class_tag,
            })
        })
        .collect()
}

fn sibling_meta_name(stream: &str) -> Option<String> {
    Some(format!(
        "{}MetaStream.dat",
        stream.strip_suffix("BulkStream.dat")?
    ))
}

pub fn decode(scan: &ContainerScan<'_>) -> Result<DecodedAct, CodecError> {
    let mut entities = Vec::new();
    let mut guids = Vec::new();
    let mut registry_channels = Vec::new();
    let mut root_components = Vec::new();
    let mut table_references = Vec::new();
    let mut non_root_component_links = 0usize;
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_act_stream(entry))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let meta_name = sibling_meta_name(&entry.name).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "F3D ACT BulkStream has no sibling MetaStream name: {}",
                entry.name
            ))
        })?;
        let meta_entry = scan
            .entries
            .iter()
            .find(|candidate| candidate.role == role::METASTREAM && candidate.name == meta_name)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "F3D ACT BulkStream has no sibling MetaStream: {}",
                    entry.name
                ))
            })?;
        let meta = crate::metastream::parse(scan.entry_bytes(&meta_entry.name)?, &meta_entry.name)?;
        let frames = decode_record_frames(bytes, &meta, &entry.name)?;
        let table_frames = frames
            .iter()
            .filter_map(|frame| table_payload_offset(bytes, frame).map(|payload| (frame, payload)))
            .collect::<Vec<_>>();
        let [(table_frame, table_payload)] = table_frames.as_slice() else {
            return Err(CodecError::malformed(format_args!(
                "F3D ACT segment must have exactly one indexed ACTTable record: {}",
                entry.name
            )));
        };
        let DecodedTable {
            entries: table,
            guids: stream_guids,
            references: stream_table_references,
            registry_channels: stream_registry_channels,
        } = decode_table(bytes, table_frame, *table_payload, &entry.name)?;
        let frame_indices = frames
            .iter()
            .map(|frame| frame.record_index)
            .collect::<BTreeSet<_>>();
        if let Some(reference) = stream_table_references
            .iter()
            .find(|reference| !frame_indices.contains(&reference.target_record))
        {
            return Err(CodecError::malformed(format_args!(
                "F3D ACTTable reference targets absent record {}: {}",
                reference.target_record, entry.name
            )));
        }
        let groups = frames
            .iter()
            .map(|frame| decode_channel_group(bytes, frame, &entry.name))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let links = frames
            .iter()
            .filter_map(|frame| decode_component_link(bytes, frame, &entry.name))
            .collect::<Vec<_>>();
        let stream_roots = links
            .iter()
            .filter(|link| link.tracked_entity_record == 3)
            .count();
        if !links.is_empty() && stream_roots != 1 {
            return Err(CodecError::malformed(format_args!(
                "F3D ACT segment does not have one root component link: {}",
                entry.name
            )));
        }
        non_root_component_links = non_root_component_links
            .checked_add(links.len().saturating_sub(stream_roots))
            .ok_or_else(|| {
                CodecError::Malformed("F3D ACT component-link count overflows".into())
            })?;
        root_components.extend(
            links
                .into_iter()
                .filter(|link| link.tracked_entity_record == 3),
        );

        entities.extend(merge_entities(&entry.name, table, groups)?);
        guids.extend(stream_guids);
        table_references.extend(stream_table_references);
        registry_channels.extend(stream_registry_channels);
    }
    Ok(DecodedAct {
        entities,
        guids,
        registry_channels,
        root_components,
        table_references,
        non_root_component_links,
    })
}

fn table_payload_offset(bytes: &[u8], frame: &RecordFrame) -> Option<usize> {
    let name_offset = frame.payload_offset.checked_add(4)?;
    if name_offset > frame.end || bytes.get(frame.payload_offset..name_offset)? != [0; 4] {
        return None;
    }
    let (name, payload) = lp_ascii_strict(bytes, name_offset, 1..=128)?;
    (name == "ACTTable" && payload <= frame.end).then_some(payload)
}

struct TableEntry {
    record_index: u32,
    record_index_offset: usize,
    entity_id: String,
    entity_id_offset: usize,
}

struct DecodedTable {
    entries: Vec<TableEntry>,
    guids: Vec<ActGuid>,
    references: Vec<ActTableReference>,
    registry_channels: Vec<ActRegistryChannel>,
}

fn decode_table(
    bytes: &[u8],
    frame: &RecordFrame,
    payload: usize,
    stream: &str,
) -> Result<DecodedTable, CodecError> {
    let malformed = |detail: &str| {
        CodecError::malformed(format_args!(
            "invalid F3D ACTTable {detail}: {stream}@{}",
            frame.start
        ))
    };
    let count_offset = payload.checked_add(2).ok_or_else(|| malformed("offset"))?;
    let mut cursor = count_offset
        .checked_add(4)
        .ok_or_else(|| malformed("offset"))?;
    if cursor > frame.end || bytes.get(payload..count_offset) != Some(&[0, 0]) {
        return Err(malformed("prologue"));
    }
    let count =
        usize::try_from(View::u32_le_at(bytes, count_offset).ok_or_else(|| malformed("count"))?)
            .map_err(|_| malformed("count"))?;
    if count > frame.end.saturating_sub(cursor) / 15 {
        return Err(malformed("entry count"));
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let index_offset = cursor.checked_add(1).ok_or_else(|| malformed("entry"))?;
        let entity_length_offset = cursor.checked_add(11).ok_or_else(|| malformed("entry"))?;
        if bytes.get(cursor) != Some(&1)
            || bytes.get(cursor + 5..entity_length_offset) != Some(&[0; 6])
        {
            return Err(malformed("entry reference"));
        }
        let record_index =
            View::u32_le_at(bytes, index_offset).ok_or_else(|| malformed("entry index"))?;
        let entity_id_offset = entity_length_offset
            .checked_add(4)
            .ok_or_else(|| malformed("entity key"))?;
        let (entity_id, end) = lp_utf16_bounded(bytes, entity_length_offset, 1..=1024)
            .filter(|(_, end)| *end <= frame.end)
            .filter(|(entity_id, _)| is_entity_key(entity_id))
            .ok_or_else(|| malformed("entity key"))?;
        entries.push(TableEntry {
            record_index,
            record_index_offset: index_offset,
            entity_id,
            entity_id_offset,
        });
        cursor = end;
    }

    let mut guids = Vec::new();
    while let Some((guid, end)) = lp_utf16_bounded(bytes, cursor, 36..=36)
        .filter(|(guid, end)| *end <= frame.end && is_guid_hyphenated(guid))
    {
        let byte_offset = cursor;
        let ordinal = u32::try_from(guids.len()).map_err(|_| malformed("GUID ordinal"))?;
        guids.push(ActGuid {
            id: crate::ids::native_scoped_id(stream, "act-guid", byte_offset),
            byte_offset: byte_offset as u64,
            guid_offset: byte_offset
                .checked_add(4)
                .ok_or_else(|| malformed("GUID offset"))? as u64,
            ordinal,
            guid,
        });
        cursor = end;
    }

    let reference_count = usize::try_from(
        View::u32_le_at(bytes, cursor).ok_or_else(|| malformed("table-reference count"))?,
    )
    .map_err(|_| malformed("table-reference count"))?;
    cursor = cursor
        .checked_add(4)
        .ok_or_else(|| malformed("table-reference count"))?;
    if reference_count > frame.end.saturating_sub(cursor) / 11 {
        return Err(malformed("table-reference count"));
    }
    let mut table_references = Vec::with_capacity(reference_count);
    for ordinal in 0..reference_count {
        let byte_offset = cursor;
        let target_record_offset = cursor
            .checked_add(1)
            .ok_or_else(|| malformed("table reference"))?;
        let (target_record, end) =
            marker_ref(bytes, cursor, 6, frame.end).ok_or_else(|| malformed("table reference"))?;
        table_references.push(ActTableReference {
            id: crate::ids::native_scoped_id(stream, "act-table-reference", byte_offset),
            ordinal: u32::try_from(ordinal).map_err(|_| malformed("table-reference ordinal"))?,
            byte_offset: byte_offset as u64,
            target_record,
            target_record_offset: target_record_offset as u64,
        });
        cursor = end;
    }

    let registry_count = usize::try_from(
        View::u32_le_at(bytes, cursor).ok_or_else(|| malformed("channel-registry count"))?,
    )
    .map_err(|_| malformed("channel-registry count"))?;
    cursor = cursor
        .checked_add(4)
        .ok_or_else(|| malformed("channel-registry count"))?;
    if registry_count > frame.end.saturating_sub(cursor) / 81 {
        return Err(malformed("channel-registry count"));
    }
    let mut registry_names = BTreeSet::new();
    let mut registry_channels = Vec::with_capacity(registry_count);
    for ordinal in 0..registry_count {
        let byte_offset = cursor;
        let (name, after_name) = lp_ascii_strict(bytes, cursor, 1..=128)
            .filter(|(name, end)| *end <= frame.end && name.is_ascii())
            .ok_or_else(|| malformed("channel-registry name"))?;
        if !registry_names.insert(name.clone()) {
            return Err(malformed("duplicate channel-registry name"));
        }
        let (guid, end) = lp_utf16_bounded(bytes, after_name, 36..=36)
            .filter(|(guid, end)| *end <= frame.end && is_guid_hyphenated(guid))
            .ok_or_else(|| malformed("channel-registry GUID"))?;
        registry_channels.push(ActRegistryChannel {
            id: crate::ids::native_scoped_id(stream, "act-registry-channel", byte_offset),
            ordinal: u32::try_from(ordinal).map_err(|_| malformed("channel-registry ordinal"))?,
            byte_offset: byte_offset as u64,
            name,
            name_offset: byte_offset
                .checked_add(4)
                .ok_or_else(|| malformed("channel-registry name offset"))?
                as u64,
            guid,
            guid_offset: after_name
                .checked_add(4)
                .ok_or_else(|| malformed("channel-registry GUID offset"))?
                as u64,
        });
        cursor = end;
    }
    if cursor != frame.end {
        return Err(malformed("channel-registry extent"));
    }
    Ok(DecodedTable {
        entries,
        guids,
        references: table_references,
        registry_channels,
    })
}

struct ChannelGroup {
    record_index: u32,
    record_index_offset: usize,
    entity_id: Option<String>,
    entity_id_offset: Option<usize>,
    class_tag: String,
    channels: BTreeMap<String, String>,
    guid_offsets: BTreeMap<String, u64>,
    class_tail: Vec<u8>,
    class_tail_offset: Option<u64>,
}

fn merge_entities(
    stream: &str,
    table: Vec<TableEntry>,
    groups: Vec<ChannelGroup>,
) -> Result<Vec<ActEntity>, CodecError> {
    let mut by_index: BTreeMap<u32, ActEntity> = BTreeMap::new();
    for item in table {
        let record_index = item.record_index;
        let entity = ActEntity {
            id: crate::ids::native_scoped_id(stream, "act-entity", record_index),
            record_index,
            entity_id: item.entity_id,
            membership: ActEntityMembership::TableOnly(ActTableRow {
                record_index_offset: item.record_index_offset as u64,
                entity_id_offset: item.entity_id_offset as u64,
            }),
        };
        if by_index.insert(record_index, entity).is_some() {
            return Err(CodecError::malformed(format_args!(
                "duplicate F3D ACTTable change-group reference {record_index}: {stream}"
            )));
        }
    }
    for group in groups {
        if let Some(entity) = by_index.get_mut(&group.record_index) {
            if group
                .entity_id
                .as_ref()
                .is_some_and(|group_id| entity.entity_id != *group_id)
            {
                return Err(CodecError::malformed(format_args!(
                    "F3D ACTTable entity key conflicts with its change group: {stream}:{}",
                    group.record_index
                )));
            }
            let attached = ActChannelGroup {
                record_index_offset: group.record_index_offset as u64,
                entity_id_offset: group.entity_id_offset.map(|offset| offset as u64),
                class_tag: group.class_tag,
                channels: group.channels,
                guid_offsets: group.guid_offsets,
                class_tail: group.class_tail,
                class_tail_offset: group.class_tail_offset,
            };
            if !entity.attach_channel_group(attached) {
                return Err(CodecError::malformed(format_args!(
                    "duplicate F3D ACT change group {stream}:{}",
                    group.record_index
                )));
            }
        } else if let Some(entity_id) = group.entity_id {
            by_index.insert(
                group.record_index,
                ActEntity {
                    id: crate::ids::native_scoped_id(stream, "act-entity", group.record_index),
                    record_index: group.record_index,
                    entity_id,
                    membership: ActEntityMembership::GroupOnly(ActChannelGroup {
                        record_index_offset: group.record_index_offset as u64,
                        entity_id_offset: group.entity_id_offset.map(|offset| offset as u64),
                        class_tag: group.class_tag,
                        channels: group.channels,
                        guid_offsets: group.guid_offsets,
                        class_tail: group.class_tail,
                        class_tail_offset: group.class_tail_offset,
                    }),
                },
            );
        }
    }
    if let Some(entity) = by_index
        .values()
        .find(|entity| entity.channel_group().is_none())
    {
        return Err(CodecError::malformed(format_args!(
            "F3D ACTTable reference has no change group: {stream}:{}",
            entity.record_index
        )));
    }
    Ok(by_index.into_values().collect())
}

fn decode_channel_group(
    bytes: &[u8],
    frame: &RecordFrame,
    stream: &str,
) -> Result<Option<ChannelGroup>, CodecError> {
    let count_offset = frame.payload_offset.checked_add(10).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "F3D ACT channel-group offset overflows: {stream}"
        ))
    })?;
    if count_offset
        .checked_add(4)
        .is_none_or(|end| end > frame.end)
        || bytes.get(frame.payload_offset..count_offset) != Some(&[0; 10])
    {
        return Ok(None);
    }
    let Some(count) = View::u32_le_at(bytes, count_offset).filter(|count| (1..=8).contains(count))
    else {
        return Ok(None);
    };
    let mut cursor = count_offset + 4;
    let mut channels = BTreeMap::new();
    let mut guid_offsets = BTreeMap::new();
    for _ in 0..count {
        let Some((name, after_name)) = lp_ascii_strict(bytes, cursor, 1..=128)
            .filter(|(name, after)| *after <= frame.end && name.is_ascii())
        else {
            return Ok(None);
        };
        let Some((guid, after_guid)) = lp_utf16_bounded(bytes, after_name, 36..=36)
            .filter(|(guid, after)| *after <= frame.end && is_guid_hyphenated(guid))
        else {
            return Ok(None);
        };
        if channels.insert(name.clone(), guid).is_some() {
            return Err(CodecError::malformed(format_args!(
                "duplicate F3D ACT channel {name:?}: {stream}@{}",
                frame.start
            )));
        }
        guid_offsets.insert(name, (after_name + 4) as u64);
        cursor = after_guid;
    }
    let (entity_id, entity_id_offset, end) = if let Some((entity_id, end)) =
        lp_utf16_bounded(bytes, cursor, 1..=1024)
            .filter(|(entity_id, end)| *end <= frame.end && is_entity_key(entity_id))
    {
        (Some(entity_id), Some(cursor + 4), end)
    } else {
        (None, None, cursor)
    };
    let remainder = &bytes[end..frame.end];
    let (class_tail, class_tail_offset) = if remainder.iter().all(|byte| *byte == 0) {
        (Vec::new(), None)
    } else {
        (remainder.to_vec(), Some(end as u64))
    };
    Ok(Some(ChannelGroup {
        record_index: frame.record_index,
        record_index_offset: frame.record_index_offset,
        entity_id,
        entity_id_offset,
        class_tag: frame.class_tag.clone(),
        channels,
        guid_offsets,
        class_tail,
        class_tail_offset,
    }))
}

fn decode_component_link(
    bytes: &[u8],
    frame: &RecordFrame,
    stream: &str,
) -> Option<ActRootComponent> {
    let mut cursor = frame.payload_offset.checked_add(10)?;
    if cursor > frame.end || bytes.get(frame.payload_offset..cursor)? != [0; 10] {
        return None;
    }
    let instance_root_record_offset = cursor.checked_add(1)?;
    let (instance_root_record, next) = marker_ref(bytes, cursor, 6, frame.end)?;
    cursor = next;
    let entity_id_offset = cursor.checked_add(4)?;
    let (entity_id, next) = lp_utf16_bounded(bytes, cursor, 1..=1024)?;
    if next > frame.end || !is_entity_key(&entity_id) {
        return None;
    }
    cursor = next;
    let tracked_entity_record_offset = cursor.checked_add(1)?;
    let (tracked_entity_record, next) = marker_ref(bytes, cursor, 5, frame.end)?;
    cursor = next;
    let registry_flag_offset = cursor.checked_add(1)?;
    let (registry_flag, next) = marker_ref(bytes, cursor, 0, frame.end)?;
    cursor = next;
    let display_name_offset = cursor.checked_add(4)?;
    let (display_name, next) = lp_utf16_bounded(bytes, cursor, 0..=1024)?;
    if next > frame.end {
        return None;
    }
    cursor = next;
    let mut components_marker = cursor;
    while components_marker < frame.end
        && bytes.get(components_marker) == Some(&0)
        && components_marker - cursor < 8
    {
        components_marker += 1;
    }
    if components_marker == cursor {
        return None;
    }
    let (components_root_record, end) = marker_value(bytes, components_marker, frame.end)?;
    if !bytes.get(end..frame.end)?.iter().all(|byte| *byte == 0) {
        return None;
    }
    Some(ActRootComponent {
        id: crate::ids::native_scoped_id(stream, "act-root-component", frame.start),
        byte_offset: frame.start as u64,
        record_index: frame.record_index,
        record_index_offset: frame.record_index_offset as u64,
        class_tag: frame.class_tag.clone(),
        instance_root_record,
        instance_root_record_offset: instance_root_record_offset as u64,
        tracked_entity_record,
        tracked_entity_record_offset: tracked_entity_record_offset as u64,
        components_root_record,
        components_root_record_offset: (components_marker + 1) as u64,
        registry_flag: crate::records::ActRegistryFlag::from_code(registry_flag),
        registry_flag_offset: registry_flag_offset as u64,
        entity_id,
        entity_id_offset: entity_id_offset as u64,
        display_name,
        display_name_offset: display_name_offset as u64,
    })
}

/// Whether `key` has the ACT entity-key form `<segment id>_<entity id>`.
pub(crate) fn is_entity_key(key: &str) -> bool {
    let Some((segment, entity)) = key.split_once('_') else {
        return false;
    };
    !segment.is_empty()
        && !entity.is_empty()
        && segment.bytes().all(|byte| byte.is_ascii_digit())
        && entity.bytes().all(|byte| byte.is_ascii_digit())
}

fn marker_ref(
    bytes: &[u8],
    position: usize,
    zero_count: usize,
    frame_end: usize,
) -> Option<(u32, usize)> {
    if bytes.get(position) != Some(&1) {
        return None;
    }
    let value = View::u32_le_at(bytes, position + 1)?;
    let end = position.checked_add(5)?.checked_add(zero_count)?;
    if end > frame_end {
        return None;
    }
    bytes
        .get(position + 5..end)?
        .iter()
        .all(|byte| *byte == 0)
        .then_some((value, end))
}

fn marker_value(bytes: &[u8], position: usize, frame_end: usize) -> Option<(u32, usize)> {
    if bytes.get(position) != Some(&1) || position.checked_add(5)? > frame_end {
        return None;
    }
    Some((View::u32_le_at(bytes, position + 1)?, position + 5))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lp_ascii(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    fn lp_utf16(out: &mut Vec<u8>, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        out.extend_from_slice(&(units.len() as u32).to_le_bytes());
        out.extend(units.into_iter().flat_map(u16::to_le_bytes));
    }

    fn table_entry(entity_id: &str) -> TableEntry {
        TableEntry {
            record_index: 7,
            record_index_offset: 20,
            entity_id: entity_id.into(),
            entity_id_offset: 40,
        }
    }

    fn channel_group(entity_id: &str) -> ChannelGroup {
        ChannelGroup {
            record_index: 7,
            record_index_offset: 100,
            entity_id: Some(entity_id.into()),
            entity_id_offset: Some(200),
            class_tag: "261".into(),
            channels: BTreeMap::from([(
                "Appearance".into(),
                "11111111-2222-3333-4444-555555555555".into(),
            )]),
            guid_offsets: BTreeMap::from([("Appearance".into(), 120)]),
            class_tail: Vec::new(),
            class_tail_offset: None,
        }
    }

    #[test]
    fn record_index_joins_exactly_one_matching_act_change_group() {
        let stream = "Synthetic/FusionACTSegmentType1/BulkStream.dat";
        let entities = merge_entities(
            stream,
            vec![table_entry("0_985")],
            vec![channel_group("0_985")],
        )
        .expect("matching table and change group");
        assert_eq!(entities.len(), 1);
        assert!(entities[0].in_table());
        assert_eq!(entities[0].record_index, 7);

        let mismatch = merge_entities(
            stream,
            vec![table_entry("0_985")],
            vec![channel_group("0_986")],
        )
        .expect_err("table and change-group keys must agree");
        assert!(mismatch.to_string().contains("entity key conflicts"));

        let duplicate = merge_entities(
            stream,
            vec![table_entry("0_985")],
            vec![channel_group("0_985"), channel_group("0_985")],
        )
        .expect_err("one record index cannot own two change groups");
        assert!(duplicate
            .to_string()
            .contains("duplicate F3D ACT change group"));

        let table_only = merge_entities(stream, vec![table_entry("0_985")], Vec::new())
            .expect_err("every table reference must resolve to a change group");
        assert!(table_only.to_string().contains("has no change group"));

        let group_only = merge_entities(stream, Vec::new(), vec![channel_group("0_985")])
            .expect("a change group need not have an inline ACTTable row");
        assert!(!group_only[0].in_table());
        assert!(group_only[0].table_record_index_offset().is_none());

        let mut table_keyed_group = channel_group("0_985");
        table_keyed_group.entity_id = None;
        table_keyed_group.entity_id_offset = None;
        let entities = merge_entities(stream, vec![table_entry("0_985")], vec![table_keyed_group])
            .expect("the table can supply an omitted group key");
        assert_eq!(entities[0].entity_id, "0_985");
        assert!(entities[0].channel_entity_id_offset().is_none());
    }

    #[test]
    fn channel_group_distinguishes_zero_padding_from_a_class_tail() {
        let payload_offset = 11;
        let mut bytes = vec![0; payload_offset + 10];
        bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_ascii(&mut bytes, "Appearance");
        lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
        let entity_at = bytes.len();
        lp_utf16(&mut bytes, "0_985");
        let tail_at = bytes.len();
        bytes.extend_from_slice(&[0; 11]);
        let mut frame = RecordFrame {
            start: 0,
            end: bytes.len(),
            record_index: 7,
            record_index_offset: 7,
            payload_offset,
            class_tag: "261".into(),
        };

        let group = decode_channel_group(&bytes, &frame, "synthetic")
            .expect("well-framed group")
            .expect("zero padding belongs to the group frame");
        assert_eq!(group.record_index, 7);
        assert_eq!(group.entity_id.as_deref(), Some("0_985"));
        assert!(group.class_tail.is_empty());
        assert!(group.class_tail_offset.is_none());

        let keyless_frame = RecordFrame {
            start: frame.start,
            end: entity_at,
            record_index: frame.record_index,
            record_index_offset: frame.record_index_offset,
            payload_offset: frame.payload_offset,
            class_tag: frame.class_tag.clone(),
        };
        let keyless = decode_channel_group(&bytes, &keyless_frame, "synthetic")
            .expect("well-framed keyless group")
            .expect("table-keyed group");
        assert!(keyless.entity_id.is_none());

        let class_tail = b"\0synthetic-class-tail\x01";
        bytes.truncate(tail_at);
        bytes.extend_from_slice(class_tail);
        frame.end = bytes.len();
        let group = decode_channel_group(&bytes, &frame, "synthetic")
            .expect("well-framed group with a class tail")
            .expect("class tail follows the complete channel grammar");
        assert_eq!(group.class_tail, class_tail);
        assert_eq!(group.class_tail_offset, Some(tail_at as u64));
    }
}
