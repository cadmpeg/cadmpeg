// SPDX-License-Identifier: Apache-2.0
use super::{
    scaled_finite, ExactSignedSum, MAX_SCALED_EXPONENT, MIN_SCALED_EXPONENT,
    MIN_SIGNIFICAND_EXPONENT,
};
use crate::scalar::FiniteReal;

#[test]
fn a_zero_value_or_denominator_makes_every_ratio_product_zero() {
    let real = |value| FiniteReal::new(value).expect("finite test value");
    for [value, denominator] in [[0.0, 1.0], [1.0, 0.0]] {
        assert_eq!(
            super::scaled_ratio_products(real(value), real(denominator), [real(2.0)])
                .map(|products| products.map(FiniteReal::get)),
            Some([0.0])
        );
    }
}

#[test]
fn scaled_value_retains_a_finite_result_with_a_large_frame_shift() {
    let high = scaled_finite(f64::MAX).expect("finite value");
    let frame = scaled_finite(0.5).expect("finite frame").exponent;
    assert_eq!(high.scaled_by(frame), f64::MAX);
}

#[test]
fn exact_dot_rounds_subnormal_ties_using_the_full_product_sum() {
    let half_minimum = 2.0_f64.powi(-537) * 2.0_f64.powi(-538);
    assert_eq!(half_minimum, 0.0);
    let coefficients = [2.0_f64.powi(-537), 2.0_f64.powi(-564)];
    let components = [2.0_f64.powi(-538), 2.0_f64.powi(-564)];
    assert_eq!(
        super::finite_dot(coefficients, components)
            .ok()
            .map(FiniteReal::get),
        Some(f64::from_bits(1))
    );
    assert_eq!(
        super::finite_dot([-coefficients[0], -coefficients[1]], components)
            .ok()
            .map(FiniteReal::get),
        Some(-f64::from_bits(1))
    );
    assert_eq!(
        super::finite_dot([coefficients[0], 0.0], components)
            .ok()
            .map(FiniteReal::get),
        Some(0.0)
    );
}

#[test]
fn every_exponent_the_scaled_value_constructors_produce_stays_inside_the_stated_range() {
    // The two ends `scaled_finite` reaches: the smallest positive subnormal and
    // the largest finite magnitude.
    let smallest_subnormal = f64::from_bits(1);
    let low = scaled_finite(smallest_subnormal).expect("finite value");
    let high = scaled_finite(f64::MAX).expect("finite value");
    assert_eq!(low.exponent.0, MIN_SIGNIFICAND_EXPONENT + 1);
    assert_eq!(high.exponent.0, 1024);

    // The two ends `ExactSignedSum::finish` reaches: three products of the
    // largest finite magnitude, and one product of two smallest subnormals.
    let mut largest = ExactSignedSum::default();
    for _ in 0..3 {
        largest.add_product(f64::MAX, f64::MAX);
    }
    let largest = largest.finish().expect("nonzero sum");
    let mut smallest = ExactSignedSum::default();
    smallest.add_product(smallest_subnormal, smallest_subnormal);
    let smallest = smallest.finish().expect("nonzero sum");

    let range = MIN_SCALED_EXPONENT..=MAX_SCALED_EXPONENT;
    for value in [low, high, largest, smallest] {
        assert!(
            range.contains(&value.exponent.0),
            "exponent {} left {MIN_SCALED_EXPONENT}..={MAX_SCALED_EXPONENT}",
            value.exponent.0
        );
    }

    // The difference of any two of them is a plain subtraction inside the
    // span the range states, at both signs.
    let span = MAX_SCALED_EXPONENT - MIN_SCALED_EXPONENT;
    for left in [low, high, largest, smallest] {
        for right in [low, high, largest, smallest] {
            let difference = left.exponent.difference(right.exponent);
            assert!(
                (-span..=span).contains(&difference),
                "difference {difference} left -{span}..={span}"
            );
        }
    }

    // `apply_normal` scales every value by the maximum exponent of the set, so
    // the difference the subtraction states is never positive and the power of
    // two it feeds `powi` is in `(0.0, 1.0]`.
    let scale_exponent = [low, high, largest, smallest]
        .iter()
        .map(|value| value.exponent)
        .max()
        .expect("nonempty set");
    for value in [low, high, largest, smallest] {
        assert!(value.exponent.difference(scale_exponent) <= 0);
        let scaled = value.scaled_by(scale_exponent);
        assert!(scaled.is_finite(), "scaled value {scaled} is not finite");
    }
}

#[test]
fn an_overflowing_quotient_value_or_dot_product_carries_the_value_it_reached() {
    let large = scaled_finite(f64::MAX).expect("finite value");
    let half = scaled_finite(0.5).expect("finite value");
    let negative_half = scaled_finite(-0.5).expect("finite value");
    assert_eq!(large.quotient(half), Err(f64::INFINITY));
    assert_eq!(large.quotient(negative_half), Err(f64::NEG_INFINITY));
    assert_eq!(
        large.quotient(scaled_finite(4.0).expect("finite value")),
        Ok(FiniteReal::new(f64::MAX / 4.0).expect("finite quotient"))
    );
    assert_eq!(large.quotient_by_factors([half], false), Err(f64::INFINITY));
    assert_eq!(
        large.quotient_by_factors([half, half], true),
        Err(f64::INFINITY)
    );
    let mut sum = ExactSignedSum::default();
    sum.add_product(f64::MAX, -2.0);
    assert_eq!(
        sum.finish().expect("nonzero sum").finite(),
        Err(f64::NEG_INFINITY)
    );
    // The plain left-to-right sum of the products.
    assert_eq!(
        super::finite_dot([1.0, 1.0], [f64::MAX, f64::MAX]),
        Err(f64::INFINITY)
    );
    assert!(super::finite_dot([1.0, 0.0], [1.0, f64::INFINITY]).is_err_and(f64::is_nan));
    assert_eq!(
        super::finite_dot([1.0, -1.0], [f64::MAX, f64::MAX]),
        Ok(FiniteReal::ZERO)
    );
}
