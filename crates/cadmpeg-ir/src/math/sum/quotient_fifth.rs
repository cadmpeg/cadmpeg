// SPDX-License-Identifier: Apache-2.0
//! Complete rational fifth numerator before final span and weight division.

use super::expanded::Numerator;
use super::ScaledValue;
use crate::scalar::FiniteReal;

mod terms;

/// The univariate fifth reads only the nineteen pure numerator terms.
/// None is exact zero; all six source orders must be available.
pub(crate) fn quotient_fifth(
    h: [Option<ScaledValue>; 6],
    w: [Option<ScaledValue>; 6],
    width: ScaledValue,
) -> Option<Result<FiniteReal, f64>> {
    let order = |index| [0, 1, 3, 6, 10, 15].iter().position(|candidate| *candidate == index);
    let mut numerator = Numerator::<6, 834, 5>::new();
    for &(coordinate, weights, negative, copies) in &terms::PURE {
        let mut factors = [w[0]; 6];
        factors[0] = h[order(coordinate)?];
        for (destination, source) in factors[1..].iter_mut().zip(weights) {
            *destination = w[order(source)?];
        }
        numerator.add_product(factors, negative, copies)?;
    }
    numerator.divide(w[0]?, [width; 5], [])
}

/// One actual six-factor numerator divided by W^6 and h^5.
/// This includes the fixed linear fifth and five-factor chain rule; the
/// latter supplies the actual unit denominators. No intermediate product
/// is converted to binary64 or to an out-of-domain ScaledValue.
pub(crate) fn product_fifth(
    factors: [Option<ScaledValue>; 6],
    weight: ScaledValue,
    width: ScaledValue,
) -> Option<Result<FiniteReal, f64>> {
    let mut numerator = Numerator::<6, 834, 5>::new();
    numerator.add_product(factors, false, 1)?;
    numerator.divide(weight, [width; 5], [])
}

/// Complete normalized H/W triangle ordered by degree, then increasing v order.
/// None is an exact zero; the caller must establish every order's availability.
/// Signed numerator cancellation precedes division by W0^6 and five span factors.
/// Existing homogeneous rounding does not certify correctly rounded quotients.
pub(crate) fn quotient_fifth_partials(
    h: [Option<ScaledValue>; 21],
    w: [Option<ScaledValue>; 21],
    widths: [ScaledValue; 2],
) -> Option<[Result<FiniteReal, f64>; 6]> {
    let weight = w[0]?;
    let partial = |table: &[terms::Term], transpose: bool, u_order: usize| {
        let index = |at| if transpose { terms::TRANSPOSE[at] } else { at };
        let mut numerator = Numerator::<6, 834, 5>::new();
        for &(coordinate, weights, negative, copies) in table {
            let mut factors = [w[0]; 6];
            factors[0] = h[index(coordinate)];
            for (destination, source) in factors[1..].iter_mut().zip(weights) {
                *destination = w[index(source)];
            }
            numerator.add_product(factors, negative, copies)?;
        }
        let denominators: [ScaledValue; 5] = std::array::from_fn(|at| widths[usize::from((at >= u_order) ^ transpose)]);
        numerator.divide(weight, denominators, [])
    };
    Some([
        partial(&terms::PURE, false, 5)?, partial(&terms::UUUUV, false, 4)?,
        partial(&terms::UUUVV, false, 3)?, partial(&terms::UUUVV, true, 3)?,
        partial(&terms::UUUUV, true, 4)?, partial(&terms::PURE, true, 5)?,
    ])
}

#[cfg(test)]
mod tests;
