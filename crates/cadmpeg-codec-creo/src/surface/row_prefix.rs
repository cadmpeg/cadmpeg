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

/// Row-close byte after which a packed `geom_id` can start.
const ROW_CLOSE: u8 = 0xe3;

/// A canonical packed `c0..df XX YY` field that ends at or before `limit`.
fn packed_field(payload: &[u8], offset: usize, limit: usize) -> Option<(u32, usize)> {
    matches!(payload.get(offset), Some(0xc0..=0xdf)).then_some(())?;
    psb::packed_compact_int(payload.get(..limit)?, offset)
        .filter(|(value, _)| *value >= PACKED_ROW_FIELD_MIN)
}

/// The readings of a row field in priority order: the packed tier, then the
/// generic compact integer.
fn field_readings(
    payload: &[u8],
    offset: usize,
    limit: usize,
) -> impl Iterator<Item = (u32, usize)> {
    let packed = packed_field(payload, offset, limit);
    let (value, end) = psb::compact_int(payload, offset);
    let generic = (end > offset).then_some((value, end));
    packed.into_iter().chain(generic)
}

/// The first reading of a row field.
pub(super) fn row_field(payload: &[u8], offset: usize, limit: usize) -> Option<(u32, usize)> {
    field_readings(payload, offset, limit).next()
}

/// A named-record row identifier: the packed tier or a reference identifier.
pub(crate) fn named_row_identifier(payload: &[u8], offset: usize) -> Option<(u32, usize)> {
    packed_field(payload, offset, payload.len()).or_else(|| psb::reference_id(payload, offset).ok())
}

/// The offset of the first frame-closing label at or after `from`.
pub(super) fn frame_limit(payload: &[u8], from: usize) -> usize {
    FRAME_END_LABELS
        .iter()
        .filter_map(|label| find(payload, label, from))
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
pub(super) fn row_prefix(payload: &[u8], type_offset: usize, limit: usize) -> Option<RowPrefix> {
    field_readings(payload, type_offset + 1, limit).find_map(|(feature_id, pos)| {
        let orientation = *payload.get(pos)?;
        matches!(orientation, 0x01 | 0xf6).then_some(())?;
        let boundary_type = BoundaryType::from_byte(*payload.get(pos + 1)?)?;
        let (next_surface, end) = row_field(payload, pos + 2, limit)?;
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
    let start = type_offset.checked_sub(3).filter(|start| *start >= lower)?;
    packed_field(payload, start, type_offset)
        .filter(|(_, end)| *end == type_offset)
        .map(|(value, _)| (value, start))
}

/// Whether a packed `geom_id` can start at `offset`: at a frame start or
/// after a row close.
pub(super) fn packed_id_anchored(payload: &[u8], offset: usize, frame_starts: &[usize]) -> bool {
    frame_starts.contains(&offset)
        || offset.checked_sub(1).and_then(|before| payload.get(before)) == Some(&ROW_CLOSE)
}

/// The byte after a positional row's `next_geom_ptr`.
pub(crate) fn positional_body_start(payload: &[u8], row: &SurfaceRow) -> Option<usize> {
    let limit = frame_limit(payload, row.offset);
    let (_, after_id) = row_field(payload, row.offset, limit)?;
    (payload
        .get(after_id)
        .and_then(|byte| SurfaceKind::from_byte(*byte))
        == Some(row.kind))
    .then_some(())?;
    let prefix = row_prefix(payload, after_id, limit)?;
    (prefix.feature_id == row.feature_id).then_some(prefix.end)
}
