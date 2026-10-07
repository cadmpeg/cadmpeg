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
    let mut head = None;
    let mut heads = 0_usize;
    let mut tails = 0_usize;
    for state in decode.admit_iter(&history.states, "scan F3D ASM history chain ends")? {
        if state.previous_ref.is_none() {
            head = head.or(Some(state));
            heads += 1;
        }
        if state.next_ref.is_none() {
            tails += 1;
        }
    }
    let (Some(head), 1, 1) = (head, heads, tails) else {
        return Ok(false);
    };
    if let Some(preamble) = history.preamble {
        if head.state_id != preamble.stream_size || preamble.history_entry_count < 0 {
            return Ok(false);
        }
    }
    // Every visited state must name the state visited before it, and only the
    // head names none, so the walk cannot revisit a state: the first repeat
    // would need its predecessor to repeat earlier. Counting steps therefore
    // counts distinct states.
    let mut visited = 0_usize;
    let mut previous = None;
    let mut current = Some(head.node_index);
    while let Some(index) = current {
        decode.charge_work(1, "visit F3D ASM history chain link")?;
        let Some(state) = by_index.get(&index) else {
            return Ok(false);
        };
        if state.previous_ref != previous {
            return Ok(false);
        }
        visited += 1;
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
    Ok(visited == history.states.len())
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
    let mut delta_offsets_storage = ctx.reserve_scoped(0, "collect F3D ASM delta offsets")?;
    let mut delta_offsets = Vec::new();
    for offset in ctx.find_bytes_iter(bytes, DELTA, "find F3D ASM delta markers")? {
        ctx.push_scoped_vec(
            &mut delta_offsets_storage,
            &mut delta_offsets,
            offset,
            "collect F3D ASM delta offsets",
        )?;
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
    let mut old_references_storage = ctx.reserve_scoped(0, "collect F3D ASM old references")?;
    let mut old_references = Vec::new();
    for state in ctx.admit_iter(&*states, "scan F3D snapshot reference states")? {
        for board in ctx.admit_iter(&state.bulletin_boards, "scan F3D snapshot reference boards")? {
            for change in ctx.admit_iter(&board.changes, "scan F3D snapshot reference changes")? {
                let Some(old_reference) = change.old_ref() else {
                    continue;
                };
                ctx.push_scoped_vec(
                    &mut old_references_storage,
                    &mut old_references,
                    old_reference,
                    "collect F3D ASM old references",
                )?;
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
    if !is_consecutive_from(ctx, &old_references, first, "check F3D ASM old references")? {
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
    let mut revision_ids = ctx.admit_iter(old_references, "bind F3D snapshot revision IDs")?;
    for state in ctx.admit_iter(&mut *states, "bind F3D snapshot revision states")? {
        for record in ctx.admit_iter(&mut state.records, "bind F3D snapshot revision records")? {
            if record.name() != "End-of-ASM-data" {
                record.revision_id = revision_ids.next();
            }
        }
    }
    Ok(())
}

/// Tests that `values` are exactly `first..first + values.len()`, an end that
/// must be representable, charging only the values compared before the first
/// gap.
fn is_consecutive_from(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    values: &[i64],
    first: i64,
    operation: &'static str,
) -> Result<bool, cadmpeg_core::CodecError> {
    let end = i64::try_from(values.len())
        .ok()
        .and_then(|count| first.checked_add(count));
    if end.is_none() {
        return Ok(false);
    }
    ctx.all_by(
        values.iter().zip(0_i64..),
        |(&value, offset)| Ok(first.checked_add(offset) == Some(value)),
        operation,
    )
}

fn is_history_boundary_record(record: &AsmHistoryRecord) -> bool {
    matches!(
        record.name(),
        "End-of-ASM-History-Section" | "End-of-ASM-data"
    )
}

/// Collects every archived record revision ID in ascending order, held for
/// as long as the caller keeps the reservation.
fn archived_revision_ids<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
    operation: &'static str,
) -> Result<(Vec<i64>, cadmpeg_core::decode::ScopedReservation<'ctx>), cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut archived = Vec::new();
    for state in ctx.admit_iter(states, "scan F3D archived history states")? {
        for record in ctx.admit_iter(&state.records, "scan F3D archived state records")? {
            let Some(revision_id) = record.revision_id else {
                continue;
            };
            ctx.push_scoped_vec(&mut storage, &mut archived, revision_id, operation)?;
        }
    }
    ctx.sort_unstable_by(
        &mut archived,
        |value| value,
        Ord::cmp,
        "sort F3D archived revisions",
    )?;
    Ok((archived, storage))
}

/// Returns the active `RecordTable` length named by sorted archived revision
/// IDs, which must be exactly the slots after the active records.
fn archived_active_count(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    archived: &[i64],
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let Some(&active_count) = archived.first() else {
        return Ok(None);
    };
    if active_count <= 0
        || !is_consecutive_from(ctx, archived, active_count, "check F3D archived revisions")?
    {
        return Ok(None);
    }
    Ok(usize::try_from(active_count).ok())
}

fn archived_active_record_count(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let (archived, _storage) =
        archived_revision_ids(ctx, states, "collect F3D archived revisions")?;
    archived_active_count(ctx, &archived)
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
    let only_boundary_records = ctx.all_by(
        states,
        |state| {
            ctx.all_by(
                &state.records,
                |record| {
                    has_boundary_record = true;
                    Ok(record.revision_id.is_none() && is_history_boundary_record(record))
                },
                "scan F3D insert-only state records",
            )
        },
        "scan F3D insert-only history states",
    )?;
    if !only_boundary_records || !has_boundary_record {
        return Ok(None);
    }
    let mut inserted_storage = ctx.reserve_scoped(0, "index F3D insert-only revisions")?;
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
                if new_ref <= 0
                    || !ctx.insert_scoped_btree_set(
                        &mut inserted_storage,
                        &mut inserted,
                        new_ref,
                        "find F3D insert-only revision",
                        "index F3D insert-only revisions",
                    )?
                {
                    return Ok(None);
                }
            }
        }
    }
    // Distinct positive references cover `1..=last` exactly when there are
    // `last` of them.
    let Some(&last) = inserted.last() else {
        return Ok(None);
    };
    if usize::try_from(last).ok() != Some(inserted.len()) {
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
    let (archived_ids, _archived_ids_storage) =
        archived_revision_ids(ctx, states, "index F3D archived revision IDs")?;
    let active_count = match archived_active_count(ctx, &archived_ids)? {
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
    let is_head =
        |state: &AsmDeltaState| Ok::<_, cadmpeg_core::CodecError>(state.previous_ref.is_none());
    let Some(mut ordinal) = ctx.position_by(&*states, is_head, "find F3D history version head")?
    else {
        return Ok(());
    };
    if ctx.any_by(
        states.get(ordinal + 1..).unwrap_or_default(),
        is_head,
        "find F3D history version head",
    )? {
        return Ok(());
    }
    let active_count_u64 = u64::try_from(active_count)
        .map_err(|_| ctx.refuse_codec_limit("seed F3D history versions", 0, u64::MAX))?;
    ctx.charge_work(active_count_u64, "seed F3D history versions")?;
    let mut versions_storage = ctx.reserve_scoped(0, "seed F3D history versions")?;
    let mut versions = BTreeMap::new();
    versions_storage.with_storage(|| {
        for id in 0..active_count {
            ctx.insert_btree_map(&mut versions, id, id, "seed F3D history versions")?;
        }
        Ok::<(), cadmpeg_core::CodecError>(())
    })?;
    // The projected vectors become the states' retained versions when the
    // whole chain binds; the index over them is scratch.
    let mut state_versions_storage = ctx.reserve_scoped(0, "materialize F3D state versions")?;
    let mut projected_storage = ctx.reserve_scoped(0, "index F3D state version projections")?;
    let mut projected = HashMap::new();
    loop {
        ctx.charge_work(1, "bind F3D historical entity versions")?;
        let state = &states[ordinal];
        // Every visited state is projected before the walk moves on, so a
        // projected state is a revisited one.
        if projected.contains_key(&state.node_index) {
            return Ok(());
        }
        let state_versions = state_versions_storage.with_storage(|| {
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
            Ok::<_, cadmpeg_core::CodecError>(state_versions)
        })?;
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
                        if !ctx.contains_key_btree_map(
                            &versions,
                            &new,
                            "find F3D updated historical version",
                        )? || ctx
                            .binary_search(
                                archived_ids.as_slice(),
                                &old,
                                "find archived F3D revision for update",
                            )?
                            .is_err()
                        {
                            return Ok(());
                        }
                        versions_storage.with_storage(|| {
                            ctx.insert_btree_map(
                                &mut versions,
                                new,
                                old,
                                "update F3D historical version",
                            )
                        })?;
                    }
                    AsmEntityChangeKind::Insert { new } => {
                        if ctx
                            .remove_btree_map(
                                &mut versions,
                                &new,
                                "remove F3D inserted historical version",
                            )?
                            .is_none()
                        {
                            return Ok(());
                        }
                    }
                    AsmEntityChangeKind::Delete { old } => {
                        if ctx.contains_key_btree_map(
                            &versions,
                            &old,
                            "find F3D deleted historical version",
                        )? || ctx
                            .binary_search(
                                archived_ids.as_slice(),
                                &old,
                                "find archived F3D revision for delete",
                            )?
                            .is_err()
                        {
                            return Ok(());
                        }
                        versions_storage.with_storage(|| {
                            ctx.insert_btree_map(
                                &mut versions,
                                old,
                                old,
                                "restore F3D historical version",
                            )
                        })?;
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
    if projected.len() != states.len()
        || versions.len() != 1
        || ctx.get_btree_map(&versions, &0, "find F3D root historical version")? != Some(&0)
    {
        return Ok(());
    }
    state_versions_storage.commit()?;
    for state in ctx.admit_iter(&mut *states, "bind F3D historical version states")? {
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
    // An archived history has revision IDs and an insert-only one has none,
    // so at most one of the two counts exists.
    let active_count = match archived_active_record_count(ctx, states)? {
        Some(count) => count,
        None => match insert_only_active_record_count(ctx, states)? {
            Some(count) if framed.len() == count => count,
            _ => return Ok(false),
        },
    };
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
    let mut complete_states = states.iter_mut();
    while let Some(state) =
        ctx.next_charged(&mut complete_states, "bind F3D complete record tables")?
    {
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
        for state in ctx.admit_iter(states, "retain F3D historical entity versions")? {
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
        for state in ctx.admit_iter(states, "release F3D incomplete topology caches")? {
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
    if ctx.any_by(
        active_records.iter().enumerate(),
        |(index, record)| Ok(record.index != index),
        "validate F3D active record ordinals",
    )? {
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
        for token in ctx.admit_iter(
            std::sync::Arc::make_mut(&mut record.tokens),
            "rebind F3D archived record references",
        )? {
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
    for (state, transition) in ctx
        .admit_iter(&mut *states, "bind F3D historical transitions")?
        .zip(transitions)
    {
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
    let empty = AsmHistoricalTopology::default();
    let previous_topology = previous
        .and_then(|state| state.topology())
        .unwrap_or(&empty);
    let (current_versions, _current_versions_storage) =
        historical_version_map(ctx, &current.entity_versions)?;
    let (previous_versions, _previous_versions_storage) = historical_version_map(
        ctx,
        previous.map_or(&[][..], |state| state.entity_versions.as_slice()),
    )?;
    let versions = (&current_versions, &previous_versions);
    let mut record_key_storage = ctx.reserve_scoped(0, "collect F3D transition version keys")?;
    let current_record_keys = record_key_storage.with_storage(|| {
        historical_version_keys(
            ctx,
            &current_versions,
            "scan F3D current transition version keys",
        )
    })?;
    let previous_record_keys = record_key_storage.with_storage(|| {
        historical_version_keys(
            ctx,
            &previous_versions,
            "scan F3D previous transition version keys",
        )
    })?;
    let delta = |current: &[i64], previous: &[i64]| {
        let (current, _current_storage) = sorted_transition_entities(ctx, current)?;
        let (previous, _previous_storage) = sorted_transition_entities(ctx, previous)?;
        entity_delta(ctx, &current, &previous, versions)
    };
    Ok(Some(AsmHistoricalTransition {
        previous_state_id: previous.map(|state| state.state_id),
        records: entity_delta(ctx, &current_record_keys, &previous_record_keys, versions)?,
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

/// Indexes a state's record version per entity, keeping the last version of a
/// repeated entity, for the duration of one transition.
fn historical_version_map<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    versions: &[AsmEntityVersion],
) -> Result<
    (
        BTreeMap<i64, i64>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    cadmpeg_core::CodecError,
> {
    ctx.collect_scoped_btree_map(
        versions
            .iter()
            .map(|version| (version.entity_ref, version.record_ref)),
        "index F3D transition versions",
    )
}

/// Copies the ascending, distinct entity keys of a version map.
fn historical_version_keys(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    versions: &BTreeMap<i64, i64>,
    operation: &'static str,
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    let mut keys = ctx.collection_vec(versions.len(), "collect F3D transition version keys")?;
    for &entity_ref in ctx
        .admit_iter(versions, operation)?
        .map(|(entity_ref, _)| entity_ref)
    {
        keys.push(entity_ref);
    }
    Ok(keys)
}

/// Copies topology entity references into ascending, distinct scratch order.
fn sorted_transition_entities<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    entities: &[i64],
) -> Result<(Vec<i64>, cadmpeg_core::decode::ScopedReservation<'ctx>), cadmpeg_core::CodecError> {
    let (mut sorted, storage) = ctx
        .copy_temporary_slice(entities, "copy F3D transition entities")
        .map_err(cadmpeg_core::CodecError::ResourceLimit)?;
    ctx.sort_unstable_by(
        &mut sorted,
        |entity| entity,
        Ord::cmp,
        "sort F3D transition entities",
    )?;
    ctx.dedup_vec(&mut sorted, "deduplicate F3D transition entities")?;
    Ok((sorted, storage))
}

/// Splits two ascending, distinct entity lists into inserted, deleted and
/// re-versioned entities in one merge pass. Each list keeps ascending order.
fn entity_delta(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    current: &[i64],
    previous: &[i64],
    (current_versions, previous_versions): (&BTreeMap<i64, i64>, &BTreeMap<i64, i64>),
) -> Result<AsmHistoricalEntityDelta, cadmpeg_core::CodecError> {
    let mut inserted = Vec::new();
    let mut deleted = Vec::new();
    let mut updated = Vec::new();
    let (mut current_index, mut previous_index) = (0, 0);
    loop {
        ctx.charge_work(1, "merge F3D transition entities")?;
        let (target, entity) = match (current.get(current_index), previous.get(previous_index)) {
            (None, None) => break,
            (Some(&entity), None) => {
                current_index += 1;
                (&mut inserted, entity)
            }
            (None, Some(&entity)) => {
                previous_index += 1;
                (&mut deleted, entity)
            }
            (Some(&current_entity), Some(&previous_entity)) => {
                match current_entity.cmp(&previous_entity) {
                    std::cmp::Ordering::Less => {
                        current_index += 1;
                        (&mut inserted, current_entity)
                    }
                    std::cmp::Ordering::Greater => {
                        previous_index += 1;
                        (&mut deleted, previous_entity)
                    }
                    std::cmp::Ordering::Equal => {
                        current_index += 1;
                        previous_index += 1;
                        let current_record = ctx.get_btree_map(
                            current_versions,
                            &current_entity,
                            "find F3D current transition record version",
                        )?;
                        let previous_record = ctx.get_btree_map(
                            previous_versions,
                            &current_entity,
                            "find F3D previous transition record version",
                        )?;
                        if current_record == previous_record {
                            continue;
                        }
                        (&mut updated, current_entity)
                    }
                }
            }
        };
        ctx.push_vec(target, entity, "collect F3D transition delta")?;
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
    // Each history's states by node, absent when node indices repeat, and
    // every state by ID across histories, absent when the ID repeats.
    let mut state_index_storage = ctx.reserve_scoped(0, "index F3D feature output states")?;
    let mut history_nodes = Vec::new();
    let mut states_by_id = HashMap::<i64, Option<(usize, &AsmDeltaState)>>::new();
    for (history_index, history) in ctx
        .admit_iter(histories, "scan F3D feature output histories")?
        .enumerate()
    {
        let nodes = state_index_storage.with_storage(|| {
            ctx.collect_hash_map(
                history.states.iter().map(|state| (state.node_index, state)),
                "index F3D feature output history nodes",
            )
        })?;
        let nodes = (nodes.len() == history.states.len()).then_some(nodes);
        state_index_storage.with_storage(|| {
            ctx.push_vec(
                &mut history_nodes,
                nodes,
                "index F3D feature output history nodes",
            )
        })?;
        for state in ctx.admit_iter(&history.states, "scan F3D history states")? {
            if let Some(indexed) = ctx.get_mut_hash_map(
                &mut states_by_id,
                &state.state_id,
                "index F3D feature output states",
            )? {
                *indexed = None;
                continue;
            }
            state_index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut states_by_id,
                    state.state_id,
                    Some((history_index, state)),
                    "index F3D feature output states",
                )
                .map(|_| ())
            })?;
        }
    }
    let mut active_storage = ctx.reserve_scoped(0, "index F3D active feature output bodies")?;
    let mut active = HashMap::new();
    for body in ctx.admit_iter(active_bodies, "scan F3D active bodies")? {
        let Some(slot) = stable_ref(ctx, body.id.as_str())? else {
            continue;
        };
        active_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut active,
                slot,
                &body.id,
                "index F3D active feature output bodies",
            )
            .map(|_| ())
        })?;
    }
    let scopes = scope_index(scopes);
    // Affected bodies per state, computed only for states a feature names.
    let mut outputs_storage = ctx.reserve_scoped(0, "collect F3D feature output states")?;
    let mut state_outputs = HashMap::<i64, Option<Vec<i64>>>::new();
    for feature in ctx.admit_iter(features, "scan F3D feature outputs")? {
        let Some(id) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(scope) = scopes.get(ctx, id)? else {
            continue;
        };
        let (Some(state_id), Some(previous_state_id)) =
            (scope.history_state_id(), scope.previous_history_state_id())
        else {
            continue;
        };
        let Some(&Some((history_index, state))) = states_by_id.get(&state_id) else {
            continue;
        };
        if state
            .transition
            .as_ref()
            .and_then(|transition| transition.previous_state_id)
            != Some(previous_state_id)
        {
            continue;
        }
        if !state_outputs.contains_key(&state_id) {
            let outputs = outputs_storage.with_storage(|| {
                state_output_bodies(ctx, history_nodes.get(history_index), state)
            })?;
            outputs_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut state_outputs,
                    state_id,
                    outputs,
                    "collect F3D feature output states",
                )
                .map(|_| ())
            })?;
        }
        let Some(Some(outputs)) = state_outputs.get(&state_id) else {
            continue;
        };
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
    Ok(())
}

/// Bodies changed by one state's transition from the state it follows in its
/// history, when that history links its states by unique node indices.
fn state_output_bodies(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    nodes: Option<&Option<HashMap<i64, &AsmDeltaState>>>,
    state: &AsmDeltaState,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    let Some(Some(nodes)) = nodes else {
        return Ok(None);
    };
    let previous = match state.next_ref {
        Some(node) => match nodes.get(&node) {
            Some(previous) => Some(*previous),
            None => return Ok(None),
        },
        None => None,
    };
    affected_body_refs(ctx, state, previous)
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
    for body in ctx.admit_iter(
        feature.evaluation.outputs(),
        "scan F3D BaseFeature output bodies",
    )? {
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
    for feature in ctx.admit_iter(features, "scan F3D sweep features")? {
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
        let mut edit_result = Ok(());
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
                    match ctx.admit_iter(sections, "convert F3D solid sweep sections") {
                        Ok(sections) => solid_sections.extend(sections.map(Into::into)),
                        Err(error) => {
                            edit_result = Err(cadmpeg_core::CodecError::ResourceLimit(error))
                        }
                    }
                    SweepShape::Solid {
                        op,
                        section: section.into(),
                        sections: solid_sections,
                    }
                }
                _ => SweepShape::sheet_sections(mode, section, sections),
            };
        });
        edit_result?;
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

/// The one value matching `matches`, or `None` when none or several do.
/// Charges each visited value, stopping at a second match.
fn unique_by<'v, T>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    values: &'v [T],
    mut matches: impl FnMut(&T) -> bool,
    operation: &'static str,
) -> Result<Option<&'v T>, cadmpeg_core::CodecError> {
    let Some(index) = ctx.position_by(values, |value| Ok(matches(value)), operation)? else {
        return Ok(None);
    };
    let repeated = ctx.any_by(&values[index + 1..], |value| Ok(matches(value)), operation)?;
    Ok((!repeated).then(|| &values[index]))
}

/// `ids::native_stream` after charging its backward scan of `id`.
pub(super) fn native_stream_of<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    id: &'a str,
) -> Result<Option<&'a str>, cadmpeg_core::CodecError> {
    ctx.charge_work(u64_from_index(id.len()), "split F3D native stream")?;
    Ok(crate::ids::native_stream(id))
}

type IndexKey<'a, K, T> = fn(
    &cadmpeg_core::decode::DecodeContext<'_>,
    &'a T,
) -> Result<Option<K>, cadmpeg_core::CodecError>;

/// Values of a slice keyed for lookup, built on the first lookup and held as
/// scratch while the index lives. A key that names one value finds it; a key
/// that several values share finds none, as `DecodeContext::unique_index`
/// keeps it.
pub(crate) struct UniqueIndex<'a, 'ctx, K, T> {
    values: &'a [T],
    key: IndexKey<'a, K, T>,
    operation: &'static str,
    table: std::cell::OnceCell<(
        HashMap<K, Option<&'a T>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    )>,
}

impl<'a, 'ctx, K, T> UniqueIndex<'a, 'ctx, K, T>
where
    K: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost,
{
    pub(crate) fn new(values: &'a [T], key: IndexKey<'a, K, T>, operation: &'static str) -> Self {
        Self {
            values,
            key,
            operation,
            table: std::cell::OnceCell::new(),
        }
    }

    pub(crate) fn get<Q>(
        &self,
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        key: &Q,
    ) -> Result<Option<&'a T>, cadmpeg_core::CodecError>
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost + ?Sized,
    {
        let (table, _) = match self.table.get() {
            Some(table) => table,
            None => {
                let built = self.build(ctx)?;
                self.table.get_or_init(|| built)
            }
        };
        Ok(ctx
            .get_hash_map(table, key, self.operation)?
            .copied()
            .flatten())
    }

    fn build(
        &self,
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<
        (
            HashMap<K, Option<&'a T>>,
            cadmpeg_core::decode::ScopedReservation<'ctx>,
        ),
        cadmpeg_core::CodecError,
    > {
        let mut storage = ctx.reserve_scoped(0, self.operation)?;
        let mut table = HashMap::new();
        for value in ctx.admit_iter(self.values, self.operation)? {
            let Some(key) = (self.key)(ctx, value)? else {
                continue;
            };
            if let Some(indexed) = ctx.get_mut_hash_map(&mut table, &key, self.operation)? {
                *indexed = None;
                continue;
            }
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut table, key, Some(value), self.operation)
                    .map(|_| ())
            })?;
        }
        Ok((table, storage))
    }
}

type DesignScope = crate::records::feature::scope::DesignParameterScope;
type OperandGroup = crate::records::topology::construction::DesignConstructionOperandGroup;
type BodyRecipeOperand = crate::records::topology::body_recipe::DesignBodyRecipeOperand;

/// Design scopes by native ID. Each scope ID names one native record.
pub(crate) type ScopeIndex<'a, 'ctx> = UniqueIndex<'a, 'ctx, &'a str, DesignScope>;

pub(crate) fn scope_index<'a, 'ctx>(scopes: &'a [DesignScope]) -> ScopeIndex<'a, 'ctx> {
    UniqueIndex::new(
        scopes,
        |_, scope| Ok(Some(scope.id.as_str())),
        "index F3D feature scopes",
    )
}

/// Operand groups by native ID. Each group ID names one native record.
pub(crate) type GroupIndex<'a, 'ctx> = UniqueIndex<'a, 'ctx, &'a str, OperandGroup>;

pub(crate) fn group_index<'a, 'ctx>(groups: &'a [OperandGroup]) -> GroupIndex<'a, 'ctx> {
    UniqueIndex::new(
        groups,
        |_, group| Ok(Some(group.id.as_str())),
        "index F3D operand groups",
    )
}

/// Returns the group named `id` when it belongs to `scope`'s stream and record
/// and passes `accept`.
fn scope_group<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    groups: &GroupIndex<'a, 'ctx>,
    scope: &DesignScope,
    id: &str,
    accept: impl FnOnce(&OperandGroup) -> bool,
) -> Result<Option<&'a OperandGroup>, cadmpeg_core::CodecError> {
    let Some(group) = groups.get(ctx, id)? else {
        return Ok(None);
    };
    if group.scope_record_index != scope.record_index || !accept(group) {
        return Ok(None);
    }
    let same_stream = ctx.equal(
        &native_stream_of(ctx, &group.id)?,
        &native_stream_of(ctx, &scope.id)?,
        "compare F3D operand group stream",
    )?;
    Ok(same_stream.then_some(group))
}

/// Stream, owning group record, member ordinal and record of a group-owned
/// body recipe operand.
type MemberOperandKey<'a> = (Option<&'a str>, u32, u32, u32);
/// Stream, owning scope record and record of a scope-reference body recipe
/// operand.
type ScopeOperandKey<'a> = (Option<&'a str>, u32, u32);

fn member_operand_key<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &'a BodyRecipeOperand,
) -> Result<Option<MemberOperandKey<'a>>, cadmpeg_core::CodecError> {
    let Some((group_record_index, ordinal)) = operand.owner.group() else {
        return Ok(None);
    };
    Ok(Some((
        native_stream_of(ctx, &operand.id)?,
        group_record_index,
        ordinal,
        operand.record_index(),
    )))
}

fn scope_operand_key<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &'a BodyRecipeOperand,
) -> Result<Option<ScopeOperandKey<'a>>, cadmpeg_core::CodecError> {
    if !matches!(
        operand.owner,
        crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
    ) {
        return Ok(None);
    }
    Ok(Some((
        native_stream_of(ctx, &operand.id)?,
        operand.scope_record_index,
        operand.record_index(),
    )))
}

/// Scope-reference body recipe operands by stream, scope record and record.
pub(crate) type ScopeOperandIndex<'a, 'ctx> =
    UniqueIndex<'a, 'ctx, ScopeOperandKey<'a>, BodyRecipeOperand>;

pub(crate) fn scope_operand_index<'a, 'ctx>(
    operands: &'a [BodyRecipeOperand],
) -> ScopeOperandIndex<'a, 'ctx> {
    UniqueIndex::new(
        operands,
        scope_operand_key,
        "index F3D scope-reference body recipe operands",
    )
}

/// Body construction recipes by native ID.
pub(crate) type BodyRecipeIndex<'a, 'ctx> =
    UniqueIndex<'a, 'ctx, &'a str, crate::records::recipes::ConstructionRecipe>;

pub(crate) fn body_recipe_index<'a, 'ctx>(
    recipes: &'a [crate::records::recipes::ConstructionRecipe],
) -> BodyRecipeIndex<'a, 'ctx> {
    UniqueIndex::new(
        recipes,
        |_, recipe| {
            Ok(
                (recipe.kind == crate::records::recipes::ConstructionRecipeKind::Body)
                    .then_some(recipe.id.as_str()),
            )
        },
        "index F3D body construction recipes",
    )
}

/// Active bodies by identity and the body each shell face belongs to. A face
/// in several shells maps to the last shell whose region names a body, and a
/// repeated identity maps to its last record.
struct ExternalBodies<'a, 'ctx> {
    body_by_face: HashMap<&'a cadmpeg_ir::ids::FaceId, &'a cadmpeg_ir::ids::BodyId>,
    bodies: HashMap<&'a cadmpeg_ir::ids::BodyId, &'a cadmpeg_ir::topology::Body>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn external_bodies<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    bodies: &'a [cadmpeg_ir::topology::Body],
    regions: &'a [cadmpeg_ir::topology::Region],
    shells: &'a [cadmpeg_ir::topology::Shell],
) -> Result<ExternalBodies<'a, 'ctx>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "index F3D external bodies")?;
    let (body_by_region, _region_storage) =
        ctx.with_scoped_storage("index F3D external body regions", || {
            ctx.collect_hash_map(
                regions.iter().map(|region| (&region.id, &region.body)),
                "index F3D external body regions",
            )
        })?;
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
            storage.with_storage(|| {
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
    let bodies = storage.with_storage(|| {
        ctx.collect_hash_map(
            bodies.iter().map(|body| (&body.id, body)),
            "index F3D external body metadata",
        )
    })?;
    Ok(ExternalBodies {
        body_by_face,
        bodies,
        _storage: storage,
    })
}

/// Persistent design links by design identifier and reference, and whether
/// each link is its target's latest: the highest ordinal, the earliest link
/// among equal ordinals.
struct PersistentBodyLinks<'a, 'ctx> {
    by_design: HashMap<(&'a str, i64), Vec<usize>>,
    current: Vec<bool>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn link_target_key(target: &cadmpeg_ir::attributes::AttributeTarget) -> (u8, &str) {
    use cadmpeg_ir::attributes::AttributeTarget;
    match target {
        AttributeTarget::Document => (0, ""),
        AttributeTarget::Body(id) => (1, id.as_str()),
        AttributeTarget::Face(id) => (2, id.as_str()),
        AttributeTarget::Shell(id) => (3, id.as_str()),
        AttributeTarget::Loop(id) => (4, id.as_str()),
        AttributeTarget::Coedge(id) => (5, id.as_str()),
        AttributeTarget::Edge(id) => (6, id.as_str()),
        AttributeTarget::Vertex(id) => (7, id.as_str()),
    }
}

fn persistent_body_links<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    links: &'a [crate::records::sketch_links::PersistentDesignLink],
) -> Result<PersistentBodyLinks<'a, 'ctx>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "index F3D persistent body links")?;
    let mut latest_storage = ctx.reserve_scoped(0, "index F3D latest persistent links")?;
    let mut latest = HashMap::<(u8, &str), usize>::new();
    let mut by_design = HashMap::new();
    for (index, link) in ctx
        .admit_iter(links, "scan F3D persistent body links")?
        .enumerate()
    {
        let target = link_target_key(&link.target);
        match ctx.get_mut_hash_map(&mut latest, &target, "index F3D latest persistent links")? {
            Some(latest) => {
                if link.ordinal > links[*latest].ordinal {
                    *latest = index;
                }
            }
            None => latest_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut latest,
                    target,
                    index,
                    "index F3D latest persistent links",
                )
                .map(|_| ())
            })?,
        }
        storage.with_storage(|| {
            ctx.push_hash_group(
                &mut by_design,
                (link.design_id.as_str(), link.design_reference),
                index,
                "index F3D persistent body links",
                "group F3D persistent body links",
            )
        })?;
    }
    let mut current = storage
        .with_storage(|| ctx.collection_vec(links.len(), "mark F3D latest persistent links"))?;
    for (index, link) in ctx
        .admit_iter(links, "mark F3D latest persistent links")?
        .enumerate()
    {
        let target = link_target_key(&link.target);
        current.push(
            ctx.get_hash_map(&latest, &target, "find F3D latest persistent link")? == Some(&index),
        );
    }
    Ok(PersistentBodyLinks {
        by_design,
        current,
        _storage: storage,
    })
}

/// Body-selection inputs keyed for the per-feature lookups of one binding
/// pass. Each index is built on its first lookup and held as scratch while
/// this value lives.
pub(crate) struct BodySelectionIndex<'a, 'ctx> {
    inputs: &'a FeatureBodySelectionInputs<'a>,
    scopes: ScopeIndex<'a, 'ctx>,
    groups: GroupIndex<'a, 'ctx>,
    member_operands: UniqueIndex<'a, 'ctx, MemberOperandKey<'a>, BodyRecipeOperand>,
    scope_operands: ScopeOperandIndex<'a, 'ctx>,
    recipes: BodyRecipeIndex<'a, 'ctx>,
    external: std::cell::OnceCell<ExternalBodies<'a, 'ctx>>,
    links: std::cell::OnceCell<PersistentBodyLinks<'a, 'ctx>>,
}

impl<'a, 'ctx> BodySelectionIndex<'a, 'ctx> {
    pub(crate) fn new(inputs: &'a FeatureBodySelectionInputs<'a>) -> Self {
        Self {
            inputs,
            scopes: scope_index(inputs.scopes),
            groups: group_index(inputs.groups),
            member_operands: UniqueIndex::new(
                inputs.body_recipe_operands,
                member_operand_key,
                "index F3D group body recipe operands",
            ),
            scope_operands: scope_operand_index(inputs.body_recipe_operands),
            recipes: body_recipe_index(inputs.construction_recipes),
            external: std::cell::OnceCell::new(),
            links: std::cell::OnceCell::new(),
        }
    }

    fn external(
        &self,
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<&ExternalBodies<'a, 'ctx>, cadmpeg_core::CodecError> {
        if let Some(external) = self.external.get() {
            return Ok(external);
        }
        let built = external_bodies(
            ctx,
            self.inputs.bodies,
            self.inputs.regions,
            self.inputs.shells,
        )?;
        Ok(self.external.get_or_init(|| built))
    }

    fn links(
        &self,
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<&PersistentBodyLinks<'a, 'ctx>, cadmpeg_core::CodecError> {
        if let Some(links) = self.links.get() {
            return Ok(links);
        }
        let built = persistent_body_links(ctx, self.inputs.persistent_design_links)?;
        Ok(self.links.get_or_init(|| built))
    }
}

pub(crate) fn bind_feature_body_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    let histories = inputs.histories;
    let index = BodySelectionIndex::new(inputs);
    let mut feature_states = FeatureHistoryStates::new(ctx)?;

    bind_pattern_body_selections(ctx, features, &index)?;
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

    for feature in ctx.admit_iter(features, "scan F3D body selection features")? {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let dependencies = &feature.dependencies;
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            'feature_edit: {
                let Some(native_ref) = native_ref else {
                    break 'feature_edit;
                };
                let scope = match index.scopes.get(ctx, native_ref) {
                    Ok(Some(scope)) => scope,
                    Ok(None) => break 'feature_edit,
                    Err(error) => {
                        edit_result = Err(error);
                        break 'feature_edit;
                    }
                };
                if matches!(
                    definition,
                    FeatureDefinition::Operation(FeatureOperation::Pattern { .. })
                ) {
                    break 'feature_edit;
                }
                if let FeatureDefinition::Operation(FeatureOperation::BoundaryFill {
                    tools,
                    cells,
                }) = definition
                {
                    if let Some(previous_state_id) = scope.previous_history_state_id() {
                        edit_result = bind_body_recipe_body_selection(
                            ctx,
                            tools,
                            feature_id,
                            previous_state_id,
                            scope,
                            &index,
                        );
                        for cell in cells {
                            if edit_result.is_err() {
                                break;
                            }
                            edit_result = bind_body_recipe_body_selection(
                                ctx,
                                cell,
                                feature_id,
                                previous_state_id,
                                scope,
                                &index,
                            );
                        }
                    } else {
                        edit_result =
                            bind_direct_body_recipe_body_selection(ctx, tools, scope, &index);
                        for cell in cells {
                            if edit_result.is_err() {
                                break;
                            }
                            edit_result =
                                bind_direct_body_recipe_body_selection(ctx, cell, scope, &index);
                        }
                    }
                    break 'feature_edit;
                }
                if let FeatureDefinition::Operation(FeatureOperation::Combine {
                    operands, ..
                }) = definition
                {
                    let edit = operands.try_edit(|target, tools| {
                        macro_rules! admitted {
                            ($value:expr) => {
                                match $value {
                                    Ok(value) => value,
                                    Err(error) => {
                                        edit_result = Err(error);
                                        return;
                                    }
                                }
                            };
                        }
                        let (Some(state_id), Some(previous_state_id)) =
                            (scope.history_state_id(), scope.previous_history_state_id())
                        else {
                            edit_result =
                                bind_direct_body_recipe_body_selection(ctx, target, scope, &index);
                            if edit_result.is_err() {
                                return;
                            }
                            edit_result =
                                bind_direct_body_recipe_body_selection(ctx, tools, scope, &index);
                            if edit_result.is_err() {
                                return;
                            }
                            if matches!(
                                tools,
                                BodySelection::Native(_) | BodySelection::NativeSet(_)
                            ) {
                                if let Some(local) =
                                    admitted!(combine_external_local_tools(ctx, scope))
                                {
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
                        let Some((history, state, _)) = admitted!(unique_history_state_pair(
                            ctx,
                            histories,
                            state_id,
                            previous_state_id,
                        )) else {
                            return;
                        };
                        let history_states = match feature_states.states_of(ctx, history) {
                            Ok(Some(states)) => states,
                            Ok(None) => return,
                            Err(error) => {
                                edit_result = Err(error);
                                return;
                            }
                        };
                        let body = match singleton_revised_input_body_across_state_chain(
                            ctx,
                            state,
                            previous_state_id,
                            history_states,
                        ) {
                            Ok(Some(body)) => body,
                            Ok(None) => return,
                            Err(error) => {
                                edit_result = Err(error);
                                return;
                            }
                        };
                        let input_state = admitted!(crate::ids::history_input_state_id_charged(
                            ctx,
                            feature_id,
                            previous_state_id
                        ));
                        let body_id = admitted!(crate::ids::history_input_body_id_charged(
                            ctx,
                            feature_id,
                            previous_state_id,
                            body
                        ));
                        let native_id = admitted!(
                            ctx.copy_retained_text(native, "copy F3D Combine target identity")
                        );
                        let mut target_bodies =
                            admitted!(ctx.collection_vec(1, "validate F3D Combine target body"));
                        target_bodies.push(body_id);
                        if let Ok(historical) =
                            admitted!(cadmpeg_ir::features::BodySelection::historical(
                                admitted!(crate::ids::history_input_state_id_charged(
                                    ctx,
                                    feature_id,
                                    previous_state_id
                                )),
                                target_bodies,
                                native_id,
                                ctx
                            )
                            .map_err(cadmpeg_core::CodecError::from))
                        {
                            *target = historical;
                        }
                        let Some(stream) = admitted!(native_stream_of(ctx, &scope.id)) else {
                            return;
                        };
                        let Some(operation) = scope.combine_operation() else {
                            return;
                        };
                        let mut native_tools = Vec::new();
                        for tool in admitted!(ctx
                            .admit_iter(
                                std::slice::from_ref(&operation.tools.first),
                                "scan F3D operation tools",
                            )
                            .map_err(cadmpeg_core::CodecError::ResourceLimit))
                        .chain(admitted!(ctx
                            .admit_iter(&operation.tools.additional, "scan F3D operation tools",)
                            .map_err(cadmpeg_core::CodecError::ResourceLimit)))
                        {
                            let native = admitted!(ctx.format_retained(
                                format_args!("{stream}:design-record#{}", tool.record_index),
                                "retain F3D Combine tool identity"
                            ));
                            admitted!(ctx.reserve_vec(
                                &mut native_tools,
                                1,
                                "collect F3D Combine tool identities"
                            ));
                            native_tools.push(native);
                        }
                        let current_history_source =
                            admitted!(historical_brep_source(ctx, &state.id));
                        let mut historical_tool_rows = Vec::new();
                        let mut direct_tool_rows = Vec::new();
                        // Tool rows repeat when their slot or body repeats; a
                        // historical body identity is injective in its slot.
                        let mut tool_bodies_storage =
                            admitted!(ctx.reserve_scoped(0, "index F3D Combine tool bodies"));
                        let mut historical_tool_slots = HashSet::new();
                        let mut direct_tool_bodies = HashSet::new();
                        let native_tool_source = admitted!(ctx
                            .admit_iter(&native_tools, "scan F3D Combine tool identities")
                            .map_err(cadmpeg_core::CodecError::ResourceLimit));
                        for (record_index, native) in operation
                            .tools
                            .iter()
                            .map(|tool| tool.record_index)
                            .zip(native_tool_source)
                        {
                            let Some(operand) = admitted!(index
                                .scope_operands
                                .get(ctx, &(Some(stream), scope.record_index, record_index),))
                            else {
                                historical_tool_rows.clear();
                                direct_tool_rows.clear();
                                break;
                            };
                            if let Some(slot) = operand.resolved_body_slot {
                                let repeated = !admitted!(tool_bodies_storage.with_storage(|| {
                                    ctx.insert_hash_set(
                                        &mut historical_tool_slots,
                                        slot,
                                        "index F3D Combine historical tool slots",
                                    )
                                }));
                                let body = admitted!(crate::ids::history_input_body_id_charged(
                                    ctx,
                                    feature_id,
                                    previous_state_id,
                                    slot
                                ));
                                let row = admitted!(body_member(
                                    ctx,
                                    body,
                                    admitted!(ctx.copy_retained_text(
                                        native,
                                        "copy F3D Combine tool member identity"
                                    ))
                                )
                                .map_err(cadmpeg_core::CodecError::from));
                                let (false, Some(row)) = (repeated, row) else {
                                    historical_tool_rows.clear();
                                    direct_tool_rows.clear();
                                    break;
                                };
                                admitted!(ctx.reserve_vec(
                                    &mut historical_tool_rows,
                                    1,
                                    "collect F3D Combine historical tool rows"
                                ));
                                historical_tool_rows.push(row);
                                continue;
                            }
                            let external = admitted!(index.external(ctx));
                            let body = match unique_external_body_candidate(
                                ctx,
                                operand,
                                current_history_source,
                                external,
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
                            let repeated = !admitted!(tool_bodies_storage.with_storage(|| {
                                ctx.insert_hash_set(
                                    &mut direct_tool_bodies,
                                    body,
                                    "index F3D Combine direct tool bodies",
                                )
                            }));
                            let body = admitted!(
                                body.try_clone_for_decode(ctx, "copy F3D Combine direct tool body")
                            );
                            let row = admitted!(body_member(
                                ctx,
                                body,
                                admitted!(ctx.copy_retained_text(
                                    native,
                                    "copy F3D Combine direct tool member identity"
                                ))
                            )
                            .map_err(cadmpeg_core::CodecError::from));
                            let (false, Some(row)) = (repeated, row) else {
                                historical_tool_rows.clear();
                                direct_tool_rows.clear();
                                break;
                            };
                            admitted!(ctx.reserve_vec(
                                &mut direct_tool_rows,
                                1,
                                "collect F3D Combine direct tool rows"
                            ));
                            direct_tool_rows.push(row);
                        }
                        if historical_tool_rows.len() == native_tools.len() {
                            let Ok(members) =
                                admitted!(cadmpeg_ir::features::BodyMembers::try_from_rows(
                                    historical_tool_rows,
                                    ctx
                                )
                                .map_err(cadmpeg_core::CodecError::from))
                            else {
                                return;
                            };
                            *tools = BodySelection::HistoricalSet {
                                state: input_state,
                                members,
                            };
                        } else if direct_tool_rows.len() == native_tools.len() {
                            *tools = if direct_tool_rows.len() == 1 {
                                let Some(row) = direct_tool_rows.pop() else {
                                    return;
                                };
                                let (body, native) = admitted!(row.into_parts(ctx));
                                let mut selected = admitted!(
                                    ctx.collection_vec(1, "validate F3D Combine resolved body")
                                );
                                selected.push(body);
                                let bodies = match cadmpeg_ir::features::DistinctMembers::try_from(
                                    selected, ctx,
                                ) {
                                    Ok(value) => value,
                                    Err(cadmpeg_ir::features::FeatureCollectionError::Invalid(
                                        _,
                                    )) => return,
                                    Err(
                                        cadmpeg_ir::features::FeatureCollectionError::Resource(
                                            limit,
                                        ),
                                    ) => {
                                        edit_result = Err(limit.into());
                                        return;
                                    }
                                };
                                BodySelection::Resolved { bodies, native }
                            } else {
                                let Ok(members) =
                                    admitted!(cadmpeg_ir::features::BodyMembers::try_from_rows(
                                        direct_tool_rows,
                                        ctx
                                    )
                                    .map_err(cadmpeg_core::CodecError::from))
                                else {
                                    return;
                                };
                                BodySelection::ResolvedSet { members }
                            };
                        } else {
                            let tool_record_indices = admitted!(ctx.collect_vec(
                                operation.tools.iter().map(|tool| tool.record_index),
                                "collect F3D Combine tool record indices"
                            ));
                            if let Some(tool_slots) = admitted!(combine_recipe_family_tool_slots(
                                ctx,
                                (stream, scope.record_index),
                                &tool_record_indices,
                                previous_state_id,
                                body,
                                &index.scope_operands,
                                &index.recipes,
                            )) {
                                if tool_slots.len() != native_tools.len() {
                                    return;
                                }
                                let Some(rows) = admitted!(combine_historical_rows(
                                    ctx,
                                    feature_id,
                                    previous_state_id,
                                    tool_slots,
                                    native_tools
                                )) else {
                                    return;
                                };
                                let Ok(members) = admitted!(
                                    cadmpeg_ir::features::BodyMembers::try_from_rows(rows, ctx)
                                        .map_err(cadmpeg_core::CodecError::from)
                                ) else {
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
                            for dependency in admitted!(ctx
                                .admit_iter(
                                    dependencies.as_slice(),
                                    "scan F3D pattern body dependencies",
                                )
                                .map_err(cadmpeg_core::CodecError::ResourceLimit))
                            {
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
                                            ctx,
                                            feature_id,
                                            previous_state_id,
                                            tool_slots,
                                            native_tools
                                        )) else {
                                            return;
                                        };
                                        let Ok(members) = admitted!(
                                            cadmpeg_ir::features::BodyMembers::try_from_rows(
                                                rows, ctx
                                            )
                                            .map_err(cadmpeg_core::CodecError::from)
                                        ) else {
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
                            &index,
                        );
                    } else {
                        edit_result =
                            bind_direct_body_recipe_body_selection(ctx, targets, scope, &index);
                    }
                    break 'feature_edit;
                }
                if let FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                    bodies, ..
                }) = definition
                {
                    if let Some(previous_state_id) = scope.previous_history_state_id() {
                        edit_result = bind_body_recipe_body_selection(
                            ctx,
                            bodies,
                            feature_id,
                            previous_state_id,
                            scope,
                            &index,
                        );
                    } else {
                        edit_result =
                            bind_direct_body_recipe_body_selection(ctx, bodies, scope, &index);
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
                            &index,
                        );
                        if edit_result.is_ok() && matches!(bodies, BodySelection::Native(_)) {
                            edit_result =
                                bind_direct_body_recipe_body_selection(ctx, bodies, scope, &index);
                        }
                    } else {
                        edit_result =
                            bind_direct_body_recipe_body_selection(ctx, bodies, scope, &index);
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
                    FeatureDefinition::Operation(FeatureOperation::SplitBody {
                        targets, ..
                    }) => (targets, BodySelectionProof::RevisedInput),
                    _ => break 'feature_edit,
                };
                let BodySelection::Native(group_id) = bodies else {
                    break 'feature_edit;
                };
                let group = match scope_group(ctx, &index.groups, scope, group_id, |group| {
                    group.role() == DesignOperandRole::BODIES_A
                }) {
                    Ok(Some(group)) => group,
                    Ok(None) => break 'feature_edit,
                    Err(error) => {
                        edit_result = Err(error);
                        break 'feature_edit;
                    }
                };
                if group.members().len() != 1 {
                    break 'feature_edit;
                }
                let (Some(state_id), Some(previous_state_id)) =
                    (scope.history_state_id(), scope.previous_history_state_id())
                else {
                    edit_result =
                        bind_direct_body_recipe_body_selection(ctx, bodies, scope, &index);
                    break 'feature_edit;
                };
                let Some((history, state, _previous)) =
                    (match unique_history_state_pair(ctx, histories, state_id, previous_state_id) {
                        Ok(pair) => pair,
                        Err(error) => {
                            edit_result = Err(error);
                            break 'feature_edit;
                        }
                    })
                else {
                    edit_result =
                        bind_direct_body_recipe_body_selection(ctx, bodies, scope, &index);
                    break 'feature_edit;
                };
                let history_states = match feature_states.states_of(ctx, history) {
                    Ok(Some(states)) => states,
                    Ok(None) => break 'feature_edit,
                    Err(error) => {
                        edit_result = Err(error);
                        break 'feature_edit;
                    }
                };
                let body = match proof {
                    BodySelectionProof::TopologyStableRevision => {
                        singleton_body_revision_across_state_chain(
                            ctx,
                            state,
                            previous_state_id,
                            history_states,
                        )
                    }
                    BodySelectionProof::RevisedInput => {
                        singleton_revised_input_body_across_state_chain(
                            ctx,
                            state,
                            previous_state_id,
                            history_states,
                        )
                    }
                };
                let body = match body {
                    Ok(body) => body,
                    Err(error) => {
                        edit_result = Err(error);
                        break 'feature_edit;
                    }
                };
                let Some(body) = body else {
                    break 'feature_edit;
                };
                let state_id =
                    crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id);
                let body_id = crate::ids::history_input_body_id_charged(
                    ctx,
                    feature_id,
                    previous_state_id,
                    body,
                );
                let native =
                    ctx.copy_retained_text(group_id, "copy F3D pattern body group identity");
                let (state_id, body_id, native) = match (state_id, body_id, native) {
                    (Ok(state_id), Ok(body_id), Ok(native)) => (state_id, body_id, native),
                    (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
                        edit_result = Err(error);
                        break 'feature_edit;
                    }
                };
                let mut selected =
                    match ctx.collection_vec(1, "validate F3D pattern body selection") {
                        Ok(selected) => selected,
                        Err(error) => {
                            edit_result = Err(error);
                            break 'feature_edit;
                        }
                    };
                selected.push(body_id);
                match cadmpeg_ir::features::BodySelection::historical(
                    state_id, selected, native, ctx,
                ) {
                    Ok(Ok(historical)) => *bodies = historical,
                    Ok(Err(_)) => {}
                    Err(limit) => {
                        edit_result = Err(limit.into());
                        break 'feature_edit;
                    }
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
    if pattern_bodies.len().checked_sub(1) != Some(native_tool_count)
        || !ctx.contains_btree_set(
            pattern_bodies,
            &target_body,
            "find F3D pattern Combine target",
        )?
    {
        return Ok(None);
    }
    let mut tools = ctx.collection_vec(native_tool_count, "collect F3D pattern Combine tools")?;
    for &body in ctx.admit_iter(pattern_bodies, "scan F3D pattern Combine bodies")? {
        if body != target_body {
            tools.push(body);
        }
    }
    Ok(Some(tools))
}

fn combine_recipe_family_tool_slots<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    scope_key: (&'a str, u32),
    tool_record_indices: &[u32],
    previous_state_id: i64,
    target_body: i64,
    operands: &ScopeOperandIndex<'a, 'ctx>,
    recipes: &BodyRecipeIndex<'a, 'ctx>,
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
    let mut seen_storage = ctx.reserve_scoped(0, "index F3D Combine tool record indices")?;
    let mut seen = HashSet::new();
    for &index in ctx.admit_iter(tool_record_indices, "scan F3D Combine tool record indices")? {
        if !seen_storage.with_storage(|| {
            ctx.insert_hash_set(&mut seen, index, "index F3D Combine tool record indices")
        })? {
            return Ok(None);
        }
    }
    let mut families_storage = ctx.reserve_scoped(0, "index F3D Combine tool families")?;
    let mut families = BTreeMap::<FamilyKey<'_>, Vec<FamilyMember<'_>>>::new();
    for &record_index in
        ctx.admit_iter(tool_record_indices, "scan F3D Combine tool record indices")?
    {
        let operand =
            required!(operands.get(ctx, &(Some(stream), scope_record_index, record_index))?);
        let [reference] = operand.references().as_slice() else {
            return Ok(None);
        };
        let recipe = required!(recipes.get(ctx, operand.recipe_id.as_str())?);
        if !ctx.equal(
            &native_stream_of(ctx, &recipe.id)?,
            &Some(stream),
            "compare F3D Combine recipe streams",
        )? {
            return Ok(None);
        }
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
        let key = (
            &operand.asset_id,
            &operand.context_id,
            reference.design_reference,
            reference.form,
            design.id.value.as_str(),
        );
        families_storage.with_storage(|| {
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
            )
        })?;
    }

    let mut selected_storage = ctx.reserve_scoped(0, "index F3D Combine selected tools")?;
    let mut selected = BTreeSet::new();
    for (_, family) in ctx.admit_iter(&families, "scan F3D Combine tool families")? {
        let mut family_storage = ctx.reserve_scoped(0, "index F3D Combine family bodies")?;
        let mut exact = BTreeSet::new();
        let mut all_exact = true;
        for (_, body, _) in ctx.admit_iter(family, "scan F3D Combine exact tool bodies")? {
            let Some(body) = *body else {
                all_exact = false;
                continue;
            };
            ctx.insert_scoped_btree_set(
                &mut family_storage,
                &mut exact,
                body,
                "find F3D Combine exact tool body",
                "index F3D Combine exact tool bodies",
            )?;
        }
        if all_exact {
            if exact.len() != family.len() {
                return Ok(None);
            }
            for body in ctx.admit_iter(exact, "scan F3D Combine exact tool bodies")? {
                ctx.insert_scoped_btree_set(
                    &mut selected_storage,
                    &mut selected,
                    body,
                    "find F3D Combine selected tool",
                    "index F3D Combine selected tools",
                )?;
            }
            continue;
        }
        // Distinct selectors covering 1..=n are exactly n distinct values in
        // that range.
        let count = required!(u32::try_from(family.len()).ok());
        let mut selectors = BTreeSet::new();
        for &(selector, _, _) in ctx.admit_iter(family, "scan F3D family")? {
            if selector == 0
                || selector > count
                || !ctx.insert_scoped_btree_set(
                    &mut family_storage,
                    &mut selectors,
                    selector,
                    "find F3D Combine selector",
                    "index F3D Combine selectors",
                )?
            {
                return Ok(None);
            }
        }
        let Some(first) = ctx.position_by(
            family,
            |(_, _, candidates)| Ok(!candidates.is_empty()),
            "find F3D Combine candidate tools",
        )?
        else {
            return Ok(None);
        };
        let mut candidates = BTreeSet::new();
        for &candidate in ctx.admit_iter(family[first].2, "scan F3D Combine candidate tools")? {
            ctx.insert_scoped_btree_set(
                &mut family_storage,
                &mut candidates,
                candidate,
                "find F3D Combine candidate tool",
                "index F3D Combine candidate tools",
            )?;
        }
        if candidates.len() != family.len()
            || ctx.contains_btree_set(&candidates, &target_body, "find F3D Combine target tool")?
            || !ctx.is_subset_btree_set(&exact, &candidates, "check F3D Combine exact tools")?
        {
            return Ok(None);
        }
        // Every other non-empty candidate list must name exactly this set.
        let mismatched = ctx.any_by(
            &family[first + 1..],
            |(_, _, other)| {
                if other.is_empty() {
                    return Ok(false);
                }
                let (other, _other_storage) =
                    ctx.with_scoped_storage("index F3D Combine other candidate tools", || {
                        ctx.collect_btree_set(
                            other.iter().copied(),
                            "index F3D Combine other candidate tools",
                        )
                    })?;
                Ok(other.len() != candidates.len()
                    || !ctx.is_subset_btree_set(
                        &other,
                        &candidates,
                        "compare F3D Combine candidate tools",
                    )?)
            },
            "scan F3D Combine candidate tool lists",
        )?;
        if mismatched {
            return Ok(None);
        }
        for body in ctx.admit_iter(candidates, "scan F3D Combine candidate tools")? {
            ctx.insert_scoped_btree_set(
                &mut selected_storage,
                &mut selected,
                body,
                "find F3D Combine selected tool",
                "index F3D Combine selected tools",
            )?;
        }
    }
    if selected.len() != tool_record_indices.len()
        || ctx.contains_btree_set(&selected, &target_body, "find F3D Combine target tool")?
    {
        return Ok(None);
    }
    let mut slots = ctx.collection_vec(selected.len(), "collect F3D Combine recipe tool slots")?;
    for body in ctx.admit_iter(selected, "collect F3D Combine recipe tool slots")? {
        slots.push(body);
    }
    Ok(Some(slots))
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

fn bind_pattern_body_selections<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    index: &BodySelectionIndex<'a, 'ctx>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{
        patterns::PatternSeed, BodySelection, FeatureDefinition, FeatureOperation,
    };

    // The non-empty second-body group of each scope, by stream and scope record.
    let seed_groups = UniqueIndex::new(
        index.inputs.groups,
        |ctx, group| {
            if group.role() != DesignOperandRole::BODIES_B || group.members().is_empty() {
                return Ok(None);
            }
            Ok(Some((
                native_stream_of(ctx, &group.id)?,
                group.scope_record_index,
            )))
        },
        "index F3D pattern seed groups",
    );
    for feature in ctx.admit_iter(features, "scan F3D pattern body features")? {
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(scope) = index.scopes.get(ctx, native_ref)? else {
            continue;
        };
        let stream = native_stream_of(ctx, &scope.id)?;
        let Some(group) = seed_groups.get(ctx, &(stream, scope.record_index))? else {
            continue;
        };
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
                    index,
                )
                .err();
            } else {
                reserve_error =
                    bind_direct_body_recipe_body_selection(ctx, selection, scope, index).err();
            }
        });
        if let Some(error) = reserve_error {
            return Err(error);
        }
    }
    Ok(())
}

/// The one active body that every operand reference's candidate faces share,
/// preferring displayed bodies when any candidate is displayed. Bodies under
/// the current history's B-rep prefix are not external candidates.
fn unique_external_body_candidate<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &BodyRecipeOperand,
    current_history_source: Option<&str>,
    external: &ExternalBodies<'a, '_>,
) -> Result<Option<&'a cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
    let (current_prefix, _prefix_storage) = match current_history_source {
        Some(source) => {
            let (prefix, storage) =
                ctx.with_scoped_storage("retain F3D current history prefix", || {
                    ctx.format_retained(
                        format_args!("f3d:brep/{source}/"),
                        "retain F3D current history prefix",
                    )
                })?;
            (Some(prefix), Some(storage))
        }
        None => (None, None),
    };
    let mut candidates_storage = ctx.reserve_scoped(0, "collect F3D external body candidates")?;
    let mut candidates: Option<BTreeSet<&'a cadmpeg_ir::ids::BodyId>> = None;
    for reference in ctx.admit_iter(operand.references(), "scan F3D external body references")? {
        let mut reference_candidates = BTreeSet::new();
        for face in ctx.admit_iter(
            &reference.candidate_faces,
            "scan F3D reference candidate faces",
        )? {
            let Some(&body) =
                ctx.get_hash_map(&external.body_by_face, face, "find F3D external face body")?
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
            ctx.insert_scoped_btree_set(
                &mut candidates_storage,
                &mut reference_candidates,
                body,
                "check F3D external body candidate",
                "collect F3D external body candidates",
            )?;
        }
        if let Some(candidates) = &mut candidates {
            if reference_candidates.is_empty() {
                return Ok(None);
            }
            ctx.retain_btree_set(
                candidates,
                |body| {
                    ctx.contains_btree_set(
                        &reference_candidates,
                        body,
                        "intersect F3D external body candidates",
                    )
                },
                "intersect F3D external body candidates",
            )?;
        } else {
            candidates = Some(reference_candidates);
        }
    }
    let Some(candidates) = candidates else {
        return Ok(None);
    };
    let mut displayed = None;
    let mut displayed_count = 0_usize;
    for &body in ctx.admit_iter(&candidates, "scan F3D displayed external body candidates")? {
        let Some(metadata) =
            ctx.get_hash_map(&external.bodies, body, "find F3D external body metadata")?
        else {
            continue;
        };
        if metadata.visible == Some(true) {
            displayed = displayed.or(Some(body));
            displayed_count += 1;
        }
    }
    Ok(match displayed_count {
        0 if candidates.len() == 1 => candidates.first().copied(),
        1 => displayed,
        _ => None,
    })
}

fn bind_body_recipe_body_selection<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::BodySelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &'a DesignScope,
    index: &BodySelectionIndex<'a, 'ctx>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::BodySelection;

    let BodySelection::Native(group_id) = selection else {
        return Ok(());
    };
    let Some(group) = scope_group(ctx, &index.groups, scope, group_id, |group| {
        matches!(
            group.role(),
            DesignOperandRole::BODIES_A | DesignOperandRole::ROLE_0X5 | DesignOperandRole::BODIES_B
        )
    })?
    else {
        return Ok(());
    };
    if group.members().is_empty() {
        return Ok(());
    }
    let stream = native_stream_of(ctx, &scope.id)?;
    let mut slots_storage = ctx.reserve_scoped(0, "collect F3D body recipe slots")?;
    let mut seen = HashSet::new();
    let mut body_slots = Vec::new();
    for (ordinal, record_index) in ctx
        .admit_iter(group.members(), "scan F3D body recipe group members")?
        .map(|member| member.value)
        .enumerate()
    {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return Ok(());
        };
        let Some(operand) = index
            .member_operands
            .get(ctx, &(stream, group.record_index, ordinal, record_index))?
        else {
            return Ok(());
        };
        let Some(body_slot) = operand.resolved_body_slot else {
            return Ok(());
        };
        slots_storage.with_storage(|| {
            if ctx.insert_hash_set(&mut seen, body_slot, "collect F3D body recipe slots")? {
                ctx.push_vec(&mut body_slots, body_slot, "collect F3D body recipe slots")?;
            }
            Ok::<(), cadmpeg_core::CodecError>(())
        })?;
    }

    let mut body_ids =
        ctx.collection_vec(body_slots.len(), "collect F3D body recipe identities")?;
    for &slot in ctx.admit_iter(&body_slots, "scan F3D body recipe slots")? {
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

fn bind_direct_body_recipe_body_selection<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::BodySelection,
    scope: &'a DesignScope,
    index: &BodySelectionIndex<'a, 'ctx>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::BodySelection;

    let stream = native_stream_of(ctx, &scope.id)?;
    let mut seen_storage = ctx.reserve_scoped(0, "index F3D direct body recipe selections")?;
    let mut seen_bodies = HashSet::new();
    let native_members = match selection {
        BodySelection::Native(group_id) => {
            let Some(group) = scope_group(ctx, &index.groups, scope, group_id, |group| {
                matches!(
                    group.role(),
                    DesignOperandRole::BODIES_A
                        | DesignOperandRole::ROLE_0X5
                        | DesignOperandRole::BODIES_B
                )
            })?
            else {
                return Ok(());
            };
            if group.members().is_empty() {
                return Ok(());
            }
            let mut selected = ctx.collection_vec(
                group.members().len(),
                "collect F3D direct body recipe selections",
            )?;
            for (ordinal, record_index) in ctx
                .admit_iter(group.members(), "scan F3D direct body recipe group members")?
                .map(|member| member.value)
                .enumerate()
            {
                let Ok(ordinal) = u32::try_from(ordinal) else {
                    return Ok(());
                };
                let Some(operand) = index
                    .member_operands
                    .get(ctx, &(stream, group.record_index, ordinal, record_index))?
                else {
                    return Ok(());
                };
                let Some(body) = direct_body_recipe_candidate(ctx, operand, index)? else {
                    return Ok(());
                };
                if !seen_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut seen_bodies,
                        body,
                        "index F3D direct body recipe selections",
                    )
                })? {
                    return Ok(());
                }
                selected
                    .push(body.try_clone_for_decode(ctx, "copy F3D direct body recipe selection")?);
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
        BodySelection::NativeSet(native) => native.as_slice(),
        _ => return Ok(()),
    };
    if native_members.is_empty() {
        return Ok(());
    }
    let mut members_storage =
        ctx.reserve_scoped(0, "index F3D direct body recipe native members")?;
    let mut members = HashSet::new();
    for native in ctx.admit_iter(native_members, "scan F3D direct body recipe native members")? {
        if !members_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut members,
                native.as_str(),
                "index F3D direct body recipe native members",
            )
        })? {
            return Ok(());
        }
    }
    let Some(stream) = stream else {
        return Ok(());
    };
    let mut rows =
        ctx.collection_vec(native_members.len(), "collect F3D direct body recipe rows")?;
    for native in ctx.admit_iter(native_members, "scan F3D native members")? {
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
        if !ctx.equal(
            native_stream_name,
            stream,
            "compare F3D direct body recipe stream identity",
        )? {
            return Ok(());
        }
        let Some(operand) = index
            .scope_operands
            .get(ctx, &(Some(stream), scope.record_index, record_index))?
        else {
            return Ok(());
        };
        let Some(body) = direct_body_recipe_candidate(ctx, operand, index)? else {
            return Ok(());
        };
        if !seen_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut seen_bodies,
                body,
                "index F3D direct body recipe selections",
            )
        })? {
            return Ok(());
        }
        let body = body.try_clone_for_decode(ctx, "copy F3D direct body recipe member body")?;
        let native =
            ctx.copy_retained_text(native, "copy F3D direct body recipe member identity")?;
        let Some(row) = body_member(ctx, body, native)? else {
            return Ok(());
        };
        rows.push(row);
    }
    if let Ok(members) = cadmpeg_ir::features::BodyMembers::try_from_rows(rows, ctx)? {
        *selection = BodySelection::ResolvedSet { members };
    }
    Ok(())
}

fn direct_body_recipe_candidate<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    operand: &BodyRecipeOperand,
    index: &BodySelectionIndex<'a, 'ctx>,
) -> Result<Option<&'a cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
    let external = index.external(ctx)?;
    if let Some(body) = body_recipe_link_candidate(ctx, operand, index)? {
        let (any_candidate, contains_body) =
            body_recipe_face_body_candidates(ctx, operand, body, external)?;
        if !any_candidate || contains_body {
            return Ok(Some(body));
        }
        return Ok(None);
    }
    unique_external_body_candidate(ctx, operand, None, external)
}

/// The active body that the operand's body recipe names through its latest
/// persistent design links, when every such link names the same body.
fn body_recipe_link_candidate<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    operand: &BodyRecipeOperand,
    index: &BodySelectionIndex<'a, 'ctx>,
) -> Result<Option<&'a cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let Some(stream) = native_stream_of(ctx, &operand.id)? else {
        return Ok(None);
    };
    let Some(recipe) = index.recipes.get(ctx, operand.recipe_id.as_str())? else {
        return Ok(None);
    };
    if !ctx.equal(
        &native_stream_of(ctx, &recipe.id)?,
        &Some(stream),
        "compare F3D body recipe stream",
    )? {
        return Ok(None);
    }
    let Some(design) = recipe.design.as_ref() else {
        return Ok(None);
    };
    let Some(selector) = design.selector else {
        return Ok(None);
    };
    let links = index.links(ctx)?;
    let Some(candidates) = ctx.get_hash_map(
        &links.by_design,
        &(design.id.value.as_str(), i64::from(selector.value)),
        "find F3D persistent body links",
    )?
    else {
        return Ok(None);
    };
    let external = index.external(ctx)?;
    let mut matching_body: Option<&'a cadmpeg_ir::ids::BodyId> = None;
    for &link in ctx.admit_iter(candidates, "resolve F3D persistent body link")? {
        if !links.current[link] {
            continue;
        }
        let AttributeTarget::Body(body) = &index.inputs.persistent_design_links[link].target else {
            continue;
        };
        if !ctx.contains_key_hash_map(
            &external.bodies,
            body,
            "find F3D persistent body candidate",
        )? {
            continue;
        }
        if let Some(existing) = matching_body {
            if !ctx.equal(existing, body, "compare F3D selected persistent body")? {
                return Ok(None);
            }
        }
        matching_body = Some(body);
    }
    Ok(matching_body)
}

/// Whether any candidate face of the operand belongs to an active body, and
/// whether one of those bodies is `selected`.
fn body_recipe_face_body_candidates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &BodyRecipeOperand,
    selected: &cadmpeg_ir::ids::BodyId,
    external: &ExternalBodies<'_, '_>,
) -> Result<(bool, bool), cadmpeg_core::CodecError> {
    let mut any_candidate = false;
    let mut contains_selected = false;
    for reference in ctx.admit_iter(operand.references(), "scan F3D body recipe references")? {
        for face in ctx.admit_iter(
            &reference.candidate_faces,
            "scan F3D body recipe candidate faces",
        )? {
            let Some(&body) = ctx.get_hash_map(
                &external.body_by_face,
                face,
                "resolve F3D body recipe face carrier",
            )?
            else {
                continue;
            };
            if ctx.contains_key_hash_map(
                &external.bodies,
                body,
                "resolve F3D body recipe face carrier",
            )? {
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

type HistoryStates<'a> = HashMap<i64, Option<&'a AsmDeltaState>>;

/// A history's states by ID; a repeated ID names no state.
fn unique_feature_history_states<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    history: &'a AsmHistory,
) -> Result<
    (
        HistoryStates<'a>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    cadmpeg_core::CodecError,
> {
    ctx.unique_index(
        history.states.iter().map(|state| (state.state_id, state)),
        "index F3D feature history states",
    )
}

/// Each history's states by ID, indexed for the first feature that walks
/// that history and kept for the rest of one binding pass.
struct FeatureHistoryStates<'a, 'ctx> {
    by_history: HashMap<
        usize,
        (
            HistoryStates<'a>,
            cadmpeg_core::decode::ScopedReservation<'ctx>,
        ),
    >,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'a, 'ctx> FeatureHistoryStates<'a, 'ctx> {
    fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            by_history: HashMap::new(),
            storage: ctx.reserve_scoped(0, "index F3D feature histories")?,
        })
    }

    fn states_of(
        &mut self,
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        history: &'a AsmHistory,
    ) -> Result<Option<&HistoryStates<'a>>, cadmpeg_core::CodecError> {
        // A history is keyed by its address: the histories outlive the pass.
        let key = std::ptr::from_ref(history).addr();
        if !self.by_history.contains_key(&key) {
            let states = unique_feature_history_states(ctx, history)?;
            self.storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut self.by_history,
                    key,
                    states,
                    "index F3D feature histories",
                )
                .map(|_| ())
            })?;
        }
        Ok(self.by_history.get(&key).map(|(states, _)| states))
    }
}

fn singleton_revised_input_body_across_state_chain<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HistoryStates<'a>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "index F3D revised input bodies")?;
    let mut current = state;
    let mut visited = HashSet::new();
    let mut revised = BTreeSet::new();
    while current.state_id != previous_state_id {
        if !storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut visited,
                current.state_id,
                "visit F3D revised input body states",
            )
        })? {
            return Ok(None);
        }
        let Some(transition) = current.transition.as_ref() else {
            return Ok(None);
        };
        let bodies = &transition.topology.bodies;
        for &body in ctx
            .admit_iter(&bodies.updated, "scan F3D revised input bodies")?
            .chain(ctx.admit_iter(&bodies.deleted, "scan F3D revised input bodies")?)
        {
            ctx.insert_scoped_btree_set(
                &mut storage,
                &mut revised,
                body,
                "find F3D revised input body",
                "index F3D revised input bodies",
            )?;
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
    let is_revised =
        |body: &i64| ctx.contains_btree_set(&revised, body, "find F3D revised input body");
    let Some(first) = ctx.position_by(&input.bodies, is_revised, "find F3D revised input body")?
    else {
        return Ok(None);
    };
    let repeated = ctx.any_by(
        &input.bodies[first + 1..],
        is_revised,
        "find F3D revised input body",
    )?;
    Ok((!repeated).then_some(input.bodies[first]))
}

fn singleton_body_revision_across_state_chain<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HistoryStates<'a>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(result_topology) = state.topology() else {
        return Ok(None);
    };
    let mut visited_storage = ctx.reserve_scoped(0, "visit F3D stable body revision states")?;
    let mut current = state;
    let mut visited = HashSet::new();
    let mut selected = None;
    while current.state_id != previous_state_id {
        if !visited_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut visited,
                current.state_id,
                "visit F3D stable body revision states",
            )
        })? {
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
    let operation = "find F3D stable revised body";
    Ok((ctx.contains(&result_topology.bodies, &body, operation)?
        && ctx.contains(&input_topology.bodies, &body, operation)?)
    .then_some(body))
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

type EntityOperand = crate::records::topology::entity_selection::DesignEntitySelectionOperand;
type FaceOperand = crate::records::topology::face::DesignFaceOperand;
/// Stream, scope record, owning group record, member ordinal and record of an
/// entity-selection operand.
type EntityOperandKey<'a> = (Option<&'a str>, u32, u32, u32, u32);
/// Stream, scope record and record of a face operand.
type FaceOperandKey<'a> = (Option<&'a str>, u32, u32);

/// Face-selection inputs keyed for the per-feature lookups of one binding
/// pass, each index built on its first lookup.
pub(crate) struct FaceSelectionIndex<'a, 'ctx> {
    pub(super) inputs: FeatureFaceSelectionInputs<'a>,
    pub(super) groups: GroupIndex<'a, 'ctx>,
    pub(super) face_operands: UniqueIndex<'a, 'ctx, FaceOperandKey<'a>, FaceOperand>,
    pub(super) body_recipe_operands: UniqueIndex<'a, 'ctx, MemberOperandKey<'a>, BodyRecipeOperand>,
    entity_operands: UniqueIndex<'a, 'ctx, EntityOperandKey<'a>, EntityOperand>,
    /// Plain role-0x5 groups of each scope, by stream and scope record.
    stitch_groups: std::cell::OnceCell<(
        HashMap<(Option<&'a str>, u32), Vec<&'a OperandGroup>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    )>,
}

impl<'a, 'ctx> FaceSelectionIndex<'a, 'ctx> {
    pub(crate) fn new(inputs: FeatureFaceSelectionInputs<'a>) -> Self {
        Self {
            inputs,
            groups: group_index(inputs.groups),
            face_operands: UniqueIndex::new(
                inputs.operands,
                |ctx, operand| {
                    Ok(Some((
                        native_stream_of(ctx, &operand.id)?,
                        operand.scope_record_index,
                        operand.record_index(),
                    )))
                },
                "index F3D face selection operands",
            ),
            body_recipe_operands: UniqueIndex::new(
                inputs.body_recipe_operands,
                member_operand_key,
                "index F3D group body recipe operands",
            ),
            entity_operands: UniqueIndex::new(
                inputs.entity_operands,
                |ctx, operand| {
                    Ok(Some((
                        native_stream_of(ctx, &operand.id)?,
                        operand.scope_record_index,
                        operand.group_record_index,
                        operand.group_member_ordinal,
                        operand.record_index(),
                    )))
                },
                "index F3D entity selection operands",
            ),
            stitch_groups: std::cell::OnceCell::new(),
        }
    }

    fn stitch_groups(
        &self,
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        stream: Option<&'a str>,
        scope_record_index: u32,
    ) -> Result<&[&'a OperandGroup], cadmpeg_core::CodecError> {
        let operation = "index F3D Stitch face groups";
        let (groups, _) = match self.stitch_groups.get() {
            Some(groups) => groups,
            None => {
                let mut storage = ctx.reserve_scoped(0, operation)?;
                let mut groups = HashMap::new();
                for group in ctx.admit_iter(self.inputs.groups, operation)? {
                    if group.role() != DesignOperandRole::ROLE_0X5
                        || group.extrude_role().is_some()
                        || group.extrude_face_role().is_some()
                    {
                        continue;
                    }
                    let key = (native_stream_of(ctx, &group.id)?, group.scope_record_index);
                    storage.with_storage(|| {
                        ctx.push_hash_group(&mut groups, key, group, operation, operation)
                    })?;
                }
                self.stitch_groups.get_or_init(|| (groups, storage))
            }
        };
        Ok(ctx
            .get_hash_map(groups, &(stream, scope_record_index), operation)?
            .map_or(&[][..], Vec::as_slice))
    }
}

/// The faces of each feature input topology, keyed by topology and feature
/// identity, so selections can add the faces they name. A key that several
/// topologies share names none.
struct InputTopologyFaces<'t, 'ctx> {
    positions: HashMap<(&'t str, &'t str), Option<usize>>,
    faces: Vec<&'t mut cadmpeg_ir::features::DistinctMembers<cadmpeg_ir::ids::HistoricalFaceId>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'t, 'ctx> InputTopologyFaces<'t, 'ctx> {
    fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        topologies: &'t mut [cadmpeg_ir::features::FeatureInputTopology],
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let operation = "index F3D feature input topologies";
        let mut storage = ctx.reserve_scoped(0, operation)?;
        let mut faces = storage.with_storage(|| ctx.collection_vec(topologies.len(), operation))?;
        let mut positions = HashMap::new();
        for (position, topology) in ctx.admit_iter(topologies, operation)?.enumerate() {
            let cadmpeg_ir::features::FeatureInputTopology {
                id,
                input_of,
                faces: topology_faces,
                ..
            } = topology;
            let (id, input_of): (&'t _, &'t _) = (id, input_of);
            let key = (id.as_str(), input_of.as_str());
            if let Some(indexed) = ctx.get_mut_hash_map(&mut positions, &key, operation)? {
                *indexed = None;
            } else {
                storage.with_storage(|| {
                    ctx.insert_hash_map(&mut positions, key, Some(position), operation)
                        .map(|_| ())
                })?;
            }
            faces.push(topology_faces);
        }
        Ok(Self {
            positions,
            faces,
            _storage: storage,
        })
    }

    fn faces_of(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        state_id: &cadmpeg_ir::ids::FeatureInputTopologyId,
        feature_id: &cadmpeg_ir::features::FeatureId,
    ) -> Result<
        Option<&mut cadmpeg_ir::features::DistinctMembers<cadmpeg_ir::ids::HistoricalFaceId>>,
        cadmpeg_core::CodecError,
    > {
        let Some(&Some(position)) = ctx.get_hash_map(
            &self.positions,
            &(state_id.as_str(), feature_id.as_str()),
            "find F3D feature input topology",
        )?
        else {
            return Ok(None);
        };
        Ok(self.faces.get_mut(position).map(|faces| &mut **faces))
    }

    /// Adds `face` to a topology's faces unless it is already present.
    fn add_face(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        state_id: &cadmpeg_ir::ids::FeatureInputTopologyId,
        feature_id: &cadmpeg_ir::features::FeatureId,
        face: &cadmpeg_ir::ids::HistoricalFaceId,
        operation: &'static str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let Some(faces) = self.faces_of(ctx, state_id, feature_id)? else {
            return Ok(());
        };
        if !ctx.contains(&**faces, face, operation)? {
            let retained = face.try_clone_for_decode(ctx, operation)?;
            faces.insert(ctx, retained, operation)?;
        }
        Ok(())
    }
}

pub(crate) fn bind_feature_face_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    input_topologies: &mut [cadmpeg_ir::features::FeatureInputTopology],
    input: FeatureFaceSelectionInputs<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let histories = input.histories;
    let scopes = scope_index(input.scopes);
    let index = FaceSelectionIndex::new(input);
    let mut input_faces = InputTopologyFaces::new(ctx, input_topologies)?;
    for feature in ctx.admit_iter(features, "scan F3D face selection features")? {
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
                let scope = match scopes.get(ctx, native_ref) {
                    Ok(Some(scope)) => scope,
                    Ok(None) => break 'feature_edit,
                    Err(error) => {
                        edit_result = Err(error);
                        break 'feature_edit;
                    }
                };
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
                let updated_faces = &transition.topology.faces.updated;
                let mut binding = FaceSelectionBinding {
                    feature_id,
                    previous_state_id,
                    operation_history_id: &history.id,
                    scope,
                    input_faces: &mut input_faces,
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
                                &index,
                                updated_faces,
                            ));
                            admitted!(bind_entity_face_selection(ctx, face, &mut binding, &index,));
                        }
                        let sides = match extent {
                            cadmpeg_ir::features::ExtrudeExtent::OneSided { side }
                            | cadmpeg_ir::features::ExtrudeExtent::Symmetric { side } => {
                                [Some(side), None]
                            }
                            cadmpeg_ir::features::ExtrudeExtent::TwoSided { first, second } => {
                                [Some(first), Some(second)]
                            }
                        };
                        for side in sides.into_iter().flatten() {
                            if let cadmpeg_ir::features::LinearTermination::ToFace {
                                face, ..
                            } = &mut side.termination
                            {
                                admitted!(selection::bind_face_selection(
                                    ctx,
                                    face,
                                    scope,
                                    &index,
                                    updated_faces,
                                ));
                            }
                        }
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Pattern { seeds, .. },
                    ) => {
                        let seeds = match ctx.admit_iter(seeds, "scan F3D pattern face seeds") {
                            Ok(seeds) => seeds,
                            Err(error) => {
                                edit_result = Err(error.into());
                                return;
                            }
                        };
                        for seed in seeds {
                            let cadmpeg_ir::features::patterns::PatternSeed::Faces(faces) = seed
                            else {
                                continue;
                            };
                            admitted!(selection::bind_face_selection(
                                ctx,
                                faces,
                                scope,
                                &index,
                                updated_faces,
                            ));
                            admitted!(
                                bind_entity_face_selection(ctx, faces, &mut binding, &index,)
                            );
                        }
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::MoveFace { faces, .. },
                    ) => {
                        admitted!(selection::bind_face_selection(
                            ctx,
                            faces,
                            scope,
                            &index,
                            updated_faces,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Thicken { faces, .. },
                    ) => {
                        admitted!(selection::bind_face_selection(
                            ctx,
                            faces,
                            scope,
                            &index,
                            updated_faces,
                        ));
                        admitted!(selection::bind_body_recipe_face_selection(
                            ctx,
                            faces,
                            feature_id,
                            previous_state_id,
                            scope,
                            &index,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::KnitSurface { faces, .. },
                    ) => {
                        admitted!(bind_surface_stitch_face_selection(
                            ctx,
                            faces,
                            &mut binding,
                            &index,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::SplitFace { targets, .. },
                    ) => {
                        admitted!(selection::bind_face_selection(
                            ctx,
                            targets,
                            scope,
                            &index,
                            updated_faces,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Hole {
                            face: Some(face), ..
                        },
                    ) => {
                        admitted!(bind_hole_face_selection(ctx, face, &mut binding));
                    }
                    _ => {}
                }
            }
        });
        edit_result?;
    }
    Ok(())
}

struct FaceSelectionBinding<'b, 'a, 't, 'ctx> {
    feature_id: &'b cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    operation_history_id: &'b str,
    scope: &'a DesignScope,
    input_faces: &'b mut InputTopologyFaces<'t, 'ctx>,
}

fn bind_entity_face_selection<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    binding: &mut FaceSelectionBinding<'_, 'a, '_, '_>,
    index: &FaceSelectionIndex<'a, 'ctx>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FaceSelection;

    let group_id = match selection {
        FaceSelection::Native(group_id) => group_id.as_str(),
        _ => return Ok(()),
    };
    let Some(group) = scope_group(ctx, &index.groups, binding.scope, group_id, |_| true)? else {
        return Ok(());
    };
    if group.members().is_empty() {
        return Ok(());
    }
    bind_entity_face_groups(
        ctx,
        selection,
        &group.id,
        binding,
        std::slice::from_ref(&group),
        index,
    )
}

fn bind_surface_stitch_face_selection<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    binding: &mut FaceSelectionBinding<'_, 'a, '_, '_>,
    index: &FaceSelectionIndex<'a, 'ctx>,
) -> Result<(), cadmpeg_core::CodecError> {
    let scope = binding.scope;
    let native_id = match selection {
        cadmpeg_ir::features::FaceSelection::Native(native_id) => native_id.as_str(),
        _ => return Ok(()),
    };
    if !ctx.equal(
        native_id,
        scope.id.as_str(),
        "compare F3D Stitch selection scope",
    )? {
        return Ok(());
    }
    let references = scope.reference_members();
    let Some(input_end) = references.len().checked_sub(2) else {
        return Ok(());
    };
    if input_end == 0 || !input_end.is_multiple_of(2) {
        return Ok(());
    }
    let stream = native_stream_of(ctx, &scope.id)?;
    let candidates = index.stitch_groups(ctx, stream, scope.record_index)?;
    if candidates.len().checked_mul(2) != Some(input_end) {
        return Ok(());
    }
    let (mut matching_groups, _matching_storage) = ctx
        .copy_temporary_slice(candidates, "collect F3D Stitch face groups")
        .map_err(cadmpeg_core::CodecError::ResourceLimit)?;
    ctx.stable_sort_by(
        &mut matching_groups,
        |value| &value.scope_reference_ordinal,
        Ord::cmp,
        "sort F3D Stitch face groups",
    )?;
    // Group k must sit at reference ordinal 2k, name reference 2k as its record
    // and hold exactly reference 2k + 1 as its one member.
    let mismatched = ctx.any_by(
        matching_groups.iter().enumerate().zip(
            references
                .values()
                .step_by(2)
                .zip(references.values().skip(1).step_by(2)),
        ),
        |((ordinal, group), (group_reference, member_reference))| {
            let [member] = group.members() else {
                return Ok(true);
            };
            Ok(
                u32::try_from(ordinal * 2) != Ok(group.scope_reference_ordinal)
                    || group.record_index != *group_reference
                    || member.value != *member_reference,
            )
        },
        "check F3D Stitch face groups",
    )?;
    if mismatched {
        return Ok(());
    }
    bind_entity_face_groups(ctx, selection, &scope.id, binding, &matching_groups, index)
}

fn bind_entity_face_groups<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    native_id: &str,
    binding: &mut FaceSelectionBinding<'_, 'a, '_, '_>,
    groups: &[&'a OperandGroup],
    index: &FaceSelectionIndex<'a, 'ctx>,
) -> Result<(), cadmpeg_core::CodecError> {
    let feature_id = binding.feature_id;
    let previous_state_id = binding.previous_state_id;
    let scope = binding.scope;

    if groups.is_empty() {
        return Ok(());
    }
    let stream = native_stream_of(ctx, &scope.id)?;
    let mut selected_storage = ctx.reserve_scoped(0, "collect F3D entity face candidates")?;
    let mut seen = HashSet::new();
    let mut selected = Vec::<(&str, i64, bool)>::new();
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
            let Some(operand) = index.entity_operands.get(
                ctx,
                &(
                    stream,
                    scope.record_index,
                    group.record_index,
                    ordinal,
                    record_index,
                ),
            )?
            else {
                return Ok(());
            };
            let [candidate] = operand.historical_face_candidates.as_slice() else {
                return Ok(());
            };
            let local = ctx.equal(
                candidate.history_id.as_str(),
                binding.operation_history_id,
                "compare F3D entity face candidate history",
            )? && ctx.contains(
                &candidate.historical.state_ids,
                &previous_state_id,
                "find F3D entity face candidate state",
            )?;
            let Some(source) = historical_brep_source(ctx, &candidate.history_id)? else {
                return Ok(());
            };
            let entry = (source, candidate.face_slot, local);
            selected_storage.with_storage(|| {
                if ctx.insert_hash_set(&mut seen, entry, "index F3D entity face candidates")? {
                    ctx.push_vec(&mut selected, entry, "collect F3D entity face candidates")?;
                }
                Ok::<(), cadmpeg_core::CodecError>(())
            })?;
        }
    }
    let state_id = crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
    if binding
        .input_faces
        .faces_of(ctx, &state_id, feature_id)?
        .is_none()
    {
        return Ok(());
    }
    let mut faces = ctx.collection_vec(selected.len(), "collect F3D historical entity faces")?;
    for &(source, face, local) in ctx.admit_iter(&selected, "scan F3D entity face candidates")? {
        let Some(face) =
            historical_input_face_id(ctx, feature_id, previous_state_id, source, face, local)?
        else {
            return Ok(());
        };
        faces.push(face);
    }
    for face in ctx.admit_iter(&faces, "scan F3D faces")? {
        binding.input_faces.add_face(
            ctx,
            &state_id,
            feature_id,
            face,
            "index F3D historical entity faces",
        )?;
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

/// The history-input identity of a candidate face: its slot alone for a face
/// of the operation's own input state, otherwise qualified by its B-rep source,
/// which must then hold no `#` and no whitespace.
fn historical_input_face_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    source: &str,
    face: i64,
    local: bool,
) -> Result<Option<cadmpeg_ir::ids::HistoricalFaceId>, cadmpeg_core::CodecError> {
    if local {
        return crate::ids::history_input_face_id_charged(ctx, feature_id, previous_state_id, face)
            .map(Some);
    }
    let operation = "check F3D historical face source";
    if ctx.contains_text(source, "#", operation)?
        || ctx.any_by(
            source.chars(),
            |character| Ok(character.is_whitespace()),
            operation,
        )?
    {
        return Ok(None);
    }
    crate::ids::history_input_face_id_charged(
        ctx,
        feature_id,
        previous_state_id,
        format_args!("{}:{source}:{face}", source.len()),
    )
    .map(Some)
}

fn bind_hole_face_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    binding: &mut FaceSelectionBinding<'_, '_, '_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FaceSelection;
    let feature_id = binding.feature_id;
    let previous_state_id = binding.previous_state_id;

    let FaceSelection::Native(native_id) = selection else {
        return Ok(());
    };
    let Some(construction) = binding.scope.hole_construction() else {
        return Ok(());
    };
    let Some(face_selection) = &construction.face_selection else {
        return Ok(());
    };
    let [candidate] = face_selection.historical_face_candidates.as_slice() else {
        return Ok(());
    };
    let local = ctx.equal(
        candidate.history_id.as_str(),
        binding.operation_history_id,
        "compare F3D hole face candidate history",
    )? && ctx.contains(
        &candidate.historical.state_ids,
        &previous_state_id,
        "find F3D hole face candidate state",
    )?;
    let Some(source) = historical_brep_source(ctx, &candidate.history_id)? else {
        return Ok(());
    };
    let state_id = crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
    if binding
        .input_faces
        .faces_of(ctx, &state_id, feature_id)?
        .is_none()
    {
        return Ok(());
    }
    let Some(face) = historical_input_face_id(
        ctx,
        feature_id,
        previous_state_id,
        source,
        candidate.face_slot,
        local,
    )?
    else {
        return Ok(());
    };
    binding.input_faces.add_face(
        ctx,
        &state_id,
        feature_id,
        &face,
        "index F3D historical hole face",
    )?;
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

/// Stream, owning group record, member ordinal and record of an
/// entity-selection operand.
type PathOperandKey<'a> = (Option<&'a str>, u32, u32, u32);

pub(crate) fn bind_feature_path_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SurfaceBoundary};

    let scopes = scope_index(scopes);
    let groups = group_index(groups);
    let operands = path_operand_index(operands);
    for feature in ctx.admit_iter(features, "scan F3D path selection features")? {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| 'feature_edit: {
            let Some(native_ref) = native_ref else {
                break 'feature_edit;
            };
            let scope = match scopes.get(ctx, native_ref) {
                Ok(Some(scope)) => scope,
                Ok(None) => break 'feature_edit,
                Err(error) => {
                    edit_result = Err(error);
                    break 'feature_edit;
                }
            };
            let Some(previous_state_id) = scope.previous_history_state_id() else {
                break 'feature_edit;
            };
            let bind = |path: &mut cadmpeg_ir::features::PathRef| {
                bind_entity_selection_path(
                    ctx,
                    path,
                    feature_id,
                    previous_state_id,
                    scope,
                    &groups,
                    &operands,
                )
            };
            match definition {
                FeatureDefinition::Operation(FeatureOperation::FilledSurface {
                    boundary: SurfaceBoundary::Path(path),
                    ..
                }) => {
                    edit_result = bind(path);
                }
                FeatureDefinition::Operation(FeatureOperation::Loft { guidance, .. }) => {
                    let paths = match guidance {
                        cadmpeg_ir::features::LoftGuidance::Guides(paths) => paths.as_mut_slice(),
                        cadmpeg_ir::features::LoftGuidance::Centerline(path) => {
                            std::slice::from_mut(path)
                        }
                    };
                    let paths = match ctx.admit_iter(paths, "scan F3D loft guide paths") {
                        Ok(paths) => paths,
                        Err(error) => {
                            edit_result = Err(error.into());
                            break 'feature_edit;
                        }
                    };
                    for path in paths {
                        edit_result = bind(path);
                        if edit_result.is_err() {
                            break;
                        }
                    }
                }
                FeatureDefinition::Operation(FeatureOperation::Sweep {
                    path, guide_rail, ..
                }) => {
                    if let Some(path) = path {
                        edit_result = bind(path);
                        if edit_result.is_err() {
                            break 'feature_edit;
                        }
                    }
                    if let Some(guide_rail) = guide_rail {
                        edit_result = bind(&mut guide_rail.path);
                    }
                }
                _ => {}
            }
        });
        edit_result?;
    }
    Ok(())
}

/// Entity-selection operands by stream, owning group record, member ordinal
/// and record.
fn path_operand_index<'a, 'ctx>(
    operands: &'a [EntityOperand],
) -> UniqueIndex<'a, 'ctx, PathOperandKey<'a>, EntityOperand> {
    UniqueIndex::new(
        operands,
        |ctx, operand| {
            Ok(Some((
                native_stream_of(ctx, &operand.id)?,
                operand.group_record_index,
                operand.group_member_ordinal,
                operand.record_index(),
            )))
        },
        "index F3D path selection operands",
    )
}

fn bind_entity_selection_path<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    path: &mut cadmpeg_ir::features::PathRef,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &'a DesignScope,
    groups: &GroupIndex<'a, 'ctx>,
    operands: &UniqueIndex<'a, 'ctx, PathOperandKey<'a>, EntityOperand>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::PathRef;

    let PathRef::Native(group_id) = path else {
        return Ok(());
    };
    let Some(group) = scope_group(ctx, groups, scope, group_id, |_| true)? else {
        return Ok(());
    };
    if group.members().is_empty() {
        return Ok(());
    }
    let stream = native_stream_of(ctx, &scope.id)?;
    let (mut edge_slots, _edge_slots_storage) = ctx
        .with_scoped_storage("collect F3D path edge slots", || {
            ctx.collection_vec(group.members().len(), "collect F3D path edge slots")
        })?;
    for (ordinal, record_index) in ctx
        .admit_iter(group.members(), "scan F3D path group members")?
        .map(|member| member.value)
        .enumerate()
    {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return Ok(());
        };
        let Some(operand) =
            operands.get(ctx, &(stream, group.record_index, ordinal, record_index))?
        else {
            return Ok(());
        };
        let Some(edge_slot) = operand.resolved_edge_slot else {
            return Ok(());
        };
        edge_slots.push(edge_slot);
    }

    let mut edge_ids = ctx.collection_vec(edge_slots.len(), "collect F3D path edge identities")?;
    for &slot in ctx.admit_iter(&edge_slots, "scan F3D path edge slots")? {
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
    let input_states = vertex_recipe_input_states(ctx, scopes, timelines)?;
    for (scope, &state_id) in ctx
        .admit_iter(&mut *scopes, "scan F3D work point recipe scopes")?
        .zip(&input_states)
    {
        if scope.kind() != crate::records::feature::scope::DesignFeatureKind::WorkPoint {
            continue;
        }
        let Some(construction) = scope.work_point_construction_mut() else {
            continue;
        };
        let solved_position = cadmpeg_ir::math::Point3::new(
            construction.position[0].get() * 10.0,
            construction.position[1].get() * 10.0,
            construction.position[2].get() * 10.0,
        );
        // The input topology and its index are found for the first recipe.
        let mut topology = None;
        let mut index = None;
        // A work point has a fixed number of inputs.
        for recipe in construction.rule.vertex_recipes_mut() {
            recipe.resolution = None;
            let Some(state_id) = state_id else {
                continue;
            };
            let found = match topology {
                Some(found) => found,
                None => *topology.insert(
                    unique_history_state(ctx, histories, state_id)?
                        .and_then(|(_, state)| state.topology()),
                ),
            };
            let Some(topology) = found else {
                continue;
            };
            let index = match &mut index {
                Some(index) => index,
                None => index.insert(boundary_vertex_index(ctx, topology)?),
            };
            let Some((vertex, position)) = vertex_recipe_candidate(ctx, recipe, topology, index)?
            else {
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

    for (scope, &state_id) in ctx
        .admit_iter(&mut *scopes, "scan F3D work plane recipe scopes")?
        .zip(&input_states)
    {
        if scope.kind() != crate::records::feature::scope::DesignFeatureKind::WorkPlane {
            continue;
        }
        let transform = scope.work_plane_transform();
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
        let index = boundary_vertex_index(ctx, topology)?;
        let (Some(first), Some(second), Some(third)) = (
            vertex_recipe_candidate(ctx, first, topology, &index)?,
            vertex_recipe_candidate(ctx, second, topology, &index)?,
            vertex_recipe_candidate(ctx, third, topology, &index)?,
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

/// The input state of each work plane and work point scope: the state of the
/// latest earlier history-bearing scope of its stream in authored order. Among
/// equally ordered predecessors the first scope wins. Other scopes have none.
fn vertex_recipe_input_states<'s>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &'s [crate::records::feature::scope::DesignParameterScope],
    timelines: &[crate::records::entity_header::DesignFeatureTimeline],
) -> Result<Vec<Option<i64>>, cadmpeg_core::CodecError> {
    use crate::records::feature::scope::DesignFeatureKind;

    let source_ordinals =
        crate::design::feature_project::authored_scope_ordinals_per_stream(ctx, scopes, timelines)?;
    let ordinal_of = |scope: &'s crate::records::feature::scope::DesignParameterScope| {
        let stream = native_stream_of(ctx, &scope.id)?.unwrap_or(crate::ids::DEFAULT_STREAM);
        Ok::<_, cadmpeg_core::CodecError>(
            ctx.get_hash_map(
                &source_ordinals,
                &(stream, scope.record_index),
                "find F3D vertex recipe scope ordinal",
            )?
            .map(|&ordinal| (stream, ordinal)),
        )
    };
    // History-bearing scopes of each stream in ascending authored order; the
    // stable sort keeps scope order among equal ordinals.
    let operation = "index F3D vertex recipe predecessors";
    let mut predecessors_storage = ctx.reserve_scoped(0, operation)?;
    let mut predecessors = HashMap::<&str, Vec<(u64, i64)>>::new();
    for scope in ctx.admit_iter(scopes, "scan F3D vertex recipe predecessors")? {
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let Some((stream, ordinal)) = ordinal_of(scope)? else {
            continue;
        };
        predecessors_storage.with_storage(|| {
            ctx.push_hash_group(
                &mut predecessors,
                stream,
                (ordinal, state_id),
                operation,
                operation,
            )
        })?;
    }
    // Each stream's list is sorted on its first lookup.
    let mut sorted_streams = HashSet::new();
    let mut input_states =
        ctx.collection_vec(scopes.len(), "index F3D vertex recipe input states")?;
    for scope in ctx.admit_iter(scopes, "scan F3D vertex recipe scopes")? {
        input_states.push(None);
        if !matches!(
            scope.kind(),
            DesignFeatureKind::WorkPlane | DesignFeatureKind::WorkPoint
        ) {
            continue;
        }
        let Some((stream, ordinal)) = ordinal_of(scope)? else {
            continue;
        };
        let Some(candidates) = ctx.get_mut_hash_map(&mut predecessors, stream, operation)? else {
            continue;
        };
        if predecessors_storage
            .with_storage(|| ctx.insert_hash_set(&mut sorted_streams, stream, operation))?
        {
            ctx.stable_sort_by_key(
                candidates,
                |&(ordinal, _)| ordinal,
                Ord::cmp,
                "sort F3D vertex recipe predecessors",
            )?;
        }
        let earlier = ctx.partition_point(
            candidates,
            |&(candidate, _)| Ok(candidate < ordinal),
            "find F3D vertex recipe predecessor",
        )?;
        let Some(&(latest, _)) = earlier.checked_sub(1).and_then(|last| candidates.get(last))
        else {
            continue;
        };
        let first_latest = ctx.partition_point(
            candidates,
            |&(candidate, _)| Ok(candidate < latest),
            "find F3D vertex recipe predecessor",
        )?;
        if let (Some(slot), Some(&(_, state_id))) =
            (input_states.last_mut(), candidates.get(first_latest))
        {
            *slot = Some(state_id);
        }
    }
    Ok(input_states)
}

/// Resolve edge-treatment corner recipes in their bound feature-input state.
pub(crate) fn bind_edge_treatment_vertex_history(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    let scopes = ScopesByRecord::new(decode, scopes)?;
    let mut recipe_index = RecentTopologyValue::default();
    for operand in decode.admit_iter(operands, "scan F3D edge treatment vertex operands")? {
        operand.recipe.resolution = None;
        let Some(scope) = scopes.find(decode, &operand.id, operand.scope_record_index)? else {
            continue;
        };
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
        let index = recipe_index.of(topology, || recipe_topology_index(decode, topology))?;
        for reference in decode.admit_iter(
            &mut operand.recipe.recipe_references,
            "scan F3D edge treatment vertex recipe references",
        )? {
            bind_historical_recipe_reference_candidates(decode, reference, index)?;
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
    index: &BoundaryVertexIndex<'_>,
) -> Result<Option<(i64, cadmpeg_ir::math::Point3)>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "collect F3D vertex recipe face slots")?;
    let mut face_slots = Vec::new();
    for reference in ctx.admit_iter(
        &recipe.recipe_references,
        "scan F3D recipe recipe references",
    )? {
        let mut slots_storage =
            ctx.reserve_scoped(0, "collect F3D vertex recipe candidate faces")?;
        let mut slots = Vec::new();
        for face in ctx.admit_iter(
            &reference.candidate_faces,
            "scan F3D vertex recipe candidate faces",
        )? {
            let Some(slot) = stable_ref(ctx, face.as_str())? else {
                continue;
            };
            if !index.faces.contains(&slot) {
                continue;
            }
            ctx.push_scoped_vec(
                &mut slots_storage,
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
        ctx.push_scoped_vec(
            &mut storage,
            &mut face_slots,
            *slot,
            "collect F3D vertex recipe face slots",
        )?;
    }
    if face_slots.is_empty() {
        return Ok(None);
    }
    let Some(vertex) = common_face_vertex(ctx, &face_slots, index)? else {
        return Ok(None);
    };
    Ok(
        unique_historical_vertex_position(ctx, vertex, topology)?
            .map(|position| (vertex, position)),
    )
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

/// One topology's faces, vertices, edge endpoints and face boundary edges,
/// indexed once for vertex recipe resolution.
struct BoundaryVertexIndex<'ctx> {
    boundaries: FaceBoundaryEdgeIndex<'ctx>,
    faces: HashSet<i64>,
    vertices: HashSet<i64>,
    /// Endpoints of each edge; an edge listed twice has none.
    edge_vertices: HashMap<i64, Option<(i64, i64)>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn boundary_vertex_index<'ctx>(
    decode: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<BoundaryVertexIndex<'ctx>, cadmpeg_core::CodecError> {
    let boundaries = face_boundary_edge_index(decode, topology)?;
    let operation = "index F3D boundary vertex topology";
    let mut storage = decode.reserve_scoped(0, operation)?;
    let faces = storage
        .with_storage(|| decode.collect_hash_set(topology.faces.iter().copied(), operation))?;
    let vertices = storage
        .with_storage(|| decode.collect_hash_set(topology.vertices.iter().copied(), operation))?;
    let mut edge_vertices = HashMap::new();
    for edge in decode.admit_iter(&topology.edge_vertices, operation)? {
        if let Some(endpoints) = edge_vertices.get_mut(&edge.edge) {
            *endpoints = None;
            continue;
        }
        storage.with_storage(|| {
            decode
                .insert_hash_map(
                    &mut edge_vertices,
                    edge.edge,
                    Some((edge.start_vertex, edge.end_vertex)),
                    operation,
                )
                .map(|_| ())
        })?;
    }
    Ok(BoundaryVertexIndex {
        boundaries,
        faces,
        vertices,
        edge_vertices,
        _storage: storage,
    })
}

fn boundary_vertices_for_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: impl IntoIterator<Item = Result<Option<i64>, cadmpeg_core::CodecError>>,
    index: &BoundaryVertexIndex<'_>,
) -> Result<Option<BTreeSet<i64>>, cadmpeg_core::CodecError> {
    let mut vertices = BTreeSet::new();
    for face in faces {
        let Some(face) = face? else {
            continue;
        };
        let Some(edges) = decode.get_btree_map(
            &index.boundaries.boundaries,
            &face,
            "find F3D boundary vertex face",
        )?
        else {
            return Ok(None);
        };
        for edge_slot in decode.admit_iter(&edges.edges, "scan F3D boundary vertex edges")? {
            let Some(&Some((start, end))) = index.edge_vertices.get(edge_slot) else {
                return Ok(None);
            };
            if !index.vertices.contains(&start) || !index.vertices.contains(&end) {
                return Ok(None);
            }
            decode.insert_btree_set(&mut vertices, start, "collect F3D boundary vertices")?;
            decode.insert_btree_set(&mut vertices, end, "collect F3D boundary vertices")?;
        }
    }
    Ok((!vertices.is_empty()).then_some(vertices))
}

fn common_face_vertex(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    face_slots: &[i64],
    index: &BoundaryVertexIndex<'_>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let mut vertices_storage = decode.reserve_scoped(0, "collect F3D boundary vertices")?;
    let mut common = None::<BTreeSet<i64>>;
    for face in decode.admit_iter(face_slots, "scan F3D common face slots")? {
        let Some(vertices) = vertices_storage.with_storage(|| {
            boundary_vertices_for_faces(
                decode,
                std::iter::once(Ok::<_, cadmpeg_core::CodecError>(Some(*face))),
                index,
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
    let index = boundary_vertex_index(decode, topology)?;
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
                Ok(index.faces.contains(&slot).then_some(slot))
            });
        let Some(vertices) =
            vertices_storage.with_storage(|| boundary_vertices_for_faces(decode, faces, &index))?
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    vertex: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<cadmpeg_ir::math::Point3>, cadmpeg_core::CodecError> {
    let Some(binding) = unique_by(
        ctx,
        &topology.vertex_points,
        |binding| binding.entity == vertex,
        "find F3D historical vertex point",
    )?
    else {
        return Ok(None);
    };
    let Some(position) = unique_by(
        ctx,
        &topology.point_positions,
        |position| position.point == binding.carrier,
        "find F3D historical point position",
    )?
    else {
        return Ok(None);
    };
    Ok(position.position.is_finite().then_some(position.position))
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
) -> Result<Option<HistoryStatePair<'a>>, cadmpeg_core::CodecError> {
    // A directly linked pair decides the answer, ambiguous or not; only an
    // absent direct link falls back to reachability.
    match unique_history_state_pair_for_link(decode, histories, state_id, previous_state_id, true)?
    {
        StatePairSearch::Unique(pair) => return Ok(Some(pair)),
        StatePairSearch::Ambiguous => return Ok(None),
        StatePairSearch::Absent => {}
    }
    Ok(
        match unique_history_state_pair_for_link(
            decode,
            histories,
            state_id,
            previous_state_id,
            false,
        )? {
            StatePairSearch::Unique(pair) => Some(pair),
            StatePairSearch::Ambiguous | StatePairSearch::Absent => None,
        },
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
        let duplicate = decode.any_by(
            &edge_context.incident_loops[..ordinal],
            |earlier| Ok(earlier.face_slot == face),
            "find duplicate F3D Hem incident face",
        )?;
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
        let Some(binding) = unique_by(
            decode,
            &previous.face_surfaces,
            |binding| binding.entity == face,
            "find F3D Hem face surface",
        )?
        else {
            continue;
        };
        let Some(plane) = unique_by(
            decode,
            &previous.surface_planes,
            |plane| plane.surface == binding.carrier,
            "find F3D Hem face plane",
        )?
        else {
            continue;
        };
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

/// A unique state pair: its history, the state and the previous state.
type HistoryStatePair<'a> = (&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState);

/// The outcome of searching every history for one state pair.
enum StatePairSearch<'a> {
    /// No history holds the pair.
    Absent,
    /// More than one history holds it, or its states cannot be read back.
    Ambiguous,
    /// Exactly one history holds it.
    Unique(HistoryStatePair<'a>),
}

fn unique_history_state_pair_for_link<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &'a [AsmHistory],
    state_id: i64,
    previous_state_id: i64,
    require_direct: bool,
) -> Result<StatePairSearch<'a>, cadmpeg_core::CodecError> {
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
        return Ok(StatePairSearch::Absent);
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
        return Ok(StatePairSearch::Ambiguous);
    }
    let (Some(history), Some((state_index, previous_index))) = (histories.get(index), pair_indices)
    else {
        return Ok(StatePairSearch::Ambiguous);
    };
    let (Some(state), Some(previous)) = (
        history.states.get(state_index),
        history.states.get(previous_index),
    ) else {
        return Ok(StatePairSearch::Ambiguous);
    };
    Ok(StatePairSearch::Unique((history, state, previous)))
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
) -> Result<Option<HistoryStatePair<'a>>, cadmpeg_core::CodecError> {
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
            if derived == linked {
                Ok(Some(derived))
            } else {
                Ok(None)
            }
        }
        (Some(derived), _) => Ok(Some(derived)),
        (None, Some(linked)) => Ok(Some(linked)),
        (None, None) => Ok(None),
    }
}

/// Design scopes by record index, for operands that name their scope by
/// stream and record.
struct ScopesByRecord<'s, 'ctx> {
    scopes: HashMap<u32, Vec<&'s DesignScope>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'s, 'ctx> ScopesByRecord<'s, 'ctx> {
    fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        scopes: &'s [DesignScope],
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let operation = "index F3D scopes by record";
        let mut storage = ctx.reserve_scoped(0, operation)?;
        let mut by_record = HashMap::new();
        for scope in ctx.admit_iter(scopes, operation)? {
            storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut by_record,
                    scope.record_index,
                    scope,
                    operation,
                    operation,
                )
            })?;
        }
        Ok(Self {
            scopes: by_record,
            _storage: storage,
        })
    }

    /// The one scope with `record_index` in the stream of `operand_id`.
    fn find(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operand_id: &str,
        record_index: u32,
    ) -> Result<Option<&'s DesignScope>, cadmpeg_core::CodecError> {
        let Some(candidates) = self.scopes.get(&record_index) else {
            return Ok(None);
        };
        let stream = native_stream_of(ctx, operand_id)?;
        let same_stream = |scope: &&DesignScope| {
            ctx.equal(
                &native_stream_of(ctx, &scope.id)?,
                &stream,
                "compare F3D operand scope stream",
            )
        };
        let operation = "find F3D operand scope";
        let Some(index) = ctx.position_by(candidates, same_stream, operation)? else {
            return Ok(None);
        };
        if ctx.any_by(&candidates[index + 1..], same_stream, operation)? {
            return Ok(None);
        }
        Ok(Some(candidates[index]))
    }
}

/// Operand groups by owning scope record and group record.
struct GroupsByRecord<'g, 'ctx> {
    groups: HashMap<(u32, u32), Vec<&'g OperandGroup>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'g, 'ctx> GroupsByRecord<'g, 'ctx> {
    fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        groups: &'g [OperandGroup],
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let operation = "index F3D operand groups by record";
        let mut storage = ctx.reserve_scoped(0, operation)?;
        let mut by_record = HashMap::new();
        for group in ctx.admit_iter(groups, operation)? {
            storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut by_record,
                    (group.scope_record_index, group.record_index),
                    group,
                    operation,
                    operation,
                )
            })?;
        }
        Ok(Self {
            groups: by_record,
            _storage: storage,
        })
    }

    /// The one group of `scope` with `record_index` in `stream` that passes
    /// `accept`.
    fn find(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        stream: Option<&str>,
        scope: &DesignScope,
        record_index: u32,
        mut accept: impl FnMut(&OperandGroup) -> bool,
    ) -> Result<Option<&'g OperandGroup>, cadmpeg_core::CodecError> {
        let Some(candidates) = self.groups.get(&(scope.record_index, record_index)) else {
            return Ok(None);
        };
        let mut matches = |group: &&OperandGroup| {
            Ok(accept(group)
                && ctx.equal(
                    &native_stream_of(ctx, &group.id)?,
                    &stream,
                    "compare F3D operand group stream",
                )?)
        };
        let operation = "find F3D operand group";
        let Some(index) = ctx.position_by(candidates, &mut matches, operation)? else {
            return Ok(None);
        };
        if ctx.any_by(&candidates[index + 1..], &mut matches, operation)? {
            return Ok(None);
        }
        Ok(Some(candidates[index]))
    }
}

/// The ROLE_0X10 group that lists `operand` at its member ordinal, in the
/// operand's own scope and stream.
fn exact_face_selection_group<'g>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    scope: &DesignScope,
    groups: &GroupsByRecord<'g, '_>,
) -> Result<Option<&'g OperandGroup>, cadmpeg_core::CodecError> {
    let Some(stream) = native_stream_of(ctx, &operand.id)? else {
        return Ok(None);
    };
    if !ctx.equal(
        &native_stream_of(ctx, &scope.id)?,
        &Some(stream),
        "compare F3D face selection group scope stream",
    )? {
        return Ok(None);
    }
    let (Some(group_record_index), Some(ordinal)) =
        (operand.group_record_index(), operand.group_member_ordinal())
    else {
        return Ok(None);
    };
    let Ok(ordinal) = usize::try_from(ordinal) else {
        return Ok(None);
    };
    groups.find(ctx, Some(stream), scope, group_record_index, |group| {
        group.role() == DesignOperandRole::ROLE_0X10
            && group.members().get(ordinal).map(|member| member.value)
                == Some(operand.record_index())
    })
}

/// A topology's live persistent tags, keyed for recipe reference binding.
struct RecipeTopologyIndex<'t, 'ctx> {
    /// Tags of live faces and edges by token and design reference, in tag order.
    tags: HashMap<
        (&'t str, i64),
        Vec<&'t crate::history_records::AsmHistoricalPersistentSubentityTag>,
    >,
    /// Live face slots by tagged design reference, in tag order.
    face_tags: HashMap<i64, Vec<i64>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn recipe_topology_index<'t, 'ctx>(
    decode: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    topology: &'t AsmHistoricalTopology,
) -> Result<RecipeTopologyIndex<'t, 'ctx>, cadmpeg_core::CodecError> {
    let (live_faces, _live_faces_storage) =
        decode.with_scoped_storage("index F3D live recipe faces", || {
            decode.collect_hash_set(
                topology.faces.iter().copied(),
                "index F3D live recipe faces",
            )
        })?;
    let (live_edges, _live_edges_storage) =
        decode.with_scoped_storage("index F3D live recipe edges", || {
            decode.collect_hash_set(
                topology.edges.iter().copied(),
                "index F3D live recipe edges",
            )
        })?;
    let operation = "index F3D recipe reference tags";
    let mut storage = decode.reserve_scoped(0, operation)?;
    let mut tags = HashMap::<_, Vec<&_>>::new();
    let mut face_tags = HashMap::new();
    for tag in decode.admit_iter(
        &topology.persistent_subentity_tags,
        "scan F3D topology persistent subentity tags",
    )? {
        let face = match tag.entity_kind {
            AsmHistoricalEntityKind::Face if live_faces.contains(&tag.entity_ref) => true,
            AsmHistoricalEntityKind::Edge if live_edges.contains(&tag.entity_ref) => false,
            _ => continue,
        };
        for &design_reference in decode.admit_iter(&tag.design_references, operation)? {
            let key = (tag.token.as_str(), design_reference);
            // A tag that repeats a design reference matches it once.
            if decode
                .get_hash_map(&tags, &key, operation)?
                .and_then(|group| group.last())
                .is_some_and(|last| std::ptr::eq(*last, tag))
            {
                continue;
            }
            storage.with_storage(|| {
                decode.push_hash_group(&mut tags, key, tag, operation, operation)?;
                if face {
                    decode.push_hash_group(
                        &mut face_tags,
                        design_reference,
                        tag.entity_ref,
                        operation,
                        operation,
                    )?;
                }
                Ok::<(), cadmpeg_core::CodecError>(())
            })?;
        }
    }
    Ok(RecipeTopologyIndex {
        tags,
        face_tags,
        _storage: storage,
    })
}

/// A value derived from the most recently used topology. Consecutive operands
/// usually share their input state, so one cached value serves a run of them;
/// switching topology releases the old value before building the new one.
struct RecentTopologyValue<T> {
    topology: usize,
    value: Option<T>,
}

impl<T> Default for RecentTopologyValue<T> {
    fn default() -> Self {
        Self {
            topology: 0,
            value: None,
        }
    }
}

impl<T> RecentTopologyValue<T> {
    fn of(
        &mut self,
        topology: &AsmHistoricalTopology,
        build: impl FnOnce() -> Result<T, cadmpeg_core::CodecError>,
    ) -> Result<&T, cadmpeg_core::CodecError> {
        let address = std::ptr::from_ref(topology).addr();
        let value = match self.value.take() {
            Some(value) if self.topology == address => value,
            _ => {
                let value = build()?;
                self.topology = address;
                value
            }
        };
        Ok(self.value.insert(value))
    }
}

/// Bind one recipe reference to every live face or edge fragment carrying its
/// token and Design reference in the recipe-state topology.
fn bind_historical_recipe_reference_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    reference: &mut crate::records::dimensions::DesignRecipeReference,
    index: &RecipeTopologyIndex<'_, '_>,
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
    let Some(tags) = decode.get_hash_map(
        &index.tags,
        &(reference.token.as_str(), reference.design_reference),
        "find F3D recipe reference tags",
    )?
    else {
        return Ok(());
    };
    for tag in decode.admit_iter(tags, "scan F3D recipe reference tags")? {
        if tag.entity_kind == AsmHistoricalEntityKind::Face {
            decode.reserve_vec(
                &mut reference.candidate_faces,
                1,
                "collect F3D recipe reference faces",
            )?;
            reference
                .candidate_faces
                .push(historical_face_id(decode, tag.entity_ref)?);
        } else {
            decode.reserve_vec(
                &mut reference.candidate_edges,
                1,
                "collect F3D recipe reference edges",
            )?;
            reference
                .candidate_edges
                .push(historical_edge_id(decode, tag.entity_ref)?);
        }
    }
    decode.stable_sort_by(
        &mut reference.candidate_faces,
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D recipe reference faces",
    )?;
    decode.dedup_vec(
        &mut reference.candidate_faces,
        "deduplicate F3D recipe reference faces",
    )?;
    decode.stable_sort_by(
        &mut reference.candidate_edges,
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D recipe reference edges",
    )?;
    decode.dedup_vec(
        &mut reference.candidate_edges,
        "deduplicate F3D recipe reference edges",
    )?;
    Ok(())
}

fn historical_recipe_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    design_reference: i64,
    index: &RecipeTopologyIndex<'_, '_>,
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    let Some(slots) = decode.get_hash_map(
        &index.face_tags,
        &design_reference,
        "find F3D historical recipe faces",
    )?
    else {
        return Ok(Vec::new());
    };
    let mut faces = decode.collection_vec(slots.len(), "collect F3D historical recipe faces")?;
    for &slot in decode.admit_iter(slots, "scan F3D historical recipe faces")? {
        faces.push(historical_face_id(decode, slot)?);
    }
    decode.stable_sort_by(
        &mut faces,
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D historical recipe faces",
    )?;
    decode.dedup_vec(&mut faces, "deduplicate F3D historical recipe faces")?;
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
    for reference in decode.admit_iter(references, "scan F3D direct face recipe references")? {
        if reference.design_reference != i64::from(recipe_record_index) {
            continue;
        }
        for face in decode.admit_iter(
            &reference.candidate_faces,
            "scan F3D direct face recipe candidates",
        )? {
            decode.reserve_vec(&mut faces, 1, "collect F3D direct face recipe candidates")?;
            faces.push(face.try_clone_for_decode(decode, "copy F3D historical face identity")?);
        }
    }
    decode.stable_sort_by(
        &mut faces,
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D direct face recipe candidates",
    )?;
    decode.dedup_vec(&mut faces, "deduplicate F3D direct face recipe candidates")?;
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

/// Copies the candidate faces that no recipe reference names.
fn unreferenced_candidate_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[cadmpeg_ir::ids::FaceId],
    references: &[crate::records::dimensions::DesignRecipeReference],
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    let operation = "index F3D referenced candidate faces";
    let mut referenced_storage = decode.reserve_scoped(0, operation)?;
    let mut referenced = HashSet::new();
    for reference in decode.admit_iter(references, operation)? {
        for face in decode.admit_iter(&reference.candidate_faces, operation)? {
            referenced_storage
                .with_storage(|| decode.insert_hash_set(&mut referenced, face, operation))?;
        }
    }
    let mut unreferenced = Vec::new();
    for face in decode.admit_iter(candidates, "scan F3D unreferenced candidate faces")? {
        if !decode.contains_hash_set(&referenced, face, operation)? {
            let face = face.try_clone_for_decode(decode, "copy F3D historical face identity")?;
            decode.push_vec(
                &mut unreferenced,
                face,
                "collect F3D unreferenced candidate faces",
            )?;
        }
    }
    Ok(unreferenced)
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
    let scope_records = ScopesByRecord::new(decode, scopes)?;
    let groups = GroupsByRecord::new(decode, operand_groups)?;
    let mut history_states = FeatureHistoryStates::new(decode)?;
    let mut recipe_index = RecentTopologyValue::default();
    for operand in decode.admit_iter(&mut *operands, "scan F3D face operands")? {
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
        let stream = native_stream_of(decode, &operand.id)?;
        let Some(scope) = scope_records.find(decode, &operand.id, operand.scope_record_index)?
        else {
            continue;
        };
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
        let Some(states) = history_states.states_of(decode, history)? else {
            continue;
        };
        let Some(topology) = previous.topology() else {
            continue;
        };
        let recipe_tags = recipe_index.of(topology, || recipe_topology_index(decode, topology))?;
        for reference in decode.admit_iter(
            &mut operand.recipe_references,
            "scan F3D face operand recipe references",
        )? {
            bind_historical_recipe_reference_candidates(decode, reference, recipe_tags)?;
        }
        if let Some(recipe_record_index) = decode.get_hash_map(
            &recipe_record_indices,
            operand.recipe_id.as_str(),
            "find F3D face operand recipe record",
        )? {
            operand.candidate_faces =
                historical_recipe_faces(decode, i64::from(*recipe_record_index), recipe_tags)?;
            operand.unreferenced_candidate_faces = unreferenced_candidate_faces(
                decode,
                &operand.candidate_faces,
                &operand.recipe_references,
            )?;
            decode.clear_vec(
                &mut operand.alternate_selector_candidate_faces,
                "clear F3D alternate selector face candidates",
            )?;
        }
        let (changed_faces, _changed_faces_storage) = decode
            .with_scoped_storage("collect F3D changed faces", || {
                face_changes_across_state_chain(decode, state, previous_state_id, states)
            })?;
        let Some(changed_faces) = changed_faces else {
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
            && exact_face_selection_group(decode, operand, scope, &groups)?.is_some()
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
            && exact_face_selection_group(decode, operand, scope, &groups)?.is_some()
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
            if groups
                .find(decode, stream, scope, group_record_index, |group| {
                    group.extrude_role().is_some_and(|role| {
                        matches!(
                            role,
                            crate::records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_)
                        )
                    }) && group.extrude_face_role().is_some()
                })?
                .is_none()
            {
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
            || match operand.group_record_index() {
                Some(group_record_index) => groups
                    .find(decode, stream, scope, group_record_index, |_| true)?
                    .is_some_and(|group| {
                        group.extrude_face_role()
                            == Some(crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Termination)
                    }),
                None => false,
            };
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &crate::history_records::AsmHistoricalTopology,
    face: i64,
) -> Result<Option<DraftSurfaceGeometry>, cadmpeg_core::CodecError> {
    let operation = "find F3D draft surface geometry";
    let Some(binding) = unique_by(
        ctx,
        &topology.face_surfaces,
        |binding| binding.entity == face,
        operation,
    )?
    else {
        return Ok(None);
    };
    let carrier = binding.carrier;
    if let Some(plane) = ctx.find_by(
        &topology.surface_planes,
        |surface| Ok(surface.surface == carrier),
        operation,
    )? {
        return Ok(Some(DraftSurfaceGeometry::Plane {
            origin: plane.origin,
            normal: plane.normal,
        }));
    }
    if let Some(cylinder) = ctx.find_by(
        &topology.surface_cylinders,
        |surface| Ok(surface.surface == carrier),
        operation,
    )? {
        return Ok(Some(DraftSurfaceGeometry::Cylinder {
            origin: cylinder.origin,
            axis: cylinder.axis,
            radius: cylinder.radius,
        }));
    }
    let Some(axis) = ctx.find_by(
        &topology.surface_axes,
        |surface| Ok(surface.surface == carrier),
        operation,
    )?
    else {
        return Ok(None);
    };
    let radius = ctx
        .find_by(
            &topology.surface_radii,
            |radius| Ok(radius.surface == carrier),
            operation,
        )?
        .map(|radius| radius.radius);
    Ok(Some(DraftSurfaceGeometry::Axis {
        origin: axis.origin,
        direction: axis.direction,
        radius,
    }))
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
    let mut storage = decode.reserve_scoped(0, "index F3D draft face candidates")?;
    let mut candidate_slots = HashSet::new();
    for face in decode.admit_iter(&operand.candidate_faces, "scan F3D draft face candidates")? {
        if let Some(face_slot) = stable_ref(decode, face.as_str())? {
            storage.with_storage(|| {
                decode.insert_hash_set(
                    &mut candidate_slots,
                    face_slot,
                    "index F3D draft face candidates",
                )
            })?;
        }
    }
    if candidate_slots.is_empty() {
        return Ok(None);
    }
    // Candidate faces named by each reference's alternate lane, else by its
    // exact lane.
    let mut lane_slots = |alternate: bool,
                          operation: &'static str|
     -> Result<BTreeSet<i64>, cadmpeg_core::CodecError> {
        let mut slots = BTreeSet::new();
        for reference in decode.admit_iter(&operand.recipe_references, operation)? {
            let faces = if alternate {
                &reference.alternate_selector_faces
            } else {
                &reference.candidate_faces
            };
            for face in decode.admit_iter(faces, operation)? {
                let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                    continue;
                };
                if candidate_slots.contains(&face_slot) {
                    decode.insert_scoped_btree_set(
                        &mut storage,
                        &mut slots,
                        face_slot,
                        operation,
                        operation,
                    )?;
                }
            }
        }
        Ok(slots)
    };
    let alternate_slots = lane_slots(true, "collect F3D draft alternate faces")?;
    let has_alternates = !alternate_slots.is_empty();
    let candidates = if has_alternates {
        alternate_slots
    } else {
        lane_slots(false, "collect F3D draft exact faces")?
    };
    let operation = "find F3D draft face in both states";
    if !has_alternates {
        // A one-entry set is a single leaf.
        let (1, Some(&face)) = (candidates.len(), candidates.first()) else {
            return Ok(None);
        };
        return Ok((decode.contains(&preceding.faces, &face, operation)?
            && decode.contains(&result.faces, &face, operation)?)
        .then_some(face));
    }
    let mut changed = None;
    for &face in decode.admit_iter(&candidates, "scan F3D draft surface candidates")? {
        if !decode.contains(&preceding.faces, &face, operation)?
            || !decode.contains(&result.faces, &face, operation)?
        {
            continue;
        }
        let (Some(before), Some(after)) = (
            draft_surface_geometry(decode, preceding, face)?,
            draft_surface_geometry(decode, result, face)?,
        ) else {
            continue;
        };
        if before != after && changed.replace(face).is_some() {
            return Ok(None);
        }
    }
    Ok(changed)
}

/// One surface radius per surface; a surface with several radii has none.
fn unique_surface_radii<'ctx>(
    decode: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Result<
    (
        HashMap<i64, Option<f64>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    cadmpeg_core::CodecError,
> {
    decode.unique_index(
        topology
            .surface_radii
            .iter()
            .map(|radius| (radius.surface, radius.radius)),
        "index F3D surface radii",
    )
}

fn resolve_pattern_face_by_surface_radius(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[cadmpeg_ir::ids::FaceId],
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let mut storage = decode.reserve_scoped(0, "index F3D pattern face candidates")?;
    let mut candidate_faces = HashSet::new();
    for face in decode.admit_iter(candidates, "scan F3D pattern face candidates")? {
        if let Some(face_slot) = stable_ref(decode, face.as_str())? {
            storage.with_storage(|| {
                decode.insert_hash_set(
                    &mut candidate_faces,
                    face_slot,
                    "index F3D pattern face candidates",
                )
            })?;
        }
    }
    if candidate_faces.is_empty() {
        return Ok(None);
    }
    let (result_radii, _result_radii_storage) = unique_surface_radii(decode, result)?;
    let mut result_radius = None;
    let mut bound_candidates = HashSet::new();
    for binding in decode
        .admit_iter(
            &result.face_surfaces,
            "scan F3D pattern result face surfaces",
        )?
        .filter(|binding| candidate_faces.contains(&binding.entity))
    {
        if !storage.with_storage(|| {
            decode.insert_hash_set(
                &mut bound_candidates,
                binding.entity,
                "index F3D pattern bound faces",
            )
        })? {
            return Ok(None);
        }
        let Some(&Some(radius)) = result_radii.get(&binding.carrier) else {
            return Ok(None);
        };
        if !radius.is_finite() || radius <= 0.0 {
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
    let (preceding_radii, _preceding_radii_storage) = unique_surface_radii(decode, preceding)?;
    let mut matched = None;
    for binding in decode
        .admit_iter(
            &preceding.face_surfaces,
            "scan F3D pattern preceding face surfaces",
        )?
        .filter(|binding| changed_faces.contains(&binding.entity))
    {
        let Some(&Some(radius)) = preceding_radii.get(&binding.carrier) else {
            continue;
        };
        if radius.to_bits() == result_radius && matched.replace(binding.entity).is_some() {
            return Ok(None);
        }
    }
    Ok(matched)
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
    let (face_carriers, _face_carriers_storage) = decode.unique_index(
        topology
            .face_surfaces
            .iter()
            .map(|binding| (binding.entity, binding.carrier)),
        "index F3D Thread face carriers",
    )?;
    let (cylinders, _cylinders_storage) = decode.unique_index(
        topology
            .surface_cylinders
            .iter()
            .map(|cylinder| (cylinder.surface, cylinder.radius)),
        "index F3D Thread cylinders",
    )?;
    // Every changed face on one cylinder within the thread diameters must be
    // the same face.
    let mut matched = None;
    for &face in decode
        .admit_iter(&topology.faces, "scan F3D Thread transition faces")?
        .filter(|face| changed_faces.contains(face))
    {
        let Some(&Some(carrier)) = face_carriers.get(&face) else {
            continue;
        };
        let Some(&Some(radius)) = cylinders.get(&carrier) else {
            continue;
        };
        if radius + tolerance < minimum_radius || radius > maximum_radius + tolerance {
            continue;
        }
        if matched.is_some_and(|matched| matched != face) {
            return Ok(None);
        }
        matched = Some(face);
    }
    Ok(matched)
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
        if !changed_faces.contains(&candidate)
            || !decode.contains(
                &topology.faces,
                &candidate,
                "check F3D grouped topology face membership",
            )?
        {
            continue;
        }
        if face.is_some_and(|expected| expected != candidate) {
            return Ok(None);
        }
        face = Some(candidate);
    }
    face.map(|face| historical_face_id(decode, face))
        .transpose()
}

fn resolve_bounded_face_recipe_target<'r>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &'r crate::history_records::AsmHistoricalTopology,
    inserted_bodies: &[i64],
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { header_value }) =
        crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)
    else {
        return Ok(None);
    };
    if operand.recipe_nodes.len() != header_value
        || decode.any_by(
            &operand.recipe_nodes,
            |node| Ok(node.recipe_structure.is_none()),
            "check F3D bounded recipe nodes",
        )?
    {
        return Ok(None);
    }
    let Some(first) = operand.recipe_references.first() else {
        return Ok(None);
    };
    let first_clause_len = decode
        .position_by(
            &operand.recipe_references,
            |reference| {
                Ok(reference.selector_offset != first.selector_offset
                    || reference.token_offset != first.token_offset)
            },
            "measure F3D bounded first clause",
        )?
        .unwrap_or(operand.recipe_references.len());
    let first_clause = &operand.recipe_references[..first_clause_len];
    let mut storage = decode.reserve_scoped(0, "index F3D bounded topology faces")?;
    let topology_faces = storage.with_storage(|| {
        decode.collect_hash_set(
            preceding.faces.iter().copied(),
            "index F3D bounded topology faces",
        )
    })?;
    let mut target_candidates = BTreeSet::new();
    if let Some(reference) = first_clause.first() {
        for face in
            decode.admit_iter(effective_faces(reference), "scan F3D bounded target faces")?
        {
            let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                continue;
            };
            if topology_faces.contains(&face_slot) {
                decode.insert_scoped_btree_set(
                    &mut storage,
                    &mut target_candidates,
                    face_slot,
                    "find F3D bounded target face",
                    "collect F3D bounded target faces",
                )?;
            }
        }
    }
    for reference in decode.admit_iter(&first_clause[1..], "scan F3D bounded face clauses")? {
        let mut clause_storage = decode.reserve_scoped(0, "collect F3D bounded clause faces")?;
        let mut candidates = HashSet::new();
        for face in
            decode.admit_iter(effective_faces(reference), "scan F3D bounded clause faces")?
        {
            let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                continue;
            };
            if topology_faces.contains(&face_slot) {
                clause_storage.with_storage(|| {
                    decode.insert_hash_set(
                        &mut candidates,
                        face_slot,
                        "collect F3D bounded clause faces",
                    )
                })?;
            }
        }
        decode.retain_btree_set(
            &mut target_candidates,
            |face| Ok(candidates.contains(face)),
            "intersect F3D bounded clause faces",
        )?;
    }
    let operation = "index F3D bounded result relations";
    let relations = |relations: &'r [crate::history_records::AsmHistoricalRelation]| {
        decode.unique_index(
            relations
                .iter()
                .map(|relation| (relation.owner_ref, relation.member_refs.as_slice())),
            operation,
        )
    };
    let (body_regions, _body_regions_storage) = relations(&result.body_regions)?;
    let (region_shells, _region_shells_storage) = relations(&result.region_shells)?;
    let (shell_faces, _shell_faces_storage) = relations(&result.shell_faces)?;
    let mut construction_faces = Vec::new();
    'bodies: for body in decode.admit_iter(inserted_bodies, "scan F3D inserted bodies")? {
        let Some(&Some(regions)) = body_regions.get(body) else {
            continue;
        };
        let mut unique = None;
        let mut ambiguous = false;
        for region in decode.admit_iter(regions, "scan F3D inserted body regions")? {
            let Some(&Some(shells)) = region_shells.get(region) else {
                continue 'bodies;
            };
            for shell in decode.admit_iter(shells, "scan F3D inserted body shells")? {
                let Some(&Some(faces)) = shell_faces.get(shell) else {
                    continue 'bodies;
                };
                for face in decode.admit_iter(faces, "scan F3D inserted body faces")? {
                    if unique.is_some_and(|prior| prior != *face) {
                        ambiguous = true;
                    }
                    unique = Some(*face);
                }
            }
        }
        if !ambiguous {
            if let Some(face) = unique {
                decode.push_scoped_vec(
                    &mut storage,
                    &mut construction_faces,
                    face,
                    "collect F3D construction faces",
                )?;
            }
        }
    }
    if construction_faces.is_empty() {
        return Ok(None);
    }
    let face_loop_positions = |face,
                               topology,
                               storage: &mut cadmpeg_core::decode::ScopedReservation<'_>|
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
        let positions = storage.with_storage(|| {
            decode.collect_vec(
                rows.iter().map(|row| row.position),
                "collect F3D bounded loop positions",
            )
        })?;
        Ok(Some((rows.len(), positions)))
    };
    let mut matches = Vec::new();
    for candidate in decode.admit_iter(target_candidates, "scan F3D bounded target candidates")? {
        let mut positions_storage =
            decode.reserve_scoped(0, "collect F3D bounded loop positions")?;
        let Some((edge_count, candidate_points)) =
            face_loop_positions(candidate, preceding, &mut positions_storage)?
        else {
            continue;
        };
        if edge_count != header_value {
            continue;
        }
        let matched = decode.any_by(
            &construction_faces,
            |face| {
                let mut construction_storage =
                    decode.reserve_scoped(0, "collect F3D bounded loop positions")?;
                let Some((construction_edge_count, construction_points)) =
                    face_loop_positions(*face, result, &mut construction_storage)?
                else {
                    return Ok(false);
                };
                Ok(construction_edge_count >= edge_count
                    && cyclic_point_subsequence(decode, &candidate_points, &construction_points)?)
            },
            "scan F3D construction faces",
        )?;
        if matched {
            decode.push_scoped_vec(
                &mut storage,
                &mut matches,
                candidate,
                "collect F3D bounded face matches",
            )?;
        }
    }
    // Candidates come from an ordered set, so the matches are distinct and in order.
    let [face] = matches.as_slice() else {
        return Ok(None);
    };
    Ok(Some(*face))
}

/// Whether `candidate` occurs, in order or reversed, as a cyclic subsequence
/// of `construction` under the coincidence tolerance. Each start position and
/// each cursor step is charged as it is examined.
fn cyclic_point_subsequence(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidate: &[cadmpeg_ir::math::Point3],
    construction: &[cadmpeg_ir::math::Point3],
) -> Result<bool, cadmpeg_core::CodecError> {
    let coincident = |left: &cadmpeg_ir::math::Point3, right: &cadmpeg_ir::math::Point3| {
        let dx = left.x - right.x;
        let dy = left.y - right.y;
        let dz = left.z - right.z;
        dx.mul_add(dx, dy.mul_add(dy, dz * dz)) <= EPS_HISTORY_CYCLIC_POINT_SUBSEQUENCE_E12
    };
    let (Some(first), Some(last)) = (candidate.first(), candidate.last()) else {
        return Ok(false);
    };
    if candidate.len() > construction.len() {
        return Ok(false);
    }
    let operation = "match F3D bounded loop positions";
    let length = construction.len();
    for reversed in [false, true] {
        let start_point = if reversed { last } else { first };
        let mut starts = 0..length;
        while let Some(start) = ctx.next_charged(&mut starts, operation)? {
            if !coincident(start_point, &construction[start]) {
                continue;
            }
            // Each remaining candidate point must appear after the previous
            // one within one turn of the loop.
            let mut cursor = start;
            let limit = start + length;
            let mut targets = 1..candidate.len();
            let mut matched = true;
            while let Some(offset) = ctx.next_charged(&mut targets, operation)? {
                let target = if reversed {
                    &candidate[candidate.len() - 1 - offset]
                } else {
                    &candidate[offset]
                };
                let mut found = false;
                while cursor < limit {
                    ctx.charge_work(1, operation)?;
                    cursor += 1;
                    if coincident(target, &construction[cursor % length]) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    matched = false;
                    break;
                }
            }
            if matched {
                return Ok(true);
            }
        }
    }
    Ok(false)
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
    let scopes = ScopesByRecord::new(decode, scopes)?;
    let mut history_states = FeatureHistoryStates::new(decode)?;
    let mut topology_faces = RecentTopologyValue::default();
    let mut closures = RecentTopologyValue::default();
    for operand in decode.admit_iter(&mut *operands, "scan F3D body recipe operands")? {
        {
            let mut bindings = operand.reference_bindings_mut();
            while let Some(reference) =
                decode.next_charged(&mut bindings, "clear F3D body recipe reference bindings")?
            {
                decode.clear_vec(
                    reference.preceding_candidate_faces,
                    "clear F3D preceding body recipe face candidates",
                )?;
                reference.preceding_body_slots.clear();
            }
        }
        operand.resolved_face_slot = None;
        operand.resolved_body_state_id = None;
        operand.resolved_body_slot = None;
        operand.resolved_body_face_slots.clear();
        let Some((history, state, previous)) =
            body_recipe_operand_history_pair(decode, operand, &scopes, histories)?
        else {
            continue;
        };
        let Some(states) = history_states.states_of(decode, history)? else {
            continue;
        };
        let Some(topology) = previous.topology() else {
            continue;
        };
        let (changed_faces, _changed_faces_storage) = decode
            .with_scoped_storage("collect F3D changed faces", || {
                face_changes_across_state_chain(decode, state, previous.state_id, states)
            })?;
        if changed_faces.is_none() {
            continue;
        }
        let Some(source) = historical_brep_source(decode, &previous.id)? else {
            continue;
        };
        let (faces, _) = topology_faces.of(topology, || {
            decode.with_scoped_storage("index F3D topology faces", || {
                decode.collect_hash_set(topology.faces.iter().copied(), "index F3D topology faces")
            })
        })?;
        let closures = closures.of(topology, || body_closures(decode, topology))?;
        {
            let mut bindings = operand.reference_bindings_mut();
            while let Some(reference) =
                decode.next_charged(&mut bindings, "scan F3D body recipe reference bindings")?
            {
                let mut face_slots_storage =
                    decode.reserve_scoped(0, "index F3D body recipe face slots")?;
                let mut face_slots = BTreeSet::new();
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
                    if faces.contains(&face_slot) {
                        let face =
                            face.try_clone_for_decode(decode, "copy F3D historical face identity")?;
                        decode.push_vec(
                            &mut preceding_candidate_faces,
                            face,
                            "collect F3D faces in topology",
                        )?;
                        decode.insert_scoped_btree_set(
                            &mut face_slots_storage,
                            &mut face_slots,
                            face_slot,
                            "index F3D body recipe face slots",
                            "index F3D body recipe face slots",
                        )?;
                    }
                }
                *reference.preceding_candidate_faces = preceding_candidate_faces;
                let Some(body_slots) = face_slots_storage
                    .with_storage(|| closures_intersecting(decode, closures, &face_slots))?
                else {
                    continue;
                };
                let mut slots = decode
                    .collection_vec(body_slots.len(), "collect F3D body recipe preceding bodies")?;
                for body in
                    decode.admit_iter(body_slots, "scan F3D body recipe preceding bodies")?
                {
                    slots.push(body);
                }
                *reference.preceding_body_slots = slots;
            }
        }
        if let [reference] = operand.references().as_slice() {
            if let [face] = reference.preceding_candidate_faces.as_slice() {
                operand.resolved_face_slot = stable_ref(decode, face.as_str())?;
            }
        }
        let Some((first, rest)) = operand.references().split_first() else {
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
        let mut intersection_storage =
            decode.reserve_scoped(0, "index F3D body recipe intersection")?;
        let mut intersection = BTreeSet::new();
        for &body in decode.admit_iter(
            &first.preceding_body_slots,
            "scan F3D first preceding body slots",
        )? {
            decode.insert_scoped_btree_set(
                &mut intersection_storage,
                &mut intersection,
                body,
                "index F3D body recipe intersection",
                "index F3D body recipe intersection",
            )?;
        }
        for reference in decode.admit_iter(rest, "scan F3D body recipe intersection references")? {
            decode.retain_btree_set(
                &mut intersection,
                |body| {
                    decode.contains(
                        &reference.preceding_body_slots,
                        body,
                        "check F3D body recipe intersection membership",
                    )
                },
                "intersect F3D body recipe references",
            )?;
        }
        if intersection.len() == 1 {
            operand.resolved_body_slot = intersection.pop_first();
        }
    }
    let recipes_by_id = body_recipe_identity_index(recipes);
    let identity = |operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand| -> Result<Option<_>, cadmpeg_core::CodecError> {
        let Some(stream) = native_stream_of(decode, &operand.id)? else { return Ok(None) };
        let Some(recipe) = recipes_by_id.get(decode, operand.recipe_id.as_str())? else {
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
    let mut body_faces = RecentTopologyValue::default();
    for operand in decode.admit_iter(operands, "bind F3D body recipe operand faces")? {
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
            body_recipe_operand_history_pair(decode, operand, &scopes, histories)?
        else {
            continue;
        };
        let Some(topology) = previous.topology() else {
            continue;
        };
        let index = body_faces.of(topology, || body_face_index(decode, topology))?;
        let Some(faces) = complete_body_face_slots(decode, index, body_slot)? else {
            continue;
        };
        operand.resolved_body_state_id = Some(previous.state_id);
        operand.resolved_body_face_slots = faces;
    }
    Ok(())
}

/// Construction recipes by native ID; a repeated ID names no recipe.
fn body_recipe_identity_index<'a, 'ctx>(
    recipes: &'a [crate::records::recipes::ConstructionRecipe],
) -> UniqueIndex<'a, 'ctx, &'a str, crate::records::recipes::ConstructionRecipe> {
    UniqueIndex::new(
        recipes,
        |_, recipe| Ok(Some(recipe.id.as_str())),
        "index F3D body recipes by id",
    )
}

fn body_recipe_operand_history_pair<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    scopes: &ScopesByRecord<'_, '_>,
    histories: &'a [AsmHistory],
) -> Result<Option<HistoryStatePair<'a>>, cadmpeg_core::CodecError> {
    if native_stream_of(decode, &operand.id)?.is_none() {
        return Ok(None);
    }
    let Some(scope) = scopes.find(decode, &operand.id, operand.scope_record_index)? else {
        return Ok(None);
    };
    let Some(state_id) = scope.history_state_id() else {
        return Ok(None);
    };
    let Some(previous_state_id) =
        effective_scope_previous_history_state_id(decode, scope, histories)?
    else {
        return Ok(None);
    };
    unique_history_state_pair(decode, histories, state_id, previous_state_id)
}

/// One topology's entity occurrence counts and body, region and shell
/// relations, indexed for complete-body face queries. A relation owner or
/// member listed twice resolves to none.
struct BodyFaceIndex<'a, 'ctx> {
    /// Occurrences of each body, region, shell and face slot, by family.
    counts: [HashMap<i64, usize>; 4],
    body_regions: RelationIndex<'a>,
    region_shells: RelationIndex<'a>,
    shell_faces: RelationIndex<'a>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

struct RelationIndex<'a> {
    members_by_owner: HashMap<i64, Option<&'a [i64]>>,
    owner_by_member: HashMap<i64, Option<i64>>,
}

fn body_face_index<'a, 'ctx>(
    decode: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    topology: &'a AsmHistoricalTopology,
) -> Result<BodyFaceIndex<'a, 'ctx>, cadmpeg_core::CodecError> {
    let mut storage = decode.reserve_scoped(0, "index F3D complete body entity counts")?;
    let mut occurrence_counts = |slots: &[i64]| {
        let mut counts = HashMap::new();
        for &slot in decode.admit_iter(slots, "scan F3D topology member slots")? {
            if let Some(count) = counts.get_mut(&slot) {
                *count += 1;
                continue;
            }
            storage.with_storage(|| {
                decode
                    .insert_hash_map(
                        &mut counts,
                        slot,
                        1_usize,
                        "index F3D complete body entity counts",
                    )
                    .map(|_| ())
            })?;
        }
        Ok::<_, cadmpeg_core::CodecError>(counts)
    };
    let counts = [
        occurrence_counts(&topology.bodies)?,
        occurrence_counts(&topology.regions)?,
        occurrence_counts(&topology.shells)?,
        occurrence_counts(&topology.faces)?,
    ];
    let mut relation_index = |relations: &'a [AsmHistoricalRelation]| {
        let mut members_by_owner = HashMap::new();
        let mut owner_by_member = HashMap::new();
        for relation in decode.admit_iter(relations, "scan F3D complete body relations")? {
            if let Some(members) = members_by_owner.get_mut(&relation.owner_ref) {
                *members = None;
            } else {
                storage.with_storage(|| {
                    decode
                        .insert_hash_map(
                            &mut members_by_owner,
                            relation.owner_ref,
                            Some(relation.member_refs.as_slice()),
                            "index F3D complete body relation owners",
                        )
                        .map(|_| ())
                })?;
            }
            for &member in
                decode.admit_iter(&relation.member_refs, "scan F3D relation member refs")?
            {
                if let Some(owner) = owner_by_member.get_mut(&member) {
                    *owner = None;
                    continue;
                }
                storage.with_storage(|| {
                    decode
                        .insert_hash_map(
                            &mut owner_by_member,
                            member,
                            Some(relation.owner_ref),
                            "index F3D complete body relation members",
                        )
                        .map(|_| ())
                })?;
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(RelationIndex {
            members_by_owner,
            owner_by_member,
        })
    };
    let body_regions = relation_index(&topology.body_regions)?;
    let region_shells = relation_index(&topology.region_shells)?;
    let shell_faces = relation_index(&topology.shell_faces)?;
    Ok(BodyFaceIndex {
        counts,
        body_regions,
        region_shells,
        shell_faces,
        _storage: storage,
    })
}

fn complete_body_face_slots(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    index: &BodyFaceIndex<'_, '_>,
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
    let [body_counts, region_counts, shell_counts, face_counts] = &index.counts;
    let count_is_one =
        |counts: &HashMap<i64, usize>, slot: i64| counts.get(&slot).copied() == Some(1);
    if !count_is_one(body_counts, body) {
        return Ok(None);
    }
    let regions = complete_some!(index
        .body_regions
        .members_by_owner
        .get(&body)
        .copied()
        .flatten());
    if regions.is_empty() {
        return Ok(None);
    }
    let mut storage = decode.reserve_scoped(0, "collect F3D complete body faces")?;
    let mut seen_regions = HashSet::new();
    let mut seen_shells = HashSet::new();
    let mut seen_faces = BTreeSet::new();
    for &region in decode.admit_iter(regions, "scan F3D complete body regions")? {
        if !storage.with_storage(|| {
            decode.insert_hash_set(
                &mut seen_regions,
                region,
                "collect F3D complete body regions",
            )
        })? || !count_is_one(region_counts, region)
            || index
                .body_regions
                .owner_by_member
                .get(&region)
                .copied()
                .flatten()
                != Some(body)
        {
            return Ok(None);
        }
        let shells = complete_some!(index
            .region_shells
            .members_by_owner
            .get(&region)
            .copied()
            .flatten());
        if shells.is_empty() {
            return Ok(None);
        }
        for &shell in decode.admit_iter(shells, "scan F3D complete body shells")? {
            if !storage.with_storage(|| {
                decode.insert_hash_set(&mut seen_shells, shell, "collect F3D complete body shells")
            })? || !count_is_one(shell_counts, shell)
                || index
                    .region_shells
                    .owner_by_member
                    .get(&shell)
                    .copied()
                    .flatten()
                    != Some(region)
            {
                return Ok(None);
            }
            let faces = complete_some!(index
                .shell_faces
                .members_by_owner
                .get(&shell)
                .copied()
                .flatten());
            if faces.is_empty() {
                return Ok(None);
            }
            for &face in decode.admit_iter(faces, "scan F3D complete body faces")? {
                if !decode.insert_scoped_btree_set(
                    &mut storage,
                    &mut seen_faces,
                    face,
                    "collect F3D complete body faces",
                    "collect F3D complete body faces",
                )? || !count_is_one(face_counts, face)
                    || index
                        .shell_faces
                        .owner_by_member
                        .get(&face)
                        .copied()
                        .flatten()
                        != Some(shell)
                {
                    return Ok(None);
                }
            }
        }
    }
    // The ordered set yields the face slots already sorted.
    let mut faces =
        decode.collection_vec(seen_faces.len(), "collect F3D complete body face slots")?;
    for face in decode.admit_iter(seen_faces, "scan F3D complete body face slots")? {
        faces.push(face);
    }
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
    let mut history_states = FeatureHistoryStates::new(decode)?;
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
            let Some(states) = history_states.states_of(decode, history)? else {
                continue;
            };
            let (changed_faces, _changed_faces_storage) = decode
                .with_scoped_storage("collect F3D changed faces", || {
                    face_changes_across_state_chain(decode, state, previous_state_id, states)
                })?;
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

/// The faces deleted or updated on the transition chain from `state` back to
/// `previous_state_id`, or `None` when the chain breaks or repeats a state.
/// The returned set is charged to the caller's storage scope.
fn face_changes_across_state_chain<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Result<Option<HashSet<i64>>, cadmpeg_core::CodecError> {
    let mut visited_storage = decode.reserve_scoped(0, "track F3D face change state chain")?;
    let mut current = state;
    let mut visited = HashSet::new();
    let mut changed = HashSet::new();
    while current.state_id != previous_state_id {
        if !visited_storage.with_storage(|| {
            decode.insert_hash_set(
                &mut visited,
                current.state_id,
                "track F3D face change state chain",
            )
        })? {
            return Ok(None);
        }
        let Some(transition) = current.transition.as_ref() else {
            return Ok(None);
        };
        let faces = &transition.topology.faces;
        for face in decode
            .admit_iter(&faces.deleted, "scan F3D changed transition faces")?
            .chain(decode.admit_iter(&faces.updated, "scan F3D changed transition faces")?)
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

/// The edges deleted and updated on the transition chain from `state` back to
/// `previous_state_id`, or `None` when the chain breaks or repeats a state.
/// The returned sets are charged to the caller's storage scope.
fn edge_changes_across_state_chain<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Result<Option<EdgeChanges>, cadmpeg_core::CodecError> {
    let mut visited_storage = decode.reserve_scoped(0, "track F3D edge change state chain")?;
    let mut current = state;
    let mut visited = HashSet::new();
    let mut deleted = HashSet::new();
    let mut updated = HashSet::new();
    while current.state_id != previous_state_id {
        if !visited_storage.with_storage(|| {
            decode.insert_hash_set(
                &mut visited,
                current.state_id,
                "track F3D edge change state chain",
            )
        })? {
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
            |candidate| decode.contains(edges, candidate, "find F3D recipe side edge candidate"),
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
    let scope_records = ScopesByRecord::new(decode, scopes)?;
    let mut history_states = FeatureHistoryStates::new(decode)?;
    let mut recipe_index = RecentTopologyValue::default();
    let mut terminal_storage = decode.reserve_scoped(0, "collect F3D terminal topologies")?;
    let mut terminal_topologies = Vec::new();
    for history in decode.admit_iter(histories, "scan F3D edge operand histories")? {
        let mut preceding_storage = decode.reserve_scoped(0, "index F3D terminal predecessors")?;
        let mut preceding = HashSet::new();
        for state in decode.admit_iter(&history.states, "scan F3D history states")? {
            if let Some(previous) = state
                .transition
                .as_ref()
                .and_then(|transition| transition.previous_state_id)
            {
                preceding_storage.with_storage(|| {
                    decode.insert_hash_set(
                        &mut preceding,
                        previous,
                        "index F3D terminal predecessors",
                    )
                })?;
            }
        }
        let terminal = unique_by(
            decode,
            &history.states,
            |state| !preceding.contains(&state.state_id),
            "find F3D terminal history state",
        )?;
        if let Some(state) = terminal {
            if let Some(topology) = state.topology() {
                decode.push_scoped_vec(
                    &mut terminal_storage,
                    &mut terminal_topologies,
                    (state.state_id, topology),
                    "collect F3D terminal topologies",
                )?;
            }
        }
    }
    for operand in decode.admit_iter(operands, "scan F3D edge operands")? {
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
        let Some(scope) = scope_records.find(decode, &operand.id, operand.scope_record_index)?
        else {
            continue;
        };
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
        let recipe_tags = recipe_index.of(topology, || recipe_topology_index(decode, topology))?;
        for reference in decode.admit_iter(
            &mut operand.recipe_references,
            "scan F3D edge operand recipe references",
        )? {
            bind_historical_recipe_reference_candidates(decode, reference, recipe_tags)?;
        }
        if let Some(recipe_record_index) = decode.get_hash_map(
            &recipe_record_indices,
            operand.recipe_id.as_str(),
            "find F3D edge recipe record",
        )? {
            operand.candidate_faces =
                historical_recipe_faces(decode, i64::from(*recipe_record_index), recipe_tags)?;
        }
        let Some(states) = history_states.states_of(decode, history)? else {
            continue;
        };
        let (changed_faces, _changed_faces_storage) = decode
            .with_scoped_storage("collect F3D changed faces", || {
                face_changes_across_state_chain(decode, state, previous_state_id, states)
            })?;
        let Some(changed_faces) = changed_faces else {
            continue;
        };
        let (edge_changes, _edge_changes_storage) = decode
            .with_scoped_storage("collect F3D changed edges", || {
                edge_changes_across_state_chain(decode, state, previous_state_id, states)
            })?;
        let Some(EdgeChanges {
            deleted: chain_deleted_edges,
            updated: chain_updated_edges,
        }) = edge_changes
        else {
            continue;
        };
        let mut scratch = decode.reserve_scoped(0, "index F3D edge operand topology")?;
        let preceding_faces = scratch.with_storage(|| {
            decode.collect_hash_set(
                topology.faces.iter().copied(),
                "index F3D preceding edge faces",
            )
        })?;
        let mut inserted_faces = Vec::new();
        for &face in decode.admit_iter(&result_topology.faces, "scan F3D result topology faces")? {
            if !preceding_faces.contains(&face) {
                decode.push_scoped_vec(
                    &mut scratch,
                    &mut inserted_faces,
                    face,
                    "collect F3D inserted faces",
                )?;
            }
        }
        let result_edges = scratch.with_storage(|| {
            decode.collect_hash_set(
                result_topology.edges.iter().copied(),
                "index F3D result edges",
            )
        })?;
        let mut deleted_edges = Vec::new();
        let mut updated_edges = Vec::new();
        for &edge in decode.admit_iter(&topology.edges, "scan F3D preceding topology edges")? {
            let (target, operation) = if !result_edges.contains(&edge) {
                if !chain_deleted_edges.contains(&edge) {
                    continue;
                }
                (&mut deleted_edges, "collect F3D deleted edge candidates")
            } else {
                if !chain_deleted_edges.contains(&edge) && !chain_updated_edges.contains(&edge) {
                    continue;
                }
                (&mut updated_edges, "collect F3D updated edge candidates")
            };
            decode.push_scoped_vec(&mut scratch, target, edge, operation)?;
        }
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
        for edge in decode
            .admit_iter(&deleted_edges, "scan F3D changed edge candidates")?
            .chain(decode.admit_iter(&updated_edges, "scan F3D changed edge candidates")?)
        {
            scratch.with_storage(|| {
                decode.insert_hash_set(
                    &mut changed_edges,
                    *edge,
                    "index F3D changed edge candidates",
                )
            })?;
        }
        let mut changed_boundary_edge_slots = Vec::new();
        for &edge in decode.admit_iter(
            &operand.preceding_boundary_edge_slots,
            "scan F3D changed boundary edges",
        )? {
            if changed_edges.contains(&edge) {
                decode.push_vec(
                    &mut changed_boundary_edge_slots,
                    edge,
                    "collect F3D changed boundary edges",
                )?;
            }
        }
        operand.changed_boundary_edge_slots = changed_boundary_edge_slots;
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
        for (ordinal, reference) in decode
            .admit_iter(
                &operand.recipe_references,
                "scan F3D edge recipe references",
            )?
            .enumerate()
        {
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
            for edges in
                decode.admit_iter(&reference_edge_sets, "scan F3D sweep reference edges")?
            {
                for &edge in decode.admit_iter(edges, "scan F3D sweep reference edges")? {
                    decode.insert_scoped_btree_set(
                        &mut scratch,
                        &mut candidate_edges,
                        edge,
                        "index F3D sweep candidate edges",
                        "index F3D sweep candidate edges",
                    )?;
                }
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
            for edges in
                decode.admit_iter(&reference_edge_sets, "scan F3D revolve reference edges")?
            {
                for &edge in decode.admit_iter(edges, "scan F3D revolve reference edges")? {
                    decode.insert_scoped_btree_set(
                        &mut scratch,
                        &mut candidate_edges,
                        edge,
                        "index F3D revolve candidate edges",
                        "index F3D revolve candidate edges",
                    )?;
                }
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
        let mut changed_edge_contexts = Vec::new();
        for &edge in decode.admit_iter(&topology.edges, "scan F3D changed topology edges")? {
            if changed_edges.contains(&edge) {
                let context = selection::historical_edge_context(decode, edge, topology)?;
                decode.push_scoped_vec(
                    &mut scratch,
                    &mut changed_edge_contexts,
                    context,
                    "collect F3D changed edge contexts",
                )?;
            }
        }
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
    let (face_loops, _face_loops_storage) = decode.unique_index(
        topology
            .face_loops
            .iter()
            .map(|relation| (relation.owner_ref, relation.member_refs.as_slice())),
        "index F3D boundary face loops",
    )?;
    let (loop_coedges, _loop_coedges_storage) = decode.unique_index(
        topology
            .loop_coedges
            .iter()
            .map(|relation| (relation.owner_ref, relation.member_refs.as_slice())),
        "index F3D boundary loop coedges",
    )?;
    let (coedge_edges, _coedge_edges_storage) = decode.unique_index(
        topology
            .coedge_topology
            .iter()
            .map(|coedge| (coedge.coedge, coedge.edge)),
        "index F3D boundary coedge edges",
    )?;
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
    let mut affected_storage = ctx.reserve_scoped(0, "merge F3D affected history bodies")?;
    let (current_changes, _current_changes_storage) = ctx
        .with_scoped_storage("index F3D changed topology members", || {
            changed_family_refs(ctx, &transition.topology, false)
        })?;
    let Some(mut affected) = affected_storage
        .with_storage(|| bodies_intersecting(ctx, current_topology, &current_changes))?
    else {
        return Ok(None);
    };
    if let Some(previous) = previous {
        let Some(previous_topology) = previous.topology() else {
            return Ok(None);
        };
        let (deleted, _deleted_storage) = ctx
            .with_scoped_storage("index F3D changed topology members", || {
                changed_family_refs(ctx, &transition.topology, true)
            })?;
        let Some(previous_affected) = affected_storage
            .with_storage(|| bodies_intersecting(ctx, previous_topology, &deleted))?
        else {
            return Ok(None);
        };
        for body in ctx.admit_iter(
            previous_affected,
            "scan F3D previous affected history bodies",
        )? {
            ctx.insert_scoped_btree_set(
                &mut affected_storage,
                &mut affected,
                body,
                "find F3D affected history body",
                "merge F3D affected history bodies",
            )?;
        }
    }
    let mut bodies = ctx.collection_vec(affected.len(), "collect F3D affected history bodies")?;
    for &body in ctx.admit_iter(&affected, "scan F3D affected history bodies")? {
        bodies.push(body);
    }
    Ok(Some(bodies))
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
        let members: [&[i64]; 2] = if deleted {
            [&family.deleted, &[]]
        } else {
            [&family.inserted, &family.updated]
        };
        for members in members {
            for &member in ctx.admit_iter(members, "scan F3D changed topology family members")? {
                ctx.insert_btree_set(&mut changed, member, "index F3D changed topology members")?;
            }
        }
    }
    Ok(changed)
}

/// Visits every entity in each body's topology closure (regions, shells,
/// faces, loops, coedges, edges, vertices and their carriers) as
/// `visit(body, entity)`, an entity once per path that reaches it. Returns
/// `false` when any closure link is missing; every closure is walked to its
/// end so a missing link anywhere is found. The relation indexes are scratch
/// held for this call.
fn walk_body_closures(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
    mut visit: impl FnMut(i64, i64) -> Result<(), cadmpeg_core::CodecError>,
) -> Result<bool, cadmpeg_core::CodecError> {
    macro_rules! linked {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(false),
            }
        };
    }
    struct EdgeLinks {
        edge_vertices: HashMap<i64, [i64; 2]>,
        vertex_points: HashMap<i64, i64>,
        edge_curves: HashMap<i64, Option<i64>>,
    }
    fn visit_edge(
        links: &EdgeLinks,
        body: i64,
        edge: i64,
        visit: &mut impl FnMut(i64, i64) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        visit(body, edge)?;
        let Some(vertices) = links.edge_vertices.get(&edge) else {
            return Ok(false);
        };
        for &vertex in vertices {
            visit(body, vertex)?;
            let Some(&point) = links.vertex_points.get(&vertex) else {
                return Ok(false);
            };
            visit(body, point)?;
        }
        if let Some(curve) = links.edge_curves.get(&edge).copied().flatten() {
            visit(body, curve)?;
        }
        Ok(true)
    }
    let mut index_storage = decode.reserve_scoped(0, "index F3D historical relations")?;
    let body_regions =
        index_storage.with_storage(|| relation_map(decode, &topology.body_regions))?;
    let region_shells =
        index_storage.with_storage(|| relation_map(decode, &topology.region_shells))?;
    let shell_faces = index_storage.with_storage(|| relation_map(decode, &topology.shell_faces))?;
    let shell_wire_edges =
        index_storage.with_storage(|| relation_map(decode, &topology.shell_wire_edges))?;
    let shell_free_vertices =
        index_storage.with_storage(|| relation_map(decode, &topology.shell_free_vertices))?;
    let face_loops = index_storage.with_storage(|| relation_map(decode, &topology.face_loops))?;
    let loop_coedges =
        index_storage.with_storage(|| relation_map(decode, &topology.loop_coedges))?;
    let coedge_edges = index_storage.with_storage(|| {
        decode.collect_hash_map(
            topology
                .coedge_topology
                .iter()
                .map(|coedge| (coedge.coedge, coedge.edge)),
            "index F3D historical coedges",
        )
    })?;
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
    let links = EdgeLinks {
        edge_vertices: index_storage.with_storage(|| {
            decode.collect_hash_map(
                topology
                    .edge_vertices
                    .iter()
                    .map(|edge| (edge.edge, [edge.start_vertex, edge.end_vertex])),
                "index F3D historical edge vertices",
            )
        })?,
        vertex_points: index_storage.with_storage(|| carrier(&topology.vertex_points))?,
        edge_curves: index_storage.with_storage(|| optional_carrier(&topology.edge_curves))?,
    };
    let face_surfaces = index_storage.with_storage(|| carrier(&topology.face_surfaces))?;
    let coedge_pcurves =
        index_storage.with_storage(|| optional_carrier(&topology.coedge_pcurves))?;
    for &body in decode.admit_iter(&topology.bodies, "scan F3D topology bodies")? {
        visit(body, body)?;
        for &region in
            decode.admit_iter(*linked!(body_regions.get(&body)), "scan F3D body regions")?
        {
            visit(body, region)?;
            for &shell in decode.admit_iter(
                *linked!(region_shells.get(&region)),
                "scan F3D region shells",
            )? {
                visit(body, shell)?;
                let wire_edges = *linked!(shell_wire_edges.get(&shell));
                let free_vertices = *linked!(shell_free_vertices.get(&shell));
                for &face in
                    decode.admit_iter(*linked!(shell_faces.get(&shell)), "scan F3D shell faces")?
                {
                    visit(body, face)?;
                    visit(body, *linked!(face_surfaces.get(&face)))?;
                    for &loop_ in
                        decode.admit_iter(*linked!(face_loops.get(&face)), "scan F3D face loops")?
                    {
                        visit(body, loop_)?;
                        for &coedge in decode.admit_iter(
                            *linked!(loop_coedges.get(&loop_)),
                            "scan F3D loop coedges",
                        )? {
                            visit(body, coedge)?;
                            let edge = *linked!(coedge_edges.get(&coedge));
                            if !visit_edge(&links, body, edge, &mut visit)? {
                                return Ok(false);
                            }
                            if let Some(pcurve) = coedge_pcurves.get(&coedge).copied().flatten() {
                                visit(body, pcurve)?;
                            }
                        }
                    }
                }
                for &edge in decode.admit_iter(wire_edges, "scan F3D shell edges")? {
                    if !visit_edge(&links, body, edge, &mut visit)? {
                        return Ok(false);
                    }
                }
                for &vertex in
                    decode.admit_iter(free_vertices, "scan F3D historical shell vertices")?
                {
                    visit(body, vertex)?;
                    visit(body, *linked!(links.vertex_points.get(&vertex)))?;
                }
            }
        }
    }
    Ok(true)
}

/// Returns the bodies whose topology closure meets `changed`, or `None` when
/// any closure link is missing.
fn bodies_intersecting(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
    changed: &BTreeSet<i64>,
) -> Result<Option<BTreeSet<i64>>, cadmpeg_core::CodecError> {
    let mut affected = BTreeSet::new();
    let complete = walk_body_closures(decode, topology, |body, entity| {
        if decode.contains_btree_set(changed, &entity, "check F3D changed body closure")? {
            decode.insert_btree_set(&mut affected, body, "collect F3D affected topology bodies")?;
        }
        Ok(())
    })?;
    Ok(complete.then_some(affected))
}

/// The bodies whose closure holds each entity of one topology, for repeated
/// intersection queries. `None` when some body closure has a missing link.
struct BodyClosures<'ctx> {
    owners: Option<HashMap<i64, Vec<i64>>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn body_closures<'ctx>(
    decode: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<BodyClosures<'ctx>, cadmpeg_core::CodecError> {
    let operation = "index F3D body closure owners";
    let mut storage = decode.reserve_scoped(0, operation)?;
    let mut owners = HashMap::<i64, Vec<i64>>::new();
    let complete = walk_body_closures(decode, topology, |body, entity| {
        // One body's closure is walked before the next body's, so a repeated
        // visit by the same body is the group's last owner.
        if let Some(group) = owners.get(&entity) {
            if group.last() == Some(&body) {
                return Ok(());
            }
        }
        storage.with_storage(|| {
            decode.push_hash_group(&mut owners, entity, body, operation, operation)
        })
    })?;
    Ok(BodyClosures {
        owners: complete.then_some(owners),
        _storage: storage,
    })
}

/// Returns the bodies whose closure meets `changed`, or `None` when a closure
/// is incomplete.
fn closures_intersecting(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    closures: &BodyClosures<'_>,
    changed: &BTreeSet<i64>,
) -> Result<Option<BTreeSet<i64>>, cadmpeg_core::CodecError> {
    let Some(owners) = &closures.owners else {
        return Ok(None);
    };
    let mut affected = BTreeSet::new();
    for entity in decode.admit_iter(changed, "scan F3D changed body closure entities")? {
        let Some(bodies) = owners.get(entity) else {
            continue;
        };
        for &body in decode.admit_iter(bodies, "scan F3D changed entity bodies")? {
            decode.insert_btree_set(&mut affected, body, "collect F3D affected topology bodies")?;
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
                    let Some((next, previous)) =
                        cadmpeg_ir::topology::coedge_ring_neighbors(&brep.loops, coedge)
                    else {
                        return Ok(None);
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
