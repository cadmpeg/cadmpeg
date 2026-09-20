// SPDX-License-Identifier: Apache-2.0
//! Real quadratic roots with coefficient-scale-independent arithmetic.

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
    let error = 64.0 * f64::EPSILON * (b * b + product.abs());
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
