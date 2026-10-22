// SPDX-License-Identifier: Apache-2.0
//! `cadmpeg measure`: rows, selection, output modes and refusals.
//!
//! Expected values are closed-form expressions of the fixture geometry
//! (`tests/fixtures/measure/README.md`).

#[cfg(feature = "step")]
use std::fs;
#[cfg(any(feature = "step", feature = "iges", feature = "fcstd"))]
use std::path::Path;
#[cfg(feature = "step")]
use std::path::PathBuf;

use assert_cmd::Command;
#[cfg(any(feature = "step", feature = "fcstd"))]
use predicates::prelude::predicate;
use serde_json::Value;
use tempfile::tempdir;

/// Relative floor of the comparison of a measured value with its expression.
const TEST_ABSOLUTE_FLOOR: f64 = 1.0e-9;

#[cfg(feature = "step")]
/// The faces table header of the measurement contract.
const FACE_HEADER: &str = "face\tbody\tsurface\tkind\tradius_mm\tdiameter_mm\thalf_angle_rad\t\
major_radius_mm\tminor_radius_mm\taxial_min_mm\taxial_max_mm\taxial_extent_mm\t\
angular_sweep_rad\tsweep_full\tarea_mm2\tboundary_length_mm\tseam_length_mm\tbbox_min_x_mm\t\
bbox_min_y_mm\tbbox_min_z_mm\tbbox_max_x_mm\tbbox_max_y_mm\tbbox_max_z_mm\tbbox_kind\t\
orientation\tstatus\tunavailable\tcaveats";

#[cfg(feature = "step")]
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/measure")
        .join(name)
}

fn cadmpeg() -> Command {
    Command::cargo_bin("cadmpeg").unwrap()
}

/// Run `measure ARGS --json`, require exit 0 and parse stdout.
fn measure_json(args: &[&str]) -> Value {
    let output = cadmpeg()
        .arg("measure")
        .args(args)
        .arg("--json")
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

#[cfg(feature = "step")]
/// Run `measure ARGS`, require exit 0 and return stdout.
fn measure_text(args: &[&str]) -> String {
    let output = cadmpeg().arg("measure").args(args).output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[cfg(any(feature = "step", feature = "iges", feature = "fcstd"))]
/// Assert a measured quantity within its numeric error plus the test floor
/// relative to `max(|expected|, scale)`.
fn assert_measured(quantity: &Value, expected: f64, scale: f64) {
    assert_eq!(quantity["status"], "measured", "{quantity:#}");
    let value = quantity["value"].as_f64().unwrap();
    let numeric = quantity["numeric_error"].as_f64().unwrap();
    let bound = numeric + TEST_ABSOLUTE_FLOOR * expected.abs().max(scale);
    assert!(
        (value - expected).abs() <= bound,
        "{value} differs from {expected} by more than {bound}"
    );
}

#[cfg(feature = "step")]
fn assert_point(quantity: &Value, expected: [f64; 3], scale: f64) {
    assert_eq!(quantity["status"], "measured", "{quantity:#}");
    let numeric = quantity["numeric_error"].as_f64().unwrap();
    for (axis, expected) in ["x", "y", "z"].into_iter().zip(expected) {
        let value = quantity["value"][axis].as_f64().unwrap();
        let bound = numeric + TEST_ABSOLUTE_FLOOR * expected.abs().max(scale);
        assert!(
            (value - expected).abs() <= bound,
            "{axis}: {value} differs from {expected} by more than {bound}"
        );
    }
}

#[cfg(feature = "step")]
fn assert_box(quantity: &Value, min: [f64; 3], max: [f64; 3], scale: f64) {
    assert_eq!(quantity["status"], "measured", "{quantity:#}");
    assert_eq!(quantity["kind"], "tight");
    let numeric = quantity["numeric_error"].as_f64().unwrap();
    for (corner, expected) in [("min", min), ("max", max)] {
        for (axis, expected) in ["x", "y", "z"].into_iter().zip(expected) {
            let value = quantity[corner][axis].as_f64().unwrap();
            let bound = numeric + TEST_ABSOLUTE_FLOOR * expected.abs().max(scale);
            assert!(
                (value - expected).abs() <= bound,
                "{corner}.{axis}: {value} differs from {expected} by more than {bound}"
            );
        }
    }
}

/// The only body row of a report.
fn only_body(report: &Value) -> &Value {
    let bodies = report["measure"]["bodies"].as_array().unwrap();
    assert_eq!(bodies.len(), 1, "{bodies:#?}");
    &bodies[0]
}

#[cfg(feature = "fcstd")]
/// Assert the defect-agnostic volume form: either the volume is measured
/// and equals `expected`, or it is unavailable with
/// `volume.orientation-conflict` and the candidate under `reading` equals
/// `expected`.
fn assert_volume_or_candidate(volume: &Value, reading: &str, expected: f64) {
    if volume["status"] == "measured" {
        assert_measured(volume, expected, 1.0);
        return;
    }
    assert_eq!(
        volume["reason"], "volume.orientation-conflict",
        "{volume:#}"
    );
    let candidate = volume["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| candidate["reading"] == reading)
        .unwrap_or_else(|| panic!("no {reading} candidate in {volume:#}"));
    let value = candidate["value"].as_f64().unwrap();
    let bound = candidate["numeric_error"].as_f64().unwrap() + TEST_ABSOLUTE_FLOOR * expected.abs();
    assert!(
        (value - expected).abs() <= bound,
        "{reading}: {value} differs from {expected} by more than {bound}"
    );
}

#[cfg(feature = "step")]
fn face<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["measure"]["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|face| face["face"] == id)
        .unwrap_or_else(|| panic!("no face row {id}"))
}

#[cfg(feature = "step")]
fn dump(input: &Path, output: &Path, extra: &[&str]) {
    cadmpeg()
        .arg("dump")
        .arg(input)
        .arg("-o")
        .arg(output)
        .args(extra)
        .assert()
        .success();
}

#[cfg(feature = "fcstd")]
mod freecad;
#[cfg(feature = "iges")]
mod iges;

#[cfg(feature = "step")]
#[test]
fn measures_plate_with_hole() {
    let plate = fixture("measure_plane_hole.p21");
    let report = measure_json(&[plate.to_str().unwrap(), "--face", "face#69"]);
    let row = face(&report, "step:data:face#69");
    let measures = &row["measures"];
    let scale = 40.0_f64;
    assert_measured(
        &measures["area"],
        1600.0 - 100.0 * std::f64::consts::PI,
        scale * scale,
    );
    assert_measured(
        &measures["boundary_length"],
        160.0 + 20.0 * std::f64::consts::PI,
        scale,
    );
    assert_box(&measures["bbox"], [0.0; 3], [40.0, 40.0, 0.0], scale);
    assert_eq!(measures["region"], "nesting");
    let loops = measures["loops"].as_array().unwrap();
    assert_eq!(loops.len(), 2);
    let outer = loops
        .iter()
        .find(|entry| entry["loop"] == "step:data:loop#64-face-69")
        .unwrap();
    let inner = loops
        .iter()
        .find(|entry| entry["loop"] == "step:data:loop#67-face-69")
        .unwrap();
    assert_eq!(outer["role"], "outer");
    assert_eq!(inner["role"], "inner");
    assert_eq!(outer["listed_order"], "traversal");
    let length = |entry: &Value| entry["length"].as_f64().unwrap();
    assert!((length(outer) - 160.0).abs() <= TEST_ABSOLUTE_FLOOR * 160.0);
    let hole = 20.0 * std::f64::consts::PI;
    assert!((length(inner) - hole).abs() <= TEST_ABSOLUTE_FLOOR * scale);
    assert_eq!(row["orientation"]["consistent"], true);
    assert_eq!(row["status"], "measured");
    assert_eq!(report["measure"]["input"]["decode_depth"], "full");
    assert_eq!(report["measure"]["input"]["origin"], "decoded");

    let text = measure_text(&[plate.to_str().unwrap(), "--face", "face#69"]);
    assert!(text.contains("face step:data:face#69"), "{text}");
    assert!(text.contains("region        nesting"), "{text}");
}

#[cfg(feature = "step")]
#[test]
fn measures_full_cylinder_side_face() {
    let solid = fixture("measure_cylinder_solid.p21");
    let report = measure_json(&[solid.to_str().unwrap(), "--face", "face#46"]);
    assert_eq!(report["measure"]["input"]["decode_depth"], "full");
    let row = face(&report, "step:data:face#46");
    let parameters = &row["carrier"]["parameters"];
    let scale = 30.0;
    assert_measured(&parameters["radius"], 10.0, scale);
    assert_eq!(parameters["radius"]["method"], "source");
    assert_eq!(
        parameters["radius"]["provenance"][0]["field"],
        "geometry.radius"
    );
    assert_eq!(
        parameters["radius"]["provenance"][0]["exactness"],
        "byte_exact"
    );
    assert_measured(&parameters["diameter"], 20.0, scale);
    let measures = &row["measures"];
    let pi = std::f64::consts::PI;
    assert_measured(&measures["area"], 600.0 * pi, 20.0 * scale);
    assert_eq!(measures["area"]["method"], "boundary_quadrature");
    assert_measured(&measures["boundary_length"], 40.0 * pi, scale);
    assert_measured(&measures["seam_length"], 30.0, scale);
    let axial = &measures["axial_range"];
    assert_eq!(axial["status"], "measured");
    assert!(axial["min"].as_f64().unwrap().abs() <= TEST_ABSOLUTE_FLOOR * scale);
    assert!((axial["max"].as_f64().unwrap() - 30.0).abs() <= TEST_ABSOLUTE_FLOOR * scale);
    assert_measured(&measures["axial_extent"], 30.0, scale);
    let sweep = &measures["angular_sweep"];
    assert_eq!(sweep["full"], true);
    assert_measured(sweep, std::f64::consts::TAU, 1.0);
    assert_box(
        &measures["bbox"],
        [-10.0, -10.0, 0.0],
        [10.0, 10.0, 30.0],
        scale,
    );
    assert_eq!(measures["loops"][0]["listed_order"], "traversal");
    assert_eq!(row["status"], "measured");
}

#[cfg(feature = "step")]
#[test]
fn measures_trimmed_patch() {
    let patch = fixture("measure_cylinder_patch.p21");
    let report = measure_json(&[patch.to_str().unwrap(), "--faces"]);
    let row = face(&report, "step:data:face#56");
    let measures = &row["measures"];
    let pi = std::f64::consts::PI;
    let scale = 30.0;
    assert_measured(&measures["area"], 150.0 * pi, 10.0 * scale);
    assert_measured(&measures["boundary_length"], 10.0 * pi + 60.0, scale);
    let sweep = &measures["angular_sweep"];
    assert_eq!(sweep["full"], false);
    assert_measured(sweep, pi / 2.0, 1.0);
    let interval = &sweep["intervals"][0];
    assert!(interval[0].as_f64().unwrap().abs() <= TEST_ABSOLUTE_FLOOR);
    assert!((interval[1].as_f64().unwrap() - pi / 2.0).abs() <= TEST_ABSOLUTE_FLOOR);
    assert_eq!(sweep["intervals"].as_array().unwrap().len(), 1);
    assert_measured(&measures["axial_extent"], 30.0, scale);
    assert_box(&measures["bbox"], [0.0; 3], [10.0, 10.0, 30.0], scale);
    assert_eq!(row["status"], "measured");

    let report = measure_json(&[patch.to_str().unwrap()]);
    let body = only_body(&report);
    assert_eq!(body["volume"]["status"], "unavailable");
    assert_eq!(body["volume"]["reason"], "volume.shell-open");
    assert_eq!(body["volume"]["category"], "undefined");
    assert_eq!(body["status"], "measured");
}

/// The cylinder solid `r = 10`, `z ∈ [0, 30]`.
#[cfg(feature = "step")]
#[test]
fn measures_cylinder_body() {
    let solid = fixture("measure_cylinder_solid.p21");
    let report = measure_json(&[solid.to_str().unwrap()]);
    let body = only_body(&report);
    let pi = std::f64::consts::PI;
    assert_measured(&body["volume"], 3000.0 * pi, 1.0);
    assert_point(&body["centroid"], [0.0, 0.0, 15.0], 30.0);
    assert_measured(&body["area"], 800.0 * pi, 1.0);
    assert_eq!(body["orientation"]["consistent"], true);
    assert_eq!(body["status"], "measured");
    let document = &report["measure"]["document"];
    assert_measured(&document["closed_volume"], 3000.0 * pi, 1.0);
    assert!(
        document["unavailable"]
            .as_object()
            .is_none_or(serde_json::Map::is_empty),
        "{document:#}"
    );
    assert_eq!(document["work"]["exhausted"], false);

    let text = measure_text(&[solid.to_str().unwrap()]);
    assert!(text.contains("  volume        9424.77796077 mm³"), "{text}");
    assert!(text.contains("  orientation   consistent;"), "{text}");
}

#[cfg(feature = "step")]
#[test]
fn measures_carrier_parameters() {
    let solid = fixture("measure_cylinder_solid.p21");
    let report = measure_json(&[solid.to_str().unwrap(), "--faces"]);
    let faces = report["measure"]["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 3);
    let scale = 30.0;
    let side = face(&report, "step:data:face#46");
    let carrier = &side["carrier"];
    assert_eq!(carrier["kind"], "cylinder");
    assert_eq!(carrier["analytic"], true);
    let parameters = &carrier["parameters"];
    assert_measured(&parameters["radius"], 10.0, scale);
    assert_eq!(parameters["radius"]["method"], "source");
    assert_eq!(
        parameters["radius"]["provenance"][0]["field"],
        "geometry.radius"
    );
    assert_eq!(
        parameters["radius"]["provenance"][0]["exactness"],
        "byte_exact"
    );
    assert_measured(&parameters["diameter"], 20.0, scale);
    assert_point(&parameters["axis_point"], [0.0; 3], scale);
    assert_eq!(parameters["axis_point"]["method"], "source");
    assert_point(&parameters["axis_direction"], [0.0, 0.0, 1.0], 1.0);
    assert_eq!(parameters["axis_direction"]["method"], "source");
    for (id, height) in [("step:data:face#53", 0.0), ("step:data:face#63", 30.0)] {
        let carrier = &face(&report, id)["carrier"];
        assert_eq!(carrier["kind"], "plane");
        assert_eq!(carrier["analytic"], true);
        assert_point(&carrier["parameters"]["origin"], [0.0, 0.0, height], scale);
        assert_eq!(carrier["parameters"]["origin"]["method"], "source");
        assert_eq!(carrier["parameters"]["normal"]["method"], "source");
        let normal = &carrier["parameters"]["normal"]["value"];
        let z = normal["z"].as_f64().unwrap();
        assert!((z.abs() - 1.0).abs() <= TEST_ABSOLUTE_FLOOR, "{normal}");
    }
}

#[cfg(feature = "step")]
#[test]
fn measures_edges() {
    let solid = fixture("measure_cylinder_solid.p21");
    let report = measure_json(&[solid.to_str().unwrap(), "--edges"]);
    let edges = report["measure"]["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 3);
    let scale = 30.0;
    for edge in edges {
        match edge["kind"].as_str().unwrap() {
            "circle" => {
                assert_measured(&edge["length"], 20.0 * std::f64::consts::PI, scale);
                assert_measured(&edge["parameters"]["radius"], 10.0, scale);
                assert_measured(&edge["arc_angle"], std::f64::consts::TAU, 1.0);
                assert_eq!(edge["closed"], true);
            }
            "line" => {
                assert_measured(&edge["length"], 30.0, scale);
                assert_eq!(edge["closed"], false);
            }
            other => panic!("unexpected edge kind {other}"),
        }
    }
}

#[cfg(feature = "step")]
#[test]
fn tsv_headers_carry_units() {
    let solid = fixture("measure_cylinder_solid.p21");
    let text = measure_text(&[solid.to_str().unwrap(), "--faces", "--tsv"]);
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some(FACE_HEADER));
    let ids: Vec<&str> = lines.map(|line| line.split('\t').next().unwrap()).collect();
    assert_eq!(
        ids,
        [
            "step:data:face#46",
            "step:data:face#53",
            "step:data:face#63"
        ]
    );
    for line in text.lines() {
        assert_eq!(line.split('\t').count(), FACE_HEADER.split('\t').count());
    }
}

#[cfg(feature = "step")]
#[test]
fn body_scopes_face_table() {
    let solid = fixture("measure_cylinder_solid.p21");
    let text = measure_text(&[
        solid.to_str().unwrap(),
        "--body",
        "body#71",
        "--faces",
        "--tsv",
    ]);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], FACE_HEADER);
    assert_eq!(lines.len(), 4, "{text}");
}

#[cfg(feature = "fcstd")]
#[test]
fn face_outside_body_scope() {
    let input = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd");
    cadmpeg()
        .arg("measure")
        .arg(&input)
        .args(["--body", "Box.Shape.brp:34", "--face", "Cut.Shape.brp:41"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("is not in the --body scope"));
}

#[cfg(feature = "step")]
#[test]
fn tsv_requires_one_level() {
    let solid = fixture("measure_cylinder_solid.p21");
    cadmpeg()
        .arg("measure")
        .arg(&solid)
        .args(["--face", "face#46", "--edge", "edge#30", "--tsv"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "--tsv prints one table; select faces or edges, not both",
        ));
}

#[cfg(feature = "step")]
#[test]
fn kind_and_radius_filters() {
    let solid = fixture("measure_cylinder_solid.p21");
    let path = solid.to_str().unwrap();
    for (filter, value, rows) in [
        ("--kind", "cylinder", 1),
        ("--kind", "plane,cylinder", 3),
        ("--radius", "9..11", 1),
        ("--radius", "10", 1),
    ] {
        let text = measure_text(&[path, filter, value, "--tsv"]);
        assert_eq!(text.lines().count(), rows + 1, "{filter} {value}: {text}");
    }
    cadmpeg()
        .args(["measure", path, "--radius", "20..", "--tsv"])
        .assert()
        .success()
        .stdout(format!("{FACE_HEADER}\n"))
        .stderr(predicate::str::contains(
            "note: no face matches --radius 20..",
        ));
}

#[cfg(feature = "step")]
#[test]
fn suffix_resolution() {
    let solid = fixture("measure_cylinder_solid.p21");
    let report = measure_json(&[solid.to_str().unwrap(), "--face", "face#46"]);
    assert_eq!(report["measure"]["faces"][0]["face"], "step:data:face#46");
    cadmpeg()
        .arg("measure")
        .arg(&solid)
        .args(["--face", "3"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("step:data:face#53"))
        .stderr(predicate::str::contains("step:data:face#63"));
    cadmpeg()
        .arg("measure")
        .arg(&solid)
        .args(["--face", "absent", "--edge", "missing"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("model.faces (3 entries)"))
        .stderr(predicate::str::contains("model.edges (3 entries)"));
}

#[cfg(feature = "step")]
#[test]
fn sidecar_path_is_refused() {
    let dir = tempdir().unwrap();
    let cadir = dir.path().join("plate.cadir.json");
    dump(&fixture("measure_plane_hole.p21"), &cadir, &[]);
    let sidecar = dir.path().join("plate.cadir.fidelity.json");
    assert!(sidecar.exists());
    let message = format!(
        "{} is a decode sidecar; pass the CADIR document beside it",
        sidecar.display()
    );
    for command in ["measure", "check"] {
        cadmpeg()
            .arg(command)
            .arg(&sidecar)
            .assert()
            .code(2)
            .stderr(predicate::str::contains(message.as_str()));
    }
}

#[cfg(feature = "step")]
#[test]
fn container_only_is_rejected() {
    cadmpeg()
        .arg("measure")
        .arg(fixture("measure_plane_hole.p21"))
        .arg("--container-only")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--container-only"));
}

#[cfg(feature = "step")]
#[test]
fn report_flag_aliases() {
    let dir = tempdir().unwrap();
    let plate = fixture("measure_plane_hole.p21");
    for flag in ["-o", "--output", "--report"] {
        let report = dir.path().join(format!("report{flag}.json"));
        cadmpeg()
            .arg("measure")
            .arg(&plate)
            .arg(flag)
            .arg(&report)
            .assert()
            .success();
        let value: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
        assert_eq!(value["command"], "measure");
        assert_eq!(value["status"], "ok");
        assert!(value["measure"]["bodies"].is_array());
    }
    let report = dir.path().join("tsv.json");
    let output = cadmpeg()
        .arg("measure")
        .arg(&plate)
        .args(["--faces", "--tsv", "--report"])
        .arg(&report)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().next(), Some(FACE_HEADER));
    let value: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(value["command"], "measure");
    assert_eq!(value["measure"]["faces"].as_array().unwrap().len(), 1);

    let existing = dir.path().join("report--report.json");
    cadmpeg()
        .arg("measure")
        .arg(&plate)
        .arg("--report")
        .arg(&existing)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("pass --force to overwrite"));
    cadmpeg()
        .arg("measure")
        .arg(&plate)
        .arg("--report")
        .arg(&existing)
        .arg("--force")
        .assert()
        .success();
}

#[cfg(feature = "step")]
#[test]
fn service_limits_select_service_allowance() {
    let solid = fixture("measure_cylinder_solid.p21");
    let path = solid.to_str().unwrap();
    let items: usize = 3 + 3;
    for (limits, per_item) in [
        (
            "service",
            cadmpeg_measure::MAX_MEASURE_PASS_WORK_PER_ITEM_SERVICE,
        ),
        ("desktop", cadmpeg_measure::MAX_MEASURE_PASS_WORK_PER_ITEM),
    ] {
        let report = measure_json(&[path, "--limits", limits]);
        let work = &report["measure"]["document"]["work"];
        let expected = cadmpeg_measure::MEASURE_PASS_WORK_FLOOR.max(per_item * items);
        assert_eq!(
            work["allowance"].as_u64(),
            Some(u64::try_from(expected).unwrap())
        );
        assert_eq!(work["exhausted"], false);
    }
}

#[cfg(feature = "step")]
#[test]
fn cadir_with_and_without_sidecar() {
    let dir = tempdir().unwrap();
    let cadir = dir.path().join("plate.cadir.json");
    dump(&fixture("measure_plane_hole.p21"), &cadir, &[]);
    let report = measure_json(&[cadir.to_str().unwrap(), "--face", "face#69"]);
    assert_eq!(report["measure"]["input"]["origin"], "restored");
    let origin = &face(&report, "step:data:face#69")["carrier"]["parameters"]["origin"];
    assert_eq!(origin["provenance"][0]["exactness"], "byte_exact");
    assert_eq!(report["measure"]["caveats"].as_array().unwrap().len(), 0);

    let bare = dir.path().join("bare.cadir.json");
    fs::copy(&cadir, &bare).unwrap();
    let report = measure_json(&[bare.to_str().unwrap(), "--face", "face#69"]);
    assert_eq!(report["measure"]["input"]["origin"], "neutral");
    let origin = &face(&report, "step:data:face#69")["carrier"]["parameters"]["origin"];
    assert_eq!(origin["provenance"][0]["exactness"], "not_recorded");
    let caveats = report["measure"]["caveats"].as_array().unwrap();
    assert!(caveats.iter().any(|caveat| {
        caveat["code"] == "measure.provenance-not-recorded" && caveat["category"] == "undefined"
    }));
}

#[cfg(feature = "step")]
#[test]
fn container_depth_cadir_is_refused() {
    let dir = tempdir().unwrap();
    let cadir = dir.path().join("container.cadir.json");
    dump(
        &fixture("measure_plane_hole.p21"),
        &cadir,
        &["--container-only"],
    );
    let output = cadmpeg()
        .arg("measure")
        .arg(&cadir)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "refused");
    assert_eq!(value["refusal"]["code"], "insufficient_depth");
    assert_eq!(value["refusal"]["stage"], "plan");
    assert_eq!(value["refusal"]["depth"]["needed"], "geometry");
    assert_eq!(value["refusal"]["depth"]["recorded"], "container");
}

#[cfg(feature = "step")]
#[test]
fn ring_order_caveat_is_visible() {
    let dir = tempdir().unwrap();
    let cadir = dir.path().join("plate.cadir.json");
    dump(&fixture("measure_plane_hole.p21"), &cadir, &[]);
    let mut document: Value = serde_json::from_slice(&fs::read(&cadir).unwrap()).unwrap();
    let outer = document["model"]["loops"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["id"] == "step:data:loop#64-face-69")
        .unwrap();
    let coedges = outer["boundary"]["coedges"].as_array_mut().unwrap();
    assert_eq!(coedges.len(), 4);
    coedges[1..].reverse();
    let reversed = dir.path().join("reversed.cadir.json");
    fs::write(&reversed, serde_json::to_vec(&document).unwrap()).unwrap();

    let report = measure_json(&[reversed.to_str().unwrap(), "--face", "face#69"]);
    let row = face(&report, "step:data:face#69");
    assert_measured(
        &row["measures"]["area"],
        1600.0 - 100.0 * std::f64::consts::PI,
        1600.0,
    );
    let outer = row["measures"]["loops"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["loop"] == "step:data:loop#64-face-69")
        .unwrap();
    assert_eq!(outer["listed_order"], "reversed");
    assert!(row["caveats"].as_array().unwrap().iter().any(|caveat| {
        caveat["code"] == "boundary.ring-order" && caveat["category"] == "input"
    }));
}

#[cfg(feature = "step")]
#[test]
fn json_refusal_envelope() {
    let output = cadmpeg()
        .arg("measure")
        .arg(fixture("measure_plane_hole.p21"))
        .args(["--max-input-bytes", "64", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "refused");
    assert_eq!(value["command"], "measure");
    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    let mut expected = [
        "ir_version",
        "command",
        "generator",
        "status",
        "refusal",
        "decode_report",
        "check_report",
        "export",
    ];
    expected.sort_unstable();
    assert_eq!(keys, expected);
}

/// The 10 mm cube with the sense of one face flipped. The face-sense
/// reading loses that face's pyramid `1000 / 6` twice; the loop-winding
/// reading is the cube.
#[test]
fn orientation_conflict_is_visible() {
    let dir = tempdir().unwrap();
    let mut cube = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let flipped = cube.model.faces[0].id.as_str().to_owned();
    cube.model.faces[0].sense = match cube.model.faces[0].sense {
        cadmpeg_ir::topology::Sense::Forward => cadmpeg_ir::topology::Sense::Reversed,
        cadmpeg_ir::topology::Sense::Reversed => cadmpeg_ir::topology::Sense::Forward,
    };
    let path = crate::support::fixture(dir.path(), "flipped.cadir.json", &cube);
    let report = measure_json(&[path.to_str().unwrap()]);
    let body = only_body(&report);
    let volume = &body["volume"];
    assert_eq!(volume["status"], "unavailable");
    assert_eq!(volume["reason"], "volume.orientation-conflict");
    assert_eq!(volume["category"], "input");
    let candidate = |reading: &str| {
        volume["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["reading"] == reading)
            .and_then(|candidate| candidate["value"].as_f64())
            .unwrap_or_else(|| panic!("no {reading} candidate"))
    };
    assert!((candidate("face_sense") - 2000.0 / 3.0).abs() <= TEST_ABSOLUTE_FLOOR * 1000.0);
    assert!((candidate("loop_winding") - 1000.0).abs() <= TEST_ABSOLUTE_FLOOR * 1000.0);
    assert_eq!(
        body["orientation"]["inconsistent_faces"],
        serde_json::json!([flipped])
    );
    let unavailable = &report["measure"]["document"]["unavailable"]["input"];
    assert!(
        unavailable["volume.orientation-conflict"].as_u64() >= Some(1),
        "{unavailable:#}"
    );
}

/// Assert a measured range equals `[min, max]` within its numeric error and
/// the test floor.
#[cfg(any(feature = "step", feature = "iges"))]
fn assert_range(quantity: &Value, [min, max]: [f64; 2]) {
    assert_eq!(quantity["status"], "measured", "{quantity:#}");
    let numeric = quantity["numeric_error"].as_f64().unwrap();
    for (key, expected) in [("min", min), ("max", max)] {
        let value = quantity[key].as_f64().unwrap();
        let bound = numeric + TEST_ABSOLUTE_FLOOR * expected.abs().max(1.0);
        assert!(
            (value - expected).abs() <= bound,
            "{key}: {value} differs from {expected} by more than {bound}"
        );
    }
}

/// A sphere face that covers its whole carrier: region, area, latitude,
/// sweep, and the body's volume.
#[cfg(any(feature = "step", feature = "iges"))]
fn assert_whole_sphere(path: &Path, radius: f64) {
    let pi = std::f64::consts::PI;
    let report = measure_json(&[path.to_str().unwrap(), "--faces"]);
    let faces = report["measure"]["faces"].as_array().unwrap();
    let [face] = faces.as_slice() else {
        panic!("one face row");
    };
    let measures = &face["measures"];
    assert_eq!(measures["region"], "whole_surface");
    assert_measured(&measures["area"], 4.0 * pi * radius * radius, 1.0);
    let half = std::f64::consts::FRAC_PI_2;
    assert_range(&measures["latitude_range"], [-half, half]);
    assert_eq!(measures["angular_sweep"]["full"], true);
    let report = measure_json(&[path.to_str().unwrap()]);
    assert_measured(
        &only_body(&report)["volume"],
        4.0 * pi * radius.powi(3) / 3.0,
        1.0,
    );
}

/// The cap of the sphere `R = 10` above `z = 6`, bounded by the circle of
/// radius 8: area `2πR²(1 − 0.6)`, complement `2πR²(1 + 0.6)`.
#[cfg(feature = "step")]
#[test]
fn measures_sphere_cap() {
    let cap = fixture("measure_sphere_cap.p21");
    let report = measure_json(&[cap.to_str().unwrap(), "--face", "face#27"]);
    let row = face(&report, "step:data:face#27");
    let measures = &row["measures"];
    let pi = std::f64::consts::PI;
    let area = &measures["area"];
    assert_measured(area, 80.0 * pi, 100.0);
    assert_eq!(area["reading"], "declared");
    let candidates = area["candidates"].as_array().unwrap();
    let [candidate] = candidates.as_slice() else {
        panic!("one candidate");
    };
    assert_eq!(candidate["reading"], "complement");
    let complement = candidate["value"].as_f64().unwrap();
    assert!((complement - 320.0 * pi).abs() <= TEST_ABSOLUTE_FLOOR * 400.0 * pi);
    assert!(row["caveats"].as_array().unwrap().iter().any(|caveat| {
        caveat["code"] == "orientation.unconfirmed" && caveat["category"] == "unsupported"
    }));
    assert_measured(&measures["boundary_length"], 16.0 * pi, 10.0);
    assert_range(
        &measures["latitude_range"],
        [0.6_f64.asin(), std::f64::consts::FRAC_PI_2],
    );
    assert_eq!(measures["angular_sweep"]["full"], true);
    let bbox = &measures["bbox"];
    assert_eq!(bbox["status"], "measured");
    for (corner, expected) in [("min", [-8.0, -8.0, 6.0]), ("max", [8.0, 8.0, 10.0])] {
        for (axis, expected) in ["x", "y", "z"].into_iter().zip(expected) {
            let value = bbox[corner][axis].as_f64().unwrap();
            assert!(
                (value - expected).abs() <= TEST_ABSOLUTE_FLOOR * 10.0,
                "{corner}.{axis}: {value}"
            );
        }
    }
    assert!(row["orientation"]["consistent"].is_null());
}

/// The sphere of radius 5 bounded by two pole vertex loops.
#[cfg(feature = "step")]
#[test]
fn vertex_loop_sphere() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cadmpeg-codec-step/tests/fixtures/ap242_vertex_loop.p21");
    assert_whole_sphere(&path, 5.0);
}
