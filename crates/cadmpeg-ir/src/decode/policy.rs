// SPDX-License-Identifier: Apache-2.0
//! Decode modes, resource ceilings, and input-proportional allowances.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

/// How a committed failure is handled once decoding has begun.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecodeMode {
    /// Any committed violation aborts the decode with the classified error.
    Strict,
    /// A committed failure may be skipped or retained opaque, but every such
    /// event must produce an accountable outcome.
    #[default]
    Salvage,
}

/// Absolute resource ceilings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResourceLimits {
    /// Maximum physical input bytes read at the root.
    pub max_input_bytes: u64,
    /// Maximum cumulative decompressed bytes across all expansions.
    pub max_decompressed_bytes_total: u64,
    /// Maximum decompressed bytes produced by any single expansion.
    pub max_decompressed_bytes_per_expand: u64,
    /// Maximum cumulative committed heap bytes.
    pub max_alloc_bytes: u64,
    /// Maximum abstract work units.
    pub max_work: u64,
    /// Maximum recursion depth.
    pub max_depth: u32,
    /// Maximum bytes retained opaque in salvage mode.
    pub max_retained_bytes: u64,
}

impl ResourceLimits {
    /// Generous ceilings for interactive desktop use; the default profile.
    pub const fn desktop() -> Self {
        Self {
            max_input_bytes: 4 * GIB,
            max_decompressed_bytes_total: 8 * GIB,
            max_decompressed_bytes_per_expand: 2 * GIB,
            max_alloc_bytes: 8 * GIB,
            max_work: 4_000_000_000,
            // Above the recorded per-codec local depth limits (32-64): the
            // desktop gauge is an outer backstop behind them, not their
            // replacement. `service()` tightens to 64.
            max_depth: 256,
            max_retained_bytes: 4 * GIB,
        }
    }

    /// Tight ceilings for unattended service use.
    pub const fn service() -> Self {
        Self {
            max_input_bytes: 256 * MIB,
            max_decompressed_bytes_total: GIB,
            max_decompressed_bytes_per_expand: 256 * MIB,
            max_alloc_bytes: GIB,
            max_work: 200_000_000,
            max_depth: 64,
            max_retained_bytes: 256 * MIB,
        }
    }
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self::desktop()
    }
}

/// Decode mode and resource ceilings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DecodePolicy {
    /// How committed failures are handled.
    pub mode: DecodeMode,
    /// The absolute ceilings.
    pub limits: ResourceLimits,
}

impl DecodePolicy {
    /// The default desktop profile in salvage mode.
    pub const fn desktop() -> Self {
        Self {
            mode: DecodeMode::Salvage,
            limits: ResourceLimits::desktop(),
        }
    }

    /// The tight service profile in salvage mode.
    pub const fn service() -> Self {
        Self {
            mode: DecodeMode::Salvage,
            limits: ResourceLimits::service(),
        }
    }
}

impl Default for DecodePolicy {
    fn default() -> Self {
        Self::desktop()
    }
}

/// Resource options for container inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct InspectOptions {
    /// The absolute ceilings applied during inspection.
    pub limits: ResourceLimits,
}

/// Per-dimension amounts used by the acceptance envelope.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DimensionAmounts {
    pub(crate) alloc_bytes: u64,
    pub(crate) decompressed_total: u64,
    pub(crate) decompressed_per_expand: u64,
    pub(crate) work: u64,
    pub(crate) retained_bytes: u64,
}

/// Input-independent floors and input-size multipliers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Envelope {
    pub(crate) base: DimensionAmounts,
    pub(crate) k: DimensionAmounts,
}

impl Envelope {
    /// The platform default envelope.
    pub(crate) const PLATFORM_DEFAULT: Envelope = Envelope {
        base: DimensionAmounts {
            alloc_bytes: 64 * MIB,
            decompressed_total: 16 * MIB,
            decompressed_per_expand: 16 * MIB,
            work: 4_000_000,
            retained_bytes: 64 * MIB,
        },
        k: DimensionAmounts {
            alloc_bytes: 64,
            // The recorded ratio-1000 precedent is a cumulative threshold;
            // it applies to `decompressed_total`. Per-expand is deliberately
            // tighter: one expansion claiming the whole cumulative envelope
            // is the amplification shape the per-expand term exists to
            // refuse.
            decompressed_total: 1000,
            decompressed_per_expand: 256,
            work: 256,
            retained_bytes: 16,
        },
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_ceilings_are_explicit() {
        let d = ResourceLimits::desktop();
        assert_eq!(d.max_input_bytes, 4 * GIB);
        assert_eq!(d.max_decompressed_bytes_total, 8 * GIB);
        assert_eq!(d.max_decompressed_bytes_per_expand, 2 * GIB);
        assert_eq!(d.max_alloc_bytes, 8 * GIB);
        assert_eq!(d.max_work, 4_000_000_000);
        assert_eq!(d.max_depth, 256);
        assert_eq!(d.max_retained_bytes, 4 * GIB);
    }

    #[test]
    fn service_ceilings_are_explicit() {
        let s = ResourceLimits::service();
        assert_eq!(s.max_input_bytes, 256 * MIB);
        assert_eq!(s.max_decompressed_bytes_total, GIB);
        assert_eq!(s.max_decompressed_bytes_per_expand, 256 * MIB);
        assert_eq!(s.max_alloc_bytes, GIB);
        assert_eq!(s.max_work, 200_000_000);
        assert_eq!(s.max_depth, 64);
        assert_eq!(s.max_retained_bytes, 256 * MIB);
    }

    #[test]
    fn envelope_constants_are_explicit() {
        let e = Envelope::PLATFORM_DEFAULT;
        assert_eq!(e.base.alloc_bytes, 64 * MIB);
        assert_eq!(e.base.decompressed_total, 16 * MIB);
        assert_eq!(e.base.decompressed_per_expand, 16 * MIB);
        assert_eq!(e.base.work, 4_000_000);
        assert_eq!(e.base.retained_bytes, 64 * MIB);
        assert_eq!(e.k.alloc_bytes, 64);
        assert_eq!(e.k.decompressed_total, 1000);
        assert_eq!(e.k.decompressed_per_expand, 256);
        assert_eq!(e.k.work, 256);
        assert_eq!(e.k.retained_bytes, 16);
    }
}
