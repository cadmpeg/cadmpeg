// SPDX-License-Identifier: Apache-2.0
//! Synthetic complete A8 sampled parameter-map bytes.

pub(crate) fn payload(count: u8, descending: bool, fit: f64) -> Vec<u8> {
    let mut bytes = vec![0x82, 0x81, 0x82, 0x81, 0x15];
    for value in [0.0, 1.0, fit, 2.0, 3.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    if count < 128 {
        bytes.push(0x80 + count);
    } else {
        bytes.extend_from_slice(&[0x08, count]);
    }
    for co in [false, true] {
        for index in 0..count {
            let parameter = f64::from(index) / f64::from(count - 1);
            let value = if co {
                if descending {
                    3.0 - parameter
                } else {
                    2.0 + parameter
                }
            } else {
                parameter
            };
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    for value in [0.25, 0.75, fit] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(1);
    bytes
}
