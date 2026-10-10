// SPDX-License-Identifier: Apache-2.0
//! Actual lower pcurve state retained until requested higher formation.

use super::{basis, decode, higher, DifferentialPoles, EvaluationFailure, PcurveDifferential};
use crate::eval::rational::pcurve::CompletedLower;
use crate::math::Point2;
use crate::scalar::FiniteReal;

/// Real selected basis, captured rows and lower sums borrow their original
/// scratch and immutable source. Drop keeps that scratch live through the
/// destruction of the held backing; it cannot become a detached receipt.
pub(in crate::eval) struct PendingDifferential<'scratch, 'ctx, 'arena, 'source> {
    pub(super) basis: decode::SupportValues<f64>,
    pub(super) captured: Option<basis::polynomial_higher::CapturedBasis<'ctx>>,
    pub(super) lower: PcurveDifferential,
    pub(super) completed: Option<CompletedLower>,
    pub(super) scratch: &'scratch decode::Scratch<'ctx, 'arena>,
    pub(super) knots: &'source [f64],
    pub(super) poles: DifferentialPoles<'source>,
    pub(super) degree: usize,
    pub(super) span: usize,
    pub(super) parameter: FiniteReal,
    pub(super) max_order: Option<usize>,
}

impl PendingDifferential<'_, '_, '_, '_> {
    /// Copy only the completed scalar outcomes while their basis stays held.
    pub(in crate::eval) fn lower(&self) -> PcurveDifferential { self.lower }

    /// Consume the real pending state in its original scratch. A refusal
    /// recorded by an intervening lower operation wins before higher work.
    pub(in crate::eval) fn complete(self)
        -> Result<PcurveDifferential, EvaluationFailure<Point2>>
    {
        self.scratch.unless_refused()?;
        let mut result = self.lower;
        if result.tangent.is_ok() {
            if let Some(captured) = &self.captured {
                let order = self.max_order.unwrap_or(2);
                result.higher = if !self.poles.has_weights() {
                    higher::polynomial(self.scratch, self.knots, self.degree, self.span,
                        self.poles, captured, order)
                } else if let Some(fixed) = higher::quadratic(self.scratch, self.knots,
                    self.poles, self.parameter, order) {
                    fixed
                } else if let Some(lower) = self.completed {
                    crate::eval::rational::pcurve::higher(self.scratch, self.knots, self.degree,
                        self.span, self.poles, lower, captured, order)
                } else { [Err(EvaluationFailure::NoValue); 3] };
            } else if self.degree == 1 && self.poles.has_weights() {
                if let Some(order) = self.max_order.filter(|order| *order >= 3) {
                    result.higher = higher::linear(self.scratch, self.knots, self.span,
                        self.poles, &self.basis, order);
                }
            }
        }
        self.scratch.settle(Ok(result))
    }
}

impl Drop for PendingDifferential<'_, '_, '_, '_> {
    fn drop(&mut self) {
        // These are the actual held allocations. Their destruction precedes
        // captured-row lease refunds and the original scratch lease's Drop.
        drop(std::mem::replace(&mut self.basis,
            decode::SupportValues::Inline { values: [0.0; 2], len: 0 }));
        drop(self.captured.take());
    }
}

#[cfg(test)]
mod tests;
