// SPDX-License-Identifier: Apache-2.0
//! Sampled polyline point and tangent evaluation.

use super::admission::EvaluationAdmission;
use super::rational::finite_lanes;
use super::{difference_quotient, EvaluationFailure};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::sampled::PolylineCurve;
use crate::math::{Point3, Vector3};
use crate::scalar::{ExtendedReal, FiniteReal, SegmentPosition};
use crate::topology::ParameterInterval;

/// Select the nearest-seed parameter without copying sample rows.
pub(super) fn parameter_near_point(
    polyline: &PolylineCurve,
    point: Point3,
    tolerance: f64,
    seed: FiniteReal,
) -> Option<FiniteReal> {
    if polyline.point_count() < 2 {
        return None;
    }
    let mut best_candidate: Option<FiniteReal> = None;
    for segment in 0..polyline.point_count() - 1 {
        let (parameter_start, parameter_end) = polyline
            .parameter_at(segment)
            .zip(polyline.parameter_at(segment + 1))?;
        let [parameter_start, parameter_end] = [parameter_start.get(), parameter_end.get()];
        let parameter_width = parameter_end - parameter_start;
        if parameter_width == 0.0 {
            continue;
        }
        let (start, end) = polyline
            .point_at(segment)
            .zip(polyline.point_at(segment + 1))?;
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
    best_candidate
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
        admission.work(1).map_err(EvaluationFailure::from)?;
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
        admission.work(1).map_err(EvaluationFailure::from)?;
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
    use super::{polyline_point, EvaluationAdmission};
    use crate::features::FinitePoint3;
    use crate::math::Point3;
    use crate::scalar::FiniteReal;

    fn samples() -> ([FinitePoint3; 4], [FiniteReal; 4]) {
        let points =
            [0.0, 1.0, 2.0, 3.0].map(|x| FinitePoint3::new(Point3::new(x, 0.0, 0.0)).unwrap());
        (points, FiniteReal::array([0.0, 1.0, 2.0, 3.0]).unwrap())
    }

    #[test]
    fn polyline_point_interpolates_its_first_enclosing_segment() {
        let (points, parameters) = samples();
        for parameter in [0.5, 2.5] {
            let point = polyline_point(
                EvaluationAdmission::Standard,
                points.len(),
                |index| points.get(index).copied(),
                |index| parameters.get(index).copied(),
                parameter,
            )
            .unwrap();
            assert_eq!(point.get(), Point3::new(parameter, 0.0, 0.0));
        }
    }
    #[test]
    fn tangent_reads_only_points_of_enclosing_segments() {
        let (points, parameters) = samples();
        let reads = std::cell::Cell::new(0);
        let tangent = super::polyline_tangent(
            EvaluationAdmission::Standard,
            points.len(),
            |index| {
                assert!(index < 2, "unselected segment point");
                reads.set(reads.get() + 1);
                points.get(index).copied()
            },
            |index| parameters.get(index).copied(),
            0.5,
        )
        .expect("straight segment tangent");
        assert_eq!(tangent.get(), crate::math::Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn tangent_stops_before_the_suffix_after_a_corner_disagrees() {
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 1.0, 0.0),
            Point3::new(3.0, 1.0, 0.0),
        ]
        .map(|point| FinitePoint3::new(point).unwrap());
        let parameters = FiniteReal::array([0.0, 1.0, 2.0, 3.0]).unwrap();
        let result = super::polyline_tangent(
            EvaluationAdmission::Standard,
            points.len(),
            |index| points.get(index).copied(),
            |index| {
                assert!(index < 3, "unvisited parameter suffix");
                parameters.get(index).copied()
            },
            1.0,
        );
        assert_eq!(result, Err(super::EvaluationFailure::NoValue));
    }
}
