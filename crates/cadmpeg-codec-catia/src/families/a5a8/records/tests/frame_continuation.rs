// SPDX-License-Identifier: Apache-2.0
//! Continuation bounds over the original consolidated curve bytes.

use crate::test_support::test_bytes::{compact_uint_bytes, le_f64};

fn a5_freeform_curve_stream() -> Vec<u8> {
    a5_freeform_curve_stream_with_count(2)
}

fn a5_freeform_curve_stream_with_count(count: u32) -> Vec<u8> {
    let mut payload = compact_uint_bytes(count);
    payload.push(21);
    payload.extend_from_slice(&compact_uint_bytes(count));
    payload.push(0x0c);
    for site in 0..count {
        payload.extend_from_slice(&le_f64(f64::from(site)));
    }
    for block in 0..3 {
        for site in 0..count {
            let radius = if site == 0 { 1.0 } else { 2.0 };
            let values = if block == 0 {
                [
                    radius,
                    0.0,
                    0.0,
                    0.0,
                    radius,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    std::f64::consts::FRAC_PI_2,
                ]
            } else {
                [0.0; 10]
            };
            for value in values {
                payload.extend_from_slice(&le_f64(value));
            }
        }
    }
    let mut record = vec![0xa5, 0x03, 0x32];
    record.extend_from_slice(
        &(u32::try_from(payload.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    record.push(0x05);
    record.extend_from_slice(&payload);
    record
}

#[test]
fn a5_curve_parser_accepts_frame_bounded_continuation() {
    let mut bytes = a5_freeform_curve_stream();
    bytes.extend(std::iter::repeat_n(0, 4097));
    let payload_len = u32::try_from(bytes.len() - 8).expect("test frame fits u32");
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    let [curve] = crate::families::a5a8::records::a5_freeform_curves(&bytes)
        .try_into()
        .expect("one rolling-ball jet");
    assert_eq!(curve.knots(), [0.0, 1.0]);
    assert_eq!(curve.sites[1].site.radius(), 2.0);
}
