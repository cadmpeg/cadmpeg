// SPDX-License-Identifier: Apache-2.0
//! Session depth and distinct cycle admission for model evaluation.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;

use cadmpeg_core::decode::{
    DecodeContext, DepthGuard, ResourceDimension, ResourceFailure, ResourceLimit, WorkBudget,
    WorkBudgetRecursionGuard,
};

use super::{admission::EvaluationAdmission, EvaluationFailure};
use crate::geometry::{Curve, Surface};

const INDEPENDENT_MODEL_EVALUATION_DEPTH: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ModelEvaluationIdentity {
    Curve(*const Curve),
    Surface(*const Surface),
}

/// The carriers on the active evaluation path: one slot per frame, and the
/// set of identities those slots bind.
struct IdentityPath {
    frames: Vec<Option<ModelEvaluationIdentity>>,
    active: HashSet<ModelEvaluationIdentity>,
}

impl IdentityPath {
    /// Unbind and drop every frame from `base` on. An outer frame dropped
    /// first has already removed this one.
    fn truncate(&mut self, base: usize) {
        if base >= self.frames.len() {
            return;
        }
        for identity in self.frames.drain(base..).flatten() {
            self.active.remove(&identity);
        }
    }
}

thread_local! {
    static MODEL_EVALUATION_CYCLE: Cell<bool> = const { Cell::new(false) };
    static MODEL_EVALUATION_REFUSAL: Cell<Option<ResourceLimit>> = const { Cell::new(None) };
    static MODEL_EVALUATION_IDENTITIES: RefCell<IdentityPath> = RefCell::new(IdentityPath {
        frames: Vec::new(),
        active: HashSet::new(),
    });
}

/// One carrier frame and the path length below it.
pub(super) struct ModelEvaluationDepthGuard<'a> {
    base: usize,
    _depth: FrameDepth<'a>,
}

enum FrameDepth<'a> {
    Session {
        _guard: DepthGuard<'a>,
    },
    Slice {
        _guard: WorkBudgetRecursionGuard<'a>,
    },
    Independent,
}

impl<'a> ModelEvaluationDepthGuard<'a> {
    /// A session admission nests in the decode context; an independent work
    /// slice and the standard admission have a 256-frame ceiling.
    pub(super) fn enter(admission: EvaluationAdmission<'a, '_>) -> Result<Self, ResourceLimit> {
        let result = MODEL_EVALUATION_IDENTITIES.with(|path| {
            let mut path = path.borrow_mut();
            let identities = &mut path.frames;
            if identities.is_empty() {
                MODEL_EVALUATION_CYCLE.with(|cycle| cycle.set(false));
                MODEL_EVALUATION_REFUSAL.with(|refusal| refusal.set(None));
            }
            let depth = match (admission.context(), admission.work_slice()) {
                (Some(context), _) => FrameDepth::Session {
                    _guard: context.enter_nested_limit("model evaluation recursion")?,
                },
                (None, Some(budget)) => FrameDepth::Slice {
                    _guard: budget.recursion_guard().map_err(|_| ResourceLimit {
                        dimension: ResourceDimension::RecursionDepth,
                        reason: ResourceFailure::BudgetExceeded,
                        limit: cadmpeg_core::decode::u64_from_index(WorkBudget::RECURSION_DEPTH),
                        used: cadmpeg_core::decode::u64_from_index(WorkBudget::RECURSION_DEPTH),
                        additional: 1,
                        operation: "work_budget_recursion",
                    })?,
                },
                (None, None) => {
                    if identities.len() >= INDEPENDENT_MODEL_EVALUATION_DEPTH {
                        return Err(ResourceLimit {
                            dimension: ResourceDimension::RecursionDepth,
                            reason: ResourceFailure::BudgetExceeded,
                            limit: cadmpeg_core::decode::u64_from_index(
                                INDEPENDENT_MODEL_EVALUATION_DEPTH,
                            ),
                            used: cadmpeg_core::decode::u64_from_index(identities.len()),
                            additional: 1,
                            operation: "independent model evaluation recursion",
                        });
                    }
                    FrameDepth::Independent
                }
            };
            let base = identities.len();
            identities.push(None);
            Ok(Self {
                base,
                _depth: depth,
            })
        });
        result.inspect_err(|&limit| {
            MODEL_EVALUATION_REFUSAL.with(|refusal| {
                if refusal.get().is_none() {
                    refusal.set(Some(limit));
                }
            });
        })
    }

    /// A repeated carrier is a cycle, independent of the session depth limit.
    pub(super) fn bind(
        &self,
        identity: ModelEvaluationIdentity,
        admission: EvaluationAdmission<'_, '_>,
    ) -> bool {
        let repeated = MODEL_EVALUATION_IDENTITIES.with(|path| {
            let mut path = path.borrow_mut();
            let IdentityPath { frames, active } = &mut *path;
            let Some(slot) = frames.get_mut(self.base) else {
                return false;
            };
            if *slot == Some(identity) {
                return false;
            }
            if active.contains(&identity) {
                return true;
            }
            if let Some(previous) = slot.replace(identity) {
                active.remove(&previous);
            }
            active.insert(identity);
            false
        });
        if repeated {
            MODEL_EVALUATION_CYCLE.with(|cycle| cycle.set(true));
            if let Some(budget) = admission.independent_work_slice() {
                budget.exhaust();
            }
        }
        !repeated
    }

    /// Preserve the first resource refusal across evaluator fallback branches.
    pub(super) fn finish_sliced<T, R>(
        admission: EvaluationAdmission<'_, '_>,
        result: Result<T, EvaluationFailure<R>>,
    ) -> Result<T, EvaluationFailure<R>> {
        // A session refusal can pass through an optional evaluator branch.
        if let Some(limit) = admission
            .context()
            .and_then(DecodeContext::resource_refusal)
            .or_else(|| MODEL_EVALUATION_REFUSAL.with(Cell::get))
        {
            Err(EvaluationFailure::ResourceLimit(limit))
        } else if MODEL_EVALUATION_CYCLE.with(Cell::get) {
            if let Some(budget) = admission.independent_work_slice() {
                budget.exhaust();
            }
            Err(EvaluationFailure::NoValue)
        } else {
            result
        }
    }
}

impl Drop for ModelEvaluationDepthGuard<'_> {
    fn drop(&mut self) {
        MODEL_EVALUATION_IDENTITIES.with(|path| path.borrow_mut().truncate(self.base));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EvaluationAdmission, ModelEvaluationDepthGuard, ModelEvaluationIdentity, ResourceDimension,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    #[test]
    fn model_depth_preserves_the_zero_session_refusal() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(limit) = ModelEvaluationDepthGuard::enter(EvaluationAdmission::Decode(&ctx)) else {
            panic!("zero session depth must refuse");
        };
        assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
        assert_eq!(limit.limit, 0);
        assert_eq!(limit.used, 0);
        assert_eq!(limit.additional, 1);
        assert!(
            matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit)
        );
    }

    #[test]
    fn attached_model_depth_uses_the_session_ceiling_above_256() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 300;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut guards = Vec::new();
        for _ in 0..300 {
            guards
                .push(ModelEvaluationDepthGuard::enter(EvaluationAdmission::Decode(&ctx)).unwrap());
        }
        let Err(limit) = ModelEvaluationDepthGuard::enter(EvaluationAdmission::Decode(&ctx)) else {
            panic!("session ceiling must refuse");
        };
        assert_eq!(limit.limit, 300);
        assert_eq!(limit.used, 300);
        while let Some(guard) = guards.pop() {
            drop(guard);
        }
    }

    #[test]
    fn deep_identity_paths_enter_and_bind_in_linear_time() {
        // Copying the path into each frame would move 2^31 identities.
        const DEPTH: usize = 1 << 16;
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = u64::try_from(DEPTH).unwrap();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let admission = EvaluationAdmission::Decode(&ctx);
        let carrier = |index: usize| {
            ModelEvaluationIdentity::Curve(
                std::ptr::NonNull::<crate::geometry::Curve>::dangling()
                    .as_ptr()
                    .wrapping_add(index),
            )
        };
        let mut guards = Vec::with_capacity(DEPTH);
        for index in 0..DEPTH - 1 {
            let guard = ModelEvaluationDepthGuard::enter(admission).unwrap();
            assert!(guard.bind(carrier(index), admission));
            guards.push(guard);
        }
        let top = ModelEvaluationDepthGuard::enter(admission).unwrap();
        assert!(!top.bind(carrier(0), admission));
        assert!(top.bind(carrier(DEPTH), admission));
        drop(top);
        // A dropped frame releases its carrier to the next frame.
        drop(guards.pop());
        let again = ModelEvaluationDepthGuard::enter(admission).unwrap();
        assert!(again.bind(carrier(DEPTH - 2), admission));
        assert!(!again.bind(carrier(DEPTH - 3), admission));
        drop(again);
        while let Some(guard) = guards.pop() {
            drop(guard);
        }
        drop(guards);
        let fresh = ModelEvaluationDepthGuard::enter(admission).unwrap();
        assert!(fresh.bind(carrier(0), admission));
        drop(fresh);
        assert!(ctx.finish_session().is_ok());
    }
}
