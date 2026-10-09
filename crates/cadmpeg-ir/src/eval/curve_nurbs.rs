// SPDX-License-Identifier: Apache-2.0
//! Borrowed NURBS curve derivative support and rational quotient arithmetic.

use std::borrow::Cow;

use super::{basis, decode, difference_quotient, homogeneous_curve_sum, CurveDerivative, EvaluationFailure};
use super::rational::finite_lanes;
use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::nurbs::NurbsPoles3;
use crate::math::sum::ExactSignedSum;
use crate::scalar::{FiniteReal, PositiveReal};

/// The two real pole representations used by derivative callers. Stored
/// curves borrow paired rows; raw Newton and pcurve callers borrow their
/// already admitted lanes. Neither representation copies the pole list.
#[derive(Clone, Copy)]
pub(super) enum DerivativePoles<'a> {
    Stored(&'a NurbsPoles3<FinitePoint3>),
    Lanes { points: &'a [FinitePoint3], weights: Option<&'a [f64]> },
}

impl DerivativePoles<'_> {
    fn count(self) -> usize {
        match self {
            Self::Stored(poles) => poles.count(),
            Self::Lanes { points, .. } => points.len(),
        }
    }

    fn point_at(self, index: usize) -> Option<FinitePoint3> {
        match self {
            Self::Stored(poles) => poles.point_at(index),
            Self::Lanes { points, .. } => points.get(index).copied(),
        }
    }

    fn weight_at(self, index: usize) -> Option<f64> {
        match self {
            Self::Stored(poles) => poles.weight_at(index),
            Self::Lanes { weights, .. } => weights.and_then(|weights| weights.get(index).copied()),
        }
    }
}

/// The derivative of order `order` at `t` of a possibly-rational B-spline
/// curve, or why it has none there.
///
/// At a finite parameter over finite knots, a basis that is absent or not
/// finite, and a derivative basis the span's scale cannot bring into range,
/// left the finite range, as did a point or derivative projection that
/// overflows. A knot vector or pole list that states no span at `t`, a zero
/// weight sum, and a span of zero width have no value.
pub(super) fn derivative(
    scratch: &decode::Scratch<'_, '_>,
    degree: u32,
    knots: &[f64],
    poles: DerivativePoles<'_>,
    t: FiniteReal,
    order: CurveDerivative,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    scratch.settle(derivative_unsettled(
        scratch,
        degree,
        knots,
        poles,
        t,
        order,
    ))
}

fn derivative_unsettled(
    scratch: &decode::Scratch<'_, '_>,
    degree: u32,
    knots: &[f64],
    poles: DerivativePoles<'_>,
    t: FiniteReal,
    order: CurveDerivative,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    let t = t.get();
    let no_value = EvaluationFailure::NoValue;
    let non_finite = EvaluationFailure::NonFinite(());
    let second = order == CurveDerivative::Second;
    let degree = usize::try_from(degree).map_err(|_| no_value)?;
    let span = scratch
        .admit(basis::bspline_span(
            scratch.admission,
            knots,
            degree,
            poles.count(),
            t,
        ))
        .flatten()
        .ok_or(no_value)?;
    let basis = basis::bspline_basis(scratch, knots, degree, span, t).ok_or(non_finite)?;
    if !basis::all_finite(scratch, &basis).ok_or_else(|| scratch.failure(non_finite))? {
        return Err(non_finite);
    }
    let mut first_basis =
        basis::bspline_basis_derivative(scratch, knots, degree, span, t).ok_or(non_finite)?;
    let mut second_basis = if second {
        basis::bspline_basis_second_derivative(scratch, knots, degree, span, t).ok_or(non_finite)?
    } else {
        Cow::Borrowed(&[][..])
    };
    let scale = if basis::all_finite(scratch, &first_basis)
        .ok_or_else(|| scratch.failure(non_finite))?
        && basis::all_finite(scratch, &second_basis).ok_or_else(|| scratch.failure(non_finite))?
    {
        PositiveReal::ONE
    } else {
        let width = knots[span + 1] - knots[span];
        let scale = PositiveReal::new(width).ok_or(if width.is_finite() {
            no_value
        } else {
            non_finite
        })?;
        if degree == 1 {
            let [x, y, z] = finite_lanes(
                linear_derivative(&basis, poles, span, scale.get(), second)
                    .ok_or(no_value)?,
            )
            .map_err(|_| non_finite)?;
            return Ok(FiniteVector3::from_components(x, y, z));
        }
        let scaled =
            basis::bspline_basis_scaled_derivatives(scratch, knots, degree, span, t, scale)
                .ok_or(non_finite)?;
        first_basis = scaled.first;
        if second {
            second_basis = Cow::Owned(scaled.second);
        }
        scale
    };
    // A sum is absent only where one of its derivative basis terms is not
    // finite.
    let sum = |values: &[f64]| {
        homogeneous_curve_sum(
            scratch,
            values,
            |index| poles.point_at(index),
            |index| poles.weight_at(index),
            span - degree,
        )
        .ok_or(non_finite)
    };
    let base = sum(&basis)?;
    let first_sum = sum(&first_basis)?;
    let point = finite_lanes(base.project(base, &[]).ok_or(no_value)?).map_err(|_| non_finite)?;
    let first = finite_lanes(
        first_sum
            .project(base, &[(first_sum, point)])
            .ok_or(no_value)?,
    )
    .map_err(|_| non_finite)?;
    let (lanes, divisions) = if second {
        let second_sum = sum(&second_basis)?;
        let lanes = finite_lanes(
            second_sum
                .project(
                    base,
                    &[(second_sum, point), (first_sum, first), (first_sum, first)],
                )
                .ok_or(no_value)?,
        )
        .map_err(|_| non_finite)?;
        (lanes, 2)
    } else {
        (first, 1)
    };
    // Each derivative lane over the span divides by the positive scale once
    // per order; a quotient that overflows leaves the derivative there.
    let unscale = |value: FiniteReal| {
        if scale.get() == 1.0 {
            return Ok(value);
        }
        (0..divisions).try_fold(value, |value, _| {
            difference_quotient(value, FiniteReal::ZERO, scale.into(), FiniteReal::ZERO)
                .map_err(|failure| failure.map(|_| ()))
        })
    };
    let [x, y, z] = lanes;
    Ok(FiniteVector3::from_components(
        unscale(x)?,
        unscale(y)?,
        unscale(z)?,
    ))
}

/// The degree-one rational derivative has a two-pole quotient form. Form its
/// numerator as exact products before dividing by the span and homogeneous
/// weight; neither derivative basis coefficient needs to fit in `f64`.
///
/// Each lane is the derivative's coordinate, or the signed infinity of a
/// coordinate whose quotient overflows.
pub(super) fn linear_derivative(
    basis: &[f64],
    poles: DerivativePoles<'_>,
    span: usize,
    width: f64,
    second: bool,
) -> Option<[Result<FiniteReal, f64>; 3]> {
    use crate::math::sum::{product_sum, scaled_finite, ProductSum};
    let start = span.checked_sub(1)?;
    let first = poles.point_at(start)?;
    let last = poles.point_at(span)?;
    let weight0 = poles.weight_at(start).unwrap_or(1.0);
    let weight1 = poles.weight_at(span).unwrap_or(1.0);
    let ProductSum::Value(base_weight) =
        product_sum([Some([basis[0], weight0]), Some([basis[1], weight1])].into_iter())
    else {
        return None;
    };
    let width = scaled_finite(width)?;
    let coordinate = |left: f64, right: f64| {
        let mut sum = ExactSignedSum::default();
        if second {
            // -w0*w1*(w1-w0)*(right-left); the factor 2 is an exponent shift.
            sum.add_factors([-weight0, weight1, weight1, right]);
            sum.add_factors([weight0, weight1, weight1, left]);
            sum.add_factors([weight0, weight1, weight0, right]);
            sum.add_factors([-weight0, weight1, weight0, left]);
            sum.finish().map_or(Ok(FiniteReal::ZERO), |numerator| {
                numerator.quotient_by_factors(
                    [base_weight, base_weight, base_weight, width, width],
                    true,
                )
            })
        } else {
            sum.add_factors([weight0, weight1, right]);
            sum.add_factors([-weight0, weight1, left]);
            sum.finish().map_or(Ok(FiniteReal::ZERO), |numerator| {
                numerator.quotient_by_factors([base_weight, base_weight, width], false)
            })
        }
    };
    Some([
        coordinate(first.x, last.x),
        coordinate(first.y, last.y),
        coordinate(first.z, last.z),
    ])
}

/// The degree-one rational law C'''=6 K D^2/(h^3 w^4), where
/// K=w0*w1*(P1-P0), D=w1-w0 and w is the local homogeneous weight.
/// Keep the actual knot width and factored values in their extended range;
/// only the final coordinate converts to binary64. This fixed analytic law
/// does not assert that a zero homogeneous third makes a rational third zero.
/// The same selected rational span supplies C4=-24 K D^3/(h^4 w^5).
/// Each order converts independently; a fourth range error retains the third.
/// The third-only reader performs no fourth quotient or second span search.
pub(super) fn linear_higher(
    scratch: &decode::Scratch<'_, '_>,
    curve: &crate::geometry::nurbs::NurbsCurve,
    parameter: FiniteReal,
    fourth: bool,
) -> Result<super::curve_higher::CurveHigher, EvaluationFailure<()>> {
    use crate::math::sum::{scaled_finite, ScaledValue};
    use super::curve_higher::CurveHigher;
    scratch.unless_refused()?;
    if curve.degree() != 1 {
        return Err(EvaluationFailure::NoValue);
    }
    let result = (|| {
        let parameter = super::map_nurbs_curve_parameter(curve, parameter)
            .ok_or(EvaluationFailure::NoValue)?;
        let poles = DerivativePoles::Stored(curve.pole_rows());
        let span = scratch.admit(basis::bspline_span(scratch.admission,
            curve.knots(), 1, poles.count(), parameter.get())).flatten()
            .ok_or(EvaluationFailure::NoValue)?;
        let values = basis::bspline_basis(scratch, curve.knots(), 1, span, parameter.get())
            .ok_or(EvaluationFailure::NonFinite(()))?;
        if !basis::all_finite(scratch, &values).ok_or(EvaluationFailure::NonFinite(()))? {
            return Err(EvaluationFailure::NonFinite(()));
        }
        let first = poles.point_at(span - 1).ok_or(EvaluationFailure::NoValue)?;
        let last = poles.point_at(span).ok_or(EvaluationFailure::NoValue)?;
        let weight0 = poles.weight_at(span - 1).unwrap_or(1.0);
        let weight1 = poles.weight_at(span).unwrap_or(1.0);
        let mut width = ExactSignedSum::default();
        width.add_factors([curve.knots()[span + 1]]);
        width.add_factors([-curve.knots()[span]]);
        let width = width.finish().ok_or(EvaluationFailure::NoValue)?;
        let mut weight = ExactSignedSum::default();
        weight.add_factors([values[0], weight0]);
        weight.add_factors([values[1], weight1]);
        let weight = weight.finish().ok_or(EvaluationFailure::NoValue)?;
        let mut difference = ExactSignedSum::default();
        difference.add_factors([weight1]);
        difference.add_factors([-weight0]);
        let Some(difference) = difference.finish() else {
            return Ok(CurveHigher {
                third: Ok(FiniteVector3::ZERO),
                fourth: if fourth { Ok(FiniteVector3::ZERO) } else { Err(EvaluationFailure::NoValue) },
            });
        };
        let six = scaled_finite(6.0).ok_or(EvaluationFailure::NoValue)?;
        let fourth_factor = if fourth {
            scaled_finite(-24.0).ok_or(EvaluationFailure::NoValue)?
        } else { six };
        let coordinate = |left, right| {
            let mut coefficient = ExactSignedSum::default();
            coefficient.add_factors([weight0, weight1, right]);
            coefficient.add_factors([-weight0, weight1, left]);
            let coefficient = coefficient.finish();
            let third = coefficient.map_or(Ok(FiniteReal::ZERO), |coefficient| {
                ScaledValue::product_quotient([six, coefficient, difference, difference],
                    [width, width, width, weight, weight, weight, weight])
            });
            let fourth = if fourth {
                coefficient.map_or(Ok(FiniteReal::ZERO), |coefficient| {
                    ScaledValue::product_quotient([fourth_factor, coefficient, difference, difference, difference],
                        [width, width, width, width, weight, weight, weight, weight, weight])
                })
            } else { Ok(FiniteReal::ZERO) };
            (third, fourth)
        };
        let lanes = [coordinate(first.x, last.x), coordinate(first.y, last.y), coordinate(first.z, last.z)];
        let vector = |lanes| finite_lanes(lanes).map(|[x, y, z]| FiniteVector3::from_components(x, y, z))
            .map_err(|_| EvaluationFailure::NonFinite(()));
        Ok(CurveHigher {
            third: vector(lanes.map(|lane| lane.0)),
            fourth: if fourth { vector(lanes.map(|lane| lane.1)) } else { Err(EvaluationFailure::NoValue) },
        })
    })();
    scratch.settle(result)
}

/// The true third of the selected polynomial span. The stored pole variant
/// supplies C=sum B_i P_i; the same conclusion does not hold for a rational
/// quotient. No first/second basis, full pole copy or per-coordinate replay
/// is constructed by this requested-order owner.
pub(super) fn polynomial_third(
    scratch: &decode::Scratch<'_, '_>,
    curve: &crate::geometry::nurbs::NurbsCurve,
    parameter: FiniteReal,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    use crate::math::sum::{scaled_finite, ScaledValue};
    scratch.unless_refused()?;
    let result = (|| {
        let NurbsPoles3::Polynomial { points } = curve.pole_rows() else {
            return Err(EvaluationFailure::NoValue);
        };
        let parameter = super::map_nurbs_curve_parameter(curve, parameter)
            .ok_or(EvaluationFailure::NoValue)?;
        let degree = usize::try_from(curve.degree()).map_err(|_| EvaluationFailure::NoValue)?;
        let span = basis::bspline_span_requested(scratch.admission, curve.knots(),
            degree, points.len(), parameter.get(), true)?.ok_or(EvaluationFailure::NoValue)?;
        let width = curve.knots()[span + 1] - curve.knots()[span];
        if width == 0.0 { return Err(EvaluationFailure::NoValue); }
        if degree < 3 { return Ok(FiniteVector3::ZERO); }
        let finite_width = PositiveReal::new(width);
        let values = if let Some(scale) = finite_width {
            Cow::Owned(basis::bspline_basis_scaled_third_derivative(scratch,
                curve.knots(), degree, span, parameter.get(), scale)
                .ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?)
        } else {
            // An overflowing positive span cannot form a PositiveReal scale.
            // The existing exact-difference recurrence owns that range.
            let base = degree - 3;
            let base_work = if base <= 1 { Some(0) } else {
                base.checked_add(2).and_then(|cost| cost.checked_add(
                    base.checked_mul(base.checked_add(1)?)?.checked_div(2)?))
                    .and_then(|cost| cost.checked_add(base))
            };
            let work = base_work.and_then(|cost| cost.checked_add(degree.checked_mul(3)?));
            scratch.admission.independent_cost(work)?;
            basis::bspline_basis_third_derivative(scratch, curve.knots(), degree, span, parameter.get())
                .ok_or_else(|| scratch.failure(EvaluationFailure::NonFinite(())))?
        };
        let mut sums = [ExactSignedSum::default(); 3];
        for (local, coefficient) in values.iter().enumerate() {
            scratch.admission.independent_cost(Some(1))?;
            scratch.work(1, "IR polynomial curve third support")
                .ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
            if !coefficient.is_finite() { return Err(EvaluationFailure::NonFinite(())); }
            let point = points.get(span - degree + local).ok_or(EvaluationFailure::NoValue)?;
            for (sum, coordinate) in sums.iter_mut().zip([point.x, point.y, point.z]) {
                sum.add_product(*coefficient, coordinate);
            }
        }
        let lane = |sum: ExactSignedSum| sum.finish().map_or(Ok(FiniteReal::ZERO), |value| {
            if let Some(scale) = finite_width {
                let scale = scaled_finite(scale.get()).ok_or(EvaluationFailure::NoValue)?;
                ScaledValue::product_quotient([value], [scale, scale, scale])
                    .map_err(|_| EvaluationFailure::NonFinite(()))
            } else { value.finite().map_err(|_| EvaluationFailure::NonFinite(())) }
        });
        let [x, y, z] = sums;
        Ok(FiniteVector3::from_components(lane(x)?, lane(y)?, lane(z)?))
    })();
    scratch.settle(result)
}

/// Requested rational higher orders. Fixed linear and clamped quadratic
/// owners keep extended coefficients; other shapes use one joint basis walk.
pub(super) fn rational_higher(
    scratch: &decode::Scratch<'_, '_>,
    curve: &crate::geometry::nurbs::NurbsCurve,
    parameter: FiniteReal,
    fourth: bool,
) -> Result<super::curve_higher::CurveHigher, EvaluationFailure<()>> {
    use super::rational::Homogeneous;
    use super::curve_higher::CurveHigher;
    scratch.unless_refused()?;
    if curve.degree() == 1 { return linear_higher(scratch, curve, parameter, fourth); }
    let vector = |lanes: Option<[Result<FiniteReal, f64>; 3]>| {
        finite_lanes(lanes.ok_or(EvaluationFailure::NoValue)?)
            .map(|[x, y, z]| FiniteVector3::from_components(x, y, z))
            .map_err(|_| EvaluationFailure::NonFinite(()))
    };
    let higher = |lanes: super::rational::HigherLanes| CurveHigher {
        third: vector(lanes.third), fourth: vector(lanes.fourth),
    };
    let result = (|| {
        let no_value = EvaluationFailure::NoValue;
        let NurbsPoles3::Rational { points } = curve.pole_rows() else { return Err(no_value); };
        let parameter = super::map_nurbs_curve_parameter(curve, parameter).ok_or(no_value)?;
        if curve.degree() == 2 {
            if let (Ok(poles), [a, a1, a2, b, b1, b2]) = (
                <&[_; 3]>::try_from(points.as_slice()), curve.knots().as_slice())
            {
                if a == a1 && a == a2 && b == b1 && b == b2 && a < b {
                    // Equal actual weights make this exact carrier polynomial.
                    if poles[0].weight == poles[1].weight && poles[0].weight == poles[2].weight {
                        return Ok(CurveHigher { third: Ok(FiniteVector3::ZERO),
                            fourth: if fourth { Ok(FiniteVector3::ZERO) } else { Err(no_value) } });
                    }
                    let [a, b] = FiniteReal::array([*a, *b]).ok_or(no_value)?;
                    let local = difference_quotient(parameter, a, b, a)
                        .map_err(|failure| failure.map(|_| ()))?;
                    let mut width = ExactSignedSum::default();
                    width.add_factors([b.get()]);
                    width.add_factors([-a.get()]);
                    let width = width.finish().ok_or(no_value)?;
                    return Ok(higher(Homogeneous::quadratic_higher(poles, local, width, fourth)
                        .ok_or(no_value)?));
                }
            }
        }
        let degree = usize::try_from(curve.degree()).map_err(|_| no_value)?;
        let span = basis::bspline_span_requested(scratch.admission, curve.knots(), degree,
            points.len(), parameter.get(), true)?.ok_or(no_value)?;
        let mut width = ExactSignedSum::default();
        width.add_factors([curve.knots()[span + 1]]);
        width.add_factors([-curve.knots()[span]]);
        let width = width.finish().ok_or(no_value)?;
        let lanes = if fourth {
            let rows = basis::requested_third::rows::<5>(scratch, curve.knots(), degree, span, parameter, width)
                .ok_or_else(|| scratch.failure(no_value))?;
            Homogeneous::curve_higher(scratch, curve.pole_rows(), span - degree,
                rows.as_slice(), width, rows.fourth_available())?
        } else {
            let rows = basis::requested_third::rows::<4>(scratch, curve.knots(), degree, span, parameter, width)
                .ok_or_else(|| scratch.failure(no_value))?;
            Homogeneous::curve_higher(scratch, curve.pole_rows(), span - degree,
                rows.as_slice(), width, false)?
        };
        Ok(higher(lanes.ok_or(no_value)?))
    })();
    scratch.settle(result)
}

#[cfg(test)]
mod tests;
