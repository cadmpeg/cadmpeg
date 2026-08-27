// SPDX-License-Identifier: Apache-2.0
//! Diff: entity deltas, source metadata, and per-input reader selection.

use assert_cmd::Command;
use cadmpeg_ir::examples::unit_cube;
use predicates::prelude::*;
use tempfile::tempdir;

use crate::support::*;

#[test]
fn diff_reports_modified_entities_and_uses_diff_exit_codes() {
    let dir = tempdir().unwrap();
    let left = unit_cube();
    let mut right = left.clone();
    right.model.points[0].position.x += 0.5;
    right.model.edges[0].tolerance = Some(0.01);
    right.model.coedges[0].sense = match right.model.coedges[0].sense {
        cadmpeg_ir::topology::Sense::Forward => cadmpeg_ir::topology::Sense::Reversed,
        cadmpeg_ir::topology::Sense::Reversed => cadmpeg_ir::topology::Sense::Forward,
    };
    let a = fixture(dir.path(), "a.json", &left);
    let b = fixture(dir.path(), "b.json", &right);

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", a.to_str().unwrap(), a.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("identical"));
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(
            predicate::str::contains("points: +0 -0 ~1")
                .and(predicate::str::contains("coedges: +0 -0 ~1"))
                .and(predicate::str::contains("edges: +0 -0 ~1")),
        )
        .stderr(predicate::str::is_empty());
}

#[test]
fn diff_reports_a_source_attribute_change_and_exits_one() {
    let dir = tempdir().unwrap();
    let a = fixture(
        dir.path(),
        "a.json",
        &cube_with_source(&[("program_version", "1.0"), ("object_count", "3")]),
    );
    let b = fixture(
        dir.path(),
        "b.json",
        &cube_with_source(&[("program_version", "1.1"), ("object_count", "3")]),
    );

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "source program_version: 1.0 → 1.1",
        ))
        .stderr(predicate::str::is_empty());
}

/// A machine-local digest cannot agree across platforms, so a difference in one
/// alone is reported without making the documents different.
#[test]
fn diff_reports_a_machine_local_digest_difference_without_a_verdict() {
    let dir = tempdir().unwrap();
    let a = fixture(
        dir.path(),
        "a.json",
        &cube_with_source(&[("document_local_sha256", &"0".repeat(64))]),
    );
    let b = fixture(
        dir.path(),
        "b.json",
        &cube_with_source(&[("document_local_sha256", &"1".repeat(64))]),
    );

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("machine-local digests")
                .and(predicate::str::contains("document_local_sha256"))
                .and(predicate::str::contains("identical")),
        );

    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", "--json", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .success();
    let value: serde_json::Value =
        serde_json::from_slice(&output.get_output().stdout).expect("diff --json emits JSON");
    assert_eq!(value["different"], serde_json::json!(false));
    assert_eq!(
        value["diff"]["source"]["local_digests"][0]["key"],
        serde_json::json!("document_local_sha256")
    );
    assert!(value["diff"]["source"]["attributes"]
        .as_array()
        .expect("attributes is an array")
        .is_empty());
}

/// A document with no source metadata against one that has some must not panic.
#[test]
fn diff_handles_absent_source_metadata() {
    let dir = tempdir().unwrap();
    let mut bare = unit_cube();
    bare.source = None;
    let a = fixture(dir.path(), "a.json", &bare);
    let b = fixture(
        dir.path(),
        "b.json",
        &cube_with_source(&[("object_count", "3")]),
    );

    for (left, right) in [(&a, &b), (&b, &a)] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(["diff", left.to_str().unwrap(), right.to_str().unwrap()])
            .assert()
            .code(1)
            .stdout(predicate::str::contains("source format:"))
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn diff_summarizes_the_source_fidelity_sidecar_for_native_inputs() {
    let dir = tempdir().unwrap();
    let a = minimal_rhino_archive(dir.path(), "a.3dm", "50");
    let b = minimal_rhino_archive(dir.path(), "b.3dm", "50");

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("source fidelity: identical"));

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", "--json", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"source_fidelity\"")
                .and(predicate::str::contains("\"present\": \"both\"")),
        );
}

#[test]
fn diff_rejects_input_format_override() {
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", "a", "b", "--input-format", "cadir"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unexpected argument"));
}

#[test]
fn diff_input_format_forces_the_reader_per_input() {
    let dir = tempdir().unwrap();
    let a = fixture(dir.path(), "a.cadir.json", &unit_cube());
    let b = fixture(dir.path(), "b.cadir.json", &unit_cube());

    // Two identical CADIR documents compare equal.
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("identical"));

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "diff",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--input-format-a",
            "rhino",
        ])
        .assert()
        .code(2);

    // The second-input flag targets its input independently.
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "diff",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--input-format-b",
            "rhino",
        ])
        .assert()
        .code(2);
}
