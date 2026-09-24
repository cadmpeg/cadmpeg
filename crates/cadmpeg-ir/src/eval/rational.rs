// SPDX-License-Identifier: Apache-2.0
//! Homogeneous sums and quotient derivatives with an extended exponent range.
use crate::features::FinitePoint3;
use crate::math::sum::{product_sum, ExactSignedSum, ProductSum, ScaledValue};
use crate::scalar::FiniteReal;

#[derive(Clone, Copy)]
pub(super) struct Homogeneous {
    values: [Option<ScaledValue>; 4],
    constant: [Option<FiniteReal>; 3],
}

impl Homogeneous {
    pub(super) fn sum(
        terms: impl Iterator<Item = Option<([f64; 2], f64, FinitePoint3)>> + Clone,
    ) -> Option<Self> {
        let mut values = [None; 4];
        for (axis, value) in values.iter_mut().enumerate() {
            let sum = product_sum(terms.clone().map(|term| {
                let (basis, weight, point) = term?;
                Some([
                    basis[0],
                    basis[1],
                    weight,
                    [point.x, point.y, point.z, 1.0][axis],
                ])
            }));
            // `values` carries the same exact zero that `ExactSignedSum::finish`
            // and `add_scaled_product` state as `None`.
            *value = match sum {
                ProductSum::Undefined => return None,
                ProductSum::Zero => None,
                ProductSum::Value(value) => Some(value),
            };
        }
        let mut constant = [None; 3];
        let mut first = true;
        for term in terms {
            let (basis, weight, point) = term?;
            if basis.contains(&0.0) || weight == 0.0 {
                continue;
            }
            for (axis, coordinate) in point.coordinates().into_iter().enumerate() {
                if first {
                    constant[axis] = Some(coordinate);
                } else if constant[axis] != Some(coordinate) {
                    constant[axis] = None;
                }
            }
            first = false;
        }
        Some(Self { values, constant })
    }

    /// Keep source weights when they remain normal. Otherwise choose one
    /// binary scale for the complete output net, preserving relative weights.
    pub(super) fn weights(values: &[Self]) -> Option<Vec<f64>> {
        let weights = values
            .iter()
            .map(|value| value.values[3])
            .collect::<Option<Vec<_>>>()?;
        let original = weights
            .iter()
            .map(|weight| weight.finite().map(FiniteReal::get))
            .collect::<Option<Vec<_>>>();
        if original
            .as_ref()
            .is_some_and(|values| values.iter().all(|value| value.is_normal()))
        {
            return original;
        }
        let minimum = weights.iter().map(|weight| weight.exponent()).min()?;
        let maximum = weights.iter().map(|weight| weight.exponent()).max()?;
        let normal_range = (maximum - 1023, minimum + 1021);
        let full_range = (maximum - 1024, minimum + 1073);
        let (lower, upper) = if normal_range.0 <= normal_range.1 {
            normal_range
        } else {
            full_range
        };
        if lower > upper {
            return None;
        }
        // `lower..=upper` states every binary scale that keeps the complete net
        // inside the `f64` exponent range, and the refusal above states that it
        // is not empty. Zero is the preferred scale because it keeps the source
        // weights. A preferred scale outside the admitted interval is not an
        // error: the nearest admitted scale is then the one that preserves the
        // relative weights.
        let exponent = 0_i32.clamp(lower, upper);
        weights
            .into_iter()
            .map(|weight| {
                weight
                    .rescale(exponent)
                    .map(FiniteReal::get)
                    .filter(|weight| *weight != 0.0)
            })
            .collect()
    }

    /// Subtract the specified weight derivatives, then divide by the base
    /// weight. Repeated corrections express second derivatives without first
    /// multiplying a possibly large first derivative by two.
    pub(super) fn project(
        self,
        base: Self,
        subtract: &[(Self, [FiniteReal; 3])],
    ) -> Option<[FiniteReal; 3]> {
        let denominator = base.values[3]?;
        let mut result = [FiniteReal::ZERO; 3];
        for (axis, coordinate) in result.iter_mut().enumerate() {
            // A constant coordinate divides out exactly, including at f64::MAX.
            if subtract.is_empty() {
                if let Some(value) = self.constant[axis] {
                    *coordinate = value;
                    continue;
                }
            }
            let numerator = if subtract.is_empty() {
                self.values[axis]
            } else {
                let mut sum = ExactSignedSum::default();
                sum.add_scaled_product(self.values[axis], 1.0)?;
                for (weight, factor) in subtract {
                    sum.add_scaled_product(weight.values[3], factor[axis].negated().get())?;
                }
                sum.finish()
            };
            *coordinate = match numerator {
                Some(value) => value.quotient(denominator)?,
                None => FiniteReal::ZERO,
            };
        }
        Some(result)
    }
}
