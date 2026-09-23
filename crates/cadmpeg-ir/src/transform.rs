// SPDX-License-Identifier: Apache-2.0
//! Finite affine transforms.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::math::sum::{finite_dot, ExactSignedSum, ScaledValue};
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

/// Whether every coefficient of one affine row is finite.
///
/// The walk is an index loop because a `const fn` cannot drive an iterator.
const fn row_is_finite(row: &[f64]) -> bool {
    let mut index = 0;
    while index < row.len() {
        if !row[index].is_finite() {
            return false;
        }
        index += 1;
    }
    true
}

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
    pub const fn is_finite(&self) -> bool {
        row_is_finite(&self.rows[0]) && row_is_finite(&self.rows[1])
    }

    /// Applies this affine transform to a two-dimensional point.
    pub fn apply_point(self, point: Point2) -> Point2 {
        let components = [point.u, point.v, 1.0];
        let apply = |row: [f64; 3]| {
            finite_dot(row, components)
                .unwrap_or_else(|| row[0] * point.u + row[1] * point.v + row[2])
        };
        Point2::new(apply(self.rows[0]), apply(self.rows[1]))
    }

    /// Applies this transform's linear component to a two-dimensional vector.
    pub fn apply_vector(self, vector: Point2) -> Point2 {
        let components = [vector.u, vector.v];
        let apply = |row: [f64; 3]| {
            finite_dot([row[0], row[1]], components)
                .unwrap_or_else(|| row[0] * vector.u + row[1] * vector.v)
        };
        Point2::new(apply(self.rows[0]), apply(self.rows[1]))
    }
}

/// Add one cyclic cofactor multiplied by a finite scalar. Cyclic row and
/// column order includes the alternating cofactor sign.
fn add_cofactor_product(
    sum: &mut ExactSignedSum,
    matrix: &[[f64; 3]; 3],
    row: usize,
    column: usize,
    factor: f64,
) {
    let r1 = (row + 1) % 3;
    let r2 = (row + 2) % 3;
    let c1 = (column + 1) % 3;
    let c2 = (column + 2) % 3;
    sum.add_factors([matrix[r1][c1], matrix[r2][c2], factor]);
    sum.add_factors([-matrix[r1][c2], matrix[r2][c1], factor]);
}

/// Exact-product determinant of a finite 3×3 linear component.
fn linear_determinant(matrix: &[[f64; 3]; 3]) -> Option<ScaledValue> {
    let [a, b, c] = matrix;
    let mut determinant = ExactSignedSum::default();
    for factors in [
        [a[0], b[1], c[2]],
        [a[1], b[2], c[0]],
        [a[2], b[0], c[1]],
        [-a[0], b[2], c[1]],
        [-a[1], b[0], c[2]],
        [-a[2], b[1], c[0]],
    ] {
        determinant.add_factors(factors);
    }
    determinant.finish()
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
    pub const fn is_finite(&self) -> bool {
        row_is_finite(&self.rows[0]) && row_is_finite(&self.rows[1]) && row_is_finite(&self.rows[2])
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
                *value = finite_dot(left[row], std::array::from_fn(|inner| right[inner][column]))
                    .ok_or(TransformError::NonFinite)?;
            }
        }
        Self::affine(rows).ok_or(TransformError::NonFinite)
    }

    /// Applies this affine transform to a point.
    ///
    /// Finite coefficients and a finite operand still overflow: a translation
    /// near the finite range added to a coordinate near it sums to an infinity.
    /// The result is absent when any coordinate is not finite.
    #[must_use]
    pub fn apply_point(self, point: Point3) -> Option<Point3> {
        let components = [point.x, point.y, point.z, 1.0];
        Some(Point3::new(
            finite_dot(self.rows[0], components)?,
            finite_dot(self.rows[1], components)?,
            finite_dot(self.rows[2], components)?,
        ))
    }

    /// Applies this transform's linear component to a vector.
    ///
    /// The result is absent when any component is not finite, which finite
    /// coefficients and a finite operand still produce by overflow.
    #[must_use]
    pub fn apply_vector(self, vector: Vector3) -> Option<Vector3> {
        let components = [vector.x, vector.y, vector.z];
        let linear = self.rows.map(|row| [row[0], row[1], row[2]]);
        Some(Vector3::new(
            finite_dot(linear[0], components)?,
            finite_dot(linear[1], components)?,
            finite_dot(linear[2], components)?,
        ))
    }

    /// Sign of the linear determinant; absent for a singular map.
    /// Products retain their exponent range until cancellation is complete.
    pub fn orientation(self) -> Option<f64> {
        let matrix = self.rows.map(|row| [row[0], row[1], row[2]]);
        let value = linear_determinant(&matrix)?;
        Some(value.rescale(value.exponent())?.signum())
    }

    /// Applies the inverse-transpose linear transform and normalizes the result.
    pub fn apply_normal(self, normal: Vector3) -> Option<Vector3> {
        if !normal.is_finite() {
            return None;
        }
        let matrix = self.rows.map(|row| [row[0], row[1], row[2]]);
        let determinant = linear_determinant(&matrix)?;
        let orientation = determinant.rescale(determinant.exponent())?.signum();
        let components = [normal.x, normal.y, normal.z];
        let values: [Option<ScaledValue>; 3] = std::array::from_fn(|row| {
            let mut sum = ExactSignedSum::default();
            for (column, component) in components.into_iter().enumerate() {
                add_cofactor_product(&mut sum, &matrix, row, column, component);
            }
            sum.finish()
        });
        let scale_exponent = values
            .iter()
            .filter_map(|value| value.map(|value| value.exponent))
            .max()?;
        let scaled = values
            .map(|value| value.map_or(0.0, |value| orientation * value.scaled_by(scale_exponent)));
        let length = scaled.iter().map(|value| value * value).sum::<f64>().sqrt();
        if !length.is_finite() || length == 0.0 {
            return None;
        }
        Some(Vector3::new(
            scaled[0] / length,
            scaled[1] / length,
            scaled[2] / length,
        ))
    }

    /// Inverts a finite affine transform with a nonsingular linear component.
    pub fn try_inverse_affine(self) -> Result<Self, TransformError> {
        let m = self.rows;
        let inverse_linear = self.inverse_linear()?;
        let translation = [m[0][3], m[1][3], m[2][3]];
        let mut rows = [[0.0; 4]; 3];
        for row in 0..3 {
            rows[row][..3].copy_from_slice(&inverse_linear[row]);
            rows[row][3] =
                -finite_dot(inverse_linear[row], translation).ok_or(TransformError::NonFinite)?;
        }
        Self::affine(rows).ok_or(TransformError::NonFinite)
    }

    fn inverse_linear(self) -> Result<[[f64; 3]; 3], TransformError> {
        let matrix = self.rows.map(|row| [row[0], row[1], row[2]]);
        let determinant = linear_determinant(&matrix).ok_or(TransformError::Singular)?;
        let mut inverse = [[0.0; 3]; 3];
        for (row, entries) in inverse.iter_mut().enumerate() {
            for (column, entry) in entries.iter_mut().enumerate() {
                let mut cofactor = ExactSignedSum::default();
                add_cofactor_product(&mut cofactor, &matrix, column, row, 1.0);
                *entry = cofactor.finish().map_or(Ok(0.0), |value| {
                    value.quotient(determinant).ok_or(TransformError::NonFinite)
                })?;
            }
        }
        Ok(inverse)
    }
}

#[cfg(test)]
mod tests {
    use super::{Transform, Transform2, TransformError};
    use crate::math::{Point3, Vector3};

    mod numeric;

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
        assert_eq!(
            transform
                .apply_point(point)
                .and_then(|placed| inverse.apply_point(placed)),
            Some(point)
        );
        assert_eq!(Transform::identity().compose(transform), Ok(transform));
        assert_eq!(
            transform.apply_vector(vector),
            Some(Vector3::new(3.0, 6.0, 12.0))
        );
    }

    #[test]
    fn finite_transform_application_rejects_overflow() {
        let transform = Transform::affine([
            [1.0, 0.0, 0.0, f64::MAX],
            [0.0, 1e200, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("affine transform");
        assert_eq!(transform.apply_point(Point3::new(f64::MAX, 0.0, 0.0)), None);
        assert_eq!(transform.apply_vector(Vector3::new(0.0, 1e200, 0.0)), None);
        assert_eq!(
            transform.apply_point(Point3::new(0.0, 0.0, 1.0)),
            Some(Point3::new(f64::MAX, 0.0, 1.0))
        );
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
