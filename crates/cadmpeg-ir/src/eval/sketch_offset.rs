// SPDX-License-Identifier: Apache-2.0
//! Endpoint frames and fitted offset distance for sketch NURBS curves.

use super::nurbs_pcurve_parameter_domain;
use crate::features::FiniteVector3;
use crate::geometry::pcurve::PcurveNurbs;
use crate::math::{Point2, Vector3};
use crate::scalar::FiniteReal;

const EPS_EVAL_CLAMPED_NURBS_PCURVE_ENDPOINT_FRAMES_E12: f64 = 1.0e-12;
const EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E12: f64 = 1.0e-12;
const EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9: f64 = 1.0e-9;

pub(super) fn clamped_nurbs_pcurve_endpoint_frames(
    curve: &PcurveNurbs,
) -> Option<[(Point2, Point2); 2]> {
    let knots = curve.knots();
    let control_points = curve.pole_rows().raw_points();
    let [lower, upper] =
        nurbs_pcurve_parameter_domain(curve.degree(), knots, control_points.len())?.endpoints();
    let degree = curve.degree() as usize;
    if knots.iter().take(degree + 1).any(|knot| *knot != lower)
        || knots
            .iter()
            .skip(control_points.len())
            .take(degree + 1)
            .any(|knot| *knot != upper)
        || curve
            .weights()
            .is_some_and(|weights| weights.iter().any(|weight| weight.get() <= 0.0))
    {
        return None;
    }
    let start = control_points[0];
    let end = *control_points.last()?;
    let start_tangent = control_points
        .iter()
        .skip(1)
        .map(|point| Point2::new(point.u - start.u, point.v - start.v))
        .find(|tangent| {
            tangent.u.hypot(tangent.v) > EPS_EVAL_CLAMPED_NURBS_PCURVE_ENDPOINT_FRAMES_E12
        })?;
    let end_tangent = control_points
        .iter()
        .rev()
        .skip(1)
        .map(|point| Point2::new(end.u - point.u, end.v - point.v))
        .find(|tangent| {
            tangent.u.hypot(tangent.v) > EPS_EVAL_CLAMPED_NURBS_PCURVE_ENDPOINT_FRAMES_E12
        })?;
    Some([(start, start_tangent), (end, end_tangent)])
}

pub(super) fn fitted_nurbs_offset_candidate(
    source: [(Point2, Point2); 2],
    result: [(Point2, Point2); 2],
    linear_tolerance: f64,
) -> Option<FiniteReal> {
    let mut distances = [0.0; 2];
    for ordinal in 0..2 {
        let (source_point, source_tangent) = source[ordinal];
        let (result_point, result_tangent) = result[ordinal];
        let source_length = source_tangent.u.hypot(source_tangent.v);
        let result_length = result_tangent.u.hypot(result_tangent.v);
        if source_length <= EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E12
            || result_length <= EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E12
        {
            return None;
        }
        let source_unit =
            FiniteVector3::new(Vector3::new(source_tangent.u, source_tangent.v, 0.0))?
                .unit_nonzero()?;
        let result_unit =
            FiniteVector3::new(Vector3::new(result_tangent.u, result_tangent.v, 0.0))?
                .unit_nonzero()?;
        if source_unit.cross(result_unit).norm() > EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9 {
            return None;
        }
        let offset_projection = |x, y| {
            crate::math::sum::finite_dot(
                [
                    result_point.u,
                    -source_point.u,
                    result_point.v,
                    -source_point.v,
                ],
                [x, x, y, y],
            )
            .ok()
        };
        let tangential = offset_projection(source_unit.x, source_unit.y)?.get();
        let coordinate_scale = 1.0
            + source_point
                .u
                .abs()
                .max(source_point.v.abs())
                .max(result_point.u.abs())
                .max(result_point.v.abs());
        if tangential.abs()
            > linear_tolerance.max(EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9 * coordinate_scale)
        {
            return None;
        }
        distances[ordinal] = offset_projection(-source_unit.y, source_unit.x)?.get();
    }
    let scale = 1.0 + distances[0].abs().max(distances[1].abs());
    // Both thresholds are comparison thresholds of this predicate, not the
    // document tolerance: the constant is the double-precision noise floor of
    // the distance arithmetic above, it is never read from the source, and no
    // stated tolerance is stored or reported at the floored magnitude.
    ((distances[0] - distances[1]).abs()
        <= linear_tolerance.max(EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9 * scale)
        && distances[0].abs() > linear_tolerance.max(EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9))
    .then(|| crate::math::interpolate(distances[0], distances[1], 0.5))
    .flatten()
}
