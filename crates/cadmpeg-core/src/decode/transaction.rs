// SPDX-License-Identifier: Apache-2.0
//! Transactional resource accounting for independent decode tasks.

use std::cell::Cell;

use super::budget::DecodeBudget;
use super::{
    DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ExpandSpec, ResourceDimension,
    ResourceLimit,
};

#[derive(Debug, Clone)]
pub(super) struct BudgetState {
    pub(super) policy: DecodePolicy,
    pub(super) input: u64,
    pub(super) decompressed: u64,
    pub(super) retained: u64,
    pub(super) entities: u64,
    pub(super) depth: u64,
    pub(super) fuse: Option<ResourceLimit>,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Event {
    Charge(ResourceDimension, u64, &'static str),
    Leave,
    Available(u64),
    Allowance(u64),
    AllowanceUse(u64, u64),
    Expand(ExpandSpec),
    Space(usize),
    Batch(Charges),
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Charges {
    amounts: [u64; 3],
    depth: i128,
    peak: i128,
    minimum: i128,
}

pub(super) fn record(events: &mut Vec<Event>, event: Event) {
    let mut batch = match events.last() {
        Some(Event::Batch(batch)) => *batch,
        _ => Charges::default(),
    };
    let compacted = match event {
        Event::Charge(dimension, amount, _) => {
            let index = match dimension {
                ResourceDimension::DecompressedBytes => Some(0),
                ResourceDimension::RetainedBytes => Some(1),
                ResourceDimension::Entities => Some(2),
                ResourceDimension::RecursionDepth => None,
                ResourceDimension::InputBytes => {
                    events.push(event);
                    return;
                }
            };
            if let Some(index) = index {
                batch.amounts[index]
                    .checked_add(amount)
                    .map(|total| batch.amounts[index] = total)
            } else {
                batch.depth.checked_add(i128::from(amount)).map(|depth| {
                    batch.depth = depth;
                    batch.peak = batch.peak.max(depth);
                })
            }
        }
        Event::Leave => batch.depth.checked_sub(1).map(|depth| {
            batch.depth = depth;
            batch.minimum = batch.minimum.min(depth);
        }),
        _ => {
            events.push(event);
            return;
        }
    };
    if compacted.is_none() {
        events.push(event);
    } else if let Some(Event::Batch(last)) = events.last_mut() {
        *last = batch;
    } else {
        events.push(Event::Batch(batch));
    }
}

impl Charges {
    fn replay(self, candidate: &DecodeContext<'_>) -> bool {
        let initial = candidate.budget.state().depth;
        let minimum = i128::from(initial) + self.minimum;
        let maximum = i128::from(initial) + self.peak;
        let final_depth = i128::from(initial) + self.depth;
        if minimum < 0 || maximum > i128::from(candidate.policy().limits.max_recursion_depth) {
            return false;
        }
        let Ok(final_depth) = u64::try_from(final_depth) else {
            return false;
        };
        for (dimension, amount) in [
            ResourceDimension::DecompressedBytes,
            ResourceDimension::RetainedBytes,
            ResourceDimension::Entities,
        ]
        .into_iter()
        .zip(self.amounts)
        {
            if candidate
                .budget
                .replay_charge(dimension, amount, "replay decode charge group")
                .is_err()
            {
                return false;
            }
        }
        let mut state = candidate.budget.state();
        state.depth = final_depth;
        candidate.budget.restore(&state);
        true
    }
}

/// Immutable resource state from which independent decode tasks can start.
///
/// A resumed task owns its counters and arena. Its work does not charge the
/// original context until that context accepts the task's trace.
#[derive(Debug, Clone)]
pub struct DecodeCheckpoint {
    state: BudgetState,
    depth: DecodeDepth,
    spaces: usize,
    source_digest: Option<[u8; 32]>,
}

/// Charge totals between ordered budget observations and expansion boundaries.
/// A refused group causes the caller to execute the original operations.
#[derive(Debug)]
pub struct DecodeTrace {
    events: Vec<Event>,
    refused: bool,
}

impl DecodeTrace {
    /// Storage estimate: event count times the event type size, excluding allocator overhead.
    pub fn retained_bytes_estimate(&self) -> u64 {
        super::u64_from_index(std::mem::size_of_val(self.events.as_slice()))
    }
}

impl DecodeCheckpoint {
    /// Starts an independent task and records its resource operations.
    pub fn resume<'a>(&self, arena: &'a DecodeArena) -> DecodeContext<'a> {
        let ctx = DecodeContext::new(arena, &self.state.policy, self.depth);
        ctx.budget.restore(&self.state);
        ctx.derived_spaces.set(self.spaces);
        ctx.source_digest.set(self.source_digest);
        *ctx.budget.trace.borrow_mut() = Some(Vec::new());
        ctx
    }
}

impl DecodeContext<'_> {
    /// Runs work in a caller-owned arena and commits all resource state.
    /// The arena can be dropped after the returned owned value no longer needs it.
    /// Charges, refusal state, address spaces and trace order remain cumulative.
    ///
    /// The task shares this context's caller account, so its charges reach the
    /// caller's cumulative totals directly; restoring the counters then only
    /// carries the local per-session view.
    pub fn with_arena<'b, T>(
        &self,
        arena: &'b DecodeArena,
        work: impl FnOnce(&DecodeContext<'b>) -> T,
    ) -> T {
        let checkpoint = self.checkpoint();
        let local = DecodeContext {
            arena,
            depth: checkpoint.depth,
            encoding: self.encoding,
            budget: DecodeBudget::from_account(
                checkpoint.state.policy,
                self.budget.operation_budget(),
            ),
            derived_spaces: Cell::new(0),
            companions: None,
            source_digest: Cell::new(self.source_digest.get()),
        };
        local.budget.restore(&checkpoint.state);
        local.derived_spaces.set(checkpoint.spaces);
        *local.budget.trace.borrow_mut() = self.budget.trace.borrow_mut().take();
        let result = work(&local);
        self.budget.restore(&local.budget.state());
        self.derived_spaces.set(local.derived_spaces.get());
        *self.budget.trace.borrow_mut() = local.budget.trace.borrow_mut().take();
        result
    }

    /// Reads an optional-enrichment allowance whose threshold decisions are
    /// recorded with [`Self::observe_retained_allowance`].
    pub fn retained_allowance(&self) -> u64 {
        let available = self.budget.retained_available();
        self.budget.record(Event::Allowance(available));
        available
    }

    /// Records a threshold decision against a previously read allowance.
    pub fn observe_retained_allowance(&self, remaining: u64, requested: u64) {
        self.budget
            .record(Event::AllowanceUse(remaining, requested));
    }

    /// Captures the current counters without sharing mutable decode state.
    pub fn checkpoint(&self) -> DecodeCheckpoint {
        DecodeCheckpoint {
            state: self.budget.state(),
            depth: self.depth(),
            spaces: self.derived_spaces.get(),
            source_digest: self.source_digest.get(),
        }
    }

    /// Ends recording for a task started with [`DecodeCheckpoint::resume`].
    pub fn take_trace(&self) -> DecodeTrace {
        let mut events = self.budget.trace.borrow_mut().take().unwrap_or_default();
        let refused = self.resource_refusal().is_some();
        if refused {
            events.clear();
        }
        events.shrink_to_fit();
        DecodeTrace { events, refused }
    }

    /// Applies a task's operations as one transaction.
    ///
    /// Returns false without changing this context if a charge refuses, a
    /// budget observation changes, or derived address spaces need rebinding.
    /// The caller must then execute the task against this context. This also
    /// preserves budget-dependent optional output and the exact refusal site.
    pub fn accept_trace(&self, trace: &DecodeTrace) -> bool {
        if trace.refused || self.resource_refusal().is_some() {
            return false;
        }
        let arena = DecodeArena::new();
        let candidate = self.checkpoint().resume(&arena);
        *candidate.budget.trace.borrow_mut() = None;
        let mut allowance_delta = 0_i128;
        for event in &trace.events {
            let accepted = match *event {
                Event::Batch(batch) => batch.replay(&candidate),
                Event::Charge(dimension, amount, operation) => candidate
                    .budget
                    .replay_charge(dimension, amount, operation)
                    .is_ok(),
                Event::Leave => {
                    candidate.budget.leave();
                    true
                }
                Event::Available(available) => candidate.retained_bytes_available() == available,
                Event::Allowance(available) => {
                    allowance_delta =
                        i128::from(candidate.retained_bytes_available()) - i128::from(available);
                    true
                }
                Event::AllowanceUse(remaining, requested) => {
                    let adjusted = i128::from(remaining) + allowance_delta;
                    (adjusted >= i128::from(requested)) == (remaining >= requested)
                }
                Event::Expand(spec) => candidate.admit_expand(spec).is_ok(),
                Event::Space(expected) => {
                    if candidate.derived_spaces.get() != expected {
                        false
                    } else if let Some(next) = expected.checked_add(1) {
                        candidate.derived_spaces.set(next);
                        true
                    } else {
                        false
                    }
                }
            };
            if !accepted {
                return false;
            }
        }
        if let Some(events) = self.budget.trace.borrow_mut().as_mut() {
            events.extend_from_slice(&trace.events);
        }
        self.budget.restore(&candidate.budget.state());
        self.derived_spaces.set(candidate.derived_spaces.get());
        true
    }
}

#[cfg(test)]
mod tests;
