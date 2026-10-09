// SPDX-License-Identifier: Apache-2.0
//! Homogeneous sums and quotient derivatives with an extended exponent range.
use super::decode;
use crate::features::FinitePoint3;
use crate::math::sum::{product_sum, ExactSignedSum, ProductSum, ScaledValue};
use crate::scalar::FiniteReal;
use cadmpeg_core::decode::ResourceLimit;

/// Cloneable pole traversal. Admission precedes each input-dependent advance.
#[derive(Clone)]
struct SumTerms<'scratch, 'ctx, 'arena, I> {
    terms: I,
    scratch: &'scratch decode::Scratch<'ctx, 'arena>,
    input_sized: bool,
}

impl<I: Iterator> Iterator for SumTerms<'_, '_, '_, I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if self.terms.size_hint().1 == Some(0) {
            return None;
        }
        if self.input_sized {
            self.scratch.work(1, "IR homogeneous pole traversal")?;
        }
        self.terms.next()
    }
}

#[derive(Clone, Copy)]
pub(super) struct Homogeneous {
    values: [Option<ScaledValue>; 4],
    constant: [Option<FiniteReal>; 3],
}

impl Homogeneous {
    /// The identically zero homogeneous derivative of a polynomial whose
    /// degree is lower than the requested derivative order.
    pub(super) fn zero() -> Self {
        Self { values: [None; 4], constant: [None; 3] }
    }

    pub(super) fn sum(
        scratch: &decode::Scratch<'_, '_>,
        terms: impl Iterator<Item = Option<([f64; 2], f64, FinitePoint3)>> + Clone,
    ) -> Result<Option<Self>, ResourceLimit> {
        let input_sized = terms.size_hint().1.is_none_or(|count| count > 2);
        let terms = SumTerms {
            terms,
            scratch,
            input_sized,
        };
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
            scratch.unless_refused()?;
            // `values` carries the same exact zero that `ExactSignedSum::finish`
            // and `add_scaled_product` state as `None`.
            *value = match sum {
                ProductSum::Undefined => return Ok(None),
                ProductSum::Zero => None,
                ProductSum::Value(value) => Some(value),
            };
        }
        let mut constant = [None; 3];
        let mut first = true;
        for term in terms {
            let Some((basis, weight, point)) = term else {
                return Ok(None);
            };
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
        scratch.unless_refused()?;
        Ok(Some(Self { values, constant }))
    }

    /// Sum the four homogeneous derivative lanes in one exact pole walk.
    /// The iterator is the actual rectangular support window. Its exact length
    /// bounds all advances; no terminal probe or replay visits the poles.
    pub(super) fn derivative_sum(
        scratch: &decode::Scratch<'_, '_>,
        mut terms: impl ExactSizeIterator<Item = Option<([f64; 2], f64, FinitePoint3)>>,
    ) -> Result<Option<Self>, ResourceLimit> {
        const OPERATION: &str = "IR homogeneous derivative pole traversal";
        scratch.admission.work(0, OPERATION)?;
        let count = terms.len();
        let input_sized = count > 2;
        let mut sums: [ExactSignedSum; 4] = std::array::from_fn(|_| ExactSignedSum::default());
        for _ in 0..count {
            scratch.admission.work(u64::from(input_sized), OPERATION)?;
            let Some(Some(([u, v], weight, point))) = terms.next() else {
                return Ok(None);
            };
            if FiniteReal::array([u, v, weight]).is_none() {
                return Ok(None);
            }
            for (sum, coordinate) in sums.iter_mut().zip([point.x, point.y, point.z, 1.0]) {
                sum.add_factors([u, v, weight, coordinate]);
            }
        }
        Ok(Some(Self { values: sums.map(ExactSignedSum::finish), constant: [None; 3] }))
    }

    /// Keep source weights when they remain normal. Otherwise choose one
    /// binary scale for the complete output net, preserving relative weights.
    pub(super) fn weights(
        scratch: &decode::Scratch<'_, '_>,
        values: &[Self],
    ) -> Result<Option<Vec<f64>>, ResourceLimit> {
        let result = (|| {
            scratch.work(0, "IR homogeneous weight inspection")?;
            for value in values {
                scratch.work(1, "IR homogeneous weight inspection")?;
                value.values[3]?;
            }
            let mut output = Vec::new();
            scratch.reserve(&mut output, values.len(), "IR homogeneous output weights")?;
            for value in values {
                scratch.work(1, "IR homogeneous weight inspection")?;
                let Some(weight) = value.values[3].and_then(|weight| weight.finite().ok()) else {
                    break;
                };
                scratch.work(std::mem::size_of::<f64>(), "IR homogeneous weight copy")?;
                output.push(weight.get());
            }
            if output.len() == values.len() {
                let mut normal = true;
                for value in &output {
                    scratch.work(1, "IR homogeneous weight inspection")?;
                    if !value.is_normal() {
                        normal = false;
                        break;
                    }
                }
                if normal {
                    return Some(output);
                }
            }
            let mut bounds: Option<(i32, i32)> = None;
            for value in values {
                scratch.work(1, "IR homogeneous weight inspection")?;
                let exponent = value.values[3]?.exponent();
                bounds = Some(match bounds {
                    Some((minimum, maximum)) => (minimum.min(exponent), maximum.max(exponent)),
                    None => (exponent, exponent),
                });
            }
            let (minimum, maximum) = bounds?;
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
            // Select the nearest exponent to zero that preserves the complete net.
            let exponent = if 0 < lower {
                lower
            } else if 0 > upper {
                upper
            } else {
                0
            };
            output.clear();
            for value in values {
                scratch.work(1, "IR homogeneous weight inspection")?;
                let weight = value.values[3]
                    .and_then(|weight| weight.rescale(exponent))
                    .map(FiniteReal::get)
                    .filter(|weight| *weight != 0.0)?;
                scratch.work(std::mem::size_of::<f64>(), "IR homogeneous weight copy")?;
                output.push(weight);
            }
            Some(output)
        })();
        scratch.unless_refused()?;
        Ok(result)
    }

    /// Subtract the specified weight derivatives, then divide by the base
    /// weight. Repeated corrections express second derivatives without first
    /// multiplying a possibly large first derivative by two.
    ///
    /// A zero base weight, and a correction outside the accumulator's range,
    /// leave no value. Each lane is its coordinate, or the signed infinity of
    /// a quotient that overflows.
    pub(super) fn project(
        self,
        base: Self,
        subtract: &[(Self, [FiniteReal; 3])],
    ) -> Option<[Result<FiniteReal, f64>; 3]> {
        let denominator = base.values[3]?;
        let mut lanes = [Ok(FiniteReal::ZERO); 3];
        for (axis, lane) in lanes.iter_mut().enumerate() {
            // A constant coordinate divides out exactly, including at f64::MAX.
            if subtract.is_empty() {
                if let Some(value) = self.constant[axis] {
                    *lane = Ok(value);
                    continue;
                }
            }
            let numerator = if subtract.is_empty() {
                self.values[axis]
            } else {
                let mut sum = ExactSignedSum::default();
                sum.add_scaled_product(self.values[axis], FiniteReal::ONE)?;
                for (weight, factor) in subtract {
                    sum.add_scaled_product(weight.values[3], factor[axis].negated())?;
                }
                sum.finish()
            };
            *lane = numerator.map_or(Ok(FiniteReal::ZERO), |value| value.quotient(denominator));
        }
        Some(lanes)
    }
}

/// The coordinates when every lane is finite. Otherwise the value of every
/// lane: the coordinate, or the signed infinity of a lane that overflows.
pub(super) fn finite_lanes<const N: usize>(
    lanes: [Result<FiniteReal, f64>; N],
) -> Result<[FiniteReal; N], [f64; N]> {
    let mut coordinates = [FiniteReal::ZERO; N];
    for (coordinate, lane) in coordinates.iter_mut().zip(lanes) {
        let Ok(value) = lane else {
            return Err(lanes.map(|lane| lane.map_or_else(|raw| raw, FiniteReal::get)));
        };
        *coordinate = value;
    }
    Ok(coordinates)
}

#[cfg(test)]
mod tests;
