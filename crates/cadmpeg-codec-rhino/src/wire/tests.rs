// SPDX-License-Identifier: Apache-2.0
#![allow(dead_code, clippy::disallowed_methods)]

use super::Uuid;

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
