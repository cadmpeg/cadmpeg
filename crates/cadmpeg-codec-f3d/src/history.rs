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

use crate::bytes::int_at;
use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmEntityVersion,
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder, AsmHistoricalEdge,
    AsmHistoricalEntityDelta, AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint,
    AsmHistoricalRelation, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory, AsmHistoryRecord,
};
use crate::records::{
    AsmHistoricalEntityKind, DesignEdgeIdentityOperand, DesignExtrudeSelectionMember,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const DELTA: &[u8] = b"\x11\x0d\x0bdelta_state";
const PREAMBLE: &[u8] = b"\x0d\x0ehistory_stream";
/// Relative tolerance for matching independently decoded millimetre point carriers.
const WORK_POINT_POSITION_TOLERANCE: f64 = 1.0e-9;

pub(crate) fn graph_is_coherent(history: &AsmHistory) -> bool {
    if history.states.is_empty()
        || history.stream_size.is_some() != history.history_entry_count.is_some()
    {
        return false;
    }
    let by_index = history
        .states
        .iter()
        .map(|state| (state.node_index, state))
        .collect::<HashMap<_, _>>();
    if by_index.len() != history.states.len()
        || history
            .states
            .iter()
            .any(|state| state.node_index < 0 || state.parent != history.id)
    {
        return false;
    }
    let heads = history
        .states
        .iter()
        .filter(|state| state.previous_ref.is_none())
        .collect::<Vec<_>>();
    let tails = history
        .states
        .iter()
        .filter(|state| state.next_ref.is_none())
        .count();
    if heads.len() != 1 || tails != 1 {
        return false;
    }
    if let (Some(size), Some(entry_count)) = (history.stream_size, history.history_entry_count) {
        if heads[0].state_id != size || entry_count < 0 {
            return false;
        }
    }
    let mut visited = HashSet::new();
    let mut previous = None;
    let mut current = Some(heads[0].node_index);
    while let Some(index) = current {
        let Some(state) = by_index.get(&index) else {
            return false;
        };
        if !visited.insert(index) || state.previous_ref != previous {
            return false;
        }
        if state.version_flag != 1 || state.state_flag != 0 {
            return false;
        }
        for board in &state.bulletin_boards {
            if board.parent != state.id
                || board.changes.iter().any(|change| {
                    let expected = match (change.old_ref.is_some(), change.new_ref.is_some()) {
                        (false, true) => Some(AsmEntityChangeKind::Insert),
                        (true, false) => Some(AsmEntityChangeKind::Delete),
                        (true, true) => Some(AsmEntityChangeKind::Update),
                        (false, false) => None,
                    };
                    change.parent != board.id || expected != Some(change.kind)
                })
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
    bytes: &[u8],
    stream: &str,
    width: usize,
    limits: &cadmpeg_core::decode::ResourceLimits,
) -> Option<AsmHistory> {
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
        let state_id = take_int(bytes, &mut position, 0x04, width)?;
        let version_flag = take_int(bytes, &mut position, 0x04, width)?;
        let state_flag = take_int(bytes, &mut position, 0x04, width)?;
        let previous = take_int(bytes, &mut position, 0x0c, width)?;
        let next = take_int(bytes, &mut position, 0x0c, width)?;
        let node_index = take_int(bytes, &mut position, 0x0c, width)?;
        let partner = take_int(bytes, &mut position, 0x0c, width)?;
        let owner_ref = take_int(bytes, &mut position, 0x0c, width)?;
        if bytes.get(position) != Some(&0x0b) {
            continue;
        }
        let (bulletin_boards, body_end) =
            decode_bulletin_boards(bytes, position + 1, stream, offset, &state_record_id, width)?;
        let records = decode_history_records(
            bytes,
            body_end,
            delta_offsets.get(ordinal + 1).copied(),
            stream,
            &state_record_id,
            width,
        );
        states.push(AsmDeltaState {
            id: state_record_id,
            parent: history_id.clone(),
            byte_offset: offset as u64,
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
            record_table_complete: false,
            topology: None,
            transition: None,
        });
    }
    bind_snapshot_revision_ids(&mut states);
    bind_historical_entity_versions(&mut states);
    let record_table_binding_budget_exceeded =
        bind_complete_record_tables(&mut states, bytes, width, limits);
    if states.is_empty() {
        return None;
    }

    let (stream_size, history_entry_count) = preamble_offset
        .and_then(|offset| decode_preamble(bytes, offset + PREAMBLE.len(), width))
        .map_or((None, None), |(size, high)| (Some(size), Some(high)));
    let offset = history_offset;
    Some(AsmHistory {
        id: history_id,
        byte_offset: offset as u64,
        stream_size,
        history_entry_count,
        record_table_binding_budget_exceeded,
        projection_finalized: false,
        states,
    })
}

fn bind_snapshot_revision_ids(states: &mut [AsmDeltaState]) {
    let mut old_references = states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
        .filter_map(|change| change.old_ref)
        .collect::<Vec<_>>();
    old_references.sort_unstable();
    if old_references.first().is_none_or(|first| {
        old_references
            .iter()
            .copied()
            .ne(*first..*first + old_references.len() as i64)
    }) {
        return;
    }
    let snapshot_records = states
        .iter_mut()
        .flat_map(|state| &mut state.records)
        .filter(|record| record.name != "End-of-ASM-data")
        .collect::<Vec<_>>();
    if snapshot_records.len() != old_references.len() {
        return;
    }
    for (record, revision_id) in snapshot_records.into_iter().zip(old_references) {
        record.revision_id = Some(revision_id);
    }
}

fn is_history_boundary_record(record: &AsmHistoryRecord) -> bool {
    matches!(
        record.name.as_str(),
        "End-of-ASM-History-Section" | "End-of-ASM-data"
    )
}

fn archived_active_record_count(states: &[AsmDeltaState]) -> Option<usize> {
    let mut archived = states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
        .collect::<Vec<_>>();
    archived.sort_unstable();
    let &active_count = archived.first()?;
    if active_count <= 0
        || archived
            .iter()
            .copied()
            .ne(active_count..active_count + archived.len() as i64)
    {
        return None;
    }
    usize::try_from(active_count).ok()
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
        let (None, Some(new_ref)) = (change.old_ref, change.new_ref) else {
            return None;
        };
        if new_ref <= 0 || !inserted.insert(new_ref) {
            return None;
        }
    }
    let &last = inserted.last()?;
    if inserted.iter().copied().ne(1..=last) {
        return None;
    }
    usize::try_from(last.checked_add(1)?).ok()
}

fn bind_historical_entity_versions(states: &mut [AsmDeltaState]) {
    let mut archived_ids = states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
        .collect::<Vec<_>>();
    archived_ids.sort_unstable();
    let active_count = archived_active_record_count(states)
        .or_else(|| insert_only_active_record_count(states))
        .and_then(|count| i64::try_from(count).ok());
    let Some(active_count) = active_count else {
        return;
    };
    let by_node = states
        .iter()
        .enumerate()
        .map(|(ordinal, state)| (state.node_index, ordinal))
        .collect::<HashMap<_, _>>();
    if by_node.len() != states.len() {
        return;
    }
    let heads = states
        .iter()
        .enumerate()
        .filter(|(_, state)| state.previous_ref.is_none())
        .map(|(ordinal, _)| ordinal)
        .collect::<Vec<_>>();
    let [mut ordinal] = heads.as_slice() else {
        return;
    };
    let mut versions = (0..active_count)
        .map(|id| (id, id))
        .collect::<BTreeMap<_, _>>();
    let mut projected = HashMap::new();
    let mut visited = HashSet::new();
    loop {
        let state = &states[ordinal];
        if !visited.insert(state.node_index) {
            return;
        }
        projected.insert(
            state.node_index,
            versions
                .iter()
                .map(|(&entity_ref, &record_ref)| AsmEntityVersion {
                    entity_ref,
                    record_ref,
                })
                .collect::<Vec<_>>(),
        );
        for change in state
            .bulletin_boards
            .iter()
            .flat_map(|board| &board.changes)
        {
            match (change.old_ref, change.new_ref) {
                (Some(old), Some(new)) => {
                    if !versions.contains_key(&new) || archived_ids.binary_search(&old).is_err() {
                        return;
                    }
                    versions.insert(new, old);
                }
                (None, Some(new)) => {
                    if versions.remove(&new).is_none() {
                        return;
                    }
                }
                (Some(old), None) => {
                    if versions.contains_key(&old) || archived_ids.binary_search(&old).is_err() {
                        return;
                    }
                    versions.insert(old, old);
                }
                (None, None) => return,
            }
        }
        let Some(next) = state.next_ref else {
            break;
        };
        let Some(&next_ordinal) = by_node.get(&next) else {
            return;
        };
        ordinal = next_ordinal;
    }
    if visited.len() != states.len() || versions != BTreeMap::from([(0, 0)]) {
        return;
    }
    for state in states {
        state.entity_versions = projected.remove(&state.node_index).unwrap_or_default();
    }
}

/// Historical topology caches retain normalized records, topology entities,
/// incidence links, and geometry measurements while Design projection runs.
/// This conservative per-entry charge bounds that temporary cache against the
/// caller's materialization policy. Above the resulting budget the binding is
/// skipped: the states keep
/// `record_table_complete = false` and no topology, the same degrade every
/// other early return here produces, and historical transitions stay unbound.
// One live entity can retain its 16-byte version pair, one 8-byte family slot,
// one 48-byte coedge link (the largest topology link), one 56-byte curve-axis
// measurement (the largest geometry measurement), one 8-byte ownership member,
// and one 32-byte relation allocation. The remaining 24 bytes cover the parent
// vector allocation and alignment. Mutually exclusive entity families make
// this an upper bound, not an average observed size.
const HISTORY_TOPOLOGY_CACHE_BYTES_PER_ENTRY: u64 = 192;

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

fn bind_complete_record_tables(
    states: &mut [AsmDeltaState],
    bytes: &[u8],
    width: usize,
    limits: &cadmpeg_core::decode::ResourceLimits,
) -> bool {
    let Some(start) = cadmpeg_asm::asm_header::record_stream_start(bytes) else {
        return false;
    };
    let active_limit = cadmpeg_asm::asm_header::solved_record_limit(bytes).unwrap_or(bytes.len());
    let Ok(framed) = cadmpeg_asm::sab::frame(bytes, start, active_limit, width) else {
        return false;
    };
    if complete_table_binding_budget_exceeded(
        states.iter().map(|state| state.entity_versions.len()),
        limits,
    ) {
        return true;
    }
    let insert_only = insert_only_active_record_count(states);
    let archived_count = archived_active_record_count(states);
    let Some(active_count) = archived_count.or(insert_only) else {
        return false;
    };
    if insert_only.is_some() && framed.len() != active_count {
        return false;
    }
    let Some(active_records) = framed.get(..active_count) else {
        return false;
    };
    let Some(archive) = historical_record_archive(states, active_records, bytes, width) else {
        return false;
    };
    let complete = states.iter_mut().all(|state| {
        let Some(records) = materialize_record_table(state, &archive) else {
            return false;
        };
        let Some(topology) = historical_topology(
            &crate::brep::decode_history_topology(&records, bytes, crate::ids::ID_FORMAT).asm,
        ) else {
            return false;
        };
        state.record_table_complete = true;
        state.topology = Some(topology);
        true
    });
    if complete {
        bind_historical_transitions(states);
        // Drop version tables once sparse transitions exist; retaining them is
        // quadratic in states × active entities.
        for state in states {
            state.entity_versions.clear();
        }
    } else {
        for state in states {
            state.record_table_complete = false;
            state.topology = None;
        }
    }
    false
}

struct HistoricalRecordArchive {
    records: HashMap<i64, cadmpeg_asm::sab::Record>,
}

fn historical_record_archive(
    states: &[AsmDeltaState],
    active_records: &[cadmpeg_asm::sab::Record],
    bytes: &[u8],
    width: usize,
) -> Option<HistoricalRecordArchive> {
    if active_records
        .iter()
        .enumerate()
        .any(|(index, record)| record.index != index)
    {
        return None;
    }
    let active_count = i64::try_from(active_records.len()).ok()?;
    let mut revision_entities = (0..active_count)
        .map(|entity_ref| (entity_ref, entity_ref))
        .collect::<HashMap<_, _>>();
    for change in states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
    {
        let Some(old_ref) = change.old_ref else {
            continue;
        };
        let entity_ref = change.new_ref.unwrap_or(old_ref);
        if revision_entities.insert(old_ref, entity_ref).is_some() {
            return None;
        }
    }
    let mut records = active_records
        .iter()
        .cloned()
        .enumerate()
        .map(|(revision, record)| Some((i64::try_from(revision).ok()?, record)))
        .collect::<Option<HashMap<_, _>>>()?;
    for record in states
        .iter()
        .flat_map(|state| &state.records)
        // `End-of-ASM-History-Section` can be the first archived entity
        // record. The snapshot pairing, not its display name, identifies
        // records that belong in the revision archive.
        .filter(|record| record.revision_id.is_some())
    {
        let revision_id = record.revision_id?;
        let offset = usize::try_from(record.byte_offset).ok()?;
        let limit = offset.checked_add(record.raw_bytes.len())?;
        if bytes.get(offset..limit)? != record.raw_bytes {
            return None;
        }
        let mut framed = cadmpeg_asm::sab::frame(bytes, offset, limit, width).ok()?;
        if framed.len() != 1 {
            return None;
        }
        let framed = framed.pop()?;
        if framed.name != record.name || records.insert(revision_id, framed).is_some() {
            return None;
        }
    }
    if records.len() != revision_entities.len() {
        return None;
    }
    for (&revision_ref, record) in &mut records {
        record.index = usize::try_from(*revision_entities.get(&revision_ref)?).ok()?;
        for token in std::sync::Arc::make_mut(&mut record.tokens) {
            let cadmpeg_asm::sab::Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0 {
                *reference = *revision_entities.get(reference)?;
            }
        }
    }
    Some(HistoricalRecordArchive { records })
}

fn bind_historical_transitions(states: &mut [AsmDeltaState]) {
    let by_node = states
        .iter()
        .enumerate()
        .map(|(ordinal, state)| (state.node_index, ordinal))
        .collect::<HashMap<_, _>>();
    if by_node.len() != states.len() {
        return;
    }
    let transitions = states
        .iter()
        .map(|state| {
            let previous = match state.next_ref {
                Some(node) => Some(states.get(*by_node.get(&node)?)?),
                None => None,
            };
            historical_transition(state, previous)
        })
        .collect::<Option<Vec<_>>>();
    if let Some(transitions) = transitions {
        for (state, transition) in states.iter_mut().zip(transitions) {
            state.transition = Some(transition);
        }
    }
}

/// Release complete historical snapshots after every projection consumer has
/// finished. Raw history records and sparse transitions remain retained.
pub(crate) fn discard_projection_caches(histories: &mut [AsmHistory]) {
    for history in histories {
        history.projection_finalized = true;
        for state in &mut history.states {
            state.entity_versions.clear();
            state.record_table_complete = false;
            state.topology = None;
        }
    }
}

pub(crate) fn projection_was_finalized(histories: &[AsmHistory]) -> bool {
    !histories.is_empty() && histories.iter().all(|history| history.projection_finalized)
}

fn historical_transition(
    current: &AsmDeltaState,
    previous: Option<&AsmDeltaState>,
) -> Option<AsmHistoricalTransition> {
    let current_topology = current.topology.as_ref()?;
    let previous_topology = previous.and_then(|state| state.topology.as_ref());
    let current_versions = current
        .entity_versions
        .iter()
        .map(|version| (version.entity_ref, version.record_ref))
        .collect::<BTreeMap<_, _>>();
    let previous_versions = previous
        .map(|state| {
            state
                .entity_versions
                .iter()
                .map(|version| (version.entity_ref, version.record_ref))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let delta = |current: &[i64], previous: &[i64]| {
        entity_delta(current, previous, &current_versions, &previous_versions)
    };
    let empty = AsmHistoricalTopology::default();
    let previous_topology = previous_topology.unwrap_or(&empty);
    Some(AsmHistoricalTransition {
        previous_state_id: previous.map(|state| state.state_id),
        records: entity_delta(
            &current_versions.keys().copied().collect::<Vec<_>>(),
            &previous_versions.keys().copied().collect::<Vec<_>>(),
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

fn entity_delta(
    current: &[i64],
    previous: &[i64],
    current_versions: &BTreeMap<i64, i64>,
    previous_versions: &BTreeMap<i64, i64>,
) -> AsmHistoricalEntityDelta {
    let current = current.iter().copied().collect::<BTreeSet<_>>();
    let previous = previous.iter().copied().collect::<BTreeSet<_>>();
    AsmHistoricalEntityDelta {
        inserted: current.difference(&previous).copied().collect(),
        deleted: previous.difference(&current).copied().collect(),
        updated: current
            .intersection(&previous)
            .copied()
            .filter(|entity| current_versions.get(entity) != previous_versions.get(entity))
            .collect(),
    }
}

pub(crate) fn bind_feature_outputs(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::DesignParameterScope],
    histories: &[AsmHistory],
    active_bodies: &[cadmpeg_ir::topology::Body],
) {
    let mut state_outputs = HashMap::<i64, Option<Vec<i64>>>::new();
    for history in histories {
        let by_node = history
            .states
            .iter()
            .map(|state| (state.node_index, state))
            .collect::<HashMap<_, _>>();
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
            let Some(outputs) = affected_body_refs(state, previous) else {
                continue;
            };
            state_outputs
                .entry(state.state_id)
                .and_modify(|outputs| *outputs = None)
                .or_insert_with(|| Some(outputs));
        }
    }
    let active = active_bodies
        .iter()
        .filter_map(|body| stable_ref(&body.id.0).map(|slot| (slot, body.id.clone())))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|id| scopes.iter().find(|scope| scope.id == id))
        else {
            continue;
        };
        let (Some(state_id), Some(previous_state_id)) =
            (scope.history_state_id, scope.previous_history_state_id)
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
            feature.outputs = outputs
                .iter()
                .filter_map(|slot| active.get(slot).cloned())
                .collect();
            bind_base_feature_output_selection(feature);
        }
    }
}

fn bind_base_feature_output_selection(feature: &mut cadmpeg_ir::features::Feature) {
    if feature.outputs.is_empty() {
        return;
    }
    let cadmpeg_ir::features::FeatureDefinition::BaseFeature { bodies } = &mut feature.definition
    else {
        return;
    };
    let cadmpeg_ir::features::BodySelection::Native(native) = bodies else {
        return;
    };
    let native = native.clone();
    *bodies = cadmpeg_ir::features::BodySelection::Resolved {
        bodies: feature.outputs.clone(),
        native,
    };
}

pub(crate) fn bind_sweep_result_modes(
    features: &mut [cadmpeg_ir::features::Feature],
    bodies: &[cadmpeg_ir::topology::Body],
) {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, SweepMode};
    use cadmpeg_ir::topology::BodyKind;

    let body_kinds = bodies
        .iter()
        .map(|body| (body.id.clone(), body.kind))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let FeatureDefinition::Sweep { mode, .. } = &mut feature.definition else {
            continue;
        };
        if *mode != SweepMode::Unresolved || feature.outputs.is_empty() {
            continue;
        }
        let output_kinds = feature
            .outputs
            .iter()
            .map(|output| body_kinds.get(output).copied())
            .collect::<Option<Vec<_>>>();
        *mode = match output_kinds.as_deref() {
            Some(kinds) if kinds.iter().all(|kind| *kind == BodyKind::Sheet) => SweepMode::Surface,
            Some(kinds) if kinds.iter().all(|kind| *kind == BodyKind::Solid) => SweepMode::Solid {
                op: BooleanOp::NewBody,
            },
            _ => SweepMode::Unresolved,
        };
    }
}

/// Native history and neutral topology used to resolve feature body operands.
pub(crate) struct FeatureBodySelectionInputs<'a> {
    /// Decoded Design feature scopes.
    pub scopes: &'a [crate::records::DesignParameterScope],
    /// Counted Design construction-operand groups.
    pub groups: &'a [crate::records::DesignConstructionOperandGroup],
    /// Whole-body recipe operands.
    pub body_recipe_operands: &'a [crate::records::DesignBodyRecipeOperand],
    /// Construction recipes backing whole-body operands.
    pub construction_recipes: &'a [crate::records::ConstructionRecipe],
    /// Persistent body identities in the active solved B-rep.
    pub persistent_design_links: &'a [crate::records::PersistentDesignLink],
    /// Independent ASM history graphs.
    pub histories: &'a [AsmHistory],
    /// Neutral top-level bodies.
    pub bodies: &'a [cadmpeg_ir::topology::Body],
    /// Neutral body regions.
    pub regions: &'a [cadmpeg_ir::topology::Region],
    /// Neutral region shells.
    pub shells: &'a [cadmpeg_ir::topology::Shell],
}

pub(crate) fn bind_feature_body_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'_>,
) {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};

    let scopes = inputs.scopes;
    let groups = inputs.groups;
    let body_recipe_operands = inputs.body_recipe_operands;
    let histories = inputs.histories;
    let bodies = inputs.bodies;
    let regions = inputs.regions;
    let shells = inputs.shells;

    bind_pattern_body_selections(features, inputs);
    let pattern_body_slots = features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::Pattern {
                seeds,
                pattern: cadmpeg_ir::features::PatternKind::Circular { count, .. },
            } = &feature.definition
            else {
                return None;
            };
            let [cadmpeg_ir::features::PatternSeed::Bodies(BodySelection::Historical {
                bodies: seed_bodies,
                ..
            })] = seeds.as_slice()
            else {
                return None;
            };
            let [seed_body] = seed_bodies.as_slice() else {
                return None;
            };
            let expected_count = usize::try_from(*count).ok()?;
            if feature.outputs.len().checked_add(1) != Some(expected_count) {
                return None;
            }
            let slots = std::iter::once(historical_body_slot(&seed_body.0))
                .chain(feature.outputs.iter().map(|body| stable_ref(&body.0)))
                .collect::<Option<BTreeSet<_>>>()?;
            (slots.len() == expected_count).then_some((feature.id.clone(), slots))
        })
        .collect::<HashMap<_, _>>();

    let mut states = HashMap::<i64, Option<&AsmDeltaState>>::new();
    for state in histories.iter().flat_map(|history| &history.states) {
        states
            .entry(state.state_id)
            .and_modify(|state| *state = None)
            .or_insert(Some(state));
    }
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
        let feature_id = feature.id.clone();
        if matches!(feature.definition, FeatureDefinition::Pattern { .. }) {
            continue;
        }
        if let FeatureDefinition::BoundaryFill { tools, cells } = &mut feature.definition {
            if let Some(previous_state_id) = scope.previous_history_state_id {
                bind_body_recipe_body_selection(
                    tools,
                    &feature_id,
                    previous_state_id,
                    scope,
                    groups,
                    body_recipe_operands,
                );
                for cell in cells {
                    bind_body_recipe_body_selection(
                        cell,
                        &feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                }
            } else {
                bind_direct_body_recipe_body_selection(tools, scope, inputs);
                for cell in cells {
                    bind_direct_body_recipe_body_selection(cell, scope, inputs);
                }
            }
            continue;
        }
        if let FeatureDefinition::Combine { target, tools, .. } = &mut feature.definition {
            let (Some(state_id), Some(previous_state_id)) =
                (scope.history_state_id, scope.previous_history_state_id)
            else {
                bind_direct_body_recipe_body_selection(target, scope, inputs);
                bind_direct_body_recipe_body_selection(tools, scope, inputs);
                if matches!(
                    tools,
                    BodySelection::Native(_) | BodySelection::NativeSet(_)
                ) {
                    if let Some(local) = combine_external_local_tools(scope) {
                        *tools = local;
                    }
                }
                continue;
            };
            if let Some(local) = combine_external_local_tools(scope) {
                *tools = local;
            }
            let BodySelection::Native(native) = target else {
                continue;
            };
            let Some((history, state, _)) =
                unique_history_state_pair(histories, state_id, previous_state_id)
            else {
                continue;
            };
            let mut history_states = HashMap::<i64, Option<&AsmDeltaState>>::new();
            for history_state in &history.states {
                history_states
                    .entry(history_state.state_id)
                    .and_modify(|state| *state = None)
                    .or_insert(Some(history_state));
            }
            let Some(body) = singleton_revised_input_body_across_state_chain(
                state,
                previous_state_id,
                &history_states,
            ) else {
                continue;
            };
            let prefix = feature_input_prefix(&feature_id, previous_state_id);
            let input_state = crate::design::edge_resolve::feature_input_topology_id(
                &feature_id,
                previous_state_id,
            );
            *target = BodySelection::Historical {
                state: input_state.clone(),
                bodies: vec![crate::ids::history_input_body_id(&prefix, body)],
                native: native.clone(),
            };
            let Some(stream) = crate::ids::native_stream(&scope.id) else {
                continue;
            };
            let Some(operation) = scope.combine_operation.as_ref() else {
                continue;
            };
            let mut native_tools = operation
                .tools
                .iter()
                .map(|tool| format!("{stream}:design-record#{}", tool.record_index))
                .collect::<Vec<_>>();
            let current_history_source = historical_brep_source(&state.id);
            let mut historical_tool_bodies = Vec::with_capacity(native_tools.len());
            let mut direct_tool_bodies = Vec::with_capacity(native_tools.len());
            for record_index in operation.tools.iter().map(|tool| tool.record_index) {
                let mut matching = body_recipe_operands.iter().filter(|operand| {
                    crate::ids::native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == scope.record_index
                        && matches!(
                            operand.owner,
                            crate::records::DesignBodyRecipeOperandOwner::ScopeReference { .. }
                        )
                        && operand.record_index == record_index
                });
                let Some(operand) = matching.next() else {
                    historical_tool_bodies.clear();
                    direct_tool_bodies.clear();
                    break;
                };
                if matching.next().is_some() {
                    historical_tool_bodies.clear();
                    direct_tool_bodies.clear();
                    break;
                }
                if let Some(body) = operand.resolved_body_slot {
                    let body = crate::ids::history_input_body_id(&prefix, body);
                    if historical_tool_bodies.contains(&body) {
                        historical_tool_bodies.clear();
                        direct_tool_bodies.clear();
                        break;
                    }
                    historical_tool_bodies.push(body);
                    continue;
                }
                let Some(body) = unique_external_body_candidate(
                    operand,
                    current_history_source,
                    bodies,
                    regions,
                    shells,
                ) else {
                    historical_tool_bodies.clear();
                    direct_tool_bodies.clear();
                    break;
                };
                if direct_tool_bodies.contains(&body) {
                    historical_tool_bodies.clear();
                    direct_tool_bodies.clear();
                    break;
                }
                direct_tool_bodies.push(body);
            }
            if historical_tool_bodies.len() == native_tools.len() {
                *tools = BodySelection::HistoricalSet {
                    state: input_state,
                    bodies: historical_tool_bodies,
                    native: native_tools,
                };
            } else if direct_tool_bodies.len() == native_tools.len() {
                *tools = if native_tools.len() == 1 {
                    BodySelection::Resolved {
                        bodies: direct_tool_bodies,
                        native: native_tools.remove(0),
                    }
                } else {
                    BodySelection::ResolvedSet {
                        bodies: direct_tool_bodies,
                        native: native_tools,
                    }
                };
            } else {
                let tool_record_indices = operation
                    .tools
                    .iter()
                    .map(|tool| tool.record_index)
                    .collect::<Vec<_>>();
                if let Some(tool_slots) = combine_recipe_family_tool_slots(
                    stream,
                    scope.record_index,
                    &tool_record_indices,
                    previous_state_id,
                    body,
                    body_recipe_operands,
                    inputs.construction_recipes,
                ) {
                    *tools = BodySelection::HistoricalUnorderedSet {
                        state: input_state,
                        bodies: tool_slots
                            .into_iter()
                            .map(|slot| crate::ids::history_input_body_id(&prefix, slot))
                            .collect(),
                        native: native_tools,
                    };
                    continue;
                }
                let dependency_sets = feature
                    .dependencies
                    .iter()
                    .filter_map(|dependency| pattern_body_slots.get(dependency))
                    .collect::<Vec<_>>();
                if let [pattern_bodies] = dependency_sets.as_slice() {
                    if let Some(tool_slots) =
                        pattern_combine_tool_slots(pattern_bodies, body, native_tools.len())
                    {
                        *tools = BodySelection::HistoricalUnorderedSet {
                            state: input_state,
                            bodies: tool_slots
                                .into_iter()
                                .map(|slot| crate::ids::history_input_body_id(&prefix, slot))
                                .collect(),
                            native: native_tools,
                        };
                    }
                }
            }
            continue;
        }
        if let FeatureDefinition::Coil {
            result: cadmpeg_ir::features::CoilResult::Boolean { targets, .. },
            ..
        } = &mut feature.definition
        {
            if let Some(previous_state_id) = scope.previous_history_state_id {
                bind_body_recipe_body_selection(
                    targets,
                    &feature_id,
                    previous_state_id,
                    scope,
                    groups,
                    body_recipe_operands,
                );
            } else {
                bind_direct_body_recipe_body_selection(targets, scope, inputs);
            }
            continue;
        }
        if let FeatureDefinition::DeleteBody { bodies, .. } = &mut feature.definition {
            if let Some(previous_state_id) = scope.previous_history_state_id {
                bind_body_recipe_body_selection(
                    bodies,
                    &feature_id,
                    previous_state_id,
                    scope,
                    groups,
                    body_recipe_operands,
                );
            } else {
                bind_direct_body_recipe_body_selection(bodies, scope, inputs);
            }
            continue;
        }
        if let FeatureDefinition::Scale { bodies, .. } = &mut feature.definition {
            if let Some(previous_state_id) = scope.previous_history_state_id {
                bind_body_recipe_body_selection(
                    bodies,
                    &feature_id,
                    previous_state_id,
                    scope,
                    groups,
                    body_recipe_operands,
                );
                if matches!(bodies, BodySelection::Native(_)) {
                    bind_direct_body_recipe_body_selection(bodies, scope, inputs);
                }
            } else {
                bind_direct_body_recipe_body_selection(bodies, scope, inputs);
            }
            continue;
        }
        let (bodies, proof) = match &mut feature.definition {
            FeatureDefinition::MoveBody { bodies, .. } => {
                (bodies, BodySelectionProof::TopologyStableRevision)
            }
            FeatureDefinition::Shell {
                bodies: Some(bodies),
                ..
            } => (bodies, BodySelectionProof::RevisedInput),
            FeatureDefinition::SplitBody { targets, .. } => {
                (targets, BodySelectionProof::RevisedInput)
            }
            _ => continue,
        };
        let BodySelection::Native(group_id) = bodies else {
            continue;
        };
        let mut matching_groups = groups.iter().filter(|group| {
            group.id == *group_id
                && group.scope_record_index == scope.record_index
                && group.role == 0x0000_0004_0000_0000
                && crate::ids::native_stream(&group.id) == crate::ids::native_stream(&scope.id)
        });
        let Some(group) = matching_groups.next() else {
            continue;
        };
        if matching_groups.next().is_some() || group.members.len() != 1 {
            continue;
        }
        let (Some(state_id), Some(previous_state_id)) =
            (scope.history_state_id, scope.previous_history_state_id)
        else {
            bind_direct_body_recipe_body_selection(bodies, scope, inputs);
            continue;
        };
        let Some(Some(state)) = states.get(&state_id) else {
            bind_direct_body_recipe_body_selection(bodies, scope, inputs);
            continue;
        };
        let body = match proof {
            BodySelectionProof::TopologyStableRevision => {
                singleton_body_revision_across_state_chain(state, previous_state_id, &states)
            }
            BodySelectionProof::RevisedInput => {
                singleton_revised_input_body_across_state_chain(state, previous_state_id, &states)
            }
        };
        let Some(body) = body else {
            continue;
        };
        let prefix = feature_input_prefix(&feature.id, previous_state_id);
        *bodies = BodySelection::Historical {
            state: crate::design::edge_resolve::feature_input_topology_id(
                &feature.id,
                previous_state_id,
            ),
            bodies: vec![crate::ids::history_input_body_id(&prefix, body)],
            native: group_id.clone(),
        };
    }
}

fn pattern_combine_tool_slots(
    pattern_bodies: &BTreeSet<i64>,
    target_body: i64,
    native_tool_count: usize,
) -> Option<Vec<i64>> {
    let mut tool_bodies = pattern_bodies.clone();
    tool_bodies.remove(&target_body).then_some(())?;
    (tool_bodies.len() == native_tool_count).then(|| tool_bodies.into_iter().collect())
}

fn combine_recipe_family_tool_slots(
    stream: &str,
    scope_record_index: u32,
    tool_record_indices: &[u32],
    previous_state_id: i64,
    target_body: i64,
    operands: &[crate::records::DesignBodyRecipeOperand],
    recipes: &[crate::records::ConstructionRecipe],
) -> Option<Vec<i64>> {
    type FamilyKey = (String, String, u64, u32, String);
    type FamilyMember = (u32, Option<i64>, BTreeSet<i64>);

    if tool_record_indices.is_empty()
        || tool_record_indices.iter().collect::<HashSet<_>>().len() != tool_record_indices.len()
    {
        return None;
    }
    let mut recipes_by_id = HashMap::<&str, Option<&crate::records::ConstructionRecipe>>::new();
    for recipe in recipes.iter().filter(|recipe| {
        recipe.kind == crate::records::ConstructionRecipeKind::Body
            && crate::ids::native_stream(&recipe.id) == Some(stream)
    }) {
        recipes_by_id
            .entry(recipe.id.as_str())
            .and_modify(|recipe| *recipe = None)
            .or_insert(Some(recipe));
    }
    let mut families = BTreeMap::<FamilyKey, Vec<FamilyMember>>::new();
    for record_index in tool_record_indices {
        let mut matching = operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope_record_index
                && matches!(
                    operand.owner,
                    crate::records::DesignBodyRecipeOperandOwner::ScopeReference { .. }
                )
                && operand.record_index == *record_index
        });
        let operand = matching.next()?;
        if matching.next().is_some() || operand.references.len() != 1 {
            return None;
        }
        let recipe = recipes_by_id
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| *recipe)?;
        let design_id = recipe.design_id.clone()?;
        let selector = recipe.design_selector?.value;
        if selector == 0 {
            return None;
        }
        let resolved = match (operand.resolved_body_state_id, operand.resolved_body_slot) {
            (Some(state), Some(body)) if state == previous_state_id => Some(body),
            (None, None) => None,
            _ => return None,
        };
        let reference = &operand.references[0];
        let key = (
            operand.asset_id.clone(),
            operand.context_id.clone(),
            reference.design_reference,
            reference.form,
            design_id,
        );
        families.entry(key).or_default().push((
            selector,
            resolved,
            reference.preceding_body_slots.iter().copied().collect(),
        ));
    }

    let mut selected = BTreeSet::new();
    for family in families.into_values() {
        let exact = family
            .iter()
            .filter_map(|(_, body, _)| *body)
            .collect::<BTreeSet<_>>();
        if family.iter().all(|(_, body, _)| body.is_some()) {
            if exact.len() != family.len() {
                return None;
            }
            selected.extend(exact);
            continue;
        }
        let selectors = family
            .iter()
            .map(|(selector, _, _)| *selector)
            .collect::<BTreeSet<_>>();
        let expected_selectors = (1..=u32::try_from(family.len()).ok()?).collect::<BTreeSet<_>>();
        if selectors != expected_selectors {
            return None;
        }
        let mut candidate_sets = family
            .iter()
            .map(|(_, _, candidates)| candidates)
            .filter(|candidates| !candidates.is_empty());
        let candidates = candidate_sets.next()?.clone();
        if candidates.len() != family.len()
            || candidates.contains(&target_body)
            || !exact.is_subset(&candidates)
            || candidate_sets.any(|other| other != &candidates)
        {
            return None;
        }
        selected.extend(candidates);
    }
    (selected.len() == tool_record_indices.len() && !selected.contains(&target_body))
        .then(|| selected.into_iter().collect())
}

fn combine_external_local_tools(
    scope: &crate::records::DesignParameterScope,
) -> Option<cadmpeg_ir::features::BodySelection> {
    let operation = scope.combine_operation.as_ref()?;
    let bodies = operation
        .tools
        .iter()
        .map(|tool| {
            tool.external_identity
                .as_ref()
                .map(crate::ids::neutral_combine_external_body_id)
        })
        .collect::<Option<Vec<_>>>()?;
    if bodies.is_empty() || bodies.iter().collect::<HashSet<_>>().len() != bodies.len() {
        return None;
    }
    Some(cadmpeg_ir::features::BodySelection::Local {
        bodies,
        native: scope.id.clone(),
    })
}

fn historical_body_slot(id: &str) -> Option<i64> {
    id.strip_prefix("f3d:history-input:body#")?
        .rsplit_once(':')?
        .1
        .parse()
        .ok()
}

fn bind_pattern_body_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'_>,
) {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, PatternSeed};

    let scopes = inputs.scopes;
    let groups = inputs.groups;
    let body_recipe_operands = inputs.body_recipe_operands;

    for feature in features {
        let FeatureDefinition::Pattern { seeds, .. } = &mut feature.definition else {
            continue;
        };
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let matching_scopes = scopes
            .iter()
            .filter(|scope| scope.id == native_ref)
            .collect::<Vec<_>>();
        let [scope] = matching_scopes.as_slice() else {
            continue;
        };
        let stream = crate::ids::native_stream(&scope.id);
        let matching_groups = groups
            .iter()
            .filter(|group| {
                group.scope_record_index == scope.record_index
                    && group.role == 0x0000_0008_0000_0000
                    && !group.members.is_empty()
                    && crate::ids::native_stream(&group.id) == stream
            })
            .collect::<Vec<_>>();
        let [group] = matching_groups.as_slice() else {
            continue;
        };
        let selection = if let [PatternSeed::Bodies(selection)] = seeds.as_mut_slice() {
            selection
        } else if seeds.is_empty() {
            seeds.push(PatternSeed::Bodies(BodySelection::Native(group.id.clone())));
            let [PatternSeed::Bodies(selection)] = seeds.as_mut_slice() else {
                unreachable!("the inserted pattern seed is a body selection")
            };
            selection
        } else {
            continue;
        };
        if let Some(previous_state_id) = scope.previous_history_state_id {
            bind_body_recipe_body_selection(
                selection,
                &feature.id,
                previous_state_id,
                scope,
                groups,
                body_recipe_operands,
            );
        } else {
            bind_direct_body_recipe_body_selection(selection, scope, inputs);
        }
    }
}

fn unique_external_body_candidate(
    operand: &crate::records::DesignBodyRecipeOperand,
    current_history_source: Option<&str>,
    bodies: &[cadmpeg_ir::topology::Body],
    regions: &[cadmpeg_ir::topology::Region],
    shells: &[cadmpeg_ir::topology::Shell],
) -> Option<cadmpeg_ir::ids::BodyId> {
    let body_by_region = regions
        .iter()
        .map(|region| (&region.id, &region.body))
        .collect::<HashMap<_, _>>();
    let body_by_face = shells
        .iter()
        .filter_map(|shell| {
            let body = body_by_region.get(&shell.region)?;
            Some(shell.faces.iter().map(move |face| (face, *body)))
        })
        .flatten()
        .collect::<HashMap<_, _>>();
    let body_metadata = bodies
        .iter()
        .map(|body| (&body.id, body))
        .collect::<HashMap<_, _>>();
    let current_prefix = current_history_source.map(|source| format!("f3d:brep/{source}/"));
    let mut reference_candidates = operand.references.iter().map(|reference| {
        reference
            .candidate_faces
            .iter()
            .filter_map(|face| body_by_face.get(face).copied())
            .filter(|body| {
                current_prefix
                    .as_ref()
                    .is_none_or(|prefix| !body.0.starts_with(prefix))
            })
            .cloned()
            .collect::<BTreeSet<_>>()
    });
    let mut candidates = reference_candidates.next()?;
    for reference in reference_candidates {
        if reference.is_empty() {
            return None;
        }
        candidates.retain(|body| reference.contains(body));
    }
    let displayed = candidates
        .iter()
        .filter(|body| {
            body_metadata
                .get(body)
                .is_some_and(|body| body.visible == Some(true))
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    if !displayed.is_empty() {
        candidates = displayed;
    }
    if candidates.len() == 1 {
        candidates.into_iter().next()
    } else {
        None
    }
}

fn bind_body_recipe_body_selection(
    selection: &mut cadmpeg_ir::features::BodySelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::DesignParameterScope,
    groups: &[crate::records::DesignConstructionOperandGroup],
    operands: &[crate::records::DesignBodyRecipeOperand],
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
                group.role,
                0x0000_0004_0000_0000 | 0x0000_0005_0000_0000 | 0x0000_0008_0000_0000
            )
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some() || group.members.is_empty() {
        return;
    }
    let mut body_slots = Vec::with_capacity(group.members.len());
    for (ordinal, record_index) in group.members.iter().copied().enumerate() {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return;
        };
        let mut matching_operands = operands.iter().filter(|operand| {
            operand.owner.group() == Some((group.record_index, ordinal))
                && operand.record_index == record_index
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
    let prefix = feature_input_prefix(feature_id, previous_state_id);
    *selection = BodySelection::Historical {
        state: crate::design::edge_resolve::feature_input_topology_id(
            feature_id,
            previous_state_id,
        ),
        bodies: body_slots
            .into_iter()
            .map(|slot| crate::ids::history_input_body_id(&prefix, slot))
            .collect(),
        native: group_id.clone(),
    };
}

fn bind_direct_body_recipe_body_selection(
    selection: &mut cadmpeg_ir::features::BodySelection,
    scope: &crate::records::DesignParameterScope,
    inputs: &FeatureBodySelectionInputs<'_>,
) {
    use cadmpeg_ir::features::BodySelection;

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
                        group.role,
                        0x0000_0004_0000_0000 | 0x0000_0005_0000_0000 | 0x0000_0008_0000_0000
                    )
                    && crate::ids::native_stream(&group.id) == stream
            });
            let Some(group) = matching_groups.next() else {
                return;
            };
            if matching_groups.next().is_some() || group.members.is_empty() {
                return;
            }
            let mut selected = Vec::with_capacity(group.members.len());
            for (ordinal, record_index) in group.members.iter().copied().enumerate() {
                let Ok(ordinal) = u32::try_from(ordinal) else {
                    return;
                };
                let mut matching_operands = operands.iter().filter(|operand| {
                    operand.owner.group() == Some((group.record_index, ordinal))
                        && operand.record_index == record_index
                        && crate::ids::native_stream(&operand.id) == stream
                });
                let Some(operand) = matching_operands.next() else {
                    return;
                };
                if matching_operands.next().is_some() {
                    return;
                }
                let Some(body) = direct_body_recipe_candidate(
                    operand,
                    construction_recipes,
                    persistent_design_links,
                    bodies,
                    regions,
                    shells,
                ) else {
                    return;
                };
                if selected.contains(&body) {
                    return;
                }
                selected.push(body);
            }
            *selection = BodySelection::Resolved {
                bodies: selected,
                native: group.id.clone(),
            };
            return;
        }
        BodySelection::NativeSet(native) => native.clone(),
        _ => return,
    };
    if native_members.is_empty()
        || native_members
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != native_members.len()
    {
        return;
    }
    let mut selected = Vec::with_capacity(native_members.len());
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
                    crate::records::DesignBodyRecipeOperandOwner::ScopeReference { .. }
                )
                && operand.record_index == record_index
        });
        let Some(operand) = matching_operands.next() else {
            return;
        };
        if matching_operands.next().is_some() {
            return;
        }
        let Some(body) = direct_body_recipe_candidate(
            operand,
            construction_recipes,
            persistent_design_links,
            bodies,
            regions,
            shells,
        ) else {
            return;
        };
        if selected.contains(&body) {
            return;
        }
        selected.push(body);
    }
    *selection = BodySelection::ResolvedSet {
        bodies: selected,
        native: native_members,
    };
}

fn direct_body_recipe_candidate(
    operand: &crate::records::DesignBodyRecipeOperand,
    construction_recipes: &[crate::records::ConstructionRecipe],
    persistent_design_links: &[crate::records::PersistentDesignLink],
    bodies: &[cadmpeg_ir::topology::Body],
    regions: &[cadmpeg_ir::topology::Region],
    shells: &[cadmpeg_ir::topology::Shell],
) -> Option<cadmpeg_ir::ids::BodyId> {
    if let Some(body) = body_recipe_link_candidate(
        operand,
        construction_recipes,
        persistent_design_links,
        bodies,
    ) {
        let candidate_bodies = body_recipe_face_body_candidates(operand, bodies, regions, shells);
        if candidate_bodies.is_empty() || candidate_bodies.contains(&body) {
            return Some(body);
        }
        return None;
    }
    unique_external_body_candidate(operand, None, bodies, regions, shells)
}

fn body_recipe_link_candidate(
    operand: &crate::records::DesignBodyRecipeOperand,
    construction_recipes: &[crate::records::ConstructionRecipe],
    persistent_design_links: &[crate::records::PersistentDesignLink],
    bodies: &[cadmpeg_ir::topology::Body],
) -> Option<cadmpeg_ir::ids::BodyId> {
    let stream = crate::ids::native_stream(&operand.id)?;
    let mut matching_recipes = construction_recipes.iter().filter(|recipe| {
        recipe.id == operand.recipe_id
            && recipe.kind == crate::records::ConstructionRecipeKind::Body
            && crate::ids::native_stream(&recipe.id) == Some(stream)
    });
    let recipe = matching_recipes.next()?;
    if matching_recipes.next().is_some() {
        return None;
    }
    let design_id = recipe.design_id.as_deref()?;
    let selector = i64::from(recipe.design_selector?.value);
    let mut matching_bodies = Vec::new();
    for link in persistent_design_links.iter().filter(|link| {
        link.entity_kind == 3
            && link.is_current
            && link.design_id == design_id
            && link.design_reference == selector
    }) {
        let cadmpeg_ir::attributes::AttributeTarget::Body(body) = &link.target else {
            continue;
        };
        if bodies.iter().any(|candidate| candidate.id == *body) && !matching_bodies.contains(body) {
            matching_bodies.push(body.clone());
        }
    }
    let [body] = matching_bodies.as_slice() else {
        return None;
    };
    Some(body.clone())
}

fn body_recipe_face_body_candidates(
    operand: &crate::records::DesignBodyRecipeOperand,
    bodies: &[cadmpeg_ir::topology::Body],
    regions: &[cadmpeg_ir::topology::Region],
    shells: &[cadmpeg_ir::topology::Shell],
) -> Vec<cadmpeg_ir::ids::BodyId> {
    let body_by_region = regions
        .iter()
        .map(|region| (&region.id, &region.body))
        .collect::<std::collections::HashMap<_, _>>();
    let body_by_face = shells
        .iter()
        .filter_map(|shell| {
            let body = body_by_region.get(&shell.region)?;
            Some(shell.faces.iter().map(move |face| (face, *body)))
        })
        .flatten()
        .collect::<std::collections::HashMap<_, _>>();
    let mut candidates = Vec::new();
    for face in operand
        .references
        .iter()
        .flat_map(|reference| &reference.candidate_faces)
    {
        let Some(body) = body_by_face.get(face).copied() else {
            continue;
        };
        if bodies.iter().any(|candidate| candidate.id == *body) && !candidates.contains(body) {
            candidates.push(body.clone());
        }
    }
    candidates
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodySelectionProof {
    TopologyStableRevision,
    RevisedInput,
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
        revised.extend(
            transition
                .topology
                .bodies
                .updated
                .iter()
                .chain(&transition.topology.bodies.deleted)
                .copied(),
        );
        let previous_id = transition.previous_state_id?;
        current = *states.get(&previous_id)?.as_ref()?;
    }
    let input = current.topology.as_ref()?;
    let mut candidates = input.bodies.iter().filter(|body| revised.contains(body));
    let body = *candidates.next()?;
    candidates.next().is_none().then_some(body)
}

fn singleton_body_revision_across_state_chain<'a>(
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Option<i64> {
    let result_topology = state.topology.as_ref()?;
    let mut current = state;
    let mut visited = HashSet::new();
    let mut selected = None;
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        if let TopologyStableBodyRevision::Revised(body) =
            body_revision_without_topology_change(current)?
        {
            match selected {
                None => selected = Some(body),
                Some(selected) if selected == body => {}
                Some(_) => return None,
            }
        }
        let previous = current.transition.as_ref()?.previous_state_id?;
        current = *states.get(&previous)?.as_ref()?;
    }
    let body = selected?;
    (result_topology.bodies.contains(&body) && current.topology.as_ref()?.bodies.contains(&body))
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn bind_feature_face_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    input_topologies: &mut [cadmpeg_ir::features::FeatureInputTopology],
    scopes: &[crate::records::DesignParameterScope],
    groups: &[crate::records::DesignConstructionOperandGroup],
    operands: &[crate::records::DesignFaceOperand],
    entity_operands: &[crate::records::DesignEntitySelectionOperand],
    body_recipe_operands: &[crate::records::DesignBodyRecipeOperand],
    histories: &[AsmHistory],
) {
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
        let Some(state_id) = scope.history_state_id else {
            continue;
        };
        let Some(previous_state_id) = effective_scope_previous_history_state_id(scope, histories)
        else {
            continue;
        };
        let Some((history, state, previous)) =
            unique_history_state_pair(histories, state_id, previous_state_id)
        else {
            continue;
        };
        let Some(transition) = &state.transition else {
            continue;
        };
        if transition.previous_state_id != Some(previous_state_id) {
            continue;
        }
        let Some(_topology) = &previous.topology else {
            continue;
        };
        let feature_id = feature.id.clone();
        match &mut feature.definition {
            cadmpeg_ir::features::FeatureDefinition::Extrude { start, extent, .. } => {
                if let cadmpeg_ir::features::ExtrudeStart::FromFace { face, .. } = start {
                    bind_face_selection(face, scope, groups, operands);
                    bind_entity_face_selection(
                        face,
                        &feature_id,
                        previous_state_id,
                        &history.id,
                        scope,
                        groups,
                        entity_operands,
                        input_topologies,
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
                    if let cadmpeg_ir::features::Termination::ToFace { face, .. } =
                        &mut side.termination
                    {
                        bind_face_selection(face, scope, groups, operands);
                    }
                }
            }
            cadmpeg_ir::features::FeatureDefinition::Pattern { seeds, .. } => {
                for seed in seeds {
                    let cadmpeg_ir::features::PatternSeed::Faces(faces) = seed else {
                        continue;
                    };
                    bind_face_selection(faces, scope, groups, operands);
                    bind_entity_face_selection(
                        faces,
                        &feature_id,
                        previous_state_id,
                        &history.id,
                        scope,
                        groups,
                        entity_operands,
                        input_topologies,
                    );
                }
            }
            cadmpeg_ir::features::FeatureDefinition::MoveFace { faces, .. } => {
                bind_face_selection(faces, scope, groups, operands);
            }
            cadmpeg_ir::features::FeatureDefinition::Thicken { faces, .. } => {
                bind_face_selection(faces, scope, groups, operands);
                bind_body_recipe_face_selection(
                    faces,
                    &feature_id,
                    previous_state_id,
                    scope,
                    groups,
                    body_recipe_operands,
                );
            }
            cadmpeg_ir::features::FeatureDefinition::SplitFace { targets, .. } => {
                bind_face_selection(targets, scope, groups, operands);
            }
            cadmpeg_ir::features::FeatureDefinition::Hole {
                face: Some(face), ..
            } => {
                bind_hole_face_selection(
                    face,
                    &feature_id,
                    previous_state_id,
                    &history.id,
                    scope,
                    input_topologies,
                );
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn bind_entity_face_selection(
    selection: &mut cadmpeg_ir::features::FaceSelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    operation_history_id: &str,
    scope: &crate::records::DesignParameterScope,
    groups: &[crate::records::DesignConstructionOperandGroup],
    operands: &[crate::records::DesignEntitySelectionOperand],
    input_topologies: &mut [cadmpeg_ir::features::FeatureInputTopology],
) {
    use cadmpeg_ir::features::FaceSelection;

    let FaceSelection::Native(group_id) = selection else {
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
    if matching_groups.next().is_some() || group.members.is_empty() {
        return;
    }
    let mut selected = Vec::<(&str, i64, bool)>::new();
    for (ordinal, record_index) in group.members.iter().copied().enumerate() {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return;
        };
        let mut matches = operands.iter().filter(|operand| {
            operand.scope_record_index == scope.record_index
                && operand.group_record_index == group.record_index
                && operand.group_member_ordinal == ordinal
                && operand.record_index == record_index
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
            && candidate.historical_state_ids.contains(&previous_state_id);
        let Some(source) = historical_brep_source(&candidate.history_id) else {
            return;
        };
        if !selected.contains(&(source, candidate.face_slot, local)) {
            selected.push((source, candidate.face_slot, local));
        }
    }
    let state_id =
        crate::design::edge_resolve::feature_input_topology_id(feature_id, previous_state_id);
    let mut topologies = input_topologies
        .iter_mut()
        .filter(|topology| topology.id == state_id && topology.input_of == *feature_id);
    let Some(topology) = topologies.next() else {
        return;
    };
    if topologies.next().is_some() {
        return;
    }
    let prefix = feature_input_prefix(feature_id, previous_state_id);
    let faces = selected
        .into_iter()
        .map(|(source, face, local)| {
            let discriminator = if local {
                face.to_string()
            } else {
                format!("{}:{source}:{face}", source.len())
            };
            crate::ids::history_input_face_id(&prefix, discriminator)
        })
        .collect::<Vec<_>>();
    for face in &faces {
        if !topology.faces.contains(face) {
            topology.faces.push(face.clone());
        }
    }
    *selection = FaceSelection::Historical {
        state: state_id,
        faces,
        native: group.id.clone(),
    };
}

#[allow(clippy::too_many_arguments)]
fn bind_hole_face_selection(
    selection: &mut cadmpeg_ir::features::FaceSelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    operation_history_id: &str,
    scope: &crate::records::DesignParameterScope,
    input_topologies: &mut [cadmpeg_ir::features::FeatureInputTopology],
) {
    use cadmpeg_ir::features::FaceSelection;

    let FaceSelection::Native(native_id) = selection else {
        return;
    };
    let Some(construction) = &scope.hole_construction else {
        return;
    };
    let Some(face_selection) = &construction.face_selection else {
        return;
    };
    let [candidate] = face_selection.historical_face_candidates.as_slice() else {
        return;
    };
    let local = candidate.history_id == operation_history_id
        && candidate.historical_state_ids.contains(&previous_state_id);
    let Some(source) = historical_brep_source(&candidate.history_id) else {
        return;
    };
    let state_id =
        crate::design::edge_resolve::feature_input_topology_id(feature_id, previous_state_id);
    let mut topologies = input_topologies
        .iter_mut()
        .filter(|topology| topology.id == state_id && topology.input_of == *feature_id);
    let Some(topology) = topologies.next() else {
        return;
    };
    if topologies.next().is_some() {
        return;
    }
    let prefix = feature_input_prefix(feature_id, previous_state_id);
    let discriminator = if local {
        candidate.face_slot.to_string()
    } else {
        format!("{}:{source}:{}", source.len(), candidate.face_slot)
    };
    let face = crate::ids::history_input_face_id(&prefix, discriminator);
    if !topology.faces.contains(&face) {
        topology.faces.push(face.clone());
    }
    *selection = FaceSelection::Historical {
        state: state_id,
        faces: vec![face],
        native: native_id.clone(),
    };
}

pub(crate) fn bind_feature_path_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::DesignParameterScope],
    groups: &[crate::records::DesignConstructionOperandGroup],
    operands: &[crate::records::DesignEntitySelectionOperand],
) {
    use cadmpeg_ir::features::{FeatureDefinition, SurfaceBoundary};

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
        let Some(previous_state_id) = scope.previous_history_state_id else {
            continue;
        };
        let feature_id = feature.id.clone();
        match &mut feature.definition {
            FeatureDefinition::FilledSurface {
                boundary: SurfaceBoundary::Path(path),
                ..
            } => bind_entity_selection_path(
                path,
                &feature_id,
                previous_state_id,
                scope,
                groups,
                operands,
            ),
            FeatureDefinition::Loft { guides, .. } => {
                for path in guides {
                    bind_entity_selection_path(
                        path,
                        &feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        operands,
                    );
                }
            }
            FeatureDefinition::Sweep {
                path, guide_rail, ..
            } => {
                if let Some(path) = path {
                    bind_entity_selection_path(
                        path,
                        &feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        operands,
                    );
                }
                if let Some(guide_rail) = guide_rail {
                    bind_entity_selection_path(
                        &mut guide_rail.path,
                        &feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        operands,
                    );
                }
            }
            _ => {}
        }
    }
}

fn bind_entity_selection_path(
    path: &mut cadmpeg_ir::features::PathRef,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::DesignParameterScope,
    groups: &[crate::records::DesignConstructionOperandGroup],
    operands: &[crate::records::DesignEntitySelectionOperand],
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
    if matching_groups.next().is_some() || group.members.is_empty() {
        return;
    }
    let mut edge_slots = Vec::with_capacity(group.members.len());
    for (ordinal, record_index) in group.members.iter().copied().enumerate() {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return;
        };
        let mut matching_operands = operands.iter().filter(|operand| {
            operand.group_record_index == group.record_index
                && operand.group_member_ordinal == ordinal
                && operand.record_index == record_index
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
    let prefix = feature_input_prefix(feature_id, previous_state_id);
    *path = PathRef::HistoricalEdges {
        state: crate::design::edge_resolve::feature_input_topology_id(
            feature_id,
            previous_state_id,
        ),
        edges: edge_slots
            .into_iter()
            .map(|slot| crate::ids::history_input_edge_id(&prefix, slot))
            .collect(),
        native: group_id.clone(),
    };
}

pub(crate) fn project_feature_input_topologies(
    features: &[cadmpeg_ir::features::Feature],
    scopes: &[crate::records::DesignParameterScope],
    histories: &[AsmHistory],
    edge_operands: &[crate::records::DesignEdgeOperand],
) -> Vec<cadmpeg_ir::features::FeatureInputTopology> {
    use cadmpeg_ir::features::FeatureInputTopology;

    features
        .iter()
        .filter_map(|feature| {
            let native_ref = feature.native_ref.as_deref()?;
            let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
            let scope = matching_scopes.next()?;
            if matching_scopes.next().is_some() {
                return None;
            }
            let previous_state_id = scope
                .previous_history_state_id
                .or_else(|| {
                    crate::design::feature_project::work_point_recipe_state_id(scope, edge_operands)
                })
                .or_else(|| crate::design::feature_project::work_plane_recipe_state_id(scope))
                .or_else(|| effective_scope_previous_history_state_id(scope, histories))?;
            let state = scope
                .history_state_id
                .and_then(|state_id| {
                    unique_history_state_pair(histories, state_id, previous_state_id)
                        .map(|(_, _, previous)| previous)
                })
                .or_else(|| {
                    unique_history_state(histories, previous_state_id).map(|(_, state)| state)
                })?;
            let topology = state.topology.as_ref()?;
            let prefix = feature_input_prefix(&feature.id, previous_state_id);
            Some(FeatureInputTopology {
                id: crate::design::edge_resolve::feature_input_topology_id(
                    &feature.id,
                    previous_state_id,
                ),
                input_of: feature.id.clone(),
                bodies: topology
                    .bodies
                    .iter()
                    .map(|slot| crate::ids::history_input_body_id(&prefix, slot))
                    .collect(),
                faces: topology
                    .faces
                    .iter()
                    .map(|slot| crate::ids::history_input_face_id(&prefix, slot))
                    .collect(),
                edges: topology
                    .edges
                    .iter()
                    .map(|slot| crate::ids::history_input_edge_id(&prefix, slot))
                    .collect(),
                vertices: topology
                    .vertices
                    .iter()
                    .map(|slot| crate::ids::history_input_vertex_id(&prefix, slot))
                    .collect(),
                native_ref: Some(state.id.clone()),
            })
        })
        .collect()
}

/// Resolve persistent vertex recipes in the last history-bearing feature state
/// that precedes their owning construction in authored timeline order.
pub(crate) fn bind_vertex_recipe_history(
    scopes: &mut [crate::records::DesignParameterScope],
    timelines: &[crate::records::DesignFeatureTimeline],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    let source_ordinals =
        crate::design::feature_project::authored_scope_ordinals_per_stream(scopes, timelines)?;
    let input_states = scopes
        .iter()
        .filter(|scope| matches!(scope.kind.as_str(), "WorkPlane" | "WorkPoint"))
        .filter_map(|scope| {
            let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
            let ordinal = *source_ordinals.get(&(stream, scope.record_index))?;
            let mut predecessors = scopes.iter().filter_map(|candidate| {
                let candidate_stream =
                    crate::ids::native_stream(&candidate.id).unwrap_or(crate::ids::DEFAULT_STREAM);
                let candidate_ordinal =
                    *source_ordinals.get(&(candidate_stream, candidate.record_index))?;
                (candidate_stream == stream && candidate_ordinal < ordinal)
                    .then_some((candidate_ordinal, candidate.history_state_id?))
            });
            let predecessor = predecessors.next()?;
            let predecessor = predecessors.fold(predecessor, |latest, candidate| {
                if candidate.0 > latest.0 {
                    candidate
                } else {
                    latest
                }
            });
            Some((scope.id.clone(), predecessor.1))
        })
        .collect::<HashMap<_, _>>();

    for scope in scopes.iter_mut().filter(|scope| scope.kind == "WorkPoint") {
        let Some(construction) = &mut scope.work_point_construction else {
            continue;
        };
        let solved_position = cadmpeg_ir::math::Point3::new(
            construction.position[0] * 10.0,
            construction.position[1] * 10.0,
            construction.position[2] * 10.0,
        );
        for input in construction.rule.inputs_mut() {
            let Some(crate::records::DesignWorkPointInputCarrier::VertexRecipe { recipe }) =
                input.carrier.as_deref_mut()
            else {
                continue;
            };
            recipe.recipe_state_id = None;
            recipe.resolved_vertex_slot = None;
            let Some(state_id) = input_states.get(&scope.id).copied() else {
                continue;
            };
            let Some((_, state)) = unique_history_state(histories, state_id) else {
                continue;
            };
            let Some(topology) = state.topology.as_ref() else {
                continue;
            };
            let Some((vertex, position)) = vertex_recipe_candidate(recipe, topology) else {
                continue;
            };
            if !point_matches(position, solved_position) {
                continue;
            }
            recipe.recipe_state_id = Some(state_id);
            recipe.resolved_vertex_slot = Some(vertex);
        }
    }

    for scope in scopes.iter_mut().filter(|scope| scope.kind == "WorkPlane") {
        let Some(crate::records::DesignWorkPlaneConstruction::ThreePoint { inputs, .. }) =
            &mut scope.work_plane_construction
        else {
            continue;
        };
        for recipe in inputs.iter_mut() {
            recipe.recipe_state_id = None;
            recipe.resolved_vertex_slot = None;
        }
        let Some(state_id) = input_states.get(&scope.id).copied() else {
            continue;
        };
        let Some((_, state)) = unique_history_state(histories, state_id) else {
            continue;
        };
        let Some(topology) = state.topology.as_ref() else {
            continue;
        };
        let candidates = inputs
            .iter()
            .map(|recipe| vertex_recipe_candidate(recipe, topology))
            .collect::<Option<Vec<_>>>();
        let Some(candidates) = candidates else {
            continue;
        };
        let Ok(candidates): Result<[(i64, cadmpeg_ir::math::Point3); 3], _> = candidates.try_into()
        else {
            continue;
        };
        let [first, second, third] = candidates;
        if first.0 == second.0
            || first.0 == third.0
            || second.0 == third.0
            || !three_point_plane_matches(scope.work_plane_transform, [first.1, second.1, third.1])
        {
            continue;
        }
        for (recipe, (vertex, _)) in inputs.iter_mut().zip(candidates) {
            recipe.recipe_state_id = Some(state_id);
            recipe.resolved_vertex_slot = Some(vertex);
        }
    }
    Ok(())
}

fn vertex_recipe_candidate(
    recipe: &crate::records::DesignVertexRecipe,
    topology: &AsmHistoricalTopology,
) -> Option<(i64, cadmpeg_ir::math::Point3)> {
    let face_slots = recipe
        .recipe_references
        .iter()
        .map(|reference| {
            let mut slots = reference
                .candidate_faces
                .iter()
                .filter_map(|face| stable_ref(&face.0))
                .filter(|face| topology.faces.contains(face))
                .collect::<Vec<_>>();
            slots.sort_unstable();
            slots.dedup();
            let [slot] = slots.as_slice() else {
                return None;
            };
            Some(*slot)
        })
        .collect::<Option<Vec<_>>>()?;
    if face_slots.is_empty() {
        return None;
    }
    let vertex = common_face_vertex(&face_slots, topology)?;
    let position = unique_historical_vertex_position(vertex, topology)?;
    Some((vertex, position))
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

fn common_face_vertex(face_slots: &[i64], topology: &AsmHistoricalTopology) -> Option<i64> {
    let boundary_edges = face_boundary_edge_index(topology);
    let mut sets = face_slots.iter().map(|face| {
        boundary_edges
            .get(face)?
            .iter()
            .map(|edge_slot| {
                let mut edges = topology
                    .edge_vertices
                    .iter()
                    .filter(|candidate| candidate.edge == *edge_slot);
                let edge = edges.next()?;
                (edges.next().is_none()
                    && topology.vertices.contains(&edge.start_vertex)
                    && topology.vertices.contains(&edge.end_vertex))
                .then_some([edge.start_vertex, edge.end_vertex])
            })
            .collect::<Option<Vec<_>>>()
            .map(|endpoints| endpoints.into_iter().flatten().collect::<HashSet<_>>())
            .filter(|vertices| !vertices.is_empty())
    });
    let mut common = sets.next()??;
    for vertices in sets {
        let vertices = vertices?;
        common.retain(|vertex| vertices.contains(vertex));
    }
    let mut common = common.into_iter().collect::<Vec<_>>();
    common.sort_unstable();
    let [vertex] = common.as_slice() else {
        return None;
    };
    Some(*vertex)
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
    (positions.next().is_none()
        && position.x.is_finite()
        && position.y.is_finite()
        && position.z.is_finite())
    .then_some(position)
}

fn point_matches(left: cadmpeg_ir::math::Point3, right: cadmpeg_ir::math::Point3) -> bool {
    [(left.x, right.x), (left.y, right.y), (left.z, right.z)]
        .into_iter()
        .all(|(left, right)| {
            let scale = left.abs().max(right.abs()).max(1.0);
            (left - right).abs() <= WORK_POINT_POSITION_TOLERANCE * scale
        })
}

fn feature_input_prefix(
    feature: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
) -> String {
    let feature_key = feature
        .0
        .split_once('#')
        .map_or(feature.0.as_str(), |(_, key)| key);
    crate::ids::history_input_prefix(feature_key, previous_state_id)
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
    scope: &crate::records::DesignParameterScope,
    histories: &[AsmHistory],
) -> Option<i64> {
    scope.previous_history_state_id.or_else(|| {
        let state_id = scope.history_state_id?;
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
    pub direction: Option<cadmpeg_ir::features::SheetMetalHemDirection>,
    pub gap_length_form: Option<HemGapLengthForm>,
}

/// Resolve Hem semantics from the bend carriers in the owning history
/// transition. The source operation's fixed fields do not carry these
/// meanings; the selected edge and the inserted coaxial cylinders do.
pub(crate) fn hem_geometry_semantics(
    scope: &crate::records::DesignParameterScope,
    edge_slot: i64,
    histories: &[AsmHistory],
) -> HemGeometrySemantics {
    let unresolved = HemGeometrySemantics {
        direction: None,
        gap_length_form: None,
    };
    let (Some(state_id), Some(previous_state_id)) = (
        scope.history_state_id,
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
        (previous.topology.as_ref(), state.transition.as_ref())
    else {
        return unresolved;
    };
    let Some(cylinders) = hem_inserted_cylinders(state, previous_topology, transition, edge_slot)
    else {
        return unresolved;
    };
    HemGeometrySemantics {
        direction: hem_direction_from_transition(
            edge_slot,
            &cylinders,
            previous_topology,
            transition,
        ),
        gap_length_form: hem_gap_length_form(&cylinders),
    }
}

fn hem_inserted_cylinders<'a>(
    state: &'a AsmDeltaState,
    previous: &AsmHistoricalTopology,
    transition: &AsmHistoricalTransition,
    edge_slot: i64,
) -> Option<Vec<&'a AsmHistoricalCylinder>> {
    let topology = state.topology.as_ref()?;
    let inserted_surfaces = &transition.topology.surfaces.inserted;
    let cylinders = topology
        .surface_cylinders
        .iter()
        .filter(|cylinder| inserted_surfaces.contains(&cylinder.surface))
        .collect::<Vec<_>>();
    if cylinders.is_empty() {
        return Some(Vec::new());
    }
    let edge_direction = historical_edge_axis(edge_slot, previous).map(|(_, direction)| direction);
    Some(
        cylinders
            .into_iter()
            .filter(|cylinder| {
                edge_direction.is_none_or(|direction| parallel_directions(direction, cylinder.axis))
            })
            .collect(),
    )
}

fn hem_gap_length_form(cylinders: &[&AsmHistoricalCylinder]) -> Option<HemGapLengthForm> {
    let [first, second] = cylinders else {
        return None;
    };
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
    let tolerance = 1.0e-7 * (1.0 + outer.abs() + inner.abs());
    if (2.0 * inner - thickness).abs() <= tolerance {
        Some(HemGapLengthForm::Open)
    } else if 2.0 * inner < thickness - tolerance {
        Some(HemGapLengthForm::Flat)
    } else {
        None
    }
}

fn hem_direction_from_transition(
    edge_slot: i64,
    cylinders: &[&AsmHistoricalCylinder],
    previous: &AsmHistoricalTopology,
    transition: &AsmHistoricalTransition,
) -> Option<cadmpeg_ir::features::SheetMetalHemDirection> {
    if cylinders.is_empty() {
        return None;
    }
    let incident_faces = historical_edge_context(edge_slot, previous)
        .incident_loops
        .into_iter()
        .map(|context| context.face_slot)
        .collect::<BTreeSet<_>>();
    let mut candidates = Vec::new();
    for face in incident_faces {
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
        let offsets = cylinders
            .iter()
            .map(|cylinder| normal.dot(cylinder.origin.vector_from(plane.origin)))
            .collect::<Vec<_>>();
        let Some(first) = offsets.first().copied() else {
            continue;
        };
        if !first.is_finite() {
            continue;
        }
        let sign_tolerance = 1.0e-7
            * (1.0
                + plane.origin.x.abs()
                + plane.origin.y.abs()
                + plane.origin.z.abs()
                + cylinders.iter().fold(0.0_f64, |scale, cylinder| {
                    scale
                        .max(cylinder.origin.x.abs())
                        .max(cylinder.origin.y.abs())
                        .max(cylinder.origin.z.abs())
                }));
        if first.abs() <= sign_tolerance
            || offsets.iter().any(|offset| {
                !offset.is_finite()
                    || offset.abs() <= sign_tolerance
                    || offset.is_sign_positive() != first.is_sign_positive()
            })
        {
            continue;
        }
        candidates.push(first.is_sign_positive());
    }
    let [positive] = candidates.as_slice() else {
        return None;
    };
    Some(if *positive {
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
    dot.is_finite() && (dot.abs() - 1.0).abs() <= 1.0e-7
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

pub(crate) fn bind_scope_histories(
    scopes: &[crate::records::DesignParameterScope],
    body_bindings: &[crate::records::DesignBodyBinding],
    body_recipe_operands: &[crate::records::DesignBodyRecipeOperand],
    histories: &[AsmHistory],
) -> HashMap<String, String> {
    let candidates = scopes
        .iter()
        .filter_map(|scope| {
            let state_id = scope.history_state_id?;
            let candidates = if let Some(previous_state_id) = scope.previous_history_state_id {
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
                            history_state_pair(history, state_id, previous_state_id, false)
                                .is_some()
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
            (!candidates.is_empty()).then_some((scope, candidates))
        })
        .collect::<Vec<_>>();
    let mut resolved = HashMap::<String, String>::new();
    for (scope, candidates) in &candidates {
        if candidates.len() == 1 {
            resolved.insert(scope.id.clone(), candidates[0].id.clone());
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
            crate::ids::same_native_occurrence(&binding.id, &scope.id)
                && binding.entity_suffix > u64::from(scope.record_index)
                && next_scope_record_index
                    .is_none_or(|next| binding.entity_suffix < u64::from(next))
        });
        if let Some(binding) = output_bindings.next() {
            if output_bindings.next().is_none() {
                let matching = candidates
                    .iter()
                    .filter(|history| {
                        historical_brep_source(&history.id).is_some_and(|source| {
                            binding.blob_name.strip_prefix("BREP.") == Some(source)
                        })
                    })
                    .collect::<Vec<_>>();
                if let [history] = matching.as_slice() {
                    resolved.insert(scope.id.clone(), history.id.clone());
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
            .flat_map(|operand| &operand.references)
            .flat_map(|reference| &reference.candidate_faces)
            .collect::<Vec<_>>();
        if !candidate_faces.is_empty() {
            let matching = candidates
                .iter()
                .filter(|history| {
                    historical_brep_source(&history.id).is_some_and(|source| {
                        candidate_faces
                            .iter()
                            .any(|face| active_brep_face_matches_source(face, source))
                    })
                })
                .collect::<Vec<_>>();
            if let [history] = matching.as_slice() {
                resolved.insert(scope.id.clone(), history.id.clone());
                continue;
            }
        }
        let Some(construction) = &scope.base_feature_construction else {
            continue;
        };
        let mut referenced_histories =
            construction
                .body_reference_records()
                .iter()
                .filter_map(|suffix| {
                    let mut bindings = body_bindings.iter().filter(|binding| {
                        crate::ids::same_native_occurrence(&binding.id, &scope.id)
                            && binding.entity_suffix == u64::from(*suffix)
                    });
                    let binding = bindings.next()?;
                    if bindings.next().is_some() {
                        return None;
                    }
                    let mut matching = candidates.iter().filter(|history| {
                        historical_brep_source(&history.id).is_some_and(|source| {
                            binding.blob_name.strip_prefix("BREP.") == Some(source)
                        })
                    });
                    let history = matching.next()?;
                    matching.next().is_none().then_some(history.id.as_str())
                });
        let Some(history_id) = referenced_histories.next() else {
            continue;
        };
        if referenced_histories.all(|candidate| candidate == history_id) {
            resolved.insert(scope.id.clone(), history_id.to_owned());
        }
    }
    let mut groups = HashMap::<(&str, i64, Option<i64>), Vec<usize>>::new();
    for (index, (scope, _)) in candidates.iter().enumerate() {
        let (Some(stream), Some(state_id)) =
            (crate::ids::native_stream(&scope.id), scope.history_state_id)
        else {
            continue;
        };
        groups
            .entry((stream, state_id, scope.previous_history_state_id))
            .or_default()
            .push(index);
    }
    for members in groups.values() {
        let candidate_histories = members
            .iter()
            .flat_map(|index| {
                candidates[*index]
                    .1
                    .iter()
                    .map(|history| history.id.as_str())
            })
            .collect::<HashSet<_>>();
        if candidate_histories.len() != members.len() {
            continue;
        }
        loop {
            let assigned = members
                .iter()
                .filter_map(|index| resolved.get(&candidates[*index].0.id))
                .cloned()
                .collect::<HashSet<_>>();
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
                let remaining = scope_candidates
                    .iter()
                    .filter(|history| !assigned.contains(&history.id))
                    .collect::<Vec<_>>();
                if let [history] = remaining.as_slice() {
                    resolved.insert(scope.id.clone(), history.id.clone());
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
    let states = history_state_index(history);
    let mut current = state;
    let mut visited = HashSet::new();
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return false;
        }
        let Some(previous_state_id) = linked_previous_state_id(history, current) else {
            return false;
        };
        let Some(previous) = states.get(&previous_state_id).and_then(|state| *state) else {
            return false;
        };
        current = previous;
    }
    true
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
    operand: &crate::records::DesignFaceOperand,
    scope: &crate::records::DesignParameterScope,
    operand_groups: &'a [crate::records::DesignConstructionOperandGroup],
) -> Option<&'a crate::records::DesignConstructionOperandGroup> {
    let stream = crate::ids::native_stream(&operand.id)?;
    if crate::ids::native_stream(&scope.id) != Some(stream) {
        return None;
    }
    let group_record_index = operand.group_record_index?;
    let group_member_ordinal = usize::try_from(operand.group_member_ordinal?).ok()?;
    let mut groups = operand_groups.iter().filter(|group| {
        crate::ids::native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && group.record_index == group_record_index
            && group.role == 0x0000_0010_0000_0000
            && group.members.get(group_member_ordinal) == Some(&operand.record_index)
    });
    let group = groups.next()?;
    groups.next().is_none().then_some(group)
}

pub(crate) fn bind_face_operand_history_candidates(
    operands: &mut [crate::records::DesignFaceOperand],
    scopes: &[crate::records::DesignParameterScope],
    operand_groups: &[crate::records::DesignConstructionOperandGroup],
    histories: &[AsmHistory],
) {
    if projection_was_finalized(histories) {
        return;
    }
    for operand in &mut *operands {
        operand.preceding_candidate_faces.clear();
        operand.changed_candidate_faces.clear();
        operand.historical_support_contexts.clear();
        operand.resolved_face_slots.clear();
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
        let Some(state_id) = scope.history_state_id else {
            continue;
        };
        let Some(previous_state_id) = effective_scope_previous_history_state_id(scope, histories)
        else {
            continue;
        };
        let Some((history, state, previous)) =
            unique_history_state_pair(histories, state_id, previous_state_id)
        else {
            continue;
        };
        let states = history_state_index(history);
        let Some(topology) = &previous.topology else {
            continue;
        };
        let Some(changed_faces) =
            face_changes_across_state_chain(state, previous_state_id, &states)
        else {
            continue;
        };
        let feature_family = crate::design::design_feature_family(&scope.kind);
        let thread_face_candidates = (feature_family
            == Some(crate::design::DesignFeatureFamily::Thread))
        .then(|| {
            exact_face_selection_group(operand, scope, operand_groups)?;
            let candidates = effective_faces(operand.recipe_references.first()?);
            (!candidates.is_empty()).then(|| candidates.to_vec())
        })
        .flatten();
        let nested_split_face_candidates = (scope.kind == "SplitFace")
            .then(|| {
                exact_face_selection_group(operand, scope, operand_groups)?;
                crate::design::face_resolve::nested_bounded_face_history_candidates(operand)
            })
            .flatten();
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
        let history_candidates = thread_face_candidates
            .clone()
            .or(nested_split_face_candidates)
            .or_else(|| grouped_reference_face_candidates.clone())
            .unwrap_or_else(|| {
                crate::design::face_resolve::historical_face_operand_candidates(operand)
            });
        operand.preceding_candidate_faces = faces_in_topology(&history_candidates, topology);
        operand.changed_candidate_faces = operand
            .preceding_candidate_faces
            .iter()
            .filter(|face| stable_ref(&face.0).is_some_and(|slot| changed_faces.contains(&slot)))
            .cloned()
            .collect();
        operand.historical_support_contexts = historical_face_support_contexts(
            &history_candidates,
            histories,
            topology,
            &changed_faces,
        );
        let preserves_stable_face_set = feature_family
            == Some(crate::design::DesignFeatureFamily::Shell)
            || operand
                .group_record_index
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
                        && group.extrude_face_role
                            == Some(crate::records::DesignExtrudeFaceRole::Termination)
                });
        operand.resolved_face_slots = match scope.direct_face_operation {
            Some(crate::records::DesignDirectFaceOperation::OffsetFaces { .. }) => {
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
                } else if preserves_stable_face_set {
                    crate::design::face_resolve::resolve_stable_bounded_face_history_set(operand)
                        .or_else(|| {
                            crate::design::face_resolve::resolve_bounded_face_history_candidates(
                                operand,
                            )
                        })
                        .unwrap_or_default()
                } else if let Some(bounded) =
                    crate::design::face_resolve::resolve_bounded_face_history_candidates(operand)
                {
                    bounded
                } else {
                    let pattern = {
                        (feature_family
                            == Some(crate::design::DesignFeatureFamily::CircularPattern))
                        .then(|| {
                            resolve_pattern_face_by_surface_radius(
                                crate::design::face_resolve::face_operand_candidates(operand),
                                topology,
                                state.topology.as_ref()?,
                                &changed_faces,
                            )
                        })
                        .flatten()
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
            && operand.recipe_kind == crate::records::ConstructionRecipeKind::BoundedFace
            && state
                .transition
                .as_ref()
                .is_some_and(|transition| transition.previous_state_id == Some(previous_state_id))
        {
            if let Some(face) = state
                .topology
                .as_ref()
                .zip(state.transition.as_ref())
                .and_then(|(result, transition)| {
                    resolve_bounded_face_recipe_target(
                        operand,
                        topology,
                        result,
                        &transition.topology.bodies.inserted,
                    )
                })
            {
                operand.resolved_face_slots = vec![face];
            }
        }
    }
    bind_profile_face_group_cardinality(operands, scopes, operand_groups, histories);
}

fn resolve_pattern_face_by_surface_radius(
    candidates: &[cadmpeg_ir::ids::FaceId],
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Option<i64> {
    let candidate_faces = candidates
        .iter()
        .filter_map(|face| stable_ref(&face.0))
        .collect::<HashSet<_>>();
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
        let radius = radii.next()?.radius;
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
    operand: &crate::records::DesignFaceOperand,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<i64> {
    if operand.group_record_index.is_some()
        || operand.group_member_ordinal.is_some()
        || operand.scope_reference_ordinal != 1
        || operand.recipe_kind != crate::records::ConstructionRecipeKind::Face
        || operand.recipe_program != [0, -1]
    {
        return None;
    }
    let [reference] = operand.recipe_references.as_slice() else {
        return None;
    };
    let candidates = faces_in_topology(&reference.candidate_faces, topology);
    let [face] = candidates.as_slice() else {
        return None;
    };
    stable_ref(&face.0)
}

fn effective_faces(
    reference: &crate::records::DesignRecipeReference,
) -> &[cadmpeg_ir::ids::FaceId] {
    if reference.candidate_faces.is_empty() {
        &reference.alternate_selector_faces
    } else {
        &reference.candidate_faces
    }
}

fn resolve_thread_face_by_transition(
    scope: &crate::records::DesignParameterScope,
    candidates: &[cadmpeg_ir::ids::FaceId],
    history: &AsmHistory,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Option<i64> {
    let construction = scope.thread_construction.as_ref()?;
    let source = historical_brep_source(&history.id)?;
    let mut source_candidates = candidates
        .iter()
        .filter(|face| active_brep_face_matches_source(face, source));
    source_candidates.next()?;
    if source_candidates.next().is_some() {
        return None;
    }
    let minimum_radius = construction.minor_diameter * 5.0;
    let maximum_radius = construction.major_diameter * 5.0;
    if !minimum_radius.is_finite()
        || !maximum_radius.is_finite()
        || minimum_radius <= 0.0
        || maximum_radius < minimum_radius
    {
        return None;
    }
    let tolerance = 1.0e-9 * (1.0 + maximum_radius.abs());
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
        })
        .collect::<Vec<_>>();
    matching_faces.sort_unstable();
    matching_faces.dedup();
    let [face] = matching_faces.as_slice() else {
        return None;
    };
    Some(*face)
}

fn grouped_reference_face_candidate(
    operand: &crate::records::DesignFaceOperand,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Option<cadmpeg_ir::ids::FaceId> {
    if operand.recipe_kind != crate::records::ConstructionRecipeKind::BoundedFace
        || !crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(
            &operand.recipe_prefix_bytes,
        )
    {
        return None;
    }
    let topology_faces = topology.faces.iter().copied().collect::<HashSet<_>>();
    let mut candidates = operand
        .recipe_references
        .iter()
        .map(|reference| reference.design_reference)
        .filter(|reference| topology_faces.contains(reference) && changed_faces.contains(reference))
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    let [face] = candidates.as_slice() else {
        return None;
    };
    Some(cadmpeg_ir::ids::FaceId(crate::ids::brep_entity_id(*face)))
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
    operand: &crate::records::DesignFaceOperand,
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
    inserted_bodies: &[i64],
) -> Option<i64> {
    let crate::design::decode::operands::FaceRecipeProgramKind::Counted { header_value } =
        crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)?
    else {
        return None;
    };
    if operand.recipe_nodes.len() != header_value
        || operand
            .recipe_nodes
            .iter()
            .any(|node| node.recipe_structure.is_none())
    {
        return None;
    }
    let first = operand.recipe_references.first()?;
    let first_clause = operand
        .recipe_references
        .iter()
        .take_while(|reference| {
            reference.selector_offset == first.selector_offset
                && reference.token_offset == first.token_offset
        })
        .collect::<Vec<_>>();
    let topology_faces = preceding.faces.iter().copied().collect::<HashSet<_>>();
    let mut target_candidates = first_clause
        .first()
        .into_iter()
        .flat_map(|reference| effective_faces(reference))
        .filter_map(|face| stable_ref(&face.0))
        .filter(|face| topology_faces.contains(face))
        .collect::<BTreeSet<_>>();
    for reference in first_clause.iter().skip(1) {
        let candidates = effective_faces(reference)
            .iter()
            .filter_map(|face| stable_ref(&face.0))
            .filter(|face| topology_faces.contains(face))
            .collect::<HashSet<_>>();
        target_candidates.retain(|face| candidates.contains(face));
    }
    let construction_faces = inserted_bodies
        .iter()
        .filter_map(|body| {
            let mut faces = Vec::new();
            for region in relation_members(&result.body_regions, *body)? {
                for shell in relation_members(&result.region_shells, *region)? {
                    faces.extend_from_slice(relation_members(&result.shell_faces, *shell)?);
                }
            }
            faces.sort_unstable();
            faces.dedup();
            let [face] = faces.as_slice() else {
                return None;
            };
            Some(*face)
        })
        .collect::<Vec<_>>();
    if construction_faces.is_empty() {
        return None;
    }
    let face_loop_positions = |face, topology| {
        let contexts = face_boundary_contexts_for_slots(&[face], topology);
        let [context] = contexts.as_slice() else {
            return None;
        };
        let [loop_] = context.loops.as_slice() else {
            return None;
        };
        (!loop_.positions.is_empty() && loop_.positions.len() == loop_.edge_slots.len())
            .then(|| (loop_.edge_slots.len(), loop_.positions.clone()))
    };
    let mut matches = target_candidates
        .into_iter()
        .filter(|candidate| {
            let Some((edge_count, candidate_points)) = face_loop_positions(*candidate, preceding)
            else {
                return false;
            };
            if edge_count != header_value {
                return false;
            }
            construction_faces
                .iter()
                .filter_map(|face| face_loop_positions(*face, result))
                .any(|(construction_edge_count, construction_points)| {
                    construction_edge_count >= edge_count
                        && cyclic_point_subsequence(&candidate_points, &construction_points)
                })
        })
        .collect::<Vec<_>>();
    matches.sort_unstable();
    matches.dedup();
    let [face] = matches.as_slice() else {
        return None;
    };
    Some(*face)
}

fn cyclic_point_subsequence(
    candidate: &[cadmpeg_ir::math::Point3],
    construction: &[cadmpeg_ir::math::Point3],
) -> bool {
    let coincident = |left: &cadmpeg_ir::math::Point3, right: &cadmpeg_ir::math::Point3| {
        let dx = left.x - right.x;
        let dy = left.y - right.y;
        let dz = left.z - right.z;
        dx.mul_add(dx, dy.mul_add(dy, dz * dz)) <= 1.0e-12
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
    let reversed = candidate.iter().copied().rev().collect::<Vec<_>>();
    matches_orientation(candidate) || matches_orientation(&reversed)
}

pub(crate) fn bind_body_recipe_operand_history_candidates(
    operands: &mut [crate::records::DesignBodyRecipeOperand],
    recipes: &[crate::records::ConstructionRecipe],
    scopes: &[crate::records::DesignParameterScope],
    histories: &[AsmHistory],
) {
    if projection_was_finalized(histories) {
        return;
    }
    for operand in operands.iter_mut() {
        for reference in &mut operand.references {
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
        let mut states = HashMap::<i64, Option<&AsmDeltaState>>::new();
        for state in &history.states {
            states
                .entry(state.state_id)
                .and_modify(|state| *state = None)
                .or_insert(Some(state));
        }
        let Some(topology) = &previous.topology else {
            continue;
        };
        if face_changes_across_state_chain(state, previous.state_id, &states).is_none() {
            continue;
        }
        let Some(source) = historical_brep_source(&previous.id) else {
            continue;
        };
        for reference in &mut operand.references {
            reference.preceding_candidate_faces = faces_in_topology(
                &reference
                    .candidate_faces
                    .iter()
                    .filter(|face| active_brep_face_matches_source(face, source))
                    .cloned()
                    .collect::<Vec<_>>(),
                topology,
            );
            let face_slots = reference
                .preceding_candidate_faces
                .iter()
                .filter_map(|face| stable_ref(&face.0))
                .collect::<BTreeSet<_>>();
            let Some(body_slots) = bodies_intersecting(topology, &face_slots) else {
                continue;
            };
            reference.preceding_body_slots = body_slots.into_iter().collect();
        }
        if let [reference] = operand.references.as_slice() {
            if let [face] = reference.preceding_candidate_faces.as_slice() {
                operand.resolved_face_slot = stable_ref(&face.0);
            }
        }
        let Some(first) = operand.references.first() else {
            continue;
        };
        if first.preceding_body_slots.is_empty()
            || operand
                .references
                .iter()
                .any(|reference| reference.preceding_body_slots.is_empty())
        {
            continue;
        }
        let mut intersection = first
            .preceding_body_slots
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        for reference in &operand.references[1..] {
            intersection.retain(|body| reference.preceding_body_slots.contains(body));
        }
        if intersection.len() == 1 {
            operand.resolved_body_slot = intersection.into_iter().next();
        }
    }
    let mut recipes_by_id = HashMap::<_, Option<&crate::records::ConstructionRecipe>>::new();
    for recipe in recipes {
        recipes_by_id
            .entry(recipe.id.as_str())
            .and_modify(|recipe| *recipe = None)
            .or_insert(Some(recipe));
    }
    let identity = |operand: &crate::records::DesignBodyRecipeOperand| {
        let stream = crate::ids::native_stream(&operand.id)?.to_owned();
        let recipe = recipes_by_id
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| *recipe)?;
        let selector = recipe.design_selector?;
        Some((
            stream,
            operand.asset_id.clone(),
            operand.context_id.clone(),
            operand
                .references
                .iter()
                .map(|reference| (reference.design_reference, reference.form))
                .collect::<Vec<_>>(),
            recipe.design_id.clone(),
            selector.value,
        ))
    };
    let mut resolved_by_identity = HashMap::new();
    for operand in operands.iter() {
        let (Some(identity), Some(body)) = (identity(operand), operand.resolved_body_slot) else {
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
            operand.resolved_body_slot = identity(operand)
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
        let Some(topology) = previous.topology.as_ref() else {
            continue;
        };
        let Some(faces) = complete_body_face_slots(topology, body_slot) else {
            continue;
        };
        operand.resolved_body_state_id = Some(previous.state_id);
        operand.resolved_body_face_slots = faces;
    }
}

fn body_recipe_operand_history_pair<'a>(
    operand: &crate::records::DesignBodyRecipeOperand,
    scopes: &[crate::records::DesignParameterScope],
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
    let state_id = scope.history_state_id?;
    let previous_state_id = effective_scope_previous_history_state_id(scope, histories)?;
    let (history, state, previous) =
        unique_history_state_pair(histories, state_id, previous_state_id)?;
    Some((history, state, previous))
}

fn complete_body_face_slots(topology: &AsmHistoricalTopology, body: i64) -> Option<Vec<i64>> {
    fn occurrence_counts(slots: &[i64]) -> HashMap<i64, usize> {
        let mut counts = HashMap::with_capacity(slots.len());
        for &slot in slots {
            *counts.entry(slot).or_default() += 1;
        }
        counts
    }

    struct RelationIndex<'a> {
        members_by_owner: HashMap<i64, Option<&'a [i64]>>,
        owner_by_member: HashMap<i64, Option<i64>>,
    }

    fn relation_index(relations: &[AsmHistoricalRelation]) -> RelationIndex<'_> {
        let mut members_by_owner = HashMap::with_capacity(relations.len());
        let mut owner_by_member = HashMap::new();
        for relation in relations {
            members_by_owner
                .entry(relation.owner_ref)
                .and_modify(|members| *members = None)
                .or_insert(Some(relation.member_refs.as_slice()));
            for &member in &relation.member_refs {
                owner_by_member
                    .entry(member)
                    .and_modify(|owner| *owner = None)
                    .or_insert(Some(relation.owner_ref));
            }
        }
        RelationIndex {
            members_by_owner,
            owner_by_member,
        }
    }

    let body_counts = occurrence_counts(&topology.bodies);
    let region_counts = occurrence_counts(&topology.regions);
    let shell_counts = occurrence_counts(&topology.shells);
    let face_counts = occurrence_counts(&topology.faces);
    let body_regions = relation_index(&topology.body_regions);
    let region_shells = relation_index(&topology.region_shells);
    let shell_faces = relation_index(&topology.shell_faces);

    if body_counts.get(&body).copied() != Some(1) {
        return None;
    }
    let regions = body_regions
        .members_by_owner
        .get(&body)
        .copied()
        .flatten()?;
    if regions.is_empty() {
        return None;
    }
    let mut seen_regions = HashSet::new();
    let mut seen_shells = HashSet::new();
    let mut seen_faces = HashSet::new();
    for &region in regions {
        if !seen_regions.insert(region)
            || region_counts.get(&region).copied() != Some(1)
            || body_regions.owner_by_member.get(&region).copied().flatten() != Some(body)
        {
            return None;
        }
        let shells = region_shells
            .members_by_owner
            .get(&region)
            .copied()
            .flatten()?;
        if shells.is_empty() {
            return None;
        }
        for &shell in shells {
            if !seen_shells.insert(shell)
                || shell_counts.get(&shell).copied() != Some(1)
                || region_shells.owner_by_member.get(&shell).copied().flatten() != Some(region)
            {
                return None;
            }
            let faces = shell_faces
                .members_by_owner
                .get(&shell)
                .copied()
                .flatten()?;
            if faces.is_empty() {
                return None;
            }
            for &face in faces {
                if !seen_faces.insert(face)
                    || face_counts.get(&face).copied() != Some(1)
                    || shell_faces.owner_by_member.get(&face).copied().flatten() != Some(shell)
                {
                    return None;
                }
            }
        }
    }
    let mut faces = seen_faces.into_iter().collect::<Vec<_>>();
    faces.sort_unstable();
    (!faces.is_empty()).then_some(faces)
}

fn active_brep_face_matches_source(face: &cadmpeg_ir::ids::FaceId, source: &str) -> bool {
    face.0.starts_with("f3d:brep:entity#") || face.0.starts_with(&format!("f3d:brep/{source}/"))
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
    references: &[crate::records::DesignRecipeReference],
    topology: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Vec<i64> {
    let mut clauses = Vec::<(u64, u64, Vec<&crate::records::DesignRecipeReference>)>::new();
    for reference in references {
        let key = (reference.selector_offset, reference.token_offset);
        if let Some((_, _, references)) = clauses
            .iter_mut()
            .find(|(selector, token, _)| (*selector, *token) == key)
        {
            references.push(reference);
        } else {
            clauses.push((key.0, key.1, vec![reference]));
        }
    }
    let topology_faces = topology.faces.iter().copied().collect::<HashSet<_>>();
    let mut resolved = Vec::new();
    for (_, _, references) in clauses {
        let mut intersection = None::<HashSet<i64>>;
        for reference in references {
            let candidates = if reference.candidate_faces.is_empty() {
                &reference.alternate_selector_faces
            } else {
                &reference.candidate_faces
            };
            let candidates = candidates
                .iter()
                .filter_map(|face| stable_ref(&face.0))
                .filter(|face| topology_faces.contains(face) && changed_faces.contains(face))
                .collect::<HashSet<_>>();
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
    operands: &mut [crate::records::DesignFaceOperand],
    scopes: &[crate::records::DesignParameterScope],
    operand_groups: &[crate::records::DesignConstructionOperandGroup],
    histories: &[AsmHistory],
) {
    let mut states = HashMap::<i64, Option<&AsmDeltaState>>::new();
    for state in histories.iter().flat_map(|history| &history.states) {
        states
            .entry(state.state_id)
            .and_modify(|state| *state = None)
            .or_insert(Some(state));
    }
    for scope in scopes {
        let Some(profile_groups) =
            crate::design::face_resolve::extrude_profile_group_roots(scope, operand_groups)
        else {
            continue;
        };
        for group in profile_groups {
            let Some(indices) = crate::design::face_resolve::extrude_profile_group_operand_indices(
                group,
                operand_groups,
                operands,
            ) else {
                continue;
            };
            if group.members.len() != indices.len()
                || indices.iter().any(|index| {
                    let operand = &operands[*index];
                    !operand.resolved_face_slots.is_empty()
                        || !crate::design::face_resolve::face_operand_candidates(operand).is_empty()
                        || operand.recipe_references.iter().any(|reference| {
                            !reference.candidate_faces.is_empty()
                                || !reference.alternate_selector_faces.is_empty()
                        })
                })
            {
                continue;
            }
            let (Some(state_id), Some(previous_state_id)) =
                (scope.history_state_id, scope.previous_history_state_id)
            else {
                continue;
            };
            let (Some(Some(state)), Some(Some(previous))) =
                (states.get(&state_id), states.get(&previous_state_id))
            else {
                continue;
            };
            let (Some(topology), Some(changed_faces)) = (
                previous.topology.as_ref(),
                face_changes_across_state_chain(state, previous_state_id, &states),
            ) else {
                continue;
            };
            let faces = profile_face_group_cardinality_candidates(
                topology,
                &changed_faces,
                group.members.len(),
            )
            .or_else(|| {
                if !crate::design::face_resolve::is_paired_extrude_profile_aggregate(
                    group,
                    operand_groups,
                    operands,
                ) {
                    return None;
                }
                let transition = state
                    .transition
                    .as_ref()
                    .filter(|transition| transition.previous_state_id == Some(previous_state_id))?;
                let preceding_faces = topology.faces.iter().copied().collect::<HashSet<_>>();
                let mut deleted = transition.topology.faces.deleted.clone();
                deleted.sort_unstable();
                deleted.dedup();
                (deleted.len() == group.members.len()
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
            });
            let Some(faces) = faces else {
                continue;
            };
            for (index, face) in indices.into_iter().zip(faces) {
                let face_id = cadmpeg_ir::ids::FaceId(crate::ids::brep_entity_id(face));
                operands[index].preceding_candidate_faces = vec![face_id.clone()];
                operands[index].changed_candidate_faces = vec![face_id];
                operands[index].resolved_face_slots = vec![face];
            }
        }
    }
}

fn profile_face_group_cardinality_candidates(
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    member_count: usize,
) -> Option<Vec<i64>> {
    let preceding_faces = topology.faces.iter().copied().collect::<HashSet<_>>();
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
    faces.sort_unstable();
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
        changed.extend(transition.topology.faces.deleted.iter().copied());
        changed.extend(transition.topology.faces.updated.iter().copied());
        current = states.get(&transition.previous_state_id?)?.as_ref()?;
    }
    Some(changed)
}

fn edge_changes_across_state_chain<'a>(
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Option<(HashSet<i64>, HashSet<i64>)> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut deleted = HashSet::new();
    let mut updated = HashSet::new();
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        let transition = current.transition.as_ref()?;
        deleted.extend(transition.topology.edges.deleted.iter().copied());
        updated.extend(transition.topology.edges.updated.iter().copied());
        current = states.get(&transition.previous_state_id?)?.as_ref()?;
    }
    Some((deleted, updated))
}

fn historical_face_support_contexts(
    candidates: &[cadmpeg_ir::ids::FaceId],
    histories: &[AsmHistory],
    preceding_topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Vec<crate::records::DesignHistoricalFaceSupportContext> {
    let preceding_faces = preceding_topology
        .faces
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    candidates
        .iter()
        .filter_map(|candidate| {
            let active_face_slot = stable_ref(&candidate.0)?;
            let mut carriers = histories
                .iter()
                .flat_map(|history| &history.states)
                .filter_map(|state| state.topology.as_ref())
                .map(|topology| {
                    let bindings = topology
                        .face_surfaces
                        .iter()
                        .filter(|binding| binding.entity == active_face_slot)
                        .collect::<Vec<_>>();
                    match bindings.as_slice() {
                        [] => Some(None),
                        [binding] => Some(Some(binding.carrier)),
                        _ => None,
                    }
                })
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            carriers.sort_unstable();
            carriers.dedup();
            let [surface_slot] = carriers.as_slice() else {
                return None;
            };
            let mut preceding_face_slots = preceding_topology
                .face_surfaces
                .iter()
                .filter(|binding| {
                    binding.carrier == *surface_slot && preceding_faces.contains(&binding.entity)
                })
                .map(|binding| binding.entity)
                .collect::<Vec<_>>();
            preceding_face_slots.sort_unstable();
            preceding_face_slots.dedup();
            if preceding_face_slots.is_empty() {
                return None;
            }
            let changed_preceding_face_slots = preceding_face_slots
                .iter()
                .copied()
                .filter(|face| changed_faces.contains(face))
                .collect();
            Some(crate::records::DesignHistoricalFaceSupportContext {
                active_face_slot,
                surface_slot: *surface_slot,
                preceding_face_boundaries: face_boundary_contexts_for_slots(
                    &preceding_face_slots,
                    preceding_topology,
                ),
                preceding_face_slots,
                changed_preceding_face_slots,
            })
        })
        .collect()
}

fn face_boundary_edges(
    faces: &[cadmpeg_ir::ids::FaceId],
    topology: &AsmHistoricalTopology,
) -> Vec<i64> {
    let face_slots = faces
        .iter()
        .filter_map(|face| stable_ref(&face.0))
        .collect::<HashSet<_>>();
    let loops = topology
        .face_loops
        .iter()
        .filter(|relation| face_slots.contains(&relation.owner_ref))
        .flat_map(|relation| relation.member_refs.iter().copied())
        .collect::<HashSet<_>>();
    let coedges = topology
        .loop_coedges
        .iter()
        .filter(|relation| loops.contains(&relation.owner_ref))
        .flat_map(|relation| relation.member_refs.iter().copied())
        .collect::<HashSet<_>>();
    let mut edges = topology
        .coedge_topology
        .iter()
        .filter(|coedge| coedges.contains(&coedge.coedge))
        .map(|coedge| coedge.edge)
        .collect::<Vec<_>>();
    edges.sort_unstable();
    edges.dedup();
    edges
}

fn face_boundary_contexts(
    faces: &[cadmpeg_ir::ids::FaceId],
    topology: &AsmHistoricalTopology,
) -> Vec<crate::records::DesignHistoricalFaceBoundaryContext> {
    let face_slots = faces
        .iter()
        .filter_map(|face| stable_ref(&face.0))
        .collect::<Vec<_>>();
    face_boundary_contexts_for_slots(&face_slots, topology)
}

fn face_boundary_contexts_for_slots(
    face_slots: &[i64],
    topology: &AsmHistoricalTopology,
) -> Vec<crate::records::DesignHistoricalFaceBoundaryContext> {
    face_slots
        .iter()
        .filter_map(|face_slot| {
            let mut face_relations = topology
                .face_loops
                .iter()
                .filter(|relation| relation.owner_ref == *face_slot);
            let face_relation = face_relations.next()?;
            if face_relations.next().is_some() {
                return None;
            }
            let loops = face_relation
                .member_refs
                .iter()
                .map(|loop_slot| {
                    let mut loop_relations = topology
                        .loop_coedges
                        .iter()
                        .filter(|relation| relation.owner_ref == *loop_slot);
                    let loop_relation = loop_relations.next()?;
                    if loop_relations.next().is_some() {
                        return None;
                    }
                    let edge_slots = loop_relation
                        .member_refs
                        .iter()
                        .map(|coedge_slot| {
                            let mut coedges = topology
                                .coedge_topology
                                .iter()
                                .filter(|coedge| coedge.coedge == *coedge_slot);
                            let edge = coedges.next()?.edge;
                            (coedges.next().is_none()).then_some(edge)
                        })
                        .collect::<Option<Vec<_>>>()?;
                    let vertex_slots =
                        ordered_loop_vertices(&edge_slots, topology).unwrap_or_default();
                    let point_slots = (!vertex_slots.is_empty())
                        .then(|| {
                            vertex_slots
                                .iter()
                                .map(|vertex| {
                                    let mut bindings = topology
                                        .vertex_points
                                        .iter()
                                        .filter(|binding| binding.entity == *vertex);
                                    let point = bindings.next()?.carrier;
                                    (bindings.next().is_none()).then_some(point)
                                })
                                .collect::<Option<Vec<_>>>()
                        })
                        .flatten()
                        .unwrap_or_default();
                    let positions = (point_slots.len() == vertex_slots.len())
                        .then(|| {
                            point_slots
                                .iter()
                                .map(|point| {
                                    let mut values = topology
                                        .point_positions
                                        .iter()
                                        .filter(|value| value.point == *point);
                                    let position = values.next()?.position;
                                    (values.next().is_none()).then_some(position)
                                })
                                .collect::<Option<Vec<_>>>()
                        })
                        .flatten()
                        .unwrap_or_default();
                    Some(crate::records::DesignHistoricalFaceLoopContext {
                        loop_slot: *loop_slot,
                        coedge_slots: loop_relation.member_refs.clone(),
                        edge_slots,
                        vertex_slots,
                        point_slots,
                        positions,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(crate::records::DesignHistoricalFaceBoundaryContext {
                face_slot: *face_slot,
                loops,
            })
        })
        .collect()
}

fn ordered_loop_vertices(edge_slots: &[i64], topology: &AsmHistoricalTopology) -> Option<Vec<i64>> {
    if edge_slots.is_empty() {
        return Some(Vec::new());
    }
    edge_slots
        .iter()
        .enumerate()
        .map(|(ordinal, edge)| {
            let previous = edge_slots[(ordinal + edge_slots.len() - 1) % edge_slots.len()];
            let endpoints = |slot| {
                let mut edges = topology
                    .edge_vertices
                    .iter()
                    .filter(|candidate| candidate.edge == slot);
                let edge = edges.next()?;
                (edges.next().is_none()).then_some([edge.start_vertex, edge.end_vertex])
            };
            let previous = endpoints(previous)?;
            let current = endpoints(*edge)?;
            let mut shared = previous
                .into_iter()
                .filter(|vertex| current.contains(vertex))
                .collect::<Vec<_>>();
            shared.sort_unstable();
            shared.dedup();
            match shared.as_slice() {
                [vertex] => Some(*vertex),
                _ => None,
            }
        })
        .collect()
}

fn preceding_support_face_slots(
    result_faces: &[cadmpeg_ir::ids::FaceId],
    result_topology: &AsmHistoricalTopology,
    preceding_topology: &AsmHistoricalTopology,
) -> Vec<i64> {
    let preceding_faces = preceding_topology
        .faces
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let mut support_faces = Vec::new();
    for result_face in result_faces {
        let Some(result_face) = stable_ref(&result_face.0) else {
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

fn edge_recipe_reference_context(
    reference_ordinal: u32,
    reference: &crate::records::DesignRecipeReference,
    result_topology: &AsmHistoricalTopology,
    result_boundary_edges: &[i64],
    preceding_topology: &AsmHistoricalTopology,
    preceding_boundary_edges: &[i64],
    changed_edges: &HashSet<i64>,
) -> crate::records::DesignEdgeRecipeReferenceContext {
    let candidate_faces = if reference.candidate_faces.is_empty() {
        reference.alternate_selector_faces.as_slice()
    } else {
        reference.candidate_faces.as_slice()
    };
    let result_faces = faces_in_topology(candidate_faces, result_topology);
    let result_face_boundaries = face_boundary_contexts(&result_faces, result_topology);
    let result_edges = face_boundary_edges(&result_faces, result_topology)
        .into_iter()
        .collect::<HashSet<_>>();
    let result_shared_edge_slots = result_boundary_edges
        .iter()
        .copied()
        .filter(|edge| result_edges.contains(edge))
        .collect();
    let preceding_faces = faces_in_topology(candidate_faces, preceding_topology);
    let preceding_face_boundaries = face_boundary_contexts(&preceding_faces, preceding_topology);
    let preceding_support_face_slots =
        preceding_support_face_slots(&result_faces, result_topology, preceding_topology);
    let preceding_support_face_boundaries =
        face_boundary_contexts_for_slots(&preceding_support_face_slots, preceding_topology);
    let preceding_edges = face_boundary_edges(&preceding_faces, preceding_topology)
        .into_iter()
        .collect::<HashSet<_>>();
    let shared_edge_slots = preceding_boundary_edges
        .iter()
        .copied()
        .filter(|edge| preceding_edges.contains(edge))
        .collect::<Vec<_>>();
    let changed_shared_edge_slots = shared_edge_slots
        .iter()
        .copied()
        .filter(|edge| changed_edges.contains(edge))
        .collect::<Vec<_>>();
    let support_edges = preceding_support_face_boundaries
        .iter()
        .flat_map(|face| &face.loops)
        .flat_map(|face_loop| face_loop.edge_slots.iter().copied())
        .collect::<HashSet<_>>();
    let mut changed_reference_edge_slots = preceding_edges
        .iter()
        .copied()
        .chain(support_edges.iter().copied())
        .filter(|edge| changed_edges.contains(edge))
        .collect::<Vec<_>>();
    changed_reference_edge_slots.sort_unstable();
    changed_reference_edge_slots.dedup();
    crate::records::DesignEdgeRecipeReferenceContext {
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
    }
}

pub(crate) fn bind_edge_operand_history_candidates(
    operands: &mut [crate::records::DesignEdgeOperand],
    scopes: &[crate::records::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) {
    if projection_was_finalized(histories) {
        return;
    }
    let mut scope_operand_counts = HashMap::<(String, u32), usize>::new();
    for operand in operands.iter() {
        let Some(stream) = crate::ids::native_stream(&operand.id) else {
            continue;
        };
        *scope_operand_counts
            .entry((stream.to_owned(), operand.scope_record_index))
            .or_default() += 1;
    }
    let terminal_topologies = histories
        .iter()
        .filter_map(|history| {
            let preceding = history
                .states
                .iter()
                .filter_map(|state| state.transition.as_ref()?.previous_state_id)
                .collect::<HashSet<_>>();
            let mut terminals = history
                .states
                .iter()
                .filter(|state| !preceding.contains(&state.state_id));
            let state = terminals.next()?;
            terminals
                .next()
                .is_none()
                .then_some((state.state_id, state.topology.as_ref()?))
        })
        .collect::<Vec<_>>();
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
        operand.resolved_axis_origin = None;
        operand.resolved_axis_direction = None;
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
        let Some(state_id) = scope.history_state_id else {
            bind_active_edge_operand_for_scope(operand, scope, &terminal_topologies);
            continue;
        };
        let Some(previous_state_id) = effective_scope_previous_history_state_id(scope, histories)
        else {
            bind_active_edge_operand_for_scope(operand, scope, &terminal_topologies);
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
        let (Some(result_topology), Some(topology)) = (&state.topology, &previous.topology) else {
            continue;
        };
        let states = history_state_index(history);
        let Some(changed_faces) =
            face_changes_across_state_chain(state, previous_state_id, &states)
        else {
            continue;
        };
        let Some((chain_deleted_edges, chain_updated_edges)) =
            edge_changes_across_state_chain(state, previous_state_id, &states)
        else {
            continue;
        };
        let preceding_faces = topology.faces.iter().copied().collect::<HashSet<_>>();
        let inserted_faces = result_topology
            .faces
            .iter()
            .copied()
            .filter(|face| !preceding_faces.contains(face))
            .collect::<Vec<_>>();
        let result_edges = result_topology
            .edges
            .iter()
            .copied()
            .collect::<HashSet<_>>();
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
            faces_in_topology(&operand.candidate_faces, result_topology);
        operand.result_boundary_edge_slots =
            face_boundary_edges(&operand.result_candidate_faces, result_topology);
        operand.preceding_candidate_faces = faces_in_topology(&operand.candidate_faces, topology);
        operand.changed_candidate_faces = operand
            .preceding_candidate_faces
            .iter()
            .filter(|face| stable_ref(&face.0).is_some_and(|slot| changed_faces.contains(&slot)))
            .cloned()
            .collect();
        operand.preceding_boundary_edge_slots =
            face_boundary_edges(&operand.preceding_candidate_faces, topology);
        let changed_edges = deleted_edges
            .iter()
            .chain(&updated_edges)
            .copied()
            .collect::<HashSet<_>>();
        operand.changed_boundary_edge_slots = operand
            .preceding_boundary_edge_slots
            .iter()
            .copied()
            .filter(|edge| changed_edges.contains(edge))
            .collect();
        operand.deleted_boundary_edge_slots =
            boundary_edges_in_changes(&operand.preceding_boundary_edge_slots, &deleted_edges);
        operand.updated_boundary_edge_slots =
            boundary_edges_in_changes(&operand.preceding_boundary_edge_slots, &updated_edges);
        operand.treatment_radius_candidates = treatment_radius_candidates(
            Some(&operand.result_candidate_faces),
            &inserted_faces,
            result_topology,
            topology,
            &deleted_edges,
        );
        operand.changed_boundary_edge_contexts = operand
            .changed_boundary_edge_slots
            .iter()
            .copied()
            .map(|edge| historical_edge_context(edge, topology))
            .collect();
        operand.recipe_reference_contexts = operand
            .recipe_references
            .iter()
            .enumerate()
            .filter_map(|(ordinal, reference)| {
                let reference_ordinal = u32::try_from(ordinal).ok()?;
                Some(edge_recipe_reference_context(
                    reference_ordinal,
                    reference,
                    result_topology,
                    &operand.result_boundary_edge_slots,
                    topology,
                    &operand.preceding_boundary_edge_slots,
                    &changed_edges,
                ))
            })
            .collect();
        if scope.kind == "SurfacePatch" && operand.surface_patch_recipe_structure.is_some() {
            operand.resolved_edge_slot = surface_patch_edge_operand_slot(
                operand.surface_patch_recipe_structure.as_ref(),
                &operand.recipe_references,
                topology,
            );
            continue;
        }
        if crate::design::design_feature_family(&scope.kind)
            == Some(crate::design::DesignFeatureFamily::Sweep)
        {
            let reference_faces = terminal_edge_recipe_reference_faces(
                &operand.recipe_references,
                operand.local_topology_references.as_deref(),
            );
            let reference_edge_sets = reference_faces
                .iter()
                .map(|faces| face_boundary_edges(&faces_in_topology(faces, topology), topology))
                .collect::<Vec<_>>();
            let candidate_edges = reference_edge_sets
                .iter()
                .flatten()
                .copied()
                .collect::<BTreeSet<_>>();
            let contexts = candidate_edges
                .into_iter()
                .map(|edge| historical_edge_context(edge, topology))
                .collect::<Vec<_>>();
            operand.recipe_selectors =
                recipe_selector_candidates(operand.recipe_structure.as_ref(), &contexts);
            operand.resolved_edge_slot =
                crate::design::edge_resolve::unique_incidence_edge_shared_by_reference_faces(
                    &operand.recipe_selectors,
                    reference_edge_sets.iter().map(Vec::as_slice),
                );
            continue;
        }
        if crate::design::design_feature_family(&scope.kind)
            == Some(crate::design::DesignFeatureFamily::Revolve)
        {
            let reference_faces = terminal_edge_recipe_reference_faces(
                &operand.recipe_references,
                operand.local_topology_references.as_deref(),
            );
            let reference_edge_sets = reference_faces
                .iter()
                .map(|faces| face_boundary_edges(&faces_in_topology(faces, topology), topology))
                .collect::<Vec<_>>();
            let candidate_edges = reference_edge_sets
                .iter()
                .flatten()
                .copied()
                .collect::<BTreeSet<_>>();
            let contexts = candidate_edges
                .into_iter()
                .map(|edge| historical_edge_context(edge, topology))
                .collect::<Vec<_>>();
            operand.recipe_selectors =
                recipe_selector_candidates(operand.recipe_structure.as_ref(), &contexts);
            operand.resolved_edge_slot =
                crate::design::edge_resolve::resolved_edge_candidate_intersection(
                    &operand.recipe_selectors,
                    reference_edge_sets.iter().map(Vec::as_slice),
                );
            if let Some((origin, direction)) = operand
                .resolved_edge_slot
                .and_then(|edge| historical_edge_axis(edge, topology))
            {
                operand.resolved_axis_origin = Some(origin);
                operand.resolved_axis_direction = Some(direction);
            }
            continue;
        }
        let changed_edge_contexts = topology
            .edges
            .iter()
            .copied()
            .filter(|edge| changed_edges.contains(edge))
            .map(|edge| historical_edge_context(edge, topology))
            .collect::<Vec<_>>();
        operand.recipe_selectors =
            recipe_selector_candidates(operand.recipe_structure.as_ref(), &changed_edge_contexts);
        operand.resolved_edge_slot =
            crate::design::edge_resolve::resolve_edge_operand_candidates(operand);
        if operand.resolved_edge_slot.is_none()
            && stream.is_some_and(|stream| {
                scope_operand_counts.get(&(stream.to_owned(), operand.scope_record_index))
                    == Some(&1)
            })
            && deleted_edges.len() == 1
        {
            operand.resolved_edge_slot = deleted_edges.first().copied();
        }
    }
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
    let support_surfaces = historical_edge_context(edge, topology)
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
        .collect::<HashSet<_>>();
    let mut axes = topology
        .surface_axes
        .iter()
        .filter(|axis| support_surfaces.contains(&axis.surface))
        .map(|axis| (axis.origin, axis.direction));
    let first = axes.next()?;
    axes.all(|axis| same_axis_line(first, axis))
        .then_some(first)
}

fn bind_active_edge_operand_for_scope(
    operand: &mut crate::records::DesignEdgeOperand,
    scope: &crate::records::DesignParameterScope,
    terminal_topologies: &[(i64, &AsmHistoricalTopology)],
) {
    bind_active_edge_operand_candidates(operand, terminal_topologies);
    if scope.kind == "SurfacePatch" && operand.surface_patch_recipe_structure.is_some() {
        operand.recipe_state_id = None;
        operand.resolved_edge_slot = None;
        let mut matches = terminal_topologies
            .iter()
            .filter_map(|(state_id, topology)| {
                surface_patch_edge_operand_slot(
                    operand.surface_patch_recipe_structure.as_ref(),
                    &operand.recipe_references,
                    topology,
                )
                .map(|edge| (*state_id, edge))
            });
        if let Some((state_id, edge)) = matches.next() {
            if matches.next().is_none() {
                operand.recipe_state_id = Some(state_id);
                operand.resolved_edge_slot = Some(edge);
            }
        }
    }
    if crate::design::design_feature_family(&scope.kind)
        == Some(crate::design::DesignFeatureFamily::Revolve)
    {
        let topology = operand.recipe_state_id.and_then(|state_id| {
            terminal_topologies
                .iter()
                .find(|(candidate, _)| *candidate == state_id)
                .map(|(_, topology)| *topology)
        });
        if let Some((origin, direction)) = operand
            .resolved_edge_slot
            .zip(topology)
            .and_then(|(edge, topology)| historical_edge_axis(edge, topology))
        {
            operand.resolved_axis_origin = Some(origin);
            operand.resolved_axis_direction = Some(direction);
        }
    }
}

fn surface_patch_edge_operand_slot(
    structure: Option<&crate::records::DesignSurfacePatchRecipeStructure>,
    recipe_references: &[crate::records::DesignRecipeReference],
    topology: &AsmHistoricalTopology,
) -> Option<i64> {
    let structure = structure?;
    let [first, second] = structure.clauses.as_slice() else {
        return None;
    };
    let common_edge_reference = common_surface_patch_reference(
        first.edge_reference_ordinals,
        second.edge_reference_ordinals,
    )?;
    let common_face_reference = common_surface_patch_reference(
        first.face_reference_ordinals,
        second.face_reference_ordinals,
    )?;
    let edge_reference = recipe_references.get(usize::try_from(common_edge_reference).ok()?)?;
    let face_reference = recipe_references.get(usize::try_from(common_face_reference).ok()?)?;
    if edge_reference.candidate_edges.is_empty() {
        return None;
    }
    let face_candidates = if face_reference.candidate_faces.is_empty() {
        &face_reference.alternate_selector_faces
    } else {
        &face_reference.candidate_faces
    };
    let face_boundary_edges =
        face_boundary_edges(&faces_in_topology(face_candidates, topology), topology);
    let mut candidates = edge_reference
        .candidate_edges
        .iter()
        .filter_map(|edge| stable_ref(&edge.0))
        .filter(|edge| face_boundary_edges.contains(edge))
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Some(*edge),
        _ => None,
    }
}

fn common_surface_patch_reference(left: [u32; 2], right: [u32; 2]) -> Option<u32> {
    let common = left
        .into_iter()
        .filter(|reference| right.contains(reference))
        .collect::<Vec<_>>();
    let [reference] = common.as_slice() else {
        return None;
    };
    Some(*reference)
}

fn bind_active_edge_operand_candidates(
    operand: &mut crate::records::DesignEdgeOperand,
    topologies: &[(i64, &AsmHistoricalTopology)],
) {
    let mut matches = topologies.iter().filter_map(|(state_id, topology)| {
        let all_reference_faces =
            terminal_edge_recipe_reference_faces(&operand.recipe_references, None);
        let reference_faces = terminal_edge_recipe_reference_faces(
            &operand.recipe_references,
            operand.local_topology_references.as_deref(),
        );
        let terminal_faces = terminal_edge_recipe_faces(&operand.candidate_faces, &reference_faces);
        let candidate_faces = faces_in_topology(&terminal_faces, topology);
        if topologies.len() != 1 && candidate_faces.is_empty() {
            return None;
        }
        let boundary_edges = face_boundary_edges(&candidate_faces, topology);
        let contexts = boundary_edges
            .iter()
            .copied()
            .map(|edge| historical_edge_context(edge, topology))
            .collect::<Vec<_>>();
        let selectors = recipe_selector_candidates(operand.recipe_structure.as_ref(), &contexts);
        let reference_edge_sets = reference_faces
            .iter()
            .map(|faces| face_boundary_edges(&faces_in_topology(faces, topology), topology))
            .collect::<Vec<_>>();
        let all_reference_edge_sets = all_reference_faces
            .iter()
            .map(|faces| face_boundary_edges(&faces_in_topology(faces, topology), topology))
            .collect::<Vec<_>>();
        let edge = crate::design::edge_resolve::resolved_edge_candidate_intersection(
            &selectors,
            reference_edge_sets.iter().map(Vec::as_slice),
        );
        Some((
            *state_id,
            edge,
            candidate_faces,
            boundary_edges,
            contexts,
            all_reference_edge_sets,
            selectors,
        ))
    });
    let Some((
        state_id,
        edge,
        candidate_faces,
        boundary_edges,
        contexts,
        all_reference_edge_sets,
        selectors,
    )) = matches.next()
    else {
        return;
    };
    if matches.next().is_some() {
        return;
    }
    operand.terminal_candidate_faces = candidate_faces;
    operand.terminal_boundary_edge_slots = boundary_edges;
    operand.terminal_boundary_edge_contexts = contexts;
    operand.terminal_reference_edge_slots = all_reference_edge_sets;
    operand.recipe_selectors = selectors;
    operand.recipe_state_id = Some(state_id);
    operand.resolved_edge_slot = edge;
}

fn terminal_edge_recipe_faces(
    primary: &[cadmpeg_ir::ids::FaceId],
    reference_faces: &[Vec<cadmpeg_ir::ids::FaceId>],
) -> Vec<cadmpeg_ir::ids::FaceId> {
    let mut faces = primary.to_vec();
    faces.extend(reference_faces.iter().flatten().cloned());
    faces.sort_by(|left, right| left.0.cmp(&right.0));
    faces.dedup();
    faces
}

fn terminal_edge_recipe_reference_faces(
    references: &[crate::records::DesignRecipeReference],
    local_topology_references: Option<&[std::num::NonZeroU32]>,
) -> Vec<Vec<cadmpeg_ir::ids::FaceId>> {
    let selected_references = match local_topology_references {
        Some(ordinals) => ordinals
            .iter()
            .filter_map(|ordinal| {
                references.get(usize::try_from(ordinal.get()).ok()?.checked_sub(1)?)
            })
            .collect::<Vec<_>>(),
        None => references.iter().collect(),
    };
    selected_references
        .into_iter()
        .map(|reference| {
            if reference.candidate_faces.is_empty() {
                reference.alternate_selector_faces.clone()
            } else {
                reference.candidate_faces.clone()
            }
        })
        .collect()
}

fn treatment_radius_candidates(
    result_candidate_faces: Option<&[cadmpeg_ir::ids::FaceId]>,
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    deleted_edges: &[i64],
) -> Vec<crate::records::DesignEdgeTreatmentRadiusCandidate> {
    treatment_edge_candidates(
        result_candidate_faces,
        inserted_faces,
        result,
        preceding,
        deleted_edges,
    )
    .0
}

fn treatment_edge_candidates(
    result_candidate_faces: Option<&[cadmpeg_ir::ids::FaceId]>,
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    deleted_edges: &[i64],
) -> (
    Vec<crate::records::DesignEdgeTreatmentRadiusCandidate>,
    Vec<i64>,
) {
    let result_boundaries = face_boundary_edge_index(result);
    let preceding_boundaries = face_boundary_edge_index(preceding);
    let supports = treatment_face_supports(inserted_faces, result, preceding, &result_boundaries);
    let deleted_edges = deleted_edges.iter().copied().collect::<HashSet<_>>();
    let candidate_edges = result_candidate_faces
        .into_iter()
        .flatten()
        .filter_map(|face| stable_ref(&face.0))
        .filter_map(|face| result_boundaries.get(&face))
        .flatten()
        .copied()
        .collect::<HashSet<_>>();
    let mut radii_out = Vec::new();
    let mut transitions_out = Vec::new();
    for (inserted, carrier, supports) in supports {
        let Some(inserted_boundary) = result_boundaries.get(&inserted) else {
            continue;
        };
        let mut radii = result
            .surface_radii
            .iter()
            .filter(|candidate| candidate.surface == carrier);
        let radius = radii
            .next()
            .map(|candidate| candidate.radius)
            .filter(|radius| radii.next().is_none() && radius.is_finite() && *radius > 0.0)
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
                    .filter(|edge| deleted_edges.contains(edge))
                {
                    transitions_out.push(*edge);
                    if let Some(radius) = radius {
                        radii_out.push(crate::records::DesignEdgeTreatmentRadiusCandidate {
                            edge_slot: *edge,
                            radius,
                        });
                    }
                }
            }
        }
    }
    radii_out.sort_by(|left, right| {
        left.radius
            .total_cmp(&right.radius)
            .then(left.edge_slot.cmp(&right.edge_slot))
    });
    radii_out
        .dedup_by(|left, right| left.radius == right.radius && left.edge_slot == right.edge_slot);
    transitions_out.sort_unstable();
    transitions_out.dedup();
    (radii_out, transitions_out)
}

fn treatment_face_supports(
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    result_boundaries: &HashMap<i64, HashSet<i64>>,
) -> Vec<(i64, i64, Vec<i64>)> {
    let preceding_faces = preceding.faces.iter().copied().collect::<HashSet<_>>();
    let preceding_surfaces = preceding.surfaces.iter().copied().collect::<HashSet<_>>();
    let result_carriers = unique_face_carriers(result);
    let preceding_carrier_faces = unique_carrier_faces(preceding, &preceding_faces);
    let mut adjacent_faces = HashMap::<i64, Vec<i64>>::new();
    for (face, edges) in result_boundaries {
        for edge in edges {
            adjacent_faces.entry(*edge).or_default().push(*face);
        }
    }
    inserted_faces
        .iter()
        .copied()
        .filter_map(|inserted| {
            let carrier = result_carriers.get(&inserted).copied().flatten()?;
            if preceding_surfaces.contains(&carrier) {
                return None;
            }
            let inserted_boundary = result_boundaries.get(&inserted)?;
            let mut supports = inserted_boundary
                .iter()
                .filter_map(|edge| adjacent_faces.get(edge))
                .flatten()
                .copied()
                .filter(|face| *face != inserted)
                .filter_map(|face| result_carriers.get(&face).copied().flatten())
                .filter_map(|carrier| preceding_carrier_faces.get(&carrier).copied().flatten())
                .collect::<Vec<_>>();
            supports.sort_unstable();
            supports.dedup();
            Some((inserted, carrier, supports))
        })
        .collect()
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
    topology
        .faces
        .iter()
        .filter_map(|face| {
            let loops = face_loops.get(face).copied().flatten()?;
            let edges = loops
                .iter()
                .map(|loop_| loop_coedges.get(loop_).copied().flatten())
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .flatten()
                .map(|coedge| coedge_edges.get(coedge).copied().flatten())
                .collect::<Option<HashSet<_>>>()?;
            Some((*face, edges))
        })
        .collect()
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
    for binding in topology
        .face_surfaces
        .iter()
        .filter(|binding| included_faces.contains(&binding.entity))
    {
        out.entry(binding.carrier)
            .and_modify(|face| *face = None)
            .or_insert(Some(binding.entity));
    }
    out
}

#[cfg(test)]
fn treatment_transition_edge_candidates(
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    deleted_edges: &[i64],
) -> Vec<i64> {
    treatment_edge_candidates(None, inserted_faces, result, preceding, deleted_edges).1
}

fn boundary_edges_in_changes(boundary_edges: &[i64], changes: &[i64]) -> Vec<i64> {
    boundary_edges
        .iter()
        .copied()
        .filter(|edge| changes.contains(edge))
        .collect()
}

fn recipe_selector_candidates(
    structure: Option<&crate::records::DesignEdgeRecipeStructure>,
    contexts: &[crate::records::DesignHistoricalEdgeContext],
) -> Vec<crate::records::DesignEdgeRecipeSelectorContext> {
    let Some(structure) = structure else {
        return Vec::new();
    };
    let selectors = structure
        .sides
        .iter()
        .flat_map(|side| side.entries.iter().map(|entry| entry.selector))
        .collect::<BTreeSet<_>>();
    selectors
        .iter()
        .map(|selector| {
            let clause_entries = structure
                .sides
                .iter()
                .map(|side| {
                    side.entries
                        .iter()
                        .find(|entry| entry.selector == *selector)
                        .cloned()
                })
                .collect::<Vec<_>>();
            let required = clause_entries
                .iter()
                .map(|entry| {
                    entry
                        .as_ref()
                        .map(|entry| i64::from(entry.boundary_edge_count.get()))
                })
                .collect::<Vec<_>>();
            let boundary_count_matching_edge_slots = contexts
                .iter()
                .filter(|context| {
                    let counts = context
                        .incident_loops
                        .iter()
                        .map(|incident| i64::from(incident.boundary_edge_count))
                        .collect::<Vec<_>>();
                    incident_loop_counts_satisfy_sides(&counts, &required)
                })
                .map(|context| context.edge_slot)
                .collect();
            let clause_triplet_edge_slots = clause_entries
                .iter()
                .map(|entry| {
                    entry.as_ref().map(|entry| {
                        entry.topology_triplets.each_ref().map(|triplet| {
                            contexts
                                .iter()
                                .filter(|context| {
                                    context.incident_loops.iter().any(|incident| {
                                        incident.boundary_edge_count
                                            == entry.boundary_edge_count.get()
                                            && triplet.incident_edge_ordinal.is_some_and(
                                                |ordinal| incident.coedge_ordinal == ordinal,
                                            )
                                    })
                                })
                                .map(|context| context.edge_slot)
                                .collect()
                        })
                    })
                })
                .collect::<Vec<_>>();
            let incidence_matching_edge_slots = contexts
                .iter()
                .filter(|context| {
                    clause_entries.iter().flatten().all(|entry| {
                        entry.topology_triplets.iter().all(|triplet| {
                            context.incident_loops.iter().any(|incident| {
                                incident.boundary_edge_count == entry.boundary_edge_count.get()
                                    && triplet
                                        .incident_edge_ordinal
                                        .is_some_and(|ordinal| incident.coedge_ordinal == ordinal)
                            })
                        })
                    })
                })
                .map(|context| context.edge_slot)
                .collect::<Vec<_>>();
            let unique_incidence_edge_slot = match incidence_matching_edge_slots.as_slice() {
                [edge] => Some(*edge),
                _ => None,
            };
            crate::records::DesignEdgeRecipeSelectorContext {
                selector: *selector,
                clause_entries,
                clause_triplet_edge_slots,
                incidence_matching_edge_slots,
                unique_incidence_edge_slot,
                boundary_count_matching_edge_slots,
            }
        })
        .collect()
}

fn historical_edge_context(
    edge: i64,
    topology: &AsmHistoricalTopology,
) -> crate::records::DesignHistoricalEdgeContext {
    let mut incident_loops = topology
        .coedge_topology
        .iter()
        .filter(|coedge| coedge.edge == edge)
        .filter_map(|coedge| {
            let loop_relation = topology
                .loop_coedges
                .iter()
                .find(|relation| relation.owner_ref == coedge.owner_loop)?;
            let ordinal = loop_relation
                .member_refs
                .iter()
                .position(|candidate| *candidate == coedge.coedge)?;
            let boundary_edge_count = u32::try_from(loop_relation.member_refs.len()).ok()?;
            let coedge_ordinal = u32::try_from(ordinal).ok()?;
            let previous_coedge = loop_relation.member_refs.get(
                (ordinal + loop_relation.member_refs.len() - 1) % loop_relation.member_refs.len(),
            )?;
            let next_coedge = loop_relation
                .member_refs
                .get((ordinal + 1) % loop_relation.member_refs.len())?;
            let edge_for_coedge = |slot| {
                topology
                    .coedge_topology
                    .iter()
                    .find(|candidate| candidate.coedge == slot)
                    .map(|candidate| candidate.edge)
            };
            let face_slot = topology
                .face_loops
                .iter()
                .find(|relation| relation.member_refs.contains(&coedge.owner_loop))?
                .owner_ref;
            Some(crate::records::DesignHistoricalEdgeLoopContext {
                coedge_slot: coedge.coedge,
                loop_slot: coedge.owner_loop,
                face_slot,
                boundary_edge_count,
                coedge_ordinal,
                previous_edge_slot: edge_for_coedge(*previous_coedge)?,
                next_edge_slot: edge_for_coedge(*next_coedge)?,
            })
        })
        .collect::<Vec<_>>();
    incident_loops.sort_by_key(|context| context.coedge_slot);
    crate::records::DesignHistoricalEdgeContext {
        edge_slot: edge,
        incident_loops,
    }
}

fn incident_loop_counts_satisfy_sides(counts: &[i64], required: &[Option<i64>]) -> bool {
    let mut available = counts.to_vec();
    required.iter().flatten().all(|required| {
        let Some(index) = available.iter().position(|count| count == required) else {
            return false;
        };
        available.remove(index);
        true
    })
}

fn bind_face_selection(
    selection: &mut cadmpeg_ir::features::FaceSelection,
    scope: &crate::records::DesignParameterScope,
    groups: &[crate::records::DesignConstructionOperandGroup],
    operands: &[crate::records::DesignFaceOperand],
) {
    let cadmpeg_ir::features::FaceSelection::Native(native) = selection else {
        return;
    };
    if native == &scope.id {
        if let Some(resolved) =
            crate::design::feature_project::direct_face_selection(scope, operands)
        {
            if !matches!(resolved, cadmpeg_ir::features::FaceSelection::Native(_)) {
                *selection = resolved;
            }
        }
        return;
    }
    let mut matching_groups = groups.iter().filter(|group| group.id == *native);
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some()
        || group.scope_record_index != scope.record_index
        || crate::ids::native_stream(&group.id) != crate::ids::native_stream(&scope.id)
    {
        return;
    }
    if let Some(resolved) = crate::design::face_resolve::resolved_historical_split_face_target_group(
        scope, group, operands,
    ) {
        *selection = resolved;
        return;
    }
    let Some(stream) = crate::ids::native_stream(&scope.id) else {
        return;
    };
    let mut faces = Vec::new();
    for record_index in &group.members {
        let mut matches = operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
                && operand.record_index == *record_index
        });
        let Some(operand) = matches.next() else {
            return;
        };
        if matches.next().is_some() {
            return;
        }
        let previous_candidates = &operand.preceding_candidate_faces;
        let candidate = match previous_candidates.as_slice() {
            [face] => face,
            _ => {
                let [face] = operand.changed_candidate_faces.as_slice() else {
                    return;
                };
                face
            }
        };
        if faces.contains(candidate) {
            continue;
        }
        if !operand.candidate_faces.contains(candidate) {
            return;
        }
        faces.push(candidate.clone());
    }
    if !faces.is_empty() {
        *selection = cadmpeg_ir::features::FaceSelection::Resolved {
            faces,
            native: native.clone(),
        };
    }
}

fn bind_body_recipe_face_selection(
    selection: &mut cadmpeg_ir::features::FaceSelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::DesignParameterScope,
    groups: &[crate::records::DesignConstructionOperandGroup],
    operands: &[crate::records::DesignBodyRecipeOperand],
) {
    use cadmpeg_ir::features::FaceSelection;

    let FaceSelection::Native(native) = selection else {
        return;
    };
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == *native
            && group.scope_record_index == scope.record_index
            && group.role == 0x0000_0005_0000_0000
            && crate::ids::native_stream(&group.id) == crate::ids::native_stream(&scope.id)
    });
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some() || group.members.is_empty() {
        return;
    }
    let stream = crate::ids::native_stream(&scope.id);
    let mut slots = Vec::new();
    for (ordinal, record_index) in group.members.iter().enumerate() {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return;
        };
        let mut matching_operands = operands.iter().filter(|operand| {
            operand.owner.group() == Some((group.record_index, ordinal))
                && operand.record_index == *record_index
                && crate::ids::native_stream(&operand.id) == stream
        });
        let Some(operand) = matching_operands.next() else {
            return;
        };
        if matching_operands.next().is_some() {
            return;
        }
        let Some(slot) = operand.resolved_face_slot else {
            return;
        };
        if !slots.contains(&slot) {
            slots.push(slot);
        }
    }
    let prefix = feature_input_prefix(feature_id, previous_state_id);
    *selection = FaceSelection::Historical {
        state: crate::design::edge_resolve::feature_input_topology_id(
            feature_id,
            previous_state_id,
        ),
        faces: slots
            .into_iter()
            .map(|slot| crate::ids::history_input_face_id(&prefix, slot))
            .collect(),
        native: native.clone(),
    };
}

fn faces_in_topology(
    candidates: &[cadmpeg_ir::ids::FaceId],
    topology: &AsmHistoricalTopology,
) -> Vec<cadmpeg_ir::ids::FaceId> {
    let faces = topology.faces.iter().copied().collect::<HashSet<_>>();
    candidates
        .iter()
        .filter(|face| stable_ref(&face.0).is_some_and(|slot| faces.contains(&slot)))
        .cloned()
        .collect()
}

fn stable_ref(id: &str) -> Option<i64> {
    id.rsplit_once('#')?
        .1
        .split(':')
        .next()?
        .parse::<i64>()
        .ok()
}

/// Topology families and states containing one requested stable ASM slot.
#[derive(Default)]
struct HistoricalIdentityMembership {
    kinds: HashSet<AsmHistoricalEntityKind>,
    states: Vec<i64>,
}

#[derive(Default)]
struct HistoricalRevisionMembership {
    entity_refs: HashSet<i64>,
    states: Vec<i64>,
}

/// Ambiguity-aware ASM identity data restricted to the Design identities a
/// binding pass requests.
struct HistoricalIdentityIndex {
    identities: HashMap<i64, HistoricalIdentityMembership>,
    revisions: HashMap<i64, HistoricalRevisionMembership>,
}

impl HistoricalIdentityIndex {
    fn build(histories: &[AsmHistory], local_ids: impl IntoIterator<Item = u64>) -> Self {
        let record_refs = local_ids
            .into_iter()
            .filter_map(|local_id| i64::try_from(local_id).ok())
            .collect::<HashSet<_>>();
        let mut identities = HashMap::<i64, HistoricalIdentityMembership>::new();
        let mut revisions = HashMap::<i64, HistoricalRevisionMembership>::new();
        if record_refs.is_empty() {
            return Self {
                identities,
                revisions,
            };
        }
        let ambiguous_states = ambiguous_history_state_ids(histories);
        for state in histories
            .iter()
            .flat_map(|history| &history.states)
            .filter(|state| !ambiguous_states.contains(&state.state_id))
        {
            for version in &state.entity_versions {
                if record_refs.contains(&version.record_ref) {
                    let membership = revisions.entry(version.record_ref).or_default();
                    membership.entity_refs.insert(version.entity_ref);
                    if !membership.states.contains(&state.state_id) {
                        membership.states.push(state.state_id);
                    }
                }
            }
        }
        let versioned_revisions = revisions.keys().copied().collect::<HashSet<_>>();
        let mut reconstructed_revisions = HashSet::new();
        for history in histories.iter().filter(|history| {
            !history.states.is_empty()
                && history
                    .states
                    .iter()
                    .all(|state| state.record_table_complete && state.topology.is_some())
        }) {
            for change in history
                .states
                .iter()
                .flat_map(|state| &state.bulletin_boards)
                .flat_map(|board| &board.changes)
            {
                let Some(record_ref) = change.old_ref.filter(|old| record_refs.contains(old))
                else {
                    continue;
                };
                let entity_ref = change.new_ref.unwrap_or(record_ref);
                revisions
                    .entry(record_ref)
                    .or_default()
                    .entity_refs
                    .insert(entity_ref);
                if !versioned_revisions.contains(&record_ref) {
                    reconstructed_revisions.insert(record_ref);
                }
            }
        }
        let entity_refs = record_refs
            .iter()
            .copied()
            .chain(
                revisions
                    .values()
                    .flat_map(|revision| revision.entity_refs.iter().copied()),
            )
            .collect::<HashSet<_>>();
        for state in histories
            .iter()
            .flat_map(|history| &history.states)
            .filter(|state| !ambiguous_states.contains(&state.state_id))
        {
            let Some(topology) = &state.topology else {
                continue;
            };
            let families: [(AsmHistoricalEntityKind, &[i64]); 12] = [
                (AsmHistoricalEntityKind::Body, &topology.bodies),
                (AsmHistoricalEntityKind::Region, &topology.regions),
                (AsmHistoricalEntityKind::Shell, &topology.shells),
                (AsmHistoricalEntityKind::Face, &topology.faces),
                (AsmHistoricalEntityKind::Loop, &topology.loops),
                (AsmHistoricalEntityKind::Coedge, &topology.coedges),
                (AsmHistoricalEntityKind::Edge, &topology.edges),
                (AsmHistoricalEntityKind::Vertex, &topology.vertices),
                (AsmHistoricalEntityKind::Point, &topology.points),
                (AsmHistoricalEntityKind::Surface, &topology.surfaces),
                (AsmHistoricalEntityKind::Curve, &topology.curves),
                (AsmHistoricalEntityKind::Pcurve, &topology.pcurves),
            ];
            for (kind, members) in families {
                for entity_ref in members
                    .iter()
                    .filter(|entity_ref| entity_refs.contains(entity_ref))
                {
                    let membership = identities.entry(*entity_ref).or_default();
                    membership.kinds.insert(kind);
                    if !membership.states.contains(&state.state_id) {
                        membership.states.push(state.state_id);
                    }
                }
            }
        }
        for record_ref in reconstructed_revisions {
            let Some(revision) = revisions.get_mut(&record_ref) else {
                continue;
            };
            revision.states = revision
                .entity_refs
                .iter()
                .filter_map(|entity_ref| identities.get(entity_ref))
                .flat_map(|membership| membership.states.iter().copied())
                .collect();
            revision.states.sort_unstable();
            revision.states.dedup();
        }
        Self {
            identities,
            revisions,
        }
    }

    fn identity_kind(&self, local_id: u64) -> Option<(AsmHistoricalEntityKind, Vec<i64>)> {
        let entity_ref = i64::try_from(local_id).ok()?;
        let membership = self.identities.get(&entity_ref)?;
        let mut kinds = membership.kinds.iter();
        let kind = *kinds.next()?;
        kinds
            .next()
            .is_none()
            .then(|| (kind, membership.states.clone()))
    }

    fn selection_identity_kind(
        &self,
        local_id: u64,
    ) -> Option<(AsmHistoricalEntityKind, i64, Vec<i64>)> {
        let record_ref = i64::try_from(local_id).ok()?;
        let revision = self.revisions.get(&record_ref);
        if let Some((kind, states)) = self.identity_kind(local_id) {
            return revision
                .is_none_or(|revision| {
                    revision.entity_refs.is_empty()
                        || revision.entity_refs == HashSet::from([record_ref])
                })
                .then_some((kind, record_ref, states));
        }
        let revision = revision?;
        let mut entity_refs = revision.entity_refs.iter();
        let entity_ref = *entity_refs.next()?;
        if entity_refs.next().is_some() {
            return None;
        }
        let (kind, _) = self.identity_kind(u64::try_from(entity_ref).ok()?)?;
        Some((kind, entity_ref, revision.states.clone()))
    }
}

#[cfg(test)]
fn historical_identity_kind(
    histories: &[AsmHistory],
    local_id: u64,
) -> Option<(AsmHistoricalEntityKind, Vec<i64>)> {
    HistoricalIdentityIndex::build(histories, [local_id]).identity_kind(local_id)
}

pub(crate) fn historical_selection_identity_kind(
    histories: &[AsmHistory],
    local_id: u64,
) -> Option<(AsmHistoricalEntityKind, i64, Vec<i64>)> {
    HistoricalIdentityIndex::build(histories, [local_id]).selection_identity_kind(local_id)
}

fn ambiguous_history_state_ids(histories: &[AsmHistory]) -> HashSet<i64> {
    let mut unique = HashSet::new();
    let mut ambiguous = HashSet::new();
    for state in histories.iter().flat_map(|history| &history.states) {
        if !unique.insert(state.state_id) {
            ambiguous.insert(state.state_id);
        }
    }
    ambiguous
}

pub(crate) fn bind_extrude_selection_history(
    members: &mut [DesignExtrudeSelectionMember],
    histories: &[AsmHistory],
) {
    let identities =
        HistoricalIdentityIndex::build(histories, members.iter().map(|member| member.local_id));
    for member in members {
        member.historical_entity_kind = None;
        member.historical_entity_ref = None;
        member.historical_state_ids.clear();
        if let Some((kind, entity_ref, states)) =
            identities.selection_identity_kind(member.local_id)
        {
            member.historical_entity_kind = Some(kind);
            member.historical_entity_ref = Some(entity_ref);
            member.historical_state_ids = states;
        }
    }
}

/// Resolve both identities in nested entity-selection operands against the
/// owning feature's exact input topology.
pub(crate) fn bind_entity_selection_history(
    operands: &mut [crate::records::DesignEntitySelectionOperand],
    scopes: &[crate::records::DesignParameterScope],
    histories: &[AsmHistory],
) {
    let identities = HistoricalIdentityIndex::build(
        histories,
        operands.iter().flat_map(|operand| {
            std::iter::once(operand.primary_identity).chain(operand.secondary_identity)
        }),
    );
    for operand in operands {
        operand.historical_edge_candidates.clear();
        operand.historical_face_candidates.clear();
        operand.resolved_edge_slot = None;
        operand.historical_face_candidates =
            entity_selection_face_candidates(operand.primary_identity, histories);
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
        let Some(previous_state_id) = scope.previous_history_state_id else {
            continue;
        };
        let mut matching_states = histories
            .iter()
            .flat_map(|history| &history.states)
            .filter(|state| state.state_id == previous_state_id);
        let Some(state) = matching_states.next() else {
            continue;
        };
        if matching_states.next().is_some() {
            continue;
        }
        let Some(topology) = &state.topology else {
            continue;
        };
        let Some(secondary_identity) = operand.secondary_identity else {
            continue;
        };
        operand.historical_edge_candidates = entity_selection_edge_candidates(
            [operand.primary_identity, secondary_identity],
            previous_state_id,
            &identities,
            topology,
        );
        operand.resolved_edge_slot =
            unique_entity_selection_edge(&operand.historical_edge_candidates);
    }
}

/// Resolve direct persistent face selections carried by Hole constructions.
pub(crate) fn bind_hole_selection_history(
    scopes: &mut [crate::records::DesignParameterScope],
    histories: &[AsmHistory],
) {
    for scope in scopes {
        let Some(construction) = &mut scope.hole_construction else {
            continue;
        };
        let Some(selection) = &mut construction.face_selection else {
            continue;
        };
        selection.historical_face_candidates.clear();
        selection.historical_face_candidates =
            entity_selection_face_candidates(selection.primary_identity, histories);
    }
}

/// Resolve persistent circular-pattern axis identities in the feature input topology.
pub(crate) fn bind_circular_pattern_axes(
    scopes: &mut [crate::records::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) {
    use crate::records::DesignCircularPatternAxis;
    for scope in scopes {
        let matching_histories = if let Some(history_id) = scope_histories.get(&scope.id) {
            histories
                .iter()
                .filter(|history| history.id == *history_id)
                .collect::<Vec<_>>()
        } else if histories.len() == 1 {
            histories.iter().collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let [history] = matching_histories.as_slice() else {
            continue;
        };
        let input_state_id =
            effective_scope_previous_history_state_id(scope, std::slice::from_ref(*history));
        let Some(construction) = &mut scope.circular_pattern_construction else {
            continue;
        };
        let DesignCircularPatternAxis::HistoricalEdge {
            persistent_identities,
            resolved_origin,
            resolved_direction,
            ..
        } = &mut construction.axis
        else {
            continue;
        };
        *resolved_origin = None;
        *resolved_direction = None;
        let identities = HistoricalIdentityIndex::build(
            std::slice::from_ref(*history),
            persistent_identities.iter().copied(),
        );
        let axes = persistent_identities
            .iter()
            .map(|identity| {
                historical_pattern_identity_axes(*identity, &identities, history, input_state_id)
            })
            .collect::<Vec<_>>();
        if axes.iter().any(Vec::is_empty) {
            continue;
        }
        let mut axes = axes.into_iter().flatten();
        let Some((origin, direction)) = axes.next() else {
            continue;
        };
        if axes.any(|candidate| !same_axis_line((origin, direction), candidate)) {
            continue;
        }
        *resolved_origin = Some(origin);
        *resolved_direction = Some(direction);
    }
}

fn historical_pattern_identity_axes(
    identity: u64,
    identities: &HistoricalIdentityIndex,
    history: &AsmHistory,
    input_state_id: Option<i64>,
) -> Vec<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    if let Some((kind, entity_ref, state_ids)) = identities.selection_identity_kind(identity) {
        let state_ids = if let Some(input_state_id) = input_state_id {
            if !state_ids.contains(&input_state_id) {
                return Vec::new();
            }
            vec![input_state_id]
        } else {
            state_ids
        };
        return historical_pattern_identity_axes_for_selection(
            Some((kind, entity_ref, &state_ids)),
            history,
        );
    }
    let Some(revision) = snapshot_edge_identity_revision(identity, history) else {
        return Vec::new();
    };
    let archived = HistoricalIdentityIndex::build(std::slice::from_ref(history), [revision]);
    let Some((kind, entity_ref, state_ids)) = archived.selection_identity_kind(revision) else {
        return Vec::new();
    };
    let state_ids = if let Some(input_state_id) = input_state_id {
        if !state_ids.contains(&input_state_id) {
            return Vec::new();
        }
        vec![input_state_id]
    } else {
        state_ids
    };
    historical_pattern_identity_axes_for_selection(Some((kind, entity_ref, &state_ids)), history)
}

fn snapshot_edge_identity_revision(identity: u64, history: &AsmHistory) -> Option<u64> {
    let matches = history
        .states
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| record.index == identity)
        .collect::<Vec<_>>();
    let [record] = matches.as_slice() else {
        return None;
    };
    (record.name == "edge")
        .then_some(record.revision_id?)
        .filter(|revision| *revision > 0)
        .and_then(|revision| u64::try_from(revision).ok())
}

fn historical_pattern_identity_axes_for_selection(
    selected: Option<(AsmHistoricalEntityKind, i64, &[i64])>,
    history: &AsmHistory,
) -> Vec<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    let Some((kind, entity_ref, state_ids)) = selected else {
        return Vec::new();
    };
    if state_ids.is_empty() {
        return Vec::new();
    }
    let mut axes = Vec::new();
    let mut matched_state_ids = HashSet::new();
    for state in history
        .states
        .iter()
        .filter(|state| state_ids.contains(&state.state_id))
    {
        matched_state_ids.insert(state.state_id);
        let Some(topology) = state.topology.as_ref() else {
            return Vec::new();
        };
        let state_axes =
            historical_pattern_identity_axis_candidates(Some((kind, entity_ref)), topology)
                .into_iter()
                .filter_map(|(origin, direction)| {
                    let direction = direction.unit()?;
                    (origin.x.is_finite()
                        && origin.y.is_finite()
                        && origin.z.is_finite()
                        && direction.x.is_finite()
                        && direction.y.is_finite()
                        && direction.z.is_finite())
                    .then_some((origin, direction))
                })
                .collect::<Vec<_>>();
        if state_axes.is_empty() {
            return Vec::new();
        }
        axes.extend(state_axes);
    }
    if matched_state_ids.len() != state_ids.len() || axes.is_empty() {
        return Vec::new();
    }
    axes
}

fn historical_pattern_identity_axis_candidates(
    selected: Option<(AsmHistoricalEntityKind, i64)>,
    topology: &AsmHistoricalTopology,
) -> Vec<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    let Some((kind, entity_ref)) = selected else {
        return Vec::new();
    };
    match kind {
        AsmHistoricalEntityKind::Face => historical_face_surface_axis(entity_ref, topology)
            .into_iter()
            .collect(),
        AsmHistoricalEntityKind::Surface => historical_surface_axis(entity_ref, topology)
            .into_iter()
            .collect(),
        _ => historical_identity_edges(kind, entity_ref, topology)
            .into_iter()
            .filter_map(|edge| historical_edge_axis(edge, topology))
            .collect(),
    }
}

fn historical_face_surface_axis(
    face: i64,
    topology: &AsmHistoricalTopology,
) -> Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    let mut bindings = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == face);
    let binding = bindings.next()?;
    bindings
        .next()
        .is_none()
        .then(|| historical_surface_axis(binding.carrier, topology))?
}

fn historical_surface_axis(
    surface: i64,
    topology: &AsmHistoricalTopology,
) -> Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    let candidates = topology
        .surface_axes
        .iter()
        .filter(|axis| axis.surface == surface)
        .map(|axis| (axis.origin, axis.direction))
        .chain(
            topology
                .surface_planes
                .iter()
                .filter(|plane| plane.surface == surface)
                .map(|plane| (plane.origin, plane.normal)),
        )
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(crate) fn same_axis_line(
    left: (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
    right: (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
) -> bool {
    let direction_dot = left.1.dot(right.1);
    if (direction_dot.abs() - 1.0).abs() > 1.0e-9 {
        return false;
    }
    let distance = right.0.vector_from(left.0).cross(left.1).norm();
    distance.is_finite() && distance <= 1.0e-8
}

/// Bind persistent Mirror plane selections to exact planes in the selected
/// historical topology.
pub(crate) fn bind_mirror_selection_planes(
    scopes: &mut [crate::records::DesignParameterScope],
    groups: &[crate::records::DesignConstructionOperandGroup],
    operands: &[crate::records::DesignEntitySelectionOperand],
    identities: &[crate::records::DesignConstructionOperandIdentity],
    histories: &[AsmHistory],
) {
    for scope in scopes {
        let stream = crate::ids::native_stream(&scope.id);
        let Some(construction) = scope.mirror_construction.as_mut() else {
            continue;
        };
        construction.plane_origin = None;
        construction.plane_normal = None;
        let (Some(selection_record_index), Some(state_id), Some(previous_state_id)) = (
            construction.plane_selection_record_index,
            scope.history_state_id,
            scope.previous_history_state_id,
        ) else {
            continue;
        };
        let Some(_) = unique_history_state_pair(histories, state_id, previous_state_id) else {
            continue;
        };
        let mut matching_groups = groups.iter().filter(|group| {
            crate::ids::native_stream(&group.id) == stream
                && group.scope_record_index == scope.record_index
                && group.record_index == construction.plane_group_record_index
                && group.role == 0x0000_0005_0000_0000
                && group.members == [selection_record_index]
        });
        let Some(group) = matching_groups.next() else {
            continue;
        };
        if matching_groups.next().is_some() {
            continue;
        }
        let mut matching_operands = operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == stream
                && operand.scope_record_index == scope.record_index
                && operand.group_record_index == group.record_index
                && operand.group_member_ordinal == 0
                && operand.record_index == selection_record_index
        });
        let Some(operand) = matching_operands.next() else {
            continue;
        };
        if matching_operands.next().is_some() {
            continue;
        }
        let mut matching_identities = identities.iter().filter(|identity| {
            crate::ids::native_stream(&identity.id) == stream
                && identity.group_record_index == group.record_index
        });
        let identity = matching_identities.next();
        let duplicate_identity = matching_identities.next();
        if duplicate_identity.is_some() {
            continue;
        }
        let persistent_candidates = identity
            .and_then(|identity| identity.persistent_identity.as_ref())
            .map_or_else(Vec::new, |identity| {
                entity_selection_face_candidates(identity.local_id, histories)
            });
        let Some(candidate) = unique_mirror_plane_candidate(
            entity_selection_face_candidates(operand.primary_identity, histories),
            persistent_candidates,
        ) else {
            continue;
        };
        let Some(plane) = historical_mirror_plane(&candidate, previous_state_id, histories) else {
            continue;
        };
        let norm = (plane.normal.x * plane.normal.x
            + plane.normal.y * plane.normal.y
            + plane.normal.z * plane.normal.z)
            .sqrt();
        if !norm.is_finite() || (norm - 1.0).abs() > 1.0e-9 {
            continue;
        }
        construction.plane_origin = Some(plane.origin);
        construction.plane_normal = Some(plane.normal);
    }
}

fn unique_mirror_plane_candidate(
    mut primary: Vec<crate::records::DesignEntitySelectionFaceCandidate>,
    persistent: Vec<crate::records::DesignEntitySelectionFaceCandidate>,
) -> Option<crate::records::DesignEntitySelectionFaceCandidate> {
    primary.sort_by(|left, right| left.history_id.cmp(&right.history_id));
    primary.dedup();
    let context_histories = primary
        .iter()
        .map(|candidate| candidate.history_id.as_str())
        .collect::<HashSet<_>>();
    let mut persistent = persistent
        .into_iter()
        .filter(|candidate| context_histories.contains(candidate.history_id.as_str()))
        .collect::<Vec<_>>();
    persistent.sort_by(|left, right| left.history_id.cmp(&right.history_id));
    persistent.dedup();
    match persistent.as_slice() {
        [candidate] => Some(candidate.clone()),
        [] => match primary.as_slice() {
            [candidate] => Some(candidate.clone()),
            _ => None,
        },
        _ => None,
    }
}

#[derive(Clone)]
struct HistoricalMirrorPlane {
    origin: cadmpeg_ir::math::Point3,
    normal: cadmpeg_ir::math::Vector3,
}

fn historical_mirror_plane(
    candidate: &crate::records::DesignEntitySelectionFaceCandidate,
    preferred_state_id: i64,
    histories: &[AsmHistory],
) -> Option<HistoricalMirrorPlane> {
    if candidate.historical_state_ids.contains(&preferred_state_id) {
        return historical_mirror_plane_in_state(candidate, preferred_state_id, histories);
    }
    let mut resolved = None;
    for state_id in &candidate.historical_state_ids {
        let plane = historical_mirror_plane_in_state(candidate, *state_id, histories)?;
        if resolved
            .as_ref()
            .is_some_and(|exact: &HistoricalMirrorPlane| !mirror_planes_coincident(exact, &plane))
        {
            return None;
        }
        resolved = Some(plane);
    }
    resolved
}

fn mirror_planes_coincident(left: &HistoricalMirrorPlane, right: &HistoricalMirrorPlane) -> bool {
    let dot = left.normal.dot(right.normal);
    let normal_distance = right.origin.vector_from(left.origin).dot(left.normal);
    (dot.abs() - 1.0).abs() <= 1.0e-9 && normal_distance.abs() <= 1.0e-8
}

fn historical_mirror_plane_in_state(
    candidate: &crate::records::DesignEntitySelectionFaceCandidate,
    state_id: i64,
    histories: &[AsmHistory],
) -> Option<HistoricalMirrorPlane> {
    let mut matching_histories = histories
        .iter()
        .filter(|history| history.id == candidate.history_id);
    let history = matching_histories.next()?;
    if matching_histories.next().is_some() {
        return None;
    }
    let mut matching_states = history
        .states
        .iter()
        .filter(|state| state.state_id == state_id);
    let state = matching_states.next()?;
    if matching_states.next().is_some() {
        return None;
    }
    let topology = state.topology.as_ref()?;
    if candidate.historical_entity_kind == AsmHistoricalEntityKind::Loop {
        return historical_loop_plane(candidate.historical_entity_ref, topology);
    }
    let mut bindings = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == candidate.face_slot);
    let binding = bindings.next()?;
    if bindings.next().is_some() {
        return None;
    }
    let mut planes = topology
        .surface_planes
        .iter()
        .filter(|plane| plane.surface == binding.carrier);
    let plane = planes.next()?;
    planes.next().is_none().then_some(HistoricalMirrorPlane {
        origin: plane.origin,
        normal: plane.normal,
    })
}

fn historical_loop_plane(
    loop_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Option<HistoricalMirrorPlane> {
    let mut loop_relations = topology
        .loop_coedges
        .iter()
        .filter(|relation| relation.owner_ref == loop_ref);
    let relation = loop_relations.next()?;
    if loop_relations.next().is_some() || relation.member_refs.is_empty() {
        return None;
    }
    let mut planes = Vec::with_capacity(relation.member_refs.len());
    for coedge_ref in &relation.member_refs {
        let mut coedges = topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.coedge == *coedge_ref && coedge.owner_loop == loop_ref);
        let coedge = coedges.next()?;
        if coedges.next().is_some() {
            return None;
        }
        let mut bindings = topology
            .edge_curves
            .iter()
            .filter(|binding| binding.entity == coedge.edge);
        let binding = bindings.next()?;
        if bindings.next().is_some() {
            return None;
        }
        let curve = binding.carrier?;
        let mut axes = topology
            .curve_axes
            .iter()
            .filter(|axis| axis.curve == curve);
        let axis = axes.next()?;
        if axes.next().is_some() {
            return None;
        }
        let norm = (axis.direction.x * axis.direction.x
            + axis.direction.y * axis.direction.y
            + axis.direction.z * axis.direction.z)
            .sqrt();
        if !norm.is_finite() || (norm - 1.0).abs() > 1.0e-9 {
            return None;
        }
        planes.push(HistoricalMirrorPlane {
            origin: axis.origin,
            normal: axis.direction,
        });
    }
    let [first, remaining @ ..] = planes.as_slice() else {
        return None;
    };
    remaining
        .iter()
        .all(|candidate| mirror_planes_coincident(first, candidate))
        .then_some(first.clone())
}

fn entity_selection_face_candidates(
    local_id: u64,
    histories: &[AsmHistory],
) -> Vec<crate::records::DesignEntitySelectionFaceCandidate> {
    use crate::records::DesignEntitySelectionFaceCandidate;

    histories
        .iter()
        .filter_map(|history| {
            let identities =
                HistoricalIdentityIndex::build(std::slice::from_ref(history), [local_id]);
            let (kind, entity_ref, state_ids) = identities.selection_identity_kind(local_id)?;
            let mut face_slot = None;
            for state_id in &state_ids {
                let mut states = history
                    .states
                    .iter()
                    .filter(|state| state.state_id == *state_id);
                let state = states.next()?;
                if states.next().is_some() {
                    return None;
                }
                let topology = state.topology.as_ref()?;
                let mut faces = historical_identity_faces(kind, entity_ref, topology).into_iter();
                let state_face = faces.next()?;
                if faces.next().is_some() || face_slot.is_some_and(|face| face != state_face) {
                    return None;
                }
                face_slot = Some(state_face);
            }
            Some(DesignEntitySelectionFaceCandidate {
                history_id: history.id.clone(),
                historical_entity_kind: kind,
                historical_entity_ref: entity_ref,
                historical_state_ids: state_ids,
                face_slot: face_slot?,
            })
        })
        .collect()
}

fn entity_selection_edge_candidates(
    identities: [u64; 2],
    previous_state_id: i64,
    history_identities: &HistoricalIdentityIndex,
    topology: &AsmHistoricalTopology,
) -> Vec<crate::records::DesignEntitySelectionEdgeCandidate> {
    use crate::records::DesignEntitySelectionEdgeCandidate;

    identities
        .into_iter()
        .enumerate()
        .filter_map(|(identity_ordinal, local_id)| {
            let (kind, entity_ref, states) =
                history_identities.selection_identity_kind(local_id)?;
            states.contains(&previous_state_id).then_some(())?;
            let mut edge_slots = historical_identity_edges(kind, entity_ref, topology)
                .into_iter()
                .collect::<Vec<_>>();
            edge_slots.sort_unstable();
            (!edge_slots.is_empty()).then_some(DesignEntitySelectionEdgeCandidate {
                identity_ordinal: u32::try_from(identity_ordinal)
                    .expect("two identity ordinals fit u32"),
                local_id,
                historical_entity_kind: kind,
                historical_entity_ref: entity_ref,
                edge_slots,
            })
        })
        .collect()
}

fn unique_entity_selection_edge(
    candidates: &[crate::records::DesignEntitySelectionEdgeCandidate],
) -> Option<i64> {
    let first = candidates.first()?;
    let mut intersection = first.edge_slots.iter().copied().collect::<BTreeSet<_>>();
    for candidate in &candidates[1..] {
        intersection.retain(|edge| candidate.edge_slots.contains(edge));
    }
    let mut intersection = intersection.into_iter();
    let edge = intersection.next()?;
    intersection.next().is_none().then_some(edge)
}

pub(crate) fn bind_edge_identity_history(
    operands: &mut [DesignEdgeIdentityOperand],
    identities: &[crate::records::DesignConstructionOperandIdentity],
    scopes: &[crate::records::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) {
    struct EdgeTreatmentTransitionCandidates {
        radii: Vec<crate::records::DesignEdgeTreatmentRadiusCandidate>,
        treatment_edges: Vec<i64>,
        deleted_edges: Vec<i64>,
    }

    if projection_was_finalized(histories) {
        return;
    }
    let mut compact_group_counts = HashMap::<(String, u32, u32), Option<usize>>::new();
    for operand in operands.iter() {
        let Some(stream) = crate::ids::native_stream(&operand.id) else {
            continue;
        };
        compact_group_counts
            .entry((
                stream.to_owned(),
                operand.scope_record_index,
                operand.group_record_index,
            ))
            .and_modify(|count| {
                *count = count.and_then(|count| operand.compact_layout.then_some(count + 1));
            })
            .or_insert(operand.compact_layout.then_some(1));
    }
    let local_ids = operands
        .iter()
        .map(|operand| operand.local_id)
        .chain(identities.iter().filter_map(|identity| {
            identity
                .persistent_identity
                .as_ref()
                .map(|persistent| persistent.local_id)
        }))
        .collect::<Vec<_>>();
    let history_identities = HistoricalIdentityIndex::build(histories, local_ids.iter().copied());
    let identities_by_history = histories
        .iter()
        .map(|history| {
            (
                history.id.as_str(),
                HistoricalIdentityIndex::build(
                    std::slice::from_ref(history),
                    local_ids.iter().copied(),
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut treatment_candidates_by_transition =
        HashMap::<(String, i64, i64), EdgeTreatmentTransitionCandidates>::new();
    for operand in operands {
        operand.historical_entity_kind = None;
        operand.historical_entity_ref = None;
        operand.historical_state_ids.clear();
        operand.treatment_radius_candidates.clear();
        operand.transition_edge_candidates.clear();
        operand.resolved_edge_slots.clear();
        operand.resolved_edge_slot = None;
        operand.resolution_identity_id = None;
        let Some(stream) = crate::ids::native_stream(&operand.id) else {
            continue;
        };
        let mut matching_scopes = scopes.iter().filter(|scope| {
            crate::ids::native_stream(&scope.id) == Some(stream)
                && scope.record_index == operand.scope_record_index
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let Some(previous_state_id) = scope.previous_history_state_id else {
            continue;
        };
        let current_state_id = scope.history_state_id;
        let bound_history = bound_scope_history(&scope.id, scope_histories, histories);
        let scoped_identities = bound_history
            .and_then(|history| identities_by_history.get(history.id.as_str()))
            .unwrap_or(&history_identities);
        if let Some((kind, entity_ref, states)) = scoped_identities
            .selection_identity_kind(operand.local_id)
            .filter(|(_, _, states)| states.contains(&previous_state_id))
        {
            operand.historical_entity_kind = Some(kind);
            operand.historical_entity_ref = Some(entity_ref);
            operand.historical_state_ids = states;
        }
        let Some(history) = bound_history else {
            continue;
        };
        let mut previous_states = history
            .states
            .iter()
            .filter(|state| state.state_id == previous_state_id);
        let Some(previous_state) = previous_states.next() else {
            continue;
        };
        if previous_states.next().is_some() {
            continue;
        }
        let Some(topology) = previous_state.topology.as_ref() else {
            continue;
        };
        if let Some(current_state_id) = current_state_id {
            let mut current_states = history
                .states
                .iter()
                .filter(|state| state.state_id == current_state_id);
            let current_state = current_states.next();
            if current_states.next().is_none()
                && current_state
                    .is_some_and(|state| history_state_reaches(history, state, previous_state_id))
            {
                let key = (history.id.clone(), current_state_id, previous_state_id);
                if !treatment_candidates_by_transition.contains_key(&key) {
                    if let Some(result) = current_state.and_then(|state| state.topology.as_ref()) {
                        let preceding_faces =
                            topology.faces.iter().copied().collect::<HashSet<_>>();
                        let inserted_faces = result
                            .faces
                            .iter()
                            .copied()
                            .filter(|face| !preceding_faces.contains(face))
                            .collect::<Vec<_>>();
                        let result_edges = result.edges.iter().copied().collect::<HashSet<_>>();
                        let deleted_edges = topology
                            .edges
                            .iter()
                            .copied()
                            .filter(|edge| !result_edges.contains(edge))
                            .collect::<Vec<_>>();
                        let (radii, treatment_edges) = treatment_edge_candidates(
                            None,
                            &inserted_faces,
                            result,
                            topology,
                            &deleted_edges,
                        );
                        treatment_candidates_by_transition.insert(
                            key.clone(),
                            EdgeTreatmentTransitionCandidates {
                                radii,
                                treatment_edges,
                                deleted_edges,
                            },
                        );
                    }
                }
                if let Some(candidates) = treatment_candidates_by_transition.get(&key) {
                    operand
                        .treatment_radius_candidates
                        .clone_from(&candidates.radii);
                    let mut treatment_edges = candidates.treatment_edges.clone();
                    // The geometric chain is transition-scoped. The deleted-
                    // set fallback is group-scoped because its proof depends
                    // on this operand group's compact member count.
                    if treatment_edges.is_empty() {
                        let is_edge_treatment = matches!(
                            crate::design::design_feature_family(&scope.kind),
                            Some(
                                crate::design::DesignFeatureFamily::Fillet
                                    | crate::design::DesignFeatureFamily::Chamfer
                            )
                        );
                        let compact_member_count = compact_group_counts
                            .get(&(
                                stream.to_owned(),
                                operand.scope_record_index,
                                operand.group_record_index,
                            ))
                            .copied()
                            .flatten();
                        treatment_edges = complete_compact_edge_treatment_deletions(
                            is_edge_treatment,
                            compact_member_count,
                            &candidates.deleted_edges,
                        );
                    }
                    operand
                        .transition_edge_candidates
                        .clone_from(&treatment_edges);
                }
            }
        }
        let direct = operand
            .historical_entity_kind
            .zip(operand.historical_entity_ref)
            .filter(|_| operand.historical_state_ids.contains(&previous_state_id))
            .and_then(|(kind, entity_ref)| historical_identity_edge(kind, entity_ref, topology));
        if let Some(edge) = direct {
            operand.resolved_edge_slot = Some(edge);
            operand.resolution_identity_id = Some(operand.id.clone());
            continue;
        }
        let mut resolved = identities.iter().filter_map(|identity| {
            (crate::ids::native_stream(&identity.id) == Some(stream)
                && identity.group_record_index == operand.group_record_index)
                .then_some(identity)?;
            let persistent = identity.persistent_identity.as_ref()?;
            let (kind, entity_ref, states) =
                scoped_identities.selection_identity_kind(persistent.local_id)?;
            states.contains(&previous_state_id).then_some(())?;
            Some((
                historical_identity_edge(kind, entity_ref, topology)?,
                identity.id.as_str(),
            ))
        });
        let Some((edge, identity_id)) = resolved.next() else {
            continue;
        };
        if resolved.any(|candidate| candidate.0 != edge) {
            continue;
        }
        operand.resolved_edge_slot = Some(edge);
        operand.resolution_identity_id = Some(identity_id.to_owned());
    }
}

fn complete_compact_edge_treatment_deletions(
    is_edge_treatment: bool,
    compact_member_count: Option<usize>,
    deleted_edges: &[i64],
) -> Vec<i64> {
    if is_edge_treatment
        && !deleted_edges.is_empty()
        && compact_member_count == Some(deleted_edges.len())
    {
        deleted_edges.to_vec()
    } else {
        Vec::new()
    }
}

/// Resolve a class-297 edge-treatment member whose persistent local identity
/// names the member's embedded bounded-face recipe. The rule selects every
/// deleted treatment edge on the recipe's exact preceding support face.
pub(crate) fn bind_edge_identity_bounded_face_rules(
    operands: &mut [DesignEdgeIdentityOperand],
    face_operands: &[crate::records::DesignFaceOperand],
) {
    use crate::records::ConstructionRecipeKind;

    for operand in operands {
        operand.resolved_edge_slots.clear();
        if operand.resolved_edge_slot.is_some() {
            continue;
        }
        let matches = face_operands
            .iter()
            .filter(|face| {
                crate::ids::native_stream(&face.id) == crate::ids::native_stream(&operand.id)
                    && face.scope_record_index == operand.scope_record_index
                    && face.group_record_index == Some(operand.group_record_index)
                    && face.group_member_ordinal == Some(operand.group_member_ordinal)
                    && face.record_index == operand.record_index
                    && face.class_tag == operand.class_tag
                    && face.recipe_kind == ConstructionRecipeKind::BoundedFace
                    && u64::from(face.recipe_record_index) == operand.local_id
            })
            .collect::<Vec<_>>();
        let [face] = matches.as_slice() else { continue };
        let [support] = face.historical_support_contexts.as_slice() else {
            continue;
        };
        if support.preceding_face_slots.is_empty()
            || support.changed_preceding_face_slots != support.preceding_face_slots
            || support.preceding_face_boundaries.len() != support.preceding_face_slots.len()
            || support
                .preceding_face_slots
                .iter()
                .collect::<HashSet<_>>()
                .len()
                != support.preceding_face_slots.len()
            || support.preceding_face_boundaries.iter().any(|boundary| {
                support
                    .preceding_face_boundaries
                    .iter()
                    .filter(|candidate| candidate.face_slot == boundary.face_slot)
                    .count()
                    != 1
                    || !support.preceding_face_slots.contains(&boundary.face_slot)
                    || boundary.loops.iter().any(|loop_| {
                        loop_.edge_slots.len() != loop_.coedge_slots.len()
                            || loop_.edge_slots.is_empty()
                    })
            })
        {
            continue;
        }
        let transition = operand
            .transition_edge_candidates
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        if transition.is_empty() {
            continue;
        }
        let mut seen = HashSet::new();
        operand.resolved_edge_slots = support
            .preceding_face_boundaries
            .iter()
            .flat_map(|boundary| &boundary.loops)
            .flat_map(|loop_| &loop_.edge_slots)
            .copied()
            .filter(|edge| transition.contains(edge) && seen.insert(*edge))
            .collect();
        if !operand.resolved_edge_slots.is_empty() {
            operand.resolution_identity_id = Some(face.id.clone());
        }
    }
}

fn historical_identity_edge(
    kind: AsmHistoricalEntityKind,
    entity_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Option<i64> {
    let candidates = historical_identity_edges(kind, entity_ref, topology);
    let mut candidates = candidates.into_iter();
    let edge = candidates.next()?;
    candidates.next().is_none().then_some(edge)
}

fn historical_identity_edges(
    kind: AsmHistoricalEntityKind,
    entity_ref: i64,
    topology: &AsmHistoricalTopology,
) -> HashSet<i64> {
    let mut candidates = HashSet::new();
    match kind {
        AsmHistoricalEntityKind::Edge => {
            if topology.edges.contains(&entity_ref) {
                candidates.insert(entity_ref);
            }
        }
        AsmHistoricalEntityKind::Coedge => {
            candidates.extend(
                topology
                    .coedge_topology
                    .iter()
                    .filter(|coedge| coedge.coedge == entity_ref)
                    .map(|coedge| coedge.edge),
            );
        }
        AsmHistoricalEntityKind::Pcurve => {
            let coedges = topology
                .coedge_pcurves
                .iter()
                .filter(|binding| binding.carrier == Some(entity_ref))
                .map(|binding| binding.entity)
                .collect::<HashSet<_>>();
            candidates.extend(
                topology
                    .coedge_topology
                    .iter()
                    .filter(|coedge| coedges.contains(&coedge.coedge))
                    .map(|coedge| coedge.edge),
            );
        }
        AsmHistoricalEntityKind::Curve => {
            candidates.extend(
                topology
                    .edge_curves
                    .iter()
                    .filter(|binding| binding.carrier == Some(entity_ref))
                    .map(|binding| binding.entity),
            );
        }
        AsmHistoricalEntityKind::Vertex | AsmHistoricalEntityKind::Point => {
            let vertices = if kind == AsmHistoricalEntityKind::Vertex {
                HashSet::from([entity_ref])
            } else {
                topology
                    .vertex_points
                    .iter()
                    .filter(|binding| binding.carrier == entity_ref)
                    .map(|binding| binding.entity)
                    .collect()
            };
            candidates.extend(
                topology
                    .edge_vertices
                    .iter()
                    .filter(|edge| {
                        vertices.contains(&edge.start_vertex) || vertices.contains(&edge.end_vertex)
                    })
                    .map(|edge| edge.edge),
            );
        }
        AsmHistoricalEntityKind::Body
        | AsmHistoricalEntityKind::Region
        | AsmHistoricalEntityKind::Shell
        | AsmHistoricalEntityKind::Face
        | AsmHistoricalEntityKind::Loop
        | AsmHistoricalEntityKind::Surface => {}
    }
    candidates
}

fn historical_identity_faces(
    kind: AsmHistoricalEntityKind,
    entity_ref: i64,
    topology: &AsmHistoricalTopology,
) -> HashSet<i64> {
    let mut carriers = HashSet::new();
    match kind {
        AsmHistoricalEntityKind::Face => {
            carriers.insert(entity_ref);
            return carriers;
        }
        AsmHistoricalEntityKind::Loop => {
            carriers.insert(entity_ref);
        }
        AsmHistoricalEntityKind::Coedge => {
            carriers.extend(
                topology
                    .loop_coedges
                    .iter()
                    .filter(|relation| relation.member_refs.contains(&entity_ref))
                    .map(|relation| relation.owner_ref),
            );
        }
        AsmHistoricalEntityKind::Pcurve => {
            let coedges = topology
                .coedge_pcurves
                .iter()
                .filter(|binding| binding.carrier == Some(entity_ref))
                .map(|binding| binding.entity)
                .collect::<HashSet<_>>();
            carriers.extend(
                topology
                    .loop_coedges
                    .iter()
                    .filter(|relation| {
                        relation
                            .member_refs
                            .iter()
                            .any(|coedge| coedges.contains(coedge))
                    })
                    .map(|relation| relation.owner_ref),
            );
        }
        AsmHistoricalEntityKind::Surface => {
            return topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.carrier == entity_ref)
                .map(|binding| binding.entity)
                .collect();
        }
        AsmHistoricalEntityKind::Body
        | AsmHistoricalEntityKind::Region
        | AsmHistoricalEntityKind::Shell
        | AsmHistoricalEntityKind::Edge
        | AsmHistoricalEntityKind::Vertex
        | AsmHistoricalEntityKind::Point
        | AsmHistoricalEntityKind::Curve => return HashSet::new(),
    }
    topology
        .face_loops
        .iter()
        .filter(|relation| {
            relation
                .member_refs
                .iter()
                .any(|loop_| carriers.contains(loop_))
        })
        .map(|relation| relation.owner_ref)
        .collect()
}

fn affected_body_refs(
    current: &AsmDeltaState,
    previous: Option<&AsmDeltaState>,
) -> Option<Vec<i64>> {
    let transition = current.transition.as_ref()?;
    if transition.previous_state_id != previous.map(|state| state.state_id) {
        return None;
    }
    let current_topology = current.topology.as_ref()?;
    let current_changes = changed_family_refs(&transition.topology, false);
    let mut affected = bodies_intersecting(current_topology, &current_changes)?;
    if let Some(previous) = previous {
        let previous_topology = previous.topology.as_ref()?;
        let deleted = changed_family_refs(&transition.topology, true);
        affected.extend(bodies_intersecting(previous_topology, &deleted)?);
    }
    Some(affected.into_iter().collect())
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
    families
        .into_iter()
        .flat_map(|family| {
            if deleted {
                family.deleted.clone()
            } else {
                family
                    .inserted
                    .iter()
                    .chain(&family.updated)
                    .copied()
                    .collect()
            }
        })
        .collect()
}

fn bodies_intersecting(
    topology: &AsmHistoricalTopology,
    changed: &BTreeSet<i64>,
) -> Option<BTreeSet<i64>> {
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
    let mut affected = BTreeSet::new();
    for &body in &topology.bodies {
        let mut closure = BTreeSet::from([body]);
        for &region in *body_regions.get(&body)? {
            closure.insert(region);
            for &shell in *region_shells.get(&region)? {
                closure.insert(shell);
                let mut shell_edges = shell_wire_edges.get(&shell)?.to_vec();
                let mut shell_vertices = shell_free_vertices.get(&shell)?.to_vec();
                for &face in *shell_faces.get(&shell)? {
                    closure.insert(face);
                    closure.insert(*face_surfaces.get(&face)?);
                    for &loop_ in *face_loops.get(&face)? {
                        closure.insert(loop_);
                        for &coedge in *loop_coedges.get(&loop_)? {
                            closure.insert(coedge);
                            let coedge_topology = coedges.get(&coedge)?;
                            shell_edges.push(coedge_topology.edge);
                            if let Some(pcurve) = coedge_pcurves.get(&coedge).copied().flatten() {
                                closure.insert(pcurve);
                            }
                        }
                    }
                }
                for edge in shell_edges {
                    closure.insert(edge);
                    let edge_topology = edges.get(&edge)?;
                    shell_vertices.extend([edge_topology.start_vertex, edge_topology.end_vertex]);
                    if let Some(curve) = edge_curves.get(&edge).copied().flatten() {
                        closure.insert(curve);
                    }
                }
                for vertex in shell_vertices {
                    closure.insert(vertex);
                    closure.insert(*vertex_points.get(&vertex)?);
                }
            }
        }
        if !closure.is_disjoint(changed) {
            affected.insert(body);
        }
    }
    Some(affected)
}

fn relation_map(items: &[AsmHistoricalRelation]) -> HashMap<i64, &[i64]> {
    items
        .iter()
        .map(|relation| (relation.owner_ref, relation.member_refs.as_slice()))
        .collect()
}

pub(crate) fn historical_topology(
    brep: &cadmpeg_asm::brep::AsmBrep,
) -> Option<AsmHistoricalTopology> {
    fn entity_ref(id: &str) -> Option<i64> {
        id.rsplit_once('#')?
            .1
            .split(':')
            .next()?
            .parse::<i64>()
            .ok()
    }

    fn refs<'a>(ids: impl Iterator<Item = &'a str>) -> Option<Vec<i64>> {
        ids.map(entity_ref).collect()
    }

    fn relations<'a>(
        items: impl Iterator<Item = (&'a str, Vec<&'a str>)>,
    ) -> Option<Vec<AsmHistoricalRelation>> {
        items
            .map(|(owner, members)| {
                Some(AsmHistoricalRelation {
                    owner_ref: entity_ref(owner)?,
                    member_refs: refs(members.into_iter())?,
                })
            })
            .collect()
    }

    let mut surface_radii = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            use cadmpeg_ir::geometry::SurfaceGeometry;
            let radius = match &surface.geometry {
                SurfaceGeometry::Cylinder { radius, .. }
                | SurfaceGeometry::Sphere { radius, .. } => *radius,
                SurfaceGeometry::Torus { minor_radius, .. } => *minor_radius,
                _ => return None,
            };
            Some(crate::history_records::AsmHistoricalSurfaceRadius {
                surface: entity_ref(&surface.id.0)?,
                radius: radius.abs(),
            })
        })
        .collect::<Vec<_>>();
    for procedural in &brep.procedural_surfaces {
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend { radius, .. } =
            &procedural.definition
        else {
            continue;
        };
        let cadmpeg_ir::geometry::BlendRadiusLaw::Constant { signed_radius } = radius else {
            continue;
        };
        let Some(surface) = entity_ref(&procedural.surface.0) else {
            continue;
        };
        surface_radii.retain(|candidate| candidate.surface != surface);
        surface_radii.push(crate::history_records::AsmHistoricalSurfaceRadius {
            surface,
            radius: signed_radius.abs(),
        });
    }
    surface_radii.sort_by_key(|candidate| candidate.surface);
    let mut surface_cylinders = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            let cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
                origin,
                axis,
                radius,
                ..
            } = surface.geometry
            else {
                return None;
            };
            Some(crate::history_records::AsmHistoricalCylinder {
                surface: entity_ref(&surface.id.0)?,
                origin,
                axis,
                radius: radius.abs(),
            })
        })
        .collect::<Vec<_>>();
    surface_cylinders.sort_by_key(|candidate| candidate.surface);
    let mut surface_planes = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            let cadmpeg_ir::geometry::SurfaceGeometry::Plane { origin, normal, .. } =
                surface.geometry
            else {
                return None;
            };
            Some(crate::history_records::AsmHistoricalPlane {
                surface: entity_ref(&surface.id.0)?,
                origin,
                normal,
            })
        })
        .collect::<Vec<_>>();
    surface_planes.sort_by_key(|candidate| candidate.surface);
    let mut surface_axes = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            use cadmpeg_ir::geometry::SurfaceGeometry;
            let (origin, direction) = match surface.geometry {
                SurfaceGeometry::Cylinder { origin, axis, .. }
                | SurfaceGeometry::Cone { origin, axis, .. } => (origin, axis),
                SurfaceGeometry::Torus { center, axis, .. } => (center, axis),
                _ => return None,
            };
            Some(crate::history_records::AsmHistoricalSurfaceAxis {
                surface: entity_ref(&surface.id.0)?,
                origin,
                direction,
            })
        })
        .collect::<Vec<_>>();
    surface_axes.sort_by_key(|candidate| candidate.surface);

    Some(AsmHistoricalTopology {
        bodies: refs(brep.bodies.iter().map(|entity| entity.id.0.as_str()))?,
        regions: refs(brep.regions.iter().map(|entity| entity.id.0.as_str()))?,
        shells: refs(brep.shells.iter().map(|entity| entity.id.0.as_str()))?,
        faces: refs(brep.faces.iter().map(|entity| entity.id.0.as_str()))?,
        loops: refs(brep.loops.iter().map(|entity| entity.id.0.as_str()))?,
        coedges: refs(brep.coedges.iter().map(|entity| entity.id.0.as_str()))?,
        edges: refs(brep.edges.iter().map(|entity| entity.id.0.as_str()))?,
        vertices: refs(brep.vertices.iter().map(|entity| entity.id.0.as_str()))?,
        points: refs(brep.points.iter().map(|entity| entity.id.0.as_str()))?,
        surfaces: refs(brep.surfaces.iter().map(|entity| entity.id.0.as_str()))?,
        surface_radii,
        surface_cylinders,
        surface_planes,
        surface_axes,
        curves: refs(brep.curves.iter().map(|entity| entity.id.0.as_str()))?,
        curve_axes: brep
            .curves
            .iter()
            .filter_map(|curve| {
                use cadmpeg_ir::geometry::CurveGeometry;
                let (origin, direction) = match curve.geometry {
                    CurveGeometry::Line { origin, direction } => (origin, direction),
                    CurveGeometry::Circle { center, axis, .. }
                    | CurveGeometry::Ellipse { center, axis, .. } => (center, axis),
                    _ => return None,
                };
                Some(crate::history_records::AsmHistoricalCurveAxis {
                    curve: entity_ref(&curve.id.0)?,
                    origin,
                    direction,
                })
            })
            .collect(),
        pcurves: refs(brep.pcurves.iter().map(|entity| entity.id.0.as_str()))?,
        body_regions: relations(brep.bodies.iter().map(|body| {
            (
                body.id.0.as_str(),
                body.regions.iter().map(|id| id.0.as_str()).collect(),
            )
        }))?,
        region_shells: relations(brep.regions.iter().map(|region| {
            (
                region.id.0.as_str(),
                region.shells.iter().map(|id| id.0.as_str()).collect(),
            )
        }))?,
        shell_faces: relations(brep.shells.iter().map(|shell| {
            (
                shell.id.0.as_str(),
                shell.faces.iter().map(|id| id.0.as_str()).collect(),
            )
        }))?,
        shell_wire_edges: relations(brep.shells.iter().map(|shell| {
            (
                shell.id.0.as_str(),
                shell.wire_edges.iter().map(|id| id.0.as_str()).collect(),
            )
        }))?,
        shell_free_vertices: relations(brep.shells.iter().map(|shell| {
            (
                shell.id.0.as_str(),
                shell.free_vertices.iter().map(|id| id.0.as_str()).collect(),
            )
        }))?,
        face_loops: relations(brep.faces.iter().map(|face| {
            (
                face.id.0.as_str(),
                face.loops.iter().map(|id| id.0.as_str()).collect(),
            )
        }))?,
        loop_coedges: relations(brep.loops.iter().map(|loop_| {
            (
                loop_.id.0.as_str(),
                loop_.coedges.iter().map(|id| id.0.as_str()).collect(),
            )
        }))?,
        coedge_topology: brep
            .coedges
            .iter()
            .map(|coedge| {
                Some(AsmHistoricalCoedge {
                    coedge: entity_ref(&coedge.id.0)?,
                    owner_loop: entity_ref(&coedge.owner_loop.0)?,
                    edge: entity_ref(&coedge.edge.0)?,
                    next: entity_ref(&coedge.next.0)?,
                    previous: entity_ref(&coedge.previous.0)?,
                    radial_next: entity_ref(&coedge.radial_next.0)?,
                })
            })
            .collect::<Option<Vec<_>>>()?,
        edge_vertices: brep
            .edges
            .iter()
            .map(|edge| {
                Some(AsmHistoricalEdge {
                    edge: entity_ref(&edge.id.0)?,
                    start_vertex: entity_ref(&edge.start.0)?,
                    end_vertex: entity_ref(&edge.end.0)?,
                })
            })
            .collect::<Option<Vec<_>>>()?,
        face_surfaces: brep
            .faces
            .iter()
            .map(|face| {
                Some(AsmHistoricalCarrierBinding {
                    entity: entity_ref(&face.id.0)?,
                    carrier: entity_ref(&face.surface.0)?,
                })
            })
            .collect::<Option<Vec<_>>>()?,
        edge_curves: brep
            .edges
            .iter()
            .map(|edge| {
                Some(AsmHistoricalOptionalCarrierBinding {
                    entity: entity_ref(&edge.id.0)?,
                    carrier: match &edge.curve {
                        Some(curve) => Some(entity_ref(&curve.0)?),
                        None => None,
                    },
                })
            })
            .collect::<Option<Vec<_>>>()?,
        coedge_pcurves: brep
            .coedges
            .iter()
            .map(|coedge| {
                Some(AsmHistoricalOptionalCarrierBinding {
                    entity: entity_ref(&coedge.id.0)?,
                    carrier: match coedge.pcurves.first() {
                        Some(use_) => Some(entity_ref(&use_.pcurve.0)?),
                        None => None,
                    },
                })
            })
            .collect::<Option<Vec<_>>>()?,
        vertex_points: brep
            .vertices
            .iter()
            .map(|vertex| {
                Some(AsmHistoricalCarrierBinding {
                    entity: entity_ref(&vertex.id.0)?,
                    carrier: entity_ref(&vertex.point.0)?,
                })
            })
            .collect::<Option<Vec<_>>>()?,
        point_positions: brep
            .points
            .iter()
            .map(|point| {
                Some(AsmHistoricalPoint {
                    point: entity_ref(&point.id.0)?,
                    position: point.position,
                })
            })
            .collect::<Option<Vec<_>>>()?,
    })
}

fn materialize_record_table(
    state: &AsmDeltaState,
    archive: &HistoricalRecordArchive,
) -> Option<Vec<cadmpeg_asm::sab::Record>> {
    if state.entity_versions.is_empty() {
        return None;
    }
    let present = state
        .entity_versions
        .iter()
        .map(|version| version.entity_ref)
        .collect::<HashSet<_>>();
    if present.len() != state.entity_versions.len() {
        return None;
    }
    let mut records = Vec::with_capacity(state.entity_versions.len());
    for version in &state.entity_versions {
        let record = archive.records.get(&version.record_ref)?;
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
        records.push(record.clone());
    }
    records.sort_unstable_by_key(|record| record.index);
    Some(records)
}

fn decode_bulletin_boards(
    bytes: &[u8],
    mut position: usize,
    stream: &str,
    state_offset: usize,
    state_id: &str,
    width: usize,
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
                (false, true) => AsmEntityChangeKind::Insert,
                (true, false) => AsmEntityChangeKind::Delete,
                (true, true) => AsmEntityChangeKind::Update,
                (false, false) => return None,
            };
            changes.push(AsmEntityChange {
                id: crate::ids::native_scoped_id(
                    stream,
                    "asm-entity-change",
                    format_args!(
                        "{state_offset:010}:{:06}:{:06}",
                        boards.len(),
                        changes.len()
                    ),
                ),
                parent: board_id.clone(),
                byte_offset: change_offset as u64,
                kind,
                old_ref: (old >= 0).then_some(old),
                new_ref: (new >= 0).then_some(new),
            });
        }
        boards.push(AsmBulletinBoard {
            id: board_id,
            parent: state_id.to_string(),
            byte_offset: board_offset as u64,
            owner_ref,
            number,
            changes,
        });
    }
    Some((boards, position))
}

fn decode_history_records(
    bytes: &[u8],
    state_end: usize,
    next_delta: Option<usize>,
    stream: &str,
    state_id: &str,
    width: usize,
) -> Vec<AsmHistoryRecord> {
    let mut start = state_end + usize::from(bytes.get(state_end) == Some(&0x11));
    if bytes.get(start) == Some(&0x04)
        && int_at(bytes, start + 1, width) == Some(0)
        && bytes.get(start + 1 + width) == Some(&0x11)
    {
        start += 2 + width;
    }
    let limit = next_delta.map_or(bytes.len(), |offset| offset + 1);
    if start >= limit {
        return Vec::new();
    }
    match cadmpeg_asm::sab::frame_history(bytes, start, limit, width) {
        Ok(records) => records
            .into_iter()
            .map(|record| {
                let entity_references = record
                    .tokens
                    .iter()
                    .filter_map(|token| match token {
                        cadmpeg_asm::sab::Token::Ref(value) => Some(*value),
                        _ => None,
                    })
                    .collect();
                AsmHistoryRecord {
                    id: crate::ids::native_scoped_id(
                        stream,
                        "asm-history-record",
                        format_args!("{:010}", record.offset),
                    ),
                    parent: state_id.to_string(),
                    revision_id: None,
                    index: record.index as u64,
                    byte_offset: record.offset as u64,
                    name: record.name,
                    framing_error: None,
                    entity_references,
                    raw_bytes: bytes[record.offset..record.offset + record.len].to_vec(),
                }
            })
            .collect(),
        Err(error) => {
            vec![AsmHistoryRecord {
                id: crate::ids::native_scoped_id(
                    stream,
                    "asm-history-record",
                    format_args!("{start:010}"),
                ),
                parent: state_id.to_string(),
                revision_id: None,
                index: 0,
                byte_offset: start as u64,
                name: "opaque_history_payload".into(),
                framing_error: Some(error.to_string()),
                entity_references: Vec::new(),
                raw_bytes: bytes[start..limit].to_vec(),
            }]
        }
    }
}

fn decode_preamble(bytes: &[u8], mut position: usize, width: usize) -> Option<(i64, i64)> {
    let size = take_int(bytes, &mut position, 0x04, width)?;
    let duplicate = take_int(bytes, &mut position, 0x04, width)?;
    let zero = take_int(bytes, &mut position, 0x04, width)?;
    let entry_count = take_int(bytes, &mut position, 0x04, width)?;
    (size == duplicate && zero == 0).then_some((size, entry_count))
}

/// Read a tagged little-endian signed integer of the stream's ref width (4 or
/// 8 bytes) and advance past it.
fn take_int(bytes: &[u8], position: &mut usize, tag: u8, width: usize) -> Option<i64> {
    if bytes.get(*position) != Some(&tag) {
        return None;
    }
    let value = int_at(bytes, *position + 1, width)?;
    *position += 1 + width;
    Some(value)
}

#[cfg(test)]
mod tests;
