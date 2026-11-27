// SPDX-License-Identifier: Apache-2.0
//! Scratch decodes: work whose output the caller reads and drops.

use std::ops::Deref;

use crate::CodecError;

use super::transaction::{BudgetState, Event};
use super::{DecodeArena, DecodeContext, ResourceLimit};

/// The context of work opened with [`DecodeContext::scratch`].
///
/// It starts from the counters of the context that opened it and owns its
/// arena. The bytes it retains are released when the work returns; its other
/// charges reach the opening context.
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
    /// Runs `work`, whose output the caller reads and drops, on a
    /// [`ScratchContext`].
    ///
    /// The retained bytes that `work` charges are bounded by this context's
    /// remaining retained allowance and released when it returns, so the
    /// caller charges the bytes it keeps of the output itself. The input,
    /// decompressed and entity charges of `work` and its address spaces
    /// stay charged here. Trace replay checks the same temporary charges at
    /// the new position. A bounded result requires execution at that position.
    ///
    /// Returns `Ok(Err(limit))` when a resource refuses inside `work`.
    /// The caller gives up its output and this context continues.
    ///
    /// # Errors
    ///
    /// A resource refusal in force at entry or a non-resource error returned
    /// by `work`.
    pub fn scratch<T>(
        &self,
        work: impl for<'s> FnOnce(&ScratchContext<'s>) -> Result<T, CodecError>,
    ) -> Result<Result<T, ResourceLimit>, CodecError> {
        if let Some(limit) = self.resource_refusal() {
            return Err(limit.into());
        }
        let entry = self.budget.state();
        let entry_spaces = self.derived_spaces.get();
        let arena = DecodeArena::new();
        let mut local = DecodeContext::new(&arena, &entry.policy, self.depth());
        local.budget.restore(&entry);
        local.derived_spaces.set(entry_spaces);
        local.companions = self.companions;
        if self.budget.trace.borrow().is_some() {
            *local.budget.trace.borrow_mut() = Some(Vec::new());
        }
        let scratch = ScratchContext { ctx: local };
        let result = work(&scratch);
        let ScratchContext { ctx: local } = scratch;
        let reached = local.budget.state();
        let spaces = local.derived_spaces.get();
        let limit = reached.fuse.or(match &result {
            Err(CodecError::ResourceLimit(limit)) => Some(*limit),
            _ => None,
        });
        self.budget.restore(&BudgetState {
            retained: entry.retained,
            depth: entry.depth,
            fuse: None,
            ..reached
        });
        self.derived_spaces.set(spaces);

        if let Some(trace) = self.budget.trace.borrow_mut().as_mut() {
            match (limit, local.budget.trace.borrow_mut().take()) {
                (None, Some(events)) => {
                    trace.push(Event::ScratchStart);
                    trace.extend(events);
                    trace.push(Event::ScratchEnd);
                }
                _ => trace.push(Event::ExecuteHere),
            }
        }

        match limit {
            Some(limit) => Ok(Err(limit)),
            None => result.map(Ok),
        }
    }
}

#[cfg(test)]
mod tests;
