// SPDX-License-Identifier: Apache-2.0
//! Six-factor extended-range sums with final weight and span division.

use super::{add_shifted, bit_is_set, signed_difference, ScaledValue,
    MAX_SCALED_EXPONENT, MIN_SCALED_EXPONENT};
use crate::scalar::FiniteReal;

// Six extended mantissas have 318 integer bits. Every closed fifth table
// totals 1082 signed copies; eleven carry bits cover the complete sum.
const ORIGIN: i32 = 6 * (MIN_SCALED_EXPONENT - 53);
const WORDS: usize = 834;
const WORDS_I32: i32 = 834;
const REQUIRED_BITS: i32 = 6 * (MAX_SCALED_EXPONENT - MIN_SCALED_EXPONENT + 53) + 11;
const _: () = assert!(WORDS == 834 && WORDS_I32 == 834);
const _: () = assert!(ORIGIN == -26472 && REQUIRED_BITS == 53321);
const _: () = assert!(WORDS_I32 * 64 >= REQUIRED_BITS);
const _: () = assert!(WORDS_I32 * 64 <= 65535);

pub(super) struct Numerator {
    positive: [u64; WORDS],
    negative: [u64; WORDS],
}

impl Numerator {
    pub(super) fn new() -> Self {
        Self { positive: [0; WORDS], negative: [0; WORDS] }
    }

    // The closed pure and mixed tables each total1082 signed copies.
    // None in a factor is a genuine exact zero.
    pub(super) fn add_product(&mut self, factors: [Option<ScaledValue>; 6], negative: bool, copies: usize)
        -> Option<()>
    {
        let [Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)] = factors else { return Some(()); };
        let mut product = [1_u64, 0, 0, 0, 0];
        let mut exponent = 0;
        let mut negative = negative;
        for factor in [a, b, c, d, e, f] {
            negative ^= factor.sign < 0.0;
            exponent += factor.exponent() - 53;
            // Every normalized mantissa has exponent -1 and 53 integer bits.
            let significand = (1_u64 << 52) | (factor.mantissa.to_bits() & ((1_u64 << 52) - 1));
            let mut carry = 0_u128;
            for word in &mut product {
                // A 64-bit limb times 53 bits plus at most 53 carry bits fits
                // in 118 bits. Six products fit the actual 320-bit array.
                let value = u128::from(*word) * u128::from(significand) + carry;
                let bytes = value.to_le_bytes();
                // endian-exception: reconstructed-scalar
                *word = u64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
                carry = value >> 64;
            }
        }
        // Six bounded exponents make a nonnegative shift below 53321.
        let shift = usize::from(u16::try_from(exponent - ORIGIN).ok()?);
        let target = if negative { &mut self.negative } else { &mut self.positive };
        for _ in 0..copies {
            add_shifted(target, u128::from(product[0]) | (u128::from(product[1]) << 64), shift);
            add_shifted(target, u128::from(product[2]) | (u128::from(product[3]) << 64), shift + 128);
            add_shifted(target, u128::from(product[4]), shift + 256);
        }
        Some(())
    }

    pub(super) fn divide(self, weight: ScaledValue, widths: [ScaledValue; 5]) -> Option<Result<FiniteReal, f64>> {
        let Some((negative, magnitude)) = signed_difference(&self.positive, &self.negative) else {
            return Some(Ok(FiniteReal::ZERO));
        };
        let word = magnitude.iter().rposition(|value| *value != 0)?;
        let highest = u16::try_from(word * 64
            + cadmpeg_core::decode::index_from_u32(magnitude[word].checked_ilog2()?)).ok()?;
        let keep = (highest + 1).min(53);
        let mut significand = 0_u64;
        for bit in (highest + 1 - keep..=highest).rev() {
            significand = (significand << 1) | u64::from(bit_is_set(&magnitude, usize::from(bit)));
        }
        let guard = highest.checked_sub(keep).is_some_and(|bit|
            bit_is_set(&magnitude, usize::from(bit)));
        let sticky = highest.checked_sub(keep).is_some_and(|bit|
            (0..bit).any(|candidate| bit_is_set(&magnitude, usize::from(candidate))));
        if guard && (sticky || significand & 1 != 0) { significand += 1; }
        // A rounding carry gives mantissa 1 instead of .5 and keeps the same
        // exponent. No out-of-domain ScaledValue is constructed here.
        let mut mantissa = cadmpeg_core::convert::f64_from_u64(significand)?
            * 2.0_f64.powi(-i32::from(keep));
        if negative { mantissa = -mantissa; }
        let mut exponent = ORIGIN + i32::from(highest) + 1;
        for denominator in [weight, weight, weight, weight, weight, weight, widths[0], widths[1], widths[2], widths[3], widths[4]] {
            mantissa /= denominator.sign * denominator.mantissa;
            exponent -= denominator.exponent();
        }
        // The numerator mantissa is in [.5,1]; eleven divisions keep it at
        // most 2048. The bounded exponent sum stays far inside i32. Only this
        // final scaling can overflow or round into the subnormal range.
        Some(crate::math::scale_power_of_two(mantissa, exponent)
            .ok_or(mantissa.signum() * f64::INFINITY))
    }
}

