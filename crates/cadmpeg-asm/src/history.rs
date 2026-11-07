// SPDX-License-Identifier: Apache-2.0
//! ASM `delta_state` history: `BulletinBoard` chain framing and historical
//! `RecordTable` materialization (`docs/formats/asm.md` section 3.2).

use crate::kernel_header::RefWidth;
use crate::sab::{Record, Token};
use crate::stream_error::StreamFailure;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Classified entity change of a `BulletinBoard` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityChange {
    /// Null to entity.
    Insert {
        /// Reference of the inserted entity.
        new: i64,
    },
    /// Entity to null.
    Delete {
        /// Reference of the deleted entity.
        old: i64,
    },
    /// Entity to entity.
    Update {
        /// Reference of the previous revision.
        old: i64,
        /// Reference of the new revision.
        new: i64,
    },
}

/// One framed item of a `BulletinBoard` chain, in stream order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulletinBoardItem {
    /// A board header.
    Board {
        /// Byte offset of the board header.
        offset: usize,
        /// Owner reference.
        owner_ref: i64,
        /// Board number.
        number: i64,
    },
    /// A change of the most recent board.
    Change {
        /// Byte offset of the change.
        offset: usize,
        /// Classified change.
        kind: EntityChange,
    },
}

/// Read a tagged little-endian signed integer of the stream's ref width (4 or
/// 8 bytes) and advance past it.
fn take_int(bytes: &[u8], position: &mut usize, tag: u8, width: RefWidth) -> Option<i64> {
    if bytes.get(*position) != Some(&tag) {
        return None;
    }
    let at = *position + 1;
    let value = match width {
        RefWidth::Four => i64::from(View::i32_le_at(bytes, at)?),
        RefWidth::Eight => View::i64_le_at(bytes, at)?,
    };
    *position += 1 + width.bytes();
    Some(value)
}

/// Walk the `BulletinBoard` chain at `position` and return the offset after
/// it, or `None` when the chain is malformed.
pub fn walk_bulletin_boards(
    bytes: &[u8],
    mut position: usize,
    width: RefWidth,
    mut visit: impl FnMut(BulletinBoardItem),
) -> Option<usize> {
    if bytes.get(position) == Some(&0x11) {
        return Some(position);
    }
    loop {
        let board_offset = position;
        let present = take_int(bytes, &mut position, 0x04, width)?;
        if present == 0 {
            break;
        }
        let owner_ref = take_int(bytes, &mut position, 0x0c, width)?;
        let number = take_int(bytes, &mut position, 0x04, width)?;
        visit(BulletinBoardItem::Board {
            offset: board_offset,
            owner_ref,
            number,
        });
        loop {
            let change_offset = position;
            let present = take_int(bytes, &mut position, 0x04, width)?;
            if present == 0 {
                break;
            }
            let old = take_int(bytes, &mut position, 0x0c, width)?;
            let new = take_int(bytes, &mut position, 0x0c, width)?;
            let kind = match (old >= 0, new >= 0) {
                (false, true) => EntityChange::Insert { new },
                (true, false) => EntityChange::Delete { old },
                (true, true) => EntityChange::Update { old, new },
                (false, false) => return None,
            };
            visit(BulletinBoardItem::Change {
                offset: change_offset,
                kind,
            });
        }
    }
    Some(position)
}

/// Materialize the `RecordTable` of one historical state.
///
/// `versions` maps each live entity slot to its record revision; `lookup`
/// returns the record of a revision. The result is ordered by record index.
/// It is `None` when `versions` is empty, a revision has no record, a
/// record's index differs from its slot, or a non-negative reference leaves
/// the slot set.
pub fn materialize_record_table<'a>(
    versions: &BTreeMap<i64, i64>,
    lookup: impl Fn(i64) -> Option<&'a Record>,
) -> Option<Vec<Record>> {
    if versions.is_empty() {
        return None;
    }
    let mut records = Vec::new();
    for (&entity_ref, &record_ref) in versions {
        let record = lookup(record_ref)?;
        if i64::try_from(record.index).ok() != Some(entity_ref) {
            return None;
        }
        for token in record.tokens.iter() {
            let Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0 && !versions.contains_key(reference) {
                return None;
            }
        }
        records.push(record.clone());
    }
    records.sort_unstable_by_key(|a| a.index);
    Some(records)
}

/// Name of the record that closes the history section.
const HISTORY_SECTION_END: &str = "End-of-ASM-History-Section";
/// Name of the record that closes the active record stream.
const ACTIVE_DATA_END: &str = "End-of-ASM-data";

/// One record of a `delta_state`, as the history walks read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRecordView<'a> {
    /// Revision identifier of an archived record; `None` for a boundary record.
    pub revision_id: Option<i64>,
    /// Byte offset of the record in the stream.
    pub byte_offset: u64,
    /// Stored bytes of the record.
    pub raw_bytes: &'a [u8],
    /// Record name.
    pub name: &'a str,
}

/// One `delta_state`, as the history walks read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeltaStateView<'a> {
    /// Node index of the state.
    pub node_index: i64,
    /// Node index of the previous state; `None` for the head.
    pub previous_ref: Option<i64>,
    /// Node index of the next state; `None` for the last state.
    pub next_ref: Option<i64>,
    /// Bulletin-board changes of the state, in stream order.
    pub changes: Vec<EntityChange>,
    /// Records of the state, in stream order.
    pub records: Vec<StateRecordView<'a>>,
}

impl EntityChange {
    fn old_ref(self) -> Option<i64> {
        match self {
            Self::Insert { .. } => None,
            Self::Delete { old } | Self::Update { old, .. } => Some(old),
        }
    }

    fn new_ref(self) -> Option<i64> {
        match self {
            Self::Delete { .. } => None,
            Self::Insert { new } | Self::Update { new, .. } => Some(new),
        }
    }
}

fn is_history_boundary_record(record: &StateRecordView<'_>) -> bool {
    matches!(record.name, HISTORY_SECTION_END | ACTIVE_DATA_END)
}

/// Active `RecordTable` length of a history whose archived revision
/// identifiers form the run `n..n + count` with `n > 0`; `n` is the length.
pub fn archived_active_record_count(
    states: &[DeltaStateView<'_>],
) -> Result<Option<usize>, CodecError> {
    let mut archived: Vec<i64> = states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
        .collect();
    archived.sort_unstable_by(Ord::cmp);
    let Some(&active_count) = archived.first() else {
        return Ok(None);
    };
    let count = i64::try_from(archived.len())
        .map_err(|_| CodecError::malformed("archived revision count exceeds i64"))?;
    let Some(end) = active_count.checked_add(count) else {
        return Ok(None);
    };
    if active_count <= 0 || archived.iter().copied().ne(active_count..end) {
        return Ok(None);
    }
    Ok(usize::try_from(active_count).ok())
}

/// Active `RecordTable` length of a history that has no archived snapshot.
/// Insert-only chains use the active records themselves as every revision, so
/// their bulletin-board references cover every non-header slot exactly once.
pub fn insert_only_active_record_count(states: &[DeltaStateView<'_>]) -> Option<usize> {
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
    for change in states.iter().flat_map(|state| &state.changes) {
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

/// Walk the state chain from its head and give each state's entity-to-record
/// version map to `visit`, before that state's changes apply. Returns false,
/// after a partial walk, when the chain or a change is inconsistent with the
/// sorted `archived_ids`, or when the walk does not end at `{0: 0}`.
pub fn walk_entity_versions(
    states: &[DeltaStateView<'_>],
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
        for change in &state.changes {
            match *change {
                EntityChange::Update { old, new } => {
                    if !versions.contains_key(&new) || archived_ids.binary_search(&old).is_err() {
                        return false;
                    }
                    versions.insert(new, old);
                }
                EntityChange::Insert { new } => {
                    if versions.remove(&new).is_none() {
                        return false;
                    }
                }
                EntityChange::Delete { old } => {
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

/// Framed records of a history stream: the active `RecordTable` and the
/// archived revisions keyed by revision identifier.
#[derive(Debug)]
pub struct RecordArchiveFrames {
    /// Active records; the record at position `i` has index `i`.
    pub active: Vec<Record>,
    /// Archived records by revision identifier.
    pub archived: HashMap<i64, Record>,
}

/// The refusal of a framing failure that must propagate; other failures mean
/// the stream does not agree with the states.
fn propagated_error(failure: StreamFailure) -> Option<CodecError> {
    match failure {
        StreamFailure::Resource(error) => Some(error.into()),
        StreamFailure::Operation(error) => Some(error.into_codec_error()),
        _ => None,
    }
}

/// Frame the active records and every archived record of a history stream.
/// Returns `None` when the stream does not agree with the states: the active
/// count is undetermined, an archived record's bytes differ from the stream, an
/// archived record does not frame to one record of its name, a revision
/// identifier repeats, or an active record's index differs from its position.
pub fn decode_record_archive(
    ctx: &DecodeContext<'_>,
    states: &[DeltaStateView<'_>],
    bytes: &[u8],
    width: RefWidth,
) -> Result<Option<RecordArchiveFrames>, CodecError> {
    let Some(start) = crate::asm_header::record_stream_start(bytes) else {
        return Ok(None);
    };
    let active_limit = crate::asm_header::solved_record_limit(bytes).unwrap_or(bytes.len());
    let mut framed = match crate::sab::frame(ctx, bytes, start, active_limit, width, None) {
        Ok(records) => records,
        Err(failure) => return propagated_error(failure).map_or(Ok(None), Err),
    };

    let insert_only = insert_only_active_record_count(states);
    let archived_count = archived_active_record_count(states)?;
    let Some(active_count) = archived_count.or(insert_only) else {
        return Ok(None);
    };
    if insert_only.is_some() && framed.len() != active_count {
        return Ok(None);
    }
    if framed.len() < active_count {
        return Ok(None);
    }
    framed.truncate(active_count);
    let mut archived_frames = HashMap::new();
    for record in states
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| record.revision_id.is_some())
    {
        let Some(revision_id) = record.revision_id else {
            return Ok(None);
        };
        let Some(offset) = usize::try_from(record.byte_offset).ok() else {
            return Ok(None);
        };
        let Some(limit) = offset.checked_add(record.raw_bytes.len()) else {
            return Ok(None);
        };
        if bytes.get(offset..limit) != Some(record.raw_bytes) {
            return Ok(None);
        }
        let mut archived = match crate::sab::frame(ctx, bytes, offset, limit, width, None) {
            Ok(records) => records,
            Err(failure) => return propagated_error(failure).map_or(Ok(None), Err),
        };
        if archived.len() != 1 {
            return Ok(None);
        }
        let Some(archived) = archived.pop() else {
            return Ok(None);
        };
        if archived.name != record.name {
            return Ok(None);
        }
        if archived_frames.insert(revision_id, archived).is_some() {
            return Ok(None);
        }
    }
    if framed
        .iter()
        .enumerate()
        .any(|(index, record)| record.index != index)
    {
        return Ok(None);
    }
    Ok(Some(RecordArchiveFrames {
        active: framed,
        archived: archived_frames,
    }))
}

#[cfg(test)]
mod tests;
