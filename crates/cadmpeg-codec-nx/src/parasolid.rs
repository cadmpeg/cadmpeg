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
use cadmpeg_core::decode::{ByteRange, DecodeContext, ExpandSpec, ScopedReservation, View};
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
pub(crate) enum StreamKind {
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
    pub(crate) fn label(self) -> &'static str {
        match self {
            StreamKind::Partition => "partition",
            StreamKind::Deltas => "deltas",
            StreamKind::Plain => "plain",
            StreamKind::Preview => "preview",
        }
    }

    /// Return whether this kind contains Parasolid neutral-binary records.
    pub(crate) fn is_parasolid(self) -> bool {
        !matches!(self, StreamKind::Preview)
    }
}

/// Parasolid stream record subtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParasolidSubtype {
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
pub(crate) enum StreamBody {
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
pub(crate) struct Stream {
    /// Byte offset of the stream start in the source file.
    ///
    /// Modern streams start at a zlib header. Legacy streams start at a clear
    /// Parasolid transmit header.
    pub(crate) file_offset: usize,
    /// Source bytes consumed by the stream at `file_offset`.
    ///
    /// For modern streams this is the compressed member length. For legacy
    /// streams it is the clear section length. The physical extent
    /// `[file_offset, file_offset + consumed)` is source-owned.
    pub(crate) consumed: u64,
    /// Inflated bytes.
    pub(crate) inflated: Vec<u8>,
    /// Payload classification.
    pub(crate) body: StreamBody,
}

impl Stream {
    /// Stream classification.
    pub(crate) fn kind(&self) -> StreamKind {
        self.body.kind()
    }

    /// Parasolid schema token.
    pub(crate) fn schema_token(&self) -> Option<&cadmpeg_parasolid::OwnedSchemaToken> {
        match &self.body {
            StreamBody::Parasolid { schema, .. } => schema.as_ref(),
            StreamBody::Preview => None,
        }
    }
}

/// Owner-flag layouts admitted by the attribute-definition grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegalOwnerFlags {
    /// Fourteen flags in the compact definition layout.
    Fourteen([bool; 14]),
    /// Sixteen flags in the extended definition layout.
    Sixteen([bool; 16]),
}

impl LegalOwnerFlags {
    pub(crate) fn as_slice(&self) -> &[bool] {
        match self {
            Self::Fourteen(flags) => flags,
            Self::Sixteen(flags) => flags,
        }
    }

    /// Fixed-width native JSON representation, padded after the source flags.
    pub(crate) fn padded(self) -> [u8; 16] {
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
pub(crate) struct AttributeDefinition<'a> {
    /// Inflated-stream offset of the `00 50` definition tag.
    pub(crate) offset: usize,
    /// Stream-local definition record identity.
    pub(crate) xmt: NonNullXmt,
    /// Optional stream-local next-definition target.
    pub(crate) next_definition_xmt: Option<XmtTarget>,
    /// Stream-local type-79 identifier identity.
    pub(crate) identifier_xmt: NonNullXmt,
    /// Inflated-stream offset of the resolved `00 4f` identifier tag.
    pub(crate) identifier_offset: usize,
    /// Exact printable class name.
    pub(crate) name: PrintableString<&'a str>,
    /// Numeric attribute type identifier.
    pub(crate) type_id: NonZeroU32,
    /// Ordered actions for the eight logged event families.
    pub(crate) action_codes: [AttributeAction; 8],
    /// Optional stream-local field-name-list target.
    pub(crate) field_names_xmt: Option<XmtTarget>,
    /// Ordered legal-owner flags.
    pub(crate) legal_owner_flags: LegalOwnerFlags,
    /// One serialized field code for every declared field.
    pub(crate) field_codes: Vec<AttributeField>,
}

/// One framed type-81 Parasolid entity/attribute-list record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entity51Record {
    /// Inflated-stream offset of the `00 51` tag.
    pub(crate) offset: usize,
    /// Exact framed record length.
    pub(crate) byte_len: usize,
    /// Stream-local record identity.
    pub(crate) xmt: NonNullXmt,
    /// Serialized sequence value.
    pub(crate) sequence: NonZeroU32,
    /// Stream-local type-80 attribute-definition identity.
    pub(crate) definition_xmt: u32,
    /// Five fixed leading stream-local references.
    pub(crate) leading_references: [u32; 5],
    /// Variable trailing stream-local references counted by `flags`.
    pub(crate) trailing_references: EntityReferences,
}

/// One counted type-99 attribute field-name record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FieldNamesRecord {
    /// Inflated-stream offset of the `00 63` tag.
    pub(crate) offset: usize,
    /// Exact framed record length.
    pub(crate) byte_len: usize,
    /// Stream-local record identity.
    pub(crate) xmt: NonNullXmt,
    /// Ordered stream-local character or Unicode value references.
    pub(crate) name_xmts: NameReferences,
}

/// Locate unique snapshot values with scoped discovery storage.
pub(crate) fn referenced_value_record_offsets<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
) -> Result<(Vec<usize>, ScopedReservation<'ctx>), CodecError> {
    referenced_value_offsets(ctx, bytes, ValueMultiplicity::UniqueSnapshot)
}

/// Locate historical value events with scoped discovery storage.
pub(crate) fn referenced_value_event_offsets<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
) -> Result<(Vec<usize>, ScopedReservation<'ctx>), CodecError> {
    referenced_value_offsets(ctx, bytes, ValueMultiplicity::HistoricalEvents)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueMultiplicity {
    UniqueSnapshot,
    HistoricalEvents,
}

fn value_is_referenced(
    ctx: &DecodeContext<'_>,
    references: &BTreeSet<u32>,
    xmt: u32,
    operation: &'static str,
) -> Result<bool, CodecError> {
    // Twelve comparisons per binary level bound eleven-key B-tree nodes.
    let comparisons = 12 * u64::from(usize::BITS - references.len().leading_zeros());
    ctx.charge_work(comparisons + 1, operation)?;
    Ok(references.contains(&xmt))
}

fn referenced_value_offsets<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    multiplicity: ValueMultiplicity,
) -> Result<(Vec<usize>, ScopedReservation<'ctx>), CodecError> {
    let (references, _reference_guard) = referenced_value_xmts(ctx, bytes, multiplicity)?;
    let (candidates, _candidate_guard) = value_records::value_record_candidates(ctx, bytes)?;
    let mut offsets = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX value owner offsets")?;
    for (xmt, positions) in candidates {
        if !value_is_referenced(ctx, &references, xmt, "resolve NX value ownership")?
            || (multiplicity == ValueMultiplicity::UniqueSnapshot && positions.len() != 1)
        {
            continue;
        }
        for offset in positions {
            ctx.push_scoped_vec(
                &mut reservation,
                &mut offsets,
                offset,
                "NX value owner offsets",
            )?;
        }
    }
    Ok((offsets, reservation))
}

fn referenced_value_xmts<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    multiplicity: ValueMultiplicity,
) -> Result<(BTreeSet<u32>, ScopedReservation<'ctx>), CodecError> {
    let mut referenced = BTreeSet::new();
    let mut reference_guard = ctx.reserve_scoped(0, "NX owned value identities")?;
    let mut groups_guard = ctx.reserve_scoped(0, "NX attribute ownership groups")?;
    let AttributeScan {
        records: entity_records,
        slots: _entity_slots,
        payloads: _entity_payloads,
    } = entity_51_records(ctx, bytes)?;
    let mut entities = BTreeMap::<u32, Vec<Entity51Record>>::new();
    for record in entity_records {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(entities.len()),
            "group NX entity-51 identities",
        )?;
        ctx.push_scoped_btree_group(
            &mut groups_guard,
            &mut entities,
            u32::from(record.xmt),
            || record,
            0,
            "NX attribute ownership groups",
        )?;
    }
    for records in entities.into_values() {
        if multiplicity == ValueMultiplicity::UniqueSnapshot && records.len() != 1 {
            continue;
        }
        for record in records {
            for xmt in record.trailing_references.into_values() {
                ctx.insert_scoped_btree_set(
                    &mut reference_guard,
                    &mut referenced,
                    xmt,
                    "index NX owned values",
                    "NX owned value identities",
                )?;
            }
        }
    }
    let AttributeScan {
        records: name_records,
        slots: _name_slots,
        payloads: _name_payloads,
    } = field_names_records(ctx, bytes)?;
    let mut field_names = BTreeMap::<u32, Vec<FieldNamesRecord>>::new();
    for record in name_records {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(field_names.len()),
            "group NX field-name identities",
        )?;
        ctx.push_scoped_btree_group(
            &mut groups_guard,
            &mut field_names,
            u32::from(record.xmt),
            || record,
            0,
            "NX attribute ownership groups",
        )?;
    }
    let AttributeScan {
        records: definition_records,
        slots: _definition_slots,
        payloads: _definition_payloads,
    } = attribute_definitions(ctx, bytes)?;
    let mut definitions = BTreeMap::<u32, Vec<AttributeDefinition<'_>>>::new();
    for record in definition_records {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(definitions.len()),
            "group NX attribute definitions",
        )?;
        ctx.push_scoped_btree_group(
            &mut groups_guard,
            &mut definitions,
            u32::from(record.xmt),
            || record,
            0,
            "NX attribute ownership groups",
        )?;
    }
    let mut referenced_lists = BTreeSet::new();
    let mut list_guard = ctx.reserve_scoped(0, "NX referenced field-name lists")?;
    for records in definitions.into_values() {
        if multiplicity == ValueMultiplicity::UniqueSnapshot && records.len() != 1 {
            continue;
        }
        for record in records {
            if let Some(xmt) = record.field_names_xmt {
                ctx.insert_scoped_btree_set(
                    &mut list_guard,
                    &mut referenced_lists,
                    u32::from(xmt),
                    "index NX field-name ownership",
                    "NX referenced field-name lists",
                )?;
            }
        }
    }
    for (xmt, records) in field_names {
        if !value_is_referenced(
            ctx,
            &referenced_lists,
            xmt,
            "resolve NX field-name ownership",
        )? || (multiplicity == ValueMultiplicity::UniqueSnapshot && records.len() != 1)
        {
            continue;
        }
        for record in records {
            for xmt in record.name_xmts.as_slice() {
                ctx.insert_scoped_btree_set(
                    &mut reference_guard,
                    &mut referenced,
                    u32::from(*xmt),
                    "index NX owned values",
                    "NX owned value identities",
                )?;
            }
        }
    }
    Ok((referenced, reference_guard))
}

pub(crate) struct AttributeScan<'ctx, T> {
    pub(crate) records: Vec<T>,
    pub(crate) slots: ScopedReservation<'ctx>,
    pub(crate) payloads: ScopedReservation<'ctx>,
}

/// Decode counted type-99 attribute field-name records with scoped ownership.
pub(crate) fn field_names_records<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
) -> Result<AttributeScan<'ctx, FieldNamesRecord>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX field names",
    )?;
    let mut records = Vec::new();
    let mut slots = ctx.reserve_scoped(0, "NX field-name record slots")?;
    let mut payloads = ctx.reserve_scoped(0, "NX field-name reference lanes")?;
    let mut offset = 0;
    while offset < bytes.len() {
        if let Some(record) = field_names_record_at(ctx, bytes, offset, &mut payloads)? {
            offset += record.byte_len;
            ctx.push_scoped_vec(
                &mut slots,
                &mut records,
                record,
                "NX field-name record slots",
            )?;
        } else {
            offset += 1;
        }
    }
    Ok(AttributeScan {
        records,
        slots,
        payloads,
    })
}

fn field_names_record_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
    payloads: &mut ScopedReservation<'_>,
) -> Result<Option<FieldNamesRecord>, CodecError> {
    let parsed: Option<Result<_, CodecError>> = (|| {
        let mut at = offset.checked_add(2)?;
        (bytes.get(offset..at) == Some(&[0, 0x63])).then_some(())?;
        if bytes.get(at) == Some(&0xff) {
            at += 1;
        }
        let count = usize::try_from(View::u32_be_at(bytes, at)?).ok()?;
        at += 4;
        let xmt = NonNullXmt::try_from(read_xmt(bytes, &mut at)?).ok()?;
        // Each reference requires at least two source bytes.
        (count <= bytes.len().checked_sub(at)? / 2).then_some(())?;
        propagate_resource!(ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(count),
            "read NX field-name references"
        ));
        let mut probe = at;
        for _ in 0..count {
            NonNullXmt::try_from(read_xmt(bytes, &mut probe)?).ok()?;
        }
        propagate_resource!(ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(count),
            "materialize NX field-name references"
        ));
        let mut name_xmts = Vec::new();
        propagate_resource!(ctx.reserve_scoped_vec(
            payloads,
            &mut name_xmts,
            count,
            "NX field-name reference lanes"
        ));
        for _ in 0..count {
            name_xmts.push(NonNullXmt::try_from(read_xmt(bytes, &mut at)?).ok()?);
        }
        Some(Ok(FieldNamesRecord {
            offset,
            byte_len: at - offset,
            xmt,
            name_xmts: NameReferences::try_from(name_xmts).ok()?,
        }))
    })();
    parsed.transpose()
}

/// Decode framed type-81 entity/attribute-list records with scoped ownership.
pub(crate) fn entity_51_records<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
) -> Result<AttributeScan<'ctx, Entity51Record>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX entity-51 records",
    )?;
    let mut records = Vec::new();
    let mut slots = ctx.reserve_scoped(0, "NX entity-51 record slots")?;
    let mut payloads = ctx.reserve_scoped(0, "NX entity-51 reference lanes")?;
    let mut offset = 0;
    while offset < bytes.len() {
        let Some(frame) = entity_51_frame_at(bytes, offset) else {
            offset += 1;
            continue;
        };
        if let Some(record) = entity_51_record_from_frame(ctx, bytes, frame, &mut payloads)? {
            let Some(next) = frame.next_offset() else {
                break;
            };
            ctx.push_scoped_vec(
                &mut slots,
                &mut records,
                record,
                "NX entity-51 record slots",
            )?;
            offset = next;
        } else {
            offset += 1;
        }
    }
    Ok(AttributeScan {
        records,
        slots,
        payloads,
    })
}

/// Decode and retain one complete type-81 entity/attribute-list record.
pub(crate) fn entity_51_record_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
) -> Result<Option<Entity51Record>, CodecError> {
    let Some(frame) = entity_51_frame_at(bytes, offset) else {
        return Ok(None);
    };
    let mut payloads = ctx.reserve_scoped(0, "NX entity-51 reference lanes")?;
    let record = entity_51_record_from_frame(ctx, bytes, frame, &mut payloads)?;
    if record.is_some() {
        payloads.commit()?;
    }
    Ok(record)
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
    fn next_offset(self) -> Option<usize> {
        if self.shared_terminal {
            self.end.checked_sub(1)
        } else {
            Some(self.end)
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

fn entity_51_record_from_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frame: Entity51Frame,
    payloads: &mut ScopedReservation<'_>,
) -> Result<Option<Entity51Record>, CodecError> {
    let parsed: Option<Result<_, CodecError>> = (|| {
        propagate_resource!(ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(frame.reference_count),
            "materialize NX entity-51 references"
        ));
        let mut at = frame.references_at;
        let prefixed = bytes.get(at) == Some(&1);
        let mut leading_references = [0; 5];
        for reference in &mut leading_references {
            if prefixed {
                matches!(bytes.get(at), Some(0 | 1)).then_some(())?;
                at += 1;
            }
            *reference = read_xmt(bytes, &mut at)?;
        }
        let mut trailing = Vec::new();
        propagate_resource!(ctx.reserve_scoped_vec(
            payloads,
            &mut trailing,
            frame.reference_count.checked_sub(5)?,
            "NX entity-51 reference lanes"
        ));
        for _ in 5..frame.reference_count {
            if prefixed {
                matches!(bytes.get(at), Some(0 | 1)).then_some(())?;
                at += 1;
            }
            trailing.push(read_xmt(bytes, &mut at)?);
        }
        Some(Ok(Entity51Record {
            offset: frame.offset,
            byte_len: frame.end - frame.offset,
            xmt: frame.xmt,
            sequence: frame.sequence,
            definition_xmt: frame.definition_xmt,
            leading_references,
            trailing_references: EntityReferences::new(trailing).ok()?,
        }))
    })();
    parsed.transpose()
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

#[derive(Debug, Clone, Copy)]
struct AttributeIdentifier<'a> {
    offset: usize,
    xmt: NonNullXmt,
    name: PrintableString<&'a str>,
}

fn attribute_identifiers<'bytes, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &'bytes [u8],
) -> Result<
    (
        BTreeMap<u32, Option<AttributeIdentifier<'bytes>>>,
        ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX attribute identifiers",
    )?;
    let mut index = BTreeMap::new();
    let mut reservation = ctx.reserve_scoped(0, "NX attribute identifier index")?;
    for offset in 0..bytes.len() {
        let candidate: Option<Result<_, CodecError>> = (|| {
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
            propagate_resource!(ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(name_len),
                "validate NX attribute name"
            ));
            Some(Ok(AttributeIdentifier {
                offset,
                xmt,
                name: PrintableString::new(std::str::from_utf8(name_bytes).ok()?).ok()?,
            }))
        })();
        if let Some(identifier) = candidate.transpose()? {
            let key = u32::from(identifier.xmt);
            if !ctx.insert_scoped_btree_map_if_vacant(
                &mut reservation,
                &mut index,
                key,
                Some(identifier),
                "index NX attribute identifiers",
                "NX attribute identifier index",
            )? {
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(index.len()),
                    "resolve duplicate NX attribute identifier",
                )?;
                if let Some(value) = index.get_mut(&key) {
                    *value = None;
                }
            }
        }
    }
    Ok((index, reservation))
}

/// Decode complete type-80 attribute definitions and resolve their type-79 identifiers.
pub(crate) fn attribute_definitions<'bytes, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &'bytes [u8],
) -> Result<AttributeScan<'ctx, AttributeDefinition<'bytes>>, CodecError> {
    let (identifiers, _identifier_reservation) = attribute_identifiers(ctx, bytes)?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX attribute definitions",
    )?;
    let mut records = Vec::new();
    let mut slots = ctx.reserve_scoped(0, "NX attribute definition slots")?;
    let mut payloads = ctx.reserve_scoped(0, "NX attribute field lanes")?;
    for offset in 0..bytes.len() {
        let candidate: Option<Result<_, CodecError>> = (|| {
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
            propagate_resource!(ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(identifiers.len()),
                "resolve NX attribute identifier"
            ));
            let identifier = identifiers.get(&u32::from(identifier_xmt))?.as_ref()?;
            let field_count_usize = usize::try_from(field_count).ok()?;
            let (legal_owner_flags, field_codes) = propagate_resource!([16_usize, 14]
                .into_iter()
                .find_map(|flag_count| {
                    let flags = bytes.get(at..at.checked_add(flag_count)?)?;
                    let legal_owner_flags = LegalOwnerFlags::try_from(flags).ok()?;
                    let field_codes_start = at.checked_add(flag_count)?;
                    let field_codes_end = field_codes_start.checked_add(field_count_usize)?;
                    let field_codes = bytes.get(field_codes_start..field_codes_end)?;
                    if flag_count == 14 && !attribute_definition_boundary(bytes, field_codes_end) {
                        return None;
                    }
                    propagate_resource!(ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(field_codes.len()),
                        "validate NX attribute fields"
                    ));
                    field_codes
                        .iter()
                        .try_for_each(|code| AttributeField::try_from(*code).map(|_| ()).ok())?;
                    Some(Ok((legal_owner_flags, field_codes)))
                })
                .transpose())?;
            let mut fields = Vec::new();
            propagate_resource!(ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(field_codes.len()),
                "materialize NX attribute fields"
            ));
            propagate_resource!(ctx.reserve_scoped_vec(
                &mut payloads,
                &mut fields,
                field_codes.len(),
                "NX attribute field lanes"
            ));
            for code in field_codes {
                fields.push(AttributeField::try_from(*code).ok()?);
            }
            Some(Ok(AttributeDefinition {
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
                field_codes: fields,
            }))
        })();
        if let Some(record) = candidate.transpose()? {
            ctx.push_scoped_vec(
                &mut slots,
                &mut records,
                record,
                "NX attribute definition slots",
            )?;
        }
    }
    Ok(AttributeScan {
        records,
        slots,
        payloads,
    })
}

fn attribute_definition_boundary(bytes: &[u8], offset: usize) -> bool {
    offset
        .checked_add(2)
        .and_then(|end| bytes.get(offset..end))
        .is_some_and(|tag| tag[0] == 0 && (0x4f..=0x63).contains(&tag[1]))
}

/// Locates, inflates, and classifies zlib streams in `/Root/UG_PART/UG_PART`.
pub(crate) fn extract_streams<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
    container: &Container,
) -> Result<Vec<Stream>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(container.entries.len()),
        "find NX part stream",
    )?;
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
            start: cadmpeg_core::decode::u64_from_index(start),
            end: cadmpeg_core::decode::u64_from_index(end),
        },
    )?;
    let part = part_view.window();

    let mut streams = Vec::new();
    if container.segment_index().is_some() {
        let mut seen = BTreeSet::new();
        let mut seen_guard = ctx.reserve_scoped(0, "NX indexed stream offsets")?;
        for wrapper in container.segment_stream_wrappers() {
            ctx.charge_work(1, "scan NX indexed stream wrappers")?;
            let Some(offset) = wrapper.zlib_offset.checked_sub(start) else {
                continue;
            };
            if !ctx.insert_scoped_btree_set(
                &mut seen_guard,
                &mut seen,
                offset,
                "index NX stream offset",
                "NX indexed stream offsets",
            )? || offset
                .checked_add(2)
                .and_then(|end| part.get(offset..end))
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
            ctx.charge_entities(1, "admit NX streams")?;
            ctx.push_vec(
                &mut streams,
                Stream {
                    file_offset: start + offset,
                    consumed,
                    inflated,
                    body,
                },
                "NX embedded stream slots",
            )?;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(streams.len()),
            "classify NX stream roster",
        )?;
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
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(part.len()),
        "scan NX embedded stream bytes",
    )?;
    let mut seen = BTreeSet::new();
    let mut seen_guard = ctx.reserve_scoped(0, "NX embedded stream offset index")?;
    for stream in streams.iter() {
        ctx.insert_scoped_btree_set(
            &mut seen_guard,
            &mut seen,
            stream.file_offset,
            "index NX stream offset",
            "NX embedded stream offset index",
        )?;
    }
    let mut i = 0usize;
    while i + 2 <= part.len() {
        if is_zlib_header(part[i], part[i + 1]) {
            if let Some((inflated, consumed)) = inflate_stream(ctx, part_view, i)? {
                let body = classify(&inflated);
                let file_offset = file_start + i;
                if ctx.insert_scoped_btree_set(
                    &mut seen_guard,
                    &mut seen,
                    file_offset,
                    "index NX stream offset",
                    "NX embedded stream offset index",
                )? && (!structural_only
                    || structural_stream_candidate(ctx, body.kind(), &inflated)?)
                {
                    ctx.charge_entities(1, "admit NX streams")?;
                    ctx.push_vec(
                        streams,
                        Stream {
                            file_offset,
                            consumed,
                            inflated,
                            body,
                        },
                        "NX embedded stream slots",
                    )?;
                }
                // Resume past the bytes this member consumed, not at the next
                // byte: a spurious `78 xx` zlib header inside the compressed
                // body would otherwise inflate into a second stream whose source
                // extent [file_offset, file_offset+consumed) overlaps this
                // member's, double-attributing the same compressed bytes to two
                // decompression origins. Skipping the consumed run keeps packed
                // members' input extents disjoint.
                //
                // Plain `+`: `inflate_stream` reads only from `part[i..]`, so
                // `inflate_zlib_member`'s report is at most the remaining
                // length and the sum is a byte offset of `part`.
                i += packed_member_advance(file_offset, consumed)?;
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

fn structural_stream_candidate(
    ctx: &DecodeContext<'_>,
    kind: StreamKind,
    inflated: &[u8],
) -> Result<bool, CodecError> {
    if !kind.is_parasolid() {
        return Ok(false);
    }
    let census = crate::deltas::census::walk(ctx, inflated)?;
    if !census.records.is_empty() || !census.tombstones.is_empty() {
        return Ok(true);
    }
    if kind == StreamKind::Deltas {
        return Ok(false);
    }
    let graph = crate::topology::Graph::parse(ctx, inflated)?;
    Ok([
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
    .any(|kind| graph.of_kind(kind).next().is_some()))
}

/// Locate clear Parasolid transmit sections in a legacy `UG_PART/UG_PART`
/// stream.
///
/// Legacy NX stores multiple self-describing Parasolid sections consecutively.
/// A section begins with `PS`, its big-endian description length, and a
/// printable `TRANSMIT FILE` description. Only those complete transmit headers
/// are admitted as boundaries; arbitrary `PS\0\0` bytes in the payload do not
/// split a stream.
pub(crate) fn extract_legacy_streams<'a>(
    ctx: &DecodeContext<'a>,
    part: View<'a>,
) -> Result<Vec<Stream>, CodecError> {
    let bytes = part.window();
    let mut streams = Vec::new();
    let mut search = 0;
    while let Some(start) = legacy_stream_start(ctx, bytes, search)? {
        let next = match start.checked_add(4) {
            Some(next) => legacy_stream_start(ctx, bytes, next)?,
            None => None,
        };
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
        ctx.charge_entities(1, "admit NX streams")?;
        ctx.push_vec(
            &mut streams,
            Stream {
                file_offset,
                consumed,
                inflated,
                body,
            },
            "NX embedded stream slots",
        )?;
        let Some(next) = next else {
            break;
        };
        search = next;
    }
    Ok(streams)
}

fn legacy_stream_start(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    mut search: usize,
) -> Result<Option<usize>, CodecError> {
    loop {
        let Some(window) = bytes.get(search..) else {
            return Ok(None);
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(window.len()),
            "scan NX legacy stream headers",
        )?;
        let Some(relative) = find(window, b"PS\x00\x00") else {
            return Ok(None);
        };
        let Some(start) = search.checked_add(relative) else {
            return Ok(None);
        };
        if legacy_transmit_header(ctx, bytes, start)? {
            return Ok(Some(start));
        }
        let Some(next) = start.checked_add(4) else {
            return Ok(None);
        };
        search = next;
    }
}

fn legacy_transmit_header(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<bool, CodecError> {
    let Some(description_len) = start
        .checked_add(2)
        .and_then(|offset| View::u32_be_at(bytes, offset))
    else {
        return Ok(false);
    };
    let Ok(description_len) = usize::try_from(description_len) else {
        return Ok(false);
    };
    let Some(description_start) = start.checked_add(6) else {
        return Ok(false);
    };
    let Some(description_end) = description_start.checked_add(description_len) else {
        return Ok(false);
    };
    let Some(description) = bytes.get(description_start..description_end) else {
        return Ok(false);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(description.len()),
        "validate NX legacy stream description",
    )?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(description.len()),
        "find NX legacy transmit marker",
    )?;
    Ok(description
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        && contains(description, b"TRANSMIT FILE"))
}

/// Inflate one complete zlib member.
/// The shortest zlib member: the two-byte header, the two bytes a final empty
/// deflate block needs, and the four-byte Adler-32 trailer.
///
/// `inflate_zlib_member` returns only on the decompressor's `StreamEnd`, so it
/// read a complete member and nothing shorter than this is one.
const MIN_ZLIB_MEMBER_LEN: usize = 8;

/// The distance the packed-member scan advances past the member at
/// `file_offset` whose decompressor reported `consumed` source bytes.
///
/// The report comes from the decompressor, not from a byte of the file, so it
/// is read rather than assumed: a count this file cannot address, or one below
/// the shortest zlib member, is a malformed member and is refused by name.
fn packed_member_advance(file_offset: usize, consumed: u64) -> Result<usize, CodecError> {
    let advance = usize::try_from(consumed).map_err(|_| {
        CodecError::malformed(format_args!(
            "nx packed member at {file_offset} reports {consumed} consumed source bytes, \
             which this file cannot address"
        ))
    })?;
    if advance < MIN_ZLIB_MEMBER_LEN {
        return Err(CodecError::malformed(format_args!(
            "nx packed member at {file_offset} reports {advance} consumed source bytes, \
             below the {MIN_ZLIB_MEMBER_LEN} a zlib member holds"
        )));
    }
    Ok(advance)
}

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
