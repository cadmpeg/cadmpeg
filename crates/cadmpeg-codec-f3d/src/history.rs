// SPDX-License-Identifier: Apache-2.0
//! Decode the ASM construction-history partition after the active model slice.
//!
//! [`decode`] reads `delta_state` headers, bulletin-board entity changes, and
//! history records while retaining source bytes for records without typed
//! semantics.

use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmEntityVersion,
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalEntityDelta,
    AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint, AsmHistoricalRelation,
    AsmHistoricalTopology, AsmHistoricalTopologyDelta, AsmHistoricalTransition, AsmHistory,
    AsmHistoryRecord,
};
use crate::records::{AsmHistoricalEntityKind, DesignExtrudeSelectionMember};
use cadmpeg_ir::le::int_at;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const DELTA: &[u8] = b"\x11\x0d\x0bdelta_state";
const PREAMBLE: &[u8] = b"\x0d\x0ehistory_stream";

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
/// record ([spec §2.3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#23-delta_state-records)) from `bytes`, each with its `BulletinBoard` chain of
/// per-entity insert/delete/update changes and the raw history-entity records
/// framed between it and the next `delta_state`. `stream` is the source ZIP
/// entry name, recorded in each decoded item's provenance. Returns `None` when
/// `bytes` carries no `delta_state` record (the stream is a construction
/// snapshot with no history tail) or a malformed history body. `width` is the
/// stream's integer/ref width (4 for `BinaryFile4`, 8 for `BinaryFile8`).
pub(crate) fn decode(bytes: &[u8], stream: &str, width: usize) -> Option<AsmHistory> {
    let preamble_offset = bytes
        .windows(PREAMBLE.len())
        .position(|window| window == PREAMBLE);
    let history_offset = preamble_offset.unwrap_or(0);
    let history_id = format!("f3d:{stream}:asm-history#{history_offset:010}");
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
        let state_record_id = format!("f3d:{stream}:asm-delta-state#{offset:010}");
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
    bind_complete_record_tables(&mut states, bytes, width);
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

fn bind_historical_entity_versions(states: &mut [AsmDeltaState]) {
    let mut archived = states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
        .collect::<Vec<_>>();
    archived.sort_unstable();
    let Some(&active_count) = archived.first() else {
        return;
    };
    if active_count <= 0
        || archived
            .iter()
            .copied()
            .ne(active_count..active_count + archived.len() as i64)
    {
        return;
    }
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
                    if !versions.contains_key(&new) || archived.binary_search(&old).is_err() {
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
                    if versions.contains_key(&old) || archived.binary_search(&old).is_err() {
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

fn bind_complete_record_tables(states: &mut [AsmDeltaState], bytes: &[u8], width: usize) {
    let Some(start) = crate::asm_header::record_stream_start(bytes) else {
        return;
    };
    let Ok(framed) = crate::sab::frame(bytes, start, bytes.len(), width) else {
        return;
    };
    let Some(active_count) = states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
        .min()
        .and_then(|count| usize::try_from(count).ok())
    else {
        return;
    };
    let Some(active_records) = framed.get(..active_count) else {
        return;
    };
    let topology = states
        .iter()
        .map(|state| {
            let records = materialize_record_table(states, state, active_records, bytes, width)?;
            historical_topology(&crate::brep::decode(&records, bytes, "history"))
        })
        .collect::<Option<Vec<_>>>();
    if let Some(topology) = topology {
        for (state, topology) in states.iter_mut().zip(topology) {
            state.record_table_complete = true;
            state.topology = Some(topology);
        }
        bind_historical_transitions(states);
    }
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
        }
    }
}

pub(crate) fn bind_feature_face_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::DesignParameterScope],
    groups: &[crate::records::DesignConstructionOperandGroup],
    operands: &[crate::records::DesignFaceOperand],
    histories: &[AsmHistory],
) {
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
        let (Some(state_id), Some(previous_state_id)) =
            (scope.history_state_id, scope.previous_history_state_id)
        else {
            continue;
        };
        let Some(Some(state)) = states.get(&state_id) else {
            continue;
        };
        let Some(transition) = &state.transition else {
            continue;
        };
        if transition.previous_state_id != Some(previous_state_id) {
            continue;
        }
        let Some(Some(previous)) = states.get(&previous_state_id) else {
            continue;
        };
        let Some(_topology) = &previous.topology else {
            continue;
        };
        let cadmpeg_ir::features::FeatureDefinition::Extrude { start, extent, .. } =
            &mut feature.definition
        else {
            continue;
        };
        if let cadmpeg_ir::features::ExtrudeStart::FromFace { face, .. } = start {
            bind_face_selection(face, scope, groups, operands);
        }
        if let cadmpeg_ir::features::Extent::ToFace { face, .. } = extent {
            bind_face_selection(face, scope, groups, operands);
        }
    }
}

pub(crate) fn bind_face_operand_history_candidates(
    operands: &mut [crate::records::DesignFaceOperand],
    scopes: &[crate::records::DesignParameterScope],
    histories: &[AsmHistory],
) {
    let mut states = HashMap::<i64, Option<&AsmDeltaState>>::new();
    for state in histories.iter().flat_map(|history| &history.states) {
        states
            .entry(state.state_id)
            .and_modify(|state| *state = None)
            .or_insert(Some(state));
    }
    for operand in operands {
        operand.preceding_candidate_faces.clear();
        operand.changed_candidate_faces.clear();
        let stream = crate::design::native_stream(&operand.id);
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == operand.scope_record_index
                && crate::design::native_stream(&scope.id) == stream
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
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
        let (Some(transition), Some(topology)) = (&state.transition, &previous.topology) else {
            continue;
        };
        if transition.previous_state_id != Some(previous_state_id) {
            continue;
        }
        operand.preceding_candidate_faces =
            faces_in_topology(crate::design::face_operand_candidates(operand), topology);
        operand.changed_candidate_faces =
            faces_changed_by_transition(&operand.preceding_candidate_faces, transition)
                .into_iter()
                .cloned()
                .collect();
    }
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

fn edge_recipe_reference_context(
    reference_ordinal: u32,
    reference: &crate::records::DesignRecipeReference,
    result_topology: &AsmHistoricalTopology,
    result_boundary_edges: &[i64],
    preceding_topology: &AsmHistoricalTopology,
    preceding_boundary_edges: &[i64],
    changed_edges: &HashSet<i64>,
) -> crate::records::DesignEdgeRecipeReferenceContext {
    let result_faces = faces_in_topology(&reference.candidate_faces, result_topology);
    let result_edges = face_boundary_edges(&result_faces, result_topology)
        .into_iter()
        .collect::<HashSet<_>>();
    let result_shared_edge_slots = result_boundary_edges
        .iter()
        .copied()
        .filter(|edge| result_edges.contains(edge))
        .collect();
    let preceding_faces = faces_in_topology(&reference.candidate_faces, preceding_topology);
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
        .collect();
    crate::records::DesignEdgeRecipeReferenceContext {
        reference_ordinal,
        result_faces,
        result_shared_edge_slots,
        preceding_faces,
        shared_edge_slots,
        changed_shared_edge_slots,
    }
}

pub(crate) fn bind_edge_operand_history_candidates(
    operands: &mut [crate::records::DesignEdgeOperand],
    scopes: &[crate::records::DesignParameterScope],
    histories: &[AsmHistory],
) {
    let mut states = HashMap::<i64, Option<&AsmDeltaState>>::new();
    for state in histories.iter().flat_map(|history| &history.states) {
        states
            .entry(state.state_id)
            .and_modify(|state| *state = None)
            .or_insert(Some(state));
    }
    for operand in operands {
        operand.result_candidate_faces.clear();
        operand.result_boundary_edge_slots.clear();
        operand.preceding_candidate_faces.clear();
        operand.changed_candidate_faces.clear();
        operand.preceding_boundary_edge_slots.clear();
        operand.changed_boundary_edge_slots.clear();
        operand.deleted_boundary_edge_slots.clear();
        operand.updated_boundary_edge_slots.clear();
        operand.changed_boundary_edge_contexts.clear();
        operand.recipe_reference_contexts.clear();
        operand.recipe_selectors.clear();
        operand.resolved_edge_slot = None;
        let stream = crate::design::native_stream(&operand.id);
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == operand.scope_record_index
                && crate::design::native_stream(&scope.id) == stream
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
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
        let (Some(transition), Some(result_topology), Some(topology)) =
            (&state.transition, &state.topology, &previous.topology)
        else {
            continue;
        };
        if transition.previous_state_id != Some(previous_state_id) {
            continue;
        }
        operand.result_candidate_faces =
            faces_in_topology(&operand.candidate_faces, result_topology);
        operand.result_boundary_edge_slots =
            face_boundary_edges(&operand.result_candidate_faces, result_topology);
        operand.preceding_candidate_faces = faces_in_topology(&operand.candidate_faces, topology);
        operand.changed_candidate_faces =
            faces_changed_by_transition(&operand.preceding_candidate_faces, transition)
                .into_iter()
                .cloned()
                .collect();
        operand.preceding_boundary_edge_slots =
            face_boundary_edges(&operand.preceding_candidate_faces, topology);
        let changed_edges = transition
            .topology
            .edges
            .deleted
            .iter()
            .chain(&transition.topology.edges.updated)
            .copied()
            .collect::<HashSet<_>>();
        operand.changed_boundary_edge_slots = operand
            .preceding_boundary_edge_slots
            .iter()
            .copied()
            .filter(|edge| changed_edges.contains(edge))
            .collect();
        operand.deleted_boundary_edge_slots = boundary_edges_in_changes(
            &operand.preceding_boundary_edge_slots,
            &transition.topology.edges.deleted,
        );
        operand.updated_boundary_edge_slots = boundary_edges_in_changes(
            &operand.preceding_boundary_edge_slots,
            &transition.topology.edges.updated,
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
        operand.recipe_selectors = recipe_selector_candidates(
            operand.recipe_structure.as_ref(),
            &operand.changed_boundary_edge_contexts,
        );
        operand.resolved_edge_slot = crate::design::resolve_edge_operand_candidates(operand);
    }
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
            let clause_entries = structure.sides.each_ref().map(|side| {
                side.entries
                    .iter()
                    .find(|entry| entry.selector == *selector)
                    .cloned()
            });
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
            let clause_triplet_edge_slots = clause_entries.each_ref().map(|entry| {
                entry.as_ref().map(|entry| {
                    entry.topology_triplets.each_ref().map(|triplet| {
                        contexts
                            .iter()
                            .filter(|context| {
                                context.incident_loops.iter().any(|incident| {
                                    incident.boundary_edge_count == entry.boundary_edge_count.get()
                                        && incident.coedge_ordinal == triplet.incident_edge_ordinal
                                })
                            })
                            .map(|context| context.edge_slot)
                            .collect()
                    })
                })
            });
            let incidence_matching_edge_slots = contexts
                .iter()
                .filter(|context| {
                    clause_entries.iter().flatten().all(|entry| {
                        entry.topology_triplets.iter().all(|triplet| {
                            context.incident_loops.iter().any(|incident| {
                                incident.boundary_edge_count == entry.boundary_edge_count.get()
                                    && incident.coedge_ordinal == triplet.incident_edge_ordinal
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
    let mut matching_groups = groups.iter().filter(|group| group.id == *native);
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some()
        || group.scope_record_index != scope.record_index
        || crate::design::native_stream(&group.id) != crate::design::native_stream(&scope.id)
    {
        return;
    }
    let Some(stream) = crate::design::native_stream(&scope.id) else {
        return;
    };
    let mut faces = Vec::new();
    for record_index in &group.members {
        let mut matches = operands.iter().filter(|operand| {
            crate::design::native_stream(&operand.id) == Some(stream)
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

fn faces_changed_by_transition<'a>(
    candidates: &'a [cadmpeg_ir::ids::FaceId],
    transition: &crate::history_records::AsmHistoricalTransition,
) -> Vec<&'a cadmpeg_ir::ids::FaceId> {
    let changed = transition
        .topology
        .faces
        .deleted
        .iter()
        .chain(&transition.topology.faces.updated)
        .copied()
        .collect::<HashSet<_>>();
    candidates
        .iter()
        .filter(|face| stable_ref(&face.0).is_some_and(|slot| changed.contains(&slot)))
        .collect()
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

/// Resolve one Design persistent local identity to its invariant stable ASM
/// history family and the states containing that slot.
pub(crate) fn historical_identity_kind(
    histories: &[AsmHistory],
    local_id: u64,
) -> Option<(AsmHistoricalEntityKind, Vec<i64>)> {
    let entity_ref = i64::try_from(local_id).ok()?;
    let mut kinds = HashSet::new();
    let mut states = Vec::new();
    for state in histories.iter().flat_map(|history| &history.states) {
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
            if members.contains(&entity_ref) {
                kinds.insert(kind);
                if !states.contains(&state.state_id) {
                    states.push(state.state_id);
                }
            }
        }
    }
    let mut kinds = kinds.into_iter();
    let kind = kinds.next()?;
    if kinds.next().is_some() {
        return None;
    }
    Some((kind, states))
}

pub(crate) fn historical_selection_identity_kind(
    histories: &[AsmHistory],
    local_id: u64,
) -> Option<(AsmHistoricalEntityKind, i64, Vec<i64>)> {
    let record_ref = i64::try_from(local_id).ok()?;
    let mut entity_refs = HashSet::new();
    let mut states = Vec::new();
    for state in histories.iter().flat_map(|history| &history.states) {
        for version in &state.entity_versions {
            if version.record_ref == record_ref {
                entity_refs.insert(version.entity_ref);
                if !states.contains(&state.state_id) {
                    states.push(state.state_id);
                }
            }
        }
    }
    if let Some(resolved) = historical_identity_kind(histories, local_id) {
        return (entity_refs.is_empty() || entity_refs == HashSet::from([record_ref]))
            .then_some((resolved.0, record_ref, resolved.1));
    }
    let mut entity_refs = entity_refs.into_iter();
    let entity_ref = entity_refs.next()?;
    if entity_refs.next().is_some() {
        return None;
    }
    let entity_ref = u64::try_from(entity_ref).ok()?;
    let (kind, _) = historical_identity_kind(histories, entity_ref)?;
    Some((kind, i64::try_from(entity_ref).ok()?, states))
}

pub(crate) fn bind_extrude_selection_history(
    members: &mut [DesignExtrudeSelectionMember],
    histories: &[AsmHistory],
) {
    for member in members {
        member.historical_entity_kind = None;
        member.historical_entity_ref = None;
        member.historical_state_ids.clear();
        if let Some((kind, entity_ref, states)) =
            historical_selection_identity_kind(histories, member.local_id)
        {
            member.historical_entity_kind = Some(kind);
            member.historical_entity_ref = Some(entity_ref);
            member.historical_state_ids = states;
        }
    }
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

fn historical_topology(brep: &crate::brep::Brep) -> Option<AsmHistoricalTopology> {
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
        curves: refs(brep.curves.iter().map(|entity| entity.id.0.as_str()))?,
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
                    carrier: match &coedge.pcurve {
                        Some(pcurve) => Some(entity_ref(&pcurve.0)?),
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
    states: &[AsmDeltaState],
    state: &AsmDeltaState,
    active_records: &[crate::sab::Record],
    bytes: &[u8],
    width: usize,
) -> Option<Vec<crate::sab::Record>> {
    if state.entity_versions.is_empty()
        || active_records
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
    let mut archived_records = HashMap::new();
    for record in states
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| record.name != "End-of-ASM-data")
    {
        let revision_id = record.revision_id?;
        let offset = usize::try_from(record.byte_offset).ok()?;
        let limit = offset.checked_add(record.raw_bytes.len())?;
        if bytes.get(offset..limit)? != record.raw_bytes {
            return None;
        }
        let mut framed = crate::sab::frame(bytes, offset, limit, width).ok()?;
        if framed.len() != 1 {
            return None;
        }
        let framed = framed.pop()?;
        if framed.name != record.name || archived_records.insert(revision_id, framed).is_some() {
            return None;
        }
    }
    if archived_records.len() != revision_entities.len().checked_sub(active_records.len())? {
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
        if revision_entities.get(&version.record_ref) != Some(&version.entity_ref) {
            return None;
        }
        let mut record = if version.record_ref < active_count {
            active_records
                .get(usize::try_from(version.record_ref).ok()?)?
                .clone()
        } else {
            archived_records.get(&version.record_ref)?.clone()
        };
        record.index = usize::try_from(version.entity_ref).ok()?;
        for token in &mut record.tokens {
            let crate::sab::Token::Ref(reference) = token else {
                continue;
            };
            if *reference < 0 {
                continue;
            }
            *reference = *revision_entities.get(reference)?;
            if !present.contains(reference) {
                return None;
            }
        }
        records.push(record);
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
        let board_id = format!(
            "f3d:{stream}:asm-bulletin-board#{state_offset:010}:{:06}",
            boards.len()
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
                id: format!(
                    "f3d:{stream}:asm-entity-change#{state_offset:010}:{:06}:{:06}",
                    boards.len(),
                    changes.len()
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
    match crate::sab::frame_history(bytes, start, limit, width) {
        Ok(records) => records
            .into_iter()
            .map(|record| {
                let entity_references = record
                    .tokens
                    .iter()
                    .filter_map(|token| match token {
                        crate::sab::Token::Ref(value) => Some(*value),
                        _ => None,
                    })
                    .collect();
                AsmHistoryRecord {
                    id: format!("f3d:{stream}:asm-history-record#{:010}", record.offset),
                    parent: state_id.to_string(),
                    revision_id: None,
                    index: record.index as u64,
                    byte_offset: record.offset as u64,
                    name: record.name,
                    entity_references,
                    raw_bytes: bytes[record.offset..record.offset + record.len].to_vec(),
                }
            })
            .collect(),
        Err(_) => {
            vec![AsmHistoryRecord {
                id: format!("f3d:{stream}:asm-history-record#{start:010}"),
                parent: state_id.to_string(),
                revision_id: None,
                index: 0,
                byte_offset: start as u64,
                name: "opaque_history_payload".into(),
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
mod tests {
    use super::*;

    #[test]
    fn boundary_edge_change_partition_preserves_boundary_order() {
        assert_eq!(boundary_edges_in_changes(&[8, 3, 5, 2], &[2, 8]), [8, 2]);
        assert!(boundary_edges_in_changes(&[8, 3], &[1, 2]).is_empty());
    }

    #[test]
    fn historical_topology_retains_ordered_ownership_and_incidence() {
        use cadmpeg_ir::ids::{
            BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId,
            SurfaceId, VertexId,
        };
        use cadmpeg_ir::topology::{
            Body, BodyKind, Coedge, Edge, Face, Loop, Region, Sense, Shell, Vertex,
        };

        let id = |slot| format!("f3d:brep:entity#{slot}");
        let mut brep = crate::brep::Brep::default();
        brep.bodies.push(Body {
            id: BodyId(id(1)),
            kind: BodyKind::Solid,
            regions: vec![RegionId(id(2))],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        brep.regions.push(Region {
            id: RegionId(id(2)),
            body: BodyId(id(1)),
            shells: vec![ShellId(id(3))],
        });
        brep.shells.push(Shell {
            id: ShellId(id(3)),
            region: RegionId(id(2)),
            faces: vec![FaceId(id(4))],
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        brep.faces.push(Face {
            id: FaceId(id(4)),
            shell: ShellId(id(3)),
            surface: SurfaceId(id(20)),
            sense: Sense::Forward,
            loops: vec![LoopId(id(5))],
            name: None,
            color: None,
            tolerance: None,
        });
        brep.loops.push(Loop {
            id: LoopId(id(5)),
            face: FaceId(id(4)),
            coedges: vec![CoedgeId(id(6))],
        });
        brep.coedges.push(Coedge {
            id: CoedgeId(id(6)),
            owner_loop: LoopId(id(5)),
            edge: EdgeId(id(7)),
            next: CoedgeId(id(6)),
            previous: CoedgeId(id(6)),
            radial_next: CoedgeId(id(6)),
            sense: Sense::Forward,
            pcurve: None,
            pcurve_parameter_range: None,
            use_curve: None,
            use_curve_parameter_range: None,
        });
        brep.edges.push(Edge {
            id: EdgeId(id(7)),
            curve: Some(CurveId(id(21))),
            start: VertexId(id(8)),
            end: VertexId(id(9)),
            param_range: None,
            tolerance: None,
        });
        for slot in [8, 9] {
            brep.vertices.push(Vertex {
                id: VertexId(id(slot)),
                point: PointId(id(slot + 20)),
                tolerance: None,
            });
        }

        let topology = historical_topology(&brep).expect("stable historical topology");
        assert_eq!(topology.body_regions[0].member_refs, [2]);
        assert_eq!(topology.region_shells[0].member_refs, [3]);
        assert_eq!(topology.shell_faces[0].member_refs, [4]);
        assert_eq!(topology.face_loops[0].member_refs, [5]);
        assert_eq!(topology.loop_coedges[0].member_refs, [6]);
        assert_eq!(topology.coedge_topology[0].edge, 7);
        assert_eq!(topology.coedge_topology[0].radial_next, 6);
        assert_eq!(
            historical_edge_context(7, &topology),
            crate::records::DesignHistoricalEdgeContext {
                edge_slot: 7,
                incident_loops: vec![crate::records::DesignHistoricalEdgeLoopContext {
                    coedge_slot: 6,
                    loop_slot: 5,
                    face_slot: 4,
                    boundary_edge_count: 1,
                    coedge_ordinal: 0,
                    previous_edge_slot: 7,
                    next_edge_slot: 7,
                }],
            }
        );
        let entry = |selector, boundary_edge_count| crate::records::DesignTopologyRecipeEntry {
            selector,
            boundary_edge_count: std::num::NonZeroU32::new(boundary_edge_count).unwrap(),
            common_incident_edge_ordinal: (boundary_edge_count == 1).then_some(0),
            topology_triplets: [
                crate::records::DesignTopologyRecipeTriplet {
                    outer: std::num::NonZeroU32::new(1).unwrap(),
                    middle: 0,
                    vertex_ordinal: 0,
                    incident_edge_ordinal: boundary_edge_count - 1,
                    incident_side: crate::records::DesignTopologyIncidentSide::Preceding,
                },
                crate::records::DesignTopologyRecipeTriplet {
                    outer: std::num::NonZeroU32::new(1).unwrap(),
                    middle: 1,
                    vertex_ordinal: 0,
                    incident_edge_ordinal: 0,
                    incident_side: crate::records::DesignTopologyIncidentSide::Following,
                },
            ],
        };
        let side = |entries: Vec<crate::records::DesignTopologyRecipeEntry>| {
            crate::records::DesignTopologyRecipeSide {
                field_count: std::num::NonZeroU32::new(3).unwrap(),
                header_value: 0,
                first: 0,
                second: 0,
                third: None,
                payload_entry_count: u32::try_from(entries.len()).unwrap(),
                entries,
            }
        };
        let structure = crate::records::DesignEdgeRecipeStructure {
            root: 2,
            sides: [
                side(vec![entry(1, 1), entry(2, 1)]),
                side(vec![entry(1, 2)]),
            ],
        };
        let loop_context =
            |coedge_slot, boundary_edge_count| crate::records::DesignHistoricalEdgeLoopContext {
                coedge_slot,
                loop_slot: coedge_slot + 10,
                face_slot: coedge_slot + 20,
                boundary_edge_count,
                coedge_ordinal: 0,
                previous_edge_slot: coedge_slot + 30,
                next_edge_slot: coedge_slot + 40,
            };
        let contexts = [
            crate::records::DesignHistoricalEdgeContext {
                edge_slot: 7,
                incident_loops: vec![loop_context(70, 1)],
            },
            crate::records::DesignHistoricalEdgeContext {
                edge_slot: 8,
                incident_loops: vec![
                    loop_context(80, 1),
                    loop_context(81, 2),
                    crate::records::DesignHistoricalEdgeLoopContext {
                        coedge_ordinal: 1,
                        ..loop_context(82, 2)
                    },
                ],
            },
        ];
        let selectors = recipe_selector_candidates(Some(&structure), &contexts);
        assert_eq!(selectors.len(), 2);
        assert_eq!(selectors[0].selector, 1);
        assert_eq!(selectors[0].boundary_count_matching_edge_slots, [8]);
        assert_eq!(
            selectors[0].clause_triplet_edge_slots,
            [Some([vec![7, 8], vec![7, 8]]), Some([vec![8], vec![8]])]
        );
        assert_eq!(selectors[0].incidence_matching_edge_slots, [8]);
        assert_eq!(selectors[0].unique_incidence_edge_slot, Some(8));
        assert_eq!(selectors[1].selector, 2);
        assert_eq!(selectors[1].boundary_count_matching_edge_slots, [7, 8]);
        assert_eq!(selectors[1].incidence_matching_edge_slots, [7, 8]);
        assert_eq!(selectors[1].unique_incidence_edge_slot, None);
        assert_eq!(
            selectors[1].clause_triplet_edge_slots,
            [Some([vec![7, 8], vec![7, 8]]), None]
        );
        assert!(incident_loop_counts_satisfy_sides(
            &[4, 5],
            &[Some(5), Some(4)]
        ));
        assert!(!incident_loop_counts_satisfy_sides(
            &[5, 6],
            &[Some(5), Some(5)]
        ));
        assert!(incident_loop_counts_satisfy_sides(
            &[5, 5],
            &[Some(5), Some(5)]
        ));
        assert!(incident_loop_counts_satisfy_sides(&[5], &[None, Some(5)]));
        assert_eq!(topology.edge_vertices[0].start_vertex, 8);
        assert_eq!(topology.edge_vertices[0].end_vertex, 9);
        assert_eq!(topology.face_surfaces[0].carrier, 20);
        assert_eq!(topology.edge_curves[0].carrier, Some(21));
        assert_eq!(topology.coedge_pcurves[0].carrier, None);
        assert_eq!(topology.vertex_points[0].carrier, 28);
        assert_eq!(
            bodies_intersecting(&topology, &BTreeSet::from([20])).unwrap(),
            BTreeSet::from([1])
        );
        assert_eq!(
            bodies_intersecting(&topology, &BTreeSet::from([28])).unwrap(),
            BTreeSet::from([1])
        );
        assert_eq!(
            faces_in_topology(
                &[FaceId(id(4)), FaceId(id(99)), FaceId("foreign".into())],
                &topology,
            ),
            [FaceId(id(4))]
        );
        let mut transition = AsmHistoricalTransition {
            previous_state_id: Some(1),
            records: AsmHistoricalEntityDelta::default(),
            topology: AsmHistoricalTopologyDelta::default(),
        };
        transition.topology.faces.updated = vec![4];
        transition.topology.faces.inserted = vec![99];
        let candidates = [FaceId(id(4)), FaceId(id(99))];
        assert_eq!(
            faces_changed_by_transition(&candidates, &transition),
            [&candidates[0]]
        );
        let reference = crate::records::DesignRecipeReference {
            token: "1".into(),
            token_offset: 0,
            design_reference: 1,
            design_reference_offset: 1,
            candidate_faces: vec![FaceId(id(4))],
            candidate_edges: Vec::new(),
        };
        let context = edge_recipe_reference_context(
            2,
            &reference,
            &topology,
            &[7, 99],
            &topology,
            &[7, 98],
            &HashSet::from([7]),
        );
        assert_eq!(context.reference_ordinal, 2);
        assert_eq!(context.result_faces, [FaceId(id(4))]);
        assert_eq!(context.result_shared_edge_slots, [7]);
        assert_eq!(context.preceding_faces, [FaceId(id(4))]);
        assert_eq!(context.shared_edge_slots, [7]);
        assert_eq!(context.changed_shared_edge_slots, [7]);
    }

    #[test]
    fn design_identity_resolves_only_one_invariant_history_family() {
        let state = |state_id, topology| AsmDeltaState {
            id: format!("state-{state_id}"),
            parent: "history".into(),
            byte_offset: 0,
            state_id,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: state_id,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            record_table_complete: true,
            topology: Some(topology),
            transition: None,
        };
        let history = AsmHistory {
            id: "history".into(),
            byte_offset: 0,
            stream_size: None,
            history_entry_count: None,
            states: vec![
                state(
                    3,
                    AsmHistoricalTopology {
                        edges: vec![42],
                        ..AsmHistoricalTopology::default()
                    },
                ),
                state(
                    5,
                    AsmHistoricalTopology {
                        edges: vec![42],
                        vertices: vec![90],
                        ..AsmHistoricalTopology::default()
                    },
                ),
            ],
        };
        assert_eq!(
            historical_identity_kind(std::slice::from_ref(&history), 42),
            Some((AsmHistoricalEntityKind::Edge, vec![3, 5]))
        );
        assert_eq!(
            historical_identity_kind(std::slice::from_ref(&history), 90),
            Some((AsmHistoricalEntityKind::Vertex, vec![5]))
        );
        assert_eq!(
            historical_selection_identity_kind(std::slice::from_ref(&history), 42),
            Some((AsmHistoricalEntityKind::Edge, 42, vec![3, 5]))
        );
        assert_eq!(
            historical_identity_kind(std::slice::from_ref(&history), 7),
            None
        );
        let mut revision_history = history.clone();
        revision_history.states[0].entity_versions = vec![AsmEntityVersion {
            entity_ref: 42,
            record_ref: 700,
        }];
        revision_history.states[1].entity_versions = vec![AsmEntityVersion {
            entity_ref: 42,
            record_ref: 701,
        }];
        assert_eq!(
            historical_selection_identity_kind(std::slice::from_ref(&revision_history), 700),
            Some((AsmHistoricalEntityKind::Edge, 42, vec![3]))
        );
        assert_eq!(
            historical_selection_identity_kind(std::slice::from_ref(&revision_history), 701),
            Some((AsmHistoricalEntityKind::Edge, 42, vec![5]))
        );
        revision_history.states[0].entity_versions = vec![AsmEntityVersion {
            entity_ref: 90,
            record_ref: 42,
        }];
        assert_eq!(
            historical_selection_identity_kind(std::slice::from_ref(&revision_history), 42),
            None
        );
        let ambiguous = AsmHistory {
            id: "other-history".into(),
            byte_offset: 0,
            stream_size: None,
            history_entry_count: None,
            states: vec![state(
                7,
                AsmHistoricalTopology {
                    vertices: vec![42],
                    ..AsmHistoricalTopology::default()
                },
            )],
        };
        assert_eq!(historical_identity_kind(&[history, ambiguous], 42), None);
    }

    #[test]
    fn historical_transition_separates_membership_and_revision_changes() {
        let state = |state_id, versions: &[(i64, i64)], topology| AsmDeltaState {
            id: format!("state-{state_id}"),
            parent: "history".into(),
            byte_offset: 0,
            state_id,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: state_id,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: versions
                .iter()
                .map(|&(entity_ref, record_ref)| AsmEntityVersion {
                    entity_ref,
                    record_ref,
                })
                .collect(),
            record_table_complete: true,
            topology: Some(topology),
            transition: None,
        };
        let previous = state(
            10,
            &[(1, 10), (4, 40), (8, 80)],
            AsmHistoricalTopology {
                bodies: vec![1],
                faces: vec![4],
                edges: vec![8],
                ..AsmHistoricalTopology::default()
            },
        );
        let current = state(
            11,
            &[(1, 11), (2, 2), (4, 40), (7, 70)],
            AsmHistoricalTopology {
                bodies: vec![1, 2],
                faces: vec![4],
                edges: vec![7],
                ..AsmHistoricalTopology::default()
            },
        );

        let transition = historical_transition(&current, Some(&previous)).unwrap();
        assert_eq!(transition.previous_state_id, Some(10));
        assert_eq!(transition.topology.bodies.inserted, [2]);
        assert_eq!(transition.topology.bodies.updated, [1]);
        assert!(transition.topology.faces.updated.is_empty());
        assert_eq!(transition.topology.edges.inserted, [7]);
        assert_eq!(transition.topology.edges.deleted, [8]);
        assert_eq!(transition.records.updated, [1]);
    }

    #[test]
    fn snapshot_ordinals_bind_the_sorted_revision_interval() {
        let history_id = "history".to_string();
        let state_id = "state".to_string();
        let board_id = "board".to_string();
        let mut state = AsmDeltaState {
            id: state_id.clone(),
            parent: history_id,
            byte_offset: 0,
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 0,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: vec![AsmBulletinBoard {
                id: board_id.clone(),
                parent: state_id.clone(),
                byte_offset: 0,
                owner_ref: 0,
                number: 2,
                changes: [7, 5, 6]
                    .into_iter()
                    .enumerate()
                    .map(|(index, old_ref)| AsmEntityChange {
                        id: format!("change-{index}"),
                        parent: board_id.clone(),
                        byte_offset: index as u64,
                        kind: AsmEntityChangeKind::Update,
                        old_ref: Some(old_ref),
                        new_ref: Some(index as i64),
                    })
                    .collect(),
            }],
            records: (0..3)
                .map(|index| AsmHistoryRecord {
                    id: format!("record-{index}"),
                    parent: state_id.clone(),
                    revision_id: None,
                    index,
                    byte_offset: index,
                    name: "edge".into(),
                    entity_references: Vec::new(),
                    raw_bytes: vec![0x11],
                })
                .collect(),
            entity_versions: Vec::new(),
            record_table_complete: false,
            topology: None,
            transition: None,
        };

        bind_snapshot_revision_ids(std::slice::from_mut(&mut state));

        assert_eq!(
            state
                .records
                .iter()
                .map(|record| record.revision_id)
                .collect::<Vec<_>>(),
            [Some(5), Some(6), Some(7)]
        );
    }

    #[test]
    fn materialized_record_table_normalizes_revision_references() {
        let mut archived_bytes = vec![0x0d, 4];
        archived_bytes.extend_from_slice(b"edge");
        archived_bytes.push(0x0c);
        archived_bytes.extend_from_slice(&2i64.to_le_bytes());
        archived_bytes.push(0x11);
        let state_id = "state".to_string();
        let board_id = "board".to_string();
        let state = AsmDeltaState {
            id: state_id.clone(),
            parent: "history".into(),
            byte_offset: 0,
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 0,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: vec![AsmBulletinBoard {
                id: board_id.clone(),
                parent: state_id.clone(),
                byte_offset: 0,
                owner_ref: 0,
                number: 2,
                changes: vec![AsmEntityChange {
                    id: "change".into(),
                    parent: board_id,
                    byte_offset: 0,
                    kind: AsmEntityChangeKind::Update,
                    old_ref: Some(2),
                    new_ref: Some(1),
                }],
            }],
            records: vec![AsmHistoryRecord {
                id: "record".into(),
                parent: state_id,
                revision_id: Some(2),
                index: 0,
                byte_offset: 0,
                name: "edge".into(),
                entity_references: vec![2],
                raw_bytes: archived_bytes.clone(),
            }],
            entity_versions: vec![
                AsmEntityVersion {
                    entity_ref: 0,
                    record_ref: 0,
                },
                AsmEntityVersion {
                    entity_ref: 1,
                    record_ref: 2,
                },
            ],
            record_table_complete: false,
            topology: None,
            transition: None,
        };
        let active = ["asmheader", "edge"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| crate::sab::Record {
                index,
                name: name.into(),
                head: name.into(),
                tokens: Vec::new(),
                offset: 0,
                len: 0,
            })
            .collect::<Vec<_>>();

        let table = materialize_record_table(
            std::slice::from_ref(&state),
            &state,
            &active,
            &archived_bytes,
            8,
        )
        .expect("complete historical RecordTable");

        assert_eq!(table.len(), 2);
        assert_eq!(table[1].index, 1);
        assert_eq!(table[1].tokens, [crate::sab::Token::Ref(1)]);
    }

    #[test]
    fn reverse_history_builds_complete_entity_version_maps() {
        let state = |node_index, previous_ref, next_ref, old_ref, new_ref| {
            let board_id = format!("board-{node_index}");
            AsmDeltaState {
                id: format!("state-{node_index}"),
                parent: "history".into(),
                byte_offset: node_index as u64,
                state_id: 10 - node_index,
                version_flag: 1,
                state_flag: 0,
                previous_ref,
                next_ref,
                node_index,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: vec![AsmBulletinBoard {
                    id: board_id.clone(),
                    parent: format!("state-{node_index}"),
                    byte_offset: node_index as u64,
                    owner_ref: 0,
                    number: 2,
                    changes: vec![AsmEntityChange {
                        id: format!("change-{node_index}"),
                        parent: board_id,
                        byte_offset: node_index as u64,
                        kind: match (old_ref, new_ref) {
                            (Some(_), Some(_)) => AsmEntityChangeKind::Update,
                            (None, Some(_)) => AsmEntityChangeKind::Insert,
                            (Some(_), None) => AsmEntityChangeKind::Delete,
                            (None, None) => unreachable!(),
                        },
                        old_ref,
                        new_ref,
                    }],
                }],
                records: Vec::new(),
                entity_versions: Vec::new(),
                record_table_complete: false,
                topology: None,
                transition: None,
            }
        };
        let mut states = vec![
            state(0, None, Some(1), Some(3), Some(1)),
            state(1, Some(0), Some(2), Some(4), Some(1)),
            state(2, Some(1), Some(3), None, Some(2)),
            state(3, Some(2), None, None, Some(1)),
        ];
        states[0].records = [3, 4]
            .map(|revision_id| AsmHistoryRecord {
                id: format!("record-{revision_id}"),
                parent: states[0].id.clone(),
                revision_id: Some(revision_id),
                index: revision_id as u64 - 3,
                byte_offset: 0,
                name: "edge".into(),
                entity_references: Vec::new(),
                raw_bytes: vec![0x11],
            })
            .into();

        bind_historical_entity_versions(&mut states);

        assert_eq!(
            states
                .iter()
                .map(|state| state.entity_versions.len())
                .collect::<Vec<_>>(),
            [3, 3, 3, 2]
        );
        assert_eq!(
            states[1].entity_versions,
            [
                AsmEntityVersion {
                    entity_ref: 0,
                    record_ref: 0,
                },
                AsmEntityVersion {
                    entity_ref: 1,
                    record_ref: 3,
                },
                AsmEntityVersion {
                    entity_ref: 2,
                    record_ref: 2,
                },
            ]
        );
        assert_eq!(states[2].entity_versions[1].record_ref, 4);
    }
}
