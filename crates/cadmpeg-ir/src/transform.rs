// SPDX-License-Identifier: Apache-2.0
//! Placements and rigid transforms.

use crate::math::{Point3, Vector3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A right-handed coordinate placement: an origin plus two orthonormal axes
/// (the third is their cross product). This is the ACIS-style representation of
/// an analytic entity's local frame.
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
/// Stored explicitly (rather than decomposed) so a byte-exact transform record
/// round-trips without lossy decomposition. The bottom row is included so the
/// matrix is self-describing; validation checks it is affine.
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
