// SPDX-License-Identifier: Apache-2.0
//! Extract and classify compressed streams in an NX part payload.
//!
//! [`extract_streams`] scans the canonical `/Root/UG_PART/UG_PART` file span for
//! valid zlib headers. An inflated `PS 00 00` prologue identifies Parasolid
//! neutral-binary data and supplies its subtype and optional `SCH_` schema token.
//! Other inflated payloads are classified as [`StreamKind::Preview`].
#![deny(clippy::disallowed_methods)]

use std::collections::BTreeSet;

use cadmpeg_container::compression::inflate_zlib_member;
use cadmpeg_core::bytes::{contains, find};
use cadmpeg_core::decode::{ByteRange, DecodeContext, ExpandSpec, View};
use cadmpeg_core::CodecError;

use crate::container::Container;
use crate::framing::read_and_advance as read_xmt;
use crate::vec3_at::vec3_be_at;

/// Classification of an inflated payload in the part stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// A Parasolid `(partition)` body snapshot.
    Partition,
    /// A Parasolid `(deltas)` edit overlay.
    Deltas,
    /// A cached Parasolid body without a partition or deltas subtype.
    Plain,
    /// An inflated non-Parasolid payload, such as preview or metadata data.
    Preview,
}

impl StreamKind {
    /// Return the stable label used in summaries and reports.
    pub fn label(self) -> &'static str {
        match self {
            StreamKind::Partition => "partition",
            StreamKind::Deltas => "deltas",
            StreamKind::Plain => "plain",
            StreamKind::Preview => "preview",
        }
    }

    /// Return whether this kind contains Parasolid neutral-binary records.
    pub fn is_parasolid(self) -> bool {
        !matches!(self, StreamKind::Preview)
    }

    /// Return the `CHART_s` Hvec layout required by this stream kind.
    pub(crate) fn chart_point_layout(self) -> Option<crate::intersection::ChartPointLayout> {
        match self {
            StreamKind::Partition | StreamKind::Plain => {
                Some(crate::intersection::ChartPointLayout::Xyz3)
            }
            StreamKind::Deltas => Some(crate::intersection::ChartPointLayout::Ext11),
            StreamKind::Preview => None,
        }
    }
}

/// A located and inflated stream from the canonical part payload.
#[derive(Debug, Clone)]
pub struct Stream {
    /// Byte offset of the `78 01` zlib header in the source file.
    pub file_offset: usize,
    /// Compressed input bytes the decoder consumed at `file_offset`.
    ///
    /// The physical extent `[file_offset, file_offset + consumed)` in the source.
    pub consumed: u64,
    /// Inflated bytes.
    pub inflated: Vec<u8>,
    /// Payload classification.
    pub kind: StreamKind,
    /// The Parasolid `SCH_<version>` token, when present.
    pub schema: Option<String>,
}

/// One Parasolid type-80 attribute definition joined to its type-79 identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributeDefinition<'a> {
    /// Inflated-stream offset of the `00 50` definition tag.
    pub offset: usize,
    /// Stream-local definition record identity.
    pub xmt: u32,
    /// Stream-local next-definition identity; `1` is null.
    pub next_definition_xmt: u32,
    /// Stream-local type-79 identifier identity.
    pub identifier_xmt: u32,
    /// Inflated-stream offset of the resolved `00 4f` identifier tag.
    pub identifier_offset: usize,
    /// Exact printable class name.
    pub name: &'a str,
    /// Numeric attribute type identifier.
    pub type_id: u32,
    /// Ordered actions for the eight logged event families.
    pub action_codes: [u8; 8],
    /// Stream-local field-name-list identity; `1` is null.
    pub field_names_xmt: u32,
    /// Ordered legal-owner flags.
    pub legal_owner_flags: [u8; 16],
    /// Declared number of fields in the `00 50` record.
    pub field_count: u32,
    /// One serialized field code for every declared field.
    pub field_codes: &'a [u8],
}

/// One framed type-81 Parasolid entity/attribute-list record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity51Record {
    /// Inflated-stream offset of the `00 51` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Record flags preceding the identity.
    pub flags: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Serialized sequence value.
    pub sequence: u32,
    /// Stream-local type-80 attribute-definition identity.
    pub definition_xmt: u32,
    /// Five fixed leading stream-local references.
    pub leading_references: [u32; 5],
    /// Variable trailing stream-local references counted by `flags`.
    pub trailing_references: Vec<u32>,
}

/// One self-framed printable type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity54StringRecord<'a> {
    /// Inflated-stream offset of the `00 54` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Nonempty printable string value.
    pub value: &'a str,
}

/// One counted type-82 unsigned-integer value record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity52IntegerRecord {
    /// Inflated-stream offset of the `00 52` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered big-endian unsigned values.
    pub values: Vec<u32>,
}

/// One counted type-83 binary64 value record.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity53DoubleRecord {
    /// Inflated-stream offset of the `00 53` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered finite big-endian binary64 values.
    pub values: Vec<f64>,
}

/// One counted type-85 point-value record.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity55PointRecord {
    /// Inflated-stream offset of the `00 55` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered finite xyz point values.
    pub values: Vec<[f64; 3]>,
}

/// One counted type-86 vector-value record.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity56VectorRecord {
    /// Inflated-stream offset of the `00 56` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered finite xyz vector values.
    pub values: Vec<[f64; 3]>,
}

/// One counted type-87 axis-value record.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity57AxisRecord {
    /// Inflated-stream offset of the `00 57` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered axes, each serialized as two finite xyz vectors.
    pub values: Vec<[[f64; 3]; 2]>,
}

/// One counted type-88 tag-value record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity58TagRecord {
    /// Inflated-stream offset of the `00 58` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered big-endian tag values.
    pub values: Vec<u32>,
}

/// One counted type-89 direction-value record.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity59DirectionRecord {
    /// Inflated-stream offset of the `00 59` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered finite xyz direction values.
    pub values: Vec<[f64; 3]>,
}

/// One counted type-98 Unicode-value record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity62UnicodeRecord {
    /// Inflated-stream offset of the `00 62` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered big-endian UTF-16 code units.
    pub code_units: Vec<u16>,
    /// Exact Unicode scalar string represented by `code_units`.
    pub value: String,
}

/// One counted type-99 attribute field-name record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldNamesRecord {
    /// Inflated-stream offset of the `00 63` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered stream-local character or Unicode value references.
    pub name_xmts: Vec<u32>,
}

/// Decode counted type-82 unsigned-integer records.
pub fn entity_52_integer_records(bytes: &[u8]) -> Vec<Entity52IntegerRecord> {
    (0..bytes.len())
        .filter_map(|offset| entity_52_integer_record_at(bytes, offset))
        .collect()
}

/// Decode counted type-83 finite binary64 records.
pub fn entity_53_double_records(bytes: &[u8]) -> Vec<Entity53DoubleRecord> {
    (0..bytes.len())
        .filter_map(|offset| entity_53_double_record_at(bytes, offset))
        .collect()
}

/// Decode counted type-85 point-value records.
pub fn entity_55_point_records(bytes: &[u8]) -> Vec<Entity55PointRecord> {
    (0..bytes.len())
        .filter_map(|offset| entity_55_point_record_at(bytes, offset))
        .collect()
}

/// Decode counted type-86 vector-value records.
pub fn entity_56_vector_records(bytes: &[u8]) -> Vec<Entity56VectorRecord> {
    (0..bytes.len())
        .filter_map(|offset| entity_56_vector_record_at(bytes, offset))
        .collect()
}

/// Decode counted type-87 axis-value records.
pub fn entity_57_axis_records(bytes: &[u8]) -> Vec<Entity57AxisRecord> {
    (0..bytes.len())
        .filter_map(|offset| entity_57_axis_record_at(bytes, offset))
        .collect()
}

/// Decode counted type-88 tag-value records.
pub fn entity_58_tag_records(bytes: &[u8]) -> Vec<Entity58TagRecord> {
    (0..bytes.len())
        .filter_map(|offset| entity_58_tag_record_at(bytes, offset))
        .collect()
}

/// Decode counted type-89 direction-value records.
pub fn entity_59_direction_records(bytes: &[u8]) -> Vec<Entity59DirectionRecord> {
    (0..bytes.len())
        .filter_map(|offset| entity_59_direction_record_at(bytes, offset))
        .collect()
}

/// Decode counted type-98 Unicode-value records.
pub fn entity_62_unicode_records(bytes: &[u8]) -> Vec<Entity62UnicodeRecord> {
    (0..bytes.len())
        .filter_map(|offset| entity_62_unicode_record_at(bytes, offset))
        .collect()
}

/// Decode counted type-99 attribute field-name records.
pub fn field_names_records(bytes: &[u8]) -> Vec<FieldNamesRecord> {
    (0..bytes.len())
        .filter_map(|offset| field_names_record_at(bytes, offset))
        .collect()
}

pub(crate) fn field_names_record_at(bytes: &[u8], offset: usize) -> Option<FieldNamesRecord> {
    let mut at = offset.checked_add(2)?;
    (bytes.get(offset..at) == Some(&[0, 0x63])).then_some(())?;
    if bytes.get(at) == Some(&0xff) {
        at += 1;
    }
    let count = usize::try_from(View::u32_be_at(bytes, at)?).ok()?;
    (count > 0).then_some(())?;
    at += 4;
    let xmt = read_xmt(bytes, &mut at).filter(|xmt| *xmt > 1)?;
    (count <= bytes.len().checked_sub(at)? / 2).then_some(())?;
    let name_xmts = (0..count)
        .map(|_| read_xmt(bytes, &mut at).filter(|xmt| *xmt > 1))
        .collect::<Option<Vec<_>>>()?;
    Some(FieldNamesRecord {
        offset,
        byte_len: at - offset,
        xmt,
        name_xmts,
    })
}

/// Decode one complete type-82 unsigned-integer record at `offset`.
pub(crate) fn entity_52_integer_record_at(
    bytes: &[u8],
    offset: usize,
) -> Option<Entity52IntegerRecord> {
    let record =
        counted_value_record_at(bytes, offset, 0x52, 4, |value| View::u32_be_at(value, 0))?;
    Some(Entity52IntegerRecord {
        offset: record.offset,
        byte_len: record.byte_len,
        xmt: record.xmt,
        values: record.values,
    })
}

/// Decode one complete type-83 finite binary64 record at `offset`.
pub(crate) fn entity_53_double_record_at(
    bytes: &[u8],
    offset: usize,
) -> Option<Entity53DoubleRecord> {
    let record = counted_value_record_at(bytes, offset, 0x53, 8, |value| {
        let value = View::f64_be_at(value, 0)?;
        value.is_finite().then_some(value)
    })?;
    Some(Entity53DoubleRecord {
        offset: record.offset,
        byte_len: record.byte_len,
        xmt: record.xmt,
        values: record.values,
    })
}

fn finite_vector(value: &[u8]) -> Option<[f64; 3]> {
    let values = vec3_be_at(value, 0)?;
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some(values)
}

/// Decode one complete type-85 point-value record at `offset`.
pub(crate) fn entity_55_point_record_at(
    bytes: &[u8],
    offset: usize,
) -> Option<Entity55PointRecord> {
    let record = counted_value_record_at(bytes, offset, 0x55, 24, finite_vector)?;
    Some(Entity55PointRecord {
        offset: record.offset,
        byte_len: record.byte_len,
        xmt: record.xmt,
        values: record.values,
    })
}

/// Decode one complete type-86 vector-value record at `offset`.
pub(crate) fn entity_56_vector_record_at(
    bytes: &[u8],
    offset: usize,
) -> Option<Entity56VectorRecord> {
    let record = counted_value_record_at(bytes, offset, 0x56, 24, finite_vector)?;
    Some(Entity56VectorRecord {
        offset: record.offset,
        byte_len: record.byte_len,
        xmt: record.xmt,
        values: record.values,
    })
}

/// Decode one complete type-87 axis-value record at `offset`.
pub(crate) fn entity_57_axis_record_at(bytes: &[u8], offset: usize) -> Option<Entity57AxisRecord> {
    let record = counted_value_record_at(bytes, offset, 0x57, 24, finite_vector)?;
    (record.values.len() % 2 == 0).then_some(())?;
    let values = record
        .values
        .chunks_exact(2)
        .map(|axis| axis.try_into().expect("two vectors per axis"))
        .collect();
    Some(Entity57AxisRecord {
        offset: record.offset,
        byte_len: record.byte_len,
        xmt: record.xmt,
        values,
    })
}

/// Decode one complete type-88 tag-value record at `offset`.
pub(crate) fn entity_58_tag_record_at(bytes: &[u8], offset: usize) -> Option<Entity58TagRecord> {
    let record =
        counted_value_record_at(bytes, offset, 0x58, 4, |value| View::u32_be_at(value, 0))?;
    Some(Entity58TagRecord {
        offset: record.offset,
        byte_len: record.byte_len,
        xmt: record.xmt,
        values: record.values,
    })
}

/// Decode one complete type-89 direction-value record at `offset`.
pub(crate) fn entity_59_direction_record_at(
    bytes: &[u8],
    offset: usize,
) -> Option<Entity59DirectionRecord> {
    let record = counted_value_record_at(bytes, offset, 0x59, 24, finite_vector)?;
    Some(Entity59DirectionRecord {
        offset: record.offset,
        byte_len: record.byte_len,
        xmt: record.xmt,
        values: record.values,
    })
}

/// Decode one complete type-98 Unicode-value record at `offset`.
pub(crate) fn entity_62_unicode_record_at(
    bytes: &[u8],
    offset: usize,
) -> Option<Entity62UnicodeRecord> {
    let record =
        counted_value_record_at(bytes, offset, 0x62, 2, |value| View::u16_be_at(value, 0))?;
    let value = String::from_utf16(&record.values).ok()?;
    Some(Entity62UnicodeRecord {
        offset: record.offset,
        byte_len: record.byte_len,
        xmt: record.xmt,
        code_units: record.values,
        value,
    })
}

struct CountedValueRecord<T> {
    offset: usize,
    byte_len: usize,
    xmt: u32,
    values: Vec<T>,
}

fn counted_value_record_at<T>(
    bytes: &[u8],
    offset: usize,
    tag: u8,
    value_width: usize,
    decode: impl Fn(&[u8]) -> Option<T>,
) -> Option<CountedValueRecord<T>> {
    let mut at = offset.checked_add(2)?;
    (bytes.get(offset..at) == Some(&[0, tag])).then_some(())?;
    if bytes.get(at) == Some(&0xff) {
        at += 1;
    }
    let count = View::u32_be_at(bytes, at)
        .map(|value| value as usize)
        .filter(|count| *count > 0)?;
    at += 4;
    let xmt = read_xmt(bytes, &mut at).filter(|xmt| *xmt > 1)?;
    let values_end = count
        .checked_mul(value_width)
        .and_then(|length| at.checked_add(length))?;
    let values = bytes
        .get(at..values_end)?
        .chunks_exact(value_width)
        .map(decode)
        .collect::<Option<Vec<_>>>()?;
    Some(CountedValueRecord {
        offset,
        byte_len: values_end - offset,
        xmt,
        values,
    })
}

/// Decode self-framed printable type-84 string records.
pub fn entity_54_string_records(bytes: &[u8]) -> Vec<Entity54StringRecord<'_>> {
    (0..bytes.len())
        .filter_map(|offset| entity_54_string_record_at(bytes, offset))
        .collect()
}

/// Decode one complete type-84 printable string record at `offset`.
pub(crate) fn entity_54_string_record_at(
    bytes: &[u8],
    offset: usize,
) -> Option<Entity54StringRecord<'_>> {
    let mut at = offset.checked_add(2)?;
    (bytes.get(offset..at) == Some(&[0x00, 0x54])).then_some(())?;
    if bytes.get(at) == Some(&0xff) {
        at += 1;
    }
    let length = View::u32_be_at(bytes, at)
        .map(|value| value as usize)
        .filter(|length| *length > 0)?;
    at += 4;
    let xmt = read_xmt(bytes, &mut at).filter(|xmt| *xmt > 1)?;
    let end = at.checked_add(length)?;
    let value = bytes.get(at..end).filter(|value| {
        value
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    })?;
    (bytes.get(end) == Some(&0)).then_some(())?;
    let value = std::str::from_utf8(value).ok()?;
    Some(Entity54StringRecord {
        offset,
        byte_len: end.checked_add(1)?.checked_sub(offset)?,
        xmt,
        value,
    })
}

/// Decode framed type-81 entity/attribute-list records.
pub fn entity_51_records(bytes: &[u8]) -> Vec<Entity51Record> {
    (0..bytes.len())
        .filter_map(|offset| entity_51_record_at(bytes, offset))
        .collect()
}

/// Decode one complete type-81 entity/attribute-list record at `offset`.
pub(crate) fn entity_51_record_at(bytes: &[u8], offset: usize) -> Option<Entity51Record> {
    let mut at = offset.checked_add(2)?;
    (bytes.get(offset..at) == Some(&[0x00, 0x51])).then_some(())?;
    if bytes.get(at) == Some(&0xff) {
        at += 1;
    }
    let flags = View::u32_be_at(bytes, at)?;
    at += 4;
    let xmt = read_xmt(bytes, &mut at)?;
    let sequence = View::u32_be_at(bytes, at)?;
    at += 4;
    let definition_xmt = read_xmt(bytes, &mut at)?;
    (xmt > 1 && sequence != 0 && (1..=0x20).contains(&flags)).then_some(())?;
    let reference_count = usize::try_from(flags).ok()?.checked_add(5)?;
    let references = entity_51_references(bytes, &mut at, reference_count)?;
    let leading_references = references.get(..5)?.try_into().ok()?;
    let trailing_references = references.get(5..)?.to_vec();
    Some(Entity51Record {
        offset,
        byte_len: at - offset,
        flags,
        xmt,
        sequence,
        definition_xmt,
        leading_references,
        trailing_references,
    })
}

fn entity_51_references(bytes: &[u8], at: &mut usize, count: usize) -> Option<Vec<u32>> {
    if bytes.get(*at) == Some(&1) {
        let mut prefixed_at = *at;
        let mut references = Vec::new();
        for _ in 0..count {
            matches!(bytes.get(prefixed_at), Some(0 | 1)).then_some(())?;
            prefixed_at += 1;
            references.push(read_xmt(bytes, &mut prefixed_at)?);
        }
        matches!(bytes.get(prefixed_at), Some(0 | 1)).then_some(())?;
        *at = prefixed_at + 1;
        return Some(references);
    }
    (0..count).map(|_| read_xmt(bytes, at)).collect()
}

#[derive(Debug, Clone, Copy)]
struct AttributeIdentifier<'a> {
    offset: usize,
    xmt: u32,
    name: &'a str,
}

fn attribute_identifiers(bytes: &[u8]) -> Vec<AttributeIdentifier<'_>> {
    (0..bytes.len())
        .filter_map(|offset| {
            let mut at = offset.checked_add(2)?;
            (bytes.get(offset..at) == Some(&[0x00, 0x4f])).then_some(())?;
            if bytes.get(at) == Some(&0xff) {
                at += 1;
            }
            let name_len = usize::try_from(View::u32_be_at(bytes, at)?).ok()?;
            at += 4;
            let xmt = read_xmt(bytes, &mut at)?;
            let name_end = at.checked_add(name_len)?;
            let name_bytes = bytes.get(at..name_end)?;
            (!name_bytes.is_empty()
                && name_bytes
                    .iter()
                    .all(|byte| byte.is_ascii() && !byte.is_ascii_control()))
            .then_some(())?;
            Some(AttributeIdentifier {
                offset,
                xmt,
                name: std::str::from_utf8(name_bytes).ok()?,
            })
        })
        .collect()
}

/// Decode complete type-80 attribute definitions and resolve their type-79 identifiers.
pub fn attribute_definitions(bytes: &[u8]) -> Vec<AttributeDefinition<'_>> {
    let identifiers = attribute_identifiers(bytes);
    (0..bytes.len())
        .filter_map(|offset| {
            let mut at = offset.checked_add(2)?;
            (bytes.get(offset..at) == Some(&[0x00, 0x50])).then_some(())?;
            if bytes.get(at) == Some(&0xff) {
                at += 1;
            }
            let field_count = View::u32_be_at(bytes, at)?;
            at += 4;
            let xmt = read_xmt(bytes, &mut at)?;
            let next_definition_xmt = read_xmt(bytes, &mut at)?;
            let identifier_xmt = read_xmt(bytes, &mut at)?;
            let type_id = View::u32_be_at(bytes, at)?;
            at += 4;
            let action_codes: [u8; 8] = bytes.get(at..at + 8)?.try_into().ok()?;
            (xmt > 1
                && identifier_xmt > 1
                && type_id != 0
                && action_codes.iter().all(|action| *action <= 6))
            .then_some(())?;
            at += 8;
            let field_names_xmt = read_xmt(bytes, &mut at)?;
            let legal_owner_flags: [u8; 16] = bytes.get(at..at + 16)?.try_into().ok()?;
            legal_owner_flags
                .iter()
                .all(|flag| matches!(flag, 0 | 1))
                .then_some(())?;
            at += 16;
            let field_codes_end = at.checked_add(usize::try_from(field_count).ok()?)?;
            let field_codes = bytes.get(at..field_codes_end)?;
            field_codes.iter().all(|code| *code <= 10).then_some(())?;
            let mut matches = identifiers
                .iter()
                .filter(|identifier| identifier.xmt == identifier_xmt);
            let identifier = matches.next()?;
            matches.next().is_none().then_some(())?;
            Some(AttributeDefinition {
                offset,
                xmt,
                next_definition_xmt,
                identifier_xmt,
                identifier_offset: identifier.offset,
                name: identifier.name,
                type_id,
                action_codes,
                field_names_xmt,
                legal_owner_flags,
                field_count,
                field_codes,
            })
        })
        .collect()
}

/// The minimum inflated length for an unindexed scan candidate to count as a
/// real stream; indexed wrappers admit any complete member length.
const MIN_INFLATED: usize = 64;

/// Locates, inflates, and classifies zlib streams in `/Root/UG_PART/UG_PART`.
pub fn extract_streams<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
    container: &Container,
) -> Result<Vec<Stream>, CodecError> {
    let Some((part_offset, part_size)) = container
        .entries
        .iter()
        .find(|entry| entry.name == "/Root/UG_PART/UG_PART")
        .and_then(|entry| entry.file_span)
    else {
        return Ok(Vec::new());
    };
    let (Ok(start), Ok(size)) = (usize::try_from(part_offset), usize::try_from(part_size)) else {
        return Ok(Vec::new());
    };
    let Some(end) = start.checked_add(size) else {
        return Ok(Vec::new());
    };
    let part_view = ctx.register_slice(
        root,
        ByteRange {
            start: start as u64,
            end: end as u64,
        },
    )?;
    let part = part_view.window();

    let mut streams = Vec::new();
    if container.segment_index().is_some() {
        let mut seen = BTreeSet::new();
        for wrapper in container.segment_stream_wrappers() {
            let Some(offset) = wrapper.zlib_offset.checked_sub(start) else {
                continue;
            };
            if !seen.insert(offset)
                || part
                    .get(offset..offset.saturating_add(2))
                    .is_none_or(|header| !is_zlib_header(header[0], header[1]))
            {
                continue;
            }
            let Some((inflated, consumed)) = inflate_stream(ctx, part_view, offset, 0)? else {
                return Err(CodecError::Malformed(format!(
                    "invalid indexed zlib member at file offset {}",
                    start + offset
                )));
            };
            let (kind, schema) = classify(&inflated);
            streams.push(Stream {
                file_offset: start + offset,
                consumed,
                inflated,
                kind,
                schema,
            });
        }
        return Ok(streams);
    }

    let mut i = 0usize;
    while i + 2 <= part.len() {
        if is_zlib_header(part[i], part[i + 1]) {
            if let Some((inflated, consumed)) = inflate_stream(ctx, part_view, i, MIN_INFLATED)? {
                let (kind, schema) = classify(&inflated);
                streams.push(Stream {
                    file_offset: start + i,
                    consumed,
                    inflated,
                    kind,
                    schema,
                });
                // Resume past the bytes this member consumed, not at the next
                // byte: a spurious `78 xx` zlib header inside the compressed
                // body would otherwise inflate into a second stream whose source
                // extent [file_offset, file_offset+consumed) overlaps this
                // member's, double-attributing the same compressed bytes to two
                // decompression origins. Skipping the consumed run keeps packed
                // members' input extents disjoint.
                i = i.saturating_add((consumed as usize).max(2));
                continue;
            }
        }
        i += 1;
    }
    Ok(streams)
}

/// Inflates one zlib member that meets [`MIN_INFLATED`].
fn inflate_stream<'a>(
    ctx: &DecodeContext<'a>,
    part_view: View<'a>,
    offset: usize,
    minimum_inflated: usize,
) -> Result<Option<(Vec<u8>, u64)>, CodecError> {
    let Some(source) = part_view.child(offset, part_view.end()) else {
        return Ok(None);
    };
    let (view, consumed) = match inflate_zlib_member(ctx, source, ExpandSpec::Unknown) {
        Ok(result) => result,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    if view.window().len() < minimum_inflated {
        return Ok(None);
    }
    let Ok(consumed) = u64::try_from(consumed) else {
        return Ok(None);
    };
    let inflated = ctx.copy_retained(
        view.window(),
        "retain NX inflated stream",
        Some(source.location()),
    )?;
    Ok(Some((inflated, consumed)))
}

/// A zlib header has compression method 8 and a 16-bit header divisible by
/// 31. NX uses the standard `78 01`, `78 9c`, and `78 da` variants, but the
/// predicate accepts every standards-conforming FLG byte rather than treating a
/// compression level as a format discriminator.
#[allow(clippy::manual_is_multiple_of)] // `is_multiple_of` exceeds the workspace MSRV.
fn is_zlib_header(cmf: u8, flg: u8) -> bool {
    cmf & 0x0f == 8 && cmf >> 4 <= 7 && u16::from_be_bytes([cmf, flg]).is_multiple_of(31)
}

/// Classify an inflated payload from its prologue text and read the schema token.
fn classify(inflated: &[u8]) -> (StreamKind, Option<String>) {
    if !inflated.starts_with(b"PS\x00\x00") {
        return (StreamKind::Preview, None);
    }
    let window = &inflated[..inflated.len().min(512)];
    let kind = if contains(window, b"(partition)") {
        StreamKind::Partition
    } else if contains(window, b"(deltas)") {
        StreamKind::Deltas
    } else {
        StreamKind::Plain
    };
    (kind, read_schema(window))
}

/// Read a `SCH_<...>` schema token: the `SCH_` prefix followed by the run of
/// token characters (alphanumeric and `_`).
fn read_schema(window: &[u8]) -> Option<String> {
    let pos = find(window, b"SCH_")?;
    let mut end = pos;
    while end < window.len() && (window[end].is_ascii_alphanumeric() || window[end] == b'_') {
        end += 1;
    }
    Some(String::from_utf8_lossy(&window[pos..end]).into_owned())
}

#[cfg(test)]
mod tests;
