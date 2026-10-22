// SPDX-License-Identifier: Apache-2.0
//! `diff --geometry`: ID-free comparison of measured bodies, with
//! expectations from the comparison rules in `docs/measurement.md`.

#[cfg(any(feature = "step", feature = "fcstd"))]
use std::path::Path;
#[cfg(feature = "step")]
use std::path::PathBuf;

use assert_cmd::Command;
use cadmpeg_ir::examples::unit_cube;
use serde_json::Value;
use tempfile::tempdir;

use crate::support::fixture;

fn cadmpeg() -> Command {
    Command::cargo_bin("cadmpeg").unwrap()
}

/// Run `diff A B --geometry --json` and return the exit code and report.
#[cfg(any(feature = "step", feature = "fcstd"))]
fn geometry_json(a: &str, b: &str) -> (Option<i32>, Value) {
    let output = cadmpeg()
        .args(["diff", a, b, "--geometry", "--json"])
        .output()
        .unwrap();
    let report = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    (output.status.code(), report)
}

#[cfg(feature = "step")]
fn measure_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/measure")
        .join(name)
}

#[cfg(feature = "step")]
#[test]
fn geometry_diff_round_trip_agrees() {
    let dir = tempdir().unwrap();
    let source = measure_fixture("measure_cylinder_solid.p21");
    let written = dir.path().join("rt.step");
    cadmpeg()
        .args(["convert", source.to_str().unwrap(), "-o"])
        .arg(&written)
        .args(["--to", "step:ap242-e3"])
        .assert()
        .success();
    let (code, report) = geometry_json(source.to_str().unwrap(), written.to_str().unwrap());
    assert_eq!(code, Some(0), "{report:#}");
    assert_eq!(report["different"], false);
    let comparison = &report["geometry"]["comparison"];
    assert_eq!(comparison["verdict"], "agree");
    assert_eq!(comparison["placement"]["a"], "trivial");
    assert_eq!(comparison["placement"]["b"], "trivial");
    assert_eq!(comparison["pairs"].as_array().unwrap().len(), 1);
    assert!(report["diff"].is_object());
    assert!(report["source_fidelity"].is_object());
}

/// The cylinder fixture moved by +1 mm in `x`: the four points of its axis
/// placements and vertices shift.
#[cfg(feature = "step")]
#[test]
fn geometry_diff_detects_moved_body() {
    let dir = tempdir().unwrap();
    let source = measure_fixture("measure_cylinder_solid.p21");
    let text = std::fs::read_to_string(&source).unwrap();
    let moved_text = text
        .replace(
            "#10=CARTESIAN_POINT('',(0.,0.,0.));",
            "#10=CARTESIAN_POINT('',(1.,0.,0.));",
        )
        .replace(
            "#11=CARTESIAN_POINT('',(0.,0.,30.));",
            "#11=CARTESIAN_POINT('',(1.,0.,30.));",
        )
        .replace(
            "#20=CARTESIAN_POINT('',(10.,0.,0.));",
            "#20=CARTESIAN_POINT('',(11.,0.,0.));",
        )
        .replace(
            "#21=CARTESIAN_POINT('',(10.,0.,30.));",
            "#21=CARTESIAN_POINT('',(11.,0.,30.));",
        );
    assert_ne!(moved_text, text);
    let moved = dir.path().join("moved.p21");
    std::fs::write(&moved, moved_text).unwrap();
    let (code, report) = geometry_json(source.to_str().unwrap(), moved.to_str().unwrap());
    assert_eq!(code, Some(1));
    let comparison = &report["geometry"]["comparison"];
    assert_eq!(comparison["verdict"], "disagree");
    assert_eq!(
        comparison["unmatched_a"],
        serde_json::json!(["step:data:body#71"])
    );
    assert_eq!(comparison["unmatched_b"].as_array().unwrap().len(), 1);
    let row = |quantity: &str| {
        comparison["document"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["quantity"] == quantity)
            .unwrap_or_else(|| panic!("document row {quantity}"))
            .clone()
    };
    assert_eq!(row("area")["agrees"], true);
    assert_eq!(row("closed_volume")["agrees"], true);
    assert_eq!(row("bbox.min.x")["agrees"], false);
}

/// Each `FreeCAD` corpus file against itself. Identical documents give
/// identical rows, so nothing disagrees and no body is unmatched. Bodies
/// whose faces lie on procedural carriers without an admitted cache
/// (`Loft`, `Sweep`, `Swept`) have no comparable quantity, and occurrences
/// that place bodies (`core_design_product`) keep the verdict
/// inconclusive.
#[cfg(feature = "fcstd")]
#[test]
fn geometry_diff_self_comparison() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/freecad_fcstd/fixtures");
    for (file, unmeasured, causes) in [
        (
            "core_operations.FCStd",
            &["#Loft:", "#Sweep:"][..],
            &["compare.unmeasured-body"][..],
        ),
        (
            "geometry_topology.FCStd",
            &["#Swept:"][..],
            &["compare.unavailable", "compare.unmeasured-body"][..],
        ),
        (
            "core_design_product.FCStd",
            &[][..],
            &["compare.occurrence-placement"][..],
        ),
    ] {
        let path = corpus.join(file);
        let (code, report) = geometry_json(path.to_str().unwrap(), path.to_str().unwrap());
        let comparison = &report["geometry"]["comparison"];
        assert_eq!(comparison["verdict"], "inconclusive", "{file}");
        assert_eq!(code, Some(1), "{file}");
        assert_eq!(
            comparison["inconclusive_causes"],
            serde_json::json!(causes),
            "{file}"
        );
        assert!(comparison["unmatched_a"].as_array().unwrap().is_empty());
        assert!(comparison["unmatched_b"].as_array().unwrap().is_empty());
        let names: Vec<&str> = comparison["unmeasured_a"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap())
            .collect();
        assert_eq!(names.len(), unmeasured.len(), "{file}: {names:?}");
        for needle in unmeasured {
            assert!(
                names.iter().any(|id| id.contains(needle)),
                "{file}: {needle}"
            );
        }
        let rows = comparison["pairs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|pair| pair["quantities"].as_array().unwrap().iter())
            .chain(comparison["document"].as_array().unwrap().iter());
        for row in rows {
            assert_ne!(row["agrees"], false, "{file}: {row:#}");
        }
    }
}

#[test]
fn geometry_diff_tsv() {
    let dir = tempdir().unwrap();
    let cube = fixture(
        dir.path(),
        "cube.cadir.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
    let output = cadmpeg()
        .args(["diff"])
        .arg(&cube)
        .arg(&cube)
        .args(["--geometry", "--tsv"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    let header = text.lines().next().unwrap();
    assert_eq!(
        header,
        "a\tb\tquantity\tunit\tvalue_a\tvalue_b\tdifference\ttolerance\trule\tagrees"
    );
    assert!(text
        .lines()
        .any(|line| line.starts_with("document\tdocument\tbody_count\t")));
    cadmpeg()
        .args(["diff"])
        .arg(&cube)
        .arg(&cube)
        .arg("--tsv")
        .assert()
        .code(2);
    cadmpeg()
        .args(["diff"])
        .arg(&cube)
        .arg(&cube)
        .args(["--geometry", "--tsv", "--json"])
        .assert()
        .code(2);
}

#[test]
fn geometry_diff_rejects_container_only() {
    let dir = tempdir().unwrap();
    let cube = fixture(
        dir.path(),
        "cube.cadir.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
    cadmpeg()
        .args(["diff"])
        .arg(&cube)
        .arg(&cube)
        .args(["--geometry", "--container-only"])
        .assert()
        .code(2);
    cadmpeg()
        .args(["diff"])
        .arg(&cube)
        .arg(&cube)
        .args(["--geometry", "--geometry-tolerance", "1.5"])
        .assert()
        .code(2);
}

#[test]
fn id_diff_report_has_no_geometry_member() {
    let dir = tempdir().unwrap();
    let cube = fixture(
        dir.path(),
        "cube.cadir.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
    let output = cadmpeg()
        .args(["diff"])
        .arg(&cube)
        .arg(&cube)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut keys: Vec<&str> = report
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "command",
            "diff",
            "different",
            "generator",
            "ir_version",
            "refusal",
            "source_fidelity",
            "status"
        ]
    );
}
