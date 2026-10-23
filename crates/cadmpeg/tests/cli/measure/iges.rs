// SPDX-License-Identifier: Apache-2.0
//! `measure` over the in-tree IGES fixtures, with expectations from the
//! vertices and curves the files state.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{assert_measured, measure_json, only_body};

fn iges(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cadmpeg-codec-iges/tests/golden/fixtures")
        .join(name)
}

fn only_face(report: &Value) -> &Value {
    let faces = report["measure"]["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 1);
    &faces[0]
}

#[test]
fn iges_trimmed_plane_with_pcurve_hole() {
    let path = iges("trimmed_plane_inner_loop.igs");
    let report = measure_json(&[path.to_str().unwrap(), "--faces"]);
    let face = only_face(&report);
    let measures = &face["measures"];
    assert_measured(&measures["area"], 0.75, 1.0);
    assert_measured(&measures["boundary_length"], 6.0, 1.0);
    let loops = measures["loops"].as_array().unwrap();
    let roles: Vec<&str> = loops
        .iter()
        .map(|entry| entry["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["outer", "inner"]);
    for entry in loops {
        for coedge in entry["coedges"].as_array().unwrap() {
            assert_eq!(coedge["uv_source"], "pcurve");
        }
    }
    let orientation = &face["orientation"];
    let consistent = orientation["consistent"].as_bool().unwrap();
    if !consistent {
        assert_eq!(orientation["loop_normal"], "mixed");
    }
}

#[test]
fn iges_inconsistent_pcurve_chain() {
    let path = iges("multi_pcurve_boundary.igs");
    let report = measure_json(&[path.to_str().unwrap(), "--faces"]);
    let face = only_face(&report);
    let area = &face["measures"]["area"];
    assert_eq!(area["status"], "unavailable");
    assert_eq!(area["reason"], "boundary.pcurve-curve-mismatch");
    assert_eq!(area["category"], "input");
    let coedge = &face["measures"]["loops"][0]["coedges"][0]["coedge"];
    assert_eq!(&area["entity"], coedge);
}

fn measure_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/measure")
        .join(name)
}

#[test]
fn iges_parameter_domain_surface() {
    let path = iges("parameter_domain_trimmed_surface.igs");
    let report = measure_json(&[path.to_str().unwrap(), "--faces"]);
    let face = only_face(&report);
    let measures = &face["measures"];
    assert_eq!(measures["region"], "whole_surface");
    assert_measured(&measures["area"], 1.0, 1.0);
    let bbox = &measures["bbox"];
    assert_eq!(bbox["status"], "measured");
    assert_eq!(bbox["kind"], "conservative");
    for (corner, expected) in [("min", [0.0, 0.0, 0.0]), ("max", [1.0, 1.0, 0.0])] {
        for (axis, value) in ["x", "y", "z"].into_iter().zip(expected) {
            assert_eq!(bbox[corner][axis].as_f64(), Some(value), "{corner}.{axis}");
        }
    }
    assert_shell_open(&path);
}

/// The body of `path` is open and has no volume.
fn assert_shell_open(path: &Path) {
    let report = measure_json(&[path.to_str().unwrap()]);
    let volume = &only_body(&report)["volume"];
    assert_eq!(volume["status"], "unavailable");
    assert_eq!(volume["reason"], "volume.shell-open");
    assert_eq!(volume["category"], "undefined");
}

/// The tetrahedron with vertices at the origin and the three unit points.
#[test]
fn iges_tetrahedron_volume() {
    let path = iges("explicit_tetrahedron_solid.igs");
    let report = measure_json(&[path.to_str().unwrap()]);
    let body = only_body(&report);
    assert_measured(&body["volume"], 1.0 / 6.0, 1.0);
    let right_faces = 3.0 * 0.5;
    let slanted_face = 3.0_f64.sqrt() / 2.0;
    assert_measured(&body["area"], right_faces + slanted_face, 1.0);
    assert_eq!(body["orientation"]["consistent"], true);
}

/// The right tetrahedron with legs 4 less the void right tetrahedron with
/// legs 0.5: `32/3 − 1/48 = 511/48`.
#[test]
fn iges_void_solid_volume() {
    let path = iges("explicit_void_solid.igs");
    let report = measure_json(&[path.to_str().unwrap()]);
    let body = only_body(&report);
    let shells = body["shells"].as_array().unwrap();
    let roles: Vec<&str> = shells
        .iter()
        .map(|shell| shell["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["outer", "void"]);
    assert!(shells.iter().all(|shell| shell["closed"] == true));
    let volume = &body["volume"];
    if volume["status"] == "measured" {
        assert_measured(volume, 511.0 / 48.0, 1.0);
    } else {
        assert_eq!(volume["reason"], "volume.orientation-conflict");
        let expected = 511.0 / 48.0;
        assert!(
            volume["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|candidate| {
                    let value = candidate["value"].as_f64().unwrap();
                    let bound = candidate["numeric_error"].as_f64().unwrap()
                        + super::TEST_ABSOLUTE_FLOOR * expected;
                    (value - expected).abs() <= bound
                }),
            "{volume:#}"
        );
    }
}

#[test]
fn iges_implicit_outer_on_bspline() {
    let path = measure_fixture("measure_implicit_outer_bspline.igs");
    let report = measure_json(&[path.to_str().unwrap(), "--faces"]);
    let face = only_face(&report);
    let measures = &face["measures"];
    assert_eq!(measures["region"], "domain_exterior");
    assert_measured(&measures["area"], 0.75, 1.0);
    assert_measured(&measures["boundary_length"], 2.0, 1.0);
    assert_shell_open(&path);
}

#[test]
fn iges_implicit_outer_on_plane() {
    let path = measure_fixture("measure_implicit_outer_plane.igs");
    let report = measure_json(&[path.to_str().unwrap(), "--faces"]);
    let area = &only_face(&report)["measures"]["area"];
    assert_eq!(area["status"], "unavailable");
    assert_eq!(area["reason"], "boundary.unbounded-face");
    assert_eq!(area["category"], "undefined");
}

#[test]
fn iges_no_geometry_transferred() {
    let path = iges("null_entity.igs");
    let output = super::cadmpeg()
        .arg("measure")
        .arg(&path)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("note: the decode transferred no geometry"),
        "{stderr}"
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let caveats = report["measure"]["caveats"].as_array().unwrap();
    assert!(caveats.iter().any(|caveat| {
        caveat["code"] == "measure.geometry-not-transferred" && caveat["category"] == "input"
    }));
}

/// The unit sphere bounded by one pole vertex loop.
#[test]
fn iges_vertex_loop_sphere() {
    super::assert_whole_sphere(&iges("explicit_vertex_loop.igs"), 1.0);
}
