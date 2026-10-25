// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn remaining_retained_bytes_follow_actual_charges_and_live_capacity_release() {
    let mut limits = ResourceLimits::service();
    limits.max_retained_bytes = 8;
    let account = OperationBudget::new(limits, 0);
    assert_eq!(account.retained_bytes_available(), 8);
    account.charge_retained(3, "three retained fields").unwrap();
    assert_eq!(account.retained_bytes_available(), 5);
    assert_eq!(
        account
            .reserve_retained(2, 4, "live encoded allocation")
            .unwrap(),
        4
    );
    assert_eq!(account.retained_bytes_available(), 1);
    account.release_retained(4);
    assert_eq!(account.retained_bytes_available(), 5);
    assert!(account.fused().is_none());
}

#[test]
fn inconsistent_retained_usage_latches_the_actual_checked_subtraction_failure() {
    let mut limits = ResourceLimits::service();
    limits.max_retained_bytes = 8;
    let account = OperationBudget::new(limits, 0);
    account.state().retained = 9;
    assert_eq!(account.retained_bytes_available(), 0);
    let failure = account.fused().expect("checked accounting failure");
    assert_eq!(failure.scope, LimitScope::Session);
    assert_eq!(failure.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(failure.reason, ResourceFailure::BudgetExceeded);
    assert_eq!((failure.limit, failure.used, failure.additional), (8, 9, 0));
    assert_eq!(failure.operation, "read retained-byte allowance");
    assert_eq!(
        account.charge_retained(0, "no work after failure"),
        Err(failure)
    );
    account.release_retained(1);
    assert_eq!(account.retained_used(), 8);
    assert_eq!(account.retained_bytes_available(), 0);
    assert_eq!(account.fused(), Some(failure));
}

#[test]
fn reading_an_inconsistent_allowance_never_replaces_the_existing_session_failure() {
    let mut limits = ResourceLimits::service();
    limits.max_retained_bytes = 8;
    limits.max_entities = 0;
    let account = OperationBudget::new(limits, 0);
    let prior = account
        .charge_entities(0, 1, "first actual entity refusal")
        .unwrap_err();
    account.state().retained = 9;
    assert_eq!(account.retained_bytes_available(), 0);
    assert_eq!(account.fused(), Some(prior));
    assert_eq!(account.charge_retained(0, "later allowance"), Err(prior));
}
