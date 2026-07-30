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

    /// Whether this is a finite, right-handed rigid transform.
    pub fn is_proper_rigid(&self) -> bool {
        if !self.is_affine() {
            return false;
        }
        let x = [self.rows[0][0], self.rows[1][0], self.rows[2][0]];
        let y = [self.rows[0][1], self.rows[1][1], self.rows[2][1]];
        let z = [self.rows[0][2], self.rows[1][2], self.rows[2][2]];
        let dot = |left: [f64; 3], right: [f64; 3]| {
            left.into_iter()
                .zip(right)
                .map(|(left, right)| left * right)
                .sum::<f64>()
        };
        let cross = [
            x[1] * y[2] - x[2] * y[1],
            x[2] * y[0] - x[0] * y[2],
            x[0] * y[1] - x[1] * y[0],
        ];
        [x, y, z]
            .into_iter()
            .all(|axis| (dot(axis, axis) - 1.0).abs() <= 1.0e-9)
            && dot(x, y).abs() <= 1.0e-9
            && dot(x, z).abs() <= 1.0e-9
            && dot(y, z).abs() <= 1.0e-9
            && (dot(cross, z) - 1.0).abs() <= 1.0e-9
    }
}

impl Default for Transform {
    fn default() -> Self {
        Transform::identity()
    }
}
