// SPDX-License-Identifier: Apache-2.0
//! `cadmpeg query fidelity` integration tests.

#![allow(clippy::unwrap_used)]

use std::fs;

use predicates::prelude::predicate;
use tempfile::tempdir;

mod query_support;
mod support;

use crate::query_support::write;
use crate::support::cadmpeg;

const CADIR_DOC: &str = r#"{
  "ir_version": "10",
  "model": {"faces": [{"id": "f1"}, {"id": "f2"}], "edges": []},
  "native": {"fcstd": {"objects": [1, 2, 3]}}
}"#;

const FIDELITY_SIDECAR: &str = r#"{
  "ir_version": "10",
  "ir_sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
  "report": {"identity": {"classification": "unclassified", "format": "f3d"},
             "transfer": {"scope": {"model": true, "design": true}, "geometry_transferred": true},
             "coverage": {}, "losses": [], "notes": []},
  "fidelity": {
    "decode_digest": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    "annotations": {"provenance": {"a:b:c#1": {"stream": "Contents/Config-0", "offset": 0}},
                    "exactness": {}},
    "retained_records": {
      "f3d:source:record#r1": {"stream": "Contents/Config-0", "offset": 0,
             "bytes": {"retention": "inline", "data": "QUJDRA=="}, "untyped": []},
      "f3d:source:record#r2": {"stream": "Contents/Config-0", "offset": 4,
             "bytes": {"retention": "inline", "data": "RUY="}, "untyped": []},
      "f3d:source:record#r3": {"stream": "Other", "offset": 0,
             "bytes": {"retention": "digest", "byte_len": 3, "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"},
             "untyped": []}
    }
  }
}"#;

#[test]
fn fidelity_lists_retained_records_with_annotation_counts() {
    let dir = tempdir().unwrap();
    let sidecar = write(dir.path(), "m.fidelity.json", FIDELITY_SIDECAR);
    cadmpeg()
        .args(["query", "fidelity", sidecar.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            "stream\toffset\tbytes\tdata\tid\n\
             Contents/Config-0\t0\t4\tyes\tf3d:source:record#r1\n\
             Contents/Config-0\t4\t2\tyes\tf3d:source:record#r2\n\
             Other\t0\t3\tno\tf3d:source:record#r3\n",
        )
        .stderr(predicate::str::contains(
            "annotations: 1 streams, 1 provenance entries, 0 exactness notes",
        ));
}

#[test]
fn fidelity_extracts_a_contiguous_stream_byte_exactly() {
    let dir = tempdir().unwrap();
    let sidecar = write(dir.path(), "m.fidelity.json", FIDELITY_SIDECAR);
    let out = dir.path().join("out.bin");

    cadmpeg()
        .args([
            "query",
            "fidelity",
            sidecar.to_str().unwrap(),
            "--stream",
            "Contents/Config-0",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("wrote 6 bytes from 2 record(s)"));
    assert_eq!(fs::read(&out).unwrap(), b"ABCDEF");

    let stdout = cadmpeg()
        .args([
            "query",
            "fidelity",
            sidecar.to_str().unwrap(),
            "--stream",
            "Contents/Config-0",
            "--binary-stdout",
        ])
        .output()
        .unwrap();
    assert!(stdout.status.success());
    assert_eq!(stdout.stdout, b"ABCDEF");
}

#[test]
fn fidelity_refuses_to_overwrite_without_force() {
    let dir = tempdir().unwrap();
    let sidecar = write(dir.path(), "m.fidelity.json", FIDELITY_SIDECAR);
    let out = dir.path().join("existing.bin");
    fs::write(&out, b"precious").unwrap();

    cadmpeg()
        .args([
            "query",
            "fidelity",
            sidecar.to_str().unwrap(),
            "--stream",
            "Contents/Config-0",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "exists; pass --force to overwrite",
        ));
    assert_eq!(fs::read(&out).unwrap(), b"precious");

    cadmpeg()
        .args([
            "query",
            "fidelity",
            sidecar.to_str().unwrap(),
            "--stream",
            "Contents/Config-0",
            "-o",
            out.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();
    assert_eq!(fs::read(&out).unwrap(), b"ABCDEF");
}

#[test]
fn fidelity_extraction_teaches_on_every_failure_shape() {
    let dir = tempdir().unwrap();
    let sidecar = write(dir.path(), "m.fidelity.json", FIDELITY_SIDECAR);
    let path = sidecar.to_str().unwrap();

    // No -o and no --binary-stdout: the house binary-stdout guard.
    let guard = cadmpeg()
        .args(["query", "fidelity", path, "--stream", "Contents/Config-0"])
        .output()
        .unwrap();
    assert_eq!(guard.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&guard.stderr).contains("--binary-stdout"));

    // Unknown stream lists the real ones.
    let miss = cadmpeg()
        .args([
            "query",
            "fidelity",
            path,
            "--stream",
            "Nope",
            "--binary-stdout",
        ])
        .output()
        .unwrap();
    assert_eq!(miss.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&miss.stderr);
    assert!(stderr.contains("Contents/Config-0"), "{stderr}");

    // Extent-only retention is a named refusal, not a silent empty file.
    let nodata = cadmpeg()
        .args([
            "query",
            "fidelity",
            path,
            "--stream",
            "Other",
            "--binary-stdout",
        ])
        .output()
        .unwrap();
    assert_eq!(nodata.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&nodata.stderr).contains("without bytes"));

    // A gap between extents refuses the concat instead of splicing bytes.
    let gapped = FIDELITY_SIDECAR.replace("\"offset\": 4", "\"offset\": 8");
    let gap_file = write(dir.path(), "gap.fidelity.json", &gapped);
    let gap = cadmpeg()
        .args([
            "query",
            "fidelity",
            gap_file.to_str().unwrap(),
            "--stream",
            "Contents/Config-0",
            "--binary-stdout",
        ])
        .output()
        .unwrap();
    assert_eq!(gap.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&gap.stderr).contains("not contiguous"));
}

#[test]
fn fidelity_rejects_non_sidecar_kinds_and_wraps_json() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.cadir.json", CADIR_DOC);
    let cadir = cadmpeg()
        .args(["query", "fidelity", doc.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(cadir.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&cadir.stderr).contains(".fidelity.json"));

    let sidecar = write(dir.path(), "m.fidelity.json", FIDELITY_SIDECAR);
    let json = cadmpeg()
        .args(["query", "fidelity", "--json", sidecar.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(json.status.success());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["command"], "query");
    assert_eq!(value["payload"]["annotations"]["provenance"], 1);
    assert_eq!(
        value["payload"]["retained_records"][2]["data_retained"],
        false
    );
}
