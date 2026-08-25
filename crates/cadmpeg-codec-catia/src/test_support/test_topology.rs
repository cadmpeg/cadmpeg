// SPDX-License-Identifier: Apache-2.0
//! Synthetic standard-family topology streams for fixture CATParts.

#![allow(clippy::unwrap_used)]
use super::{be_f32, le_f32};

pub(crate) fn standard_quad_topology_stream() -> Vec<u8> {
    let mut bytes = vec![0x01, 0x44, 0x01, 0xff, 10, 0, 0, 0, 10];
    for handle in [1u16, 10, 11, 12, 13, 14, 15, 16, 17, 10] {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }

    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    bytes.extend_from_slice(&[0x01, 0x01, 0x04]);
    for row in [
        [100u16, 11, 101],
        [101, 13, 102],
        [102, 15, 103],
        [103, 17, 100],
    ] {
        bytes.extend_from_slice(&[0x02, 0x03]);
        for handle in row {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    bytes.extend_from_slice(&[0x01, 0x06, 0x04]);
    for xyz in [
        [0.0f32, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in xyz {
            bytes.extend_from_slice(&le_f32(value));
        }
    }
    bytes
}

pub(crate) fn compact_standard_triangle_topology_stream() -> Vec<u8> {
    let mut bytes = vec![0x01, 0x41, 0x01, 0xff, 0x03, 0x00, 0x00, 0x00, 0, 1, 2];
    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    bytes.extend_from_slice(&[0x01, 0x01, 0x03]);
    for handles in [[0, 1], [1, 2], [2, 0]] {
        bytes.extend_from_slice(&[0x02, 0x02, handles[0], handles[1]]);
    }
    bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    bytes.extend_from_slice(&[0x01, 0x06, 0x03]);
    for xyz in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in xyz {
            bytes.extend_from_slice(&le_f32(value));
        }
    }
    bytes
}

pub(crate) fn fbb_only_quad_topology_stream() -> Vec<u8> {
    let standard = standard_quad_topology_stream();
    let fbb_start = standard
        .windows(4)
        .position(|marker| marker == [0x30, 0x04, 0x04, 0xff])
        .expect("FBB face row");
    let mut bytes = standard[..fbb_start + 8].to_vec();
    bytes[1] = 0x4c;
    let mut frame = Vec::new();
    for value in [0.0f32, 0.0, 1.0] {
        frame.extend_from_slice(&le_f32(value));
    }
    bytes.splice(8..8, frame);
    let delimiter = [0x10, 0xf4, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
    for (kind, rows) in [
        (1, [[10u16, 11, 12], [12, 13, 14]]),
        (2, [[14u16, 15, 16], [16, 17, 10]]),
    ] {
        bytes.extend_from_slice(&[0x01, kind, 2]);
        for row in rows {
            bytes.extend_from_slice(&[0x02, 3]);
            for handle in row {
                bytes.extend_from_slice(&handle.to_be_bytes());
            }
        }
        bytes.extend_from_slice(&delimiter);
    }
    bytes.extend_from_slice(&[0x01, 0x06, 0x04]);
    for xyz in [
        [0.0f32, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in xyz {
            bytes.extend_from_slice(&le_f32(value));
        }
    }
    bytes
}

pub(crate) fn fbb_mixed_boundary_topology_stream() -> Vec<u8> {
    let complete = fbb_only_quad_topology_stream();
    let table_start = complete
        .windows(5)
        .position(|window| window == [0x01, 0x01, 0x02, 0x02, 0x03])
        .expect("first FBB edge table");
    let vertex_start = complete
        .windows(3)
        .position(|marker| marker == [0x01, 0x06, 0x04])
        .expect("FBB vertex table");
    let mut bytes = complete[..table_start].to_vec();
    let delimiter = [0x10, 0xf4, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
    bytes.extend_from_slice(&[0x01, 0x01, 0x01, 0x02, 0x05]);
    for handle in [99u16, 11, 12, 13, 98] {
        bytes.extend_from_slice(&[handle.to_be_bytes()[0], handle.to_be_bytes()[1]]);
    }
    bytes.extend_from_slice(&delimiter);
    bytes.extend_from_slice(&[0x01, 0x02, 0x04]);
    for [start, end] in [[14u16, 15], [15, 16], [16, 17], [17, 10]] {
        bytes.extend_from_slice(&[0x02, 0x02]);
        bytes.extend_from_slice(&start.to_be_bytes());
        bytes.extend_from_slice(&end.to_be_bytes());
    }
    bytes.extend_from_slice(&delimiter);
    bytes.extend_from_slice(&complete[vertex_start..]);
    bytes
}

pub(crate) fn fbb_only_quad_unmatched_edge_topology_stream() -> Vec<u8> {
    let mut bytes = fbb_only_quad_topology_stream();
    let row_header = bytes
        .windows(5)
        .position(|window| window == [0x01, 0x01, 0x02, 0x02, 0x03])
        .expect("first FBB edge table");
    for (offset, handle) in (row_header + 5..row_header + 11).zip([0u8, 20, 0, 21, 0, 22]) {
        bytes[offset] = handle;
    }
    bytes
}

pub(crate) fn fbb_only_quad_surface_stream() -> Vec<u8> {
    let mut bytes = vec![0x11, 0x22, 0x33, 0x00, 0x02, 0x00, 0x33, 0x32];
    bytes.resize(49, 0);
    bytes[48] = 0x01;
    for (tag, center) in [
        (1u8, [0.5f32, 0.0]),
        (2, [1.0, 0.5]),
        (3, [0.5, 1.0]),
        (4, [0.0, 0.5]),
    ] {
        bytes.extend_from_slice(&[0x60, tag, 0, 0, 0x00, 0x12, 0x00, 0x33, 0x37]);
        for value in [center[0], center[1], 0.0, 0.5] {
            bytes.extend_from_slice(&be_f32(value));
        }
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.extend_from_slice(&[0xff, 0x11, 0x22, 0x33, 0x00, 0x02, 0x00, 0x33, 0x32]);
    for value in [0.5f32, 0.5, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 2.0] {
        bytes.extend_from_slice(&le_f32(value));
    }
    bytes
}
