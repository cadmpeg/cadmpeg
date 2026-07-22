// SPDX-License-Identifier: Apache-2.0
//! Small geometric value types shared by geometry and topology.
//!
//! These are plain data carriers (no invariants enforced at construction); the
//! validation pass is responsible for sanity checks such as "a direction is
//! non-degenerate" where the IR is expected to hold geometry.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A point in 3D model space, in the document's length unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Point3 {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Z coordinate.
    pub z: f64,
}

impl Point3 {
    /// Construct a point.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Point3 { x, y, z }
    }

    /// Euclidean distance to another point.
    pub fn distance(self, other: Point3) -> f64 {
        self.distance_squared(other).sqrt()
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Vector3 {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
}

impl Vector3 {
    /// Construct a vector.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Vector3 { x, y, z }
    }

    /// Euclidean length.
    pub fn norm(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
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

    /// Unit vector in the same direction, or `None` when the length is
    /// within [`f64::EPSILON`] of zero (degenerate direction).
    #[must_use]
    pub fn unit(self) -> Option<Vector3> {
        let length = self.norm();
        (length > f64::EPSILON)
            .then(|| Vector3::new(self.x / length, self.y / length, self.z / length))
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
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
}

/// An axis-aligned bounding box, in the document's length unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Aabb {
    /// Minimum corner.
    pub min: Point3,
    /// Maximum corner.
    pub max: Point3,
}
