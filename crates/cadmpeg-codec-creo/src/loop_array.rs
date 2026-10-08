// SPDX-License-Identifier: Apache-2.0
//! Native `lo_array` frame and row retention.
//!
//! The loop-array body carries a native loop roster, but its joins to faces,
//! contours, and curve topology are not established. This module therefore
//! stops at exact frame and row boundaries and does not construct neutral
//! loops.
#![deny(clippy::disallowed_methods)]

use cadmpeg_core::decode::{bounded_len, DecodeContext};
use cadmpeg_core::CodecError;

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

/// Layout marker between the loop-array label and its array opener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutMarker {
    /// The `f2` marker.
    F2,
    /// The `f3` marker.
    F3,
}

impl serde::Serialize for LayoutMarker {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(match self {
            Self::F2 => 0xf2,
            Self::F3 => 0xf3,
        })
    }
}

/// One validated `lo_array` frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopArrayFrame {
    /// Byte offset of the `lo_array` label.
    pub(crate) offset: usize,
    /// Optional layout marker immediately after the label: `f2` or `f3`.
    /// Older frames omit this marker and begin directly with `f8`.
    pub(crate) variant: Option<LayoutMarker>,
    /// Stored loop-array slot extent.
    pub(crate) declared_count: u32,
    /// Native class reference from the frame header and prototype close.
    pub(crate) class_id: u32,
    /// Byte offset immediately after the named prototype close.
    pub(crate) prototype_end: usize,
    /// Byte offset of the next array label or the section end.
    pub(crate) end: usize,
    /// Additional validated rows exceed the declared extent.
    pub(crate) overfull: bool,
}

/// One complete positional `lo_array` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopArrayRecord {
    /// Owning frame label offset.
    pub(crate) frame_offset: usize,
    /// `lo_id` compact integer.
    pub(crate) lo_id: u32,
    /// `lo_type` compact integer.
    pub(crate) lo_type: u32,
    /// `lo_subtype` compact integer.
    pub(crate) lo_subtype: u32,
    /// `feat_id` compact integer.
    pub(crate) feature_id: u32,
    /// Raw `attributes` byte.
    pub(crate) attributes: u8,
    /// `direction` compact integer.
    pub(crate) direction: u32,
    /// `next_lo_ptr` compact integer.
    pub(crate) next_lo_ptr: u32,
    /// Exact row body from the first body byte through its `e3` close.
    pub(crate) body: Vec<u8>,
    /// Byte offset of the fixed row prefix.
    pub(crate) offset: usize,
    /// Byte offset of the first body byte.
    pub(crate) body_offset: usize,
}

/// Results of scanning all `lo_array` frames in one section payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LoopArrayScan {
    /// Validated frame headers and named prototypes.
    pub(crate) frames: Vec<LoopArrayFrame>,
    /// Complete positional rows from non-overfull frames.
    pub(crate) records: Vec<LoopArrayRecord>,
}

fn find_named_field(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    name: &[u8],
) -> Result<Option<usize>, CodecError> {
    let Some(marker_len) = name.len().checked_add(3) else {
        return Ok(None);
    };
    let Some(bytes) = data.get(start..end) else {
        return Ok(None);
    };
    Ok(ctx
        .position_by(
            bytes.windows(marker_len),
            |marker| {
                Ok(marker[0] == 0xe0
                    && &marker[2..2 + name.len()] == name
                    && marker[2 + name.len()] == 0)
            },
            "creo loop prototype scan",
        )?
        .map(|offset| start + offset))
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

fn prototype_close(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    class_id: u32,
) -> Result<Option<usize>, CodecError> {
    let Some(close_end) = end.checked_sub(4) else {
        return Ok(None);
    };
    ctx.find_map(
        start..=close_end,
        |offset| {
            if data.get(offset..offset + 2) != Some(&[0xf1, 0xf7]) {
                return Ok(None);
            }
            let Ok((reference, after_reference)) = psb::reference_id(data, offset + 2) else {
                return Ok(None);
            };
            Ok(
                (reference == class_id && after_reference < end && data[after_reference] == 0xe3)
                    .then_some(after_reference + 1),
            )
        },
        "creo loop prototype scan",
    )
}

fn named_prototype_end(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    class_id: u32,
) -> Result<Option<usize>, CodecError> {
    let Some(close_end) = prototype_close(ctx, data, start, end, class_id)? else {
        return Ok(None);
    };
    let mut cursor = start;
    for field in PROTOTYPE_FIELDS {
        let Some(field_start) = find_named_field(ctx, data, cursor, close_end, field)? else {
            return Ok(None);
        };
        let Some(next) = field
            .len()
            .checked_add(3)
            .and_then(|length| field_start.checked_add(length))
        else {
            return Ok(None);
        };
        cursor = next;
    }
    Ok(Some(close_end))
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

struct PendingLoopRow {
    prefix: Prefix,
    offset: usize,
    close: usize,
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
        // `token_at` answers only for an offset inside `data`, so every token
        // it states spans at least its own head byte. A zero-length token would
        // not advance the walk, and the mint refuses it instead of flooring it.
        cursor = cursor.checked_add(std::num::NonZeroUsize::new(token.length)?.get())?;
    }
    None
}

fn parse_frame(
    ctx: &DecodeContext<'_>,
    record_scope: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    data: &[u8],
    offset: usize,
    section_end: usize,
) -> Result<Option<(LoopArrayFrame, Vec<PendingLoopRow>)>, CodecError> {
    let Some(mut cursor) = offset.checked_add(LO_ARRAY_LABEL.len()) else {
        return Ok(None);
    };
    let Some(&marker) = data.get(cursor) else {
        return Ok(None);
    };
    let variant = match marker {
        0xf2 => Some(LayoutMarker::F2),
        0xf3 => Some(LayoutMarker::F3),
        0xf8 => None,
        _ => return Ok(None),
    };
    cursor += usize::from(variant.is_some());
    if data.get(cursor) != Some(&0xf8) {
        return Ok(None);
    }
    let Some((declared_count, after_count)) = compact_at(data, cursor + 1, section_end) else {
        return Ok(None);
    };
    cursor = after_count;
    if data.get(cursor) != Some(&0xf7) {
        return Ok(None);
    }
    let Ok((class_id, after_class)) = psb::reference_id(data, cursor + 1) else {
        return Ok(None);
    };
    if class_id == 0 || data.get(after_class..after_class + 2) != Some(&[0xfb, 0xe3]) {
        return Ok(None);
    }
    let header_end = after_class + 2;
    let mut end = section_end;
    for label in ARRAY_BOUNDARY_LABELS {
        if let Some(offset) = ctx
            .find_map(
                data.get(header_end..)
                    .unwrap_or_default()
                    .windows(label.len())
                    .enumerate(),
                |(offset, bytes)| Ok((bytes == label).then_some(header_end + offset)),
                "creo loop frame boundaries",
            )?
            .filter(|offset| *offset < section_end)
        {
            end = end.min(offset);
        }
    }
    let Some(prototype_end) = named_prototype_end(ctx, data, header_end, end, class_id)? else {
        return Ok(None);
    };

    let Some(remaining) = end.checked_sub(prototype_end) else {
        return Ok(None);
    };
    let Some(max_records) = bounded_len(u64::from(declared_count), 1, remaining) else {
        return Ok(Some((
            LoopArrayFrame {
                offset,
                variant,
                declared_count,
                class_id,
                prototype_end,
                end,
                overfull: false,
            },
            Vec::new(),
        )));
    };
    let mut cursor = prototype_end;
    let mut records = Vec::new();
    let mut rows = 0..max_records;
    while cursor < end
        && ctx
            .next_charged(&mut rows, "creo loop row traversal")?
            .is_some()
    {
        let Some(prefix) = row_prefix(data, cursor, end) else {
            break;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(end - prefix.body_offset),
            "creo loop row token walk",
        )?;
        let Some(close) = row_end(data, prefix.body_offset, end) else {
            break;
        };
        record_scope
            .with_storage(|| ctx.reserve_vec(&mut records, 1, "creo loop array frame records"))?;
        records.push(PendingLoopRow {
            prefix,
            offset: cursor,
            close,
        });
        cursor = close + 1;
    }
    let overfull = records.len() == max_records && row_prefix(data, cursor, end).is_some();
    if overfull {
        records.clear();
    }
    Ok(Some((
        LoopArrayFrame {
            offset,
            variant,
            declared_count,
            class_id,
            prototype_end,
            end,
            overfull,
        },
        records,
    )))
}

/// Retain structurally complete `lo_array` frames and positional rows.
pub(crate) fn scan(ctx: &DecodeContext<'_>, data: &[u8]) -> Result<LoopArrayScan, CodecError> {
    let mut result = LoopArrayScan::default();
    let mut search = 0;
    while let Some(offset) = ctx.find_map(
        data.get(search..)
            .unwrap_or_default()
            .windows(LO_ARRAY_LABEL.len())
            .enumerate(),
        |(offset, bytes)| Ok((bytes == LO_ARRAY_LABEL).then_some(search + offset)),
        "creo loop array discovery",
    )? {
        let Some(next_search) = offset.checked_add(LO_ARRAY_LABEL.len()) else {
            break;
        };
        search = next_search;
        let mut record_scope = ctx.reserve_scoped(0, "creo loop frame record scratch")?;
        let Some((frame, records)) = parse_frame(ctx, &mut record_scope, data, offset, data.len())?
        else {
            continue;
        };
        ctx.reserve_vec(&mut result.frames, 1, "creo loop array frames")?;
        let frame_offset = frame.offset;
        result.frames.push(frame);
        ctx.reserve_vec(
            &mut result.records,
            records.len(),
            "creo loop array section records",
        )?;
        for PendingLoopRow {
            prefix,
            offset,
            close,
        } in ctx.admit_iter(records, "creo loop retained row projection")?
        {
            let body = ctx.copy_retained(
                &data[prefix.body_offset..=close],
                "creo loop array record body",
            )?;
            result.records.push(LoopArrayRecord {
                frame_offset,
                lo_id: prefix.lo_id,
                lo_type: prefix.lo_type,
                lo_subtype: prefix.lo_subtype,
                feature_id: prefix.feature_id,
                attributes: prefix.attributes,
                direction: prefix.direction,
                next_lo_ptr: prefix.next_lo_ptr,
                body,
                offset,
                body_offset: prefix.body_offset,
            });
        }
        search = search.max(result.frames.last().map_or(search, |frame| frame.end));
    }
    ctx.stable_sort_by(
        result.frames.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo scan result frames ordering",
    )?;
    ctx.stable_sort_by(
        result.records.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo scan result records ordering",
    )?;
    Ok(result)
}

#[cfg(test)]
mod tests;
