// SPDX-License-Identifier: Apache-2.0
//! Control and owner-chart invariants for parameter-space records.

use super::parsed_owner_charts;
use crate::families::b2::records::b2_parameter_points;
use crate::test_support::test_b2::{b2_owner_chart_stream, b2_parameter_point_stream};

#[test]
fn parameter_point_controls_require_matching_scalar_count_and_unit_direction() {
    for axis in [1u8, 2] {
        for mask in 0u8..7 {
            let control = axis | ((mask & 1) << 4) | ((mask & 2) << 1) | ((mask & 4) << 1);
            let count = 3 - usize::try_from(mask.count_ones()).expect("three-bit mask");
            let mut bytes = vec![
                0xb2,
                0x03,
                0x18,
                u8::try_from(2 + 8 * count).expect("bounded scalar lane"),
                0x05,
                0x05,
                control,
            ];
            for value in [2.0f64, 3.0, 4.0].into_iter().take(count) {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            assert_eq!(b2_parameter_points(&bytes).len(), 1);
            bytes[6] = control ^ 0x10;
            assert!(
                b2_parameter_points(&bytes).is_empty(),
                "control {control:#04x}"
            );
        }
    }
    let valid = b2_parameter_point_stream();
    let records = crate::wire::records::consolidated_records(&valid);
    let explicit = records
        .iter()
        .find(|r| r.class() == 0x18 && r.payload().unwrap().len() == 42)
        .unwrap();
    let mut bad_direction = valid.clone();
    let direction = explicit.payload().unwrap().start + 2 + 3 * 8;
    bad_direction[direction..direction + 8].copy_from_slice(&2.0f64.to_le_bytes());
    assert_eq!(b2_parameter_points(&bad_direction).len(), 3);
    let first = records
        .iter()
        .find(|r| r.class() == 0x18)
        .unwrap()
        .payload()
        .unwrap()
        .start;
    let mut unknown = valid;
    unknown[first + 1] = 0x80;
    assert_eq!(b2_parameter_points(&unknown).len(), 3);
}

#[test]
fn owner_chart_rejects_opposing_side_with_wrong_constant_axis() {
    let mut bytes = b2_owner_chart_stream(0x28);
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_eq!(parsed_owner_charts(&bytes, &records).len(), 1);
    let side = records
        .iter()
        .find(|r| r.class() == 0x18)
        .unwrap()
        .payload()
        .unwrap()
        .start;
    bytes[side + 1] = 0x02;
    let records = crate::wire::records::consolidated_records(&bytes);
    assert!(parsed_owner_charts(&bytes, &records).is_empty());
}

#[test]
fn owner_chart_implicit_line_requires_half_span_midpoint_and_boundary() {
    for (carrier, control, values) in [
        (0x28, 0x88, [-1.5f64, 3.5, 3.0]),
        (0x2b, 0x48, [-2.0f64, 2.0, 5.0]),
    ] {
        let mut bytes = b2_owner_chart_stream(carrier);
        let records = crate::wire::records::consolidated_records(&bytes);
        let side = records
            .iter()
            .filter(|r| r.class() == 0x18)
            .nth(2)
            .unwrap()
            .payload()
            .unwrap()
            .start;
        bytes[side + 1] = control;
        for (index, value) in values.into_iter().enumerate() {
            let at = side + 2 + index * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        let records = crate::wire::records::consolidated_records(&bytes);
        assert_eq!(parsed_owner_charts(&bytes, &records).len(), 1);
        for (index, value) in values.into_iter().enumerate() {
            let mut invalid = bytes.clone();
            let at = side + 2 + index * 8;
            invalid[at..at + 8].copy_from_slice(&(value + 1.0).to_le_bytes());
            let records = crate::wire::records::consolidated_records(&invalid);
            assert!(
                parsed_owner_charts(&invalid, &records).is_empty(),
                "lane {index}"
            );
        }
    }
}
