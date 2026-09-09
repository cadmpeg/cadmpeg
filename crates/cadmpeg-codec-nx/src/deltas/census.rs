// SPDX-License-Identifier: Apache-2.0
//! Admitted deltas events and their source-byte census.

use super::record_kind::RecordKind;
use super::tails::{TermUseNumericTail, TerminalNullReferences};
use super::{
    body_revision_prefix, compact_tombstone, consume_attdef_list, consume_fixed,
    consume_intersection_auxiliary, consume_intersection_data, consume_nurbs_auxiliary,
    consume_shared_record, consume_type_101, consume_type_141, consume_type_45, consume_type_67,
    consume_type_70, consume_variable, fixed_signature, inline_body_states,
    inline_schema_declaration, inline_schema_declarations, is_value_family, merged_event_spans,
    reference_marker_packets, reference_state_packets, reference_type_map, reference_type_maps,
    schema_reference_preamble, schema_reference_preambles, tagged_reference_lanes,
    term_use_numeric_tails, transmit_header, type_150_state_packets, uncovered_spans, BodyRevision,
    InlineBodyState, InlineSchemaDeclaration, Record, ReferenceMarkerPacket, ReferenceStatePacket,
    ReferenceTypeMap, ReferenceTypeMapLimit, SchemaReferencePreamble, TaggedReferenceLane,
    Tombstone, TransmitHeader, Type150StatePacket,
};
use cadmpeg_core::decode::View;
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
    pub transmit_header: Option<TransmitHeader>,
    /// Complete null-reference stream trailer.
    pub terminal_null_references: Option<TerminalNullReferences>,
    /// Complete records in source order.
    pub records: Vec<Record>,
    /// Compact tombstones in source order.
    pub tombstones: Vec<Tombstone>,
    /// BODY revision envelopes in source order.
    pub body_revisions: Vec<BodyRevision>,
    /// Complete count-selected numeric tails following `term_use` records.
    pub term_use_numeric_tails: Vec<TermUseNumericTail>,
    /// Maximal event gaps composed entirely of typed stream-local references.
    pub tagged_reference_lanes: Vec<TaggedReferenceLane>,
    /// Complete framed reference/type maps in source order.
    pub reference_type_maps: Vec<ReferenceTypeMap>,
    /// Complete four-reference state packets in source order.
    pub reference_state_packets: Vec<ReferenceStatePacket>,
    /// Complete schema reference preambles in source order.
    pub schema_reference_preambles: Vec<SchemaReferencePreamble>,
    /// Complete reference-marker packets in source order.
    pub reference_marker_packets: Vec<ReferenceMarkerPacket>,
    /// Complete single-byte type-150 state packets in source order.
    pub type_150_state_packets: Vec<Type150StatePacket>,
    /// Complete inline schema declarations in source order.
    pub inline_schema_declarations: Vec<InlineSchemaDeclaration>,
    /// Complete schema-bound type-12 BODY states in source order.
    pub inline_body_states: Vec<InlineBodyState>,
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
    pub fn full_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for record in &self.records {
            *counts.entry(record.family_name()).or_default() += 1;
        }
        counts
    }

    /// Compact tombstone counts keyed by Parasolid family name.
    pub fn tombstone_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for tombstone in &self.tombstones {
            *counts.entry(tombstone.kind.name()).or_default() += 1;
        }
        counts
    }

    /// Return the sorted disjoint union of every admitted event byte span.
    pub(crate) fn covered_spans(&self) -> Vec<(usize, usize)> {
        merged_event_spans(self, true)
    }
}

/// Walk all accepted records, revisions, tombstones, and numeric tails in an
/// inflated deltas stream.
pub(crate) fn walk(stream: &[u8]) -> Census {
    let transmit_header = transmit_header(stream);
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
    let mut intersection_schema_anchor_seen = false;
    while offset + 4 <= stream.len() {
        intersection_schema_anchor_seen |=
            crate::topology::intersection_data_schema_header_at(stream, offset);
        if let Some(preamble) = schema_reference_preamble(stream, offset, stream.len()) {
            census.bytes_decoded += preamble.end - preamble.offset;
            offset = preamble.end;
            value_boundary = true;
            census.events.schema_reference_preambles.push(preamble);
            continue;
        }
        if let Some(declaration) = inline_schema_declaration(stream, offset, stream.len()) {
            census.bytes_decoded += declaration.end - declaration.offset;
            offset = declaration.end;
            value_boundary = true;
            census.events.inline_schema_declarations.push(declaration);
            continue;
        }
        if let Some(map) =
            reference_type_map(stream, offset, ReferenceTypeMapLimit::TargetTerminated)
        {
            census.bytes_decoded += map.end - map.offset;
            offset = map.end;
            value_boundary = true;
            census.events.reference_type_maps.push(map);
            continue;
        }
        let complete_record = consume_shared_record(
            stream,
            offset,
            &census.records,
            intersection_schema_anchor_seen,
        )
        .or_else(|| consume_intersection_auxiliary(stream, offset))
        .or_else(|| consume_nurbs_auxiliary(stream, offset))
        .or_else(|| consume_type_141(stream, offset))
        .or_else(|| consume_type_45(stream, offset))
        .or_else(|| consume_type_67(stream, offset))
        .or_else(|| consume_type_70(stream, offset))
        .or_else(|| consume_attdef_list(stream, offset))
        .or_else(|| consume_type_101(stream, offset))
        .or_else(|| consume_intersection_data(stream, offset, intersection_schema_anchor_seen));
        if let Some(record) = complete_record {
            census.bytes_decoded += record.end - offset;
            offset = record.end;
            value_boundary = true;
            census.events.records.push(record);
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
                census.events.body_revisions.push(revision);
                continue;
            }
        }
        let value_owned = !is_value_family(kind)
            || value_boundary
            || referenced_value_offsets
                .get_or_insert_with(|| {
                    crate::parasolid::referenced_value_event_offsets(stream)
                        .into_iter()
                        .collect()
                })
                .contains(&offset);
        if !value_owned {
            if let Some((parsed_kind, _, byte_len)) =
                crate::parasolid::value_records::entity_value_record_identity_at(stream, offset)
            {
                if parsed_kind == kind {
                    offset += byte_len;
                    continue;
                }
            }
        }
        let decoded = fixed_signature(kind)
            .and_then(|signature| consume_fixed(stream, offset, kind, signature))
            .or_else(|| {
                value_owned
                    .then(|| consume_variable(stream, offset, kind))
                    .flatten()
            });
        if let Some(record) = decoded {
            census.bytes_decoded += record.end - record.offset;
            offset = record.end;
            value_boundary = true;
            census.events.records.push(record);
            continue;
        }
        if let Some(xmt) = (kind != 98)
            .then(|| compact_tombstone(stream, offset))
            .flatten()
        {
            if xmt > 1 {
                census.events.tombstones.push(Tombstone {
                    kind: record_kind,
                    xmt,
                    offset,
                });
                census.bytes_decoded += 6;
                offset += 6;
                value_boundary = true;
                continue;
            }
        }
        offset += 1;
        value_boundary = false;
    }
    census.events.term_use_numeric_tails = term_use_numeric_tails(stream, &census);
    census.bytes_decoded += census
        .term_use_numeric_tails
        .iter()
        .map(|tail| tail.values().byte_len())
        .sum::<usize>();
    census.bytes_decoded += populate_gap_events(stream, &mut census);
    let body_revision_state_bytes = populate_body_revision_state_tails(stream, &mut census);
    census.bytes_decoded += body_revision_state_bytes;
    census
}

fn populate_gap_events(stream: &[u8], census: &mut Census) -> usize {
    let mut admitted_bytes = 0;
    loop {
        let covered_before = merged_event_spans(census, true)
            .into_iter()
            .map(|(start, end)| end - start)
            .sum::<usize>();

        let lanes = tagged_reference_lanes(stream, census);
        census.events.tagged_reference_lanes.extend(lanes);

        let maps = reference_type_maps(stream, census);
        census.events.reference_type_maps.extend(maps);

        let state_packets = reference_state_packets(stream, census);
        census.events.reference_state_packets.extend(state_packets);

        let preambles = schema_reference_preambles(stream, census);
        census.events.schema_reference_preambles.extend(preambles);

        let declarations = inline_schema_declarations(stream, census);
        census
            .events
            .inline_schema_declarations
            .extend(declarations);

        let body_states = inline_body_states(stream, census);
        census.events.inline_body_states.extend(body_states);

        let marker_packets = reference_marker_packets(stream, census);
        census
            .events
            .reference_marker_packets
            .extend(marker_packets);

        let type_150_packets = type_150_state_packets(stream, census);
        census
            .events
            .type_150_state_packets
            .extend(type_150_packets);

        let covered_after = merged_event_spans(census, true)
            .into_iter()
            .map(|(start, end)| end - start)
            .sum::<usize>();
        let added_bytes = covered_after - covered_before;
        admitted_bytes += added_bytes;
        if added_bytes == 0 {
            break;
        }
    }

    census
        .events
        .tagged_reference_lanes
        .sort_unstable_by_key(|lane| lane.offset);
    census
        .events
        .reference_type_maps
        .sort_unstable_by_key(|map| map.offset);
    census
        .events
        .reference_state_packets
        .sort_unstable_by_key(|packet| packet.offset);
    census
        .events
        .schema_reference_preambles
        .sort_unstable_by_key(|preamble| preamble.offset);
    census
        .events
        .inline_schema_declarations
        .sort_unstable_by_key(|declaration| declaration.offset);
    census
        .events
        .inline_body_states
        .sort_unstable_by_key(|state| state.offset);
    census
        .events
        .reference_marker_packets
        .sort_unstable_by_key(|packet| packet.offset);
    census
        .events
        .type_150_state_packets
        .sort_unstable_by_key(|packet| packet.offset);
    admitted_bytes
}

fn populate_body_revision_state_tails(stream: &[u8], census: &mut Census) -> usize {
    let tails = uncovered_spans(stream.len(), census, true)
        .filter_map(|(start, end)| {
            census
                .body_revisions
                .iter()
                .position(|revision| revision.prefix_end == start)
                .map(|index| (index, start, end))
        })
        .collect::<Vec<_>>();
    let mut byte_len = 0;
    for (index, start, end) in tails {
        let revision = &mut census.events.body_revisions[index];
        revision.end = end;
        byte_len += end - start;
    }
    byte_len
}
