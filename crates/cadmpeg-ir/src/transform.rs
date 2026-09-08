// SPDX-License-Identifier: Apache-2.0
//! Rigid transforms.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::math::{Point2, Point3, Vector3};

fn deserialize_affine2<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<[[f64; 3]; 3], D::Error> {
    let rows = <[[f64; 3]; 3]>::deserialize(deserializer)?;
    Transform2::from_rows(rows)
        .map(|transform| transform.rows)
        .ok_or_else(|| serde::de::Error::custom("transform2 is not affine"))
}

fn deserialize_affine4<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<[[f64; 4]; 4], D::Error> {
    let rows = <[[f64; 4]; 4]>::deserialize(deserializer)?;
    Transform::from_rows(rows)
        .map(|transform| transform.rows)
        .ok_or_else(|| serde::de::Error::custom("transform is not affine"))
}

/// A 3×3 row-major affine transform applied to two-dimensional geometry.
///
/// The explicit matrix preserves source coefficients. The bottom row is
/// `[0, 0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct Transform2 {
    #[serde(deserialize_with = "deserialize_affine2")]
    rows: [[f64; 3]; 3],
}

impl Default for Transform2 {
    fn default() -> Self {
        Self::identity()
    }
}

impl Transform2 {
    /// The identity transform.
    pub fn identity() -> Self {
        Self {
            rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    /// Build an affine transform from the two linear rows and translation.
    #[must_use]
    pub fn affine(rows: [[f64; 3]; 2]) -> Option<Self> {
        Self::from_rows([rows[0], rows[1], [0.0, 0.0, 1.0]])
    }

    /// Build from a 3×3 matrix, rejecting a non-affine or non-finite bottom row.
    #[must_use]
    pub fn from_rows(rows: [[f64; 3]; 3]) -> Option<Self> {
        let transform = Self { rows };
        transform.is_affine().then_some(transform)
    }

    /// Row-major 3×3 matrix.
    #[must_use]
    pub fn rows(self) -> [[f64; 3]; 3] {
        self.rows
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

/// Failure to compute a finite affine transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransformError {
    /// Arithmetic produced a non-finite coefficient.
    #[error("transform arithmetic produced non-finite coefficients")]
    NonFinite,
    /// The linear component has no inverse.
    #[error("transform linear component is singular")]
    Singular,
}

/// A 4×4 row-major affine transform applied to a body's geometry.
///
/// The explicit matrix preserves source coefficients. The bottom row is
/// `[0, 0, 0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct Transform {
    #[serde(deserialize_with = "deserialize_affine4")]
    rows: [[f64; 4]; 4],
}

impl Default for Transform {
    fn default() -> Self {
        Self::identity()
    }
}

impl Transform {
    /// The identity transform.
    pub fn identity() -> Self {
        Self {
            rows: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Build an affine transform from the three linear rows and translation.
    #[must_use]
    pub fn affine(rows: [[f64; 4]; 3]) -> Option<Self> {
        Self::from_rows([rows[0], rows[1], rows[2], [0.0, 0.0, 0.0, 1.0]])
    }

    /// Build from a 4×4 matrix, rejecting a non-affine or non-finite bottom row.
    #[must_use]
    pub fn from_rows(rows: [[f64; 4]; 4]) -> Option<Self> {
        let transform = Self { rows };
        transform.is_affine().then_some(transform)
    }

    /// Row-major 4×4 matrix.
    #[must_use]
    pub fn rows(self) -> [[f64; 4]; 4] {
        self.rows
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
        const EPSILON: f64 = 1.0e-9;
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
    pub fn compose(self, right: Self) -> Result<Self, TransformError> {
        let mut rows = [[0.0; 4]; 4];
        for (row, values) in rows.iter_mut().enumerate() {
            for (column, value) in values.iter_mut().enumerate() {
                *value = (0..4)
                    .map(|inner| self.rows[row][inner] * right.rows[inner][column])
                    .sum();
            }
        }
        Self::from_rows(rows).ok_or(TransformError::NonFinite)
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
        let inverse = self.try_inverse_affine().ok()?;
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
    pub fn try_inverse_affine(self) -> Result<Self, TransformError> {
        let m = self.rows;
        let determinant = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        if !determinant.is_finite() {
            return Err(TransformError::NonFinite);
        }
        if determinant == 0.0 {
            return Err(TransformError::Singular);
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
        Self::from_rows(rows).ok_or(TransformError::NonFinite)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_and_inverse_preserve_points_and_vectors() {
        let transform = Transform::from_rows([
            [2.0, 0.5, 0.0, 4.0],
            [0.0, 3.0, 0.0, -2.0],
            [0.0, 0.0, 4.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .expect("affine transform");
        let point = Point3::new(1.0, 2.0, 3.0);
        let vector = Vector3::new(1.0, 2.0, 3.0);
        let inverse = transform
            .try_inverse_affine()
            .expect("invertible affine transform");
        assert_eq!(inverse.apply_point(transform.apply_point(point)), point);
        assert_eq!(Transform::identity().compose(transform), Ok(transform));
        assert_eq!(transform.apply_vector(vector), Vector3::new(3.0, 6.0, 12.0));
    }

    #[test]
    fn finite_transform_composition_rejects_overflow() {
        let transform = Transform::affine([
            [1e200, 0.0, 0.0, 0.0],
            [0.0, 1e200, 0.0, 0.0],
            [0.0, 0.0, 1e200, 0.0],
        ])
        .unwrap();
        assert_eq!(transform.compose(transform), Err(TransformError::NonFinite));
        assert!(Transform::from_rows(transform.rows()).is_some());
    }

    #[test]
    fn finite_transform_inverse_rejects_overflow() {
        let transform = Transform::affine([
            [0.5, 0.0, 0.0, f64::MAX],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
        assert_eq!(
            transform.try_inverse_affine(),
            Err(TransformError::NonFinite)
        );
        assert!(Transform::from_rows(transform.rows()).is_some());
    }

    #[test]
    fn normal_uses_inverse_transpose_and_rejects_singular_transforms() {
        let transform = Transform::from_rows([
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .expect("affine transform");
        assert_eq!(
            transform.apply_normal(Vector3::new(1.0, 1.0, 0.0)),
            Some(Vector3::new(
                1.0 / 5.0_f64.sqrt(),
                2.0 / 5.0_f64.sqrt(),
                0.0
            ))
        );
        let mut rows = transform.rows();
        rows[0][0] = 0.0;
        let singular = Transform::from_rows(rows).expect("affine transform");
        assert_eq!(singular.try_inverse_affine(), Err(TransformError::Singular));
        assert!(singular.apply_normal(Vector3::new(1.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn from_rows_and_deserialize_reject_a_non_affine_or_non_finite_matrix() {
        assert!(Transform::from_rows(Transform::identity().rows()).is_some());
        let mut projective = Transform::identity().rows();
        projective[3][0] = 1.0;
        assert!(Transform::from_rows(projective).is_none());
        let mut nonfinite = Transform::identity().rows();
        nonfinite[0][0] = f64::NAN;
        assert!(Transform::from_rows(nonfinite).is_none());
        assert!(serde_json::from_str::<Transform>(
            "[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[1.0,0.0,0.0,1.0]]"
        )
        .is_err());
        assert!(serde_json::from_str::<Transform>(
            "[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]"
        )
        .is_ok());
        assert!(
            serde_json::from_str::<Transform2>("[[1.0,0.0,0.0],[0.0,1.0,0.0],[1.0,0.0,1.0]]")
                .is_err()
        );
    }
}
