// SPDX-License-Identifier: Apache-2.0
//! Complete normalized rational fifth raw orders with the actual evaluated base.

use super::{decode, ExactSignedSum, Homogeneous, ScaledValue};
use super::tensor::TensorWindow;
use crate::eval::EvaluationFailure;
use crate::features::FiniteVector3;
use crate::geometry::nurbs::NurbsSurface;
use crate::scalar::FiniteReal;

impl Homogeneous {
    /// Every supplied axis order is available. Each selected source pole
    /// participates in the constant-coordinate proof and the one raw traversal.
    pub(in crate::eval) fn surface_fifth(
        self,
        scratch: &decode::Scratch<'_, '_>,
        surface: &NurbsSurface,
        axes: [(usize, &[[f64; 6]]); 2],
        widths: [ScaledValue; 2],
    ) -> Result<[FiniteVector3; 6], EvaluationFailure<()>> {
        scratch.settle((|| {
            scratch.unless_refused().map_err(EvaluationFailure::ResourceLimit)?;
            let no_value = EvaluationFailure::NoValue;
            self.values[3].ok_or(no_value)?;
            let [(u_start, u), (v_start, v)] = axes;
            let count = u.len().checked_mul(v.len()).ok_or(no_value)?;
            let variable = count > 2;
            const OPERATION: &str = "IR rational surface fifth pole traversal";
            scratch.admission.work(0, OPERATION).map_err(EvaluationFailure::ResourceLimit)?;
            let window = TensorWindow::new(surface, [u_start, v_start]);
            let mut sums: [[ExactSignedSum; 4]; 20] =
                std::array::from_fn(|_| std::array::from_fn(|_| ExactSignedSum::default()));
            let mut constant = [None; 3];
            for at in 0..count {
                if variable { scratch.admission.independent_cost(Some(1))?; }
                scratch.admission.work(u64::from(variable), OPERATION).map_err(EvaluationFailure::ResourceLimit)?;
                let (i, j) = (at / v.len(), at % v.len());
                let pole = window.pole([i, j]).ok_or(no_value)?;
                for (axis, coordinate) in pole.point.coordinates().into_iter().enumerate() {
                    if at == 0 { constant[axis] = Some(coordinate); }
                    else if constant[axis] != Some(coordinate) { constant[axis] = None; }
                }
                let mut ordinal = 0;
                for degree in 1..=5 {
                    for v_order in 0..=degree {
                        window.add_partial(&mut sums[ordinal], [u[i][degree - v_order], v[j][v_order]],
                            [i, j], pole, [degree > v_order, v_order > 0]).ok_or(no_value)?;
                        ordinal += 1;
                    }
                }
            }
            let mut values = [self.values; 21];
            for (target, sum) in values[1..].iter_mut().zip(sums) {
                *target = sum.map(ExactSignedSum::finish);
            }
            let weights = values.map(|row| row[3]);
            let mut output = [[FiniteReal::ZERO; 3]; 6];
            for (axis, coordinate) in constant.into_iter().enumerate() {
                if coordinate.is_some() { continue; }
                let homogeneous = values.map(|row| row[axis]);
                let partials = crate::math::sum::quotient_fifth::quotient_fifth_partials(homogeneous, weights, widths)
                    .ok_or(no_value)?;
                for (destination, partial) in output.iter_mut().zip(partials) {
                    destination[axis] = partial.map_err(|_| EvaluationFailure::NonFinite(()))?;
                }
            }
            Ok(output.map(|[x, y, z]| FiniteVector3::from_components(x, y, z)))
        })())
    }
}
