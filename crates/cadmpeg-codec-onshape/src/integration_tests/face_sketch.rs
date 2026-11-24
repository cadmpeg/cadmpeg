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

#[test]
fn face_sketch_matrix_off_the_face_plane_or_malformed_keeps_the_sketch_native() {
    for matrix in [
        // Origin 1 mm above the face plane.
        json!([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.006, 0.0, 0.0, 0.0, 1.0]),
        // Normal along y, not parallel to the face normal.
        json!([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.005, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]),
        // Left-handed rotation.
        json!([1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.005, 0.0, 0.0, 0.0, 1.0]),
    ] {
        let bytes = with_sketch_matrix(face_sketch_bundle([0.0, 0.0, 1.0], true), &matrix);
        let (wire, report) = read(&bytes);
        assert_eq!(
            kinds(&wire),
            ["sketch", "extrude", "native"],
            "{:?}",
            report.losses()
        );
        assert_eq!(wire["sketches"].as_array().unwrap().len(), 1);
    }
}
