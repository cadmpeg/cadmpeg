// SPDX-License-Identifier: Apache-2.0
//! Small vector helpers over the IR's raw point and vector lanes.

use cadmpeg_ir::math::{Point3, Vector3};

/// `point + vector`.
pub(crate) fn offset(point: Point3, vector: Vector3) -> Point3 {
    Point3::new(point.x + vector.x, point.y + vector.y, point.z + vector.z)
}

/// The linear combination `Σ scale × vector`.
pub(crate) fn combine(terms: &[(f64, Vector3)]) -> Vector3 {
    terms
        .iter()
        .fold(Vector3::new(0.0, 0.0, 0.0), |sum, (scale, vector)| {
            sum + vector.scale(*scale)
        })
}

/// One coordinate of a point.
pub(crate) const fn coordinate(point: Point3, axis: usize) -> f64 {
    match axis {
        0 => point.x,
        1 => point.y,
        _ => point.z,
    }
}

/// One component of a vector.
pub(crate) const fn component(vector: Vector3, axis: usize) -> f64 {
    match axis {
        0 => vector.x,
        1 => vector.y,
        _ => vector.z,
    }
}

/// An axis-aligned box accumulated from points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Aabb {
    pub(crate) min: [f64; 3],
    pub(crate) max: [f64; 3],
}

impl Aabb {
    pub(crate) fn of_point(point: Point3) -> Self {
        let coordinates = [point.x, point.y, point.z];
        Self {
            min: coordinates,
            max: coordinates,
        }
    }

    pub(crate) fn include(&mut self, point: Point3) {
        for axis in 0..3 {
            let value = coordinate(point, axis);
            self.min[axis] = self.min[axis].min(value);
            self.max[axis] = self.max[axis].max(value);
        }
    }

    pub(crate) fn union(self, other: Self) -> Self {
        Self {
            min: std::array::from_fn(|axis| self.min[axis].min(other.min[axis])),
            max: std::array::from_fn(|axis| self.max[axis].max(other.max[axis])),
        }
    }

    pub(crate) fn diagonal(&self) -> f64 {
        Vector3::new(
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        )
        .norm()
    }

    pub(crate) fn min_point(&self) -> Point3 {
        Point3::new(self.min[0], self.min[1], self.min[2])
    }

    pub(crate) fn max_point(&self) -> Point3 {
        Point3::new(self.max[0], self.max[1], self.max[2])
    }
}
