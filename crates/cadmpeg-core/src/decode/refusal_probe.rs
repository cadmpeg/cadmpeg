// SPDX-License-Identifier: Apache-2.0
//! Test support: refuse the first charge of a named operation.
//!
//! A refusal test needs the budget state where one operation first needs more
//! than the dimension has held: the boundary that raising a limit one refusal
//! at a time reaches. A test marks the budget under search by leaving the
//! dimension unlimited in its policy (`u64::MAX`). An armed probe tracks that
//! budget's peak usage of the dimension and refuses the first charge of the
//! operation that would raise the peak, as if the limit were the peak. For a
//! cumulative dimension that is the operation's first positive charge. Other
//! budgets on the thread are not affected. The test then decodes once to find
//! the boundary. The probe is per thread and refuses at most once.

use std::cell::RefCell;
use std::marker::PhantomData;

use super::ResourceDimension;

struct Target {
    dimension: ResourceDimension,
    operation: String,
    additional: Option<u64>,
    /// Peak usage of the dimension per decode budget, keyed by its address.
    peaks: Vec<(usize, u64)>,
}

thread_local! {
    static TARGET: RefCell<Option<Target>> = const { RefCell::new(None) };
}

/// Disarms the probe when dropped.
#[derive(Debug)]
#[must_use = "the probe is disarmed when the guard is dropped"]
pub struct RefusalProbe {
    _thread_bound: PhantomData<*const ()>,
}

impl RefusalProbe {
    /// Refuses the first charge of `operation` that would raise the peak usage
    /// of `dimension` in a budget whose policy leaves the dimension unlimited.
    /// With `additional`, only a charge of exactly that amount matches. The
    /// refusal reports the peak as its limit.
    pub fn arm(dimension: ResourceDimension, operation: &str, additional: Option<u64>) -> Self {
        TARGET.with(|target| {
            *target.borrow_mut() = Some(Target {
                dimension,
                operation: operation.to_owned(),
                additional,
                peaks: Vec::new(),
            });
        });
        Self {
            _thread_bound: PhantomData,
        }
    }
}

impl Drop for RefusalProbe {
    fn drop(&mut self) {
        TARGET.with(|target| *target.borrow_mut() = None);
    }
}

/// Returns the budget's peak as the refusal limit when this charge matches
/// the armed target, and disarms it. Any other charge in the dimension updates
/// the peak of the budget it charges.
pub(super) fn refusal_limit(
    budget: usize,
    dimension: ResourceDimension,
    before: u64,
    amount: u64,
    operation: &str,
) -> Option<u64> {
    TARGET.with(|target| {
        let mut target = target.borrow_mut();
        let armed = target
            .as_mut()
            .filter(|armed| armed.dimension == dimension)?;
        let index = match armed.peaks.iter().position(|(key, _)| *key == budget) {
            Some(index) => index,
            None => {
                armed.peaks.push((budget, 0));
                armed.peaks.len() - 1
            }
        };
        let peak = armed.peaks[index].1;
        let need = before.checked_add(amount);
        if need.is_none_or(|need| need > peak)
            && armed.operation == operation
            && armed.additional.is_none_or(|expected| expected == amount)
        {
            *target = None;
            return Some(peak);
        }
        if let Some(need) = need {
            armed.peaks[index].1 = peak.max(need);
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::RefusalProbe;
    use crate::decode::{
        DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceFailure,
    };
    use crate::CodecError;

    fn unlimited(dimension: ResourceDimension) -> DecodePolicy {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = u64::MAX,
            ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = u64::MAX;
            }
            _ => panic!("dimension outside these tests"),
        }
        policy
    }

    #[test]
    fn a_cumulative_dimension_refuses_the_first_matching_charge_once() {
        let arena = DecodeArena::new();
        let policy = unlimited(ResourceDimension::WorkUnits);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let bounded_arena = DecodeArena::new();
        let (bounded, _) =
            DecodeContext::from_root_bytes(&[], &bounded_arena, &DecodePolicy::service())
                .expect("context");
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "target", Some(3));
        bounded
            .charge_work(3, "target")
            .expect("a budget with a policy limit is not searched");
        ctx.charge_work(5, "before").expect("other operations pass");
        ctx.charge_work(0, "target")
            .expect("an empty charge does not match");
        ctx.charge_work(2, "target")
            .expect("another amount does not match");
        let CodecError::ResourceLimit(limit) = ctx
            .charge_work(3, "target")
            .expect_err("the matching charge refuses")
        else {
            panic!("resource refusal");
        };
        drop(probe);
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.reason, ResourceFailure::BudgetExceeded);
        assert_eq!(limit.limit, 7);
        assert_eq!(limit.used, 7);
        assert_eq!(limit.additional, 3);
        assert_eq!(limit.operation, "target");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    }

    #[test]
    fn a_peak_dimension_refuses_the_first_matching_charge_above_the_peak() {
        let arena = DecodeArena::new();
        let policy = unlimited(ResourceDimension::MaterializedBytes);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let probe = RefusalProbe::arm(ResourceDimension::MaterializedBytes, "target", None);
        drop(
            ctx.reserve_scoped(10, "earlier peak")
                .expect("other operations pass"),
        );
        let held = ctx.reserve_scoped(4, "target").expect("below the peak");
        let CodecError::ResourceLimit(limit) = ctx
            .reserve_scoped(7, "target")
            .expect_err("the charge above the peak refuses")
        else {
            panic!("resource refusal");
        };
        drop((held, probe));
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(limit.limit, 10);
        assert_eq!(limit.used, 4);
        assert_eq!(limit.additional, 7);
    }

    #[test]
    fn a_dropped_probe_refuses_nothing() {
        let arena = DecodeArena::new();
        let policy = unlimited(ResourceDimension::WorkUnits);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        drop(RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "target",
            None,
        ));
        ctx.charge_work(1, "target").expect("no probe is armed");
    }
}
