// SPDX-License-Identifier: Apache-2.0
//! Requested fifth polynomial partials from the selected normalized tensor.

use super::{basis, decode, EvaluationFailure, ExactSignedSum, FiniteReal, FiniteVector3, NurbsSurfaceLocal, TensorWindow};
use crate::geometry::nurbs::NurbsPoleGrid;

pub(in crate::eval::surface_nurbs) fn evaluate(
    scratch: &decode::Scratch<'_, '_>,
    local: &NurbsSurfaceLocal<'_>,
) -> Result<[FiniteVector3; 6], EvaluationFailure<()>> {
    scratch.settle((|| {
        scratch.unless_refused().map_err(EvaluationFailure::ResourceLimit)?;
        let no_value = EvaluationFailure::NoValue;
        if !matches!(local.surface.pole_grid(), NurbsPoleGrid::Polynomial { .. }) {
            return Err(no_value);
        }
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
        let rows = |axis: usize| basis::requested_third::rows::<6>(
            scratch, knots[axis], local.degrees[axis], local.spans[axis],
            FiniteReal::new(local.parameters[axis])?, widths[axis],
        );
        let u = rows(0).ok_or_else(|| scratch.failure(no_value))?;
        let v = rows(1).ok_or_else(|| scratch.failure(no_value))?;
        for (axis, rows) in [&u, &v].into_iter().enumerate() {
            if local.degrees[axis] >= 5 && !rows.fifth_available()
                || local.degrees[axis] >= 4 && local.degrees[1 - axis] >= 1 && !rows.fourth_available()
            {
                return Err(no_value);
            }
        }
        let (u, v) = (u.as_slice(), v.as_slice());
        let count = u.len().checked_mul(v.len()).ok_or(no_value)?;
        let variable = count > 2;
        const OPERATION: &str = "IR polynomial surface fifth pole traversal";
        scratch.admission.work(0, OPERATION).map_err(EvaluationFailure::ResourceLimit)?;
        let window = TensorWindow::new(local.surface, [
            local.spans[0] - local.degrees[0], local.spans[1] - local.degrees[1],
        ]);
        let mut sums: [[ExactSignedSum; 3]; 6] =
            std::array::from_fn(|_| std::array::from_fn(|_| ExactSignedSum::default()));
        for at in 0..count {
            if variable { scratch.admission.independent_cost(Some(1))?; }
            scratch.admission.work(u64::from(variable), OPERATION).map_err(EvaluationFailure::ResourceLimit)?;
            let (i, j) = (at / v.len(), at % v.len());
            let pole = window.pole([i, j]).ok_or(no_value)?;
            for (order, lanes) in sums.iter_mut().enumerate() {
                window.add_partial(lanes, [u[i][5 - order], v[j][order]],
                    [i, j], pole, [order < 5, order > 0]).ok_or(no_value)?;
            }
        }
        // Every expanded term still has four finite factors. Divide by the
        // five actual span factors in extended range before final rounding.
        super::project::<6, 5>(sums, widths)
    })())
}
