// SPDX-License-Identifier: Apache-2.0
//! Authored JT 9.4 tetrahedron: scene shape, late-loaded property and mesh LOD.
#![allow(clippy::unwrap_used)]

use super::test_bytes::zlib_compress;
use super::test_prt::prt_with_named_payloads;

fn u32_bytes(value: usize) -> [u8; 4] {
    u32::try_from(value).unwrap().to_le_bytes()
}

fn packet(values: &[u32]) -> Vec<u8> {
    let mut out = u32_bytes(values.len()).to_vec();
    let Some(&low) = values.iter().min() else {
        return out;
    };
    let high = *values.iter().max().unwrap();
    let width = 32 - (high - low).leading_zeros();
    let mut bits = Vec::new();
    let mut append = |value: u32, width: u32| {
        for shift in (0..width).rev() {
            bits.push((value >> shift) & 1);
        }
    };
    append(0, 1);
    append(32, 6);
    append(32, 6);
    append(low, 32);
    append(high, 32);
    for value in values {
        append(value - low, width);
    }
    out.push(1); // Bit-length codec.
    out.extend(u32_bytes(bits.len()));
    for chunk in bits.chunks(32) {
        let word = chunk.iter().fold(0, |word, bit| (word << 1) | bit) << (32 - chunk.len());
        out.extend(word.to_le_bytes());
    }
    out
}

fn object_type(first: u32) -> [u8; 16] {
    let mut id = [0; 16];
    id[..4].copy_from_slice(&first.to_le_bytes());
    id[4..].copy_from_slice(&[
        0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59, 0x97,
    ]);
    id
}

fn element(kind: [u8; 16], base: u8, id: u32, body: &[u8]) -> Vec<u8> {
    let mut out = u32_bytes(21 + body.len()).to_vec();
    out.extend(kind);
    out.push(base);
    out.extend(id.to_le_bytes());
    out.extend(body);
    out
}

fn end_elements(out: &mut Vec<u8>) {
    out.extend(16_u32.to_le_bytes());
    out.extend([0xff; 16]);
}

fn mesh_lod() -> Vec<u8> {
    // Closed tetrahedron topology, using the public degree/valence grammar.
    let lanes: [&[u32]; 24] = [
        &[3, 3, 3],
        &[3],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[3, 3, 3, 3],
        &[10, 12, 11, 13],
        &[0, 0, 0, 0],
        &[],
        &[1, 1, 1, 1],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    ];
    let mut representation: Vec<u8> = lanes.into_iter().flat_map(packet).collect();
    representation.extend(0x1234_5678_u32.to_le_bytes());
    representation.extend(0_u64.to_le_bytes());
    representation.extend([8, 0, 0, 0]);
    representation.extend(4_u32.to_le_bytes());
    representation.extend(4_u32.to_le_bytes());
    representation.extend(4_u32.to_le_bytes());
    representation.push(3);
    for _ in 0..3 {
        representation.extend(0_f32.to_le_bytes());
        representation.extend(1_f32.to_le_bytes());
        representation.push(8);
    }
    for coordinates in [[0, 255, 0, 0], [0, 0, 255, 0], [0, 0, 0, 255]] {
        representation.extend(packet(&coordinates));
    }
    representation.extend(0_u32.to_le_bytes());
    let mut body = Vec::new();
    body.extend(1_u16.to_le_bytes());
    body.extend(1_u16.to_le_bytes());
    body.extend(0_u64.to_le_bytes());
    body.extend(1_u16.to_le_bytes());
    body.extend(0_u32.to_le_bytes());
    body.extend(1_u16.to_le_bytes());
    body.extend(representation);
    let mut payload = element(object_type(0x10dd_10ab), 4, 42, &body);
    end_elements(&mut payload);
    payload.extend([1, 0, 0, 0, 0, 0]);
    let mut segment = vec![3; 16];
    segment.extend(7_u32.to_le_bytes());
    segment.extend(u32_bytes(24 + payload.len()));
    segment.extend(payload);
    segment
}

fn scene_segment() -> Vec<u8> {
    let mut shape = Vec::new();
    shape.extend(1_u16.to_le_bytes());
    shape.extend(0_u32.to_le_bytes());
    shape.extend(0_u32.to_le_bytes());
    shape.extend(1_u16.to_le_bytes());
    for coordinate in [0_f32, 0., 0., 1., 1., 1., 0., 0., 0., 1., 1., 1.] {
        shape.extend(coordinate.to_le_bytes());
    }
    shape.extend(1_f32.to_le_bytes());
    for value in [4_u32, 4, 1, 1, 4, 4, 4096] {
        shape.extend(value.to_le_bytes());
    }
    shape.extend(0_f32.to_le_bytes());
    shape.extend(1_u16.to_le_bytes());
    shape.extend(0_u64.to_le_bytes());
    shape.extend([8, 0, 0, 0]);
    let mut scene = element(object_type(0x10dd_1077), 2, 5, &shape);
    end_elements(&mut scene);

    let mut late = Vec::new();
    late.extend(1_u16.to_le_bytes());
    late.extend(0_u32.to_le_bytes());
    late.extend(1_u16.to_le_bytes());
    late.extend([3; 16]);
    late.extend(7_u32.to_le_bytes());
    late.extend(42_u32.to_le_bytes());
    late.extend(1_u32.to_le_bytes());
    scene.extend(element(
        [
            0xe5, 0x5b, 0xb0, 0xe0, 0xbd, 0xfb, 0xd1, 0x11, 0xa3, 0xa7, 0x00, 0xaa, 0x00, 0xd1,
            0x09, 0x54,
        ],
        8,
        20,
        &late,
    ));
    let key = "JT_LLPROP_SHAPEIMPL";
    let mut property = Vec::new();
    property.extend(1_u16.to_le_bytes());
    property.extend(0x4000_0000_u32.to_le_bytes());
    property.extend(1_u16.to_le_bytes());
    property.extend(u32_bytes(key.encode_utf16().count()));
    property.extend(key.encode_utf16().flat_map(u16::to_le_bytes));
    scene.extend(element(object_type(0x10dd_106e), 5, 21, &property));
    end_elements(&mut scene);
    scene.extend(1_u16.to_le_bytes());
    for value in [1_u32, 5, 21, 20, 0] {
        scene.extend(value.to_le_bytes());
    }
    let compressed = zlib_compress(&scene);
    let mut segment = vec![2; 16];
    segment.extend(1_u32.to_le_bytes());
    segment.extend(u32_bytes(33 + compressed.len()));
    segment.extend(2_u32.to_le_bytes());
    segment.extend(u32_bytes(1 + compressed.len()));
    segment.push(2);
    segment.extend(compressed);
    segment
}

/// A mesh-only part; no Parasolid carrier or fabricated occurrence placement.
pub(crate) fn tetrahedron_prt() -> Vec<u8> {
    let segments = [(2_u8, 1_u32, scene_segment()), (3, 7, mesh_lod())];
    let mut document = b"Version 9.4 JT".to_vec();
    document.resize(80, b' ');
    document.push(0);
    document.extend(0_u32.to_le_bytes());
    document.extend(105_u32.to_le_bytes());
    document.extend([1; 16]);
    document.extend(2_u32.to_le_bytes());
    let mut offset = 105 + 4 + 2 * 28;
    for (id, kind, bytes) in &segments {
        document.extend([*id; 16]);
        document.extend(u32_bytes(offset));
        document.extend(u32_bytes(bytes.len()));
        document.extend(kind.to_be_bytes());
        offset += bytes.len();
    }
    for (_, _, bytes) in segments {
        document.extend(bytes);
    }
    let mut indexed = Vec::new();
    for value in [9_u32, 1, 0, 100, 0, 28, 0] {
        indexed.extend(value.to_le_bytes());
    }
    indexed.extend(document);
    prt_with_named_payloads(&[("/Root/UG_PART/DisplayJT", indexed)])
}
