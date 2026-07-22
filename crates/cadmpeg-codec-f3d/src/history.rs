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
use crate::records::{
    AsmHistoricalEntityKind, DesignEdgeIdentityOperand, DesignExtrudeSelectionMember,
};
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
    let Some(archive) = historical_record_archive(states, active_records, bytes, width) else {
        return;
    };
    let topology = states
        .iter()
        .map(|state| {
            let records = materialize_record_table(state, &archive)?;
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

struct HistoricalRecordArchive {
    records: HashMap<i64, crate::sab::Record>,
}

fn historical_record_archive(
    states: &[AsmDeltaState],
    active_records: &[crate::sab::Record],
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
            let crate::sab::Token::Ref(reference) = token else {
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

pub(crate) fn bind_feature_body_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::DesignParameterScope],
    groups: &[crate::records::DesignConstructionOperandGroup],
    body_recipe_operands: &[crate::records::DesignBodyRecipeOperand],
    histories: &[AsmHistory],
) {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};

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
        if let FeatureDefinition::BoundaryFill { tools, cells } = &mut feature.definition {
            let Some(previous_state_id) = scope.previous_history_state_id else {
                continue;
            };
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
            continue;
        }
        let (bodies, proof) = match &mut feature.definition {
            FeatureDefinition::MoveBody { bodies, .. } => {
                (bodies, BodySelectionProof::TopologyStableRevision)
            }
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
            continue;
        };
        let Some(Some(state)) = states.get(&state_id) else {
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
            && matches!(group.role, 0x0000_0004_0000_0000 | 0x0000_0005_0000_0000)
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

pub(crate) fn bind_feature_face_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::DesignParameterScope],
    groups: &[crate::records::DesignConstructionOperandGroup],
    operands: &[crate::records::DesignFaceOperand],
    body_recipe_operands: &[crate::records::DesignBodyRecipeOperand],
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
        let feature_id = feature.id.clone();
        match &mut feature.definition {
            cadmpeg_ir::features::FeatureDefinition::Extrude { start, extent, .. } => {
                if let cadmpeg_ir::features::ExtrudeStart::FromFace { face, .. } = start {
                    bind_face_selection(face, scope, groups, operands);
                }
                if let cadmpeg_ir::features::Extent::ToFace { face, .. } = extent {
                    bind_face_selection(face, scope, groups, operands);
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
            _ => {}
        }
    }
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
                path: Some(path), ..
            } => bind_entity_selection_path(
                path,
                &feature_id,
                previous_state_id,
                scope,
                groups,
                operands,
            ),
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

    let mut states = HashMap::<i64, Option<&AsmDeltaState>>::new();
    for state in histories.iter().flat_map(|history| &history.states) {
        states
            .entry(state.state_id)
            .and_modify(|state| *state = None)
            .or_insert(Some(state));
    }
    features
        .iter()
        .filter_map(|feature| {
            let native_ref = feature.native_ref.as_deref()?;
            let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
            let scope = matching_scopes.next()?;
            if matching_scopes.next().is_some() {
                return None;
            }
            let previous_state_id = scope.previous_history_state_id.or_else(|| {
                let stream = crate::ids::native_stream(&scope.id);
                let operands = edge_operands
                    .iter()
                    .filter(|operand| {
                        operand.scope_record_index == scope.record_index
                            && crate::ids::native_stream(&operand.id) == stream
                    })
                    .collect::<Vec<_>>();
                let state = operands.first()?.recipe_state_id?;
                operands
                    .iter()
                    .all(|operand| operand.recipe_state_id == Some(state))
                    .then_some(state)
            })?;
            let state = (*states.get(&previous_state_id)?)?;
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
                native_ref: Some(state.id.clone()),
            })
        })
        .collect()
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

pub(crate) fn bind_face_operand_history_candidates(
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
        let Some(topology) = &previous.topology else {
            continue;
        };
        let Some(changed_faces) =
            face_changes_across_state_chain(state, previous_state_id, &states)
        else {
            continue;
        };
        operand.preceding_candidate_faces = faces_in_topology(
            crate::design::face_resolve::face_operand_candidates(operand),
            topology,
        );
        operand.changed_candidate_faces = operand
            .preceding_candidate_faces
            .iter()
            .filter(|face| stable_ref(&face.0).is_some_and(|slot| changed_faces.contains(&slot)))
            .cloned()
            .collect();
        operand.historical_support_contexts = historical_face_support_contexts(
            crate::design::face_resolve::face_operand_candidates(operand),
            histories,
            topology,
            &changed_faces,
        );
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
            _ => crate::design::face_resolve::resolve_face_operand_history_candidates(operand)
                .into_iter()
                .collect(),
        };
        if crate::design::design_feature_family(&scope.kind)
            == Some(crate::design::DesignFeatureFamily::Split)
        {
            operand.resolved_face_slots = resolve_split_tool_face(operand, topology)
                .into_iter()
                .collect();
        }
        if scope.kind == "Loft"
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
        for reference in &mut operand.references {
            reference.preceding_candidate_faces.clear();
            reference.preceding_body_slots.clear();
        }
        operand.resolved_face_slot = None;
        operand.resolved_body_slot = None;
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
        let Some(topology) = &previous.topology else {
            continue;
        };
        if face_changes_across_state_chain(state, previous_state_id, &states).is_none() {
            continue;
        }
        let Some(source) = historical_brep_source(&previous.id) else {
            continue;
        };
        let source_prefix = format!("f3d:brep/{source}/");
        for reference in &mut operand.references {
            reference.preceding_candidate_faces = faces_in_topology(
                &reference
                    .candidate_faces
                    .iter()
                    .filter(|face| face.0.starts_with(&source_prefix))
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
    let mut groups = HashMap::<(String, u32, u32), Vec<usize>>::new();
    for (index, operand) in operands.iter().enumerate() {
        let (Some(stream), Some(group)) = (
            crate::ids::native_stream(&operand.id),
            operand.group_record_index,
        ) else {
            continue;
        };
        groups
            .entry((stream.to_owned(), operand.scope_record_index, group))
            .or_default()
            .push(index);
    }
    for ((stream, scope_record_index, group_record_index), mut indices) in groups {
        let mut matching_groups = operand_groups.iter().filter(|group| {
            group.record_index == group_record_index
                && group.scope_record_index == scope_record_index
                && crate::ids::native_stream(&group.id) == Some(stream.as_str())
        });
        let Some(group) = matching_groups.next() else {
            continue;
        };
        if matching_groups.next().is_some()
            || group.extrude_role != Some(crate::records::DesignExtrudeOperandRole::Profile)
            || group.members.len() != indices.len()
        {
            continue;
        }
        if indices.iter().any(|index| {
            let operand = &operands[*index];
            !operand.resolved_face_slots.is_empty()
                || !crate::design::face_resolve::face_operand_candidates(operand).is_empty()
                || operand.recipe_references.iter().any(|reference| {
                    !reference.candidate_faces.is_empty()
                        || !reference.alternate_selector_faces.is_empty()
                })
        }) {
            continue;
        }
        indices.sort_by_key(|index| operands[*index].group_member_ordinal);
        if indices.iter().enumerate().any(|(ordinal, index)| {
            operands[*index].group_member_ordinal != u32::try_from(ordinal).ok()
                || group.members.get(ordinal) != Some(&operands[*index].record_index)
        }) {
            continue;
        }
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == scope_record_index
                && crate::ids::native_stream(&scope.id) == Some(stream.as_str())
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
        let (Some(topology), Some(changed_faces)) = (
            previous.topology.as_ref(),
            face_changes_across_state_chain(state, previous_state_id, &states),
        ) else {
            continue;
        };
        let Some(faces) =
            profile_face_group_cardinality_candidates(topology, &changed_faces, indices.len())
        else {
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
            (shared.len() == 1).then_some(shared[0])
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
) {
    let mut scope_operand_counts = HashMap::<(String, u32), usize>::new();
    for operand in operands.iter() {
        let Some(stream) = crate::ids::native_stream(&operand.id) else {
            continue;
        };
        *scope_operand_counts
            .entry((stream.to_owned(), operand.scope_record_index))
            .or_default() += 1;
    }
    let mut states = HashMap::<i64, Option<&AsmDeltaState>>::new();
    for state in histories.iter().flat_map(|history| &history.states) {
        states
            .entry(state.state_id)
            .and_modify(|state| *state = None)
            .or_insert(Some(state));
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
        let (Some(state_id), Some(previous_state_id)) =
            (scope.history_state_id, scope.previous_history_state_id)
        else {
            bind_active_edge_operand_candidates(operand, &terminal_topologies);
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
        operand.recipe_state_id = Some(previous_state_id);
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
        operand.treatment_radius_candidates = treatment_radius_candidates(
            Some(&operand.result_candidate_faces),
            &transition.topology.faces.inserted,
            result_topology,
            topology,
            &transition.topology.edges.deleted,
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
            && transition.topology.edges.deleted.len() == 1
        {
            operand.resolved_edge_slot = transition.topology.edges.deleted.first().copied();
        }
    }
}

fn historical_edge_axis(
    edge: i64,
    topology: &AsmHistoricalTopology,
) -> Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    let curve = topology
        .edge_curves
        .iter()
        .find(|binding| binding.entity == edge)?
        .carrier?;
    let axis = topology
        .curve_axes
        .iter()
        .find(|axis| axis.curve == curve)?;
    Some((axis.origin, axis.direction))
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
    let boundary = |face, topology: &AsmHistoricalTopology| {
        face_boundary_contexts_for_slots(&[face], topology)
            .into_iter()
            .flat_map(|context| context.loops)
            .flat_map(|loop_| loop_.edge_slots)
            .collect::<HashSet<_>>()
    };
    let mut out = Vec::new();
    let candidate_edges = result_candidate_faces
        .into_iter()
        .flatten()
        .filter_map(|face| stable_ref(&face.0))
        .flat_map(|face| boundary(face, result))
        .collect::<HashSet<_>>();
    for (inserted, carrier, supports) in treatment_face_supports(inserted_faces, result, preceding)
    {
        let inserted_boundary = boundary(inserted, result);
        if !candidate_edges.is_empty() && inserted_boundary.is_disjoint(&candidate_edges) {
            continue;
        }
        let mut radii = result
            .surface_radii
            .iter()
            .filter(|candidate| candidate.surface == carrier);
        let Some(radius) = radii.next().map(|candidate| candidate.radius) else {
            continue;
        };
        if radii.next().is_some() || !radius.is_finite() || radius <= 0.0 {
            continue;
        }
        for (ordinal, left) in supports.iter().enumerate() {
            let left_edges = boundary(*left, preceding);
            for right in supports.iter().skip(ordinal + 1) {
                let right_edges = boundary(*right, preceding);
                out.extend(
                    left_edges
                        .intersection(&right_edges)
                        .filter(|edge| deleted_edges.contains(edge))
                        .map(|edge| crate::records::DesignEdgeTreatmentRadiusCandidate {
                            edge_slot: *edge,
                            radius,
                        }),
                );
            }
        }
    }
    out.sort_by(|left, right| {
        left.radius
            .total_cmp(&right.radius)
            .then(left.edge_slot.cmp(&right.edge_slot))
    });
    out.dedup_by(|left, right| left.radius == right.radius && left.edge_slot == right.edge_slot);
    out
}

fn treatment_face_supports(
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
) -> Vec<(i64, i64, Vec<i64>)> {
    let boundary = |face, topology: &AsmHistoricalTopology| {
        face_boundary_contexts_for_slots(&[face], topology)
            .into_iter()
            .flat_map(|context| context.loops)
            .flat_map(|loop_| loop_.edge_slots)
            .collect::<HashSet<_>>()
    };
    let preceding_faces = preceding.faces.iter().copied().collect::<HashSet<_>>();
    let preceding_surfaces = preceding.surfaces.iter().copied().collect::<HashSet<_>>();
    let support = |result_face| {
        let mut bindings = result
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == result_face);
        let carrier = bindings.next()?.carrier;
        if bindings.next().is_some() {
            return None;
        }
        let mut matches = preceding.face_surfaces.iter().filter(|binding| {
            binding.carrier == carrier && preceding_faces.contains(&binding.entity)
        });
        let face = matches.next()?.entity;
        matches.next().is_none().then_some(face)
    };
    inserted_faces
        .iter()
        .copied()
        .filter_map(|inserted| {
            let mut bindings = result
                .face_surfaces
                .iter()
                .filter(|binding| binding.entity == inserted);
            let carrier = bindings.next()?.carrier;
            if bindings.next().is_some() || preceding_surfaces.contains(&carrier) {
                return None;
            }
            let inserted_boundary = boundary(inserted, result);
            let mut supports = result
                .faces
                .iter()
                .copied()
                .filter(|face| *face != inserted)
                .filter(|face| !boundary(*face, result).is_disjoint(&inserted_boundary))
                .filter_map(support)
                .collect::<Vec<_>>();
            supports.sort_unstable();
            supports.dedup();
            Some((inserted, carrier, supports))
        })
        .collect()
}

fn treatment_transition_edge_candidates(
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    deleted_edges: &[i64],
) -> Vec<i64> {
    let boundary = |face, topology: &AsmHistoricalTopology| {
        face_boundary_contexts_for_slots(&[face], topology)
            .into_iter()
            .flat_map(|context| context.loops)
            .flat_map(|loop_| loop_.edge_slots)
            .collect::<HashSet<_>>()
    };
    let mut out = Vec::new();
    for (_, _, supports) in treatment_face_supports(inserted_faces, result, preceding) {
        for (ordinal, left) in supports.iter().enumerate() {
            let left_edges = boundary(*left, preceding);
            for right in supports.iter().skip(ordinal + 1) {
                let right_edges = boundary(*right, preceding);
                out.extend(
                    left_edges
                        .intersection(&right_edges)
                        .filter(|edge| deleted_edges.contains(edge))
                        .copied(),
                );
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
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
            operand.group_record_index == group.record_index
                && operand.group_member_ordinal == ordinal
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
fn historical_identity_kind(
    histories: &[AsmHistory],
    local_id: u64,
) -> Option<(AsmHistoricalEntityKind, Vec<i64>)> {
    let entity_ref = i64::try_from(local_id).ok()?;
    let ambiguous_states = ambiguous_history_state_ids(histories);
    let mut kinds = HashSet::new();
    let mut states = Vec::new();
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
    let ambiguous_states = ambiguous_history_state_ids(histories);
    let mut entity_refs = HashSet::new();
    let mut states = Vec::new();
    for state in histories
        .iter()
        .flat_map(|history| &history.states)
        .filter(|state| !ambiguous_states.contains(&state.state_id))
    {
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

/// Resolve both identities in nested entity-selection operands against the
/// owning feature's exact input topology.
pub(crate) fn bind_entity_selection_history(
    operands: &mut [crate::records::DesignEntitySelectionOperand],
    scopes: &[crate::records::DesignParameterScope],
    histories: &[AsmHistory],
) {
    for operand in operands {
        operand.historical_edge_candidates.clear();
        operand.resolved_edge_slot = None;
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
        operand.historical_edge_candidates = entity_selection_edge_candidates(
            [operand.primary_identity, operand.secondary_identity],
            previous_state_id,
            histories,
            topology,
        );
        operand.resolved_edge_slot =
            unique_entity_selection_edge(&operand.historical_edge_candidates);
    }
}

fn entity_selection_edge_candidates(
    identities: [u64; 2],
    previous_state_id: i64,
    histories: &[AsmHistory],
    topology: &AsmHistoricalTopology,
) -> Vec<crate::records::DesignEntitySelectionEdgeCandidate> {
    use crate::records::DesignEntitySelectionEdgeCandidate;

    identities
        .into_iter()
        .enumerate()
        .filter_map(|(identity_ordinal, local_id)| {
            let (kind, entity_ref, states) =
                historical_selection_identity_kind(histories, local_id)?;
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
) {
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
        let Some(previous_state_id) = scopes
            .iter()
            .find(|scope| {
                crate::ids::native_stream(&scope.id) == Some(stream)
                    && scope.record_index == operand.scope_record_index
            })
            .and_then(|scope| scope.previous_history_state_id)
        else {
            continue;
        };
        let current_state_id = scopes
            .iter()
            .find(|scope| {
                crate::ids::native_stream(&scope.id) == Some(stream)
                    && scope.record_index == operand.scope_record_index
            })
            .and_then(|scope| scope.history_state_id);
        if let Some((kind, entity_ref, states)) =
            historical_selection_identity_kind(histories, operand.local_id)
                .filter(|(_, _, states)| states.contains(&previous_state_id))
        {
            operand.historical_entity_kind = Some(kind);
            operand.historical_entity_ref = Some(entity_ref);
            operand.historical_state_ids = states;
        }
        let mut topologies = histories
            .iter()
            .flat_map(|history| &history.states)
            .filter(|state| state.state_id == previous_state_id)
            .filter_map(|state| state.topology.as_ref());
        let Some(topology) = topologies.next() else {
            continue;
        };
        if topologies.next().is_some() {
            continue;
        }
        if let Some(current_state_id) = current_state_id {
            let mut current_states = histories
                .iter()
                .flat_map(|history| &history.states)
                .filter(|state| state.state_id == current_state_id);
            let current = current_states.next().filter(|state| {
                state
                    .transition
                    .as_ref()
                    .and_then(|transition| transition.previous_state_id)
                    == Some(previous_state_id)
            });
            if current_states.next().is_none() {
                if let Some((result, transition)) =
                    current.and_then(|state| state.topology.as_ref().zip(state.transition.as_ref()))
                {
                    operand.treatment_radius_candidates = treatment_radius_candidates(
                        None,
                        &transition.topology.faces.inserted,
                        result,
                        topology,
                        &transition.topology.edges.deleted,
                    );
                    operand.transition_edge_candidates = treatment_transition_edge_candidates(
                        &transition.topology.faces.inserted,
                        result,
                        topology,
                        &transition.topology.edges.deleted,
                    );
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
                historical_selection_identity_kind(histories, persistent.local_id)?;
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
) -> Option<Vec<crate::sab::Record>> {
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
            let crate::sab::Token::Ref(reference) = token else {
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
    fn historical_brep_source_qualifies_state_local_candidates() {
        assert_eq!(
            historical_brep_source(
                "f3d:asset/Breps.BlobParts/BREP.example.smbh:asm-delta-state#42"
            ),
            Some("example.smbh")
        );
        assert_eq!(historical_brep_source("f3d:unqualified:state#42"), None);
    }
    use crate::history_records::{
        AsmHistoricalCurveAxis, AsmHistoricalOptionalCarrierBinding, AsmHistoricalSurfaceRadius,
    };

    #[test]
    fn historical_edge_axis_uses_the_state_specific_curve_carrier() {
        let topology = AsmHistoricalTopology {
            edge_curves: vec![AsmHistoricalOptionalCarrierBinding {
                entity: 7,
                carrier: Some(27),
            }],
            curve_axes: vec![AsmHistoricalCurveAxis {
                curve: 27,
                origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
                direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            }],
            ..AsmHistoricalTopology::default()
        };
        assert_eq!(
            historical_edge_axis(7, &topology),
            Some((
                cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            ))
        );
        assert_eq!(historical_edge_axis(8, &topology), None);
    }

    #[test]
    fn historical_identity_edge_requires_unique_incidence() {
        let mut topology = AsmHistoricalTopology {
            edges: vec![7, 8],
            coedges: vec![17, 18],
            curves: vec![27],
            pcurves: vec![37],
            coedge_topology: vec![
                crate::history_records::AsmHistoricalCoedge {
                    coedge: 17,
                    owner_loop: 0,
                    edge: 7,
                    next: 18,
                    previous: 18,
                    radial_next: 17,
                },
                crate::history_records::AsmHistoricalCoedge {
                    coedge: 18,
                    owner_loop: 0,
                    edge: 8,
                    next: 17,
                    previous: 17,
                    radial_next: 18,
                },
            ],
            edge_curves: vec![
                crate::history_records::AsmHistoricalOptionalCarrierBinding {
                    entity: 7,
                    carrier: Some(27),
                },
            ],
            coedge_pcurves: vec![
                crate::history_records::AsmHistoricalOptionalCarrierBinding {
                    entity: 17,
                    carrier: Some(37),
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            historical_identity_edge(AsmHistoricalEntityKind::Coedge, 17, &topology),
            Some(7)
        );
        assert_eq!(
            historical_identity_edge(AsmHistoricalEntityKind::Curve, 27, &topology),
            Some(7)
        );
        assert_eq!(
            historical_identity_edge(AsmHistoricalEntityKind::Pcurve, 37, &topology),
            Some(7)
        );
        topology.edge_curves.push(
            crate::history_records::AsmHistoricalOptionalCarrierBinding {
                entity: 8,
                carrier: Some(27),
            },
        );
        assert_eq!(
            historical_identity_edge(AsmHistoricalEntityKind::Curve, 27, &topology),
            None
        );
    }

    #[test]
    fn terminal_edge_recipe_faces_use_exact_then_alternate_references() {
        use cadmpeg_ir::ids::FaceId;

        let reference =
            |candidate_faces, alternate_selector_faces| crate::records::DesignRecipeReference {
                selector: 1,
                selector_offset: 0,
                token: "1".into(),
                token_offset: 0,
                design_reference: 1,
                design_reference_offset: 0,
                candidate_faces,
                candidate_edges: Vec::new(),
                alternate_selector_faces,
                alternate_selector_edges: Vec::new(),
            };
        assert_eq!(
            terminal_edge_recipe_reference_faces(
                &[
                    reference(
                        vec![FaceId("face-c".into())],
                        vec![FaceId("ignored".into())],
                    ),
                    reference(Vec::new(), vec![FaceId("face-d".into())]),
                    reference(vec![FaceId("face-a".into())], Vec::new()),
                ],
                None,
            ),
            vec![
                vec![FaceId("face-c".into())],
                vec![FaceId("face-d".into())],
                vec![FaceId("face-a".into())],
            ]
        );
        let reference_faces = terminal_edge_recipe_reference_faces(
            &[
                reference(
                    vec![FaceId("face-c".into())],
                    vec![FaceId("ignored".into())],
                ),
                reference(Vec::new(), vec![FaceId("face-d".into())]),
                reference(vec![FaceId("face-e".into())], Vec::new()),
            ],
            Some(&[std::num::NonZeroU32::new(2).unwrap()]),
        );
        assert_eq!(reference_faces, vec![vec![FaceId("face-d".into())]]);
        assert_eq!(
            terminal_edge_recipe_faces(
                &[FaceId("face-b".into()), FaceId("face-a".into())],
                &reference_faces,
            ),
            vec![
                FaceId("face-a".into()),
                FaceId("face-b".into()),
                FaceId("face-d".into()),
            ]
        );
    }

    #[test]
    fn treatment_radius_candidates_require_a_new_radius_carrier_and_deleted_support_edge() {
        use cadmpeg_ir::ids::FaceId;

        let relation = |owner_ref, member_refs| AsmHistoricalRelation {
            owner_ref,
            member_refs,
        };
        let coedge = |coedge, owner_loop, edge| AsmHistoricalCoedge {
            coedge,
            owner_loop,
            edge,
            next: coedge,
            previous: coedge,
            radial_next: coedge,
        };
        let preceding = AsmHistoricalTopology {
            faces: vec![10, 11],
            surfaces: vec![100, 101],
            face_loops: vec![relation(10, vec![110]), relation(11, vec![111])],
            loop_coedges: vec![relation(110, vec![1100]), relation(111, vec![1110])],
            coedge_topology: vec![coedge(1100, 110, 17), coedge(1110, 111, 17)],
            face_surfaces: vec![
                AsmHistoricalCarrierBinding {
                    entity: 10,
                    carrier: 100,
                },
                AsmHistoricalCarrierBinding {
                    entity: 11,
                    carrier: 101,
                },
            ],
            ..AsmHistoricalTopology::default()
        };
        let result = AsmHistoricalTopology {
            faces: vec![10, 11, 20],
            surfaces: vec![100, 101, 200],
            surface_radii: vec![AsmHistoricalSurfaceRadius {
                surface: 200,
                radius: 3.0,
            }],
            face_loops: vec![
                relation(10, vec![210]),
                relation(11, vec![211]),
                relation(20, vec![220]),
            ],
            loop_coedges: vec![
                relation(210, vec![2100]),
                relation(211, vec![2110]),
                relation(220, vec![2200, 2201]),
            ],
            coedge_topology: vec![
                coedge(2100, 210, 30),
                coedge(2110, 211, 31),
                coedge(2200, 220, 30),
                coedge(2201, 220, 31),
            ],
            face_surfaces: vec![
                AsmHistoricalCarrierBinding {
                    entity: 10,
                    carrier: 100,
                },
                AsmHistoricalCarrierBinding {
                    entity: 11,
                    carrier: 101,
                },
                AsmHistoricalCarrierBinding {
                    entity: 20,
                    carrier: 200,
                },
            ],
            ..AsmHistoricalTopology::default()
        };
        let candidates = treatment_radius_candidates(
            Some(&[FaceId("f3d:brep:entity#10".into())]),
            &[20],
            &result,
            &preceding,
            &[17],
        );
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].edge_slot, 17);
        assert_eq!(candidates[0].radius, 3.0);
        assert_eq!(
            treatment_transition_edge_candidates(&[20], &result, &preceding, &[17]),
            [17]
        );

        let mut existing_carrier = preceding.clone();
        existing_carrier.surfaces.push(200);
        assert!(treatment_radius_candidates(
            Some(&[FaceId("f3d:brep:entity#10".into())]),
            &[20],
            &result,
            &existing_carrier,
            &[17],
        )
        .is_empty());
        assert!(treatment_transition_edge_candidates(&[20], &result, &preceding, &[18]).is_empty());
        assert!(treatment_radius_candidates(
            Some(&[FaceId("f3d:brep:entity#10".into())]),
            &[20],
            &result,
            &preceding,
            &[18],
        )
        .is_empty());
    }

    #[test]
    fn boundary_edge_change_partition_preserves_boundary_order() {
        assert_eq!(boundary_edges_in_changes(&[8, 3, 5, 2], &[2, 8]), [8, 2]);
        assert!(boundary_edges_in_changes(&[8, 3], &[1, 2]).is_empty());
    }

    #[test]
    fn result_face_support_maps_only_to_one_preceding_owner() {
        use cadmpeg_ir::ids::FaceId;

        let result_faces = [FaceId("f3d:brep:entity#40".into())];
        let result = AsmHistoricalTopology {
            faces: vec![40],
            face_surfaces: vec![AsmHistoricalCarrierBinding {
                entity: 40,
                carrier: 20,
            }],
            ..AsmHistoricalTopology::default()
        };
        let preceding = AsmHistoricalTopology {
            faces: vec![4, 5],
            face_surfaces: vec![
                AsmHistoricalCarrierBinding {
                    entity: 4,
                    carrier: 20,
                },
                AsmHistoricalCarrierBinding {
                    entity: 5,
                    carrier: 21,
                },
            ],
            ..AsmHistoricalTopology::default()
        };
        assert_eq!(
            preceding_support_face_slots(&result_faces, &result, &preceding),
            [4]
        );

        let mut ambiguous = preceding.clone();
        ambiguous.face_surfaces[1].carrier = 20;
        assert!(preceding_support_face_slots(&result_faces, &result, &ambiguous).is_empty());

        let mut ambiguous_result = result.clone();
        ambiguous_result
            .face_surfaces
            .push(AsmHistoricalCarrierBinding {
                entity: 40,
                carrier: 21,
            });
        assert!(
            preceding_support_face_slots(&result_faces, &ambiguous_result, &preceding).is_empty()
        );
    }

    #[test]
    fn active_face_support_retains_invariant_preceding_owners() {
        use cadmpeg_ir::ids::FaceId;

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
        let active = AsmHistoricalTopology {
            faces: vec![40],
            face_surfaces: vec![AsmHistoricalCarrierBinding {
                entity: 40,
                carrier: 20,
            }],
            ..AsmHistoricalTopology::default()
        };
        let history = AsmHistory {
            id: "history".into(),
            byte_offset: 0,
            stream_size: None,
            history_entry_count: None,
            states: vec![state(2, active.clone()), state(3, active)],
        };
        let preceding = AsmHistoricalTopology {
            faces: vec![4, 5],
            face_surfaces: vec![
                AsmHistoricalCarrierBinding {
                    entity: 4,
                    carrier: 20,
                },
                AsmHistoricalCarrierBinding {
                    entity: 5,
                    carrier: 20,
                },
            ],
            ..AsmHistoricalTopology::default()
        };
        let changed_faces = HashSet::from([5]);
        assert_eq!(
            historical_face_support_contexts(
                &[FaceId("f3d:brep:entity#40".into())],
                &[history.clone()],
                &preceding,
                &changed_faces,
            ),
            [crate::records::DesignHistoricalFaceSupportContext {
                active_face_slot: 40,
                surface_slot: 20,
                preceding_face_slots: vec![4, 5],
                preceding_face_boundaries: Vec::new(),
                changed_preceding_face_slots: vec![5],
            }]
        );

        let mut variant = history;
        variant.states[1].topology.as_mut().unwrap().face_surfaces[0].carrier = 21;
        assert!(historical_face_support_contexts(
            &[FaceId("f3d:brep:entity#40".into())],
            &[variant],
            &preceding,
            &changed_faces,
        )
        .is_empty());
    }

    #[test]
    fn face_changes_span_complete_intermediate_state_chain() {
        let state = |state_id| AsmDeltaState {
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
            topology: Some(AsmHistoricalTopology::default()),
            transition: None,
        };
        let preceding = state(1);
        let mut intermediate = state(2);
        let mut result = state(3);
        let mut first = AsmHistoricalTransition {
            previous_state_id: Some(1),
            records: AsmHistoricalEntityDelta::default(),
            topology: AsmHistoricalTopologyDelta::default(),
        };
        first.topology.faces.updated = vec![10];
        intermediate.transition = Some(first);
        let mut second = AsmHistoricalTransition {
            previous_state_id: Some(2),
            records: AsmHistoricalEntityDelta::default(),
            topology: AsmHistoricalTopologyDelta::default(),
        };
        second.topology.faces.deleted = vec![11];
        result.transition = Some(second);
        let states = HashMap::from([
            (1, Some(&preceding)),
            (2, Some(&intermediate)),
            (3, Some(&result)),
        ]);

        assert_eq!(
            face_changes_across_state_chain(&result, 1, &states),
            Some(HashSet::from([10, 11]))
        );
        let incomplete = HashMap::from([(1, Some(&preceding)), (3, Some(&result))]);
        assert_eq!(
            face_changes_across_state_chain(&result, 1, &incomplete),
            None
        );
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
            boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
            vertex_uses: Vec::new(),
        });
        brep.coedges.push(Coedge {
            id: CoedgeId(id(6)),
            owner_loop: LoopId(id(5)),
            edge: EdgeId(id(7)),
            next: CoedgeId(id(6)),
            previous: CoedgeId(id(6)),
            radial_next: CoedgeId(id(6)),
            sense: Sense::Forward,
            pcurves: Vec::new(),
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
                    incident_edge_ordinal: Some(boundary_edge_count - 1),
                    incident_side: Some(crate::records::DesignTopologyIncidentSide::Preceding),
                },
                crate::records::DesignTopologyRecipeTriplet {
                    outer: std::num::NonZeroU32::new(1).unwrap(),
                    middle: 1,
                    vertex_ordinal: 0,
                    incident_edge_ordinal: Some(0),
                    incident_side: Some(crate::records::DesignTopologyIncidentSide::Following),
                },
            ],
        };
        let side = |entries: Vec<crate::records::DesignTopologyRecipeEntry>| {
            crate::records::DesignTopologyRecipeSide {
                field_count: std::num::NonZeroU32::new(3).unwrap(),
                header_value: 0,
                scalars: vec![0, 0],
                payload_prefix: vec![0],
                payload_entry_count: u32::try_from(entries.len()).unwrap(),
                entries,
            }
        };
        let structure = crate::records::DesignEdgeRecipeStructure {
            root: 2,
            sides: vec![
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
        let mut reference = crate::records::DesignRecipeReference {
            selector: 1,
            selector_offset: 0,
            token: "1".into(),
            token_offset: 0,
            design_reference: 1,
            design_reference_offset: 1,
            candidate_faces: vec![FaceId(id(4))],
            candidate_edges: Vec::new(),
            alternate_selector_faces: Vec::new(),
            alternate_selector_edges: Vec::new(),
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
        let boundary = crate::records::DesignHistoricalFaceBoundaryContext {
            face_slot: 4,
            loops: vec![crate::records::DesignHistoricalFaceLoopContext {
                loop_slot: 5,
                coedge_slots: vec![6],
                edge_slots: vec![7],
                vertex_slots: Vec::new(),
                point_slots: Vec::new(),
                positions: Vec::new(),
            }],
        };
        assert_eq!(context.result_face_boundaries, [boundary.clone()]);
        assert_eq!(context.result_shared_edge_slots, [7]);
        assert_eq!(context.preceding_faces, [FaceId(id(4))]);
        assert_eq!(context.preceding_face_boundaries, [boundary]);
        assert_eq!(context.preceding_support_face_slots, [4]);
        assert_eq!(context.preceding_support_face_boundaries.len(), 1);
        assert_eq!(context.shared_edge_slots, [7]);
        assert_eq!(context.changed_shared_edge_slots, [7]);
        assert_eq!(context.changed_reference_edge_slots, [7]);
        reference.candidate_faces.clear();
        reference.alternate_selector_faces = vec![FaceId(id(4))];
        let alternate_context = edge_recipe_reference_context(
            2,
            &reference,
            &topology,
            &[7, 99],
            &topology,
            &[7, 98],
            &HashSet::from([7]),
        );
        assert_eq!(alternate_context.result_faces, [FaceId(id(4))]);
        assert_eq!(alternate_context.preceding_faces, [FaceId(id(4))]);
        assert_eq!(alternate_context.changed_reference_edge_slots, [7]);
        let support_only_context = edge_recipe_reference_context(
            2,
            &reference,
            &topology,
            &[99],
            &topology,
            &[98],
            &HashSet::from([7]),
        );
        assert!(support_only_context.shared_edge_slots.is_empty());
        assert_eq!(support_only_context.changed_reference_edge_slots, [7]);
        let cyclic = AsmHistoricalTopology {
            edge_vertices: vec![
                AsmHistoricalEdge {
                    edge: 7,
                    start_vertex: 1,
                    end_vertex: 2,
                },
                AsmHistoricalEdge {
                    edge: 8,
                    start_vertex: 3,
                    end_vertex: 2,
                },
                AsmHistoricalEdge {
                    edge: 9,
                    start_vertex: 1,
                    end_vertex: 3,
                },
            ],
            ..AsmHistoricalTopology::default()
        };
        assert_eq!(
            ordered_loop_vertices(&[7, 8, 9], &cyclic),
            Some(vec![1, 2, 3])
        );
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
        let duplicate_state_history = AsmHistory {
            id: "duplicate-state-history".into(),
            byte_offset: 0,
            stream_size: None,
            history_entry_count: None,
            states: vec![state(
                3,
                AsmHistoricalTopology {
                    vertices: vec![42],
                    ..AsmHistoricalTopology::default()
                },
            )],
        };
        assert_eq!(
            historical_identity_kind(&[history.clone(), duplicate_state_history.clone()], 42),
            Some((AsmHistoricalEntityKind::Edge, vec![5]))
        );
        let mut duplicate_revision_history = duplicate_state_history;
        duplicate_revision_history.states[0].entity_versions = vec![AsmEntityVersion {
            entity_ref: 42,
            record_ref: 700,
        }];
        assert_eq!(
            historical_selection_identity_kind(
                &[revision_history.clone(), duplicate_revision_history],
                700,
            ),
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
    fn nested_entity_identity_resolves_through_input_coedge_incidence() {
        let topology = AsmHistoricalTopology {
            coedges: vec![42],
            edges: vec![17, 18],
            vertices: vec![50, 51, 52],
            coedge_topology: vec![AsmHistoricalCoedge {
                coedge: 42,
                owner_loop: 5,
                edge: 17,
                next: 42,
                previous: 42,
                radial_next: 42,
            }],
            edge_vertices: vec![
                AsmHistoricalEdge {
                    edge: 17,
                    start_vertex: 50,
                    end_vertex: 51,
                },
                AsmHistoricalEdge {
                    edge: 18,
                    start_vertex: 50,
                    end_vertex: 52,
                },
            ],
            ..AsmHistoricalTopology::default()
        };
        let history = AsmHistory {
            id: "history".into(),
            byte_offset: 0,
            stream_size: None,
            history_entry_count: None,
            states: vec![AsmDeltaState {
                id: "state-3".into(),
                parent: "history".into(),
                byte_offset: 0,
                state_id: 3,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: 3,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                entity_versions: vec![
                    AsmEntityVersion {
                        entity_ref: 42,
                        record_ref: 700,
                    },
                    AsmEntityVersion {
                        entity_ref: 50,
                        record_ref: 800,
                    },
                ],
                record_table_complete: true,
                topology: Some(topology.clone()),
                transition: None,
            }],
        };
        let candidates = entity_selection_edge_candidates([700, 800], 3, &[history], &topology);
        assert_eq!(
            candidates,
            [
                crate::records::DesignEntitySelectionEdgeCandidate {
                    identity_ordinal: 0,
                    local_id: 700,
                    historical_entity_kind: AsmHistoricalEntityKind::Coedge,
                    historical_entity_ref: 42,
                    edge_slots: vec![17],
                },
                crate::records::DesignEntitySelectionEdgeCandidate {
                    identity_ordinal: 1,
                    local_id: 800,
                    historical_entity_kind: AsmHistoricalEntityKind::Vertex,
                    historical_entity_ref: 50,
                    edge_slots: vec![17, 18],
                },
            ]
        );
        assert_eq!(unique_entity_selection_edge(&candidates), Some(17));
    }

    #[test]
    fn body_selection_proofs_distinguish_stable_and_topology_changing_operations() {
        let state = |state_id, transition| AsmDeltaState {
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
            topology: Some(AsmHistoricalTopology {
                bodies: vec![7],
                ..AsmHistoricalTopology::default()
            }),
            transition,
        };
        let previous = state(10, None);
        let mut transition = AsmHistoricalTransition {
            previous_state_id: Some(10),
            records: AsmHistoricalEntityDelta::default(),
            topology: AsmHistoricalTopologyDelta::default(),
        };
        transition.topology.bodies.updated.push(7);
        let current = state(11, Some(transition.clone()));
        assert_eq!(
            body_revision_without_topology_change(&current),
            Some(TopologyStableBodyRevision::Revised(7))
        );

        transition.topology.points.updated.push(31);
        transition.topology.surfaces.inserted.push(32);
        transition.topology.curves.deleted.push(33);
        transition.topology.pcurves.updated.push(34);
        let carrier_revisions = state(11, Some(transition.clone()));
        assert_eq!(
            body_revision_without_topology_change(&carrier_revisions),
            Some(TopologyStableBodyRevision::Revised(7))
        );

        let mut intermediate_transition = AsmHistoricalTransition {
            previous_state_id: Some(10),
            records: AsmHistoricalEntityDelta::default(),
            topology: AsmHistoricalTopologyDelta::default(),
        };
        intermediate_transition.topology.bodies.updated.push(7);
        let intermediate = state(11, Some(intermediate_transition));
        transition.previous_state_id = Some(11);
        let result = state(12, Some(transition.clone()));
        let states = HashMap::from([
            (10, Some(&previous)),
            (11, Some(&intermediate)),
            (12, Some(&result)),
        ]);
        assert_eq!(
            singleton_body_revision_across_state_chain(&result, 10, &states),
            Some(7)
        );

        let topology = |bodies: &[i64]| AsmHistoricalTopology {
            bodies: bodies.to_vec(),
            ..AsmHistoricalTopology::default()
        };
        let mut split_previous = state(20, None);
        split_previous.topology = Some(topology(&[7, 8]));
        let mut split_transition = AsmHistoricalTransition {
            previous_state_id: Some(20),
            records: AsmHistoricalEntityDelta::default(),
            topology: AsmHistoricalTopologyDelta::default(),
        };
        split_transition.topology.bodies.updated.push(7);
        split_transition.topology.bodies.inserted.push(9);
        let mut split_result = state(21, Some(split_transition.clone()));
        split_result.topology = Some(topology(&[7, 8, 9]));
        let split_states = HashMap::from([(20, Some(&split_previous)), (21, Some(&split_result))]);
        assert_eq!(
            singleton_revised_input_body_across_state_chain(&split_result, 20, &split_states),
            Some(7)
        );

        split_transition.topology.bodies.updated.push(8);
        let mut ambiguous_split = state(21, Some(split_transition));
        ambiguous_split.topology = Some(topology(&[7, 8, 9]));
        let ambiguous_states =
            HashMap::from([(20, Some(&split_previous)), (21, Some(&ambiguous_split))]);
        assert_eq!(
            singleton_revised_input_body_across_state_chain(
                &ambiguous_split,
                20,
                &ambiguous_states
            ),
            None
        );

        transition.previous_state_id = Some(10);
        transition.topology.faces.updated.push(19);
        let topology_changing = state(11, Some(transition));
        assert_eq!(
            body_revision_without_topology_change(&topology_changing),
            None
        );
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
                tokens: Vec::new().into(),
                offset: 0,
                len: 0,
            })
            .collect::<Vec<_>>();

        let archive =
            historical_record_archive(std::slice::from_ref(&state), &active, &archived_bytes, 8)
                .expect("complete historical record archive");
        let table =
            materialize_record_table(&state, &archive).expect("complete historical RecordTable");

        assert_eq!(table.len(), 2);
        assert_eq!(table[1].index, 1);
        assert_eq!(&*table[1].tokens, [crate::sab::Token::Ref(1)]);
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

    #[test]
    fn profile_face_group_cardinality_requires_one_changed_surface_family() {
        let topology = AsmHistoricalTopology {
            faces: vec![10, 11, 12, 20],
            face_surfaces: vec![
                AsmHistoricalCarrierBinding {
                    entity: 10,
                    carrier: 100,
                },
                AsmHistoricalCarrierBinding {
                    entity: 11,
                    carrier: 100,
                },
                AsmHistoricalCarrierBinding {
                    entity: 12,
                    carrier: 100,
                },
                AsmHistoricalCarrierBinding {
                    entity: 20,
                    carrier: 200,
                },
            ],
            ..AsmHistoricalTopology::default()
        };
        let changed = [20, 12, 10, 11].into_iter().collect();
        assert_eq!(
            profile_face_group_cardinality_candidates(&topology, &changed, 3),
            Some(vec![10, 11, 12])
        );
        assert_eq!(
            profile_face_group_cardinality_candidates(&topology, &[20].into_iter().collect(), 1,),
            Some(vec![20])
        );
        assert_eq!(
            profile_face_group_cardinality_candidates(
                &topology,
                &[10, 20].into_iter().collect(),
                1,
            ),
            None
        );

        let mut ambiguous = topology;
        ambiguous.faces.extend([30, 31, 32]);
        ambiguous
            .face_surfaces
            .extend([30, 31, 32].map(|entity| AsmHistoricalCarrierBinding {
                entity,
                carrier: 300,
            }));
        let changed = [10, 11, 12, 30, 31, 32].into_iter().collect();
        assert_eq!(
            profile_face_group_cardinality_candidates(&ambiguous, &changed, 3),
            None
        );
    }

    #[test]
    fn direct_face_recipe_clauses_resolve_ordered_changed_intersections() {
        use cadmpeg_ir::ids::FaceId;

        let reference =
            |selector_offset, candidates: &[i64]| crate::records::DesignRecipeReference {
                selector: 1,
                selector_offset,
                token: "x".into(),
                token_offset: selector_offset + 1,
                design_reference: 1,
                design_reference_offset: selector_offset + 2,
                candidate_faces: candidates
                    .iter()
                    .map(|face| FaceId(format!("f3d:brep:entity#{face}")))
                    .collect(),
                candidate_edges: Vec::new(),
                alternate_selector_faces: Vec::new(),
                alternate_selector_edges: Vec::new(),
            };
        let references = [
            reference(10, &[1, 2]),
            reference(10, &[2, 3]),
            reference(20, &[4, 5]),
            reference(20, &[4, 6]),
            reference(30, &[2]),
        ];
        let topology = AsmHistoricalTopology {
            faces: vec![1, 2, 3, 4, 5, 6],
            ..AsmHistoricalTopology::default()
        };

        assert_eq!(
            resolve_direct_face_recipe_clauses(
                &references,
                &topology,
                &[2, 4].into_iter().collect()
            ),
            [2, 4]
        );
        assert!(resolve_direct_face_recipe_clauses(
            &references,
            &topology,
            &[2].into_iter().collect()
        )
        .is_empty());
    }

    #[test]
    fn bounded_face_copy_matches_cyclic_boundary_with_split_vertices() {
        use cadmpeg_ir::math::Point3;

        let point = |x, y| Point3 { x, y, z: 0.0 };
        let source = [
            point(0.0, 0.0),
            point(2.0, 0.0),
            point(2.0, 2.0),
            point(0.0, 2.0),
        ];
        let split_copy = [
            point(2.0, 2.0),
            point(1.0, 2.0),
            point(0.0, 2.0),
            point(0.0, 0.0),
            point(2.0, 0.0),
        ];
        assert!(cyclic_point_subsequence(&source, &split_copy));

        let reversed = split_copy.iter().copied().rev().collect::<Vec<_>>();
        assert!(cyclic_point_subsequence(&source, &reversed));

        let wrong_order = [
            point(0.0, 0.0),
            point(2.0, 2.0),
            point(2.0, 0.0),
            point(0.0, 2.0),
        ];
        assert!(!cyclic_point_subsequence(&source, &wrong_order));
        assert!(!cyclic_point_subsequence(&source, &split_copy[..3]));
    }

    #[test]
    fn bounded_face_identity_selects_ordered_deleted_treatment_edges() {
        use crate::records::{
            ConstructionRecipeKind, DesignEdgeIdentityOperand, DesignFaceOperand,
            DesignHistoricalFaceBoundaryContext, DesignHistoricalFaceLoopContext,
            DesignHistoricalFaceSupportContext,
        };

        let mut identities = vec![DesignEdgeIdentityOperand {
            id: "f3d:Design/BulkStream.dat:edge-identity#10".into(),
            scope_record_index: 1,
            group_record_index: 2,
            group_member_ordinal: 0,
            record_index: 10,
            byte_offset: 100,
            class_tag: "297".into(),
            compact_layout: false,
            local_id: 13,
            local_id_offset: 123,
            asset_id: "asset".into(),
            asset_id_offset: 0,
            context_id: "context".into(),
            context_id_offset: 0,
            historical_entity_kind: None,
            historical_entity_ref: None,
            historical_state_ids: Vec::new(),
            treatment_radius_candidates: Vec::new(),
            transition_edge_candidates: vec![7, 8, 9],
            resolved_edge_slots: Vec::new(),
            resolved_edge_slot: None,
            resolution_identity_id: None,
        }];
        let face = DesignFaceOperand {
            id: "f3d:Design/BulkStream.dat:design-face-operand#10".into(),
            scope_record_index: 1,
            scope_reference_ordinal: 0,
            group_record_index: Some(2),
            group_member_ordinal: Some(0),
            record_index: 10,
            byte_offset: 100,
            class_tag: "297".into(),
            paired_byte_offset: 200,
            paired_class_tag: "259".into(),
            recipe_record_index: 13,
            recipe_record_byte_offset: 300,
            recipe_id: "recipe".into(),
            recipe_prefix_offset: 0,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_kind: ConstructionRecipeKind::BoundedFace,
            recipe_program_offset: 0,
            recipe_program: vec![0],
            recipe_node_offsets: Vec::new(),
            recipe_nodes: Vec::new(),
            candidate_faces: Vec::new(),
            unreferenced_candidate_faces: Vec::new(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: vec![DesignHistoricalFaceSupportContext {
                active_face_slot: 30,
                surface_slot: 40,
                preceding_face_slots: vec![50],
                preceding_face_boundaries: vec![DesignHistoricalFaceBoundaryContext {
                    face_slot: 50,
                    loops: vec![DesignHistoricalFaceLoopContext {
                        loop_slot: 60,
                        coedge_slots: vec![70, 71, 72],
                        edge_slots: vec![8, 6, 7],
                        vertex_slots: Vec::new(),
                        point_slots: Vec::new(),
                        positions: Vec::new(),
                    }],
                }],
                changed_preceding_face_slots: vec![50],
            }],
            resolved_face_slots: Vec::new(),
            next_record_index: 14,
            next_byte_offset: 400,
        };

        bind_edge_identity_bounded_face_rules(&mut identities, &[face.clone()]);
        assert_eq!(identities[0].resolved_edge_slots, [8, 7]);
        assert_eq!(
            identities[0].resolution_identity_id.as_deref(),
            Some(face.id.as_str())
        );

        let mut inconsistent = face;
        inconsistent.historical_support_contexts[0]
            .changed_preceding_face_slots
            .clear();
        bind_edge_identity_bounded_face_rules(&mut identities, &[inconsistent]);
        assert!(identities[0].resolved_edge_slots.is_empty());
    }
}
