// SPDX-License-Identifier: Apache-2.0
//! Finite sums with an exact-product fallback for floating-point range loss.

use crate::scalar::{FiniteReal, NonZeroReal};

/// A finite dot product with an exact-product fallback for range loss or cancellation.
///
/// A non-finite input, and a sum outside the finite range, leave the dot
/// product non-finite; it then carries the plain left-to-right sum of the
/// products.
pub(crate) fn finite_dot<const N: usize>(
    coefficients: [f64; N],
    components: [f64; N],
) -> Result<FiniteReal, f64> {
    let products = std::array::from_fn(|index| coefficients[index] * components[index]);
    let plain = || {
        products
            .into_iter()
            .reduce(|sum, product| sum + product)
            .unwrap_or(0.0)
    };
    if coefficients
        .iter()
        .chain(&components)
        .any(|value| !value.is_finite())
    {
        return Err(plain());
    }
    if let Some(value) = fast_dot(coefficients, components, products) {
        return Ok(value);
    }
    let mut sum = ExactSignedSum::default();
    for (coefficient, component) in coefficients.into_iter().zip(components) {
        sum.add_product(coefficient, component);
    }
    sum.finite_sum().ok_or_else(plain)
}

// The smallest exponent a finite `f64` significand carries. Every exponent
// this module handles is biased by it, which keeps the biased value unsigned
// and makes the product shift below a `usize` with no conversion.
const MIN_SIGNIFICAND_EXPONENT: i32 = -1074;

// Four finite factors need at most 212 significand bits. The accumulator
// includes their entire exponent range, 64 low guard bits for rounded scaled
// terms, and enough high carry bits for any addressable collection of terms.
const EXACT_PRODUCT_EXPONENT: i32 = 4 * MIN_SIGNIFICAND_EXPONENT - 64;
const EXACT_SUM_WORDS: usize = 138;
const SUBNORMAL_UNIT_BIT: usize = (-1074 - EXACT_PRODUCT_EXPONENT) as usize;

#[derive(Clone, Copy)]
pub(crate) struct ExactSignedSum {
    positive: [u64; EXACT_SUM_WORDS],
    negative: [u64; EXACT_SUM_WORDS],
}

impl Default for ExactSignedSum {
    fn default() -> Self {
        Self {
            positive: [0; EXACT_SUM_WORDS],
            negative: [0; EXACT_SUM_WORDS],
        }
    }
}

/// The largest biased significand exponent a finite `f64` states. The field is
/// eleven bits wide and a finite value never reaches the all-ones field, and
/// biasing by [`MIN_SIGNIFICAND_EXPONENT`] subtracts one more.
const MAX_BIASED_SIGNIFICAND_EXPONENT: i32 = 0x7fe - 1;

/// The widest significand a finite `f64` states, the hidden bit included.
///
/// [`finite_significand`] proves it. Its subnormal branch answers a nonzero
/// `fraction`, which is below `1 << 52`, and its normal branch answers
/// `(1 << 52) | fraction`. The highest set bit is therefore at most 52 and the
/// width at most 53.
const MAX_SIGNIFICAND_BITS: i32 = 53;

/// The least power of two a [`ScaledExponent`] holds. `ExactSignedSum::finish`
/// states `EXACT_PRODUCT_EXPONENT + highest_bit + 1` with `highest_bit` at
/// least zero, which is below everything `scaled_finite` can state.
const MIN_SCALED_EXPONENT: i32 = EXACT_PRODUCT_EXPONENT + 1;

/// The greatest power of two a [`ScaledExponent`] holds.
/// `ExactSignedSum::finish` states `EXACT_PRODUCT_EXPONENT + highest_bit + 2`
/// in its rounding-carry arm, with `highest_bit` at most one below the
/// accumulator's bit width.
const MAX_SCALED_EXPONENT: i32 = EXACT_PRODUCT_EXPONENT + EXACT_SUM_WORDS as i32 * 64 + 1;

/// The power of two that scales a [`ScaledValue`]'s mantissa back to the value.
///
/// The field is private and the type has no constructor of its own, so the two
/// construction expressions in [`ScaledValue::of_nonzero`] and
/// [`ExactSignedSum::finish`] are the only values that exist. Neither reads an
/// exponent from outside, and the four `const` assertions below state that
/// both reach only `MIN_SCALED_EXPONENT..=MAX_SCALED_EXPONENT`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ScaledExponent(i32);

/// `ScaledValue::of_nonzero` states the exponent of the value's own
/// significand, `MIN_SIGNIFICAND_EXPONENT + biased + bits`, with `biased` from
/// zero to `MAX_BIASED_SIGNIFICAND_EXPONENT` and `bits` from one to
/// `MAX_SIGNIFICAND_BITS`: the raise of a subnormal value is taken back off.
const _: () = assert!(MIN_SIGNIFICAND_EXPONENT + 1 >= MIN_SCALED_EXPONENT);
const _: () = assert!(
    MIN_SIGNIFICAND_EXPONENT + MAX_BIASED_SIGNIFICAND_EXPONENT + MAX_SIGNIFICAND_BITS
        <= MAX_SCALED_EXPONENT
);

/// `ExactSignedSum::finish` states `EXACT_PRODUCT_EXPONENT + highest_bit + 1`,
/// and `+ 2` in its rounding-carry arm, with `highest_bit` indexing an
/// accumulator `EXACT_SUM_WORDS` words of 64 bits wide.
const _: () = assert!(EXACT_PRODUCT_EXPONENT + 1 >= MIN_SCALED_EXPONENT);
const _: () =
    assert!(EXACT_PRODUCT_EXPONENT + (EXACT_SUM_WORDS as i32 * 64 - 1) + 2 <= MAX_SCALED_EXPONENT);

impl ScaledExponent {
    /// The exponent shift that rescales a value in this frame into `other`'s.
    ///
    /// Plain `-`: both sides lie in `MIN_SCALED_EXPONENT..=MAX_SCALED_EXPONENT`,
    /// so the difference is at most `MAX_SCALED_EXPONENT - MIN_SCALED_EXPONENT`
    /// in magnitude.
    fn difference(self, other: Self) -> i32 {
        self.0 - other.0
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ScaledValue {
    sign: f64,
    mantissa: f64,
    pub(crate) exponent: ScaledExponent,
}

impl ScaledValue {
    pub(crate) fn exponent(self) -> i32 {
        self.exponent.0
    }

    pub(crate) fn rescale(self, exponent: i32) -> Option<FiniteReal> {
        super::scale_power_of_two(
            self.sign * self.mantissa,
            self.exponent.0.checked_sub(exponent)?,
        )
    }

    /// The value, rescaled into the frame whose exponent is `scale_exponent`.
    pub(crate) fn scaled_by(self, scale_exponent: ScaledExponent) -> f64 {
        super::scale_power_of_two(
            self.sign * self.mantissa,
            self.exponent.difference(scale_exponent),
        )
        .map_or(self.sign * f64::INFINITY, FiniteReal::get)
    }
    /// The value, or the signed infinity it overflows to. The mantissa is
    /// finite and nonzero, so only the scaling can leave the finite range.
    pub(crate) fn finite(self) -> Result<FiniteReal, f64> {
        super::scale_power_of_two(self.sign * self.mantissa, self.exponent.0)
            .ok_or(self.sign * f64::INFINITY)
    }

    /// `self / denominator`, or the signed infinity the quotient overflows
    /// to. Both mantissas are in `[0.5, 1)`, so their ratio is finite and
    /// nonzero, and the exponent difference lies in the `ScaledExponent` span:
    /// only the final scaling can leave the finite range.
    pub(crate) fn quotient(self, denominator: Self) -> Result<FiniteReal, f64> {
        let ratio = self.sign * denominator.sign * (self.mantissa / denominator.mantissa);
        super::scale_power_of_two(ratio, self.exponent.difference(denominator.exponent))
            .ok_or(ratio.signum() * f64::INFINITY)
    }

    /// Divide by up to five nonzero extended-range factors before rounding to
    /// binary64, doubling the quotient when `doubled` is set. Each normalized
    /// mantissa is in `[0.5, 1)`, so the small product of mantissa quotients
    /// stays in range independently of exponent. A quotient that overflows
    /// carries its signed infinity.
    ///
    /// Plain `+` and `-`: every exponent lies in
    /// `MIN_SCALED_EXPONENT..=MAX_SCALED_EXPONENT`, and six of them and a
    /// doubling stay far inside `i32`.
    pub(crate) fn quotient_by_factors<const N: usize>(
        self,
        denominators: [Self; N],
        doubled: bool,
    ) -> Result<FiniteReal, f64> {
        const { assert!(N <= 5) };
        let mut mantissa = self.sign * self.mantissa;
        let mut exponent = self.exponent.0 + i32::from(doubled);
        for denominator in denominators {
            mantissa /= denominator.sign * denominator.mantissa;
            exponent -= denominator.exponent.0;
        }
        super::scale_power_of_two(mantissa, exponent).ok_or(mantissa.signum() * f64::INFINITY)
    }

    /// `self / denominator` times `2^exponent_shift`, without rounding either
    /// operand into the binary64 range first: the ratio of the two mantissas
    /// lies in `(0.5, 2)`, and only the final scaling rounds. The caller
    /// states the bound that keeps the product finite.
    ///
    /// Plain `+`: the exponent difference lies in the `ScaledExponent` span,
    /// and a shift within the binary64 exponent range keeps the sum far inside
    /// `i32`.
    pub(crate) fn quotient_shifted_product(self, denominator: Self, exponent_shift: i32) -> f64 {
        super::power_of_two_product(
            self.sign * denominator.sign * (self.mantissa / denominator.mantissa),
            self.exponent.difference(denominator.exponent) + exponent_shift,
        )
    }

    /// The scaled form of a nonzero finite value: its sign, the mantissa of
    /// its magnitude in `[0.5, 1)`, and the power of two between them.
    ///
    /// A subnormal magnitude is first raised by `2^RAISE`, which is exact and
    /// makes it normal, and the raise is taken back off the exponent. A normal
    /// magnitude's significand is `(1 << 52) | fraction`, whose highest bit is
    /// set, so its 53 bits form the mantissa without a search. Every nonzero
    /// finite value has this form, so nothing is checked.
    pub(crate) fn of_nonzero(value: NonZeroReal) -> Self {
        // The least subnormal magnitude is `2^-1074`; raised, it is `2^-1010`,
        // above the least normal magnitude `2^-1022`.
        const RAISE: i32 = 64;
        let value = value.get();
        let (normal, raised) = if value.abs() < f64::MIN_POSITIVE {
            (value * 2.0_f64.powi(RAISE), RAISE)
        } else {
            (value, 0)
        };
        let bits = normal.to_bits();
        // The mask keeps eleven bits, so the field is an `i32` by
        // construction; a normal field is at least one.
        let field = ((bits >> 52) & 0x7ff) as i32;
        let significand = (1_u64 << 52) | (bits & ((1_u64 << 52) - 1));
        Self {
            sign: if bits >> 63 != 0 { -1.0 } else { 1.0 },
            // A 53-bit integer converts exactly.
            mantissa: significand as f64 * 2.0_f64.powi(-MAX_SIGNIFICAND_BITS),
            exponent: ScaledExponent(
                field - 1 + MIN_SIGNIFICAND_EXPONENT + MAX_SIGNIFICAND_BITS - raised,
            ),
        }
    }
}

/// The sign, the integer significand, and the significand's exponent biased by
/// [`MIN_SIGNIFICAND_EXPONENT`], of a finite `f64`.
///
/// The biased exponent is a `u16`: the field is eleven bits wide, and a finite
/// value never reaches the all-ones field, so the biased value is at most 2045.
fn finite_significand(value: f64) -> Option<(bool, u64, u16)> {
    if !value.is_finite() {
        return None;
    }
    let bits = value.to_bits();
    let negative = bits >> 63 != 0;
    // The mask keeps eleven bits, so the field is a `u16` by construction.
    let exponent_field = ((bits >> 52) & 0x7ff) as u16;
    let fraction = bits & ((1_u64 << 52) - 1);
    if exponent_field == 0 {
        (fraction != 0).then_some((negative, fraction, 0))
    } else {
        // A normal value's exponent is `field - 1075`, which is
        // `field - 1` once biased by `MIN_SIGNIFICAND_EXPONENT`.
        Some((negative, (1_u64 << 52) | fraction, exponent_field - 1))
    }
}

fn add_word(words: &mut [u64; EXACT_SUM_WORDS], index: usize, value: u64) {
    let (sum, mut carry) = words[index].overflowing_add(value);
    words[index] = sum;
    let mut index = index + 1;
    while carry {
        let (sum, next) = words[index].overflowing_add(1);
        words[index] = sum;
        carry = next;
        index += 1;
    }
}

fn add_shifted(words: &mut [u64; EXACT_SUM_WORDS], value: u128, shift: usize) {
    for (limb_index, limb) in [value as u64, (value >> 64) as u64].into_iter().enumerate() {
        if limb == 0 {
            continue;
        }
        let bit = shift + limb_index * 64;
        let word = bit / 64;
        let remainder = bit % 64;
        if remainder == 0 {
            add_word(words, word, limb);
        } else {
            add_word(words, word, limb << remainder);
            add_word(words, word + 1, limb >> (64 - remainder));
        }
    }
}

impl ExactSignedSum {
    pub(crate) fn add_product(&mut self, left: f64, right: f64) {
        self.add_factors([left, right]);
    }

    pub(crate) fn add_factors<const N: usize>(&mut self, factors: [f64; N]) {
        // All callers supply at most four factors, the tensor-product point case.
        assert!(N <= 4);
        let mut product = [1_u64, 0, 0, 0];
        let mut negative = false;
        let mut exponent = 0_i32;
        for factor in factors {
            let Some((sign, significand, biased)) = finite_significand(factor) else {
                return;
            };
            negative ^= sign;
            exponent += MIN_SIGNIFICAND_EXPONENT + i32::from(biased);
            let mut carry = 0_u128;
            for word in &mut product {
                let value = u128::from(*word) * u128::from(significand) + carry;
                *word = value as u64;
                carry = value >> 64;
            }
        }
        let shift = (exponent - EXACT_PRODUCT_EXPONENT) as usize;
        let target = if negative {
            &mut self.negative
        } else {
            &mut self.positive
        };
        add_shifted(
            target,
            u128::from(product[0]) | (u128::from(product[1]) << 64),
            shift,
        );
        add_shifted(
            target,
            u128::from(product[2]) | (u128::from(product[3]) << 64),
            shift + 128,
        );
    }

    /// Add a rounded extended-range sum times a finite coefficient. Rational
    /// quotient derivatives use this to subtract weight derivatives before division.
    pub(crate) fn add_scaled_product(
        &mut self,
        value: Option<ScaledValue>,
        factor: FiniteReal,
    ) -> Option<()> {
        let Some(value) = value else {
            return Some(());
        };
        let Some((negative, significand, exponent)) =
            finite_significand(value.sign * value.mantissa)
        else {
            return Some(());
        };
        let Some((factor_negative, factor_significand, factor_exponent)) =
            finite_significand(factor.get())
        else {
            return Some(());
        };
        let product = u128::from(significand) * u128::from(factor_significand);
        let shift = value.exponent.0
            + 2 * MIN_SIGNIFICAND_EXPONENT
            + i32::from(exponent)
            + i32::from(factor_exponent)
            - EXACT_PRODUCT_EXPONENT;
        let shift = usize::try_from(shift).ok()?;
        if shift + 128 >= EXACT_SUM_WORDS * 64 {
            return None;
        }
        let target = if negative ^ factor_negative {
            &mut self.negative
        } else {
            &mut self.positive
        };
        add_shifted(target, product, shift);
        Some(())
    }

    pub(crate) fn finish(self) -> Option<ScaledValue> {
        let (negative, magnitude) = signed_difference(&self.positive, &self.negative)?;
        let word = magnitude.iter().rposition(|value| *value != 0)?;
        // `checked_ilog2` answers `None` for a zero word, which `rposition`
        // has already excluded; the `?` states that rather than asserting it.
        // `word` indexes `EXACT_SUM_WORDS` words of 64 bits, so a bit index of
        // the accumulator is a `u16` and converts to `i32` with no range check.
        let highest_bit = (word * 64 + magnitude[word].checked_ilog2()? as usize) as u16;
        let keep = (highest_bit + 1).min(53);
        let mut significand = 0_u64;
        for bit in (highest_bit + 1 - keep..=highest_bit).rev() {
            significand = (significand << 1) | u64::from(bit_is_set(&magnitude, usize::from(bit)));
        }
        if keep == 53 {
            let guard_bit = highest_bit
                .checked_sub(keep)
                .is_some_and(|bit| bit_is_set(&magnitude, usize::from(bit)));
            let sticky = highest_bit.checked_sub(keep).is_some_and(|bit| {
                (0..bit).any(|candidate| bit_is_set(&magnitude, usize::from(candidate)))
            });
            if guard_bit && (sticky || significand & 1 != 0) {
                significand += 1;
                if significand == 1_u64 << 53 {
                    return Some(ScaledValue {
                        sign: if negative { -1.0 } else { 1.0 },
                        mantissa: 0.5,
                        exponent: ScaledExponent(
                            EXACT_PRODUCT_EXPONENT + i32::from(highest_bit) + 2,
                        ),
                    });
                }
            }
        }
        Some(ScaledValue {
            sign: if negative { -1.0 } else { 1.0 },
            mantissa: significand as f64 * 2.0_f64.powi(-i32::from(keep)),
            exponent: ScaledExponent(EXACT_PRODUCT_EXPONENT + i32::from(highest_bit) + 1),
        })
    }

    /// Round a sum in the subnormal range directly onto the binary64 grid.
    /// Rounding through a 53-bit scaled mantissa first can discard a tail that
    /// breaks a tie at half of the smallest subnormal.
    pub(crate) fn finite_sum(self) -> Option<FiniteReal> {
        let Some((negative, magnitude)) = signed_difference(&self.positive, &self.negative) else {
            return Some(FiniteReal::ZERO);
        };
        let word = magnitude.iter().rposition(|value| *value != 0)?;
        let highest_bit = word * 64 + magnitude[word].checked_ilog2()? as usize;
        if highest_bit >= SUBNORMAL_UNIT_BIT + 52 {
            return self.finish()?.finite().ok();
        }
        let mut units = 0_u64;
        if highest_bit >= SUBNORMAL_UNIT_BIT {
            for bit in (SUBNORMAL_UNIT_BIT..=highest_bit).rev() {
                units = (units << 1) | u64::from(bit_is_set(&magnitude, bit));
            }
        }
        let guard = bit_is_set(&magnitude, SUBNORMAL_UNIT_BIT - 1);
        let sticky = (0..SUBNORMAL_UNIT_BIT - 1).any(|bit| bit_is_set(&magnitude, bit));
        if guard && (sticky || units & 1 != 0) {
            units += 1;
        }
        if units == 0 {
            return Some(FiniteReal::ZERO);
        }
        Some(FiniteReal::subnormal_units(negative, units))
    }
}

/// Returns the sign and the magnitude of `positive - negative`, or `None` when
/// the two accumulators are equal.
///
/// The comparison that names the larger accumulator is the only route to the
/// subtraction below, so the minuend is never the smaller side and the loop
/// cannot leave a borrow in the highest word. Splitting the comparison from the
/// subtraction would state that relation as an assertion instead of holding it.
fn signed_difference(
    positive: &[u64; EXACT_SUM_WORDS],
    negative: &[u64; EXACT_SUM_WORDS],
) -> Option<(bool, [u64; EXACT_SUM_WORDS])> {
    let (larger, smaller, is_negative) =
        positive
            .iter()
            .zip(negative)
            .rev()
            .find_map(|(left, right)| match left.cmp(right) {
                std::cmp::Ordering::Greater => Some((positive, negative, false)),
                std::cmp::Ordering::Less => Some((negative, positive, true)),
                std::cmp::Ordering::Equal => None,
            })?;
    let mut magnitude = [0; EXACT_SUM_WORDS];
    let mut borrow = false;
    for index in 0..EXACT_SUM_WORDS {
        let (difference, first_borrow) = larger[index].overflowing_sub(smaller[index]);
        let (difference, second_borrow) = difference.overflowing_sub(u64::from(borrow));
        magnitude[index] = difference;
        borrow = first_borrow || second_borrow;
    }
    Some((is_negative, magnitude))
}

fn bit_is_set(words: &[u64; EXACT_SUM_WORDS], bit: usize) -> bool {
    words[bit / 64] & (1_u64 << (bit % 64)) != 0
}

/// The scaled form of a finite nonzero value; a zero or non-finite value has
/// none.
pub(crate) fn scaled_finite(value: f64) -> Option<ScaledValue> {
    NonZeroReal::new(value).map(ScaledValue::of_nonzero)
}

/// `value / denominator`, multiplied by each factor through one quotient that
/// keeps the extended exponent range. A knot span below the normal range makes
/// the plain quotient overflow although every product here is finite, and one
/// quotient states the same rounding for every factor.
///
/// A zero value, and a zero denominator that a repeated knot states, make every
/// product zero. `None` states a product that no finite `f64` holds.
pub(crate) fn scaled_ratio_products<const N: usize>(
    value: FiniteReal,
    denominator: FiniteReal,
    factors: [FiniteReal; N],
) -> Option<[FiniteReal; N]> {
    // A finite value has a scaled form exactly when it is not zero.
    let (Some(value), Some(denominator)) =
        (scaled_finite(value.get()), scaled_finite(denominator.get()))
    else {
        return Some([FiniteReal::ZERO; N]);
    };
    // Both significands are in `[0.5, 1)`, so the ratio is in `(0.5, 2)` and
    // every product below stays normal until `scale_power_of_two` states it.
    let ratio = value.sign * denominator.sign * (value.mantissa / denominator.mantissa);
    // Plain `+`: the difference lies inside the `ScaledExponent` span and a
    // factor's exponent inside the `f64` range.
    let exponent = value.exponent.difference(denominator.exponent);
    let mut products = [FiniteReal::ZERO; N];
    for (product, factor) in products.iter_mut().zip(factors) {
        let Some(factor) = scaled_finite(factor.get()) else {
            continue;
        };
        *product = super::scale_power_of_two(
            ratio * factor.sign * factor.mantissa,
            exponent + factor.exponent.0,
        )?;
    }
    Some(products)
}

pub(crate) fn fast_dot<const N: usize>(
    coefficients: [f64; N],
    components: [f64; N],
    products: [f64; N],
) -> Option<FiniteReal> {
    if products.iter().any(|product| !product.is_finite()) {
        return None;
    }
    if coefficients.into_iter().zip(components).zip(products).any(
        |((coefficient, component), product)| {
            // A product below the normal range may have rounded before it
            // reached this check, even when it is nonzero. Recompute every
            // such column from the exact significands.
            coefficient != 0.0 && component != 0.0 && product.abs() < f64::MIN_POSITIVE
        },
    ) {
        return None;
    }
    let scale = products
        .iter()
        .fold(0.0_f64, |scale, product| scale.max(product.abs()));
    if scale == 0.0 {
        return Some(FiniteReal::ZERO);
    }
    let sum = FiniteReal::new(products.into_iter().sum::<f64>())?;
    if sum.get().abs() / scale < f64::EPSILON {
        return None;
    }
    Some(sum)
}

/// The outcome of [`product_sum`].
pub(crate) enum ProductSum {
    /// A term is absent, or a factor is not finite. The sum has no value.
    Undefined,
    /// The terms cancel to exactly zero.
    Zero,
    /// The sum, in the extended exponent range.
    Value(ScaledValue),
}

impl ProductSum {
    /// The two outcomes `ExactSignedSum::finish` and `scaled_finite` state,
    /// whose `None` is an exact zero.
    fn from_scaled(value: Option<ScaledValue>) -> Self {
        match value {
            Some(value) => Self::Value(value),
            None => Self::Zero,
        }
    }
}

/// Sum products without losing a finite result to an intermediate exponent.
/// Ordinary inputs use f64 arithmetic; range loss and cancellation replay the
/// same terms through the exact accumulator.
pub(crate) fn product_sum<const N: usize>(
    terms: impl Iterator<Item = Option<[f64; N]>> + Clone,
) -> ProductSum {
    let mut sum = 0.0_f64;
    let mut largest = 0.0_f64;
    let mut exact = false;
    for factors in terms.clone() {
        let Some(factors) = factors else {
            return ProductSum::Undefined;
        };
        if factors.iter().any(|value| !value.is_finite()) {
            return ProductSum::Undefined;
        }
        if factors.contains(&0.0) {
            continue;
        }
        let mut product = 1.0;
        for factor in factors {
            product *= factor;
            exact |= !product.is_normal();
        }
        sum += product;
        largest = largest.max(product.abs());
        exact |= !sum.is_finite();
    }
    exact |= largest != 0.0 && sum.abs() / largest < f64::EPSILON;
    if !exact {
        return ProductSum::from_scaled(scaled_finite(sum));
    }
    let mut sum = ExactSignedSum::default();
    for factors in terms {
        let Some(factors) = factors else {
            return ProductSum::Undefined;
        };
        sum.add_factors(factors);
    }
    ProductSum::from_scaled(sum.finish())
}

#[cfg(test)]
mod tests;
