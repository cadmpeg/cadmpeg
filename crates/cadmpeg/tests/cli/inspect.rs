// SPDX-License-Identifier: Apache-2.0
//! Inspect and dump: detection, forced readers, and container listings.

use std::fs;

use assert_cmd::Command;
use cadmpeg_ir::examples::unit_cube;
use predicates::prelude::*;
use tempfile::tempdir;

use crate::support::*;

#[test]
fn fcstd_inspect_and_container_decode_work_automatically_and_forced() {
    let dir = tempdir().unwrap();
    let input = minimal_fcstd(dir.path(), "document.FCStd");

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", input.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("format: fcstd (detected high)")
                .and(predicate::str::contains("SchemaVersion=4")),
        );

    for forced in [false, true] {
        let mut command = Command::cargo_bin("cadmpeg").unwrap();
        command.args(["dump", input.to_str().unwrap(), "--container-only"]);
        if forced {
            command.args(["--input-format", "fcstd"]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["source"]["format"], "fcstd");
        assert_eq!(value["source"]["attributes"]["schema_version"], "4");
    }
}

#[test]
fn iges_dump_report_classifies_codec_refusal() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("malformed.igs");
    let report = dir.path().join("decode-report.json");
    fs::write(&input, b"not an IGES file").unwrap();

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "dump",
            input.to_str().unwrap(),
            "--input-format",
            "iges",
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("decode failed"));

    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 7);
    assert_eq!(value["command"], "dump");
    assert_eq!(value["status"], "refused");
    assert_eq!(value["refusal"]["stage"], "decode");
    assert_eq!(value["refusal"]["code"], "decode_failed");
    assert!(value["refusal"]["message"]
        .as_str()
        .unwrap()
        .contains("IGES"));
}

#[test]
fn rhino_inspect_detects_archive_and_reports_tables_in_text_and_json() {
    let dir = tempdir().unwrap();
    let input = minimal_rhino_archive(dir.path(), "empty.3dm", "50");

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", input.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("format: rhino (detected high)")
                .and(predicate::str::contains("container: 3dm-chunks"))
                .and(predicate::str::contains("entries: 3"))
                .and(predicate::str::contains("table-0x10000014"))
                .and(predicate::str::contains("table-0x10000015"))
                .and(predicate::str::contains("table-0x10000013"))
                .and(predicate::str::contains("archive version 50")),
        );

    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 7);
    assert_eq!(value["command"], "inspect");
    assert_eq!(value["confidence"], "high");
    assert_eq!(value["summary"]["format"], "rhino");
    assert_eq!(value["summary"]["container_kind"], "3dm-chunks");
    assert_eq!(value["summary"]["entries"].as_array().unwrap().len(), 3);
    assert_eq!(value["summary"]["notes"][0], "archive version 50");
}

#[test]
fn rhino_forced_input_format_and_3dm_alias_bypass_detection() {
    let dir = tempdir().unwrap();
    let input = minimal_rhino_archive(dir.path(), "extensionless", "50");

    for input_format in ["rhino", "3dm"] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "inspect",
                input.to_str().unwrap(),
                "--input-format",
                input_format,
            ])
            .assert()
            .success()
            .stdout(
                predicate::str::contains("format: rhino (forced)")
                    .and(predicate::str::contains("container: 3dm-chunks")),
            );

        let output = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "dump",
                input.to_str().unwrap(),
                "--input-format",
                input_format,
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ir_version"], cadmpeg_ir::IR_VERSION);
        assert_eq!(value["source"]["format"], "rhino");
    }
}

#[test]
fn rhino_full_band_empty_archive_decodes_to_current_ir() {
    let dir = tempdir().unwrap();
    for version in ["50", "60", "70", "80"] {
        let input = minimal_rhino_archive(dir.path(), &format!("empty-{version}.3dm"), version);
        for extra in [None, Some("--container-only")] {
            let mut command = Command::cargo_bin("cadmpeg").unwrap();
            command.args(["dump", input.to_str().unwrap()]);
            if let Some(argument) = extra {
                command.arg(argument);
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "archive {version}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["ir_version"], cadmpeg_ir::IR_VERSION);
            assert_eq!(value["source"]["format"], "rhino");
            assert_eq!(value["source"]["attributes"]["archive_version"], version);
            assert_eq!(
                value["source"]["attributes"]["container_kind"],
                "3dm-chunks"
            );
            assert_eq!(value["model"]["subds"], serde_json::json!([]));
        }
    }
}

#[test]
fn rhino_point_archive_inspect_decode_and_validate_expose_geometry() {
    let dir = tempdir().unwrap();
    let input = synthetic_rhino_point(dir.path(), "point.3dm", [1.25, -2.5, 3.75]);

    let inspect = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(inspect.status.success());
    let summary: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(
        summary["summary"]["entries"][2]["attributes"]["record_count"],
        "1"
    );

    let decoded = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dump", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(decoded.status.success());
    let ir: serde_json::Value = serde_json::from_slice(&decoded.stdout).unwrap();
    assert_eq!(ir["model"]["points"][0]["position"]["x"], 1.25);
    assert_eq!(ir["model"]["points"][0]["position"]["y"], -2.5);
    assert_eq!(ir["model"]["points"][0]["position"]["z"], 3.75);
    let body_id = ir["model"]["bodies"][0]["id"].as_str().unwrap();
    assert!(ir["native"]["rhino"]["arenas"]["unknowns"][0]["links"]
        .as_array()
        .unwrap()
        .iter()
        .any(|link| link == body_id));

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", input.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("check: OK"));
}

#[test]
fn rhino_v1_to_v4_decode_metadata_but_legacy_v5_is_header_only() {
    let dir = tempdir().unwrap();
    for version in ["1", "2", "3", "4"] {
        let input = minimal_rhino_archive(dir.path(), &format!("empty-{version}.3dm"), version);
        let output = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(["dump", input.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(output.status.success());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ir_version"], cadmpeg_ir::IR_VERSION);
        assert_eq!(value["source"]["attributes"]["archive_version"], version);
        assert_eq!(value["model"]["subds"], serde_json::json!([]));
    }

    let input = dir.path().join("header-5.3dm");
    fs::write(&input, rhino_header("5")).unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", input.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("container: 3dm-chunks")
                .and(predicate::str::contains("entries: 0"))
                .and(predicate::str::contains("archive version 5")),
        );
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dump", input.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "Rhino archive version 5 decode is not implemented",
        ));
}

#[test]
fn rhino_cli_rejects_truncated_and_malformed_archives_with_context() {
    let dir = tempdir().unwrap();
    let truncated = dir.path().join("truncated.3dm");
    let mut bytes = rhino_header("50");
    bytes.extend([1, 0, 0]);
    fs::write(&truncated, bytes).unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", truncated.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("inspecting").and(predicate::str::contains("truncated")));

    let malformed = minimal_rhino_archive(dir.path(), "malformed.3dm", "50");
    let mut bytes = fs::read(&malformed).unwrap();
    bytes.truncate(bytes.len() - 20);
    fs::write(&malformed, bytes).unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dump", malformed.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("decoding")
                .and(predicate::str::contains("missing end-of-file chunk")),
        );
}

#[test]
fn inspect_garbage_reports_rhino_among_supported_formats() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("garbage.bin");
    fs::write(&input, b"not a CAD file").unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", input.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "supported: FCStd, f3d, Inventor IPT/IAM, sldprt, CATPart, NX/Creo prt, Rhino 3DM, IGES, STEP",
        ));
}

#[test]
fn cadir_override_bypasses_native_detection() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "no-extension", &unit_cube());
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", input.to_str().unwrap(), "--input-format", "cadir"])
        .assert()
        .success();
}

#[test]
fn input_flag_reaches_every_single_input_command() {
    let dir = tempdir().unwrap();
    let ir = unit_cube();
    let model = fixture(dir.path(), "cube.cadir.json", &ir);
    let path = model.to_str().unwrap();

    // Byte-identical stdout under either input spelling.
    for args in [
        vec!["dump", "--input-format", "cadir"],
        vec!["check"],
        vec!["convert", "-f", "step"],
    ] {
        let positional = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(&args)
            .arg(path)
            .output()
            .unwrap();
        let mut flagged = args.clone();
        flagged.push("--input");
        flagged.push(path);
        let via_flag = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(&flagged)
            .output()
            .unwrap();
        assert_eq!(positional.status.code(), via_flag.status.code(), "{args:?}");
        assert_eq!(positional.stdout, via_flag.stdout, "{args:?}");
    }

    // Both spellings at once are a clap conflict.
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", path, "--input", path])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}
