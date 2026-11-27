// SPDX-License-Identifier: Apache-2.0
//! Scratch decodes: work whose output the caller reads and drops.

use std::ops::Deref;

use crate::CodecError;

use super::transaction::{release_retained, BudgetState, Event};
use super::{DecodeArena, DecodeContext, ResourceDimension, ResourceLimit};

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
    /// stay charged here. A traced context records the retained bytes `work`
    /// needed as an allowance decision, so a trace that composes where the
    /// decision differs is refused and executed again. A trace that holds a
    /// given-up scratch composes only where the retained room is the same.
    ///
    /// Returns `Ok(Err(limit))` when `work` passed the retained allowance:
    /// the caller gives up the output of `work`, and this context continues.
    ///
    /// # Errors
    ///
    /// A refusal in force at entry, a refusal of `work` in another dimension
    /// (it fuses this context as the same charge here would), or the error
    /// `work` returned.
    pub fn scratch<T>(
        &self,
        work: impl for<'s> FnOnce(&ScratchContext<'s>) -> Result<T, CodecError>,
    ) -> Result<Result<T, ResourceLimit>, CodecError> {
        if let Some(limit) = self.resource_refusal() {
            return Err(limit.into());
        }
        let entry = self.budget.state();
        let entry_spaces = self.derived_spaces.get();
        let available = self.budget.retained_available();
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
        let events = local.budget.trace.borrow_mut().take().unwrap_or_default();

        let given_up = reached
            .fuse
            .filter(|limit| limit.dimension == ResourceDimension::RetainedBytes);
        self.budget.restore(&BudgetState {
            retained: entry.retained,
            depth: entry.depth,
            fuse: reached.fuse.filter(|_| given_up.is_none()),
            ..reached.clone()
        });
        self.derived_spaces.set(spaces);
        if given_up.is_some() {
            // The charges after the refusal were recorded and refused. A
            // replay composes only where the room equals the room here.
            self.budget.record(Event::Available(available));
            record_charges(self, &entry, &reached, entry_spaces..spaces);
        } else {
            self.budget.record(Event::Allowance(available));
            self.budget.record(Event::AllowanceUse(
                available,
                reached.retained.saturating_sub(entry.retained),
            ));
            for event in release_retained(events) {
                self.budget.record(event);
            }
        }

        match (given_up, reached.fuse) {
            (Some(limit), _) => Ok(Err(limit)),
            (None, Some(limit)) => Err(limit.into()),
            (None, None) => result.map(Ok),
        }
    }
}

fn record_charges(
    ctx: &DecodeContext<'_>,
    entry: &BudgetState,
    reached: &BudgetState,
    spaces: std::ops::Range<usize>,
) {
    for (dimension, before, after) in [
        (ResourceDimension::InputBytes, entry.input, reached.input),
        (
            ResourceDimension::DecompressedBytes,
            entry.decompressed,
            reached.decompressed,
        ),
        (
            ResourceDimension::Entities,
            entry.entities,
            reached.entities,
        ),
    ] {
        if let Some(amount) = after.checked_sub(before).filter(|amount| *amount > 0) {
            ctx.budget
                .record(Event::Charge(dimension, amount, "scratch decode charges"));
        }
    }
    for used in spaces {
        ctx.budget.record(Event::Space(used));
    }
}

#[cfg(test)]
mod tests;
