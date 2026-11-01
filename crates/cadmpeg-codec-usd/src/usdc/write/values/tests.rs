// SPDX-License-Identifier: Apache-2.0

use super::half_bits;

#[test]
fn half_bits_round_to_nearest_even() {
    assert_eq!(half_bits(0.0), 0x0000);
    assert_eq!(half_bits(-0.0), 0x8000);
    assert_eq!(half_bits(1.0), 0x3C00);
    assert_eq!(half_bits(-2.0), 0xC000);
    assert_eq!(half_bits(65504.0), 0x7BFF);
    assert_eq!(half_bits(1e6), 0x7C00);
    assert_eq!(half_bits(f64::from(2.0_f32.powi(-24))), 0x0001);
    assert_eq!(half_bits(1.0 + f64::from(2.0_f32.powi(-11))), 0x3C00);
    assert_eq!(half_bits(1.0 + 3.0 * f64::from(2.0_f32.powi(-11))), 0x3C02);
}
