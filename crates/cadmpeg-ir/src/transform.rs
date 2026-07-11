// SPDX-License-Identifier: Apache-2.0
//! Placements and rigid transforms.

use crate::math::{Point3, Vector3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A right-handed local frame defined by an origin and two orthonormal axes.
///
/// The third axis is the cross product of `axis` and `ref_direction`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Placement {
    /// Local frame origin.
    pub origin: Point3,
    /// Local +Z axis (e.g. a cylinder axis or plane normal).
    pub axis: Vector3,
    /// Local +X axis (reference direction); should be orthogonal to `axis`.
    pub ref_direction: Vector3,
}

/// A 4×4 row-major affine transform applied to a body's geometry.
///
/// The explicit matrix preserves source coefficients. Validation checks the
/// affine bottom row.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Transform {
    /// Row-major 4×4 matrix; `rows[3]` is normally `[0, 0, 0, 1]`.
    pub rows: [[f64; 4]; 4],
}

impl Transform {
    /// The identity transform.
    pub fn identity() -> Self {
        Transform {
            rows: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Transform::identity()
    }
}
