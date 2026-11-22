// SPDX-License-Identifier: Apache-2.0
//! Admission of scan work through host-controlled cancellation.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use super::{ScanLimits, StopReason};
use crate::decode::{u64_from_index, SpaceId, View};

/// Host time and cancellation source; implementations can run on wasm.
pub trait ScanClock {
    /// Elapsed time since the host started this scan.
    fn elapsed(&self) -> Duration;
    /// Whether the host requested cancellation.
    fn is_cancelled(&self) -> bool;
}

/// Manually controlled clock for hosts and deterministic tests.
#[derive(Debug, Default)]
pub struct ManualClock {
    elapsed: Cell<Duration>,
    cancelled: Cell<bool>,
}

impl ManualClock {
    /// Sets the host's elapsed duration.
    pub fn set_elapsed(&self, elapsed: Duration) {
        self.elapsed.set(elapsed);
    }
    /// Requests cancellation.
    pub fn cancel(&self) {
        self.cancelled.set(true);
    }
}

impl ScanClock for ManualClock {
    fn elapsed(&self) -> Duration {
        self.elapsed.get()
    }
    fn is_cancelled(&self) -> bool {
        self.cancelled.get()
    }
}

/// Native host clock backed by a monotonic timer.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct SystemClock(std::time::Instant);

#[cfg(not(target_arch = "wasm32"))]
impl Default for SystemClock {
    fn default() -> Self {
        Self(std::time::Instant::now())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ScanClock for SystemClock {
    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Cumulative admission meter shared by every reader in one scan.
pub struct ScanBudget<'a> {
    limits: ScanLimits,
    clock: &'a dyn ScanClock,
    source: Cell<u64>,
    expanded: Cell<u64>,
    retained: Cell<u64>,
    features: Cell<u64>,
    next_space: Cell<usize>,
    refusal: RefCell<Option<StopReason>>,
}

/// Admission for one feature-list visit.
pub struct FeatureVisit<'a> {
    pub(super) budget: &'a ScanBudget<'a>,
}

impl<'a> ScanBudget<'a> {
    /// Starts an empty meter.
    pub fn new(limits: ScanLimits, clock: &'a dyn ScanClock) -> Self {
        Self {
            limits,
            clock,
            source: Cell::new(0),
            expanded: Cell::new(0),
            retained: Cell::new(0),
            features: Cell::new(0),
            next_space: Cell::new(1),
            refusal: RefCell::new(None),
        }
    }

    /// Checks cancellation and elapsed time before a reader step or seek.
    pub fn check(&self) -> Result<(), StopReason> {
        if self.clock.is_cancelled() {
            return Err(self.refuse(StopReason::Cancelled));
        }
        if self.clock.elapsed() >= self.limits.elapsed_time {
            return Err(self.refuse(StopReason::ElapsedTime));
        }
        Ok(())
    }

    pub(crate) fn refuse(&self, reason: StopReason) -> StopReason {
        let mut refusal = self.refusal.borrow_mut();
        if refusal.is_none() {
            *refusal = Some(reason.clone());
        }
        reason
    }

    /// First resource refusal, retained through dependency error wrapping.
    pub fn stop_reason(&self) -> Option<StopReason> {
        self.refusal.borrow().clone()
    }

    /// Registers a retained member in a distinct session address space.
    pub fn view<'b>(&self, bytes: &'b [u8]) -> Result<View<'b>, StopReason> {
        self.check()?;
        let index = self.next_space.get();
        self.next_space
            .set(index.checked_add(1).ok_or(StopReason::RetainedBytes)?);
        Ok(View::over_space(bytes, SpaceId::from_index(index)))
    }

    fn observe_space(&self, view: View<'_>) -> Result<(), StopReason> {
        let next = view
            .location()
            .space
            .index()
            .checked_add(1)
            .ok_or(StopReason::RetainedBytes)?;
        self.next_space.set(self.next_space.get().max(next));
        Ok(())
    }

    fn charge(
        &self,
        used: &Cell<u64>,
        amount: u64,
        limit: u64,
        reason: StopReason,
    ) -> Result<(), StopReason> {
        self.check()?;
        let next = used
            .get()
            .checked_add(amount)
            .filter(|next| *next <= limit)
            .ok_or_else(|| self.refuse(reason))?;
        used.set(next);
        Ok(())
    }

    /// Charges a physical read before accessing the source.
    pub fn source(&self, bytes: u64) -> Result<(), StopReason> {
        self.charge(
            &self.source,
            bytes,
            self.limits.source_bytes,
            StopReason::SourceBytes,
        )
    }
    /// Charges parser storage or result storage before allocation.
    pub fn retain(&self, bytes: u64) -> Result<(), StopReason> {
        self.charge(
            &self.retained,
            bytes,
            self.limits.retained_bytes,
            StopReason::RetainedBytes,
        )
    }
    /// Returns a charge for storage that was dropped.
    pub fn release_retained(&self, bytes: u64) {
        self.retained
            .set(self.retained.get().checked_sub(bytes).unwrap_or_default());
    }
    /// Charges one feature visit before reading its listing record.
    pub fn feature(&self) -> Result<FeatureVisit<'_>, StopReason> {
        self.charge(
            &self.features,
            1,
            self.limits.feature_count,
            StopReason::FeatureCount,
        )?;
        Ok(FeatureVisit { budget: self })
    }
    /// Checks a descent before following a nested document or reference.
    pub fn depth(&self, depth: u64) -> Result<(), StopReason> {
        self.check()?;
        if depth > self.limits.nesting_depth {
            return Err(self.refuse(StopReason::NestingDepth));
        }
        Ok(())
    }
    /// Physical bytes admitted so far.
    pub fn bytes_read(&self) -> u64 {
        self.source.get()
    }
    /// Remaining expansion capacity.
    pub(crate) fn expansion_available(&self) -> u64 {
        self.limits.expanded_bytes - self.expanded.get()
    }
    /// Reserves output capacity before an inflate step.
    pub(crate) fn expand(&self, bytes: u64) -> Result<(), StopReason> {
        self.charge(
            &self.expanded,
            bytes,
            self.limits.expanded_bytes,
            StopReason::ExpandedBytes,
        )
    }
    /// Returns unused capacity from a completed bounded step.
    pub(crate) fn release_expansion(&self, bytes: u64) {
        self.expanded.set(self.expanded.get() - bytes);
    }
}

/// A metered cursor that preserves its source address space.
pub struct ScanReader<'a, 'b> {
    view: View<'a>,
    budget: &'b ScanBudget<'b>,
    physical: bool,
}

impl<'a, 'b> ScanReader<'a, 'b> {
    /// Opens physical source bytes.
    pub fn source(view: View<'a>, budget: &'b ScanBudget<'b>) -> Self {
        Self {
            view,
            budget,
            physical: true,
        }
    }
    /// Opens an already charged expansion or retained member.
    pub fn retained(view: View<'a>, budget: &'b ScanBudget<'b>) -> Self {
        Self {
            view,
            budget,
            physical: false,
        }
    }
    /// Cursor position in its address space.
    pub fn position(&self) -> usize {
        self.view.position()
    }
    /// Bytes remaining in this reader's window.
    pub fn remaining(&self) -> usize {
        self.view.remaining()
    }
    /// Seeks after checking cancellation.
    pub fn seek(&mut self, at: usize) -> Result<(), StopReason> {
        self.budget.check()?;
        self.view
            .seek(at)
            .ok_or_else(|| StopReason::Truncated(format!("seek to {at}")))
    }
    /// Reads a child window after admitting the work.
    pub fn take(&mut self, bytes: usize) -> Result<View<'a>, StopReason> {
        self.budget.observe_space(self.view)?;
        if self.physical {
            self.budget
                .source(u64_from_index(bytes.min(self.view.remaining())))?;
        } else {
            self.budget.check()?;
        }
        Ok(self.view.req_take_child(bytes)?)
    }
    /// Reads a little-endian 32-bit field.
    pub fn u32(&mut self) -> Result<u32, StopReason> {
        Ok(self.take(4)?.req_u32_le()?)
    }
    /// Reads a little-endian 16-bit field.
    pub fn u16(&mut self) -> Result<u16, StopReason> {
        Ok(self.take(2)?.req_u16_le()?)
    }
    /// Reads a little-endian 64-bit field.
    pub fn u64(&mut self) -> Result<u64, StopReason> {
        Ok(self.take(8)?.req_u64_le()?)
    }
    /// Reads one byte.
    pub fn u8(&mut self) -> Result<u8, StopReason> {
        Ok(self.take(1)?.req_u8()?)
    }
}

#[cfg(test)]
mod tests;
