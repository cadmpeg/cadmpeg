// SPDX-License-Identifier: Apache-2.0
//! Rational pcurve orders from completed lower sums and real captured rows.

use super::{ExactSignedSum, Homogeneous};
use crate::eval::basis::polynomial_higher::CapturedBasis;
use crate::eval::pcurve_nurbs::{DifferentialPoles, HigherPcurve};
use crate::eval::{decode, EvaluationFailure};
use crate::math::sum::scaled_finite;
use crate::scalar::{FiniteReal, PositiveReal};
use crate::units::FinitePoint2;

/// Values retained where the old lower owner actually computed each sum.
/// An absent sum is not an identically zero homogeneous derivative.
#[derive(Clone, Copy)]
pub(in crate::eval) struct CompletedLower {
    pub(in crate::eval) orders: [Option<Homogeneous>; 3],
    pub(in crate::eval) scale: PositiveReal,
}

pub(in crate::eval) fn higher(
    scratch: &decode::Scratch<'_, '_>,
    knots: &[f64],
    degree: usize,
    span: usize,
    poles: DifferentialPoles<'_>,
    lower: CompletedLower,
    captured: &CapturedBasis<'_>,
    max_order: usize,
) -> HigherPcurve {
    let no_value = EvaluationFailure::NoValue;
    let evaluated = (|| {
        scratch.unless_refused()?;
        let scale = PositiveReal::new(knots[span + 1] - knots[span]).ok_or(no_value)?;
        let width = scaled_finite(scale.get()).ok_or(no_value)?;
        let rows = captured.rows(scratch, knots, span, scale);
        let rows = [rows.third, rows.fourth, rows.fifth];
        let mut available = [Err(no_value); 6];
        let mut values = [[None; 4]; 6];
        for (order, actual) in lower.orders.into_iter().enumerate() {
            let Some(actual) = actual else { continue; };
            let normalized = (|| {
                let mut lanes = actual.values;
                if lower.scale != scale {
                    if lower.scale != PositiveReal::ONE { return None; }
                    // Old unscaled Hn becomes h^n Hn. Each actual product
                    // remains extended; unsupported range is unavailable.
                    for _ in 0..order {
                        for lane in &mut lanes {
                            let mut sum = ExactSignedSum::default();
                            sum.add_scaled_product(*lane, scale.into())?;
                            *lane = sum.finish();
                        }
                    }
                }
                Some(lanes)
            })();
            if let Some(lanes) = normalized {
                values[order] = lanes;
                available[order] = Ok(());
            }
        }
        values[0][3].ok_or(no_value)?;
        for (at, row) in rows.iter().enumerate() {
            available[at + 3] = row.as_ref().map(|_| ()).map_err(|failure| *failure);
        }
        let mut sums: [[ExactSignedSum; 4]; 3] =
            std::array::from_fn(|_| std::array::from_fn(|_| ExactSignedSum::default()));
        let mut constant = [None; 2];
        for local in 0..=degree {
            if degree >= 4 {
                scratch.admission.independent_cost::<()>(Some(1))?;
                scratch.admission.work(1, "IR rational pcurve higher support")?;
            }
            let point = poles.point_at(span - degree + local).ok_or(no_value)?;
            let weight = FiniteReal::new(poles.weight_at(span - degree + local).unwrap_or(1.0))
                .ok_or(no_value)?;
            for (axis, coordinate) in [point.x, point.y].into_iter().enumerate() {
                let coordinate = FiniteReal::new(coordinate).ok_or(no_value)?;
                if local == 0 { constant[axis] = Some(coordinate); }
                else if constant[axis] != Some(coordinate) { constant[axis] = None; }
            }
            // Include every actual support pole in the theorem above, even
            // when its coefficient is zero at the selected parameter.
            for (at, row) in rows.iter().enumerate() {
                if available[at + 3].is_err() { continue; }
                let coefficient = row.as_ref().ok().and_then(|row| row.get(local).copied())
                    .and_then(FiniteReal::new);
                let Some(coefficient) = coefficient else {
                    available[at + 3] = Err(EvaluationFailure::NonFinite(()));
                    continue;
                };
                for (sum, coordinate) in sums[at].iter_mut().zip([point.x, point.y, point.z, 1.0]) {
                    sum.add_factors([coefficient.get(), weight.get(), coordinate]);
                }
            }
        }
        for (at, sums) in sums.into_iter().enumerate() {
            if available[at + 3].is_ok() { values[at + 3] = sums.map(ExactSignedSum::finish); }
        }
        Ok(std::array::from_fn(|at| {
            let order = at + 3;
            if order > max_order { return Err(no_value); }
            let coordinate = |axis: usize| {
                if constant[axis].is_some() { return Ok(FiniteReal::ZERO); }
                for state in &available[..=order] { (*state)?; }
                let result = match order {
                    3 => crate::math::sum::quotient_third::quotient_third(
                        std::array::from_fn(|n| values[n][axis]),
                        std::array::from_fn(|n| values[n][3]), width),
                    4 => crate::math::sum::quotient_fourth::quotient_fourth(
                        std::array::from_fn(|n| values[n][axis]),
                        std::array::from_fn(|n| values[n][3]), width),
                    _ => crate::math::sum::quotient_fifth::quotient_fifth(
                        std::array::from_fn(|n| values[n][axis]),
                        std::array::from_fn(|n| values[n][3]), width),
                };
                result.ok_or(no_value)?.map_err(|_| EvaluationFailure::NonFinite(()))
            };
            Ok(FinitePoint2::from_coordinates(coordinate(0)?, coordinate(1)?))
        }))
    })();
    match evaluated {
        Ok(values) => values,
        Err(failure) => std::array::from_fn(|at| if at + 3 <= max_order { Err(failure) }
            else { Err(no_value) }),
    }
}
