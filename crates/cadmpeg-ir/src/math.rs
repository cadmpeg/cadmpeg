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
