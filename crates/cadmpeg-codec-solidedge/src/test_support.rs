// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_container::compound::CompoundClassId;
use cadmpeg_test_support::bytes::{put_u16, put_u32};

/// Root class of a part document.
pub(crate) const PART: CompoundClassId = CompoundClassId::from_guid(
    0x23c5_2e80,
    0x4698,
    0x11ce,
    [0xb3, 0x07, 0x08, 0x00, 0x36, 0x3a, 0x1e, 0x02],
);
/// Root class of a sheet-metal document.
pub(crate) const SHEET_METAL: CompoundClassId = CompoundClassId::from_guid(
    0xdd85_22e0,
    0x2375,
    0x11d0,
    [0xac, 0x05, 0x08, 0x00, 0x36, 0xfd, 0x18, 0x02],
);
/// Root class of an assembly document.
pub(crate) const ASSEMBLY: CompoundClassId = CompoundClassId::from_guid(
    0x00c6_bf00,
    0x483b,
    0x11ce,
    [0x95, 0x1a, 0x08, 0x00, 0x36, 0x01, 0xbe, 0x52],
);
/// Root class of a family-of-assemblies document.
pub(crate) const FAMILY: CompoundClassId = CompoundClassId::from_guid(
    0x04d6_13a0,
    0xa322,
    0x40b5,
    [0xa2, 0xa4, 0x36, 0xca, 0x0d, 0xe6, 0xf5, 0xd9],
);
/// A root class that no Solid Edge document kind uses.
pub(crate) const OTHER: CompoundClassId =
    CompoundClassId::from_guid(0x0000_0001, 0x0002, 0x0003, [4, 5, 6, 7, 8, 9, 10, 11]);

/// Compound document whose root holds the two named stream entries and the given class.
pub(crate) fn compound_with(first: &str, second: &str, class: CompoundClassId) -> Vec<u8> {
    let mut file = cadmpeg_test_support::compound::compound_fixture();
    let directory = &mut file[512..1024];
    directory[80..96].copy_from_slice(&class.to_bytes());
    directory[128..192].fill(0);
    name(&mut directory[128..256], first);
    put_u32(directory, 128 + 72, 2);
    let entry = &mut directory[256..384];
    name(entry, second);
    entry[66] = 2;
    entry[67] = 0;
    entry[68..80].fill(0xff);
    put_u32(entry, 116, 0xffff_fffe);
    file
}

pub(crate) fn compound() -> Vec<u8> {
    compound_with("PSMroots", "JSitesList", PART)
}

fn name(entry: &mut [u8], value: &str) {
    for (i, unit) in value.encode_utf16().enumerate() {
        put_u16(entry, i * 2, unit);
    }
    put_u16(entry, 64, u16::try_from((value.len() + 1) * 2).unwrap());
}

pub(crate) fn with_partitions(streams: &[Vec<u8>]) -> Vec<u8> {
    let fat = 2 + 8 * streams.len();
    let mut file = vec![0u8; (fat + 2) * 512];
    let base = compound();
    file[..512].copy_from_slice(&base[..512]);
    put_u32(&mut file, 76, u32::try_from(fat).unwrap());
    let directory = &mut file[512..1536];
    for entry in directory.chunks_exact_mut(128) {
        entry[68..80].fill(255);
    }
    entry(directory, 0, "Root Entry", 5, 1, None);
    directory[80..96].copy_from_slice(&PART.to_bytes());
    entry(directory, 1, "PSMroots", 2, u32::MAX, None);
    put_u32(directory, 128 + 72, 3);
    entry(directory, 2, "JSitesList", 2, u32::MAX, None);
    put_u32(directory, 384 + 72, 2);
    entry(directory, 3, "PARASOLID", 1, 4, None);
    for (index, stream) in streams.iter().enumerate() {
        assert!(stream.len() <= 4096);
        let slot = 4 + index;
        entry(
            directory,
            slot,
            &format!("STREAM{index}.P_B"),
            2,
            u32::MAX,
            Some(2 + 8 * index),
        );
        if index + 1 < streams.len() {
            put_u32(directory, slot * 128 + 72, u32::try_from(slot + 1).unwrap());
        }
    }
    for (index, stream) in streams.iter().enumerate() {
        let start = (3 + 8 * index) * 512;
        file[start..start + stream.len()].copy_from_slice(stream);
    }
    let table = &mut file[(fat + 1) * 512..(fat + 2) * 512];
    table.fill(255);
    put_u32(table, 0, 1);
    put_u32(table, 4, 0xffff_fffe);
    for index in 0..streams.len() {
        for sector in 2 + index * 8..10 + index * 8 {
            put_u32(
                table,
                sector * 4,
                if sector == 9 + index * 8 {
                    0xffff_fffe
                } else {
                    u32::try_from(sector + 1).unwrap()
                },
            );
        }
    }
    put_u32(table, fat * 4, 0xffff_fffd);
    file
}

fn entry(
    directory: &mut [u8],
    index: usize,
    value: &str,
    kind: u8,
    child: u32,
    sector: Option<usize>,
) {
    let bytes = &mut directory[index * 128..(index + 1) * 128];
    name(bytes, value);
    bytes[66] = kind;
    bytes[67] = 1;
    put_u32(bytes, 76, child);
    put_u32(
        bytes,
        116,
        sector.map_or(0xffff_fffe, |value| u32::try_from(value).unwrap()),
    );
    bytes[120..128].copy_from_slice(&(if sector.is_some() { 4096u64 } else { 0 }).to_le_bytes());
}

pub(crate) fn sphere_partition() -> Vec<u8> {
    let description = b": TRANSMIT FILE (partition)";
    let schema = b"SCH_999_13006";
    let mut bytes = b"PS".to_vec();
    bytes.extend_from_slice(&u32::try_from(description.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(description);
    bytes.extend_from_slice(&u32::try_from(schema.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(schema);
    bytes.extend_from_slice(&0u32.to_be_bytes());
    node(&mut bytes, 12, 2);
    integer(&mut bytes, 0);
    for _ in 0..6 {
        pointer(&mut bytes, 1);
    }
    real(&mut bytes, 1.0e-8);
    real(&mut bytes, 1.0e-8);
    for _ in 0..3 {
        pointer(&mut bytes, 1);
    }
    bytes.push(0);
    pointer(&mut bytes, 1);
    bytes.extend_from_slice(&[1, 0]);
    pointer(&mut bytes, 3);
    for _ in 0..6 {
        pointer(&mut bytes, 1);
    }
    node(&mut bytes, 13, 3);
    integer(&mut bytes, 0);
    pointer(&mut bytes, 1);
    pointer(&mut bytes, 2);
    pointer(&mut bytes, 1);
    pointer(&mut bytes, 4);
    for _ in 0..4 {
        pointer(&mut bytes, 1);
    }
    node(&mut bytes, 14, 4);
    integer(&mut bytes, 0);
    pointer(&mut bytes, 1);
    real(&mut bytes, 0.0);
    for _ in 0..3 {
        pointer(&mut bytes, 1);
    }
    pointer(&mut bytes, 3);
    pointer(&mut bytes, 5);
    bytes.push(b'+');
    for _ in 0..5 {
        pointer(&mut bytes, 1);
    }
    node(&mut bytes, 53, 5);
    integer(&mut bytes, 0);
    pointer(&mut bytes, 1);
    pointer(&mut bytes, 2);
    for _ in 0..3 {
        pointer(&mut bytes, 1);
    }
    bytes.push(b'+');
    for value in [0.0, 0.0, 0.0, 0.007, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0] {
        real(&mut bytes, value);
    }
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    bytes
}

fn node(bytes: &mut Vec<u8>, kind: u16, index: u16) {
    bytes.extend_from_slice(&kind.to_be_bytes());
    pointer(bytes, index);
}
fn pointer(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_be_bytes());
}
fn integer(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_be_bytes());
}
fn real(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

pub(crate) fn cube_partition() -> Vec<u8> {
    let sphere = sphere_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&sphere)
        .unwrap()
        .unwrap();
    let header_len = records.nodes().next().unwrap().range().start;
    let mut bytes = sphere[..header_len].to_vec();
    let coordinates = [
        [0.0, 0.0, 0.0],
        [0.002, 0.0, 0.0],
        [0.002, 0.002, 0.0],
        [0.0, 0.002, 0.0],
        [0.0, 0.0, 0.002],
        [0.002, 0.0, 0.002],
        [0.002, 0.002, 0.002],
        [0.0, 0.002, 0.002],
    ];
    let rings = [
        [0usize, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ];
    let edge_ends = [
        [0usize, 1],
        [1, 2],
        [2, 3],
        [3, 0],
        [4, 5],
        [5, 6],
        [6, 7],
        [7, 4],
        [0, 4],
        [1, 5],
        [2, 6],
        [3, 7],
    ];
    let normals = [
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
        [0.0, -1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [-1.0, 0.0, 0.0],
    ];
    let x_axes = [
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let mut incidences = Vec::new();
    for (face, ring) in rings.iter().enumerate() {
        for side in 0..4 {
            let ends = [ring[side], ring[(side + 1) % 4]];
            let edge = edge_ends
                .iter()
                .position(|pair| *pair == ends || *pair == [ends[1], ends[0]])
                .unwrap();
            incidences.push((face, side, edge, ends, edge_ends[edge] == ends));
        }
    }
    node(&mut bytes, 12, 2);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1; 6]);
    real(&mut bytes, 1.0e-8);
    real(&mut bytes, 1.0e-8);
    pointers(&mut bytes, &[1; 3]);
    bytes.push(0);
    pointer(&mut bytes, 1);
    bytes.extend_from_slice(&[1, 0]);
    pointer(&mut bytes, 3);
    pointers(&mut bytes, &[1; 6]);
    node(&mut bytes, 13, 3);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1, 2, 1, 4, 1, 1, 1, 1]);
    for face in 0..6 {
        node(&mut bytes, 14, wire_id(4, face));
        integer(&mut bytes, 0);
        pointer(&mut bytes, 1);
        real(&mut bytes, 0.0);
        pointers(
            &mut bytes,
            &[
                if face == 5 { 1 } else { wire_id(5, face) },
                if face == 0 { 1 } else { wire_id(3, face) },
                wire_id(16, face),
                3,
                wire_id(10, face),
            ],
        );
        bytes.push(b'+');
        pointers(&mut bytes, &[1; 5]);
        node(&mut bytes, 50, wire_id(10, face));
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 2, 1, 1, 1]);
        bytes.push(b'+');
        vector(&mut bytes, coordinates[rings[face][0]]);
        vector(&mut bytes, normals[face]);
        vector(&mut bytes, x_axes[face]);
        node(&mut bytes, 15, wire_id(16, face));
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, wire_id(62, face * 4), wire_id(4, face), 1]);
    }
    for (vertex, position) in coordinates.iter().enumerate() {
        let point = 37 - u16::try_from(vertex).unwrap();
        node(&mut bytes, 18, wire_id(22, vertex));
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 1, 1, 1, point]);
        real(&mut bytes, 0.0);
        pointer(&mut bytes, 2);
        node(&mut bytes, 29, point);
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 2, 1, 1]);
        vector(&mut bytes, *position);
    }
    for (edge, [start, end]) in edge_ends.iter().copied().enumerate() {
        let fin = incidences
            .iter()
            .position(|incidence| incidence.2 == edge)
            .unwrap();
        node(&mut bytes, 16, wire_id(38, edge));
        integer(&mut bytes, 0);
        pointer(&mut bytes, 1);
        real(&mut bytes, 0.0);
        pointers(
            &mut bytes,
            &[wire_id(62, fin), 1, 1, wire_id(50, edge), 1, 1, 2],
        );
        node(&mut bytes, 30, wire_id(50, edge));
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 2, 1, 1, 1]);
        bytes.push(b'+');
        vector(&mut bytes, coordinates[start]);
        vector(
            &mut bytes,
            std::array::from_fn(|axis| (coordinates[end][axis] - coordinates[start][axis]) / 0.002),
        );
    }
    for (fin, (face, side, edge, ends, forward)) in incidences.iter().copied().enumerate() {
        let other = incidences
            .iter()
            .enumerate()
            .position(|(index, incidence)| index != fin && incidence.2 == edge)
            .unwrap();
        node(&mut bytes, 17, wire_id(62, fin));
        pointers(
            &mut bytes,
            &[
                1,
                wire_id(16, face),
                wire_id(62, face * 4 + (side + 1) % 4),
                wire_id(62, face * 4 + (side + 3) % 4),
                wire_id(22, ends[1]),
                wire_id(62, other),
                wire_id(38, edge),
                1,
                1,
            ],
        );
        bytes.push(if forward { b'+' } else { b'-' });
    }
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    bytes
}

/// A planar disc face of radius 5 mm bounded by one ring edge on a circle, with
/// no vertex: the single fin is its own successor and predecessor.
pub(crate) fn disc_partition() -> Vec<u8> {
    let sphere = sphere_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&sphere)
        .unwrap()
        .unwrap();
    let header_len = records.nodes().next().unwrap().range().start;
    let mut bytes = sphere[..header_len].to_vec();
    node(&mut bytes, 12, 2);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1; 6]);
    real(&mut bytes, 1.0e-8);
    real(&mut bytes, 1.0e-8);
    pointers(&mut bytes, &[1; 3]);
    bytes.push(0);
    pointer(&mut bytes, 1);
    bytes.extend_from_slice(&[1, 0]);
    pointer(&mut bytes, 3);
    pointers(&mut bytes, &[1; 6]);
    node(&mut bytes, 13, 3);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1, 2, 1, 4, 1, 1, 1, 1]);
    node(&mut bytes, 14, 4);
    integer(&mut bytes, 0);
    pointer(&mut bytes, 1);
    real(&mut bytes, 0.0);
    pointers(&mut bytes, &[1, 1, 6, 3, 5]);
    bytes.push(b'+');
    pointers(&mut bytes, &[1; 5]);
    node(&mut bytes, 50, 5);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1, 2, 1, 1, 1]);
    bytes.push(b'+');
    vector(&mut bytes, [0.0, 0.0, 0.0]);
    vector(&mut bytes, [0.0, 0.0, 1.0]);
    vector(&mut bytes, [1.0, 0.0, 0.0]);
    node(&mut bytes, 15, 6);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1, 7, 4, 1]);
    node(&mut bytes, 16, 8);
    integer(&mut bytes, 0);
    pointer(&mut bytes, 1);
    real(&mut bytes, 0.0);
    pointers(&mut bytes, &[7, 1, 1, 9, 1, 1, 2]);
    node(&mut bytes, 31, 9);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1, 2, 1, 1, 1]);
    bytes.push(b'+');
    vector(&mut bytes, [0.0, 0.0, 0.0]);
    vector(&mut bytes, [0.0, 0.0, 1.0]);
    vector(&mut bytes, [1.0, 0.0, 0.0]);
    real(&mut bytes, 0.005);
    node(&mut bytes, 17, 7);
    pointers(&mut bytes, &[1, 6, 7, 7, 1, 1, 8, 1, 1]);
    bytes.push(b'+');
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    bytes
}

fn wire_id(base: u16, index: usize) -> u16 {
    base + u16::try_from(index).unwrap()
}
fn pointers(bytes: &mut Vec<u8>, values: &[u16]) {
    for value in values {
        pointer(bytes, *value);
    }
}
fn vector(bytes: &mut Vec<u8>, values: [f64; 3]) {
    for value in values {
        real(bytes, value);
    }
}

pub(crate) fn chart_partition(scale: f64) -> Vec<u8> {
    let sphere = sphere_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&sphere)
        .unwrap()
        .unwrap();
    let start = records.nodes().next().unwrap().range().start;
    let mut bytes = sphere[..start].to_vec();
    node(&mut bytes, 38, 2);
    integer(&mut bytes, 0);
    for _ in 0..5 {
        pointer(&mut bytes, 1);
    }
    bytes.push(b'+');
    for reference in [3, 4, 5, 6, 7] {
        pointer(&mut bytes, reference);
    }
    let points = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0],
    ];
    bytes.extend_from_slice(&40u16.to_be_bytes());
    integer(&mut bytes, u32::try_from(points.len()).unwrap());
    pointer(&mut bytes, 5);
    real(&mut bytes, 0.0);
    real(&mut bytes, scale);
    integer(&mut bytes, u32::try_from(points.len()).unwrap());
    real(&mut bytes, 1.0e-6);
    real(&mut bytes, 0.0);
    real(&mut bytes, -3.14158e13);
    real(&mut bytes, -3.14158e13);
    for value in points.into_iter().flatten() {
        real(&mut bytes, value);
    }
    for index in [6, 7] {
        bytes.extend_from_slice(&41u16.to_be_bytes());
        integer(&mut bytes, 1);
        pointer(&mut bytes, index);
        bytes.push(b'H');
        for _ in 0..3 {
            real(&mut bytes, 0.0);
        }
    }
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    bytes
}

/// The chart partition plus trimmed curves: node 8 restricts the
/// intersection, node 9 restricts node 8, node 10 restricts itself and node 11
/// restricts a node that is absent.
pub(crate) fn trimmed_chart_partition() -> Vec<u8> {
    let mut bytes = chart_partition(1.0);
    bytes.truncate(bytes.len() - 4);
    for (index, basis) in [(8, 2), (9, 8), (10, 10), (11, 99)] {
        node(&mut bytes, 133, index);
        integer(&mut bytes, 0);
        for _ in 0..5 {
            pointer(&mut bytes, 1);
        }
        bytes.push(b'+');
        pointer(&mut bytes, basis);
        for _ in 0..6 {
            real(&mut bytes, 0.0);
        }
        real(&mut bytes, 0.0);
        real(&mut bytes, 1.0);
    }
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    bytes
}
