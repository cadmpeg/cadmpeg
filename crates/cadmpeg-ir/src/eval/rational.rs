// SPDX-License-Identifier: Apache-2.0
//! Homogeneous sums and quotient derivatives with an extended exponent range.
use super::admission::EvaluationRefusal;
use super::decode;
use crate::features::FinitePoint3;
use crate::math::sum::{ExactSignedSum, LaneProductSums, LaneSum, ScaledValue};
use crate::scalar::FiniteReal;

/// Cloneable pole traversal. Admission precedes each input-dependent advance.
#[derive(Clone)]
struct SumTerms<'scratch, 'ctx, 'arena, I> {
    terms: I,
    scratch: &'scratch decode::Scratch<'ctx, 'arena>,
    input_sized: bool,
    /// The work steps this traversal has charged.
    charged: usize,
}

impl<I: Iterator> Iterator for SumTerms<'_, '_, '_, I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if self.terms.size_hint().1 == Some(0) {
            return None;
        }
        if self.input_sized {
            self.scratch.work(1)?;
            self.charged += 1;
        }
        self.terms.next()
    }
}

/// Charge a traversal of the terms that a shared traversal has already made:
/// `work` steps, the count one traversal charged.
fn charge_traversal(
    scratch: &decode::Scratch<'_, '_>,
    work: usize,
) -> Result<(), EvaluationRefusal> {
    if work > 0 {
        // A refused charge is recorded in `scratch`, which `unless_refused`
        // states.
        scratch.work(work);
    }
    scratch.unless_refused()
}

#[derive(Clone, Copy)]
pub(super) struct Homogeneous {
    values: [Option<ScaledValue>; 4],
    constant: [Option<FiniteReal>; 3],
}

impl Homogeneous {
    /// The four lanes are summed as `product_sum` sums each, and a constant
    /// coordinate is one every term with a nonzero blend shares.
    ///
    /// One traversal forms the f64 pass of every lane and the constant
    /// coordinates; a lane that `product_sum` would replay is replayed through
    /// the exact accumulator. Work is charged as for one traversal per lane,
    /// one per replay and one for the constant coordinates, in that order, so
    /// the budget and the point of any refusal do not depend on how the
    /// traversals are shared.
    pub(super) fn sum(
        scratch: &decode::Scratch<'_, '_>,
        terms: impl Iterator<Item = Option<([f64; 2], f64, FinitePoint3)>> + Clone,
    ) -> Result<Option<Self>, EvaluationRefusal> {
        let input_sized = terms.size_hint().1.is_none_or(|count| count > 2);
        let terms = SumTerms {
            terms,
            scratch,
            input_sized,
            charged: 0,
        };
        let mut traversal = terms.clone();
        let mut lanes = LaneProductSums::<4>::default();
        let mut constant = [None; 3];
        let mut first = true;
        for term in traversal.by_ref() {
            // A missing pole or a factor that is not finite leaves the first
            // lane undefined, and `product_sum` stops there.
            let Some((basis, weight, point)) = term else {
                scratch.unless_refused()?;
                return Ok(None);
            };
            let shared = [basis[0], basis[1], weight];
            if lanes
                .add(shared, [point.x, point.y, point.z, 1.0])
                .is_none()
            {
                scratch.unless_refused()?;
                return Ok(None);
            }
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
        let traversal_work = traversal.charged;
        let mut values = [None; 4];
        for (axis, (value, lane)) in values.iter_mut().zip(lanes.finish()).enumerate() {
            if axis > 0 {
                charge_traversal(scratch, traversal_work)?;
            }
            // `values` carries the same exact zero that `ExactSignedSum::finish`
            // and `add_scaled_product` state as `None`.
            *value = match lane {
                LaneSum::Settled(value) => value,
                LaneSum::Replay => {
                    let mut sum = ExactSignedSum::default();
                    for term in terms.clone() {
                        let Some((basis, weight, point)) = term else {
                            scratch.unless_refused()?;
                            return Ok(None);
                        };
                        sum.add_factors([
                            basis[0],
                            basis[1],
                            weight,
                            [point.x, point.y, point.z, 1.0][axis],
                        ]);
                    }
                    scratch.unless_refused()?;
                    sum.finish()
                }
            };
        }
        charge_traversal(scratch, traversal_work)?;
        Ok(Some(Self { values, constant }))
    }

    /// Keep source weights when they remain normal. Otherwise choose one
    /// binary scale for the complete output net, preserving relative weights.
    pub(super) fn weights(
        scratch: &decode::Scratch<'_, '_>,
        values: &[Self],
    ) -> Result<Option<Vec<f64>>, EvaluationRefusal> {
        let result = (|| {
            scratch.work(0)?;
            for value in values {
                scratch.work(1)?;
                value.values[3]?;
            }
            let mut output = Vec::new();
            for value in values {
                scratch.work(1)?;
                let Some(weight) = value.values[3].and_then(|weight| weight.finite().ok()) else {
                    break;
                };
                scratch.work(std::mem::size_of::<f64>())?;
                output.push(weight.get());
            }
            if output.len() == values.len() {
                let mut normal = true;
                for value in &output {
                    scratch.work(1)?;
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
                scratch.work(1)?;
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
                scratch.work(1)?;
                let weight = value.values[3]
                    .and_then(|weight| weight.rescale(exponent))
                    .map(FiniteReal::get)
                    .filter(|weight| *weight != 0.0)?;
                scratch.work(std::mem::size_of::<f64>())?;
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
