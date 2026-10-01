// SPDX-License-Identifier: Apache-2.0
//! Sampled polyline point and tangent evaluation.

use super::rational::finite_lanes;
use super::{difference_quotient, EvaluationFailure};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::sampled::PolylineCurve;
use crate::math::Point3;
use crate::scalar::{FiniteReal, SegmentPosition};

/// The polyline's samples with the parameterization it evaluates on.
///
/// A sample row carries its own parameter, so the two lists this returns agree
/// by construction. An unparameterized polyline evaluates on its sample index.
pub(super) fn polyline_samples<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    polyline: &PolylineCurve,
) -> Result<Option<(Vec<FinitePoint3>, Vec<FiniteReal>, cadmpeg_core::decode::ScopedReservation<'ctx>)>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "IR polyline inversion samples")?;
    let points = storage.with_storage(|| ctx.collect_vec(polyline.points(), "IR polyline inversion points"))?;
    let mut parameters = Vec::new();
    ctx.reserve_scoped_vec(&mut storage, &mut parameters, points.len(), "IR polyline inversion parameters")?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(points.len()), "IR polyline inversion parameter scan")?;
    match polyline.parameters() {
        Some(values) => parameters.extend(values),
        None => for index in 0..points.len() {
            let Some(parameter) = FiniteReal::from_index(index) else { return Ok(None); };
            parameters.push(parameter);
        },
    }
    Ok(Some((points, parameters, storage)))
}

/// The point of a sampled polyline at `t`, interpolated on the first
/// segment whose parameters enclose it, at its fraction of that segment. A
/// parameter outside every segment, and a segment of zero parameter width,
/// have no value; a coordinate whose interpolation overflows carries its
/// plain sum.
pub(super) fn polyline_point(
    count: usize,
    point: impl Fn(usize) -> Option<FinitePoint3>,
    parameter: impl Fn(usize) -> Option<FiniteReal>,
    t: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    if count < 2 {
        return Err(EvaluationFailure::NoValue);
    }
    let t = FiniteReal::new(t).ok_or(EvaluationFailure::NoValue)?;
    let mut selected = None;
    for segment in 0..count - 1 {
        let start = parameter(segment).ok_or(EvaluationFailure::NoValue)?;
        let end = parameter(segment + 1).ok_or(EvaluationFailure::NoValue)?;
        match t.segment_position(start, end) {
            SegmentPosition::Outside => {}
            SegmentPosition::Degenerate => return Err(EvaluationFailure::NoValue),
            SegmentPosition::Within(fraction) => {
                selected = Some((segment, fraction.get()));
                break;
            }
        }
    }
    let (segment, fraction) = selected.ok_or(EvaluationFailure::NoValue)?;
    let start = point(segment).ok_or(EvaluationFailure::NoValue)?.get();
    let end = point(segment + 1).ok_or(EvaluationFailure::NoValue)?.get();
    let lerp = |start, end| crate::math::sum::finite_dot([1.0 - fraction, fraction], [start, end]);
    let [x, y, z] = finite_lanes([
        lerp(start.x, end.x),
        lerp(start.y, end.y),
        lerp(start.z, end.z),
    ])
    .map_err(|[x, y, z]| EvaluationFailure::NonFinite(Point3::new(x, y, z)))?;
    Ok(FinitePoint3::from_coordinates(x, y, z))
}

/// The tangent of a sampled polyline at `t`: the chord slope of every
/// segment whose parameters enclose it, which must agree. A parameter outside
/// every segment, a segment of zero parameter width, and a vertex whose
/// segments disagree have no tangent; a slope that overflows leaves the
/// tangent outside the finite range.
pub(super) fn polyline_tangent(
    points: &[FinitePoint3],
    parameters: &[FiniteReal],
    t: f64,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    if points.len() < 2 || points.len() != parameters.len() {
        return Err(EvaluationFailure::NoValue);
    }
    let t = FiniteReal::new(t).ok_or(EvaluationFailure::NoValue)?;
    let mut tangent = None;
    for (segment, window) in parameters.windows(2).enumerate() {
        if !((t >= window[0] && t <= window[1]) || (t <= window[0] && t >= window[1])) {
            continue;
        }
        let [start_x, start_y, start_z] = points[segment].coordinates();
        let [end_x, end_y, end_z] = points[segment + 1].coordinates();
        let slope = |end, start| {
            difference_quotient(end, start, window[1], window[0])
                .map_err(|failure| failure.map(|_| ()))
        };
        let candidate = FiniteVector3::from_components(
            slope(end_x, start_x)?,
            slope(end_y, start_y)?,
            slope(end_z, start_z)?,
        );
        if tangent.is_some_and(|tangent| tangent != candidate) {
            return Err(EvaluationFailure::NoValue);
        }
        tangent = Some(candidate);
    }
    tangent.ok_or(EvaluationFailure::NoValue)
}
