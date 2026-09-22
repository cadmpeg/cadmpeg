// SPDX-License-Identifier: Apache-2.0
//! Real quadratic roots over coefficients that carry the magnitudes of the
//! terms they were formed from, which is what states the degree of the problem
//! and the sign of its discriminant.

use cadmpeg_ir::math::{multiply_divide, power_of_two_bound, scale_power_of_two};

/// The relative band inside which a sum of products states zero.
///
/// A sum of n products in f64 carries one rounding per product and one per
/// addition, and each product whose factors are themselves read or computed
/// values carries one further rounding, so the computed value differs from the
/// exact sum of the exact products by at most `(3n - 1) * u * terms`, where
/// `u = f64::EPSILON / 2` is the unit roundoff and `terms` is the sum of the
/// magnitudes of the products. This constant is `128 * u`, which covers
/// `n = 43`.
///
/// The longest sum a caller forms is the constant term of a quadric restricted
/// to a line or to a plane: three products against the matrix-vector product,
/// three against the linear part, and the quadric constant, which is thirteen
/// products once each matrix-vector entry's own three-product dot is counted.
/// That is `38 * u`, under a third of the band. The Bezier plane-distance
/// differences of a cubic extrusion reach four products and the section
/// equal-length constant four, well inside it.
const EPS_QUADRATIC_CANCELLATION: f64 = 64.0 * f64::EPSILON;

/// The bound on the difference between a sum of products computed in f64 and
/// the exact sum of the exact products.
///
/// `terms` is the sum of the magnitudes of those products. Callers that form a
/// value from several such sums, or from products of them, state their own
/// multiple of this bound beside the operation count that earns it.
pub(super) fn cancellation_bound(terms: f64) -> f64 {
    EPS_QUADRATIC_CANCELLATION * terms.abs()
}

/// A coefficient of a quadratic root problem, with the sum of the magnitudes of
/// the products that formed it.
///
/// `terms` bounds the coefficient's own error at `EPS_QUADRATIC_CANCELLATION *
/// terms`. That bound is what lets `real_roots` state the degree and the sign
/// of the discriminant from the arithmetic that produced the coefficients
/// rather than from the coefficient values alone, which say nothing about how
/// much cancellation they carry.
#[derive(Clone, Copy)]
pub(super) struct Coefficient {
    value: f64,
    terms: f64,
}

impl Coefficient {
    /// A coefficient formed as a sum of products, where `terms` is the sum of
    /// the magnitudes of those products.
    ///
    /// A sum that cancels to within the rounding error of its terms states
    /// zero. A zero quadratic coefficient states that the problem is linear, so
    /// `real_roots` takes the lower-degree branch. A zero constant states that
    /// the origin of the restriction satisfies the equation.
    pub(super) fn summed(value: f64, terms: f64) -> Self {
        let terms = terms.abs();
        if value.abs() <= cancellation_bound(terms) {
            return Self { value: 0.0, terms };
        }
        Self { value, terms }
    }

    /// A coefficient that is one value rather than a sum of several, so its own
    /// magnitude is its only term and it states zero only when it is zero.
    pub(super) fn single(value: f64) -> Self {
        Self {
            value,
            terms: value.abs(),
        }
    }

    /// The value the coefficient states.
    pub(super) const fn stated(self) -> f64 {
        self.value
    }

    /// The sum of the magnitudes of the products that formed the coefficient.
    pub(super) const fn terms(self) -> f64 {
        self.terms
    }
}

/// Return finite real roots in ascending order.
///
/// The discriminant is read against the error its coefficients carry, not
/// against zero. Each coefficient is within `EPS_QUADRATIC_CANCELLATION *
/// terms` of the exact one, so a discriminant inside the band that propagates
/// from those three error bars states a repeated root, and only a discriminant
/// below the band states that the equation has no real root.
///
/// A power-of-two change of variable balances the quadratic and constant
/// coefficients before common scaling. This preserves finite roots when the
/// original coefficients span more than one f64 exponent range.
pub(super) fn real_roots(
    quadratic: Coefficient,
    linear: Coefficient,
    constant: Coefficient,
) -> Vec<f64> {
    if [quadratic, linear, constant]
        .iter()
        .any(|coefficient| !coefficient.value.is_finite() || !coefficient.terms.is_finite())
    {
        return Vec::new();
    }
    if quadratic.value == 0.0 {
        let root = -constant.value / linear.value;
        return root.is_finite().then_some(root).into_iter().collect();
    }
    let exponent = |value: f64| power_of_two_bound(value).unwrap_or(0);
    let variable_exponent = if constant.value != 0.0 {
        (exponent(constant.value) - exponent(quadratic.value)).div_euclid(2)
    } else if linear.value != 0.0 {
        exponent(linear.value) - exponent(quadratic.value)
    } else {
        0
    };
    let coefficients = [quadratic, linear, constant];
    let shifts = [2 * variable_exponent, variable_exponent, 0];
    let scale_exponent = coefficients
        .iter()
        .zip(shifts)
        .filter_map(|(coefficient, shift)| {
            power_of_two_bound(coefficient.value.abs().max(coefficient.terms)).map(|e| e + shift)
        })
        .max()
        .unwrap_or(0);
    let mut values = [0.0; 3];
    let mut errors = [0.0; 3];
    for (index, (coefficient, shift)) in coefficients.into_iter().zip(shifts).enumerate() {
        let Some(value) = scale_power_of_two(coefficient.value, shift - scale_exponent) else {
            return Vec::new();
        };
        let Some(terms) = scale_power_of_two(coefficient.terms, shift - scale_exponent) else {
            return Vec::new();
        };
        values[index] = value;
        errors[index] = EPS_QUADRATIC_CANCELLATION * terms;
    }
    let [a, b, c] = values;
    let product = 4.0 * a * c;
    let discriminant = b.mul_add(b, -product);
    // With `e_x` the error bar of each scaled coefficient, `|b^2 - exact b^2|`
    // is at most `(2 |b| + e_b) e_b` and `|4ac - exact 4ac|` is at most
    // `4 (|c| e_a + |a| e_c + e_a e_c)`. The last summand is the rounding of
    // the two multiplications and the fused add that form the discriminant
    // here, which is two products and one sum.
    let [error_quadratic, error_linear, error_constant] = errors;
    let error = (2.0 * b.abs() + error_linear) * error_linear
        + 4.0
            * (c.abs() * error_quadratic
                + a.abs() * error_constant
                + error_quadratic * error_constant)
        + EPS_QUADRATIC_CANCELLATION * (b * b + product.abs());
    if discriminant.abs() <= error {
        return multiply_divide(-linear.value, 0.5, quadratic.value)
            .into_iter()
            .collect();
    }
    if discriminant < 0.0 {
        return Vec::new();
    }
    let root = discriminant.sqrt();
    let q = -0.5 * (b + root.copysign(b));
    let scaled_quotient = |numerator: f64, denominator: f64, shift: i32| {
        let denominator_exponent = power_of_two_bound(denominator)?;
        if numerator == 0.0 {
            return Some(0.0);
        }
        let numerator_exponent = power_of_two_bound(numerator)?;
        let ratio = scale_power_of_two(numerator, -numerator_exponent)?
            / scale_power_of_two(denominator, -denominator_exponent)?;
        scale_power_of_two(ratio, numerator_exponent - denominator_exponent + shift)
    };
    // q is in the scaled variable's chart. Combine the chart exponent with
    // each original coefficient before division can overflow or underflow.
    // Original coefficients also retain bits lost by subnormal common scaling.
    let mut roots = [
        scaled_quotient(q, quadratic.value, scale_exponent - variable_exponent),
        scaled_quotient(constant.value, q, variable_exponent - scale_exponent),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    roots.sort_by(f64::total_cmp);
    roots.dedup();
    roots
}

#[cfg(test)]
mod tests {
    use super::Coefficient;

    #[test]
    fn numerical_followup_quadratic_roots_ignore_common_coefficient_scale() {
        for scale in [1.0, 1e-20, 1e-200, 1e200, 1e308] {
            assert_eq!(
                super::real_roots(
                    Coefficient::single(scale),
                    Coefficient::single(0.0),
                    Coefficient::single(-scale)
                ),
                [-1.0, 1.0]
            );
            assert!(super::real_roots(
                Coefficient::single(scale),
                Coefficient::single(0.0),
                Coefficient::single(scale)
            )
            .is_empty());
            assert_eq!(
                super::real_roots(
                    Coefficient::single(0.0),
                    Coefficient::single(scale),
                    Coefficient::single(-scale)
                ),
                [1.0]
            );
        }
        let roots = super::real_roots(
            Coefficient::single(1.0),
            Coefficient::single(-1e16),
            Coefficient::single(1.0),
        );
        assert_eq!(roots, [1e-16, 1e16]);
    }
    #[test]
    fn numerical_0922b_finite_quadratic_roots() {
        for s in [1., 1e200, 1e-200] {
            let roots = super::real_roots(
                Coefficient::single(1. / s),
                Coefficient::single(0.),
                Coefficient::single(-s),
            );
            println!("Creo quadratic x^2/{s:e}-{s:e}: {roots:?}");
            assert_eq!(roots.len(), 2);
            for (root, expected) in roots.iter().zip([-s, s]) {
                assert!((root / expected - 1.0).abs() <= 8.0 * f64::EPSILON);
            }
        }
    }

    #[test]
    fn numerical_0922b_quadratic_rescaling_retains_finite_root() {
        let smallest = f64::from_bits(1);
        for (quadratic, constant) in [(1e308, smallest), (smallest, 1e308)] {
            for sign in [-1.0, 1.0] {
                let roots = super::real_roots(
                    Coefficient::single(quadratic),
                    Coefficient::single(sign * 1e308),
                    Coefficient::single(constant),
                );
                assert!(roots.iter().all(|root| root.is_finite()));
                assert!(roots
                    .iter()
                    .any(|root| (root + sign).abs() <= 8.0 * f64::EPSILON));
            }
        }
    }
}
