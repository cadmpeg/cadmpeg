// SPDX-License-Identifier: Apache-2.0
//! Draft leading index frames with token positions derived from their encodings.

use std::ops::Add;
use super::operation_record::OperationPayload;
use super::compact::{CompactIndexTarget, CountedIndexMembers, LocatedCompactIndex, PositionedIndex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DraftLeadingLane<T = (), O = usize> {
    offset: O,
    indices: CountedIndexMembers<CompactIndexTarget<T>, 1>,
}

impl<T, O> DraftLeadingLane<T, O> {
    pub(crate) fn declared_count(&self) -> u8 { self.indices.declared_count() }
    fn byte_len(&self) -> u16 {
        26 + self.indices.as_slice().iter().map(|token| token.atom.raw().len() as u16).sum::<u16>()
    }
}

impl<T, O: Copy + Add<Output = O> + From<u16>> DraftLeadingLane<T, O> {
    pub(crate) fn indices(&self) -> impl Iterator<Item = PositionedIndex<'_, T, O>> {
        let mut offset = self.offset + O::from(24);
        self.indices.as_slice().iter().map(move |token| {
            let positioned = PositionedIndex { atom: token.atom, target: &token.target, offset };
            offset = offset + O::from(token.atom.raw().len() as u16);
            positioned
        })
    }
}

macro_rules! checked_origin {
    ($offset:ty) => {
        impl<T> DraftLeadingLane<T, $offset> {
            pub(crate) fn new(indices: CountedIndexMembers<CompactIndexTarget<T>, 1>, offset: $offset) -> Option<Self> {
                let lane = Self { offset, indices };
                offset.checked_add(<$offset>::from(lane.byte_len()))?;
                Some(lane)
            }
        }
    };
}
checked_origin!(usize);
checked_origin!(u64);

impl DraftLeadingLane<(), usize> {
    pub(crate) fn into_absolute(self, base: u64) -> Option<DraftLeadingLane<(), u64>> {
        DraftLeadingLane::<(), u64>::new(self.indices, base.checked_add(self.offset as u64)?)
    }
}

impl<O> DraftLeadingLane<(), O> {
    pub(crate) fn resolve<T>(self, mut resolve: impl FnMut(u32) -> T) -> DraftLeadingLane<T, O> {
        DraftLeadingLane {
            offset: self.offset,
            indices: self.indices.map(|token| CompactIndexTarget { atom: token.atom, target: resolve(token.atom.value()) }),
        }
    }

    pub(crate) fn try_resolve<T>(self, mut resolve: impl FnMut(u32) -> Option<T>) -> Option<DraftLeadingLane<T, O>> {
        Some(DraftLeadingLane {
            offset: self.offset,
            indices: self.indices.try_map(|token| Some(CompactIndexTarget { atom: token.atom, target: resolve(token.atom.value())? }))?,
        })
    }
}

/// Decode the exactly positioned counted compact-index lane preceding a `DRAFT` graph.
pub(crate) fn scan(
    record: OperationPayload<'_>,
) -> Option<DraftLeadingLane> {
    const PREFIX: [u8; 22] = [
        0x67, 0x00, 0x00, 0x01, 0x00, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 0x03, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ];
    if record.name() != "DRAFT" || record.payload().get(..PREFIX.len()) != Some(&PREFIX) {
        return None;
    }
    let mut at = PREFIX.len();
    (record.payload().get(at) == Some(&0x01)).then_some(())?;
    let declared_count = *record.payload().get(at + 1)?;
    (declared_count >= 2).then_some(())?;
    at += 2;
    let mut indices = Vec::with_capacity(usize::from(declared_count - 1));
    for _ in 1..declared_count {
        let token = LocatedCompactIndex::read(record.payload(), at)?;
        at += token.atom.raw().len();
        indices.push(token.atom.into());
    }
    (record.payload().get(at..at + 2) == Some(&[0x01, 0x02])).then_some(())?;

    DraftLeadingLane::<(), usize>::new(CountedIndexMembers::new(indices).ok()?, record.payload_offset())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_leading_positions_follow_token_widths_and_checked_frame_extent() {
        for count in [1, 254] {
            let mut bytes = vec![
                0x67, 0, 0, 1, 0, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 3,
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1, (count + 1) as u8,
            ];
            let mut positions = Vec::new();
            for slot in 0..count {
                positions.push(bytes.len() + 100);
                if slot % 2 == 0 { bytes.extend_from_slice(&[0x80, 7]); }
                else { bytes.push(8); }
            }
            bytes.extend_from_slice(&[1, 2]);
            let frame = scan(OperationPayload::new(&bytes, 100, "DRAFT").unwrap()).unwrap();
            assert_eq!(usize::from(frame.declared_count()), count + 1);
            assert_eq!(frame.indices().map(|token| token.offset).collect::<Vec<_>>(), positions);
            let base = u64::MAX - 100 - bytes.len() as u64;
            let absolute = frame.clone().into_absolute(base).unwrap();
            assert_eq!(absolute.indices().map(|token| token.offset).collect::<Vec<_>>(),
                positions.iter().map(|offset| base + *offset as u64).collect::<Vec<_>>());
            assert!(frame.clone().into_absolute(base + 1).is_none());
            let resolved = absolute.try_resolve(|index| Some(index.to_string())).unwrap();
            assert!(resolved.indices().all(|token| *token.target == token.atom.value().to_string()));
            assert!(frame.try_resolve(|_| None::<String>).is_none());
        }
    }
}
