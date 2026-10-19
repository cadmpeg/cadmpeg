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

/// Two oppositely oriented triangular faces with three source-closed physical edges.
pub(crate) fn zero_entity_allocation_stream() -> Vec<u8> {
    fn tagged(record: &mut [u8], offset: usize, value: u32) {
        record[offset] = 0x10;
        record[offset + 1..offset + 5].copy_from_slice(&value.to_le_bytes());
    }
    fn frame(tag: [u8; 2]) -> Vec<u8> {
        let mut record = vec![0; usize::from(tag[1]) + 12];
        record[..4].copy_from_slice(&[0xa9, 0x03, tag[0], tag[1]]);
        tagged(&mut record, 7, 1);
        record
    }
    let prototype = zero_entity_support_stream();
    let mut stream = Vec::new();
    for order in [[(0, 1), (1, 2), (2, 0)], [(2, 0), (1, 2), (0, 1)]] {
        stream.extend_from_slice(&prototype[..118]);
        for (index, (start, end)) in order.into_iter().enumerate() {
            let mut support = prototype[118..].to_vec();
            tagged(
                &mut support,
                12,
                u32::try_from(index + 2).expect("three slots"),
            );
            let uv = [[0_f64, 0.], [1., 0.], [0., 1.]];
            for (offset, point) in [(93, uv[start]), (109, uv[end])] {
                support[offset..offset + 8].copy_from_slice(&point[0].to_le_bytes());
                support[offset + 8..offset + 16].copy_from_slice(&point[1].to_le_bytes());
            }
            stream.extend(support);
        }
    }
    for base in [[72_u32, 49], [74, 53], [76, 57]] {
        let mut header = frame([0x25, 0x69]);
        header[12] = 0x82;
        tagged(&mut header, 13, base[0]);
        tagged(&mut header, 18, base[1]);
        stream.extend(header);
        for side in [1, 2] {
            let mut use_record = frame([0x06, 0x38]);
            use_record[12] = 0x83;
            tagged(&mut use_record, 13, side);
            tagged(&mut use_record, 18, base[0] + side);
            tagged(&mut use_record, 23, base[1] + side);
            stream.extend(use_record);
        }
    }
    for distances in [[8, 1], [9, 7], [8, 6]] {
        let mut incidence = frame([0x05, 0x0b]);
        incidence[12] = 0x82;
        tagged(&mut incidence, 13, distances[0]);
        tagged(&mut incidence, 18, distances[1]);
        stream.extend(incidence);
        let mut owner = frame([0x5d, 0x06]);
        tagged(&mut owner, 12, 1);
        stream.extend(owner);
    }
    for values in [[15, 5, 3, 14, 13], [13, 4, 2, 12, 11], [11, 3, 7, 10, 9]] {
        let mut stride = frame([0x5e, 0x1a]);
        for (index, value) in values.into_iter().enumerate() {
            tagged(&mut stride, 12 + index * 5, value);
        }
        stride[37] = 0x21;
        stream.extend(stride);
    }
    for (values, trailer) in [
        ([90, 3, 89, 2, 88, 1, 92], [0x83, 0x41, 0xff, 0x01, 0x01]),
        ([70, 2, 69, 3, 68, 4, 72], [0x83, 0xc1, 0x92, 0x00, 0x01]),
    ] {
        let mut loop_record = frame([0x62, 0x29]);
        loop_record[12] = 0x87;
        for (index, value) in values.into_iter().enumerate() {
            tagged(&mut loop_record, 13 + index * 5, value);
        }
        loop_record[48..].copy_from_slice(&trailer);
        stream.extend(loop_record);
    }
    for (first, control) in [(95, 5), (75, 3)] {
        let mut face = frame([0x5f, 0x0c]);
        face[12] = 0x82;
        tagged(&mut face, 13, first);
        tagged(&mut face, 18, 3);
        face[23] = control;
        stream.extend(face);
    }
    stream
}
