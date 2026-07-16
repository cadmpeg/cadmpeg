// SPDX-License-Identifier: Apache-2.0
//! The exhaustive stage-1 truncation and mutation sweep.
//!
//! Ignored by default so `cargo test` stays fast; run it deliberately:
//!
//! ```text
//! cargo test -p cadmpeg-harness --test sweep -- --ignored --nocapture
//! ```
//!
//! Every discovered fixture is truncated at each boundary neighbourhood, at
//! stratified offsets, and (below the size threshold) at every byte, and
//! single-byte-mutated at header/count positions. Each derived input runs
//! through all four operations under both profiles; any oracle failure is a
//! sweep failure. `CADMPEG_HARNESS_SWEEP_LIMIT` caps the number of derived
//! inputs for a quick smoke.

use std::path::PathBuf;

use cadmpeg_harness::boundary::provider_for;
use cadmpeg_harness::driver::{run_job, RunKey};
use cadmpeg_harness::fixtures::{default_corpus_root, discover};
use cadmpeg_harness::oracle::OracleLimits;
use cadmpeg_harness::sweep::all_cases;
use cadmpeg_harness::{Operation, PolicyProfile};

fn runner() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-runner"))
}

#[test]
#[ignore = "exhaustive sweep; run deliberately, not in the fast gate"]
fn full_sweep() {
    let corpus = default_corpus_root();
    let fixtures = discover(&corpus).expect("discover fixtures");
    assert!(
        !fixtures.is_empty(),
        "no fixtures found under {}",
        corpus.display()
    );

    let limits = OracleLimits::from_env();
    let case_limit = std::env::var("CADMPEG_HARNESS_SWEEP_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok());

    let mut derived = 0usize;
    let mut runs = 0usize;
    let mut failures = Vec::new();

    'fixtures: for fixture in &fixtures {
        let Some(provider) = provider_for(&fixture.codec_id) else {
            continue;
        };
        let bytes = std::fs::read(&fixture.abs_path).expect("read fixture");
        for case in all_cases(provider, &bytes) {
            if case_limit.is_some_and(|limit| derived >= limit) {
                break 'fixtures;
            }
            derived += 1;
            let fixture_label = format!("{}#{}", fixture.rel_path, case.label);
            for op in Operation::ALL {
                for profile in PolicyProfile::ALL {
                    let key = RunKey::new(&fixture.codec_id, &fixture_label, op, profile);
                    let result = run_job(&runner(), key.clone(), &case.bytes, &limits)
                        .expect("run sweep job");
                    runs += 1;
                    if !result.all_pass() {
                        let broken: Vec<String> = result
                            .oracles
                            .iter()
                            .filter(|(_, status)| {
                                **status != cadmpeg_harness::oracle::OracleStatus::Pass
                            })
                            .map(|(oracle, status)| {
                                format!("{}={}", oracle.label(), status.label())
                            })
                            .collect();
                        failures.push(format!(
                            "{} [{}]{}",
                            key.joined(),
                            broken.join(","),
                            if result.stderr.is_empty() {
                                String::new()
                            } else {
                                format!(" stderr={}", result.stderr.trim())
                            }
                        ));
                    }
                }
            }
        }
    }

    eprintln!(
        "sweep: {derived} derived inputs, {runs} runs across {} fixtures",
        fixtures.len()
    );
    assert!(
        failures.is_empty(),
        "sweep oracle failures ({}):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
