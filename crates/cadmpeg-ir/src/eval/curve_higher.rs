// SPDX-License-Identifier: Apache-2.0
//! Requested third derivatives of analytic and placed model curve carriers.

use super::decode::Scratch;
use super::{admit_derivative, vector_sum, EvaluationFailure};
use crate::features::FiniteVector3;
use crate::geometry::{ProceduralCurveDefinition, SolvedCurveGeometry};
use crate::math::sum::ExactSignedSum;
use crate::scalar::FiniteReal;

/// The final placed tangent is already evaluated by this curve's owner.
/// Affine placement preserves C'''=-C' for circles/ellipses and C'''=C'
/// for hyperbolas. Follow each stored placement once to establish that law;
/// do not re-evaluate or re-place the tangent. Polynomial lines/parabolas
/// have identically zero third derivatives. A polyline needs its actual
/// tangent outcome to establish a differentiable segment.
pub(super) fn stored_third(
    scratch: &Scratch<'_, '_>,
    geometry: &SolvedCurveGeometry,
    tangent: Result<FiniteVector3, EvaluationFailure<()>>,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    scratch.unless_refused()?;
    let _depth = scratch.enter().ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
    let result = match geometry {
        SolvedCurveGeometry::Line(_) | SolvedCurveGeometry::Parabola(_) => Ok(FiniteVector3::ZERO),
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => tangent.map(FiniteVector3::negated),
        SolvedCurveGeometry::Hyperbola(_) => tangent,
        SolvedCurveGeometry::Polyline(_) => tangent.map(|_| FiniteVector3::ZERO),
        SolvedCurveGeometry::Transformed(placed) => {
            scratch.admission.independent_cost(Some(1))?;
            scratch.work(1, "IR curve higher source traversal")
                .ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
            stored_third(scratch, placed.basis(), tangent)
        }
        // The rational curve third needs its genuine local homogeneous state,
        // basis recurrences and scaled-span fallback. Existing lower orders
        // remain available; this owner does not assert a polynomial zero.
        SolvedCurveGeometry::Nurbs(_)
        | SolvedCurveGeometry::Degenerate(_)
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
