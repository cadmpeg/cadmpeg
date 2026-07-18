// SPDX-License-Identifier: Apache-2.0
//! Stage-2 runtime report-oracle gate.
//!
//! The stage-2 adoption matrix (`tests/stage2_gates.rs`) pins *which* report
//! oracles gate for each codec; this gate runs them. For every gate fixture's
//! successful full decode, it judges the child's [`ReportSummary`] against the codec's report
//! oracles, failing when a produced source-fidelity ledger does not validate or
//! a retention degradation carries no paired loss note.

use std::path::{Path, PathBuf};

use cadmpeg_harness::driver::{run_job, RunKey};
use cadmpeg_harness::execute::probe_runtime_gates;
use cadmpeg_harness::fixtures::{default_corpus_root, gate_fixtures};
use cadmpeg_harness::oracle::OracleLimits;
use cadmpeg_harness::stage2::{status_from_manifest, workspace_root};
use cadmpeg_harness::{Operation, PolicyProfile};

/// The `harness-runner` executable Cargo built for this test.
fn runner() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-runner"))
}

#[test]
fn report_oracles_hold_on_gate_fixtures() {
    let corpus = default_corpus_root();
    let fixtures = gate_fixtures(&corpus);
    assert!(
        !fixtures.is_empty(),
        "no gate fixtures found under {}; set CADMPEG_HARNESS_CORPUS",
        corpus.display()
    );

    let limits = OracleLimits::from_env();
    let root: &Path = &workspace_root();
    let mut failures = Vec::new();

    for fixture in &fixtures {
        let status = status_from_manifest(root, &fixture.codec_id)
            .unwrap_or_else(|error| panic!("read {} manifest: {error}", fixture.codec_id));
        let bytes = std::fs::read(&fixture.abs_path).expect("read fixture");
        let runtime = probe_runtime_gates(&fixture.codec_id, &bytes);
        for violation in status.judge_runtime(&runtime) {
            failures.push(format!(
                "{} [{:?}] {}",
                fixture.rel_path, violation.oracle, violation.detail
            ));
        }
        for profile in PolicyProfile::ALL {
            let key = RunKey::new(
                &fixture.codec_id,
                &fixture.rel_path,
                Operation::FullDecode,
                profile,
            );
            let result = run_job(&runner(), key.clone(), &bytes, &limits).expect("run decode");
            let Some(report) = &result.report else {
                continue;
            };
            for violation in status.judge_report(report) {
                failures.push(format!(
                    "{} [{:?}] {}",
                    key.joined(),
                    violation.oracle,
                    violation.detail
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "stage-2 report-oracle violations:\n{}",
        failures.join("\n")
    );
}
