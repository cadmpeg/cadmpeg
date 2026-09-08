// SPDX-License-Identifier: Apache-2.0
//! Ordered status-table bodies with positions derived from one origin.

use super::nonempty::NonEmpty;
use super::state_index::StateIndexToken;
use super::state_slots::StateSlots;
use super::state_status::StateStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StateTableEntry<'a> {
    Status(StateStatus<&'a str, &'a [u8]>),
    Slots(StateSlots<Option<StateIndexToken>>),
}

impl StateTableEntry<'_> {
    pub(super) fn byte_len(&self) -> usize {
        match self {
            Self::Status(body) => body.byte_len(),
            Self::Slots(slots) => slots.iter().fold(5, |len, (_, slot)| {
                len + usize::from(slot.map_or(1, StateIndexToken::byte_len))
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperationStateStatusTable<'a> {
    offset: usize,
    entries: NonEmpty<StateTableEntry<'a>>,
}

impl<'a> OperationStateStatusTable<'a> {
    pub(super) fn new(offset: usize, entries: NonEmpty<StateTableEntry<'a>>) -> Option<Self> {
        entries
            .iter()
            .try_fold(offset, |end, entry| end.checked_add(entry.byte_len()))?;
        Some(Self { offset, entries })
    }

    pub(crate) fn into_entries(self) -> impl Iterator<Item = (usize, StateTableEntry<'a>)> {
        let mut offset = self.offset;
        self.entries.into_iter().map(move |entry| {
            let start = offset;
            offset += entry.byte_len();
            (start, entry)
        })
    }

    #[cfg(test)]
    pub(super) fn end_offset(&self) -> usize {
        self.entries
            .iter()
            .fold(self.offset, |end, entry| end + entry.byte_len())
    }
    #[cfg(test)]
    pub(super) fn rows(&self) -> Vec<&StateStatus<&'a str, &'a [u8]>> {
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                StateTableEntry::Status(row) => Some(row),
                StateTableEntry::Slots(_) => None,
            })
            .collect()
    }
    #[cfg(test)]
    pub(super) fn slot_lanes(&self) -> Vec<&StateSlots<Option<StateIndexToken>>> {
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                StateTableEntry::Status(_) => None,
                StateTableEntry::Slots(slots) => Some(slots),
            })
            .collect()
    }
}

#[cfg(test)]
pub(super) fn operation_state_status_table(
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Option<OperationStateStatusTable<'_>> {
    use super::state_message::OperationStateMessage;
    use super::state_slot_lane::StateSlotLane;
    use super::state_status::operation_state_status_row_at;

    if start >= end || end > bytes.len() {
        return None;
    }
    let mut entries = Vec::new();
    let mut at = start;
    while at < end {
        if OperationStateMessage::read(bytes, at, base_offset).is_some() {
            break;
        }
        if bytes.get(at..at + 3) == Some(&[0x02, 0x01, 0x11]) {
            let lane = StateSlotLane::read(bytes, at, end, base_offset)?;
            at = lane.end_offset() - base_offset;
            entries.push(StateTableEntry::Slots(lane.into_slots()));
            continue;
        }
        let Some(row) = operation_state_status_row_at(bytes, at, end, base_offset, None) else {
            break;
        };
        at = row.end_offset() - base_offset;
        entries.push(StateTableEntry::Status(row.body()));
    }
    if !entries
        .iter()
        .any(|entry| matches!(entry, StateTableEntry::Status(_)))
    {
        return None;
    }
    OperationStateStatusTable::new(base_offset.checked_add(start)?, NonEmpty::new(entries)?)
}
