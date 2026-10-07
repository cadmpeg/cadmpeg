// SPDX-License-Identifier: Apache-2.0
//! B-spline span selection and basis recurrences.

use super::admission::EvaluationAdmission;
use super::{decode, difference_quotient, finite_or_refusal};
use crate::math::sum::scaled_ratio_products;
use crate::scalar::{FiniteReal, PositiveReal};
use cadmpeg_core::convert::f64_from_index;
use cadmpeg_core::decode::ResourceLimit;

/// Knot span index of `t` for a clamped B-spline basis, or `None` when the
/// knot vector cannot support `count` poles of the given degree.
pub(super) fn bspline_span<'ctx, 'arena: 'ctx>(
    admission: impl Into<EvaluationAdmission<'ctx, 'arena>>,
    knots: &[f64],
    degree: usize,
    count: usize,
    t: f64,
) -> Result<Option<usize>, ResourceLimit> {
    let admission = admission.into();
    admission.work(0, "IR B-spline span search")?;
    let Some(required) = count
        .checked_add(degree)
        .and_then(|size| size.checked_add(1))
    else {
        return Ok(None);
    };
    if count <= degree || knots.len() < required {
        return Ok(None);
    }
    if t >= knots[count] {
        return Ok(Some(count - 1));
    }
    if t <= knots[degree] {
        return Ok(Some(degree));
    }
    if count - degree == 1 {
        return Ok(Some(degree));
    }
    let mut lo = degree;
    let mut hi = count;
    while lo < hi {
        admission.work(1, "IR B-spline span search")?;
        let mid = usize::midpoint(lo, hi);
        if t < knots[mid] {
            hi = mid;
        } else if t >= knots[mid + 1] {
            lo = mid + 1;
        } else {
            return Ok(Some(mid));
        }
    }
    Ok(Some(lo))
}

/// Non-zero basis function values at `t` for the given span (Cox–de Boor).
/// Scratch contains `degree + 1` values, at most the admitted control count.
pub(super) fn bspline_basis(
    scratch: &decode::Scratch<'_, '_>,
    knots: &[f64],
    degree: usize,
    span: usize,
    t: f64,
) -> Option<decode::SupportValues<f64>> {
    let support = degree.checked_add(1)?;
    let mut values = if support <= 4 {
        decode::SupportValues::Inline {
            values: [0.0; 4],
            len: support,
        }
    } else {
        decode::SupportValues::Heap(scratch.filled(
            support,
            0.0,
            "IR B-spline basis",
            "IR B-spline basis work",
        )?)
    };
    scratch.admit(fill_bspline_basis(
        scratch.admission,
        knots,
        degree,
        span,
        t,
        &mut values,
    ))??;
    Some(values)
}

/// Writes the non-zero basis values at `t` for `span` into `values`, which
/// holds exactly `degree + 1` entries. `None` states a buffer of another
/// length, or a term that left the finite range.
pub(super) fn fill_bspline_basis<'ctx, 'arena: 'ctx>(
    admission: impl Into<EvaluationAdmission<'ctx, 'arena>>,
    knots: &[f64],
    degree: usize,
    span: usize,
    t: f64,
    values: &mut [f64],
) -> Result<Option<()>, ResourceLimit> {
    let admission = admission.into();
    admission.work(0, "IR B-spline basis work")?;
    if Some(values.len()) != degree.checked_add(1) {
        return Ok(None);
    }
    let finite_t = FiniteReal::new(t);
    if degree > 1 {
        admission.work(1, "IR B-spline basis work")?;
    }
    values[0] = 1.0;
    for j in 1..=degree {
        let mut saved = 0.0;
        for r in 0..j {
            if degree > 1 {
                admission.work(1, "IR B-spline basis work")?;
            }
            let value = values[r];
            // Each knot distance is admitted where it is formed.
            let right = FiniteReal::new(knots[span + r + 1] - t);
            let left = FiniteReal::new(t - knots[span + 1 - j + r]);
            // Two finite distances with a finite sum form the scaled ratio. A
            // distance or a sum outside the finite range takes the exact knot
            // differences instead.
            let ratio_terms = right.zip(left).and_then(|(right, left)| {
                Some((right, left, FiniteReal::new(right.get() + left.get())?))
            });
            let [right_term, left_term] = if let Some((right, left, denominator)) = ratio_terms {
                let Some(value) = FiniteReal::new(value) else {
                    return Ok(None);
                };
                let Some(terms) = scaled_ratio_products(value, denominator, [right, left]) else {
                    return Ok(None);
                };
                terms.map(FiniteReal::get)
            } else {
                let Some(t) = finite_t else {
                    return Ok(None);
                };
                let Some([right_knot, left_knot]) =
                    FiniteReal::array([knots[span + r + 1], knots[span + 1 - j + r]])
                else {
                    return Ok(None);
                };
                let Some(right_quotient) =
                    finite_or_refusal(difference_quotient(right_knot, t, right_knot, left_knot))?
                else {
                    return Ok(None);
                };
                let Some(left_quotient) =
                    finite_or_refusal(difference_quotient(t, left_knot, right_knot, left_knot))?
                else {
                    return Ok(None);
                };
                [value * right_quotient.get(), value * left_quotient.get()]
            };
            values[r] = saved + right_term;
            saved = left_term;
        }
        if degree > 1 {
            admission.work(1, "IR B-spline basis work")?;
        }
        values[j] = saved;
    }
    Ok(Some(()))
}

/// Inspect the finite range of a basis, admitting input-sized visits first.
pub(super) fn all_finite(scratch: &decode::Scratch<'_, '_>, values: &[f64]) -> Option<bool> {
    scratch.work(0, "IR B-spline finite basis inspection")?;
    for value in values {
        if values.len() > 2 {
            scratch.work(1, "IR B-spline finite basis inspection")?;
        }
        if !value.is_finite() {
            return Some(false);
        }
    }
    Some(true)
}

/// The quotient of three finite lanes over the exact knot difference, NaN where
/// a lane or the quotient left the finite range, and `None` with the refusal
/// recorded when the quotient was refused a resource.
fn nan_quotient(scratch: &decode::Scratch<'_, '_>, lanes: Option<[FiniteReal; 3]>) -> Option<f64> {
    let Some([value, end, start]) = lanes else {
        return Some(f64::NAN);
    };
    let quotient = scratch.admit(finite_or_refusal(difference_quotient(
        value,
        FiniteReal::ZERO,
        end,
        start,
    )))?;
    Some(quotient.map_or(f64::NAN, FiniteReal::get))
}

pub(super) fn bspline_basis_derivative(
    scratch: &decode::Scratch<'_, '_>,
    knots: &[f64],
    degree: usize,
    span: usize,
    t: f64,
) -> Option<decode::SupportValues<f64>> {
    if degree == 0 {
        return scratch.support_values(
            std::iter::once(Some(0.0)),
            "IR B-spline derivative basis",
            "IR B-spline derivative work",
        );
    }
    let degree_real = f64_from_index(degree)?;
    let lower = bspline_basis(scratch, knots, degree - 1, span, t)?;
    let lower_start = span - (degree - 1);
    scratch.support_values(
        (0..degree.checked_add(1)?).map(|local| {
            let index = span - degree + local;
            let lower_at = |global: usize| {
                global
                    .checked_sub(lower_start)
                    .and_then(|at| lower.get(at))
                    .copied()
                    .unwrap_or(0.0)
            };
            let left_denominator = knots[index + degree] - knots[index];
            let right_denominator = knots[index + degree + 1] - knots[index + 1];
            let left = if left_denominator == 0.0 {
                0.0
            } else if left_denominator.is_finite() {
                degree_real * lower_at(index) / left_denominator
            } else {
                nan_quotient(
                    scratch,
                    FiniteReal::array([
                        degree_real * lower_at(index),
                        knots[index + degree],
                        knots[index],
                    ]),
                )?
            };
            let right = if right_denominator == 0.0 {
                0.0
            } else if right_denominator.is_finite() {
                degree_real * lower_at(index + 1) / right_denominator
            } else {
                nan_quotient(
                    scratch,
                    FiniteReal::array([
                        degree_real * lower_at(index + 1),
                        knots[index + degree + 1],
                        knots[index + 1],
                    ]),
                )?
            };
            Some(left - right)
        }),
        "IR B-spline derivative basis",
        "IR B-spline derivative work",
    )
}

/// The basis has `degree + 1` values, at most the admitted control count.
pub(super) fn bspline_basis_second_derivative(
    scratch: &decode::Scratch<'_, '_>,
    knots: &[f64],
    degree: usize,
    span: usize,
    t: f64,
) -> Option<decode::SupportValues<f64>> {
    if degree <= 1 {
        return Some(decode::SupportValues::Inline {
            values: [0.0; 4],
            len: degree + 1,
        });
    }
    let degree_real = f64_from_index(degree)?;
    let lower = bspline_basis_derivative(scratch, knots, degree - 1, span, t)?;
    let lower_start = span - (degree - 1);
    scratch.support_values(
        (0..degree.checked_add(1)?).map(|local| {
            let index = span - degree + local;
            let lower_at = |global: usize| {
                global
                    .checked_sub(lower_start)
                    .and_then(|at| lower.get(at))
                    .copied()
                    .unwrap_or(0.0)
            };
            let left_denominator = knots[index + degree] - knots[index];
            let right_denominator = knots[index + degree + 1] - knots[index + 1];
            let left = if left_denominator == 0.0 {
                0.0
            } else if left_denominator.is_finite() {
                degree_real * lower_at(index) / left_denominator
            } else {
                nan_quotient(
                    scratch,
                    FiniteReal::array([
                        degree_real * lower_at(index),
                        knots[index + degree],
                        knots[index],
                    ]),
                )?
            };
            let right = if right_denominator == 0.0 {
                0.0
            } else if right_denominator.is_finite() {
                degree_real * lower_at(index + 1) / right_denominator
            } else {
                nan_quotient(
                    scratch,
                    FiniteReal::array([
                        degree_real * lower_at(index + 1),
                        knots[index + degree + 1],
                        knots[index + 1],
                    ]),
                )?
            };
            Some(left - right)
        }),
        "IR B-spline second derivative basis",
        "IR B-spline second derivative work",
    )
}

/// Basis derivatives with respect to a local coordinate whose unit is the
/// active knot span. This keeps the coefficients finite when derivatives in
/// the original parameter would exceed binary64 range.
#[derive(Debug, PartialEq)]
pub(super) struct ScaledBasisDerivatives {
    pub(super) first: Vec<f64>,
    pub(super) second: Vec<f64>,
}

pub(super) fn bspline_basis_scaled_derivatives(
    scratch: &decode::Scratch<'_, '_>,
    knots: &[f64],
    degree: usize,
    span: usize,
    t: f64,
    scale: PositiveReal,
) -> Option<ScaledBasisDerivatives> {
    if degree == 0 {
        return Some(ScaledBasisDerivatives {
            first: scratch.filled(
                1,
                0.0,
                "IR scaled B-spline first basis",
                "IR scaled B-spline derivative work",
            )?,
            second: scratch.filled(
                1,
                0.0,
                "IR scaled B-spline second basis",
                "IR scaled B-spline derivative work",
            )?,
        });
    }
    let lower = bspline_basis(scratch, knots, degree - 1, span, t)?;
    let first = bspline_basis_scaled_derivative_level(scratch, knots, degree, span, scale, &lower)?;
    let second = if degree == 1 {
        scratch.filled(
            2,
            0.0,
            "IR scaled B-spline second basis",
            "IR scaled B-spline derivative work",
        )?
    } else {
        let lower_lower = bspline_basis(scratch, knots, degree - 2, span, t)?;
        let lower_first = bspline_basis_scaled_derivative_level(
            scratch,
            knots,
            degree - 1,
            span,
            scale,
            &lower_lower,
        )?;
        bspline_basis_scaled_derivative_level(scratch, knots, degree, span, scale, &lower_first)?
    };
    Some(ScaledBasisDerivatives { first, second })
}

fn bspline_basis_scaled_derivative_level(
    scratch: &decode::Scratch<'_, '_>,
    knots: &[f64],
    degree: usize,
    span: usize,
    scale: PositiveReal,
    lower: &[f64],
) -> Option<Vec<f64>> {
    let degree_real = f64_from_index(degree)?;
    let lower_start = span - (degree - 1);
    scratch.collect(
        (0..degree.checked_add(1)?).map(|local| {
            let index = span - degree + local;
            let lower_at = |values: &[f64], global: usize| {
                global
                    .checked_sub(lower_start)
                    .and_then(|at| values.get(at))
                    .copied()
                    .unwrap_or(0.0)
            };
            let ratio = |hi: usize, lo: usize| {
                if knots[hi] == knots[lo] {
                    Some(0.0)
                } else {
                    let [hi_knot, lo_knot] = FiniteReal::array([knots[hi], knots[lo]])?;
                    scratch
                        .admit(finite_or_refusal(difference_quotient(
                            scale.into(),
                            FiniteReal::ZERO,
                            hi_knot,
                            lo_knot,
                        )))?
                        .map(FiniteReal::get)
                }
            };
            let left = ratio(index + degree, index)?;
            let right = ratio(index + degree + 1, index + 1)?;
            let derivative =
                degree_real * (left * lower_at(lower, index) - right * lower_at(lower, index + 1));
            derivative.is_finite().then_some(derivative)
        }),
        "IR scaled B-spline derivative basis",
        "IR scaled B-spline derivative work",
    )
}

#[cfg(test)]
mod tests;
