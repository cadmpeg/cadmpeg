// SPDX-License-Identifier: Apache-2.0
//! Truncation and mutation sweeps.
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
//! through all four operations under both profiles; any check failure is a
//! sweep failure. `CADMPEG_HARNESS_SWEEP_LIMIT` caps the number of derived
//! inputs for a quick smoke.

use std::path::PathBuf;

use cadmpeg_harness::boundary::provider_for;
use cadmpeg_harness::driver::{run_job, RunKey};
use cadmpeg_harness::fixtures::{default_corpus_root, discover};
use cadmpeg_harness::limits::RunLimits;
use cadmpeg_harness::sweep::all_cases;
use cadmpeg_harness::{Operation, PolicyProfile};

fn runner() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-runner"))
}

/// Environment override for the number of cases selected from each case family.
const ENV_SMOKE_PER_FIXTURE: &str = "CADMPEG_HARNESS_SMOKE_PER_FIXTURE";

/// Default number selected from each case family.
const DEFAULT_SMOKE_PER_FIXTURE: usize = 1;

/// A bounded truncation and mutation sweep that runs in the fast test suite.
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

    let limits = RunLimits::from_env();
    let mut covered = 0usize;
    let mut failures = Vec::new();

    for fixture in &fixtures {
        let Some(provider) = provider_for(&fixture.codec_id) else {
            continue;
        };
        let bytes = std::fs::read(&fixture.abs_path).expect("read fixture");
        let cases = all_cases(provider, &bytes);
        let selected = cases
            .iter()
            .filter(|case| matches!(case.kind, cadmpeg_harness::sweep::CaseKind::Truncation { len } if len > 0))
            .take(per_fixture)
            .chain(
                cases
                    .iter()
                    .filter(|case| matches!(case.kind, cadmpeg_harness::sweep::CaseKind::Mutation { .. }))
                    .take(per_fixture),
            );
        for case in selected {
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
                    failures.push(format!(
                        "{} [{}]",
                        key.joined(),
                        result.failures().join(",")
                    ));
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
        "sweep-smoke failures ({}):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "exhaustive sweep; run deliberately"]
fn full_sweep() {
    let corpus = default_corpus_root();
    let fixtures = discover(&corpus).expect("discover fixtures");
    assert!(
        !fixtures.is_empty(),
        "no fixtures found under {}",
        corpus.display()
    );

    let limits = RunLimits::from_env();
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
                        failures.push(format!(
                            "{} [{}]{}",
                            key.joined(),
                            result.failures().join(","),
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
        "sweep failures ({}):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
