// SPDX-License-Identifier: Apache-2.0
//! `measure` over the public `FreeCAD` corpus, with expectations from the
//! authoring scripts (`corpus/freecad_fcstd/author_fixtures.py`). The values
//! hold whatever order the rings are listed in.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{assert_measured, measure_json};

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
