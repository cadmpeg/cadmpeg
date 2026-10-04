// SPDX-License-Identifier: Apache-2.0
//! Admitted deltas events and their source-byte census.

use super::record_kind::RecordKind;
use super::tails::{TermUseNumericTail, TerminalNullReferences};
use super::{
    body_revision_prefix, compact_tombstone, consume_fixed, consume_intersection_auxiliary,
    consume_shared_record, consume_variable, first_complete_record, fixed_signature,
    inline_body_states, inline_schema_declaration, inline_schema_declarations, is_value_family,
    merged_event_spans, reference_marker_packets, reference_state_packets, reference_type_map,
    reference_type_maps, schema_reference_preamble, schema_reference_preambles,
    tagged_reference_lanes, term_use_numeric_tails, transmit_header, type_150_state_packets,
    uncovered_spans, BodyRevision, InlineBodyState, InlineSchemaDeclaration, Record,
    ReferenceMarkerPacket, ReferenceStatePacket, ReferenceTypeMap, ReferenceTypeMapLimit,
    SchemaReferencePreamble, TaggedReferenceLane, Tombstone, TransmitHeader, Type150StatePacket,
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

/// Result of a deterministic deltas record walk.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Census {
    events: CensusEvents,
    bytes_decoded: usize,
}

/// Admitted events released by a completed census.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CensusEvents {
    /// Complete stream transmit header.
    pub(crate) transmit_header: Option<TransmitHeader>,
    /// Complete null-reference stream trailer.
    pub(crate) terminal_null_references: Option<TerminalNullReferences>,
    /// Complete records in source order.
    pub(crate) records: Vec<Record>,
    /// Compact tombstones in source order.
    pub(crate) tombstones: Vec<Tombstone>,
    /// BODY revision envelopes in source order.
    pub(crate) body_revisions: Vec<BodyRevision>,
    /// Complete count-selected numeric tails following `term_use` records.
    pub(crate) term_use_numeric_tails: Vec<TermUseNumericTail>,
    /// Maximal event gaps composed entirely of typed stream-local references.
    pub(crate) tagged_reference_lanes: Vec<TaggedReferenceLane>,
    /// Complete framed reference/type maps in source order.
    pub(crate) reference_type_maps: Vec<ReferenceTypeMap>,
    /// Complete four-reference state packets in source order.
    pub(crate) reference_state_packets: Vec<ReferenceStatePacket>,
    /// Complete schema reference preambles in source order.
    pub(crate) schema_reference_preambles: Vec<SchemaReferencePreamble>,
    /// Complete reference-marker packets in source order.
    pub(crate) reference_marker_packets: Vec<ReferenceMarkerPacket>,
    /// Complete single-byte type-150 state packets in source order.
    pub(crate) type_150_state_packets: Vec<Type150StatePacket>,
    /// Complete inline schema declarations in source order.
    pub(crate) inline_schema_declarations: Vec<InlineSchemaDeclaration>,
    /// Complete schema-bound type-12 BODY states in source order.
    pub(crate) inline_body_states: Vec<InlineBodyState>,
}

impl std::ops::Deref for Census {
    type Target = CensusEvents;

    fn deref(&self) -> &Self::Target {
        &self.events
    }
}

impl Census {
    pub(crate) fn bytes_decoded(&self) -> usize {
        self.bytes_decoded
    }

    pub(crate) fn into_events(self) -> CensusEvents {
        self.events
    }

    /// Complete-record counts keyed by Parasolid family name.
    pub(crate) fn full_counts(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<BTreeMap<&'static str, usize>, CodecError> {
        let mut counts = BTreeMap::new();
        for record in &self.records {
            let family = record.family_name();
            ctx.admit_btree_entry(&counts, &family, "NX deltas full count families")?;
            *counts.entry(family).or_default() += 1;
        }
        Ok(counts)
    }

    /// Compact tombstone counts keyed by Parasolid family name.
    pub(crate) fn tombstone_counts(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<BTreeMap<&'static str, usize>, CodecError> {
        let mut counts = BTreeMap::new();
        for tombstone in &self.tombstones {
            let family = tombstone.kind.name();
            ctx.admit_btree_entry(&counts, &family, "NX deltas tombstone count families")?;
            *counts.entry(family).or_default() += 1;
        }
        Ok(counts)
    }

    /// Return the sorted disjoint union of every admitted event byte span.
    pub(crate) fn covered_spans(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<(usize, usize)>, CodecError> {
        merged_event_spans(ctx, self, true)
    }
}

/// Walk all accepted records, revisions, tombstones, and numeric tails in an
/// inflated deltas stream.
pub(crate) fn walk(ctx: &DecodeContext<'_>, stream: &[u8]) -> Result<Census, CodecError> {
    let transmit_header = transmit_header(ctx, stream)?;
    let header_byte_len = transmit_header.as_ref().map_or(0, |header| header.end);
    let terminal_null_references = TerminalNullReferences::at_end(stream);
    let trailer_byte_len = terminal_null_references
        .as_ref()
        .map_or(0, |trailer| trailer.end() - trailer.offset());
    let mut census = Census {
        events: CensusEvents {
            transmit_header,
            terminal_null_references,
            ..CensusEvents::default()
        },
        bytes_decoded: header_byte_len + trailer_byte_len,
    };
    let mut offset = census
        .transmit_header
        .as_ref()
        .map_or(0, |header| header.end);
    let mut value_boundary = true;
    let mut referenced_value_offsets = None::<BTreeSet<usize>>;
    let mut referenced_offsets_guard = ctx.reserve_scoped(0, "NX referenced value offsets")?;
    let mut intersection_schema_anchor_seen = false;
    while offset + 4 <= stream.len() {
        ctx.charge_work(1, "walk NX deltas census")?;
        intersection_schema_anchor_seen |=
            crate::topology::intersection_data_schema_header_at(stream, offset);
        if let Some(preamble) = schema_reference_preamble(ctx, stream, offset, stream.len())? {
            census.bytes_decoded += preamble.end - preamble.offset;
            offset = preamble.end;
            value_boundary = true;
            ctx.push_vec(
                &mut census.events.schema_reference_preambles,
                preamble,
                "NX deltas schema preambles",
            )?;
            continue;
        }
        if let Some(declaration) = inline_schema_declaration(ctx, stream, offset, stream.len())? {
            census.bytes_decoded += declaration.end - declaration.offset;
            offset = declaration.end;
            value_boundary = true;
            ctx.push_vec(
                &mut census.events.inline_schema_declarations,
                declaration,
                "NX deltas schema declarations",
            )?;
            continue;
        }
        if let Some(map) =
            reference_type_map(ctx, stream, offset, ReferenceTypeMapLimit::TargetTerminated)?
        {
            census.bytes_decoded += map.end - map.offset;
            offset = map.end;
            value_boundary = true;
            ctx.push_vec(
                &mut census.events.reference_type_maps,
                map,
                "NX deltas reference type maps",
            )?;
            continue;
        }
        let shared_record = consume_shared_record(
            ctx,
            stream,
            offset,
            &census.records,
            intersection_schema_anchor_seen,
        )?;
        let complete_record = if shared_record.is_some() {
            shared_record
        } else {
            consume_intersection_auxiliary(ctx, stream, offset)?
        };
        let complete_record = if complete_record.is_some() {
            complete_record
        } else {
            first_complete_record(ctx, stream, offset, intersection_schema_anchor_seen, true)?
        };
        if let Some(record) = complete_record {
            census.bytes_decoded += record.end - offset;
            offset = record.end;
            value_boundary = true;
            ctx.push_vec(&mut census.events.records, record, "NX deltas records")?;
            continue;
        }
        let Some(kind) = View::u16_be_at(stream, offset) else {
            break;
        };
        let Ok(record_kind) = RecordKind::try_from(kind) else {
            offset += 1;
            value_boundary = false;
            continue;
        };
        if kind == 12 {
            if let Some(revision) = body_revision_prefix(stream, offset) {
                census.bytes_decoded += revision.prefix_end - revision.offset;
                offset = revision.prefix_end;
                value_boundary = true;
                ctx.push_vec(
                    &mut census.events.body_revisions,
                    revision,
                    "NX deltas body revisions",
                )?;
                continue;
            }
        }
        if is_value_family(kind) && !value_boundary && referenced_value_offsets.is_none() {
            let mut offsets = BTreeSet::new();
            let (events, _event_guard) =
                crate::parasolid::referenced_value_event_offsets(ctx, stream)?;
            for event_offset in events {
                ctx.insert_scoped_btree_set(
                    &mut referenced_offsets_guard,
                    &mut offsets,
                    event_offset,
                    "index NX referenced offsets",
                    "NX referenced value offsets",
                )?;
            }
            referenced_value_offsets = Some(offsets);
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(
                referenced_value_offsets.as_ref().map_or(0, BTreeSet::len),
            ),
            "resolve NX referenced offset",
        )?;
        let value_owned = !is_value_family(kind)
            || value_boundary
            || referenced_value_offsets
                .as_ref()
                .is_some_and(|offsets| offsets.contains(&offset));
        if !value_owned {
            if let Some((parsed_kind, _, byte_len)) =
                crate::parasolid::value_records::entity_value_record_identity_at(
                    ctx, stream, offset,
                )?
            {
                if parsed_kind == kind {
                    offset += byte_len;
                    continue;
                }
            }
        }
        let fixed = if let Some(signature) = fixed_signature(kind) {
            consume_fixed(ctx, stream, offset, kind, signature)?
        } else {
            None
        };
        let decoded = if fixed.is_some() || !value_owned {
            fixed
        } else {
            consume_variable(ctx, stream, offset, kind)?
        };
        if let Some(record) = decoded {
            census.bytes_decoded += record.end - record.offset;
            offset = record.end;
            value_boundary = true;
            ctx.push_vec(&mut census.events.records, record, "NX deltas records")?;
            continue;
        }
        if let Some(xmt) = (kind != 98)
            .then(|| compact_tombstone(stream, offset))
            .flatten()
        {
            if xmt > 1 {
                ctx.push_vec(
                    &mut census.events.tombstones,
                    Tombstone {
                        kind: record_kind,
                        xmt,
                        offset,
                    },
                    "NX deltas tombstones",
                )?;
                census.bytes_decoded += 6;
                offset += 6;
                value_boundary = true;
                continue;
            }
        }
        offset += 1;
        value_boundary = false;
    }
    census.events.term_use_numeric_tails = term_use_numeric_tails(ctx, stream, &census)?;
    census.bytes_decoded += census
        .term_use_numeric_tails
        .iter()
        .map(|tail| tail.values().byte_len())
        .sum::<usize>();
    census.bytes_decoded += populate_gap_events(ctx, stream, &mut census)?;
    let body_revision_state_bytes = populate_body_revision_state_tails(ctx, stream, &mut census)?;
    census.bytes_decoded += body_revision_state_bytes;
    Ok(census)
}

fn populate_gap_events(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    census: &mut Census,
) -> Result<usize, CodecError> {
    let mut admitted_bytes = 0;
    loop {
        let covered_before = merged_event_spans(ctx, census, true)?
            .into_iter()
            .map(|(start, end)| end - start)
            .sum::<usize>();

        let lanes = tagged_reference_lanes(ctx, stream, census)?;
        ctx.extend_vec(
            &mut census.events.tagged_reference_lanes,
            lanes,
            "NX tagged reference lanes",
        )?;

        let maps = reference_type_maps(ctx, stream, census)?;
        ctx.extend_vec(
            &mut census.events.reference_type_maps,
            maps,
            "NX reference type maps",
        )?;

        let state_packets = reference_state_packets(ctx, stream, census)?;
        ctx.extend_vec(
            &mut census.events.reference_state_packets,
            state_packets,
            "NX reference state packets",
        )?;

        let preambles = schema_reference_preambles(ctx, stream, census)?;
        ctx.extend_vec(
            &mut census.events.schema_reference_preambles,
            preambles,
            "NX schema reference preambles",
        )?;

        let declarations = inline_schema_declarations(ctx, stream, census)?;
        ctx.extend_vec(
            &mut census.events.inline_schema_declarations,
            declarations,
            "NX inline schema declarations",
        )?;

        let body_states = inline_body_states(ctx, stream, census)?;
        ctx.extend_vec(
            &mut census.events.inline_body_states,
            body_states,
            "NX inline body states",
        )?;

        let marker_packets = reference_marker_packets(ctx, stream, census)?;
        ctx.extend_vec(
            &mut census.events.reference_marker_packets,
            marker_packets,
            "NX reference marker packets",
        )?;

        let type_150_packets = type_150_state_packets(ctx, stream, census)?;
        ctx.extend_vec(
            &mut census.events.type_150_state_packets,
            type_150_packets,
            "NX type 150 state packets",
        )?;

        let covered_after = merged_event_spans(ctx, census, true)?
            .into_iter()
            .map(|(start, end)| end - start)
            .sum::<usize>();
        let added_bytes = covered_after - covered_before;
        admitted_bytes += added_bytes;
        if added_bytes == 0 {
            break;
        }
    }

    ctx.sort_unstable_by(
        &mut census.events.tagged_reference_lanes,
        |value| &value.offset,
        Ord::cmp,
        "NX deltas tagged reference lanes",
    )?;
    ctx.sort_unstable_by(
        &mut census.events.reference_type_maps,
        |value| &value.offset,
        Ord::cmp,
        "NX deltas reference type maps",
    )?;
    ctx.sort_unstable_by(
        &mut census.events.reference_state_packets,
        |value| &value.offset,
        Ord::cmp,
        "NX deltas reference state packets",
    )?;
    ctx.sort_unstable_by(
        &mut census.events.schema_reference_preambles,
        |value| &value.offset,
        Ord::cmp,
        "NX deltas schema reference preambles",
    )?;
    ctx.sort_unstable_by(
        &mut census.events.inline_schema_declarations,
        |value| &value.offset,
        Ord::cmp,
        "NX deltas inline schema declarations",
    )?;
    ctx.sort_unstable_by(
        &mut census.events.inline_body_states,
        |value| &value.offset,
        Ord::cmp,
        "NX deltas inline body states",
    )?;
    ctx.sort_unstable_by(
        &mut census.events.reference_marker_packets,
        |value| &value.offset,
        Ord::cmp,
        "NX deltas reference marker packets",
    )?;
    ctx.sort_unstable_by(
        &mut census.events.type_150_state_packets,
        |value| &value.offset,
        Ord::cmp,
        "NX deltas type 150 state packets",
    )?;
    Ok(admitted_bytes)
}

fn populate_body_revision_state_tails(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    census: &mut Census,
) -> Result<usize, CodecError> {
    let mut byte_len = 0;
    for (start, end) in uncovered_spans(ctx, stream.len(), census, true)? {
        if let Some(revision) = census
            .events
            .body_revisions
            .iter_mut()
            .find(|revision| revision.prefix_end == start)
        {
            revision.end = end;
            byte_len += end - start;
        }
    }
    Ok(byte_len)
}
