// SPDX-License-Identifier: Apache-2.0
//! Native CATIA loss events become history after a lossless CADIR parse.

use std::fs;

use assert_cmd::Command;
use cadmpeg_ir::{decode_sidecar_path, CadIr, DecodeSidecar};
use tempfile::tempdir;

#[test]
fn catia_a8_parameter_maps_cadir_decode_policy_is_independent_of_history_sidecar() {
    let dir = tempdir().unwrap();
    let native = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cadmpeg-codec-catia/tests/golden/fixtures/standard_a8_parameter_maps.catpart");
    let cadir = dir.path().join("parameter-maps.cadir.json");
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
    let history = DecodeSidecar::from_json(&fs::read_to_string(&sidecar_path).unwrap()).unwrap();
    assert!(
        !history.report.losses.is_empty(),
        "the native decode must actually report losses"
    );
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            native.to_str().unwrap(),
            "-f",
            "cadir",
            "--reject-lossy=decode",
        ])
        .assert()
        .code(1);
    let expected = CadIr::from_json(&fs::read_to_string(&cadir).unwrap()).unwrap();
    for with_history in [true, false] {
        if !with_history {
            fs::remove_file(&sidecar_path).unwrap();
        }
        let report = dir
            .path()
            .join(format!("history-{with_history}.report.json"));
        let assertion = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                cadir.to_str().unwrap(),
                "-f",
                "cadir",
                "--reject-lossy=decode",
                "--report",
                report.to_str().unwrap(),
            ])
            .assert()
            .success();
        let output = assertion.get_output();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            stderr.contains("decode that produced this CADIR; the current CADIR parse is lossless"),
            with_history,
            "history must be printed exactly when an adjacent report was restored: {stderr}"
        );
        let written = CadIr::from_json(std::str::from_utf8(&output.stdout).unwrap()).unwrap();
        assert_eq!(written.scope, expected.scope);
        assert_eq!(
            serde_json::to_value(&written.model).unwrap(),
            serde_json::to_value(&expected.model).unwrap()
        );
        let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(report["status"], "ok");
        assert!(report["refusal"].is_null());
        assert_eq!(report["decode_report"].is_null(), !with_history);
    }
}
