// SPDX-License-Identifier: Apache-2.0
//! The exact accumulator as it read when every scan covered all
//! `EXACT_SUM_WORDS` words, kept as the reference the windowed accumulator
//! must reproduce bit for bit.

use super::super::{
    bit_field, bit_is_set, finite_significand, ScaledExponent, ScaledValue, EXACT_PRODUCT_EXPONENT,
    EXACT_SUM_WORDS, MIN_SIGNIFICAND_EXPONENT, SUBNORMAL_UNIT_BIT,
};
use crate::scalar::{FiniteReal, NonZeroReal};

#[derive(Clone, Copy)]
pub(super) struct FullScanSum {
    positive: [u64; EXACT_SUM_WORDS],
    negative: [u64; EXACT_SUM_WORDS],
    invalid: bool,
}

impl Default for FullScanSum {
    fn default() -> Self {
        Self {
            positive: [0; EXACT_SUM_WORDS],
            negative: [0; EXACT_SUM_WORDS],
            invalid: false,
        }
    }
}

pub(super) fn product_of_nonzero<const N: usize>(factors: [NonZeroReal; N]) -> Option<ScaledValue> {
    let mut sum = FullScanSum::default();
    sum.add_factors(factors.map(NonZeroReal::get));
    if sum.invalid {
        return None;
    }
    let negative = factors
        .iter()
        .fold(false, |negative, factor| negative ^ (factor.get() < 0.0));
    let magnitude = if negative { sum.negative } else { sum.positive };
    let highest_bit = magnitude
        .iter()
        .enumerate()
        .try_fold(0_u16, |highest, (word, value)| {
            if *value == 0 {
                Some(highest)
            } else {
                u16::try_from(
                    word * 64 + 63 - cadmpeg_core::decode::index_from_u32(value.leading_zeros()),
                )
                .ok()
            }
        })?;
    rounded_magnitude(negative, &magnitude, highest_bit)
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
    let bytes = value.to_le_bytes();
    // endian-exception: reconstructed-scalar
    let low = u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]);
    // endian-exception: reconstructed-scalar
    let high = u64::from_le_bytes([
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ]);
    for (limb_index, limb) in [low, high].into_iter().enumerate() {
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

impl FullScanSum {
    pub(super) fn add_factors<const N: usize>(&mut self, factors: [f64; N]) {
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
                let bytes = value.to_le_bytes();
                // endian-exception: reconstructed-scalar
                *word = u64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
                carry = value >> 64;
            }
        }
        let Ok(shift) = usize::try_from(exponent - EXACT_PRODUCT_EXPONENT) else {
            self.invalid = true;
            return;
        };
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

    pub(super) fn add_scaled_product(
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

    pub(super) fn finish(self) -> Option<ScaledValue> {
        if self.invalid {
            return None;
        }
        let (negative, magnitude) = signed_difference(&self.positive, &self.negative)?;
        let word = magnitude.iter().rposition(|value| *value != 0)?;
        let highest_bit = u16::try_from(
            word * 64 + cadmpeg_core::decode::index_from_u32(magnitude[word].checked_ilog2()?),
        )
        .ok()?;
        rounded_magnitude(negative, &magnitude, highest_bit)
    }

    pub(super) fn finite_sum(self) -> Option<FiniteReal> {
        if self.invalid {
            return None;
        }
        let Some((negative, magnitude)) = signed_difference(&self.positive, &self.negative) else {
            return Some(FiniteReal::ZERO);
        };
        let word = magnitude.iter().rposition(|value| *value != 0)?;
        let highest_bit =
            word * 64 + cadmpeg_core::decode::index_from_u32(magnitude[word].checked_ilog2()?);
        if highest_bit >= SUBNORMAL_UNIT_BIT + 52 {
            return self.finish()?.finite().ok();
        }
        let mut units = 0_u64;
        if highest_bit >= SUBNORMAL_UNIT_BIT {
            units = bit_field(
                &magnitude,
                SUBNORMAL_UNIT_BIT,
                highest_bit + 1 - SUBNORMAL_UNIT_BIT,
            );
        }
        let guard = bit_is_set(&magnitude, SUBNORMAL_UNIT_BIT - 1);
        let sticky = any_bit_below(&magnitude, SUBNORMAL_UNIT_BIT - 1);
        if guard && (sticky || units & 1 != 0) {
            units += 1;
        }
        if units == 0 {
            return Some(FiniteReal::ZERO);
        }
        FiniteReal::subnormal_units(negative, units)
    }
}

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

fn any_bit_below(words: &[u64; EXACT_SUM_WORDS], bit: usize) -> bool {
    let (word, offset) = (bit / 64, bit % 64);
    words[..word].iter().any(|value| *value != 0) || words[word] & ((1_u64 << offset) - 1) != 0
}

fn rounded_magnitude(
    negative: bool,
    magnitude: &[u64; EXACT_SUM_WORDS],
    highest_bit: u16,
) -> Option<ScaledValue> {
    let keep = (highest_bit + 1).min(53);
    let mut significand = bit_field(
        magnitude,
        usize::from(highest_bit + 1 - keep),
        usize::from(keep),
    );
    if keep == 53 {
        let guard_bit = highest_bit
            .checked_sub(keep)
            .is_some_and(|bit| bit_is_set(magnitude, usize::from(bit)));
        let sticky = highest_bit
            .checked_sub(keep)
            .is_some_and(|bit| any_bit_below(magnitude, usize::from(bit)));
        if guard_bit && (sticky || significand & 1 != 0) {
            significand += 1;
            if significand == 1_u64 << 53 {
                return Some(ScaledValue {
                    sign: if negative { -1.0 } else { 1.0 },
                    mantissa: 0.5,
                    exponent: ScaledExponent(EXACT_PRODUCT_EXPONENT + i32::from(highest_bit) + 2),
                });
            }
        }
    }
    Some(ScaledValue {
        sign: if negative { -1.0 } else { 1.0 },
        mantissa: cadmpeg_core::convert::f64_from_u64(significand)?
            * 2.0_f64.powi(-i32::from(keep)),
        exponent: ScaledExponent(EXACT_PRODUCT_EXPONENT + i32::from(highest_bit) + 1),
    })
}
