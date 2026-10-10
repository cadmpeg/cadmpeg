// SPDX-License-Identifier: Apache-2.0
//! Complete mixed rational fourth numerators from triangular H/W orders.

use super::{quotient_fourth, Numerator, ScaledValue};
use crate::scalar::FiniteReal;

const TRANSPOSE: [usize; 15] = [0, 2, 1, 5, 4, 3, 9, 8, 7, 6, 14, 13, 12, 11, 10];
const UUUV: [(usize, [usize; 4], bool, usize); 21] = [
    (0, [2, 1, 1, 1], false, 24),
    (0, [2, 1, 3, 0], true, 18),
    (0, [2, 6, 0, 0], false, 2),
    (0, [1, 1, 4, 0], true, 18),
    (0, [1, 7, 0, 0], false, 6),
    (0, [4, 3, 0, 0], false, 6),
    (0, [11, 0, 0, 0], true, 1),
    (2, [1, 1, 1, 0], true, 6),
    (2, [1, 3, 0, 0], false, 6),
    (2, [6, 0, 0, 0], true, 1),
    (1, [2, 1, 1, 0], true, 18),
    (1, [2, 3, 0, 0], false, 6),
    (1, [1, 4, 0, 0], false, 12),
    (1, [7, 0, 0, 0], true, 3),
    (4, [1, 1, 0, 0], false, 6),
    (4, [3, 0, 0, 0], true, 3),
    (3, [2, 1, 0, 0], false, 6),
    (3, [4, 0, 0, 0], true, 3),
    (7, [1, 0, 0, 0], true, 3),
    (6, [2, 0, 0, 0], true, 1),
    (11, [0, 0, 0, 0], false, 1),
];
const UUVV: [(usize, [usize; 4], bool, usize); 26] = [
    (0, [2, 2, 1, 1], false, 24),
    (0, [2, 2, 3, 0], true, 6),
    (0, [2, 1, 4, 0], true, 24),
    (0, [2, 7, 0, 0], false, 4),
    (0, [5, 1, 1, 0], true, 6),
    (0, [5, 3, 0, 0], false, 2),
    (0, [1, 8, 0, 0], false, 4),
    (0, [4, 4, 0, 0], false, 4),
    (0, [12, 0, 0, 0], true, 1),
    (2, [2, 1, 1, 0], true, 12),
    (2, [2, 3, 0, 0], false, 4),
    (2, [1, 4, 0, 0], false, 8),
    (2, [7, 0, 0, 0], true, 2),
    (5, [1, 1, 0, 0], false, 2),
    (5, [3, 0, 0, 0], true, 1),
    (1, [2, 2, 1, 0], true, 12),
    (1, [2, 4, 0, 0], false, 8),
    (1, [5, 1, 0, 0], false, 4),
    (1, [8, 0, 0, 0], true, 2),
    (4, [2, 1, 0, 0], false, 8),
    (4, [4, 0, 0, 0], true, 4),
    (8, [1, 0, 0, 0], true, 2),
    (3, [2, 2, 0, 0], false, 2),
    (3, [5, 0, 0, 0], true, 1),
    (7, [2, 0, 0, 0], true, 2),
    (12, [0, 0, 0, 0], false, 1),
];

/// Every supplied homogeneous order is complete; None is exact zero.
/// The fixed triangle is ordered by total degree, then increasing v order.
pub(crate) fn quotient_fourth_partials(
    h: [Option<ScaledValue>; 15],
    w: [Option<ScaledValue>; 15],
    widths: [ScaledValue; 2],
) -> Option<[Result<FiniteReal, f64>; 5]> {
    let pure = |axis: usize| quotient_fourth(
        std::array::from_fn(|order| h[order * (order + 1) / 2 + axis * order]),
        std::array::from_fn(|order| w[order * (order + 1) / 2 + axis * order]),
        widths[axis],
    );
    Some([
        pure(0)?,
        mixed(&h, &w, widths, &UUUV, false, 3)?,
        mixed(&h, &w, widths, &UUVV, false, 2)?,
        mixed(&h, &w, widths, &UUUV, true, 3)?,
        pure(1)?,
    ])
}

fn mixed(
    h: &[Option<ScaledValue>; 15],
    w: &[Option<ScaledValue>; 15],
    widths: [ScaledValue; 2],
    table: &[(usize, [usize; 4], bool, usize)],
    transpose: bool,
    u_order: usize
) -> Option<Result<FiniteReal, f64>> {
    let index = |at: usize| if transpose { TRANSPOSE[at] } else { at };
    let mut numerator = Numerator::new();
    for &(coordinate, weights, negative, copies) in table {
        let mut factors = [w[0]; 5];
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
