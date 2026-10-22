// SPDX-License-Identifier: Apache-2.0
//! `measure` over the in-tree IGES fixtures, with expectations from the
//! vertices and curves the files state.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{assert_measured, measure_json};

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
