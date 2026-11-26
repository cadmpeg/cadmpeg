// SPDX-License-Identifier: Apache-2.0
//! Refusals and exit statuses: what the CLI declines to do, and how it says so.

use std::fs;

use assert_cmd::Command;
use cadmpeg_ir::examples::unit_cube;
use predicates::prelude::{predicate, PredicateBooleanExt};
use tempfile::tempdir;

use crate::support::fixture;
#[cfg(all(feature = "creo", any(feature = "step", feature = "iges")))]
use crate::support::geometryless_creo;
#[cfg(feature = "rhino")]
use crate::support::rhino_archive_with_unreadable_point;

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
    invalid.geometry.faces[0].surface =
        cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#missing")
            .expect("valid identity for an absent surface");
    let invalid = fixture(dir.path(), "invalid.json", &invalid);
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", invalid.to_str().unwrap()])
        .assert()
        .code(1);
}

#[cfg(all(feature = "creo", feature = "step"))]
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
        .stderr(predicate::str::contains("the decode's strictness refuses"));

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
    let value: serde_json::Value =
        cadmpeg_core::serde_erase::json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert!(value["decode_report"].is_object());
    assert!(value["export"].is_null());

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", lossy.to_str().unwrap(), "-f", "step"])
        .assert()
        .success();
}

#[cfg(feature = "rhino")]
#[test]
fn strict_decode_refusal_is_semantic_and_serializes_the_completed_report() {
    let dir = tempdir().unwrap();
    let input = rhino_archive_with_unreadable_point(dir.path(), "unreadable.3dm");

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
            .stderr(predicate::str::contains("the decode's strictness refuses"));

        let value: serde_json::Value =
            cadmpeg_core::serde_erase::json::from_slice(&fs::read(&report).unwrap()).unwrap();
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
                loss["code"]["value"]["namespace"] == "rhino"
                    && loss["code"]["value"]["code"] == "object.framing-undecodable"
            }));
    }
}

#[cfg(all(feature = "creo", feature = "step"))]
#[test]
fn convert_writes_when_native_decode_has_no_geometry() {
    let dir = tempdir().unwrap();
    let input = geometryless_creo(dir.path(), "empty.prt");
    let report = dir.path().join("empty-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("ISO-10303-21"));
    let report: serde_json::Value =
        cadmpeg_core::serde_erase::json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert!(report["export"]["losses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|loss| { loss["code"]["value"]["code"] == "output.empty" }));
}

/// A container-only document converts to STEP: the writer writes what the
/// document holds, charges its empty-output loss, and notes the depth it plans
/// from.
#[cfg(all(feature = "creo", feature = "step"))]
#[test]
fn convert_writes_a_container_only_document_to_step() {
    let dir = tempdir().unwrap();
    let input = geometryless_creo(dir.path(), "empty.prt");
    let output = dir.path().join("empty.step");
    let report = dir.path().join("depth.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--container-only",
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(fs::read(&output).unwrap().starts_with(b"ISO-10303-21"));
    let value: serde_json::Value =
        cadmpeg_core::serde_erase::json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert!(value["export"]["losses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|loss| loss["code"]["value"]["code"] == "output.empty"));
    assert!(value["export"]["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note.as_str().unwrap().contains(
            "the document records container depth; the step writer plans from geometry"
        )));
}

/// A container-only document converts to IGES: the writer writes a file that
/// holds no entity and charges `export:output.empty`, which `--reject-empty`
/// refuses before any byte is written.
#[cfg(all(feature = "creo", feature = "iges"))]
#[test]
fn convert_writes_a_container_only_document_to_iges_and_reject_empty_refuses_it() {
    let dir = tempdir().unwrap();
    let input = geometryless_creo(dir.path(), "empty.prt");
    let run = |output: &std::path::Path, report: &std::path::Path, reject_empty: bool| {
        let mut command = Command::cargo_bin("cadmpeg").unwrap();
        command.args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--container-only",
            "--report",
            report.to_str().unwrap(),
        ]);
        if reject_empty {
            command.arg("--reject-empty");
        }
        command.assert()
    };

    let output = dir.path().join("empty.igs");
    let report = dir.path().join("written.json");
    run(&output, &report, false).success();
    assert!(!fs::read(&output).unwrap().is_empty());
    let value: serde_json::Value =
        cadmpeg_core::serde_erase::json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert!(value["export"]["losses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|loss| loss["code"]["value"]["namespace"] == "export"
            && loss["code"]["value"]["code"] == "output.empty"));

    let refused = dir.path().join("refused.igs");
    let report = dir.path().join("refused.json");
    run(&refused, &report, true).code(1);
    assert!(!refused.exists());
    let value: serde_json::Value =
        cadmpeg_core::serde_erase::json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(value["refusal"]["code"], "strict_export_rejected");
}

/// A same-format container-only F3Z conversion replays the archive, whose
/// members the decode kept undecoded, and reports the carried bytes.
#[cfg(feature = "f3d")]
#[test]
fn convert_replays_a_container_only_f3z_and_reports_the_carried_source() {
    let dir = tempdir().unwrap();
    let source = include_bytes!("../../../../corpus/f3d/F30a.f3z");
    let input = dir.path().join("source.f3z");
    fs::write(&input, source).unwrap();
    let output = dir.path().join("written.f3z");
    let report = dir.path().join("report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--to",
            "f3d",
            "--container-only",
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(fs::read(&output).unwrap(), source.as_slice());
    let value: serde_json::Value =
        cadmpeg_core::serde_erase::json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(value["export"]["write_path"]["kind"], "verbatim_replay");
    assert!(value["export"]["losses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|loss| loss["code"]["value"]["namespace"] == "export"
            && loss["code"]["value"]["code"] == "source.carried"
            && loss["severity"] == "info"));
}

/// The format half of `--to` is checked before the input is opened.
///
/// A format this build cannot write is wrong whatever the source turns out to
/// be, so it fails against an absent path as a typed plan refusal. A dialect
/// token is admitted by the selected encoder's catalog at the same static
/// boundary.
#[cfg(all(feature = "catia", feature = "iges", feature = "step"))]
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
    let value: serde_json::Value =
        cadmpeg_core::serde_erase::json::from_slice(&fs::read(report).unwrap()).unwrap();
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
#[cfg(all(feature = "iges", feature = "step"))]
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
    let value: serde_json::Value =
        cadmpeg_core::serde_erase::json::from_slice(&fs::read(&report).unwrap()).unwrap();
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
#[cfg(feature = "rhino")]
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
/// exit 1 rather than 2: `strict_decode_rejected` and `strict_export_rejected`. A file that loses nothing is written under every
/// scope, which is what proves the scope is a predicate and not a switch that
/// refuses on sight.
#[cfg(all(feature = "creo", feature = "step"))]
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
            .stderr(predicate::str::contains("the decode's strictness refuses"));
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
            predicate::str::contains("the write's strictness refuses")
                .and(predicate::str::contains("the decode's strictness").not()),
        );
    let report: serde_json::Value =
        cadmpeg_core::serde_erase::json::from_slice(&fs::read(export_report).unwrap()).unwrap();
    assert_eq!(report["refusal"]["code"], "strict_export_rejected");
    assert_eq!(report["refusal"]["stage"], "export");
    assert!(report["export"].is_object());
    assert_eq!(report["export"]["identity"]["kind"], "native");
    // The decode's one native record is a `creo` `unknowns` stub, which
    // incurs no omission loss (Rule W3).
    let losses = report["export"]["losses"].as_array().unwrap();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0]["code"]["value"]["code"], "output.empty");

    let lossless = fixture(dir.path(), "cube.cadir.json", &{
        let mut cube = unit_cube().expect("unit cube fixture is admitted");
        cadmpeg_ir::examples::add_document_root(&mut cube);
        cube
    });
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

/// A `convert` that names no target takes the catalog default where the
/// document's source is of another format or absent. That loses nothing, so
/// `--reject-lossy` at floor Warning writes the file, and the report charges
/// no `target.source-dialect-displaced`.
#[cfg(feature = "step")]
#[test]
fn a_cross_format_convert_that_names_no_target_is_not_refused() {
    let dir = tempdir().unwrap();
    let foreign = {
        let mut cube = unit_cube().expect("unit cube fixture is admitted");
        cadmpeg_ir::examples::add_document_root(&mut cube);
        cube.source = Some(cadmpeg_ir::SourceMeta::classified(
            cadmpeg_core::dialect::DialectLayers::of(
                cadmpeg_core::dialect::DialectMatch::admitted(cadmpeg_core::dialect_id!(
                    "iges:5.3-fixed-ascii"
                )),
            ),
            std::collections::BTreeMap::new(),
        ));
        cube
    };
    let source_less = {
        let mut cube = unit_cube().expect("unit cube fixture is admitted");
        cadmpeg_ir::examples::add_document_root(&mut cube);
        cube.source = None;
        cube
    };
    for (name, ir) in [("foreign", foreign), ("source-less", source_less)] {
        let input = fixture(dir.path(), &format!("{name}.cadir.json"), &ir);
        let output = dir.path().join(format!("{name}.step"));
        let report = dir.path().join(format!("{name}.json"));
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                input.to_str().unwrap(),
                "-o",
                output.to_str().unwrap(),
                "--reject-lossy=export",
                "--reject-lossy-floor=warning",
                "--report",
                report.to_str().unwrap(),
            ])
            .assert()
            .success();
        let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(
            report["export"]["identity"]["target"], "step:ap214",
            "{name}"
        );
        let losses = report["export"]["losses"].as_array().unwrap();
        assert!(
            losses
                .iter()
                .all(|loss| loss["code"]["value"]["code"] != "target.source-dialect-displaced"),
            "{name}: {losses:?}"
        );
    }
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
        &cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted"),
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

/// `--reject-lossy=decode` judges the report of a CADIR read: a read that
/// repairs a spelling is refused under it, and written with the repair
/// reported without it.
#[test]
fn reject_lossy_decode_judges_the_cadir_read_report() {
    let dir = tempdir().unwrap();
    let mut value = cadmpeg_core::serde_erase::json::to_value(
        unit_cube().expect("unit cube fixture is admitted"),
    )
    .expect("the unit cube writes");
    value["geometry"]["points"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    let input = dir.path().join("unsorted.cadir.json");
    fs::write(&input, value.to_string()).unwrap();
    let output = dir.path().join("out.cadir.json");
    let args = [
        "convert",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
    ];

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(args)
        .arg("--reject-lossy=decode")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "the read's strictness refuses read/spelling.normalized",
        ));
    assert!(!output.exists());

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(args)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "warning: read/spelling.normalized: geometry.points",
        ));
}
