// SPDX-License-Identifier: Apache-2.0
//! Higher pcurve orders over the actual selected span and borrowed poles.

use super::{basis, decode, DifferentialPoles, EvaluationFailure, FinitePoint2, HigherPcurve};
use crate::math::sum::{scaled_finite, ExactSignedSum, ScaledValue};
use crate::scalar::{FiniteReal, PositiveReal};

/// Degree-one rational orders from the existing point basis. The fixed two
/// poles, their weights and exact span differences form the quotient law;
/// no new span selection or point evaluation runs here.
pub(super) fn linear(
    scratch: &decode::Scratch<'_, '_>,
    knots: &[f64],
    span: usize,
    poles: DifferentialPoles<'_>,
    basis: &[f64],
    max_order: usize,
) -> HigherPcurve {
    let mut output = [Err(EvaluationFailure::NoValue); 3];
    let evaluated = (|| {
        scratch.unless_refused()?;
        let no_value = EvaluationFailure::NoValue;
        let first = poles.point_at(span - 1).ok_or(no_value)?;
        let last = poles.point_at(span).ok_or(no_value)?;
        let w0 = poles.weight_at(span - 1).unwrap_or(1.0);
        let w1 = poles.weight_at(span).unwrap_or(1.0);
        let mut width = ExactSignedSum::default();
        width.add_factors([knots[span + 1]]);
        width.add_factors([-knots[span]]);
        let width = width.finish().ok_or(no_value)?;
        let mut weight = ExactSignedSum::default();
        weight.add_product(*basis.first().ok_or(no_value)?, w0);
        weight.add_product(*basis.get(1).ok_or(no_value)?, w1);
        let weight = weight.finish().ok_or(no_value)?;
        let mut difference = ExactSignedSum::default();
        difference.add_factors([w1]);
        difference.add_factors([-w0]);
        let zero = FinitePoint2::from_coordinates(FiniteReal::ZERO, FiniteReal::ZERO);
        let Some(difference) = difference.finish() else {
            return Ok(std::array::from_fn(|at| if at + 3 <= max_order { Ok(zero) } else { Err(no_value) }));
        };
        let coordinate = |left, right| {
            let mut coefficient = ExactSignedSum::default();
            coefficient.add_factors([w0, w1, right]);
            coefficient.add_factors([-w0, w1, left]);
            let coefficient = coefficient.finish();
            std::array::from_fn(|at| {
                if at + 3 > max_order { return Err(no_value); }
                let Some(coefficient) = coefficient else { return Ok(FiniteReal::ZERO); };
                let value = match at {
                    0 => ScaledValue::product_quotient(
                        [scaled_finite(6.0).ok_or(no_value)?, coefficient, difference, difference],
                        [width, width, width, weight, weight, weight, weight]),
                    1 => ScaledValue::product_quotient(
                        [scaled_finite(-24.0).ok_or(no_value)?, coefficient, difference, difference, difference],
                        [width, width, width, width, weight, weight, weight, weight, weight]),
                    _ => crate::math::sum::quotient_fifth::product_fifth(
                        [scaled_finite(120.0), Some(coefficient), Some(difference), Some(difference), Some(difference), Some(difference)],
                        weight, width).ok_or(no_value)?,
                };
                value.map_err(|_| EvaluationFailure::NonFinite(()))
            })
        };
        let u: [Result<FiniteReal, EvaluationFailure<()>>; 3] = coordinate(first.x, last.x);
        let v: [Result<FiniteReal, EvaluationFailure<()>>; 3] = coordinate(first.y, last.y);
        Ok(std::array::from_fn(|at| u[at].and_then(|u| {
            Ok(FinitePoint2::from_coordinates(u, v[at]?))
        })))
    })();
    match evaluated {
        Ok(values) => values,
        Err(failure) => {
            for (at, value) in output.iter_mut().enumerate() {
                if at + 3 <= max_order { *value = Err(failure); }
            }
            output
        }
    }
}

pub(super) fn polynomial(
    scratch: &decode::Scratch<'_, '_>,
    knots: &[f64],
    degree: usize,
    span: usize,
    poles: DifferentialPoles<'_>,
    captured: &basis::polynomial_higher::CapturedBasis<'_>,
    max_order: usize,
) -> HigherPcurve {
    let zero = FinitePoint2::from_coordinates(FiniteReal::ZERO, FiniteReal::ZERO);
    let mut result = [Err(EvaluationFailure::NoValue); 3];
    for (at, value) in result.iter_mut().enumerate() {
        if at + 3 <= max_order && degree < at + 3 { *value = Ok(zero); }
    }
    if degree < 3 { return result; }
    let Some(scale) = PositiveReal::new(knots[span + 1] - knots[span]) else { return result; };
    let rows = captured.rows(scratch, knots, span, scale);
    let rows = [rows.third, rows.fourth, rows.fifth];
    let mut sums: [[ExactSignedSum; 2]; 3] = std::array::from_fn(|_| std::array::from_fn(|_| ExactSignedSum::default()));
    let mut active = rows.each_ref().map(|row| row.as_ref().map(|_| ()).map_err(|failure| *failure));
    for (at, value) in active.iter_mut().enumerate() {
        if degree < at + 3 { *value = Err(EvaluationFailure::NoValue); }
    }
    for local in 0..=degree {
        if active.iter().all(Result::is_err) { break; }
        if scratch.admission.independent_cost::<()>(Some(1)).is_err() {
            for value in &mut active { if value.is_ok() { *value = Err(EvaluationFailure::NoValue); } }
            break;
        }
        if scratch.work(1, "IR polynomial pcurve higher support").is_none() {
            for value in &mut active { if value.is_ok() { *value = Err(scratch.failure(EvaluationFailure::NoValue)); } }
            break;
        }
        let Some(point) = poles.point_at(span - degree + local) else {
            for value in &mut active { if value.is_ok() { *value = Err(EvaluationFailure::NoValue); } }
            break;
        };
        for (at, (row, value)) in rows.iter().zip(&mut active).enumerate() {
            if value.is_err() { continue; }
            let coefficient = row.as_ref().ok().and_then(|row| row.get(local).copied()).and_then(FiniteReal::new);
            let Some(coefficient) = coefficient else { *value = Err(EvaluationFailure::NonFinite(())); continue; };
            sums[at][0].add_product(coefficient.get(), point.x);
            sums[at][1].add_product(coefficient.get(), point.y);
        }
    }
    let scale = scaled_finite(scale.get());
    for (at, (active, sums)) in active.into_iter().zip(sums).enumerate() {
        if degree < at + 3 { continue; }
        result[at] = active.and_then(|()| {
            let scale = scale.ok_or(EvaluationFailure::NoValue)?;
            let lane = |sum: ExactSignedSum| sum.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                let result = match at {
                    0 => ScaledValue::product_quotient([value], [scale, scale, scale]),
                    1 => ScaledValue::product_quotient([value], [scale, scale, scale, scale]),
                    _ => ScaledValue::product_quotient([value], [scale, scale, scale, scale, scale]),
                };
                result.map_err(|_| EvaluationFailure::NonFinite(()))
            });
            let [u, v] = sums;
            Ok(FinitePoint2::from_coordinates(lane(u)?, lane(v)?))
        });
    }
    result
}
