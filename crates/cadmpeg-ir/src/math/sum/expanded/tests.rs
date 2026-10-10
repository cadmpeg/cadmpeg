// SPDX-License-Identifier: Apache-2.0
use crate::math::sum::{quotient_second::quotient_second, quotient_third::quotient_third,
    quotient_fourth::quotient_fourth, scaled_finite, ExactSignedSum, ScaledValue};
use crate::scalar::FiniteReal;

// At most sixteen normalized factor operations, original numerator rounding
// and the independently evaluated source ratio each contribute roundoff.
const EPS_PHASE_FACTOR_RATIO: f64 = 64.0 * f64::EPSILON;
type Outcome = Option<Result<FiniteReal, f64>>;

fn constant_weight_orders<const N: usize>(h: Option<ScaledValue>, w: Option<ScaledValue>,
    factors: [ScaledValue; N]) -> [Outcome; 3]
{
    let unit = scaled_finite(1.0).unwrap();
    [quotient_second([None, None, h], [w, None, None], unit, factors),
        quotient_third([None, None, None, h], [w, None, None, None], unit, factors),
        quotient_fourth([None, None, None, None, h], [w, None, None, None, None], unit, factors)]
}

fn phase_orders(h: f64, w: f64, rate: f64) -> [Outcome; 3] {
    let (h, w, unit, rate) = (scaled_finite(h), scaled_finite(w),
        scaled_finite(1.0).unwrap(), scaled_finite(rate).unwrap());
    [quotient_second([None, None, h], [w, None, None], unit, [rate; 3]),
        quotient_third([None, None, None, h], [w, None, None, None], unit, [rate; 4]),
        quotient_fourth([None, None, None, None, h], [w, None, None, None, None], unit, [rate; 5])]
}

fn near(actual: Outcome, expected: f64) {
    let actual = actual.unwrap().unwrap().get();
    if expected == 0.0 { assert_eq!(actual, expected); }
    else { assert!((actual / expected - 1.0).abs() <= EPS_PHASE_FACTOR_RATIO,
        "{actual} versus source ratio {expected}"); }
}

#[test]
fn normalized_quotient_factors_recover_true_final_range_before_admission() {
    let least = f64::from_bits(1);
    for h_sign in [-1.0, 1.0] {
        for w_sign in [-1.0, 1.0] {
            for rate_sign in [-1.0, 1.0] {
                // Constant W gives C^(m)=H^(m)/W. The actual unscaled
                // quotient is outside binary64; phase rate^n rescues it.
                let h = h_sign * f64::MAX;
                let w = w_sign * least;
                for value in constant_weight_orders(scaled_finite(h), scaled_finite(w), []) {
                    assert_eq!(value, Some(Err(h_sign * w_sign * f64::INFINITY)));
                }
                let actual = phase_orders(h, w, rate_sign * 2.0_f64.powi(-400));
                for (at, value) in actual.into_iter().enumerate() {
                    let n = i32::try_from(at).unwrap() + 3;
                    let expected = h_sign * w_sign * rate_sign.powi(n)
                        * (f64::MAX * 2.0_f64.powi(1074 - 400 * n));
                    near(value, expected);
                }
                // Conversely, H/W rounds to zero before the actual phase
                // factor is included. Each expected denominator step is
                // normal and finite; no intermediate expected value is lost.
                let h = h_sign * least;
                let w = w_sign * f64::MAX;
                for value in constant_weight_orders(scaled_finite(h), scaled_finite(w), []) {
                    assert_eq!(value.unwrap().unwrap().get().to_bits(), (h_sign * w_sign * 0.0).to_bits());
                }
                let actual = phase_orders(h, w, rate_sign * 2.0_f64.powi(400));
                for (at, value) in actual.into_iter().enumerate() {
                    let n = i32::try_from(at).unwrap() + 3;
                    let mut denominator = f64::MAX;
                    for _ in 0..n { denominator *= 2.0_f64.powi(-400); }
                    let expected = h_sign * w_sign * rate_sign.powi(n) * (least / denominator);
                    assert!(expected != 0.0 && expected.is_finite());
                    near(value, expected);
                }
            }
        }
    }
}

#[test]
fn normalized_quotient_factors_follow_independent_leibniz_and_phase_laws() {
    const BINOMIAL: [[f64; 5]; 5] = [[0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0],
        [2.0, 1.0, 0.0, 0.0, 0.0], [3.0, 3.0, 1.0, 0.0, 0.0],
        [4.0, 6.0, 4.0, 1.0, 0.0]];
    for w in [[2.0, 1.0, -3.0, 2.0, 4.0], [-2.0, 3.0, 4.0, -1.0, 2.0],
        [4.0, -2.0, 1.0, 3.0, -4.0]]
    {
        for impulse in 0..5 {
            let h: [f64; 5] = std::array::from_fn(|at| if at == impulse { 1.0 } else { 0.0 });
            let mut c = [0.0; 5];
            for n in 0..5 {
                c[n] = (h[n] - (1..=n).map(|k| BINOMIAL[n][k - 1] * w[k] * c[n - k]).sum::<f64>()) / w[0];
            }
            for width in [-4.0_f64, 1.0, 2.0] {
                for rate in [-2.0_f64, 0.5, 1.0, 2.0] {
                    let (scale, span) = (scaled_finite(rate).unwrap(), scaled_finite(width).unwrap());
                    let actual = [
                        quotient_second(std::array::from_fn(|n| scaled_finite(h[n])),
                            std::array::from_fn(|n| scaled_finite(w[n])), span, [scale; 3]),
                        quotient_third(std::array::from_fn(|n| scaled_finite(h[n])),
                            std::array::from_fn(|n| scaled_finite(w[n])), span, [scale; 4]),
                        quotient_fourth(h.map(scaled_finite), w.map(scaled_finite), span, [scale; 5]),
                    ];
                    for (at, value) in actual.into_iter().enumerate() {
                        let order = i32::try_from(at).unwrap() + 2;
                        near(value, c[at + 2] * rate.powi(order + 1) / width.powi(order));
                    }
                }
            }
        }
    }
}

#[test]
fn normalized_quotient_factors_keep_cancellation_sign_ties_and_true_final_failures() {
    let unit = scaled_finite(1.0).unwrap();
    let least = f64::from_bits(1);
    for value in [f64::MAX, least] {
        let mut sum = ExactSignedSum::default(); sum.add_factors([value; 4]);
        let extended = Some(sum.finish().unwrap());
        let rate = scaled_finite(value).unwrap();
        assert_eq!(quotient_second([extended; 3], [extended; 3], unit, [rate; 3]), Some(Ok(FiniteReal::ZERO)));
        assert_eq!(quotient_third([extended; 4], [extended; 4], unit, [rate; 4]), Some(Ok(FiniteReal::ZERO)));
        assert_eq!(quotient_fourth([extended; 5], [extended; 5], unit, [rate; 5]), Some(Ok(FiniteReal::ZERO)));
    }
    for sign in [-1.0, 1.0] {
        for value in constant_weight_orders(scaled_finite(sign * least), scaled_finite(2.0),
            [scaled_finite(2.0).unwrap(), unit, unit])
        {
            assert_eq!(value.unwrap().unwrap().get().to_bits(), (sign * least).to_bits());
        }
        for value in phase_orders(sign * f64::MAX, 1.0, 2.0) {
            assert_eq!(value, Some(Err(sign * f64::INFINITY)));
        }
    }
    assert_eq!(constant_weight_orders(Some(unit), None, [scaled_finite(f64::MAX).unwrap(); 5]), [None; 3]);
}
