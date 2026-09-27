// SPDX-License-Identifier: Apache-2.0
//! Cox–de Boor basis values written into a caller-owned buffer.
use super::difference_quotient;
use crate::math::sum::scaled_ratio_products;
use crate::scalar::FiniteReal;

/// Writes the non-zero basis values at `t` for `span` into `values`, which
/// holds exactly `degree + 1` entries. `None` states a buffer of another
/// length, or a term that left the finite range.
pub(super) fn fill_bspline_basis(
    knots: &[f64],
    degree: usize,
    span: usize,
    t: f64,
    values: &mut [f64],
) -> Option<()> {
    if values.len() != degree.checked_add(1)? {
        return None;
    }
    let finite_t = FiniteReal::new(t);
    values[0] = 1.0;
    for j in 1..=degree {
        let mut saved = 0.0;
        for r in 0..j {
            let value = values[r];
            // Each knot distance is admitted where it is formed.
            let right = FiniteReal::new(knots[span + r + 1] - t);
            let left = FiniteReal::new(t - knots[span + 1 - j + r]);
            // Two finite distances with a finite sum form the scaled ratio. A
            // distance or a sum outside the finite range takes the exact knot
            // differences instead.
            let ratio_terms = right.zip(left).and_then(|(right, left)| {
                Some((right, left, FiniteReal::new(right.get() + left.get())?))
            });
            let [right_term, left_term] = if let Some((right, left, denominator)) = ratio_terms {
                scaled_ratio_products(FiniteReal::new(value)?, denominator, [right, left])?
                    .map(FiniteReal::get)
            } else {
                let t = finite_t?;
                let [right_knot, left_knot] =
                    FiniteReal::array([knots[span + r + 1], knots[span + 1 - j + r]])?;
                [
                    value
                        * difference_quotient(right_knot, t, right_knot, left_knot)
                            .ok()?
                            .get(),
                    value
                        * difference_quotient(t, left_knot, right_knot, left_knot)
                            .ok()?
                            .get(),
                ]
            };
            values[r] = saved + right_term;
            saved = left_term;
        }
        values[j] = saved;
    }
    Some(())
}
