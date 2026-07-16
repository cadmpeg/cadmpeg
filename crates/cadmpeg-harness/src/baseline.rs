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

/// One baseline entry: the classified result and each oracle's verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineRecord {
    /// Classified result label. A gated ratchet dimension beside the oracle
    /// verdicts: [`regressions`] flags any divergence from this blessed class.
    pub result_class: String,
    /// Oracle label to verdict label.
    pub oracles: BTreeMap<String, String>,
}

/// A committed set of baselines plus the envelopes that produced them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    /// Acceptance-envelope version tag (`envelope-v1`).
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
            elapsed: Duration::from_millis(0),
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
}
