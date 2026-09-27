// SPDX-License-Identifier: Apache-2.0
//! Shared depth account for recursive model evaluation.

use std::cell::{Cell, RefCell};

use cadmpeg_core::decode::WorkBudget;

use super::EvaluationFailure;
use crate::geometry::{Curve, Surface};
use crate::math::Point3;

const MAX_MODEL_EVALUATION_DEPTH: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ModelEvaluationIdentity {
    Curve(*const Curve),
    Surface(*const Surface),
}

thread_local! {
    static MODEL_EVALUATION_DEPTH: Cell<usize> = const { Cell::new(0) };
    static MODEL_EVALUATION_DEPTH_EXCEEDED: Cell<bool> = const { Cell::new(false) };
    static MODEL_EVALUATION_IDENTITIES: RefCell<[Option<ModelEvaluationIdentity>; MAX_MODEL_EVALUATION_DEPTH]> =
        const { RefCell::new([None; MAX_MODEL_EVALUATION_DEPTH]) };
}

/// One curve or surface carrier frame in the current thread's model walk.
pub(super) struct ModelEvaluationDepthGuard {
    slot: usize,
}

impl ModelEvaluationDepthGuard {
    pub(super) fn enter(budget: Option<&WorkBudget<'_>>) -> Option<Self> {
        MODEL_EVALUATION_DEPTH.with(|depth| {
            let current = depth.get();
            if current == 0 {
                MODEL_EVALUATION_DEPTH_EXCEEDED.with(|exceeded| exceeded.set(false));
            }
            if current >= MAX_MODEL_EVALUATION_DEPTH {
                MODEL_EVALUATION_DEPTH_EXCEEDED.with(|exceeded| exceeded.set(true));
                if let Some(budget) = budget {
                    budget.exhaust();
                }
                None
            } else {
                depth.set(current + 1);
                Some(Self { slot: current })
            }
        })
    }

    /// Refuse a carrier reached again in the current model evaluation.
    pub(super) fn bind(
        &self,
        identity: ModelEvaluationIdentity,
        budget: Option<&WorkBudget<'_>>,
    ) -> bool {
        let repeated = MODEL_EVALUATION_IDENTITIES.with(|identities| {
            let mut identities = identities.borrow_mut();
            if identities[..self.slot].contains(&Some(identity)) {
                true
            } else {
                identities[self.slot] = Some(identity);
                false
            }
        });
        if repeated {
            MODEL_EVALUATION_DEPTH_EXCEEDED.with(|exceeded| exceeded.set(true));
            if let Some(budget) = budget {
                budget.exhaust();
            }
        }
        !repeated
    }

    /// A depth refusal in a nested arm exhausts its enclosing work slice.
    pub(super) fn finish_budgeted<T>(
        budget: &WorkBudget<'_>,
        result: Result<T, EvaluationFailure<Point3>>,
    ) -> Result<T, EvaluationFailure<Point3>> {
        if MODEL_EVALUATION_DEPTH_EXCEEDED.with(Cell::get) {
            budget.exhaust();
            Err(EvaluationFailure::NoValue)
        } else {
            result
        }
    }
}

impl Drop for ModelEvaluationDepthGuard {
    fn drop(&mut self) {
        MODEL_EVALUATION_IDENTITIES.with(|identities| {
            identities.borrow_mut()[self.slot] = None;
        });
        MODEL_EVALUATION_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}
