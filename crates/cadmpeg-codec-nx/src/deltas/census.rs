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
        let value_owned = value_is_owned(
            ctx,
            kind,
            offset,
            value_boundary,
            referenced_value_offsets.as_ref(),
        )?;
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
    let mut covered = merged_event_spans(ctx, census, true)?;
    loop {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(covered.len()),
            "count NX deltas covered bytes",
        )?;
        let covered_before = covered
            .iter()
            .copied()
            .map(|(start, end)| end - start)
            .sum::<usize>();

        let lanes = tagged_reference_lanes(ctx, stream, &covered)?;
        extend_covered_spans(
            ctx,
            &mut covered,
            lanes.iter().map(|event| (event.offset, event.end)),
        )?;
        ctx.extend_vec(
            &mut census.events.tagged_reference_lanes,
            lanes,
            "NX tagged reference lanes",
        )?;

        let maps = reference_type_maps(ctx, stream, census, &covered)?;
        extend_covered_spans(
            ctx,
            &mut covered,
            maps.iter().map(|event| (event.offset, event.end)),
        )?;
        ctx.extend_vec(
            &mut census.events.reference_type_maps,
            maps,
            "NX reference type maps",
        )?;

        let state_packets = reference_state_packets(ctx, stream, &covered)?;
        extend_covered_spans(
            ctx,
            &mut covered,
            state_packets.iter().map(|event| (event.offset, event.end)),
        )?;
        ctx.extend_vec(
            &mut census.events.reference_state_packets,
            state_packets,
            "NX reference state packets",
        )?;

        let preambles = schema_reference_preambles(ctx, stream, &covered)?;
        extend_covered_spans(
            ctx,
            &mut covered,
            preambles.iter().map(|event| (event.offset, event.end)),
        )?;
        ctx.extend_vec(
            &mut census.events.schema_reference_preambles,
            preambles,
            "NX schema reference preambles",
        )?;

        let declarations = inline_schema_declarations(ctx, stream, &covered)?;
        extend_covered_spans(
            ctx,
            &mut covered,
            declarations.iter().map(|event| (event.offset, event.end)),
        )?;
        ctx.extend_vec(
            &mut census.events.inline_schema_declarations,
            declarations,
            "NX inline schema declarations",
        )?;

        let body_states = inline_body_states(ctx, stream, census, &covered)?;
        extend_covered_spans(
            ctx,
            &mut covered,
            body_states.iter().map(|event| (event.offset, event.end)),
        )?;
        ctx.extend_vec(
            &mut census.events.inline_body_states,
            body_states,
            "NX inline body states",
        )?;

        let marker_packets = reference_marker_packets(ctx, stream, &covered)?;
        extend_covered_spans(
            ctx,
            &mut covered,
            marker_packets.iter().map(|event| (event.offset, event.end)),
        )?;
        ctx.extend_vec(
            &mut census.events.reference_marker_packets,
            marker_packets,
            "NX reference marker packets",
        )?;

        let type_150_packets = type_150_state_packets(ctx, stream, &covered)?;
        extend_covered_spans(
            ctx,
            &mut covered,
            type_150_packets
                .iter()
                .map(|event| (event.offset, event.end)),
        )?;
        ctx.extend_vec(
            &mut census.events.type_150_state_packets,
            type_150_packets,
            "NX type 150 state packets",
        )?;

        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(covered.len()),
            "count NX deltas covered bytes",
        )?;
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

    ctx.sort_unstable_by(
        &mut census.events.tagged_reference_lanes,
        |left, right| left.offset.cmp(&right.offset),
        |_| 0,
        "NX deltas tagged reference lanes",
    )?;
    ctx.sort_unstable_by(
        &mut census.events.reference_type_maps,
        |left, right| left.offset.cmp(&right.offset),
        |_| 0,
        "NX deltas reference type maps",
    )?;
    ctx.sort_unstable_by(
        &mut census.events.reference_state_packets,
        |left, right| left.offset.cmp(&right.offset),
        |_| 0,
        "NX deltas reference state packets",
    )?;
    ctx.sort_unstable_by(
        &mut census.events.schema_reference_preambles,
        |left, right| left.offset.cmp(&right.offset),
        |_| 0,
        "NX deltas schema reference preambles",
    )?;
    ctx.sort_unstable_by(
        &mut census.events.inline_schema_declarations,
        |left, right| left.offset.cmp(&right.offset),
        |_| 0,
        "NX deltas inline schema declarations",
    )?;
    ctx.sort_unstable_by(
        &mut census.events.inline_body_states,
        |left, right| left.offset.cmp(&right.offset),
        |_| 0,
        "NX deltas inline body states",
    )?;
    ctx.sort_unstable_by(
        &mut census.events.reference_marker_packets,
        |left, right| left.offset.cmp(&right.offset),
        |_| 0,
        "NX deltas reference marker packets",
    )?;
    ctx.sort_unstable_by(
        &mut census.events.type_150_state_packets,
        |left, right| left.offset.cmp(&right.offset),
        |_| 0,
        "NX deltas type 150 state packets",
    )?;
    Ok(admitted_bytes)
}

fn value_is_owned(
    ctx: &DecodeContext<'_>,
    kind: u16,
    offset: usize,
    value_boundary: bool,
    referenced_offsets: Option<&BTreeSet<usize>>,
) -> Result<bool, CodecError> {
    ctx.charge_work(0, "resolve NX referenced offset")?;
    if !is_value_family(kind) || value_boundary {
        return Ok(true);
    }
    let Some(offsets) = referenced_offsets else {
        return Ok(false);
    };
    // Twelve comparisons per binary level bound the eleven-key B-tree nodes.
    let comparisons = 12 * u64::from(usize::BITS - offsets.len().leading_zeros()) + 1;
    ctx.charge_work(comparisons, "resolve NX referenced offset")?;
    Ok(offsets.contains(&offset))
}

fn populate_body_revision_state_tails(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    census: &mut Census,
) -> Result<usize, CodecError> {
    ctx.charge_work(0, "NX body revision state tail boundary")?;
    if census.body_revisions.is_empty() {
        return Ok(0);
    }
    let mut byte_len = 0;
    let covered = merged_event_spans(ctx, census, true)?;
    for (start, end) in uncovered_spans(ctx, stream.len(), &covered)? {
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

/// Merge newly admitted spans into an already sorted, disjoint coverage union.
/// Only the new batch needs sorting; existing record spans remain indexed.
fn extend_covered_spans(
    ctx: &DecodeContext<'_>,
    covered: &mut Vec<(usize, usize)>,
    spans: impl Iterator<Item = (usize, usize)>,
) -> Result<(), CodecError> {
    ctx.charge_work(0, "merge NX deltas coverage")?;
    let mut added = Vec::new();
    for span in spans {
        ctx.push_vec(&mut added, span, "NX added deltas spans")?;
    }
    if added.is_empty() {
        return Ok(());
    }
    ctx.sort_unstable_by(&mut added, Ord::cmp, |_| 0, "sort NX added deltas spans")?;
    let capacity = covered.len().checked_add(added.len()).ok_or_else(|| {
        ctx.refuse_codec_limit("NX merged deltas coverage", u64::MAX - 1, u64::MAX)
    })?;
    let mut merged = ctx.collection_vec(capacity, "NX merged deltas coverage")?;
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
        ctx.charge_work(1, "merge NX deltas coverage")?;
        if let Some((_, previous_end)) = merged.last_mut().filter(|(_, end)| start <= *end) {
            *previous_end = (*previous_end).max(end);
        } else {
            merged.push((start, end));
        }
    }
    *covered = merged;
    Ok(())
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
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units = 0;
                policy.limits.max_collection_items = 0;
            },
            |ctx| {
                assert_eq!(
                    super::populate_body_revision_state_tails(ctx, &[], &mut census)
                        .expect("no BODY revision needs a state tail"),
                    0
                );
                assert!(ctx.resource_refusal().is_none());
            },
        );
        assert_eq!(census, before);
    }

    #[test]
    fn empty_body_revision_completion_preserves_the_first_refusal() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 0,
            |ctx| {
                ctx.charge_work(1, "body revision boundary test")
                    .expect_err("work refusal");
                let first = ctx.resource_refusal().expect("first refusal");
                let mut census = super::Census {
                    events: super::CensusEvents::default(),
                    bytes_decoded: 0,
                };
                assert!(matches!(
                    super::populate_body_revision_state_tails(ctx, &[], &mut census),
                    Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == first
                ));
            },
        );
    }

    #[test]
    fn referenced_value_membership_uses_the_offset_index() {
        let offsets = (0..10_000).map(|index| 4 * index).collect::<BTreeSet<_>>();
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 400,
            |ctx| {
                assert!(value_is_owned(ctx, 98, 32, false, Some(&offsets)).unwrap());
                assert!(!value_is_owned(ctx, 98, 33, false, Some(&offsets)).unwrap());
                assert!(ctx.resource_refusal().is_none());
            },
        );
    }

    #[test]
    fn known_value_boundaries_and_nonvalues_do_not_search_references() {
        let offsets = BTreeSet::from([32]);
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 0,
            |ctx| {
                assert!(value_is_owned(ctx, 14, 33, false, Some(&offsets)).unwrap());
                assert!(value_is_owned(ctx, 98, 33, true, Some(&offsets)).unwrap());
            },
        );
    }

    #[test]
    fn referenced_value_membership_preserves_work_refusals() {
        let offsets = BTreeSet::from([32]);
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 0,
            |ctx| {
                let error = value_is_owned(ctx, 98, 32, false, Some(&offsets))
                    .expect_err("indexed membership remains charged");
                assert!(matches!(
                    error,
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                        if limit.operation == "resolve NX referenced offset"
                            && ctx.resource_refusal() == Some(limit)
                ));
            },
        );
    }

    #[test]
    fn coverage_merges_overlaps_adjacency_and_duplicates() {
        crate::test_support::with_decode_context(|ctx| {
            let mut covered = vec![(1, 3), (8, 10)];
            extend_covered_spans(
                ctx,
                &mut covered,
                [(10, 12), (4, 8), (2, 5), (2, 5)].into_iter(),
            )
            .expect("admitted coverage merge");
            assert_eq!(covered, [(1, 12)]);
        });
    }

    #[test]
    fn adding_a_span_does_not_sort_existing_coverage_again() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 10_000,
            |ctx| {
                let mut covered = (0..1000).map(|index| (4 * index, 4 * index + 1)).collect();
                extend_covered_spans(ctx, &mut covered, [(2, 3)].into_iter())
                    .expect("one sorted addition and a linear merge");
                assert_eq!(covered.len(), 1001);
                assert_eq!(&covered[..3], [(0, 1), (2, 3), (4, 5)]);
                assert_eq!(covered.last(), Some(&(3996, 3997)));
                assert!(ctx.resource_refusal().is_none());
            },
        );
    }
}
