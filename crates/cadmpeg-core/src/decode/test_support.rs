// SPDX-License-Identifier: Apache-2.0
//! Test-only charge probes. Normal builds contain no probe state or calls.

use std::cell::RefCell;

use super::{ResourceDimension, ResourceLimits};

#[derive(Debug)]
struct Target {
    dimension: ResourceDimension,
    operation: String,
    skip: usize,
    peak: u64,
}

#[derive(Debug)]
struct Recording {
    dimension: ResourceDimension,
    operation: String,
    charged: u64,
}

thread_local! {
    static TARGET: RefCell<Option<Target>> = const { RefCell::new(None) };
    static RECORDING: RefCell<Option<Recording>> = const { RefCell::new(None) };
}

#[derive(Debug)]
struct RestoreTarget(Option<Target>);

impl Drop for RestoreTarget {
    fn drop(&mut self) {
        TARGET.with(|target| *target.borrow_mut() = self.0.take());
    }
}

/// Refuse a named charge at a new budget boundary, one unit below its need.
///
/// `skip` counts matching boundaries admitted before probing. A charge is
/// reachable by a normal ceiling only when its need exceeds all earlier needs
/// in this dimension. Released storage can make later needs smaller.
/// Only budgets whose selected policy ceiling is `u64::MAX` participate.
/// Fixture setup with a separate fixed policy keeps its normal behavior.
///
/// The probe can only lower a policy ceiling. It affects this thread until
/// `run` returns or unwinds; nested probes restore the outer target. Tests
/// must repeat the decode with the discovered policy ceiling and no probe.
pub fn at_charge<T>(
    dimension: ResourceDimension,
    operation: &str,
    skip: usize,
    run: impl FnOnce() -> T,
) -> T {
    with_target(
        Some(Target {
            dimension,
            operation: operation.to_owned(),
            skip,
            peak: 0,
        }),
        run,
    )
}

/// Run `run` and return its result with the amount charged to `dimension`
/// under `operation`.
///
/// Use this to prove that a route charges a named allocation when released
/// storage keeps that charge below an earlier peak, so no ceiling can make it
/// the refusing charge. Nested recordings restore the outer recording.
pub fn charged_at<T>(
    dimension: ResourceDimension,
    operation: &str,
    run: impl FnOnce() -> T,
) -> (T, u64) {
    let previous = RECORDING.with(|recording| {
        recording.replace(Some(Recording {
            dimension,
            operation: operation.to_owned(),
            charged: 0,
        }))
    });
    let guard = RestoreRecording(previous);
    let result = run();
    let charged = RECORDING.with(|recording| {
        recording
            .borrow()
            .as_ref()
            .map_or(0, |recording| recording.charged)
    });
    drop(guard);
    (result, charged)
}

#[derive(Debug)]
struct RestoreRecording(Option<Recording>);

impl Drop for RestoreRecording {
    fn drop(&mut self) {
        RECORDING.with(|recording| *recording.borrow_mut() = self.0.take());
    }
}

pub(super) fn record_charge(dimension: ResourceDimension, operation: &str, amount: u64) {
    RECORDING.with(|recording| {
        if let Some(recording) = recording.borrow_mut().as_mut() {
            if recording.dimension == dimension && recording.operation == operation {
                // A total past u64::MAX keeps its earlier, already positive value.
                if let Some(total) = recording.charged.checked_add(amount) {
                    recording.charged = total;
                }
            }
        }
    });
}

/// Run a normal policy check while suspending any enclosing charge probe.
/// The outer probe is restored when `run` returns or unwinds.
pub fn without_probe<T>(run: impl FnOnce() -> T) -> T {
    with_target(None, run)
}

fn with_target<T>(replacement: Option<Target>, run: impl FnOnce() -> T) -> T {
    let previous = TARGET.with(|target| target.replace(replacement));
    let guard = RestoreTarget(previous);
    let result = run();
    drop(guard);
    result
}

pub(super) fn charge_limit(
    limits: &ResourceLimits,
    dimension: ResourceDimension,
    operation: &str,
    used: u64,
    additional: u64,
) -> Option<u64> {
    if additional == 0 {
        return None;
    }
    TARGET.with(|target| {
        let mut target = target.borrow_mut();
        let target = target.as_mut()?;
        if target.dimension != dimension {
            return None;
        }
        let ceiling = match dimension {
            ResourceDimension::InputBytes => limits.max_input_bytes,
            ResourceDimension::DecompressedBytes => limits.max_decompressed_bytes_total,
            ResourceDimension::RetainedBytes => limits.max_retained_bytes,
            ResourceDimension::Entities => limits.max_entities,
            ResourceDimension::RecursionDepth => limits.max_recursion_depth,
        };
        if ceiling != u64::MAX {
            return None;
        }
        let need = used.checked_add(additional)?;
        if need <= target.peak {
            return None;
        }
        target.peak = need;
        if target.operation != operation {
            return None;
        }
        if target.skip > 0 {
            target.skip -= 1;
            return None;
        }
        need.checked_sub(1)
    })
}

#[cfg(test)]
mod tests {
    use super::{at_charge, charged_at, without_probe};
    use crate::decode::budget::DecodeBudget;
    use crate::decode::{DecodePolicy, ResourceDimension, ResourceLimit};

    fn entity_probe_policy() -> DecodePolicy {
        let mut policy = DecodePolicy::desktop();
        policy.limits.max_entities = u64::MAX;
        policy
    }

    fn run(policy: DecodePolicy) -> Result<(), ResourceLimit> {
        let limit = |error| match error {
            crate::CodecError::ResourceLimit(limit) => limit,
            other => panic!("expected a resource refusal: {other:?}"),
        };
        let budget = DecodeBudget::new(policy, 0);
        budget.charge_entities(2, "before").map_err(limit)?;
        budget.charge_entities(0, "target").map_err(limit)?;
        budget.charge_entities(2, "target").map_err(limit)
    }

    #[test]
    fn probe_matches_normal_policy_refusal() {
        let found = at_charge(ResourceDimension::Entities, "target", 0, || {
            run(entity_probe_policy())
        })
        .expect_err("target is probed");
        assert_eq!(
            (found.operation, found.used, found.additional),
            ("target", 2, 2)
        );
        let mut policy = DecodePolicy::desktop();
        policy.limits.max_entities = found.used + found.additional - 1;
        assert_eq!(run(policy), Err(found));
        assert!(run(entity_probe_policy()).is_ok());
    }

    #[test]
    fn probes_are_suspended_and_restored() {
        at_charge(ResourceDimension::Entities, "target", 0, || {
            assert!(without_probe(|| run(entity_probe_policy())).is_ok());
            assert!(run(entity_probe_policy()).is_err());
        });
        assert!(run(entity_probe_policy()).is_ok());
    }

    #[test]
    fn recording_sums_matching_charges() {
        let (result, charged) = charged_at(ResourceDimension::Entities, "target", || {
            run(entity_probe_policy())
        });
        assert_eq!(result, Ok(()));
        assert_eq!(charged, 2);
    }
}
