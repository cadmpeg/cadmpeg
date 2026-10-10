// SPDX-License-Identifier: Apache-2.0
//! Normalized polynomial tensor higher partials after lower projection failure.

use super::{basis, decode, finite_vector, EvaluationFailure, FiniteReal, NurbsSurfaceLocal};
use super::HigherPartials;
use crate::features::FiniteVector3;
use crate::math::sum::{ExactSignedSum, ScaledValue};

#[derive(Clone, Copy)]
pub(super) enum Orders {
    Third,
    Fourth,
    ThirdAndFourth,
}

/// Compute only the needed higher orders from one selected polynomial window.
/// The caller proves the stored polynomial representation. Both orders share
/// their actual normalized triangle and pole walk; projections are independent.
pub(super) fn evaluate(
    scratch: &decode::Scratch<'_, '_>,
    local: &NurbsSurfaceLocal<'_>,
    orders: Orders,
) -> Result<HigherPartials, EvaluationFailure<()>> {
    match orders {
        Orders::Third => evaluate_rows::<4>(scratch, local, orders),
        Orders::Fourth | Orders::ThirdAndFourth => evaluate_rows::<5>(scratch, local, orders),
    }
}

fn evaluate_rows<const N: usize>(
    scratch: &decode::Scratch<'_, '_>,
    local: &NurbsSurfaceLocal<'_>,
    orders: Orders,
) -> Result<HigherPartials, EvaluationFailure<()>> {
    const { assert!(N == 4 || N == 5) };
    scratch.settle((|| {
        scratch.unless_refused().map_err(EvaluationFailure::ResourceLimit)?;
        let no_value = EvaluationFailure::NoValue;
        let knots = [local.surface.u_knots(), local.surface.v_knots()];
        let width = |axis: usize| {
            let span = local.spans[axis];
            if knots[axis][span + 1] <= knots[axis][span] { return None; }
            let mut sum = ExactSignedSum::default();
            sum.add_factors([knots[axis][span + 1]]);
            sum.add_factors([-knots[axis][span]]);
            sum.finish()
        };
        let widths = [width(0).ok_or(no_value)?, width(1).ok_or(no_value)?];
        let rows = |axis: usize| basis::requested_third::rows::<N>(
            scratch, knots[axis], local.degrees[axis], local.spans[axis],
            FiniteReal::new(local.parameters[axis])?, widths[axis],
        );
        let u = rows(0).ok_or_else(|| scratch.failure(no_value))?;
        let v = rows(1).ok_or_else(|| scratch.failure(no_value))?;
        let want_third = !matches!(orders, Orders::Fourth);
        let want_fourth = !matches!(orders, Orders::Third);
        let fourth_available = N == 5 && u.fourth_available() && v.fourth_available();
        if !want_third && !fourth_available { return Err(no_value); }
        let u = u.as_slice();
        let v = v.as_slice();
        let count = u.len().checked_mul(v.len()).ok_or(no_value)?;
        let variable = count > 2;
        let operation = if want_fourth { "IR polynomial surface fourth pole traversal" }
            else { "IR polynomial surface third pole traversal" };
        scratch.admission.work(0, operation).map_err(EvaluationFailure::ResourceLimit)?;
        let mut third: Option<[[ExactSignedSum; 3]; 4]> = want_third.then(|| std::array::from_fn(|_| std::array::from_fn(|_| ExactSignedSum::default())));
        let mut fourth: Option<[[ExactSignedSum; 3]; 5]> = (want_fourth && fourth_available).then(|| std::array::from_fn(|_| std::array::from_fn(|_| ExactSignedSum::default())));
        for at in 0..count {
            if variable { scratch.admission.independent_cost(Some(1))?; }
            scratch.admission.work(u64::from(variable), operation).map_err(EvaluationFailure::ResourceLimit)?;
            let (i, j) = (at / v.len(), at % v.len());
            let point = local.surface.pole(local.spans[0] - local.degrees[0] + i,
                local.spans[1] - local.degrees[1] + j).ok_or(no_value)?;
            if let Some(sums) = &mut third {
                for (order, lanes) in sums.iter_mut().enumerate() {
                    for (sum, coordinate) in lanes.iter_mut().zip(point.coordinates()) {
                        sum.add_factors([u[i][3 - order], v[j][order], coordinate.get()]);
                    }
                }
            }
            if let Some(sums) = &mut fourth {
                for (order, lanes) in sums.iter_mut().enumerate() {
                    for (sum, coordinate) in lanes.iter_mut().zip(point.coordinates()) {
                        sum.add_factors([u[i][4 - order], v[j][order], coordinate.get()]);
                    }
                }
            }
        }
        let third = third.map_or(Err(no_value), |sums| project::<4, 3>(sums, widths));
        let fourth = fourth.map_or(Err(no_value), |sums| project::<5, 4>(sums, widths));
        Ok(match orders {
            Orders::Third => HigherPartials::Third(third),
            Orders::Fourth | Orders::ThirdAndFourth => HigherPartials::Fourth { third, fourth },
        })
    })())
}

/// Three finite factors fit the existing exact accumulator. Each requested
/// order divides by its actual span factors, independently of the other order.
fn project<const N: usize, const D: usize>(
    sums: [[ExactSignedSum; 3]; N],
    widths: [ScaledValue; 2],
) -> Result<[FiniteVector3; N], EvaluationFailure<()>> {
    const { assert!((N == 4 && D == 3) || (N == 5 && D == 4)) };
    let mut output = [FiniteVector3::ZERO; N];
    for (order, (lanes, destination)) in sums.into_iter().zip(&mut output).enumerate() {
        let denominators = std::array::from_fn(|factor| widths[usize::from(factor >= D - order)]);
        let mut coordinates = [FiniteReal::ZERO; 3];
        for (sum, destination) in lanes.into_iter().zip(&mut coordinates) {
            if let Some(sum) = sum.finish() {
                *destination = sum.quotient_by_factors::<D>(denominators, false)
                    .map_err(|_| EvaluationFailure::NonFinite(()))?;
            }
        }
        *destination = finite_vector(coordinates);
    }
    Ok(output)
}
