// SPDX-License-Identifier: Apache-2.0
//! Finite affine transforms.

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

// A product of two finite f64 values has an integer significand with at most
// 106 bits. The smallest product exponent is -2148 and the largest is 1942;
// this range leaves 66 words for three exact signed products and their sum.
const EXACT_PRODUCT_EXPONENT: i32 = -2148;
const EXACT_SUM_WORDS: usize = 66;

#[derive(Clone, Copy)]
struct ExactSignedSum {
    positive: [u64; EXACT_SUM_WORDS],
    negative: [u64; EXACT_SUM_WORDS],
}

impl Default for ExactSignedSum {
    fn default() -> Self {
        Self {
            positive: [0; EXACT_SUM_WORDS],
            negative: [0; EXACT_SUM_WORDS],
        }
    }
}

#[derive(Clone, Copy)]
struct ScaledValue {
    sign: f64,
    mantissa: f64,
    exponent: i32,
}

fn finite_significand(value: f64) -> Option<(bool, u64, i32)> {
    if !value.is_finite() {
        return None;
    }
    let bits = value.to_bits();
    let negative = bits >> 63 != 0;
    let exponent = (bits >> 52) & 0x7ff;
    let fraction = bits & ((1_u64 << 52) - 1);
    if exponent == 0 {
        (fraction != 0).then_some((negative, fraction, -1074))
    } else {
        Some((
            negative,
            (1_u64 << 52) | fraction,
            i32::try_from(exponent).expect("f64 exponent fits i32") - 1075,
        ))
    }
}

fn add_word(words: &mut [u64; EXACT_SUM_WORDS], index: usize, value: u64) {
    let (sum, mut carry) = words[index].overflowing_add(value);
    words[index] = sum;
    let mut index = index + 1;
    while carry {
        let (sum, next) = words[index].overflowing_add(1);
        words[index] = sum;
        carry = next;
        index += 1;
    }
}

fn add_shifted(words: &mut [u64; EXACT_SUM_WORDS], value: u128, shift: i32) {
    let shift = usize::try_from(shift).expect("product exponent is in range");
    for (limb_index, limb) in [value as u64, (value >> 64) as u64].into_iter().enumerate() {
        if limb == 0 {
            continue;
        }
        let bit = shift + limb_index * 64;
        let word = bit / 64;
        let remainder = bit % 64;
        if remainder == 0 {
            add_word(words, word, limb);
        } else {
            add_word(words, word, limb << remainder);
            add_word(words, word + 1, limb >> (64 - remainder));
        }
    }
}

impl ExactSignedSum {
    fn add_product(&mut self, left: f64, right: f64) {
        let Some((left_negative, left_significand, left_exponent)) = finite_significand(left)
        else {
            return;
        };
        let Some((right_negative, right_significand, right_exponent)) = finite_significand(right)
        else {
            return;
        };
        let product = u128::from(left_significand) * u128::from(right_significand);
        let exponent = left_exponent + right_exponent;
        let target = if left_negative != right_negative {
            &mut self.negative
        } else {
            &mut self.positive
        };
        add_shifted(target, product, exponent - EXACT_PRODUCT_EXPONENT);
    }

    fn finish(self) -> Option<ScaledValue> {
        let (negative, magnitude) = signed_difference(&self.positive, &self.negative)?;
        let word = magnitude.iter().rposition(|value| *value != 0)?;
        let highest_bit = word * 64
            + usize::try_from(63 - magnitude[word].leading_zeros())
                .expect("u32 bit index fits usize");
        let keep = (highest_bit + 1).min(53);
        let mut significand = 0_u64;
        for bit in (highest_bit + 1 - keep..=highest_bit).rev() {
            significand = (significand << 1) | u64::from(bit_is_set(&magnitude, bit));
        }
        if keep == 53 {
            let guard_bit = highest_bit
                .checked_sub(keep)
                .is_some_and(|bit| bit_is_set(&magnitude, bit));
            let sticky = highest_bit
                .checked_sub(keep)
                .is_some_and(|bit| (0..bit).any(|candidate| bit_is_set(&magnitude, candidate)));
            if guard_bit && (sticky || significand & 1 != 0) {
                significand += 1;
                if significand == 1_u64 << 53 {
                    return Some(ScaledValue {
                        sign: if negative { -1.0 } else { 1.0 },
                        mantissa: 0.5,
                        exponent: EXACT_PRODUCT_EXPONENT
                            + i32::try_from(highest_bit).expect("bit index fits i32")
                            + 2,
                    });
                }
            }
        }
        Some(ScaledValue {
            sign: if negative { -1.0 } else { 1.0 },
            mantissa: significand as f64
                * 2.0_f64.powi(-(i32::try_from(keep).expect("bit count fits i32"))),
            exponent: EXACT_PRODUCT_EXPONENT
                + i32::try_from(highest_bit).expect("bit index fits i32")
                + 1,
        })
    }
}

/// Returns the sign and the magnitude of `positive - negative`, or `None` when
/// the two accumulators are equal.
///
/// The comparison that names the larger accumulator is the only route to the
/// subtraction below, so the minuend is never the smaller side and the loop
/// cannot leave a borrow in the highest word. Splitting the comparison from the
/// subtraction would state that relation as an assertion instead of holding it.
fn signed_difference(
    positive: &[u64; EXACT_SUM_WORDS],
    negative: &[u64; EXACT_SUM_WORDS],
) -> Option<(bool, [u64; EXACT_SUM_WORDS])> {
    let (larger, smaller, is_negative) = positive
        .iter()
        .zip(negative)
        .rev()
        .find_map(|(left, right)| match left.cmp(right) {
            std::cmp::Ordering::Greater => Some((positive, negative, false)),
            std::cmp::Ordering::Less => Some((negative, positive, true)),
            std::cmp::Ordering::Equal => None,
        })?;
    let mut magnitude = [0; EXACT_SUM_WORDS];
    let mut borrow = false;
    for index in 0..EXACT_SUM_WORDS {
        let (difference, first_borrow) = larger[index].overflowing_sub(smaller[index]);
        let (difference, second_borrow) = difference.overflowing_sub(u64::from(borrow));
        magnitude[index] = difference;
        borrow = first_borrow || second_borrow;
    }
    Some((is_negative, magnitude))
}

fn bit_is_set(words: &[u64; EXACT_SUM_WORDS], bit: usize) -> bool {
    words[bit / 64] & (1_u64 << (bit % 64)) != 0
}

fn scaled_finite(value: f64) -> Option<ScaledValue> {
    let (negative, significand, exponent) = finite_significand(value)?;
    let highest_bit = 63 - significand.leading_zeros();
    let bits = i32::try_from(highest_bit + 1).expect("f64 significand fits i32");
    Some(ScaledValue {
        sign: if negative { -1.0 } else { 1.0 },
        mantissa: significand as f64 * 2.0_f64.powi(-bits),
        exponent: exponent + bits,
    })
}

fn fast_dot(coefficients: [f64; 3], components: [f64; 3], products: [f64; 3]) -> Option<f64> {
    if products.iter().any(|product| !product.is_finite()) {
        return None;
    }
    if coefficients.into_iter().zip(components).zip(products).any(
        |((coefficient, component), product)| {
            // A product below the normal range may have rounded before it
            // reached this check, even when it is nonzero. Recompute every
            // such column from the exact significands.
            coefficient != 0.0 && component != 0.0 && product.abs() < f64::MIN_POSITIVE
        },
    ) {
        return None;
    }
    let scale = products
        .iter()
        .fold(0.0_f64, |scale, product| scale.max(product.abs()));
    if scale == 0.0 {
        return Some(0.0);
    }
    let sum = products.into_iter().sum::<f64>();
    if !sum.is_finite() || sum.abs() / scale < f64::EPSILON {
        return None;
    }
    Some(sum)
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
                *value = (0..4)
                    .map(|inner| left[row][inner] * right[inner][column])
                    .sum();
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
        if ![normal.x, normal.y, normal.z]
            .into_iter()
            .all(f64::is_finite)
        {
            return None;
        }
        let inverse = self.inverse_linear().ok()?;
        let components = [normal.x, normal.y, normal.z];
        let values: [Option<ScaledValue>; 3] = std::array::from_fn(|column| {
            let coefficients = [inverse[0][column], inverse[1][column], inverse[2][column]];
            let products = [
                coefficients[0] * components[0],
                coefficients[1] * components[1],
                coefficients[2] * components[2],
            ];
            fast_dot(coefficients, components, products)
                .and_then(scaled_finite)
                .or_else(|| {
                    let mut sum = ExactSignedSum::default();
                    for (coefficient, component) in coefficients.into_iter().zip(components) {
                        sum.add_product(coefficient, component);
                    }
                    sum.finish()
                })
        });
        let scale_exponent = values
            .iter()
            .filter_map(|value| value.map(|value| value.exponent))
            .max()?;
        let scaled = values.map(|value| {
            value.map_or(0.0, |value| {
                value.sign
                    * value.mantissa
                    * 2.0_f64.powi(value.exponent.saturating_sub(scale_exponent))
            })
        });
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
            rows[row][3] = -inverse_linear[row]
                .iter()
                .zip(translation)
                .map(|(coefficient, value)| coefficient * value)
                .sum::<f64>();
        }
        Self::affine(rows).ok_or(TransformError::NonFinite)
    }

    fn inverse_linear(self) -> Result<[[f64; 3]; 3], TransformError> {
        let mut matrix = self.rows.map(|row| [row[0], row[1], row[2]]);
        let mut inverse = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let mut columns = [0, 1, 2];
        // Full pivoting avoids multiplying source coefficients into a determinant.
        for column in 0..3 {
            let mut pivot_row = column;
            let mut pivot_column = column;
            for row in column..3 {
                for candidate in column..3 {
                    if matrix[row][candidate].abs() > matrix[pivot_row][pivot_column].abs() {
                        pivot_row = row;
                        pivot_column = candidate;
                    }
                }
            }
            let pivot = matrix[pivot_row][pivot_column];
            if pivot == 0.0 {
                return Err(TransformError::Singular);
            }
            matrix.swap(column, pivot_row);
            inverse.swap(column, pivot_row);
            for row in &mut matrix {
                row.swap(column, pivot_column);
            }
            columns.swap(column, pivot_column);
            for value in &mut matrix[column] {
                *value /= pivot;
            }
            for value in &mut inverse[column] {
                *value /= pivot;
            }
            for row in 0..3 {
                if row == column {
                    continue;
                }
                let factor = matrix[row][column];
                for entry in 0..3 {
                    matrix[row][entry] =
                        (-factor).mul_add(matrix[column][entry], matrix[row][entry]);
                    inverse[row][entry] =
                        (-factor).mul_add(inverse[column][entry], inverse[row][entry]);
                }
                matrix[row][column] = 0.0;
            }
            if !matrix
                .iter()
                .chain(&inverse)
                .flatten()
                .all(|value| value.is_finite())
            {
                return Err(TransformError::NonFinite);
            }
        }
        let mut rows = [[0.0; 3]; 3];
        for (row, source) in columns.into_iter().zip(inverse) {
            rows[row] = source;
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
