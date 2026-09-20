// SPDX-License-Identifier: Apache-2.0
//! Planar intersection arithmetic with scaled products.

use super::{sum::ExactSignedSum, Point2};
use std::cmp::Ordering;

/// Orientation of three finite points. Returns `None` for nonfinite input.
/// The exact-product fallback preserves the sign at extreme scales and at cancellation.
pub fn orientation(a: Point2, b: Point2, p: Point2) -> Option<Ordering> {
    if !a.is_finite() || !b.is_finite() || !p.is_finite() {
        return None;
    }
    let left = (b.u - a.u) * (p.v - a.v);
    let right = (b.v - a.v) * (p.u - a.u);
    let determinant = left - right;
    if determinant.is_finite()
        && determinant.abs() > 4.0 * f64::EPSILON * (left.abs() + right.abs())
    {
        return determinant.partial_cmp(&0.0);
    }
    let mut sum = ExactSignedSum::default();
    for (x, y) in [
        (b.u, p.v),
        (-b.u, a.v),
        (-a.u, p.v),
        (-b.v, p.u),
        (b.v, a.u),
        (a.v, p.u),
    ] {
        sum.add_product(x, y);
    }
    match sum.finish() {
        Some(value) => value.rescale(value.exponent())?.partial_cmp(&0.0),
        None => Some(Ordering::Equal),
    }
}

// Scale differences before subtraction only when their finite subtraction overflows.
fn scaled_displacement(start: Point2, end: Point2, minimum_scale: f64) -> (Point2, f64) {
    let delta = Point2::new(end.u - start.u, end.v - start.v);
    if delta.is_finite() {
        let scale = delta.u.abs().max(delta.v.abs()).max(minimum_scale);
        if scale == 0.0 {
            return (delta, scale);
        }
        (Point2::new(delta.u / scale, delta.v / scale), scale)
    } else {
        let scale = start
            .u
            .abs()
            .max(start.v.abs())
            .max(end.u.abs())
            .max(end.v.abs())
            .max(minimum_scale);
        (
            Point2::new(
                end.u / scale - start.u / scale,
                end.v / scale - start.v / scale,
            ),
            scale,
        )
    }
}

/// Parameters on the infinite line `start + t * (end - start)` at a circle.
/// A tangent returns the same parameter twice. Invalid, degenerate and disjoint inputs return `None`.
// Scaled coordinates and quadratic coefficients use standard mathematical names.
#[allow(clippy::many_single_char_names)]
pub fn line_circle_parameters(
    start: Point2,
    end: Point2,
    center: Point2,
    radius: f64,
) -> Option<[f64; 2]> {
    if !start.is_finite()
        || !end.is_finite()
        || !center.is_finite()
        || !radius.is_finite()
        || radius <= 0.0
    {
        return None;
    }
    let (d, direction_scale) = scaled_displacement(start, end, 0.0);
    if direction_scale == 0.0 {
        return None;
    }
    let (o, geometry_scale) = scaled_displacement(center, start, radius);
    let r = radius / geometry_scale;
    let a = d.u.mul_add(d.u, d.v * d.v);
    if a == 0.0 {
        return None;
    }
    let b = 2.0 * o.u.mul_add(d.u, o.v * d.v);
    let c = o.u.mul_add(o.u, o.v * o.v) - r * r;
    let discriminant = b.mul_add(b, -4.0 * a * c);
    let error = 64.0 * f64::EPSILON * (b * b + (4.0 * a * c).abs());
    if discriminant < -error {
        return None;
    }
    let root = discriminant.max(0.0).sqrt();
    let q = -0.5 * (b + root.copysign(b));
    let parameters = if root == 0.0 {
        [-b / (2.0 * a); 2]
    } else {
        [q / a, c / q]
    };
    Some([
        super::multiply_divide(parameters[0], geometry_scale, direction_scale)?,
        super::multiply_divide(parameters[1], geometry_scale, direction_scale)?,
    ])
}

/// The finite intersections of two positive-radius circles.
/// Coincident circles and invalid or unrepresentable results return `None`.
/// Disjoint circles return an empty vector; a tangent returns one point.
pub fn circle_intersections(
    first: Point2,
    first_radius: f64,
    second: Point2,
    second_radius: f64,
) -> Option<Vec<Point2>> {
    if !first.is_finite()
        || !second.is_finite()
        || !first_radius.is_finite()
        || !second_radius.is_finite()
        || first_radius <= 0.0
        || second_radius <= 0.0
    {
        return None;
    }
    let (delta, scale) = scaled_displacement(first, second, first_radius.max(second_radius));
    let distance = delta.u.hypot(delta.v);
    if distance == 0.0 {
        return (first_radius != second_radius).then(Vec::new);
    }
    let r = first_radius / scale;
    let s = second_radius / scale;
    if distance > r + s || distance < (r - s).abs() {
        return Some(Vec::new());
    }
    let along = 0.5 * (distance + (r - s) * (r + s) / distance);
    let height_squared = (r - along) * (r + along);
    let error = 64.0 * f64::EPSILON * (r * r + along * along);
    if height_squared < -error {
        return Some(Vec::new());
    }
    let height = height_squared.max(0.0).sqrt();
    let unit = Point2::new(delta.u / distance, delta.v / distance);
    let mut points = Vec::new();
    for height in [height, -height] {
        let point = Point2::new(
            super::sum::finite_dot([1.0, scale], [first.u, along * unit.u - height * unit.v])?,
            super::sum::finite_dot([1.0, scale], [first.v, along * unit.v + height * unit.u])?,
        );
        if !point.is_finite() {
            return None;
        }
        if !points.contains(&point) {
            points.push(point);
        }
    }
    Some(points)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intersections_preserve_independent_direction_and_position_scales() {
        let mut roots = line_circle_parameters(
            Point2::new(0.0, 0.0),
            Point2::new(1e-200, 0.0),
            Point2::new(0.0, 0.0),
            1.0,
        )
        .unwrap();
        roots.sort_by(f64::total_cmp);
        assert_eq!(roots, [-1e200, 1e200]);
        let points = circle_intersections(
            Point2::new(-1e308, 0.0),
            1e308,
            Point2::new(1e308, 0.0),
            1e308,
        )
        .unwrap();
        assert_eq!(points, vec![Point2::new(0.0, 0.0)]);
    }

    #[test]
    fn intersections_and_orientation_preserve_scale() {
        for r in [1e-200, 1., 1e200] {
            let a = Point2::new(0., 0.);
            let b = Point2::new(r, 0.);
            let points = circle_intersections(a, r, b, r).unwrap();
            assert_eq!(points.len(), 2);
            for p in points {
                assert!((p.u / r - 0.5).abs() <= 4. * f64::EPSILON);
                assert!((p.v.abs() / r - 0.75_f64.sqrt()).abs() <= 4. * f64::EPSILON);
            }
            let mut parameters =
                line_circle_parameters(Point2::new(-2. * r, 0.), Point2::new(2. * r, 0.), a, r)
                    .unwrap();
            parameters.sort_by(f64::total_cmp);
            assert_eq!(parameters, [0.25, 0.75]);
            assert_eq!(
                orientation(a, b, Point2::new(0., r)),
                Some(Ordering::Greater)
            );
            assert_eq!(orientation(a, b, Point2::new(0., -r)), Some(Ordering::Less));
        }
    }
}
