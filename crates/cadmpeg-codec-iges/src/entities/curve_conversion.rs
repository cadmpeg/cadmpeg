// SPDX-License-Identifier: Apache-2.0
//! Exact conversions from bounded analytic curves to NURBS carriers.

use cadmpeg_ir::geometry::NurbsCurve;
use cadmpeg_ir::math::{Point3, Vector3};

/// Angular slack this module allows when bounding or dividing a sweep.
pub(crate) const ANGULAR_TOLERANCE: f64 = std::f64::consts::TAU * 1.0e-12;

pub(super) fn angularly_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= ANGULAR_TOLERANCE
}

/// Number of quarter-turn rational spans a positive `sweep` divides into.
///
/// `ceil` is discontinuous exactly at a whole number of quarter turns, which is
/// where CAD sweeps overwhelmingly land: a semicircle divides to exactly `2.0`,
/// and one unit in the last place above that yields three spans and a
/// structurally different curve. A sweep is a difference of decoded angles, so
/// it carries last-place noise and the platform's libm decides which side of the
/// boundary it falls on. Backing the sweep off by [`ANGULAR_TOLERANCE`] first
/// keeps an exact multiple of a quarter turn on the lower side.
pub(super) fn quarter_turn_spans(sweep: f64) -> usize {
    let quarters = (sweep - ANGULAR_TOLERANCE) / std::f64::consts::FRAC_PI_2;
    quarters.ceil().max(1.0) as usize
}

pub(crate) fn circular_arc_nurbs(
    center: Point3,
    axis: Vector3,
    reference: Vector3,
    radius: f64,
    interval: [f64; 2],
) -> Option<NurbsCurve> {
    elliptical_arc_nurbs(center, axis, reference, radius, radius, interval)
}

pub(crate) fn elliptical_arc_nurbs(
    center: Point3,
    axis: Vector3,
    major_direction: Vector3,
    major_radius: f64,
    minor_radius: f64,
    interval: [f64; 2],
) -> Option<NurbsCurve> {
    let delta = interval[1] - interval[0];
    if !delta.is_finite()
        || delta <= 0.0
        || delta > std::f64::consts::TAU + ANGULAR_TOLERANCE
        || !major_radius.is_finite()
        || !minor_radius.is_finite()
        || major_radius <= 0.0
        || minor_radius <= 0.0
    {
        return None;
    }
    let delta = delta.min(std::f64::consts::TAU);
    let transverse = axis.cross(major_direction);
    let spans = quarter_turn_spans(delta);
    let step = delta / spans as f64;
    let mut knots = Vec::with_capacity(spans * 2 + 4);
    let mut control_points = Vec::with_capacity(spans * 2 + 1);
    let mut weights = Vec::with_capacity(spans * 2 + 1);
    for span in 0..spans {
        let start = if span == 0 {
            interval[0]
        } else {
            interval[0] + step * span as f64
        };
        let end = if span + 1 == spans {
            interval[1]
        } else {
            interval[0] + step * (span + 1) as f64
        };
        let middle = (start + end) * 0.5;
        let middle_weight = ((end - start) * 0.5).cos();
        if !middle_weight.is_finite() || middle_weight <= 0.0 {
            return None;
        }
        if span == 0 {
            control_points.push(
                center
                    .translated(major_direction, major_radius * start.cos())
                    .translated(transverse, minor_radius * start.sin()),
            );
            weights.push(1.0);
            knots.extend([start, start, start]);
        } else {
            knots.extend([start, start]);
        }
        control_points.push(
            center
                .translated(major_direction, major_radius * middle.cos() / middle_weight)
                .translated(transverse, minor_radius * middle.sin() / middle_weight),
        );
        weights.push(middle_weight);
        control_points.push(
            center
                .translated(major_direction, major_radius * end.cos())
                .translated(transverse, minor_radius * end.sin()),
        );
        weights.push(1.0);
        if span + 1 == spans {
            knots.extend([end, end, end]);
        }
    }
    Some(NurbsCurve {
        degree: 2,
        knots,
        control_points,
        weights: Some(weights),
        periodic: false,
    })
}

pub(crate) fn parabolic_arc_nurbs(
    vertex: Point3,
    axis: Vector3,
    major_direction: Vector3,
    focal_distance: f64,
    interval: [f64; 2],
) -> Option<NurbsCurve> {
    let [start, end] = interval;
    let delta = end - start;
    if !delta.is_finite() || delta <= 0.0 || !focal_distance.is_finite() || focal_distance <= 0.0 {
        return None;
    }
    let transverse = axis.cross(major_direction);
    let start_point = vertex
        .translated(major_direction, focal_distance * start * start)
        .translated(transverse, 2.0 * focal_distance * start);
    let middle_point = start_point
        .translated(major_direction, focal_distance * start * delta)
        .translated(transverse, focal_distance * delta);
    let end_point = vertex
        .translated(major_direction, focal_distance * end * end)
        .translated(transverse, 2.0 * focal_distance * end);
    [start_point, middle_point, end_point]
        .iter()
        .all(|point| {
            [point.x, point.y, point.z]
                .iter()
                .all(|value| value.is_finite())
        })
        .then_some(NurbsCurve {
            degree: 2,
            knots: vec![start, start, start, end, end, end],
            control_points: vec![start_point, middle_point, end_point],
            weights: None,
            periodic: false,
        })
}

#[cfg(test)]
mod tests;
