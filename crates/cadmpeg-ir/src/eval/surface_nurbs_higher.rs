// SPDX-License-Identifier: Apache-2.0
//! Fourth tensor-product quotient partials over completed lower-order state.

use super::{basis, decode, finite_lanes, EvaluationFailure, Homogeneous,
    NurbsSurfaceFirstPartials, NurbsSurfaceLocal, NurbsSurfaceSecondPartials,
    NurbsSurfaceThirdPartials};
use crate::scalar::FiniteReal;

impl NurbsSurfaceLocal<'_> {
    pub(super) fn fourth(
        &self,
        scratch: &decode::Scratch<'_, '_>,
        first: &NurbsSurfaceFirstPartials,
        second: &NurbsSurfaceSecondPartials,
        third: &NurbsSurfaceThirdPartials,
    ) -> Result<[[FiniteReal; 3]; 5], EvaluationFailure<()>> {
        scratch.settle((|| {
            scratch.admission.independent_cost(fourth_evaluation_cost(self.degrees))?;
            let non_finite = EvaluationFailure::NonFinite(());
            let knots = [self.surface.u_knots(), self.surface.v_knots()];
            let basis = |axis: usize| basis::bspline_basis_fourth_derivative(
                scratch, knots[axis], self.degrees[axis], self.spans[axis], self.parameters[axis],
            ).ok_or_else(|| scratch.failure(non_finite));
            let bases = [basis(0)?, basis(1)?];
            let sum = |active, u: &[f64], v: &[f64]| if active {
                self.derivative_sum(scratch, u, v).ok_or(non_finite)
            } else { Ok(Homogeneous::zero()) };
            let [u_degree, v_degree] = self.degrees;
            let uuuu = sum(u_degree >= 4, &bases[0], &self.bases[1])?;
            let uuuv = sum(u_degree >= 3 && v_degree >= 1, &third.bases[0], &first.bases[1])?;
            let uuvv = sum(u_degree >= 2 && v_degree >= 2, &second.bases[0], &second.bases[1])?;
            let uvvv = sum(u_degree >= 1 && v_degree >= 3, &first.bases[0], &third.bases[1])?;
            let vvvv = sum(v_degree >= 4, &self.bases[0], &bases[1])?;
            let [u, v] = first.sums;
            let [uu, uv, vv] = second.sums;
            let [uuu, uuv, uvv, vvv] = third.sums;
            let [du, dv] = first.lanes;
            let [duu, duv, dvv] = second.lanes;
            let [duuu, duuv, duvv, dvvv] = third.lanes;
            // Each coefficient is the product-rule binomial multiplicity.
            // Repeated terms avoid an overflowing binary64 coefficient product.
            let lane = |sum: Homogeneous, corrections: &[(Homogeneous, [FiniteReal; 3])]| {
                finite_lanes(sum.project(self.base, corrections).ok_or(non_finite)?).map_err(|_| non_finite)
            };
            Ok([
                lane(uuuu, &[
                    (uuuu, self.point),
                    (uuu, du), (uuu, du), (uuu, du), (uuu, du),
                    (uu, duu), (uu, duu), (uu, duu), (uu, duu), (uu, duu), (uu, duu),
                    (u, duuu), (u, duuu), (u, duuu), (u, duuu),
                ])?,
                lane(uuuv, &[
                    (uuuv, self.point),
                    (uuv, du), (uuv, du), (uuv, du), (uuu, dv),
                    (uv, duu), (uv, duu), (uv, duu), (uu, duv), (uu, duv), (uu, duv),
                    (v, duuu), (u, duuv), (u, duuv), (u, duuv),
                ])?,
                lane(uuvv, &[
                    (uuvv, self.point), (uvv, du), (uvv, du), (uuv, dv), (uuv, dv),
                    (vv, duu), (uv, duv), (uv, duv), (uv, duv), (uv, duv), (uu, dvv),
                    (v, duuv), (v, duuv), (u, duvv), (u, duvv),
                ])?,
                lane(uvvv, &[
                    (uvvv, self.point), (vvv, du), (uvv, dv), (uvv, dv), (uvv, dv),
                    (vv, duv), (vv, duv), (vv, duv), (uv, dvv), (uv, dvv), (uv, dvv),
                    (v, duvv), (v, duvv), (v, duvv), (u, dvvv),
                ])?,
                lane(vvvv, &[
                    (vvvv, self.point),
                    (vvv, dv), (vvv, dv), (vvv, dv), (vvv, dv),
                    (vv, dvv), (vv, dvv), (vv, dvv), (vv, dvv), (vv, dvv), (vv, dvv),
                    (v, dvvv), (v, dvvv), (v, dvvv), (v, dvvv),
                ])?,
            ])
        })())
    }
}

/// Actual degree-4 base and four derivative-row writes, then active pole walks.
pub(super) fn fourth_evaluation_cost([u_degree, v_degree]: [usize; 2]) -> Option<usize> {
    let basis_work = |degree: usize| {
        if degree < 4 { return Some(0); }
        let base = degree - 4;
        let base_work = if base <= 1 { 0 } else {
            base.checked_add(2)?
                .checked_add(base.checked_mul(base.checked_add(1)?)?.checked_div(2)?)?
                .checked_add(base)?
        };
        base_work.checked_add(degree.checked_mul(4)?.checked_sub(2)?)
    };
    let supports = u_degree.checked_add(1)?.checked_mul(v_degree.checked_add(1)?)?;
    let sums = usize::from(u_degree >= 4)
        + usize::from(u_degree >= 3 && v_degree >= 1)
        + usize::from(u_degree >= 2 && v_degree >= 2)
        + usize::from(u_degree >= 1 && v_degree >= 3)
        + usize::from(v_degree >= 4);
    let visits = if supports > 2 { supports.checked_mul(sums)? } else { 0 };
    basis_work(u_degree)?.checked_add(basis_work(v_degree)?)?.checked_add(visits)
}

#[cfg(test)]
pub(in crate::eval) mod tests;
