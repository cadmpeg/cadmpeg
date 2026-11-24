// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
use std::io::{Cursor, Write};

use serde_json::{json, Value};

use super::projection::{face_sketch_bundle, kinds, read};

fn with_sketch_matrix(bytes: Vec<u8>, matrix: &Value) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new_append(Cursor::new(bytes)).unwrap();
    zip.start_file(
        "partstudios/studio/sketches.json",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    let response = json!({"sketches":[{"featureId":"side-sketch","sketchMatrix":matrix}]});
    zip.write_all(&serde_json::to_vec(&response).unwrap())
        .unwrap();
    zip.finish().unwrap().into_inner()
}

// The face plane is z = 0.005 m. Matrix columns are the x axis, y axis, normal and origin.
#[test]
fn face_sketch_takes_its_frame_from_the_response_matrix() {
    for (matrix, expected) in [
        (
            json!([
                0.0, -1.0, 0.0, 0.02, 1.0, 0.0, 0.0, 0.03, 0.0, 0.0, 1.0, 0.005, 0.0, 0.0, 0.0, 1.0
            ]),
            json!({"origin":{"x":20.0,"y":30.0,"z":5.0},"normal":{"x":0.0,"y":0.0,"z":1.0},
                "u_axis":{"x":0.0,"y":1.0,"z":0.0}}),
        ),
        (
            json!([
                1.0, 0.0, 0.0, -0.01, 0.0, -1.0, 0.0, 0.04, 0.0, 0.0, -1.0, 0.005, 0.0, 0.0, 0.0,
                1.0
            ]),
            json!({"origin":{"x":-10.0,"y":40.0,"z":5.0},"normal":{"x":0.0,"y":0.0,"z":-1.0},
                "u_axis":{"x":1.0,"y":0.0,"z":0.0}}),
        ),
    ] {
        let bytes = with_sketch_matrix(face_sketch_bundle([0.0, 0.0, 1.0], true), &matrix);
        let (wire, report) = read(&bytes);
        assert_eq!(
            kinds(&wire),
            ["sketch", "extrude", "sketch"],
            "{:?}",
            report.losses()
        );
        assert_eq!(wire["sketches"][1]["placement"]["frame"], expected);
    }
}

const IDENTITY: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.005, 0.0, 0.0, 0.0, 1.0,
];

fn with_sketches_bytes(bytes: Vec<u8>, member: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new_append(Cursor::new(bytes)).unwrap();
    zip.start_file(
        "partstudios/studio/sketches.json",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(member).unwrap();
    zip.finish().unwrap().into_inner()
}

fn assert_native(bytes: &[u8], detail: &str) {
    let (wire, report) = read(bytes);
    assert_eq!(
        kinds(&wire),
        ["sketch", "extrude", "native"],
        "{:?}",
        report.losses()
    );
    assert_eq!(wire["sketches"].as_array().unwrap().len(), 1);
    assert!(
        report
            .losses()
            .iter()
            .any(|loss| loss.detail.contains(detail)),
        "{detail}: {:?}",
        report.losses()
    );
}

#[test]
fn face_sketch_matrix_off_the_face_plane_or_malformed_keeps_the_sketch_native() {
    for (matrix, detail) in [
        // Origin 1 mm above the face plane.
        (
            json!([
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.006, 0.0, 0.0, 0.0, 1.0
            ]),
            "contradicts its planar face",
        ),
        // Normal along y, not collinear with the face normal.
        (
            json!([
                1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.005, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0
            ]),
            "contradicts its planar face",
        ),
        // Left-handed rotation.
        (
            json!([
                1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.005, 0.0, 0.0, 0.0, 1.0
            ]),
            "needs an orthonormal rotation",
        ),
    ] {
        let bytes = with_sketch_matrix(face_sketch_bundle([0.0, 0.0, 1.0], true), &matrix);
        assert_native(&bytes, detail);
    }
}

#[test]
fn face_sketch_without_a_usable_attachment_row_keeps_the_sketch_native() {
    let base = || face_sketch_bundle([0.0, 0.0, 1.0], true);
    let unresolved = "attachment translation and rotation unresolved";
    assert_native(&base(), unresolved);
    let other_row = json!({"sketches":[{"featureId":"other","sketchMatrix":IDENTITY}]});
    assert_native(
        &with_sketches_bytes(base(), &serde_json::to_vec(&other_row).unwrap()),
        unresolved,
    );
    let row = json!({"featureId":"side-sketch","sketchMatrix":IDENTITY});
    let repeated = json!({"sketches":[row, row]});
    assert_native(
        &with_sketches_bytes(base(), &serde_json::to_vec(&repeated).unwrap()),
        "attachment identity repeats",
    );
    assert_native(
        &with_sketches_bytes(base(), b"not json"),
        "the member bytes",
    );
}

#[test]
fn face_sketch_plane_object_takes_precedence_over_the_response_matrix() {
    let stored = crate::test_support::review::edit(
        &face_sketch_bundle([0.0, 0.0, 1.0], true),
        |path, response| {
            if path.ends_with("features.json") {
                response["features"][2]["plane"] = json!({
                    "origin":{"x":0.04,"y":0.07,"z":0.005},
                    "normal":{"x":0.0,"y":0.0,"z":1.0},
                    "x":{"x":0.0,"y":1.0,"z":0.0}
                });
            }
        },
    );
    let matrix = json!([
        1.0, 0.0, 0.0, -0.01, 0.0, 1.0, 0.0, 0.04, 0.0, 0.0, 1.0, 0.005, 0.0, 0.0, 0.0, 1.0
    ]);
    let (wire, report) = read(&with_sketch_matrix(stored, &matrix));
    assert_eq!(
        kinds(&wire),
        ["sketch", "extrude", "sketch"],
        "{:?}",
        report.losses()
    );
    assert_eq!(
        wire["sketches"][1]["placement"]["frame"],
        json!({"origin":{"x":40.0,"y":70.0,"z":5.0},"normal":{"x":0.0,"y":0.0,"z":1.0},
            "u_axis":{"x":0.0,"y":1.0,"z":0.0}})
    );
}
