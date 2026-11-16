// SPDX-License-Identifier: Apache-2.0

#[test]
fn sheet_rolling_bridge_admits_first_middle_control_one() {
    let mut bytes = vec![0x85, 0x15, 0x11, 0x0d, 0x09, 0x05, 0x11, 0x05];
    bytes.extend(1.0_f64.to_le_bytes());
    bytes.extend([0x01, 0x05]);
    bytes.extend([0; 8]);
    bytes.extend([0x01, 0x05]);
    assert_eq!(
        super::bridge_radius(&bytes, true).map(|(_, radius)| radius),
        Some(1.0)
    );
    assert!(super::bridge_radius(&bytes, false).is_none());
    bytes[16] = 0x03;
    bytes[17] = 0x01;
    assert!(super::bridge_radius(&bytes, true).is_none());
}
