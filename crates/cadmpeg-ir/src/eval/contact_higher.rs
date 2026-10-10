// SPDX-License-Identifier: Apache-2.0
//! Contact derivatives from one actual pcurve and selected support jet.

use super::surface_request::{RequestedJet, SurfaceRequest};
use super::{EvaluationFailure, PcurveAcceleration, PcurveEvaluation};
use crate::features::FiniteVector3;
use crate::geometry::pcurve::PcurveGeometry;
use crate::math::sum::contact::{contact_derivative, directional_derivative};
use crate::scalar::FiniteReal;
use crate::units::FinitePoint2;

type Derivative = Result<FiniteVector3, EvaluationFailure<()>>;

pub(super) fn evaluate(
    geometry: &PcurveGeometry,
    pcurve: &PcurveEvaluation,
    higher: &super::pcurve_nurbs::HigherPcurve,
    support: &RequestedJet,
    request: SurfaceRequest,
) -> [Derivative; 4] {
    let mut result = [Err(EvaluationFailure::NoValue); 4];
    if request.needs_second() { result[0] = derivative::<2>(geometry, pcurve, higher, support); }
    if request.needs_third() { result[1] = derivative::<3>(geometry, pcurve, higher, support); }
    if request.needs_fourth() { result[2] = derivative::<4>(geometry, pcurve, higher, support); }
    if request == SurfaceRequest::Fifth { result[3] = derivative::<5>(geometry, pcurve, higher, support); }
    result
}

fn derivative<const N: usize>(
    geometry: &PcurveGeometry,
    pcurve: &PcurveEvaluation,
    higher: &super::pcurve_nurbs::HigherPcurve,
    support: &RequestedJet,
) -> Derivative {
    let lanes = if matches!(geometry, PcurveGeometry::Line(_)) {
        // The source is affine in its actual parameter. Every pcurve order
        // above First is identically zero, so only support order N remains.
        let direction = pcurve.tangent.map_err(|failure| failure.map(|_| ()))?;
        directional_derivative::<N>(support_row(support, N)?, direction)
    } else {
        let mut uv = [[FiniteReal::ZERO; 2]; 5];
        let mut partials = [[FiniteVector3::ZERO; 6]; 5];
        for at in 0..N {
            uv[at] = pcurve_order(geometry, pcurve, higher, at + 1)?.coordinates();
            partials[at] = support_row(support, at + 1)?;
        }
        contact_derivative::<N>(partials, uv)
    }.ok_or(EvaluationFailure::NoValue)?;
    let [x, y, z] = lanes.map(|lane| lane.map_err(|_| EvaluationFailure::NonFinite(())));
    Ok(FiniteVector3::from_components(x?, y?, z?))
}

fn support_row(support: &RequestedJet, order: usize) -> Result<[FiniteVector3; 6], EvaluationFailure<()>> {
    let mut row = [FiniteVector3::ZERO; 6];
    match order {
        1 => row[..2].copy_from_slice(&support.jet.first?),
        2 => row[..3].copy_from_slice(&support.jet.second?),
        3 => row[..4].copy_from_slice(&support.higher.third()?),
        4 => row[..5].copy_from_slice(&support.higher.fourth()?),
        5 => row.copy_from_slice(&support.higher.fifth()?),
        _ => return Err(EvaluationFailure::NoValue),
    }
    Ok(row)
}

fn pcurve_order(
    geometry: &PcurveGeometry,
    evaluated: &PcurveEvaluation,
    higher: &super::pcurve_nurbs::HigherPcurve,
    order: usize,
) -> Result<FinitePoint2, EvaluationFailure<()>> {
    let first = || evaluated.tangent.map_err(|failure| failure.map(|_| ()));
    let second = || match evaluated.acceleration {
        PcurveAcceleration::Finite(value) => Ok(value),
        PcurveAcceleration::NonFinite => Err(EvaluationFailure::NonFinite(())),
        PcurveAcceleration::Unstated => Err(EvaluationFailure::NoValue),
    };
    let negated = |point: FinitePoint2| {
        let [u, v] = point.coordinates();
        FinitePoint2::from_coordinates(u.negated(), v.negated())
    };
    match order {
        1 => first(),
        2 => second(),
        3..=5 => match geometry {
            PcurveGeometry::Nurbs { .. } | PcurveGeometry::PolarHarmonic(_)
                | PcurveGeometry::PolarNurbs { .. } | PcurveGeometry::SphericalGreatCircle(_) => higher[order - 3],
            PcurveGeometry::Line(_) | PcurveGeometry::Parabola(_) => {
                Ok(FinitePoint2::from_coordinates(FiniteReal::ZERO, FiniteReal::ZERO))
            }
            PcurveGeometry::Circle(_) | PcurveGeometry::Ellipse(_) | PcurveGeometry::Harmonic(_) => {
                match order {
                    3 => first().map(negated),
                    4 => second().map(negated),
                    _ => first(),
                }
            }
            PcurveGeometry::Hyperbola(_) | PcurveGeometry::Hyperbolic(_) => {
                if order == 4 { second() } else { first() }
            }
            // These carriers retain their actual completed lower orders.
            // Their owners do not yet supply this needed higher interface.
            _ => Err(EvaluationFailure::NoValue),
        },
        _ => Err(EvaluationFailure::NoValue),
    }
}
