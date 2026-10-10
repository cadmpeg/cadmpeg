// SPDX-License-Identifier: Apache-2.0
//! Closed extended-range numerators with final weight and span division.

use super::{add_shifted, bit_is_set, signed_difference, ScaledValue,
    MAX_SCALED_EXPONENT, MIN_SCALED_EXPONENT};
use crate::scalar::FiniteReal;

// Three through six factors need 159, 212, 265 and 318 significand bits.
// The closed tables have 6, 26, 150 and 1082 signed copies, so their sums
// need three, five, eight and eleven carry bits. Each keeps its own extent.
const _: () = assert!(3 * (MIN_SCALED_EXPONENT - 53) == -13236);
const _: () = assert!(3 * (MAX_SCALED_EXPONENT - MIN_SCALED_EXPONENT + 53) + 3 == 26658);
const _: () = assert!(417 * 64 >= 26658);
const _: () = assert!(4 * (MIN_SCALED_EXPONENT - 53) == -17648);
const _: () = assert!(5 * (MIN_SCALED_EXPONENT - 53) == -22060);
const _: () = assert!(6 * (MIN_SCALED_EXPONENT - 53) == -26472);
const _: () = assert!(4 * (MAX_SCALED_EXPONENT - MIN_SCALED_EXPONENT + 53) + 5 == 35545);
const _: () = assert!(5 * (MAX_SCALED_EXPONENT - MIN_SCALED_EXPONENT + 53) + 8 == 44433);
const _: () = assert!(6 * (MAX_SCALED_EXPONENT - MIN_SCALED_EXPONENT + 53) + 11 == 53321);
const _: () = assert!(556 * 64 >= 35545 && 695 * 64 >= 44433 && 834 * 64 >= 53321);

pub(super) struct Numerator<const FACTORS: usize, const WORDS: usize, const LIMBS: usize> {
    positive: [u64; WORDS],
    negative: [u64; WORDS],
}

impl<const FACTORS: usize, const WORDS: usize, const LIMBS: usize>
    Numerator<FACTORS, WORDS, LIMBS>
{
    const ORIGIN: i32 = match FACTORS {
        3 => 3 * (MIN_SCALED_EXPONENT - 53),
        4 => 4 * (MIN_SCALED_EXPONENT - 53),
        5 => 5 * (MIN_SCALED_EXPONENT - 53),
        _ => 6 * (MIN_SCALED_EXPONENT - 53),
    };

    pub(super) fn new() -> Self {
        const {
            assert!(matches!((FACTORS, WORDS, LIMBS), (3, 417, 3) | (4, 556, 4) | (5, 695, 5) | (6, 834, 5)));
            assert!(LIMBS * 64 >= FACTORS * 53);
            assert!(WORDS * 64 <= 65535);
        }
        Self { positive: [0; WORDS], negative: [0; WORDS] }
    }

    // None is a genuine exact zero. Callers supply only their closed table's
    // multiplicities; the complete sum stays inside its carry bound.
    pub(super) fn add_product(
        &mut self,
        factors: [Option<ScaledValue>; FACTORS],
        negative: bool,
        copies: usize,
    ) -> Option<()> {
        if factors.iter().any(Option::is_none) { return Some(()); }
        let mut product = [0_u64; LIMBS];
        product[0] = 1;
        let mut exponent = 0;
        let mut negative = negative;
        for factor in factors.into_iter().flatten() {
            negative ^= factor.sign < 0.0;
            exponent += factor.exponent() - 53;
            let significand = (1_u64 << 52) | (factor.mantissa.to_bits() & ((1_u64 << 52) - 1));
            let mut carry = 0_u128;
            for word in &mut product {
                // A 64-bit limb times 53 bits plus at most 53 carry bits fits
                // in 118 bits. The admitted limb count holds the full product.
                let value = u128::from(*word) * u128::from(significand) + carry;
                let bytes = value.to_le_bytes();
                // endian-exception: reconstructed-scalar
                *word = u64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
                carry = value >> 64;
            }
        }
        let shift = usize::from(u16::try_from(exponent - Self::ORIGIN).ok()?);
        let target = if negative { &mut self.negative } else { &mut self.positive };
        for _ in 0..copies {
            for (at, chunk) in product.chunks(2).enumerate() {
                let upper = chunk.get(1).copied().map_or(0, |word| u128::from(word) << 64);
                add_shifted(target, u128::from(chunk[0]) | upper, shift + at * 128);
            }
        }
        Some(())
    }

    pub(super) fn divide<const WIDTHS: usize>(
        self,
        weight: ScaledValue,
        widths: [ScaledValue; WIDTHS],
    ) -> Option<Result<FiniteReal, f64>> {
        const { assert!(WIDTHS + 1 == FACTORS); }
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
        // A rounding carry gives mantissa 1 instead of .5 with the same exponent.
        let mut mantissa = cadmpeg_core::convert::f64_from_u64(significand)?
            * 2.0_f64.powi(-i32::from(keep));
        if negative { mantissa = -mantissa; }
        let mut exponent = Self::ORIGIN + i32::from(highest) + 1;
        for denominator in std::iter::repeat_n(weight, FACTORS).chain(widths) {
            mantissa /= denominator.sign * denominator.mantissa;
            exponent -= denominator.exponent();
        }
        // Five, seven, nine or eleven divisions keep the mantissa at most32,
        //128,512 or2048. Only final scaling can overflow or round into subnormal range.
        Some(crate::math::scale_power_of_two(mantissa, exponent)
            .ok_or(mantissa.signum() * f64::INFINITY))
    }
}
