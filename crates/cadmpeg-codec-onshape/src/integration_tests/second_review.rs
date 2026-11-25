// SPDX-License-Identifier: Apache-2.0
//! Stored construction and timeline regression controls.
use super::projection::{compact, replace_query, stored_thin};
use crate::reader::id;
use crate::test_support::fixture;
use crate::test_support::review::{
    decode, edit, members, parameter, two_body_step, validate, wire,
};
use base64::Engine;
use serde_json::{json, Value};
use std::io::Write;

fn kind(wire: &Value, ordinal: usize) -> &Value {
    &wire["model"]["features"][ordinal]["definition"]["value"]["kind"]
}
fn loss(result: &cadmpeg_ir::codec::DecodeResult, code: &str) {
    assert!(
        result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.to_string() == code),
        "{:?}",
        result.report().losses
    );
}

#[test]
fn proved_thin_operands_have_a_native_model_companion_and_a_typed_design_wall() {
    for open in [false, true] {
        for flip in [false, true] {
            for thickness in [0.3, 0.7] {
                let bytes = edit(&stored_thin(open, flip, false), |path, response| {
                    if path.ends_with("features.json") {
                        parameter(&mut response["features"][1], "thickness1")["expression"] =
                            json!(format!("{thickness} mm"));
                    }
                });
                let result = decode(&bytes);
                let wire = wire(&result);
                assert_eq!(kind(&wire, 1), "native");
                assert_eq!(
                    wire["model"]["features"][1]["dependencies"],
                    json!([id("feature", "Base sketch")])
                );
                assert_eq!(
                    wire["model"]["features"][1]["outputs"],
                    wire["design"]["scopes"][0]["current"][0]["bodies"]
                );
                let operation = &wire["design"]["features"][1]["definition"]["operation"];
                assert_eq!(operation["kind"], "extrude");
                let wall = &operation["value"]["output"]["body"]["thickness"];
                assert_eq!(wall["value"]["value"]["value"], thickness);
                assert_eq!(
                    wall["value"]["side"]["kind"],
                    match (open, flip) {
                        (true, false) => "right",
                        (true, true) => "left",
                        (false, false) => "outside",
                        (false, true) => "inside",
                    }
                );
                loss(&result, "onshape/extrude.thin-model-not-supported");
                validate(result);
            }
        }
    }
}

fn thin_then(later: &str) -> Vec<u8> {
    let bytes = edit(&stored_thin(false, false, false), |path, response| {
        if path.ends_with("features.json") {
            let features = response["features"].as_array_mut().unwrap();
            let mut feature = if later == "new-body" {
                features[1].clone()
            } else {
                json!({"featureType":"fillet","parameters":[
                    {"parameterId":"entities","queries":[{"deterministicIds":["upright"]}]},
                    {"parameterId":"crossSection","value":"CIRCULAR"},
                    {"parameterId":"radius","expression":"0.2 mm"}
                ]})
            };
            feature["featureId"] = json!(later);
            if later == "new-body" {
                parameter(&mut feature, "bodyType")["value"] = json!("SOLID");
                parameter(&mut feature, "wallShape")["parameterId"] = json!("entities");
            }
            features.push(feature);
            response["rollbackIndex"] = json!(3);
        }
    });
    if later == "new-body" {
        members(&bytes, |members| {
            members.insert("partstudios/studio/export.step".into(), two_body_step());
        })
    } else {
        bytes
    }
}

#[test]
fn final_thin_scope_follows_the_complete_source_timeline() {
    for later in ["modifier", "new-body"] {
        let result = decode(&thin_then(later));
        let wire = wire(&result);
        assert_eq!(
            result.ir().geometry.bodies.len(),
            if later == "new-body" { 2 } else { 1 }
        );
        let members = &wire["design"]["containers"][0]["value"]["members"];
        assert_eq!(
            members,
            &json!(["Base sketch", "Plate", later]
                .map(|name| json!({"kind":"feature","value":id("feature", name)})))
        );
        let current = &wire["design"]["scopes"][0]["current"][0];
        assert_eq!(
            current["at"],
            json!({"kind":"after","value":{"kind":"feature","value":id("feature", later)}})
        );
        assert_eq!(
            current["bodies"].as_array().unwrap().len(),
            if later == "new-body" { 2 } else { 1 }
        );
        assert_eq!(
            current["bodies"],
            wire["model"]["product_definitions"][0]["bodies"]
        );
        assert_eq!(
            wire["design"]["features"][2]["definition"]["operation"]["kind"],
            "native"
        );
        validate(result);
        let bytes = edit(&thin_then(later), |path, response| {
            if path.ends_with("features.json") {
                for feature in response["features"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .skip(1)
                {
                    feature["parameters"]
                        .as_array_mut()
                        .unwrap()
                        .retain(|p| p["parameterId"] != "domain");
                }
            }
        });
        let result = decode(&bytes);
        assert!(result.ir().design.scopes.is_empty());
        loss(&result, "onshape/extrude.thin-not-supported");
        validate(result);
    }
}

fn changed_edge(producer: Option<&str>, compact: bool) -> Vec<u8> {
    let bytes = edit(&fixture("plate-fillet"), |path, response| {
        if path.ends_with("features.json") {
            let features = response["features"].as_array_mut().unwrap();
            let mut independent = features[0].clone();
            independent["featureId"] = json!("independent");
            let mut modifier = features[1].clone();
            modifier["featureId"] = json!("modifier");
            parameter(&mut modifier, "operationType")["value"] = json!("REMOVE");
            features.insert(2, independent);
            features.insert(3, modifier);
            let query = &mut parameter(&mut features[4], "entities")["queries"][0];
            if let Some(producer) = producer {
                if compact {
                    replace_query(
                        query,
                        &json!({"operationId":[format!("{producer}.opFillet")]}),
                    );
                } else {
                    query["featureId"] = json!(producer);
                }
            }
        }
    });
    members(&bytes, |members| {
        let current = members
            .remove("partstudios/studio/bodydetails/2.json")
            .unwrap();
        let mut before: Value = cadmpeg_core::serde_erase::json::from_slice(&current).unwrap();
        before["bodies"][0]["edges"][0]["geometry"]["endPoint"]["z"] = json!(0.001);
        before["bodies"][0]["edges"][0]["geometry"]["midPoint"]["z"] = json!(0.0005);
        members.insert(
            "partstudios/studio/bodydetails/3.json".into(),
            cadmpeg_core::serde_erase::json::to_vec(&before).unwrap(),
        );
        members.insert("partstudios/studio/bodydetails/4.json".into(), current);
        members.insert(
            "partstudios/studio/bodydetails/index.json".into(),
            b"[3,4]".to_vec(),
        );
    })
}

#[test]
fn explicit_provenance_agrees_with_the_selected_stored_topology_revision() {
    for compact in [false, true] {
        for producer in [None, Some("modifier")] {
            let result = decode(&changed_edge(producer, compact));
            let wire = wire(&result);
            assert_eq!(kind(&wire, 4), "fillet", "{:?}", result.report().losses);
            assert_eq!(
                wire["model"]["features"][4]["dependencies"],
                json!([id("feature", "modifier")])
            );
            validate(result);
        }
    }
}

#[test]
fn superseded_unrelated_and_unproved_explicit_producers_refuse_history() {
    for compact in [false, true] {
        for producer in ["Plate", "independent", "Base sketch"] {
            let result = decode(&changed_edge(Some(producer), compact));
            assert_eq!(kind(&wire(&result), 4), "native");
            loss(&result, "onshape/history.not-supported");
            validate(result);
        }
        let bytes = members(&changed_edge(Some("modifier"), compact), |members| {
            members.remove("partstudios/studio/bodydetails/3.json");
            members.insert(
                "partstudios/studio/bodydetails/index.json".into(),
                b"[4]".to_vec(),
            );
        });
        let result = decode(&bytes);
        assert_eq!(kind(&wire(&result), 4), "native");
        loss(&result, "onshape/history.not-supported");
        validate(result);
    }
}

fn cap_frame(start: bool, sense: f64, missing: Option<&str>) -> Vec<u8> {
    edit(&fixture("plate-fillet"), |path, response| {
        if path.ends_with("features.json") {
            let features = response["features"].as_array_mut().unwrap();
            features.truncate(2);
            let mut sketch = features[0].clone();
            sketch["featureId"] = json!("cap-sketch");
            sketch["plane"] = json!({"origin":{"x":0.04,"y":0.07,"z":if start {0.0} else {0.002}},
                "normal":{"x":0.0,"y":0.0,"z":sense},"x":{"x":0.0,"y":1.0,"z":0.0}});
            let query = &mut parameter(&mut sketch, "sketchPlane")["queries"][0];
            *query = json!({});
            let cap = json!({"entityType":"FACE","historyType":"CREATION","queryType":"CAP_FACE","operationId":["Plate.opExtrude"],"isStart":start});
            let literal = format!("%{}", compact(&cap));
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(literal.as_bytes()).unwrap();
            let encoded = format!(
                "&{:x}${}",
                literal.len(),
                base64::engine::general_purpose::STANDARD.encode(encoder.finish().unwrap())
            );
            query["queryString"] = json!(format!(
                "query=qCompressed(1.0,{},id);",
                cadmpeg_core::serde_erase::json::to_string(&encoded).unwrap()
            ));
            if let Some(missing) = missing {
                if missing == "plane" {
                    sketch.as_object_mut().unwrap().remove(missing);
                } else {
                    sketch["plane"].as_object_mut().unwrap().remove(missing);
                }
            }
            features.push(sketch);
        }
    })
}

#[test]
fn complete_stored_cap_frames_keep_both_ends_and_attachment_senses() {
    for start in [false, true] {
        for sense in [-1.0, 1.0] {
            let result = decode(&cap_frame(start, sense, None));
            let wire = wire(&result);
            assert_eq!(kind(&wire, 2), "sketch", "{:?}", result.report().losses);
            assert_eq!(
                wire["model"]["features"][2]["dependencies"],
                json!([id("feature", "Plate")])
            );
            let sketch = wire["model"]["sketches"]
                .as_array()
                .unwrap()
                .iter()
                .find(|sketch| sketch["id"] == id("sketch", "cap-sketch"))
                .unwrap();
            assert_eq!(
                sketch["placement"]["frame"],
                json!({"origin":{"x":40.0,"y":70.0,"z":if start {0.0} else {2.0}},
                "normal":{"x":0.0,"y":0.0,"z":sense},"u_axis":{"x":0.0,"y":1.0,"z":0.0}})
            );
            validate(result);
        }
    }
}

#[test]
fn each_absent_cap_frame_datum_refuses_both_ends_and_senses() {
    for start in [false, true] {
        for sense in [-1.0, 1.0] {
            for missing in ["plane", "origin", "normal", "x"] {
                let result = decode(&cap_frame(start, sense, Some(missing)));
                assert_eq!(kind(&wire(&result), 2), "native");
                loss(&result, "onshape/history.not-supported");
                validate(result);
            }
        }
    }
}

fn propagated_edge(ambiguous: bool) -> Vec<u8> {
    edit(&changed_edge(Some("modifier"), false), |path, response| {
        if path.ends_with("features.json") {
            if ambiguous {
                response["features"][2] = response["features"][1].clone();
                response["features"][2]["featureId"] = json!("independent");
            }
            response["features"][4]["parameters"]
                .as_array_mut()
                .unwrap()
                .push(json!({"parameterId":"tangentPropagation","value":true}));
        }
        if path.ends_with("bodydetails/3.json") || path.ends_with("bodydetails/4.json") {
            let body = &mut response["bodies"][0];
            body["vertices"] = json!([{"id":"a"},{"id":"b"},{"id":"c"}]);
            let edge = &mut body["edges"][0];
            edge["vertices"] = json!(["a", "b"]);
            edge["geometry"]["startVector"] = json!({"z":1.0});
            edge["geometry"]["endVector"] = json!({"z":1.0});
            let next = json!({"id":"propagated","vertices":["b","c"],
                "curve":{"type":"LINE","origin":{"z":0.002},"direction":{"z":1.0}},
                "geometry":{"startPoint":{"z":0.002},"endPoint":{"z":0.004},"midPoint":{"z":0.003},
                    "startVector":{"z":1.0},"endVector":{"z":1.0}}});
            body["edges"].as_array_mut().unwrap().push(next);
        }
    })
}

#[test]
fn tangent_added_edges_require_their_own_stored_producer_proof() {
    let result = decode(&propagated_edge(false));
    let wire = wire(&result);
    assert_eq!(kind(&wire, 4), "fillet", "{:?}", result.report().losses);
    assert_eq!(
        wire["model"]["features"][4]["dependencies"],
        json!([id("feature", "modifier"), id("feature", "Plate")])
    );
    assert_eq!(
        wire["model"]["features"][4]["definition"]["value"]["value"]["groups"][0]["edges"]["value"]
            ["edges"],
        json!(["propagated", "upright"])
    );
    validate(result);
    let result = decode(&propagated_edge(true));
    assert_eq!(
        kind(&crate::test_support::review::wire(&result), 4),
        "native"
    );
    loss(&result, "onshape/history.not-supported");
    validate(result);
}

#[test]
fn mixed_historical_edges_axes_and_faces_reconcile_explicit_provenance() {
    for (operand, key, expected) in [
        ("edge", "entities", "fillet"),
        ("axis", "axis", "revolve"),
        ("face", "endBoundEntityFace", "extrude"),
    ] {
        for compact in [false, true] {
            for producer in ["Plate", "independent"] {
                let bytes = edit(&super::rereview::separated(operand), |path, response| {
                    if path.ends_with("features.json") {
                        let query = &mut parameter(&mut response["features"][3], key)["queries"][0];
                        if compact {
                            replace_query(
                                query,
                                &json!({"operationId":[format!("{producer}.operation")]}),
                            );
                        } else {
                            query["featureId"] = json!(producer);
                        }
                    }
                });
                let result = decode(&bytes);
                assert_eq!(
                    kind(&wire(&result), 3),
                    if producer == "Plate" {
                        expected
                    } else {
                        "native"
                    },
                    "{operand}: {:?}",
                    result.report().losses
                );
                if producer == "independent" {
                    loss(&result, "onshape/history.not-supported");
                }
                validate(result);
            }
        }
    }
}
