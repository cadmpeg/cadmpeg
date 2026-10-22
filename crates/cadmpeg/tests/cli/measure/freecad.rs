// SPDX-License-Identifier: Apache-2.0
//! `measure` over the public `FreeCAD` corpus, with expectations from the
//! authoring scripts (`corpus/freecad_fcstd/author_fixtures.py`). The values
//! hold whatever order the rings are listed in.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{assert_measured, assert_volume_or_candidate, measure_json};

fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/freecad_fcstd/fixtures")
        .join(name)
}

fn faces_of<'a>(report: &'a Value, needle: &str) -> Vec<&'a Value> {
    report["measure"]["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|face| face["face"].as_str().unwrap().contains(needle))
        .collect()
}

/// The `Cut` body: a 30 × 20 × 10 box minus a cylinder of radius 4 and
/// height 16 at (15, 10, 0).
#[test]
fn freecad_cut_faces() {
    let path = corpus("core_design_product.FCStd");
    let report = measure_json(&[path.to_str().unwrap(), "--faces"]);
    let cut = faces_of(&report, "#Cut:");
    let cylinders: Vec<&&Value> = cut
        .iter()
        .filter(|face| face["carrier"]["kind"] == "cylinder")
        .collect();
    let [hole] = cylinders.as_slice() else {
        panic!("one cylinder face");
    };
    let parameters = &hole["carrier"]["parameters"];
    assert_measured(&parameters["radius"], 4.0, 10.0);
    assert_measured(&parameters["diameter"], 8.0, 10.0);
    let measures = &hole["measures"];
    assert_measured(&measures["axial_extent"], 10.0, 10.0);
    assert_eq!(measures["angular_sweep"]["full"], true);
    assert_measured(&measures["area"], 80.0 * PI, 100.0);
    assert_measured(&measures["boundary_length"], 16.0 * PI, 10.0);
    assert_measured(&measures["seam_length"], 10.0, 10.0);
    let holed: Vec<&&Value> = cut
        .iter()
        .filter(|face| {
            face["carrier"]["kind"] == "plane"
                && face["measures"]["loops"].as_array().unwrap().len() == 2
        })
        .collect();
    assert_eq!(holed.len(), 2);
    for face in holed {
        assert_measured(&face["measures"]["area"], 600.0 - 16.0 * PI, 900.0);
    }
}

/// The `Revolved` body: the annulus between radii 4 and 7, height 8,
/// revolved 270°.
#[test]
fn freecad_revolved_faces() {
    let path = corpus("geometry_topology.FCStd");
    let report = measure_json(&[path.to_str().unwrap(), "--faces"]);
    let revolved = faces_of(&report, "#Revolved:");
    let mut radii = Vec::new();
    for face in revolved
        .iter()
        .filter(|face| face["carrier"]["kind"] == "cylinder")
    {
        let sweep = &face["measures"]["angular_sweep"];
        assert_eq!(sweep["full"], false);
        assert_measured(sweep, 1.5 * PI, 1.0);
        radii.push(
            face["carrier"]["parameters"]["radius"]["value"]
                .as_f64()
                .unwrap(),
        );
    }
    radii.sort_by(f64::total_cmp);
    assert_eq!(radii.len(), 2);
    for (radius, expected) in radii.into_iter().zip([4.0, 7.0]) {
        assert!((radius - expected).abs() <= super::TEST_ABSOLUTE_FLOOR * expected);
    }
    let bodies = measure_json(&[path.to_str().unwrap()]);
    let body = bodies["measure"]["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|body| body["body"].as_str().unwrap().contains("#Revolved:"))
        .unwrap();
    assert_measured(&body["area"], 48.0 + 181.5 * PI, 100.0);
}

/// Body volumes from the authoring scripts: `Cut` is the 30 × 20 × 10 box
/// less the cylinder of radius 4 through it; `Cylinder` has radius 4 and
/// height 16; `Revolved` is the annulus between radii 4 and 7, height 8,
/// revolved 270°; `Mirror` is the mirror image of a cylinder of radius 3
/// and height 10.
#[test]
fn freecad_volumes() {
    for (file, needle, expected) in [
        ("core_design_product.FCStd", "#Cut:", 6000.0 - 160.0 * PI),
        ("core_design_product.FCStd", "#Cylinder:", 256.0 * PI),
        ("geometry_topology.FCStd", "#Revolved:", 198.0 * PI),
        ("core_operations.FCStd", "#Mirror:", 90.0 * PI),
    ] {
        let path = corpus(file);
        let report = measure_json(&[path.to_str().unwrap()]);
        let bodies: Vec<&Value> = report["measure"]["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|body| body["body"].as_str().unwrap().contains(needle))
            .collect();
        let [body] = bodies.as_slice() else {
            panic!("one {needle} body in {file}");
        };
        assert_volume_or_candidate(&body["volume"], "loop_winding", expected);
    }
}

/// The `Compound` body: the sphere of radius 4 and the torus `R = 6`,
/// `r = 2` about `(15, 0, 0)`, each one face covering its whole carrier.
#[test]
fn freecad_compound_whole_faces() {
    let path = corpus("geometry_topology.FCStd");
    let report = measure_json(&[path.to_str().unwrap(), "--faces"]);
    let compound = faces_of(&report, "#Compound:");
    let sphere_area = 4.0 * PI * 16.0;
    let torus_area = 4.0 * PI * PI * 6.0 * 2.0;
    for face in &compound {
        assert_eq!(face["measures"]["region"], "whole_surface");
        let expected = match face["carrier"]["kind"].as_str().unwrap() {
            "sphere" => sphere_area,
            "torus" => torus_area,
            other => panic!("unexpected carrier {other}"),
        };
        assert_measured(&face["measures"]["area"], expected, 100.0);
    }
    assert_eq!(compound.len(), 2);
    let report = measure_json(&[path.to_str().unwrap()]);
    let bodies: Vec<&Value> = report["measure"]["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|body| body["body"].as_str().unwrap().contains("#Compound:"))
        .collect();
    let [body] = bodies.as_slice() else {
        panic!("one compound body");
    };
    let sphere_volume = 4.0 * PI * 64.0 / 3.0;
    let torus_volume = 2.0 * PI * PI * 6.0 * 4.0;
    assert_measured(&body["volume"], sphere_volume + torus_volume, 1.0);
}
