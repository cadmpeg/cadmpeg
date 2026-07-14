// SPDX-License-Identifier: Apache-2.0
//! Decode the ASM construction-history partition after the active model slice.
//!
//! [`decode`] reads `delta_state` headers, bulletin-board entity changes, and
//! history records while retaining source bytes for records without typed
//! semantics.

use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmEntityVersion,
    AsmHistory, AsmHistoryRecord,
};
use cadmpeg_ir::le::int_at;
use std::collections::{BTreeMap, HashMap, HashSet};

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
        });
    }
    bind_snapshot_revision_ids(&mut states);
    bind_historical_entity_versions(&mut states);
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
    let old_references = states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
        .filter_map(|change| change.old_ref)
        .collect::<Vec<_>>();
    if old_references.iter().copied().collect::<HashSet<_>>().len() != old_references.len() {
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
