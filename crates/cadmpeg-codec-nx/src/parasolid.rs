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
use std::num::NonZeroU32;

use cadmpeg_container::compression::inflate_zlib_member;
use cadmpeg_core::bytes::{contains, find};
use cadmpeg_core::decode::{ByteRange, DecodeContext, ExpandSpec, View};
use cadmpeg_core::CodecError;

pub(crate) mod attribute_action;
use attribute_action::AttributeAction;

pub(crate) mod attribute_field;
use attribute_field::AttributeField;

pub(crate) mod value_records;

pub(crate) mod unicode_value;

pub(crate) mod counted_values;

use crate::printable_string::PrintableString;

pub(crate) mod entity_references;
pub(crate) mod name_references;
use entity_references::EntityReferences;
use name_references::NameReferences;

use crate::container::Container;
use crate::framing::node_kind::NodeKind;
use crate::framing::read_and_advance as read_xmt;
use crate::framing::xmt_reference::{NonNullXmt, XmtTarget};

/// Classification of an inflated payload in the part stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
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
}

/// Parasolid stream record subtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParasolidSubtype {
    /// Body snapshot.
    Partition,
    /// Edit overlay.
    Deltas,
    /// Body without a named subtype.
    Plain,
}

impl ParasolidSubtype {
    pub(crate) fn chart_point_layout(self) -> crate::intersection::ChartPointLayout {
        match self {
            Self::Partition | Self::Plain => crate::intersection::ChartPointLayout::Xyz3,
            Self::Deltas => crate::intersection::ChartPointLayout::Ext11,
        }
    }
}

/// Classified stream body and its Parasolid schema.
#[derive(Debug, Clone)]
pub enum StreamBody {
    /// Parasolid records.
    Parasolid {
        /// Record subtype.
        subtype: ParasolidSubtype,
        /// Schema token.
        schema: Option<cadmpeg_parasolid::OwnedSchemaToken>,
    },
    /// Non-Parasolid payload.
    Preview,
}

impl StreamBody {
    fn kind(&self) -> StreamKind {
        match self {
            Self::Parasolid {
                subtype: ParasolidSubtype::Partition,
                ..
            } => StreamKind::Partition,
            Self::Parasolid {
                subtype: ParasolidSubtype::Deltas,
                ..
            } => StreamKind::Deltas,
            Self::Parasolid {
                subtype: ParasolidSubtype::Plain,
                ..
            } => StreamKind::Plain,
            Self::Preview => StreamKind::Preview,
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
    pub body: StreamBody,
}

impl Stream {
    /// Stream classification.
    pub fn kind(&self) -> StreamKind {
        self.body.kind()
    }

    /// Parasolid schema token.
    pub fn schema_token(&self) -> Option<&cadmpeg_parasolid::OwnedSchemaToken> {
        match &self.body {
            StreamBody::Parasolid { schema, .. } => schema.as_ref(),
            StreamBody::Preview => None,
        }
    }
}

/// Owner-flag layouts admitted by the attribute-definition grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegalOwnerFlags {
    /// Fourteen flags in the compact definition layout.
    Fourteen([bool; 14]),
    /// Sixteen flags in the extended definition layout.
    Sixteen([bool; 16]),
}

impl LegalOwnerFlags {
    pub fn as_slice(&self) -> &[bool] {
        match self {
            Self::Fourteen(flags) => flags,
            Self::Sixteen(flags) => flags,
        }
    }

    /// Fixed-width native JSON representation, padded after the source flags.
    pub fn padded(self) -> [u8; 16] {
        let mut padded = [0; 16];
        for (byte, flag) in padded.iter_mut().zip(self.as_slice()) {
            *byte = u8::from(*flag);
        }
        padded
    }
}

impl TryFrom<&[u8]> for LegalOwnerFlags {
    type Error = &'static str;
    fn try_from(flags: &[u8]) -> Result<Self, Self::Error> {
        if !flags.iter().all(|flag| matches!(flag, 0 | 1)) {
            return Err("legal_owner_flags must be binary");
        }
        if let Ok(flags) = <[u8; 16]>::try_from(flags) {
            Ok(Self::Sixteen(flags.map(|flag| flag == 1)))
        } else if let Ok(flags) = <[u8; 14]>::try_from(flags) {
            Ok(Self::Fourteen(flags.map(|flag| flag == 1)))
        } else {
            Err("attribute owner flags require fourteen or sixteen bytes")
        }
    }
}

/// One Parasolid type-80 attribute definition joined to its type-79 identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeDefinition<'a> {
    /// Inflated-stream offset of the `00 50` definition tag.
    pub offset: usize,
    /// Stream-local definition record identity.
    pub xmt: NonNullXmt,
    /// Optional stream-local next-definition target.
    pub next_definition_xmt: Option<XmtTarget>,
    /// Stream-local type-79 identifier identity.
    pub identifier_xmt: NonNullXmt,
    /// Inflated-stream offset of the resolved `00 4f` identifier tag.
    pub identifier_offset: usize,
    /// Exact printable class name.
    pub name: PrintableString<&'a str>,
    /// Numeric attribute type identifier.
    pub type_id: NonZeroU32,
    /// Ordered actions for the eight logged event families.
    pub action_codes: [AttributeAction; 8],
    /// Optional stream-local field-name-list target.
    pub field_names_xmt: Option<XmtTarget>,
    /// Ordered legal-owner flags.
    pub legal_owner_flags: LegalOwnerFlags,
    /// One serialized field code for every declared field.
    pub field_codes: Vec<AttributeField>,
}

/// One framed type-81 Parasolid entity/attribute-list record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity51Record {
    /// Inflated-stream offset of the `00 51` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: NonNullXmt,
    /// Serialized sequence value.
    pub sequence: NonZeroU32,
    /// Stream-local type-80 attribute-definition identity.
    pub definition_xmt: u32,
    /// Five fixed leading stream-local references.
    pub leading_references: [u32; 5],
    /// Variable trailing stream-local references counted by `flags`.
    pub trailing_references: EntityReferences,
}

/// One counted type-99 attribute field-name record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldNamesRecord {
    /// Inflated-stream offset of the `00 63` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: NonNullXmt,
    /// Ordered stream-local character or Unicode value references.
    pub name_xmts: NameReferences,
}

/// Locate unique snapshot values owned by typed attribute relations.
pub(crate) fn referenced_value_record_offsets(bytes: &[u8]) -> Vec<usize> {
    let referenced_xmts = referenced_value_xmts(bytes, ValueMultiplicity::UniqueSnapshot);
    value_records::value_record_candidates(bytes)
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
    value_records::value_record_candidates(bytes)
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
        entities
            .entry(u32::from(record.xmt))
            .or_default()
            .push(record);
    }
    for records in entities.into_values() {
        if multiplicity == ValueMultiplicity::UniqueSnapshot && records.len() != 1 {
            continue;
        }
        for record in records {
            referenced.extend(record.leading_references);
            referenced.extend(record.trailing_references.into_values());
        }
    }

    let mut field_name_lists = BTreeMap::<u32, Vec<FieldNamesRecord>>::new();
    for record in field_names_records(bytes) {
        field_name_lists
            .entry(u32::from(record.xmt))
            .or_default()
            .push(record);
    }
    let mut definitions = BTreeMap::<u32, Vec<AttributeDefinition<'_>>>::new();
    for definition in attribute_definitions(bytes) {
        definitions
            .entry(u32::from(definition.xmt))
            .or_default()
            .push(definition);
    }
    let mut referenced_lists = BTreeSet::new();
    for records in definitions.into_values() {
        if multiplicity == ValueMultiplicity::UniqueSnapshot && records.len() != 1 {
            continue;
        }
        referenced_lists.extend(
            records
                .into_iter()
                .filter_map(|record| record.field_names_xmt.map(u32::from)),
        );
    }
    for (xmt, records) in field_name_lists {
        if !referenced_lists.contains(&xmt)
            || (multiplicity == ValueMultiplicity::UniqueSnapshot && records.len() != 1)
        {
            continue;
        }
        for record in records {
            referenced.extend(record.name_xmts.as_slice().iter().copied().map(u32::from));
        }
    }
    referenced
}

/// Decode counted type-99 attribute field-name records.
pub fn field_names_records(bytes: &[u8]) -> Vec<FieldNamesRecord> {
    let mut records = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        if let Some(record) = field_names_record_at(bytes, offset) {
            offset += record.byte_len;
            records.push(record);
        } else {
            offset += 1;
        }
    }
    records
}

pub(crate) fn field_names_record_at(bytes: &[u8], offset: usize) -> Option<FieldNamesRecord> {
    let mut at = offset.checked_add(2)?;
    (bytes.get(offset..at) == Some(&[0, 0x63])).then_some(())?;
    if bytes.get(at) == Some(&0xff) {
        at += 1;
    }
    let count = usize::try_from(View::u32_be_at(bytes, at)?).ok()?;
    at += 4;
    let xmt = NonNullXmt::try_from(read_xmt(bytes, &mut at)?).ok()?;
    let name_xmts = (0..count)
        .map(|_| NonNullXmt::try_from(read_xmt(bytes, &mut at)?).ok())
        .collect::<Option<Vec<_>>>()?;
    Some(FieldNamesRecord {
        offset,
        byte_len: at - offset,
        xmt,
        name_xmts: NameReferences::try_from(name_xmts).ok()?,
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
    xmt: NonNullXmt,
    sequence: NonZeroU32,
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
    let xmt = NonNullXmt::try_from(read_xmt(bytes, &mut at)?).ok()?;
    let sequence = NonZeroU32::new(View::u32_be_at(bytes, at)?)?;
    at += 4;
    let definition_xmt = read_xmt(bytes, &mut at)?;
    (1..=0x20).contains(&flags).then_some(())?;
    let reference_count = usize::try_from(flags).ok()?.checked_add(5)?;
    let references_at = at;
    let (end, shared_terminal) = entity_51_reference_end(bytes, &mut at, reference_count)?;
    Some(Entity51Frame {
        offset,
        end,
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
    let trailing_references = EntityReferences::new(references.get(5..)?.to_vec()).ok()?;
    Some(Entity51Record {
        offset: frame.offset,
        byte_len: frame.end - frame.offset,
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
    xmt: NonNullXmt,
    name: PrintableString<&'a str>,
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
            let xmt = NonNullXmt::try_from(read_xmt(bytes, &mut at)?).ok()?;
            let name_end = at.checked_add(name_len)?;
            let name_bytes = bytes.get(at..name_end)?;
            Some(AttributeIdentifier {
                offset,
                xmt,
                name: PrintableString::new(std::str::from_utf8(name_bytes).ok()?).ok()?,
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
            let xmt = NonNullXmt::try_from(read_xmt(bytes, &mut at)?).ok()?;
            let next_definition_xmt = XmtTarget::from_wire(read_xmt(bytes, &mut at)?);
            let identifier_xmt = NonNullXmt::try_from(read_xmt(bytes, &mut at)?).ok()?;
            let type_id = NonZeroU32::new(View::u32_be_at(bytes, at)?)?;
            at += 4;
            let mut action_codes = [AttributeAction::Code0; 8];
            for (action, byte) in action_codes.iter_mut().zip(bytes.get(at..at + 8)?) {
                *action = AttributeAction::try_from(*byte).ok()?;
            }
            at += 8;
            let field_names_xmt = XmtTarget::from_wire(read_xmt(bytes, &mut at)?);
            let mut matches = identifiers
                .iter()
                .filter(|identifier| identifier.xmt == identifier_xmt);
            let identifier = matches.next()?;
            matches.next().is_none().then_some(())?;
            let field_count_usize = usize::try_from(field_count).ok()?;
            let (legal_owner_flags, field_codes) =
                [16_usize, 14].into_iter().find_map(|flag_count| {
                    let flags = bytes.get(at..at.checked_add(flag_count)?)?;
                    let legal_owner_flags = LegalOwnerFlags::try_from(flags).ok()?;
                    let field_codes_start = at.checked_add(flag_count)?;
                    let field_codes_end = field_codes_start.checked_add(field_count_usize)?;
                    let field_codes = bytes.get(field_codes_start..field_codes_end)?;
                    if flag_count == 14 && !attribute_definition_boundary(bytes, field_codes_end) {
                        return None;
                    }
                    let fields = field_codes
                        .iter()
                        .copied()
                        .map(AttributeField::try_from)
                        .collect::<Result<Vec<_>, _>>()
                        .ok()?;
                    Some((legal_owner_flags, fields))
                })?;
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
        .and_then(crate::container::DirEntry::file_span)
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
            let body = classify(&inflated);
            streams.push(Stream {
                file_offset: start + offset,
                consumed,
                inflated,
                body,
            });
        }
        if streams.iter().any(|stream| stream.kind().is_parasolid()) {
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
                let body = classify(&inflated);
                let file_offset = file_start + i;
                if seen.insert(file_offset)
                    && (!structural_only || structural_stream_candidate(body.kind(), &inflated))
                {
                    streams.push(Stream {
                        file_offset,
                        consumed,
                        inflated,
                        body,
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
    let census = crate::deltas::census::walk(inflated);
    if !census.records.is_empty() || !census.tombstones.is_empty() {
        return true;
    }
    if kind == StreamKind::Deltas {
        return false;
    }
    let graph = crate::topology::Graph::parse(inflated);
    [
        NodeKind::Body,
        NodeKind::Shell,
        NodeKind::Face,
        NodeKind::Loop,
        NodeKind::Edge,
        NodeKind::Fin,
        NodeKind::Vertex,
        NodeKind::Region,
    ]
    .into_iter()
    .any(|kind| graph.of_kind(kind).next().is_some())
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
        let inflated = ctx.copy_retained(payload, "retain legacy NX Parasolid stream")?;
        let body = classify(&inflated);
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
            body,
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
    let inflated = ctx.copy_retained(view.window(), "retain NX inflated stream")?;
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
fn classify(inflated: &[u8]) -> StreamBody {
    if !inflated.starts_with(b"PS\x00\x00") {
        return StreamBody::Preview;
    }
    let window = &inflated[..inflated.len().min(512)];
    let subtype = if contains(window, b"(partition)") {
        ParasolidSubtype::Partition
    } else if contains(window, b"(deltas)") {
        ParasolidSubtype::Deltas
    } else {
        ParasolidSubtype::Plain
    };
    StreamBody::Parasolid {
        subtype,
        schema: cadmpeg_parasolid::find_schema_token(window)
            .map(cadmpeg_parasolid::OwnedSchemaToken::from),
    }
}

#[cfg(test)]
mod tests;
