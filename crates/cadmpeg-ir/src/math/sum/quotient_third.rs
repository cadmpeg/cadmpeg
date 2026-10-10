// SPDX-License-Identifier: Apache-2.0
//! Complete rational third numerator before final span and weight division.

use super::expanded::Numerator;
use super::ScaledValue;
use crate::scalar::FiniteReal;

/// C''' for C=H/W in a local coordinate whose physical width is `width`.
/// Every supplied order is present; None represents exact zero, not an
/// unavailable derivative. A zero W leaves no value. The complete signed
/// numerator cancels before rounding and division by W^4 * width^3.
/// Homogeneous orders retain their existing 53-bit rounding; this operation
/// does not certify a correctly rounded rational quotient.
/// Fixed normalized multipliers join the ratio before final range admission.
pub(crate) fn quotient_third<const MULTIPLIERS: usize>(
    h: [Option<ScaledValue>; 4],
    w: [Option<ScaledValue>; 4],
    width: ScaledValue,
    multipliers: [ScaledValue; MULTIPLIERS],
) -> Option<Result<FiniteReal, f64>> {
    let weight = w[0]?;
    let mut numerator = Numerator::<4, 556, 4>::new();
    numerator.add_product([h[3], w[0], w[0], w[0]], false, 1)?;
    numerator.add_product([h[2], w[0], w[0], w[1]], true, 3)?;
    numerator.add_product([h[1], w[0], w[0], w[2]], true, 3)?;
    numerator.add_product([h[1], w[0], w[1], w[1]], false, 6)?;
    numerator.add_product([h[0], w[0], w[0], w[3]], true, 1)?;
    numerator.add_product([h[0], w[0], w[1], w[2]], false, 6)?;
    numerator.add_product([h[0], w[1], w[1], w[1]], true, 6)?;
    numerator.divide(weight, [width; 3], multipliers)
}

pub(crate) mod mixed;

#[cfg(test)]
mod tests;
