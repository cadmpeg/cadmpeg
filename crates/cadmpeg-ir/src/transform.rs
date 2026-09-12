// SPDX-License-Identifier: Apache-2.0
//! Rigid transforms.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::math::{Point2, Point3, Vector3};

/// A row-major affine transform applied to two-dimensional geometry.
///
/// The two stored rows preserve the source coefficients. The bottom row
/// `[0, 0, 1]` is a fact of the type: it is produced by [`Self::rows`] and is
/// neither carried nor compared.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct Transform2 {
    #[serde(deserialize_with = "deserialize_finite2")]
    rows: [[f64; 3]; 2],
}

const BOTTOM_ROW_2: [f64; 3] = [0.0, 0.0, 1.0];
const BOTTOM_ROW_4: [f64; 4] = [0.0, 0.0, 0.0, 1.0];

fn deserialize_finite2<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<[[f64; 3]; 2], D::Error> {
    let rows = <[[f64; 3]; 2]>::deserialize(deserializer)?;
    rows.iter()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(rows)
        .ok_or_else(|| serde::de::Error::custom("transform2 coefficients must be finite"))
}

fn deserialize_finite4<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<[[f64; 4]; 3], D::Error> {
    let rows = <[[f64; 4]; 3]>::deserialize(deserializer)?;
    rows.iter()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(rows)
        .ok_or_else(|| serde::de::Error::custom("transform coefficients must be finite"))
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
            rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        }
    }

    /// Build an affine transform from the two linear rows and translation.
    #[must_use]
    pub fn affine(rows: [[f64; 3]; 2]) -> Option<Self> {
        let transform = Self { rows };
        transform.is_finite().then_some(transform)
    }

    /// The affine rows, without the constant bottom row.
    #[must_use]
    pub fn affine_rows(self) -> [[f64; 3]; 2] {
        self.rows
    }

    /// Row-major 3×3 matrix, with the constant bottom row.
    #[must_use]
    pub fn rows(self) -> [[f64; 3]; 3] {
        [self.rows[0], self.rows[1], BOTTOM_ROW_2]
    }

    /// Whether every matrix coefficient is finite.
    pub fn is_finite(&self) -> bool {
        self.rows.iter().flatten().all(|value| value.is_finite())
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

/// A row-major affine transform applied to a body's geometry.
///
/// The three stored rows preserve the source coefficients. The bottom row
/// `[0, 0, 0, 1]` is a fact of the type: it is produced by [`Self::rows`] and
/// is neither carried nor compared.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct Transform {
    #[serde(deserialize_with = "deserialize_finite4")]
    rows: [[f64; 4]; 3],
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
            ],
        }
    }

    /// Build an affine transform from the three linear rows and translation.
    #[must_use]
    pub fn affine(rows: [[f64; 4]; 3]) -> Option<Self> {
        let transform = Self { rows };
        transform.is_finite().then_some(transform)
    }

    /// The affine rows, without the constant bottom row.
    #[must_use]
    pub fn affine_rows(self) -> [[f64; 4]; 3] {
        self.rows
    }

    /// Row-major 4×4 matrix, with the constant bottom row.
    #[must_use]
    pub fn rows(self) -> [[f64; 4]; 4] {
        [self.rows[0], self.rows[1], self.rows[2], BOTTOM_ROW_4]
    }

    /// Whether every matrix coefficient is finite.
    pub fn is_finite(&self) -> bool {
        self.rows.iter().flatten().all(|value| value.is_finite())
    }

    /// Whether this is a finite, right-handed rigid transform.
    pub fn is_proper_rigid(&self) -> bool {
        const EPSILON: f64 = 1.0e-9;
        if !self.is_finite() {
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
        let left = self.rows();
        let right = right.rows();
        let mut rows = [[0.0; 4]; 3];
        for (row, values) in rows.iter_mut().enumerate() {
            for (column, value) in values.iter_mut().enumerate() {
                *value = (0..4).map(|inner| left[row][inner] * right[inner][column]).sum();
            }
        }
        Self::affine(rows).ok_or(TransformError::NonFinite)
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
        let mut rows = [[0.0; 4]; 3];
        for row in 0..3 {
            rows[row][..3].copy_from_slice(&inverse_linear[row]);
            rows[row][3] = -inverse_linear[row]
                .iter()
                .zip(translation)
                .map(|(coefficient, value)| coefficient * value)
                .sum::<f64>();
        }
        Self::affine(rows).ok_or(TransformError::NonFinite)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_and_inverse_preserve_points_and_vectors() {
        let transform = Transform::affine([
            [2.0, 0.5, 0.0, 4.0],
            [0.0, 3.0, 0.0, -2.0],
            [0.0, 0.0, 4.0, 1.0],
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
        assert!(Transform::affine(transform.affine_rows()).is_some());
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
        assert!(Transform::affine(transform.affine_rows()).is_some());
    }

    #[test]
    fn normal_uses_inverse_transpose_and_rejects_singular_transforms() {
        let transform = Transform::affine([
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
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
        let mut rows = transform.affine_rows();
        rows[0][0] = 0.0;
        let singular = Transform::affine(rows).expect("affine transform");
        assert_eq!(singular.try_inverse_affine(), Err(TransformError::Singular));
        assert!(singular.apply_normal(Vector3::new(1.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn a_transform_carries_only_its_affine_rows() {
        assert!(Transform::affine(Transform::identity().affine_rows()).is_some());
        let mut nonfinite = Transform::identity().affine_rows();
        nonfinite[0][0] = f64::NAN;
        assert!(Transform::affine(nonfinite).is_none());

        // The wire is the affine rows. A fourth row has no spelling, so there
        // is no bottom row to compare and no "is not affine" refusal.
        assert!(serde_json::from_str::<Transform>(
            "[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]"
        )
        .is_err());
        let transform = serde_json::from_str::<Transform>(
            "[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0]]",
        )
        .expect("three affine rows");
        assert_eq!(transform, Transform::identity());
        assert_eq!(transform.rows()[3], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            serde_json::to_string(&transform).expect("serializes"),
            "[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0]]"
        );

        assert!(
            serde_json::from_str::<Transform2>("[[1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]]")
                .is_err()
        );
        let transform2 = serde_json::from_str::<Transform2>("[[1.0,0.0,0.0],[0.0,1.0,0.0]]")
            .expect("two affine rows");
        assert_eq!(transform2, Transform2::identity());
        assert_eq!(transform2.rows()[2], [0.0, 0.0, 1.0]);
    }
}
