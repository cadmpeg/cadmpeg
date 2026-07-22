// SPDX-License-Identifier: Apache-2.0
//! Record-decoder tests for the `zero_entity` family over synthetic byte fixtures.

#![allow(clippy::unwrap_used)]

use crate::tests::{le_f32, le_f64, zero_entity_record, OUTER_MAGIC};

#[test]
fn zero_entity_vertices_exclude_framed_payload_markers() {
    let mut bytes = vec![0u8; 16];
    bytes[..8].copy_from_slice(OUTER_MAGIC);
    let mut record = vec![0u8; 0x15 + 12];
    record[..4].copy_from_slice(&[0xa9, 0x03, 0x10, 0x15]);
    record[4..7].copy_from_slice(&[0x05, 0x08, 0x01]);
    for (index, value) in [90.0f32, 91.0, 92.0].into_iter().enumerate() {
        record[7 + index * 4..11 + index * 4].copy_from_slice(&le_f32(value));
    }
    bytes.extend_from_slice(&record);
    bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
    for value in [1.0f32, 2.0, 3.0] {
        bytes.extend_from_slice(&le_f32(value));
    }

    assert_eq!(
        crate::families::zero_entity::graph::unframed_vertices(&bytes),
        [cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)]
    );

    let mut malformed = vec![0xa9, 0x03, 0x10, 0xff, 0x05, 0x08, 0x01];
    malformed.extend_from_slice(&[0; 12]);
    assert!(crate::families::zero_entity::graph::unframed_vertices(&malformed).is_empty());
}

#[test]
fn zero_entity_parser_decodes_face_loop_lanes_and_packed_senses() {
    let mut carrier = zero_entity_record(0x27, vec![0; 106]);
    for (offset, value) in [
        (14usize, 10.0f64),
        (22, 0.0),
        (30, 0.0),
        (38, 1.0),
        (46, 0.0),
        (54, 0.0),
        (62, 0.0),
        (70, 1.0),
        (78, 0.0),
    ] {
        carrier[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    let mut support_tail = vec![0; 113];
    support_tail[0] = 0x10;
    support_tail[1..5].copy_from_slice(&2u32.to_le_bytes());
    for (offset, value) in [(81usize, 1.0f64), (89, 2.0), (97, 3.0), (105, 4.0)] {
        support_tail[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    let support = zero_entity_record(0x21, support_tail);
    let mut face_tail = vec![0x82, 0x10];
    face_tail.extend_from_slice(&1000u32.to_le_bytes());
    face_tail.push(0x10);
    face_tail.extend_from_slice(&900u32.to_le_bytes());
    face_tail.push(0);
    let face = zero_entity_record(0x5f, face_tail);

    let mut loop_tail = vec![0x85];
    for reference in [98u32, 500, 97, 501, 100] {
        loop_tail.push(0x10);
        loop_tail.extend_from_slice(&reference.to_le_bytes());
    }
    loop_tail.extend_from_slice(&[0x82, 0x41, 0b01_0111, 0x01]);
    let loop_record = zero_entity_record(0x62, loop_tail);
    let mut physical_edge = zero_entity_record(0x5e, vec![0; 26]);
    for (offset, reference) in [
        (7usize, 10u32),
        (12, 20),
        (17, 30),
        (22, 40),
        (27, 50),
        (32, 60),
    ] {
        physical_edge[offset] = 0x10;
        physical_edge[offset + 1..offset + 5].copy_from_slice(&reference.to_le_bytes());
    }
    let mut side_pair_tail = vec![0x82, 0x10];
    side_pair_tail.extend_from_slice(&1000u32.to_le_bytes());
    side_pair_tail.push(0x10);
    side_pair_tail.extend_from_slice(&2000u32.to_le_bytes());
    side_pair_tail.resize(105, 0);
    let side_pair = zero_entity_record(0x25, side_pair_tail);
    let coedge_twin = |side: u8| {
        let mut record = zero_entity_record(0x06, vec![0; 56]);
        record[7] = 0x10;
        record[8..12].copy_from_slice(&1u32.to_le_bytes());
        record[12..15].copy_from_slice(&[0x83, 0x10, side]);
        for (offset, reference) in [
            (15usize, 1000u32 + u32::from(side)),
            (20, 2000u32 + u32::from(side)),
        ] {
            record[offset] = 0x10;
            record[offset + 1..offset + 5].copy_from_slice(&reference.to_le_bytes());
        }
        record
    };
    let mut incidence_tail = vec![0x83];
    for item in [700u32, 701, 702] {
        incidence_tail.push(0x10);
        incidence_tail.extend_from_slice(&item.to_le_bytes());
    }
    let incidence = zero_entity_record(0x05, incidence_tail);
    let vertex_marker = zero_entity_record(0x5d, vec![0; 6]);
    let mut bytes = carrier;
    bytes.extend_from_slice(&support);
    bytes.extend_from_slice(&face);
    bytes.extend_from_slice(&loop_record);
    bytes.extend_from_slice(&physical_edge);
    bytes.extend_from_slice(&side_pair);
    bytes.extend_from_slice(&coedge_twin(1));
    bytes.extend_from_slice(&coedge_twin(2));
    bytes.extend_from_slice(&incidence);
    bytes.extend_from_slice(&vertex_marker);

    let topology =
        crate::families::zero_entity::graph::parse(&bytes).expect("zero-entity topology records");
    assert_eq!(topology.faces[0].loop_terminals, vec![100]);
    assert_eq!(topology.loops[0].member_ids, vec![98, 97]);
    assert_eq!(topology.loops[0].secondary_refs, vec![500, 501]);
    assert_eq!(topology.loops[0].terminal_id, 100);
    assert_eq!(topology.loops[0].reversed, vec![false, true]);
    assert!(!topology.loops[0].inner);
    assert_eq!(topology.carrier_runs[0].support_ordinals, vec![1]);
    assert_eq!(topology.supports[0].slot, 2);
    assert_eq!(
        topology.supports[0].uv_endpoints,
        Some([[1.0, 2.0], [3.0, 4.0]])
    );
    assert_eq!(topology.faces[0].loop_indices, vec![0]);
    assert_eq!(topology.loops[0].support_indices, vec![Some(0), None]);
    assert_eq!(
        topology.supports[0].lifted_endpoints,
        Some([[11.0, 2.0, 0.0], [13.0, 4.0, 0.0]])
    );
    assert_eq!(
        topology.physical_edges[0].references,
        [10, 20, 30, 40, 50, 60]
    );
    assert_eq!(topology.side_pairs[0].bases, [1000, 2000]);
    assert_eq!(
        topology.side_pairs[0].composite_keys,
        [[1001, 2001], [1002, 2002]]
    );
    assert_eq!(topology.coedge_twins[1].side, 2);
    assert_eq!(topology.vertices[0].incidence_items, vec![700, 701, 702]);
}
