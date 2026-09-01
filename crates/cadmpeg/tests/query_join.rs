// SPDX-License-Identifier: Apache-2.0
//! `cadmpeg query join` integration tests.

#![allow(clippy::unwrap_used)]

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn cadmpeg() -> Command {
    Command::cargo_bin("cadmpeg").unwrap()
}

fn write(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

const CHECK_REPORT: &str = r#"{
        "schema_version": 8,
  "command": "check",
  "status": "ok",
  "refusal": null
}"#;

const JOIN_DOC: &str = r#"{
  "ir_version": "4",
  "model": {
    "features": [
      {"id": "f1", "native_ref": "n1"},
      {"id": "f2", "native_ref": "missing"}
    ]
  },
  "native": {
    "rhino": {
      "arenas": {
        "unknowns": [
          {"id": "n1", "kind": "curve"},
          {"id": "n2", "kind": "other"}
        ]
      }
    }
  }
}"#;

const RIGHT_DOC: &str = r#"{
  "ir_version": "4",
  "native": {
    "rhino": {
      "arenas": {
        "unknowns": [
          {"id": "other-id", "name": "n1"}
        ]
      }
    }
  }
}"#;

#[test]
fn join_help_mentions_left_key() {
    cadmpeg()
        .args(["query", "join", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("--left-key")
                .and(predicate::str::contains("--right-key"))
                .and(predicate::str::contains("--mode"))
                .and(predicate::str::contains("--right-file")),
        );
}

#[test]
fn join_unknown_arena_teaches() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", JOIN_DOC);
    let missing = cadmpeg()
        .args([
            "query",
            "join",
            doc.to_str().unwrap(),
            "model.bogus",
            "native.rhino.unknowns",
            "--left-key",
            "native_ref",
            "--right-key",
            "id",
        ])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(stderr.contains("unknown arena model.bogus"), "{stderr}");
    assert!(stderr.contains("addressable arenas"), "{stderr}");
}

#[test]
fn join_rejects_report() {
    let dir = tempdir().unwrap();
    let report = write(dir.path(), "report.json", CHECK_REPORT);
    cadmpeg()
        .args([
            "query",
            "join",
            report.to_str().unwrap(),
            "model.features",
            "native.rhino.unknowns",
            "--left-key",
            "native_ref",
            "--right-key",
            "id",
        ])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("command report").and(predicate::str::contains("dump SOURCE")),
        );
}

#[test]
fn join_json_envelope_matched_native_ref() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", JOIN_DOC);
    let output = cadmpeg()
        .args([
            "query",
            "join",
            "--json",
            doc.to_str().unwrap(),
            "model.features",
            "native.rhino.unknowns",
            "--left-key",
            "native_ref",
            "--right-key",
            "id",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 8);
    assert_eq!(value["command"], "query join");
    let rows = value["join"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["key"], "n1");
    assert_eq!(rows[0]["left"]["id"], "f1");
    assert_eq!(rows[0]["right"]["id"], "n1");
    assert_eq!(rows[0]["left_arena"], "model.features");
    assert_eq!(rows[0]["right_arena"], "native.rhino.unknowns");
}

#[test]
fn join_unmatched_and_right_file() {
    let dir = tempdir().unwrap();
    let left = write(dir.path(), "left.json", JOIN_DOC);
    let right = write(dir.path(), "right.json", RIGHT_DOC);

    let unmatched = cadmpeg()
        .args([
            "query",
            "join",
            "--json",
            left.to_str().unwrap(),
            "model.features",
            "native.rhino.unknowns",
            "--left-key",
            "native_ref",
            "--right-key",
            "id",
            "--mode",
            "unmatched",
        ])
        .output()
        .unwrap();
    assert!(unmatched.status.success());
    let value: serde_json::Value = serde_json::from_slice(&unmatched.stdout).unwrap();
    let rows = value["join"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["left"]["id"], "f2");
    assert_eq!(rows[0]["right"], serde_json::Value::Null);

    let cross = cadmpeg()
        .args([
            "query",
            "join",
            "--json",
            left.to_str().unwrap(),
            "model.features",
            "native.rhino.unknowns",
            "--left-key",
            "native_ref",
            "--right-key",
            "name",
            "--right-file",
            right.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        cross.status.success(),
        "{}",
        String::from_utf8_lossy(&cross.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&cross.stdout).unwrap();
    let rows = value["join"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["key"], "n1");
    assert_eq!(rows[0]["left"]["id"], "f1");
    assert_eq!(rows[0]["right"]["id"], "other-id");
    assert_eq!(rows[0]["left_file"], left.to_str().unwrap());
    assert_eq!(rows[0]["right_file"], right.to_str().unwrap());
}

#[test]
fn join_requires_keys() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", JOIN_DOC);
    cadmpeg()
        .args([
            "query",
            "join",
            doc.to_str().unwrap(),
            "model.features",
            "native.rhino.unknowns",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("left-key"));
}
