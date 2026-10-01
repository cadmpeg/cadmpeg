// SPDX-License-Identifier: Apache-2.0
//! Session depth and distinct cycle admission for model evaluation.

use std::cell::{Cell, RefCell};

use cadmpeg_core::decode::{ResourceDimension, ResourceFailure, ResourceLimit, WorkBudget, WorkBudgetRecursionGuard};

use cadmpeg_core::decode::work_scratch::WorkScratch;

use super::EvaluationFailure;
use crate::geometry::{Curve, Surface};
use crate::math::Point3;

const INDEPENDENT_MODEL_EVALUATION_DEPTH: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ModelEvaluationIdentity {
    Curve(*const Curve),
    Surface(*const Surface),
}

thread_local! {
    static MODEL_EVALUATION_CYCLE: Cell<bool> = const { Cell::new(false) };
    static MODEL_EVALUATION_REFUSAL: Cell<Option<ResourceLimit>> = const { Cell::new(None) };
    static MODEL_EVALUATION_IDENTITIES: RefCell<Vec<Option<ModelEvaluationIdentity>>> = const { RefCell::new(Vec::new()) };
}

/// One carrier frame and the temporary identity path it owns.
pub(super) struct ModelEvaluationDepthGuard<'budget, 'session> {
    previous: Vec<Option<ModelEvaluationIdentity>>,
    _depth: Option<WorkBudgetRecursionGuard<'budget, 'session>>,
    _storage: WorkScratch<'session>,
}

impl<'budget, 'session> ModelEvaluationDepthGuard<'budget, 'session> {
    pub(super) fn enter(budget: Option<&'budget WorkBudget<'session>>) -> Result<Self, ResourceLimit> {
        let result = MODEL_EVALUATION_IDENTITIES.with(|identities| {
            let mut identities = identities.borrow_mut();
            if identities.is_empty() {
                MODEL_EVALUATION_CYCLE.with(|cycle| cycle.set(false));
                MODEL_EVALUATION_REFUSAL.with(|refusal| refusal.set(None));
            }
            let depth = if let Some(budget) = budget {
                Some(budget.recursion_guard()?)
            } else {
                if identities.len() >= INDEPENDENT_MODEL_EVALUATION_DEPTH {
                    return Err(ResourceLimit {
                        dimension: ResourceDimension::RecursionDepth,
                        reason: ResourceFailure::BudgetExceeded,
                        limit: cadmpeg_core::decode::u64_from_index(INDEPENDENT_MODEL_EVALUATION_DEPTH),
                        used: cadmpeg_core::decode::u64_from_index(identities.len()),
                        additional: 1,
                        operation: "independent model evaluation recursion",
                    });
                }
                None
            };
            let (mut current, storage) = match budget {
                Some(budget) => budget.copy_recursion_path(&identities)?,
                None => WorkBudget::new(usize::MAX).copy_recursion_path(&identities)?,
            };
            current.push(None);
            let previous = std::mem::replace(&mut *identities, current);
            Ok(Self { previous, _depth: depth, _storage: storage })
        });
        result.map_err(|limit| {
            MODEL_EVALUATION_REFUSAL.with(|refusal| {
                if refusal.get().is_none() { refusal.set(Some(limit)); }
            });
            limit
        })
    }

    /// A repeated carrier is a cycle, independent of the session depth limit.
    pub(super) fn bind(&self, identity: ModelEvaluationIdentity, budget: Option<&WorkBudget<'_>>) -> bool {
        let repeated = self.previous.contains(&Some(identity));
        if repeated {
            MODEL_EVALUATION_CYCLE.with(|cycle| cycle.set(true));
            if let Some(budget) = budget { budget.exhaust(); }
        } else {
            MODEL_EVALUATION_IDENTITIES.with(|identities| {
                if let Some(slot) = identities.borrow_mut().last_mut() { *slot = Some(identity); }
            });
        }
        !repeated
    }

    /// Preserve the first resource refusal across evaluator fallback branches.
    pub(super) fn finish_budgeted<T>(budget: &WorkBudget<'_>, result: Result<T, EvaluationFailure<Point3>>) -> Result<T, EvaluationFailure<Point3>> {
        if let Some(limit) = MODEL_EVALUATION_REFUSAL.with(Cell::get) {
            Err(EvaluationFailure::ResourceLimit(limit))
        } else if MODEL_EVALUATION_CYCLE.with(Cell::get) {
            budget.exhaust();
            Err(EvaluationFailure::NoValue)
        } else { result }
    }
}

impl Drop for ModelEvaluationDepthGuard<'_, '_> {
    fn drop(&mut self) {
        MODEL_EVALUATION_IDENTITIES.with(|identities| {
            drop(std::mem::replace(&mut *identities.borrow_mut(), std::mem::take(&mut self.previous)));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{ModelEvaluationDepthGuard, ResourceDimension};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    #[test]
    fn model_depth_preserves_the_zero_session_refusal() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let budget = ctx.work_budget(100);
        let Err(limit) = ModelEvaluationDepthGuard::enter(Some(&budget)) else { panic!("zero session depth must refuse"); };
        assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
        assert_eq!(limit.limit, 0);
        assert_eq!(limit.used, 0);
        assert_eq!(limit.additional, 1);
        assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit));
    }

    #[test]
    fn attached_model_depth_uses_the_session_ceiling_above_256() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 300;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let budget = ctx.work_budget(100_000);
        let mut guards = Vec::new();
        for _ in 0..300 { guards.push(ModelEvaluationDepthGuard::enter(Some(&budget)).unwrap()); }
        let Err(limit) = ModelEvaluationDepthGuard::enter(Some(&budget)) else { panic!("session ceiling must refuse"); };
        assert_eq!(limit.limit, 300);
        assert_eq!(limit.used, 300);
        while let Some(guard) = guards.pop() { drop(guard); }
    }
}
