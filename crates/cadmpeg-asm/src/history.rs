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
    /// State identifier of the `delta_state` header.
    pub state_id: i64,
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

/// Re-key the framed records of a history stream by revision identifier, with
/// each record's index and each non-negative reference moved from revision
/// identifiers to entity slots. Returns `None` when a revision identifier
/// repeats, a revision names no entity slot, or a reference names a revision
/// that no change places.
pub fn historical_record_archive(
    states: &[DeltaStateView<'_>],
    frames: RecordArchiveFrames,
) -> Option<HashMap<i64, Record>> {
    let active_count = i64::try_from(frames.active.len()).ok()?;
    let mut revision_entities = HashMap::new();
    for entity_ref in 0..active_count {
        revision_entities.insert(entity_ref, entity_ref);
    }
    for change in states.iter().flat_map(|state| &state.changes) {
        let Some(old_ref) = change.old_ref() else {
            continue;
        };
        let entity_ref = change.new_ref().unwrap_or(old_ref);
        if revision_entities.insert(old_ref, entity_ref).is_some() {
            return None;
        }
    }
    let mut records = HashMap::new();
    for (revision, record) in frames.active.into_iter().enumerate() {
        records.insert(i64::try_from(revision).ok()?, record);
    }
    for (revision_id, framed) in frames.archived {
        if records.insert(revision_id, framed).is_some() {
            return None;
        }
    }
    if records.len() != revision_entities.len() {
        return None;
    }
    for (&revision_ref, record) in &mut records {
        let &entity_ref = revision_entities.get(&revision_ref)?;
        record.index = usize::try_from(entity_ref).ok()?;
        for token in std::sync::Arc::make_mut(&mut record.tokens) {
            let Token::Ref(reference) = token else {
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

/// Marker that opens a `delta_state` record: terminator, identifier tag, and the
/// 11-byte class name.
const DELTA_STATE_MARKER: &[u8] = b"\x11\x0d\x0bdelta_state";

/// Byte offsets of every `delta_state` marker of a history stream, in stream order.
pub fn delta_state_offsets(bytes: &[u8]) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut search = 0usize;
    while let Some(relative) = bytes[search..]
        .windows(DELTA_STATE_MARKER.len())
        .position(|window| window == DELTA_STATE_MARKER)
    {
        let offset = search + relative;
        offsets.push(offset);
        search = offset + DELTA_STATE_MARKER.len();
    }
    offsets
}

/// Fixed integer header of one `delta_state` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeltaStateHeader {
    /// Byte offset of the record marker.
    pub offset: usize,
    /// State identifier.
    pub state_id: i64,
    /// Version flag.
    pub version_flag: i64,
    /// State flag.
    pub state_flag: i64,
    /// Node index of the previous state; negative for the head.
    pub previous: i64,
    /// Node index of the next state; negative for the last state.
    pub next: i64,
    /// Node index of the state.
    pub node_index: i64,
    /// Partner reference.
    pub partner: i64,
    /// Owner reference.
    pub owner_ref: i64,
    /// First byte after the `0x0b` marker that opens the state body.
    pub body_start: usize,
}

/// Result of reading the header of a `delta_state` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaStateHeaderRead {
    /// The header is complete and the state body opens with `0x0b`.
    Framed(DeltaStateHeader),
    /// The header is complete and no state body follows it.
    Unframed,
    /// A header integer is missing or mistagged.
    Invalid,
}

/// Read the header of the `delta_state` record whose marker is at `offset`.
pub fn read_delta_state_header(
    bytes: &[u8],
    offset: usize,
    width: RefWidth,
) -> DeltaStateHeaderRead {
    let mut position = offset + DELTA_STATE_MARKER.len();
    let Some([state_id, version_flag, state_flag, previous, next, node_index, partner, owner_ref]) =
        (|| {
            Some([
                take_int(bytes, &mut position, 0x04, width)?,
                take_int(bytes, &mut position, 0x04, width)?,
                take_int(bytes, &mut position, 0x04, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
            ])
        })()
    else {
        return DeltaStateHeaderRead::Invalid;
    };
    if bytes.get(position) != Some(&0x0b) {
        return DeltaStateHeaderRead::Unframed;
    }
    DeltaStateHeaderRead::Framed(DeltaStateHeader {
        offset,
        state_id,
        version_flag,
        state_flag,
        previous,
        next,
        node_index,
        partner,
        owner_ref,
        body_start: position + 1,
    })
}

/// Records framed between the end of one state's bulletin boards and the next
/// `delta_state` marker.
#[derive(Debug)]
pub enum StateRecords {
    /// The span holds no record.
    Empty,
    /// The span frames to these records.
    Framed(Vec<Record>),
    /// The span does not frame; `start..limit` is the span.
    Unframed {
        /// First byte of the span.
        start: usize,
        /// End of the span.
        limit: usize,
        /// Framing diagnostic.
        error: String,
    },
}

/// Frame the records of the state whose bulletin boards end at `state_end`;
/// `next_delta` is the marker offset of the next state. A resource or
/// operation failure is an error; any other framing failure is
/// [`StateRecords::Unframed`].
pub fn frame_state_records(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    state_end: usize,
    next_delta: Option<usize>,
    width: RefWidth,
) -> Result<StateRecords, CodecError> {
    let mut start = state_end + usize::from(bytes.get(state_end) == Some(&0x11));
    if bytes.get(start) == Some(&0x04)
        && int_at(bytes, start + 1, width) == Some(0)
        && bytes.get(start + 1 + width.bytes()) == Some(&0x11)
    {
        start += 2 + width.bytes();
    }
    let limit = next_delta.map_or(bytes.len(), |offset| offset + 1);
    if start >= limit {
        return Ok(StateRecords::Empty);
    }
    match crate::sab::frame_history(ctx, bytes, start, limit, width, None) {
        Ok(records) => Ok(StateRecords::Framed(records)),
        Err(StreamFailure::Resource(error)) => Err(error.into()),
        Err(StreamFailure::Operation(error)) => Err(error.into_codec_error()),
        Err(error) => Ok(StateRecords::Unframed {
            start,
            limit,
            error: format!("{error}"),
        }),
    }
}

fn int_at(bytes: &[u8], offset: usize, width: RefWidth) -> Option<i64> {
    match width {
        RefWidth::Four => Some(i64::from(View::i32_le_at(bytes, offset)?)),
        RefWidth::Eight => View::i64_le_at(bytes, offset),
    }
}

/// Revision identifiers of the snapshot records of a history. The old
/// references of all changes, sorted, form a run `first..first + n`; the run
/// is the identifier list when `snapshot_record_count` equals `n`, and the
/// i-th snapshot record in stream order takes the i-th identifier.
pub fn snapshot_revision_ids(
    changes: impl Iterator<Item = EntityChange>,
    snapshot_record_count: usize,
) -> Option<std::ops::Range<i64>> {
    let mut old_references: Vec<i64> = changes.filter_map(EntityChange::old_ref).collect();
    old_references.sort_unstable_by(Ord::cmp);
    let &first = old_references.first()?;
    let count = i64::try_from(old_references.len()).ok()?;
    let end = first.checked_add(count)?;
    if old_references.iter().copied().ne(first..end)
        || snapshot_record_count != old_references.len()
    {
        return None;
    }
    Some(first..end)
}

/// One record framed in a `delta_state` span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainRecord {
    /// Revision identifier of an archived record; `None` for a boundary record.
    pub revision_id: Option<i64>,
    /// Byte offset of the record in the stream.
    pub offset: usize,
    /// Stored length of the record in bytes.
    pub len: usize,
    /// Record name.
    pub name: String,
}

/// One framed `delta_state`: its header, bulletin-board changes in stream
/// order, and records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainState {
    /// Header of the state.
    pub header: DeltaStateHeader,
    /// Changes of all boards of the state, in stream order.
    pub changes: Vec<EntityChange>,
    /// Records of the state, in stream order.
    pub records: Vec<ChainRecord>,
}

/// The framed `delta_state` chain of a history stream, in stream order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryChain {
    /// Framed states.
    pub states: Vec<ChainState>,
}

/// Outcome of framing the `delta_state` chain of a stream.
#[derive(Debug)]
pub enum ChainFraming {
    /// The stream holds no `delta_state` record.
    Absent,
    /// A header, bulletin-board chain, or record span does not frame.
    Unframed,
    /// Every state frames.
    Framed(HistoryChain),
}

impl HistoryChain {
    /// Views of the states for the shared history walks; record bytes borrow
    /// from `bytes`, the stream the chain was framed from.
    #[must_use]
    pub fn views<'a>(&'a self, bytes: &'a [u8]) -> Vec<DeltaStateView<'a>> {
        self.states
            .iter()
            .map(|state| DeltaStateView {
                state_id: state.header.state_id,
                node_index: state.header.node_index,
                previous_ref: (state.header.previous >= 0).then_some(state.header.previous),
                next_ref: (state.header.next >= 0).then_some(state.header.next),
                changes: state.changes.clone(),
                records: state
                    .records
                    .iter()
                    .map(|record| StateRecordView {
                        revision_id: record.revision_id,
                        byte_offset: cadmpeg_core::decode::u64_from_index(record.offset),
                        raw_bytes: record
                            .offset
                            .checked_add(record.len)
                            .and_then(|end| bytes.get(record.offset..end))
                            .unwrap_or_default(),
                        name: record.name.as_str(),
                    })
                    .collect(),
            })
            .collect()
    }
}

/// Frame the `delta_state` chain of a history stream: every state header, its
/// bulletin boards, and the records framed up to the next state, with the
/// snapshot revision identifiers bound. A state whose header opens no body is
/// skipped. A resource or operation failure is an error.
pub fn frame_history_chain(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    width: RefWidth,
) -> Result<ChainFraming, CodecError> {
    let offsets = delta_state_offsets(bytes);
    if offsets.is_empty() {
        return Ok(ChainFraming::Absent);
    }
    let mut states = Vec::new();
    for (ordinal, &offset) in offsets.iter().enumerate() {
        let header = match read_delta_state_header(bytes, offset, width) {
            DeltaStateHeaderRead::Framed(header) => header,
            DeltaStateHeaderRead::Unframed => continue,
            DeltaStateHeaderRead::Invalid => return Ok(ChainFraming::Unframed),
        };
        let mut changes = Vec::new();
        let Some(body_end) = walk_bulletin_boards(bytes, header.body_start, width, |item| {
            if let BulletinBoardItem::Change { kind, .. } = item {
                changes.push(kind);
            }
        }) else {
            return Ok(ChainFraming::Unframed);
        };
        let framed = match frame_state_records(
            ctx,
            bytes,
            body_end,
            offsets.get(ordinal + 1).copied(),
            width,
        )? {
            StateRecords::Empty => Vec::new(),
            StateRecords::Framed(records) => records,
            StateRecords::Unframed { .. } => return Ok(ChainFraming::Unframed),
        };
        states.push(ChainState {
            header,
            changes,
            records: framed
                .into_iter()
                .map(|record| ChainRecord {
                    revision_id: None,
                    offset: record.offset,
                    len: record.len,
                    name: record.name,
                })
                .collect(),
        });
    }
    if states.is_empty() {
        return Ok(ChainFraming::Absent);
    }
    let snapshot_count = states
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| record.name != ACTIVE_DATA_END)
        .count();
    let changes = states
        .iter()
        .flat_map(|state| state.changes.iter().copied());
    if let Some(ids) = snapshot_revision_ids(changes, snapshot_count) {
        for (record, id) in states
            .iter_mut()
            .flat_map(|state| &mut state.records)
            .filter(|record| record.name != ACTIVE_DATA_END)
            .zip(ids)
        {
            record.revision_id = Some(id);
        }
    }
    Ok(ChainFraming::Framed(HistoryChain { states }))
}

#[cfg(test)]
mod tests;
