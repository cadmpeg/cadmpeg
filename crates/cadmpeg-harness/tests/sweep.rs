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

/// Environment override for the per-fixture case cap of [`sweep_smoke`].
const ENV_SMOKE_PER_FIXTURE: &str = "CADMPEG_HARNESS_SMOKE_PER_FIXTURE";

/// Default per-fixture derived-case cap for the smoke. One case per fixture
/// keeps the smoke to roughly `fixtures × profiles` child runs, comparable to
/// the regression gate, while still touching every discovered fixture.
const DEFAULT_SMOKE_PER_FIXTURE: usize = 1;

/// A bounded truncation sweep that runs in the fast gate, unlike [`full_sweep`].
///
/// [`full_sweep`] is exhaustive (tens of thousands of runs) and therefore
/// `#[ignore]`, so before this test nothing exercised the truncation sweep in
/// `cargo test` — the pipeline only ran when someone invoked it by hand. That
/// let a panic on a truncated non-gate fixture (only six of ~sixty fixtures are
/// curated gate fixtures, decoded whole) reach `main` unnoticed. This smoke
/// closes the routine gap: it discovers **every** fixture, derives a small,
/// deterministic prefix of its sweep cases (the boundary-neighbourhood
/// truncations, ordered first), and runs each through the isolated runner under
/// both profiles, judged by the same stage-1 oracles. The exhaustive matrix
/// stays in [`full_sweep`], which a scheduled CI job runs.
#[test]
fn sweep_smoke() {
    let corpus = default_corpus_root();
    let fixtures = discover(&corpus).expect("discover fixtures");
    assert!(
        !fixtures.is_empty(),
        "no fixtures found under {}",
        corpus.display()
    );

    let per_fixture = std::env::var(ENV_SMOKE_PER_FIXTURE)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_SMOKE_PER_FIXTURE);

    let limits = OracleLimits::from_env();
    let mut covered = 0usize;
    let mut failures = Vec::new();

    for fixture in &fixtures {
        let Some(provider) = provider_for(&fixture.codec_id) else {
            continue;
        };
        let bytes = std::fs::read(&fixture.abs_path).expect("read fixture");
        for case in all_cases(provider, &bytes).into_iter().take(per_fixture) {
            covered += 1;
            let fixture_label = format!("{}#{}", fixture.rel_path, case.label);
            for profile in PolicyProfile::ALL {
                let key = RunKey::new(
                    &fixture.codec_id,
                    &fixture_label,
                    Operation::FullDecode,
                    profile,
                );
                let result =
                    run_job(&runner(), key.clone(), &case.bytes, &limits).expect("run smoke job");
                if !result.all_pass() {
                    let broken: Vec<String> = result
                        .oracles
                        .iter()
                        .filter(|(_, status)| {
                            **status != cadmpeg_harness::oracle::OracleStatus::Pass
                        })
                        .map(|(oracle, status)| format!("{}={}", oracle.label(), status.label()))
                        .collect();
                    failures.push(format!("{} [{}]", key.joined(), broken.join(",")));
                }
            }
        }
    }

    assert!(
        covered > 0,
        "no sweep cases derived across {} fixtures",
        fixtures.len()
    );
    assert!(
        failures.is_empty(),
        "sweep-smoke oracle failures ({}):\n{}",
        failures.len(),
        failures.join("\n")
    );
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
