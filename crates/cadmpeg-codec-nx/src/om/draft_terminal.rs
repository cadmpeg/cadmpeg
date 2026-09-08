// SPDX-License-Identifier: Apache-2.0
//! End-anchored draft indices with positions derived from the fixed frame.

use super::compact::{ExtendedCompactIndex, LocatedCompactIndex};
use super::operation_record::OperationPayload;
use std::ops::Add;

const FIXED: [u8; 11] = [
    0x01, 0x03, 0x02, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00,
];
const BYTE_LEN: u16 = 4 + FIXED.len() as u16 + 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DraftTerminalLane<O = usize> {
    offset: O,
    indices: [ExtendedCompactIndex; 2],
    tail: [u8; 3],
}

impl<O: Copy + Add<Output = O> + From<u16>> DraftTerminalLane<O> {
    pub(crate) fn offset(&self) -> O {
        self.offset
    }
    pub(crate) fn indices(&self) -> [LocatedCompactIndex<O, ExtendedCompactIndex>; 2] {
        [
            LocatedCompactIndex {
                atom: self.indices[0],
                offset: self.offset,
            },
            LocatedCompactIndex {
                atom: self.indices[1],
                offset: self.offset + O::from(2),
            },
        ]
    }
    pub(crate) fn tail(&self) -> [u8; 3] {
        self.tail
    }
}

macro_rules! checked_origin {
    ($offset:ty) => {
        impl DraftTerminalLane<$offset> {
            pub(crate) fn new(
                indices: [ExtendedCompactIndex; 2],
                tail: [u8; 3],
                offset: $offset,
            ) -> Option<Self> {
                offset.checked_add(<$offset>::from(BYTE_LEN))?;
                Some(Self {
                    offset,
                    indices,
                    tail,
                })
            }
        }
    };
}
checked_origin!(usize);
checked_origin!(u64);

impl DraftTerminalLane<usize> {
    pub(crate) fn into_absolute(self, base: u64) -> Option<DraftTerminalLane<u64>> {
        DraftTerminalLane::<u64>::new(
            self.indices,
            self.tail,
            base.checked_add(self.offset as u64)?,
        )
    }
}

pub(crate) fn scan(record: OperationPayload<'_>) -> Option<DraftTerminalLane> {
    if record.name() != "DRAFT" {
        return None;
    }
    let start = record.payload().len().checked_sub(usize::from(BYTE_LEN))?;
    let first = ExtendedCompactIndex::read(record.payload().get(start..)?)?;
    let second = ExtendedCompactIndex::read(record.payload().get(start + 2..)?)?;
    let at = start + 4;
    (record.payload().get(at..at + FIXED.len()) == Some(&FIXED)).then_some(())?;
    let at = at + FIXED.len();
    let tail = record.payload().get(at..at + 3)?.try_into().ok()?;
    (record.payload().get(at + 3) == Some(&0x00)).then_some(())?;
    DraftTerminalLane::<usize>::new(
        [first, second],
        tail,
        record.payload_offset().checked_add(start)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_frame_requires_room_for_indices_fixed_bytes_and_tail() {
        let indices = [
            ExtendedCompactIndex::read(&[0x80, 1]).unwrap(),
            ExtendedCompactIndex::read(&[0x81, 2]).unwrap(),
        ];
        let last_origin = u64::MAX - 19;
        let lane = DraftTerminalLane::<u64>::new(indices, [1, 2, 3], last_origin).unwrap();
        assert_eq!(
            lane.indices().map(|token| token.offset),
            [last_origin, last_origin + 2]
        );
        assert_eq!(lane.tail(), [1, 2, 3]);
        assert!(DraftTerminalLane::<u64>::new(indices, [1, 2, 3], last_origin + 1).is_none());
        let relative = DraftTerminalLane::<usize>::new(indices, [1, 2, 3], 7).unwrap();
        assert_eq!(relative.clone().into_absolute(last_origin - 7), Some(lane));
        assert!(relative.into_absolute(last_origin - 6).is_none());
    }
}
