// SPDX-License-Identifier: Apache-2.0

use super::scan;

#[test]
fn row_positions_follow_token_widths_and_bound_absolute_extent() {
    for first in [&[7][..], &[128, 7][..]] {
        for target in [&[8][..], &[128, 8][..]] {
            for index in [&[9][..], &[128, 9][..]] {
                let mut bytes = vec![0x2d, 0x02, 0x0b];
                let first_offset = bytes.len();
                bytes.extend(first);
                bytes.extend([0x93, 0x8a, 3]);
                let index_offsets = std::array::from_fn::<_, 4, _>(|_| {
                    let offset = bytes.len();
                    bytes.extend(index);
                    offset
                });
                bytes.extend([0, 0x47, 4, 4, 1, 0xc0, 0x44, 4, 0]);
                let rows = scan::index_rows(&bytes);
                let [row] = rows.as_slice() else {
                    panic!("one complete index row");
                };
                assert_eq!(usize::from(row.byte_len()), bytes.len());
                assert_eq!(row.first_index().offset, first_offset);
                assert_eq!(row.indices().map(|index| index.offset), index_offsets);
                let base = u64::MAX - bytes.len() as u64;
                let absolute = row.into_absolute(base).unwrap();
                assert_eq!(absolute.first_index().offset, base + first_offset as u64);
                assert_eq!(
                    absolute.indices().map(|index| index.offset),
                    index_offsets.map(|offset| base + offset as u64)
                );
                assert!(row.into_absolute(base + 1).is_none());
                assert!(row.try_resolve(|_| None::<String>).is_none());

                let mut bytes = vec![2, 0x0b];
                let first_offset = bytes.len();
                bytes.extend(first);
                bytes.extend([0x93, 0x8c, 0x16]);
                let target_offset = bytes.len();
                bytes.extend(target);
                bytes.extend([0xff, 0xff, 0x90, 0xfe]);
                let index_offsets = std::array::from_fn::<_, 3, _>(|_| {
                    let offset = bytes.len();
                    bytes.extend(index);
                    offset
                });
                bytes.extend([0, 0x47, 3, 4, 1, 0xc0, 0x44, 4, 0]);
                let rows = scan::linked_rows(&bytes);
                let [row] = rows.as_slice() else {
                    panic!("one complete linked row");
                };
                assert_eq!(usize::from(row.byte_len()), bytes.len());
                assert_eq!(row.first_index().offset, first_offset);
                assert_eq!(row.target_index().offset, target_offset);
                assert_eq!(row.indices().map(|index| index.offset), index_offsets);
                let base = u64::MAX - bytes.len() as u64;
                let absolute = row.into_absolute(base).unwrap();
                assert_eq!(absolute.first_index().offset, base + first_offset as u64);
                assert_eq!(absolute.target_index().offset, base + target_offset as u64);
                assert_eq!(
                    absolute.indices().map(|index| index.offset),
                    index_offsets.map(|offset| base + offset as u64)
                );
                assert!(row.into_absolute(base + 1).is_none());
                assert!(row.try_resolve(|_| None::<String>).is_none());

                let mut bytes = vec![2, 1, 1, 1, 0x16];
                let target_offset = bytes.len();
                bytes.extend(target);
                bytes.extend([0xff, 0xff, 0x90, 0xfe]);
                let index_offsets = std::array::from_fn::<_, 3, _>(|_| {
                    let offset = bytes.len();
                    bytes.extend(index);
                    offset
                });
                bytes.extend([0, 0x47, 3, 7, 1, 0xc0, 0x44, 4, 0]);
                let rows = scan::target_rows(&bytes);
                let [row] = rows.as_slice() else {
                    panic!("one complete target row");
                };
                assert_eq!(usize::from(row.byte_len()), bytes.len());
                assert_eq!(row.target_index().offset, target_offset);
                assert_eq!(row.indices().map(|index| index.offset), index_offsets);
                let base = u64::MAX - bytes.len() as u64;
                let absolute = row.into_absolute(base).unwrap();
                assert_eq!(absolute.target_index().offset, base + target_offset as u64);
                assert_eq!(
                    absolute.indices().map(|index| index.offset),
                    index_offsets.map(|offset| base + offset as u64)
                );
                assert!(row.into_absolute(base + 1).is_none());
                assert!(row.try_resolve(|_| None::<String>).is_none());
            }
        }
    }
}

#[test]
fn column_row_slots_reject_out_of_lane_wire_indices() {
    for value in 0..=u8::MAX {
        let decoded = serde_json::from_value::<super::ColumnRowSlot>(value.into());
        assert_eq!(decoded.is_ok(), value < 4);
        if let Ok(slot) = decoded {
            assert_eq!(
                serde_json::to_value(slot).unwrap(),
                serde_json::json!(value)
            );
        }
    }
}
