// SPDX-License-Identifier: Apache-2.0
use super::quotient_fourth;
use crate::math::sum::{scaled_finite, ExactSignedSum, ScaledValue};
use crate::scalar::FiniteReal;

fn product(factors: [f64; 4]) -> ScaledValue {
    let mut sum = ExactSignedSum::default();
    sum.add_factors(factors);
    sum.finish().expect("nonzero actual extended product")
}

#[test]
fn complete_fourth_cancels_all_twelve_terms_before_extreme_range_division() {
    let width = scaled_finite(1.0).unwrap();
    for value in [product([f64::MAX; 4]), product([f64::from_bits(1); 4])] {
        // H=W defines C=1. Every actual order has the same extended value.
        let jet = [Some(value); 5];
        assert_eq!(quotient_fourth(jet, jet, width, []), Some(Ok(FiniteReal::ZERO)));
    }
}

#[test]
fn polynomial_fourth_keeps_five_factor_numerator_outside_the_scaled_domain() {
    let width = scaled_finite(1.0).unwrap();
    for sign in [-1.0, 1.0] {
        for exponent in [-1000, 1000] {
            let scale = sign * 2.0_f64.powi(exponent);
            let weight = product([scale, scale.abs(), scale.abs(), scale.abs()]);
            assert_eq!(quotient_fourth([None, None, None, None, Some(weight)],
                [Some(weight), None, None, None, None], width, []), Some(Ok(FiniteReal::ONE)));
        }
    }
}

#[test]
fn fourth_expansion_matches_the_independent_quotient_recurrence() {
    let width = scaled_finite(1.0).unwrap();
    for weight in [-2.0, 1.0, 4.0] {
        for tail in [[0.0; 4], [1.0; 4], [-2.0, 3.0, -1.0, 2.0], [0.0, 2.0, 0.0, -3.0]] {
            let w = [weight, tail[0], tail[1], tail[2], tail[3]];
            for axis in 0..5 {
                let mut h = [0.0; 5]; h[axis] = 1.0;
                let mut c = [0.0; 5];
                for (n, coefficients) in [[0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0],
                    [2.0, 1.0, 0.0, 0.0, 0.0], [3.0, 3.0, 1.0, 0.0, 0.0],
                    [4.0, 6.0, 4.0, 1.0, 0.0]].into_iter().enumerate()
                {
                    c[n] = (h[n] - (1..=n).map(|k| coefficients[k - 1] * w[k] * c[n - k]).sum::<f64>()) / w[0];
                }
                let actual = quotient_fourth(h.map(scaled_finite), w.map(scaled_finite), width, []).unwrap().unwrap().get();
                assert_eq!(actual, c[4]);
            }
        }
    }
}

#[test]
fn fourth_keeps_final_subnormal_ties_signs_zero_weight_and_true_overflow() {
    let one = scaled_finite(1.0).unwrap();
    let least = f64::from_bits(1);
    for sign in [-1.0, 1.0] {
        let weight = [scaled_finite(2.0), None, None, None, None];
        let half = quotient_fourth([None, None, None, None, scaled_finite(sign * least)], weight, one, [])
            .unwrap().unwrap().get();
        assert_eq!(half.to_bits(), (sign * 0.0).to_bits());
        assert_eq!(quotient_fourth([None, None, None, None, scaled_finite(sign * 3.0 * least)], weight, one, [])
            .unwrap().unwrap().get(), sign * f64::from_bits(2));
        assert_eq!(quotient_fourth([None, None, None, None, scaled_finite(sign * f64::MAX)],
            [scaled_finite(0.5), None, None, None, None], one, []), Some(Err(sign * f64::INFINITY)));
        // H=common, W=common*(1+s): C4=24/(1+s)^5, physical width2.
        let common = scaled_finite(sign * 2.0_f64.powi(900));
        assert_eq!(quotient_fourth([common, None, None, None, None], [common, common, None, None, None],
            scaled_finite(2.0).unwrap(), []).unwrap().unwrap().get(), 1.5);
    }
    assert!(quotient_fourth([Some(one); 5], [None; 5], one, []).is_none());
    assert_eq!(quotient_fourth([None; 5], [Some(one); 5], one, []), Some(Ok(FiniteReal::ZERO)));
}
