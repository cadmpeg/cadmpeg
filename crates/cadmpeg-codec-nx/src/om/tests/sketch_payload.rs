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
