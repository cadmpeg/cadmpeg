// SPDX-License-Identifier: Apache-2.0
//! Lane extent and token-position construction boundaries.

use super::*;

#[test]
fn counted_lane_positions_follow_every_encoded_width() {
    for anchor_raw in [&[1][..], &[128, 1][..]] {
        for member_raw in [&[2][..], &[128, 2][..]] {
            for count in [3_u8, 255] {
                let mut bytes = vec![0xaa, 0x01, count];
                let anchor_offset = bytes.len();
                bytes.extend_from_slice(anchor_raw);
                let mut offsets = Vec::new();
                for _ in 2..count {
                    offsets.push(bytes.len());
                    bytes.extend_from_slice(member_raw);
                }
                bytes.extend_from_slice(&[0x01, 0x11]);
                let [lane]: [CountedLane; 1] = scan::counted_lanes(&bytes).try_into().unwrap();
                assert_eq!(lane.anchor().offset, anchor_offset);
                assert_eq!(
                    lane.members().map(|index| index.offset).collect::<Vec<_>>(),
                    offsets
                );
                assert_eq!(lane.declared_count(), count);
                let base = u64::MAX - bytes.len() as u64;
                let absolute = lane.clone().into_absolute(base).unwrap();
                assert_eq!(absolute.anchor().offset, base + anchor_offset as u64);
                assert_eq!(
                    absolute
                        .members()
                        .map(|index| index.offset)
                        .collect::<Vec<_>>(),
                    offsets
                        .iter()
                        .map(|offset| base + *offset as u64)
                        .collect::<Vec<_>>()
                );
                assert!(lane.clone().into_absolute(base + 1).is_none());
                let resolved = lane.clone().try_resolve(|atom| Some(atom.value())).unwrap();
                assert_eq!(*resolved.anchor().target, 1);
                assert!(resolved.members().all(|index| *index.target == 2));
                assert!(lane
                    .try_resolve(|atom| (atom.value() != 2).then_some(()))
                    .is_none());
            }
        }
    }
}

#[test]
fn abr_lane_positions_include_null_and_extended_widths() {
    for raw in [&[0xff][..], &[2][..], &[128, 2][..]] {
        let mut bytes = vec![0xaa, 0x11];
        let mut offsets = [0; 16];
        for offset in &mut offsets {
            *offset = bytes.len();
            bytes.extend_from_slice(raw);
        }
        bytes.extend_from_slice(&[0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03]);
        let [lane]: [AbrLane; 1] = scan::abr_lanes(&bytes).try_into().unwrap();
        assert_eq!(lane.slots().map(|slot| slot.offset), offsets);
        let base = u64::MAX - bytes.len() as u64;
        let absolute = lane.clone().into_absolute(base).unwrap();
        assert_eq!(
            absolute.slots().map(|slot| slot.offset),
            offsets.map(|offset| base + offset as u64)
        );
        assert!(lane.clone().into_absolute(base + 1).is_none());
        let resolved = lane.clone().try_resolve(|atom| Some(atom.value())).unwrap();
        assert!(resolved
            .slots()
            .iter()
            .all(|slot| slot.atom.map(|index| index.target)
                == if raw == [0xff] { None } else { Some(2) }));
        assert_eq!(lane.try_resolve(|_| None::<()>).is_some(), raw == [0xff]);
    }
}
