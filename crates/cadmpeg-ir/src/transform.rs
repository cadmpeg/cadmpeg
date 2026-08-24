// SPDX-License-Identifier: Apache-2.0
//! Rigid transforms.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::math::{Point2, Point3, Vector3};

/// A 3×3 row-major affine transform applied to two-dimensional geometry.
///
/// The explicit matrix preserves source coefficients. Validation checks the
/// affine bottom row.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct Transform2 {
    /// Row-major 3×3 matrix; `rows[2]` is normally `[0, 0, 1]`.
    pub rows: [[f64; 3]; 3],
}

impl Transform2 {
    /// The identity transform.
    pub fn identity() -> Self {
        Transform2 {
            rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    /// Whether every matrix coefficient is finite.
    pub fn is_finite(&self) -> bool {
        self.rows.iter().flatten().all(|value| value.is_finite())
    }

    /// Whether this is a finite affine transform.
    pub fn is_affine(&self) -> bool {
        self.is_finite() && self.rows[2] == [0.0, 0.0, 1.0]
    }

    /// Applies this affine transform to a two-dimensional point.
    pub fn apply_point(self, point: Point2) -> Point2 {
        Point2::new(
            self.rows[0][0] * point.u + self.rows[0][1] * point.v + self.rows[0][2],
            self.rows[1][0] * point.u + self.rows[1][1] * point.v + self.rows[1][2],
        )
    }

    /// Applies this transform's linear component to a two-dimensional vector.
    pub fn apply_vector(self, vector: Point2) -> Point2 {
        Point2::new(
            self.rows[0][0] * vector.u + self.rows[0][1] * vector.v,
            self.rows[1][0] * vector.u + self.rows[1][1] * vector.v,
        )
    }
}

/// A 4×4 row-major affine transform applied to a body's geometry.
///
/// The explicit matrix preserves source coefficients. Validation checks the
/// affine bottom row.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
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
        const EPSILON: f64 = 1e-9;
        if !self.is_finite()
            || self.rows[3]
                .iter()
                .zip([0.0, 0.0, 0.0, 1.0])
                .any(|(actual, expected)| (*actual - expected).abs() > EPSILON)
        {
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
            .all(|axis| (dot(axis, axis) - 1.0).abs() <= EPSILON)
            && dot(x, y).abs() <= EPSILON
            && dot(x, z).abs() <= EPSILON
            && dot(y, z).abs() <= EPSILON
            && (dot(cross, z) - 1.0).abs() <= EPSILON
    }

    /// Composes transforms as `self * right` for column-vector application.
    #[must_use]
    pub fn compose(self, right: Self) -> Self {
        let mut rows = [[0.0; 4]; 4];
        for (row, values) in rows.iter_mut().enumerate() {
            for (column, value) in values.iter_mut().enumerate() {
                *value = (0..4)
                    .map(|inner| self.rows[row][inner] * right.rows[inner][column])
                    .sum();
            }
        }
        Self { rows }
    }

    /// Applies this affine transform to a point.
    pub fn apply_point(self, point: Point3) -> Point3 {
        Point3::new(
            self.rows[0][0] * point.x
                + self.rows[0][1] * point.y
                + self.rows[0][2] * point.z
                + self.rows[0][3],
            self.rows[1][0] * point.x
                + self.rows[1][1] * point.y
                + self.rows[1][2] * point.z
                + self.rows[1][3],
            self.rows[2][0] * point.x
                + self.rows[2][1] * point.y
                + self.rows[2][2] * point.z
                + self.rows[2][3],
        )
    }

    /// Applies this transform's linear component to a vector.
    pub fn apply_vector(self, vector: Vector3) -> Vector3 {
        Vector3::new(
            self.rows[0][0] * vector.x + self.rows[0][1] * vector.y + self.rows[0][2] * vector.z,
            self.rows[1][0] * vector.x + self.rows[1][1] * vector.y + self.rows[1][2] * vector.z,
            self.rows[2][0] * vector.x + self.rows[2][1] * vector.y + self.rows[2][2] * vector.z,
        )
    }

    /// Applies the inverse-transpose linear transform and normalizes the result.
    pub fn apply_normal(self, normal: Vector3) -> Option<Vector3> {
        let inverse = self.try_inverse_affine()?;
        let transformed = Vector3::new(
            inverse.rows[0][0] * normal.x
                + inverse.rows[1][0] * normal.y
                + inverse.rows[2][0] * normal.z,
            inverse.rows[0][1] * normal.x
                + inverse.rows[1][1] * normal.y
                + inverse.rows[2][1] * normal.z,
            inverse.rows[0][2] * normal.x
                + inverse.rows[1][2] * normal.y
                + inverse.rows[2][2] * normal.z,
        );
        let length = (transformed.x * transformed.x
            + transformed.y * transformed.y
            + transformed.z * transformed.z)
            .sqrt();
        (length.is_finite() && length > 0.0).then(|| {
            Vector3::new(
                transformed.x / length,
                transformed.y / length,
                transformed.z / length,
            )
        })
    }

    /// Inverts a finite affine transform with a nonsingular linear component.
    pub fn try_inverse_affine(self) -> Option<Self> {
        if !self.is_affine() {
            return None;
        }
        let m = self.rows;
        let determinant = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        if determinant == 0.0 || !determinant.is_finite() {
            return None;
        }
        let inverse_linear = [
            [
                (m[1][1] * m[2][2] - m[1][2] * m[2][1]) / determinant,
                (m[0][2] * m[2][1] - m[0][1] * m[2][2]) / determinant,
                (m[0][1] * m[1][2] - m[0][2] * m[1][1]) / determinant,
            ],
            [
                (m[1][2] * m[2][0] - m[1][0] * m[2][2]) / determinant,
                (m[0][0] * m[2][2] - m[0][2] * m[2][0]) / determinant,
                (m[0][2] * m[1][0] - m[0][0] * m[1][2]) / determinant,
            ],
            [
                (m[1][0] * m[2][1] - m[1][1] * m[2][0]) / determinant,
                (m[0][1] * m[2][0] - m[0][0] * m[2][1]) / determinant,
                (m[0][0] * m[1][1] - m[0][1] * m[1][0]) / determinant,
            ],
        ];
        let translation = [m[0][3], m[1][3], m[2][3]];
        let mut rows = [[0.0; 4]; 4];
        for row in 0..3 {
            rows[row][..3].copy_from_slice(&inverse_linear[row]);
            rows[row][3] = -inverse_linear[row]
                .iter()
                .zip(translation)
                .map(|(coefficient, value)| coefficient * value)
                .sum::<f64>();
        }
        rows[3][3] = 1.0;
        rows.iter()
            .flatten()
            .all(|value| value.is_finite())
            .then_some(Self { rows })
    }
}

impl Default for Transform {
    fn default() -> Self {
        Transform::identity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_and_inverse_preserve_points_and_vectors() {
        let transform = Transform {
            rows: [
                [2.0, 0.5, 0.0, 4.0],
                [0.0, 3.0, 0.0, -2.0],
                [0.0, 0.0, 4.0, 1.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        let point = Point3::new(1.0, 2.0, 3.0);
        let vector = Vector3::new(1.0, 2.0, 3.0);
        let inverse = transform
            .try_inverse_affine()
            .expect("invertible affine transform");
        assert_eq!(inverse.apply_point(transform.apply_point(point)), point);
        assert_eq!(Transform::identity().compose(transform), transform);
        assert_eq!(transform.apply_vector(vector), Vector3::new(3.0, 6.0, 12.0));
    }

    #[test]
    fn normal_uses_inverse_transpose_and_rejects_singular_transforms() {
        let transform = Transform {
            rows: [
                [2.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        assert_eq!(
            transform.apply_normal(Vector3::new(1.0, 1.0, 0.0)),
            Some(Vector3::new(
                1.0 / 5.0_f64.sqrt(),
                2.0 / 5.0_f64.sqrt(),
                0.0
            ))
        );
        let mut singular = transform;
        singular.rows[0][0] = 0.0;
        assert!(singular.try_inverse_affine().is_none());
        assert!(singular.apply_normal(Vector3::new(1.0, 0.0, 0.0)).is_none());
    }
}
