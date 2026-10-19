// SPDX-License-Identifier: Apache-2.0
//! Native CATIA loss events become history after a lossless CADIR parse.

use std::fs;

use assert_cmd::Command;
use cadmpeg_ir::{decode_sidecar_path, CadIr, DecodeSidecar};
use tempfile::tempdir;

#[test]
fn catia_a8_parameter_maps_cadir_decode_policy_is_independent_of_history_sidecar() {
    let dir = tempdir().unwrap();
    // The historical frozen fixture is absent from this checkout. Author a
    // fresh parameter-map input from the codec's existing public grammar.
    let native = dir.path().join("standard_a8_parameter_maps.catpart");
    fs::write(&native, authored_a8_parameter_maps_catpart()).unwrap();
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

// Authored test input from the existing public CATIA synthetic grammar in
// src/test_support/test_a5a8.rs and test_container.rs. This is a fresh input,
// not a copy of the absent historical standard_a8_parameter_maps.catpart.
fn authored_a8_parameter_maps_catpart() -> Vec<u8> {
    fn descriptor(name: &str, offset: u32, length: u32) -> Vec<u8> {
        let mut bytes = vec![0_u8; 0x54];
        bytes[0x0c..0x10].copy_from_slice(&length.to_be_bytes());
        for (ordinal, character) in name.encode_utf16().enumerate() {
            let at = 0x10 + ordinal * 2;
            bytes[at..at + 2].copy_from_slice(&character.to_le_bytes());
        }
        bytes[0x50..0x54].copy_from_slice(&1_u32.to_be_bytes());
        for value in [offset, length, length, 0, 0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    let mut payload = vec![0, 9, 0, 0, 9, 1];
    for value in [0.0_f64, 1.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[13, 13, 9, 0, 0, 9, 1]);
    for value in [0.0_f64, 1.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[13, 13, 1]);
    for pole in 0..9_u32 {
        for value in [f64::from(pole), f64::from(pole / 3), f64::from(pole % 3)] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
    }
    let mut tail = vec![0_u8; 141];
    tail[..4].copy_from_slice(&[0x05, 0x21, 0x05, 0x05]);
    // U/V domains, then a nonidentity affine map for each parameter.
    for (offset, value) in [
        (4, 0.0_f64),
        (12, 1.0),
        (20, 0.0),
        (28, 1.0),
        (36, 2.0),
        (44, 3.0),
        (52, -4.0),
        (60, 5.0),
    ] {
        tail[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    tail[68..71].copy_from_slice(&[1, 1, 1]);
    tail[135..141].copy_from_slice(&[1, 0, 1, 0, 7, 7]);
    payload.extend_from_slice(&tail);
    let mut main = vec![0xa8, 0x03, 0x34];
    main.extend_from_slice(
        &u32::try_from(payload.len())
            .expect("small fixture")
            .to_le_bytes(),
    );
    main.extend_from_slice(&0xdeca_fbad_u32.to_le_bytes());
    main.extend_from_slice(&payload);
    let surf = [0_u8];
    let main_len = u32::try_from(main.len()).expect("small fixture");
    let surf_offset = 16 + main_len;
    let directory_offset = surf_offset + 1;
    let mut directory = b"CATIA_V5 CB0001\0".to_vec();
    directory.extend_from_slice(&descriptor("MainDataStream", 16, main_len));
    directory.extend_from_slice(&descriptor("SurfacicReps", surf_offset, 1));
    directory.extend_from_slice(b"CB__END");
    let mut inner = b"V5_CFV2\0".to_vec();
    inner.extend_from_slice(&directory_offset.to_be_bytes());
    inner.extend_from_slice(
        &u32::try_from(directory.len())
            .expect("small fixture")
            .to_be_bytes(),
    );
    inner.extend_from_slice(&main);
    inner.extend_from_slice(&surf);
    inner.extend_from_slice(&directory);
    let mut file = b"V5_CFV2\0".to_vec();
    file.extend_from_slice(
        &(16 + u32::try_from(inner.len()).expect("small fixture")).to_be_bytes(),
    );
    file.extend_from_slice(&0_u32.to_be_bytes());
    file.extend_from_slice(&inner);
    file
}
