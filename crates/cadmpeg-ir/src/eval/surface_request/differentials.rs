// SPDX-License-Identifier: Apache-2.0
//! Analytic third partials and second partials of an oriented offset.

use super::super::{admit_lanes, vector_sum, EvaluationFailure, SurfaceJet};
use crate::features::FiniteVector3;
use crate::geometry::SolvedSurfaceGeometry;
use crate::math::sum::{scaled_finite, ExactSignedSum, ScaledValue};
use crate::math::Vector3;
use crate::scalar::FiniteReal;

pub(in crate::eval) fn analytic_third(
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
    base: SurfaceJet,
) -> Result<[FiniteVector3; 4], EvaluationFailure<()>> {
    let zero = Vector3::new(0.0, 0.0, 0.0);
    let [du, dv] = base.first?;
    let neg_u = du.negated().get();
    let neg_v = dv.negated().get();
    let lanes = match geometry {
        SolvedSurfaceGeometry::Cylinder(_) => [neg_u, zero, zero, zero],
        SolvedSurfaceGeometry::Cone(cone) => {
            let reference = *cone.frame().reference().as_raw();
            let transverse = cone.frame().axis().as_raw().cross(reference);
            let slope = cone.half_angle().get().tan();
            [
                neg_u,
                vector_sum(&[
                    (-slope * u.cos(), reference),
                    (-slope * cone.ratio().get() * u.sin(), transverse),
                ]),
                zero,
                zero,
            ]
        }
        SolvedSurfaceGeometry::Sphere(sphere) => {
            let reference = *sphere.frame().reference().as_raw();
            let transverse = sphere.frame().axis().as_raw().cross(reference);
            let radius = sphere.radius().get();
            [
                neg_u,
                vector_sum(&[
                    (radius * v.sin() * u.cos(), reference),
                    (radius * v.sin() * u.sin(), transverse),
                ]),
                neg_u,
                neg_v,
            ]
        }
        SolvedSurfaceGeometry::Torus(torus) => {
            let reference = *torus.frame().reference().as_raw();
            let transverse = torus.frame().axis().as_raw().cross(reference);
            let minor = torus.minor_radius().get();
            [
                neg_u,
                vector_sum(&[
                    (minor * v.sin() * u.cos(), reference),
                    (minor * v.sin() * u.sin(), transverse),
                ]),
                vector_sum(&[
                    (minor * v.cos() * u.sin(), reference),
                    (-minor * v.cos() * u.cos(), transverse),
                ]),
                neg_v,
            ]
        }
        _ => return Err(EvaluationFailure::NoValue),
    };
    admit_lanes(lanes)
}

/// A cross-product sum kept outside the binary64 exponent range. Each term
/// has at most three finite factors, including its fixed coefficient.
fn cross_sum(terms: &[(f64, Vector3, Vector3)]) -> [Option<ScaledValue>; 3] {
    std::array::from_fn(|axis| {
        let left_axis = (axis + 1) % 3;
        let right_axis = (axis + 2) % 3;
        let mut sum = ExactSignedSum::default();
        for (coefficient, left, right) in terms {
            let left = [left.x, left.y, left.z];
            let right = [right.x, right.y, right.z];
            sum.add_factors([*coefficient, left[left_axis], right[right_axis]]);
            sum.add_factors([-*coefficient, left[right_axis], right[left_axis]]);
        }
        sum.finish()
    })
}

fn dot_scaled(
    vector: [Option<ScaledValue>; 3],
    factors: [FiniteReal; 3],
) -> Result<Option<ScaledValue>, EvaluationFailure<()>> {
    let mut sum = ExactSignedSum::default();
    for (value, factor) in vector.into_iter().zip(factors) {
        sum.add_scaled_product(value, factor)
            .ok_or(EvaluationFailure::NonFinite(()))?;
    }
    Ok(sum.finish())
}

fn normal_first(
    derivative: [Option<ScaledValue>; 3],
    normal: [FiniteReal; 3],
    radial: Option<ScaledValue>,
    magnitude: ScaledValue,
) -> Result<[FiniteReal; 3], EvaluationFailure<()>> {
    let mut output = [FiniteReal::ZERO; 3];
    for (axis, lane) in output.iter_mut().enumerate() {
        let mut sum = ExactSignedSum::default();
        sum.add_scaled_product(derivative[axis], FiniteReal::ONE)
            .ok_or(EvaluationFailure::NonFinite(()))?;
        sum.add_scaled_product(radial, normal[axis].negated())
            .ok_or(EvaluationFailure::NonFinite(()))?;
        *lane = sum.finish().map_or(Ok(FiniteReal::ZERO), |sum| {
            sum.quotient(magnitude)
                .map_err(|_| EvaluationFailure::NonFinite(()))
        })?;
    }
    Ok(output)
}

/// Differentiate W=r*n twice, where W=S_u cross S_v. The caller has already
/// applied the existing finite/nonzero-normal gate for the offset point.
/// First normal derivatives and radial derivatives stay in extended sums
/// until division. The largest product has four finite factors: r*n_i*n_j*n.
pub(super) fn offset_second(
    base: SurfaceJet,
    third: Result<[FiniteVector3; 4], EvaluationFailure<()>>,
    distance: f64,
    normal: Vector3,
    magnitude: f64,
) -> Result<[FiniteVector3; 3], EvaluationFailure<()>> {
    let [du, dv] = FiniteVector3::raw_array(base.first?);
    let [duu, duv, dvv] = FiniteVector3::raw_array(base.second?);
    let [duuu, duuv, duvv, dvvv] = FiniteVector3::raw_array(third?);
    let non_finite = EvaluationFailure::NonFinite(());
    let normal = FiniteReal::array([normal.x, normal.y, normal.z]).ok_or(non_finite)?;
    let finite_magnitude = FiniteReal::new(magnitude).ok_or(non_finite)?;
    let magnitude = scaled_finite(finite_magnitude.get()).ok_or(non_finite)?;
    let first_cross = [
        cross_sum(&[(1.0, duu, dv), (1.0, du, duv)]),
        cross_sum(&[(1.0, duv, dv), (1.0, du, dvv)]),
    ];
    let radials = [dot_scaled(first_cross[0], normal)?, dot_scaled(first_cross[1], normal)?];
    let first_normal = [
        normal_first(first_cross[0], normal, radials[0], magnitude)?,
        normal_first(first_cross[1], normal, radials[1], magnitude)?,
    ];
    let second_cross = [
        cross_sum(&[(1.0, duuu, dv), (2.0, duu, duv), (1.0, du, duuv)]),
        cross_sum(&[(1.0, duuv, dv), (1.0, duu, dvv), (1.0, du, duvv)]),
        cross_sum(&[(1.0, duvv, dv), (2.0, duv, dvv), (1.0, du, dvvv)]),
    ];
    let second = [duu, duv, dvv];
    let mut output = [FiniteVector3::ZERO; 3];
    for (order, (first_axis, second_axis)) in [(0, 0), (0, 1), (1, 1)].into_iter().enumerate() {
        let radial_second = dot_scaled(second_cross[order], normal)?;
        let mut normal_dot = ExactSignedSum::default();
        for axis in 0..3 {
            normal_dot.add_product(
                first_normal[first_axis][axis].get(),
                first_normal[second_axis][axis].get(),
            );
        }
        let normal_dot = normal_dot.finish();
        let mut normal_scaled_dot = ExactSignedSum::default();
        normal_scaled_dot
            .add_scaled_product(normal_dot, finite_magnitude)
            .ok_or(non_finite)?;
        let normal_scaled_dot = normal_scaled_dot.finish();
        let mut lanes = [FiniteReal::ZERO; 3];
        let base_lanes = [second[order].x, second[order].y, second[order].z];
        for (axis, lane) in lanes.iter_mut().enumerate() {
            let mut derivative = ExactSignedSum::default();
            derivative.add_scaled_product(second_cross[order][axis], FiniteReal::ONE).ok_or(non_finite)?;
            derivative.add_scaled_product(radial_second, normal[axis].negated()).ok_or(non_finite)?;
            derivative.add_scaled_product(radials[second_axis], first_normal[first_axis][axis].negated()).ok_or(non_finite)?;
            derivative.add_scaled_product(radials[first_axis], first_normal[second_axis][axis].negated()).ok_or(non_finite)?;
            derivative.add_scaled_product(normal_scaled_dot, normal[axis].negated()).ok_or(non_finite)?;
            let normal_second = derivative.finish().map_or(Ok(FiniteReal::ZERO), |value| value.quotient(magnitude).map_err(|_| non_finite))?;
            let mut sum = ExactSignedSum::default();
            sum.add_product(base_lanes[axis], 1.0);
            sum.add_product(distance, normal_second.get());
            *lane = sum.finish().map_or(Ok(FiniteReal::ZERO), |value| value.finite().map_err(|_| non_finite))?;
        }
        output[order] = FiniteVector3::from_components(lanes[0], lanes[1], lanes[2]);
    }
    Ok(output)
}
