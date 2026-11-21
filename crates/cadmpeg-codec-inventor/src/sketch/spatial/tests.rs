// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::test_fixtures::{content_at, parse as with_source};

fn record() -> Vec<u8> {
    let mut bytes = content_at(7, 31, &[]);
    bytes.extend_from_slice(&(-7_i32).to_le_bytes());
    bytes.extend_from_slice(&9_u32.to_le_bytes());
    bytes.extend_from_slice(&8_u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    for reference in [0x8000_0002_u32, 0x8000_0003] {
        bytes.extend_from_slice(&reference.to_le_bytes());
    }
    for value in [0x1234_u32, 0x5678] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [-1.0_f64, -2.0, -3.0, 4.0, 5.0, 6.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn state_header_members_values_and_bounds_exhaust_the_spatial_record() {
    let bytes = record();
    let parsed = with_source(&bytes, |ctx, source| {
        parse(ctx, source).expect("spatial sketch")
    });
    assert_eq!(parsed.save_version_major, 31);
    assert_eq!(parsed.state, -7);
    assert!(parsed.header.stamps.is_empty());
    assert_eq!(parsed.count_value, 9);
    assert_eq!(parsed.entities.marker(), 8);
    assert_eq!(
        parsed
            .entities
            .references()
            .iter()
            .map(|item| item.index())
            .collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(parsed.values, [0x1234, 0x5678]);
    assert_eq!(parsed.lower.map(FiniteReal::get), [-1.0, -2.0, -3.0]);
    assert_eq!(parsed.upper.map(FiniteReal::get), [4.0, 5.0, 6.0]);
    for len in 0..bytes.len() {
        with_source(&bytes[..len], |ctx, source| {
            assert!(parse(ctx, source).is_err());
        });
    }
    let mut extended = bytes;
    extended.push(0);
    with_source(
        &extended,
        |ctx, source| assert!(parse(ctx, source).is_err()),
    );
}
