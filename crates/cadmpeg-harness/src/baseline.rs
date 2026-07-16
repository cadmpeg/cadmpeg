// SPDX-License-Identifier: Apache-2.0
//! The multidimensional baseline schema and the regression check.
//!
//! A baseline entry is keyed `codec|fixture|operation|profile` and records each
//! oracle's verdict separately, never an aggregate — so one oracle regressing
//! while another improves cannot hide. The committed baseline is a ratchet: the
//! gate fails only when an oracle that passed now fails (or can no longer be
//! verified).

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
    /// Classified result label, informative context beside the oracles.
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
}

/// One regression: an oracle that passed in the baseline no longer passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Regression {
    /// Baseline entry key.
    pub key: String,
    /// Oracle label.
    pub oracle: String,
    /// Baseline verdict (always `pass`).
    pub baseline: String,
    /// Current verdict.
    pub current: String,
}

/// Every regression of `current` against `baseline`.
///
/// A key present in the baseline but missing from the current run regresses
/// every oracle that passed — the property can no longer be verified. New keys
/// in the current run are not regressions; they are candidates for a re-bless.
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
                    oracle: oracle_label.clone(),
                    baseline: baseline_status.clone(),
                    current: current_status
                        .map_or("missing", OracleStatus::label)
                        .to_owned(),
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
