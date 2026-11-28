// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::indexing_slicing, clippy::panic)]
use cadmpeg_core::serde_erase::json;
use std::io::{Cursor, Write};

use cadmpeg_core::json;
use serde_json::Value;

use super::projection::{face_sketch_bundle, kinds, read};

fn with_sketch_matrix(bytes: Vec<u8>, matrix: &Value) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new_append(Cursor::new(bytes)).unwrap();
    zip.start_file(
        "partstudios/studio/sketches.json",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    let response = json!({"sketches":[{"featureId":"side-sketch","sketchMatrix":matrix}]});
    zip.write_all(&json::to_vec(&response).unwrap()).unwrap();
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
        &with_sketches_bytes(base(), &json::to_vec(&other_row).unwrap()),
        unresolved,
    );
    let row = json!({"featureId":"side-sketch","sketchMatrix":IDENTITY});
    let repeated = json!({"sketches":[row, row]});
    assert_native(
        &with_sketches_bytes(base(), &json::to_vec(&repeated).unwrap()),
        "attachment identity repeats",
    );
    assert_native(
        &with_sketches_bytes(base(), b"not json"),
        "omitted the member",
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

// Tolerance for a frame value in millimetres.
const EPS_FRAME_MM: f64 = 1e-9;

fn assert_frame(frame: &Value, origin: [f64; 3], normal: [f64; 3], u_axis: [f64; 3]) {
    for (key, expected) in [("origin", origin), ("normal", normal), ("u_axis", u_axis)] {
        for (axis, value) in ["x", "y", "z"].into_iter().zip(expected) {
            let actual = frame[key][axis].as_f64().unwrap();
            assert!(
                (actual - value).abs() < EPS_FRAME_MM,
                "{key}.{axis}: {actual} != {value}"
            );
        }
    }
}

// Rewrites the sketch plane query of the face sketch to a CAP_FACE query. The
// deterministic face id stays on the query, so the cap face resolves to the stored face.
fn as_cap_face(bytes: &[u8]) -> Vec<u8> {
    crate::test_support::review::edit(bytes, |path, response| {
        if path.ends_with("features.json") {
            let producer = response["features"][1]["featureId"]
                .as_str()
                .unwrap()
                .to_owned();
            super::projection::replace_query(
                &mut response["features"][2]["parameters"][0]["queries"][0],
                &json!({"entityType":"FACE","historyType":"CREATION","queryType":"CAP_FACE",
                    "operationId":[format!("{producer}.opExtrude")],"isStart":false}),
            );
        }
    })
}

// A face plane through (0.01, -0.02, 0.005) m with normal (0.6, 0, 0.8). The matrix
// columns are x = (0,1,0), y = normal x x = (-0.8,0,0.6), the normal and an origin on the plane.
fn tilted_matrix() -> Value {
    json!([0.0, -0.8, 0.6, 0.034, 1.0, 0.0, 0.0, 0.07, 0.0, 0.6, 0.8, -0.013, 0.0, 0.0, 0.0, 1.0])
}

#[test]
fn face_sketch_places_cap_face_and_tilted_face_from_the_stored_matrix() {
    let flat = json!([
        0.0, -1.0, 0.0, 0.04, 1.0, 0.0, 0.0, 0.07, 0.0, 0.0, 1.0, 0.005, 0.0, 0.0, 0.0, 1.0
    ]);
    for cap in [false, true] {
        for (normal, matrix, origin, expected_normal) in [
            ([0.0, 0.0, 1.0], &flat, [40.0, 70.0, 5.0], [0.0, 0.0, 1.0]),
            (
                [0.6, 0.0, 0.8],
                &tilted_matrix(),
                [34.0, 70.0, -13.0],
                [0.6, 0.0, 0.8],
            ),
        ] {
            let mut bytes = face_sketch_bundle(normal, true);
            if cap {
                bytes = as_cap_face(&bytes);
            }
            let (wire, report) = read(&with_sketch_matrix(bytes, matrix));
            assert_eq!(
                kinds(&wire),
                ["sketch", "extrude", "sketch"],
                "cap={cap} normal={normal:?}: {:?}",
                report.losses()
            );
            let frame = &wire["sketches"][1]["placement"]["frame"];
            assert_frame(frame, origin, expected_normal, [0.0, 1.0, 0.0]);
        }
    }
}

#[test]
fn face_sketch_matrix_with_missing_or_non_finite_entries_keeps_the_sketch_native() {
    let element = |index: usize, value: Value| {
        let mut matrix = json!(IDENTITY);
        matrix[index] = value;
        matrix
    };
    for (matrix, detail) in [
        // The x column has length 0.5.
        (element(0, json!(0.5)), "needs an orthonormal rotation"),
        (element(3, Value::Null), "needs finite numbers"),
        (element(3, json!("0.04")), "needs finite numbers"),
        (
            element(3, json!(f64::MAX)),
            "geometry exceeds the finite range",
        ),
    ] {
        let bytes = with_sketch_matrix(face_sketch_bundle([0.0, 0.0, 1.0], true), &matrix);
        assert_native(&bytes, detail);
    }
    let row = json!({"sketches":[{"featureId":"side-sketch"}]});
    assert_native(
        &with_sketches_bytes(
            face_sketch_bundle([0.0, 0.0, 1.0], true),
            &json::to_vec(&row).unwrap(),
        ),
        "sketchMatrix is missing or exceeds the row limit",
    );
}
