// SPDX-License-Identifier: Apache-2.0
//! Fusion ACT entity table and change-version channel groups.

use cadmpeg_core::decode::u64_from_index;

use cadmpeg_core::container::ContainerRole;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::bytes::{is_guid_hyphenated, lp_ascii_strict_charged, lp_utf16_bounded_charged};
use crate::container::ContainerScan;
use crate::metastream::MetaStream;
use crate::records::{
    act::{
        ActChannelGroup, ActClassTail, ActEntity, ActGuid, ActRegistryChannel, ActRootComponent,
        ActTableReference, ActTableRow,
    },
    identity::Located,
    references::DesignClassTag,
};

pub(crate) struct DecodedAct {
    pub(crate) entities: Vec<ActEntity>,
    pub(crate) guids: Vec<ActGuid>,
    pub(crate) registry_channels: Vec<ActRegistryChannel>,
    pub(crate) root_components: Vec<ActRootComponent>,
    pub(crate) table_references: Vec<ActTableReference>,
    pub(crate) non_root_component_links: usize,
}

struct RecordFrame {
    start: usize,
    end: usize,
    record_index: u32,
    record_index_offset: usize,
    payload_offset: usize,
    class_tag: DesignClassTag,
}

fn decode_record_frames(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &MetaStream,
    stream: &str,
) -> Result<Vec<RecordFrame>, CodecError> {
    if meta.records.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "F3D ACT MetaStream has no primary record index: {stream}"
        )));
    }
    let mut previous_offset = None;
    for record in ctx.admit_iter(&meta.records, "validate F3D ACT record offset order")? {
        if previous_offset.is_some_and(|previous| previous >= record.bulk_offset) {
            return Err(CodecError::malformed(format_args!(
                "F3D ACT primary record offsets are not strictly increasing: {stream}"
            )));
        }
        previous_offset = Some(record.bulk_offset);
    }

    let mut record_indices = BTreeSet::new();
    ctx.try_collect_vec(
        meta.records.iter().enumerate().map(|(ordinal, record)| {
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
            if !ctx.insert_btree_set(
                &mut record_indices,
                expected_index,
                "index F3D ACT records",
            )? {
                return Err(CodecError::malformed(format_args!(
                    "duplicate F3D ACT primary record index {expected_index}: {stream}"
                )));
            }
            let (class_tag, after_tag) = lp_ascii_strict_charged(ctx, bytes, start, 3..=3)?
                .and_then(|(tag, after)| DesignClassTag::try_from(tag).ok().map(|tag| (tag, after)))
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
            let class_index = class_tag.dynamic_ordinal().ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "F3D ACT class tag is outside the dynamic registry: {stream}@{start}"
                ))
            })?;
            let mut registered = false;
            if let Some(record_type) = meta.types.get(class_index) {
                let (entity_ids, _) = record_type.entities.storage_slices();
                if let Some(located_entities) = record_type.entities.located_rows() {
                    for row in
                        ctx.admit_iter(located_entities, "check F3D ACT entity registration")?
                    {
                        if ctx.equal(
                            &row.value,
                            &record.entity_id,
                            "compare F3D ACT entity registration",
                        )? {
                            registered = true;
                            break;
                        }
                    }
                } else {
                    for registered_id in
                        ctx.admit_iter(entity_ids, "check F3D ACT entity registration")?
                    {
                        if ctx.equal(
                            registered_id,
                            &record.entity_id,
                            "compare F3D ACT entity registration",
                        )? {
                            registered = true;
                            break;
                        }
                    }
                }
            }
            if !registered {
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
        }),
        "frame F3D ACT records",
    )
}

fn sibling_meta_name(ctx: &DecodeContext<'_>, stream: &str) -> Result<Option<String>, CodecError> {
    let Some(prefix) =
        ctx.strip_suffix(stream, "BulkStream.dat", "find F3D ACT MetaStream suffix")?
    else {
        return Ok(None);
    };
    ctx.format_retained(
        format_args!("{prefix}MetaStream.dat"),
        "name F3D ACT MetaStream",
    )
    .map(Some)
}

pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<DecodedAct, CodecError> {
    let mut entities = Vec::new();
    let mut guids = Vec::new();
    let mut registry_channels = Vec::new();
    let mut root_components = Vec::new();
    let mut table_references = Vec::new();
    let mut non_root_component_links = 0usize;
    for entry in ctx.admit_iter(&scan.entries, "scan F3D ACT stream entries")? {
        if !scan.is_act_stream(ctx, entry)? {
            continue;
        }
        let bytes = scan.entry_bytes(ctx, &entry.name)?;
        let meta_name = sibling_meta_name(ctx, &entry.name)?.ok_or_else(|| {
            CodecError::malformed(format_args!(
                "F3D ACT BulkStream has no sibling MetaStream name: {}",
                entry.name
            ))
        })?;
        let mut meta_entry = None;
        for candidate in ctx.admit_iter(&scan.entries, "find F3D ACT sibling MetaStream")? {
            if candidate.role == ContainerRole::Metastream
                && ctx.equal(
                    &candidate.name,
                    &meta_name,
                    "compare F3D ACT MetaStream names",
                )?
            {
                meta_entry = Some(candidate);
                break;
            }
        }
        let meta_entry = meta_entry.ok_or_else(|| {
            CodecError::malformed(format_args!(
                "F3D ACT BulkStream has no sibling MetaStream: {}",
                entry.name
            ))
        })?;
        let meta = crate::metastream::parse(
            ctx,
            scan.entry_bytes(ctx, &meta_entry.name)?,
            &meta_entry.name,
        )?;
        let frames = decode_record_frames(ctx, bytes, &meta, &entry.name)?;
        let mut selected_table = None;
        let mut table_count = 0usize;
        for frame in ctx.admit_iter(&frames, "select F3D ACT table frames")? {
            if let Some(payload) = table_payload_offset(bytes, frame) {
                table_count = table_count.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit("select F3D ACT table frames", 0, u64::MAX)
                })?;
                if selected_table.is_none() {
                    selected_table = Some((frame, payload));
                }
            }
        }
        let Some((table_frame, table_payload)) = selected_table.filter(|_| table_count == 1) else {
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
        } = decode_table(ctx, bytes, table_frame, table_payload, &entry.name)?;
        let mut frame_indices = BTreeSet::new();
        for frame in ctx.admit_iter(&frames, "index F3D ACT frame records")? {
            ctx.insert_btree_set(
                &mut frame_indices,
                frame.record_index,
                "index F3D ACT frame records",
            )?;
        }
        for reference in ctx.admit_iter(
            &stream_table_references,
            "check F3D ACT table-reference targets",
        )? {
            if !ctx.contains_btree_set(
                &frame_indices,
                &reference.target_record,
                "find F3D ACT target record",
            )? {
                return Err(CodecError::malformed(format_args!(
                    "F3D ACTTable reference targets absent record {}: {}",
                    reference.target_record, entry.name
                )));
            }
        }
        let mut groups = Vec::new();
        for frame in ctx.admit_iter(&frames, "decode F3D ACT channel groups")? {
            if let Some(group) = decode_channel_group(ctx, bytes, frame, &entry.name)? {
                ctx.push_vec(&mut groups, group, "collect F3D ACT channel groups")?;
            }
        }
        let mut links = Vec::new();
        for frame in ctx.admit_iter(&frames, "decode F3D ACT component links")? {
            if let Some(link) = decode_component_link(ctx, bytes, frame, &entry.name)? {
                ctx.push_vec(&mut links, link, "collect F3D ACT component links")?;
            }
        }
        let stream_roots = ctx
            .admit_iter(&links, "count F3D ACT root links")?
            .filter(|link| matches!(link, ComponentLink::Root(_)))
            .count();
        if !links.is_empty() && stream_roots != 1 {
            return Err(CodecError::malformed(format_args!(
                "F3D ACT segment does not have one root component link: {}",
                entry.name
            )));
        }
        non_root_component_links = non_root_component_links
            .checked_add(links.len().checked_sub(stream_roots).ok_or_else(|| {
                CodecError::Malformed("F3D ACT root count exceeds component links".into())
            })?)
            .ok_or_else(|| {
                CodecError::Malformed("F3D ACT component-link count overflows".into())
            })?;
        for link in links {
            if let ComponentLink::Root(root) = link {
                ctx.push_vec(&mut root_components, root, "collect F3D ACT roots")?;
            }
        }

        let merged_entities = merge_entities(ctx, &entry.name, table, groups)?;
        ctx.extend_vec(&mut entities, merged_entities, "collect F3D ACT entities")?;
        ctx.extend_vec(&mut guids, stream_guids, "collect F3D ACT GUIDs")?;
        ctx.extend_vec(
            &mut table_references,
            stream_table_references,
            "collect F3D ACT table references",
        )?;
        ctx.extend_vec(
            &mut registry_channels,
            stream_registry_channels,
            "collect F3D ACT registry channels",
        )?;
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
    let name_len = usize::try_from(View::u32_le_at(bytes, name_offset)?).ok()?;
    if name_len != b"ACTTable".len() {
        return None;
    }
    let name_start = name_offset.checked_add(4)?;
    let payload = name_start.checked_add(name_len)?;
    (bytes.get(name_start..payload)? == b"ACTTable" && payload <= frame.end).then_some(payload)
}

struct TableEntry {
    record_index: u32,
    row: ActTableRow,
    entity_id: String,
}

struct DecodedTable {
    entries: Vec<TableEntry>,
    guids: Vec<ActGuid>,
    references: Vec<ActTableReference>,
    registry_channels: Vec<ActRegistryChannel>,
}

fn decode_table(
    ctx: &DecodeContext<'_>,
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
    if frame
        .end
        .checked_sub(cursor)
        .is_none_or(|left| count > left / 15)
    {
        return Err(malformed("entry count"));
    }
    let mut entries = Vec::new();
    for _ in ctx.admit_iter(&(0..count), "scan F3D ACT table entries")? {
        let index_offset = cursor.checked_add(1).ok_or_else(|| malformed("entry"))?;
        let entity_length_offset = cursor.checked_add(11).ok_or_else(|| malformed("entry"))?;
        if bytes.get(cursor) != Some(&1)
            || bytes.get(cursor + 5..entity_length_offset) != Some(&[0; 6])
        {
            return Err(malformed("entry reference"));
        }
        let record_index =
            View::u32_le_at(bytes, index_offset).ok_or_else(|| malformed("entry index"))?;
        let (entity_id, end) = lp_utf16_bounded_charged(
            ctx,
            bytes,
            entity_length_offset,
            1..=1024,
            "retain F3D UTF-16 string",
        )?
        .filter(|(_, end)| *end <= frame.end)
        .ok_or_else(|| malformed("entity key"))?;
        if !is_entity_key(&entity_id) {
            return Err(malformed("entity key"));
        }
        ctx.push_vec(
            &mut entries,
            TableEntry {
                record_index,
                row: ActTableRow::new(u64_from_index(index_offset))
                    .map_err(CodecError::malformed)?,
                entity_id,
            },
            "collect F3D ACT table entries",
        )?;
        cursor = end;
    }

    let mut guids = Vec::new();
    loop {
        ctx.charge_work(1, "scan F3D ACT GUID run")?;
        let Some((guid, end)) =
            lp_utf16_bounded_charged(ctx, bytes, cursor, 36..=36, "retain F3D UTF-16 string")?
        else {
            break;
        };
        if end > frame.end || !is_guid_hyphenated(&guid) {
            break;
        }
        let byte_offset = cursor;
        let ordinal = u32::try_from(guids.len()).map_err(|_| malformed("GUID ordinal"))?;
        ctx.push_vec(
            &mut guids,
            ActGuid::new(
                crate::ids::native_scoped_id(ctx, stream, "act-guid", byte_offset)?,
                u64_from_index(byte_offset),
                ordinal,
                guid,
            )
            .map_err(|_| malformed("GUID offset"))?,
            "collect F3D ACT GUID run",
        )?;
        cursor = end;
    }

    let reference_count = usize::try_from(
        View::u32_le_at(bytes, cursor).ok_or_else(|| malformed("table-reference count"))?,
    )
    .map_err(|_| malformed("table-reference count"))?;
    cursor = cursor
        .checked_add(4)
        .ok_or_else(|| malformed("table-reference count"))?;
    if frame
        .end
        .checked_sub(cursor)
        .is_none_or(|left| reference_count > left / 11)
    {
        return Err(malformed("table-reference count"));
    }
    let mut table_references = Vec::new();
    for ordinal in ctx.admit_iter(&(0..reference_count), "scan F3D ACT table references")? {
        let byte_offset = cursor;
        let (target_record, end) = marker_ref(ctx, bytes, cursor, 6, frame.end)?
            .ok_or_else(|| malformed("table reference"))?;
        ctx.push_vec(
            &mut table_references,
            ActTableReference::new(
                crate::ids::native_scoped_id(ctx, stream, "act-table-reference", byte_offset)?,
                u32::try_from(ordinal).map_err(|_| malformed("table-reference ordinal"))?,
                u64_from_index(byte_offset),
                target_record,
            )
            .map_err(CodecError::malformed)?,
            "collect F3D ACT table references",
        )?;
        cursor = end;
    }

    let registry_count = usize::try_from(
        View::u32_le_at(bytes, cursor).ok_or_else(|| malformed("channel-registry count"))?,
    )
    .map_err(|_| malformed("channel-registry count"))?;
    cursor = cursor
        .checked_add(4)
        .ok_or_else(|| malformed("channel-registry count"))?;
    if frame
        .end
        .checked_sub(cursor)
        .is_none_or(|left| registry_count > left / 81)
    {
        return Err(malformed("channel-registry count"));
    }
    let mut registry_names = BTreeSet::new();
    let mut registry_channels = Vec::new();
    for ordinal in ctx.admit_iter(&(0..registry_count), "scan F3D ACT registry channels")? {
        let byte_offset = cursor;
        let (name, after_name) = lp_ascii_strict_charged(ctx, bytes, cursor, 1..=128)?
            .ok_or_else(|| malformed("channel-registry name"))?;
        if after_name > frame.end
            || !ctx.is_ascii(name.as_bytes(), "validate F3D ACT registry-name ASCII")?
        {
            return Err(malformed("channel-registry name"));
        }
        if ctx.contains_btree_set(
            &registry_names,
            &name,
            "check duplicate F3D ACT registry name",
        )? {
            return Err(malformed("duplicate channel-registry name"));
        }
        ctx.insert_btree_set(
            &mut registry_names,
            ctx.copy_retained_text(&name, "index F3D ACT registry name")?,
            "index F3D ACT registry names",
        )?;
        let (guid, end) =
            lp_utf16_bounded_charged(ctx, bytes, after_name, 36..=36, "retain F3D UTF-16 string")?
                .filter(|(guid, end)| *end <= frame.end && is_guid_hyphenated(guid))
                .ok_or_else(|| malformed("channel-registry GUID"))?;
        ctx.push_vec(
            &mut registry_channels,
            ActRegistryChannel::new(
                crate::ids::native_scoped_id(ctx, stream, "act-registry-channel", byte_offset)?,
                u32::try_from(ordinal).map_err(|_| malformed("channel-registry ordinal"))?,
                u64_from_index(byte_offset),
                name,
                guid,
            )
            .map_err(|_| malformed("channel-registry entry"))?,
            "collect F3D ACT registry channels",
        )?;
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
    entity_id: Option<Located<String, usize>>,
    class_tag: DesignClassTag,
    channels: BTreeMap<String, Located<crate::records::mesh::DesignGuidText>>,
    class_tail: Option<ActClassTail>,
}

fn merge_entities(
    ctx: &DecodeContext<'_>,
    stream: &str,
    table: Vec<TableEntry>,
    groups: Vec<ChannelGroup>,
) -> Result<Vec<ActEntity>, CodecError> {
    let mut table_storage = ctx.reserve_scoped(0, "index F3D ACT table entries")?;
    let mut table_by_index = BTreeMap::new();
    for item in table {
        let record_index = item.record_index;
        if !ctx.insert_scoped_btree_map_if_vacant(
            &mut table_storage,
            &mut table_by_index,
            record_index,
            item,
            "index F3D ACT table entries",
            "index F3D ACT table entries",
        )? {
            return Err(CodecError::malformed(format_args!(
                "duplicate F3D ACTTable change-group reference {record_index}: {stream}"
            )));
        }
    }
    let mut by_index_storage = ctx.reserve_scoped(0, "index F3D ACT entities")?;
    let mut by_index = BTreeMap::new();
    for group in groups {
        let record_index = group.record_index;
        if ctx.contains_key_btree_map(
            &by_index,
            &record_index,
            "check duplicate F3D ACT change group",
        )? {
            return Err(CodecError::malformed(format_args!(
                "duplicate F3D ACT change group {stream}:{record_index}"
            )));
        }
        let (entity_id, row) = if let Some(item) = ctx.remove_btree_map(
            &mut table_by_index,
            &record_index,
            "take F3D ACT table entry",
        )? {
            if let Some(group_id) = group.entity_id.as_ref() {
                if !ctx.equal(
                    &item.entity_id,
                    &group_id.value,
                    "compare F3D ACT entity identifiers",
                )? {
                    return Err(CodecError::malformed(format_args!("F3D ACTTable entity key conflicts with its change group: {stream}:{record_index}")));
                }
            }
            (item.entity_id, Some(item.row))
        } else if let Some(entity_id) = group.entity_id.as_ref() {
            (
                ctx.copy_retained_text(&entity_id.value, "retain F3D ACT group entity id")?,
                None,
            )
        } else {
            continue;
        };
        let channel_group = ActChannelGroup::try_new(
            u64_from_index(group.record_index_offset),
            group.entity_id.as_ref().map(|id| u64_from_index(id.offset)),
            group.class_tag,
            group.channels,
            group.class_tail,
        )
        .map_err(CodecError::malformed)?;
        let entity = ActEntity::try_new(
            crate::ids::native_scoped_id(ctx, stream, "act-entity", record_index)?,
            record_index,
            entity_id,
            row,
            channel_group,
        )
        .map_err(CodecError::malformed)?;
        by_index_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut by_index,
                record_index,
                entity,
                "index F3D ACT entities",
            )
        })?;
    }
    if let Some(record_index) = table_by_index.keys().next() {
        return Err(CodecError::malformed(format_args!(
            "F3D ACTTable reference has no change group: {stream}:{record_index}"
        )));
    }
    ctx.collect_vec(by_index.into_values(), "collect F3D ACT entities by index")
}

fn decode_channel_group(
    ctx: &DecodeContext<'_>,
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
    for _ in ctx.admit_iter(&(0..count), "scan F3D ACT channel group entries")? {
        let Some((name, after_name)) = lp_ascii_strict_charged(ctx, bytes, cursor, 1..=128)? else {
            return Ok(None);
        };
        if after_name > frame.end
            || !ctx.is_ascii(name.as_bytes(), "validate F3D ACT channel-name ASCII")?
        {
            return Ok(None);
        }
        let Some((guid, after_guid)) =
            lp_utf16_bounded_charged(ctx, bytes, after_name, 36..=36, "retain F3D UTF-16 string")?
                .filter(|(guid, after)| *after <= frame.end && is_guid_hyphenated(guid))
        else {
            return Ok(None);
        };
        if ctx
            .insert_btree_map(
                &mut channels,
                ctx.copy_retained_text(&name, "retain F3D ACT channel name")?,
                Located {
                    value: guid.try_into().map_err(CodecError::malformed)?,
                    offset: u64_from_index(after_name + 4),
                },
                "index F3D ACT channels",
            )?
            .is_some()
        {
            return Err(CodecError::malformed(format_args!(
                "duplicate F3D ACT channel {name:?}: {stream}@{}",
                frame.start
            )));
        }
        cursor = after_guid;
    }
    let (entity_id, end) = if let Some((entity_id, end)) =
        lp_utf16_bounded_charged(ctx, bytes, cursor, 1..=1024, "retain F3D UTF-16 string")?
            .filter(|(_, end)| *end <= frame.end)
    {
        if is_entity_key(&entity_id) {
            (
                Some(Located {
                    value: entity_id,
                    offset: cursor + 4,
                }),
                end,
            )
        } else {
            (None, cursor)
        }
    } else {
        (None, cursor)
    };
    let remainder = &bytes[end..frame.end];
    let class_tail = if ctx
        .admit_iter(remainder, "check F3D ACT class-tail padding")?
        .all(|byte| *byte == 0)
    {
        None
    } else {
        Some(
            ActClassTail::new(
                ctx.copy_retained(remainder, "retain F3D ACT class tail")?,
                u64_from_index(end),
            )
            .map_err(CodecError::malformed)?,
        )
    };
    Ok(Some(ChannelGroup {
        record_index: frame.record_index,
        record_index_offset: frame.record_index_offset,
        entity_id,
        class_tag: frame
            .class_tag
            .try_clone_for_decode(ctx, "copy F3D ACT class tag")?,
        channels,
        class_tail,
    }))
}

enum ComponentLink {
    Root(ActRootComponent),
    NonRoot,
}

fn decode_component_link(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frame: &RecordFrame,
    stream: &str,
) -> Result<Option<ComponentLink>, CodecError> {
    macro_rules! some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let mut cursor = some!(frame.payload_offset.checked_add(10));
    if cursor > frame.end || some!(bytes.get(frame.payload_offset..cursor)) != [0; 10] {
        return Ok(None);
    }
    let (instance_root_record, next) = some!(marker_ref(ctx, bytes, cursor, 6, frame.end)?);
    cursor = next;
    let (entity_id, next) = some!(lp_utf16_bounded_charged(
        ctx,
        bytes,
        cursor,
        1..=1024,
        "retain F3D UTF-16 string"
    )?);
    if next > frame.end || !is_entity_key(&entity_id) {
        return Ok(None);
    }
    cursor = next;
    let (tracked_entity_record, next) = some!(marker_ref(ctx, bytes, cursor, 5, frame.end)?);
    cursor = next;
    let (registry_flag, next) = some!(marker_ref(ctx, bytes, cursor, 0, frame.end)?);
    cursor = next;
    let (display_name, next) = some!(lp_utf16_bounded_charged(
        ctx,
        bytes,
        cursor,
        0..=1024,
        "retain F3D UTF-16 string"
    )?);
    if next > frame.end {
        return Ok(None);
    }
    cursor = next;
    let mut components_marker = cursor;
    loop {
        if components_marker >= frame.end || components_marker - cursor >= 8 {
            break;
        }
        ctx.charge_work(1, "scan F3D ACT component padding")?;
        if bytes.get(components_marker) != Some(&0) {
            break;
        }
        components_marker += 1;
    }
    if components_marker == cursor {
        return Ok(None);
    }
    let (components_root_record, end) = some!(marker_value(bytes, components_marker, frame.end));
    let Some(trailing_padding) = bytes.get(end..frame.end) else {
        return Ok(None);
    };
    if !ctx
        .admit_iter(trailing_padding, "check F3D ACT component-tail padding")?
        .all(|byte| *byte == 0)
    {
        return Ok(None);
    }
    let registry_flag = some!(crate::records::act::ActRegistryFlag::from_code(
        registry_flag
    ));
    if tracked_entity_record != 3 {
        return Ok(Some(ComponentLink::NonRoot));
    }
    let layout = crate::records::act::ActRootLayout::new(
        u64_from_index(frame.start),
        entity_id,
        display_name,
        u64_from_index(components_marker - cursor),
    )
    .ok();
    let layout = some!(layout);
    Ok(Some(ComponentLink::Root(some!(ActRootComponent::try_new(
        crate::ids::native_scoped_id(ctx, stream, "act-root-component", frame.start)?,
        frame.record_index,
        frame
            .class_tag
            .try_clone_for_decode(ctx, "copy F3D ACT root class tag")?,
        instance_root_record,
        components_root_record,
        registry_flag,
        layout,
    )
    .ok()))))
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
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
    zero_count: usize,
    frame_end: usize,
) -> Result<Option<(u32, usize)>, CodecError> {
    if bytes.get(position) != Some(&1) {
        return Ok(None);
    }
    let Some(value) = View::u32_le_at(bytes, position + 1) else {
        return Ok(None);
    };
    let Some(end) = position
        .checked_add(5)
        .and_then(|next| next.checked_add(zero_count))
    else {
        return Ok(None);
    };
    if end > frame_end {
        return Ok(None);
    }
    let Some(padding) = bytes.get(position + 5..end) else {
        return Ok(None);
    };
    if ctx
        .admit_iter(padding, "check F3D ACT marker padding")?
        .all(|byte| *byte == 0)
    {
        Ok(Some((value, end)))
    } else {
        Ok(None)
    }
}

fn marker_value(bytes: &[u8], position: usize, frame_end: usize) -> Option<(u32, usize)> {
    if bytes.get(position) != Some(&1) || position.checked_add(5)? > frame_end {
        return None;
    }
    Some((View::u32_le_at(bytes, position + 1)?, position + 5))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::u64_from_index;

    use super::{decode_channel_group, merge_entities, ChannelGroup, RecordFrame, TableEntry};
    use crate::records::act::ActTableRow;
    use crate::records::identity::Located;
    use crate::test_support::{lp_ascii, lp_utf16};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeMap;

    #[test]
    fn act_vector_growth_refuses_collection_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = ctx
            .push_vec(&mut Vec::new(), 1, "collect F3D ACT test")
            .unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(_)));
    }

    #[test]
    fn act_set_growth_refuses_collection_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = ctx
            .insert_btree_set(
                &mut std::collections::BTreeSet::new(),
                1,
                "index F3D ACT test set",
            )
            .unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(_)));
    }

    #[test]
    fn act_map_growth_refuses_collection_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = ctx
            .insert_btree_map(&mut BTreeMap::new(), 1, 2, "index F3D ACT test map")
            .unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(_)));
    }

    #[test]
    fn act_meta_name_refuses_retained_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::sibling_meta_name(&ctx, "ACT/BulkStream.dat").unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(_)));
    }

    fn act_table_frame(end: usize) -> RecordFrame {
        RecordFrame {
            start: 0,
            end,
            record_index: 0,
            record_index_offset: 0,
            payload_offset: 0,
            class_tag: "001".to_owned().try_into().unwrap(),
        }
    }

    fn table_entry_frame() -> (Vec<u8>, RecordFrame) {
        let mut bytes = vec![0, 0];
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&7_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        lp_utf16(&mut bytes, "0_7");
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        let frame = act_table_frame(bytes.len());
        (bytes, frame)
    }

    fn table_reference_frame() -> (Vec<u8>, RecordFrame) {
        let mut bytes = vec![0, 0];
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        let frame = act_table_frame(bytes.len());
        (bytes, frame)
    }

    fn registry_channel_frame() -> (Vec<u8>, RecordFrame) {
        let mut bytes = vec![0, 0];
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        lp_ascii(&mut bytes, "Appearance");
        lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
        let frame = act_table_frame(bytes.len());
        (bytes, frame)
    }

    fn channel_group_frame() -> (Vec<u8>, RecordFrame, usize, usize) {
        let payload_offset = 11;
        let mut bytes = vec![0; payload_offset + 10];
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        lp_ascii(&mut bytes, "Appearance");
        lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
        let entity_at = bytes.len();
        lp_utf16(&mut bytes, "0_985");
        let tail_at = bytes.len();
        bytes.extend_from_slice(&[0; 11]);
        let frame = RecordFrame {
            start: 0,
            end: bytes.len(),
            record_index: 7,
            record_index_offset: 7,
            payload_offset,
            class_tag: "261".to_owned().try_into().unwrap(),
        };
        (bytes, frame, entity_at, tail_at)
    }

    #[test]
    fn act_table_entry_count_range_refuses_work_limit() {
        let (bytes, frame) = table_entry_frame();
        let decoded = super::decode_table(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &frame,
            0,
            "synthetic",
        )
        .unwrap();
        assert_eq!(decoded.entries.len(), 1);
        assert_eq!(decoded.entries[0].record_index, 7);
        assert_eq!(decoded.entries[0].entity_id, "0_7");
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "scan F3D ACT table entries",
            0,
            |ctx| super::decode_table(ctx, &bytes, &frame, 0, "synthetic").map(|_| ()),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == "scan F3D ACT table entries"));
    }

    #[test]
    fn act_table_reference_count_range_refuses_work_limit() {
        let (bytes, frame) = table_reference_frame();
        let decoded = super::decode_table(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &frame,
            0,
            "synthetic",
        )
        .unwrap();
        assert_eq!(decoded.references.len(), 1);
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "scan F3D ACT table references",
            0,
            |ctx| super::decode_table(ctx, &bytes, &frame, 0, "synthetic").map(|_| ()),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == "scan F3D ACT table references"));
    }

    #[test]
    fn act_registry_count_range_refuses_work_limit() {
        let (bytes, frame) = registry_channel_frame();
        let decoded = super::decode_table(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &frame,
            0,
            "synthetic",
        )
        .unwrap();
        assert_eq!(decoded.registry_channels.len(), 1);
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "scan F3D ACT registry channels",
            0,
            |ctx| super::decode_table(ctx, &bytes, &frame, 0, "synthetic").map(|_| ()),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == "scan F3D ACT registry channels"));
    }

    #[test]
    fn act_channel_group_count_range_refuses_work_limit() {
        let (bytes, frame, _, _) = channel_group_frame();
        let group = decode_channel_group(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &frame,
            "synthetic",
        )
        .unwrap()
        .expect("valid channel group");
        assert_eq!(group.channels.len(), 1);
        assert_eq!(
            group.entity_id.as_ref().map(|entity| entity.value.as_str()),
            Some("0_985")
        );
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "scan F3D ACT channel group entries",
            0,
            |ctx| decode_channel_group(ctx, &bytes, &frame, "synthetic").map(|_| ()),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == "scan F3D ACT channel group entries"));
    }

    fn table_entry(entity_id: &str) -> TableEntry {
        TableEntry {
            record_index: 7,
            row: ActTableRow::new(20).unwrap(),
            entity_id: entity_id.into(),
        }
    }

    fn channel_group(entity_id: &str) -> ChannelGroup {
        ChannelGroup {
            record_index: 7,
            record_index_offset: 100,
            entity_id: Some(Located {
                value: entity_id.into(),
                offset: 200,
            }),
            class_tag: "261".to_owned().try_into().unwrap(),
            channels: BTreeMap::from([(
                "Appearance".into(),
                Located {
                    value: String::from("11111111-2222-3333-4444-555555555555")
                        .try_into()
                        .unwrap(),
                    offset: 120,
                },
            )]),
            class_tail: None,
        }
    }

    #[test]
    fn record_index_joins_exactly_one_matching_act_change_group() {
        let stream = "Synthetic/FusionACTSegmentType1/BulkStream.dat";
        let ctx = cadmpeg_test_support::service_decode_context();
        let entities = merge_entities(
            &ctx,
            stream,
            vec![table_entry("0_985")],
            vec![channel_group("0_985")],
        )
        .expect("matching table and change group");
        assert_eq!(entities.len(), 1);
        assert!(entities[0].in_table());
        assert_eq!(entities[0].record_index(), 7);

        let mismatch = merge_entities(
            &ctx,
            stream,
            vec![table_entry("0_985")],
            vec![channel_group("0_986")],
        )
        .expect_err("table and change-group keys must agree");
        assert!(mismatch.to_string().contains("entity key conflicts"));

        let duplicate = merge_entities(
            &ctx,
            stream,
            vec![table_entry("0_985")],
            vec![channel_group("0_985"), channel_group("0_985")],
        )
        .expect_err("one record index cannot own two change groups");
        assert!(duplicate
            .to_string()
            .contains("duplicate F3D ACT change group"));

        let table_only = merge_entities(&ctx, stream, vec![table_entry("0_985")], Vec::new())
            .expect_err("every table reference must resolve to a change group");
        assert!(table_only.to_string().contains("has no change group"));

        let group_only = merge_entities(&ctx, stream, Vec::new(), vec![channel_group("0_985")])
            .expect("a change group need not have an inline ACTTable row");
        assert!(!group_only[0].in_table());
        assert!(group_only[0].table_record_index_offset().is_none());

        let mut table_keyed_group = channel_group("0_985");
        table_keyed_group.entity_id = None;
        let entities = merge_entities(
            &ctx,
            stream,
            vec![table_entry("0_985")],
            vec![table_keyed_group],
        )
        .expect("the table can supply an omitted group key");
        assert_eq!(entities[0].entity_id(), "0_985");
        assert!(entities[0].channel_entity_id_offset().is_none());
    }

    #[test]
    fn channel_group_distinguishes_zero_padding_from_a_class_tail() {
        let (mut bytes, mut frame, entity_at, tail_at) = channel_group_frame();

        let group = decode_channel_group(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &frame,
            "synthetic",
        )
        .expect("well-framed group")
        .expect("zero padding belongs to the group frame");
        assert_eq!(group.record_index, 7);
        assert_eq!(
            group.entity_id.as_ref().map(|id| id.value.as_str()),
            Some("0_985")
        );
        assert!(group.class_tail.is_none());

        let keyless_frame = RecordFrame {
            start: frame.start,
            end: entity_at,
            record_index: frame.record_index,
            record_index_offset: frame.record_index_offset,
            payload_offset: frame.payload_offset,
            class_tag: frame.class_tag.clone(),
        };
        let keyless = decode_channel_group(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &keyless_frame,
            "synthetic",
        )
        .expect("well-framed keyless group")
        .expect("table-keyed group");
        assert!(keyless.entity_id.is_none());

        let class_tail = b"\0synthetic-class-tail\x01";
        bytes.truncate(tail_at);
        bytes.extend_from_slice(class_tail);
        frame.end = bytes.len();
        let group = decode_channel_group(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &frame,
            "synthetic",
        )
        .expect("well-framed group with a class tail")
        .expect("class tail follows the complete channel grammar");
        let tail = group.class_tail.as_ref().unwrap();
        assert_eq!(tail.bytes(), class_tail);
        assert_eq!(tail.offset(), u64_from_index(tail_at));
    }
}
