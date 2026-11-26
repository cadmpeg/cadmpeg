// SPDX-License-Identifier: Apache-2.0
//! Face sketches keep their attachment native unless the stored frame proves it.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use serde_json::json;

use super::projection::{face_sketch_bundle, stored_attachment};
use crate::test_support::review::{decode, edit, validate, wire};

/// Model feature kinds in timeline order.
fn kinds(wire: &serde_json::Value) -> Vec<&str> {
    wire["model"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["definition"]["value"]["kind"].as_str().unwrap())
        .collect()
}

#[test]
fn tilted_face_plane_stays_unresolved_for_the_reversed_sense() {
    let result = decode(&face_sketch_bundle([0.6, 0.0, 0.8], false));
    let output = wire(&result);
    assert_eq!(kinds(&output), ["sketch", "extrude", "native"]);
    assert_eq!(output["model"]["sketches"].as_array().unwrap().len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.to_string() == "onshape/history.not-supported"
            && loss
                .message
                .contains("attachment translation and rotation unresolved")
    }));
    validate(result);
}

#[test]
fn contradictory_or_off_plane_stored_frames_refuse_the_attachment() {
    for (datum, reason) in [
        ("contradictory", "not in its plane"),
        ("off-plane", "contradicts its planar face"),
    ] {
        let bytes = edit(&stored_attachment(), |path, response| {
            if !path.ends_with("features.json") {
                return;
            }
            let plane = &mut response["features"][2]["plane"];
            if datum == "contradictory" {
                // The stored x axis lies along the stored normal.
                plane["x"] = json!({"x":0.0,"y":0.0,"z":1.0});
            } else {
                // The stored origin leaves the face plane.
                plane["origin"]["z"] = json!(0.006);
            }
        });
        let result = decode(&bytes);
        assert_eq!(
            kinds(&wire(&result))[2],
            "native",
            "{datum}: {:?}",
            result.report().losses
        );
        assert!(
            result.report().losses.iter().any(|loss| {
                loss.code.to_string() == "onshape/history.not-supported"
                    && loss.message.contains(reason)
            }),
            "{datum}: {:?}",
            result.report().losses
        );
        validate(result);
    }
}
