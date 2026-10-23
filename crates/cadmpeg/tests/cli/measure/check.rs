// SPDX-License-Identifier: Apache-2.0
//! `check` warnings from measurement: rings listed out of traversal order,
//! faces whose sense disagrees with their loop winding, and closed bodies
//! whose volume readings conflict. Expectations come from the dumped
//! document's own rings and shells; warnings never change the exit status.

#[cfg(any(feature = "fcstd", feature = "iges"))]
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
#[cfg(feature = "step")]
use std::path::PathBuf;

use serde_json::Value;
#[cfg(any(feature = "fcstd", feature = "iges"))]
use tempfile::tempdir;

/// Run `check PATH --json ARGS`, require exit 0 and return the report.
fn check_json(path: &Path, args: &[&str]) -> Value {
    let output = super::cadmpeg()
        .arg("check")
        .arg(path)
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// The findings of one check, each a warning.
fn warnings<'a>(report: &'a Value, check: &str) -> Vec<&'a Value> {
    let found: Vec<&Value> = report["check_report"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["check"] == check)
        .collect();
    for finding in &found {
        assert_eq!(finding["severity"], "warning", "{finding:#}");
    }
    found
}

#[cfg(any(feature = "fcstd", feature = "iges"))]
/// The CADIR document `path` decodes to.
fn dumped(path: &Path) -> Value {
    let dir = tempdir().unwrap();
    let out = dir.path().join("model.cadir.json");
    super::cadmpeg()
        .arg("dump")
        .arg(path)
        .arg("-o")
        .arg(&out)
        .assert()
        .success();
    serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap()
}

#[cfg(any(feature = "fcstd", feature = "iges"))]
fn by_id<'a>(document: &'a Value, arena: &str) -> BTreeMap<&'a str, &'a Value> {
    document["model"][arena]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|record| (record["id"].as_str().unwrap(), record))
        .collect()
}

#[cfg(feature = "fcstd")]
/// Each ring loop whose listed coedges do not run end to start by oriented
/// vertex identity, with its face.
fn discontinuous_rings(document: &Value) -> BTreeMap<String, String> {
    let coedges = by_id(document, "coedges");
    let edges = by_id(document, "edges");
    let ends = |coedge: &str| {
        let coedge = coedges[coedge];
        let edge = edges[coedge["edge"].as_str().unwrap()];
        let (start, end) = (
            edge["start"].as_str().unwrap(),
            edge["end"].as_str().unwrap(),
        );
        if coedge["sense"] == "reversed" {
            (end, start)
        } else {
            (start, end)
        }
    };
    let mut rings = BTreeMap::new();
    for face_loop in document["model"]["loops"].as_array().unwrap() {
        if face_loop["boundary"]["kind"] != "ring" {
            continue;
        }
        let listed: Vec<(&str, &str)> = face_loop["boundary"]["coedges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|coedge| ends(coedge.as_str().unwrap()))
            .collect();
        let continuous =
            (0..listed.len()).all(|index| listed[index].1 == listed[(index + 1) % listed.len()].0);
        if !continuous {
            rings.insert(
                face_loop["id"].as_str().unwrap().to_owned(),
                face_loop["face"].as_str().unwrap().to_owned(),
            );
        }
    }
    rings
}

#[cfg(feature = "fcstd")]
#[test]
fn freecad_rings_out_of_traversal_order_are_check_warnings() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd");
    let expected = discontinuous_rings(&dumped(&path));
    assert!(!expected.is_empty());
    let report = check_json(&path, &[]);
    let found: BTreeMap<String, String> = warnings(&report, "ring_order")
        .into_iter()
        .map(|finding| {
            let related = finding["related"].as_array().unwrap();
            assert_eq!(related.len(), 1, "{finding:#}");
            (
                finding["entity"].as_str().unwrap().to_owned(),
                related[0].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(found, expected);
    assert_eq!(
        report["check_report"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|finding| finding["severity"] == "error")
            .count(),
        0
    );
}

#[cfg(feature = "iges")]
/// `docs/formats/iges-open-items.md` TP-02: each face of the void shell of
/// `explicit_void_solid.igs` has the sense `reversed` and a loop that keeps
/// it on its right, so the loop winding gives the surface normal.
#[test]
fn iges_void_shell_orientation_conflict_is_a_check_warning() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cadmpeg-codec-iges/tests/golden/fixtures/explicit_void_solid.igs");
    let document = dumped(&path);
    let [region] = document["model"]["regions"].as_array().unwrap().as_slice() else {
        panic!("one region");
    };
    let void = region["shells"][1].as_str().unwrap();
    let void_faces: BTreeSet<&str> = by_id(&document, "faces")
        .into_iter()
        .filter(|(_, face)| face["shell"] == void)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(void_faces.len(), 4);
    let body = region["body"].as_str().unwrap();

    for args in [&[][..], &["--limits", "service"][..]] {
        let report = check_json(&path, args);
        let found = warnings(&report, "orientation");
        let faces: BTreeSet<&str> = found
            .iter()
            .filter(|finding| finding["entity"] != body)
            .map(|finding| {
                assert_eq!(
                    finding["message"],
                    "the face's sense gives the reversed normal and its loop winding the \
                     surface normal"
                );
                finding["entity"].as_str().unwrap()
            })
            .collect();
        assert_eq!(faces, void_faces);
        let body_findings: Vec<&&Value> = found
            .iter()
            .filter(|finding| finding["entity"] == body)
            .collect();
        let [body_finding] = body_findings.as_slice() else {
            panic!("one body finding: {found:#?}");
        };
        let message = body_finding["message"].as_str().unwrap();
        assert!(message.contains("volume.orientation-conflict"), "{message}");
        let related: BTreeSet<&str> = body_finding["related"]
            .as_array()
            .unwrap()
            .iter()
            .map(|face| face.as_str().unwrap())
            .collect();
        assert_eq!(related, void_faces);
        assert!(warnings(&report, "ring_order").is_empty());
    }
}

#[cfg(feature = "iges")]
#[test]
fn conversion_carries_measurement_warnings_and_succeeds() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cadmpeg-codec-iges/tests/golden/fixtures/explicit_void_solid.igs");
    let dir = tempdir().unwrap();
    let report = dir.path().join("conversion.json");
    super::cadmpeg()
        .arg("convert")
        .arg(&path)
        .arg("-o")
        .arg(dir.path().join("void.cadir.json"))
        .arg("--report")
        .arg(&report)
        .assert()
        .success();
    let report: Value = serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
    assert_eq!(warnings(&report, "orientation").len(), 5);
}

#[cfg(feature = "step")]
/// The freeform B-spline face of `geometry_topology.step` needs far more
/// than one findings face slice. The check does not read it and states, in
/// one info finding, how many faces and bodies it did not check, under each
/// profile; the exit status stays 0.
#[test]
fn a_face_beyond_the_findings_slice_is_counted_in_one_info_finding() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cadmpeg-codec-freecad/tests/golden/step/geometry_topology.step");
    for args in [&[][..], &["--limits", "service"][..]] {
        let report = check_json(&path, args);
        let incomplete: Vec<&Value> = report["check_report"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|finding| finding["check"] == "measurement_incomplete")
            .collect();
        let [finding] = incomplete.as_slice() else {
            panic!("one measurement_incomplete finding: {incomplete:#?}");
        };
        assert_eq!(finding["severity"], "info");
        assert!(finding.get("entity").is_none());
        let message = finding["message"].as_str().unwrap();
        let counts = message
            .strip_prefix("measurement checks were skipped: the findings allowance ran out before ")
            .and_then(|rest| rest.strip_suffix(" were checked"))
            .unwrap_or_else(|| panic!("{message}"));
        let faces: usize = counts
            .split_once(' ')
            .and_then(|(count, _)| count.parse().ok())
            .unwrap_or_else(|| panic!("{message}"));
        assert!(faces >= 1, "{message}");
    }
}

#[cfg(feature = "step")]
fn measure_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/measure")
        .join(name)
}

#[cfg(feature = "step")]
#[test]
fn the_188_step_fixtures_have_no_measurement_warnings() {
    for name in [
        "measure_cylinder_solid.p21",
        "measure_cylinder_patch.p21",
        "measure_plane_hole.p21",
    ] {
        let report = check_json(&measure_fixture(name), &[]);
        assert!(warnings(&report, "ring_order").is_empty(), "{name}");
        assert!(warnings(&report, "orientation").is_empty(), "{name}");
    }
}
