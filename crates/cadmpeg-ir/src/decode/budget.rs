// SPDX-License-Identifier: Apache-2.0
//! Interior-mutable budget counters, the depth gauge, and the input basis.
//!
//! Counters never decrease: abandoning a speculative parse keeps its charges,
//! because the work was done and a refund would let a prober relitigate the
//! budget. The depth gauge releases on guard drop, which is its point. The
//! input basis grows as expansions finalize, so a stream that legitimately
//! inflates earns allowance proportional to what it now contains.

use std::cell::Cell;

use super::error::ResourceDimension;
use super::policy::{Envelope, ResourceLimits};

/// The `Cell`-backed counters and gauge for one decode.
#[derive(Debug, Default)]
pub(crate) struct BudgetCells {
    input_bytes: Cell<u64>,
    decompressed_bytes: Cell<u64>,
    alloc_bytes: Cell<u64>,
    work: Cell<u64>,
    retained_bytes: Cell<u64>,
    depth: Cell<u32>,
    input_basis: Cell<u64>,
}

impl BudgetCells {
    /// Reads a dimension's current value. For [`ResourceDimension::Depth`]
    /// this is the gauge's current level, so failure reports can carry it
    /// through the same path as the counters.
    pub(crate) fn counter(&self, dim: ResourceDimension) -> u64 {
        match dim {
            ResourceDimension::InputBytes => self.input_bytes.get(),
            ResourceDimension::DecompressedBytes => self.decompressed_bytes.get(),
            ResourceDimension::AllocBytes => self.alloc_bytes.get(),
            ResourceDimension::Work => self.work.get(),
            ResourceDimension::RetainedBytes => self.retained_bytes.get(),
            ResourceDimension::Depth => self.depth.get().into(),
        }
    }

    /// Writes a counter dimension. [`ResourceDimension::Depth`] is a gauge
    /// written only through [`BudgetCells::set_depth`]; no charge path passes
    /// it here, and the arm asserts that in debug builds.
    pub(crate) fn set_counter(&self, dim: ResourceDimension, value: u64) {
        match dim {
            ResourceDimension::InputBytes => self.input_bytes.set(value),
            ResourceDimension::DecompressedBytes => self.decompressed_bytes.set(value),
            ResourceDimension::AllocBytes => self.alloc_bytes.set(value),
            ResourceDimension::Work => self.work.set(value),
            ResourceDimension::RetainedBytes => self.retained_bytes.set(value),
            ResourceDimension::Depth => {
                debug_assert!(false, "depth is a gauge; use set_depth");
            }
        }
    }

    /// Returns the current recursion depth.
    pub(crate) fn depth(&self) -> u32 {
        self.depth.get()
    }

    /// Sets the current recursion depth.
    pub(crate) fn set_depth(&self, value: u32) {
        self.depth.set(value);
    }

    /// Returns the input basis: root bytes plus finalized expansion lengths.
    #[cfg(test)]
    pub(crate) fn input_basis(&self) -> u64 {
        self.input_basis.get()
    }

    /// Sets the input basis to a value; used once when the root is read.
    pub(crate) fn set_input_basis(&self, value: u64) {
        self.input_basis.set(value);
    }

    /// Grows the input basis by a finalized expansion length.
    pub(crate) fn grow_input_basis(&self, amount: u64) {
        self.input_basis
            .set(self.input_basis.get().saturating_add(amount));
    }

    /// Returns the effective allowance for a counter dimension: the smaller of
    /// the absolute ceiling and the input-proportional envelope term.
    pub(crate) fn allowance(
        &self,
        dim: ResourceDimension,
        limits: &ResourceLimits,
        envelope: &Envelope,
    ) -> u64 {
        let basis = self.input_basis.get();
        let envelope_term = |base: u64, k: u64| base.saturating_add(k.saturating_mul(basis));
        match dim {
            ResourceDimension::InputBytes => limits.max_input_bytes,
            ResourceDimension::AllocBytes => limits.max_alloc_bytes.min(envelope_term(
                envelope.base.alloc_bytes,
                envelope.k.alloc_bytes,
            )),
            ResourceDimension::DecompressedBytes => {
                limits.max_decompressed_bytes_total.min(envelope_term(
                    envelope.base.decompressed_total,
                    envelope.k.decompressed_total,
                ))
            }
            ResourceDimension::Work => limits
                .max_work
                .min(envelope_term(envelope.base.work, envelope.k.work)),
            ResourceDimension::RetainedBytes => limits.max_retained_bytes.min(envelope_term(
                envelope.base.retained_bytes,
                envelope.k.retained_bytes,
            )),
            ResourceDimension::Depth => u64::from(limits.max_depth),
        }
    }

    /// Returns the per-expand decompressed allowance: the smaller of the
    /// per-expand ceiling and its envelope term.
    pub(crate) fn per_expand_allowance(&self, limits: &ResourceLimits, envelope: &Envelope) -> u64 {
        let basis = self.input_basis.get();
        let term = envelope
            .base
            .decompressed_per_expand
            .saturating_add(envelope.k.decompressed_per_expand.saturating_mul(basis));
        limits.max_decompressed_bytes_per_expand.min(term)
    }
}
