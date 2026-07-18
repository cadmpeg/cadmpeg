// SPDX-License-Identifier: Apache-2.0
//! Stage-1 oracles, verdicts, and resource envelopes.
//!
//! An oracle is a single falsifiable property of one run. Baselines key on
//! each oracle separately so one oracle cannot regress while another improves
//! behind an aggregate count.

use std::time::Duration;

/// A gibibyte.
const GIB: u64 = 1024 * 1024 * 1024;

/// Default peak-allocation envelope: the largest process-wide live heap a run
/// may reach.
///
/// This process-safety ceiling is separate from the decode budget's per-input
/// `K` term: the process
/// legitimately pays for IR, serde, and report memory the budget never meters.
/// It is not derivable from `alloc_bytes`.
pub const DEFAULT_PEAK_ENVELOPE_BYTES: u64 = GIB;

/// Default wall-clock ceiling per run. The hard timeout that kills a child that
/// cannot be stopped from a test thread.
pub const DEFAULT_WALL_CLOCK_MS: u64 = 10_000;

/// Environment override for [`DEFAULT_PEAK_ENVELOPE_BYTES`].
pub const ENV_PEAK_BYTES: &str = "CADMPEG_HARNESS_PEAK_BYTES";

/// Environment override for [`DEFAULT_WALL_CLOCK_MS`].
pub const ENV_TIMEOUT_MS: &str = "CADMPEG_HARNESS_TIMEOUT_MS";

/// The four stage-1 oracles, in baseline-key order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Oracle {
    /// The child neither panicked nor aborted.
    NoPanic,
    /// Peak process allocation stayed within the envelope.
    PeakAlloc,
    /// The run finished within the wall-clock ceiling.
    WallClock,
    /// The two runs produced identical IR JSON, report, and losses.
    Determinism,
}

impl Oracle {
    /// Every oracle, in baseline-key order.
    pub const ALL: [Oracle; 4] = [
        Oracle::NoPanic,
        Oracle::PeakAlloc,
        Oracle::WallClock,
        Oracle::Determinism,
    ];

    /// The stable baseline label.
    pub fn label(self) -> &'static str {
        match self {
            Oracle::NoPanic => "no_panic",
            Oracle::PeakAlloc => "peak_alloc",
            Oracle::WallClock => "wall_clock",
            Oracle::Determinism => "determinism",
        }
    }

    /// Parse a label produced by [`Oracle::label`].
    pub fn from_label(label: &str) -> Option<Oracle> {
        Oracle::ALL.into_iter().find(|o| o.label() == label)
    }
}

/// One oracle's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleStatus {
    /// The property held.
    Pass,
    /// The property was violated.
    Fail,
    /// The property could not be judged (the run broke before it could be
    /// measured). Treated as a regression from a passing baseline.
    Unevaluated,
}

impl OracleStatus {
    /// The stable baseline label.
    pub fn label(self) -> &'static str {
        match self {
            OracleStatus::Pass => "pass",
            OracleStatus::Fail => "fail",
            OracleStatus::Unevaluated => "unevaluated",
        }
    }

    /// Whether a passing baseline moving to this status is a regression.
    ///
    /// Only [`OracleStatus::Pass`] preserves a passing baseline; both `Fail`
    /// and `Unevaluated` are regressions, because losing the ability to verify
    /// a previously verified property is itself a loss.
    pub fn is_regression_from_pass(self) -> bool {
        self != OracleStatus::Pass
    }
}

/// The resource envelopes an oracle run enforces.
#[derive(Debug, Clone, Copy)]
pub struct OracleLimits {
    /// Peak-allocation envelope in bytes.
    pub peak_envelope_bytes: u64,
    /// Wall-clock ceiling.
    pub wall_clock: Duration,
}

impl OracleLimits {
    /// Envelopes from the compiled defaults, overridden by the environment
    /// variables when present and parseable.
    pub fn from_env() -> OracleLimits {
        let peak_envelope_bytes = std::env::var(ENV_PEAK_BYTES)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_PEAK_ENVELOPE_BYTES);
        let wall_clock_ms = std::env::var(ENV_TIMEOUT_MS)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_WALL_CLOCK_MS);
        OracleLimits {
            peak_envelope_bytes,
            wall_clock: Duration::from_millis(wall_clock_ms),
        }
    }
}

impl Default for OracleLimits {
    fn default() -> Self {
        OracleLimits {
            peak_envelope_bytes: DEFAULT_PEAK_ENVELOPE_BYTES,
            wall_clock: Duration::from_millis(DEFAULT_WALL_CLOCK_MS),
        }
    }
}
