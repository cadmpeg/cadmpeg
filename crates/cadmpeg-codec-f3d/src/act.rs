// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Fusion ACT entity table and change-version channel groups.
//!
//! Migrated to the platform read path (doc section 8 / 10 Phase 2): every
//! hostile read goes through a [`View`] rather than a raw `&[u8]`, the
//! `ACTTable` count-framed record loop reserves through
//! [`DecodeContext::exact_vec`] under a [`BoundedCount`] physical-floor proof,
//! the entity/guid/root-component accumulators grow through
//! [`DecodeContext::grow_vec`], each whole-stream marker scan charges the
//! work budget for the position-stepping pass, and every `lp_ascii`/`lp_utf16`
//! probe charges the bytes it examines before it reads and allocates, so a
//! hostile stream that forces a large-string probe at every scan position
//! charges CPU proportionally rather than a flat unit per position. Random-access marker
//! probes read at offsets relative to the entry window through [`seek_rel`], so
//! the record fields keep their entry-relative byte offsets while the reads
//! stay bounded by the view.

use std::collections::BTreeMap;

use crate::records::{ActEntity, ActGuid, ActRootComponent};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::decode::{DecodeContext, View};

use crate::container::{role, ContainerScan};

/// Minimum encoded bytes of one `ACTTable` record: the `0x01` marker, the
/// little-endian u32 record index, six zero bytes, and a length-prefixed UTF-16
/// entity id whose smallest form is a four-byte zero count. The physical-floor
/// proof for the count-framed table loop uses this as `min_element_size`.
const MIN_TABLE_ENTRY_BYTES: usize = 1 + 4 + 6 + 4;

/// Upper bound on the `ACTTable` record count. A format fact retained as
/// defense in depth alongside the budget: a hostile count still cannot reserve
/// past this even before the physical-floor and allocation proofs apply.
const MAX_TABLE_ENTRIES: usize = 100_000;

/// Auxiliary allocation charged per merged `ActEntity` graph node. The
/// `by_key` merge map's growth tracks the untrusted table/channel record count,
/// so it is charged against the input-proportional allocation allowance rather
/// than left to the domain cap alone (doc section 4.4 element charging basis).
const ACT_ENTITY_GRAPH_BYTES: u64 = 128;

/// The decoded ACT records recovered from one archive.
pub struct DecodedAct {
    /// Entities merged from the `ACTTable` and the channel-group stream.
    pub entities: Vec<ActEntity>,
    /// GUID literals recovered after the table, in stream order.
    pub guids: Vec<ActGuid>,
    /// Root-component descriptors recovered from the change-version stream.
    pub root_components: Vec<ActRootComponent>,
}

/// Decode the ACT segment-type bulk streams into merged entities, GUIDs, and
/// root components. Charges work per scanned byte and allocation per reserved
/// record.
pub fn decode(ctx: &DecodeContext<'_>, scan: &ContainerScan<'_>) -> Result<DecodedAct, CodecError> {
    let mut entities = ctx.grow_vec::<ActEntity>();
    let mut guids = ctx.grow_vec::<ActGuid>();
    let mut root_components = ctx.grow_vec::<ActRootComponent>();
    for entry in scan.entries.iter().filter(|entry| {
        entry.role == role::BULKSTREAM && entry.name.contains("FusionACTSegmentType")
    }) {
        let Some(view) = scan.entry_view(&entry.name) else {
            continue;
        };
        let (table, stream_guids) = decode_table(ctx, view)?;
        let groups = decode_channel_groups(ctx, view)?;
        for component in decode_root_components(ctx, view, &entry.name)? {
            root_components.try_push(component)?;
        }
        let mut by_key: BTreeMap<(u32, String), ActEntity> = BTreeMap::new();
        for item in table {
            ctx.charge_alloc(ACT_ENTITY_GRAPH_BYTES, "act::merge_table", None)?;
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
            if !by_key.contains_key(&key) {
                ctx.charge_alloc(ACT_ENTITY_GRAPH_BYTES, "act::merge_channel", None)?;
            }
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
        for entity in by_key.into_values() {
            entities.try_push(entity)?;
        }
        for (ordinal, (guid, offset)) in stream_guids.into_iter().enumerate() {
            guids.try_push(ActGuid {
                id: format!("f3d:{}:act-guid#{offset}", entry.name),
                byte_offset: offset as u64,
                guid_offset: (offset + 4) as u64,
                ordinal: ordinal as u32,
                guid,
            })?;
        }
    }
    Ok(DecodedAct {
        entities: entities.finish(),
        guids: guids.finish(),
        root_components: root_components.finish(),
    })
}

/// Length of the readable window, in bytes. Offsets in this module are relative
/// to this window, matching the entry-relative byte offsets the records report.
fn win_len(base: View<'_>) -> usize {
    base.end().saturating_sub(base.start())
}

/// A copy of `base` positioned at the entry-relative offset `rel`, or `None`
/// when `rel` falls past the window.
fn seek_rel(base: View<'_>, rel: usize) -> Option<View<'_>> {
    let mut view = base;
    let abs = base.start().checked_add(rel)?;
    view.seek(abs)?;
    Some(view)
}

/// Little-endian u32 at entry-relative offset `rel`, without advancing `base`.
fn u32_le_rel(base: View<'_>, rel: usize) -> Option<u32> {
    seek_rel(base, rel)?.u32_le()
}

/// The single byte at entry-relative offset `rel`, without advancing `base`.
fn byte_rel(base: View<'_>, rel: usize) -> Option<u8> {
    seek_rel(base, rel)?.u8()
}

/// Whether the `n` bytes at entry-relative offset `rel` are all present and
/// zero. A short window reads as `false`, matching the original slice-equality
/// framing check.
fn zeros_rel(base: View<'_>, rel: usize, n: usize) -> bool {
    match seek_rel(base, rel).and_then(|mut view| view.take(n)) {
        Some(slice) => slice.iter().all(|byte| *byte == 0),
        None => false,
    }
}

/// Entry-relative offset of the first `needle` occurrence, scanning through the
/// view. Charges the caller for the scan separately.
fn find_marker(base: View<'_>, needle: &[u8]) -> Option<usize> {
    let len = win_len(base);
    let n = needle.len();
    if n == 0 || len < n {
        return None;
    }
    (0..=len - n).find(|&rel| seek_rel(base, rel).and_then(|mut view| view.take(n)) == Some(needle))
}

/// A GUID literal recovered after the `ACTTable`, with its entry-relative
/// offset.
type StreamGuid = (String, usize);

/// The `ACTTable` record list and the trailing GUID literals.
type DecodedTable = (Vec<TableEntry>, Vec<StreamGuid>);

/// One `ACTTable` record: an entity id keyed by record index, with both fields'
/// entry-relative byte offsets.
struct TableEntry {
    record_index: u32,
    record_index_offset: usize,
    entity_id: String,
    entity_id_offset: usize,
}

/// Decode the `ACTTable` count-framed record list and the GUID literals that
/// follow it. Reserves the record list through [`DecodeContext::exact_vec`]
/// under a physical-floor proof and charges work for the table search and the
/// trailing GUID scan.
fn decode_table(ctx: &DecodeContext<'_>, base: View<'_>) -> Result<DecodedTable, CodecError> {
    let empty = || (Vec::new(), Vec::new());
    // The table search and the trailing GUID scan each examine the window once.
    ctx.charge_work(
        (win_len(base) as u64).saturating_mul(2),
        "act::decode_table",
        Some(base.location()),
    )?;
    let Some(name_at) = find_marker(base, b"ACTTable") else {
        return Ok(empty());
    };
    let mut cursor = name_at + 8;
    if !zeros_rel(base, cursor, 2) {
        return Ok(empty());
    }
    cursor += 2;
    let Some(count) = u32_le_rel(base, cursor).map(|value| value as usize) else {
        return Ok(empty());
    };
    if count > MAX_TABLE_ENTRIES {
        return Ok(empty());
    }
    cursor += 4;
    // Proof 1 — the records could physically fit in the unread window.
    let Some(bounded) =
        seek_rel(base, cursor).and_then(|view| view.counted(count as u64, MIN_TABLE_ENTRY_BYTES))
    else {
        return Ok(empty());
    };
    // Proof 2 — the decode may commit the memory (charges alloc_bytes first).
    let mut indexed = ctx.exact_vec::<TableEntry>(bounded)?;
    for _ in 0..count {
        if byte_rel(base, cursor) != Some(1) {
            return Ok(empty());
        }
        let Some(record_index) = u32_le_rel(base, cursor + 1) else {
            return Ok(empty());
        };
        if !zeros_rel(base, cursor + 5, 6) {
            return Ok(empty());
        }
        let entity_id_offset = cursor + 15;
        let Some((entity_id, end)) = lp_utf16(ctx, base, cursor + 11)? else {
            return Ok(empty());
        };
        indexed.push(TableEntry {
            record_index,
            record_index_offset: cursor + 1,
            entity_id,
            entity_id_offset,
        })?;
        cursor = end;
    }
    let mut guids = ctx.grow_vec::<StreamGuid>();
    let len = win_len(base);
    while cursor + 4 <= len {
        if let Some((guid, end)) = lp_utf16(ctx, base, cursor)?.filter(|(value, _)| is_guid(value))
        {
            guids.try_push((guid, cursor))?;
            cursor = end;
        } else {
            cursor += 1;
        }
    }
    Ok((indexed.finish(), guids.finish()))
}

/// One change-version channel group keyed by (record index, entity id), with
/// the channel name/GUID map and the GUIDs' entry-relative offsets.
struct ChannelGroup {
    record_index: u32,
    record_index_offset: usize,
    entity_id: String,
    entity_id_offset: usize,
    class_tag: String,
    channels: BTreeMap<String, String>,
    guid_offsets: BTreeMap<String, u64>,
}

/// Scan the whole stream for change-version channel groups. Charges work for
/// the single-pass byte scan; the per-group channel map is bounded to eight
/// entries by the format, so it is not additionally alloc-charged.
fn decode_channel_groups(
    ctx: &DecodeContext<'_>,
    base: View<'_>,
) -> Result<Vec<ChannelGroup>, CodecError> {
    ctx.charge_work(
        win_len(base) as u64,
        "act::decode_channel_groups",
        Some(base.location()),
    )?;
    let mut out = ctx.grow_vec::<ChannelGroup>();
    let len = win_len(base);
    let mut position = 0usize;
    while position + 4 <= len {
        let Some((class_tag, after_tag)) = lp_ascii(ctx, base, position)? else {
            position += 1;
            continue;
        };
        if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
            position += 1;
            continue;
        }
        let Some(record_index) = u32_le_rel(base, after_tag) else {
            break;
        };
        if !zeros_rel(base, after_tag + 4, 10) {
            position += 1;
            continue;
        }
        let Some(count) = u32_le_rel(base, after_tag + 14).map(|value| value as usize) else {
            break;
        };
        if !(1..=8).contains(&count) {
            position += 1;
            continue;
        }
        let mut cursor = after_tag + 18;
        let mut channels = BTreeMap::new();
        let mut guid_offsets = BTreeMap::new();
        for _ in 0..count {
            let Some((name, after_name)) = lp_ascii(ctx, base, cursor)? else {
                channels.clear();
                break;
            };
            let Some((guid, after_guid)) =
                lp_utf16(ctx, base, after_name)?.filter(|(v, _)| is_guid(v))
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
            if let Some((entity_id, end)) = lp_utf16(ctx, base, cursor)? {
                out.try_push(ChannelGroup {
                    record_index,
                    record_index_offset: after_tag,
                    entity_id,
                    entity_id_offset: cursor + 4,
                    class_tag,
                    channels,
                    guid_offsets,
                })?;
                position = end;
                continue;
            }
        }
        position += 1;
    }
    Ok(out.finish())
}

/// Scan the whole stream for change-version root-component descriptors. Charges
/// work for the single-pass byte scan and grows the result through the budgeted
/// accumulator.
fn decode_root_components(
    ctx: &DecodeContext<'_>,
    base: View<'_>,
    stream: &str,
) -> Result<Vec<ActRootComponent>, CodecError> {
    ctx.charge_work(
        win_len(base) as u64,
        "act::decode_root_components",
        Some(base.location()),
    )?;
    let mut out = ctx.grow_vec::<ActRootComponent>();
    let len = win_len(base);
    let mut position = 0usize;
    while position + 4 <= len {
        let Some((class_tag, after_tag)) = lp_ascii(ctx, base, position)? else {
            position += 1;
            continue;
        };
        if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
            position += 1;
            continue;
        }
        let Some(record_index) = u32_le_rel(base, after_tag) else {
            break;
        };
        if !zeros_rel(base, after_tag + 4, 10) {
            position += 1;
            continue;
        }
        let cursor = after_tag + 14;
        let instance_root_record_offset = cursor + 1;
        let Some((instance_root_record, cursor)) = marker_ref(base, cursor, 6) else {
            position += 1;
            continue;
        };
        let entity_id_offset = cursor + 4;
        let Some((entity_id, cursor)) = lp_utf16(ctx, base, cursor)? else {
            position += 1;
            continue;
        };
        let Some((flag, cursor)) = marker_ref(base, cursor, 5) else {
            position += 1;
            continue;
        };
        if flag != 3 {
            position += 1;
            continue;
        }
        let registry_flag_offset = cursor + 1;
        let Some((selector, cursor)) = marker_ref(base, cursor, 0) else {
            position += 1;
            continue;
        };
        if selector > 1 {
            position += 1;
            continue;
        }
        let display_name_offset = cursor + 4;
        let Some((display_name, cursor)) = lp_utf16(ctx, base, cursor)? else {
            position += 1;
            continue;
        };
        let mut components_marker = cursor;
        while byte_rel(base, components_marker) == Some(0) && components_marker - cursor < 8 {
            components_marker += 1;
        }
        if components_marker == cursor {
            position += 1;
            continue;
        }
        let Some((components_root_record, end)) = marker_value(base, components_marker) else {
            position += 1;
            continue;
        };
        out.try_push(ActRootComponent {
            id: format!("f3d:{stream}:act-root-component#{position}"),
            byte_offset: position as u64,
            record_index,
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
        })?;
        position = end;
    }
    Ok(out.finish())
}

/// A `0x01`-tagged little-endian u32 reference followed by `zero_count` zero
/// bytes at entry-relative offset `position`. Returns the value and the offset
/// just past the trailing zeros.
fn marker_ref(base: View<'_>, position: usize, zero_count: usize) -> Option<(u32, usize)> {
    if byte_rel(base, position) != Some(1) {
        return None;
    }
    let value = u32_le_rel(base, position + 1)?;
    let end = position + 5 + zero_count;
    zeros_rel(base, position + 5, zero_count).then_some((value, end))
}

/// A `0x01`-tagged little-endian u32 value at entry-relative offset `position`.
/// Returns the value and the offset just past it.
fn marker_value(base: View<'_>, position: usize) -> Option<(u32, usize)> {
    if byte_rel(base, position) != Some(1) {
        return None;
    }
    Some((u32_le_rel(base, position + 1)?, position + 5))
}

/// A length-prefixed ASCII string (little-endian u32 length, then bytes) at
/// entry-relative offset `position`. Returns the string and the offset just
/// past it. Lengths outside `1..=128` are rejected.
fn lp_ascii(
    ctx: &DecodeContext<'_>,
    base: View<'_>,
    position: usize,
) -> Result<Option<(String, usize)>, CodecError> {
    let Some(length) = u32_le_rel(base, position).and_then(|value| usize::try_from(value).ok())
    else {
        return Ok(None);
    };
    if !(1..=128).contains(&length) {
        return Ok(None);
    }
    // Charge the bytes this probe examines (length header plus payload) before
    // it reads and allocates. A whole-stream scan runs this probe at many
    // positions; charging only the scan step (once per position) undercharges
    // the per-position read by a large constant, so a hostile stream would
    // evade the work freeze. Charging bytes-examined keeps work a faithful CPU
    // proxy.
    ctx.charge_work((4 + length) as u64, "act::lp_ascii", Some(base.location()))?;
    let Some(mut view) = seek_rel(base, position) else {
        return Ok(None);
    };
    if view.u32_le().is_none() {
        return Ok(None);
    }
    let Some(raw) = view.take(length) else {
        return Ok(None);
    };
    let Ok(value) = std::str::from_utf8(raw) else {
        return Ok(None);
    };
    Ok(Some((value.into(), position + 4 + length)))
}

/// A length-prefixed UTF-16LE string (little-endian u32 unit count, then units)
/// at entry-relative offset `position`. Returns the string and the offset just
/// past it. Counts above 1024 units are rejected; the transient decode buffer
/// is bounded by that domain cap.
fn lp_utf16(
    ctx: &DecodeContext<'_>,
    base: View<'_>,
    position: usize,
) -> Result<Option<(String, usize)>, CodecError> {
    let Some(count) = u32_le_rel(base, position).and_then(|value| usize::try_from(value).ok())
    else {
        return Ok(None);
    };
    if count > 1024 {
        return Ok(None);
    }
    let Some(byte_length) = count.checked_mul(2) else {
        return Ok(None);
    };
    // Charge the bytes this probe examines (length header plus the UTF-16
    // payload) before it reads, decodes, and allocates. See `lp_ascii` for why
    // the per-position scan charge alone undercharges this probe.
    ctx.charge_work(
        (4 + byte_length) as u64,
        "act::lp_utf16",
        Some(base.location()),
    )?;
    let Some(mut view) = seek_rel(base, position) else {
        return Ok(None);
    };
    if view.u32_le().is_none() {
        return Ok(None);
    }
    let Some(raw) = view.take(byte_length) else {
        return Ok(None);
    };
    let units = raw
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    let Ok(value) = String::from_utf16(&units) else {
        return Ok(None);
    };
    Ok(Some((value, position + 4 + byte_length)))
}

/// Whether `value` is a canonical 36-character hyphenated GUID.
fn is_guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}
