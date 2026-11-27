// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{edge_line, vertex_point, EdgeLine};

/// Agreement of a computed millimetre value with the hand-computed one.
const EPS_MM: f64 = 1e-9;

/// A proxy of `len` bytes whose geometry record is a straight edge with the
/// fields in metres. The geometry class identifier ends at offset 104 + 16.
fn proxy(len: usize, start: [f64; 3], direction: [f64; 3], length: f64) -> Vec<u8> {
    const IDENTIFIER_AT: usize = 104;
    let tail = [0xce, 0x11, 0x9a, 0x5e, 0x08, 0x00, 0x36, 0x9b, 0x41, 0x02];
    let mut bytes = vec![0_u8; len];
    bytes[IDENTIFIER_AT + 6..IDENTIFIER_AT + 16].copy_from_slice(&tail);
    bytes[IDENTIFIER_AT + 16] = 3;
    let mut put = |at: usize, values: &[f64]| {
        for (index, value) in values.iter().enumerate() {
            let start = at + index * 8;
            bytes[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }
    };
    put(121, &start);
    put(145, &direction);
    put(177, &[length]);
    bytes
}

#[test]
fn a_straight_edge_reads_in_millimetres() {
    let bytes = proxy(559, [0.010, 0.020, 0.030], [0.0, 1.0, 0.0], 0.040);
    let line = edge_line(&bytes).unwrap();
    assert!((line.start.x - 10.0).abs() <= EPS_MM);
    assert!((line.start.y - 20.0).abs() <= EPS_MM);
    assert!((line.start.z - 30.0).abs() <= EPS_MM);
    assert!((line.length - 40.0).abs() <= EPS_MM);
    let end = line.end();
    assert!((end.x - 10.0).abs() <= EPS_MM);
    assert!((end.y - 60.0).abs() <= EPS_MM);
    assert!((end.z - 30.0).abs() <= EPS_MM);
}

#[test]
fn a_proxy_of_another_size_reads_by_its_geometry_record() {
    // The geometry record, not the object length, selects the reader (§7.6).
    let bytes = proxy(560, [0.0; 3], [1.0, 0.0, 0.0], 0.010);
    let line = edge_line(&bytes).unwrap();
    assert!((line.length - 10.0).abs() <= EPS_MM);
}

#[test]
fn a_proxy_without_a_geometry_class_identifier_is_not_an_edge() {
    assert_eq!(edge_line(&[0_u8; 559]), None);
}

#[test]
fn a_proxy_that_ends_inside_its_edge_record_is_not_an_edge() {
    let bytes = proxy(559, [0.010, 0.020, 0.030], [0.0, 1.0, 0.0], 0.040);
    assert_eq!(edge_line(&bytes[..177]), None);
}

#[test]
fn a_vertex_proxy_reads_its_point_in_millimetres() {
    let mut bytes = vec![0_u8; 40];
    bytes[6..16].copy_from_slice(&[0xce, 0x11, 0x9a, 0x5e, 0x08, 0x00, 0x36, 0x9b, 0x41, 0x02]);
    for (index, value) in [0.001_f64, 0.002, 0.003].iter().enumerate() {
        let at = 16 + index * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    let point = vertex_point(&bytes).unwrap();
    assert!((point.x - 1.0).abs() <= EPS_MM);
    assert!((point.y - 2.0).abs() <= EPS_MM);
    assert!((point.z - 3.0).abs() <= EPS_MM);
}

#[test]
fn a_direction_that_is_not_a_unit_vector_is_not_an_edge() {
    let bytes = proxy(559, [0.0; 3], [2.0, 0.0, 0.0], 0.010);
    assert_eq!(edge_line(&bytes), None);
}

#[test]
fn a_zero_length_is_not_an_edge() {
    let bytes = proxy(559, [0.0; 3], [1.0, 0.0, 0.0], 0.0);
    assert_eq!(edge_line(&bytes), None::<EdgeLine>);
}

/// A proxy of `len` bytes whose geometry record starts at `len` - 439.
fn tailed_proxy(len: usize, start: [f64; 3], direction: [f64; 3], length: f64) -> Vec<u8> {
    let tail = [0xce, 0x11, 0x9a, 0x5e, 0x08, 0x00, 0x36, 0x9b, 0x41, 0x02];
    let mut bytes = vec![0_u8; len];
    let base = len - 438;
    bytes[base - 11..base - 1].copy_from_slice(&tail);
    bytes[base - 1] = 3;
    let mut put = |at: usize, values: &[f64]| {
        for (index, value) in values.iter().enumerate() {
            let start = at + index * 8;
            bytes[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }
    };
    put(base, &start);
    put(base + 24, &direction);
    put(base + 56, &[length]);
    bytes
}

#[test]
fn proxies_of_563_and_575_bytes_read_by_their_geometry_record() {
    for len in [563, 575] {
        let bytes = tailed_proxy(len, [0.010, 0.020, 0.030], [-1.0, 0.0, 0.0], 0.040);
        let line = edge_line(&bytes).unwrap();
        assert!((line.start.y - 20.0).abs() <= EPS_MM);
        assert!((line.direction.x + 1.0).abs() <= EPS_MM);
        assert!((line.length - 40.0).abs() <= EPS_MM);
    }
}
