// SPDX-License-Identifier: Apache-2.0
//! Finite affine transforms.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::features::{FinitePoint3, FiniteVector3};
use crate::math::sum::{finite_dot, ExactSignedSum, ScaledValue};
use crate::math::{Point2, Point3, Vector3};
use crate::scalar::{FiniteReal, PositiveReal};
use crate::units::UnitVector3;

/// A row-major affine transform applied to two-dimensional geometry.
///
/// The two stored rows preserve the source coefficients. The bottom row
/// `[0, 0, 1]` is a fact of the type: it is produced by [`Self::rows`] and is
/// neither carried nor compared.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct Transform2 {
    rows: [[f64; 3]; 2],
}

const BOTTOM_ROW_2: [f64; 3] = [0.0, 0.0, 1.0];
const BOTTOM_ROW_4: [f64; 4] = [0.0, 0.0, 0.0, 1.0];

impl<'de> Deserialize<'de> for Transform2 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let rows = <[[f64; 3]; 2]>::deserialize(deserializer)?;
        Self::affine(rows)
            .ok_or_else(|| serde::de::Error::custom("transform2 coefficients must be finite"))
    }
}

impl<'de> Deserialize<'de> for Transform {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let rows = <[[f64; 4]; 3]>::deserialize(deserializer)?;
        Self::affine(rows)
            .ok_or_else(|| serde::de::Error::custom("transform coefficients must be finite"))
    }
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
        rows.iter()
            .flatten()
            .all(|value| value.is_finite())
            .then_some(Self { rows })
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

    /// Applies this affine transform to a two-dimensional point.
    pub fn apply_point(self, point: Point2) -> Point2 {
        let components = [point.u, point.v, 1.0];
        let apply = |row: [f64; 3]| {
            // A coordinate that is not finite is the plain
            // `row[0] * u + row[1] * v + row[2]`.
            finite_dot(row, components).map_or_else(|plain| plain, FiniteReal::get)
        };
        Point2::new(apply(self.rows[0]), apply(self.rows[1]))
    }

    /// Applies this transform's linear component to a two-dimensional vector.
    pub fn apply_vector(self, vector: Point2) -> Point2 {
        let components = [vector.u, vector.v];
        let apply = |row: [f64; 3]| {
            // A component that is not finite is the plain
            // `row[0] * u + row[1] * v`.
            finite_dot([row[0], row[1]], components).map_or_else(|plain| plain, FiniteReal::get)
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct Transform {
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
        rows.iter()
            .flatten()
            .all(|value| value.is_finite())
            .then_some(Self { rows })
    }

    /// Build an affine transform from finite coefficients. Every coefficient
    /// is finite, so the rows keep the admission of [`Self::affine`] without
    /// a check.
    pub(crate) fn from_finite_rows(rows: [[FiniteReal; 4]; 3]) -> Self {
        Self {
            rows: rows.map(|row| row.map(FiniteReal::get)),
        }
    }

    /// Replace the translation and keep the linear rows. The linear rows are
    /// held finite and every translation component is finite, so the result
    /// keeps the admission of [`Self::affine`] without a check.
    #[must_use]
    pub fn with_translation(self, translation: FiniteVector3) -> Self {
        let mut rows = self.rows;
        for (row, component) in rows.iter_mut().zip(translation.components()) {
            row[3] = component.get();
        }
        Self { rows }
    }

    /// Multiply only the translation by a positive length-unit scale.
    /// The admitted linear rows are kept. A translation overflow is refused.
    #[must_use]
    pub fn scaled_translation(self, scale: PositiveReal) -> Option<Self> {
        let [x, y, z] = self.rows.map(|row| row[3] * scale.get());
        let translation = FiniteVector3::new(Vector3::new(x, y, z))?;
        Some(self.with_translation(translation))
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

    /// Whether this is a finite, right-handed rigid transform.
    pub fn is_proper_rigid(&self) -> bool {
        const EPSILON: f64 = 1.0e-9;
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
        let mut rows = [[FiniteReal::ZERO; 4]; 3];
        for (row, values) in rows.iter_mut().enumerate() {
            for (column, value) in values.iter_mut().enumerate() {
                *value = finite_dot(left[row], std::array::from_fn(|inner| right[inner][column]))
                    .map_err(|_| TransformError::NonFinite)?;
            }
        }
        Ok(Self::from_finite_rows(rows))
    }

    /// Applies this affine transform to a point.
    ///
    /// Finite coefficients and a finite operand still overflow: a translation
    /// near the finite range added to a coordinate near it sums to an infinity.
    /// The result is absent when any coordinate is not finite.
    #[must_use]
    pub fn apply_point(self, point: Point3) -> Option<FinitePoint3> {
        self.apply_point_reaching(point).ok()
    }

    /// Applies this affine transform to a point, or gives the point it
    /// reaches when a coordinate is not finite. Such a coordinate is the
    /// plain sum of its row's products.
    pub(crate) fn apply_point_reaching(self, point: Point3) -> Result<FinitePoint3, Point3> {
        let components = [point.x, point.y, point.z, 1.0];
        match self.rows.map(|row| finite_dot(row, components)) {
            [Ok(x), Ok(y), Ok(z)] => Ok(FinitePoint3::from_coordinates(x, y, z)),
            coordinates => {
                let [x, y, z] = coordinates
                    .map(|coordinate| coordinate.map_or_else(|plain| plain, FiniteReal::get));
                Err(Point3::new(x, y, z))
            }
        }
    }

    /// Applies this transform's linear component to a vector.
    ///
    /// The result is absent when any component is not finite, which finite
    /// coefficients and a finite operand still produce by overflow.
    #[must_use]
    pub fn apply_vector(self, vector: Vector3) -> Option<FiniteVector3> {
        let components = [vector.x, vector.y, vector.z];
        let linear = self.rows.map(|row| [row[0], row[1], row[2]]);
        Some(FiniteVector3::from_components(
            finite_dot(linear[0], components).ok()?,
            finite_dot(linear[1], components).ok()?,
            finite_dot(linear[2], components).ok()?,
        ))
    }

    /// Sign of the linear determinant; absent for a singular map.
    /// Products retain their exponent range until cancellation is complete.
    pub fn orientation(self) -> Option<f64> {
        let matrix = self.rows.map(|row| [row[0], row[1], row[2]]);
        let value = linear_determinant(&matrix)?;
        Some(value.rescale(value.exponent())?.get().signum())
    }

    /// Applies the inverse-transpose linear transform and normalizes the result.
    pub fn apply_normal(self, normal: Vector3) -> Option<UnitVector3> {
        if !normal.is_finite() {
            return None;
        }
        self.apply_finite_normal(normal)
    }

    /// Applies the inverse-transpose linear transform to an admitted unit
    /// normal and normalizes the result.
    pub fn apply_unit_normal(self, normal: UnitVector3) -> Option<UnitVector3> {
        self.apply_finite_normal(*normal.as_raw())
    }

    /// The inverse-transpose map of a normal with finite components.
    fn apply_finite_normal(self, normal: Vector3) -> Option<UnitVector3> {
        let matrix = self.rows.map(|row| [row[0], row[1], row[2]]);
        let determinant = linear_determinant(&matrix)?;
        let orientation = determinant.rescale(determinant.exponent())?.get().signum();
        let components = [normal.x, normal.y, normal.z];
        let values: [Option<ScaledValue>; 3] = std::array::from_fn(|row| {
            let mut sum = ExactSignedSum::default();
            for (column, component) in components.into_iter().enumerate() {
                add_cofactor_product(&mut sum, &matrix, row, column, component);
            }
            sum.finish()
        });
        UnitVector3::from_exact_sums(values, orientation < 0.0)
    }

    /// Inverts a finite affine transform with a nonsingular linear component.
    pub fn try_inverse_affine(self) -> Result<Self, TransformError> {
        let m = self.rows;
        let inverse_linear = self.inverse_linear()?;
        let translation = [m[0][3], m[1][3], m[2][3]];
        let mut rows = [[FiniteReal::ZERO; 4]; 3];
        for row in 0..3 {
            rows[row][..3].copy_from_slice(&inverse_linear[row]);
            rows[row][3] = finite_dot(inverse_linear[row].map(FiniteReal::get), translation)
                .map_err(|_| TransformError::NonFinite)?
                .negated();
        }
        Ok(Self::from_finite_rows(rows))
    }

    fn inverse_linear(self) -> Result<[[FiniteReal; 3]; 3], TransformError> {
        let matrix = self.rows.map(|row| [row[0], row[1], row[2]]);
        let determinant = linear_determinant(&matrix).ok_or(TransformError::Singular)?;
        let mut inverse = [[FiniteReal::ZERO; 3]; 3];
        for (row, entries) in inverse.iter_mut().enumerate() {
            for (column, entry) in entries.iter_mut().enumerate() {
                let mut cofactor = ExactSignedSum::default();
                add_cofactor_product(&mut cofactor, &matrix, column, row, 1.0);
                *entry = cofactor.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                    value
                        .quotient(determinant)
                        .map_err(|_| TransformError::NonFinite)
                })?;
            }
        }
        Ok(inverse)
    }
}

#[cfg(test)]
mod tests {
    use super::{PositiveReal, Transform, Transform2, TransformError};
    use crate::features::{FinitePoint3, FiniteVector3};
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
                .and_then(|placed| inverse.apply_point(placed.get()))
                .map(FinitePoint3::get),
            Some(point)
        );
        assert_eq!(Transform::identity().compose(transform), Ok(transform));
        assert_eq!(
            transform.apply_vector(vector).map(FiniteVector3::get),
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
            transform
                .apply_point(Point3::new(0.0, 0.0, 1.0))
                .map(FinitePoint3::get),
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
            transform
                .apply_normal(Vector3::new(1.0, 1.0, 0.0))
                .map(Vector3::from),
            Some(Vector3::new(
                1.0 / 5.0_f64.sqrt(),
                2.0 / 5.0_f64.sqrt(),
                0.0
            ))
        );
        assert_eq!(
            transform
                .apply_unit_normal(crate::units::UnitVector3::X_AXIS)
                .map(Vector3::from),
            transform
                .apply_normal(Vector3::new(1.0, 0.0, 0.0))
                .map(Vector3::from)
        );
        let mut rows = transform.affine_rows();
        rows[0][0] = 0.0;
        let singular = Transform::affine(rows).expect("affine transform");
        assert_eq!(singular.try_inverse_affine(), Err(TransformError::Singular));
        assert!(singular.apply_normal(Vector3::new(1.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn a_unit_normal_maps_to_an_admitted_unit_normal() {
        use crate::units::UnitVector3;
        let transform = Transform::affine([
            [1.0e-300, 0.0, 0.0, 0.0],
            [0.0, 1.0e300, 0.0, 0.0],
            [0.3, 0.2, 7.0, 0.0],
        ])
        .expect("affine transform");
        for normal in [
            UnitVector3::X_AXIS,
            UnitVector3::Y_AXIS,
            UnitVector3::Z_AXIS,
            UnitVector3::new(Vector3::new(0.6, 0.8, 0.0)).expect("unit normal"),
        ] {
            let mapped = transform
                .apply_unit_normal(normal)
                .expect("nonsingular linear component");
            assert_eq!(UnitVector3::new(*mapped.as_raw()), Some(mapped));
            assert_eq!(
                transform.apply_normal(*normal.as_raw()).map(Vector3::from),
                Some(*mapped.as_raw())
            );
        }
    }

    #[test]
    fn deserialization_admits_rows_through_the_affine_constructor() {
        use serde::de::value::Error;
        use serde::de::IntoDeserializer;
        use serde::Deserialize;

        fn from_rows<T: for<'de> Deserialize<'de>>(rows: Vec<Vec<f64>>) -> Result<T, Error> {
            T::deserialize(rows.into_deserializer())
        }
        let rows = |value: f64| {
            vec![
                vec![1.0, 0.0, 0.0, value],
                vec![0.0, 1.0, 0.0, 0.0],
                vec![0.0, 0.0, 1.0, 0.0],
            ]
        };
        assert_eq!(
            from_rows::<Transform>(rows(2.0)).ok(),
            Transform::affine([
                [1.0, 0.0, 0.0, 2.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
        );
        for value in [f64::NAN, f64::INFINITY] {
            let error = from_rows::<Transform>(rows(value)).expect_err("non-finite coefficient");
            assert_eq!(error.to_string(), "transform coefficients must be finite");
        }

        let rows2 = |value: f64| vec![vec![1.0, 0.0, value], vec![0.0, 1.0, 0.0]];
        assert_eq!(
            from_rows::<Transform2>(rows2(2.0)).ok(),
            Transform2::affine([[1.0, 0.0, 2.0], [0.0, 1.0, 0.0]])
        );
        for value in [f64::NAN, f64::NEG_INFINITY] {
            let error = from_rows::<Transform2>(rows2(value)).expect_err("non-finite coefficient");
            assert_eq!(error.to_string(), "transform2 coefficients must be finite");
        }
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
        let mut nonfinite2 = Transform2::identity().affine_rows();
        nonfinite2[1][2] = f64::INFINITY;
        assert!(Transform2::affine(nonfinite2).is_none());
        let transform2 = serde_json::from_str::<Transform2>("[[1.0,0.0,0.0],[0.0,1.0,0.0]]")
            .expect("two affine rows");
        assert_eq!(transform2, Transform2::identity());
        assert_eq!(transform2.rows()[2], [0.0, 0.0, 1.0]);
    }

    #[test]
    fn scaling_translation_keeps_admitted_linear_rows() {
        let transform = Transform::affine([
            [0.0, -1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, -2.0],
            [0.0, 0.0, 1.0, 3.0],
        ])
        .expect("finite rows");
        let scale = PositiveReal::new(25.4).expect("positive scale");
        let scaled = transform.scaled_translation(scale).expect("finite product");
        assert_eq!(scaled.affine_rows()[0], [0.0, -1.0, 0.0, 25.4]);
        assert_eq!(scaled.affine_rows()[1], [1.0, 0.0, 0.0, -50.8]);
        assert_eq!(scaled.affine_rows()[2], [0.0, 0.0, 1.0, 3.0 * 25.4]);

        let overflow = Transform::affine([
            [1.0, 0.0, 0.0, f64::MAX],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("finite rows");
        assert_eq!(overflow.scaled_translation(scale), None);
    }
}
