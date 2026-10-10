// SPDX-License-Identifier: Apache-2.0
//! Requested local angle derivatives from actual finite radial derivatives.

use super::EvaluationFailure;
use crate::math::sum::{ExactSignedSum, ScaledValue};
use crate::scalar::{FiniteReal, NonZeroReal};
use crate::units::FinitePoint2;

type RadialOrder = Result<FinitePoint2, EvaluationFailure<()>>;

/// theta'=F/D, F=cross(r,r'), D=dot(r,r). Each supplied order is its
/// actual derivative. An unavailable order is never an exact-zero factor.
/// Form each requested F/D prefix once; preserve separate order outcomes.
/// A supplied affine phase rate contributes its order power before final
/// quotient range admission. No rate keeps the supplied parameterization.
pub(super) fn angular(radial: [RadialOrder; 6], max_order: usize, phase_rate: Option<NonZeroReal>)
    -> [Result<FiniteReal, EvaluationFailure<()>>; 3]
{
    let no_value = EvaluationFailure::NoValue;
    let mut f = [None; 5];
    let mut d = [None; 5];
    let mut available = [Err(no_value); 5];
    if max_order < 3 { return [Err(no_value); 3]; }
    for order in 0..max_order.min(5) {
        let completed = (|| {
            let r0 = radial[0]?;
            let next = radial[order + 1]?;
            let mut numerator = ExactSignedSum::default();
            let mut denominator = ExactSignedSum::default();
            cross(&mut numerator, r0, next, 1);
            match order {
                0 => dot(&mut denominator, r0, r0, 1),
                1 => dot(&mut denominator, r0, radial[1]?, 2),
                2 => {
                    cross(&mut numerator, radial[1]?, radial[2]?, 1);
                    dot(&mut denominator, r0, radial[2]?, 2);
                    dot(&mut denominator, radial[1]?, radial[1]?, 2);
                }
                3 => {
                    cross(&mut numerator, radial[1]?, radial[3]?, 2);
                    dot(&mut denominator, r0, radial[3]?, 2);
                    dot(&mut denominator, radial[1]?, radial[2]?, 6);
                }
                _ => {
                    cross(&mut numerator, radial[1]?, radial[4]?, 3);
                    cross(&mut numerator, radial[2]?, radial[3]?, 2);
                    dot(&mut denominator, r0, radial[4]?, 2);
                    dot(&mut denominator, radial[1]?, radial[3]?, 8);
                    dot(&mut denominator, radial[2]?, radial[2]?, 6);
                }
            }
            Ok((numerator.finish(), denominator.finish()))
        })();
        available[order] = completed.map(|(numerator, denominator)| {
            f[order] = numerator;
            d[order] = denominator;
        });
    }
    let unit = ScaledValue::of_nonzero(NonZeroReal::ONE);
    let phase_rate = phase_rate.map(ScaledValue::of_nonzero);
    std::array::from_fn(|at| {
        let order = at + 3;
        if order > max_order { return Err(no_value); }
        for state in &available[..order] { (*state)?; }
        let value = match (order, phase_rate) {
            (3, None) => crate::math::sum::quotient_second::quotient_second(
                std::array::from_fn(|n| f[n]), std::array::from_fn(|n| d[n]), unit, []),
            (4, None) => crate::math::sum::quotient_third::quotient_third(
                std::array::from_fn(|n| f[n]), std::array::from_fn(|n| d[n]), unit, []),
            (_, None) => crate::math::sum::quotient_fourth::quotient_fourth(
                f, d, unit, []),
            (3, Some(rate)) => crate::math::sum::quotient_second::quotient_second(
                std::array::from_fn(|n| f[n]), std::array::from_fn(|n| d[n]), unit, [rate; 3]),
            (4, Some(rate)) => crate::math::sum::quotient_third::quotient_third(
                std::array::from_fn(|n| f[n]), std::array::from_fn(|n| d[n]), unit, [rate; 4]),
            (_, Some(rate)) => crate::math::sum::quotient_fourth::quotient_fourth(
                f, d, unit, [rate; 5]),
        };
        value.ok_or(no_value)?.map_err(|_| EvaluationFailure::NonFinite(()))
    })
}

fn cross(sum: &mut ExactSignedSum, left: FinitePoint2, right: FinitePoint2, copies: usize) {
    for _ in 0..copies {
        sum.add_product(left.u, right.v);
        sum.add_product(-left.v, right.u);
    }
}

fn dot(sum: &mut ExactSignedSum, left: FinitePoint2, right: FinitePoint2, copies: usize) {
    for _ in 0..copies {
        sum.add_product(left.u, right.u);
        sum.add_product(left.v, right.v);
    }
}

#[cfg(test)]
mod tests;
