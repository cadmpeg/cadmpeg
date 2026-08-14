// SPDX-License-Identifier: Apache-2.0
//! Zero-entity-family synthetic stream builders.

#![allow(clippy::unwrap_used)]

pub(crate) fn zero_entity_support_stream() -> Vec<u8> {
    let mut plane = vec![0u8; 0x6a + 12];
    plane[..4].copy_from_slice(&[0xa9, 0x03, 0x27, 0x6a]);
    for (offset, value) in [
        (14, 1.0f64),
        (22, 2.0),
        (30, 3.0),
        (38, 1.0),
        (46, 0.0),
        (54, 0.0),
        (62, 0.0),
        (70, 1.0),
        (78, 0.0),
    ] {
        plane[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut support = vec![0u8; 0x71 + 12];
    support[..4].copy_from_slice(&[0xa9, 0x03, 0x21, 0x71]);
    support[12] = 0x10;
    support[13..17].copy_from_slice(&42u32.to_le_bytes());
    support[67..75].copy_from_slice(&0.0f64.to_le_bytes());
    support[75..83].copy_from_slice(&1.0f64.to_le_bytes());
    support[83] = 0x10;
    support[84..88].copy_from_slice(&2u32.to_le_bytes());
    support[88] = 0x10;
    support[89..93].copy_from_slice(&2u32.to_le_bytes());
    for (offset, value) in [(93, -2.0f64), (101, 4.0), (109, 6.0), (117, 8.0)] {
        support[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    plane.extend(support);
    plane
}

pub(crate) fn zero_entity_face_support_stream() -> Vec<u8> {
    let mut stream = zero_entity_support_stream();
    let mut face = vec![0u8; 0x0c + 12];
    face[..4].copy_from_slice(&[0xa9, 0x03, 0x5f, 0x0c]);
    face[7] = 0x10;
    face[8..12].copy_from_slice(&1u32.to_le_bytes());
    face[12] = 0x82;
    face[13] = 0x10;
    face[14..18].copy_from_slice(&10u32.to_le_bytes());
    face[18] = 0x10;
    face[19..23].copy_from_slice(&3u32.to_le_bytes());
    face[23] = 0x05;
    stream.extend(face);
    stream
}

pub(crate) fn zero_entity_face_loop_support_stream() -> Vec<u8> {
    let mut stream = zero_entity_face_support_stream();
    let mut loop_record = vec![0u8; 0x14 + 12];
    loop_record[..4].copy_from_slice(&[0xa9, 0x03, 0x62, 0x14]);
    loop_record[12] = 0x83;
    for (index, value) in [6u32, 1, 7].into_iter().enumerate() {
        let offset = 13 + index * 5;
        loop_record[offset] = 0x10;
        loop_record[offset + 1..offset + 5].copy_from_slice(&value.to_le_bytes());
    }
    loop_record[28..].copy_from_slice(&[0x81, 0x41, 0x07, 0x01]);
    stream.extend(loop_record);
    stream
}

pub(crate) fn zero_entity_topology_stream() -> Vec<u8> {
    let write_tagged_u32 = |record: &mut [u8], at: usize, value: u32| {
        record[at] = 0x10;
        record[at + 1..at + 5].copy_from_slice(&value.to_le_bytes());
    };
    let mut edge_stride = vec![0u8; 38];
    edge_stride[..4].copy_from_slice(&[0xa9, 0x03, 0x5e, 0x1a]);
    for (index, value) in [1, 5, 7, 8, 4, 3].into_iter().enumerate() {
        write_tagged_u32(&mut edge_stride, 7 + index * 5, value);
    }
    edge_stride[37] = 0x21;

    let mut header = vec![0u8; 0x69 + 12];
    header[..4].copy_from_slice(&[0xa9, 0x03, 0x25, 0x69]);
    write_tagged_u32(&mut header, 7, 1);
    header[12] = 0x82;
    write_tagged_u32(&mut header, 13, 100);
    write_tagged_u32(&mut header, 18, 200);

    let make_use = |side, allocations: [u32; 2]| {
        let mut record = vec![0u8; 0x38 + 12];
        record[..4].copy_from_slice(&[0xa9, 0x03, 0x06, 0x38]);
        write_tagged_u32(&mut record, 7, 1);
        record[12] = 0x83;
        write_tagged_u32(&mut record, 13, side);
        write_tagged_u32(&mut record, 18, allocations[0]);
        write_tagged_u32(&mut record, 23, allocations[1]);
        record
    };

    let mut incidence = vec![0u8; 0x10 + 12];
    incidence[..4].copy_from_slice(&[0xa9, 0x03, 0x05, 0x10]);
    write_tagged_u32(&mut incidence, 7, 1);
    incidence[12] = 0x83;
    for (index, value) in [1, 2, 5].into_iter().enumerate() {
        write_tagged_u32(&mut incidence, 13 + index * 5, value);
    }

    let mut vertex = vec![0u8; 0x06 + 12];
    vertex[..4].copy_from_slice(&[0xa9, 0x03, 0x5d, 0x06]);
    write_tagged_u32(&mut vertex, 7, 1);
    write_tagged_u32(&mut vertex, 12, 1);
    let mut support0 = vec![0u8; 0x18 + 12];
    support0[..4].copy_from_slice(&[0xa9, 0x03, 0x21, 0x18]);
    let mut support1 = vec![0u8; 0x18 + 12];
    support1[..4].copy_from_slice(&[0xa9, 0x03, 0x21, 0x18]);

    edge_stride
        .into_iter()
        .chain(header)
        .chain(make_use(1, [101, 201]))
        .chain(make_use(2, [102, 202]))
        .chain(incidence)
        .chain(vertex)
        .chain(support0)
        .chain(support1)
        .collect()
}

pub(crate) fn zero_entity_ownership_stream(face_count: u8) -> Vec<u8> {
    assert!(face_count != 0 && face_count < 0x80);
    let write_tagged_u32 = |record: &mut Vec<u8>, value: u32| {
        record.push(0x10);
        record.extend_from_slice(&value.to_le_bytes());
    };
    let mut face_roster = vec![0xa9, 0x03, 0x61, 0x42, 0, 0, 0];
    write_tagged_u32(&mut face_roster, 1);
    face_roster.push(0x80 + face_count);
    for slot in (1..=u32::from(face_count)).rev() {
        write_tagged_u32(&mut face_roster, slot);
    }
    face_roster.extend_from_slice(&[0x00, 0x01, 0xc0, 0xff, 0xff, 0x3f, 0, 0, 0, 0, 0x03]);

    let mut shell = vec![0xa9, 0x03, 0x60, 0x06, 0, 0, 0];
    write_tagged_u32(&mut shell, 1);
    shell.push(0x81);
    write_tagged_u32(&mut shell, 1);

    let mut body = vec![0xa9, 0x03, 0x65, 0x08, 0, 0, 0];
    write_tagged_u32(&mut body, 1);
    body.push(0x81);
    write_tagged_u32(&mut body, 1);
    body.extend_from_slice(&[0x05, 0x0d]);

    face_roster.into_iter().chain(shell).chain(body).collect()
}
