// SPDX-License-Identifier: Apache-2.0
//! PSB scalar forms with context-independent IEEE-754 mappings.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::bytes::find_from;
use cadmpeg_core::decode::View;

use crate::psb::{compact_int, short_form_float};

/// Counted `double_xar` dictionary stored in a model-level scalar section.
#[derive(Debug, Clone, PartialEq)]
pub struct DoubleXarTable {
    /// Offset of the `double_xar` label in the expanded section.
    pub offset: usize,
    /// Stored array extent.
    pub count: u32,
    /// Entries in stored order, including an explicit terminal null slot.
    pub entries: Vec<DoubleXarEntry>,
}

/// One stored slot in a `double_xar` dictionary.
#[derive(Debug, Clone, PartialEq)]
pub struct DoubleXarEntry {
    /// Zero-based array index.
    pub index: u32,
    /// Exact bytes occupying the slot.
    pub raw: Vec<u8>,
    /// Scalar value when the slot uses a defined literal form.
    pub value: Option<f64>,
    /// Structural token family.
    pub kind: &'static str,
}

/// Decode every complete counted `double_xar` dictionary in one expanded section.
#[must_use]
pub fn double_xar_tables(data: &[u8]) -> Vec<DoubleXarTable> {
    const LABEL: &[u8] = b"double_xar\0";
    let mut tables = Vec::new();
    let mut search = 0;
    while let Some(offset) = find_from(data, LABEL, search) {
        let count_offset = offset + LABEL.len();
        if data.get(count_offset) != Some(&0xf8) {
            search = count_offset;
            continue;
        }
        let (count, mut cursor) = compact_int(data, count_offset + 1);
        if cursor == count_offset + 1 {
            search = count_offset + 1;
            continue;
        }
        let mut entries = Vec::new();
        for index in 0..count {
            let start = cursor;
            let Some(head) = data.get(cursor).copied() else {
                entries.clear();
                break;
            };
            let (value, end, kind) = match head {
                0x0b => (Some(0.0), cursor + 1, "stock_zero"),
                0x10 => (Some(1.0), cursor + 1, "stock_one"),
                0xe0 => (None, cursor + 1, "terminal_null"),
                0xe5 if data.get(cursor..cursor + 5) == Some(&[0xe5, 0x07, 0x23, 0x11, 0x2e]) => {
                    (None, cursor + 5, "recursive_placeholder_1")
                }
                0xe8 if data.get(cursor..cursor + 4) == Some(&[0xe8, 0x26, 0xd6, 0x95]) => {
                    (None, cursor + 4, "recursive_placeholder_3")
                }
                _ => match decode(data, cursor) {
                    Some((value, end)) => (Some(value), end, "literal"),
                    None => {
                        entries.clear();
                        break;
                    }
                },
            };
            let Some(raw) = data.get(start..end) else {
                entries.clear();
                break;
            };
            entries.push(DoubleXarEntry {
                index,
                raw: raw.to_vec(),
                value,
                kind,
            });
            cursor = end;
        }
        if entries.len() == usize::try_from(count).unwrap_or(usize::MAX)
            && entries
                .last()
                .is_some_and(|entry| entry.kind == "terminal_null")
        {
            tables.push(DoubleXarTable {
                offset,
                count,
                entries,
            });
        }
        search = count_offset + 1;
    }
    tables
}

/// Section-local dictionary formed by distinct raw `0x46` token images.
#[derive(Debug, Clone, Default)]
pub struct ScalarCache {
    entries: Vec<CacheEntry>,
    /// Unique leading payload byte for each paired-form tail. `None` marks a
    /// tail shared by distinct cache images.
    paired_byte_1_by_tail: BTreeMap<[u8; 6], Option<u8>>,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    value: f64,
}

impl ScalarCache {
    /// Build the dictionary in first-appearance order from every complete
    /// eight-byte sequence beginning with `0x46` in one section.
    pub fn from_section(section: &[u8]) -> Self {
        let mut entries = Vec::<CacheEntry>::new();
        let mut seen = HashSet::<[u8; 8]>::new();
        let mut paired_byte_1_by_tail = BTreeMap::new();
        for offset in 0..section.len() {
            if section[offset] != 0x46 {
                continue;
            }
            let Some(bytes) = section.get(offset..offset + 8) else {
                continue;
            };
            let raw: [u8; 8] = bytes.try_into().expect("bounded eight-byte slice");
            if !seen.insert(raw) {
                continue;
            }
            let mut ieee = raw;
            ieee[0] = 0x40;
            let tail = raw[2..].try_into().expect("six-byte cache tail");
            let paired_byte_1 = paired_byte_1_by_tail.entry(tail).or_insert(Some(raw[1]));
            if let Some(existing) = *paired_byte_1 {
                if existing != raw[1] {
                    *paired_byte_1 = None;
                }
            }
            entries.push(CacheEntry {
                value: f64::from_be_bytes(ieee),
            });
        }
        Self {
            entries,
            paired_byte_1_by_tail,
        }
    }

    fn value(&self, index: u32) -> Option<f64> {
        self.entries
            .get(usize::try_from(index).ok()?)
            .map(|entry| entry.value)
    }

    fn paired_byte_1(&self, tail: &[u8]) -> Option<u8> {
        self.paired_byte_1_by_tail
            .get(<&[u8; 6]>::try_from(tail).ok()?)
            .copied()
            .flatten()
    }
}

// Only prefixes decoded by the generic row/f9 lane belong here. Other scalar
// lanes must classify their own prefixes before delegating to this lane.
const GENERIC_LANE_OPENERS: &[u8] = &[
    0x0d, 0x0f, 0x18, 0x29, 0x2a, 0x2d, 0x2e, 0x2f, 0x41, 0x42, 0x43, 0x46, 0x47, 0x48, 0x4b, 0x5e,
    0x66, 0x67, 0x68, 0x6a, 0x71, 0x76, 0x77, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a,
    0x8b, 0x8c, 0x8d, 0x8e, 0x8f, 0x9e, 0xa3, 0xaf, 0xb0, 0xb1, 0xb3, 0xb9, 0xbf, 0xd1, 0xd3, 0xde,
    0xdf, 0xe4, 0xe6, 0xe8,
];

/// Decode one scalar in a row or `f9` scalar lane using its section cache.
pub fn decode_in_lane(data: &[u8], offset: usize, cache: &ScalarCache) -> Option<(f64, usize)> {
    match *data.get(offset)? {
        0x18 => {
            let next = *data.get(offset + 1)?;
            if GENERIC_LANE_OPENERS.contains(&next)
                || matches!(next, 0xe0..=0xe3 | 0xf1 | 0xf2 | 0xf7 | 0xf8)
            {
                return Some((0.0, offset + 1));
            }
            let (index, end) = compact_int(data, offset + 1);
            (end > offset + 1).then(|| cache.value(index).map(|value| (value, end)))?
        }
        0x9e | 0xa3 => {
            let tail = data.get(offset + 1..offset + 7)?;
            let byte_1 = cache.paired_byte_1(tail)?;
            let mut raw = [0; 8];
            raw[0] = if data[offset] == 0x9e { 0x40 } else { 0xc0 };
            raw[1] = byte_1;
            raw[2..].copy_from_slice(tail);
            Some((f64::from_be_bytes(raw), offset + 7))
        }
        0x76 | 0xb3 => {
            let tail = data.get(offset + 1..offset + 7)?;
            let mut raw = [0; 8];
            raw[..2].copy_from_slice(if data[offset] == 0x76 {
                &[0x3f, 0xeb]
            } else {
                &[0xbf, 0xe0]
            });
            raw[2..].copy_from_slice(tail);
            Some((f64::from_be_bytes(raw), offset + 7))
        }
        0xe8 if data.get(offset + 1) == Some(&0) => Some((1.0, offset + 2)),
        _ => decode(data, offset),
    }
}

/// Decode one scalar in a positional surface or curve row lane.
///
/// Positional rows store `0x71` as a seven-byte sub-one IEEE form with an
/// implicit zero low byte. Named scalar fields use the eight-byte `0x71`
/// form handled by [`decode_in_lane`].
pub fn decode_in_row_lane(data: &[u8], offset: usize, cache: &ScalarCache) -> Option<(f64, usize)> {
    if data.get(offset) == Some(&0x18) && data.get(offset + 1) == Some(&0x0e) {
        return Some((0.0, offset + 1));
    }
    if data.get(offset) == Some(&0x0e) {
        return Some((-0.5, offset + 1));
    }
    if data.get(offset) == Some(&0x71) {
        return ieee7(data, offset, 0x3f);
    }
    decode_in_lane(data, offset, cache)
}

/// Decode one scalar in a complete positional pcurve parameter lane.
///
/// Pcurve rows use the generic row forms first. Their remaining seven-byte
/// positive DICT forms are selected by the pcurve grammar, so they are tried
/// only after the generic lane declines the prefix.
pub fn decode_in_pcurve_lane(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    decode_in_row_lane(data, offset, cache).or_else(|| decode_positive_dict(data, offset))
}

/// Decode one scalar in a positional surface-row lane.
pub fn decode_in_surface_row_lane(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    if data.get(offset) == Some(&0x18)
        && matches!(data.get(offset + 1), Some(0x73 | 0x92 | 0xa0 | 0xbb | 0xda))
    {
        return Some((0.0, offset + 1));
    }
    if data.get(offset) == Some(&0xa0) {
        let tail = data.get(offset + 1..offset + 7)?;
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&[0xc0, 0x15]);
        raw[2..].copy_from_slice(tail);
        return Some((f64::from_be_bytes(raw), offset + 7));
    }
    if matches!(data.get(offset), Some(0x92 | 0xda)) {
        let payload: [u8; 6] = data.get(offset + 1..offset + 7)?.try_into().ok()?;
        let signed = i64::from_be_bytes([
            if payload[0] & 0x80 == 0 { 0 } else { 0xff },
            if payload[0] & 0x80 == 0 { 0 } else { 0xff },
            payload[0],
            payload[1],
            payload[2],
            payload[3],
            payload[4],
            payload[5],
        ]);
        return Some((signed as f64, offset + 7));
    }
    if let Some(high) = match data.get(offset) {
        Some(0x73) => Some(0x3fe8),
        Some(0xa7) => Some(0xbfd3),
        Some(0xbb) => Some(0xbfe8),
        _ => None,
    } {
        return ieee7_dict(data, offset, high);
    }
    if let Some(high) = match data.get(offset) {
        Some(0xd1) => Some(0x3fff),
        Some(0xd3) => Some(0x4001),
        Some(0xde) => Some(0x4010),
        Some(0xdf) => Some(0x4011),
        _ => None,
    } {
        return ieee7_dict(data, offset, high);
    }
    decode_in_row_lane(data, offset, cache)
}

/// Decode one scalar in a positional torus-or-sphere surface-row lane.
///
/// This lane stores structurally delimited negative model coordinates beginning
/// with `0x2d` in a seven-byte form. The token supplies IEEE bytes one through
/// six after the fixed `0xc0` high byte; the low byte is zero. Unframed `0x2d`
/// tokens retain the generic row lane's eight-byte form.
pub fn decode_in_torus_row_lane(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    if data.get(offset) == Some(&0x2d)
        && (data.get(offset + 7).is_none()
            || matches!(
                data.get(offset + 7),
                Some(0xe0..=0xe3 | 0xf1 | 0xf2 | 0xf6..=0xf8)
            ))
    {
        let tail = data.get(offset + 1..offset + 7)?;
        let mut raw = [0; 8];
        raw[0] = 0xc0;
        raw[1..7].copy_from_slice(tail);
        return Some((f64::from_be_bytes(raw), offset + 7));
    }
    decode_in_surface_row_lane(data, offset, cache)
}

/// Decode the first coordinate of a tabulated-cylinder directrix control point.
///
/// This lane has its own signed DICT lattices and fixed-width forms. They take
/// precedence over the same prefix bytes in positional surface-row lanes.
pub fn decode_tabulated_cylinder_first_coordinate(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    let head = *data.get(offset)?;
    if head == 0x28 {
        return ieee8(data, offset, 0x3f);
    }
    if head == 0x2d {
        return ieee8(data, offset, 0x40);
    }
    if head == 0x31 {
        return ieee7(data, offset, 0x40);
    }
    if head == 0x41 {
        return ieee8(data, offset, 0x3f);
    }
    if matches!(head, 0x2c | 0x4e..=0x4f | 0x52 | 0x54 | 0x58..=0x5a) {
        return ieee7(data, offset, 0x3f);
    }
    if head == 0x45 {
        return ieee7(data, offset, 0xbf);
    }
    if data.get(offset) == Some(&0x46) {
        return ieee8(data, offset, 0xc0);
    }
    if data.get(offset) == Some(&0x4a) {
        return ieee7(data, offset, 0xc0);
    }
    if matches!(head, 0x5b..=0xa3) {
        return ieee7_dict(data, offset, 0x3f75 + u16::from(head));
    }
    if matches!(head, 0xa5..=0xa6) {
        return ieee7_dict(data, offset, 0xbf2b + u16::from(head));
    }
    if matches!(head, 0xa7..=0xae) {
        return ieee7_dict(data, offset, 0xbf2c + u16::from(head));
    }
    if matches!(head, 0xb2..=0xcf) {
        return ieee7_dict(data, offset, 0xbf2d + u16::from(head));
    }
    if matches!(head, 0xd0..=0xdc) {
        return ieee7_dict(data, offset, 0xbf2e + u16::from(head));
    }
    if head == 0xdd {
        return ieee7_dict(data, offset, 0xbf2f + u16::from(head));
    }
    if matches!(head, 0xde..=0xdf) {
        return ieee7_dict(data, offset, 0xbf32 + u16::from(head));
    }
    decode_in_surface_row_lane(data, offset, cache)
}

/// Decode one scalar in a type-24 round-edge envelope.
///
/// Round-edge envelopes use the tabulated-cylinder first-coordinate lane for
/// their two edge parameters and six endpoint coordinates. Their positive
/// DICT lattice starts at `0x4b`, rather than at the narrower range used by the
/// general tabulated-cylinder parser. The enclosing envelope grammar supplies
/// the field boundaries, so this function does not classify a prefix outside
/// that lane by itself.
pub fn decode_round_edge_coordinate(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    let prefix = *data.get(offset)?;
    if (0x4b..=0xa3).contains(&prefix) {
        let byte_1 = prefix.wrapping_add(0x75);
        let byte_0: u8 = if byte_1 >= 0x80 { 0x3f } else { 0x40 };
        return ieee7_dict(data, offset, u16::from(byte_0) << 8 | u16::from(byte_1));
    }
    decode_tabulated_cylinder_first_coordinate(data, offset, cache)
}

/// Decode the second coordinate of a tabulated-cylinder directrix control point.
///
/// Positive DICT tokens encode the first two IEEE bytes as `0x3f75 + prefix`;
/// their six-byte payload supplies the remaining bytes.
pub fn decode_tabulated_cylinder_second_coordinate(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    decode_second_coordinate_lane(data, offset, cache)
}

/// Decode a model-space coordinate in an `ActDatums` outline.
///
/// Named and positional datum outlines use the backed subset of the bounded
/// positive/negative DICT lattice for the second coordinate of a
/// tabulated-cylinder directrix. The unresolved `45` and `5c` forms are
/// excluded here so the datum walker can retain their seven-byte slot
/// boundaries without assigning numeric values.
pub(crate) fn decode_datum_outline_coordinate(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    if matches!(data.get(offset), Some(0x45 | 0x5c)) {
        return None;
    }
    decode_second_coordinate_lane(data, offset, cache)
}

fn decode_second_coordinate_lane(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    let head = *data.get(offset)?;
    if matches!(head, 0x28 | 0x41) {
        return ieee8(data, offset, 0x3f);
    }
    if head == 0x45 {
        return ieee7(data, offset, 0xbf);
    }
    if matches!(head, 0x2c | 0x4c..=0x4d | 0x50 | 0x54) {
        return ieee7(data, offset, 0x3f);
    }
    if matches!(head, 0x5c | 0x5e..=0xa3) {
        return ieee7_dict(data, offset, 0x3f75 + u16::from(head));
    }
    if matches!(head, 0xa4..=0xa6) {
        return ieee7_dict(data, offset, 0xbf2b + u16::from(head));
    }
    if matches!(head, 0xa7..=0xb1) {
        return ieee7_dict(data, offset, 0xbf2c + u16::from(head));
    }
    if matches!(head, 0xb2..=0xcf) {
        return ieee7_dict(data, offset, 0xbf2d + u16::from(head));
    }
    if matches!(head, 0xd0..=0xdc) {
        return ieee7_dict(data, offset, 0xbf2e + u16::from(head));
    }
    if head == 0xdd {
        return ieee7_dict(data, offset, 0xbf2f + u16::from(head));
    }
    if matches!(head, 0xde..=0xdf) {
        return ieee7_dict(data, offset, 0xbf32 + u16::from(head));
    }
    decode_in_surface_row_lane(data, offset, cache)
}

/// Whether `byte` opens a dedicated scalar form in the second
/// directrix-coordinate lane.
pub(crate) fn is_tabulated_cylinder_second_coordinate_opener(byte: u8) -> bool {
    matches!(
        byte,
        0x28
            | 0x2c
            | 0x41
            | 0x45
            | 0x4c..=0x4d
            | 0x50
            | 0x54
            | 0x5c
            | 0x5e..=0xdf
    )
}

/// Decode one coordinate in a named surface-prototype `local_sys` body.
///
/// Compact `0x0e` is positive one half in this lane. Positional surface rows
/// assign the negative value to the same byte.
pub fn decode_named_local_system_coordinate(
    data: &[u8],
    offset: usize,
    slot: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    if data.get(offset) == Some(&0x0e) {
        return Some((0.5, offset + 1));
    }
    if slot == 6 && data.get(offset) == Some(&0x41) {
        return ieee8(data, offset, 0xbf);
    }
    if data.get(offset) == Some(&0x5d) {
        return ieee7_dict(data, offset, 0xbfd2);
    }
    decode_tabulated_cylinder_second_coordinate(data, offset, cache)
}

/// Decode one scalar in a named analytic surface-radius field.
///
/// Prefix `0x28` supplies IEEE bytes one through seven after an implicit
/// positive subunit high byte. DICT prefixes `0x5b..=0xa3` encode the first
/// two IEEE bytes as `0x3f75 + prefix`; their six-byte payload supplies the
/// remaining bytes.
pub fn decode_named_surface_radius(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    if data.get(offset) == Some(&0x28) {
        return ieee8(data, offset, 0x3f);
    }
    let head = *data.get(offset)?;
    if GENERIC_LANE_OPENERS.contains(&head) {
        return decode_in_lane(data, offset, cache);
    }
    if matches!(head, 0x5b..=0xa3) {
        return ieee7_dict(data, offset, 0x3f75 + u16::from(head));
    }
    if head == 0xb7 {
        return decode_positive_dict(data, offset);
    }
    decode_in_lane(data, offset, cache)
}

/// Decode one scalar in a named field using the positive DICT lane.
///
/// Positive-DICT forms take precedence over generic forms when a prefix has
/// an alternate width or IEEE mapping. Prefixes `0x5b..=0xa3` encode the first
/// two IEEE bytes as `0x3f75 + prefix`; their six-byte payload supplies the
/// remaining bytes.
pub fn decode_named_positive_dict_scalar(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    let head = *data.get(offset)?;
    if matches!(head, 0x5b..=0xa3) {
        return ieee7_dict(data, offset, 0x3f75 + u16::from(head));
    }
    if matches!(head, 0x71 | 0x74 | 0x76 | 0x81 | 0x90 | 0x91 | 0xb7) {
        return decode_positive_dict(data, offset);
    }
    decode_in_lane(data, offset, cache)
}

/// Whether a byte opens a dedicated coordinate form in the named-local-system
/// lane rather than a generic compact scalar or cache reference.
pub(crate) fn is_named_local_system_coordinate_opener(byte: u8) -> bool {
    matches!(
        byte,
        0x0e | 0x28 | 0x2c | 0x41 | 0x45 | 0x4c..=0x4d | 0x50 | 0x54 | 0x5c..=0xdf
    )
}

/// Decode one coordinate in a model-reference entity row.
///
/// The `0xed` form stores a complete big-endian IEEE-754 value in the eight
/// bytes following the opener. Other coordinates use the signed DICT lane
/// shared with tabulated-cylinder control points.
pub fn decode_model_reference_coordinate(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    if matches!(data.get(offset), Some(0x19 | 0x32)) {
        return ieee8(data, offset, 0x3f);
    }
    if data.get(offset) == Some(&0xed) {
        return Some((View::f64_be_at(data, offset + 1)?, offset + 9));
    }
    decode_tabulated_cylinder_second_coordinate(data, offset, cache)
}

/// Decode a complete twelve-slot support frame using the local-system macro
/// language shared by feature definitions and curve-equation entities.
pub fn decode_explicit_local_system_slots(body: &[u8], cache: &ScalarCache) -> Option<[f64; 12]> {
    decode_local_system_slots(body, cache, LocalSystemVariant::Explicit)
}

/// Decode the feature-definition variant of the twelve-slot support frame.
pub fn decode_feature_local_system_slots(body: &[u8], cache: &ScalarCache) -> Option<[f64; 12]> {
    decode_local_system_slots(body, cache, LocalSystemVariant::Feature)
}

/// Decode the leading `f9 4 3` planar frame of a saved section conic.
///
/// Saved conic records may store additional fields after the frame, so the
/// consumed byte count is returned with the expanded frame.
pub fn decode_saved_conic_local_system_prefix(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<([f64; 12], usize)> {
    (body.first() == Some(&0xf9)).then_some(())?;
    let (rows, cursor) = compact_int(body, 1);
    let (columns, mut cursor) = compact_int(body, cursor);
    (rows == 4 && columns == 3).then_some(())?;

    let mut values = [0.0; 12];
    for (slot, value) in values.iter_mut().enumerate().take(5) {
        let (decoded, next) = decode_named_local_system_coordinate(body, cursor, slot, cache)?;
        *value = decoded;
        cursor = next;
    }
    (body.get(cursor..cursor + 3) == Some(&[0x18, 0xe5, 0x0f])).then_some(())?;
    values[5..9].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
    cursor += 3;
    for value in values.iter_mut().skip(9) {
        let (decoded, next) = decode_tabulated_cylinder_first_frame_coordinate(body, cursor, cache)
            .or_else(|| decode_in_row_lane(body, cursor, cache))?;
        *value = decoded;
        cursor = next;
    }
    Some((finite_local_system_slots(values)?, cursor))
}

/// Decode a positional plane local system, including its terminal-zero macro.
pub fn decode_positional_plane_local_system_slots(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<[f64; 12]> {
    decode_local_system_slots(body, cache, LocalSystemVariant::PositionalPlane)
}

/// Decode a positional cylinder local system whose origin uses the cylinder
/// first-coordinate lane.
pub fn decode_positional_cylinder_local_system_slots(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<[f64; 12]> {
    decode_local_system_slots(body, cache, LocalSystemVariant::PositionalCylinder)
}

/// Decode the twelve-slot local-system prefix in a positional torus body.
///
/// The returned byte count leaves the following radius suffix unconsumed.
pub fn decode_positional_torus_local_system_prefix(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<([f64; 12], usize)> {
    let prefix = decode_local_system_slot_prefix(body, cache, LocalSystemVariant::PositionalTorus)?;
    Some((finite_local_system_slots(prefix.values)?, prefix.cursor))
}

/// A local system in an inline non-plane surface row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct InlineNonPlaneLocalSystemPrefix {
    /// Expanded twelve-slot local-system values.
    pub(crate) values: [f64; 12],
    /// First byte after the local-system image.
    pub(crate) cursor: usize,
    /// Axis coordinate named by a compact image, when the image is compact.
    pub(crate) compact_axis: Option<usize>,
}

/// Decode the bounded local-system prefix used by inline non-plane rows.
///
/// This lane is separate from the older positional-cylinder grammars. Compact
/// images name only an axis coordinate; explicit frames use the three
/// component lanes and the four-slot `18 e5 0f` fill.
pub(crate) fn decode_inline_non_plane_local_system_prefix(
    body: &[u8],
    cache: &ScalarCache,
) -> Vec<InlineNonPlaneLocalSystemPrefix> {
    let mut prefixes = Vec::new();
    if let Some((axis, reference_sign, cursor)) = decode_inline_compact_image(body) {
        prefixes.push(InlineNonPlaneLocalSystemPrefix {
            values: compact_inline_frame(axis, reference_sign),
            cursor,
            compact_axis: Some(axis),
        });
    }

    let mut explicit = Vec::new();
    let mut values = [0.0; 12];
    walk_inline_explicit_local_system(body, cache, 0, 0, &mut values, &mut explicit);
    prefixes.extend(
        explicit
            .into_iter()
            .map(|(values, cursor)| InlineNonPlaneLocalSystemPrefix {
                values,
                cursor,
                compact_axis: None,
            }),
    );
    prefixes
}

/// Decode the three origin slots that follow a compact inline local-system
/// image.
pub(crate) fn decode_inline_non_plane_origin_prefix(
    body: &[u8],
    cursor: usize,
    cache: &ScalarCache,
) -> Vec<([f64; 3], usize)> {
    let mut values = [0.0; 3];
    let mut results = Vec::new();
    walk_inline_origin(body, cache, 0, cursor, &mut values, &mut results);
    results
}

/// Decode one inline family suffix scalar. The suffix has its own compact
/// unit and half-unit tokens; `0x0f` is one here, not the row-lane zero token.
pub(crate) fn decode_inline_surface_suffix_scalar(
    body: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    match body.get(offset)? {
        0x0e => Some((0.5, offset + 1)),
        0x0f => Some((1.0, offset + 1)),
        0x18 => Some((0.0, offset + 1)),
        _ => decode_positive_dict(body, offset)
            .or_else(|| decode_in_surface_row_lane(body, offset, cache)),
    }
}

fn decode_inline_compact_image(body: &[u8]) -> Option<(usize, f64, usize)> {
    const X_IMAGES: [&[u8]; 3] = [
        &[0x18, 0xe4, 0x0f, 0x18, 0x0f, 0x18, 0x10, 0x18, 0xe4],
        &[0x18, 0x10, 0x18, 0xe5, 0x10, 0x0f, 0x18, 0xe4],
        &[0x18, 0x0f, 0x18, 0xe5, 0x0f, 0xe4, 0x18, 0xe4],
    ];
    let sign = |byte| match byte {
        0x0f => Some(1.0),
        0x10 => Some(-1.0),
        _ => None,
    };
    if body.len() >= 7 {
        if body[1] == 0x18 && body[2] == 0xe5 && body[4] == 0x18 && body[5] == 0xe5 {
            let reference_sign = sign(body[0])?;
            sign(body[3])?;
            sign(body[6])?;
            return Some((2, reference_sign, 7));
        }
        if body[0] == 0x18 && body[2] == 0x18 && body[4] == 0x18 && body[5] == 0xe6 {
            let reference_sign = sign(body[1])?;
            sign(body[3])?;
            sign(body[6])?;
            return Some((2, reference_sign, 7));
        }
        if body[1] == 0x18 && body[2] == 0xe6 && body[4] == 0x18 && body[6] == 0x18 {
            let reference_sign = sign(body[0])?;
            sign(body[3])?;
            sign(body[5])?;
            return Some((1, reference_sign, 7));
        }
    }

    X_IMAGES
        .into_iter()
        .find(|image| body.starts_with(image))
        .map(|image| (0, 1.0, image.len()))
}

fn compact_inline_frame(axis: usize, reference_sign: f64) -> [f64; 12] {
    let reference_coordinate = match axis {
        0 => 1,
        1 | 2 => 0,
        _ => unreachable!("compact axis is a model coordinate"),
    };
    let transverse_coordinate = match axis {
        0 | 1 => 2,
        2 => 1,
        _ => unreachable!("compact axis is a model coordinate"),
    };
    let transverse_sign = match axis {
        1 => -reference_sign,
        0 | 2 => reference_sign,
        _ => unreachable!("compact axis is a model coordinate"),
    };
    let mut values = [0.0; 12];
    values[reference_coordinate] = reference_sign;
    values[3 + transverse_coordinate] = transverse_sign;
    values[6 + axis] = 1.0;
    values
}

fn walk_inline_explicit_local_system(
    body: &[u8],
    cache: &ScalarCache,
    slot: usize,
    cursor: usize,
    values: &mut [f64; 12],
    results: &mut Vec<([f64; 12], usize)>,
) {
    if results.len() >= 16 {
        return;
    }
    if slot == 12 {
        if let Some(values) = finite_local_system_slots(*values) {
            results.push((values, cursor));
        }
        return;
    }

    if slot == 5 && body.get(cursor..cursor + 3) == Some(&[0x18, 0xe5, 0x0f]) {
        values[slot..slot + 4].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
        walk_inline_explicit_local_system(body, cache, slot + 4, cursor + 3, values, results);
    }
    if slot == 5 && body.get(cursor..cursor + 3) == Some(&[0x18, 0xe5, 0x10]) {
        values[slot..slot + 4].copy_from_slice(&[0.0, 0.0, 0.0, -1.0]);
        walk_inline_explicit_local_system(body, cache, slot + 4, cursor + 3, values, results);
    }
    if slot <= 9 && body.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
        values[slot..slot + 3].copy_from_slice(&[0.0, 1.0, 0.0]);
        walk_inline_explicit_local_system(body, cache, slot + 3, cursor + 2, values, results);
    }

    for (value, next) in decode_inline_local_system_coordinates(body, cursor, slot, cache) {
        values[slot] = value;
        walk_inline_explicit_local_system(body, cache, slot + 1, next, values, results);
    }
}

fn walk_inline_origin(
    body: &[u8],
    cache: &ScalarCache,
    slot: usize,
    cursor: usize,
    values: &mut [f64; 3],
    results: &mut Vec<([f64; 3], usize)>,
) {
    if results.len() >= 8 {
        return;
    }
    if slot == 3 {
        results.push((*values, cursor));
        return;
    }
    for (value, next) in decode_inline_local_system_coordinates(body, cursor, slot + 9, cache) {
        values[slot] = value;
        walk_inline_origin(body, cache, slot + 1, next, values, results);
    }
}

fn decode_inline_local_system_coordinates(
    body: &[u8],
    cursor: usize,
    slot: usize,
    cache: &ScalarCache,
) -> Vec<(f64, usize)> {
    let mut candidates: Vec<(f64, usize)> = Vec::new();
    // The inline local-system lane assigns the signed IEEE form to `0x28`.
    // The same byte is positive in the generic directrix lanes, so this
    // interpretation must be selected before those lane decoders run.
    if body.get(cursor) == Some(&0x28) {
        if let Some((value, next)) = ieee8(body, cursor, 0xbf) {
            candidates.push((value, next));
        }
    }
    // Named local-system records use the reflected sign for this third-frame
    // coordinate. Keep the same slot-specific rule for positional frames.
    if slot == 6 && body.get(cursor) == Some(&0x41) {
        if let Some((value, next)) = ieee8(body, cursor, 0xbf) {
            candidates.push((value, next));
        }
    }
    if matches!(body.get(cursor), Some(0x0f | 0x10 | 0xe6)) {
        candidates.push((0.0, cursor + 1));
    }
    if body.get(cursor) == Some(&0x18) {
        candidates.push((0.0, cursor + 1));
    }
    // Keep the candidate set within the slot's coordinate lane. A prefix can
    // still be ambiguous with the generic positional row lane (for example,
    // `0x46` is signed in the first-coordinate lane and positive in the row
    // lane), but accepting both tabulated coordinate lanes turns one scalar
    // into an unrelated second frame interpretation.
    let decoded = match slot % 3 {
        0 => decode_tabulated_cylinder_first_coordinate(body, cursor, cache),
        1 => decode_tabulated_cylinder_second_coordinate(body, cursor, cache),
        _ => decode_in_surface_row_lane(body, cursor, cache),
    }
    .into_iter()
    .chain(decode_positive_dict(body, cursor))
    .chain(decode_in_row_lane(body, cursor, cache));
    for candidate in decoded {
        if candidate.0.is_finite()
            && !candidates.iter().any(|existing| {
                existing.1 == candidate.1 && existing.0.to_bits() == candidate.0.to_bits()
            })
        {
            candidates.push(candidate);
        }
    }
    candidates
}

/// Storage layout of a complete positional plane support frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaneSupportFrameLayout {
    /// The decoder expanded a compact or specialized form into support triples.
    SupportTriples,
    /// The generic form stores the parameter direction, zero rank, and plane
    /// normal as three consecutive triples.
    DirectNormalTriples,
    /// The generic form stores a 3x3 frame in row-major bytes, so its columns
    /// are the frame directions.
    MatrixColumns,
}

const EPS_PLANE_LAYOUT_NONZERO: f64 = 1e-6;
const EPS_PLANE_LAYOUT_RELATIVE: f64 = 1e-9;

fn valid_equal_scale_orthogonal_directions(first: [f64; 3], second: [f64; 3]) -> bool {
    let first_magnitude = first.iter().map(|value| value * value).sum::<f64>().sqrt();
    let second_magnitude = second.iter().map(|value| value * value).sum::<f64>().sqrt();
    let scale = first_magnitude.max(second_magnitude).max(1.0);
    first_magnitude.is_finite()
        && second_magnitude.is_finite()
        && first_magnitude > EPS_PLANE_LAYOUT_NONZERO
        && second_magnitude > EPS_PLANE_LAYOUT_NONZERO
        && (first_magnitude - second_magnitude).abs() <= EPS_PLANE_LAYOUT_RELATIVE * scale
        && first
            .into_iter()
            .zip(second)
            .map(|(first, second)| first * second)
            .sum::<f64>()
            .abs()
            <= EPS_PLANE_LAYOUT_RELATIVE * first_magnitude * second_magnitude
}

fn plane_support_layout(values: &[f64; 12], saw_zero_slot_prefix: bool) -> PlaneSupportFrameLayout {
    let direct_zero_rank = values[3..6].iter().all(|value| *value == 0.0);
    let direct_frame = direct_zero_rank
        && valid_equal_scale_orthogonal_directions(
            values[0..3].try_into().expect("three direction slots"),
            values[6..9].try_into().expect("three direction slots"),
        );
    if direct_frame {
        return PlaneSupportFrameLayout::DirectNormalTriples;
    }

    let matrix_zero_rank = [values[1], values[4], values[7]]
        .into_iter()
        .all(|value| value == 0.0);
    let matrix_frame = matrix_zero_rank
        && valid_equal_scale_orthogonal_directions(
            [values[0], values[3], values[6]],
            [values[2], values[5], values[8]],
        );
    if saw_zero_slot_prefix && matrix_frame {
        PlaneSupportFrameLayout::MatrixColumns
    } else {
        PlaneSupportFrameLayout::SupportTriples
    }
}

/// Decode a positional plane support frame and retain its storage layout.
pub(crate) fn decode_plane_support_local_system(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<([f64; 12], PlaneSupportFrameLayout)> {
    let (values, cursor, layout) =
        if let Some((values, cursor)) = decode_plane_support_special_prefix(body, cache) {
            (values, cursor, PlaneSupportFrameLayout::SupportTriples)
        } else {
            let prefix =
                decode_local_system_slot_prefix(body, cache, LocalSystemVariant::PlaneSupport)?;
            let layout = if matches!(body.first(), Some(0x0e | 0x0f | 0x10 | 0x18)) {
                // Compact support prefixes use the same numeric shape as a
                // direct frame. Their field grammar, not the values alone,
                // identifies the two in-plane support directions.
                PlaneSupportFrameLayout::SupportTriples
            } else {
                plane_support_layout(&prefix.values, prefix.saw_zero_slot_prefix)
            };
            (prefix.values, prefix.cursor, layout)
        };
    (cursor == body.len()).then_some(())?;
    Some((finite_local_system_slots(values)?, layout))
}

/// Decode a positional plane support frame whose origin uses the named
/// local-system sign for compact one-half coordinates.
#[cfg(test)]
pub(crate) fn decode_plane_support_local_system_slots(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<[f64; 12]> {
    decode_plane_support_local_system(body, cache).map(|(values, _)| values)
}

#[derive(Clone, Copy)]
enum LocalSystemVariant {
    Explicit,
    Feature,
    PositionalPlane,
    PositionalCylinder,
    PositionalTorus,
    PlaneSupport,
}

fn decode_local_system_slots(
    body: &[u8],
    cache: &ScalarCache,
    variant: LocalSystemVariant,
) -> Option<[f64; 12]> {
    let prefix = decode_local_system_slot_prefix(body, cache, variant)?;
    (prefix.cursor == body.len()).then(|| finite_local_system_slots(prefix.values))?
}

fn finite_local_system_slots(values: [f64; 12]) -> Option<[f64; 12]> {
    values.into_iter().all(f64::is_finite).then_some(values)
}

struct LocalSystemSlotPrefix {
    values: [f64; 12],
    cursor: usize,
    saw_zero_slot_prefix: bool,
}

fn decode_local_system_slot_prefix(
    body: &[u8],
    cache: &ScalarCache,
    variant: LocalSystemVariant,
) -> Option<LocalSystemSlotPrefix> {
    if matches!(variant, LocalSystemVariant::PlaneSupport) {
        if let Some(frame) = decode_plane_support_special_prefix(body, cache) {
            return Some(LocalSystemSlotPrefix {
                values: frame.0,
                cursor: frame.1,
                saw_zero_slot_prefix: false,
            });
        }
    }
    if matches!(variant, LocalSystemVariant::PositionalCylinder) {
        if let Some(frame) = decode_reflected_xy_cylinder_local_system(body, cache) {
            return Some(LocalSystemSlotPrefix {
                values: frame.0,
                cursor: frame.1,
                saw_zero_slot_prefix: false,
            });
        }
    }
    if body == [0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6] {
        return Some(LocalSystemSlotPrefix {
            values: [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            cursor: body.len(),
            saw_zero_slot_prefix: false,
        });
    }
    let mut values = Vec::with_capacity(12);
    let mut cursor = 0;
    let mut saw_zero_slot_prefix = false;
    while cursor < body.len() && values.len() < 12 {
        if body.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
            if matches!(variant, LocalSystemVariant::Feature) && values.len() == 4 {
                values.extend([0.0, 0.0, 1.0, 0.0, 0.0]);
            } else {
                values.extend([0.0, 1.0, 0.0]);
            }
            cursor += 2;
            continue;
        }
        if body.get(cursor) == Some(&0x18)
            && body
                .get(cursor + 1)
                .is_some_and(|byte| matches!(byte, 0x10 | 0xe4 | 0xe6))
        {
            saw_zero_slot_prefix = true;
            values.push(0.0);
            cursor += 1;
            continue;
        }
        if matches!(variant, LocalSystemVariant::PlaneSupport)
            && body.get(cursor) == Some(&0x18)
            && values.len() < 11
            && decode_plane_support_coordinate(body, cursor + 1, values.len() + 1, cache).is_some()
        {
            saw_zero_slot_prefix = true;
            values.push(0.0);
            cursor += 1;
            continue;
        }
        if matches!(variant, LocalSystemVariant::PositionalCylinder)
            && body.get(cursor) == Some(&0x18)
            && values.len() < 11
            && decode_positional_cylinder_support_coordinate(
                body,
                cursor + 1,
                values.len() + 1,
                cache,
            )
            .is_some()
        {
            saw_zero_slot_prefix = true;
            values.push(0.0);
            cursor += 1;
            continue;
        }
        if body.get(cursor) == Some(&0x10) {
            values.push(0.0);
            cursor += 1;
            continue;
        }
        if !matches!(variant, LocalSystemVariant::Explicit)
            && body.get(cursor) == Some(&0x18)
            && cursor + 1 == body.len()
        {
            saw_zero_slot_prefix = true;
            values.push(0.0);
            cursor += 1;
            continue;
        }
        let row = decode_in_row_lane(body, cursor, cache);
        let (value, next) = match (variant, values.len()) {
            (LocalSystemVariant::PlaneSupport, 0..=8) => {
                decode_plane_support_coordinate(body, cursor, values.len(), cache)?
            }
            (LocalSystemVariant::PlaneSupport, 9..=11) if body.get(cursor) == Some(&0x0e) => {
                (0.5, cursor + 1)
            }
            (LocalSystemVariant::PositionalPlane | LocalSystemVariant::PlaneSupport, 9) => {
                row.or_else(|| decode_tabulated_cylinder_first_coordinate(body, cursor, cache))?
            }
            (LocalSystemVariant::PositionalPlane | LocalSystemVariant::PlaneSupport, 10 | 11) => {
                row.or_else(|| decode_tabulated_cylinder_second_coordinate(body, cursor, cache))?
            }
            (LocalSystemVariant::PositionalCylinder, 0..=8) => {
                decode_positional_cylinder_support_coordinate(body, cursor, values.len(), cache)?
            }
            (LocalSystemVariant::PositionalCylinder, 9..=11) => {
                decode_tabulated_cylinder_first_coordinate(body, cursor, cache).or(row)?
            }
            (LocalSystemVariant::PositionalTorus, 6) if body.get(cursor) == Some(&0x28) => {
                ieee8(body, cursor, 0xbf)?
            }
            (LocalSystemVariant::PositionalTorus, 0..=8) => {
                decode_tabulated_cylinder_first_coordinate(body, cursor, cache).or(row)?
            }
            (LocalSystemVariant::PositionalTorus, 9..=11) => {
                row.or_else(|| decode_tabulated_cylinder_second_coordinate(body, cursor, cache))?
            }
            _ => row?,
        };
        values.push(value);
        cursor = next;
    }
    (values.len() == 12).then(|| LocalSystemSlotPrefix {
        values: values
            .try_into()
            .expect("twelve bounded local-system slots"),
        cursor,
        saw_zero_slot_prefix,
    })
}

fn decode_plane_support_special_prefix(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<([f64; 12], usize)> {
    if let Some(frame) = decode_inline_plane_support_image(body, cache) {
        return Some(frame);
    }
    if let Some(frame) = decode_normal_x_plane_support(body, cache) {
        return Some(frame);
    }
    if let Some(frame) = decode_compact_axis_plane_support(body, cache) {
        return Some(frame);
    }
    if let Some(frame) = decode_prefixed_orthogonal_plane_support(body, cache) {
        return Some(frame);
    }
    if let Some(frame) = decode_trailing_rank_orthogonal_plane_support(body, cache) {
        return Some(frame);
    }
    if let Some(frame) = decode_reflected_component_plane_support(body, cache) {
        return Some(frame);
    }
    if let Some(frame) = decode_trailing_rank_reflected_plane_support(body, cache) {
        return Some(frame);
    }
    (body == [0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6]).then_some((
        [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        body.len(),
    ))
}

/// Expand a compact axis image into the support triples used by the plane-row
/// solver. The inline image contains two in-plane directions followed by the
/// model normal; plane support stores the second in-plane direction in its
/// third triple with a zero rank triple between them.
fn decode_inline_plane_support_image(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<([f64; 12], usize)> {
    // The older compact support prefix also matches one of the generic image
    // templates. It has its own support-frame semantics and must reach the
    // legacy decoder below instead of being replayed as an inline frame.
    if !body.starts_with(&[0x18, 0xe4]) {
        return None;
    }
    let (axis, reference_sign, prefix_end) = decode_inline_compact_image(body)?;
    let inline = compact_inline_frame(axis, reference_sign);
    let (origin, cursor) = decode_plane_support_origin(body, prefix_end, cache)?;
    Some((
        [
            inline[0], inline[1], inline[2], 0.0, 0.0, 0.0, inline[3], inline[4], inline[5],
            origin[0], origin[1], origin[2],
        ],
        cursor,
    ))
}

fn decode_reflected_xy_cylinder_local_system(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<([f64; 12], usize)> {
    let (first_x, cursor) = decode_positional_cylinder_support_coordinate(body, 0, 0, cache)?;
    let (first_y, cursor) = decode_positional_cylinder_support_coordinate(body, cursor, 1, cache)?;
    (body.get(cursor) == Some(&0x18)).then_some(())?;
    let (stored_first_y, cursor) =
        decode_positional_cylinder_support_coordinate(body, cursor + 1, 3, cache)?;
    let (stored_first_x, cursor) =
        decode_positional_cylinder_support_coordinate(body, cursor, 4, cache)?;
    (body.get(cursor..cursor + 3) == Some(&[0x18, 0xe5, 0x0f])).then_some(())?;
    let mut cursor = cursor + 3;

    [first_x, first_y, stored_first_y, stored_first_x]
        .into_iter()
        .all(f64::is_finite)
        .then_some(())?;
    let scale = first_x.abs().max(first_y.abs()).max(1.0);
    ((first_x.mul_add(first_x, first_y * first_y) - 1.0).abs() <= 1e-9 * scale).then_some(())?;
    ((stored_first_y - first_y).abs() <= 1e-9 * scale).then_some(())?;
    ((stored_first_x - first_x).abs() <= 1e-9 * scale).then_some(())?;

    let mut origin = [0.0; 3];
    for value in &mut origin {
        let (decoded, next) = decode_tabulated_cylinder_first_coordinate(body, cursor, cache)
            .or_else(|| decode_in_row_lane(body, cursor, cache))?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    Some((
        [
            first_x, first_y, 0.0, first_y, -first_x, 0.0, 1.0, 0.0, 0.0, origin[0], origin[1],
            origin[2],
        ],
        cursor,
    ))
}

fn decode_positional_cylinder_support_coordinate(
    body: &[u8],
    cursor: usize,
    slot: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    if slot.is_multiple_of(3) {
        decode_tabulated_cylinder_first_coordinate(body, cursor, cache)
    } else {
        decode_tabulated_cylinder_second_coordinate(body, cursor, cache)
    }
    .or_else(|| decode_in_row_lane(body, cursor, cache))
}

const NORMAL_X_PLANE_SUPPORT_PREFIXES: [&[u8]; 2] = [
    &[0x0f, 0x18, 0xe6, 0x0f, 0x18, 0x10, 0x18],
    &[0x18, 0xe4, 0x10, 0xe4, 0x18, 0xe5, 0x0f, 0x18],
];

fn decode_normal_x_plane_support(body: &[u8], cache: &ScalarCache) -> Option<([f64; 12], usize)> {
    let prefix = NORMAL_X_PLANE_SUPPORT_PREFIXES
        .into_iter()
        .find(|prefix| body.starts_with(prefix))?;
    let (origin, cursor) = decode_plane_support_origin(body, prefix.len(), cache)?;
    Some((
        [
            0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, origin[0], origin[1], origin[2],
        ],
        cursor,
    ))
}

fn decode_compact_axis_plane_support(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<([f64; 12], usize)> {
    let prefix = [0x18, 0x0f, 0x18, 0xe5, 0x0f, 0xe4, 0x18, 0xe4];
    body.starts_with(&prefix).then_some(())?;
    let (origin, cursor) = decode_plane_support_origin(body, prefix.len(), cache)?;
    Some((
        [
            1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, origin[0], origin[1], origin[2],
        ],
        cursor,
    ))
}

fn decode_prefixed_orthogonal_plane_support(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<([f64; 12], usize)> {
    let mut cursor = 0;
    for slot in 0..3 {
        let (value, next) = decode_zero_or_plane_support_coordinate(body, cursor, slot, cache)?;
        (value == 0.0).then_some(())?;
        cursor = next;
    }
    let (first_x, next) = decode_zero_or_plane_support_coordinate(body, cursor, 3, cache)?;
    cursor = next;
    let (first_y, next) = decode_zero_or_plane_support_coordinate(body, cursor, 4, cache)?;
    (first_y == 0.0).then_some(())?;
    cursor = next;
    let (first_z, next) = decode_zero_or_plane_support_coordinate(body, cursor, 5, cache)?;
    cursor = next;
    (body.get(cursor) == Some(&0xe4)).then_some(())?;
    cursor += 1;
    let (second_y, next) = decode_zero_or_plane_support_coordinate(body, cursor, 7, cache)?;
    (second_y == 0.0).then_some(())?;
    cursor = next;
    let (stored_first_x_magnitude, next) =
        decode_zero_or_plane_support_coordinate(body, cursor, 8, cache)?;
    cursor = next;

    [first_x, first_z, stored_first_x_magnitude]
        .into_iter()
        .all(f64::is_finite)
        .then_some(())?;
    let scale = first_x.abs().max(first_z.abs()).max(1.0);
    ((first_x.mul_add(first_x, first_z * first_z) - 1.0).abs() <= 1e-9 * scale).then_some(())?;
    ((stored_first_x_magnitude.abs() - first_x.abs()).abs() <= 1e-9 * scale).then_some(())?;

    let (origin, cursor) = decode_plane_support_origin(body, cursor, cache)?;
    Some((
        [
            first_x, 0.0, first_z, 0.0, 0.0, 0.0, first_z, 0.0, -first_x, origin[0], origin[1],
            origin[2],
        ],
        cursor,
    ))
}

fn decode_trailing_rank_orthogonal_plane_support(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<([f64; 12], usize)> {
    let mut cursor = 0;
    let (first_x, next) = decode_zero_or_plane_support_coordinate(body, cursor, 0, cache)?;
    cursor = next;
    let (first_y, next) = decode_zero_or_plane_support_coordinate(body, cursor, 1, cache)?;
    (first_y == 0.0).then_some(())?;
    cursor = next;
    let (first_z, next) = decode_zero_or_plane_support_coordinate(body, cursor, 2, cache)?;
    cursor = next;
    (body.get(cursor) == Some(&0xe4)).then_some(())?;
    cursor += 1;
    let (second_y, next) = decode_zero_or_plane_support_coordinate(body, cursor, 4, cache)?;
    (second_y == 0.0).then_some(())?;
    cursor = next;
    let (stored_first_x_magnitude, next) =
        decode_zero_or_plane_support_coordinate(body, cursor, 5, cache)?;
    cursor = next;
    for slot in 6..9 {
        let (value, next) = decode_zero_or_plane_support_coordinate(body, cursor, slot, cache)?;
        (value == 0.0).then_some(())?;
        cursor = next;
    }

    [first_x, first_z, stored_first_x_magnitude]
        .into_iter()
        .all(f64::is_finite)
        .then_some(())?;
    let scale = first_x.abs().max(first_z.abs()).max(1.0);
    ((first_x.mul_add(first_x, first_z * first_z) - 1.0).abs() <= 1e-9 * scale).then_some(())?;
    ((stored_first_x_magnitude.abs() - first_x.abs()).abs() <= 1e-9 * scale).then_some(())?;

    let (origin, cursor) = decode_plane_support_origin(body, cursor, cache)?;
    Some((
        [
            first_x, 0.0, first_z, 0.0, 0.0, 0.0, first_z, 0.0, -first_x, origin[0], origin[1],
            origin[2],
        ],
        cursor,
    ))
}

fn decode_reflected_component_plane_support(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<([f64; 12], usize)> {
    let mut cursor = 0;
    let (first_x, next) = decode_zero_or_plane_support_coordinate(body, cursor, 0, cache)?;
    cursor = next;
    let (first_y, next) = decode_zero_or_plane_support_coordinate(body, cursor, 1, cache)?;
    (first_y == 0.0).then_some(())?;
    cursor = next;
    let (first_z, next) = decode_zero_or_plane_support_coordinate(body, cursor, 2, cache)?;
    cursor = next;
    for slot in 3..6 {
        let (value, next) = decode_zero_or_plane_support_coordinate(body, cursor, slot, cache)?;
        (value == 0.0).then_some(())?;
        cursor = next;
    }
    let (second_x, next) = decode_zero_or_plane_support_coordinate(body, cursor, 6, cache)?;
    cursor = next;
    let (second_y, next) = decode_zero_or_plane_support_coordinate(body, cursor, 7, cache)?;
    (second_y == 0.0).then_some(())?;
    cursor = next;
    let (stored_first_x, next) = decode_zero_or_plane_support_coordinate(body, cursor, 8, cache)?;
    cursor = next;

    [first_x, first_z, second_x, stored_first_x]
        .into_iter()
        .all(f64::is_finite)
        .then_some(())?;
    let scale = first_x.abs().max(first_z.abs()).max(1.0);
    ((first_x.mul_add(first_x, first_z * first_z) - 1.0).abs() <= 1e-9 * scale).then_some(())?;
    ((second_x - first_z).abs() <= 1e-9 * scale).then_some(())?;
    ((stored_first_x - first_x).abs() <= 1e-9 * scale).then_some(())?;

    let (origin, cursor) = decode_plane_support_origin(body, cursor, cache)?;
    Some((
        [
            first_x, 0.0, first_z, 0.0, 0.0, 0.0, first_z, 0.0, -first_x, origin[0], origin[1],
            origin[2],
        ],
        cursor,
    ))
}

fn decode_trailing_rank_reflected_plane_support(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<([f64; 12], usize)> {
    let mut cursor = 0;
    let (first_x, next) = decode_zero_or_plane_support_coordinate(body, cursor, 0, cache)?;
    (first_x == 0.0).then_some(())?;
    cursor = next;
    let (first_y, next) = decode_zero_or_plane_support_coordinate(body, cursor, 1, cache)?;
    cursor = next;
    let (first_z, next) = decode_zero_or_plane_support_coordinate(body, cursor, 2, cache)?;
    cursor = next;
    let (stored_second_x, next) = decode_zero_or_plane_support_coordinate(body, cursor, 3, cache)?;
    (stored_second_x == 0.0).then_some(())?;
    cursor = next;
    let (stored_second_y, next) = decode_zero_or_plane_support_coordinate(body, cursor, 4, cache)?;
    cursor = next;
    let (stored_first_y, next) = decode_zero_or_plane_support_coordinate(body, cursor, 5, cache)?;
    cursor = next;
    for slot in 6..8 {
        let (value, next) = decode_zero_or_plane_support_coordinate(body, cursor, slot, cache)?;
        (value == 0.0).then_some(())?;
        cursor = next;
    }
    (body.get(cursor) == Some(&0xe4)).then_some(())?;
    cursor += 1;

    [first_y, first_z, stored_second_y, stored_first_y]
        .into_iter()
        .all(f64::is_finite)
        .then_some(())?;
    let scale = first_y.abs().max(first_z.abs()).max(1.0);
    ((first_y.mul_add(first_y, first_z * first_z) - 1.0).abs() <= 1e-9 * scale).then_some(())?;
    (stored_second_y == first_z).then_some(())?;
    (stored_first_y == first_y).then_some(())?;

    let (origin, cursor) = decode_plane_support_origin(body, cursor, cache)?;
    Some((
        [
            0.0, first_y, first_z, 0.0, 0.0, 0.0, 0.0, first_z, -first_y, origin[0], origin[1],
            origin[2],
        ],
        cursor,
    ))
}

fn decode_zero_or_plane_support_coordinate(
    body: &[u8],
    offset: usize,
    slot: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    if matches!(body.get(offset), Some(0x0f | 0x10 | 0x18 | 0xe6)) {
        Some((0.0, offset + 1))
    } else {
        decode_plane_support_coordinate(body, offset, slot, cache)
    }
}

fn decode_plane_support_origin(
    body: &[u8],
    mut cursor: usize,
    cache: &ScalarCache,
) -> Option<([f64; 3], usize)> {
    let mut origin = [0.0; 3];
    for (index, value) in origin.iter_mut().enumerate() {
        if body.get(cursor) == Some(&0x0e) {
            *value = 0.5;
            cursor += 1;
            continue;
        }
        if matches!(body.get(cursor), Some(0x0f | 0x10 | 0x18 | 0xe6)) {
            cursor += 1;
            continue;
        }
        let row = decode_in_row_lane(body, cursor, cache);
        let (decoded, next) = if index == 0 {
            row.or_else(|| decode_tabulated_cylinder_first_coordinate(body, cursor, cache))?
        } else {
            row.or_else(|| decode_tabulated_cylinder_second_coordinate(body, cursor, cache))?
        };
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    Some((origin, cursor))
}

fn decode_plane_support_coordinate(
    body: &[u8],
    offset: usize,
    slot: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    if slot == 6 && body.get(offset) == Some(&0x4e) {
        return ieee7_with_prefix(body, offset, 0x3f, 0xcf);
    }
    if slot == 8 && body.get(offset) == Some(&0x50) {
        return ieee7_with_prefix(body, offset, 0xbf, 0xc2);
    }
    if slot.is_multiple_of(3) {
        decode_tabulated_cylinder_first_coordinate(body, offset, cache)
    } else {
        decode_tabulated_cylinder_second_coordinate(body, offset, cache)
    }
}

/// Decode one scalar in a replay-bound tabulated-cylinder envelope frame.
///
/// The frame otherwise uses the second-coordinate lane, but `0x4a` is a
/// seven-byte positive IEEE form with an implicit zero low byte.
pub fn decode_tabulated_cylinder_frame_coordinate(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    if data.get(offset) == Some(&0x4a) {
        return ieee7(data, offset, 0x40);
    }
    decode_tabulated_cylinder_second_coordinate(data, offset, cache)
}

/// Decode a first-directrix-coordinate slot in a replay-bound envelope frame.
///
/// These slots use the first-coordinate lane, except that frame-specific
/// `0x4a` retains its positive seven-byte form.
pub fn decode_tabulated_cylinder_first_frame_coordinate(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    if data.get(offset) == Some(&0x4a) {
        return ieee7(data, offset, 0x40);
    }
    decode_tabulated_cylinder_first_coordinate(data, offset, cache)
}

/// Decode one scalar in the positive seven-byte DICT lane.
///
/// The enclosing record grammar must establish this lane. Several prefix
/// bytes have different meanings in positional row and generic scalar lanes.
pub fn decode_positive_dict(data: &[u8], offset: usize) -> Option<(f64, usize)> {
    let prefix = *data.get(offset)?;
    let (byte_0, byte_1) = if prefix == 0xb7 {
        (0x3f, 0xe4)
    } else if (0x5b..=0xa3).contains(&prefix) {
        let byte_1 = prefix.wrapping_add(0x75);
        (if byte_1 >= 0x80 { 0x3f } else { 0x40 }, byte_1)
    } else {
        return None;
    };
    let tail = data.get(offset + 1..offset + 7)?;
    let mut raw = [0; 8];
    raw[0] = byte_0;
    raw[1] = byte_1;
    raw[2..].copy_from_slice(tail);
    Some((f64::from_be_bytes(raw), offset + 7))
}

/// Decode one scalar with a defined byte-to-IEEE mapping.
///
/// Returns the value and first unread offset. Returns `None` when the prefix
/// requires interpretation by the enclosing record grammar or input is
/// truncated.
pub fn decode(data: &[u8], offset: usize) -> Option<(f64, usize)> {
    let head = *data.get(offset)?;
    match head {
        0x0d => Some((-1.0, offset + 1)),
        0x0f | 0xe6 => Some((0.0, offset + 1)),
        0xe4 => Some((1.0, offset + 1)),
        0x29 | 0x2a | 0x2e | 0x2f | 0x42 | 0x43 | 0x47 | 0x48 => short_form_float(data, offset),
        0x46 => ieee8(data, offset, 0x40),
        0x71 => ieee8(data, offset, 0x3f),
        0x2d => ieee8(data, offset, 0xc0),
        0x6a => ieee7(data, offset, 0x40),
        0x5e => ieee7_with_prefix(data, offset, 0x3f, 0xd3),
        0xa3 => ieee7(data, offset, 0xc0),
        0xb9 | 0xd1 | 0xd3 | 0xde | 0xdf | 0xaf | 0xb0 | 0xb1 | 0xbf => ieee7(data, offset, 0xbf),
        0x41 | 0x4b | 0x66 | 0x67 | 0x68 | 0x77 | 0x82..=0x8f => ieee7(data, offset, 0x3f),
        _ => None,
    }
}

fn ieee8(data: &[u8], offset: usize, first: u8) -> Option<(f64, usize)> {
    let tail = data.get(offset + 1..offset + 8)?;
    let mut raw = [0; 8];
    raw[0] = first;
    raw[1..].copy_from_slice(tail);
    Some((f64::from_be_bytes(raw), offset + 8))
}
fn ieee7(data: &[u8], offset: usize, first: u8) -> Option<(f64, usize)> {
    let tail = data.get(offset + 1..offset + 7)?;
    let mut raw = [0; 8];
    raw[0] = first;
    raw[1..7].copy_from_slice(tail);
    Some((f64::from_be_bytes(raw), offset + 7))
}

fn ieee7_with_prefix(data: &[u8], offset: usize, first: u8, second: u8) -> Option<(f64, usize)> {
    let tail = data.get(offset + 1..offset + 7)?;
    let mut raw = [0; 8];
    raw[0] = first;
    raw[1] = second;
    raw[2..].copy_from_slice(tail);
    Some((f64::from_be_bytes(raw), offset + 7))
}

fn ieee7_dict(data: &[u8], offset: usize, high: u16) -> Option<(f64, usize)> {
    let tail = data.get(offset + 1..offset + 7)?;
    let mut raw = [0; 8];
    raw[..2].copy_from_slice(&high.to_be_bytes());
    raw[2..].copy_from_slice(tail);
    Some((f64::from_be_bytes(raw), offset + 7))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn positive_subunit_coordinate(value: f64) -> [u8; 8] {
        let bytes = value.to_be_bytes();
        assert_eq!(bytes[0], 0x3f);
        [
            0x41, bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]
    }

    #[test]
    fn round_edge_positive_dict_uses_the_extended_prefix_lattice() {
        let low = [0x4b, 0, 0, 0, 0, 0, 0];
        let high = [0xa3, 0, 0, 0, 0, 0, 0];
        let cache = ScalarCache::default();
        let (low_value, low_end) = decode_round_edge_coordinate(&low, 0, &cache)
            .expect("low extended positive-DICT prefix");
        let (high_value, high_end) = decode_round_edge_coordinate(&high, 0, &cache)
            .expect("high extended positive-DICT prefix");
        assert_eq!(low_end, low.len());
        assert_eq!(high_end, high.len());
        assert_eq!(
            low_value,
            f64::from_be_bytes([0x3f, 0xc0, 0, 0, 0, 0, 0, 0])
        );
        assert_eq!(
            high_value,
            f64::from_be_bytes([0x40, 0x18, 0, 0, 0, 0, 0, 0])
        );
    }

    #[test]
    fn positional_plane_origin_x_prefers_row_then_signed_first_coordinate_lanes() {
        let cache = ScalarCache::from_section(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
        let body = [
            0x10, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x0f, 0x4a, 0x08, 0, 0, 0, 0, 0, 0x18, 0x00, 0x0f,
        ];

        assert_eq!(
            decode_positional_plane_local_system_slots(&body, &cache),
            Some([0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -3.0, 3.0, 0.0])
        );
        assert!(decode_explicit_local_system_slots(&body, &cache).is_none());

        let fixed = [
            0x10, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x0f, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x18, 0x00,
            0x0f,
        ];
        assert_eq!(
            decode_positional_plane_local_system_slots(&fixed, &cache).map(|slots| slots[9]),
            Some(3.0)
        );

        let dict_origin = [
            0x18, 0xe4, 0x10, 0x18, 0x0f, 0x18, 0x0f, 0x18, 0xe4, 0x9f, 0x77, 0xa7, 0x70, 0x76,
            0xc8, 0xb8, 0x2d, 0x1e, 0, 0, 0, 0, 0, 0x65, 0xb9, 0x11, 0x9e, 0xed, 0x48, 0x6f, 0x9e,
        ];
        assert_eq!(
            decode_positional_plane_local_system_slots(&dict_origin, &cache)
                .map(|slots| { [slots[9], slots[10], slots[11]] }),
            Some([
                f64::from_be_bytes([0x40, 0x14, 0x77, 0xa7, 0x70, 0x76, 0xc8, 0xb8]),
                f64::from_be_bytes([0xc0, 0x1e, 0, 0, 0, 0, 0, 0x65]),
                f64::from_be_bytes([0xbf, 0x11, 0x9e, 0xed, 0x48, 0x6f, 0x9e, 0]),
            ])
        );
    }

    #[test]
    fn cache_zero_prefix_recognizes_every_short_float_opener() {
        let cache = ScalarCache::default();
        for token in [
            [0x29, 0xe8, 0x00],
            [0x2a, 0xfa, 0x00],
            [0x2e, 0x00, 0x00],
            [0x2f, 0x05, 0x00],
            [0x42, 0xe8, 0x00],
            [0x43, 0xfa, 0x00],
            [0x47, 0x00, 0x00],
            [0x48, 0x05, 0x00],
        ] {
            let body = [0x18, token[0], token[1], token[2]];
            assert_eq!(decode_in_lane(&body, 0, &cache), Some((0.0, 1)));
            assert_eq!(
                decode_in_lane(&body, 1, &cache).map(|(_, end)| end),
                Some(4)
            );
        }
    }

    #[test]
    fn rank_two_image_is_shared_by_all_twelve_slot_local_system_lanes() {
        let body = [0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6];
        let expected = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let cache = ScalarCache::default();

        assert_eq!(
            decode_positional_plane_local_system_slots(&body, &cache),
            Some(expected)
        );
        assert_eq!(
            decode_explicit_local_system_slots(&body, &cache),
            Some(expected)
        );
        assert_eq!(
            decode_feature_local_system_slots(&body, &cache),
            Some(expected)
        );
        assert_eq!(
            decode_positional_cylinder_local_system_slots(&body, &cache),
            Some(expected)
        );
        assert_eq!(
            decode_plane_support_local_system_slots(&body, &cache),
            Some(expected)
        );
    }

    #[test]
    fn inline_non_plane_compact_images_name_the_axis_coordinate() {
        let cases = [
            (
                vec![0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x0f],
                2,
                [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            ),
            (
                vec![0x18, 0x10, 0x18, 0x10, 0x18, 0xe6, 0x10],
                2,
                [-1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            ),
            (
                vec![0x0f, 0x18, 0xe6, 0x0f, 0x18, 0x10, 0x18],
                1,
                [1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            ),
            (
                vec![0x18, 0xe4, 0x0f, 0x18, 0x0f, 0x18, 0x10, 0x18, 0xe4],
                0,
                [0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
        ];
        let cache = ScalarCache::default();
        for (body, axis, expected) in cases {
            let prefix = decode_inline_non_plane_local_system_prefix(&body, &cache)
                .into_iter()
                .find(|prefix| prefix.compact_axis == Some(axis))
                .unwrap_or_else(|| panic!("compact inline local-system image {body:02x?}"));
            assert_eq!(prefix.cursor, body.len());
            assert_eq!(prefix.values, expected);
        }
    }

    #[test]
    fn plane_support_reuses_the_compact_x_axis_image_as_in_plane_supports() {
        let mut body = vec![0x18, 0xe4, 0x0f, 0x18, 0x0f, 0x18, 0x10, 0x18, 0xe4];
        body.extend([0x18, 0x18, 0x18]);

        assert_eq!(
            decode_plane_support_local_system(&body, &ScalarCache::default()),
            Some((
                [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,],
                PlaneSupportFrameLayout::SupportTriples,
            ))
        );
    }

    #[test]
    fn inline_non_plane_explicit_frames_expand_the_four_slot_fill() {
        let body = [
            0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x2f, 0x00, 0x00, 0x2f, 0x00, 0x00,
            0x2f, 0x10, 0x00,
        ];
        let expected = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 4.0];
        let cache = ScalarCache::default();

        assert!(decode_inline_non_plane_local_system_prefix(&body, &cache)
            .into_iter()
            .any(|prefix| prefix.compact_axis.is_none()
                && prefix.cursor == body.len()
                && prefix.values == expected));
    }

    #[test]
    fn inline_non_plane_explicit_frames_accept_the_reflected_four_slot_fill() {
        let body = [
            0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x18, 0xe5, 0x10, 0x2f, 0x00, 0x00, 0x2f, 0x00, 0x00,
            0x2f, 0x10, 0x00,
        ];
        let expected = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0, 2.0, 2.0, 4.0];
        let cache = ScalarCache::default();

        assert!(decode_inline_non_plane_local_system_prefix(&body, &cache)
            .into_iter()
            .any(|prefix| prefix.compact_axis.is_none()
                && prefix.cursor == body.len()
                && prefix.values == expected));
    }

    #[test]
    fn inline_surface_suffix_uses_exact_zero_and_unit_tokens() {
        let cache = ScalarCache::default();
        assert_eq!(
            decode_inline_surface_suffix_scalar(&[0x0e], 0, &cache),
            Some((0.5, 1))
        );
        assert_eq!(
            decode_inline_surface_suffix_scalar(&[0x0f], 0, &cache),
            Some((1.0, 1))
        );
        assert_eq!(
            decode_inline_surface_suffix_scalar(&[0x18], 0, &cache),
            Some((0.0, 1))
        );
    }

    #[test]
    fn positive_dict_arithmetic_covers_unlisted_inline_prefixes() {
        for prefix in [0x78_u8, 0x7a_u8] {
            let bytes = [prefix, 0, 0, 0, 0, 0, 0];
            let byte_1 = prefix.wrapping_add(0x75);
            assert_eq!(
                decode_positive_dict(&bytes, 0),
                Some((f64::from_be_bytes([0x3f, byte_1, 0, 0, 0, 0, 0, 0]), 7))
            );
        }
    }

    #[test]
    fn inline_non_plane_negative_lanes_follow_their_prefix_arithmetic() {
        let cache = ScalarCache::default();
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&[0xd5, 0, 0, 0, 0, 0, 0], 0, &cache),
            Some((f64::from_be_bytes([0xc0, 0x03, 0, 0, 0, 0, 0, 0]), 7))
        );
        for (prefix, byte_1) in [(0xc0, 0xed), (0xc1, 0xee), (0xc2, 0xef)] {
            assert_eq!(
                decode_tabulated_cylinder_first_coordinate(&[prefix, 0, 0, 0, 0, 0, 0], 0, &cache,),
                Some((f64::from_be_bytes([0xbf, byte_1, 0, 0, 0, 0, 0, 0]), 7))
            );
        }
    }

    #[test]
    fn plane_support_layout_keeps_an_invalid_generic_frame_unresolved() {
        let cache = ScalarCache::from_section(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
        let mut body = vec![0x46, 0x08, 0, 0, 0, 0, 0, 0];
        body.extend([0x10; 10]);
        body.push(0x18);

        assert_eq!(
            decode_plane_support_local_system(&body, &cache).map(|(_, layout)| layout),
            Some(PlaneSupportFrameLayout::SupportTriples)
        );
        assert_eq!(
            decode_plane_support_local_system(&[0x10; 12], &ScalarCache::default())
                .map(|(_, layout)| layout),
            Some(PlaneSupportFrameLayout::SupportTriples)
        );
        assert_eq!(
            plane_support_layout(
                &[
                    1.0, 0.0, 0.0, // first matrix row
                    0.0, 0.0, 1.0, // second matrix row
                    0.0, 0.0, 0.0, // third matrix row
                    0.0, 0.0, 0.0,
                ],
                true,
            ),
            PlaneSupportFrameLayout::MatrixColumns
        );
        assert_eq!(
            plane_support_layout(
                &[
                    0.6, 0.0, 0.8, // direct parameter direction
                    0.0, 0.0, 0.0, // direct zero rank
                    0.8, 0.0, -0.6, // direct plane normal
                    0.0, 0.0, 0.0,
                ],
                true,
            ),
            PlaneSupportFrameLayout::DirectNormalTriples
        );
    }

    #[test]
    fn complete_local_system_rejects_a_nonfinite_slot() {
        let cache = ScalarCache {
            entries: vec![CacheEntry { value: 1.0 }, CacheEntry { value: f64::NAN }],
            paired_byte_1_by_tail: BTreeMap::new(),
        };
        let mut body = Vec::new();
        for _ in 0..9 {
            body.extend_from_slice(&[0x18, 0x00]);
        }
        for _ in 0..3 {
            body.extend_from_slice(&[0x18, 0x01]);
        }

        assert!(decode_feature_local_system_slots(&body, &cache).is_none());
    }

    #[test]
    fn saved_conic_local_system_expands_its_planar_normal() {
        let body = [
            0xf9, 4, 3, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x0f, 0x0f, 0x0f,
        ];

        assert_eq!(
            decode_saved_conic_local_system_prefix(&body, &ScalarCache::default()),
            Some((
                [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
                body.len()
            ))
        );
    }

    #[test]
    fn positional_plane_origin_yz_fall_back_to_the_second_coordinate_lane() {
        let body = [
            0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x0f, 0x9f, 0x77, 0xa7, 0x70, 0x76, 0xc8, 0xb8,
            0x2d, 0x1e, 0, 0, 0, 0, 0, 0x65, 0xad, 0x53, 0xd5, 0xa1, 0x38, 0xce, 0xd8,
        ];

        assert_eq!(
            decode_positional_plane_local_system_slots(&body, &ScalarCache::default())
                .map(|slots| [slots[9], slots[10], slots[11]]),
            Some([
                f64::from_be_bytes([0x40, 0x14, 0x77, 0xa7, 0x70, 0x76, 0xc8, 0xb8]),
                f64::from_be_bytes([0xc0, 0x1e, 0, 0, 0, 0, 0, 0x65]),
                f64::from_be_bytes([0xbf, 0xd9, 0x53, 0xd5, 0xa1, 0x38, 0xce, 0xd8]),
            ])
        );
    }

    #[test]
    fn plane_support_origin_uses_positive_compact_half() {
        let body = [
            0x18, 0xe4, 0x0f, 0x18, 0x0f, 0x18, 0x10, 0x18, 0xe4, 0x0e, 0x18, 0xe4,
        ];
        let cache = ScalarCache::default();

        assert_eq!(
            decode_plane_support_local_system_slots(&body, &cache),
            Some([0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.5, 0.0, 1.0])
        );
        assert_eq!(
            decode_positional_plane_local_system_slots(&body, &cache).map(|slots| slots[9]),
            Some(-0.5)
        );
    }

    #[test]
    fn plane_support_directions_use_component_coordinate_lanes() {
        let body = [
            0x4e, 0xf0, 0, 0, 0, 0, 0,    // first direction x = 1
            0x18, // first direction y = 0
            0x4c, 0xf0, 0, 0, 0, 0, 0, // first direction z = 1
            0x10, 0x10, 0x10, // zero rank marker
            0x10, 0x10, 0x4c, 0xf0, 0, 0, 0, 0, 0, // second direction
            0x10, 0x10, 0x18, // origin
        ];

        assert_eq!(
            decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
            Some([1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0])
        );
        assert!(
            decode_positional_plane_local_system_slots(&body, &ScalarCache::default()).is_none()
        );
    }

    #[test]
    fn plane_support_prefix_constructs_orthogonal_directions() {
        let body = [
            0x18, 0x0f, 0x18, 0x18, 0x18, 0xe4, 0xe4, 0x18, 0x18, 0x18, 0x18, 0x18,
        ];

        assert_eq!(
            decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
            Some([0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -0.0, 0.0, 0.0, 0.0])
        );

        let other_axis = [
            0x18, 0x0f, 0x18, 0xe4, 0x18, 0x18, 0xe4, 0x18, 0xe4, 0x18, 0x18, 0x18,
        ];
        let slots = decode_plane_support_local_system_slots(&other_axis, &ScalarCache::default())
            .expect("complete orthogonal-copy frame");
        assert_eq!(slots[0..3], [1.0, 0.0, 0.0]);
        assert_eq!(slots[3..6], [0.0, 0.0, 0.0]);
        assert_eq!(slots[6..9], [0.0, 0.0, -1.0]);
    }

    #[test]
    fn compact_axis_plane_support_decodes_rank_and_origin() {
        let body = [
            0x18, 0x0f, 0x18, 0xe5, 0x0f, 0xe4, 0x18, 0xe4, 0x18, 0x18, 0x18,
        ];

        assert_eq!(
            decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
            Some([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0])
        );
    }

    #[test]
    fn normal_x_plane_support_prefixes_decode_rank_and_origin() {
        let origin = [0x2f, 0x02, 0x00, 0x2a, 0xe8, 0x00, 0x2f, 0x26, 0x00];

        for prefix in NORMAL_X_PLANE_SUPPORT_PREFIXES {
            let body = [prefix, &origin].concat();
            assert_eq!(
                decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
                Some([0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.25, 0.75, 11.0])
            );
        }
    }

    #[test]
    fn normal_x_plane_support_requires_a_complete_bounded_origin() {
        for prefix in NORMAL_X_PLANE_SUPPORT_PREFIXES {
            let mut truncated = prefix.to_vec();
            truncated.extend([0x18, 0x18]);
            assert!(
                decode_plane_support_local_system_slots(&truncated, &ScalarCache::default())
                    .is_none()
            );

            let mut trailing = prefix.to_vec();
            trailing.extend([0x18, 0x18, 0x18, 0x18]);
            assert!(
                decode_plane_support_local_system_slots(&trailing, &ScalarCache::default())
                    .is_none()
            );
        }
    }

    #[test]
    fn trailing_rank_plane_support_constructs_orthogonal_directions() {
        let body = [
            0xe4, 0x18, 0x18, 0xe4, 0x18, 0xe4, 0x18, 0x0f, 0x18, 0x18, 0x18, 0x18,
        ];

        assert_eq!(
            decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
            Some([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0])
        );
    }

    #[test]
    fn reflected_plane_support_component_constructs_orthogonal_directions() {
        let mut body = Vec::new();
        body.extend_from_slice(&positive_subunit_coordinate(0.6));
        body.push(0x18);
        body.extend_from_slice(&positive_subunit_coordinate(0.8));
        body.extend_from_slice(&[0x18, 0x0f, 0x18]);
        body.extend_from_slice(&positive_subunit_coordinate(0.8));
        body.push(0x18);
        body.extend_from_slice(&positive_subunit_coordinate(0.6));
        body.extend_from_slice(&[0x18, 0x18, 0x18]);

        assert_eq!(
            decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
            Some([0.6, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0, -0.6, 0.0, 0.0, 0.0])
        );
    }

    #[test]
    fn trailing_rank_reflected_plane_support_constructs_orthogonal_directions() {
        let mut body = vec![0x18];
        body.extend_from_slice(&positive_subunit_coordinate(0.6));
        body.extend_from_slice(&positive_subunit_coordinate(0.8));
        body.push(0x18);
        body.extend_from_slice(&positive_subunit_coordinate(0.8));
        body.extend_from_slice(&positive_subunit_coordinate(0.6));
        body.extend_from_slice(&[0x18, 0x18, 0xe4, 0x18, 0x18, 0x18]);

        assert_eq!(
            decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
            Some([0.0, 0.6, 0.8, 0.0, 0.0, 0.0, 0.0, 0.8, -0.6, 0.0, 0.0, 0.0])
        );
    }

    #[test]
    fn plane_support_slot_eight_decodes_compact_negated_component() {
        let first_x = f64::from_be_bytes([0x3f, 0xc2, 0, 0, 0, 0, 0, 0]);
        let first_z = (1.0 - first_x * first_x).sqrt();
        let first_x_bytes = positive_subunit_coordinate(first_x);
        let mut body = Vec::new();
        body.extend_from_slice(&first_x_bytes);
        body.push(0x18);
        body.extend_from_slice(&positive_subunit_coordinate(first_z));
        body.extend_from_slice(&[0x18, 0x0f, 0x18]);
        body.extend_from_slice(&positive_subunit_coordinate(first_z));
        body.push(0x18);
        body.push(0x50);
        body.extend_from_slice(&first_x_bytes[2..]);
        body.extend_from_slice(&[0x18, 0x18, 0x18]);

        assert_eq!(
            decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
            Some([first_x, 0.0, first_z, 0.0, 0.0, 0.0, first_z, 0.0, -first_x, 0.0, 0.0, 0.0])
        );
    }

    #[test]
    fn plane_support_slot_six_decodes_paired_positive_component() {
        let first_x = 0.75;
        let first_z = f64::from_be_bytes([0xbf, 0xcf, 0, 0, 0, 0, 0, 0]);
        let second_x = -first_z;
        let mut body = Vec::new();
        body.extend_from_slice(&positive_subunit_coordinate(first_x));
        body.push(0x18);
        body.extend_from_slice(&[0xa4, 0, 0, 0, 0, 0, 0]);
        body.extend_from_slice(&[0x18, 0x0f, 0x18]);
        body.extend_from_slice(&[0x4e, 0, 0, 0, 0, 0, 0]);
        body.push(0x18);
        body.extend_from_slice(&positive_subunit_coordinate(first_x));
        body.extend_from_slice(&[0x18, 0x18, 0x18]);

        assert_eq!(
            decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
            Some([first_x, 0.0, first_z, 0.0, 0.0, 0.0, second_x, 0.0, first_x, 0.0, 0.0, 0.0])
        );
    }

    #[test]
    fn decodes_model_reference_wrapped_ieee_coordinate() {
        let data = [0xed, 0x3b, 0xbc, 0xea, 0x89, 0x1b, 0xc2, 0xbd, 0x60];
        let cache = ScalarCache::default();
        assert_eq!(
            decode_model_reference_coordinate(&data, 0, &cache),
            Some((
                f64::from_be_bytes(data[1..].try_into().expect("required invariant")),
                9
            ))
        );
        assert_eq!(
            decode_model_reference_coordinate(&data[..8], 0, &cache),
            None
        );
    }

    #[test]
    fn decodes_model_reference_positive_ieee_coordinate() {
        let data = [0x32, 0xb3, 0xa2, 0x70, 0xe5, 0xa0, 0x3f, 0xfa];
        let cache = ScalarCache::default();
        assert_eq!(
            decode_model_reference_coordinate(&data, 0, &cache),
            Some((
                f64::from_be_bytes([0x3f, 0xb3, 0xa2, 0x70, 0xe5, 0xa0, 0x3f, 0xfa]),
                8
            ))
        );
        assert_eq!(
            decode_model_reference_coordinate(&data[..7], 0, &cache),
            None
        );
    }

    #[test]
    fn decodes_model_reference_low_positive_ieee_coordinate() {
        let data = [0x19, 0xc3, 0xa2, 0x70, 0xe5, 0xa0, 0x3f, 0xfd];
        let cache = ScalarCache::default();
        assert_eq!(
            decode_model_reference_coordinate(&data, 0, &cache),
            Some((
                f64::from_be_bytes([0x3f, 0xc3, 0xa2, 0x70, 0xe5, 0xa0, 0x3f, 0xfd]),
                8
            ))
        );
    }

    #[test]
    fn decodes_counted_double_xar_dictionary() {
        let mut data = b"prefix double_xar\0".to_vec();
        data.extend_from_slice(&[
            0xf8, 0x07, 0x10, 0xe5, 0x07, 0x23, 0x11, 0x2e, 0x0b, 0xe8, 0x26, 0xd6, 0x95, 0x46,
            0x08, 0, 0, 0, 0, 0, 0, 0x0b, 0xe0,
        ]);
        let tables = double_xar_tables(&data);
        let [table] = tables.as_slice() else {
            panic!("complete dictionary");
        };
        assert_eq!(table.count, 7);
        assert_eq!(table.entries[0].value, Some(1.0));
        assert_eq!(table.entries[1].kind, "recursive_placeholder_1");
        assert_eq!(table.entries[2].value, Some(0.0));
        assert_eq!(table.entries[3].kind, "recursive_placeholder_3");
        assert_eq!(table.entries[4].value, Some(3.0));
        assert_eq!(table.entries[5].value, Some(0.0));
        assert_eq!(table.entries[6].kind, "terminal_null");
    }

    #[test]
    fn withholds_incomplete_double_xar_dictionary() {
        assert!(double_xar_tables(b"double_xar\0\xf8\x02\x10").is_empty());
        assert!(double_xar_tables(b"double_xar\0\xf8\x02\x10\x0b").is_empty());
    }

    #[test]
    fn decodes_defined_ieee_forms() {
        assert_eq!(decode(&[0xe4], 0), Some((1.0, 1)));
        assert_eq!(decode(&[0x0d], 0), Some((-1.0, 1)));
        assert_eq!(decode(&[0x46, 0x08, 0, 0, 0, 0, 0, 0], 0), Some((3.0, 8)));
        assert_eq!(decode(&[0x6a, 0x08, 0, 0, 0, 0, 0], 0), Some((3.0, 7)));
        assert_eq!(
            decode(&[0x5e, 0x33, 0x33, 0x33, 0x33, 0x33, 0x2c], 0),
            Some((
                f64::from_be_bytes([0x3f, 0xd3, 0x33, 0x33, 0x33, 0x33, 0x33, 0x2c]),
                7
            ))
        );
        assert_eq!(decode(&[0x2d, 0x08, 0, 0, 0, 0, 0, 0], 0), Some((-3.0, 8)));
        assert_eq!(
            decode(&[0xde, 0x5c, 0xfa, 0x99, 0x80, 0x36, 0x84], 0),
            Some((
                f64::from_be_bytes([0xbf, 0x5c, 0xfa, 0x99, 0x80, 0x36, 0x84, 0]),
                7
            ))
        );
    }

    #[test]
    fn torus_row_lane_decodes_seven_byte_negative_coordinates() {
        let data = [0x2d, 0x1c, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf6];
        let cache = ScalarCache::default();

        assert_eq!(decode_in_torus_row_lane(&data, 0, &cache), Some((-7.0, 7)));
        assert_ne!(
            decode_in_surface_row_lane(&data, 0, &cache),
            Some((-7.0, 7))
        );
    }

    #[test]
    fn decodes_tabulated_cylinder_coordinate_lanes() {
        let cache = ScalarCache::default();
        let first_eight = [0x46, 0x13, 0x77, 0x9f, 0x89, 0x00, 0x00, 0x00];
        let first = [0x4a, 0x13, 0x21, 0xe3, 0xe3, 0x00, 0x00];
        let first_positive_dict = [0x96, 0x02, 0xf4, 0x7a, 0, 0, 0];
        let first_negative_dict = [0xd7, 0xd4, 0x8d, 0x46, 0, 0, 0];
        let first_negative_subunit = [0xc8, 0xd6, 0xa3, 0x0c, 0, 0, 0];
        let first_negative_large = [0xde, 0xbe, 0x21, 0xc3, 0, 0, 0];
        let first_negative_reserved_gap = [0xdd, 0x9f, 0xe4, 0x46, 0, 0, 0];
        let first_negative_subunit_gap = [0xa7, 0x6b, 0x7c, 0x32, 0x0d, 0x03, 0xd0];
        let first_positive_seven = [0x54, 0xad, 0xf7, 0xa0, 0, 0, 0];
        let first_positive_eight = [0x41, 0xb9, 0x9d, 0x5b, 0x81, 0x25, 0x62, 0xc0];
        let first_negative_low = [0xb2, 0x05, 0xe8, 0xa6, 0, 0, 0];
        let second = [0x7f, 0x24, 0x57, 0x89, 0x13, 0x66, 0x08];
        let second_positive_low = [0x69, 0x91, 0x22, 0x33, 0x44, 0x55, 0x66];
        let second_positive_lower_dict = [0x5c, 0x47, 0x59, 0x45, 0x2d, 0x97, 0x90];
        let second_negative_fixed = [0x45, 0xa7, 0x21, 0x45, 0x78, 0x5e, 0x04];
        let second_negative = [0xc7, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77];
        let second_negative_large = [0xdd, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77];
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&first_eight, 0, &cache),
            Some((
                f64::from_be_bytes([0xc0, 0x13, 0x77, 0x9f, 0x89, 0, 0, 0]),
                8
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&first, 0, &cache),
            Some((
                f64::from_be_bytes([0xc0, 0x13, 0x21, 0xe3, 0xe3, 0x00, 0x00, 0]),
                7
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&first_positive_dict, 0, &cache),
            Some((
                f64::from_be_bytes([0x40, 0x0b, 0x02, 0xf4, 0x7a, 0, 0, 0]),
                7
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&first_negative_dict, 0, &cache),
            Some((
                f64::from_be_bytes([0xc0, 0x05, 0xd4, 0x8d, 0x46, 0, 0, 0]),
                7
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&first_negative_subunit, 0, &cache),
            Some((
                f64::from_be_bytes([0xbf, 0xf5, 0xd6, 0xa3, 0x0c, 0, 0, 0]),
                7
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&first_negative_large, 0, &cache),
            Some((
                f64::from_be_bytes([0xc0, 0x10, 0xbe, 0x21, 0xc3, 0, 0, 0]),
                7
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&first_negative_reserved_gap, 0, &cache),
            Some((
                f64::from_be_bytes([0xc0, 0x0c, 0x9f, 0xe4, 0x46, 0, 0, 0]),
                7
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&first_negative_subunit_gap, 0, &cache),
            Some((
                f64::from_be_bytes([0xbf, 0xd3, 0x6b, 0x7c, 0x32, 0x0d, 0x03, 0xd0]),
                7
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&first_positive_seven, 0, &cache),
            Some((f64::from_be_bytes([0x3f, 0xad, 0xf7, 0xa0, 0, 0, 0, 0]), 7))
        );
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&first_positive_eight, 0, &cache),
            Some((
                f64::from_be_bytes([0x3f, 0xb9, 0x9d, 0x5b, 0x81, 0x25, 0x62, 0xc0]),
                8
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&first_negative_low, 0, &cache),
            Some((
                f64::from_be_bytes([0xbf, 0xdf, 0x05, 0xe8, 0xa6, 0, 0, 0]),
                7
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_second_coordinate(&second, 0, &cache),
            Some((
                f64::from_be_bytes([0x3f, 0xf4, 0x24, 0x57, 0x89, 0x13, 0x66, 0x08]),
                7
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_second_coordinate(&second_positive_low, 0, &cache),
            Some((
                f64::from_be_bytes([0x3f, 0xde, 0x91, 0x22, 0x33, 0x44, 0x55, 0x66]),
                7
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_second_coordinate(&second_positive_lower_dict, 0, &cache),
            Some((
                f64::from_be_bytes([0x3f, 0xd1, 0x47, 0x59, 0x45, 0x2d, 0x97, 0x90]),
                7
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_second_coordinate(&second_negative_fixed, 0, &cache),
            Some((
                f64::from_be_bytes([0xbf, 0xa7, 0x21, 0x45, 0x78, 0x5e, 0x04, 0]),
                7
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_second_coordinate(&second_negative, 0, &cache),
            Some((
                f64::from_be_bytes([0xbf, 0xf4, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]),
                7
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_second_coordinate(&second_negative_large, 0, &cache),
            Some((
                f64::from_be_bytes([0xc0, 0x0c, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]),
                7
            ))
        );
    }

    #[test]
    fn surface_row_lane_decodes_large_positive_dict_forms() {
        let cache = ScalarCache::default();
        assert_eq!(
            decode_in_surface_row_lane(&[0xd1, 0xf1, 0x60, 0x5a, 0xa4, 0xd9, 0x00], 0, &cache),
            Some((
                f64::from_be_bytes([0x3f, 0xff, 0xf1, 0x60, 0x5a, 0xa4, 0xd9, 0x00]),
                7
            ))
        );
        assert_eq!(
            decode_in_surface_row_lane(&[0xd3, 0x65, 0x1a, 0x84, 0x5c, 0xa9, 0xf0], 0, &cache),
            Some((
                f64::from_be_bytes([0x40, 0x01, 0x65, 0x1a, 0x84, 0x5c, 0xa9, 0xf0]),
                7
            ))
        );
        assert_eq!(
            decode_in_surface_row_lane(&[0xde, 0xee, 0xa1, 0x55, 0x61, 0x88, 0x28], 0, &cache),
            Some((
                f64::from_be_bytes([0x40, 0x10, 0xee, 0xa1, 0x55, 0x61, 0x88, 0x28]),
                7
            ))
        );
        assert_eq!(
            decode_in_surface_row_lane(&[0xdf, 0x19, 0x4c, 0x93, 0x0f, 0x96, 0xe8], 0, &cache),
            Some((
                f64::from_be_bytes([0x40, 0x11, 0x19, 0x4c, 0x93, 0x0f, 0x96, 0xe8]),
                7
            ))
        );
    }

    #[test]
    fn tabulated_cylinder_frame_decodes_positive_4a() {
        let cache = ScalarCache::default();
        assert_eq!(
            decode_tabulated_cylinder_frame_coordinate(
                &[0x4a, 0x13, 0x1f, 0x1c, 0x0b, 0, 0],
                0,
                &cache
            ),
            Some((
                f64::from_be_bytes([0x40, 0x13, 0x1f, 0x1c, 0x0b, 0, 0, 0]),
                7
            ))
        );
    }

    #[test]
    fn tabulated_cylinder_first_frame_uses_the_first_coordinate_sign() {
        let cache = ScalarCache::default();
        let fixed = [0x46, 0x12, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            decode_tabulated_cylinder_first_frame_coordinate(&fixed, 0, &cache),
            Some((f64::from_be_bytes([0xc0, 0x12, 0, 0, 0, 0, 0, 0]), 8))
        );
        assert_eq!(
            decode_tabulated_cylinder_first_frame_coordinate(
                &[0x4a, 0x13, 0, 0, 0, 0, 0],
                0,
                &cache,
            ),
            Some((f64::from_be_bytes([0x40, 0x13, 0, 0, 0, 0, 0, 0]), 7))
        );
    }

    #[test]
    fn section_cache_uses_unique_raw_tokens_in_first_appearance_order() {
        let first = [0x46, 0x08, 0, 0, 0, 0, 0, 0];
        let second = [0x46, 0x10, 0, 0, 0, 0, 0, 0];
        let mut section = vec![0xaa];
        section.extend_from_slice(&first);
        section.extend_from_slice(&first);
        section.extend_from_slice(&second);
        let cache = ScalarCache::from_section(&section);

        assert_eq!(decode_in_lane(&[0x18, 0], 0, &cache), Some((3.0, 2)));
        assert_eq!(decode_in_lane(&[0x18, 1], 0, &cache), Some((4.0, 2)));
    }

    #[test]
    fn lane_zero_does_not_consume_the_following_scalar_opener() {
        let cache = ScalarCache::default();
        assert_eq!(decode_in_lane(&[0x18, 0xe4], 0, &cache), Some((0.0, 1)));
        assert_eq!(decode_in_lane(&[0x18, 0xe4], 1, &cache), Some((1.0, 2)));
        assert_eq!(decode_in_lane(&[0x18, 0x0d], 0, &cache), Some((0.0, 1)));
        assert_eq!(
            decode_in_lane(&[0x18, 0x67, 0, 0, 0, 0, 0, 0], 0, &cache),
            Some((0.0, 1))
        );
        assert_eq!(
            decode_in_lane(&[0x18, 0x67, 0, 0, 0, 0, 0, 0], 1, &cache),
            Some((f64::from_be_bytes([0x3f, 0, 0, 0, 0, 0, 0, 0]), 8))
        );
        assert_eq!(decode_in_row_lane(&[0x18, 0x0e], 0, &cache), Some((0.0, 1)));
        assert_eq!(decode_in_lane(&[0x18, 0x18, 0], 0, &cache), Some((0.0, 1)));
    }

    #[test]
    fn cache_indices_use_only_the_enclosing_lane_opener_set() {
        let mut section = Vec::new();
        for index in 0..=116_u8 {
            let encoded = if index < 0x46 { index } else { index + 1 };
            section.extend_from_slice(&[0x46, 0x08, encoded, 0, 0, 0, 0, 0]);
        }
        let cache = ScalarCache::from_section(&section);

        assert_eq!(
            decode_in_lane(&[0x18, 0x74], 0, &cache),
            Some((f64::from_be_bytes([0x40, 0x08, 0x75, 0, 0, 0, 0, 0]), 2))
        );
        assert_eq!(
            decode_in_lane(&[0x18, 0x0e], 0, &cache),
            Some((f64::from_be_bytes([0x40, 0x08, 0x0e, 0, 0, 0, 0, 0]), 2))
        );
        assert_eq!(decode_in_row_lane(&[0x18, 0x0e], 0, &cache), Some((0.0, 1)));
    }

    #[test]
    fn lane_zero_does_not_consume_the_following_named_record() {
        let cache = ScalarCache::default();
        assert_eq!(decode_in_lane(&[0x18, 0xe0], 0, &cache), Some((0.0, 1)));
    }

    #[test]
    fn paired_negative_lane_uses_matching_positive_cache_tail() {
        let cache = ScalarCache::from_section(&[0x46, 0x08, 1, 2, 3, 4, 5, 6]);
        let expected = f64::from_be_bytes([0xc0, 0x08, 1, 2, 3, 4, 5, 6]);
        assert_eq!(
            decode_in_lane(&[0xa3, 1, 2, 3, 4, 5, 6], 0, &cache),
            Some((expected, 7))
        );
    }

    #[test]
    fn decodes_saved_spline_tangent_dict_forms() {
        let cache = ScalarCache::default();
        let negative = [0xb3, 0, 0, 0, 0, 0, 0];
        let positive = [0x76, 0xb6, 0x7a, 0xe8, 0x58, 0x4c, 0x9a];

        assert_eq!(decode_in_lane(&negative, 0, &cache), Some((-0.5, 7)));
        let (value, end) = decode_in_lane(&positive, 0, &cache).expect("positive tangent");
        assert_eq!(end, 7);
        assert!((value - 3.0_f64.sqrt() / 2.0).abs() < 3e-15);
    }

    #[test]
    fn paired_positive_lane_uses_matching_cache_exponent() {
        let cache = ScalarCache::from_section(&[0x46, 0x13, 1, 2, 3, 4, 5, 6]);
        let expected = f64::from_be_bytes([0x40, 0x13, 1, 2, 3, 4, 5, 6]);
        assert_eq!(
            decode_in_lane(&[0x9e, 1, 2, 3, 4, 5, 6], 0, &cache),
            Some((expected, 7))
        );
        assert_eq!(
            decode_in_lane(&[0x18, 0x9e, 1, 2, 3, 4, 5, 6], 0, &cache),
            Some((0.0, 1))
        );
    }

    #[test]
    fn paired_cache_tail_collision_withholds_value() {
        let cache = ScalarCache::from_section(&[
            0x46, 0x08, 1, 2, 3, 4, 5, 6, 0x46, 0x13, 1, 2, 3, 4, 5, 6,
        ]);
        assert_eq!(decode_in_lane(&[0x9e, 1, 2, 3, 4, 5, 6], 0, &cache), None);
        assert_eq!(decode_in_lane(&[0xa3, 1, 2, 3, 4, 5, 6], 0, &cache), None);
    }

    #[test]
    fn row_lane_uses_seven_byte_0x71_without_consuming_the_next_scalar() {
        let cache = ScalarCache::default();
        let data = [0x71, 0xf0, 0, 0, 0, 0, 0, 0xe4];
        assert_eq!(decode_in_row_lane(&data, 0, &cache), Some((1.0, 7)));
        assert_eq!(decode_in_row_lane(&data, 7, &cache), Some((1.0, 8)));
        assert_eq!(
            decode_in_lane(&data, 0, &cache).map(|(_, end)| end),
            Some(8)
        );
    }

    #[test]
    fn pcurve_lane_falls_back_to_positive_dict_after_generic_forms() {
        let cache = ScalarCache::default();
        let positive_dict = [0x98, 1, 2, 3, 4, 5, 6];
        assert_eq!(
            decode_in_pcurve_lane(&positive_dict, 0, &cache),
            Some((f64::from_be_bytes([0x40, 0x0d, 1, 2, 3, 4, 5, 6]), 7))
        );

        let generic = [0x86, 1, 2, 3, 4, 5, 6];
        assert_eq!(
            decode_in_pcurve_lane(&generic, 0, &cache),
            Some((f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 0]), 7))
        );
    }

    #[test]
    fn surface_row_lane_decodes_negative_a0_dict_form() {
        let cache = ScalarCache::default();
        let data = [0xa0, 0x5c, 0x28, 0xf5, 0xc2, 0x8f, 0x5c, 0xe4];
        assert_eq!(
            decode_in_surface_row_lane(&data, 0, &cache),
            Some((
                f64::from_be_bytes([0xc0, 0x15, 0x5c, 0x28, 0xf5, 0xc2, 0x8f, 0x5c]),
                7
            ))
        );
        assert_eq!(decode_in_surface_row_lane(&data, 7, &cache), Some((1.0, 8)));
    }

    #[test]
    fn surface_row_lane_decodes_negative_a7_dict_form() {
        let cache = ScalarCache::default();
        let data = [0xa7, 0x33, 0x33, 0x33, 0x33, 0x33, 0x80, 0xe4];
        assert_eq!(
            decode_in_surface_row_lane(&data, 0, &cache),
            Some((
                f64::from_be_bytes([0xbf, 0xd3, 0x33, 0x33, 0x33, 0x33, 0x33, 0x80]),
                7
            ))
        );
        assert_eq!(decode_in_surface_row_lane(&data, 7, &cache), Some((1.0, 8)));
    }

    #[test]
    fn named_local_system_decodes_negative_5d_dict_form() {
        let cache = ScalarCache::default();
        let data = [0x5d, 0x3c, 0xfc, 0xe9, 0x9e, 0x37, 0xb2, 0xe4];
        assert_eq!(
            decode_named_local_system_coordinate(&data, 0, 4, &cache),
            Some((
                f64::from_be_bytes([0xbf, 0xd2, 0x3c, 0xfc, 0xe9, 0x9e, 0x37, 0xb2]),
                7
            ))
        );
        assert_eq!(
            decode_named_local_system_coordinate(&data, 7, 5, &cache),
            Some((1.0, 8))
        );
    }

    #[test]
    fn surface_row_lane_decodes_signed_i48_form() {
        let cache = ScalarCache::default();
        assert_eq!(
            decode_in_surface_row_lane(&[0x92, 0xff, 0xff, 0xff, 0xff, 0xff, 0xe8], 0, &cache),
            Some((-24.0, 7))
        );
        assert_eq!(
            decode_in_surface_row_lane(&[0x92, 0x00, 0x00, 0x00, 0x00, 0x01, 0x23], 0, &cache),
            Some((291.0, 7))
        );
        assert_eq!(
            decode_in_surface_row_lane(&[0xda, 0x00, 0x00, 0x00, 0x00, 0x00, 0x15], 0, &cache),
            Some((21.0, 7))
        );
        assert_eq!(
            decode_in_surface_row_lane(&[0x92, 0xff, 0xff], 0, &cache),
            None
        );
    }

    #[test]
    fn surface_row_zero_does_not_consume_a_surface_only_opener() {
        let cache = ScalarCache::default();

        for opener in [0x73, 0x92, 0xa0, 0xbb, 0xda] {
            assert_eq!(
                decode_in_surface_row_lane(&[0x18, opener, 0, 0, 0, 0, 0, 0], 0, &cache),
                Some((0.0, 1))
            );
        }
    }

    #[test]
    fn positive_dict_lane_decodes_cone_half_angles() {
        let forty_five_degrees = [0x74, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x23];
        let eighty_degrees = [0x81, 0x57, 0x18, 0x4a, 0xe7, 0x44, 0x8d];
        let other_angle = [0xb7, 0x5e, 0x8a, 0x1c, 0xf2, 0x17, 0x1e];

        assert_eq!(
            decode_positive_dict(&forty_five_degrees, 0),
            Some((
                f64::from_be_bytes([0x3f, 0xe9, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x23]),
                7
            ))
        );
        assert_eq!(
            decode_positive_dict(&eighty_degrees, 0),
            Some((
                f64::from_be_bytes([0x3f, 0xf6, 0x57, 0x18, 0x4a, 0xe7, 0x44, 0x8d]),
                7
            ))
        );
        assert_eq!(
            decode_positive_dict(&other_angle, 0),
            Some((
                f64::from_be_bytes([0x3f, 0xe4, 0x5e, 0x8a, 0x1c, 0xf2, 0x17, 0x1e]),
                7
            ))
        );
    }

    #[test]
    fn named_positive_dict_lane_precedes_generic_scalar_forms() {
        let cache = ScalarCache::default();
        let subunit = [0x71, 0, 0, 0, 0, 0, 0];
        let named_dict = [0xa3, 0, 0, 0, 0, 0, 0];

        assert_eq!(
            decode_named_positive_dict_scalar(&subunit, 0, &cache),
            Some((f64::from_be_bytes([0x3f, 0xe6, 0, 0, 0, 0, 0, 0]), 7))
        );
        assert_eq!(
            decode_named_positive_dict_scalar(&named_dict, 0, &cache),
            Some((f64::from_be_bytes([0x40, 0x18, 0, 0, 0, 0, 0, 0]), 7))
        );
    }

    #[test]
    fn row_lane_decodes_negative_half_literal() {
        let cache = ScalarCache::default();

        assert_eq!(
            decode_in_row_lane(&[0x0e, 0x18], 0, &cache),
            Some((-0.5, 1))
        );
    }
}
