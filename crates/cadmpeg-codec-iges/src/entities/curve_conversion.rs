// SPDX-License-Identifier: Apache-2.0
//! Exact conversions from bounded analytic curves to NURBS carriers.

use cadmpeg_ir::geometry::NurbsCurve;
use cadmpeg_ir::math::{Point3, Vector3};

fn add_scaled(center: Point3, x: Vector3, x_scale: f64, y: Vector3, y_scale: f64) -> Point3 {
    Point3::new(
        center.x + x.x * x_scale + y.x * y_scale,
        center.y + x.y * x_scale + y.y * y_scale,
        center.z + x.z * x_scale + y.z * y_scale,
    )
}

fn cross(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(
        left.y * right.z - left.z * right.y,
        left.z * right.x - left.x * right.z,
        left.x * right.y - left.y * right.x,
    )
}

pub(super) fn circular_arc_nurbs(
    center: Point3,
    axis: Vector3,
    reference: Vector3,
    radius: f64,
    interval: [f64; 2],
) -> Option<NurbsCurve> {
    let delta = interval[1] - interval[0];
    let angular_tolerance = std::f64::consts::TAU * 1.0e-12;
    if !delta.is_finite() || delta <= 0.0 || delta > std::f64::consts::TAU + angular_tolerance {
        return None;
    }
    let delta = delta.min(std::f64::consts::TAU);
    let transverse = cross(axis, reference);
    let spans = (delta / std::f64::consts::FRAC_PI_2).ceil() as usize;
    let step = delta / spans as f64;
    let mut knots = Vec::with_capacity(spans * 2 + 4);
    let mut control_points = Vec::with_capacity(spans * 2 + 1);
    let mut weights = Vec::with_capacity(spans * 2 + 1);
    for span in 0..spans {
        let start = interval[0] + step * span as f64;
        let end = interval[0] + step * (span + 1) as f64;
        let middle = (start + end) * 0.5;
        let middle_weight = ((end - start) * 0.5).cos();
        if !middle_weight.is_finite() || middle_weight <= 0.0 {
            return None;
        }
        if span == 0 {
            control_points.push(add_scaled(
                center,
                reference,
                radius * start.cos(),
                transverse,
                radius * start.sin(),
            ));
            weights.push(1.0);
            knots.extend([start, start, start]);
        } else {
            knots.extend([start, start]);
        }
        control_points.push(add_scaled(
            center,
            reference,
            radius * middle.cos() / middle_weight,
            transverse,
            radius * middle.sin() / middle_weight,
        ));
        weights.push(middle_weight);
        control_points.push(add_scaled(
            center,
            reference,
            radius * end.cos(),
            transverse,
            radius * end.sin(),
        ));
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
