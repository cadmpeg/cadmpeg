// SPDX-License-Identifier: Apache-2.0
//! State-block path selection and ordered status/message phases.

use super::nonempty::NonEmpty;
use super::state_message::{OperationStateMessage, StateMessage};
use super::state_slot_lane::StateSlotLane;
use super::state_status::{operation_state_opaque_lane_end_at, operation_state_status_row_at};
use super::state_table::{OperationStateStatusTable, StateTableEntry};

pub(super) struct OperationStateBlock<'a> {
    offset: usize,
    body: BlockBody<'a>,
}

enum BlockBody<'a> {
    Statuses {
        entries: NonEmpty<StateTableEntry<'a>>,
        messages: Vec<StateMessage<&'a str>>,
    },
    Messages(NonEmpty<StateMessage<&'a str>>),
}

impl<'a> OperationStateBlock<'a> {
    fn new(
        offset: usize,
        entries: Vec<StateTableEntry<'a>>,
        messages: Vec<StateMessage<&'a str>>,
    ) -> Option<Self> {
        let after_status = entries
            .iter()
            .try_fold(offset, |end, entry| end.checked_add(entry.byte_len()))?;
        messages.iter().try_fold(after_status, |end, message| {
            end.checked_add(message.byte_len())
        })?;
        let body = match NonEmpty::new(entries) {
            Some(entries) => BlockBody::Statuses { entries, messages },
            None => BlockBody::Messages(NonEmpty::new(messages)?),
        };
        Some(Self { offset, body })
    }
    pub(super) fn into_status_table(self) -> Option<OperationStateStatusTable<'a>> {
        match self.body {
            BlockBody::Statuses { entries, .. } => {
                OperationStateStatusTable::new(self.offset, entries)
            }
            BlockBody::Messages(_) => None,
        }
    }
    pub(super) fn status_end_offset(&self) -> usize {
        match &self.body {
            BlockBody::Statuses { entries, .. } => entries
                .iter()
                .fold(self.offset, |end, entry| end + entry.byte_len()),
            BlockBody::Messages(_) => self.offset,
        }
    }
    pub(super) fn into_messages(self) -> Option<Vec<OperationStateMessage<'a>>> {
        let offset = self.status_end_offset();
        match self.body {
            BlockBody::Statuses { messages, .. } => locate_messages(offset, messages),
            BlockBody::Messages(messages) => locate_messages(offset, messages),
        }
    }
    #[cfg(test)]
    pub(super) fn offset(&self) -> usize {
        self.offset
    }
    #[cfg(test)]
    pub(super) fn rows(&self) -> Vec<&super::state_status::StateStatus<&'a str, &'a [u8]>> {
        match &self.body {
            BlockBody::Statuses { entries, .. } => entries
                .iter()
                .filter_map(|entry| match entry {
                    StateTableEntry::Status(row) => Some(row),
                    StateTableEntry::Slots(_) => None,
                })
                .collect(),
            BlockBody::Messages(_) => Vec::new(),
        }
    }
    #[cfg(test)]
    pub(super) fn messages(&self) -> Vec<&StateMessage<&'a str>> {
        match &self.body {
            BlockBody::Statuses { messages, .. } => messages.iter().collect(),
            BlockBody::Messages(messages) => messages.iter().collect(),
        }
    }
}

fn locate_messages<'a>(
    mut offset: usize,
    messages: impl IntoIterator<Item = StateMessage<&'a str>>,
) -> Option<Vec<OperationStateMessage<'a>>> {
    messages
        .into_iter()
        .map(|body| {
            let message = OperationStateMessage::new(offset, body)?;
            offset = message.end_offset();
            Some(message)
        })
        .collect()
}

fn operation_state_status_end_at(
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
    opaque_lane_starts: Option<&[usize]>,
) -> Option<usize> {
    if bytes.get(at..at + 3) == Some(&[0x02, 0x01, 0x11]) {
        let precomputed_end = opaque_lane_starts
            .and_then(|starts| operation_state_opaque_lane_end_at(starts, at, end));
        precomputed_end.or_else(|| StateSlotLane::end_at(bytes, at, end))
    } else {
        operation_state_status_row_at(bytes, at, end, base_offset, opaque_lane_starts)
            .map(|row| row.end_offset() - base_offset)
    }
}

#[derive(Clone, Copy)]
struct OperationStatePath {
    length: usize,
    end: usize,
}

fn operation_state_path_at(
    paths: &[(usize, OperationStatePath)],
    at: usize,
) -> Option<OperationStatePath> {
    paths
        .binary_search_by(|(offset, _)| offset.cmp(&at).reverse())
        .ok()
        .map(|index| paths[index].1)
}

pub(super) fn operation_state_block_before_boundary(
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Option<OperationStateBlock<'_>> {
    const MAX_STATE_BLOCK_TAIL_BYTES: usize = 64 * 1024;

    if start >= end || end > bytes.len() {
        return None;
    }

    let mut opaque_lane_starts = Vec::new();
    for at in start..end.saturating_sub(1) {
        if bytes.get(at..at + 2) == Some(&[0x02, 0x11]) {
            opaque_lane_starts.try_reserve(1).ok()?;
            opaque_lane_starts.push(at);
        }
    }

    let mut status_paths = Vec::new();
    let mut message_paths = Vec::new();
    for at in (start..end).rev() {
        if let Some(message) = OperationStateMessage::read(bytes, at, base_offset) {
            let next = message.end_offset() - base_offset;
            if next > at && next <= end {
                let continuation = (next < end)
                    .then(|| operation_state_path_at(&message_paths, next))
                    .flatten();
                let length = continuation.map_or(Some(1), |path| path.length.checked_add(1))?;
                let path_end = continuation.map_or(next, |path| path.end);
                message_paths.try_reserve(1).ok()?;
                message_paths.push((
                    at,
                    OperationStatePath {
                        length,
                        end: path_end,
                    },
                ));
            }
        }

        let (status_length, status_end) =
            operation_state_status_end_at(bytes, at, end, base_offset, Some(&opaque_lane_starts))
                .filter(|next| *next > at && *next <= end)
                .map_or((0, usize::MAX), |next| {
                    let continuation = (next < end)
                        .then(|| operation_state_path_at(&status_paths, next))
                        .flatten();
                    let Some(length) =
                        continuation.map_or(Some(1), |path| path.length.checked_add(1))
                    else {
                        return (0, usize::MAX);
                    };
                    let path_end = continuation.map_or(next, |path| path.end);
                    (length, path_end)
                });
        let message_path = operation_state_path_at(&message_paths, at);
        let best_path =
            if status_length >= message_path.map_or(0, |path| path.length) && status_length > 0 {
                Some(OperationStatePath {
                    length: status_length,
                    end: status_end,
                })
            } else {
                message_path
            };
        if let Some(path) = best_path {
            status_paths.try_reserve(1).ok()?;
            status_paths.push((at, path));
        }
    }

    let has_exact_boundary_path = status_paths.iter().any(|(_, path)| path.end == end);
    let (offset, path) = status_paths
        .iter()
        .filter(|(at, path)| {
            if has_exact_boundary_path {
                path.end == end
            } else {
                path.end >= *at && end.saturating_sub(path.end) <= MAX_STATE_BLOCK_TAIL_BYTES
            }
        })
        .max_by_key(|(at, path)| (path.length, std::cmp::Reverse(*at)))
        .map(|(at, path)| (*at, *path))?;
    let path_end = path.end;
    let mut entries = Vec::new();
    let mut messages = Vec::new();
    let mut at = offset;
    while at < path_end {
        let status_next =
            operation_state_status_end_at(bytes, at, end, base_offset, Some(&opaque_lane_starts));
        let status_length = status_next
            .filter(|next| {
                *next > at
                    && *next <= path_end
                    && (*next == path_end
                        || (*next < end
                            && operation_state_path_at(&status_paths, *next)
                                .is_some_and(|path| path.end == path_end)))
            })
            .map_or(0, |next| {
                if next == path_end {
                    1
                } else {
                    operation_state_path_at(&status_paths, next)
                        .and_then(|path| path.length.checked_add(1))
                        .unwrap_or(0)
                }
            });
        let message = OperationStateMessage::read(bytes, at, base_offset);
        let message_next = message
            .as_ref()
            .map(|message| message.end_offset() - base_offset);
        let message_length = operation_state_path_at(&message_paths, at)
            .filter(|path| path.end == path_end)
            .map_or(0, |path| path.length);

        if status_length >= message_length && status_length > 0 {
            let next = status_next?;
            if bytes.get(at..at + 3) == Some(&[0x02, 0x01, 0x11]) {
                let lane = StateSlotLane::read(bytes, at, end, base_offset)?;
                let lane_end = lane.end_offset() - base_offset;
                (lane_end == next).then_some(())?;
                entries.push(StateTableEntry::Slots(lane.into_slots()));
                at = next;
            } else {
                let row = operation_state_status_row_at(
                    bytes,
                    at,
                    end,
                    base_offset,
                    Some(&opaque_lane_starts),
                )?;
                let row_end = row.end_offset() - base_offset;
                (row_end == next).then_some(())?;
                entries.push(StateTableEntry::Status(row.body()));
                at = next;
            }
        } else {
            let message = message?;
            let next = message_next?;
            (next > at && next <= path_end && message_length > 0).then_some(())?;
            messages.push(message.body());
            at = next;
            break;
        }
    }
    while at < path_end {
        let message = OperationStateMessage::read(bytes, at, base_offset)?;
        let next = message.end_offset() - base_offset;
        (next > at && next <= path_end).then_some(())?;
        messages.push(message.body());
        at = next;
    }
    OperationStateBlock::new(base_offset.checked_add(offset)?, entries, messages)
}

#[cfg(test)]
mod tests {
    use super::{OperationStateBlock, OperationStateMessage, StateSlotLane, StateTableEntry};
    use crate::om::state_status::operation_state_status_row_at;

    #[test]
    fn table_entries_and_messages_follow_one_block_origin() {
        let row = operation_state_status_row_at(&[0x41, 1, 0x3f], 0, 3, 0, None)
            .unwrap()
            .body();
        let slots = StateSlotLane::read(&[2, 1, 0x11, 0xff, 2, 0x11], 0, 6, 0)
            .unwrap()
            .into_slots();
        let message =
            OperationStateMessage::read(&[3, 3, b'A', 0, 0, 0, 0, 0, 0xa0, 0, 0, 0, 0], 0, 0)
                .unwrap()
                .body();
        let entries = vec![
            StateTableEntry::Status(row),
            StateTableEntry::Slots(slots),
            StateTableEntry::Status(row),
        ];
        let block = OperationStateBlock::new(100, entries.clone(), vec![message]).unwrap();
        assert_eq!(block.status_end_offset(), 112);
        let table = block.into_status_table().unwrap();
        let positioned: Vec<_> = table.into_entries().collect();
        assert_eq!(
            positioned
                .iter()
                .map(|(offset, _)| *offset)
                .collect::<Vec<_>>(),
            [100, 103, 109]
        );
        assert!(matches!(positioned[1].1, StateTableEntry::Slots(_)));
        let messages = OperationStateBlock::new(100, entries, vec![message])
            .unwrap()
            .into_messages()
            .unwrap();
        assert_eq!((messages[0].offset(), messages[0].end_offset()), (112, 125));
    }

    #[test]
    fn message_only_blocks_are_nonempty_and_bound_their_derived_end() {
        let message =
            OperationStateMessage::read(&[3, 3, b'A', 0, 0, 0, 0, 0, 0xa0, 0, 0, 0, 0], 0, 0)
                .unwrap()
                .body();
        assert!(OperationStateBlock::new(100, Vec::new(), Vec::new()).is_none());
        let block = OperationStateBlock::new(100, Vec::new(), vec![message]).unwrap();
        assert_eq!(block.status_end_offset(), 100);
        assert!(block.into_status_table().is_none());
        let block = OperationStateBlock::new(usize::MAX - 13, Vec::new(), vec![message]).unwrap();
        assert_eq!(block.into_messages().unwrap()[0].end_offset(), usize::MAX);
        assert!(OperationStateBlock::new(usize::MAX - 12, Vec::new(), vec![message]).is_none());
    }
}
