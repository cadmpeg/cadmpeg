// SPDX-License-Identifier: Apache-2.0
//! Generated native records for two planar faces with one shared edge.

use super::test_b2::b2_line_profile_stream;
use super::test_container::standard_catpart_from_streams;

pub(crate) fn joined_planar_sheet(roster_control: u8, second_loop_tail: u8) -> Vec<u8> {
    const FIRST_FACE: u8 = 1;
    const FIRST_LOOP: u8 = 3;
    const PLANE: u8 = 5;
    const FIRST_PCURVE: u8 = 6;
    const FIRST_EDGE: u8 = 13;
    const FIRST_VERTEX: u8 = 20;

    fn frame(bytes: &mut Vec<u8>, flag: u8, class: u8, payload: &[u8]) {
        let size = u8::try_from(payload.len()).expect("small fixture payload");
        bytes.extend([0xb2, flag, class, size, 0x05]);
        bytes.extend(payload);
    }
    let child = |from, to| 4 * (to - from - 1) + 3;
    // Include a standalone profile to select the freeform route.
    let mut bytes = b2_line_profile_stream();
    let mut group = vec![0x81, 0x03, 0x05, 0x01];
    for value in [0.0_f32, 2.0, 0.0, 1.0, 0.0, 0.0] {
        group.extend(value.to_le_bytes());
    }
    group.extend([0x08, 0xd6, 0x28]);
    frame(&mut bytes, 0x13, 0x5b, &group);
    frame(&mut bytes, 0x03, 0x61, &[0x82, 3, 7, 0, roster_control, 3]);
    for i in 0..2 {
        let face = FIRST_FACE + i;
        frame(
            &mut bytes,
            0x03,
            0x5f,
            &[0x82, child(face, PLANE), child(face, FIRST_LOOP + i), 3],
        );
    }
    for (index, edges) in [[0, 1, 2, 3], [4, 5, 6, 1]].into_iter().enumerate() {
        let slot = FIRST_LOOP + u8::try_from(index).expect("fixture loop index");
        let mut ring = vec![0x89];
        for edge in edges {
            ring.extend([
                child(slot, FIRST_PCURVE + edge),
                child(slot, FIRST_EDGE + edge),
            ]);
        }
        ring.push(child(slot, PLANE));
        ring.extend([
            0x84,
            0x41,
            0,
            0,
            if index == 0 { 1 } else { second_loop_tail },
        ]);
        frame(&mut bytes, 0x03, 0x62, &ring);
    }

    let mut plane = vec![0x84, 0];
    for value in [
        0.0_f64, 0.0, 0.0, // Origin.
        1.0, 0.0, 0.0, // U direction.
        0.0, 1.0, 0.0, // V direction.
        0.0, 2.0, 0.0, 1.0, // Chart bounds.
    ] {
        plane.extend(value.to_le_bytes());
    }
    frame(&mut bytes, 0x03, 0x27, &plane);

    let vertices = [
        [0.0_f64, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [2.0, 0.0],
        [2.0, 1.0],
    ];
    let edges = [[0_u8, 1], [1, 2], [3, 2], [0, 3], [1, 4], [4, 5], [2, 5]];
    for [start, end] in edges {
        let [x0, y0] = vertices[usize::from(start)];
        let [x1, y1] = vertices[usize::from(end)];
        let (control, values) = if x0 == x1 {
            (1, [x0, y0, y1])
        } else {
            (2, [y0, x0, x1])
        };
        let mut payload = vec![5, control];
        for value in values {
            payload.extend(value.to_le_bytes());
        }
        frame(&mut bytes, 0x03, 0x18, &payload);
    }
    for (index, [start, end]) in edges.into_iter().enumerate() {
        let slot = FIRST_EDGE + u8::try_from(index).expect("fixture edge index");
        frame(
            &mut bytes,
            0x03,
            0x5e,
            &[
                1,
                child(slot, FIRST_VERTEX + start),
                child(slot, FIRST_VERTEX + end),
                1,
                1,
                1,
            ],
        );
    }
    for _ in vertices {
        frame(&mut bytes, 0x03, 0x5d, &[1]);
    }
    standard_catpart_from_streams(&bytes, &[])
}
