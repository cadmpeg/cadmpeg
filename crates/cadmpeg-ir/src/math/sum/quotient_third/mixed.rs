// SPDX-License-Identifier: Apache-2.0
//! Complete mixed rational third numerators from triangular H/W orders.

use super::{quotient_third, Numerator, ScaledValue};
use crate::scalar::FiniteReal;

const TRANSPOSE: [usize; 10] = [0, 2, 1, 5, 4, 3, 9, 8, 7, 6];
const UUV: [(usize, [usize; 3], bool, usize); 11] = [
    (0, [2, 1, 1], true, 6),
    (0, [2, 3, 0], false, 2),
    (0, [1, 4, 0], false, 4),
    (0, [7, 0, 0], true, 1),
    (2, [1, 1, 0], false, 2),
    (2, [3, 0, 0], true, 1),
    (1, [2, 1, 0], false, 4),
    (1, [4, 0, 0], true, 2),
    (4, [1, 0, 0], true, 2),
    (3, [2, 0, 0], true, 1),
    (7, [0, 0, 0], false, 1),
];

/// Every supplied homogeneous order is complete; None is exact zero.
/// The fixed triangle is ordered by total degree, then increasing v order.
pub(crate) fn quotient_third_partials(
    h: [Option<ScaledValue>; 10],
    w: [Option<ScaledValue>; 10],
    widths: [ScaledValue; 2],
) -> Option<[Result<FiniteReal, f64>; 4]> {
    let pure = |axis: usize| quotient_third(
        std::array::from_fn(|order| h[order * (order + 1) / 2 + axis * order]),
        std::array::from_fn(|order| w[order * (order + 1) / 2 + axis * order]),
        widths[axis],
    );
    Some([
        pure(0)?,
        mixed(&h, &w, widths, &UUV, false, 2)?,
        mixed(&h, &w, widths, &UUV, true, 2)?,
        pure(1)?,
    ])
}

fn mixed(
    h: &[Option<ScaledValue>; 10],
    w: &[Option<ScaledValue>; 10],
    widths: [ScaledValue; 2],
    table: &[(usize, [usize; 3], bool, usize)],
    transpose: bool,
    u_order: usize
) -> Option<Result<FiniteReal, f64>> {
    let index = |at: usize| if transpose { TRANSPOSE[at] } else { at };
    let mut numerator = Numerator::new();
    for &(coordinate, weights, negative, copies) in table {
        let mut factors = [w[0]; 4];
        factors[0] = h[index(coordinate)];
        for (destination, source) in factors[1..].iter_mut().zip(weights) {
            *destination = w[index(source)];
        }
        numerator.add_product(factors, negative, copies)?;
    }
    let denominators = std::array::from_fn(|at| widths[usize::from((at >= u_order) ^ transpose)]);
    numerator.divide(w[0]?, denominators)
}

#[cfg(test)]
mod tests;
