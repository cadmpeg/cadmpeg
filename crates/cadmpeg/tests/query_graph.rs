// SPDX-License-Identifier: Apache-2.0
//! `cadmpeg query graph` integration tests.

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

const SIDECAR: &str = r#"{
  "version": "1",
  "ir_sha256": "abc123"
}"#;

const BREP_DOC: &str = r#"{
  "ir_version": "4",
  "model": {
    "bodies": [{"id": "body#1", "regions": ["region#1"]}],
    "regions": [{"id": "region#1", "shells": ["shell#1"]}],
    "shells": [{"id": "shell#1", "faces": ["face#1", "face#2"]}],
    "faces": [{"id": "face#1"}, {"id": "face#2"}]
  }
}"#;

const GRAPH_DOC: &str = r#"{
  "ir_version": "4",
  "model": {
    "features": [
      {
        "id": "f1",
        "links": ["n1"],
        "native_ref": "n2",
        "definition": {"parameters": {"segment_0_object": "n1"}}
      }
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

#[test]
fn graph_help_mentions_hops_follow_and_reverse() {
    cadmpeg()
        .args(["query", "graph", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("--hops")
                .and(predicate::str::contains("--follow"))
                .and(predicate::str::contains("--reverse"))
                .and(predicate::str::contains("--max-paths")),
        );
}

#[test]
fn graph_unknown_arena_teaches() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", GRAPH_DOC);
    let missing = cadmpeg()
        .args(["query", "graph", doc.to_str().unwrap(), "model.bogus", "f1"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(stderr.contains("unknown arena model.bogus"), "{stderr}");
    assert!(stderr.contains("addressable arenas"), "{stderr}");
    assert!(stderr.contains("query counts"), "{stderr}");
}

#[test]
fn graph_rejects_report_and_sidecar() {
    let dir = tempdir().unwrap();
    let report = write(dir.path(), "report.json", CHECK_REPORT);
    cadmpeg()
        .args([
            "query",
            "graph",
            report.to_str().unwrap(),
            "model.faces",
            "f1",
        ])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("command report")
                .and(predicate::str::contains("query findings"))
                .and(predicate::str::contains("dump SOURCE")),
        );

    let sidecar = write(dir.path(), "model.fidelity.json", SIDECAR);
    cadmpeg()
        .args([
            "query",
            "graph",
            sidecar.to_str().unwrap(),
            "model.faces",
            "f1",
        ])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("fidelity.json").and(predicate::str::contains("dump SOURCE")),
        );
}

#[test]
fn graph_json_envelope() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", GRAPH_DOC);
    let output = cadmpeg()
        .args([
            "query",
            "graph",
            "--json",
            doc.to_str().unwrap(),
            "model.features",
            "f1",
            "--hops",
            "1",
            "--follow",
            "links",
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
    assert_eq!(value["command"], "query graph");
    let graph = value["graph"].as_array().unwrap();
    assert_eq!(graph.len(), 2);
    assert_eq!(graph[0]["start"], "model.features#f1");
    assert!(graph[0]["path"].as_array().unwrap().is_empty());
    assert_eq!(graph[1]["record"]["id"], "n1");
    assert_eq!(graph[1]["path"][0]["field"], "links");
}

#[test]
fn graph_brep_hops_3_reaches_faces() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "brep.json", BREP_DOC);
    let output = cadmpeg()
        .args([
            "query",
            "graph",
            "--json",
            doc.to_str().unwrap(),
            "model.bodies",
            "body#1",
            "--hops",
            "3",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let ids: Vec<&str> = value["graph"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["record"]["id"].as_str())
        .collect();
    assert!(ids.contains(&"body#1"), "{ids:?}");
    assert!(ids.contains(&"region#1"), "{ids:?}");
    assert!(ids.contains(&"shell#1"), "{ids:?}");
    assert!(ids.contains(&"face#1"), "{ids:?}");
    assert!(ids.contains(&"face#2"), "{ids:?}");
}

#[test]
fn graph_missing_id_teaches() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", GRAPH_DOC);
    let missing = cadmpeg()
        .args(["query", "graph", doc.to_str().unwrap(), "features", "nope"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(stderr.contains("no record in model.features"), "{stderr}");
    assert!(stderr.contains("f1"), "{stderr}");
}
