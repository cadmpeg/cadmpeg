// SPDX-License-Identifier: Apache-2.0
//! Compact-index column rows with positions derived from their wire layout.

use std::ops::Add;
use super::compact::{CompactIndexAtom, LocatedCompactIndex};
use super::discriminators::{IndexRowMode, LinkedIndexDiscriminator, LinkedIndexFlag};

pub(crate) mod scan;

const INDEX_PREFIX: [u8; 3] = [0x2d, 0x02, 0x0b];
const INDEX_MIDDLE: [u8; 2] = [0x93, 0x8a];
const INDEX_SUFFIX: [u8; 9] = [0x00, 0x47, 0x04, 0x04, 0x01, 0xc0, 0x44, 0x04, 0x00];
const LINKED_PREFIX: [u8; 2] = [0x02, 0x0b];
const LINKED_MIDDLE: [u8; 2] = [0x93, 0x8c];
const TARGET_PREFIX: [u8; 5] = [0x02, 0x01, 0x01, 0x01, 0x16];
const TARGET_MIDDLE: [u8; 4] = [0xff, 0xff, 0x90, 0xfe];
pub(crate) const ROW_SUFFIX: [u8; 5] = [0x01, 0xc0, 0x44, 0x04, 0x00];

/// One source index paired with its resolved target, or `()` before resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RowIndex<T> {
    pub(crate) atom: CompactIndexAtom,
    pub(crate) target: T,
}

impl From<CompactIndexAtom> for RowIndex<()> {
    fn from(atom: CompactIndexAtom) -> Self { Self { atom, target: () } }
}

/// Read-only projection of one row index and its derived source position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PositionedIndex<'a, T, O> {
    pub(crate) atom: CompactIndexAtom,
    pub(crate) target: &'a T,
    pub(crate) offset: O,
}

fn width(atom: CompactIndexAtom) -> u8 { atom.raw().len() as u8 }

fn positions<T, O: Copy + Add<Output = O> + From<u8>, const N: usize>(
    indices: &[RowIndex<T>; N], mut offset: O,
) -> [PositionedIndex<'_, T, O>; N] {
    std::array::from_fn(|i| {
        let index = &indices[i];
        let position = PositionedIndex { atom: index.atom, target: &index.target, offset };
        offset = offset + O::from(width(index.atom));
        position
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IndexRow<T = (), O = usize> {
    offset: O,
    first_index: CompactIndexAtom,
    flag: LinkedIndexFlag,
    indices: [RowIndex<T>; 4],
}

impl<T, O> IndexRow<T, O> {
    fn indices_start(&self) -> u8 { INDEX_PREFIX.len() as u8 + width(self.first_index) + INDEX_MIDDLE.len() as u8 + 1 }
    pub(crate) fn byte_len(&self) -> u8 { self.indices_start() + self.indices.iter().map(|index| width(index.atom)).sum::<u8>() + INDEX_SUFFIX.len() as u8 }
    pub(crate) fn flag(&self) -> LinkedIndexFlag { self.flag }
}

impl<T, O: Copy + Add<Output = O> + From<u8>> IndexRow<T, O> {
    pub(crate) fn offset(&self) -> O { self.offset }
    pub(crate) fn first_index(&self) -> LocatedCompactIndex<O> { LocatedCompactIndex { atom: self.first_index, offset: self.offset + O::from(INDEX_PREFIX.len() as u8) } }
    pub(crate) fn indices(&self) -> [PositionedIndex<'_, T, O>; 4] { positions(&self.indices, self.offset + O::from(self.indices_start())) }
}

impl<T> IndexRow<T, usize> {
    pub(crate) fn into_absolute(self, base: u64) -> Option<IndexRow<T, u64>> {
        let offset = base.checked_add(self.offset as u64)?;
        IndexRow::<T, u64>::new(self.first_index, self.flag, self.indices, offset)
    }
}

impl<O> IndexRow<(), O> {
    pub(crate) fn try_resolve<U>(self, mut resolve: impl FnMut(CompactIndexAtom) -> Option<U>) -> Option<IndexRow<U, O>> {
        let [a, b, c, d] = self.indices.map(|index| Some(RowIndex { atom: index.atom, target: resolve(index.atom)? }));
        Some(IndexRow { offset: self.offset, indices: [a?, b?, c?, d?], first_index: self.first_index, flag: self.flag })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LinkedRow<T = (), O = usize> {
    offset: O,
    first_index: CompactIndexAtom,
    discriminator: LinkedIndexDiscriminator,
    target_index: RowIndex<T>,
    indices: [RowIndex<T>; 3],
    flag: LinkedIndexFlag,
    mode: IndexRowMode,
}

impl<T, O> LinkedRow<T, O> {
    fn indices_start(&self) -> u8 { LINKED_PREFIX.len() as u8 + width(self.first_index) + LINKED_MIDDLE.len() as u8 + 1 + width(self.target_index.atom) + TARGET_MIDDLE.len() as u8 }
    pub(crate) fn byte_len(&self) -> u8 { self.indices_start() + self.indices.iter().map(|index| width(index.atom)).sum::<u8>() + 4 + ROW_SUFFIX.len() as u8 }
    pub(crate) fn discriminator(&self) -> LinkedIndexDiscriminator { self.discriminator }
    pub(crate) fn flag(&self) -> LinkedIndexFlag { self.flag }
    pub(crate) fn mode(&self) -> IndexRowMode { self.mode }
}

impl<T, O: Copy + Add<Output = O> + From<u8>> LinkedRow<T, O> {
    pub(crate) fn offset(&self) -> O { self.offset }
    pub(crate) fn first_index(&self) -> LocatedCompactIndex<O> { LocatedCompactIndex { atom: self.first_index, offset: self.offset + O::from(LINKED_PREFIX.len() as u8) } }
    pub(crate) fn target_index(&self) -> PositionedIndex<'_, T, O> { PositionedIndex { atom: self.target_index.atom, target: &self.target_index.target, offset: self.offset + O::from(LINKED_PREFIX.len() as u8 + width(self.first_index) + LINKED_MIDDLE.len() as u8 + 1) } }
    pub(crate) fn indices(&self) -> [PositionedIndex<'_, T, O>; 3] { positions(&self.indices, self.offset + O::from(self.indices_start())) }
}

impl<T> LinkedRow<T, usize> {
    pub(crate) fn into_absolute(self, base: u64) -> Option<LinkedRow<T, u64>> {
        let offset = base.checked_add(self.offset as u64)?;
        LinkedRow::<T, u64>::new(self.first_index, self.discriminator, self.target_index, self.indices, self.flag, self.mode, offset)
    }
}

impl<O> LinkedRow<(), O> {
    pub(crate) fn try_resolve<U>(self, mut resolve: impl FnMut(CompactIndexAtom) -> Option<U>) -> Option<LinkedRow<U, O>> {
        let target_index = RowIndex { atom: self.target_index.atom, target: resolve(self.target_index.atom)? };
        let [a, b, c] = self.indices.map(|index| Some(RowIndex { atom: index.atom, target: resolve(index.atom)? }));
        Some(LinkedRow { offset: self.offset, indices: [a?, b?, c?], first_index: self.first_index, discriminator: self.discriminator, flag: self.flag, mode: self.mode, target_index })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TargetRow<T = (), O = usize> {
    offset: O,
    target_index: RowIndex<T>,
    indices: [RowIndex<T>; 3],
    mode: IndexRowMode,
}

impl<T, O> TargetRow<T, O> {
    fn indices_start(&self) -> u8 { TARGET_PREFIX.len() as u8 + width(self.target_index.atom) + TARGET_MIDDLE.len() as u8 }
    pub(crate) fn byte_len(&self) -> u8 { self.indices_start() + self.indices.iter().map(|index| width(index.atom)).sum::<u8>() + 4 + ROW_SUFFIX.len() as u8 }
    pub(crate) fn mode(&self) -> IndexRowMode { self.mode }
}

impl<T, O: Copy + Add<Output = O> + From<u8>> TargetRow<T, O> {
    pub(crate) fn offset(&self) -> O { self.offset }
    pub(crate) fn target_index(&self) -> PositionedIndex<'_, T, O> { PositionedIndex { atom: self.target_index.atom, target: &self.target_index.target, offset: self.offset + O::from(TARGET_PREFIX.len() as u8) } }
    pub(crate) fn indices(&self) -> [PositionedIndex<'_, T, O>; 3] { positions(&self.indices, self.offset + O::from(self.indices_start())) }
}

impl<T> TargetRow<T, usize> {
    pub(crate) fn into_absolute(self, base: u64) -> Option<TargetRow<T, u64>> {
        let offset = base.checked_add(self.offset as u64)?;
        TargetRow::<T, u64>::new(self.target_index, self.indices, self.mode, offset)
    }
}

impl<O> TargetRow<(), O> {
    pub(crate) fn try_resolve<U>(self, mut resolve: impl FnMut(CompactIndexAtom) -> Option<U>) -> Option<TargetRow<U, O>> {
        let target_index = RowIndex { atom: self.target_index.atom, target: resolve(self.target_index.atom)? };
        let [a, b, c] = self.indices.map(|index| Some(RowIndex { atom: index.atom, target: resolve(index.atom)? }));
        Some(TargetRow { offset: self.offset, indices: [a?, b?, c?], mode: self.mode, target_index })
    }
}

macro_rules! checked_origins {
    ($offset:ty) => {
        impl<T> IndexRow<T, $offset> {
            pub(crate) fn new(first_index: CompactIndexAtom, flag: LinkedIndexFlag, indices: [RowIndex<T>; 4], offset: $offset) -> Option<Self> {
                let row = Self { offset, first_index, flag, indices };
                offset.checked_add(<$offset>::from(row.byte_len()))?;
                Some(row)
            }
        }
        impl<T> LinkedRow<T, $offset> {
            pub(crate) fn new(first_index: CompactIndexAtom, discriminator: LinkedIndexDiscriminator, target_index: RowIndex<T>, indices: [RowIndex<T>; 3], flag: LinkedIndexFlag, mode: IndexRowMode, offset: $offset) -> Option<Self> {
                let row = Self { offset, first_index, discriminator, target_index, indices, flag, mode };
                offset.checked_add(<$offset>::from(row.byte_len()))?;
                Some(row)
            }
        }
        impl<T> TargetRow<T, $offset> {
            pub(crate) fn new(target_index: RowIndex<T>, indices: [RowIndex<T>; 3], mode: IndexRowMode, offset: $offset) -> Option<Self> {
                let row = Self { offset, target_index, indices, mode };
                offset.checked_add(<$offset>::from(row.byte_len()))?;
                Some(row)
            }
        }
    };
}
checked_origins!(usize);
checked_origins!(u64);

#[cfg(test)]
mod tests;
