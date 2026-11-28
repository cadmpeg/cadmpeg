// SPDX-License-Identifier: Apache-2.0
//! Thin walls carry their Boolean scope, side and thickness controls into the design.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;
use serde_json::Value;

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
            feature["parameters"].as_array_mut().unwrap().extend([
                json!({"parameterId":"defaultScope","value":false}),
                json!({"parameterId":"booleanScope","queries":[{"deterministicIds":["body"]}]}),
            ]);
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
        let mut features: Value = json::from_slice(&members[path]).unwrap();
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
        members.insert(path.into(), json::to_vec(&features).unwrap());
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
                if operation != "NEW" {
                    let targets = &design["output"]["boolean"]["targets"];
                    assert_eq!(targets["kind"], "selected");
                    assert_eq!(
                        targets["picks"]["value"][0]["native"]["value"]["value"],
                        "body"
                    );
                }
                assert!(!has_loss(&result, "onshape/extrude.thin-not-supported"));
                validate(result);
            }
        }
    }
}

#[test]
fn equal_evaluated_setbacks_keep_a_multicurve_thin_wall() {
    let bytes = edit(&stored_thin(true, false, false), |path, response| {
        if !path.ends_with("features.json") {
            return;
        }
        let curves = response["features"][0]["entities"].as_array_mut().unwrap();
        let mut second_curve = curves[0].clone();
        second_curve["entityId"] = json!("next-line");
        second_curve["startPointId"] = json!("next-line.start");
        second_curve["endPointId"] = json!("next-line.end");
        second_curve["geometry"]["pntX"] = json!(0.02);
        curves.push(second_curve);
        let feature = &mut response["features"][1];
        parameter(feature, "wallShape")["queries"] = json!([
            {"featureId":"Base sketch","btType":"BTMIndividualSketchRegionQuery-140"}
        ]);
        parameter(feature, "thickness1")["expression"] = json!("1 mm + 1 mm");
        parameter(feature, "thickness2")["expression"] = json!("2 mm");
    });
    let result = decode(&bytes);
    let operation = crate::test_support::review::design_operation(result.ir(), "Plate");
    assert_eq!(operation["kind"], "extrude", "{:?}", result.report().losses);
    assert_eq!(
        operation["value"]["profile"]["picks"]["value"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        operation["value"]["output"]["body"]["thickness"],
        json!({"kind":"symmetric","value":{"kind":"literal","value":4.0}})
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.local_code() == "history.parameter-not-projected"
            && loss.message.starts_with("parameter thickness1 ")
    }));
    validate(result);
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

/// A Part Studio without its STEP export has no product definition of its
/// own; the design scope states the root definition it spans (§5.1).
#[test]
fn thin_without_a_step_export_keeps_its_design_on_a_stated_root() {
    let bytes = members(&stored_thin(true, false, false), |members| {
        members.remove("partstudios/studio/export.step");
    });
    let result = decode(&bytes);
    let output = wire(&result);
    assert_eq!(
        output["model"]["features"][1]["definition"]["value"]["kind"],
        "native"
    );
    assert_eq!(plate(&output)["definition"]["operation"]["kind"], "extrude");
    assert_eq!(
        plate(&output)["definition"]["operation"]["value"]["output"]["body"]["kind"],
        "thin"
    );
    let [root] = result.ir().model.product_definitions.as_slice() else {
        panic!("one stated root definition");
    };
    assert!(root.is_root());
    assert_eq!(result.ir().design.scopes[0].definition, root.id);
    assert!(has_loss(
        &result,
        "onshape/extrude.thin-model-not-supported"
    ));
    assert!(!has_loss(&result, "onshape/extrude.thin-not-supported"));
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
        assert_eq!(
            crate::test_support::review::design_operation(result.ir(), "Plate"),
            crate::test_support::review::unresolved("extrude"),
            "{missing}"
        );
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
    assert_eq!(
        crate::test_support::review::design_operation(result.ir(), "Plate"),
        crate::test_support::review::unresolved("extrude")
    );
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
            assert_eq!(
                crate::test_support::review::design_operation(result.ir(), "Plate"),
                crate::test_support::review::unresolved("extrude")
            );
            assert!(has_loss(&result, "onshape/extrude.thin-not-supported"));
        }
        validate(result);
    }
}

#[test]
fn accounting_consumes_thin_domain_and_the_stored_direction() {
    for custom_direction in [false, true] {
        let bytes = edit(&stored_thin(true, false, false), |path, response| {
            if path.ends_with("features.json") {
                response["features"][1]["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .extend([
                        json!({"parameterId":"hasExtrudeDirection","value":custom_direction}),
                        json!({"parameterId":"oppositeExtrudeDirection","value":true}),
                    ]);
            }
        });
        let result = decode(&bytes);
        let output = wire(&result);
        let design = &plate(&output)["definition"]["operation"];
        assert_eq!(design["kind"], "extrude");
        assert_eq!(design["value"]["direction"]["reversed"]["value"], true);
        assert!(!result.report().losses.iter().any(|loss| {
            loss.code.local_code() == "history.parameter-not-projected"
                && loss.message.ends_with("; subject: Plate")
        }));
        validate(result);
    }
}
