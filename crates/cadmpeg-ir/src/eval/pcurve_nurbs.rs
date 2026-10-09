// SPDX-License-Identifier: Apache-2.0
//! Borrowed parameter-plane and polar NURBS differential evaluation.

use std::borrow::Cow;

use super::{basis, curve_nurbs, decode, difference_quotient, finite_or_refusal, homogeneous_curve_sum, planar_pole, planar_value, EvaluationFailure};
use super::rational::finite_lanes;
use crate::features::FinitePoint3;
use crate::geometry::pcurve::{PcurveNurbsPoles, PolarNurbsPoles};
use crate::math::Point2;
use crate::scalar::{FiniteReal, PositiveReal};
use crate::units::FinitePoint2;
use cadmpeg_core::decode::ResourceLimit;

/// The actual representations reached by the differential callers. A stored
/// paired row retains its source weight; raw lanes admit only reached poles.
#[derive(Clone, Copy)]
pub(super) enum DifferentialPoles<'a> {
    #[cfg(test)]
    Raw { points: &'a [Point2], weights: Option<&'a [f64]> },
    Stored(&'a PcurveNurbsPoles<FinitePoint2>),
    PolarRadial(&'a PolarNurbsPoles<FinitePoint2, FiniteReal>),
    PolarAxial(&'a PolarNurbsPoles<FinitePoint2, FiniteReal>),
}

impl DifferentialPoles<'_> {
    fn count(self) -> usize {
        match self {
            #[cfg(test)]
            Self::Raw { points, .. } => points.len(),
            Self::Stored(poles) => poles.count(),
            Self::PolarRadial(poles) | Self::PolarAxial(poles) => poles.count(),
        }
    }

    fn has_weights(self) -> bool {
        match self {
            #[cfg(test)]
            Self::Raw { weights, .. } => weights.is_some(),
            Self::Stored(poles) => matches!(poles, PcurveNurbsPoles::Rational { .. }),
            Self::PolarRadial(poles) | Self::PolarAxial(poles) => matches!(poles, PolarNurbsPoles::Rational { .. }),
        }
    }

    fn weight_at(self, index: usize) -> Option<f64> {
        match self {
            #[cfg(test)]
            Self::Raw { weights, .. } => weights.and_then(|weights| weights.get(index).copied()),
            Self::Stored(poles) => poles.weight_at(index),
            Self::PolarRadial(poles) | Self::PolarAxial(poles) => match poles {
                PolarNurbsPoles::Polynomial { .. } => None,
                PolarNurbsPoles::Rational { poles } => poles.get(index).map(|pole| pole.weight.get()),
            },
        }
    }

    fn point_at(self, index: usize) -> Option<FinitePoint3> {
        match self {
            #[cfg(test)]
            Self::Raw { points, .. } => FinitePoint2::new(*points.get(index)?).map(planar_pole),
            Self::Stored(poles) => poles.point_at(index).map(planar_pole),
            Self::PolarRadial(poles) => match poles {
                PolarNurbsPoles::Polynomial { poles } => poles.get(index).map(|pole| planar_pole(pole.radial)),
                PolarNurbsPoles::Rational { poles } => poles.get(index).map(|pole| planar_pole(pole.radial)),
            },
            Self::PolarAxial(poles) => {
                let axial = match poles {
                    PolarNurbsPoles::Polynomial { poles } => poles.get(index)?.axial,
                    PolarNurbsPoles::Rational { poles } => poles.get(index)?.axial,
                };
                Some(FinitePoint3::from_coordinates(axial, FiniteReal::ZERO, FiniteReal::ZERO))
            }
        }
    }
}

pub(super) struct PcurveDifferential {
    pub(super) point: FinitePoint2,
    /// The first derivative, or why it has no finite value.
    pub(super) tangent: Result<FinitePoint2, EvaluationFailure<Point2>>,
    pub(super) acceleration: Option<FinitePoint2>,
}

/// The point and first two derivatives at `t` of a possibly-rational
/// B-spline over the real borrowed pole representation. Planar values lie
/// in the model-space plane `z = 0`; a polar axial lane lies along x.
///
/// A point that overflows is non-finite and carries the value each
/// coordinate reached; a basis that leaves the finite range reaches no
/// coordinate, and each reads NaN. The rational quotient rule forms the
/// derivatives from the finite point, so a curve without one has no
/// derivatives here.
pub(super) fn differential(
    scratch: &decode::Scratch<'_, '_>,
    degree: u32,
    knots: &[f64],
    poles: DifferentialPoles<'_>,
    t: FiniteReal,
) -> Result<PcurveDifferential, EvaluationFailure<Point2>> {
    scratch.settle(differential_unsettled(
        scratch, degree, knots, poles, t,
    ))
}

fn differential_unsettled(
    scratch: &decode::Scratch<'_, '_>,
    degree: u32,
    knots: &[f64],
    poles: DifferentialPoles<'_>,
    t: FiniteReal,
) -> Result<PcurveDifferential, EvaluationFailure<Point2>> {
    let t = t.get();
    let unreached = EvaluationFailure::NonFinite(Point2::new(f64::NAN, f64::NAN));
    let degree = usize::try_from(degree).map_err(|_| EvaluationFailure::NoValue)?;
    let span = scratch
        .admit(basis::bspline_span(
            scratch.admission,
            knots,
            degree,
            poles.count(),
            t,
        ))
        .flatten()
        .ok_or(EvaluationFailure::NoValue)?;
    // At a finite parameter over finite knots, the basis is absent or not
    // finite only where one of its terms left the finite range.
    let basis = basis::bspline_basis(scratch, knots, degree, span, t).ok_or(unreached)?;
    if !basis::all_finite(scratch, &basis).ok_or_else(|| scratch.failure(unreached))? {
        return Err(unreached);
    }
    let sum = |values: &[f64]| {
        homogeneous_curve_sum(
            scratch,
            values,
            |index| poles.point_at(index),
            |index| poles.weight_at(index),
            span - degree,
        )
    };
    let base = sum(&basis).ok_or(EvaluationFailure::NoValue)?;
    let point = finite_lanes(base.project(base, &[]).ok_or(EvaluationFailure::NoValue)?)
        .map_err(|[u, v, _]| EvaluationFailure::NonFinite(Point2::new(u, v)))?;
    let uv = |p: [FiniteReal; 3]| FinitePoint2::from_coordinates(p[0], p[1]);
    let point_only = |tangent| PcurveDifferential {
        point: uv(point),
        tangent: Err(tangent),
        acceleration: None,
    };
    // The derivative basis is absent only where a term of the lower basis
    // left the finite range.
    let Some(mut first_basis) = basis::bspline_basis_derivative(scratch, knots, degree, span, t)
    else {
        return Ok(point_only(unreached));
    };
    let mut second_basis = basis::bspline_basis_second_derivative(scratch, knots, degree, span, t);
    let scale = if basis::all_finite(scratch, &first_basis)
        .ok_or_else(|| scratch.failure(unreached))?
        && match &second_basis {
            Some(values) => {
                basis::all_finite(scratch, values).ok_or_else(|| scratch.failure(unreached))?
            }
            None => true,
        } {
        PositiveReal::ONE
    } else {
        let width = knots[span + 1] - knots[span];
        let Some(scale) = PositiveReal::new(width) else {
            // A span of zero width has no derivative; the derivative over a
            // width outside the finite range is not reached.
            return Ok(point_only(if width.is_finite() {
                EvaluationFailure::NoValue
            } else {
                unreached
            }));
        };
        if degree == 1 {
            let local_points = [
                poles.point_at(span - 1).ok_or(EvaluationFailure::NoValue)?,
                poles.point_at(span).ok_or(EvaluationFailure::NoValue)?,
            ];
            let local_weights = poles.has_weights().then(|| {
                [
                    poles.weight_at(span - 1).unwrap_or(1.0),
                    poles.weight_at(span).unwrap_or(1.0),
                ]
            });
            let derivative = |second| {
                curve_nurbs::linear_derivative(
                    &basis,
                    curve_nurbs::DerivativePoles::Lanes {
                        points: &local_points,
                        weights: local_weights.as_ref().map(<[f64; 2]>::as_slice),
                    },
                    1,
                    scale.get(),
                    second,
                )
            };
            // The quotient form divides by the span itself, so each lane is
            // the derivative's coordinate or the signed infinity it reached.
            let lane = |lane: Result<FiniteReal, f64>| lane.map_err(EvaluationFailure::NonFinite);
            return Ok(PcurveDifferential {
                point: uv(point),
                tangent: derivative(false).map_or(Err(EvaluationFailure::NoValue), |[x, y, _]| {
                    planar_value(lane(x), lane(y))
                }),
                acceleration: derivative(true)
                    .and_then(|lanes| finite_lanes(lanes).ok())
                    .map(uv),
            });
        }
        let Some(scaled) =
            basis::bspline_basis_scaled_derivatives(scratch, knots, degree, span, t, scale)
        else {
            return Ok(point_only(unreached));
        };
        first_basis = scaled.first;
        second_basis = Some(Cow::Owned(scaled.second));
        scale
    };
    let first_sum = sum(&first_basis);
    let first_lanes = first_sum.and_then(|sum| sum.project(base, &[(sum, point)]));
    let first = first_lanes.and_then(|lanes| finite_lanes(lanes).ok());
    let second = first_sum.zip(first).and_then(|(first_sum, first)| {
        let second_sum = sum(second_basis.as_ref()?)?;
        finite_lanes(second_sum.project(
            base,
            &[(second_sum, point), (first_sum, first), (first_sum, first)],
        )?)
        .ok()
    });
    let unscale_twice = |value: FiniteReal| -> Result<Option<FiniteReal>, ResourceLimit> {
        if scale.get() == 1.0 {
            Ok(Some(value))
        } else {
            let Some(value) = finite_or_refusal(difference_quotient(
                value,
                FiniteReal::ZERO,
                scale.into(),
                FiniteReal::ZERO,
            ))?
            else {
                return Ok(None);
            };
            finite_or_refusal(difference_quotient(
                value,
                FiniteReal::ZERO,
                scale.into(),
                FiniteReal::ZERO,
            ))
        }
    };
    // A derivative lane over the span is the coordinate over the scale, or
    // the signed infinity its quotient reached; the positive scale leaves an
    // infinity unchanged.
    let tangent_lane = |lane: Result<FiniteReal, f64>| match lane {
        Ok(value) if scale.get() == 1.0 => Ok(value),
        Ok(value) => difference_quotient(value, FiniteReal::ZERO, scale.into(), FiniteReal::ZERO),
        Err(reached) => Err(EvaluationFailure::NonFinite(reached)),
    };
    let acceleration = match second {
        Some(value) => match (unscale_twice(value[0])?, unscale_twice(value[1])?) {
            (Some(u), Some(v)) => Some(FinitePoint2::from_coordinates(u, v)),
            _ => None,
        },
        None => None,
    };
    Ok(PcurveDifferential {
        point: uv(point),
        // A derivative sum or projection is absent only where the derivative
        // basis left the finite range.
        tangent: first_lanes.map_or(Err(unreached), |[x, y, _]| {
            planar_value(tangent_lane(x), tangent_lane(y))
        }),
        acceleration,
    })
}

#[cfg(test)]
mod tests;
