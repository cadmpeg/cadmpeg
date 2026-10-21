// SPDX-License-Identifier: Apache-2.0
//! Polynomial B-spline approximation of a native helix path.

use cadmpeg_core::convert::f64_from_index;
use cadmpeg_core::decode::ResourceLimit;

use super::{finite_or_refusal, helix_differential, ModelCurveDifferential};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::HelixCurveConstruction;
use crate::math::{Point3, Vector3};
use crate::scalar::PositiveLength;

/// Largest angle one cubic segment spans before its deviation is measured.
const MAX_SEGMENT_ANGLE: f64 = std::f64::consts::FRAC_PI_2;

/// Number of times the segment count doubles before the fit is refused.
const MAX_REFINEMENTS: u32 = 16;

/// Segment fractions at which a cubic segment is compared with the helix.
const DEVIATION_SAMPLES: [f64; 7] = [0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875];

/// Why a helix has no fitted B-spline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelixNurbsError {
    /// The helix has no finite point or tangent at a segment end or sample.
    NotFinite,
    /// No segment count within the refinement bound meets the tolerance.
    ToleranceNotMet,
    /// The evaluation was refused by its resource admission.
    Refused(ResourceLimit),
}

/// A cubic B-spline that stays within `tolerance` of `helix` over its whole
/// angle range.
///
/// Each segment is the cubic Hermite interpolant of the helix point and
/// tangent at both segment ends, so the spline passes through the helix at
/// every segment end and shares its parameter there. Interior knots carry
/// multiplicity three: the poles of each segment are its Bézier poles.
///
/// The deviation is measured between the spline and the helix at the same
/// parameter. The segment count doubles until every sampled deviation is
/// within `tolerance`, and the fit is refused after `MAX_REFINEMENTS`
/// doublings.
pub fn helix_nurbs(
    helix: &HelixCurveConstruction,
    tolerance: PositiveLength,
) -> Result<NurbsCurve, HelixNurbsError> {
    let [start, end] = helix.angle_range().finite_components();
    let (start, end) = (start.get(), end.get());
    let span = end - start;
    if span <= 0.0 {
        return Err(HelixNurbsError::NotFinite);
    }

    let mut segments: usize = 1;
    for _ in 0..=MAX_REFINEMENTS {
        let count = f64_from_index(segments).ok_or(HelixNurbsError::ToleranceNotMet)?;
        if span / count <= MAX_SEGMENT_ANGLE {
            let breaks = segment_breaks(start, end, segments)?;
            let ends = breaks
                .iter()
                .map(|&parameter| differential(helix, parameter))
                .collect::<Result<Vec<_>, _>>()?;
            let poles = hermite_poles(&breaks, &ends)?;
            if within_tolerance(helix, &breaks, &poles, tolerance.get())? {
                return spline(&breaks, poles);
            }
        }
        segments = segments
            .checked_mul(2)
            .ok_or(HelixNurbsError::ToleranceNotMet)?;
    }
    Err(HelixNurbsError::ToleranceNotMet)
}

fn differential(
    helix: &HelixCurveConstruction,
    parameter: f64,
) -> Result<ModelCurveDifferential, HelixNurbsError> {
    finite_or_refusal(helix_differential(helix, parameter))
        .map_err(HelixNurbsError::Refused)?
        .ok_or(HelixNurbsError::NotFinite)
}

fn tangent(differential: &ModelCurveDifferential) -> Result<FiniteVector3, HelixNurbsError> {
    finite_or_refusal(differential.tangent())
        .map_err(HelixNurbsError::Refused)?
        .ok_or(HelixNurbsError::NotFinite)
}

/// `segments + 1` parameters from `start` to `end` in equal steps, with both
/// ends exact.
fn segment_breaks(start: f64, end: f64, segments: usize) -> Result<Vec<f64>, HelixNurbsError> {
    let count = f64_from_index(segments).ok_or(HelixNurbsError::ToleranceNotMet)?;
    let capacity = segments
        .checked_add(1)
        .ok_or(HelixNurbsError::ToleranceNotMet)?;
    let mut breaks = Vec::with_capacity(capacity);
    for index in 0..segments {
        let fraction = f64_from_index(index).ok_or(HelixNurbsError::ToleranceNotMet)? / count;
        breaks.push(start + (end - start) * fraction);
    }
    breaks.push(end);
    Ok(breaks)
}

/// The Bézier poles of every segment, with each shared segment end once.
fn hermite_poles(
    breaks: &[f64],
    ends: &[ModelCurveDifferential],
) -> Result<Vec<Point3>, HelixNurbsError> {
    let capacity = ends
        .len()
        .checked_mul(3)
        .ok_or(HelixNurbsError::ToleranceNotMet)?;
    let mut poles = Vec::with_capacity(capacity);
    for (pair, parameters) in ends.windows(2).zip(breaks.windows(2)) {
        let [from, to] = [pair[0], pair[1]];
        let third = (parameters[1] - parameters[0]) / 3.0;
        let from_point = from.point.get();
        let to_point = to.point.get();
        if poles.is_empty() {
            poles.push(from_point);
        }
        poles.push(shifted(from_point, tangent(&from)?.get(), third));
        poles.push(shifted(to_point, tangent(&to)?.get(), -third));
        poles.push(to_point);
    }
    Ok(poles)
}

fn shifted(point: Point3, direction: Vector3, scale: f64) -> Point3 {
    Point3::new(
        point.x + direction.x * scale,
        point.y + direction.y * scale,
        point.z + direction.z * scale,
    )
}

fn within_tolerance(
    helix: &HelixCurveConstruction,
    breaks: &[f64],
    poles: &[Point3],
    tolerance: f64,
) -> Result<bool, HelixNurbsError> {
    for (segment, parameters) in poles.windows(4).step_by(3).zip(breaks.windows(2)) {
        for fraction in DEVIATION_SAMPLES {
            let parameter = parameters[0] + (parameters[1] - parameters[0]) * fraction;
            let exact = differential(helix, parameter)?.point.get();
            let fitted = bezier_point(segment, fraction);
            let deviation =
                Vector3::new(fitted.x - exact.x, fitted.y - exact.y, fitted.z - exact.z).norm();
            if deviation.is_nan() || deviation > tolerance {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn bezier_point(poles: &[Point3], fraction: f64) -> Point3 {
    let rest = 1.0 - fraction;
    let weights = [
        rest * rest * rest,
        3.0 * rest * rest * fraction,
        3.0 * rest * fraction * fraction,
        fraction * fraction * fraction,
    ];
    let mut point = Point3::new(0.0, 0.0, 0.0);
    for (pole, weight) in poles.iter().zip(weights) {
        point = shifted(point, Vector3::new(pole.x, pole.y, pole.z), weight);
    }
    point
}

fn spline(breaks: &[f64], poles: Vec<Point3>) -> Result<NurbsCurve, HelixNurbsError> {
    let (Some(&first), Some(&last)) = (breaks.first(), breaks.last()) else {
        return Err(HelixNurbsError::NotFinite);
    };
    let last_index = breaks
        .len()
        .checked_sub(1)
        .ok_or(HelixNurbsError::NotFinite)?;
    let interior = breaks
        .get(1..last_index)
        .ok_or(HelixNurbsError::NotFinite)?;
    let mut knots = vec![first; 4];
    for &knot in interior {
        knots.extend([knot; 3]);
    }
    knots.extend([last; 4]);
    let poles = poles
        .into_iter()
        .map(|pole| FinitePoint3::new(pole).ok_or(HelixNurbsError::NotFinite))
        .collect::<Result<Vec<_>, _>>()?;
    NurbsCurve::from_lanes(3, knots, poles, None, false).map_err(|_| HelixNurbsError::NotFinite)
}

#[cfg(test)]
mod tests;
