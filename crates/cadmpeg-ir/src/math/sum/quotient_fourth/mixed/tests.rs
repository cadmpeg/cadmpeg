// SPDX-License-Identifier: Apache-2.0
use super::quotient_fourth_partials;
use crate::math::sum::{scaled_finite, ExactSignedSum};
use crate::scalar::FiniteReal;

#[test]
fn mixed_fourth_follows_reciprocal_linear_weight_and_physical_axis_laws() {
    let widths = [scaled_finite(2.0).unwrap(), scaled_finite(4.0).unwrap()];
    for common in [-2.0_f64.powi(900), -1.0, 1.0, 2.0_f64.powi(900)] {
        // C=1/(1+u+v): all order-4 local derivatives equal 24.
        // Each physical partial divides by its actual u/v width factors.
        let mut h = [None; 15]; h[0] = scaled_finite(common);
        let mut w = [None; 15]; w[..3].fill(scaled_finite(common));
        let actual = quotient_fourth_partials(h, w, widths).unwrap();
        assert_eq!(actual.map(|lane| lane.unwrap().get()), [1.5, 0.75, 0.375, 0.1875, 0.09375]);
    }
}

#[test]
fn mixed_fourth_cancels_complete_extreme_homogeneous_orders() {
    let width = scaled_finite(1.0).unwrap();
    for factor in [f64::MAX, f64::from_bits(1)] {
        let mut sum = ExactSignedSum::default(); sum.add_factors([factor; 4]);
        let value = sum.finish().unwrap();
        // Every jet H=W represents the exact constant C=1.
        let jet = [Some(value); 15];
        assert_eq!(quotient_fourth_partials(jet, jet, [width; 2]),
            Some([Ok(FiniteReal::ZERO); 5]));
    }
}

#[test]
fn mixed_fourth_keeps_only_actual_polynomial_partial_and_own_range() {
    let widths = [scaled_finite(2.0).unwrap(), scaled_finite(4.0).unwrap()];
    for sign in [-1.0, 1.0] {
        // C=u^2*v^2: the sole order-4 partial is 4 in local coordinates.
        let mut h = [None; 15]; h[12] = scaled_finite(sign * 4.0);
        let mut w = [None; 15]; w[0] = scaled_finite(sign);
        let mut expected = [Ok(FiniteReal::ZERO); 5];
        expected[2] = Ok(FiniteReal::new(0.0625).unwrap());
        assert_eq!(quotient_fourth_partials(h, w, widths), Some(expected));
        let one = scaled_finite(1.0).unwrap();
        w[0] = scaled_finite(0.5); h[12] = scaled_finite(sign * f64::MAX);
        let actual = quotient_fourth_partials(h, w, [one; 2]).unwrap();
        assert_eq!(actual[2], Err(sign * f64::INFINITY));
        for (lane, actual) in actual.into_iter().enumerate() { if lane != 2 { assert_eq!(actual, Ok(FiniteReal::ZERO)); } }
        w[0] = scaled_finite(2.0); h[12] = scaled_finite(sign * f64::from_bits(1));
        let actual = quotient_fourth_partials(h, w, [one; 2]).unwrap()[2].unwrap().get();
        assert_eq!(actual.to_bits(), (sign * 0.0).to_bits());
        h[12] = scaled_finite(sign * 3.0 * f64::from_bits(1));
        assert_eq!(quotient_fourth_partials(h, w, [one; 2]).unwrap()[2].unwrap().get(),
            sign * f64::from_bits(2));
    }
    let one = scaled_finite(1.0).unwrap();
    assert!(quotient_fourth_partials([Some(one); 15], [None; 15], [one; 2]).is_none());
}

#[test]
fn mixed_fourth_follows_complete_polynomial_quotient_jet_and_transpose() {
    // H/W at zero, with authored polynomial coefficients H_ab/(a!b!)
    // and W_ab/(a!b!). Differentiate H=W*C recursively; physical widths2/4
    // give [-59/64,117/256,3/512,3/256,-9/1024]. All W orders are nonzero.
    for transpose in [false, true] {
        let h = if transpose { [1.0, 3.0, 2.0, 6.0, 5.0, 4.0, 10.0, 9.0, 8.0, 7.0, 15.0, 14.0, 13.0, 12.0, 11.0] }
            else { [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0] };
        let w = if transpose { [2.0, 3.0, -1.0, 1.0, -2.0, 2.0, -1.0, 2.0, -3.0, 4.0, -6.0, 5.0, -4.0, 3.0, -2.0] }
            else { [2.0, -1.0, 3.0, 2.0, -2.0, 1.0, 4.0, -3.0, 2.0, -1.0, -2.0, 3.0, -4.0, 5.0, -6.0] };
        let mut widths = [2.0, 4.0];
        let mut expected = [-59.0 / 64.0, 117.0 / 256.0, 3.0 / 512.0, 3.0 / 256.0, -9.0 / 1024.0];
        if transpose { widths.reverse(); expected.reverse(); }
        for common in [-2.0_f64.powi(900), -1.0, 1.0, 2.0_f64.powi(900)] {
            let actual = quotient_fourth_partials(h.map(|v| scaled_finite(v * common)),
                w.map(|v| scaled_finite(v * common)), widths.map(|v| scaled_finite(v).unwrap())).unwrap();
            assert_eq!(actual.map(|v| v.unwrap().get()), expected);
        }
    }
}
