// SPDX-License-Identifier: Apache-2.0
//! Synthetic regressions for history admission and operand dependencies.
use crate::test_support::review::{decode, edit, members, parameter, validate, wire};
use crate::test_support::{codec, fixture};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::CodecBackend;
use serde_json::Value;

fn kind(wire: &Value, ordinal: usize) -> &Value {
    &wire["model"]["features"][ordinal]["definition"]["value"]["kind"]
}
fn native(result: &cadmpeg_ir::codec::DecodeResult, ordinal: usize) {
    assert_eq!(kind(&wire(result), ordinal), "native");
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code.to_string() == "onshape/history.not-supported" }));
}
fn midpoint_sketch() -> Vec<u8> {
    let bytes = edit(&fixture("plate-fillet"), |path, response| {
        if path.ends_with("features.json") {
            response["features"].as_array_mut().unwrap().truncate(1);
            response["rollbackIndex"] = json!(response["features"].as_array().unwrap().len());
            let sketch = &mut response["features"][0];
            sketch["entities"] = json!([
                {"btType":"BTMSketchPoint-158","entityId":"a","x":0.0,"y":0.0},
                {"btType":"BTMSketchPoint-158","entityId":"b","x":0.001,"y":0.0},
                {"btType":"BTMSketchPoint-158","entityId":"c","x":0.002,"y":0.0}
            ]);
            sketch["constraints"] = json!((0..3)
                .map(|index| json!({
                    "constraintType":"MIDPOINT","entityId":format!("midpoint{index}"),
                    "parameters":[
                        {"parameterId":"localMidpoint","value":"b"},
                        {"parameterId":"localEntity1","value":"a"},
                        {"parameterId":"localEntity2","value":"c"}
                    ]
                }))
                .collect::<Vec<_>>());
        }
    });
    members(&bytes, |members| {
        members.remove("partstudios/studio/export.step");
        members.remove("partstudios/studio/bodydetails/2.json");
        members.insert(
            "partstudios/studio/bodydetails/index.json".into(),
            b"[]".to_vec(),
        );
    })
}

#[test]
fn history_admission_reserves_each_midpoint_output_arena() {
    let result = decode(&midpoint_sketch());
    assert_eq!(kind(&wire(&result), 0), "sketch");
    let model = &result.ir().model;
    assert_eq!(model.sketch_entities.len(), 6);
    assert_eq!(model.sketch_constraints.len(), 9);
    assert_eq!(model.sketches.len(), 1);
    assert_eq!(model.features.len(), 1);
    assert!(model.parameters.is_empty());
    assert!(model.sketch_entities.capacity() >= 6);
    assert!(model.sketch_constraints.capacity() >= 9);
    validate(result);
}

#[test]
fn midpoint_expansion_refuses_the_history_budget_before_emission() {
    let bytes = midpoint_sketch();
    // History admits 17 entities, the design timeline 3 (one feature and its
    // container and scope), the design sketch 7 (the sketch and its six
    // entities), the stated root definition 1, and the member assets 3.
    for ceiling in [15, 17, 18, 20, 21, 27, 28, 29, 31, 32] {
        let mut policy = DecodePolicy::default();
        policy.limits.max_entities = ceiling;
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let result = codec().decode_impl(&ctx, root);
        let refused = match ceiling {
            ..=17 => Some("Onshape history admission"),
            18..=20 => Some("Onshape design timeline"),
            21..=27 => Some("Onshape design sketch"),
            28 => Some("root product definition"),
            29..=31 => Some("Onshape asset"),
            _ => None,
        };
        if let Some(operation) = refused {
            let CodecError::ResourceLimit(limit) = result.unwrap_err() else {
                panic!("every admission must return the caller resource refusal")
            };
            assert_eq!(limit.dimension, ResourceDimension::Entities);
            assert_eq!(limit.operation, operation, "{ceiling}");
        } else {
            let decoded = result.unwrap();
            assert_eq!(decoded.ir.model.sketch_entities.len(), 6);
            assert_eq!(decoded.ir.model.sketch_constraints.len(), 9);
            let [root] = decoded.ir.model.product_definitions.as_slice() else {
                panic!("the decoded history must have one root definition");
            };
            assert!(root.is_root());
        }
    }
}

pub(super) fn separated(kind: &str) -> Vec<u8> {
    let bytes = edit(&fixture("plate-fillet"), |path, response| {
        if path.ends_with("features.json") {
            let features = response["features"].as_array_mut().unwrap();
            let mut independent = features[0].clone();
            independent["featureId"] = json!("independent");
            features.insert(2, independent);
            if kind == "axis" {
                features[3] = json!({"featureId":"consumer","featureType":"revolve","parameters":[
                    {"parameterId":"bodyType","value":"SOLID"},
                    {"parameterId":"operationType","value":"NEW"},
                    {"parameterId":"entities","queries":[{"btType":"BTMIndividualSketchRegionQuery-140","featureId":"independent"}]},
                    {"parameterId":"axis","queries":[{"deterministicIds":["upright"]}]},
                    {"parameterId":"revolveType","value":"FULL"}
                ]});
            } else if kind == "face" {
                features[3] = features[1].clone();
                features[3]["featureId"] = json!("consumer");
                parameter(&mut features[3], "endBound")["value"] = json!("UP_TO_SURFACE");
                features[3]["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({
                        "parameterId":"endBoundEntityFace","queries":[{"deterministicIds":["top"]}]
                    }));
            }
        }
        if path.ends_with("bodydetails/2.json") {
            if kind == "axis" {
                let edge = &mut response["bodies"][0]["edges"][0];
                edge["curve"]["direction"] = json!({"x":0.0,"y":1.0,"z":0.0});
                edge["geometry"]["endPoint"] = json!({"x":0.0,"y":0.002,"z":0.0});
                edge["geometry"]["midPoint"] = json!({"x":0.0,"y":0.001,"z":0.0});
            } else if kind == "face" {
                let points = [
                    [0.0, 0.0, 0.002],
                    [0.02, 0.0, 0.002],
                    [0.02, 0.01, 0.002],
                    [0.0, 0.01, 0.002],
                ];
                let point = |p: [f64; 3]| json!({"x":p[0],"y":p[1],"z":p[2]});
                response["bodies"][0]["edges"] = json!((0..4).map(|index| {
                    let a = points[index];
                    let b = points[(index + 1) % 4];
                    json!({"id":format!("side{index}"),"curve":{"type":"LINE","origin":point(a),"direction":point(std::array::from_fn(|i| b[i]-a[i]))},
                        "geometry":{"startPoint":point(a),"endPoint":point(b),"midPoint":point(std::array::from_fn(|i| f64::midpoint(a[i],b[i])))}})
                }).collect::<Vec<_>>());
                response["bodies"][0]["faces"] = json!([{"id":"top","surface":{"type":"PLANE","origin":point(points[0]),"normal":point([0.0,0.0,1.0])},
                    "loops":[{"isOuter":true,"coedges":(0..4).map(|i| json!({"edgeId":format!("side{i}"),"orientation":true})).collect::<Vec<_>>()}]}]);
            }
        }
    });
    members(&bytes, |members| {
        let state = members
            .remove("partstudios/studio/bodydetails/2.json")
            .unwrap();
        members.insert("partstudios/studio/bodydetails/3.json".into(), state);
        members.insert(
            "partstudios/studio/bodydetails/index.json".into(),
            b"[3]".to_vec(),
        );
    })
}

#[test]
fn deterministic_historical_operands_keep_their_producer_across_an_independent_sketch() {
    for (operand, expected, sources) in [
        ("edge", "fillet", vec!["Plate"]),
        ("axis", "revolve", vec!["independent", "Plate"]),
        ("face", "extrude", vec!["Base sketch", "Plate"]),
    ] {
        let bytes = separated(operand);
        let result = decode(&bytes);
        let wire = crate::test_support::review::wire(&result);
        assert_eq!(
            kind(&wire, 3),
            expected,
            "{operand}: {:?}",
            result.report().losses
        );
        assert_eq!(
            wire["model"]["features"][3]["dependencies"],
            json!(sources
                .iter()
                .map(|source| crate::reader::id("feature", source))
                .collect::<Vec<_>>())
        );
        validate(result);
        let ambiguous = edit(&bytes, |path, response| {
            if path.ends_with("features.json") {
                response["features"][2] = response["features"][1].clone();
                response["features"][2]["featureId"] = json!("independent");
                if operand == "axis" {
                    parameter(&mut response["features"][3], "entities")["queries"][0]
                        ["featureId"] = json!("Base sketch");
                }
            }
        });
        let result = decode(&ambiguous);
        native(&result, 3);
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("producer is ambiguous")));
        validate(result);
        let bracketed = members(&ambiguous, |members| {
            let mut before: Value =
                json::from_slice(&members["partstudios/studio/bodydetails/3.json"]).unwrap();
            before["bodies"][0]["edges"] = json!([]);
            before["bodies"][0]["faces"] = json!([]);
            members.insert(
                "partstudios/studio/bodydetails/2.json".into(),
                json::to_vec(&before).unwrap(),
            );
            members.insert(
                "partstudios/studio/bodydetails/index.json".into(),
                b"[2,3]".to_vec(),
            );
        });
        let bracketed = edit(&bracketed, |path, response| {
            if path.ends_with("features.json") {
                let modifier = &mut response["features"][2];
                parameter(modifier, "operationType")["value"] = json!("REMOVE");
                modifier["parameters"].as_array_mut().unwrap().push(json!({
                    "parameterId":"booleanScope","queries":[{"deterministicIds":["body"]}]
                }));
            }
        });
        let result = decode(&bracketed);
        let wire = crate::test_support::review::wire(&result);
        assert_eq!(
            kind(&wire, 3),
            expected,
            "{operand}: {:?}",
            result.report().losses
        );
        let sources = if operand == "edge" {
            vec!["independent"]
        } else {
            vec!["Base sketch", "independent"]
        };
        assert_eq!(
            wire["model"]["features"][3]["dependencies"],
            json!(sources
                .iter()
                .map(|source| crate::reader::id("feature", source))
                .collect::<Vec<_>>())
        );
        validate(result);
        let unchanged = members(&bracketed, |members| {
            let mut before: Value =
                json::from_slice(&members["partstudios/studio/bodydetails/3.json"]).unwrap();
            before["bodies"][0]["edges"][0]["curve"]["origin"]["x"] = json!(0);
            if operand == "face" {
                before["bodies"][0]["faces"][0]["surface"]["normal"]["z"] = json!(1);
            }
            members.insert(
                "partstudios/studio/bodydetails/2.json".into(),
                json::to_vec(&before).unwrap(),
            );
        });
        let result = decode(&unchanged);
        let wire = crate::test_support::review::wire(&result);
        let original = if operand == "edge" {
            vec!["Plate"]
        } else {
            vec!["Base sketch", "Plate"]
        };
        assert_eq!(
            kind(&wire, 3),
            expected,
            "{operand}: {:?}",
            result.report().losses
        );
        assert_eq!(
            wire["model"]["features"][3]["dependencies"],
            json!(original
                .iter()
                .map(|source| crate::reader::id("feature", source))
                .collect::<Vec<_>>())
        );
        validate(result);
        let changed = members(&bracketed, |members| {
            let mut before: Value =
                json::from_slice(&members["partstudios/studio/bodydetails/3.json"]).unwrap();
            if operand == "face" {
                for edge in before["bodies"][0]["edges"].as_array_mut().unwrap() {
                    let x = edge["curve"]["origin"]["x"].as_f64().unwrap();
                    edge["curve"]["origin"]["x"] = json!(x * 0.5);
                    for key in ["startPoint", "endPoint", "midPoint"] {
                        let x = edge["geometry"][key]["x"].as_f64().unwrap();
                        edge["geometry"][key]["x"] = json!(x * 0.5);
                    }
                }
            } else {
                let component = if operand == "axis" { "y" } else { "z" };
                before["bodies"][0]["edges"][0]["geometry"]["endPoint"][component] = json!(0.001);
                before["bodies"][0]["edges"][0]["geometry"]["midPoint"][component] = json!(0.0005);
            }
            members.insert(
                "partstudios/studio/bodydetails/2.json".into(),
                json::to_vec(&before).unwrap(),
            );
        });
        let result = decode(&changed);
        let wire = crate::test_support::review::wire(&result);
        assert_eq!(
            kind(&wire, 3),
            expected,
            "{operand}: {:?}",
            result.report().losses
        );
        assert_eq!(
            wire["model"]["features"][3]["dependencies"],
            json!(sources
                .iter()
                .map(|source| crate::reader::id("feature", source))
                .collect::<Vec<_>>())
        );
        validate(result);
    }
}

#[test]
fn deterministic_axis_with_an_unresolved_producer_stays_native() {
    let bytes = edit(&fixture("revolve"), |path, response| {
        if path.ends_with("features.json") {
            parameter(&mut response["features"][1], "axis")["queries"] =
                json!([{"deterministicIds":["axis"],"featureId":"Ring sketch"}]);
        }
    });
    let result = decode(&bytes);
    native(&result, 1);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("producer is unresolved")));
    validate(result);
}

#[test]
fn dependency_order_follows_stored_operands() {
    let bytes = edit(&separated("axis"), |path, response| {
        if path.ends_with("features.json") {
            response["features"][3]["parameters"]
                .as_array_mut()
                .unwrap()
                .swap(2, 3);
        }
    });
    let result = decode(&bytes);
    assert_eq!(
        wire(&result)["model"]["features"][3]["dependencies"],
        json!([
            crate::reader::id("feature", "Plate"),
            crate::reader::id("feature", "independent")
        ])
    );
    validate(result);
}

#[test]
fn malformed_stored_dependency_queries_refuse_construction() {
    let bytes = edit(&separated("edge"), |path, response| {
        if path.ends_with("features.json") {
            parameter(&mut response["features"][3], "entities")["queries"][0]["queryString"] =
                json!("query=qCompressed(1.0,\"invalid\",id);");
        }
    });
    let result = decode(&bytes);
    native(&result, 3);
    assert!(result.ir().model.feature_input_topologies.is_empty());
    validate(result);
}

#[test]
fn through_bore_and_thread_termination_remain_independent() {
    for through in [false, true] {
        let bytes = edit(&fixture("hole-forms"), |path, response| {
            if path.ends_with("features.json") {
                let hole = &mut response["features"][5];
                parameter(hole, "endStyle")["value"] = json!("THROUGH");
                parameter(hole, "isTappedThrough")["value"] = json!(through);
                parameter(hole, "tappedDepth")["expression"] = json!("1 mm");
            }
        });
        let result = decode(&bytes);
        let wire = crate::test_support::review::wire(&result);
        assert_eq!(kind(&wire, 5), "hole");
        let hole = &wire["model"]["features"][5]["definition"]["value"]["value"];
        assert_eq!(hole["extent"]["kind"], "through_all");
        assert_eq!(
            hole["shape"]["construction"]["value"]["specification"]["depth"],
            if through {
                json!({"kind":"hole_depth"})
            } else {
                json!({"kind":"blind","depth":1.0})
            }
        );
        validate(result);
    }
}

#[test]
fn through_bores_with_unread_finite_thread_depths_stay_native() {
    for invalid in [Value::Null, json!("unsupported(1 mm)")] {
        let bytes = edit(&fixture("hole-forms"), |path, response| {
            if path.ends_with("features.json") {
                let hole = &mut response["features"][5];
                parameter(hole, "endStyle")["value"] = json!("THROUGH");
                parameter(hole, "tappedDepth")["expression"] = invalid.clone();
            }
        });
        let result = decode(&bytes);
        native(&result, 5);
        validate(result);
    }
}

fn scoped_operation(operation: &str, malformed: &str) -> Vec<u8> {
    let base = separated(if operation == "revolve" {
        "axis"
    } else {
        "edge"
    });
    edit(&base, |path, response| {
        if !path.ends_with("features.json") {
            return;
        }
        let features = response["features"].as_array_mut().unwrap();
        if operation == "extrude" {
            features[3] = features[1].clone();
            features[3]["featureId"] = json!("consumer");
            parameter(&mut features[3], "operationType")["value"] = json!("REMOVE");
        } else if operation == "hole" {
            features[3] = json!({"featureId":"consumer","featureType":"hole","parameters":[
                {"parameterId":"entities","queries":[{"btType":"BTMIndividualSketchUniqueVerticesQuery-1472","featureId":"independent"}]},
                {"parameterId":"style","value":"SIMPLE"},{"parameterId":"endStyle","value":"THROUGH"},
                {"parameterId":"holeDiameter","expression":"1 mm"}
            ]});
            features[2]["entities"] =
                json!([{"btType":"BTMSketchPoint-158","entityId":"location","x":0.005,"y":0.005}]);
            features[2]["constraints"] = json!([]);
        } else {
            parameter(&mut features[3], "operationType")["value"] = json!("REMOVE");
        }
        let key = if operation == "hole" {
            "scope"
        } else {
            "booleanScope"
        };
        let mut scope = json!({"parameterId":key});
        match malformed {
            "string" => scope["queries"] = json!("body"),
            "null" => scope["queries"] = Value::Null,
            "missing" => {}
            "empty" => scope["queries"] = json!([]),
            _ => {
                scope["queries"] = json!([{"deterministicIds":[if malformed == "valid" { "body" } else { "unknown" }]}]);
            }
        }
        features[3]["parameters"]
            .as_array_mut()
            .unwrap()
            .push(scope);
    })
}

#[test]
fn admitted_stored_scopes_keep_each_operation_typed() {
    for operation in ["extrude", "revolve", "hole"] {
        for scope in ["valid", "empty"] {
            let result = decode(&scoped_operation(operation, scope));
            assert_eq!(
                kind(&wire(&result), 3),
                operation,
                "{operation}: {:?}",
                result.report().losses
            );
            validate(result);
        }
    }
}

#[test]
fn unread_stored_scope_lists_and_targets_refuse_each_operation() {
    for operation in ["extrude", "revolve", "hole"] {
        for scope in ["string", "null", "missing", "unresolved"] {
            let result = decode(&scoped_operation(operation, scope));
            native(&result, 3);
            validate(result);
        }
    }
}

fn stored_next_target() -> Vec<u8> {
    edit(&separated("face"), |path, response| {
        if path.ends_with("features.json") {
            parameter(&mut response["features"][3], "endBound")["value"] = json!("UP_TO_NEXT");
        }
    })
}

#[test]
fn a_stored_closed_planar_target_proves_the_exact_next_intersection() {
    let result = decode(&stored_next_target());
    let wire = crate::test_support::review::wire(&result);
    assert_eq!(kind(&wire, 3), "extrude", "{:?}", result.report().losses);
    let termination = &wire["model"]["features"][3]["definition"]["value"]["value"]["extent"]
        ["side"]["termination"];
    assert_eq!(termination["kind"], "to_face");
    assert_eq!(termination["face"]["value"]["faces"], json!(["top"]));
    assert_eq!(
        wire["model"]["features"][3]["dependencies"],
        json!([
            crate::reader::id("feature", "Base sketch"),
            crate::reader::id("feature", "Plate")
        ])
    );
    validate(result);
}

#[test]
fn absent_stored_next_target_geometry_refuses_the_extent() {
    for missing in ["origin", "normal", "boundary"] {
        let bytes = edit(&stored_next_target(), |path, response| {
            if path.ends_with("bodydetails/3.json") {
                let face = &mut response["bodies"][0]["faces"][0];
                if missing == "boundary" {
                    face["loops"] = json!([]);
                } else {
                    face["surface"].as_object_mut().unwrap().remove(missing);
                }
            }
        });
        let result = decode(&bytes);
        native(&result, 3);
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("UP_TO_NEXT")));
        validate(result);
    }
}
