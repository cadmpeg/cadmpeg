// SPDX-License-Identifier: Apache-2.0
//! `cadmpeg query` integration tests.

#![allow(clippy::unwrap_used)]

use std::fs;

use assert_cmd::Command;
use cadmpeg_ir::examples::unit_cube;
use predicates::prelude::*;
use tempfile::tempdir;

fn cadmpeg() -> Command {
    Command::cargo_bin("cadmpeg").unwrap()
}

fn write(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

const VALIDATE_REPORT: &str = r#"{
  "schema_version": 5,
  "command": "validate",
  "decode_report": null,
  "validation_report": {
    "entity_counts": {"faces": 2, "edges": 12},
    "findings": [
      {"check": "identity", "severity": "error", "message": "duplicate id", "entity": "e1"},
      {"check": "units", "severity": "warning", "message": "non-canonical unit"}
    ],
    "losses": [
      {"code": "topology_not_transferred", "severity": "warning", "message": "wire dropped"}
    ]
  }
}"#;

const CADIR_DOC: &str = r#"{
  "ir_version": "4",
  "model": {"faces": [{"id": "f1"}, {"id": "f2"}], "edges": []},
  "native": {"fcstd": {"arenas": {"objects": [1, 2, 3]}}}
}"#;

const SIDECAR: &str = r#"{
  "version": "1",
  "ir_sha256": "abc123",
  "report": {
    "format": "f3d",
    "container_only": false,
    "geometry_transferred": true,
    "coverage": {"streams": 7, "segments": 3},
    "losses": [{"code": "metadata_not_transferred", "severity": "info", "message": "thumbnail"}]
  }
}"#;

#[test]
fn summary_detects_all_three_artifact_kinds() {
    let dir = tempdir().unwrap();
    for (name, content, kind) in [
        ("report.json", VALIDATE_REPORT, "command report"),
        ("model.cadir.json", CADIR_DOC, "CADIR document"),
        ("model.decode.json", SIDECAR, "decode sidecar"),
    ] {
        let path = write(dir.path(), name, content);
        cadmpeg()
            .args(["query", "summary", path.to_str().unwrap()])
            .assert()
            .success()
            .stdout(
                predicate::str::starts_with("field\tvalue\n")
                    .and(predicate::str::contains(format!("kind\t{kind}"))),
            );
    }
}

#[test]
fn findings_and_losses_project_tsv_with_a_header() {
    let dir = tempdir().unwrap();
    let report = write(dir.path(), "report.json", VALIDATE_REPORT);

    cadmpeg()
        .args(["query", "findings", report.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            "severity\tcheck\tentity\tmessage\n\
             error\tidentity\te1\tduplicate id\n\
             warning\tunits\t\tnon-canonical unit\n",
        );

    cadmpeg()
        .args(["query", "losses", report.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            "severity\tcode\tmessage\n\
             warning\ttopology_not_transferred\twire dropped\n",
        );
}

#[test]
fn coverage_on_a_report_without_a_decode_stage_is_empty_not_an_error() {
    let dir = tempdir().unwrap();
    let report = write(dir.path(), "report.json", VALIDATE_REPORT);
    cadmpeg()
        .args(["query", "coverage", report.to_str().unwrap()])
        .assert()
        .success()
        .stdout("measure\tcount\n")
        .stderr(predicate::str::contains("no decode report"));
}

#[test]
fn coverage_projects_a_sidecar_decode_report() {
    let dir = tempdir().unwrap();
    let sidecar = write(dir.path(), "model.decode.json", SIDECAR);
    cadmpeg()
        .args(["query", "coverage", sidecar.to_str().unwrap()])
        .assert()
        .success()
        .stdout("measure\tcount\nsegments\t3\nstreams\t7\n");
}

#[test]
fn counts_works_on_all_three_kinds_with_teaching_on_the_sidecar() {
    let dir = tempdir().unwrap();

    let cadir = write(dir.path(), "model.cadir.json", CADIR_DOC);
    cadmpeg()
        .args(["query", "counts", cadir.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            "namespace\tarena\tentries\n\
             model\tedges\t0\n\
             model\tfaces\t2\n\
             native.fcstd\tobjects\t3\n",
        );

    let report = write(dir.path(), "report.json", VALIDATE_REPORT);
    cadmpeg()
        .args(["query", "counts", report.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            "namespace\tarena\tentries\n\
             model\tedges\t12\n\
             model\tfaces\t2\n",
        );

    let sidecar = write(dir.path(), "model.decode.json", SIDECAR);
    cadmpeg()
        .args(["query", "counts", sidecar.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("no entity counts")
                .and(predicate::str::contains("query coverage")),
        );
}

#[test]
fn arenas_alias_matches_counts_byte_for_byte() {
    let dir = tempdir().unwrap();
    let cadir = write(dir.path(), "model.cadir.json", CADIR_DOC);
    let counts = cadmpeg()
        .args(["query", "counts", cadir.to_str().unwrap()])
        .output()
        .unwrap();
    let arenas = cadmpeg()
        .args(["query", "arenas", cadir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(counts.status.success());
    assert_eq!(counts.stdout, arenas.stdout);
}

#[test]
fn findings_on_a_cadir_document_teaches_validate_then_query() {
    let dir = tempdir().unwrap();
    let cadir = write(dir.path(), "model.cadir.json", CADIR_DOC);
    cadmpeg()
        .args(["query", "findings", cadir.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("cadmpeg validate")
                .and(predicate::str::contains("query findings")),
        );
}

#[test]
fn query_json_wraps_the_projection_in_the_versioned_envelope() {
    let dir = tempdir().unwrap();
    let report = write(dir.path(), "report.json", VALIDATE_REPORT);
    let output = cadmpeg()
        .args(["query", "findings", "--json", report.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 5);
    assert_eq!(value["command"], "query findings");
    assert_eq!(value["findings"].as_array().unwrap().len(), 2);
    assert_eq!(value["findings"][0]["check"], "identity");
}

#[test]
fn query_reads_stdin_with_dash() {
    cadmpeg()
        .args(["query", "losses", "-"])
        .write_stdin(SIDECAR)
        .assert()
        .success()
        .stdout(predicate::str::contains("metadata_not_transferred"));
}

#[test]
fn a_version_two_sidecar_still_projects() {
    let dir = tempdir().unwrap();
    let sidecar = write(
        dir.path(),
        "model.decode.json",
        &SIDECAR.replace("\"version\": \"1\"", "\"version\": \"2\""),
    );
    cadmpeg()
        .args(["query", "coverage", sidecar.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("streams\t7"));
}

#[test]
fn an_unrecognized_json_file_names_the_expected_kinds() {
    let dir = tempdir().unwrap();
    let stray = write(dir.path(), "stray.json", r#"{"hello": "world"}"#);
    cadmpeg()
        .args(["query", "summary", stray.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("command report")
                .and(predicate::str::contains("CADIR document"))
                .and(predicate::str::contains(".decode.json sidecar")),
        );
}

#[test]
fn a_non_json_file_is_an_operational_error() {
    let dir = tempdir().unwrap();
    let stray = write(dir.path(), "stray.bin", "\u{1}\u{2}not json");
    cadmpeg()
        .args(["query", "summary", stray.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not a JSON object"));
}

#[test]
fn query_projects_a_real_validate_report_end_to_end() {
    let dir = tempdir().unwrap();
    let ir = unit_cube();
    let model = dir.path().join("cube.cadir.json");
    fs::write(&model, ir.to_canonical_json().unwrap()).unwrap();
    let report = dir.path().join("report.json");

    cadmpeg()
        .args([
            "validate",
            model.to_str().unwrap(),
            "-o",
            report.to_str().unwrap(),
        ])
        .assert()
        .success();

    cadmpeg()
        .args(["query", "summary", report.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("kind\tcommand report"));

    cadmpeg()
        .args(["query", "counts", report.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("namespace\tarena\tentries\n"));

    cadmpeg()
        .args(["query", "counts", model.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::starts_with("namespace\tarena\tentries\n")
                .and(predicate::str::contains("model\t")),
        );
}
