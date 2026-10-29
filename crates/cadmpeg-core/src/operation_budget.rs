// SPDX-License-Identifier: Apache-2.0
//! Aggregate resources shared by an encode operation and its decode helpers.

use std::sync::{Mutex, MutexGuard};

use crate::decode::{
    u64_from_index, LimitScope, ResourceDimension, ResourceFailure, ResourceLimit, ResourceLimits,
};

#[derive(Debug)]
pub(crate) struct OperationBudget {
    pub(crate) limits: ResourceLimits,
    state: Mutex<OperationBudgetState>,
}

#[derive(Debug)]
pub(crate) struct OperationBudgetState {
    pub(crate) entities: u64,
    input: u64,
    decompressed: u64,
    retained: u64,
    failure: Option<ResourceLimit>,
}

impl OperationBudget {
    pub(crate) const fn new(limits: ResourceLimits, input: u64) -> Self {
        Self {
            limits,
            state: Mutex::new(OperationBudgetState {
                entities: 0,
                input,
                decompressed: 0,
                retained: 0,
                failure: None,
            }),
        }
    }

    pub(crate) fn state(&self) -> MutexGuard<'_, OperationBudgetState> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub(crate) fn fused(&self) -> Option<ResourceLimit> {
        self.state().failure
    }
    pub(crate) fn retained_used(&self) -> u64 {
        self.state().retained
    }
    pub(crate) fn retained_bytes_available(&self) -> u64 {
        let mut state = self.state();
        if state.failure.is_some() {
            return 0;
        }
        let limit = self.limits.max_retained_bytes;
        match limit.checked_sub(state.retained) {
            Some(remaining) => remaining,
            None => {
                let failure = ResourceLimit {
                    scope: LimitScope::Session,
                    dimension: ResourceDimension::RetainedBytes,
                    reason: ResourceFailure::BudgetExceeded,
                    limit,
                    used: state.retained,
                    additional: 0,
                    operation: "read retained-byte allowance",
                };
                Self::refuse_locked(&mut state, failure);
                0
            }
        }
    }
    pub(crate) fn entities_used(&self) -> u64 {
        self.state().entities
    }

    pub(crate) fn admit(
        &self,
        dimension: ResourceDimension,
        limit: u64,
        used: u64,
        additional: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        Self::admit_locked(
            &mut self.state(),
            dimension,
            limit,
            used,
            additional,
            operation,
        )
    }

    pub(crate) fn charge_input(
        &self,
        amount: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        let mut state = self.state();
        let used = state.input;
        Self::admit_locked(
            &mut state,
            ResourceDimension::InputBytes,
            self.limits.max_input_bytes,
            used,
            amount,
            operation,
        )?;
        state.input += amount;
        Ok(())
    }

    pub(crate) fn charge_decompressed(
        &self,
        amount: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        let mut state = self.state();
        let used = state.decompressed;
        Self::admit_locked(
            &mut state,
            ResourceDimension::DecompressedBytes,
            self.limits.max_decompressed_bytes_total,
            used,
            amount,
            operation,
        )?;
        state.decompressed += amount;
        Ok(())
    }

    pub(crate) fn allocation_failed(
        &self,
        additional: u64,
        operation: &'static str,
    ) -> ResourceLimit {
        let mut state = self.state();
        let resource = ResourceLimit {
            scope: LimitScope::Session,
            dimension: ResourceDimension::RetainedBytes,
            reason: ResourceFailure::AllocationFailed,
            limit: self.limits.max_retained_bytes,
            used: state.retained,
            additional,
            operation,
        };
        Self::refuse_locked(&mut state, resource)
    }

    pub(crate) fn charge_entities(
        &self,
        limit: u64,
        count: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        let mut state = self.state();
        let used = state.entities;
        Self::admit_locked(
            &mut state,
            ResourceDimension::Entities,
            limit.min(self.limits.max_entities),
            used,
            count,
            operation,
        )?;
        state.entities += count;
        Ok(())
    }

    pub(crate) fn charge_retained(
        &self,
        count: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        let mut state = self.state();
        let used = state.retained;
        Self::admit_locked(
            &mut state,
            ResourceDimension::RetainedBytes,
            self.limits.max_retained_bytes,
            used,
            count,
            operation,
        )?;
        state.retained += count;
        Ok(())
    }

    pub(crate) fn reserve_retained(
        &self,
        minimum: usize,
        preferred: usize,
        operation: &'static str,
    ) -> Result<usize, ResourceLimit> {
        let mut state = self.state();
        let used = state.retained;
        Self::admit_locked(
            &mut state,
            ResourceDimension::RetainedBytes,
            self.limits.max_retained_bytes,
            used,
            u64_from_index(minimum),
            operation,
        )?;
        let available = self.limits.max_retained_bytes - used;
        let growth =
            usize::try_from(available).map_or(preferred, |remaining| preferred.min(remaining));
        debug_assert!(
            growth >= minimum,
            "the preferred allocation covers the minimum growth"
        );
        state.retained += u64_from_index(growth);
        Ok(growth)
    }

    pub(crate) fn release_retained(&self, bytes: u64) {
        let mut state = self.state();
        if let Some(remaining) = state.retained.checked_sub(bytes) {
            state.retained = remaining;
        } else {
            debug_assert!(false, "only the buffer's own live reservation is refunded");
        }
    }

    fn admit_locked(
        state: &mut OperationBudgetState,
        dimension: ResourceDimension,
        limit: u64,
        used: u64,
        additional: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        if let Some(error) = state.failure {
            return Err(error);
        }
        if limit
            .checked_sub(used)
            .is_none_or(|remaining| additional > remaining)
        {
            return Err(Self::refuse_locked(
                state,
                ResourceLimit {
                    scope: LimitScope::for_dimension(dimension),
                    dimension,
                    reason: ResourceFailure::BudgetExceeded,
                    limit,
                    used,
                    additional,
                    operation,
                },
            ));
        }
        Ok(())
    }

    fn refuse_locked(state: &mut OperationBudgetState, resource: ResourceLimit) -> ResourceLimit {
        let resource = match state.failure {
            Some(previous)
                if previous.scope == LimitScope::Session || resource.scope == LimitScope::Part =>
            {
                previous
            }
            _ => resource,
        };
        state.failure = Some(resource);
        resource
    }

    pub(crate) fn refuse(&self, resource: ResourceLimit) -> ResourceLimit {
        Self::refuse_locked(&mut self.state(), resource)
    }
    pub(crate) fn recover_part(&self, expected: ResourceLimit) -> Result<(), ResourceLimit> {
        let mut state = self.state();
        match state.failure {
            Some(actual) if actual == expected && actual.scope == LimitScope::Part => {
                state.failure = None;
                Ok(())
            }
            Some(actual) => Err(actual),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    mod retained_allowance;
    use super::*;
    use std::sync::Arc;

    fn poison(account: &Arc<OperationBudget>) {
        let moved = Arc::clone(account);
        let thread = std::thread::spawn(move || {
            let _state = moved.state();
            panic!("poison the locked accounting state");
        });
        assert!(thread.join().is_err());
    }

    #[test]
    fn poisoned_lock_permanently_refuses_without_claiming_budget_exhaustion() {
        let mut limits = ResourceLimits::service();
        limits.max_retained_bytes = 8;
        let account = Arc::new(OperationBudget::new(limits, 0));
        let native = Box::new(*b"native");
        account
            .charge_retained(6, "native fields")
            .expect("native fits");
        poison(&account);
        assert_eq!(account.retained_used(), 6);
        assert_eq!(account.fused(), None);
        account
            .charge_retained(2, "verification fields")
            .expect("recovered accounting admits the remaining capacity");
        let verification = Box::new(*b"ok");
        assert_eq!(account.retained_used(), 8);
        assert_eq!(account.fused(), None);
        let error = account
            .charge_retained(1, "over limit after poison recovery")
            .expect_err("the actual retained ceiling remains enforced");
        assert_eq!(error.reason, ResourceFailure::BudgetExceeded);
        assert_eq!(error.scope, LimitScope::Session);
        assert_eq!(error.dimension, ResourceDimension::RetainedBytes);
        assert_eq!((error.limit, error.used, error.additional), (8, 8, 1));
        assert_eq!(error.operation, "over limit after poison recovery");
        assert_eq!(account.retained_used(), 8);
        account.release_retained(6);
        assert_eq!(account.retained_used(), 2);
        assert_eq!(account.fused(), Some(error));
        assert_eq!(account.charge_entities(1, 1, "later field"), Err(error));
        assert_eq!(account.charge_retained(0, "empty request"), Err(error));
        assert_eq!(account.recover_part(error), Err(error));
        assert_eq!(verification.as_slice(), b"ok");
        assert_eq!(native.as_slice(), b"native");
    }

    #[test]
    fn poisoned_lock_keeps_the_first_session_refusal() {
        let mut limits = ResourceLimits::service();
        limits.max_retained_bytes = 8;
        let account = Arc::new(OperationBudget::new(limits, 0));
        account
            .charge_retained(6, "native fields")
            .expect("native fits");
        let first = account.charge_retained(3, "over limit").expect_err("limit");
        poison(&account);
        assert_eq!(account.fused(), Some(first));
        assert_eq!(account.charge_entities(1, 1, "later field"), Err(first));
        assert_eq!(account.recover_part(first), Err(first));
    }

    #[test]
    fn poisoned_lock_preserves_entity_usage_and_live_capacity_reservations() {
        let mut limits = ResourceLimits::service();
        limits.max_retained_bytes = 8;
        limits.max_entities = 3;
        let account = Arc::new(OperationBudget::new(limits, 0));
        account
            .charge_retained(6, "native fields")
            .expect("native fields fit");
        account
            .charge_entities(3, 3, "native entities")
            .expect("native entities fit");
        poison(&account);
        assert_eq!(account.retained_used(), 6);
        assert_eq!(account.entities_used(), 3);
        assert_eq!(account.fused(), None);
        assert_eq!(
            account
                .reserve_retained(2, 8, "live output")
                .expect("remaining output capacity fits"),
            2
        );
        assert_eq!(account.retained_used(), 8);
        assert_eq!(account.entities_used(), 3);
        assert_eq!(account.fused(), None);
        let first = account
            .charge_entities(3, 1, "additional entity")
            .expect_err("the actual entity ceiling remains enforced");
        assert_eq!(first.dimension, ResourceDimension::Entities);
        assert_eq!(first.reason, ResourceFailure::BudgetExceeded);
        assert_eq!((first.limit, first.used, first.additional), (3, 3, 1));
        account.release_retained(2);
        poison(&account);
        assert_eq!(account.retained_used(), 6);
        assert_eq!(account.entities_used(), 3);
        assert_eq!(account.fused(), Some(first));
        assert_eq!(account.reserve_retained(0, 0, "empty output"), Err(first));
        assert_eq!(account.charge_retained(0, "empty fields"), Err(first));
        assert_eq!(account.recover_part(first), Err(first));
    }
}
