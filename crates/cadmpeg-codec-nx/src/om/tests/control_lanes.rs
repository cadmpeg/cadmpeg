// SPDX-License-Identifier: Apache-2.0
//! Offset-store control-lane grammar tests.

use super::*;

#[test]
fn product_anchored_control_lane_crosses_the_first_column_boundary() {
    let control = [0x11, 0x01, 0x00, 0xe0];
    let mut first_record = vec![0x38, 0x01, 0x00];
    first_record.extend_from_slice(&7u32.to_le_bytes());
    first_record.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");

    assert_eq!(
        offset_store_control_form(&control, Some(&first_record)),
        Some(OffsetStoreControlForm::ProductAnchored {
            leading_value: Some((3, 0x111)),
            values: vec![0x0001_38e0, 7],
        })
    );

    let mut duplicate = control.to_vec();
    duplicate.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    assert!(offset_store_control_form(&duplicate, Some(&first_record)).is_none());

    let aligned_control = 7u32.to_le_bytes();
    let anchored_first_record = b"\x04\x01\x0eNX 2027.3102\0";
    assert!(offset_store_control_form(&aligned_control, Some(anchored_first_record)).is_none());
}
