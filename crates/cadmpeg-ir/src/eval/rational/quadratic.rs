// SPDX-License-Identifier: Apache-2.0
//! Fixed three-pole Bernstein homogeneous orders and rational quotients.

use super::{HigherLanes, Homogeneous};
use crate::features::FinitePoint3;
use crate::math::sum::{ExactSignedSum, ScaledValue};
use crate::scalar::{FiniteReal, NonZeroReal};
use crate::units::FinitePoint2;

/// The actual fixed spatial or parameter-plane source rows.
#[derive(Clone, Copy)]
pub(in crate::eval) enum QuadraticPoles<'a> {
    Spatial(&'a [crate::geometry::nurbs::WeightedPole3<FinitePoint3>; 3]),
    Planar(&'a [crate::geometry::pcurve::WeightedPole2<FinitePoint2>; 3]),
}

impl QuadraticPoles<'_> {
    fn pole(self, index: usize) -> (FinitePoint3, NonZeroReal) {
        match self {
            Self::Spatial(poles) => (poles[index].point, poles[index].weight),
            Self::Planar(poles) => (crate::eval::planar_pole(poles[index].point), poles[index].weight),
        }
    }
}

/// The three normalized orders of a quadratic Bezier carrier, in one
/// fixed three-pole walk. Accumulate the expanded Bernstein terms before
/// rounding; a tiny squared parameter remains an extended product.
pub(in crate::eval) fn orders(
    poles: QuadraticPoles<'_>,
    parameter: FiniteReal,
) -> Option<[Homogeneous; 3]> {
    let s = parameter.get();
    if !(0.0..=1.0).contains(&s) { return None; }
    let mut sums: [[ExactSignedSum; 4]; 3] =
        std::array::from_fn(|_| std::array::from_fn(|_| ExactSignedSum::default()));
    let mut constant = poles.pole(0).0.coordinates().map(Some);
    for local in 0..3 {
        let (point, weight) = poles.pole(local);
        for (axis, coordinate) in point.coordinates().into_iter().enumerate() {
            if constant[axis] != Some(coordinate) { constant[axis] = None; }
        }
        // B=[1-2s+s²,2s-2s²,s²], B'=[-2+2s,2-4s,2s].
        // Every term has at most four finite factors, including weight
        // and coordinate. The ordinal is bounded by the actual array3.
        let (base, first, second): (&[[f64; 2]], &[[f64; 2]], f64) = match local {
            0 => (&[[1.0, 1.0], [-s, 1.0], [-s, 1.0], [s, s]],
                &[[-2.0, 1.0], [2.0, s]], 2.0),
            1 => (&[[s, 1.0], [s, 1.0], [-s, s], [-s, s]],
                &[[2.0, 1.0], [-4.0, s]], -4.0),
            _ => (&[[s, s]], &[[2.0, s]], 2.0),
        };
        for (lanes, terms) in sums[..2].iter_mut().zip([base, first]) {
            for (sum, coordinate) in lanes.iter_mut().zip([point.x, point.y, point.z, 1.0]) {
                for [u, v] in terms {
                    sum.add_factors([*u, *v, weight.get(), coordinate]);
                }
            }
        }
        for (sum, coordinate) in sums[2].iter_mut().zip([point.x, point.y, point.z, 1.0]) {
            sum.add_factors([second, weight.get(), coordinate]);
        }
    }
    let [base, first, second] = sums.map(|lanes| Homogeneous {
        values: lanes.map(ExactSignedSum::finish), constant: [None; 3],
    });
    Some([Homogeneous { constant, ..base }, first, second])
}

/// The complete requested quotient orders for this fixed quadratic carrier.
/// Only the all-three-source-pole equality from orders proves
/// a coordinate constant. General sampled support equality does not.
pub(in crate::eval) fn higher(
    poles: QuadraticPoles<'_>,
    parameter: FiniteReal,
    width: ScaledValue,
    fourth: bool,
    fifth: bool,
) -> Option<HigherLanes> {
    let [base, first, second] = orders(poles, parameter)?;
    let zero = [None; 4];
    if fifth {
        HigherLanes::from_orders([base.values, first.values, second.values, zero, zero, zero],
            base.constant, width, true, true)
    } else if fourth {
        HigherLanes::from_orders([base.values, first.values, second.values, zero, zero],
            base.constant, width, true, false)
    } else {
        HigherLanes::from_orders([base.values, first.values, second.values, zero],
            base.constant, width, false, false)
    }
}

#[cfg(test)]
mod tests;
