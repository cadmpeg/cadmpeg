// SPDX-License-Identifier: Apache-2.0
//! Fourth normal partials from actual fifth surface partials.

use super::normal_third::{finite_radial, normal_lanes, NormalThird};
use super::{dot_scaled, ExactSignedSum, FiniteReal, FiniteVector3};
use crate::eval::surface_request::HigherPartials;
use crate::eval::{EvaluationFailure, SurfaceJet};

const BINOMIAL: [[f64; 5]; 5] = [
    [1.0, 0.0, 0.0, 0.0, 0.0],
    [1.0, 1.0, 0.0, 0.0, 0.0],
    [1.0, 2.0, 1.0, 0.0, 0.0],
    [1.0, 3.0, 3.0, 1.0, 0.0],
    [1.0, 4.0, 6.0, 4.0, 1.0],
];

// The complete zero-through-third bivariate triangle, in total-order lanes.
fn index(u: usize, v: usize) -> usize {
    let order = u + v;
    order * (order + 1) / 2 + v
}

pub(in crate::eval::surface_request) fn offset_fourth(
    base: SurfaceJet,
    higher: HigherPartials,
    distance: f64,
    third_normal: NormalThird,
) -> Result<[FiniteVector3; 5], EvaluationFailure<()>> {
    let first = base.first?;
    let second = base.second?;
    let third = higher.third()?;
    let fourth = higher.fourth()?;
    let fifth = higher.fifth()?;
    let partials: [&[FiniteVector3]; 5] = [&first, &second, &third, &fourth, &fifth];
    let state = third_normal.second;
    let mut normals = [[FiniteReal::ZERO; 3]; 10];
    normals[0] = state.normal;
    for (i, derivative) in state.first.into_iter().chain(state.second).enumerate() {
        normals[i + 1] = normal_lanes(derivative)?;
    }
    normals[6..].copy_from_slice(&third_normal.normal);
    let mut radials = [FiniteReal::ZERO; 10];
    radials[0] = state.finite_magnitude;
    for (i, value) in state.radial_first.into_iter().enumerate() {
        radials[i + 1] = finite_radial(value)?;
    }
    for (i, (cross, normal)) in state.radial_second_cross.into_iter().zip(state.radial_second_normal).enumerate() {
        let mut sum = ExactSignedSum::default();
        sum.add_scaled_product(cross, FiniteReal::ONE).ok_or(EvaluationFailure::NoValue)?;
        sum.add_scaled_product(normal, FiniteReal::ONE).ok_or(EvaluationFailure::NoValue)?;
        radials[i + 3] = finite_radial(sum.finish())?;
    }
    for (i, value) in third_normal.radial.into_iter().enumerate() {
        radials[i + 6] = finite_radial(value)?;
    }
    let mut output = [FiniteVector3::ZERO; 5];
    for (v, output) in output.iter_mut().enumerate() {
        let u = 4 - v;
        // D^alpha(S_u cross S_v), with actual partials through order five.
        let cross = std::array::from_fn(|axis| {
            let mut sum = ExactSignedSum::default();
            let left_axis = (axis + 1) % 3;
            let right_axis = (axis + 2) % 3;
            for bu in 0..=u {
                for bv in 0..=v {
                    let coefficient = BINOMIAL[u][bu] * BINOMIAL[v][bv];
                    let left = partials[bu + bv][bv].get();
                    let right = partials[u + v - bu - bv][v - bv + 1].get();
                    let left = [left.x, left.y, left.z];
                    let right = [right.x, right.y, right.z];
                    sum.add_factors([coefficient, left[left_axis], right[right_axis]]);
                    sum.add_factors([-coefficient, left[right_axis], right[left_axis]]);
                }
            }
            sum.finish()
        });
        let mut radial = ExactSignedSum::default();
        radial.add_scaled_product(dot_scaled(cross, state.normal)?, FiniteReal::ONE)
            .ok_or(EvaluationFailure::NoValue)?;
        // n.n_alpha = -1/2 sum of proper lower-order normal products.
        // Dot W_alpha=r_alpha*n+r*n_alpha+sum r_beta*n_(alpha-beta).
        for bu in 0..=u {
            for bv in 0..=v {
                if (bu == 0 && bv == 0) || (bu == u && bv == v) { continue; }
                let coefficient = BINOMIAL[u][bu] * BINOMIAL[v][bv];
                let left = index(bu, bv);
                let right = index(u - bu, v - bv);
                for axis in 0..3 {
                    radial.add_factors([0.5 * coefficient, state.finite_magnitude.get(), normals[left][axis].get(), normals[right][axis].get()]);
                    radial.add_factors([-coefficient, radials[left].get(), state.normal[axis].get(), normals[right][axis].get()]);
                }
            }
        }
        let radial = radial.finish();
        let original = fourth[v].get();
        let original = [original.x, original.y, original.z];
        let mut lanes = [FiniteReal::ZERO; 3];
        for (axis, lane) in lanes.iter_mut().enumerate() {
            let mut numerator = ExactSignedSum::default();
            numerator.add_scaled_product(cross[axis], FiniteReal::ONE).ok_or(EvaluationFailure::NoValue)?;
            numerator.add_scaled_product(radial, state.normal[axis].negated()).ok_or(EvaluationFailure::NoValue)?;
            for bu in 0..=u {
                for bv in 0..=v {
                    if (bu == 0 && bv == 0) || (bu == u && bv == v) { continue; }
                    let coefficient = BINOMIAL[u][bu] * BINOMIAL[v][bv];
                    numerator.add_factors([-coefficient, radials[index(bu, bv)].get(), normals[index(u - bu, v - bv)][axis].get()]);
                }
            }
            let normal_fourth = numerator.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                value.quotient(state.magnitude).ok().filter(|value| value.get().is_normal()).ok_or(EvaluationFailure::NoValue)
            })?;
            let mut sum = ExactSignedSum::default();
            sum.add_product(original[axis], 1.0);
            sum.add_product(distance, normal_fourth.get());
            *lane = sum.finish().map_or(Ok(FiniteReal::ZERO), |value| value.finite().map_err(|_| EvaluationFailure::NonFinite(())))?;
        }
        *output = FiniteVector3::from_components(lanes[0], lanes[1], lanes[2]);
    }
    Ok(output)
}
