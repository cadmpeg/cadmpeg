// SPDX-License-Identifier: Apache-2.0
//! Control and owner-chart invariants for parameter-space records.

use super::parsed_owner_charts;
use crate::families::b2::records::b2_parameter_points;
use crate::test_support::test_b2::{
    b2_compact_parameter_operand_stream, b2_owner_chart_stream, b2_parameter_point_stream,
};

#[test]
fn parameter_points_retain_wide_and_nonminimal_compact_operands() {
    let bytes = b2_compact_parameter_operand_stream();
    let points = b2_parameter_points(&bytes);
    assert_eq!(
        points.iter().map(|p| p.operand.value()).collect::<Vec<_>>(),
        [220, 65_390, 1_727_824, u32::MAX, 1]
    );
    assert_eq!(
        points
            .iter()
            .map(|p| p.operand.byte_len())
            .collect::<Vec<_>>(),
        [2, 3, 4, 5, 2]
    );
    assert_eq!(points.last().unwrap().end, bytes.len());
    assert!(points.windows(2).all(|p| p[0].end == p[1].pos));
    for point in points {
        assert_eq!(
            point.payload.byte_len(point.operand),
            u8::try_from(point.end - point.pos - 5).unwrap()
        );
    }
}

#[test]
fn parameter_points_abstain_on_truncation_extra_words_and_centered_empty_domain() {
    let valid = b2_compact_parameter_operand_stream();
    let first_end = b2_parameter_points(&valid)[0].end;
    let first = &valid[..first_end];
    for length in 0..first.len() {
        assert!(b2_parameter_points(&first[..length]).is_empty());
    }
    let mut extra = first.to_vec();
    extra[3] += 8;
    extra.extend_from_slice(&0.0f64.to_le_bytes());
    assert!(b2_parameter_points(&extra).is_empty());
    for control in [0x08, 0x48, 0x88] {
        for t0 in [0.0f64, 1.0] {
            let count = if control == 0x08 { 5 } else { 3 };
            let mut bytes = vec![0xb2, 0x03, 0x18, 2 + 8 * count, 5, 5, control];
            for value in [t0, 2.0, 3.0, 1.0, 0.0]
                .into_iter()
                .take(usize::from(count))
            {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            assert!(b2_parameter_points(&bytes).is_empty());
        }
    }
}

#[test]
fn owner_chart_compares_operand_values_independently_of_compact_width() {
    let original = b2_owner_chart_stream(0x28);
    let records = crate::wire::records::consolidated_records(&original);
    let mut bytes = Vec::new();
    for record in records {
        let frame = &original[record.range().unwrap()];
        if record.class() != 0x18 {
            bytes.extend_from_slice(frame);
            continue;
        }
        bytes.extend_from_slice(&frame[..3]);
        bytes.extend_from_slice(&[frame[3] + 1, frame[4], 0x04, frame[5] / 4]);
        bytes.extend_from_slice(&frame[6..]);
    }
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_eq!(parsed_owner_charts(&bytes, &records).len(), 1);
}

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
