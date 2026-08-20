// SPDX-License-Identifier: Apache-2.0
//! History resource-budget unit tests.
#![allow(clippy::unwrap_used)]

use super::super::*;

#[test]
fn history_binding_work_budget_charges_state_record_cross_product() {
    let desktop = cadmpeg_core::decode::ResourceLimits::desktop();
    let desktop_entries = desktop.max_work_units / HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY;
    assert!(!history_topology_work_budget_exceeded(
        [usize::try_from(desktop_entries).expect("desktop entry budget fits usize")],
        &desktop
    ));
    assert!(history_topology_work_budget_exceeded(
        [usize::try_from(desktop_entries + 1).expect("desktop entry overflow fits usize")],
        &desktop
    ));
    assert!(history_topology_work_budget_exceeded(
        [usize::MAX, 1],
        &desktop
    ));

    let service = cadmpeg_core::decode::ResourceLimits::service();
    let service_entries = service.max_work_units / HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY;
    assert!(!history_topology_work_budget_exceeded(
        [usize::try_from(service_entries).expect("service entry budget fits usize")],
        &service
    ));
    assert!(history_topology_work_budget_exceeded(
        [usize::try_from(service_entries + 1).expect("service entry overflow fits usize")],
        &service
    ));
}
