// SPDX-License-Identifier: Apache-2.0
//! Trim table framing, entry parsing and section-space intersection resolution.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{index_from_u32, DecodeContext};
use cadmpeg_core::CodecError;

use super::{
    next_segment_int, points, preceding_byte, segment_int, FeatureSegment, FeatureSegmentKind,
    FeatureSegmentTable, FeatureTrimBucket, FeatureTrimEntity, FeatureTrimEntityTable,
    FeatureTrimVertex, FeatureTrimVertexTable, FeatureVariableTable, ReconciledPoints,
    TrimEntityKind, TRIM_COORDINATE_EPS,
};
use crate::psb;

pub(super) fn trim_entity_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<FeatureTrimEntityTable>, CodecError> {
    let Some(table) = ctx.find_bytes_in(
        payload,
        b"ent_tab\0",
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let header = trim_table_header(ctx, payload, b"ent_tab\0", start, end)?;
    let Some(prototype) = ctx.find_bytes_in(
        payload,
        b"entry_ptr(entity_entry)",
        table,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let preferred_cursor = match header {
        Some(header) => ctx.find_map(
            prototype..end,
            |offset| {
                if payload.get(offset..offset + 3) != Some(&[0xf4, 0x04, psb::token::ENTITY_REF]) {
                    return Ok(None);
                }
                let Ok((class, after_reference)) = psb::reference_id(payload, offset + 3) else {
                    return Ok(None);
                };
                Ok((class == header.classes.table && payload.get(after_reference) == Some(&0xe2))
                    .then_some(after_reference + 1))
            },
            "creo trim entity cursor",
        )?,
        None => None,
    };
    let cursor = match preferred_cursor {
        Some(cursor) => Some(cursor),
        None => match ctx.find_bytes_in(
            payload,
            &[0xf2, psb::token::ENTITY_REF],
            prototype,
            end,
            "find Creo feature definition field",
        )? {
            Some(close) => psb::reference_id(payload, close + 2)
                .ok()
                .map(|(_, after_reference)| after_reference),
            None => None,
        },
    };
    let Some(mut cursor) = cursor else {
        return Ok(None);
    };
    if payload.get(cursor) == Some(&0xe3) {
        cursor += 1;
    }
    let first_row = cursor;
    let region_end = ctx
        .find_bytes_in(
            payload,
            b"vert_tab",
            cursor,
            end,
            "find Creo feature definition field",
        )?
        .unwrap_or(end);
    let buckets = match header {
        Some(header) => trim_buckets(
            ctx,
            payload,
            table,
            region_end,
            header,
            TrimEntryKind::Entity,
        )?,
        None => Vec::new(),
    };
    let mut rows = Vec::new();
    let mut seen_storage = ctx.reserve_scoped(0, "creo trim entity ID storage")?;
    let mut seen = BTreeSet::new();
    while cursor < region_end {
        ctx.next_charged(&mut (cursor..region_end), "creo trim row traversal")?;
        if cursor != first_row && preceding_byte(payload, cursor) != Some(0xe3) {
            cursor += 1;
            continue;
        }
        let row_offset = cursor;
        let mut p = row_offset;
        let external_id = next_segment_int(payload, &mut p);
        let mode = next_segment_int(payload, &mut p);
        let start_vertex = next_segment_int(payload, &mut p);
        let end_vertex = next_segment_int(payload, &mut p);
        let center_vertex = next_segment_int(payload, &mut p);
        if let (Some(external_id), Some(start_vertex), Some(end_vertex)) =
            (external_id, start_vertex, end_vertex)
        {
            if external_id != 0 && payload.get(p) == Some(&0) {
                seen_storage.with_storage(|| {
                    ctx.insert_btree_set(&mut seen, external_id, "creo trim entity ID nodes")
                })?;
                ctx.reserve_vec(&mut rows, 1, "creo trim entity rows")?;
                rows.push(FeatureTrimEntity {
                    external_id,
                    mode,
                    vertices: [start_vertex, end_vertex],
                    kind: center_vertex.map_or(TrimEntityKind::Line, |center_vertex| {
                        TrimEntityKind::Arc { center_vertex }
                    }),
                    offset: row_offset,
                });
            }
        }
        cursor += 1;
    }
    let mut solved_external_ids = Vec::new();
    ctx.reserve_vec(
        &mut solved_external_ids,
        seen.len(),
        "creo trim entity solved IDs",
    )?;
    solved_external_ids.extend(ctx.admit_iter(seen, "creo trim solved ID traversal")?);
    Ok(Some(FeatureTrimEntityTable {
        declared_count: header.map(|header| header.declared_count),
        entity_ref: header.map(|header| header.classes.table),
        entry_ref: header.map(|header| header.classes.entry),
        buckets,
        solved_external_ids,
        rows,
        offset: table,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TrimTableClasses {
    pub(super) table: u32,
    pub(super) bucket: u32,
    pub(super) entry: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TrimTableHeader {
    pub(super) declared_count: u32,
    pub(super) classes: TrimTableClasses,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TrimEntryKind {
    Entity,
    Vertex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrimBucketStart {
    index: u32,
    declared_entry_count: u32,
    offset: usize,
    body_start: usize,
}

pub(super) fn trim_buckets(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    table: usize,
    end: usize,
    header: TrimTableHeader,
    kind: TrimEntryKind,
) -> Result<Vec<FeatureTrimBucket>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if header.declared_count == 0 {
        return Ok(Vec::new());
    }
    let Some(label) = ctx.find_bytes_in(
        payload,
        b"bucket_index\0",
        table,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(Vec::new());
    };
    let first_offset = label + b"bucket_index\0".len();
    let (Some(first), mut cursor) = segment_int(payload, first_offset) else {
        return Ok(Vec::new());
    };
    if first != 0 {
        return Ok(Vec::new());
    }
    let Some(bucket_label) = ctx.find_bytes_in(
        payload,
        b"bucket_xar\0",
        cursor,
        end,
        "find Creo first trim bucket",
    )?
    else {
        return Ok(Vec::new());
    };
    let label_end = bucket_label + b"bucket_xar\0".len();
    let Some(opener) = ctx.find_by(
        label_end..end,
        |&offset| Ok(payload[offset] == psb::token::ARRAY_OPEN),
        "creo first trim bucket opener",
    )?
    else {
        return Ok(Vec::new());
    };
    let Some((first_count, first_body)) =
        trim_bucket_array_count(payload, opener, header.classes.bucket)
    else {
        return Ok(Vec::new());
    };
    let mut starts_storage = ctx.reserve_scoped(0, "creo trim bucket start storage")?;
    let mut starts = Vec::new();
    starts_storage.with_storage(|| ctx.reserve_vec(&mut starts, 1, "creo trim bucket starts"))?;
    starts.push(TrimBucketStart {
        index: first,
        declared_entry_count: first_count,
        offset: first_offset,
        body_start: first_body,
    });
    while cursor < end && starts.len() < index_from_u32(header.declared_count) {
        ctx.next_charged(
            &mut (starts.len()..index_from_u32(header.declared_count)),
            "creo trim bucket traversal",
        )?;
        let Some((offset, index, next)) = ctx.find_map(
            cursor..end,
            |offset| {
                if preceding_byte(payload, offset) != Some(0xe2) {
                    return Ok(None);
                }
                let (Some(index), next) = segment_int(payload, offset) else {
                    return Ok(None);
                };
                // Compare the stored index in the type used for positions.
                Ok((index_from_u32(index) == starts.len()).then_some((offset, index, next)))
            },
            "creo trim bucket index",
        )?
        else {
            break;
        };
        let Some((declared_entry_count, body_start)) =
            positional_trim_bucket_count(payload, next, end, header.classes)
        else {
            break;
        };
        starts_storage
            .with_storage(|| ctx.reserve_vec(&mut starts, 1, "creo trim bucket starts"))?;
        starts.push(TrimBucketStart {
            index,
            declared_entry_count,
            offset,
            body_start,
        });
        cursor = next;
    }
    let mut buckets = Vec::new();
    ctx.reserve_vec(&mut buckets, starts.len(), "creo trim buckets")?;
    for (position, start) in ctx
        .admit_iter(&starts, "creo trim bucket output traversal")?
        .enumerate()
    {
        // Every bucket start after the first follows an 0xe2 separator, so
        // it has a preceding byte.
        let body_end = starts
            .get(position + 1)
            .and_then(|next| next.offset.checked_sub(1))
            .unwrap_or(end);
        buckets.push(FeatureTrimBucket {
            index: start.index,
            declared_entry_count: start.declared_entry_count,
            decoded_entry_count: trim_bucket_entry_count(
                ctx,
                payload,
                start.body_start,
                body_end,
                header.classes,
                kind,
                position == 0,
            )?,
            offset: start.offset,
        });
    }
    Ok(buckets)
}

fn positional_trim_bucket_count(
    payload: &[u8],
    mut cursor: usize,
    end: usize,
    classes: TrimTableClasses,
) -> Option<(u32, usize)> {
    match payload.get(cursor)? {
        &psb::token::ARRAY_OPEN => trim_bucket_array_count(payload, cursor, classes.bucket),
        0xf0 => {
            (payload.get(cursor + 1) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let (class, next) = psb::reference_id(payload, cursor + 2).ok()?;
            (class == classes.bucket).then_some(())?;
            cursor = next;
            (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
            trim_bucket_array_count(payload, cursor, classes.bucket)
        }
        0xf1 => {
            (payload.get(cursor + 1) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let (class, next) = psb::reference_id(payload, cursor + 2).ok()?;
            (class == classes.table && payload.get(next) == Some(&0xe2)).then_some((0, next + 1))
        }
        0xe2 | 0xe0 if cursor < end => Some((0, cursor + 1)),
        _ => None,
    }
}

fn trim_bucket_array_count(
    payload: &[u8],
    opener: usize,
    bucket_class: u32,
) -> Option<(u32, usize)> {
    let (count, after_count) = psb::compact_int(payload, opener + 1);
    (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (class, after_reference) = psb::reference_id(payload, after_count + 1).ok()?;
    (class == bucket_class
        && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe3]))
    .then_some((count, after_reference + 2))
}

fn trim_bucket_entry_count(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
    kind: TrimEntryKind,
    named_first: bool,
) -> Result<Option<u32>, CodecError> {
    match kind {
        TrimEntryKind::Entity => {
            let mut rows = 0usize;
            for offset in ctx.admit_iter(start..end, "creo trim bucket entry scan")? {
                if preceding_byte(payload, offset) == Some(0xe3)
                    && complete_trim_entity_entry(payload, offset, end)
                {
                    rows += 1;
                }
            }
            let prototype = usize::from(
                named_first
                    && named_trim_entity_prototype_complete(ctx, payload, start, end, classes)?,
            );
            // Decoded rows counted over `start..end`, not a stated count. The
            // count is stated in the width the declared count is stored in,
            // and a scan that passes that width states none.
            Ok(u32::try_from(rows + prototype).ok())
        }
        TrimEntryKind::Vertex => {
            let mut row_storage = ctx.reserve_scoped(0, "creo trim bucket vertex storage")?;
            let mut rows = BTreeSet::new();
            for offset in ctx.admit_iter(start..end, "creo trim bucket entry scan")? {
                if payload.get(offset) == Some(&psb::token::ENTITY_REF) {
                    if let Ok((class, row)) = psb::reference_id(payload, offset + 1) {
                        if class == classes.entry
                            && trim_vertex_entry_bounds(ctx, payload, row, end)?.is_some()
                        {
                            row_storage.with_storage(|| {
                                ctx.insert_btree_set(
                                    &mut rows,
                                    row,
                                    "creo trim bucket vertex nodes",
                                )
                            })?;
                        }
                    }
                }
                if preceding_byte(payload, offset) == Some(0xe3)
                    && trim_vertex_entry_bounds(ctx, payload, offset, end)?.is_some()
                {
                    row_storage.with_storage(|| {
                        ctx.insert_btree_set(&mut rows, offset, "creo trim bucket vertex nodes")
                    })?;
                }
            }
            let prototype = usize::from(
                named_first
                    && named_trim_vertex_prototype_complete(ctx, payload, start, end, classes)?,
            );
            // Decoded rows counted over `start..end`, not a stated count. The
            // count is stated in the width the declared count is stored in,
            // and a scan that passes that width states none.
            Ok(u32::try_from(rows.len() + prototype).ok())
        }
    }
}

fn complete_trim_entity_entry(payload: &[u8], offset: usize, end: usize) -> bool {
    let mut cursor = offset;
    for _ in 0..5 {
        let Some(next) = trim_entry_field(payload, cursor, end) else {
            return false;
        };
        cursor = next;
    }
    cursor < end && payload.get(cursor) == Some(&0)
}

fn trim_vertex_entry_bounds(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
    end: usize,
) -> Result<Option<(usize, u32, usize, usize)>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut cursor = offset;
    if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
        let (count, next) = psb::compact_int(payload, cursor + 1);
        cursor = next;
        let entities_start = cursor;
        let mut remaining = 0..count;
        while !remaining.is_empty() {
            if cursor >= end {
                return Ok(None);
            }
            ctx.next_charged(&mut remaining, "creo trim vertex bound entities")?;
            let (value, next) = segment_int(payload, cursor);
            if value.is_none() || next > end {
                return Ok(None);
            }
            cursor = next;
        }
        let (Some(vertex_id), next) = segment_int(payload, cursor) else {
            return Ok(None);
        };
        return Ok((next < end && payload.get(next) == Some(&0)).then_some((
            index_from_u32(count),
            vertex_id,
            next + 1,
            entities_start,
        )));
    }
    let entities_start = cursor;
    let mut value_count = 0usize;
    let mut vertex_id = None;
    while cursor < end && payload.get(cursor) != Some(&0) {
        let (Some(value), next) = segment_int(payload, cursor) else {
            return Ok(None);
        };
        if next > end {
            return Ok(None);
        }
        vertex_id = Some(value);
        cursor = next;
        value_count += 1;
        if value_count > 64 {
            return Ok(None);
        }
    }
    Ok(vertex_id.and_then(|vertex_id| {
        (value_count >= 3 && cursor < end).then_some((
            value_count - 1,
            vertex_id,
            cursor + 1,
            entities_start,
        ))
    }))
}

pub(super) fn trim_vertex_entry(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
    end: usize,
) -> Result<Option<(Vec<u32>, u32, usize)>, CodecError> {
    let mut cursor = offset;
    let mut storage = ctx.reserve_scoped(0, "creo trim vertex entities")?;
    let mut entities = Vec::new();
    let vertex_id;
    if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
        let (count, next) = psb::compact_int(payload, cursor + 1);
        cursor = next;
        let mut items = 0..count;
        while !items.is_empty() {
            if cursor >= end {
                return Ok(None);
            }
            ctx.next_charged(&mut items, "creo trim vertex entity traversal")?;
            let (Some(value), next) = segment_int(payload, cursor) else {
                return Ok(None);
            };
            if next > end {
                return Ok(None);
            }
            storage
                .with_storage(|| ctx.push_vec(&mut entities, value, "creo trim vertex entities"))?;
            cursor = next;
        }
        let (Some(value), next) = segment_int(payload, cursor) else {
            return Ok(None);
        };
        vertex_id = value;
        cursor = next;
    } else {
        while cursor < end && payload.get(cursor) != Some(&0) {
            let (Some(value), next) = segment_int(payload, cursor) else {
                return Ok(None);
            };
            if next > end {
                return Ok(None);
            }
            storage
                .with_storage(|| ctx.push_vec(&mut entities, value, "creo trim vertex entities"))?;
            cursor = next;
            if entities.len() > 64 {
                return Ok(None);
            }
        }
        if entities.len() < 3 {
            return Ok(None);
        }
        let Some(value) = entities.pop() else {
            return Ok(None);
        };
        vertex_id = value;
    }
    if cursor >= end || payload.get(cursor) != Some(&0) {
        return Ok(None);
    }
    let entities = storage.commit_value(entities)?;
    Ok(Some((entities, vertex_id, cursor + 1)))
}

fn trim_entry_field(payload: &[u8], offset: usize, end: usize) -> Option<usize> {
    let &head = payload.get(offset)?;
    let next = match head {
        0..=0x7f | 0xf6 => offset + 1,
        0x80..=0xbf if offset + 1 < end => offset + 2,
        _ => return None,
    };
    (next <= end).then_some(next)
}

fn named_trim_entity_prototype_complete(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
) -> Result<bool, CodecError> {
    let entry_label = b"entry_ptr(entity_entry)\0";
    let Some(entry) = ctx.find_bytes_in(
        payload,
        entry_label,
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(false);
    };
    let mut cursor = entry + entry_label.len();
    if payload.get(cursor) != Some(&0xe3) {
        return Ok(false);
    }
    cursor += 1;
    let labels = [
        b"xid\0".as_slice(),
        b"ent_mode\0",
        b"start_vtx\0",
        b"end_vtx\0",
        b"center_vtx\0",
        b"pers_attribs\0",
    ];
    for label in labels {
        let Some(offset) = ctx.find_bytes_in(
            payload,
            label,
            cursor,
            end,
            "find Creo feature definition field",
        )?
        else {
            return Ok(false);
        };
        let Some(next) = trim_entry_field(payload, offset + label.len(), end) else {
            return Ok(false);
        };
        cursor = next;
    }
    ctx.any_by(
        cursor..end,
        |offset| {
            Ok({
                if payload.get(offset..offset + 3) != Some(&[0xf4, 0x04, psb::token::ENTITY_REF]) {
                    return Ok(false);
                }
                psb::reference_id(payload, offset + 3).is_ok_and(|(class, next)| {
                    class == classes.table && payload.get(next) == Some(&0xe2)
                })
            })
        },
        "creo trim entity prototype terminator",
    )
}

fn named_trim_vertex_prototype_complete(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
) -> Result<bool, CodecError> {
    let Some(entity_ids) = ctx.find_bytes_in(
        payload,
        b"ent_ids\0",
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(false);
    };
    let array = entity_ids + b"ent_ids\0".len();
    if payload.get(array) != Some(&psb::token::ARRAY_OPEN) {
        return Ok(false);
    }
    let (count, mut cursor) = psb::compact_int(payload, array + 1);
    if count < 2 {
        return Ok(false);
    }
    let mut items = 0..count;
    while !items.is_empty() {
        if cursor >= end {
            return Ok(false);
        }
        ctx.next_charged(&mut items, "creo trim prototype entities")?;
        let (value, next) = segment_int(payload, cursor);
        if value.is_none() || next > end {
            return Ok(false);
        }
        cursor = next;
    }
    let Some(vertex_id) = ctx.find_bytes_in(
        payload,
        b"vertex_id\0",
        cursor,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(false);
    };
    let (vertex, next) = segment_int(payload, vertex_id + b"vertex_id\0".len());
    if vertex.is_none() || next > end {
        return Ok(false);
    }
    let Some(attributes) = ctx.find_bytes_in(
        payload,
        b"attribs\0",
        next,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(false);
    };
    let Some(next) = trim_entry_field(payload, attributes + b"attribs\0".len(), end) else {
        return Ok(false);
    };
    ctx.any_by(
        next..end,
        |offset| {
            Ok({
                if payload.get(offset..offset + 2) != Some(&[0xf3, psb::token::ENTITY_REF]) {
                    return Ok(false);
                }
                psb::reference_id(payload, offset + 2).is_ok_and(|(class, next)| {
                    class == classes.table && payload.get(next) == Some(&0xe2)
                })
            })
        },
        "creo trim vertex prototype terminator",
    )
}

pub(super) fn trim_table_header(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    label: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<TrimTableHeader>, CodecError> {
    let Some(offset) = ctx.find_bytes_in(payload, label, start, end, "find Creo trim table")?
    else {
        return Ok(None);
    };
    let table = offset + label.len();
    let Some(opener) = ctx.find_by(
        table..end,
        |&offset| Ok(payload[offset] == psb::token::ARRAY_OPEN),
        "creo trim table opener",
    )?
    else {
        return Ok(None);
    };
    let (declared_count, after_count) = psb::compact_int(payload, opener + 1);
    if payload.get(after_count) != Some(&psb::token::ENTITY_REF) {
        return Ok(None);
    }
    let Ok((table_class, _)) = psb::reference_id(payload, after_count + 1) else {
        return Ok(None);
    };
    let Some(offset) = ctx.find_bytes_in(
        payload,
        b"bucket_xar\0",
        table,
        end,
        "find Creo trim bucket class",
    )?
    else {
        return Ok(None);
    };
    let bucket_label = offset + b"bucket_xar\0".len();
    let Some(bucket_opener) = ctx.find_by(
        bucket_label..end,
        |&offset| Ok(payload[offset] == psb::token::ARRAY_OPEN),
        "creo trim bucket opener",
    )?
    else {
        return Ok(None);
    };
    let (_, after_bucket_count) = psb::compact_int(payload, bucket_opener + 1);
    if payload.get(after_bucket_count) != Some(&psb::token::ENTITY_REF) {
        return Ok(None);
    }
    let Ok((bucket_class, _)) = psb::reference_id(payload, after_bucket_count + 1) else {
        return Ok(None);
    };
    let Some(entry_class) = ctx.find_map(
        after_count..end,
        |offset| {
            Ok(trim_entry_class(
                payload,
                offset,
                table_class,
                label == b"vert_tab\0",
            ))
        },
        "creo trim entry class",
    )?
    else {
        return Ok(None);
    };
    Ok(Some(TrimTableHeader {
        declared_count,
        classes: TrimTableClasses {
            table: table_class,
            bucket: bucket_class,
            entry: entry_class,
        },
    }))
}

fn trim_entry_class(payload: &[u8], offset: usize, table_class: u32, vertex: bool) -> Option<u32> {
    if payload.get(offset) != Some(&psb::token::ENTITY_REF) {
        return None;
    }
    let (class, after_reference) = psb::reference_id(payload, offset + 1).ok()?;
    if vertex {
        let (first, next) = segment_int(payload, after_reference);
        let (second, next) = segment_int(payload, next);
        let (third, next) = segment_int(payload, next);
        (class != table_class
            && first.is_some()
            && second.is_some()
            && third.is_some()
            && payload.get(next) == Some(&0))
        .then_some(class)
    } else {
        (payload.get(after_reference..after_reference + 2) == Some(&[0, 0xe3])).then_some(class)
    }
}

fn positional_table_region(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
    next_table_class: Option<u32>,
) -> Result<Option<(usize, u32, usize, usize)>, CodecError> {
    let Some((table, declared_count, rows_start)) = ctx.find_map(
        start..end,
        |table| Ok(positional_trim_region_start(payload, table, table_class)),
        "creo positional trim table",
    )?
    else {
        return Ok(None);
    };
    let region_end = match next_table_class {
        Some(next_class) => ctx
            .find_by(
                rows_start..end,
                |&offset| {
                    if payload.get(offset) != Some(&psb::token::ARRAY_OPEN) {
                        return Ok(false);
                    }
                    let (_, after_count) = psb::compact_int(payload, offset + 1);
                    if payload.get(after_count) != Some(&psb::token::ENTITY_REF) {
                        return Ok(false);
                    }
                    Ok(psb::reference_id(payload, after_count + 1).is_ok_and(
                        |(class, after_reference)| {
                            class == next_class
                                && payload.get(after_reference..after_reference + 2)
                                    == Some(&[0xfb, 0xe2])
                        },
                    ))
                },
                "creo positional trim next table",
            )?
            .unwrap_or(end),
        None => end,
    };
    Ok(Some((table, declared_count, rows_start, region_end)))
}

fn positional_trim_region_start(
    payload: &[u8],
    table: usize,
    table_class: u32,
) -> Option<(usize, u32, usize)> {
    if payload.get(table) != Some(&psb::token::ARRAY_OPEN) {
        return None;
    }
    let (declared_count, after_count) = psb::compact_int(payload, table + 1);
    if payload.get(after_count) != Some(&psb::token::ENTITY_REF) {
        return None;
    }
    let (class, after_reference) = psb::reference_id(payload, after_count + 1).ok()?;
    (class == table_class
        && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe2]))
    .then_some((table, declared_count, after_reference + 2))
}

pub(super) fn positional_trim_entity_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
    next_table_class: Option<u32>,
) -> Result<Option<FeatureTrimEntityTable>, CodecError> {
    let TrimTableClasses {
        table: table_class,
        entry: entry_class,
        ..
    } = classes;
    let Some((table, declared_count, rows_start, region_end)) =
        positional_table_region(ctx, payload, start, end, table_class, next_table_class)?
    else {
        return Ok(None);
    };
    let mut rows = Vec::new();
    let mut seen_storage = ctx.reserve_scoped(0, "creo trim entity ID storage")?;
    let mut seen = BTreeSet::new();
    let has_entry_class = ctx.any_by(
        rows_start..region_end,
        |offset| {
            if payload.get(offset) != Some(&psb::token::ENTITY_REF) {
                return Ok(false);
            }
            Ok(
                psb::reference_id(payload, offset + 1).is_ok_and(|(class, after_reference)| {
                    class == entry_class
                        && payload.get(after_reference..after_reference + 2) == Some(&[0, 0xe3])
                }),
            )
        },
        "creo positional trim entity class",
    )?;
    let mut cursor = if declared_count == 0 || has_entry_class {
        rows_start
    } else {
        region_end
    };
    while cursor < region_end {
        ctx.next_charged(&mut (cursor..region_end), "creo trim row traversal")?;
        if cursor == rows_start || preceding_byte(payload, cursor) != Some(0xe3) {
            cursor += 1;
            continue;
        }
        let row_offset = cursor;
        let mut p = row_offset;
        let external_id = next_segment_int(payload, &mut p);
        let mode = next_segment_int(payload, &mut p);
        let start_vertex = next_segment_int(payload, &mut p);
        let end_vertex = next_segment_int(payload, &mut p);
        let center_vertex = next_segment_int(payload, &mut p);
        if let (Some(external_id), Some(start_vertex), Some(end_vertex)) =
            (external_id, start_vertex, end_vertex)
        {
            if external_id != 0 && payload.get(p) == Some(&0) {
                seen_storage.with_storage(|| {
                    ctx.insert_btree_set(&mut seen, external_id, "creo trim entity ID nodes")
                })?;
                ctx.reserve_vec(&mut rows, 1, "creo trim entity rows")?;
                rows.push(FeatureTrimEntity {
                    external_id,
                    mode,
                    vertices: [start_vertex, end_vertex],
                    kind: center_vertex.map_or(TrimEntityKind::Line, |center_vertex| {
                        TrimEntityKind::Arc { center_vertex }
                    }),
                    offset: row_offset,
                });
            }
        }
        cursor += 1;
    }
    let buckets = trim_buckets(
        ctx,
        payload,
        table,
        region_end,
        TrimTableHeader {
            declared_count,
            classes,
        },
        TrimEntryKind::Entity,
    )?;
    let mut solved_external_ids = Vec::new();
    ctx.reserve_vec(
        &mut solved_external_ids,
        seen.len(),
        "creo trim entity solved IDs",
    )?;
    solved_external_ids.extend(ctx.admit_iter(seen, "creo trim solved ID traversal")?);
    Ok(Some(FeatureTrimEntityTable {
        declared_count: Some(declared_count),
        entity_ref: Some(table_class),
        entry_ref: Some(entry_class),
        buckets,
        solved_external_ids,
        rows,
        offset: table,
    }))
}

pub(super) fn trim_vertex_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Result<Option<FeatureTrimVertexTable>, CodecError> {
    const CHAINS_WINDOW: usize = 120;
    let Some(table) = ctx.find_bytes_in(
        payload,
        b"vert_tab\0",
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let header = trim_table_header(ctx, payload, b"vert_tab\0", start, end)?;
    let mut region_end = end;
    for label in [
        b"skamp_ptr\0".as_slice(),
        b"triples_ptr\0",
        b"order_table\0",
        b"dimtab_ptr\0",
        b"relat_ptr\0",
        b"p_saved_result\0",
        b"S2D",
    ] {
        if let Some(offset) = ctx.find_bytes_in(
            payload,
            label,
            table + b"vert_tab\0".len(),
            end,
            "find Creo feature definition field",
        )? {
            region_end = region_end.min(offset);
        }
    }
    let chains_end = table
        .checked_add(b"vert_tab\0".len())
        .and_then(|after_label| after_label.checked_add(CHAINS_WINDOW))
        .map_or(end, |window_end| window_end.min(end));
    let Some(chains) = payload[table..chains_end].windows(b"chains\0".len())
        .position(|window| window == b"chains\0").map(|relative| table + relative)
    else {
        return Ok(None);
    };
    let mut cursor = chains + b"chains\0".len();
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Ok(None);
    }
    let (_, after_count) = psb::compact_int(payload, cursor + 1);
    cursor = after_count;
    if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Ok(None);
    }
    let reference_start = cursor + 1;
    let Ok((_, reference_end)) = psb::reference_id(payload, reference_start) else {
        return Ok(None);
    };
    let Some(reference) = payload.get(reference_start..reference_end) else {
        return Ok(None);
    };
    let Some(marker_len) = reference.len().checked_add(3) else {
        return Ok(None);
    };
    let is_block_marker = |offset: usize| {
        payload.get(offset..region_end).is_some_and(|tail| {
            tail.starts_with(&[0xf3, psb::token::ENTITY_REF])
                && tail.get(2..).is_some_and(|tail| {
                    tail.starts_with(reference) && tail.get(reference.len()) == Some(&0xe2)
                })
        })
    };
    let Some(first_marker) = ctx.find_by(
        reference_end..region_end,
        |&offset| Ok(is_block_marker(offset)),
        "creo trim vertex block marker",
    )?
    else {
        return Ok(None);
    };
    cursor = first_marker;

    let mut geometry = None;
    let mut rows = Vec::new();
    while cursor < region_end {
        ctx.next_charged(&mut (cursor..region_end), "creo trim row traversal")?;
        if is_block_marker(cursor) {
            cursor += marker_len;
            let (_, next) = segment_int(payload, cursor);
            cursor = next;
            continue;
        }
        match payload[cursor] {
            psb::token::ARRAY_OPEN => {
                if let Some((entities, vertex_id, next)) =
                    trim_vertex_entry(ctx, payload, cursor, region_end)?
                {
                    ctx.reserve_vec(&mut rows, 1, "creo trim vertex rows")?;
                    rows.push(FeatureTrimVertex {
                        section_coordinates: trim_vertex_intersection(
                            ctx,
                            &entities,
                            segments,
                            variables,
                            &mut geometry,
                        )?,
                        vertex_id,
                        entities,
                        offset: cursor,
                    });
                    cursor = next;
                } else {
                    let (_, next) = psb::compact_int(payload, cursor + 1);
                    cursor = next;
                }
                continue;
            }
            psb::token::ENTITY_REF => {
                let Ok((class, next)) = psb::reference_id(payload, cursor + 1) else {
                    cursor += 1;
                    continue;
                };
                if header.is_some_and(|header| class == header.classes.entry) {
                    if let Some((entities, vertex_id, after_entry)) =
                        trim_vertex_entry(ctx, payload, next, region_end)?
                    {
                        ctx.reserve_vec(&mut rows, 1, "creo trim vertex rows")?;
                        rows.push(FeatureTrimVertex {
                            section_coordinates: trim_vertex_intersection(
                                ctx,
                                &entities,
                                segments,
                                variables,
                                &mut geometry,
                            )?,
                            vertex_id,
                            entities,
                            offset: next,
                        });
                        cursor = after_entry;
                        continue;
                    }
                }
                cursor = next;
                continue;
            }
            0x00 | 0xf1 | 0xe2 | 0xe3 | 0xfb => {
                cursor += 1;
                continue;
            }
            _ => {}
        }
        let row_offset = cursor;
        let Some((entities, vertex_id, next)) =
            trim_vertex_entry(ctx, payload, cursor, region_end)?
        else {
            cursor += 1;
            continue;
        };
        ctx.reserve_vec(&mut rows, 1, "creo trim vertex rows")?;
        rows.push(FeatureTrimVertex {
            section_coordinates: trim_vertex_intersection(
                ctx, &entities, segments, variables, &mut geometry,
            )?,
            vertex_id,
            entities,
            offset: row_offset,
        });
        cursor = next;
    }
    let buckets = match header {
        Some(header) => trim_buckets(
            ctx,
            payload,
            table,
            region_end,
            header,
            TrimEntryKind::Vertex,
        )?,
        None => Vec::new(),
    };
    Ok(Some(FeatureTrimVertexTable {
        declared_count: header.map(|header| header.declared_count),
        entity_ref: header.map(|header| header.classes.table),
        entry_ref: header.map(|header| header.classes.entry),
        buckets,
        rows,
        offset: table,
    }))
}

pub(super) fn positional_trim_vertex_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Result<Option<FeatureTrimVertexTable>, CodecError> {
    let TrimTableClasses {
        table: table_class,
        entry: entry_class,
        ..
    } = classes;
    let Some((table, declared_count, rows_start, region_end)) =
        positional_table_region(ctx, payload, start, end, table_class, None)?
    else {
        return Ok(None);
    };
    let mut geometry = None;
    let mut rows = Vec::new();
    let mut cursor = rows_start;
    while cursor < region_end {
        ctx.next_charged(&mut (cursor..region_end), "creo trim row traversal")?;
        if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
            cursor += 1;
            continue;
        }
        let Ok((class, after_reference)) = psb::reference_id(payload, cursor + 1) else {
            cursor += 1;
            continue;
        };
        if class != entry_class {
            cursor += 1;
            continue;
        }
        let row_offset = after_reference;
        let Some((entities, vertex_id, next)) =
            trim_vertex_entry(ctx, payload, row_offset, region_end)?
        else {
            cursor += 1;
            continue;
        };
        ctx.reserve_vec(&mut rows, 1, "creo trim vertex rows")?;
        rows.push(FeatureTrimVertex {
            section_coordinates: trim_vertex_intersection(
                ctx, &entities, segments, variables, &mut geometry,
            )?,
            vertex_id,
            entities,
            offset: row_offset,
        });
        cursor = next.max(cursor + 1);
    }
    let buckets = trim_buckets(
        ctx,
        payload,
        table,
        region_end,
        TrimTableHeader {
            declared_count,
            classes,
        },
        TrimEntryKind::Vertex,
    )?;
    Ok(Some(FeatureTrimVertexTable {
        declared_count: Some(declared_count),
        entity_ref: Some(table_class),
        entry_ref: Some(entry_class),
        buckets,
        rows,
        offset: table,
    }))
}

const TRIM_INTERSECTION_EPS: f64 = 1.0e-12;

#[derive(Clone, Copy)]
enum TrimCarrier {
    Line {
        start: [f64; 2],
        end: [f64; 2],
    },
    Circle {
        center: [f64; 2],
        radius: cadmpeg_ir::scalar::PositiveReal,
    },
}

fn trim_vertex_intersection<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    entities: &[u32],
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
    geometry: &mut Option<points::CheckedTrimGeometry<'ctx>>,
) -> Result<Option<cadmpeg_ir::units::FinitePoint2>, CodecError> {
    Ok(
        entity_intersection_cached(ctx, entities, segments, variables, geometry)?.and_then(
            |[u, v]| cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(u, v)),
        ),
    )
}

pub(super) fn trim_endpoint_radius(
    ctx: &DecodeContext<'_>,
    segment: &FeatureSegment,
    center: [f64; 2],
    points: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Result<Result<Option<cadmpeg_ir::scalar::PositiveReal>, ()>, CodecError> {
    let mut first: Option<cadmpeg_ir::scalar::PositiveReal> = None;
    for point_id in segment.point_ids() {
        let Some([Some(u), Some(v)]) = ctx
            .get_btree_map(points, &point_id, "creo trim endpoint point")?
            .copied()
        else {
            continue;
        };
        let radius = (u - center[0]).hypot(v - center[1]);
        let Some(radius) = cadmpeg_ir::scalar::PositiveReal::new(radius) else {
            return Ok(Err(()));
        };
        if radius.get() <= TRIM_INTERSECTION_EPS {
            return Ok(Err(()));
        }
        if let Some(first) = first {
            let scale = first.get().max(radius.get()).max(1.0);
            if (radius.get() - first.get()).abs() > TRIM_COORDINATE_EPS * scale {
                return Ok(Err(()));
            }
        } else {
            first = Some(radius);
        }
    }
    Ok(Ok(first))
}

fn trim_radius(
    ctx: &DecodeContext<'_>,
    segment: &FeatureSegment,
    center: [f64; 2],
    geometry: &points::TrimGeometry,
) -> Result<Option<cadmpeg_ir::scalar::PositiveReal>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(radius_ref) = segment.radius_ref else {
        return Ok(None);
    };
    let Ok(stored) = geometry.radius(radius_ref) else {
        return Ok(None);
    };
    let Ok(endpoint) = trim_endpoint_radius(ctx, segment, center, &geometry.coordinates.points)?
    else {
        return Ok(None);
    };
    let radius = match (stored, endpoint) {
        (Some(stored), Some(endpoint)) => {
            let scale = stored.get().abs().max(endpoint.get()).max(1.0);
            if (stored.get() - endpoint.get()).abs() > TRIM_COORDINATE_EPS * scale {
                return Ok(None);
            }
            stored.get()
        }
        (Some(stored), None) => stored.get(),
        (None, Some(endpoint)) => endpoint.get(),
        (None, None) => return Ok(None),
    };
    Ok(cadmpeg_ir::scalar::PositiveReal::new(radius))
}

fn trim_point(
    ctx: &DecodeContext<'_>,
    geometry: &points::TrimGeometry,
    point_id: u32,
) -> Result<Option<[f64; 2]>, CodecError> {
    let Some([Some(u), Some(v)]) = ctx
        .get_btree_map(
            &geometry.coordinates.points,
            &point_id,
            "creo trim carrier point",
        )?
        .copied()
    else {
        return Ok(None);
    };
    Ok((u.is_finite() && v.is_finite()).then_some([u, v]))
}

fn trim_carrier(
    ctx: &DecodeContext<'_>,
    segment: &FeatureSegment,
    geometry: &points::TrimGeometry,
) -> Result<Option<TrimCarrier>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    Ok(match segment.kind {
        FeatureSegmentKind::Line(_) => {
            let Some(start) = trim_point(ctx, geometry, segment.point_ids()[0])? else {
                return Ok(None);
            };
            let Some(end) = trim_point(ctx, geometry, segment.point_ids()[1])? else {
                return Ok(None);
            };
            let scale = start
                .into_iter()
                .chain(end)
                .map(f64::abs)
                .fold(1.0, f64::max);
            ((end[0] - start[0]).hypot(end[1] - start[1]) > TRIM_INTERSECTION_EPS * scale)
                .then_some(TrimCarrier::Line { start, end })
        }
        FeatureSegmentKind::Arc(_) => {
            let Some(center_id) = segment.center_id else {
                return Ok(None);
            };
            let Some(center) = trim_point(ctx, geometry, center_id)? else {
                return Ok(None);
            };
            let Some(radius) = trim_radius(ctx, segment, center, geometry)? else {
                return Ok(None);
            };
            Some(TrimCarrier::Circle { center, radius })
        }
        FeatureSegmentKind::Point(_) => None,
    })
}

pub(super) fn trim_line_line_intersection(
    first_start: [f64; 2],
    first_end: [f64; 2],
    second_start: [f64; 2],
    second_end: [f64; 2],
) -> Option<[f64; 2]> {
    use cadmpeg_ir::math::Vector3;
    let first = Vector3::new(
        first_end[0] - first_start[0],
        first_end[1] - first_start[1],
        0.0,
    );
    let second = Vector3::new(
        second_end[0] - second_start[0],
        second_end[1] - second_start[1],
        0.0,
    );
    let first_unit = cadmpeg_ir::features::FiniteVector3::new(first)?.unit_nonzero()?;
    let second_unit = cadmpeg_ir::features::FiniteVector3::new(second)?.unit_nonzero()?;
    if first_unit.cross(second_unit).z.abs() <= TRIM_INTERSECTION_EPS {
        return None;
    }
    // Solve in component-scaled directions. Unit directions are for the
    // angular gate; their square roots need not perturb exact junctions.
    let first_scale = first.x.abs().max(first.y.abs());
    let second_scale = second.x.abs().max(second.y.abs());
    let first = [first.x / first_scale, first.y / first_scale];
    let second = [second.x / second_scale, second.y / second_scale];
    let determinant = first[0].mul_add(second[1], -first[1] * second[0]);
    let relative = [
        second_start[0] - first_start[0],
        second_start[1] - first_start[1],
    ];
    let parameter = relative[0].mul_add(second[1], -relative[1] * second[0]) / determinant;
    let point = [
        parameter.mul_add(first[0], first_start[0]),
        parameter.mul_add(first[1], first_start[1]),
    ];
    point.iter().all(|value| value.is_finite()).then_some(point)
}

pub(super) fn trim_line_circle_intersection(
    start: [f64; 2],
    end: [f64; 2],
    center: [f64; 2],
    radius: f64,
) -> Option<[f64; 2]> {
    let intersections = cadmpeg_ir::math::planar::line_circle_intersections(
        cadmpeg_ir::math::Point2::new(start[0], start[1]),
        cadmpeg_ir::math::Point2::new(end[0], end[1]),
        cadmpeg_ir::math::Point2::new(center[0], center[1]),
        radius,
    )?;
    let endpoint_tolerance = TRIM_COORDINATE_EPS * radius;
    // A segment end that is not finite has no distance to measure.
    let segment =
        cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(start[0], start[1]))
            .zip(cadmpeg_ir::units::FinitePoint2::new(
                cadmpeg_ir::math::Point2::new(end[0], end[1]),
            ));
    let mut inside = intersections.into_iter().filter(|(_, point)| {
        segment.is_some_and(|(start, end)| {
            cadmpeg_ir::math::planar::point_segment_distance(*point, start, end)
                <= endpoint_tolerance
        })
    });
    let (_, point) = inside.next()?;
    if inside.next().is_some_and(|(_, other)| other != point) {
        return None;
    }
    let radial = (point.u - center[0]).hypot(point.v - center[1]);
    ((radial - radius).abs() <= endpoint_tolerance).then_some([point.u, point.v])
}

pub(super) fn trim_circle_circle_intersection(
    first_center: [f64; 2],
    first_radius: f64,
    second_center: [f64; 2],
    second_radius: f64,
) -> Option<[f64; 2]> {
    let delta = [
        second_center[0] - first_center[0],
        second_center[1] - first_center[1],
    ];
    let distance = delta[0].hypot(delta[1]);
    let scale = distance.max(first_radius).max(second_radius);
    if !distance.is_finite() || distance <= TRIM_INTERSECTION_EPS * scale {
        return None;
    }
    let distance_scaled = distance / scale;
    let first = first_radius / scale;
    let second = second_radius / scale;
    if !first.is_finite() || !second.is_finite() || first <= 0.0 || second <= 0.0 {
        return None;
    }
    let axial_scaled = (first * first - second * second + distance_scaled * distance_scaled)
        / (2.0 * distance_scaled);
    let height_squared = first.mul_add(first, -(axial_scaled * axial_scaled));
    let tolerance = TRIM_INTERSECTION_EPS * (first * first + axial_scaled * axial_scaled);
    if !height_squared.is_finite()
        || height_squared.abs() > tolerance
        || (axial_scaled.abs() - first).abs() > TRIM_COORDINATE_EPS
        || ((distance_scaled - axial_scaled).abs() - second).abs() > TRIM_COORDINATE_EPS
    {
        return None;
    }
    let axial = axial_scaled * scale;
    let direction = [delta[0] / distance, delta[1] / distance];
    let coordinate = [
        first_center[0] + axial * direction[0],
        first_center[1] + axial * direction[1],
    ];
    coordinate
        .into_iter()
        .all(f64::is_finite)
        .then_some(coordinate)
}

#[cfg(test)]
pub(super) fn entity_intersection(
    ctx: &DecodeContext<'_>,
    entity_ids: &[u32],
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Result<Option<[f64; 2]>, CodecError> {
    let mut geometry = None;
    entity_intersection_cached(ctx, entity_ids, segments, variables, &mut geometry)
}

fn entity_intersection_cached<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    entity_ids: &[u32],
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
    geometry: &mut Option<points::CheckedTrimGeometry<'ctx>>,
) -> Result<Option<[f64; 2]>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let (Some(segments), Some(variables)) = (segments, variables) else {
        return Ok(None);
    };
    if !variables.is_complete() || entity_ids.len() < 2 {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "creo trim intersection storage")?;
    let mut unique_entities = BTreeSet::new();
    let mut segments_for_intersection = Vec::new();
    let mut ids = entity_ids.iter();
    while ids.len() != 0 {
        let Some(entity_id) = ctx.next_charged(&mut ids, "creo trim intersection entities")? else {
            break;
        };
        if !storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut unique_entities,
                *entity_id,
                "creo trim intersection entity nodes",
            )
        })? {
            return Ok(None);
        }
        let Some(segment) = segments.unique_segment(*entity_id) else {
            return Ok(None);
        };
        storage.with_storage(|| {
            ctx.push_vec(
                &mut segments_for_intersection,
                segment,
                "creo trim intersection segments",
            )
        })?;
    }
    if geometry.is_none() {
        *geometry = Some(variables.reconciled_trim_geometry(ctx)?);
    }
    let Some(geometry) = geometry.as_ref() else {
        return Ok(None);
    };
    let ReconciledPoints {
        points,
        ambiguous: ambiguous_points,
    } = &geometry.coordinates;
    if entity_ids.len() == 2 {
        let first_ids = segments_for_intersection[0].point_ids();
        let second_ids = segments_for_intersection[1].point_ids();
        let mut common = first_ids.into_iter().filter(|id| second_ids.contains(id));
        let point_id = common.next();
        if let Some(point_id) = point_id.filter(|id| common.all(|other| other == *id)) {
            if !ctx.contains_btree_set(
                ambiguous_points,
                &point_id,
                "creo trim common point ambiguity",
            )? {
                if let Some([Some(u), Some(v)]) = ctx
                    .get_btree_map(points, &point_id, "creo trim common point")?
                    .copied()
                {
                    if u.is_finite() && v.is_finite() {
                        return Ok(Some([u, v]));
                    }
                }
            }
        }
    }
    let mut carriers = Vec::new();
    let mut segment_iter = segments_for_intersection.iter();
    while segment_iter.len() != 0 {
        let Some(segment) = ctx.next_charged(&mut segment_iter, "creo trim carrier traversal")? else {
            break;
        };
        let Some(carrier) = trim_carrier(ctx, segment, geometry)? else {
            return Ok(None);
        };
        storage.with_storage(|| {
            ctx.push_vec(&mut carriers, carrier, "creo trim intersection carriers")
        })?;
    }
    let mut first_coordinate: Option<[f64; 2]> = None;
    let mut scale = 1.0_f64;
    let mut difference = 0.0_f64;
    let mut firsts = 0..carriers.len();
    while !firsts.is_empty() {
        let Some(first) = ctx.next_charged(&mut firsts, "creo trim carrier pairs")? else {
            break;
        };
        let mut seconds = first + 1..carriers.len();
        while !seconds.is_empty() {
            let Some(second) = ctx.next_charged(&mut seconds, "creo trim intersections")? else {
                break;
            };
            let Some(coordinate) = (match (carriers[first], carriers[second]) {
                (
                    TrimCarrier::Line { start, end },
                    TrimCarrier::Line {
                        start: second_start,
                        end: second_end,
                    },
                ) => trim_line_line_intersection(start, end, second_start, second_end),
                (TrimCarrier::Line { start, end }, TrimCarrier::Circle { center, radius })
                | (TrimCarrier::Circle { center, radius }, TrimCarrier::Line { start, end }) => {
                    trim_line_circle_intersection(start, end, center, radius.get())
                }
                (
                    TrimCarrier::Circle { center, radius },
                    TrimCarrier::Circle {
                        center: second_center,
                        radius: second_radius,
                    },
                ) => trim_circle_circle_intersection(
                    center,
                    radius.get(),
                    second_center,
                    second_radius.get(),
                ),
            }) else {
                return Ok(None);
            };
            scale = scale.max(coordinate[0].abs()).max(coordinate[1].abs());
            if let Some(first) = first_coordinate {
                difference =
                    difference.max((coordinate[0] - first[0]).hypot(coordinate[1] - first[1]));
            } else {
                first_coordinate = Some(coordinate);
            }
        }
    }
    Ok(first_coordinate.filter(|_| difference <= TRIM_COORDINATE_EPS * scale))
}

#[cfg(test)]
mod tests;
