// SPDX-License-Identifier: Apache-2.0
//! Normalized polynomial tensor fourth partials after lower projection failure.

use super::{basis, decode, EvaluationFailure, FiniteReal, NurbsSurfaceLocal};
use crate::math::sum::ExactSignedSum;

/// Evaluate all five fourth partials from the selected polynomial pole window.
/// The caller proves the stored polynomial representation. No lower projected
/// derivative, point sum or weight derivative is evaluated here.
pub(super) fn evaluate(
    scratch: &decode::Scratch<'_, '_>,
    local: &NurbsSurfaceLocal<'_>,
) -> Result<[[FiniteReal; 3]; 5], EvaluationFailure<()>> {
    scratch.settle((|| {
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
        let rows = |axis: usize| basis::requested_third::rows::<5>(
            scratch, knots[axis], local.degrees[axis], local.spans[axis],
            FiniteReal::new(local.parameters[axis])?, widths[axis],
        );
        let u = rows(0).ok_or_else(|| scratch.failure(no_value))?;
        let v = rows(1).ok_or_else(|| scratch.failure(no_value))?;
        if !u.fourth_available() || !v.fourth_available() { return Err(no_value); }
        let u = u.as_slice();
        let v = v.as_slice();
        let count = u.len().checked_mul(v.len()).ok_or(no_value)?;
        let variable = count > 2;
        const OPERATION: &str = "IR polynomial surface fourth pole traversal";
        scratch.admission.work(0, OPERATION).map_err(EvaluationFailure::ResourceLimit)?;
        let mut sums: [[ExactSignedSum; 3]; 5] = std::array::from_fn(|_| std::array::from_fn(|_| ExactSignedSum::default()));
        for at in 0..count {
            if variable { scratch.admission.independent_cost(Some(1))?; }
            scratch.admission.work(u64::from(variable), OPERATION).map_err(EvaluationFailure::ResourceLimit)?;
            let (i, j) = (at / v.len(), at % v.len());
            let point = local.surface.pole(local.spans[0] - local.degrees[0] + i,
                local.spans[1] - local.degrees[1] + j).ok_or(no_value)?;
            for (order, lanes) in sums.iter_mut().enumerate() {
                let (du, dv) = (4 - order, order);
                for (sum, coordinate) in lanes.iter_mut().zip(point.coordinates()) {
                    // Three finite factors fit the existing exact accumulator.
                    sum.add_factors([u[i][du], v[j][dv], coordinate.get()]);
                }
            }
        }
        let mut output = [[FiniteReal::ZERO; 3]; 5];
        for (order, (lanes, destination)) in sums.into_iter().zip(&mut output).enumerate() {
            let denominators = std::array::from_fn(|factor| widths[usize::from(factor >= 4 - order)]);
            for (sum, destination) in lanes.into_iter().zip(destination) {
                if let Some(sum) = sum.finish() {
                    *destination = sum.quotient_by_factors::<4>(denominators, false)
                        .map_err(|_| EvaluationFailure::NonFinite(()))?;
                }
            }
        }
        Ok(output)
    })())
}
