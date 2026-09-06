// SPDX-License-Identifier: Apache-2.0
//! Operation-state slot lanes with token-derived extents.

use super::state_index::{OperationStateIndex, StateIndexToken};
use super::state_slots::StateSlots;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StateSlotLane<O = usize> {
    offset: O,
    slots: StateSlots<Option<StateIndexToken>>,
}

impl<O: Copy + From<u8> + std::ops::Add<Output = O>> StateSlotLane<O> {
    pub(crate) fn offset(&self) -> O {
        self.offset
    }
    #[cfg(test)]
    pub(crate) fn slots(&self) -> &StateSlots<Option<StateIndexToken>> {
        &self.slots
    }
    pub(crate) fn into_slots(self) -> StateSlots<Option<StateIndexToken>> {
        self.slots
    }
    pub(crate) fn end_offset(&self) -> O {
        self.slots
            .iter()
            .fold(self.offset + O::from(5), |end, (_, slot)| {
                end + O::from(slot.map_or(1, StateIndexToken::byte_len))
            })
    }
}

impl StateSlotLane {
    pub(crate) fn read(bytes: &[u8], at: usize, end: usize, base: usize) -> Option<Self> {
        if bytes.get(at..at.checked_add(3)?) != Some(&[0x02, 0x01, 0x11]) {
            return None;
        }
        let mut slots = Vec::new();
        let mut cursor = at + 3;
        while cursor < end {
            if bytes.get(cursor..cursor.checked_add(2)?) == Some(&[0x02, 0x11]) {
                base.checked_add(cursor + 2)?;
                return Some(Self {
                    offset: base.checked_add(at)?,
                    slots: StateSlots::new(slots).ok()?,
                });
            }
            let slot = OperationStateIndex::read_at(bytes, cursor, base)?;
            cursor = cursor.checked_add(slot.raw().len())?;
            slots.push(slot.token());
        }
        None
    }

    pub(crate) fn end_at(bytes: &[u8], at: usize, end: usize) -> Option<usize> {
        if bytes.get(at..at.checked_add(3)?) != Some(&[0x02, 0x01, 0x11]) {
            return None;
        }
        let mut cursor = at + 3;
        while cursor < end {
            if bytes.get(cursor..cursor.checked_add(2)?) == Some(&[0x02, 0x11]) {
                return cursor.checked_add(2);
            }
            let slot = OperationStateIndex::read_at(bytes, cursor, 0)?;
            cursor = cursor.checked_add(slot.raw().len())?;
        }
        None
    }

    pub(crate) fn into_absolute(self, base: u64) -> Option<StateSlotLane<u64>> {
        StateSlotLane::new(
            base.checked_add(u64::try_from(self.offset).ok()?)?,
            self.slots,
        )
        .ok()
    }
}

impl StateSlotLane<u64> {
    pub(crate) fn new(
        offset: u64,
        slots: StateSlots<Option<StateIndexToken>>,
    ) -> Result<Self, &'static str> {
        let byte_len = slots.iter().fold(5u64, |len, (_, slot)| {
            len + u64::from(slot.map_or(1, StateIndexToken::byte_len))
        });
        offset
            .checked_add(byte_len)
            .ok_or("source_offset: slot-lane extent overflows")?;
        Ok(Self { offset, slots })
    }
}

#[cfg(test)]
mod tests {
    use super::StateSlotLane;

    #[test]
    fn source_and_native_extents_follow_null_and_variable_width_slots() {
        let bytes = [2, 1, 0x11, 0xff, 1, 0x80, 1, 0x90, 0, 1, 2, 0x11];
        let lane = StateSlotLane::read(&bytes, 0, bytes.len(), 100).unwrap();
        assert_eq!(lane.slots().len(), 4);
        assert_eq!(lane.offset(), 100);
        assert_eq!(lane.end_offset(), 112);
        assert_eq!(StateSlotLane::end_at(&bytes, 0, bytes.len()), Some(12));
        let native = lane.clone().into_absolute(200).unwrap();
        assert_eq!((native.offset(), native.end_offset()), (300, 312));
        assert!(lane.clone().into_absolute(u64::MAX - 112).is_some());
        assert!(lane.into_absolute(u64::MAX - 111).is_none());
        assert!(StateSlotLane::read(&bytes, 0, bytes.len(), usize::MAX - 11).is_none());
    }
}
