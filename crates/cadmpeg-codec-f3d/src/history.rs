// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(
    test,
    allow(clippy::cloned_ref_to_slice_refs, clippy::default_trait_access)
)]
//! Decode the ASM construction-history partition after the active model slice.
//!
//! [`decode`] reads `delta_state` headers, bulletin-board entity changes, and
//! history records while retaining source bytes for records without typed
//! semantics.

use cadmpeg_core::decode::u64_from_index;

pub(crate) mod selection;

use crate::bytes::int_at;
use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmEntityVersion,
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder, AsmHistoricalEdge,
    AsmHistoricalEntityDelta, AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint,
    AsmHistoricalRelation, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory, AsmHistoryRecord, AsmPreamble,
};
use crate::records::feature::base_feature::DesignBaseFeatureBodyReferenceSource;
use crate::records::topology::{
    body_recipe::AsmHistoricalEntityKind, extrude_selection::DesignOperandRole,
};
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const EPS_HISTORY_HEM_GAP_LENGTH_FORM_E7: f64 = 1.0e-7;
const EPS_HISTORY_HEM_DIRECTION_FROM_TRANSITION_E7: f64 = 1.0e-7;
const EPS_HISTORY_PARALLEL_DIRECTIONS_E7: f64 = 1.0e-7;
const EPS_HISTORY_RESOLVE_THREAD_FACE_BY_TRANSITION_E9: f64 = 1.0e-9;
const EPS_HISTORY_CYCLIC_POINT_SUBSEQUENCE_E12: f64 = 1.0e-12;
const EPS_HISTORY_SAME_AXIS_LINE_E9: f64 = 1.0e-9;
const EPS_HISTORY_SAME_AXIS_LINE_E8: f64 = 1.0e-8;
const EPS_HISTORY_BIND_MIRROR_SELECTION_PLANES_E9: f64 = 1.0e-9;
const EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E9: f64 = 1.0e-9;
const EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E8: f64 = 1.0e-8;
const EPS_HISTORY_HISTORICAL_LOOP_PLANE_E9: f64 = 1.0e-9;

const DELTA: &[u8] = b"\x11\x0d\x0bdelta_state";
const PREAMBLE: &[u8] = b"\x0d\x0ehistory_stream";
/// Relative tolerance for matching independently decoded millimetre point carriers.
const WORK_POINT_POSITION_TOLERANCE: f64 = 1.0e-9;
/// Relative tolerance for admitting an oriented planar Hole support face.
const HOLE_SUPPORT_NORMAL_TOLERANCE: f64 = 1.0e-9;
/// Relative tolerance for the Hole point lying on its support plane.
const HOLE_SUPPORT_POINT_TOLERANCE: f64 = 1.0e-8;

pub(crate) fn graph_is_coherent(history: &AsmHistory) -> bool {
    let decode_arena = cadmpeg_core::decode::DecodeArena::new();
    let Ok((decode_ctx, _)) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &decode_arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    ) else {
        return false;
    };
    let decode_ctx = &decode_ctx;

    graph_is_coherent_inner(decode_ctx, history).unwrap_or(false)
}

pub(crate) fn graph_is_coherent_charged(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &AsmHistory,
) -> Result<bool, cadmpeg_core::CodecError> {
    graph_is_coherent_inner(decode, history)
}

fn graph_is_coherent_inner(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &AsmHistory,
) -> Result<bool, cadmpeg_core::CodecError> {
    if history.states.is_empty() {
        return Ok(false);
    }
    let mut by_index_storage = decode.reserve_scoped(0, "index F3D ASM history states")?;
    let mut by_index = HashMap::new();
    for state in decode.admit_iter(&history.states, "scan F3D ASM history states")? {
        by_index_storage.with_storage(|| {
            decode
                .insert_hash_map(
                    &mut by_index,
                    state.node_index,
                    state,
                    "index F3D ASM history states",
                )
                .map(|_| ())
        })?;
    }
    if by_index.len() != history.states.len()
        || decode.any_by(
            &history.states,
            |state| {
                if state.node_index < 0 {
                    return Ok(true);
                }
                Ok(!decode.equal(
                    &state.parent,
                    &history.id,
                    "compare F3D ASM history state parent",
                )?)
            },
            "scan F3D ASM history state parents",
        )?
    {
        return Ok(false);
    }
    let mut heads = history
        .states
        .iter()
        .filter(|state| state.previous_ref.is_none());
    let head = heads.next();
    let tails = decode
        .admit_iter(&history.states, "count F3D ASM history tail states")?
        .filter(|state| state.next_ref.is_none())
        .count();
    if head.is_none() || heads.next().is_some() || tails != 1 {
        return Ok(false);
    }
    let Some(head) = head else {
        return Ok(false);
    };
    if let Some(preamble) = history.preamble {
        if head.state_id != preamble.stream_size || preamble.history_entry_count < 0 {
            return Ok(false);
        }
    }
    let mut visited = HashSet::new();
    let mut previous = None;
    let mut current = Some(head.node_index);
    while let Some(index) = current {
        decode.charge_work(1, "visit F3D ASM history chain link")?;
        let Some(state) = by_index.get(&index) else {
            return Ok(false);
        };
        if visited.contains(&index) || state.previous_ref != previous {
            return Ok(false);
        }
        {
            decode.reserve_set(&mut visited, 1, "visit F3D ASM history state")?;
        }
        visited.insert(index);
        if state.version_flag != 1 || state.state_flag != 0 {
            return Ok(false);
        }
        for board in decode.admit_iter(
            &state.bulletin_boards,
            "scan F3D ASM history bulletin boards",
        )? {
            if !decode.equal(
                &board.parent,
                &state.id,
                "compare F3D ASM history board parent",
            )? || decode.any_by(
                &board.changes,
                |change| {
                    decode
                        .equal(
                            &change.parent,
                            &board.id,
                            "compare F3D ASM history change parent",
                        )
                        .map(|matches| !matches)
                },
                "scan F3D ASM history board changes",
            )? {
                return Ok(false);
            }
        }
        if decode.any_by(
            &state.records,
            |record| {
                let parent_matches = decode.equal(
                    &record.parent,
                    &state.id,
                    "compare F3D ASM history record parent",
                )?;
                Ok(!parent_matches || record.raw_bytes.is_empty())
            },
            "scan F3D ASM history records",
        )? {
            return Ok(false);
        }
        previous = Some(index);
        current = state.next_ref;
    }
    Ok(visited.len() == history.states.len())
}

/// Decode the construction-history tail of an ASM stream: every `delta_state`
/// record ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#32-delta_state-records)) from `bytes`, each with its `BulletinBoard` chain of
/// per-entity insert/delete/update changes and the raw history-entity records
/// framed between it and the next `delta_state`. `stream` is the source ZIP
/// entry name, recorded in each decoded item's provenance. Returns `None` when
/// `bytes` carries no `delta_state` record (the stream is a construction
/// snapshot with no history tail) or a malformed history body. `width` is the
/// stream's integer/ref width (4 for `BinaryFile4`, 8 for `BinaryFile8`).
pub(crate) fn decode(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    stream: &str,
    width: RefWidth,
    limits: &cadmpeg_core::decode::ResourceLimits,
) -> Result<Option<AsmHistory>, cadmpeg_core::CodecError> {
    let preamble_offset = ctx.find_bytes(bytes, PREAMBLE, "find F3D ASM preamble")?;
    let history_offset = preamble_offset.unwrap_or(0);
    let history_id = crate::ids::native_scoped_id(
        ctx,
        stream,
        "asm-history",
        format_args!("{history_offset:010}"),
    )?;
    let mut delta_offsets = Vec::new();
    for offset in ctx.find_bytes_iter(bytes, DELTA, "find F3D ASM delta markers")? {
        ctx.reserve_vec(&mut delta_offsets, 1, "f3d history delta offsets")?;
        delta_offsets.push(offset);
    }
    let mut states = Vec::new();
    for (ordinal, &offset) in ctx
        .admit_iter(&delta_offsets, "scan F3D ASM delta offsets")?
        .enumerate()
    {
        let state_record_id = crate::ids::native_scoped_id(
            ctx,
            stream,
            "asm-delta-state",
            format_args!("{offset:010}"),
        )?;
        let mut position = offset + DELTA.len();
        let Some((
            state_id,
            version_flag,
            state_flag,
            previous,
            next,
            node_index,
            partner,
            owner_ref,
        )) = (|| {
            Some((
                take_int(bytes, &mut position, 0x04, width)?,
                take_int(bytes, &mut position, 0x04, width)?,
                take_int(bytes, &mut position, 0x04, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
            ))
        })()
        else {
            return Ok(None);
        };
        if bytes.get(position) != Some(&0x0b) {
            continue;
        }
        let Some((bulletin_boards, body_end)) = decode_bulletin_boards(
            ctx,
            bytes,
            position + 1,
            stream,
            offset,
            &state_record_id,
            width,
        )?
        else {
            return Ok(None);
        };
        let records = decode_history_records(
            ctx,
            bytes,
            body_end,
            delta_offsets.get(ordinal + 1).copied(),
            stream,
            &state_record_id,
            width,
        )?;

        ctx.reserve_vec(&mut states, 1, "admit F3D ASM delta state")?;
        let parent = ctx.copy_retained_text(&history_id, "copy F3D ASM history parent")?;
        states.push(AsmDeltaState {
            id: state_record_id,
            parent,
            byte_offset: u64_from_index(offset),
            state_id,
            version_flag,
            state_flag,
            previous_ref: (previous >= 0).then_some(previous),
            next_ref: (next >= 0).then_some(next),
            node_index,
            partner_ref: (partner >= 0).then_some(partner),
            owner_ref,
            bulletin_boards,
            records,
            entity_versions: Vec::new(),
            topology_cache: crate::history_records::AsmTopologyCache::Absent,
            transition: None,
        });
    }
    bind_snapshot_revision_ids(ctx, &mut states)?;
    bind_historical_entity_versions(ctx, &mut states)?;
    let record_table_binding_budget_exceeded =
        bind_complete_record_tables(ctx, &mut states, bytes, width, limits)?;
    if states.is_empty() {
        return Ok(None);
    }

    let preamble = preamble_offset
        .and_then(|offset| decode_preamble(bytes, offset + PREAMBLE.len(), width))
        .map(|(stream_size, history_entry_count)| AsmPreamble {
            stream_size,
            history_entry_count,
        });
    let offset = history_offset;
    Ok(Some(AsmHistory {
        id: history_id,
        byte_offset: u64_from_index(offset),
        preamble,
        record_table_binding_budget_exceeded,
        states,
    }))
}

fn bind_snapshot_revision_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut old_references = Vec::new();
    for state in ctx.admit_iter(&*states, "scan F3D snapshot reference states")? {
        for board in ctx.admit_iter(&state.bulletin_boards, "scan F3D snapshot reference boards")? {
            for change in ctx.admit_iter(&board.changes, "scan F3D snapshot reference changes")? {
                let Some(old_reference) = change.old_ref() else {
                    continue;
                };
                ctx.reserve_vec(&mut old_references, 1, "collect F3D ASM old references")?;
                old_references.push(old_reference);
            }
        }
    }
    ctx.sort_unstable_by(
        &mut old_references,
        |value| value,
        Ord::cmp,
        "sort F3D ASM old references",
    )?;
    let Some(&first) = old_references.first() else {
        return Ok(());
    };
    let count = i64::try_from(old_references.len())
        .map_err(|_| ctx.refuse_codec_limit("collect F3D ASM old references", 0, u64::MAX))?;
    let Some(end) = first.checked_add(count) else {
        return Ok(());
    };
    if old_references.iter().copied().ne(first..end) {
        return Ok(());
    }
    let mut snapshot_record_count = 0usize;
    for state in ctx.admit_iter(&*states, "scan F3D snapshot record states")? {
        for record in ctx.admit_iter(&state.records, "scan F3D snapshot records")? {
            if record.name() != "End-of-ASM-data" {
                snapshot_record_count = snapshot_record_count.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "count F3D snapshot records",
                        u64_from_index(usize::MAX - 1),
                        u64_from_index(snapshot_record_count),
                    )
                })?;
            }
        }
    }
    if snapshot_record_count != old_references.len() {
        return Ok(());
    }
    for (record, revision_id) in states
        .iter_mut()
        .flat_map(|state| &mut state.records)
        .filter(|record| record.name() != "End-of-ASM-data")
        .zip(old_references)
    {
        record.revision_id = Some(revision_id);
    }
    Ok(())
}

fn is_history_boundary_record(record: &AsmHistoryRecord) -> bool {
    matches!(
        record.name(),
        "End-of-ASM-History-Section" | "End-of-ASM-data"
    )
}

fn archived_active_record_count(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let mut archived = Vec::new();
    for state in ctx.admit_iter(states, "scan F3D archived history states")? {
        for record in ctx.admit_iter(&state.records, "scan F3D archived state records")? {
            let Some(revision_id) = record.revision_id else {
                continue;
            };
            ctx.reserve_vec(&mut archived, 1, "collect F3D archived revisions")?;
            archived.push(revision_id);
        }
    }
    ctx.sort_unstable_by(
        &mut archived,
        |value| value,
        Ord::cmp,
        "sort F3D archived revisions",
    )?;
    let Some(&active_count) = archived.first() else {
        return Ok(None);
    };
    let count = i64::try_from(archived.len())
        .map_err(|_| ctx.refuse_codec_limit("collect F3D archived revisions", 0, u64::MAX))?;
    let Some(end) = active_count.checked_add(count) else {
        return Ok(None);
    };
    if active_count <= 0 || archived.iter().copied().ne(active_count..end) {
        return Ok(None);
    }
    Ok(usize::try_from(active_count).ok())
}

/// Return the active `RecordTable` length for a history that has no archived
/// snapshot. Insert-only chains use the active records themselves as every
/// revision, so their bulletin-board references must cover every non-header
/// slot exactly once.
fn insert_only_active_record_count(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let mut has_boundary_record = false;
    for state in ctx.admit_iter(states, "scan F3D insert-only history states")? {
        for record in ctx.admit_iter(&state.records, "scan F3D insert-only state records")? {
            if record.revision_id.is_some() || !is_history_boundary_record(record) {
                return Ok(None);
            }
            has_boundary_record = true;
        }
    }
    if !has_boundary_record {
        return Ok(None);
    }
    let mut inserted = BTreeSet::new();
    for state in ctx.admit_iter(states, "scan F3D insert-only history states")? {
        for board in ctx.admit_iter(
            &state.bulletin_boards,
            "scan F3D insert-only bulletin boards",
        )? {
            for change in ctx.admit_iter(&board.changes, "scan F3D insert-only board changes")? {
                let Some(new_ref) = change.new_ref().filter(|_| change.old_ref().is_none()) else {
                    return Ok(None);
                };
                if new_ref <= 0 || inserted.contains(&new_ref) {
                    return Ok(None);
                }
                ctx.insert_btree_set(&mut inserted, new_ref, "index F3D insert-only revisions")?;
            }
        }
    }
    let Some(&last) = inserted.last() else {
        return Ok(None);
    };
    if inserted.iter().copied().ne(1..=last) {
        return Ok(None);
    }
    Ok(last
        .checked_add(1)
        .and_then(|count| usize::try_from(count).ok()))
}

fn bind_historical_entity_versions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut archived_ids = Vec::new();
    for state in ctx.admit_iter(&*states, "scan F3D archived revision states")? {
        for record in ctx.admit_iter(&state.records, "scan F3D archived revision records")? {
            let Some(revision_id) = record.revision_id else {
                continue;
            };
            ctx.reserve_vec(&mut archived_ids, 1, "index F3D archived revision IDs")?;
            archived_ids.push(revision_id);
        }
    }
    ctx.sort_unstable_by(
        &mut archived_ids,
        |value| value,
        Ord::cmp,
        "sort F3D archived revision IDs",
    )?;
    let active_count = match archived_active_record_count(ctx, states)? {
        Some(count) => Some(count),
        None => insert_only_active_record_count(ctx, states)?,
    }
    .and_then(|count| i64::try_from(count).ok());
    let Some(active_count) = active_count else {
        return Ok(());
    };
    let mut by_node_storage = ctx.reserve_scoped(0, "index F3D history node ordinals")?;
    let mut by_node = HashMap::new();
    for (ordinal, state) in ctx
        .admit_iter(&*states, "scan F3D history node ordinals")?
        .enumerate()
    {
        by_node_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut by_node,
                state.node_index,
                ordinal,
                "index F3D history node ordinals",
            )
            .map(|_| ())
        })?;
    }
    if by_node.len() != states.len() {
        return Ok(());
    }
    let mut heads = states
        .iter()
        .enumerate()
        .filter(|(_, state)| state.previous_ref.is_none())
        .map(|(ordinal, _)| ordinal);
    let Some(mut ordinal) = heads.next() else {
        return Ok(());
    };
    if heads.next().is_some() {
        return Ok(());
    }
    let active_count_u64 = u64::try_from(active_count)
        .map_err(|_| ctx.refuse_codec_limit("seed F3D history versions", 0, u64::MAX))?;
    ctx.charge_work(active_count_u64, "seed F3D history versions")?;
    let mut versions = BTreeMap::new();
    for id in 0..active_count {
        ctx.insert_btree_map(&mut versions, id, id, "seed F3D history versions")?;
    }
    let mut projected_storage = ctx.reserve_scoped(0, "index F3D state version projections")?;
    let mut projected = HashMap::new();
    let mut visited = HashSet::new();
    loop {
        ctx.charge_work(1, "bind F3D historical entity versions")?;
        let state = &states[ordinal];
        if visited.contains(&state.node_index) {
            return Ok(());
        }

        ctx.reserve_set(&mut visited, 1, "visit F3D history version state")?;
        visited.insert(state.node_index);
        let mut state_versions = Vec::new();
        for (&entity_ref, &record_ref) in ctx.admit_iter(&versions, "scan F3D versions")? {
            ctx.push_vec(
                &mut state_versions,
                AsmEntityVersion {
                    entity_ref,
                    record_ref,
                },
                "materialize F3D state versions",
            )?;
        }

        projected_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut projected,
                state.node_index,
                state_versions,
                "index F3D state version projections",
            )
            .map(|_| ())
        })?;
        for board in ctx.admit_iter(
            &state.bulletin_boards,
            "scan F3D historical version bulletin boards",
        )? {
            for change in ctx.admit_iter(&board.changes, "scan F3D historical version changes")? {
                match change.kind {
                    AsmEntityChangeKind::Update { old, new } => {
                        if !versions.contains_key(&new)
                            || ctx
                                .binary_search(
                                    archived_ids.as_slice(),
                                    &old,
                                    "find archived F3D revision for update",
                                )?
                                .is_err()
                        {
                            return Ok(());
                        }
                        ctx.insert_btree_map(
                            &mut versions,
                            new,
                            old,
                            "update F3D historical version",
                        )?;
                    }
                    AsmEntityChangeKind::Insert { new } => {
                        if versions.remove(&new).is_none() {
                            return Ok(());
                        }
                    }
                    AsmEntityChangeKind::Delete { old } => {
                        if versions.contains_key(&old)
                            || ctx
                                .binary_search(
                                    archived_ids.as_slice(),
                                    &old,
                                    "find archived F3D revision for delete",
                                )?
                                .is_err()
                        {
                            return Ok(());
                        }
                        ctx.insert_btree_map(
                            &mut versions,
                            old,
                            old,
                            "restore F3D historical version",
                        )?;
                    }
                }
            }
        }
        let Some(next) = state.next_ref else {
            break;
        };
        let Some(&next_ordinal) = by_node.get(&next) else {
            return Ok(());
        };
        ordinal = next_ordinal;
    }
    if visited.len() != states.len() || versions.len() != 1 || versions.get(&0) != Some(&0) {
        return Ok(());
    }
    for state in states {
        state.entity_versions = projected.remove(&state.node_index).unwrap_or_default();
    }
    Ok(())
}

/// Historical topology caches retain normalized records, topology entities,
/// incidence links, and geometry measurements while Design projection runs.
/// This conservative per-entry bound checks that temporary cache against the
/// caller's materialization policy. A binding that exceeds either limit
/// refuses through the caller context before the topology cache is built.
// One live entity can retain its 16-byte version pair, one 8-byte family slot,
// one 48-byte coedge link (the largest topology link), one 56-byte curve-axis
// measurement (the largest geometry measurement), one 8-byte ownership member,
// and one 32-byte relation allocation. The remaining 24 bytes cover the parent
// vector allocation and alignment. Mutually exclusive entity families make
// this an upper bound, not an average observed size.
const HISTORY_TOPOLOGY_CACHE_BYTES_PER_ENTRY: u64 = 192;
/// Conservative work charge for decoding one record in one historical state.
/// A state snapshot traverses the record table repeatedly to build topology,
/// incidence, and carrier projections; this keeps the admission estimate
/// proportional to the state-by-record cross product instead of only the
/// final cache size.
const HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY: u64 = 4096;

#[cfg(test)]
fn complete_table_binding_budget_exceeded(
    table_lengths: impl IntoIterator<Item = usize>,
    limits: &cadmpeg_core::decode::ResourceLimits,
) -> bool {
    table_lengths
        .into_iter()
        .try_fold(0_u64, |total, length| {
            total.checked_add(u64::try_from(length).ok()?)
        })
        .and_then(|entries| entries.checked_mul(HISTORY_TOPOLOGY_CACHE_BYTES_PER_ENTRY))
        .is_none_or(|bytes| bytes > limits.max_materialized_bytes)
}

#[cfg(test)]
fn history_topology_work_budget_exceeded(
    table_lengths: impl IntoIterator<Item = usize>,
    limits: &cadmpeg_core::decode::ResourceLimits,
) -> bool {
    table_lengths
        .into_iter()
        .try_fold(0_u64, |total, length| {
            total.checked_add(u64::try_from(length).ok()?)
        })
        .and_then(|entries| entries.checked_mul(HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY))
        .is_none_or(|work| work > limits.max_work_units)
}

fn admit_complete_table_binding_budget(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    mut table_lengths: impl ExactSizeIterator<Item = usize>,
    limits: &cadmpeg_core::decode::ResourceLimits,
) -> Result<(), cadmpeg_core::CodecError> {
    let state_count = u64_from_index(table_lengths.len());
    ctx.charge_work(state_count, "check F3D complete history topology")?;
    let entries = table_lengths.try_fold(0_u64, |total, length| {
        total.checked_add(u64_from_index(length))
    });
    let bytes = entries
        .and_then(|entries| entries.checked_mul(HISTORY_TOPOLOGY_CACHE_BYTES_PER_ENTRY))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "bind F3D complete history topology bytes",
                limits.max_materialized_bytes,
                u64::MAX,
            )
        })?;
    if bytes > limits.max_materialized_bytes {
        return Err(ctx.refuse_codec_limit(
            "bind F3D complete history topology bytes",
            limits.max_materialized_bytes,
            bytes,
        ));
    }
    let work = entries
        .and_then(|entries| entries.checked_mul(HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "bind F3D complete history topology work",
                limits.max_work_units,
                u64::MAX,
            )
        })?;
    if work > limits.max_work_units {
        return Err(ctx.refuse_codec_limit(
            "bind F3D complete history topology work",
            limits.max_work_units,
            work,
        ));
    }
    Ok(())
}

fn bind_complete_record_tables(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
    bytes: &[u8],
    width: RefWidth,
    limits: &cadmpeg_core::decode::ResourceLimits,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(start) = cadmpeg_asm::asm_header::record_stream_start(bytes) else {
        return Ok(false);
    };
    let active_limit =
        cadmpeg_asm::asm_header::solved_record_limit(ctx, bytes)?.unwrap_or(bytes.len());
    let framed = match cadmpeg_asm::sab::frame(ctx, bytes, start, active_limit, width, None) {
        Ok(records) => records,
        Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => {
            return Err(cadmpeg_core::CodecError::ResourceLimit(error))
        }
        Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
            return Err(error.into_codec_error())
        }
        Err(_) => return Ok(false),
    };
    admit_complete_table_binding_budget(
        ctx,
        states.iter().map(|state| state.entity_versions.len()),
        limits,
    )?;
    let insert_only = insert_only_active_record_count(ctx, states)?;
    let archived_count = archived_active_record_count(ctx, states)?;
    let Some(active_count) = archived_count.or(insert_only) else {
        return Ok(false);
    };
    if insert_only.is_some() && framed.len() != active_count {
        return Ok(false);
    }
    let Some(active_records) = framed.get(..active_count) else {
        return Ok(false);
    };
    let mut archived_frames_storage = ctx.reserve_scoped(0, "retain F3D archived record frame")?;
    let mut archived_frames = BTreeMap::new();
    for state in ctx.admit_iter(&*states, "scan F3D complete record states")? {
        for record in ctx.admit_iter(&state.records, "scan F3D complete state records")? {
            if record.revision_id.is_none() {
                continue;
            }
            let Some(revision_id) = record.revision_id else {
                return Ok(false);
            };
            let Some(offset) = usize::try_from(record.byte_offset).ok() else {
                return Ok(false);
            };
            let Some(limit) = offset.checked_add(record.raw_bytes.len()) else {
                return Ok(false);
            };
            let Some(record_bytes) = bytes.get(offset..limit) else {
                return Ok(false);
            };
            if !ctx.equal(
                record_bytes,
                record.raw_bytes.as_slice(),
                "compare F3D archived record bytes",
            )? {
                return Ok(false);
            }
            let mut framed = match cadmpeg_asm::sab::frame(ctx, bytes, offset, limit, width, None) {
                Ok(records) => records,
                Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => {
                    return Err(cadmpeg_core::CodecError::ResourceLimit(error))
                }
                Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
                    return Err(error.into_codec_error())
                }
                Err(_) => return Ok(false),
            };
            if framed.len() != 1 {
                return Ok(false);
            }
            let Some(framed) = framed.pop() else {
                return Ok(false);
            };
            if !ctx.equal(
                framed.name.as_str(),
                record.name(),
                "compare F3D archived record names",
            )? {
                return Ok(false);
            }

            if !ctx.insert_scoped_btree_map_if_vacant(
                &mut archived_frames_storage,
                &mut archived_frames,
                revision_id,
                framed,
                "find F3D archived record frame",
                "retain F3D archived record frame",
            )? {
                return Ok(false);
            }
        }
    }
    let Some(archive) = historical_record_archive(ctx, states, active_records, archived_frames)?
    else {
        return Ok(false);
    };
    let mut complete = true;
    for state in states.iter_mut() {
        let Some(records) = materialize_record_table(ctx, state, &archive)? else {
            complete = false;
            break;
        };
        let decoded = match crate::brep::decode_history_topology(
            ctx,
            &records.records,
            bytes,
            crate::ids::ID_FORMAT,
        ) {
            Ok(decoded) => decoded,
            Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => {
                complete = false;
                break;
            }
        };
        let Some(topology) = historical_topology_with_tags(ctx, &decoded)? else {
            complete = false;
            break;
        };
        state.topology_cache = crate::history_records::AsmTopologyCache::Complete(topology);
    }
    if complete {
        bind_historical_transitions(ctx, states)?;
        // Keep only record revisions that can be named by a late persistent
        // selection. Non-topological ASM attributes do not participate in
        // feature selection and need not survive projection finalization.
        for state in states {
            if let Some(topology) = state.topology() {
                let slots = topology_entity_slots(ctx, topology)?;
                ctx.retain_vec(
                    &mut state.entity_versions,
                    |version| {
                        ctx.contains_hash_set(
                            &slots.slots,
                            &version.entity_ref,
                            "find F3D historical entity version slot",
                        )
                    },
                    "retain F3D historical entity versions",
                )?;
            } else {
                ctx.clear_vec(
                    &mut state.entity_versions,
                    "clear F3D historical entity versions",
                )?;
            }
        }
    } else {
        for state in states {
            state.topology_cache = crate::history_records::AsmTopologyCache::Absent;
        }
    }
    Ok(false)
}

#[derive(Debug)]
struct HistoricalTopologyEntitySlots<'ctx> {
    slots: HashSet<i64>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn topology_entity_slots<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<HistoricalTopologyEntitySlots<'ctx>, cadmpeg_core::CodecError> {
    let operation = "index F3D historical topology slots";
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut slots = HashSet::new();
    for slot in ctx.admit_iter(&topology.bodies, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.regions, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.shells, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.faces, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.loops, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.coedges, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.edges, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.vertices, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.points, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.surfaces, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.curves, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.pcurves, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    Ok(HistoricalTopologyEntitySlots {
        slots,
        _storage: storage,
    })
}

#[derive(Debug)]
struct HistoricalRecordArchive<'ctx> {
    records: BTreeMap<i64, cadmpeg_asm::sab::Record>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    _token_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn historical_record_archive<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
    active_records: &[cadmpeg_asm::sab::Record],
    archived_frames: BTreeMap<i64, cadmpeg_asm::sab::Record>,
) -> Result<Option<HistoricalRecordArchive<'ctx>>, cadmpeg_core::CodecError> {
    if ctx
        .admit_iter(active_records, "validate F3D active record ordinals")?
        .enumerate()
        .any(|(index, record)| record.index != index)
    {
        return Ok(None);
    }
    let Some(active_count) = i64::try_from(active_records.len()).ok() else {
        return Ok(None);
    };

    let mut revision_entities_storage =
        ctx.reserve_scoped(0, "index F3D active record revisions")?;
    let mut revision_entities = HashMap::new();
    for entity_ref in 0..active_count {
        revision_entities_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut revision_entities,
                entity_ref,
                entity_ref,
                "index F3D active record revisions",
            )
            .map(|_| ())
        })?;
    }
    for state in ctx.admit_iter(states, "scan F3D archived history states")? {
        for board in ctx.admit_iter(&state.bulletin_boards, "scan F3D archived bulletin boards")? {
            for change in ctx.admit_iter(&board.changes, "scan F3D archived board changes")? {
                let Some(old_ref) = change.old_ref() else {
                    continue;
                };
                let entity_ref = change.new_ref().unwrap_or(old_ref);

                if revision_entities_storage
                    .with_storage(|| {
                        ctx.insert_hash_map(
                            &mut revision_entities,
                            old_ref,
                            entity_ref,
                            "index F3D archived record revisions",
                        )
                    })?
                    .is_some()
                {
                    return Ok(None);
                }
            }
        }
    }

    let mut records_storage = ctx.reserve_scoped(0, "retain F3D active record archive")?;
    let mut token_storage = ctx.reserve_scoped(0, "copy F3D archived record tokens")?;
    let mut records = BTreeMap::new();
    for (revision, record) in ctx
        .admit_iter(active_records, "scan F3D active record archive")?
        .enumerate()
    {
        let Some(revision) = i64::try_from(revision).ok() else {
            return Ok(None);
        };
        records_storage.with_storage(|| {
            let cloned = clone_historical_record(ctx, record)?;
            ctx.insert_btree_map(
                &mut records,
                revision,
                cloned,
                "retain F3D active record archive",
            )
            .map(|_| ())
        })?;
    }
    for (revision_id, framed) in
        ctx.admit_iter(archived_frames, "scan F3D archived record frames")?
    {
        if records_storage
            .with_storage(|| {
                ctx.insert_btree_map(
                    &mut records,
                    revision_id,
                    framed,
                    "retain F3D archived record archive",
                )
            })?
            .is_some()
        {
            return Ok(None);
        }
    }
    if records.len() != revision_entities.len() {
        return Ok(None);
    }
    for (&revision_ref, record) in
        ctx.admit_iter(&mut records, "scan F3D record archive revisions")?
    {
        let Some(&entity_ref) = revision_entities.get(&revision_ref) else {
            return Ok(None);
        };
        let Some(index) = usize::try_from(entity_ref).ok() else {
            return Ok(None);
        };
        record.index = index;
        if std::sync::Arc::strong_count(&record.tokens) > 1 {
            let token_count = record.tokens.len();
            let token_width = std::mem::size_of::<cadmpeg_asm::sab::Token>();
            let maximum_token_count = usize::MAX / token_width;
            let token_bytes = token_count.checked_mul(token_width).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "copy F3D archived record tokens",
                    u64_from_index(maximum_token_count),
                    u64_from_index(token_count),
                )
            })?;
            let token_bytes = u64_from_index(token_bytes);
            let mut token_vector_storage =
                ctx.reserve_scoped(0, "copy F3D archived record tokens")?;
            let mut tokens = Vec::new();
            for token in ctx.admit_iter(record.tokens.as_ref(), "scan F3D history record tokens")? {
                let cloned = token_storage.with_storage(|| clone_historical_token(ctx, token))?;
                token_vector_storage.with_storage(|| {
                    ctx.push_vec(&mut tokens, cloned, "copy F3D archived record tokens")
                })?;
            }
            ctx.charge_work(token_bytes, "copy F3D archived record tokens")?;
            token_storage.grow(token_bytes)?;
            record.tokens = tokens.into();
        }
        for token in std::sync::Arc::make_mut(&mut record.tokens) {
            let cadmpeg_asm::sab::Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0 {
                let Some(&entity_ref) = revision_entities.get(reference) else {
                    return Ok(None);
                };
                *reference = entity_ref;
            }
        }
    }
    Ok(Some(HistoricalRecordArchive {
        records,
        _storage: records_storage,
        _token_storage: token_storage,
    }))
}

fn clone_historical_record(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &cadmpeg_asm::sab::Record,
) -> Result<cadmpeg_asm::sab::Record, cadmpeg_core::CodecError> {
    let name = ctx.copy_retained_text(&record.name, "copy F3D historical record text")?;
    Ok(cadmpeg_asm::sab::Record {
        index: record.index,
        name,
        tokens: record.tokens.clone(),
        offset: record.offset,
        len: record.len,
    })
}

fn clone_historical_token(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    token: &cadmpeg_asm::sab::Token,
) -> Result<cadmpeg_asm::sab::Token, cadmpeg_core::CodecError> {
    use cadmpeg_asm::sab::Token;
    Ok(match token {
        Token::Str(value) => {
            Token::Str(ctx.copy_retained_text(value, "copy F3D historical record text")?)
        }
        Token::Ident(value) => {
            Token::Ident(ctx.copy_retained_text(value, "copy F3D historical record text")?)
        }
        Token::SubIdent(value) => {
            Token::SubIdent(ctx.copy_retained_text(value, "copy F3D historical record text")?)
        }
        Token::Char(value) => Token::Char(*value),
        Token::Short(value) => Token::Short(*value),
        Token::Long(value) => Token::Long(*value),
        Token::Float(value) => Token::Float(*value),
        Token::Double(value) => Token::Double(*value),
        Token::True => Token::True,
        Token::False => Token::False,
        Token::Ref(value) => Token::Ref(*value),
        Token::SubtypeOpen => Token::SubtypeOpen,
        Token::SubtypeClose => Token::SubtypeClose,
        Token::Enum(value) => Token::Enum(*value),
        Token::Position(value) => Token::Position(*value),
        Token::Vector3(value) => Token::Vector3(*value),
        Token::Vector2(value) => Token::Vector2(*value),
        Token::Int64(value) => Token::Int64(*value),
    })
}

fn bind_historical_transitions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut by_node_storage = ctx.reserve_scoped(0, "index F3D transition nodes")?;
    let mut by_node = HashMap::new();
    for (ordinal, state) in ctx
        .admit_iter(&*states, "scan F3D transition nodes")?
        .enumerate()
    {
        by_node_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut by_node,
                state.node_index,
                ordinal,
                "index F3D transition nodes",
            )
            .map(|_| ())
        })?;
    }
    if by_node.len() != states.len() {
        return Ok(());
    }

    let mut transitions_storage = ctx.reserve_scoped(0, "collect F3D historical transitions")?;
    let mut transitions = Vec::new();
    for state in ctx.admit_iter(&*states, "scan F3D states")? {
        let previous = match state.next_ref {
            Some(node) => {
                let Some(&ordinal) = by_node.get(&node) else {
                    return Ok(());
                };
                states.get(ordinal)
            }
            None => None,
        };
        let Some(transition) = historical_transition(ctx, state, previous)? else {
            return Ok(());
        };
        transitions_storage.with_storage(|| {
            ctx.push_vec(
                &mut transitions,
                transition,
                "collect F3D historical transitions",
            )
        })?;
    }
    for (state, transition) in states.iter_mut().zip(transitions) {
        state.transition = Some(transition);
    }
    Ok(())
}

fn retain_mirror_plane_topology(
    mut topology: AsmHistoricalTopology,
) -> Option<AsmHistoricalTopology> {
    let retained = AsmHistoricalTopology {
        bodies: std::mem::take(&mut topology.bodies),
        regions: std::mem::take(&mut topology.regions),
        shells: std::mem::take(&mut topology.shells),
        faces: std::mem::take(&mut topology.faces),
        loops: std::mem::take(&mut topology.loops),
        coedges: std::mem::take(&mut topology.coedges),
        edges: std::mem::take(&mut topology.edges),
        vertices: std::mem::take(&mut topology.vertices),
        points: std::mem::take(&mut topology.points),
        surfaces: std::mem::take(&mut topology.surfaces),
        curves: std::mem::take(&mut topology.curves),
        pcurves: std::mem::take(&mut topology.pcurves),
        face_surfaces: std::mem::take(&mut topology.face_surfaces),
        surface_planes: std::mem::take(&mut topology.surface_planes),
        loop_coedges: std::mem::take(&mut topology.loop_coedges),
        coedge_topology: std::mem::take(&mut topology.coedge_topology),
        face_loops: std::mem::take(&mut topology.face_loops),
        edge_curves: std::mem::take(&mut topology.edge_curves),
        coedge_pcurves: std::mem::take(&mut topology.coedge_pcurves),
        curve_axes: std::mem::take(&mut topology.curve_axes),
        ..Default::default()
    };
    (!retained.face_surfaces.is_empty()
        || !retained.surface_planes.is_empty()
        || !retained.loop_coedges.is_empty()
        || !retained.coedge_topology.is_empty()
        || !retained.face_loops.is_empty()
        || !retained.edge_curves.is_empty()
        || !retained.coedge_pcurves.is_empty()
        || !retained.curve_axes.is_empty())
    .then_some(retained)
}

/// Release complete historical snapshots after every projection consumer has
/// finished. Raw history records, sparse transitions, and the compact
/// plane-selection topology remain retained.
pub(crate) fn discard_projection_caches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &mut [AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    for history in ctx.admit_iter(histories, "scan F3D projection cache histories")? {
        for state in ctx.admit_iter(&mut history.states, "scan F3D projection cache states")? {
            let topology = match std::mem::take(&mut state.topology_cache) {
                crate::history_records::AsmTopologyCache::Absent
                | crate::history_records::AsmTopologyCache::Released => None,
                crate::history_records::AsmTopologyCache::Complete(topology)
                | crate::history_records::AsmTopologyCache::Retained(topology) => {
                    retain_mirror_plane_topology(topology)
                }
            };
            if let Some(topology) = topology {
                let slots = topology_entity_slots(ctx, &topology)?;
                ctx.retain_vec(
                    &mut state.entity_versions,
                    |version| {
                        ctx.contains_hash_set(
                            &slots.slots,
                            &version.entity_ref,
                            "find F3D historical entity version slot",
                        )
                    },
                    "retain F3D historical entity versions",
                )?;
                state.topology_cache = crate::history_records::AsmTopologyCache::Retained(topology);
            } else {
                ctx.clear_vec(
                    &mut state.entity_versions,
                    "clear F3D historical entity versions",
                )?;
                state.topology_cache = crate::history_records::AsmTopologyCache::Released;
            }
        }
    }
    Ok(())
}

pub(crate) fn projection_was_finalized(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &[AsmHistory],
) -> Result<bool, cadmpeg_core::CodecError> {
    if histories.is_empty() {
        return Ok(false);
    }
    decode.all_by(
        histories,
        |history| history.projection_finalized(decode),
        "check F3D history projection finalization",
    )
}

fn historical_transition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    current: &AsmDeltaState,
    previous: Option<&AsmDeltaState>,
) -> Result<Option<AsmHistoricalTransition>, cadmpeg_core::CodecError> {
    let Some(current_topology) = current.topology() else {
        return Ok(None);
    };
    let previous_topology = previous.and_then(|state| state.topology());
    let current_versions = historical_version_map(ctx, &current.entity_versions)?;
    let previous_versions = match previous {
        Some(state) => historical_version_map(ctx, &state.entity_versions)?,
        None => BTreeMap::new(),
    };
    let delta = |current: &[i64], previous: &[i64]| {
        entity_delta(
            ctx,
            current,
            previous,
            &current_versions,
            &previous_versions,
        )
    };
    let empty = AsmHistoricalTopology::default();
    let previous_topology = previous_topology.unwrap_or(&empty);
    let mut record_key_storage = ctx.reserve_scoped(0, "collect F3D transition version keys")?;
    let current_record_key_source = ctx.admit_iter(
        &current_versions,
        "scan F3D current transition version keys",
    )?;
    let current_record_keys = record_key_storage.with_storage(|| {
        ctx.collect_vec(
            current_record_key_source.map(|(entity_ref, _)| *entity_ref),
            "collect F3D transition version keys",
        )
    })?;
    let previous_record_key_source = ctx.admit_iter(
        &previous_versions,
        "scan F3D previous transition version keys",
    )?;
    let previous_record_keys = record_key_storage.with_storage(|| {
        ctx.collect_vec(
            previous_record_key_source.map(|(entity_ref, _)| *entity_ref),
            "collect F3D transition version keys",
        )
    })?;
    Ok(Some(AsmHistoricalTransition {
        previous_state_id: previous.map(|state| state.state_id),
        records: entity_delta(
            ctx,
            &current_record_keys,
            &previous_record_keys,
            &current_versions,
            &previous_versions,
        )?,
        topology: AsmHistoricalTopologyDelta {
            bodies: delta(&current_topology.bodies, &previous_topology.bodies)?,
            regions: delta(&current_topology.regions, &previous_topology.regions)?,
            shells: delta(&current_topology.shells, &previous_topology.shells)?,
            faces: delta(&current_topology.faces, &previous_topology.faces)?,
            loops: delta(&current_topology.loops, &previous_topology.loops)?,
            coedges: delta(&current_topology.coedges, &previous_topology.coedges)?,
            edges: delta(&current_topology.edges, &previous_topology.edges)?,
            vertices: delta(&current_topology.vertices, &previous_topology.vertices)?,
            points: delta(&current_topology.points, &previous_topology.points)?,
            surfaces: delta(&current_topology.surfaces, &previous_topology.surfaces)?,
            curves: delta(&current_topology.curves, &previous_topology.curves)?,
            pcurves: delta(&current_topology.pcurves, &previous_topology.pcurves)?,
        },
    }))
}

fn historical_version_map(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    versions: &[AsmEntityVersion],
) -> Result<BTreeMap<i64, i64>, cadmpeg_core::CodecError> {
    let mut indexed = BTreeMap::new();
    for version in ctx.admit_iter(versions, "scan F3D historical versions")? {
        ctx.insert_btree_map(
            &mut indexed,
            version.entity_ref,
            version.record_ref,
            "index F3D transition versions",
        )?;
    }
    Ok(indexed)
}

fn entity_delta(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    current: &[i64],
    previous: &[i64],
    current_versions: &BTreeMap<i64, i64>,
    previous_versions: &BTreeMap<i64, i64>,
) -> Result<AsmHistoricalEntityDelta, cadmpeg_core::CodecError> {
    let current = ctx.collect_btree_set(
        current.iter().copied(),
        "index F3D current transition entities",
    )?;
    let previous = ctx.collect_btree_set(
        previous.iter().copied(),
        "index F3D previous transition entities",
    )?;
    let mut inserted = Vec::new();
    for entity in ctx.admit_iter(&current, "scan F3D inserted transition entities")? {
        if !ctx.contains_btree_set(&previous, entity, "find F3D inserted transition entity")? {
            ctx.charge_work(1, "copy F3D transition delta entity")?;
            ctx.push_vec(&mut inserted, *entity, "collect F3D transition delta")?;
        }
    }
    let mut deleted = Vec::new();
    for entity in ctx.admit_iter(&previous, "scan F3D deleted transition entities")? {
        if !ctx.contains_btree_set(&current, entity, "find F3D deleted transition entity")? {
            ctx.charge_work(1, "copy F3D transition delta entity")?;
            ctx.push_vec(&mut deleted, *entity, "collect F3D transition delta")?;
        }
    }
    let mut updated = Vec::new();
    for entity in ctx.admit_iter(&current, "scan F3D shared transition entities")? {
        if ctx.contains_btree_set(&previous, entity, "find F3D shared transition entity")? {
            let current_record = ctx.get_btree_map(
                current_versions,
                entity,
                "find F3D current transition record version",
            )?;
            let previous_record = ctx.get_btree_map(
                previous_versions,
                entity,
                "find F3D previous transition record version",
            )?;
            if !ctx.equal(
                &current_record,
                &previous_record,
                "compare F3D transition record versions",
            )? {
                ctx.charge_work(1, "copy F3D transition delta entity")?;
                ctx.push_vec(&mut updated, *entity, "collect F3D transition delta")?;
            }
        }
    }
    Ok(AsmHistoricalEntityDelta {
        inserted,
        deleted,
        updated,
    })
}

pub(crate) fn bind_feature_outputs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    active_bodies: &[cadmpeg_ir::topology::Body],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut state_outputs = HashMap::<i64, Option<Vec<i64>>>::new();
    for history in ctx.admit_iter(histories, "scan F3D feature output histories")? {
        let mut by_node_storage =
            ctx.reserve_scoped(0, "index F3D feature output history nodes")?;
        let mut by_node = HashMap::new();
        for state in ctx.admit_iter(&history.states, "scan F3D history states")? {
            by_node_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut by_node,
                    state.node_index,
                    state,
                    "index F3D feature output history nodes",
                )
                .map(|_| ())
            })?;
        }
        if by_node.len() != history.states.len() {
            continue;
        }
        for state in ctx.admit_iter(&history.states, "scan F3D history states")? {
            let previous = match state.next_ref {
                Some(node) => match by_node.get(&node) {
                    Some(previous) => Some(*previous),
                    None => continue,
                },
                None => None,
            };
            let Some(outputs) = affected_body_refs(ctx, state, previous)? else {
                continue;
            };
            if !state_outputs.contains_key(&state.state_id) {
                ctx.reserve_map(&mut state_outputs, 1, "index F3D feature output states")?;
            }
            state_outputs
                .entry(state.state_id)
                .and_modify(|outputs| *outputs = None)
                .or_insert_with(|| Some(outputs));
        }
    }
    let mut active_storage = ctx.reserve_scoped(0, "index F3D active feature output bodies")?;
    let mut active = HashMap::new();
    for body in ctx.admit_iter(active_bodies, "scan F3D active bodies")? {
        let Some(slot) = stable_ref(ctx, body.id.as_str())? else {
            continue;
        };
        active_storage.with_storage(|| {
            let id = body
                .id
                .try_clone_for_decode(ctx, "copy F3D active body identity")?;
            ctx.insert_hash_map(
                &mut active,
                slot,
                id,
                "index F3D active feature output bodies",
            )
            .map(|_| ())
        })?;
    }
    for feature in features {
        let Some(id) = feature.native_ref.as_deref() else {
            continue;
        };
        let operation = "find F3D feature output scope";
        let Some(scope) = ctx.find_by(
            scopes,
            |scope| ctx.equal(scope.id.as_str(), id, operation),
            operation,
        )?
        else {
            continue;
        };
        let (Some(state_id), Some(previous_state_id)) =
            (scope.history_state_id(), scope.previous_history_state_id())
        else {
            continue;
        };
        let Some(Some(outputs)) = state_outputs.get(&state_id) else {
            continue;
        };
        let transition_matches = histories
            .iter()
            .flat_map(|history| &history.states)
            .filter(|state| state.state_id == state_id)
            .map(|state| {
                state
                    .transition
                    .as_ref()
                    .and_then(|transition| transition.previous_state_id)
                    == Some(previous_state_id)
            })
            .eq([true]);
        if transition_matches {
            let mut resolved = Vec::new();
            for slot in ctx.admit_iter(outputs, "scan F3D feature output slots")? {
                let Some(id) = active.get(slot) else {
                    continue;
                };
                let id = id.try_clone_for_decode(ctx, "copy F3D feature output body identity")?;

                ctx.reserve_vec(&mut resolved, 1, "collect F3D feature output bodies")?;
                resolved.push(id);
            }
            feature.evaluation.set_outputs(
                cadmpeg_ir::features::DistinctMembers::try_from(resolved, ctx)
                    .map_err(cadmpeg_core::CodecError::from)?,
            );
            bind_base_feature_output_selection(ctx, feature)?;
        }
    }
    Ok(())
}

fn bind_base_feature_output_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &mut cadmpeg_ir::features::Feature,
) -> Result<(), cadmpeg_core::CodecError> {
    if feature.evaluation.outputs().is_empty() {
        return Ok(());
    }
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::BaseFeature {
            bodies: cadmpeg_ir::features::BodySelection::Native(native),
        },
    ) = feature.evaluation.definition()
    else {
        return Ok(());
    };
    let mut selected = Vec::new();
    for body in feature.evaluation.outputs() {
        let id = body.try_clone_for_decode(ctx, "copy F3D BaseFeature body identity")?;

        ctx.reserve_vec(&mut selected, 1, "collect F3D BaseFeature output bodies")?;
        selected.push(id);
    }
    let bodies = cadmpeg_ir::features::BodySelection::Resolved {
        bodies: cadmpeg_ir::features::DistinctMembers::try_from(selected, ctx)
            .map_err(cadmpeg_core::CodecError::from)?,
        native: ctx.copy_retained_text(native, "copy F3D BaseFeature native selection")?,
    };
    feature
        .evaluation
        .set_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::BaseFeature { bodies },
        ));
    Ok(())
}

pub(crate) fn bind_sweep_result_modes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    bodies: &[cadmpeg_ir::topology::Body],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SweepMode, SweepShape};
    use cadmpeg_ir::topology::BodyKind;

    let mut body_kind_storage = ctx.reserve_scoped(0, "index F3D sweep body kinds")?;
    let mut body_kinds = HashMap::new();
    for body in ctx.admit_iter(bodies, "scan F3D sweep bodies")? {
        body_kind_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut body_kinds,
                &body.id,
                body.kind,
                "index F3D sweep body kinds",
            )
            .map(|_| ())
        })?;
    }
    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: SweepShape::Unresolved { sections, .. },
            ..
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        if feature.evaluation.outputs().is_empty() {
            continue;
        }
        let section_count = sections.len();
        let mut all_sheet = true;
        let mut all_solid = true;
        for output in
            ctx.admit_iter(feature.evaluation.outputs(), "scan F3D sweep output bodies")?
        {
            match ctx.get_hash_map(&body_kinds, output, "resolve F3D sweep output kind")? {
                Some(BodyKind::Sheet) => all_solid = false,
                Some(BodyKind::Solid) => all_sheet = false,
                _ => {
                    all_sheet = false;
                    all_solid = false;
                }
            }
        }
        let mode = if all_sheet {
            SweepMode::Surface {}
        } else if all_solid {
            SweepMode::Solid {
                op: cadmpeg_ir::features::SolidSweepOperation::NewBody,
            }
        } else {
            SweepMode::Unresolved {}
        };
        let mut solid_sections = Vec::new();
        if matches!(mode, SweepMode::Solid { .. }) {
            ctx.reserve_vec(
                &mut solid_sections,
                section_count,
                "convert F3D solid sweep sections",
            )?;
        }
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) = definition
            else {
                return;
            };
            let SweepShape::Unresolved { section, sections } =
                std::mem::replace(shape, SweepShape::unresolved(None))
            else {
                return;
            };
            *shape = match mode {
                SweepMode::Solid { op } => {
                    solid_sections.extend(sections.into_iter().map(Into::into));
                    SweepShape::Solid {
                        op,
                        section: section.into(),
                        sections: solid_sections,
                    }
                }
                _ => SweepShape::sheet_sections(mode, section, sections),
            };
        });
    }
    Ok(())
}

/// Native history and neutral topology used to resolve feature body operands.
pub(crate) struct FeatureBodySelectionInputs<'a> {
    /// Decoded Design feature scopes.
    pub(crate) scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    /// Counted Design construction-operand groups.
    pub(crate) groups:
        &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
    /// Whole-body recipe operands.
    pub(crate) body_recipe_operands:
        &'a [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    /// Construction recipes backing whole-body operands.
    pub(crate) construction_recipes: &'a [crate::records::recipes::ConstructionRecipe],
    /// Persistent body identities in the active solved B-rep.
    pub(crate) persistent_design_links: &'a [crate::records::sketch_links::PersistentDesignLink],
    /// Independent ASM history graphs.
    pub(crate) histories: &'a [AsmHistory],
    /// Neutral top-level bodies.
    pub(crate) bodies: &'a [cadmpeg_ir::topology::Body],
    /// Neutral body regions.
    pub(crate) regions: &'a [cadmpeg_ir::topology::Region],
    /// Neutral region shells.
    pub(crate) shells: &'a [cadmpeg_ir::topology::Shell],
}

pub(crate) fn bind_feature_body_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{BodyMember, BodySelection, FeatureDefinition, FeatureOperation};

    let scopes = inputs.scopes;
    let groups = inputs.groups;
    let body_recipe_operands = inputs.body_recipe_operands;
    let histories = inputs.histories;
    let bodies = inputs.bodies;
    let regions = inputs.regions;
    let shells = inputs.shells;

    bind_pattern_body_selections(ctx, features, inputs)?;
    let mut pattern_body_slots_storage =
        ctx.reserve_scoped(0, "index F3D pattern body features")?;
    let mut pattern_body_slots = HashMap::new();
    for feature in ctx.admit_iter(&*features, "scan F3D pattern body features")? {
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let cadmpeg_ir::features::patterns::PatternTransform::Circular { count, .. } =
            pattern.definition()
        else {
            continue;
        };
        let [cadmpeg_ir::features::patterns::PatternSeed::Bodies(BodySelection::Historical {
            bodies: seed_bodies,
            ..
        })] = seeds.as_slice()
        else {
            continue;
        };
        let [seed_body] = seed_bodies.as_slice() else {
            continue;
        };
        let Ok(expected_count) = usize::try_from(*count) else {
            continue;
        };
        if feature.evaluation.outputs().len().checked_add(1) != Some(expected_count) {
            continue;
        }
        let Some(seed_slot) = historical_body_slot(ctx, seed_body.as_str())? else {
            continue;
        };
        pattern_body_slots_storage.with_storage(|| {
            let mut slots = BTreeSet::new();
            ctx.insert_btree_set(&mut slots, seed_slot, "index F3D pattern body slots")
                .map(|_| ())?;
            for body in ctx.admit_iter(
                feature.evaluation.outputs(),
                "scan F3D pattern body outputs",
            )? {
                let Some(slot) = stable_ref(ctx, body.as_str())? else {
                    return Ok(());
                };
                ctx.insert_btree_set(&mut slots, slot, "index F3D pattern body slots")
                    .map(|_| ())?;
            }
            if slots.len() != expected_count {
                return Ok(());
            }
            let feature_id = feature
                .id
                .try_clone_for_decode(ctx, "copy F3D pattern body feature ID")?;
            ctx.insert_hash_map(
                &mut pattern_body_slots,
                feature_id,
                slots,
                "index F3D pattern body features",
            )
            .map(|_| ())
        })?;
    }

    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let dependencies = &feature.dependencies;
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
        'feature_edit: {
            let Some(native_ref) = native_ref else {
                break 'feature_edit;
            };
            let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
            let Some(scope) = matching_scopes.next() else {
                break 'feature_edit;
            };
            if matching_scopes.next().is_some() {
                break 'feature_edit;
            }
            if matches!(
                definition,
                FeatureDefinition::Operation(FeatureOperation::Pattern { .. })
            ) {
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::BoundaryFill { tools, cells }) =
                definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    edit_result = bind_body_recipe_body_selection(
                        ctx,
                        tools,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                    for cell in cells {
                        if edit_result.is_err() { break; }
                        edit_result = bind_body_recipe_body_selection(
                            ctx,
                            cell,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            body_recipe_operands,
                        );
                    }
                } else {
                    edit_result = bind_direct_body_recipe_body_selection(ctx, tools, scope, inputs);
                    for cell in cells {
                        if edit_result.is_err() { break; }
                        edit_result = bind_direct_body_recipe_body_selection(ctx, cell, scope, inputs);
                    }
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
                definition
            {
                let edit = operands
                    .try_edit(|target, tools| {
                        macro_rules! admitted {
                            ($value:expr) => {
                                match $value {
                                    Ok(value) => value,
                                    Err(error) => { edit_result = Err(error); return; }
                                }
                            };
                        }
                        let (Some(state_id), Some(previous_state_id)) =
                            (scope.history_state_id(), scope.previous_history_state_id())
                        else {
                            edit_result = bind_direct_body_recipe_body_selection(ctx, target, scope, inputs);
                            if edit_result.is_err() { return; }
                            edit_result = bind_direct_body_recipe_body_selection(ctx, tools, scope, inputs);
                            if edit_result.is_err() { return; }
                            if matches!(
                                tools,
                                BodySelection::Native(_) | BodySelection::NativeSet(_)
                            ) {
                                if let Some(local) = admitted!(combine_external_local_tools(ctx, scope)) {
                                    *tools = local;
                                }
                            }
                            return;
                        };
                        if let Some(local) = admitted!(combine_external_local_tools(ctx, scope)) {
                            *tools = local;
                        }
                        let BodySelection::Native(native) = target else {
                            return;
                        };
                        let Some((history, state, _)) =
                            admitted!(unique_history_state_pair(
                                ctx,
                                histories,
                                state_id,
                                previous_state_id,
                            ))
                        else {
                            return;
                        };
                        let history_states = match unique_feature_history_states(ctx, history) {
                            Ok(states) => states,
                            Err(error) => { edit_result = Err(error); return; }
                        };
                        let body = match singleton_revised_input_body_across_state_chain(
                            ctx,
                            state,
                            previous_state_id,
                            &history_states,
                        ) {
                            Ok(Some(body)) => body,
                            Ok(None) => return,
                            Err(error) => { edit_result = Err(error); return; }
                        };
                        let input_state = admitted!(crate::ids::history_input_state_id_charged(
                            ctx, feature_id, previous_state_id));
                        let body_id = admitted!(crate::ids::history_input_body_id_charged(
                            ctx, feature_id, previous_state_id, body));
                        let native_id = admitted!(ctx.copy_retained_text(native, "copy F3D Combine target identity"));
                        let mut target_bodies = admitted!(ctx.collection_vec(1, "validate F3D Combine target body"));
                        target_bodies.push(body_id);
                        if let Ok(historical) = admitted!(cadmpeg_ir::features::BodySelection::historical(admitted!(crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)), target_bodies, native_id, ctx).map_err(cadmpeg_core::CodecError::from)) {
                            *target = historical;
                        }
                        let Some(stream) = crate::ids::native_stream(&scope.id) else {
                            return;
                        };
                        let Some(operation) = scope.combine_operation() else {
                            return;
                        };
                        let mut native_tools = Vec::new();
                        for tool in admitted!(ctx.admit_iter(
                            std::slice::from_ref(&operation.tools.first),
                            "scan F3D operation tools",
                        ).map_err(cadmpeg_core::CodecError::ResourceLimit)).chain(admitted!(ctx.admit_iter(
                            &operation.tools.additional,
                            "scan F3D operation tools",
                        ).map_err(cadmpeg_core::CodecError::ResourceLimit)))
                        {
                            let native = admitted!(ctx.format_retained(format_args!("{stream}:design-record#{}", tool.record_index), "retain F3D Combine tool identity"));
                            admitted!(ctx.reserve_vec(&mut native_tools, 1, "collect F3D Combine tool identities"));
                            native_tools.push(native);
                        }
                        let current_history_source = admitted!(historical_brep_source(ctx, &state.id));
                        let mut historical_tool_rows = Vec::new();
                        let mut direct_tool_rows = Vec::new();
                        for (record_index, native) in operation.tools.iter()
                            .map(|tool| tool.record_index).zip(&native_tools) {
                            let mut matching = body_recipe_operands.iter().filter(|operand| {
                                crate::ids::native_stream(&operand.id) == Some(stream)
                                    && operand.scope_record_index == scope.record_index
                                    && matches!(
                            operand.owner,
                            crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
                        ) && operand.record_index() == record_index
                            });
                            let Some(operand) = matching.next() else {
                                historical_tool_rows.clear();
                                direct_tool_rows.clear();
                                break;
                            };
                            if matching.next().is_some() {
                                historical_tool_rows.clear();
                                direct_tool_rows.clear();
                                break;
                            }
                            if let Some(body) = operand.resolved_body_slot {
                                let body = admitted!(crate::ids::history_input_body_id_charged(
                                    ctx, feature_id, previous_state_id, body));
                                let repeated = historical_tool_rows
                                    .iter()
                                    .any(|row: &BodyMember<_>| row.body() == &body);
                                let row = admitted!(body_member(ctx, body, admitted!(ctx.copy_retained_text(native, "copy F3D Combine tool member identity"))).map_err(cadmpeg_core::CodecError::from));
                                let (false, Some(row)) = (repeated, row) else {
                                    historical_tool_rows.clear();
                                    direct_tool_rows.clear();
                                    break;
                                };
                                admitted!(ctx.reserve_vec(&mut historical_tool_rows, 1, "collect F3D Combine historical tool rows"));
                                historical_tool_rows.push(row);
                                continue;
                            }
                            let body = match unique_external_body_candidate(
                                ctx,
                                operand,
                                current_history_source,
                                bodies,
                                regions,
                                shells,
                            ) {
                                Ok(Some(body)) => body,
                                Ok(None) => {
                                    historical_tool_rows.clear();
                                    direct_tool_rows.clear();
                                    break;
                                }
                                Err(error) => {
                                    edit_result = Err(error);
                                    return;
                                }
                            };
                            let repeated = direct_tool_rows
                                .iter()
                                .any(|row: &BodyMember<_>| row.body() == &body);
                            let row = admitted!(body_member(ctx, body, admitted!(ctx.copy_retained_text(native, "copy F3D Combine direct tool member identity"))).map_err(cadmpeg_core::CodecError::from));
                            let (false, Some(row)) = (repeated, row) else {
                                historical_tool_rows.clear();
                                direct_tool_rows.clear();
                                break;
                            };
                            admitted!(ctx.reserve_vec(&mut direct_tool_rows, 1, "collect F3D Combine direct tool rows"));
                            direct_tool_rows.push(row);
                        }
                        if historical_tool_rows.len() == native_tools.len() {
                            let Ok(members) = admitted!(cadmpeg_ir::features::BodyMembers::try_from_rows(
                                historical_tool_rows, ctx).map_err(cadmpeg_core::CodecError::from)) else {
                                return;
                            };
                            *tools = BodySelection::HistoricalSet {
                                state: input_state,
                                members,
                            };
                        } else if direct_tool_rows.len() == native_tools.len() {
                            *tools = if direct_tool_rows.len() == 1 {
                                let Some(row) = direct_tool_rows.pop() else { return; };
                                let (body, native) = admitted!(row.into_parts(ctx));
                                let mut selected = admitted!(ctx.collection_vec(1, "validate F3D Combine resolved body"));
                                selected.push(body);
                                let bodies = match cadmpeg_ir::features::DistinctMembers::try_from(selected, ctx) {
    Ok(value) => value,
    Err(cadmpeg_ir::features::FeatureCollectionError::Invalid(_)) => return,
    Err(cadmpeg_ir::features::FeatureCollectionError::Resource(limit)) => { edit_result = Err(limit.into()); return; },
};
                                BodySelection::Resolved {
                                    bodies,
                                    native,
                                }
                            } else {
                                let Ok(members) = admitted!(cadmpeg_ir::features::BodyMembers::try_from_rows(
                                    direct_tool_rows, ctx).map_err(cadmpeg_core::CodecError::from)) else {
                                    return;
                                };
                                BodySelection::ResolvedSet { members }
                            };
                        } else {
                            let tool_record_indices = admitted!(ctx.collect_vec(operation.tools.iter().map(|tool| tool.record_index), "collect F3D Combine tool record indices"));
                            if let Some(tool_slots) = admitted!(combine_recipe_family_tool_slots(
                                ctx,
                                (stream, scope.record_index),
                                &tool_record_indices,
                                previous_state_id,
                                body,
                                body_recipe_operands,
                                inputs.construction_recipes,
                            )) {
                                if tool_slots.len() != native_tools.len() {
                                    return;
                                }
                                let Some(rows) = admitted!(combine_historical_rows(
                                    ctx, feature_id, previous_state_id, tool_slots,
                                    native_tools)) else {
                                    return;
                                };
                                let Ok(members) =
                                    admitted!(cadmpeg_ir::features::BodyMembers::try_from_rows(rows, ctx).map_err(cadmpeg_core::CodecError::from))
                                else {
                                    return;
                                };
                                *tools = BodySelection::HistoricalSet {
                                    state: input_state,
                                    members,
                                };
                                return;
                            }
                            let mut pattern_bodies = None;
                            let mut multiple_pattern_bodies = false;
                            for dependency in admitted!(ctx.admit_iter(
                                dependencies.as_slice(),
                                "scan F3D pattern body dependencies",
                            ).map_err(cadmpeg_core::CodecError::ResourceLimit)) {
                                if let Some(bodies) = admitted!(ctx.get_hash_map(
                                    &pattern_body_slots,
                                    dependency,
                                    "find F3D pattern body dependency",
                                )) {
                                    if pattern_bodies.is_some() {
                                        multiple_pattern_bodies = true;
                                        break;
                                    }
                                    pattern_bodies = Some(bodies);
                                }
                            }
                            if !multiple_pattern_bodies {
                              if let Some(pattern_bodies) = pattern_bodies {
                                if let Some(tool_slots) = admitted!(pattern_combine_tool_slots(
                                    ctx,
                                    pattern_bodies,
                                    body,
                                    native_tools.len(),
                                )) {
                                    if tool_slots.len() != native_tools.len() {
                                        return;
                                    }
                                    let Some(rows) = admitted!(combine_historical_rows(
                                        ctx, feature_id, previous_state_id, tool_slots,
                                        native_tools)) else {
                                        return;
                                    };
                                    let Ok(members) =
                                        admitted!(cadmpeg_ir::features::BodyMembers::try_from_rows(rows, ctx).map_err(cadmpeg_core::CodecError::from))
                                    else {
                                        return;
                                    };
                                    *tools = BodySelection::HistoricalSet {
                                        state: input_state,
                                        members,
                                    };
                                }
                              }
                            }
                        }
                    });
                if let Err(error) = edit {
                    edit_result = Err(cadmpeg_core::CodecError::malformed(error));
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Coil {
                result: cadmpeg_ir::features::CoilResult::Boolean { targets, .. },
                ..
            }) = definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    edit_result = bind_body_recipe_body_selection(
                        ctx,
                        targets,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                } else {
                    edit_result = bind_direct_body_recipe_body_selection(ctx, targets, scope, inputs);
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, .. }) =
                definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    edit_result = bind_body_recipe_body_selection(
                        ctx,
                        bodies,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                } else {
                    edit_result = bind_direct_body_recipe_body_selection(ctx, bodies, scope, inputs);
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Scale { bodies, .. }) =
                definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    edit_result = bind_body_recipe_body_selection(
                        ctx,
                        bodies,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                    if edit_result.is_ok() && matches!(bodies, BodySelection::Native(_)) {
                        edit_result = bind_direct_body_recipe_body_selection(ctx, bodies, scope, inputs);
                    }
                } else {
                    edit_result = bind_direct_body_recipe_body_selection(ctx, bodies, scope, inputs);
                }
                break 'feature_edit;
            }
            let (bodies, proof) = match definition {
                FeatureDefinition::Operation(FeatureOperation::MoveBody { bodies, .. }) => {
                    (bodies, BodySelectionProof::TopologyStableRevision)
                }
                FeatureDefinition::Operation(FeatureOperation::Shell {
                    bodies: Some(bodies),
                    ..
                }) => (bodies, BodySelectionProof::RevisedInput),
                FeatureDefinition::Operation(FeatureOperation::SplitBody { targets, .. }) => {
                    (targets, BodySelectionProof::RevisedInput)
                }
                _ => break 'feature_edit,
            };
            let BodySelection::Native(group_id) = bodies else {
                break 'feature_edit;
            };
            let mut matching_groups = groups.iter().filter(|group| {
                group.id == *group_id
                    && group.scope_record_index == scope.record_index
                    && group.role() == DesignOperandRole::BODIES_A
                    && crate::ids::native_stream(&group.id) == crate::ids::native_stream(&scope.id)
            });
            let Some(group) = matching_groups.next() else {
                break 'feature_edit;
            };
            if matching_groups.next().is_some() || group.members().len() != 1 {
                break 'feature_edit;
            }
            let (Some(state_id), Some(previous_state_id)) =
                (scope.history_state_id(), scope.previous_history_state_id())
            else {
                edit_result = bind_direct_body_recipe_body_selection(ctx, bodies, scope, inputs);
                break 'feature_edit;
            };
            let Some((history, state, _previous)) = (match unique_history_state_pair(
                ctx,
                histories,
                state_id,
                previous_state_id,
            ) {
                Ok(pair) => pair,
                Err(error) => {
                    edit_result = Err(error);
                    break 'feature_edit;
                }
            }) else {
                edit_result = bind_direct_body_recipe_body_selection(ctx, bodies, scope, inputs);
                break 'feature_edit;
            };
            let history_states = match unique_feature_history_states(ctx, history) {
                Ok(states) => states,
                Err(error) => { edit_result = Err(error); break 'feature_edit; }
            };
            let body = match proof {
                BodySelectionProof::TopologyStableRevision => {
                    singleton_body_revision_across_state_chain(
                        ctx,
                        state,
                        previous_state_id,
                        &history_states,
                    )
                }
                BodySelectionProof::RevisedInput => {
                    singleton_revised_input_body_across_state_chain(
                        ctx,
                        state,
                        previous_state_id,
                        &history_states,
                    )
                }
            };
            let body = match body {
                Ok(body) => body,
                Err(error) => { edit_result = Err(error); break 'feature_edit; }
            };
            let Some(body) = body else {
                break 'feature_edit;
            };
            let state_id = crate::ids::history_input_state_id_charged(
                ctx, feature_id, previous_state_id);
            let body_id = crate::ids::history_input_body_id_charged(
                ctx, feature_id, previous_state_id, body);
            let native = ctx.copy_retained_text(group_id, "copy F3D pattern body group identity");
            let (state_id, body_id, native) = match (state_id, body_id, native) {
                (Ok(state_id), Ok(body_id), Ok(native)) => (state_id, body_id, native),
                (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
                    edit_result = Err(error);
                    break 'feature_edit;
                }
            };
            let mut selected = match ctx.collection_vec(1, "validate F3D pattern body selection") {
                Ok(selected) => selected,
                Err(error) => {
                    edit_result = Err(error);
                    break 'feature_edit;
                }
            };
            selected.push(body_id);
            match cadmpeg_ir::features::BodySelection::historical(state_id, selected, native, ctx) {
                Ok(Ok(historical)) => *bodies = historical,
                Ok(Err(_)) => {},
                Err(limit) => { edit_result = Err(limit.into()); break 'feature_edit; },
            }
        }
        });
        edit_result?;
    }

    Ok(())
}

fn combine_historical_rows(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    slots: Vec<i64>,
    native_tools: Vec<String>,
) -> Result<
    Option<Vec<cadmpeg_ir::features::BodyMember<cadmpeg_ir::ids::HistoricalBodyId>>>,
    cadmpeg_core::CodecError,
> {
    ctx.collect_fallible_options(
        slots.into_iter().zip(native_tools).map(|(slot, native)| {
            let body = crate::ids::history_input_body_id_charged(
                ctx,
                feature_id,
                previous_state_id,
                slot,
            )?;
            body_member(ctx, body, native).map_err(cadmpeg_core::CodecError::ResourceLimit)
        }),
        "collect F3D Combine fallback rows",
    )
}

fn pattern_combine_tool_slots(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pattern_bodies: &BTreeSet<i64>,
    target_body: i64,
    native_tool_count: usize,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    if !pattern_bodies.contains(&target_body)
        || pattern_bodies.len().checked_sub(1) != Some(native_tool_count)
    {
        return Ok(None);
    }
    Ok(Some(
        ctx.collect_vec(
            pattern_bodies
                .iter()
                .copied()
                .filter(|body| *body != target_body),
            "collect F3D pattern Combine tools",
        )?,
    ))
}

fn combine_recipe_family_tool_slots(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope_key: (&str, u32),
    tool_record_indices: &[u32],
    previous_state_id: i64,
    target_body: i64,
    operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    recipes: &[crate::records::recipes::ConstructionRecipe],
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    macro_rules! required {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    type FamilyKey<'a> = (
        &'a crate::records::mesh::DesignRelaxedGuidText,
        &'a crate::records::mesh::DesignRelaxedGuidText,
        u64,
        u32,
        &'a str,
    );
    type FamilyMember<'a> = (u32, Option<i64>, &'a [i64]);
    let (stream, scope_record_index) = scope_key;

    if tool_record_indices.is_empty() {
        return Ok(None);
    }
    let mut seen = HashSet::new();
    for index in ctx.admit_iter(tool_record_indices, "scan F3D Combine tool record indices")? {
        if !ctx.insert_hash_set(&mut seen, *index, "index F3D Combine tool record indices")? {
            return Ok(None);
        }
    }
    let mut recipes_by_id_storage = ctx.reserve_scoped(0, "index F3D Combine recipes")?;
    let mut recipes_by_id =
        HashMap::<&str, Option<&crate::records::recipes::ConstructionRecipe>>::new();
    for recipe in ctx.admit_iter(recipes, "scan F3D Combine recipes")? {
        if recipe.kind != crate::records::recipes::ConstructionRecipeKind::Body {
            continue;
        }
        let Some(recipe_stream) = crate::ids::native_stream(&recipe.id) else {
            continue;
        };
        if !ctx.equal(recipe_stream, stream, "compare F3D Combine recipe streams")? {
            continue;
        }
        recipes_by_id_storage.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
            match ctx.entry_hash_map(
                &mut recipes_by_id,
                recipe.id.as_str(),
                "index F3D Combine recipes",
            )? {
                std::collections::hash_map::Entry::Occupied(mut entry) => *entry.get_mut() = None,
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(recipe));
                }
            }
            Ok(())
        })?;
    }
    let mut families = BTreeMap::<FamilyKey<'_>, Vec<FamilyMember<'_>>>::new();
    for record_index in
        ctx.admit_iter(tool_record_indices, "scan F3D Combine tool record indices")?
    {
        let mut operand_matches =
            |operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand| {
                let Some(operand_stream) = crate::ids::native_stream(&operand.id) else {
                    return Ok(false);
                };
                Ok(ctx.equal(
                    operand_stream,
                    stream,
                    "compare F3D Combine operand streams",
                )? && operand.scope_record_index == scope_record_index
                    && matches!(
                        operand.owner,
                        crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
                    )
                    && operand.record_index() == *record_index)
            };
        let Some(operand_index) = ctx.position_by(
            operands,
            &mut operand_matches,
            "find F3D Combine tool operand",
        )?
        else {
            return Ok(None);
        };
        if ctx
            .position_by(
                &operands[operand_index + 1..],
                &mut operand_matches,
                "find additional F3D Combine tool operand",
            )?
            .is_some()
            || operands[operand_index].references().len() != 1
        {
            return Ok(None);
        }
        let operand = &operands[operand_index];
        let recipe = required!(ctx.get_hash_map(
            &recipes_by_id,
            operand.recipe_id.as_str(),
            "find F3D Combine recipe",
        )?);
        let recipe = required!(*recipe);
        let design = required!(recipe.design.as_ref());
        let selector = required!(design.selector).value;
        if selector == 0 {
            return Ok(None);
        }
        let resolved = match (operand.resolved_body_state_id, operand.resolved_body_slot) {
            (Some(state), Some(body)) if state == previous_state_id => Some(body),
            (None, None) => None,
            _ => return Ok(None),
        };
        let reference = &operand.references()[0];
        let key = (
            &operand.asset_id,
            &operand.context_id,
            reference.design_reference,
            reference.form,
            design.id.value.as_str(),
        );
        ctx.push_btree_group(
            &mut families,
            key,
            (
                selector,
                resolved,
                reference.preceding_body_slots.as_slice(),
            ),
            "index F3D Combine tool families",
            "collect F3D Combine family members",
        )?;
    }

    let mut selected = BTreeSet::new();
    for family in families.into_values() {
        let mut exact = BTreeSet::new();
        for body in family.iter().filter_map(|(_, body, _)| *body) {
            ctx.insert_btree_set(&mut exact, body, "index F3D Combine exact tool bodies")
                .map(|_| ())?;
        }
        if family.iter().all(|(_, body, _)| body.is_some()) {
            if exact.len() != family.len() {
                return Ok(None);
            }
            for body in exact {
                ctx.insert_btree_set(&mut selected, body, "index F3D Combine selected tools")
                    .map(|_| ())?;
            }
            continue;
        }
        let mut selectors = BTreeSet::new();
        for (selector, _, _) in ctx.admit_iter(&family, "scan F3D family")? {
            ctx.insert_btree_set(&mut selectors, *selector, "index F3D Combine selectors")?;
        }
        let count = required!(u32::try_from(family.len()).ok());
        if selectors.len() != family.len()
            || !(1..=count).all(|selector| selectors.contains(&selector))
        {
            return Ok(None);
        }
        let mut candidate_sets = family
            .iter()
            .map(|(_, _, candidates)| candidates)
            .filter(|candidates| !candidates.is_empty());
        let candidate_slice = required!(candidate_sets.next());
        let mut candidates = BTreeSet::new();
        for candidate in *candidate_slice {
            ctx.insert_btree_set(
                &mut candidates,
                *candidate,
                "index F3D Combine candidate tools",
            )
            .map(|_| ())?;
        }
        if candidates.len() != family.len()
            || candidates.contains(&target_body)
            || !exact.is_subset(&candidates)
            || candidate_sets.any(|other| {
                other.iter().any(|body| !candidates.contains(body))
                    || candidates.iter().any(|body| !other.contains(body))
            })
        {
            return Ok(None);
        }
        for body in candidates {
            ctx.insert_btree_set(&mut selected, body, "index F3D Combine selected tools")
                .map(|_| ())?;
        }
    }
    if selected.len() != tool_record_indices.len() || selected.contains(&target_body) {
        return Ok(None);
    }
    Ok(Some(ctx.collect_vec(
        selected,
        "collect F3D Combine recipe tool slots",
    )?))
}

fn combine_external_local_tools(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Result<Option<cadmpeg_ir::features::BodySelection>, cadmpeg_core::CodecError> {
    let Some(operation) = scope.combine_operation() else {
        return Ok(None);
    };
    let mut bodies = Vec::new();
    for tool in ctx
        .admit_iter(
            std::slice::from_ref(&operation.tools.first),
            "scan F3D operation tools",
        )?
        .chain(ctx.admit_iter(&operation.tools.additional, "scan F3D operation tools")?)
    {
        let Some(identity) = tool.external_identity.as_ref() else {
            return Ok(None);
        };
        let id = crate::ids::neutral_combine_external_body_id_charged(ctx, identity)?;

        if ctx.any_by(
            &bodies,
            |body| ctx.equal(body, &id, "compare F3D Combine external tool identities"),
            "scan F3D Combine external tool identities",
        )? {
            return Ok(None);
        }
        ctx.push_vec(&mut bodies, id, "collect F3D Combine external tools")?;
    }
    let native = ctx.copy_retained_text(&scope.id, "copy F3D Combine scope identity")?;
    Ok(cadmpeg_ir::features::BodySelection::local(bodies, native, ctx)?.ok())
}

fn historical_body_slot(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    id: &str,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(id) = ctx.strip_prefix(
        id,
        "f3d:history-input:body#",
        "strip F3D historical body identity prefix",
    )?
    else {
        return Ok(None);
    };
    let Some((_, slot)) = ctx.rsplit_once(id, ":", "split F3D historical body slot")? else {
        return Ok(None);
    };
    Ok(ctx
        .parse_text::<i64>(slot, "parse F3D historical body slot")?
        .ok())
}

fn bind_pattern_body_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{
        patterns::PatternSeed, BodySelection, FeatureDefinition, FeatureOperation,
    };

    let scopes = inputs.scopes;
    let groups = inputs.groups;
    let body_recipe_operands = inputs.body_recipe_operands;

    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let stream = crate::ids::native_stream(&scope.id);
        let mut matching_groups = groups.iter().filter(|group| {
            group.scope_record_index == scope.record_index
                && group.role() == DesignOperandRole::BODIES_B
                && !group.members().is_empty()
                && crate::ids::native_stream(&group.id) == stream
        });
        let Some(group) = matching_groups.next() else {
            continue;
        };
        if matching_groups.next().is_some() {
            continue;
        }
        let mut seed = if seeds.is_empty() {
            Some(PatternSeed::Bodies(BodySelection::Native(
                ctx.copy_retained_text(&group.id, "copy F3D pattern seed native identity")?,
            )))
        } else {
            None
        };
        let feature_id = &feature.id;
        let mut reserve_error = None;
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = definition
            else {
                return;
            };
            if let Some(seed) = seed.take() {
                if let Err(error) = ctx.push_vec(seeds, seed, "collect F3D pattern body seeds") {
                    reserve_error = Some(error);
                    return;
                }
            }
            let [PatternSeed::Bodies(selection)] = seeds.as_mut_slice() else {
                return;
            };
            if let Some(previous_state_id) = scope.previous_history_state_id() {
                reserve_error = bind_body_recipe_body_selection(
                    ctx,
                    selection,
                    feature_id,
                    previous_state_id,
                    scope,
                    groups,
                    body_recipe_operands,
                )
                .err();
            } else {
                reserve_error =
                    bind_direct_body_recipe_body_selection(ctx, selection, scope, inputs).err();
            }
        });
        if let Some(error) = reserve_error {
            return Err(error);
        }
    }
    Ok(())
}

fn unique_external_body_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    current_history_source: Option<&str>,
    bodies: &[cadmpeg_ir::topology::Body],
    regions: &[cadmpeg_ir::topology::Region],
    shells: &[cadmpeg_ir::topology::Shell],
) -> Result<Option<cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
    let body_by_region = ctx.collect_hash_map(
        regions.iter().map(|region| (&region.id, &region.body)),
        "index F3D external body regions",
    )?;
    let mut body_by_face_storage = ctx.reserve_scoped(0, "index F3D external body faces")?;
    let mut body_by_face = HashMap::new();
    for shell in ctx.admit_iter(shells, "scan F3D external body shells")? {
        let Some(body) = ctx.get_hash_map(
            &body_by_region,
            &shell.region,
            "find F3D external shell body",
        )?
        else {
            continue;
        };
        for face in ctx.admit_iter(shell.faces(), "scan F3D external shell faces")? {
            body_by_face_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut body_by_face,
                    face,
                    *body,
                    "index F3D external body faces",
                )
                .map(|_| ())
            })?;
        }
    }
    let body_metadata = ctx.collect_hash_map(
        bodies.iter().map(|body| (&body.id, body)),
        "index F3D external body metadata",
    )?;
    let current_prefix = current_history_source
        .map(|source| {
            ctx.format_retained(
                format_args!("f3d:brep/{source}/"),
                "retain F3D current history prefix",
            )
        })
        .transpose()?;
    let mut candidates: Option<BTreeSet<cadmpeg_ir::ids::BodyId>> = None;
    for reference in ctx.admit_iter(operand.references(), "scan F3D external body references")? {
        let mut reference_candidates = BTreeSet::new();
        for face in ctx.admit_iter(
            &reference.candidate_faces,
            "scan F3D reference candidate faces",
        )? {
            let Some(body) = ctx
                .get_hash_map(&body_by_face, face, "find F3D external face body")?
                .copied()
            else {
                continue;
            };
            if let Some(prefix) = current_prefix.as_ref() {
                if ctx.starts_with(
                    body.as_str(),
                    prefix,
                    "check F3D external body history prefix",
                )? {
                    continue;
                }
            }
            if !ctx.contains_btree_set(
                &reference_candidates,
                body,
                "check F3D external body candidate",
            )? {
                let id = body.try_clone_for_decode(ctx, "copy F3D external body candidate")?;
                ctx.insert_btree_set(
                    &mut reference_candidates,
                    id,
                    "collect F3D external body candidates",
                )?;
            }
        }
        if let Some(candidates) = &mut candidates {
            if reference_candidates.is_empty() {
                return Ok(None);
            }
            let mut refusal = None;
            candidates.retain(|body| {
                if refusal.is_some() {
                    return true;
                }
                match ctx.contains_btree_set(
                    &reference_candidates,
                    body,
                    "intersect F3D external body candidates",
                ) {
                    Ok(contains) => contains,
                    Err(error) => {
                        refusal = Some(error);
                        true
                    }
                }
            });
            if let Some(error) = refusal {
                return Err(error);
            }
        } else {
            candidates = Some(reference_candidates);
        }
    }
    let Some(mut candidates) = candidates else {
        return Ok(None);
    };
    let mut displayed = BTreeSet::new();
    for body in ctx.admit_iter(&candidates, "scan F3D displayed external body candidates")? {
        let Some(metadata) =
            ctx.get_hash_map(&body_metadata, &body, "find F3D external body metadata")?
        else {
            continue;
        };
        if metadata.visible != Some(true) {
            continue;
        }
        let id = body.try_clone_for_decode(ctx, "copy F3D displayed external body")?;
        ctx.insert_btree_set(&mut displayed, id, "collect F3D displayed external bodies")?;
    }
    if !displayed.is_empty() {
        candidates = displayed;
    }
    if candidates.len() == 1 {
        Ok(candidates.into_iter().next())
    } else {
        Ok(None)
    }
}

fn bind_body_recipe_body_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::BodySelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::BodySelection;

    let BodySelection::Native(group_id) = selection else {
        return Ok(());
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == *group_id
            && group.scope_record_index == scope.record_index
            && matches!(
                group.role(),
                DesignOperandRole::BODIES_A
                    | DesignOperandRole::ROLE_0X5
                    | DesignOperandRole::BODIES_B
            )
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return Ok(());
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return Ok(());
    }
    let mut body_slots = Vec::new();
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| member.value)
        .enumerate()
    {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return Ok(());
        };
        let mut matching_operands = operands.iter().filter(|operand| {
            operand.owner.group() == Some((group.record_index, ordinal))
                && operand.record_index() == record_index
                && crate::ids::native_stream(&operand.id) == stream
        });
        let Some(operand) = matching_operands.next() else {
            return Ok(());
        };
        if matching_operands.next().is_some() {
            return Ok(());
        }
        let Some(body_slot) = operand.resolved_body_slot else {
            return Ok(());
        };
        if !body_slots.contains(&body_slot) {
            ctx.reserve_vec(&mut body_slots, 1, "collect F3D body recipe slots")?;
            body_slots.push(body_slot);
        }
    }

    let mut body_ids = Vec::new();
    ctx.reserve_vec(
        &mut body_ids,
        body_slots.len(),
        "collect F3D body recipe identities",
    )?;
    for slot in body_slots {
        body_ids.push(crate::ids::history_input_body_id_charged(
            ctx,
            feature_id,
            previous_state_id,
            slot,
        )?);
    }
    let state = crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
    let native = ctx.copy_retained_text(&group.id, "copy F3D body recipe group identity")?;
    if let Ok(historical) =
        cadmpeg_ir::features::BodySelection::historical(state, body_ids, native, ctx)?
    {
        *selection = historical;
    }
    Ok(())
}

fn bind_direct_body_recipe_body_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::BodySelection,
    scope: &crate::records::feature::scope::DesignParameterScope,
    inputs: &FeatureBodySelectionInputs<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{BodyMember, BodySelection};

    let groups = inputs.groups;
    let operands = inputs.body_recipe_operands;
    let construction_recipes = inputs.construction_recipes;
    let persistent_design_links = inputs.persistent_design_links;
    let bodies = inputs.bodies;
    let regions = inputs.regions;
    let shells = inputs.shells;

    let stream = crate::ids::native_stream(&scope.id);
    let native_members = match selection {
        BodySelection::Native(group_id) => {
            let mut matching_groups = groups.iter().filter(|group| {
                group.id == *group_id
                    && group.scope_record_index == scope.record_index
                    && matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::ROLE_0X5
                            | DesignOperandRole::BODIES_B
                    )
                    && crate::ids::native_stream(&group.id) == stream
            });
            let Some(group) = matching_groups.next() else {
                return Ok(());
            };
            if matching_groups.next().is_some() || group.members().is_empty() {
                return Ok(());
            }
            let mut selected = Vec::new();
            for (ordinal, record_index) in group
                .members()
                .iter()
                .map(|member| member.value)
                .enumerate()
            {
                let Ok(ordinal) = u32::try_from(ordinal) else {
                    return Ok(());
                };
                let mut matching_operands = operands.iter().filter(|operand| {
                    operand.owner.group() == Some((group.record_index, ordinal))
                        && operand.record_index() == record_index
                        && crate::ids::native_stream(&operand.id) == stream
                });
                let Some(operand) = matching_operands.next() else {
                    return Ok(());
                };
                if matching_operands.next().is_some() {
                    return Ok(());
                }
                let Some(body) = direct_body_recipe_candidate(
                    ctx,
                    operand,
                    construction_recipes,
                    persistent_design_links,
                    bodies,
                    regions,
                    shells,
                )?
                else {
                    return Ok(());
                };
                if selected.contains(&body) {
                    return Ok(());
                }

                ctx.reserve_vec(
                    &mut selected,
                    1,
                    "collect F3D direct body recipe selections",
                )?;
                selected.push(body);
            }
            let bodies = match cadmpeg_ir::features::DistinctMembers::try_from(selected, ctx) {
                Ok(value) => value,
                Err(cadmpeg_ir::features::FeatureCollectionError::Invalid(_)) => return Ok(()),
                Err(cadmpeg_ir::features::FeatureCollectionError::Resource(limit)) => {
                    return Err(limit.into())
                }
            };
            let native =
                ctx.copy_retained_text(&group.id, "copy F3D direct body recipe group identity")?;
            *selection = BodySelection::Resolved { bodies, native };
            return Ok(());
        }
        BodySelection::NativeSet(native) => ctx.collect_vec(
            native.iter().map(String::as_str),
            "collect F3D direct body recipe native members",
        )?,
        _ => return Ok(()),
    };
    if native_members.is_empty() {
        return Ok(());
    }
    for (index, native) in ctx
        .admit_iter(
            &native_members,
            "scan F3D direct body recipe native members",
        )?
        .enumerate()
    {
        for previous in ctx.admit_iter(
            &native_members[..index],
            "scan previous F3D direct body recipe native members",
        )? {
            if ctx.equal(
                native,
                previous,
                "compare F3D direct body recipe native members",
            )? {
                return Ok(());
            }
        }
    }
    let mut rows = Vec::new();
    for native in ctx.admit_iter(&native_members, "scan F3D native members")? {
        let Some((native_stream_name, record_index)) = ctx.rsplit_once(
            native,
            ":design-record#",
            "split F3D direct body recipe native identity",
        )?
        else {
            return Ok(());
        };
        let Ok(record_index) =
            ctx.parse_text::<u32>(record_index, "parse F3D direct body recipe record index")?
        else {
            return Ok(());
        };
        let Some(stream) = stream else {
            return Ok(());
        };
        if !ctx.equal(
            native_stream_name,
            stream,
            "compare F3D direct body recipe stream identity",
        )? {
            return Ok(());
        }
        let matches_operand =
            |operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand| {
                let Some(operand_stream) = crate::ids::native_stream(&operand.id) else {
                    return Ok(false);
                };
                if !ctx.equal(
                    operand_stream,
                    native_stream_name,
                    "compare F3D direct body recipe operand stream",
                )? {
                    return Ok(false);
                }
                Ok(operand.scope_record_index == scope.record_index
                    && matches!(
                        operand.owner,
                        crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
                    )
                    && operand.record_index() == record_index)
            };
        let Some(operand_index) = ctx.position_by(
            operands,
            matches_operand,
            "find F3D direct body recipe operand",
        )?
        else {
            return Ok(());
        };
        if ctx
            .position_by(
                &operands[operand_index + 1..],
                matches_operand,
                "find duplicate F3D direct body recipe operand",
            )?
            .is_some()
        {
            return Ok(());
        }
        let operand = &operands[operand_index];
        let Some(body) = direct_body_recipe_candidate(
            ctx,
            operand,
            construction_recipes,
            persistent_design_links,
            bodies,
            regions,
            shells,
        )?
        else {
            return Ok(());
        };
        if rows.iter().any(|row: &BodyMember<_>| row.body() == &body) {
            return Ok(());
        }
        let native =
            ctx.copy_retained_text(native, "copy F3D direct body recipe member identity")?;
        let Some(row) = body_member(ctx, body, native)? else {
            return Ok(());
        };

        ctx.reserve_vec(&mut rows, 1, "collect F3D direct body recipe rows")?;
        rows.push(row);
    }
    if let Ok(members) = cadmpeg_ir::features::BodyMembers::try_from_rows(rows, ctx)? {
        *selection = BodySelection::ResolvedSet { members };
    }
    Ok(())
}

fn direct_body_recipe_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    construction_recipes: &[crate::records::recipes::ConstructionRecipe],
    persistent_design_links: &[crate::records::sketch_links::PersistentDesignLink],
    bodies: &[cadmpeg_ir::topology::Body],
    regions: &[cadmpeg_ir::topology::Region],
    shells: &[cadmpeg_ir::topology::Shell],
) -> Result<Option<cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
    if let Some(body) = body_recipe_link_candidate(
        ctx,
        operand,
        construction_recipes,
        persistent_design_links,
        bodies,
    )? {
        let (any_candidate, contains_body) =
            body_recipe_face_body_candidates(ctx, operand, &body, bodies, regions, shells)?;
        if !any_candidate || contains_body {
            return Ok(Some(body));
        }
        return Ok(None);
    }
    unique_external_body_candidate(ctx, operand, None, bodies, regions, shells)
}

fn body_recipe_link_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    construction_recipes: &[crate::records::recipes::ConstructionRecipe],
    persistent_design_links: &[crate::records::sketch_links::PersistentDesignLink],
    bodies: &[cadmpeg_ir::topology::Body],
) -> Result<Option<cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let Some(stream) = crate::ids::native_stream(&operand.id) else {
        return Ok(None);
    };
    let mut matches_recipe = |recipe: &crate::records::recipes::ConstructionRecipe| {
        if !ctx.equal(
            &recipe.id,
            &operand.recipe_id,
            "compare F3D body recipe identity",
        )? {
            return Ok(false);
        }
        if recipe.kind != crate::records::recipes::ConstructionRecipeKind::Body {
            return Ok(false);
        }
        ctx.equal(
            &crate::ids::native_stream(&recipe.id),
            &Some(stream),
            "compare F3D body recipe stream",
        )
    };
    let Some(recipe_index) = ctx.position_by(
        construction_recipes,
        &mut matches_recipe,
        "scan F3D body recipe candidates",
    )?
    else {
        return Ok(None);
    };
    let recipe = &construction_recipes[recipe_index];
    if ctx
        .position_by(
            &construction_recipes[recipe_index + 1..],
            &mut matches_recipe,
            "scan F3D duplicate body recipe candidates",
        )?
        .is_some()
    {
        return Ok(None);
    }
    let Some(design) = recipe.design.as_ref() else {
        return Ok(None);
    };
    let design_id = design.id.value.as_str();
    let Some(selector) = design.selector else {
        return Ok(None);
    };
    let selector = i64::from(selector.value);
    let mut matching_body: Option<&cadmpeg_ir::ids::BodyId> = None;
    for (index, link) in persistent_design_links.iter().enumerate() {
        ctx.charge_work(1, "resolve F3D persistent body link")?;
        if !ctx.equal(
            link.design_id.as_str(),
            design_id,
            "compare F3D persistent design identifier",
        )? || link.design_reference != selector
        {
            continue;
        }
        let mut superseded = false;
        for (other_index, other) in ctx
            .admit_iter(
                persistent_design_links,
                "scan F3D persistent body link versions",
            )?
            .enumerate()
        {
            let same_target = match (&other.target, &link.target) {
                (AttributeTarget::Document, AttributeTarget::Document) => true,
                (AttributeTarget::Body(left), AttributeTarget::Body(right)) => {
                    ctx.equal(left, right, "compare F3D persistent body link target")?
                }
                (AttributeTarget::Face(left), AttributeTarget::Face(right)) => {
                    ctx.equal(left, right, "compare F3D persistent body link target")?
                }
                (AttributeTarget::Shell(left), AttributeTarget::Shell(right)) => {
                    ctx.equal(left, right, "compare F3D persistent body link target")?
                }
                (AttributeTarget::Loop(left), AttributeTarget::Loop(right)) => {
                    ctx.equal(left, right, "compare F3D persistent body link target")?
                }
                (AttributeTarget::Coedge(left), AttributeTarget::Coedge(right)) => {
                    ctx.equal(left, right, "compare F3D persistent body link target")?
                }
                (AttributeTarget::Edge(left), AttributeTarget::Edge(right)) => {
                    ctx.equal(left, right, "compare F3D persistent body link target")?
                }
                (AttributeTarget::Vertex(left), AttributeTarget::Vertex(right)) => {
                    ctx.equal(left, right, "compare F3D persistent body link target")?
                }
                _ => false,
            };
            if same_target
                && (other.ordinal > link.ordinal
                    || (other.ordinal == link.ordinal && other_index < index))
            {
                superseded = true;
                break;
            }
        }
        if superseded {
            continue;
        }
        let AttributeTarget::Body(body) = &link.target else {
            continue;
        };
        if ctx.any_by(
            bodies,
            |candidate| {
                ctx.equal(
                    &candidate.id,
                    body,
                    "compare F3D persistent body candidate identity",
                )
            },
            "scan F3D persistent body candidates",
        )? {
            if let Some(existing) = matching_body {
                if !ctx.equal(existing, body, "compare F3D selected persistent body")? {
                    return Ok(None);
                }
            }
            matching_body = Some(body);
        }
    }
    let Some(body) = matching_body else {
        return Ok(None);
    };
    Ok(Some(body.try_clone_for_decode(
        ctx,
        "copy F3D persistent body link identity",
    )?))
}

fn body_recipe_face_body_candidates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    selected: &cadmpeg_ir::ids::BodyId,
    bodies: &[cadmpeg_ir::topology::Body],
    regions: &[cadmpeg_ir::topology::Region],
    shells: &[cadmpeg_ir::topology::Shell],
) -> Result<(bool, bool), cadmpeg_core::CodecError> {
    let mut any_candidate = false;
    let mut contains_selected = false;
    for reference in ctx.admit_iter(operand.references(), "scan F3D body recipe references")? {
        for face in ctx.admit_iter(
            &reference.candidate_faces,
            "scan F3D body recipe candidate faces",
        )? {
            let mut mapped_body = None;
            for shell in shells.iter().rev() {
                ctx.charge_work(1, "resolve F3D body recipe face carrier")?;
                ctx.charge_work(
                    u64::try_from(shell.faces().len()).map_err(|_| {
                        ctx.refuse_codec_limit("resolve F3D body recipe face carrier", 0, u64::MAX)
                    })?,
                    "resolve F3D body recipe face carrier",
                )?;
                if !shell.faces().contains(face) {
                    continue;
                }
                for region in regions.iter().rev() {
                    ctx.charge_work(1, "resolve F3D body recipe face carrier")?;
                    if ctx.equal(&region.id, &shell.region, "compare F3D body recipe region")? {
                        mapped_body = Some(&region.body);
                        break;
                    }
                }
                if mapped_body.is_some() {
                    break;
                }
            }
            let Some(body) = mapped_body else {
                continue;
            };
            ctx.charge_work(
                u64::try_from(bodies.len()).map_err(|_| {
                    ctx.refuse_codec_limit("resolve F3D body recipe face carrier", 0, u64::MAX)
                })?,
                "resolve F3D body recipe face carrier",
            )?;
            if bodies.iter().any(|candidate| candidate.id == *body) {
                any_candidate = true;
                contains_selected |=
                    ctx.equal(body, selected, "compare F3D selected body recipe candidate")?;
            }
        }
    }
    Ok((any_candidate, contains_selected))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodySelectionProof {
    TopologyStableRevision,
    RevisedInput,
}

fn unique_feature_history_states<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &'a AsmHistory,
) -> Result<HashMap<i64, Option<&'a AsmDeltaState>>, cadmpeg_core::CodecError> {
    let mut states = HashMap::new();
    for state in ctx.admit_iter(&history.states, "scan F3D history states")? {
        if !states.contains_key(&state.state_id) {
            ctx.reserve_map(&mut states, 1, "index F3D feature history states")?;
        }
        states
            .entry(state.state_id)
            .and_modify(|existing| *existing = None)
            .or_insert(Some(state));
    }
    Ok(states)
}

fn singleton_revised_input_body_across_state_chain<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut revised = BTreeSet::new();
    while current.state_id != previous_state_id {
        if !ctx.insert_hash_set(
            &mut visited,
            current.state_id,
            "visit F3D revised input body states",
        )? {
            return Ok(None);
        }
        let Some(transition) = current.transition.as_ref() else {
            return Ok(None);
        };
        for &body in transition
            .topology
            .bodies
            .updated
            .iter()
            .chain(&transition.topology.bodies.deleted)
        {
            ctx.insert_btree_set(&mut revised, body, "index F3D revised input bodies")
                .map(|_| ())?;
        }
        let Some(previous_id) = transition.previous_state_id else {
            return Ok(None);
        };
        let Some(Some(previous)) = states.get(&previous_id) else {
            return Ok(None);
        };
        current = previous;
    }
    let Some(input) = current.topology() else {
        return Ok(None);
    };
    let mut candidates = input.bodies.iter().filter(|body| revised.contains(body));
    let Some(body) = candidates.next().copied() else {
        return Ok(None);
    };
    Ok(candidates.next().is_none().then_some(body))
}

fn singleton_body_revision_across_state_chain<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(result_topology) = state.topology() else {
        return Ok(None);
    };
    let mut current = state;
    let mut visited = HashSet::new();
    let mut selected = None;
    while current.state_id != previous_state_id {
        if !ctx.insert_hash_set(
            &mut visited,
            current.state_id,
            "visit F3D stable body revision states",
        )? {
            return Ok(None);
        }
        let Some(revision) = body_revision_without_topology_change(current) else {
            return Ok(None);
        };
        if let TopologyStableBodyRevision::Revised(body) = revision {
            match selected {
                None => selected = Some(body),
                Some(selected) if selected == body => {}
                Some(_) => return Ok(None),
            }
        }
        let Some(previous) = current
            .transition
            .as_ref()
            .and_then(|transition| transition.previous_state_id)
        else {
            return Ok(None);
        };
        let Some(Some(previous)) = states.get(&previous) else {
            return Ok(None);
        };
        current = previous;
    }
    let Some(body) = selected else {
        return Ok(None);
    };
    let Some(input_topology) = current.topology() else {
        return Ok(None);
    };
    Ok(
        (result_topology.bodies.contains(&body) && input_topology.bodies.contains(&body))
            .then_some(body),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TopologyStableBodyRevision {
    Unchanged,
    Revised(i64),
}

fn body_revision_without_topology_change(
    current: &AsmDeltaState,
) -> Option<TopologyStableBodyRevision> {
    let transition = current.transition.as_ref()?;
    let delta = &transition.topology;
    let body = match delta.bodies.updated.as_slice() {
        [] => TopologyStableBodyRevision::Unchanged,
        [body] => TopologyStableBodyRevision::Revised(*body),
        _ => return None,
    };
    if !delta.bodies.inserted.is_empty()
        || !delta.bodies.deleted.is_empty()
        || [
            &delta.regions,
            &delta.shells,
            &delta.faces,
            &delta.loops,
            &delta.coedges,
            &delta.edges,
            &delta.vertices,
        ]
        .into_iter()
        .any(|family| {
            !family.inserted.is_empty() || !family.deleted.is_empty() || !family.updated.is_empty()
        })
    {
        return None;
    }
    Some(body)
}

#[derive(Clone, Copy)]
pub(crate) struct FeatureFaceSelectionInputs<'a> {
    pub(crate) scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    pub(crate) groups:
        &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
    pub(crate) operands: &'a [crate::records::topology::face::DesignFaceOperand],
    pub(crate) entity_operands:
        &'a [crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    pub(crate) body_recipe_operands:
        &'a [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    pub(crate) histories: &'a [AsmHistory],
}

pub(crate) fn bind_feature_face_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    input_topologies: &mut [cadmpeg_ir::features::FeatureInputTopology],
    input: FeatureFaceSelectionInputs<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let FeatureFaceSelectionInputs {
        scopes,
        groups,
        operands,
        entity_operands,
        body_recipe_operands,
        histories,
    } = input;
    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            macro_rules! admitted {
                ($value:expr) => {
                    if let Err(error) = $value {
                        edit_result = Err(error);
                        return;
                    }
                };
            }
            'feature_edit: {
                let Some(native_ref) = native_ref else {
                    break 'feature_edit;
                };
                let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
                let Some(scope) = matching_scopes.next() else {
                    break 'feature_edit;
                };
                if matching_scopes.next().is_some() {
                    break 'feature_edit;
                }
                let Some(state_id) = scope.history_state_id() else {
                    break 'feature_edit;
                };
                let previous_state_id =
                    match effective_scope_previous_history_state_id(ctx, scope, histories) {
                        Ok(previous_state_id) => previous_state_id,
                        Err(error) => {
                            edit_result = Err(error);
                            break 'feature_edit;
                        }
                    };
                let Some(previous_state_id) = previous_state_id else {
                    break 'feature_edit;
                };
                let history_state_pair =
                    match unique_history_state_pair(ctx, histories, state_id, previous_state_id) {
                        Ok(history_state_pair) => history_state_pair,
                        Err(error) => {
                            edit_result = Err(error);
                            break 'feature_edit;
                        }
                    };
                let Some((history, state, previous)) = history_state_pair else {
                    break 'feature_edit;
                };
                let Some(transition) = &state.transition else {
                    break 'feature_edit;
                };
                if transition.previous_state_id != Some(previous_state_id) {
                    break 'feature_edit;
                }
                let Some(_topology) = previous.topology() else {
                    break 'feature_edit;
                };
                match definition {
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Extrude { start, extent, .. },
                    ) => {
                        if let cadmpeg_ir::features::ExtrudeStart::FromFace { face, .. } = start {
                            admitted!(selection::bind_face_selection(
                                ctx,
                                face,
                                scope,
                                groups,
                                operands,
                                &transition.topology.faces.updated,
                            ));
                            admitted!(bind_entity_face_selection(
                                ctx,
                                face,
                                FaceSelectionBinding {
                                    feature_id,
                                    previous_state_id,
                                    operation_history_id: &history.id,
                                    scope,
                                    input_topologies: &mut *input_topologies
                                },
                                groups,
                                entity_operands,
                            ));
                        }
                        let sides = match extent {
                            cadmpeg_ir::features::ExtrudeExtent::OneSided { side }
                            | cadmpeg_ir::features::ExtrudeExtent::Symmetric { side } => vec![side],
                            cadmpeg_ir::features::ExtrudeExtent::TwoSided { first, second } => {
                                vec![first, second]
                            }
                        };
                        for side in sides {
                            if let cadmpeg_ir::features::LinearTermination::ToFace {
                                face, ..
                            } = &mut side.termination
                            {
                                admitted!(selection::bind_face_selection(
                                    ctx,
                                    face,
                                    scope,
                                    groups,
                                    operands,
                                    &transition.topology.faces.updated,
                                ));
                            }
                        }
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Pattern { seeds, .. },
                    ) => {
                        for seed in seeds {
                            let cadmpeg_ir::features::patterns::PatternSeed::Faces(faces) = seed
                            else {
                                continue;
                            };
                            admitted!(selection::bind_face_selection(
                                ctx,
                                faces,
                                scope,
                                groups,
                                operands,
                                &transition.topology.faces.updated,
                            ));
                            admitted!(bind_entity_face_selection(
                                ctx,
                                faces,
                                FaceSelectionBinding {
                                    feature_id,
                                    previous_state_id,
                                    operation_history_id: &history.id,
                                    scope,
                                    input_topologies: &mut *input_topologies
                                },
                                groups,
                                entity_operands,
                            ));
                        }
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::MoveFace { faces, .. },
                    ) => {
                        admitted!(selection::bind_face_selection(
                            ctx,
                            faces,
                            scope,
                            groups,
                            operands,
                            &transition.topology.faces.updated,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Thicken { faces, .. },
                    ) => {
                        admitted!(selection::bind_face_selection(
                            ctx,
                            faces,
                            scope,
                            groups,
                            operands,
                            &transition.topology.faces.updated,
                        ));
                        admitted!(selection::bind_body_recipe_face_selection(
                            ctx,
                            faces,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            body_recipe_operands,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::KnitSurface { faces, .. },
                    ) => {
                        admitted!(bind_surface_stitch_face_selection(
                            ctx,
                            faces,
                            FaceSelectionBinding {
                                feature_id,
                                previous_state_id,
                                operation_history_id: &history.id,
                                scope,
                                input_topologies: &mut *input_topologies
                            },
                            groups,
                            entity_operands,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::SplitFace { targets, .. },
                    ) => {
                        admitted!(selection::bind_face_selection(
                            ctx,
                            targets,
                            scope,
                            groups,
                            operands,
                            &transition.topology.faces.updated,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Hole {
                            face: Some(face), ..
                        },
                    ) => {
                        admitted!(bind_hole_face_selection(
                            ctx,
                            face,
                            FaceSelectionBinding {
                                feature_id,
                                previous_state_id,
                                operation_history_id: &history.id,
                                scope,
                                input_topologies: &mut *input_topologies
                            },
                        ));
                    }
                    _ => {}
                }
            }
        });
        edit_result?;
    }
    Ok(())
}

struct FaceSelectionBinding<'a> {
    feature_id: &'a cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    operation_history_id: &'a str,
    scope: &'a crate::records::feature::scope::DesignParameterScope,
    input_topologies: &'a mut [cadmpeg_ir::features::FeatureInputTopology],
}

fn bind_entity_face_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    input: FaceSelectionBinding<'_>,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FaceSelection;
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;

    let group_id = match selection {
        FaceSelection::Native(group_id) => group_id.as_str(),
        _ => return Ok(()),
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == group_id
            && group.scope_record_index == scope.record_index
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return Ok(());
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return Ok(());
    }
    bind_entity_face_groups(
        ctx,
        selection,
        &group.id,
        FaceSelectionBinding {
            feature_id,
            previous_state_id,
            operation_history_id,
            scope,
            input_topologies: &mut *input_topologies,
        },
        std::slice::from_ref(&group),
        operands,
    )
}

// Keep the serialized-selection context explicit at this boundary so every
// admission input remains visible to the strict all-members proof.
fn bind_surface_stitch_face_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    input: FaceSelectionBinding<'_>,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;
    let native_id = match selection {
        cadmpeg_ir::features::FaceSelection::Native(native_id) => native_id.as_str(),
        _ => return Ok(()),
    };
    if native_id != scope.id {
        return Ok(());
    }
    let Some(input_end) = scope.reference_members().len().checked_sub(2) else {
        return Ok(());
    };
    if input_end == 0 || !input_end.is_multiple_of(2) {
        return Ok(());
    }
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = ctx.collect_vec(
        groups.iter().filter(|group| {
            crate::ids::native_stream(&group.id) == stream
                && group.scope_record_index == scope.record_index
                && group.role() == DesignOperandRole::ROLE_0X5
                && group.extrude_role().is_none()
                && group.extrude_face_role().is_none()
        }),
        "collect F3D Stitch face groups",
    )?;
    ctx.stable_sort_by(
        &mut matching_groups,
        |value| &value.scope_reference_ordinal,
        Ord::cmp,
        "sort F3D Stitch face groups",
    )?;
    if matching_groups.len().checked_mul(2) != Some(input_end)
        || matching_groups
            .iter()
            .enumerate()
            .zip(
                scope
                    .reference_members()
                    .values()
                    .step_by(2)
                    .zip(scope.reference_members().values().skip(1).step_by(2)),
            )
            .any(|((ordinal, group), (group_reference, member_reference))| {
                u32::try_from(ordinal * 2) != Ok(group.scope_reference_ordinal)
                    || group.record_index != *group_reference
                    || !group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .eq([*member_reference])
            })
    {
        return Ok(());
    }
    bind_entity_face_groups(
        ctx,
        selection,
        &scope.id,
        FaceSelectionBinding {
            feature_id,
            previous_state_id,
            operation_history_id,
            scope,
            input_topologies: &mut *input_topologies,
        },
        &matching_groups,
        operands,
    )
}

// Keep the shared selection-binding inputs explicit; this helper is the
// single admission point for both one-group and SurfaceStitch selections.
fn bind_entity_face_groups(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    native_id: &str,
    input: FaceSelectionBinding<'_>,
    groups: &[&crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;

    if groups.is_empty() {
        return Ok(());
    }
    let mut selected = Vec::<(&str, i64, bool)>::new();
    let stream = crate::ids::native_stream(&scope.id);
    for group in ctx.admit_iter(groups, "scan F3D entity face groups")? {
        if group.members().is_empty() {
            return Ok(());
        }
        for (ordinal, record_index) in ctx
            .admit_iter(group.members(), "scan F3D entity face group members")?
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return Ok(());
            };
            let mut matches = operands.iter().filter(|operand| {
                operand.scope_record_index == scope.record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.record_index() == record_index
                    && crate::ids::native_stream(&operand.id) == stream
            });
            let Some(operand) = matches.next() else {
                return Ok(());
            };
            let [candidate] = operand.historical_face_candidates.as_slice() else {
                return Ok(());
            };
            if matches.next().is_some() {
                return Ok(());
            }
            let local = candidate.history_id == operation_history_id
                && candidate.historical.state_ids.contains(&previous_state_id);
            let Some(source) = historical_brep_source(ctx, &candidate.history_id)? else {
                return Ok(());
            };
            if !selected.contains(&(source, candidate.face_slot, local)) {
                ctx.reserve_vec(&mut selected, 1, "collect F3D entity face candidates")?;
                selected.push((source, candidate.face_slot, local));
            }
        }
    }
    let state_id = crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
    let mut topologies = input_topologies
        .iter_mut()
        .filter(|topology| topology.id == state_id && topology.input_of == *feature_id);
    let Some(topology) = topologies.next() else {
        return Ok(());
    };
    if topologies.next().is_some() {
        return Ok(());
    }
    let mut faces = Vec::new();
    for (source, face, local) in selected {
        if !local && (source.contains('#') || source.chars().any(char::is_whitespace)) {
            return Ok(());
        }
        let id = if local {
            crate::ids::history_input_face_id_charged(ctx, feature_id, previous_state_id, face)?
        } else {
            crate::ids::history_input_face_id_charged(
                ctx,
                feature_id,
                previous_state_id,
                format_args!("{}:{source}:{face}", source.len()),
            )?
        };

        ctx.reserve_vec(&mut faces, 1, "collect F3D historical entity faces")?;
        faces.push(id);
    }
    for face in ctx.admit_iter(&faces, "scan F3D faces")? {
        if !topology.faces.contains(face) {
            let retained =
                face.try_clone_for_decode(ctx, "copy F3D historical topology face identity")?;
            topology
                .faces
                .insert(ctx, retained, "index F3D historical entity faces")?;
        }
    }
    let native =
        ctx.copy_retained_text(native_id, "copy F3D historical face selection identity")?;
    if let Ok(historical) =
        cadmpeg_ir::features::FaceSelection::historical(state_id, faces, native, ctx)?
    {
        *selection = historical;
    }
    Ok(())
}

fn bind_hole_face_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    input: FaceSelectionBinding<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FaceSelection;
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;

    let FaceSelection::Native(native_id) = selection else {
        return Ok(());
    };
    let Some(construction) = scope.hole_construction() else {
        return Ok(());
    };
    let Some(face_selection) = &construction.face_selection else {
        return Ok(());
    };
    let [candidate] = face_selection.historical_face_candidates.as_slice() else {
        return Ok(());
    };
    let local = candidate.history_id == operation_history_id
        && candidate.historical.state_ids.contains(&previous_state_id);
    let Some(source) = historical_brep_source(ctx, &candidate.history_id)? else {
        return Ok(());
    };
    let state_id = crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
    let mut topologies = input_topologies
        .iter_mut()
        .filter(|topology| topology.id == state_id && topology.input_of == *feature_id);
    let Some(topology) = topologies.next() else {
        return Ok(());
    };
    if topologies.next().is_some() {
        return Ok(());
    }
    if !local && (source.contains('#') || source.chars().any(char::is_whitespace)) {
        return Ok(());
    }
    let face = if local {
        crate::ids::history_input_face_id_charged(
            ctx,
            feature_id,
            previous_state_id,
            candidate.face_slot,
        )?
    } else {
        crate::ids::history_input_face_id_charged(
            ctx,
            feature_id,
            previous_state_id,
            format_args!("{}:{source}:{}", source.len(), candidate.face_slot),
        )?
    };
    if !topology.faces.contains(&face) {
        let retained = face.try_clone_for_decode(ctx, "copy F3D historical hole topology face")?;
        topology
            .faces
            .insert(ctx, retained, "index F3D historical hole face")?;
    }
    let native = ctx.copy_retained_text(native_id, "copy F3D historical hole identity")?;
    let mut selected = ctx.collection_vec(1, "validate F3D historical hole face")?;
    selected.push(face);
    if let Ok(historical) =
        cadmpeg_ir::features::FaceSelection::historical(state_id, selected, native, ctx)?
    {
        *selection = historical;
    }
    Ok(())
}

pub(crate) fn bind_feature_path_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SurfaceBoundary};

    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| 'feature_edit: {
            let Some(native_ref) = native_ref else {
                break 'feature_edit;
            };
            let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
            let Some(scope) = matching_scopes.next() else {
                break 'feature_edit;
            };
            if matching_scopes.next().is_some() {
                break 'feature_edit;
            }
            let Some(previous_state_id) = scope.previous_history_state_id() else {
                break 'feature_edit;
            };
            match definition {
                FeatureDefinition::Operation(FeatureOperation::FilledSurface {
                    boundary: SurfaceBoundary::Path(path),
                    ..
                }) => {
                    edit_result = bind_entity_selection_path(
                        ctx,
                        path,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        operands,
                    );
                }
                FeatureDefinition::Operation(FeatureOperation::Loft { guidance, .. }) => {
                    let paths = match guidance {
                        cadmpeg_ir::features::LoftGuidance::Guides(paths) => paths,
                        cadmpeg_ir::features::LoftGuidance::Centerline(path) => {
                            std::slice::from_mut(path)
                        }
                    };
                    for path in paths {
                        edit_result = bind_entity_selection_path(
                            ctx,
                            path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                        if edit_result.is_err() {
                            break;
                        }
                    }
                }
                FeatureDefinition::Operation(FeatureOperation::Sweep {
                    path, guide_rail, ..
                }) => {
                    if let Some(path) = path {
                        edit_result = bind_entity_selection_path(
                            ctx,
                            path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                        if edit_result.is_err() {
                            break 'feature_edit;
                        }
                    }
                    if let Some(guide_rail) = guide_rail {
                        edit_result = bind_entity_selection_path(
                            ctx,
                            &mut guide_rail.path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                    }
                }
                _ => {}
            }
        });
        edit_result?;
    }
    Ok(())
}

fn bind_entity_selection_path(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    path: &mut cadmpeg_ir::features::PathRef,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::PathRef;

    let PathRef::Native(group_id) = path else {
        return Ok(());
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == *group_id
            && group.scope_record_index == scope.record_index
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return Ok(());
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return Ok(());
    }

    let mut edge_slots = Vec::new();
    ctx.reserve_vec(
        &mut edge_slots,
        group.members().len(),
        "collect F3D path edge slots",
    )?;
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| member.value)
        .enumerate()
    {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return Ok(());
        };
        let mut matching_operands = operands.iter().filter(|operand| {
            operand.group_record_index == group.record_index
                && operand.group_member_ordinal == ordinal
                && operand.record_index() == record_index
                && crate::ids::native_stream(&operand.id) == stream
        });
        let Some(operand) = matching_operands.next() else {
            return Ok(());
        };
        if matching_operands.next().is_some() {
            return Ok(());
        }
        let Some(edge_slot) = operand.resolved_edge_slot else {
            return Ok(());
        };
        edge_slots.push(edge_slot);
    }

    let mut edge_ids = Vec::new();
    ctx.reserve_vec(
        &mut edge_ids,
        edge_slots.len(),
        "collect F3D path edge identities",
    )?;
    for slot in edge_slots {
        edge_ids.push(crate::ids::history_input_edge_id_charged(
            ctx,
            feature_id,
            previous_state_id,
            slot,
        )?);
    }
    let state = crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
    let native = ctx.copy_retained_text(&group.id, "copy F3D path group identity")?;
    if let Ok(historical) = PathRef::historical_edges(state, edge_ids, native, ctx)? {
        *path = historical;
    }
    Ok(())
}

pub(crate) fn project_feature_input_topologies(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    edge_operands: &[crate::records::topology::edge_identity::DesignEdgeOperand],
) -> Result<Vec<cadmpeg_ir::features::FeatureInputTopology>, cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FeatureInputTopology;

    let mut projected = Vec::new();
    for feature in ctx.admit_iter(features, "scan F3D input topology features")? {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let previous_state_id = match scope.previous_history_state_id() {
            Some(previous_state_id) => Some(previous_state_id),
            None => {
                crate::design::feature_project::work_point_recipe_state_id(scope, edge_operands)
                    .or_else(|| crate::design::feature_project::work_plane_recipe_state_id(scope))
            }
        };
        let previous_state_id = match previous_state_id {
            Some(previous_state_id) => Some(previous_state_id),
            None => effective_scope_previous_history_state_id(ctx, scope, histories)?,
        };
        let Some(previous_state_id) = previous_state_id else {
            continue;
        };
        let state_from_pair = if let Some(state_id) = scope.history_state_id() {
            unique_history_state_pair(ctx, histories, state_id, previous_state_id)?
                .map(|(_, _, previous)| previous)
        } else {
            None
        };
        let state = match state_from_pair {
            Some(state) => Some(state),
            None => {
                unique_history_state(ctx, histories, previous_state_id)?.map(|(_, state)| state)
            }
        };
        let Some(state) = state else {
            continue;
        };
        let Some(topology) = state.topology() else {
            continue;
        };
        let Some(bodies) =
            project_input_members(ctx, &topology.bodies, "collect F3D input bodies", |slot| {
                crate::ids::history_input_body_id_charged(ctx, &feature.id, previous_state_id, slot)
            })?
        else {
            continue;
        };
        let Some(faces) =
            project_input_members(ctx, &topology.faces, "collect F3D input faces", |slot| {
                crate::ids::history_input_face_id_charged(ctx, &feature.id, previous_state_id, slot)
            })?
        else {
            continue;
        };
        let Some(edges) =
            project_input_members(ctx, &topology.edges, "collect F3D input edges", |slot| {
                crate::ids::history_input_edge_id_charged(ctx, &feature.id, previous_state_id, slot)
            })?
        else {
            continue;
        };
        let Some(vertices) = project_input_members(
            ctx,
            &topology.vertices,
            "collect F3D input vertices",
            |slot| {
                crate::ids::history_input_vertex_id_charged(
                    ctx,
                    &feature.id,
                    previous_state_id,
                    slot,
                )
            },
        )?
        else {
            continue;
        };
        let id = crate::ids::history_input_state_id_charged(ctx, &feature.id, previous_state_id)?;
        let input_of = feature
            .id
            .try_clone_for_decode(ctx, "copy F3D input feature identity")?;
        let native_ref = ctx.copy_retained_text(&state.id, "copy F3D input state reference")?;

        ctx.reserve_vec(&mut projected, 1, "collect F3D input topologies")?;
        projected.push(FeatureInputTopology {
            id,
            input_of,
            bodies,
            faces,
            edges,
            vertices,
            native_ref: Some(native_ref),
        });
    }
    Ok(projected)
}

fn project_input_members<T: Eq + std::hash::Hash>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    slots: &[i64],
    operation: &'static str,
    mut id: impl FnMut(i64) -> Result<T, cadmpeg_core::CodecError>,
) -> Result<Option<cadmpeg_ir::features::DistinctMembers<T>>, cadmpeg_core::CodecError> {
    let mut members = Vec::new();
    ctx.reserve_vec(&mut members, slots.len(), operation)?;
    for &slot in ctx.admit_iter(slots, "scan F3D input topology members")? {
        members.push(id(slot)?);
    }
    Ok(Some(
        match cadmpeg_ir::features::DistinctMembers::try_from(members, ctx) {
            Ok(value) => value,
            Err(cadmpeg_ir::features::FeatureCollectionError::Invalid(_)) => return Ok(None),
            Err(cadmpeg_ir::features::FeatureCollectionError::Resource(limit)) => {
                return Err(limit.into())
            }
        },
    ))
}

/// Resolve persistent vertex recipes in the last history-bearing feature state
/// that precedes their owning construction in authored timeline order.
pub(crate) fn bind_vertex_recipe_history(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    timelines: &[crate::records::entity_header::DesignFeatureTimeline],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    let source_ordinals =
        crate::design::feature_project::authored_scope_ordinals_per_stream(ctx, scopes, timelines)?;
    let mut input_states_storage = ctx.reserve_scoped(0, "index F3D vertex recipe input states")?;
    let mut input_states = HashMap::new();
    for scope in ctx.admit_iter(&*scopes, "scan F3D vertex recipe scopes")? {
        if !matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::WorkPlane
                | crate::records::feature::scope::DesignFeatureKind::WorkPoint
        ) {
            continue;
        }
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        let Some(&ordinal) = ctx.get_hash_map(
            &source_ordinals,
            &(stream, scope.record_index),
            "find F3D vertex recipe scope ordinal",
        )?
        else {
            continue;
        };
        let mut predecessor = None;
        for candidate in ctx.admit_iter(&*scopes, "scan F3D vertex recipe predecessors")? {
            let candidate_stream =
                crate::ids::native_stream(&candidate.id).unwrap_or(crate::ids::DEFAULT_STREAM);
            let Some(&candidate_ordinal) = ctx.get_hash_map(
                &source_ordinals,
                &(candidate_stream, candidate.record_index),
                "find F3D predecessor scope ordinal",
            )?
            else {
                continue;
            };
            if !ctx.equal(
                candidate_stream,
                stream,
                "compare F3D vertex recipe scope streams",
            )? || candidate_ordinal >= ordinal
            {
                continue;
            }
            let Some(candidate_state_id) = candidate.history_state_id() else {
                continue;
            };
            let candidate = (candidate_ordinal, candidate_state_id);
            if predecessor.is_none_or(|latest: (u64, i64)| candidate.0 > latest.0) {
                predecessor = Some(candidate);
            }
        }
        let Some(predecessor) = predecessor else {
            continue;
        };
        input_states_storage.with_storage(|| {
            let id = ctx.copy_retained_text(&scope.id, "copy F3D vertex recipe scope identity")?;
            ctx.insert_hash_map(
                &mut input_states,
                id,
                predecessor.1,
                "index F3D vertex recipe input states",
            )
            .map(|_| ())
        })?;
    }

    for scope in ctx.admit_iter(&mut *scopes, "scan F3D work point recipe scopes")? {
        if !ctx.equal(
            &scope.kind(),
            &crate::records::feature::scope::DesignFeatureKind::WorkPoint,
            "compare F3D work point recipe scope kind",
        )? {
            continue;
        }
        let state_id = ctx
            .get_hash_map(
                &input_states,
                &scope.id,
                "find F3D vertex recipe input state",
            )?
            .copied();
        let Some(construction) = scope.work_point_construction_mut() else {
            continue;
        };
        let solved_position = cadmpeg_ir::math::Point3::new(
            construction.position[0].get() * 10.0,
            construction.position[1].get() * 10.0,
            construction.position[2].get() * 10.0,
        );
        for recipe in construction.rule.vertex_recipes_mut() {
            recipe.resolution = None;
            let Some(state_id) = state_id else {
                continue;
            };
            let Some((_, state)) = unique_history_state(ctx, histories, state_id)? else {
                continue;
            };
            let Some(topology) = state.topology() else {
                continue;
            };
            let Some((vertex, position)) = vertex_recipe_candidate(ctx, recipe, topology)? else {
                continue;
            };
            if !point_matches(position, solved_position) {
                continue;
            }
            recipe.resolution = crate::records::feature::work_geometry::DesignVertexResolution::new(
                state_id, vertex,
            );
        }
    }

    for scope in ctx.admit_iter(&mut *scopes, "scan F3D work plane recipe scopes")? {
        if !ctx.equal(
            &scope.kind(),
            &crate::records::feature::scope::DesignFeatureKind::WorkPlane,
            "compare F3D work plane recipe scope kind",
        )? {
            continue;
        }
        let transform = scope.work_plane_transform();
        let state_id = ctx
            .get_hash_map(
                &input_states,
                &scope.id,
                "find F3D work plane recipe input state",
            )?
            .copied();
        let Some(construction) = scope.work_plane_construction_mut() else {
            continue;
        };
        construction.clear_resolution();
        let Some(state_id) = state_id else {
            continue;
        };
        let Some((_, state)) = unique_history_state(ctx, histories, state_id)? else {
            continue;
        };
        let Some(topology) = state.topology() else {
            continue;
        };
        let [first, second, third] = construction.inputs();
        let (Some(first), Some(second), Some(third)) = (
            vertex_recipe_candidate(ctx, first, topology)?,
            vertex_recipe_candidate(ctx, second, topology)?,
            vertex_recipe_candidate(ctx, third, topology)?,
        ) else {
            continue;
        };
        let candidates = [first, second, third];
        if !three_point_plane_matches(
            transform.map(crate::records::sketch_placement::SketchPlacementMatrix::rows),
            [first.1, second.1, third.1],
        ) {
            continue;
        }
        let [Some(first), Some(second), Some(third)] = candidates.map(|(vertex, _)| {
            crate::records::feature::work_geometry::DesignVertexResolution::new(state_id, vertex)
        }) else {
            continue;
        };
        construction
            .try_set_resolution([first, second, third])
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }
    Ok(())
}

/// Resolve edge-treatment corner recipes in their bound feature-input state.
pub(crate) fn bind_edge_treatment_vertex_history(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    for operand in operands {
        operand.recipe.resolution = None;
        let stream = crate::ids::native_stream(&operand.id);
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == operand.scope_record_index
                && crate::ids::native_stream(&scope.id) == stream
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let Some(previous_state_id) =
            effective_scope_previous_history_state_id(decode, scope, histories)?
        else {
            continue;
        };
        let Some((_, _, previous)) = bound_history_state_pair(
            decode,
            &scope.id,
            state_id,
            previous_state_id,
            scope_histories,
            histories,
        )?
        else {
            continue;
        };
        let Some(topology) = previous.topology() else {
            continue;
        };
        for reference in &mut operand.recipe.recipe_references {
            bind_historical_recipe_reference_candidates(decode, reference, topology)?;
        }
        let Some(vertex) = recipe_reference_common_vertex(decode, &operand.recipe, topology)?
        else {
            continue;
        };
        operand.recipe.resolution =
            crate::records::feature::work_geometry::DesignVertexResolution::new(
                previous_state_id,
                vertex,
            );
    }
    Ok(())
}

fn vertex_recipe_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    recipe: &crate::records::feature::work_geometry::DesignVertexRecipe,
    topology: &AsmHistoricalTopology,
) -> Result<Option<(i64, cadmpeg_ir::math::Point3)>, cadmpeg_core::CodecError> {
    let mut face_slots = Vec::new();
    for reference in ctx.admit_iter(
        &recipe.recipe_references,
        "scan F3D recipe recipe references",
    )? {
        let mut slots = Vec::new();
        for face in ctx.admit_iter(
            &reference.candidate_faces,
            "scan F3D vertex recipe candidate faces",
        )? {
            let Some(slot) = stable_ref(ctx, face.as_str())? else {
                continue;
            };
            if !ctx.contains(
                &topology.faces,
                &slot,
                "check F3D vertex recipe candidate face",
            )? {
                continue;
            }
            ctx.push_vec(
                &mut slots,
                slot,
                "collect F3D vertex recipe candidate faces",
            )?;
        }
        ctx.sort_unstable_by(
            &mut slots,
            |value| value,
            Ord::cmp,
            "sort F3D vertex recipe candidate faces",
        )?;
        ctx.dedup_vec(&mut slots, "deduplicate F3D vertex recipe candidate faces")?;
        let [slot] = slots.as_slice() else {
            return Ok(None);
        };

        ctx.reserve_vec(&mut face_slots, 1, "collect F3D vertex recipe face slots")?;
        face_slots.push(*slot);
    }
    if face_slots.is_empty() {
        return Ok(None);
    }
    let Some(vertex) = common_face_vertex(ctx, &face_slots, topology)? else {
        return Ok(None);
    };
    Ok(unique_historical_vertex_position(vertex, topology).map(|position| (vertex, position)))
}

fn three_point_plane_matches(
    transform: Option<[[f64; 4]; 4]>,
    points: [cadmpeg_ir::math::Point3; 3],
) -> bool {
    use cadmpeg_ir::math::{Point3, Vector3};

    let Some(transform) = transform else {
        return false;
    };
    let origin = Point3::new(
        transform[0][3] * 10.0,
        transform[1][3] * 10.0,
        transform[2][3] * 10.0,
    );
    let Some(normal) = Vector3::new(transform[0][2], transform[1][2], transform[2][2]).unit()
    else {
        return false;
    };
    let Some(point_normal) = points[1]
        .vector_from(points[0])
        .cross(points[2].vector_from(points[0]))
        .unit()
    else {
        return false;
    };
    let scale = points
        .iter()
        .flat_map(|point| [point.x.abs(), point.y.abs(), point.z.abs()])
        .fold(1.0_f64, f64::max);
    (normal.dot(point_normal).abs() - 1.0).abs() <= WORK_POINT_POSITION_TOLERANCE
        && points.iter().all(|point| {
            point.vector_from(origin).dot(normal).abs() <= WORK_POINT_POSITION_TOLERANCE * scale
        })
}

fn boundary_vertices_for_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: impl IntoIterator<Item = Result<Option<i64>, cadmpeg_core::CodecError>>,
    topology: &AsmHistoricalTopology,
    boundary_edges: &FaceBoundaryEdgeIndex<'_>,
) -> Result<Option<BTreeSet<i64>>, cadmpeg_core::CodecError> {
    let mut vertices = BTreeSet::new();
    for face in faces {
        let Some(face) = face? else {
            continue;
        };
        let Some(edges) = decode.get_btree_map(
            &boundary_edges.boundaries,
            &face,
            "find F3D boundary vertex face",
        )?
        else {
            return Ok(None);
        };
        for edge_slot in decode.admit_iter(&edges.edges, "scan F3D boundary vertex edges")? {
            let mut matches = topology
                .edge_vertices
                .iter()
                .filter(|candidate| candidate.edge == *edge_slot);
            let Some(edge) = matches.next() else {
                return Ok(None);
            };
            if matches.next().is_some()
                || !topology.vertices.contains(&edge.start_vertex)
                || !topology.vertices.contains(&edge.end_vertex)
            {
                return Ok(None);
            }
            decode.insert_btree_set(
                &mut vertices,
                edge.start_vertex,
                "collect F3D boundary vertices",
            )?;
            decode.insert_btree_set(
                &mut vertices,
                edge.end_vertex,
                "collect F3D boundary vertices",
            )?;
        }
    }
    Ok((!vertices.is_empty()).then_some(vertices))
}

fn common_face_vertex(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    face_slots: &[i64],
    topology: &AsmHistoricalTopology,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let boundary_edges = face_boundary_edge_index(decode, topology)?;
    let mut vertices_storage = decode.reserve_scoped(0, "collect F3D boundary vertices")?;
    let mut common = None::<BTreeSet<i64>>;
    for face in decode.admit_iter(face_slots, "scan F3D common face slots")? {
        let Some(vertices) = vertices_storage.with_storage(|| {
            boundary_vertices_for_faces(
                decode,
                std::iter::once(Ok::<_, cadmpeg_core::CodecError>(Some(*face))),
                topology,
                &boundary_edges,
            )
        })?
        else {
            return Ok(None);
        };
        if let Some(common) = &mut common {
            decode.retain_btree_set(
                common,
                |vertex| {
                    decode.contains_btree_set(&vertices, vertex, "find F3D common face vertex")
                },
                "intersect F3D common face vertices",
            )?;
        } else {
            common = Some(vertices);
        }
    }
    single_btree_value(decode, common, "take F3D common face vertex")
}

fn recipe_reference_common_vertex(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    recipe: &crate::records::feature::work_geometry::DesignVertexRecipe,
    topology: &AsmHistoricalTopology,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let boundary_edges = face_boundary_edge_index(decode, topology)?;
    let mut vertices_storage = decode.reserve_scoped(0, "collect F3D boundary vertices")?;
    let mut common = None::<BTreeSet<i64>>;
    for reference in decode.admit_iter(
        &recipe.recipe_references,
        "scan F3D recipe recipe references",
    )? {
        let faces = decode
            .admit_iter(
                &reference.candidate_faces,
                "scan F3D common vertex recipe faces",
            )?
            .map(|face| {
                let Some(slot) = stable_ref(decode, face.as_str())? else {
                    return Ok(None);
                };
                if decode.contains(
                    &topology.faces,
                    &slot,
                    "check F3D common vertex recipe face",
                )? {
                    Ok(Some(slot))
                } else {
                    Ok(None)
                }
            });
        let Some(vertices) = vertices_storage.with_storage(|| {
            boundary_vertices_for_faces(decode, faces, topology, &boundary_edges)
        })?
        else {
            return Ok(None);
        };
        if let Some(common) = &mut common {
            decode.retain_btree_set(
                common,
                |vertex| {
                    decode.contains_btree_set(&vertices, vertex, "find F3D common face vertex")
                },
                "intersect F3D common face vertices",
            )?;
        } else {
            common = Some(vertices);
        }
    }
    single_btree_value(decode, common, "take F3D common face vertex")
}

/// Returns the only value of a present set; an absent, empty or multi-valued
/// set yields `None`. Only a singleton is visited.
fn single_btree_value<T: Copy>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    values: Option<BTreeSet<T>>,
    operation: &'static str,
) -> Result<Option<T>, cadmpeg_core::CodecError> {
    let Some(values) = values.filter(|values| values.len() == 1) else {
        return Ok(None);
    };
    Ok(decode.admit_iter(values, operation)?.next())
}

fn unique_historical_vertex_position(
    vertex: i64,
    topology: &AsmHistoricalTopology,
) -> Option<cadmpeg_ir::math::Point3> {
    let mut bindings = topology
        .vertex_points
        .iter()
        .filter(|binding| binding.entity == vertex);
    let point = bindings.next()?.carrier;
    if bindings.next().is_some() {
        return None;
    }
    let mut positions = topology
        .point_positions
        .iter()
        .filter(|position| position.point == point);
    let position = positions.next()?.position;
    (positions.next().is_none() && position.is_finite()).then_some(position)
}

fn point_matches(left: cadmpeg_ir::math::Point3, right: cadmpeg_ir::math::Point3) -> bool {
    [(left.x, right.x), (left.y, right.y), (left.z, right.z)]
        .into_iter()
        .all(|(left, right)| {
            let scale = left.abs().max(right.abs()).max(1.0);
            (left - right).abs() <= WORK_POINT_POSITION_TOLERANCE * scale
        })
}

#[cfg(test)]
fn feature_input_prefix(
    feature: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
) -> cadmpeg_ir::ids::IdentityKey {
    let feature_key = feature.key();
    crate::ids::history_input_prefix(&feature_key, previous_state_id)
}

fn unique_history_state<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &'a [AsmHistory],
    state_id: i64,
) -> Result<Option<(&'a AsmHistory, &'a AsmDeltaState)>, cadmpeg_core::CodecError> {
    let operation = "find F3D history state";
    let mut found_state_index = None;
    let Some(index) = decode.position_by(
        histories,
        |history| {
            found_state_index = unique_history_state_in(decode, history, state_id)?;
            Ok(found_state_index.is_some())
        },
        operation,
    )?
    else {
        return Ok(None);
    };
    if decode
        .position_by(
            &histories[index + 1..],
            |history| Ok(unique_history_state_in(decode, history, state_id)?.is_some()),
            operation,
        )?
        .is_some()
    {
        return Ok(None);
    }
    let (Some(history), Some(state_index)) = (histories.get(index), found_state_index) else {
        return Ok(None);
    };
    Ok(history
        .states
        .get(state_index)
        .map(|state| (history, state)))
}

fn unique_history_state_in(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &AsmHistory,
    state_id: i64,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let operation = "find F3D ASM history state";
    let Some(index) = decode.position_by(
        &history.states,
        |state| Ok(state.state_id == state_id),
        operation,
    )?
    else {
        return Ok(None);
    };
    if decode
        .position_by(
            &history.states[index + 1..],
            |state| Ok(state.state_id == state_id),
            operation,
        )?
        .is_some()
    {
        return Ok(None);
    }
    Ok(Some(index))
}

/// Return the effective input state for a scope. Some Design scope envelopes
/// omit the preceding state identity even though the current ASM delta state
/// carries the direct transition predecessor.
pub(crate) fn effective_scope_previous_history_state_id(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
    histories: &[AsmHistory],
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    if let Some(previous_state_id) = scope.previous_history_state_id() {
        return Ok(Some(previous_state_id));
    }
    let Some(state_id) = scope.history_state_id() else {
        return Ok(None);
    };
    let Some((history, state)) = unique_history_state(decode, histories, state_id)? else {
        return Ok(None);
    };
    linked_previous_state_id(decode, history, state)
}

pub(crate) fn unique_history_state_pair<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &'a [AsmHistory],
    state_id: i64,
    previous_state_id: i64,
) -> Result<Option<(&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState)>, cadmpeg_core::CodecError>
{
    if let Some(direct) =
        unique_history_state_pair_for_link(decode, histories, state_id, previous_state_id, true)?
    {
        return Ok(direct);
    }
    Ok(
        unique_history_state_pair_for_link(decode, histories, state_id, previous_state_id, false)?
            .flatten(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HemGapLengthForm {
    Flat,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HemGeometrySemantics {
    pub(crate) direction: Option<cadmpeg_ir::features::SheetMetalHemDirection>,
    pub(crate) gap_length_form: Option<HemGapLengthForm>,
}

/// Resolve Hem semantics from the bend carriers in the owning history
/// transition. The source operation's fixed fields do not carry these
/// meanings; the selected edge and the inserted coaxial cylinders do.
pub(crate) fn hem_geometry_semantics(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
    edge_slot: i64,
    histories: &[AsmHistory],
) -> Result<HemGeometrySemantics, cadmpeg_core::CodecError> {
    let unresolved = HemGeometrySemantics {
        direction: None,
        gap_length_form: None,
    };
    let Some(state_id) = scope.history_state_id() else {
        return Ok(unresolved);
    };
    let Some(previous_state_id) =
        effective_scope_previous_history_state_id(decode, scope, histories)?
    else {
        return Ok(unresolved);
    };
    let Some((_, state, previous)) =
        unique_history_state_pair(decode, histories, state_id, previous_state_id)?
    else {
        return Ok(unresolved);
    };
    let (Some(previous_topology), Some(transition)) =
        (previous.topology(), state.transition.as_ref())
    else {
        return Ok(unresolved);
    };
    let Some(current_topology) = state.topology() else {
        return Ok(unresolved);
    };
    let inserted_surfaces = &transition.topology.surfaces.inserted;
    if !decode.any_by(
        &current_topology.surface_cylinders,
        |cylinder| {
            decode.contains(
                inserted_surfaces,
                &cylinder.surface,
                "find F3D inserted Hem cylinder surface",
            )
        },
        "scan F3D inserted Hem cylinders",
    )? {
        return Ok(unresolved);
    }
    let edge_direction =
        historical_edge_axis(decode, edge_slot, previous_topology)?.map(|(_, direction)| direction);
    let Some(_) = decode.find_by(
        &current_topology.surface_cylinders,
        |cylinder| {
            hem_cylinder_matches(
                decode,
                cylinder,
                inserted_surfaces,
                edge_direction,
                "match F3D inserted Hem cylinder surface",
            )
        },
        "find F3D Hem direction cylinder",
    )?
    else {
        return Ok(unresolved);
    };
    Ok(HemGeometrySemantics {
        direction: hem_direction_from_transition(
            decode,
            edge_slot,
            &current_topology.surface_cylinders,
            inserted_surfaces,
            edge_direction,
            previous_topology,
            transition,
        )?,
        gap_length_form: hem_gap_length_form(
            decode,
            &current_topology.surface_cylinders,
            inserted_surfaces,
            edge_direction,
        )?,
    })
}

fn hem_cylinder_matches(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    cylinder: &AsmHistoricalCylinder,
    inserted_surfaces: &[i64],
    edge_direction: Option<cadmpeg_ir::math::Vector3>,
    membership_operation: &'static str,
) -> Result<bool, cadmpeg_core::CodecError> {
    if !decode.contains(inserted_surfaces, &cylinder.surface, membership_operation)? {
        return Ok(false);
    }
    Ok(edge_direction.is_none_or(|direction| parallel_directions(direction, cylinder.axis)))
}

fn hem_gap_length_form(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    cylinders: &[AsmHistoricalCylinder],
    inserted_surfaces: &[i64],
    edge_direction: Option<cadmpeg_ir::math::Vector3>,
) -> Result<Option<HemGapLengthForm>, cadmpeg_core::CodecError> {
    let mut first = None;
    let mut second = None;
    for cylinder in decode.admit_iter(cylinders, "scan F3D Hem gap-length carriers")? {
        if !hem_cylinder_matches(
            decode,
            cylinder,
            inserted_surfaces,
            edge_direction,
            "match F3D inserted Hem cylinder surface",
        )? {
            continue;
        }
        if first.is_none() {
            first = Some(cylinder);
        } else if second.is_none() {
            second = Some(cylinder);
        } else {
            return Ok(None);
        }
    }
    let (Some(first), Some(second)) = (first, second) else {
        return Ok(None);
    };
    if !same_axis_line((first.origin, first.axis), (second.origin, second.axis)) {
        return Ok(None);
    }
    let [inner, outer] = if first.radius <= second.radius {
        [first.radius, second.radius]
    } else {
        [second.radius, first.radius]
    };
    if !inner.is_finite() || !outer.is_finite() || inner < 0.0 || outer <= inner {
        return Ok(None);
    }
    let thickness = outer - inner;
    let tolerance = EPS_HISTORY_HEM_GAP_LENGTH_FORM_E7 * (1.0 + outer.abs() + inner.abs());
    if (2.0 * inner - thickness).abs() <= tolerance {
        Ok(Some(HemGapLengthForm::Open))
    } else if 2.0 * inner < thickness - tolerance {
        Ok(Some(HemGapLengthForm::Flat))
    } else {
        Ok(None)
    }
}

fn hem_direction_from_transition(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    edge_slot: i64,
    cylinders: &[AsmHistoricalCylinder],
    inserted_surfaces: &[i64],
    edge_direction: Option<cadmpeg_ir::math::Vector3>,
    previous: &AsmHistoricalTopology,
    transition: &AsmHistoricalTransition,
) -> Result<Option<cadmpeg_ir::features::SheetMetalHemDirection>, cadmpeg_core::CodecError> {
    if decode
        .find_by(
            cylinders,
            |cylinder| {
                hem_cylinder_matches(
                    decode,
                    cylinder,
                    inserted_surfaces,
                    edge_direction,
                    "match F3D inserted Hem cylinder surface",
                )
            },
            "find F3D Hem direction cylinder",
        )?
        .is_none()
    {
        return Ok(None);
    }
    let edge_context = selection::historical_edge_context(decode, edge_slot, previous)?;
    let mut candidate = None;
    for (ordinal, incident) in decode
        .admit_iter(&edge_context.incident_loops, "scan F3D Hem incident loops")?
        .enumerate()
    {
        let face = incident.face_slot;
        let duplicate = decode
            .admit_iter(
                &edge_context.incident_loops[..ordinal],
                "find duplicate F3D Hem incident face",
            )?
            .any(|earlier| earlier.face_slot == face);
        if duplicate {
            continue;
        }
        if decode.contains(
            &transition.topology.faces.deleted,
            &face,
            "find deleted F3D Hem face",
        )? {
            continue;
        }
        let mut bindings = previous
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == face);
        let Some(binding) = bindings.next() else {
            continue;
        };
        if bindings.next().is_some() {
            continue;
        }
        let mut planes = previous
            .surface_planes
            .iter()
            .filter(|plane| plane.surface == binding.carrier);
        let Some(plane) = planes.next() else {
            continue;
        };
        if planes.next().is_some() {
            continue;
        }
        let length = plane.normal.norm();
        if !(length.is_finite() && length > 0.0) {
            continue;
        }
        let normal = plane.normal.scale(1.0 / length);
        let Some(first_cylinder) = decode.find_by(
            cylinders,
            |cylinder| {
                hem_cylinder_matches(
                    decode,
                    cylinder,
                    inserted_surfaces,
                    edge_direction,
                    "match F3D inserted Hem cylinder surface",
                )
            },
            "find F3D Hem direction cylinder",
        )?
        else {
            continue;
        };
        let first = normal.dot(first_cylinder.origin.vector_from(plane.origin));
        if !first.is_finite() {
            continue;
        }
        let scale = decode.fold(
            cylinders,
            0.0_f64,
            |scale, cylinder| {
                if !hem_cylinder_matches(
                    decode,
                    cylinder,
                    inserted_surfaces,
                    edge_direction,
                    "match F3D inserted Hem cylinder surface",
                )? {
                    return Ok(scale);
                }
                Ok(scale
                    .max(cylinder.origin.x.abs())
                    .max(cylinder.origin.y.abs())
                    .max(cylinder.origin.z.abs()))
            },
            "scan F3D Hem cylinder direction scale",
        )?;
        let sign_tolerance = EPS_HISTORY_HEM_DIRECTION_FROM_TRANSITION_E7
            * (1.0 + plane.origin.x.abs() + plane.origin.y.abs() + plane.origin.z.abs() + scale);
        if first.abs() <= sign_tolerance
            || decode.any_by(
                cylinders,
                |cylinder| {
                    if !hem_cylinder_matches(
                        decode,
                        cylinder,
                        inserted_surfaces,
                        edge_direction,
                        "match F3D inserted Hem cylinder surface",
                    )? {
                        return Ok(false);
                    }
                    let offset = normal.dot(cylinder.origin.vector_from(plane.origin));
                    Ok(!offset.is_finite()
                        || offset.abs() <= sign_tolerance
                        || offset.is_sign_positive() != first.is_sign_positive())
                },
                "check F3D Hem cylinder direction offsets",
            )?
        {
            continue;
        }
        if candidate.replace(first.is_sign_positive()).is_some() {
            return Ok(None);
        }
    }
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    Ok(Some(if candidate {
        cadmpeg_ir::features::SheetMetalHemDirection::Forward
    } else {
        cadmpeg_ir::features::SheetMetalHemDirection::Reverse
    }))
}

fn parallel_directions(left: cadmpeg_ir::math::Vector3, right: cadmpeg_ir::math::Vector3) -> bool {
    let left_length = left.norm();
    let right_length = right.norm();
    if !(left_length.is_finite()
        && left_length > 0.0
        && right_length.is_finite()
        && right_length > 0.0)
    {
        return false;
    }
    let dot = left
        .scale(1.0 / left_length)
        .dot(right.scale(1.0 / right_length));
    dot.is_finite() && (dot.abs() - 1.0).abs() <= EPS_HISTORY_PARALLEL_DIRECTIONS_E7
}

fn history_state_pair(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &AsmHistory,
    state_id: i64,
    previous_state_id: i64,
    require_direct: bool,
) -> Result<Option<(usize, usize)>, cadmpeg_core::CodecError> {
    let Some(state_index) = unique_history_state_in(decode, history, state_id)? else {
        return Ok(None);
    };
    let Some(state) = history.states.get(state_index) else {
        return Ok(None);
    };
    if require_direct {
        let Some(linked_previous) = linked_previous_state_id(decode, history, state)? else {
            return Ok(None);
        };
        if linked_previous != previous_state_id {
            return Ok(None);
        }
    }
    let Some(previous_index) = unique_history_state_in(decode, history, previous_state_id)? else {
        return Ok(None);
    };
    if !require_direct && !history_state_reaches(decode, history, state, previous_state_id)? {
        return Ok(None);
    }
    Ok(Some((state_index, previous_index)))
}

fn unique_history_state_pair_for_link<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &'a [AsmHistory],
    state_id: i64,
    previous_state_id: i64,
    require_direct: bool,
) -> Result<
    Option<Option<(&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState)>>,
    cadmpeg_core::CodecError,
> {
    let operation = "find F3D history state pair";
    let mut pair_indices = None;
    let Some(index) = decode.position_by(
        histories,
        |history| {
            pair_indices =
                history_state_pair(decode, history, state_id, previous_state_id, require_direct)?;
            Ok(pair_indices.is_some())
        },
        operation,
    )?
    else {
        return Ok(None);
    };
    if decode
        .position_by(
            &histories[index + 1..],
            |history| {
                Ok(history_state_pair(
                    decode,
                    history,
                    state_id,
                    previous_state_id,
                    require_direct,
                )?
                .is_some())
            },
            operation,
        )?
        .is_some()
    {
        return Ok(Some(None));
    }
    let (Some(history), Some((state_index, previous_index))) = (histories.get(index), pair_indices)
    else {
        return Ok(Some(None));
    };
    let (Some(state), Some(previous)) = (
        history.states.get(state_index),
        history.states.get(previous_index),
    ) else {
        return Ok(Some(None));
    };
    Ok(Some(Some((history, state, previous))))
}

/// Return the one ASM history bound to a Design scope.
pub(crate) fn bound_scope_history<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scope_id: &str,
    scope_histories: &HashMap<String, String>,
    histories: &'a [AsmHistory],
) -> Result<Option<&'a AsmHistory>, cadmpeg_core::CodecError> {
    let Some(history_id) =
        decode.get_hash_map(scope_histories, scope_id, "find F3D scope history binding")?
    else {
        return Ok(None);
    };
    let operation = "find F3D bound history";
    let Some(index) = decode.position_by(
        histories,
        |history| decode.equal(history.id.as_str(), history_id.as_str(), operation),
        operation,
    )?
    else {
        return Ok(None);
    };
    if decode
        .position_by(
            &histories[index + 1..],
            |history| decode.equal(history.id.as_str(), history_id.as_str(), operation),
            operation,
        )?
        .is_some()
    {
        return Ok(None);
    }
    Ok(Some(&histories[index]))
}

fn bound_history_state_pair<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scope_id: &str,
    state_id: i64,
    previous_state_id: i64,
    scope_histories: &HashMap<String, String>,
    histories: &'a [AsmHistory],
) -> Result<Option<(&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState)>, cadmpeg_core::CodecError>
{
    let Some(history) = bound_scope_history(decode, scope_id, scope_histories, histories)? else {
        return Ok(None);
    };
    let Some((state_index, previous_index)) =
        history_state_pair(decode, history, state_id, previous_state_id, false)?
    else {
        return Ok(None);
    };
    let (Some(state), Some(previous)) = (
        history.states.get(state_index),
        history.states.get(previous_index),
    ) else {
        return Ok(None);
    };
    Ok(Some((history, state, previous)))
}

fn insert_scope_history_binding(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    resolved: &mut HashMap<String, String>,
    scope_id: &str,
    history_id: &str,
) -> Result<(), cadmpeg_core::CodecError> {
    let scope = {
        let ctx = decode;
        ctx.copy_retained_text(scope_id, "copy F3D bound scope identity")?
    };
    let history = {
        let ctx = decode;
        ctx.copy_retained_text(history_id, "copy F3D bound history identity")?
    };
    decode
        .insert_hash_map(resolved, scope, history, "index F3D scope history bindings")
        .map(|_| ())?;
    Ok(())
}

pub(crate) fn bind_scope_histories(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    body_bindings: &[crate::records::bodies::DesignBodyBinding],
    body_recipe_operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    histories: &[AsmHistory],
) -> Result<HashMap<String, String>, cadmpeg_core::CodecError> {
    let mut candidate_storage = decode.reserve_scoped(0, "collect F3D scope history candidates")?;
    let mut candidates = Vec::new();
    for scope in decode.admit_iter(scopes, "scan F3D scope history candidates")? {
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let mut scope_candidates = Vec::new();
        if let Some(previous_state_id) = scope.previous_history_state_id() {
            for history in decode.admit_iter(histories, "scan F3D direct scope histories")? {
                if history_state_pair(decode, history, state_id, previous_state_id, true)?.is_some()
                {
                    decode.push_scoped_vec(
                        &mut candidate_storage,
                        &mut scope_candidates,
                        history,
                        "collect F3D direct scope histories",
                    )?;
                }
            }
            if scope_candidates.is_empty() {
                for history in decode.admit_iter(histories, "scan F3D reachable scope histories")? {
                    if history_state_pair(decode, history, state_id, previous_state_id, false)?
                        .is_some()
                    {
                        decode.push_scoped_vec(
                            &mut candidate_storage,
                            &mut scope_candidates,
                            history,
                            "collect F3D reachable scope histories",
                        )?;
                    }
                }
            }
        } else {
            for history in decode.admit_iter(histories, "scan F3D matching scope histories")? {
                if unique_history_state_in(decode, history, state_id)?.is_some() {
                    decode.push_scoped_vec(
                        &mut candidate_storage,
                        &mut scope_candidates,
                        history,
                        "collect F3D matching scope histories",
                    )?;
                }
            }
        }
        if !scope_candidates.is_empty() {
            decode.push_scoped_vec(
                &mut candidate_storage,
                &mut candidates,
                (scope, scope_candidates),
                "collect F3D scopes with histories",
            )?;
        }
    }
    let mut resolved = HashMap::<String, String>::new();
    for (scope, candidates) in decode.admit_iter(&candidates, "scan F3D candidates")? {
        if candidates.len() == 1 {
            insert_scope_history_binding(decode, &mut resolved, &scope.id, &candidates[0].id)?;
            continue;
        }
        let next_scope_record_index = decode.fold(
            scopes,
            None::<u32>,
            |next, candidate| {
                if crate::ids::same_native_occurrence(decode, &candidate.id, &scope.id)?
                    && candidate.record_index > scope.record_index
                {
                    Ok(Some(next.map_or(candidate.record_index, |record_index| {
                        record_index.min(candidate.record_index)
                    })))
                } else {
                    Ok(next)
                }
            },
            "find F3D next scope record index",
        )?;
        let mut is_output_binding = |binding: &crate::records::bodies::DesignBodyBinding| {
            Ok(
                crate::ids::same_native_occurrence(decode, binding.id(), &scope.id)?
                    && binding.entity_suffix > u64::from(scope.record_index)
                    && next_scope_record_index
                        .is_none_or(|next| binding.entity_suffix < u64::from(next)),
            )
        };
        if let Some(binding_index) = decode.position_by(
            body_bindings,
            &mut is_output_binding,
            "find F3D output body binding",
        )? {
            if decode
                .position_by(
                    &body_bindings[binding_index + 1..],
                    &mut is_output_binding,
                    "find F3D output body binding",
                )?
                .is_none()
            {
                let binding = &body_bindings[binding_index];
                let mut matches_binding_source = |history: &&AsmHistory| {
                    let Some(source) = historical_brep_source(decode, &history.id)? else {
                        return Ok(false);
                    };
                    let Some(blob_source) = decode.strip_prefix(
                        binding.blob_name(),
                        "BREP.",
                        "strip F3D output BREP prefix",
                    )?
                    else {
                        return Ok(false);
                    };
                    decode.equal(blob_source, source, "compare F3D output BREP source")
                };
                if let Some(history_index) = decode.position_by(
                    candidates.as_slice(),
                    &mut matches_binding_source,
                    "find F3D output BREP history",
                )? {
                    if decode
                        .position_by(
                            &candidates.as_slice()[history_index + 1..],
                            &mut matches_binding_source,
                            "find F3D output BREP history",
                        )?
                        .is_none()
                    {
                        let history = candidates[history_index];
                        insert_scope_history_binding(
                            decode,
                            &mut resolved,
                            &scope.id,
                            &history.id,
                        )?;
                        continue;
                    }
                }
            }
        }
        let candidate_has_face = |source: Option<&str>| -> Result<bool, cadmpeg_core::CodecError> {
            for operand in decode.admit_iter(
                body_recipe_operands,
                "scan F3D body recipe candidate operands",
            )? {
                if !crate::ids::same_native_occurrence(decode, &operand.id, &scope.id)?
                    || operand.scope_record_index != scope.record_index
                {
                    continue;
                }
                for reference in decode.admit_iter(
                    operand.references(),
                    "scan F3D body recipe candidate references",
                )? {
                    for face in decode.admit_iter(
                        &reference.candidate_faces,
                        "scan F3D body recipe candidate faces",
                    )? {
                        let matches_source = match source {
                            Some(source) => active_brep_face_matches_source(decode, face, source)?,
                            None => true,
                        };
                        if matches_source {
                            return Ok(true);
                        }
                    }
                }
            }
            Ok(false)
        };
        if candidate_has_face(None)? {
            let mut matching_history = None;
            let mut ambiguous_history = false;
            for history in
                decode.admit_iter(candidates.as_slice(), "scan F3D candidate face histories")?
            {
                let Some(source) = historical_brep_source(decode, &history.id)? else {
                    continue;
                };
                if candidate_has_face(Some(source))? {
                    if matching_history.is_some() {
                        ambiguous_history = true;
                        break;
                    }
                    matching_history = Some(*history);
                }
            }
            if !ambiguous_history {
                if let Some(history) = matching_history {
                    insert_scope_history_binding(decode, &mut resolved, &scope.id, &history.id)?;
                    continue;
                }
            }
        }
        let Some(construction) = scope.base_feature_construction() else {
            continue;
        };
        let mut referenced_history_id = None;
        let mut ambiguous_referenced_history = false;
        {
            let mut resolve_body_reference =
                |suffix: u32| -> Result<bool, cadmpeg_core::CodecError> {
                    if ambiguous_referenced_history {
                        return Ok(true);
                    }
                    let mut binding_matches_reference =
                        |binding: &crate::records::bodies::DesignBodyBinding| {
                            Ok(
                                crate::ids::same_native_occurrence(
                                    decode,
                                    binding.id(),
                                    &scope.id,
                                )? && binding.entity_suffix == u64::from(suffix),
                            )
                        };
                    let Some(binding_index) = decode.position_by(
                        body_bindings,
                        &mut binding_matches_reference,
                        "find F3D referenced body binding",
                    )?
                    else {
                        return Ok(false);
                    };
                    if decode
                        .position_by(
                            &body_bindings[binding_index + 1..],
                            &mut binding_matches_reference,
                            "find F3D referenced body binding",
                        )?
                        .is_some()
                    {
                        return Ok(false);
                    }
                    let binding = &body_bindings[binding_index];
                    let mut history_matches_binding = |history: &&AsmHistory| {
                        let Some(source) = historical_brep_source(decode, &history.id)? else {
                            return Ok(false);
                        };
                        let Some(blob_source) = decode.strip_prefix(
                            binding.blob_name(),
                            "BREP.",
                            "strip F3D referenced BREP prefix",
                        )?
                        else {
                            return Ok(false);
                        };
                        decode.equal(blob_source, source, "compare F3D referenced BREP source")
                    };
                    let Some(history_index) = decode.position_by(
                        candidates.as_slice(),
                        &mut history_matches_binding,
                        "find F3D referenced BREP history",
                    )?
                    else {
                        return Ok(false);
                    };
                    if decode
                        .position_by(
                            &candidates.as_slice()[history_index + 1..],
                            &mut history_matches_binding,
                            "find F3D referenced BREP history",
                        )?
                        .is_some()
                    {
                        return Ok(false);
                    }
                    let history_id = candidates[history_index].id.as_str();
                    if let Some(previous_history_id) = referenced_history_id {
                        if !decode.equal(
                            previous_history_id,
                            history_id,
                            "compare F3D referenced history IDs",
                        )? {
                            ambiguous_referenced_history = true;
                            return Ok(true);
                        }
                    } else {
                        referenced_history_id = Some(history_id);
                    }
                    Ok(false)
                };
            match construction.body_reference_records() {
                DesignBaseFeatureBodyReferenceSource::ResultRows(rows) => {
                    for row in decode.admit_iter(rows, "scan F3D body reference rows")? {
                        if resolve_body_reference(row.reference.value)? {
                            break;
                        }
                    }
                }
                DesignBaseFeatureBodyReferenceSource::RepeatedResultRows { first, rest } => {
                    let first_rows = decode.admit_iter(
                        std::slice::from_ref(first),
                        "scan F3D first body reference row",
                    )?;
                    let repeated_rows =
                        decode.admit_iter(rest, "scan F3D repeated body reference rows")?;
                    for row in first_rows.chain(repeated_rows.map(|(row, _)| row)) {
                        if resolve_body_reference(row.reference.value)? {
                            break;
                        }
                    }
                }
                DesignBaseFeatureBodyReferenceSource::LegacyRows(rows) => {
                    for row in decode.admit_iter(rows, "scan F3D legacy body reference rows")? {
                        if resolve_body_reference(row.entity.value)? {
                            break;
                        }
                    }
                }
                DesignBaseFeatureBodyReferenceSource::SingleBody(suffix) => {
                    for suffix in decode.admit_iter(
                        std::slice::from_ref(suffix),
                        "scan F3D single body reference",
                    )? {
                        if resolve_body_reference(*suffix)? {
                            break;
                        }
                    }
                }
                DesignBaseFeatureBodyReferenceSource::Empty => {}
            }
        }
        if !ambiguous_referenced_history {
            if let Some(history_id) = referenced_history_id {
                insert_scope_history_binding(decode, &mut resolved, &scope.id, history_id)?;
            }
        }
    }
    let mut groups_storage = decode.reserve_scoped(0, "index F3D scope history groups")?;
    let mut groups = BTreeMap::<(&str, i64, Option<i64>), Vec<usize>>::new();
    for (index, (scope, _)) in decode
        .admit_iter(&candidates, "scan F3D scope history group candidates")?
        .enumerate()
    {
        let (Some(stream), Some(state_id)) = (
            crate::ids::native_stream(&scope.id),
            scope.history_state_id(),
        ) else {
            continue;
        };
        let key = (stream, state_id, scope.previous_history_state_id());
        decode.push_scoped_btree_group(
            &mut groups_storage,
            &mut groups,
            key,
            || index,
            0,
            "index F3D scope history groups",
        )?;
    }
    for members in decode
        .admit_iter(&groups, "scan F3D groups")?
        .map(|(_, value)| value)
    {
        let mut candidate_histories = HashSet::new();
        for index in decode.admit_iter(members, "scan F3D scope history group members")? {
            for history in decode.admit_iter(
                &candidates[*index].1,
                "scan F3D scope history group histories",
            )? {
                decode.insert_hash_set(
                    &mut candidate_histories,
                    history.id.as_str(),
                    "index F3D scope candidate histories",
                )?;
            }
        }
        if candidate_histories.len() != members.len() {
            continue;
        }
        loop {
            let mut assigned = HashSet::new();
            for index in decode.admit_iter(members, "scan F3D assigned history members")? {
                if let Some(history_id) = decode.get_hash_map(
                    &resolved,
                    &candidates[*index].0.id,
                    "find F3D assigned history identity",
                )? {
                    let copy = {
                        let ctx = decode;
                        ctx.copy_retained_text(history_id, "copy F3D assigned history identity")?
                    };
                    decode.insert_hash_set(
                        &mut assigned,
                        copy,
                        "index F3D assigned scope histories",
                    )?;
                }
            }
            let mut resolved_member_count: usize = 0;
            for index in decode.admit_iter(members, "count F3D resolved history members")? {
                if decode.contains_key_hash_map(
                    &resolved,
                    &candidates[*index].0.id,
                    "check F3D resolved history member",
                )? {
                    resolved_member_count =
                        resolved_member_count.checked_add(1).ok_or_else(|| {
                            decode.refuse_codec_limit(
                                "count F3D resolved history members",
                                u64::MAX - 1,
                                u64::MAX,
                            )
                        })?;
                }
            }
            if assigned.len() != resolved_member_count {
                break;
            }
            let mut progress = false;
            for index in decode.admit_iter(members, "scan F3D unresolved history members")? {
                let (scope, scope_candidates) = &candidates[*index];
                if decode.contains_key_hash_map(
                    &resolved,
                    &scope.id,
                    "check F3D scope history resolution",
                )? {
                    continue;
                }
                let mut remaining = scope_candidates
                    .iter()
                    .filter(|history| !assigned.contains(history.id.as_str()));
                if let Some(history) = remaining.next().filter(|_| remaining.next().is_none()) {
                    insert_scope_history_binding(decode, &mut resolved, &scope.id, &history.id)?;
                    progress = true;
                }
            }
            if !progress {
                break;
            }
        }
    }
    Ok(resolved)
}

fn history_state_reaches(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &AsmHistory,
    state: &AsmDeltaState,
    previous_state_id: i64,
) -> Result<bool, cadmpeg_core::CodecError> {
    let mut current = state;
    let state_chain_steps = 0..=history.states.len();
    for _ in decode.admit_iter(&state_chain_steps, "walk F3D history state chain")? {
        if current.state_id == previous_state_id {
            return Ok(true);
        }
        let Some(previous_state_id) = linked_previous_state_id(decode, history, current)? else {
            return Ok(false);
        };
        let Some(previous_index) = unique_history_state_in(decode, history, previous_state_id)?
        else {
            return Ok(false);
        };
        let Some(previous) = history.states.get(previous_index) else {
            return Ok(false);
        };
        current = previous;
    }
    Ok(false)
}

/// Return the state ID reached by a delta state's `next` link.
///
/// Reject disagreement between the derived transition predecessor and the raw
/// `next` chain.
fn linked_previous_state_id(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &AsmHistory,
    state: &AsmDeltaState,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let linked = if let Some(node_index) = state.next_ref {
        let operation = "find F3D ASM predecessor node";
        if let Some(index) = decode.position_by(
            &history.states,
            |candidate| Ok(candidate.node_index == node_index),
            operation,
        )? {
            if decode
                .position_by(
                    &history.states[index + 1..],
                    |candidate| Ok(candidate.node_index == node_index),
                    operation,
                )?
                .is_some()
            {
                None
            } else {
                Some(history.states[index].state_id)
            }
        } else {
            None
        }
    } else {
        None
    };
    let derived = state
        .transition
        .as_ref()
        .and_then(|transition| transition.previous_state_id);
    match (derived, linked) {
        (Some(derived), Some(linked)) => {
            if derived != linked {
                Ok(None)
            } else {
                Ok(Some(derived))
            }
        }
        (Some(derived), _) => Ok(Some(derived)),
        (None, Some(linked)) => Ok(Some(linked)),
        (None, None) => Ok(None),
    }
}

fn history_state_index<'h>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &'h AsmHistory,
) -> Result<HashMap<i64, Option<&'h AsmDeltaState>>, cadmpeg_core::CodecError> {
    let mut states = HashMap::new();
    for state in decode.admit_iter(&history.states, "scan F3D history states")? {
        if !states.contains_key(&state.state_id) {
            decode.reserve_map(&mut states, 1, "index F3D history states")?;
        }
        states
            .entry(state.state_id)
            .and_modify(|state| *state = None)
            .or_insert(Some(state));
    }
    Ok(states)
}

fn exact_face_selection_group<'a>(
    operand: &crate::records::topology::face::DesignFaceOperand,
    scope: &crate::records::feature::scope::DesignParameterScope,
    operand_groups: &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
) -> Option<&'a crate::records::topology::construction::DesignConstructionOperandGroup> {
    let stream = crate::ids::native_stream(&operand.id)?;
    if crate::ids::native_stream(&scope.id) != Some(stream) {
        return None;
    }
    let group_record_index = operand.group_record_index()?;
    let group_member_ordinal = usize::try_from(operand.group_member_ordinal()?).ok()?;
    let mut groups = operand_groups.iter().filter(|group| {
        crate::ids::native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && group.record_index == group_record_index
            && group.role() == DesignOperandRole::ROLE_0X10
            && group
                .members()
                .get(group_member_ordinal)
                .map(|member| &member.value)
                == Some(&operand.record_index())
    });
    let group = groups.next()?;
    groups.next().is_none().then_some(group)
}

/// Bind one recipe reference to every live face or edge fragment carrying its
/// token and Design reference in the recipe-state topology.
fn bind_historical_recipe_reference_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    reference: &mut crate::records::dimensions::DesignRecipeReference,
    topology: &AsmHistoricalTopology,
) -> Result<(), cadmpeg_core::CodecError> {
    decode.clear_vec(
        &mut reference.candidate_faces,
        "clear F3D recipe reference candidate faces",
    )?;
    decode.clear_vec(
        &mut reference.candidate_edges,
        "clear F3D recipe reference candidate edges",
    )?;
    decode.clear_vec(
        &mut reference.alternate_selector_faces,
        "clear F3D recipe reference alternate faces",
    )?;
    decode.clear_vec(
        &mut reference.alternate_selector_edges,
        "clear F3D recipe reference alternate edges",
    )?;
    let mut live_faces = HashSet::new();
    for face in decode.admit_iter(&topology.faces, "scan F3D topology faces")? {
        decode.insert_hash_set(&mut live_faces, *face, "index F3D live recipe faces")?;
    }
    let mut live_edges = HashSet::new();
    for edge in decode.admit_iter(&topology.edges, "scan F3D topology edges")? {
        decode.insert_hash_set(&mut live_edges, *edge, "index F3D live recipe edges")?;
    }
    for tag in topology.persistent_subentity_tags.iter().filter(|tag| {
        tag.token == reference.token && tag.design_references.contains(&reference.design_reference)
    }) {
        match tag.entity_kind {
            AsmHistoricalEntityKind::Face if live_faces.contains(&tag.entity_ref) => {
                decode.reserve_vec(
                    &mut reference.candidate_faces,
                    1,
                    "collect F3D recipe reference faces",
                )?;
                reference
                    .candidate_faces
                    .push(historical_face_id(decode, tag.entity_ref)?);
            }
            AsmHistoricalEntityKind::Edge if live_edges.contains(&tag.entity_ref) => {
                decode.reserve_vec(
                    &mut reference.candidate_edges,
                    1,
                    "collect F3D recipe reference edges",
                )?;
                reference
                    .candidate_edges
                    .push(historical_edge_id(decode, tag.entity_ref)?);
            }
            _ => {}
        }
    }
    decode.stable_sort_by(
        &mut reference.candidate_faces,
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D recipe reference faces",
    )?;
    reference.candidate_faces.dedup();
    decode.stable_sort_by(
        &mut reference.candidate_edges,
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D recipe reference edges",
    )?;
    reference.candidate_edges.dedup();
    Ok(())
}

fn historical_recipe_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    design_reference: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    let mut live_faces = HashSet::new();
    for face in decode.admit_iter(&topology.faces, "scan F3D topology faces")? {
        decode.insert_hash_set(&mut live_faces, *face, "index F3D historical recipe faces")?;
    }
    let mut faces = Vec::new();
    for tag in decode.admit_iter(
        &topology.persistent_subentity_tags,
        "scan F3D topology persistent subentity tags",
    )? {
        if tag.entity_kind == AsmHistoricalEntityKind::Face
            && live_faces.contains(&tag.entity_ref)
            && tag.design_references.contains(&design_reference)
        {
            decode.reserve_vec(&mut faces, 1, "collect F3D historical recipe faces")?;
            faces.push(historical_face_id(decode, tag.entity_ref)?);
        }
    }
    decode.stable_sort_by(
        &mut faces,
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D historical recipe faces",
    )?;
    faces.dedup();
    Ok(faces)
}

fn direct_face_recipe_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    recipe_kind: crate::records::recipes::ConstructionRecipeKind,
    references: &[crate::records::dimensions::DesignRecipeReference],
    recipe_record_index: i32,
) -> Result<Option<Vec<cadmpeg_ir::ids::FaceId>>, cadmpeg_core::CodecError> {
    if recipe_kind != crate::records::recipes::ConstructionRecipeKind::Face {
        return Ok(None);
    }
    let mut faces = Vec::new();
    for face in references
        .iter()
        .filter(|reference| reference.design_reference == i64::from(recipe_record_index))
        .flat_map(|reference| &reference.candidate_faces)
    {
        decode.reserve_vec(&mut faces, 1, "collect F3D direct face recipe candidates")?;
        faces.push((face).try_clone_for_decode(decode, "copy F3D historical face identity")?);
    }
    decode.stable_sort_by(
        &mut faces,
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D direct face recipe candidates",
    )?;
    faces.dedup();
    Ok((!faces.is_empty()).then_some(faces))
}

fn historical_face_id(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    slot: i64,
) -> Result<cadmpeg_ir::ids::FaceId, cadmpeg_core::CodecError> {
    let ctx = decode;
    let text = ctx.format_retained(
        format_args!("f3d:brep:entity#{slot}"),
        "retain F3D historical face identity",
    )?;
    ctx.charge_work(
        u64_from_index(text.len()),
        "validate F3D historical face identity",
    )?;
    cadmpeg_ir::ids::FaceId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

fn historical_edge_id(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    slot: i64,
) -> Result<cadmpeg_ir::ids::EdgeId, cadmpeg_core::CodecError> {
    let ctx = decode;
    let text = ctx.format_retained(
        format_args!("f3d:brep:entity#{slot}"),
        "retain F3D historical edge identity",
    )?;
    ctx.charge_work(
        u64_from_index(text.len()),
        "validate F3D historical edge identity",
    )?;
    cadmpeg_ir::ids::EdgeId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

pub(crate) fn bind_face_operand_history_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::face::DesignFaceOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    operand_groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    if projection_was_finalized(decode, histories)? {
        return Ok(());
    }
    let mut recipe_record_indices_storage =
        decode.reserve_scoped(0, "index F3D face operand recipe records")?;
    let mut recipe_record_indices = HashMap::new();
    for recipe in decode.admit_iter(recipes, "scan F3D face operand recipe records")? {
        let Some(record_index) = recipe.record_index else {
            continue;
        };
        recipe_record_indices_storage.with_storage(|| {
            decode
                .insert_hash_map(
                    &mut recipe_record_indices,
                    recipe.id.as_str(),
                    record_index.value,
                    "index F3D face operand recipe records",
                )
                .map(|_| ())
        })?;
    }
    for operand in &mut *operands {
        decode.clear_vec(
            &mut operand.preceding_candidate_faces,
            "clear F3D preceding face candidates",
        )?;
        decode.clear_vec(
            &mut operand.changed_candidate_faces,
            "clear F3D changed face candidates",
        )?;
        decode.clear_vec(
            &mut operand.historical_support_contexts,
            "clear F3D historical face support contexts",
        )?;
        operand.resolved_face_slots.clear();
        operand.resolved_active_face = None;
        let stream = crate::ids::native_stream(&operand.id);
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == operand.scope_record_index
                && crate::ids::native_stream(&scope.id) == stream
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let scoped_history = if decode.contains_key_hash_map(
            scope_histories,
            &scope.id,
            "check F3D profile scope history binding",
        )? {
            let Some(history) = bound_scope_history(decode, &scope.id, scope_histories, histories)?
            else {
                continue;
            };
            Some(history)
        } else {
            None
        };
        let scoped_histories = scoped_history.map_or(histories, std::slice::from_ref);
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let Some(previous_state_id) =
            effective_scope_previous_history_state_id(decode, scope, scoped_histories)?
        else {
            continue;
        };
        let Some((history, state, previous)) = (if scoped_history.is_some() {
            bound_history_state_pair(
                decode,
                &scope.id,
                state_id,
                previous_state_id,
                scope_histories,
                histories,
            )?
        } else {
            unique_history_state_pair(decode, histories, state_id, previous_state_id)?
        }) else {
            continue;
        };
        let states = history_state_index(decode, history)?;
        let Some(topology) = previous.topology() else {
            continue;
        };
        for reference in &mut operand.recipe_references {
            bind_historical_recipe_reference_candidates(decode, reference, topology)?;
        }
        if let Some(recipe_record_index) = decode.get_hash_map(
            &recipe_record_indices,
            operand.recipe_id.as_str(),
            "find F3D face operand recipe record",
        )? {
            operand.candidate_faces =
                historical_recipe_faces(decode, i64::from(*recipe_record_index), topology)?;
            operand.unreferenced_candidate_faces = decode.try_collect_vec(
                (operand.candidate_faces.iter().filter(|face| {
                    !operand
                        .recipe_references
                        .iter()
                        .flat_map(|reference| &reference.candidate_faces)
                        .any(|candidate| candidate == *face)
                }))
                .map(|face| face.try_clone_for_decode(decode, "copy F3D historical face identity")),
                "collect F3D unreferenced candidate faces",
            )?;
            decode.clear_vec(
                &mut operand.alternate_selector_candidate_faces,
                "clear F3D alternate selector face candidates",
            )?;
        }
        let Some(changed_faces) =
            face_changes_across_state_chain(decode, state, previous_state_id, &states)?
        else {
            continue;
        };
        let direct_face_candidates = if let Some(record_index) = decode.get_hash_map(
            &recipe_record_indices,
            operand.recipe_id.as_str(),
            "find F3D direct face recipe record",
        )? {
            direct_face_recipe_candidates(
                decode,
                operand.recipe_kind,
                &operand.recipe_references,
                *record_index,
            )?
        } else {
            None
        };
        let feature_family = crate::design::design_feature_family(&scope.kind());
        let thread_face_candidates = if feature_family
            == Some(crate::design::DesignFeatureFamily::Thread)
            && exact_face_selection_group(operand, scope, operand_groups).is_some()
        {
            operand
                .recipe_references
                .first()
                .and_then(|reference| {
                    let candidates = effective_faces(reference);
                    (!candidates.is_empty()).then_some(candidates)
                })
                .map(|candidates| {
                    decode.try_collect_vec(
                        (candidates).iter().map(|face| {
                            face.try_clone_for_decode(decode, "copy F3D historical face identity")
                        }),
                        "copy F3D thread face candidates",
                    )
                })
                .transpose()?
        } else {
            None
        };
        let nested_split_face_candidates = if scope.kind()
            == crate::records::feature::scope::DesignFeatureKind::SplitFace
            && exact_face_selection_group(operand, scope, operand_groups).is_some()
        {
            crate::design::face_resolve::nested_bounded_face_history_candidates(decode, operand)?
        } else {
            None
        };
        let grouped_reference_face_candidates = (!matches!(
            feature_family,
            Some(
                crate::design::DesignFeatureFamily::Thread
                    | crate::design::DesignFeatureFamily::Split
            )
        ))
        .then(|| grouped_reference_face_candidate(decode, operand, topology, &changed_faces))
        .transpose()?
        .flatten()
        .map(|face| vec![face]);
        let legacy_face_candidates = (|| -> Result<Option<_>, cadmpeg_core::CodecError> {
            if feature_family != Some(crate::design::DesignFeatureFamily::Extrude) {
                return Ok(None);
            }
            let Some(group_record_index) = operand.group_record_index() else {
                return Ok(None);
            };
            let mut groups = operand_groups.iter().filter(|group| {
                crate::ids::native_stream(&group.id) == stream
                    && group.scope_record_index == scope.record_index
                    && group.record_index == group_record_index
                    && group.extrude_role().is_some_and(|role| {
                        matches!(
                            role,
                            crate::records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_)
                        )
                    })
                    && group.extrude_face_role().is_some()
            });
            if groups.next().is_none() {
                return Ok(None);
            }
            if groups.next().is_some() {
                return Ok(None);
            }
            let Some(recipe_record_index) = decode.get_hash_map(
                &recipe_record_indices,
                operand.recipe_id.as_str(),
                "find F3D edge operand recipe record",
            )?
            else {
                return Ok(None);
            };
            crate::design::face_resolve::legacy_face_recipe_reference_candidates(
                decode,
                operand,
                *recipe_record_index,
            )
        })()?;
        let fallback_candidates;
        let history_candidates = if let Some(candidates) = direct_face_candidates
            .as_deref()
            .or(thread_face_candidates.as_deref())
            .or(nested_split_face_candidates.as_deref())
            .or(grouped_reference_face_candidates.as_deref())
        {
            candidates
        } else {
            fallback_candidates =
                crate::design::face_resolve::historical_face_operand_candidates(decode, operand)?;
            &fallback_candidates
        };
        operand.preceding_candidate_faces =
            selection::faces_in_topology(decode, history_candidates, topology)?;
        let mut changed_candidate_faces = Vec::new();
        for face in decode.admit_iter(
            operand.preceding_candidate_faces.as_slice(),
            "scan F3D changed candidate faces",
        )? {
            if stable_ref(decode, face.as_str())?.is_some_and(|slot| changed_faces.contains(&slot))
            {
                let face =
                    face.try_clone_for_decode(decode, "copy F3D historical face identity")?;
                decode.push_vec(
                    &mut changed_candidate_faces,
                    face,
                    "collect F3D changed candidate faces",
                )?;
            }
        }
        operand.changed_candidate_faces = changed_candidate_faces;
        operand.historical_support_contexts = historical_face_support_contexts(
            decode,
            history_candidates,
            history,
            topology,
            &changed_faces,
        )?;
        if direct_face_candidates.is_some() {
            let mut resolved_face_slots = Vec::new();
            for face in decode.admit_iter(
                operand.preceding_candidate_faces.as_slice(),
                "scan F3D direct resolved face slots",
            )? {
                if let Some(slot) = stable_ref(decode, face.as_str())? {
                    decode.push_vec(
                        &mut resolved_face_slots,
                        slot,
                        "collect F3D direct resolved face slots",
                    )?;
                }
            }
            operand.resolved_face_slots = resolved_face_slots;
            continue;
        }
        let preserves_stable_face_set = feature_family
            == Some(crate::design::DesignFeatureFamily::Shell)
            || operand
                .group_record_index()
                .is_some_and(|group_record_index| {
                    let mut groups = operand_groups.iter().filter(|group| {
                        crate::ids::native_stream(&group.id) == stream
                            && group.scope_record_index == scope.record_index
                            && group.record_index == group_record_index
                    });
                    let Some(group) = groups.next() else {
                        return false;
                    };
                    groups.next().is_none()
                        && group.extrude_face_role()
                            == Some(crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Termination)
                });
        operand.resolved_face_slots = match &scope.payload() {
            crate::records::feature::scope::DesignScopePayload::OffsetFaces(Some(_))
            | crate::records::feature::scope::DesignScopePayload::DecalerLesFaces(Some(_)) => {
                let direct = resolve_direct_face_recipe_clauses(
                    decode,
                    &operand.recipe_references,
                    topology,
                    &changed_faces,
                )?;
                if direct.is_empty() {
                    crate::design::face_resolve::resolve_face_operand_history_candidates(operand)
                        .into_iter()
                        .collect()
                } else {
                    direct
                }
            }
            _ => {
                let direct =
                    crate::design::face_resolve::resolve_face_operand_history_candidates(operand);
                if let Some(direct) = direct {
                    vec![direct]
                } else if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                {
                    crate::design::face_resolve::resolve_surface_delete_face_history_set(
                        decode, operand,
                    )?
                    .unwrap_or_default()
                } else if preserves_stable_face_set {
                    if let Some(stable) =
                        crate::design::face_resolve::resolve_stable_bounded_face_history_set(
                            decode, operand,
                        )?
                    {
                        stable
                    } else {
                        crate::design::face_resolve::resolve_bounded_face_history_candidates(
                            decode, operand,
                        )?
                        .unwrap_or_default()
                    }
                } else if let Some(bounded) =
                    crate::design::face_resolve::resolve_bounded_face_history_candidates(
                        decode, operand,
                    )?
                {
                    bounded
                } else {
                    let pattern = if feature_family
                        == Some(crate::design::DesignFeatureFamily::CircularPattern)
                    {
                        if let Some(result) = state.topology() {
                            resolve_pattern_face_by_surface_radius(
                                decode,
                                crate::design::face_resolve::face_operand_candidates(operand),
                                topology,
                                result,
                                &changed_faces,
                            )?
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    pattern.into_iter().collect()
                }
            }
        };
        if let Some(candidates) = &thread_face_candidates {
            // Thread's first prefix reference is its exclusive selection lane.
            // An unresolved lane must not fall back to construction context.
            let resolved =
                match crate::design::face_resolve::resolve_face_operand_history_candidate_from(
                    operand, candidates,
                ) {
                    Some(face) => Some(face),
                    None => resolve_thread_face_by_transition(
                        decode,
                        scope,
                        candidates,
                        history,
                        topology,
                        &changed_faces,
                    )?,
                };
            operand.resolved_face_slots = resolved.into_iter().collect();
        }
        if let Some(candidates) = &grouped_reference_face_candidates {
            // A grouped frame's unique changed topology-face reference is its
            // exact historical selection lane.
            operand.resolved_face_slots =
                crate::design::face_resolve::resolve_face_operand_history_candidate_from(
                    operand, candidates,
                )
                .into_iter()
                .collect();
        }
        if feature_family == Some(crate::design::DesignFeatureFamily::Split) {
            operand.resolved_face_slots = resolve_split_tool_face(decode, operand, topology)?
                .into_iter()
                .collect();
        }
        if feature_family == Some(crate::design::DesignFeatureFamily::Loft)
            && operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::BoundedFace
            && state
                .transition
                .as_ref()
                .is_some_and(|transition| transition.previous_state_id == Some(previous_state_id))
        {
            if let Some((result, transition)) = state.topology().zip(state.transition.as_ref()) {
                if let Some(face) = resolve_bounded_face_recipe_target(
                    decode,
                    operand,
                    topology,
                    result,
                    &transition.topology.bodies.inserted,
                )? {
                    operand.resolved_face_slots = vec![face];
                }
            }
        }
        if operand.resolved_face_slots.is_empty()
            && scope.kind() == crate::records::feature::scope::DesignFeatureKind::Draft
        {
            if let Some(result) = state.topology() {
                if let Some(face) =
                    resolve_draft_face_by_surface_transition(decode, operand, topology, result)?
                {
                    operand.resolved_face_slots = vec![face];
                }
            }
        }
        if operand.resolved_face_slots.is_empty() {
            if let Some(candidates) = legacy_face_candidates {
                let history_source = historical_brep_source(decode, &history.id)?;
                match select_legacy_extrude_face_candidate(
                    decode,
                    candidates,
                    topology,
                    &changed_faces,
                    history_source,
                )? {
                    Some(LegacyFaceResolution::Historical(slot)) => {
                        operand.resolved_face_slots = vec![slot];
                    }
                    Some(LegacyFaceResolution::Active(face)) => {
                        operand.resolved_active_face = Some(face);
                    }
                    None => {}
                }
            }
        }
    }
    bind_profile_face_group_cardinality(
        decode,
        operands,
        scopes,
        operand_groups,
        histories,
        scope_histories,
    )?;
    Ok(())
}

/// Resolve a Draft face whose persistent selector lane is ambiguous in the
/// active B-rep by following the surface carrier through the exact Draft
/// transition.
///
/// Draft changes the selected face's actual surface geometry. The same
/// transition may update tangent or context faces, so the transition is
/// admissible only when exactly one face from the operand's alternate lane (or,
/// when no alternate lane exists, its exact candidate lane) changes surface
/// geometry. A missing, duplicate, or unchanged surface is not a selection
/// proof. Carrier record identities are not compared: a history update may
/// assign a new carrier record without changing the surface.
#[derive(Clone, Copy, PartialEq)]
enum DraftSurfaceGeometry {
    Plane {
        origin: cadmpeg_ir::math::Point3,
        normal: cadmpeg_ir::math::Vector3,
    },
    Cylinder {
        origin: cadmpeg_ir::math::Point3,
        axis: cadmpeg_ir::math::Vector3,
        radius: f64,
    },
    Axis {
        origin: cadmpeg_ir::math::Point3,
        direction: cadmpeg_ir::math::Vector3,
        radius: Option<f64>,
    },
}

fn draft_surface_geometry(
    topology: &crate::history_records::AsmHistoricalTopology,
    face: i64,
) -> Option<DraftSurfaceGeometry> {
    let mut bindings = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == face)
        .map(|binding| binding.carrier);
    let carrier = bindings.next()?;
    if bindings.next().is_some() {
        return None;
    }
    if let Some(plane) = topology
        .surface_planes
        .iter()
        .find(|surface| surface.surface == carrier)
    {
        return Some(DraftSurfaceGeometry::Plane {
            origin: plane.origin,
            normal: plane.normal,
        });
    }
    if let Some(cylinder) = topology
        .surface_cylinders
        .iter()
        .find(|surface| surface.surface == carrier)
    {
        return Some(DraftSurfaceGeometry::Cylinder {
            origin: cylinder.origin,
            axis: cylinder.axis,
            radius: cylinder.radius,
        });
    }
    topology
        .surface_axes
        .iter()
        .find(|surface| surface.surface == carrier)
        .map(|axis| DraftSurfaceGeometry::Axis {
            origin: axis.origin,
            direction: axis.direction,
            radius: topology
                .surface_radii
                .iter()
                .find(|radius| radius.surface == carrier)
                .map(|radius| radius.radius),
        })
}

fn resolve_draft_face_by_surface_transition(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !matches!(
            crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program),
            Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { .. })
        )
        || operand.recipe_nodes.is_empty()
    {
        return Ok(None);
    }
    let mut candidate_slots = HashSet::new();
    for face in decode.admit_iter(&operand.candidate_faces, "scan F3D draft face candidates")? {
        if let Some(face_slot) = stable_ref(decode, face.as_str())? {
            decode.insert_hash_set(
                &mut candidate_slots,
                face_slot,
                "index F3D draft face candidates",
            )?;
        }
    }
    if candidate_slots.is_empty() {
        return Ok(None);
    }
    let mut alternate_slots = BTreeSet::new();
    for reference in decode.admit_iter(
        &operand.recipe_references,
        "scan F3D draft alternate face references",
    )? {
        for face in decode.admit_iter(
            &reference.alternate_selector_faces,
            "scan F3D draft alternate faces",
        )? {
            let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                continue;
            };
            if candidate_slots.contains(&face_slot) {
                decode
                    .insert_btree_set(
                        &mut alternate_slots,
                        face_slot,
                        "collect F3D draft alternate faces",
                    )
                    .map(|_| ())?;
            }
        }
    }
    let mut exact_slots = BTreeSet::new();
    for reference in decode.admit_iter(
        &operand.recipe_references,
        "scan F3D draft exact face references",
    )? {
        for face in decode.admit_iter(&reference.candidate_faces, "scan F3D draft exact faces")? {
            let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                continue;
            };
            if candidate_slots.contains(&face_slot) {
                decode
                    .insert_btree_set(&mut exact_slots, face_slot, "collect F3D draft exact faces")
                    .map(|_| ())?;
            }
        }
    }
    let has_alternates = !alternate_slots.is_empty();
    let candidates = if has_alternates {
        alternate_slots
    } else {
        exact_slots
    };
    if candidates.is_empty() {
        return Ok(None);
    }
    if !has_alternates {
        let mut faces = candidates.iter();
        let Some(face) = faces.next().copied() else {
            return Ok(None);
        };
        if faces.next().is_some() {
            return Ok(None);
        }
        return Ok(
            (preceding.faces.contains(&face) && result.faces.contains(&face)).then_some(face),
        );
    }
    let mut changed = None;
    for face in candidates
        .into_iter()
        .filter(|face| preceding.faces.contains(face) && result.faces.contains(face))
        .filter(|face| {
            draft_surface_geometry(preceding, *face)
                .zip(draft_surface_geometry(result, *face))
                .is_some_and(|(before, after)| before != after)
        })
    {
        if changed.replace(face).is_some() {
            return Ok(None);
        }
    }
    Ok(changed)
}

fn resolve_pattern_face_by_surface_radius(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[cadmpeg_ir::ids::FaceId],
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let mut candidate_faces = HashSet::new();
    for face in decode.admit_iter(candidates, "scan F3D pattern face candidates")? {
        if let Some(face_slot) = stable_ref(decode, face.as_str())? {
            decode.insert_hash_set(
                &mut candidate_faces,
                face_slot,
                "index F3D pattern face candidates",
            )?;
        }
    }
    if candidate_faces.is_empty() {
        return Ok(None);
    }
    let mut result_radius = None;
    let mut bound_candidates = HashSet::new();
    for binding in result
        .face_surfaces
        .iter()
        .filter(|binding| candidate_faces.contains(&binding.entity))
    {
        if !decode.insert_hash_set(
            &mut bound_candidates,
            binding.entity,
            "index F3D pattern bound faces",
        )? {
            return Ok(None);
        }
        let mut radii = result
            .surface_radii
            .iter()
            .filter(|radius| radius.surface == binding.carrier);
        let Some(radius) = radii.next().map(|radius| radius.radius) else {
            return Ok(None);
        };
        if radii.next().is_some() || !radius.is_finite() || radius <= 0.0 {
            return Ok(None);
        }
        match result_radius {
            None => result_radius = Some(radius.to_bits()),
            Some(expected) if expected == radius.to_bits() => {}
            Some(_) => return Ok(None),
        }
    }
    // Every bound face passed the candidate filter, so equal sizes mean that
    // each candidate face is bound.
    if bound_candidates.len() != candidate_faces.len() {
        return Ok(None);
    }
    let Some(result_radius) = result_radius else {
        return Ok(None);
    };
    let mut matches = preceding
        .face_surfaces
        .iter()
        .filter(|binding| changed_faces.contains(&binding.entity))
        .filter_map(|binding| {
            let mut radii = preceding
                .surface_radii
                .iter()
                .filter(|radius| radius.surface == binding.carrier);
            let radius = radii.next()?;
            (radii.next().is_none() && radius.radius.to_bits() == result_radius)
                .then_some(binding.entity)
        });
    let Some(face) = matches.next() else {
        return Ok(None);
    };
    Ok(matches.next().is_none().then_some(face))
}

fn resolve_split_tool_face(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    if operand.group_record_index().is_some()
        || operand.group_member_ordinal().is_some()
        || operand.scope_reference_ordinal != 1
        || operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::Face
        || operand.recipe_program != [0, -1]
    {
        return Ok(None);
    }
    let [reference] = operand.recipe_references.as_slice() else {
        return Ok(None);
    };
    let candidates = selection::faces_in_topology(decode, &reference.candidate_faces, topology)?;
    let [face] = candidates.as_slice() else {
        return Ok(None);
    };
    stable_ref(decode, face.as_str())
}

fn effective_faces(
    reference: &crate::records::dimensions::DesignRecipeReference,
) -> &[cadmpeg_ir::ids::FaceId] {
    if reference.candidate_faces.is_empty() {
        &reference.alternate_selector_faces
    } else {
        &reference.candidate_faces
    }
}

fn resolve_thread_face_by_transition(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
    candidates: &[cadmpeg_ir::ids::FaceId],
    history: &AsmHistory,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(construction) = scope.thread_construction() else {
        return Ok(None);
    };
    let Some(source) = historical_brep_source(decode, &history.id)? else {
        return Ok(None);
    };
    let mut is_source_face =
        |face: &cadmpeg_ir::ids::FaceId| active_brep_face_matches_source(decode, face, source);
    let Some(source_face_index) = decode.position_by(
        candidates,
        &mut is_source_face,
        "find F3D Thread source face candidate",
    )?
    else {
        return Ok(None);
    };
    if decode
        .position_by(
            &candidates[source_face_index + 1..],
            &mut is_source_face,
            "find additional F3D Thread source face candidate",
        )?
        .is_some()
    {
        return Ok(None);
    }
    let minimum_radius = construction.diameters.minor() * 5.0;
    let maximum_radius = construction.diameters.major() * 5.0;
    if !minimum_radius.is_finite() || !maximum_radius.is_finite() {
        return Ok(None);
    }
    let tolerance = EPS_HISTORY_RESOLVE_THREAD_FACE_BY_TRANSITION_E9 * (1.0 + maximum_radius.abs());
    let mut matching_faces = topology
        .faces
        .iter()
        .copied()
        .filter(|face| changed_faces.contains(face))
        .filter_map(|face| {
            let mut bindings = topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.entity == face);
            let binding = bindings.next()?;
            if bindings.next().is_some() {
                return None;
            }
            let mut cylinders = topology
                .surface_cylinders
                .iter()
                .filter(|cylinder| cylinder.surface == binding.carrier);
            let cylinder = cylinders.next()?;
            (cylinders.next().is_none()
                && cylinder.radius + tolerance >= minimum_radius
                && cylinder.radius <= maximum_radius + tolerance)
                .then_some(face)
        });
    let Some(face) = matching_faces.next() else {
        return Ok(None);
    };
    Ok(matching_faces
        .all(|candidate| candidate == face)
        .then_some(face))
}

fn grouped_reference_face_candidate(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Result<Option<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(
            decode,
            &operand.recipe_prefix_bytes,
        )?
    {
        return Ok(None);
    }
    let mut face = None;
    for reference in decode.admit_iter(
        &operand.recipe_references,
        "scan F3D grouped recipe face references",
    )? {
        let candidate = reference.design_reference;
        if !decode.contains(
            &topology.faces,
            &candidate,
            "check F3D grouped topology face membership",
        )? || !decode.contains_hash_set(
            changed_faces,
            &candidate,
            "check F3D grouped changed face membership",
        )? {
            continue;
        }
        if let Some(expected) = face {
            if !decode.equal(
                &candidate,
                &expected,
                "compare F3D grouped recipe candidate faces",
            )? {
                return Ok(None);
            }
        } else {
            face = Some(candidate);
        }
    }
    face.map(|face| historical_face_id(decode, face))
        .transpose()
}

fn relation_members(
    relations: &[crate::history_records::AsmHistoricalRelation],
    owner: i64,
) -> Option<&[i64]> {
    let mut matches = relations
        .iter()
        .filter(|relation| relation.owner_ref == owner);
    let members = matches.next()?.member_refs.as_slice();
    matches.next().is_none().then_some(members)
}

fn resolve_bounded_face_recipe_target(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
    inserted_bodies: &[i64],
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { header_value }) =
        crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)
    else {
        return Ok(None);
    };
    if operand.recipe_nodes.len() != header_value
        || operand
            .recipe_nodes
            .iter()
            .any(|node| node.recipe_structure.is_none())
    {
        return Ok(None);
    }
    let Some(first) = operand.recipe_references.first() else {
        return Ok(None);
    };
    let first_clause_len = operand
        .recipe_references
        .iter()
        .take_while(|reference| {
            reference.selector_offset == first.selector_offset
                && reference.token_offset == first.token_offset
        })
        .count();
    let first_clause = &operand.recipe_references[..first_clause_len];
    let mut topology_faces = HashSet::new();
    for face in decode.admit_iter(&preceding.faces, "scan F3D preceding faces")? {
        decode.insert_hash_set(
            &mut topology_faces,
            *face,
            "index F3D bounded topology faces",
        )?;
    }
    let mut target_candidates = BTreeSet::new();
    if let Some(reference) = first_clause.first() {
        for face in
            decode.admit_iter(effective_faces(reference), "scan F3D bounded target faces")?
        {
            let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                continue;
            };
            if topology_faces.contains(&face_slot) {
                decode
                    .insert_btree_set(
                        &mut target_candidates,
                        face_slot,
                        "collect F3D bounded target faces",
                    )
                    .map(|_| ())?;
            }
        }
    }
    for reference in decode.admit_iter(&first_clause[1..], "scan F3D bounded face clauses")? {
        let mut candidates = HashSet::new();
        for face in
            decode.admit_iter(effective_faces(reference), "scan F3D bounded clause faces")?
        {
            let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                continue;
            };
            if topology_faces.contains(&face_slot) {
                decode.insert_hash_set(
                    &mut candidates,
                    face_slot,
                    "collect F3D bounded clause faces",
                )?;
            }
        }
        target_candidates.retain(|face| candidates.contains(face));
    }
    let mut construction_faces = Vec::new();
    'bodies: for body in decode.admit_iter(inserted_bodies, "scan F3D inserted bodies")? {
        let Some(regions) = relation_members(&result.body_regions, *body) else {
            continue;
        };
        let mut unique = None;
        let mut ambiguous = false;
        for region in regions {
            let Some(shells) = relation_members(&result.region_shells, *region) else {
                continue 'bodies;
            };
            for shell in shells {
                let Some(faces) = relation_members(&result.shell_faces, *shell) else {
                    continue 'bodies;
                };
                for face in faces {
                    if unique.is_some_and(|prior| prior != *face) {
                        ambiguous = true;
                    }
                    unique = Some(*face);
                }
            }
        }
        if !ambiguous {
            if let Some(face) = unique {
                decode.reserve_vec(&mut construction_faces, 1, "collect F3D construction faces")?;
                construction_faces.push(face);
            }
        }
    }
    if construction_faces.is_empty() {
        return Ok(None);
    }
    let face_loop_positions = |face,
                               topology|
     -> Result<
        Option<(usize, Vec<cadmpeg_ir::math::Point3>)>,
        cadmpeg_core::CodecError,
    > {
        let contexts = face_boundary_contexts_for_slots(decode, &[face], topology)?;
        let [context] = contexts.as_slice() else {
            return Ok(None);
        };
        let [loop_] = context.loops.as_slice() else {
            return Ok(None);
        };
        let crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Positions(
            rows,
        ) = &loop_.boundary
        else {
            return Ok(None);
        };
        if rows.is_empty() {
            return Ok(None);
        }
        let positions = decode.collect_vec(
            rows.iter().map(|row| row.position),
            "collect F3D bounded loop positions",
        )?;
        Ok(Some((rows.len(), positions)))
    };
    let mut matches = Vec::new();
    for candidate in target_candidates {
        let Some((edge_count, candidate_points)) = face_loop_positions(candidate, preceding)?
        else {
            continue;
        };
        if edge_count != header_value {
            continue;
        }
        let mut matched = false;
        for face in decode.admit_iter(&construction_faces, "scan F3D construction faces")? {
            if let Some((construction_edge_count, construction_points)) =
                face_loop_positions(*face, result)?
            {
                if construction_edge_count >= edge_count
                    && cyclic_point_subsequence(&candidate_points, &construction_points)
                {
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            decode.reserve_vec(&mut matches, 1, "collect F3D bounded face matches")?;
            matches.push(candidate);
        }
    }
    decode.sort_unstable_by(
        &mut matches,
        |value| value,
        Ord::cmp,
        "sort F3D bounded face matches",
    )?;
    matches.dedup();
    let [face] = matches.as_slice() else {
        return Ok(None);
    };
    Ok(Some(*face))
}

fn cyclic_point_subsequence(
    candidate: &[cadmpeg_ir::math::Point3],
    construction: &[cadmpeg_ir::math::Point3],
) -> bool {
    let coincident = |left: &cadmpeg_ir::math::Point3, right: &cadmpeg_ir::math::Point3| {
        let dx = left.x - right.x;
        let dy = left.y - right.y;
        let dz = left.z - right.z;
        dx.mul_add(dx, dy.mul_add(dy, dz * dz)) <= EPS_HISTORY_CYCLIC_POINT_SUBSEQUENCE_E12
    };
    let matches_orientation = |candidate: &[cadmpeg_ir::math::Point3]| {
        construction.iter().enumerate().any(|(start, point)| {
            if !coincident(&candidate[0], point) {
                return false;
            }
            let mut cursor = start;
            candidate.iter().skip(1).all(|target| {
                let limit = start + construction.len();
                while cursor < limit {
                    cursor += 1;
                    if coincident(target, &construction[cursor % construction.len()]) {
                        return true;
                    }
                }
                false
            })
        })
    };
    if candidate.is_empty() || candidate.len() > construction.len() {
        return false;
    }
    if matches_orientation(candidate) {
        return true;
    }
    let reversed_matches = construction.iter().enumerate().any(|(start, point)| {
        if !coincident(&candidate[candidate.len() - 1], point) {
            return false;
        }
        let mut cursor = start;
        candidate.iter().rev().skip(1).all(|target| {
            let limit = start + construction.len();
            while cursor < limit {
                cursor += 1;
                if coincident(target, &construction[cursor % construction.len()]) {
                    return true;
                }
            }
            false
        })
    });
    reversed_matches
}

pub(crate) fn bind_body_recipe_operand_history_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    if projection_was_finalized(decode, histories)? {
        return Ok(());
    }
    for operand in operands.iter_mut() {
        for reference in operand.reference_bindings_mut() {
            decode.clear_vec(
                reference.preceding_candidate_faces,
                "clear F3D preceding body recipe face candidates",
            )?;
            reference.preceding_body_slots.clear();
        }
        operand.resolved_face_slot = None;
        operand.resolved_body_state_id = None;
        operand.resolved_body_slot = None;
        operand.resolved_body_face_slots.clear();
        let Some((history, state, previous)) =
            body_recipe_operand_history_pair(decode, operand, scopes, histories)?
        else {
            continue;
        };
        let states = history_state_index(decode, history)?;
        let Some(topology) = previous.topology() else {
            continue;
        };
        if face_changes_across_state_chain(decode, state, previous.state_id, &states)?.is_none() {
            continue;
        }
        let Some(source) = historical_brep_source(decode, &previous.id)? else {
            continue;
        };
        for reference in operand.reference_bindings_mut() {
            let mut topology_faces_storage =
                decode.reserve_scoped(0, "index F3D topology faces")?;
            let mut topology_faces = HashSet::new();
            topology_faces_storage.with_storage(|| {
                for face_slot in decode.admit_iter(&topology.faces, "scan F3D topology faces")? {
                    decode.insert_hash_set(
                        &mut topology_faces,
                        *face_slot,
                        "index F3D topology faces",
                    )?;
                }
                Ok::<(), cadmpeg_core::CodecError>(())
            })?;
            let mut preceding_candidate_faces = Vec::new();
            for face in decode.admit_iter(
                reference.candidate_faces.as_slice(),
                "scan F3D body recipe candidate faces",
            )? {
                if !active_brep_face_matches_source(decode, face, source)? {
                    continue;
                }
                let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                    continue;
                };
                if topology_faces.contains(&face_slot) {
                    let face =
                        face.try_clone_for_decode(decode, "copy F3D historical face identity")?;
                    decode.push_vec(
                        &mut preceding_candidate_faces,
                        face,
                        "collect F3D faces in topology",
                    )?;
                }
            }
            *reference.preceding_candidate_faces = preceding_candidate_faces;
            let mut face_slots = BTreeSet::new();
            for face in decode.admit_iter(
                reference.preceding_candidate_faces.as_slice(),
                "scan F3D body recipe face slots",
            )? {
                if let Some(face_slot) = stable_ref(decode, face.as_str())? {
                    decode
                        .insert_btree_set(
                            &mut face_slots,
                            face_slot,
                            "index F3D body recipe face slots",
                        )
                        .map(|_| ())?;
                }
            }
            let Some(body_slots) = bodies_intersecting(decode, topology, &face_slots)? else {
                continue;
            };
            *reference.preceding_body_slots =
                decode.collect_vec(body_slots, "collect F3D body recipe preceding bodies")?;
        }
        if let [reference] = operand.references().as_slice() {
            if let [face] = reference.preceding_candidate_faces.as_slice() {
                operand.resolved_face_slot = stable_ref(decode, face.as_str())?;
            }
        }
        let Some(first) = operand.references().first() else {
            continue;
        };
        if first.preceding_body_slots.is_empty() {
            continue;
        }
        if decode.any_by(
            operand.references(),
            |reference| Ok(reference.preceding_body_slots.is_empty()),
            "scan F3D body recipe references with empty body slots",
        )? {
            continue;
        }
        let mut intersection = BTreeSet::new();
        for body in decode.admit_iter(
            &first.preceding_body_slots,
            "scan F3D first preceding body slots",
        )? {
            decode
                .insert_btree_set(
                    &mut intersection,
                    *body,
                    "index F3D body recipe intersection",
                )
                .map(|_| ())?;
        }
        for reference in decode.admit_iter(
            &operand.references()[1..],
            "scan F3D body recipe intersection references",
        )? {
            let mut retain_result = Ok(());
            intersection.retain(|body| {
                if retain_result.is_err() {
                    return false;
                }
                match decode.contains(
                    &reference.preceding_body_slots,
                    body,
                    "check F3D body recipe intersection membership",
                ) {
                    Ok(contains) => contains,
                    Err(error) => {
                        retain_result = Err(error);
                        false
                    }
                }
            });
            retain_result?;
        }
        if intersection.len() == 1 {
            operand.resolved_body_slot = intersection.into_iter().next();
        }
    }
    let mut recipes_by_id_storage = decode.reserve_scoped(0, "index F3D body recipes by id")?;
    let mut recipes_by_id =
        HashMap::<_, Option<&crate::records::recipes::ConstructionRecipe>>::new();
    for recipe in decode.admit_iter(recipes, "scan F3D body recipes")? {
        recipes_by_id_storage.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
            match decode.entry_hash_map(
                &mut recipes_by_id,
                recipe.id.as_str(),
                "index F3D body recipes by id",
            )? {
                std::collections::hash_map::Entry::Occupied(mut entry) => *entry.get_mut() = None,
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(recipe));
                }
            }
            Ok::<(), cadmpeg_core::CodecError>(())
        })?;
    }
    let identity = |operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand| -> Result<Option<_>, cadmpeg_core::CodecError> {
        let Some(stream) = crate::ids::native_stream(&operand.id) else { return Ok(None) };
        let Some(recipe) = decode
            .get_hash_map(
                &recipes_by_id,
                operand.recipe_id.as_str(),
                "find F3D body recipe identity",
            )?
            .copied()
            .flatten()
        else {
            return Ok(None);
        };
        let Some(design) = recipe.design.as_ref() else { return Ok(None) };
        let Some(selector) = design.selector else { return Ok(None) };
        Ok(Some((
            decode.copy_retained_text(stream, "copy F3D body recipe identity stream")?,
            operand
                .asset_id
                .try_clone_for_decode(decode, "copy F3D body recipe asset ID")?,
            operand
                .context_id
                .try_clone_for_decode(decode, "copy F3D body recipe context ID")?,
            decode.collect_vec(operand
                .references()
                .iter()
                .map(|reference| (reference.design_reference, reference.form)), "collect F3D body recipe identity references")?,
            decode.copy_retained_text(&design.id.value, "copy F3D body recipe design id")?,
            selector.value,
        )))
    };
    let mut resolved_by_identity_storage =
        decode.reserve_scoped(0, "index F3D resolved body recipe identities")?;
    let mut resolved_by_identity = HashMap::new();
    for operand in decode.admit_iter(&*operands, "scan F3D operands")? {
        resolved_by_identity_storage.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
            let (Some(identity), Some(body)) = (identity(operand)?, operand.resolved_body_slot)
            else {
                return Ok(());
            };
            match decode.entry_hash_map(
                &mut resolved_by_identity,
                identity,
                "index F3D resolved body recipe identities",
            )? {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    if *entry.get() != Some(body) {
                        *entry.get_mut() = None;
                    }
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(body));
                }
            }
            Ok(())
        })?;
    }
    for operand in operands {
        if operand.resolved_body_slot.is_none() {
            let resolved_body_slot = {
                let mut lookup_identity_storage =
                    decode.reserve_scoped(0, "lookup F3D resolved body recipe identity")?;
                lookup_identity_storage.with_storage(
                    || -> Result<Option<i64>, cadmpeg_core::CodecError> {
                        let Some(identity) = identity(operand)? else {
                            return Ok(None);
                        };
                        Ok(decode
                            .get_hash_map(
                                &resolved_by_identity,
                                &identity,
                                "find F3D resolved body recipe identity",
                            )?
                            .copied()
                            .flatten())
                    },
                )?
            };
            operand.resolved_body_slot = resolved_body_slot;
        }
        let Some(body_slot) = operand.resolved_body_slot else {
            continue;
        };
        let Some((_, _, previous)) =
            body_recipe_operand_history_pair(decode, operand, scopes, histories)?
        else {
            continue;
        };
        let Some(topology) = previous.topology() else {
            continue;
        };
        let Some(faces) = complete_body_face_slots(decode, topology, body_slot)? else {
            continue;
        };
        operand.resolved_body_state_id = Some(previous.state_id);
        operand.resolved_body_face_slots = faces;
    }
    Ok(())
}

fn body_recipe_operand_history_pair<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &'a [AsmHistory],
) -> Result<Option<(&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState)>, cadmpeg_core::CodecError>
{
    let operation = "find F3D body recipe history scope";
    let Some((stream, _)) = decode.rsplit_once(&operand.id, ":", operation)? else {
        return Ok(None);
    };
    let matches_scope = |scope: &crate::records::feature::scope::DesignParameterScope| {
        if scope.record_index != operand.scope_record_index {
            return Ok(false);
        }
        let Some((scope_stream, _)) = decode.rsplit_once(&scope.id, ":", operation)? else {
            return Ok(false);
        };
        decode.equal(scope_stream, stream, operation)
    };
    let Some(index) = decode.position_by(scopes, matches_scope, operation)? else {
        return Ok(None);
    };
    if decode
        .position_by(&scopes[index + 1..], matches_scope, operation)?
        .is_some()
    {
        return Ok(None);
    }
    let scope = &scopes[index];
    let Some(state_id) = scope.history_state_id() else {
        return Ok(None);
    };
    let Some(previous_state_id) =
        effective_scope_previous_history_state_id(decode, scope, histories)?
    else {
        return Ok(None);
    };
    let Some((history, state, previous)) =
        unique_history_state_pair(decode, histories, state_id, previous_state_id)?
    else {
        return Ok(None);
    };
    Ok(Some((history, state, previous)))
}

fn complete_body_face_slots(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
    body: i64,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    macro_rules! complete_some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    fn occurrence_counts(
        decode: &cadmpeg_core::decode::DecodeContext<'_>,
        slots: &[i64],
    ) -> Result<HashMap<i64, usize>, cadmpeg_core::CodecError> {
        let mut counts = HashMap::new();
        for &slot in decode.admit_iter(slots, "scan F3D topology member slots")? {
            if !counts.contains_key(&slot) {
                decode.reserve_map(&mut counts, 1, "index F3D complete body entity counts")?;
            }
            *counts.entry(slot).or_default() += 1;
        }
        Ok(counts)
    }

    struct RelationIndex<'a> {
        members_by_owner: HashMap<i64, Option<&'a [i64]>>,
        owner_by_member: HashMap<i64, Option<i64>>,
    }

    fn relation_index<'a>(
        decode: &cadmpeg_core::decode::DecodeContext<'_>,
        relations: &'a [AsmHistoricalRelation],
    ) -> Result<RelationIndex<'a>, cadmpeg_core::CodecError> {
        let mut members_by_owner = HashMap::new();
        let mut owner_by_member = HashMap::new();
        for relation in relations {
            if !members_by_owner.contains_key(&relation.owner_ref) {
                decode.reserve_map(
                    &mut members_by_owner,
                    1,
                    "index F3D complete body relation owners",
                )?;
            }
            members_by_owner
                .entry(relation.owner_ref)
                .and_modify(|members| *members = None)
                .or_insert(Some(relation.member_refs.as_slice()));
            for &member in
                decode.admit_iter(&relation.member_refs, "scan F3D relation member refs")?
            {
                if !owner_by_member.contains_key(&member) {
                    decode.reserve_map(
                        &mut owner_by_member,
                        1,
                        "index F3D complete body relation members",
                    )?;
                }
                owner_by_member
                    .entry(member)
                    .and_modify(|owner| *owner = None)
                    .or_insert(Some(relation.owner_ref));
            }
        }
        Ok(RelationIndex {
            members_by_owner,
            owner_by_member,
        })
    }

    let body_counts = occurrence_counts(decode, &topology.bodies)?;
    let region_counts = occurrence_counts(decode, &topology.regions)?;
    let shell_counts = occurrence_counts(decode, &topology.shells)?;
    let face_counts = occurrence_counts(decode, &topology.faces)?;
    let body_regions = relation_index(decode, &topology.body_regions)?;
    let region_shells = relation_index(decode, &topology.region_shells)?;
    let shell_faces = relation_index(decode, &topology.shell_faces)?;

    if body_counts.get(&body).copied() != Some(1) {
        return Ok(None);
    }
    let regions = complete_some!(body_regions.members_by_owner.get(&body).copied().flatten());
    if regions.is_empty() {
        return Ok(None);
    }
    let mut seen_regions = HashSet::new();
    let mut seen_shells = HashSet::new();
    let mut seen_faces_storage = decode.reserve_scoped(0, "collect F3D complete body faces")?;
    let mut seen_faces = BTreeSet::new();
    for &region in regions {
        if !decode.insert_hash_set(
            &mut seen_regions,
            region,
            "collect F3D complete body regions",
        )? || region_counts.get(&region).copied() != Some(1)
            || body_regions.owner_by_member.get(&region).copied().flatten() != Some(body)
        {
            return Ok(None);
        }
        let shells = complete_some!(region_shells
            .members_by_owner
            .get(&region)
            .copied()
            .flatten());
        if shells.is_empty() {
            return Ok(None);
        }
        for &shell in shells {
            if !decode.insert_hash_set(
                &mut seen_shells,
                shell,
                "collect F3D complete body shells",
            )? || shell_counts.get(&shell).copied() != Some(1)
                || region_shells.owner_by_member.get(&shell).copied().flatten() != Some(region)
            {
                return Ok(None);
            }
            let faces = complete_some!(shell_faces.members_by_owner.get(&shell).copied().flatten());
            if faces.is_empty() {
                return Ok(None);
            }
            for &face in faces {
                if !decode.insert_scoped_btree_set(
                    &mut seen_faces_storage,
                    &mut seen_faces,
                    face,
                    "collect F3D complete body faces",
                    "collect F3D complete body faces",
                )? || face_counts.get(&face).copied() != Some(1)
                    || shell_faces.owner_by_member.get(&face).copied().flatten() != Some(shell)
                {
                    return Ok(None);
                }
            }
        }
    }
    // The ordered set yields the face slots already sorted.
    let faces = decode.collect_vec(seen_faces, "collect F3D complete body face slots")?;
    Ok((!faces.is_empty()).then_some(faces))
}

fn active_brep_face_matches_source(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    face: &cadmpeg_ir::ids::FaceId,
    source: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    if decode.starts_with(
        face.as_str(),
        "f3d:brep:entity#",
        "match F3D default BREP face prefix",
    )? {
        Ok(true)
    } else {
        face_in_brep_source(decode, face.as_str(), source)
    }
}

fn face_in_brep_source(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    id: &str,
    source: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(tail) = decode.strip_prefix(id, "f3d:brep/", "strip F3D BREP face prefix")? else {
        return Ok(false);
    };
    let Some(tail) = decode.strip_prefix(tail, source, "strip F3D BREP source")? else {
        return Ok(false);
    };
    decode.starts_with(tail, "/", "match F3D BREP source separator")
}

#[derive(Debug, PartialEq)]
enum LegacyFaceResolution {
    Historical(i64),
    Active(cadmpeg_ir::ids::FaceId),
}

fn select_legacy_extrude_face_candidate(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    mut candidates: Vec<cadmpeg_ir::ids::FaceId>,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    history_source: Option<&str>,
) -> Result<Option<LegacyFaceResolution>, cadmpeg_core::CodecError> {
    for changed_only in [true, false] {
        let mut matches_historical_face = |face: &cadmpeg_ir::ids::FaceId| {
            let Some(slot) = stable_ref(decode, face.as_str())? else {
                return Ok(false);
            };
            if !decode.contains(
                &topology.faces,
                &slot,
                "find F3D legacy Extrude topology face",
            )? {
                return Ok(false);
            }
            if changed_only
                && !decode.contains_hash_set(
                    changed_faces,
                    &slot,
                    "find F3D legacy Extrude changed face",
                )?
            {
                return Ok(false);
            }
            Ok(true)
        };
        let Some(face_index) = decode.position_by(
            &candidates,
            &mut matches_historical_face,
            "find F3D legacy Extrude history face",
        )?
        else {
            continue;
        };
        if decode
            .position_by(
                &candidates[face_index + 1..],
                &mut matches_historical_face,
                "find additional F3D legacy Extrude history face",
            )?
            .is_none()
        {
            if let Some(slot) = stable_ref(decode, candidates[face_index].as_str())? {
                return Ok(Some(LegacyFaceResolution::Historical(slot)));
            }
        }
    }
    if let Some(source) = history_source {
        let mut matches_source_face =
            |face: &cadmpeg_ir::ids::FaceId| face_in_brep_source(decode, face.as_str(), source);
        if let Some(index) = decode.position_by(
            &candidates,
            &mut matches_source_face,
            "find F3D legacy Extrude source face",
        )? {
            if decode
                .position_by(
                    &candidates[index + 1..],
                    &mut matches_source_face,
                    "find additional F3D legacy Extrude source face",
                )?
                .is_none()
            {
                return Ok(Some(LegacyFaceResolution::Active(
                    candidates.swap_remove(index),
                )));
            }
        }
    }
    if candidates.len() == 1 {
        Ok(candidates.pop().map(LegacyFaceResolution::Active))
    } else {
        Ok(None)
    }
}

/// One body/native selection row, minting the non-blank native member at the
/// boundary where the F3D record states it.
///
/// `None` when the record states a blank native member: a row the IR carrier
/// does not hold.
fn body_member<B>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    body: B,
    native: String,
) -> Result<Option<cadmpeg_ir::features::BodyMember<B>>, cadmpeg_core::decode::ResourceLimit> {
    Ok(
        cadmpeg_core::text::NonBlankString::for_decode(ctx, native, "validate body native member")?
            .map(|native| cadmpeg_ir::features::BodyMember::new(body, native)),
    )
}

fn historical_brep_source<'text>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    state_id: &'text str,
) -> Result<Option<&'text str>, cadmpeg_core::CodecError> {
    let brep_suffix =
        match decode.rsplit_once(state_id, "/BREP.", "find F3D historical BREP source")? {
            Some((_, suffix)) => suffix,
            None => {
                let Some((_, suffix)) =
                    decode.rsplit_once(state_id, "BREP.", "find F3D historical BREP source")?
                else {
                    return Ok(None);
                };
                suffix
            }
        };
    Ok(decode
        .split_once(brep_suffix, ":asm-", "find F3D BREP assembly delimiter")?
        .map(|(source, _)| source))
}

fn resolve_direct_face_recipe_clauses(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    references: &[crate::records::dimensions::DesignRecipeReference],
    topology: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    let mut clauses = Vec::<(
        u64,
        u64,
        Vec<&crate::records::dimensions::DesignRecipeReference>,
    )>::new();
    for reference in decode.admit_iter(references, "scan F3D direct face references")? {
        let key = (reference.selector_offset, reference.token_offset);
        if let Some((_, _, references)) = clauses
            .iter_mut()
            .find(|(selector, token, _)| (*selector, *token) == key)
        {
            decode.reserve_vec(references, 1, "group F3D direct face references")?;
            references.push(reference);
        } else {
            decode.reserve_vec(&mut clauses, 1, "collect F3D direct face clauses")?;

            let mut grouped = Vec::new();
            decode.reserve_vec(&mut grouped, 1, "group F3D direct face references")?;
            grouped.push(reference);
            clauses.push((key.0, key.1, grouped));
        }
    }
    let mut topology_faces = HashSet::new();
    for face in decode.admit_iter(&topology.faces, "scan F3D topology faces")? {
        decode.insert_hash_set(
            &mut topology_faces,
            *face,
            "index F3D direct topology faces",
        )?;
    }
    let mut clause_storage =
        decode.reserve_scoped(0, "collect F3D direct face clause candidates")?;
    let mut resolved = Vec::new();
    for (_, _, references) in decode.admit_iter(&clauses, "scan F3D direct face clauses")? {
        let mut intersection = None::<BTreeSet<i64>>;
        for reference in decode.admit_iter(references, "scan F3D direct face clause references")? {
            let candidates = if reference.candidate_faces.is_empty() {
                &reference.alternate_selector_faces
            } else {
                &reference.candidate_faces
            };
            let mut eligible = BTreeSet::new();
            for face in decode.admit_iter(candidates, "scan F3D direct face clause candidates")? {
                let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                    continue;
                };
                if topology_faces.contains(&face_slot) && changed_faces.contains(&face_slot) {
                    decode.insert_scoped_btree_set(
                        &mut clause_storage,
                        &mut eligible,
                        face_slot,
                        "collect F3D direct face clause candidates",
                        "collect F3D direct face clause candidates",
                    )?;
                }
            }
            let candidates = eligible;
            if candidates.is_empty() {
                return Ok(Vec::new());
            }
            intersection = Some(match intersection {
                None => candidates,
                Some(mut intersection) => {
                    decode.retain_btree_set(
                        &mut intersection,
                        |face| {
                            decode.contains_btree_set(
                                &candidates,
                                face,
                                "find F3D direct face clause candidate",
                            )
                        },
                        "intersect F3D direct face clauses",
                    )?;
                    intersection
                }
            });
        }
        let Some(face) =
            single_btree_value(decode, intersection, "take F3D direct face clause face")?
        else {
            return Ok(Vec::new());
        };
        if !resolved.contains(&face) {
            decode.reserve_vec(&mut resolved, 1, "collect F3D resolved direct faces")?;
            resolved.push(face);
        }
    }
    Ok(resolved)
}

fn bind_profile_face_group_cardinality(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::face::DesignFaceOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    operand_groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    for scope in decode.admit_iter(scopes, "scan F3D profile face scopes")? {
        let scoped_history = if decode.contains_key_hash_map(
            scope_histories,
            &scope.id,
            "check F3D edge scope history binding",
        )? {
            let Some(history) = bound_scope_history(decode, &scope.id, scope_histories, histories)?
            else {
                continue;
            };
            Some(history)
        } else {
            None
        };
        let scoped_histories = scoped_history.map_or(histories, std::slice::from_ref);
        let Some(profile_groups) = crate::design::face_resolve::extrude_profile_group_roots(
            decode,
            scope,
            operand_groups,
        )?
        else {
            continue;
        };
        for group in profile_groups {
            let Some(indices) = crate::design::face_resolve::extrude_profile_group_operand_indices(
                decode,
                group,
                operand_groups,
                operands,
            )?
            else {
                continue;
            };
            if group.members().len() != indices.len()
                || indices.iter().any(|index| {
                    let operand = &operands[*index];
                    !operand.resolved_face_slots.is_empty()
                        || operand.resolved_active_face.is_some()
                })
            {
                continue;
            }
            let (Some(state_id), Some(previous_state_id)) = (
                scope.history_state_id(),
                effective_scope_previous_history_state_id(decode, scope, scoped_histories)?,
            ) else {
                continue;
            };
            let history_pair = if scoped_history.is_some() {
                bound_history_state_pair(
                    decode,
                    &scope.id,
                    state_id,
                    previous_state_id,
                    scope_histories,
                    histories,
                )?
            } else {
                unique_history_state_pair(decode, histories, state_id, previous_state_id)?
            };
            let Some((history, state, previous)) = history_pair else {
                continue;
            };
            let states = history_state_index(decode, history)?;
            let changed_faces =
                face_changes_across_state_chain(decode, state, previous_state_id, &states)?;
            let (Some(topology), Some(changed_faces)) = (previous.topology(), changed_faces) else {
                continue;
            };
            let paired_aggregate =
                crate::design::face_resolve::is_paired_extrude_profile_aggregate(
                    decode,
                    group,
                    operand_groups,
                    operands,
                )?;
            let mut carrier_faces_storage =
                decode.reserve_scoped(0, "index F3D profile face carriers")?;
            let faces =
                if paired_aggregate {
                    if let Some(transition) = state.transition.as_ref().filter(|transition| {
                        transition.previous_state_id == Some(previous_state_id)
                    }) {
                        let mut preceding_faces = HashSet::new();
                        for face in decode.admit_iter(&topology.faces, "scan F3D topology faces")? {
                            decode.insert_hash_set(
                                &mut preceding_faces,
                                *face,
                                "index F3D profile preceding faces",
                            )?;
                        }
                        let mut deleted = decode.collect_vec(
                            transition.topology.faces.deleted.iter().copied(),
                            "copy F3D profile deleted faces",
                        )?;
                        decode.sort_unstable_by(
                            &mut deleted,
                            |value| value,
                            Ord::cmp,
                            "sort F3D profile deleted faces",
                        )?;
                        deleted.dedup();
                        (deleted.len() == group.members().len()
                            && deleted.len() == transition.topology.faces.deleted.len()
                            && deleted.iter().all(|face| {
                                preceding_faces.contains(face)
                                    && topology
                                        .face_surfaces
                                        .iter()
                                        .filter(|binding| binding.entity == *face)
                                        .count()
                                        == 1
                            }))
                        .then_some(deleted)
                    } else {
                        None
                    }
                } else {
                    carrier_faces_storage.with_storage(|| {
                        profile_face_group_cardinality_candidates(
                            decode,
                            topology,
                            &changed_faces,
                            group.members().len(),
                        )
                    })?
                };
            let Some(faces) = faces else {
                continue;
            };
            for (index, face) in indices.into_iter().zip(faces) {
                let face_id = historical_face_id(decode, face)?;
                let preceding_id =
                    face_id.try_clone_for_decode(decode, "copy F3D historical face identity")?;
                operands[index].preceding_candidate_faces = decode.collect_vec(
                    std::iter::once(preceding_id),
                    "bind F3D profile preceding face",
                )?;
                operands[index].changed_candidate_faces = decode
                    .collect_vec(std::iter::once(face_id), "bind F3D profile changed face")?;
                operands[index].resolved_face_slots = vec![face];
            }
        }
    }
    Ok(())
}

fn profile_face_group_cardinality_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    member_count: usize,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    // Visit each distinct preceding face once, in topology order, and keep
    // the changed ones: the same face set as the changed faces present in
    // the preceding topology.
    let mut preceding_faces = HashSet::new();
    let mut faces_by_carrier = BTreeMap::<i64, Vec<i64>>::new();
    for face in decode
        .admit_iter(&topology.faces, "scan F3D topology faces")?
        .copied()
    {
        if !decode.insert_hash_set(
            &mut preceding_faces,
            face,
            "index F3D profile candidate faces",
        )? || !decode.contains_hash_set(
            changed_faces,
            &face,
            "scan F3D changed profile faces",
        )? {
            continue;
        }
        let mut bindings = topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == face);
        let Some(carrier) = bindings.next().map(|binding| binding.carrier) else {
            continue;
        };
        if bindings.next().is_none() {
            decode.push_btree_group(
                &mut faces_by_carrier,
                carrier,
                face,
                "index F3D profile face carriers",
                "collect F3D profile carrier faces",
            )?;
        }
    }
    let mut candidates = decode
        .admit_iter(faces_by_carrier, "scan F3D profile carrier groups")?
        .map(|(_, faces)| faces)
        .filter(|faces| faces.len() == member_count);
    let Some(mut faces) = candidates.next() else {
        return Ok(None);
    };
    if candidates.next().is_some() {
        return Ok(None);
    }
    // The faces are distinct, so the sorted group keeps `member_count` faces.
    decode.sort_unstable_by(
        &mut faces,
        |value| value,
        Ord::cmp,
        "sort F3D profile carrier faces",
    )?;
    Ok(Some(faces))
}

fn face_changes_across_state_chain<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Result<Option<HashSet<i64>>, cadmpeg_core::CodecError> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut changed = HashSet::new();
    while current.state_id != previous_state_id {
        if !decode.insert_hash_set(
            &mut visited,
            current.state_id,
            "track F3D face change state chain",
        )? {
            return Ok(None);
        }
        let Some(transition) = current.transition.as_ref() else {
            return Ok(None);
        };
        for face in transition
            .topology
            .faces
            .deleted
            .iter()
            .chain(&transition.topology.faces.updated)
        {
            decode.insert_hash_set(&mut changed, *face, "collect F3D changed faces")?;
        }
        let Some(previous) = transition
            .previous_state_id
            .and_then(|id| states.get(&id).copied().flatten())
        else {
            return Ok(None);
        };
        current = previous;
    }
    Ok(Some(changed))
}

#[derive(Debug, PartialEq)]
struct EdgeChanges {
    deleted: HashSet<i64>,
    updated: HashSet<i64>,
}

fn edge_changes_across_state_chain<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Result<Option<EdgeChanges>, cadmpeg_core::CodecError> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut deleted = HashSet::new();
    let mut updated = HashSet::new();
    while current.state_id != previous_state_id {
        if !decode.insert_hash_set(
            &mut visited,
            current.state_id,
            "track F3D edge change state chain",
        )? {
            return Ok(None);
        }
        let Some(transition) = current.transition.as_ref() else {
            return Ok(None);
        };
        for edge in decode.admit_iter(
            &transition.topology.edges.deleted,
            "scan F3D transition topology edges deleted",
        )? {
            decode.insert_hash_set(&mut deleted, *edge, "collect F3D deleted edges")?;
        }
        for edge in decode.admit_iter(
            &transition.topology.edges.updated,
            "scan F3D transition topology edges updated",
        )? {
            decode.insert_hash_set(&mut updated, *edge, "collect F3D updated edges")?;
        }
        let Some(previous) = transition
            .previous_state_id
            .and_then(|id| states.get(&id).copied().flatten())
        else {
            return Ok(None);
        };
        current = previous;
    }
    Ok(Some(EdgeChanges { deleted, updated }))
}

fn historical_face_support_contexts(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[cadmpeg_ir::ids::FaceId],
    history: &AsmHistory,
    preceding_topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Result<
    Vec<crate::records::topology::historical_context::DesignHistoricalFaceSupportContext>,
    cadmpeg_core::CodecError,
> {
    let mut preceding_faces = HashSet::new();
    for face in decode.admit_iter(
        &preceding_topology.faces,
        "scan F3D preceding topology faces",
    )? {
        decode.insert_hash_set(
            &mut preceding_faces,
            *face,
            "index F3D historical support faces",
        )?;
    }
    let mut contexts = Vec::new();
    'candidates: for candidate in
        decode.admit_iter(candidates, "scan F3D face support candidates")?
    {
        let Some(active_face_slot) = stable_ref(decode, candidate.as_str())? else {
            continue;
        };
        let mut preceding_bindings = preceding_topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == active_face_slot);
        let surface_slot = if let Some(binding) = preceding_bindings.next() {
            if preceding_bindings.next().is_some() {
                continue;
            }
            binding.carrier
        } else {
            // The face must bind one carrier, the same in every state.
            let mut carrier = None;
            let mut carrier_is_ambiguous = false;
            for topology in history.states.iter().filter_map(|state| state.topology()) {
                let mut bindings = topology
                    .face_surfaces
                    .iter()
                    .filter(|binding| binding.entity == active_face_slot);
                if let Some(binding) = bindings.next() {
                    if bindings.next().is_some() {
                        continue 'candidates;
                    }
                    match carrier {
                        None => carrier = Some(binding.carrier),
                        Some(known) if known != binding.carrier => carrier_is_ambiguous = true,
                        Some(_) => {}
                    }
                }
            }
            let Some(carrier) = carrier.filter(|_| !carrier_is_ambiguous) else {
                continue;
            };
            carrier
        };
        let mut preceding_face_slots = decode.collect_vec(
            preceding_topology
                .face_surfaces
                .iter()
                .filter(|binding| {
                    binding.carrier == surface_slot && preceding_faces.contains(&binding.entity)
                })
                .map(|binding| binding.entity),
            "collect F3D historical support face slots",
        )?;
        decode.sort_unstable_by(
            &mut preceding_face_slots,
            |value| value,
            Ord::cmp,
            "sort F3D historical support face slots",
        )?;
        preceding_face_slots.dedup();
        if preceding_face_slots.is_empty() {
            continue;
        }
        let changed_preceding_face_slots = decode.collect_vec(
            preceding_face_slots
                .iter()
                .copied()
                .filter(|face| changed_faces.contains(face)),
            "collect F3D changed support face slots",
        )?;
        let preceding_face_boundaries =
            face_boundary_contexts_for_slots(decode, &preceding_face_slots, preceding_topology)?;

        decode.reserve_vec(&mut contexts, 1, "collect F3D historical support contexts")?;
        contexts.push(
            crate::records::topology::historical_context::DesignHistoricalFaceSupportContext {
                active_face_slot,
                surface_slot,
                preceding_face_slots,
                preceding_face_boundaries,
                changed_preceding_face_slots,
            },
        );
    }
    Ok(contexts)
}

fn face_boundary_edges(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: &[cadmpeg_ir::ids::FaceId],
    topology: &AsmHistoricalTopology,
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    let mut face_slots = HashSet::new();
    for face in decode.admit_iter(faces, "scan F3D boundary faces")? {
        if let Some(face_slot) = stable_ref(decode, face.as_str())? {
            decode.insert_hash_set(&mut face_slots, face_slot, "index F3D boundary faces")?;
        }
    }
    let mut loops = HashSet::new();
    for relation in topology
        .face_loops
        .iter()
        .filter(|relation| face_slots.contains(&relation.owner_ref))
    {
        for loop_slot in
            decode.admit_iter(&relation.member_refs, "scan F3D relation member refs")?
        {
            decode.insert_hash_set(&mut loops, *loop_slot, "index F3D boundary loops")?;
        }
    }
    let mut coedges = HashSet::new();
    for relation in topology
        .loop_coedges
        .iter()
        .filter(|relation| loops.contains(&relation.owner_ref))
    {
        for coedge in decode.admit_iter(&relation.member_refs, "scan F3D relation member refs")? {
            decode.insert_hash_set(&mut coedges, *coedge, "index F3D boundary coedges")?;
        }
    }
    let mut edges = decode.collect_vec(
        topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedges.contains(&coedge.coedge))
            .map(|coedge| coedge.edge),
        "collect F3D boundary edges",
    )?;
    decode.sort_unstable_by(
        &mut edges,
        |value| value,
        Ord::cmp,
        "sort F3D boundary edges",
    )?;
    edges.dedup();
    Ok(edges)
}

fn collect_reference_edge_sets(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    reference_faces: &[Vec<cadmpeg_ir::ids::FaceId>],
    topology: &AsmHistoricalTopology,
) -> Result<Vec<Vec<i64>>, cadmpeg_core::CodecError> {
    let mut sets = Vec::new();
    for faces in decode.admit_iter(reference_faces, "scan F3D recipe reference faces")? {
        let faces = selection::faces_in_topology(decode, faces, topology)?;
        let edges = face_boundary_edges(decode, &faces, topology)?;

        decode.reserve_vec(&mut sets, 1, "collect F3D reference edge sets")?;
        sets.push(edges);
    }
    Ok(sets)
}

fn face_boundary_contexts(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: &[cadmpeg_ir::ids::FaceId],
    topology: &AsmHistoricalTopology,
) -> Result<
    Vec<crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext>,
    cadmpeg_core::CodecError,
> {
    let mut face_slots = Vec::new();
    for face in decode.admit_iter(faces, "scan F3D boundary face slots")? {
        if let Some(face_slot) = stable_ref(decode, face.as_str())? {
            decode.push_vec(
                &mut face_slots,
                face_slot,
                "collect F3D boundary face slots",
            )?;
        }
    }
    face_boundary_contexts_for_slots(decode, &face_slots, topology)
}

fn face_boundary_contexts_for_slots(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    face_slots: &[i64],
    topology: &AsmHistoricalTopology,
) -> Result<
    Vec<crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext>,
    cadmpeg_core::CodecError,
> {
    let mut contexts = Vec::new();
    'faces: for face_slot in decode.admit_iter(face_slots, "scan F3D boundary face slots")? {
        let mut face_relations = topology
            .face_loops
            .iter()
            .filter(|relation| relation.owner_ref == *face_slot);
        let Some(face_relation) = face_relations.next() else {
            continue;
        };
        if face_relations.next().is_some() {
            continue;
        }
        let mut loops = Vec::new();
        for loop_slot in decode.admit_iter(
            &face_relation.member_refs,
            "scan F3D face relation member refs",
        )? {
            let mut loop_relations = topology
                .loop_coedges
                .iter()
                .filter(|relation| relation.owner_ref == *loop_slot);
            let Some(loop_relation) = loop_relations.next() else {
                continue 'faces;
            };
            if loop_relations.next().is_some() {
                continue 'faces;
            }
            let mut coedges = Vec::new();
            for coedge_slot in decode.admit_iter(
                &loop_relation.member_refs,
                "scan F3D loop relation member refs",
            )? {
                let mut matches = topology
                    .coedge_topology
                    .iter()
                    .filter(|coedge| coedge.coedge == *coedge_slot);
                let Some(edge_slot) = matches.next().map(|coedge| coedge.edge) else {
                    continue 'faces;
                };
                if matches.next().is_some() {
                    continue 'faces;
                }

                decode.reserve_vec(&mut coedges, 1, "collect F3D face loop coedges")?;
                coedges.push(
                    crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                        coedge_slot: *coedge_slot,
                        edge_slot,
                    },
                );
            }
            let boundary = historical_loop_boundary(decode, coedges, topology)?;

            decode.reserve_vec(&mut loops, 1, "collect F3D face boundary loops")?;
            loops.push(
                crate::records::topology::historical_context::DesignHistoricalFaceLoopContext {
                    loop_slot: *loop_slot,
                    boundary,
                },
            );
        }

        decode.reserve_vec(&mut contexts, 1, "collect F3D face boundary contexts")?;
        contexts.push(
            crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext {
                face_slot: *face_slot,
                loops,
            },
        );
    }
    Ok(contexts)
}

fn historical_loop_boundary(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    coedges: Vec<crate::records::topology::historical_context::DesignHistoricalLoopCoedge>,
    topology: &AsmHistoricalTopology,
) -> Result<
    crate::records::topology::historical_context::DesignHistoricalLoopBoundary,
    cadmpeg_core::CodecError,
> {
    use crate::records::topology::{
        historical_context::DesignHistoricalLoopBoundary,
        historical_context::DesignHistoricalLoopPoint,
        historical_context::DesignHistoricalLoopPosition,
        historical_context::DesignHistoricalLoopVertex,
    };
    let mut vertices = Vec::new();
    for (ordinal, coedge) in coedges.iter().enumerate() {
        let previous = coedges[(ordinal + coedges.len() - 1) % coedges.len()].edge_slot;
        let endpoints = |slot| {
            let mut edges = topology
                .edge_vertices
                .iter()
                .filter(|candidate| candidate.edge == slot);
            let edge = edges.next()?;
            edges
                .next()
                .is_none()
                .then_some([edge.start_vertex, edge.end_vertex])
        };
        let (Some(previous), Some(current)) = (endpoints(previous), endpoints(coedge.edge_slot))
        else {
            return Ok(DesignHistoricalLoopBoundary::Coedges(coedges));
        };
        let mut shared = previous
            .into_iter()
            .filter(|vertex| current.contains(vertex));
        let Some(vertex_slot) = shared.next() else {
            return Ok(DesignHistoricalLoopBoundary::Coedges(coedges));
        };
        if shared.any(|vertex| vertex != vertex_slot) {
            return Ok(DesignHistoricalLoopBoundary::Coedges(coedges));
        }

        decode.reserve_vec(&mut vertices, 1, "collect F3D loop vertices")?;
        vertices.push(DesignHistoricalLoopVertex {
            coedge: coedge.clone(),
            vertex_slot,
        });
    }
    if vertices.is_empty() {
        return Ok(DesignHistoricalLoopBoundary::Coedges(coedges));
    }
    let mut points = Vec::new();
    for vertex in decode.admit_iter(&vertices, "scan F3D vertices")? {
        let mut bindings = topology
            .vertex_points
            .iter()
            .filter(|binding| binding.entity == vertex.vertex_slot);
        let Some(point_slot) = bindings.next().map(|binding| binding.carrier) else {
            return Ok(DesignHistoricalLoopBoundary::Vertices(vertices));
        };
        if bindings.next().is_some() {
            return Ok(DesignHistoricalLoopBoundary::Vertices(vertices));
        }

        decode.reserve_vec(&mut points, 1, "collect F3D loop points")?;
        points.push(DesignHistoricalLoopPoint {
            vertex: vertex.clone(),
            point_slot,
        });
    }
    let mut positions = Vec::new();
    for point in decode.admit_iter(&points, "scan F3D points")? {
        let mut values = topology
            .point_positions
            .iter()
            .filter(|value| value.point == point.point_slot);
        let Some(position) = values.next().map(|value| value.position) else {
            return Ok(DesignHistoricalLoopBoundary::Points(points));
        };
        if values.next().is_some() {
            return Ok(DesignHistoricalLoopBoundary::Points(points));
        }

        decode.reserve_vec(&mut positions, 1, "collect F3D loop positions")?;
        positions.push(DesignHistoricalLoopPosition {
            point: point.clone(),
            position,
        });
    }
    Ok(DesignHistoricalLoopBoundary::Positions(positions))
}

fn preceding_support_face_slots(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    result_faces: &[cadmpeg_ir::ids::FaceId],
    result_topology: &AsmHistoricalTopology,
    preceding_topology: &AsmHistoricalTopology,
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    let mut preceding_faces = HashSet::new();
    for face in decode.admit_iter(
        &preceding_topology.faces,
        "scan F3D preceding topology faces",
    )? {
        decode.insert_hash_set(
            &mut preceding_faces,
            *face,
            "index F3D preceding support faces",
        )?;
    }
    let mut support_faces = Vec::new();
    for result_face in decode.admit_iter(result_faces, "scan F3D result faces")? {
        let Some(result_face) = stable_ref(decode, result_face.as_str())? else {
            continue;
        };
        let mut result_bindings = result_topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == result_face);
        let Some(carrier) = result_bindings.next().map(|binding| binding.carrier) else {
            continue;
        };
        if result_bindings.next().is_some() {
            continue;
        }
        let mut preceding_bindings = preceding_topology.face_surfaces.iter().filter(|binding| {
            binding.carrier == carrier && preceding_faces.contains(&binding.entity)
        });
        let Some(preceding_face) = preceding_bindings.next().map(|binding| binding.entity) else {
            continue;
        };
        if preceding_bindings.next().is_none() && !support_faces.contains(&preceding_face) {
            decode.reserve_vec(&mut support_faces, 1, "collect F3D preceding support faces")?;
            support_faces.push(preceding_face);
        }
    }
    Ok(support_faces)
}

#[derive(Clone, Copy)]
struct EdgeBoundaryContext<'a> {
    topology: &'a AsmHistoricalTopology,
    boundary_edges: &'a [i64],
}

fn edge_recipe_reference_context(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    reference_ordinal: u32,
    reference: &crate::records::dimensions::DesignRecipeReference,
    result: EdgeBoundaryContext<'_>,
    preceding: EdgeBoundaryContext<'_>,
    changed_edges: &HashSet<i64>,
) -> Result<
    crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext,
    cadmpeg_core::CodecError,
> {
    let candidate_faces = if reference.candidate_faces.is_empty() {
        reference.alternate_selector_faces.as_slice()
    } else {
        reference.candidate_faces.as_slice()
    };
    let result_faces = selection::faces_in_topology(decode, candidate_faces, result.topology)?;
    let result_face_boundaries = face_boundary_contexts(decode, &result_faces, result.topology)?;
    let result_edges = face_boundary_edges(decode, &result_faces, result.topology)?;
    let result_shared_edge_slots = decode.try_collect_vec(
        decode
            .admit_iter(result.boundary_edges, "scan F3D result boundary edges")?
            .filter_map(|edge| {
                match decode.contains(
                    result_edges.as_slice(),
                    edge,
                    "find F3D result boundary edge",
                ) {
                    Ok(true) => Some(Ok(*edge)),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                }
            }),
        "collect F3D result shared edges",
    )?;
    let preceding_faces =
        selection::faces_in_topology(decode, candidate_faces, preceding.topology)?;
    let preceding_face_boundaries =
        face_boundary_contexts(decode, &preceding_faces, preceding.topology)?;
    let preceding_support_face_slots =
        preceding_support_face_slots(decode, &result_faces, result.topology, preceding.topology)?;
    let preceding_support_face_boundaries = face_boundary_contexts_for_slots(
        decode,
        &preceding_support_face_slots,
        preceding.topology,
    )?;
    let preceding_edges = face_boundary_edges(decode, &preceding_faces, preceding.topology)?;
    let shared_edge_slots = decode.try_collect_vec(
        decode
            .admit_iter(
                preceding.boundary_edges,
                "scan F3D preceding boundary edges",
            )?
            .filter_map(|edge| {
                match decode.contains(
                    preceding_edges.as_slice(),
                    edge,
                    "find F3D preceding boundary edge",
                ) {
                    Ok(true) => Some(Ok(*edge)),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                }
            }),
        "collect F3D shared edge slots",
    )?;
    let changed_shared_edge_slots = decode.try_collect_vec(
        decode
            .admit_iter(&shared_edge_slots, "scan F3D shared edge slots")?
            .filter_map(|edge| {
                match decode.contains_hash_set(changed_edges, edge, "find F3D changed shared edge")
                {
                    Ok(true) => Some(Ok(*edge)),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                }
            }),
        "collect F3D changed shared edges",
    )?;
    let mut support_edges_storage = decode.reserve_scoped(0, "index F3D support edges")?;
    let mut support_edges = BTreeSet::new();
    for face in decode.admit_iter(
        &preceding_support_face_boundaries,
        "scan F3D support face boundaries",
    )? {
        for face_loop in decode.admit_iter(&face.loops, "scan F3D support face loops")? {
            match &face_loop.boundary {
                crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Coedges(rows) => {
                    for row in decode.admit_iter(rows, "scan F3D support loop members")? {
                        decode.insert_scoped_btree_set(&mut support_edges_storage, &mut support_edges, row.edge_slot, "index F3D support edges", "index F3D support edges")?;
                    }
                }
                crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Vertices(rows) => {
                    for row in decode.admit_iter(rows, "scan F3D support loop members")? {
                        decode.insert_scoped_btree_set(&mut support_edges_storage, &mut support_edges, row.coedge.edge_slot, "index F3D support edges", "index F3D support edges")?;
                    }
                }
                crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Points(rows) => {
                    for row in decode.admit_iter(rows, "scan F3D support loop members")? {
                        decode.insert_scoped_btree_set(&mut support_edges_storage, &mut support_edges, row.vertex.coedge.edge_slot, "index F3D support edges", "index F3D support edges")?;
                    }
                }
                crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Positions(rows) => {
                    for row in decode.admit_iter(rows, "scan F3D support loop members")? {
                        decode.insert_scoped_btree_set(&mut support_edges_storage, &mut support_edges, row.point.vertex.coedge.edge_slot, "index F3D support edges", "index F3D support edges")?;
                    }
                }
            }
        }
    }
    let mut changed_reference_edge_slots = decode.try_collect_vec(
        decode
            .admit_iter(&preceding_edges, "scan F3D preceding reference edges")?
            .chain(decode.admit_iter(&support_edges, "scan F3D support reference edges")?)
            .filter_map(|edge| {
                match decode.contains_hash_set(
                    changed_edges,
                    edge,
                    "find F3D changed reference edge",
                ) {
                    Ok(true) => Some(Ok(*edge)),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                }
            }),
        "collect F3D changed reference edges",
    )?;
    decode.sort_unstable_by(
        &mut changed_reference_edge_slots,
        |value| value,
        Ord::cmp,
        "sort F3D changed reference edges",
    )?;
    decode.dedup_vec(
        &mut changed_reference_edge_slots,
        "deduplicate F3D changed reference edges",
    )?;
    Ok(
        crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
            reference_ordinal,
            result_faces,
            result_face_boundaries,
            result_shared_edge_slots,
            preceding_faces,
            preceding_face_boundaries,
            preceding_support_face_slots,
            preceding_support_face_boundaries,
            shared_edge_slots,
            changed_shared_edge_slots,
            changed_reference_edge_slots,
        },
    )
}

/// Resolve the unique candidate edge shared by the non-null face references
/// in the first side of a standard edge recipe.
fn side_one_recipe_edge(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    reference_contexts: &[crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext],
    selectors: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    candidate_edges: &[i64],
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(side) = structure.and_then(|structure| structure.sides.first()) else {
        return Ok(None);
    };
    let mut ordinals = Vec::new();
    for value in std::iter::once(side.header_value)
        .chain(
            decode
                .admit_iter(&side.scalars, "scan F3D recipe side scalars")?
                .copied(),
        )
        .filter(|value| *value != 0)
    {
        let Some(ordinal) = usize::try_from(value)
            .ok()
            .and_then(|value| value.checked_sub(1))
        else {
            return Ok(None);
        };

        decode.reserve_vec(&mut ordinals, 1, "collect F3D recipe side ordinals")?;
        ordinals.push(ordinal);
    }
    decode.sort_unstable_by(
        &mut ordinals,
        |value| value,
        Ord::cmp,
        "sort F3D recipe side ordinals",
    )?;
    decode.dedup_vec(&mut ordinals, "deduplicate F3D recipe side ordinals")?;
    let mut edge_sets = Vec::new();
    for &ordinal in decode.admit_iter(&ordinals, "scan F3D recipe side ordinals")? {
        let Some(context) = reference_contexts.get(ordinal) else {
            return Ok(None);
        };
        if Some(context.reference_ordinal) != u32::try_from(ordinal).ok() {
            return Ok(None);
        }

        decode.reserve_vec(&mut edge_sets, 1, "collect F3D recipe side edge sets")?;
        edge_sets.push(context.shared_edge_slots.as_slice());
    }
    if decode
        .admit_iter(&edge_sets, "scan F3D recipe side edge sets")?
        .any(|edges| edges.is_empty())
    {
        return Ok(None);
    }
    let Some(first) = edge_sets.first() else {
        return Ok(None);
    };
    let mut candidates = decode.collect_vec(first.iter().copied(), "copy F3D recipe side edges")?;
    for edges in decode
        .admit_iter(&edge_sets, "scan F3D recipe side edge intersections")?
        .skip(1)
    {
        decode.retain_vec(
            &mut candidates,
            |candidate| decode.contains(*edges, candidate, "find F3D recipe side edge candidate"),
            "retain F3D recipe side edge candidates",
        )?;
    }
    decode.retain_vec(
        &mut candidates,
        |candidate| {
            decode.contains(
                candidate_edges,
                candidate,
                "find F3D terminal edge candidate",
            )
        },
        "retain F3D terminal edge candidates",
    )?;
    decode.sort_unstable_by(
        &mut candidates,
        |value| value,
        Ord::cmp,
        "sort F3D recipe side edges",
    )?;
    decode.dedup_vec(&mut candidates, "deduplicate F3D recipe side edges")?;
    match candidates.as_slice() {
        [edge] => Ok(Some(*edge)),
        _ => Ok(
            crate::design::edge_resolve::resolved_edge_candidate_intersection(selectors, edge_sets),
        ),
    }
}

pub(crate) fn bind_edge_operand_history_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::edge_identity::DesignEdgeOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    if projection_was_finalized(decode, histories)? {
        return Ok(());
    }
    let mut recipe_record_indices_storage = decode.reserve_scoped(0, "index F3D edge recipes")?;
    let mut recipe_record_indices = HashMap::new();
    for recipe in decode.admit_iter(recipes, "scan F3D edge recipes")? {
        if let Some(index) = recipe.record_index {
            recipe_record_indices_storage.with_storage(|| {
                decode
                    .insert_hash_map(
                        &mut recipe_record_indices,
                        recipe.id.as_str(),
                        index.value,
                        "index F3D edge recipes",
                    )
                    .map(|_| ())
            })?;
        }
    }
    let mut terminal_topologies = Vec::new();
    for history in decode.admit_iter(histories, "scan F3D edge operand histories")? {
        let mut preceding = HashSet::new();
        for state in decode.admit_iter(&history.states, "scan F3D history states")? {
            if let Some(previous) = state
                .transition
                .as_ref()
                .and_then(|transition| transition.previous_state_id)
            {
                decode.insert_hash_set(
                    &mut preceding,
                    previous,
                    "index F3D terminal predecessors",
                )?;
            }
        }
        let mut terminals = history
            .states
            .iter()
            .filter(|state| !preceding.contains(&state.state_id));
        if let Some(state) = terminals.next().filter(|_| terminals.next().is_none()) {
            if let Some(topology) = state.topology() {
                decode.reserve_vec(
                    &mut terminal_topologies,
                    1,
                    "collect F3D terminal topologies",
                )?;
                terminal_topologies.push((state.state_id, topology));
            }
        }
    }
    for operand in operands {
        decode.clear_vec(
            &mut operand.result_candidate_faces,
            "clear F3D edge result face candidates",
        )?;
        operand.result_boundary_edge_slots.clear();
        decode.clear_vec(
            &mut operand.preceding_candidate_faces,
            "clear F3D preceding edge face candidates",
        )?;
        decode.clear_vec(
            &mut operand.terminal_candidate_faces,
            "clear F3D terminal edge face candidates",
        )?;
        decode.clear_vec(
            &mut operand.changed_candidate_faces,
            "clear F3D changed edge face candidates",
        )?;
        operand.preceding_boundary_edge_slots.clear();
        operand.terminal_boundary_edge_slots.clear();
        operand.changed_boundary_edge_slots.clear();
        operand.deleted_boundary_edge_slots.clear();
        operand.updated_boundary_edge_slots.clear();
        decode.clear_vec(
            &mut operand.treatment_radius_candidates,
            "clear F3D edge treatment radius candidates",
        )?;
        decode.clear_vec(
            &mut operand.changed_boundary_edge_contexts,
            "clear F3D changed edge boundary contexts",
        )?;
        decode.clear_vec(
            &mut operand.terminal_boundary_edge_contexts,
            "clear F3D terminal edge boundary contexts",
        )?;
        decode.clear_vec(
            &mut operand.recipe_reference_contexts,
            "clear F3D edge recipe reference contexts",
        )?;
        decode.clear_vec(
            &mut operand.recipe_selectors,
            "clear F3D edge recipe selectors",
        )?;
        operand.recipe_state_id = None;
        operand.resolved_edge_slot = None;
        operand.resolved_axis = None;
        let stream = crate::ids::native_stream(&operand.id);
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == operand.scope_record_index
                && crate::ids::native_stream(&scope.id) == stream
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let Some(state_id) = scope.history_state_id() else {
            bind_active_edge_operand_for_scope(decode, operand, scope, &terminal_topologies)?;
            continue;
        };
        let Some(previous_state_id) =
            effective_scope_previous_history_state_id(decode, scope, histories)?
        else {
            bind_active_edge_operand_for_scope(decode, operand, scope, &terminal_topologies)?;
            continue;
        };
        let Some((history, state, previous)) = bound_history_state_pair(
            decode,
            &scope.id,
            state_id,
            previous_state_id,
            scope_histories,
            histories,
        )?
        else {
            continue;
        };
        let (Some(result_topology), Some(topology)) = (state.topology(), previous.topology())
        else {
            continue;
        };
        for reference in &mut operand.recipe_references {
            bind_historical_recipe_reference_candidates(decode, reference, topology)?;
        }
        if let Some(recipe_record_index) = decode.get_hash_map(
            &recipe_record_indices,
            operand.recipe_id.as_str(),
            "find F3D edge recipe record",
        )? {
            operand.candidate_faces =
                historical_recipe_faces(decode, i64::from(*recipe_record_index), topology)?;
        }
        let states = history_state_index(decode, history)?;
        let Some(changed_faces) =
            face_changes_across_state_chain(decode, state, previous_state_id, &states)?
        else {
            continue;
        };
        let Some(EdgeChanges {
            deleted: chain_deleted_edges,
            updated: chain_updated_edges,
        }) = edge_changes_across_state_chain(decode, state, previous_state_id, &states)?
        else {
            continue;
        };
        let mut preceding_faces = HashSet::new();
        for face in decode.admit_iter(&topology.faces, "scan F3D topology faces")? {
            decode.insert_hash_set(
                &mut preceding_faces,
                *face,
                "index F3D preceding edge faces",
            )?;
        }
        let inserted_faces = decode.collect_vec(
            result_topology
                .faces
                .iter()
                .copied()
                .filter(|face| !preceding_faces.contains(face)),
            "collect F3D inserted faces",
        )?;
        let mut result_edges = HashSet::new();
        for edge in decode.admit_iter(&result_topology.edges, "scan F3D result topology edges")? {
            decode.insert_hash_set(&mut result_edges, *edge, "index F3D result edges")?;
        }
        let deleted_edges =
            decode.collect_vec(
                topology.edges.iter().copied().filter(|edge| {
                    !result_edges.contains(edge) && chain_deleted_edges.contains(edge)
                }),
                "collect F3D deleted edge candidates",
            )?;
        let updated_edges = decode.collect_vec(
            topology.edges.iter().copied().filter(|edge| {
                result_edges.contains(edge)
                    && (chain_deleted_edges.contains(edge) || chain_updated_edges.contains(edge))
            }),
            "collect F3D updated edge candidates",
        )?;
        operand.recipe_state_id = Some(previous_state_id);
        operand.result_candidate_faces =
            selection::faces_in_topology(decode, &operand.candidate_faces, result_topology)?;
        operand.result_boundary_edge_slots =
            face_boundary_edges(decode, &operand.result_candidate_faces, result_topology)?;
        operand.preceding_candidate_faces =
            selection::faces_in_topology(decode, &operand.candidate_faces, topology)?;
        let mut changed_candidate_faces = Vec::new();
        for face in decode.admit_iter(
            operand.preceding_candidate_faces.as_slice(),
            "scan F3D changed edge faces",
        )? {
            if stable_ref(decode, face.as_str())?.is_some_and(|slot| changed_faces.contains(&slot))
            {
                let face =
                    face.try_clone_for_decode(decode, "copy F3D historical face identity")?;
                decode.push_vec(
                    &mut changed_candidate_faces,
                    face,
                    "collect F3D changed edge faces",
                )?;
            }
        }
        operand.changed_candidate_faces = changed_candidate_faces;
        operand.preceding_boundary_edge_slots =
            face_boundary_edges(decode, &operand.preceding_candidate_faces, topology)?;
        let mut changed_edges = HashSet::new();
        for edge in deleted_edges.iter().chain(&updated_edges) {
            decode.insert_hash_set(
                &mut changed_edges,
                *edge,
                "index F3D changed edge candidates",
            )?;
        }
        operand.changed_boundary_edge_slots = decode.collect_vec(
            operand
                .preceding_boundary_edge_slots
                .iter()
                .copied()
                .filter(|edge| changed_edges.contains(edge)),
            "collect F3D changed boundary edges",
        )?;
        operand.deleted_boundary_edge_slots = selection::boundary_edges_in_changes(
            decode,
            &operand.preceding_boundary_edge_slots,
            &deleted_edges,
        )?;
        operand.updated_boundary_edge_slots = selection::boundary_edges_in_changes(
            decode,
            &operand.preceding_boundary_edge_slots,
            &updated_edges,
        )?;
        operand.treatment_radius_candidates = treatment_radius_candidates(
            decode,
            Some(&operand.result_candidate_faces),
            &inserted_faces,
            result_topology,
            topology,
            &deleted_edges,
        )?;
        operand.changed_boundary_edge_contexts = decode.try_collect_vec(
            operand
                .changed_boundary_edge_slots
                .iter()
                .copied()
                .map(|edge| selection::historical_edge_context(decode, edge, topology)),
            "collect F3D changed boundary contexts",
        )?;
        let mut reference_contexts = Vec::new();
        for (ordinal, reference) in operand.recipe_references.iter().enumerate() {
            let Ok(reference_ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let context = edge_recipe_reference_context(
                decode,
                reference_ordinal,
                reference,
                EdgeBoundaryContext {
                    topology: result_topology,
                    boundary_edges: &operand.result_boundary_edge_slots,
                },
                EdgeBoundaryContext {
                    topology,
                    boundary_edges: &operand.preceding_boundary_edge_slots,
                },
                &changed_edges,
            )?;

            decode.reserve_vec(
                &mut reference_contexts,
                1,
                "collect F3D edge recipe contexts",
            )?;
            reference_contexts.push(context);
        }
        operand.recipe_reference_contexts = reference_contexts;
        if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
            && operand.surface_patch_recipe_structure.is_some()
        {
            operand.resolved_edge_slot = surface_patch_edge_operand_slot(
                decode,
                operand.surface_patch_recipe_structure.as_ref(),
                &operand.recipe_references,
                topology,
            )?;
            continue;
        }
        if crate::design::design_feature_family(&scope.kind())
            == Some(crate::design::DesignFeatureFamily::Sweep)
        {
            let reference_faces = terminal_edge_recipe_reference_faces(
                decode,
                &operand.recipe_references,
                operand.local_topology_references.as_deref(),
            )?;
            let reference_edge_sets =
                collect_reference_edge_sets(decode, &reference_faces, topology)?;
            let mut candidate_edges = BTreeSet::new();
            for edge in reference_edge_sets.iter().flatten() {
                decode
                    .insert_btree_set(
                        &mut candidate_edges,
                        *edge,
                        "index F3D sweep candidate edges",
                    )
                    .map(|_| ())?;
            }
            let contexts = decode.try_collect_vec(
                candidate_edges
                    .into_iter()
                    .map(|edge| selection::historical_edge_context(decode, edge, topology)),
                "collect F3D sweep edge contexts",
            )?;
            operand.recipe_selectors = selection::recipe_selector_candidates(
                decode,
                operand.recipe_structure.as_ref(),
                &contexts,
            )?;
            operand.resolved_edge_slot =
                crate::design::edge_resolve::unique_incidence_edge_shared_by_reference_faces(
                    &operand.recipe_selectors,
                    reference_edge_sets.iter().map(Vec::as_slice),
                );
            continue;
        }
        if crate::design::design_feature_family(&scope.kind())
            == Some(crate::design::DesignFeatureFamily::Revolve)
        {
            let reference_faces = terminal_edge_recipe_reference_faces(
                decode,
                &operand.recipe_references,
                operand.local_topology_references.as_deref(),
            )?;
            let reference_edge_sets =
                collect_reference_edge_sets(decode, &reference_faces, topology)?;
            let mut candidate_edges = BTreeSet::new();
            for edge in reference_edge_sets.iter().flatten() {
                decode
                    .insert_btree_set(
                        &mut candidate_edges,
                        *edge,
                        "index F3D revolve candidate edges",
                    )
                    .map(|_| ())?;
            }
            let contexts = decode.try_collect_vec(
                candidate_edges
                    .into_iter()
                    .map(|edge| selection::historical_edge_context(decode, edge, topology)),
                "collect F3D revolve edge contexts",
            )?;
            operand.recipe_selectors = selection::recipe_selector_candidates(
                decode,
                operand.recipe_structure.as_ref(),
                &contexts,
            )?;
            operand.resolved_edge_slot =
                crate::design::edge_resolve::resolved_edge_candidate_intersection(
                    &operand.recipe_selectors,
                    reference_edge_sets.iter().map(Vec::as_slice),
                );
            let resolved_axis = if let Some(edge) = operand.resolved_edge_slot {
                historical_edge_axis(decode, edge, topology)?
                    .and_then(|(origin, direction)| design_axis(origin, direction))
            } else {
                None
            };
            if let Some(axis) = resolved_axis {
                operand.resolved_axis = Some(axis);
            }
            continue;
        }
        let changed_edge_contexts = decode.try_collect_vec(
            topology
                .edges
                .iter()
                .copied()
                .filter(|edge| changed_edges.contains(edge))
                .map(|edge| selection::historical_edge_context(decode, edge, topology)),
            "collect F3D changed edge contexts",
        )?;
        operand.recipe_selectors = selection::recipe_selector_candidates(
            decode,
            operand.recipe_structure.as_ref(),
            &changed_edge_contexts,
        )?;
        operand.resolved_edge_slot = side_one_recipe_edge(
            decode,
            operand.recipe_structure.as_ref(),
            &operand.recipe_reference_contexts,
            &operand.recipe_selectors,
            &operand.preceding_boundary_edge_slots,
        )?;
    }
    Ok(())
}

/// A resolved axis with finite origin and unit direction.
fn design_axis(
    origin: cadmpeg_ir::math::Point3,
    direction: cadmpeg_ir::math::Vector3,
) -> Option<crate::records::feature::patterns::DesignAxis> {
    crate::records::feature::patterns::DesignAxis::from_parts(
        cadmpeg_ir::features::FinitePoint3::new(origin)?,
        cadmpeg_ir::features::FiniteVector3::new(direction)?,
    )
}

fn historical_edge_axis(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    edge: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)>, cadmpeg_core::CodecError>
{
    if let Some(axis) = topology
        .edge_curves
        .iter()
        .find(|binding| binding.entity == edge)
        .and_then(|binding| binding.carrier)
        .and_then(|curve| topology.curve_axes.iter().find(|axis| axis.curve == curve))
    {
        return Ok(Some((axis.origin, axis.direction)));
    }
    let mut support_surfaces = HashSet::new();
    for carrier in selection::historical_edge_context(decode, edge, topology)?
        .incident_loops
        .into_iter()
        .filter_map(|context| {
            let mut bindings = topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.entity == context.face_slot);
            let carrier = bindings.next()?.carrier;
            bindings.next().is_none().then_some(carrier)
        })
    {
        decode.insert_hash_set(
            &mut support_surfaces,
            carrier,
            "index F3D edge axis support surfaces",
        )?;
    }
    let mut axes = topology
        .surface_axes
        .iter()
        .filter(|axis| support_surfaces.contains(&axis.surface))
        .map(|axis| (axis.origin, axis.direction));
    let Some(first) = axes.next() else {
        return Ok(None);
    };
    Ok(axes
        .all(|axis| same_axis_line(first, axis))
        .then_some(first))
}

fn bind_active_edge_operand_for_scope(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &mut crate::records::topology::edge_identity::DesignEdgeOperand,
    scope: &crate::records::feature::scope::DesignParameterScope,
    terminal_topologies: &[(i64, &AsmHistoricalTopology)],
) -> Result<(), cadmpeg_core::CodecError> {
    bind_active_edge_operand_candidates(decode, operand, terminal_topologies)?;
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
        && operand.surface_patch_recipe_structure.is_some()
    {
        operand.recipe_state_id = None;
        operand.resolved_edge_slot = None;
        let mut matched = None;
        let mut ambiguous = false;
        for (state_id, topology) in
            decode.admit_iter(terminal_topologies, "scan F3D terminal topologies")?
        {
            if let Some(edge) = surface_patch_edge_operand_slot(
                decode,
                operand.surface_patch_recipe_structure.as_ref(),
                &operand.recipe_references,
                topology,
            )? {
                if matched.is_some() {
                    ambiguous = true;
                    break;
                }
                matched = Some((*state_id, edge));
            }
        }
        if !ambiguous {
            if let Some((state_id, edge)) = matched {
                operand.recipe_state_id = Some(state_id);
                operand.resolved_edge_slot = Some(edge);
            }
        }
    }
    if crate::design::design_feature_family(&scope.kind())
        == Some(crate::design::DesignFeatureFamily::Revolve)
    {
        let topology = operand.recipe_state_id.and_then(|state_id| {
            terminal_topologies
                .iter()
                .find(|(candidate, _)| *candidate == state_id)
                .map(|(_, topology)| *topology)
        });
        let resolved_axis = if let Some((edge, topology)) = operand.resolved_edge_slot.zip(topology)
        {
            historical_edge_axis(decode, edge, topology)?
                .and_then(|(origin, direction)| design_axis(origin, direction))
        } else {
            None
        };
        if let Some(axis) = resolved_axis {
            operand.resolved_axis = Some(axis);
        }
    }
    Ok(())
}

fn surface_patch_edge_operand_slot(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    structure: Option<&crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure>,
    recipe_references: &[crate::records::dimensions::DesignRecipeReference],
    topology: &AsmHistoricalTopology,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(structure) = structure else {
        return Ok(None);
    };
    let [first, second] = &structure.clauses;
    let Some(common_edge_reference) = common_surface_patch_reference(
        first.edge_reference_ordinals,
        second.edge_reference_ordinals,
    ) else {
        return Ok(None);
    };
    let Some(common_face_reference) = common_surface_patch_reference(
        first.face_reference_ordinals,
        second.face_reference_ordinals,
    ) else {
        return Ok(None);
    };
    let Some(edge_reference) = usize::try_from(common_edge_reference)
        .ok()
        .and_then(|ordinal| recipe_references.get(ordinal))
    else {
        return Ok(None);
    };
    let Some(face_reference) = usize::try_from(common_face_reference)
        .ok()
        .and_then(|ordinal| recipe_references.get(ordinal))
    else {
        return Ok(None);
    };
    if edge_reference.candidate_edges.is_empty() {
        return Ok(None);
    }
    let face_candidates = if face_reference.candidate_faces.is_empty() {
        &face_reference.alternate_selector_faces
    } else {
        &face_reference.candidate_faces
    };
    let faces = selection::faces_in_topology(decode, face_candidates, topology)?;
    let face_boundary_edges = face_boundary_edges(decode, &faces, topology)?;
    let mut candidates = Vec::new();
    for edge in decode.admit_iter(
        &edge_reference.candidate_edges,
        "scan F3D surface patch edge candidates",
    )? {
        if let Some(edge_slot) = stable_ref(decode, edge.as_str())? {
            if face_boundary_edges.contains(&edge_slot) {
                decode.push_vec(
                    &mut candidates,
                    edge_slot,
                    "collect F3D surface patch edge candidates",
                )?;
            }
        }
    }
    decode.sort_unstable_by(
        &mut candidates,
        |value| value,
        Ord::cmp,
        "sort F3D surface patch edge candidates",
    )?;
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Ok(Some(*edge)),
        _ => Ok(None),
    }
}

fn common_surface_patch_reference(left: [u32; 2], right: [u32; 2]) -> Option<u32> {
    let mut common = left
        .into_iter()
        .filter(|reference| right.contains(reference));
    let reference = common.next()?;
    common.next().is_none().then_some(reference)
}

fn bind_active_edge_operand_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &mut crate::records::topology::edge_identity::DesignEdgeOperand,
    topologies: &[(i64, &AsmHistoricalTopology)],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut unique = None;
    for (state_id, topology) in decode.admit_iter(topologies, "scan F3D active edge topologies")? {
        let all_reference_faces =
            terminal_edge_recipe_reference_faces(decode, &operand.recipe_references, None)?;
        let reference_faces = terminal_edge_recipe_reference_faces(
            decode,
            &operand.recipe_references,
            operand.local_topology_references.as_deref(),
        )?;
        let terminal_faces =
            terminal_edge_recipe_faces(decode, &operand.candidate_faces, &reference_faces)?;
        let candidate_faces = selection::faces_in_topology(decode, &terminal_faces, topology)?;
        if topologies.len() != 1 && candidate_faces.is_empty() {
            continue;
        }
        let boundary_edges = face_boundary_edges(decode, &candidate_faces, topology)?;
        let contexts = decode.try_collect_vec(
            boundary_edges
                .iter()
                .copied()
                .map(|edge| selection::historical_edge_context(decode, edge, topology)),
            "collect F3D terminal edge contexts",
        )?;
        let selectors = selection::recipe_selector_candidates(
            decode,
            operand.recipe_structure.as_ref(),
            &contexts,
        )?;
        let reference_edge_sets = collect_reference_edge_sets(decode, &reference_faces, topology)?;
        let all_reference_edge_sets =
            collect_reference_edge_sets(decode, &all_reference_faces, topology)?;
        let edge = crate::design::edge_resolve::resolved_edge_candidate_intersection(
            &selectors,
            reference_edge_sets.iter().map(Vec::as_slice),
        );
        if unique.is_some() {
            return Ok(());
        }
        unique = Some((
            *state_id,
            edge,
            candidate_faces,
            boundary_edges,
            contexts,
            all_reference_edge_sets,
            selectors,
        ));
    }
    let Some((
        state_id,
        edge,
        candidate_faces,
        boundary_edges,
        contexts,
        all_reference_edge_sets,
        selectors,
    )) = unique
    else {
        return Ok(());
    };
    operand.terminal_candidate_faces = candidate_faces;
    operand.terminal_boundary_edge_slots = boundary_edges;
    operand.terminal_boundary_edge_contexts = contexts;
    operand.terminal_reference_edge_slots = all_reference_edge_sets;
    operand.recipe_selectors = selectors;
    operand.recipe_state_id = Some(state_id);
    operand.resolved_edge_slot = edge;
    Ok(())
}

fn terminal_edge_recipe_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    primary: &[cadmpeg_ir::ids::FaceId],
    reference_faces: &[Vec<cadmpeg_ir::ids::FaceId>],
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    let mut faces = decode.try_collect_vec(
        (primary.iter().chain(reference_faces.iter().flatten()))
            .map(|face| face.try_clone_for_decode(decode, "copy F3D historical face identity")),
        "collect F3D terminal edge recipe faces",
    )?;
    decode.stable_sort_by(
        &mut faces,
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D terminal edge recipe faces",
    )?;
    faces.dedup();
    Ok(faces)
}

fn terminal_edge_recipe_reference_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    references: &[crate::records::dimensions::DesignRecipeReference],
    local_topology_references: Option<&[std::num::NonZeroU32]>,
) -> Result<Vec<Vec<cadmpeg_ir::ids::FaceId>>, cadmpeg_core::CodecError> {
    let mut selected = Vec::new();
    let mut append = |reference: &crate::records::dimensions::DesignRecipeReference| {
        let faces = if reference.candidate_faces.is_empty() {
            &reference.alternate_selector_faces
        } else {
            &reference.candidate_faces
        };
        let faces = decode.try_collect_vec(
            (faces)
                .iter()
                .map(|face| face.try_clone_for_decode(decode, "copy F3D historical face identity")),
            "copy F3D terminal reference faces",
        )?;

        decode.reserve_vec(&mut selected, 1, "collect F3D terminal reference groups")?;
        selected.push(faces);
        Ok::<(), cadmpeg_core::CodecError>(())
    };
    if let Some(ordinals) = local_topology_references {
        for ordinal in ordinals {
            if let Some(reference) = usize::try_from(ordinal.get())
                .ok()
                .and_then(|ordinal| ordinal.checked_sub(1))
                .and_then(|ordinal| references.get(ordinal))
            {
                append(reference)?;
            }
        }
    } else {
        for reference in
            decode.admit_iter(references, "scan F3D terminal edge recipe references")?
        {
            append(reference)?;
        }
    }
    Ok(selected)
}

fn treatment_radius_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    result_candidate_faces: Option<&[cadmpeg_ir::ids::FaceId]>,
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    deleted_edges: &[i64],
) -> Result<
    Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate>,
    cadmpeg_core::CodecError,
> {
    Ok(treatment_edge_candidates(
        decode,
        result_candidate_faces,
        inserted_faces,
        result,
        preceding,
        deleted_edges,
    )?
    .0)
}

fn treatment_edge_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    result_candidate_faces: Option<&[cadmpeg_ir::ids::FaceId]>,
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    deleted_edges: &[i64],
) -> Result<
    (
        Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate>,
        Vec<i64>,
    ),
    cadmpeg_core::CodecError,
> {
    let result_boundaries = face_boundary_edge_index(decode, result)?;
    let preceding_boundaries = face_boundary_edge_index(decode, preceding)?;
    let supports = treatment_face_supports(
        decode,
        inserted_faces,
        result,
        preceding,
        &result_boundaries,
    )?;
    let mut deleted_edge_set = HashSet::new();
    for edge in decode.admit_iter(deleted_edges, "scan F3D deleted edges")? {
        decode.insert_hash_set(
            &mut deleted_edge_set,
            *edge,
            "index F3D treatment deleted edges",
        )?;
    }
    let mut candidate_edges_storage =
        decode.reserve_scoped(0, "index F3D treatment candidate edges")?;
    let mut candidate_edges = BTreeSet::new();
    if let Some(result_candidate_faces) = result_candidate_faces {
        for face in
            decode.admit_iter(result_candidate_faces, "scan F3D treatment candidate faces")?
        {
            let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                continue;
            };
            let Some(edges) = decode.get_btree_map(
                &result_boundaries.boundaries,
                &face_slot,
                "find F3D treatment candidate face boundary",
            )?
            else {
                continue;
            };
            for edge in decode.admit_iter(&edges.edges, "scan F3D treatment candidate edges")? {
                decode.insert_scoped_btree_set(
                    &mut candidate_edges_storage,
                    &mut candidate_edges,
                    *edge,
                    "index F3D treatment candidate edges",
                    "index F3D treatment candidate edges",
                )?;
            }
        }
    }
    let mut radii_out = Vec::new();
    let mut transitions_out = Vec::new();
    for (inserted, carrier, supports) in
        decode.admit_iter(supports, "scan F3D treatment face supports")?
    {
        let Some(inserted_boundary) = decode.get_btree_map(
            &result_boundaries.boundaries,
            &inserted,
            "find F3D inserted treatment face boundary",
        )?
        else {
            continue;
        };
        let mut radii = result
            .surface_radii
            .iter()
            .filter(|candidate| candidate.surface == carrier);
        let radius = match radii
            .next()
            .and_then(|candidate| cadmpeg_ir::scalar::PositiveReal::new(candidate.radius))
            .filter(|_| radii.next().is_none())
        {
            Some(radius)
                if candidate_edges.is_empty()
                    || !decode.is_disjoint_btree_set(
                        &inserted_boundary.edges,
                        &candidate_edges,
                        "compare F3D treatment candidate edges",
                    )? =>
            {
                Some(radius)
            }
            _ => None,
        };
        for (ordinal, left) in decode
            .admit_iter(&supports, "scan F3D treatment support pairs")?
            .enumerate()
        {
            let Some(left_edges) = decode.get_btree_map(
                &preceding_boundaries.boundaries,
                left,
                "find F3D treatment support boundary",
            )?
            else {
                continue;
            };
            for right in
                decode.admit_iter(&supports[ordinal + 1..], "scan F3D treatment support pairs")?
            {
                let Some(right_edges) = decode.get_btree_map(
                    &preceding_boundaries.boundaries,
                    right,
                    "find F3D treatment support boundary",
                )?
                else {
                    continue;
                };
                for edge in decode.admit_iter(
                    &left_edges.edges,
                    "scan F3D treatment support boundary edges",
                )? {
                    if !decode.contains_btree_set(
                        &right_edges.edges,
                        edge,
                        "find F3D shared treatment support edge",
                    )? || !decode.contains_hash_set(
                        &deleted_edge_set,
                        edge,
                        "find F3D deleted treatment edge",
                    )? {
                        continue;
                    }
                    decode.reserve_vec(
                        &mut transitions_out,
                        1,
                        "collect F3D treatment transition edges",
                    )?;
                    transitions_out.push(*edge);
                    if let Some(radius) = radius {
                        decode.reserve_vec(&mut radii_out, 1, "collect F3D treatment radii")?;
                        radii_out.push(
                            crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate {
                                edge_slot: *edge,
                                radius,
                            },
                        );
                    }
                }
            }
        }
    }
    decode.stable_sort_by_key(
        &mut radii_out,
        |value| (value.radius.get(), value.edge_slot),
        |left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)),
        "sort F3D treatment edge radii",
    )?;
    decode.dedup_by(
        &mut radii_out,
        |left, right| Ok(left.radius == right.radius && left.edge_slot == right.edge_slot),
        "deduplicate F3D treatment edge radii",
    )?;
    decode.sort_unstable_by(
        &mut transitions_out,
        |value| value,
        Ord::cmp,
        "sort F3D treatment edge transitions",
    )?;
    decode.dedup_vec(
        &mut transitions_out,
        "deduplicate F3D treatment edge transitions",
    )?;
    Ok((radii_out, transitions_out))
}

fn treatment_face_supports(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    result_boundaries: &FaceBoundaryEdgeIndex<'_>,
) -> Result<Vec<(i64, i64, Vec<i64>)>, cadmpeg_core::CodecError> {
    let mut preceding_faces = HashSet::new();
    for face in decode.admit_iter(&preceding.faces, "scan F3D preceding faces")? {
        decode.insert_hash_set(
            &mut preceding_faces,
            *face,
            "index F3D treatment preceding faces",
        )?;
    }
    let mut preceding_surfaces = HashSet::new();
    for surface in decode.admit_iter(&preceding.surfaces, "scan F3D preceding surfaces")? {
        decode.insert_hash_set(
            &mut preceding_surfaces,
            *surface,
            "index F3D treatment preceding surfaces",
        )?;
    }
    let result_carriers = unique_face_carriers(decode, result)?;
    let preceding_carrier_faces = unique_carrier_faces(decode, preceding, &preceding_faces)?;
    let mut adjacent_faces = HashMap::<i64, Vec<i64>>::new();
    for (face, edges) in decode.admit_iter(
        &result_boundaries.boundaries,
        "scan F3D result face boundaries",
    )? {
        for edge in decode.admit_iter(&edges.edges, "scan F3D result face boundary edges")? {
            decode.push_hash_group(
                &mut adjacent_faces,
                *edge,
                *face,
                "index F3D adjacent treatment edges",
                "collect F3D adjacent treatment faces",
            )?;
        }
    }
    let mut selected = Vec::new();
    for inserted in decode
        .admit_iter(inserted_faces, "scan F3D inserted faces")?
        .copied()
    {
        let Some(carrier) = result_carriers.get(&inserted).copied().flatten() else {
            continue;
        };
        if preceding_surfaces.contains(&carrier) {
            continue;
        }
        let Some(inserted_boundary) = decode.get_btree_map(
            &result_boundaries.boundaries,
            &inserted,
            "find F3D inserted support face boundary",
        )?
        else {
            continue;
        };
        let inserted_boundary_edges =
            decode.admit_iter(&inserted_boundary.edges, "scan F3D inserted boundary edges")?;
        let mut supports = Vec::new();
        for edge in inserted_boundary_edges {
            let Some(adjacent) = adjacent_faces.get(edge) else {
                continue;
            };
            for face in decode.admit_iter(adjacent, "scan F3D adjacent treatment faces")? {
                if *face == inserted {
                    continue;
                }
                let Some(carrier) = result_carriers.get(face).copied().flatten() else {
                    continue;
                };
                let Some(support) = preceding_carrier_faces.get(&carrier).copied().flatten() else {
                    continue;
                };
                decode.push_vec(
                    &mut supports,
                    support,
                    "collect F3D treatment support faces",
                )?;
            }
        }
        decode.sort_unstable_by(
            &mut supports,
            |value| value,
            Ord::cmp,
            "sort F3D treatment support faces",
        )?;
        decode.dedup_vec(&mut supports, "deduplicate F3D treatment support faces")?;

        decode.reserve_vec(&mut selected, 1, "collect F3D treatment face supports")?;
        selected.push((inserted, carrier, supports));
    }
    Ok(selected)
}

#[derive(Debug)]
struct FaceBoundaryEdges<'ctx> {
    edges: BTreeSet<i64>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

#[derive(Debug)]
struct FaceBoundaryEdgeIndex<'ctx> {
    boundaries: BTreeMap<i64, FaceBoundaryEdges<'ctx>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn face_boundary_edge_index<'ctx>(
    decode: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<FaceBoundaryEdgeIndex<'ctx>, cadmpeg_core::CodecError> {
    fn unique_relations<'a>(
        decode: &cadmpeg_core::decode::DecodeContext<'_>,
        relations: &'a [AsmHistoricalRelation],
    ) -> Result<HashMap<i64, Option<&'a [i64]>>, cadmpeg_core::CodecError> {
        let mut out = HashMap::<i64, Option<&[i64]>>::new();
        for relation in decode.admit_iter(relations, "scan F3D boundary relations")? {
            if !out.contains_key(&relation.owner_ref) {
                decode.reserve_map(&mut out, 1, "index F3D boundary relations")?;
            }
            out.entry(relation.owner_ref)
                .and_modify(|members| *members = None)
                .or_insert(Some(&relation.member_refs));
        }
        Ok(out)
    }
    let face_loops = unique_relations(decode, &topology.face_loops)?;
    let loop_coedges = unique_relations(decode, &topology.loop_coedges)?;
    let mut coedge_edges = HashMap::<i64, Option<i64>>::new();
    for coedge in decode.admit_iter(
        &topology.coedge_topology,
        "scan F3D topology coedge topology",
    )? {
        if !coedge_edges.contains_key(&coedge.coedge) {
            decode.reserve_map(&mut coedge_edges, 1, "index F3D boundary coedge edges")?;
        }
        coedge_edges
            .entry(coedge.coedge)
            .and_modify(|edge| *edge = None)
            .or_insert(Some(coedge.edge));
    }
    let mut boundaries_storage = decode.reserve_scoped(0, "index F3D face boundaries")?;
    let mut boundaries = BTreeMap::new();
    'faces: for face in decode.admit_iter(&topology.faces, "scan F3D boundary faces")? {
        let Some(loops) = face_loops.get(face).copied().flatten() else {
            continue;
        };
        let mut edges_storage = decode.reserve_scoped(0, "index F3D face boundary edges")?;
        let mut edges = BTreeSet::new();
        for loop_slot in decode.admit_iter(loops, "scan F3D boundary face loops")? {
            let Some(coedges) = loop_coedges.get(loop_slot).copied().flatten() else {
                continue 'faces;
            };
            for coedge in decode.admit_iter(coedges, "scan F3D boundary loop coedges")? {
                let Some(edge) = coedge_edges.get(coedge).copied().flatten() else {
                    continue 'faces;
                };
                edges_storage.with_storage(|| {
                    decode.insert_btree_set(&mut edges, edge, "index F3D face boundary edges")
                })?;
            }
        }
        let entry = FaceBoundaryEdges {
            edges,
            _storage: edges_storage,
        };
        boundaries_storage.with_storage(|| {
            decode
                .insert_btree_map(&mut boundaries, *face, entry, "index F3D face boundaries")
                .map(|_| ())
        })?;
    }
    Ok(FaceBoundaryEdgeIndex {
        boundaries,
        _storage: boundaries_storage,
    })
}

fn unique_face_carriers(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<HashMap<i64, Option<i64>>, cadmpeg_core::CodecError> {
    let mut out = HashMap::new();
    for binding in decode.admit_iter(&topology.face_surfaces, "scan F3D topology face surfaces")? {
        if !out.contains_key(&binding.entity) {
            decode.reserve_map(&mut out, 1, "index F3D face carriers")?;
        }
        out.entry(binding.entity)
            .and_modify(|carrier| *carrier = None)
            .or_insert(Some(binding.carrier));
    }
    Ok(out)
}

fn unique_carrier_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
    included_faces: &HashSet<i64>,
) -> Result<HashMap<i64, Option<i64>>, cadmpeg_core::CodecError> {
    let mut out = HashMap::new();
    for binding in decode.admit_iter(&topology.face_surfaces, "scan F3D carrier face candidates")? {
        if !included_faces.contains(&binding.entity) {
            continue;
        }
        if !out.contains_key(&binding.carrier) {
            decode.reserve_map(&mut out, 1, "index F3D carrier faces")?;
        }
        out.entry(binding.carrier)
            .and_modify(|face| *face = None)
            .or_insert(Some(binding.entity));
    }
    Ok(out)
}

fn stable_ref(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    id: &str,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some((_, tail)) = decode.rsplit_once(id, "#", "split F3D stable entity identity")? else {
        return Ok(None);
    };
    let Some(reference) = tail.split(':').next() else {
        return Ok(None);
    };
    Ok(decode
        .parse_text::<i64>(reference, "parse F3D stable entity reference")?
        .ok())
}

pub(crate) fn same_axis_line(
    left: (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
    right: (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
) -> bool {
    let direction_dot = left.1.dot(right.1);
    if (direction_dot.abs() - 1.0).abs() > EPS_HISTORY_SAME_AXIS_LINE_E9 {
        return false;
    }
    let distance = right.0.vector_from(left.0).cross(left.1).norm();
    distance.is_finite() && distance <= EPS_HISTORY_SAME_AXIS_LINE_E8
}

fn affected_body_refs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    current: &AsmDeltaState,
    previous: Option<&AsmDeltaState>,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    let Some(transition) = current.transition.as_ref() else {
        return Ok(None);
    };
    if transition.previous_state_id != previous.map(|state| state.state_id) {
        return Ok(None);
    }
    let Some(current_topology) = current.topology() else {
        return Ok(None);
    };
    let current_changes = changed_family_refs(ctx, &transition.topology, false)?;
    let Some(mut affected) = bodies_intersecting(ctx, current_topology, &current_changes)? else {
        return Ok(None);
    };
    if let Some(previous) = previous {
        let Some(previous_topology) = previous.topology() else {
            return Ok(None);
        };
        let deleted = changed_family_refs(ctx, &transition.topology, true)?;
        let Some(previous_affected) = bodies_intersecting(ctx, previous_topology, &deleted)? else {
            return Ok(None);
        };
        for body in previous_affected {
            ctx.insert_btree_set(&mut affected, body, "merge F3D affected history bodies")?;
        }
    }
    Ok(Some(
        ctx.collect_vec(
            ctx.admit_iter(&affected, "scan F3D affected history bodies")?
                .copied(),
            "collect F3D affected history bodies",
        )?,
    ))
}

fn changed_family_refs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    delta: &AsmHistoricalTopologyDelta,
    deleted: bool,
) -> Result<BTreeSet<i64>, cadmpeg_core::CodecError> {
    let families = [
        &delta.bodies,
        &delta.regions,
        &delta.shells,
        &delta.faces,
        &delta.loops,
        &delta.coedges,
        &delta.edges,
        &delta.vertices,
        &delta.points,
        &delta.surfaces,
        &delta.curves,
        &delta.pcurves,
    ];
    let mut changed = BTreeSet::new();
    for family in families {
        if deleted {
            for &member in
                ctx.admit_iter(&family.deleted, "scan F3D changed topology family members")?
            {
                ctx.insert_btree_set(&mut changed, member, "index F3D changed topology members")?;
            }
        } else {
            for &member in
                ctx.admit_iter(&family.inserted, "scan F3D changed topology family members")?
            {
                ctx.insert_btree_set(&mut changed, member, "index F3D changed topology members")?;
            }
            for &member in
                ctx.admit_iter(&family.updated, "scan F3D changed topology family members")?
            {
                ctx.insert_btree_set(&mut changed, member, "index F3D changed topology members")?;
            }
        }
    }
    Ok(changed)
}

fn bodies_intersecting(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
    changed: &BTreeSet<i64>,
) -> Result<Option<BTreeSet<i64>>, cadmpeg_core::CodecError> {
    macro_rules! history_some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let body_regions = relation_map(decode, &topology.body_regions)?;
    let region_shells = relation_map(decode, &topology.region_shells)?;
    let shell_faces = relation_map(decode, &topology.shell_faces)?;
    let shell_wire_edges = relation_map(decode, &topology.shell_wire_edges)?;
    let shell_free_vertices = relation_map(decode, &topology.shell_free_vertices)?;
    let face_loops = relation_map(decode, &topology.face_loops)?;
    let loop_coedges = relation_map(decode, &topology.loop_coedges)?;
    let coedges = decode.collect_hash_map(
        topology
            .coedge_topology
            .iter()
            .map(|coedge| (coedge.coedge, coedge)),
        "index F3D historical coedges",
    )?;
    let edges = decode.collect_hash_map(
        topology.edge_vertices.iter().map(|edge| (edge.edge, edge)),
        "index F3D historical edge vertices",
    )?;
    let carrier = |items: &[AsmHistoricalCarrierBinding]| {
        decode.collect_hash_map(
            items
                .iter()
                .map(|binding| (binding.entity, binding.carrier)),
            "index F3D historical carriers",
        )
    };
    let optional_carrier = |items: &[AsmHistoricalOptionalCarrierBinding]| {
        decode.collect_hash_map(
            items
                .iter()
                .map(|binding| (binding.entity, binding.carrier)),
            "index F3D historical optional carriers",
        )
    };
    let face_surfaces = carrier(&topology.face_surfaces)?;
    let edge_curves = optional_carrier(&topology.edge_curves)?;
    let coedge_pcurves = optional_carrier(&topology.coedge_pcurves)?;
    let vertex_points = carrier(&topology.vertex_points)?;
    let mut affected = BTreeSet::new();
    for &body in decode.admit_iter(&topology.bodies, "scan F3D topology bodies")? {
        let mut closure = BTreeSet::new();
        decode
            .insert_btree_set(&mut closure, body, "collect F3D historical body closure")
            .map(|_| ())?;
        for &region in decode.admit_iter(
            *history_some!(body_regions.get(&body)),
            "scan F3D body regions",
        )? {
            decode
                .insert_btree_set(&mut closure, region, "collect F3D historical body closure")
                .map(|_| ())?;
            for &shell in decode.admit_iter(
                *history_some!(region_shells.get(&region)),
                "scan F3D region shells",
            )? {
                decode
                    .insert_btree_set(&mut closure, shell, "collect F3D historical body closure")
                    .map(|_| ())?;
                let mut shell_edges = decode.collect_vec(
                    history_some!(shell_wire_edges.get(&shell)).iter().copied(),
                    "copy F3D historical shell edges",
                )?;
                let mut shell_vertices = decode.collect_vec(
                    history_some!(shell_free_vertices.get(&shell))
                        .iter()
                        .copied(),
                    "copy F3D historical shell vertices",
                )?;
                for &face in decode.admit_iter(
                    *history_some!(shell_faces.get(&shell)),
                    "scan F3D shell faces",
                )? {
                    decode
                        .insert_btree_set(&mut closure, face, "collect F3D historical body closure")
                        .map(|_| ())?;
                    decode
                        .insert_btree_set(
                            &mut closure,
                            *history_some!(face_surfaces.get(&face)),
                            "collect F3D historical body closure",
                        )
                        .map(|_| ())?;
                    for &loop_ in decode
                        .admit_iter(*history_some!(face_loops.get(&face)), "scan F3D face loops")?
                    {
                        decode
                            .insert_btree_set(
                                &mut closure,
                                loop_,
                                "collect F3D historical body closure",
                            )
                            .map(|_| ())?;
                        for &coedge in decode.admit_iter(
                            *history_some!(loop_coedges.get(&loop_)),
                            "scan F3D loop coedges",
                        )? {
                            decode
                                .insert_btree_set(
                                    &mut closure,
                                    coedge,
                                    "collect F3D historical body closure",
                                )
                                .map(|_| ())?;
                            let coedge_topology = history_some!(coedges.get(&coedge));

                            decode.reserve_vec(
                                &mut shell_edges,
                                1,
                                "collect F3D historical shell edges",
                            )?;
                            shell_edges.push(coedge_topology.edge);
                            if let Some(pcurve) = coedge_pcurves.get(&coedge).copied().flatten() {
                                decode
                                    .insert_btree_set(
                                        &mut closure,
                                        pcurve,
                                        "collect F3D historical body closure",
                                    )
                                    .map(|_| ())?;
                            }
                        }
                    }
                }
                for edge in decode.admit_iter(&shell_edges, "scan F3D shell edges")? {
                    let edge = *edge;
                    decode
                        .insert_btree_set(&mut closure, edge, "collect F3D historical body closure")
                        .map(|_| ())?;
                    let edge_topology = history_some!(edges.get(&edge));
                    for vertex in [edge_topology.start_vertex, edge_topology.end_vertex] {
                        decode.reserve_vec(
                            &mut shell_vertices,
                            1,
                            "collect F3D historical shell vertices",
                        )?;
                        shell_vertices.push(vertex);
                    }
                    if let Some(curve) = edge_curves.get(&edge).copied().flatten() {
                        decode
                            .insert_btree_set(
                                &mut closure,
                                curve,
                                "collect F3D historical body closure",
                            )
                            .map(|_| ())?;
                    }
                }
                for vertex in
                    decode.admit_iter(&shell_vertices, "scan F3D historical shell vertices")?
                {
                    let vertex = *vertex;
                    decode
                        .insert_btree_set(
                            &mut closure,
                            vertex,
                            "collect F3D historical body closure",
                        )
                        .map(|_| ())?;
                    decode
                        .insert_btree_set(
                            &mut closure,
                            *history_some!(vertex_points.get(&vertex)),
                            "collect F3D historical body closure",
                        )
                        .map(|_| ())?;
                }
            }
        }
        if !decode.is_disjoint_btree_set(&closure, changed, "check F3D changed body closure")? {
            decode
                .insert_btree_set(&mut affected, body, "collect F3D affected topology bodies")
                .map(|_| ())?;
        }
    }
    Ok(Some(affected))
}

fn relation_map<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    items: &'a [AsmHistoricalRelation],
) -> Result<HashMap<i64, &'a [i64]>, cadmpeg_core::CodecError> {
    decode.collect_hash_map(
        items
            .iter()
            .map(|relation| (relation.owner_ref, relation.member_refs.as_slice())),
        "index F3D historical relations",
    )
}

fn historical_topology(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    brep: &cadmpeg_asm::brep::AsmBrep,
) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
    macro_rules! topology_some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }

    fn refs<'a, Owner>(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        owners: &'a [Owner],
        id: impl Fn(&'a Owner) -> &'a str,
    ) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError>
    where
        Owner: 'a,
    {
        let owners = ctx.admit_iter(owners, "scan F3D historical topology reference owners")?;
        ctx.collect_fallible_options(
            owners.map(|owner| stable_ref(ctx, id(owner))),
            "collect F3D historical topology references",
        )
    }

    fn relations<'a, Owner, Member, OwnerMembers, MemberId>(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        owners: &'a [Owner],
        owner_members: OwnerMembers,
        member_id: MemberId,
    ) -> Result<Option<Vec<AsmHistoricalRelation>>, cadmpeg_core::CodecError>
    where
        Member: 'a,
        OwnerMembers: Fn(&'a Owner) -> (&'a str, &'a [Member]),
        MemberId: Fn(&'a Member) -> &'a str,
    {
        let owners = ctx.admit_iter(owners, "scan F3D historical topology relation owners")?;
        ctx.collect_fallible_options(
            owners.map(
                |owner| -> Result<Option<AsmHistoricalRelation>, cadmpeg_core::CodecError> {
                    let (owner, members) = owner_members(owner);
                    let Some(owner_ref) = stable_ref(ctx, owner)? else {
                        return Ok(None);
                    };
                    let member_refs = ctx.collect_fallible_options(
                        ctx.admit_iter(members, "scan F3D historical topology relation members")?
                            .map(|member| stable_ref(ctx, member_id(member))),
                        "collect F3D historical topology references",
                    )?;
                    let Some(member_refs) = member_refs else {
                        return Ok(None);
                    };
                    Ok(Some(AsmHistoricalRelation {
                        owner_ref,
                        member_refs,
                    }))
                },
            ),
            "collect F3D historical topology relations",
        )
    }

    fn face_loop_relations(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        faces: &[cadmpeg_ir::topology::Face],
    ) -> Result<Option<Vec<AsmHistoricalRelation>>, cadmpeg_core::CodecError> {
        let faces = ctx.admit_iter(faces, "scan F3D historical topology relation owners")?;
        ctx.collect_fallible_options(
            faces.map(
                |face| -> Result<Option<AsmHistoricalRelation>, cadmpeg_core::CodecError> {
                    let Some(owner_ref) = stable_ref(ctx, face.id.as_str())? else {
                        return Ok(None);
                    };
                    let mut member_refs = Vec::new();
                    match &face.loops {
                        cadmpeg_ir::topology::FaceLoops::Unspecified { loops } => {
                            for loop_id in ctx.admit_iter(
                                loops,
                                "scan F3D historical topology relation members",
                            )? {
                                let Some(loop_ref) = stable_ref(ctx, loop_id.as_str())? else {
                                    return Ok(None);
                                };
                                ctx.push_vec(
                                    &mut member_refs,
                                    loop_ref,
                                    "collect F3D historical topology references",
                                )?;
                            }
                        }
                        cadmpeg_ir::topology::FaceLoops::Classified { outer, inner } => {
                            ctx.charge_work(1, "visit F3D historical classified face loop")?;
                            let Some(outer_ref) = stable_ref(ctx, outer.as_str())? else {
                                return Ok(None);
                            };
                            ctx.push_vec(
                                &mut member_refs,
                                outer_ref,
                                "collect F3D historical topology references",
                            )?;
                            for loop_id in ctx.admit_iter(
                                inner,
                                "scan F3D historical topology relation members",
                            )? {
                                let Some(loop_ref) = stable_ref(ctx, loop_id.as_str())? else {
                                    return Ok(None);
                                };
                                ctx.push_vec(
                                    &mut member_refs,
                                    loop_ref,
                                    "collect F3D historical topology references",
                                )?;
                            }
                        }
                    }
                    Ok(Some(AsmHistoricalRelation {
                        owner_ref,
                        member_refs,
                    }))
                },
            ),
            "collect F3D historical topology relations",
        )
    }

    let mut surface_radii = Vec::new();
    for surface in ctx.admit_iter(&brep.surfaces, "scan F3D historical surface radii")? {
        use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
        let radius = match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                cylinder_surface.radius().get()
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
                sphere_surface.radius().get()
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                torus_surface.minor_radius().get()
            }
            _ => continue,
        };
        let Some(surface_ref) = stable_ref(ctx, surface.id.as_str())? else {
            continue;
        };
        ctx.push_vec(
            &mut surface_radii,
            crate::history_records::AsmHistoricalSurfaceRadius {
                surface: surface_ref,
                radius: radius.abs(),
            },
            "collect F3D historical surface radii",
        )?;
    }
    for (owner, procedural) in ctx.admit_iter(
        &brep.procedural_surfaces,
        "scan F3D brep procedural surfaces",
    )? {
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend(definition_payload) =
            procedural.definition()
        else {
            continue;
        };
        let radius = definition_payload.radius();

        let cadmpeg_ir::geometry::BlendRadiusLaw::Constant { signed_radius } = radius else {
            continue;
        };
        let Some(surface) = stable_ref(ctx, owner.as_str())? else {
            continue;
        };
        ctx.retain_vec(
            &mut surface_radii,
            |candidate| Ok(candidate.surface != surface),
            "retain F3D historical surface radii",
        )?;

        ctx.reserve_vec(
            &mut surface_radii,
            1,
            "collect F3D historical surface radii",
        )?;
        surface_radii.push(crate::history_records::AsmHistoricalSurfaceRadius {
            surface,
            radius: signed_radius.get().abs(),
        });
    }
    ctx.stable_sort_by(
        &mut surface_radii,
        |value| &value.surface,
        Ord::cmp,
        "sort F3D historical surface radii",
    )?;
    let mut surface_cylinders = Vec::new();
    for surface in ctx.admit_iter(&brep.surfaces, "scan F3D historical surface cylinders")? {
        let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved()
        else {
            continue;
        };
        let Some(surface_ref) = stable_ref(ctx, surface.id.as_str())? else {
            continue;
        };
        ctx.push_vec(
            &mut surface_cylinders,
            crate::history_records::AsmHistoricalCylinder {
                surface: surface_ref,
                origin: cylinder_surface.origin().get(),
                axis: *cylinder_surface.frame().axis().as_raw(),
                radius: cylinder_surface.radius().get().abs(),
            },
            "collect F3D historical surface cylinders",
        )?;
    }
    ctx.stable_sort_by(
        &mut surface_cylinders,
        |value| &value.surface,
        Ord::cmp,
        "sort F3D historical surface cylinders",
    )?;
    let mut surface_planes = Vec::new();
    for surface in ctx.admit_iter(&brep.surfaces, "scan F3D historical surface planes")? {
        let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = surface.geometry.solved() else {
            continue;
        };
        let Some(surface_ref) = stable_ref(ctx, surface.id.as_str())? else {
            continue;
        };
        ctx.push_vec(
            &mut surface_planes,
            crate::history_records::AsmHistoricalPlane {
                surface: surface_ref,
                origin: plane_surface.origin().get(),
                normal: *plane_surface.frame().axis().as_raw(),
            },
            "collect F3D historical surface planes",
        )?;
    }
    ctx.stable_sort_by(
        &mut surface_planes,
        |value| &value.surface,
        Ord::cmp,
        "sort F3D historical surface planes",
    )?;
    let mut surface_axes = Vec::new();
    for surface in ctx.admit_iter(&brep.surfaces, "scan F3D historical surface axes")? {
        use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
        let (origin, direction) = match surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => (
                cylinder_surface.origin().get(),
                *cylinder_surface.frame().axis().as_raw(),
            ),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => (
                cone_surface.origin().get(),
                *cone_surface.frame().axis().as_raw(),
            ),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => (
                torus_surface.center().get(),
                *torus_surface.frame().axis().as_raw(),
            ),
            _ => continue,
        };
        let Some(surface_ref) = stable_ref(ctx, surface.id.as_str())? else {
            continue;
        };
        ctx.push_vec(
            &mut surface_axes,
            crate::history_records::AsmHistoricalSurfaceAxis {
                surface: surface_ref,
                origin,
                direction,
            },
            "collect F3D historical surface axes",
        )?;
    }
    ctx.stable_sort_by(
        &mut surface_axes,
        |value| &value.surface,
        Ord::cmp,
        "sort F3D historical surface axes",
    )?;

    Ok(Some(AsmHistoricalTopology {
        bodies: topology_some!(refs(ctx, &brep.bodies, |entity| entity.id.as_str())?),
        regions: topology_some!(refs(ctx, &brep.regions, |entity| entity.id.as_str())?),
        shells: topology_some!(refs(ctx, &brep.shells, |entity| entity.id.as_str())?),
        faces: topology_some!(refs(ctx, &brep.faces, |entity| entity.id.as_str())?),
        loops: topology_some!(refs(ctx, &brep.loops, |entity| entity.id.as_str())?),
        coedges: topology_some!(refs(ctx, &brep.coedges, |entity| entity.id.as_str())?),
        edges: topology_some!(refs(ctx, &brep.edges, |entity| entity.id.as_str())?),
        vertices: topology_some!(refs(ctx, &brep.vertices, |entity| entity.id.as_str())?),
        points: topology_some!(refs(ctx, &brep.points, |entity| entity.id.as_str())?),
        surfaces: topology_some!(refs(ctx, &brep.surfaces, |entity| entity.id.as_str())?),
        surface_radii,
        surface_cylinders,
        surface_planes,
        surface_axes,
        curves: topology_some!(refs(ctx, &brep.curves, |entity| entity.id.as_str())?),
        curve_axes: {
            let mut curve_axes = Vec::new();
            for curve in ctx.admit_iter(&brep.curves, "scan F3D historical curve axes")? {
                use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
                let (origin, direction) = match curve.geometry {
                    CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                        (line_curve.origin().get(), *line_curve.direction().as_raw())
                    }
                    CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => (
                        circle_curve.center().get(),
                        *circle_curve.frame().axis().as_raw(),
                    ),
                    CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => (
                        ellipse_curve.center().get(),
                        *ellipse_curve.frame().axis().as_raw(),
                    ),
                    _ => continue,
                };
                let Some(curve_ref) = stable_ref(ctx, curve.id.as_str())? else {
                    continue;
                };
                ctx.push_vec(
                    &mut curve_axes,
                    crate::history_records::AsmHistoricalCurveAxis {
                        curve: curve_ref,
                        origin,
                        direction,
                    },
                    "collect F3D historical curve axes",
                )?;
            }
            curve_axes
        },
        pcurves: topology_some!(refs(ctx, &brep.pcurves, |entity| entity.id.as_str())?),
        persistent_subentity_tags: Vec::new(),
        body_regions: topology_some!(relations(
            ctx,
            &brep.bodies,
            |body| (body.id.as_str(), body.regions.as_slice()),
            cadmpeg_ir::ids::RegionId::as_str,
        )?),
        region_shells: topology_some!(relations(
            ctx,
            &brep.regions,
            |region| (region.id.as_str(), region.shells.as_slice()),
            cadmpeg_ir::ids::ShellId::as_str,
        )?),
        shell_faces: topology_some!(relations(
            ctx,
            &brep.shells,
            |shell| (shell.id.as_str(), shell.faces()),
            cadmpeg_ir::ids::FaceId::as_str,
        )?),
        shell_wire_edges: topology_some!(relations(
            ctx,
            &brep.shells,
            |shell| (shell.id.as_str(), shell.wire_edges()),
            cadmpeg_ir::ids::EdgeId::as_str,
        )?),
        shell_free_vertices: topology_some!(relations(
            ctx,
            &brep.shells,
            |shell| (shell.id.as_str(), shell.free_vertices()),
            cadmpeg_ir::ids::VertexId::as_str,
        )?),
        face_loops: topology_some!(face_loop_relations(ctx, &brep.faces)?),
        loop_coedges: topology_some!(relations(
            ctx,
            &brep.loops,
            |loop_| (loop_.id.as_str(), loop_.coedges()),
            cadmpeg_ir::ids::CoedgeId::as_str,
        )?),
        coedge_topology: topology_some!(ctx.collect_fallible_options(
            brep.coedges.iter().map(
                |coedge| -> Result<Option<AsmHistoricalCoedge>, cadmpeg_core::CodecError> {
                    let (next, previous) =
                        match cadmpeg_ir::topology::coedge_ring_neighbors(&brep.loops, coedge) {
                            Some(neighbors) => neighbors,
                            None => return Ok(None),
                        };
                    let Some(coedge_ref) = stable_ref(ctx, coedge.id.as_str())? else {
                        return Ok(None);
                    };
                    let Some(owner_loop_ref) = stable_ref(ctx, coedge.owner_loop.as_str())? else {
                        return Ok(None);
                    };
                    let Some(edge_ref) = stable_ref(ctx, coedge.edge.as_str())? else {
                        return Ok(None);
                    };
                    let Some(next_ref) = stable_ref(ctx, next.as_str())? else {
                        return Ok(None);
                    };
                    let Some(previous_ref) = stable_ref(ctx, previous.as_str())? else {
                        return Ok(None);
                    };
                    let Some(radial_next_ref) = stable_ref(ctx, coedge.radial_next.as_str())?
                    else {
                        return Ok(None);
                    };
                    Ok(Some(AsmHistoricalCoedge {
                        coedge: coedge_ref,
                        owner_loop: owner_loop_ref,
                        edge: edge_ref,
                        next: next_ref,
                        previous: previous_ref,
                        radial_next: radial_next_ref,
                    }))
                }
            ),
            "collect F3D historical coedges"
        )?),
        edge_vertices: topology_some!(ctx.collect_fallible_options(
            brep.edges.iter().map(
                |edge| -> Result<Option<AsmHistoricalEdge>, cadmpeg_core::CodecError> {
                    let Some(edge_ref) = stable_ref(ctx, edge.id.as_str())? else {
                        return Ok(None);
                    };
                    let Some(start_vertex_ref) = stable_ref(ctx, edge.start.as_str())? else {
                        return Ok(None);
                    };
                    let Some(end_vertex_ref) = stable_ref(ctx, edge.end.as_str())? else {
                        return Ok(None);
                    };
                    Ok(Some(AsmHistoricalEdge {
                        edge: edge_ref,
                        start_vertex: start_vertex_ref,
                        end_vertex: end_vertex_ref,
                    }))
                }
            ),
            "collect F3D historical edges"
        )?),
        face_surfaces: topology_some!(ctx.collect_fallible_options(
            brep.faces.iter().map(
                |face| -> Result<Option<AsmHistoricalCarrierBinding>, cadmpeg_core::CodecError> {
                    let Some(entity) = stable_ref(ctx, face.id.as_str())? else {
                        return Ok(None);
                    };
                    let Some(carrier) = stable_ref(ctx, face.surface.as_str())? else {
                        return Ok(None);
                    };
                    Ok(Some(AsmHistoricalCarrierBinding { entity, carrier }))
                }
            ),
            "collect F3D historical face surfaces"
        )?),
        edge_curves: topology_some!(ctx.collect_fallible_options(
            brep.edges.iter().map(
                |edge| -> Result<
                    Option<AsmHistoricalOptionalCarrierBinding>,
                    cadmpeg_core::CodecError,
                > {
                    let Some(entity) = stable_ref(ctx, edge.id.as_str())? else {
                        return Ok(None);
                    };
                    let carrier = match edge.curve() {
                        Some(curve) => match stable_ref(ctx, curve.as_str())? {
                            Some(carrier) => Some(carrier),
                            None => return Ok(None),
                        },
                        None => None,
                    };
                    Ok(Some(AsmHistoricalOptionalCarrierBinding {
                        entity,
                        carrier,
                    }))
                }
            ),
            "collect F3D historical edge curves"
        )?),
        coedge_pcurves: topology_some!(ctx.collect_fallible_options(
            brep.coedges.iter().map(
                |coedge| -> Result<
                    Option<AsmHistoricalOptionalCarrierBinding>,
                    cadmpeg_core::CodecError,
                > {
                    let Some(entity) = stable_ref(ctx, coedge.id.as_str())? else {
                        return Ok(None);
                    };
                    let carrier = match coedge.pcurves.first() {
                        Some(use_) => match stable_ref(ctx, use_.pcurve.as_str())? {
                            Some(carrier) => Some(carrier),
                            None => return Ok(None),
                        },
                        None => None,
                    };
                    Ok(Some(AsmHistoricalOptionalCarrierBinding {
                        entity,
                        carrier,
                    }))
                }
            ),
            "collect F3D historical coedge pcurves"
        )?),
        vertex_points: topology_some!(ctx.collect_fallible_options(
            brep.vertices.iter().map(
                |vertex| -> Result<Option<AsmHistoricalCarrierBinding>, cadmpeg_core::CodecError> {
                    let Some(entity) = stable_ref(ctx, vertex.id.as_str())? else {
                        return Ok(None);
                    };
                    let Some(carrier) = stable_ref(ctx, vertex.point.as_str())? else {
                        return Ok(None);
                    };
                    Ok(Some(AsmHistoricalCarrierBinding { entity, carrier }))
                }
            ),
            "collect F3D historical vertex points"
        )?),
        point_positions: topology_some!(ctx.collect_fallible_options(
            brep.points.iter().map(
                |point| -> Result<Option<AsmHistoricalPoint>, cadmpeg_core::CodecError> {
                    let Some(point_ref) = stable_ref(ctx, point.id.as_str())? else {
                        return Ok(None);
                    };
                    Ok(Some(AsmHistoricalPoint {
                        point: point_ref,
                        position: point.position().get(),
                    }))
                }
            ),
            "collect F3D historical point positions"
        )?),
    }))
}

pub(crate) fn historical_topology_with_tags(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    brep: &crate::brep::Brep,
) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
    let Some(mut topology) = historical_topology(ctx, &brep.asm)? else {
        return Ok(None);
    };
    for tag in ctx.admit_iter(
        &brep.persistent_subentity_tags,
        "scan F3D brep persistent subentity tags",
    )? {
        let (entity_kind, entity_ref) = match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(face) => (
                AsmHistoricalEntityKind::Face,
                stable_ref(ctx, face.as_str())?,
            ),
            cadmpeg_ir::attributes::AttributeTarget::Edge(edge) => (
                AsmHistoricalEntityKind::Edge,
                stable_ref(ctx, edge.as_str())?,
            ),
            _ => continue,
        };
        let Some(entity_ref) = entity_ref else {
            continue;
        };
        let design_references = ctx.collect_vec(
            tag.design_references.iter().copied(),
            "collect F3D historical tag design references",
        )?;
        let token = ctx.copy_retained_text(tag.token.as_str(), "copy F3D historical tag token")?;

        ctx.reserve_vec(
            &mut topology.persistent_subentity_tags,
            1,
            "collect F3D historical persistent tags",
        )?;
        topology.persistent_subentity_tags.push(
            crate::history_records::AsmHistoricalPersistentSubentityTag {
                entity_kind,
                entity_ref,
                selector: tag.selector,
                token,
                design_references,
                ordinal: tag.ordinal,
            },
        );
    }
    Ok(Some(topology))
}

#[derive(Debug)]
struct MaterializedRecordTable<'ctx> {
    records: Vec<cadmpeg_asm::sab::Record>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn materialize_record_table<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    state: &AsmDeltaState,
    archive: &HistoricalRecordArchive<'_>,
) -> Result<Option<MaterializedRecordTable<'ctx>>, cadmpeg_core::CodecError> {
    if state.entity_versions.is_empty() {
        return Ok(None);
    }

    let mut present_storage = ctx.reserve_scoped(0, "index F3D historical record presence")?;
    let mut present = HashSet::new();
    for version in ctx.admit_iter(&state.entity_versions, "scan F3D state entity versions")? {
        present_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut present,
                version.entity_ref,
                "index F3D historical record presence",
            )
        })?;
    }
    if present.len() != state.entity_versions.len() {
        return Ok(None);
    }

    let mut records_storage = ctx.reserve_scoped(0, "materialize F3D historical record table")?;
    let mut records = Vec::new();
    for version in ctx.admit_iter(&state.entity_versions, "scan F3D state entity versions")? {
        let Some(record) = ctx.get_btree_map(
            &archive.records,
            &version.record_ref,
            "find F3D archived record revision",
        )?
        else {
            return Ok(None);
        };
        if i64::try_from(record.index).ok() != Some(version.entity_ref) {
            return Ok(None);
        }
        for token in ctx.admit_iter(record.tokens.as_ref(), "scan F3D history record tokens")? {
            let cadmpeg_asm::sab::Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0 && !present.contains(reference) {
                return Ok(None);
            }
        }
        records_storage.with_storage(|| {
            let cloned = clone_historical_record(ctx, record)?;
            ctx.push_vec(
                &mut records,
                cloned,
                "materialize F3D historical record table",
            )
        })?;
    }
    ctx.sort_unstable_by(
        &mut records,
        |value| &value.index,
        Ord::cmp,
        "sort F3D historical records",
    )?;
    Ok(Some(MaterializedRecordTable {
        records,
        _storage: records_storage,
    }))
}

fn decode_bulletin_boards(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    mut position: usize,
    stream: &str,
    state_offset: usize,
    state_id: &str,
    width: RefWidth,
) -> Result<Option<(Vec<AsmBulletinBoard>, usize)>, cadmpeg_core::CodecError> {
    if bytes.get(position) == Some(&0x11) {
        return Ok(Some((Vec::new(), position)));
    }
    let mut boards = Vec::new();
    loop {
        ctx.charge_work(1, "read F3D ASM bulletin board entry")?;
        let board_offset = position;
        let Some(present) = take_int(bytes, &mut position, 0x04, width) else {
            return Ok(None);
        };
        if present == 0 {
            break;
        }
        let Some(owner_ref) = take_int(bytes, &mut position, 0x0c, width) else {
            return Ok(None);
        };
        let Some(number) = take_int(bytes, &mut position, 0x04, width) else {
            return Ok(None);
        };
        let board_id = crate::ids::native_scoped_id(
            ctx,
            stream,
            "asm-bulletin-board",
            format_args!("{state_offset:010}:{:06}", boards.len()),
        )?;
        let mut changes = Vec::new();
        loop {
            ctx.charge_work(1, "read F3D ASM entity change entry")?;
            let change_offset = position;
            let Some(present) = take_int(bytes, &mut position, 0x04, width) else {
                return Ok(None);
            };
            if present == 0 {
                break;
            }
            let Some(old) = take_int(bytes, &mut position, 0x0c, width) else {
                return Ok(None);
            };
            let Some(new) = take_int(bytes, &mut position, 0x0c, width) else {
                return Ok(None);
            };
            let kind = match (old >= 0, new >= 0) {
                (false, true) => AsmEntityChangeKind::Insert { new },
                (true, false) => AsmEntityChangeKind::Delete { old },
                (true, true) => AsmEntityChangeKind::Update { old, new },
                (false, false) => return Ok(None),
            };

            ctx.reserve_vec(&mut changes, 1, "admit F3D ASM entity change")?;
            let change_id = crate::ids::native_scoped_id(
                ctx,
                stream,
                "asm-entity-change",
                format_args!(
                    "{state_offset:010}:{:06}:{:06}",
                    boards.len(),
                    changes.len()
                ),
            )?;
            let parent = ctx.copy_retained_text(&board_id, "copy F3D ASM change parent")?;
            changes.push(AsmEntityChange {
                id: change_id,
                parent,
                byte_offset: u64_from_index(change_offset),
                kind,
            });
        }

        ctx.reserve_vec(&mut boards, 1, "admit F3D ASM bulletin board")?;
        let parent = ctx.copy_retained_text(state_id, "copy F3D ASM board parent")?;
        boards.push(AsmBulletinBoard {
            id: board_id,
            parent,
            byte_offset: u64_from_index(board_offset),
            owner_ref,
            number,
            changes,
        });
    }
    Ok(Some((boards, position)))
}

fn decode_history_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    state_end: usize,
    next_delta: Option<usize>,
    stream: &str,
    state_id: &str,
    width: RefWidth,
) -> Result<Vec<AsmHistoryRecord>, cadmpeg_core::CodecError> {
    let mut start = state_end + usize::from(bytes.get(state_end) == Some(&0x11));
    if bytes.get(start) == Some(&0x04)
        && int_at(bytes, start + 1, width) == Some(0)
        && bytes.get(start + 1 + width.bytes()) == Some(&0x11)
    {
        start += 2 + width.bytes();
    }
    let limit = next_delta.map_or(bytes.len(), |offset| offset + 1);
    if start >= limit {
        return Ok(Vec::new());
    }
    match cadmpeg_asm::sab::frame_history(ctx, bytes, start, limit, width, None) {
        Ok(records) => ctx.try_collect_vec(
            records.into_iter().map(|record| {
                let reference_count = ctx
                    .admit_iter(
                        record.tokens.as_ref(),
                        "count F3D history record references",
                    )?
                    .filter(|token| matches!(token, cadmpeg_asm::sab::Token::Ref(_)))
                    .count();

                let mut entity_references = Vec::new();
                ctx.reserve_capacity(
                    &mut entity_references,
                    reference_count,
                    "frame F3D history references",
                )?;
                for token in
                    ctx.admit_iter(record.tokens.as_ref(), "scan F3D history record tokens")?
                {
                    if let cadmpeg_asm::sab::Token::Ref(value) = token {
                        ctx.push_vec(
                            &mut entity_references,
                            *value,
                            "frame F3D history references",
                        )?;
                    }
                }
                let end = record.offset.checked_add(record.len).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("F3D history record byte range overflows")
                })?;
                let source = bytes.get(record.offset..end).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "F3D history record byte range exceeds stream",
                    )
                })?;
                let raw_bytes = ctx.copy_retained(source, "retain F3D history record")?;

                let id = crate::ids::native_scoped_id(
                    ctx,
                    stream,
                    "asm-history-record",
                    format_args!("{:010}", record.offset),
                )?;
                let parent = ctx.copy_retained_text(state_id, "copy F3D history record parent")?;
                Ok(AsmHistoryRecord {
                    id,
                    parent,
                    revision_id: None,
                    byte_offset: u64_from_index(record.offset),
                    framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                        index: u64_from_index(record.index),
                        name: record.name,
                        entity_references,
                    },
                    raw_bytes,
                })
            }),
            "frame F3D history record",
        ),
        Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => Err(error.into()),
        Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
            Err(error.into_codec_error())
        }
        Err(error) => {
            let raw_bytes =
                ctx.copy_retained(&bytes[start..limit], "retain opaque F3D history record")?;
            let mut decoded = ctx.collection_vec(1, "frame opaque F3D history record")?;
            let id = crate::ids::native_scoped_id(
                ctx,
                stream,
                "asm-history-record",
                format_args!("{start:010}"),
            )?;
            let parent =
                ctx.copy_retained_text(state_id, "copy opaque F3D history record parent")?;
            let error =
                ctx.format_retained(format_args!("{error}"), "retain opaque F3D history error")?;
            decoded.push(AsmHistoryRecord {
                id,
                parent,
                revision_id: None,
                byte_offset: u64_from_index(start),
                framing: crate::history_records::AsmHistoryRecordFraming::Opaque { error },
                raw_bytes,
            });
            Ok(decoded)
        }
    }
}

fn decode_preamble(bytes: &[u8], mut position: usize, width: RefWidth) -> Option<(i64, i64)> {
    let size = take_int(bytes, &mut position, 0x04, width)?;
    let duplicate = take_int(bytes, &mut position, 0x04, width)?;
    let zero = take_int(bytes, &mut position, 0x04, width)?;
    let entry_count = take_int(bytes, &mut position, 0x04, width)?;
    (size == duplicate && zero == 0).then_some((size, entry_count))
}

/// Read a tagged little-endian signed integer of the stream's ref width (4 or
/// 8 bytes) and advance past it.
fn take_int(bytes: &[u8], position: &mut usize, tag: u8, width: RefWidth) -> Option<i64> {
    if bytes.get(*position) != Some(&tag) {
        return None;
    }
    let value = int_at(bytes, *position + 1, width)?;
    *position += 1 + width.bytes();
    Some(value)
}

#[cfg(test)]
mod tests;
