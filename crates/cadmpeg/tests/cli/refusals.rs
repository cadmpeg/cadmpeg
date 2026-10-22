// SPDX-License-Identifier: Apache-2.0
//! Refusals and exit statuses: what the CLI declines to do, and how it says so.

use std::fs;

use assert_cmd::Command;
use cadmpeg_ir::examples::unit_cube;
use predicates::prelude::{predicate, PredicateBooleanExt};
use tempfile::tempdir;

use crate::support::{fixture, geometryless_creo, minimal_rhino_archive};

#[cfg(feature = "catia")]
mod catia;
#[cfg(feature = "step")]
mod empty_step;

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

    let mut invalid = unit_cube().expect("unit cube fixture is admitted");
    invalid.model.faces[0].surface = cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#missing")
        .expect("valid identity for an absent surface");
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
        .args(["convert", lossy.to_str().unwrap(), "-f", "step"])
        .assert()
        .success();
}

#[test]
fn strict_decode_refusal_is_semantic_and_serializes_the_completed_report() {
    let dir = tempdir().unwrap();
    let input = minimal_rhino_archive(dir.path(), "undeclared.3dm", "100");

    for (command, extra) in [
        ("dump", Vec::<&str>::new()),
        ("convert", vec!["--to", "cadir", "--reject-lossy"]),
    ] {
        let report = dir.path().join(format!("{command}-strict-report.json"));
        let mut args = vec![command, input.to_str().unwrap(), "--no-salvage"];
        args.extend(extra);
        args.extend(["--report", report.to_str().unwrap()]);

        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(args)
            .assert()
            .code(1)
            .stderr(predicate::str::contains("strict mode rejects"));

        let value: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
        assert_eq!(value["status"], "refused");
        assert_eq!(value["refusal"]["stage"], "decode");
        assert_eq!(value["refusal"]["code"], "strict_decode_rejected");
        let decode_report = value["decode_report"]
            .as_object()
            .expect("strict refusal keeps its completed decode report");
        assert!(decode_report["losses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|loss| {
                loss["code"]["namespace"] == "rhino"
                    && loss["code"]["code"] == "source.dialect-unverified"
            }));
    }
}

#[test]
fn reject_empty_refuses_while_default_writes_empty_native_geometry() {
    let dir = tempdir().unwrap();
    let input = geometryless_creo(dir.path(), "empty.prt");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "--reject-empty",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("refusing to write an empty"));
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", input.to_str().unwrap(), "-f", "step"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("ISO-10303-21"));
}

/// Container-only output remains an explicit caller policy, independent of writer scope.
#[test]
fn convert_refuses_container_only_native_output_under_each_empty_policy() {
    let dir = tempdir().unwrap();
    let input = geometryless_creo(dir.path(), "empty.prt");
    for reject_empty in [false, true] {
        let report = dir.path().join(format!("container-{reject_empty}.json"));
        let mut args = vec![
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "--container-only",
            "--report",
            report.to_str().unwrap(),
        ];
        if reject_empty {
            args.push("--reject-empty");
        }
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(args)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(
                "--container-only permits CADIR output only",
            ));
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
        assert_eq!(
            value["decode_report"]["transfer"]["scope"],
            serde_json::json!({"model": false, "design": false})
        );
        assert_eq!(value["refusal"]["code"], "container_only_output");
        assert_eq!(value["refusal"]["stage"], "plan");
    }
}

/// The format half of `--to` is checked before the input is opened.
///
/// A format this build cannot write is wrong whatever the source turns out to
/// be, so it fails against an absent path as a typed plan refusal. A dialect
/// token is admitted by the selected encoder's catalog at the same static
/// boundary.
#[test]
fn an_unwritable_output_format_refuses_before_reading_input() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("does-not-exist.cadir.json");
    let path = missing.to_str().unwrap();

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "--to", "catia:v5"])
        .assert()
        .code(1)
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
        .code(1)
        .stderr(predicate::str::contains(
            "catia is not an output format of this build",
        ));

    let input = fixture(
        dir.path(),
        "cube.cadir.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
    let report = dir.path().join("unwritable-format-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "--to",
            "catia:v5",
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .code(1);
    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["refusal"]["stage"], "plan");
    assert_eq!(value["refusal"]["code"], "unsupported_output_format");
    assert!(value["decode_report"].is_null());

    let missing_report = dir.path().join("missing-input-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            path,
            "--to",
            "5.1",
            "-o",
            output.to_str().unwrap(),
            "--report",
            missing_report.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains(format!("opening {}", missing.display()))
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

/// A dialect outside the encoder's catalog is refused with that catalog by the
/// encoder plan.
///
/// This is the whole error surface the deleted `--iges-target requires IGES
/// output` strings used to hand-write. The catalog in the message comes from
/// `Encoder::targets()`, so it reflects the build's own feature set.
#[test]
fn an_unknown_dialect_is_refused_with_the_encoder_catalog() {
    let dir = tempdir().unwrap();
    let input = fixture(
        dir.path(),
        "cube.cadir.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
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
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("iges cannot write 9.9"));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(value["refusal"]["code"], "unsupported_target");
    assert_eq!(
        value["refusal"]["target"]["refusal"]["kind"],
        "unknown_explicit"
    );
    assert_eq!(value["refusal"]["target"]["format"], "iges");
    assert_eq!(value["refusal"]["target"]["refusal"]["requested"], "9.9");
    assert!(value["refusal"]["target"]["available"]
        .as_array()
        .is_some_and(|available| !available.is_empty()));
    assert!(value["decode_report"].is_object());
    assert!(value["decode_report"]["identity"]["dialects"].is_object());
    assert!(value["check_report"].is_object());

    // Destination safety is checked before input work or planning. An existing
    // report path therefore wins before dialect admission.
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            native_input.to_str().unwrap(),
            "-o",
            refused_output.to_str().unwrap(),
            "--to",
            "8.8",
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("exists; pass --force")
                .and(predicate::str::contains("iges cannot write 8.8").not()),
        );

    // Dialect admission follows input loading because the encoder plan is its
    // only owner. A missing input therefore fails before dialect resolution.
    let missing_input = dir.path().join("missing.cadir.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            missing_input.to_str().unwrap(),
            "-o",
            refused_output.to_str().unwrap(),
            "--to",
            "9.9",
        ])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("error: opening")
                .and(predicate::str::contains("missing.cadir.json"))
                .and(predicate::str::contains("iges cannot write 9.9").not()),
        );

    // A format-qualified token outside the catalog is the same refusal. The
    // encoder receives the token after the colon unchanged.
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", input.to_str().unwrap(), "-f", "step:ap999"])
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
    let input = fixture(
        dir.path(),
        "cube.cadir.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
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
    let export_report = dir.path().join("export-loss-refusal.json");

    for scope in [
        "--reject-lossy",
        "--reject-lossy=decode",
        "--reject-lossy=any",
    ] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(["convert", path, "-f", "step", scope])
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
            "--reject-lossy=export",
            "--report",
            export_report.to_str().unwrap(),
        ])
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("export planning reported 3 loss(es)")
                .and(predicate::str::contains(
                    "uninterpreted passthrough record(s)",
                ))
                .and(predicate::str::contains("decode reported").not()),
        );
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(export_report).unwrap()).unwrap();
    assert_eq!(report["refusal"]["code"], "export_loss_rejected");
    assert!(report["export"].is_object());
    assert_eq!(report["export"]["identity"]["payload"], "native");
    assert_eq!(report["export"]["losses"].as_array().unwrap().len(), 3);
    assert!(report["export"]["losses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|loss| {
            loss["code"]["namespace"] == "export"
                && loss["code"]["code"] == "output.empty"
                && loss["severity"] == "warning"
        }));

    let lossless = fixture(
        dir.path(),
        "cube.cadir.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
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
        .args(["convert", path, "-f", "step", "--reject-lossy=nonesuch"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid value"));
}

#[test]
fn json_on_artifact_commands_is_a_teaching_error() {
    let dir = tempdir().unwrap();
    let ir = unit_cube().expect("unit cube fixture is admitted");
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
    let input = fixture(
        dir.path(),
        "cube.cadir.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
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

#[test]
fn decode_loss_policy_treats_restored_sidecars_as_history() {
    let dir = tempdir().unwrap();
    let native = geometryless_creo(dir.path(), "lossy.prt");
    let cadir = dir.path().join("lossy.cadir.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "dump",
            native.to_str().unwrap(),
            "-o",
            cadir.to_str().unwrap(),
        ])
        .assert()
        .success();
    for with_history in [true, false] {
        if !with_history {
            fs::remove_file(cadmpeg_ir::decode_sidecar_path(&cadir).unwrap()).unwrap();
        }
        let mut command = Command::cargo_bin("cadmpeg").unwrap();
        command.args([
            "convert",
            cadir.to_str().unwrap(),
            "-f",
            "cadir",
            "--reject-lossy=decode",
        ]);
        let assertion = command.assert().success();
        if with_history {
            assertion.stderr(predicate::str::contains(
                "decode that produced this CADIR; the current CADIR parse is lossless",
            ));
        }
    }
}

#[test]
fn edited_cadir_ignores_stale_sidecar_and_decode_policy_gates_current_warning() {
    use cadmpeg_ir::{decode_sidecar_path, hash::digest::Sha256Digest, CadIr, DecodeSidecar};

    let dir = tempdir().unwrap();
    let native = geometryless_creo(dir.path(), "lossy.prt");
    let cadir = dir.path().join("edited.cadir.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "dump",
            native.to_str().unwrap(),
            "-o",
            cadir.to_str().unwrap(),
        ])
        .assert()
        .success();
    let sidecar_path = decode_sidecar_path(&cadir).unwrap();
    let sidecar = DecodeSidecar::from_json(&fs::read_to_string(&sidecar_path).unwrap()).unwrap();
    let mut document = CadIr::from_json(&fs::read_to_string(&cadir).unwrap()).unwrap();
    document.tolerances = cadmpeg_ir::units::Tolerances::new(0.001, 0.001).unwrap();
    let edited = document.to_canonical_json().unwrap();
    fs::write(&cadir, &edited).unwrap();
    let arguments = [
        "convert",
        cadir.to_str().unwrap(),
        "-f",
        "cadir",
        "--reject-lossy=decode",
    ];
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(arguments)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("sidecar.stale"));
    for floor in ["warning", "info"] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(arguments)
            .args(["--reject-lossy-floor", floor])
            .assert()
            .code(1)
            .stderr(predicate::str::contains("sidecar.stale"));
    }
    for floor in ["error", "blocking"] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(arguments)
            .args(["--reject-lossy-floor", floor])
            .assert()
            .success()
            .stderr(predicate::str::contains("sidecar.stale"));
    }
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", cadir.to_str().unwrap(), "-f", "cadir"])
        .assert()
        .success()
        .stderr(predicate::str::contains("sidecar.stale"));
    let rebound = DecodeSidecar::bind_sha256(
        Sha256Digest::digest(edited.as_bytes()),
        sidecar.report,
        sidecar.fidelity,
    );
    fs::write(&sidecar_path, rebound.to_canonical_json().unwrap()).unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(arguments)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "decode that produced this CADIR; the current CADIR parse is lossless",
        ));
}

#[test]
fn a_partial_document_scope_prints_a_note_under_each_loss_policy() {
    let dir = tempdir().unwrap();
    let mut document = unit_cube().unwrap();
    document
        .set_scope(cadmpeg_ir::document::ContentScope::geometry())
        .unwrap();
    let input = fixture(dir.path(), "geometry.cadir.json", &document);
    for policy in [
        "--reject-lossy=decode",
        "--reject-lossy=export",
        "--reject-lossy=any",
    ] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(["convert", input.to_str().unwrap(), "-f", "step", policy])
            .assert()
            .success()
            .stderr(predicate::str::contains(
                "the document's neutral scope excludes design; no neutral records of those classes are supplied",
            ));
    }
}

#[test]
fn decode_loss_floor_applies_to_native_input_and_never_to_history() {
    let dir = tempdir().unwrap();
    let native = minimal_rhino_archive(dir.path(), "unverified.3dm", "100");
    let cadir = dir.path().join("unverified.cadir.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "dump",
            native.to_str().unwrap(),
            "-o",
            cadir.to_str().unwrap(),
        ])
        .assert()
        .success();
    for (floor, refuses_native) in [
        ("info", true),
        ("warning", true),
        ("error", false),
        ("blocking", false),
    ] {
        for input in [&native, &cadir] {
            let mut command = Command::cargo_bin("cadmpeg").unwrap();
            command.args([
                "convert",
                input.to_str().unwrap(),
                "-f",
                "cadir",
                "--reject-lossy=decode",
                "--reject-lossy-floor",
                floor,
            ]);
            if input == &native && refuses_native {
                command
                    .assert()
                    .code(1)
                    .stderr(predicate::str::contains("decode reported"));
            } else if input == &cadir {
                command.assert().success().stderr(predicate::str::contains(
                    "decode that produced this CADIR; the current CADIR parse is lossless",
                ));
            } else {
                command.assert().success();
            }
        }
    }
}

#[cfg(all(feature = "step", feature = "rhino"))]
#[test]
fn reject_empty_refuses_geometry_all_omitted_by_target() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;
    const FINER_THAN_RHINO_WRITE_TOLERANCE: f64 = 1e-12;
    let dir = tempdir().unwrap();
    let mut document = unit_cube().unwrap();
    document.tolerances = cadmpeg_ir::units::Tolerances::new(
        FINER_THAN_RHINO_WRITE_TOLERANCE,
        document.tolerances.angular.get(),
    )
    .unwrap();
    let mut source = Vec::new();
    cadmpeg_codec_step::StepCodec::default()
        .plan(EncodeInput::new(&document, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut source))
        .expect("STEP carries the exact cube and fine stated tolerance");
    let decoded = cadmpeg_codec_step::StepCodec::default()
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();
    assert!(
        decoded.report().geometry_transferred(),
        "the source decode delivers BRep, so this exercises completed-target emptiness"
    );
    let input = dir.path().join("fine.step");
    let output = dir.path().join("empty.3dm");
    let report = dir.path().join("refusal.json");
    fs::write(&input, source).unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--to",
            "rhino:archive-80",
            "--report",
            report.to_str().unwrap(),
            "--reject-empty",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("refusing to write an empty"));
    assert!(!output.exists());
    let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(report["refusal"]["code"], "empty_geometry");
    assert_eq!(
        report["decode_report"]["transfer"]["geometry_transferred"],
        true
    );
    assert!(report["export"]["losses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|loss| loss["code"]["code"]
            .as_str()
            .is_some_and(|code| code.contains("no-exportable-solids"))));
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--to",
            "rhino:archive-80",
        ])
        .assert()
        .success();
    let empty = cadmpeg_codec_rhino::RhinoCodec
        .decode(
            &mut Cursor::new(fs::read(output).unwrap()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(empty.ir().model.bodies.is_empty());
    assert!(empty.ir().model.points.is_empty());
    assert!(empty.ir().model.curves.is_empty());
    assert!(empty.ir().model.surfaces.is_empty());
}

#[cfg(feature = "rhino")]
#[test]
fn an_info_only_native_decode_is_rejected_at_info_and_passes_higher_floors() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::report::Severity;
    use std::io::Cursor;

    let native = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cadmpeg-codec-rhino/tests/golden/fixtures/generated_point.3dm");
    let bytes = fs::read(&native).unwrap();
    let decoded = cadmpeg_codec_rhino::RhinoCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded.report().losses.is_empty());
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.severity == Severity::Info));
    let dir = tempdir().unwrap();
    for (floor, refused) in [
        ("info", true),
        ("warning", false),
        ("error", false),
        ("blocking", false),
    ] {
        let report = dir.path().join(format!("{floor}.report.json"));
        let assertion = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                native.to_str().unwrap(),
                "-f",
                "cadir",
                "--reject-lossy=decode",
                "--reject-lossy-floor",
                floor,
                "--report",
                report.to_str().unwrap(),
            ])
            .assert();
        if refused {
            assertion
                .code(1)
                .stderr(predicate::str::contains("decode reported"));
        } else {
            assertion.success();
        }
        let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(report["status"], if refused { "refused" } else { "ok" });
        if refused {
            assert_eq!(report["refusal"]["code"], "decode_loss_rejected");
        }
        assert!(report["decode_report"]["losses"]
            .as_array()
            .unwrap()
            .iter()
            .all(|loss| loss["severity"] == "info"));
    }
}

#[test]
fn empty_cadir_writes_warning_and_export_floor_applies_before_emission() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "empty.cadir.json", &cadmpeg_ir::CadIr::empty());
    let output = dir.path().join("empty.step");
    let report = dir.path().join("empty-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(fs::read(&output).unwrap().starts_with(b"ISO-10303-21"));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(value["status"], "ok");
    let empty = value["export"]["losses"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|loss| {
            loss["code"]["namespace"] == "export" && loss["code"]["code"] == "output.empty"
        })
        .collect::<Vec<_>>();
    assert_eq!(empty.len(), 1);
    assert_eq!(empty[0]["severity"], "warning");
    for (floor, exit) in [("info", 1), ("warning", 1), ("error", 0), ("blocking", 0)] {
        let target = dir.path().join(format!("empty-{floor}.step"));
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                input.to_str().unwrap(),
                "-o",
                target.to_str().unwrap(),
                "--reject-lossy=export",
                "--reject-lossy-floor",
                floor,
            ])
            .assert()
            .code(exit);
        assert_eq!(target.exists(), exit == 0);
    }
    let rejected = dir.path().join("rejected.step");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            rejected.to_str().unwrap(),
            "--reject-empty",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("refusing to write an empty"));
    assert!(!rejected.exists());
}
