// SPDX-License-Identifier: Apache-2.0
use super::*;

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
