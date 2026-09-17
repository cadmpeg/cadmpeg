// SPDX-License-Identifier: Apache-2.0
use super::named_parameter_scope_tail_is_valid;
use crate::design::test_support::dump::lp_utf16;

fn named_scope_tail(lane_value: u64) -> Vec<u8> {
    let label = "Canvas";
    let label_code_units = label.encode_utf16().count();
    let marker = 19 + label_code_units * 2;
    let mut bytes = vec![0; marker + 59];
    bytes[0..4].copy_from_slice(&1u32.to_le_bytes());
    let mut label_bytes = Vec::new();
    lp_utf16(&mut label_bytes, label);
    bytes[8..8 + label_bytes.len()].copy_from_slice(&label_bytes);
    bytes[marker] = 1;
    bytes[marker + 1] = 0xd5;
    bytes[marker + 2..marker + 10].copy_from_slice(&lane_value.to_le_bytes());
    bytes[marker + 12..marker + 16].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes[marker + 16..marker + 20].copy_from_slice(&0xfcu32.to_le_bytes());
    bytes[marker + 20..marker + 28].copy_from_slice(&0.25f64.to_le_bytes());
    bytes[marker + 28..marker + 32].copy_from_slice(&0xfcu32.to_le_bytes());
    bytes[marker + 32] = 1;
    bytes[marker + 33] = 0xd4;
    bytes[marker + 34..marker + 42].copy_from_slice(&lane_value.to_le_bytes());
    bytes[marker + 42..marker + 46].copy_from_slice(&[0, 1, 0, 0]);
    bytes[marker + 46] = 1;
    bytes[marker + 47] = 0xd3;
    bytes[marker + 48..marker + 56].copy_from_slice(&lane_value.to_le_bytes());
    bytes
}

#[test]
fn named_scope_tail_requires_one_repeated_binary_lane_value() {
    for lane_value in [0, 1] {
        let bytes = named_scope_tail(lane_value);
        assert_eq!(
            named_parameter_scope_tail_is_valid(&bytes, 0, bytes.len(), bytes.len()),
            Some(true)
        );
    }

    let mut mismatched = named_scope_tail(0);
    let marker = mismatched.len() - 59;
    mismatched[marker + 34..marker + 42].copy_from_slice(&1u64.to_le_bytes());
    assert_eq!(
        named_parameter_scope_tail_is_valid(&mismatched, 0, mismatched.len(), mismatched.len()),
        Some(false)
    );

    let outside_domain = named_scope_tail(2);
    assert_eq!(
        named_parameter_scope_tail_is_valid(
            &outside_domain,
            0,
            outside_domain.len(),
            outside_domain.len()
        ),
        Some(false)
    );
}
