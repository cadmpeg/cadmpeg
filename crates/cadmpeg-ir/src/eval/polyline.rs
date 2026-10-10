// SPDX-License-Identifier: Apache-2.0
//! Sampled polyline point and tangent evaluation.

use super::admission::EvaluationAdmission;
use super::rational::finite_lanes;
use super::{difference_quotient, EvaluationFailure};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::math::{Point3, Vector3};
use crate::scalar::{ExtendedReal, FiniteReal, SegmentPosition};
use crate::topology::ParameterInterval;

/// Select the nearest-seed parameter of an admitted polyline without copying its rows.
pub(super) fn parameter_near_point(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    polyline: &crate::geometry::sampled::PolylineCurve,
    point: Point3,
    tolerance: f64,
    seed: FiniteReal,
) -> Result<Option<FiniteReal>, cadmpeg_core::CodecError> {
    if polyline.point_count() < 2 {
        return Ok(None);
    }
    let mut best_candidate: Option<FiniteReal> = None;
    for segment in ctx.admit_iter(
        0..polyline.point_count() - 1,
        "IR polyline inversion segment scan",
    )? {
        let Some((parameter_start, parameter_end)) = polyline
            .parameter_at(segment)
            .zip(polyline.parameter_at(segment + 1))
        else {
            return Ok(None);
        };
        let [parameter_start, parameter_end] = [parameter_start.get(), parameter_end.get()];
        let parameter_width = parameter_end - parameter_start;
        if parameter_width == 0.0 {
            continue;
        }
        let Some((start, end)) = polyline
            .point_at(segment)
            .zip(polyline.point_at(segment + 1))
        else {
            return Ok(None);
        };
        let (start, end) = (start.get(), end.get());
        let direction = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
        let offset = Vector3::new(point.x - start.x, point.y - start.y, point.z - start.z);
        let length = direction.x.hypot(direction.y).hypot(direction.z);
        if !length.is_finite() {
            continue;
        }
        // A width or length that overflows leaves a NaN fraction, which has no
        // candidate.
        let fraction = if length == 0.0 {
            if offset.x.hypot(offset.y).hypot(offset.z) > tolerance {
                continue;
            }
            ExtendedReal::new((seed.get() - parameter_start) / parameter_width)
        } else {
            let unit = Vector3::new(
                direction.x / length,
                direction.y / length,
                direction.z / length,
            );
            ExtendedReal::new(offset.dot(unit) / length)
        };
        let Some(fraction) = fraction else {
            continue;
        };
        let fraction = ParameterInterval::UNIT.project(fraction).get();
        let candidate = parameter_start + fraction * parameter_width;
        let mapped = Point3::new(
            start.x + fraction * direction.x,
            start.y + fraction * direction.y,
            start.z + fraction * direction.z,
        );
        let error = (mapped.x - point.x)
            .hypot(mapped.y - point.y)
            .hypot(mapped.z - point.z);
        if let Some(candidate) = FiniteReal::new(candidate) {
            if error.is_finite()
                && error <= tolerance
                && best_candidate.is_none_or(|best| {
                    (candidate.get() - seed.get())
                        .abs()
                        .total_cmp(&(best.get() - seed.get()).abs())
                        == std::cmp::Ordering::Less
                })
            {
                best_candidate = Some(candidate);
            }
        }
    }
    Ok(best_candidate)
}

/// The point of a sampled polyline at `t`, interpolated on the first
/// segment whose parameters enclose it, at its fraction of that segment. A
/// parameter outside every segment, and a segment of zero parameter width,
/// have no value; a coordinate whose interpolation overflows carries its
/// plain sum.
pub(super) fn polyline_point(
    admission: EvaluationAdmission<'_, '_>,
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
        admission
            .work(1, "IR polyline point segment scan")
            .map_err(EvaluationFailure::ResourceLimit)?;
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
    admission: EvaluationAdmission<'_, '_>,
    count: usize,
    point: impl Fn(usize) -> Option<FinitePoint3>,
    parameter: impl Fn(usize) -> Option<FiniteReal>,
    t: f64,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    if count < 2 {
        return Err(EvaluationFailure::NoValue);
    }
    let t = FiniteReal::new(t).ok_or(EvaluationFailure::NoValue)?;
    let mut tangent = None;
    for segment in 0..count - 1 {
        admission
            .work(1, "IR polyline tangent segment scan")
            .map_err(EvaluationFailure::ResourceLimit)?;
        let start = parameter(segment).ok_or(EvaluationFailure::NoValue)?;
        let end = parameter(segment + 1).ok_or(EvaluationFailure::NoValue)?;
        if !((t >= start && t <= end) || (t <= start && t >= end)) {
            continue;
        }
        let [start_x, start_y, start_z] = point(segment)
            .ok_or(EvaluationFailure::NoValue)?
            .coordinates();
        let [end_x, end_y, end_z] = point(segment + 1)
            .ok_or(EvaluationFailure::NoValue)?
            .coordinates();
        let slope = |value_end, value_start| {
            difference_quotient(value_end, value_start, end, start)
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

#[cfg(test)]
mod tests {
    use super::{polyline_point, polyline_tangent, EvaluationAdmission, EvaluationFailure};
    use crate::features::FinitePoint3;
    use crate::math::Point3;
    use crate::scalar::FiniteReal;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    fn samples() -> ([FinitePoint3; 4], [FiniteReal; 4]) {
        let points =
            [0.0, 1.0, 2.0, 3.0].map(|x| FinitePoint3::new(Point3::new(x, 0.0, 0.0)).unwrap());
        (points, FiniteReal::array([0.0, 1.0, 2.0, 3.0]).unwrap())
    }

    #[test]
    fn polyline_segment_scans_preserve_zero_work_refusal() {
        let (points, parameters) = samples();
        for derivative in [false, true] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let admission = EvaluationAdmission::Decode(&ctx);
            let result = if derivative {
                polyline_tangent(
                    admission,
                    points.len(),
                    |index| points.get(index).copied(),
                    |index| parameters.get(index).copied(),
                    0.5,
                )
                .map(|_| ())
                .map_err(|error| error.map(|()| ()))
            } else {
                polyline_point(
                    admission,
                    points.len(),
                    |index| points.get(index).copied(),
                    |index| parameters.get(index).copied(),
                    0.5,
                )
                .map(|_| ())
                .map_err(|error| error.map(|_| ()))
            };
            let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
                panic!("segment work is admitted before access");
            };
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
            assert_eq!(ctx.resource_refusal(), Some(first));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
            );
        }
    }

    #[test]
    fn polyline_point_charges_only_segments_before_its_first_match() {
        let (points, parameters) = samples();
        for (parameter, work) in [(0.5, 1), (2.5, 3)] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = work;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let point = polyline_point(
                EvaluationAdmission::Decode(&ctx),
                points.len(),
                |index| points.get(index).copied(),
                |index| parameters.get(index).copied(),
                parameter,
            )
            .unwrap();
            assert_eq!(point.get(), Point3::new(parameter, 0.0, 0.0));
            let next = ctx.charge_work_limit(1, "test next segment").unwrap_err();
            assert_eq!((next.limit, next.used, next.additional), (work, work, 1));
        }
    }

    #[test]
    fn polyline_tangent_stops_at_the_first_conflicting_segment() {
        use cadmpeg_core::decode::WorkBudget;
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 1.0, 0.0),
            Point3::new(3.0, 1.0, 0.0),
        ]
        .map(|point| FinitePoint3::new(point).unwrap());
        let parameters = FiniteReal::array([0.0, 1.0, 2.0, 3.0]).unwrap();
        for sliced in [false, true] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 2;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let budget = WorkBudget::new(2);
            let admission = EvaluationAdmission::Decode(&ctx);
            let result = if sliced {
                admission.within_work_slice(&budget, |admission| {
                    polyline_tangent(
                        admission,
                        points.len(),
                        |index| points.get(index).copied(),
                        |index| parameters.get(index).copied(),
                        1.0,
                    )
                })
            } else {
                polyline_tangent(
                    admission,
                    points.len(),
                    |index| points.get(index).copied(),
                    |index| parameters.get(index).copied(),
                    1.0,
                )
            };
            assert_eq!(result, Err(EvaluationFailure::NoValue));
            if sliced {
                assert_eq!(budget.consumed(), 2);
            }
            ctx.finish_session().unwrap();
        }
    }

    #[test]
    fn polyline_tangent_admits_every_segment_before_agreement_checks() {
        let (points, parameters) = samples();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = polyline_tangent(
            EvaluationAdmission::Decode(&ctx),
            points.len(),
            |index| points.get(index).copied(),
            |index| parameters.get(index).copied(),
            0.5,
        );
        let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
            panic!("a tangent examines later segments");
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
        );
    }

    #[test]
    fn polyline_inverse_borrows_sample_rows_without_storage() {
        use crate::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
        use crate::scalar::FiniteReal;
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        for parameterized in [false, true] {
            let points = [0.0, 1.0, 2.0, 3.0].map(|x| Point3::new(x, 0.0, 0.0));
            let samples = if parameterized {
                PolylineSamples::Parameterized {
                    vertices: points
                        .into_iter()
                        .zip([6.0, 4.0, 2.0, 0.0])
                        .map(|(point, parameter)| PolylineVertex { parameter, point })
                        .collect::<Vec<_>>()
                        .try_into()
                        .unwrap(),
                }
            } else {
                PolylineSamples::Unparameterized {
                    points: points.to_vec().try_into().unwrap(),
                }
            };
            let curve = PolylineCurve::new(
                samples,
                0.0,
                &cadmpeg_test_support::service_decode_context(),
            )
            .unwrap()
            .unwrap();
            let expected = FiniteReal::new(if parameterized { 1.0 } else { 2.5 }).unwrap();
            let run = |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_collection_items = 0;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = super::parameter_near_point(
                    &ctx,
                    &curve,
                    Point3::new(2.5, 0.0, 0.0),
                    0.0,
                    expected,
                );
                match &result {
                    Ok(value) => {
                        assert_eq!(*value, Some(expected));
                        ctx.finish_session().unwrap();
                    }
                    Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => {
                        assert!(matches!(ctx.finish_session(),
                        Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == *limit));
                    }
                    Err(error) => panic!("unexpected polyline inversion error: {error}"),
                }
                result
            };
            // The best-candidate search visits the three segments exactly once.
            assert_eq!(run(3).unwrap(), Some(expected));
            cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits,
                "IR polyline inversion segment scan",
                run,
            );
        }
    }
}
