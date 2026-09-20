// SPDX-License-Identifier: Apache-2.0
//! Finite sums with an exact-product fallback for floating-point range loss.

/// A finite dot product with an exact-product fallback for range loss or cancellation.
pub(crate) fn finite_dot<const N: usize>(
    coefficients: [f64; N],
    components: [f64; N],
) -> Option<f64> {
    if coefficients
        .iter()
        .chain(&components)
        .any(|value| !value.is_finite())
    {
        return None;
    }
    let products = std::array::from_fn(|index| coefficients[index] * components[index]);
    if let Some(value) = fast_dot(coefficients, components, products) {
        return Some(value);
    }
    let mut sum = ExactSignedSum::default();
    for (coefficient, component) in coefficients.into_iter().zip(components) {
        sum.add_product(coefficient, component);
    }
    let Some(value) = sum.finish() else {
        return Some(0.0);
    };
    let exponent = value.exponent.0;
    let outer = exponent.clamp(-1022, 1023);
    let result =
        (value.sign * value.mantissa * 2.0_f64.powi(exponent - outer)) * 2.0_f64.powi(outer);
    result.is_finite().then_some(result)
}

// The smallest exponent a finite `f64` significand carries. Every exponent
// this module handles is biased by it, which keeps the biased value unsigned
// and makes the product shift below a `usize` with no conversion.
const MIN_SIGNIFICAND_EXPONENT: i32 = -1074;

// A product of two finite f64 values has an integer significand with at most
// 106 bits. The smallest product exponent is the sum of two smallest
// significand exponents and the largest is 1942; this range leaves 66 words
// for three exact signed products and their sum.
const EXACT_PRODUCT_EXPONENT: i32 = 2 * MIN_SIGNIFICAND_EXPONENT;
const EXACT_SUM_WORDS: usize = 66;

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
/// construction expressions in [`scaled_finite`] and [`ExactSignedSum::finish`]
/// are the only values that exist. Neither reads an exponent from outside, and
/// the four `const` assertions below state that both reach only
/// `MIN_SCALED_EXPONENT..=MAX_SCALED_EXPONENT`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ScaledExponent(i32);

/// `scaled_finite` states `MIN_SIGNIFICAND_EXPONENT + biased + bits`, with
/// `biased` from zero to `MAX_BIASED_SIGNIFICAND_EXPONENT` and `bits` from one
/// to `MAX_SIGNIFICAND_BITS`.
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
    /// in magnitude. This is the one operation the exponent is read for.
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
    /// The value, rescaled into the frame whose exponent is `scale_exponent`.
    pub(crate) fn scaled_by(self, scale_exponent: ScaledExponent) -> f64 {
        self.sign * self.mantissa * 2.0_f64.powi(self.exponent.difference(scale_exponent))
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
        let Some((left_negative, left_significand, left_exponent)) = finite_significand(left)
        else {
            return;
        };
        let Some((right_negative, right_significand, right_exponent)) = finite_significand(right)
        else {
            return;
        };
        let product = u128::from(left_significand) * u128::from(right_significand);
        // Both exponents are biased by `MIN_SIGNIFICAND_EXPONENT`, so their sum
        // is the product exponent already offset from `EXACT_PRODUCT_EXPONENT`.
        let shift = usize::from(left_exponent) + usize::from(right_exponent);
        let target = if left_negative ^ right_negative {
            &mut self.negative
        } else {
            &mut self.positive
        };
        add_shifted(target, product, shift);
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

pub(crate) fn scaled_finite(value: f64) -> Option<ScaledValue> {
    let (negative, significand, exponent) = finite_significand(value)?;
    // A finite significand is never zero: the subnormal branch requires a
    // nonzero fraction and the normal branch sets bit 52.
    let highest_bit = significand.checked_ilog2()?;
    // `finite_significand` answers a significand whose highest set bit is at
    // most 52, so `bits` is at most `MAX_SIGNIFICAND_BITS`. That is the bound
    // the `ScaledExponent` assertion above relies on.
    let bits = highest_bit as i32 + 1;
    Some(ScaledValue {
        sign: if negative { -1.0 } else { 1.0 },
        mantissa: significand as f64 * 2.0_f64.powi(-bits),
        exponent: ScaledExponent(i32::from(exponent) + MIN_SIGNIFICAND_EXPONENT + bits),
    })
}

pub(crate) fn fast_dot<const N: usize>(
    coefficients: [f64; N],
    components: [f64; N],
    products: [f64; N],
) -> Option<f64> {
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
        return Some(0.0);
    }
    let sum = products.into_iter().sum::<f64>();
    if !sum.is_finite() || sum.abs() / scale < f64::EPSILON {
        return None;
    }
    Some(sum)
}

#[cfg(test)]
mod tests;
