// SPDX-License-Identifier: Apache-2.0
//! Actual zero-radius rounded-chamfer partials from its two contact tracks.

use super::{HigherPartials, RequestedJet, SurfaceRequest};
use crate::eval::admission::EvaluationAdmission;
use crate::eval::{admit_derivative, admit_lanes, admit_point, cacheless_ruled_variable_blend_tracks,
    offset, point_displacement, vector_sum, ContactRequest, EvaluationFailure, SurfaceJet, UNREACHED_POINT};
use crate::features::FiniteVector3;
use crate::geometry::surface_payloads::VariableBlendSurfacePayload;
use crate::index::ModelIndex;
use crate::math::{Point3, Vector3};

pub(super) fn evaluate(
    admission: EvaluationAdmission<'_, '_>,
    index: &ModelIndex<'_>,
    payload: &VariableBlendSurfacePayload,
    u: f64,
    v: f64,
    request: SurfaceRequest,
) -> Result<RequestedJet, EvaluationFailure<Point3>> {
    let contact = if request == SurfaceRequest::First { ContactRequest::Tangent }
        else { ContactRequest::Higher(request) };
    let [first, second] = cacheless_ruled_variable_blend_tracks(admission, index, payload, u, v, contact)
        .map_err(|failure| failure.map(|()| UNREACHED_POINT))?;
    let chord = point_displacement(second.point(), first.point());
    let point = admit_point(offset(first.point(), &[(u, chord)]))?;
    let tangents = first.tangent().and_then(|first| Ok([first, second.tangent()?]));
    let first_order = tangents.map(|[first, second]| [chord, vector_sum(&[(1.0 - u, first), (u, second)])])
        .and_then(admit_lanes);
    let second_order = if request.needs_second() {
        tangents.and_then(|[a, b]| {
            let avv = first.higher[0]?;
            let bvv = second.higher[0]?;
            admit_lanes([Vector3::new(0.0, 0.0, 0.0), vector_sum(&[(-1.0, a), (1.0, b)]),
                vector_sum(&[(1.0 - u, avv.get()), (u, bvv.get())])])
        })
    } else { Err(EvaluationFailure::NoValue) };
    let higher = |order: usize| {
        let mixed = admit_derivative(vector_sum(&[(-1.0, first.higher[order - 3]?.get()),
            (1.0, second.higher[order - 3]?.get())]))?;
        let pure = admit_derivative(vector_sum(&[(1.0 - u, first.higher[order - 2]?.get()),
            (u, second.higher[order - 2]?.get())]))?;
        Ok((mixed, pure))
    };
    let third = if request.needs_third() {
        higher(3).map(|(mixed, pure)| [FiniteVector3::ZERO, FiniteVector3::ZERO, mixed, pure])
    } else { Err(EvaluationFailure::NoValue) };
    let higher = if request.needs_fourth() {
        let fourth = higher(4).map(|(mixed, pure)| [FiniteVector3::ZERO, FiniteVector3::ZERO,
            FiniteVector3::ZERO, mixed, pure]);
        if request == SurfaceRequest::Fifth {
            let fifth = higher(5).map(|(mixed, pure)| [FiniteVector3::ZERO, FiniteVector3::ZERO,
                FiniteVector3::ZERO, FiniteVector3::ZERO, mixed, pure]);
            HigherPartials::Fifth { third, fourth, fifth }
        } else { HigherPartials::Fourth { third, fourth } }
    } else { HigherPartials::Third(third) };
    Ok(RequestedJet { jet: SurfaceJet { point, first: first_order, second: second_order }, higher })
}

#[cfg(test)]
mod tests;
