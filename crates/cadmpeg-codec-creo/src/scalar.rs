// SPDX-License-Identifier: Apache-2.0
//! PSB scalar forms with context-independent IEEE-754 mappings.

use std::collections::{BTreeMap, HashSet};

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
    while let Some(relative) = data
        .get(search..)
        .and_then(|tail| tail.windows(LABEL.len()).position(|window| window == LABEL))
    {
        let offset = search + relative;
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
    paired_byte_1_by_tail: BTreeMap<[u8; 6], u8>,
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
            paired_byte_1_by_tail
                .entry(raw[2..].try_into().expect("six-byte cache tail"))
                .or_insert(raw[1]);
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
    }
}

const LANE_OPENERS: &[u8] = &[
    0x0d, 0x0e, 0x0f, 0x18, 0x29, 0x2a, 0x2d, 0x2e, 0x2f, 0x41, 0x42, 0x43, 0x46, 0x47, 0x48, 0x4b,
    0x5e, 0x66, 0x68, 0x6a, 0x71, 0x74, 0x76, 0x77, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88,
    0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x91, 0x9e, 0xa1, 0xa2, 0xa3, 0xaf, 0xb0, 0xb1,
    0xb7, 0xb9, 0xbf, 0xd3, 0xd7, 0xde, 0xdf, 0xe4, 0xd1, 0xe6, 0xe8, 0xb3,
];

/// Decode one scalar in a row or `f9` scalar lane using its section cache.
pub fn decode_in_lane(data: &[u8], offset: usize, cache: &ScalarCache) -> Option<(f64, usize)> {
    match *data.get(offset)? {
        0x18 => {
            let next = *data.get(offset + 1)?;
            if LANE_OPENERS.contains(&next)
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
    if data.get(offset) == Some(&0x0e) {
        return Some((-0.5, offset + 1));
    }
    if data.get(offset) == Some(&0x71) {
        return ieee7(data, offset, 0x3f);
    }
    decode_in_lane(data, offset, cache)
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

/// Decode the second coordinate of a tabulated-cylinder directrix control point.
///
/// Positive DICT tokens encode the first two IEEE bytes as `0x3f75 + prefix`;
/// their six-byte payload supplies the remaining bytes.
pub fn decode_tabulated_cylinder_second_coordinate(
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
        let raw: [u8; 8] = data.get(offset + 1..offset + 9)?.try_into().ok()?;
        return Some((f64::from_be_bytes(raw), offset + 9));
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
    decode_local_system_slot_prefix(body, cache, LocalSystemVariant::PositionalTorus)
}

/// Decode a positional plane support frame whose origin uses the named
/// local-system sign for compact one-half coordinates.
pub(crate) fn decode_plane_support_local_system_slots(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<[f64; 12]> {
    decode_local_system_slots(body, cache, LocalSystemVariant::PlaneSupport)
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
    let (values, cursor) = decode_local_system_slot_prefix(body, cache, variant)?;
    (cursor == body.len()).then_some(values)
}

fn decode_local_system_slot_prefix(
    body: &[u8],
    cache: &ScalarCache,
    variant: LocalSystemVariant,
) -> Option<([f64; 12], usize)> {
    if body == [0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6] {
        return Some((
            [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            body.len(),
        ));
    }
    let mut values = Vec::with_capacity(12);
    let mut cursor = 0;
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
            values.push(0.0);
            cursor += 1;
            continue;
        }
        let row = decode_in_row_lane(body, cursor, cache);
        let (value, next) = match (variant, values.len()) {
            (LocalSystemVariant::PlaneSupport, 9..=11) if body.get(cursor) == Some(&0x0e) => {
                (0.5, cursor + 1)
            }
            (LocalSystemVariant::PositionalPlane | LocalSystemVariant::PlaneSupport, 9) => {
                row.or_else(|| decode_tabulated_cylinder_first_coordinate(body, cursor, cache))?
            }
            (LocalSystemVariant::PositionalPlane | LocalSystemVariant::PlaneSupport, 10 | 11) => {
                row.or_else(|| decode_tabulated_cylinder_second_coordinate(body, cursor, cache))?
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
    (values.len() == 12).then(|| {
        (
            values
                .try_into()
                .expect("twelve bounded local-system slots"),
            cursor,
        )
    })
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
    let (byte_0, byte_1) = match *data.get(offset)? {
        0x71 => (0x3f, 0xe6),
        0x74 => (0x3f, 0xe9),
        0x76 => (0x3f, 0xeb),
        0x81 => (0x3f, 0xf6),
        0x8b => (0x40, 0x00),
        0x90 => (0x40, 0x05),
        0x91 => (0x40, 0x06),
        0xa1 => (0x40, 0x16),
        0xa2 => (0x40, 0x17),
        0xa3 => (0x40, 0x18),
        0xb7 => (0x3f, 0xe4),
        _ => return None,
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
        assert_eq!(decode_in_row_lane(&[0x18, 0x0e], 0, &cache), Some((0.0, 1)));
        assert_eq!(decode_in_lane(&[0x18, 0x18, 0], 0, &cache), Some((0.0, 1)));
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
    fn paired_cache_tail_keeps_its_first_exponent() {
        let cache = ScalarCache::from_section(&[
            0x46, 0x08, 1, 2, 3, 4, 5, 6, 0x46, 0x13, 1, 2, 3, 4, 5, 6,
        ]);
        let expected = f64::from_be_bytes([0x40, 0x08, 1, 2, 3, 4, 5, 6]);
        assert_eq!(
            decode_in_lane(&[0x9e, 1, 2, 3, 4, 5, 6], 0, &cache),
            Some((expected, 7))
        );
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
    fn row_lane_decodes_negative_half_literal() {
        let cache = ScalarCache::default();

        assert_eq!(
            decode_in_row_lane(&[0x0e, 0x18], 0, &cache),
            Some((-0.5, 1))
        );
    }
}
