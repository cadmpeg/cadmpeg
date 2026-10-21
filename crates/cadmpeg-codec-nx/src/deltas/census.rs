// SPDX-License-Identifier: Apache-2.0
//! Admitted deltas events and their source-byte census.

use super::record_kind::RecordKind;
use super::tails::TerminalNullReferences;
use super::{
    body_revision_prefix, compact_tombstone, consume_fixed, consume_intersection_auxiliary,
    consume_shared_record, consume_variable, first_complete_record, fixed_signature,
    inline_schema_node, is_value_family, merged_event_spans, reference_marker_packets,
    reference_type_map, reference_type_maps, tagged_reference_lanes, transmit_header,
    uncovered_spans, BodyRevision, PartitionPrefix, PartitionPrefixState, PmarkPacket, Record,
    ReferenceMarkerPacket, ReferenceTypeMap, ReferenceTypeMapLimit, SchemaNodeEvent,
    TaggedReferenceLane, Tombstone, TransmitHeader,
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
    /// Maximal event gaps composed entirely of typed stream-local references.
    pub(crate) tagged_reference_lanes: Vec<TaggedReferenceLane>,
    /// Complete framed reference/type maps in source order.
    pub(crate) reference_type_maps: Vec<ReferenceTypeMap>,
    /// Partition-owned PMARK packets in source order.
    pub(crate) pmark_packets: Vec<PmarkPacket>,
    /// Leading ordinary PARTITION and current PMARK nodes.
    pub(crate) partition_prefix: Option<PartitionPrefix>,
    /// Complete reference-marker packets in source order.
    pub(crate) reference_marker_packets: Vec<ReferenceMarkerPacket>,
    /// Complete inline schema declarations in source order.
    pub(crate) schema_nodes: Vec<SchemaNodeEvent>,
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
        merged_event_spans(self)
    }
}

/// Walk all accepted records, revisions, tombstones, and null trailers in an
/// inflated deltas stream.
pub(crate) fn walk(stream: &[u8]) -> Result<Census, CodecError> {
    let transmit_header = transmit_header(stream);
    let header_byte_len = transmit_header.as_ref().map_or(0, |header| header.end);
    let partition_prefix = transmit_header.as_ref().and_then(|header| {
        let schema = header.state.schema();
        (schema.split('_').count() == 4
            && schema.ends_with("_13006")
            && header.state.maximum_node_type() >= 4
            && header.state.user_field_size() == 0)
            .then_some(())?;
        let (state, end) = PartitionPrefixState::read(stream, header.end)?;
        Some(PartitionPrefix {
            state,
            offset: header.end,
            end,
        })
    });
    let prefix_byte_len = partition_prefix
        .as_ref()
        .map_or(0, |prefix| prefix.end - prefix.offset);
    let mut census = Census {
        events: CensusEvents {
            transmit_header,
            partition_prefix,
            ..CensusEvents::default()
        },
        bytes_decoded: header_byte_len + prefix_byte_len,
    };
    let mut offset = census
        .partition_prefix
        .as_ref()
        .map_or(header_byte_len, |prefix| prefix.end);
    let mut value_boundary = true;
    let mut referenced_value_offsets = None::<BTreeSet<usize>>;

    let mut world_layout = None;
    let mut schema_seen = [false; 102];
    let supported_base = census
        .transmit_header
        .as_ref()
        .is_none_or(|header| header.state.schema().ends_with("_13006"));
    let mut intersection_layout = if !supported_base {
        super::intersection::IntersectionLayout::Unsupported
    } else if census.transmit_header.is_some() {
        super::intersection::IntersectionLayout::Base
    } else {
        super::intersection::IntersectionLayout::WithData
    };
    while offset + 4 <= stream.len() {
        if let Some(prefix) = census.partition_prefix.as_ref() {
            if let Some(packet) = super::pmark_packets::at(stream, offset, prefix) {
                // The walk reaches this head only after complete preceding
                // records have consumed their entire counted storage.
                offset = packet.end;
                value_boundary = true;
                census.events.pmark_packets.push(packet);
                continue;
            }
        }
        let declaration = if supported_base {
            inline_schema_node(stream, offset, stream.len(), &mut schema_seen)?
        } else {
            None
        };
        if let Some(declaration) = declaration {
            if declaration.fields.kind() == 38 {
                intersection_layout = super::intersection::IntersectionLayout::from_schema(
                    declaration.fields.definition(),
                );
            }
            if declaration.fields.kind() == 101 {
                world_layout = super::WorldLayout::from_schema(declaration.fields.definition());
            }
            census.bytes_decoded += declaration.end - declaration.offset;
            offset = declaration.end;
            value_boundary = true;
            census.events.schema_nodes.push(declaration);
            continue;
        }
        if let Some(map) =
            reference_type_map(stream, offset, ReferenceTypeMapLimit::WithTerminalWord)
        {
            census.bytes_decoded += map.end - map.offset;
            offset = map.end;
            value_boundary = true;
            census.events.reference_type_maps.push(map);
            continue;
        }
        let shared_head = census.records.last().and_then(|previous| {
            (previous.end == offset && super::has_shareable_terminal(stream, previous))
                .then(|| offset.checked_sub(1))
                .flatten()
        });
        let ambiguous_prefix_end = shared_head
            .into_iter()
            .chain(std::iter::once(offset))
            .find_map(|head| {
                let kind = View::u16_be_at(stream, head)?;
                let signature = fixed_signature(kind, intersection_layout)?;
                match super::fixed_envelope(stream, head, kind, signature) {
                    super::FixedEnvelope::Ambiguous { prefix_end } => Some(prefix_end),
                    super::FixedEnvelope::Absent | super::FixedEnvelope::Unique(_) => None,
                }
            });
        if let Some(prefix_end) = ambiguous_prefix_end {
            // Both readings claim these bytes as fields. Their shared prefix
            // stays opaque; scanning inside it would invent new record heads.
            // The shorter extent does not select either reading's identity.
            offset = prefix_end;
            value_boundary = false;
            continue;
        }
        let shared_record = consume_shared_record(
            stream,
            offset,
            &census.records,
            world_layout,
            intersection_layout,
        )?;
        let complete_record = if shared_record.is_some() {
            shared_record
        } else {
            consume_intersection_auxiliary(stream, offset)
        };
        let complete_record = if complete_record.is_some() {
            complete_record
        } else {
            first_complete_record(stream, offset, true, world_layout)
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
        let fixed = if let Some(signature) = fixed_signature(kind, intersection_layout) {
            consume_fixed(stream, offset, kind, signature, world_layout)?
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
    if let Some(prefix) = census.partition_prefix.as_ref() {
        if !super::pmark_packets::admit(&census.pmark_packets, prefix.state) {
            census.events.pmark_packets.clear();
        }
    }
    census.bytes_decoded += census
        .pmark_packets
        .iter()
        .map(|packet| packet.end - packet.offset)
        .sum::<usize>();
    // A PMARK scalar with null-reference-shaped bytes remains scalar storage.
    // Existing record reference tails can still share the stream trailer.
    census.events.terminal_null_references =
        TerminalNullReferences::at_end(stream).filter(|tail| {
            census
                .partition_prefix
                .as_ref()
                .is_none_or(|prefix| prefix.end <= tail.offset())
                && census
                    .pmark_packets
                    .iter()
                    .all(|packet| packet.end <= tail.offset() || packet.offset >= tail.end())
        });
    census.bytes_decoded += census
        .terminal_null_references
        .as_ref()
        .map_or(0, |tail| tail.end() - tail.offset());
    census.bytes_decoded += populate_gap_events(stream, &mut census, supported_base)?;
    let body_revision_state_bytes = populate_body_revision_state_tails(stream, &mut census);
    census.bytes_decoded += body_revision_state_bytes;
    Ok(census)
}

fn populate_gap_events(
    stream: &[u8],
    census: &mut Census,
    supported_base: bool,
) -> Result<usize, CodecError> {
    let mut admitted_bytes = 0;
    let mut covered = merged_event_spans(census);
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

        let declarations = if supported_base {
            super::inline_schema_nodes(stream, &covered, &census.schema_nodes)?
        } else {
            Vec::new()
        };
        extend_covered_spans(
            &mut covered,
            declarations.iter().map(|event| (event.offset, event.end)),
        );
        census.events.schema_nodes.extend(declarations);

        let marker_packets = reference_marker_packets(stream, &covered);
        extend_covered_spans(
            &mut covered,
            marker_packets.iter().map(|event| (event.offset, event.end)),
        );
        census
            .events
            .reference_marker_packets
            .extend(marker_packets);

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
        .schema_nodes
        .sort_unstable_by_key(|left| left.offset);
    census
        .events
        .reference_marker_packets
        .sort_unstable_by_key(|left| left.offset);
    Ok(admitted_bytes)
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
    let covered = merged_event_spans(census);
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
