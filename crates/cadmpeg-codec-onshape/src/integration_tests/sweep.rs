// SPDX-License-Identifier: Apache-2.0
use crate::loss::OnshapeLossCode;
use crate::test_support::{fixture, review};
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;
use serde_json::Value;

fn sweep_bundle(operation: &str, control: Option<&str>) -> Vec<u8> {
    review::edit(&fixture("plate-fillet"), |path, value| {
        if !path.ends_with("/features.json") {
            return;
        }
        let mut path_sketch = value["features"][0].clone();
        path_sketch["featureId"] = json!("path-sketch");
        path_sketch["name"] = json!("Path sketch");
        path_sketch["constraints"] = json!([]);
        path_sketch["entities"] = json!([
            {"btType":"BTMSketchCurveSegment-155", "entityId":"z-line",
             "startParam":0.0,"endParam":0.02,
             "geometry":{"btType":"BTCurveGeometryLine-117", "dirX":1.0}},
            {"btType":"BTMSketchCurveSegment-155", "entityId":"a-arc",
             "startParam":-std::f64::consts::FRAC_PI_2,"endParam":0.0,
             "geometry":{"btType":"BTCurveGeometryCircle-115", "radius":0.01,
                "xCenter":0.02,"yCenter":0.01,"xDir":1.0,"yDir":0.0}}
        ]);
        let leaves: Vec<_> = ["z-line", "a-arc"].into_iter().map(|entity| {
            format!(
                "B5$QueryM5Sa$entityTypeBa$EntityTypeS4$EDGESb$historyTypeS8$CREATIONSb$operationIdB2$IdA1S{:x}$path-sketch.wireOpS9$queryTypeB9$QueryTypeSd$SKETCH_ENTITYSe$sketchEntityIdS{:x}${entity}",
                "path-sketch.wireOp".len(), entity.len())
        }).collect();
        let compact_queries = if control.is_some() {
            vec![format!(
                "%B5$QueryM2S9$queryTypeB9$QueryTypeS5$UNIONSa$subqueriesA2{}{}",
                leaves[0], leaves[1]
            )]
        } else {
            leaves.iter().map(|leaf| format!("%{leaf}")).collect()
        };
        let queries: Vec<_> = compact_queries.iter().map(|compact| {
            json!({"queryString":format!("query=qCompressed(1.0,{},id);", json::to_string(compact).unwrap())})
        }).collect();
        let mut sweep = json!({"featureId":"swept-body", "featureType":"sweep", "name":"Sweep", "parameters":[
            {"parameterId":"bodyType","value":"SOLID"},
            {"parameterId":"operationType","value":operation},
            {"parameterId":"profiles","queries":[{"btType":"BTMIndividualSketchRegionQuery-140","featureId":"Base sketch"}]},
            {"parameterId":"path","queries":queries}
        ]});
        if let Some(control) = control {
            sweep["parameters"]
                .as_array_mut()
                .unwrap()
                .push(json!({"parameterId":"profileControl","value":control}));
        }
        let mut features = vec![value["features"][0].clone(), path_sketch];
        if operation == "ADD" {
            features.push(value["features"][1].clone());
        }
        features.push(sweep);
        value["rollbackIndex"] = json!(features.len());
        value["features"] = json!(features);
    })
}

#[test]
fn plain_sweep_resolves_profile_path_order_and_dependencies() {
    for (operation, control, expected) in [
        ("NEW", None, "new_body"),
        ("NEW", Some("NONE"), "new_body"),
        ("ADD", Some("NONE"), "join"),
    ] {
        let result = review::decode(&sweep_bundle(operation, control));
        let wire = review::wire(&result);
        let feature = wire["model"]["features"]
            .as_array()
            .unwrap()
            .last()
            .unwrap();
        assert_eq!(feature["definition"]["value"]["kind"], "sweep");
        let sweep = &feature["definition"]["value"]["value"];
        assert_eq!(sweep["shape"]["op"], expected);
        assert_eq!(
            sweep["shape"]["section"]["value"],
            json!({"kind":"sketch","value":"onshape:model:sketch#Base%20sketch"})
        );
        assert_eq!(
            sweep["path"],
            json!({"kind":"sketch_curves","value":{"sketch":"onshape:model:sketch#path-sketch","curves":["onshape:model:entity#path-sketch/z-line","onshape:model:entity#path-sketch/a-arc"]}})
        );
        assert_eq!(
            feature["dependencies"],
            json!([
                "onshape:model:feature#Base%20sketch",
                "onshape:model:feature#path-sketch"
            ])
        );
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.to_string() == "onshape/sweep.not-supported"));
        let design = json::to_value(
            &result
                .ir()
                .design
                .features
                .last()
                .unwrap()
                .definition
                .operation,
        )
        .unwrap();
        assert_eq!(design["kind"], "sweep");
        assert_eq!(
            design["value"]["section"]["picks"]["value"],
            json!([{"target":{"kind":"sketch","value":"onshape:model:sketch#Base%20sketch"}}])
        );
        assert_eq!(
            design["value"]["path"]["picks"]["value"],
            json!([
                {"target":{"kind":"sketch_entity","value":"onshape:model:entity#path-sketch/z-line"}},
                {"target":{"kind":"sketch_entity","value":"onshape:model:entity#path-sketch/a-arc"}}
            ])
        );
        assert_eq!(
            design["value"]["output"]["boolean"]["kind"],
            if expected == "new_body" {
                "new_body"
            } else {
                "apply"
            }
        );
        review::validate(result);
    }
}

#[test]
fn unsupported_sweep_options_retain_native_with_named_loss() {
    for (key, value) in [
        ("profileControl", json!("LOCK_FACES")),
        ("profileControl", json!("LOCK_DIRECTION")),
        ("bodyType", json!("SURFACE")),
        ("hasTwist", json!(true)),
        ("hasScale", json!(true)),
        ("extendToFullPath", json!(true)),
        ("pathOppositeDirection", json!(true)),
        ("trimEnds", json!(true)),
        ("unlistedControl", json!(false)),
    ] {
        let bytes = review::edit(&sweep_bundle("NEW", Some("NONE")), |path, source| {
            if path.ends_with("/features.json") {
                let parameters = source["features"]
                    .as_array_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()["parameters"]
                    .as_array_mut()
                    .unwrap();
                parameters.retain(|p| p["parameterId"] != key);
                parameters.push(json!({"parameterId":key,"value":value}));
            }
        });
        let result = review::decode(&bytes);
        let wire = review::wire(&result);
        let feature = wire["model"]["features"]
            .as_array()
            .unwrap()
            .last()
            .unwrap();
        assert_eq!(feature["definition"]["value"]["kind"], "native", "{key}");
        assert!(
            result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code.to_string() == "onshape/sweep.not-supported"),
            "{key}"
        );
        review::validate(result);
    }
}

fn sweep_feature(wire: &Value) -> &Value {
    wire["model"]["features"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
}

#[test]
fn malformed_sweep_parameters_retain_native_with_history_loss() {
    for repeat in [false, true] {
        let bytes = review::edit(&sweep_bundle("NEW", Some("NONE")), |path, source| {
            if path.ends_with("/features.json") {
                let parameters = source["features"]
                    .as_array_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()["parameters"]
                    .as_array_mut()
                    .unwrap();
                if !repeat {
                    parameters.retain(|p| p["parameterId"] != "profileControl");
                }
                parameters.push(json!({"parameterId":"profileControl","value":if repeat {json!("NONE")} else {Value::Null}}));
            }
        });
        let result = review::decode(&bytes);
        assert_eq!(
            sweep_feature(&review::wire(&result))["definition"]["value"]["kind"],
            "native"
        );
        assert!(result
            .report()
            .losses
            .iter()
            .any(|l| l.code.to_string() == "onshape/history.not-supported"));
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|l| l.code.to_string() == "onshape/sweep.not-supported"));
        if repeat {
            assert!(result
                .report()
                .losses
                .iter()
                .any(|l| l.message.contains("parameter profileControl repeats")));
        }
        review::validate(result);
    }
}

#[test]
fn sweep_lock_queries_retain_native_with_named_loss() {
    for key in ["lockFaces", "lockDirectionQuery"] {
        let bytes = review::edit(&sweep_bundle("NEW", Some("NONE")), |path, source| {
            if path.ends_with("/features.json") {
                source["features"]
                    .as_array_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"parameterId":key,"queries":[{"deterministicIds":["locked"]}]}));
            }
        });
        let result = review::decode(&bytes);
        assert_eq!(
            sweep_feature(&review::wire(&result))["definition"]["value"]["kind"],
            "native"
        );
        assert!(result.report().losses.iter().any(|l| l.code.to_string()
            == "onshape/sweep.not-supported"
            && l.message.contains(key)));
        review::validate(result);
    }
}

fn path_query(entity: &str) -> Value {
    let compact = format!("%B5$QueryM5Sa$entityTypeBa$EntityTypeS4$EDGESb$historyTypeS8$CREATIONSb$operationIdB2$IdA1S{:x}$path-sketch.wireOpS9$queryTypeB9$QueryTypeSd$SKETCH_ENTITYSe$sketchEntityIdS{:x}${entity}", "path-sketch.wireOp".len(), entity.len());
    json!({"queryString":format!("query=qCompressed(1.0,{},id);", json::to_string(&compact).unwrap())})
}

#[test]
fn sweep_path_connectivity_orders_selection_and_refuses_disconnected_or_construction_curves() {
    for mode in ["open", "closed", "disconnected", "construction", "branch"] {
        let bytes = review::edit(&sweep_bundle("NEW", Some("NONE")), |path, source| {
            if !path.ends_with("/features.json") {
                return;
            }
            source["features"][1]["entities"] = json!([
                {"btType":"BTMSketchCurveSegment-155","entityId":"first","startParam":0.0,"endParam":0.01,
                 "isConstruction":mode=="construction", "geometry":{"btType":"BTCurveGeometryLine-117","dirX":1.0}},
                {"btType":"BTMSketchCurveSegment-155","entityId":"middle","startParam":0.01,"endParam":0.02,
                 "geometry":{"btType":"BTCurveGeometryLine-117","dirX":1.0}},
                {"btType":"BTMSketchCurveSegment-155","entityId":"last","startParam":if mode=="disconnected" {0.04} else {0.02},"endParam":0.03,
                 "geometry":{"btType":"BTCurveGeometryLine-117","dirX":1.0}}
            ]);
            if mode == "closed" {
                source["features"][1]["entities"][2]["startParam"] = json!(0.0);
                source["features"][1]["entities"][2]["endParam"] = json!(0.02);
            } else if mode == "branch" {
                source["features"][1]["entities"][2]["startParam"] = json!(0.0);
                source["features"][1]["entities"][2]["endParam"] = json!(0.01);
                source["features"][1]["entities"][2]["geometry"]["dirX"] = json!(0.0);
                source["features"][1]["entities"][2]["geometry"]["dirY"] = json!(1.0);
                source["features"][1]["entities"][2]["geometry"]["pntX"] = json!(0.01);
            }
            let params = source["features"]
                .as_array_mut()
                .unwrap()
                .last_mut()
                .unwrap()["parameters"]
                .as_array_mut()
                .unwrap();
            params
                .iter_mut()
                .find(|p| p["parameterId"] == "path")
                .unwrap()["queries"] = json!([
                path_query("middle"),
                path_query("first"),
                path_query("last")
            ]);
        });
        let result = review::decode(&bytes);
        let wire = review::wire(&result);
        let definition = &sweep_feature(&wire)["definition"]["value"];
        if matches!(mode, "open" | "closed") {
            assert_eq!(
                definition["kind"],
                "sweep",
                "{mode}: {:?}",
                result.report().losses
            );
            if mode == "open" {
                assert_eq!(
                    definition["value"]["path"]["value"]["curves"],
                    json!([
                        "onshape:model:entity#path-sketch/first",
                        "onshape:model:entity#path-sketch/middle",
                        "onshape:model:entity#path-sketch/last"
                    ])
                );
            }
        } else {
            assert_eq!(definition["kind"], "native", "{mode}");
            assert!(
                result
                    .report()
                    .losses
                    .iter()
                    .any(|l| l.code.to_string() == "onshape/sweep.not-supported"),
                "{mode}"
            );
        }
        review::validate(result);
    }
}

#[test]
fn disabled_sweep_twist_and_scale_expressions_are_unused() {
    let bytes = review::edit(&sweep_bundle("NEW", Some("NONE")), |path, source| {
        if path.ends_with("/features.json") {
            let params = source["features"]
                .as_array_mut()
                .unwrap()
                .last_mut()
                .unwrap()["parameters"]
                .as_array_mut()
                .unwrap();
            for key in ["turns", "angle", "pitch", "scaleFactor"] {
                params.push(json!({"parameterId":key,"expression":"#unused","value":1.0}));
            }
            for key in ["hasTwist", "hasScale"] {
                params.push(json!({"parameterId":key,"value":false}));
            }
            for key in ["lockFaces", "lockDirectionQuery"] {
                params.push(json!({"parameterId":key,"queries":[]}));
            }
        }
    });
    let result = review::decode(&bytes);
    assert_eq!(
        sweep_feature(&review::wire(&result))["definition"]["value"]["kind"],
        "sweep"
    );
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|l| l.code.to_string() == "onshape/history.variable-evaluated"));
    review::validate(result);
}

#[test]
fn sweep_design_keeps_cut_intersect_twist_scale_and_profile_lock() {
    for (operation, boolean) in [("REMOVE", "cut"), ("INTERSECT", "intersect")] {
        let bytes = crate::test_support::review::edit(
            &sweep_bundle(operation, Some("KEEP_ORIENTATION")),
            |path, response| {
                if path.ends_with("/features.json") {
                    let feature = response["features"]
                        .as_array_mut()
                        .unwrap()
                        .last_mut()
                        .unwrap();
                    feature["parameters"].as_array_mut().unwrap().extend([
                        json!({"parameterId":"hasTwist","value":true}),
                        json!({"parameterId":"twistType","value":"ANGLE"}),
                        json!({"parameterId":"angle","expression":"45 deg"}),
                        json!({"parameterId":"ccw","value":true}),
                        json!({"parameterId":"hasScale","value":true}),
                        json!({"parameterId":"scaleFactor","expression":"2"}),
                    ]);
                }
            },
        );
        let result = crate::test_support::review::decode(&bytes);
        let wire = crate::test_support::review::wire(&result);
        let op = wire["design"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|feature| {
                feature
                    .pointer("/definition/operation")
                    .filter(|op| op["kind"] == "sweep")
            })
            .unwrap_or_else(|| panic!("{:?}", result.report().losses));
        assert_eq!(op["value"]["output"]["boolean"]["op"]["kind"], boolean);
        assert_eq!(op["value"]["controls"]["orientation"]["kind"], "fixed");
        assert_eq!(op["value"]["controls"]["twist"]["kind"], "angle");
        assert_eq!(
            op["value"]["controls"]["scaling"]["value"]["value"]["end"]["value"],
            2.0
        );
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| OnshapeLossCode::SweepScaleAssumed.matches(loss)));
        crate::test_support::review::validate(result);
    }
}

#[test]
fn pitch_twist_uses_the_sweep_path_length() {
    let bytes = review::edit(&sweep_bundle("NEW", Some("NONE")), |path, source| {
        if path.ends_with("/features.json") {
            let feature = source["features"]
                .as_array_mut()
                .unwrap()
                .last_mut()
                .unwrap();
            feature["parameters"].as_array_mut().unwrap().extend([
                json!({"parameterId":"hasTwist","value":true}),
                json!({"parameterId":"twistType","value":"PITCH"}),
                json!({"parameterId":"pitch","expression":"10 mm"}),
                json!({"parameterId":"ccw","value":true}),
            ]);
        }
    });
    let result = review::decode(&bytes);
    let operation = review::design_operation(result.ir(), "swept-body");
    let twist = &operation["value"]["controls"]["twist"];
    assert_eq!(twist["kind"], "angle");
    assert_eq!(twist["value"]["kind"], "parameter");
    let view = review::wire(&result);
    let parameter = view["design"]["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|parameter| parameter["id"] == twist["value"]["value"])
        .unwrap();
    let measures = &parameter["definition"]["value"]["value"]["measures"];
    assert_eq!(measures[0]["kind"], json!({"kind":"length"}), "{parameter}");
    let selection = &measures[0]["of"];
    assert_eq!(selection["kind"], "selection");
    assert_eq!(selection["at"], operation["value"]["path"]["at"]);
    assert_eq!(selection["picks"], operation["value"]["path"]["picks"]);
    assert!(measures[0].get("cached").is_none());
    review::validate(result);
}

#[test]
fn an_active_face_lock_is_a_sweep_orientation() {
    let bytes = review::edit(&sweep_bundle("NEW", Some("LOCK_FACES")), |path, source| {
        if path.ends_with("/features.json") {
            source["features"]
                .as_array_mut()
                .unwrap()
                .last_mut()
                .unwrap()["parameters"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "parameterId":"lockFaces","queries":[{"deterministicIds":["support"]}]
                }));
        }
    });
    let result = review::decode(&bytes);
    let operation = review::design_operation(result.ir(), "swept-body");
    let lock = &operation["value"]["controls"]["orientation"];
    assert_eq!(lock["kind"], "surface");
    assert_eq!(
        lock["value"]["picks"]["value"][0]["native"]["value"]["value"],
        "support"
    );
    review::validate(result);
}
