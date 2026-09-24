// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::disallowed_methods)]

use super::{read_finite, Uuid};
use crate::chunks::{BoundedReader, FramingError};

/// A non-finite value is refused at its own first byte, not after the read.
#[test]
fn read_finite_refuses_a_nonfinite_value_at_its_first_byte() {
    let mut bytes = vec![0xa5, 0xa5, 0xa5];
    let value_offset = bytes.len();
    bytes.extend(f64::NAN.to_le_bytes());
    bytes.extend(1.5_f64.to_le_bytes());
    let mut reader = BoundedReader::new(&bytes, 3, bytes.len()).expect("bounded reader");
    let error = read_finite(&mut reader, "witness").expect_err("nonfinite value");
    assert_eq!(
        error,
        FramingError::structural(value_offset, "witness is not finite")
    );

    let mut reader = BoundedReader::new(&bytes, 11, bytes.len()).expect("bounded reader");
    assert_eq!(
        read_finite(&mut reader, "witness"),
        Ok(crate::test_support::finite(1.5))
    );
}

/// `to_wire` inverts `from_wire` on the mixed-endian group transposition.
#[test]
fn wire_and_canonical_forms_round_trip() {
    let canonical = Uuid::from_canonical([
        0x05, 0x59, 0x73, 0x3b, 0x53, 0x32, 0x49, 0xd1, 0xa9, 0x36, 0x05, 0x32, 0xac, 0x76, 0xad,
        0xe5,
    ]);
    let wire = canonical.to_wire();
    assert_eq!(
        wire,
        [
            0x3b, 0x73, 0x59, 0x05, 0x32, 0x53, 0xd1, 0x49, 0xa9, 0x36, 0x05, 0x32, 0xac, 0x76,
            0xad, 0xe5,
        ]
    );
    assert_eq!(Uuid::from_wire(wire), canonical);
}

#[test]
fn parses_mixed_endian_uuid_and_nil_uuid() {
    let uuid = Uuid::from_wire([
        0xdd, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ]);
    assert_eq!(uuid.to_string(), "4ed7d4dd-e947-11d3-bfe5-0010830122f0");
    assert!(!uuid.is_nil());
    assert!(Uuid::nil().is_nil());
    assert_eq!(
        Uuid::nil().to_string(),
        "00000000-0000-0000-0000-000000000000"
    );
}

#[test]
fn nonempty_source_id_matches_the_uuid_rendering() {
    for wire in [
        [
            0xdd, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01,
            0x22, 0xf0,
        ],
        [0; 16],
        [0xff; 16],
    ] {
        let uuid = Uuid::from_wire(wire);
        assert_eq!(uuid.to_nonempty().as_str(), uuid.to_string());
    }
}

/// A non-finite coordinate and an overflowing product are both refused, so the
/// single product test covers every non-finite input.
#[test]
fn scaled_coordinate_refuses_nonfinite_inputs_and_overflowing_products() {
    let scale = crate::test_support::millimeter_scale(25.4);
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(super::scaled_coordinate(value, scale), None, "{value}");
    }
    assert_eq!(
        super::scaled_coordinate(2.0, scale),
        Some(crate::test_support::finite(50.8))
    );

    let huge = crate::test_support::millimeter_scale(f64::MAX);
    assert_eq!(super::scaled_coordinate(f64::MAX, huge), None);
}
