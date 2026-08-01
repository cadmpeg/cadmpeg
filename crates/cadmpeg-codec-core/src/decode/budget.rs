// SPDX-License-Identifier: Apache-2.0
//! Unified resource accounting for one decode session.

use std::cell::Cell;

use crate::CodecError;

use super::error::{
    ErrorContext, LimitScope, ResourceDimension, ResourceFailure, ResourceLimit, SourceLocation,
};
use super::policy::{
    DecodePolicy, DECOMPRESSED_TOTAL_BASE, DECOMPRESSED_TOTAL_PER_INPUT_BYTE, MATERIALIZED_BASE,
    MATERIALIZED_PER_INPUT_BYTE, RETAINED_BASE, RETAINED_PER_INPUT_BYTE,
};

#[derive(Debug)]
pub(crate) struct DecodeBudget {
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
    pub(crate) fn new(policy: DecodePolicy, input_bytes: u64) -> Self {
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

    pub(crate) fn fused(&self) -> Option<ResourceLimit> {
        self.fuse.get()
    }

    pub(crate) fn decompression_allowance(&self) -> u64 {
        let proportional = DECOMPRESSED_TOTAL_BASE
            .saturating_add(DECOMPRESSED_TOTAL_PER_INPUT_BYTE.saturating_mul(self.input_bytes));
        self.policy
            .limits
            .max_decompressed_bytes_total
            .min(proportional)
    }

    pub(crate) fn input_bytes(&self) -> u64 {
        self.input_bytes
    }

    pub(crate) fn charge_decompressed(
        &self,
        amount: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<(), CodecError> {
        self.charge(
            ResourceDimension::DecompressedBytes,
            &self.decompressed,
            self.decompression_allowance(),
            amount,
            operation,
            location,
        )
    }

    pub(crate) fn decompressed_used(&self) -> u64 {
        self.decompressed.get()
    }

    fn materialized_allowance(&self) -> u64 {
        self.policy.limits.max_materialized_bytes.min(
            MATERIALIZED_BASE
                .saturating_add(MATERIALIZED_PER_INPUT_BYTE.saturating_mul(self.input_bytes)),
        )
    }

    fn retained_allowance(&self) -> u64 {
        self.policy.limits.max_retained_bytes.min(
            RETAINED_BASE.saturating_add(RETAINED_PER_INPUT_BYTE.saturating_mul(self.input_bytes)),
        )
    }

    fn charge(
        &self,
        dimension: ResourceDimension,
        used: &Cell<u64>,
        limit: u64,
        amount: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<(), CodecError> {
        if let Some(resource) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(resource));
        }
        let before = used.get();
        if amount > limit.saturating_sub(before) {
            return Err(self.refuse(
                dimension,
                ResourceFailure::BudgetExceeded,
                LimitScope::Global,
                limit,
                before,
                amount,
                operation,
                location,
            ));
        }
        used.set(before + amount);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn refuse(
        &self,
        dimension: ResourceDimension,
        reason: ResourceFailure,
        scope: LimitScope,
        limit: u64,
        used: u64,
        additional: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> CodecError {
        let resource = ResourceLimit {
            dimension,
            reason,
            scope,
            limit,
            used,
            additional,
            context: ErrorContext {
                operation,
                location,
            },
        };
        self.fuse.set(Some(resource));
        CodecError::ResourceLimit(resource)
    }

    pub(crate) fn reserve_scoped(
        &self,
        bytes: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<ScopedReservation<'_>, CodecError> {
        self.charge(
            ResourceDimension::MaterializedBytes,
            &self.materialized,
            self.materialized_allowance(),
            bytes,
            operation,
            location,
        )?;
        Ok(ScopedReservation {
            budget: self,
            bytes,
            operation,
            location,
        })
    }

    pub(crate) fn charge_retained(
        &self,
        bytes: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<(), CodecError> {
        self.charge(
            ResourceDimension::RetainedBytes,
            &self.retained,
            self.retained_allowance(),
            bytes,
            operation,
            location,
        )
    }

    pub(crate) fn charge_entities(
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
            None,
        )
    }

    pub(crate) fn charge_collection_items(
        &self,
        count: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge(
            ResourceDimension::CollectionItems,
            &self.collection_items,
            self.policy.limits.max_collection_items,
            count,
            operation,
            None,
        )
    }

    pub(crate) fn enter_nested(
        &self,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<DepthGuard<'_>, CodecError> {
        self.charge(
            ResourceDimension::RecursionDepth,
            &self.recursion_depth,
            self.policy.limits.max_recursion_depth,
            1,
            operation,
            location,
        )?;
        Ok(DepthGuard { budget: self })
    }

    pub(crate) fn charge_work(
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
            None,
        )
    }
}

/// A temporary materialization charged until it is dropped or committed.
#[derive(Debug)]
pub struct ScopedReservation<'a> {
    budget: &'a DecodeBudget,
    bytes: u64,
    operation: &'static str,
    location: Option<SourceLocation>,
}

impl ScopedReservation<'_> {
    /// Increases the live temporary reservation.
    pub fn grow(&mut self, bytes: u64) -> Result<(), CodecError> {
        self.budget.charge(
            ResourceDimension::MaterializedBytes,
            &self.budget.materialized,
            self.budget.materialized_allowance(),
            bytes,
            self.operation,
            self.location,
        )?;
        self.bytes = self.bytes.saturating_add(bytes);
        Ok(())
    }

    /// Converts the temporary reservation into session-retained bytes.
    pub fn commit(self) -> Result<(), CodecError> {
        self.budget
            .charge_retained(self.bytes, self.operation, self.location)
    }

    /// Returns the currently reserved byte count.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

impl Drop for ScopedReservation<'_> {
    fn drop(&mut self) {
        self.budget
            .materialized
            .set(self.budget.materialized.get().saturating_sub(self.bytes));
    }
}

/// A guard holding one unit of recursive nesting depth.
#[derive(Debug)]
pub struct DepthGuard<'a> {
    budget: &'a DecodeBudget,
}

impl Drop for DepthGuard<'_> {
    fn drop(&mut self) {
        self.budget
            .recursion_depth
            .set(self.budget.recursion_depth.get().saturating_sub(1));
    }
}

/// A sticky, context-free local work counter.
#[derive(Debug)]
pub struct WorkBudget<'a> {
    limit: usize,
    remaining: Cell<usize>,
    exhausted: Cell<bool>,
    session: Option<&'a DecodeBudget>,
}

impl WorkBudget<'static> {
    /// Creates an independent local work slice.
    pub const fn new(limit: usize) -> Self {
        Self {
            limit,
            remaining: Cell::new(limit),
            exhausted: Cell::new(false),
            session: None,
        }
    }
}

impl<'a> WorkBudget<'a> {
    pub(crate) fn for_session(limit: u64, session: &'a DecodeBudget) -> Self {
        let limit = if limit > usize::MAX as u64 {
            usize::MAX
        } else {
            limit as usize
        };
        Self {
            limit,
            remaining: Cell::new(limit),
            exhausted: Cell::new(false),
            session: Some(session),
        }
    }

    /// Charges one work unit, returning false after exhaustion.
    pub fn charge(&self) -> bool {
        self.charge_by(1)
    }

    /// Charges several work units, with sticky exhaustion on refusal.
    pub fn charge_by(&self, work: usize) -> bool {
        if self.exhausted.get() {
            return false;
        }
        let remaining = self.remaining.get();
        if work > remaining {
            self.exhausted.set(true);
            false
        } else {
            if let Some(session) = self.session {
                if session.charge_work(work as u64, "work_budget").is_err() {
                    self.exhausted.set(true);
                    return false;
                }
            }
            self.remaining.set(remaining - work);
            true
        }
    }

    /// Returns whether this slice has refused a charge.
    pub fn exhausted(&self) -> bool {
        self.exhausted.get()
    }

    /// Marks this slice exhausted without consuming additional session work.
    pub fn exhaust(&self) {
        self.exhausted.set(true);
    }

    /// Returns unconsumed work units.
    pub fn remaining(&self) -> usize {
        self.remaining.get()
    }

    /// Returns work units consumed by successful charges.
    pub fn consumed(&self) -> usize {
        self.limit.saturating_sub(self.remaining.get())
    }

    /// Creates an independent child slice capped by this budget's remainder.
    pub fn child_slice(&self, limit: usize) -> WorkBudget<'static> {
        WorkBudget::new(limit.min(self.remaining()))
    }

    /// Charges this budget for work consumed by a child slice.
    pub fn consume_child(&self, child: &WorkBudget<'_>) -> bool {
        self.charge_by(child.consumed())
    }

    /// Classifies this local budget's refusal as a resource limit.
    pub fn refuse(&self, operation: &'static str) -> CodecError {
        if let Some(resource) = self.session.and_then(DecodeBudget::fused) {
            return CodecError::ResourceLimit(resource);
        }
        local_limit_error(
            "work_units",
            self.limit as u64,
            self.consumed().saturating_add(1) as u64,
            operation,
            None,
        )
    }
}

/// Builds a correctly classified refusal for a codec-local ceiling.
pub fn refuse_local_limit(
    what: &'static str,
    limit: u64,
    requested: u64,
    location: Option<SourceLocation>,
) -> CodecError {
    local_limit_error(what, limit, requested, what, location)
}

fn local_limit_error(
    what: &'static str,
    limit: u64,
    requested: u64,
    operation: &'static str,
    location: Option<SourceLocation>,
) -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        dimension: ResourceDimension::Codec(what),
        reason: ResourceFailure::BudgetExceeded,
        scope: LimitScope::Global,
        limit,
        used: requested.min(limit),
        additional: requested.saturating_sub(limit),
        context: ErrorContext {
            operation,
            location,
        },
    })
}
