// SPDX-License-Identifier: Apache-2.0
//! `cadmpeg query` integration tests.

#![allow(clippy::unwrap_used)]

use std::fs;

use assert_cmd::Command;
use cadmpeg_ir::examples::unit_cube;
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
  "schema_version": 7,
  "command": "check",
  "status": "ok",
  "refusal": null,
  "decode_report": null,
  "check_report": {
    "entity_counts": {"faces": 2, "edges": 12},
    "findings": [
      {"check": "identity", "severity": "error", "message": "duplicate id", "entity": "e1"},
      {"check": "units", "severity": "warning", "message": "non-canonical unit"}
    ],
    "losses": [
      {
        "code": {
          "namespace": "shared",
          "code": "topology_not_transferred",
          "kind": "topology_not_transferred"
        },
        "severity": "warning",
        "message": "wire dropped"
      }
    ]
  }
}"#;

const CADIR_DOC: &str = r#"{
  "ir_version": "4",
  "model": {"faces": [{"id": "f1"}, {"id": "f2"}], "edges": []},
  "native": {"fcstd": {"arenas": {"objects": [1, 2, 3]}}}
}"#;

const SIDECAR: &str = r#"{
  "version": "1",
  "ir_sha256": "abc123",
  "report": {
    "format": "f3d",
    "container_only": false,
    "geometry_transferred": true,
    "coverage": {"streams": 7, "segments": 3},
    "losses": [{"code": "metadata_not_transferred", "severity": "info", "message": "thumbnail"}]
  }
}"#;

#[test]
fn summary_detects_all_three_artifact_kinds() {
    let dir = tempdir().unwrap();
    for (name, content, kind) in [
        ("report.json", CHECK_REPORT, "command report"),
        ("model.cadir.json", CADIR_DOC, "CADIR document"),
        ("model.fidelity.json", SIDECAR, "decode sidecar"),
    ] {
        let path = write(dir.path(), name, content);
        cadmpeg()
            .args(["query", "summary", path.to_str().unwrap()])
            .assert()
            .success()
            .stdout(
                predicate::str::starts_with("field\tvalue\n")
                    .and(predicate::str::contains(format!("kind\t{kind}"))),
            );
    }
}

#[test]
fn summary_exposes_document_and_decode_dialect_identity() {
    let dir = tempdir().unwrap();
    let cadir = write(
        dir.path(),
        "classified.cadir.json",
        r#"{
          "ir_version": "4",
          "source": {
            "format": "rhino",
            "attributes": {},
            "dialects": {
              "primary": {
                "format": "rhino",
                "dialect": "rhino:archive-80",
                "declared": {"archive_version": "80"},
                "admission": "admitted"
              },
              "extra": []
            }
          },
          "model": {},
          "native": {}
        }"#,
    );
    cadmpeg()
        .args(["query", "summary", cadir.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("source_format\trhino")
                .and(predicate::str::contains("source_dialect_layers\t1"))
                .and(predicate::str::contains("source_dialect\trhino:archive-80"))
                .and(predicate::str::contains(
                    "source_dialect_admission\tadmitted",
                ))
                .and(predicate::str::contains(
                    "source_dialect_declared\t{\"archive_version\":\"80\"}",
                )),
        );

    let sidecar = write(
        dir.path(),
        "classified.fidelity.json",
        r#"{
          "version": "3",
          "ir_sha256": "abc123",
          "report": {
            "format": "f3d",
            "container_only": false,
            "geometry_transferred": true,
            "coverage": {},
            "losses": [],
            "dialects": {
              "primary": {
                "format": "f3d",
                "dialect": "f3d:archive-2",
                "declared": {"manifest_version": "2"},
                "admission": "admitted"
              },
              "extra": [{
                "format": "acis",
                "dialect": "acis:sab-22300",
                "admission": {"admitted_unverified": {"using": "acis:sab-22200"}},
                "instance": "member:model.sab"
              }]
            }
          }
        }"#,
    );
    cadmpeg()
        .args(["query", "summary", sidecar.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("decode_dialect_layers\t2")
                .and(predicate::str::contains("decode_dialects\t{\"primary\":"))
                .and(predicate::str::contains("decode_dialect\tf3d:archive-2"))
                .and(predicate::str::contains(
                    "decode_dialect_admission\tadmitted",
                ))
                .and(predicate::str::contains(
                    "decode_dialect_declared\t{\"manifest_version\":\"2\"}",
                )),
        );
}

#[test]
fn summary_exposes_inspect_export_and_refusal_identity_without_positional_layers() {
    let dir = tempdir().unwrap();
    let report = write(
        dir.path(),
        "identity.report.json",
        r#"{
          "schema_version": 7,
          "command": "convert",
          "status": "refused",
          "refusal": {
            "stage": "decode",
            "code": "unsupported_dialect",
            "message": "unsupported source",
            "dialects": {
              "primary": {
                "format": "rhino",
                "dialect": "rhino:unknown",
                "declared": {"archive_version": "100"},
                "admission": "refused"
              },
              "extra": []
            }
          },
          "summary": {
            "format": "rhino",
            "losses": [{
              "code": {"namespace": "rhino", "code": "source.dialect-unverified"},
              "severity": "warning",
              "message": "archive word is residual"
            }],
            "dialects": {
              "primary": {
                "format": "rhino",
                "dialect": "rhino:archive-80",
                "declared": {"archive_version": "80"},
                "admission": "admitted"
              },
              "extra": []
            }
          },
          "decode_report": null,
          "check_report": null,
          "export": {"format": "step", "target": "step:ap242-e3"}
        }"#,
    );

    let output = cadmpeg()
        .args(["query", "summary", report.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.status);
    let stdout = String::from_utf8(output.stdout).unwrap();
    for expected in [
        "refusal_dialects\t{\"primary\":",
        "refusal_dialect\trhino:unknown",
        "refusal_dialect_declared\t{\"archive_version\":\"100\"}",
        "inspect_dialects\t{\"primary\":",
        "inspect_dialect\trhino:archive-80",
        "inspect_dialect_declared\t{\"archive_version\":\"80\"}",
        "export_format\tstep",
        "export_target\tstep:ap242-e3",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?} in:\n{stdout}"
        );
    }
    assert!(!stdout.contains("_extra_"), "{stdout}");

    let json = cadmpeg()
        .args(["query", "summary", "--json", report.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(json.status.success());
    let json: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(
        json["summary"]["refusal_dialects"]["primary"]["dialect"],
        "rhino:unknown"
    );
    assert_eq!(
        json["summary"]["inspect_dialect_declared"]["archive_version"],
        "80"
    );
    assert_eq!(json["summary"]["export_target"], "step:ap242-e3");

    cadmpeg()
        .args(["query", "losses", report.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            "severity\tcode\tmessage\n\
             warning\trhino/source.dialect-unverified\tarchive word is residual\n",
        );
}

#[test]
fn summary_projects_structured_target_refusals() {
    let dir = tempdir().unwrap();
    let report = write(
        dir.path(),
        "target-refusal.json",
        r#"{
          "schema_version": 7,
          "command": "convert",
          "status": "refused",
          "refusal": {
            "stage": "plan",
            "code": "unsupported_target",
            "message": "iges cannot write 9.9",
            "target": {
              "kind": "unknown_explicit",
              "format": "iges",
              "requested": "9.9",
              "available": [{
                "id": "iges:5.3-fixed-ascii",
                "aliases": ["5.3"],
                "default": true
              }]
            }
          },
          "summary": null,
          "decode_report": null,
          "check_report": null,
          "export": null
        }"#,
    );

    let output = cadmpeg()
        .args(["query", "summary", report.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.status);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("refusal_target\t{\"available\":[{"),
        "{stdout}"
    );
    assert!(stdout.contains("\"kind\":\"unknown_explicit\""), "{stdout}");

    let output = cadmpeg()
        .args(["query", "summary", "--json", report.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.status);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["summary"]["refusal_target"]["kind"],
        "unknown_explicit"
    );
    assert_eq!(value["summary"]["refusal_target"]["requested"], "9.9");
    assert_eq!(
        value["summary"]["refusal_target"]["available"][0]["id"],
        "iges:5.3-fixed-ascii"
    );
}

#[test]
fn sidecar_summary_projects_supported_versions_and_discloses_unchecked_fidelity() {
    let dir = tempdir().unwrap();
    let sidecar = write(dir.path(), "legacy.fidelity.json", SIDECAR);

    cadmpeg()
        .args(["query", "summary", sidecar.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("sidecar_version\t3")
                .and(predicate::str::contains("sidecar_input_version\t1"))
                .and(predicate::str::contains(
                    "sidecar_fidelity_validation\tnot_run",
                )),
        );

    let unsupported = write(
        dir.path(),
        "future.fidelity.json",
        &SIDECAR.replacen("\"version\": \"1\"", "\"version\": \"9\"", 1),
    );
    cadmpeg()
        .args(["query", "summary", unsupported.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unsupported decode-sidecar version: 9",
        ));
}

#[test]
fn findings_and_losses_project_tsv_with_a_header() {
    let dir = tempdir().unwrap();
    let report = write(dir.path(), "report.json", CHECK_REPORT);

    cadmpeg()
        .args(["query", "findings", report.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            "severity\tcheck\tentity\tmessage\n\
             error\tidentity\te1\tduplicate id\n\
             warning\tunits\t\tnon-canonical unit\n",
        );

    cadmpeg()
        .args(["query", "losses", report.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            "severity\tcode\tmessage\n\
             warning\tshared/topology_not_transferred\twire dropped\n",
        );
}

#[test]
fn coverage_on_a_report_without_a_decode_stage_is_empty_not_an_error() {
    let dir = tempdir().unwrap();
    let report = write(dir.path(), "report.json", CHECK_REPORT);
    cadmpeg()
        .args(["query", "coverage", report.to_str().unwrap()])
        .assert()
        .success()
        .stdout("measure\tcount\n")
        .stderr(predicate::str::contains("no decode report"));
}

#[test]
fn coverage_projects_a_sidecar_decode_report() {
    let dir = tempdir().unwrap();
    let sidecar = write(dir.path(), "model.fidelity.json", SIDECAR);
    cadmpeg()
        .args(["query", "coverage", sidecar.to_str().unwrap()])
        .assert()
        .success()
        .stdout("measure\tcount\nsegments\t3\nstreams\t7\n");
}

#[test]
fn counts_works_on_all_three_kinds_with_teaching_on_the_sidecar() {
    let dir = tempdir().unwrap();

    let cadir = write(dir.path(), "model.cadir.json", CADIR_DOC);
    cadmpeg()
        .args(["query", "counts", cadir.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            "namespace\tarena\tentries\n\
             model\tedges\t0\n\
             model\tfaces\t2\n\
             native.fcstd\tobjects\t3\n",
        );

    let report = write(dir.path(), "report.json", CHECK_REPORT);
    cadmpeg()
        .args(["query", "counts", report.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            "namespace\tarena\tentries\n\
             model\tedges\t12\n\
             model\tfaces\t2\n",
        );

    let sidecar = write(dir.path(), "model.fidelity.json", SIDECAR);
    cadmpeg()
        .args(["query", "counts", sidecar.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("no entity counts")
                .and(predicate::str::contains("query coverage")),
        );
}

#[test]
fn arenas_alias_matches_counts_byte_for_byte() {
    let dir = tempdir().unwrap();
    let cadir = write(dir.path(), "model.cadir.json", CADIR_DOC);
    let counts = cadmpeg()
        .args(["query", "counts", cadir.to_str().unwrap()])
        .output()
        .unwrap();
    let arenas = cadmpeg()
        .args(["query", "arenas", cadir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(counts.status.success());
    assert_eq!(counts.stdout, arenas.stdout);
}

#[test]
fn findings_on_a_cadir_document_teaches_check_then_query() {
    let dir = tempdir().unwrap();
    let cadir = write(dir.path(), "model.cadir.json", CADIR_DOC);
    cadmpeg()
        .args(["query", "findings", cadir.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("cadmpeg check")
                .and(predicate::str::contains("query findings")),
        );
}

#[test]
fn query_json_wraps_the_projection_in_the_versioned_envelope() {
    let dir = tempdir().unwrap();
    let report = write(dir.path(), "report.json", CHECK_REPORT);
    let output = cadmpeg()
        .args(["query", "findings", "--json", report.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 7);
    assert_eq!(value["command"], "query findings");
    assert_eq!(value["findings"].as_array().unwrap().len(), 2);
    assert_eq!(value["findings"][0]["check"], "identity");
}

#[test]
fn query_reads_stdin_with_dash() {
    cadmpeg()
        .args(["query", "losses", "-"])
        .write_stdin(SIDECAR)
        .assert()
        .success()
        .stdout(predicate::str::contains("metadata_not_transferred"));
}

#[test]
fn a_version_two_sidecar_still_projects() {
    let dir = tempdir().unwrap();
    let sidecar = write(
        dir.path(),
        "model.fidelity.json",
        &SIDECAR.replace("\"version\": \"1\"", "\"version\": \"2\""),
    );
    cadmpeg()
        .args(["query", "coverage", sidecar.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("streams\t7"));
}

#[test]
fn an_unrecognized_json_file_names_the_expected_kinds() {
    let dir = tempdir().unwrap();
    let stray = write(dir.path(), "stray.json", r#"{"hello": "world"}"#);
    cadmpeg()
        .args(["query", "summary", stray.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("command report")
                .and(predicate::str::contains("CADIR document"))
                .and(predicate::str::contains(".fidelity.json decode sidecar")),
        );
}

#[test]
fn a_non_json_file_is_an_operational_error() {
    let dir = tempdir().unwrap();
    let stray = write(dir.path(), "stray.bin", "\u{1}\u{2}not json");
    cadmpeg()
        .args(["query", "summary", stray.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not a JSON object"));
}

#[test]
fn query_projects_a_real_check_report_end_to_end() {
    let dir = tempdir().unwrap();
    let ir = unit_cube();
    let model = dir.path().join("cube.cadir.json");
    fs::write(&model, ir.to_canonical_json().unwrap()).unwrap();
    let report = dir.path().join("report.json");

    cadmpeg()
        .args([
            "check",
            model.to_str().unwrap(),
            "-o",
            report.to_str().unwrap(),
        ])
        .assert()
        .success();

    cadmpeg()
        .args(["query", "summary", report.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("kind\tcommand report"));

    cadmpeg()
        .args(["query", "counts", report.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("namespace\tarena\tentries\n"));

    cadmpeg()
        .args(["query", "counts", model.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::starts_with("namespace\tarena\tentries\n")
                .and(predicate::str::contains("model\t")),
        );
}

// --- query item -------------------------------------------------------------

const ITEM_DOC: &str = r#"{
  "ir_version": "4",
  "model": {
    "sketch_entities": [
      {
        "id": "ns:sketch_entity#offset:1299062:skamp:2",
        "kind": "point",
        "meta": {"tag": "a", "note": "x\ty"},
        "links": [1, 2],
        "optional": "present"
      },
      {
        "id": "ns:sketch_entity#offset:1299062:skamp:3",
        "kind": "line",
        "meta": {"tag": "b"}
      },
      {
        "id": "other:face#802",
        "kind": "face",
        "meta": {"tag": "c"}
      },
      {
        "id": "other:coedge#802",
        "kind": "coedge",
        "meta": {"tag": "d"}
      }
    ],
    "empty_arena": [],
    "null_arena": null,
    "object_arena": {"not": "array"}
  },
  "native": {
    "creo": {
      "arenas": {
        "curve_parameters": [
          {"id": "creo:curve#818", "type_byte": 8, "feature_id": 11372},
          {"id": "creo:curve#825", "type_byte": 1, "feature_id": 11831}
        ]
      }
    }
  }
}"#;

// serde_json::Value pretty-print sorts object keys.
const FIRST_SKETCH: &str = r#"{
  "id": "ns:sketch_entity#offset:1299062:skamp:2",
  "kind": "point",
  "links": [
    1,
    2
  ],
  "meta": {
    "note": "x\ty",
    "tag": "a"
  },
  "optional": "present"
}"#;

#[test]
fn item_hits_by_full_id_and_aliases_match_byte_for_byte() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", ITEM_DOC);
    let path = doc.to_str().unwrap();
    let expected = format!("{FIRST_SKETCH}\n");

    let full = cadmpeg()
        .args([
            "query",
            "item",
            path,
            "model.sketch_entities",
            "ns:sketch_entity#offset:1299062:skamp:2",
        ])
        .output()
        .unwrap();
    assert!(
        full.status.success(),
        "{}",
        String::from_utf8_lossy(&full.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&full.stdout), expected);

    let alias = cadmpeg()
        .args([
            "query",
            "record",
            path,
            "model.sketch_entities",
            "ns:sketch_entity#offset:1299062:skamp:2",
        ])
        .output()
        .unwrap();
    assert_eq!(alias.stdout, full.stdout);

    let shorthand = cadmpeg()
        .args([
            "query",
            "item",
            path,
            "sketch_entities",
            "ns:sketch_entity#offset:1299062:skamp:2",
        ])
        .output()
        .unwrap();
    assert_eq!(shorthand.stdout, full.stdout);
}

#[test]
fn item_suffix_hit_and_ambiguous_suffix_error() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", ITEM_DOC);
    let path = doc.to_str().unwrap();

    cadmpeg()
        .args([
            "query",
            "item",
            path,
            "model.sketch_entities",
            "#offset:1299062:skamp:2",
        ])
        .assert()
        .success()
        .stdout(format!("{FIRST_SKETCH}\n"));

    cadmpeg()
        .args(["query", "item", path, "model.sketch_entities", "#802"])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("ambiguous id suffix")
                .and(predicate::str::contains("other:face#802"))
                .and(predicate::str::contains("other:coedge#802")),
        );
}

#[test]
fn item_multi_id_preserves_request_order_and_partial_failure() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", ITEM_DOC);
    let path = doc.to_str().unwrap();

    let out = cadmpeg()
        .args([
            "query",
            "item",
            path,
            "model.sketch_entities",
            "#offset:1299062:skamp:2",
            "other:face#802",
            "#offset:1299062:skamp:3",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let first = stdout.find("skamp:2").unwrap();
    let second = stdout.find("face#802").unwrap();
    let third = stdout.find("skamp:3").unwrap();
    assert!(first < second && second < third);

    let partial = cadmpeg()
        .args([
            "query",
            "item",
            path,
            "model.sketch_entities",
            "#offset:1299062:skamp:2",
            "missing-id",
        ])
        .output()
        .unwrap();
    assert_eq!(partial.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&partial.stdout).contains("skamp:2"));
    assert!(String::from_utf8_lossy(&partial.stderr).contains("missing-id"));
}

#[test]
fn item_native_arena_hits_string_id() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", ITEM_DOC);
    cadmpeg()
        .args([
            "query",
            "item",
            doc.to_str().unwrap(),
            "native.creo.curve_parameters",
            "curve#818",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"creo:curve#818\""));
}

#[test]
fn item_json_envelope_uses_item_payload_key() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", ITEM_DOC);
    let output = cadmpeg()
        .args([
            "query",
            "item",
            "--json",
            doc.to_str().unwrap(),
            "model.sketch_entities",
            "#offset:1299062:skamp:2",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 7);
    assert_eq!(value["command"], "query item");
    assert_eq!(value["item"].as_array().unwrap().len(), 1);
    assert_eq!(
        value["item"][0]["id"],
        "ns:sketch_entity#offset:1299062:skamp:2"
    );
}

#[test]
fn item_miss_arena_rejects_absent_and_null() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", ITEM_DOC);
    let path = doc.to_str().unwrap();

    for arena in ["model.no_such", "model.null_arena", "model.object_arena"] {
        cadmpeg()
            .args(["query", "item", path, arena])
            .assert()
            .code(2)
            .stderr(
                predicate::str::contains("unknown arena")
                    .and(predicate::str::contains("model.sketch_entities"))
                    .and(predicate::str::contains("native.creo.curve_parameters"))
                    .and(predicate::str::contains("query counts")),
            );
    }
}

#[test]
fn item_miss_id_lists_close_candidates() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", ITEM_DOC);
    cadmpeg()
        .args([
            "query",
            "item",
            doc.to_str().unwrap(),
            "model.sketch_entities",
            "skamp:99",
        ])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("no record in model.sketch_entities")
                .and(predicate::str::contains("4 entries"))
                .and(predicate::str::contains("skamp")),
        );
}

#[test]
fn item_rejects_report_and_sidecar() {
    let dir = tempdir().unwrap();
    let report = write(dir.path(), "report.json", CHECK_REPORT);
    cadmpeg()
        .args([
            "query",
            "item",
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
            "item",
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
fn item_no_id_and_head_modes() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", ITEM_DOC);
    let path = doc.to_str().unwrap();

    cadmpeg()
        .args(["query", "item", path, "model.sketch_entities"])
        .assert()
        .success()
        .stdout(format!("{FIRST_SKETCH}\n"));

    let head = cadmpeg()
        .args([
            "query",
            "item",
            path,
            "model.sketch_entities",
            "--head",
            "2",
        ])
        .output()
        .unwrap();
    assert!(head.status.success());
    let text = String::from_utf8_lossy(&head.stdout);
    assert!(text.contains("skamp:2"));
    assert!(text.contains("skamp:3"));
    assert!(!text.contains("face#802"));
    assert!(text.contains("\n\n"));

    cadmpeg()
        .args([
            "query",
            "item",
            path,
            "model.sketch_entities",
            "--head",
            "99",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("coedge#802"));

    cadmpeg()
        .args(["query", "item", path, "model.empty_arena"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn item_fields_projects_tsv_with_escapes_and_empty_path_teaching() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", ITEM_DOC);
    let path = doc.to_str().unwrap();

    cadmpeg()
        .args([
            "query",
            "item",
            path,
            "model.sketch_entities",
            "--head",
            "2",
            "--fields",
            "id,meta.tag,optional,links,meta.note",
        ])
        .assert()
        .success()
        .stdout(
            "id\tmeta.tag\toptional\tlinks\tmeta.note\n\
             ns:sketch_entity#offset:1299062:skamp:2\ta\tpresent\t[1,2]\tx\\ty\n\
             ns:sketch_entity#offset:1299062:skamp:3\tb\t\t\t\n",
        );

    cadmpeg()
        .args([
            "query",
            "item",
            path,
            "model.sketch_entities",
            "--head",
            "2",
            "--fields",
            "id,missing.path",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::starts_with("id\tmissing.path\n"))
        .stderr(
            predicate::str::contains("missing.path")
                .and(predicate::str::contains("empty in every projected row")),
        );

    cadmpeg()
        .args([
            "query",
            "item",
            path,
            "native.creo.curve_parameters",
            "curve#818",
            "--fields",
            "id,type_byte",
        ])
        .assert()
        .success()
        .stdout("id\ttype_byte\ncreo:curve#818\t8\n");

    cadmpeg()
        .args([
            "query",
            "item",
            path,
            "model.sketch_entities",
            "--json",
            "--fields",
            "id",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn item_reads_stdin_with_dash() {
    cadmpeg()
        .args(["query", "item", "-", "model.sketch_entities", "face#802"])
        .write_stdin(ITEM_DOC)
        .assert()
        .success()
        .stdout(predicate::str::contains("other:face#802"));
}

#[test]
fn item_head_conflicts_with_ids_and_fields_conflicts_with_json() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", ITEM_DOC);
    let path = doc.to_str().unwrap();

    let head = cadmpeg()
        .args([
            "query",
            "item",
            path,
            "model.sketch_entities",
            "face#802",
            "--head",
            "2",
        ])
        .output()
        .unwrap();
    assert!(!head.status.success());
    let stderr = String::from_utf8_lossy(&head.stderr);
    assert!(stderr.contains("cannot be used with"), "{stderr}");

    let fields = cadmpeg()
        .args([
            "query",
            "item",
            "--json",
            "--fields",
            "id",
            path,
            "model.sketch_entities",
        ])
        .output()
        .unwrap();
    assert!(!fields.status.success());
    let stderr = String::from_utf8_lossy(&fields.stderr);
    assert!(stderr.contains("cannot be used with"), "{stderr}");
}

#[test]
fn item_round_trips_counts_dotted_name_on_unit_cube() {
    let dir = tempdir().unwrap();
    let ir = unit_cube();
    let model = dir.path().join("cube.cadir.json");
    fs::write(&model, ir.to_canonical_json().unwrap()).unwrap();

    let counts = cadmpeg()
        .args(["query", "counts", "--json", model.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(counts.status.success());
    let value: serde_json::Value = serde_json::from_slice(&counts.stdout).unwrap();
    let counts_map = value["counts"].as_object().unwrap();
    assert!(counts_map.contains_key("model.faces"));
    assert!(counts_map["model.faces"].as_u64().unwrap() > 0);

    let face_id = ir.model.faces[0].id.to_string();
    cadmpeg()
        .args([
            "query",
            "item",
            model.to_str().unwrap(),
            "model.faces",
            &face_id,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(&face_id));
}

#[test]
fn schema_lists_model_arenas_with_element_types() {
    let output = cadmpeg().args(["query", "schema"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("arena\telement\trequired\n"), "{stdout}");
    assert!(stdout.contains("model.faces\tFace\tyes"), "{stdout}");
    assert!(
        stdout.contains("model.assembly_joints\tAssemblyJoint\tno"),
        "{stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("query schema sidecar"), "{stderr}");
}

#[test]
fn schema_projects_feature_fields_with_tagged_union_inventory() {
    let output = cadmpeg()
        .args(["query", "schema", "model.features"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("field\ttype\trequired\tdescription\n"),
        "{stdout}"
    );
    // The measured trap: the discriminator of a feature's definition is
    // `.definition.definition`, and the schema view names the tag key.
    assert!(
        stdout.contains("definition\tFeatureDefinition (tagged by definition,"),
        "{stdout}"
    );
    assert!(stdout.contains("extrude"), "{stdout}");
    // Optionality is the other measured trap: these fields are absent-as-null.
    assert!(stdout.contains("suppressed\tboolean?\tno"), "{stdout}");
    assert!(stdout.contains("outputs\tarray<BodyId>\tno"), "{stdout}");
    assert!(stdout.contains("id\tFeatureId\tyes"), "{stdout}");

    // Bare shorthand is byte-identical to the dotted name.
    let bare = cadmpeg()
        .args(["query", "schema", "features"])
        .output()
        .unwrap();
    assert_eq!(output.stdout, bare.stdout);
}

#[test]
fn schema_teaches_on_native_without_file_and_unknown_ir_arena() {
    let native = cadmpeg()
        .args(["query", "schema", "native.creo.rows"])
        .output()
        .unwrap();
    assert_eq!(native.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&native.stderr);
    assert!(stderr.contains("query schema FILE"), "{stderr}");
    assert!(stderr.contains("native.creo.rows"), "{stderr}");

    let unknown = cadmpeg()
        .args(["query", "schema", "model.bogus"])
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&unknown.stderr);
    assert!(stderr.contains("the IR defines:"), "{stderr}");
    assert!(stderr.contains("faces"), "{stderr}");
}

#[test]
fn schema_infers_native_fields_from_a_document() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", ITEM_DOC);
    let path = doc.to_str().unwrap();

    cadmpeg()
        .args(["query", "schema", path, "native.creo.curve_parameters"])
        .assert()
        .success()
        .stdout(
            "path\tpresence\ttype\texample\trelation\n\
             feature_id\t2/2\tnumber\t11372\t\n\
             id\t2/2\tstring\tcreo:curve#818\tid\n\
             type_byte\t2/2\tnumber\t8\t\n",
        )
        .stderr(predicate::str::contains(
            "inferred from 2 records in native.creo.curve_parameters",
        ));

    let output = cadmpeg()
        .args(["query", "schema", path, "model.sketch_entities"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("path\tpresence\ttype\texample\trelation\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("optional\t1/4\tstring\tpresent"),
        "{stdout}"
    );
    assert!(stdout.contains("meta.note\t1/4\tstring\tx\\ty"), "{stdout}");
    assert!(stdout.contains("meta.tag\t4/4\tstring\ta"), "{stdout}");
    assert!(stdout.contains("links\t1/4\tarray\t[1,2]"), "{stdout}");
    assert!(!stdout.contains("links.0"), "{stdout}");
    assert!(
        stdout.contains("id\t4/4\tstring\tns:sketch_entity#offset:1299062:skamp:2\tid"),
        "{stdout}"
    );

    let json = cadmpeg()
        .args([
            "query",
            "schema",
            "--json",
            path,
            "native.creo.curve_parameters",
        ])
        .output()
        .unwrap();
    assert!(json.status.success());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["command"], "query schema");
    assert_eq!(value["schema"]["inferred"], true);
    assert_eq!(value["schema"]["arena"], "native.creo.curve_parameters");
    assert_eq!(value["schema"]["records"], 2);
    let fields = value["schema"]["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0]["path"], "feature_id");
    assert_eq!(fields[0]["present"], 2);
    assert_eq!(fields[0]["type"], "number");
    assert_eq!(fields[0]["relation"], serde_json::Value::Null);
    assert_eq!(fields[1]["path"], "id");
    assert_eq!(fields[1]["relation"], "id");
}

#[test]
fn schema_document_unknown_arena_lists_counts() {
    let dir = tempdir().unwrap();
    let doc = write(dir.path(), "doc.json", ITEM_DOC);
    let path = doc.to_str().unwrap();

    let missing = cadmpeg()
        .args(["query", "schema", path, "native.creo.missing"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(
        stderr.contains("unknown arena native.creo.missing"),
        "{stderr}"
    );
    assert!(
        stderr.contains("native.creo.curve_parameters\t2"),
        "{stderr}"
    );
    assert!(stderr.contains("model.sketch_entities\t4"), "{stderr}");
    assert!(stderr.contains("model.empty_arena\t0"), "{stderr}");
    assert!(!stderr.contains("model.null_arena"), "{stderr}");

    let file_only = cadmpeg().args(["query", "schema", path]).output().unwrap();
    assert_eq!(file_only.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&file_only.stderr);
    assert!(stderr.contains("needs an arena name"), "{stderr}");
    assert!(
        stderr.contains("native.creo.curve_parameters\t2"),
        "{stderr}"
    );

    cadmpeg()
        .args(["query", "schema", path, "model.empty_arena"])
        .assert()
        .success()
        .stdout("path\tpresence\ttype\texample\trelation\n")
        .stderr(predicate::str::contains("(arena is empty)"));

    let report = write(dir.path(), "report.json", CHECK_REPORT);
    cadmpeg()
        .args([
            "query",
            "schema",
            report.to_str().unwrap(),
            "native.creo.rows",
        ])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("command report").and(predicate::str::contains("dump SOURCE")),
        );
}

#[test]
fn schema_sidecar_and_json_envelope() {
    let sidecar = cadmpeg()
        .args(["query", "schema", "sidecar"])
        .output()
        .unwrap();
    assert!(sidecar.status.success());
    let stdout = String::from_utf8_lossy(&sidecar.stdout);
    assert!(stdout.contains("fidelity\tSourceFidelity\tyes"), "{stdout}");
    assert!(stdout.contains("ir_sha256\tstring\tyes"), "{stdout}");

    let json = cadmpeg()
        .args(["query", "schema", "--json", "model.faces"])
        .output()
        .unwrap();
    assert!(json.status.success());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["command"], "query schema");
    assert_eq!(value["schema"]["element"], "Face");
    assert!(value["schema"]["defs"]
        .as_object()
        .unwrap()
        .contains_key("FaceId"));
}

const FIDELITY_SIDECAR: &str = r#"{
  "version": "1",
  "ir_sha256": "abc",
  "report": {"format": "f3d", "container_only": false, "geometry_transferred": true,
             "coverage": {}, "losses": [], "notes": []},
  "fidelity": {
    "version": "3",
    "annotations": {"streams": ["Contents/Config-0"],
                    "provenance": {"a:b:c#1": {"stream": 0, "offset": 0}},
                    "exactness": {}},
    "retained_records": [
      {"id": "r1", "stream": "Contents/Config-0", "offset": 0, "byte_len": 4,
       "sha256": "e12e115acf4552b2568b55e93cbd39394c4ef81c82447fafc997882a02d23677", "data": "QUJDRA=="},
      {"id": "r2", "stream": "Contents/Config-0", "offset": 4, "byte_len": 2,
       "sha256": "3a4db4ee1e59ce1a0a1b9f56bd6d5506d8c204e2f1d501b7a3a4021e6365e8db", "data": "RUY="},
      {"id": "r3", "stream": "Other", "offset": 0, "byte_len": 3, "sha256": "z"}
    ]
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
             Contents/Config-0\t0\t4\tyes\tr1\n\
             Contents/Config-0\t4\t2\tyes\tr2\n\
             Other\t0\t3\tno\tr3\n",
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
            "exists; pass --force to replace it",
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
    assert_eq!(value["command"], "query fidelity");
    assert_eq!(value["fidelity"]["annotations"]["provenance"], 1);
    assert_eq!(
        value["fidelity"]["retained_records"][2]["data_retained"],
        false
    );
}

#[test]
fn a_written_report_carries_the_generator_and_summary_prints_it() {
    let dir = tempdir().unwrap();
    let ir = unit_cube();
    let model = dir.path().join("cube.cadir.json");
    fs::write(&model, ir.to_canonical_json().unwrap()).unwrap();
    let report = dir.path().join("cube.report.json");

    cadmpeg()
        .args([
            "check",
            model.to_str().unwrap(),
            "-o",
            report.to_str().unwrap(),
        ])
        .assert()
        .success();
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
    let generator = value["generator"].as_str().unwrap();
    assert!(generator.starts_with("cadmpeg "), "{generator}");
    assert!(generator.contains("+g"), "{generator}");

    cadmpeg()
        .args(["query", "summary", report.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("generator\tcadmpeg "));

    // Reports from older builds have no generator; the row is simply absent.
    let stripped = write(dir.path(), "old.report.json", CHECK_REPORT);
    let output = cadmpeg()
        .args(["query", "summary", stripped.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("generator"));
}
