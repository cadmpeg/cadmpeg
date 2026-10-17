// SPDX-License-Identifier: Apache-2.0
//! The homogeneous sum as it read when each lane, and the constant
//! coordinates, traversed the terms on its own, kept as the reference the
//! shared traversal must reproduce, work charges included.

use super::super::{Homogeneous, SumTerms};
use crate::eval::admission::EvaluationRefusal;
use crate::eval::decode;
use crate::features::FinitePoint3;
use crate::math::sum::{product_sum, ProductSum};

pub(super) fn sum(
    scratch: &decode::Scratch<'_, '_>,
    terms: impl Iterator<Item = Option<([f64; 2], f64, FinitePoint3)>> + Clone,
) -> Result<Option<Homogeneous>, EvaluationRefusal> {
    let input_sized = terms.size_hint().1.is_none_or(|count| count > 2);
    let terms = SumTerms {
        terms,
        scratch,
        input_sized,
        charged: 0,
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
    Ok(Some(Homogeneous { values, constant }))
}
