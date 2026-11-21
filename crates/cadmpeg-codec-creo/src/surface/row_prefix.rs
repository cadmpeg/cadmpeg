// SPDX-License-Identifier: Apache-2.0
//! Compact-integer fields of `srf_array` row headers.

use cadmpeg_core::bytes::find_from as find;

use super::{BoundaryType, SurfaceKind, SurfaceRow};
use crate::psb;

/// The smallest value the packed `c0..df XX YY` tier stores canonically.
const PACKED_ROW_FIELD_MIN: u32 = 0x4000;

/// Labels whose start closes the `srf_array` frame before them.
const FRAME_END_LABELS: [&[u8]; 4] = [
    b"srf_array\0",
    b"crv_array\0",
    b"lo_array\0",
    b"qlt_array\0",
];

/// Row-close byte: a packed `geom_id` can start after it, and a packed
/// field's value bytes do not hold it.
const ROW_CLOSE: u8 = 0xe3;

/// Named-record header byte.
const NAMED_RECORD: u8 = 0xe0;

/// Largest named-record field-type byte.
const NAMED_RECORD_FIELD_TYPE_MAX: u8 = 0x24;

/// Longest positional row header: a packed id, the type byte, a packed
/// `feat_id`, the two discriminators, and a packed `next_geom_ptr`.
const POSITIONAL_HEADER_MAX: usize = 12;

/// Length of a packed field.
const PACKED_FIELD_LEN: usize = 3;

/// The compact-integer tiers a row scan reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowReading {
    /// The one-byte and two-byte tiers, and a lone control byte as its own value.
    Compact,
    /// The packed `c0..df XX YY` tier where it applies, then the compact
    /// reading. A packed `geom_id` applies only at a frame start or after an
    /// `e3`, where no one- or two-byte identifier ending at the same type byte
    /// starts.
    Packed,
}

/// A canonical packed `c0..df XX YY` field that ends at or before `limit`
/// and before the next frame label, and whose two value bytes hold no `e3`
/// and start no named-record header.
fn packed_field(payload: &[u8], offset: usize, limit: usize) -> Option<(u32, usize)> {
    matches!(payload.get(offset), Some(0xc0..=0xdf)).then_some(())?;
    let limit = limit.min(label_limit(payload, offset, PACKED_FIELD_LEN));
    let (value, end) = psb::packed_compact_int(payload.get(..limit)?, offset)?;
    (value >= PACKED_ROW_FIELD_MIN).then_some(())?;
    (offset + 1..end)
        .all(|at| payload.get(at) != Some(&ROW_CLOSE) && !named_record_header_at(payload, at))
        .then_some((value, end))
}

/// The readings of a row field in priority order.
fn field_readings(
    payload: &[u8],
    offset: usize,
    limit: usize,
    reading: RowReading,
) -> impl Iterator<Item = (u32, usize)> {
    let packed = match reading {
        RowReading::Packed => packed_field(payload, offset, limit),
        RowReading::Compact => None,
    };
    let (value, end) = psb::compact_int(payload, offset);
    let compact = (end > offset).then_some((value, end));
    packed.into_iter().chain(compact)
}

/// The first reading of a row field.
fn row_field(
    payload: &[u8],
    offset: usize,
    limit: usize,
    reading: RowReading,
) -> Option<(u32, usize)> {
    field_readings(payload, offset, limit, reading).next()
}

/// Whether a named-record header starts at `offset`.
fn named_record_header_at(payload: &[u8], offset: usize) -> bool {
    payload.get(offset) == Some(&NAMED_RECORD)
        && payload
            .get(offset + 1)
            .is_some_and(|field_type| *field_type <= NAMED_RECORD_FIELD_TYPE_MAX)
        && payload.get(offset + 2).is_some_and(u8::is_ascii_alphabetic)
}

/// The value of a named row field at `offset`. A packed value ends before the
/// namespace end.
pub(super) fn named_row_field(
    payload: &[u8],
    offset: usize,
    end: usize,
    reading: RowReading,
) -> Option<u32> {
    row_field(payload, offset, end, reading).map(|(value, _)| value)
}

/// A named-record row identifier: with `packed`, the packed tier; otherwise,
/// or when that fails, a reference identifier.
pub(crate) fn named_row_identifier(
    payload: &[u8],
    offset: usize,
    packed: bool,
) -> Option<(u32, usize)> {
    packed
        .then(|| packed_field(payload, offset, payload.len()))
        .flatten()
        .or_else(|| psb::reference_id(payload, offset).ok())
}

/// The start of the first frame-closing label that begins within `span`
/// bytes of `from`, or the payload end.
fn label_limit(payload: &[u8], from: usize, span: usize) -> usize {
    FRAME_END_LABELS
        .iter()
        .filter_map(|label| {
            let window_end = payload.len().min(from + span + label.len());
            find(payload.get(from..window_end)?, label, 0).map(|at| from + at)
        })
        .min()
        .unwrap_or(payload.len())
}

/// Offsets of every frame-closing label, ascending.
pub(super) fn frame_label_offsets(payload: &[u8]) -> Vec<usize> {
    let mut offsets = Vec::new();
    for label in FRAME_END_LABELS {
        let mut search = 0;
        while let Some(at) = find(payload, label, search) {
            offsets.push(at);
            search = at + label.len();
        }
    }
    offsets.sort_unstable();
    offsets
}

/// The row-header fields after `geom_type`.
#[derive(Debug, Clone, Copy)]
pub(super) struct RowPrefix {
    pub(super) feature_id: u32,
    pub(super) reversed: bool,
    pub(super) boundary_type: BoundaryType,
    pub(super) next_surface: u32,
    pub(super) end: usize,
}

/// Read `feat_id`, `orient`, `boundary_type`, and `next_geom_ptr` after the
/// type byte at `type_offset`. The first `feat_id` reading followed by valid
/// discriminators selects the header.
pub(super) fn row_prefix(
    payload: &[u8],
    type_offset: usize,
    limit: usize,
    reading: RowReading,
) -> Option<RowPrefix> {
    field_readings(payload, type_offset + 1, limit, reading).find_map(|(feature_id, pos)| {
        let orientation = *payload.get(pos)?;
        matches!(orientation, 0x01 | 0xf6).then_some(())?;
        let boundary_type = BoundaryType::from_byte(*payload.get(pos + 1)?)?;
        let (next_surface, end) = row_field(payload, pos + 2, limit, reading)?;
        Some(RowPrefix {
            feature_id,
            reversed: orientation == 0xf6,
            boundary_type,
            next_surface,
            end,
        })
    })
}

/// A packed `geom_id` that ends at `type_offset` and starts at or after `lower`.
pub(super) fn packed_id_ending_at(
    payload: &[u8],
    type_offset: usize,
    lower: usize,
) -> Option<(u32, usize)> {
    let start = type_offset
        .checked_sub(PACKED_FIELD_LEN)
        .filter(|start| *start >= lower)?;
    packed_field(payload, start, type_offset)
        .filter(|(_, end)| *end == type_offset)
        .map(|(value, _)| (value, start))
}

/// Whether a packed `geom_id` can start at `offset`: at a frame start or
/// after an `e3` byte.
pub(super) fn packed_id_anchored(payload: &[u8], offset: usize, frame_starts: &[usize]) -> bool {
    frame_starts.contains(&offset)
        || offset.checked_sub(1).and_then(|before| payload.get(before)) == Some(&ROW_CLOSE)
}

/// The byte after a positional row's `next_geom_ptr`.
pub(crate) fn positional_body_start(payload: &[u8], row: &SurfaceRow) -> Option<usize> {
    let limit = label_limit(payload, row.offset, POSITIONAL_HEADER_MAX);
    let (_, after_id) = row_field(payload, row.offset, limit, RowReading::Packed)?;
    (payload
        .get(after_id)
        .and_then(|byte| SurfaceKind::from_byte(*byte))
        == Some(row.kind))
    .then_some(())?;
    [RowReading::Packed, RowReading::Compact]
        .into_iter()
        .filter_map(|reading| row_prefix(payload, after_id, limit, reading))
        .find(|prefix| {
            prefix.feature_id == row.feature_id && prefix.next_surface == row.next_surface
        })
        .map(|prefix| prefix.end)
}
