// SPDX-License-Identifier: Apache-2.0
//! Decode the ASM construction-history partition after the active model slice.
//!
//! [`decode`] reads `delta_state` headers, bulletin-board entity changes, and
//! history records while retaining source bytes for records without typed
//! semantics.

#![cfg_attr(
    test,
    allow(clippy::cloned_ref_to_slice_refs, clippy::default_trait_access)
)]

use crate::bytes::int_at;
use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmHistory,
    AsmHistoryRecord, AsmPreamble,
};
use cadmpeg_asm::history::{
    delta_state_offsets, frame_state_records, read_delta_state_header, snapshot_revision_ids,
    walk_bulletin_boards, BulletinBoardItem, DeltaStateHeader, DeltaStateHeaderRead, EntityChange,
    StateRecords,
};
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::u64_from_index;
#[cfg(test)]
use std::collections::BTreeMap;
use std::collections::{HashMap, HashSet};

pub(crate) mod binding_histories;
pub(crate) mod body_closures;
pub(crate) mod body_selections;
pub(crate) mod edge_operand_history;
pub(crate) mod face_boundaries;
pub(crate) mod face_operand_history;
mod face_resolution;
pub(crate) mod face_selections;
pub(crate) mod feature_outputs;
pub(crate) mod hem_gap_forms;
mod historical_index;
pub(crate) mod history_states;
pub(crate) mod input_topologies;
pub(crate) mod path_selections;
mod profile_face_groups;
pub(crate) mod scope_histories;
pub(crate) mod topology_rows;
mod treatment_candidates;
pub(crate) mod vertex_recipes;

mod arc_extremes;
pub(crate) mod binding;
mod blend_spine;
mod chamfer_supports;

mod body_candidates;
pub(crate) mod body_history_tail;
mod body_index;
pub(crate) mod body_recipe_operands;
pub(crate) mod body_writers;
mod cache;
pub(crate) mod chamfer_blends;
mod combine_binding;
pub(crate) mod combine_history_tools;
mod combine_selection;
pub(crate) mod compact_lineage;
mod delete_face;
pub(crate) mod edge_flange;
pub(crate) mod edge_recipe_scope;
mod edge_samples;
pub(crate) mod extrude_target;
pub(crate) mod face_admission;
mod face_envelopes;
mod face_recipe;
mod face_samples;
mod face_support;
mod fallback_face;
pub(crate) mod fillet_blends;
pub(crate) mod fragment_edge;
mod generated_bodies;
pub(crate) mod history_bodies;
mod hole_support;
pub(crate) mod linked_states;
pub(crate) mod loft_solid;
mod loop_index;
mod move_face;
mod output_blob;
mod output_chain;
mod output_measure;
mod output_properties;
mod pattern_bodies;
pub(crate) mod pipe_section;
mod recipe_index;
mod reference_edge;
mod reference_face;
pub(crate) mod selection;
mod shell_walls;
pub(crate) mod stored_bodies;
mod surface_rows;
pub(crate) mod thread_face;
mod topology_cache;
mod treatment_index;

pub(crate) use generated_bodies::bind_generated_body_selections;

const EPS_HISTORY_HEM_GAP_LENGTH_FORM_E7: f64 = 1.0e-7;
const EPS_HISTORY_PARALLEL_DIRECTIONS_E7: f64 = 1.0e-7;
const EPS_HISTORY_RESOLVE_THREAD_FACE_BY_TRANSITION_E9: f64 = 1.0e-9;
const EPS_HISTORY_CYCLIC_POINT_SUBSEQUENCE_E12: f64 = 1.0e-12;
const EPS_HISTORY_SAME_AXIS_LINE_E9: f64 = 1.0e-9;
const EPS_HISTORY_CIRCLE_EDGE_E7: f64 = 1.0e-7;
const EPS_HISTORY_SAME_AXIS_LINE_E8: f64 = 1.0e-8;
const EPS_HISTORY_BIND_MIRROR_SELECTION_PLANES_E9: f64 = 1.0e-9;
const EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E9: f64 = 1.0e-9;
const EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E8: f64 = 1.0e-8;
const EPS_HISTORY_HISTORICAL_LOOP_PLANE_E9: f64 = 1.0e-9;

const PREAMBLE: &[u8] = b"\x0d\x0ehistory_stream";
/// Relative tolerance for matching independently decoded millimetre point carriers.
const WORK_POINT_POSITION_TOLERANCE: f64 = 1.0e-9;

pub(crate) fn graph_is_coherent(history: &AsmHistory) -> bool {
    if history.states().is_empty() {
        return false;
    }
    let mut by_index = HashMap::new();
    for state in history.states() {
        by_index.insert(state.node_index, state);
    }
    if by_index.len() != history.states().len()
        || history
            .states()
            .iter()
            .any(|state| state.node_index < 0 || state.parent != history.id)
    {
        return false;
    }
    let mut heads = history
        .states()
        .iter()
        .filter(|state| state.previous_ref.is_none());
    let head = heads.next();
    let tails = history
        .states()
        .iter()
        .filter(|state| state.next_ref.is_none())
        .count();
    if head.is_none() || heads.next().is_some() || tails != 1 {
        return false;
    }
    let Some(head) = head else {
        return false;
    };
    // The preamble's first field need not name the head state (asm-open-items HS-01).
    if history
        .preamble
        .is_some_and(|preamble| preamble.history_entry_count < 0)
    {
        return false;
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
    visited.len() == history.states().len()
}

/// Decode the construction-history tail of an ASM stream: every `delta_state`
/// record ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#32-delta_state-records)) from `bytes`, each with its `BulletinBoard` chain of
/// per-entity insert/delete/update changes and the raw history-entity records
/// framed between it and the next `delta_state`. `stream` is the source ZIP
/// entry name, recorded in each decoded item's provenance. Returns `None` when
/// `bytes` carries no `delta_state` record that states a state (the stream is
/// a construction snapshot with no history tail). `width` is the
/// stream's integer/ref width (4 for `BinaryFile4`, 8 for `BinaryFile8`).
/// `remaining_retained_bytes` is the session's remaining retained-byte allowance,
/// less the estimates reserved for earlier history streams. A history whose
/// estimate does not fit keeps only its state headers. `binding_work` bounds
/// the topology binding of the history; a history that exhausts it has no
/// historical topology.
///
/// # Errors
///
/// A [`HistoryFailure`] when a `delta_state` record does not read, so no
/// state of the history is kept: it names where the history partition
/// starts.
pub(crate) fn decode(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    stream: &str,
    width: RefWidth,
    remaining_retained_bytes: &mut u64,
    binding_work: &cadmpeg_core::decode::WorkBudget,
) -> Result<Option<AsmHistory>, HistoryFailure> {
    let framing = HistoryFraming::scan(bytes, stream);
    let start = framing
        .preamble_offset
        .or_else(|| framing.delta_offsets.first().copied())
        .unwrap_or(0);
    decode_framed(
        ctx,
        bytes,
        stream,
        width,
        remaining_retained_bytes,
        binding_work,
        framing,
    )
    .map_err(|error| HistoryFailure { start, error })
}

/// An ASM history whose states do not read.
#[derive(Debug)]
pub(crate) struct HistoryFailure {
    /// Where the history partition starts in its stream: at the history
    /// preamble, or at the first `delta_state` record of a stream without
    /// one.
    pub(crate) start: usize,
    pub(crate) error: cadmpeg_core::CodecError,
}

fn decode_framed(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    stream: &str,
    width: RefWidth,
    remaining_retained_bytes: &mut u64,
    binding_work: &cadmpeg_core::decode::WorkBudget,
    framing: HistoryFraming,
) -> Result<Option<AsmHistory>, cadmpeg_core::CodecError> {
    let _chamfer_orientation = chamfer_supports::HistoryScope::enter(&framing.history_id);
    let mut states = Vec::new();
    let mut preframed_records = Vec::new();
    for (ordinal, &offset) in framing.delta_offsets.iter().enumerate() {
        let header = match state_header(bytes, offset, width) {
            DeltaStateHeaderRead::Framed(header) => header,
            DeltaStateHeaderRead::Unframed => continue,
            DeltaStateHeaderRead::Invalid => {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "the ASM delta_state record at byte {offset} of {stream} has a missing or mistagged header integer"
                )));
            }
        };
        let state_record_id = delta_state_record_id(&header, stream);
        let Some((bulletin_boards, body_end)) = decode_bulletin_boards(
            bytes,
            header.body_start,
            stream,
            offset,
            &state_record_id,
            width,
        ) else {
            return Err(cadmpeg_core::CodecError::malformed(format_args!(
                "the bulletin boards of the ASM delta_state record at byte {offset} of {stream} do not frame"
            )));
        };
        let records = decode_history_records(
            ctx,
            bytes,
            body_end,
            framing.delta_offsets.get(ordinal + 1).copied(),
            (stream, &state_record_id),
            width,
            &mut preframed_records,
        )?;
        states.push(delta_state_from_header(
            header,
            state_record_id,
            &framing.history_id,
            bulletin_boards,
            records,
        ));
    }
    bind_snapshot_revision_ids(&mut states);
    let binding_budget_exceeded = admit_history_binding(ctx, &states, remaining_retained_bytes)?
        == HistoryBindingAdmission::Skipped;
    let cached_archive = if binding_budget_exceeded {
        drop(preframed_records);
        for state in &mut states {
            state.bulletin_boards = Vec::new();
            state.records = Vec::new();
        }
        None
    } else {
        let mut preframed = HashMap::with_capacity(preframed_records.len());
        for record in preframed_records {
            preframed.insert(record.offset, record);
        }
        bind_complete_record_tables(
            ctx,
            &mut states,
            bytes,
            width,
            binding_work,
            Some(&mut preframed),
        )?
    };
    if states.is_empty() {
        return Ok(None);
    }
    let history = framing.into_history(bytes, width, binding_budget_exceeded, states);
    if let Some(archive) = cached_archive {
        history.cache_record_archive(archive);
    }
    Ok(Some(history))
}

/// Decode the header-only index of one ASM history stream.
///
/// Each state carries its identity and chain links (state id, node index,
/// previous and next references) and no bulletin boards, record table or
/// bound transition. The index rejects a stream on the same header, framing
/// and bulletin-board conditions as [`decode`]; it frames the boards without
/// retaining them.
pub(crate) fn decode_header_index(
    bytes: &[u8],
    stream: &str,
    width: RefWidth,
) -> Option<AsmHistory> {
    let framing = HistoryFraming::scan(bytes, stream);
    let mut states = Vec::new();
    for &offset in &framing.delta_offsets {
        let header = match state_header(bytes, offset, width) {
            DeltaStateHeaderRead::Framed(header) => header,
            DeltaStateHeaderRead::Unframed => continue,
            DeltaStateHeaderRead::Invalid => return None,
        };
        walk_bulletin_boards(bytes, header.body_start, width, |_| {})?;
        states.push(delta_state_from_header(
            header,
            delta_state_record_id(&header, stream),
            &framing.history_id,
            Vec::new(),
            Vec::new(),
        ));
    }
    if states.is_empty() {
        return None;
    }
    Some(framing.into_history(bytes, width, false, states))
}

/// Length of the `delta_state` marker, from the `0x11` that ends the record
/// before it through the record name.
const STATE_MARKER_LEN: usize = b"\x11\x0d\x0bdelta_state".len();

/// The header of the `delta_state` record whose marker is at `offset`. A
/// marker that ends the stream stores no header, so, like a header that
/// frames no state body, it states no state.
fn state_header(bytes: &[u8], offset: usize, width: RefWidth) -> DeltaStateHeaderRead {
    match read_delta_state_header(bytes, offset, width) {
        DeltaStateHeaderRead::Invalid
            if bytes
                .len()
                .checked_sub(offset)
                .is_none_or(|rest| rest <= STATE_MARKER_LEN) =>
        {
            DeltaStateHeaderRead::Unframed
        }
        read => read,
    }
}

/// Drop chamfer face senses recorded for the design decode that just finished.
pub(crate) fn clear_chamfer_orientation() {
    chamfer_supports::clear();
}

/// Stream-level layout of one ASM history: its preamble and `delta_state` offsets.
struct HistoryFraming {
    preamble_offset: Option<usize>,
    history_offset: usize,
    history_id: String,
    delta_offsets: Vec<usize>,
}

impl HistoryFraming {
    fn scan(bytes: &[u8], stream: &str) -> Self {
        let preamble_offset = bytes
            .windows(PREAMBLE.len())
            .position(|window| window == PREAMBLE);
        let history_offset = preamble_offset.unwrap_or(0);
        let history_id = crate::ids::native_scoped_id(
            stream,
            "asm-history",
            format_args!("{history_offset:010}"),
        );
        let delta_offsets = delta_state_offsets(bytes);
        Self {
            preamble_offset,
            history_offset,
            history_id,
            delta_offsets,
        }
    }

    fn into_history(
        self,
        bytes: &[u8],
        width: RefWidth,
        binding_budget_exceeded: bool,
        states: Vec<AsmDeltaState>,
    ) -> AsmHistory {
        let preamble = self
            .preamble_offset
            .and_then(|offset| decode_preamble(bytes, offset + PREAMBLE.len(), width))
            .map(|(stream_size, history_entry_count)| AsmPreamble {
                stream_size,
                history_entry_count,
            });
        AsmHistory::new(
            self.history_id,
            u64_from_index(self.history_offset),
            preamble,
            binding_budget_exceeded,
            states,
        )
    }
}

fn delta_state_record_id(header: &DeltaStateHeader, stream: &str) -> String {
    let offset = header.offset;
    crate::ids::native_scoped_id(stream, "asm-delta-state", format_args!("{offset:010}"))
}

fn delta_state_from_header(
    header: DeltaStateHeader,
    id: String,
    history_id: &str,
    bulletin_boards: Vec<crate::history_records::AsmBulletinBoard>,
    records: Vec<crate::history_records::AsmHistoryRecord>,
) -> AsmDeltaState {
    AsmDeltaState {
        id,
        parent: history_id.to_owned(),
        byte_offset: u64_from_index(header.offset),
        state_id: header.state_id,
        version_flag: header.version_flag,
        state_flag: header.state_flag,
        previous_ref: (header.previous >= 0).then_some(header.previous),
        next_ref: (header.next >= 0).then_some(header.next),
        node_index: header.node_index,
        partner_ref: (header.partner >= 0).then_some(header.partner),
        owner_ref: header.owner_ref,
        bulletin_boards,
        records,
        transition: None,
    }
}

fn bind_snapshot_revision_ids(states: &mut [AsmDeltaState]) {
    let snapshot_record_count = states
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| record.name() != "End-of-ASM-data")
        .count();
    let changes = states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
        .map(|change| entity_change(change.kind));
    let Some(ids) = snapshot_revision_ids(changes, snapshot_record_count) else {
        return;
    };
    for (record, revision_id) in states
        .iter_mut()
        .flat_map(|state| &mut state.records)
        .filter(|record| record.name() != "End-of-ASM-data")
        .zip(ids)
    {
        record.revision_id = Some(revision_id);
    }
}

fn entity_change(kind: AsmEntityChangeKind) -> EntityChange {
    match kind {
        AsmEntityChangeKind::Insert { new } => EntityChange::Insert { new },
        AsmEntityChangeKind::Delete { old } => EntityChange::Delete { old },
        AsmEntityChangeKind::Update { old, new } => EntityChange::Update { old, new },
    }
}

/// Borrowed view of `states` for the shared history walks.
fn delta_state_views(states: &[AsmDeltaState]) -> Vec<cadmpeg_asm::history::DeltaStateView<'_>> {
    states
        .iter()
        .map(|state| cadmpeg_asm::history::DeltaStateView {
            state_id: state.state_id,
            node_index: state.node_index,
            previous_ref: state.previous_ref,
            next_ref: state.next_ref,
            changes: state
                .bulletin_boards
                .iter()
                .flat_map(|board| &board.changes)
                .map(|change| entity_change(change.kind))
                .collect(),
            records: state
                .records
                .iter()
                .map(|record| cadmpeg_asm::history::StateRecordView {
                    revision_id: record.revision_id,
                    byte_offset: record.byte_offset,
                    raw_bytes: record.raw_bytes.as_slice(),
                    name: record.name(),
                })
                .collect(),
        })
        .collect()
}

pub(crate) fn archived_active_record_count(
    states: &[AsmDeltaState],
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    cadmpeg_asm::history::archived_active_record_count(&delta_state_views(states))
}

pub(crate) fn insert_only_active_record_count(states: &[AsmDeltaState]) -> Option<usize> {
    cadmpeg_asm::history::insert_only_active_record_count(&delta_state_views(states))
}

/// Estimated native `asm_delta_states` text bytes per changed record slot.
/// History streams with 206 to 385,790 changed slots retain 96.96 to 161.14
/// complete-scope bytes per slot. Rounded up to the next ten bytes.
pub(crate) const HISTORY_RETAINED_BYTES_PER_CHANGED_SLOT: u64 = 170;

/// Whether one history stream's estimated retained bytes fit the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryBindingAdmission {
    /// The estimate fits; it is reserved and the history is bound.
    Admitted,
    /// The estimate exceeds the remaining allowance; the history keeps its state
    /// headers only.
    Skipped,
}

/// Validate the chain, then reserve its estimated retained bytes before decoding
/// record tables. A skipped history leaves the allowance unchanged and keeps no
/// bulletin boards, history records or transitions. A chain that does not
/// validate is admitted and stays unbound.
pub(crate) fn admit_history_binding(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
    remaining_retained_bytes: &mut u64,
) -> Result<HistoryBindingAdmission, cadmpeg_core::CodecError> {
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
        return Ok(HistoryBindingAdmission::Admitted);
    };
    let Ok(active_count) = i64::try_from(active_count) else {
        return Ok(HistoryBindingAdmission::Admitted);
    };
    let mut changed_slots = Some(0_u64);
    if !walk_entity_versions_hashed(states, active_count, &archived_ids, |ordinal, versions| {
        let state = &states[ordinal];
        if state.next_ref.is_none() {
            changed_slots =
                changed_slots.and_then(|total| total.checked_add(u64_from_index(versions.len())));
        }
        for board in &state.bulletin_boards {
            changed_slots = changed_slots
                .and_then(|total| total.checked_add(u64_from_index(board.changes.len())));
        }
    }) {
        return Ok(HistoryBindingAdmission::Admitted);
    }
    let Some(remaining) = changed_slots
        .and_then(|count| count.checked_mul(HISTORY_RETAINED_BYTES_PER_CHANGED_SLOT))
        .and_then(|estimate| {
            ctx.observe_retained_allowance(*remaining_retained_bytes, estimate);
            remaining_retained_bytes.checked_sub(estimate)
        })
    else {
        return Ok(HistoryBindingAdmission::Skipped);
    };
    *remaining_retained_bytes = remaining;
    Ok(HistoryBindingAdmission::Admitted)
}

#[cfg(test)]
pub(crate) fn walk_entity_versions(
    states: &[AsmDeltaState],
    active_count: i64,
    archived_ids: &[i64],
    visit: impl FnMut(usize, &BTreeMap<i64, i64>),
) -> bool {
    cadmpeg_asm::history::walk_entity_versions(
        &delta_state_views(states),
        active_count,
        archived_ids,
        visit,
    )
}

fn walk_entity_versions_hashed(
    states: &[AsmDeltaState],
    active_count: i64,
    archived_ids: &[i64],
    visit: impl FnMut(usize, &binding::FastMap<i64, i64>),
) -> bool {
    cadmpeg_asm::history::walk_entity_versions_hashed::<binding::FastBuildHasher>(
        &delta_state_views(states),
        active_count,
        archived_ids,
        visit,
    )
}

fn bind_complete_record_tables(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
    bytes: &[u8],
    width: RefWidth,
    work: &cadmpeg_core::decode::WorkBudget,
    preframed: Option<&mut HashMap<usize, cadmpeg_asm::sab::Record>>,
) -> Result<Option<crate::history_records::CachedRecordArchive>, cadmpeg_core::CodecError> {
    binding::bind_with_cache_and_preframed(ctx, states, bytes, width, work, preframed)
        .map(|(_, cache)| cache)
}

pub(crate) fn decode_record_archive(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
    bytes: &[u8],
    width: RefWidth,
) -> Result<Option<HistoricalRecordArchive>, cadmpeg_core::CodecError> {
    decode_record_archive_with_preframed(ctx, states, bytes, width, None)
}

pub(crate) fn decode_record_archive_with_preframed(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
    bytes: &[u8],
    width: RefWidth,
    preframed: Option<&mut HashMap<usize, cadmpeg_asm::sab::Record>>,
) -> Result<Option<HistoricalRecordArchive>, cadmpeg_core::CodecError> {
    let Some(frames) = cadmpeg_asm::history::decode_record_archive_with_preframed(
        ctx,
        &delta_state_views(states),
        bytes,
        width,
        preframed,
    )?
    else {
        return Ok(None);
    };
    Ok(historical_record_archive(
        states,
        &frames.active,
        frames.archived,
    ))
}

pub(crate) type HistoricalRecordArchive = HashMap<i64, cadmpeg_asm::sab::Record>;

fn historical_record_archive(
    states: &[AsmDeltaState],
    active_records: &[cadmpeg_asm::sab::Record],
    archived_frames: HashMap<i64, cadmpeg_asm::sab::Record>,
) -> Option<HistoricalRecordArchive> {
    cadmpeg_asm::history::historical_record_archive(
        &delta_state_views(states),
        cadmpeg_asm::history::RecordArchiveFrames {
            active: active_records.to_vec(),
            archived: archived_frames,
        },
    )
}

/// Release derived topologies after projection and retain only late-selection rows.
pub(crate) fn discard_projection_caches(histories: &mut [AsmHistory]) {
    for history in histories {
        history.topology_scope = crate::history_records::AsmTopologyScope::Retained;
        for state in history.states_mut() {
            if let Some(transition) = &mut state.transition {
                transition.rows.retain_projection_rows();
            }
        }
        history.release_topologies();
    }
}

pub(crate) fn projection_was_finalized(histories: &[AsmHistory]) -> bool {
    !histories.is_empty()
        && histories.iter().all(|history| {
            history.topology_scope == crate::history_records::AsmTopologyScope::Retained
                && !history.states().is_empty()
        })
}

/// Maximum delta-state count for which `AsmHistory` keeps materialized state
/// topologies cached across sequential oldest-first history passes.
pub(crate) const KEEP_MATERIALIZED_MAX_STATES: usize = 256;
#[cfg(test)]
pub(crate) fn materialize_record_table(
    versions: &BTreeMap<i64, i64>,
    archive: &HistoricalRecordArchive,
) -> Option<Vec<cadmpeg_asm::sab::Record>> {
    cadmpeg_asm::history::materialize_record_table(versions, |revision| archive.get(&revision))
}

fn entity_change_kind(kind: EntityChange) -> AsmEntityChangeKind {
    match kind {
        EntityChange::Insert { new } => AsmEntityChangeKind::Insert { new },
        EntityChange::Delete { old } => AsmEntityChangeKind::Delete { old },
        EntityChange::Update { old, new } => AsmEntityChangeKind::Update { old, new },
    }
}

fn decode_bulletin_boards(
    bytes: &[u8],
    position: usize,
    stream: &str,
    state_offset: usize,
    state_id: &str,
    width: RefWidth,
) -> Option<(Vec<AsmBulletinBoard>, usize)> {
    let mut boards = Vec::<AsmBulletinBoard>::new();
    let end = walk_bulletin_boards(bytes, position, width, |item| match item {
        BulletinBoardItem::Board {
            offset,
            owner_ref,
            number,
        } => {
            let id = crate::ids::native_scoped_id(
                stream,
                "asm-bulletin-board",
                format_args!("{state_offset:010}:{:06}", boards.len()),
            );
            boards.push(AsmBulletinBoard {
                id,
                parent: state_id.to_owned(),
                byte_offset: u64_from_index(offset),
                owner_ref,
                number,
                changes: Vec::new(),
            });
        }
        BulletinBoardItem::Change { offset, kind } => {
            let (Some(ordinal), Some(board)) = (boards.len().checked_sub(1), boards.last_mut())
            else {
                return;
            };
            let id = crate::ids::native_scoped_id(
                stream,
                "asm-entity-change",
                format_args!("{state_offset:010}:{ordinal:06}:{:06}", board.changes.len()),
            );
            let parent = board.id.clone();
            board.changes.push(AsmEntityChange {
                id,
                parent,
                byte_offset: u64_from_index(offset),
                kind: entity_change_kind(kind),
            });
        }
    })?;
    Some((boards, end))
}

fn decode_history_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    state_end: usize,
    next_delta: Option<usize>,
    (stream, state_id): (&str, &str),
    width: RefWidth,
    preframed_records: &mut Vec<cadmpeg_asm::sab::Record>,
) -> Result<Vec<AsmHistoryRecord>, cadmpeg_core::CodecError> {
    match frame_state_records(ctx, bytes, state_end, next_delta, width)? {
        StateRecords::Empty => Ok(Vec::new()),
        StateRecords::Framed(records) => {
            let mut decoded = Vec::with_capacity(records.len());
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
                let record_offset = record.offset;
                let record_index = record.index;
                let keep_preframed =
                    record.name != "End-of-ASM-data" && source.last() == Some(&0x11);
                let name = if keep_preframed {
                    let cloned = record.name.clone();
                    preframed_records.push(record);
                    cloned
                } else {
                    record.name
                };
                decoded.push(AsmHistoryRecord {
                    id,
                    parent,
                    revision_id: None,
                    byte_offset: u64_from_index(record_offset),
                    framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                        index: u64_from_index(record_index),
                        name,
                        entity_references,
                    },
                    raw_bytes: raw_bytes.into(),
                });
            }
            Ok(decoded)
        }
        StateRecords::Unframed {
            start,
            limit,
            error,
        } => {
            let raw_bytes = bytes[start..limit].to_vec();
            let mut decoded = Vec::new();
            let id = crate::ids::native_scoped_id(
                stream,
                "asm-history-record",
                format_args!("{start:010}"),
            );
            let parent = state_id.to_owned();
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
