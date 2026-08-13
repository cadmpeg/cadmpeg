// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for the crate's `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.prt` byte images and embedded-stream payloads. They
//! construct raw bytes only; no native record type crosses in here.
#![allow(clippy::unwrap_used)]

use super::*;

/// Shared `PS`-signatured deltas-stream transmit preamble used by the deltas
/// fixture builders.
pub(crate) const DELTAS_PREAMBLE: &[u8] =
    b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00";

/// Append `count` deltas topology references, each the placeholder index `1`
/// followed by a set status byte, matching the deltas record framing.
pub(crate) fn push_reference_run(record: &mut Vec<u8>, count: usize) {
    for _ in 0..count {
        record.extend_from_slice(&1u16.to_be_bytes());
        record.push(1);
    }
}

pub(crate) fn status_framed_deltas_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    let mut face = Vec::new();
    face.extend_from_slice(&14u16.to_be_bytes());
    face.extend_from_slice(&100u16.to_be_bytes());
    face.extend_from_slice(&7u32.to_be_bytes());
    let push_ref = |record: &mut Vec<u8>, reference: u16| {
        record.extend_from_slice(&reference.to_be_bytes());
        record.push(1);
    };
    push_ref(&mut face, 1);
    face.extend_from_slice(&(-31_415_800_000_000.0f64).to_be_bytes());
    push_reference_run(&mut face, 5);
    face.push(b'+');
    push_reference_run(&mut face, 5);
    stream.extend(face);
    stream.extend_from_slice(&16u16.to_be_bytes());
    stream.extend_from_slice(&50_000u16.to_be_bytes());
    stream.extend_from_slice(&[0, 1]);
    stream
}

pub(crate) fn variable_status_framed_deltas_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&15u16.to_be_bytes());
    stream.extend_from_slice(&(-100i16).to_be_bytes());
    stream.extend_from_slice(&0u16.to_be_bytes());
    stream.extend_from_slice(&8u32.to_be_bytes());
    for reference in [1u16, 2, 3, 4] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.extend_from_slice(&17u16.to_be_bytes());
    stream.extend_from_slice(&101u16.to_be_bytes());
    stream.extend_from_slice(&9u32.to_be_bytes());
    for reference in [1u16, 2] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

pub(crate) fn status_framed_deltas_point_stream() -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(&29u16.to_be_bytes());
    stream.extend_from_slice(&50u16.to_be_bytes());
    stream.extend_from_slice(&900u32.to_be_bytes());
    for reference in [1u16; 4] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    for value in [0.0125f64, -0.002, 0.004] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

pub(crate) fn status_framed_deltas_intersection_stream() -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(&38u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&901u32.to_be_bytes());
    for reference in [1u16, 2, 3, 4, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for reference in [6u16, 7, 20, 21, 22, 23] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

pub(crate) fn deltas_point_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend(status_framed_deltas_point_stream());
    stream
}

pub(crate) fn deltas_edge_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&16u16.to_be_bytes());
    stream.extend_from_slice(&8u16.to_be_bytes());
    stream.extend_from_slice(&901u32.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream.extend_from_slice(&0.000_9f64.to_be_bytes());
    for reference in [7u16, 1, 1, 9, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

pub(crate) fn deltas_face_vertex_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&14u16.to_be_bytes());
    stream.extend_from_slice(&4u16.to_be_bytes());
    stream.extend_from_slice(&902u32.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream.extend_from_slice(&0.000_8f64.to_be_bytes());
    for reference in [1u16, 1, 5, 3, 6] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    push_reference_run(&mut stream, 5);

    stream.extend_from_slice(&18u16.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&903u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 11] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.extend_from_slice(&0.000_7f64.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream
}

pub(crate) fn deltas_loop_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&15u16.to_be_bytes());
    stream.extend_from_slice(&5u16.to_be_bytes());
    stream.extend_from_slice(&904u32.to_be_bytes());
    for reference in [1u16, 7, 4, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

pub(crate) fn deltas_shell_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&13u16.to_be_bytes());
    stream.extend_from_slice(&3u16.to_be_bytes());
    stream.extend_from_slice(&905u32.to_be_bytes());
    for reference in [1u16, 2, 1, 4, 1, 1, 12, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

pub(crate) fn deltas_fin_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&17u16.to_be_bytes());
    stream.extend_from_slice(&7u16.to_be_bytes());
    for reference in [1u16, 5, 7, 7, 10, 1, 8, 9, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'-');
    stream
}

/// Build a deltas analytic-surface partition record: the shared transmit
/// preamble, a `type`/`xmt`/`node_id` header, a five-reference run, the `+`
/// status marker, and the shape's big-endian `f64` payload values.
pub(crate) fn deltas_analytic_partition_stream(
    type_code: u16,
    xmt: u16,
    node_id: u32,
    values: &[f64],
) -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&type_code.to_be_bytes());
    stream.extend_from_slice(&xmt.to_be_bytes());
    stream.extend_from_slice(&node_id.to_be_bytes());
    push_reference_run(&mut stream, 5);
    stream.push(b'+');
    for value in values {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

pub(crate) fn deltas_line_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(30, 9, 906, &[0.004, 0.005, 0.006, 0.0, 1.0, 0.0])
}

pub(crate) fn deltas_plane_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        50,
        6,
        907,
        &[0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0],
    )
}

pub(crate) fn deltas_offset_surface_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&60u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&907u32.to_be_bytes());
    push_reference_run(&mut stream, 5);
    stream.push(b'+');
    stream.extend_from_slice(b"V");
    stream.push(1);
    stream.extend_from_slice(&6u16.to_be_bytes());
    stream.push(1);
    stream.extend_from_slice(&0.004_5f64.to_be_bytes());
    stream.extend_from_slice(&0xc2bc_928f_996e_0000u64.to_be_bytes());
    stream
}

pub(crate) fn status_frame_compact_references(
    mut record: Vec<u8>,
    reference_offsets: &[usize],
) -> Vec<u8> {
    for &offset in reference_offsets.iter().rev() {
        record.insert(offset + 2, 1);
    }
    record
}

pub(crate) fn deltas_stream_with_record(record: Vec<u8>) -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend(record);
    stream
}

pub(crate) fn deltas_blend_surface_partition_stream() -> Vec<u8> {
    let mut blend = record(56, 66);
    put_ref(&mut blend, 2, 12);
    blend[4..8].copy_from_slice(&908u32.to_be_bytes());
    for at in [8, 10, 12, 14, 16] {
        put_ref(&mut blend, at, 1);
    }
    blend[18] = b'+';
    blend[19] = b'R';
    put_ref(&mut blend, 20, 6);
    put_ref(&mut blend, 22, 6);
    put_ref(&mut blend, 24, 1);
    put_f64(&mut blend, 26, -0.004);
    put_f64(&mut blend, 34, 0.004);
    put_f64(&mut blend, 42, 1.0);
    put_f64(&mut blend, 50, 1.0);
    for at in [58, 60, 62, 64] {
        put_ref(&mut blend, at, 1);
    }
    deltas_stream_with_record(status_frame_compact_references(
        blend,
        &[8, 10, 12, 14, 16, 20, 22, 24, 58, 60, 62, 64],
    ))
}

pub(crate) fn deltas_trimmed_curve_partition_stream() -> Vec<u8> {
    let mut trim = record(133, 85);
    put_ref(&mut trim, 2, 12);
    trim[4..8].copy_from_slice(&909u32.to_be_bytes());
    for at in [8, 10, 12, 14, 16] {
        put_ref(&mut trim, at, 1);
    }
    trim[18] = b'+';
    put_ref(&mut trim, 19, 9);
    put_f64(&mut trim, 69, 0.000_3);
    put_f64(&mut trim, 77, 0.000_7);
    deltas_stream_with_record(status_frame_compact_references(
        trim,
        &[8, 10, 12, 14, 16, 19],
    ))
}

pub(crate) fn deltas_surface_curve_partition_stream() -> Vec<u8> {
    let mut surface_curve = record(137, 33);
    put_ref(&mut surface_curve, 2, 12);
    surface_curve[4..8].copy_from_slice(&910u32.to_be_bytes());
    for at in [8, 10, 12, 14, 16] {
        put_ref(&mut surface_curve, at, 1);
    }
    surface_curve[18] = b'+';
    put_ref(&mut surface_curve, 19, 6);
    put_ref(&mut surface_curve, 21, 9);
    put_ref(&mut surface_curve, 23, 9);
    put_f64(&mut surface_curve, 25, 0.000_02);
    deltas_stream_with_record(status_frame_compact_references(
        surface_curve,
        &[8, 10, 12, 14, 16, 19, 21, 23],
    ))
}

/// Point the single partition face record at geometry reference `reference`.
pub(crate) fn link_partition_face(stream: &mut [u8], reference: u16) {
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face record");
    put_ref(stream, face + 26, reference);
}

/// Point both the edge and fin topology records at geometry reference
/// `reference`.
pub(crate) fn link_partition_edge_and_fin(stream: &mut [u8], reference: u16) {
    for (kind, xmt, field) in [(16u8, 8u8, 24usize), (17, 7, 18)] {
        let record = stream
            .windows(4)
            .position(|window| window == [0, kind, 0, xmt])
            .expect("topology record");
        put_ref(stream, record + field, reference);
    }
}

pub(crate) fn circle_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_edge_and_fin(&mut stream, 12);
    let mut circle = record(31, 99);
    put_ref(&mut circle, 2, 12);
    circle[18] = b'+';
    put_vec3(&mut circle, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut circle, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut circle, 67, [1.0, 0.0, 0.0]);
    put_f64(&mut circle, 91, 0.01);
    stream.extend(circle);
    stream
}

pub(crate) fn deltas_circle_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        31,
        12,
        908,
        &[0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.025],
    )
}

pub(crate) fn ellipse_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_edge_and_fin(&mut stream, 13);
    let mut ellipse = record(32, 107);
    put_ref(&mut ellipse, 2, 13);
    ellipse[18] = b'+';
    put_vec3(&mut ellipse, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut ellipse, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut ellipse, 67, [1.0, 0.0, 0.0]);
    put_f64(&mut ellipse, 91, 0.02);
    put_f64(&mut ellipse, 99, 0.01);
    stream.extend(ellipse);
    stream
}

pub(crate) fn deltas_ellipse_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        32,
        13,
        909,
        &[
            0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.03, 0.012,
        ],
    )
}

pub(crate) fn cylinder_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);
    let mut cylinder = record(51, 99);
    put_ref(&mut cylinder, 2, 12);
    cylinder[18] = b'+';
    put_vec3(&mut cylinder, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cylinder, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cylinder, 67, 0.01);
    put_vec3(&mut cylinder, 75, [1.0, 0.0, 0.0]);
    stream.extend(cylinder);
    stream
}

pub(crate) fn deltas_cylinder_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        51,
        12,
        910,
        &[0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 0.025, 1.0, 0.0, 0.0],
    )
}

pub(crate) fn cone_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);
    let mut cone = record(52, 115);
    put_ref(&mut cone, 2, 12);
    cone[18] = b'+';
    put_vec3(&mut cone, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cone, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cone, 67, 0.01);
    put_f64(&mut cone, 75, 0.0);
    put_f64(&mut cone, 83, 1.0);
    put_vec3(&mut cone, 91, [1.0, 0.0, 0.0]);
    stream.extend(cone);
    stream
}

pub(crate) fn deltas_cone_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        52,
        12,
        911,
        &[
            0.001,
            0.002,
            0.003,
            0.0,
            1.0,
            0.0,
            0.025,
            0.5,
            3.0f64.sqrt() / 2.0,
            1.0,
            0.0,
            0.0,
        ],
    )
}

pub(crate) fn sphere_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);
    let mut sphere = record(53, 99);
    put_ref(&mut sphere, 2, 12);
    sphere[18] = b'+';
    put_vec3(&mut sphere, 19, [0.0, 0.0, 0.0]);
    put_f64(&mut sphere, 43, 0.01);
    put_vec3(&mut sphere, 51, [0.0, 0.0, 1.0]);
    put_vec3(&mut sphere, 75, [1.0, 0.0, 0.0]);
    stream.extend(sphere);
    stream
}

pub(crate) fn deltas_sphere_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        53,
        12,
        912,
        &[0.001, 0.002, 0.003, 0.025, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0],
    )
}

pub(crate) fn torus_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);
    let mut torus = record(54, 107);
    put_ref(&mut torus, 2, 12);
    torus[18] = b'+';
    put_vec3(&mut torus, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut torus, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut torus, 67, 0.03);
    put_f64(&mut torus, 75, 0.01);
    put_vec3(&mut torus, 83, [1.0, 0.0, 0.0]);
    stream.extend(torus);
    stream
}

pub(crate) fn deltas_torus_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        54,
        12,
        913,
        &[
            0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 0.04, 0.015, 1.0, 0.0, 0.0,
        ],
    )
}

pub(crate) fn bspline_partition_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00XX: TRANSMIT FILE (partition)\x00SCH_TEST_1_9999\x00");
    let mut surface = record(124, 23);
    put_ref(&mut surface, 2, 10);
    surface[18] = b'+';
    put_ref(&mut surface, 19, 20);
    put_ref(&mut surface, 21, 21);
    s.extend(surface);

    let mut descriptor = record(126, 48);
    put_ref(&mut descriptor, 2, 20);
    put_ref(&mut descriptor, 6, 1);
    put_ref(&mut descriptor, 8, 1);
    put_ref(&mut descriptor, 12, 2);
    put_ref(&mut descriptor, 16, 2);
    descriptor[18] = 5;
    descriptor[19] = 5;
    descriptor[20..24].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut descriptor, 36, 30);
    put_ref(&mut descriptor, 38, 31);
    put_ref(&mut descriptor, 40, 32);
    put_ref(&mut descriptor, 42, 33);
    put_ref(&mut descriptor, 44, 125);
    put_ref(&mut descriptor, 46, 21);
    s.extend(descriptor);

    let mut data = record(125, 97 + 12 * 8);
    put_ref(&mut data, 2, 21);
    data[90] = b'+';
    data[91..95].copy_from_slice(&12u32.to_be_bytes());
    for (index, value) in [
        0.0, 0.0, 0.0, 0.0, 0.02, 0.0, 0.01, 0.0, 0.0, 0.01, 0.02, 0.0,
    ]
    .into_iter()
    .enumerate()
    {
        put_f64(&mut data, 97 + index * 8, value);
    }
    s.extend(data);

    for (tag, reference, values) in [(127, 30, vec![2u16, 2]), (127, 31, vec![2, 2])] {
        let mut array = record(tag, 8 + values.len() * 2);
        array[4..6].copy_from_slice(&(values.len() as u16).to_be_bytes());
        put_ref(&mut array, 6, reference);
        for (index, value) in values.into_iter().enumerate() {
            put_ref(&mut array, 8 + index * 2, value);
        }
        s.extend(array);
    }
    for reference in [32, 33] {
        let mut array = record(128, 8 + 2 * 8);
        array[4..6].copy_from_slice(&2u16.to_be_bytes());
        put_ref(&mut array, 6, reference);
        put_f64(&mut array, 8, 0.0);
        put_f64(&mut array, 16, 1.0);
        s.extend(array);
    }

    let mut curve = record(134, 23);
    put_ref(&mut curve, 2, 50);
    curve[18] = b'+';
    put_ref(&mut curve, 19, 40);
    put_ref(&mut curve, 21, 41);
    s.extend(curve);
    let mut curve_descriptor = record(136, 27);
    put_ref(&mut curve_descriptor, 2, 40);
    put_ref(&mut curve_descriptor, 4, 1);
    put_ref(&mut curve_descriptor, 8, 2);
    put_ref(&mut curve_descriptor, 10, 3);
    put_ref(&mut curve_descriptor, 14, 2);
    curve_descriptor[16] = 5;
    put_ref(&mut curve_descriptor, 23, 42);
    put_ref(&mut curve_descriptor, 25, 43);
    s.extend(curve_descriptor);
    let mut curve_data = record(135, 15 + 6 * 8);
    put_ref(&mut curve_data, 2, 41);
    curve_data[9..13].copy_from_slice(&6u32.to_be_bytes());
    for (index, value) in [0.0, 0.0, 0.0, 0.02, 0.0, 0.0].into_iter().enumerate() {
        put_f64(&mut curve_data, 15 + index * 8, value);
    }
    s.extend(curve_data);
    for (tag, reference) in [(127, 42), (128, 43)] {
        let mut array = record(tag, if tag == 127 { 12 } else { 24 });
        array[4..6].copy_from_slice(&2u16.to_be_bytes());
        put_ref(&mut array, 6, reference);
        if tag == 127 {
            put_ref(&mut array, 8, 2);
            put_ref(&mut array, 10, 2);
        } else {
            put_f64(&mut array, 8, 0.0);
            put_f64(&mut array, 16, 1.0);
        }
        s.extend(array);
    }
    s
}

pub(crate) fn extended_bspline_surface_stream() -> Vec<u8> {
    let descriptor_ref = 40_000u32;
    let payload_ref = 40_001u32;
    let support_refs = [40_010u32, 40_011, 40_012, 40_013];

    let mut stream = Vec::new();
    let mut wrapper = record(124, 19);
    put_ref(&mut wrapper, 2, 10);
    wrapper[18] = b'+';
    stream.extend(wrapper);
    stream.extend(encoded_xmt(descriptor_ref));
    stream.extend(encoded_xmt(payload_ref));

    let xmt = encoded_xmt(descriptor_ref);
    let shift = xmt.len() - 2;
    let encoded_payload_ref = encoded_xmt(payload_ref);
    let mut descriptor = vec![0u8; 56 + shift + encoded_payload_ref.len()];
    descriptor[..2].copy_from_slice(&126u16.to_be_bytes());
    descriptor[2..2 + xmt.len()].copy_from_slice(&xmt);
    put_ref(&mut descriptor, 6 + shift, 1);
    put_ref(&mut descriptor, 8 + shift, 1);
    put_ref(&mut descriptor, 12 + shift, 2);
    put_ref(&mut descriptor, 16 + shift, 2);
    descriptor[18 + shift] = 5;
    descriptor[19 + shift] = 5;
    descriptor[20 + shift..24 + shift].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24 + shift..28 + shift].copy_from_slice(&2u32.to_be_bytes());
    let mut at = 34 + shift;
    for reference in [
        40_009,
        support_refs[0],
        support_refs[1],
        support_refs[2],
        support_refs[3],
    ] {
        let encoded = encoded_xmt(reference);
        descriptor[at..at + encoded.len()].copy_from_slice(&encoded);
        at += encoded.len();
    }
    assert_eq!(at, 54 + shift);
    put_ref(&mut descriptor, 54 + shift, 125);
    descriptor[56 + shift..].copy_from_slice(&encoded_payload_ref);
    stream.extend(descriptor);

    let xmt = encoded_xmt(payload_ref);
    let shift = xmt.len() - 2;
    let first = encoded_xmt(40_020);
    let data_at = 95 + shift + first.len();
    let mut payload = vec![0u8; data_at + 12 * 8];
    payload[..2].copy_from_slice(&125u16.to_be_bytes());
    payload[2..2 + xmt.len()].copy_from_slice(&xmt);
    payload[90 + shift] = b'+';
    payload[91 + shift..95 + shift].copy_from_slice(&12u32.to_be_bytes());
    payload[95 + shift..data_at].copy_from_slice(&first);
    for (index, value) in [
        0.0, 0.0, 0.0, 0.0, 0.02, 0.0, 0.01, 0.0, 0.0, 0.01, 0.02, 0.0,
    ]
    .into_iter()
    .enumerate()
    {
        put_f64(&mut payload, data_at + index * 8, value);
    }
    stream.extend(payload);

    for (tag, reference, values) in [
        (127, support_refs[0], vec![2u16, 2]),
        (127, support_refs[1], vec![2, 2]),
    ] {
        let reference = encoded_xmt(reference);
        let mut array = record(tag, 6 + reference.len() + values.len() * 2);
        array[4..6].copy_from_slice(&(values.len() as u16).to_be_bytes());
        array[6..6 + reference.len()].copy_from_slice(&reference);
        for (index, value) in values.into_iter().enumerate() {
            put_ref(&mut array, 6 + reference.len() + index * 2, value);
        }
        stream.extend(array);
    }
    for reference in [support_refs[2], support_refs[3]] {
        let reference = encoded_xmt(reference);
        let mut array = record(128, 6 + reference.len() + 16);
        array[4..6].copy_from_slice(&2u16.to_be_bytes());
        array[6..6 + reference.len()].copy_from_slice(&reference);
        put_f64(&mut array, 6 + reference.len(), 0.0);
        put_f64(&mut array, 14 + reference.len(), 1.0);
        stream.extend(array);
    }
    stream
}

pub(crate) fn bspline_surface_replacement_partition_stream() -> Vec<u8> {
    let mut stream = bspline_partition_stream();
    let mut descriptor = record(126, 48);
    put_ref(&mut descriptor, 2, 60);
    put_ref(&mut descriptor, 6, 1);
    put_ref(&mut descriptor, 8, 1);
    put_ref(&mut descriptor, 12, 2);
    put_ref(&mut descriptor, 16, 2);
    descriptor[18] = 5;
    descriptor[19] = 5;
    descriptor[20..24].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut descriptor, 36, 30);
    put_ref(&mut descriptor, 38, 31);
    put_ref(&mut descriptor, 40, 32);
    put_ref(&mut descriptor, 42, 33);
    put_ref(&mut descriptor, 44, 125);
    put_ref(&mut descriptor, 46, 61);
    stream.extend(descriptor);

    let mut data = record(125, 97 + 12 * 8);
    put_ref(&mut data, 2, 61);
    data[90] = b'+';
    data[91..95].copy_from_slice(&12u32.to_be_bytes());
    for (index, value) in [
        0.0, 0.0, 0.0, 0.0, 0.03, 0.0, 0.015, 0.0, 0.0, 0.015, 0.03, 0.0,
    ]
    .into_iter()
    .enumerate()
    {
        put_f64(&mut data, 97 + index * 8, value);
    }
    stream.extend(data);
    stream
}

pub(crate) fn deltas_bspline_surface_wrapper_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&124u16.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&914u32.to_be_bytes());
    push_reference_run(&mut stream, 5);
    stream.push(b'+');
    for reference in [60u16, 61] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

pub(crate) fn bspline_curve_replacement_partition_stream() -> Vec<u8> {
    let mut stream = bspline_partition_stream();
    let mut descriptor = record(136, 27);
    put_ref(&mut descriptor, 2, 70);
    put_ref(&mut descriptor, 4, 1);
    put_ref(&mut descriptor, 8, 2);
    put_ref(&mut descriptor, 10, 3);
    put_ref(&mut descriptor, 14, 2);
    descriptor[16] = 5;
    put_ref(&mut descriptor, 23, 42);
    put_ref(&mut descriptor, 25, 43);
    stream.extend(descriptor);

    let mut data = record(135, 15 + 6 * 8);
    put_ref(&mut data, 2, 71);
    data[9..13].copy_from_slice(&6u32.to_be_bytes());
    for (index, value) in [0.0, 0.0, 0.0, 0.02, 0.01, 0.0].into_iter().enumerate() {
        put_f64(&mut data, 15 + index * 8, value);
    }
    stream.extend(data);
    stream
}

pub(crate) fn deltas_bspline_curve_wrapper_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&134u16.to_be_bytes());
    stream.extend_from_slice(&50u16.to_be_bytes());
    stream.extend_from_slice(&915u32.to_be_bytes());
    push_reference_run(&mut stream, 5);
    stream.push(b'+');
    for reference in [70u16, 71] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

pub(crate) fn trimmed_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 12);
    let mut trim = record(133, 85);
    put_ref(&mut trim, 2, 12);
    trim[18] = b'+';
    put_ref(&mut trim, 19, 9);
    put_f64(&mut trim, 69, 0.000_25);
    put_f64(&mut trim, 77, 0.000_75);
    // The closed edge's single vertex sits at the trim range's midpoint on the
    // basis line so both trimmed endpoints fall inside the edge's stored
    // 0.3 mm tolerance; the point record is the topology stream's last
    // 40 bytes, before the trim record is appended.
    let point_vec = stream.len() - 40 + 16;
    put_vec3(&mut stream, point_vec, [0.000_5, 0.0, 0.0]);
    stream.extend(trim);
    stream
}

pub(crate) fn mismatched_trimmed_topology_partition_stream() -> Vec<u8> {
    let mut stream = trimmed_topology_partition_stream();
    let point_vec = stream.len() - 85 - 40 + 16;
    put_vec3(&mut stream, point_vec, [0.000_5, 0.01, 0.0]);
    stream
}

pub(crate) fn partnered_trimmed_topology_partition_stream() -> Vec<u8> {
    let mut stream = trimmed_topology_partition_stream();
    let trim = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("trimmed curve");
    put_f64(&mut stream, trim + 69, 0.000_75);
    put_f64(&mut stream, trim + 77, 0.000_25);
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("first face");
    put_ref(&mut stream, face + 18, 20);
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("first fin");
    put_ref(&mut stream, fin + 14, 22);
    let first_point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("first point");
    put_vec3(&mut stream, first_point + 16, [0.000_25, 0.0, 0.0]);

    let mut second_face = record(14, 39);
    put_ref(&mut second_face, 2, 20);
    put_f64(&mut second_face, 10, 0.000_2);
    put_ref(&mut second_face, 18, 1);
    put_ref(&mut second_face, 20, 4);
    put_ref(&mut second_face, 22, 21);
    put_ref(&mut second_face, 24, 3);
    put_ref(&mut second_face, 26, 6);
    second_face[28] = b'+';
    stream.extend(second_face);

    let mut second_loop = record(15, 16);
    put_ref(&mut second_loop, 2, 21);
    put_ref(&mut second_loop, 10, 22);
    put_ref(&mut second_loop, 12, 20);
    put_ref(&mut second_loop, 14, 1);
    stream.extend(second_loop);

    let mut second_fin = record(17, 23);
    put_ref(&mut second_fin, 2, 22);
    put_ref(&mut second_fin, 6, 21);
    put_ref(&mut second_fin, 8, 22);
    put_ref(&mut second_fin, 10, 22);
    put_ref(&mut second_fin, 12, 23);
    put_ref(&mut second_fin, 14, 7);
    put_ref(&mut second_fin, 16, 8);
    put_ref(&mut second_fin, 18, 1);
    second_fin[22] = b'-';
    stream.extend(second_fin);

    let mut second_vertex = record(18, 28);
    put_ref(&mut second_vertex, 2, 23);
    put_ref(&mut second_vertex, 16, 24);
    put_f64(&mut second_vertex, 18, 0.000_1);
    stream.extend(second_vertex);

    let mut second_point = record(29, 40);
    put_ref(&mut second_point, 2, 24);
    put_vec3(&mut second_point, 16, [0.000_75, 0.0, 0.0]);
    stream.extend(second_point);
    stream
}

pub(crate) fn forward_trimmed_curve_chain_stream() -> Vec<u8> {
    let mut stream = trimmed_topology_partition_stream();
    let first = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("first trimmed curve");
    put_ref(&mut stream, first + 19, 20);

    let mut second = record(133, 85);
    put_ref(&mut second, 2, 20);
    second[18] = b'+';
    put_ref(&mut second, 19, 9);
    put_f64(&mut second, 69, 0.000_25);
    put_f64(&mut second, 77, 0.000_75);
    stream.extend(second);
    stream
}

pub(crate) fn topology_with_extended_edge_curve_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    stream[edge + 24..edge + 26].copy_from_slice(&(-9i16).to_be_bytes());
    stream.splice(edge + 26..edge + 26, [0, 0]);
    stream
}

pub(crate) fn topology_with_extended_face_attribute_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face record");
    stream.splice(face + 8..face + 10, [0xff, 0xff, 0x00, 0x00]);
    stream
}

pub(crate) fn topology_with_extended_edge_attribute_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    stream.splice(edge + 8..edge + 10, [0xff, 0xff, 0x00, 0x00]);
    stream
}

pub(crate) fn topology_with_extended_internal_topology_references() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(13, 3, 8), (15, 5, 8), (17, 7, 4), (18, 10, 8), (29, 11, 8)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        stream.splice(
            record + offset..record + offset + 2,
            [0xff, 0xff, 0x00, 0x00],
        );
    }
    stream
}

pub(crate) fn topology_with_fully_extended_geometry_headers() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt) in [(50, 6), (30, 9)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("geometry record");
        for index in 0..5 {
            let at = record + 8 + index * 4;
            stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
        }
    }
    stream
}

pub(crate) fn topology_with_escaped_geometry_envelopes() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for marker in [[0, 50, 0, 6], [0, 30, 0, 9]] {
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("geometry record");
        stream.insert(record + 2, 0xff);
    }
    stream
}

pub(crate) fn offset_surface_with_fully_extended_common_header() -> Vec<u8> {
    let mut stream = offset_surface_topology_partition_stream();
    let record = stream
        .windows(4)
        .position(|window| window == [0, 60, 0, 12])
        .expect("offset record");
    for index in 0..5 {
        let at = record + 8 + index * 4;
        stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
    }
    stream
}

pub(crate) fn fully_extend_common_header(stream: &mut Vec<u8>, marker: [u8; 4]) {
    let record = stream
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("compact geometry record");
    for index in 0..5 {
        let at = record + 8 + index * 4;
        stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
    }
}

