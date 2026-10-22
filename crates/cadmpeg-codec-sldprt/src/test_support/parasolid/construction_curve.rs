// SPDX-License-Identifier: Apache-2.0
//! Independent neutral-binary wire-curve fixtures.

pub(crate) fn stream(points_mm: &[[f64; 3]]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(229_u16.to_be_bytes());
    body.extend(0_u32.to_be_bytes());
    body.extend([0, 12, 255]);
    body.extend(2_u16.to_be_bytes());
    body.extend(1_u32.to_be_bytes());
    for _ in 0..6 {
        body.extend(1_u16.to_be_bytes());
    }
    body.extend(1000_f64.to_be_bytes());
    body.extend(1e-8_f64.to_be_bytes());
    for _ in 0..3 {
        body.extend(1_u16.to_be_bytes());
    }
    body.push(1);
    body.extend(1_u16.to_be_bytes());
    body.extend([2, 1]);
    for value in [1_u16, 1, 31, 1, 1, 1, 1] {
        body.extend(value.to_be_bytes());
    }
    body.extend([0, 134, 255]);
    body.extend(31_u16.to_be_bytes());
    body.extend(1_u32.to_be_bytes());
    for value in [1_u16, 2, 1, 1, 1] {
        body.extend(value.to_be_bytes());
    }
    body.push(b'+');
    body.extend(43_u16.to_be_bytes());
    body.extend(1_u16.to_be_bytes());
    body.extend([0, 136, 255]);
    body.extend(43_u16.to_be_bytes());
    body.extend(1_u16.to_be_bytes());
    let count = u32::try_from(points_mm.len()).expect("synthetic count fits u32");
    body.extend(count.to_be_bytes());
    body.extend(3_u16.to_be_bytes());
    body.extend(count.to_be_bytes());
    body.extend([2, 0, 0, 0, 3]);
    for value in [59_u16, 60, 61] {
        body.extend(value.to_be_bytes());
    }
    body.extend([0, 45, 255]);
    body.extend(
        count
            .checked_mul(3)
            .expect("synthetic scalar count fits")
            .to_be_bytes(),
    );
    body.extend(59_u16.to_be_bytes());
    for value in points_mm.iter().flatten() {
        body.extend((value / 1000.0).to_be_bytes());
    }
    body.extend([0, 127, 255]);
    body.extend(count.to_be_bytes());
    body.extend(60_u16.to_be_bytes());
    for index in 0..count {
        body.extend(
            (if index == 0 || index + 1 == count {
                2_u16
            } else {
                1
            })
            .to_be_bytes(),
        );
    }
    body.extend([0, 128, 255]);
    body.extend(count.to_be_bytes());
    body.extend(61_u16.to_be_bytes());
    for index in 0..count {
        body.extend(f64::from(index).to_be_bytes());
    }
    body.extend([0, 1, 0, 1]);
    super::parasolid_with_body("construction curve", "SCH_3201255_32001_13006", &body)
}
