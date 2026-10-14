// SPDX-License-Identifier: Apache-2.0
//! Unified resource accounting for one decode session.

use std::cell::Cell;
use std::num::NonZeroU64;

use crate::CodecError;

use super::error::{ResourceDimension, ResourceFailure, ResourceLimit};
use super::policy::{
    DecodePolicy, DECOMPRESSED_TOTAL_BASE, DECOMPRESSED_TOTAL_PER_INPUT_BYTE, MATERIALIZED_BASE,
    MATERIALIZED_PER_INPUT_BYTE, RETAINED_BASE, RETAINED_PER_INPUT_BYTE,
};
use super::view::u64_from_index;

#[derive(Debug)]
pub(super) struct DecodeBudget {
    /// Policy applied to this budget.
    policy: DecodePolicy,
    input_bytes: Cell<u64>,
    decompressed: Cell<u64>,
    materialized: Cell<u64>,
    retained: Cell<u64>,
    entities: Cell<u64>,
    collection_items: Cell<u64>,
    recursion_depth: Cell<u64>,
    work: Cell<u64>,
    fuse: Cell<Option<ResourceLimit>>,
}

impl DecodeBudget {
    /// Returns the policy applied to this budget.
    pub(super) fn policy(&self) -> &DecodePolicy {
        &self.policy
    }

    pub(super) fn new(policy: DecodePolicy, input_bytes: u64) -> Self {
        Self {
            policy,
            input_bytes: Cell::new(input_bytes),
            decompressed: Cell::new(0),
            materialized: Cell::new(0),
            retained: Cell::new(0),
            entities: Cell::new(0),
            collection_items: Cell::new(0),
            recursion_depth: Cell::new(0),
            work: Cell::new(0),
            fuse: Cell::new(None),
        }
    }

    pub(super) fn fused(&self) -> Option<ResourceLimit> {
        self.fuse.get()
    }

    /// The smaller of a policy ceiling and `base + per_input_byte * input`.
    pub(super) fn proportional_allowance(
        &self,
        ceiling: u64,
        base: u64,
        per_input_byte: u64,
    ) -> u64 {
        let Some(proportional) = per_input_byte
            .checked_mul(self.input_bytes.get())
            .and_then(|amount| base.checked_add(amount))
        else {
            return ceiling;
        };
        ceiling.min(proportional)
    }

    pub(super) fn decompression_allowance(&self) -> u64 {
        self.proportional_allowance(
            self.policy.limits.max_decompressed_bytes_total,
            DECOMPRESSED_TOTAL_BASE,
            DECOMPRESSED_TOTAL_PER_INPUT_BYTE,
        )
    }

    pub(super) fn input_bytes(&self) -> u64 {
        self.input_bytes.get()
    }

    pub(super) fn charge_input(
        &self,
        amount: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge(
            ResourceDimension::InputBytes,
            &self.input_bytes,
            self.policy.limits.max_input_bytes,
            amount,
            operation,
        )
        .map_err(Into::into)
    }

    pub(super) fn charge_decompressed(
        &self,
        amount: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge(
            ResourceDimension::DecompressedBytes,
            &self.decompressed,
            self.decompression_allowance(),
            amount,
            operation,
        )
        .map_err(Into::into)
    }

    pub(super) fn decompressed_used(&self) -> u64 {
        self.decompressed.get()
    }

    fn materialized_allowance(&self) -> u64 {
        self.proportional_allowance(
            self.policy.limits.max_materialized_bytes,
            MATERIALIZED_BASE,
            MATERIALIZED_PER_INPUT_BYTE,
        )
    }

    fn retained_allowance(&self) -> u64 {
        self.proportional_allowance(
            self.policy.limits.max_retained_bytes,
            RETAINED_BASE,
            RETAINED_PER_INPUT_BYTE,
        )
    }

    fn charge(
        &self,
        dimension: ResourceDimension,
        used: &Cell<u64>,
        limit: u64,
        amount: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        if let Some(resource) = self.fuse.get() {
            return Err(resource);
        }
        if !dimension.is_counted() {
            return Ok(());
        }
        let before = used.get();
        #[cfg(feature = "test-support")]
        let limit = super::test_support::charge_limit(
            &self.policy.limits,
            dimension,
            operation,
            before,
            amount,
        )
        .map_or(limit, |probe| limit.min(probe));
        if limit
            .checked_sub(before)
            .is_none_or(|remaining| amount > remaining)
        {
            return Err(self.refuse_limit(
                dimension,
                ResourceFailure::BudgetExceeded,
                limit,
                before,
                amount,
                operation,
            ));
        }
        used.set(before + amount);
        #[cfg(feature = "test-support")]
        super::test_support::record_charge(dimension, operation, amount);
        Ok(())
    }

    pub(super) fn refuse_limit(
        &self,
        dimension: ResourceDimension,
        reason: ResourceFailure,
        limit: u64,
        used: u64,
        additional: u64,
        operation: &'static str,
    ) -> ResourceLimit {
        let resource = ResourceLimit {
            dimension,
            reason,
            limit,
            used,
            additional,
            operation,
        };
        self.fuse.set(Some(resource));
        resource
    }

    pub(super) fn refuse(
        &self,
        dimension: ResourceDimension,
        reason: ResourceFailure,
        limit: u64,
        used: u64,
        additional: u64,
        operation: &'static str,
    ) -> CodecError {
        self.refuse_limit(dimension, reason, limit, used, additional, operation)
            .into()
    }

    pub(super) fn reserve_scoped(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<ScopedReservation<'_>, CodecError> {
        self.reserve_scoped_limit(bytes, operation)
            .map_err(Into::into)
    }

    pub(super) fn reserve_scoped_limit(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<ScopedReservation<'_>, ResourceLimit> {
        self.charge(
            ResourceDimension::MaterializedBytes,
            &self.materialized,
            self.materialized_allowance(),
            bytes,
            operation,
        )?;
        Ok(ScopedReservation { budget: self })
    }

    /// Report allocator refusal after a scoped-byte reservation was recorded.
    pub(super) fn scoped_allocation_failed(
        &self,
        charged: u64,
        operation: &'static str,
    ) -> CodecError {
        self.scoped_allocation_failed_limit(charged, operation)
            .into()
    }

    pub(super) fn scoped_allocation_failed_limit(
        &self,
        charged: u64,
        operation: &'static str,
    ) -> ResourceLimit {
        self.refuse_limit(
            ResourceDimension::MaterializedBytes,
            ResourceFailure::AllocationFailed,
            self.materialized_allowance(),
            self.materialized.get(),
            charged,
            operation,
        )
    }

    pub(super) fn scoped_size_overflow_limit(&self, operation: &'static str) -> ResourceLimit {
        self.refuse_limit(
            ResourceDimension::MaterializedBytes,
            ResourceFailure::BudgetExceeded,
            self.materialized_allowance(),
            self.materialized.get(),
            u64::MAX,
            operation,
        )
    }

    pub(super) fn charge_retained(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_retained_limit(bytes, operation)
            .map_err(Into::into)
    }

    pub(super) fn charge_retained_limit(
        &self,
        _bytes: u64,
        _operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        // Retained bytes count only at output chokepoints (`retain_bytes`).
        // This per-site charge remains until its callers are removed.
        self.fuse.get().map_or(Ok(()), Err)
    }

    /// Charges bytes that decoded output keeps for the rest of the session:
    /// native record text, arena buffers, and retained source byte copies.
    pub(super) fn retain_bytes(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.charge(
            ResourceDimension::RetainedBytes,
            &self.retained,
            self.retained_allowance(),
            bytes,
            operation,
        )
    }

    pub(super) fn retained_size_overflow_limit(&self, operation: &'static str) -> ResourceLimit {
        let (dimension, limit, used) = (
            ResourceDimension::RetainedBytes,
            self.retained_allowance(),
            self.retained.get(),
        );
        self.refuse_limit(
            dimension,
            ResourceFailure::BudgetExceeded,
            limit,
            used,
            u64::MAX,
            operation,
        )
    }

    /// Report allocator refusal after a retained charge was already recorded.
    pub(super) fn retained_allocation_failed(
        &self,
        charged: u64,
        operation: &'static str,
    ) -> CodecError {
        self.retained_allocation_failed_limit(charged, operation)
            .into()
    }

    pub(super) fn retained_allocation_failed_limit(
        &self,
        charged: u64,
        operation: &'static str,
    ) -> ResourceLimit {
        let (dimension, current, limit) = (
            ResourceDimension::RetainedBytes,
            self.retained.get(),
            self.retained_allowance(),
        );
        let prior = match current.checked_sub(charged) {
            Some(prior) => prior,
            None => current,
        };
        self.refuse_limit(
            dimension,
            ResourceFailure::AllocationFailed,
            limit,
            prior,
            charged,
            operation,
        )
    }

    pub(super) fn charge_entities(
        &self,
        count: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge(
            ResourceDimension::Entities,
            &self.entities,
            self.policy.limits.max_entities,
            count,
            operation,
        )
        .map_err(Into::into)
    }

    pub(super) fn charge_collection_items(
        &self,
        count: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_collection_items_limit(count, operation)
            .map_err(Into::into)
    }

    pub(super) fn charge_collection_items_limit(
        &self,
        count: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.charge(
            ResourceDimension::CollectionItems,
            &self.collection_items,
            u64::MAX,
            count,
            operation,
        )
    }

    pub(super) fn collection_allocation_failed(
        &self,
        charged: u64,
        operation: &'static str,
    ) -> CodecError {
        self.collection_allocation_failed_limit(charged, operation)
            .into()
    }

    pub(super) fn collection_allocation_failed_limit(
        &self,
        charged: u64,
        operation: &'static str,
    ) -> ResourceLimit {
        let current = self.collection_items.get();
        let prior = match current.checked_sub(charged) {
            Some(prior) => prior,
            None => current,
        };
        self.refuse_limit(
            ResourceDimension::CollectionItems,
            ResourceFailure::AllocationFailed,
            u64::MAX,
            prior,
            charged,
            operation,
        )
    }

    pub(super) fn enter_nested(
        &self,
        operation: &'static str,
    ) -> Result<DepthGuard<'_>, ResourceLimit> {
        self.charge(
            ResourceDimension::RecursionDepth,
            &self.recursion_depth,
            self.policy.limits.max_recursion_depth,
            1,
            operation,
        )?;
        Ok(DepthGuard { budget: self })
    }

    pub(super) fn charge_work(
        &self,
        units: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_work_limit(units, operation).map_err(Into::into)
    }

    pub(super) fn charge_work_limit(
        &self,
        units: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.charge(
            ResourceDimension::WorkUnits,
            &self.work,
            u64::MAX,
            units,
            operation,
        )
    }
}

/// A temporary-storage reservation. Temporary storage is no longer counted;
/// the type remains until its callers are removed.
#[derive(Debug)]
pub struct ScopedReservation<'a> {
    budget: &'a DecodeBudget,
}

impl ScopedReservation<'_> {
    /// Runs `build`; temporary storage is not counted.
    pub fn with_storage<T, E: From<CodecError>>(
        &mut self,
        build: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        build()
    }

    /// Runs `build`; temporary storage is not counted.
    pub fn with_storage_limit<T>(
        &mut self,
        build: impl FnOnce() -> Result<T, ResourceLimit>,
    ) -> Result<T, ResourceLimit> {
        build()
    }

    /// Temporary storage is not counted; reports an earlier session refusal.
    pub fn grow_limit(&mut self, _bytes: u64) -> Result<(), ResourceLimit> {
        self.budget.fused().map_or(Ok(()), Err)
    }

    /// Temporary storage is not counted; reports an earlier session refusal.
    pub fn grow(&mut self, bytes: u64) -> Result<(), CodecError> {
        self.grow_limit(bytes).map_err(Into::into)
    }

    /// Temporary storage is not counted; reports an earlier session refusal.
    pub fn commit(self) -> Result<(), CodecError> {
        self.budget
            .fused()
            .map_or(Ok(()), |limit| Err(limit.into()))
    }
}

// Callers still end reservations with `drop`; the empty impl keeps those
// calls meaningful until the reservations are removed.
impl Drop for ScopedReservation<'_> {
    fn drop(&mut self) {}
}

/// A guard holding one unit of recursive nesting depth.
#[derive(Debug)]
pub struct DepthGuard<'a> {
    budget: &'a DecodeBudget,
}

impl Drop for DepthGuard<'_> {
    fn drop(&mut self) {
        let Some(depth) = self.budget.recursion_depth.get().checked_sub(1) else {
            return;
        };
        self.budget.recursion_depth.set(depth);
    }
}

/// A sticky, context-free local work counter.
#[derive(Debug)]
pub struct WorkBudget<'a> {
    limit: usize,
    remaining: Cell<Option<usize>>,
    recursion_depth: Cell<usize>,
    session: Option<&'a DecodeBudget>,
    session_work_scale: NonZeroU64,
}

/// RAII guard for one recursive geometry-evaluation frame.
#[derive(Debug)]
pub struct WorkBudgetRecursionGuard<'budget, 'session> {
    account: WorkRecursionAccount<'budget, 'session>,
}

#[derive(Debug)]
enum WorkRecursionAccount<'budget, 'session> {
    Session { _guard: DepthGuard<'session> },
    Independent(&'budget WorkBudget<'session>),
}

impl Drop for WorkBudgetRecursionGuard<'_, '_> {
    fn drop(&mut self) {
        if let WorkRecursionAccount::Independent(budget) = &self.account {
            if let Some(depth) = budget.recursion_depth.get().checked_sub(1) {
                budget.recursion_depth.set(depth);
            }
        }
    }
}

impl WorkBudget<'static> {
    /// Creates an independent local work slice.
    pub const fn new(limit: usize) -> Self {
        Self {
            limit,
            remaining: Cell::new(Some(limit)),
            recursion_depth: Cell::new(0),
            session: None,
            session_work_scale: NonZeroU64::MIN,
        }
    }
}

impl<'a> WorkBudget<'a> {
    pub(super) fn for_session(limit: u64, session: &'a DecodeBudget) -> Self {
        let Ok(limit) = usize::try_from(limit) else {
            drop(session.refuse(
                ResourceDimension::WorkUnits,
                ResourceFailure::BudgetExceeded,
                u64_from_index(usize::MAX),
                0,
                limit,
                "work_budget",
            ));
            return Self {
                limit: 0,
                remaining: Cell::new(None),
                recursion_depth: Cell::new(0),
                session: Some(session),
                session_work_scale: NonZeroU64::MIN,
            };
        };
        Self {
            limit,
            remaining: Cell::new(Some(limit)),
            recursion_depth: Cell::new(0),
            session: Some(session),
            session_work_scale: NonZeroU64::MIN,
        }
    }

    /// Set the session work owed by each local unit without changing this
    /// slice's local ceiling. Attached children preserve the same scale.
    #[must_use]
    pub fn with_session_work_scale(mut self, scale: NonZeroU64) -> Self {
        self.session_work_scale = scale;
        self
    }

    /// Reserve temporary evaluator storage in the attached decode session.
    /// Independent slices retain fallible allocation without session charging.
    pub fn reserve_scratch(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<super::work_scratch::WorkScratch<'a>, ResourceLimit> {
        super::work_scratch::WorkScratch::new(self.session, bytes, operation)
    }

    /// Charges one work unit, returning false after exhaustion.
    pub fn charge(&self) -> bool {
        self.charge_by(1)
    }

    /// Charges several work units, with sticky exhaustion on refusal.
    pub fn charge_by(&self, work: usize) -> bool {
        self.charge_by_against(work, self.session)
    }

    fn charge_by_against(&self, work: usize, session: Option<&DecodeBudget>) -> bool {
        let Some(remaining) = self.remaining.get() else {
            return false;
        };
        if work > remaining {
            self.remaining.set(None);
            false
        } else {
            if let Some(session) = session {
                let Some(scaled) = u64_from_index(work).checked_mul(self.session_work_scale.get())
                else {
                    // The sticky session keeps the refusal for finish_session.
                    let _failure = session.refuse(
                        ResourceDimension::Codec("scaled_work_budget"),
                        ResourceFailure::BudgetExceeded,
                        u64::MAX - 1,
                        u64::MAX - 1,
                        1,
                        "scaled_work_budget",
                    );
                    self.remaining.set(None);
                    return false;
                };
                if session.charge_work(scaled, "work_budget").is_err() {
                    self.remaining.set(None);
                    return false;
                }
            }
            self.remaining.set(Some(remaining - work));
            true
        }
    }

    /// Returns whether this slice has refused a charge.
    pub fn exhausted(&self) -> bool {
        self.remaining.get().is_none()
    }

    /// Marks this slice exhausted without consuming additional session work.
    pub fn exhaust(&self) {
        self.remaining.set(None);
    }

    /// Returns unconsumed work units.
    pub fn remaining(&self) -> usize {
        self.remaining.get().unwrap_or(0)
    }

    /// Returns charged work and the remainder forfeited by exhaustion.
    pub fn consumed(&self) -> usize {
        let remaining = self.remaining();
        if remaining > self.limit {
            self.exhaust();
            return self.limit;
        }
        self.limit - remaining
    }

    /// Enters a session frame, or an independent frame with a 256-frame ceiling.
    /// Attached child slices share the active session depth.
    pub fn recursion_guard(&self) -> Result<WorkBudgetRecursionGuard<'_, 'a>, ResourceLimit> {
        const INDEPENDENT_RECURSION_DEPTH: usize = 256;
        if let Some(session) = self.session {
            return session.enter_nested("work_budget_recursion").map(|guard| {
                WorkBudgetRecursionGuard {
                    account: WorkRecursionAccount::Session { _guard: guard },
                }
            });
        }
        let depth = self.recursion_depth.get();
        if depth >= INDEPENDENT_RECURSION_DEPTH {
            self.exhaust();
            return Err(ResourceLimit {
                dimension: ResourceDimension::RecursionDepth,
                reason: ResourceFailure::BudgetExceeded,
                limit: u64_from_index(INDEPENDENT_RECURSION_DEPTH),
                used: u64_from_index(depth),
                additional: 1,
                operation: "work_budget_recursion",
            });
        }
        self.recursion_depth.set(depth + 1);
        Ok(WorkBudgetRecursionGuard {
            account: WorkRecursionAccount::Independent(self),
        })
    }

    /// Copy the active cycle path and admit one new frame slot.
    /// The reservation covers the copied path until the frame leaves.
    pub fn copy_recursion_path<T: Copy>(
        &self,
        path: &[T],
    ) -> Result<(Vec<T>, super::work_scratch::WorkScratch<'a>), ResourceLimit> {
        const OPERATION: &str = "model evaluation cycle path";
        let capacity = path.len().checked_add(1).ok_or_else(|| ResourceLimit {
            dimension: ResourceDimension::CollectionItems,
            reason: ResourceFailure::BudgetExceeded,
            limit: u64::MAX,
            used: u64_from_index(path.len()),
            additional: 1,
            operation: OPERATION,
        })?;
        let bytes = u64_from_index(capacity)
            .checked_mul(u64_from_index(std::mem::size_of::<T>()))
            .ok_or(ResourceLimit {
                dimension: ResourceDimension::MaterializedBytes,
                reason: ResourceFailure::BudgetExceeded,
                limit: u64::MAX,
                used: 0,
                additional: u64::MAX,
                operation: OPERATION,
            })?;
        if let Some(session) = self.session {
            session.charge_collection_items_limit(u64_from_index(capacity), OPERATION)?;
            // Copy each path member and compare it once when binding the frame.
            session.charge_work_limit(
                bytes
                    .checked_add(u64_from_index(path.len()))
                    .ok_or_else(|| {
                        session.refuse_limit(
                            ResourceDimension::WorkUnits,
                            ResourceFailure::BudgetExceeded,
                            u64::MAX,
                            bytes,
                            u64_from_index(path.len()),
                            OPERATION,
                        )
                    })?,
                OPERATION,
            )?;
        }
        let storage = self.reserve_scratch(bytes, OPERATION)?;
        let mut copied = Vec::new();
        copied
            .try_reserve_exact(capacity)
            .map_err(|_| match self.session {
                Some(session) => session.refuse_limit(
                    ResourceDimension::MaterializedBytes,
                    ResourceFailure::AllocationFailed,
                    session.materialized_allowance(),
                    session.materialized.get(),
                    bytes,
                    OPERATION,
                ),
                None => ResourceLimit::allocation_failed(
                    ResourceDimension::MaterializedBytes,
                    bytes,
                    bytes,
                    OPERATION,
                ),
            })?;
        copied.extend_from_slice(path);
        Ok((copied, storage))
    }

    /// Creates an independent child slice capped by this budget's remainder.
    pub fn child_slice(&self, limit: usize) -> WorkBudget<'static> {
        WorkBudget::new(limit.min(self.remaining()))
    }

    /// Creates a child slice capped by this budget's remainder and attached to its session.
    pub fn session_child_slice(&self, limit: usize) -> WorkBudget<'_> {
        WorkBudget {
            limit: limit.min(self.remaining()),
            remaining: Cell::new(Some(limit.min(self.remaining()))),
            recursion_depth: Cell::new(0),
            session: self.session,
            session_work_scale: self.session_work_scale,
        }
    }

    /// Charges this budget for work consumed by a child slice.
    ///
    /// # Errors
    ///
    /// [`BudgetExhausted`] when the child's work is above this budget's
    /// remainder. The budget marks itself exhausted in that case, so
    /// [`WorkBudget::exhausted`] answers `true` afterwards.
    pub fn consume_child(&self, child: &WorkBudget<'_>) -> Result<(), BudgetExhausted> {
        // A session child already charged the shared session for each unit.
        // Only transfer its consumption into this parent's local slice.
        let session = match (self.session, child.session) {
            (Some(parent), Some(child)) if std::ptr::eq(parent, child) => None,
            _ => self.session,
        };
        if self.charge_by_against(child.consumed(), session) {
            Ok(())
        } else {
            Err(BudgetExhausted)
        }
    }
}

/// A work budget reached its limit.
///
/// The budget marks itself exhausted when it refuses, so this error names the
/// event and carries no state of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("work budget exhausted")]
pub struct BudgetExhausted;

/// The work a step over `items` owes a [`WorkBudget`].
///
/// A step that walks no item still runs, so it owes one unit. Charging zero
/// would leave an unbounded number of empty steps free of the budget, and the
/// walk that makes them would not be bounded by it.
#[must_use]
pub const fn work_units(items: usize) -> usize {
    match items {
        0 => 1,
        items => items,
    }
}

/// Builds a correctly classified refusal for a codec-local ceiling.
pub fn refuse_local_limit(what: &'static str, limit: u64, requested: u64) -> CodecError {
    local_limit_error(what, limit, requested, what)
}

fn local_limit_error(
    what: &'static str,
    limit: u64,
    requested: u64,
    operation: &'static str,
) -> CodecError {
    let (used, additional) = match requested.checked_sub(limit) {
        Some(excess) => (limit, excess),
        None => (requested, 0),
    };
    CodecError::ResourceLimit(ResourceLimit {
        dimension: ResourceDimension::Codec(what),
        reason: ResourceFailure::BudgetExceeded,
        limit,
        used,
        additional,
        operation,
    })
}

#[cfg(test)]
mod tests {
    use super::{work_units, DecodeBudget, WorkBudget};
    use crate::decode::{DecodePolicy, ResourceDimension, ResourceFailure};

    fn descend(budget: &WorkBudget<'_>, depth: usize) -> usize {
        let Ok(_guard) = budget.recursion_guard() else {
            return depth;
        };
        descend(budget, depth + 1)
    }

    #[test]
    fn recursion_guard_is_shared_and_bounded() {
        let budget = WorkBudget::new(10_000);
        assert_eq!(descend(&budget, 0), 256);
        assert!(budget.exhausted());
    }

    #[test]
    fn attached_recursion_refuses_zero_session_depth() {
        let mut policy = DecodePolicy::default();
        policy.limits.max_recursion_depth = 0;
        let session = DecodeBudget::new(policy, 1);
        let budget = WorkBudget::for_session(100, &session);
        let failure = budget
            .recursion_guard()
            .expect_err("zero depth refuses first frame");
        assert_eq!(failure.dimension, ResourceDimension::RecursionDepth);
        assert_eq!(failure.limit, 0);
        assert_eq!(failure.used, 0);
        assert_eq!(failure.additional, 1);
        assert_eq!(session.fused(), Some(failure));
    }

    #[test]
    fn attached_child_preserves_active_session_depth() {
        let mut policy = DecodePolicy::default();
        policy.limits.max_recursion_depth = 1;
        let session = DecodeBudget::new(policy, 1);
        let budget = WorkBudget::for_session(100, &session);
        let guard = budget.recursion_guard().expect("first frame fits");
        let child = budget.session_child_slice(100);
        let failure = child.recursion_guard().expect_err("child shares depth");
        assert_eq!(failure.dimension, ResourceDimension::RecursionDepth);
        assert_eq!(failure.used, 1);
        drop(guard);
        assert_eq!(session.recursion_depth.get(), 0);
    }

    #[test]
    fn a_step_over_no_item_owes_exactly_one_unit() {
        assert_eq!(work_units(0), 1);
        for items in [1, 2, 7, usize::MAX] {
            assert_eq!(work_units(items), items);
        }

        let budget = WorkBudget::new(1);
        assert!(budget.charge_by(work_units(0)));
        assert_eq!(budget.consumed(), 1);
        assert!(!budget.charge_by(work_units(0)));
        assert!(budget.exhausted());
    }

    #[test]
    fn retained_allocation_refusal_uses_the_retained_dimension_and_prior_usage() {
        let session = DecodeBudget::new(DecodePolicy::default(), 1);
        session
            .charge_retained(3, "copy retained")
            .expect("retained charge fits the default session allowance");
        let crate::CodecError::ResourceLimit(limit) =
            session.retained_allocation_failed(3, "copy retained")
        else {
            panic!("retained allocation must produce a resource refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(limit.reason, ResourceFailure::AllocationFailed);
        assert_eq!(limit.used, 0);
        assert_eq!(limit.additional, 3);
        assert_eq!(session.fused(), Some(limit));
    }

    #[test]
    fn zero_charge_refuses_when_usage_already_exceeds_limit() {
        let mut policy = DecodePolicy::default();
        policy.limits.max_entities = 1;
        let session = DecodeBudget::new(policy, 1);
        session.entities.set(2);
        let error = session.charge_entities(0, "entities");
        assert!(matches!(
            error,
            Err(crate::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.reason == ResourceFailure::BudgetExceeded
                    && limit.used == 2
                    && limit.additional == 0
        ));
    }

    #[test]
    fn retained_allocation_refusal_reports_existing_usage_when_charge_exceeds_it() {
        let session = DecodeBudget::new(DecodePolicy::default(), 1);
        session.retained.set(2);
        let limit = session.retained_allocation_failed_limit(3, "retain");
        assert_eq!(limit.used, 2);
        assert_eq!(limit.additional, 3);
    }
}
