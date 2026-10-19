// SPDX-License-Identifier: Apache-2.0
//! Diff: entity deltas, source metadata, and per-input reader selection.

use std::fs;

use assert_cmd::Command;
use cadmpeg_ir::examples::unit_cube;
use predicates::prelude::{predicate, PredicateBooleanExt};
use tempfile::tempdir;

use crate::support::{cube_with_source, fixture, minimal_rhino_archive};

#[test]
fn diff_reports_modified_entities_and_uses_diff_exit_codes() {
    let dir = tempdir().unwrap();
    let left = unit_cube().expect("unit cube fixture is admitted");
    let mut right = left.clone();
    let moved = right.model.points[0].position().get();
    right.model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x + 0.5,
            moved.y,
            moved.z,
        ))
        .expect("a finite position is a point"),
    );
    right.model.edges[0].tolerance =
        Some(cadmpeg_ir::scalar::PositiveReal::new(0.01).expect("positive finite tolerance"));
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

#[test]
fn diff_reports_dialect_admission_and_declaration_changes() {
    use cadmpeg_core::dialect::{DialectLayers, DialectMatch};
    use std::collections::BTreeMap;

    let dir = tempdir().unwrap();
    let mut left = unit_cube().expect("unit cube fixture is admitted");
    left.source = Some(cadmpeg_ir::SourceMeta::classified(
        DialectLayers::of(
            DialectMatch::admitted(cadmpeg_core::dialect_id!("synthetic:v1")).with_declared(
                BTreeMap::from([(cadmpeg_core::nonblank_literal!("version"), "1".into())]),
            ),
        ),
        BTreeMap::new(),
    ));
    let mut right = unit_cube().expect("unit cube fixture is admitted");
    right.source = Some(cadmpeg_ir::SourceMeta::classified(
        DialectLayers::of(
            DialectMatch::residual(cadmpeg_core::dialect_id!("synthetic:v1")).with_declared(
                BTreeMap::from([(cadmpeg_core::nonblank_literal!("version"), "2".into())]),
            ),
        ),
        BTreeMap::new(),
    ));
    let a = fixture(dir.path(), "a.json", &left);
    let b = fixture(dir.path(), "b.json", &right);

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(
            predicate::str::contains("source dialect layers:").and(
                predicate::str::contains("\"declared\":{\"version\":\"1\"}")
                    .and(predicate::str::contains("\"declared\":{\"version\":\"2\"}")),
            ),
        )
        .stderr(predicate::str::is_empty());
}

/// Every source attribute is content, regardless of its name.
#[test]
fn diff_counts_every_source_attribute_in_its_verdict() {
    let dir = tempdir().unwrap();
    let a = fixture(
        dir.path(),
        "a.json",
        &cube_with_source(&[("document_local_sha256", "before")]),
    );
    let b = fixture(
        dir.path(),
        "b.json",
        &cube_with_source(&[("document_local_sha256", "after")]),
    );
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "source document_local_sha256: before → after",
        ));
    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", "--json", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .code(1);
    let value: serde_json::Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
    assert_eq!(value["different"], true);
    assert_eq!(
        value["diff"]["source"]["attributes"][0]["key"],
        "document_local_sha256"
    );
}

/// A document with no source metadata against one that has some must not panic.
#[test]
fn diff_handles_absent_source_metadata() {
    let dir = tempdir().unwrap();
    let mut bare = unit_cube().expect("unit cube fixture is admitted");
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
    let a = fixture(
        dir.path(),
        "a.cadir.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
    let b = fixture(
        dir.path(),
        "b.cadir.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );

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

#[test]
fn diff_reports_scope_changes_as_document_changes() {
    let dir = tempdir().unwrap();
    let full = unit_cube().unwrap();
    let mut geometry = full.clone();
    geometry
        .set_scope(cadmpeg_ir::document::ContentScope::geometry())
        .unwrap();
    let left = fixture(dir.path(), "full.json", &full);
    let right = fixture(dir.path(), "geometry.json", &geometry);
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", left.to_str().unwrap(), right.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("scope:"));
    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "diff",
            "--json",
            left.to_str().unwrap(),
            right.to_str().unwrap(),
        ])
        .assert()
        .code(1);
    let report: serde_json::Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
    assert_eq!(report["different"], true);
    assert_eq!(
        report["diff"]["scope_change"],
        serde_json::json!([
            {"model": true, "design": true}, {"model": true, "design": false}
        ])
    );
}

#[test]
fn diff_reports_unrepresented_class_changes_even_when_cadir_is_identical() {
    use cadmpeg_ir::{decode_sidecar_path, DecodeSidecar};
    let dir = tempdir().unwrap();
    let native = minimal_rhino_archive(dir.path(), "source.3dm", "50");
    let left = dir.path().join("left.cadir.json");
    let right = dir.path().join("right.cadir.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "dump",
            native.to_str().unwrap(),
            "-o",
            left.to_str().unwrap(),
        ])
        .assert()
        .success();
    fs::copy(&left, &right).unwrap();
    let mut sidecar =
        DecodeSidecar::from_json(&fs::read_to_string(decode_sidecar_path(&left).unwrap()).unwrap())
            .unwrap();
    sidecar
        .fidelity
        .insert_retained_record(
            "test:source:unknown#unread-design".try_into().unwrap(),
            cadmpeg_ir::RetainedSourceRecord::whole("test:source", b"unread design".to_vec())
                .with_untyped([cadmpeg_ir::document::ArenaClass::Design].into()),
        )
        .unwrap();
    fs::write(
        decode_sidecar_path(&right).unwrap(),
        sidecar.to_canonical_json().unwrap(),
    )
    .unwrap();
    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "diff",
            "--json",
            left.to_str().unwrap(),
            right.to_str().unwrap(),
        ])
        .assert()
        .code(1);
    let report: serde_json::Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
    assert_eq!(report["different"], true);
    assert_eq!(
        report["source_fidelity"]["diff"]["unrepresented_changed"],
        true
    );
    assert!(report["diff"]["scope_change"].is_null());
}
