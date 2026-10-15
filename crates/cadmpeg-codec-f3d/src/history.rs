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

mod body_candidates;
mod body_index;
mod cache;
pub(crate) mod face_admission;
mod loop_index;
mod recipe_index;
pub(crate) mod selection;
mod topology_cache;
mod treatment_index;

use body_candidates::BodyCandidates;
use body_index::CompleteBodyIndex;
use cache::SnapshotCache;
use loop_index::LoopIndex;
use recipe_index::RecipeTopologyCache;
use topology_cache::{TopologyQueryCache, VertexBoundaryIndex};
use treatment_index::TreatmentCache;

use crate::bytes::int_at;
use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmEntityVersion,
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder, AsmHistoricalEdge,
    AsmHistoricalEntityDelta, AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint,
    AsmHistoricalRelation, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory, AsmHistoryRecord, AsmPreamble,
};
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
    if history.states.is_empty() {
        return false;
    }
    let mut by_index = HashMap::new();
    for state in &history.states {
        by_index.insert(state.node_index, state);
    }
    if by_index.len() != history.states.len()
        || history
            .states
            .iter()
            .any(|state| state.node_index < 0 || state.parent != history.id)
    {
        return false;
    }
    let mut heads = history
        .states
        .iter()
        .filter(|state| state.previous_ref.is_none());
    let head = heads.next();
    let tails = history
        .states
        .iter()
        .filter(|state| state.next_ref.is_none())
        .count();
    if head.is_none() || heads.next().is_some() || tails != 1 {
        return false;
    }
    let Some(head) = head else {
        return false;
    };
    if let Some(preamble) = history.preamble {
        if head.state_id != preamble.stream_size || preamble.history_entry_count < 0 {
            return false;
        }
    }
    let mut visited = HashSet::new();
    let mut previous = None;
    let mut current = Some(head.node_index);
    while let Some(index) = current {
        let Some(state) = by_index.get(&index) else {
            return false;
        };
        if visited.contains(&index) || state.previous_ref != previous {
            return false;
        }
        visited.insert(index);
        if state.version_flag != 1 || state.state_flag != 0 {
            return false;
        }
        for board in &state.bulletin_boards {
            if board.parent != state.id
                || board.changes.iter().any(|change| change.parent != board.id)
            {
                return false;
            }
        }
        if state
            .records
            .iter()
            .any(|record| record.parent != state.id || record.raw_bytes.is_empty())
        {
            return false;
        }
        previous = Some(index);
        current = state.next_ref;
    }
    visited.len() == history.states.len()
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
) -> Result<Option<AsmHistory>, cadmpeg_core::CodecError> {
    let preamble_offset = bytes
        .windows(PREAMBLE.len())
        .position(|window| window == PREAMBLE);
    let history_offset = preamble_offset.unwrap_or(0);
    let history_id =
        crate::ids::native_scoped_id(stream, "asm-history", format_args!("{history_offset:010}"));
    let mut delta_offsets = Vec::new();
    let mut search = 0usize;
    while let Some(relative) = bytes[search..]
        .windows(DELTA.len())
        .position(|window| window == DELTA)
    {
        let offset = search + relative;

        delta_offsets.push(offset);
        search = offset + DELTA.len();
    }
    let mut states = Vec::new();
    for (ordinal, &offset) in delta_offsets.iter().enumerate() {
        let state_record_id =
            crate::ids::native_scoped_id(stream, "asm-delta-state", format_args!("{offset:010}"));
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
        let Some((bulletin_boards, body_end)) =
            decode_bulletin_boards(bytes, position + 1, stream, offset, &state_record_id, width)
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

        let parent = history_id.clone();
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
    bind_snapshot_revision_ids(&mut states)?;
    let record_table_binding_budget_exceeded = bind_historical_entity_versions(
        &mut states,
        history_binding_max_state_records(ctx.policy()),
    )? == TableBindingCost::Exceeded;
    if !record_table_binding_budget_exceeded {
        bind_complete_record_tables(ctx, &mut states, bytes, width)?;
    }
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
    states: &mut [AsmDeltaState],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut old_references = Vec::new();
    for old_reference in states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
        .filter_map(super::history_records::AsmEntityChange::old_ref)
    {
        old_references.push(old_reference);
    }
    old_references.sort_unstable_by(Ord::cmp);
    let Some(&first) = old_references.first() else {
        return Ok(());
    };
    let count = i64::try_from(old_references.len()).map_err(|_| {
        cadmpeg_core::CodecError::malformed("F3D ASM old reference count exceeds i64")
    })?;
    let Some(end) = first.checked_add(count) else {
        return Ok(());
    };
    if old_references.iter().copied().ne(first..end) {
        return Ok(());
    }
    let snapshot_record_count = states
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| record.name() != "End-of-ASM-data")
        .count();
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
    states: &[AsmDeltaState],
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let mut archived = Vec::new();
    for revision_id in states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
    {
        archived.push(revision_id);
    }
    archived.sort_unstable_by(Ord::cmp);
    let Some(&active_count) = archived.first() else {
        return Ok(None);
    };
    let count = i64::try_from(archived.len()).map_err(|_| {
        cadmpeg_core::CodecError::malformed("F3D archived revision count exceeds i64")
    })?;
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
fn insert_only_active_record_count(states: &[AsmDeltaState]) -> Option<usize> {
    let mut has_boundary_record = false;
    for record in states.iter().flat_map(|state| &state.records) {
        if record.revision_id.is_some() || !is_history_boundary_record(record) {
            return None;
        }
        has_boundary_record = true;
    }
    if !has_boundary_record {
        return None;
    }
    let mut inserted = BTreeSet::new();
    for change in states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
    {
        let new_ref = change.new_ref().filter(|_| change.old_ref().is_none())?;
        if new_ref <= 0 || inserted.contains(&new_ref) {
            return None;
        }
        inserted.insert(new_ref);
    }
    let &last = inserted.last()?;
    if inserted.iter().copied().ne(1..=last) {
        return None;
    }
    last.checked_add(1)
        .and_then(|count| usize::try_from(count).ok())
}

/// Native text that binding retains for one state-record entry: the
/// entity-version pair and the plane-selection topology that each
/// `asm_delta_states` record keeps for the entry, and the Design operands that
/// the bound history resolves. User designs with 1 to 20 million entries
/// retain 110 to 124 bytes for each entry. The value is rounded up.
const HISTORY_RETAINED_BYTES_PER_STATE_RECORD: u64 = 128;

/// Largest number of state-record entries that one history stream binds.
///
/// One entry is one record of one state's record table. The per-state
/// topology decode, the projected snapshots, and the retained native text all
/// grow linearly with the entry count. At this count the binding alone retains
/// the policy's complete `max_retained_bytes`, so a larger binding cannot
/// complete in the session. A history past this count skips historical
/// topology and reports the skip.
fn history_binding_max_state_records(policy: &cadmpeg_core::decode::DecodePolicy) -> u64 {
    policy.limits.max_retained_bytes / HISTORY_RETAINED_BYTES_PER_STATE_RECORD
}

/// Whether one history stream's state-record entries fit the binding bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableBindingCost {
    /// The entries fit; the history is bound.
    Admitted,
    /// The entries exceed the bound; historical topology is skipped.
    Exceeded,
}

/// Project the entity-to-record version map onto every state.
///
/// The walk is done twice. The first walk validates the chain and counts the
/// state-record entries without a projection. A history with more than
/// `max_state_records` entries is not projected and reports
/// [`TableBindingCost::Exceeded`]. The second walk stores one version map for
/// each state.
fn bind_historical_entity_versions(
    states: &mut [AsmDeltaState],
    max_state_records: u64,
) -> Result<TableBindingCost, cadmpeg_core::CodecError> {
    let mut archived_ids = Vec::new();
    for revision_id in states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
    {
        archived_ids.push(revision_id);
    }
    archived_ids.sort_unstable_by(Ord::cmp);
    let Some(active_count) =
        archived_active_record_count(states)?.or_else(|| insert_only_active_record_count(states))
    else {
        return Ok(TableBindingCost::Admitted);
    };
    let Ok(active_count) = i64::try_from(active_count) else {
        return Ok(TableBindingCost::Admitted);
    };
    let mut state_records = Some(0_u64);
    if !walk_entity_versions(states, active_count, &archived_ids, |_, versions| {
        state_records =
            state_records.and_then(|total| total.checked_add(u64_from_index(versions.len())));
    }) {
        return Ok(TableBindingCost::Admitted);
    }
    if state_records.is_none_or(|count| count > max_state_records) {
        return Ok(TableBindingCost::Exceeded);
    }
    let mut projected = Vec::new();
    walk_entity_versions(states, active_count, &archived_ids, |ordinal, versions| {
        let mut state_versions = Vec::new();
        for (&entity_ref, &record_ref) in versions {
            state_versions.push(AsmEntityVersion {
                entity_ref,
                record_ref,
            });
        }
        projected.push((ordinal, state_versions));
    });
    for (ordinal, versions) in projected {
        states[ordinal].entity_versions = versions;
    }
    Ok(TableBindingCost::Admitted)
}

/// Walk the state chain from its head and give each state's entity-to-record
/// version map to `visit`. Returns false, after a partial walk, when the chain
/// or a bulletin-board change is inconsistent with the archived revisions.
fn walk_entity_versions(
    states: &[AsmDeltaState],
    active_count: i64,
    archived_ids: &[i64],
    mut visit: impl FnMut(usize, &BTreeMap<i64, i64>),
) -> bool {
    let mut by_node = HashMap::new();
    for (ordinal, state) in states.iter().enumerate() {
        by_node.insert(state.node_index, ordinal);
    }
    if by_node.len() != states.len() {
        return false;
    }
    let mut heads = states
        .iter()
        .enumerate()
        .filter(|(_, state)| state.previous_ref.is_none())
        .map(|(ordinal, _)| ordinal);
    let Some(mut ordinal) = heads.next() else {
        return false;
    };
    if heads.next().is_some() {
        return false;
    }

    let mut versions = BTreeMap::new();
    for id in 0..active_count {
        versions.insert(id, id);
    }
    let mut visited = HashSet::new();
    loop {
        let state = &states[ordinal];
        if !visited.insert(state.node_index) {
            return false;
        }
        visit(ordinal, &versions);
        for change in state
            .bulletin_boards
            .iter()
            .flat_map(|board| &board.changes)
        {
            match change.kind {
                AsmEntityChangeKind::Update { old, new } => {
                    if !versions.contains_key(&new) || archived_ids.binary_search(&old).is_err() {
                        return false;
                    }
                    versions.insert(new, old);
                }
                AsmEntityChangeKind::Insert { new } => {
                    if versions.remove(&new).is_none() {
                        return false;
                    }
                }
                AsmEntityChangeKind::Delete { old } => {
                    if versions.contains_key(&old) || archived_ids.binary_search(&old).is_err() {
                        return false;
                    }
                    versions.insert(old, old);
                }
            }
        }
        let Some(next) = state.next_ref else {
            break;
        };
        let Some(&next_ordinal) = by_node.get(&next) else {
            return false;
        };
        ordinal = next_ordinal;
    }
    visited.len() == states.len() && versions.len() == 1 && versions.get(&0) == Some(&0)
}

fn bind_complete_record_tables(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
    bytes: &[u8],
    width: RefWidth,
) -> Result<(), cadmpeg_core::CodecError> {
    let Some(start) = cadmpeg_asm::asm_header::record_stream_start(bytes) else {
        return Ok(());
    };
    let active_limit = cadmpeg_asm::asm_header::solved_record_limit(bytes).unwrap_or(bytes.len());
    let framed = match cadmpeg_asm::sab::frame(ctx, bytes, start, active_limit, width, None) {
        Ok(records) => records,
        Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => return Err(error.into()),
        Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
            return Err(error.into_codec_error())
        }
        Err(_) => return Ok(()),
    };

    let insert_only = insert_only_active_record_count(states);
    let archived_count = archived_active_record_count(states)?;
    let Some(active_count) = archived_count.or(insert_only) else {
        return Ok(());
    };
    if insert_only.is_some() && framed.len() != active_count {
        return Ok(());
    }
    let Some(active_records) = framed.get(..active_count) else {
        return Ok(());
    };
    let mut archived_frames = HashMap::new();
    for record in states
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| record.revision_id.is_some())
    {
        let Some(revision_id) = record.revision_id else {
            return Ok(());
        };
        let Some(offset) = usize::try_from(record.byte_offset).ok() else {
            return Ok(());
        };
        let Some(limit) = offset.checked_add(record.raw_bytes.len()) else {
            return Ok(());
        };
        if bytes.get(offset..limit) != Some(record.raw_bytes.as_slice()) {
            return Ok(());
        }
        let mut framed = match cadmpeg_asm::sab::frame(ctx, bytes, offset, limit, width, None) {
            Ok(records) => records,
            Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => {
                return Err(error.into())
            }
            Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
                return Err(error.into_codec_error())
            }
            Err(_) => return Ok(()),
        };
        if framed.len() != 1 {
            return Ok(());
        }
        let Some(framed) = framed.pop() else {
            return Ok(());
        };
        if framed.name != record.name() {
            return Ok(());
        }

        if archived_frames.insert(revision_id, framed).is_some() {
            return Ok(());
        }
    }
    let Some(archive) = historical_record_archive(states, active_records, archived_frames) else {
        return Ok(());
    };
    let mut complete = true;
    for state in states.iter_mut() {
        let Some(records) = materialize_record_table(state, &archive) else {
            complete = false;
            break;
        };
        let decoded =
            match crate::brep::decode_history_topology(ctx, &records, bytes, crate::ids::ID_FORMAT)
            {
                Ok(decoded) => decoded,
                Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Err(error),
                Err(_) => {
                    complete = false;
                    break;
                }
            };
        let Some(topology) = historical_topology_with_tags(&decoded)? else {
            complete = false;
            break;
        };
        state.topology_cache = crate::history_records::AsmTopologyCache::Complete(topology);
    }
    if complete {
        bind_historical_transitions(states);
        // Keep only record revisions that can be named by a late persistent
        // selection. Non-topological ASM attributes do not participate in
        // feature selection and need not survive projection finalization.
        for state in states {
            if let Some(topology) = state.topology() {
                let slots = topology_entity_slots(topology);
                state
                    .entity_versions
                    .retain(|version| slots.contains(&version.entity_ref));
            } else {
                state.entity_versions.clear();
            }
        }
    } else {
        for state in states {
            state.topology_cache = crate::history_records::AsmTopologyCache::Absent;
        }
    }
    Ok(())
}

fn topology_entity_slots(topology: &AsmHistoricalTopology) -> HashSet<i64> {
    let families = [
        &topology.bodies,
        &topology.regions,
        &topology.shells,
        &topology.faces,
        &topology.loops,
        &topology.coedges,
        &topology.edges,
        &topology.vertices,
        &topology.points,
        &topology.surfaces,
        &topology.curves,
        &topology.pcurves,
    ];

    let mut slots = HashSet::new();

    for slot in families.into_iter().flatten() {
        slots.insert(*slot);
    }
    slots
}

type HistoricalRecordArchive = HashMap<i64, cadmpeg_asm::sab::Record>;

fn historical_record_archive(
    states: &[AsmDeltaState],
    active_records: &[cadmpeg_asm::sab::Record],
    archived_frames: HashMap<i64, cadmpeg_asm::sab::Record>,
) -> Option<HistoricalRecordArchive> {
    if active_records
        .iter()
        .enumerate()
        .any(|(index, record)| record.index != index)
    {
        return None;
    }
    let active_count = i64::try_from(active_records.len()).ok()?;

    let mut revision_entities = HashMap::new();

    for entity_ref in 0..active_count {
        revision_entities.insert(entity_ref, entity_ref);
    }
    for change in states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
    {
        let Some(old_ref) = change.old_ref() else {
            continue;
        };
        let entity_ref = change.new_ref().unwrap_or(old_ref);

        if revision_entities.insert(old_ref, entity_ref).is_some() {
            return None;
        }
    }

    let mut records = HashMap::new();

    for (revision, record) in active_records.iter().enumerate() {
        let revision = i64::try_from(revision).ok()?;
        records.insert(revision, clone_historical_record(record));
    }
    for (revision_id, framed) in archived_frames {
        if records.insert(revision_id, framed).is_some() {
            return None;
        }
    }
    if records.len() != revision_entities.len() {
        return None;
    }
    for (&revision_ref, record) in &mut records {
        let &entity_ref = revision_entities.get(&revision_ref)?;
        let index = usize::try_from(entity_ref).ok()?;
        record.index = index;
        if std::sync::Arc::strong_count(&record.tokens) > 1 {
            let mut tokens = Vec::new();
            for token in record.tokens.iter() {
                tokens.push(clone_historical_token(token));
            }
            record.tokens = tokens.into();
        }
        for token in std::sync::Arc::make_mut(&mut record.tokens) {
            let cadmpeg_asm::sab::Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0 {
                let &entity_ref = revision_entities.get(reference)?;
                *reference = entity_ref;
            }
        }
    }
    Some(records)
}

fn clone_historical_record(record: &cadmpeg_asm::sab::Record) -> cadmpeg_asm::sab::Record {
    let name = record.name.clone();
    cadmpeg_asm::sab::Record {
        index: record.index,
        name,
        tokens: record.tokens.clone(),
        offset: record.offset,
        len: record.len,
    }
}

fn clone_historical_token(token: &cadmpeg_asm::sab::Token) -> cadmpeg_asm::sab::Token {
    use cadmpeg_asm::sab::Token;
    match token {
        Token::Str(value) => Token::Str(value.to_owned()),
        Token::Ident(value) => Token::Ident(value.to_owned()),
        Token::SubIdent(value) => Token::SubIdent(value.to_owned()),
        _ => token.clone(),
    }
}

fn bind_historical_transitions(states: &mut [AsmDeltaState]) {
    let mut by_node = HashMap::new();

    for (ordinal, state) in states.iter().enumerate() {
        by_node.insert(state.node_index, ordinal);
    }
    if by_node.len() != states.len() {
        return;
    }

    let mut transitions = Vec::new();

    for state in states.iter() {
        let previous = match state.next_ref {
            Some(node) => {
                let Some(&ordinal) = by_node.get(&node) else {
                    return;
                };
                states.get(ordinal)
            }
            None => None,
        };
        let Some(transition) = historical_transition(state, previous) else {
            return;
        };
        transitions.push(transition);
    }
    for (state, transition) in states.iter_mut().zip(transitions) {
        state.transition = Some(transition);
    }
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
pub(crate) fn discard_projection_caches(histories: &mut [AsmHistory]) {
    for history in histories {
        for state in &mut history.states {
            let topology = match std::mem::take(&mut state.topology_cache) {
                crate::history_records::AsmTopologyCache::Absent
                | crate::history_records::AsmTopologyCache::Released => None,
                crate::history_records::AsmTopologyCache::Complete(topology)
                | crate::history_records::AsmTopologyCache::Retained(topology) => {
                    retain_mirror_plane_topology(topology)
                }
            };
            if let Some(topology) = topology {
                let slots = topology_entity_slots(&topology);
                state
                    .entity_versions
                    .retain(|version| slots.contains(&version.entity_ref));
                state.topology_cache = crate::history_records::AsmTopologyCache::Retained(topology);
            } else {
                state.entity_versions.clear();
                state.topology_cache = crate::history_records::AsmTopologyCache::Released;
            }
        }
    }
}

pub(crate) fn projection_was_finalized(histories: &[AsmHistory]) -> bool {
    !histories.is_empty()
        && histories
            .iter()
            .all(crate::history_records::AsmHistory::projection_finalized)
}

fn historical_transition(
    current: &AsmDeltaState,
    previous: Option<&AsmDeltaState>,
) -> Option<AsmHistoricalTransition> {
    let current_topology = current.topology()?;
    let previous_topology = previous.and_then(|state| state.topology());
    // The version maps and record keys are scratch for this transition.
    let current_versions = historical_version_map(&current.entity_versions);
    let previous_versions = match previous {
        Some(state) => historical_version_map(&state.entity_versions),
        None => BTreeMap::new(),
    };
    let delta = |current: &[i64], previous: &[i64]| {
        entity_delta(current, previous, &current_versions, &previous_versions)
    };
    let empty = AsmHistoricalTopology::default();
    let previous_topology = previous_topology.unwrap_or(&empty);
    let current_record_keys = current_versions.keys().copied().collect::<Vec<_>>();
    let previous_record_keys = previous_versions.keys().copied().collect::<Vec<_>>();
    Some(AsmHistoricalTransition {
        previous_state_id: previous.map(|state| state.state_id),
        records: entity_delta(
            &current_record_keys,
            &previous_record_keys,
            &current_versions,
            &previous_versions,
        ),
        topology: AsmHistoricalTopologyDelta {
            bodies: delta(&current_topology.bodies, &previous_topology.bodies),
            regions: delta(&current_topology.regions, &previous_topology.regions),
            shells: delta(&current_topology.shells, &previous_topology.shells),
            faces: delta(&current_topology.faces, &previous_topology.faces),
            loops: delta(&current_topology.loops, &previous_topology.loops),
            coedges: delta(&current_topology.coedges, &previous_topology.coedges),
            edges: delta(&current_topology.edges, &previous_topology.edges),
            vertices: delta(&current_topology.vertices, &previous_topology.vertices),
            points: delta(&current_topology.points, &previous_topology.points),
            surfaces: delta(&current_topology.surfaces, &previous_topology.surfaces),
            curves: delta(&current_topology.curves, &previous_topology.curves),
            pcurves: delta(&current_topology.pcurves, &previous_topology.pcurves),
        },
    })
}

fn historical_version_map(versions: &[AsmEntityVersion]) -> BTreeMap<i64, i64> {
    let mut indexed = BTreeMap::new();
    for version in versions {
        indexed.insert(version.entity_ref, version.record_ref);
    }
    indexed
}

fn entity_delta(
    current: &[i64],
    previous: &[i64],
    current_versions: &BTreeMap<i64, i64>,
    previous_versions: &BTreeMap<i64, i64>,
) -> AsmHistoricalEntityDelta {
    let current = current.iter().copied().collect::<BTreeSet<_>>();
    let previous = previous.iter().copied().collect::<BTreeSet<_>>();

    AsmHistoricalEntityDelta {
        inserted: current.difference(&previous).copied().collect::<Vec<_>>(),
        deleted: previous.difference(&current).copied().collect::<Vec<_>>(),
        updated: current
            .intersection(&previous)
            .copied()
            .filter(|entity| current_versions.get(entity) != previous_versions.get(entity))
            .collect::<Vec<_>>(),
    }
}

pub(crate) fn bind_feature_outputs(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    active_bodies: &[cadmpeg_ir::topology::Body],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut state_outputs = HashMap::<i64, Option<Vec<i64>>>::new();
    let mut body_closures = HistoricalBodyClosureCache::default();
    for history in histories {
        let mut by_node = HashMap::new();
        for state in &history.states {
            by_node.insert(state.node_index, state);
        }
        if by_node.len() != history.states.len() {
            continue;
        }
        for state in &history.states {
            let previous = match state.next_ref {
                Some(node) => match by_node.get(&node) {
                    Some(previous) => Some(*previous),
                    None => continue,
                },
                None => None,
            };
            let Some(outputs) = affected_body_refs(state, previous, &mut body_closures) else {
                continue;
            };
            state_outputs
                .entry(state.state_id)
                .and_modify(|outputs| *outputs = None)
                .or_insert_with(|| Some(outputs));
        }
    }
    let mut active = HashMap::new();
    for body in active_bodies {
        let Some(slot) = stable_ref(body.id.as_str()) else {
            continue;
        };
        let id = body.id.clone();
        active.insert(slot, id);
    }
    for feature in features {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|id| scopes.iter().find(|scope| scope.id == id))
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
            for slot in outputs {
                let Some(id) = active.get(slot) else {
                    continue;
                };
                resolved.push(id.clone());
            }
            feature.evaluation.set_outputs(
                cadmpeg_ir::features::DistinctMembers::try_from(resolved)
                    .map_err(cadmpeg_core::CodecError::from)?,
            );
            bind_base_feature_output_selection(feature)?;
        }
    }
    Ok(())
}

fn bind_base_feature_output_selection(
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
        let id = body.clone();

        selected.push(id);
    }
    let bodies = cadmpeg_ir::features::BodySelection::Resolved {
        bodies: cadmpeg_ir::features::DistinctMembers::try_from(selected)
            .map_err(cadmpeg_core::CodecError::from)?,
        native: native.to_owned(),
    };
    feature
        .evaluation
        .set_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::BaseFeature { bodies },
        ));
    Ok(())
}

pub(crate) fn bind_sweep_result_modes(
    features: &mut [cadmpeg_ir::features::Feature],
    bodies: &[cadmpeg_ir::topology::Body],
) {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SweepMode, SweepShape};
    use cadmpeg_ir::topology::BodyKind;

    let mut body_kinds = HashMap::new();
    for body in bodies {
        body_kinds.insert(&body.id, body.kind);
    }
    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: SweepShape::Unresolved { .. },
            ..
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        if feature.evaluation.outputs().is_empty() {
            continue;
        }
        let mut all_sheet = true;
        let mut all_solid = true;
        for output in feature.evaluation.outputs() {
            match body_kinds.get(output) {
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

    let mut body_candidates = BodyCandidates::new(
        bodies,
        regions,
        shells,
        inputs.construction_recipes,
        inputs.persistent_design_links,
    );
    bind_pattern_body_selections(features, inputs, &mut body_candidates);
    let mut pattern_body_slots = HashMap::new();
    'pattern: for feature in features.iter() {
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
        let mut slots = BTreeSet::new();
        for slot in std::iter::once(historical_body_slot(seed_body.as_str())).chain(
            feature
                .evaluation
                .outputs()
                .iter()
                .map(|body| stable_ref(body.as_str())),
        ) {
            let Some(slot) = slot else {
                continue 'pattern;
            };
            slots.insert(slot);
        }
        if slots.len() != expected_count {
            continue;
        }
        let feature_id = feature.id.clone();
        pattern_body_slots.insert(feature_id, slots);
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
                    bind_body_recipe_body_selection(
                        tools,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                    for cell in cells {
                        bind_body_recipe_body_selection(
                            cell,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            body_recipe_operands,
                        );
                    }
                } else {
                    bind_direct_body_recipe_body_selection(tools, scope, inputs, &mut body_candidates);
                    for cell in cells {
                        bind_direct_body_recipe_body_selection(cell, scope, inputs, &mut body_candidates);
                    }
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
                definition
            {
                let edit = operands
                    .try_edit(|target, tools| {
                        let (Some(state_id), Some(previous_state_id)) =
                            (scope.history_state_id(), scope.previous_history_state_id())
                        else {
                            bind_direct_body_recipe_body_selection(target, scope, inputs, &mut body_candidates);
                            bind_direct_body_recipe_body_selection(tools, scope, inputs, &mut body_candidates);
                            if matches!(
                                tools,
                                BodySelection::Native(_) | BodySelection::NativeSet(_)
                            ) {
                                if let Some(local) = combine_external_local_tools(scope) {
                                    *tools = local;
                                }
                            }
                            return;
                        };
                        if let Some(local) = combine_external_local_tools(scope) {
                            *tools = local;
                        }
                        let BodySelection::Native(native) = target else {
                            return;
                        };
                        let Some((history, state, _)) =
                            unique_history_state_pair(histories, state_id, previous_state_id)
                        else {
                            return;
                        };
                        let history_states = unique_feature_history_states(history);
                        let Some(body) = singleton_revised_input_body_across_state_chain(state, previous_state_id, &history_states) else {
                            return;
                        };
                        let prefix = crate::ids::history_input_prefix(&feature_id.key(), previous_state_id);
                        let input_state = crate::ids::history_input_state_id(&prefix);
                        let body_id = crate::ids::history_input_body_id(&prefix, body);
                        let native_id = native.to_owned();
                        let target_bodies = vec![body_id];
                        if let Ok(historical) = cadmpeg_ir::features::BodySelection::historical(crate::ids::history_input_state_id(&prefix), target_bodies, native_id) {
                            *target = historical;
                        }
                        let Some(stream) = crate::ids::native_stream(&scope.id) else {
                            return;
                        };
                        let Some(operation) = scope.combine_operation() else {
                            return;
                        };
                        let mut native_tools = Vec::new();
                        for tool in operation.tools.iter() {
                            let native = format!("{stream}:design-record#{}", tool.record_index);
                            native_tools.push(native);
                        }
                        let current_history_source = historical_brep_source(&state.id);
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
                                let body = crate::ids::history_input_body_id(&prefix, body);
                                let repeated = historical_tool_rows
                                    .iter()
                                    .any(|row: &BodyMember<_>| row.body() == &body);
                                let row = body_member(body, native.to_owned());
                                let (false, Some(row)) = (repeated, row) else {
                                    historical_tool_rows.clear();
                                    direct_tool_rows.clear();
                                    break;
                                };
                                historical_tool_rows.push(row);
                                continue;
                            }
                            let Some(body) =
                                body_candidates.external(operand, current_history_source)
                            else {
                                historical_tool_rows.clear();
                                direct_tool_rows.clear();
                                break;
                            };
                            let repeated = direct_tool_rows
                                .iter()
                                .any(|row: &BodyMember<_>| row.body() == &body);
                            let row = body_member(body, native.to_owned());
                            let (false, Some(row)) = (repeated, row) else {
                                historical_tool_rows.clear();
                                direct_tool_rows.clear();
                                break;
                            };
                            direct_tool_rows.push(row);
                        }
                        if historical_tool_rows.len() == native_tools.len() {
                            let Ok(members) = cadmpeg_ir::features::BodyMembers::try_from_rows(historical_tool_rows) else {
                                return;
                            };
                            *tools = BodySelection::HistoricalSet {
                                state: input_state,
                                members,
                            };
                        } else if direct_tool_rows.len() == native_tools.len() {
                            *tools = if direct_tool_rows.len() == 1 {
                                let Some(row) = direct_tool_rows.pop() else { return; };
                                let (body, native) = row.into_parts();
                                let selected = vec![body];
                                let Ok(bodies) = cadmpeg_ir::features::DistinctMembers::try_from(selected) else {
    return;
};
                                BodySelection::Resolved {
                                    bodies,
                                    native,
                                }
                            } else {
                                let Ok(members) = cadmpeg_ir::features::BodyMembers::try_from_rows(direct_tool_rows) else {
                                    return;
                                };
                                BodySelection::ResolvedSet { members }
                            };
                        } else {
                            let tool_record_indices = operation.tools.iter().map(|tool| tool.record_index).collect::<Vec<_>>();
                            if let Some(tool_slots) = combine_recipe_family_tool_slots((stream, scope.record_index), &tool_record_indices, previous_state_id, body, body_recipe_operands, inputs.construction_recipes) {
                                if tool_slots.len() != native_tools.len() {
                                    return;
                                }
                                let Some(rows) = combine_historical_rows(&prefix, tool_slots, native_tools) else {
                                    return;
                                };
                                let Ok(members) =
                                    cadmpeg_ir::features::BodyMembers::try_from_rows(rows)
                                else {
                                    return;
                                };
                                *tools = BodySelection::HistoricalSet {
                                    state: input_state,
                                    members,
                                };
                                return;
                            }
                            let mut dependency_sets = dependencies.iter()
                                .filter_map(|dependency| pattern_body_slots.get(dependency));
                            let pattern_bodies = dependency_sets.next();
                            if dependency_sets.next().is_none() {
                              if let Some(pattern_bodies) = pattern_bodies {
                                if let Some(tool_slots) = pattern_combine_tool_slots(pattern_bodies, body, native_tools.len()) {
                                    if tool_slots.len() != native_tools.len() {
                                        return;
                                    }
                                    let Some(rows) = combine_historical_rows(&prefix, tool_slots, native_tools) else {
                                        return;
                                    };
                                    let Ok(members) =
                                        cadmpeg_ir::features::BodyMembers::try_from_rows(rows)
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
                    bind_body_recipe_body_selection(
                        targets,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                } else {
                    bind_direct_body_recipe_body_selection(targets, scope, inputs, &mut body_candidates);
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, .. }) =
                definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    bind_body_recipe_body_selection(
                        bodies,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                } else {
                    bind_direct_body_recipe_body_selection(bodies, scope, inputs, &mut body_candidates);
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Scale { bodies, .. }) =
                definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    bind_body_recipe_body_selection(
                        bodies,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                    if matches!(bodies, BodySelection::Native(_)) {
                        bind_direct_body_recipe_body_selection(bodies, scope, inputs, &mut body_candidates);
                    }
                } else {
                    bind_direct_body_recipe_body_selection(bodies, scope, inputs, &mut body_candidates);
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
                bind_direct_body_recipe_body_selection(bodies, scope, inputs, &mut body_candidates);
                break 'feature_edit;
            };
            let Some((history, state, _previous)) =
                unique_history_state_pair(histories, state_id, previous_state_id)
            else {
                bind_direct_body_recipe_body_selection(bodies, scope, inputs, &mut body_candidates);
                break 'feature_edit;
            };
            let history_states = unique_feature_history_states(history);
            let body = match proof {
                BodySelectionProof::TopologyStableRevision => {
                    singleton_body_revision_across_state_chain(state, previous_state_id, &history_states)
                }
                BodySelectionProof::RevisedInput => {
                    singleton_revised_input_body_across_state_chain(state, previous_state_id, &history_states)
                }
            };
            let Some(body) = body else {
                break 'feature_edit;
            };
            let prefix = crate::ids::history_input_prefix(&feature_id.key(), previous_state_id);
            let state_id = crate::ids::history_input_state_id(&prefix);
            let body_id = crate::ids::history_input_body_id(&prefix, body);
            let native = group_id.to_owned();
            let selected = vec![body_id];
            if let Ok(historical) = cadmpeg_ir::features::BodySelection::historical(state_id, selected, native) {
                *bodies = historical;
            }
        }
        });
        edit_result?;
    }

    Ok(())
}

fn combine_historical_rows(
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slots: Vec<i64>,
    native_tools: Vec<String>,
) -> Option<Vec<cadmpeg_ir::features::BodyMember<cadmpeg_ir::ids::HistoricalBodyId>>> {
    let mut rows = Vec::new();
    for (slot, native) in slots.into_iter().zip(native_tools) {
        let body = crate::ids::history_input_body_id(prefix, slot);
        let row = body_member(body, native)?;

        rows.push(row);
    }
    Some(rows)
}

fn pattern_combine_tool_slots(
    pattern_bodies: &BTreeSet<i64>,
    target_body: i64,
    native_tool_count: usize,
) -> Option<Vec<i64>> {
    if !pattern_bodies.contains(&target_body)
        || pattern_bodies.len().checked_sub(1) != Some(native_tool_count)
    {
        return None;
    }
    Some(
        pattern_bodies
            .iter()
            .copied()
            .filter(|body| *body != target_body)
            .collect::<Vec<_>>(),
    )
}

fn combine_recipe_family_tool_slots(
    scope_key: (&str, u32),
    tool_record_indices: &[u32],
    previous_state_id: i64,
    target_body: i64,
    operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    recipes: &[crate::records::recipes::ConstructionRecipe],
) -> Option<Vec<i64>> {
    macro_rules! required {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return None,
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
        return None;
    }
    let mut seen = HashSet::new();
    for index in tool_record_indices {
        if !seen.insert(*index) {
            return None;
        }
    }
    let mut recipes_by_id =
        HashMap::<&str, Option<&crate::records::recipes::ConstructionRecipe>>::new();
    for recipe in recipes.iter().filter(|recipe| {
        recipe.kind == crate::records::recipes::ConstructionRecipeKind::Body
            && crate::ids::native_stream(&recipe.id) == Some(stream)
    }) {
        recipes_by_id
            .entry(recipe.id.as_str())
            .and_modify(|entry| *entry = None)
            .or_insert(Some(recipe));
    }
    let mut families = BTreeMap::<FamilyKey<'_>, Vec<FamilyMember<'_>>>::new();
    for record_index in tool_record_indices {
        let mut matching = operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope_record_index
                && matches!(
                    operand.owner,
                    crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
                )
                && operand.record_index() == *record_index
        });
        let operand = required!(matching.next());
        if matching.next().is_some() || operand.references().len() != 1 {
            return None;
        }
        let recipe = required!(required!(recipes_by_id.get(operand.recipe_id.as_str())).as_ref());
        let design = required!(recipe.design.as_ref());
        let selector = required!(design.selector).value;
        if selector == 0 {
            return None;
        }
        let resolved = match (operand.resolved_body_state_id, operand.resolved_body_slot) {
            (Some(state), Some(body)) if state == previous_state_id => Some(body),
            (None, None) => None,
            _ => return None,
        };
        let reference = &operand.references()[0];
        let key = (
            &operand.asset_id,
            &operand.context_id,
            reference.design_reference,
            reference.form,
            design.id.value.as_str(),
        );
        families.entry(key).or_default().push((
            selector,
            resolved,
            reference.preceding_body_slots.as_slice(),
        ));
    }

    let mut selected = BTreeSet::new();
    for family in families.into_values() {
        let mut exact = BTreeSet::new();
        for body in family.iter().filter_map(|(_, body, _)| *body) {
            exact.insert(body);
        }
        if family.iter().all(|(_, body, _)| body.is_some()) {
            if exact.len() != family.len() {
                return None;
            }
            for body in exact {
                selected.insert(body);
            }
            continue;
        }
        let mut selectors = BTreeSet::new();
        for (selector, _, _) in &family {
            selectors.insert(*selector);
        }
        let count = required!(u32::try_from(family.len()).ok());
        if selectors.len() != family.len()
            || !(1..=count).all(|selector| selectors.contains(&selector))
        {
            return None;
        }
        let mut candidate_sets = family
            .iter()
            .map(|(_, _, candidates)| candidates)
            .filter(|candidates| !candidates.is_empty());
        let candidate_slice = required!(candidate_sets.next());
        let mut candidates = BTreeSet::new();
        for candidate in *candidate_slice {
            candidates.insert(*candidate);
        }
        if candidates.len() != family.len()
            || candidates.contains(&target_body)
            || !exact.is_subset(&candidates)
            || candidate_sets.any(|other| {
                other.iter().any(|body| !candidates.contains(body))
                    || candidates.iter().any(|body| !other.contains(body))
            })
        {
            return None;
        }
        for body in candidates {
            selected.insert(body);
        }
    }
    if selected.len() != tool_record_indices.len() || selected.contains(&target_body) {
        return None;
    }
    Some(selected.into_iter().collect::<Vec<_>>())
}

fn combine_external_local_tools(
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Option<cadmpeg_ir::features::BodySelection> {
    let operation = scope.combine_operation()?;
    let mut bodies = Vec::new();
    for tool in operation.tools.iter() {
        let identity = tool.external_identity.as_ref()?;
        let id = crate::ids::neutral_combine_external_body_id(identity);

        if bodies.iter().any(|body| body == &id) {
            return None;
        }
        bodies.push(id);
    }
    let native = scope.id.clone();
    cadmpeg_ir::features::BodySelection::local(bodies, native).ok()
}

fn historical_body_slot(id: &str) -> Option<i64> {
    id.strip_prefix("f3d:history-input:body#")?
        .rsplit_once(':')?
        .1
        .parse()
        .ok()
}

fn bind_pattern_body_selections<'a>(
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'a>,
    body_candidates: &mut BodyCandidates<'a>,
) {
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
            Some(PatternSeed::Bodies(BodySelection::Native(group.id.clone())))
        } else {
            None
        };
        let feature_id = &feature.id;
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = definition
            else {
                return;
            };
            if let Some(seed) = seed.take() {
                seeds.push(seed);
            }
            let [PatternSeed::Bodies(selection)] = seeds.as_mut_slice() else {
                return;
            };
            if let Some(previous_state_id) = scope.previous_history_state_id() {
                bind_body_recipe_body_selection(
                    selection,
                    feature_id,
                    previous_state_id,
                    scope,
                    groups,
                    body_recipe_operands,
                );
            } else {
                bind_direct_body_recipe_body_selection(selection, scope, inputs, body_candidates);
            }
        });
    }
}

fn bind_body_recipe_body_selection(
    selection: &mut cadmpeg_ir::features::BodySelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
) {
    use cadmpeg_ir::features::BodySelection;

    let BodySelection::Native(group_id) = selection else {
        return;
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
        return;
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return;
    }
    let mut body_slots = Vec::new();
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| member.value)
        .enumerate()
    {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return;
        };
        let mut matching_operands = operands.iter().filter(|operand| {
            operand.owner.group() == Some((group.record_index, ordinal))
                && operand.record_index() == record_index
                && crate::ids::native_stream(&operand.id) == stream
        });
        let Some(operand) = matching_operands.next() else {
            return;
        };
        if matching_operands.next().is_some() {
            return;
        }
        let Some(body_slot) = operand.resolved_body_slot else {
            return;
        };
        if !body_slots.contains(&body_slot) {
            body_slots.push(body_slot);
        }
    }

    let prefix = crate::ids::history_input_prefix(&feature_id.key(), previous_state_id);
    let mut body_ids = Vec::new();

    for slot in body_slots {
        body_ids.push(crate::ids::history_input_body_id(&prefix, slot));
    }
    let state = crate::ids::history_input_state_id(&prefix);
    let native = group.id.clone();
    if let Ok(historical) = cadmpeg_ir::features::BodySelection::historical(state, body_ids, native)
    {
        *selection = historical;
    }
}

fn bind_direct_body_recipe_body_selection<'a>(
    selection: &mut cadmpeg_ir::features::BodySelection,
    scope: &crate::records::feature::scope::DesignParameterScope,
    inputs: &FeatureBodySelectionInputs<'a>,
    body_candidates: &mut BodyCandidates<'a>,
) {
    use cadmpeg_ir::features::{BodyMember, BodySelection};

    let groups = inputs.groups;
    let operands = inputs.body_recipe_operands;

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
                return;
            };
            if matching_groups.next().is_some() || group.members().is_empty() {
                return;
            }
            let mut selected = Vec::new();
            for (ordinal, record_index) in group
                .members()
                .iter()
                .map(|member| member.value)
                .enumerate()
            {
                let Ok(ordinal) = u32::try_from(ordinal) else {
                    return;
                };
                let mut matching_operands = operands.iter().filter(|operand| {
                    operand.owner.group() == Some((group.record_index, ordinal))
                        && operand.record_index() == record_index
                        && crate::ids::native_stream(&operand.id) == stream
                });
                let Some(operand) = matching_operands.next() else {
                    return;
                };
                if matching_operands.next().is_some() {
                    return;
                }
                let Some(body) = body_candidates.direct(operand) else {
                    return;
                };
                if selected.contains(&body) {
                    return;
                }

                selected.push(body);
            }
            let Ok(bodies) = cadmpeg_ir::features::DistinctMembers::try_from(selected) else {
                return;
            };
            let native = group.id.clone();
            *selection = BodySelection::Resolved { bodies, native };
            return;
        }
        BodySelection::NativeSet(native) => native.iter().map(String::as_str).collect::<Vec<_>>(),
        _ => return,
    };
    if native_members.is_empty() {
        return;
    }
    for (index, native) in native_members.iter().enumerate() {
        for previous in &native_members[..index] {
            if native == previous {
                return;
            }
        }
    }
    let mut rows = Vec::new();
    for native in &native_members {
        let Some((native_stream_name, record_index)) = native.rsplit_once(":design-record#") else {
            return;
        };
        let Ok(record_index) = record_index.parse::<u32>() else {
            return;
        };
        if Some(native_stream_name) != stream {
            return;
        }
        let mut matching_operands = operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == Some(native_stream_name)
                && operand.scope_record_index == scope.record_index
                && matches!(
                    operand.owner,
                    crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
                )
                && operand.record_index() == record_index
        });
        let Some(operand) = matching_operands.next() else {
            return;
        };
        if matching_operands.next().is_some() {
            return;
        }
        let Some(body) = body_candidates.direct(operand) else {
            return;
        };
        if rows.iter().any(|row: &BodyMember<_>| row.body() == &body) {
            return;
        }
        let native = (*native).to_owned();
        let Some(row) = body_member(body, native) else {
            return;
        };

        rows.push(row);
    }
    if let Ok(members) = cadmpeg_ir::features::BodyMembers::try_from_rows(rows) {
        *selection = BodySelection::ResolvedSet { members };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodySelectionProof {
    TopologyStableRevision,
    RevisedInput,
}

fn unique_feature_history_states(history: &AsmHistory) -> HashMap<i64, Option<&AsmDeltaState>> {
    let mut states = HashMap::new();
    for state in &history.states {
        states
            .entry(state.state_id)
            .and_modify(|existing| *existing = None)
            .or_insert(Some(state));
    }
    states
}

fn singleton_revised_input_body_across_state_chain<'a>(
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Option<i64> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut revised = BTreeSet::new();
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        let transition = current.transition.as_ref()?;
        for &body in transition
            .topology
            .bodies
            .updated
            .iter()
            .chain(&transition.topology.bodies.deleted)
        {
            revised.insert(body);
        }
        let previous_id = transition.previous_state_id?;
        let Some(Some(previous)) = states.get(&previous_id) else {
            return None;
        };
        current = previous;
    }
    let input = current.topology()?;
    let mut candidates = input.bodies.iter().filter(|body| revised.contains(body));
    let body = candidates.next().copied()?;
    candidates.next().is_none().then_some(body)
}

fn singleton_body_revision_across_state_chain<'a>(
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Option<i64> {
    let result_topology = state.topology()?;
    let mut current = state;
    let mut visited = HashSet::new();
    let mut selected = None;
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        let revision = body_revision_without_topology_change(current)?;
        if let TopologyStableBodyRevision::Revised(body) = revision {
            match selected {
                None => selected = Some(body),
                Some(selected) if selected == body => {}
                Some(_) => return None,
            }
        }
        let previous = current
            .transition
            .as_ref()
            .and_then(|transition| transition.previous_state_id)?;
        let Some(Some(previous)) = states.get(&previous) else {
            return None;
        };
        current = previous;
    }
    let body = selected?;
    let input_topology = current.topology()?;
    (result_topology.bodies.contains(&body) && input_topology.bodies.contains(&body))
        .then_some(body)
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
                let Some(previous_state_id) =
                    effective_scope_previous_history_state_id(scope, histories)
                else {
                    break 'feature_edit;
                };
                let Some((history, state, previous)) =
                    unique_history_state_pair(histories, state_id, previous_state_id)
                else {
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
                                face,
                                scope,
                                groups,
                                operands,
                                &transition.topology.faces.updated,
                            ));
                            bind_entity_face_selection(
                                face,
                                FaceSelectionBinding {
                                    feature_id,
                                    previous_state_id,
                                    operation_history_id: &history.id,
                                    scope,
                                    input_topologies: &mut *input_topologies,
                                },
                                groups,
                                entity_operands,
                            );
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
                                faces,
                                scope,
                                groups,
                                operands,
                                &transition.topology.faces.updated,
                            ));
                            bind_entity_face_selection(
                                faces,
                                FaceSelectionBinding {
                                    feature_id,
                                    previous_state_id,
                                    operation_history_id: &history.id,
                                    scope,
                                    input_topologies: &mut *input_topologies,
                                },
                                groups,
                                entity_operands,
                            );
                        }
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::MoveFace { faces, .. },
                    ) => {
                        admitted!(selection::bind_face_selection(
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
                            faces,
                            scope,
                            groups,
                            operands,
                            &transition.topology.faces.updated,
                        ));
                        selection::bind_body_recipe_face_selection(
                            faces,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            body_recipe_operands,
                        );
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::KnitSurface { faces, .. },
                    ) => {
                        bind_surface_stitch_face_selection(
                            faces,
                            FaceSelectionBinding {
                                feature_id,
                                previous_state_id,
                                operation_history_id: &history.id,
                                scope,
                                input_topologies: &mut *input_topologies,
                            },
                            groups,
                            entity_operands,
                        );
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::SplitFace { targets, .. },
                    ) => {
                        admitted!(selection::bind_face_selection(
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
                        bind_hole_face_selection(
                            face,
                            FaceSelectionBinding {
                                feature_id,
                                previous_state_id,
                                operation_history_id: &history.id,
                                scope,
                                input_topologies: &mut *input_topologies,
                            },
                        );
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
    selection: &mut cadmpeg_ir::features::FaceSelection,
    input: FaceSelectionBinding<'_>,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
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
        _ => return,
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == group_id
            && group.scope_record_index == scope.record_index
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return;
    }
    bind_entity_face_groups(
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
    );
}

// Keep the serialized-selection context explicit at this boundary so every
// admission input remains visible to the strict all-members proof.
fn bind_surface_stitch_face_selection(
    selection: &mut cadmpeg_ir::features::FaceSelection,
    input: FaceSelectionBinding<'_>,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;
    let native_id = match selection {
        cadmpeg_ir::features::FaceSelection::Native(native_id) => native_id.as_str(),
        _ => return,
    };
    if native_id != scope.id {
        return;
    }
    let Some(input_end) = scope.reference_members().len().checked_sub(2) else {
        return;
    };
    if input_end == 0 || !input_end.is_multiple_of(2) {
        return;
    }
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups
        .iter()
        .filter(|group| {
            crate::ids::native_stream(&group.id) == stream
                && group.scope_record_index == scope.record_index
                && group.role() == DesignOperandRole::ROLE_0X5
                && group.extrude_role().is_none()
                && group.extrude_face_role().is_none()
        })
        .collect::<Vec<_>>();
    matching_groups.sort_by_key(|a| a.scope_reference_ordinal);
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
        return;
    }
    bind_entity_face_groups(
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
    );
}

// Keep the shared selection-binding inputs explicit; this helper is the
// single admission point for both one-group and SurfaceStitch selections.
fn bind_entity_face_groups(
    selection: &mut cadmpeg_ir::features::FaceSelection,
    native_id: &str,
    input: FaceSelectionBinding<'_>,
    groups: &[&crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;

    if groups.is_empty() {
        return;
    }
    let mut selected = Vec::<(&str, i64, bool)>::new();
    let stream = crate::ids::native_stream(&scope.id);
    for group in groups {
        if group.members().is_empty() {
            return;
        }
        for (ordinal, record_index) in group
            .members()
            .iter()
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return;
            };
            let mut matches = operands.iter().filter(|operand| {
                operand.scope_record_index == scope.record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.record_index() == record_index
                    && crate::ids::native_stream(&operand.id) == stream
            });
            let Some(operand) = matches.next() else {
                return;
            };
            let [candidate] = operand.historical_face_candidates.as_slice() else {
                return;
            };
            if matches.next().is_some() {
                return;
            }
            let local = candidate.history_id == operation_history_id
                && candidate.historical.state_ids.contains(&previous_state_id);
            let Some(source) = historical_brep_source(&candidate.history_id) else {
                return;
            };
            if !selected.contains(&(source, candidate.face_slot, local)) {
                selected.push((source, candidate.face_slot, local));
            }
        }
    }
    let prefix = crate::ids::history_input_prefix(&feature_id.key(), previous_state_id);
    let state_id = crate::ids::history_input_state_id(&prefix);
    let mut topologies = input_topologies
        .iter_mut()
        .filter(|topology| topology.id == state_id && topology.input_of == *feature_id);
    let Some(topology) = topologies.next() else {
        return;
    };
    if topologies.next().is_some() {
        return;
    }
    let mut faces = Vec::new();
    for (source, face, local) in selected {
        let slot = if local {
            cadmpeg_ir::ids::IdentityKey::from(face)
        } else {
            let Ok(slot) =
                cadmpeg_ir::ids::IdentityKey::try_new(format!("{}:{source}:{face}", source.len()))
            else {
                return;
            };
            slot
        };
        let id = crate::ids::history_input_face_id(&prefix, slot);

        faces.push(id);
    }
    for face in &faces {
        if !topology.faces.contains(face) {
            let retained = face.clone();
            topology.faces.insert(retained);
        }
    }
    let native = native_id.to_owned();
    if let Ok(historical) = cadmpeg_ir::features::FaceSelection::historical(state_id, faces, native)
    {
        *selection = historical;
    }
}

fn bind_hole_face_selection(
    selection: &mut cadmpeg_ir::features::FaceSelection,
    input: FaceSelectionBinding<'_>,
) {
    use cadmpeg_ir::features::FaceSelection;
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;

    let FaceSelection::Native(native_id) = selection else {
        return;
    };
    let Some(construction) = scope.hole_construction() else {
        return;
    };
    let Some(face_selection) = &construction.face_selection else {
        return;
    };
    let [candidate] = face_selection.historical_face_candidates.as_slice() else {
        return;
    };
    let local = candidate.history_id == operation_history_id
        && candidate.historical.state_ids.contains(&previous_state_id);
    let Some(source) = historical_brep_source(&candidate.history_id) else {
        return;
    };
    let prefix = crate::ids::history_input_prefix(&feature_id.key(), previous_state_id);
    let state_id = crate::ids::history_input_state_id(&prefix);
    let mut topologies = input_topologies
        .iter_mut()
        .filter(|topology| topology.id == state_id && topology.input_of == *feature_id);
    let Some(topology) = topologies.next() else {
        return;
    };
    if topologies.next().is_some() {
        return;
    }
    let slot = if local {
        cadmpeg_ir::ids::IdentityKey::from(candidate.face_slot)
    } else {
        let Ok(slot) = cadmpeg_ir::ids::IdentityKey::try_new(format!(
            "{}:{source}:{}",
            source.len(),
            candidate.face_slot
        )) else {
            return;
        };
        slot
    };
    let face = crate::ids::history_input_face_id(&prefix, slot);
    if !topology.faces.contains(&face) {
        let retained = face.clone();
        topology.faces.insert(retained);
    }
    let native = native_id.to_owned();
    let selected = vec![face];
    if let Ok(historical) =
        cadmpeg_ir::features::FaceSelection::historical(state_id, selected, native)
    {
        *selection = historical;
    }
}

pub(crate) fn bind_feature_path_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SurfaceBoundary};

    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
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
                    bind_entity_selection_path(
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
                        bind_entity_selection_path(
                            path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                    }
                }
                FeatureDefinition::Operation(FeatureOperation::Sweep {
                    path, guide_rail, ..
                }) => {
                    if let Some(path) = path {
                        bind_entity_selection_path(
                            path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                    }
                    if let Some(guide_rail) = guide_rail {
                        bind_entity_selection_path(
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
    }
}

fn bind_entity_selection_path(
    path: &mut cadmpeg_ir::features::PathRef,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    use cadmpeg_ir::features::PathRef;

    let PathRef::Native(group_id) = path else {
        return;
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == *group_id
            && group.scope_record_index == scope.record_index
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return;
    }

    let mut edge_slots = Vec::new();

    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| member.value)
        .enumerate()
    {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return;
        };
        let mut matching_operands = operands.iter().filter(|operand| {
            operand.group_record_index == group.record_index
                && operand.group_member_ordinal == ordinal
                && operand.record_index() == record_index
                && crate::ids::native_stream(&operand.id) == stream
        });
        let Some(operand) = matching_operands.next() else {
            return;
        };
        if matching_operands.next().is_some() {
            return;
        }
        let Some(edge_slot) = operand.resolved_edge_slot else {
            return;
        };
        edge_slots.push(edge_slot);
    }

    let prefix = crate::ids::history_input_prefix(&feature_id.key(), previous_state_id);
    let mut edge_ids = Vec::new();

    for slot in edge_slots {
        edge_ids.push(crate::ids::history_input_edge_id(&prefix, slot));
    }
    let state = crate::ids::history_input_state_id(&prefix);
    let native = group.id.clone();
    if let Ok(historical) = PathRef::historical_edges(state, edge_ids, native) {
        *path = historical;
    }
}

pub(crate) fn project_feature_input_topologies(
    features: &[cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    edge_operands: &[crate::records::topology::edge_identity::DesignEdgeOperand],
    scope_histories: &HashMap<String, String>,
) -> Vec<cadmpeg_ir::features::FeatureInputTopology> {
    use cadmpeg_ir::features::FeatureInputTopology;

    let mut projected = Vec::new();
    for feature in features {
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
        let scoped_history = if scope_histories.contains_key(&scope.id) {
            let Some(history) = bound_scope_history(&scope.id, scope_histories, histories) else {
                continue;
            };
            Some(history)
        } else {
            None
        };
        let scoped_histories = scoped_history.map_or(histories, std::slice::from_ref);
        let Some(previous_state_id) = scope
            .previous_history_state_id()
            .or_else(|| {
                crate::design::feature_project::work_point_recipe_state_id(scope, edge_operands)
            })
            .or_else(|| crate::design::feature_project::work_plane_recipe_state_id(scope))
            .or_else(|| effective_scope_previous_history_state_id(scope, scoped_histories))
        else {
            continue;
        };
        let Some(state) = scope
            .history_state_id()
            .and_then(|state_id| {
                unique_history_state_pair(scoped_histories, state_id, previous_state_id)
                    .map(|(_, _, previous)| previous)
            })
            .or_else(|| {
                unique_history_state(scoped_histories, previous_state_id).map(|(_, state)| state)
            })
        else {
            continue;
        };
        let Some(topology) = state.topology() else {
            continue;
        };
        let prefix = crate::ids::history_input_prefix(&feature.id.key(), previous_state_id);
        let Some(bodies) = project_input_members(&topology.bodies, |slot| {
            crate::ids::history_input_body_id(&prefix, slot)
        }) else {
            continue;
        };
        let Some(faces) = project_input_members(&topology.faces, |slot| {
            crate::ids::history_input_face_id(&prefix, slot)
        }) else {
            continue;
        };
        let Some(edges) = project_input_members(&topology.edges, |slot| {
            crate::ids::history_input_edge_id(&prefix, slot)
        }) else {
            continue;
        };
        let Some(vertices) = project_input_members(&topology.vertices, |slot| {
            crate::ids::history_input_vertex_id(&prefix, slot)
        }) else {
            continue;
        };
        let id = crate::ids::history_input_state_id(&prefix);
        let input_of = feature.id.clone();
        let native_ref = state.id.clone();

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
    projected
}

fn project_input_members<T: Eq + std::hash::Hash>(
    slots: &[i64],
    id: impl FnMut(i64) -> T,
) -> Option<cadmpeg_ir::features::DistinctMembers<T>> {
    let members = slots.iter().copied().map(id).collect::<Vec<_>>();
    cadmpeg_ir::features::DistinctMembers::try_from(members).ok()
}

/// Resolve persistent vertex recipes in the last history-bearing feature state
/// that precedes their owning construction in authored timeline order.
pub(crate) fn bind_vertex_recipe_history(
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    timelines: &[crate::records::entity_header::DesignFeatureTimeline],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut topology_queries = TopologyQueryCache::default();
    let source_ordinals =
        crate::design::feature_project::authored_scope_ordinals_per_stream(scopes, timelines)?;
    let mut input_states = HashMap::new();
    for scope in scopes.iter().filter(|scope| {
        matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::WorkPlane
                | crate::records::feature::scope::DesignFeatureKind::WorkPoint
        )
    }) {
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        let Some(&ordinal) = source_ordinals.get(&(stream, scope.record_index)) else {
            continue;
        };
        let mut predecessors = scopes.iter().filter_map(|candidate| {
            let candidate_stream =
                crate::ids::native_stream(&candidate.id).unwrap_or(crate::ids::DEFAULT_STREAM);
            let candidate_ordinal =
                *source_ordinals.get(&(candidate_stream, candidate.record_index))?;
            (candidate_stream == stream && candidate_ordinal < ordinal)
                .then_some((candidate_ordinal, candidate.history_state_id()?))
        });
        let Some(predecessor) = predecessors.next() else {
            continue;
        };
        let predecessor = predecessors.fold(predecessor, |latest, candidate| {
            if candidate.0 > latest.0 {
                candidate
            } else {
                latest
            }
        });
        let id = scope.id.clone();
        input_states.insert(id, predecessor.1);
    }

    for scope in scopes.iter_mut().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPoint
    }) {
        let state_id = input_states.get(&scope.id).copied();
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
            let Some((_, state)) = unique_history_state(histories, state_id) else {
                continue;
            };
            let Some(topology) = state.topology() else {
                continue;
            };
            let Some((vertex, position)) =
                vertex_recipe_candidate(recipe, topology, &mut topology_queries)
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

    for scope in scopes.iter_mut().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPlane
    }) {
        let transform = scope.work_plane_transform();
        let state_id = input_states.get(&scope.id).copied();
        let Some(construction) = scope.work_plane_construction_mut() else {
            continue;
        };
        construction.clear_resolution();
        let Some(state_id) = state_id else {
            continue;
        };
        let Some((_, state)) = unique_history_state(histories, state_id) else {
            continue;
        };
        let Some(topology) = state.topology() else {
            continue;
        };
        let [first, second, third] = construction.inputs();
        let (Some(first), Some(second), Some(third)) = (
            vertex_recipe_candidate(first, topology, &mut topology_queries),
            vertex_recipe_candidate(second, topology, &mut topology_queries),
            vertex_recipe_candidate(third, topology, &mut topology_queries),
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
    operands: &mut [crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut topology_queries = TopologyQueryCache::default();
    let mut recipe_topologies = RecipeTopologyCache::default();
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
        let (Some(state_id), Some(previous_state_id)) = (
            scope.history_state_id(),
            effective_scope_previous_history_state_id(scope, histories),
        ) else {
            continue;
        };
        let Some((_, _, previous)) = bound_history_state_pair(
            &scope.id,
            state_id,
            previous_state_id,
            scope_histories,
            histories,
        ) else {
            continue;
        };
        let Some(topology) = previous.topology() else {
            continue;
        };
        for reference in &mut operand.recipe.recipe_references {
            bind_historical_recipe_reference_candidates(
                reference,
                topology,
                &mut recipe_topologies,
            )?;
        }
        let Some(vertex) =
            recipe_reference_common_vertex(&operand.recipe, topology, &mut topology_queries)
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

fn vertex_recipe_candidate<'a>(
    recipe: &crate::records::feature::work_geometry::DesignVertexRecipe,
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<(i64, cadmpeg_ir::math::Point3)> {
    let mut face_slots = Vec::new();
    for reference in &recipe.recipe_references {
        let mut slots = reference
            .candidate_faces
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .filter(|face| topology.faces.contains(face))
            .collect::<Vec<_>>();
        slots.sort_unstable_by(Ord::cmp);
        slots.dedup();
        let [slot] = slots.as_slice() else {
            return None;
        };

        face_slots.push(*slot);
    }
    if face_slots.is_empty() {
        return None;
    }
    let vertex = common_face_vertex(&face_slots, topology, topology_queries)?;
    unique_historical_vertex_position(vertex, topology).map(|position| (vertex, position))
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
    faces: impl IntoIterator<Item = i64>,
    index: &VertexBoundaryIndex,
    edge_endpoints: &topology_cache::EdgeEndpoints,
) -> Option<HashSet<i64>> {
    let mut vertices = HashSet::new();
    for face in faces {
        let edges = index.boundaries.get(&face)?;
        for edge_slot in edges {
            let [start, end] = edge_endpoints.get(edge_slot).copied().flatten()?;
            if !index.vertices.contains(&start) || !index.vertices.contains(&end) {
                return None;
            }
            vertices.insert(start);
            vertices.insert(end);
        }
    }
    (!vertices.is_empty()).then_some(vertices)
}

fn common_face_vertex<'a>(
    face_slots: &[i64],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<i64> {
    let index = topology_queries
        .vertices
        .get(topology, VertexBoundaryIndex::new);
    let edge_endpoints = topology_queries
        .endpoints
        .get(topology, topology_cache::endpoint_index);
    let mut common = None::<HashSet<i64>>;
    for face in face_slots {
        let vertices = boundary_vertices_for_faces(std::iter::once(*face), index, edge_endpoints)?;
        if let Some(common) = &mut common {
            common.retain(|vertex| vertices.contains(vertex));
        } else {
            common = Some(vertices);
        }
    }
    let common = common?;
    if common.len() == 1 {
        common.into_iter().next()
    } else {
        None
    }
}

fn recipe_reference_common_vertex<'a>(
    recipe: &crate::records::feature::work_geometry::DesignVertexRecipe,
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<i64> {
    let index = topology_queries
        .vertices
        .get(topology, VertexBoundaryIndex::new);
    let edge_endpoints = topology_queries
        .endpoints
        .get(topology, topology_cache::endpoint_index);
    let mut common = None::<HashSet<i64>>;
    for reference in &recipe.recipe_references {
        let faces = reference
            .candidate_faces
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .filter(|face| index.faces.contains(face));
        let vertices = boundary_vertices_for_faces(faces, index, edge_endpoints)?;
        if let Some(common) = &mut common {
            common.retain(|vertex| vertices.contains(vertex));
        } else {
            common = Some(vertices);
        }
    }
    let common = common?;
    if common.len() == 1 {
        common.into_iter().next()
    } else {
        None
    }
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

fn unique_history_state(
    histories: &[AsmHistory],
    state_id: i64,
) -> Option<(&AsmHistory, &AsmDeltaState)> {
    let mut matches = histories.iter().filter_map(|history| {
        let mut states = history
            .states
            .iter()
            .filter(|state| state.state_id == state_id);
        let state = states.next()?;
        states.next().is_none().then_some((history, state))
    });
    let state = matches.next()?;
    matches.next().is_none().then_some(state)
}

fn unique_history_state_in(history: &AsmHistory, state_id: i64) -> bool {
    let mut states = history
        .states
        .iter()
        .filter(|state| state.state_id == state_id);
    states.next().is_some() && states.next().is_none()
}

/// Return the effective input state for a scope. Some Design scope envelopes
/// omit the preceding state identity even though the current ASM delta state
/// carries the direct transition predecessor.
pub(crate) fn effective_scope_previous_history_state_id(
    scope: &crate::records::feature::scope::DesignParameterScope,
    histories: &[AsmHistory],
) -> Option<i64> {
    scope.previous_history_state_id().or_else(|| {
        let state_id = scope.history_state_id()?;
        let (history, state) = unique_history_state(histories, state_id)?;
        linked_previous_state_id(history, state)
    })
}

pub(crate) fn unique_history_state_pair(
    histories: &[AsmHistory],
    state_id: i64,
    previous_state_id: i64,
) -> Option<(&AsmHistory, &AsmDeltaState, &AsmDeltaState)> {
    let mut direct = histories
        .iter()
        .filter_map(|history| history_state_pair(history, state_id, previous_state_id, true));
    if let Some(pair) = direct.next() {
        return direct.next().is_none().then_some(pair);
    }
    let mut matches = histories
        .iter()
        .filter_map(|history| history_state_pair(history, state_id, previous_state_id, false));
    let pair = matches.next()?;
    matches.next().is_none().then_some(pair)
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
    scope: &crate::records::feature::scope::DesignParameterScope,
    edge_slot: i64,
    histories: &[AsmHistory],
) -> HemGeometrySemantics {
    let unresolved = HemGeometrySemantics {
        direction: None,
        gap_length_form: None,
    };
    let (Some(state_id), Some(previous_state_id)) = (
        scope.history_state_id(),
        effective_scope_previous_history_state_id(scope, histories),
    ) else {
        return unresolved;
    };
    let Some((_, state, previous)) =
        unique_history_state_pair(histories, state_id, previous_state_id)
    else {
        return unresolved;
    };
    let (Some(previous_topology), Some(transition)) =
        (previous.topology(), state.transition.as_ref())
    else {
        return unresolved;
    };
    let Some(current_topology) = state.topology() else {
        return unresolved;
    };
    let inserted_surfaces = &transition.topology.surfaces.inserted;
    if !current_topology
        .surface_cylinders
        .iter()
        .any(|cylinder| inserted_surfaces.contains(&cylinder.surface))
    {
        return unresolved;
    }
    let edge_direction =
        historical_edge_axis(edge_slot, previous_topology).map(|(_, direction)| direction);
    let cylinders = || {
        current_topology
            .surface_cylinders
            .iter()
            .filter(|cylinder| {
                inserted_surfaces.contains(&cylinder.surface)
                    && edge_direction
                        .is_none_or(|direction| parallel_directions(direction, cylinder.axis))
            })
    };
    if cylinders().next().is_none() {
        return unresolved;
    }
    HemGeometrySemantics {
        direction: hem_direction_from_transition(
            edge_slot,
            &cylinders(),
            previous_topology,
            transition,
        ),
        gap_length_form: hem_gap_length_form(cylinders()),
    }
}

fn hem_gap_length_form<'a>(
    mut cylinders: impl Iterator<Item = &'a AsmHistoricalCylinder>,
) -> Option<HemGapLengthForm> {
    let first = cylinders.next()?;
    let second = cylinders.next()?;
    if cylinders.next().is_some() {
        return None;
    }
    if !same_axis_line((first.origin, first.axis), (second.origin, second.axis)) {
        return None;
    }
    let [inner, outer] = if first.radius <= second.radius {
        [first.radius, second.radius]
    } else {
        [second.radius, first.radius]
    };
    if !inner.is_finite() || !outer.is_finite() || inner < 0.0 || outer <= inner {
        return None;
    }
    let thickness = outer - inner;
    let tolerance = EPS_HISTORY_HEM_GAP_LENGTH_FORM_E7 * (1.0 + outer.abs() + inner.abs());
    if (2.0 * inner - thickness).abs() <= tolerance {
        Some(HemGapLengthForm::Open)
    } else if 2.0 * inner < thickness - tolerance {
        Some(HemGapLengthForm::Flat)
    } else {
        None
    }
}

fn hem_direction_from_transition<'a>(
    edge_slot: i64,
    cylinders: &(impl Iterator<Item = &'a AsmHistoricalCylinder> + Clone),
    previous: &AsmHistoricalTopology,
    transition: &AsmHistoricalTransition,
) -> Option<cadmpeg_ir::features::SheetMetalHemDirection> {
    (*cylinders).clone().next()?;
    let edge_context = selection::historical_edge_context(edge_slot, previous);
    let mut candidate = None;
    for (ordinal, incident) in edge_context.incident_loops.iter().enumerate() {
        let face = incident.face_slot;
        if edge_context.incident_loops[..ordinal]
            .iter()
            .any(|earlier| earlier.face_slot == face)
        {
            continue;
        }
        if transition.topology.faces.deleted.contains(&face) {
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
        let Some(first_cylinder) = (*cylinders).clone().next() else {
            continue;
        };
        let first = normal.dot(first_cylinder.origin.vector_from(plane.origin));
        if !first.is_finite() {
            continue;
        }
        let sign_tolerance = EPS_HISTORY_HEM_DIRECTION_FROM_TRANSITION_E7
            * (1.0
                + plane.origin.x.abs()
                + plane.origin.y.abs()
                + plane.origin.z.abs()
                + (*cylinders).clone().fold(0.0_f64, |scale, cylinder| {
                    scale
                        .max(cylinder.origin.x.abs())
                        .max(cylinder.origin.y.abs())
                        .max(cylinder.origin.z.abs())
                }));
        if first.abs() <= sign_tolerance
            || (*cylinders).clone().any(|cylinder| {
                let offset = normal.dot(cylinder.origin.vector_from(plane.origin));
                !offset.is_finite()
                    || offset.abs() <= sign_tolerance
                    || offset.is_sign_positive() != first.is_sign_positive()
            })
        {
            continue;
        }
        if candidate.replace(first.is_sign_positive()).is_some() {
            return None;
        }
    }
    let candidate = candidate?;
    Some(if candidate {
        cadmpeg_ir::features::SheetMetalHemDirection::Forward
    } else {
        cadmpeg_ir::features::SheetMetalHemDirection::Reverse
    })
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
    history: &AsmHistory,
    state_id: i64,
    previous_state_id: i64,
    require_direct: bool,
) -> Option<(&AsmHistory, &AsmDeltaState, &AsmDeltaState)> {
    let mut states = history
        .states
        .iter()
        .filter(|state| state.state_id == state_id);
    let state = states.next()?;
    if states.next().is_some()
        || (require_direct && linked_previous_state_id(history, state) != Some(previous_state_id))
    {
        return None;
    }
    let mut previous_states = history
        .states
        .iter()
        .filter(|state| state.state_id == previous_state_id);
    let previous = previous_states.next()?;
    if previous_states.next().is_some()
        || (!require_direct && !history_state_reaches(history, state, previous_state_id))
    {
        return None;
    }
    Some((history, state, previous))
}

/// Return the one ASM history bound to a Design scope.
pub(crate) fn bound_scope_history<'a>(
    scope_id: &str,
    scope_histories: &HashMap<String, String>,
    histories: &'a [AsmHistory],
) -> Option<&'a AsmHistory> {
    let history_id = scope_histories.get(scope_id)?;
    let mut matches = histories.iter().filter(|history| &history.id == history_id);
    let history = matches.next()?;
    matches.next().is_none().then_some(history)
}

fn bound_history_state_pair<'a>(
    scope_id: &str,
    state_id: i64,
    previous_state_id: i64,
    scope_histories: &HashMap<String, String>,
    histories: &'a [AsmHistory],
) -> Option<(&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState)> {
    history_state_pair(
        bound_scope_history(scope_id, scope_histories, histories)?,
        state_id,
        previous_state_id,
        false,
    )
}

fn insert_scope_history_binding(
    resolved: &mut HashMap<String, String>,
    scope_id: &str,
    history_id: &str,
) {
    let scope = scope_id.to_owned();
    let history = history_id.to_owned();
    resolved.insert(scope, history);
}

pub(crate) fn bind_scope_histories(
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    body_bindings: &[crate::records::bodies::DesignBodyBinding],
    body_recipe_operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    histories: &[AsmHistory],
) -> HashMap<String, String> {
    let mut candidates = Vec::new();
    for scope in scopes {
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let scope_candidates = if let Some(previous_state_id) = scope.previous_history_state_id() {
            let direct = histories
                .iter()
                .filter(|history| {
                    history_state_pair(history, state_id, previous_state_id, true).is_some()
                })
                .collect::<Vec<_>>();
            if direct.is_empty() {
                histories
                    .iter()
                    .filter(|history| {
                        history_state_pair(history, state_id, previous_state_id, false).is_some()
                    })
                    .collect::<Vec<_>>()
            } else {
                direct
            }
        } else {
            histories
                .iter()
                .filter(|history| unique_history_state_in(history, state_id))
                .collect::<Vec<_>>()
        };
        if !scope_candidates.is_empty() {
            candidates.push((scope, scope_candidates));
        }
    }
    let mut resolved = HashMap::<String, String>::new();
    for (scope, candidates) in &candidates {
        if candidates.len() == 1 {
            insert_scope_history_binding(&mut resolved, &scope.id, &candidates[0].id);
            continue;
        }
        let next_scope_record_index = scopes
            .iter()
            .filter(|candidate| {
                crate::ids::same_native_occurrence(&candidate.id, &scope.id)
                    && candidate.record_index > scope.record_index
            })
            .map(|candidate| candidate.record_index)
            .min();
        let mut output_bindings = body_bindings.iter().filter(|binding| {
            crate::ids::same_native_occurrence(binding.id(), &scope.id)
                && binding.entity_suffix > u64::from(scope.record_index)
                && next_scope_record_index
                    .is_none_or(|next| binding.entity_suffix < u64::from(next))
        });
        if let Some(binding) = output_bindings.next() {
            if output_bindings.next().is_none() {
                let mut matching = candidates.iter().filter(|history| {
                    historical_brep_source(&history.id).is_some_and(|source| {
                        binding.blob_name().strip_prefix("BREP.") == Some(source)
                    })
                });
                if let Some(history) = matching.next().filter(|_| matching.next().is_none()) {
                    insert_scope_history_binding(&mut resolved, &scope.id, &history.id);
                    continue;
                }
            }
        }
        let candidate_faces = body_recipe_operands
            .iter()
            .filter(|operand| {
                crate::ids::same_native_occurrence(&operand.id, &scope.id)
                    && operand.scope_record_index == scope.record_index
            })
            .flat_map(super::records::topology::body_recipe::DesignBodyRecipeOperand::references)
            .flat_map(|reference| &reference.candidate_faces);
        if candidate_faces.clone().next().is_some() {
            let mut matching = candidates.iter().filter(|history| {
                historical_brep_source(&history.id).is_some_and(|source| {
                    candidate_faces
                        .clone()
                        .any(|face| active_brep_face_matches_source(face, source))
                })
            });
            if let Some(history) = matching.next().filter(|_| matching.next().is_none()) {
                insert_scope_history_binding(&mut resolved, &scope.id, &history.id);
                continue;
            }
        }
        let Some(construction) = scope.base_feature_construction() else {
            continue;
        };
        let mut referenced_histories = construction.body_reference_records().filter_map(|suffix| {
            let mut bindings = body_bindings.iter().filter(|binding| {
                crate::ids::same_native_occurrence(binding.id(), &scope.id)
                    && binding.entity_suffix == u64::from(suffix)
            });
            let binding = bindings.next()?;
            if bindings.next().is_some() {
                return None;
            }
            let mut matching = candidates.iter().filter(|history| {
                historical_brep_source(&history.id)
                    .is_some_and(|source| binding.blob_name().strip_prefix("BREP.") == Some(source))
            });
            let history = matching.next()?;
            matching.next().is_none().then_some(history.id.as_str())
        });
        let Some(history_id) = referenced_histories.next() else {
            continue;
        };
        if referenced_histories.all(|candidate| candidate == history_id) {
            insert_scope_history_binding(&mut resolved, &scope.id, history_id);
        }
    }
    let mut groups = HashMap::<(&str, i64, Option<i64>), Vec<usize>>::new();
    for (index, (scope, _)) in candidates.iter().enumerate() {
        let (Some(stream), Some(state_id)) = (
            crate::ids::native_stream(&scope.id),
            scope.history_state_id(),
        ) else {
            continue;
        };
        let key = (stream, state_id, scope.previous_history_state_id());
        groups.entry(key).or_default().push(index);
    }
    for members in groups.values() {
        let mut candidate_histories = HashSet::new();
        for index in members {
            for history in &candidates[*index].1 {
                candidate_histories.insert(history.id.as_str());
            }
        }
        if candidate_histories.len() != members.len() {
            continue;
        }
        loop {
            let mut assigned = HashSet::new();
            for index in members {
                if let Some(history_id) = resolved.get(&candidates[*index].0.id) {
                    let copy = history_id.to_owned();
                    assigned.insert(copy);
                }
            }
            if assigned.len()
                != members
                    .iter()
                    .filter(|index| resolved.contains_key(&candidates[**index].0.id))
                    .count()
            {
                break;
            }
            let mut progress = false;
            for index in members {
                let (scope, scope_candidates) = &candidates[*index];
                if resolved.contains_key(&scope.id) {
                    continue;
                }
                let mut remaining = scope_candidates
                    .iter()
                    .filter(|history| !assigned.contains(history.id.as_str()));
                if let Some(history) = remaining.next().filter(|_| remaining.next().is_none()) {
                    insert_scope_history_binding(&mut resolved, &scope.id, &history.id);
                    progress = true;
                }
            }
            if !progress {
                break;
            }
        }
    }
    resolved
}

fn history_state_reaches(
    history: &AsmHistory,
    state: &AsmDeltaState,
    previous_state_id: i64,
) -> bool {
    let mut current = state;
    for _ in 0..=history.states.len() {
        if current.state_id == previous_state_id {
            return true;
        }
        let Some(previous_state_id) = linked_previous_state_id(history, current) else {
            return false;
        };
        let mut states = history
            .states
            .iter()
            .filter(|state| state.state_id == previous_state_id);
        let Some(previous) = states.next() else {
            return false;
        };
        if states.next().is_some() {
            return false;
        }
        current = previous;
    }
    false
}

/// Return the state ID reached by a delta state's `next` link.
///
/// Reject disagreement between the derived transition predecessor and the raw
/// `next` chain.
fn linked_previous_state_id(history: &AsmHistory, state: &AsmDeltaState) -> Option<i64> {
    let linked = state.next_ref.and_then(|node_index| {
        let mut states = history
            .states
            .iter()
            .filter(|candidate| candidate.node_index == node_index);
        let previous = states.next()?;
        states.next().is_none().then_some(previous.state_id)
    });
    let derived = state
        .transition
        .as_ref()
        .and_then(|transition| transition.previous_state_id);
    match (derived, linked) {
        (Some(derived), Some(linked)) if derived != linked => None,
        (Some(derived), _) => Some(derived),
        (None, Some(linked)) => Some(linked),
        (None, None) => None,
    }
}

fn history_state_index(history: &AsmHistory) -> HashMap<i64, Option<&AsmDeltaState>> {
    let mut states = HashMap::new();
    for state in &history.states {
        states
            .entry(state.state_id)
            .and_modify(|state| *state = None)
            .or_insert(Some(state));
    }
    states
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
fn bind_historical_recipe_reference_candidates<'a>(
    reference: &mut crate::records::dimensions::DesignRecipeReference,
    topology: &'a AsmHistoricalTopology,
    recipe_topologies: &mut RecipeTopologyCache<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    reference.candidate_faces.clear();
    reference.candidate_edges.clear();
    reference.alternate_selector_faces.clear();
    reference.alternate_selector_edges.clear();
    let index = recipe_topologies.get(topology);
    for tag_index in index.matching_tags(&reference.token, reference.design_reference) {
        let tag = &index.topology.persistent_subentity_tags[*tag_index];
        match tag.entity_kind {
            AsmHistoricalEntityKind::Face => {
                reference
                    .candidate_faces
                    .push(historical_face_id(tag.entity_ref)?);
            }
            AsmHistoricalEntityKind::Edge => {
                reference
                    .candidate_edges
                    .push(historical_edge_id(tag.entity_ref)?);
            }
            _ => {}
        }
    }
    reference
        .candidate_faces
        .sort_by(|left, right| left.as_str().cmp(right.as_str()));
    reference.candidate_faces.dedup();
    reference
        .candidate_edges
        .sort_by(|left, right| left.as_str().cmp(right.as_str()));
    reference.candidate_edges.dedup();
    Ok(())
}

fn historical_recipe_faces<'a>(
    design_reference: i64,
    topology: &'a AsmHistoricalTopology,
    recipe_topologies: &mut RecipeTopologyCache<'a>,
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    let index = recipe_topologies.get(topology);
    let topology = index.topology;
    let mut faces = Vec::new();
    for tag_index in index.face_tags(design_reference) {
        let tag = &topology.persistent_subentity_tags[*tag_index];

        faces.push(historical_face_id(tag.entity_ref)?);
    }
    faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    faces.dedup();
    Ok(faces)
}

fn direct_face_recipe_candidates(
    recipe_kind: crate::records::recipes::ConstructionRecipeKind,
    references: &[crate::records::dimensions::DesignRecipeReference],
    recipe_record_index: i32,
) -> Option<Vec<cadmpeg_ir::ids::FaceId>> {
    if recipe_kind != crate::records::recipes::ConstructionRecipeKind::Face {
        return None;
    }
    let mut faces = Vec::new();
    for face in references
        .iter()
        .filter(|reference| reference.design_reference == i64::from(recipe_record_index))
        .flat_map(|reference| &reference.candidate_faces)
    {
        faces.push((face).clone());
    }
    faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    faces.dedup();
    (!faces.is_empty()).then_some(faces)
}

fn historical_face_id(slot: i64) -> Result<cadmpeg_ir::ids::FaceId, cadmpeg_core::CodecError> {
    let text = format!("f3d:brep:entity#{slot}");
    cadmpeg_ir::ids::FaceId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

fn historical_edge_id(slot: i64) -> Result<cadmpeg_ir::ids::EdgeId, cadmpeg_core::CodecError> {
    let text = format!("f3d:brep:entity#{slot}");
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
    let mut topology_queries = TopologyQueryCache::default();
    let mut state_indexes = SnapshotCache::default();
    let mut recipe_topologies = RecipeTopologyCache::default();
    if projection_was_finalized(histories) {
        return Ok(());
    }
    let recipe_record_indices = recipes
        .iter()
        .filter_map(|recipe| Some((recipe.id.as_str(), recipe.record_index?.value)))
        .collect::<std::collections::HashMap<_, _>>();
    for operand in &mut *operands {
        operand.preceding_candidate_faces.clear();
        operand.changed_candidate_faces.clear();
        operand.historical_support_contexts.clear();
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
        let scoped_history = if scope_histories.contains_key(&scope.id) {
            let Some(history) = bound_scope_history(&scope.id, scope_histories, histories) else {
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
            effective_scope_previous_history_state_id(scope, scoped_histories)
        else {
            continue;
        };
        let Some((history, state, previous)) = (if scoped_history.is_some() {
            bound_history_state_pair(
                &scope.id,
                state_id,
                previous_state_id,
                scope_histories,
                histories,
            )
        } else {
            unique_history_state_pair(histories, state_id, previous_state_id)
        }) else {
            continue;
        };
        let states = state_indexes.get(history, history_state_index);
        let Some(topology) = previous.topology() else {
            continue;
        };
        for reference in &mut operand.recipe_references {
            bind_historical_recipe_reference_candidates(
                reference,
                topology,
                &mut recipe_topologies,
            )?;
        }
        if let Some(recipe_record_index) = recipe_record_indices.get(operand.recipe_id.as_str()) {
            operand.candidate_faces = historical_recipe_faces(
                i64::from(*recipe_record_index),
                topology,
                &mut recipe_topologies,
            )?;
            operand.unreferenced_candidate_faces =
                (operand.candidate_faces.iter().filter(|face| {
                    !operand
                        .recipe_references
                        .iter()
                        .flat_map(|reference| &reference.candidate_faces)
                        .any(|candidate| candidate == *face)
                }))
                .cloned()
                .collect::<Vec<_>>();
            operand.alternate_selector_candidate_faces.clear();
        }
        let Some(changed_faces) = face_changes_across_state_chain(state, previous_state_id, states)
        else {
            continue;
        };
        let direct_face_candidates =
            if let Some(record_index) = recipe_record_indices.get(operand.recipe_id.as_str()) {
                direct_face_recipe_candidates(
                    operand.recipe_kind,
                    &operand.recipe_references,
                    *record_index,
                )
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
                .map(<[cadmpeg_ir::ids::FaceId]>::to_vec)
        } else {
            None
        };
        let nested_split_face_candidates = if scope.kind()
            == crate::records::feature::scope::DesignFeatureKind::SplitFace
            && exact_face_selection_group(operand, scope, operand_groups).is_some()
        {
            crate::design::face_resolve::nested_bounded_face_history_candidates(operand)
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
        .then(|| grouped_reference_face_candidate(operand, topology, &changed_faces))
        .flatten()
        .map(|face| vec![face]);
        let legacy_face_candidates = (|| {
            if feature_family != Some(crate::design::DesignFeatureFamily::Extrude) {
                return None;
            }
            let group_record_index = operand.group_record_index()?;
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
            groups.next()?;
            if groups.next().is_some() {
                return None;
            }
            let recipe_record_index = recipe_record_indices.get(operand.recipe_id.as_str())?;
            crate::design::face_resolve::legacy_face_recipe_reference_candidates(
                operand,
                *recipe_record_index,
            )
        })();
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
                crate::design::face_resolve::historical_face_operand_candidates(operand);
            &fallback_candidates
        };
        operand.preceding_candidate_faces =
            selection::faces_in_topology(history_candidates, topology);
        operand.changed_candidate_faces =
            (operand.preceding_candidate_faces.iter().filter(|face| {
                stable_ref(face.as_str()).is_some_and(|slot| changed_faces.contains(&slot))
            }))
            .cloned()
            .collect::<Vec<_>>();
        operand.historical_support_contexts = historical_face_support_contexts(
            history_candidates,
            history,
            topology,
            &changed_faces,
            &mut topology_queries,
        );
        if direct_face_candidates.is_some() {
            operand.resolved_face_slots = operand
                .preceding_candidate_faces
                .iter()
                .filter_map(|face| stable_ref(face.as_str()))
                .collect::<Vec<_>>();
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
                    &operand.recipe_references,
                    topology,
                    &changed_faces,
                );
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
                    crate::design::face_resolve::resolve_surface_delete_face_history_set(operand)
                        .unwrap_or_default()
                } else if preserves_stable_face_set {
                    if let Some(stable) =
                        crate::design::face_resolve::resolve_stable_bounded_face_history_set(
                            operand,
                        )
                    {
                        stable
                    } else {
                        crate::design::face_resolve::resolve_bounded_face_history_candidates(
                            operand,
                        )
                        .unwrap_or_default()
                    }
                } else if let Some(bounded) =
                    crate::design::face_resolve::resolve_bounded_face_history_candidates(operand)
                {
                    bounded
                } else {
                    let pattern = if feature_family
                        == Some(crate::design::DesignFeatureFamily::CircularPattern)
                    {
                        if let Some(result) = state.topology() {
                            resolve_pattern_face_by_surface_radius(
                                crate::design::face_resolve::face_operand_candidates(operand),
                                topology,
                                result,
                                &changed_faces,
                            )
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
            operand.resolved_face_slots =
                crate::design::face_resolve::resolve_face_operand_history_candidate_from(
                    operand, candidates,
                )
                .or_else(|| {
                    resolve_thread_face_by_transition(
                        scope,
                        candidates,
                        history,
                        topology,
                        &changed_faces,
                    )
                })
                .into_iter()
                .collect();
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
            operand.resolved_face_slots = resolve_split_tool_face(operand, topology)
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
                    operand,
                    topology,
                    result,
                    &transition.topology.bodies.inserted,
                    &mut topology_queries,
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
                    resolve_draft_face_by_surface_transition(operand, topology, result)
                {
                    operand.resolved_face_slots = vec![face];
                }
            }
        }
        if operand.resolved_face_slots.is_empty() {
            if let Some(candidates) = legacy_face_candidates {
                match select_legacy_extrude_face_candidate(
                    candidates,
                    topology,
                    &changed_faces,
                    historical_brep_source(&history.id),
                ) {
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
    operand: &crate::records::topology::face::DesignFaceOperand,
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
) -> Option<i64> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !matches!(
            crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program),
            Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { .. })
        )
        || operand.recipe_nodes.is_empty()
    {
        return None;
    }
    let mut candidate_slots = HashSet::new();
    for face in operand
        .candidate_faces
        .iter()
        .filter_map(|face| stable_ref(face.as_str()))
    {
        candidate_slots.insert(face);
    }
    if candidate_slots.is_empty() {
        return None;
    }
    let mut alternate_slots = BTreeSet::new();
    for face in operand
        .recipe_references
        .iter()
        .flat_map(|reference| &reference.alternate_selector_faces)
        .filter_map(|face| stable_ref(face.as_str()))
        .filter(|face| candidate_slots.contains(face))
    {
        alternate_slots.insert(face);
    }
    let mut exact_slots = BTreeSet::new();
    for face in operand
        .recipe_references
        .iter()
        .flat_map(|reference| &reference.candidate_faces)
        .filter_map(|face| stable_ref(face.as_str()))
        .filter(|face| candidate_slots.contains(face))
    {
        exact_slots.insert(face);
    }
    let has_alternates = !alternate_slots.is_empty();
    let candidates = if has_alternates {
        alternate_slots
    } else {
        exact_slots
    };
    if candidates.is_empty() {
        return None;
    }
    if !has_alternates {
        let mut faces = candidates.iter();
        let face = faces.next().copied()?;
        if faces.next().is_some() {
            return None;
        }
        return (preceding.faces.contains(&face) && result.faces.contains(&face)).then_some(face);
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
            return None;
        }
    }
    changed
}

fn resolve_pattern_face_by_surface_radius(
    candidates: &[cadmpeg_ir::ids::FaceId],
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Option<i64> {
    let mut candidate_faces = HashSet::new();
    for face in candidates
        .iter()
        .filter_map(|face| stable_ref(face.as_str()))
    {
        candidate_faces.insert(face);
    }
    if candidate_faces.is_empty() {
        return None;
    }
    let mut result_radius = None;
    let mut bound_candidates = HashSet::new();
    for binding in result
        .face_surfaces
        .iter()
        .filter(|binding| candidate_faces.contains(&binding.entity))
    {
        if !bound_candidates.insert(binding.entity) {
            return None;
        }
        let mut radii = result
            .surface_radii
            .iter()
            .filter(|radius| radius.surface == binding.carrier);
        let radius = radii.next().map(|radius| radius.radius)?;
        if radii.next().is_some() || !radius.is_finite() || radius <= 0.0 {
            return None;
        }
        match result_radius {
            None => result_radius = Some(radius.to_bits()),
            Some(expected) if expected == radius.to_bits() => {}
            Some(_) => return None,
        }
    }
    if bound_candidates != candidate_faces {
        return None;
    }
    let result_radius = result_radius?;
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
    let face = matches.next()?;
    matches.next().is_none().then_some(face)
}

fn resolve_split_tool_face(
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<i64> {
    if operand.group_record_index().is_some()
        || operand.group_member_ordinal().is_some()
        || operand.scope_reference_ordinal != 1
        || operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::Face
        || operand.recipe_program != [0, -1]
    {
        return None;
    }
    let [reference] = operand.recipe_references.as_slice() else {
        return None;
    };
    let candidates = selection::faces_in_topology(&reference.candidate_faces, topology);
    let [face] = candidates.as_slice() else {
        return None;
    };
    stable_ref(face.as_str())
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
    scope: &crate::records::feature::scope::DesignParameterScope,
    candidates: &[cadmpeg_ir::ids::FaceId],
    history: &AsmHistory,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Option<i64> {
    let construction = scope.thread_construction()?;
    let source = historical_brep_source(&history.id)?;
    let mut source_candidates = candidates
        .iter()
        .filter(|face| active_brep_face_matches_source(face, source));
    source_candidates.next()?;
    if source_candidates.next().is_some() {
        return None;
    }
    let minimum_radius = construction.diameters.minor() * 5.0;
    let maximum_radius = construction.diameters.major() * 5.0;
    if !minimum_radius.is_finite() || !maximum_radius.is_finite() {
        return None;
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
    let face = matching_faces.next()?;
    matching_faces
        .all(|candidate| candidate == face)
        .then_some(face)
}

fn grouped_reference_face_candidate(
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Option<cadmpeg_ir::ids::FaceId> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(
            &operand.recipe_prefix_bytes,
        )
    {
        return None;
    }
    let mut candidates = operand
        .recipe_references
        .iter()
        .map(|reference| reference.design_reference)
        .filter(|reference| {
            topology.faces.contains(reference) && changed_faces.contains(reference)
        });
    let face = candidates.next()?;
    candidates
        .all(|candidate| candidate == face)
        .then(|| crate::ids::brep_face_id(face))
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

fn resolve_bounded_face_recipe_target<'a>(
    operand: &crate::records::topology::face::DesignFaceOperand,
    preceding: &'a crate::history_records::AsmHistoricalTopology,
    result: &'a crate::history_records::AsmHistoricalTopology,
    inserted_bodies: &[i64],
    topology_queries: &mut TopologyQueryCache<'a>,
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
    for face in &preceding.faces {
        topology_faces.insert(*face);
    }
    let mut target_candidates = BTreeSet::new();
    for face in first_clause
        .first()
        .into_iter()
        .flat_map(effective_faces)
        .filter_map(|face| stable_ref(face.as_str()))
        .filter(|face| topology_faces.contains(face))
    {
        target_candidates.insert(face);
    }
    for reference in first_clause.iter().skip(1) {
        let mut candidates = HashSet::new();
        for face in effective_faces(reference)
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .filter(|face| topology_faces.contains(face))
        {
            candidates.insert(face);
        }
        target_candidates.retain(|face| candidates.contains(face));
    }
    let mut construction_faces = Vec::new();
    'bodies: for body in inserted_bodies {
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
                construction_faces.push(face);
            }
        }
    }
    if construction_faces.is_empty() {
        return Ok(None);
    }
    let mut face_loop_positions = |face,
                                   topology|
     -> Result<
        Option<(usize, Vec<cadmpeg_ir::math::Point3>)>,
        cadmpeg_core::CodecError,
    > {
        let contexts = face_boundary_contexts_for_slots(&[face], topology, topology_queries);
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
        let positions = rows.iter().map(|row| row.position).collect::<Vec<_>>();
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
        for face in &construction_faces {
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
            matches.push(candidate);
        }
    }
    matches.sort_unstable_by(Ord::cmp);
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
    operands: &mut [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut complete_bodies = SnapshotCache::default();
    let mut state_indexes = SnapshotCache::default();
    if projection_was_finalized(histories) {
        return Ok(());
    }
    let mut body_closures = HistoricalBodyClosureCache::default();
    for operand in operands.iter_mut() {
        for reference in operand.reference_bindings_mut() {
            reference.preceding_candidate_faces.clear();
            reference.preceding_body_slots.clear();
        }
        operand.resolved_face_slot = None;
        operand.resolved_body_state_id = None;
        operand.resolved_body_slot = None;
        operand.resolved_body_face_slots.clear();
        let Some((history, state, previous)) =
            body_recipe_operand_history_pair(operand, scopes, histories)
        else {
            continue;
        };
        let states = state_indexes.get(history, history_state_index);
        let Some(topology) = previous.topology() else {
            continue;
        };
        if face_changes_across_state_chain(state, previous.state_id, states).is_none() {
            continue;
        }
        let Some(source) = historical_brep_source(&previous.id) else {
            continue;
        };
        for reference in operand.reference_bindings_mut() {
            *reference.preceding_candidate_faces = selection::faces_in_topology(
                reference
                    .candidate_faces
                    .iter()
                    .filter(|face| active_brep_face_matches_source(face, source)),
                topology,
            );
            let mut face_slots = BTreeSet::new();
            for face in reference
                .preceding_candidate_faces
                .iter()
                .filter_map(|face| stable_ref(face.as_str()))
            {
                face_slots.insert(face);
            }
            let Some(body_slots) = body_closures.intersecting(topology, &face_slots) else {
                continue;
            };
            *reference.preceding_body_slots = body_slots.into_iter().collect::<Vec<_>>();
        }
        if let [reference] = operand.references().as_slice() {
            if let [face] = reference.preceding_candidate_faces.as_slice() {
                operand.resolved_face_slot = stable_ref(face.as_str());
            }
        }
        let Some(first) = operand.references().first() else {
            continue;
        };
        if first.preceding_body_slots.is_empty()
            || operand
                .references()
                .iter()
                .any(|reference| reference.preceding_body_slots.is_empty())
        {
            continue;
        }
        let mut intersection = BTreeSet::new();
        for body in &first.preceding_body_slots {
            intersection.insert(*body);
        }
        for reference in &operand.references()[1..] {
            intersection.retain(|body| reference.preceding_body_slots.contains(body));
        }
        if intersection.len() == 1 {
            operand.resolved_body_slot = intersection.into_iter().next();
        }
    }
    let mut recipes_by_id =
        HashMap::<_, Option<&crate::records::recipes::ConstructionRecipe>>::new();
    for recipe in recipes {
        recipes_by_id
            .entry(recipe.id.as_str())
            .and_modify(|recipe| *recipe = None)
            .or_insert(Some(recipe));
    }
    let identity = |operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand| -> Result<Option<_>, cadmpeg_core::CodecError> {
        let Some(stream) = crate::ids::native_stream(&operand.id) else { return Ok(None) };
        let Some(recipe) = recipes_by_id
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| *recipe) else { return Ok(None) };
        let Some(design) = recipe.design.as_ref() else { return Ok(None) };
        let Some(selector) = design.selector else { return Ok(None) };
        Ok(Some((
            stream.to_owned(),
            operand.asset_id.clone(),
            operand.context_id.clone(),
            operand
                .references()
                .iter()
                .map(|reference| (reference.design_reference, reference.form)).collect::<Vec<_>>(),
            design.id.value.clone(),
            selector.value,
        )))
    };
    let mut resolved_by_identity = HashMap::new();
    for operand in operands.iter() {
        let (Some(identity), Some(body)) = (identity(operand)?, operand.resolved_body_slot) else {
            continue;
        };
        resolved_by_identity
            .entry(identity)
            .and_modify(|resolved| {
                if *resolved != Some(body) {
                    *resolved = None;
                }
            })
            .or_insert(Some(body));
    }
    for operand in operands {
        if operand.resolved_body_slot.is_none() {
            operand.resolved_body_slot = identity(operand)?
                .and_then(|identity| resolved_by_identity.get(&identity).copied())
                .flatten();
        }
        let Some(body_slot) = operand.resolved_body_slot else {
            continue;
        };
        let Some((_, _, previous)) = body_recipe_operand_history_pair(operand, scopes, histories)
        else {
            continue;
        };
        let Some(topology) = previous.topology() else {
            continue;
        };
        let Some(faces) = complete_bodies
            .get(topology, CompleteBodyIndex::new)
            .faces(body_slot)
        else {
            continue;
        };
        operand.resolved_body_state_id = Some(previous.state_id);
        operand.resolved_body_face_slots = faces;
    }
    Ok(())
}

fn body_recipe_operand_history_pair<'a>(
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &'a [AsmHistory],
) -> Option<(&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState)> {
    let stream = crate::ids::native_stream(&operand.id)?;
    let mut matching_scopes = scopes.iter().filter(|scope| {
        scope.record_index == operand.scope_record_index
            && crate::ids::native_stream(&scope.id) == Some(stream)
    });
    let scope = matching_scopes.next()?;
    if matching_scopes.next().is_some() {
        return None;
    }
    let state_id = scope.history_state_id()?;
    let previous_state_id = effective_scope_previous_history_state_id(scope, histories)?;
    let (history, state, previous) =
        unique_history_state_pair(histories, state_id, previous_state_id)?;
    Some((history, state, previous))
}

fn active_brep_face_matches_source(face: &cadmpeg_ir::ids::FaceId, source: &str) -> bool {
    face.as_str().starts_with("f3d:brep:entity#") || face_in_brep_source(face.as_str(), source)
}

fn face_in_brep_source(id: &str, source: &str) -> bool {
    id.strip_prefix("f3d:brep/")
        .and_then(|tail| tail.strip_prefix(source))
        .is_some_and(|tail| tail.starts_with('/'))
}

#[derive(Debug, PartialEq)]
enum LegacyFaceResolution {
    Historical(i64),
    Active(cadmpeg_ir::ids::FaceId),
}

fn select_legacy_extrude_face_candidate(
    mut candidates: Vec<cadmpeg_ir::ids::FaceId>,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    history_source: Option<&str>,
) -> Option<LegacyFaceResolution> {
    for changed_only in [true, false] {
        let mut faces = candidates.iter().filter(|face| {
            stable_ref(face.as_str()).is_some_and(|slot| {
                topology.faces.contains(&slot) && (!changed_only || changed_faces.contains(&slot))
            })
        });
        let face = faces.next();
        if faces.next().is_none() {
            let Some(face) = face else {
                continue;
            };
            if let Some(slot) = stable_ref(face.as_str()) {
                return Some(LegacyFaceResolution::Historical(slot));
            }
        }
    }
    if let Some(source) = history_source {
        let unique_index = {
            let mut matches = candidates
                .iter()
                .enumerate()
                .filter(|(_, face)| face_in_brep_source(face.as_str(), source));
            let first = matches.next().map(|(index, _)| index);
            first.filter(|_| matches.next().is_none())
        };
        if let Some(index) = unique_index {
            return Some(LegacyFaceResolution::Active(candidates.swap_remove(index)));
        }
    }
    if candidates.len() == 1 {
        candidates.pop().map(LegacyFaceResolution::Active)
    } else {
        None
    }
}

/// One body/native selection row, minting the non-blank native member at the
/// boundary where the F3D record states it.
///
/// `None` when the record states a blank native member: a row the IR carrier
/// does not hold.
fn body_member<B>(body: B, native: String) -> Option<cadmpeg_ir::features::BodyMember<B>> {
    Some(cadmpeg_ir::features::BodyMember::new(
        body,
        cadmpeg_core::text::NonBlankString::new(native)?,
    ))
}

fn historical_brep_source(state_id: &str) -> Option<&str> {
    state_id
        .rsplit_once("/BREP.")
        .or_else(|| state_id.rsplit_once("BREP."))?
        .1
        .split_once(":asm-")
        .map(|(source, _)| source)
}

fn resolve_direct_face_recipe_clauses(
    references: &[crate::records::dimensions::DesignRecipeReference],
    topology: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Vec<i64> {
    let mut clauses = Vec::<(
        u64,
        u64,
        Vec<&crate::records::dimensions::DesignRecipeReference>,
    )>::new();
    for reference in references {
        let key = (reference.selector_offset, reference.token_offset);
        if let Some((_, _, references)) = clauses
            .iter_mut()
            .find(|(selector, token, _)| (*selector, *token) == key)
        {
            references.push(reference);
        } else {
            let grouped = vec![reference];
            clauses.push((key.0, key.1, grouped));
        }
    }
    let mut topology_faces = HashSet::new();
    for face in &topology.faces {
        topology_faces.insert(*face);
    }
    let mut resolved = Vec::new();
    for (_, _, references) in clauses {
        let mut intersection = None::<HashSet<i64>>;
        for reference in references {
            let candidates = if reference.candidate_faces.is_empty() {
                &reference.alternate_selector_faces
            } else {
                &reference.candidate_faces
            };
            let mut eligible = HashSet::new();
            for face in candidates
                .iter()
                .filter_map(|face| stable_ref(face.as_str()))
                .filter(|face| topology_faces.contains(face) && changed_faces.contains(face))
            {
                eligible.insert(face);
            }
            let candidates = eligible;
            if candidates.is_empty() {
                return Vec::new();
            }
            intersection = Some(match intersection {
                None => candidates,
                Some(mut intersection) => {
                    intersection.retain(|face| candidates.contains(face));
                    intersection
                }
            });
        }
        let Some(intersection) = intersection else {
            return Vec::new();
        };
        let mut candidates = intersection.into_iter();
        let Some(face) = candidates.next() else {
            return Vec::new();
        };
        if candidates.next().is_some() {
            return Vec::new();
        }
        if !resolved.contains(&face) {
            resolved.push(face);
        }
    }
    resolved
}

fn bind_profile_face_group_cardinality(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::face::DesignFaceOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    operand_groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut state_indexes = SnapshotCache::default();
    for scope in scopes {
        let scoped_history = if scope_histories.contains_key(&scope.id) {
            let Some(history) = bound_scope_history(&scope.id, scope_histories, histories) else {
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
                effective_scope_previous_history_state_id(scope, scoped_histories),
            ) else {
                continue;
            };
            let history_pair = if scoped_history.is_some() {
                bound_history_state_pair(
                    &scope.id,
                    state_id,
                    previous_state_id,
                    scope_histories,
                    histories,
                )
            } else {
                unique_history_state_pair(histories, state_id, previous_state_id)
            };
            let Some((history, state, previous)) = history_pair else {
                continue;
            };
            let states = state_indexes.get(history, history_state_index);
            let changed_faces = face_changes_across_state_chain(state, previous_state_id, states);
            let (Some(topology), Some(changed_faces)) = (previous.topology(), changed_faces) else {
                continue;
            };
            let paired_aggregate = crate::design::face_resolve::is_paired_extrude_profile_aggregate(
                group,
                operand_groups,
                operands,
            );
            let faces =
                if paired_aggregate {
                    if let Some(transition) = state.transition.as_ref().filter(|transition| {
                        transition.previous_state_id == Some(previous_state_id)
                    }) {
                        let mut preceding_faces = HashSet::new();
                        for face in &topology.faces {
                            preceding_faces.insert(*face);
                        }
                        let mut deleted = transition.topology.faces.deleted.clone();
                        deleted.sort_unstable_by(Ord::cmp);
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
                    profile_face_group_cardinality_candidates(
                        topology,
                        &changed_faces,
                        group.members().len(),
                    )
                };
            let Some(faces) = faces else {
                continue;
            };
            for (index, face) in indices.into_iter().zip(faces) {
                let face_id = historical_face_id(face)?;
                let preceding_id = face_id.clone();
                operands[index].preceding_candidate_faces =
                    std::iter::once(preceding_id).collect::<Vec<_>>();
                operands[index].changed_candidate_faces =
                    std::iter::once(face_id).collect::<Vec<_>>();
                operands[index].resolved_face_slots = vec![face];
            }
        }
    }
    Ok(())
}

fn profile_face_group_cardinality_candidates(
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    member_count: usize,
) -> Option<Vec<i64>> {
    let mut preceding_faces = HashSet::new();
    for face in &topology.faces {
        preceding_faces.insert(*face);
    }
    let mut faces_by_carrier = HashMap::<i64, Vec<i64>>::new();
    for face in changed_faces
        .iter()
        .copied()
        .filter(|face| preceding_faces.contains(face))
    {
        let mut bindings = topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == face);
        let Some(carrier) = bindings.next().map(|binding| binding.carrier) else {
            continue;
        };
        if bindings.next().is_none() {
            faces_by_carrier.entry(carrier).or_default().push(face);
        }
    }
    let mut candidates = faces_by_carrier
        .into_values()
        .filter(|faces| faces.len() == member_count);
    let mut faces = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    faces.sort_unstable_by(Ord::cmp);
    faces.dedup();
    (faces.len() == member_count).then_some(faces)
}

fn face_changes_across_state_chain<'a>(
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Option<HashSet<i64>> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut changed = HashSet::new();
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        let transition = current.transition.as_ref()?;
        for face in transition
            .topology
            .faces
            .deleted
            .iter()
            .chain(&transition.topology.faces.updated)
        {
            changed.insert(*face);
        }
        let previous = transition
            .previous_state_id
            .and_then(|id| states.get(&id).copied().flatten())?;
        current = previous;
    }
    Some(changed)
}

#[derive(Debug, PartialEq)]
struct EdgeChanges {
    deleted: HashSet<i64>,
    updated: HashSet<i64>,
}

fn edge_changes_across_state_chain<'a>(
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Option<EdgeChanges> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut deleted = HashSet::new();
    let mut updated = HashSet::new();
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        let transition = current.transition.as_ref()?;
        for edge in &transition.topology.edges.deleted {
            deleted.insert(*edge);
        }
        for edge in &transition.topology.edges.updated {
            updated.insert(*edge);
        }
        let previous = transition
            .previous_state_id
            .and_then(|id| states.get(&id).copied().flatten())?;
        current = previous;
    }
    Some(EdgeChanges { deleted, updated })
}

fn historical_face_support_contexts<'a>(
    candidates: &[cadmpeg_ir::ids::FaceId],
    history: &'a AsmHistory,
    preceding_topology: &'a AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Vec<crate::records::topology::historical_context::DesignHistoricalFaceSupportContext> {
    let mut preceding_faces = HashSet::new();
    for face in &preceding_topology.faces {
        preceding_faces.insert(*face);
    }
    let mut contexts = Vec::new();
    'candidates: for candidate in candidates {
        let Some(active_face_slot) = stable_ref(candidate.as_str()) else {
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
            let mut carriers = HashSet::new();
            for topology in history.states.iter().filter_map(|state| state.topology()) {
                let mut bindings = topology
                    .face_surfaces
                    .iter()
                    .filter(|binding| binding.entity == active_face_slot);
                if let Some(binding) = bindings.next() {
                    if bindings.next().is_some() {
                        continue 'candidates;
                    }
                    carriers.insert(binding.carrier);
                }
            }
            if carriers.len() != 1 {
                continue;
            }
            let Some(carrier) = carriers.into_iter().next() else {
                continue;
            };
            carrier
        };
        let mut preceding_face_slots = preceding_topology
            .face_surfaces
            .iter()
            .filter(|binding| {
                binding.carrier == surface_slot && preceding_faces.contains(&binding.entity)
            })
            .map(|binding| binding.entity)
            .collect::<Vec<_>>();
        preceding_face_slots.sort_unstable_by(Ord::cmp);
        preceding_face_slots.dedup();
        if preceding_face_slots.is_empty() {
            continue;
        }
        let changed_preceding_face_slots = preceding_face_slots
            .iter()
            .copied()
            .filter(|face| changed_faces.contains(face))
            .collect::<Vec<_>>();
        let preceding_face_boundaries = face_boundary_contexts_for_slots(
            &preceding_face_slots,
            preceding_topology,
            topology_queries,
        );

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
    contexts
}

fn face_boundary_edges<'a>(
    faces: &[cadmpeg_ir::ids::FaceId],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Vec<i64> {
    let index = topology_queries
        .boundaries
        .get(topology, topology_cache::aggregate_boundaries);
    let mut selected = HashSet::new();
    for face in faces.iter().filter_map(|face| stable_ref(face.as_str())) {
        for edge in index.get(&face).into_iter().flatten() {
            selected.insert(*edge);
        }
    }
    let mut edges = selected.into_iter().collect::<Vec<_>>();
    edges.sort_unstable_by(Ord::cmp);
    edges.dedup();
    edges
}

fn collect_reference_edge_sets<'a>(
    reference_faces: &[Vec<cadmpeg_ir::ids::FaceId>],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Vec<Vec<i64>> {
    let mut sets = Vec::new();
    for faces in reference_faces {
        let faces = selection::faces_in_topology(faces, topology);
        let edges = face_boundary_edges(&faces, topology, topology_queries);

        sets.push(edges);
    }
    sets
}

fn face_boundary_contexts_for_slots<'a>(
    face_slots: &[i64],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Vec<crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext> {
    let index = topology_queries.loops.get(topology, LoopIndex::new);
    let edge_endpoints = topology_queries
        .endpoints
        .get(topology, topology_cache::endpoint_index);
    let mut contexts = Vec::new();
    'faces: for face_slot in face_slots {
        let Some(face_loops) = index.face_loops.get(face_slot).copied().flatten() else {
            continue;
        };
        let mut loops = Vec::new();
        for loop_slot in face_loops {
            let Some(loop_coedges) = index.loop_coedges.get(loop_slot).copied().flatten() else {
                continue 'faces;
            };
            let mut coedges = Vec::new();
            for coedge_slot in loop_coedges {
                let Some(edge_slot) = index.coedge_edges.get(coedge_slot).copied().flatten() else {
                    continue 'faces;
                };

                coedges.push(
                    crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                        coedge_slot: *coedge_slot,
                        edge_slot,
                    },
                );
            }
            let boundary = historical_loop_boundary(coedges, index, edge_endpoints);

            loops.push(
                crate::records::topology::historical_context::DesignHistoricalFaceLoopContext {
                    loop_slot: *loop_slot,
                    boundary,
                },
            );
        }

        contexts.push(
            crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext {
                face_slot: *face_slot,
                loops,
            },
        );
    }
    contexts
}

fn historical_loop_boundary(
    coedges: Vec<crate::records::topology::historical_context::DesignHistoricalLoopCoedge>,
    index: &LoopIndex<'_>,
    edge_endpoints: &topology_cache::EdgeEndpoints,
) -> crate::records::topology::historical_context::DesignHistoricalLoopBoundary {
    use crate::records::topology::{
        historical_context::DesignHistoricalLoopBoundary,
        historical_context::DesignHistoricalLoopPoint,
        historical_context::DesignHistoricalLoopPosition,
        historical_context::DesignHistoricalLoopVertex,
    };
    let mut vertices = Vec::new();
    for (ordinal, coedge) in coedges.iter().enumerate() {
        let previous = coedges[(ordinal + coedges.len() - 1) % coedges.len()].edge_slot;

        let endpoints = |slot| edge_endpoints.get(&slot).copied().flatten();
        let (Some(previous), Some(current)) = (endpoints(previous), endpoints(coedge.edge_slot))
        else {
            return DesignHistoricalLoopBoundary::Coedges(coedges);
        };
        let mut shared = previous
            .into_iter()
            .filter(|vertex| current.contains(vertex));
        let Some(vertex_slot) = shared.next() else {
            return DesignHistoricalLoopBoundary::Coedges(coedges);
        };
        if shared.any(|vertex| vertex != vertex_slot) {
            return DesignHistoricalLoopBoundary::Coedges(coedges);
        }

        vertices.push(DesignHistoricalLoopVertex {
            coedge: coedge.clone(),
            vertex_slot,
        });
    }
    if vertices.is_empty() {
        return DesignHistoricalLoopBoundary::Coedges(coedges);
    }
    let mut points = Vec::new();
    for vertex in &vertices {
        let Some(point_slot) = index
            .vertex_points
            .get(&vertex.vertex_slot)
            .copied()
            .flatten()
        else {
            return DesignHistoricalLoopBoundary::Vertices(vertices);
        };

        points.push(DesignHistoricalLoopPoint {
            vertex: vertex.clone(),
            point_slot,
        });
    }
    let mut positions = Vec::new();
    for point in &points {
        let Some(position) = index.positions.get(&point.point_slot).copied().flatten() else {
            return DesignHistoricalLoopBoundary::Points(points);
        };

        positions.push(DesignHistoricalLoopPosition {
            point: point.clone(),
            position,
        });
    }
    DesignHistoricalLoopBoundary::Positions(positions)
}

fn preceding_support_face_slots(
    result_faces: &[cadmpeg_ir::ids::FaceId],
    result_topology: &AsmHistoricalTopology,
    preceding_topology: &AsmHistoricalTopology,
) -> Vec<i64> {
    let mut preceding_faces = HashSet::new();
    for face in &preceding_topology.faces {
        preceding_faces.insert(*face);
    }
    let mut support_faces = Vec::new();
    for result_face in result_faces {
        let Some(result_face) = stable_ref(result_face.as_str()) else {
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
            support_faces.push(preceding_face);
        }
    }
    support_faces
}

#[derive(Clone, Copy)]
struct EdgeBoundaryContext<'a, 'b> {
    state: &'a AsmDeltaState,
    topology: &'a AsmHistoricalTopology,
    boundary_edges: &'b [i64],
}

/// Face boundaries named by edge-recipe reference contexts, stored once per
/// historical state and face slot.
#[derive(Default)]
pub(crate) struct FaceBoundaryArena {
    records: BTreeMap<String, crate::records::topology::historical_context::AsmFaceBoundary>,
    /// Identities of faces whose loop topology is incomplete in their state.
    incomplete: HashSet<String>,
}

impl FaceBoundaryArena {
    /// Return the boundary identities of the complete `face_slots` in the
    /// state, in slot order, and store each boundary not yet present.
    fn intern<'a>(
        &mut self,
        state: &AsmDeltaState,
        topology: &'a AsmHistoricalTopology,
        face_slots: &[i64],
        topology_queries: &mut TopologyQueryCache<'a>,
    ) -> Vec<String> {
        let mut ids = Vec::new();
        for face_slot in face_slots {
            let Some(id) = crate::records::topology::historical_context::AsmFaceBoundary::identity(
                &state.id,
                state.byte_offset,
                *face_slot,
            ) else {
                continue;
            };
            if self.incomplete.contains(&id) {
                continue;
            }
            if !self.records.contains_key(&id) {
                let Some(boundary) = face_boundary_contexts_for_slots(
                    std::slice::from_ref(face_slot),
                    topology,
                    topology_queries,
                )
                .pop() else {
                    self.incomplete.insert(id);
                    continue;
                };
                self.records.insert(
                    id.clone(),
                    crate::records::topology::historical_context::AsmFaceBoundary {
                        id: id.clone(),
                        parent: state.id.clone(),
                        face_slot: boundary.face_slot,
                        loops: boundary.loops,
                    },
                );
            }
            ids.push(id);
        }
        ids
    }

    /// The stored boundaries in identity order.
    pub(crate) fn into_records(
        self,
    ) -> Vec<crate::records::topology::historical_context::AsmFaceBoundary> {
        self.records.into_values().collect()
    }
}

fn edge_recipe_reference_context<'a>(
    reference_ordinal: u32,
    reference: &crate::records::dimensions::DesignRecipeReference,
    result: EdgeBoundaryContext<'a, '_>,
    preceding: EdgeBoundaryContext<'a, '_>,
    changed_edges: &HashSet<i64>,
    topology_queries: &mut TopologyQueryCache<'a>,
    face_boundaries: &mut FaceBoundaryArena,
) -> crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
    let candidate_faces = if reference.candidate_faces.is_empty() {
        reference.alternate_selector_faces.as_slice()
    } else {
        reference.candidate_faces.as_slice()
    };
    let face_slots = |faces: &[cadmpeg_ir::ids::FaceId]| {
        faces
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .collect::<Vec<_>>()
    };
    let result_faces = selection::faces_in_topology(candidate_faces, result.topology);
    let result_face_boundary_ids = face_boundaries.intern(
        result.state,
        result.topology,
        &face_slots(&result_faces),
        topology_queries,
    );
    let result_edges = face_boundary_edges(&result_faces, result.topology, topology_queries);
    let result_shared_edge_slots = result
        .boundary_edges
        .iter()
        .copied()
        .filter(|edge| result_edges.contains(edge))
        .collect::<Vec<_>>();
    let preceding_faces = selection::faces_in_topology(candidate_faces, preceding.topology);
    let preceding_face_boundary_ids = face_boundaries.intern(
        preceding.state,
        preceding.topology,
        &face_slots(&preceding_faces),
        topology_queries,
    );
    let preceding_support_face_slots =
        preceding_support_face_slots(&result_faces, result.topology, preceding.topology);
    let preceding_support_face_boundary_ids = face_boundaries.intern(
        preceding.state,
        preceding.topology,
        &preceding_support_face_slots,
        topology_queries,
    );
    let preceding_edges =
        face_boundary_edges(&preceding_faces, preceding.topology, topology_queries);
    let shared_edge_slots = preceding
        .boundary_edges
        .iter()
        .copied()
        .filter(|edge| preceding_edges.contains(edge))
        .collect::<Vec<_>>();
    let changed_shared_edge_slots = shared_edge_slots
        .iter()
        .copied()
        .filter(|edge| changed_edges.contains(edge))
        .collect::<Vec<_>>();
    let mut support_edges = HashSet::new();
    for edge in preceding_support_face_boundary_ids
        .iter()
        .filter_map(|id| face_boundaries.records.get(id))
        .flat_map(|face| &face.loops)
        .flat_map(|face_loop| face_loop.boundary.coedges().map(|row| row.edge_slot))
    {
        support_edges.insert(edge);
    }
    let mut changed_reference_edge_slots = preceding_edges
        .iter()
        .copied()
        .chain(support_edges.iter().copied())
        .filter(|edge| changed_edges.contains(edge))
        .collect::<Vec<_>>();
    changed_reference_edge_slots.sort_unstable_by(Ord::cmp);
    changed_reference_edge_slots.dedup();
    crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
        reference_ordinal,
        result_faces,
        result_face_boundary_ids,
        result_shared_edge_slots,
        preceding_faces,
        preceding_face_boundary_ids,
        preceding_support_face_slots,
        preceding_support_face_boundary_ids,
        shared_edge_slots,
        changed_shared_edge_slots,
        changed_reference_edge_slots,
    }
}

/// Resolve the unique candidate edge shared by the non-null face references
/// in the first side of a standard edge recipe.
fn side_one_recipe_edge(
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    reference_contexts: &[crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext],
    selectors: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    candidate_edges: &[i64],
) -> Option<i64> {
    let side = structure.and_then(|structure| structure.sides.first())?;
    let mut ordinals = Vec::new();
    for value in std::iter::once(side.header_value)
        .chain(side.scalars.iter().copied())
        .filter(|value| *value != 0)
    {
        let ordinal = usize::try_from(value)
            .ok()
            .and_then(|value| value.checked_sub(1))?;

        ordinals.push(ordinal);
    }
    ordinals.sort_unstable_by(Ord::cmp);
    ordinals.dedup();
    let mut edge_sets = Vec::new();
    for ordinal in ordinals {
        let context = reference_contexts.get(ordinal)?;
        if Some(context.reference_ordinal) != u32::try_from(ordinal).ok() {
            return None;
        }

        edge_sets.push(context.shared_edge_slots.as_slice());
    }
    if edge_sets.iter().any(|edges| edges.is_empty()) {
        return None;
    }
    let first = edge_sets.first()?;
    let mut candidates = first.to_vec();
    for edges in edge_sets.iter().skip(1) {
        candidates.retain(|candidate| edges.contains(candidate));
    }
    candidates.retain(|candidate| candidate_edges.contains(candidate));
    candidates.sort_unstable_by(Ord::cmp);
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Some(*edge),
        _ => {
            crate::design::edge_resolve::resolved_edge_candidate_intersection(selectors, edge_sets)
        }
    }
}

pub(crate) fn bind_edge_operand_history_candidates(
    operands: &mut [crate::records::topology::edge_identity::DesignEdgeOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<
    Vec<crate::records::topology::historical_context::AsmFaceBoundary>,
    cadmpeg_core::CodecError,
> {
    let mut treatment_cache = TreatmentCache::default();
    let mut topology_queries = TopologyQueryCache::default();
    let mut state_indexes = SnapshotCache::default();
    let mut recipe_topologies = RecipeTopologyCache::default();
    let mut face_boundaries = FaceBoundaryArena::default();
    if projection_was_finalized(histories) {
        return Ok(Vec::new());
    }
    let mut recipe_record_indices = HashMap::new();
    for recipe in recipes {
        if let Some(index) = recipe.record_index {
            recipe_record_indices.insert(recipe.id.as_str(), index.value);
        }
    }
    let mut terminal_topologies = Vec::new();
    for history in histories {
        let mut preceding = HashSet::new();
        for state in &history.states {
            if let Some(previous) = state
                .transition
                .as_ref()
                .and_then(|transition| transition.previous_state_id)
            {
                preceding.insert(previous);
            }
        }
        let mut terminals = history
            .states
            .iter()
            .filter(|state| !preceding.contains(&state.state_id));
        if let Some(state) = terminals.next().filter(|_| terminals.next().is_none()) {
            if let Some(topology) = state.topology() {
                terminal_topologies.push((state.state_id, topology));
            }
        }
    }
    for operand in operands {
        operand.result_candidate_faces.clear();
        operand.result_boundary_edge_slots.clear();
        operand.preceding_candidate_faces.clear();
        operand.terminal_candidate_faces.clear();
        operand.changed_candidate_faces.clear();
        operand.preceding_boundary_edge_slots.clear();
        operand.terminal_boundary_edge_slots.clear();
        operand.changed_boundary_edge_slots.clear();
        operand.deleted_boundary_edge_slots.clear();
        operand.updated_boundary_edge_slots.clear();
        operand.treatment_radius_candidates.clear();
        operand.changed_boundary_edge_contexts.clear();
        operand.terminal_boundary_edge_contexts.clear();
        operand.recipe_reference_contexts.clear();
        operand.recipe_selectors.clear();
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
            bind_active_edge_operand_for_scope(
                operand,
                scope,
                &terminal_topologies,
                &mut topology_queries,
            )?;
            continue;
        };
        let Some(previous_state_id) = effective_scope_previous_history_state_id(scope, histories)
        else {
            bind_active_edge_operand_for_scope(
                operand,
                scope,
                &terminal_topologies,
                &mut topology_queries,
            )?;
            continue;
        };
        let Some((history, state, previous)) = bound_history_state_pair(
            &scope.id,
            state_id,
            previous_state_id,
            scope_histories,
            histories,
        ) else {
            continue;
        };
        let (Some(result_topology), Some(topology)) = (state.topology(), previous.topology())
        else {
            continue;
        };
        for reference in &mut operand.recipe_references {
            bind_historical_recipe_reference_candidates(
                reference,
                topology,
                &mut recipe_topologies,
            )?;
        }
        if let Some(recipe_record_index) = recipe_record_indices.get(operand.recipe_id.as_str()) {
            operand.candidate_faces = historical_recipe_faces(
                i64::from(*recipe_record_index),
                topology,
                &mut recipe_topologies,
            )?;
        }
        let states = state_indexes.get(history, history_state_index);
        let Some(changed_faces) = face_changes_across_state_chain(state, previous_state_id, states)
        else {
            continue;
        };
        let Some(EdgeChanges {
            deleted: chain_deleted_edges,
            updated: chain_updated_edges,
        }) = edge_changes_across_state_chain(state, previous_state_id, states)
        else {
            continue;
        };
        let mut preceding_faces = HashSet::new();
        for face in &topology.faces {
            preceding_faces.insert(*face);
        }
        let inserted_faces = result_topology
            .faces
            .iter()
            .copied()
            .filter(|face| !preceding_faces.contains(face))
            .collect::<Vec<_>>();
        let mut result_edges = HashSet::new();
        for edge in &result_topology.edges {
            result_edges.insert(*edge);
        }
        let deleted_edges = topology
            .edges
            .iter()
            .copied()
            .filter(|edge| !result_edges.contains(edge) && chain_deleted_edges.contains(edge))
            .collect::<Vec<_>>();
        let updated_edges = topology
            .edges
            .iter()
            .copied()
            .filter(|edge| {
                result_edges.contains(edge)
                    && (chain_deleted_edges.contains(edge) || chain_updated_edges.contains(edge))
            })
            .collect::<Vec<_>>();
        operand.recipe_state_id = Some(previous_state_id);
        operand.result_candidate_faces =
            selection::faces_in_topology(&operand.candidate_faces, result_topology);
        operand.result_boundary_edge_slots = face_boundary_edges(
            &operand.result_candidate_faces,
            result_topology,
            &mut topology_queries,
        );
        operand.preceding_candidate_faces =
            selection::faces_in_topology(&operand.candidate_faces, topology);
        operand.changed_candidate_faces =
            (operand.preceding_candidate_faces.iter().filter(|face| {
                stable_ref(face.as_str()).is_some_and(|slot| changed_faces.contains(&slot))
            }))
            .cloned()
            .collect::<Vec<_>>();
        operand.preceding_boundary_edge_slots = face_boundary_edges(
            &operand.preceding_candidate_faces,
            topology,
            &mut topology_queries,
        );
        let mut changed_edges = HashSet::new();
        for edge in deleted_edges.iter().chain(&updated_edges) {
            changed_edges.insert(*edge);
        }
        operand.changed_boundary_edge_slots = operand
            .preceding_boundary_edge_slots
            .iter()
            .copied()
            .filter(|edge| changed_edges.contains(edge))
            .collect::<Vec<_>>();
        operand.deleted_boundary_edge_slots = selection::boundary_edges_in_changes(
            &operand.preceding_boundary_edge_slots,
            &deleted_edges,
        );
        operand.updated_boundary_edge_slots = selection::boundary_edges_in_changes(
            &operand.preceding_boundary_edge_slots,
            &updated_edges,
        );
        operand.treatment_radius_candidates = treatment_radius_candidates(
            Some(&operand.result_candidate_faces),
            &inserted_faces,
            result_topology,
            topology,
            &deleted_edges,
            &mut treatment_cache,
        );
        operand.changed_boundary_edge_contexts = operand
            .changed_boundary_edge_slots
            .iter()
            .copied()
            .map(|edge| selection::historical_edge_context(edge, topology))
            .collect::<Vec<_>>();
        let mut reference_contexts = Vec::new();
        for (ordinal, reference) in operand.recipe_references.iter().enumerate() {
            let Ok(reference_ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let context = edge_recipe_reference_context(
                reference_ordinal,
                reference,
                EdgeBoundaryContext {
                    state,
                    topology: result_topology,
                    boundary_edges: &operand.result_boundary_edge_slots,
                },
                EdgeBoundaryContext {
                    state: previous,
                    topology,
                    boundary_edges: &operand.preceding_boundary_edge_slots,
                },
                &changed_edges,
                &mut topology_queries,
                &mut face_boundaries,
            );

            reference_contexts.push(context);
        }
        operand.recipe_reference_contexts = reference_contexts;
        if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
            && operand.surface_patch_recipe_structure.is_some()
        {
            operand.resolved_edge_slot = surface_patch_edge_operand_slot(
                operand.surface_patch_recipe_structure.as_ref(),
                &operand.recipe_references,
                topology,
                &mut topology_queries,
            );
            continue;
        }
        if crate::design::design_feature_family(&scope.kind())
            == Some(crate::design::DesignFeatureFamily::Sweep)
        {
            let reference_faces = terminal_edge_recipe_reference_faces(
                &operand.recipe_references,
                operand.local_topology_references.as_deref(),
            )?;
            let reference_edge_sets =
                collect_reference_edge_sets(&reference_faces, topology, &mut topology_queries);
            let mut candidate_edges = BTreeSet::new();
            for edge in reference_edge_sets.iter().flatten() {
                candidate_edges.insert(*edge);
            }
            let contexts = candidate_edges
                .into_iter()
                .map(|edge| selection::historical_edge_context(edge, topology))
                .collect::<Vec<_>>();
            operand.recipe_selectors =
                selection::recipe_selector_candidates(operand.recipe_structure.as_ref(), &contexts);
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
                &operand.recipe_references,
                operand.local_topology_references.as_deref(),
            )?;
            let reference_edge_sets =
                collect_reference_edge_sets(&reference_faces, topology, &mut topology_queries);
            let mut candidate_edges = BTreeSet::new();
            for edge in reference_edge_sets.iter().flatten() {
                candidate_edges.insert(*edge);
            }
            let contexts = candidate_edges
                .into_iter()
                .map(|edge| selection::historical_edge_context(edge, topology))
                .collect::<Vec<_>>();
            operand.recipe_selectors =
                selection::recipe_selector_candidates(operand.recipe_structure.as_ref(), &contexts);
            operand.resolved_edge_slot =
                crate::design::edge_resolve::resolved_edge_candidate_intersection(
                    &operand.recipe_selectors,
                    reference_edge_sets.iter().map(Vec::as_slice),
                );
            let resolved_axis = if let Some(edge) = operand.resolved_edge_slot {
                historical_edge_axis(edge, topology)
                    .and_then(|(origin, direction)| design_axis(origin, direction))
            } else {
                None
            };
            if let Some(axis) = resolved_axis {
                operand.resolved_axis = Some(axis);
            }
            continue;
        }
        let changed_edge_contexts = topology
            .edges
            .iter()
            .copied()
            .filter(|edge| changed_edges.contains(edge))
            .map(|edge| selection::historical_edge_context(edge, topology))
            .collect::<Vec<_>>();
        operand.recipe_selectors = selection::recipe_selector_candidates(
            operand.recipe_structure.as_ref(),
            &changed_edge_contexts,
        );
        operand.resolved_edge_slot = side_one_recipe_edge(
            operand.recipe_structure.as_ref(),
            &operand.recipe_reference_contexts,
            &operand.recipe_selectors,
            &operand.preceding_boundary_edge_slots,
        );
    }
    Ok(face_boundaries.into_records())
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
    edge: i64,
    topology: &AsmHistoricalTopology,
) -> Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    if let Some(axis) = topology
        .edge_curves
        .iter()
        .find(|binding| binding.entity == edge)
        .and_then(|binding| binding.carrier)
        .and_then(|curve| topology.curve_axes.iter().find(|axis| axis.curve == curve))
    {
        return Some((axis.origin, axis.direction));
    }
    let mut support_surfaces = HashSet::new();
    for carrier in selection::historical_edge_context(edge, topology)
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
        support_surfaces.insert(carrier);
    }
    let mut axes = topology
        .surface_axes
        .iter()
        .filter(|axis| support_surfaces.contains(&axis.surface))
        .map(|axis| (axis.origin, axis.direction));
    let first = axes.next()?;
    axes.all(|axis| same_axis_line(first, axis))
        .then_some(first)
}

fn bind_active_edge_operand_for_scope<'a>(
    operand: &mut crate::records::topology::edge_identity::DesignEdgeOperand,
    scope: &crate::records::feature::scope::DesignParameterScope,
    terminal_topologies: &[(i64, &'a AsmHistoricalTopology)],
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    bind_active_edge_operand_candidates(operand, terminal_topologies, topology_queries)?;
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
        && operand.surface_patch_recipe_structure.is_some()
    {
        operand.recipe_state_id = None;
        operand.resolved_edge_slot = None;
        let mut matched = None;
        let mut ambiguous = false;
        for (state_id, topology) in terminal_topologies {
            if let Some(edge) = surface_patch_edge_operand_slot(
                operand.surface_patch_recipe_structure.as_ref(),
                &operand.recipe_references,
                topology,
                topology_queries,
            ) {
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
            historical_edge_axis(edge, topology)
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

fn surface_patch_edge_operand_slot<'a>(
    structure: Option<&crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure>,
    recipe_references: &[crate::records::dimensions::DesignRecipeReference],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<i64> {
    let structure = structure?;
    let [first, second] = &structure.clauses;
    let common_edge_reference = common_surface_patch_reference(
        first.edge_reference_ordinals,
        second.edge_reference_ordinals,
    )?;
    let common_face_reference = common_surface_patch_reference(
        first.face_reference_ordinals,
        second.face_reference_ordinals,
    )?;
    let edge_reference = usize::try_from(common_edge_reference)
        .ok()
        .and_then(|ordinal| recipe_references.get(ordinal))?;
    let face_reference = usize::try_from(common_face_reference)
        .ok()
        .and_then(|ordinal| recipe_references.get(ordinal))?;
    if edge_reference.candidate_edges.is_empty() {
        return None;
    }
    let face_candidates = if face_reference.candidate_faces.is_empty() {
        &face_reference.alternate_selector_faces
    } else {
        &face_reference.candidate_faces
    };
    let faces = selection::faces_in_topology(face_candidates, topology);
    let face_boundary_edges = face_boundary_edges(&faces, topology, topology_queries);
    let mut candidates = edge_reference
        .candidate_edges
        .iter()
        .filter_map(|edge| stable_ref(edge.as_str()))
        .filter(|edge| face_boundary_edges.contains(edge))
        .collect::<Vec<_>>();
    candidates.sort_unstable_by(Ord::cmp);
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Some(*edge),
        _ => None,
    }
}

fn common_surface_patch_reference(left: [u32; 2], right: [u32; 2]) -> Option<u32> {
    let mut common = left
        .into_iter()
        .filter(|reference| right.contains(reference));
    let reference = common.next()?;
    common.next().is_none().then_some(reference)
}

fn bind_active_edge_operand_candidates<'a>(
    operand: &mut crate::records::topology::edge_identity::DesignEdgeOperand,
    topologies: &[(i64, &'a AsmHistoricalTopology)],
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut unique = None;
    for (state_id, topology) in topologies {
        let all_reference_faces =
            terminal_edge_recipe_reference_faces(&operand.recipe_references, None)?;
        let reference_faces = terminal_edge_recipe_reference_faces(
            &operand.recipe_references,
            operand.local_topology_references.as_deref(),
        )?;
        let terminal_faces = terminal_edge_recipe_faces(&operand.candidate_faces, &reference_faces);
        let candidate_faces = selection::faces_in_topology(&terminal_faces, topology);
        if topologies.len() != 1 && candidate_faces.is_empty() {
            continue;
        }
        let boundary_edges = face_boundary_edges(&candidate_faces, topology, topology_queries);
        let contexts = boundary_edges
            .iter()
            .copied()
            .map(|edge| selection::historical_edge_context(edge, topology))
            .collect::<Vec<_>>();
        let selectors =
            selection::recipe_selector_candidates(operand.recipe_structure.as_ref(), &contexts);
        let reference_edge_sets =
            collect_reference_edge_sets(&reference_faces, topology, topology_queries);
        let all_reference_edge_sets =
            collect_reference_edge_sets(&all_reference_faces, topology, topology_queries);
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
    primary: &[cadmpeg_ir::ids::FaceId],
    reference_faces: &[Vec<cadmpeg_ir::ids::FaceId>],
) -> Vec<cadmpeg_ir::ids::FaceId> {
    let mut faces = (primary.iter().chain(reference_faces.iter().flatten()))
        .cloned()
        .collect::<Vec<_>>();
    faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    faces.dedup();
    faces
}

fn terminal_edge_recipe_reference_faces(
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
        let faces = (faces).clone();

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
        for reference in references {
            append(reference)?;
        }
    }
    Ok(selected)
}

fn treatment_radius_candidates<'a>(
    result_candidate_faces: Option<&[cadmpeg_ir::ids::FaceId]>,
    inserted_faces: &[i64],
    result: &'a AsmHistoricalTopology,
    preceding: &'a AsmHistoricalTopology,
    deleted_edges: &[i64],
    treatment_cache: &mut TreatmentCache<'a>,
) -> Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate> {
    treatment_edge_candidates(
        result_candidate_faces,
        inserted_faces,
        result,
        preceding,
        deleted_edges,
        treatment_cache,
    )
    .0
}

fn treatment_edge_candidates<'a>(
    result_candidate_faces: Option<&[cadmpeg_ir::ids::FaceId]>,
    inserted_faces: &[i64],
    result: &'a AsmHistoricalTopology,
    preceding: &'a AsmHistoricalTopology,
    deleted_edges: &[i64],
    treatment_cache: &mut TreatmentCache<'a>,
) -> (
    Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate>,
    Vec<i64>,
) {
    let index = treatment_cache.get(result, preceding);
    let result_boundaries = &index.result_boundaries;
    let preceding_boundaries = &index.preceding_boundaries;
    let mut deleted_edge_set = HashSet::new();
    for edge in deleted_edges {
        deleted_edge_set.insert(*edge);
    }
    let mut candidate_edges = HashSet::new();
    for edge in result_candidate_faces
        .into_iter()
        .flatten()
        .filter_map(|face| stable_ref(face.as_str()))
        .filter_map(|face| result_boundaries.get(&face))
        .flatten()
        .copied()
    {
        candidate_edges.insert(edge);
    }
    let mut radii_out = Vec::new();
    let mut transitions_out = Vec::new();
    for inserted in inserted_faces {
        let Some((carrier, supports)) = index.supports.get(inserted) else {
            continue;
        };
        let Some(inserted_boundary) = result_boundaries.get(inserted) else {
            continue;
        };

        let radius = index
            .radii
            .get(carrier)
            .copied()
            .flatten()
            .and_then(cadmpeg_ir::scalar::PositiveReal::new)
            .filter(|_| {
                candidate_edges.is_empty() || !inserted_boundary.is_disjoint(&candidate_edges)
            });
        for (ordinal, left) in supports.iter().enumerate() {
            let Some(left_edges) = preceding_boundaries.get(left) else {
                continue;
            };
            for right in supports.iter().skip(ordinal + 1) {
                let Some(right_edges) = preceding_boundaries.get(right) else {
                    continue;
                };

                for edge in left_edges
                    .intersection(right_edges)
                    .filter(|edge| deleted_edge_set.contains(edge))
                {
                    transitions_out.push(*edge);
                    if let Some(radius) = radius {
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
    radii_out.sort_by(|left, right| {
        left.radius
            .get()
            .total_cmp(&right.radius.get())
            .then(left.edge_slot.cmp(&right.edge_slot))
    });
    radii_out
        .dedup_by(|left, right| left.radius == right.radius && left.edge_slot == right.edge_slot);
    transitions_out.sort_unstable_by(Ord::cmp);
    transitions_out.dedup();
    (radii_out, transitions_out)
}

fn treatment_face_supports(
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    result_boundaries: &HashMap<i64, HashSet<i64>>,
) -> Vec<(i64, i64, Vec<i64>)> {
    let mut preceding_faces = HashSet::new();
    for face in &preceding.faces {
        preceding_faces.insert(*face);
    }
    let mut preceding_surfaces = HashSet::new();
    for surface in &preceding.surfaces {
        preceding_surfaces.insert(*surface);
    }
    let result_carriers = unique_face_carriers(result);
    let preceding_carrier_faces = unique_carrier_faces(preceding, &preceding_faces);
    let mut adjacent_faces = HashMap::<i64, Vec<i64>>::new();
    for (face, edges) in result_boundaries {
        for edge in edges {
            adjacent_faces.entry(*edge).or_default().push(*face);
        }
    }
    let mut selected = Vec::new();
    for inserted in inserted_faces.iter().copied() {
        let Some(carrier) = result_carriers.get(&inserted).copied().flatten() else {
            continue;
        };
        if preceding_surfaces.contains(&carrier) {
            continue;
        }
        let Some(inserted_boundary) = result_boundaries.get(&inserted) else {
            continue;
        };
        let mut supports = inserted_boundary
            .iter()
            .filter_map(|edge| adjacent_faces.get(edge))
            .flatten()
            .copied()
            .filter(|face| *face != inserted)
            .filter_map(|face| result_carriers.get(&face).copied().flatten())
            .filter_map(|carrier| preceding_carrier_faces.get(&carrier).copied().flatten())
            .collect::<Vec<_>>();
        supports.sort_unstable_by(Ord::cmp);
        supports.dedup();

        selected.push((inserted, carrier, supports));
    }
    selected
}

fn face_boundary_edge_index(topology: &AsmHistoricalTopology) -> HashMap<i64, HashSet<i64>> {
    fn unique_relations(relations: &[AsmHistoricalRelation]) -> HashMap<i64, Option<&[i64]>> {
        let mut out = HashMap::<i64, Option<&[i64]>>::new();
        for relation in relations {
            out.entry(relation.owner_ref)
                .and_modify(|members| *members = None)
                .or_insert(Some(&relation.member_refs));
        }
        out
    }
    let face_loops = unique_relations(&topology.face_loops);
    let loop_coedges = unique_relations(&topology.loop_coedges);
    let mut coedge_edges = HashMap::<i64, Option<i64>>::new();
    for coedge in &topology.coedge_topology {
        coedge_edges
            .entry(coedge.coedge)
            .and_modify(|edge| *edge = None)
            .or_insert(Some(coedge.edge));
    }
    let mut boundaries = HashMap::new();
    'faces: for face in &topology.faces {
        let Some(loops) = face_loops.get(face).copied().flatten() else {
            continue;
        };
        let mut edges = HashSet::new();
        for loop_slot in loops {
            let Some(coedges) = loop_coedges.get(loop_slot).copied().flatten() else {
                continue 'faces;
            };
            for coedge in coedges {
                let Some(edge) = coedge_edges.get(coedge).copied().flatten() else {
                    continue 'faces;
                };
                edges.insert(edge);
            }
        }
        boundaries.insert(*face, edges);
    }
    boundaries
}

fn unique_face_carriers(topology: &AsmHistoricalTopology) -> HashMap<i64, Option<i64>> {
    let mut out = HashMap::new();
    for binding in &topology.face_surfaces {
        out.entry(binding.entity)
            .and_modify(|carrier| *carrier = None)
            .or_insert(Some(binding.carrier));
    }
    out
}

fn unique_carrier_faces(
    topology: &AsmHistoricalTopology,
    included_faces: &HashSet<i64>,
) -> HashMap<i64, Option<i64>> {
    let mut out = HashMap::new();
    for binding in &topology.face_surfaces {
        if !included_faces.contains(&binding.entity) {
            continue;
        }
        out.entry(binding.carrier)
            .and_modify(|face| *face = None)
            .or_insert(Some(binding.entity));
    }
    out
}

pub(crate) fn stable_ref(id: &str) -> Option<i64> {
    id.rsplit_once('#')?
        .1
        .split(':')
        .next()?
        .parse::<i64>()
        .ok()
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

fn affected_body_refs<'a>(
    current: &'a AsmDeltaState,
    previous: Option<&'a AsmDeltaState>,
    body_closures: &mut HistoricalBodyClosureCache<'a>,
) -> Option<Vec<i64>> {
    let transition = current.transition.as_ref()?;
    if transition.previous_state_id != previous.map(|state| state.state_id) {
        return None;
    }
    let current_topology = current.topology()?;
    let current_changes = changed_family_refs(&transition.topology, false);
    let mut affected = body_closures.intersecting(current_topology, &current_changes)?;
    if let Some(previous) = previous {
        let previous_topology = previous.topology()?;
        let deleted = changed_family_refs(&transition.topology, true);
        let previous_affected = body_closures.intersecting(previous_topology, &deleted)?;
        for body in previous_affected {
            affected.insert(body);
        }
    }
    Some(affected.into_iter().collect::<Vec<_>>())
}

fn changed_family_refs(delta: &AsmHistoricalTopologyDelta, deleted: bool) -> BTreeSet<i64> {
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
        let members = if deleted {
            family.deleted.iter().chain([].iter())
        } else {
            family.inserted.iter().chain(family.updated.iter())
        };
        for &member in members {
            changed.insert(member);
        }
    }
    changed
}

type HistoricalBodyClosures = Vec<(i64, HashSet<i64>)>;

/// Reuse body incidence for queries over immutable historical snapshots.
#[derive(Default)]
struct HistoricalBodyClosureCache<'a> {
    entries: HashMap<*const AsmHistoricalTopology, Option<HistoricalBodyClosures>>,
    snapshots: std::marker::PhantomData<&'a AsmHistoricalTopology>,
}

impl<'a> HistoricalBodyClosureCache<'a> {
    fn intersecting(
        &mut self,
        topology: &'a AsmHistoricalTopology,
        changed: &BTreeSet<i64>,
    ) -> Option<BTreeSet<i64>> {
        let key = std::ptr::from_ref(topology);
        self.entries
            .entry(key)
            .or_insert_with(|| historical_body_closures(topology));
        let closures = self.entries.get(&key).and_then(Option::as_ref)?;
        let mut affected = BTreeSet::new();
        for (body, closure) in closures {
            if changed.iter().any(|entity| closure.contains(entity)) {
                affected.insert(*body);
            }
        }
        Some(affected)
    }
}

#[cfg(test)]
fn bodies_intersecting(
    topology: &AsmHistoricalTopology,
    changed: &BTreeSet<i64>,
) -> Option<BTreeSet<i64>> {
    HistoricalBodyClosureCache::default().intersecting(topology, changed)
}

fn historical_body_closures(topology: &AsmHistoricalTopology) -> Option<HistoricalBodyClosures> {
    macro_rules! history_some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return None,
            }
        };
    }
    // The relation, carrier, and per-shell indexes are scratch; only the
    // body closures are cached.
    let body_regions = relation_map(&topology.body_regions);
    let region_shells = relation_map(&topology.region_shells);
    let shell_faces = relation_map(&topology.shell_faces);
    let shell_wire_edges = relation_map(&topology.shell_wire_edges);
    let shell_free_vertices = relation_map(&topology.shell_free_vertices);
    let face_loops = relation_map(&topology.face_loops);
    let loop_coedges = relation_map(&topology.loop_coedges);
    let coedges = topology
        .coedge_topology
        .iter()
        .map(|coedge| (coedge.coedge, coedge))
        .collect::<HashMap<_, _>>();
    let edges = topology
        .edge_vertices
        .iter()
        .map(|edge| (edge.edge, edge))
        .collect::<HashMap<_, _>>();
    let carrier = |items: &[AsmHistoricalCarrierBinding]| {
        items
            .iter()
            .map(|binding| (binding.entity, binding.carrier))
            .collect::<HashMap<_, _>>()
    };
    let optional_carrier = |items: &[AsmHistoricalOptionalCarrierBinding]| {
        items
            .iter()
            .map(|binding| (binding.entity, binding.carrier))
            .collect::<HashMap<_, _>>()
    };
    let face_surfaces = carrier(&topology.face_surfaces);
    let edge_curves = optional_carrier(&topology.edge_curves);
    let coedge_pcurves = optional_carrier(&topology.coedge_pcurves);
    let vertex_points = carrier(&topology.vertex_points);
    let mut closures = Vec::new();
    for &body in &topology.bodies {
        let mut closure = HashSet::new();
        closure.insert(body);
        for &region in *history_some!(body_regions.get(&body)) {
            closure.insert(region);
            for &shell in *history_some!(region_shells.get(&region)) {
                closure.insert(shell);
                let wire_edges = *history_some!(shell_wire_edges.get(&shell));
                let free_vertices = *history_some!(shell_free_vertices.get(&shell));
                let mut shell_edges = wire_edges.iter().copied().collect::<HashSet<_>>();
                let mut shell_vertices = free_vertices.iter().copied().collect::<HashSet<_>>();
                for &face in *history_some!(shell_faces.get(&shell)) {
                    closure.insert(face);
                    closure.insert(*history_some!(face_surfaces.get(&face)));
                    for &loop_ in *history_some!(face_loops.get(&face)) {
                        closure.insert(loop_);
                        for &coedge in *history_some!(loop_coedges.get(&loop_)) {
                            closure.insert(coedge);
                            let coedge_topology = history_some!(coedges.get(&coedge));

                            shell_edges.insert(coedge_topology.edge);
                            if let Some(pcurve) = coedge_pcurves.get(&coedge).copied().flatten() {
                                closure.insert(pcurve);
                            }
                        }
                    }
                }
                for edge in shell_edges {
                    closure.insert(edge);
                    let edge_topology = history_some!(edges.get(&edge));
                    for vertex in [edge_topology.start_vertex, edge_topology.end_vertex] {
                        shell_vertices.insert(vertex);
                    }
                    if let Some(curve) = edge_curves.get(&edge).copied().flatten() {
                        closure.insert(curve);
                    }
                }
                for vertex in shell_vertices {
                    closure.insert(vertex);
                    closure.insert(*history_some!(vertex_points.get(&vertex)));
                }
            }
        }
        closures.push((body, closure));
    }
    Some(closures)
}

fn relation_map(items: &[AsmHistoricalRelation]) -> HashMap<i64, &[i64]> {
    items
        .iter()
        .map(|relation| (relation.owner_ref, relation.member_refs.as_slice()))
        .collect::<HashMap<_, _>>()
}

fn historical_topology(
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

    fn refs<'a>(ids: impl Iterator<Item = &'a str>) -> Option<Vec<i64>> {
        ids.map(stable_ref).collect::<Option<Vec<_>>>()
    }

    fn relations<'a, M>(
        items: impl Iterator<Item = (&'a str, M)>,
    ) -> Option<Vec<AsmHistoricalRelation>>
    where
        M: Iterator<Item = &'a str>,
    {
        let mut collected = Vec::new();
        for (owner, members) in items {
            let owner_ref = stable_ref(owner)?;
            let member_refs = refs(members)?;

            collected.push(AsmHistoricalRelation {
                owner_ref,
                member_refs,
            });
        }
        Some(collected)
    }

    let mut surface_radii = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
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
                _ => return None,
            };
            Some(crate::history_records::AsmHistoricalSurfaceRadius {
                surface: stable_ref(surface.id.as_str())?,
                radius: radius.abs(),
            })
        })
        .collect::<Vec<_>>();
    let mut blend_radii = HashMap::new();
    for (owner, procedural) in &brep.procedural_surfaces {
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend(definition_payload) =
            procedural.definition()
        else {
            continue;
        };
        let radius = definition_payload.radius();

        let cadmpeg_ir::geometry::BlendRadiusLaw::Constant { signed_radius } = radius else {
            continue;
        };
        let Some(surface) = stable_ref(owner.as_str()) else {
            continue;
        };
        blend_radii.insert(surface, signed_radius.get().abs());
    }

    surface_radii.retain(|candidate| !blend_radii.contains_key(&candidate.surface));
    for (surface, radius) in blend_radii {
        surface_radii.push(crate::history_records::AsmHistoricalSurfaceRadius { surface, radius });
    }
    surface_radii.sort_by_key(|a| a.surface);
    let mut surface_cylinders = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved()
            else {
                return None;
            };
            let origin = cylinder_surface.origin().get();
            let axis = *cylinder_surface.frame().axis().as_raw();
            let radius = cylinder_surface.radius().get();
            Some(crate::history_records::AsmHistoricalCylinder {
                surface: stable_ref(surface.id.as_str())?,
                origin,
                axis,
                radius: radius.abs(),
            })
        })
        .collect::<Vec<_>>();
    surface_cylinders.sort_by_key(|a| a.surface);
    let mut surface_planes = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = surface.geometry.solved()
            else {
                return None;
            };
            let origin = plane_surface.origin().get();
            let normal = *plane_surface.frame().axis().as_raw();
            Some(crate::history_records::AsmHistoricalPlane {
                surface: stable_ref(surface.id.as_str())?,
                origin,
                normal,
            })
        })
        .collect::<Vec<_>>();
    surface_planes.sort_by_key(|a| a.surface);
    let mut surface_axes = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
            let (origin, direction) = match surface.geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                    let origin = cylinder_surface.origin().get();
                    let axis = *cylinder_surface.frame().axis().as_raw();
                    (origin, axis)
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
                    let origin = cone_surface.origin().get();
                    let axis = *cone_surface.frame().axis().as_raw();
                    (origin, axis)
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                    let center = torus_surface.center().get();
                    let axis = *torus_surface.frame().axis().as_raw();
                    (center, axis)
                }
                _ => return None,
            };
            Some(crate::history_records::AsmHistoricalSurfaceAxis {
                surface: stable_ref(surface.id.as_str())?,
                origin,
                direction,
            })
        })
        .collect::<Vec<_>>();
    surface_axes.sort_by_key(|a| a.surface);

    let mut ring_neighbors = HashMap::new();
    for loop_ in &brep.loops {
        let ring = loop_.coedges();
        for (ordinal, coedge) in ring.iter().enumerate() {
            let key = (&loop_.id, coedge);

            ring_neighbors.entry(key).or_insert((
                &ring[(ordinal + 1) % ring.len()],
                &ring[(ordinal + ring.len() - 1) % ring.len()],
            ));
        }
    }

    Ok(Some(AsmHistoricalTopology {
        bodies: topology_some!(refs(brep.bodies.iter().map(|entity| entity.id.as_str()))),
        regions: topology_some!(refs(brep.regions.iter().map(|entity| entity.id.as_str()))),
        shells: topology_some!(refs(brep.shells.iter().map(|entity| entity.id.as_str()))),
        faces: topology_some!(refs(brep.faces.iter().map(|entity| entity.id.as_str()))),
        loops: topology_some!(refs(brep.loops.iter().map(|entity| entity.id.as_str()))),
        coedges: topology_some!(refs(brep.coedges.iter().map(|entity| entity.id.as_str()))),
        edges: topology_some!(refs(brep.edges.iter().map(|entity| entity.id.as_str()))),
        vertices: topology_some!(refs(brep.vertices.iter().map(|entity| entity.id.as_str()))),
        points: topology_some!(refs(brep.points.iter().map(|entity| entity.id.as_str()))),
        surfaces: topology_some!(refs(brep.surfaces.iter().map(|entity| entity.id.as_str()))),
        surface_radii,
        surface_cylinders,
        surface_planes,
        surface_axes,
        curves: topology_some!(refs(brep.curves.iter().map(|entity| entity.id.as_str()))),
        curve_axes: brep
            .curves
            .iter()
            .filter_map(|curve| {
                use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
                let (origin, direction) = match curve.geometry {
                    CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                        let origin = line_curve.origin().get();
                        let direction = *line_curve.direction().as_raw();
                        (origin, direction)
                    }
                    CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                        let center = circle_curve.center().get();
                        let axis = *circle_curve.frame().axis().as_raw();
                        (center, axis)
                    }
                    CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
                        let center = ellipse_curve.center().get();
                        let axis = *ellipse_curve.frame().axis().as_raw();
                        (center, axis)
                    }
                    _ => return None,
                };
                Some(crate::history_records::AsmHistoricalCurveAxis {
                    curve: stable_ref(curve.id.as_str())?,
                    origin,
                    direction,
                })
            })
            .collect::<Vec<_>>(),
        pcurves: topology_some!(refs(brep.pcurves.iter().map(|entity| entity.id.as_str()))),
        persistent_subentity_tags: Vec::new(),
        body_regions: topology_some!(relations(brep.bodies.iter().map(|body| {
            (
                body.id.as_str(),
                body.regions.iter().map(cadmpeg_ir::ids::RegionId::as_str),
            )
        }))),
        region_shells: topology_some!(relations(brep.regions.iter().map(|region| {
            (
                region.id.as_str(),
                region.shells.iter().map(cadmpeg_ir::ids::ShellId::as_str),
            )
        }))),
        shell_faces: topology_some!(relations(brep.shells.iter().map(|shell| {
            (
                shell.id.as_str(),
                shell.faces().iter().map(cadmpeg_ir::ids::FaceId::as_str),
            )
        }))),
        shell_wire_edges: topology_some!(relations(brep.shells.iter().map(|shell| {
            (
                shell.id.as_str(),
                shell
                    .wire_edges()
                    .iter()
                    .map(cadmpeg_ir::ids::EdgeId::as_str),
            )
        }))),
        shell_free_vertices: topology_some!(relations(brep.shells.iter().map(|shell| {
            (
                shell.id.as_str(),
                shell
                    .free_vertices()
                    .iter()
                    .map(cadmpeg_ir::ids::VertexId::as_str),
            )
        }))),
        face_loops: topology_some!(relations(brep.faces.iter().map(|face| {
            (
                face.id.as_str(),
                face.loops.iter().map(cadmpeg_ir::ids::LoopId::as_str),
            )
        }))),
        loop_coedges: topology_some!(relations(brep.loops.iter().map(|loop_| {
            (
                loop_.id.as_str(),
                loop_
                    .coedges()
                    .iter()
                    .map(cadmpeg_ir::ids::CoedgeId::as_str),
            )
        }))),
        coedge_topology: topology_some!(brep
            .coedges
            .iter()
            .map(|coedge| {
                let (next, previous) = ring_neighbors
                    .get(&(&coedge.owner_loop, &coedge.id))
                    .copied()?;
                Some(AsmHistoricalCoedge {
                    coedge: stable_ref(coedge.id.as_str())?,
                    owner_loop: stable_ref(coedge.owner_loop.as_str())?,
                    edge: stable_ref(coedge.edge.as_str())?,
                    next: stable_ref(next.as_str())?,
                    previous: stable_ref(previous.as_str())?,
                    radial_next: stable_ref(coedge.radial_next.as_str())?,
                })
            })
            .collect::<Option<Vec<_>>>()),
        edge_vertices: topology_some!(brep
            .edges
            .iter()
            .map(|edge| {
                Some(AsmHistoricalEdge {
                    edge: stable_ref(edge.id.as_str())?,
                    start_vertex: stable_ref(edge.start.as_str())?,
                    end_vertex: stable_ref(edge.end.as_str())?,
                })
            })
            .collect::<Option<Vec<_>>>()),
        face_surfaces: topology_some!(brep
            .faces
            .iter()
            .map(|face| {
                Some(AsmHistoricalCarrierBinding {
                    entity: stable_ref(face.id.as_str())?,
                    carrier: stable_ref(face.surface.as_str())?,
                })
            })
            .collect::<Option<Vec<_>>>()),
        edge_curves: topology_some!(brep
            .edges
            .iter()
            .map(|edge| {
                Some(AsmHistoricalOptionalCarrierBinding {
                    entity: stable_ref(edge.id.as_str())?,
                    carrier: match edge.curve() {
                        Some(curve) => Some(stable_ref(curve.as_str())?),
                        None => None,
                    },
                })
            })
            .collect::<Option<Vec<_>>>()),
        coedge_pcurves: topology_some!(brep
            .coedges
            .iter()
            .map(|coedge| {
                Some(AsmHistoricalOptionalCarrierBinding {
                    entity: stable_ref(coedge.id.as_str())?,
                    carrier: match coedge.pcurves.first() {
                        Some(use_) => Some(stable_ref(use_.pcurve.as_str())?),
                        None => None,
                    },
                })
            })
            .collect::<Option<Vec<_>>>()),
        vertex_points: topology_some!(brep
            .vertices
            .iter()
            .map(|vertex| {
                Some(AsmHistoricalCarrierBinding {
                    entity: stable_ref(vertex.id.as_str())?,
                    carrier: stable_ref(vertex.point.as_str())?,
                })
            })
            .collect::<Option<Vec<_>>>()),
        point_positions: topology_some!(brep
            .points
            .iter()
            .map(|point| {
                Some(AsmHistoricalPoint {
                    point: stable_ref(point.id.as_str())?,
                    position: point.position().get(),
                })
            })
            .collect::<Option<Vec<_>>>()),
    }))
}

pub(crate) fn historical_topology_with_tags(
    brep: &crate::brep::Brep,
) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
    let Some(mut topology) = historical_topology(&brep.asm)? else {
        return Ok(None);
    };
    for tag in &brep.persistent_subentity_tags {
        let (entity_kind, entity_ref) = match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(face) => {
                (AsmHistoricalEntityKind::Face, stable_ref(face.as_str()))
            }
            cadmpeg_ir::attributes::AttributeTarget::Edge(edge) => {
                (AsmHistoricalEntityKind::Edge, stable_ref(edge.as_str()))
            }
            _ => continue,
        };
        let Some(entity_ref) = entity_ref else {
            continue;
        };
        let design_references = tag.design_references.clone();
        let token = tag.token.as_str().to_owned();

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

fn materialize_record_table(
    state: &AsmDeltaState,
    archive: &HistoricalRecordArchive,
) -> Option<Vec<cadmpeg_asm::sab::Record>> {
    if state.entity_versions.is_empty() {
        return None;
    }

    let mut present = HashSet::new();

    for version in &state.entity_versions {
        present.insert(version.entity_ref);
    }
    if present.len() != state.entity_versions.len() {
        return None;
    }

    let mut records = Vec::new();

    for version in &state.entity_versions {
        let record = archive.get(&version.record_ref)?;
        if i64::try_from(record.index).ok() != Some(version.entity_ref) {
            return None;
        }
        for token in record.tokens.iter() {
            let cadmpeg_asm::sab::Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0 && !present.contains(reference) {
                return None;
            }
        }
        records.push(clone_historical_record(record));
    }
    records.sort_unstable_by_key(|a| a.index);
    Some(records)
}

fn decode_bulletin_boards(
    bytes: &[u8],
    mut position: usize,
    stream: &str,
    state_offset: usize,
    state_id: &str,
    width: RefWidth,
) -> Option<(Vec<AsmBulletinBoard>, usize)> {
    if bytes.get(position) == Some(&0x11) {
        return Some((Vec::new(), position));
    }
    let mut boards = Vec::new();
    loop {
        let board_offset = position;
        let present = take_int(bytes, &mut position, 0x04, width)?;
        if present == 0 {
            break;
        }
        let owner_ref = take_int(bytes, &mut position, 0x0c, width)?;
        let number = take_int(bytes, &mut position, 0x04, width)?;
        let board_id = crate::ids::native_scoped_id(
            stream,
            "asm-bulletin-board",
            format_args!("{state_offset:010}:{:06}", boards.len()),
        );
        let mut changes = Vec::new();
        loop {
            let change_offset = position;
            let present = take_int(bytes, &mut position, 0x04, width)?;
            if present == 0 {
                break;
            }
            let old = take_int(bytes, &mut position, 0x0c, width)?;
            let new = take_int(bytes, &mut position, 0x0c, width)?;
            let kind = match (old >= 0, new >= 0) {
                (false, true) => AsmEntityChangeKind::Insert { new },
                (true, false) => AsmEntityChangeKind::Delete { old },
                (true, true) => AsmEntityChangeKind::Update { old, new },
                (false, false) => return None,
            };

            let change_id = crate::ids::native_scoped_id(
                stream,
                "asm-entity-change",
                format_args!(
                    "{state_offset:010}:{:06}:{:06}",
                    boards.len(),
                    changes.len()
                ),
            );
            let parent = board_id.clone();
            changes.push(AsmEntityChange {
                id: change_id,
                parent,
                byte_offset: u64_from_index(change_offset),
                kind,
            });
        }

        let parent = state_id.to_owned();
        boards.push(AsmBulletinBoard {
            id: board_id,
            parent,
            byte_offset: u64_from_index(board_offset),
            owner_ref,
            number,
            changes,
        });
    }
    Some((boards, position))
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
        Ok(records) => {
            let mut decoded = Vec::new();
            for record in records {
                let mut entity_references = Vec::new();

                for token in record.tokens.iter() {
                    if let cadmpeg_asm::sab::Token::Ref(value) = token {
                        entity_references.push(*value);
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
                let raw_bytes = source.to_vec();

                let id = crate::ids::native_scoped_id(
                    stream,
                    "asm-history-record",
                    format_args!("{:010}", record.offset),
                );
                let parent = state_id.to_owned();
                decoded.push(AsmHistoryRecord {
                    id,
                    parent,
                    revision_id: None,
                    byte_offset: u64_from_index(record.offset),
                    framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                        index: u64_from_index(record.index),
                        name: record.name,
                        entity_references,
                    },
                    raw_bytes: raw_bytes.into(),
                });
            }
            Ok(decoded)
        }
        Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => Err(error.into()),
        Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
            Err(error.into_codec_error())
        }
        Err(error) => {
            let raw_bytes = bytes[start..limit].to_vec();
            let mut decoded = Vec::new();
            let id = crate::ids::native_scoped_id(
                stream,
                "asm-history-record",
                format_args!("{start:010}"),
            );
            let parent = state_id.to_owned();
            let error = format!("{error}");
            decoded.push(AsmHistoryRecord {
                id,
                parent,
                revision_id: None,
                byte_offset: u64_from_index(start),
                framing: crate::history_records::AsmHistoryRecordFraming::Opaque { error },
                raw_bytes: raw_bytes.into(),
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
