// SPDX-License-Identifier: Apache-2.0
//! Vector arithmetic in the parameter plane and in model space.

use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::math::{Point3, Vector3};

/// A parameter-plane point `(u, v)`.
pub(crate) type Uv = [f64; 2];

pub(crate) fn cross2(origin: Uv, a: Uv, b: Uv) -> f64 {
    (a[0] - origin[0]) * (b[1] - origin[1]) - (a[1] - origin[1]) * (b[0] - origin[0])
}

/// Twice the signed area of a closed polygon; positive when it runs
/// counterclockwise.
pub(crate) fn signed_area(points: &[Uv]) -> f64 {
    let mut sum = 0.0;
    for (index, point) in points.iter().enumerate() {
        let next = points[(index + 1) % points.len()];
        sum += point[0] * next[1] - next[0] * point[1];
    }
    sum
}

/// Whether `point` lies inside the closed polygon `points`, by the
/// even-odd rule.
pub(crate) fn contains(points: &[Uv], point: Uv) -> bool {
    let mut inside = false;
    let count = points.len();
    for index in 0..count {
        let a = points[index];
        let b = points[(index + count - 1) % count];
        if (a[1] > point[1]) != (b[1] > point[1]) {
            let cut = a[0] + (point[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
            if point[0] < cut {
                inside = !inside;
            }
        }
    }
    inside
}

pub(crate) fn midpoint(a: Uv, b: Uv) -> Uv {
    [f64::midpoint(a[0], b[0]), f64::midpoint(a[1], b[1])]
}

pub(crate) fn sub(a: Point3, b: Point3) -> Vector3 {
    Vector3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

pub(crate) fn dot(a: Vector3, b: Vector3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

pub(crate) fn cross(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

pub(crate) fn length(a: Vector3) -> f64 {
    dot(a, a).sqrt()
}

pub(crate) fn distance(a: Point3, b: Point3) -> f64 {
    length(sub(a, b))
}

pub(crate) fn scaled(a: Vector3, factor: f64) -> Vector3 {
    Vector3::new(a.x * factor, a.y * factor, a.z * factor)
}

/// `a` with unit length, or `None` when it has no direction.
pub(crate) fn unit(a: Vector3) -> Option<Vector3> {
    let size = length(a);
    (size.is_finite() && size > 0.0).then(|| scaled(a, 1.0 / size))
}

pub(crate) fn mid3(a: Point3, b: Point3) -> Point3 {
    Point3::new(
        f64::midpoint(a.x, b.x),
        f64::midpoint(a.y, b.y),
        f64::midpoint(a.z, b.z),
    )
}

pub(crate) fn centroid3(a: Point3, b: Point3, c: Point3) -> Point3 {
    Point3::new(
        (a.x + b.x + c.x) / 3.0,
        (a.y + b.y + c.y) / 3.0,
        (a.z + b.z + c.z) / 3.0,
    )
}

/// The angle between two vectors, in radians.
pub(crate) fn angle(a: Vector3, b: Vector3) -> f64 {
    let sin = length(cross(a, b));
    let cos = dot(a, b);
    if sin == 0.0 && cos == 0.0 {
        return 0.0;
    }
    sin.portable_atan2(cos)
}

#[cfg(test)]
mod tests;
