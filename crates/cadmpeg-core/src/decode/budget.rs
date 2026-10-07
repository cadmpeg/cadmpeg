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
    scoped_storage: Cell<Option<u64>>,
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
            scoped_storage: Cell::new(None),
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

    pub(super) fn decompression_allowance(&self) -> u64 {
        let policy_limit = self.policy.limits.max_decompressed_bytes_total;
        let Some(proportional) = DECOMPRESSED_TOTAL_PER_INPUT_BYTE
            .checked_mul(self.input_bytes.get())
            .and_then(|bytes| DECOMPRESSED_TOTAL_BASE.checked_add(bytes))
        else {
            return policy_limit;
        };
        policy_limit.min(proportional)
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

    #[cfg(test)]
    pub(super) fn retained_used(&self) -> u64 {
        self.retained.get()
    }

    pub(super) fn decompressed_used(&self) -> u64 {
        self.decompressed.get()
    }

    fn materialized_allowance(&self) -> u64 {
        let policy_limit = self.policy.limits.max_materialized_bytes;
        let Some(proportional) = MATERIALIZED_PER_INPUT_BYTE
            .checked_mul(self.input_bytes.get())
            .and_then(|bytes| MATERIALIZED_BASE.checked_add(bytes))
        else {
            return policy_limit;
        };
        policy_limit.min(proportional)
    }

    fn retained_allowance(&self) -> u64 {
        let policy_limit = self.policy.limits.max_retained_bytes;
        let Some(proportional) = RETAINED_PER_INPUT_BYTE
            .checked_mul(self.input_bytes.get())
            .and_then(|bytes| RETAINED_BASE.checked_add(bytes))
        else {
            return policy_limit;
        };
        policy_limit.min(proportional)
    }

    /// The policy leaves this dimension unlimited, which marks the budget a
    /// refusal probe searches.
    #[cfg(any(test, feature = "test-support"))]
    fn unlimited_by_policy(&self, dimension: ResourceDimension) -> bool {
        let limits = &self.policy.limits;
        let limit = match dimension {
            ResourceDimension::InputBytes => limits.max_input_bytes,
            ResourceDimension::DecompressedBytes => limits.max_decompressed_bytes_total,
            ResourceDimension::MaterializedBytes => limits.max_materialized_bytes,
            ResourceDimension::RetainedBytes => limits.max_retained_bytes,
            ResourceDimension::Entities => limits.max_entities,
            ResourceDimension::CollectionItems => limits.max_collection_items,
            ResourceDimension::RecursionDepth => limits.max_recursion_depth,
            ResourceDimension::WorkUnits => limits.max_work_units,
            ResourceDimension::Codec(_) => return false,
        };
        limit == u64::MAX
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
        let before = used.get();
        #[cfg(any(test, feature = "test-support"))]
        if let Some(peak) = self
            .unlimited_by_policy(dimension)
            .then(|| {
                super::refusal_probe::refusal_limit(
                    std::ptr::from_ref(self).addr(),
                    dimension,
                    before,
                    amount,
                    operation,
                )
            })
            .flatten()
        {
            return Err(self.refuse_limit(
                dimension,
                ResourceFailure::BudgetExceeded,
                peak,
                before,
                amount,
                operation,
            ));
        }
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
        if let Some(resource) = self.fuse.get() {
            return resource;
        }
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
        CodecError::ResourceLimit(
            self.refuse_limit(dimension, reason, limit, used, additional, operation),
        )
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
        Ok(ScopedReservation {
            budget: self,
            bytes,
            operation,
        })
    }

    /// Report allocator refusal after a scoped-byte reservation was recorded.
    pub(super) fn scoped_allocation_failed(
        &self,
        charged: u64,
        operation: &'static str,
    ) -> CodecError {
        CodecError::ResourceLimit(self.scoped_allocation_failed_limit(charged, operation))
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
            self.materialized.get() - charged,
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
        bytes: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        if let Some(current) = self.scoped_storage.get() {
            let total = current.checked_add(bytes).ok_or_else(|| {
                self.refuse_limit(
                    ResourceDimension::MaterializedBytes,
                    ResourceFailure::BudgetExceeded,
                    self.materialized_allowance(),
                    current,
                    bytes,
                    operation,
                )
            })?;
            self.charge(
                ResourceDimension::MaterializedBytes,
                &self.materialized,
                self.materialized_allowance(),
                bytes,
                operation,
            )?;
            self.scoped_storage.set(Some(total));
            return Ok(());
        }
        self.charge(
            ResourceDimension::RetainedBytes,
            &self.retained,
            self.retained_allowance(),
            bytes,
            operation,
        )
    }

    pub(super) fn retained_size_overflow_limit(&self, operation: &'static str) -> ResourceLimit {
        let (dimension, limit, used) = if self.scoped_storage.get().is_some() {
            (
                ResourceDimension::MaterializedBytes,
                self.materialized_allowance(),
                self.materialized.get(),
            )
        } else {
            (
                ResourceDimension::RetainedBytes,
                self.retained_allowance(),
                self.retained.get(),
            )
        };
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
        CodecError::ResourceLimit(self.retained_allocation_failed_limit(charged, operation))
    }

    pub(super) fn retained_allocation_failed_limit(
        &self,
        charged: u64,
        operation: &'static str,
    ) -> ResourceLimit {
        let (dimension, current, limit) = if self.scoped_storage.get().is_some() {
            (
                ResourceDimension::MaterializedBytes,
                self.materialized.get(),
                self.materialized_allowance(),
            )
        } else {
            (
                ResourceDimension::RetainedBytes,
                self.retained.get(),
                self.retained_allowance(),
            )
        };
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
            self.policy.limits.max_collection_items,
            count,
            operation,
        )
    }

    pub(super) fn collection_allocation_failed(
        &self,
        charged: u64,
        operation: &'static str,
    ) -> CodecError {
        CodecError::ResourceLimit(self.collection_allocation_failed_limit(charged, operation))
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
            self.policy.limits.max_collection_items,
            prior,
            charged,
            operation,
        )
    }

    pub(super) fn storage_scope(&self, operation: &'static str) -> StorageScope<'_> {
        let prior = self.scoped_storage.replace(Some(0));
        StorageScope {
            budget: self,
            prior,
            operation,
        }
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
            self.policy.limits.max_work_units,
            units,
            operation,
        )
    }

    pub(super) fn work_bound_overflow_limit(&self, operation: &'static str) -> ResourceLimit {
        self.refuse_limit(
            ResourceDimension::WorkUnits,
            ResourceFailure::BudgetExceeded,
            self.policy.limits.max_work_units,
            self.work.get(),
            u64::MAX,
            operation,
        )
    }
}

pub(super) struct StorageScope<'a> {
    budget: &'a DecodeBudget,
    prior: Option<u64>,
    operation: &'static str,
}

impl<'a> StorageScope<'a> {
    pub(super) fn finish(self) -> ScopedReservation<'a> {
        let bytes = self
            .budget
            .scoped_storage
            .replace(Some(0))
            .map_or(0, std::convert::identity);
        ScopedReservation {
            budget: self.budget,
            bytes,
            operation: self.operation,
        }
    }
}

impl Drop for StorageScope<'_> {
    fn drop(&mut self) {
        let bytes = self
            .budget
            .scoped_storage
            .replace(self.prior)
            .map_or(0, std::convert::identity);
        drop(ScopedReservation {
            budget: self.budget,
            bytes,
            operation: self.operation,
        });
    }
}

/// A temporary materialization charged until it is dropped or committed.
#[derive(Debug)]
pub struct ScopedReservation<'a> {
    budget: &'a DecodeBudget,
    bytes: u64,
    operation: &'static str,
}

impl ScopedReservation<'_> {
    /// Account copied storage in this live temporary reservation.
    pub fn with_storage<T, E: From<CodecError>>(
        &mut self,
        build: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        self.with_storage_error(build, |limit| E::from(CodecError::ResourceLimit(limit)))
    }

    /// Account temporary storage with a resource-only refusal channel.
    pub fn with_storage_limit<T>(
        &mut self,
        build: impl FnOnce() -> Result<T, ResourceLimit>,
    ) -> Result<T, ResourceLimit> {
        self.with_storage_error(build, |limit| limit)
    }

    fn with_storage_error<T, E>(
        &mut self,
        build: impl FnOnce() -> Result<T, E>,
        failure: impl FnOnce(ResourceLimit) -> E,
    ) -> Result<T, E> {
        let scope = self.budget.storage_scope(self.operation);
        let value = build();
        let mut storage = scope.finish();
        let Some(bytes) = self.bytes.checked_add(storage.bytes) else {
            return value.and_then(|_| {
                Err(failure(self.budget.refuse_limit(
                    ResourceDimension::MaterializedBytes,
                    ResourceFailure::BudgetExceeded,
                    self.budget.materialized_allowance(),
                    self.bytes,
                    storage.bytes,
                    self.operation,
                )))
            });
        };
        self.bytes = bytes;
        storage.bytes = 0;
        value
    }

    /// Increases live temporary storage and returns the resource refusal.
    pub fn grow_limit(&mut self, bytes: u64) -> Result<(), ResourceLimit> {
        self.budget.charge(
            ResourceDimension::MaterializedBytes,
            &self.budget.materialized,
            self.budget.materialized_allowance(),
            bytes,
            self.operation,
        )?;
        self.bytes = self.bytes.checked_add(bytes).ok_or_else(|| {
            self.budget.refuse_limit(
                ResourceDimension::MaterializedBytes,
                ResourceFailure::BudgetExceeded,
                self.budget.materialized_allowance(),
                self.bytes,
                bytes,
                self.operation,
            )
        })?;
        Ok(())
    }

    /// Increases the live temporary reservation.
    pub fn grow(&mut self, bytes: u64) -> Result<(), CodecError> {
        self.grow_limit(bytes).map_err(Into::into)
    }

    /// Converts the temporary reservation into session-retained bytes.
    pub fn commit(self) -> Result<(), CodecError> {
        self.budget.charge_retained(self.bytes, self.operation)
    }
}

impl Drop for ScopedReservation<'_> {
    fn drop(&mut self) {
        let Some(remaining) = self.budget.materialized.get().checked_sub(self.bytes) else {
            self.budget.refuse_limit(
                ResourceDimension::MaterializedBytes,
                ResourceFailure::BudgetExceeded,
                self.budget.materialized_allowance(),
                self.budget.materialized.get(),
                self.bytes,
                self.operation,
            );
            return;
        };
        self.budget.materialized.set(remaining);
    }
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
    /// Maximum recursive frames for evaluation without an attached decode session.
    pub const INDEPENDENT_RECURSION_DEPTH: usize = 256;

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
        if let Some(session) = self.session {
            return session.enter_nested("work_budget_recursion").map(|guard| {
                WorkBudgetRecursionGuard {
                    account: WorkRecursionAccount::Session { _guard: guard },
                }
            });
        }
        let depth = self.recursion_depth.get();
        if depth >= Self::INDEPENDENT_RECURSION_DEPTH {
            self.exhaust();
            return Err(ResourceLimit {
                dimension: ResourceDimension::RecursionDepth,
                reason: ResourceFailure::BudgetExceeded,
                limit: u64_from_index(Self::INDEPENDENT_RECURSION_DEPTH),
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
        let path_len = path.len();
        let capacity = path_len.checked_add(1).ok_or_else(|| ResourceLimit {
            dimension: ResourceDimension::CollectionItems,
            reason: ResourceFailure::BudgetExceeded,
            limit: u64::MAX,
            used: u64_from_index(path_len),
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
                bytes.checked_add(u64_from_index(path_len)).ok_or_else(|| {
                    session.refuse_limit(
                        ResourceDimension::WorkUnits,
                        ResourceFailure::BudgetExceeded,
                        u64::MAX,
                        bytes,
                        u64_from_index(path_len),
                        OPERATION,
                    )
                })?,
                OPERATION,
            )?;
            let reservation = session.reserve_scoped_limit(bytes, OPERATION)?;
            let mut copied = Vec::new();
            copied
                .try_reserve_exact(capacity)
                .map_err(|_| session.scoped_allocation_failed_limit(bytes, OPERATION))?;
            copied.extend(path.iter().copied());
            let storage = super::work_scratch::WorkScratch::from_reservation(reservation);
            return Ok((copied, storage));
        }
        if path_len >= Self::INDEPENDENT_RECURSION_DEPTH {
            self.exhaust();
            return Err(ResourceLimit {
                dimension: ResourceDimension::RecursionDepth,
                reason: ResourceFailure::BudgetExceeded,
                limit: u64_from_index(Self::INDEPENDENT_RECURSION_DEPTH),
                used: u64_from_index(path_len),
                additional: 1,
                operation: OPERATION,
            });
        }
        let storage = super::work_scratch::WorkScratch::new(None, bytes, OPERATION)?;
        let mut copied = Vec::new();
        copied.try_reserve_exact(capacity).map_err(|_| {
            ResourceLimit::allocation_failed(
                ResourceDimension::MaterializedBytes,
                bytes,
                bytes,
                OPERATION,
            )
        })?;
        copied.extend(path.iter().copied());
        Ok((copied, storage))
    }

    /// Creates an independent child slice capped by this budget's remainder.
    pub fn child_slice(&self, limit: usize) -> WorkBudget<'static> {
        WorkBudget::new(limit.min(self.remaining()))
    }

    /// Creates a child slice capped by this budget's remainder and attached to
    /// its session. The slice borrows the session, not this budget.
    #[must_use]
    pub fn session_child_slice(&self, limit: usize) -> WorkBudget<'a> {
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
    use super::{u64_from_index, work_units, DecodeBudget, WorkBudget};
    use crate::decode::{DecodePolicy, ResourceDimension, ResourceFailure};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Copy, PartialEq, Eq)]
    struct CopyWithObservableClone<T>(T);

    static CLONE_CALLS: AtomicUsize = AtomicUsize::new(0);

    // The callback detects a Clone invocation in an operation that must only copy.
    #[allow(clippy::expl_impl_clone_on_copy)]
    impl<T: Clone> Clone for CopyWithObservableClone<T> {
        fn clone(&self) -> Self {
            CLONE_CALLS.fetch_add(1, Ordering::SeqCst);
            Self(self.0.clone())
        }
    }

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
    fn independent_cycle_path_is_bounded_without_charging_local_work() {
        let budget = WorkBudget::new(10_000);
        let path = [0u8; WorkBudget::INDEPENDENT_RECURSION_DEPTH - 1];
        let (copied, storage) = budget
            .copy_recursion_path(&path)
            .expect("the maximum independent prior path fits");
        assert_eq!(copied.len(), path.len());
        assert_eq!(budget.consumed(), 0);
        assert!(!budget.exhausted());
        drop(storage);

        let budget = WorkBudget::new(10_000);
        let path = [0u8; WorkBudget::INDEPENDENT_RECURSION_DEPTH];
        let failure = budget
            .copy_recursion_path(&path)
            .expect_err("an independent path cannot exceed its frame ceiling");
        assert_eq!(failure.dimension, ResourceDimension::RecursionDepth);
        assert_eq!(
            failure.limit,
            u64_from_index(WorkBudget::INDEPENDENT_RECURSION_DEPTH)
        );
        assert_eq!(failure.used, u64_from_index(path.len()));
        assert_eq!(failure.additional, 1);
        assert!(budget.exhausted());
    }

    #[test]
    fn recursion_path_copy_does_not_call_a_copy_types_clone_method() {
        let budget = WorkBudget::new(10);
        let path = [CopyWithObservableClone(3_u8), CopyWithObservableClone(7_u8)];
        CLONE_CALLS.store(0, Ordering::SeqCst);

        let (copied, storage) = budget
            .copy_recursion_path(&path)
            .expect("independent cycle path fits");
        assert_eq!(copied, path);
        assert_eq!(CLONE_CALLS.load(Ordering::SeqCst), 0);
        drop(storage);
    }

    #[test]
    fn attached_cycle_path_keeps_exact_admission_above_independent_ceiling() {
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 257;
        policy.limits.max_work_units = 513;
        policy.limits.max_materialized_bytes = 257;
        let session = DecodeBudget::new(policy, 1);
        let budget = WorkBudget::for_session(1_000, &session);
        let path = [0u8; 256];

        let (copied, storage) = budget
            .copy_recursion_path(&path)
            .expect("attached session limits admit a path above the independent ceiling");
        assert_eq!(copied.len(), path.len());
        assert_eq!(budget.consumed(), 0);
        assert_eq!(session.collection_items.get(), 257);
        assert_eq!(session.work.get(), 513);
        assert_eq!(session.materialized.get(), 257);
        assert!(session.fused().is_none());
        drop(storage);
        assert_eq!(session.materialized.get(), 0);
    }

    #[test]
    fn attached_cycle_path_work_refusal_keeps_prior_charges_and_first_fuse() {
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 3;
        policy.limits.max_work_units = 4;
        policy.limits.max_materialized_bytes = 3;
        let session = DecodeBudget::new(policy, 1);
        let budget = WorkBudget::for_session(10, &session);
        let path = [0u8; 2];

        let failure = budget
            .copy_recursion_path(&path)
            .expect_err("the exact path work exceeds the work limit by one");
        assert_eq!(failure.dimension, ResourceDimension::WorkUnits);
        assert_eq!(failure.limit, 4);
        assert_eq!(failure.used, 0);
        assert_eq!(failure.additional, 5);
        assert_eq!(failure.operation, "model evaluation cycle path");
        assert_eq!(session.collection_items.get(), 3);
        assert_eq!(session.work.get(), 0);
        assert_eq!(session.materialized.get(), 0);
        assert_eq!(budget.consumed(), 0);
        assert_eq!(session.fused(), Some(failure));
        assert_eq!(
            budget
                .copy_recursion_path(&path)
                .expect_err("the first refusal remains sticky"),
            failure
        );
        assert_eq!(session.collection_items.get(), 3);
        assert_eq!(session.work.get(), 0);
        assert_eq!(session.materialized.get(), 0);
    }

    #[test]
    fn attached_cycle_path_storage_refusal_keeps_work_charges_and_first_fuse() {
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 3;
        policy.limits.max_work_units = 5;
        policy.limits.max_materialized_bytes = 2;
        let session = DecodeBudget::new(policy, 1);
        let budget = WorkBudget::for_session(10, &session);
        let path = [0u8; 2];

        let failure = budget
            .copy_recursion_path(&path)
            .expect_err("the exact scoped allocation exceeds the materialized limit by one");
        assert_eq!(failure.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(failure.reason, ResourceFailure::BudgetExceeded);
        assert_eq!(failure.limit, 2);
        assert_eq!(failure.used, 0);
        assert_eq!(failure.additional, 3);
        assert_eq!(session.collection_items.get(), 3);
        assert_eq!(session.work.get(), 5);
        assert_eq!(session.materialized.get(), 0);
        assert_eq!(session.fused(), Some(failure));
        assert_eq!(
            budget
                .copy_recursion_path(&path)
                .expect_err("the storage refusal remains sticky"),
            failure
        );
        assert_eq!(session.collection_items.get(), 3);
        assert_eq!(session.work.get(), 5);
        assert_eq!(session.materialized.get(), 0);
    }

    #[test]
    fn scoped_allocator_refusal_reports_usage_before_the_request() {
        let mut policy = DecodePolicy::default();
        policy.limits.max_materialized_bytes = 20;
        let session = DecodeBudget::new(policy, 1);
        let prior = session
            .reserve_scoped_limit(5, "prior scratch")
            .expect("prior scoped storage fits");
        let request = session
            .reserve_scoped_limit(4, "cycle path")
            .expect("cycle path storage fits");

        let failure = session.scoped_allocation_failed_limit(4, "cycle path");
        assert_eq!(failure.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(failure.reason, ResourceFailure::AllocationFailed);
        assert_eq!(failure.limit, 20);
        assert_eq!(failure.used, 5);
        assert_eq!(failure.additional, 4);
        assert_eq!(session.fused(), Some(failure));
        drop(request);
        drop(prior);
        assert_eq!(session.materialized.get(), 0);
        assert_eq!(session.fused(), Some(failure));
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
    fn session_child_consumption_charges_the_session_once() {
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = 1;
        let session = DecodeBudget::new(policy, 1);
        let parent = WorkBudget::for_session(2, &session);
        let child = parent.session_child_slice(1);
        assert!(child.charge());
        assert_eq!(session.work.get(), 1);
        assert!(parent.consume_child(&child).is_ok());
        assert_eq!(parent.remaining(), 1);
        assert_eq!(session.work.get(), 1);
        assert!(session.fused().is_none());
    }

    #[test]
    fn scaled_session_work_preserves_local_ceiling_and_child_accounting() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 8;
        let session = DecodeBudget::new(policy, 1);
        let parent = WorkBudget::for_session(3, &session)
            .with_session_work_scale(std::num::NonZeroU64::new(4).expect("nonzero work scale"));
        let child = parent.session_child_slice(1);
        assert!(child.charge());
        assert_eq!(session.work.get(), 4);
        assert!(parent.consume_child(&child).is_ok());
        assert_eq!(parent.remaining(), 2);
        assert_eq!(session.work.get(), 4);
        assert!(parent.charge());
        assert_eq!(parent.remaining(), 1);
        assert_eq!(session.work.get(), 8);
        assert!(!parent.charge());
        assert!(parent.exhausted());
        let failure = session.fused().expect("session work refusal");
        assert_eq!(failure.dimension, super::ResourceDimension::WorkUnits);
        assert_eq!(failure.used, 8);
        assert_eq!(failure.additional, 4);
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
    #[test]
    fn later_refusals_preserve_the_first_session_refusal() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let session = DecodeBudget::new(policy, 1);
        let first = session
            .charge_work_limit(1, "first work refusal")
            .expect_err("refusal");
        let later = session.retained_allocation_failed_limit(7, "later allocator refusal");
        assert_eq!(later, first);
        assert_eq!(session.fused(), Some(first));
        let later = session.refuse_limit(
            ResourceDimension::Codec("later codec"),
            ResourceFailure::BudgetExceeded,
            2,
            2,
            1,
            "later codec",
        );
        assert_eq!(later, first);
        assert_eq!(session.fused(), Some(first));
    }
}
