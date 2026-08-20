// SPDX-License-Identifier: Apache-2.0
//! Inferred `query schema` relation-column integration tests.

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

const REL_DOC: &str = r#"{
  "ir_version": "4",
  "model": {
    "features": [
      {
        "id": "f1",
        "native_ref": "n1",
        "links": ["n1"],
        "name": "plain"
      }
    ]
  },
  "native": {
    "rhino": {
      "arenas": {
        "unknowns": [
          {"id": "n1", "kind": "curve"}
        ]
      }
    }
  }
}"#;

#[test]
fn schema_relation_column_marks_ref_and_refs() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", REL_DOC);
    let path = doc.to_str().unwrap();

    cadmpeg()
        .args(["query", "schema", path, "model.features"])
        .assert()
        .success()
        .stdout(
            predicate::str::starts_with("path\tpresence\ttype\texample\trelation\n")
                .and(predicate::str::contains("id\t1/1\tstring\tf1\tid"))
                .and(predicate::str::contains("native_ref\t1/1\tstring\tn1\tref"))
                .and(predicate::str::contains(
                    "links\t1/1\tarray\t[\"n1\"]\trefs",
                ))
                .and(predicate::str::contains("name\t1/1\tstring\tplain\t")),
        )
        .stderr(predicate::str::contains(
            "relation=ref|refs paths are graph --follow fields and join keys",
        ));

    let json = cadmpeg()
        .args(["query", "schema", "--json", path, "model.features"])
        .output()
        .unwrap();
    assert!(json.status.success());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["command"], "query schema");
    let fields = value["schema"]["fields"].as_array().unwrap();
    let by_path: std::collections::BTreeMap<&str, &serde_json::Value> = fields
        .iter()
        .filter_map(|field| field["path"].as_str().map(|path| (path, field)))
        .collect();
    assert_eq!(by_path["id"]["relation"], "id");
    assert_eq!(by_path["native_ref"]["relation"], "ref");
    assert_eq!(by_path["links"]["relation"], "refs");
    assert_eq!(by_path["name"]["relation"], serde_json::Value::Null);
}
