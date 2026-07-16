// SPDX-License-Identifier: Apache-2.0
//! Fast regression gate over the curated base fixtures.
//!
//! [`regression_gate`] re-runs the committed baseline's key set and fails only
//! on a regression, so it stays fast enough for `cargo test`. [`bless_baselines`]
//! regenerates the committed baseline after an intended behavior change and is
//! ignored by default.

use std::path::{Path, PathBuf};

use cadmpeg_harness::baseline::{regressions, run_matrix, Baseline};
use cadmpeg_harness::fixtures::{default_corpus_root, gate_fixtures};
use cadmpeg_harness::oracle::OracleLimits;

/// The `harness-runner` executable Cargo built for this test.
fn runner() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-runner"))
}

/// The committed baseline path.
fn baseline_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("baselines/stage1.json")
}

#[test]
fn regression_gate() {
    let corpus = default_corpus_root();
    let fixtures = gate_fixtures(&corpus);
    assert!(
        !fixtures.is_empty(),
        "no gate fixtures found under {}; set CADMPEG_HARNESS_CORPUS",
        corpus.display()
    );

    let limits = OracleLimits::from_env();
    let baseline = Baseline::load(&baseline_path()).unwrap_or_else(|error| {
        panic!(
            "load baseline {}: {error}; bless with `cargo test -p cadmpeg-harness --test gate -- --ignored bless_baselines`",
            baseline_path().display()
        )
    });

    let results = run_matrix(&runner(), &fixtures, &limits).expect("run gate matrix");
    let regressions = regressions(&baseline, &results);

    assert!(
        regressions.is_empty(),
        "oracle regressions against committed baseline:\n{}",
        regressions
            .iter()
            .map(|r| format!("  {} [{}] {} -> {}", r.key, r.oracle, r.baseline, r.current))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
#[ignore = "writes the committed baseline; run deliberately after a behavior change"]
fn bless_baselines() {
    let corpus = default_corpus_root();
    let fixtures = gate_fixtures(&corpus);
    assert!(
        !fixtures.is_empty(),
        "no gate fixtures found under {}",
        corpus.display()
    );

    let limits = OracleLimits::from_env();
    let results = run_matrix(&runner(), &fixtures, &limits).expect("run gate matrix");
    let baseline = Baseline::from_results(&results, &limits);
    baseline.save(&baseline_path()).expect("write baseline");
    eprintln!(
        "blessed {} baseline entries to {}",
        baseline.entries.len(),
        baseline_path().display()
    );
}
