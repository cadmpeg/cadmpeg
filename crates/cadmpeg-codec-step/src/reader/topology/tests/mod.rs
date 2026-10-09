// SPDX-License-Identifier: Apache-2.0
mod admission_limits;
mod components;
mod draft;
pub(crate) mod faces;
pub(crate) mod sheets;
pub(crate) mod shells;
mod wires;

mod numerical_range;
mod representation_bodies;

#[test]
fn topology_failure_count_refuses_overflow() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut outcome = super::BuildOutcome {
        storage: ctx
            .reserve_scoped(0, "test outcome")
            .expect("empty outcome"),
        built: Vec::new(),
        failures: Some(super::BuildFailures {
            count: std::num::NonZeroUsize::new(usize::MAX).expect("nonzero maximum"),
            first: None,
        }),
    };
    assert!(format!("{:?}", outcome.fail(None)).contains("ResourceLimit"));
}

mod set_lookups;

mod equality;

mod budget_regressions;
mod early_exits;
mod face_ancestry;

mod known_length;
