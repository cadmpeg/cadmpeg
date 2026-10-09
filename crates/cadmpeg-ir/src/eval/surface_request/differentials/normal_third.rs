// SPDX-License-Identifier: Apache-2.0
//! Third normal partials from actual fourth surface partials.

use super::{cross_sum, dot_scaled, ExactSignedSum, FiniteReal, FiniteVector3, NormalDerivative, NormalSecond, ScaledValue};
use crate::eval::{EvaluationFailure, SurfaceJet};

// Higher normal products need their nonzero intermediate coefficients in
// normal binary64. A rounded subnormal or zero must not be amplified later.
fn finite_radial(value: Option<ScaledValue>) -> Result<FiniteReal, EvaluationFailure<()>> {
    value.map_or(Ok(FiniteReal::ZERO), |value| {
        value.finite().ok().filter(|value| value.get().is_normal()).ok_or(EvaluationFailure::NoValue)
    })
}

fn normal_lanes(derivative: NormalDerivative) -> Result<[FiniteReal; 3], EvaluationFailure<()>> {
    for (value, numerator) in derivative.finite.into_iter().zip(derivative.numerator) {
        if numerator.is_some() && !value.get().is_normal() {
            return Err(EvaluationFailure::NoValue);
        }
    }
    Ok(derivative.finite)
}

pub(in crate::eval::surface_request) fn offset_third(
    base: SurfaceJet,
    third: Result<[FiniteVector3; 4], EvaluationFailure<()>>,
    fourth: Result<[FiniteVector3; 5], EvaluationFailure<()>>,
    distance: f64,
    state: NormalSecond,
) -> Result<[FiniteVector3; 4], EvaluationFailure<()>> {
    let [du, dv] = FiniteVector3::raw_array(base.first?);
    let [duu, duv, dvv] = FiniteVector3::raw_array(base.second?);
    let third = FiniteVector3::raw_array(third?);
    let [duuu, duuv, duvv, dvvv] = third;
    let [duuuu, duuuv, duuvv, duvvv, dvvvv] = FiniteVector3::raw_array(fourth?);
    let first_normal = [normal_lanes(state.first[0])?, normal_lanes(state.first[1])?];
    let second_normal = [normal_lanes(state.second[0])?, normal_lanes(state.second[1])?, normal_lanes(state.second[2])?];
    let radial_first = [finite_radial(state.radial_first[0])?, finite_radial(state.radial_first[1])?];
    let mut radial_second = [FiniteReal::ZERO; 3];
    for (order, lane) in radial_second.iter_mut().enumerate() {
        let mut sum = ExactSignedSum::default();
        sum.add_scaled_product(state.radial_second_cross[order], FiniteReal::ONE).ok_or(EvaluationFailure::NoValue)?;
        sum.add_scaled_product(state.radial_second_normal[order], FiniteReal::ONE).ok_or(EvaluationFailure::NoValue)?;
        *lane = finite_radial(sum.finish())?;
    }
    let cross_third = [
        cross_sum(&[(1.0, duuuu, dv), (3.0, duuu, duv), (3.0, duu, duuv), (1.0, du, duuuv)]),
        cross_sum(&[(1.0, duuuv, dv), (1.0, duuu, dvv), (1.0, duuv, duv), (2.0, duu, duvv), (1.0, du, duuvv)]),
        cross_sum(&[(1.0, duuvv, dv), (2.0, duuv, dvv), (1.0, duu, dvvv), (1.0, duv, duvv), (1.0, du, duvvv)]),
        cross_sum(&[(1.0, duvvv, dv), (3.0, duvv, dvv), (3.0, duv, dvvv), (1.0, du, dvvvv)]),
    ];
    // Each ordered tuple lists i,j,k and their corresponding jk,ik,ij.
    // Repeated indices remain separate product-rule terms.
    let orders = [(0, 0, 0, 0, 0, 0), (0, 0, 1, 1, 1, 0), (0, 1, 1, 2, 1, 1), (1, 1, 1, 2, 2, 2)];
    let mut output = [FiniteVector3::ZERO; 4];
    for (order, (i, j, k, jk, ik, ij)) in orders.into_iter().enumerate() {
        let mut radial = ExactSignedSum::default();
        radial.add_scaled_product(dot_scaled(cross_third[order], state.normal)?, FiniteReal::ONE)
            .ok_or(EvaluationFailure::NoValue)?;
        for axis in 0..3 {
            radial.add_factors([radial_first[i].get(), first_normal[j][axis].get(), first_normal[k][axis].get()]);
            radial.add_factors([radial_first[j].get(), first_normal[i][axis].get(), first_normal[k][axis].get()]);
            radial.add_factors([radial_first[k].get(), first_normal[i][axis].get(), first_normal[j][axis].get()]);
            radial.add_factors([state.finite_magnitude.get(), first_normal[i][axis].get(), second_normal[jk][axis].get()]);
            radial.add_factors([state.finite_magnitude.get(), first_normal[j][axis].get(), second_normal[ik][axis].get()]);
            radial.add_factors([state.finite_magnitude.get(), first_normal[k][axis].get(), second_normal[ij][axis].get()]);
        }
        let radial = radial.finish();
        let base_lanes = [third[order].x, third[order].y, third[order].z];
        let mut lanes = [FiniteReal::ZERO; 3];
        for (axis, lane) in lanes.iter_mut().enumerate() {
            let mut numerator = ExactSignedSum::default();
            numerator.add_scaled_product(cross_third[order][axis], FiniteReal::ONE).ok_or(EvaluationFailure::NoValue)?;
            numerator.add_scaled_product(radial, state.normal[axis].negated()).ok_or(EvaluationFailure::NoValue)?;
            numerator.add_product(-radial_second[ij].get(), first_normal[k][axis].get());
            numerator.add_product(-radial_second[ik].get(), first_normal[j][axis].get());
            numerator.add_product(-radial_second[jk].get(), first_normal[i][axis].get());
            numerator.add_product(-radial_first[i].get(), second_normal[jk][axis].get());
            numerator.add_product(-radial_first[j].get(), second_normal[ik][axis].get());
            numerator.add_product(-radial_first[k].get(), second_normal[ij][axis].get());
            let normal_third = numerator.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                value.quotient(state.magnitude).ok().filter(|value| value.get().is_normal()).ok_or(EvaluationFailure::NoValue)
            })?;
            let mut sum = ExactSignedSum::default();
            sum.add_product(base_lanes[axis], 1.0);
            sum.add_product(distance, normal_third.get());
            *lane = sum.finish().map_or(Ok(FiniteReal::ZERO), |value| value.finite().map_err(|_| EvaluationFailure::NonFinite(())))?;
        }
        output[order] = FiniteVector3::from_components(lanes[0], lanes[1], lanes[2]);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_nonzero_radial_range_loss_is_unavailable_before_amplification() {
        assert_eq!(finite_radial(None).unwrap(), FiniteReal::ZERO);
        for factors in [[f64::from_bits(1), 1.0], [f64::from_bits(1), f64::from_bits(1)], [f64::MAX, f64::MAX]] {
            let mut sum = ExactSignedSum::default();
            sum.add_product(factors[0], factors[1]);
            assert!(sum.finish().is_some());
            assert_eq!(finite_radial(sum.finish()), Err(EvaluationFailure::NoValue));
        }
        let mut numerator = ExactSignedSum::default();
        numerator.add_product(f64::from_bits(1), f64::from_bits(1));
        let rounded_zero = NormalDerivative {
            finite: [FiniteReal::ZERO; 3], numerator: [numerator.finish(), None, None],
        };
        assert_eq!(normal_lanes(rounded_zero), Err(EvaluationFailure::NoValue));
        assert_eq!(normal_lanes(NormalDerivative { finite: [FiniteReal::ZERO; 3], numerator: [None; 3] }).unwrap(), [FiniteReal::ZERO; 3]);
    }
}
