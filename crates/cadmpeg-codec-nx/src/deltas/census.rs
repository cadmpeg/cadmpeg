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
use cadmpeg_core::decode::View;
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
    pub(crate) fn full_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for record in &self.records {
            *counts.entry(record.family_name()).or_default() += 1;
        }
        counts
    }

    /// Compact tombstone counts keyed by Parasolid family name.
    pub(crate) fn tombstone_counts(&self) -> BTreeMap<&'static str, usize> {
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
pub(crate) fn walk(stream: &[u8]) -> Result<Census, CodecError> {
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
        let shared_record = consume_shared_record(
            stream,
            offset,
            &census.records,
            intersection_schema_anchor_seen,
        )?;
        let complete_record = if shared_record.is_some() {
            shared_record
        } else {
            consume_intersection_auxiliary(stream, offset)
        };
        let complete_record = if complete_record.is_some() {
            complete_record
        } else {
            first_complete_record(stream, offset, intersection_schema_anchor_seen, true)
        };
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
        if is_value_family(kind) && !value_boundary && referenced_value_offsets.is_none() {
            referenced_value_offsets = Some(
                crate::parasolid::referenced_value_event_offsets(stream)
                    .into_iter()
                    .collect(),
            );
        }
        let value_owned = value_is_owned(
            kind,
            offset,
            value_boundary,
            referenced_value_offsets.as_ref(),
        );
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
        let fixed = if let Some(signature) = fixed_signature(kind) {
            consume_fixed(stream, offset, kind, signature)?
        } else {
            None
        };
        let decoded = if fixed.is_some() || !value_owned {
            fixed
        } else {
            consume_variable(stream, offset, kind)
        };
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
    Ok(census)
}

fn populate_gap_events(stream: &[u8], census: &mut Census) -> usize {
    let mut admitted_bytes = 0;
    let mut covered = merged_event_spans(census, true);
    loop {
        let covered_before = covered
            .iter()
            .copied()
            .map(|(start, end)| end - start)
            .sum::<usize>();

        let lanes = tagged_reference_lanes(stream, &covered);
        extend_covered_spans(
            &mut covered,
            lanes.iter().map(|event| (event.offset, event.end)),
        );
        census.events.tagged_reference_lanes.extend(lanes);

        let maps = reference_type_maps(stream, census, &covered);
        extend_covered_spans(
            &mut covered,
            maps.iter().map(|event| (event.offset, event.end)),
        );
        census.events.reference_type_maps.extend(maps);

        let state_packets = reference_state_packets(stream, &covered);
        extend_covered_spans(
            &mut covered,
            state_packets.iter().map(|event| (event.offset, event.end)),
        );
        census.events.reference_state_packets.extend(state_packets);

        let preambles = schema_reference_preambles(stream, &covered);
        extend_covered_spans(
            &mut covered,
            preambles.iter().map(|event| (event.offset, event.end)),
        );
        census.events.schema_reference_preambles.extend(preambles);

        let declarations = inline_schema_declarations(stream, &covered);
        extend_covered_spans(
            &mut covered,
            declarations.iter().map(|event| (event.offset, event.end)),
        );
        census
            .events
            .inline_schema_declarations
            .extend(declarations);

        let body_states = inline_body_states(stream, census, &covered);
        extend_covered_spans(
            &mut covered,
            body_states.iter().map(|event| (event.offset, event.end)),
        );
        census.events.inline_body_states.extend(body_states);

        let marker_packets = reference_marker_packets(stream, &covered);
        extend_covered_spans(
            &mut covered,
            marker_packets.iter().map(|event| (event.offset, event.end)),
        );
        census
            .events
            .reference_marker_packets
            .extend(marker_packets);

        let type_150_packets = type_150_state_packets(stream, &covered);
        extend_covered_spans(
            &mut covered,
            type_150_packets
                .iter()
                .map(|event| (event.offset, event.end)),
        );
        census
            .events
            .type_150_state_packets
            .extend(type_150_packets);

        let covered_after = covered
            .iter()
            .copied()
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
        .sort_unstable_by_key(|event| event.offset);
    census
        .events
        .reference_type_maps
        .sort_unstable_by_key(|event| event.offset);
    census
        .events
        .reference_state_packets
        .sort_unstable_by_key(|event| event.offset);
    census
        .events
        .schema_reference_preambles
        .sort_unstable_by_key(|event| event.offset);
    census
        .events
        .inline_schema_declarations
        .sort_unstable_by_key(|event| event.offset);
    census
        .events
        .inline_body_states
        .sort_unstable_by_key(|event| event.offset);
    census
        .events
        .reference_marker_packets
        .sort_unstable_by_key(|event| event.offset);
    census
        .events
        .type_150_state_packets
        .sort_unstable_by_key(|event| event.offset);
    admitted_bytes
}

fn value_is_owned(
    kind: u16,
    offset: usize,
    value_boundary: bool,
    referenced_offsets: Option<&BTreeSet<usize>>,
) -> bool {
    if !is_value_family(kind) || value_boundary {
        return true;
    }
    referenced_offsets.is_some_and(|offsets| offsets.contains(&offset))
}

fn populate_body_revision_state_tails(stream: &[u8], census: &mut Census) -> usize {
    if census.body_revisions.is_empty() {
        return 0;
    }
    let mut byte_len = 0;
    let covered = merged_event_spans(census, true);
    for (start, end) in uncovered_spans(stream.len(), &covered) {
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
    byte_len
}

/// Merge newly admitted spans into an already sorted, disjoint coverage union.
/// Only the new batch needs sorting; existing record spans remain indexed.
fn extend_covered_spans(
    covered: &mut Vec<(usize, usize)>,
    spans: impl Iterator<Item = (usize, usize)>,
) {
    let mut added = spans.collect::<Vec<_>>();
    if added.is_empty() {
        return;
    }
    added.sort_unstable_by(Ord::cmp);
    let mut merged = Vec::new();
    let mut previous = covered.iter().copied().peekable();
    let mut additions = added.into_iter().peekable();
    loop {
        let span = match (previous.peek(), additions.peek()) {
            (Some(left), Some(right)) if left <= right => previous.next(),
            (_, Some(_)) => additions.next(),
            (Some(_), None) => previous.next(),
            (None, None) => break,
        };
        let Some((start, end)) = span else {
            break;
        };
        if let Some((_, previous_end)) = merged.last_mut().filter(|(_, end)| start <= *end) {
            *previous_end = (*previous_end).max(end);
        } else {
            merged.push((start, end));
        }
    }
    *covered = merged;
}

#[cfg(test)]
mod tests {
    use super::{extend_covered_spans, value_is_owned};
    use std::collections::BTreeSet;

    #[test]
    fn no_body_revisions_do_not_build_unrelated_event_coverage() {
        let mut census = super::Census {
            events: super::CensusEvents::default(),
            bytes_decoded: 6000,
        };
        for index in 0..1000 {
            census.events.tombstones.push(super::Tombstone {
                kind: super::RecordKind::try_from(12).expect("BODY kind"),
                xmt: u32::try_from(index + 2).expect("identity"),
                offset: 6 * index,
            });
        }
        let before = census.clone();
        assert_eq!(
            super::populate_body_revision_state_tails(&[], &mut census),
            0
        );
        assert_eq!(census, before);
    }

    #[test]
    fn referenced_value_membership_uses_the_offset_index() {
        let offsets = (0..10_000).map(|index| 4 * index).collect::<BTreeSet<_>>();
        assert!(value_is_owned(98, 32, false, Some(&offsets)));
        assert!(!value_is_owned(98, 33, false, Some(&offsets)));
    }

    #[test]
    fn known_value_boundaries_and_nonvalues_do_not_search_references() {
        let offsets = BTreeSet::from([32]);
        assert!(value_is_owned(14, 33, false, Some(&offsets)));
        assert!(value_is_owned(98, 33, true, Some(&offsets)));
    }

    #[test]
    fn coverage_merges_overlaps_adjacency_and_duplicates() {
        let mut covered = vec![(1, 3), (8, 10)];
        extend_covered_spans(&mut covered, [(10, 12), (4, 8), (2, 5), (2, 5)].into_iter());
        assert_eq!(covered, [(1, 12)]);
    }

    #[test]
    fn adding_a_span_does_not_sort_existing_coverage_again() {
        let mut covered = (0..1000).map(|index| (4 * index, 4 * index + 1)).collect();
        extend_covered_spans(&mut covered, [(2, 3)].into_iter());
        assert_eq!(covered.len(), 1001);
        assert_eq!(&covered[..3], [(0, 1), (2, 3), (4, 5)]);
        assert_eq!(covered.last(), Some(&(3996, 3997)));
    }
}
