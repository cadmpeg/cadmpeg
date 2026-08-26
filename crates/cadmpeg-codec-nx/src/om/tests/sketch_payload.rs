// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::shifted_f64_bytes;

#[test]
fn om_sketch_scalar_pairs_accept_the_repeated_type_frame() {
    const EPS_SKETCH_SCALAR: f64 = 1e-12;

    let mut bytes = vec![0xaa, 0x00];
    let discriminator_offset = bytes.len();
    bytes.extend_from_slice(&[
        0x14, 0x14, 0x41, 0x00, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02,
        0x00, 0x03,
    ]);
    let first_offset = bytes.len();
    bytes.extend_from_slice(&shifted_f64_bytes(10.0));
    let second_offset = bytes.len();
    bytes.extend_from_slice(&shifted_f64_bytes(-20.0));

    let pairs = sketch_payload_scalar_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].offset, discriminator_offset);
    assert_eq!(pairs[0].value_offsets, [first_offset, second_offset]);
    assert!((pairs[0].values[0] - 10.0).abs() < EPS_SKETCH_SCALAR);
    assert!((pairs[0].values[1] + 20.0).abs() < EPS_SKETCH_SCALAR);
    assert_eq!(
        pairs[0].discriminator,
        bytes[discriminator_offset..first_offset].to_vec()
    );
    assert!(object_payload_scalar_pairs(&bytes).is_empty());

    bytes[discriminator_offset + 1] = 0x15;
    assert!(sketch_payload_scalar_pairs(&bytes).is_empty());
    bytes[discriminator_offset + 1] = 0x14;
    bytes.truncate(second_offset + 7);
    assert!(sketch_payload_scalar_pairs(&bytes).is_empty());
}

#[test]
fn om_sketch_scalar_pairs_reject_non_binary64_atoms() {
    let mut bytes = vec![0x00, 0x21, 0x21, 0x41, 0x00];
    bytes.extend_from_slice(&[
        0x00, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
    ]);
    bytes.extend_from_slice(&[0x30, 0x42, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00]);
    bytes.extend_from_slice(&[0xd0, 0x29, 0x33, 0x32, 0x50, 0x20, 0x00, 0x00]);

    assert!(sketch_payload_scalar_pairs(&bytes).is_empty());
}

#[test]
fn sketch_scalar_lane_parser_reads_mixed_nonzero_scalar_atoms() {
    let discriminator = vec![
        0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86, 0x02,
        0x00, 0x01, 0x00,
    ];
    let mut bytes = discriminator.clone();
    let mut shifted_f64 = 1.5_f64.to_be_bytes();
    shifted_f64[0] -= 0x10;
    bytes.extend_from_slice(&shifted_f64);
    let mut shifted_f32 = 3.25_f32.to_be_bytes();
    shifted_f32[0] += 0x10;
    bytes.extend_from_slice(&shifted_f32);
    bytes.push(0x00);

    let lanes = sketch_payload_scalar_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 0);
    assert_eq!(lanes[0].discriminator, discriminator);
    assert_eq!(lanes[0].values, [1.5, 3.25]);
    assert_eq!(
        lanes[0].raw_values,
        [shifted_f64.to_vec(), shifted_f32.to_vec()]
    );
    assert_eq!(lanes[0].value_offsets, [18, 26]);
    assert_eq!(lanes[0].terminator_offset, 30);

    let long_discriminator = vec![
        0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x04, 0x80, 0x86, 0x81,
        0x02, 0x00, 0x01, 0x00,
    ];
    let mut long_bytes = long_discriminator.clone();
    long_bytes.extend_from_slice(&shifted_f64);
    long_bytes.extend_from_slice(&shifted_f32);
    long_bytes.push(0x00);

    let long_lanes = sketch_payload_scalar_lanes(&long_bytes);
    assert_eq!(long_lanes.len(), 1);
    assert_eq!(long_lanes[0].discriminator, long_discriminator);
    assert_eq!(long_lanes[0].values, [1.5, 3.25]);
    assert_eq!(long_lanes[0].value_offsets, [19, 27]);
    assert_eq!(long_lanes[0].terminator_offset, 31);

    let mut missing_terminator = bytes[..bytes.len() - 1].to_vec();
    assert!(sketch_payload_scalar_lanes(&missing_terminator).is_empty());
    missing_terminator[18] = 0x00;
    assert!(sketch_payload_scalar_lanes(&missing_terminator).is_empty());
}
