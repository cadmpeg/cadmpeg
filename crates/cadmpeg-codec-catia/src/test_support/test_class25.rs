// SPDX-License-Identifier: Apache-2.0
//! Complete B-family parameter-map fixtures.

pub(crate) fn affine_stream() -> Vec<u8> {
    const ACCURACY: f64 = 1.0e-6;
    let mut payload = vec![0x82, 0x05, 0xe7, 0x0a, 0x87, 0x0d];
    for value in [1.0_f64, 2.0, ACCURACY, 1.0, 2.0, 1.0, 0.0, ACCURACY] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    frame(&payload)
}

pub(crate) fn sampled_stream() -> Vec<u8> {
    let mut payload = vec![0x82, 0x09, 0x05, 0x0a, 0x87, 0x15];
    let upper = 18.232_007_915_496_25;
    let accuracy = 0.000_688_399_361_461_999;
    for value in [0.0_f64, upper, accuracy, 0.0, upper] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.push(0x82);
    for value in [0.0_f64, upper, 0.0, upper, accuracy] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    frame(&payload)
}

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![
        0xb2,
        0x03,
        0x25,
        u8::try_from(payload.len()).expect("bounded fixture"),
        0x05,
    ];
    bytes.extend_from_slice(payload);
    bytes
}
