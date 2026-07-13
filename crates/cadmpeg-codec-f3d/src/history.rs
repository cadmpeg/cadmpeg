// SPDX-License-Identifier: Apache-2.0
//! Decode the ASM construction-history partition after the active model slice.
//!
//! [`decode`] reads `delta_state` headers, bulletin-board entity changes, and
//! history records while retaining source bytes for records without typed
//! semantics.

use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmHistoricalModel,
    AsmHistory, AsmHistoryRecord,
};
use cadmpeg_ir::le::int_at;
use std::collections::{BTreeMap, HashMap, HashSet};

const DELTA: &[u8] = b"\x11\x0d\x0bdelta_state";
const PREAMBLE: &[u8] = b"\x0d\x0ehistory_stream";

pub(crate) fn reconstruct_models(
    history: &AsmHistory,
    active_records: &[crate::sab::Record],
    bytes: &[u8],
    width: usize,
) -> Option<Vec<AsmHistoricalModel>> {
    if !graph_is_coherent(history) {
        return None;
    }
    let mut snapshots = BTreeMap::new();
    for stored in history
        .states
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| record.table_index.is_some())
    {
        let table_index = usize::try_from(stored.table_index?).ok()?;
        let mut framed =
            crate::sab::frame(&stored.raw_bytes, 0, stored.raw_bytes.len(), width).ok()?;
        if framed.len() != 1 {
            return None;
        }
        let mut record = framed.pop()?;
        record.index = table_index;
        record.offset = usize::try_from(stored.byte_offset).ok()?;
        snapshots.insert(table_index, record);
    }
    if snapshots.is_empty() {
        return Some(Vec::new());
    }

    let by_node = history
        .states
        .iter()
        .map(|state| (state.node_index, state))
        .collect::<HashMap<_, _>>();
    let mut state = history
        .states
        .iter()
        .find(|state| state.previous_ref.is_none())?;
    let mut table = active_records
        .iter()
        .cloned()
        .map(|record| (record.index, record))
        .collect::<BTreeMap<_, _>>();
    let mut models = Vec::new();
    while let Some(next_index) = state.next_ref {
        for change in state
            .bulletin_boards
            .iter()
            .flat_map(|board| &board.changes)
        {
            match change.kind {
                AsmEntityChangeKind::Insert => {
                    table.remove(&usize::try_from(change.new_ref?).ok()?);
                }
                AsmEntityChangeKind::Delete => {
                    let old = usize::try_from(change.old_ref?).ok()?;
                    if let Some(record) = snapshots.get(&old) {
                        table.insert(old, record.clone());
                    }
                }
                AsmEntityChangeKind::Update => {
                    table.remove(&usize::try_from(change.new_ref?).ok()?);
                    let old = usize::try_from(change.old_ref?).ok()?;
                    if let Some(record) = snapshots.get(&old) {
                        table.insert(old, record.clone());
                    }
                }
            }
        }
        let next = *by_node.get(&next_index)?;
        let records = table.values().cloned().collect::<Vec<_>>();
        let mut model = crate::brep::into_model(crate::brep::decode(&records, bytes, "history"));
        model.finalize();
        models.push(AsmHistoricalModel {
            id: format!("{}:model#{}", history.id, next.node_index),
            history: history.id.clone(),
            state_id: next.state_id,
            node_index: next.node_index,
            model,
        });
        state = next;
    }
    Some(models)
}

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
    visited.len() == history.states.len() && snapshot_indices_are_coherent(history)
}

pub(crate) fn reconstructed_models_are_coherent(history: &AsmHistory) -> bool {
    let has_snapshot = history.states.iter().any(|state| {
        state
            .records
            .first()
            .is_some_and(|record| record.name == "End-of-ASM-History-Section")
    });
    if !has_snapshot {
        return history.historical_models.is_empty();
    }
    let Some(mut current) = history
        .states
        .iter()
        .find(|state| state.previous_ref.is_none())
    else {
        return false;
    };
    let by_node = history
        .states
        .iter()
        .map(|state| (state.node_index, state))
        .collect::<HashMap<_, _>>();
    let by_model = history
        .historical_models
        .iter()
        .map(|model| (model.node_index, model))
        .collect::<HashMap<_, _>>();
    if by_model.len() != history.states.len().saturating_sub(1) {
        return false;
    }
    while let Some(next_index) = current.next_ref {
        let (Some(next), Some(model)) = (by_node.get(&next_index), by_model.get(&next_index))
        else {
            return false;
        };
        if model.history != history.id || model.state_id != next.state_id {
            return false;
        }
        current = next;
    }
    true
}

fn snapshot_indices_are_coherent(history: &AsmHistory) -> bool {
    let old_refs = history
        .states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
        .filter_map(|change| change.old_ref)
        .collect::<Vec<_>>();
    let Some(snapshot) = history.states.iter().find(|state| {
        state
            .records
            .first()
            .is_some_and(|record| record.name == "End-of-ASM-History-Section")
    }) else {
        return history
            .states
            .iter()
            .flat_map(|state| &state.records)
            .all(|record| record.table_index.is_none());
    };
    if snapshot
        .records
        .last()
        .is_none_or(|record| record.name != "End-of-ASM-data")
    {
        return false;
    }
    let Some(base) = old_refs.iter().copied().min() else {
        return false;
    };
    let semantic = &snapshot.records[1..snapshot.records.len() - 1];
    semantic.iter().enumerate().all(|(ordinal, record)| {
        i64::try_from(ordinal + 1)
            .ok()
            .and_then(|index| base.checked_add(index))
            == record.table_index
    }) && snapshot.records[0].table_index.is_none()
        && snapshot.records.last().unwrap().table_index.is_none()
        && old_refs
            .iter()
            .all(|reference| *reference >= base && *reference <= base + semantic.len() as i64)
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
        });
    }
    if states.is_empty() {
        return None;
    }

    let (stream_size, history_entry_count) = preamble_offset
        .and_then(|offset| decode_preamble(bytes, offset + PREAMBLE.len(), width))
        .map_or((None, None), |(size, high)| (Some(size), Some(high)));
    assign_snapshot_table_indices(&mut states);
    let offset = history_offset;
    Some(AsmHistory {
        id: history_id,
        byte_offset: offset as u64,
        stream_size,
        history_entry_count,
        states,
        historical_models: Vec::new(),
    })
}

fn assign_snapshot_table_indices(states: &mut [AsmDeltaState]) {
    let Some(base) = states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
        .filter_map(|change| change.old_ref)
        .min()
    else {
        return;
    };
    let Some(snapshot) = states.iter_mut().find(|state| {
        state
            .records
            .first()
            .is_some_and(|record| record.name == "End-of-ASM-History-Section")
            && state
                .records
                .last()
                .is_some_and(|record| record.name == "End-of-ASM-data")
    }) else {
        return;
    };
    let last = snapshot.records.len().saturating_sub(1);
    for (local_index, record) in snapshot.records.iter_mut().enumerate() {
        if local_index != 0 && local_index != last {
            record.table_index = i64::try_from(local_index)
                .ok()
                .and_then(|index| base.checked_add(index));
        }
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
            .map(|record| AsmHistoryRecord {
                id: format!("f3d:{stream}:asm-history-record#{:010}", record.offset),
                parent: state_id.to_string(),
                index: record.index as u64,
                byte_offset: record.offset as u64,
                table_index: None,
                name: record.name,
                raw_bytes: bytes[record.offset..record.offset + record.len].to_vec(),
            })
            .collect(),
        Err(_) => {
            vec![AsmHistoryRecord {
                id: format!("f3d:{stream}:asm-history-record#{start:010}"),
                parent: state_id.to_string(),
                index: 0,
                byte_offset: start as u64,
                table_index: None,
                name: "opaque_history_payload".into(),
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
    use super::assign_snapshot_table_indices;
    use crate::history_records::{
        AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmHistoryRecord,
    };

    fn record(index: u64, name: &str) -> AsmHistoryRecord {
        AsmHistoryRecord {
            id: format!("record-{index}"),
            parent: "state".into(),
            index,
            byte_offset: 0,
            table_index: None,
            name: name.into(),
            raw_bytes: vec![1],
        }
    }

    #[test]
    fn snapshot_local_indices_restore_record_table_identity() {
        let mut states = vec![AsmDeltaState {
            id: "state".into(),
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
                id: "board".into(),
                parent: "state".into(),
                byte_offset: 0,
                owner_ref: 0,
                number: 0,
                changes: vec![AsmEntityChange {
                    id: "change".into(),
                    parent: "board".into(),
                    byte_offset: 0,
                    kind: AsmEntityChangeKind::Delete,
                    old_ref: Some(40),
                    new_ref: None,
                }],
            }],
            records: vec![
                record(0, "End-of-ASM-History-Section"),
                record(1, "edge"),
                record(2, "point"),
                record(3, "End-of-ASM-data"),
            ],
        }];

        assign_snapshot_table_indices(&mut states);

        assert_eq!(
            states[0]
                .records
                .iter()
                .map(|record| record.table_index)
                .collect::<Vec<_>>(),
            vec![None, Some(41), Some(42), None]
        );
    }
}
