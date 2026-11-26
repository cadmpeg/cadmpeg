// SPDX-License-Identifier: Apache-2.0
//! Thin walls carry their Boolean scope, side and thickness controls into the design.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use serde_json::{json, Value};

use super::projection::stored_thin;
use crate::reader::id;
use crate::test_support::review::{decode, edit, members, parameter, validate, wire};

/// A thin extrude over an open line sketch that follows an earlier solid-producing extrude,
/// with one stored body in the state before the thin feature. `reverse` flips the stored
/// line and sets `oppositeDirection` together; `missing` removes one thin parameter.
fn thin_after_producer(
    reverse: bool,
    flip: bool,
    operation: &str,
    missing: Option<&str>,
) -> Vec<u8> {
    let bytes = edit(&stored_thin(true, flip, false), |path, response| {
        if !path.ends_with("features.json") {
            return;
        }
        if reverse {
            let geometry = &mut response["features"][0]["entities"][0]["geometry"];
            geometry["pntX"] = json!(0.02);
            geometry["dirX"] = json!(-1.0);
        }
        let feature = &mut response["features"][1];
        parameter(feature, "operationType")["value"] = json!(operation);
        parameter(feature, "oppositeDirection")["value"] = json!(reverse);
        if operation != "NEW" {
            feature["parameters"].as_array_mut().unwrap().push(
                json!({"parameterId":"booleanScope","queries":[{"deterministicIds":["body"]}]}),
            );
        }
        if let Some(missing) = missing {
            feature["parameters"]
                .as_array_mut()
                .unwrap()
                .retain(|p| p["parameterId"] != missing);
        }
    });
    members(&bytes, |members| {
        let path = "partstudios/studio/features.json";
        let mut features: Value =
            cadmpeg_core::serde_erase::json::from_slice(&members[path]).unwrap();
        let native = features["features"][0]["featureId"].clone();
        let producer = json!({"featureType":"extrude","featureId":"stored-producer","parameters":[
            {"parameterId":"bodyType","value":"SOLID"},{"parameterId":"operationType","value":"NEW"},
            {"parameterId":"entities","queries":[{"btType":"BTMIndividualSketchRegionQuery-140","featureId":native}]},
            {"parameterId":"endBound","value":"BLIND"},{"parameterId":"depth","expression":"2 mm"}]});
        features["rollbackIndex"] = json!(3);
        features["features"]
            .as_array_mut()
            .unwrap()
            .insert(1, producer);
        members.insert(
            path.into(),
            cadmpeg_core::serde_erase::json::to_vec(&features).unwrap(),
        );
        members.insert(
            "partstudios/studio/bodydetails/index.json".into(),
            b"[0,1,2]".to_vec(),
        );
        members.insert(
            "partstudios/studio/bodydetails/0.json".into(),
            br#"{"bodies":[]}"#.to_vec(),
        );
        members.insert(
            "partstudios/studio/bodydetails/1.json".into(),
            br#"{"bodies":[]}"#.to_vec(),
        );
        members.insert(
            "partstudios/studio/bodydetails/2.json".into(),
            br#"{"bodies":[{"id":"body","faces":[],"edges":[],"vertices":[]}]}"#.to_vec(),
        );
    })
}

/// The design record of the thin extrude.
fn plate(output: &Value) -> &Value {
    output["design"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|feature| feature["id"] == id("feature", "Plate"))
        .unwrap()
}

fn has_loss(result: &cadmpeg_ir::codec::DecodeResult, code: &str) -> bool {
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.to_string() == code)
}

#[test]
fn stored_thin_scope_and_controls_map_a_typed_design() {
    for operation in ["NEW", "ADD", "REMOVE", "INTERSECT"] {
        for reverse in [false, true] {
            for flip in [false, true] {
                let result = decode(&thin_after_producer(reverse, flip, operation, None));
                let output = wire(&result);
                let thin = plate(&output);
                let design = &thin["definition"]["operation"]["value"];
                assert_eq!(
                    design["output"]["body"]["thickness"]["value"]["side"]["kind"],
                    if flip { "left" } else { "right" },
                    "{operation} reverse={reverse} flip={flip}: {:?}",
                    result.report().losses
                );
                assert_eq!(design["direction"]["reversed"]["value"], reverse);
                assert!(!has_loss(&result, "onshape/extrude.thin-not-supported"));
                validate(result);
            }
        }
    }
}

#[test]
fn thin_ellipse_profiles_keep_solved_curves_and_design_ownership() {
    let bytes = edit(
        &thin_after_producer(false, false, "NEW", None),
        |path, response| {
            if !path.ends_with("features.json") {
                return;
            }
            response["features"][0]["entities"]
                .as_array_mut()
                .unwrap()
                .truncate(1);
            let entity = &mut response["features"][0]["entities"][0];
            entity["btType"] = json!("BTMSketchCurve-4");
            entity["geometry"] = json!({"btType":"BTCurveGeometryEllipse-1189",
                "xCenter":0.001,"yCenter":0.002,"xDir":1.0,"radius":0.005,"minorRadius":0.003,"clockwise":false});
        },
    );
    let result = decode(&bytes);
    let output = wire(&result);
    let design = &output["design"];
    assert_eq!(
        design["sketches"].as_array().unwrap().len(),
        1,
        "{:?}",
        result.report().losses
    );
    assert_eq!(design["sketch_entities"].as_array().unwrap().len(), 1);
    assert_eq!(design["sketches"][0]["owner"], design["features"][0]["id"]);
    assert_eq!(
        design["sketch_entities"][0]["sketch"],
        design["sketches"][0]["id"]
    );
    assert_eq!(
        design["sketch_entities"][0]["definition"],
        json!({"kind":"free","value":{
            "kind":"ellipse","value":{"center":{"u":1.0,"v":2.0},"major":[1.0,0.0],"radii":[5.0,3.0]}
        }})
    );
    let thin = plate(&output);
    assert_eq!(
        thin["definition"]["operation"]["value"]["profile"]["picks"]["value"][0]["target"]["value"],
        design["sketch_entities"][0]["id"]
    );
    validate(result);
}

#[test]
fn thin_without_a_product_scope_stays_native_with_named_loss() {
    let bytes = members(&stored_thin(true, false, false), |members| {
        members.remove("partstudios/studio/export.step");
    });
    let result = decode(&bytes);
    let output = wire(&result);
    assert_eq!(
        output["model"]["features"][1]["definition"]["value"]["kind"],
        "native"
    );
    assert!(has_loss(&result, "onshape/extrude.thin-not-supported"));
    validate(result);
}

#[test]
fn missing_thin_scope_side_thickness_or_direction_refuses_with_named_loss() {
    for missing in [
        "domain",
        "booleanScope",
        "flipWall",
        "oppositeDirection",
        "thickness1",
        "thickness2",
        "midplane",
    ] {
        let result = decode(&thin_after_producer(false, false, "ADD", Some(missing)));
        assert!(result.ir().design.features.is_empty(), "{missing}");
        assert!(
            has_loss(&result, "onshape/extrude.thin-not-supported"),
            "{missing}: {:?}",
            result.report().losses
        );
        validate(result);
    }
    let bytes = edit(
        &thin_after_producer(false, false, "ADD", None),
        |path, response| {
            if path.ends_with("bodydetails/2.json") {
                response["bodies"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"id":"another-body","faces":[],"edges":[],"vertices":[]}));
            }
        },
    );
    let result = decode(&bytes);
    assert!(result.ir().design.features.is_empty());
    assert!(has_loss(&result, "onshape/extrude.thin-not-supported"));
    validate(result);
}

#[test]
fn stored_symmetric_thickness_maps_with_explicit_wall_controls() {
    for present in [true, false] {
        let bytes = edit(&stored_thin(true, false, false), |path, response| {
            if !path.ends_with("features.json") {
                return;
            }
            let feature = &mut response["features"][1];
            parameter(feature, "midplane")["value"] = json!(true);
            feature["parameters"]
                .as_array_mut()
                .unwrap()
                .retain(|p| p["parameterId"] != "thickness");
            if present {
                feature["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"parameterId":"thickness","expression":"2 mm"}));
            }
        });
        let result = decode(&bytes);
        let output = wire(&result);
        if present {
            assert_eq!(
                plate(&output)["definition"]["operation"]["value"]["output"]["body"]["thickness"]
                    ["kind"],
                "symmetric",
                "{:?}",
                result.report().losses
            );
        } else {
            assert!(result.ir().design.features.is_empty());
            assert!(has_loss(&result, "onshape/extrude.thin-not-supported"));
        }
        validate(result);
    }
}
