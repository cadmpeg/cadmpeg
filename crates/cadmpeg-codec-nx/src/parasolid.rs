// SPDX-License-Identifier: Apache-2.0
//! Extract and classify Parasolid streams in an NX part payload.
//!
//! [`extract_streams`] scans the canonical `/Root/UG_PART/UG_PART` file span for
//! valid zlib headers. An inflated `PS 00 00` prologue identifies Parasolid
//! neutral-binary data and supplies its subtype and optional `SCH_` schema token.
//! Legacy CFB parts use [`extract_legacy_streams`] to split the same prologue
//! from clear `UG_PART/UG_PART` bytes. Other inflated payloads are classified
//! as [`StreamKind::Preview`].
#![deny(clippy::disallowed_methods)]

use std::collections::{BTreeMap, BTreeSet};

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
    /// Byte offset of the stream start in the source file.
    ///
    /// Modern streams start at a zlib header. Legacy streams start at a clear
    /// Parasolid transmit header.
    pub file_offset: usize,
    /// Source bytes consumed by the stream at `file_offset`.
    ///
    /// For modern streams this is the compressed member length. For legacy
    /// streams it is the clear section length. The physical extent
    /// `[file_offset, file_offset + consumed)` is source-owned.
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
    /// Number of legal-owner flags serialized by the definition.
    pub legal_owner_flag_count: u8,
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

/// All attribute-value records admitted from one byte view.
///
/// These families share the same byte-level discovery rule: each complete
/// record starts with a two-byte tag. A single pass can dispatch only the
/// matching record parser without changing any family-specific framing or
/// value validation.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct EntityValueRecords<'a> {
    /// Counted type-82 unsigned-integer records.
    pub(crate) integers: Vec<Entity52IntegerRecord>,
    /// Counted type-83 finite binary64 records.
    pub(crate) doubles: Vec<Entity53DoubleRecord>,
    /// Self-framed printable type-84 string records.
    pub(crate) strings: Vec<Entity54StringRecord<'a>>,
    /// Counted type-85 point records.
    pub(crate) points: Vec<Entity55PointRecord>,
    /// Counted type-86 vector records.
    pub(crate) vectors: Vec<Entity56VectorRecord>,
    /// Counted type-87 axis records.
    pub(crate) axes: Vec<Entity57AxisRecord>,
    /// Counted type-88 tag records.
    pub(crate) tags: Vec<Entity58TagRecord>,
    /// Counted type-89 direction records.
    pub(crate) directions: Vec<Entity59DirectionRecord>,
    /// Counted type-98 Unicode records.
    pub(crate) unicode: Vec<Entity62UnicodeRecord>,
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

/// Decode every attribute-value family in one bounded byte pass.
#[cfg(test)]
pub(crate) fn entity_value_records(bytes: &[u8]) -> EntityValueRecords<'_> {
    let mut records = EntityValueRecords::default();
    let mut offset = 0;
    while offset < bytes.len() {
        let Some(frame) = value_record_frame_at(bytes, offset) else {
            offset += 1;
            continue;
        };
        if append_value_record(frame, &mut records).is_some() {
            offset = frame.next_offset();
        } else {
            // The frame has already passed its family-specific validation. If
            // materialization ever disagrees, preserve the old recovery rule
            // and keep looking for a later record instead of owning a partial
            // candidate.
            offset += 1;
        }
    }
    records
}

/// Decode value records at offsets owned by an enclosing record ledger.
pub(crate) fn entity_value_records_at(
    bytes: &[u8],
    offsets: impl IntoIterator<Item = usize>,
) -> EntityValueRecords<'_> {
    let mut records = EntityValueRecords::default();
    for offset in offsets {
        if let Some(frame) = value_record_frame_at(bytes, offset) {
            let _ = append_value_record(frame, &mut records);
        }
    }
    records
}

/// Locate unique snapshot values owned by typed attribute relations.
pub(crate) fn referenced_value_record_offsets(bytes: &[u8]) -> Vec<usize> {
    let referenced_xmts = referenced_value_xmts(bytes, ValueMultiplicity::UniqueSnapshot);
    value_record_candidates(bytes)
        .into_iter()
        .filter_map(|(xmt, offsets)| {
            let [offset] = offsets.as_slice() else {
                return None;
            };
            referenced_xmts.contains(&xmt).then_some(*offset)
        })
        .collect()
}

/// Locate historical value events owned by typed attribute relations.
pub(crate) fn referenced_value_event_offsets(bytes: &[u8]) -> Vec<usize> {
    let referenced_xmts = referenced_value_xmts(bytes, ValueMultiplicity::HistoricalEvents);
    value_record_candidates(bytes)
        .into_iter()
        .filter(|(xmt, _)| referenced_xmts.contains(xmt))
        .flat_map(|(_, offsets)| offsets)
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueMultiplicity {
    UniqueSnapshot,
    HistoricalEvents,
}

fn referenced_value_xmts(bytes: &[u8], multiplicity: ValueMultiplicity) -> BTreeSet<u32> {
    let mut referenced = BTreeSet::new();
    let mut entities = BTreeMap::<u32, Vec<Entity51Record>>::new();
    for record in entity_51_records(bytes) {
        entities.entry(record.xmt).or_default().push(record);
    }
    for records in entities.into_values() {
        if multiplicity == ValueMultiplicity::UniqueSnapshot && records.len() != 1 {
            continue;
        }
        for record in records {
            referenced.extend(record.leading_references);
            referenced.extend(record.trailing_references);
        }
    }

    let mut field_name_lists = BTreeMap::<u32, Vec<FieldNamesRecord>>::new();
    for record in field_names_records(bytes) {
        field_name_lists.entry(record.xmt).or_default().push(record);
    }
    let mut definitions = BTreeMap::<u32, Vec<AttributeDefinition<'_>>>::new();
    for definition in attribute_definitions(bytes) {
        definitions
            .entry(definition.xmt)
            .or_default()
            .push(definition);
    }
    let mut referenced_lists = BTreeSet::new();
    for records in definitions.into_values() {
        if multiplicity == ValueMultiplicity::UniqueSnapshot && records.len() != 1 {
            continue;
        }
        referenced_lists.extend(
            records.into_iter().filter_map(|record| {
                (record.field_names_xmt > 1).then_some(record.field_names_xmt)
            }),
        );
    }
    for (xmt, records) in field_name_lists {
        if !referenced_lists.contains(&xmt)
            || (multiplicity == ValueMultiplicity::UniqueSnapshot && records.len() != 1)
        {
            continue;
        }
        for record in records {
            referenced.extend(record.name_xmts);
        }
    }
    referenced
}

fn value_record_candidates(bytes: &[u8]) -> BTreeMap<u32, Vec<usize>> {
    let mut candidates = BTreeMap::<u32, Vec<usize>>::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let Some(frame) = value_record_frame_at(bytes, offset) else {
            offset += 1;
            continue;
        };
        candidates.entry(frame.xmt()).or_default().push(offset);
        offset = frame.next_offset();
    }
    candidates
}

/// Return `(kind, xmt, byte_len)` for one complete value record.
pub(crate) fn entity_value_record_identity_at(
    bytes: &[u8],
    offset: usize,
) -> Option<(u16, u32, usize)> {
    let frame = value_record_frame_at(bytes, offset)?;
    Some((
        u16::from(frame.tag()),
        frame.xmt(),
        frame.end().checked_sub(offset)?,
    ))
}

#[derive(Clone, Copy)]
enum ValueRecordFrame<'a> {
    Counted {
        tag: u8,
        offset: usize,
        end: usize,
        xmt: u32,
        value_width: usize,
        raw: &'a [u8],
    },
    String {
        offset: usize,
        end: usize,
        xmt: u32,
        value: &'a str,
    },
}

impl ValueRecordFrame<'_> {
    fn tag(self) -> u8 {
        match self {
            Self::Counted { tag, .. } => tag,
            Self::String { .. } => 0x54,
        }
    }

    fn xmt(self) -> u32 {
        match self {
            Self::Counted { xmt, .. } | Self::String { xmt, .. } => xmt,
        }
    }

    fn end(self) -> usize {
        match self {
            Self::Counted { end, .. } | Self::String { end, .. } => end,
        }
    }

    fn next_offset(self) -> usize {
        match self {
            Self::Counted { end, .. } => end,
            // Type-84's terminator can be the leading zero of the following
            // two-byte record tag. Keep that shared byte available to the
            // sequential scanner.
            Self::String { end, .. } => end.saturating_sub(1),
        }
    }
}

fn value_record_frame_at(bytes: &[u8], offset: usize) -> Option<ValueRecordFrame<'_>> {
    let tag = *bytes.get(offset.checked_add(1)?)?;
    match tag {
        0x52 | 0x58 => {
            let frame = counted_value_frame_at(bytes, offset, tag, 4)?;
            Some(ValueRecordFrame::Counted {
                tag,
                offset: frame.offset,
                end: frame.end,
                xmt: frame.xmt,
                value_width: frame.value_width,
                raw: frame.raw,
            })
        }
        0x53 => {
            let frame = counted_value_frame_at(bytes, offset, tag, 8)?;
            finite_scalar_lane(frame.raw)?;
            Some(ValueRecordFrame::Counted {
                tag,
                offset: frame.offset,
                end: frame.end,
                xmt: frame.xmt,
                value_width: frame.value_width,
                raw: frame.raw,
            })
        }
        0x54 => {
            let frame = string_value_frame_at(bytes, offset)?;
            Some(ValueRecordFrame::String {
                offset: frame.offset,
                end: frame.end,
                xmt: frame.xmt,
                value: frame.value,
            })
        }
        0x55 | 0x56 | 0x59 => {
            let frame = counted_value_frame_at(bytes, offset, tag, 24)?;
            finite_vector_lane(frame.raw)?;
            Some(ValueRecordFrame::Counted {
                tag,
                offset: frame.offset,
                end: frame.end,
                xmt: frame.xmt,
                value_width: frame.value_width,
                raw: frame.raw,
            })
        }
        0x57 => {
            let frame = counted_value_frame_at(bytes, offset, tag, 24)?;
            frame.count.is_multiple_of(2).then_some(())?;
            finite_vector_lane(frame.raw)?;
            Some(ValueRecordFrame::Counted {
                tag,
                offset: frame.offset,
                end: frame.end,
                xmt: frame.xmt,
                value_width: frame.value_width,
                raw: frame.raw,
            })
        }
        0x62 => {
            let frame = counted_value_frame_at(bytes, offset, tag, 2)?;
            valid_utf16_lane(frame.raw)?;
            Some(ValueRecordFrame::Counted {
                tag,
                offset: frame.offset,
                end: frame.end,
                xmt: frame.xmt,
                value_width: frame.value_width,
                raw: frame.raw,
            })
        }
        _ => None,
    }
}

fn append_value_record<'a>(
    frame: ValueRecordFrame<'a>,
    records: &mut EntityValueRecords<'a>,
) -> Option<()> {
    match frame {
        ValueRecordFrame::String {
            offset,
            end,
            xmt,
            value,
        } => records.strings.push(Entity54StringRecord {
            offset,
            byte_len: end.checked_sub(offset)?,
            xmt,
            value,
        }),
        ValueRecordFrame::Counted {
            tag,
            offset,
            end,
            xmt,
            value_width,
            raw,
        } => match tag {
            0x52 => records.integers.push(Entity52IntegerRecord {
                offset,
                byte_len: end.checked_sub(offset)?,
                xmt,
                values: materialize_values(raw, value_width, |value| View::u32_be_at(value, 0))?,
            }),
            0x53 => records.doubles.push(Entity53DoubleRecord {
                offset,
                byte_len: end.checked_sub(offset)?,
                xmt,
                values: materialize_values(raw, value_width, |value| View::f64_be_at(value, 0))?,
            }),
            0x55 => records.points.push(Entity55PointRecord {
                offset,
                byte_len: end.checked_sub(offset)?,
                xmt,
                values: materialize_values(raw, value_width, finite_vector)?,
            }),
            0x56 => records.vectors.push(Entity56VectorRecord {
                offset,
                byte_len: end.checked_sub(offset)?,
                xmt,
                values: materialize_values(raw, value_width, finite_vector)?,
            }),
            0x57 => {
                let vectors = materialize_values(raw, value_width, finite_vector)?;
                records.axes.push(Entity57AxisRecord {
                    offset,
                    byte_len: end.checked_sub(offset)?,
                    xmt,
                    values: vectors
                        .chunks_exact(2)
                        .map(|axis| [axis[0], axis[1]])
                        .collect(),
                });
            }
            0x58 => records.tags.push(Entity58TagRecord {
                offset,
                byte_len: end.checked_sub(offset)?,
                xmt,
                values: materialize_values(raw, value_width, |value| View::u32_be_at(value, 0))?,
            }),
            0x59 => records.directions.push(Entity59DirectionRecord {
                offset,
                byte_len: end.checked_sub(offset)?,
                xmt,
                values: materialize_values(raw, value_width, finite_vector)?,
            }),
            0x62 => {
                let code_units =
                    materialize_values(raw, value_width, |value| View::u16_be_at(value, 0))?;
                records.unicode.push(Entity62UnicodeRecord {
                    offset,
                    byte_len: end.checked_sub(offset)?,
                    xmt,
                    value: String::from_utf16(&code_units).ok()?,
                    code_units,
                });
            }
            _ => return None,
        },
    }
    Some(())
}

fn materialize_values<T>(
    raw: &[u8],
    value_width: usize,
    decode: impl Fn(&[u8]) -> Option<T>,
) -> Option<Vec<T>> {
    raw.chunks_exact(value_width).map(decode).collect()
}

/// Decode counted type-99 attribute field-name records.
pub fn field_names_records(bytes: &[u8]) -> Vec<FieldNamesRecord> {
    let mut records = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let Some(frame) = field_names_frame_at(bytes, offset) else {
            offset += 1;
            continue;
        };
        if let Some(record) = field_names_record_from_frame(bytes, frame) {
            records.push(record);
            offset = frame.end;
        } else {
            offset += 1;
        }
    }
    records
}

#[cfg(test)]
pub(crate) fn field_names_record_at(bytes: &[u8], offset: usize) -> Option<FieldNamesRecord> {
    let frame = field_names_frame_at(bytes, offset)?;
    field_names_record_from_frame(bytes, frame)
}

#[derive(Clone, Copy)]
struct FieldNamesFrame {
    offset: usize,
    end: usize,
    xmt: u32,
    count: usize,
    names_at: usize,
}

fn field_names_frame_at(bytes: &[u8], offset: usize) -> Option<FieldNamesFrame> {
    let mut at = offset.checked_add(2)?;
    (bytes.get(offset..at) == Some(&[0, 0x63])).then_some(())?;
    if bytes.get(at) == Some(&0xff) {
        at += 1;
    }
    let count = usize::try_from(View::u32_be_at(bytes, at)?).ok()?;
    (count > 0).then_some(())?;
    at += 4;
    let xmt = read_xmt(bytes, &mut at).filter(|xmt| *xmt > 1)?;
    let names_at = at;
    for _ in 0..count {
        read_xmt(bytes, &mut at).filter(|xmt| *xmt > 1)?;
    }
    Some(FieldNamesFrame {
        offset,
        end: at,
        xmt,
        count,
        names_at,
    })
}

fn field_names_record_from_frame(bytes: &[u8], frame: FieldNamesFrame) -> Option<FieldNamesRecord> {
    let mut at = frame.names_at;
    let name_xmts = (0..frame.count)
        .map(|_| read_xmt(bytes, &mut at).filter(|xmt| *xmt > 1))
        .collect::<Option<Vec<_>>>()?;
    Some(FieldNamesRecord {
        offset: frame.offset,
        byte_len: frame.end - frame.offset,
        xmt: frame.xmt,
        name_xmts,
    })
}

/// Decode one complete type-82 unsigned-integer record at `offset`.
#[cfg(test)]
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
#[cfg(test)]
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

fn finite_scalar_lane(raw: &[u8]) -> Option<()> {
    for value in raw.chunks_exact(8) {
        View::f64_be_at(value, 0)?.is_finite().then_some(())?;
    }
    Some(())
}

fn finite_vector_lane(raw: &[u8]) -> Option<()> {
    for value in raw.chunks_exact(24) {
        finite_vector(value)?;
    }
    Some(())
}

fn valid_utf16_lane(raw: &[u8]) -> Option<()> {
    let mut high_surrogate = false;
    for value in raw.chunks_exact(2) {
        let unit = View::u16_be_at(value, 0)?;
        if high_surrogate {
            (0xdc00..=0xdfff).contains(&unit).then_some(())?;
            high_surrogate = false;
        } else if (0xd800..=0xdbff).contains(&unit) {
            high_surrogate = true;
        } else {
            (!(0xdc00..=0xdfff).contains(&unit)).then_some(())?;
        }
    }
    (!high_surrogate).then_some(())
}

#[cfg(test)]
struct CountedValueRecord<T> {
    offset: usize,
    byte_len: usize,
    xmt: u32,
    values: Vec<T>,
}

#[derive(Clone, Copy)]
struct CountedValueFrame<'a> {
    offset: usize,
    end: usize,
    xmt: u32,
    count: usize,
    value_width: usize,
    raw: &'a [u8],
}

fn counted_value_frame_at(
    bytes: &[u8],
    offset: usize,
    tag: u8,
    value_width: usize,
) -> Option<CountedValueFrame<'_>> {
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
    let raw = bytes.get(at..values_end)?;
    Some(CountedValueFrame {
        offset,
        end: values_end,
        xmt,
        count,
        value_width,
        raw,
    })
}

#[cfg(test)]
fn counted_value_record_at<T>(
    bytes: &[u8],
    offset: usize,
    tag: u8,
    value_width: usize,
    decode: impl Fn(&[u8]) -> Option<T>,
) -> Option<CountedValueRecord<T>> {
    let frame = counted_value_frame_at(bytes, offset, tag, value_width)?;
    let values = frame
        .raw
        .chunks_exact(value_width)
        .map(decode)
        .collect::<Option<Vec<_>>>()?;
    Some(CountedValueRecord {
        offset: frame.offset,
        byte_len: frame.end - frame.offset,
        xmt: frame.xmt,
        values,
    })
}

/// Decode one complete type-84 printable string record at `offset`.
#[cfg(test)]
pub(crate) fn entity_54_string_record_at(
    bytes: &[u8],
    offset: usize,
) -> Option<Entity54StringRecord<'_>> {
    let frame = string_value_frame_at(bytes, offset)?;
    Some(Entity54StringRecord {
        offset: frame.offset,
        byte_len: frame.end - frame.offset,
        xmt: frame.xmt,
        value: frame.value,
    })
}

#[derive(Clone, Copy)]
struct StringValueFrame<'a> {
    offset: usize,
    end: usize,
    xmt: u32,
    value: &'a str,
}

fn string_value_frame_at(bytes: &[u8], offset: usize) -> Option<StringValueFrame<'_>> {
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
    Some(StringValueFrame {
        offset,
        end: end.checked_add(1)?,
        xmt,
        value,
    })
}

/// Decode framed type-81 entity/attribute-list records.
pub fn entity_51_records(bytes: &[u8]) -> Vec<Entity51Record> {
    let mut records = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let Some(frame) = entity_51_frame_at(bytes, offset) else {
            offset += 1;
            continue;
        };
        if let Some(record) = entity_51_record_from_frame(bytes, frame) {
            records.push(record);
            offset = frame.next_offset();
        } else {
            offset += 1;
        }
    }
    records
}

/// Decode one complete type-81 entity/attribute-list record at `offset`.
pub(crate) fn entity_51_record_at(bytes: &[u8], offset: usize) -> Option<Entity51Record> {
    let frame = entity_51_frame_at(bytes, offset)?;
    entity_51_record_from_frame(bytes, frame)
}

#[derive(Clone, Copy)]
struct Entity51Frame {
    offset: usize,
    end: usize,
    flags: u32,
    xmt: u32,
    sequence: u32,
    definition_xmt: u32,
    references_at: usize,
    reference_count: usize,
    shared_terminal: bool,
}

impl Entity51Frame {
    fn next_offset(self) -> usize {
        if self.shared_terminal {
            self.end.saturating_sub(1)
        } else {
            self.end
        }
    }
}

fn entity_51_frame_at(bytes: &[u8], offset: usize) -> Option<Entity51Frame> {
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
    let references_at = at;
    let (end, shared_terminal) = entity_51_reference_end(bytes, &mut at, reference_count)?;
    Some(Entity51Frame {
        offset,
        end,
        flags,
        xmt,
        sequence,
        definition_xmt,
        references_at,
        reference_count,
        shared_terminal,
    })
}

fn entity_51_record_from_frame(bytes: &[u8], frame: Entity51Frame) -> Option<Entity51Record> {
    let mut at = frame.references_at;
    let references = entity_51_references(bytes, &mut at, frame.reference_count)?;
    let leading_references = references.get(..5)?.try_into().ok()?;
    let trailing_references = references.get(5..)?.to_vec();
    Some(Entity51Record {
        offset: frame.offset,
        byte_len: frame.end - frame.offset,
        flags: frame.flags,
        xmt: frame.xmt,
        sequence: frame.sequence,
        definition_xmt: frame.definition_xmt,
        leading_references,
        trailing_references,
    })
}

fn entity_51_reference_end(bytes: &[u8], at: &mut usize, count: usize) -> Option<(usize, bool)> {
    if bytes.get(*at) == Some(&1) {
        let mut prefixed_at = *at;
        for _ in 0..count {
            matches!(bytes.get(prefixed_at), Some(0 | 1)).then_some(())?;
            prefixed_at += 1;
            read_xmt(bytes, &mut prefixed_at)?;
        }
        matches!(bytes.get(prefixed_at), Some(0 | 1)).then_some(())?;
        *at = prefixed_at + 1;
        return Some((*at, true));
    }
    for _ in 0..count {
        read_xmt(bytes, at)?;
    }
    Some((*at, false))
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
            let field_count_usize = usize::try_from(field_count).ok()?;
            let (legal_owner_flags, legal_owner_flag_count, field_codes) =
                [16_usize, 14].into_iter().find_map(|flag_count| {
                    let flags = bytes.get(at..at.checked_add(flag_count)?)?;
                    flags
                        .iter()
                        .all(|flag| matches!(flag, 0 | 1))
                        .then_some(())?;
                    let field_codes_start = at.checked_add(flag_count)?;
                    let field_codes_end = field_codes_start.checked_add(field_count_usize)?;
                    let field_codes = bytes.get(field_codes_start..field_codes_end)?;
                    field_codes.iter().all(|code| *code <= 10).then_some(())?;
                    if flag_count == 14 && !attribute_definition_boundary(bytes, field_codes_end) {
                        return None;
                    }
                    let mut legal_owner_flags = [0; 16];
                    legal_owner_flags[..flag_count].copy_from_slice(flags);
                    Some((
                        legal_owner_flags,
                        u8::try_from(flag_count).ok()?,
                        field_codes,
                    ))
                })?;
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
                legal_owner_flag_count,
                field_count,
                field_codes,
            })
        })
        .collect()
}

fn attribute_definition_boundary(bytes: &[u8], offset: usize) -> bool {
    bytes
        .get(offset..offset.saturating_add(2))
        .is_some_and(|tag| tag[0] == 0 && (0x4f..=0x63).contains(&tag[1]))
}

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
            let Some((inflated, consumed)) = inflate_stream(ctx, part_view, offset)? else {
                return Err(CodecError::malformed(format_args!(
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
        if streams.iter().any(|stream| stream.kind.is_parasolid()) {
            return Ok(streams);
        }
        append_unindexed_structural_streams(ctx, part_view, start, &mut streams)?;
        return Ok(streams);
    }

    append_all_zlib_streams(ctx, part_view, start, &mut streams, false)?;
    Ok(streams)
}

fn append_all_zlib_streams<'a>(
    ctx: &DecodeContext<'a>,
    part_view: View<'a>,
    file_start: usize,
    streams: &mut Vec<Stream>,
    structural_only: bool,
) -> Result<(), CodecError> {
    let part = part_view.window();
    let mut seen = streams
        .iter()
        .map(|stream| stream.file_offset)
        .collect::<BTreeSet<_>>();
    let mut i = 0usize;
    while i + 2 <= part.len() {
        if is_zlib_header(part[i], part[i + 1]) {
            if let Some((inflated, consumed)) = inflate_stream(ctx, part_view, i)? {
                let (kind, schema) = classify(&inflated);
                let file_offset = file_start + i;
                if seen.insert(file_offset)
                    && (!structural_only || structural_stream_candidate(kind, &inflated))
                {
                    streams.push(Stream {
                        file_offset,
                        consumed,
                        inflated,
                        kind,
                        schema,
                    });
                }
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
    Ok(())
}

fn append_unindexed_structural_streams<'a>(
    ctx: &DecodeContext<'a>,
    part_view: View<'a>,
    file_start: usize,
    streams: &mut Vec<Stream>,
) -> Result<(), CodecError> {
    append_all_zlib_streams(ctx, part_view, file_start, streams, true)
}

fn structural_stream_candidate(kind: StreamKind, inflated: &[u8]) -> bool {
    if !kind.is_parasolid() {
        return false;
    }
    let census = crate::deltas::walk(inflated);
    if !census.records.is_empty() || !census.tombstones.is_empty() {
        return true;
    }
    if kind == StreamKind::Deltas {
        return false;
    }
    let graph = crate::topology::Graph::parse(inflated);
    (12..=19).any(|topology_kind| graph.of_kind(topology_kind).next().is_some())
}

/// Locate clear Parasolid transmit sections in a legacy `UG_PART/UG_PART`
/// stream.
///
/// Legacy NX stores multiple self-describing Parasolid sections consecutively.
/// A section begins with `PS`, its big-endian description length, and a
/// printable `TRANSMIT FILE` description. Only those complete transmit headers
/// are admitted as boundaries; arbitrary `PS\0\0` bytes in the payload do not
/// split a stream.
pub fn extract_legacy_streams<'a>(
    ctx: &DecodeContext<'a>,
    part: View<'a>,
) -> Result<Vec<Stream>, CodecError> {
    let bytes = part.window();
    let mut streams = Vec::new();
    let mut search = 0;
    while let Some(start) = legacy_stream_start(bytes, search) {
        let next = legacy_stream_start(bytes, start.saturating_add(4));
        let end = next.unwrap_or(bytes.len());
        let payload = bytes.get(start..end).ok_or_else(|| {
            CodecError::Malformed("legacy Parasolid stream range escapes payload".into())
        })?;
        let inflated = ctx.copy_retained(
            payload,
            "retain legacy NX Parasolid stream",
            Some(part.location()),
        )?;
        let (kind, schema) = classify(&inflated);
        let consumed = u64::try_from(payload.len()).map_err(|_| {
            CodecError::Malformed("legacy Parasolid stream length exceeds u64".into())
        })?;
        let file_offset = part.start().checked_add(start).ok_or_else(|| {
            CodecError::Malformed("legacy Parasolid stream offset overflow".into())
        })?;
        streams.push(Stream {
            file_offset,
            consumed,
            inflated,
            kind,
            schema,
        });
        let Some(next) = next else {
            break;
        };
        search = next;
    }
    Ok(streams)
}

fn legacy_stream_start(bytes: &[u8], mut search: usize) -> Option<usize> {
    while let Some(relative) = find(bytes.get(search..).unwrap_or_default(), b"PS\x00\x00") {
        let start = search.checked_add(relative)?;
        if legacy_transmit_header(bytes, start) {
            return Some(start);
        }
        search = start.saturating_add(4);
    }
    None
}

fn legacy_transmit_header(bytes: &[u8], start: usize) -> bool {
    let Some(description_len) = View::u32_be_at(bytes, start.saturating_add(2)) else {
        return false;
    };
    let Ok(description_len) = usize::try_from(description_len) else {
        return false;
    };
    let Some(description_start) = start.checked_add(6) else {
        return false;
    };
    let Some(description_end) = description_start.checked_add(description_len) else {
        return false;
    };
    let Some(description) = bytes.get(description_start..description_end) else {
        return false;
    };
    description
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        && contains(description, b"TRANSMIT FILE")
}

/// Inflate one complete zlib member.
fn inflate_stream<'a>(
    ctx: &DecodeContext<'a>,
    part_view: View<'a>,
    offset: usize,
) -> Result<Option<(Vec<u8>, u64)>, CodecError> {
    let Some(source) = part_view.child(offset, part_view.end()) else {
        return Ok(None);
    };
    let (view, consumed) = match inflate_zlib_member(ctx, source, ExpandSpec::Unknown) {
        Ok(result) => result,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
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
    cmf & 0x0f == 8 && cmf >> 4 <= 7 && ((u16::from(cmf) << 8) | u16::from(flg)).is_multiple_of(31)
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
    (
        kind,
        cadmpeg_parasolid::find_schema_token(window).map(|token| token.value().to_owned()),
    )
}

#[cfg(test)]
mod tests;
