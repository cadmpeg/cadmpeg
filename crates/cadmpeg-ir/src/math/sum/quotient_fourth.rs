// SPDX-License-Identifier: Apache-2.0
//! Complete rational fourth numerator before final span and weight division.

use super::expanded::Numerator;
use super::ScaledValue;
use crate::scalar::FiniteReal;

/// C4 for C=H/W in a local coordinate whose physical width is `width`.
/// Every supplied order is present; None represents exact zero, not an
/// unavailable derivative. A zero W leaves no value. The complete signed
/// numerator cancels before rounding and division by W^5 * width^4.
/// Homogeneous orders retain their existing 53-bit rounding; this operation
/// does not certify a correctly rounded rational quotient.
pub(crate) fn quotient_fourth(
    h: [Option<ScaledValue>; 5],
    w: [Option<ScaledValue>; 5],
    width: ScaledValue,
) -> Option<Result<FiniteReal, f64>> {
    let weight = w[0]?;
    let mut numerator = Numerator::<5, 695, 5>::new();
    numerator.add_product([h[4], w[0], w[0], w[0], w[0]], false, 1)?;
    numerator.add_product([h[3], w[0], w[0], w[0], w[1]], true, 4)?;
    numerator.add_product([h[2], w[0], w[0], w[0], w[2]], true, 6)?;
    numerator.add_product([h[2], w[0], w[0], w[1], w[1]], false, 12)?;
    numerator.add_product([h[1], w[0], w[0], w[0], w[3]], true, 4)?;
    numerator.add_product([h[1], w[0], w[0], w[1], w[2]], false, 24)?;
    numerator.add_product([h[1], w[0], w[1], w[1], w[1]], true, 24)?;
    numerator.add_product([h[0], w[0], w[0], w[0], w[4]], true, 1)?;
    numerator.add_product([h[0], w[0], w[0], w[1], w[3]], false, 8)?;
    numerator.add_product([h[0], w[0], w[0], w[2], w[2]], false, 6)?;
    numerator.add_product([h[0], w[0], w[1], w[1], w[2]], true, 36)?;
    numerator.add_product([h[0], w[1], w[1], w[1], w[1]], false, 24)?;
    numerator.divide(weight, [width; 4])
}

pub(crate) mod mixed;

#[cfg(test)]
mod tests;
