// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{edge_line, EdgeLine};

/// Agreement of a computed millimetre value with the hand-computed one.
const EPS_MM: f64 = 1e-9;

/// A proxy of `len` bytes with the edge fields in metres.
fn proxy(len: usize, start: [f64; 3], direction: [f64; 3], length: f64) -> Vec<u8> {
    let mut bytes = vec![0_u8; len];
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
fn a_proxy_of_another_size_is_not_an_edge() {
    let bytes = proxy(560, [0.0; 3], [1.0, 0.0, 0.0], 0.010);
    assert_eq!(edge_line(&bytes), None);
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
