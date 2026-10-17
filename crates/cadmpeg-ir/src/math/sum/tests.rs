// SPDX-License-Identifier: Apache-2.0
use proptest::prelude::{prop_oneof, Just, Strategy};
use proptest::{prop_assert_eq, proptest};

use super::{
    bit_is_set, rounded_magnitude, scaled_finite, ExactSignedSum, ScaledValue, Window,
    EXACT_PRODUCT_EXPONENT, EXACT_SUM_WORDS, MAX_SCALED_EXPONENT, MIN_SCALED_EXPONENT,
    MIN_SIGNIFICAND_EXPONENT, SUBNORMAL_UNIT_BIT,
};
use crate::scalar::{FiniteReal, NonZeroReal};

mod full_scan;

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

/// The narrowest window over a nonzero magnitude's nonzero words.
fn tight_window(magnitude: &[u64; EXACT_SUM_WORDS]) -> Window {
    let low = magnitude
        .iter()
        .position(|value| *value != 0)
        .expect("a nonzero magnitude");
    let high = magnitude
        .iter()
        .rposition(|value| *value != 0)
        .expect("a nonzero magnitude");
    Window {
        low,
        high: high + 1,
    }
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
        let low = tight_window(&magnitude).low;
        let rounded =
            rounded_magnitude(negative, &magnitude, low, highest).expect("a rounded value");
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
            window: tight_window(&magnitude),
            invalid: false,
        };
        prop_assert_eq!(
            sum.finite_sum().map(|value| value.get().to_bits()),
            Some(bitwise_subnormal_sum(&magnitude, highest).get().to_bits())
        );
    }
}

/// A term of an exact sum, as the evaluators add them.
#[derive(Clone, Debug)]
enum Term {
    Factors2([f64; 2]),
    Factors3([f64; 3]),
    Factors4([f64; 4]),
    /// A binary64 value times a coefficient.
    Scaled(f64, f64),
    /// An exact three-factor product, rounded once into the extended range,
    /// times a coefficient.
    ScaledProduct([f64; 3], f64),
}

impl Term {
    /// The same term with its first factor negated, and with the lowest
    /// significand bit of that factor flipped when `perturbed`, so that the
    /// pair cancels exactly or leaves a tail far below both terms.
    fn opposite(&self, perturbed: bool) -> Self {
        let flip = |value: f64| {
            let value = -value;
            if perturbed {
                f64::from_bits(value.to_bits() ^ 1)
            } else {
                value
            }
        };
        match self.clone() {
            Self::Factors2([first, rest @ ..]) => Self::Factors2([flip(first), rest[0]]),
            Self::Factors3([first, second, third]) => Self::Factors3([flip(first), second, third]),
            Self::Factors4([first, second, third, fourth]) => {
                Self::Factors4([flip(first), second, third, fourth])
            }
            Self::Scaled(value, factor) => Self::Scaled(flip(value), factor),
            Self::ScaledProduct([first, second, third], factor) => {
                Self::ScaledProduct([flip(first), second, third], factor)
            }
        }
    }

    fn scaled_value(&self) -> Option<(Option<ScaledValue>, FiniteReal)> {
        match self {
            Self::Scaled(value, factor) => Some((scaled_finite(*value), FiniteReal::new(*factor)?)),
            Self::ScaledProduct(factors, factor) => {
                let factors = [
                    NonZeroReal::new(factors[0])?,
                    NonZeroReal::new(factors[1])?,
                    NonZeroReal::new(factors[2])?,
                ];
                Some((
                    ScaledValue::product_of_nonzero(factors),
                    FiniteReal::new(*factor)?,
                ))
            }
            Self::Factors2(_) | Self::Factors3(_) | Self::Factors4(_) => None,
        }
    }

    /// Add the term to both accumulators, and answer whether both admitted it
    /// alike.
    fn add(&self, windowed: &mut ExactSignedSum, scanned: &mut full_scan::FullScanSum) -> bool {
        match self {
            Self::Factors2(factors) => {
                windowed.add_factors(*factors);
                scanned.add_factors(*factors);
            }
            Self::Factors3(factors) => {
                windowed.add_factors(*factors);
                scanned.add_factors(*factors);
            }
            Self::Factors4(factors) => {
                windowed.add_factors(*factors);
                scanned.add_factors(*factors);
            }
            Self::Scaled(..) | Self::ScaledProduct(..) => {
                if let Some((value, factor)) = self.scaled_value() {
                    return windowed.add_scaled_product(value, factor)
                        == scanned.add_scaled_product(value, factor);
                }
            }
        }
        true
    }
}

/// A factor drawn across every binary64 exponent, the subnormal and
/// non-finite fields included, with the ends of the range and short
/// significands weighted in so that terms meet and carry.
fn any_factor() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => (proptest::bool::ANY, 0_u64..=0x7ff, 0_u64..1 << 52).prop_map(
            |(negative, field, fraction)| {
                f64::from_bits((u64::from(negative) << 63) | (field << 52) | fraction)
            }
        ),
        2 => (proptest::bool::ANY, 0_u64..=0x7ff).prop_map(|(negative, field)| {
            f64::from_bits((u64::from(negative) << 63) | (field << 52) | ((1 << 52) - 1))
        }),
        2 => (-64_i32..64, 1_u32..16).prop_map(|(exponent, significand)| {
            f64::from(significand) * 2.0_f64.powi(exponent)
        }),
        1 => proptest::sample::select(vec![
            0.0,
            -0.0,
            1.0,
            -1.0,
            f64::MAX,
            -f64::MAX,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            -f64::from_bits(1),
            f64::INFINITY,
            f64::NAN,
        ]),
    ]
}

fn any_term() -> impl Strategy<Value = Term> {
    prop_oneof![
        3 => [any_factor(), any_factor()].prop_map(Term::Factors2),
        1 => [any_factor(), any_factor(), any_factor()].prop_map(Term::Factors3),
        1 => [any_factor(), any_factor(), any_factor(), any_factor()].prop_map(Term::Factors4),
        1 => (any_factor(), any_factor()).prop_map(|(value, factor)| Term::Scaled(value, factor)),
        1 => ([any_factor(), any_factor(), any_factor()], any_factor())
            .prop_map(|(factors, factor)| Term::ScaledProduct(factors, factor)),
    ]
}

/// A term, and how its opposite follows it: not at all, exactly, or with the
/// lowest bit of its first factor flipped.
fn any_paired_term() -> impl Strategy<Value = (Term, Option<bool>)> {
    (
        any_term(),
        prop_oneof![Just(None), Just(Some(false)), Just(Some(true))],
    )
}

type ScaledParts = Option<(u64, u64, i32)>;

fn scaled_parts(value: Option<ScaledValue>) -> ScaledParts {
    value.map(|value| {
        (
            value.sign.to_bits(),
            value.mantissa.to_bits(),
            value.exponent.0,
        )
    })
}

fn finished(
    windowed: &ExactSignedSum,
    scanned: &full_scan::FullScanSum,
) -> [(ScaledParts, Option<u64>); 2] {
    [
        (
            scaled_parts(windowed.finish()),
            windowed.finite_sum().map(|value| value.get().to_bits()),
        ),
        (
            scaled_parts(scanned.finish()),
            scanned.finite_sum().map(|value| value.get().to_bits()),
        ),
    ]
}

/// Whether every word outside the window is zero on both sides.
fn window_holds_every_nonzero_word(sum: &ExactSignedSum) -> bool {
    let words = sum.window.words();
    (0..EXACT_SUM_WORDS)
        .filter(|index| !words.contains(index))
        .all(|index| sum.positive[index] == 0 && sum.negative[index] == 0)
}

#[test]
fn a_carry_out_of_a_full_word_rounds_as_the_full_scan_rounds() {
    // `2^64 - 1` is the product of the subnormal significand 3855 and a normal
    // significand. A normal exponent field of 29 modulo 64 places the product
    // on a word boundary, so it fills one word and writes none above it; a
    // unit at the right place then carries into a word nothing has written.
    const SUBNORMAL_SIGNIFICAND: u64 = 3855;
    assert_eq!(u64::MAX % SUBNORMAL_SIGNIFICAND, 0);
    let normal_fraction = u64::MAX / SUBNORMAL_SIGNIFICAND - (1 << 52);
    let units = (0..52)
        .map(|bit| f64::from_bits(1 << bit))
        .chain((1..=0x7fe).map(|field| f64::from_bits(field << 52)));
    for field in (29..=0x7fe).step_by(64) {
        let full_word = [
            f64::from_bits(SUBNORMAL_SIGNIFICAND),
            f64::from_bits((field << 52) | normal_fraction),
        ];
        for unit in units.clone() {
            for unit_term in [[f64::from_bits(1), unit], [unit, 1.0], [-unit, 1.0]] {
                let mut windowed = ExactSignedSum::default();
                let mut scanned = full_scan::FullScanSum::default();
                for term in [full_word, unit_term] {
                    windowed.add_factors(term);
                    scanned.add_factors(term);
                    assert!(window_holds_every_nonzero_word(&windowed));
                    let [windowed_result, scanned_result] = finished(&windowed, &scanned);
                    assert_eq!(windowed_result, scanned_result, "{term:?}");
                }
            }
        }
    }
}

proptest! {
    #[test]
    fn windowed_sums_round_as_the_full_scan_rounds(
        terms in proptest::collection::vec(any_paired_term(), 0..12),
    ) {
        let mut windowed = ExactSignedSum::default();
        let mut scanned = full_scan::FullScanSum::default();
        for (term, opposite) in &terms {
            prop_assert_eq!(term.add(&mut windowed, &mut scanned), true);
            prop_assert_eq!(window_holds_every_nonzero_word(&windowed), true);
            let [windowed_result, scanned_result] = finished(&windowed, &scanned);
            prop_assert_eq!(windowed_result, scanned_result);
            if let Some(perturbed) = opposite {
                prop_assert_eq!(term.opposite(*perturbed).add(&mut windowed, &mut scanned), true);
                prop_assert_eq!(window_holds_every_nonzero_word(&windowed), true);
                let [windowed_result, scanned_result] = finished(&windowed, &scanned);
                prop_assert_eq!(windowed_result, scanned_result);
            }
        }
    }

    #[test]
    fn a_windowed_product_rounds_as_the_full_scan_rounds(
        factors in [any_factor(), any_factor(), any_factor(), any_factor()],
    ) {
        let Some(factors) = factors
            .into_iter()
            .map(NonZeroReal::new)
            .collect::<Option<Vec<_>>>()
        else {
            return Ok(());
        };
        prop_assert_eq!(
            scaled_parts(ScaledValue::product_of_nonzero([factors[0]])),
            scaled_parts(full_scan::product_of_nonzero([factors[0]]))
        );
        prop_assert_eq!(
            scaled_parts(ScaledValue::product_of_nonzero([factors[0], factors[1]])),
            scaled_parts(full_scan::product_of_nonzero([factors[0], factors[1]]))
        );
        let four = [factors[0], factors[1], factors[2], factors[3]];
        prop_assert_eq!(
            scaled_parts(ScaledValue::product_of_nonzero(four)),
            scaled_parts(full_scan::product_of_nonzero(four))
        );
    }
}
