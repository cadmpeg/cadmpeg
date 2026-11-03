// SPDX-License-Identifier: Apache-2.0
//! Structural class-32 fixtures with independent limit and scalar lanes.

#[derive(Clone, Copy)]
pub(crate) enum TailForm {
    Absent79,
    Absent87,
    Explicit,
}

pub(crate) fn fixture(count: u32, wide_marker: bool, form: TailForm) -> (Vec<u8>, usize) {
    assert!(count >= 2);
    let mut bytes = super::test_a5a8::a5_freeform_curve_stream_with_count(count);
    bytes.truncate(bytes.len() - 79);
    if wide_marker {
        let prefix = super::test_bytes::compact_uint_bytes(count).len() * 2 + 1;
        let marker = 8 + prefix;
        assert_eq!(bytes[marker], 0x0c);
        bytes.splice(marker..=marker, [0x08, 0x09]);
    }
    let tail_start = bytes.len();
    for value in [0.25_f64, 0.75, -0.0, 2.0, 2.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x03);
    for value in [-0.1_f64, 2.1] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    match form {
        TailForm::Absent79 | TailForm::Absent87 => bytes.extend_from_slice(&[5, 0, 0, 0, 0, 0]),
        TailForm::Explicit => {
            bytes.extend_from_slice(&[1, 1]);
            for i in 0..count {
                bytes.extend_from_slice(&f64::from(count - i).to_le_bytes());
            }
            for _ in 0..count {
                bytes.extend_from_slice(&(-1.0_f64).to_le_bytes());
            }
            for _ in 0..count {
                bytes.extend_from_slice(&(-0.0_f64).to_le_bytes());
            }
            bytes.extend_from_slice(&1u32.to_le_bytes());
        }
    }
    for value in [0.0_f64, f64::from(count - 1)] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    if !matches!(form, TailForm::Absent79) {
        bytes.extend_from_slice(&2.0_f64.to_le_bytes());
    }
    let size = u32::try_from(bytes.len() - 8).expect("fixture payload fits");
    bytes[3..7].copy_from_slice(&size.to_le_bytes());
    (bytes, tail_start)
}
