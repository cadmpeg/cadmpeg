// SPDX-License-Identifier: Apache-2.0
//! Requested higher derivatives of analytic and placed model curve carriers.

use super::decode::Scratch;
use super::{admit_derivative, vector_sum, EvaluationFailure};
use crate::features::FiniteVector3;
use crate::geometry::{ProceduralCurveDefinition, SolvedCurveGeometry};
use crate::math::sum::ExactSignedSum;
use crate::scalar::FiniteReal;

/// Independently completed third and requested fourth curve derivatives.
#[derive(Clone, Copy)]
pub(super) struct CurveHigher {
    pub(super) third: Result<FiniteVector3, EvaluationFailure<()>>,
    pub(super) fourth: Result<FiniteVector3, EvaluationFailure<()>>,
}

// Analytic laws read the completed final frame; NURBS starts in a local frame.
enum HigherFrame {
    Final(CurveHigher),
    Local(CurveHigher),
}

#[cfg(test)]
pub(super) fn stored_third(
    scratch: &Scratch<'_, '_>,
    geometry: &SolvedCurveGeometry,
    parameter: FiniteReal,
    tangent: Result<FiniteVector3, EvaluationFailure<()>>,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    stored_higher(scratch, geometry, parameter, tangent,
        Err(EvaluationFailure::NoValue), super::ModelCurveRequest::Third)?.third
}

/// Classify each actual source once. Final analytic derivatives have already
/// crossed every stored placement. Only local derivatives need placement here.
pub(super) fn stored_higher(
    scratch: &Scratch<'_, '_>,
    geometry: &SolvedCurveGeometry,
    parameter: FiniteReal,
    tangent: Result<FiniteVector3, EvaluationFailure<()>>,
    acceleration: Result<FiniteVector3, EvaluationFailure<()>>,
    request: super::ModelCurveRequest,
) -> Result<CurveHigher, EvaluationFailure<()>> {
    stored_higher_frame(scratch, geometry, parameter, tangent, acceleration, request)
        .map(|frame| match frame { HigherFrame::Final(value) | HigherFrame::Local(value) => value })
}

fn stored_higher_frame(
    scratch: &Scratch<'_, '_>,
    geometry: &SolvedCurveGeometry,
    parameter: FiniteReal,
    tangent: Result<FiniteVector3, EvaluationFailure<()>>,
    acceleration: Result<FiniteVector3, EvaluationFailure<()>>,
    request: super::ModelCurveRequest,
) -> Result<HigherFrame, EvaluationFailure<()>> {
    scratch.unless_refused()?;
    let _depth = scratch.enter().ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
    let fourth = request == super::ModelCurveRequest::Fourth;
    let result = match geometry {
        SolvedCurveGeometry::Line(_) | SolvedCurveGeometry::Parabola(_) => Ok(HigherFrame::Final(CurveHigher {
            third: Ok(FiniteVector3::ZERO),
            fourth: if fourth { Ok(FiniteVector3::ZERO) } else { Err(EvaluationFailure::NoValue) },
        })),
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => Ok(HigherFrame::Final(CurveHigher {
            third: tangent.map(FiniteVector3::negated),
            fourth: if fourth { acceleration.map(FiniteVector3::negated) } else { Err(EvaluationFailure::NoValue) },
        })),
        SolvedCurveGeometry::Hyperbola(_) => Ok(HigherFrame::Final(CurveHigher {
            third: tangent,
            fourth: if fourth { acceleration } else { Err(EvaluationFailure::NoValue) },
        })),
        SolvedCurveGeometry::Polyline(_) => Ok(HigherFrame::Final(CurveHigher {
            third: tangent.map(|_| FiniteVector3::ZERO),
            fourth: if fourth { tangent.map(|_| FiniteVector3::ZERO) } else { Err(EvaluationFailure::NoValue) },
        })),
        SolvedCurveGeometry::Transformed(placed) => {
            scratch.admission.independent_cost(Some(1))?;
            scratch.work(1, "IR curve higher source traversal")
                .ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
            stored_higher_frame(scratch, placed.basis(), parameter, tangent, acceleration, request).map(|frame| match frame {
                HigherFrame::Final(value) => HigherFrame::Final(value),
                HigherFrame::Local(value) => HigherFrame::Local(CurveHigher {
                    third: super::placed_derivative(*placed.transform(), value.third),
                    fourth: super::placed_derivative(*placed.transform(), value.fourth),
                }),
            })
        }
        SolvedCurveGeometry::Nurbs(curve) => {
            if fourth && curve.degree() == 1 && matches!(curve.pole_rows(),
                crate::geometry::nurbs::NurbsPoles3::Rational { .. })
            {
                return scratch.settle(super::curve_nurbs::linear_higher(scratch, curve, parameter, true)
                    .map(HigherFrame::Local));
            }
            let third = match curve.pole_rows() {
                crate::geometry::nurbs::NurbsPoles3::Polynomial { .. } => super::curve_nurbs::polynomial_third(scratch, curve, parameter),
                crate::geometry::nurbs::NurbsPoles3::Rational { .. } => super::curve_nurbs::rational_third(scratch, curve, parameter),
            };
            // The actual degree-one polynomial span has zero fourth.
            // Preserve its original selected-span work and width gate.
            let fourth = if fourth && curve.degree() == 1 && matches!(curve.pole_rows(),
                crate::geometry::nurbs::NurbsPoles3::Polynomial { .. })
            { third.map(|_| FiniteVector3::ZERO) } else { Err(EvaluationFailure::NoValue) };
            Ok(HigherFrame::Local(CurveHigher { third, fourth }))
        }
        SolvedCurveGeometry::Degenerate(_)
        | SolvedCurveGeometry::Composite { .. }
        | SolvedCurveGeometry::Unknown { .. } => Err(EvaluationFailure::NoValue),
    };
    scratch.settle(result)
}

/// C=a(t)r(t)+q(t)pitch, with a'=apex/TAU, r''=-r and r'''=-r'.
/// The original helix point owner has already admitted the native interval.
/// This fixed arithmetic runs only for a third-order request.
pub(super) fn helix_third(
    definition: &ProceduralCurveDefinition,
    parameter: f64,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    let ProceduralCurveDefinition::Helix(helix) = definition else {
        return Err(EvaluationFailure::NoValue);
    };
    let parameter = FiniteReal::new(parameter).ok_or(EvaluationFailure::NoValue)?;
    let start = helix.angle_range().finite_components()[0];
    let fraction = parameter.turns_from(start).get();
    let apex = helix.apex_factor().get();
    let scale = 1.0 + apex * fraction;
    let scale_first = apex * (1.0 / std::f64::consts::TAU);
    let major = helix.major().get();
    let minor = helix.minor().get();
    let (sine, cosine) = parameter.get().sin_cos();
    let radial = vector_sum(&[(cosine, major), (sine, minor)]);
    let radial_first = vector_sum(&[(-sine, major), (cosine, minor)]);
    admit_derivative(vector_sum(&[(-scale, radial_first), (-3.0 * scale_first, radial)]))
}

/// The fourth derivative of C=a(t)r(t)+q(t)pitch is a*r+4a'*r'.
/// The point owner has already checked the native domain.
pub(super) fn helix_fourth(
    definition: &ProceduralCurveDefinition,
    parameter: f64,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    let ProceduralCurveDefinition::Helix(helix) = definition else {
        return Err(EvaluationFailure::NoValue);
    };
    let parameter = FiniteReal::new(parameter).ok_or(EvaluationFailure::NoValue)?;
    let fraction = parameter.turns_from(helix.angle_range().finite_components()[0]).get();
    let scale = 1.0 + helix.apex_factor().get() * fraction;
    let scale_first = helix.apex_factor().get() * (1.0 / std::f64::consts::TAU);
    let (sine, cosine) = parameter.get().sin_cos();
    let radial = vector_sum(&[(cosine, helix.major().get()), (sine, helix.minor().get())]);
    let radial_first = vector_sum(&[(-sine, helix.major().get()), (cosine, helix.minor().get())]);
    admit_derivative(vector_sum(&[(scale, radial), (4.0 * scale_first, radial_first)]))
}

/// Keep four chain factors and a coordinate in the normalized exponent frame.
/// Zero is exact; only the final binary64 scaling can overflow.
pub(super) fn scale_fourth(
    vector: FiniteVector3,
    factors: [FiniteReal; 4],
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    let component = |coordinate: FiniteReal| {
        let [a, b, c, d] = factors;
        let nonzero = [coordinate, a, b, c, d].map(|value| crate::scalar::NonZeroReal::new(value.get()));
        let [Some(coordinate), Some(a), Some(b), Some(c), Some(d)] = nonzero else {
            return Ok(FiniteReal::ZERO);
        };
        crate::math::sum::ScaledValue::product_quotient(
            [coordinate, a, b, c, d].map(crate::math::sum::ScaledValue::of_nonzero), [],
        ).map_err(|_| EvaluationFailure::NonFinite(()))
    };
    let [x, y, z] = vector.components();
    Ok(FiniteVector3::from_components(component(x)?, component(y)?, component(z)?))
}

/// Multiply a vector by three finite chain factors without rounding or
/// overflowing their intermediate product. The exact sum owns four factors,
/// including the coordinate; zero remains zero for every finite factor.
pub(super) fn scale_third(
    vector: FiniteVector3,
    first: FiniteReal,
    second: FiniteReal,
    third: FiniteReal,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    let vector = vector.get();
    let component = |coordinate| {
        let mut sum = ExactSignedSum::default();
        sum.add_factors([coordinate, first.get(), second.get(), third.get()]);
        sum.finish().map_or(Ok(FiniteReal::ZERO), |value| {
            value.finite().map_err(|_| EvaluationFailure::NonFinite(()))
        })
    };
    Ok(FiniteVector3::from_components(component(vector.x)?, component(vector.y)?, component(vector.z)?))
}

#[cfg(test)]
mod tests;
