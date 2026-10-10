// SPDX-License-Identifier: Apache-2.0
//! Closed second-through-fifth composition of a surface with a parameter curve.

use super::six_product::Numerator;
use super::{scaled_finite, ScaledValue};
use crate::features::FiniteVector3;
use crate::scalar::{FiniteReal, NonZeroReal};
use crate::units::FinitePoint2;

const FACTORIAL: [usize; 6] = [1, 1, 2, 6, 24, 120];

/// The actual order-n chain rule. Source rows are ordered by degree1..5,
/// then increasing v-order. Each pcurve row states its actual derivative.
/// Unused row tails are not read. No point or derivative is sampled again.
pub(crate) fn contact_derivative<const N: usize>(
    partials: [[FiniteVector3; 6]; 5],
    pcurve: [[FiniteReal; 2]; 5],
) -> Option<[Result<FiniteReal, f64>; 3]> {
    const { assert!(N >= 2 && N <= 5); }
    let unit = ScaledValue::of_nonzero(NonZeroReal::ONE);
    let mut output = [Ok(FiniteReal::ZERO); 3];
    for (coordinate, result) in output.iter_mut().enumerate() {
        let mut sum = ChainSum { numerator: Numerator::new(), pcurve: &pcurve, unit };
        for degree in 1..=N {
            for v_order in 0..=degree {
                let vector = partials[degree - 1][v_order].get();
                let component = [vector.x, vector.y, vector.z][coordinate];
                sum.add_partitions::<N>(component, &mut [0; 5],
                    0, N, degree - v_order, degree)?;
            }
        }
        // All source inputs are finite. The integer chain-rule term count
        // is sum S(N,m)*2^m, at most454 at N=5; six factors and454 copies
        // lie inside the owner's original1082-copy exponent/carry bound.
        *result = sum.numerator.divide(unit, [unit; 5])?;
    }
    Some(output)
}

/// A line pcurve removes every lower support-order term by its source law.
/// Only the requested support row and the real line direction are read.
pub(crate) fn directional_derivative<const N: usize>(
    partials: [FiniteVector3; 6],
    direction: FinitePoint2,
) -> Option<[Result<FiniteReal, f64>; 3]> {
    const { assert!(N >= 2 && N <= 5); }
    const BINOMIAL: [[usize; 6]; 6] = [
        [1, 0, 0, 0, 0, 0], [1, 1, 0, 0, 0, 0], [1, 2, 1, 0, 0, 0],
        [1, 3, 3, 1, 0, 0], [1, 4, 6, 4, 1, 0], [1, 5, 10, 10, 5, 1],
    ];
    let unit = ScaledValue::of_nonzero(NonZeroReal::ONE);
    let direction = direction.coordinates().map(|value| scaled_finite(value.get()));
    let mut output = [Ok(FiniteReal::ZERO); 3];
    for (coordinate, result) in output.iter_mut().enumerate() {
        let mut sum = Numerator::new();
        for v_order in 0..=N {
            let vector = partials[v_order].get();
            let mut factors = [Some(unit); 6];
            factors[0] = scaled_finite([vector.x, vector.y, vector.z][coordinate]);
            for (at, factor) in factors[1..=N].iter_mut().enumerate() {
                *factor = direction[usize::from(at >= N - v_order)];
            }
            sum.add_product(factors, false, BINOMIAL[N][v_order])?;
        }
        // The total binomial multiplicity is2^N, at most32. Every factor
        // is a genuine finite source coordinate or a padding unit.
        *result = sum.divide(unit, [unit; 5])?;
    }
    Some(output)
}

struct ChainSum<'a> {
    numerator: Numerator,
    pcurve: &'a [[FiniteReal; 2]; 5],
    unit: ScaledValue,
}

impl ChainSum<'_> {
fn add_partitions<const N: usize>(
    &mut self,
    component: f64,
    orders: &mut [usize; 5],
    slot: usize,
    remaining: usize,
    u_slots: usize,
    slots: usize,
) -> Option<()> {
    if slot == slots {
        if remaining != 0 { return Some(()); }
        let mut denominator = 1;
        let mut multiplicities = [[0_usize; 6]; 2];
        let mut factors = [Some(self.unit); 6];
        factors[0] = scaled_finite(component);
        for at in 0..slots {
            let axis = usize::from(at >= u_slots);
            let order = orders[at];
            denominator *= FACTORIAL[order];
            multiplicities[axis][order] += 1;
            factors[at + 1] = scaled_finite(self.pcurve[order - 1][axis].get());
        }
        for axis in multiplicities {
            for count in axis { denominator *= FACTORIAL[count]; }
        }
        // A partition of N labelled differentiations has this integer
        // multiplicity. N<=5 bounds every product by5!=120.
        return self.numerator.add_product(factors, false, FACTORIAL[N] / denominator);
    }
    // Sort the derivative orders within each equal-axis group. Starting
    // the v group resets the lower bound, so each coloured partition occurs
    // once. The remaining slots each need at least one differentiation.
    let lower = if slot != 0 && slot != u_slots { orders[slot - 1] } else { 1 };
    for order in lower..=remaining - (slots - slot - 1) {
        orders[slot] = order;
        self.add_partitions::<N>(component, orders, slot + 1,
            remaining - order, u_slots, slots)?;
    }
    Some(())
}

}

#[cfg(test)]
mod tests;
