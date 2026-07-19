// SPDX-License-Identifier: Apache-2.0
//! Decode modes, resource ceilings, and input-proportional allowances.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

/// Whether a successful decode may report mandatory transfer losses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecodeMode {
    /// Reject a completed decode that reports a mandatory transfer loss.
    Strict,
    /// Return the decoded result with its loss report.
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
}

impl ResourceLimits {
    /// Generous ceilings for interactive desktop use; the default profile.
    pub const fn desktop() -> Self {
        Self {
            max_input_bytes: 4 * GIB,
            max_decompressed_bytes_total: 8 * GIB,
            max_decompressed_bytes_per_expand: 2 * GIB,
        }
    }

    /// Tight ceilings for unattended service use.
    pub const fn service() -> Self {
        Self {
            max_input_bytes: 256 * MIB,
            max_decompressed_bytes_total: GIB,
            max_decompressed_bytes_per_expand: 256 * MIB,
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
    /// How mandatory transfer losses are handled.
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

pub(crate) const DECOMPRESSED_TOTAL_BASE: u64 = 16 * MIB;
pub(crate) const DECOMPRESSED_TOTAL_PER_INPUT_BYTE: u64 = 1000;
pub(crate) const DECOMPRESSED_PER_EXPAND_BASE: u64 = 16 * MIB;
pub(crate) const DECOMPRESSED_PER_EXPAND_PER_INPUT_BYTE: u64 = 256;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_ceilings_are_explicit() {
        let d = ResourceLimits::desktop();
        assert_eq!(d.max_input_bytes, 4 * GIB);
        assert_eq!(d.max_decompressed_bytes_total, 8 * GIB);
        assert_eq!(d.max_decompressed_bytes_per_expand, 2 * GIB);
    }

    #[test]
    fn service_ceilings_are_explicit() {
        let s = ResourceLimits::service();
        assert_eq!(s.max_input_bytes, 256 * MIB);
        assert_eq!(s.max_decompressed_bytes_total, GIB);
        assert_eq!(s.max_decompressed_bytes_per_expand, 256 * MIB);
    }
}
