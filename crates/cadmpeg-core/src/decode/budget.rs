// SPDX-License-Identifier: Apache-2.0
//! Resource ceilings for one decode session, and the local work counter.
//!
//! A session counts input bytes, decompressed bytes, retained output bytes,
//! entities, and recursion depth. A session refusal permanently fuses the
//! budget. A part refusal blocks further work until its owning part runner
//! acknowledges it after rollback; cumulative resource charges remain.

use std::cell::Cell;
use std::rc::Rc;

use crate::CodecError;

use super::error::{LimitScope, ResourceDimension, ResourceFailure, ResourceLimit};
use super::policy::{
    DecodePolicy, DECOMPRESSED_TOTAL_BASE, DECOMPRESSED_TOTAL_PER_INPUT_BYTE, RETAINED_BASE,
    RETAINED_PER_INPUT_BYTE,
};

#[derive(Debug)]
pub(super) struct DecodeBudget {
    /// Policy applied to this budget.
    policy: DecodePolicy,
    input_bytes: Cell<u64>,
    decompressed: Cell<u64>,
    retained: Cell<u64>,
    entities: Cell<u64>,
    recursion_depth: Cell<u64>,
    fuse: Cell<Option<ResourceLimit>>,
}

impl DecodeBudget {
    pub(super) fn new(policy: DecodePolicy, input_bytes: u64) -> Self {
        Self {
            policy,
            input_bytes: Cell::new(input_bytes),
            decompressed: Cell::new(0),
            retained: Cell::new(0),
            entities: Cell::new(0),
            recursion_depth: Cell::new(0),
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

    fn retained_allowance(&self) -> u64 {
        self.proportional_allowance(
            self.policy.limits.max_retained_bytes,
            RETAINED_BASE,
            RETAINED_PER_INPUT_BYTE,
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
            scope: LimitScope::for_dimension(dimension),
            dimension,
            reason,
            limit,
            used,
            additional,
            operation,
        };
        self.refuse_resource(resource)
    }

    pub(super) fn refuse_resource(&self, resource: ResourceLimit) -> ResourceLimit {
        // A later part refusal cannot replace a session refusal. A session
        // refusal remains fatal even if a part fault was raised first.
        let resource = match self.fuse.get() {
            Some(previous)
                if previous.scope == LimitScope::Session || resource.scope == LimitScope::Part =>
            {
                previous
            }
            _ => resource,
        };
        self.fuse.set(Some(resource));
        resource
    }

    pub(super) fn recover_part(&self, expected: ResourceLimit) -> Result<(), ResourceLimit> {
        match self.fuse.get() {
            Some(actual) if actual == expected && actual.scope == LimitScope::Part => {
                self.fuse.set(None);
                Ok(())
            }
            Some(actual) => Err(actual),
            None => Ok(()),
        }
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

    /// Retained bytes the session can still charge before `retain_bytes` refuses.
    pub(super) fn retained_bytes_available(&self) -> u64 {
        if self.fuse.get().is_some() {
            return 0;
        }
        // A session that retained past its allowance can afford nothing more.
        let Some(remaining) = self.retained_allowance().checked_sub(self.retained.get()) else {
            return 0;
        };
        remaining
    }

    /// Reports an allocator refusal for `requested` bytes or slots.
    pub(super) fn retained_allocation_failed(
        &self,
        requested: u64,
        operation: &'static str,
    ) -> CodecError {
        self.refuse_limit(
            ResourceDimension::RetainedBytes,
            ResourceFailure::AllocationFailed,
            self.retained_allowance(),
            self.retained.get(),
            requested,
            operation,
        )
        .into()
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

    pub(super) fn charge_nested(&self, operation: &'static str) -> Result<(), ResourceLimit> {
        self.charge(
            ResourceDimension::RecursionDepth,
            &self.recursion_depth,
            self.policy.limits.max_recursion_depth,
            1,
            operation,
        )
    }

    pub(super) fn enter_levels(
        &self,
        levels: u64,
        operation: &'static str,
    ) -> Result<LocalDepthGuard<'_>, ResourceLimit> {
        self.charge(
            ResourceDimension::RecursionDepth,
            &self.recursion_depth,
            self.policy.limits.max_recursion_depth,
            levels,
            operation,
        )?;
        Ok(LocalDepthGuard {
            budget: self,
            levels,
        })
    }

    pub(super) fn active_depth(&self) -> u64 {
        self.recursion_depth.get()
    }

    fn leave_nested(&self) {
        if let Some(depth) = self.recursion_depth.get().checked_sub(1) {
            self.recursion_depth.set(depth);
        }
    }
}

/// Synchronous source-lineage nesting while the session state is borrowed.
pub(super) struct LocalDepthGuard<'a> {
    budget: &'a DecodeBudget,
    levels: u64,
}

impl Drop for LocalDepthGuard<'_> {
    fn drop(&mut self) {
        if let Some(depth) = self.budget.recursion_depth.get().checked_sub(self.levels) {
            self.budget.recursion_depth.set(depth);
        }
    }
}

/// A guard holding one unit of recursive nesting depth.
#[derive(Debug)]
pub struct DepthGuard<'a> {
    budget: super::tracking::SessionHandle<'a>,
}

impl Drop for DepthGuard<'_> {
    fn drop(&mut self) {
        self.budget.with_budget(DecodeBudget::leave_nested);
        self.budget.release_guard();
    }
}

impl<'a> super::tracking::SessionHandle<'a> {
    pub(super) fn enter_nested(
        self,
        operation: &'static str,
    ) -> Result<DepthGuard<'a>, ResourceLimit> {
        self.with_budget(|budget| budget.charge_nested(operation))?;
        Ok(DepthGuard { budget: self })
    }
}

/// A local work counter for one bounded search or iteration.
///
/// Exhaustion is sticky: once a charge is refused, every later charge is
/// refused. A caller that runs out skips the step it bounds and reports the
/// skip; exhaustion never refuses the decode session.
#[derive(Debug)]
pub struct WorkBudget {
    limit: usize,
    account: Rc<WorkAccount>,
    recursion_depth: Cell<usize>,
}

/// Remaining units of one counter, and the counter that pays for its work.
#[derive(Debug)]
struct WorkAccount {
    remaining: Cell<Option<usize>>,
    parent: Option<Rc<WorkAccount>>,
}

/// RAII guard for one recursive frame of a [`WorkBudget`].
#[derive(Debug)]
pub struct WorkBudgetRecursionGuard<'budget> {
    budget: &'budget WorkBudget,
}

impl Drop for WorkBudgetRecursionGuard<'_> {
    fn drop(&mut self) {
        if let Some(depth) = self.budget.recursion_depth.get().checked_sub(1) {
            self.budget.recursion_depth.set(depth);
        }
    }
}

impl WorkBudget {
    /// The recursion ceiling of [`WorkBudget::recursion_guard`].
    pub const RECURSION_DEPTH: usize = 256;

    /// Creates a work counter with `limit` units.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self::with_parent(limit, None)
    }

    fn with_parent(limit: usize, parent: Option<Rc<WorkAccount>>) -> Self {
        Self {
            limit,
            account: Rc::new(WorkAccount {
                remaining: Cell::new(Some(limit)),
                parent,
            }),
            recursion_depth: Cell::new(0),
        }
    }

    fn ancestors(&self) -> impl Iterator<Item = &WorkAccount> {
        std::iter::successors(self.account.parent.as_deref(), |account| {
            account.parent.as_deref()
        })
    }

    /// Charges one work unit, returning false after exhaustion.
    pub fn charge(&self) -> bool {
        self.charge_by(1)
    }

    /// Charges several work units, with sticky exhaustion on refusal.
    ///
    /// A charging slice also charges every counter it was drawn from. A
    /// charge that one of them cannot pay exhausts this counter only.
    pub fn charge_by(&self, work: usize) -> bool {
        let charged = self.debit(work);
        #[cfg(feature = "test-support")]
        if charged {
            super::test_support::record_work(work);
        }
        charged
    }

    fn debit(&self, work: usize) -> bool {
        let Some(remaining) = self.account.remaining.get() else {
            return false;
        };
        let affordable = |account: &WorkAccount| {
            account
                .remaining
                .get()
                .is_some_and(|remaining| work <= remaining)
        };
        if work > remaining || !self.ancestors().all(affordable) {
            self.exhaust();
            return false;
        }
        self.account.remaining.set(Some(remaining - work));
        for account in self.ancestors() {
            account
                .remaining
                .set(account.remaining.get().map(|remaining| remaining - work));
        }
        true
    }

    /// Returns whether this counter has refused a charge.
    pub fn exhausted(&self) -> bool {
        self.account.remaining.get().is_none()
    }

    /// Marks this counter exhausted. The counters it was drawn from are not.
    pub fn exhaust(&self) {
        self.account.remaining.set(None);
    }

    /// Returns the work units a charge can still use: the smallest remainder
    /// of this counter and every counter it was drawn from.
    pub fn remaining(&self) -> usize {
        std::iter::once(&*self.account)
            .chain(self.ancestors())
            .map(|account| account.remaining.get().unwrap_or(0))
            .min()
            .unwrap_or(0)
    }

    /// Returns work charged to this counter and the remainder forfeited by
    /// exhaustion.
    pub fn consumed(&self) -> usize {
        self.limit - self.account.remaining.get().unwrap_or(0)
    }

    /// Enters one recursive frame, refusing past [`WorkBudget::RECURSION_DEPTH`].
    /// The refusal exhausts the counter; it does not touch a decode session.
    pub fn recursion_guard(&self) -> Result<WorkBudgetRecursionGuard<'_>, BudgetExhausted> {
        let depth = self.recursion_depth.get();
        if depth >= Self::RECURSION_DEPTH {
            self.exhaust();
            return Err(BudgetExhausted);
        }
        self.recursion_depth.set(depth + 1);
        Ok(WorkBudgetRecursionGuard { budget: self })
    }

    /// Creates a child counter capped by this counter's remainder.
    ///
    /// The child's charges do not reach this counter until
    /// [`WorkBudget::consume_child`] transfers them.
    #[must_use]
    pub fn child_slice(&self, limit: usize) -> WorkBudget {
        WorkBudget::new(limit.min(self.remaining()))
    }

    /// Creates a child counter of at most `limit` units whose every charge is
    /// also charged to this counter, so the work below a counter never
    /// exceeds its limit.
    ///
    /// Exhausting the child, by its own limit or by an ancestor that cannot
    /// pay, leaves this counter usable.
    #[must_use]
    pub fn charging_slice(&self, limit: usize) -> WorkBudget {
        WorkBudget::with_parent(limit.min(self.remaining()), Some(Rc::clone(&self.account)))
    }

    /// Charges this counter for work consumed by a child counter.
    ///
    /// # Errors
    ///
    /// [`BudgetExhausted`] when the child's work is above this counter's
    /// remainder. The counter marks itself exhausted in that case.
    pub fn consume_child(&self, child: &WorkBudget) -> Result<(), BudgetExhausted> {
        if self.debit(child.consumed()) {
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

#[cfg(test)]
mod tests {
    use super::{work_units, DecodeBudget, WorkBudget, RETAINED_BASE, RETAINED_PER_INPUT_BYTE};
    use crate::decode::{DecodePolicy, ResourceDimension, ResourceFailure};

    fn descend(budget: &WorkBudget, depth: usize) -> usize {
        let Ok(_guard) = budget.recursion_guard() else {
            return depth;
        };
        descend(budget, depth + 1)
    }

    #[test]
    fn recursion_guard_is_bounded_and_exhausts_the_counter() {
        let budget = WorkBudget::new(10_000);
        assert_eq!(descend(&budget, 0), WorkBudget::RECURSION_DEPTH);
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
    fn child_consumption_transfers_to_the_parent_once() {
        let parent = WorkBudget::new(3);
        let child = parent.child_slice(2);
        assert!(child.charge());
        assert!(parent.consume_child(&child).is_ok());
        assert_eq!(parent.remaining(), 2);
        let greedy = WorkBudget::new(5);
        assert!(greedy.charge_by(5));
        assert!(parent.consume_child(&greedy).is_err());
        assert!(parent.exhausted());
    }

    #[test]
    fn charging_slices_charge_every_ancestor_and_exhaust_locally() {
        let root = WorkBudget::new(10);
        let child = root.charging_slice(8);
        let grandchild = child.charging_slice(100);
        assert_eq!(grandchild.remaining(), 8);
        assert!(grandchild.charge_by(3));
        assert_eq!((root.remaining(), child.remaining()), (7, 5));
        assert_eq!(grandchild.consumed(), 3);

        assert!(root.charge_by(5));
        assert_eq!(grandchild.remaining(), 2);
        assert!(!grandchild.charge_by(3));
        assert!(grandchild.exhausted());
        assert!(!child.exhausted() && !root.exhausted());
        assert_eq!((root.remaining(), child.remaining()), (2, 2));

        let sibling = child.charging_slice(1);
        assert!(sibling.charge());
        assert!(!sibling.charge());
        assert!(!child.exhausted());
        assert_eq!(root.remaining(), 1);

        root.exhaust();
        assert!(!child.charge());
        assert!(child.exhausted());
        assert_eq!(root.consumed(), 10);
    }

    #[test]
    fn retained_allocation_refusal_fuses_with_current_usage() {
        let session = DecodeBudget::new(DecodePolicy::default(), 1);
        session
            .retain_bytes(3, "copy retained")
            .expect("retained charge fits the default session allowance");
        let crate::CodecError::ResourceLimit(limit) =
            session.retained_allocation_failed(5, "copy retained")
        else {
            panic!("retained allocation must produce a resource refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(limit.reason, ResourceFailure::AllocationFailed);
        assert_eq!(limit.used, 3);
        assert_eq!(limit.additional, 5);
        assert_eq!(session.fused(), Some(limit));
    }

    #[test]
    fn retained_bytes_available_reports_proportional_allowance() {
        let policy = DecodePolicy::default();
        let input = 3;
        let expected = policy
            .limits
            .max_retained_bytes
            .min(RETAINED_BASE + RETAINED_PER_INPUT_BYTE * input);
        let session = DecodeBudget::new(policy, input);
        assert_eq!(session.retained_bytes_available(), expected);
    }

    #[test]
    fn retained_bytes_available_reports_policy_ceiling() {
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = RETAINED_BASE / 2;
        let session = DecodeBudget::new(policy, 3);
        assert_eq!(
            session.retained_bytes_available(),
            policy.limits.max_retained_bytes
        );
    }

    #[test]
    fn retained_bytes_available_reads_without_charging_or_fusing() {
        let policy = DecodePolicy::default();
        let input = 3;
        let allowance = policy
            .limits
            .max_retained_bytes
            .min(RETAINED_BASE + RETAINED_PER_INPUT_BYTE * input);
        let session = DecodeBudget::new(policy, input);
        let retained = 7;
        assert!(session.retain_bytes(retained, "retain").is_ok());
        for _ in 0..3 {
            assert_eq!(session.retained_bytes_available(), allowance - retained);
            assert_eq!(session.fused(), None);
        }
        assert!(session
            .retain_bytes(allowance - retained, "retain remaining")
            .is_ok());
        assert_eq!(session.retained_bytes_available(), 0);
        assert_eq!(session.fused(), None);
    }

    #[test]
    fn retained_bytes_available_is_zero_after_any_dimension_fuses() {
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = RETAINED_BASE / 2;
        let retained_session = DecodeBudget::new(policy, 3);
        assert!(retained_session
            .retain_bytes(policy.limits.max_retained_bytes + 1, "retain over limit")
            .is_err());
        assert!(retained_session.fused().is_some());
        assert_eq!(retained_session.retained_bytes_available(), 0);

        let entity_session = DecodeBudget::new(policy, 3);
        assert!(entity_session
            .charge_entities(policy.limits.max_entities + 1, "entities over limit")
            .is_err());
        assert!(entity_session.fused().is_some());
        assert_eq!(entity_session.retained_bytes_available(), 0);
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
}
