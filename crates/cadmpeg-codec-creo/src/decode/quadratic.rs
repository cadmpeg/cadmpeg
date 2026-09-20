// SPDX-License-Identifier: Apache-2.0
//! Real quadratic roots with coefficient-scale-independent arithmetic, and the
//! cancellation rule that decides when a coefficient of such a root problem is
//! exactly zero.

/// The relative band inside which a sum of products states zero.
///
/// A sum of n products in f64 carries one rounding per product and one per
/// addition, so the computed value differs from the exact sum of the exact
/// products by at most `(2n - 1) * u * terms`, where `u = f64::EPSILON / 2` is
/// the unit roundoff and `terms` is the sum of the magnitudes of the products.
/// This constant is `128 * u`, which covers `n = 64`.
///
/// The longest sum a caller forms is the constant term of a quadric restricted
/// to a line or to a plane: three products against the matrix-vector product,
/// three against the linear part, and the quadric constant, which is thirteen
/// products once each matrix-vector entry's own three-product dot is counted.
/// That is `25 * u`, one fifth of the band. The Bezier plane-distance
/// differences of a cubic extrusion reach four products and the section
/// equal-length constant four, well inside it.
///
/// The bound measures the products summed at the call site. A term that is
/// itself the result of cancellation carries its own error, which this bound
/// does not state.
const EPS_QUADRATIC_CANCELLATION: f64 = 64.0 * f64::EPSILON;

/// A coefficient formed as a sum of products, where `terms` is the sum of the
/// magnitudes of those products. A sum that cancels to within the rounding
/// error of its terms states the coefficient exactly.
///
/// The caller applies this rule because only the site that forms the sum holds
/// the magnitudes of its terms; `real_roots` sees the coefficients alone. A
/// zero quadratic coefficient states that the problem is linear, so
/// `real_roots` takes the lower-degree branch. A zero constant states that the
/// origin of the restriction satisfies the equation, which keeps a tangency a
/// double root rather than a discriminant that is negative only by the rounding
/// of its coefficients.
pub(super) fn cancelling_coefficient(value: f64, terms: f64) -> f64 {
    if value.abs() <= EPS_QUADRATIC_CANCELLATION * terms {
        return 0.0;
    }
    value
}

/// Return finite real roots in ascending order. A small negative discriminant
/// is admitted only within the rounding error of its two terms.
pub(super) fn real_roots(quadratic: f64, linear: f64, constant: f64) -> Vec<f64> {
    if [quadratic, linear, constant].iter().any(|v| !v.is_finite()) {
        return Vec::new();
    }
    if quadratic == 0.0 {
        let root = -constant / linear;
        return root.is_finite().then_some(root).into_iter().collect();
    }
    let scale = quadratic.abs().max(linear.abs()).max(constant.abs());
    let a = quadratic / scale;
    let b = linear / scale;
    let c = constant / scale;
    let product = 4.0 * a * c;
    let discriminant = b.mul_add(b, -product);
    let error = EPS_QUADRATIC_CANCELLATION * (b * b + product.abs());
    if discriminant < -error {
        return Vec::new();
    }
    let root = discriminant.max(0.0).sqrt();
    let mut roots = if root == 0.0 {
        vec![-b / (2.0 * a)]
    } else {
        let q = -0.5 * (b + root.copysign(b));
        vec![q / a, c / q]
    };
    roots.retain(|root| root.is_finite());
    roots.sort_by(f64::total_cmp);
    roots.dedup();
    roots
}

#[cfg(test)]
mod tests {
    #[test]
    fn numerical_followup_quadratic_roots_ignore_common_coefficient_scale() {
        for scale in [1.0, 1e-20, 1e-200, 1e200, 1e308] {
            assert_eq!(super::real_roots(scale, 0.0, -scale), [-1.0, 1.0]);
            assert!(super::real_roots(scale, 0.0, scale).is_empty());
            assert_eq!(super::real_roots(0.0, scale, -scale), [1.0]);
        }
        let roots = super::real_roots(1.0, -1e16, 1.0);
        assert_eq!(roots, [1e-16, 1e16]);
    }
}
