// SPDX-License-Identifier: Apache-2.0
//! Rigid transforms.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

    /// Whether every matrix coefficient is finite.
    pub fn is_finite(&self) -> bool {
        self.rows.iter().flatten().all(|value| value.is_finite())
    }

    /// Whether this is a finite affine transform.
    pub fn is_affine(&self) -> bool {
        self.is_finite() && self.rows[3] == [0.0, 0.0, 0.0, 1.0]
    }
}

impl Default for Transform {
    fn default() -> Self {
        Transform::identity()
    }
}
