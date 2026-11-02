// SPDX-License-Identifier: Apache-2.0

use crate::brep::topology::{parse_point, scan_plain};
use crate::test_support::parasolid::world_point;

fn wide_point(first: bool) -> Vec<u8> {
    let mut bytes = vec![0, 0x1d];
    if first {
        bytes.push(0xff);
    }
    bytes.extend([0xff, 0x72, 0, 1]);
    bytes.extend(0x1200_u32.to_be_bytes());
    for reference in [1_u16, 2, 3, 4] {
        bytes.extend(reference.to_be_bytes());
    }
    for value in [1.0_f64, 2.0, 3.0] {
        bytes.extend(value.to_be_bytes());
    }
    bytes
}

#[test]
fn wide_point_and_first_of_type_prefix_have_complete_coordinate_boundaries() {
    for first in [false, true] {
        for next in [false, true] {
            let mut bytes = wide_point(first);
            let end = bytes.len();
            if next {
                bytes.extend(world_point(211, [4.0, 5.0, 6.0]));
            }
            let point = parse_point(&bytes, 0, false).expect("complete wide POINT");
            assert_eq!(point.attr, 0xff72);
            assert_eq!(point.refs, [1, 2, 3, 4]);
            assert_eq!(point.xyz_m, [1.0, 2.0, 3.0]);
            assert_eq!(point.xyz_offset + 24, end);
            let points = scan_plain(&bytes);
            assert_eq!(points.points().len(), 1 + usize::from(next));
            assert!(points.points().contains_key(&0xff72));
        }
    }
}

#[test]
fn truncated_coordinate_vectors_leave_no_complete_point_frame() {
    for first in [false, true] {
        let mut bytes = wide_point(first);
        bytes.truncate(bytes.len() - 2);
        assert!(parse_point(&bytes, 0, false).is_none());
    }
}
