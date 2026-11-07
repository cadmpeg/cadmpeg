// SPDX-License-Identifier: Apache-2.0
//! A stale sidecar cannot override edits to a CADIR document.

use std::fs;

use assert_cmd::Command;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::{decode_sidecar_path, CadIr, DecodeSidecar};
use serde_json::Value;
use tempfile::tempdir;

use super::{decode, workspace};

#[test]
fn edited_freecad_with_stale_sidecar_synthesizes_like_no_sidecar() {
    let dir = tempdir().unwrap();
    let source = workspace().join("corpus/freecad_fcstd/fixtures/design_history.FCStd");
    let cadir = dir.path().join("model.cadir.json");
    let output = dir.path().join("model.FCStd");
    let report = dir.path().join("report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "dump",
            source.to_str().unwrap(),
            "-o",
            cadir.to_str().unwrap(),
        ])
        .assert()
        .success();
    let mut document = CadIr::from_json(&fs::read_to_string(&cadir).unwrap()).unwrap();
    assert_eq!(document.scope, cadmpeg_ir::document::ContentScope::full());
    let feature = document
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id.as_str().ends_with("#Pad"))
        .unwrap();
    feature.name = Some("EditedPad".into());
    fs::write(&cadir, document.to_canonical_json().unwrap()).unwrap();
    let arguments = [
        "convert",
        cadir.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ];
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(arguments)
        .assert()
        .success();
    let with_stale_sidecar = fs::read(&output).unwrap();
    let stale_report: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(stale_report["export"]["write_path"]["path"], "synthesized");
    let losses = stale_report["decode_report"]["losses"].as_array().unwrap();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0]["code"]["code"], "sidecar.stale");
    assert_eq!(losses[0]["severity"], "warning");
    let sidecar_path = decode_sidecar_path(&cadir).unwrap();
    let original_sidecar =
        DecodeSidecar::from_json(&fs::read_to_string(&sidecar_path).unwrap()).unwrap();
    fs::remove_file(&sidecar_path).unwrap();
    let output_without = dir.path().join("without.FCStd");
    let report_without = dir.path().join("without.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            cadir.to_str().unwrap(),
            "-o",
            output_without.to_str().unwrap(),
            "--report",
            report_without.to_str().unwrap(),
        ])
        .assert()
        .success();
    let without_sidecar = fs::read(&output_without).unwrap();
    let no_sidecar_report: Value =
        serde_json::from_slice(&fs::read(&report_without).unwrap()).unwrap();
    assert_eq!(
        no_sidecar_report["export"]["write_path"]["path"],
        "synthesized"
    );
    assert_eq!(with_stale_sidecar, without_sidecar);
    let written = decode(&cadmpeg_codec_freecad::FcstdCodec, &without_sidecar);
    assert!(written
        .ir()
        .model
        .features
        .iter()
        .any(|feature| feature.name.as_deref() == Some("EditedPad")));

    let edited = fs::read(&cadir).unwrap();
    let rebound = DecodeSidecar::bind_sha256(
        Sha256Digest::digest(&edited),
        original_sidecar.report,
        original_sidecar.fidelity,
    );
    fs::write(&sidecar_path, rebound.to_canonical_json().unwrap()).unwrap();
    let restored_output = dir.path().join("restored.FCStd");
    let restored_report = dir.path().join("restored.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            cadir.to_str().unwrap(),
            "-o",
            restored_output.to_str().unwrap(),
            "--report",
            restored_report.to_str().unwrap(),
            "--reject-lossy=decode",
        ])
        .assert()
        .success();
    let restored_report: Value =
        serde_json::from_slice(&fs::read(restored_report).unwrap()).unwrap();
    assert_eq!(
        restored_report["export"]["write_path"]["path"],
        "synthesized"
    );
    let restored = decode(
        &cadmpeg_codec_freecad::FcstdCodec,
        &fs::read(restored_output).unwrap(),
    );
    assert!(restored
        .ir()
        .model
        .features
        .iter()
        .any(|feature| feature.name.as_deref() == Some("EditedPad")));

    let export: cadmpeg_ir::report::export::ExportReport =
        serde_json::from_value(restored_report["export"].clone()).unwrap();
    for (floor, severity) in [
        ("info", cadmpeg_ir::report::Severity::Info),
        ("error", cadmpeg_ir::report::Severity::Error),
    ] {
        let output = dir.path().join(format!("{floor}.FCStd"));
        let result = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                cadir.to_str().unwrap(),
                "-o",
                output.to_str().unwrap(),
                "--reject-lossy",
                "--reject-lossy-floor",
                floor,
            ])
            .assert();
        if export.losses.iter().any(|loss| loss.severity >= severity) {
            result.code(1);
        } else {
            result.success();
        }
    }
}
