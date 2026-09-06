// SPDX-License-Identifier: Apache-2.0
//! Frame NX object-model entities using external boundary and identity arrays.

pub(crate) mod draft_identity;
pub(crate) mod plane_descriptor;
pub(crate) mod csys_descriptor;
pub(crate) mod reference_index;
pub(crate) mod instances;
use reference_index::ReferenceIndexToken;
pub(crate) mod control_word;
use control_word::ControlWord24;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::num::NonZeroU8;

use cadmpeg_core::decode::{alloc_filled, View};
use crate::printable_string::PrintableString;

pub(crate) mod compact;
use compact::{CompactIndexAtom, WrappedCompactIndex, LocatedCompactIndex, NullableCompactIndex, CountedIndexMembers};
pub(crate) mod color;
use color::{ColorComponent, PaletteIndex, PALETTE_SIZE, BACKGROUND_NAME};
pub(crate) mod branch_items;
pub(crate) mod discriminators;
use branch_items::BranchItems;
pub(crate) mod parameter_name;
pub(crate) mod scalar_pair;
use scalar_pair::{SketchPairForm, DatumPairForm};
pub(crate) mod scalar_run;
use scalar_run::FramedScalarRun;
pub(crate) mod sketch_scalar;
use sketch_scalar::{SketchScaledAtom, SketchMixedScalars, SketchScalarLaneForm};
pub(crate) mod fixed;
use fixed::{Q155, Q155Atom, Q155Marker, Q155LaneFrame};
pub(crate) mod nonempty;
pub(crate) mod state_tagged_value;
pub(crate) mod state_index;
pub(crate) mod state_slots;
pub(crate) mod source_span;
use source_span::SourceSpan;
use state_slots::StateSlots;
pub(crate) mod state_link;
use state_link::StateLinkCode;
pub(crate) mod state_group;
use state_group::{OperationStateGroupCount, OperationStateGroupOpener, StateGroupMembers};
use state_index::{OperationStateIndex, NonNullStateIndex};
pub(crate) mod state_message_text;
use state_message_text::StateMessageText;
use state_tagged_value::StateTaggedValue;
use nonempty::NonEmpty;
pub(crate) mod pattern;
use pattern::{PatternRow, PatternRows, PatternTerminal, PatternValue, PatternWideValues};
pub(crate) mod swp104_state;
pub(crate) mod scalar;
use scalar::{LocatedBinary64, PayloadScalarAtom, RepeatedScalar, ShiftedBinary32, ShiftedBinary64, ShiftedScalar, shifted_ieee_f64};
pub(crate) mod thru_curve_endings;
pub(crate) mod thru_curve_controls;
use thru_curve_controls::ThruCurveControls;
pub(crate) mod thru_curve_state;
use thru_curve_state::ThruCurveBranchItems;
use thru_curve_endings::{ThruCurveBranchSuffix, ThruCurveGroupTerminator};
use swp104_state::Swp104StateLane;
use discriminators::{
    DraftBinary32Branch, OperationStateCounterKind, OperationStatePairTag,
};
pub(crate) mod registry;
pub(crate) mod cache;
pub(crate) mod product;
pub(crate) mod audit;
use audit::{AuditRecord, AuditTrailRow};
pub(crate) mod control_leading_value;
use control_leading_value::ControlLeadingValue;
use product::{ProductRecord, ProductRecordForm, ProductText};
mod index_table;
use index_table::{DescendingU32Edges, FixedIndex, OffsetIndex};
use parameter_name::ParameterName;

/// One NX object-model entity payload without a fixed object-id table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRecord<'a> {
    /// Absolute byte offset of the entity payload.
    pub offset: usize,
    /// Exactly bounded serialized entity payload.
    pub bytes: &'a [u8],
}

/// One NX object-model entity in a fixed-width object-id table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedEntityRecord<'a> {
    /// Object identifier and the absolute offset of its table word.
    pub object_id: (u32, u64),
    /// Absolute byte offset of the entity payload.
    pub offset: usize,
    /// Exactly bounded serialized entity payload.
    pub bytes: &'a [u8],
}

/// How one indexed section stores entity identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexedStore<'a> {
    /// Every record carries a fixed-width object id.
    Fixed {
        /// Entity records following the reserved zero-offset slot.
        records: Arc<[FixedEntityRecord<'a>]>,
    },
    /// Identity lives in the control block and column storage.
    OffsetOnly {
        /// Store-level control block bounded by slot zero.
        control: EntityRecord<'a>,
        /// Contiguous column-storage region after the control block.
        column_storage: &'a [u8],
        /// Entity records following the reserved zero-offset slot.
        records: Arc<[EntityRecord<'a>]>,
    },
}

/// One length-framed NX object-model class definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDefinition<'a> {
    /// Absolute byte offset of the definition's length byte.
    pub offset: usize,
    /// Registered `UGS::` class name.
    pub name: &'a str,
    /// Complete registry bytes following the class name.
    pub registry_tail: &'a [u8],
}

/// One member declaration in an NX OM field registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDefinition<'a> {
    /// Offset of the declaration length byte.
    pub offset: usize,
    /// Registered `m_` member name.
    pub name: &'a str,
    /// Complete registry bytes following the member name.
    pub registry_tail: &'a [u8],
}

/// One self-framed printable string value in an NX OM entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringValue<'a> {
    /// Absolute byte offset of the `66 32 03` marker.
    pub offset: usize,
    /// Printable value bytes.
    pub value: PrintableString<&'a str>,
}

/// One canonical UUID in the compact NX OM string frame `03 26, text, 00`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UuidStringValue<'a> {
    /// Absolute byte offset of the `03 26` marker.
    pub offset: usize,
    /// Canonical lowercase UUID text.
    pub value: crate::canonical_uuid::CanonicalUuid<&'a str>,
}

/// One self-framed printable string in a surface-referenced payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfacePayloadString<'a> {
    /// Payload-relative offset of the `66 1b 03` marker.
    pub offset: usize,
    /// Exact non-empty string value.
    pub value: crate::payload_text::PayloadText<&'a str>,
}

/// Self-framed NX product/version marker in an OM store root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreVersion<'a> {
    /// Absolute offset of the `04 01` marker.
    pub offset: usize,
    /// Exact printable product/version text, including the `NX ` prefix.
    pub value: ProductText<&'a str>,
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
struct CompactToken<T = CompactIndex> {
    value: T,
    offset: usize,
    width: usize,
}

fn compact_index(bytes: &[u8]) -> Option<(CompactIndex, usize)> {
    let token = NullableCompactIndex::read(bytes, 0)?;
    let value = match token.atom {
        None => CompactIndex::Null,
        Some(atom) => CompactIndex::Value(atom.value()),
    };
    Some((value, token.raw().len()))
}

fn compact_token(bytes: &[u8], offset: usize) -> Option<CompactToken> {
    let (value, width) = compact_index(bytes.get(offset..)?)?;
    Some(CompactToken {
        value,
        offset,
        width,
    })
}

fn raw_compact_token<T>(bytes: &[u8], token: CompactToken<T>) -> Vec<u8> {
    bytes[token.offset..token.offset + token.width].to_vec()
}

/// One counted compact-index lane ending in the exact `01 11` marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetStoreCountedIndexLane {
    /// Byte offset of the opening `01` marker.
    pub offset: usize,
    /// Non-null compact index immediately following the count.
    pub anchor: LocatedCompactIndex,
    /// Ordered non-null compact indices preceding the terminator.
    pub members: CountedIndexMembers<LocatedCompactIndex>,
}

/// Fixed-width nullable block-index lane terminated by the literal `ABR` tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetStoreAbrReferenceLane {
    /// Byte offset of the opening `11` marker.
    pub offset: usize,
    /// Sixteen ordered nullable compact indices and their byte offsets.
    pub slots: [NullableCompactIndex; 16],
}

/// One self-framed index row in contiguous offset-store column storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetStoreIndexRow {
    /// Byte offset of the opening `2d 02 0b` discriminator.
    pub offset: usize,
    /// First non-null compact index.
    pub first_index: LocatedCompactIndex,
    /// Serialized row flag.
    pub flag: crate::om::discriminators::LinkedIndexFlag,
    /// Four ordered non-null compact indices after the row flag.
    pub indices: [LocatedCompactIndex; 4],
}

/// One self-framed linked index row in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetStoreLinkedIndexRow {
    /// Byte offset of the opening `02 0b` discriminator.
    pub offset: usize,
    /// Unresolved leading compact index and its byte offset.
    pub first_index: LocatedCompactIndex,
    /// Serialized `16`, `17`, or `18` row discriminator.
    pub discriminator: crate::om::discriminators::LinkedIndexDiscriminator,
    /// Compact target index and its byte offset.
    pub target_index: LocatedCompactIndex,
    /// Three ordered non-null compact indices after `ff ff 90 fe`.
    pub indices: [LocatedCompactIndex; 3],
    /// Serialized `03` or `07` row flag.
    pub flag: crate::om::discriminators::LinkedIndexFlag,
    /// Serialized `04` or `07` row mode.
    pub mode: crate::om::discriminators::IndexRowMode,
}

/// Canonical NX color-index token immediately preceding a display row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedRowColorIndex {
    /// One-based part palette index.
    pub color_index: PaletteIndex,
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
    for width in [1, 2] {
        let Some(offset) = row_offset.checked_sub(width) else { continue; };
        let Some(prefix_offset) = offset.checked_sub(PRECEDING_SUFFIX.len()) else { continue; };
        if bytes.get(prefix_offset..offset) != Some(&PRECEDING_SUFFIX) { continue; }
        if let Some(color_index) = PaletteIndex::read_display(bytes.get(offset..row_offset)?) {
            return Some(LinkedRowColorIndex { color_index, offset });
        }
    }
    None
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
        assert_eq!(color.color_index.value(), 201);
        assert_eq!(color.color_index.display_raw(), [0x80, 201]);

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
        assert_eq!(color.color_index.value(), 201);
        assert_eq!(color.color_index.display_raw(), [0x80, 201]);
    }
}

/// One self-framed target-index row in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetStoreTargetIndexRow {
    /// Byte offset of the opening `02 01 01 01 16` discriminator.
    pub offset: usize,
    /// Compact target index and its byte offset.
    pub target_index: LocatedCompactIndex,
    /// Three ordered non-null compact indices after `ff ff 90 fe`.
    pub indices: [LocatedCompactIndex; 3],
    /// Serialized `04` or `07` row mode.
    pub mode: crate::om::discriminators::IndexRowMode,
}

/// One RGB definition from an NX part color table.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorTableDefinition<'a> {
    /// Color name paired by table order.
    pub name: &'a str,
    /// Exact normalized components and their payload offsets.
    pub components: [(ColorComponent, usize); 3],
    /// Byte offset of the opening `05` marker.
    pub offset: usize,
}

/// Complete 216-entry NX part color table.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorTable<'a> {
    /// Byte offset of the counted name roster.
    pub offset: usize,
    /// Exact background components and their payload offsets.
    pub background: [(ColorComponent, usize); 3],
    /// Ordered definitions for color indices 1 through 216.
    pub definitions: [ColorTableDefinition<'a>; PALETTE_SIZE],
}

fn color_components(bytes: &[u8], at: &mut usize) -> Option<[(ColorComponent, usize); 3]> {
    (0..3).map(|_| {
        let offset = *at;
        let component = ColorComponent::read(bytes.get(offset..)?)?;
        *at += component.raw().len();
        Some((component, offset))
    }).collect::<Option<Vec<_>>>()?.try_into().ok()
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
        if ordinal == 0 && name != BACKGROUND_NAME {
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
        at += ColorComponent::read(bytes.get(at..)?)?.raw().len();
    }
    for color_index in PaletteIndex::all() {
        if bytes.get(at) != Some(&0x05) {
            return None;
        }
        at += 1;
        let (token, width) = color_index.definition_token();
        if bytes.get(at..at + width) != Some(&token[..width]) { return None; }
        at += width;
        if bytes.get(at..at + 3) != Some(&[0x01, 0x80, 0xc8]) {
            return None;
        }
        at += 3;
        for _ in 0..3 {
            at += ColorComponent::read(bytes.get(at..)?)?.raw().len();
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

    let background = color_components(bytes, &mut at)?;
    let mut definitions = Vec::with_capacity(PALETTE_SIZE);
    for color_index in PaletteIndex::all() {
        let offset = at;
        at += 1 + color_index.definition_token().1 + 3;
        let components = color_components(bytes, &mut at)?;
        definitions.push(ColorTableDefinition {
            name: names[usize::from(color_index.value())],
            components,
            offset,
        });
    }
    Some(ColorTable {
        offset: start,
        background,
        definitions: definitions.try_into().ok()?,
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
        let Some(first_token) = LocatedCompactIndex::read(bytes, first_index_offset) else {
            start += 1;
            continue;
        };
        let marker = first_token.offset + first_token.atom.raw().len();
        if bytes.get(marker..marker + 2) != Some(&MIDDLE[..2]) {
            start += 1;
            continue;
        }
        let Some(flag) = bytes.get(marker + 2).copied().and_then(|value| discriminators::LinkedIndexFlag::try_from(value).ok()) else {
            start += 1;
            continue;
        };
        let mut at = marker + 3;
        let Some(index_tokens) = LocatedCompactIndex::read_array(bytes, &mut at) else {
            start += 1;
            continue;
        };
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
            first_index: first_token,
            flag,
            indices: index_tokens,
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
        let Some(first_token) = LocatedCompactIndex::read(bytes, first_offset) else {
            start += 1;
            continue;
        };
        let marker = first_token.offset + first_token.atom.raw().len();
        if bytes.get(marker..marker + 2) != Some(&[0x93, 0x8c]) {
            start += 1;
            continue;
        }
        let Some(discriminator) = bytes
            .get(marker + 2)
            .copied()
            .and_then(|value| discriminators::LinkedIndexDiscriminator::try_from(value).ok())
        else {
            start += 1;
            continue;
        };
        let target_offset = marker + 3;
        let Some(target_token) = LocatedCompactIndex::read(bytes, target_offset) else {
            start += 1;
            continue;
        };
        let mut at = target_token.offset + target_token.atom.raw().len();
        if bytes.get(at..at + MIDDLE.len()) != Some(&MIDDLE) {
            start += 1;
            continue;
        }
        at += MIDDLE.len();
        let Some(index_tokens) = LocatedCompactIndex::read_array(bytes, &mut at) else {
            start += 1;
            continue;
        };
        if bytes.get(at..at + 2) != Some(&[0x00, 0x47]) {
            start += 1;
            continue;
        }
        let Some(flag) = bytes
            .get(at + 2)
            .copied()
            .and_then(|value| discriminators::LinkedIndexFlag::try_from(value).ok())
        else {
            start += 1;
            continue;
        };
        let Some(mode) = bytes
            .get(at + 3)
            .copied()
            .and_then(|value| discriminators::IndexRowMode::try_from(value).ok())
        else {
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
            first_index: first_token,
            discriminator,
            target_index: target_token,
            indices: index_tokens,
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
        let Some(target_token) = LocatedCompactIndex::read(bytes, target_offset) else {
            start += 1;
            continue;
        };
        let mut at = target_token.offset + target_token.atom.raw().len();
        if bytes.get(at..at + MIDDLE.len()) != Some(&MIDDLE) {
            start += 1;
            continue;
        }
        at += MIDDLE.len();
        let Some(index_tokens) = LocatedCompactIndex::read_array(bytes, &mut at) else {
            start += 1;
            continue;
        };
        if bytes.get(at..at + 3) != Some(&[0x00, 0x47, 0x03]) {
            start += 1;
            continue;
        }
        let Some(mode) = bytes
            .get(at + 3)
            .copied()
            .and_then(|value| discriminators::IndexRowMode::try_from(value).ok())
        else {
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
            target_index: target_token,
            indices: index_tokens,
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
        let tokens = (0..SLOT_COUNT).map(|_| {
            let token = NullableCompactIndex::read(bytes, at)?;
            at += token.raw().len();
            Some(token)
        }).collect::<Option<Vec<_>>>();
        let Some(tokens) = tokens.and_then(|tokens| tokens.try_into().ok()) else {
            start += 1;
            continue;
        };
        let Some(end) = at.checked_add(TERMINATOR.len()) else {
            start += 1;
            continue;
        };
        if bytes.get(at..end) == Some(&TERMINATOR) {
            lanes.push(OffsetStoreAbrReferenceLane { offset: start, slots: tokens });
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
    let decode = |start: usize| {
        (bytes.get(start) == Some(&0x01)).then_some(())?;
        let declared_count = *bytes.get(start + 1)?;
        (declared_count >= 3).then_some(())?;
        let anchor = LocatedCompactIndex::read(bytes, start + 2)?;
        let members_start = anchor.offset + anchor.atom.raw().len();
        let mut at = members_start;
        for _ in 0..usize::from(declared_count) - 2 {
            at += LocatedCompactIndex::read(bytes, at)?.atom.raw().len();
        }
        let end = at.checked_add(2)?;
        (bytes.get(at..end) == Some(&[0x01, 0x11])).then_some(())?;
        at = members_start;
        let members = (0..usize::from(declared_count) - 2)
            .map(|_| {
                let token = LocatedCompactIndex::read(bytes, at)?;
                at += token.atom.raw().len();
                Some(token)
            })
            .collect::<Option<Vec<_>>>()?;
        Some((
            OffsetStoreCountedIndexLane {
                offset: start,
                anchor,
                members: CountedIndexMembers::new(members).ok()?,
            },
            end,
        ))
    };
    let mut lanes = Vec::new();
    let mut start = 0;
    while start + 4 <= bytes.len() {
        if let Some((lane, end)) = decode(start) {
            lanes.push(lane);
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
    /// Checked shifted-binary64 atom.
    pub scalar: ShiftedBinary64,
}

const SHIFTED_BINARY64_SCALAR_FRAME_LEN: usize = 13;

/// Decode exact `50 59 66, field_code, 00, shifted-f64` construction fields.
pub fn construction_payload_scalar_fields(bytes: &[u8]) -> Vec<ConstructionPayloadScalarField> {
    let mut fields = Vec::new();
    for start in 0..bytes.len().saturating_sub(12) {
        if bytes.get(start..start + 3) != Some(b"PYf")
            || bytes.get(start + 4) != Some(&0x00)
        {
            continue;
        }
        let Some(scalar) = bytes
            .get(start + 5..start + SHIFTED_BINARY64_SCALAR_FRAME_LEN)
            .and_then(ShiftedBinary64::read)
        else {
            continue;
        };
        fields.push(ConstructionPayloadScalarField {
            offset: start,
            field_code: bytes[start + 3],
            scalar,
        });
    }
    fields
}

/// Compact type code on a construction payload name that is not payload-leading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstructionPayloadTypeCode {
    /// Decoded non-null compact type code following the `66` marker.
    pub value: u32,
    /// Exact compact type-code token.
    pub raw: Vec<u8>,
    /// Payload-relative compact type-code offset.
    pub offset: usize,
}

/// One compact-code string field in a reconstructed construction payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstructionPayloadNamedField<'a> {
    /// Payload-relative offset of the `66` marker.
    pub offset: usize,
    /// Compact type code, absent for the type-free payload-leading form.
    pub type_code: Option<ConstructionPayloadTypeCode>,
    /// Exact nonempty printable ASCII value.
    pub value: &'a str,
}

impl ConstructionPayloadNamedField<'_> {
    /// Whether the field uses the type-free payload-leading form.
    pub fn payload_leading(&self) -> bool {
        self.type_code.is_none()
    }
}

/// Exact type-free named point record spanning consecutive store blocks.
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetStoreNamedPoint {
    /// Exact `Point<positive decimal>` name.
    pub name: String,
    /// Two checked scalar atoms and their frame offsets in block order.
    pub values: [LocatedBinary64; 2],
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
                .is_some_and(|name| name.payload_leading())
        {
            return candidate;
        }
        bytes.extend_from_slice(block);
        let names = construction_payload_named_fields(&bytes);
        let name = names.first()?;
        if !name.payload_leading() || parse_positive_decimal_suffix(name.value, "Point").is_none() {
            return None;
        }
        let next_name = names
            .iter()
            .find(|next| !next.payload_leading() && next.offset > name.offset);
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
                    values: [first_scalar, second_scalar].map(|field| LocatedBinary64 {
                        scalar: field.scalar,
                        offset: field.offset,
                    }),
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
            type_code: Some(ConstructionPayloadTypeCode {
                value: type_code,
                raw: bytes[start + 1..marker].to_vec(),
                offset: start + 1,
            }),
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpressionUnit {
    /// Model length in millimeters as serialized by NX.
    Millimeter,
    /// Model length in inches as serialized by NX.
    Inch,
    /// Angular value in degrees as serialized by NX.
    Degree,
    /// Unit label without a neutral dimensional mapping.
    Native(String),
}

/// One numeric expression decoded from an exactly bounded OM entity.
#[derive(Debug, Clone, PartialEq)]
pub struct NumericExpression<'a> {
    /// Persistent identity of the containing OM entity, when indexed.
    pub object_id: Option<u32>,
    /// Absolute byte offset of the expression text.
    pub offset: usize,
    /// NX parameter name.
    pub name: ParameterName<&'a str>,
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
    /// Identity store used by this section.
    pub store: IndexedStore<'a>,
}

/// Internally pointed record-area bytes with their absolute offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordArea<'a> {
    /// Absolute offset of the record-area start.
    pub offset: usize,
    /// Exact record-area bytes, including the 12-byte control prefix.
    pub bytes: &'a [u8],
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
    /// Internally pointed record area, when the section carries one.
    pub record_area: Option<RecordArea<'a>>,
    /// Operation labels decoded while the section's record area is framed.
    cached_operation_labels: Arc<[OperationLabel<'a>]>,
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

/// One operation record bounded by consecutive validated operation headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationRecord<'a> {
    /// Complete record bytes through the next operation header or section end.
    pub bytes: &'a [u8],
    /// Absolute offset of the first byte after the operation-label terminator.
    pub payload_offset: usize,
    /// Post-label serialized operation payload.
    pub payload: &'a [u8],
    /// Label decoded from this record's header.
    pub label: OperationLabel<'a>,
}

impl OperationRecord<'_> {
    /// Absolute offset of the fixed operation-header marker.
    pub fn offset(&self) -> usize {
        self.label.header_offset
    }
}

/// One unlabeled operation record bounded by validated operation headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnlabeledOperationRecord<'a> {
    /// Absolute offset of the fixed operation-header marker.
    pub offset: usize,
    /// Complete record bytes through the next operation header or section end.
    pub bytes: &'a [u8],
    /// Absolute offset of the first byte after the four header slots.
    pub payload_offset: usize,
    /// Serialized payload after the four header slots.
    pub payload: &'a [u8],
    /// Four object-index slots in header order; `None` is the `ff` sentinel.
    pub object_indices: [Option<u32>; 4],
    /// Absolute byte offset of each object-index token in header order.
    pub object_index_offsets: [usize; 4],
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

/// One row in the operation-state object counter map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationStateCounter {
    /// Checked source position within its record area.
    pub span: SourceSpan,
    /// Row-kind byte following `05`; modern files use `01` and `02`.
    pub row_kind: OperationStateCounterKind,
    /// Object whose state-counter pair is recorded.
    pub object_index: NonNullStateIndex,
    /// Journal state at which the object was introduced.
    pub introduced_state: u8,
    /// Journal state at which the object was last modified.
    pub modified_state: u8,
}

/// Contiguous object state-counter map at the end of a feature-history area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationStateCounterMap<'a> {
    /// Absolute byte offset of the first counter row.
    pub offset: usize,
    /// Absolute byte offset after the final counter row.
    pub end_offset: usize,
    /// Rows in serialized order.
    pub rows: Vec<OperationStateCounter>,
    /// Exact bytes after the counter rows within the bounded record area.
    pub trailing_bytes: &'a [u8],
}

/// One diagnostic/message record in the operation-state block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationStateMessage<'a> {
    /// Checked source position within its record area.
    pub span: SourceSpan,
    /// Exact byte-length-framed ASCII Part Navigator text.
    pub text: StateMessageText<&'a str>,
    /// Tagged value following the four zero bytes.
    pub value: StateTaggedValue,
    /// Big-endian count or severity word following the tagged value.
    pub count_or_severity: u16,
}

/// Payload form of one per-object operation-state status row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStateStatusPayload<'a> {
    /// Normal built/healthy object state, encoded as `3f`.
    Plain,
    /// Status with a link-code and one linked object index.
    Linked {
        /// Serialized link discriminator.
        link_code: StateLinkCode,
        /// Linked object index between the two `ff` sentinels.
        object_index: NonNullStateIndex,
    },
    /// Status carrying the exact inline diagnostic record.
    Diagnostic {
        /// Inline diagnostic record beginning at the payload's `03` marker.
        message: OperationStateMessage<'a>,
    },
    /// A typed status code whose payload lane has no settled subgrammar.
    Opaque {
        /// Exact bytes from the first payload byte through its lane terminator.
        raw: &'a [u8],
    },
}

/// One per-object status row in the operation-state block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationStateStatus<'a> {
    /// Checked source position within its record area.
    pub span: SourceSpan,
    /// Exact non-null status-code token and decoded value.
    pub status_code: NonNullStateIndex,
    /// Object carrying this status.
    pub object_index: NonNullStateIndex,
    /// Status payload, retained without naming suppression codes.
    pub payload: OperationStateStatusPayload<'a>,
}

/// A bounded sequence of operation-state status rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationStateStatusTable<'a> {
    /// Absolute byte offset of the first status row.
    pub offset: usize,
    /// Absolute byte offset after the final complete status row.
    pub end_offset: usize,
    /// Rows in serialized order.
    pub rows: Vec<OperationStateStatus<'a>>,
    /// Standalone feature-record slot lanes following the status rows.
    pub slot_lanes: Vec<OperationStateSlotLane>,
    /// Exact bounded bytes after the last complete status row.
    pub trailing_bytes: &'a [u8],
}

/// One standalone feature-record slot lane in the operation-state block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationStateSlotLane {
    /// Checked source position within its record area.
    pub span: SourceSpan,
    /// Null or object-index slots in serialized order.
    pub slots: StateSlots<OperationStateIndex>,
}

struct OperationStateBlock<'a> {
    offset: usize,
    status_end_offset: usize,
    rows: Vec<OperationStateStatus<'a>>,
    slot_lanes: Vec<OperationStateSlotLane>,
    messages: Vec<OperationStateMessage<'a>>,
}

/// One row in an `m_rollForwardStates` group table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStateGroupRow {
    /// `4a object_index position ff` list member. The common position is a
    /// direct byte; one generation uses the same compact token family as an
    /// object index for positions above the direct range.
    List {
        /// Absolute byte offset of the row's `4a` marker.
        offset: usize,
        /// Ordered feature-record member.
        object_index: NonNullStateIndex,
        /// Serialized list-position token.
        position: NonNullStateIndex,
    },
    /// `tag object_index object_index ff ff` relation member.
    Pair {
        /// Absolute byte offset of the row's relation tag.
        offset: usize,
        /// Schema-generation relation tag (`4f` or `48`).
        tag: OperationStatePairTag,
        /// First relation endpoint.
        first: NonNullStateIndex,
        /// Second relation endpoint.
        second: NonNullStateIndex,
    },
}

/// One counted `m_rollForwardStates` group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationStateGroup {
    /// Checked source position within its record area.
    pub span: SourceSpan,
    /// Admitted two-byte group opener.
    pub opener: OperationStateGroupOpener,
    /// Ordered rows with their exact count-header form.
    pub members: StateGroupMembers<OperationStateGroupRow>,
}

/// A bounded sequence of `m_rollForwardStates` groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationStateGroupTable<'a> {
    /// Absolute byte offset of the first group.
    pub offset: usize,
    /// Absolute byte offset after the final group.
    pub end_offset: usize,
    /// Groups in serialized order.
    pub groups: Vec<OperationStateGroup>,
    /// Exact table-boundary bytes after the final complete group.
    pub trailing_bytes: &'a [u8],
}

/// One state-journal row preceding feature operation records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationStateJournalRow {
    /// Checked source position within its record area.
    pub span: SourceSpan,
    /// Big-endian Unix timestamp.
    pub timestamp: u32,
    /// Tagged schema value stored by the journal.
    pub value: StateTaggedValue,
    /// Schema identifier varint.
    pub schema_id: NonNullStateIndex,
    /// Monotone state ordinal varint.
    pub ordinal: NonNullStateIndex,
}

/// One state-journal group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationStateJournalGroup {
    /// Checked source position within its record area.
    pub span: SourceSpan,
    /// Two opener selector bytes.
    pub selector: [u8; 2],
    /// Journal rows in serialized order.
    pub rows: Vec<OperationStateJournalRow>,
}

/// One length-framed UTF-8 string in a bounded operation payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationPayloadString<'a> {
    /// Absolute offset of the `04` marker.
    pub offset: usize,
    /// Exact non-empty string value.
    pub value: crate::payload_text::PayloadText<&'a str>,
}

/// Marker selecting a bounded operation text frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationTextMarker {
    Text,
    String,
}

/// One length-framed UTF-8 text frame in a bounded operation payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationPayloadTextFrame<'a> {
    /// Marker selecting the payload text-frame family.
    pub marker: OperationTextMarker,
    /// Absolute offset of the marker.
    pub offset: usize,
    /// Exact non-empty text value.
    pub value: crate::payload_text::PayloadText<&'a str>,
}

/// One canonical variable-width object index in an operation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadObjectReference<T = ReferenceIndexToken, O = usize> {
    /// Absolute offset of the width marker.
    pub offset: O,
    /// Checked token retaining the exact marker and width.
    pub token: T,
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
    pub token: Option<reference_index::ReferenceIndexToken>,
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

/// One exact counted transform lane in a pattern operation payload.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternPayloadTransformLane {
    /// Absolute offset of the opening `01, count` field.
    pub offset: usize,
    /// Schema index framing every row in the lane.
    pub row_schema_index: NonZeroU8,
    pub rows: PatternRows<LocatedCompactIndex, usize>,
}

/// Exact counted instance-output lane in a multi-instance operation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiInstanceOutputPayloadLane {
    /// Absolute offset of the opening `25 01, count` field.
    pub offset: usize,
    /// Complete selector groups and their trailing references.
    pub outputs: instances::MultiInstanceOutputs<usize>,
}

/// Count schema index with room for the three consecutive selector-row indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdenticalInstanceSchemaIndex(u8);

impl IdenticalInstanceSchemaIndex {
    pub fn new(value: u8) -> Option<Self> {
        value.checked_add(3).map(|_| Self(value))
    }

    pub fn value(self) -> u8 {
        self.0
    }

    pub fn row_indices(self) -> [u8; 3] {
        [self.0 + 1, self.0 + 2, self.0 + 3]
    }
}

/// Exact counted selector lane in an identical-instance output payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdenticalInstanceOutputPayloadLane {
    /// Absolute offset of the leading schema index.
    pub offset: usize,
    /// Schema index preceding the count field.
    pub leading_schema_index: u8,
    /// Schema index framing the serialized count.
    pub count_schema_index: IdenticalInstanceSchemaIndex,
    /// Ordered non-null compact selectors with their exact source tokens.
    pub selectors: compact::CountedIndexMembers<LocatedCompactIndex>,
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
    /// Six checked shifted-binary64 atoms in byte order.
    pub values: [ShiftedBinary64; 6],
    /// Start of the contiguous lane in the joined blocks.
    offset: usize,
}

impl PointFeatureScalarLane {
    pub fn value_offsets(&self) -> [usize; 6] {
        std::array::from_fn(|i| self.offset + i * 8)
    }
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
    /// Non-null compact indices in serialized order with absolute token offsets.
    pub indices: CountedIndexMembers<LocatedCompactIndex, 1>,
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
}

/// Exact common construction references in a surface-feature payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceFeaturePayloadReferenceField {
    /// Eleven header references followed by the trailing three references.
    pub references: [PayloadObjectReference; 14],
}

/// Exact leading construction references in a `THRU_CURVE` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThruCurvePayloadReferenceField {
    /// Nonzero construction discriminator at the payload start.
    pub discriminator: NonZeroU8,
    /// Exact opaque controls between the two reference groups.
    pub controls: ThruCurveControls,
    /// Three header references followed by six construction references.
    pub references: [PayloadObjectReference; 9],
    /// Nonzero control byte following the reference groups.
    pub trailing_control: NonZeroU8,
    /// Exact two-byte value selected by the `a0` marker.
    pub trailing_value: [u8; 2],
    /// Absolute offset immediately after the envelope.
    pub end_offset: usize,
}

/// One exact counted branch in a `THRU_CURVE` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThruCurvePayloadBranch {
    /// Absolute offset of the branch mode byte.
    pub offset: usize,
    /// Serialized nonzero branch mode.
    pub mode: NonZeroU8,
    /// Ordered nonterminal references.
    pub members: ThruCurveBranchItems<PayloadObjectReference>,
    /// Terminal reference.
    pub terminal: PayloadObjectReference,
    /// Exact two-byte branch suffix.
    pub suffix: ThruCurveBranchSuffix,
}

/// Exact counted branch group after a `THRU_CURVE` reference envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThruCurvePayloadBranchGroup {
    /// Absolute offset of the serialized group count.
    pub offset: usize,
    /// Ordered explicit branches.
    pub branches: BranchItems<ThruCurvePayloadBranch>,
    /// Exact group terminator selected by the schema generation.
    pub terminator: ThruCurveGroupTerminator,
}

/// Exact leading construction branch in a `SWP104` payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Swp104PayloadLeadingBranch {
    /// Nonzero construction discriminator at the payload start.
    pub discriminator: NonZeroU8,
    /// Four finite shifted-binary64 values in serialized order.
    pub scalars: [ShiftedBinary64; 4],
    /// Whether one zero byte precedes the branch mode.
    pub leading_zero: bool,
    /// Serialized nonzero branch mode.
    pub mode: NonZeroU8,
    /// Exact state lane preceding the terminal marker.
    pub state_lane: Swp104StateLane,
    /// Ordered nonterminal references.
    pub members: BranchItems<PayloadObjectReference>,
    /// Terminal reference.
    pub terminal: PayloadObjectReference,
    /// Absolute offset immediately after the terminal zero.
    pub end_offset: usize,
}

/// One counted construction branch in a surface-feature payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceFeaturePayloadBranch {
    /// Absolute offset of the branch mode byte.
    pub offset: usize,
    /// Serialized `16` or `40` branch mode.
    pub mode: discriminators::SurfaceBranchMode,
    /// Whether the count is repeated before the zero lane.
    pub witnessed: bool,
    /// Ordered nonterminal references.
    pub members: BranchItems<PayloadObjectReference>,
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

/// One extrusion profile reference and its duplicate-list witness location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtrudeProfileReference {
    /// Exact primary reference token.
    pub reference: PayloadObjectReference,
    /// Location of this token in the unique byte-identical witness list.
    pub witness_offset: Option<usize>,
}

/// Ordered extrusion profile-reference field and its redundant witness state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtrudeProfileReferenceField {
    /// Serialized field tag between the relation marker and list marker.
    pub field_tag: u8,
    /// Ordered profile object indices, each with its witness when present.
    pub references: Vec<ExtrudeProfileReference>,
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
    /// Non-null compact descriptor with its absolute source offset.
    pub descriptor: LocatedCompactIndex,
    /// Canonical payload object reference with its absolute source offset.
    pub object: PayloadObjectReference<reference_index::PayloadIndexToken>,
}

/// Two canonical references carried by a tag-`29` datum-plane branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumPlaneDoubleReferenceBranch {
    /// Canonical payload object indices in branch order.
    pub references: [PayloadObjectReference<reference_index::PayloadIndexToken>; 2],
}

/// Complete terminal compact-index lane in a reconstructed datum-plane payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumPlaneObjectIndexLane<O = usize> {
    /// Payload-relative offset of the opening `01` marker.
    pub offset: O,
    /// Ordered non-null compact indices and their payload-relative offsets.
    pub indices: CountedIndexMembers<LocatedCompactIndex<O>, 1>,
    /// Big-endian trailer word after the zero separator.
    pub trailer: u32,
}

/// Exact scalar pair following a datum-plane object-record discriminator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DatumPlaneObjectScalarPair {
    /// Payload-relative offset of the discriminator.
    pub offset: usize,
    /// Checked scalar atoms with their payload-relative offsets.
    pub values: [LocatedBinary64; 2],
}

/// Exact scalar pair following an object or sketch discriminator.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectPayloadScalarPair {
    /// Payload-relative offset of the discriminator.
    pub offset: usize,
    /// Checked scalar atoms with their payload-relative offsets.
    pub values: [LocatedBinary64; 2],
    /// Exact discriminator selecting the scalar-pair branch.
    pub discriminator: Vec<u8>,
}

/// Exact pair of scaled shifted-binary64 atoms in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchPayloadFixedPair {
    /// Payload-relative offset of the discriminator.
    pub offset: usize,
    /// Ordered values reconstructed from the `30` shifted-binary64 atoms and scaled by `1/4`.
    pub values: [SketchScaledAtom; 2],
    /// Exact discriminator and branch prefix selecting the pair layout.
    pub form: SketchPairForm,
}

impl SketchPayloadFixedPair {
    pub fn discriminator(&self) -> &'static [u8] { self.form.discriminator() }
    pub fn value_offsets(&self) -> [usize; 2] {
        let first = self.offset + self.discriminator().len();
        [first, first + 8 + self.form.separator_width()]
    }
}

/// Exact mixed scaled shifted-binary64 and shifted-binary32 pair in a sketch payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchPayloadMixedPair {
    /// Payload-relative offset of the discriminator.
    pub offset: usize,
    /// Exact scaled binary64 and binary32 atoms.
    pub scalars: SketchMixedScalars,
}

impl SketchPayloadMixedPair {
    pub fn discriminator(&self) -> &'static [u8] { SketchPairForm::Legacy.discriminator() }
    pub fn value_offsets(&self) -> [usize; 2] {
        let first = self.offset + self.discriminator().len();
        [first, first + 8 + 1]
    }
}

/// Exact pair of signed Q1.55 atoms following a datum-CSYS branch discriminator.
#[derive(Debug, Clone, PartialEq)]
pub struct DatumCsysPayloadFixedPair {
    /// Payload-relative offset of the discriminator.
    pub offset: usize,
    /// Ordered dimensionless Q1.55 values.
    pub values: [Q155; 2],
    /// Exact discriminator selecting the pair branch.
    pub form: DatumPairForm,
}

impl DatumCsysPayloadFixedPair {
    pub fn discriminator(&self) -> &'static [u8] { self.form.discriminator() }
    pub fn value_offsets(&self) -> [usize; 2] {
        let first = self.offset + self.discriminator().len();
        [first, first + 8 + 1]
    }
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
    pub scalars: [ShiftedBinary64; 2],
}

/// Exact terminal discriminator lane at the end of a bounded operation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationTerminalDiscriminator {
    /// Payload-relative offset of the fixed footer prelude.
    pub offset: usize,
    /// Two compact type indices following `01 01 02`.
    pub type_indices: [LocatedCompactIndex; 2],
    /// Four serialized one-byte flags.
    pub flags: [u8; 4],
    /// Compact values between `29 29` and the terminal zero, with source tokens.
    pub trailing_indices: Vec<LocatedCompactIndex>,
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
    pub selector: NonZeroU8,
    /// Branch byte repeated between the two reference pairs.
    pub branch: NonZeroU8,
    /// Ordered first and second construction-block pairs.
    pub references: [PayloadObjectReference; 4],
}

/// One typed scalar in a bounded operation payload.
#[derive(Debug, Clone, PartialEq)]
pub struct PayloadScalar {
    /// Absolute offset of the scalar marker.
    pub offset: usize,
    /// Checked scalar atom with a derived value and width.
    pub atom: PayloadScalarAtom,
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
    /// Exact compact continuation index and its absolute source offset.
    pub continuation: LocatedCompactIndex,
    /// Exact required terminal reference and its absolute source offset.
    pub terminal: PayloadObjectReference,
}

/// Homogeneous checked references in an operation body lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationBodyReferenceLaneValues {
    CompactIndex(Vec<LocatedCompactIndex>),
    PayloadObjectIndex(Vec<PayloadObjectReference<reference_index::PayloadIndexToken>>),
}

/// Counted reference lane following an operation body scalar clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationBodyReferenceLane {
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    pub branch: discriminators::OperationBodyReferenceBranch,
    /// Ordered non-null lane values with their encoding.
    pub values: OperationBodyReferenceLaneValues,
}

/// Structured `32` branch following an extrusion body reference.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtrudePayload32Branch {
    /// Absolute offset of the `32` branch marker.
    pub offset: usize,
    /// Finite shifted-IEEE scalar following the branch marker.
    pub scalar: ShiftedBinary64,
    /// Fixed-width wrapped compact indices with their source words and offsets.
    pub atoms: Vec<LocatedCompactIndex<usize, WrappedCompactIndex>>,
    /// Ordered values in the first compact-index lane.
    pub first_indices: Vec<LocatedCompactIndex>,
    /// Ordered values in the second compact-index lane.
    pub second_indices: Vec<LocatedCompactIndex>,
    /// Exact required terminal reference and its absolute source offset.
    pub terminal: PayloadObjectReference,
}

/// Ordered construction-reference field at the start of a `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockConstructionReferenceField {
    /// Payload control byte preceding the field framing.
    pub control: u8,
    /// Eighteen leading references followed by the terminal reference.
    pub references: [PayloadObjectReference; 19],
}

/// Self-framed NX parameter name in one bounded expression declaration record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpressionDeclarationName<'a> {
    /// Byte offset of the `04` marker within the containing byte range.
    pub offset: usize,
    /// Exact `p<decimal>[_qualifier]` name.
    pub name: ParameterName<&'a str, u32>,
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

/// One exact body-write frame in a bounded operation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationBodyWriteFrame {
    /// Absolute offset of the opening `01 02` marker.
    pub offset: usize,
    /// Byte between the opening marker and the first object index.
    pub body_identity: u8,
    /// Partition-local Parasolid GROUP node.
    pub group_node: u32,
    /// Exact serialized GROUP-node token.
    pub raw_group_node: Vec<u8>,
    /// Absolute offset of the GROUP-node token.
    pub group_node_offset: usize,
    /// Tagged body-image field discriminator.
    pub endpoint_tag: u8,
    /// Offset-store body-image object index.
    pub body_image_object_index: u32,
    /// Exact serialized body-image object-index token.
    pub raw_body_image_object_index: Vec<u8>,
    /// Absolute offset of the body-image object-index token.
    pub body_image_object_index_offset: usize,
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
    /// Target body reference and its exact source token.
    pub target: PayloadObjectReference,
    /// Ordered tool body references and their exact source tokens.
    pub tools: Vec<PayloadObjectReference>,
}

impl<'a> IndexedSection<'a> {
    /// Return the section base used by its external record offsets.
    pub const fn base_offset(&self) -> usize {
        self.base
    }

    /// Fixed-width object-id records, when this section is that store.
    pub fn as_fixed(&self) -> Option<&[FixedEntityRecord<'a>]> {
        match &self.store {
            IndexedStore::Fixed { records } => Some(records.as_ref()),
            IndexedStore::OffsetOnly { .. } => None,
        }
    }

    /// Control block, column storage, and records of an offset-only store.
    pub fn as_offset_only(&self) -> Option<(&EntityRecord<'a>, &'a [u8], &[EntityRecord<'a>])> {
        match &self.store {
            IndexedStore::OffsetOnly {
                control,
                column_storage,
                records,
            } => Some((control, column_storage, records.as_ref())),
            IndexedStore::Fixed { .. } => None,
        }
    }

    fn record_views(&self) -> Vec<(usize, &'a [u8], Option<u32>)> {
        match &self.store {
            IndexedStore::Fixed { records } => records
                .iter()
                .map(|record| (record.offset, record.bytes, Some(record.object_id.0)))
                .collect(),
            IndexedStore::OffsetOnly { records, .. } => records
                .iter()
                .map(|record| (record.offset, record.bytes, None))
                .collect(),
        }
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
        let records = self.record_views();
        if !records.iter().any(|(_, bytes, _)| {
            bytes
                .windows(b"hostglobalvariables".len())
                .any(|window| window == b"hostglobalvariables")
        }) {
            return Vec::new();
        }
        records
            .into_iter()
            .enumerate()
            .filter_map(|(record_ordinal, (offset, bytes, object_id))| {
                numeric_expression_at(bytes, offset, object_id)
                    .map(|expression| (record_ordinal, expression))
            })
            .collect()
    }

    /// Decode every strictly framed printable string in each bounded record.
    pub fn string_values(&self) -> Vec<(usize, usize, Option<u32>, StringValue<'a>)> {
        self.record_views()
            .into_iter()
            .enumerate()
            .flat_map(|(record_ordinal, (offset, bytes, object_id))| {
                string_values(bytes, offset).into_iter().enumerate().map(
                    move |(value_ordinal, value)| (record_ordinal, value_ordinal, object_id, value),
                )
            })
            .collect()
    }

    /// Decode tagged cross-record references from every bounded record.
    pub fn references(&self) -> Vec<(usize, usize, Option<u32>, ReferenceValue)> {
        let records = self.record_views();
        let record_count = records.len();
        records
            .into_iter()
            .enumerate()
            .flat_map(|(record_ordinal, (offset, bytes, object_id))| {
                let mut references = record_references(bytes, offset);
                references.extend(counted_record_references(bytes, offset, record_count));
                references.sort_by_key(|reference| reference.offset);
                references
                    .into_iter()
                    .enumerate()
                    .map(move |(reference_ordinal, reference)| {
                        (record_ordinal, reference_ordinal, object_id, reference)
                    })
            })
            .collect()
    }
}

impl<'a> Section<'a> {
    fn record_area_parts(&self) -> Option<(usize, &'a [u8])> {
        self.record_area.map(|area| (area.offset, area.bytes))
    }

    /// Decode the validated record-area control and product header.
    pub fn record_area_header(&self) -> Option<RecordAreaHeader<'a>> {
        let (offset, bytes) = self.record_area_parts()?;
        let control_words = [
            View::u32_le_at(bytes, 0)?,
            View::u32_le_at(bytes, 4)?,
            View::u32_le_at(bytes, 8)?,
        ];
        let suffix = bytes.get(12..)?;
        let layout = ProductRecord::read(suffix, ProductRecordForm::Modern)
            .or_else(|| ProductRecord::read(suffix, ProductRecordForm::LegacyFeature))?;
        Some(RecordAreaHeader {
            offset,
            control_words,
            product: StoreVersion {
                offset: offset + 12,
                value: layout.text(),
            },
        })
    }

    /// Decode strictly framed operation labels for parser/cache tests.
    #[cfg(test)]
    pub fn operation_labels(&self) -> Vec<OperationLabel<'a>> {
        self.cached_operation_labels.to_vec()
    }

    /// Decode fully framed Boolean operations from the pointed record area.
    pub fn boolean_operations(&self) -> Vec<BooleanOperation> {
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Vec::new();
        };
        boolean_operations_with_labels(bytes, base_offset, &self.cached_operation_labels)
    }

    /// Bound operation records and retain their ordinal in the complete label sequence.
    pub fn operation_records_with_label_ordinals(&self) -> Vec<(usize, OperationRecord<'a>)> {
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Vec::new();
        };
        operation_records_with_labels_and_ordinals(
            bytes,
            base_offset,
            &self.cached_operation_labels,
        )
    }

    /// Bound validated operation headers that have no complete label frame.
    pub fn unlabeled_operation_records_with_ordinals(
        &self,
    ) -> Vec<(usize, UnlabeledOperationRecord<'a>)> {
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Vec::new();
        };
        unlabeled_operation_records_with_ordinals(bytes, base_offset, &self.cached_operation_labels)
    }

    /// Decode the bounded state-counter map of a feature-history record area.
    ///
    /// The section-role check is intentional. The same byte patterns occur in
    /// ordinary model-store payloads, where they do not carry operation state.
    pub fn operation_state_counter_map(&self) -> Option<OperationStateCounterMap<'a>> {
        let is_feature_history = self
            .types
            .iter()
            .any(|definition| definition.name == "UGS::FEATURE_RECORD")
            || self
                .fields
                .iter()
                .any(|definition| definition.name == "m_rollForwardStates");
        if !is_feature_history {
            return None;
        }
        let (base_offset, bytes) = self.record_area_parts()?;
        operation_state_counter_map(bytes, base_offset)
    }

    /// Decode the field-declared `m_rollForwardStates` group table before the
    /// bounded operation-state counter map.
    pub fn operation_state_group_table(&self) -> Option<OperationStateGroupTable<'a>> {
        if !self
            .fields
            .iter()
            .any(|definition| definition.name == "m_rollForwardStates")
        {
            return None;
        }
        let map = self.operation_state_counter_map()?;
        let (base_offset, bytes) = self.record_area_parts()?;
        let map_start = map.offset.checked_sub(base_offset)?;
        operation_state_group_table_before_counter_map(bytes, map_start, base_offset)
    }

    /// Decode anchored state-journal groups from a feature-history record area.
    ///
    /// The journal prefix is selected by the record-area marker and its
    /// version-token run. After a complete group, only the exact `04 00`
    /// separator form may be skipped when it leads to another complete group.
    /// Any other byte stops the journal so later record-region data cannot
    /// become state.
    pub fn operation_state_journal_groups(&self) -> Option<Vec<OperationStateJournalGroup>> {
        let is_feature_history = self
            .types
            .iter()
            .any(|definition| definition.name == "UGS::FEATURE_RECORD");
        if !is_feature_history {
            return None;
        }
        let (base_offset, bytes) = self.record_area_parts()?;
        let header = self.record_area_header()?;
        let product = header.product.offset.checked_sub(base_offset)?;
        let product_end = record_area_product_end(bytes, product)?;
        let start = operation_state_journal_start(bytes, product_end)?;
        let end = self
            .cached_operation_labels
            .first()
            .and_then(|label| label.header_offset.checked_sub(base_offset))
            .unwrap_or(bytes.len());
        operation_state_journal_groups_before_boundary(bytes, start, end, base_offset)
    }

    fn operation_state_block(&self) -> Option<OperationStateBlock<'a>> {
        let map = self.operation_state_counter_map()?;
        let (base_offset, bytes) = self.record_area_parts()?;
        let (_, last_record) = self
            .operation_records_with_label_ordinals()
            .into_iter()
            .last()?;
        let start_offset = last_record.payload_offset;
        let start = start_offset.checked_sub(base_offset)?;
        let group = self.operation_state_group_table();
        let terminal = group
            .as_ref()
            .map_or(map.offset, |table| table.offset)
            .checked_sub(base_offset)?;
        let mut ends = Vec::with_capacity(2);
        if let Some(table) = &group {
            let overlap_end = terminal.checked_add(table.groups.first()?.opener.bytes().len())?;
            ends.push(overlap_end);
        }
        ends.push(terminal);
        ends.into_iter()
            .find_map(|end| operation_state_block_before_boundary(bytes, start, end, base_offset))
    }

    /// Decode the bounded per-object status lane after the operation records.
    pub fn operation_state_status_table(&self) -> Option<OperationStateStatusTable<'a>> {
        let (base_offset, bytes) = self.record_area_parts()?;
        let block = self.operation_state_block()?;
        let status_after = block.status_end_offset.checked_sub(base_offset)?;
        let message_start = match block.messages.first() {
            Some(message) => message.span.local_start(),
            None => status_after,
        };
        (!block.rows.is_empty() || !block.slot_lanes.is_empty()).then_some(
            OperationStateStatusTable {
                offset: block.offset,
                end_offset: block.status_end_offset,
                rows: block.rows,
                slot_lanes: block.slot_lanes,
                trailing_bytes: bytes.get(status_after..message_start)?,
            },
        )
    }

    /// Decode the contiguous standalone message records immediately before
    /// the roll-forward table or counter-map boundary.
    pub fn operation_state_messages(&self) -> Option<Vec<OperationStateMessage<'a>>> {
        Some(self.operation_state_block()?.messages)
    }

    /// Decode complete rows in an audit-trail record area.
    ///
    /// The registry role check prevents the same compact byte patterns in
    /// feature-history and model areas from being interpreted as audit data.
    /// Unknown bytes before, between, and after complete rows remain outside
    /// this typed view.
    pub fn audit_trail_rows(&self) -> Option<Vec<AuditTrailRow>> {
        let has_audit_marker = self
            .types
            .iter()
            .any(|definition| definition.name == "UGS::OM::SaveAuditTrail");
        let has_specialized_marker = self.types.iter().any(|definition| {
            matches!(
                definition.name,
                "UGS::FEATURE_RECORD" | "UGS::EXP_expression" | "UGS::Solid::Topol"
            )
        });
        if !has_audit_marker || has_specialized_marker {
            return None;
        }
        let (base_offset, bytes) = self.record_area_parts()?;
        let header = self.record_area_header()?;
        let product = header.product.offset.checked_sub(base_offset)?;
        let product_end = record_area_product_end(bytes, product)?;
        let start = bytes
            .get(product_end..)?
            .windows(2)
            .position(|window| window == [0x41, 0x00])?
            .checked_add(product_end + 2)?;
        audit_trail_rows(bytes, start, bytes.len(), base_offset)
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
    validated_operation_headers(bytes, base_offset)
        .into_iter()
        .filter_map(|header| operation_label_at(bytes, base_offset, header))
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct OperationHeaderLayout {
    offset: usize,
    fields_end: usize,
    object_indices: [Option<u32>; 4],
    object_index_offsets: [usize; 4],
}

fn validated_operation_headers(bytes: &[u8], base_offset: usize) -> Vec<OperationHeaderLayout> {
    const PREFIX: &[u8] = &[0x80, 0xcd, 0x01, 0x04, 0x01];
    const SCALAR_LEN: usize = 8;
    let mut headers = Vec::new();
    for marker in bytes
        .windows(PREFIX.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == PREFIX).then_some(offset))
    {
        let scalar_at = marker + PREFIX.len();
        let Some(raw_scalar) = bytes.get(scalar_at..scalar_at + SCALAR_LEN) else {
            continue;
        };
        if shifted_ieee_f64(raw_scalar).is_none()
            || bytes.get(scalar_at + SCALAR_LEN..scalar_at + SCALAR_LEN + 2) != Some(&[0xff, 0xff])
        {
            continue;
        }
        let mut at = scalar_at + SCALAR_LEN + 2;
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
        if !valid {
            continue;
        }
        headers.push(OperationHeaderLayout {
            offset: base_offset + marker,
            fields_end: at,
            object_indices,
            object_index_offsets,
        });
    }
    headers
}

fn operation_label_at(
    bytes: &[u8],
    base_offset: usize,
    header: OperationHeaderLayout,
) -> Option<OperationLabel<'_>> {
    let at = header.fields_end;
    if bytes.get(at) != Some(&0x03) {
        return None;
    }
    let length = bytes.get(at + 1).copied().map(usize::from)?;
    if length < 3 {
        return None;
    }
    let end = at.checked_add(length)?;
    if bytes.get(end) != Some(&0) {
        return None;
    }
    let name = bytes.get(at + 2..end)?;
    if !name
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return None;
    }
    let Ok(value) = std::str::from_utf8(name) else {
        return None;
    };
    Some(OperationLabel {
        header_offset: header.offset,
        offset: base_offset + at,
        value,
        object_indices: header.object_indices,
        object_index_offsets: header.object_index_offsets,
    })
}

fn operation_records_with_labels_and_ordinals<'a>(
    bytes: &'a [u8],
    base_offset: usize,
    labels: &[OperationLabel<'a>],
) -> Vec<(usize, OperationRecord<'a>)> {
    let headers = validated_operation_headers(bytes, base_offset);
    headers
        .iter()
        .enumerate()
        .filter_map(|(ordinal, header)| {
            let label = labels
                .iter()
                .find(|label| label.header_offset == header.offset)?;
            let start = label.header_offset.checked_sub(base_offset)?;
            let end = headers
                .get(ordinal + 1)
                .map_or(bytes.len(), |next| next.offset - base_offset);
            let label_at = label.offset.checked_sub(base_offset)?;
            let payload_start = label_at
                .checked_add(usize::from(*bytes.get(label_at + 1)?))?
                .checked_add(1)?;
            Some((
                ordinal,
                OperationRecord {
                    bytes: bytes.get(start..end)?,
                    payload_offset: base_offset + payload_start,
                    payload: bytes.get(payload_start..end)?,
                    label: *label,
                },
            ))
        })
        .collect()
}

fn unlabeled_operation_records_with_ordinals<'a>(
    bytes: &'a [u8],
    base_offset: usize,
    labels: &[OperationLabel<'a>],
) -> Vec<(usize, UnlabeledOperationRecord<'a>)> {
    let headers = validated_operation_headers(bytes, base_offset);
    headers
        .iter()
        .enumerate()
        .filter_map(|(ordinal, header)| {
            if labels
                .iter()
                .any(|label| label.header_offset == header.offset)
            {
                return None;
            }
            let start = header.offset.checked_sub(base_offset)?;
            let end = headers
                .get(ordinal + 1)
                .map_or(bytes.len(), |next| next.offset - base_offset);
            Some((
                ordinal,
                UnlabeledOperationRecord {
                    offset: header.offset,
                    bytes: bytes.get(start..end)?,
                    payload_offset: base_offset + header.fields_end,
                    payload: bytes.get(header.fields_end..end)?,
                    object_indices: header.object_indices,
                    object_index_offsets: header.object_index_offsets,
                },
            ))
        })
        .collect()
}

/// Decode ordered `03|04, length, text, 00` frames from one operation payload.
pub fn operation_payload_text_frames(
    record: OperationRecord<'_>,
) -> Vec<OperationPayloadTextFrame<'_>> {
    let mut frames = Vec::new();
    let mut at = 0usize;
    while at + 4 <= record.payload.len() {
        let marker = match record.payload[at] {
            0x03 => OperationTextMarker::Text,
            0x04 => OperationTextMarker::String,
            _ => {
                at += 1;
                continue;
            }
        };
        let declared = usize::from(record.payload[at + 1]);
        let Some(end) = at.checked_add(declared) else {
            at += 1;
            continue;
        };
        let Some(raw) = record.payload.get(at + 2..end) else {
            at += 1;
            continue;
        };
        let Some(value) = std::str::from_utf8(raw).ok().and_then(|value| crate::payload_text::PayloadText::new(value).ok()) else {
            at += 1;
            continue;
        };
        if declared < 3 || record.payload.get(end) != Some(&0) {
            at += 1;
            continue;
        }
        frames.push(OperationPayloadTextFrame {
            marker,
            offset: record.payload_offset + at,
            value,
        });
        at = end + 1;
    }
    frames
}

/// Decode ordered `04, length, text, 00` strings from one operation payload.
pub fn operation_payload_strings(record: OperationRecord<'_>) -> Vec<OperationPayloadString<'_>> {
    operation_payload_text_frames(record)
        .into_iter()
        .filter(|frame| frame.marker == OperationTextMarker::String)
        .map(|frame| OperationPayloadString {
            offset: frame.offset,
            value: frame.value,
        })
        .collect()
}

/// Decode an exact nonempty duplicated shifted-binary64 lane before a hole template.
pub fn simple_hole_repeated_scalar_lane(
    record: OperationRecord<'_>,
) -> Option<NonEmpty<RepeatedScalar<usize>>> {
    if record.label.value != "SIMPLE HOLE" {
        return None;
    }
    let templates = operation_payload_strings(record)
        .into_iter()
        .filter(|value| value.value.as_str().starts_with("Hole_"))
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
            if let Some(scalar) = ShiftedBinary64::read(&prefix[at..at + 8]) {
                scalars.push((scalar, record.payload_offset + at));
                at += 8;
                continue;
            }
        }
        at += 1;
    }
    let half = scalars.len() / 2;
    if scalars.len() != half * 2 {
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
    NonEmpty::new(first.iter().zip(second).map(|(left, right)| RepeatedScalar {
        scalar: left.0,
        witness_offsets: [left.1, right.1],
    }))
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
            [first.value(), second.value()],
            [
                record.payload_offset + first_offset,
                record.payload_offset + second_offset,
            ],
            prefix,
        ))
    };
    let (first, first_offsets, first_prefix) =
        decode_pair(pair.last().witness_offsets[0], FIRST_PREFIX)?;
    let (second, second_offsets, second_prefix) =
        decode_pair(pair.last().witness_offsets[1], SECOND_PREFIX)?;
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
    let mut candidate = None;
    for start in 0..record.payload.len().saturating_sub(PREFIX.len()) {
        if record.payload.get(start..start + PREFIX.len()) != Some(&PREFIX) {
            continue;
        }
        let Some(lane) = (|| {
            let selector = NonZeroU8::new(*record.payload.get(start + 5)?)?;
            let branch = NonZeroU8::new(*record.payload.get(start + 7)?)?;
            if record.payload.get(start + 6) != Some(&0)
                || record.payload.get(start + 8..start + 12) != Some(&ZEROES)
            {
                return None;
            }
            let mut at = start + 12;
            let references = std::array::from_fn::<_, 4, _>(|ordinal| {
                if ordinal == 2 {
                    if record.payload.get(at) != Some(&branch.get())
                        || record.payload.get(at + 1..at + 5) != Some(&ZEROES)
                    {
                        return None;
                    }
                    at += 5;
                }
                let reference_offset = at;
                let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
                at += width;
                Some(PayloadObjectReference {
                    offset: record.payload_offset + reference_offset,
                    token: object_index,
                })
            });
            let [a, b, c, d] = references;
            let references = [a?, b?, c?, d?];
            if record.payload.get(at..at + SUFFIX.len()) != Some(&SUFFIX) {
                return None;
            }
            Some(HolePackageConstructionGroupLane {
                offset: start,
                selector,
                branch,
                references,
            })
        })() else {
            continue;
        };
        if candidate.is_some() {
            return None;
        }
        candidate = Some(lane);
    }
    candidate
}

/// Decode the unique counted reference field in a bounded `SKETCH` payload.
pub fn sketch_payload_references(
    record: OperationRecord<'_>,
) -> Option<SketchPayloadReferenceField> {
    if record.label.value != "SKETCH" {
        return None;
    }
    unique_candidate(
        (0..record.payload.len().saturating_sub(3)).filter_map(|start| {
            if record.payload.get(start..start + 2) != Some(&[0x01, 0x00]) {
                return None;
            }
            sketch_reference_field(record, start)
        }),
    )
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
            token: object_index,
        });
        at += width;
    }
    at += 2;
    let object_index = ReferenceIndexToken::read_payload(record.payload.get(at..)?)?;
    references.push(PayloadObjectReference {
        offset: record.payload_offset + at,
        token: object_index,
    });
    Some(SketchPayloadReferenceField {
        declared_count,
        references,
    })
}

fn payload_object_index(bytes: &[u8]) -> Option<(ReferenceIndexToken, usize)> {
    let token = ReferenceIndexToken::read_payload(bytes)?;
    Some((token, token.raw().len()))
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
            token: object_index,
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
                (repeated.token.value() == references[anchor].token.value()).then_some(())?;
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
    unique_candidate(
        (0..=record.payload.len().saturating_sub(marker.len())).filter_map(|start| {
            if record.payload.get(start..start + marker.len()) != Some(marker) {
                return None;
            }
            decode_field(start)
        }),
    )
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
            token: object_index,
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
    let field = match record.label.value {
        "Pattern Feature" | "Pattern Geometry" => {
            unique_candidate((0..record.payload.len()).filter_map(|start| {
                match record.payload.get(start) {
                    Some(0x61) => decode_graph(start),
                    Some(0x3b) => decode_compact_graph(start),
                    _ => None,
                }
            }))
        }
        "Geometry Instance" => unique_candidate(
            (0..=record.payload.len().saturating_sub(INSTANCE_PREFIX.len()))
                .filter(|&start| {
                    record.payload.get(start..start + INSTANCE_PREFIX.len())
                        == Some(&INSTANCE_PREFIX)
                })
                .filter_map(decode_instance),
        ),
        _ => return None,
    };
    field
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
                token: object_index,
            });
        }
        Some(PatternPayloadCountedReferenceLane {
            offset: record.payload_offset + start,
            references,
        })
    };
    unique_candidate((0..record.payload.len()).filter_map(decode))
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
        let object_index = ReferenceIndexToken::read_feature(record.payload.get(offset..)?)?;
        let width = 3;
        *at += width;
        Some(PayloadObjectReference {
            offset: record.payload_offset + offset,
            token: object_index,
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
        let (selector, first) =
            unique_candidate((body_start + 2..body_end - 1).filter_map(|selector_end| {
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
            }))?;
        let mut at = body_end;
        let second = [
            decode_reference(&mut at)?,
            decode_reference(&mut at)?,
            decode_reference(&mut at)?,
        ];
        (record.payload.get(at..at + SUFFIX.len()) == Some(&SUFFIX)).then_some(())?;
        Some(FsetPayloadReferenceGraph {
            selector,
            first,
            second,
            offset: record.payload_offset + start,
        })
    };
    unique_candidate((0..record.payload.len().saturating_sub(1)).filter_map(decode))
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
    let references = std::array::from_fn::<_, 5, _>(|_| {
        let offset = at;
        let (object_index, width) = if record.payload.get(at) == Some(&0xff) {
            (None, 1)
        } else {
            let (object_index, width) = payload_object_index(&record.payload[at..])?;
            (Some(object_index), width)
        };
        at += width;
        Some(DeletePayloadReferenceSlot {
            token: object_index,
            offset: record.payload_offset + offset,
        })
    });
    let [a, b, c, d, e] = references;
    let references = [a?, b?, c?, d?, e?];
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
    let decode = |start: usize| {
        (record.payload.get(start) == Some(&0x01)).then_some(())?;
        let declared_count @ 2.. = *record.payload.get(start + 1)? else { return None; };
        let row_schema_index = NonZeroU8::new(*record.payload.get(start + 2)?)?;
        let mut at = start + 2;
        let mut rows = Vec::new();
        for ordinal in 1..declared_count {
            (record.payload.get(at) == Some(&row_schema_index.get())).then_some(())?;
            (record.payload.get(at + 1..at + 1 + prefix_tail.len()) == Some(prefix_tail))
                .then_some(())?;
            at += 1 + prefix_tail.len();
            let scalar = ShiftedScalar::read(record.payload.get(at..)?)?;
            let width = scalar.raw().len();
            let value = PatternValue {
                scalar,
                offset: record.payload_offset + at,
            };
            at += width;
            (record.payload.get(at..at + scalar_suffix.len()) == Some(scalar_suffix))
                .then_some(())?;
            at += scalar_suffix.len();
            let selector_offset = at;
            let atom = CompactIndexAtom::read(record.payload.get(at..)?)?;
            let width = atom.raw().len();
            let selector = LocatedCompactIndex {
                atom,
                offset: record.payload_offset + selector_offset,
            };
            rows.push(PatternRow {
                values: value,
                selector,
            });
            at += width;
            (record.payload.get(at) == Some(&0x01)).then_some(())?;
            (record.payload.get(at + 1) == Some(&ordinal)).then_some(())?;
            (record.payload.get(at + 2..at + 2 + ROW_TAIL.len()) == Some(&ROW_TAIL))
                .then_some(())?;
            at += 2 + ROW_TAIL.len();
        }
        let terminal_schema_index = row_schema_index.get() - 1;
        (record.payload.get(at) == Some(&terminal_schema_index)).then_some(())?;
        (record.payload.get(at + 1..at + 3) == Some(&[0x00, 0x00])).then_some(())?;
        (record.payload.get(at + 3) == Some(&0x01)).then_some(())?;
        Some(PatternPayloadTransformLane {
            offset: record.payload_offset + start,
            row_schema_index,
            rows: PatternRows::Scalar(BranchItems::new(rows).ok()?),
        })
    };
    let decode_wide = |start: usize| {
        (record.label.value == "Pattern Feature").then_some(())?;
        (record.payload.get(start) == Some(&0x01)).then_some(())?;
        let declared_count @ 2.. = *record.payload.get(start + 1)? else { return None; };
        let row_schema_index = NonZeroU8::new(*record.payload.get(start + 2)?)?;
        let mut at = start + 2;
        let mut rows = Vec::new();
        for ordinal in 1..declared_count {
            (record.payload.get(at) == Some(&row_schema_index.get())).then_some(())?;
            at += 1;
            let mut decode_value = |value_ordinal| {
                let value_offset = at;
                let scalar = ShiftedBinary64::read(record.payload.get(at..at + 8)?)?;
                let value = PatternValue {
                    scalar,
                    offset: record.payload_offset + value_offset,
                };
                at += 8;
                if value_ordinal == 1 {
                    (record.payload.get(at..at + 2) == Some(&[0x00, 0x00])).then_some(())?;
                    at += 2;
                }
                Some(value)
            };
            let first = [
                decode_value(0)?,
                decode_value(1)?,
                decode_value(2)?,
                decode_value(3)?,
            ];
            (record.payload.get(at..at + 4) == Some(&[0x00; 4])).then_some(())?;
            at += 4;
            let terminal_value_offset = at;
            let scalar = PatternTerminal::read(record.payload.get(at..)?)?;
            let width = scalar.raw().len();
            let terminal = PatternValue {
                scalar,
                offset: record.payload_offset + terminal_value_offset,
            };
            at += width;
            (record.payload.get(at..at + 7) == Some(&[0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x03]))
                .then_some(())?;
            at += 7;
            let selector_offset = at;
            let atom = CompactIndexAtom::read(record.payload.get(at..)?)?;
            let width = atom.raw().len();
            let selector = LocatedCompactIndex {
                atom,
                offset: record.payload_offset + selector_offset,
            };
            rows.push(PatternRow {
                values: PatternWideValues { first, terminal },
                selector,
            });
            at += width;
            (record.payload.get(at) == Some(&0x01)).then_some(())?;
            (record.payload.get(at + 1) == Some(&ordinal)).then_some(())?;
            (record.payload.get(at + 2..at + 2 + ROW_TAIL.len()) == Some(&ROW_TAIL))
                .then_some(())?;
            at += 2 + ROW_TAIL.len();
        }
        let terminal_schema_index = row_schema_index.get() - 1;
        (record.payload.get(at) == Some(&terminal_schema_index)).then_some(())?;
        (record.payload.get(at + 1..at + 4) == Some(&[0x00, 0x00, 0x02])).then_some(())?;
        Some(PatternPayloadTransformLane {
            offset: record.payload_offset + start,
            row_schema_index,
            rows: PatternRows::Wide(BranchItems::new(rows).ok()?),
        })
    };
    unique_candidate(
        (0..record.payload.len().saturating_sub(1))
            .filter_map(decode)
            .chain((0..record.payload.len().saturating_sub(1)).filter_map(decode_wide)),
    )
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
    let decode = |start: usize| {
        (record.payload.get(start..start + ENVELOPE.len()) == Some(&ENVELOPE)).then_some(())?;
        let declared_count = *record.payload.get(start + ENVELOPE.len())?;
        (declared_count >= 2).then_some(())?;
        let mut instance_count = 0;
        let row_count = usize::from(declared_count - 1);
        let mut at = start + ENVELOPE.len() + 1;
        let mut rows = Vec::with_capacity(row_count);
        for expected_row_index in 2..=declared_count {
            (record.payload.get(at..at + ROW_PREFIX.len()) == Some(&ROW_PREFIX)).then_some(())?;
            at += ROW_PREFIX.len();
            let selector_offset = at;
            let atom = CompactIndexAtom::read(record.payload.get(at..)?)?;
            let width = atom.raw().len();
            let selector = LocatedCompactIndex {
                atom,
                offset: record.payload_offset + selector_offset,
            };
            at += width;
            (record.payload.get(at) == Some(&ROW_ORDINAL_MARKER)).then_some(())?;
            let ordinal = *record.payload.get(at + 1)?;
            instance_count = instance_count.max(ordinal);
            (record.payload.get(at + 2) == Some(&expected_row_index)).then_some(())?;
            rows.push((selector, ordinal));
            at += 3;
        }
        (record.payload.get(at..at + REFERENCE_PREFIX.len()) == Some(&REFERENCE_PREFIX))
            .then_some(())?;
        at += REFERENCE_PREFIX.len();
        let mut trailing_references =
            Vec::with_capacity(usize::from(instance_count.saturating_sub(1)));
        for _ in 1..instance_count {
            let reference_offset = at;
            let object_index = reference_index::FeatureReferenceToken::read(record.payload.get(at..)?)?;
            let end = at + object_index.raw().len();
            trailing_references.push(PayloadObjectReference {
                offset: record.payload_offset + reference_offset,
                token: object_index,
            });
            at = end;
        }
        (record.payload.get(at..at + 2) == Some(&[0x01, instance_count])).then_some(())?;
        Some(MultiInstanceOutputPayloadLane {
            offset: record.payload_offset + start + 8,
            outputs: instances::MultiInstanceOutputs::new(rows, trailing_references).ok()?,
        })
    };
    unique_candidate((0..=record.payload.len().saturating_sub(ENVELOPE.len())).filter_map(decode))
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
    let decode = |start: usize| {
        let leading_schema_index = *record.payload.get(start)?;
        let count_schema_index = IdenticalInstanceSchemaIndex::new(*record.payload.get(start + 1)?)?;
        (record.payload.get(start + 2) == Some(&0x01)).then_some(())?;
        let declared_count = *record.payload.get(start + 3)?;
        (declared_count >= 2).then_some(())?;
        let [first_schema_index, second_schema_index, third_schema_index] =
            count_schema_index.row_indices();
        let mut at = start + 4;
        let mut selectors = Vec::with_capacity(usize::from(declared_count - 1));
        for ordinal in 2..=declared_count {
            (record.payload.get(at) == Some(&first_schema_index)).then_some(())?;
            (record.payload.get(at + 1) == Some(&second_schema_index)).then_some(())?;
            (record.payload.get(at + 2..at + 4) == Some(&ROW_MIDDLE)).then_some(())?;
            (record.payload.get(at + 4) == Some(&third_schema_index)).then_some(())?;
            at += 5;
            let selector_offset = at;
            let atom = CompactIndexAtom::read(record.payload.get(at..)?)?;
            let width = atom.raw().len();
            selectors.push(LocatedCompactIndex {
                atom,
                offset: record.payload_offset + selector_offset,
            });
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
            selectors: compact::CountedIndexMembers::new(selectors).ok()?,
        })
    };
    unique_candidate((0..record.payload.len().saturating_sub(3)).filter_map(decode))
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
            token: object_index,
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
    let values = lane
        .chunks_exact(8)
        .map(ShiftedBinary64::read)
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    Some(PointFeatureScalarLane {
        values,
        offset: preceding_start,
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
                token: object_index,
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
    unique_candidate(
        (PAYLOAD_PREFIX.len()..=record.payload.len().saturating_sub(GRAPH_PREFIX.len()))
            .filter(|&start| {
                record.payload.get(start..start + GRAPH_PREFIX.len()) == Some(&GRAPH_PREFIX)
            })
            .filter_map(decode),
    )
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
    let mut indices = Vec::with_capacity(usize::from(declared_count - 1));
    for _ in 1..declared_count {
        let token = LocatedCompactIndex::read(record.payload, at)?;
        at += token.atom.raw().len();
        indices.push(LocatedCompactIndex {
            atom: token.atom,
            offset: record.payload_offset + token.offset,
        });
    }
    (record.payload.get(at..at + 2) == Some(&[0x01, 0x02])).then_some(())?;

    Some(DraftFeatureLeadingIndexLane {
        indices: CountedIndexMembers::new(indices).ok()?,
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
    let mut candidate = None;
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
        let lane = DraftFeatureTerminalLane {
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
        };
        if candidate.is_some() {
            return None;
        }
        candidate = Some(lane);
    }
    candidate
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
            token: object_index,
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

    let trailing_start = unique_candidate(
        record
            .payload
            .windows(TRAILING_PREFIX.len())
            .enumerate()
            .filter_map(|(start, bytes)| (bytes == TRAILING_PREFIX).then_some(start)),
    )?;
    at = trailing_start + TRAILING_PREFIX.len();
    for _ in 0..3 {
        references.push(decode_reference(&mut at)?);
    }
    (record.payload.get(at..at + TRAILING_SUFFIX.len()) == Some(&TRAILING_SUFFIX)).then_some(())?;
    Some(SurfaceFeaturePayloadReferenceField {
        references: references.try_into().ok()?,
    })
}

/// Decode the exact leading construction-reference envelope in a bounded
/// `THRU_CURVE` payload.
pub fn thru_curve_payload_references(
    record: OperationRecord<'_>,
) -> Option<ThruCurvePayloadReferenceField> {
    const HEADER: [u8; 4] = [0x00, 0x00, 0x01, 0x00];
    (record.label.value == "THRU_CURVE").then_some(())?;
    let discriminator = NonZeroU8::new(*record.payload.first()?)?;
    (record.payload.get(1..1 + HEADER.len()) == Some(&HEADER)).then_some(())?;

    let mut at = 1 + HEADER.len();
    let mut references = Vec::with_capacity(9);
    let decode_reference = |at: &mut usize| {
        let offset = *at;
        let (object_index, width) = payload_object_index(record.payload.get(offset..)?)?;
        *at += width;
        Some(PayloadObjectReference {
            offset: record.payload_offset + offset,
            token: object_index,
        })
    };
    for _ in 0..3 {
        references.push(decode_reference(&mut at)?);
    }
    (record.payload.get(at..at + 2) == Some(&[0x01, 0x08])).then_some(())?;
    at += 2;
    let controls: [u8; 9] = record.payload.get(at..at + 9)?.try_into().ok()?;
    let controls = ThruCurveControls::try_from(controls).ok()?;
    at += 9;
    for _ in 0..6 {
        references.push(decode_reference(&mut at)?);
    }
    (*record.payload.get(at)? == 0x04).then_some(())?;
    let trailing_control = NonZeroU8::new(*record.payload.get(at + 1)?)?;
    (*record.payload.get(at + 2)? == 0xa0).then_some(())?;
    let trailing_value = record.payload.get(at + 3..at + 5)?.try_into().ok()?;
    (record.payload.get(at + 5..at + 7) == Some(&[0x13, 0x01])).then_some(())?;

    Some(ThruCurvePayloadReferenceField {
        discriminator,
        controls,
        references: references.try_into().ok()?,
        trailing_control,
        trailing_value,
        end_offset: record.payload_offset + at + 7,
    })
}

fn thru_curve_payload_branch(
    record: OperationRecord<'_>,
    at: usize,
) -> Option<(ThruCurvePayloadBranch, usize)> {
    let mode = NonZeroU8::new(*record.payload.get(at)?)?;
    (*record.payload.get(at + 1)? == 0x01).then_some(())?;
    let declared_count @ 2.. = *record.payload.get(at + 2)? else {
        return None;
    };
    let mut cursor = at + 3;
    let mut members = Vec::with_capacity(usize::from(declared_count) - 1);
    for _ in 1..declared_count {
        let offset = cursor;
        let (object_index, width) = payload_object_index(record.payload.get(cursor..)?)?;
        cursor += width;
        members.push(PayloadObjectReference {
            offset: record.payload_offset + offset,
            token: object_index,
        });
    }
    (record.payload.get(cursor..cursor + 2) == Some(&[0x01, declared_count])).then_some(())?;
    cursor += 2;

    let standard_len = usize::from(declared_count) + 3;
    let lane = record.payload.get(cursor..cursor + standard_len)?;
    let lane = if lane.iter().all(|&byte| byte == 0) {
        lane
    } else {
        record.payload.get(cursor..cursor + 18)?
    };
    let lane_len = lane.len();
    let members = ThruCurveBranchItems::from_parts(members, lane).ok()?;
    cursor += lane_len;
    (record.payload.get(cursor..cursor + 3) == Some(&[0xff, 0x01, 0x02])).then_some(())?;
    cursor += 3;
    let terminal_offset = cursor;
    let (object_index, width) = payload_object_index(record.payload.get(cursor..)?)?;
    cursor += width;
    let terminal = PayloadObjectReference {
        offset: record.payload_offset + terminal_offset,
        token: object_index,
    };
    (*record.payload.get(cursor)? == 0x00).then_some(())?;
    cursor += 1;
    let suffix: [u8; 2] = record.payload.get(cursor..cursor + 2)?.try_into().ok()?;
    let suffix = ThruCurveBranchSuffix::try_from(suffix).ok()?;
    cursor += 2;

    Some((
        ThruCurvePayloadBranch {
            offset: record.payload_offset + at,
            mode,
            members,
            terminal,
            suffix,
        },
        cursor,
    ))
}

/// Decode the exact counted branch group after a bounded `THRU_CURVE`
/// reference envelope.
pub fn thru_curve_payload_branch_group(
    record: OperationRecord<'_>,
) -> Option<ThruCurvePayloadBranchGroup> {
    let envelope = thru_curve_payload_references(record)?;
    let mut at = envelope.end_offset.checked_sub(record.payload_offset)?;
    let group_offset = at;
    let declared_count @ 2.. = *record.payload.get(at)? else {
        return None;
    };
    at += 1;
    let mut branches = Vec::with_capacity(usize::from(declared_count) - 1);
    for _ in 1..declared_count {
        let (branch, next) = thru_curve_payload_branch(record, at)?;
        branches.push(branch);
        at = next;
    }
    let terminator = ThruCurveGroupTerminator::ALL
        .into_iter()
        .find(|terminator| record.payload.get(at..at + terminator.bytes().len()) == Some(terminator.bytes()))?;
    Some(ThruCurvePayloadBranchGroup {
        offset: record.payload_offset + group_offset,
        branches: BranchItems::new(branches).ok()?,
        terminator,
    })
}

/// Decode the exact leading construction branch in a bounded `SWP104`
/// payload.
pub fn swp104_payload_leading_branch(
    record: OperationRecord<'_>,
) -> Option<Swp104PayloadLeadingBranch> {
    const HEADER: [u8; 4] = [0x00, 0x00, 0x01, 0x00];
    (record.label.value == "SWP104").then_some(())?;
    let discriminator = NonZeroU8::new(*record.payload.first()?)?;
    (record.payload.get(1..5) == Some(&HEADER)).then_some(())?;

    let mut at = 5;
    let mut scalars = Vec::with_capacity(4);
    for _ in 0..4 {
        scalars.push(ShiftedBinary64::read(record.payload.get(at..at + 8)?)?);
        at += 8;
    }
    let scalars = scalars.try_into().ok()?;

    let leading_zero = record.payload.get(at) == Some(&0x00);
    at += usize::from(leading_zero);
    let mode = NonZeroU8::new(*record.payload.get(at)?)?;
    (*record.payload.get(at + 1)? == 0x01).then_some(())?;
    let declared_count @ 2.. = *record.payload.get(at + 2)? else {
        return None;
    };
    at += 3;
    let mut members = Vec::with_capacity(usize::from(declared_count) - 1);
    for _ in 1..declared_count {
        let offset = at;
        let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
        at += width;
        members.push(PayloadObjectReference {
            offset: record.payload_offset + offset,
            token: object_index,
        });
    }

    let witnessed_count = if record.payload.get(at) == Some(&0x01) {
        let count @ 2.. = *record.payload.get(at + 1)? else {
            return None;
        };
        Some(count)
    } else {
        None
    };
    let state_len = if let Some(count) = witnessed_count {
        at += 2;
        usize::from(count) + 3
    } else {
        5
    };
    let state_lane = Swp104StateLane::from_parts(
        witnessed_count, record.payload.get(at..at + state_len)?.to_vec(),
    ).ok()?;
    at += state_len;
    (record.payload.get(at..at + 3) == Some(&[0xff, 0x01, 0x02])).then_some(())?;
    at += 3;
    let terminal_offset = at;
    let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
    at += width;
    let terminal = PayloadObjectReference {
        offset: record.payload_offset + terminal_offset,
        token: object_index,
    };
    (*record.payload.get(at)? == 0x00).then_some(())?;
    at += 1;

    Some(Swp104PayloadLeadingBranch {
        discriminator,
        scalars,
        leading_zero,
        mode,
        state_lane,
        members: BranchItems::new(members).ok()?,
        terminal,
        end_offset: record.payload_offset + at,
    })
}

fn surface_feature_branch_paths(
    payload: &[u8],
    payload_offset: usize,
    at: usize,
    remaining: u8,
    terminator: &[u8],
) -> Vec<Vec<SurfaceFeaturePayloadBranch>> {
    if remaining == 0 {
        return Vec::new();
    }
    let Some(mode) = payload
        .get(at)
        .copied()
        .and_then(|value| discriminators::SurfaceBranchMode::try_from(value).ok())
    else {
        return Vec::new();
    };
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
                token: object_index,
            });
            member_cursor += width;
        }
        let terminal = PayloadObjectReference {
            offset: payload_offset + terminal_offset,
            token: object_index,
        };
        let Ok(members) = BranchItems::new(members) else { return Vec::new(); };
        for mut continuation in continuations {
            let branch = SurfaceFeaturePayloadBranch {
                offset: payload_offset + at,
                mode,
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
    let mut candidate = None;
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
        let group = SurfaceFeaturePayloadBranches {
            family,
            header_code,
            branches: branches.clone(),
        };
        if candidate.is_some() {
            return None;
        }
        candidate = Some(group);
    }
    candidate
}

/// Decode the unique witnessed profile-reference field in an `EXTRUDE` payload.
pub fn extrude_profile_references(
    record: OperationRecord<'_>,
) -> Option<ExtrudeProfileReferenceField> {
    if record.label.value != "EXTRUDE" {
        return None;
    }
    unique_candidate(
        (0..record.payload.len().saturating_sub(6)).filter_map(|start| {
            if record.payload.get(start..start + 2) != Some(&[0x01, 0x02])
                || record.payload.get(start + 3) != Some(&0x01)
            {
                return None;
            }
            extrude_profile_reference_field(record, start)
        }),
    )
}

/// Decode the fixed two-scalar header in a bounded `EXTRUDE` payload.
pub fn extrude_payload_header(record: OperationRecord<'_>) -> Option<ExtrudePayloadHeader> {
    if record.label.value != "EXTRUDE"
        || record.payload.get(..5) != Some(&[0x0f, 0x00, 0x00, 0x01, 0x00])
    {
        return None;
    }
    Some(ExtrudePayloadHeader {
        offset: record.payload_offset + 5,
        scalars: [
            ShiftedBinary64::read(record.payload.get(5..13)?)?,
            ShiftedBinary64::read(record.payload.get(13..21)?)?,
        ],
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
        let type_tokens = LocatedCompactIndex::read_array::<2>(record.payload, &mut at)?;
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
        let trailing_bytes = record.payload.get(at..trailing_end)?;
        let mut scan = 0;
        let mut trailing_count = 0;
        while scan < trailing_bytes.len() {
            let token = LocatedCompactIndex::read(trailing_bytes, scan)?;
            scan += token.atom.raw().len();
            trailing_count += 1;
        }

        // Candidates are scanned at every payload offset. Validate the bounded
        // trailing bytes before allocating their owned representation.
        let mut trailing_indices = Vec::with_capacity(trailing_count);
        let mut scan = 0;
        while scan < trailing_bytes.len() {
            let token = LocatedCompactIndex::read(trailing_bytes, scan)?;
            scan += token.atom.raw().len();
            trailing_indices.push(LocatedCompactIndex {
                atom: token.atom,
                offset: record.payload_offset + at + token.offset,
            });
        }

        Some(OperationTerminalDiscriminator {
            offset: record.payload_offset + start,
            type_indices: type_tokens.map(|token| LocatedCompactIndex {
                atom: token.atom,
                offset: record.payload_offset + token.offset,
            }),
            flags,
            trailing_indices,
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

/// Decode complete three-scalar clauses following ordered operation body fields.
pub fn operation_body_scalar_triples(
    record: OperationRecord<'_>,
) -> Vec<OperationBodyScalarTriple> {
    operation_body_references(record)
        .into_iter()
        .enumerate()
        .filter_map(|(ordinal, reference)| {
            let token = reference.offset.checked_sub(record.offset())?;
            let (_, end) = feature_object_index(record.bytes, token)?;
            if record.bytes.get(end) != Some(&0xff) {
                return None;
            }
            let branch = *record.bytes.get(end + 1)?;
            let mut at = end + 2;
            let mut scalars = Vec::with_capacity(3);
            for _ in 0..3 {
                let atom = PayloadScalarAtom::read(record.bytes.get(at..)?)?;
                let width = atom.raw().len();
                scalars.push(PayloadScalar {
                    offset: record.offset() + at,
                    atom,
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
            let Some(token) = reference.offset.checked_sub(record.offset()) else {
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
                let Some(atom) = record.bytes.get(at..).and_then(PayloadScalarAtom::read) else {
                    return Vec::new();
                };
                at += atom.raw().len();
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
                    offset: record.offset() + member_at,
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
            let token = reference.offset.checked_sub(record.offset())?;
            let (_, end) = feature_object_index(record.bytes, token)?;
            if record.bytes.get(end..end + 2) != Some(&[0xff, 0x11]) {
                return None;
            }
            let mut at = end + 2;
            for _ in 0..3 {
                let width = PayloadScalarAtom::read(record.bytes.get(at..)?)?.raw().len();
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
            let mut continuation = LocatedCompactIndex::read(record.bytes, at)?;
            at += continuation.atom.raw().len();
            continuation.offset += record.offset();
            if record.bytes.get(at..at + 3) != Some(&[0x00, 0x00, 0x01]) {
                return None;
            }
            at += 3;
            let terminal_at = at;
            let terminal_token = ReferenceIndexToken::read_feature(record.bytes.get(at..)?)?;
            let next = at + terminal_token.raw().len();
            if record.bytes.get(next..next + 2) != Some(&[0x00, 0x00]) {
                return None;
            }
            Some(OperationBody11Continuation {
                body_reference_ordinal: body_ordinal as u32,
                body_object_index: reference.object_index,
                continuation,
                terminal: PayloadObjectReference {
                    token: terminal_token,
                    offset: record.offset() + terminal_at,
                },
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
            let token = reference.offset.checked_sub(record.offset())?;
            let (_, end) = feature_object_index(record.bytes, token)?;
            if record.bytes.get(end) != Some(&0xff) {
                return None;
            }
            let branch = discriminators::OperationBodyReferenceBranch::try_from(*record.bytes.get(end + 1)?).ok()?;
            let mut at = end + 2;
            for _ in 0..3 {
                let width = PayloadScalarAtom::read(record.bytes.get(at..)?)?.raw().len();
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
            let compact = operation_body_reference_lane_values(record, at, count - 1, |bytes, offset| {
                let atom = CompactIndexAtom::read(bytes)?;
                let width = atom.raw().len();
                Some((LocatedCompactIndex { atom, offset }, width))
            });
            let objects = operation_body_reference_lane_values(record, at, count - 1, |bytes, offset| {
                let token = reference_index::PayloadIndexToken::read(bytes)?;
                let width = token.raw().len();
                Some((PayloadObjectReference { token, offset }, width))
            });
            let values = match (compact, objects) {
                (Some(values), None) => OperationBodyReferenceLaneValues::CompactIndex(values),
                (None, Some(values)) => OperationBodyReferenceLaneValues::PayloadObjectIndex(values),
                _ => return None,
            };
            Some(OperationBodyReferenceLane {
                body_reference_ordinal: body_ordinal as u32,
                body_object_index: reference.object_index,
                branch,
                values,
            })
        })
        .collect()
}

fn operation_body_reference_lane_values<T>(
    record: OperationRecord<'_>,
    mut at: usize,
    count: usize,
    read: impl Fn(&[u8], usize) -> Option<(T, usize)>,
) -> Option<Vec<T>> {
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let (value, width) = read(record.bytes.get(at..)?, record.offset() + at)?;
        at += width;
        values.push(value);
    }
    (record.bytes.get(at..at + 4) == Some(&[0x00, 0x00, 0x0b, 0x00])).then_some(values)
}

/// Decode the structured `32` branch following an extrusion body field.
pub fn extrude_payload_32_branch(record: OperationRecord<'_>) -> Option<ExtrudePayload32Branch> {
    if record.label.value != "EXTRUDE" {
        return None;
    }
    let reference = operation_body_reference(record)?;
    let token = reference.offset.checked_sub(record.offset())?;
    let (_, end) = feature_object_index(record.bytes, token)?;
    if record.bytes.get(end..end + 4) != Some(&[0xff, 0x32, 0x00, 0x00]) {
        return None;
    }
    let branch_at = end + 1;
    let scalar = ShiftedBinary64::read(record.bytes.get(end + 4..end + 12)?)?;
    let mut at = end + 12;
    let mut atoms = counted_u32_atoms(record.bytes, &mut at)?;
    for token in &mut atoms {
        token.offset += record.offset();
    }
    let mut first = counted_compact_values(record.bytes, &mut at)?;
    let mut second = counted_compact_values(record.bytes, &mut at)?;
    for token in first.iter_mut().chain(&mut second) {
        token.offset += record.offset();
    }
    if record.bytes.get(at..at + 2) != Some(&[0x00, 0x01]) {
        return None;
    }
    let terminal_token = ReferenceIndexToken::read_feature(record.bytes.get(at + 2..)?)?;
    let next = at + 2 + terminal_token.raw().len();
    if terminal_token.value() != reference.object_index
        || record.bytes.get(next..next + 2) != Some(&[0x00, 0x00])
    {
        return None;
    }
    Some(ExtrudePayload32Branch {
        offset: record.offset() + branch_at,
        scalar,
        atoms,
        first_indices: first,
        second_indices: second,
        terminal: PayloadObjectReference {
            token: terminal_token,
            offset: record.offset() + at + 2,
        },
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
            token: object_index,
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
        token: object_index,
    });
    at += width;
    if record.payload.get(at..at + TRAILER.len()) != Some(&TRAILER) {
        return None;
    }
    Some(BlockConstructionReferenceField {
        control: record.payload[0],
        references: references.try_into().ok()?,
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
    let references = std::array::from_fn::<_, 8, _>(|_| {
        let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
        let offset = record.payload_offset + at;
        at += width;
        Some(PayloadObjectReference { offset, token: object_index })
    });
    let [a, b, c, d, e, f, g, h] = references;
    let references = [a?, b?, c?, d?, e?, f?, g?, h?];
    (record.payload.get(at..at + TRAILER.len()) == Some(&TRAILER)).then_some(())?;
    Some(DatumCsysReferenceField {
        control: record.payload[0],
        references,
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
    let mut descriptor = LocatedCompactIndex::read(record.payload, at)?;
    at += descriptor.atom.raw().len();
    descriptor.offset += record.payload_offset;
    (record.payload.get(at) == Some(&0x01)).then_some(())?;
    at += 1;
    let object_offset = record.payload_offset + at;
    let object_index = reference_index::PayloadIndexToken::read(record.payload.get(at..)?)?;
    at += object_index.raw().len();
    (record.payload.get(at..at + SUFFIX.len()) == Some(&SUFFIX)).then_some(())?;
    Some(DatumPlaneSingleReferenceBranch {
        descriptor,
        object: PayloadObjectReference { token: object_index, offset: object_offset },
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
    let mut descriptor = LocatedCompactIndex::read(record.payload, at)?;
    at += descriptor.atom.raw().len();
    descriptor.offset += record.payload_offset;
    (record.payload.get(at..at + SEPARATOR.len()) == Some(&SEPARATOR)).then_some(())?;
    at += SEPARATOR.len();
    let object_offset = record.payload_offset + at;
    let object_index = reference_index::PayloadIndexToken::read(record.payload.get(at..)?)?;
    at += object_index.raw().len();
    (record.payload.get(at..at + SUFFIX.len()) == Some(&SUFFIX)).then_some(())?;
    Some(DatumPlaneSingleReferenceBranch {
        descriptor,
        object: PayloadObjectReference { token: object_index, offset: object_offset },
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
    let first_index = reference_index::PayloadIndexToken::read(record.payload.get(at..)?)?;
    let first = PayloadObjectReference {
        offset: record.payload_offset + at,
        token: first_index,
    };
    at += first_index.raw().len();
    let middle = if header.declared_count == 2 {
        COUNT_TWO_MIDDLE.as_slice()
    } else {
        COUNT_THREE_MIDDLE.as_slice()
    };
    (record.payload.get(at..at + middle.len()) == Some(middle)).then_some(())?;
    at += middle.len();
    let second_index = reference_index::PayloadIndexToken::read(record.payload.get(at..)?)?;
    let second = PayloadObjectReference {
        offset: record.payload_offset + at,
        token: second_index,
    };
    at += second_index.raw().len();
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
        let mut scan_at = start + 2;
        let mut complete = true;
        for _ in 1..declared_count {
            let Some((CompactIndex::Value(_), width)) =
                bytes.get(scan_at..).and_then(compact_index)
            else {
                complete = false;
                break;
            };
            scan_at += width;
        }
        if !complete || bytes.get(scan_at) != Some(&0x00) || scan_at + 5 != bytes.len() {
            continue;
        }
        let mut at = start + 2;
        let indices = (1..declared_count).map(|_| {
            let token = LocatedCompactIndex::read(&bytes[..scan_at], at)?;
            at += token.atom.raw().len();
            Some(token)
        }).collect::<Option<Vec<_>>>();
        let Some(indices) = indices.and_then(|indices| CountedIndexMembers::new(indices).ok()) else {
            continue;
        };
        let Some(trailer) = View::u32_be_at(bytes, scan_at + 1) else {
            continue;
        };
        lanes.push(DatumPlaneObjectIndexLane {
            offset: start,
            indices,
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
            let [first, second] = [first, second].map(|offset| LocatedBinary64::read(bytes, offset));
            Some(DatumPlaneObjectScalarPair {
                offset,
                values: [first?, second?],
            })
        })
        .collect()
}

/// Decode one complete datum-plane descriptor block.
pub fn datum_plane_descriptor_block(bytes: &[u8]) -> Option<plane_descriptor::PlaneDescriptor> {
    plane_descriptor::PlaneDescriptor::read(bytes)
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
            let [Some(first), Some(second)] = [first, second].map(|offset| LocatedBinary64::read(bytes, offset)) else {
                continue;
            };
            pairs.push(ObjectPayloadScalarPair {
                offset,
                values: [first, second],
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
        let [Some(first), Some(second)] = [first, second].map(|offset| LocatedBinary64::read(bytes, offset)) else {
            continue;
        };
        pairs.push(ObjectPayloadScalarPair {
            offset,
            values: [first, second],
            discriminator: bytes[offset..offset + discriminator_len].to_vec(),
        });
    }
    pairs.sort_by_key(|pair| pair.offset);
    pairs
}

/// Decode every complete scalar-vector frame in a reconstructed sketch
/// payload.
pub fn sketch_payload_scalar_lanes(bytes: &[u8]) -> Vec<FramedScalarRun<SketchScalarLaneForm, ()>> {
    let mut lanes = [SketchScalarLaneForm::Form03, SketchScalarLaneForm::Form07]
        .into_iter()
        .flat_map(|form| {
            let discriminator = form.discriminator();
            bytes.windows(discriminator.len()).enumerate()
                .filter_map(move |(offset, window)| {
                    (window == discriminator).then_some(())?;
                    let mut at = offset + discriminator.len();
                    let mut values = Vec::new();
                    loop {
                        if bytes.get(at) == Some(&0x00) {
                            break;
                        }
                        let scalar = ShiftedScalar::read(bytes.get(at..)?)?;
                        at += scalar.raw().len();
                        values.push((scalar, ()));
                    }
                    FramedScalarRun::new(form, offset as u64, NonEmpty::new(values)?).ok()
                })
        })
        .collect::<Vec<_>>();
    lanes.sort_by_key(FramedScalarRun::offset);
    lanes
}

/// Decode every exactly framed scaled shifted-binary64 pair in a reconstructed sketch payload.
pub fn sketch_payload_fixed_pairs(bytes: &[u8]) -> Vec<SketchPayloadFixedPair> {
    let mut pairs = Vec::new();
    for form in SketchPairForm::ALL {
        let discriminator = form.discriminator();
        let separator_width = form.separator_width();
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
            let Some(first_value) = sketch_fixed_atom(bytes, first) else {
                continue;
            };
            let Some(second_value) = sketch_fixed_atom(bytes, second) else {
                continue;
            };
            pairs.push(SketchPayloadFixedPair {
                offset,
                values: [first_value, second_value],
                form,
            });
        }
    }
    pairs.sort_by_key(|pair| pair.offset);
    pairs
}

/// Decode every exactly framed mixed scaled shifted-binary64/binary32 pair in a sketch payload.
pub fn sketch_payload_mixed_pairs(bytes: &[u8]) -> Vec<SketchPayloadMixedPair> {
    let discriminator = SketchPairForm::Legacy.discriminator();
    let mut pairs = Vec::new();
    for (offset, window) in bytes.windows(discriminator.len()).enumerate() {
        if window != discriminator {
            continue;
        }
        let fixed_offset = offset + discriminator.len();
        let binary32_offset = fixed_offset + 9;
        if bytes.get(fixed_offset) != Some(&0x30) || bytes.get(fixed_offset + 8) != Some(&0x00) {
            continue;
        }
        let Some(binary32_raw_value): Option<[u8; 4]> = bytes
            .get(binary32_offset..binary32_offset + 4)
            .and_then(|raw| raw.try_into().ok())
        else {
            continue;
        };
        let Some(binary32_atom) = ShiftedBinary32::read(&binary32_raw_value)
        else {
            continue;
        };
        let Some(fixed) = sketch_fixed_atom(bytes, fixed_offset) else {
            continue;
        };
        pairs.push(SketchPayloadMixedPair {
            offset,
            scalars: SketchMixedScalars { fixed, binary32: binary32_atom },
        });
    }
    pairs
}

fn sketch_fixed_atom(bytes: &[u8], offset: usize) -> Option<SketchScaledAtom> {
    Some(SketchScaledAtom::from_raw(bytes.get(offset + 1..offset + 8)?.try_into().ok()?))
}

/// Decode every exactly framed signed Q1.55 pair in a datum-CSYS payload.
pub fn datum_csys_payload_fixed_pairs(bytes: &[u8]) -> Vec<DatumCsysPayloadFixedPair> {
    let mut pairs = Vec::new();
    for form in DatumPairForm::ALL {
        let discriminator = form.discriminator();
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
                values: [Q155::from_raw(first_raw), Q155::from_raw(second_raw)],
                form,
            });
        }
    }
    pairs.sort_by_key(|pair| pair.offset);
    pairs
}

/// Decode every complete signed Q1.55 lane in a reconstructed draft graph payload.
pub fn draft_construction_fixed_lanes(bytes: &[u8]) -> Vec<FramedScalarRun<Q155LaneFrame, ()>> {
    bytes
        .windows(Q155LaneFrame::DISCRIMINATOR.len())
        .enumerate()
        .filter_map(|(offset, window)| {
            (window == Q155LaneFrame::DISCRIMINATOR).then_some(())?;
            let mut at = offset + Q155LaneFrame::DISCRIMINATOR.len();
            let mut values = Vec::new();
            while let Some(marker) = bytes.get(at).copied().and_then(Q155Marker::read) {
                let raw = bytes.get(at + 1..at + 8)?.try_into().ok()?;
                values.push((Q155Atom { marker, scalar: Q155::from_raw(raw) }, ()));
                at += 8;
            }
            if bytes.get(at) != Some(&0x00) {
                return None;
            }
            FramedScalarRun::new(Q155LaneFrame, offset as u64, NonEmpty::new(values)?).ok()
        })
        .collect()
}

/// Decode every complete shifted-binary32 lane in a reconstructed draft graph payload.
pub fn draft_construction_binary32_lanes(bytes: &[u8]) -> Vec<FramedScalarRun<DraftBinary32Branch, ()>> {
    let mut lanes = [DraftBinary32Branch::Form04, DraftBinary32Branch::Form03]
        .into_iter()
        .flat_map(|branch| {
            let discriminator = branch.discriminator();
            bytes
                .windows(discriminator.len())
                .enumerate()
                .filter_map(move |(offset, window)| {
                    (window == discriminator).then_some(())?;
                    let mut at = offset + discriminator.len();
                    let mut values = Vec::new();
                    while matches!(bytes.get(at), Some(0x40..=0x5f | 0xc0..=0xdf)) {
                        let scalar = ShiftedBinary32::read(bytes.get(at..at + 4)?)?;
                        values.push((scalar, ()));
                        at += 4;
                    }
                    if bytes.get(at) != Some(&0x00) {
                        return None;
                    }
                    FramedScalarRun::new(branch, offset as u64, NonEmpty::new(values)?).ok()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    lanes.sort_by_key(FramedScalarRun::offset);
    lanes
}

/// Decode a bounded datum-CSYS descriptor containing one unique maximal identity run.
pub fn datum_csys_descriptor_block(bytes: &[u8]) -> Option<csys_descriptor::CsysDescriptor> {
    csys_descriptor::CsysDescriptor::read(bytes)
}

/// Decode every complete identity frame in a reconstructed draft construction payload.
pub fn draft_construction_identity_frames(bytes: &[u8]) -> Vec<draft_identity::DraftIdentityFrame> {
    (0..bytes.len()).filter_map(|offset| draft_identity::DraftIdentityFrame::read(bytes, offset)).collect()
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

fn counted_u32_atoms(bytes: &[u8], at: &mut usize) -> Option<Vec<LocatedCompactIndex<usize, WrappedCompactIndex>>> {
    if bytes.get(*at) != Some(&0x01) { return None; }
    let count = usize::from(*bytes.get(*at + 1)?);
    if count < 2 { return None; }
    *at += 2;
    let mut values = Vec::with_capacity(count - 1);
    for _ in 1..count {
        let atom = WrappedCompactIndex::read(View::u32_be_at(bytes, *at)?)?;
        values.push(LocatedCompactIndex { atom, offset: *at });
        *at += 4;
    }
    Some(values)
}

fn counted_compact_values(bytes: &[u8], at: &mut usize) -> Option<Vec<LocatedCompactIndex>> {
    if bytes.get(*at) != Some(&0x01) {
        return None;
    }
    let count = usize::from(*bytes.get(*at + 1)?);
    if count < 2 {
        return None;
    }
    *at += 2;
    let mut values = Vec::with_capacity(count - 1);
    for _ in 1..count {
        let token = LocatedCompactIndex::read(bytes, *at)?;
        *at += token.atom.raw().len();
        values.push(token);
    }
    Some(values)
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
    let mut at = references_start;
    let mut references = Vec::with_capacity(usize::from(count - 1));
    for _ in 1..count {
        let (object_index, width) = payload_object_index(record.payload.get(at..)?)?;
        references.push(PayloadObjectReference {
            offset: record.payload_offset + at,
            token: object_index,
        });
        at += width;
    }
    if record.payload.get(at..at + 3) != Some(&[0x01, 0x03, 0x79]) {
        return None;
    }
    let encoded_references = record.payload.get(references_start..at)?;
    let witness_len = 2 + encoded_references.len() + 2;
    let witness_starts = record
        .payload
        .windows(witness_len)
        .enumerate()
        .filter_map(|(witness_start, candidate)| {
            (candidate.starts_with(&[0x01, count])
                && candidate.get(2..2 + encoded_references.len()) == Some(encoded_references)
                && candidate.ends_with(&[0x00, 0x00]))
            .then_some(witness_start)
        })
        .collect::<Vec<_>>();
    let witness_start = match witness_starts.as_slice() {
        [witness_start] => Some(*witness_start),
        _ => None,
    };
    Some(ExtrudeProfileReferenceField {
        field_tag: record.payload[start + 2],
        references: references
            .into_iter()
            .map(|reference| {
                let relative_offset = reference.offset - record.payload_offset - references_start;
                ExtrudeProfileReference {
                    reference,
                    witness_offset: witness_start
                        .map(|start| record.payload_offset + start + 2 + relative_offset),
                }
            })
            .collect(),
    })
}

/// Decode the unique `04, length, p<decimal>[_qualifier], 00` declaration name.
pub fn expression_declaration_name(bytes: &[u8]) -> Option<ExpressionDeclarationName<'_>> {
    let mut declaration = None;
    let mut literal = None;
    let mut multiple_literals = false;
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
        let Some(name) = ParameterName::<_, u32>::parse(value) else {
            if evaluate_constant_expression(value).is_some() && literal.replace(value).is_some() {
                multiple_literals = true;
            }
            continue;
        };
        let next = ExpressionDeclarationName {
            offset: at,
            name,
            literal: None,
        };
        if declaration.replace(next).is_some() {
            return None;
        }
    }
    let declaration = declaration?;
    let literal = (!multiple_literals).then_some(literal).flatten();
    Some(ExpressionDeclarationName {
        literal,
        ..declaration
    })
}

/// Decode the unique direct primary-body field in one operation.
pub fn operation_body_reference(record: OperationRecord<'_>) -> Option<OperationBodyReference> {
    unique_candidate(operation_body_reference_candidates(record))
}

fn operation_body_reference_candidates(
    record: OperationRecord<'_>,
) -> impl Iterator<Item = OperationBodyReference> + '_ {
    let payload_start = record.payload_offset.checked_sub(record.offset());
    let mut cursor = 0usize;
    std::iter::from_fn(move || loop {
        let window_end = cursor.checked_add(3)?;
        let window = record.bytes.get(cursor..window_end)?;
        let marker = cursor;
        cursor += 1;
        let body_write = payload_start
            .and_then(|payload_start| marker.checked_sub(payload_start))
            .and_then(|payload_marker| {
                operation_body_write_frame_at(record.payload, record.payload_offset, payload_marker)
            });
        if let Some(body_write) = body_write {
            if let Some(end) = body_write.end_offset.checked_sub(record.offset()) {
                cursor = cursor.max(end);
            }
            continue;
        }
        if window == [0x01, 0x02, 0x10] {
            let token = marker + 3;
            let Some((Some(object_index), end)) = feature_object_index(record.bytes, token) else {
                continue;
            };
            if record.bytes.get(end) != Some(&0xff) {
                continue;
            }
            return Some(OperationBodyReference {
                offset: record.offset() + token,
                object_index,
                raw_object_index: record.bytes[token..end].to_vec(),
            });
        }
    })
}

/// Decode every ordered direct primary-body field in one operation.
pub fn operation_body_references(record: OperationRecord<'_>) -> Vec<OperationBodyReference> {
    operation_body_reference_candidates(record).collect()
}

/// Decode every exact nested `01 02 tag index 97 75 01 02 endpoint_tag index ff` frame.
///
/// Both indices are non-null and canonical. Endpoint tags `10`, `12`, and
/// `15` select the body-image field across the supported schema generations.
pub fn operation_body_write_frames(record: OperationRecord<'_>) -> Vec<OperationBodyWriteFrame> {
    body_write_frames(record.payload, record.payload_offset)
}

/// Decode body-write frames from one independently bounded unlabeled record.
pub fn unlabeled_operation_body_write_frames(
    record: UnlabeledOperationRecord<'_>,
) -> Vec<OperationBodyWriteFrame> {
    body_write_frames(record.payload, record.payload_offset)
}

fn body_write_frames(payload: &[u8], payload_offset: usize) -> Vec<OperationBodyWriteFrame> {
    let mut relations = Vec::new();
    for marker in payload
        .windows(2)
        .enumerate()
        .filter_map(|(offset, window)| (window == [0x01, 0x02]).then_some(offset))
    {
        if let Some(write) = operation_body_write_frame_at(payload, payload_offset, marker) {
            relations.push(write);
        }
    }
    relations
}

fn operation_body_write_frame_at(
    payload: &[u8],
    payload_offset: usize,
    marker: usize,
) -> Option<OperationBodyWriteFrame> {
    let body_identity = *payload.get(marker + 2)?;
    let first_token = marker + 3;
    let (Some(first_object_index), first_end) =
        operation_relation_object_index(payload, first_token)?
    else {
        return None;
    };
    let raw_first_object_index = payload.get(first_token..first_end)?;
    if !canonical_operation_relation_object_index(Some(first_object_index), raw_first_object_index)
        || payload.get(first_end..first_end + 4) != Some(&[0x97, 0x75, 0x01, 0x02])
    {
        return None;
    }
    let endpoint_tag = *payload.get(first_end + 4)?;
    matches!(endpoint_tag, 0x10 | 0x12 | 0x15).then_some(())?;
    let second_token = first_end + 5;
    let (Some(second_object_index), second_end) =
        operation_relation_object_index(payload, second_token)?
    else {
        return None;
    };
    let raw_second_object_index = payload.get(second_token..second_end)?;
    if !canonical_operation_relation_object_index(
        Some(second_object_index),
        raw_second_object_index,
    ) || payload.get(second_end) != Some(&0xff)
    {
        return None;
    }
    Some(OperationBodyWriteFrame {
        offset: payload_offset + marker,
        body_identity,
        group_node: first_object_index,
        raw_group_node: raw_first_object_index.to_vec(),
        group_node_offset: payload_offset + first_token,
        endpoint_tag,
        body_image_object_index: second_object_index,
        raw_body_image_object_index: raw_second_object_index.to_vec(),
        body_image_object_index_offset: payload_offset + second_token,
        end_offset: payload_offset + second_end + 1,
    })
}

/// Decode every exact direct `01 02 17 index ff 80 00 00 02` field.
///
/// The fixed suffix separates this field from the nested body-write frame,
/// which uses the same opening marker and tag but has a different
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
        0x90 => Some((Some(u32::from(View::u16_be_at(bytes, at + 1)?)), at + 3)),
        0xff => Some((None, at + 1)),
        _ => None,
    }
}

fn operation_relation_object_index(bytes: &[u8], at: usize) -> Option<(Option<u32>, usize)> {
    let token = OperationStateIndex::read_at(bytes, at, 0)?;
    Some((token.value(), at + token.raw().len()))
}

fn operation_state_counter_row(
    bytes: &[u8],
    at: usize,
    base_offset: usize,
) -> Option<OperationStateCounter> {
    if bytes.get(at) != Some(&0x05) {
        return None;
    }
    let row_kind = OperationStateCounterKind::try_from(*bytes.get(at + 1)?).ok()?;
    let object_at = at.checked_add(2)?;
    let object_index = NonNullStateIndex::from_index(OperationStateIndex::read_at(bytes, object_at, base_offset)?)?;
    let state_at = object_at.checked_add(object_index.raw().len())?;
    let introduced_state = *bytes.get(state_at)?;
    let modified_state = *bytes.get(state_at + 1)?;
    let end = state_at.checked_add(3)?;
    (bytes.get(end - 1) == Some(&0x4e)).then_some(OperationStateCounter {
        span: SourceSpan::new(base_offset, at, end)?,
        row_kind,
        object_index,
        introduced_state,
        modified_state,
    })
}

/// Decode the contiguous operation-state counter-map suffix of a bounded area.
///
/// The map is selected by the longest run of complete `05, row_kind, index,
/// state, state, 4e` rows whose remaining bounded tail is small enough to be
/// an area footer. This end anchor prevents a syntactically valid short lane in
/// an operation payload from becoming a state map.
pub fn operation_state_counter_map(
    bytes: &[u8],
    base_offset: usize,
) -> Option<OperationStateCounterMap<'_>> {
    const MAX_COUNTER_TAIL_BYTES: usize = 64;
    let mut best: Option<(usize, usize, usize)> = None;
    let mut run_start = 0;
    let mut run_end = 0;
    let mut run_len = 0;
    for at in 0..bytes.len().saturating_sub(2) {
        if bytes.get(at) != Some(&0x05) || !matches!(bytes.get(at + 1), Some(0x01 | 0x02)) {
            continue;
        }
        let Some(row) = operation_state_counter_row(bytes, at, base_offset) else {
            continue;
        };
        let row_end = row.span.local_end();
        if at == run_end {
            run_end = row_end;
            run_len += 1;
        } else {
            run_start = at;
            run_end = row_end;
            run_len = 1;
        }
        if run_len >= 2
            && bytes.len().saturating_sub(run_end) <= MAX_COUNTER_TAIL_BYTES
            && best.is_none_or(|(_, _, current_len)| run_len > current_len)
        {
            best = Some((run_start, run_end, run_len));
        }
    }
    let (start, end, row_count) = best?;
    let mut rows = Vec::with_capacity(row_count);
    let mut cursor = start;
    while cursor < end {
        let row = operation_state_counter_row(bytes, cursor, base_offset)?;
        cursor = row.span.local_end();
        rows.push(row);
    }
    (cursor == end).then_some(OperationStateCounterMap {
        offset: base_offset.checked_add(start)?,
        end_offset: base_offset.checked_add(end)?,
        rows,
        trailing_bytes: bytes.get(end..)?,
    })
}

fn operation_state_message_at(
    bytes: &[u8],
    at: usize,
    base_offset: usize,
) -> Option<OperationStateMessage<'_>> {
    if bytes.get(at) != Some(&0x03) {
        return None;
    }
    let declared_length = *bytes.get(at + 1)?;
    let text_end = at.checked_add(usize::from(declared_length))?;
    let text = bytes.get(at + 2..text_end)?;
    let text = StateMessageText::new(std::str::from_utf8(text).ok()?).ok()?;
    let terminator = text_end;
    (bytes.get(terminator) == Some(&0)).then_some(())?;
    let zeros_start = terminator.checked_add(1)?;
    let zeros_end = zeros_start.checked_add(4)?;
    (bytes.get(zeros_start..zeros_end) == Some(&[0, 0, 0, 0])).then_some(())?;
    let value = StateTaggedValue::read_at(bytes, zeros_end)?;
    let count_at = zeros_end.checked_add(value.raw().len())?;
    let count_or_severity = View::u16_be_at(bytes, count_at)?;
    let end = count_at.checked_add(2)?;
    Some(OperationStateMessage {
        span: SourceSpan::new(base_offset, at, end)?,
        text,
        value,
        count_or_severity,
    })
}

fn operation_state_status_end_at(
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
    opaque_lane_starts: Option<&[usize]>,
) -> Option<usize> {
    if bytes.get(at..at + 3) == Some(&[0x02, 0x01, 0x11]) {
        let precomputed_end = opaque_lane_starts
            .and_then(|starts| operation_state_opaque_lane_end_at(starts, at, end));
        precomputed_end.or_else(|| operation_state_slot_lane_end_at(bytes, at, end))
    } else {
        operation_state_status_row_at(bytes, at, end, base_offset, opaque_lane_starts)
            .map(|row| row.span.local_end())
    }
}

#[derive(Clone, Copy)]
struct OperationStatePath {
    length: usize,
    end: usize,
}

fn operation_state_path_at(
    paths: &[(usize, OperationStatePath)],
    at: usize,
) -> Option<OperationStatePath> {
    paths
        .binary_search_by(|(offset, _)| offset.cmp(&at).reverse())
        .ok()
        .map(|index| paths[index].1)
}

fn operation_state_opaque_lane_end_at(
    lane_starts: &[usize],
    at: usize,
    end: usize,
) -> Option<usize> {
    let index = lane_starts.binary_search(&at).unwrap_or_else(|index| index);
    let lane_start = *lane_starts.get(index)?;
    let lane_end = lane_start.checked_add(2)?;
    (lane_end <= end).then_some(lane_end)
}

fn operation_state_block_before_boundary(
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Option<OperationStateBlock<'_>> {
    const MAX_STATE_BLOCK_TAIL_BYTES: usize = 64 * 1024;

    if start >= end || end > bytes.len() {
        return None;
    }

    let mut opaque_lane_starts = Vec::new();
    for at in start..end.saturating_sub(1) {
        if bytes.get(at..at + 2) == Some(&[0x02, 0x11]) {
            opaque_lane_starts.try_reserve(1).ok()?;
            opaque_lane_starts.push(at);
        }
    }

    let mut status_paths = Vec::new();
    let mut message_paths = Vec::new();
    for at in (start..end).rev() {
        if let Some(message) = operation_state_message_at(bytes, at, base_offset) {
            let next = message.span.local_end();
            if next > at && next <= end {
                let continuation = (next < end)
                    .then(|| operation_state_path_at(&message_paths, next))
                    .flatten();
                let length = continuation.map_or(Some(1), |path| path.length.checked_add(1))?;
                let path_end = continuation.map_or(next, |path| path.end);
                message_paths.try_reserve(1).ok()?;
                message_paths.push((
                    at,
                    OperationStatePath {
                        length,
                        end: path_end,
                    },
                ));
            }
        }

        let (status_length, status_end) =
            operation_state_status_end_at(bytes, at, end, base_offset, Some(&opaque_lane_starts))
                .filter(|next| *next > at && *next <= end)
                .map_or((0, usize::MAX), |next| {
                    let continuation = (next < end)
                        .then(|| operation_state_path_at(&status_paths, next))
                        .flatten();
                    let Some(length) =
                        continuation.map_or(Some(1), |path| path.length.checked_add(1))
                    else {
                        return (0, usize::MAX);
                    };
                    let path_end = continuation.map_or(next, |path| path.end);
                    (length, path_end)
                });
        let message_path = operation_state_path_at(&message_paths, at);
        let best_path =
            if status_length >= message_path.map_or(0, |path| path.length) && status_length > 0 {
                Some(OperationStatePath {
                    length: status_length,
                    end: status_end,
                })
            } else {
                message_path
            };
        if let Some(path) = best_path {
            status_paths.try_reserve(1).ok()?;
            status_paths.push((at, path));
        }
    }

    let has_exact_boundary_path = status_paths.iter().any(|(_, path)| path.end == end);
    let (offset, path) = status_paths
        .iter()
        .filter(|(at, path)| {
            if has_exact_boundary_path {
                path.end == end
            } else {
                path.end >= *at && end.saturating_sub(path.end) <= MAX_STATE_BLOCK_TAIL_BYTES
            }
        })
        .max_by_key(|(at, path)| (path.length, std::cmp::Reverse(*at)))
        .map(|(at, path)| (*at, *path))?;
    let path_end = path.end;
    let mut rows = Vec::new();
    let mut slot_lanes = Vec::new();
    let mut messages = Vec::new();
    let mut status_end_offset = base_offset.checked_add(offset)?;
    let mut at = offset;
    let mut in_messages = false;
    while at < path_end {
        if in_messages {
            let message = operation_state_message_at(bytes, at, base_offset)?;
            let next = message.span.local_end();
            (next > at && next <= path_end).then_some(())?;
            messages.push(message);
            at = next;
            continue;
        }

        let status_next =
            operation_state_status_end_at(bytes, at, end, base_offset, Some(&opaque_lane_starts));
        let status_length = status_next
            .filter(|next| {
                *next > at
                    && *next <= path_end
                    && (*next == path_end
                        || (*next < end
                            && operation_state_path_at(&status_paths, *next)
                                .is_some_and(|path| path.end == path_end)))
            })
            .map_or(0, |next| {
                if next == path_end {
                    1
                } else {
                    operation_state_path_at(&status_paths, next)
                        .and_then(|path| path.length.checked_add(1))
                        .unwrap_or(0)
                }
            });
        let message = operation_state_message_at(bytes, at, base_offset);
        let message_next = message
            .as_ref()
            .map(|message| message.span.local_end());
        let message_length = operation_state_path_at(&message_paths, at)
            .filter(|path| path.end == path_end)
            .map_or(0, |path| path.length);

        if status_length >= message_length && status_length > 0 {
            let next = status_next?;
            if bytes.get(at..at + 3) == Some(&[0x02, 0x01, 0x11]) {
                let lane = operation_state_slot_lane_at(bytes, at, end, base_offset)?;
                let lane_end = lane.span.local_end();
                (lane_end == next).then_some(())?;
                let lane_end_offset = lane.span.end_offset();
                slot_lanes.push(lane);
                at = next;
                status_end_offset = lane_end_offset;
            } else {
                let row = operation_state_status_row_at(
                    bytes,
                    at,
                    end,
                    base_offset,
                    Some(&opaque_lane_starts),
                )?;
                let row_end = row.span.local_end();
                (row_end == next).then_some(())?;
                rows.push(row);
                at = next;
                status_end_offset = row.span.end_offset();
            }
        } else {
            let message = message?;
            let next = message_next?;
            (next > at && next <= path_end && message_length > 0).then_some(())?;
            messages.push(message);
            at = next;
            in_messages = true;
        }
    }
    Some(OperationStateBlock {
        offset: base_offset.checked_add(offset)?,
        status_end_offset,
        rows,
        slot_lanes,
        messages,
    })
}

/// Decode complete message records in one already bounded state region.
#[cfg(test)]
pub fn operation_state_messages(
    bytes: &[u8],
    base_offset: usize,
) -> Vec<OperationStateMessage<'_>> {
    let mut messages = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let Some(message) = operation_state_message_at(bytes, at, base_offset) else {
            at += 1;
            continue;
        };
        at = message.span.local_end();
        messages.push(message);
    }
    messages
}

fn operation_state_opaque_payload_end(bytes: &[u8], at: usize, end: usize) -> Option<usize> {
    const MAX_OPAQUE_STATUS_BYTES: usize = 64 * 1024;
    let first = *bytes.get(at)?;
    if !matches!(first, 0x02 | 0x1e | 0xff) {
        return None;
    }
    if bytes.get(at..at + 3) == Some(&[0x02, 0x01, 0x11]) {
        return Some(at + 3);
    }
    let search_end = end.min(at.saturating_add(MAX_OPAQUE_STATUS_BYTES));
    for cursor in at..search_end.saturating_sub(1) {
        if bytes.get(cursor..cursor + 2) == Some(&[0x02, 0x11]) {
            return Some(cursor + 2);
        }
    }
    None
}

fn operation_state_link_payload(
    bytes: &[u8],
    payload_at: usize,
    end: usize,
    base_offset: usize,
) -> Option<(OperationStateStatusPayload<'_>, usize)> {
    let link_code = StateLinkCode::try_from(*bytes.get(payload_at)?).ok()?;
    if bytes.get(payload_at + 1) != Some(&0xff) {
        return None;
    }
    let linked_at = payload_at.checked_add(2)?;
    let linked = NonNullStateIndex::from_index(OperationStateIndex::read_at(bytes, linked_at, base_offset)?)?;
    let sentinel_at = linked_at.checked_add(linked.raw().len())?;
    if bytes.get(sentinel_at) != Some(&0xff) {
        return None;
    }
    let payload_end = sentinel_at.checked_add(1)?;
    (payload_end <= end).then_some((
        OperationStateStatusPayload::Linked {
            link_code,
            object_index: linked,
        },
        payload_end,
    ))
}

fn operation_state_slot_lane_at(
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
) -> Option<OperationStateSlotLane> {
    if bytes.get(at..at + 3) != Some(&[0x02, 0x01, 0x11]) {
        return None;
    }
    let mut slots = Vec::new();
    let mut cursor = at + 3;
    while cursor < end {
        if bytes.get(cursor..cursor + 2) == Some(&[0x02, 0x11]) {
            let lane_end = cursor + 2;
            return Some(OperationStateSlotLane {
                span: SourceSpan::new(base_offset, at, lane_end)?,
                slots: StateSlots::new(slots).ok()?,
            });
        }
        let slot = OperationStateIndex::read_at(bytes, cursor, base_offset)?;
        cursor = cursor.checked_add(slot.raw().len())?;
        slots.push(slot);
    }
    None
}

fn operation_state_slot_lane_end_at(bytes: &[u8], at: usize, end: usize) -> Option<usize> {
    if bytes.get(at..at + 3) != Some(&[0x02, 0x01, 0x11]) {
        return None;
    }
    let mut cursor = at + 3;
    while cursor < end {
        if bytes.get(cursor..cursor + 2) == Some(&[0x02, 0x11]) {
            return cursor.checked_add(2);
        }
        let slot = OperationStateIndex::read_at(bytes, cursor, 0)?;
        cursor = cursor.checked_add(slot.raw().len())?;
    }
    None
}

fn operation_state_status_row_at<'a>(
    bytes: &'a [u8],
    at: usize,
    end: usize,
    base_offset: usize,
    opaque_lane_starts: Option<&[usize]>,
) -> Option<OperationStateStatus<'a>> {
    let status_code =
        NonNullStateIndex::from_index(OperationStateIndex::read_at(bytes, at, base_offset)?)?;
    let object_at = at.checked_add(status_code.raw().len())?;
    let object_index = NonNullStateIndex::from_index(OperationStateIndex::read_at(bytes, object_at, base_offset)?)?;
    let payload_at = object_at.checked_add(object_index.raw().len())?;
    if payload_at >= end {
        return None;
    }
    let (payload, payload_end) = match bytes[payload_at] {
        0x3f => (OperationStateStatusPayload::Plain, payload_at + 1),
        0x03 => {
            let message = operation_state_message_at(bytes, payload_at, base_offset)?;
            let payload_end = message.span.local_end();
            (
                OperationStateStatusPayload::Diagnostic { message },
                payload_end,
            )
        }
        0x02 | 0x1e | 0xff => {
            let precomputed_end = opaque_lane_starts
                .and_then(|starts| operation_state_opaque_lane_end_at(starts, payload_at, end));
            let payload_end = precomputed_end
                .or_else(|| operation_state_opaque_payload_end(bytes, payload_at, end))?;
            (
                OperationStateStatusPayload::Opaque {
                    raw: bytes.get(payload_at..payload_end)?,
                },
                payload_end,
            )
        }
        _ => operation_state_link_payload(bytes, payload_at, end, base_offset)?,
    };
    (payload_end <= end).then_some(OperationStateStatus {
        span: SourceSpan::new(base_offset, at, payload_end)?,
        status_code,
        object_index,
        payload,
    })
}

/// Decode a bounded sequence of per-object operation-state status rows.
#[cfg(test)]
pub fn operation_state_status_table(
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Option<OperationStateStatusTable<'_>> {
    if start >= end || end > bytes.len() {
        return None;
    }
    let mut rows = Vec::new();
    let mut slot_lanes = Vec::new();
    let mut at = start;
    while at < end {
        if operation_state_message_at(bytes, at, base_offset).is_some() {
            break;
        }
        if bytes.get(at..at + 3) == Some(&[0x02, 0x01, 0x11]) {
            let lane = operation_state_slot_lane_at(bytes, at, end, base_offset)?;
            at = lane.span.local_end();
            slot_lanes.push(lane);
            continue;
        }
        let Some(row) = operation_state_status_row_at(bytes, at, end, base_offset, None) else {
            break;
        };
        at = row.span.local_end();
        rows.push(row);
    }
    (!rows.is_empty()).then_some(OperationStateStatusTable {
        offset: base_offset.checked_add(start)?,
        end_offset: base_offset.checked_add(at)?,
        rows,
        slot_lanes,
        trailing_bytes: bytes.get(at..end)?,
    })
}

fn operation_state_group_header_at(
    bytes: &[u8],
    at: usize,
) -> Option<(OperationStateGroupOpener, OperationStateGroupCount, usize)> {
    let raw_opener: [u8; 2] = bytes.get(at..at + 2)?.try_into().ok()?;
    let opener = OperationStateGroupOpener::try_from(raw_opener).ok()?;
    let count_at = at.checked_add(2)?;
    let (count, cursor) = match bytes.get(count_at) {
        Some(0) => (OperationStateGroupCount::Empty, count_at + 1),
        Some(1) => (
            OperationStateGroupCount::Counted(*bytes.get(count_at + 1)?),
            count_at + 2,
        ),
        _ => return None,
    };
    Some((opener, count, cursor))
}

fn operation_state_group_row_at(
    bytes: &[u8],
    cursor: usize,
    base_offset: usize,
) -> Option<(OperationStateGroupRow, usize)> {
    let tag = *bytes.get(cursor)?;
    match tag {
        0x4a => {
            let object_at = cursor.checked_add(1)?;
            let object_index = NonNullStateIndex::from_index(OperationStateIndex::read_at(bytes, object_at, base_offset)?)?;
            let position_at = object_at.checked_add(object_index.raw().len())?;
            let position = NonNullStateIndex::from_index(OperationStateIndex::read_at(bytes, position_at, base_offset)?)?;
            let sentinel_at = position_at.checked_add(position.raw().len())?;
            let row_end = sentinel_at.checked_add(1)?;
            (bytes.get(sentinel_at) == Some(&0xff)).then_some((
                OperationStateGroupRow::List {
                    offset: base_offset.checked_add(cursor)?,
                    object_index,
                    position,
                },
                row_end,
            ))
        }
        tag => {
            let tag = OperationStatePairTag::try_from(tag).ok()?;
            let first_at = cursor.checked_add(1)?;
            let first = NonNullStateIndex::from_index(OperationStateIndex::read_at(bytes, first_at, base_offset)?)?;
            let second_at = first_at.checked_add(first.raw().len())?;
            let second = NonNullStateIndex::from_index(OperationStateIndex::read_at(bytes, second_at, base_offset)?)?;
            let sentinels_at = second_at.checked_add(second.raw().len())?;
            let row_end = sentinels_at.checked_add(2)?;
            (bytes.get(sentinels_at..row_end) == Some(&[0xff, 0xff])).then_some((
                OperationStateGroupRow::Pair {
                    offset: base_offset.checked_add(cursor)?,
                    tag,
                    first,
                    second,
                },
                row_end,
            ))
        }
    }
}

fn operation_state_group_end_at(
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
) -> Option<usize> {
    let (_, count, mut cursor) = operation_state_group_header_at(bytes, at)?;
    let member_count = usize::from(count.declared_count().saturating_sub(1));
    for _ in 0..member_count {
        cursor = operation_state_group_row_at(bytes, cursor, base_offset)?.1;
    }
    (cursor <= end).then_some(cursor)
}

fn operation_state_group_at(
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
) -> Option<OperationStateGroup> {
    let (opener, count, mut cursor) = operation_state_group_header_at(bytes, at)?;
    let member_count = usize::from(count.declared_count().saturating_sub(1));
    let mut rows = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        let (row, row_end) = operation_state_group_row_at(bytes, cursor, base_offset)?;
        rows.push(row);
        cursor = row_end;
    }
    (cursor <= end).then_some(OperationStateGroup {
        span: SourceSpan::new(base_offset, at, cursor)?,
        opener,
        members: StateGroupMembers::new(count, rows).ok()?,
    })
}

fn operation_state_group_table_before_counter_map(
    bytes: &[u8],
    map_start: usize,
    base_offset: usize,
) -> Option<OperationStateGroupTable<'_>> {
    #[derive(Clone, Copy)]
    struct GroupPath {
        last_candidate: usize,
        length: usize,
        first_start: usize,
    }

    if map_start > bytes.len() {
        return None;
    }
    let mut candidates = Vec::new();
    for at in 0..map_start.saturating_sub(2) {
        if !matches!(bytes.get(at..at + 2), Some([0x01, 0x00 | 0x01])) {
            continue;
        }
        let Some(end) = operation_state_group_end_at(bytes, at, map_start, base_offset) else {
            continue;
        };
        candidates.push((at, end));
    }
    candidates.sort_by_key(|(start, end)| (*end, *start));

    let mut predecessors = cadmpeg_core::decode::alloc_filled(
        candidates.len(),
        None,
        "nx operation-state group predecessors",
    )
    .ok()?;
    let mut best_by_end = BTreeMap::<usize, GroupPath>::new();
    for (candidate_index, (start, end)) in candidates.iter().enumerate() {
        let previous = best_by_end.get(start).copied();
        let path = GroupPath {
            last_candidate: candidate_index,
            length: previous.map_or(1, |path| path.length + 1),
            first_start: previous.map_or(*start, |path| path.first_start),
        };
        predecessors[candidate_index] = previous.map(|path| path.last_candidate);
        let replace = best_by_end.get(end).is_none_or(|current| {
            path.length > current.length
                || (path.length == current.length && path.first_start < current.first_start)
        });
        if replace {
            best_by_end.insert(*end, path);
        }
    }

    let trailing_start =
        if map_start >= 2 && bytes.get(map_start - 2..map_start) == Some(&[0x01, 0x01]) {
            map_start - 2
        } else {
            map_start
        };
    let terminal = best_by_end.get(&trailing_start).copied()?;
    let mut path = Vec::with_capacity(terminal.length);
    let mut candidate = Some(terminal.last_candidate);
    while let Some(candidate_index) = candidate {
        path.push(candidate_index);
        candidate = predecessors[candidate_index];
    }
    path.reverse();
    let first = *path.first()?;
    let last = *path.last()?;
    let groups = path
        .into_iter()
        .map(|candidate| {
            operation_state_group_at(bytes, candidates[candidate].0, map_start, base_offset)
        })
        .collect::<Option<Vec<_>>>()?;
    Some(OperationStateGroupTable {
        offset: base_offset.checked_add(candidates[first].0)?,
        end_offset: base_offset.checked_add(map_start)?,
        groups,
        trailing_bytes: bytes.get(candidates[last].1..map_start)?,
    })
}

/// Decode a complete bounded `m_rollForwardStates` group table.
#[cfg(test)]
pub fn operation_state_group_table(
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Option<OperationStateGroupTable<'_>> {
    if start >= end || end > bytes.len() {
        return None;
    }
    let mut groups = Vec::new();
    let mut at = start;
    let mut trailing_start = end;
    while at < end {
        let Some(group) = operation_state_group_at(bytes, at, end, base_offset) else {
            if bytes.get(at..end) == Some(&[0x01, 0x01]) {
                trailing_start = at;
                at = end;
                break;
            }
            return None;
        };
        at = group.span.local_end();
        groups.push(group);
    }
    (!groups.is_empty() && at == end).then_some(OperationStateGroupTable {
        offset: base_offset.checked_add(start)?,
        end_offset: base_offset.checked_add(end)?,
        groups,
        trailing_bytes: bytes.get(trailing_start..end)?,
    })
}

fn operation_state_journal_row_at(
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
) -> Option<OperationStateJournalRow> {
    if bytes.get(at) != Some(&0xe0) {
        return None;
    }
    let timestamp = View::u32_be_at(bytes, at + 1)?;
    let value = StateTaggedValue::read_at(bytes, at + 5)?;
    let schema_at = at.checked_add(5 + value.raw().len())?;
    let schema_id = NonNullStateIndex::from_index(OperationStateIndex::read_at(bytes, schema_at, base_offset)?)?;
    let ordinal_at = schema_at.checked_add(schema_id.raw().len())?;
    let ordinal = NonNullStateIndex::from_index(OperationStateIndex::read_at(bytes, ordinal_at, base_offset)?)?;
    let terminator_at = ordinal_at.checked_add(ordinal.raw().len())?;
    if terminator_at >= end || bytes.get(terminator_at) != Some(&0x13) {
        return None;
    }
    let row_end = terminator_at + 1;
    Some(OperationStateJournalRow {
        span: SourceSpan::new(base_offset, at, row_end)?,
        timestamp,
        value,
        schema_id,
        ordinal,
    })
}

fn audit_trail_row_at(
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
) -> Option<AuditTrailRow> {
    let bytes = bytes.get(..end)?;
    if bytes.get(at) != Some(&0x04) {
        return None;
    }
    let ordinal = NonNullStateIndex::from_index(OperationStateIndex::read_at(bytes, at.checked_add(1)?, base_offset)?)?;
    let mut cursor = at.checked_add(1 + ordinal.raw().len())?;
    if bytes.get(cursor) != Some(&0x13) {
        return None;
    }
    cursor += 1;

    let frame_selector = if bytes
        .get(cursor..cursor + 4)
        .is_some_and(|frame| frame[0] == 0x04 && frame[1] == 0x05 && frame[3] == 0x00)
    {
        let selector = *bytes.get(cursor + 2)?;
        cursor += 4;
        Some(selector)
    } else {
        None
    };

    if bytes.get(cursor) != Some(&0xe0) {
        return None;
    }
    let timestamp = View::u32_be_at(bytes, cursor + 1)?;
    cursor = cursor.checked_add(5)?;
    let value = StateTaggedValue::read_at(bytes, cursor)?;
    AuditTrailRow::new(base_offset, at, AuditRecord {
        ordinal: ordinal.token(), frame_selector, timestamp, value,
    })
}

/// Decode complete audit-trail rows from a bounded record-area suffix.
///
/// A row scan is admitted only when its ordinal tokens are strictly increasing
/// in source order. This rejects a coincidental inner match instead of
/// assigning a second interpretation to a row sequence. Bytes that do not
/// complete the row grammar are left untyped.
pub fn audit_trail_rows(
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Option<Vec<AuditTrailRow>> {
    if start >= end || end > bytes.len() {
        return None;
    }
    let mut rows = Vec::new();
    let mut at = start;
    let mut previous_ordinal = None;
    while at < end {
        let Some(row) = audit_trail_row_at(bytes, at, end, base_offset) else {
            at += 1;
            continue;
        };
        let ordinal = row.record().ordinal.value();
        if previous_ordinal.is_some_and(|previous| ordinal <= previous) {
            return None;
        }
        previous_ordinal = Some(ordinal);
        at = row.local_end();
        rows.push(row);
    }
    Some(rows)
}

fn operation_state_journal_group_at(
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
) -> Option<OperationStateJournalGroup> {
    if bytes.get(at) != Some(&0x04) {
        return None;
    }
    let selector = [*bytes.get(at + 1)?, *bytes.get(at + 2)?];
    if bytes.get(at + 3) != Some(&0) {
        return None;
    }
    let mut cursor = at + 4;
    if bytes.get(cursor) == Some(&0) {
        cursor += 1;
    }
    let mut rows = Vec::new();
    while cursor < end {
        let Some(row) = operation_state_journal_row_at(bytes, cursor, end, base_offset) else {
            break;
        };
        cursor = row.span.local_end();
        rows.push(row);
    }
    (!rows.is_empty()).then_some(OperationStateJournalGroup {
        span: SourceSpan::new(base_offset, at, cursor)?,
        selector,
        rows,
    })
}

fn operation_state_journal_start(bytes: &[u8], product_end: usize) -> Option<usize> {
    let marker = bytes
        .get(product_end..)?
        .windows(2)
        .position(|window| window == [0x41, 0x00])?
        .checked_add(product_end)?;
    let mut at = marker.checked_add(2)?;
    let mut count_tokens = 0usize;
    let mut saw_repeated_token = false;
    loop {
        if bytes
            .get(at..at + 3)
            .is_some_and(|token| token[0] == 0x03 && token[1] == 0x05)
        {
            count_tokens += 1;
            at += 3;
        } else if bytes.get(at..at + 4) == Some(&[0x03, 0x03, 0x02, 0x00]) {
            saw_repeated_token = true;
            at += 4;
        } else {
            break;
        }
    }
    if bytes.get(at) == Some(&0x00) {
        at += 1;
    }
    while bytes.get(at..at + 2) == Some(&[0x04, 0x00]) {
        at += 2;
    }
    (count_tokens > 0 && (saw_repeated_token || count_tokens >= 2)).then_some(at)
}

fn operation_state_journal_groups_before_boundary(
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Option<Vec<OperationStateJournalGroup>> {
    if start >= end || end > bytes.len() {
        return None;
    }
    let mut groups = Vec::new();
    let mut at = start;
    let mut previous_ordinal = None;
    loop {
        let Some(group) = operation_state_journal_group_at(bytes, at, end, base_offset) else {
            let mut next = at;
            while bytes.get(next..next + 2) == Some(&[0x04, 0x00]) {
                next += 2;
            }
            if next == at
                || operation_state_journal_group_at(bytes, next, end, base_offset).is_none()
            {
                break;
            }
            at = next;
            continue;
        };
        for row in &group.rows {
            let ordinal = row.ordinal.value();
            if previous_ordinal.is_some_and(|previous| ordinal <= previous) {
                return None;
            }
            previous_ordinal = Some(ordinal);
        }
        at = group.span.local_end();
        groups.push(group);
    }
    (!groups.is_empty()).then_some(groups)
}

/// Decode a complete bounded state journal.
#[cfg(test)]
pub fn operation_state_journal(
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Option<Vec<OperationStateJournalGroup>> {
    if start >= end || end > bytes.len() {
        return None;
    }
    let mut groups = Vec::new();
    let mut at = start;
    while at < end {
        let group = operation_state_journal_group_at(bytes, at, end, base_offset)?;
        at = group.span.local_end();
        groups.push(group);
    }
    (!groups.is_empty() && at == end).then_some(groups)
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

fn canonical_operation_relation_object_index(value: Option<u32>, raw: &[u8]) -> bool {
    matches!(
        (value, raw),
        (None, [0xff])
            | (Some(0..=0x7f), [_])
            | (Some(0x80..=0x0fff), [0x80..=0x8f, _])
            | (Some(0x1000..=0xffff), [0x90, _, _])
            | (Some(_), [0xa0..=0xaf | 0xf1, _, _])
    )
}

/// Return one decoded candidate only when the scan produced exactly one.
///
/// A number of OM fields are discovered by probing every byte in an already
/// bounded payload. Materializing all successful probes lets overlapping
/// malformed framing turn a linear scan into an allocation proportional to
/// the number of hits. The parser needs only the uniqueness decision, so keep
/// the first candidate and reject as soon as a second one is complete.
fn unique_candidate<T>(candidates: impl IntoIterator<Item = T>) -> Option<T> {
    let mut candidate = None;
    for next in candidates {
        if candidate.is_some() {
            return None;
        }
        candidate = Some(next);
    }
    candidate
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
    unique_candidate(
        (terminator.saturating_sub(9)..terminator).filter_map(|start| {
            let Some((Some(local_ordinal), first_end)) =
                feature_object_index(record.payload, start)
            else {
                return None;
            };
            let first_raw = &record.payload[start..first_end];
            if !canonical_feature_object_index(Some(local_ordinal), first_raw) {
                return None;
            }
            let Some((Some(repeated), second_end)) =
                feature_object_index(record.payload, first_end)
            else {
                return None;
            };
            let second_raw = &record.payload[first_end..second_end];
            if repeated != local_ordinal || second_raw != first_raw {
                return None;
            }
            let (object_index, object_end) = feature_object_index(record.payload, second_end)?;
            let object_raw = &record.payload[second_end..object_end];
            if object_end != terminator || !canonical_feature_object_index(object_index, object_raw)
            {
                return None;
            }
            let local_ordinal_offset = record.payload_offset + start;
            let immediate_common_frame_offset = common_frames
                .iter()
                .find(|frame| {
                    frame.local_ordinal_offset == local_ordinal_offset
                        && frame.end_offset == record.payload_offset + object_end + 1
                })
                .map(|frame| frame.offset);
            Some(OperationTerminalFrame {
                immediate_common_frame_offset,
                local_ordinal,
                raw_local_ordinal: first_raw.to_vec(),
                object_index,
                raw_object_index: object_raw.to_vec(),
                offset: local_ordinal_offset,
                object_index_offset: record.payload_offset + second_end,
            })
        }),
    )
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
                target,
                tools,
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
        let value = ReferenceIndexToken::read_feature(bytes.get(cursor..)?)?;
        let next = cursor + value.raw().len();
        values.push(PayloadObjectReference {
            offset: base_offset + cursor,
            token: value,
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
            let Some(value) = View::u16_be_at(bytes, token + 1) else {
                run.clear();
                break;
            };
            if usize::from(value) >= record_count {
                run.clear();
                break;
            }
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
    out.extend(parsed.iter().zip(parsed.iter().skip(1)).filter_map(|(persistent, tagged)| {
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
            (bytes.get(end) == Some(&0)).then_some(())?;
            let value = PrintableString::new(std::str::from_utf8(raw).ok()?).ok()?;
            Some(StringValue { offset: base_offset + offset, value })
        })
        .collect()
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
            let value = crate::canonical_uuid::CanonicalUuid::new(std::str::from_utf8(raw).ok()?).ok()?;
            (bytes.get(end) == Some(&0)).then_some(UuidStringValue {
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
        assert_eq!(values[0].value.as_str(), "01234567-89ab-cdef-0123-456789abcdef");

        bytes[6 + 2 + 9] = b'A';
        assert!(uuid_string_values(&bytes, 0).is_empty());
        assert!(crate::canonical_uuid::CanonicalUuid::new("01234567-89ab-cdef-0123-456789abcde").is_err());
        assert!(crate::canonical_uuid::CanonicalUuid::new("01234567-89ab-cdef-0123_456789abcdef").is_err());
        assert!(crate::canonical_uuid::CanonicalUuid::new("01234567-89ab-cdef-0123-456789abcdeg").is_err());
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
            let value = crate::payload_text::PayloadText::new(std::str::from_utf8(raw).ok()?).ok()?;
            (bytes.get(end) == Some(&0))
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
        let type_registry = registry::type_registry(bytes, offset + 16, end);
        let types = type_registry.definitions;
        let field_start = type_registry.field_start;
        let record_area_pointer = section_record_area_pointer(bytes, offset, field_start, end)
            .or_else(|| {
                legacy_feature_record_area_pointer(bytes, offset, field_start, end, &types)
            });
        let (fields, record_area_offset) =
            if let Some((record_area_offset, pointer_offset)) = record_area_pointer {
                (
                    registry::all_field_definitions(bytes, field_start, pointer_offset),
                    Some(record_area_offset),
                )
            } else {
                (registry::field_definitions(bytes, field_start, end), None)
            };
        let record_area = record_area_offset.map(|start| RecordArea {
            offset: start,
            bytes: &bytes[start..end],
        });
        let cached_operation_labels = record_area
            .map_or_else(Vec::new, |area| operation_labels(area.bytes, area.offset));
        out.push(Section {
            offset,
            byte_len: end - offset,
            types: types.into(),
            fields: fields.into(),
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
        ProductRecord::read(bytes.get(target.checked_add(12)?..section_end)?, ProductRecordForm::Modern).is_some().then_some((target, at))
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn legacy_feature_record_area_pointer(
    bytes: &[u8],
    section_offset: usize,
    schema_start: usize,
    section_end: usize,
    types: &[TypeDefinition<'_>],
) -> Option<(usize, usize)> {
    if !types
        .iter()
        .any(|definition| definition.name == "UGS::FEATURE_RECORD")
    {
        return None;
    }
    unique_candidate(
        (schema_start..section_end.saturating_sub(4)).filter_map(|at| {
            if bytes.get(at) != Some(&0x01) {
                return None;
            }
            let relative = usize::try_from(View::u32_le_at(bytes, at + 1)?).ok()?;
            let target = section_offset.checked_add(relative)?.checked_add(1)?;
            (target >= at.checked_add(5)? && target.checked_add(12)? <= section_end)
                .then_some(())?;
            View::u32_le_at(bytes, target)?;
            View::u32_le_at(bytes, target + 4)?;
            View::u32_le_at(bytes, target + 8)?;
            ProductRecord::read(
                bytes.get(target + 12..section_end)?,
                ProductRecordForm::LegacyFeature,
            )?;
            Some((target, at))
        }),
    )
}

#[derive(Debug, Clone, Copy)]
struct ProductRecordRange {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone)]
enum IndexedCandidateKind<'a> {
    Fixed(FixedIndex<'a>),
    OffsetOnly(OffsetIndex<'a>),
}

#[derive(Debug, Clone)]
struct IndexedCandidate<'a> {
    discovery_order: usize,
    kind: IndexedCandidateKind<'a>,
}

impl<'a> IndexedCandidate<'a> {
    fn start(&self) -> usize {
        match &self.kind {
            IndexedCandidateKind::Fixed(index) => index.index_start(),
            IndexedCandidateKind::OffsetOnly(index) => index.index_start(),
        }
    }

    fn source(&self) -> &'a [u8] {
        match &self.kind {
            IndexedCandidateKind::Fixed(index) => index.source(),
            IndexedCandidateKind::OffsetOnly(index) => index.source(),
        }
    }
}

fn product_record_range_at(bytes: &[u8], offset: usize) -> Option<ProductRecordRange> {
    let suffix = bytes.get(offset..)?;
    let layout = ProductRecord::read(suffix, ProductRecordForm::Modern)?;
    Some(ProductRecordRange {
        start: offset,
        end: offset.checked_add(layout.byte_len())?,
    })
}

fn record_area_product_end(bytes: &[u8], offset: usize) -> Option<usize> {
    let suffix = bytes.get(offset..)?;
    let layout = ProductRecord::read(suffix, ProductRecordForm::Modern)
        .or_else(|| ProductRecord::read(suffix, ProductRecordForm::LegacyFeature))?;
    offset.checked_add(layout.byte_len())
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

/// Select outer indexed interpretations before materializing section records.
///
/// The entity-index and object-id arrays are the physical ownership envelope
/// of an indexed section. A second valid table wholly inside that envelope is
/// serialized record content, not another section. Partial overlap remains in
/// the result because neither candidate owns the other candidate's bytes.
///
/// Sorting by start and then descending end makes the furthest end of all
/// admitted candidates sufficient to recognize every nested candidate. This
/// keeps the admission pass linear after sorting and, more importantly, keeps
/// rejected candidates as layout metadata rather than allocated records.
fn select_outer_indexed_candidates(mut candidates: Vec<IndexedCandidate<'_>>) -> Vec<IndexedCandidate<'_>> {
    candidates.sort_by(|left, right| {
        left.start()
            .cmp(&right.start())
            .then_with(|| right.source().len().cmp(&left.source().len()))
    });
    let mut admitted = Vec::with_capacity(candidates.len());
    let mut furthest_end = 0;
    for candidate in candidates {
        if candidate.source().len() <= furthest_end {
            continue;
        }
        furthest_end = candidate.source().len();
        admitted.push(candidate);
    }
    admitted.sort_by_key(|candidate| candidate.discovery_order);
    admitted
}

fn materialize_indexed_candidate(candidate: IndexedCandidate<'_>) -> IndexedSection<'_> {
    let bytes = candidate.source();
    let entity_index_offset = candidate.start();
    let base = match &candidate.kind {
        IndexedCandidateKind::Fixed(index) => index.base(),
        IndexedCandidateKind::OffsetOnly(_) => 0,
    };
    let type_registry = registry::type_registry(bytes, base, entity_index_offset);
    let fields = registry::all_field_definitions(bytes, type_registry.field_start, entity_index_offset);
    let (object_id_table_offset, store) = match candidate.kind {
        IndexedCandidateKind::Fixed(index) => (index.object_id_table_offset(), IndexedStore::Fixed {
            records: index.records().collect::<Vec<_>>().into(),
        }),
        IndexedCandidateKind::OffsetOnly(index) => {
            let control = index.control();
            (control.offset, IndexedStore::OffsetOnly {
                control,
                column_storage: index.column_storage(),
                records: index.records().collect::<Vec<_>>().into(),
            })
        }
    };
    IndexedSection {
        base, entity_index_offset, object_id_table_offset,
        types: type_registry.definitions.into(), fields: fields.into(), store,
    }
}

/// Locate validated NX OM entity-index/object-id-table pairs.
///
/// A candidate is accepted only when the arrays are adjacent, the index is
/// monotone, its first offset is zero, its second offset self-anchors the first
/// entity exactly at the end of the object-id table, and that entity carries the
/// NX root marker.
pub fn indexed_sections(bytes: &[u8]) -> Vec<IndexedSection<'_>> {
    let mut candidates = Vec::new();
    let mut seen_record_starts = BTreeSet::new();
    let product_record_ranges = (0..bytes.len())
        .filter_map(|offset| product_record_range_at(bytes, offset))
        .collect::<Vec<_>>();
    let descending_u32_edges = DescendingU32Edges::new(bytes);
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
        let Some(_) = product_record_range_at(bytes, table_end) else {
            continue;
        };
        if View::u32_le_at(bytes, index_start) != Some(0) {
            continue;
        }
        let Some(first) = View::u32_le_at(bytes, index_start + 4).map(|value| value as usize)
        else {
            continue;
        };
        let Some(base) = table_end.checked_sub(first) else {
            continue;
        };
        let Some(index) = FixedIndex::new(&descending_u32_edges, index_start, count, base, table) else {
            continue;
        };
        if !seen_record_starts.insert(table_end) { continue; }
        candidates.push(IndexedCandidate {
            discovery_order: candidates.len(),
            kind: IndexedCandidateKind::Fixed(index),
        });
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
        let Some(index) = OffsetIndex::new(&descending_u32_edges, index_start, offset_count, count_offset) else {
            continue;
        };
        if !seen_record_starts.insert(second) { continue; }
        candidates.push(IndexedCandidate {
            discovery_order: candidates.len(),
            kind: IndexedCandidateKind::OffsetOnly(index),
        });
    }
    select_outer_indexed_candidates(candidates)
        .into_iter()
        .map(materialize_indexed_candidate)
        .collect()
}

/// Decode the first self-framed NX product/version marker in `bytes`.
pub fn store_version(bytes: &[u8], base_offset: usize) -> Option<StoreVersion<'_>> {
    (0..bytes.len().saturating_sub(3)).find_map(|at| {
        let product = ProductRecord::read(&bytes[at..], ProductRecordForm::Modern)?;
        Some(StoreVersion { offset: base_offset.checked_add(at)?, value: product.text() })
    })
}

/// Decode the zero-prefixed offset-store control form as ordered 24-bit values.
///
/// Each word is serialized `00, value:u24 LE`. The complete form is atomic.
pub fn offset_store_control_values(bytes: &[u8]) -> Option<NonEmpty<ControlWord24>> {
    bytes.len().is_multiple_of(4).then_some(())?;
    NonEmpty::new(bytes.chunks_exact(4).map(|word| {
        (word[0] == 0).then(|| ControlWord24::new([word[1], word[2], word[3]]))
    }))?.transpose()
}

/// Decode the distinct leading class-registry identities in an offset-store
/// control block.
///
/// The registry may omit declarations that this decoder cannot type, so its
/// retained declaration count is not an ordinal bound. The class lane is
/// instead the unique nonempty prefix whose identities are distinct and all
/// smaller than every following metadata value.
pub fn offset_store_control_class_ordinals(bytes: &[u8]) -> Option<Vec<u32>> {
    let values = offset_store_control_values(bytes)?.into_iter().map(ControlWord24::value).collect::<Vec<_>>();
    let mut suffix_minima =
        alloc_filled(values.len(), u32::MAX, "nx offset-store suffix minima").ok()?;
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
    Some(values[..boundary].to_vec())
}

fn joined_control_byte(control: &[u8], first_record: &[u8], offset: usize) -> Option<u8> {
    if offset < control.len() {
        control.get(offset).copied()
    } else {
        first_record.get(offset - control.len()).copied()
    }
}

fn joined_control_u32_le(control: &[u8], first_record: &[u8], offset: usize) -> Option<u32> {
    Some(
        u32::from(joined_control_byte(control, first_record, offset)?)
            | (u32::from(joined_control_byte(control, first_record, offset + 1)?) << 8)
            | (u32::from(joined_control_byte(control, first_record, offset + 2)?) << 16)
            | (u32::from(joined_control_byte(control, first_record, offset + 3)?) << 24),
    )
}

fn offset_store_product_anchored_form(
    control: &[u8],
    first_record: &[u8],
) -> Option<OffsetStoreControlForm> {
    let product_offset = unique_candidate(
        (0..control.len())
            .filter(|offset| ProductRecord::read(&control[*offset..], ProductRecordForm::Modern).is_some())
            .chain(
                (0..first_record.len())
                    .filter(|offset| ProductRecord::read(&first_record[*offset..], ProductRecordForm::Modern).is_some())
                    .map(|offset| control.len() + offset),
            ),
    )?;
    let leading_width = product_offset % 4;
    if product_offset >= control.len() {
        let control_array_bytes = control.len().checked_sub(leading_width)?;
        (!control_array_bytes.is_multiple_of(4)).then_some(())?;
    }
    let leading_value = if leading_width == 0 {
        None
    } else {
        Some(ControlLeadingValue::read(leading_width, control.iter().chain(first_record).copied())?)
    };
    let values = NonEmpty::new((0..(product_offset - leading_width) / 4)
        .map(|index| joined_control_u32_le(control, first_record, leading_width + index * 4)))?.transpose()?;
    Some(OffsetStoreControlForm::ProductAnchored {
        leading_value,
        values,
    })
}

/// One complete admitted offset-only store control-block form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OffsetStoreControlForm {
    /// Complete `00 + value:u24 LE` word array.
    ZeroPrefixed {
        /// Ordered values decoded from the complete control block.
        values: NonEmpty<ControlWord24>,
    },
    /// Compact leading value and aligned `u32 LE` array preceding one
    /// self-framed product record.
    ProductAnchored {
        /// Width and value of the compact leading little-endian integer.
        leading_value: Option<ControlLeadingValue>,
        /// Ordered values preceding the product record.
        values: NonEmpty<u32>,
    },
}

/// Classify one complete offset-only store control lane atomically.
///
/// Product-anchored storage may cross the physical boundary between the
/// control block and the first column block. Exactly one admitted grammar must
/// accept the complete control envelope.
pub fn offset_store_control_form(
    control: &[u8],
    first_record: Option<&[u8]>,
) -> Option<OffsetStoreControlForm> {
    let zero = offset_store_control_values(control);
    let product = offset_store_product_anchored_form(control, first_record.unwrap_or_default());
    match (zero, product) {
        (Some(values), None) => Some(OffsetStoreControlForm::ZeroPrefixed { values }),
        (_, Some(form)) => Some(form),
        (None, None) => None,
    }
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
    let text = text.strip_prefix("(Number [")?;
    let (unit, rest) = text.split_once("]) ")?;
    let unit = crate::om_tokens::unit_for(unit)?;
    let (name, value_tail) = rest.split_once(": ")?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return None;
    }
    let (value_text, comment) = value_tail.split_once("; ")?;
    if !comment.is_empty() && !numeric_expression_comment_is_valid(comment) {
        return None;
    }
    let value = evaluate_constant_expression(value_text);
    Some(NumericExpression {
        object_id,
        offset: base_offset + relative,
        name: ParameterName::new(name),
        unit,
        expression: value_text,
        value,
    })
}

fn numeric_expression_comment_is_valid(comment: &str) -> bool {
    comment.starts_with("//")
        && comment
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

/// Evaluate the context-free arithmetic subset of NX numeric formulas.
/// Names and function calls fail; they need the parameter graph.
pub(crate) fn evaluate_constant_expression(text: &str) -> Option<f64> {
    // This is the expression grammar's explicit operator stack. Do not turn
    // nested parentheses or unary signs back into recursive descent: formula
    // text is untrusted input, and a valid bounded record must not consume the
    // decoder stack merely because its nesting is deep.
    #[derive(Debug, Clone, Copy)]
    enum Operator {
        OpenParen,
        Unary(u8),
        Binary(u8),
    }

    impl Operator {
        fn precedence(self) -> u8 {
            match self {
                Self::OpenParen => 0,
                Self::Binary(b'+' | b'-') => 1,
                Self::Binary(b'*' | b'/') => 2,
                Self::Unary(_) => 3,
                Self::Binary(b'^') => 4,
                Self::Binary(_) => 0,
            }
        }

        fn is_right_associative(self) -> bool {
            matches!(self, Self::Unary(_) | Self::Binary(b'^'))
        }
    }

    struct Parser<'a> {
        bytes: &'a [u8],
        at: usize,
        values: Vec<f64>,
        operators: Vec<Operator>,
        expect_operand: bool,
    }

    impl Parser<'_> {
        fn spaces(&mut self) {
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
                self.at += 1;
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
            let value: f64 = std::str::from_utf8(&self.bytes[start..self.at])
                .ok()?
                .parse()
                .ok()?;
            value.is_finite().then_some(value)
        }

        fn apply_top(&mut self) -> Option<()> {
            let operator = self.operators.pop()?;
            let value = match operator {
                Operator::OpenParen => return None,
                Operator::Unary(operator) => {
                    let value = self.values.pop()?;
                    match operator {
                        b'+' => value,
                        b'-' => -value,
                        _ => return None,
                    }
                }
                Operator::Binary(operator) => {
                    let right = self.values.pop()?;
                    let left = self.values.pop()?;
                    match operator {
                        b'+' => left + right,
                        b'-' => left - right,
                        b'*' => left * right,
                        b'/' => left / right,
                        b'^' => left.powf(right),
                        _ => return None,
                    }
                }
            };
            value.is_finite().then(|| self.values.push(value))
        }

        fn push_binary(&mut self, operator: u8) -> Option<()> {
            let incoming = Operator::Binary(operator);
            while self.operators.last().is_some_and(|top| {
                !matches!(top, Operator::OpenParen)
                    && (top.precedence() > incoming.precedence()
                        || (top.precedence() == incoming.precedence()
                            && !incoming.is_right_associative()))
            }) {
                self.apply_top()?;
            }
            self.operators.push(incoming);
            self.expect_operand = true;
            Some(())
        }

        fn close_group(&mut self) -> Option<()> {
            while self
                .operators
                .last()
                .is_some_and(|operator| !matches!(operator, Operator::OpenParen))
            {
                self.apply_top()?;
            }
            matches!(self.operators.pop(), Some(Operator::OpenParen)).then_some(())
        }

        fn parse(mut self) -> Option<f64> {
            while self.at < self.bytes.len() {
                self.spaces();
                if self.at == self.bytes.len() {
                    break;
                }
                let byte = *self.bytes.get(self.at)?;
                if self.expect_operand {
                    match byte {
                        b'+' | b'-' => {
                            self.operators.push(Operator::Unary(byte));
                            self.at += 1;
                        }
                        b'(' => {
                            self.operators.push(Operator::OpenParen);
                            self.at += 1;
                        }
                        _ => {
                            let value = self.number()?;
                            self.values.push(value);
                            self.expect_operand = false;
                        }
                    }
                } else {
                    match byte {
                        b'+' | b'-' | b'*' | b'/' | b'^' => {
                            self.push_binary(byte)?;
                            self.at += 1;
                        }
                        b')' => {
                            self.close_group()?;
                            self.at += 1;
                        }
                        _ => return None,
                    }
                }
            }
            if self.expect_operand {
                return None;
            }
            while let Some(operator) = self.operators.last() {
                if matches!(operator, Operator::OpenParen) {
                    return None;
                }
                self.apply_top()?;
            }
            (self.values.len() == 1).then(|| self.values[0])
        }
    }

    Parser {
        bytes: text.as_bytes(),
        at: 0,
        values: Vec::new(),
        operators: Vec::new(),
        expect_operand: true,
    }
    .parse()
}

#[cfg(test)]
mod tests;
