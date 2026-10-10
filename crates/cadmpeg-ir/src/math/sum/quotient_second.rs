// SPDX-License-Identifier: Apache-2.0
//! Complete rational second numerator before final span and weight division.

use super::expanded::Numerator;
use super::ScaledValue;
use crate::scalar::FiniteReal;

/// C'' for C=H/W. All three homogeneous orders are available; None is exact
/// zero. Cancel the four terms before division by W^3 and physical width^2.
/// Source scaled sums retain their own rounding, so this does not certify a
/// correctly rounded quotient. A zero weight has no value.
/// Fixed normalized multipliers join the ratio before final range admission.
pub(crate) fn quotient_second<const MULTIPLIERS: usize>(
    h: [Option<ScaledValue>; 3],
    w: [Option<ScaledValue>; 3],
    width: ScaledValue,
    multipliers: [ScaledValue; MULTIPLIERS],
) -> Option<Result<FiniteReal, f64>> {
    let weight = w[0]?;
    let mut numerator = Numerator::<3, 417, 3>::new();
    numerator.add_product([h[2], w[0], w[0]], false, 1)?;
    numerator.add_product([h[1], w[1], w[0]], true, 2)?;
    numerator.add_product([h[0], w[2], w[0]], true, 1)?;
    numerator.add_product([h[0], w[1], w[1]], false, 2)?;
    numerator.divide(weight, [width; 2], multipliers)
}

#[cfg(test)]
mod tests;
