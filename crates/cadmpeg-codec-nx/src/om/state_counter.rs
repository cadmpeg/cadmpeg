// SPDX-License-Identifier: Apache-2.0
//! Framed operation-state counter rows.

use super::discriminators::OperationStateCounterKind;
use super::state_index::StateIndexToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StateCounter<O = u64> {
    offset: O,
    kind: OperationStateCounterKind,
    object: StateIndexToken,
    introduced: u8,
    modified: u8,
}

impl<O: Copy> StateCounter<O> {
    pub(crate) fn offset(self) -> O { self.offset }
    pub(crate) fn kind(self) -> OperationStateCounterKind { self.kind }
    pub(crate) fn object(self) -> StateIndexToken { self.object }
    pub(crate) fn introduced(self) -> u8 { self.introduced }
    pub(crate) fn modified(self) -> u8 { self.modified }
    pub(crate) fn byte_len(self) -> usize { 5 + self.object.raw().len() }
}

impl StateCounter<usize> {
    pub(crate) fn read(bytes: &[u8], at: usize, base: usize) -> Option<Self> {
        let tail = bytes.get(at..)?;
        if tail.first() != Some(&0x05) { return None; }
        let kind = OperationStateCounterKind::try_from(*tail.get(1)?).ok()?;
        let object = StateIndexToken::read_at(tail, 2)?;
        let state_at = 2 + object.raw().len();
        let introduced = *tail.get(state_at)?;
        let modified = *tail.get(state_at + 1)?;
        if tail.get(state_at + 2) != Some(&0x4e) { return None; }
        let offset = base.checked_add(at)?;
        offset.checked_add(state_at + 3)?;
        Some(Self { offset, kind, object, introduced, modified })
    }

    pub(crate) fn into_absolute(self, base: u64) -> Option<StateCounter> {
        StateCounter::new(base.checked_add(u64::try_from(self.offset).ok()?)?,
            self.kind, self.object, self.introduced, self.modified).ok()
    }
}

impl StateCounter {
    pub(crate) fn new(offset: u64, kind: OperationStateCounterKind,
        object: StateIndexToken, introduced: u8, modified: u8) -> Result<Self, &'static str> {
        offset.checked_add(5 + object.raw().len() as u64)
            .ok_or("source_offset: counter row extent overflows")?;
        Ok(Self { offset, kind, object, introduced, modified })
    }

    pub(crate) fn object_offset(self) -> u64 { self.offset + 2 }
}
