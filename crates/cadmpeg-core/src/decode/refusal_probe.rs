// SPDX-License-Identifier: Apache-2.0
//! Test support: refuse the first charge of a named operation.
//!
//! A refusal test needs the budget state where one operation first needs more
//! than the dimension has ever held: the boundary a limit raised one refusal
//! at a time reaches. An armed probe tracks the peak usage of its dimension and
//! refuses the first charge of the operation that would raise that peak, as if
//! the limit were the peak. For a cumulative dimension that is its first
//! positive charge. A test then decodes once to find the boundary. The probe is
//! per thread and refuses at most once.

use std::cell::RefCell;
use std::marker::PhantomData;

use super::ResourceDimension;

struct Target {
    dimension: ResourceDimension,
    operation: String,
    additional: Option<u64>,
    peak: u64,
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
    /// of `dimension` on this thread. With `additional`, only a charge of
    /// exactly that amount matches. The refusal reports the peak as its limit.
    pub fn arm(dimension: ResourceDimension, operation: &str, additional: Option<u64>) -> Self {
        TARGET.with(|target| {
            *target.borrow_mut() = Some(Target {
                dimension,
                operation: operation.to_owned(),
                additional,
                peak: 0,
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

/// Returns the peak as the refusal limit when this charge matches the armed
/// target, and disarms it. Any other charge in the dimension updates the peak.
pub(super) fn refusal_limit(
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
        let need = before.checked_add(amount);
        if need.is_none_or(|need| need > armed.peak)
            && armed.operation == operation
            && armed.additional.is_none_or(|expected| expected == amount)
        {
            let limit = armed.peak;
            *target = None;
            return Some(limit);
        }
        if let Some(need) = need {
            armed.peak = armed.peak.max(need);
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

    #[test]
    fn a_cumulative_dimension_refuses_the_first_matching_charge_once() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "target", Some(3));
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
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
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
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        drop(RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "target",
            None,
        ));
        ctx.charge_work(1, "target").expect("no probe is armed");
    }
}
