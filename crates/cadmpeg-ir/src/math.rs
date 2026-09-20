// SPDX-License-Identifier: Apache-2.0
//! Small geometric value types shared by geometry and topology.
//!
//! These are plain data carriers (no invariants enforced at construction); the
//! validation pass is responsible for sanity checks such as "a direction is
//! non-degenerate" where the IR is expected to hold geometry.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Scaled least-squares solvers.
pub mod solve;
pub(crate) mod sum;

/// A point in 3D model space, in the document's length unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Point3 {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Z coordinate.
    pub z: f64,
}

impl From<[f64; 3]> for Point3 {
    fn from([x, y, z]: [f64; 3]) -> Self {
        Point3::new(x, y, z)
    }
}

impl From<Point3> for [f64; 3] {
    fn from(point: Point3) -> Self {
        [point.x, point.y, point.z]
    }
}

impl Point3 {
    /// Construct a point.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Point3 { x, y, z }
    }

    /// Whether every coordinate is finite.
    pub const fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    /// Euclidean distance to another point.
    pub fn distance(self, other: Point3) -> f64 {
        (self.x - other.x)
            .hypot(self.y - other.y)
            .hypot(self.z - other.z)
    }

    /// Squared Euclidean distance to another point (no square root).
    pub fn distance_squared(self, other: Point3) -> f64 {
        (self.x - other.x).powi(2) + (self.y - other.y).powi(2) + (self.z - other.z).powi(2)
    }

    /// Displacement from `origin` to `self`, i.e. `self - origin`.
    pub fn vector_from(self, origin: Point3) -> Vector3 {
        Vector3::new(self.x - origin.x, self.y - origin.y, self.z - origin.z)
    }

    /// Point translated by `scale * vector`.
    #[must_use]
    pub fn translated(self, vector: Vector3, scale: f64) -> Point3 {
        Point3::new(
            self.x + scale * vector.x,
            self.y + scale * vector.y,
            self.z + scale * vector.z,
        )
    }
}

/// A 3D vector. Depending on context this may be a direction (often but not
/// always unit length) or a length-bearing displacement.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Vector3 {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
}

impl From<[f64; 3]> for Vector3 {
    fn from([x, y, z]: [f64; 3]) -> Self {
        Vector3::new(x, y, z)
    }
}

impl From<Vector3> for [f64; 3] {
    fn from(vector: Vector3) -> Self {
        [vector.x, vector.y, vector.z]
    }
}

impl Vector3 {
    /// Construct a vector.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Vector3 { x, y, z }
    }

    /// Whether every component is finite.
    pub const fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    /// Euclidean length.
    pub fn norm(&self) -> f64 {
        self.x.hypot(self.y).hypot(self.z)
    }

    /// Dot product with another vector.
    pub fn dot(self, other: Vector3) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Cross product with another vector.
    #[must_use]
    pub fn cross(self, other: Vector3) -> Vector3 {
        Vector3::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Vector scaled by a factor.
    #[must_use]
    pub fn scale(self, factor: f64) -> Vector3 {
        Vector3::new(self.x * factor, self.y * factor, self.z * factor)
    }

    /// Unit vector in the same direction, or `None` when a component is
    /// non-finite or the length is within [`f64::EPSILON`] of zero.
    #[must_use]
    pub fn unit(self) -> Option<Vector3> {
        if self.norm() <= f64::EPSILON {
            return None;
        }
        self.unit_nonzero()
    }

    /// Unit direction for every finite nonzero vector, including subnormals.
    /// Callers that impose a geometric length threshold must check it separately.
    #[must_use]
    pub fn unit_nonzero(self) -> Option<Vector3> {
        if !self.is_finite() {
            return None;
        }
        let scale = self.x.abs().max(self.y.abs()).max(self.z.abs());
        if scale == 0.0 {
            return None;
        }
        // Divide by the power of two the largest component states, not by the
        // component. The division is then exact, so a direction whose
        // components state an exact ratio keeps it, and the largest scaled
        // component stays in `[0.5, 1)`, which is what the norm needs.
        let exponent = sum::scaled_finite(scale)?.exponent();
        let scaled = Vector3::new(
            sum::scale_power_of_two(self.x, -exponent)?,
            sum::scale_power_of_two(self.y, -exponent)?,
            sum::scale_power_of_two(self.z, -exponent)?,
        );
        let length = scaled.norm();
        Some(Vector3::new(
            scaled.x / length,
            scaled.y / length,
            scaled.z / length,
        ))
    }
}

impl std::ops::Add for Vector3 {
    type Output = Vector3;

    fn add(self, other: Vector3) -> Vector3 {
        Vector3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl std::ops::Sub for Vector3 {
    type Output = Vector3;

    fn sub(self, other: Vector3) -> Vector3 {
        Vector3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

/// A point in 2D surface parameter (u, v) space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Point2 {
    /// U parameter.
    pub u: f64,
    /// V parameter.
    pub v: f64,
}

impl Point2 {
    /// Construct a 2D parameter point.
    pub fn new(u: f64, v: f64) -> Self {
        Point2 { u, v }
    }

    /// Whether every coordinate is finite.
    pub const fn is_finite(&self) -> bool {
        self.u.is_finite() && self.v.is_finite()
    }
}

/// Compute `left * right / denominator` without intermediate range loss.
/// Inputs must be finite and the denominator nonzero; the result must be finite.
pub fn multiply_divide(left: f64, right: f64, denominator: f64) -> Option<f64> {
    if ![left, right, denominator].into_iter().all(f64::is_finite) {
        return None;
    }
    let denominator = sum::scaled_finite(denominator)?;
    match sum::product_sum(std::iter::once(Some([left, right]))) {
        sum::ProductSum::Value(numerator) => numerator.quotient(denominator),
        sum::ProductSum::Zero => Some(0.0),
        sum::ProductSum::Undefined => None,
    }
}

/// Reflect a finite parameter about the midpoint of two finite bounds.
/// The exact sum avoids overflow and cancellation in `start + end - parameter`.
/// Returns `None` when an input or the reflected result is non-finite.
pub fn reflect_parameter(parameter: f64, start: f64, end: f64) -> Option<f64> {
    if ![parameter, start, end].into_iter().all(f64::is_finite) {
        return None;
    }
    let mut sum = sum::ExactSignedSum::default();
    sum.add_product(start, 1.0);
    sum.add_product(end, 1.0);
    sum.add_product(parameter, -1.0);
    sum.finish().map_or(Some(0.0), sum::ScaledValue::finite)
}

#[cfg(test)]
mod tests;
