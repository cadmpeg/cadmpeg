// SPDX-License-Identifier: Apache-2.0
use super::quotient_third;
use crate::math::sum::{scaled_finite, ExactSignedSum, ScaledValue};
use crate::scalar::FiniteReal;

fn product(factors: [f64; 4]) -> ScaledValue {
    let mut sum = ExactSignedSum::default();
    sum.add_factors(factors);
    sum.finish().expect("nonzero actual extended product")
}

#[test]
fn complete_third_cancels_all_seven_terms_before_extreme_range_division() {
    let width = scaled_finite(1.0).unwrap();
    let least = f64::from_bits(1);
    for value in [product([f64::MAX; 4]), product([least; 4])] {
        // H=W defines C=1. Every actual homogeneous order is equal,
        // independently of whether four extended factors fit ScaledExponent.
        let jet = [Some(value); 4];
        assert_eq!(quotient_third(jet, jet, width), Some(Ok(FiniteReal::ZERO)));
    }
}

#[test]
fn polynomial_third_uses_real_higher_order_without_intermediate_range_loss() {
    let width = scaled_finite(1.0).unwrap();
    for sign in [-1.0, 1.0] {
        for exponent in [-1000, 1000] {
            let scale = sign * 2.0_f64.powi(exponent);
            let weight = product([scale, scale.abs(), scale.abs(), scale.abs()]);
            // W is constant and H'''=W. C'''=1 even when H'''*W^3
            // has exponent +/-16000, outside the original scaled domain.
            assert_eq!(quotient_third([None, None, None, Some(weight)],
                [Some(weight), None, None, None], width), Some(Ok(FiniteReal::ONE)));
        }
    }
}

#[test]
fn linear_weight_and_constant_numerator_have_the_source_quotient_third() {
    let width = scaled_finite(2.0).unwrap();
    for common in [-2.0_f64.powi(900), -1.0, 1.0, 2.0_f64.powi(900)] {
        // H=common, W=common*(1+s): C'''=-6/(1+s)^4.
        // At s=0 and physical width2, the result is -6/8=-.75.
        let value = scaled_finite(common);
        assert_eq!(quotient_third([value, None, None, None], [value, value, None, None], width)
            .unwrap().map(FiniteReal::get), Ok(-0.75));
    }
}

#[test]
fn third_keeps_final_subnormal_ties_signs_zero_weight_and_true_overflow() {
    let one = scaled_finite(1.0).unwrap();
    let least = f64::from_bits(1);
    for sign in [-1.0, 1.0] {
        let weight = [scaled_finite(2.0), None, None, None];
        let half = quotient_third([None, None, None, scaled_finite(sign * least)], weight, one)
            .unwrap().unwrap().get();
        assert_eq!(half.to_bits(), (sign * 0.0).to_bits());
        let three_halves = quotient_third([None, None, None, scaled_finite(sign * 3.0 * least)],
            weight, one).unwrap().unwrap().get();
        assert_eq!(three_halves, sign * f64::from_bits(2));
        assert_eq!(quotient_third([None, None, None, scaled_finite(sign * f64::MAX)],
            [scaled_finite(0.5), None, None, None], one), Some(Err(sign * f64::INFINITY)));
    }
    assert!(quotient_third([Some(one); 4], [None; 4], one).is_none());
    assert_eq!(quotient_third([None; 4], [Some(one); 4], one), Some(Ok(FiniteReal::ZERO)));
}
