// SPDX-License-Identifier: Apache-2.0
//! Native `lo_array` frame and row retention.
//!
//! The loop-array body carries a native loop roster, but its joins to faces,
//! contours, and curve topology are not established. This module therefore
//! stops at exact frame and row boundaries and does not construct neutral
//! loops.
#![deny(clippy::disallowed_methods)]

use cadmpeg_core::bytes::find_from as find;
use cadmpeg_core::decode::bounded_len;

use crate::psb;

const LO_ARRAY_LABEL: &[u8] = b"lo_array\0";
const ARRAY_BOUNDARY_LABELS: [&[u8]; 4] = [
    b"crv_array\0",
    b"lo_array\0",
    b"qlt_array\0",
    b"srf_array\0",
];
const PROTOTYPE_FIELDS: [&[u8]; 8] = [
    b"lo_id",
    b"lo_type",
    b"lo_subtype",
    b"feat_id",
    b"attributes",
    b"direction",
    b"next_lo_ptr",
    b"object_data",
];

/// One validated `lo_array` frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopArrayFrame {
    /// Byte offset of the `lo_array` label.
    pub offset: usize,
    /// Optional layout marker immediately after the label: `f2` or `f3`.
    /// Older frames omit this marker and begin directly with `f8`.
    pub variant: Option<u8>,
    /// Stored loop-array slot extent.
    pub declared_count: u32,
    /// Native class reference from the frame header and prototype close.
    pub class_id: u32,
    /// Byte offset immediately after the named prototype close.
    pub prototype_end: usize,
    /// Byte offset of the next array label or the section end.
    pub end: usize,
    /// Number of complete positional rows retained from the frame.
    pub materialized_count: usize,
    /// True when an additional validated row starts after the declared extent.
    pub overfull: bool,
}

/// One complete positional `lo_array` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopArrayRecord {
    /// Owning frame label offset.
    pub frame_offset: usize,
    /// `lo_id` compact integer.
    pub lo_id: u32,
    /// `lo_type` compact integer.
    pub lo_type: u32,
    /// `lo_subtype` compact integer.
    pub lo_subtype: u32,
    /// `feat_id` compact integer.
    pub feature_id: u32,
    /// Raw `attributes` byte.
    pub attributes: u8,
    /// `direction` compact integer.
    pub direction: u32,
    /// `next_lo_ptr` compact integer.
    pub next_lo_ptr: u32,
    /// Exact row body from the first body byte through its `e3` close.
    pub body: Vec<u8>,
    /// Byte offset of the fixed row prefix.
    pub offset: usize,
    /// Byte offset of the first body byte.
    pub body_offset: usize,
    /// Exclusive byte offset after the row close.
    pub end: usize,
}

/// Results of scanning all `lo_array` frames in one section payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoopArrayScan {
    /// Validated frame headers and named prototypes.
    pub frames: Vec<LoopArrayFrame>,
    /// Complete positional rows from non-overfull frames.
    pub records: Vec<LoopArrayRecord>,
}

fn find_named_field(data: &[u8], start: usize, end: usize, name: &[u8]) -> Option<usize> {
    let marker_len = name.len().checked_add(3)?;
    data.get(start..end)?
        .windows(marker_len)
        .position(|marker| {
            marker[0] == 0xe0 && &marker[2..2 + name.len()] == name && marker[2 + name.len()] == 0
        })
        .map(|offset| start + offset)
}

fn compact_at(data: &[u8], offset: usize, end: usize) -> Option<(u32, usize)> {
    let head = *data.get(offset)?;
    match head {
        0..=0x7f => Some((u32::from(head), offset + 1)),
        0x80..=0xbf => {
            let tail = *data.get(offset + 1)?;
            (offset + 2 <= end).then_some((
                (u32::from(head) - 0x80) * 0x100 + u32::from(tail),
                offset + 2,
            ))
        }
        _ => None,
    }
}

fn frame_end(data: &[u8], start: usize, end: usize) -> usize {
    ARRAY_BOUNDARY_LABELS
        .iter()
        .filter_map(|label| find(data, label, start).filter(|offset| *offset < end))
        .min()
        .unwrap_or(end)
}

fn prototype_close(data: &[u8], start: usize, end: usize, class_id: u32) -> Option<usize> {
    let close_end = end.checked_sub(4)?;
    for offset in start..=close_end {
        if data.get(offset..offset + 2) != Some(&[0xf1, 0xf7]) {
            continue;
        }
        let Ok((reference, after_reference)) = psb::reference_id(data, offset + 2) else {
            continue;
        };
        if reference == class_id && after_reference < end && data[after_reference] == 0xe3 {
            return Some(after_reference + 1);
        }
    }
    None
}

fn named_prototype_end(data: &[u8], start: usize, end: usize, class_id: u32) -> Option<usize> {
    let close_end = prototype_close(data, start, end, class_id)?;
    let mut cursor = start;
    for field in PROTOTYPE_FIELDS {
        cursor = find_named_field(data, cursor, close_end, field)?
            .checked_add(field.len().saturating_add(3))?;
    }
    Some(close_end)
}

#[derive(Debug, Clone, Copy)]
struct Prefix {
    lo_id: u32,
    lo_type: u32,
    lo_subtype: u32,
    feature_id: u32,
    attributes: u8,
    direction: u32,
    next_lo_ptr: u32,
    body_offset: usize,
}

fn row_prefix(data: &[u8], offset: usize, end: usize) -> Option<Prefix> {
    let (lo_id, cursor) = compact_at(data, offset, end)?;
    let (lo_type, cursor) = compact_at(data, cursor, end)?;
    let (lo_subtype, cursor) = compact_at(data, cursor, end)?;
    let (feature_id, cursor) = compact_at(data, cursor, end)?;
    let attributes = *data.get(cursor)?;
    let (direction, cursor) = compact_at(data, cursor + 1, end)?;
    let (next_lo_ptr, body_offset) = compact_at(data, cursor, end)?;
    (body_offset <= end).then_some(Prefix {
        lo_id,
        lo_type,
        lo_subtype,
        feature_id,
        attributes,
        direction,
        next_lo_ptr,
        body_offset,
    })
}

fn row_end(data: &[u8], body_start: usize, end: usize) -> Option<usize> {
    let mut cursor = body_start;
    while cursor < end {
        let token = psb::token_at(data, cursor)?;
        if matches!(token.kind, psb::TokenKind::CompoundClose) {
            return Some(cursor);
        }
        cursor = cursor.checked_add(token.length.max(1))?;
    }
    None
}

fn parse_frame(
    data: &[u8],
    offset: usize,
    section_end: usize,
) -> Option<(LoopArrayFrame, Vec<LoopArrayRecord>)> {
    let mut cursor = offset.checked_add(LO_ARRAY_LABEL.len())?;
    let variant = match *data.get(cursor)? {
        0xf2 | 0xf3 => {
            let marker = data[cursor];
            cursor += 1;
            Some(marker)
        }
        0xf8 => None,
        _ => return None,
    };
    if data.get(cursor) != Some(&0xf8) {
        return None;
    }
    let (declared_count, after_count) = compact_at(data, cursor + 1, section_end)?;
    cursor = after_count;
    if data.get(cursor) != Some(&0xf7) {
        return None;
    }
    let (class_id, after_class) = psb::reference_id(data, cursor + 1).ok()?;
    if class_id == 0 || data.get(after_class..after_class + 2) != Some(&[0xfb, 0xe3]) {
        return None;
    }
    let header_end = after_class + 2;
    let end = frame_end(data, header_end, section_end);
    let prototype_end = named_prototype_end(data, header_end, end, class_id)?;

    let max_records = bounded_len(
        u64::from(declared_count),
        1,
        end.saturating_sub(prototype_end),
    )
    .unwrap_or(0);
    let mut cursor = prototype_end;
    let mut records = Vec::new();
    let mut overfull = false;
    while cursor < end && records.len() < max_records {
        let Some(prefix) = row_prefix(data, cursor, end) else {
            break;
        };
        let Some(close) = row_end(data, prefix.body_offset, end) else {
            break;
        };
        records.push(LoopArrayRecord {
            frame_offset: offset,
            lo_id: prefix.lo_id,
            lo_type: prefix.lo_type,
            lo_subtype: prefix.lo_subtype,
            feature_id: prefix.feature_id,
            attributes: prefix.attributes,
            direction: prefix.direction,
            next_lo_ptr: prefix.next_lo_ptr,
            body: data[prefix.body_offset..=close].to_vec(),
            offset: cursor,
            body_offset: prefix.body_offset,
            end: close + 1,
        });
        cursor = close + 1;
    }
    if records.len() == max_records && row_prefix(data, cursor, end).is_some() {
        overfull = true;
        records.clear();
    }
    Some((
        LoopArrayFrame {
            offset,
            variant,
            declared_count,
            class_id,
            prototype_end,
            end,
            materialized_count: records.len(),
            overfull,
        },
        records,
    ))
}

/// Retain structurally complete `lo_array` frames and positional rows.
pub fn scan(data: &[u8]) -> LoopArrayScan {
    let mut result = LoopArrayScan::default();
    let mut search = 0;
    while let Some(offset) = find(data, LO_ARRAY_LABEL, search) {
        search = offset.saturating_add(LO_ARRAY_LABEL.len());
        let Some((frame, records)) = parse_frame(data, offset, data.len()) else {
            continue;
        };
        result.frames.push(frame);
        result.records.extend(records);
        search = search.max(result.frames.last().map_or(search, |frame| frame.end));
    }
    result.frames.sort_by_key(|frame| frame.offset);
    result.records.sort_by_key(|record| record.offset);
    result
}

#[cfg(test)]
mod tests;
