// SPDX-License-Identifier: Apache-2.0
//! Scaled geometric least-squares solves.

use super::Vector3;

/// Scalar least-squares step along a finite nonzero tangent. Exact products
/// retain the quotient when either squared norms or projections overflow or underflow.
pub fn projection_step(tangent: Vector3, residual: Vector3) -> Option<f64> {
    if !tangent.is_finite() || !residual.is_finite() {
        return None;
    }
    let mut numerator = super::sum::ExactSignedSum::default();
    let mut denominator = super::sum::ExactSignedSum::default();
    for (t, r) in [
        (tangent.x, residual.x),
        (tangent.y, residual.y),
        (tangent.z, residual.z),
    ] {
        numerator.add_product(t, r);
        denominator.add_product(t, t);
    }
    let denominator = denominator.finish()?;
    numerator
        .finish()
        .map_or(Some(0.0), |value| value.quotient(denominator))
}

/// Solve a two-column least-squares step without imposing an absolute rank scale.
pub fn least_squares_step(du: Vector3, dv: Vector3, residual: Vector3) -> Option<(f64, f64)> {
    if !du.is_finite() || !dv.is_finite() || !residual.is_finite() {
        return None;
    }
    let du_scale = du.x.abs().max(du.y.abs()).max(du.z.abs());
    let dv_scale = dv.x.abs().max(dv.y.abs()).max(dv.z.abs());
    if du_scale == 0.0 || dv_scale == 0.0 {
        return None;
    }
    let du = Vector3::new(du.x / du_scale, du.y / du_scale, du.z / du_scale);
    let dv = Vector3::new(dv.x / dv_scale, dv.y / dv_scale, dv.z / dv_scale);
    let du_squared = du.dot(du);
    let mixed = du.dot(dv);
    let dv_squared = dv.dot(dv);
    let determinant = du_squared.mul_add(dv_squared, -mixed * mixed);
    if determinant <= f64::EPSILON * du_squared * dv_squared {
        return None;
    }
    let residual_scale = residual.x.abs().max(residual.y.abs()).max(residual.z.abs());
    if residual_scale == 0.0 {
        return Some((0.0, 0.0));
    }
    let residual = Vector3::new(
        residual.x / residual_scale,
        residual.y / residual_scale,
        residual.z / residual_scale,
    );
    let du_residual = du.dot(residual);
    let dv_residual = dv.dot(residual);
    let u = super::multiply_divide(
        (dv_squared * du_residual - mixed * dv_residual) / determinant,
        residual_scale,
        du_scale,
    )?;
    let v = super::multiply_divide(
        (du_squared * dv_residual - mixed * du_residual) / determinant,
        residual_scale,
        dv_scale,
    )?;
    (u.is_finite() && v.is_finite()).then_some((u, v))
}

#[cfg(test)]
mod tests {
    use super::Vector3;
    #[test]
    fn numerical_audit_least_squares_checks_rank_independent_of_column_scale() {
        let tiny = Vector3::new(1.0e-200, 0.0, 0.0);
        let huge = Vector3::new(0.0, 1.0e200, 0.0);
        assert_eq!(
            super::least_squares_step(tiny, huge, huge),
            Some((0.0, 1.0))
        );
        for scale in [1.0e-200, 1.0e-9, 1.0, 1.0e200] {
            let du = Vector3::new(1.0, 0.0, 0.0);
            let dv = Vector3::new(0.0, scale, 0.0);
            assert_eq!(super::least_squares_step(du, dv, dv), Some((0.0, 1.0)));
            assert_eq!(super::least_squares_step(du, du, dv), None);
        }
    }

    #[test]
    fn numerical_audit_projection_step_preserves_tangent_units() {
        for scale in [1e-200, 1., 1e200] {
            let step =
                super::projection_step(Vector3::new(scale, 0., 0.), Vector3::new(0.25, 0., 0.))
                    .unwrap();
            assert!((step * scale - 0.25).abs() <= 8. * f64::EPSILON);
        }
        assert_eq!(
            super::projection_step(Vector3::new(0., 0., 0.), Vector3::new(1., 0., 0.)),
            None
        );
    }
}
