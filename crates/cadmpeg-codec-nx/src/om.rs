// SPDX-License-Identifier: Apache-2.0
//! Frame NX object-model entities using external boundary and identity arrays.

use std::collections::BTreeSet;
use std::sync::Arc;

use cadmpeg_core::decode::View;

/// One NX object-model entity with persistent object identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRecord<'a> {
    /// NX object identifier paired with this boundary slot, when the section
    /// carries a fixed-width object-id table.
    pub object_id: Option<u32>,
    /// Absolute byte offset of the paired object-id table word, when present.
    pub object_id_offset: Option<usize>,
    /// Absolute byte offset of the entity payload.
    pub offset: usize,
    /// Exactly bounded serialized entity payload.
    pub bytes: &'a [u8],
}

/// One length-framed NX object-model class definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDefinition<'a> {
    /// Absolute byte offset of the definition's length byte.
    pub offset: usize,
    /// Registered `UGS::` class name.
    pub name: &'a str,
    /// Declaration code following the name.
    pub trailing_code: u8,
    /// Bytes between this declaration core and the next class declaration.
    pub registry_suffix: &'a [u8],
}

/// One member declaration in an NX OM field registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDefinition<'a> {
    /// Offset of the declaration length byte.
    pub offset: usize,
    /// Registered `m_` member name.
    pub name: &'a str,
    /// Declaration code immediately following the name.
    pub trailing_code: u8,
    /// Bytes between this declaration core and the next member declaration.
    pub registry_suffix: &'a [u8],
}

/// One self-framed printable string value in an NX OM entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringValue<'a> {
    /// Absolute byte offset of the `66 32 03` marker.
    pub offset: usize,
    /// Printable value bytes.
    pub value: &'a str,
}

/// One canonical UUID in the compact NX OM string frame `03 26, text, 00`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UuidStringValue<'a> {
    /// Absolute byte offset of the `03 26` marker.
    pub offset: usize,
    /// Canonical lowercase UUID text.
    pub value: &'a str,
}

/// One self-framed printable string in a surface-referenced payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfacePayloadString<'a> {
    /// Payload-relative offset of the `66 1b 03` marker.
    pub offset: usize,
    /// Exact non-empty string value.
    pub value: &'a str,
}

/// Self-framed NX product/version marker in an OM store root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreVersion<'a> {
    /// Absolute offset of the `04 01` marker.
    pub offset: usize,
    /// Exact printable product/version text, including the `NX ` prefix.
    pub value: &'a str,
}

/// Header of an internally pointed size-framed OM record area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordAreaHeader<'a> {
    /// Absolute offset of the first control word.
    pub offset: usize,
    /// Three little-endian control words preceding the product record.
    pub control_words: [u32; 3],
    /// Product/version record following the control words.
    pub product: StoreVersion<'a>,
}

/// Tagged NX OM cross-record reference family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    /// `e0` marker followed by a 32-bit big-endian persistent handle.
    PersistentHandle,
    /// Four-byte word whose high nibble is `c` and low 28 bits are the value.
    Tagged28,
    /// `90` marker followed by a 16-bit big-endian record ordinal.
    RecordOrdinal16,
}

/// One value in an NX OM compact-index lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactIndex {
    /// `ff` null/sentinel entry.
    Null,
    /// Decoded non-null index.
    Value(u32),
}

/// Decode a complete NX OM compact-index lane.
///
/// `00..7f` are direct values, `80..fe` introduce one low byte, and `ff` is
/// null. A dangling two-byte prefix rejects the whole lane.
pub fn compact_indices(bytes: &[u8]) -> Option<Vec<CompactIndex>> {
    let mut values = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let (value, width) = compact_index(bytes.get(at..)?)?;
        at += width;
        values.push(value);
    }
    Some(values)
}

#[derive(Debug, Clone, Copy)]
struct CompactToken {
    value: CompactIndex,
    offset: usize,
    width: usize,
}

fn compact_index(bytes: &[u8]) -> Option<(CompactIndex, usize)> {
    let prefix = *bytes.first()?;
    if prefix == 0xff {
        Some((CompactIndex::Null, 1))
    } else if prefix >= 0x80 {
        let low = u32::from(*bytes.get(1)?);
        Some((CompactIndex::Value(u32::from(prefix - 0x80) * 256 + low), 2))
    } else {
        Some((CompactIndex::Value(u32::from(prefix)), 1))
    }
}

fn compact_token(bytes: &[u8], offset: usize) -> Option<CompactToken> {
    let (value, width) = compact_index(bytes.get(offset..)?)?;
    Some(CompactToken {
        value,
        offset,
        width,
    })
}

fn compact_value_token(bytes: &[u8], offset: usize) -> Option<CompactToken> {
    let token = compact_token(bytes, offset)?;
    matches!(token.value, CompactIndex::Value(_)).then_some(token)
}

fn raw_compact_token(bytes: &[u8], token: CompactToken) -> Vec<u8> {
    bytes[token.offset..token.offset + token.width].to_vec()
}

/// One counted compact-index lane ending in the exact `01 11` marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetStoreCountedIndexLane {
    /// Byte offset of the opening `01` marker.
    pub offset: usize,
    /// Serialized count. One slot is the anchor and one is the terminator.
    pub declared_count: u8,
    /// Non-null compact index immediately following the count.
    pub anchor: u32,
    /// Exact serialized anchor token.
    pub raw_anchor: Vec<u8>,
    /// Byte offset of the anchor compact index.
    pub anchor_offset: usize,
    /// Ordered non-null compact indices preceding the terminator.
    pub members: Vec<(u32, usize)>,
    /// Exact serialized member tokens in lane order.
    pub raw_members: Vec<Vec<u8>>,
}

/// Fixed-width nullable block-index lane terminated by the literal `ABR` tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetStoreAbrReferenceLane {
    /// Byte offset of the opening `11` marker.
    pub offset: usize,
    /// Sixteen ordered nullable compact indices and their byte offsets.
    pub slots: Vec<(Option<u32>, usize)>,
    /// Exact compact-index tokens in slot order.
    pub raw_slots: Vec<Vec<u8>>,
}

/// One self-framed index row in contiguous offset-store column storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetStoreIndexRow {
    /// Byte offset of the opening `2d 02 0b` discriminator.
    pub offset: usize,
    /// First non-null compact index.
    pub first_index: u32,
    /// Exact serialized first-index token.
    pub raw_first_index: Vec<u8>,
    /// Byte offset of the first compact index.
    pub first_index_offset: usize,
    /// Serialized row flag.
    pub flag: u8,
    /// Four ordered non-null compact indices after the row flag.
    pub indices: [(u32, usize); 4],
    /// Exact serialized four-index tokens in row order.
    pub raw_indices: [Vec<u8>; 4],
}

/// One self-framed linked index row in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetStoreLinkedIndexRow {
    /// Byte offset of the opening `02 0b` discriminator.
    pub offset: usize,
    /// Unresolved leading compact index and its byte offset.
    pub first_index: (u32, usize),
    /// Exact serialized leading-index token.
    pub raw_first_index: Vec<u8>,
    /// Serialized `16`, `17`, or `18` row discriminator.
    pub discriminator: u8,
    /// Compact target index and its byte offset.
    pub target_index: (u32, usize),
    /// Exact serialized target-index token.
    pub raw_target_index: Vec<u8>,
    /// Three ordered non-null compact indices after `ff ff 90 fe`.
    pub indices: [(u32, usize); 3],
    /// Exact serialized post-marker tokens in row order.
    pub raw_indices: [Vec<u8>; 3],
    /// Serialized `03` or `07` row flag.
    pub flag: u8,
    /// Serialized `04` or `07` row mode.
    pub mode: u8,
}

/// Canonical NX color-index token immediately preceding a display row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedRowColorIndex {
    /// One-based part palette index.
    pub color_index: u16,
    /// Exact serialized index token.
    pub raw_color_index: Vec<u8>,
    /// Color token's byte offset.
    pub offset: usize,
}

/// Decode the color-index prefix of an `RMFastLoad` linked row.
pub fn linked_row_color_index(
    bytes: &[u8],
    row: &OffsetStoreLinkedIndexRow,
) -> Option<LinkedRowColorIndex> {
    row_color_index(bytes, row.offset)
}

/// Decode the color-index prefix of an `RMFastLoad` target-index row.
pub fn target_row_color_index(
    bytes: &[u8],
    row: &OffsetStoreTargetIndexRow,
) -> Option<LinkedRowColorIndex> {
    row_color_index(bytes, row.offset)
}

fn row_color_index(bytes: &[u8], row_offset: usize) -> Option<LinkedRowColorIndex> {
    const PRECEDING_SUFFIX: [u8; 5] = [0x01, 0xc0, 0x44, 0x04, 0x00];
    let direct_offset = row_offset.checked_sub(1)?;
    let direct = *bytes.get(direct_offset)?;
    if (1..=127).contains(&direct)
        && bytes.get(direct_offset.checked_sub(PRECEDING_SUFFIX.len())?..direct_offset)
            == Some(&PRECEDING_SUFFIX)
    {
        return Some(LinkedRowColorIndex {
            color_index: u16::from(direct),
            raw_color_index: vec![direct],
            offset: direct_offset,
        });
    }
    let extended_offset = row_offset.checked_sub(2)?;
    let token: [u8; 2] = bytes.get(extended_offset..row_offset)?.try_into().ok()?;
    (token[0] == 0x80
        && (128..=216).contains(&token[1])
        && bytes.get(extended_offset.checked_sub(PRECEDING_SUFFIX.len())?..extended_offset)
            == Some(&PRECEDING_SUFFIX))
    .then(|| LinkedRowColorIndex {
        color_index: u16::from(token[1]),
        raw_color_index: token.to_vec(),
        offset: extended_offset,
    })
}

#[cfg(test)]
mod linked_row_color_index_tests {
    use super::*;

    #[test]
    fn requires_the_complete_preceding_suffix() {
        let row_bytes = [
            0x02, 0x0b, 7, 0x93, 0x8c, 0x16, 2, 0xff, 0xff, 0x90, 0xfe, 3, 4, 5, 0, 0x47, 3, 4, 1,
            0xc0, 0x44, 4, 0,
        ];
        let mut bytes = [1, 0xc0, 0x44, 4, 0, 0x80, 201].to_vec();
        bytes.extend(row_bytes);
        let rows = offset_store_linked_index_rows(&bytes);
        let color = linked_row_color_index(&bytes, &rows[0]).expect("complete prefix");
        assert_eq!(color.color_index, 201);
        assert_eq!(color.raw_color_index, [0x80, 201]);

        bytes[1] = 0;
        assert_eq!(linked_row_color_index(&bytes, &rows[0]), None);
    }

    #[test]
    fn accepts_the_same_prefix_for_a_target_index_row() {
        let row_bytes = [
            0x02, 0x01, 0x01, 0x01, 0x16, 2, 0xff, 0xff, 0x90, 0xfe, 3, 4, 5, 0, 0x47, 3, 4, 1,
            0xc0, 0x44, 4, 0,
        ];
        let mut bytes = [1, 0xc0, 0x44, 4, 0, 0x80, 201].to_vec();
        bytes.extend(row_bytes);
        let rows = offset_store_target_index_rows(&bytes);
        let color = target_row_color_index(&bytes, &rows[0]).expect("complete prefix");
        assert_eq!(color.color_index, 201);
        assert_eq!(color.raw_color_index, [0x80, 201]);
    }
}

/// One self-framed target-index row in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetStoreTargetIndexRow {
    /// Byte offset of the opening `02 01 01 01 16` discriminator.
    pub offset: usize,
    /// Compact target index and its byte offset.
    pub target_index: (u32, usize),
    /// Exact serialized target-index token.
    pub raw_target_index: Vec<u8>,
    /// Three ordered non-null compact indices after `ff ff 90 fe`.
    pub indices: [(u32, usize); 3],
    /// Exact serialized post-marker tokens in row order.
    pub raw_indices: [Vec<u8>; 3],
    /// Serialized `04` or `07` row mode.
    pub mode: u8,
}

/// One RGB definition from an NX part color table.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorTableDefinition<'a> {
    /// One-based NX color index.
    pub color_index: u16,
    /// Color name paired by table order.
    pub name: &'a str,
    /// Normalized red, green, and blue components.
    pub rgb: [f32; 3],
    /// Exact serialized index token after the `05` marker.
    pub raw_color_index: Vec<u8>,
    /// Exact serialized component atoms.
    pub raw_components: [Vec<u8>; 3],
    /// Byte offset of the opening `05` marker.
    pub offset: usize,
    /// Byte offsets of the three component atoms.
    pub component_offsets: [usize; 3],
}

/// Complete 216-entry NX part color table.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorTable<'a> {
    /// Byte offset of the counted name roster.
    pub offset: usize,
    /// Name associated with the separately encoded background color.
    pub background_name: &'a str,
    /// Normalized background RGB components.
    pub background_rgb: [f32; 3],
    /// Exact serialized background component atoms.
    pub raw_background_components: [Vec<u8>; 3],
    /// Byte offsets of the three background component atoms.
    pub background_component_offsets: [usize; 3],
    /// Ordered definitions for color indices 1 through 216.
    pub definitions: Vec<ColorTableDefinition<'a>>,
}

fn color_component_layout(bytes: &[u8]) -> Option<(f32, usize)> {
    match bytes.first().copied()? {
        0x00 => Some((0.0, 1)),
        0x01 => Some((1.0, 1)),
        marker @ (0x20..=0x3f | 0xa0..=0xbf) => {
            let raw: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
            let value = shifted_ieee_f64(&raw)? / 4.0;
            (value.is_finite() && (0.0..=1.0).contains(&value)).then(|| {
                debug_assert_eq!(raw[0], marker);
                (value as f32, 8)
            })
        }
        marker @ (0x40..=0x5f | 0xc0..=0xdf) => {
            let raw: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
            let mut decoded = raw;
            decoded[0] = decoded[0].checked_sub(0x10)?;
            let value = f32::from_be_bytes(decoded) / 4.0;
            (value.is_finite() && (0.0..=1.0).contains(&value)).then(|| {
                debug_assert_eq!(raw[0], marker);
                (value, 4)
            })
        }
        _ => None,
    }
}

fn color_component(bytes: &[u8]) -> Option<(f32, Vec<u8>, usize)> {
    let (value, width) = color_component_layout(bytes)?;
    Some((value, bytes.get(..width)?.to_vec(), width))
}

fn color_name_frame(bytes: &[u8], offset: usize) -> Option<(&str, usize)> {
    let byte_len = usize::from(bytes.get(offset).copied()?);
    if byte_len < 2 {
        return None;
    }
    let end = offset.checked_add(byte_len)?;
    let text = bytes.get(offset + 1..end)?.strip_suffix(&[0])?;
    if text.is_empty()
        || !text
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return None;
    }
    Some((std::str::from_utf8(text).ok()?, byte_len))
}

const COLOR_TABLE_NAME_HEADER: [u8; 4] = [0x02, 0x80, 0xd9, 0x01];
const COLOR_TABLE_DEFINITION_PREAMBLE: [u8; 21] = [
    0x02, 0x14, 0xff, 0x06, 0x00, 0xf0, 0x02, 0x80, 0x9d, 0x80, 0xc7, 0x00, 0xc0, 0x13, 0x0a, 0xc6,
    0x01, 0x80, 0xd9, 0x80, 0xc8,
];

// Validate a complete palette through borrowed names, component widths, and
// index tokens. The owning color vectors are materialized only after this
// self-framed candidate has passed every check.
fn color_table_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start..start + COLOR_TABLE_NAME_HEADER.len()) != Some(&COLOR_TABLE_NAME_HEADER) {
        return None;
    }
    let mut at = start + COLOR_TABLE_NAME_HEADER.len();
    for ordinal in 0..=216 {
        let (name, width) = color_name_frame(bytes, at)?;
        if ordinal == 0 && name != "Background" {
            return None;
        }
        at += width;
    }
    if bytes.get(at..at + COLOR_TABLE_DEFINITION_PREAMBLE.len())
        != Some(&COLOR_TABLE_DEFINITION_PREAMBLE)
    {
        return None;
    }
    at += COLOR_TABLE_DEFINITION_PREAMBLE.len();
    for _ in 0..3 {
        at += color_component_layout(bytes.get(at..)?)?.1;
    }
    for color_index in 1u16..=216 {
        if bytes.get(at) != Some(&0x05) {
            return None;
        }
        at += 1;
        let token = if color_index < 128 {
            [color_index as u8, 0]
        } else {
            [0x80, (color_index - 1) as u8]
        };
        let token_width = if color_index < 128 { 1 } else { 2 };
        if bytes.get(at..at + token_width) != Some(&token[..token_width]) {
            return None;
        }
        at += token_width;
        if bytes.get(at..at + 3) != Some(&[0x01, 0x80, 0xc8]) {
            return None;
        }
        at += 3;
        for _ in 0..3 {
            at += color_component_layout(bytes.get(at..)?)?.1;
        }
    }
    Some(at)
}

fn color_table_at(bytes: &[u8], start: usize) -> Option<ColorTable<'_>> {
    let mut at = start + COLOR_TABLE_NAME_HEADER.len();
    let mut names = Vec::with_capacity(217);
    for _ in 0..217 {
        let (name, width) = color_name_frame(bytes, at)?;
        names.push(name);
        at += width;
    }
    at += COLOR_TABLE_DEFINITION_PREAMBLE.len();

    let mut background_rgb = Vec::with_capacity(3);
    let mut raw_background_components = Vec::with_capacity(3);
    let mut background_component_offsets = Vec::with_capacity(3);
    for _ in 0..3 {
        background_component_offsets.push(at);
        let (value, raw, width) = color_component(bytes.get(at..)?)?;
        background_rgb.push(value);
        raw_background_components.push(raw);
        at += width;
    }

    let mut definitions = Vec::with_capacity(216);
    for color_index in 1u16..=216 {
        let offset = at;
        at += 1;
        let raw_color_index = if color_index < 128 {
            vec![color_index as u8]
        } else {
            vec![0x80, (color_index - 1) as u8]
        };
        at += raw_color_index.len();
        at += 3;

        let mut rgb = Vec::with_capacity(3);
        let mut raw_components = Vec::with_capacity(3);
        let mut component_offsets = Vec::with_capacity(3);
        for _ in 0..3 {
            component_offsets.push(at);
            let (value, raw, width) = color_component(bytes.get(at..)?)?;
            rgb.push(value);
            raw_components.push(raw);
            at += width;
        }
        definitions.push(ColorTableDefinition {
            color_index,
            name: names[usize::from(color_index)],
            rgb: rgb.try_into().ok()?,
            raw_color_index,
            raw_components: raw_components.try_into().ok()?,
            offset,
            component_offsets: component_offsets.try_into().ok()?,
        });
    }
    Some(ColorTable {
        offset: start,
        background_name: names[0],
        background_rgb: background_rgb.try_into().ok()?,
        raw_background_components: raw_background_components.try_into().ok()?,
        background_component_offsets: background_component_offsets.try_into().ok()?,
        definitions,
    })
}

/// Decode every complete NX part color table in a bounded byte region.
pub fn color_tables(bytes: &[u8]) -> Vec<ColorTable<'_>> {
    let mut tables = Vec::new();
    let mut start = 0;
    while start + COLOR_TABLE_NAME_HEADER.len() <= bytes.len() {
        if bytes.get(start..start + COLOR_TABLE_NAME_HEADER.len()) != Some(&COLOR_TABLE_NAME_HEADER)
        {
            start += 1;
            continue;
        }
        let Some(end) = color_table_end(bytes, start) else {
            start += 1;
            continue;
        };
        if let Some(table) = color_table_at(bytes, start) {
            tables.push(table);
        }
        start = end;
    }
    tables
}

/// Decode complete self-framed index rows from contiguous column storage.
pub fn offset_store_index_rows(bytes: &[u8]) -> Vec<OffsetStoreIndexRow> {
    const PREFIX: [u8; 3] = [0x2d, 0x02, 0x0b];
    const MIDDLE: [u8; 2] = [0x93, 0x8a];
    const SUFFIX: [u8; 9] = [0x00, 0x47, 0x04, 0x04, 0x01, 0xc0, 0x44, 0x04, 0x00];
    let mut rows = Vec::new();
    let mut start = 0;
    while start + PREFIX.len() <= bytes.len() {
        if bytes.get(start..start + PREFIX.len()) != Some(&PREFIX) {
            start += 1;
            continue;
        }
        let first_index_offset = start + PREFIX.len();
        let Some(first_token) = compact_value_token(bytes, first_index_offset) else {
            start += 1;
            continue;
        };
        let CompactIndex::Value(first_index) = first_token.value else {
            unreachable!("compact value token is non-null")
        };
        let marker = first_token.offset + first_token.width;
        if bytes.get(marker..marker + 2) != Some(&MIDDLE[..2]) {
            start += 1;
            continue;
        }
        let Some(flag @ (0x03 | 0x07)) = bytes.get(marker + 2).copied() else {
            start += 1;
            continue;
        };
        let mut at = marker + 3;
        let mut index_tokens = [CompactToken {
            value: CompactIndex::Null,
            offset: 0,
            width: 0,
        }; 4];
        let mut complete = true;
        for token in &mut index_tokens {
            let Some(index_token) = compact_value_token(bytes, at) else {
                complete = false;
                break;
            };
            at += index_token.width;
            *token = index_token;
        }
        if !complete {
            start += 1;
            continue;
        }
        let Some(end) = at.checked_add(SUFFIX.len()) else {
            start += 1;
            continue;
        };
        if bytes.get(at..end) != Some(&SUFFIX) {
            start += 1;
            continue;
        }
        rows.push(OffsetStoreIndexRow {
            offset: start,
            first_index,
            raw_first_index: raw_compact_token(bytes, first_token),
            first_index_offset,
            flag,
            indices: index_tokens.map(|token| {
                let CompactIndex::Value(index) = token.value else {
                    unreachable!("compact value token is non-null")
                };
                (index, token.offset)
            }),
            raw_indices: index_tokens.map(|token| raw_compact_token(bytes, token)),
        });
        start = end;
    }
    rows
}

/// Decode complete linked index rows from contiguous column storage.
pub fn offset_store_linked_index_rows(bytes: &[u8]) -> Vec<OffsetStoreLinkedIndexRow> {
    const MIDDLE: [u8; 4] = [0xff, 0xff, 0x90, 0xfe];
    const SUFFIX: [u8; 5] = [0x01, 0xc0, 0x44, 0x04, 0x00];
    let mut rows = Vec::new();
    let mut start = 0;
    while start + 2 <= bytes.len() {
        if bytes.get(start..start + 2) != Some(&[0x02, 0x0b]) {
            start += 1;
            continue;
        }
        let first_offset = start + 2;
        let Some(first_token) = compact_value_token(bytes, first_offset) else {
            start += 1;
            continue;
        };
        let CompactIndex::Value(first_index) = first_token.value else {
            unreachable!("compact value token is non-null")
        };
        let marker = first_token.offset + first_token.width;
        if bytes.get(marker..marker + 2) != Some(&[0x93, 0x8c]) {
            start += 1;
            continue;
        }
        let Some(discriminator @ (0x16..=0x18)) = bytes.get(marker + 2).copied() else {
            start += 1;
            continue;
        };
        let target_offset = marker + 3;
        let Some(target_token) = compact_value_token(bytes, target_offset) else {
            start += 1;
            continue;
        };
        let CompactIndex::Value(target_index) = target_token.value else {
            unreachable!("compact value token is non-null")
        };
        let mut at = target_token.offset + target_token.width;
        if bytes.get(at..at + MIDDLE.len()) != Some(&MIDDLE) {
            start += 1;
            continue;
        }
        at += MIDDLE.len();
        let mut index_tokens = [CompactToken {
            value: CompactIndex::Null,
            offset: 0,
            width: 0,
        }; 3];
        let mut complete = true;
        for token in &mut index_tokens {
            let Some(index_token) = compact_value_token(bytes, at) else {
                complete = false;
                break;
            };
            at += index_token.width;
            *token = index_token;
        }
        if !complete {
            start += 1;
            continue;
        }
        if bytes.get(at..at + 2) != Some(&[0x00, 0x47]) {
            start += 1;
            continue;
        }
        let Some(flag @ (0x03 | 0x07)) = bytes.get(at + 2).copied() else {
            start += 1;
            continue;
        };
        let Some(mode @ (0x04 | 0x07)) = bytes.get(at + 3).copied() else {
            start += 1;
            continue;
        };
        let Some(end) = at.checked_add(4 + SUFFIX.len()) else {
            start += 1;
            continue;
        };
        if bytes.get(at + 4..end) != Some(&SUFFIX) {
            start += 1;
            continue;
        }
        rows.push(OffsetStoreLinkedIndexRow {
            offset: start,
            first_index: (first_index, first_offset),
            raw_first_index: raw_compact_token(bytes, first_token),
            discriminator,
            target_index: (target_index, target_offset),
            raw_target_index: raw_compact_token(bytes, target_token),
            indices: index_tokens.map(|token| {
                let CompactIndex::Value(index) = token.value else {
                    unreachable!("compact value token is non-null")
                };
                (index, token.offset)
            }),
            raw_indices: index_tokens.map(|token| raw_compact_token(bytes, token)),
            flag,
            mode,
        });
        start = end;
    }
    rows
}

/// Decode complete target-index rows from contiguous column storage.
pub fn offset_store_target_index_rows(bytes: &[u8]) -> Vec<OffsetStoreTargetIndexRow> {
    const PREFIX: [u8; 5] = [0x02, 0x01, 0x01, 0x01, 0x16];
    const MIDDLE: [u8; 4] = [0xff, 0xff, 0x90, 0xfe];
    const SUFFIX: [u8; 5] = [0x01, 0xc0, 0x44, 0x04, 0x00];
    let mut rows = Vec::new();
    let mut start = 0;
    while start + PREFIX.len() <= bytes.len() {
        if bytes.get(start..start + PREFIX.len()) != Some(&PREFIX) {
            start += 1;
            continue;
        }
        let target_offset = start + PREFIX.len();
        let Some(target_token) = compact_value_token(bytes, target_offset) else {
            start += 1;
            continue;
        };
        let CompactIndex::Value(target_index) = target_token.value else {
            unreachable!("compact value token is non-null")
        };
        let mut at = target_token.offset + target_token.width;
        if bytes.get(at..at + MIDDLE.len()) != Some(&MIDDLE) {
            start += 1;
            continue;
        }
        at += MIDDLE.len();
        let mut index_tokens = [CompactToken {
            value: CompactIndex::Null,
            offset: 0,
            width: 0,
        }; 3];
        let mut complete = true;
        for token in &mut index_tokens {
            let Some(index_token) = compact_value_token(bytes, at) else {
                complete = false;
                break;
            };
            at += index_token.width;
            *token = index_token;
        }
        if !complete {
            start += 1;
            continue;
        }
        if bytes.get(at..at + 3) != Some(&[0x00, 0x47, 0x03]) {
            start += 1;
            continue;
        }
        let Some(mode @ (0x04 | 0x07)) = bytes.get(at + 3).copied() else {
            start += 1;
            continue;
        };
        let Some(end) = at.checked_add(4 + SUFFIX.len()) else {
            start += 1;
            continue;
        };
        if bytes.get(at + 4..end) != Some(&SUFFIX) {
            start += 1;
            continue;
        }
        rows.push(OffsetStoreTargetIndexRow {
            offset: start,
            target_index: (target_index, target_offset),
            raw_target_index: raw_compact_token(bytes, target_token),
            indices: index_tokens.map(|token| {
                let CompactIndex::Value(index) = token.value else {
                    unreachable!("compact value token is non-null")
                };
                (index, token.offset)
            }),
            raw_indices: index_tokens.map(|token| raw_compact_token(bytes, token)),
            mode,
        });
        start = end;
    }
    rows
}

/// Decode fixed-width `ABR` block-reference lanes from contiguous column storage.
pub fn offset_store_abr_reference_lanes(bytes: &[u8]) -> Vec<OffsetStoreAbrReferenceLane> {
    const SLOT_COUNT: usize = 16;
    const TERMINATOR: [u8; 7] = [0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03];
    let mut lanes = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        if bytes[start] != 0x11 {
            start += 1;
            continue;
        }
        let mut at = start + 1;
        let mut tokens = [CompactToken {
            value: CompactIndex::Null,
            offset: 0,
            width: 0,
        }; SLOT_COUNT];
        let mut complete = true;
        for token in &mut tokens {
            let Some(slot_token) = compact_token(bytes, at) else {
                complete = false;
                break;
            };
            at += slot_token.width;
            *token = slot_token;
        }
        let Some(end) = at.checked_add(TERMINATOR.len()) else {
            start += 1;
            continue;
        };
        if complete && bytes.get(at..end) == Some(&TERMINATOR) {
            lanes.push(OffsetStoreAbrReferenceLane {
                offset: start,
                slots: tokens
                    .map(|token| {
                        (
                            match token.value {
                                CompactIndex::Null => None,
                                CompactIndex::Value(value) => Some(value),
                            },
                            token.offset,
                        )
                    })
                    .into_iter()
                    .collect(),
                raw_slots: tokens
                    .map(|token| raw_compact_token(bytes, token))
                    .into_iter()
                    .collect(),
            });
            start = end;
        } else {
            start += 1;
        }
    }
    lanes
}

/// Decode complete counted compact-index lanes from one bounded store block.
///
/// A lane is `01, count:u8, anchor, member[count-2], 01 11`, with
/// `count >= 3`. Compact indices use the ordinary direct/extended encoding;
/// null indices reject the candidate atomically.
pub fn offset_store_counted_index_lanes(bytes: &[u8]) -> Vec<OffsetStoreCountedIndexLane> {
    let mut lanes = Vec::new();
    let mut start = 0;
    while start + 4 <= bytes.len() {
        if bytes[start] != 0x01 {
            start += 1;
            continue;
        }
        let declared_count = bytes[start + 1];
        if declared_count < 3 {
            start += 1;
            continue;
        }
        let Some(anchor_token) = compact_value_token(bytes, start + 2) else {
            start += 1;
            continue;
        };
        let mut at = anchor_token.offset + anchor_token.width;
        let mut complete = true;
        for _ in 0..usize::from(declared_count) - 2 {
            let Some(member_token) = compact_value_token(bytes, at) else {
                complete = false;
                break;
            };
            at += member_token.width;
        }
        let Some(end) = at.checked_add(2) else {
            start += 1;
            continue;
        };
        if complete && bytes.get(at..end) == Some(&[0x01, 0x11]) {
            let CompactIndex::Value(anchor) = anchor_token.value else {
                unreachable!("compact value token is non-null")
            };
            let mut member_at = anchor_token.offset + anchor_token.width;
            let mut members = Vec::with_capacity(usize::from(declared_count) - 2);
            let mut raw_members = Vec::with_capacity(usize::from(declared_count) - 2);
            for _ in 0..usize::from(declared_count) - 2 {
                let member_token = compact_value_token(bytes, member_at)
                    .expect("validated counted lane member remains readable");
                let CompactIndex::Value(value) = member_token.value else {
                    unreachable!("compact value token is non-null")
                };
                members.push((value, member_token.offset));
                raw_members.push(raw_compact_token(bytes, member_token));
                member_at += member_token.width;
            }
            lanes.push(OffsetStoreCountedIndexLane {
                offset: start,
                declared_count,
                anchor,
                raw_anchor: raw_compact_token(bytes, anchor_token),
                anchor_offset: anchor_token.offset,
                members,
                raw_members,
            });
            start = end;
        } else {
            start += 1;
        }
    }
    lanes
}

/// One exact shifted-IEEE scalar field in a reconstructed construction payload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConstructionPayloadScalarField {
    /// Payload-relative offset of the `50 59 66` marker.
    pub offset: usize,
    /// Serialized field discriminator following the marker.
    pub field_code: u8,
    /// Finite decoded binary64 value.
    pub value: f64,
    /// Exact shifted-binary64 encoding.
    pub raw_value: [u8; 8],
}

const SHIFTED_BINARY64_SCALAR_FRAME_LEN: usize = 13;

/// Decode exact `50 59 66, field_code, 00, shifted-f64` construction fields.
pub fn construction_payload_scalar_fields(bytes: &[u8]) -> Vec<ConstructionPayloadScalarField> {
    let mut fields = Vec::new();
    for start in 0..bytes.len().saturating_sub(12) {
        if bytes.get(start..start + 3) != Some(b"PYf")
            || bytes.get(start + 4) != Some(&0x00)
            || !matches!(bytes.get(start + 5), Some(0x20..=0x3f | 0xa0..=0xbf))
        {
            continue;
        }
        let Some(raw_value) = bytes
            .get(start + 5..start + SHIFTED_BINARY64_SCALAR_FRAME_LEN)
            .and_then(|value| <[u8; 8]>::try_from(value).ok())
        else {
            continue;
        };
        let Some(value) = shifted_ieee_f64(&raw_value) else {
            continue;
        };
        fields.push(ConstructionPayloadScalarField {
            offset: start,
            field_code: bytes[start + 3],
            value,
            raw_value,
        });
    }
    fields
}

/// One compact-code string field in a reconstructed construction payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstructionPayloadNamedField<'a> {
    /// Payload-relative offset of the `66` marker.
    pub offset: usize,
    /// Decoded non-null compact type code following the marker.
    pub type_code: Option<u32>,
    /// Exact compact type-code token, absent for the payload-leading form.
    pub raw_type_code: Option<Vec<u8>>,
    /// Payload-relative compact type-code offset, when present.
    pub type_code_offset: Option<usize>,
    /// Whether the field uses the type-free payload-leading form.
    pub payload_leading: bool,
    /// Exact nonempty printable ASCII value.
    pub value: &'a str,
}

/// Exact type-free named point record spanning consecutive store blocks.
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetStoreNamedPoint {
    /// Exact `Point<positive decimal>` name.
    pub name: String,
    /// Two framed scalar values in block order.
    pub values: [f64; 2],
    /// Exact shifted-binary64 encodings in scalar order.
    pub raw_values: [[u8; 8]; 2],
    /// Scalar marker offsets in the concatenated payload.
    pub value_offsets: [usize; 2],
    /// Minimal number of consecutive blocks containing both scalar frames.
    pub block_count: usize,
}

/// Decode a named two-scalar point from a streaming block sequence.
pub(crate) fn offset_store_named_point<'a>(
    blocks: impl IntoIterator<Item = &'a [u8]>,
) -> Option<OffsetStoreNamedPoint> {
    let mut bytes = Vec::new();
    let mut candidate = None;
    for (block_ordinal, block) in blocks.into_iter().enumerate() {
        // A later type-free name starts the next bounded data-block object.
        if !bytes.is_empty()
            && construction_payload_named_fields(block)
                .first()
                .is_some_and(|name| name.payload_leading)
        {
            return candidate;
        }
        bytes.extend_from_slice(block);
        let names = construction_payload_named_fields(&bytes);
        let name = names.first()?;
        if !name.payload_leading || parse_positive_decimal_suffix(name.value, "Point").is_none() {
            return None;
        }
        let next_name = names
            .iter()
            .find(|next| !next.payload_leading && next.offset > name.offset);
        let interval_end = next_name.map_or(bytes.len(), |next| next.offset);
        let scalars = construction_payload_scalar_fields(&bytes)
            .into_iter()
            .filter(|scalar| {
                scalar.offset > name.offset
                    && scalar
                        .offset
                        .checked_add(SHIFTED_BINARY64_SCALAR_FRAME_LEN)
                        .is_some_and(|end| end <= interval_end)
            })
            .collect::<Vec<_>>();
        match scalars.as_slice() {
            [] | [_] => {}
            [first_scalar, second_scalar] => {
                candidate.get_or_insert_with(|| OffsetStoreNamedPoint {
                    name: name.value.to_string(),
                    values: [first_scalar.value, second_scalar.value],
                    raw_values: [first_scalar.raw_value, second_scalar.raw_value],
                    value_offsets: [first_scalar.offset, second_scalar.offset],
                    block_count: block_ordinal + 1,
                });
            }
            _ => return None,
        }
        if next_name.is_some() {
            return candidate;
        }
    }
    candidate
}

fn parse_positive_decimal_suffix(value: &str, prefix: &str) -> Option<u32> {
    let suffix = value.strip_prefix(prefix)?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let ordinal = suffix.parse::<u32>().ok()?;
    (ordinal != 0).then_some(ordinal)
}

/// Decode exact `66, compact_type, 03, declared_len, text, 00` fields.
pub fn construction_payload_named_fields(bytes: &[u8]) -> Vec<ConstructionPayloadNamedField<'_>> {
    let mut fields = Vec::new();
    if bytes.first() == Some(&0x03) {
        if let Some(value) = construction_payload_name_text(bytes, 1) {
            fields.push(ConstructionPayloadNamedField {
                offset: 0,
                type_code: None,
                raw_type_code: None,
                type_code_offset: None,
                payload_leading: true,
                value,
            });
        }
    }
    for start in 0..bytes.len().saturating_sub(5) {
        if bytes[start] != 0x66 {
            continue;
        }
        let Some((CompactIndex::Value(type_code), type_width)) =
            bytes.get(start + 1..).and_then(compact_index)
        else {
            continue;
        };
        let marker = start + 1 + type_width;
        if bytes.get(marker) != Some(&0x03) {
            continue;
        }
        let Some(value) = construction_payload_name_text(bytes, marker + 1) else {
            continue;
        };
        fields.push(ConstructionPayloadNamedField {
            offset: start,
            type_code: Some(type_code),
            raw_type_code: Some(bytes[start + 1..marker].to_vec()),
            type_code_offset: Some(start + 1),
            payload_leading: false,
            value,
        });
    }
    fields
}

fn construction_payload_name_text(bytes: &[u8], length_offset: usize) -> Option<&str> {
    let text_len = usize::from(bytes.get(length_offset).copied()?.checked_sub(2)?);
    let text_start = length_offset.checked_add(1)?;
    let text_end = text_start.checked_add(text_len)?;
    let text = bytes.get(text_start..text_end)?;
    if text.is_empty()
        || !text.iter().all(u8::is_ascii_graphic)
        || bytes.get(text_end) != Some(&0x00)
    {
        return None;
    }
    std::str::from_utf8(text).ok()
}

/// One tagged reference occurrence in an externally bounded OM record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceValue {
    /// Absolute byte offset of the reference marker.
    pub offset: usize,
    /// Reference family.
    pub kind: ReferenceKind,
    /// Unsigned reference value without its marker/tag bits.
    pub value: u32,
}

/// Unit declared by an NX numeric-expression serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpressionUnit {
    /// Canonical model length in millimeters.
    Millimeter,
    /// Angular value in degrees as serialized by NX.
    Degree,
}

/// One numeric expression decoded from an exactly bounded OM entity.
#[derive(Debug, Clone, PartialEq)]
pub struct NumericExpression<'a> {
    /// Persistent identity of the containing OM entity, when indexed.
    pub object_id: Option<u32>,
    /// Absolute byte offset of the expression text.
    pub offset: usize,
    /// NX parameter name.
    pub name: &'a str,
    /// Decimal identifier following the leading `p`, when present.
    pub parameter_index: Option<u32>,
    /// Name component following the parameter index and underscore.
    pub qualifier: Option<&'a str>,
    /// Declared native unit.
    pub unit: ExpressionUnit,
    /// Exact expression text following the serialized name separator.
    pub expression: &'a str,
    /// Finite value when the expression is context-free arithmetic.
    pub value: Option<f64>,
}

/// One validated external entity-index/object-id-table pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedSection<'a> {
    /// Self-anchored base used by every entity-index offset.
    pub base: usize,
    /// Absolute offset of the entity-index array.
    pub entity_index_offset: usize,
    /// Absolute offset of the object-id table or offset-only identity metadata.
    pub object_id_table_offset: usize,
    /// Length-framed class definitions preceding the entity index.
    pub types: Arc<[TypeDefinition<'a>]>,
    /// Length-framed member definitions preceding the entity index.
    pub fields: Arc<[FieldDefinition<'a>]>,
    /// Store-level control block bounded by slot zero in an offset-only index.
    pub control: Option<EntityRecord<'a>>,
    /// Contiguous column-storage region after the control block.
    ///
    /// Present only for an offset-only store. Physical block boundaries do not
    /// delimit logical field lanes within this region.
    pub column_storage: Option<&'a [u8]>,
    /// Entity records following the reserved zero-offset slot.
    pub records: Arc<[EntityRecord<'a>]>,
}

/// Byte ranges needed to materialize one indexed section without rescanning
/// the containing payload.
#[derive(Debug, Clone, Copy)]
struct IndexedByteRange {
    start: usize,
    end: usize,
}

/// One cached declaration range in an indexed section.
#[derive(Debug, Clone, Copy)]
struct IndexedDefinitionLayout {
    offset: usize,
    name_len: usize,
    trailing_code: u8,
    registry_suffix: Option<IndexedByteRange>,
}

/// One cached entity-record range in an indexed section.
#[derive(Debug, Clone, Copy)]
struct IndexedRecordLayout {
    object_id: Option<u32>,
    object_id_offset: Option<usize>,
    bytes: IndexedByteRange,
}

/// Cached indexed-section layout owned by a parsed container.
#[derive(Debug, Clone)]
pub(crate) struct IndexedSectionLayout {
    base: usize,
    entity_index_offset: usize,
    pub(crate) object_id_table_offset: usize,
    types: Vec<IndexedDefinitionLayout>,
    fields: Vec<IndexedDefinitionLayout>,
    control: Option<IndexedRecordLayout>,
    column_storage: Option<IndexedByteRange>,
    records: Vec<IndexedRecordLayout>,
}

impl IndexedSectionLayout {
    fn from_section(section: &IndexedSection<'_>) -> Self {
        let types = section
            .types
            .iter()
            .enumerate()
            .map(|(index, definition)| IndexedDefinitionLayout {
                offset: definition.offset,
                name_len: definition.name.len(),
                trailing_code: definition.trailing_code,
                registry_suffix: (index + 1 < section.types.len()).then(|| IndexedByteRange {
                    start: definition.offset + definition.name.len() + 2,
                    end: section.types[index + 1].offset,
                }),
            })
            .collect();
        let fields = section
            .fields
            .iter()
            .enumerate()
            .map(|(index, definition)| IndexedDefinitionLayout {
                offset: definition.offset,
                name_len: definition.name.len(),
                trailing_code: definition.trailing_code,
                registry_suffix: (index + 1 < section.fields.len()).then(|| IndexedByteRange {
                    start: definition.offset + definition.name.len() + 2,
                    end: section.fields[index + 1].offset,
                }),
            })
            .collect();
        let record_layout = |record: &EntityRecord<'_>| IndexedRecordLayout {
            object_id: record.object_id,
            object_id_offset: record.object_id_offset,
            bytes: IndexedByteRange {
                start: record.offset,
                end: record.offset + record.bytes.len(),
            },
        };
        let control = section.control.as_ref().map(record_layout);
        let column_storage = section.column_storage.map(|storage| {
            let start = section
                .records
                .first()
                .expect("offset-only indexed section has records")
                .offset;
            IndexedByteRange {
                start,
                end: start + storage.len(),
            }
        });
        Self {
            base: section.base,
            entity_index_offset: section.entity_index_offset,
            object_id_table_offset: section.object_id_table_offset,
            types,
            fields,
            control,
            column_storage,
            records: section.records.iter().map(record_layout).collect(),
        }
    }

    pub(crate) fn materialize<'a>(&self, bytes: &'a [u8]) -> IndexedSection<'a> {
        let materialize_record = |layout: &IndexedRecordLayout| EntityRecord {
            object_id: layout.object_id,
            object_id_offset: layout.object_id_offset,
            offset: layout.bytes.start,
            bytes: bytes
                .get(layout.bytes.start..layout.bytes.end)
                .expect("cached indexed record remains in source"),
        };
        IndexedSection {
            base: self.base,
            entity_index_offset: self.entity_index_offset,
            object_id_table_offset: self.object_id_table_offset,
            types: self
                .types
                .iter()
                .map(|layout| materialize_type_definition(bytes, layout))
                .collect::<Vec<_>>()
                .into(),
            fields: self
                .fields
                .iter()
                .map(|layout| materialize_field_definition(bytes, layout))
                .collect::<Vec<_>>()
                .into(),
            control: self.control.as_ref().map(materialize_record),
            column_storage: self.column_storage.map(|range| {
                bytes
                    .get(range.start..range.end)
                    .expect("cached indexed column storage remains in source")
            }),
            records: self
                .records
                .iter()
                .map(materialize_record)
                .collect::<Vec<_>>()
                .into(),
        }
    }
}

fn materialize_registry_suffix(bytes: &[u8], range: Option<IndexedByteRange>) -> &[u8] {
    range.map_or(&bytes[..0], |range| {
        bytes
            .get(range.start..range.end)
            .expect("cached indexed registry suffix remains in source")
    })
}

fn materialize_type_definition<'a>(
    bytes: &'a [u8],
    layout: &IndexedDefinitionLayout,
) -> TypeDefinition<'a> {
    let name_start = layout.offset + 1;
    let name_end = name_start + layout.name_len;
    let name = std::str::from_utf8(
        bytes
            .get(name_start..name_end)
            .expect("cached indexed declaration name remains in source"),
    )
    .expect("cached indexed declaration name remains UTF-8");
    TypeDefinition {
        offset: layout.offset,
        name,
        trailing_code: layout.trailing_code,
        registry_suffix: materialize_registry_suffix(bytes, layout.registry_suffix),
    }
}

fn materialize_field_definition<'a>(
    bytes: &'a [u8],
    layout: &IndexedDefinitionLayout,
) -> FieldDefinition<'a> {
    let name_start = layout.offset + 1;
    let name_end = name_start + layout.name_len;
    let name = std::str::from_utf8(
        bytes
            .get(name_start..name_end)
            .expect("cached indexed declaration name remains in source"),
    )
    .expect("cached indexed declaration name remains UTF-8");
    FieldDefinition {
        offset: layout.offset,
        name,
        trailing_code: layout.trailing_code,
        registry_suffix: materialize_registry_suffix(bytes, layout.registry_suffix),
    }
}

/// One size-framed NX object-model section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section<'a> {
    /// Offset of the `ff ff ff ff` section signature.
    pub offset: usize,
    /// Complete section length including its 16-byte header.
    pub byte_len: usize,
    /// Class declarations in the section's contiguous type registry.
    pub types: Arc<[TypeDefinition<'a>]>,
    /// Member declarations in the section's field registry.
    pub fields: Arc<[FieldDefinition<'a>]>,
    /// Absolute offset of the section's internally pointed record area.
    pub record_area_offset: Option<usize>,
    /// Exact record-area bytes, including its 12-byte control prefix.
    pub record_area: Option<&'a [u8]>,
    /// Operation labels decoded while the section's record area is framed.
    cached_operation_labels: Arc<[OperationLabel<'a>]>,
}

/// Cached byte layout for one size-framed object-model section.
///
/// The layout contains offsets and validated declaration metadata only. It
/// does not borrow the container image, so a container that owns its input can
/// reuse the layout without reparsing the section on every extractor call.
#[derive(Debug, Clone)]
pub(crate) struct SectionLayout {
    offset: usize,
    byte_len: usize,
    types: Vec<IndexedDefinitionLayout>,
    fields: Vec<IndexedDefinitionLayout>,
    record_area_offset: Option<usize>,
    record_area: Option<IndexedByteRange>,
    operation_labels: Vec<OperationLabelLayout>,
}

impl SectionLayout {
    pub(crate) fn from_section(section: &Section<'_>) -> Self {
        let record_area = section.record_area.map(|bytes| {
            let start = section
                .record_area_offset
                .expect("record area has an absolute offset");
            IndexedByteRange {
                start,
                end: start + bytes.len(),
            }
        });
        Self {
            offset: section.offset,
            byte_len: section.byte_len,
            types: type_definition_layouts(&section.types),
            fields: field_definition_layouts(&section.fields),
            record_area_offset: section.record_area_offset,
            record_area,
            operation_labels: operation_label_layouts(&section.cached_operation_labels),
        }
    }

    pub(crate) fn materialize<'a>(&self, bytes: &'a [u8]) -> Section<'a> {
        let record_area = self.record_area.map(|range| {
            bytes
                .get(range.start..range.end)
                .expect("cached section record area remains in source")
        });
        let cached_operation_labels = match (record_area, self.record_area_offset) {
            (Some(record_area), Some(record_area_offset)) => materialize_operation_labels(
                record_area,
                record_area_offset,
                &self.operation_labels,
            )
            .into(),
            _ => Arc::from([]),
        };
        Section {
            offset: self.offset,
            byte_len: self.byte_len,
            types: self
                .types
                .iter()
                .map(|layout| materialize_type_definition(bytes, layout))
                .collect::<Vec<_>>()
                .into(),
            fields: self
                .fields
                .iter()
                .map(|layout| materialize_field_definition(bytes, layout))
                .collect::<Vec<_>>()
                .into(),
            record_area_offset: self.record_area_offset,
            record_area,
            cached_operation_labels,
        }
    }
}

fn type_definition_layouts(definitions: &[TypeDefinition<'_>]) -> Vec<IndexedDefinitionLayout> {
    definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| IndexedDefinitionLayout {
            offset: definition.offset,
            name_len: definition.name.len(),
            trailing_code: definition.trailing_code,
            registry_suffix: (index + 1 < definitions.len()).then(|| IndexedByteRange {
                start: definition.offset + definition.name.len() + 2,
                end: definitions[index + 1].offset,
            }),
        })
        .collect()
}

fn field_definition_layouts(definitions: &[FieldDefinition<'_>]) -> Vec<IndexedDefinitionLayout> {
    definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| IndexedDefinitionLayout {
            offset: definition.offset,
            name_len: definition.name.len(),
            trailing_code: definition.trailing_code,
            registry_suffix: (index + 1 < definitions.len()).then(|| IndexedByteRange {
                start: definition.offset + definition.name.len() + 2,
                end: definitions[index + 1].offset,
            }),
        })
        .collect()
}

/// A feature operation name in a size-framed feature-history record area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationLabel<'a> {
    /// Absolute offset of the fixed operation-header marker.
    pub header_offset: usize,
    /// Absolute offset of the `03` label tag within the containing entry.
    pub offset: usize,
    /// Printable operation name without its terminating NUL.
    pub value: &'a str,
    /// Four object-index slots in header order; `None` is the `ff` sentinel.
    pub object_indices: [Option<u32>; 4],
    /// Absolute byte offset of each object-index token in header order.
    pub object_index_offsets: [usize; 4],
}

/// Cached byte layout for one validated operation label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationLabelLayout {
    header_offset: usize,
    offset: usize,
    value_len: usize,
    object_indices: [Option<u32>; 4],
    object_index_offsets: [usize; 4],
}

/// Convert borrowed operation labels into cached byte layouts.
pub(crate) fn operation_label_layouts(labels: &[OperationLabel<'_>]) -> Vec<OperationLabelLayout> {
    labels
        .iter()
        .map(|label| OperationLabelLayout {
            header_offset: label.header_offset,
            offset: label.offset,
            value_len: label.value.len(),
            object_indices: label.object_indices,
            object_index_offsets: label.object_index_offsets,
        })
        .collect()
}

fn materialize_operation_labels<'a>(
    bytes: &'a [u8],
    base_offset: usize,
    layouts: &[OperationLabelLayout],
) -> Vec<OperationLabel<'a>> {
    layouts
        .iter()
        .map(|layout| {
            let value_start = layout
                .offset
                .checked_sub(base_offset)
                .and_then(|offset| offset.checked_add(2))
                .expect("cached operation label offset remains in record area");
            let value_end = value_start
                .checked_add(layout.value_len)
                .expect("cached operation label length remains in record area");
            let value = std::str::from_utf8(
                bytes
                    .get(value_start..value_end)
                    .expect("cached operation label value remains in record area"),
            )
            .expect("cached operation label remains UTF-8");
            OperationLabel {
                header_offset: layout.header_offset,
                offset: layout.offset,
                value,
                object_indices: layout.object_indices,
                object_index_offsets: layout.object_index_offsets,
            }
        })
        .collect()
}

/// One operation record bounded by consecutive validated operation headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationRecord<'a> {
    /// Absolute offset of the fixed operation-header marker.
    pub offset: usize,
    /// Complete record bytes through the next operation header or section end.
    pub bytes: &'a [u8],
    /// Absolute offset of the first byte after the operation-label terminator.
    pub payload_offset: usize,
    /// Post-label serialized operation payload.
    pub payload: &'a [u8],
    /// Label decoded from this record's header.
    pub label: OperationLabel<'a>,
}

/// Exactly framed common record in one bounded operation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationCommonFrame {
    /// Three compact prefix indices.
    pub indices: [u32; 3],
    /// Exact compact-index tokens in order.
    pub raw_indices: [Vec<u8>; 3],
    /// Fixed marker selecting the index layout.
    pub marker: [u8; 3],
    /// Exact eight-byte state lane following the fixed state marker.
    pub state: [u8; 8],
    /// Absolute offset of the first compact index token.
    pub offset: usize,
    /// Absolute offsets of the compact prefix-index tokens.
    pub index_offsets: [usize; 3],
    /// Absolute offset of the first state byte.
    pub state_offset: usize,
    /// Duplicated frame-local ordinal.
    pub local_ordinal: u32,
    /// Exact canonical token repeated for the local ordinal.
    pub raw_local_ordinal: Vec<u8>,
    /// Nullable object reference following the duplicated ordinal.
    pub object_index: Option<u32>,
    /// Exact canonical nullable object-reference token.
    pub raw_object_index: Vec<u8>,
    /// Absolute offset of the first local-ordinal token.
    pub local_ordinal_offset: usize,
    /// Absolute offset of the object-reference token.
    pub object_index_offset: usize,
    /// Exclusive absolute end offset after the frame terminator.
    pub end_offset: usize,
}

/// Canonical terminal common-frame suffix in one bounded operation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationTerminalFrame {
    /// Absolute offset of the exact common frame immediately preceding this suffix.
    pub immediate_common_frame_offset: Option<usize>,
    /// Duplicated frame-local ordinal.
    pub local_ordinal: u32,
    /// Exact canonical token repeated for the local ordinal.
    pub raw_local_ordinal: Vec<u8>,
    /// Nullable object reference following the duplicated ordinal.
    pub object_index: Option<u32>,
    /// Exact canonical nullable object-reference token.
    pub raw_object_index: Vec<u8>,
    /// Absolute offset of the first local-ordinal token.
    pub offset: usize,
    /// Absolute offset of the object-reference token.
    pub object_index_offset: usize,
}

/// One length-framed UTF-8 string in a bounded operation payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationPayloadString<'a> {
    /// Absolute offset of the `04` marker.
    pub offset: usize,
    /// Exact non-empty string value.
    pub value: &'a str,
}

/// One canonical variable-width object index in an operation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadObjectReference {
    /// Absolute offset of the width marker.
    pub offset: usize,
    /// Decoded object index.
    pub object_index: u32,
    /// Exact serialized variable-width object-index token.
    pub raw_object_index: Vec<u8>,
}

/// Counted reference field in one bounded sketch-operation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SketchPayloadReferenceField {
    /// Effective count encoded by the nonempty flag and optional count byte.
    pub declared_count: u8,
    /// Ordered pre-separator references followed by the terminal reference.
    pub references: Vec<PayloadObjectReference>,
}

/// Exact construction-reference field in a projected-curve payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedCurvePayloadReferenceField {
    /// Ordered non-repeated construction references.
    pub references: Vec<PayloadObjectReference>,
}

/// Byte layout selected by a pattern construction-reference field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternPayloadReferenceLayout {
    /// The `61`/`ff 00 ff 01`/`ff 62` graph framing.
    CanonicalGraph,
    /// The `3b`/`ff 00 01`/`ff 3c` graph framing.
    CompactGraph,
    /// The one-reference `Geometry Instance` framing.
    GeometryInstance,
}

/// Exact non-null construction references in a pattern payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternPayloadReferenceField {
    /// Exact byte layout that framed the field.
    pub layout: PatternPayloadReferenceLayout,
    /// Non-null references in serialized slot order.
    pub references: Vec<PayloadObjectReference>,
}

/// Exact counted non-null reference lane in a `Pattern Feature` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternPayloadCountedReferenceLane {
    /// Absolute offset of the opening `01, count` field.
    pub offset: usize,
    /// Serialized count including the implicit owner slot.
    pub declared_count: u8,
    /// Ordered non-null object references after the count.
    pub references: Vec<PayloadObjectReference>,
}

/// Exact two-group reference graph in an `FSET` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsetPayloadReferenceGraph {
    /// Printable selector preceding the reference groups.
    pub selector: String,
    /// Two references before the group separator.
    pub first: [PayloadObjectReference; 2],
    /// Three references after the group separator.
    pub second: [PayloadObjectReference; 3],
    /// Absolute offset of the graph prefix.
    pub offset: usize,
}

/// One nullable object-index slot in a counted `DELETE` payload field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePayloadReferenceSlot {
    /// Decoded object index, or `None` for the exact `ff` null token.
    pub object_index: Option<u32>,
    /// Exact serialized object-index token.
    pub raw_object_index: Vec<u8>,
    /// Absolute offset of the token.
    pub offset: usize,
}

/// Exact five-slot nullable reference field in a `DELETE` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePayloadReferenceField {
    /// Leading operation-local control byte.
    pub control: u8,
    /// Five slots in serialized order.
    pub references: [DeletePayloadReferenceSlot; 5],
    /// Absolute offset of the leading control byte.
    pub offset: usize,
}

/// Scalar width selected by one pattern-transform row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternTransformEncoding {
    /// Single-byte exact one used by a wide row terminal value.
    ExactOne,
    /// Four-byte shifted IEEE-754 binary32 atom.
    Binary32,
    /// Eight-byte shifted IEEE-754 binary64 atom.
    Binary64,
}

/// Byte layout selected by a counted pattern-transform lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternTransformLayout {
    /// One shifted scalar per row and terminal mode `01`.
    ScalarRows,
    /// Four shifted binary64 values and one terminal value per row, with terminal mode `02`.
    WideRows,
}

/// One exact counted transform lane in a pattern operation payload.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternPayloadTransformLane {
    /// Absolute offset of the opening `01, count` field.
    pub offset: usize,
    /// Schema index framing every row in the lane.
    pub row_schema_index: u8,
    /// Row byte layout selected by the terminal mode.
    pub layout: PatternTransformLayout,
    /// Count including the implicit seed row.
    pub declared_count: u8,
    /// Scalar encodings in row-major order.
    pub encodings: Vec<PatternTransformEncoding>,
    /// Finite scalars in row-major order.
    pub values: Vec<f64>,
    /// Absolute offsets of the scalar encodings.
    pub value_offsets: Vec<usize>,
    /// Exact scalar bytes in row order.
    pub raw_values: Vec<Vec<u8>>,
    /// Ordered non-null compact selectors.
    pub selectors: Vec<u32>,
    /// Exact compact-index selector tokens in row order.
    pub raw_selectors: Vec<Vec<u8>>,
    /// Absolute offsets of the compact-index selector tokens.
    pub selector_offsets: Vec<usize>,
}

/// Exact counted instance-output lane in a multi-instance operation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiInstanceOutputPayloadLane {
    /// Absolute offset of the opening `25 01, count` field.
    pub offset: usize,
    /// Count including the implicit seed row.
    pub declared_count: u8,
    /// Ordered non-null compact selectors.
    pub selectors: Vec<u32>,
    /// Exact compact-index selector tokens in row order.
    pub raw_selectors: Vec<Vec<u8>>,
    /// Absolute offsets of the compact-index selector tokens.
    pub selector_offsets: Vec<usize>,
    /// Ordered serialized instance ordinals.
    pub ordinals: Vec<u8>,
    /// Ordered serialized row indices.
    pub row_indices: Vec<u8>,
    /// Count including the implicit seed instance.
    pub instance_count: u8,
    /// Ordered non-null trailing object references.
    pub trailing_references: Vec<PayloadObjectReference>,
}

/// Exact counted selector lane in an identical-instance output payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdenticalInstanceOutputPayloadLane {
    /// Absolute offset of the leading schema index.
    pub offset: usize,
    /// Schema index preceding the count field.
    pub leading_schema_index: u8,
    /// Schema index framing the serialized count.
    pub count_schema_index: u8,
    /// Three consecutive schema indices framing every selector row.
    pub row_schema_indices: [u8; 3],
    /// Count including the implicit owner row.
    pub declared_count: u8,
    /// Ordered non-null compact selectors.
    pub selectors: Vec<u32>,
    /// Exact compact-index selector tokens in row order.
    pub raw_selectors: Vec<Vec<u8>>,
    /// Absolute offsets of the compact-index selector tokens.
    pub selector_offsets: Vec<usize>,
}

/// Exact construction header in a point-feature payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointFeaturePayloadHeader {
    /// Construction object referenced by the header.
    pub reference: PayloadObjectReference,
    /// Serialized header mode.
    pub mode: u8,
}

/// Exact six-scalar lane selected by a point-feature construction header.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointFeatureScalarLane {
    /// Six finite scalar values in byte order.
    pub values: [f64; 6],
    /// Exact shifted-binary64 encodings in byte order.
    pub raw_values: [[u8; 8]; 6],
    /// Scalar marker offsets across the concatenated preceding and target blocks.
    pub value_offsets: [usize; 6],
}

/// Exact construction-reference graph in a draft-feature payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftFeaturePayloadReferenceField {
    /// Four construction references in serialized order.
    pub references: [PayloadObjectReference; 4],
}

/// Counted compact-index lane preceding a draft-feature construction graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftFeatureLeadingIndexLane {
    /// Serialized count including the omitted lane owner.
    pub declared_count: u8,
    /// Non-null compact indices in serialized order with absolute token offsets.
    pub indices: Vec<(u32, usize)>,
    /// Exact compact-index tokens in serialized order.
    pub raw_indices: Vec<Vec<u8>>,
}

/// End-anchored compact-index lane in a draft-feature payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftFeatureTerminalLane {
    /// Two non-null compact indices in serialized order.
    pub indices: [u32; 2],
    /// Exact two-byte compact-index tokens in serialized order.
    pub raw_indices: [[u8; 2]; 2],
    /// Absolute offsets of the compact-index tokens.
    pub index_offsets: [usize; 2],
    /// Three uninterpreted bytes preceding the terminal zero.
    pub tail: [u8; 3],
    /// Absolute offset of the first compact-index token.
    pub offset: usize,
}

/// Exact common construction references in a surface-feature payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceFeaturePayloadReferenceField {
    /// Eleven header references followed by the trailing three references.
    pub references: [PayloadObjectReference; 14],
}

/// One counted construction branch in a surface-feature payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceFeaturePayloadBranch {
    /// Absolute offset of the branch mode byte.
    pub offset: usize,
    /// Serialized `16` or `40` branch mode.
    pub mode: u8,
    /// Count including the terminal reference.
    pub declared_count: u8,
    /// Whether the count is repeated before the zero lane.
    pub witnessed: bool,
    /// Ordered nonterminal references.
    pub members: Vec<PayloadObjectReference>,
    /// Terminal reference.
    pub terminal: PayloadObjectReference,
    /// Opaque bytes separating the terminal from the next branch or terminator.
    pub suffix: Vec<u8>,
}

/// Exact counted branch group in a surface-feature payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceFeaturePayloadBranches {
    /// Serialized construction family byte following `a0 5a`.
    pub family: u8,
    /// Serialized group header code.
    pub header_code: u8,
    /// Ordered branches matching the declared group count.
    pub branches: Vec<SurfaceFeaturePayloadBranch>,
}

/// Ordered extrusion profile-reference field and its redundant witness state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtrudeProfileReferenceField {
    /// Ordered profile object indices.
    pub references: Vec<PayloadObjectReference>,
    /// Whether a second field repeats the encoded reference list exactly once.
    pub witnessed: bool,
}

/// Fixed ordered construction-reference lane in a datum coordinate-system payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumCsysReferenceField {
    /// Payload control byte preceding the fixed header suffix.
    pub control: u8,
    /// Eight canonical payload object references in serialized order.
    pub references: [PayloadObjectReference; 8],
}

/// Common typed header preceding tag-specific datum-plane construction data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatumPlanePayloadHeader {
    /// Payload control byte.
    pub control: u8,
    /// Declared construction count.
    pub declared_count: u8,
    /// Tag selecting the following construction branch.
    pub branch_tag: u8,
}

/// Count-two datum-plane branch shared by tags `1b` and `23`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumPlaneSingleReferenceBranch {
    /// Non-null compact descriptor index.
    pub descriptor_index: u32,
    /// Exact serialized compact descriptor-index token.
    pub raw_descriptor_index: Vec<u8>,
    /// Absolute offset of the compact descriptor index.
    pub descriptor_offset: usize,
    /// Canonical payload object index.
    pub object_index: u32,
    /// Exact serialized payload object-index token.
    pub raw_object_index: Vec<u8>,
    /// Absolute offset of the canonical width marker.
    pub object_offset: usize,
}

/// Two canonical references carried by a tag-`29` datum-plane branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumPlaneDoubleReferenceBranch {
    /// Canonical payload object indices in branch order.
    pub references: [PayloadObjectReference; 2],
}

/// Complete terminal compact-index lane in a reconstructed datum-plane payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumPlaneObjectIndexLane {
    /// Payload-relative offset of the opening `01` marker.
    pub offset: usize,
    /// Serialized count.
    pub declared_count: u8,
    /// Ordered non-null compact indices and their payload-relative offsets.
    pub indices: Vec<(u32, usize)>,
    /// Exact compact-index tokens in serialized order.
    pub raw_indices: Vec<Vec<u8>>,
    /// Big-endian trailer word after the zero separator.
    pub trailer: u32,
}

/// Exact scalar pair following a datum-plane object-record discriminator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DatumPlaneObjectScalarPair {
    /// Payload-relative offset of the discriminator.
    pub offset: usize,
    /// Ordered finite shifted-IEEE binary64 values.
    pub values: [f64; 2],
    /// Exact shifted-binary64 encodings in value order.
    pub raw_values: [[u8; 8]; 2],
    /// Payload-relative offsets of the two scalar encodings.
    pub value_offsets: [usize; 2],
}

/// Exact 40-byte datum-plane descriptor block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumPlaneDescriptorBlock {
    /// Lowercase hexadecimal identity preceding the delimiter.
    pub identity: String,
    /// Exact descriptor suffix beginning with `?`.
    pub suffix: Vec<u8>,
    /// Non-null compact schema index following `?A`.
    pub schema_index: u32,
    /// Nonempty printable terminal label.
    pub label: String,
}

/// Exact scalar pair following an object or sketch discriminator.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectPayloadScalarPair {
    /// Payload-relative offset of the discriminator.
    pub offset: usize,
    /// Ordered finite shifted-IEEE values.
    pub values: [f64; 2],
    /// Exact shifted-binary64 encodings in value order.
    pub raw_values: [[u8; 8]; 2],
    /// Payload-relative offsets of the two scalar encodings.
    pub value_offsets: [usize; 2],
    /// Exact discriminator selecting the scalar-pair branch.
    pub discriminator: Vec<u8>,
}

/// Exact pair of signed Q1.55 atoms in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchPayloadFixedPair {
    /// Payload-relative offset of the discriminator.
    pub offset: usize,
    /// Ordered dimensionless Q1.55 values.
    pub values: [f64; 2],
    /// Payload-relative offsets of the two `30` atom markers.
    pub value_offsets: [usize; 2],
    /// Exact seven-byte two's-complement payloads.
    pub raw_values: [[u8; 7]; 2],
    /// Exact discriminator and branch prefix selecting the pair layout.
    pub discriminator: Vec<u8>,
}

/// Exact mixed Q1.55 and shifted-binary32 pair in a sketch payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchPayloadMixedPair {
    /// Payload-relative offset of the discriminator.
    pub offset: usize,
    /// Dimensionless signed Q1.55 value.
    pub fixed_value: f64,
    /// Finite shifted-IEEE binary32 value widened exactly to binary64.
    pub binary32_value: f64,
    /// Exact seven-byte two's-complement Q1.55 payload.
    pub fixed_raw_value: [u8; 7],
    /// Exact four-byte shifted-binary32 encoding.
    pub binary32_raw_value: [u8; 4],
    /// Payload-relative offsets of the two atom markers.
    pub value_offsets: [usize; 2],
    /// Exact discriminator selecting the mixed pair layout.
    pub discriminator: Vec<u8>,
}

/// Exact pair of signed Q1.55 atoms following a datum-CSYS branch discriminator.
#[derive(Debug, Clone, PartialEq)]
pub struct DatumCsysPayloadFixedPair {
    /// Payload-relative offset of the discriminator.
    pub offset: usize,
    /// Ordered dimensionless Q1.55 values.
    pub values: [f64; 2],
    /// Payload-relative offsets of the two `30` atom markers.
    pub value_offsets: [usize; 2],
    /// Exact seven-byte two's-complement payloads.
    pub raw_values: [[u8; 7]; 2],
    /// Exact discriminator selecting the pair branch.
    pub discriminator: Vec<u8>,
}

/// One bounded datum-CSYS descriptor block with a unique hexadecimal identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumCsysDescriptorBlock {
    /// Exact bytes preceding the identity.
    pub prefix: Vec<u8>,
    /// Lowercase 30–32 digit hexadecimal identity.
    pub identity: String,
    /// Exact bytes following the identity.
    pub suffix: Vec<u8>,
    /// Block-relative identity offset.
    pub identity_offset: usize,
}

/// Complete identity frame in a reconstructed draft construction payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftConstructionIdentityFrame {
    /// Payload-relative offset of the opening `41` marker.
    pub offset: usize,
    /// Exact bytes from the opening marker through the identity introducer.
    pub prefix: Vec<u8>,
    /// Typed frame form selected by the exact prefix.
    pub form: DraftConstructionIdentityFrameForm,
    /// Nonempty lowercase hexadecimal identity.
    pub identity: String,
    /// Payload-relative identity offset.
    pub identity_offset: usize,
}

/// Typed prefix form of a draft construction identity frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftConstructionIdentityFrameForm {
    /// Two compact indices and a `02` or `03` branch.
    IndexedBranch {
        /// Non-null first compact index.
        first_index: u32,
        /// Nullable second compact index.
        second_index: Option<u32>,
        /// Exact `02` or `03` branch byte.
        branch: u8,
    },
    /// One nullable compact index followed by `ff 02 01`.
    Tagged {
        /// Nullable compact index.
        index: Option<u32>,
    },
}

/// Complete signed Q1.55 lane in a reconstructed draft graph payload.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftConstructionFixedLane {
    /// Payload-relative offset of the fixed discriminator.
    pub offset: usize,
    /// Ordered dimensionless Q1.55 values.
    pub values: Vec<f64>,
    /// Exact atom markers in value order.
    pub markers: Vec<u8>,
    /// Exact seven-byte two's-complement payloads.
    pub raw_values: Vec<[u8; 7]>,
    /// Payload-relative offsets of the atom markers.
    pub value_offsets: Vec<usize>,
}

/// Complete shifted-binary32 lane in a reconstructed draft graph payload.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftConstructionBinary32Lane {
    /// Payload-relative offset of the discriminator.
    pub offset: usize,
    /// Exact discriminator selecting the lane form.
    pub discriminator: [u8; 18],
    /// Exact `03` or `04` branch byte.
    pub branch: u8,
    /// Ordered finite shifted-IEEE binary32 values.
    pub values: Vec<f64>,
    /// Exact four-byte shifted encodings.
    pub raw_values: Vec<[u8; 4]>,
    /// Payload-relative offsets of the scalar encodings.
    pub value_offsets: Vec<usize>,
}

/// Compact object frame in a bounded offset-store block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataBlockObjectFrame {
    /// Serialized persistent object ID.
    pub object_id: u32,
    /// Exact serialized compact object-index token.
    pub raw_object_id: Vec<u8>,
    /// Block-relative offset of the compact index.
    pub offset: usize,
}

/// Fixed scalar header in one bounded extrusion payload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtrudePayloadHeader {
    /// Absolute offset of the first shifted-IEEE scalar.
    pub offset: usize,
    /// Ordered finite scalar values.
    pub scalars: [f64; 2],
    /// Exact shifted-binary64 encodings in scalar order.
    pub raw_scalars: [[u8; 8]; 2],
}

/// Exact terminal discriminator lane at the end of a bounded operation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationTerminalDiscriminator {
    /// Payload-relative offset of the fixed footer prelude.
    pub offset: usize,
    /// Two compact type indices following `01 01 02`.
    pub type_indices: [u32; 2],
    /// Exact compact-index tokens for the two type indices.
    pub raw_type_indices: [Vec<u8>; 2],
    /// Absolute offsets of the two type-index tokens.
    pub type_index_offsets: [usize; 2],
    /// Four serialized one-byte flags.
    pub flags: [u8; 4],
    /// Compact values between `29 29` and the terminal zero.
    pub trailing_indices: Vec<u32>,
    /// Exact compact-index tokens in the trailing lane.
    pub raw_trailing_indices: Vec<Vec<u8>>,
    /// Absolute offsets of the trailing compact-index tokens.
    pub trailing_index_offsets: Vec<usize>,
}

/// Nonempty scalar lane serialized twice in a simple-hole payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SimpleHoleRepeatedScalarLane {
    /// Ordered finite shifted-binary64 values.
    pub values: Vec<f64>,
    /// Exact scalar encodings shared by both witnesses.
    pub raw_values: Vec<[u8; 8]>,
    /// Absolute offsets of the first and repeated scalar lanes.
    pub witness_offsets: [Vec<usize>; 2],
}

/// Two tagged offset-store indices following each repeated scalar-lane witness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimpleHoleRepeatedScalarLaneBlockReferences {
    /// Ordered block indices following the first coordinate pair.
    pub first: [u32; 2],
    /// Ordered block indices following the repeated coordinate pair.
    pub second: [u32; 2],
    /// Absolute offsets of the four tagged-index tokens.
    pub offsets: [[usize; 2]; 2],
    /// Exact optional eight-byte wrappers before the two reference pairs.
    pub prefixes: [Option<[u8; 8]>; 2],
}

/// Four construction-block references carried by a `HOLE PACKAGE` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HolePackageConstructionGroupLane {
    /// Payload-relative offset of the fixed lane prefix.
    pub offset: usize,
    /// Compact selector preceding the repeated branch byte.
    pub selector: u8,
    /// Branch byte repeated between the two reference pairs.
    pub branch: u8,
    /// Ordered first and second construction-block pairs.
    pub references: [PayloadObjectReference; 4],
}

/// Width form of one self-delimiting operation-payload scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadScalarEncoding {
    /// Single-byte exact zero.
    Zero,
    /// Four-byte shifted IEEE-754 binary32.
    Binary32,
    /// Eight-byte shifted IEEE-754 binary64.
    Binary64,
}

/// One typed scalar in a bounded operation payload.
#[derive(Debug, Clone, PartialEq)]
pub struct PayloadScalar {
    /// Absolute offset of the scalar marker.
    pub offset: usize,
    /// Finite scalar value.
    pub value: f64,
    /// Serialized width form.
    pub encoding: PayloadScalarEncoding,
    /// Exact serialized scalar atom.
    pub raw_value: Vec<u8>,
}

/// One three-scalar clause anchored to an ordered operation body reference.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationBodyScalarTriple {
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    pub branch: u8,
    /// Three scalar atoms in byte order.
    pub scalars: [PayloadScalar; 3],
}

/// One wrapped member index in a branch-`11` operation body clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationBodyMember {
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Zero-based member order in the counted lane.
    pub ordinal: u32,
    /// Decoded compact index.
    pub member_index: u32,
    /// Exact compact-index token.
    pub raw_member_index: Vec<u8>,
    /// Absolute offset of the compact-index marker.
    pub offset: usize,
}

/// Exact continuation following a `TRIM BODY` branch-`11` member lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationBody11Continuation {
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Compact index in the single-entry continuation lane.
    pub continuation_index: u32,
    /// Exact compact-index token in the continuation lane.
    pub raw_continuation_index: Vec<u8>,
    /// Absolute offset of the continuation compact-index marker.
    pub continuation_offset: usize,
    /// Object index in the terminal field.
    pub terminal_object_index: u32,
    /// Exact serialized terminal object-index token.
    pub raw_terminal_object_index: Vec<u8>,
    /// Absolute offset of the terminal object-index marker.
    pub terminal_offset: usize,
}

/// Homogeneous value encoding in an operation body-reference lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationBodyReferenceLaneEncoding {
    /// NX OM compact-index encoding.
    CompactIndex,
    /// `f0`/`f1` payload object-index encoding.
    PayloadObjectIndex,
}

/// One value in a bounded operation body-reference lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationBodyReferenceLaneValue {
    /// Zero-based value order.
    pub ordinal: u32,
    /// Decoded index.
    pub object_index: u32,
    /// Exact encoded index token.
    pub raw_value: Vec<u8>,
    /// Absolute offset of the encoded index marker.
    pub offset: usize,
}

/// Counted reference lane following an operation body scalar clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationBodyReferenceLane {
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    pub branch: u8,
    /// Homogeneous encoding used by every lane value.
    pub encoding: OperationBodyReferenceLaneEncoding,
    /// Ordered non-null lane values.
    pub values: Vec<OperationBodyReferenceLaneValue>,
}

/// Structured `32` branch following an extrusion body reference.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtrudePayload32Branch {
    /// Absolute offset of the `32` branch marker.
    pub offset: usize,
    /// Body object index anchoring the branch.
    pub body_object_index: u32,
    /// Finite shifted-IEEE scalar following the branch marker.
    pub scalar: f64,
    /// Exact shifted-binary64 scalar encoding.
    pub raw_scalar: [u8; 8],
    /// Ordered fixed-width big-endian atoms in the first counted lane.
    pub atoms_be: Vec<u32>,
    /// Absolute offsets of the fixed-width atoms in lane order.
    pub atom_offsets: Vec<usize>,
    /// Compact indices wrapped by the fixed-width atoms.
    pub atom_indices: Vec<u32>,
    /// Ordered values in the first compact-index lane.
    pub first_indices: Vec<u32>,
    /// Exact compact-index tokens in the first lane.
    pub raw_first_indices: Vec<Vec<u8>>,
    /// Absolute offsets of the first-lane compact-index tokens.
    pub first_index_offsets: Vec<usize>,
    /// Ordered values in the second compact-index lane.
    pub second_indices: Vec<u32>,
    /// Exact compact-index tokens in the second lane.
    pub raw_second_indices: Vec<Vec<u8>>,
    /// Absolute offsets of the second-lane compact-index tokens.
    pub second_index_offsets: Vec<usize>,
    /// Object index in the terminal field.
    pub terminal_object_index: u32,
    /// Exact serialized terminal object-index token.
    pub raw_terminal_object_index: Vec<u8>,
    /// Absolute offset of the terminal object-index token.
    pub terminal_offset: usize,
}

/// Ordered construction-reference field at the start of a `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockConstructionReferenceField {
    /// Payload control byte preceding the field framing.
    pub control: u8,
    /// Eighteen leading references followed by the terminal reference.
    pub references: Vec<PayloadObjectReference>,
}

/// Self-framed NX parameter name in one bounded expression declaration record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpressionDeclarationName<'a> {
    /// Byte offset of the `04` marker within the containing byte range.
    pub offset: usize,
    /// Exact `p<decimal>[_qualifier]` name.
    pub value: &'a str,
    /// Decimal parameter identifier following `p`.
    pub parameter_index: u32,
    /// Qualified role following the parameter identifier.
    pub qualifier: Option<&'a str>,
    /// Independently framed numeric literal in the declaration record.
    pub literal: Option<&'a str>,
}

/// Primary body-object reference carried by one bounded operation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationBodyReference {
    /// Absolute offset of the object-index token.
    pub offset: usize,
    /// Referenced body object index.
    pub object_index: u32,
    /// Exact serialized variable-width object-index token.
    pub raw_object_index: Vec<u8>,
}

/// One exact nested object-relation frame in a bounded operation record.
///
/// Object indices in wire order; roles depend on the owning operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationObjectRelation {
    /// Absolute offset of the opening `01 02` marker.
    pub offset: usize,
    /// Byte between the opening marker and the first object index.
    pub link_tag: u8,
    /// First object index in serialized order.
    pub first_object_index: u32,
    /// Exact serialized first object-index token.
    pub raw_first_object_index: Vec<u8>,
    /// Absolute offset of the first object-index token.
    pub first_object_index_offset: usize,
    /// Second object index in serialized order.
    pub second_object_index: u32,
    /// Exact serialized second object-index token.
    pub raw_second_object_index: Vec<u8>,
    /// Absolute offset of the second object-index token.
    pub second_object_index_offset: usize,
    /// Exclusive absolute end offset after the frame terminator.
    pub end_offset: usize,
}

/// One exact direct tagged-reference field in a bounded operation record.
///
/// The field's tag is retained as native evidence; it does not assign a
/// semantic role to the referenced object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationTaggedReference {
    /// Absolute offset of the opening `01 02` marker.
    pub offset: usize,
    /// Byte between the opening marker and the object index.
    pub tag: u8,
    /// Referenced feature object index.
    pub object_index: u32,
    /// Exact serialized variable-width object-index token.
    pub raw_object_index: Vec<u8>,
    /// Absolute offset of the object-index token.
    pub object_index_offset: usize,
    /// Exclusive absolute end offset after the fixed field suffix.
    pub end_offset: usize,
}

/// One exact direct operation data-block reference field.
///
/// The frame retains its object index and fixed suffix without assigning an
/// operation or construction role to the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationDataBlockReference {
    /// Absolute offset of the opening `01 02` marker.
    pub offset: usize,
    /// Referenced feature object index.
    pub object_index: u32,
    /// Exact serialized variable-width object-index token.
    pub raw_object_index: Vec<u8>,
    /// Absolute offset of the object-index token.
    pub object_index_offset: usize,
    /// Exclusive absolute end offset after the fixed field suffix.
    pub end_offset: usize,
}

/// Object-index reference in one bounded offset-only OM data block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataBlockObjectReference {
    /// Byte offset of the object-index token within the containing byte range.
    pub offset: usize,
    /// Referenced OM object ID.
    pub object_index: u32,
    /// Exact serialized object-index token.
    pub raw_object_index: Vec<u8>,
}

/// Boolean operation kind stored after an operation label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOperationKind {
    /// Add tool bodies to the target.
    Unite,
    /// Remove tool bodies from the target.
    Subtract,
    /// Retain target/tool intersections.
    Intersect,
}

/// One feature-history Boolean with object-index operands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BooleanOperation {
    /// Absolute offset of the operation label tag.
    pub offset: usize,
    /// Boolean operation kind.
    pub kind: BooleanOperationKind,
    /// Object index of the target body.
    pub target: u32,
    /// Exact serialized target object-index token.
    pub raw_target: Vec<u8>,
    /// Absolute offset of the target object-index token.
    pub target_offset: usize,
    /// Ordered object indices of the tool bodies.
    pub tools: Vec<u32>,
    /// Exact serialized tool object-index tokens in tool order.
    pub raw_tools: Vec<Vec<u8>>,
    /// Absolute offsets of the tool object-index tokens in tool order.
    pub tool_offsets: Vec<usize>,
}

impl<'a> IndexedSection<'a> {
    /// Return the section base used by its external record offsets.
    pub const fn base_offset(&self) -> usize {
        self.base
    }

    /// Decode explicit numeric-expression text within bounded entity records.
    pub fn numeric_expressions(&self) -> Vec<NumericExpression<'a>> {
        self.numeric_expression_records()
            .into_iter()
            .map(|(_, expression)| expression)
            .collect()
    }

    /// Decode expressions together with their owning record ordinal.
    pub fn numeric_expression_records(&self) -> Vec<(usize, NumericExpression<'a>)> {
        if !self.records.iter().any(|record| {
            record
                .bytes
                .windows(b"hostglobalvariables".len())
                .any(|window| window == b"hostglobalvariables")
        }) {
            return Vec::new();
        }
        self.records
            .iter()
            .enumerate()
            .filter_map(|(record_ordinal, record)| {
                numeric_expression_at(record.bytes, record.offset, record.object_id)
                    .map(|expression| (record_ordinal, expression))
            })
            .collect()
    }

    /// Decode every strictly framed printable string in each bounded record.
    pub fn string_values(&self) -> Vec<(usize, usize, Option<u32>, StringValue<'a>)> {
        self.records
            .iter()
            .enumerate()
            .flat_map(|(record_ordinal, record)| {
                string_values(record.bytes, record.offset)
                    .into_iter()
                    .enumerate()
                    .map(move |(value_ordinal, value)| {
                        (record_ordinal, value_ordinal, record.object_id, value)
                    })
            })
            .collect()
    }

    /// Decode tagged cross-record references from every bounded record.
    pub fn references(&self) -> Vec<(usize, usize, Option<u32>, ReferenceValue)> {
        self.records
            .iter()
            .enumerate()
            .flat_map(|(record_ordinal, record)| {
                let mut references = record_references(record.bytes, record.offset);
                references.extend(counted_record_references(
                    record.bytes,
                    record.offset,
                    self.records.len(),
                ));
                references.sort_by_key(|reference| reference.offset);
                references
                    .into_iter()
                    .enumerate()
                    .map(move |(reference_ordinal, reference)| {
                        (
                            record_ordinal,
                            reference_ordinal,
                            record.object_id,
                            reference,
                        )
                    })
            })
            .collect()
    }
}

impl<'a> Section<'a> {
    /// Decode the validated record-area control and product header.
    pub fn record_area_header(&self) -> Option<RecordAreaHeader<'a>> {
        let bytes = self.record_area?;
        let offset = self.record_area_offset?;
        let control_words = [
            View::u32_le_at(bytes, 0)?,
            View::u32_le_at(bytes, 4)?,
            View::u32_le_at(bytes, 8)?,
        ];
        let suffix = bytes.get(12..)?;
        is_product_record(suffix).then_some(())?;
        let length = usize::from(suffix[2]) - 2;
        Some(RecordAreaHeader {
            offset,
            control_words,
            product: StoreVersion {
                offset: offset + 12,
                value: std::str::from_utf8(&suffix[3..3 + length]).ok()?,
            },
        })
    }

    /// Decode strictly framed operation labels from the pointed record area.
    pub fn operation_labels(&self) -> Vec<OperationLabel<'a>> {
        self.cached_operation_labels.to_vec()
    }

    /// Return the validated operation-label layouts for container caching.
    pub(crate) fn operation_label_layouts(&self) -> Vec<OperationLabelLayout> {
        operation_label_layouts(&self.cached_operation_labels)
    }

    /// Decode fully framed Boolean operations from the pointed record area.
    pub fn boolean_operations(&self) -> Vec<BooleanOperation> {
        let Some(bytes) = self.record_area else {
            return Vec::new();
        };
        let Some(base_offset) = self.record_area_offset else {
            return Vec::new();
        };
        boolean_operations_with_labels(bytes, base_offset, &self.cached_operation_labels)
    }

    /// Bound operation records and retain their ordinal in the complete label sequence.
    pub fn operation_records_with_label_ordinals(&self) -> Vec<(usize, OperationRecord<'a>)> {
        let Some(bytes) = self.record_area else {
            return Vec::new();
        };
        let Some(base_offset) = self.record_area_offset else {
            return Vec::new();
        };
        operation_records_with_labels_and_ordinals(
            bytes,
            base_offset,
            &self.cached_operation_labels,
        )
    }

    /// Decode unambiguous primary body references from bounded operation records.
    pub fn operation_body_references(&self) -> Vec<(usize, OperationBodyReference)> {
        self.operation_records_with_label_ordinals()
            .into_iter()
            .filter_map(|(ordinal, record)| {
                operation_body_reference(record).map(|reference| (ordinal, reference))
            })
            .collect()
    }
}

/// Decode complete feature-operation headers and their label frames.
pub fn operation_labels(bytes: &[u8], base_offset: usize) -> Vec<OperationLabel<'_>> {
    const HEADER: &[u8] = &[
        0x80, 0xcd, 0x01, 0x04, 0x01, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 0xff, 0xff,
    ];
    let mut labels = Vec::new();
    for marker in bytes
        .windows(HEADER.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == HEADER).then_some(offset))
    {
        let mut at = marker + HEADER.len();
        let mut object_indices = [None; 4];
        let mut object_index_offsets = [0; 4];
        let mut valid = true;
        for (slot, offset) in object_indices
            .iter_mut()
            .zip(object_index_offsets.iter_mut())
        {
            *offset = base_offset + at;
            let Some((value, next)) = feature_object_index(bytes, at) else {
                valid = false;
                break;
            };
            *slot = value;
            at = next;
        }
        if !valid || bytes.get(at) != Some(&0x03) {
            continue;
        }
        let Some(length) = bytes.get(at + 1).copied().map(usize::from) else {
            continue;
        };
        if length < 3 {
            continue;
        }
        let Some(end) = at.checked_add(length) else {
            continue;
        };
        if bytes.get(end) != Some(&0) {
            continue;
        }
        let Some(name) = bytes.get(at + 2..end) else {
            continue;
        };
        if !name
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        {
            continue;
        }
        let Ok(value) = std::str::from_utf8(name) else {
            continue;
        };
        labels.push(OperationLabel {
            header_offset: base_offset + marker,
            offset: base_offset + at,
            value,
            object_indices,
            object_index_offsets,
        });
    }
    labels
}

/// Bound every validated operation header through its successor or area end.
#[allow(dead_code)] // Direct byte-slice parser entry point retained for focused parser tests.
pub fn operation_records(bytes: &[u8], base_offset: usize) -> Vec<OperationRecord<'_>> {
    let labels = operation_labels(bytes, base_offset);
    operation_records_with_labels(bytes, base_offset, &labels)
}

fn operation_records_with_labels<'a>(
    bytes: &'a [u8],
    base_offset: usize,
    labels: &[OperationLabel<'a>],
) -> Vec<OperationRecord<'a>> {
    operation_records_with_labels_and_ordinals(bytes, base_offset, labels)
        .into_iter()
        .map(|(_, record)| record)
        .collect()
}

fn operation_records_with_labels_and_ordinals<'a>(
    bytes: &'a [u8],
    base_offset: usize,
    labels: &[OperationLabel<'a>],
) -> Vec<(usize, OperationRecord<'a>)> {
    labels
        .iter()
        .enumerate()
        .filter_map(|(ordinal, label)| {
            let start = label.header_offset.checked_sub(base_offset)?;
            let end = labels
                .get(ordinal + 1)
                .map_or(bytes.len(), |next| next.header_offset - base_offset);
            let label_at = label.offset.checked_sub(base_offset)?;
            let payload_start = label_at
                .checked_add(usize::from(*bytes.get(label_at + 1)?))?
                .checked_add(1)?;
            Some((
                ordinal,
                OperationRecord {
                    offset: label.header_offset,
                    bytes: bytes.get(start..end)?,
                    payload_offset: base_offset + payload_start,
                    payload: bytes.get(payload_start..end)?,
                    label: *label,
                },
            ))
        })
        .collect()
}

/// Decode ordered `04, length, text, 00` strings from one operation payload.
pub fn operation_payload_strings(record: OperationRecord<'_>) -> Vec<OperationPayloadString<'_>> {
    let mut strings = Vec::new();
    let mut at = 0usize;
    while at + 4 <= record.payload.len() {
        if record.payload[at] != 0x04 {
            at += 1;
            continue;
        }
        let declared = usize::from(record.payload[at + 1]);
        let Some(end) = at.checked_add(declared) else {
            at += 1;
            continue;
        };
        let Some(raw) = record.payload.get(at + 2..end) else {
            at += 1;
            continue;
        };
        let Some(value) = std::str::from_utf8(raw).ok().filter(|value| {
            !value.is_empty() && value.chars().all(|character| !character.is_control())
        }) else {
            at += 1;
            continue;
        };
        if declared < 3 || record.payload.get(end) != Some(&0) {
            at += 1;
            continue;
        }
        strings.push(OperationPayloadString {
            offset: record.payload_offset + at,
            value,
        });
        at = end + 1;
    }
    strings
}

/// Decode an exact nonempty duplicated shifted-binary64 lane before a hole template.
pub fn simple_hole_repeated_scalar_lane(
    record: OperationRecord<'_>,
) -> Option<SimpleHoleRepeatedScalarLane> {
    if record.label.value != "SIMPLE HOLE" {
        return None;
    }
    let templates = operation_payload_strings(record)
        .into_iter()
        .filter(|value| value.value.starts_with("Hole_"))
        .collect::<Vec<_>>();
    let [template] = templates.as_slice() else {
        return None;
    };
    let boundary = template.offset.checked_sub(record.payload_offset)?;
    let prefix = record.payload.get(..boundary)?;
    let mut scalars = Vec::new();
    let mut at = 0usize;
    while at + 8 <= prefix.len() {
        if prefix[at] == 0x30 {
            let raw_value = <[u8; 8]>::try_from(&prefix[at..at + 8]).ok()?;
            if let Some(value) = shifted_ieee_f64(&raw_value) {
                scalars.push((raw_value, value, record.payload_offset + at));
                at += 8;
                continue;
            }
        }
        at += 1;
    }
    let half = scalars.len().checked_div(2)?;
    if half == 0 || scalars.len() != half * 2 {
        return None;
    }
    let (first, second) = scalars.split_at(half);
    if first
        .iter()
        .zip(second)
        .any(|(left, right)| left.0 != right.0)
    {
        return None;
    }
    Some(SimpleHoleRepeatedScalarLane {
        values: first.iter().map(|scalar| scalar.1).collect(),
        raw_values: first.iter().map(|scalar| scalar.0).collect(),
        witness_offsets: [
            first.iter().map(|scalar| scalar.2).collect(),
            second.iter().map(|scalar| scalar.2).collect(),
        ],
    })
}

/// Decode the two tagged block indices immediately following each witnessed
/// simple-hole scalar lane.
pub fn simple_hole_repeated_scalar_lane_block_references(
    record: OperationRecord<'_>,
) -> Option<SimpleHoleRepeatedScalarLaneBlockReferences> {
    const FIRST_PREFIX: [u8; 8] = [0x50, 0x10, 0x00, 0x04, 0x50, 0x49, 0x66, 0x2e];
    const SECOND_PREFIX: [u8; 8] = [0x50, 0x21, 0x66, 0x62, 0x50, 0x49, 0x66, 0x2e];
    let pair = simple_hole_repeated_scalar_lane(record)?;
    let decode_pair = |coordinate_offset: usize, admitted_prefix: [u8; 8]| {
        let relative = coordinate_offset.checked_sub(record.payload_offset)?;
        let mut at = relative.checked_add(8)?;
        let prefix = if payload_object_index(record.payload.get(at..)?).is_some() {
            None
        } else {
            let candidate =
                <[u8; 8]>::try_from(record.payload.get(at..at.checked_add(8)?)?).ok()?;
            (candidate == admitted_prefix).then_some(())?;
            at += 8;
            Some(candidate)
        };
        let first_offset = at;
        let (first, width) = payload_object_index(record.payload.get(at..)?)?;
        at += width;
        let second_offset = at;
        let (second, _) = payload_object_index(record.payload.get(at..)?)?;
        Some((
            [first, second],
            [
                record.payload_offset + first_offset,
                record.payload_offset + second_offset,
            ],
            prefix,
        ))
    };
    let (first, first_offsets, first_prefix) =
        decode_pair(*pair.witness_offsets[0].last()?, FIRST_PREFIX)?;
    let (second, second_offsets, second_prefix) =
        decode_pair(*pair.witness_offsets[1].last()?, SECOND_PREFIX)?;
    Some(SimpleHoleRepeatedScalarLaneBlockReferences {
        first,
        second,
        offsets: [first_offsets, second_offsets],
        prefixes: [first_prefix, second_prefix],
    })
}

/// Decode the unique four-block construction-group lane in a `HOLE PACKAGE` payload.
pub fn hole_package_construction_group_lane(
    record: OperationRecord<'_>,
) -> Option<HolePackageConstructionGroupLane> {
    const PREFIX: [u8; 5] = [0x00, 0x00, 0x01, 0x00, 0x00];
    const ZEROES: [u8; 4] = [0; 4];
    const SUFFIX: [u8; 3] = [0x00, 0x00, 0xff];
    if record.label.value != "HOLE PACKAGE" {
        return None;
    }
    let mut matches = Vec::new();
    for start in 0..record.payload.len().saturating_sub(PREFIX.len()) {
        if record.payload.get(start..start + PREFIX.len()) != Some(&PREFIX) {
            continue;
        }
        let Some(lane) = (|| {
            let selector = *record.payload.get(start + 5)?;
            let branch = *record.payload.get(start + 7)?;
            if selector == 0
                || branch == 0
                || record.payload.get(start + 6) != Some(&0)
                || record.payload.get(start + 8..start + 12) != Some(&ZEROES)
            {
                return None;
            }
            let mut at = start + 12;
            let mut references = Vec::with_capacity(4);
            for ordinal in 0..4 {
                if ordinal == 2 {
                    if record.payload.get(at) != Some(&branch)
                        || record.payload.get(at + 1..at + 5) != Some(&ZEROES)
                    {
                        return None;
                    }
                    at += 5;
                }
                let reference_offset = at;
                let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
                if !matches!(record.payload.get(at), Some(0xf0 | 0xf1)) {
                    return None;
                }
                references.push(PayloadObjectReference {
                    offset: record.payload_offset + reference_offset,
                    object_index,
                    raw_object_index: record.payload.get(at..at + width)?.to_vec(),
                });
                at += width;
            }
            if record.payload.get(at..at + SUFFIX.len()) != Some(&SUFFIX) {
                return None;
            }
            Some(HolePackageConstructionGroupLane {
                offset: start,
                selector,
                branch,
                references: references.try_into().ok()?,
            })
        })() else {
            continue;
        };
        matches.push(lane);
    }
    let [lane] = matches.as_slice() else {
        return None;
    };
    Some(lane.clone())
}

/// Decode the unique counted reference field in a bounded `SKETCH` payload.
pub fn sketch_payload_references(
    record: OperationRecord<'_>,
) -> Option<SketchPayloadReferenceField> {
    if record.label.value != "SKETCH" {
        return None;
    }
    let mut matches = Vec::new();
    for start in 0..record.payload.len().saturating_sub(3) {
        if record.payload.get(start..start + 2) != Some(&[0x01, 0x00]) {
            continue;
        }
        if let Some(references) = sketch_reference_field(record, start) {
            matches.push(references);
        }
    }
    let [references] = matches.as_slice() else {
        return None;
    };
    Some(references.clone())
}

fn sketch_reference_field(
    record: OperationRecord<'_>,
    start: usize,
) -> Option<SketchPayloadReferenceField> {
    let flag = *record.payload.get(start + 2)?;
    let (declared_count, mut at) = match flag {
        0 => (0, start + 3),
        1 => {
            let count = *record.payload.get(start + 3)?;
            if count == 0 {
                return None;
            }
            (count, start + 4)
        }
        _ => return None,
    };
    let leading_count = declared_count.saturating_sub(1) as usize;
    let leading_start = at;
    let mut scan_at = leading_start;
    for _ in 0..leading_count {
        let (_, width) = payload_object_index(record.payload.get(scan_at..)?)?;
        scan_at += width;
    }
    if record.payload.get(scan_at..scan_at + 2) != Some(&[0x00, 0x00]) {
        return None;
    }
    scan_at += 2;
    let (_, width) = payload_object_index(record.payload.get(scan_at..)?)?;
    scan_at += width;
    if record.payload.get(scan_at..scan_at + 4) != Some(&[0x01, 0x00, 0x00, 0x00]) {
        return None;
    }

    let mut references = Vec::with_capacity(leading_count + 1);
    at = leading_start;
    for _ in 0..leading_count {
        let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
        references.push(PayloadObjectReference {
            offset: record.payload_offset + at,
            object_index,
            raw_object_index: record.payload[at..at + width].to_vec(),
        });
        at += width;
    }
    at += 2;
    let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
    references.push(PayloadObjectReference {
        offset: record.payload_offset + at,
        object_index,
        raw_object_index: record.payload[at..at + width].to_vec(),
    });
    Some(SketchPayloadReferenceField {
        declared_count,
        references,
    })
}

fn payload_object_index(bytes: &[u8]) -> Option<(u32, usize)> {
    match *bytes.first()? {
        0xf0 => Some((u32::from(*bytes.get(1)?), 2)),
        0xf1 => {
            let value = u16::from_be_bytes([*bytes.get(1)?, *bytes.get(2)?]);
            (value >= 0x0100).then_some((u32::from(value), 3))
        }
        _ => None,
    }
}

/// Decode the unique exactly framed construction-reference field in a bounded
/// projected-curve payload.
pub fn projected_curve_payload_references(
    record: OperationRecord<'_>,
) -> Option<ProjectedCurvePayloadReferenceField> {
    const CPROJ_MIDDLE: [u8; 5] = [0x80, 0x57, 0x00, 0x02, 0x01];
    const CPROJ_SUFFIX: [u8; 5] = [0xff, 0x01, 0x02, 0x02, 0x7d];
    const CMB_PREFIX: [u8; 10] = [0x3c, 0x32, 0x01, 0x02, 0x32, 0x01, 0x04, 0x36, 0x01, 0x33];
    const CMB_BRANCH_PREFIX: [u8; 3] = [0x16, 0x01, 0x02];
    const CMB_BRANCH_MIDDLE: [u8; 7] = [0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00];
    const CMB_BRANCH_SUFFIX: [u8; 3] = [0x00, 0x81, 0x5c];
    const CMB_TAIL_PREFIX: [u8; 4] = [0xff, 0x01, 0xff, 0x01];
    const CMB_TAIL_SUFFIX: [u8; 2] = [0x04, 0x02];
    let decode_reference = |at: &mut usize| {
        let offset = *at;
        let (object_index, width) = payload_object_index(record.payload.get(offset..)?)?;
        *at += width;
        Some(PayloadObjectReference {
            offset: record.payload_offset + offset,
            object_index,
            raw_object_index: record.payload[offset..offset + width].to_vec(),
        })
    };
    let decode_field = |start: usize| match record.label.value {
        "CPROJ" => {
            let mut at = start + 2;
            let mut references = Vec::with_capacity(3);
            references.push(decode_reference(&mut at)?);
            references.push(decode_reference(&mut at)?);
            (record.payload.get(at..at + CPROJ_MIDDLE.len()) == Some(&CPROJ_MIDDLE))
                .then_some(())?;
            at += CPROJ_MIDDLE.len();
            references.push(decode_reference(&mut at)?);
            (record.payload.get(at..at + CPROJ_SUFFIX.len()) == Some(&CPROJ_SUFFIX))
                .then_some(())?;
            Some(ProjectedCurvePayloadReferenceField { references })
        }
        "CPROJ_CMB" => {
            let mut at = start + CMB_PREFIX.len();
            let mut references = Vec::with_capacity(8);
            references.push(decode_reference(&mut at)?);
            (record.payload.get(at) == Some(&0x33)).then_some(())?;
            at += 1;
            references.push(decode_reference(&mut at)?);
            (record.payload.get(at) == Some(&0x00)).then_some(())?;
            at += 1;
            references.push(decode_reference(&mut at)?);
            (record.payload.get(at..at + 6) == Some(&[0; 6])).then_some(())?;
            at += 6;
            references.push(decode_reference(&mut at)?);
            for anchor in 0..2 {
                (record.payload.get(at..at + CMB_BRANCH_PREFIX.len()) == Some(&CMB_BRANCH_PREFIX))
                    .then_some(())?;
                at += CMB_BRANCH_PREFIX.len();
                let repeated = decode_reference(&mut at)?;
                (repeated.object_index == references[anchor].object_index).then_some(())?;
                (record.payload.get(at..at + CMB_BRANCH_MIDDLE.len()) == Some(&CMB_BRANCH_MIDDLE))
                    .then_some(())?;
                at += CMB_BRANCH_MIDDLE.len();
                (record.payload.get(at..at + 3) == Some(&[0xff, 0x01, 0x02])).then_some(())?;
                at += 3;
                references.push(decode_reference(&mut at)?);
                (record.payload.get(at..at + CMB_BRANCH_SUFFIX.len()) == Some(&CMB_BRANCH_SUFFIX))
                    .then_some(())?;
                at += CMB_BRANCH_SUFFIX.len();
            }
            (record.payload.get(at..at + CMB_TAIL_PREFIX.len()) == Some(&CMB_TAIL_PREFIX))
                .then_some(())?;
            at += CMB_TAIL_PREFIX.len();
            references.push(decode_reference(&mut at)?);
            references.push(decode_reference(&mut at)?);
            (record.payload.get(at..at + CMB_TAIL_SUFFIX.len()) == Some(&CMB_TAIL_SUFFIX))
                .then_some(())?;
            Some(ProjectedCurvePayloadReferenceField { references })
        }
        _ => None,
    };
    let marker = match record.label.value {
        "CPROJ" => &[0x01, 0x02][..],
        "CPROJ_CMB" => &CMB_PREFIX[..],
        _ => return None,
    };
    let mut matches = Vec::new();
    for start in 0..=record.payload.len().saturating_sub(marker.len()) {
        if record.payload.get(start..start + marker.len()) != Some(marker) {
            continue;
        }
        if let Some(field) = decode_field(start) {
            matches.push(field);
        }
    }
    let [field] = matches.as_slice() else {
        return None;
    };
    Some(field.clone())
}

/// Decode the unique exactly framed construction-reference field in a bounded
/// pattern payload.
pub fn pattern_payload_references(
    record: OperationRecord<'_>,
) -> Option<PatternPayloadReferenceField> {
    const GRAPH_SEPARATOR: [u8; 4] = [0xff, 0x00, 0xff, 0x01];
    const GRAPH_TAIL_PREFIX: [u8; 4] = [0xff, 0x00, 0x00, 0x01];
    const GRAPH_SUFFIX: [u8; 3] = [0xff, 0xff, 0x01];
    const COMPACT_GRAPH_SEPARATOR: [u8; 3] = [0xff, 0x00, 0x01];
    const COMPACT_GRAPH_MIDDLE: [u8; 2] = [0xff, 0x3c];
    const INSTANCE_PREFIX: [u8; 3] = [0x00, 0xff, 0xff];
    const INSTANCE_SUFFIX: [u8; 17] = [
        0x01, 0x02, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00,
        0x01, 0x02,
    ];
    let decode_reference = |at: &mut usize| {
        let offset = *at;
        let (object_index, width) = payload_object_index(record.payload.get(offset..)?)?;
        *at += width;
        Some(PayloadObjectReference {
            offset: record.payload_offset + offset,
            object_index,
            raw_object_index: record.payload[offset..offset + width].to_vec(),
        })
    };
    let decode_graph = |start: usize| {
        let mut at = start + 1;
        let mut references = Vec::with_capacity(10);
        references.push(decode_reference(&mut at)?);
        (record.payload.get(at..at + GRAPH_SEPARATOR.len()) == Some(&GRAPH_SEPARATOR))
            .then_some(())?;
        at += GRAPH_SEPARATOR.len();
        references.push(decode_reference(&mut at)?);
        references.push(decode_reference(&mut at)?);
        (record.payload.get(at) == Some(&0x61)).then_some(())?;
        at += 1;
        references.push(decode_reference(&mut at)?);
        (record.payload.get(at..at + GRAPH_SEPARATOR.len()) == Some(&GRAPH_SEPARATOR))
            .then_some(())?;
        at += GRAPH_SEPARATOR.len();
        references.push(decode_reference(&mut at)?);
        references.push(decode_reference(&mut at)?);
        (record.payload.get(at..at + 2) == Some(&[0xff, 0x62])).then_some(())?;
        at += 2;
        references.push(decode_reference(&mut at)?);
        references.push(decode_reference(&mut at)?);
        (record.payload.get(at..at + GRAPH_TAIL_PREFIX.len()) == Some(&GRAPH_TAIL_PREFIX))
            .then_some(())?;
        at += GRAPH_TAIL_PREFIX.len();
        references.push(decode_reference(&mut at)?);
        if record.payload.get(at) == Some(&0xff) {
            at += 1;
        } else {
            references.push(decode_reference(&mut at)?);
        }
        (record.payload.get(at..at + GRAPH_SUFFIX.len()) == Some(&GRAPH_SUFFIX)).then_some(())?;
        Some(PatternPayloadReferenceField {
            layout: PatternPayloadReferenceLayout::CanonicalGraph,
            references,
        })
    };
    let decode_compact_graph = |start: usize| {
        let mut at = start + 1;
        let mut references = Vec::with_capacity(10);
        references.push(decode_reference(&mut at)?);
        (record.payload.get(at..at + COMPACT_GRAPH_SEPARATOR.len())
            == Some(&COMPACT_GRAPH_SEPARATOR))
        .then_some(())?;
        at += COMPACT_GRAPH_SEPARATOR.len();
        references.push(decode_reference(&mut at)?);
        references.push(decode_reference(&mut at)?);
        (record.payload.get(at) == Some(&0x3b)).then_some(())?;
        at += 1;
        references.push(decode_reference(&mut at)?);
        (record.payload.get(at..at + COMPACT_GRAPH_SEPARATOR.len())
            == Some(&COMPACT_GRAPH_SEPARATOR))
        .then_some(())?;
        at += COMPACT_GRAPH_SEPARATOR.len();
        references.push(decode_reference(&mut at)?);
        references.push(decode_reference(&mut at)?);
        (record.payload.get(at..at + COMPACT_GRAPH_MIDDLE.len()) == Some(&COMPACT_GRAPH_MIDDLE))
            .then_some(())?;
        at += COMPACT_GRAPH_MIDDLE.len();
        references.push(decode_reference(&mut at)?);
        references.push(decode_reference(&mut at)?);
        (record.payload.get(at..at + GRAPH_TAIL_PREFIX.len()) == Some(&GRAPH_TAIL_PREFIX))
            .then_some(())?;
        at += GRAPH_TAIL_PREFIX.len();
        references.push(decode_reference(&mut at)?);
        if record.payload.get(at) == Some(&0xff) {
            at += 1;
        } else {
            references.push(decode_reference(&mut at)?);
        }
        (record.payload.get(at..at + GRAPH_SUFFIX.len()) == Some(&GRAPH_SUFFIX)).then_some(())?;
        Some(PatternPayloadReferenceField {
            layout: PatternPayloadReferenceLayout::CompactGraph,
            references,
        })
    };
    let decode_instance = |start: usize| {
        let mut at = start + INSTANCE_PREFIX.len();
        let reference = decode_reference(&mut at)?;
        (record.payload.get(at..at + INSTANCE_SUFFIX.len()) == Some(&INSTANCE_SUFFIX))
            .then_some(())?;
        Some(PatternPayloadReferenceField {
            layout: PatternPayloadReferenceLayout::GeometryInstance,
            references: vec![reference],
        })
    };
    let matches = match record.label.value {
        "Pattern Feature" | "Pattern Geometry" => (0..record.payload.len())
            .filter_map(|start| match record.payload.get(start) {
                Some(0x61) => decode_graph(start),
                Some(0x3b) => decode_compact_graph(start),
                _ => None,
            })
            .collect::<Vec<_>>(),
        "Geometry Instance" => (0..=record.payload.len().saturating_sub(INSTANCE_PREFIX.len()))
            .filter(|&start| {
                record.payload.get(start..start + INSTANCE_PREFIX.len()) == Some(&INSTANCE_PREFIX)
            })
            .filter_map(decode_instance)
            .collect::<Vec<_>>(),
        _ => return None,
    };
    let [field] = matches.as_slice() else {
        return None;
    };
    Some(field.clone())
}

/// Decode the unique exactly terminated counted reference lane in a bounded
/// `Pattern Feature` payload without assigning its reference roles.
pub fn pattern_payload_counted_reference_lane(
    record: OperationRecord<'_>,
) -> Option<PatternPayloadCountedReferenceLane> {
    const TRAILER: [u8; 19] = [
        0x00, 0x00, 0x00, 0x37, 0xff, 0xff, 0x01, 0x00, 0x00, 0x00, 0x38, 0xff, 0x01, 0xff, 0xff,
        0xff, 0xff, 0x01, 0xff,
    ];
    if record.label.value != "Pattern Feature" {
        return None;
    }
    let decode = |start: usize| {
        (record.payload.get(start) == Some(&0x01)).then_some(())?;
        let declared_count = *record.payload.get(start + 1)?;
        (declared_count >= 2).then_some(())?;
        let reference_count = usize::from(declared_count - 1);
        let references_start = start.checked_add(2)?;
        cadmpeg_core::decode::bounded_len(
            u64::from(declared_count - 1),
            2,
            record.payload.len().saturating_sub(references_start),
        )?;
        let mut scan_at = references_start;
        for _ in 0..reference_count {
            let (_, width) = payload_object_index(record.payload.get(scan_at..)?)?;
            scan_at = scan_at.checked_add(width)?;
        }
        let trailer_end = scan_at.checked_add(TRAILER.len())?;
        (record.payload.get(scan_at..trailer_end) == Some(&TRAILER)).then_some(())?;

        let mut at = references_start;
        let mut references = Vec::with_capacity(reference_count);
        for _ in 0..reference_count {
            let offset = at;
            let (object_index, width) = payload_object_index(record.payload.get(offset..)?)?;
            at = at.checked_add(width)?;
            references.push(PayloadObjectReference {
                offset: record.payload_offset + offset,
                object_index,
                raw_object_index: record.payload[offset..at].to_vec(),
            });
        }
        Some(PatternPayloadCountedReferenceLane {
            offset: record.payload_offset + start,
            declared_count,
            references,
        })
    };
    let matches = (0..record.payload.len())
        .filter_map(decode)
        .collect::<Vec<_>>();
    let [lane] = matches.as_slice() else {
        return None;
    };
    Some(lane.clone())
}

/// Decode the unique exactly bounded two-group reference graph in an `FSET`
/// payload without assigning selection roles to either group.
pub fn fset_payload_reference_graph(
    record: OperationRecord<'_>,
) -> Option<FsetPayloadReferenceGraph> {
    const SUFFIX: [u8; 3] = [0x00, 0x03, 0x00];
    if record.label.value != "FSET" {
        return None;
    }
    let decode_reference = |at: &mut usize| {
        let offset = *at;
        (record.payload.get(offset) == Some(&0x90)).then_some(())?;
        let object_index = u32::from(u16::from_be_bytes([
            *record.payload.get(offset + 1)?,
            *record.payload.get(offset + 2)?,
        ]));
        let width = 3;
        *at += width;
        Some(PayloadObjectReference {
            offset: record.payload_offset + offset,
            object_index,
            raw_object_index: record.payload[offset..offset + width].to_vec(),
        })
    };
    let decode = |start: usize| {
        (record.payload.get(start) == Some(&0x01)).then_some(())?;
        let declared_len = usize::from(*record.payload.get(start + 1)?);
        let body_start = start.checked_add(2)?;
        let body_end = body_start.checked_add(declared_len)?;
        (declared_len >= 9
            && record.payload.get(body_start) == Some(&0x3c)
            && record.payload.get(body_end.checked_sub(1)?) == Some(&0x3e))
        .then_some(())?;
        let first = (body_start + 2..body_end - 1)
            .filter_map(|selector_end| {
                let selector = record.payload.get(body_start + 1..selector_end)?;
                (!selector.is_empty()
                    && selector
                        .iter()
                        .all(|byte| byte.is_ascii_graphic() && *byte != 0x3e))
                .then_some(())?;
                let mut at = selector_end;
                let first = [decode_reference(&mut at)?, decode_reference(&mut at)?];
                (at == body_end - 1)
                    .then_some((std::str::from_utf8(selector).ok()?.to_string(), first))
            })
            .collect::<Vec<_>>();
        let [(selector, first)] = first.as_slice() else {
            return None;
        };
        let mut at = body_end;
        let second = [
            decode_reference(&mut at)?,
            decode_reference(&mut at)?,
            decode_reference(&mut at)?,
        ];
        (record.payload.get(at..at + SUFFIX.len()) == Some(&SUFFIX)).then_some(())?;
        Some(FsetPayloadReferenceGraph {
            selector: selector.clone(),
            first: first.clone(),
            second,
            offset: record.payload_offset + start,
        })
    };
    let matches = (0..record.payload.len().saturating_sub(1))
        .filter_map(decode)
        .collect::<Vec<_>>();
    let [graph] = matches.as_slice() else {
        return None;
    };
    Some(graph.clone())
}

/// Decode the exactly counted nullable construction-reference field at the
/// start of a bounded `DELETE` payload.
pub fn delete_payload_references(
    record: OperationRecord<'_>,
) -> Option<DeletePayloadReferenceField> {
    const PREFIX: [u8; 6] = [0x00, 0x00, 0x01, 0x00, 0x01, 0x06];
    if record.label.value != "DELETE" || record.payload.get(1..1 + PREFIX.len()) != Some(&PREFIX) {
        return None;
    }
    let control = *record.payload.first()?;
    let mut at = 1 + PREFIX.len();
    let mut references = Vec::with_capacity(5);
    for _ in 0..5 {
        let offset = at;
        let (object_index, width) = if record.payload.get(at) == Some(&0xff) {
            (None, 1)
        } else {
            let (object_index, width) = payload_object_index(&record.payload[at..])?;
            (Some(object_index), width)
        };
        at += width;
        references.push(DeletePayloadReferenceSlot {
            object_index,
            raw_object_index: record.payload[offset..at].to_vec(),
            offset: record.payload_offset + offset,
        });
    }
    let references = references.try_into().ok()?;
    (record.payload.get(at) == Some(&0x00)).then_some(DeletePayloadReferenceField {
        control,
        references,
        offset: record.payload_offset,
    })
}

/// Decode the unique exactly counted transform lane in a bounded pattern payload.
pub fn pattern_payload_transform_lane(
    record: OperationRecord<'_>,
) -> Option<PatternPayloadTransformLane> {
    const FEATURE_PREFIX_TAIL: [u8; 3] = [0x01, 0x00, 0x00];
    const FEATURE_SCALAR_SUFFIX: [u8; 14] = [
        0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x03,
    ];
    const GEOMETRY_PREFIX_TAIL: [u8; 7] = [0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00];
    const GEOMETRY_SCALAR_SUFFIX: [u8; 10] =
        [0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x03];
    const ROW_TAIL: [u8; 5] = [0x00, 0x00, 0xff, 0x00, 0x00];
    let (prefix_tail, scalar_suffix) = match record.label.value {
        "Pattern Feature" => (
            FEATURE_PREFIX_TAIL.as_slice(),
            FEATURE_SCALAR_SUFFIX.as_slice(),
        ),
        "Pattern Geometry" => (
            GEOMETRY_PREFIX_TAIL.as_slice(),
            GEOMETRY_SCALAR_SUFFIX.as_slice(),
        ),
        _ => return None,
    };
    let validate_scalar = |start: usize| {
        (record.payload.get(start) == Some(&0x01)).then_some(())?;
        let declared_count = *record.payload.get(start + 1)?;
        (declared_count >= 2).then_some(())?;
        let mut at = start + 2;
        let row_schema_index = *record.payload.get(at)?;
        for ordinal in 1..declared_count {
            (record.payload.get(at) == Some(&row_schema_index)).then_some(())?;
            (record.payload.get(at + 1..at + 1 + prefix_tail.len()) == Some(prefix_tail))
                .then_some(())?;
            at += 1 + prefix_tail.len();
            let (_, encoding, width) = payload_scalar(record.payload.get(at..)?)?;
            (encoding != PayloadScalarEncoding::Zero).then_some(())?;
            at += width;
            (record.payload.get(at..at + scalar_suffix.len()) == Some(scalar_suffix))
                .then_some(())?;
            at += scalar_suffix.len();
            let (CompactIndex::Value(_), width) = compact_index(record.payload.get(at..)?)? else {
                return None;
            };
            at += width;
            (record.payload.get(at) == Some(&0x01)).then_some(())?;
            (record.payload.get(at + 1) == Some(&ordinal)).then_some(())?;
            (record.payload.get(at + 2..at + 2 + ROW_TAIL.len()) == Some(&ROW_TAIL))
                .then_some(())?;
            at += 2 + ROW_TAIL.len();
        }
        let terminal_schema_index = row_schema_index.checked_sub(1)?;
        (record.payload.get(at) == Some(&terminal_schema_index)).then_some(())?;
        (record.payload.get(at + 1..at + 3) == Some(&[0x00, 0x00])).then_some(())?;
        (record.payload.get(at + 3) == Some(&0x01)).then_some(())?;
        Some((declared_count, row_schema_index))
    };
    let validate_wide = |start: usize| {
        (record.label.value == "Pattern Feature").then_some(())?;
        (record.payload.get(start) == Some(&0x01)).then_some(())?;
        let declared_count = *record.payload.get(start + 1)?;
        (declared_count >= 2).then_some(())?;
        let mut at = start + 2;
        let row_schema_index = *record.payload.get(at)?;
        for ordinal in 1..declared_count {
            (record.payload.get(at) == Some(&row_schema_index)).then_some(())?;
            at += 1;
            for value_ordinal in 0..4 {
                let (_, encoding, width) = payload_scalar(record.payload.get(at..)?)?;
                (encoding == PayloadScalarEncoding::Binary64 && width == 8).then_some(())?;
                at += width;
                if value_ordinal == 1 {
                    (record.payload.get(at..at + 2) == Some(&[0x00, 0x00])).then_some(())?;
                    at += 2;
                }
            }
            (record.payload.get(at..at + 4) == Some(&[0x00; 4])).then_some(())?;
            at += 4;
            let width = if record.payload.get(at) == Some(&0x01) {
                1
            } else {
                let (_, encoding, width) = payload_scalar(record.payload.get(at..)?)?;
                (encoding == PayloadScalarEncoding::Binary32).then_some(())?;
                width
            };
            at += width;
            (record.payload.get(at..at + 7) == Some(&[0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x03]))
                .then_some(())?;
            at += 7;
            let (CompactIndex::Value(_), width) = compact_index(record.payload.get(at..)?)? else {
                return None;
            };
            at += width;
            (record.payload.get(at) == Some(&0x01)).then_some(())?;
            (record.payload.get(at + 1) == Some(&ordinal)).then_some(())?;
            (record.payload.get(at + 2..at + 2 + ROW_TAIL.len()) == Some(&ROW_TAIL))
                .then_some(())?;
            at += 2 + ROW_TAIL.len();
        }
        let terminal_schema_index = row_schema_index.checked_sub(1)?;
        (record.payload.get(at) == Some(&terminal_schema_index)).then_some(())?;
        (record.payload.get(at + 1..at + 4) == Some(&[0x00, 0x00, 0x02])).then_some(())?;
        Some((declared_count, row_schema_index))
    };
    let decode = |start: usize| {
        let (declared_count, row_schema_index) = validate_scalar(start)?;
        let row_count = usize::from(declared_count - 1);
        let mut at = start + 2;
        let mut encodings = Vec::with_capacity(row_count);
        let mut values = Vec::with_capacity(row_count);
        let mut value_offsets = Vec::with_capacity(row_count);
        let mut raw_values = Vec::with_capacity(row_count);
        let mut selectors = Vec::with_capacity(row_count);
        let mut raw_selectors = Vec::with_capacity(row_count);
        let mut selector_offsets = Vec::with_capacity(row_count);
        for ordinal in 1..declared_count {
            (record.payload.get(at) == Some(&row_schema_index)).then_some(())?;
            (record.payload.get(at + 1..at + 1 + prefix_tail.len()) == Some(prefix_tail))
                .then_some(())?;
            at += 1 + prefix_tail.len();
            let (value, actual_encoding, width) = payload_scalar(record.payload.get(at..)?)?;
            encodings.push(match actual_encoding {
                PayloadScalarEncoding::Zero => return None,
                PayloadScalarEncoding::Binary32 => PatternTransformEncoding::Binary32,
                PayloadScalarEncoding::Binary64 => PatternTransformEncoding::Binary64,
            });
            values.push(value);
            value_offsets.push(record.payload_offset + at);
            raw_values.push(record.payload.get(at..at + width)?.to_vec());
            at += width;
            (record.payload.get(at..at + scalar_suffix.len()) == Some(scalar_suffix))
                .then_some(())?;
            at += scalar_suffix.len();
            let selector_offset = at;
            let (selector, width) = compact_index(record.payload.get(at..)?)?;
            let CompactIndex::Value(selector) = selector else {
                return None;
            };
            selectors.push(selector);
            raw_selectors.push(record.payload[at..at + width].to_vec());
            selector_offsets.push(record.payload_offset + selector_offset);
            at += width;
            (record.payload.get(at) == Some(&0x01)).then_some(())?;
            (record.payload.get(at + 1) == Some(&ordinal)).then_some(())?;
            (record.payload.get(at + 2..at + 2 + ROW_TAIL.len()) == Some(&ROW_TAIL))
                .then_some(())?;
            at += 2 + ROW_TAIL.len();
        }
        let terminal_schema_index = row_schema_index.checked_sub(1)?;
        (record.payload.get(at) == Some(&terminal_schema_index)).then_some(())?;
        (record.payload.get(at + 1..at + 3) == Some(&[0x00, 0x00])).then_some(())?;
        (record.payload.get(at + 3) == Some(&0x01)).then_some(())?;
        Some(PatternPayloadTransformLane {
            offset: record.payload_offset + start,
            row_schema_index,
            layout: PatternTransformLayout::ScalarRows,
            declared_count,
            encodings,
            values,
            value_offsets,
            raw_values,
            selectors,
            raw_selectors,
            selector_offsets,
        })
    };
    let decode_wide = |start: usize| {
        let (declared_count, row_schema_index) = validate_wide(start)?;
        let row_count = usize::from(declared_count - 1);
        let mut at = start + 2;
        let mut encodings = Vec::with_capacity(row_count * 5);
        let mut values = Vec::with_capacity(row_count * 5);
        let mut value_offsets = Vec::with_capacity(row_count * 5);
        let mut raw_values = Vec::with_capacity(row_count * 5);
        let mut selectors = Vec::with_capacity(row_count);
        let mut raw_selectors = Vec::with_capacity(row_count);
        let mut selector_offsets = Vec::with_capacity(row_count);
        for ordinal in 1..declared_count {
            (record.payload.get(at) == Some(&row_schema_index)).then_some(())?;
            at += 1;
            for value_ordinal in 0..4 {
                let value_offset = at;
                let (value, encoding, width) = payload_scalar(record.payload.get(at..)?)?;
                (encoding == PayloadScalarEncoding::Binary64 && width == 8).then_some(())?;
                values.push(value);
                encodings.push(PatternTransformEncoding::Binary64);
                value_offsets.push(record.payload_offset + value_offset);
                raw_values.push(record.payload.get(at..at + width)?.to_vec());
                at += width;
                if value_ordinal == 1 {
                    (record.payload.get(at..at + 2) == Some(&[0x00, 0x00])).then_some(())?;
                    at += 2;
                }
            }
            (record.payload.get(at..at + 4) == Some(&[0x00; 4])).then_some(())?;
            at += 4;
            let terminal_value_offset = at;
            let (terminal_value, encoding, width) = if record.payload.get(at) == Some(&0x01) {
                (1.0, PatternTransformEncoding::ExactOne, 1)
            } else {
                let (value, encoding, width) = payload_scalar(record.payload.get(at..)?)?;
                let encoding = match encoding {
                    PayloadScalarEncoding::Binary32 => PatternTransformEncoding::Binary32,
                    _ => return None,
                };
                (value, encoding, width)
            };
            values.push(terminal_value);
            encodings.push(encoding);
            value_offsets.push(record.payload_offset + terminal_value_offset);
            raw_values.push(record.payload.get(at..at + width)?.to_vec());
            at += width;
            (record.payload.get(at..at + 7) == Some(&[0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x03]))
                .then_some(())?;
            at += 7;
            let selector_offset = at;
            let (selector, width) = compact_index(record.payload.get(at..)?)?;
            let CompactIndex::Value(selector) = selector else {
                return None;
            };
            selectors.push(selector);
            raw_selectors.push(record.payload.get(at..at + width)?.to_vec());
            selector_offsets.push(record.payload_offset + selector_offset);
            at += width;
            (record.payload.get(at) == Some(&0x01)).then_some(())?;
            (record.payload.get(at + 1) == Some(&ordinal)).then_some(())?;
            (record.payload.get(at + 2..at + 2 + ROW_TAIL.len()) == Some(&ROW_TAIL))
                .then_some(())?;
            at += 2 + ROW_TAIL.len();
        }
        let terminal_schema_index = row_schema_index.checked_sub(1)?;
        (record.payload.get(at) == Some(&terminal_schema_index)).then_some(())?;
        (record.payload.get(at + 1..at + 4) == Some(&[0x00, 0x00, 0x02])).then_some(())?;
        Some(PatternPayloadTransformLane {
            offset: record.payload_offset + start,
            row_schema_index,
            layout: PatternTransformLayout::WideRows,
            declared_count,
            encodings,
            values,
            value_offsets,
            raw_values,
            selectors,
            raw_selectors,
            selector_offsets,
        })
    };
    let mut matches = (0..record.payload.len().saturating_sub(1))
        .filter_map(decode)
        .collect::<Vec<_>>();
    matches.extend((0..record.payload.len().saturating_sub(1)).filter_map(decode_wide));
    let [lane] = matches.as_slice() else {
        return None;
    };
    Some(lane.clone())
}

/// Decode the unique exactly counted instance-output lane in a bounded payload.
pub fn multi_instance_output_payload_lane(
    record: OperationRecord<'_>,
) -> Option<MultiInstanceOutputPayloadLane> {
    const ENVELOPE: [u8; 10] = [0x3a, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x25, 0x01];
    const ROW_PREFIX: [u8; 7] = [0x26, 0x27, 0x01, 0x02, 0x65, 0x01, 0x02];
    const ROW_ORDINAL_MARKER: u8 = 0x28;
    const REFERENCE_PREFIX: [u8; 2] = [0x00, 0x3b];

    if record.label.value != "Multi Instance Output" {
        return None;
    }
    let validate = |start: usize| {
        (record.payload.get(start..start + ENVELOPE.len()) == Some(&ENVELOPE)).then_some(())?;
        let declared_count = *record.payload.get(start + ENVELOPE.len())?;
        (declared_count >= 2).then_some(())?;
        let mut at = start + ENVELOPE.len() + 1;
        let mut instance_count = 0;
        for expected_row_index in 2..=declared_count {
            (record.payload.get(at..at + ROW_PREFIX.len()) == Some(&ROW_PREFIX)).then_some(())?;
            at += ROW_PREFIX.len();
            let (CompactIndex::Value(_), width) = compact_index(record.payload.get(at..)?)? else {
                return None;
            };
            at += width;
            (record.payload.get(at) == Some(&ROW_ORDINAL_MARKER)).then_some(())?;
            let ordinal = *record.payload.get(at + 1)?;
            (ordinal >= 2).then_some(())?;
            instance_count = instance_count.max(ordinal);
            (record.payload.get(at + 2) == Some(&expected_row_index)).then_some(())?;
            at += 3;
        }
        (record.payload.get(at..at + REFERENCE_PREFIX.len()) == Some(&REFERENCE_PREFIX))
            .then_some(())?;
        at += REFERENCE_PREFIX.len();
        for _ in 1..instance_count {
            let (Some(_), end) = feature_object_index(record.payload, at)? else {
                return None;
            };
            at = end;
        }
        (record.payload.get(at..at + 2) == Some(&[0x01, instance_count])).then_some(())?;
        Some((declared_count, instance_count))
    };
    let decode = |start: usize| {
        let (declared_count, instance_count) = validate(start)?;
        let row_count = usize::from(declared_count - 1);
        let mut at = start + ENVELOPE.len() + 1;
        let mut selectors = Vec::with_capacity(row_count);
        let mut raw_selectors = Vec::with_capacity(row_count);
        let mut selector_offsets = Vec::with_capacity(row_count);
        let mut ordinals = Vec::with_capacity(row_count);
        let mut row_indices = Vec::with_capacity(row_count);
        for expected_row_index in 2..=declared_count {
            (record.payload.get(at..at + ROW_PREFIX.len()) == Some(&ROW_PREFIX)).then_some(())?;
            at += ROW_PREFIX.len();
            let selector_offset = at;
            let (selector, width) = compact_index(record.payload.get(at..)?)?;
            let CompactIndex::Value(selector) = selector else {
                return None;
            };
            selectors.push(selector);
            raw_selectors.push(record.payload[at..at + width].to_vec());
            selector_offsets.push(record.payload_offset + selector_offset);
            at += width;
            (record.payload.get(at) == Some(&ROW_ORDINAL_MARKER)).then_some(())?;
            let ordinal = *record.payload.get(at + 1)?;
            (ordinal >= 2).then_some(())?;
            ordinals.push(ordinal);
            (record.payload.get(at + 2) == Some(&expected_row_index)).then_some(())?;
            row_indices.push(expected_row_index);
            at += 3;
        }
        (ordinals.iter().copied().max() == Some(instance_count)).then_some(())?;
        let expected_ordinals = (2..=instance_count).collect::<Vec<_>>();
        let mut distinct_selectors = Vec::new();
        for selector in &selectors {
            if !distinct_selectors.contains(selector) {
                distinct_selectors.push(*selector);
            }
        }
        for selector in distinct_selectors {
            let actual = selectors
                .iter()
                .zip(&ordinals)
                .filter_map(|(candidate, ordinal)| (*candidate == selector).then_some(*ordinal))
                .collect::<Vec<_>>();
            (actual == expected_ordinals).then_some(())?;
        }
        (record.payload.get(at..at + REFERENCE_PREFIX.len()) == Some(&REFERENCE_PREFIX))
            .then_some(())?;
        at += REFERENCE_PREFIX.len();
        let mut trailing_references =
            Vec::with_capacity(usize::from(instance_count.saturating_sub(1)));
        for _ in 1..instance_count {
            let reference_offset = at;
            let (Some(object_index), end) = feature_object_index(record.payload, at)? else {
                return None;
            };
            trailing_references.push(PayloadObjectReference {
                offset: record.payload_offset + reference_offset,
                object_index,
                raw_object_index: record.payload[reference_offset..end].to_vec(),
            });
            at = end;
        }
        (record.payload.get(at..at + 2) == Some(&[0x01, instance_count])).then_some(())?;
        Some(MultiInstanceOutputPayloadLane {
            offset: record.payload_offset + start + 8,
            declared_count,
            selectors,
            raw_selectors,
            selector_offsets,
            ordinals,
            row_indices,
            instance_count,
            trailing_references,
        })
    };
    let matches = (0..=record.payload.len().saturating_sub(ENVELOPE.len()))
        .filter_map(decode)
        .collect::<Vec<_>>();
    let [lane] = matches.as_slice() else {
        return None;
    };
    Some(lane.clone())
}

/// Decode the unique exactly counted selector lane in an
/// `IDENTICAL INSTANCE OUTPUT` payload.
pub fn identical_instance_output_payload_lane(
    record: OperationRecord<'_>,
) -> Option<IdenticalInstanceOutputPayloadLane> {
    const ROW_MIDDLE: [u8; 2] = [0x01, 0x02];
    const SENTINEL: [u8; 7] = [0xe0, 0x7f, 0xff, 0xff, 0xff, 0x00, 0x00];

    if record.label.value != "IDENTICAL INSTANCE OUTPUT" {
        return None;
    }
    let validate = |start: usize| {
        let leading_schema_index = *record.payload.get(start)?;
        let count_schema_index = *record.payload.get(start + 1)?;
        (record.payload.get(start + 2) == Some(&0x01)).then_some(())?;
        let declared_count = *record.payload.get(start + 3)?;
        (declared_count >= 2).then_some(())?;
        let first_schema_index = count_schema_index.checked_add(1)?;
        let second_schema_index = count_schema_index.checked_add(2)?;
        let third_schema_index = count_schema_index.checked_add(3)?;
        let mut at = start + 4;
        for ordinal in 2..=declared_count {
            (record.payload.get(at) == Some(&first_schema_index)).then_some(())?;
            (record.payload.get(at + 1) == Some(&second_schema_index)).then_some(())?;
            (record.payload.get(at + 2..at + 4) == Some(&ROW_MIDDLE)).then_some(())?;
            (record.payload.get(at + 4) == Some(&third_schema_index)).then_some(())?;
            at += 5;
            let (CompactIndex::Value(_), width) = compact_index(record.payload.get(at..)?)? else {
                return None;
            };
            at += width;
            (record.payload.get(at) == Some(&0x00)).then_some(())?;
            (record.payload.get(at + 1) == Some(&ordinal)).then_some(())?;
            at += 2;
        }
        let terminal_count = declared_count.checked_add(1)?;
        (record.payload.get(at) == Some(&0x00)).then_some(())?;
        (record.payload.get(at + 1) == Some(&terminal_count)).then_some(())?;
        (record.payload.get(at + 2..at + 2 + SENTINEL.len()) == Some(&SENTINEL)).then_some(())?;
        Some((
            leading_schema_index,
            count_schema_index,
            declared_count,
            [first_schema_index, second_schema_index, third_schema_index],
        ))
    };
    let decode = |start: usize| {
        let (
            leading_schema_index,
            count_schema_index,
            declared_count,
            [first_schema_index, second_schema_index, third_schema_index],
        ) = validate(start)?;
        let mut at = start + 4;
        let mut selectors = Vec::with_capacity(usize::from(declared_count - 1));
        let mut raw_selectors = Vec::with_capacity(usize::from(declared_count - 1));
        let mut selector_offsets = Vec::with_capacity(usize::from(declared_count - 1));
        for ordinal in 2..=declared_count {
            (record.payload.get(at) == Some(&first_schema_index)).then_some(())?;
            (record.payload.get(at + 1) == Some(&second_schema_index)).then_some(())?;
            (record.payload.get(at + 2..at + 4) == Some(&ROW_MIDDLE)).then_some(())?;
            (record.payload.get(at + 4) == Some(&third_schema_index)).then_some(())?;
            at += 5;
            let selector_offset = at;
            let (selector, width) = compact_index(record.payload.get(at..)?)?;
            let CompactIndex::Value(selector) = selector else {
                return None;
            };
            selectors.push(selector);
            raw_selectors.push(record.payload[at..at + width].to_vec());
            selector_offsets.push(record.payload_offset + selector_offset);
            at += width;
            (record.payload.get(at) == Some(&0x00)).then_some(())?;
            (record.payload.get(at + 1) == Some(&ordinal)).then_some(())?;
            at += 2;
        }
        let terminal_count = declared_count.checked_add(1)?;
        (record.payload.get(at) == Some(&0x00)).then_some(())?;
        (record.payload.get(at + 1) == Some(&terminal_count)).then_some(())?;
        (record.payload.get(at + 2..at + 2 + SENTINEL.len()) == Some(&SENTINEL)).then_some(())?;
        Some(IdenticalInstanceOutputPayloadLane {
            offset: record.payload_offset + start,
            leading_schema_index,
            count_schema_index,
            row_schema_indices: [first_schema_index, second_schema_index, third_schema_index],
            declared_count,
            selectors,
            raw_selectors,
            selector_offsets,
        })
    };
    let matches = (0..record.payload.len().saturating_sub(3))
        .filter_map(decode)
        .collect::<Vec<_>>();
    let [lane] = matches.as_slice() else {
        return None;
    };
    Some(lane.clone())
}

/// Decode the exact leading construction header in a bounded `POINT` payload.
pub fn point_feature_payload_header(
    record: OperationRecord<'_>,
) -> Option<PointFeaturePayloadHeader> {
    const PREFIX: [u8; 7] = [0x72, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
    const REFERENCE_SUFFIX: [u8; 42] = [
        0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d, 0x01, 0x02, 0x01, 0x00, 0x00, 0x00, 0x89,
        0x02, 0x01, 0x01, 0x01, 0x00, 0xa5, 0x57, 0x95, 0x01, 0x00, 0x00, 0xff,
    ];
    const MODE_SUFFIX: [u8; 20] = [
        0xc0, 0x1f, 0xff, 0xfd, 0x01, 0x00, 0x00, 0x01, 0x01, 0x01, 0x03, 0x02, 0x01, 0x01, 0x01,
        0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    if record.label.value != "POINT" || record.payload.get(..PREFIX.len()) != Some(&PREFIX) {
        return None;
    }
    let mut at = PREFIX.len();
    let reference_offset = at;
    let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
    at += width;
    (record.payload.get(at..at + REFERENCE_SUFFIX.len()) == Some(&REFERENCE_SUFFIX))
        .then_some(())?;
    at += REFERENCE_SUFFIX.len();
    let mode = *record.payload.get(at)?;
    matches!(mode, 0x02 | 0x03).then_some(())?;
    at += 1;
    (record.payload.get(at..at + MODE_SUFFIX.len()) == Some(&MODE_SUFFIX)).then_some(())?;
    Some(PointFeaturePayloadHeader {
        reference: PayloadObjectReference {
            offset: record.payload_offset + reference_offset,
            object_index,
            raw_object_index: record.payload[reference_offset..reference_offset + width].to_vec(),
        },
        mode,
    })
}

/// Decode the exact cross-block scalar lane selected by a `POINT` header target.
pub fn point_feature_scalar_lane(
    preceding_block: &[u8],
    target_block: &[u8],
) -> Option<PointFeatureScalarLane> {
    const SUFFIX: [u8; 19] = [
        0x00, 0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86,
        0x02, 0x00, 0x01, 0x00,
    ];
    let preceding_start = preceding_block.len().checked_sub(3)?;
    (target_block.get(45..64) == Some(&SUFFIX)).then_some(())?;
    let mut lane = Vec::with_capacity(48);
    lane.extend_from_slice(&preceding_block[preceding_start..]);
    lane.extend_from_slice(target_block.get(..45)?);
    let raw_values: [[u8; 8]; 6] = lane
        .chunks_exact(8)
        .map(|bytes| bytes.try_into().expect("eight-byte chunk"))
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    let values = raw_values
        .iter()
        .map(|bytes| shifted_ieee_f64(bytes))
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    Some(PointFeatureScalarLane {
        values,
        raw_values,
        value_offsets: [
            preceding_start,
            preceding_block.len() + 5,
            preceding_block.len() + 13,
            preceding_block.len() + 21,
            preceding_block.len() + 29,
            preceding_block.len() + 37,
        ],
    })
}

/// Decode the unique exactly framed construction-reference graph in a bounded `DRAFT` payload.
pub fn draft_feature_payload_references(
    record: OperationRecord<'_>,
) -> Option<DraftFeaturePayloadReferenceField> {
    const PAYLOAD_PREFIX: [u8; 14] = [
        0x67, 0x00, 0x00, 0x01, 0x00, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 0x03,
    ];
    const GRAPH_PREFIX: [u8; 2] = [0x01, 0x02];
    const MIDDLE: [u8; 35] = [
        0x68, 0x2f, 0x70, 0x62, 0x4d, 0xd2, 0xf1, 0xa9, 0xfc, 0x03, 0x50, 0x44, 0x00, 0x00, 0x01,
        0x46, 0x8a, 0x2a, 0x01, 0xa3, 0x60, 0x10, 0x01, 0x01, 0x01, 0x04, 0x02, 0x01, 0x02, 0x01,
        0x00, 0x00, 0x00, 0x00, 0x01,
    ];
    if record.label.value != "DRAFT"
        || record.payload.get(..PAYLOAD_PREFIX.len()) != Some(&PAYLOAD_PREFIX)
    {
        return None;
    }
    let decode = |start: usize| {
        let mut at = start + GRAPH_PREFIX.len();
        let decode_reference = |at: &mut usize| {
            let offset = *at;
            let (object_index, width) = payload_object_index(record.payload.get(offset..)?)?;
            *at += width;
            Some(PayloadObjectReference {
                offset: record.payload_offset + offset,
                object_index,
                raw_object_index: record.payload[offset..offset + width].to_vec(),
            })
        };
        let first = decode_reference(&mut at)?;
        (record.payload.get(at..at + GRAPH_PREFIX.len()) == Some(&GRAPH_PREFIX)).then_some(())?;
        at += GRAPH_PREFIX.len();
        let second = decode_reference(&mut at)?;
        (record.payload.get(at..at + MIDDLE.len()) == Some(&MIDDLE)).then_some(())?;
        at += MIDDLE.len();
        let third = decode_reference(&mut at)?;
        (record.payload.get(at..at + 4) == Some(&[0xff, 0x00, 0x00, 0x00])).then_some(())?;
        at += 4;
        let fourth = decode_reference(&mut at)?;
        (record.payload.get(at) == Some(&0xff)).then_some(())?;
        Some(DraftFeaturePayloadReferenceField {
            references: [first, second, third, fourth],
        })
    };
    let matches = (PAYLOAD_PREFIX.len()..=record.payload.len().saturating_sub(GRAPH_PREFIX.len()))
        .filter(|&start| {
            record.payload.get(start..start + GRAPH_PREFIX.len()) == Some(&GRAPH_PREFIX)
        })
        .filter_map(decode)
        .collect::<Vec<_>>();
    let [field] = matches.as_slice() else {
        return None;
    };
    Some(field.clone())
}

/// Decode the exactly positioned counted compact-index lane preceding a `DRAFT` graph.
pub fn draft_feature_leading_index_lane(
    record: OperationRecord<'_>,
) -> Option<DraftFeatureLeadingIndexLane> {
    const PREFIX: [u8; 22] = [
        0x67, 0x00, 0x00, 0x01, 0x00, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 0x03, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ];
    if record.label.value != "DRAFT" || record.payload.get(..PREFIX.len()) != Some(&PREFIX) {
        return None;
    }
    let mut at = PREFIX.len();
    (record.payload.get(at) == Some(&0x01)).then_some(())?;
    let declared_count = *record.payload.get(at + 1)?;
    (declared_count >= 2).then_some(())?;
    at += 2;
    let indices_start = at;
    let mut scan_at = indices_start;
    for _ in 1..declared_count {
        let (CompactIndex::Value(_), width) = compact_index(record.payload.get(scan_at..)?)? else {
            return None;
        };
        scan_at += width;
    }
    (record.payload.get(scan_at..scan_at + 2) == Some(&[0x01, 0x02])).then_some(())?;

    let mut indices = Vec::with_capacity(usize::from(declared_count - 1));
    let mut raw_indices = Vec::with_capacity(usize::from(declared_count - 1));
    at = indices_start;
    for _ in 1..declared_count {
        let offset = at;
        let (CompactIndex::Value(value), width) = compact_index(record.payload.get(at..)?)? else {
            return None;
        };
        at += width;
        indices.push((value, record.payload_offset + offset));
        raw_indices.push(record.payload[offset..offset + width].to_vec());
    }
    Some(DraftFeatureLeadingIndexLane {
        declared_count,
        indices,
        raw_indices,
    })
}

/// Decode the complete end-anchored terminal lane in a bounded `DRAFT` payload.
pub fn draft_feature_terminal_lane(
    record: OperationRecord<'_>,
) -> Option<DraftFeatureTerminalLane> {
    const FIXED: [u8; 11] = [
        0x01, 0x03, 0x02, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00,
    ];
    if record.label.value != "DRAFT" {
        return None;
    }
    let mut matches = Vec::new();
    for start in 0..record.payload.len() {
        let mut at = start;
        let first_offset = at;
        if !record
            .payload
            .get(at)
            .is_some_and(|marker| (0x80..=0xfe).contains(marker))
        {
            continue;
        }
        let Some((CompactIndex::Value(first), first_width)) =
            record.payload.get(at..).and_then(compact_index)
        else {
            continue;
        };
        at += first_width;
        let second_offset = at;
        if !record
            .payload
            .get(at)
            .is_some_and(|marker| (0x80..=0xfe).contains(marker))
        {
            continue;
        }
        let Some((CompactIndex::Value(second), second_width)) =
            record.payload.get(at..).and_then(compact_index)
        else {
            continue;
        };
        at += second_width;
        if record.payload.get(at..at + FIXED.len()) != Some(&FIXED) {
            continue;
        }
        at += FIXED.len();
        let Some(tail) = record
            .payload
            .get(at..at + 3)
            .and_then(|bytes| bytes.try_into().ok())
        else {
            continue;
        };
        at += 3;
        if at + 1 != record.payload.len() || record.payload.get(at) != Some(&0x00) {
            continue;
        }
        matches.push(DraftFeatureTerminalLane {
            indices: [first, second],
            raw_indices: [
                record.payload[first_offset..first_offset + first_width]
                    .try_into()
                    .ok()?,
                record.payload[second_offset..second_offset + second_width]
                    .try_into()
                    .ok()?,
            ],
            index_offsets: [
                record.payload_offset + first_offset,
                record.payload_offset + second_offset,
            ],
            tail,
            offset: record.payload_offset + start,
        });
    }
    let [lane] = matches.as_slice() else {
        return None;
    };
    Some(lane.clone())
}

/// Decode the exact common construction-reference envelope in a bounded
/// `SKIN` or `Studio Surface` payload.
pub fn surface_feature_payload_references(
    record: OperationRecord<'_>,
) -> Option<SurfaceFeaturePayloadReferenceField> {
    const HEADER_PREFIX: [u8; 4] = [0x00, 0x00, 0x01, 0x00];
    const TRAILING_PREFIX: [u8; 10] = [0x03, 0x03, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b];
    const TRAILING_SUFFIX: [u8; 17] = [
        0x01, 0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x01, 0x02,
    ];
    let discriminator = *record.payload.first()?;
    match record.label.value {
        "SKIN" if matches!(discriminator, 0x3e | 0x3f) => {}
        "Studio Surface" if discriminator == 0x14 => {}
        _ => return None,
    }
    (record.payload.get(1..5) == Some(&HEADER_PREFIX)).then_some(())?;
    let decode_reference = |at: &mut usize| {
        let offset = *at;
        let (object_index, width) = payload_object_index(record.payload.get(offset..)?)?;
        *at += width;
        Some(PayloadObjectReference {
            offset: record.payload_offset + offset,
            object_index,
            raw_object_index: record.payload[offset..offset + width].to_vec(),
        })
    };
    let mut at = 5;
    let mut references = Vec::with_capacity(14);
    for _ in 0..3 {
        references.push(decode_reference(&mut at)?);
    }
    (record.payload.get(at..at + 2) == Some(&[0x01, 0x09])).then_some(())?;
    at += 2;
    record.payload.get(at..at + 8)?;
    at += 8;
    (record.payload.get(at..at + 2) == Some(&[0x01, 0x09])).then_some(())?;
    at += 2;
    for _ in 0..8 {
        references.push(decode_reference(&mut at)?);
    }

    let trailing_starts = record
        .payload
        .windows(TRAILING_PREFIX.len())
        .enumerate()
        .filter_map(|(start, bytes)| (bytes == TRAILING_PREFIX).then_some(start))
        .collect::<Vec<_>>();
    let [trailing_start] = trailing_starts.as_slice() else {
        return None;
    };
    at = trailing_start + TRAILING_PREFIX.len();
    for _ in 0..3 {
        references.push(decode_reference(&mut at)?);
    }
    (record.payload.get(at..at + TRAILING_SUFFIX.len()) == Some(&TRAILING_SUFFIX)).then_some(())?;
    Some(SurfaceFeaturePayloadReferenceField {
        references: references.try_into().ok()?,
    })
}

fn surface_feature_branch_paths(
    payload: &[u8],
    payload_offset: usize,
    at: usize,
    remaining: u8,
    terminator: &[u8],
) -> Vec<Vec<SurfaceFeaturePayloadBranch>> {
    if remaining == 0 || !matches!(payload.get(at), Some(0x16 | 0x40)) {
        return Vec::new();
    }
    let mode = payload[at];
    if payload.get(at + 1) != Some(&0x01) {
        return Vec::new();
    }
    let Some(declared_count @ 2..) = payload.get(at + 2).copied() else {
        return Vec::new();
    };
    let members_start = at + 3;
    let mut cursor = members_start;
    for _ in 1..declared_count {
        let Some((_, width)) = payload.get(cursor..).and_then(payload_object_index) else {
            return Vec::new();
        };
        cursor += width;
    }
    let witnessed = payload.get(cursor..cursor + 2) == Some(&[0x01, declared_count]);
    if witnessed {
        cursor += 2;
    }
    let zero_count = if witnessed {
        usize::from(declared_count) + 3
    } else {
        5
    };
    let Some(zero_lane) = payload.get(cursor..cursor + zero_count) else {
        return Vec::new();
    };
    if !zero_lane.iter().all(|&byte| byte == 0) {
        return Vec::new();
    }
    cursor += zero_count;
    if payload.get(cursor..cursor + 3) != Some(&[0xff, 0x01, 0x02]) {
        return Vec::new();
    }
    cursor += 3;
    let Some((object_index, width)) = payload.get(cursor..).and_then(payload_object_index) else {
        return Vec::new();
    };
    let terminal_offset = cursor;
    let terminal_width = width;
    cursor += width;
    if payload.get(cursor) != Some(&0x00) {
        return Vec::new();
    }
    cursor += 1;

    let mut paths = Vec::new();
    for suffix_len in 1..=5 {
        let Some(suffix) = payload.get(cursor..cursor + suffix_len) else {
            continue;
        };
        let next = cursor + suffix_len;
        let continuations = if remaining == 1 {
            (payload.get(next..next + terminator.len()) == Some(terminator))
                .then_some(Vec::new())
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            surface_feature_branch_paths(payload, payload_offset, next, remaining - 1, terminator)
        };
        if continuations.is_empty() {
            continue;
        }
        let mut members = Vec::with_capacity(usize::from(declared_count) - 1);
        let mut member_cursor = members_start;
        for _ in 1..declared_count {
            let Some((object_index, width)) =
                payload.get(member_cursor..).and_then(payload_object_index)
            else {
                return Vec::new();
            };
            members.push(PayloadObjectReference {
                offset: payload_offset + member_cursor,
                object_index,
                raw_object_index: payload[member_cursor..member_cursor + width].to_vec(),
            });
            member_cursor += width;
        }
        let terminal = PayloadObjectReference {
            offset: payload_offset + terminal_offset,
            object_index,
            raw_object_index: payload[terminal_offset..terminal_offset + terminal_width].to_vec(),
        };
        for mut continuation in continuations {
            let branch = SurfaceFeaturePayloadBranch {
                offset: payload_offset + at,
                mode,
                declared_count,
                witnessed,
                members: members.clone(),
                terminal: terminal.clone(),
                suffix: suffix.to_vec(),
            };
            continuation.insert(0, branch);
            paths.push(continuation);
            if paths.len() == 2 {
                return paths;
            }
        }
    }
    paths
}

/// Decode the unique exactly framed counted branch group in a bounded `SKIN`
/// or `Studio Surface` payload.
pub fn surface_feature_payload_branches(
    record: OperationRecord<'_>,
) -> Option<SurfaceFeaturePayloadBranches> {
    const SKIN_TERMINATOR: [u8; 11] = [
        0x00, 0x00, 0x00, 0x01, 0x03, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01,
    ];
    const STUDIO_TERMINATOR: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01];
    let terminator = match record.label.value {
        "SKIN" => &SKIN_TERMINATOR[..],
        "Studio Surface" => &STUDIO_TERMINATOR[..],
        _ => return None,
    };
    let mut matches = Vec::new();
    for start in 0..record.payload.len().saturating_sub(6) {
        if record.payload.get(start..start + 2) != Some(&[0xa0, 0x5a]) {
            continue;
        }
        let Some(family @ (0x14 | 0x50)) = record.payload.get(start + 2).copied() else {
            continue;
        };
        let Some(header_code) = record.payload.get(start + 3).copied() else {
            continue;
        };
        if record.payload.get(start + 4) != Some(&0x01) {
            continue;
        }
        let Some(declared_group_count @ 1..) = record.payload.get(start + 5).copied() else {
            continue;
        };
        let paths = surface_feature_branch_paths(
            record.payload,
            record.payload_offset,
            start + 6,
            declared_group_count,
            terminator,
        );
        let [branches] = paths.as_slice() else {
            continue;
        };
        matches.push(SurfaceFeaturePayloadBranches {
            family,
            header_code,
            branches: branches.clone(),
        });
    }
    let [group] = matches.as_slice() else {
        return None;
    };
    Some(group.clone())
}

/// Decode the unique witnessed profile-reference field in an `EXTRUDE` payload.
pub fn extrude_profile_references(
    record: OperationRecord<'_>,
) -> Option<ExtrudeProfileReferenceField> {
    if record.label.value != "EXTRUDE" {
        return None;
    }
    let mut matches = Vec::new();
    for start in 0..record.payload.len().saturating_sub(6) {
        if record.payload.get(start..start + 4) != Some(&[0x01, 0x02, 0x16, 0x01]) {
            continue;
        }
        let Some(references) = extrude_profile_reference_field(record, start) else {
            continue;
        };
        matches.push(references);
    }
    let [references] = matches.as_slice() else {
        return None;
    };
    Some(references.clone())
}

/// Decode the fixed two-scalar header in a bounded `EXTRUDE` payload.
pub fn extrude_payload_header(record: OperationRecord<'_>) -> Option<ExtrudePayloadHeader> {
    if record.label.value != "EXTRUDE"
        || record.payload.get(..5) != Some(&[0x0f, 0x00, 0x00, 0x01, 0x00])
    {
        return None;
    }
    let raw_scalars = [
        <[u8; 8]>::try_from(record.payload.get(5..13)?).ok()?,
        <[u8; 8]>::try_from(record.payload.get(13..21)?).ok()?,
    ];
    Some(ExtrudePayloadHeader {
        offset: record.payload_offset + 5,
        scalars: [
            shifted_ieee_f64(&raw_scalars[0])?,
            shifted_ieee_f64(&raw_scalars[1])?,
        ],
        raw_scalars,
    })
}

/// Decode the unique terminal discriminator lane in a bounded operation payload.
pub fn operation_terminal_discriminator(
    record: OperationRecord<'_>,
) -> Option<OperationTerminalDiscriminator> {
    if record.payload.last() != Some(&0) {
        return None;
    }

    let decode = |start: usize| {
        if record.payload.get(start..start + 3) != Some(&[0x01, 0x01, 0x02]) {
            return None;
        }
        let mut at = start + 3;
        let mut type_tokens = [CompactToken {
            value: CompactIndex::Null,
            offset: 0,
            width: 0,
        }; 2];
        let mut type_indices = [0; 2];
        let mut type_index_offsets = [0; 2];
        for slot in 0..2 {
            let token = compact_value_token(record.payload, at)?;
            type_indices[slot] = match token.value {
                CompactIndex::Value(value) => value,
                CompactIndex::Null => return None,
            };
            type_index_offsets[slot] = record.payload_offset + at;
            type_tokens[slot] = token;
            at += token.width;
        }
        if record.payload.get(at..at + 4) != Some(&[0x01, 0x03, 0x02, 0x01]) {
            return None;
        }
        at += 4;
        let flags = record
            .payload
            .get(at..at + 4)
            .and_then(|bytes| bytes.try_into().ok())?;
        at += 4;
        if record.payload.get(at..at + 5) != Some(&[0x00, 0x00, 0x00, 0x29, 0x29]) {
            return None;
        }
        at += 5;

        let trailing_end = record.payload.len() - 1;
        let mut trailing_at = at;
        let mut trailing_count = 0;
        while trailing_at < trailing_end {
            let token = compact_value_token(record.payload, trailing_at)?;
            trailing_at += token.width;
            trailing_count += 1;
        }
        (trailing_at == trailing_end).then_some(())?;

        // Validate the complete trailing lane before allocating its owned
        // representation. A terminal candidate is tested at every payload
        // offset, so malformed prefixes must remain allocation-free.
        let mut trailing_indices = Vec::with_capacity(trailing_count);
        let mut raw_trailing_indices = Vec::with_capacity(trailing_count);
        let mut trailing_index_offsets = Vec::with_capacity(trailing_count);
        trailing_at = at;
        while trailing_at < trailing_end {
            let token = compact_value_token(record.payload, trailing_at)?;
            let value = match token.value {
                CompactIndex::Value(value) => value,
                CompactIndex::Null => return None,
            };
            trailing_indices.push(value);
            raw_trailing_indices.push(raw_compact_token(record.payload, token));
            trailing_index_offsets.push(record.payload_offset + trailing_at);
            trailing_at += token.width;
        }

        Some(OperationTerminalDiscriminator {
            offset: record.payload_offset + start,
            type_indices,
            raw_type_indices: std::array::from_fn(|slot| {
                raw_compact_token(record.payload, type_tokens[slot])
            }),
            type_index_offsets,
            flags,
            trailing_indices,
            raw_trailing_indices,
            trailing_index_offsets,
        })
    };

    let mut found = None;
    for start in 0..record.payload.len().saturating_sub(18) {
        let Some(lane) = decode(start) else {
            continue;
        };
        if found.is_some() {
            return None;
        }
        found = Some(lane);
    }
    found
}

fn shifted_ieee_f64(bytes: &[u8]) -> Option<f64> {
    let encoded: [u8; 8] = bytes.try_into().ok()?;
    let mut raw = encoded;
    raw[0] = raw[0].checked_add(0x10)?;
    let value = f64::from_be_bytes(raw);
    value.is_finite().then_some(value)
}

/// Decode complete three-scalar clauses following ordered operation body fields.
pub fn operation_body_scalar_triples(
    record: OperationRecord<'_>,
) -> Vec<OperationBodyScalarTriple> {
    operation_body_references(record)
        .into_iter()
        .enumerate()
        .filter_map(|(ordinal, reference)| {
            let token = reference.offset.checked_sub(record.offset)?;
            let (_, end) = feature_object_index(record.bytes, token)?;
            if record.bytes.get(end) != Some(&0xff) {
                return None;
            }
            let branch = *record.bytes.get(end + 1)?;
            let mut at = end + 2;
            let mut scalars = Vec::with_capacity(3);
            for _ in 0..3 {
                let (value, encoding, width) = payload_scalar(record.bytes.get(at..)?)?;
                scalars.push(PayloadScalar {
                    offset: record.offset + at,
                    value,
                    encoding,
                    raw_value: record.bytes.get(at..at + width)?.to_vec(),
                });
                at += width;
            }
            Some(OperationBodyScalarTriple {
                body_reference_ordinal: ordinal as u32,
                body_object_index: reference.object_index,
                branch,
                scalars: scalars.try_into().ok()?,
            })
        })
        .collect()
}

/// Decode wrapped member lanes following branch-`11` body scalar clauses.
pub fn operation_body_members(record: OperationRecord<'_>) -> Vec<OperationBodyMember> {
    operation_body_references(record)
        .into_iter()
        .enumerate()
        .flat_map(|(body_ordinal, reference)| {
            let Some(token) = reference.offset.checked_sub(record.offset) else {
                return Vec::new();
            };
            let Some((_, end)) = feature_object_index(record.bytes, token) else {
                return Vec::new();
            };
            if record.bytes.get(end..end + 2) != Some(&[0xff, 0x11]) {
                return Vec::new();
            }
            let mut at = end + 2;
            for _ in 0..3 {
                let Some((_, _, width)) = record.bytes.get(at..).and_then(payload_scalar) else {
                    return Vec::new();
                };
                at += width;
            }
            if record.bytes.get(at) != Some(&0x01) {
                return Vec::new();
            }
            let Some(count) = record.bytes.get(at + 1).copied().map(usize::from) else {
                return Vec::new();
            };
            if count < 2 {
                return Vec::new();
            }
            at += 2;
            let members_start = at;
            let mut scan_at = members_start;
            for _ in 0..count - 1 {
                if record.bytes.get(scan_at) != Some(&0x2e) {
                    return Vec::new();
                }
                scan_at += 1;
                let Some((CompactIndex::Value(_), width)) =
                    record.bytes.get(scan_at..).and_then(compact_index)
                else {
                    return Vec::new();
                };
                scan_at += width;
                if record.bytes.get(scan_at) != Some(&0x00) {
                    return Vec::new();
                }
                scan_at += 1;
            }

            at = members_start;
            let mut members = Vec::with_capacity(count - 1);
            for ordinal in 0..count - 1 {
                if record.bytes.get(at) != Some(&0x2e) {
                    return Vec::new();
                }
                at += 1;
                let member_at = at;
                let Some((CompactIndex::Value(member_index), width)) =
                    record.bytes.get(at..).and_then(compact_index)
                else {
                    return Vec::new();
                };
                at += width;
                if record.bytes.get(at) != Some(&0x00) {
                    return Vec::new();
                }
                at += 1;
                members.push(OperationBodyMember {
                    body_reference_ordinal: body_ordinal as u32,
                    body_object_index: reference.object_index,
                    ordinal: ordinal as u32,
                    member_index,
                    raw_member_index: record.bytes[member_at..member_at + width].to_vec(),
                    offset: record.offset + member_at,
                });
            }
            members
        })
        .collect()
}

/// Decode exact continuations following `TRIM BODY` branch-`11` member lanes.
pub fn operation_body_11_continuations(
    record: OperationRecord<'_>,
) -> Vec<OperationBody11Continuation> {
    if record.label.value != "TRIM BODY" {
        return Vec::new();
    }
    operation_body_references(record)
        .into_iter()
        .enumerate()
        .filter_map(|(body_ordinal, reference)| {
            let token = reference.offset.checked_sub(record.offset)?;
            let (_, end) = feature_object_index(record.bytes, token)?;
            if record.bytes.get(end..end + 2) != Some(&[0xff, 0x11]) {
                return None;
            }
            let mut at = end + 2;
            for _ in 0..3 {
                let (_, _, width) = payload_scalar(record.bytes.get(at..)?)?;
                at += width;
            }
            if record.bytes.get(at) != Some(&0x01) {
                return None;
            }
            let member_count = usize::from(*record.bytes.get(at + 1)?);
            if member_count < 2 {
                return None;
            }
            at += 2;
            for _ in 0..member_count - 1 {
                if record.bytes.get(at) != Some(&0x2e) {
                    return None;
                }
                at += 1;
                let (CompactIndex::Value(_), width) = compact_index(record.bytes.get(at..)?)?
                else {
                    return None;
                };
                at += width;
                if record.bytes.get(at) != Some(&0x00) {
                    return None;
                }
                at += 1;
            }
            if record.bytes.get(at..at + 2) != Some(&[0x01, 0x02]) {
                return None;
            }
            at += 2;
            let continuation_at = at;
            let (CompactIndex::Value(continuation_index), width) =
                compact_index(record.bytes.get(at..)?)?
            else {
                return None;
            };
            at += width;
            if record.bytes.get(at..at + 3) != Some(&[0x00, 0x00, 0x01]) {
                return None;
            }
            at += 3;
            let terminal_at = at;
            let (Some(terminal_object_index), next) = feature_object_index(record.bytes, at)?
            else {
                return None;
            };
            if record.bytes.get(next..next + 2) != Some(&[0x00, 0x00]) {
                return None;
            }
            Some(OperationBody11Continuation {
                body_reference_ordinal: body_ordinal as u32,
                body_object_index: reference.object_index,
                continuation_index,
                raw_continuation_index: record.bytes[continuation_at..continuation_at + width]
                    .to_vec(),
                continuation_offset: record.offset + continuation_at,
                terminal_object_index,
                raw_terminal_object_index: record.bytes[terminal_at..next].to_vec(),
                terminal_offset: record.offset + terminal_at,
            })
        })
        .collect()
}

/// Decode complete unwrapped counted reference lanes following body scalar clauses.
pub fn operation_body_reference_lanes(
    record: OperationRecord<'_>,
) -> Vec<OperationBodyReferenceLane> {
    operation_body_references(record)
        .into_iter()
        .enumerate()
        .filter_map(|(body_ordinal, reference)| {
            let token = reference.offset.checked_sub(record.offset)?;
            let (_, end) = feature_object_index(record.bytes, token)?;
            if record.bytes.get(end) != Some(&0xff) {
                return None;
            }
            let branch = *record.bytes.get(end + 1)?;
            if !matches!(branch, 0x11 | 0x1c) {
                return None;
            }
            let mut at = end + 2;
            for _ in 0..3 {
                let (_, _, width) = payload_scalar(record.bytes.get(at..)?)?;
                at += width;
            }
            if record.bytes.get(at) != Some(&0x01) {
                return None;
            }
            let count = usize::from(*record.bytes.get(at + 1)?);
            if count < 2 {
                return None;
            }
            at += 2;
            let compact = operation_body_reference_lane_values(
                record,
                at,
                count - 1,
                OperationBodyReferenceLaneEncoding::CompactIndex,
            );
            let objects = operation_body_reference_lane_values(
                record,
                at,
                count - 1,
                OperationBodyReferenceLaneEncoding::PayloadObjectIndex,
            );
            let (encoding, values) = match (compact, objects) {
                (Some(values), None) => (OperationBodyReferenceLaneEncoding::CompactIndex, values),
                (None, Some(values)) => (
                    OperationBodyReferenceLaneEncoding::PayloadObjectIndex,
                    values,
                ),
                _ => return None,
            };
            Some(OperationBodyReferenceLane {
                body_reference_ordinal: body_ordinal as u32,
                body_object_index: reference.object_index,
                branch,
                encoding,
                values,
            })
        })
        .collect()
}

fn operation_body_reference_lane_values(
    record: OperationRecord<'_>,
    at: usize,
    count: usize,
    encoding: OperationBodyReferenceLaneEncoding,
) -> Option<Vec<OperationBodyReferenceLaneValue>> {
    let mut scan_at = at;
    for _ in 0..count {
        let width = match encoding {
            OperationBodyReferenceLaneEncoding::CompactIndex => {
                let (CompactIndex::Value(_), width) = compact_index(record.bytes.get(scan_at..)?)?
                else {
                    return None;
                };
                width
            }
            OperationBodyReferenceLaneEncoding::PayloadObjectIndex => {
                payload_object_index(record.bytes.get(scan_at..)?)?.1
            }
        };
        scan_at += width;
    }
    (record.bytes.get(scan_at..scan_at + 4) == Some(&[0x00, 0x00, 0x0b, 0x00])).then_some(())?;

    let mut values = Vec::with_capacity(count);
    let mut at = at;
    for ordinal in 0..count {
        let value_at = at;
        let (object_index, width) = match encoding {
            OperationBodyReferenceLaneEncoding::CompactIndex => {
                let (CompactIndex::Value(value), width) = compact_index(record.bytes.get(at..)?)?
                else {
                    return None;
                };
                (value, width)
            }
            OperationBodyReferenceLaneEncoding::PayloadObjectIndex => {
                payload_object_index(record.bytes.get(at..)?)?
            }
        };
        at += width;
        values.push(OperationBodyReferenceLaneValue {
            ordinal: ordinal as u32,
            object_index,
            raw_value: record.bytes[value_at..value_at + width].to_vec(),
            offset: record.offset + value_at,
        });
    }
    Some(values)
}

/// Decode the structured `32` branch following an extrusion body field.
pub fn extrude_payload_32_branch(record: OperationRecord<'_>) -> Option<ExtrudePayload32Branch> {
    if record.label.value != "EXTRUDE" {
        return None;
    }
    let reference = operation_body_reference(record)?;
    let token = reference.offset.checked_sub(record.offset)?;
    let (_, end) = feature_object_index(record.bytes, token)?;
    if record.bytes.get(end..end + 4) != Some(&[0xff, 0x32, 0x00, 0x00]) {
        return None;
    }
    let branch_at = end + 1;
    let raw_scalar = <[u8; 8]>::try_from(record.bytes.get(end + 4..end + 12)?).ok()?;
    let scalar = shifted_ieee_f64(&raw_scalar)?;
    let mut at = end + 12;
    let (atoms_be, atom_offsets) = counted_u32_atoms(record.bytes, &mut at)?;
    let atom_indices = atoms_be
        .iter()
        .map(|atom| {
            let bytes = atom.to_be_bytes();
            if bytes[0] != 0x3d || bytes[3] != 0x00 || !(0x80..=0xfe).contains(&bytes[1]) {
                return None;
            }
            Some(u32::from(bytes[1] - 0x80) * 256 + u32::from(bytes[2]))
        })
        .collect::<Option<Vec<_>>>()?;
    let first = counted_compact_values(record.bytes, &mut at)?;
    let second = counted_compact_values(record.bytes, &mut at)?;
    if record.bytes.get(at..at + 2) != Some(&[0x00, 0x01]) {
        return None;
    }
    let (terminal_object_index, next) = feature_object_index(record.bytes, at + 2)?;
    let terminal_object_index = terminal_object_index?;
    if terminal_object_index != reference.object_index
        || record.bytes.get(next..next + 2) != Some(&[0x00, 0x00])
    {
        return None;
    }
    Some(ExtrudePayload32Branch {
        offset: record.offset + branch_at,
        body_object_index: reference.object_index,
        scalar,
        raw_scalar,
        atoms_be,
        atom_offsets: atom_offsets
            .into_iter()
            .map(|offset| record.offset + offset)
            .collect(),
        atom_indices,
        first_indices: first.values,
        raw_first_indices: first.raw_values,
        first_index_offsets: first
            .offsets
            .into_iter()
            .map(|offset| record.offset + offset)
            .collect(),
        second_indices: second.values,
        raw_second_indices: second.raw_values,
        second_index_offsets: second
            .offsets
            .into_iter()
            .map(|offset| record.offset + offset)
            .collect(),
        terminal_object_index,
        raw_terminal_object_index: record.bytes[at + 2..next].to_vec(),
        terminal_offset: record.offset + at + 2,
    })
}

/// Decode the ordered construction-reference field at the start of a `BLOCK` payload.
pub fn block_construction_references(
    record: OperationRecord<'_>,
) -> Option<BlockConstructionReferenceField> {
    const TRAILER: [u8; 15] = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
    ];
    if record.label.value != "BLOCK"
        || record.payload.get(1..6) != Some(&[0x00, 0x00, 0x01, 0x00, 0x00])
    {
        return None;
    }
    let mut at = 6usize;
    let mut references = Vec::with_capacity(19);
    for _ in 0..18 {
        let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
        references.push(PayloadObjectReference {
            offset: record.payload_offset + at,
            object_index,
            raw_object_index: record.payload[at..at + width].to_vec(),
        });
        at += width;
    }
    if record.payload.get(at) != Some(&0x01) {
        return None;
    }
    at += 1;
    let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
    references.push(PayloadObjectReference {
        offset: record.payload_offset + at,
        object_index,
        raw_object_index: record.payload[at..at + width].to_vec(),
    });
    at += width;
    if record.payload.get(at..at + TRAILER.len()) != Some(&TRAILER) {
        return None;
    }
    Some(BlockConstructionReferenceField {
        control: record.payload[0],
        references,
    })
}

/// Decode the fixed eight-reference construction lane at the start of a
/// `DATUM_CSYS` payload.
pub fn datum_csys_references(record: OperationRecord<'_>) -> Option<DatumCsysReferenceField> {
    const HEADER_SUFFIX: [u8; 13] = [
        0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ];
    const TRAILER: [u8; 8] = [0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00];
    if record.label.value != "DATUM_CSYS"
        || record.payload.get(1..1 + HEADER_SUFFIX.len()) != Some(&HEADER_SUFFIX)
    {
        return None;
    }
    let mut at = 1 + HEADER_SUFFIX.len();
    let mut references = Vec::with_capacity(8);
    for _ in 0..8 {
        let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
        references.push(PayloadObjectReference {
            offset: record.payload_offset + at,
            object_index,
            raw_object_index: record.payload[at..at + width].to_vec(),
        });
        at += width;
    }
    (record.payload.get(at..at + TRAILER.len()) == Some(&TRAILER)).then_some(())?;
    Some(DatumCsysReferenceField {
        control: record.payload[0],
        references: references.try_into().ok()?,
    })
}

/// Decode the common header of a bounded `DATUM_PLANE` payload.
pub fn datum_plane_payload_header(record: OperationRecord<'_>) -> Option<DatumPlanePayloadHeader> {
    const PREFIX: [u8; 5] = [0x00, 0x00, 0x01, 0x00, 0x01];
    if record.label.value != "DATUM_PLANE"
        || record.payload.get(1..6) != Some(&PREFIX)
        || record.payload.get(8..10) != Some(&[0x01, 0x02])
    {
        return None;
    }
    let declared_count = *record.payload.get(6)?;
    (declared_count >= 2).then_some(DatumPlanePayloadHeader {
        control: record.payload[0],
        declared_count,
        branch_tag: record.payload[7],
    })
}

/// Decode the count-two single-reference datum-plane construction branch.
pub fn datum_plane_single_reference_branch(
    record: OperationRecord<'_>,
) -> Option<DatumPlaneSingleReferenceBranch> {
    const SUFFIX: [u8; 12] = [
        0x00, 0x14, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00,
    ];
    let header = datum_plane_payload_header(record)?;
    if header.declared_count != 2 || !matches!(header.branch_tag, 0x1b | 0x23) {
        return None;
    }
    let mut at = 10;
    let descriptor_offset = record.payload_offset + at;
    let (CompactIndex::Value(descriptor_index), width) = compact_index(record.payload.get(at..)?)?
    else {
        return None;
    };
    let raw_descriptor_index = record.payload[at..at + width].to_vec();
    at += width;
    (record.payload.get(at) == Some(&0x01)).then_some(())?;
    at += 1;
    let object_offset = record.payload_offset + at;
    let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
    let raw_object_index = record.payload[at..at + width].to_vec();
    at += width;
    (record.payload.get(at..at + SUFFIX.len()) == Some(&SUFFIX)).then_some(())?;
    Some(DatumPlaneSingleReferenceBranch {
        descriptor_index,
        raw_descriptor_index,
        descriptor_offset,
        object_index,
        raw_object_index,
        object_offset,
    })
}

/// Decode any datum-plane branch carrying one descriptor and one object reference.
pub fn datum_plane_descriptor_reference_branch(
    record: OperationRecord<'_>,
) -> Option<DatumPlaneSingleReferenceBranch> {
    const SEPARATOR: [u8; 4] = [0x01, 0x29, 0x01, 0x02];
    const SUFFIX: [u8; 35] = [
        0x01, 0x01, 0x07, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    if let Some(branch) = datum_plane_single_reference_branch(record) {
        return Some(branch);
    }
    let header = datum_plane_payload_header(record)?;
    if header.declared_count != 3 || header.branch_tag != 0x28 {
        return None;
    }
    let mut at = 10;
    let descriptor_offset = record.payload_offset + at;
    let (CompactIndex::Value(descriptor_index), width) = compact_index(record.payload.get(at..)?)?
    else {
        return None;
    };
    let raw_descriptor_index = record.payload[at..at + width].to_vec();
    at += width;
    (record.payload.get(at..at + SEPARATOR.len()) == Some(&SEPARATOR)).then_some(())?;
    at += SEPARATOR.len();
    let object_offset = record.payload_offset + at;
    let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
    let raw_object_index = record.payload[at..at + width].to_vec();
    at += width;
    (record.payload.get(at..at + SUFFIX.len()) == Some(&SUFFIX)).then_some(())?;
    Some(DatumPlaneSingleReferenceBranch {
        descriptor_index,
        raw_descriptor_index,
        descriptor_offset,
        object_index,
        raw_object_index,
        object_offset,
    })
}

/// Decode either exact tag-`29` two-reference branch form.
pub fn datum_plane_double_reference_branch(
    record: OperationRecord<'_>,
) -> Option<DatumPlaneDoubleReferenceBranch> {
    const COUNT_TWO_MIDDLE: [u8; 11] = [
        0x01, 0x01, 0x18, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff,
    ];
    const COUNT_TWO_SUFFIX: [u8; 23] = [
        0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    const COUNT_THREE_MIDDLE: [u8; 5] = [0x01, 0x01, 0x3a, 0x01, 0x02];
    const COUNT_THREE_SUFFIX: [u8; 34] = [
        0x01, 0x17, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x0d,
    ];
    let header = datum_plane_payload_header(record)?;
    if header.branch_tag != 0x29 || !matches!(header.declared_count, 2 | 3) {
        return None;
    }
    let mut at = 10;
    let (first_index, first_width) = payload_object_index(record.payload.get(at..)?)?;
    let first = PayloadObjectReference {
        offset: record.payload_offset + at,
        object_index: first_index,
        raw_object_index: record.payload[at..at + first_width].to_vec(),
    };
    at += first_width;
    let middle = if header.declared_count == 2 {
        COUNT_TWO_MIDDLE.as_slice()
    } else {
        COUNT_THREE_MIDDLE.as_slice()
    };
    (record.payload.get(at..at + middle.len()) == Some(middle)).then_some(())?;
    at += middle.len();
    let (second_index, second_width) = payload_object_index(record.payload.get(at..)?)?;
    let second = PayloadObjectReference {
        offset: record.payload_offset + at,
        object_index: second_index,
        raw_object_index: record.payload[at..at + second_width].to_vec(),
    };
    at += second_width;
    let suffix = if header.declared_count == 2 {
        COUNT_TWO_SUFFIX.as_slice()
    } else {
        COUNT_THREE_SUFFIX.as_slice()
    };
    (record.payload.get(at..at + suffix.len()) == Some(suffix)).then_some(())?;
    Some(DatumPlaneDoubleReferenceBranch {
        references: [first, second],
    })
}

/// Decode unique datum-plane index lanes ending at the logical payload boundary.
pub fn datum_plane_object_index_lanes(bytes: &[u8]) -> Vec<DatumPlaneObjectIndexLane> {
    let mut lanes = Vec::new();
    for start in 0..bytes.len().saturating_sub(7) {
        if bytes[start] != 0x01 {
            continue;
        }
        let declared_count = bytes[start + 1];
        if declared_count < 2 {
            continue;
        }
        let indices_start = start + 2;
        let mut at = indices_start;
        let mut complete = true;
        for _ in 1..declared_count {
            let Some((CompactIndex::Value(_), width)) = bytes.get(at..).and_then(compact_index)
            else {
                complete = false;
                break;
            };
            at += width;
        }
        if !complete || bytes.get(at) != Some(&0x00) || at + 5 != bytes.len() {
            continue;
        }
        let Some(trailer) = View::u32_be_at(bytes, at + 1) else {
            continue;
        };
        let mut indices = Vec::with_capacity(usize::from(declared_count) - 1);
        let mut raw_indices = Vec::with_capacity(usize::from(declared_count) - 1);
        at = indices_start;
        for _ in 1..declared_count {
            let Some((CompactIndex::Value(value), width)) = bytes.get(at..).and_then(compact_index)
            else {
                complete = false;
                break;
            };
            indices.push((value, at));
            raw_indices.push(bytes[at..at + width].to_vec());
            at += width;
        }
        if !complete {
            continue;
        }
        lanes.push(DatumPlaneObjectIndexLane {
            offset: start,
            declared_count,
            indices,
            raw_indices,
            trailer,
        });
    }
    lanes
}

/// Decode every exactly framed scalar pair in a reconstructed datum-plane payload.
pub fn datum_plane_object_scalar_pairs(bytes: &[u8]) -> Vec<DatumPlaneObjectScalarPair> {
    const DISCRIMINATOR: [u8; 18] = [
        0x6d, 0x00, 0xf0, 0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86,
        0x02, 0x00, 0x03,
    ];
    bytes
        .windows(DISCRIMINATOR.len())
        .enumerate()
        .filter_map(|(offset, window)| {
            (window == DISCRIMINATOR).then_some(())?;
            let first = offset + DISCRIMINATOR.len();
            let second = first + 9;
            (bytes.get(first + 8) == Some(&0x00)).then_some(())?;
            Some(DatumPlaneObjectScalarPair {
                offset,
                values: [
                    shifted_ieee_f64(bytes.get(first..first + 8)?)?,
                    shifted_ieee_f64(bytes.get(second..second + 8)?)?,
                ],
                raw_values: [
                    bytes.get(first..first + 8)?.try_into().ok()?,
                    bytes.get(second..second + 8)?.try_into().ok()?,
                ],
                value_offsets: [first, second],
            })
        })
        .collect()
}

/// Decode one complete datum-plane descriptor block.
pub fn datum_plane_descriptor_block(bytes: &[u8]) -> Option<DatumPlaneDescriptorBlock> {
    if bytes.len() != 40 {
        return None;
    }
    let delimiter = bytes.iter().position(|byte| *byte == b'?')?;
    let identity = bytes.get(..delimiter)?;
    if identity.is_empty()
        || !identity
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return None;
    }
    let suffix = bytes.get(delimiter..)?;
    if suffix.get(..2) != Some(b"?A") {
        return None;
    }
    let (CompactIndex::Value(schema_index), width) = compact_index(suffix.get(2..)?)? else {
        return None;
    };
    let label_start = 2 + width + 3;
    if suffix.get(2 + width..label_start) != Some(&[0xff, 0x02, 0x01]) {
        return None;
    }
    let label = suffix.get(label_start..)?;
    if label.is_empty() || !label.iter().all(u8::is_ascii_graphic) {
        return None;
    }
    Some(DatumPlaneDescriptorBlock {
        identity: std::str::from_utf8(identity).ok()?.to_string(),
        suffix: suffix.to_vec(),
        schema_index,
        label: std::str::from_utf8(label).ok()?.to_string(),
    })
}

/// Decode every exactly framed scalar pair in a reconstructed object payload.
pub fn object_payload_scalar_pairs(bytes: &[u8]) -> Vec<ObjectPayloadScalarPair> {
    const SHORT: [u8; 15] = [
        0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
    ];
    const EXTENDED: [u8; 16] = [
        0x08, 0x02, 0x03, 0x01, 0x81, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00,
        0x03,
    ];
    let mut pairs = Vec::new();
    for discriminator in [SHORT.as_slice(), EXTENDED.as_slice()] {
        for (offset, window) in bytes.windows(discriminator.len()).enumerate() {
            if window != discriminator {
                continue;
            }
            let first = offset + discriminator.len();
            let second = first + 9;
            if bytes.get(first + 8) != Some(&0x00) {
                continue;
            }
            let Some(raw_values) = bytes
                .get(first..first + 8)
                .and_then(|value| <[u8; 8]>::try_from(value).ok())
                .zip(
                    bytes
                        .get(second..second + 8)
                        .and_then(|value| <[u8; 8]>::try_from(value).ok()),
                )
                .map(|(first, second)| [first, second])
            else {
                continue;
            };
            let Some(values) = shifted_ieee_f64(&raw_values[0])
                .zip(shifted_ieee_f64(&raw_values[1]))
                .map(|(first, second)| [first, second])
            else {
                continue;
            };
            pairs.push(ObjectPayloadScalarPair {
                offset,
                values,
                raw_values,
                value_offsets: [first, second],
                discriminator: discriminator.to_vec(),
            });
        }
    }
    pairs.sort_by_key(|pair| pair.offset);
    pairs
}

/// Decode the repeated-type scalar-pair lane in a reconstructed sketch payload.
pub fn sketch_payload_scalar_pairs(bytes: &[u8]) -> Vec<ObjectPayloadScalarPair> {
    const FRAME_SUFFIX: [u8; 14] = [
        0x00, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
    ];
    let mut pairs = object_payload_scalar_pairs(bytes);
    for (offset, window) in bytes.windows(3).enumerate() {
        let [type_code, repeated_type_code, 0x41] = window else {
            continue;
        };
        if *type_code == 0 || type_code != repeated_type_code {
            continue;
        }
        if offset == 0 || bytes.get(offset - 1) != Some(&0x00) {
            continue;
        }
        let discriminator_len = 3 + FRAME_SUFFIX.len();
        if bytes.get(offset + 3..offset + discriminator_len) != Some(&FRAME_SUFFIX) {
            continue;
        }
        let first = offset + discriminator_len;
        let second = first + 8;
        let Some(raw_values) = bytes
            .get(first..first + 8)
            .and_then(|value| <[u8; 8]>::try_from(value).ok())
            .zip(
                bytes
                    .get(second..second + 8)
                    .and_then(|value| <[u8; 8]>::try_from(value).ok()),
            )
            .map(|(first, second)| [first, second])
        else {
            continue;
        };
        let Some(values) = shifted_ieee_f64(&raw_values[0])
            .zip(shifted_ieee_f64(&raw_values[1]))
            .map(|(first, second)| [first, second])
        else {
            continue;
        };
        pairs.push(ObjectPayloadScalarPair {
            offset,
            values,
            raw_values,
            value_offsets: [first, second],
            discriminator: bytes[offset..offset + discriminator_len].to_vec(),
        });
    }
    pairs.sort_by_key(|pair| pair.offset);
    pairs
}

/// Decode every exactly framed signed Q1.55 pair in a reconstructed sketch payload.
pub fn sketch_payload_fixed_pairs(bytes: &[u8]) -> Vec<SketchPayloadFixedPair> {
    const LEGACY: [u8; 8] = [0x04, 0xe0, 0x48, 0x0e, 0x02, 0x03, 0x80, 0x84];
    const SHORT: [u8; 15] = [
        0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x01,
    ];
    const EXTENDED: [u8; 17] = [
        0x08, 0x02, 0x03, 0x01, 0xc0, 0x40, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02,
        0x00, 0x01,
    ];
    const THREE_MEMBER: [u8; 15] = [
        0x0b, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
    ];
    let mut pairs = Vec::new();
    for (discriminator, separator_width) in [
        (LEGACY.as_slice(), 1usize),
        (SHORT.as_slice(), 0),
        (EXTENDED.as_slice(), 0),
        (THREE_MEMBER.as_slice(), 1),
    ] {
        for (offset, window) in bytes.windows(discriminator.len()).enumerate() {
            if window != discriminator {
                continue;
            }
            let first = offset + discriminator.len();
            let second = first + 8 + separator_width;
            if bytes.get(first) != Some(&0x30)
                || (separator_width != 0 && bytes.get(first + 8) != Some(&0x00))
                || bytes.get(second) != Some(&0x30)
            {
                continue;
            }
            let Some(first_raw) = bytes
                .get(first + 1..first + 8)
                .and_then(|raw| raw.try_into().ok())
            else {
                continue;
            };
            let Some(second_raw) = bytes
                .get(second + 1..second + 8)
                .and_then(|raw| raw.try_into().ok())
            else {
                continue;
            };
            pairs.push(SketchPayloadFixedPair {
                offset,
                values: [decode_q1_55(first_raw), decode_q1_55(second_raw)],
                value_offsets: [first, second],
                raw_values: [first_raw, second_raw],
                discriminator: discriminator.to_vec(),
            });
        }
    }
    pairs.sort_by_key(|pair| pair.offset);
    pairs
}

/// Decode every exactly framed mixed Q1.55/binary32 pair in a sketch payload.
pub fn sketch_payload_mixed_pairs(bytes: &[u8]) -> Vec<SketchPayloadMixedPair> {
    const DISCRIMINATOR: [u8; 8] = [0x04, 0xe0, 0x48, 0x0e, 0x02, 0x03, 0x80, 0x84];
    let mut pairs = Vec::new();
    for (offset, window) in bytes.windows(DISCRIMINATOR.len()).enumerate() {
        if window != DISCRIMINATOR {
            continue;
        }
        let fixed_offset = offset + DISCRIMINATOR.len();
        let binary32_offset = fixed_offset + 9;
        if bytes.get(fixed_offset) != Some(&0x30) || bytes.get(fixed_offset + 8) != Some(&0x00) {
            continue;
        }
        let Some(fixed_raw_value) = bytes
            .get(fixed_offset + 1..fixed_offset + 8)
            .and_then(|raw| raw.try_into().ok())
        else {
            continue;
        };
        let Some(binary32_raw_value): Option<[u8; 4]> = bytes
            .get(binary32_offset..binary32_offset + 4)
            .and_then(|raw| raw.try_into().ok())
        else {
            continue;
        };
        let Some((binary32_value, PayloadScalarEncoding::Binary32, 4)) =
            payload_scalar(&binary32_raw_value)
        else {
            continue;
        };
        pairs.push(SketchPayloadMixedPair {
            offset,
            fixed_value: decode_q1_55(fixed_raw_value),
            binary32_value,
            fixed_raw_value,
            binary32_raw_value,
            value_offsets: [fixed_offset, binary32_offset],
            discriminator: DISCRIMINATOR.to_vec(),
        });
    }
    pairs
}

/// Decode every exactly framed signed Q1.55 pair in a datum-CSYS payload.
pub fn datum_csys_payload_fixed_pairs(bytes: &[u8]) -> Vec<DatumCsysPayloadFixedPair> {
    const DISCRIMINATORS: [&[u8]; 2] = [
        &[
            0x0b, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00,
            0x03,
        ],
        &[
            0x80, 0x8d, 0x00, 0xff, 0x80, 0x81, 0x01, 0x02, 0x01, 0x00, 0x00, 0x00, 0x87, 0xd7,
            0x01, 0x01, 0x01, 0x01, 0x02, 0xa5, 0x30, 0x21, 0xa5, 0x30, 0x21, 0x01, 0x00, 0x01,
            0xaf, 0xff, 0xdf, 0x02, 0x01, 0x02,
        ],
    ];
    let mut pairs = Vec::new();
    for discriminator in DISCRIMINATORS {
        for (offset, window) in bytes.windows(discriminator.len()).enumerate() {
            if window != discriminator {
                continue;
            }
            let first = offset + discriminator.len();
            let second = first + 9;
            if bytes.get(first) != Some(&0x30)
                || bytes.get(first + 8) != Some(&0x00)
                || bytes.get(second) != Some(&0x30)
            {
                continue;
            }
            let Some(first_raw) = bytes
                .get(first + 1..first + 8)
                .and_then(|raw| raw.try_into().ok())
            else {
                continue;
            };
            let Some(second_raw) = bytes
                .get(second + 1..second + 8)
                .and_then(|raw| raw.try_into().ok())
            else {
                continue;
            };
            pairs.push(DatumCsysPayloadFixedPair {
                offset,
                values: [decode_q1_55(first_raw), decode_q1_55(second_raw)],
                value_offsets: [first, second],
                raw_values: [first_raw, second_raw],
                discriminator: discriminator.to_vec(),
            });
        }
    }
    pairs.sort_by_key(|pair| pair.offset);
    pairs
}

fn decode_q1_55(raw: [u8; 7]) -> f64 {
    let unsigned = raw
        .into_iter()
        .fold(0_u64, |value, byte| (value << 8) | u64::from(byte));
    let signed = if unsigned & (1_u64 << 55) == 0 {
        unsigned as i64
    } else {
        (unsigned as i64) - (1_i64 << 56)
    };
    signed as f64 / (1_u64 << 55) as f64
}

/// Decode every complete signed Q1.55 lane in a reconstructed draft graph payload.
pub fn draft_construction_fixed_lanes(bytes: &[u8]) -> Vec<DraftConstructionFixedLane> {
    const DISCRIMINATOR: [u8; 18] = [
        0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86, 0x02,
        0x00, 0x01, 0x00,
    ];
    bytes
        .windows(DISCRIMINATOR.len())
        .enumerate()
        .filter_map(|(offset, window)| {
            (window == DISCRIMINATOR).then_some(())?;
            let mut at = offset + DISCRIMINATOR.len();
            let mut markers = Vec::new();
            let mut raw_values = Vec::new();
            let mut value_offsets = Vec::new();
            while matches!(bytes.get(at), Some(0x30 | 0xb0)) {
                let raw = bytes.get(at + 1..at + 8)?.try_into().ok()?;
                markers.push(bytes[at]);
                raw_values.push(raw);
                value_offsets.push(at);
                at += 8;
            }
            if raw_values.is_empty() || bytes.get(at) != Some(&0x00) {
                return None;
            }
            let values = raw_values.iter().copied().map(decode_q1_55).collect();
            Some(DraftConstructionFixedLane {
                offset,
                values,
                markers,
                raw_values,
                value_offsets,
            })
        })
        .collect()
}

/// Decode every complete shifted-binary32 lane in a reconstructed draft graph payload.
pub fn draft_construction_binary32_lanes(bytes: &[u8]) -> Vec<DraftConstructionBinary32Lane> {
    const BRANCH_04: [u8; 18] = [
        0x90, 0x18, 0x45, 0x01, 0x04, 0x01, 0x04, 0x01, 0xc0, 0x45, 0x04, 0x04, 0x80, 0x86, 0x02,
        0x00, 0x03, 0x00,
    ];
    const BRANCH_03: [u8; 18] = [
        0x90, 0x18, 0x45, 0x01, 0x04, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02,
        0x00, 0x03, 0x00,
    ];
    let mut lanes = [BRANCH_04, BRANCH_03]
        .into_iter()
        .flat_map(|discriminator| {
            bytes
                .windows(discriminator.len())
                .enumerate()
                .filter_map(move |(offset, window)| {
                    (window == discriminator).then_some(())?;
                    let mut at = offset + discriminator.len();
                    let mut values = Vec::new();
                    let mut raw_values = Vec::new();
                    let mut value_offsets = Vec::new();
                    while matches!(bytes.get(at), Some(0x40..=0x5f | 0xc0..=0xdf)) {
                        let raw: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
                        let Some((value, PayloadScalarEncoding::Binary32, 4)) =
                            payload_scalar(&raw)
                        else {
                            return None;
                        };
                        values.push(value);
                        raw_values.push(raw);
                        value_offsets.push(at);
                        at += 4;
                    }
                    if values.is_empty() || bytes.get(at) != Some(&0x00) {
                        return None;
                    }
                    Some(DraftConstructionBinary32Lane {
                        offset,
                        discriminator,
                        branch: discriminator[6],
                        values,
                        raw_values,
                        value_offsets,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    lanes.sort_by_key(|lane| lane.offset);
    lanes
}

/// Decode a bounded datum-CSYS descriptor containing one unique maximal identity run.
pub fn datum_csys_descriptor_block(bytes: &[u8]) -> Option<DatumCsysDescriptorBlock> {
    let mut matches = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if !(bytes[at].is_ascii_digit() || (b'a'..=b'f').contains(&bytes[at])) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && (bytes[at].is_ascii_digit() || (b'a'..=b'f').contains(&bytes[at]))
        {
            at += 1;
        }
        if (30..=32).contains(&(at - start)) {
            matches.push((start, at));
        }
    }
    let [(start, end)] = matches.as_slice() else {
        return None;
    };
    Some(DatumCsysDescriptorBlock {
        prefix: bytes[..*start].to_vec(),
        identity: std::str::from_utf8(&bytes[*start..*end]).ok()?.to_string(),
        suffix: bytes[*end..].to_vec(),
        identity_offset: *start,
    })
}

/// Decode every complete identity frame in a reconstructed draft construction payload.
pub fn draft_construction_identity_frames(bytes: &[u8]) -> Vec<DraftConstructionIdentityFrame> {
    let mut frames = Vec::new();
    for offset in 0..bytes.len() {
        if bytes[offset] != 0x41 {
            continue;
        }
        let Some((prefix_len, form)) = draft_identity_prefix(&bytes[offset..]) else {
            continue;
        };
        let identity_start = offset + prefix_len;
        let mut identity_end = identity_start;
        while bytes
            .get(identity_end)
            .is_some_and(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            identity_end += 1;
        }
        if identity_end == identity_start || bytes.get(identity_end) != Some(&b'?') {
            continue;
        }
        frames.push(DraftConstructionIdentityFrame {
            offset,
            prefix: bytes[offset..offset + prefix_len].to_vec(),
            form,
            identity: String::from_utf8(bytes[identity_start..identity_end].to_vec())
                .expect("lowercase hexadecimal bytes are UTF-8"),
            identity_offset: identity_start,
        });
    }
    frames
}

fn draft_identity_prefix(bytes: &[u8]) -> Option<(usize, DraftConstructionIdentityFrameForm)> {
    if bytes.first() != Some(&0x41) {
        return None;
    }
    if bytes.get(1) == Some(&0xf0) {
        let (index, index_width) = compact_index(bytes.get(2..)?)?;
        let end = 2 + index_width + 3;
        (bytes.get(2 + index_width..end) == Some(&[0xff, 0x02, 0x01])).then_some((
            end,
            DraftConstructionIdentityFrameForm::Tagged {
                index: compact_index_value(index),
            },
        ))
    } else {
        let (CompactIndex::Value(first_index), first_width) = compact_index(bytes.get(1..)?)?
        else {
            return None;
        };
        let second_at = 1 + first_width + 1;
        if bytes.get(1 + first_width) != Some(&0xf0) {
            return None;
        }
        let (second_index, second_width) = compact_index(bytes.get(second_at..)?)?;
        let branch_at = second_at + second_width;
        let end = branch_at + 2;
        (matches!(bytes.get(branch_at), Some(0x02 | 0x03))
            && bytes.get(branch_at + 1) == Some(&0x01))
        .then_some((
            end,
            DraftConstructionIdentityFrameForm::IndexedBranch {
                first_index,
                second_index: compact_index_value(second_index),
                branch: bytes[branch_at],
            },
        ))
    }
}

fn compact_index_value(index: CompactIndex) -> Option<u32> {
    match index {
        CompactIndex::Null => None,
        CompactIndex::Value(value) => Some(value),
    }
}

/// Decode compact object IDs followed by their complete frame discriminator.
pub fn data_block_object_frames(bytes: &[u8]) -> Vec<DataBlockObjectFrame> {
    const DISCRIMINATOR: [u8; 18] = [
        0x00, 0x72, 0x01, 0xc0, 0x20, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x01,
        0x02, 0x80, 0xa4,
    ];
    let mut references = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let Some((CompactIndex::Value(object_id), width)) = compact_index(&bytes[offset..]) else {
            offset += 1;
            continue;
        };
        if bytes.get(offset + width..offset + width + DISCRIMINATOR.len()) != Some(&DISCRIMINATOR) {
            offset += 1;
            continue;
        }
        references.push(DataBlockObjectFrame {
            object_id,
            raw_object_id: bytes[offset..offset + width].to_vec(),
            offset,
        });
        offset += width + DISCRIMINATOR.len();
    }
    references
}

fn counted_u32_atoms(bytes: &[u8], at: &mut usize) -> Option<(Vec<u32>, Vec<usize>)> {
    if bytes.get(*at) != Some(&0x01) {
        return None;
    }
    let count = usize::from(*bytes.get(*at + 1)?);
    if count < 2 {
        return None;
    }
    *at += 2;
    let values_start = *at;
    let mut scan_at = values_start;
    for _ in 1..count {
        View::u32_be_at(bytes, scan_at)?;
        scan_at += 4;
    }

    let mut values = Vec::with_capacity(count - 1);
    let mut offsets = Vec::with_capacity(count - 1);
    *at = values_start;
    for _ in 1..count {
        offsets.push(*at);
        values.push(View::u32_be_at(bytes, *at)?);
        *at += 4;
    }
    Some((values, offsets))
}

struct CountedCompactValues {
    values: Vec<u32>,
    raw_values: Vec<Vec<u8>>,
    offsets: Vec<usize>,
}

fn counted_compact_values(bytes: &[u8], at: &mut usize) -> Option<CountedCompactValues> {
    if bytes.get(*at) != Some(&0x01) {
        return None;
    }
    let count = usize::from(*bytes.get(*at + 1)?);
    if count < 2 {
        return None;
    }
    *at += 2;
    let values_start = *at;
    let mut scan_at = values_start;
    for _ in 1..count {
        let (CompactIndex::Value(_), width) = compact_index(bytes.get(scan_at..)?)? else {
            return None;
        };
        scan_at += width;
    }

    let mut values = Vec::with_capacity(count - 1);
    let mut raw_values = Vec::with_capacity(count - 1);
    let mut offsets = Vec::with_capacity(count - 1);
    *at = values_start;
    for _ in 1..count {
        let value_at = *at;
        let (CompactIndex::Value(value), width) = compact_index(bytes.get(*at..)?)? else {
            return None;
        };
        values.push(value);
        raw_values.push(bytes[value_at..value_at + width].to_vec());
        offsets.push(value_at);
        *at += width;
    }
    Some(CountedCompactValues {
        values,
        raw_values,
        offsets,
    })
}

fn payload_scalar(bytes: &[u8]) -> Option<(f64, PayloadScalarEncoding, usize)> {
    let marker = *bytes.first()?;
    match marker {
        0x00 => Some((0.0, PayloadScalarEncoding::Zero, 1)),
        0x20..=0x3f | 0xa0..=0xbf => Some((
            shifted_ieee_f64(bytes.get(..8)?)?,
            PayloadScalarEncoding::Binary64,
            8,
        )),
        0x40..=0x5f | 0xc0..=0xdf => {
            let encoded: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
            let mut raw = encoded;
            raw[0] = raw[0].checked_sub(0x10)?;
            let value = f32::from_be_bytes(raw);
            value
                .is_finite()
                .then_some((f64::from(value), PayloadScalarEncoding::Binary32, 4))
        }
        _ => None,
    }
}

fn extrude_profile_reference_field(
    record: OperationRecord<'_>,
    start: usize,
) -> Option<ExtrudeProfileReferenceField> {
    let count = *record.payload.get(start + 4)?;
    if count < 2 {
        return None;
    }
    let references_start = start + 5;
    let mut scan_at = references_start;
    for _ in 1..count {
        let (_, width) = payload_object_index(record.payload.get(scan_at..)?)?;
        scan_at += width;
    }
    if record.payload.get(scan_at..scan_at + 3) != Some(&[0x01, 0x03, 0x79]) {
        return None;
    }

    let mut at = references_start;
    let mut references = Vec::with_capacity(usize::from(count - 1));
    for _ in 1..count {
        let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
        references.push(PayloadObjectReference {
            offset: record.payload_offset + at,
            object_index,
            raw_object_index: record.payload[at..at + width].to_vec(),
        });
        at += width;
    }
    let encoded_references = record.payload.get(references_start..at)?;
    let witness_len = 2 + encoded_references.len() + 2;
    let witness_count = record
        .payload
        .windows(witness_len)
        .filter(|candidate| {
            candidate.starts_with(&[0x01, count])
                && candidate.get(2..2 + encoded_references.len()) == Some(encoded_references)
                && candidate.ends_with(&[0x00, 0x00])
        })
        .count();
    Some(ExtrudeProfileReferenceField {
        references,
        witnessed: witness_count == 1,
    })
}

/// Decode the unique `04, length, p<decimal>[_qualifier], 00` declaration name.
pub fn expression_declaration_name(bytes: &[u8]) -> Option<ExpressionDeclarationName<'_>> {
    let mut matches = Vec::new();
    let mut literals = Vec::new();
    for at in 0..bytes.len().saturating_sub(4) {
        if bytes[at] != 0x04 {
            continue;
        }
        let declared = usize::from(bytes[at + 1]);
        if declared < 4 {
            continue;
        }
        let Some(end) = at.checked_add(declared) else {
            continue;
        };
        let Some(raw) = bytes.get(at + 2..end) else {
            continue;
        };
        if bytes.get(end) != Some(&0) {
            continue;
        }
        let Ok(value) = std::str::from_utf8(raw) else {
            continue;
        };
        let Some((parameter_index, qualifier)) = parameter_name_parts(value) else {
            if evaluate_constant_expression(value).is_some() {
                literals.push(value);
            }
            continue;
        };
        matches.push(ExpressionDeclarationName {
            offset: at,
            value,
            parameter_index,
            qualifier,
            literal: None,
        });
    }
    let [declaration] = matches.as_slice() else {
        return None;
    };
    let literal = match literals.as_slice() {
        [literal] => Some(*literal),
        _ => None,
    };
    Some(ExpressionDeclarationName {
        literal,
        ..*declaration
    })
}

/// Decode the unique `01 02 10 index ff` primary-body field in one operation.
pub fn operation_body_reference(record: OperationRecord<'_>) -> Option<OperationBodyReference> {
    let matches = operation_body_references(record);
    let [reference] = matches.as_slice() else {
        return None;
    };
    Some(reference.clone())
}

/// Decode every ordered `01 02 10 index ff` body-reference field in one operation.
pub fn operation_body_references(record: OperationRecord<'_>) -> Vec<OperationBodyReference> {
    let mut matches = Vec::new();
    for marker in record
        .bytes
        .windows(3)
        .enumerate()
        .filter_map(|(offset, window)| (window == [0x01, 0x02, 0x10]).then_some(offset))
    {
        let token = marker + 3;
        let Some((Some(object_index), end)) = feature_object_index(record.bytes, token) else {
            continue;
        };
        if record.bytes.get(end) == Some(&0xff) {
            matches.push(OperationBodyReference {
                offset: record.offset + token,
                object_index,
                raw_object_index: record.bytes[token..end].to_vec(),
            });
        }
    }
    matches
}

/// Decode every exact nested `01 02 tag index 97 75 01 02 11 index ff` frame.
///
/// Both indices are non-null and canonical. The fixed middle sequence is a
/// structural relation marker; this parser does not assign endpoint roles.
pub fn operation_object_relations(record: OperationRecord<'_>) -> Vec<OperationObjectRelation> {
    const MIDDLE: &[u8] = &[0x97, 0x75, 0x01, 0x02, 0x11];
    let mut relations = Vec::new();
    for marker in record
        .payload
        .windows(2)
        .enumerate()
        .filter_map(|(offset, window)| (window == [0x01, 0x02]).then_some(offset))
    {
        let Some(link_tag) = record.payload.get(marker + 2).copied() else {
            continue;
        };
        let first_token = marker + 3;
        let Some((Some(first_object_index), first_end)) =
            feature_object_index(record.payload, first_token)
        else {
            continue;
        };
        let raw_first_object_index = &record.payload[first_token..first_end];
        if !canonical_feature_object_index(Some(first_object_index), raw_first_object_index)
            || record.payload.get(first_end..first_end + MIDDLE.len()) != Some(MIDDLE)
        {
            continue;
        }
        let second_token = first_end + MIDDLE.len();
        let Some((Some(second_object_index), second_end)) =
            feature_object_index(record.payload, second_token)
        else {
            continue;
        };
        let raw_second_object_index = &record.payload[second_token..second_end];
        if !canonical_feature_object_index(Some(second_object_index), raw_second_object_index)
            || record.payload.get(second_end) != Some(&0xff)
        {
            continue;
        }
        relations.push(OperationObjectRelation {
            offset: record.payload_offset + marker,
            link_tag,
            first_object_index,
            raw_first_object_index: raw_first_object_index.to_vec(),
            first_object_index_offset: record.payload_offset + first_token,
            second_object_index,
            raw_second_object_index: raw_second_object_index.to_vec(),
            second_object_index_offset: record.payload_offset + second_token,
            end_offset: record.payload_offset + second_end + 1,
        });
    }
    relations
}

/// Decode every exact direct `01 02 17 index ff 80 00 00 02` field.
///
/// The fixed suffix separates this field from the nested object-relation
/// frame, which uses the same opening marker and tag but has a different
/// middle sequence. The parser retains no endpoint or operation role.
pub fn operation_tagged_references(record: OperationRecord<'_>) -> Vec<OperationTaggedReference> {
    const PREFIX: &[u8] = &[0x01, 0x02, 0x17];
    const SUFFIX: &[u8] = &[0xff, 0x80, 0x00, 0x00, 0x02];
    let mut references = Vec::new();
    for marker in record
        .payload
        .windows(PREFIX.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == PREFIX).then_some(offset))
    {
        let token = marker + PREFIX.len();
        let Some((Some(object_index), end)) = feature_object_index(record.payload, token) else {
            continue;
        };
        let raw_object_index = &record.payload[token..end];
        if !canonical_feature_object_index(Some(object_index), raw_object_index) {
            continue;
        }
        let Some(suffix_end) = end.checked_add(SUFFIX.len()) else {
            continue;
        };
        if record.payload.get(end..suffix_end) != Some(SUFFIX) {
            continue;
        }
        references.push(OperationTaggedReference {
            offset: record.payload_offset + marker,
            tag: 0x17,
            object_index,
            raw_object_index: raw_object_index.to_vec(),
            object_index_offset: record.payload_offset + token,
            end_offset: record.payload_offset + suffix_end,
        });
    }
    references
}

/// Decode every exact direct `01 02 03 index 01 00 00 00 00 00` field.
///
/// The object index is retained as native evidence. It does not assign a
/// body, operand, input, output, seed, transform, or construction role.
pub fn operation_data_block_references(
    record: OperationRecord<'_>,
) -> Vec<OperationDataBlockReference> {
    const PREFIX: &[u8] = &[0x01, 0x02, 0x03];
    const SUFFIX: &[u8] = &[0x01, 0x00, 0x00, 0x00, 0x00, 0x00];
    let mut references = Vec::new();
    for marker in record
        .payload
        .windows(PREFIX.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == PREFIX).then_some(offset))
    {
        let token = marker + PREFIX.len();
        let Some((Some(object_index), end)) = feature_object_index(record.payload, token) else {
            continue;
        };
        let raw_object_index = &record.payload[token..end];
        if !canonical_feature_object_index(Some(object_index), raw_object_index) {
            continue;
        }
        let Some(suffix_end) = end.checked_add(SUFFIX.len()) else {
            continue;
        };
        if record.payload.get(end..suffix_end) != Some(SUFFIX) {
            continue;
        }
        references.push(OperationDataBlockReference {
            offset: record.payload_offset + marker,
            object_index,
            raw_object_index: raw_object_index.to_vec(),
            object_index_offset: record.payload_offset + token,
            end_offset: record.payload_offset + suffix_end,
        });
    }
    references
}

fn feature_object_index(bytes: &[u8], at: usize) -> Option<(Option<u32>, usize)> {
    let prefix = *bytes.get(at)?;
    match prefix {
        0x00..=0x7f => Some((Some(u32::from(prefix)), at + 1)),
        0x80..=0x8f => Some((
            Some(u32::from(prefix - 0x80) * 256 + u32::from(*bytes.get(at + 1)?)),
            at + 2,
        )),
        0x90 => Some((
            Some(u32::from(u16::from_be_bytes([
                *bytes.get(at + 1)?,
                *bytes.get(at + 2)?,
            ]))),
            at + 3,
        )),
        0xff => Some((None, at + 1)),
        _ => None,
    }
}

fn canonical_feature_object_index(value: Option<u32>, raw: &[u8]) -> bool {
    matches!(
        (value, raw),
        (None, [0xff])
            | (Some(0..=0x7f), [_])
            | (Some(0x80..=0x0fff), [0x80..=0x8f, _])
            | (Some(0x1000..=0xffff), [0x90, _, _])
    )
}

/// Decode every exact common frame in one bounded operation payload.
pub fn operation_common_frames(record: OperationRecord<'_>) -> Vec<OperationCommonFrame> {
    let decode = |prefix_start: usize, widths: [usize; 3], marker: [u8; 3]| {
        if marker == [0x01, 0x01, 0x01] && record.label.value != "DELETE" {
            return None;
        }

        // The compact prefix has fixed widths for each frame family. Check the
        // discriminator at its exact position before decoding any token. This
        // scan visits every payload byte, so a candidate must not own heap
        // storage until all of its framing and suffix invariants pass.
        let prefix_width = widths.into_iter().sum::<usize>();
        let marker_start = prefix_start.checked_add(prefix_width)?;
        (record
            .payload
            .get(marker_start..marker_start + marker.len())
            == Some(&marker))
        .then_some(())?;

        let mut at = prefix_start;
        let mut tokens = [CompactToken {
            value: CompactIndex::Null,
            offset: 0,
            width: 0,
        }; 3];
        let mut indices = [0; 3];
        let mut index_offsets = [0; 3];
        for (slot, width) in widths.into_iter().enumerate() {
            let token = compact_token(record.payload, at)?;
            let CompactIndex::Value(index) = token.value else {
                return None;
            };
            (token.width == width).then_some(())?;
            tokens[slot] = token;
            indices[slot] = index;
            index_offsets[slot] = record.payload_offset + at;
            at += width;
        }
        at += marker.len();
        let state_offset = at;
        let state = record.payload.get(at..at + 8)?.try_into().ok()?;
        at += 8;
        let local_ordinal_offset = at;
        let (Some(local_ordinal), first_end) = feature_object_index(record.payload, at)? else {
            return None;
        };
        let first_raw = &record.payload[at..first_end];
        canonical_feature_object_index(Some(local_ordinal), first_raw).then_some(())?;
        let (Some(repeated), second_end) = feature_object_index(record.payload, first_end)? else {
            return None;
        };
        let second_raw = &record.payload[first_end..second_end];
        (repeated == local_ordinal && second_raw == first_raw).then_some(())?;
        let object_index_offset = second_end;
        let (object_index, object_end) = feature_object_index(record.payload, second_end)?;
        let object_raw = &record.payload[second_end..object_end];
        canonical_feature_object_index(object_index, object_raw).then_some(())?;
        (record.payload.get(object_end) == Some(&0)).then_some(())?;
        Some(OperationCommonFrame {
            indices,
            raw_indices: std::array::from_fn(|slot| {
                raw_compact_token(record.payload, tokens[slot])
            }),
            marker,
            state,
            offset: record.payload_offset + prefix_start,
            index_offsets,
            state_offset: record.payload_offset + state_offset,
            local_ordinal,
            raw_local_ordinal: first_raw.to_vec(),
            object_index,
            raw_object_index: object_raw.to_vec(),
            local_ordinal_offset: record.payload_offset + local_ordinal_offset,
            object_index_offset: record.payload_offset + object_index_offset,
            end_offset: record.payload_offset + object_end + 1,
        })
    };

    let mut frames = Vec::new();
    for start in 0..record.payload.len() {
        if let Some(frame) = decode(start, [1, 2, 2], [0x01, 0x03, 0x02]) {
            frames.push(frame);
        }
        if let Some(frame) = decode(start, [1, 1, 1], [0x01, 0x01, 0x01]) {
            frames.push(frame);
        }
    }
    frames.sort_by_key(|frame| frame.offset);
    frames
}

/// Decode the unique terminal common-frame suffix and its exact immediate common frame.
pub fn operation_terminal_frame(record: OperationRecord<'_>) -> Option<OperationTerminalFrame> {
    let terminator = record.payload.len().checked_sub(1)?;
    (record.payload.get(terminator) == Some(&0)).then_some(())?;
    let common_frames = operation_common_frames(record);
    let mut matches = Vec::new();
    for start in terminator.saturating_sub(9)..terminator {
        let Some((Some(local_ordinal), first_end)) = feature_object_index(record.payload, start)
        else {
            continue;
        };
        let first_raw = &record.payload[start..first_end];
        if !canonical_feature_object_index(Some(local_ordinal), first_raw) {
            continue;
        }
        let Some((Some(repeated), second_end)) = feature_object_index(record.payload, first_end)
        else {
            continue;
        };
        let second_raw = &record.payload[first_end..second_end];
        if repeated != local_ordinal || second_raw != first_raw {
            continue;
        }
        let Some((object_index, object_end)) = feature_object_index(record.payload, second_end)
        else {
            continue;
        };
        let object_raw = &record.payload[second_end..object_end];
        if object_end != terminator || !canonical_feature_object_index(object_index, object_raw) {
            continue;
        }
        let local_ordinal_offset = record.payload_offset + start;
        let immediate_common_frame_offset = common_frames
            .iter()
            .find(|frame| {
                frame.local_ordinal_offset == local_ordinal_offset
                    && frame.end_offset == record.payload_offset + object_end + 1
            })
            .map(|frame| frame.offset);
        matches.push(OperationTerminalFrame {
            immediate_common_frame_offset,
            local_ordinal,
            raw_local_ordinal: first_raw.to_vec(),
            object_index,
            raw_object_index: object_raw.to_vec(),
            offset: local_ordinal_offset,
            object_index_offset: record.payload_offset + second_end,
        });
    }
    let [frame] = matches.as_slice() else {
        return None;
    };
    Some(frame.clone())
}

/// Decode ordered `04 00, object_index, 02 0b` references from one bounded block.
pub fn data_block_object_references(bytes: &[u8]) -> Vec<DataBlockObjectReference> {
    let mut references = Vec::new();
    let mut at = 0usize;
    while at + 5 <= bytes.len() {
        if bytes.get(at..at + 2) != Some(&[0x04, 0x00]) {
            at += 1;
            continue;
        }
        let token = at + 2;
        let Some((Some(object_index), end)) = feature_object_index(bytes, token) else {
            at += 1;
            continue;
        };
        if bytes.get(end..end + 2) != Some(&[0x02, 0x0b]) {
            at += 1;
            continue;
        }
        references.push(DataBlockObjectReference {
            offset: token,
            object_index,
            raw_object_index: bytes[token..end].to_vec(),
        });
        at = end + 2;
    }
    references
}

/// Decode Boolean target and tool lists following complete operation labels.
#[allow(dead_code)] // Direct byte-slice parser entry point retained for focused parser tests.
pub fn boolean_operations(bytes: &[u8], base_offset: usize) -> Vec<BooleanOperation> {
    let labels = operation_labels(bytes, base_offset);
    boolean_operations_with_labels(bytes, base_offset, &labels)
}

fn boolean_operations_with_labels(
    bytes: &[u8],
    base_offset: usize,
    labels: &[OperationLabel<'_>],
) -> Vec<BooleanOperation> {
    const BODY_HEADER: &[u8] = &[
        0x31, 0x00, 0x00, 0x01, 0x00, 0x14, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 0x03,
        0x00, 0x00, 0xe0, 0x7f, 0xff, 0xff, 0xff, 0x01, 0x01,
    ];
    labels
        .iter()
        .copied()
        .filter_map(|label| {
            let kind = match label.value {
                "UNITE" => BooleanOperationKind::Unite,
                "SUBTRACT" => BooleanOperationKind::Subtract,
                "INTERSECT" => BooleanOperationKind::Intersect,
                _ => return None,
            };
            let at = label.offset.checked_sub(base_offset)?;
            let label_end = at.checked_add(usize::from(*bytes.get(at + 1)?))? + 1;
            if bytes.get(label_end..label_end + BODY_HEADER.len()) != Some(BODY_HEADER) {
                return None;
            }
            let (targets, next) =
                counted_feature_object_indices(bytes, base_offset, label_end + BODY_HEADER.len())?;
            if targets.len() != 1 || bytes.get(next) != Some(&0) {
                return None;
            }
            let (tools, end) = counted_feature_object_indices(bytes, base_offset, next + 1)?;
            if tools.is_empty() || bytes.get(end) != Some(&0) {
                return None;
            }
            let target = targets.into_iter().next()?;
            Some(BooleanOperation {
                offset: label.offset,
                kind,
                target: target.object_index,
                raw_target: target.raw_object_index,
                target_offset: target.offset,
                tools: tools.iter().map(|tool| tool.object_index).collect(),
                raw_tools: tools
                    .iter()
                    .map(|tool| tool.raw_object_index.clone())
                    .collect(),
                tool_offsets: tools.iter().map(|tool| tool.offset).collect(),
            })
        })
        .collect()
}

fn counted_feature_object_indices(
    bytes: &[u8],
    base_offset: usize,
    at: usize,
) -> Option<(Vec<PayloadObjectReference>, usize)> {
    if bytes.get(at) != Some(&0x01) {
        return None;
    }
    let count = usize::from(*bytes.get(at + 1)?).checked_sub(1)?;
    let values_start = at + 2;
    let mut scan_cursor = values_start;
    for _ in 0..count {
        let (value, next) = feature_object_index(bytes, scan_cursor)?;
        value?;
        scan_cursor = next;
    }

    let mut cursor = values_start;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let (value, next) = feature_object_index(bytes, cursor)?;
        values.push(PayloadObjectReference {
            offset: base_offset + cursor,
            object_index: value?,
            raw_object_index: bytes.get(cursor..next)?.to_vec(),
        });
        cursor = next;
    }
    Some((values, cursor))
}

/// Decode count-framed runs of same-section record references.
pub fn counted_record_references(
    bytes: &[u8],
    base_offset: usize,
    record_count: usize,
) -> Vec<ReferenceValue> {
    let mut references = Vec::new();
    let mut at = 0usize;
    while at + 5 <= bytes.len() {
        if bytes[at] != 0x01 || bytes[at + 1] < 2 {
            at += 1;
            continue;
        }
        let count = usize::from(bytes[at + 1] - 1);
        let Some(end) = at.checked_add(2 + count * 3) else {
            at += 1;
            continue;
        };
        if end > bytes.len() || (0..count).any(|index| bytes[at + 2 + index * 3] != 0x90) {
            at += 1;
            continue;
        }
        if (0..count).any(|index| {
            let token = at + 2 + index * 3;
            let value = u16::from_be_bytes([bytes[token + 1], bytes[token + 2]]);
            usize::from(value) >= record_count
        }) {
            at += 1;
            continue;
        }
        let mut run = Vec::with_capacity(count);
        for index in 0..count {
            let token = at + 2 + index * 3;
            let value = u16::from_be_bytes([bytes[token + 1], bytes[token + 2]]);
            run.push(ReferenceValue {
                offset: base_offset + token,
                kind: ReferenceKind::RecordOrdinal16,
                value: u32::from(value),
            });
        }
        if run.is_empty() {
            at += 1;
        } else {
            references.extend(run);
            at = end;
        }
    }
    references
}

/// Decode self-identifying persistent handles and exact adjacent handle pairs.
pub fn record_references(bytes: &[u8], base_offset: usize) -> Vec<ReferenceValue> {
    let parsed = references(bytes, base_offset);
    let mut out = parsed
        .iter()
        .copied()
        .filter(|reference| reference.kind == ReferenceKind::PersistentHandle)
        .collect::<Vec<_>>();
    out.extend(parsed.windows(2).filter_map(|pair| {
        let [persistent, tagged] = pair else {
            unreachable!("a two-element window always has two references");
        };
        let adjacent = persistent
            .offset
            .checked_add(5)
            .is_some_and(|offset| tagged.offset == offset);
        (persistent.kind == ReferenceKind::PersistentHandle
            && tagged.kind == ReferenceKind::Tagged28
            && adjacent)
            .then_some(*tagged)
    }));
    out.sort_by_key(|reference| reference.offset);
    out
}

/// Decode tagged references wholly contained in `bytes`.
pub fn references(bytes: &[u8], base_offset: usize) -> Vec<ReferenceValue> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        if bytes[at] == 0xe0 {
            if let Some(value) = View::u32_be_at(bytes, at + 1) {
                out.push(ReferenceValue {
                    offset: base_offset + at,
                    kind: ReferenceKind::PersistentHandle,
                    value,
                });
                at += 5;
                continue;
            }
        } else if bytes[at] & 0xf0 == 0xc0 {
            if let Some(value) = View::u32_be_at(bytes, at) {
                out.push(ReferenceValue {
                    offset: base_offset + at,
                    kind: ReferenceKind::Tagged28,
                    value: value & 0x0fff_ffff,
                });
                at += 4;
                continue;
            }
        }
        at += 1;
    }
    out
}

/// Decode `66 32 03` printable-string values wholly contained in `bytes`.
pub fn string_values(bytes: &[u8], base_offset: usize) -> Vec<StringValue<'_>> {
    const MARKER: &[u8] = &[0x66, 0x32, 0x03];
    bytes
        .windows(MARKER.len())
        .enumerate()
        .filter(|(_, window)| *window == MARKER)
        .filter_map(|(offset, _)| {
            let declared = usize::from(*bytes.get(offset + 3)?);
            let text_len = declared.checked_sub(2)?;
            let start = offset.checked_add(4)?;
            let end = start.checked_add(text_len)?;
            let raw = bytes.get(start..end)?;
            (!raw.is_empty()
                && raw
                    .iter()
                    .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
                && bytes.get(end) == Some(&0))
            .then(|| StringValue {
                offset: base_offset + offset,
                value: std::str::from_utf8(raw).expect("invariant: printable ASCII is valid UTF-8"),
            })
        })
        .collect()
}

/// Return whether `value` is canonical lowercase UUID text.
pub fn canonical_uuid_text(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

/// Decode complete `03 26, canonical UUID text, 00` values in `bytes`.
pub fn uuid_string_values(bytes: &[u8], base_offset: usize) -> Vec<UuidStringValue<'_>> {
    const MARKER: &[u8] = &[0x03, 0x26];
    const TEXT_LEN: usize = 36;
    bytes
        .windows(MARKER.len())
        .enumerate()
        .filter(|(_, window)| *window == MARKER)
        .filter_map(|(offset, _)| {
            let start = offset.checked_add(MARKER.len())?;
            let end = start.checked_add(TEXT_LEN)?;
            let raw = bytes.get(start..end)?;
            let value = std::str::from_utf8(raw).ok()?;
            (canonical_uuid_text(value) && bytes.get(end) == Some(&0)).then_some(UuidStringValue {
                offset: base_offset + offset,
                value,
            })
        })
        .collect()
}

#[cfg(test)]
mod uuid_string_value_tests {
    use super::*;

    #[test]
    fn decodes_only_complete_canonical_uuid_frames() {
        let mut bytes = b"prefix\x03\x2601234567-89ab-cdef-0123-456789abcdef\0suffix".to_vec();
        let values = uuid_string_values(&bytes, 100);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].offset, 106);
        assert_eq!(values[0].value, "01234567-89ab-cdef-0123-456789abcdef");

        bytes[6 + 2 + 9] = b'A';
        assert!(uuid_string_values(&bytes, 0).is_empty());
        assert!(!canonical_uuid_text("01234567-89ab-cdef-0123-456789abcde"));
        assert!(!canonical_uuid_text("01234567-89ab-cdef-0123_456789abcdef"));
        assert!(!canonical_uuid_text("01234567-89ab-cdef-0123-456789abcdeg"));
    }

    #[test]
    fn rejects_truncated_or_unterminated_uuid_frames() {
        let frame = b"\x03\x2601234567-89ab-cdef-0123-456789abcdef\0";
        assert!(uuid_string_values(&frame[..frame.len() - 1], 0).is_empty());
        let mut unterminated = frame.to_vec();
        *unterminated.last_mut().expect("nonempty frame") = 1;
        assert!(uuid_string_values(&unterminated, 0).is_empty());
    }
}

/// Decode `66 1b 03, byte-length, printable UTF-8, 00` values in `bytes`.
pub fn surface_payload_strings(bytes: &[u8]) -> Vec<SurfacePayloadString<'_>> {
    const MARKER: &[u8] = &[0x66, 0x1b, 0x03];
    bytes
        .windows(MARKER.len())
        .enumerate()
        .filter(|(_, window)| *window == MARKER)
        .filter_map(|(offset, _)| {
            let text_len = usize::from(*bytes.get(offset + MARKER.len())?);
            let start = offset.checked_add(MARKER.len() + 1)?;
            let end = start.checked_add(text_len)?;
            let raw = bytes.get(start..end)?;
            let value = std::str::from_utf8(raw).ok()?;
            (!value.is_empty()
                && value.chars().all(|character| !character.is_control())
                && bytes.get(end) == Some(&0))
            .then_some(SurfacePayloadString { offset, value })
        })
        .collect()
}

/// Decode every strictly length-framed numeric expression in an OM payload.
///
/// The `hostglobalvariables` marker identifies the owning table. Individual
/// records are self-framed as `handle, 04, length, text, 00`, so expression
/// decoding does not depend on an object-id table having the same cardinality
/// as an external entity-index array.
pub fn numeric_expressions(bytes: &[u8]) -> Vec<NumericExpression<'_>> {
    if !bytes
        .windows(b"hostglobalvariables".len())
        .any(|window| window == b"hostglobalvariables")
    {
        return Vec::new();
    }
    bytes
        .windows(b"(Number [".len())
        .enumerate()
        .filter(|(_, window)| *window == b"(Number [")
        .filter_map(|(offset, _)| {
            numeric_expression_at(
                &bytes[offset.saturating_sub(3)..],
                offset.saturating_sub(3),
                None,
            )
        })
        .collect()
}

/// Locate independently size-framed OM sections and their type registries.
pub fn sections(bytes: &[u8]) -> Vec<Section<'_>> {
    sections_with_operation_label_layouts(bytes, None, &[])
}

/// Locate sections while reusing cached operation-label layouts when present.
pub(crate) fn sections_with_operation_label_layouts<'a>(
    bytes: &'a [u8],
    entry_index: Option<usize>,
    cached_operation_label_layouts: &[(usize, usize, Vec<OperationLabelLayout>)],
) -> Vec<Section<'a>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 16 <= bytes.len() {
        let Some(relative) = bytes[at..]
            .windows(4)
            .position(|window| window == [0xff; 4])
        else {
            break;
        };
        let offset = at + relative;
        let Some(payload_len) = View::u32_be_at(bytes, offset + 8).map(|value| value as usize)
        else {
            break;
        };
        let standard_end = offset
            .checked_add(16)
            .and_then(|header_end| header_end.checked_add(payload_len));
        let compact_terminal_end = offset
            .checked_add(12)
            .and_then(|header_end| header_end.checked_add(payload_len))
            .filter(|end| *end == bytes.len());
        let Some(end) = standard_end
            .filter(|end| *end <= bytes.len())
            .or(compact_terminal_end)
        else {
            at = offset + 4;
            continue;
        };
        if bytes.get(offset + 12..offset + 14) != Some(b"OM") || end > bytes.len() {
            at = offset + 4;
            continue;
        }
        let types = type_definitions(bytes, offset + 16, end);
        let field_start = types.last().map_or(offset + 16, |definition| {
            definition.offset + definition.name.len() + 2
        });
        let record_area_pointer = section_record_area_pointer(bytes, offset, field_start, end);
        let (fields, record_area_offset) =
            if let Some((record_area_offset, pointer_offset)) = record_area_pointer {
                (
                    all_field_definitions(bytes, field_start, pointer_offset),
                    Some(record_area_offset),
                )
            } else {
                (field_definitions(bytes, field_start, end), None)
            };
        let record_area = record_area_offset.map(|start| &bytes[start..end]);
        let cached_operation_labels = match (record_area, record_area_offset) {
            (Some(record_area), Some(record_area_offset)) => cached_operation_label_layouts
                .iter()
                .find(|(cached_entry, offset, _)| {
                    Some(*cached_entry) == entry_index && *offset == record_area_offset
                })
                .map_or_else(
                    || operation_labels(record_area, record_area_offset),
                    |(_, _, layouts)| {
                        materialize_operation_labels(record_area, record_area_offset, layouts)
                    },
                ),
            _ => Vec::new(),
        };
        out.push(Section {
            offset,
            byte_len: end - offset,
            types: types.into(),
            fields: fields.into(),
            record_area_offset,
            record_area,
            cached_operation_labels: cached_operation_labels.into(),
        });
        at = end;
    }
    out
}

fn section_record_area_pointer(
    bytes: &[u8],
    section_offset: usize,
    schema_start: usize,
    section_end: usize,
) -> Option<(usize, usize)> {
    let mut matches = (schema_start..section_end.saturating_sub(3)).filter_map(|at| {
        let relative = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
        let target = section_offset.checked_add(relative)?;
        (target >= at.checked_add(4)? && target.checked_add(15)? <= section_end).then_some(())?;
        is_product_record(bytes.get(target.checked_add(12)?..section_end)?).then_some((target, at))
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

/// Validate one self-framed NX product record.
pub(crate) fn is_product_record(bytes: &[u8]) -> bool {
    if !matches!(bytes.get(..2), Some([0x04 | 0x05, 0x01])) {
        return false;
    }
    let Some(length) = bytes
        .get(2)
        .copied()
        .map(usize::from)
        .and_then(|declared| declared.checked_sub(2))
    else {
        return false;
    };
    let Some(end) = 3usize.checked_add(length) else {
        return false;
    };
    bytes.get(3..end).is_some_and(|text| {
        text.starts_with(b"NX ")
            && text
                .iter()
                .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    }) && bytes.get(end) == Some(&0)
}

#[derive(Debug, Clone, Copy)]
struct ProductRecordRange {
    start: usize,
    end: usize,
}

/// Count validated product records fully contained in `[lower, upper]`.
///
/// A validated product record cannot contain another validated product-record
/// start: its text is restricted to printable ASCII and spaces, while a record
/// start begins with a non-printable byte. The discovery pass therefore orders
/// both starts and ends. This makes the containment query a pair of binary
/// searches instead of a scan over every product record for every candidate.
fn product_record_count_within(ranges: &[ProductRecordRange], lower: usize, upper: usize) -> usize {
    let first = ranges.partition_point(|range| range.start < lower);
    let end = ranges.partition_point(|range| range.end <= upper);
    end.saturating_sub(first)
}

/// Locate validated NX OM entity-index/object-id-table pairs.
///
/// A candidate is accepted only when the arrays are adjacent, the index is
/// monotone, its first offset is zero, its second offset self-anchors the first
/// entity exactly at the end of the object-id table, and that entity carries the
/// NX root marker.
pub fn indexed_sections(bytes: &[u8]) -> Vec<IndexedSection<'_>> {
    let mut out = Vec::new();
    let mut seen_record_starts = BTreeSet::new();
    let product_record_ranges = (0..bytes.len())
        .filter_map(|offset| {
            let suffix = bytes.get(offset..)?;
            is_product_record(suffix).then_some(())?;
            let text_length = usize::from(*suffix.get(2)?).checked_sub(2)?;
            let record_len = 3usize.checked_add(text_length)?.checked_add(1)?;
            Some(ProductRecordRange {
                start: offset,
                end: offset.checked_add(record_len)?,
            })
        })
        .collect::<Vec<_>>();
    for table in 0..bytes.len().saturating_sub(4) {
        let Some(count) = View::u32_le_at(bytes, table).map(|value| value as usize) else {
            continue;
        };
        if !(2..=100_000).contains(&count) {
            continue;
        }
        let Some(index_len) = count.checked_add(1).and_then(|n| n.checked_mul(4)) else {
            continue;
        };
        let Some(index_start) = table.checked_sub(index_len) else {
            continue;
        };
        let Some(table_end) = count
            .checked_mul(4)
            .and_then(|length| table.checked_add(4 + length))
        else {
            continue;
        };
        if !is_product_record(bytes.get(table_end..).unwrap_or_default())
            || View::u32_le_at(bytes, index_start) != Some(0)
        {
            continue;
        }
        let Some(first) = View::u32_le_at(bytes, index_start + 4).map(|value| value as usize)
        else {
            continue;
        };
        let Some(base) = table_end.checked_sub(first) else {
            continue;
        };
        if !entity_index_is_valid(bytes, index_start, count, base) {
            continue;
        }
        if !seen_record_starts.insert(table_end) {
            continue;
        }
        let mut records = Vec::with_capacity(count - 1);
        for index in 1..count {
            let start_offset = entity_index_offset(bytes, index_start, index)
                .expect("validated entity index remains readable");
            let end_offset = entity_index_offset(bytes, index_start, index + 1)
                .expect("validated entity index remains readable");
            let start = base + start_offset;
            let end = base + end_offset;
            let Some(payload) = bytes.get(start..end) else {
                records.clear();
                break;
            };
            let Some(object_id) = View::u32_le_at(bytes, table + 4 + index * 4) else {
                records.clear();
                break;
            };
            records.push(EntityRecord {
                object_id: Some(object_id),
                object_id_offset: Some(table + 4 + index * 4),
                offset: start,
                bytes: payload,
            });
        }
        if records.len() == count - 1 {
            let types = type_definitions(bytes, base, index_start);
            let fields = all_field_definitions(bytes, base, index_start);
            out.push(IndexedSection {
                base,
                entity_index_offset: index_start,
                object_id_table_offset: table,
                types: types.into(),
                fields: fields.into(),
                control: None,
                column_storage: None,
                records: records.into(),
            });
        }
    }
    for count_offset in 8..bytes.len().saturating_sub(4) {
        let Some(record_count) = View::u32_le_at(bytes, count_offset).map(|value| value as usize)
        else {
            continue;
        };
        if !(2..=100_000).contains(&record_count) {
            continue;
        }
        let offset_count = record_count + 2;
        let Some(index_len) = offset_count.checked_mul(4) else {
            continue;
        };
        let Some(index_start) = count_offset.checked_sub(index_len) else {
            continue;
        };
        let Some(first) = View::u32_le_at(bytes, index_start).map(|value| value as usize) else {
            continue;
        };
        let Some(second) = View::u32_le_at(bytes, index_start + 4).map(|value| value as usize)
        else {
            continue;
        };
        let Some(third) = View::u32_le_at(bytes, index_start + 8).map(|value| value as usize)
        else {
            continue;
        };
        let Some(last) = View::u32_le_at(bytes, count_offset - 4).map(|value| value as usize)
        else {
            continue;
        };
        if first < count_offset + 4
            || first >= second
            || second > third
            || third > last
            || last > bytes.len()
        {
            continue;
        }
        // The first two data ranges contain the sole product marker. Reject
        // random count words before allocating and walking the full offset
        // table; malformed payloads commonly satisfy the cheap monotonicity
        // checks while carrying no self-framed NX record at all.
        let product_record_count =
            product_record_count_within(&product_record_ranges, first, second).saturating_add(
                product_record_count_within(&product_record_ranges, second, third),
            );
        if product_record_count != 1 {
            continue;
        }
        if !offset_only_index_is_valid(bytes, index_start, offset_count, count_offset) {
            continue;
        }
        let first_record = second;
        if !seen_record_starts.insert(first_record) {
            continue;
        }
        let records = (0..record_count)
            .map(|index| {
                let start = entity_index_offset(bytes, index_start, index + 1)
                    .expect("validated offset-only index remains readable");
                let end = entity_index_offset(bytes, index_start, index + 2)
                    .expect("validated offset-only index remains readable");
                EntityRecord {
                    object_id: None,
                    object_id_offset: None,
                    offset: start,
                    bytes: &bytes[start..end],
                }
            })
            .collect::<Vec<_>>();
        out.push(IndexedSection {
            base: 0,
            entity_index_offset: index_start,
            object_id_table_offset: first,
            types: type_definitions(bytes, 0, index_start).into(),
            fields: all_field_definitions(bytes, 0, index_start).into(),
            control: Some(EntityRecord {
                object_id: None,
                object_id_offset: None,
                offset: first,
                bytes: &bytes[first..first_record],
            }),
            column_storage: Some(&bytes[first_record..last]),
            records: records.into(),
        });
    }
    out
}

fn entity_index_offset(bytes: &[u8], index_start: usize, index: usize) -> Option<usize> {
    let offset = index_start.checked_add(index.checked_mul(4)?)?;
    usize::try_from(View::u32_le_at(bytes, offset)?).ok()
}

// Validate candidate offset tables through borrowed words. A count word is
// only a framing hint; do not allocate an offset table until every word is
// monotone and its terminal range is in bounds.
fn entity_index_is_valid(bytes: &[u8], index_start: usize, count: usize, base: usize) -> bool {
    let Some(mut previous) = entity_index_offset(bytes, index_start, 0) else {
        return false;
    };
    if previous != 0 {
        return false;
    }
    let Some(first) = entity_index_offset(bytes, index_start, 1) else {
        return false;
    };
    if first == 0 {
        return false;
    }
    previous = first;
    for index in 2..=count {
        let Some(current) = entity_index_offset(bytes, index_start, index) else {
            return false;
        };
        if current < previous {
            return false;
        }
        previous = current;
    }
    base.checked_add(previous)
        .is_some_and(|end| end <= bytes.len())
}

fn offset_only_index_is_valid(
    bytes: &[u8],
    index_start: usize,
    offset_count: usize,
    count_offset: usize,
) -> bool {
    let Some(mut previous) = entity_index_offset(bytes, index_start, 0) else {
        return false;
    };
    if previous < count_offset.saturating_add(4) {
        return false;
    }
    for index in 1..offset_count {
        let Some(current) = entity_index_offset(bytes, index_start, index) else {
            return false;
        };
        if current < previous {
            return false;
        }
        previous = current;
    }
    previous <= bytes.len()
}

/// Parse indexed sections once and retain only their source ranges.
pub(crate) fn indexed_section_layouts(bytes: &[u8]) -> Vec<IndexedSectionLayout> {
    indexed_sections(bytes)
        .iter()
        .map(IndexedSectionLayout::from_section)
        .collect()
}

/// Decode the first self-framed NX product/version marker in `bytes`.
pub fn store_version(bytes: &[u8], base_offset: usize) -> Option<StoreVersion<'_>> {
    (0..bytes.len().saturating_sub(3)).find_map(|at| {
        let suffix = &bytes[at..];
        is_product_record(suffix).then(|| {
            let length = usize::from(suffix[2]) - 2;
            StoreVersion {
                offset: base_offset + at,
                value: std::str::from_utf8(&suffix[3..3 + length])
                    .expect("validated printable NX version is UTF-8"),
            }
        })
    })
}

/// Decode the zero-prefixed offset-store control form as ordered 24-bit values.
///
/// Each word is serialized `00, value:u24 LE`. The complete form is atomic.
pub fn offset_store_control_values(bytes: &[u8]) -> Option<Vec<u32>> {
    (!bytes.is_empty() && bytes.len().is_multiple_of(4)).then_some(())?;
    bytes
        .chunks_exact(4)
        .map(|word| {
            (word[0] == 0).then(|| {
                u32::from(word[1]) | (u32::from(word[2]) << 8) | (u32::from(word[3]) << 16)
            })
        })
        .collect()
}

/// Decode the distinct leading class-registry identities in an offset-store
/// control block.
///
/// The registry may omit declarations that this decoder cannot type, so its
/// retained declaration count is not an ordinal bound. The class lane is
/// instead the unique nonempty prefix whose identities are distinct and all
/// smaller than every following metadata value.
pub fn offset_store_control_class_ordinals(bytes: &[u8]) -> Option<Vec<u32>> {
    let values = offset_store_control_values(bytes)?;
    let mut suffix_minima = vec![u32::MAX; values.len()];
    for index in (0..values.len().saturating_sub(1)).rev() {
        suffix_minima[index] = suffix_minima[index + 1].min(values[index + 1]);
    }
    let mut identities = BTreeSet::new();
    let mut maximum_identity = 0;
    let mut boundary = None;
    for index in 0..values.len().saturating_sub(1) {
        let identity = values[index];
        if !identities.insert(identity) {
            break;
        }
        maximum_identity = maximum_identity.max(identity);
        if maximum_identity < suffix_minima[index] && boundary.replace(index + 1).is_some() {
            return None;
        }
    }
    let boundary = boundary?;
    if boundary == values.len() {
        return None;
    }
    Some(values[..boundary].to_vec())
}

fn offset_store_product_anchored_form(bytes: &[u8]) -> Option<OffsetStoreControlForm> {
    let matches = (0..bytes.len())
        .filter(|offset| is_product_record(&bytes[*offset..]))
        .collect::<Vec<_>>();
    let [product_offset] = matches.as_slice() else {
        return None;
    };
    let leading_width = *product_offset % 4;
    (*product_offset > leading_width).then_some(())?;
    let leading_value = (leading_width != 0).then(|| {
        bytes[..leading_width]
            .iter()
            .enumerate()
            .fold(0u32, |value, (shift, byte)| {
                value | (u32::from(*byte) << (shift * 8))
            })
    });
    let values = (0..(*product_offset - leading_width) / 4)
        .map(|index| View::u32_le_at(bytes, leading_width + index * 4).expect("four-byte chunk"))
        .collect();
    Some(OffsetStoreControlForm::ProductAnchored {
        leading_value: leading_value.map(|value| (leading_width, value)),
        values,
    })
}

/// One complete admitted offset-only store control-block form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OffsetStoreControlForm {
    /// Complete `00 + value:u24 LE` word array.
    ZeroPrefixed {
        /// Ordered values decoded from the complete control block.
        values: Vec<u32>,
    },
    /// Compact leading value and aligned `u32 LE` array preceding one
    /// self-framed product record.
    ProductAnchored {
        /// Width and value of the compact leading little-endian integer.
        leading_value: Option<(usize, u32)>,
        /// Ordered values preceding the product record.
        values: Vec<u32>,
    },
}

/// Classify one complete offset-only store control block atomically.
///
/// Exactly one admitted grammar must accept the complete control envelope.
pub fn offset_store_control_form(bytes: &[u8]) -> Option<OffsetStoreControlForm> {
    match (
        offset_store_control_values(bytes),
        offset_store_product_anchored_form(bytes),
    ) {
        (Some(values), None) => Some(OffsetStoreControlForm::ZeroPrefixed { values }),
        (None, Some(form)) => Some(form),
        _ => None,
    }
}

fn type_definitions(bytes: &[u8], start: usize, end: usize) -> Vec<TypeDefinition<'_>> {
    let mut out = Vec::new();
    let mut at = start;
    while at < end {
        let declared = usize::from(bytes[at]);
        let Some(length) = declared.checked_sub(1) else {
            at += 1;
            continue;
        };
        let name_start = at + 1;
        let name_end = name_start.saturating_add(length);
        let Some(raw) = bytes.get(name_start..name_end) else {
            at += 1;
            continue;
        };
        let valid = raw.starts_with(b"UGS::")
            && raw.iter().all(|byte| (0x20..0x7f).contains(byte))
            && name_end < end;
        if valid {
            let name = std::str::from_utf8(raw)
                .expect("invariant: validated printable ASCII is valid UTF-8");
            out.push(TypeDefinition {
                offset: at,
                name,
                trailing_code: bytes[name_end],
                registry_suffix: &[],
            });
            at = name_end + 1;
        } else {
            at += 1;
        }
    }
    for index in 0..out.len().saturating_sub(1) {
        let suffix_start = out[index].offset + out[index].name.len() + 2;
        let suffix_end = out[index + 1].offset;
        out[index].registry_suffix = &bytes[suffix_start..suffix_end];
    }
    out
}

fn field_definitions(bytes: &[u8], start: usize, end: usize) -> Vec<FieldDefinition<'_>> {
    let mut out = Vec::new();
    let mut search = start;
    let mut limit = start.saturating_add(256).min(end);
    while let Some((definition, at)) = (search..limit)
        .find_map(|at| field_definition_at(bytes, at, end).map(|definition| (definition, at)))
    {
        let next = at + definition.name.len() + 2;
        search = next;
        limit = search.saturating_add(256).min(end);
        out.push(definition);
    }
    bound_field_registry_suffixes(bytes, &mut out);
    out
}

fn all_field_definitions(bytes: &[u8], start: usize, end: usize) -> Vec<FieldDefinition<'_>> {
    let mut out = Vec::new();
    let mut at = start;
    while at < end {
        if let Some(definition) = field_definition_at(bytes, at, end) {
            at += definition.name.len() + 2;
            out.push(definition);
        } else {
            at += 1;
        }
    }
    bound_field_registry_suffixes(bytes, &mut out);
    out
}

fn bound_field_registry_suffixes<'a>(bytes: &'a [u8], definitions: &mut [FieldDefinition<'a>]) {
    for index in 0..definitions.len().saturating_sub(1) {
        let suffix_start = definitions[index].offset + definitions[index].name.len() + 2;
        let suffix_end = definitions[index + 1].offset;
        definitions[index].registry_suffix = &bytes[suffix_start..suffix_end];
    }
}

fn field_definition_at(bytes: &[u8], at: usize, end: usize) -> Option<FieldDefinition<'_>> {
    let declared = usize::from(*bytes.get(at)?);
    let length = declared.checked_sub(1)?;
    let name_start = at.checked_add(1)?;
    let name_end = name_start.checked_add(length)?;
    (name_end < end).then_some(())?;
    let raw = bytes.get(name_start..name_end)?;
    (raw.starts_with(b"m_") && raw.iter().all(|byte| (0x20..0x7f).contains(byte))).then_some(())?;
    Some(FieldDefinition {
        offset: at,
        name: std::str::from_utf8(raw).ok()?,
        trailing_code: bytes[name_end],
        registry_suffix: &[],
    })
}

fn numeric_expression_at(
    bytes: &[u8],
    base_offset: usize,
    object_id: Option<u32>,
) -> Option<NumericExpression<'_>> {
    const PREFIX: &[u8] = b"(Number [";
    let relative = bytes
        .windows(PREFIX.len())
        .position(|window| window == PREFIX)?;
    if relative < 3 || bytes.get(relative - 2) != Some(&0x04) {
        return None;
    }
    let declared = usize::from(*bytes.get(relative - 1)?);
    let text_len = declared.checked_sub(2)?;
    let text_end = relative.checked_add(text_len)?;
    (bytes.get(text_end) == Some(&0)).then_some(())?;
    let text = std::str::from_utf8(bytes.get(relative..text_end)?).ok()?;
    text.ends_with("; ").then_some(())?;
    let text = text.strip_prefix("(Number [")?;
    let (unit, rest) = text.split_once("]) ")?;
    let unit = match unit {
        "mm" => ExpressionUnit::Millimeter,
        "degrees" => ExpressionUnit::Degree,
        _ => return None,
    };
    let (name, value_tail) = rest.split_once(": ")?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return None;
    }
    let value_text = value_tail.strip_suffix("; ")?;
    let (parameter_index, qualifier) = parameter_name_parts(name)
        .map_or((None, None), |(index, qualifier)| (Some(index), qualifier));
    let value = evaluate_constant_expression(value_text);
    Some(NumericExpression {
        object_id,
        offset: base_offset + relative,
        name,
        parameter_index,
        qualifier,
        unit,
        expression: value_text,
        value,
    })
}

/// Evaluate the context-free arithmetic subset of NX numeric formulas.
/// Names and function calls fail; they need the parameter graph.
pub(crate) fn evaluate_constant_expression(text: &str) -> Option<f64> {
    struct Parser<'a> {
        bytes: &'a [u8],
        at: usize,
    }

    impl Parser<'_> {
        fn spaces(&mut self) {
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
                self.at += 1;
            }
        }

        fn take(&mut self, byte: u8) -> bool {
            self.spaces();
            if self.bytes.get(self.at) == Some(&byte) {
                self.at += 1;
                true
            } else {
                false
            }
        }

        fn sum(&mut self) -> Option<f64> {
            let mut value = self.product()?;
            loop {
                if self.take(b'+') {
                    value += self.product()?;
                } else if self.take(b'-') {
                    value -= self.product()?;
                } else {
                    return value.is_finite().then_some(value);
                }
            }
        }

        fn product(&mut self) -> Option<f64> {
            let mut value = self.unary()?;
            loop {
                if self.take(b'*') {
                    value *= self.unary()?;
                } else if self.take(b'/') {
                    value /= self.unary()?;
                } else {
                    return value.is_finite().then_some(value);
                }
            }
        }

        fn power(&mut self) -> Option<f64> {
            let value = self.primary()?;
            if self.take(b'^') {
                let result = value.powf(self.unary()?);
                result.is_finite().then_some(result)
            } else {
                Some(value)
            }
        }

        fn unary(&mut self) -> Option<f64> {
            if self.take(b'+') {
                self.unary()
            } else if self.take(b'-') {
                Some(-self.unary()?)
            } else {
                self.power()
            }
        }

        fn primary(&mut self) -> Option<f64> {
            if self.take(b'(') {
                let value = self.sum()?;
                self.take(b')').then_some(value)
            } else {
                self.number()
            }
        }

        fn number(&mut self) -> Option<f64> {
            self.spaces();
            let start = self.at;
            while self
                .bytes
                .get(self.at)
                .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
            {
                self.at += 1;
            }
            if self
                .bytes
                .get(self.at)
                .is_some_and(|byte| matches!(byte, b'e' | b'E'))
            {
                self.at += 1;
                if self
                    .bytes
                    .get(self.at)
                    .is_some_and(|byte| matches!(byte, b'+' | b'-'))
                {
                    self.at += 1;
                }
                let exponent = self.at;
                while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                    self.at += 1;
                }
                (self.at > exponent).then_some(())?;
            }
            (self.at > start).then_some(())?;
            std::str::from_utf8(&self.bytes[start..self.at])
                .ok()?
                .parse()
                .ok()
        }
    }

    let mut parser = Parser {
        bytes: text.as_bytes(),
        at: 0,
    };
    let value = parser.sum()?;
    parser.spaces();
    (parser.at == parser.bytes.len() && value.is_finite()).then_some(value)
}

/// Parse one complete canonical `p<decimal>[_qualifier]` parameter name.
pub(crate) fn parameter_name_parts(name: &str) -> Option<(u32, Option<&str>)> {
    let tail = name.strip_prefix('p')?;
    let digit_count = tail.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 {
        return None;
    }
    let index = tail[..digit_count].parse().ok()?;
    match &tail[digit_count..] {
        "" => Some((index, None)),
        suffix => {
            let qualifier = suffix.strip_prefix('_').filter(|qualifier| {
                !qualifier.is_empty()
                    && qualifier
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            });
            qualifier.map(|qualifier| (index, Some(qualifier)))
        }
    }
}

#[cfg(test)]
mod tests;
