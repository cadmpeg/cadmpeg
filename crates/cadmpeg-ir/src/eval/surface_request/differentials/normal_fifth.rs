// SPDX-License-Identifier: Apache-2.0
//! Fifth normal partials from actual sixth surface partials.

use super::normal_fourth::NormalFourth;
use super::normal_third::{finite_radial, normal_lanes};
use super::{dot_scaled, ExactSignedSum, FiniteReal, FiniteVector3};
use crate::eval::surface_request::HigherPartials;
use crate::eval::{EvaluationFailure, SurfaceJet};

const BINOMIAL: [[f64; 6]; 6] = [
    [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [1.0, 1.0, 0.0, 0.0, 0.0, 0.0],
    [1.0, 2.0, 1.0, 0.0, 0.0, 0.0],
    [1.0, 3.0, 3.0, 1.0, 0.0, 0.0],
    [1.0, 4.0, 6.0, 4.0, 1.0, 0.0],
    [1.0, 5.0, 10.0, 10.0, 5.0, 1.0],
];

// Zero through Fourth in total-order lanes; no row is a sampled theorem.
fn index(u: usize, v: usize) -> usize {
    let order = u + v;
    order * (order + 1) / 2 + v
}

pub(in crate::eval::surface_request) fn offset_fifth(
    base: SurfaceJet,
    higher: HigherPartials,
    distance: f64,
    fourth_normal: NormalFourth,
) -> Result<[FiniteVector3; 6], EvaluationFailure<()>> {
    let first = base.first?;
    let second = base.second?;
    let third = higher.third()?;
    let fourth = higher.fourth()?;
    let fifth = higher.fifth()?;
    let sixth = higher.sixth()?;
    let partials: [&[FiniteVector3]; 6] = [&first, &second, &third, &fourth, &fifth, &sixth];
    let third_normal = fourth_normal.third;
    let state = third_normal.second;
    let mut normals = [[FiniteReal::ZERO; 3]; 15];
    normals[0] = state.normal;
    for (i, derivative) in state.first.into_iter().chain(state.second).enumerate() {
        normals[i + 1] = normal_lanes(derivative)?;
    }
    normals[6..10].copy_from_slice(&third_normal.normal);
    normals[10..].copy_from_slice(&fourth_normal.normal);
    let mut radials = [FiniteReal::ZERO; 15];
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
    for (i, value) in third_normal.radial.into_iter().chain(fourth_normal.radial).enumerate() {
        radials[i + 6] = finite_radial(value)?;
    }
    let mut output = [FiniteVector3::ZERO; 6];
    for (v, output) in output.iter_mut().enumerate() {
        let u = 5 - v;
        // D^alpha(S_u cross S_v), from the supplied actual surface rows.
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
        // Differentiate n.n=1 and W=r*n. Both proper lower products
        // retain their bivariate binomial coefficient and actual order.
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
        let original = fifth[v].get();
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
            let normal_fifth = numerator.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                value.quotient(state.magnitude).ok().filter(|value| value.get().is_normal()).ok_or(EvaluationFailure::NoValue)
            })?;
            let mut sum = ExactSignedSum::default();
            sum.add_product(original[axis], 1.0);
            sum.add_product(distance, normal_fifth.get());
            *lane = sum.finish().map_or(Ok(FiniteReal::ZERO), |value| value.finite().map_err(|_| EvaluationFailure::NonFinite(())))?;
        }
        *output = FiniteVector3::from_components(lanes[0], lanes[1], lanes[2]);
    }
    Ok(output)
}
