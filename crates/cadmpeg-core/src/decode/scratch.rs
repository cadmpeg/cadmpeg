// SPDX-License-Identifier: Apache-2.0
//! Independent resource accounting for optional decode evidence.

use std::ops::Deref;

use crate::CodecError;

use super::budget::DecodeBudget;
use super::{u64_from_index, DecodeArena, DecodeContext, ResourceLimit};

/// The context of work opened with [`DecodeContext::scratch`].
///
/// Its counters and arena belong to the evidence operation.
#[derive(Debug)]
pub struct ScratchContext<'a> {
    ctx: DecodeContext<'a>,
}

impl<'a> Deref for ScratchContext<'a> {
    type Target = DecodeContext<'a>;

    fn deref(&self) -> &DecodeContext<'a> {
        &self.ctx
    }
}

impl DecodeContext<'_> {
    /// Runs optional evidence work with an independent budget and arena.
    ///
    /// `input_bytes` is the measured length of the admitted carrier, including
    /// expanded input. It sets the input-proportional allowances. The policy
    /// caps further input reads. Local charges and refusals end with the work.
    /// The caller admits any kept output separately. Owned results must refer
    /// only to source spaces that were registered outside this operation.
    ///
    /// Returns `Ok(Err(limit))` when a resource refuses inside `work`.
    ///
    /// # Errors
    ///
    /// A refusal on the parent, a parent budget or space change inside
    /// `work`, or a non-resource error returned by `work`.
    pub fn scratch<T>(
        &self,
        input_bytes: usize,
        work: impl for<'s> FnOnce(&ScratchContext<'s>) -> Result<T, CodecError>,
    ) -> Result<Result<T, ResourceLimit>, CodecError> {
        if let Some(limit) = self.resource_refusal() {
            return Err(limit.into());
        }
        let entry = self.budget.state();
        let entry_spaces = self.derived_spaces.get();
        let arena = DecodeArena::new();
        let mut local = DecodeContext::new(&arena, &entry.policy, self.depth());
        local.budget = DecodeBudget::new(
            entry.policy,
            u64_from_index(input_bytes).min(entry.policy.limits.max_input_bytes),
        );
        let scratch = ScratchContext { ctx: local };
        let result = work(&scratch);
        if let Some(limit) = self.resource_refusal() {
            return Err(limit.into());
        }
        if self.budget.state() != entry || self.derived_spaces.get() != entry_spaces {
            return Err(CodecError::malformed(
                "parent resources changed during evidence work",
            ));
        }
        match (scratch.resource_refusal(), result) {
            (Some(limit), _) | (None, Err(CodecError::ResourceLimit(limit))) => Ok(Err(limit)),
            (None, result) => result.map(Ok),
        }
    }
}

#[cfg(test)]
mod tests;
