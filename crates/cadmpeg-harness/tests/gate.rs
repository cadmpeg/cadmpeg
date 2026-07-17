// SPDX-License-Identifier: Apache-2.0
//! Fast regression gate over the curated base fixtures.
//!
//! [`regression_gate`] re-runs the committed baseline's key set and fails only
//! on a regression, so it stays fast enough for `cargo test`. [`bless_baselines`]
//! regenerates the committed baseline after an intended behavior change and is
//! ignored by default.

use std::path::{Path, PathBuf};

use cadmpeg_harness::baseline::{perf_regressions, regressions, run_matrix, Baseline};
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

    // Oracle verdicts computed under one envelope cannot be compared against a
    // baseline blessed under another. Refuse before running rather than ratchet
    // against a calibration the committed baseline no longer claims.
    let mismatches = baseline.calibration_mismatches(&limits);
    assert!(
        mismatches.is_empty(),
        "runtime envelope differs from the committed baseline's; re-bless with `cargo test -p cadmpeg-harness --test gate -- --ignored bless_baselines`:\n{}",
        mismatches
            .iter()
            .map(|m| format!("  {}: baseline {} vs current {}", m.field, m.baseline, m.current))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let results = run_matrix(&runner(), &fixtures, &limits).expect("run gate matrix");
    let regressions = regressions(&baseline, &results);

    assert!(
        regressions.is_empty(),
        "oracle regressions against committed baseline:\n{}",
        regressions
            .iter()
            .map(|r| format!(
                "  {} [{}] {} -> {}",
                r.key, r.dimension, r.baseline, r.current
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );

    // The performance ratchet: an order-of-magnitude slowdown or heap growth
    // that stays inside the absolute envelope still fails, requiring a re-bless
    // (explicit review) to clear — doc §10 Phase 2 performance gate.
    let perf = perf_regressions(&baseline, &results);
    assert!(
        perf.is_empty(),
        "measured performance regressions against committed baseline (re-bless with `cargo test -p cadmpeg-harness --test gate -- --ignored bless_baselines` after reviewing):\n{}",
        perf
            .iter()
            .map(|r| format!(
                "  {} [{}] {} -> {}",
                r.key, r.dimension, r.baseline, r.current
            ))
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
