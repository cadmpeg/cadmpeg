// SPDX-License-Identifier: Apache-2.0
//! The multidimensional baseline schema and the regression check.
//!
//! A baseline entry is keyed `codec|fixture|operation|profile` and records the
//! classified result plus each oracle's verdict separately, never an aggregate
//! — so one oracle regressing while another improves cannot hide. The committed
//! baseline is a ratchet: the gate fails when an oracle that passed now fails
//! (or can no longer be verified), or when the classified result diverges from
//! the blessed class. Oracle verdicts are only comparable under the envelope
//! they were blessed with, so the baseline records that envelope and the gate
//! refuses a run whose calibration has since shifted.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::driver::{run_job, RunKey, RunResult};
use crate::fixtures::Fixture;
use crate::model::{Operation, PolicyProfile, ENVELOPE_VERSION};
use crate::oracle::{Oracle, OracleLimits, OracleStatus};

/// One baseline entry: the classified result, each oracle's verdict, and the
/// measured performance values the pass/fail oracles were derived from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineRecord {
    /// Classified result label. A gated ratchet dimension beside the oracle
    /// verdicts: [`regressions`] flags any divergence from this blessed class.
    pub result_class: String,
    /// Oracle label to verdict label.
    pub oracles: BTreeMap<String, String>,
    /// Measured wall-clock milliseconds the parent observed for this run. `0`
    /// when the run broke before it could be timed, or when read from a baseline
    /// blessed before measured values were recorded. [`perf_regressions`] gates a
    /// significant increase against this value even while the pass/fail
    /// [`Oracle::WallClock`] verdict stays green inside the envelope.
    #[serde(default)]
    pub wall_clock_ms: u64,
    /// Measured peak process allocation in bytes. `0` when the child reported no
    /// outcome, or when read from a pre-measurement baseline. [`perf_regressions`]
    /// gates a significant increase against this value even while
    /// [`Oracle::PeakAlloc`] stays green inside the envelope.
    #[serde(default)]
    pub peak_alloc_bytes: u64,
}

/// A committed set of baselines plus the envelopes that produced them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    /// Acceptance-envelope version tag (`envelope-v3`).
    pub envelope_version: String,
    /// Peak-allocation envelope the run used.
    pub peak_envelope_bytes: u64,
    /// Wall-clock ceiling the run used, in milliseconds.
    pub wall_clock_ms: u64,
    /// Baseline entries, sorted by key.
    pub entries: BTreeMap<String, BaselineRecord>,
}

impl Baseline {
    /// Build a baseline from a set of run results and the envelopes that
    /// produced them.
    pub fn from_results(results: &[RunResult], limits: &OracleLimits) -> Baseline {
        let mut entries = BTreeMap::new();
        for result in results {
            let oracles = result
                .oracles
                .iter()
                .map(|(oracle, status)| (oracle.label().to_owned(), status.label().to_owned()))
                .collect();
            entries.insert(
                result.key.joined(),
                BaselineRecord {
                    result_class: result
                        .result_class
                        .clone()
                        .unwrap_or_else(|| "unknown".to_owned()),
                    oracles,
                    wall_clock_ms: result.elapsed.as_millis() as u64,
                    peak_alloc_bytes: result.peak_alloc_bytes.unwrap_or(0),
                },
            );
        }
        Baseline {
            envelope_version: ENVELOPE_VERSION.to_owned(),
            peak_envelope_bytes: limits.peak_envelope_bytes,
            wall_clock_ms: limits.wall_clock.as_millis() as u64,
            entries,
        }
    }

    /// Load a baseline from `path`.
    pub fn load(path: &Path) -> std::io::Result<Baseline> {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }

    /// Serialize this baseline as stable, pretty JSON with a trailing newline.
    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self).unwrap_or_default();
        json.push('\n');
        json
    }

    /// Write this baseline to `path`.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.to_json())
    }

    /// Every calibration value that differs between this baseline's recorded
    /// envelope and the runtime `limits`/[`ENVELOPE_VERSION`].
    ///
    /// Oracle verdicts are only comparable under the envelope they were blessed
    /// with: a peak that passes at a raised ceiling, or a run that finishes
    /// under a lengthened timeout, would ratchet against a baseline that
    /// recorded the old, tighter value and never register the change. A
    /// non-empty result means the committed baseline must be re-blessed before
    /// its verdicts can be trusted; the gate refuses rather than compare across
    /// a shifted calibration.
    pub fn calibration_mismatches(&self, limits: &OracleLimits) -> Vec<CalibrationMismatch> {
        let mut out = Vec::new();
        let mut check = |field: &str, baseline: String, current: String| {
            if baseline != current {
                out.push(CalibrationMismatch {
                    field: field.to_owned(),
                    baseline,
                    current,
                });
            }
        };
        check(
            "envelope_version",
            self.envelope_version.clone(),
            ENVELOPE_VERSION.to_owned(),
        );
        check(
            "peak_envelope_bytes",
            self.peak_envelope_bytes.to_string(),
            limits.peak_envelope_bytes.to_string(),
        );
        check(
            "wall_clock_ms",
            self.wall_clock_ms.to_string(),
            (limits.wall_clock.as_millis() as u64).to_string(),
        );
        out
    }
}

/// One calibration value that differs between the committed baseline and the
/// runtime envelope, so the gate cannot trust the baseline's oracle verdicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalibrationMismatch {
    /// Which calibration value differs (`envelope_version`,
    /// `peak_envelope_bytes`, or `wall_clock_ms`).
    pub field: String,
    /// The value the committed baseline recorded.
    pub baseline: String,
    /// The value the current run uses.
    pub current: String,
}

/// The pseudo-dimension label used to report a classified-result divergence,
/// distinct from any [`Oracle::label`].
pub const RESULT_CLASS_DIMENSION: &str = "result_class";

/// A `result_class` recorded when the run broke before it could be classified;
/// the classification ratchet does not gate it.
const UNKNOWN_CLASS: &str = "unknown";

/// One regression: a gated dimension that no longer matches the baseline —
/// an oracle that passed now fails, or the classified result diverged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Regression {
    /// Baseline entry key.
    pub key: String,
    /// The gated dimension: an [`Oracle::label`], or [`RESULT_CLASS_DIMENSION`]
    /// for a classified-result divergence.
    pub dimension: String,
    /// Baseline verdict (`pass` for an oracle, the blessed class otherwise).
    pub baseline: String,
    /// Current verdict.
    pub current: String,
}

/// Every regression of `current` against `baseline`.
///
/// A key present in the baseline but missing from the current run regresses
/// every oracle that passed — the property can no longer be verified — and its
/// classified result. Beyond the four oracles, the blessed `result_class` is
/// itself gated: any divergence is flagged (in either direction, since result
/// classes carry no pass/fail ordering), so a codec silently flipping a fixture
/// between `ok` and an error class cannot ship green. New keys in the current
/// run are not regressions; they are candidates for a re-bless.
pub fn regressions(baseline: &Baseline, current: &[RunResult]) -> Vec<Regression> {
    let current_by_key: BTreeMap<String, &RunResult> = current
        .iter()
        .map(|result| (result.key.joined(), result))
        .collect();

    let mut out = Vec::new();
    for (key, record) in &baseline.entries {
        for (oracle_label, baseline_status) in &record.oracles {
            if baseline_status != OracleStatus::Pass.label() {
                continue;
            }
            let current_status = current_by_key
                .get(key)
                .zip(Oracle::from_label(oracle_label))
                .and_then(|(result, oracle)| result.oracles.get(&oracle))
                .copied();
            let regressed = match current_status {
                Some(status) => status.is_regression_from_pass(),
                None => true,
            };
            if regressed {
                out.push(Regression {
                    key: key.clone(),
                    dimension: oracle_label.clone(),
                    baseline: baseline_status.clone(),
                    current: current_status
                        .map_or("missing", OracleStatus::label)
                        .to_owned(),
                });
            }
        }

        // `unknown` marks a run that broke before it could be classified when
        // blessed; there is no meaningful class to hold it to.
        if record.result_class != UNKNOWN_CLASS {
            let current_class = current_by_key
                .get(key)
                .and_then(|result| result.result_class.clone());
            let diverged = current_class.as_deref() != Some(record.result_class.as_str());
            if diverged {
                out.push(Regression {
                    key: key.clone(),
                    dimension: RESULT_CLASS_DIMENSION.to_owned(),
                    baseline: record.result_class.clone(),
                    current: current_class.unwrap_or_else(|| "missing".to_owned()),
                });
            }
        }
    }
    out
}

/// The factor by which a measured performance value must exceed its blessed
/// baseline to count as a significant regression.
///
/// The pass/fail [`Oracle::WallClock`]/[`Oracle::PeakAlloc`] verdicts only fire
/// at the far process-safety envelope (1 GiB peak, 10 s wall); a safety refactor
/// that makes a codec 50x slower or 100x hungrier stays green under them. Gating
/// the *measured* value against its blessed value catches that class. The factor
/// is deliberately loose so ordinary machine-to-machine variance and allocator
/// noise do not trip it — only an order-of-magnitude regression does — matching
/// doc §10's "significant regressions require explicit review".
pub const PERF_REGRESSION_FACTOR: u64 = 4;

/// Absolute wall-clock floor below which a ratio is not judged. A run measured
/// in single-digit milliseconds crosses [`PERF_REGRESSION_FACTOR`] on scheduler
/// jitter alone; the floor suppresses that noise.
pub const PERF_WALL_FLOOR_MS: u64 = 250;

/// Absolute peak-allocation floor below which a ratio is not judged, suppressing
/// large ratios between two small heaps.
pub const PERF_PEAK_FLOOR_BYTES: u64 = 16 * 1024 * 1024;

/// One measured performance value that regressed past the ratchet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerfRegression {
    /// Baseline entry key.
    pub key: String,
    /// The measured dimension (`wall_clock_ms` or `peak_alloc_bytes`).
    pub dimension: String,
    /// The blessed measured value.
    pub baseline: u64,
    /// The current measured value.
    pub current: u64,
}

/// Judge one measured dimension against its blessed value under the ratchet.
fn perf_exceeds(baseline: u64, current: u64, floor: u64) -> bool {
    // A zero baseline is "not recorded" (a pre-measurement bless or a broken
    // run), and a current at or below the noise floor is never judged.
    baseline != 0 && current > floor && current > baseline.saturating_mul(PERF_REGRESSION_FACTOR)
}

/// Every measured performance value in `current` that regressed past the
/// ratchet against its blessed baseline entry.
///
/// This is the automated half of the doc §10 Phase 2 performance gate: the
/// pass/fail oracles hold the absolute envelope, this holds each run to the
/// order of magnitude it was blessed at. A regression here means the codec got
/// dramatically slower or hungrier on a fixture without crossing the envelope,
/// and requires an explicit re-bless (the reviewer's sign-off) to clear.
/// Entries whose blessed measured values are `0` (blessed before measured
/// values were recorded, or a broken run) are skipped, so an old baseline keeps
/// gating its oracle verdicts while its performance ratchet lies dormant until a
/// re-bless populates the measured values.
pub fn perf_regressions(baseline: &Baseline, current: &[RunResult]) -> Vec<PerfRegression> {
    let current_by_key: BTreeMap<String, &RunResult> = current
        .iter()
        .map(|result| (result.key.joined(), result))
        .collect();

    let mut out = Vec::new();
    for (key, record) in &baseline.entries {
        let Some(result) = current_by_key.get(key) else {
            continue;
        };
        let current_wall = result.elapsed.as_millis() as u64;
        if perf_exceeds(record.wall_clock_ms, current_wall, PERF_WALL_FLOOR_MS) {
            out.push(PerfRegression {
                key: key.clone(),
                dimension: "wall_clock_ms".to_owned(),
                baseline: record.wall_clock_ms,
                current: current_wall,
            });
        }
        if let Some(current_peak) = result.peak_alloc_bytes {
            if perf_exceeds(record.peak_alloc_bytes, current_peak, PERF_PEAK_FLOOR_BYTES) {
                out.push(PerfRegression {
                    key: key.clone(),
                    dimension: "peak_alloc_bytes".to_owned(),
                    baseline: record.peak_alloc_bytes,
                    current: current_peak,
                });
            }
        }
    }
    out
}

/// Run the full operation × profile matrix over `fixtures`, one child process
/// per cell.
pub fn run_matrix(
    runner: &Path,
    fixtures: &[Fixture],
    limits: &OracleLimits,
) -> std::io::Result<Vec<RunResult>> {
    let mut results = Vec::new();
    for fixture in fixtures {
        let bytes = std::fs::read(&fixture.abs_path)?;
        for op in Operation::ALL {
            for profile in PolicyProfile::ALL {
                let key = RunKey::new(&fixture.codec_id, &fixture.rel_path, op, profile);
                results.push(run_job(runner, key, &bytes, limits)?);
            }
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn limits(peak: u64, wall_ms: u64) -> OracleLimits {
        OracleLimits {
            peak_envelope_bytes: peak,
            wall_clock: Duration::from_millis(wall_ms),
        }
    }

    fn empty_baseline(peak: u64, wall_ms: u64) -> Baseline {
        Baseline {
            envelope_version: ENVELOPE_VERSION.to_owned(),
            peak_envelope_bytes: peak,
            wall_clock_ms: wall_ms,
            entries: BTreeMap::new(),
        }
    }

    fn all_pass_result(codec: &str, class: &str) -> RunResult {
        perf_result(codec, class, 0, 0)
    }

    fn perf_result(codec: &str, class: &str, wall_ms: u64, peak: u64) -> RunResult {
        let key = RunKey::new(
            codec,
            "fixture",
            Operation::FullDecode,
            PolicyProfile::DesktopV1,
        );
        let oracles = Oracle::ALL
            .into_iter()
            .map(|oracle| (oracle, OracleStatus::Pass))
            .collect();
        RunResult {
            key,
            oracles,
            result_class: Some(class.to_owned()),
            peak_alloc_bytes: Some(peak),
            report: None,
            elapsed: Duration::from_millis(wall_ms),
            timed_out: false,
            stderr: String::new(),
        }
    }

    fn baseline_from(result: &RunResult) -> Baseline {
        Baseline::from_results(std::slice::from_ref(result), &limits(1024, 10_000))
    }

    #[test]
    fn matching_calibration_reports_no_mismatch() {
        let baseline = empty_baseline(1024, 10_000);
        assert!(baseline
            .calibration_mismatches(&limits(1024, 10_000))
            .is_empty());
    }

    #[test]
    fn a_raised_ceiling_is_a_calibration_mismatch() {
        let baseline = empty_baseline(1024, 10_000);
        let mismatches = baseline.calibration_mismatches(&limits(2048, 10_000));
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].field, "peak_envelope_bytes");
        assert_eq!(mismatches[0].baseline, "1024");
        assert_eq!(mismatches[0].current, "2048");
    }

    #[test]
    fn a_class_flip_regresses_even_when_every_oracle_still_passes() {
        // The blessed run classified `malformed`; a later run keeps every oracle
        // green but returns `ok`. Without gating the class, the flip would ship
        // silently — the exact gap this dimension closes.
        let baseline = baseline_from(&all_pass_result("rhino", "malformed"));
        let current = vec![all_pass_result("rhino", "ok")];
        let regs = regressions(&baseline, &current);
        assert_eq!(regs.len(), 1);
        assert_eq!(regs[0].dimension, RESULT_CLASS_DIMENSION);
        assert_eq!(regs[0].baseline, "malformed");
        assert_eq!(regs[0].current, "ok");
    }

    #[test]
    fn a_reproduced_class_does_not_regress() {
        let baseline = baseline_from(&all_pass_result("sldprt", "ok"));
        let current = vec![all_pass_result("sldprt", "ok")];
        assert!(regressions(&baseline, &current).is_empty());
    }

    fn baseline_with(result: &RunResult) -> Baseline {
        Baseline::from_results(std::slice::from_ref(result), &limits(1 << 30, 10_000))
    }

    #[test]
    fn an_order_of_magnitude_slowdown_inside_the_envelope_regresses() {
        // Blessed at 300 ms; a 3 s run stays green under the 10 s wall-clock
        // oracle yet is a 10x regression — the class the ratchet must catch.
        let baseline = baseline_with(&perf_result("nx", "ok", 300, 32 << 20));
        let current = vec![perf_result("nx", "ok", 3_000, 32 << 20)];
        let regs = perf_regressions(&baseline, &current);
        assert_eq!(regs.len(), 1);
        assert_eq!(regs[0].dimension, "wall_clock_ms");
    }

    #[test]
    fn a_hundredfold_peak_growth_inside_the_envelope_regresses() {
        let baseline = baseline_with(&perf_result("nx", "ok", 300, 20 << 20));
        let current = vec![perf_result("nx", "ok", 300, 2_000 << 20)];
        let regs = perf_regressions(&baseline, &current);
        assert_eq!(regs.len(), 1);
        assert_eq!(regs[0].dimension, "peak_alloc_bytes");
    }

    #[test]
    fn noise_under_the_floor_does_not_regress() {
        // 2 ms blessed, 40 ms current: a 20x ratio, but both below the wall
        // floor, so scheduler jitter cannot trip the ratchet.
        let baseline = baseline_with(&perf_result("nx", "ok", 2, 1 << 20));
        let current = vec![perf_result("nx", "ok", 40, 4 << 20)];
        assert!(perf_regressions(&baseline, &current).is_empty());
    }

    #[test]
    fn a_pre_measurement_baseline_has_a_dormant_perf_ratchet() {
        // Zero measured values (an old bless) are "not recorded": the ratchet
        // stays dormant rather than treating every current value as infinite
        // regression.
        let baseline = baseline_with(&perf_result("nx", "ok", 0, 0));
        let current = vec![perf_result("nx", "ok", 9_000, 900 << 20)];
        assert!(perf_regressions(&baseline, &current).is_empty());
    }
}
