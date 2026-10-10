// SPDX-License-Identifier: Apache-2.0
//! Complete normalized tensor H/W orders with the already evaluated base.

use super::{Homogeneous, ExactSignedSum, ScaledValue, FiniteReal};
use super::tensor::TensorWindow;
use crate::eval::{decode, EvaluationFailure};
use crate::eval::surface_nurbs::higher::Orders;
use crate::eval::surface_request::HigherPartials;
use crate::features::FiniteVector3;
use crate::geometry::nurbs::NurbsSurface;

fn third<const C: usize>(
    rows: &[[Option<ScaledValue>; 4]; C],
    constant: [Option<FiniteReal>; 3],
    widths: [ScaledValue; 2],
) -> Result<[FiniteVector3; 4], EvaluationFailure<()>> {
    const { assert!(C == 10 || C == 15) };
    let w = std::array::from_fn(|order| rows[order][3]);
    let mut output = [[FiniteReal::ZERO; 3]; 4];
    for (axis, value) in constant.into_iter().enumerate() {
        if value.is_some() { continue; }
        let h = std::array::from_fn(|order| rows[order][axis]);
        let partials = crate::math::sum::quotient_third::mixed::quotient_third_partials(h, w, widths)
            .ok_or(EvaluationFailure::NoValue)?;
        for (target, partial) in output.iter_mut().zip(partials) {
            target[axis] = partial.map_err(|_| EvaluationFailure::NonFinite(()))?;
        }
    }
    Ok(output.map(|[x, y, z]| FiniteVector3::from_components(x, y, z)))
}

fn fourth<const C: usize>(
    rows: &[[Option<ScaledValue>; 4]; C],
    constant: [Option<FiniteReal>; 3],
    widths: [ScaledValue; 2],
) -> Result<[FiniteVector3; 5], EvaluationFailure<()>> {
    const { assert!(C == 10 || C == 15) };
    let mut output = [[FiniteReal::ZERO; 3]; 5];
    for (axis, value) in constant.into_iter().enumerate() {
        if value.is_some() { continue; }
        if C != 15 { return Err(EvaluationFailure::NoValue); }
        let w = std::array::from_fn(|order| rows[order][3]);
        let h = std::array::from_fn(|order| rows[order][axis]);
        let partials = crate::math::sum::quotient_fourth::mixed::quotient_fourth_partials(h, w, widths)
            .ok_or(EvaluationFailure::NoValue)?;
        for (target, partial) in output.iter_mut().zip(partials) {
            target[axis] = partial.map_err(|_| EvaluationFailure::NonFinite(()))?;
        }
    }
    Ok(output.map(|[x, y, z]| FiniteVector3::from_components(x, y, z)))
}


impl Homogeneous {
    /// Use the actual H00 and one selected Rational pole traversal.
    /// Every selected pole participates in the constant-coordinate theorem.
    pub(in crate::eval) fn surface_higher<const N: usize>(
        self,
        scratch: &decode::Scratch<'_, '_>,
        surface: &NurbsSurface,
        axes: [(usize, &[[f64; N]]); 2],
        widths: [ScaledValue; 2],
        orders: Orders,
        fourth_available: bool,
    ) -> Result<HigherPartials, EvaluationFailure<()>> {
        const { assert!(N == 4 || N == 5) };
        if N == 5 && !matches!(orders, Orders::Third) && fourth_available {
            self.surface_higher_rows::<N, 14, 15>(scratch, surface, axes, widths, orders)
        } else {
            self.surface_higher_rows::<N, 9, 10>(scratch, surface, axes, widths, orders)
        }
    }

    fn surface_higher_rows<const N: usize, const S: usize, const C: usize>(
        self,
        scratch: &decode::Scratch<'_, '_>,
        surface: &NurbsSurface,
        axes: [(usize, &[[f64; N]]); 2],
        widths: [ScaledValue; 2],
        orders: Orders,
    ) -> Result<HigherPartials, EvaluationFailure<()>> {
        const { assert!((S == 9 && C == 10) || (S == 14 && C == 15)) };
        // Only the N=5 dispatch above executes C=15. Rust also instantiates
        // the unreachable N=4 generic branch during code generation.
        scratch.settle((|| {
            scratch.unless_refused().map_err(EvaluationFailure::ResourceLimit)?;
            let no_value = EvaluationFailure::NoValue;
            self.values[3].ok_or(no_value)?;
            let want_third = !matches!(orders, Orders::Fourth);
            let want_fourth = !matches!(orders, Orders::Third);
            if !want_third && C != 15 { return Err(no_value); }
            let mut sums: [[ExactSignedSum; 4]; S] =
                std::array::from_fn(|_| std::array::from_fn(|_| ExactSignedSum::default()));
            let [(u_start, u), (v_start, v)] = axes;
            let count = u.len().checked_mul(v.len()).ok_or(no_value)?;
            let variable = count > 2;
            const OPERATION: &str = "IR rational surface higher pole traversal";
            scratch.admission.work(0, OPERATION).map_err(EvaluationFailure::ResourceLimit)?;
            let mut constant = [None; 3];
            let window = TensorWindow::new(surface, [u_start, v_start]);
            for at in 0..count {
                if variable { scratch.admission.independent_cost(Some(1))?; }
                scratch.admission.work(u64::from(variable), OPERATION).map_err(EvaluationFailure::ResourceLimit)?;
                let (i, j) = (at / v.len(), at % v.len());
                let pole = window.pole([i, j]).ok_or(no_value)?;
                let point = pole.point;
                for (axis, coordinate) in point.coordinates().into_iter().enumerate() {
                    if at == 0 { constant[axis] = Some(coordinate); }
                    else if constant[axis] != Some(coordinate) { constant[axis] = None; }
                }
                let mut ordinal = 0;
                let max_degree = if C == 15 { 4 } else { 3 };
                for degree in 1..=max_degree {
                    for v_order in 0..=degree {
                        let lanes = &mut sums[ordinal];
                        window.add_partial(lanes, [u[i][degree - v_order], v[j][v_order]],
                            [i, j], pole, [degree > v_order, v_order > 0]).ok_or(no_value)?;
                        ordinal += 1;
                    }
                }
            }
            let mut values = [self.values; C];
            for (target, sum) in values[1..].iter_mut().zip(sums) {
                *target = sum.map(ExactSignedSum::finish);
            }
            let third = if want_third { third(&values, constant, widths) } else { Err(no_value) };
            let fourth = if want_fourth { fourth(&values, constant, widths) } else { Err(no_value) };
            Ok(match orders {
                Orders::Third => HigherPartials::Third(third),
                Orders::Fourth | Orders::ThirdAndFourth => HigherPartials::Fourth { third, fourth },
            })
        })())
    }
}
