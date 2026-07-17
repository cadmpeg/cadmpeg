// SPDX-License-Identifier: Apache-2.0
//! Decode policy: caller-owned ceilings and the platform acceptance envelope.
//!
//! [`ResourceLimits`] are absolute ceilings the caller owns. The envelope is
//! a platform-owned default describing what amplification the platform
//! accepts as plausible; it is not a proven bound on well-formed files. The
//! effective allowance for a counter dimension is the smaller of the absolute
//! ceiling and the input-proportional envelope term, so a tight profile
//! always wins.
//!
//! Budgets are active in both [`DecodeMode`] variants; the mode governs
//! failure handling, not resource limits.

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

/// Caller-owned absolute ceilings.
///
/// Physical input is a first-class limit: it is enforced before any
/// input-proportional allowance can exist, because every other allowance is
/// derived from the input basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResourceLimits {
    /// Maximum physical input bytes read at the root. Frozen at Phase 0B — the
    /// one dimension whose charges are already real (§5.2).
    pub max_input_bytes: u64,
    /// Maximum cumulative decompressed bytes across all expansions.
    /// **Provisional** until the Phase 1 decompression calibration (§5.2).
    pub max_decompressed_bytes_total: u64,
    /// Maximum decompressed bytes produced by any single expansion.
    /// **Provisional** until the Phase 1 decompression calibration (§5.2).
    pub max_decompressed_bytes_per_expand: u64,
    /// Maximum cumulative committed heap bytes.
    /// **Frozen** at Phase 2 (§5.2 alloc/work/depth freeze): the Phase 2
    /// per-codec calibration measured the migrated charge sites well inside
    /// this ceiling, so the value is unchanged and now load-bearing.
    pub max_alloc_bytes: u64,
    /// Maximum abstract work units.
    /// **Frozen** at Phase 2 (§5.2 alloc/work/depth freeze); value unchanged by
    /// calibration.
    pub max_work: u64,
    /// Maximum recursion depth.
    /// **Frozen** at Phase 2 (§5.2 alloc/work/depth freeze); value unchanged by
    /// calibration.
    pub max_depth: u32,
    /// Maximum bytes retained opaque in salvage mode.
    /// **Provisional** until the Phase 3 retained calibration (§5.2).
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

    /// Version tag for the desktop profile's ceilings (§5.2). The §5.2
    /// per-dimension freeze schedule is complete: `max_input_bytes` (Phase 0B),
    /// `max_decompressed_bytes_*` (Phase 1), `max_alloc_bytes`/`max_work`/
    /// `max_depth` (Phase 2), and `max_retained_bytes` (Phase 3) are all frozen.
    /// The Phase 3 retained freeze, like the Phase 2 freeze before it, left every
    /// ceiling value unchanged — the migrated charge sites calibrated well inside
    /// them — so the tag stays `desktop-v1`; it advances only when a ceiling
    /// *value* changes. The `desktop_version_pins_its_ceilings` test pins the tag
    /// to its values so a ceiling cannot change without one.
    pub const DESKTOP_VERSION: &'static str = "desktop-v1";

    /// Version tag for the service profile's ceilings (§5.2), advanced whenever
    /// a service ceiling changes. Provisional under the same per-dimension
    /// freeze schedule as `DESKTOP_VERSION`, and pinned to its values by
    /// `service_version_pins_its_ceilings`.
    pub const SERVICE_VERSION: &'static str = "service-v1";

    /// The version tag of the named profile these ceilings match exactly, or
    /// `None` when the caller supplied ceilings that match no named profile.
    ///
    /// Structural equality is the identity test: a caller who reproduces a
    /// profile's exact ceilings is treated as using that profile; any deviation
    /// makes the profile `custom` and is recorded against the default baseline.
    pub fn profile_version(&self) -> Option<&'static str> {
        if *self == Self::desktop() {
            Some(Self::DESKTOP_VERSION)
        } else if *self == Self::service() {
            Some(Self::SERVICE_VERSION)
        } else {
            None
        }
    }
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self::desktop()
    }
}

/// Mode plus ceilings: the whole decode policy the caller supplies.
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

/// Options for container inspection.
///
/// `inspect` parses the same hostile containers as `decode`. It gains its own
/// limits so an inspection cannot be turned into an amplification vector.
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

/// The platform acceptance envelope: an input-independent `base` floor plus a
/// `k` multiplier applied to the input basis.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Envelope {
    pub(crate) base: DimensionAmounts,
    pub(crate) k: DimensionAmounts,
}

impl Envelope {
    /// The platform default envelope. The `alloc_bytes` and `work` terms are
    /// **frozen** at Phase 2 (§5.2): the per-codec calibration notes measured
    /// the migrated charge sites (container framing plus graduated leaves) at
    /// bytes-to-low-KiB cumulative `alloc_bytes` and tens-to-low-thousands
    /// `work` units per fixture — orders of magnitude inside these constants —
    /// so the freeze left every value unchanged. `decompressed_*` froze at
    /// Phase 1; `retained_bytes` froze at Phase 3, where the retained-byte charge
    /// sites became real — the multi-space fidelity ledger charges blob retention
    /// against this dimension, and the measured per-fixture retention sits far
    /// inside `base`, so the freeze again left every value unchanged. The whole
    /// §5.2 schedule is now frozen. A false reject on a legitimate file remains a
    /// calibration bug, not a contract.
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

    /// Version tag for the platform default envelope's calibration constants
    /// (§5.2). No caller API overrides the envelope yet, so every decode runs
    /// this version. `envelope-v3` records the Phase 3 freeze of the envelope's
    /// `retained_bytes` term: it moved from a provisional starting point to a
    /// frozen, calibration-defended value (its magnitude was unchanged — the
    /// measured retention charges sit far inside it — but its status did),
    /// completing the §5.2 schedule. `envelope-v2` before it recorded the Phase 2
    /// freeze of `alloc_bytes` and `work` on the same terms. The `max_depth`
    /// ceiling is not an envelope term; its Phase 2
    /// freeze is recorded by `DESKTOP_VERSION`/`SERVICE_VERSION` and their
    /// pinning tests, not here.
    /// The tag advances when a `PLATFORM_DEFAULT` constant changes or when a
    /// dimension's freeze status advances. This differs deliberately from the
    /// profile ceiling tags (`DESKTOP_VERSION`/`SERVICE_VERSION`), which advance
    /// on a ceiling *value* change only: the envelope carries no other durable
    /// record of its own freeze status, whereas each ceiling freeze is pinned by
    /// a `*_version_pins_its_ceilings` test that asserts the frozen value, so the
    /// profile tag is left free to signal value drift. The
    /// `envelope_version_pins_its_constants` test pins the tag to its values so
    /// a constant cannot change without advancing the tag.
    pub(crate) const VERSION: &'static str = "envelope-v3";
}

#[cfg(test)]
mod tests {
    use super::*;

    // §5.2 durable record: each version tag certifies an exact set of
    // calibration constants. `profile_version` decides the tag by structural
    // equality against the live `desktop()`/`service()` values, so it cannot
    // notice a ceiling that changed without its tag — the comparison and the
    // constant move together and every report keeps claiming the old tag.
    // These tests pin each tag to the values it names: changing any constant
    // below without bumping the paired version string fails here. When a
    // recalibration is intended, bump the version tag AND update the pinned
    // value in the same commit.

    #[test]
    fn desktop_version_pins_its_ceilings() {
        assert_eq!(ResourceLimits::DESKTOP_VERSION, "desktop-v1");
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
    fn service_version_pins_its_ceilings() {
        assert_eq!(ResourceLimits::SERVICE_VERSION, "service-v1");
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
    fn envelope_version_pins_its_constants() {
        assert_eq!(Envelope::VERSION, "envelope-v3");
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
