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

/// The format half of `--to` is checked before the input is opened.
///
/// A format this build cannot write is wrong whatever the source turns out to
/// be, so it fails against an absent path. The dialect half is a different
/// question and is answered after the read, by the encoder.
#[test]
fn an_unwritable_output_format_refuses_before_reading_input() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("does-not-exist.cadir.json");
    let path = missing.to_str().unwrap();

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "--to", "catia:v5"])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("catia is not an output format of this build")
                .and(predicate::str::contains("step")),
        );

    let output = dir.path().join("out.igs");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            path,
            "--to",
            "catia",
            "-o",
            output.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "catia is not an output format of this build",
        ));

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            path,
            "--to",
            "5.1",
            "-o",
            output.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("No such file or directory")
                .and(predicate::str::contains("not an output format").not()),
        );

    // A bare value that names neither a format nor an inferable one: there is
    // no output path to read a format from, so nothing can be resolved.
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "--to", "ap242e3"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "no output path to read a format from",
        ));

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "--to", "step:"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("nothing after the colon"));
}

/// A dialect outside the encoder's catalog is refused with that catalog, and
/// only after the source has been read.
///
/// This is the whole error surface the deleted `--iges-target requires IGES
/// output` strings used to hand-write. The catalog in the message comes from
/// `Encoder::targets()`, so it reflects the build's own feature set.
#[test]
fn an_unknown_dialect_is_refused_with_the_encoder_catalog() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.cadir.json", &unit_cube());
    let output = dir.path().join("cube.igs");

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--to",
            "ap242e3",
            "--allow-errors",
        ])
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("ap242e3")
                .and(predicate::str::contains("iges:5.3-fixed-ascii"))
                .and(predicate::str::contains("iges:4.0-fixed-ascii")),
        );
    assert!(!output.exists());

    let native_input = dir.path().join("native-cube.igs");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            native_input.to_str().unwrap(),
            "--allow-errors",
        ])
        .assert()
        .success();
    let report = dir.path().join("unsupported-target-report.json");
    let refused_output = dir.path().join("refused.igs");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            native_input.to_str().unwrap(),
            "-o",
            refused_output.to_str().unwrap(),
            "--to",
            "9.9",
            "--report",
            report.to_str().unwrap(),
            "--allow-errors",
        ])
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("iges cannot write 9.9")
                .and(predicate::str::contains("decode report (iges)")),
        );
    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["refusal"]["code"], "unsupported_target");
    assert!(value["decode_report"].is_object());
    assert!(value["check_report"].is_object());

    // A format-qualified token outside the catalog is the same refusal. The
    // encoder receives the token after the colon unchanged.
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step:ap999",
            "--allow-errors",
        ])
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("step cannot write ap999")
                .and(predicate::str::contains("step:ap242-e3")),
        );
}

/// Archive word 5 is a read-only dialect, not shorthand for archive 50.
#[test]
fn rhino_word_5_is_refused_with_the_encoder_catalog() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.cadir.json", &unit_cube());
    let output = dir.path().join("cube.3dm");

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--to",
            "5",
            "--allow-errors",
        ])
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("rhino cannot write 5")
                .and(predicate::str::contains("rhino:archive-50"))
                .and(predicate::str::contains("rhino:archive-80")),
        );
    assert!(!output.exists());
}

/// `--reject-lossy` takes an optional scope, and each scope refuses on its own
/// half of the loss surface.
///
/// The same Creo file loses content on both sides, and the two scopes name
/// different stages for it: `decode` reports what the reader could not carry,
/// `export` what the writer will not emit. Both are negative verdicts, so both
/// exit 1 rather than 2. A file that loses nothing is written under every
/// scope, which is what proves the scope is a predicate and not a switch that
/// refuses on sight.
#[test]
fn reject_lossy_scopes_select_which_losses_refuse() {
    let dir = tempdir().unwrap();
    let lossy = geometryless_creo(dir.path(), "lossy.prt");
    let path = lossy.to_str().unwrap();

    for scope in [
        "--reject-lossy",
        "--reject-lossy=decode",
        "--reject-lossy=any",
    ] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(["convert", path, "-f", "step", "--allow-empty", scope])
            .assert()
            .code(1)
            .stderr(predicate::str::contains("decode reported"));
    }

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            path,
            "-f",
            "step",
            "--allow-empty",
            "--reject-lossy=export",
        ])
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("export planning reported 1 loss(es)")
                .and(predicate::str::contains(
                    "uninterpreted passthrough record(s)",
                ))
                .and(predicate::str::contains("decode reported").not()),
        );

    let lossless = fixture(dir.path(), "cube.cadir.json", &unit_cube());
    for scope in [
        "--reject-lossy",
        "--reject-lossy=decode",
        "--reject-lossy=export",
    ] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(["convert", lossless.to_str().unwrap(), "-f", "step", scope])
            .assert()
            .success();
    }

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            path,
            "-f",
            "step",
            "--allow-empty",
            "--reject-lossy=nonesuch",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid value"));
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
