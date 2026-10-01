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
    input_bytes: u64,
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
            input_bytes,
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

    pub(super) fn decompression_allowance(&self) -> u64 {
        let policy_limit = self.policy.limits.max_decompressed_bytes_total;
        let Some(proportional) = DECOMPRESSED_TOTAL_PER_INPUT_BYTE
            .checked_mul(self.input_bytes)
            .and_then(|bytes| DECOMPRESSED_TOTAL_BASE.checked_add(bytes))
        else {
            return policy_limit;
        };
        policy_limit.min(proportional)
    }

    pub(super) fn input_bytes(&self) -> u64 {
        self.input_bytes
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
        let policy_limit = self.policy.limits.max_materialized_bytes;
        let Some(proportional) = MATERIALIZED_PER_INPUT_BYTE
            .checked_mul(self.input_bytes)
            .and_then(|bytes| MATERIALIZED_BASE.checked_add(bytes))
        else {
            return policy_limit;
        };
        policy_limit.min(proportional)
    }

    fn retained_allowance(&self) -> u64 {
        let policy_limit = self.policy.limits.max_retained_bytes;
        let Some(proportional) = RETAINED_PER_INPUT_BYTE
            .checked_mul(self.input_bytes)
            .and_then(|bytes| RETAINED_BASE.checked_add(bytes))
        else {
            return policy_limit;
        };
        policy_limit.min(proportional)
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

    fn refuse_limit(
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
        Ok(ScopedReservation {
            budget: self,
            bytes,
            operation,
        })
    }

    pub(super) fn reserve_scoped_resource(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<ScopedReservation<'_>, ResourceLimit> {
        self.reserve_scoped_limit(bytes, operation)
    }

    /// Report allocator refusal after a scoped-byte reservation was recorded.
    pub(super) fn scoped_allocation_failed(
        &self,
        charged: u64,
        operation: &'static str,
    ) -> CodecError {
        self.refuse(
            ResourceDimension::MaterializedBytes,
            ResourceFailure::AllocationFailed,
            self.materialized_allowance(),
            // The successful charge is still live when allocation fails.
            self.materialized.get() - charged,
            charged,
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
        self.charge(
            ResourceDimension::RetainedBytes,
            &self.retained,
            self.retained_allowance(),
            bytes,
            operation,
        )
    }

    pub(super) fn preflight_retained(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(resource) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(resource));
        }
        let used = self.retained.get();
        let limit = self.retained_allowance();
        if used > limit || bytes > limit - used {
            return Err(self.refuse(
                ResourceDimension::RetainedBytes,
                ResourceFailure::BudgetExceeded,
                limit,
                used,
                bytes,
                operation,
            ));
        }
        Ok(())
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
        let current = self.retained.get();
        let Some(prior) = current.checked_sub(charged) else {
            return self.refuse_limit(
                ResourceDimension::RetainedBytes,
                ResourceFailure::AllocationFailed,
                self.retained_allowance(),
                current,
                charged,
                operation,
            );
        };
        self.refuse_limit(
            ResourceDimension::RetainedBytes,
            ResourceFailure::AllocationFailed,
            self.retained_allowance(),
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
        self.refuse(
            ResourceDimension::CollectionItems,
            ResourceFailure::AllocationFailed,
            self.policy.limits.max_collection_items,
            self.collection_items.get() - charged,
            charged,
            operation,
        )
    }

    /// Report allocator refusal after a collection-item charge was recorded.
    pub(super) fn collection_allocation_failed_requested(
        &self,
        charged: u64,
        requested: u64,
        operation: &'static str,
    ) -> CodecError {
        self.refuse(
            ResourceDimension::CollectionItems,
            ResourceFailure::AllocationFailed,
            self.policy.limits.max_collection_items,
            // The successful charge is still live when allocation fails.
            self.collection_items.get() - charged,
            requested,
            operation,
        )
    }

    pub(super) fn enter_nested(
        &self,
        operation: &'static str,
    ) -> Result<DepthGuard<'_>, CodecError> {
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
        self.charge(
            ResourceDimension::WorkUnits,
            &self.work,
            self.policy.limits.max_work_units,
            units,
            operation,
        )
        .map_err(Into::into)
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
    pub(super) fn grow_resource(&mut self, bytes: u64) -> Result<(), ResourceLimit> {
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
        self.grow_resource(bytes).map_err(Into::into)
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
    budget: &'budget WorkBudget<'session>,
}

impl Drop for WorkBudgetRecursionGuard<'_, '_> {
    fn drop(&mut self) {
        let Some(depth) = self.budget.recursion_depth.get().checked_sub(1) else {
            return;
        };
        self.budget.recursion_depth.set(depth);
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
                let Some(scaled) =
                    u64_from_index(work).checked_mul(self.session_work_scale.get())
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

    /// Enters one recursive geometry-evaluation frame.
    ///
    /// The depth is shared by all model curve and surface calls using this
    /// slice, so cross-carrier cycles cannot reset a local recursion limit.
    pub fn recursion_guard(&self) -> Option<WorkBudgetRecursionGuard<'_, 'a>> {
        const MAX_WORK_RECURSION_DEPTH: usize = 256;
        if self.remaining.get().is_none() || self.recursion_depth.get() >= MAX_WORK_RECURSION_DEPTH
        {
            self.exhaust();
            return None;
        }
        self.recursion_depth.set(self.recursion_depth.get() + 1);
        Some(WorkBudgetRecursionGuard { budget: self })
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

/// Allocates `count` copies of `value` with `try_reserve_exact` and no panic on OOM.
///
/// Use [`crate::decode::DecodeContext::alloc_filled`] when a decode session can also charge
/// collection items. This helper is for call sites that only need the
/// allocation bound.
pub fn alloc_filled<T: Clone>(
    count: usize,
    value: T,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut out = Vec::new();
    out.try_reserve_exact(count).map_err(|_| {
        crate::CodecError::ResourceLimit(crate::decode::ResourceLimit::allocation_failed(
            crate::decode::ResourceDimension::Codec(operation),
            u64_from_index(count),
            u64_from_index(count),
            operation,
        ))
    })?;
    for _ in 0..count {
        out.push(value.clone());
    }
    Ok(out)
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
        let Some(_guard) = budget.recursion_guard() else {
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
}
