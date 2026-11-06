// SPDX-License-Identifier: Apache-2.0

use crate::brep::topology::{nodes, point_value_offsets};
use crate::test_support::parasolid::sequence::topology_sequence;

#[test]
fn framed_point_values_come_from_the_node_sequence() {
    use crate::test_support::parasolid::world_point;
    let (mut payload, body_offset) = topology_sequence();
    let suffix = payload.len() - body_offset;
    payload.extend(world_point(77, [1.0, 2.0, 3.0]));
    let records = nodes::sequence(&payload).expect("terminated sequence");
    let expected: Vec<(usize, usize)> = records
        .nodes()
        .filter(|node| node.kind() == 29)
        .map(|node| {
            (
                node.range().start - body_offset,
                node.field_range("pvec").unwrap().start - body_offset,
            )
        })
        .collect();
    assert!(!expected.is_empty());
    let scanned = crate::brep::topology::scan(&payload[body_offset..]);
    assert!(scanned
        .points()
        .values()
        .any(|point| point.offset == suffix));
    let offsets = point_value_offsets(&payload, body_offset);
    assert_eq!(offsets.len(), expected.len());
    for (record, values) in expected {
        assert_eq!(offsets.get(&record), Some(&values));
    }
}

#[test]
fn unframed_point_values_come_from_the_scanned_body() {
    use crate::test_support::parasolid::{parasolid_with_body, triangle_body};
    let payload = parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body());
    let header = crate::parasolid::stream_header(&payload).unwrap().unwrap();
    assert!(nodes::sequence(&payload).is_none());
    let body = &payload[header.body_offset..];
    let scanned: Vec<(usize, usize)> = crate::brep::topology::scan(body)
        .points()
        .values()
        .map(|point| (point.offset, point.xyz_offset))
        .collect();
    assert!(!scanned.is_empty());
    let offsets = point_value_offsets(&payload, header.body_offset);
    assert_eq!(offsets.len(), scanned.len());
    for (record, values) in scanned {
        assert_eq!(offsets.get(&record), Some(&values));
    }
}
