// SPDX-License-Identifier: Apache-2.0
//! Resource policy for geometry evaluation.

use std::cell::Cell;

use cadmpeg_core::decode::{DecodeContext, DepthGuard, ResourceDimension, ResourceFailure, ResourceLimit, u64_from_index};

const INDEPENDENT_EVALUATION_DEPTH: usize = 256;

/// Evaluation uses the caller's decode session or standard-library storage.
#[derive(Clone, Copy)]
pub enum EvaluationAdmission<'ctx, 'arena> {
    /// Charge storage, work and nesting to this live decode context.
    Decode(&'ctx DecodeContext<'arena>),
    /// Use fallible standard-library storage and an independent depth limit of 256.
    Standard,
}

impl<'ctx, 'arena> From<&'ctx DecodeContext<'arena>> for EvaluationAdmission<'ctx, 'arena> {
    fn from(context: &'ctx DecodeContext<'arena>) -> Self { Self::Decode(context) }
}

impl<'ctx, 'arena> EvaluationAdmission<'ctx, 'arena> {
    pub(super) fn work(self, count: u64, operation: &'static str) -> Result<(), ResourceLimit> {
        match self {
            Self::Decode(context) => context.charge_work_limit(count, operation),
            Self::Standard => Ok(()),
        }
    }

    pub(super) fn enter<'scratch>(self, depth: &'scratch Cell<usize>) -> Result<EvaluationDepthGuard<'scratch>, ResourceLimit>
    where 'ctx: 'scratch {
        match self {
            Self::Decode(context) => context.enter_nested_limit("geometry evaluation nesting")
                .map(|guard| EvaluationDepthGuard::Decode { _guard: guard }),
            Self::Standard => {
                if depth.get() >= INDEPENDENT_EVALUATION_DEPTH {
                    return Err(ResourceLimit {
                        dimension: ResourceDimension::RecursionDepth,
                        reason: ResourceFailure::BudgetExceeded,
                        limit: u64_from_index(INDEPENDENT_EVALUATION_DEPTH),
                        used: u64_from_index(depth.get()),
                        additional: 1,
                        operation: "independent geometry evaluation nesting",
                    });
                }
                depth.set(depth.get() + 1);
                Ok(EvaluationDepthGuard::Standard { depth })
            }
        }
    }
}

pub(super) enum EvaluationDepthGuard<'scratch> {
    Decode { _guard: DepthGuard<'scratch> },
    Standard { depth: &'scratch Cell<usize> },
}

impl Drop for EvaluationDepthGuard<'_> {
    fn drop(&mut self) {
        if let Self::Standard { depth } = self { depth.set(depth.get() - 1); }
    }
}
