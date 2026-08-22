// SPDX-License-Identifier: Apache-2.0
//! e5-family synthetic stream and CATPart builders.

#![allow(clippy::unwrap_used)]
use super::{be32, descriptor, le_f32, le_f64};
use crate::container::{DIR_MAGIC, OUTER_MAGIC};

pub(crate) fn e5_circle_stream() -> Vec<u8> {
    let mut record = vec![0u8; 113];
    record[..3].copy_from_slice(&[0xe5, 0x0d, 0x03]);
    record[3] = 0xc9;
    record[5..7].copy_from_slice(&100u16.to_le_bytes());
    let write = |record: &mut [u8], at: usize, value: f64| {
        record[at..at + 8].copy_from_slice(&le_f64(value));
    };
    for (at, value) in [
        (14, 10.0),
        (22, 20.0),
        (30, 30.0),
        (38, 1.0),
        (70, 1.0),
        (86, 2.5),
    ] {
        write(&mut record, at, value);
    }
    let mut edge = vec![0u8; 19];
    edge[..3].copy_from_slice(&[0xe5, 0x0d, 0x03]);
    edge[3] = 0xff;
    edge[5..7].copy_from_slice(&6u16.to_le_bytes());
    edge[13..19].copy_from_slice(&[0x85, 0x80, 0x81, 0x82, 0x80, 0x80]);
    record.extend_from_slice(&edge);
    for xyz in [[12.5f32, 20.0, 30.0], [7.5, 20.0, 30.0]] {
        record.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in xyz {
            record.extend_from_slice(&le_f32(value));
        }
    }
    record
}

pub(crate) fn e5_torus_stream() -> Vec<u8> {
    let mut record = vec![0u8; 143];
    record[..3].copy_from_slice(&[0xe5, 0x0d, 0x03]);
    record[3] = 0xcc;
    record[5..7].copy_from_slice(&130u16.to_le_bytes());
    let write = |record: &mut [u8], at: usize, value: f64| {
        record[at..at + 8].copy_from_slice(&le_f64(value));
    };
    for (at, value) in [
        (14, 1.0),
        (22, 2.0),
        (30, 3.0),
        (38, 1.0),
        (102, 1.0),
        (110, 12.0),
        (118, 2.0),
    ] {
        write(&mut record, at, value);
    }
    record
}

pub(crate) fn e5_plane_stream() -> Vec<u8> {
    e5_plane_stream_with_transform_scalars(4)
}

pub(crate) fn e5_plane_stream_with_transform_scalars(scalar_count: usize) -> Vec<u8> {
    let mut payload = vec![0u8; 58 + 8 * scalar_count];
    for (index, value) in [1.0f64, 2.0, 3.0].into_iter().enumerate() {
        payload[1 + 8 * index..9 + 8 * index].copy_from_slice(&le_f64(value));
    }
    payload[25] = 0x33;
    for index in 0..scalar_count {
        payload[26 + 8 * index..34 + 8 * index].copy_from_slice(&le_f64(1.0));
    }
    for (index, value) in [-4.0f64, 7.0, -2.0, 9.0].into_iter().enumerate() {
        let at = 26 + 8 * scalar_count + 8 * index;
        payload[at..at + 8].copy_from_slice(&le_f64(value));
    }
    let mut bytes = Vec::new();
    append_e5_record(&mut bytes, 0xc8, 42, &payload);
    bytes
}

pub(crate) fn e5_catpart() -> Vec<u8> {
    let mut main = e5_circle_stream();
    for id in 2..=10 {
        append_e5_record(&mut main, 0xfe, id, &[]);
    }
    let surf = vec![0u8];
    let main_off = 16u32;
    let surf_off = main_off + main.len() as u32;
    let dir_rel = surf_off + surf.len() as u32;
    let mut dir = Vec::new();
    dir.extend_from_slice(DIR_MAGIC);
    dir.extend_from_slice(&descriptor("MainDataStream", main_off, main.len() as u32));
    dir.extend_from_slice(&descriptor("SurfacicReps", surf_off, surf.len() as u32));
    dir.extend_from_slice(b"CB__END");
    let mut inner = Vec::new();
    inner.extend_from_slice(OUTER_MAGIC);
    inner.extend_from_slice(&be32(dir_rel));
    inner.extend_from_slice(&be32(dir.len() as u32));
    inner.extend_from_slice(&main);
    inner.extend_from_slice(&surf);
    inner.extend_from_slice(&dir);
    let mut file = Vec::new();
    file.extend_from_slice(OUTER_MAGIC);
    file.extend_from_slice(&be32(16 + inner.len() as u32));
    file.extend_from_slice(&be32(0));
    file.extend_from_slice(&inner);
    file
}

pub(crate) fn append_e5_record(bytes: &mut Vec<u8>, class: u8, id: u32, payload: &[u8]) {
    bytes.extend_from_slice(&[0xe5, 0x0d, 0x03, class, 0]);
    bytes.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(payload);
}

pub(crate) fn e5_d8_rolling_ball_stream() -> Vec<u8> {
    let mut payload = vec![0x80];
    payload.extend_from_slice(&2_u32.to_le_bytes());
    payload.extend_from_slice(&5_u32.to_le_bytes());
    payload.extend_from_slice(&[0; 8]);
    payload.extend_from_slice(&2_u32.to_le_bytes());
    payload.extend_from_slice(&[0; 4]);
    for knot in [2.0_f64, 5.0] {
        payload.extend_from_slice(&knot.to_le_bytes());
    }
    for multiplicity in [6_u32, 6] {
        payload.extend_from_slice(&multiplicity.to_le_bytes());
    }
    for row in [
        [
            2.0_f64,
            0.0,
            0.0,
            0.0,
            2.0,
            0.0,
            0.0,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
        ],
        [
            3.0_f64,
            0.0,
            0.0,
            1.0,
            2.0,
            0.0,
            1.0,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
        ],
    ] {
        for value in row {
            payload.extend_from_slice(&value.to_le_bytes());
        }
    }
    for _ in 0..4 {
        payload.extend_from_slice(&[0; 80]);
    }
    payload.extend_from_slice(&2.0_f64.to_le_bytes());
    payload.extend_from_slice(&5.0_f64.to_le_bytes());
    payload.extend_from_slice(&0.0_f64.to_le_bytes());
    payload.extend_from_slice(&2.0_f64.to_le_bytes());
    payload.extend_from_slice(&2.0_f64.to_le_bytes());
    payload.extend_from_slice(&(-1_i32).to_le_bytes());
    payload.extend_from_slice(&0.0_f64.to_le_bytes());
    payload.extend_from_slice(&2.0_f64.to_le_bytes());
    payload.extend_from_slice(&[1, 0, 0]);
    let mut stream = Vec::new();
    append_e5_record(&mut stream, 0xd8, 42, &payload);
    stream
}

pub(crate) fn e5_uv_line_payload(surface: u16, offset: f64) -> Vec<u8> {
    let mut payload = vec![0x81, 0x18];
    payload.extend_from_slice(&surface.to_le_bytes());
    for value in [offset, 0.0, 1.0, 0.0, -1.0, 1.0] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload
}

pub(crate) fn e5_torus_topology_stream() -> Vec<u8> {
    let mut bytes = Vec::new();

    let mut torus = vec![0; 130];
    for (offset, value) in [
        (1, 0.0),
        (9, 0.0),
        (17, 0.0),
        (25, 1.0),
        (33, 0.0),
        (41, 0.0),
        (73, 0.0),
        (81, 0.0),
        (89, 1.0),
        (97, 10.0),
        (105, 2.0),
    ] {
        torus[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    append_e5_record(&mut bytes, 0xcc, 50, &torus);

    for id in [10u32, 20, 30, 40] {
        append_e5_record(&mut bytes, 0xfe, id, &[]);
    }

    let raw_corners = [
        [0.0, 0.0],
        [5.0 * std::f64::consts::PI, std::f64::consts::FRAC_PI_2],
        [5.0 * std::f64::consts::PI, std::f64::consts::PI],
        [0.0, std::f64::consts::PI],
    ];
    for index in 0..4 {
        let start = raw_corners[index];
        let end = raw_corners[(index + 1) % 4];
        let mut payload = vec![0x81, 0xb2];
        for value in [
            start[0],
            start[1],
            end[0] - start[0],
            end[1] - start[1],
            0.0,
            1.0,
        ] {
            payload.extend_from_slice(&le_f64(value));
        }
        append_e5_record(&mut bytes, 0x96, 60 + index as u32, &payload);

        let mut support = vec![0x81, 0xbc + index as u8, 0x81, 0, 0];
        support.extend_from_slice(&le_f64(0.0));
        support.extend_from_slice(&le_f64(1.0));
        append_e5_record(&mut bytes, 0xc0, 70 + index as u32, &support);
    }

    let mut bound_payload = vec![0x84, 0xbc, 0xbd, 0xbe, 0xbf, 0x84];
    for parameter in [0.0_f64, 1.0, 0.0, 1.0] {
        bound_payload.extend_from_slice(&le_f64(parameter));
        bound_payload.extend_from_slice(&0_u32.to_le_bytes());
    }
    append_e5_record(&mut bytes, 0x0e, 0, &bound_payload);

    for (index, (start, end)) in [(10u8, 20u8), (20, 30), (30, 40), (40, 10)]
        .into_iter()
        .enumerate()
    {
        append_e5_record(
            &mut bytes,
            0xff,
            80 + index as u32,
            &[
                0x85,
                0xc6 + index as u8,
                0x80 + start,
                0x80 + end,
                0x80,
                0x80,
                0x80,
            ],
        );
    }

    let mut loop_payload = vec![0x89];
    for index in 0..4 {
        loop_payload.extend_from_slice(&[0xbc + index, 0xd0 + index]);
    }
    loop_payload.push(0xb2);
    append_e5_record(&mut bytes, 0x09, 90, &loop_payload);
    append_e5_record(&mut bytes, 0x00, 91, &[0x82, 0xb2, 0xda, 1, 0]);
    append_e5_record(&mut bytes, 0x08, 92, &[0x81, 0xdb, 0x81, 1, 0, 1, 0, 1, 0]);
    append_e5_record(&mut bytes, 0x01, 93, &[0x81, 0xdc]);

    for xyz in [
        [12.0f32, 0.0, 0.0],
        [
            0.0,
            10.0 + std::f32::consts::SQRT_2,
            std::f32::consts::SQRT_2,
        ],
        [0.0, 10.0, 2.0],
        [10.0, 0.0, 2.0],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in xyz {
            bytes.extend_from_slice(&le_f32(value));
        }
    }
    bytes
}
