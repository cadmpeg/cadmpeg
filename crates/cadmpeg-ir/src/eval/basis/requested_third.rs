// SPDX-License-Identifier: Apache-2.0
//! Joint local basis orders for requested rational higher derivatives.

use super::super::{decode, difference_quotient};
use crate::math::sum::{ExactSignedSum, ScaledValue};
use crate::scalar::FiniteReal;
use cadmpeg_core::decode::ScopedReservation;

pub(in crate::eval) struct BasisRows<'ctx, const N: usize> {
    backing: Backing<'ctx, N>,
    fourth_available: bool,
    fifth_available: bool,
}

enum Backing<'ctx, const N: usize> {
    Inline { rows: [[f64; N]; 4], len: usize },
    Heap(HeapRows<'ctx, N>),
}

struct HeapRows<'ctx, const N: usize> {
    rows: Vec<[f64; N]>,
    // The rows die before their genuine temporary reservation.
    _storage: Option<ScopedReservation<'ctx>>,
}

impl<'ctx, const N: usize> BasisRows<'ctx, N> {
    fn new(scratch: &decode::Scratch<'ctx, '_>, support: usize) -> Option<Self> {
        if support <= 4 {
            return Some(Self { backing: Backing::Inline { rows: [[0.0; N]; 4], len: support }, fourth_available: N >= 5, fifth_available: N == 6 });
        }
        let (rows, storage) = scratch.temporary_vec(support, "IR requested curve basis storage")?;
        let mut heap = HeapRows { rows, _storage: storage };
        for _ in 0..support {
            scratch.admission.independent_cost::<()>(Some(1)).ok()?;
            scratch.work(1, "IR requested curve basis initialization")?;
            heap.rows.push([0.0; N]);
        }
        Some(Self { backing: Backing::Heap(heap), fourth_available: N >= 5, fifth_available: N == 6 })
    }

    pub(in crate::eval) fn as_slice(&self) -> &[[f64; N]] {
        match &self.backing {
            Backing::Inline { rows, len } => &rows[..*len],
            Backing::Heap(heap) => &heap.rows,
        }
    }

    pub(in crate::eval) fn fourth_available(&self) -> bool { self.fourth_available }

    pub(in crate::eval) fn fifth_available(&self) -> bool { self.fifth_available }

    fn as_mut_slice(&mut self) -> &mut [[f64; N]] {
        match &mut self.backing {
            Backing::Inline { rows, len } => &mut rows[..*len],
            Backing::Heap(heap) => &mut heap.rows,
        }
    }
}

/// Four, five or six local-coordinate basis orders in one degree triangle.
/// Loss of a higher lane leaves the completed lower orders intact.
/// Nonzero coefficients lost outside the normal range leave this finite-row
/// representation unavailable. Exact zero coefficients remain exact zero.
pub(in crate::eval) fn rows<'ctx, const N: usize>(
    scratch: &decode::Scratch<'ctx, '_>,
    knots: &[f64],
    degree: usize,
    span: usize,
    t: FiniteReal,
    width: ScaledValue,
) -> Option<BasisRows<'ctx, N>> {
    const { assert!(N == 4 || N == 5 || N == 6) };
    scratch.work(0, "IR requested curve basis boundary")?;
    let support = degree.checked_add(1)?;
    let variable = support > 4;
    let mut lower = BasisRows::new(scratch, support)?;
    let mut next = BasisRows::new(scratch, support)?;
    lower.as_mut_slice()[0][0] = 1.0;
    for level in 1..=degree {
        // D4 at this degree reads only the completed D3 of the lower degree.
        // A lost lower D4 does not constrain this independently computed row.
        next.fourth_available = N >= 5;
        // D5 reads the lower degree's D4. An unavailable D4 is not zero.
        // Below degree5 the fifth derivative is identically zero.
        next.fifth_available = N == 6 && (level < 5 || lower.fourth_available);
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
                .filter(|at| *at < level).map_or([0.0; N], |at| lower.as_slice()[at]);
            let left = lower_at(index);
            let right = lower_at(index + 1);
            let mut sums: [ExactSignedSum; N] = std::array::from_fn(|_| ExactSignedSum::default());
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
                let lower_needed = values[..level.min(3)].iter().any(|value| *value != 0.0);
                let fourth_needed = N >= 5 && next.fourth_available && level >= 4 && values[3] != 0.0;
                let fifth_needed = N == 6 && next.fifth_available && level >= 5 && values[4] != 0.0;
                if lower_needed || fourth_needed || fifth_needed {
                    let mut denominator = ExactSignedSum::default();
                    denominator.add_factors([end_knot.get()]);
                    denominator.add_factors([-start_knot.get()]);
                    let ratio = denominator.finish().and_then(|denominator| width.quotient(denominator).ok())
                        .map(FiniteReal::get).filter(|ratio| ratio.is_normal());
                    let Some(ratio) = ratio else {
                        if lower_needed { return None; }
                        if fourth_needed { next.fourth_available = false; }
                        if fifth_needed { next.fifth_available = false; }
                        continue;
                    };
                    let signed_degree = if left_side { degree_real } else { -degree_real };
                    for order in 1..=level.min(3) {
                        sums[order].add_factors([signed_degree, ratio, values[order - 1]]);
                    }
                    if fourth_needed { sums[4].add_factors([signed_degree, ratio, values[3]]); }
                    if fifth_needed { sums[5].add_factors([signed_degree, ratio, values[4]]); }
                }
            }
            let mut values = [0.0; N];
            for (order, (value, sum)) in values.iter_mut().zip(sums).enumerate() {
                if order == 4 && !next.fourth_available { continue; }
                if order == 5 && !next.fifth_available { continue; }
                if let Some(sum) = sum.finish() {
                    let coefficient = sum.finite().ok().map(FiniteReal::get)
                        .filter(|coefficient| coefficient.is_normal());
                    let Some(coefficient) = coefficient else {
                        if order < 4 { return None; }
                        if order == 4 { next.fourth_available = false; }
                        else { next.fifth_available = false; }
                        continue;
                    };
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
