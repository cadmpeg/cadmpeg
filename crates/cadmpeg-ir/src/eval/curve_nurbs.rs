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

#[cfg(test)]
mod tests;
