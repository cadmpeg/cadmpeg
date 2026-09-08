// SPDX-License-Identifier: Apache-2.0
//! Compact-index lanes with token positions derived from their framing.

use super::compact::{
    CompactIndexAtom, CompactIndexTarget, CountedIndexMembers, LocatedCompactIndex, PositionedIndex,
};
use std::ops::Add;

pub(crate) mod scan;

const COUNTED_PREFIX: u16 = 2;
const COUNTED_TERMINATOR: [u8; 2] = [0x01, 0x11];
const ABR_TERMINATOR: [u8; 7] = [0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CountedLane<T = (), O = usize> {
    offset: O,
    anchor: CompactIndexTarget<T>,
    members: CountedIndexMembers<CompactIndexTarget<T>>,
}

impl<T, O> CountedLane<T, O> {
    pub(crate) fn declared_count(&self) -> u8 {
        self.members.declared_count()
    }
    fn byte_len(&self) -> u16 {
        COUNTED_PREFIX
            + self.anchor.atom.raw().len() as u16
            + self
                .members
                .as_slice()
                .iter()
                .map(|index| index.atom.raw().len() as u16)
                .sum::<u16>()
            + COUNTED_TERMINATOR.len() as u16
    }
}

impl<T, O: Copy + Add<Output = O> + From<u16>> CountedLane<T, O> {
    pub(crate) fn offset(&self) -> O {
        self.offset
    }
    pub(crate) fn anchor(&self) -> PositionedIndex<'_, T, O> {
        PositionedIndex {
            atom: self.anchor.atom,
            target: &self.anchor.target,
            offset: self.offset + O::from(COUNTED_PREFIX),
        }
    }
    pub(crate) fn members(&self) -> impl Iterator<Item = PositionedIndex<'_, T, O>> {
        let mut offset = self.anchor().offset + O::from(self.anchor.atom.raw().len() as u16);
        self.members.as_slice().iter().map(move |index| {
            let position = PositionedIndex {
                atom: index.atom,
                target: &index.target,
                offset,
            };
            offset = offset + O::from(index.atom.raw().len() as u16);
            position
        })
    }
}

impl<T> CountedLane<T, usize> {
    pub(crate) fn into_absolute(self, base: u64) -> Option<CountedLane<T, u64>> {
        CountedLane::<T, u64>::new(
            self.anchor,
            self.members,
            base.checked_add(self.offset as u64)?,
        )
    }
}

impl<O> CountedLane<(), O> {
    pub(crate) fn try_resolve<T>(
        self,
        mut resolve: impl FnMut(CompactIndexAtom) -> Option<T>,
    ) -> Option<CountedLane<T, O>> {
        let anchor = CompactIndexTarget {
            atom: self.anchor.atom,
            target: resolve(self.anchor.atom)?,
        };
        let members = self.members.try_map(|index| {
            Some(CompactIndexTarget {
                atom: index.atom,
                target: resolve(index.atom)?,
            })
        })?;
        Some(CountedLane {
            offset: self.offset,
            anchor,
            members,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AbrLane<T = (), O = usize> {
    offset: O,
    slots: [Option<CompactIndexTarget<T>>; 16],
}

impl<T, O> AbrLane<T, O> {
    fn byte_len(&self) -> u16 {
        1 + self
            .slots
            .iter()
            .map(|slot| {
                slot.as_ref()
                    .map_or(1, |index| index.atom.raw().len() as u16)
            })
            .sum::<u16>()
            + ABR_TERMINATOR.len() as u16
    }
}

impl<T, O: Copy + Add<Output = O> + From<u16>> AbrLane<T, O> {
    pub(crate) fn offset(&self) -> O {
        self.offset
    }
    pub(crate) fn slots(&self) -> [LocatedCompactIndex<O, Option<&CompactIndexTarget<T>>>; 16] {
        let mut offset = self.offset + O::from(1);
        self.slots.each_ref().map(|slot| {
            let position = LocatedCompactIndex {
                atom: slot.as_ref(),
                offset,
            };
            offset = offset
                + O::from(
                    slot.as_ref()
                        .map_or(1, |index| index.atom.raw().len() as u16),
                );
            position
        })
    }
}

impl<T> AbrLane<T, usize> {
    pub(crate) fn into_absolute(self, base: u64) -> Option<AbrLane<T, u64>> {
        AbrLane::<T, u64>::new(self.slots, base.checked_add(self.offset as u64)?)
    }
}

impl<O> AbrLane<(), O> {
    pub(crate) fn try_resolve<T>(
        self,
        mut resolve: impl FnMut(CompactIndexAtom) -> Option<T>,
    ) -> Option<AbrLane<T, O>> {
        let mut slots = std::array::from_fn(|_| None);
        for (slot, source) in slots.iter_mut().zip(self.slots) {
            *slot = match source {
                Some(index) => Some(CompactIndexTarget {
                    atom: index.atom,
                    target: resolve(index.atom)?,
                }),
                None => None,
            };
        }
        Some(AbrLane {
            offset: self.offset,
            slots,
        })
    }
}

macro_rules! checked_origins {
    ($offset:ty) => {
        impl<T> CountedLane<T, $offset> {
            pub(crate) fn new(
                anchor: CompactIndexTarget<T>,
                members: CountedIndexMembers<CompactIndexTarget<T>>,
                offset: $offset,
            ) -> Option<Self> {
                let lane = Self {
                    offset,
                    anchor,
                    members,
                };
                offset.checked_add(<$offset>::from(lane.byte_len()))?;
                Some(lane)
            }
        }
        impl<T> AbrLane<T, $offset> {
            pub(crate) fn new(
                slots: [Option<CompactIndexTarget<T>>; 16],
                offset: $offset,
            ) -> Option<Self> {
                let lane = Self { offset, slots };
                offset.checked_add(<$offset>::from(lane.byte_len()))?;
                Some(lane)
            }
        }
    };
}
checked_origins!(usize);
checked_origins!(u64);

#[cfg(test)]
mod tests;
