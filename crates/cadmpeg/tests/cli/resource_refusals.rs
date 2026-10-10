// SPDX-License-Identifier: Apache-2.0
//! Resource and codec failures retain the CLI stream and exit contract.

use std::fs;

use assert_cmd::Command;
use cadmpeg_ir::CadIr;
use serde_json::Value;
use tempfile::tempdir;

fn assert_resource_report(value: &Value, command: &str) {
    assert_eq!(value["ir_version"], cadmpeg_ir::IR_VERSION);
    assert_eq!(value["command"], command);
    assert_eq!(value["status"], "refused");
    assert!(value["generator"].is_string());
    assert_eq!(value["refusal"]["stage"], "decode");
    assert_eq!(value["refusal"]["code"], "resource_limit");
    assert_eq!(
        value["refusal"]["resource"],
        serde_json::json!({
            "dimension": "input_bytes",
            "reason": "budget_exceeded",
            "limit": 4,
            "used": 4,
            "requested": 1,
            "operation": "complete input",
        })
    );
    assert!(value["refusal"]["message"]
        .as_str()
        .unwrap()
        .contains("complete input"));
    assert!(value["decode_report"].is_null());
    assert!(value["check_report"].is_null());
    assert!(value["export"].is_null());
}

#[test]
fn check_json_reports_a_tiny_input_policy_refusal() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("input.cadir.json");
    fs::write(&input, b"12345").unwrap();
    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "check",
            input.to_str().unwrap(),
            "--json",
            "--input-format",
            "cadir",
            "--max-input-bytes",
            "4",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_resource_report(&value, "check");
    assert!(String::from_utf8_lossy(&output.stderr).contains("during complete input"));
}

#[test]
fn report_files_retain_resource_refusals_without_writing_artifacts() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("input.cadir.json");
    fs::write(&input, b"12345").unwrap();
    let native_format = cadmpeg_registry::input_names()
        .unwrap()
        .into_iter()
        .find(|name| {
            matches!(
                cadmpeg_registry::forced_input(name).unwrap(),
                Some(cadmpeg_registry::ForcedInput::Codec(_))
            )
        });
    for command in ["check", "dump", "convert", "inspect", "diff"] {
        if command == "inspect" && native_format.is_none() {
            continue;
        }
        let report = directory.path().join(format!("{command}-report.json"));
        let artifact = directory
            .path()
            .join(format!("{command}-output.cadir.json"));
        let mut process = Command::cargo_bin("cadmpeg").unwrap();
        process.args([command, input.to_str().unwrap()]);
        if command == "diff" {
            process.arg(&input).args(["--input-format-a", "cadir"]);
        } else {
            let format = if command == "inspect" {
                native_format.unwrap()
            } else {
                "cadir"
            };
            process.args(["--input-format", format]);
        }
        if matches!(command, "dump" | "convert") {
            process.args(["-o", artifact.to_str().unwrap()]);
        } else {
            process.arg("--json");
        }
        let output = process
            .args([
                "--max-input-bytes",
                "4",
                "--report",
                report.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{command}");
        let value: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
        assert_resource_report(&value, command);
        if matches!(command, "dump" | "convert") {
            assert!(output.stdout.is_empty(), "{command}");
        } else {
            let stdout: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(stdout, value, "{command}");
        }
        assert!(!artifact.exists(), "{command}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("during complete input"));
    }
}

#[test]
fn check_resource_report_works_without_json_stdout() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("input.cadir.json");
    let report = directory.path().join("check-report.json");
    fs::write(&input, b"12345").unwrap();
    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "check",
            input.to_str().unwrap(),
            "--input-format",
            "cadir",
            "--max-input-bytes",
            "4",
            "--report",
            report.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let value: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_resource_report(&value, "check");
}

#[cfg(feature = "iges")]
#[test]
fn codec_failures_write_one_envelope_for_each_reporting_command() {
    let directory = tempdir().unwrap();
    let malformed = directory.path().join("malformed.igs");
    fs::write(&malformed, b"not an IGES file").unwrap();
    let valid = directory.path().join("valid.cadir.json");
    fs::write(&valid, CadIr::empty().to_canonical_json().unwrap()).unwrap();
    for command in ["check", "inspect", "dump", "convert", "diff"] {
        for right_fails in [false, true] {
            if right_fails && command != "diff" {
                continue;
            }
            let report = directory
                .path()
                .join(format!("{command}-{right_fails}-report.json"));
            let mut process = Command::cargo_bin("cadmpeg").unwrap();
            process.arg(command);
            if command == "diff" {
                if right_fails {
                    process.args([
                        valid.to_str().unwrap(),
                        malformed.to_str().unwrap(),
                        "--input-format-b",
                        "iges",
                    ]);
                } else {
                    process.args([
                        malformed.to_str().unwrap(),
                        valid.to_str().unwrap(),
                        "--input-format-a",
                        "iges",
                    ]);
                }
            } else {
                process.args([malformed.to_str().unwrap(), "--input-format", "iges"]);
            }
            if command == "convert" {
                process.args(["--to", "cadir"]);
            }
            if !matches!(command, "dump" | "convert") {
                process.arg("--json");
            }
            let output = process
                .args(["--report", report.to_str().unwrap()])
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(2));
            let value: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
            assert_eq!(value["command"], command);
            assert_eq!(value["status"], "refused");
            assert_eq!(value["refusal"]["stage"], "decode");
            assert_eq!(value["refusal"]["code"], "decode_failed");
            assert!(value["refusal"]["message"]
                .as_str()
                .unwrap()
                .contains("IGES"));
            if matches!(command, "dump" | "convert") {
                assert!(output.stdout.is_empty());
            } else {
                let stdout: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(stdout, value);
            }
        }
    }
}

#[cfg(feature = "rhino")]
#[test]
fn binary_stdout_guard_writes_the_requested_refusal_report() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("input.cadir.json");
    fs::write(&input, CadIr::empty().to_canonical_json().unwrap()).unwrap();
    let report = directory.path().join("binary-stdout-report.json");
    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "--to",
            "rhino",
            "--report",
            report.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let value: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["status"], "refused");
    assert_eq!(value["refusal"]["code"], "binary_stdout_rejected");
    assert_eq!(value["refusal"]["stage"], "plan");
}

#[test]
fn report_io_failure_preserves_resource_exit_status_and_json_evidence() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("input.cadir.json");
    let report = directory
        .path()
        .join("missing-directory")
        .join("report.json");
    fs::write(&input, b"12345").unwrap();
    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "check",
            input.to_str().unwrap(),
            "--json",
            "--input-format",
            "cadir",
            "--max-input-bytes",
            "4",
            "--report",
            report.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_resource_report(&value, "check");
    assert!(!report.exists());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("could not write check refusal report")
    );
}

#[test]
fn diff_refusal_report_cannot_replace_either_input() {
    let directory = tempdir().unwrap();
    let left = directory.path().join("left.cadir.json");
    let right = directory.path().join("right.cadir.json");
    let text = CadIr::empty().to_canonical_json().unwrap();
    for path in [&left, &right] {
        fs::write(path, &text).unwrap();
    }
    for limit in ["4".to_owned(), text.len().to_string()] {
        for report in [&left, &right] {
            Command::cargo_bin("cadmpeg")
                .unwrap()
                .args([
                    "diff",
                    left.to_str().unwrap(),
                    right.to_str().unwrap(),
                    "--input-format-a",
                    "cadir",
                    "--input-format-b",
                    "cadir",
                    "--max-input-bytes",
                    &limit,
                    "--report",
                    report.to_str().unwrap(),
                    "--force",
                ])
                .assert()
                .code(2);
            assert_eq!(fs::read(report).unwrap(), text.as_bytes());
        }
    }
}

#[test]
fn input_byte_override_admits_the_exact_file_length() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("input.cadir.json");
    let text = CadIr::empty().to_canonical_json().unwrap();
    fs::write(&input, &text).unwrap();
    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "check",
            input.to_str().unwrap(),
            "--json",
            "--limits",
            "service",
            "--max-input-bytes",
            &text.len().to_string(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "ok");
    assert!(value["refusal"].is_null());
}

#[cfg(all(feature = "rhino", feature = "step"))]
#[test]
fn check_json_retains_semantic_decode_refusals() {
    let directory = tempdir().unwrap();
    let strict = crate::support::minimal_rhino_archive(directory.path(), "undeclared.3dm", "100");
    let unsupported = directory.path().join("part28.xml");
    fs::write(&unsupported, b"<iso_10303_28/>").unwrap();
    for (input, format, code) in [
        (&strict, "rhino", "strict_decode_rejected"),
        (&unsupported, "step", "unsupported_dialect"),
    ] {
        let report = directory.path().join(format!("{code}-report.json"));
        let output = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "check",
                input.to_str().unwrap(),
                "--input-format",
                format,
                "--no-salvage",
                "--json",
                "--report",
                report.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["status"], "refused");
        assert_eq!(value["refusal"]["stage"], "decode");
        assert_eq!(value["refusal"]["code"], code);
        let saved: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(saved, value);
    }
}

#[test]
fn inspect_container_json_retains_resource_evidence() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("input.bin");
    fs::write(&input, b"12345").unwrap();
    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "inspect",
            "container",
            input.to_str().unwrap(),
            "--json",
            "--max-input-bytes",
            "4",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "inspect");
    assert_eq!(value["subcommand"], "container");
    assert_eq!(value["status"], "refused");
    assert_eq!(value["refusal"]["code"], "resource_limit");
    assert_eq!(
        value["refusal"]["resource"],
        serde_json::json!({
            "dimension": "input_bytes", "reason": "budget_exceeded", "limit": 4,
            "used": 4, "requested": 1, "operation": "complete input",
        })
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("during complete input"));
}
