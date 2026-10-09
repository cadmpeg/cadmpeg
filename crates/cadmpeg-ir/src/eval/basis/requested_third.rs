// SPDX-License-Identifier: Apache-2.0
//! Joint local basis orders for a requested rational Third.

use super::super::{decode, difference_quotient};
use crate::math::sum::{ExactSignedSum, ScaledValue};
use crate::scalar::FiniteReal;
use cadmpeg_core::decode::ScopedReservation;

pub(in crate::eval) struct ThirdBasisRows<'ctx> {
    backing: Backing<'ctx>,
}

enum Backing<'ctx> {
    Inline { rows: [[f64; 4]; 4], len: usize },
    Heap(HeapRows<'ctx>),
}

struct HeapRows<'ctx> {
    rows: Vec<[f64; 4]>,
    // The rows die before their genuine temporary reservation.
    _storage: Option<ScopedReservation<'ctx>>,
}

impl<'ctx> ThirdBasisRows<'ctx> {
    fn new(scratch: &decode::Scratch<'ctx, '_>, support: usize) -> Option<Self> {
        if support <= 4 {
            return Some(Self { backing: Backing::Inline { rows: [[0.0; 4]; 4], len: support } });
        }
        let (rows, storage) = scratch.temporary_vec(support, "IR requested curve basis storage")?;
        let mut heap = HeapRows { rows, _storage: storage };
        for _ in 0..support {
            scratch.admission.independent_cost::<()>(Some(1)).ok()?;
            scratch.work(1, "IR requested curve basis initialization")?;
            heap.rows.push([0.0; 4]);
        }
        Some(Self { backing: Backing::Heap(heap) })
    }

    pub(in crate::eval) fn as_slice(&self) -> &[[f64; 4]] {
        match &self.backing {
            Backing::Inline { rows, len } => &rows[..*len],
            Backing::Heap(heap) => &heap.rows,
        }
    }

    fn as_mut_slice(&mut self) -> &mut [[f64; 4]] {
        match &mut self.backing {
            Backing::Inline { rows, len } => &mut rows[..*len],
            Backing::Heap(heap) => &mut heap.rows,
        }
    }
}

/// Four completed local-coordinate basis orders in one degree triangle.
/// Nonzero coefficients lost outside the normal range leave this finite-row
/// representation unavailable. Exact zero coefficients remain exact zero.
pub(in crate::eval) fn rows<'ctx>(
    scratch: &decode::Scratch<'ctx, '_>,
    knots: &[f64],
    degree: usize,
    span: usize,
    t: FiniteReal,
    width: ScaledValue,
) -> Option<ThirdBasisRows<'ctx>> {
    scratch.work(0, "IR requested curve basis boundary")?;
    let support = degree.checked_add(1)?;
    let variable = support > 4;
    let mut lower = ThirdBasisRows::new(scratch, support)?;
    let mut next = ThirdBasisRows::new(scratch, support)?;
    lower.as_mut_slice()[0][0] = 1.0;
    for level in 1..=degree {
        if variable {
            scratch.admission.independent_cost::<()>(Some(1)).ok()?;
            scratch.work(1, "IR requested curve basis row")?;
        }
        let degree_real = cadmpeg_core::convert::f64_from_index(level)?;
        let start = span.checked_sub(level)?;
        for local in 0..=level {
            if variable {
                scratch.admission.independent_cost::<()>(Some(1)).ok()?;
                scratch.work(1, "IR requested curve basis cell")?;
            }
            let index = start.checked_add(local)?;
            let lower_at = |index: usize| index.checked_sub(start + 1)
                .filter(|at| *at < level).map_or([0.0; 4], |at| lower.as_slice()[at]);
            let left = lower_at(index);
            let right = lower_at(index + 1);
            let mut sums: [ExactSignedSum; 4] = std::array::from_fn(|_| ExactSignedSum::default());
            for (values, start_knot, end_knot, left_side) in [
                (left, knots[index], knots[index + level], true),
                (right, knots[index + 1], knots[index + level + 1], false),
            ] {
                let [start_knot, end_knot] = FiniteReal::array([start_knot, end_knot])?;
                if start_knot == end_knot { continue; }
                if values[0] != 0.0 {
                    let (end, start) = if left_side { (t, start_knot) } else { (end_knot, t) };
                    let coefficient = difference_quotient(end, start, end_knot, start_knot).ok()?.get();
                    if end != start && !coefficient.is_normal() { return None; }
                    sums[0].add_factors([coefficient, values[0]]);
                }
                if values[..level.min(3)].iter().any(|value| *value != 0.0) {
                    let mut denominator = ExactSignedSum::default();
                    denominator.add_factors([end_knot.get()]);
                    denominator.add_factors([-start_knot.get()]);
                    let ratio = width.quotient(denominator.finish()?).ok()?.get();
                    if !ratio.is_normal() { return None; }
                    let signed_degree = if left_side { degree_real } else { -degree_real };
                    for order in 1..=level.min(3) {
                        sums[order].add_factors([signed_degree, ratio, values[order - 1]]);
                    }
                }
            }
            let mut values = [0.0; 4];
            for (value, sum) in values.iter_mut().zip(sums) {
                if let Some(sum) = sum.finish() {
                    let coefficient = sum.finite().ok()?.get();
                    if !coefficient.is_normal() { return None; }
                    *value = coefficient;
                }
            }
            next.as_mut_slice()[local] = values;
        }
        std::mem::swap(&mut lower, &mut next);
    }
    Some(lower)
}

#[cfg(test)]
mod tests;
