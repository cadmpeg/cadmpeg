// SPDX-License-Identifier: Apache-2.0
use super::{quotient_fifth, quotient_fifth_partials};
use crate::math::sum::{scaled_finite, ExactSignedSum, ScaledValue};
use crate::scalar::FiniteReal;

fn product(factors: [f64; 4]) -> ScaledValue {
    let mut sum = ExactSignedSum::default();
    sum.add_factors(factors);
    sum.finish().expect("nonzero actual extended product")
}

#[test]
fn fifth_quotient_cancels_complete_extreme_range_numerators() {
    let one = scaled_finite(1.0).unwrap();
    for value in [product([f64::MAX; 4]), product([f64::from_bits(1); 4])] {
        // H=W states C=1, including every mixed derivative.
        let jet = [Some(value); 21];
        assert_eq!(quotient_fifth_partials(jet, jet, [one; 2]), Some([Ok(FiniteReal::ZERO); 6]));
    }
    for sign in [-1.0, 1.0] {
        for exponent in [-1000, 1000] {
            let scale = sign * 2.0_f64.powi(exponent);
            let weight = product([scale, scale.abs(), scale.abs(), scale.abs()]);
            let mut h = [None; 21]; h[15] = Some(weight);
            let mut w = [None; 21]; w[0] = Some(weight);
            let mut expected = [Ok(FiniteReal::ZERO); 6]; expected[0] = Ok(FiniteReal::ONE);
            assert_eq!(quotient_fifth_partials(h, w, [one; 2]), Some(expected));
        }
    }
}

#[test]
fn fifth_quotient_follows_linear_homogeneous_mixed_and_span_laws() {
    let widths = [2.0, 4.0];
    let mut w = [None; 21];
    w[..3].fill(scaled_finite(1.0));
    for (coordinate, expected) in [(1, [120.0, 96.0, 72.0, 48.0, 24.0, 0.0]),
        (2, [0.0, 24.0, 48.0, 72.0, 96.0, 120.0])] {
        // H=u or v, W=1+u+v: fifth Taylor term H*(u+v)^4.
        let mut h = [None; 21]; h[coordinate] = scaled_finite(1.0);
        let actual = quotient_fifth_partials(h, w, widths.map(|x| scaled_finite(x).unwrap())).unwrap();
        for (order, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
            let v_order = i32::try_from(order).unwrap();
            assert_eq!(actual.unwrap().get(), expected / (widths[0].powi(5 - v_order) * widths[1].powi(v_order)));
        }
    }
    for sign in [-1.0, 1.0] {
        for exponent in [-900, 900] {
            let common = scaled_finite(sign * 2.0_f64.powi(exponent));
            let mut h = [None; 21]; h[0] = common;
            let mut w = [None; 21]; w[..3].fill(common);
            // Common H/W scaling cancels: d^5(1/(1+u+v))=-120.
            for actual in quotient_fifth_partials(h, w, [scaled_finite(2.0).unwrap(); 2]).unwrap() {
                assert_eq!(actual.unwrap().get(), -3.75);
            }
        }
    }
}

#[test]
fn fifth_complete_tables_match_independent_bivariate_product_recurrence() {
    const BINOMIAL: [[f64; 6]; 6] = [
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        [1.0, 2.0, 1.0, 0.0, 0.0, 0.0], [1.0, 3.0, 3.0, 1.0, 0.0, 0.0],
        [1.0, 4.0, 6.0, 4.0, 1.0, 0.0], [1.0, 5.0, 10.0, 10.0, 5.0, 1.0],
    ];
    let index = |u: usize, v: usize| (u + v) * (u + v + 1) / 2 + v;
    let tail = [2.0, -1.0, 3.0, 2.0, -2.0, 1.0, 4.0, -3.0, 2.0, -1.0,
        -2.0, 3.0, -4.0, 5.0, -6.0, 7.0, -8.0, 9.0, -10.0, 11.0, -12.0];
    for weight in [-2.0, 1.0, 4.0] {
        let mut w = tail; w[0] = weight;
        for coordinate in 0..21 {
            let mut h = [0.0; 21]; h[coordinate] = 1.0;
            let mut c = [0.0; 21];
            for degree in 0..=5 {
                for v in 0..=degree {
                    let u = degree - v;
                    // Leibniz W*C=H. Every proper predecessor has lower degree.
                    let mut numerator = h[index(u, v)];
                    for a in 0..=u {
                        for b in 0..=v {
                            if a + b != 0 {
                                numerator -= BINOMIAL[u][a] * BINOMIAL[v][b] * w[index(a, b)] * c[index(u - a, v - b)];
                            }
                        }
                    }
                    c[index(u, v)] = numerator / w[0];
                }
            }
            let widths = [scaled_finite(2.0).unwrap(), scaled_finite(4.0).unwrap()];
            let actual = quotient_fifth_partials(h.map(scaled_finite), w.map(scaled_finite), widths).unwrap();
            for (v, actual) in actual.into_iter().enumerate() {
                let power = i32::try_from(5 + v).unwrap();
                assert_eq!(actual.unwrap().get(), c[index(5 - v, v)] / 2.0_f64.powi(power));
            }
        }
    }
}

#[test]
fn fifth_final_subnormal_sign_ties_zero_weight_and_true_overflow() {
    let one = scaled_finite(1.0).unwrap();
    let least = f64::from_bits(1);
    for sign in [-1.0, 1.0] {
        let mut w = [None; 21]; w[0] = scaled_finite(2.0);
        let mut h = [None; 21]; h[15] = scaled_finite(sign * least);
        assert_eq!(quotient_fifth_partials(h, w, [one; 2]).unwrap()[0].unwrap().get().to_bits(), (sign * 0.0).to_bits());
        h[15] = scaled_finite(sign * 3.0 * least);
        assert_eq!(quotient_fifth_partials(h, w, [one; 2]).unwrap()[0].unwrap().get(), sign * f64::from_bits(2));
        h[15] = scaled_finite(sign * f64::MAX); w[0] = scaled_finite(0.5);
        assert_eq!(quotient_fifth_partials(h, w, [one; 2]).unwrap()[0], Err(sign * f64::INFINITY));
    }
    assert!(quotient_fifth_partials([Some(one); 21], [None; 21], [one; 2]).is_none());
    assert_eq!(quotient_fifth_partials([None; 21], [Some(one); 21], [one; 2]), Some([Ok(FiniteReal::ZERO); 6]));
}

#[test]
fn pure_fifth_reads_only_real_six_orders_and_matches_independent_leibniz_laws() {
    let choose = [[1, 0, 0, 0, 0, 0], [1, 1, 0, 0, 0, 0], [1, 2, 1, 0, 0, 0],
        [1, 3, 3, 1, 0, 0], [1, 4, 6, 4, 1, 0], [1, 5, 10, 10, 5, 1]];
    for weights in [[2.0, 1.0, -2.0, 3.0, 4.0, -1.0], [-2.0, 3.0, 4.0, -1.0, 2.0, 5.0]] {
        for impulse in 0..6 {
            let h = std::array::from_fn(|at| if at == impulse { 1.0 } else { 0.0 });
            let mut derivatives = [0.0; 6];
            for n in 0..6 {
                let correction = (1..=n).map(|k| f64::from(choose[n][k]) * weights[k] * derivatives[n - k]).sum::<f64>();
                derivatives[n] = (h[n] - correction) / weights[0];
            }
            let actual = quotient_fifth(h.map(scaled_finite), weights.map(scaled_finite), scaled_finite(2.0).unwrap()).unwrap().unwrap();
            // All independent controls are dyadic; physical fifth divides by2^5.
            assert_eq!(actual.get(), derivatives[5] / 32.0);
        }
    }
}
