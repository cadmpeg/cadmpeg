// SPDX-License-Identifier: Apache-2.0
use super::quotient_second;
use crate::math::sum::{scaled_finite, ExactSignedSum};
use crate::scalar::FiniteReal;

#[test]
fn second_quotient_cancels_complete_extreme_homogeneous_orders() {
    let unit = scaled_finite(1.0).unwrap();
    for factor in [f64::MAX, f64::from_bits(1)] {
        let mut sum = ExactSignedSum::default();
        sum.add_factors([factor; 4]);
        let jet = [Some(sum.finish().unwrap()); 3];
        // H=W defines C=1 even outside the binary64 exponent range.
        assert_eq!(quotient_second(jet, jet, unit, []), Some(Ok(FiniteReal::ZERO)));
        assert_eq!(quotient_second([None, None, jet[0]], [jet[0], None, None], unit, []),
            Some(Ok(FiniteReal::ONE)));
    }
}

#[test]
fn second_quotient_follows_independent_leibniz_and_physical_width_laws() {
    for w in [[2.0, 1.0, -3.0], [-2.0, 3.0, 4.0], [4.0, -2.0, 1.0]] {
        for impulse in 0..3 {
            let h: [f64; 3] = std::array::from_fn(|n| if n == impulse { 1.0 } else { 0.0 });
            // Differentiate W*C=H, rather than replay the four-term numerator.
            let c0 = h[0] / w[0];
            let c1 = (h[1] - w[1] * c0) / w[0];
            let c2 = (h[2] - 2.0 * w[1] * c1 - w[2] * c0) / w[0];
            for width in [-4.0_f64, 1.0, 2.0] {
                let expected = c2 / (width * width);
                for common in [-2.0_f64.powi(900), -1.0, 1.0, 2.0_f64.powi(900)] {
                    let actual = quotient_second(h.map(|v| scaled_finite(v * common)),
                        w.map(|v| scaled_finite(v * common)), scaled_finite(width).unwrap(), []);
                    assert_eq!(actual.unwrap().unwrap().get(), expected);
                }
            }
        }
    }
}

#[test]
fn second_quotient_keeps_final_subnormal_sign_ties_zero_weight_and_overflow() {
    let unit = scaled_finite(1.0).unwrap();
    let least = f64::from_bits(1);
    for sign in [-1.0, 1.0] {
        let w = [scaled_finite(2.0), None, None];
        let half = quotient_second([None, None, scaled_finite(sign * least)], w, unit, [])
            .unwrap().unwrap().get();
        assert_eq!(half.to_bits(), (sign * 0.0).to_bits());
        let three_halves = quotient_second([None, None, scaled_finite(sign * 3.0 * least)], w, unit, [])
            .unwrap().unwrap().get();
        assert_eq!(three_halves, sign * f64::from_bits(2));
        assert_eq!(quotient_second([None, None, scaled_finite(sign * f64::MAX)],
            [scaled_finite(0.5), None, None], unit, []), Some(Err(sign * f64::INFINITY)));
    }
    assert!(quotient_second([Some(unit); 3], [None; 3], unit, []).is_none());
    assert_eq!(quotient_second([None; 3], [Some(unit); 3], unit, []), Some(Ok(FiniteReal::ZERO)));
}
