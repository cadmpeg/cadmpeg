// SPDX-License-Identifier: Apache-2.0
//! Terminal datum-plane index lanes with derived token positions.

use super::compact::NullableCompactIndex;
use super::compact::{CompactIndexAtom, CountedIndexMembers, LocatedCompactIndex};
use cadmpeg_core::decode::View;
use std::ops::Add;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DatumIndexLane<O = usize> {
    offset: O,
    indices: CountedIndexMembers<CompactIndexAtom, 1>,
    trailer: u32,
}

impl<O> DatumIndexLane<O> {
    pub(crate) fn declared_count(&self) -> u8 {
        self.indices.declared_count()
    }
    pub(crate) fn trailer(&self) -> u32 {
        self.trailer
    }
    fn byte_len(&self) -> u16 {
        7 + self
            .indices
            .as_slice()
            .iter()
            .map(|atom| atom.raw().len() as u16)
            .sum::<u16>()
    }
}

impl<O: Copy + Add<Output = O> + From<u16>> DatumIndexLane<O> {
    pub(crate) fn offset(&self) -> O {
        self.offset
    }
    pub(crate) fn indices(&self) -> impl Iterator<Item = LocatedCompactIndex<O>> + '_ {
        let mut offset = self.offset + O::from(2);
        self.indices.as_slice().iter().map(move |atom| {
            let token = LocatedCompactIndex {
                atom: *atom,
                offset,
            };
            offset = offset + O::from(atom.raw().len() as u16);
            token
        })
    }
}

macro_rules! checked_origin {
    ($offset:ty) => {
        impl DatumIndexLane<$offset> {
            pub(crate) fn new(
                indices: CountedIndexMembers<CompactIndexAtom, 1>,
                trailer: u32,
                offset: $offset,
            ) -> Option<Self> {
                let lane = Self {
                    offset,
                    indices,
                    trailer,
                };
                offset.checked_add(<$offset>::from(lane.byte_len()))?;
                Some(lane)
            }
        }
    };
}
checked_origin!(usize);
checked_origin!(u64);

impl DatumIndexLane<usize> {
    pub(crate) fn into_u64(self) -> DatumIndexLane<u64> {
        DatumIndexLane {
            offset: self.offset as u64,
            indices: self.indices,
            trailer: self.trailer,
        }
    }
}

/// Decode unique datum-plane index lanes ending at the logical payload boundary.
pub(crate) fn scan(bytes: &[u8]) -> Vec<DatumIndexLane> {
    let mut lanes = Vec::new();
    for start in 0..bytes.len().saturating_sub(7) {
        if bytes[start] != 0x01 {
            continue;
        }
        let declared_count = bytes[start + 1];
        if declared_count < 2 {
            continue;
        }
        let mut scan_at = start + 2;
        let mut complete = true;
        for _ in 1..declared_count {
            let Some(token) =
                NullableCompactIndex::read(bytes, scan_at).filter(|token| token.atom.is_some())
            else {
                complete = false;
                break;
            };
            scan_at += token.raw().len();
        }
        if !complete || bytes.get(scan_at) != Some(&0x00) || scan_at + 5 != bytes.len() {
            continue;
        }
        let mut at = start + 2;
        let indices = (1..declared_count)
            .map(|_| {
                let token = LocatedCompactIndex::read(&bytes[..scan_at], at)?;
                at += token.atom.raw().len();
                Some(token.atom)
            })
            .collect::<Option<Vec<_>>>();
        let Some(indices) = indices.and_then(|indices| CountedIndexMembers::new(indices).ok())
        else {
            continue;
        };
        let Some(trailer) = View::u32_be_at(bytes, scan_at + 1) else {
            continue;
        };
        if let Some(lane) = DatumIndexLane::<usize>::new(indices, trailer, start) {
            lanes.push(lane);
        }
    }
    lanes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datum_terminal_positions_follow_mixed_token_widths_and_checked_extent() {
        for count in [1, 254] {
            let mut bytes = vec![0x7f, 0x01, (count + 1) as u8];
            let mut offsets = Vec::new();
            for slot in 0..count {
                offsets.push(bytes.len());
                if slot % 2 == 0 {
                    bytes.extend_from_slice(&[0x80, 1]);
                } else {
                    bytes.push(2);
                }
            }
            bytes.extend_from_slice(&[0, 0x12, 0x34, 0x56, 0x78]);
            let lane = scan(&bytes)
                .into_iter()
                .find(|lane| lane.offset() == 1)
                .unwrap();
            assert_eq!(
                lane.indices().map(|token| token.offset).collect::<Vec<_>>(),
                offsets
            );
            assert_eq!(usize::from(lane.declared_count()), count + 1);
            assert_eq!(lane.trailer(), 0x1234_5678);
            let origin = u64::MAX - (bytes.len() - 1) as u64;
            let last =
                DatumIndexLane::<u64>::new(lane.indices.clone(), lane.trailer, origin).unwrap();
            assert_eq!(
                last.indices().map(|token| token.offset).collect::<Vec<_>>(),
                offsets
                    .iter()
                    .map(|offset| origin + (*offset - 1) as u64)
                    .collect::<Vec<_>>()
            );
            assert!(DatumIndexLane::<u64>::new(lane.indices, lane.trailer, origin + 1).is_none());
        }
    }

    #[test]
    fn om_datum_plane_object_index_lane_ends_at_logical_payload_boundary() {
        let bytes = [
            0x80, 0xab, 0x01, 0x04, 0x81, 0x01, 0x01, 0x01, 0x00, 0x12, 0x34, 0x56, 0x78,
        ];
        let lanes = crate::om::datum_index::scan(&bytes);
        assert_eq!(lanes.len(), 1);
        assert_eq!(lanes[0].offset(), 2);
        assert_eq!(usize::from(lanes[0].declared_count()), 4);
        assert_eq!(
            lanes[0]
                .indices()
                .map(|token| (token.atom.value(), token.offset))
                .collect::<Vec<_>>(),
            [(257, 4), (1, 6), (1, 7)]
        );
        assert_eq!(
            lanes[0]
                .indices()
                .map(|token| token.atom.raw().to_vec())
                .collect::<Vec<_>>(),
            [vec![0x81, 0x01], vec![1], vec![1]]
        );
        assert_eq!(lanes[0].trailer(), 0x1234_5678);

        let mut trailing = bytes.to_vec();
        trailing.push(0);
        assert!(crate::om::datum_index::scan(&trailing).is_empty());
    }
}
