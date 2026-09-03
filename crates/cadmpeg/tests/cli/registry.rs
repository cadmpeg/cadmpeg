// SPDX-License-Identifier: Apache-2.0
//! `cadmpeg formats` and `cadmpeg dialects`: the registries, rendered.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

use crate::support::*;

/// `formats` states read and write separately, because they differ.
///
/// Inventor, CATIA, Creo, NX, and SAT are read-only; a single column would
/// have to lie about one half of every one of them.
#[test]
fn formats_separates_reading_from_writing() {
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .arg("formats")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("FORMAT")
                .and(predicate::str::contains("READ"))
                .and(predicate::str::contains("WRITE"))
                .and(predicate::str::is_match(r"inventor\s+yes\s+no").unwrap())
                .and(predicate::str::is_match(r"rhino\s+yes\s+yes").unwrap())
                .and(predicate::str::contains("3dm")),
        );
}

/// `dialects` renders the identity registry crossed with the capability
/// registry, and marks the rows this build's encoder can write.
///
/// The three columns come from three sources — `docs/dialects.toml`,
/// `docs/dialect-support.toml`, and the compiled `Encoder::targets()` catalog
/// — so a row that reads correctly and writes correctly proves the join, not
/// just the rendering.
#[test]
fn dialects_joins_identity_capability_and_the_compiled_catalog() {
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dialects", "rhino"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("default rhino:archive-80")
                .and(predicate::str::contains(
                    "Rhino 3DM archive version 50 (Rhino 5)",
                ))
                .and(
                    predicate::str::is_match(r"rhino:archive-50\s+L\d+\s+emitted \(target\)")
                        .unwrap(),
                )
                // A read-side row is present and is not marked a target.
                .and(
                    predicate::str::is_match(r"rhino:unknown\s+unclassified-recovered\s+none\s")
                        .unwrap(),
                ),
        );

    // The bare command covers every format the registry declares, including
    // the two with no owning crate.
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .arg("dialects")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("acis  (no encoder in this build)")
                .and(predicate::str::contains("parasolid"))
                .and(predicate::str::contains("iges:5.3-fixed-ascii")),
        );

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dialects", "nosuch"])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("no format nosuch").and(predicate::str::contains("rhino")),
        );
}

/// Every id `dialects` prints is a spelling `--to` accepts.
///
/// The loop the design asks for: run the tool and it prints the value you
/// would pass. A `dialects` row for a write target that `--to` then refused
/// would break it, so the write targets are re-offered to the CLI here.
#[test]
fn every_write_target_dialects_lists_is_a_to_value() {
    let dir = tempdir().unwrap();
    let listing = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dialects", "rhino"])
        .output()
        .unwrap();
    let listed = String::from_utf8(listing.stdout).unwrap();

    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.points.push(cadmpeg_ir::topology::Point {
        id: cadmpeg_ir::ids::PointId("cadir:model:point#listing".into()),
        position: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
        source_object: None,
    });
    let input = fixture(dir.path(), "point.cadir.json", &ir);

    let targets = listed
        .lines()
        .filter(|line| line.contains("(target)"))
        .map(|line| line.split_whitespace().next().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), 4, "{listed}");
    for (index, id) in targets.iter().enumerate() {
        let output = dir.path().join(format!("point-{index}.3dm"));
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                input.to_str().unwrap(),
                "-o",
                output.to_str().unwrap(),
                "--to",
                id,
                "--allow-errors",
            ])
            .assert()
            .success();
    }
}

/// `inspect` prints the dialect line the same vocabulary uses.
#[test]
fn inspect_states_the_dialect_its_read_score_and_the_write_catalog() {
    let dir = tempdir().unwrap();
    let input = minimal_rhino_archive(dir.path(), "empty.3dm", "50");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", input.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("dialect: rhino:archive-50")
                .and(predicate::str::contains("read L"))
                .and(predicate::str::contains("write targets archive-50"))
                .and(predicate::str::contains("archive-80")),
        );
}
