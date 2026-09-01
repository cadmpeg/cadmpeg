// SPDX-License-Identifier: Apache-2.0
//! Command reports: the versioned JSON envelope on stdout and on disk.

use std::fs;

use assert_cmd::Command;
use cadmpeg_ir::examples::unit_cube;
use predicates::prelude::*;
use tempfile::tempdir;

use crate::support::*;

#[test]
fn artifact_reports_cover_success_and_semantic_refusal() {
    let dir = tempdir().unwrap();
    let cube = fixture(dir.path(), "cube.json", &unit_cube());
    let success_report = dir.path().join("success-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            cube.to_str().unwrap(),
            "-f",
            "step",
            "--report",
            success_report.to_str().unwrap(),
        ])
        .assert()
        .success();
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(success_report).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 7);
    assert_eq!(value["command"], "convert");
    assert_eq!(value["status"], "ok");
    assert!(value["refusal"].is_null());
    assert!(value["decode_report"].is_null());
    assert!(value["check_report"].is_object());
    assert_eq!(value["export"]["format"], "step");
    assert_eq!(value["export"]["census"]["basis"], "target_records");
    assert!(value["export"]["census"]["counts"].is_object());
    assert_eq!(value["export"]["fidelity"]["status"], "not_provided");
    assert!(value["export"]["losses"].is_array());
    assert!(value["export"]["notes"].is_array());

    let empty = geometryless_creo(dir.path(), "empty.prt");
    let refusal_report = dir.path().join("refusal-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            empty.to_str().unwrap(),
            "-f",
            "step",
            "--report",
            refusal_report.to_str().unwrap(),
        ])
        .assert()
        .code(1);
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(refusal_report).unwrap()).unwrap();
    assert_eq!(value["command"], "convert");
    assert_eq!(value["schema_version"], 7);
    assert_eq!(value["status"], "refused");
    assert_eq!(value["refusal"]["stage"], "plan");
    assert_eq!(value["refusal"]["code"], "empty_geometry");
    assert!(value["refusal"]["message"]
        .as_str()
        .unwrap()
        .contains("geometry"));
    assert!(value["decode_report"].is_object());
    assert!(value["check_report"].is_object());
    assert!(value["export"].is_null());
}

#[test]
fn convert_refuses_one_path_for_the_cad_file_and_command_report() {
    let dir = tempdir().unwrap();
    let cube = fixture(dir.path(), "cube.json", &unit_cube());
    let output = dir.path().join("collision.step");

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            cube.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--report",
            output.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "CAD output and command report resolve to the same path",
        ));

    assert!(!output.exists());
}

#[test]
fn report_write_failure_does_not_replace_a_typed_refusal() {
    let dir = tempdir().unwrap();
    let cube = fixture(dir.path(), "cube.json", &unit_cube());

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            cube.to_str().unwrap(),
            "--to",
            "step:ap999",
            "--report",
            dir.path().to_str().unwrap(),
            "--force",
        ])
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("step cannot write ap999").and(predicate::str::contains(
                "could not write convert refusal report",
            )),
        );
}

#[test]
fn f3d_export_report_identifies_regenerated_output() {
    let dir = tempdir().unwrap();
    let cube = fixture(dir.path(), "cube.json", &unit_cube());
    let output = dir.path().join("cube.f3d");
    let report = dir.path().join("f3d-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            cube.to_str().unwrap(),
            "-f",
            "f3d",
            "-o",
            output.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success();
    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 7);
    assert_eq!(value["export"]["format"], "f3d");
    assert!(value["export"]["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note
            .as_str()
            .is_some_and(|note| note.contains("regenerated"))));
}

#[test]
fn reporting_commands_emit_versioned_json_only_on_stdout() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.json", &unit_cube());
    let validate = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&validate.stdout).unwrap();
    assert_eq!(value["schema_version"], 7);
    assert_eq!(value["command"], "check");

    let diff = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "diff",
            input.to_str().unwrap(),
            input.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&diff.stdout).unwrap();
    assert_eq!(value["schema_version"], 7);
    assert_eq!(value["command"], "diff");

    let native = geometryless_creo(dir.path(), "ambiguous.bin");
    let inspect = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "inspect",
            native.to_str().unwrap(),
            "--input-format",
            "creo",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(inspect.status.success());
    let value: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(value["schema_version"], 7);
    assert_eq!(value["command"], "inspect");
}

#[test]
fn inspect_report_writes_versioned_summary_to_file() {
    let dir = tempdir().unwrap();
    let input = minimal_rhino_archive(dir.path(), "empty.3dm", "50");
    let report = dir.path().join("inspect-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "inspect",
            input.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("format: rhino (detected high)"));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 7);
    assert_eq!(value["command"], "inspect");
    assert_eq!(value["confidence"], "high");
    assert_eq!(value["summary"]["format"], "rhino");
}

#[test]
fn validate_report_writes_versioned_result_to_file() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.cadir.json", &unit_cube());
    let report = dir.path().join("validate-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "check",
            input.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("check: OK"));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 7);
    assert_eq!(value["command"], "check");
    assert!(value["decode_report"].is_null());
    assert!(value["check_report"].is_object());
}

#[test]
fn check_classifies_and_reports_an_unsupported_dialect() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("part28.xml");
    fs::write(&input, b"<iso_10303_28/>").unwrap();
    let report = dir.path().join("unsupported-dialect-report.json");

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "check",
            input.to_str().unwrap(),
            "--input-format",
            "step",
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "unsupported step dialect step:part28-xml",
        ));

    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["command"], "check");
    assert_eq!(value["status"], "refused");
    assert_eq!(value["refusal"]["stage"], "decode");
    assert_eq!(value["refusal"]["code"], "unsupported_dialect");
    assert!(value["decode_report"].is_null());
    assert!(value["check_report"].is_null());
}

#[test]
fn reporting_commands_accept_o_for_the_report_and_force_to_replace_it() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.cadir.json", &unit_cube());
    let report = dir.path().join("report.json");

    for (command, path_flag) in [
        (vec!["check"], "-o"),
        (vec!["check"], "--output"),
        (vec!["diff"], "-o"),
    ] {
        fs::write(&report, b"keep").unwrap();
        let mut arguments = command.clone();
        arguments.push(input.to_str().unwrap());
        if command[0] == "diff" {
            arguments.push(input.to_str().unwrap());
        }
        arguments.extend([path_flag, report.to_str().unwrap()]);

        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(&arguments)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("pass --force to overwrite"));
        assert_eq!(fs::read(&report).unwrap(), b"keep");

        arguments.push("--force");
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(&arguments)
            .assert()
            .success();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
        assert_eq!(value["command"], command[0]);
    }
}

#[test]
fn validate_agrees_between_its_exit_code_printed_summary_and_report() {
    // findings under `.check_report.findings` in the written report.
    let dir = tempdir().unwrap();
    let mut ir = unit_cube();
    let absent = format!("{}-absent", ir.model.faces[0].surface);
    ir.model.faces[0].surface = absent.into();
    let input = fixture(dir.path(), "broken.cadir.json", &ir);
    let report = dir.path().join("report.json");

    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "check",
            input.to_str().unwrap(),
            "-o",
            report.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let printed = String::from_utf8(output.stdout).unwrap();
    let summary = printed
        .lines()
        .find(|line| line.starts_with("check: FAILED"))
        .unwrap_or_else(|| panic!("{printed}"));
    let printed_errors: usize = summary
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(" error(s)"))
        .map_or_else(|| panic!("{summary}"), |(count, _)| count.parse().unwrap());

    let value: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    let findings = value["check_report"]["findings"].as_array().unwrap();
    let reported_errors = findings
        .iter()
        .filter(|finding| finding["severity"] == "error" || finding["severity"] == "blocking")
        .count();
    assert!(printed_errors > 0, "{summary}");
    assert_eq!(printed_errors, reported_errors, "{findings:#?}");
}

#[test]
fn diff_report_writes_versioned_result_to_file() {
    let dir = tempdir().unwrap();
    let cube = unit_cube();
    let a = fixture(dir.path(), "a.cadir.json", &cube);
    let b = fixture(dir.path(), "b.cadir.json", &cube);
    let report = dir.path().join("diff-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "diff",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("identical"));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 7);
    assert_eq!(value["command"], "diff");
    assert_eq!(value["different"], false);
    assert!(value["diff"].is_object());
}
