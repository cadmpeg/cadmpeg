// SPDX-License-Identifier: Apache-2.0
//! Scaled geometric least-squares solves.

use super::Vector3;
use crate::features::FiniteVector3;
use crate::scalar::FiniteReal;

/// Scalar least-squares step along a finite nonzero tangent. Exact products
/// retain the quotient when either squared norms or projections overflow or underflow.
pub fn projection_step(tangent: Vector3, residual: Vector3) -> Option<FiniteReal> {
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
    numerator.finish().map_or(Some(FiniteReal::ZERO), |value| {
        value.quotient(denominator).ok()
    })
}

/// Solve a two-column least-squares step without imposing an absolute rank scale.
pub fn least_squares_step(
    du: FiniteVector3,
    dv: FiniteVector3,
    residual: Vector3,
) -> Option<(FiniteReal, FiniteReal)> {
    if !residual.is_finite() {
        return None;
    }
    let (du, dv) = (du.get(), dv.get());
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
    if residual.x == 0.0 && residual.y == 0.0 && residual.z == 0.0 {
        return Some((FiniteReal::ZERO, FiniteReal::ZERO));
    }
    let fast = (|| {
        let residual_components = [residual.x, residual.y, residual.z];
        let du_components = [du.x, du.y, du.z];
        let dv_components = [dv.x, dv.y, dv.z];
        let du_residual = super::sum::fast_dot(
            du_components,
            residual_components,
            std::array::from_fn(|index| du_components[index] * residual_components[index]),
        )?
        .get();
        let dv_residual = super::sum::fast_dot(
            dv_components,
            residual_components,
            std::array::from_fn(|index| dv_components[index] * residual_components[index]),
        )?
        .get();
        let u_numerator = super::sum::fast_dot(
            [dv_squared, -mixed],
            [du_residual, dv_residual],
            [dv_squared * du_residual, -mixed * dv_residual],
        )?
        .get();
        let v_numerator = super::sum::fast_dot(
            [du_squared, -mixed],
            [dv_residual, du_residual],
            [du_squared * dv_residual, -mixed * du_residual],
        )?
        .get();
        let u_denominator = determinant * du_scale;
        let v_denominator = determinant * dv_scale;
        if !u_denominator.is_normal() || !v_denominator.is_normal() {
            return None;
        }
        // A quotient is kept when it is normal, or zero from a zero numerator.
        let admitted = |value: f64, numerator: f64| {
            FiniteReal::normal_or_zero(value).filter(|value| value.get() != 0.0 || numerator == 0.0)
        };
        Some((
            admitted(u_numerator / u_denominator, u_numerator)?,
            admitted(v_numerator / v_denominator, v_numerator)?,
        ))
    })();
    if let Some(step) = fast {
        return Some(step);
    }

    let mut u_numerator = super::sum::ExactSignedSum::default();
    let mut v_numerator = super::sum::ExactSignedSum::default();
    for (u, v, r) in [
        (du.x, dv.x, residual.x),
        (du.y, dv.y, residual.y),
        (du.z, dv.z, residual.z),
    ] {
        u_numerator.add_factors([dv_squared, u, r]);
        u_numerator.add_factors([-mixed, v, r]);
        v_numerator.add_factors([du_squared, v, r]);
        v_numerator.add_factors([-mixed, u, r]);
    }
    let mut u_denominator = super::sum::ExactSignedSum::default();
    u_denominator.add_product(determinant, du_scale);
    let mut v_denominator = super::sum::ExactSignedSum::default();
    v_denominator.add_product(determinant, dv_scale);
    let u_denominator = u_denominator.finish()?;
    let v_denominator = v_denominator.finish()?;
    let u = u_numerator
        .finish()
        .map_or(Some(FiniteReal::ZERO), |value| {
            value.quotient(u_denominator).ok()
        })?;
    let v = v_numerator
        .finish()
        .map_or(Some(FiniteReal::ZERO), |value| {
            value.quotient(v_denominator).ok()
        })?;
    Some((u, v))
}

#[cfg(test)]
mod tests {
    use super::Vector3;
    use crate::scalar::FiniteReal;

    fn step(du: Vector3, dv: Vector3, residual: Vector3) -> Option<(f64, f64)> {
        super::least_squares_step(
            super::FiniteVector3::new(du)?,
            super::FiniteVector3::new(dv)?,
            residual,
        )
        .map(|(u, v)| (u.get(), v.get()))
    }

    #[test]
    fn numerical_audit_least_squares_checks_rank_independent_of_column_scale() {
        let tiny = Vector3::new(1.0e-200, 0.0, 0.0);
        let huge = Vector3::new(0.0, 1.0e200, 0.0);
        assert_eq!(step(tiny, huge, huge), Some((0.0, 1.0)));
        for scale in [1.0e-200, 1.0e-9, 1.0, 1.0e200] {
            let du = Vector3::new(1.0, 0.0, 0.0);
            let dv = Vector3::new(0.0, scale, 0.0);
            assert_eq!(step(du, dv, dv), Some((0.0, 1.0)));
            assert_eq!(step(du, du, dv), None);
        }
    }

    #[test]
    fn least_squares_preserves_independent_residual_components() {
        let tiny = Vector3::new(1.0e-200, 0.0, 0.0);
        let huge = Vector3::new(0.0, 1.0e200, 0.0);
        let residual = Vector3::new(tiny.x, huge.y, 0.0);
        assert_eq!(step(tiny, huge, residual), Some((1.0, 1.0)));

        let smallest = Vector3::new(f64::from_bits(1), 0.0, 0.0);
        let unit = Vector3::new(0.0, 1.0, 0.0);
        let residual = Vector3::new(smallest.x, unit.y, 0.0);
        assert_eq!(step(smallest, unit, residual), Some((1.0, 1.0)));
    }

    #[test]
    fn numerical_audit_projection_step_preserves_tangent_units() {
        for scale in [1e-200, 1., 1e200] {
            let step =
                super::projection_step(Vector3::new(scale, 0., 0.), Vector3::new(0.25, 0., 0.))
                    .unwrap()
                    .get();
            assert!((step * scale - 0.25).abs() <= 8. * f64::EPSILON);
        }
        assert_eq!(
            super::projection_step(Vector3::new(0., 0., 0.), Vector3::new(1., 0., 0.))
                .map(FiniteReal::get),
            None
        );
    }
}
