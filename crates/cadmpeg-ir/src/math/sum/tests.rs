// SPDX-License-Identifier: Apache-2.0
use proptest::{prop_assert_eq, proptest};

use super::{
    bit_is_set, rounded_magnitude, scaled_finite, ExactSignedSum, ScaledValue,
    EXACT_PRODUCT_EXPONENT, EXACT_SUM_WORDS, MAX_SCALED_EXPONENT, MIN_SCALED_EXPONENT,
    MIN_SIGNIFICAND_EXPONENT, SUBNORMAL_UNIT_BIT,
};
use crate::scalar::{FiniteReal, NonZeroReal};

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

#[test]
fn a_nonzero_value_has_a_scaled_form_that_states_it_exactly() {
    // Normal and subnormal magnitudes of both signs, the two ends of the
    // finite range among them.
    for value in [
        1.0,
        -3.5,
        f64::MAX,
        -f64::MAX,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        -f64::from_bits(0x000f_ffff_ffff_ffff),
        1.0e-310,
    ] {
        let scaled = ScaledValue::of_nonzero(NonZeroReal::new(value).expect("nonzero value"));
        assert!((0.5..1.0).contains(&scaled.mantissa), "{value:e}");
        assert_eq!(
            scaled.finite().map(|value| value.get().to_bits()),
            Ok(value.to_bits()),
            "{value:e}"
        );
    }
    assert!(scaled_finite(0.0).is_none());
    assert!(scaled_finite(f64::INFINITY).is_none());
}

/// An accumulator magnitude: up to four random words ending at word `top`,
/// with `low_bit` also set when it lies below them.
fn magnitude(top: usize, words: &[u64], low_bit: usize) -> [u64; EXACT_SUM_WORDS] {
    let mut magnitude = [0_u64; EXACT_SUM_WORDS];
    for (offset, word) in words.iter().enumerate() {
        if let Some(index) = top.checked_sub(offset) {
            magnitude[index] = *word;
        }
    }
    magnitude[top] |= 1;
    let lowest_word = top.saturating_sub(words.len());
    if low_bit < lowest_word * 64 {
        magnitude[low_bit / 64] |= 1_u64 << (low_bit % 64);
    }
    magnitude
}

fn highest_bit(magnitude: &[u64; EXACT_SUM_WORDS]) -> usize {
    let word = magnitude
        .iter()
        .rposition(|value| *value != 0)
        .expect("a nonzero magnitude");
    word * 64 + 63 - cadmpeg_core::decode::index_from_u32(magnitude[word].leading_zeros())
}

/// `rounded_magnitude` as it read bit by bit, as `(sign, mantissa, exponent)`.
fn bitwise_rounded_magnitude(
    negative: bool,
    magnitude: &[u64; EXACT_SUM_WORDS],
    highest_bit: u16,
) -> (f64, f64, i32) {
    let sign = if negative { -1.0 } else { 1.0 };
    let keep = (highest_bit + 1).min(53);
    let mut significand = 0_u64;
    for bit in (highest_bit + 1 - keep..=highest_bit).rev() {
        significand = (significand << 1) | u64::from(bit_is_set(magnitude, usize::from(bit)));
    }
    if keep == 53 {
        let guard = highest_bit
            .checked_sub(keep)
            .is_some_and(|bit| bit_is_set(magnitude, usize::from(bit)));
        let sticky = highest_bit.checked_sub(keep).is_some_and(|bit| {
            (0..bit).any(|candidate| bit_is_set(magnitude, usize::from(candidate)))
        });
        if guard && (sticky || significand & 1 != 0) {
            significand += 1;
            if significand == 1_u64 << 53 {
                return (
                    sign,
                    0.5,
                    EXACT_PRODUCT_EXPONENT + i32::from(highest_bit) + 2,
                );
            }
        }
    }
    (
        sign,
        cadmpeg_core::convert::f64_from_u64(significand).expect("a 53-bit significand")
            * 2.0_f64.powi(-i32::from(keep)),
        EXACT_PRODUCT_EXPONENT + i32::from(highest_bit) + 1,
    )
}

/// `ExactSignedSum::finite_sum` below the normal range as it read bit by bit.
fn bitwise_subnormal_sum(magnitude: &[u64; EXACT_SUM_WORDS], highest_bit: usize) -> FiniteReal {
    let mut units = 0_u64;
    if highest_bit >= SUBNORMAL_UNIT_BIT {
        for bit in (SUBNORMAL_UNIT_BIT..=highest_bit).rev() {
            units = (units << 1) | u64::from(bit_is_set(magnitude, bit));
        }
    }
    let guard = bit_is_set(magnitude, SUBNORMAL_UNIT_BIT - 1);
    let sticky = (0..SUBNORMAL_UNIT_BIT - 1).any(|bit| bit_is_set(magnitude, bit));
    if guard && (sticky || units & 1 != 0) {
        units += 1;
    }
    if units == 0 {
        return FiniteReal::ZERO;
    }
    FiniteReal::subnormal_units(false, units).expect("a subnormal unit count")
}

proptest! {
    #[test]
    fn rounding_reads_the_same_bits_as_a_bitwise_scan(
        top in 0_usize..EXACT_SUM_WORDS,
        words in proptest::collection::vec(proptest::num::u64::ANY, 0..4),
        low_bit in 0_usize..EXACT_SUM_WORDS * 64,
        negative in proptest::bool::ANY,
    ) {
        let magnitude = magnitude(top, &words, low_bit);
        let highest = u16::try_from(highest_bit(&magnitude)).expect("a u16 bit index");
        let rounded = rounded_magnitude(negative, &magnitude, highest).expect("a rounded value");
        let (sign, mantissa, exponent) = bitwise_rounded_magnitude(negative, &magnitude, highest);
        prop_assert_eq!(rounded.sign.to_bits(), sign.to_bits());
        prop_assert_eq!(rounded.mantissa.to_bits(), mantissa.to_bits());
        prop_assert_eq!(rounded.exponent.0, exponent);
    }

    #[test]
    fn subnormal_rounding_reads_the_same_bits_as_a_bitwise_scan(
        top in 0_usize..=(SUBNORMAL_UNIT_BIT + 51) / 64,
        words in proptest::collection::vec(proptest::num::u64::ANY, 0..4),
        low_bit in 0_usize..SUBNORMAL_UNIT_BIT,
    ) {
        let mut magnitude = magnitude(top, &words, low_bit);
        for bit in SUBNORMAL_UNIT_BIT + 52..(top + 1) * 64 {
            magnitude[bit / 64] &= !(1_u64 << (bit % 64));
        }
        let highest = highest_bit(&magnitude);
        let sum = ExactSignedSum {
            positive: magnitude,
            negative: [0; EXACT_SUM_WORDS],
            invalid: false,
        };
        prop_assert_eq!(
            sum.finite_sum().map(|value| value.get().to_bits()),
            Some(bitwise_subnormal_sum(&magnitude, highest).get().to_bits())
        );
    }
}
