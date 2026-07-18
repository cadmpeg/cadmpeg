// SPDX-License-Identifier: Apache-2.0
//! Subprocess resource limits.

use std::time::Duration;

const GIB: u64 = 1024 * 1024 * 1024;

/// Default largest process-wide live heap for one run.
pub const DEFAULT_PEAK_BYTES: u64 = GIB;
/// Default wall-clock ceiling per run.
pub const DEFAULT_WALL_CLOCK_MS: u64 = 10_000;
/// Environment override for [`DEFAULT_PEAK_BYTES`].
pub const ENV_PEAK_BYTES: &str = "CADMPEG_HARNESS_PEAK_BYTES";
/// Environment override for [`DEFAULT_WALL_CLOCK_MS`].
pub const ENV_TIMEOUT_MS: &str = "CADMPEG_HARNESS_TIMEOUT_MS";

/// Resource limits enforced around one subprocess run.
#[derive(Debug, Clone, Copy)]
pub struct RunLimits {
    /// Maximum peak live heap in bytes.
    pub peak_bytes: u64,
    /// Wall-clock ceiling.
    pub wall_clock: Duration,
}

impl RunLimits {
    /// Returns defaults overridden by valid environment values.
    pub fn from_env() -> RunLimits {
        let peak_bytes = std::env::var(ENV_PEAK_BYTES)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_PEAK_BYTES);
        let wall_clock_ms = std::env::var(ENV_TIMEOUT_MS)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_WALL_CLOCK_MS);
        RunLimits {
            peak_bytes,
            wall_clock: Duration::from_millis(wall_clock_ms),
        }
    }
}

impl Default for RunLimits {
    fn default() -> Self {
        RunLimits {
            peak_bytes: DEFAULT_PEAK_BYTES,
            wall_clock: Duration::from_millis(DEFAULT_WALL_CLOCK_MS),
        }
    }
}
