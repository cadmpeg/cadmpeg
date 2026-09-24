// SPDX-License-Identifier: Apache-2.0
//! Small geometric value types shared by geometry and topology.
//!
//! These are plain data carriers (no invariants enforced at construction); the
//! validation pass is responsible for sanity checks such as "a direction is
//! non-degenerate" where the IR is expected to hold geometry.

use crate::scalar::FiniteReal;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Scaled planar intersections and exact orientation signs.
pub mod planar;

/// Scaled least-squares solvers.
pub mod solve;
pub(crate) mod sum;

/// A point in 3D model space, in the document's length unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Point3 {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Z coordinate.
    pub z: f64,
}

impl From<[f64; 3]> for Point3 {
    fn from([x, y, z]: [f64; 3]) -> Self {
        Point3::new(x, y, z)
    }
}

impl From<Point3> for [f64; 3] {
    fn from(point: Point3) -> Self {
        [point.x, point.y, point.z]
    }
}

impl Point3 {
    /// Construct a point.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Point3 { x, y, z }
    }

    /// Whether every coordinate is finite.
    pub const fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    /// Euclidean distance to another point.
    pub fn distance(self, other: Point3) -> f64 {
        (self.x - other.x)
            .hypot(self.y - other.y)
            .hypot(self.z - other.z)
    }

    /// Squared Euclidean distance to another point (no square root).
    pub fn distance_squared(self, other: Point3) -> f64 {
        let delta = [self.x - other.x, self.y - other.y, self.z - other.z];
        sum::finite_dot(delta, delta).map_or_else(
            || delta[0].powi(2) + delta[1].powi(2) + delta[2].powi(2),
            FiniteReal::get,
        )
    }

    /// Displacement from `origin` to `self`, i.e. `self - origin`.
    pub fn vector_from(self, origin: Point3) -> Vector3 {
        Vector3::new(self.x - origin.x, self.y - origin.y, self.z - origin.z)
    }

    /// Point translated by `scale * vector`.
    #[must_use]
    pub fn translated(self, vector: Vector3, scale: f64) -> Point3 {
        Point3::new(
            scale.mul_add(vector.x, self.x),
            scale.mul_add(vector.y, self.y),
            scale.mul_add(vector.z, self.z),
        )
    }
}

/// A 3D vector. Depending on context this may be a direction (often but not
/// always unit length) or a length-bearing displacement.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Vector3 {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
}

impl From<[f64; 3]> for Vector3 {
    fn from([x, y, z]: [f64; 3]) -> Self {
        Vector3::new(x, y, z)
    }
}

impl From<Vector3> for [f64; 3] {
    fn from(vector: Vector3) -> Self {
        [vector.x, vector.y, vector.z]
    }
}

impl Vector3 {
    /// Construct a vector.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Vector3 { x, y, z }
    }

    /// Whether every component is finite.
    pub const fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    /// Euclidean length.
    pub fn norm(&self) -> f64 {
        self.x.hypot(self.y).hypot(self.z)
    }

    /// Dot product with another vector.
    pub fn dot(self, other: Vector3) -> f64 {
        match sum::finite_dot([self.x, self.y, self.z], [other.x, other.y, other.z]) {
            Some(value) => value.get(),
            None => self.x * other.x + self.y * other.y + self.z * other.z,
        }
    }

    /// Cross product with another vector.
    #[must_use]
    pub fn cross(self, other: Vector3) -> Vector3 {
        let component = |a: f64, b: f64, c: f64, d: f64| match sum::finite_dot([a, -b], [c, d]) {
            Some(value) => value.get(),
            None => a * c - b * d,
        };
        Vector3::new(
            component(self.y, self.z, other.z, other.y),
            component(self.z, self.x, other.x, other.z),
            component(self.x, self.y, other.y, other.x),
        )
    }

    /// Vector scaled by a factor.
    #[must_use]
    pub fn scale(self, factor: f64) -> Vector3 {
        Vector3::new(self.x * factor, self.y * factor, self.z * factor)
    }

    /// Unit vector in the same direction, or `None` when a component is
    /// non-finite or the length is within [`f64::EPSILON`] of zero.
    #[must_use]
    pub fn unit(self) -> Option<Vector3> {
        if self.norm() <= f64::EPSILON {
            return None;
        }
        crate::features::FiniteVector3::new(self)?.unit_nonzero()
    }
}

impl crate::features::FiniteVector3 {
    /// Unit direction for every nonzero vector, including subnormals.
    /// Callers that impose a geometric length threshold must check it separately.
    #[must_use]
    pub fn unit_nonzero(self) -> Option<Vector3> {
        let scale = self.x.abs().max(self.y.abs()).max(self.z.abs());
        if scale == 0.0 {
            return None;
        }
        // Chart by the largest component's power of two so the norm stays in
        // range. A charted subnormal may already be rounded or zero; form that
        // final quotient from its original component instead.
        let exponent = sum::scaled_finite(scale)?.exponent();
        let scaled = Vector3::new(
            scale_power_of_two(self.x, -exponent)?.get(),
            scale_power_of_two(self.y, -exponent)?.get(),
            scale_power_of_two(self.z, -exponent)?.get(),
        );
        let length = scaled.norm();
        let component = |value: f64, chart: f64| {
            if value != 0.0 && chart.abs() < f64::MIN_POSITIVE {
                sum::scaled_finite(value)?
                    .quotient_shifted(sum::scaled_finite(length)?, -exponent)
                    .map(FiniteReal::get)
            } else {
                Some(chart / length)
            }
        };
        Some(Vector3::new(
            component(self.x, scaled.x)?,
            component(self.y, scaled.y)?,
            component(self.z, scaled.z)?,
        ))
    }
}

impl std::ops::Add for Vector3 {
    type Output = Vector3;

    fn add(self, other: Vector3) -> Vector3 {
        Vector3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl std::ops::Sub for Vector3 {
    type Output = Vector3;

    fn sub(self, other: Vector3) -> Vector3 {
        Vector3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

/// A point in 2D surface parameter (u, v) space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Point2 {
    /// U parameter.
    pub u: f64,
    /// V parameter.
    pub v: f64,
}

impl Point2 {
    /// Construct a 2D parameter point.
    pub fn new(u: f64, v: f64) -> Self {
        Point2 { u, v }
    }

    /// Whether every coordinate is finite.
    pub const fn is_finite(&self) -> bool {
        self.u.is_finite() && self.v.is_finite()
    }
}

/// Multiply a finite value by a power of two, retaining representable subnormals.
/// Large exponents are applied in normal chunks. Downward chunks leave 53 bits
/// of exponent headroom so only the final multiplication rounds to subnormal.
/// A chunk that rounds the value to zero ends the downward chunks: the final
/// factor is then finite and positive, so the product keeps the signed zero.
pub fn scale_power_of_two(value: f64, mut exponent: i32) -> Option<FiniteReal> {
    let admitted = FiniteReal::new(value)?;
    if value == 0.0 {
        return Some(admitted);
    }
    let mut value = value;
    while exponent > 1023 {
        value *= 2.0_f64.powi(1023);
        if !value.is_finite() {
            return None;
        }
        exponent -= 1023;
    }
    while exponent < -1022 {
        value *= 2.0_f64.powi(-969);
        if value == 0.0 {
            break;
        }
        exponent += 969;
    }
    FiniteReal::new(value * 2.0_f64.powi(exponent))
}

/// The exponent of the least power of two above a finite nonzero magnitude.
///
/// `value` has magnitude in `[2^(exponent - 1), 2^exponent)`, so
/// `scale_power_of_two(value, -exponent)` lands in `[0.5, 1)` and states the
/// same significand: a normalisation by this exponent moves no bit of the data
/// it normalises. `None` states a zero or non-finite value, which bounds no
/// binade.
pub fn power_of_two_bound(value: f64) -> Option<i32> {
    sum::scaled_finite(value).map(sum::ScaledValue::exponent)
}

/// Compute `left * right / denominator` without intermediate range loss.
/// The denominator must be nonzero and the result finite.
pub fn multiply_divide(
    left: FiniteReal,
    right: FiniteReal,
    denominator: FiniteReal,
) -> Option<FiniteReal> {
    let denominator = sum::scaled_finite(denominator.get())?;
    match sum::product_sum(std::iter::once(Some([left.get(), right.get()]))) {
        sum::ProductSum::Value(numerator) => numerator.quotient(denominator),
        sum::ProductSum::Zero => Some(FiniteReal::ZERO),
        sum::ProductSum::Undefined => None,
    }
}

/// Evaluate a quotient of products with up to four finite factors per side.
/// A zero denominator or a nonfinite result returns `None`. Intermediate
/// products keep their exponent range until the final division.
pub fn product_quotient<const N: usize, const D: usize>(
    numerator: [f64; N],
    denominator: [f64; D],
) -> Option<FiniteReal> {
    if N > 4 || D > 4 {
        return None;
    }
    let sum::ProductSum::Value(denominator) = sum::product_sum(std::iter::once(Some(denominator)))
    else {
        return None;
    };
    match sum::product_sum(std::iter::once(Some(numerator))) {
        sum::ProductSum::Value(value) => value.quotient(denominator),
        sum::ProductSum::Zero => Some(FiniteReal::ZERO),
        sum::ProductSum::Undefined => None,
    }
}

/// The finite products `(scale * sinh(parameter), scale * cosh(parameter))`.
/// Large parameters are exponentiated in thirds before the products are
/// combined; a small scale can then retain otherwise overflowing values.
pub fn scaled_sinh_cosh(scale: f64, parameter: f64) -> Option<(FiniteReal, FiniteReal)> {
    let finite_scale = FiniteReal::new(scale)?;
    if !parameter.is_finite() {
        return None;
    }
    if scale == 0.0 {
        return Some((FiniteReal::ZERO, FiniteReal::ZERO));
    }
    let sinh = parameter.sinh();
    if let Some(cosh) = FiniteReal::new(parameter.cosh()) {
        // The sinh magnitude stays below cosh, but each is its own rounded
        // library result, so the sinh is admitted where it is read.
        return Some((
            multiply_divide(finite_scale, FiniteReal::new(sinh)?, FiniteReal::ONE)?,
            multiply_divide(finite_scale, cosh, FiniteReal::ONE)?,
        ));
    }
    let third = parameter.abs() / 3.0;
    let exponential = third.exp();
    let tail = (parameter.abs() - third - third).exp();
    let magnitude = product_quotient([scale, exponential, exponential, tail], [2.0])?;
    // At this parameter magnitude, the omitted exp(-abs(parameter)) term
    // is too small to change either rounded product for any finite scale.
    // The sign of a finite parameter selects the negation, which is the
    // product with its signum bit for bit.
    let signed = if parameter.is_sign_negative() {
        magnitude.negated()
    } else {
        magnitude
    };
    Some((signed, magnitude))
}

/// Interpolate between finite endpoints at a fraction in `[0, 1]`.
/// The weighted sum does not form an overflowing endpoint difference.
pub fn interpolate(start: f64, end: f64, fraction: f64) -> Option<FiniteReal> {
    if !(0.0..=1.0).contains(&fraction) {
        return None;
    }
    sum::finite_dot([1.0 - fraction, fraction], [start, end])
}

/// The finite fraction `(parameter - start) / (end - start)`.
/// Exact differences retain finite quotients across overflowing widths. Fractions
/// outside `[0, 1]` are retained for exterior knots of unclamped curves.
pub fn parameter_fraction(parameter: f64, start: f64, end: f64) -> Option<FiniteReal> {
    if ![parameter, start, end].into_iter().all(f64::is_finite) || start == end {
        return None;
    }
    let numerator = parameter - start;
    let denominator = end - start;
    if numerator.is_finite() && denominator.is_finite() {
        if let Some(fraction) = FiniteReal::new(numerator / denominator) {
            return Some(fraction);
        }
    }
    let mut numerator = sum::ExactSignedSum::default();
    numerator.add_product(parameter, 1.0);
    numerator.add_product(start, -1.0);
    let mut denominator = sum::ExactSignedSum::default();
    denominator.add_product(end, 1.0);
    denominator.add_product(start, -1.0);
    let denominator = denominator.finish()?;
    numerator
        .finish()
        .map_or(Some(FiniteReal::ZERO), |value| value.quotient(denominator))
}

/// Whether a finite parameter lies in an ordered domain, allowing roundoff
/// relative to its width. No absolute parameter-unit floor is imposed.
pub fn parameter_in_domain(value: f64, domain: [f64; 2], relative_tolerance: f64) -> bool {
    if ![value, domain[0], domain[1], relative_tolerance]
        .into_iter()
        .all(f64::is_finite)
        || domain[0] > domain[1]
        || relative_tolerance < 0.0
    {
        return false;
    }
    if (domain[0]..=domain[1]).contains(&value) {
        return true;
    }
    parameter_fraction(value, domain[0], domain[1]).is_some_and(|fraction| {
        fraction.get() >= -relative_tolerance && fraction.get() - 1.0 <= relative_tolerance
    })
}

/// Map a finite parameter into the half-open finite interval `[start, end)`.
/// A period wider than f64's range is evaluated in a half-scale chart.
pub fn wrap_parameter(parameter: f64, start: f64, end: f64) -> Option<FiniteReal> {
    let [Some(admitted_parameter), Some(admitted_start), Some(_)] =
        [parameter, start, end].map(FiniteReal::new)
    else {
        return None;
    };
    if start >= end {
        return None;
    }
    if (start..end).contains(&parameter) {
        return Some(admitted_parameter);
    }
    if parameter == end {
        return Some(admitted_start);
    }
    let period = end - start;
    if !period.is_finite() {
        return multiply_divide(
            wrap_parameter(parameter * 0.5, start * 0.5, end * 0.5)?,
            FiniteReal::TWO,
            FiniteReal::ONE,
        );
    }
    let relative = parameter - start;
    let offset = if relative.is_finite() {
        relative.rem_euclid(period)
    } else {
        (parameter.rem_euclid(period) - start.rem_euclid(period)).rem_euclid(period)
    };
    FiniteReal::new(start + offset).map(|wrapped| {
        if wrapped.get() >= end {
            admitted_start
        } else {
            wrapped
        }
    })
}

/// Reflect a parameter about the midpoint of two bounds.
/// The exact sum avoids overflow and cancellation in `start + end - parameter`.
/// Returns `None` when the reflected result is non-finite.
pub fn reflect_parameter(
    parameter: FiniteReal,
    start: FiniteReal,
    end: FiniteReal,
) -> Option<FiniteReal> {
    let mut sum = sum::ExactSignedSum::default();
    sum.add_product(start.get(), 1.0);
    sum.add_product(end.get(), 1.0);
    sum.add_product(parameter.get(), -1.0);
    sum.finish()
        .map_or(Some(FiniteReal::ZERO), sum::ScaledValue::finite)
}

#[cfg(test)]
mod tests;
