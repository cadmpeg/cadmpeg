// SPDX-License-Identifier: Apache-2.0
//! Compact-index lanes with token positions derived from their framing.

use super::compact::{
    CompactIndexAtom, CompactIndexTarget, CountedIndexMembers, LocatedCompactIndex, PositionedIndex,
};
use std::ops::Add;

pub(crate) mod scan;

const COUNTED_PREFIX: u16 = 2;
const COUNTED_TERMINATOR_LEN: u16 = 2;
const ABR_TERMINATOR_LEN: u16 = 7;
const COUNTED_TERMINATOR: [u8; 2] = [0x01, 0x11];
const ABR_TERMINATOR: [u8; 7] = [0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CountedLane<T = (), O = usize> {
    offset: O,
    anchor: CompactIndexTarget<T>,
    members: CountedIndexMembers<CompactIndexTarget<T>>,
    byte_len: u16,
}

impl<T, O> CountedLane<T, O> {
    pub(crate) fn declared_count(&self) -> u8 {
        self.members.declared_count()
    }
    fn byte_len(&self) -> u16 {
        self.byte_len
    }

    fn extent<'a, E, I: Iterator<Item = &'a CompactIndexTarget<T>>>(
        anchor: &CompactIndexTarget<T>,
        members: &'a [CompactIndexTarget<T>],
        admit: impl FnOnce(&'a [CompactIndexTarget<T>]) -> Result<I, E>,
    ) -> Result<Option<u16>, E>
    where
        T: 'a,
    {
        let Some(start) = COUNTED_PREFIX.checked_add(u16::from(anchor.atom.byte_len())) else {
            return Ok(None);
        };
        Ok(admit(members)?
            .try_fold(start, |length, index| {
                length.checked_add(u16::from(index.atom.byte_len()))
            })
            .and_then(|length| length.checked_add(COUNTED_TERMINATOR_LEN)))
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
    pub(crate) fn members(&self) -> impl Iterator<Item = PositionedIndex<'_, T, O>> + Clone {
        let mut offset = self.anchor().offset + O::from(u16::from(self.anchor.atom.byte_len()));
        self.members.as_slice().iter().map(move |index| {
            let position = PositionedIndex {
                atom: index.atom,
                target: &index.target,
                offset,
            };
            offset = offset + O::from(u16::from(index.atom.byte_len()));
            position
        })
    }
}

impl<T> CountedLane<T, usize> {
    pub(crate) fn from_wire(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        anchor: CompactIndexTarget<T>,
        members: CountedIndexMembers<CompactIndexTarget<T>>,
        offset: usize,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let byte_len = Self::extent(&anchor, members.as_slice(), |members| {
            ctx.admit_iter(members, "NX counted lane token widths")
        })?
        .ok_or_else(|| ctx.refuse_codec_limit("NX counted lane extent", u64::MAX, u64::MAX))?;
        if offset.checked_add(usize::from(byte_len)).is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            offset,
            anchor,
            members,
            byte_len,
        }))
    }

    pub(crate) fn into_absolute(self, base: u64) -> Option<CountedLane<T, u64>> {
        let offset = base.checked_add(cadmpeg_core::decode::u64_from_index(self.offset))?;
        offset.checked_add(u64::from(self.byte_len()))?;
        Some(CountedLane {
            offset,
            anchor: self.anchor,
            members: self.members,
            byte_len: self.byte_len,
        })
    }
}

impl<O> CountedLane<(), O> {
    pub(crate) fn try_resolve_charged<T>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        mut resolve: impl FnMut(CompactIndexAtom) -> Result<Option<T>, cadmpeg_core::CodecError>,
    ) -> Result<Option<CountedLane<T, O>>, cadmpeg_core::CodecError> {
        let Some(target) = resolve(self.anchor.atom)? else {
            return Ok(None);
        };
        let anchor = CompactIndexTarget {
            atom: self.anchor.atom,
            target,
        };
        let Some(members) = self.members.try_map_charged(ctx, |index| {
            Ok(Some(CompactIndexTarget {
                atom: index.atom,
                target: match resolve(index.atom)? {
                    Some(target) => target,
                    None => return Ok(None),
                },
            }))
        })?
        else {
            return Ok(None);
        };
        Ok(Some(CountedLane {
            offset: self.offset,
            anchor,
            members,
            byte_len: self.byte_len,
        }))
    }
    #[cfg(test)]
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
            byte_len: self.byte_len,
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
                    .map_or(1, |index| u16::from(index.atom.byte_len()))
            })
            .sum::<u16>()
            + ABR_TERMINATOR_LEN
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
                        .map_or(1, |index| u16::from(index.atom.byte_len())),
                );
            position
        })
    }
}

impl<T> AbrLane<T, usize> {
    pub(crate) fn into_absolute(self, base: u64) -> Option<AbrLane<T, u64>> {
        AbrLane::<T, u64>::new(
            self.slots,
            base.checked_add(cadmpeg_core::decode::u64_from_index(self.offset))?,
        )
    }
}

impl<O> AbrLane<(), O> {
    pub(crate) fn try_resolve<T>(
        self,
        mut resolve: impl FnMut(CompactIndexAtom) -> Result<Option<T>, cadmpeg_core::CodecError>,
    ) -> Result<Option<AbrLane<T, O>>, cadmpeg_core::CodecError> {
        let mut slots = std::array::from_fn(|_| None);
        for (slot, source) in slots.iter_mut().zip(self.slots) {
            *slot = match source {
                Some(index) => Some(CompactIndexTarget {
                    atom: index.atom,
                    target: match resolve(index.atom)? {
                        Some(target) => target,
                        None => return Ok(None),
                    },
                }),
                None => None,
            };
        }
        Ok(Some(AbrLane {
            offset: self.offset,
            slots,
        }))
    }
}

impl<T> CountedLane<T, u64> {
    pub(crate) fn new(
        anchor: CompactIndexTarget<T>,
        members: CountedIndexMembers<CompactIndexTarget<T>>,
        offset: u64,
    ) -> Option<Self> {
        let byte_len = match Self::extent(&anchor, members.as_slice(), |members| {
            Ok::<_, std::convert::Infallible>(members.iter())
        }) {
            Ok(byte_len) => byte_len?,
            Err(error) => match error {},
        };
        let lane = Self {
            offset,
            anchor,
            members,
            byte_len,
        };
        offset.checked_add(u64::from(lane.byte_len()))?;
        Some(lane)
    }
}

macro_rules! checked_origins {
    ($offset:ty) => {
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
