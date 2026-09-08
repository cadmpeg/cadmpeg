// SPDX-License-Identifier: Apache-2.0
//! Complete counted and ABR lane admission.

use super::{AbrLane, CountedLane, ABR_TERMINATOR, COUNTED_PREFIX, COUNTED_TERMINATOR};
use crate::om::compact::{CountedIndexMembers, LocatedCompactIndex, NullableCompactIndex};

/// Decode fixed-width `ABR` block-reference lanes from contiguous column storage.
pub(crate) fn abr_lanes(bytes: &[u8]) -> Vec<AbrLane> {
    let mut lanes = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        if bytes[start] != 0x11 {
            start += 1;
            continue;
        }
        let mut at = start + 1;
        let tokens = (|| {
            let mut slots = [None; 16];
            for slot in &mut slots {
                let token = NullableCompactIndex::read(bytes, at)?;
                at += token.raw().len();
                *slot = token.atom.map(Into::into);
            }
            Some(slots)
        })();
        let Some(tokens) = tokens else {
            start += 1;
            continue;
        };
        let Some(end) = at.checked_add(ABR_TERMINATOR.len()) else {
            start += 1;
            continue;
        };
        if bytes.get(at..end) == Some(&ABR_TERMINATOR) {
            if let Some(lane) = AbrLane::<(), usize>::new(tokens, start) {
                lanes.push(lane);
            }
            start = end;
        } else {
            start += 1;
        }
    }
    lanes
}

/// Decode complete counted compact-index lanes from one bounded store block.
///
/// A lane is `01, count:u8, anchor, member[count-2], 01 11`, with
/// `count >= 3`. Compact indices use the ordinary direct/extended encoding;
/// null indices reject the candidate atomically.
pub(crate) fn counted_lanes(bytes: &[u8]) -> Vec<CountedLane> {
    let decode = |start: usize| {
        (bytes.get(start) == Some(&0x01)).then_some(())?;
        let declared_count = *bytes.get(start + 1)?;
        (declared_count >= 3).then_some(())?;
        let anchor = LocatedCompactIndex::read(bytes, start + usize::from(COUNTED_PREFIX))?;
        let members_start = anchor.offset + anchor.atom.raw().len();
        let mut at = members_start;
        for _ in 0..usize::from(declared_count) - 2 {
            at += LocatedCompactIndex::read(bytes, at)?.atom.raw().len();
        }
        let end = at.checked_add(COUNTED_TERMINATOR.len())?;
        (bytes.get(at..end) == Some(&COUNTED_TERMINATOR)).then_some(())?;
        at = members_start;
        let members = (0..usize::from(declared_count) - 2)
            .map(|_| {
                let token = LocatedCompactIndex::read(bytes, at)?;
                at += token.atom.raw().len();
                Some(token.atom.into())
            })
            .collect::<Option<Vec<_>>>()?;
        Some((
            CountedLane::<(), usize>::new(
                anchor.atom.into(),
                CountedIndexMembers::new(members).ok()?,
                start,
            )?,
            end,
        ))
    };
    let mut lanes = Vec::new();
    let mut start = 0;
    while start + 4 <= bytes.len() {
        if let Some((lane, end)) = decode(start) {
            lanes.push(lane);
            start = end;
        } else {
            start += 1;
        }
    }
    lanes
}

#[cfg(test)]
mod tests {
    #[test]
    fn om_offset_store_counted_index_lane_requires_complete_non_null_members() {
        let bytes = [
            0xaa, 0x01, 0x06, 0x42, 0x62, 0x80, 0x48, 0x80, 0x50, 0x7c, 0x01, 0x11, 0xbb,
        ];
        let lanes = crate::om::compact_lane::scan::counted_lanes(&bytes);
        assert_eq!(lanes.len(), 1);
        assert_eq!(lanes[0].offset(), 1);
        assert_eq!(lanes[0].declared_count(), 6);
        assert_eq!(lanes[0].anchor().atom.value(), 0x42);
        assert_eq!(lanes[0].anchor().atom.raw(), [0x42]);
        assert_eq!(lanes[0].anchor().offset, 3);
        assert_eq!(
            lanes[0]
                .members()
                .map(|token| (token.atom.value(), token.offset))
                .collect::<Vec<_>>(),
            vec![(0x62, 4), (0x48, 5), (0x50, 7), (0x7c, 9)]
        );
        assert_eq!(
            lanes[0]
                .members()
                .map(|token| token.atom.raw().to_vec())
                .collect::<Vec<_>>(),
            [vec![0x62], vec![0x80, 0x48], vec![0x80, 0x50], vec![0x7c]]
        );

        assert!(crate::om::compact_lane::scan::counted_lanes(&[
            0x01, 0x03, 0x42, 0xff, 0x01, 0x11,
        ])
        .is_empty());
        assert!(crate::om::compact_lane::scan::counted_lanes(&[
            0x01, 0x03, 0x42, 0x80, 0x01, 0x11,
        ])
        .is_empty());
        assert!(crate::om::compact_lane::scan::counted_lanes(&[
            0x01, 0x03, 0x42, 0x62, 0x01, 0x10,
        ])
        .is_empty());
    }

    #[test]
    fn om_offset_store_abr_lane_requires_sixteen_slots_and_exact_terminator() {
        let mut bytes = vec![0xaa, 0x11];
        bytes.extend_from_slice(&[0xff; 6]);
        bytes.extend_from_slice(&[0x82, 0x83]);
        bytes.extend_from_slice(&[0xff; 9]);
        bytes.extend_from_slice(&[0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03, 0xbb]);

        let lanes = crate::om::compact_lane::scan::abr_lanes(&bytes);
        assert_eq!(lanes.len(), 1);
        assert_eq!(lanes[0].offset(), 1);
        assert_eq!(lanes[0].slots().len(), 16);
        assert_eq!(
            (
                lanes[0].slots()[6].atom.map(|index| index.atom.value()),
                lanes[0].slots()[6].offset
            ),
            (Some(643), 8)
        );
        assert_eq!(lanes[0].slots()[6].atom.unwrap().atom.raw(), [0x82, 0x83]);
        assert!(lanes[0]
            .slots()
            .iter()
            .enumerate()
            .all(|(slot, token)| slot == 6
                || token.atom.map_or(&[0xff][..], |index| index.atom.raw()) == [0xff]));
        assert!(lanes[0]
            .slots()
            .iter()
            .enumerate()
            .all(|(slot, token)| slot == 6 || token.atom.is_none()));

        bytes[23] = b'X';
        assert!(crate::om::compact_lane::scan::abr_lanes(&bytes).is_empty());
        bytes[23] = b'R';
        bytes.remove(18);
        assert!(crate::om::compact_lane::scan::abr_lanes(&bytes).is_empty());
    }
}
