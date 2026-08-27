// SPDX-License-Identifier: Apache-2.0
//! Refusals and exit statuses: what the CLI declines to do, and how it says so.

use std::fs;

use assert_cmd::Command;
use cadmpeg_ir::examples::unit_cube;
use predicates::prelude::*;
use tempfile::tempdir;

use crate::support::*;

#[test]
fn garbage_reports_supported_formats() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("garbage.bin");
    fs::write(&input, b"not a CAD file").unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", input.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "supported: FCStd, f3d, Inventor IPT/IAM, sldprt, CATPart, NX/Creo prt, Rhino 3DM, IGES, STEP",
        ));
}

#[test]
fn exit_codes_distinguish_semantic_and_operational_failures() {
    let dir = tempdir().unwrap();
    let garbage = dir.path().join("garbage");
    fs::write(&garbage, b"garbage").unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dump", garbage.to_str().unwrap()])
        .assert()
        .code(2);
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", garbage.to_str().unwrap(), garbage.to_str().unwrap()])
        .assert()
        .code(2);

    let mut invalid = unit_cube();
    invalid.model.faces[0].surface.0 = "missing".into();
    let invalid = fixture(dir.path(), "invalid.json", &invalid);
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", invalid.to_str().unwrap()])
        .assert()
        .code(1);
}

#[test]
fn reject_lossy_refuses_lossy_export_as_a_model_refusal() {
    let dir = tempdir().unwrap();
    let lossy = geometryless_creo(dir.path(), "lossy.prt");

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            lossy.to_str().unwrap(),
            "-f",
            "step",
            "--allow-empty",
            "--reject-lossy",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("refusing to write a lossy"));

    let report = dir.path().join("lossy-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            lossy.to_str().unwrap(),
            "-f",
            "step",
            "--allow-empty",
            "--reject-lossy",
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .code(1);
    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert!(value["decode_report"].is_object());
    assert!(value["export"].is_null());

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            lossy.to_str().unwrap(),
            "-f",
            "step",
            "--allow-empty",
        ])
        .assert()
        .success();
}

#[test]
fn convert_rejects_empty_native_geometry_unless_allowed() {
    let dir = tempdir().unwrap();
    let input = geometryless_creo(dir.path(), "empty.prt");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", input.to_str().unwrap(), "-f", "step"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--allow-empty"));
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "--allow-empty",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("ISO-10303-21"));
}

#[test]
fn convert_rejects_container_only_geometry_unless_allowed() {
    let dir = tempdir().unwrap();
    let input = geometryless_creo(dir.path(), "empty.prt");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "--container-only",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("--allow-empty"));
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "--container-only",
            "--allow-empty",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("ISO-10303-21"));
}

#[test]
fn wrong_target_flags_refuse_before_reading_input() {
    let dir = tempdir().unwrap();
    // Absent path: wrong-target must fail before open/read.
    let missing = dir.path().join("does-not-exist.cadir.json");
    let path = missing.to_str().unwrap();

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "-f", "step", "--iges-target", "5.3"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--iges-target requires IGES output",
        ));

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "-f", "cadir", "--step-target", "ap214"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--step-target/--reject-step-losses require STEP output",
        ));

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "-f", "iges", "--reject-step-losses"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--step-target/--reject-step-losses require STEP output",
        ));

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "-f", "step", "--rhino-target", "80"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--rhino-target requires Rhino output",
        ));
}

#[test]
fn json_on_artifact_commands_is_a_teaching_error() {
    let dir = tempdir().unwrap();
    let ir = unit_cube();
    let model = fixture(dir.path(), "cube.cadir.json", &ir);
    let path = model.to_str().unwrap();

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dump", path, "--json"])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("already JSON").and(predicate::str::contains("cadmpeg query")),
        );

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "--json"])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("not an output selector")
                .and(predicate::str::contains("--report")),
        );
}

#[test]
fn report_to_an_unwritable_path_is_an_operational_error() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.cadir.json", &unit_cube());
    let report = dir.path().join("missing-subdir").join("report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "check",
            input.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .code(2);
    assert!(!report.exists());
}

#[cfg(unix)]
#[test]
fn closed_stdout_pipe_exits_on_sigpipe_without_panic() {
    use std::io::Read;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command as ProcessCommand, Stdio};

    // Hex of 64 KiB exceeds the typical pipe buffer, so the writer is still
    // printing when the reader closes.
    let dir = tempdir().unwrap();
    let input = dir.path().join("zeros.bin");
    fs::write(&input, vec![0u8; 64 * 1024]).unwrap();

    let mut child = ProcessCommand::new(assert_cmd::cargo::cargo_bin("cadmpeg"))
        .args(["inspect", "hex", input.to_str().unwrap(), "--len", "65536"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdout = child.stdout.take().unwrap();
    let mut first = [0u8; 1];
    stdout.read_exact(&mut first).unwrap();
    drop(stdout);

    let mut stderr = child.stderr.take().unwrap();
    let status = child.wait().unwrap();
    let mut err = String::new();
    stderr.read_to_string(&mut err).unwrap();

    assert!(
        !err.contains("panicked"),
        "closed stdout pipe panicked:\n{err}"
    );
    assert_eq!(
        status.signal(),
        Some(13),
        "expected SIGPIPE, got {status:?}; stderr={err}"
    );
}
