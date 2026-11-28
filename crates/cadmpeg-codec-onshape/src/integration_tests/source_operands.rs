// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::json;
use serde_json::Value;

use crate::reader::id;
use crate::test_support::fixture;
use crate::test_support::projection::with_variables;
use crate::test_support::review::{
    cadir_round_trip, decode, design_operation, edit, parameter, validate, variable, wire,
};

fn feature(id: &str, kind: &str, parameters: Value) -> Value {
    let mut feature = json!({"featureId":id,"name":id,"featureType":kind});
    feature["parameters"] = parameters;
    feature
}

fn rows(feature: &mut Value) -> &mut Vec<Value> {
    feature["parameters"].as_array_mut().unwrap()
}

fn part_studio(fixture_name: &str, mut update: impl FnMut(&mut Value)) -> Vec<u8> {
    edit(&fixture(fixture_name), |path, document| {
        if path == "partstudios/studio/features.json" {
            update(document);
        }
    })
}

fn parameter_wire<'a>(wire: &'a Value, native: &str) -> &'a Value {
    let id = id("parameter", native);
    wire["design"]["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|parameter| parameter["id"] == id)
        .unwrap_or_else(|| panic!("{id} has no design parameter"))
}

fn parameter_loss(result: &cadmpeg_ir::codec::DecodeResult, key: &str, subject: &str) -> bool {
    result.report().losses.iter().any(|loss| {
        loss.code.local_code() == "history.parameter-not-projected"
            && loss.message.starts_with(&format!("parameter {key} "))
            && loss.message.ends_with(&format!("; subject: {subject}"))
    })
}

#[test]
fn boolean_scope_keeps_selected_targets_without_a_false_loss() {
    for (fixture, subject) in [("plate-fillet", "Plate"), ("revolve", "Ring")] {
        let bytes = part_studio(fixture, |document| {
            let operation = &mut document["features"][1];
            parameter(operation, "operationType")["value"] = json!("REMOVE");
            rows(operation).extend([
                json!({"parameterId":"defaultScope","value":false}),
                json!({"parameterId":"booleanScope","queries":[{"deterministicIds":["body"]}]}),
            ]);
        });
        let result = decode(&bytes);
        let operation = design_operation(result.ir(), subject);
        let targets = &operation["value"]["output"]["boolean"]["targets"];
        assert_eq!(targets["kind"], "selected");
        assert_eq!(
            targets["picks"]["value"][0]["native"]["value"]["value"],
            "body"
        );
        assert!(!parameter_loss(&result, "defaultScope", subject));
        assert!(!parameter_loss(&result, "booleanScope", subject));
        validate(result);
    }
}

#[test]
fn an_empty_or_unread_explicit_scope_never_selects_all_bodies() {
    for (queries, expected) in [
        (json!([]), Some("selected")),
        (
            json!([{"deterministicIds":[],"queryString":"query=qEverything(EntityType.BODY);"}]),
            None,
        ),
        (
            json!([{"queryString":"query=qEverything(EntityType.BODY);"}]),
            None,
        ),
    ] {
        let bytes = part_studio("plate-fillet", |document| {
            let extrude = &mut document["features"][1];
            parameter(extrude, "operationType")["value"] = json!("REMOVE");
            rows(extrude).extend([
                json!({"parameterId":"defaultScope","value":false}),
                json!({"parameterId":"booleanScope","queries":queries}),
            ]);
        });
        let result = decode(&bytes);
        let operation = design_operation(result.ir(), "Plate");
        let boolean = &operation["value"]["output"]["boolean"];
        assert_eq!(boolean["kind"], "apply");
        assert_eq!(boolean["targets"]["kind"].as_str(), expected);
        if expected.is_some() {
            assert_eq!(boolean["targets"]["picks"]["value"], json!([]));
        }
        assert_eq!(
            parameter_loss(&result, "booleanScope", "Plate"),
            expected.is_none()
        );
        validate(result);
    }
}

#[test]
fn direct_selection_keeps_resolved_ids_and_reports_only_unread_queries() {
    for (last_row, missing_ids) in [
        (json!({"deterministicIds":[]}), false),
        (
            json!({"deterministicIds":[],"queryString":"query=qEverything(EntityType.FACE);"}),
            true,
        ),
        (
            json!({"queryString":"query=qEverything(EntityType.FACE);"}),
            true,
        ),
    ] {
        let bytes = part_studio("plate-fillet", |document| {
            document["features"][2] = feature(
                "Wall",
                "shell",
                json!([
                    {"parameterId":"entities","queries":[{"deterministicIds":["entry-face"]},last_row]},
                    {"parameterId":"thickness","expression":"2 mm"},
                ]),
            );
        });
        let result = decode(&bytes);
        let operation = design_operation(result.ir(), "Wall");
        let removed = operation["value"]["removed"]["picks"]["value"]
            .as_array()
            .unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0]["native"]["value"]["value"], "entry-face");
        assert_eq!(parameter_loss(&result, "entities", "Wall"), missing_ids);
        validate(result);
    }
}

#[test]
fn symmetric_draft_uses_the_span_rule_and_clears_the_legacy_loss() {
    for (end_bound, stale_second) in [("BLIND", false), ("BLIND", true), ("THROUGH_ALL", false)] {
        let bytes = part_studio("plate-fillet", |document| {
            let extrude = &mut document["features"][1];
            parameter(extrude, "endBound")["value"] = json!(end_bound);
            rows(extrude).extend([
                json!({"parameterId":"symmetric","value":true}),
                json!({"parameterId":"hasDraft","value":true}),
                json!({"parameterId":"draftAngle","expression":"5 deg"}),
                json!({"parameterId":"draftPullDirection","value":true}),
            ]);
            if stale_second {
                rows(extrude).extend([
                    json!({"parameterId":"hasSecondDirection","value":true}),
                    json!({"parameterId":"hasSecondDirectionDraft","value":true}),
                ]);
            }
        });
        let result = decode(&bytes);
        let operation = design_operation(result.ir(), "Plate");
        let extrude = &operation["value"];
        let draft = &extrude["draft"];
        assert_eq!(
            draft["end"],
            json!({"kind":"literal","value":5_f64.to_radians()})
        );
        if end_bound == "BLIND" {
            assert_eq!(extrude["span"]["kind"], "symmetric");
            assert!(draft.get("start").is_none());
        } else {
            assert_eq!(extrude["span"]["start"]["kind"], "through_all");
            assert_eq!(extrude["span"]["end"]["kind"], "through_all");
            assert_eq!(draft["start"], draft["end"]);
        }
        assert!(!result.report().losses.iter().any(|loss| {
            loss.message.starts_with("hasDraft ") || loss.message.starts_with("hasSecondDirection ")
        }));
        validate(result);
    }
}

#[test]
fn an_explicit_sketch_hole_start_is_charged_without_losing_the_bore() {
    for start in ["PART", "SKETCH"] {
        let bytes = part_studio("hole-forms", |document| {
            let hole = &mut document["features"][5];
            rows(hole).push(json!({"parameterId":"startStyle","value":start}));
        });
        let result = decode(&bytes);
        let operation = design_operation(result.ir(), "Tapped");
        assert_eq!(operation["kind"], "hole");
        assert_eq!(operation["value"]["positions"]["kind"], "points");
        assert_eq!(operation["value"]["extent"]["kind"], "amount");
        assert_eq!(
            parameter_loss(&result, "startStyle", "Tapped"),
            start == "SKETCH"
        );
        if start == "SKETCH" {
            assert!(result
                .report()
                .losses
                .iter()
                .any(|loss| loss.message == "parameter startStyle uses SKETCH as the hole depth reference; subject: Tapped"));
        }
        validate(result);
    }
}

#[test]
fn chamfer_angle_measure_and_per_edge_override_come_from_its_parameters() {
    for (measure, expected) in [
        ("FACE_OFFSET", json!({"kind":"offset_faces"})),
        ("APEX_RANGE", json!({"kind":"native","value":"APEX_RANGE"})),
    ] {
        let bytes = edit(&fixture("plate-fillet"), |path, document| {
            if path == "partstudios/studio/bodydetails/2.json" {
                let edges = document["bodies"][0]["edges"].as_array_mut().unwrap();
                let mut edge = edges[0].clone();
                edge["id"] = json!("other");
                edges.push(edge);
            }
            if path == "partstudios/studio/features.json" {
                document["features"][2] = feature(
                    "Corner bevel",
                    "chamfer",
                    json!([
                        {"parameterId":"chamferType","value":"OFFSET_ANGLE"},
                        {"parameterId":"width","expression":"1 mm"},
                        {"parameterId":"angle","expression":"30 deg"},
                        {"parameterId":"chamferMethod","value":measure},
                        {"parameterId":"tangentPropagation","value":false},
                        {"parameterId":"oppositeDirection","value":false},
                        {"parameterId":"directionOverrides","queries":[{"deterministicIds":["other"]}]},
                        {"parameterId":"entities","queries":[{"deterministicIds":["upright","other"]}]}
                    ]),
                );
            }
        });
        let result = decode(&bytes);
        let operation = design_operation(result.ir(), "Corner bevel");
        let sets = operation["value"]["sets"].as_array().unwrap();
        assert_eq!(sets.len(), 2);
        for (set, (edge, flipped)) in sets.iter().zip([("upright", false), ("other", true)]) {
            assert_eq!(set["size"]["kind"], "distance_angle");
            assert_eq!(set["size"]["value"]["distance"]["value"], 1.0);
            assert!(
                (set["size"]["value"]["angle"]["value"].as_f64().unwrap()
                    - std::f64::consts::FRAC_PI_6)
                    .abs()
                    < 1e-12
            );
            assert_eq!(set["measure"], expected);
            assert_eq!(set["flip"]["value"], flipped);
            assert_eq!(
                set["edges"]["picks"]["value"]["terms"][0]["seeds"][0]["native"]["value"]["value"],
                edge
            );
        }
        assert_eq!(
            parameter_loss(&result, "chamferMethod", "Corner bevel"),
            measure == "APEX_RANGE"
        );
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| { loss.code.local_code() == "history.chamfer-side-assumed" }));
        let feature = id("feature", "Corner bevel");
        let fields = result.source_fidelity().annotations.exactness()[feature.as_str()].fields();
        for index in 0..sets.len() {
            let path = format!("definition.operation.value.sets.{index}.size.value.distance");
            assert!(fields.iter().any(|(field, exactness)| {
                field.as_str() == path && *exactness == cadmpeg_ir::Exactness::Inferred
            }));
        }
        validate(result);
    }
}

#[test]
fn an_asymmetric_fillet_keeps_its_ordered_seeds_and_compound_radius() {
    let bytes = edit(&fixture("plate-fillet"), |path, document| {
        if path == "partstudios/studio/bodydetails/2.json" {
            let edges = document["bodies"][0]["edges"].as_array_mut().unwrap();
            let mut edge = edges[0].clone();
            edge["id"] = json!("a-first-in-sort");
            edges.push(edge);
        }
        if path == "partstudios/studio/features.json" {
            let fillet = &mut document["features"][2];
            parameter(fillet, "entities")["queries"][0]["deterministicIds"] =
                json!(["upright", "a-first-in-sort"]);
            parameter(fillet, "radius")["expression"] = json!("2 mm + 3 mm");
            rows(fillet).extend([
                json!({"parameterId":"otherRadius","expression":"2 mm"}),
                json!({"parameterId":"isAsymmetric","value":true}),
                json!({"parameterId":"flipAsymmetric","value":true}),
                json!({"parameterId":"tangentPropagation","value":false}),
            ]);
        }
    });
    let result = decode(&bytes);
    let operation = design_operation(result.ir(), "Corner round");
    let set = &operation["value"]["sets"][0];
    assert_eq!(
        set["section"]["size"],
        json!({"kind":"offsets","value":{
            "first":{"kind":"literal","value":2.0},
            "second":{"kind":"parameter","value":id("parameter", "Corner round/radius")}
        }})
    );
    let seeds = set["edges"]["picks"]["value"]["terms"][0]["seeds"]
        .as_array()
        .unwrap();
    assert_eq!(
        seeds
            .iter()
            .map(|pick| pick["native"]["value"]["value"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["upright", "a-first-in-sort"]
    );
    let wire = wire(&result);
    let radius = parameter_wire(&wire, "Corner round/radius");
    assert_eq!(
        radius["definition"]["value"]["value"]["text"],
        "2[mm] + 3[mm]"
    );
    assert!(radius.get("value").is_none());
    validate(result);
}

#[test]
fn a_hole_keeps_its_start_flat_bottom_and_active_thread_bindings() {
    for (bore, tap_through, full) in [
        ("BLIND", false, false),
        ("BLIND", true, false),
        ("THROUGH", false, false),
        ("THROUGH", true, true),
    ] {
        let bytes = with_variables(
            &fixture("hole-forms"),
            &[
                variable("major", "thread-major", "LENGTH", "4 mm"),
                variable("threadDepth", "thread-depth", "LENGTH", "5 mm"),
            ],
            |document| {
                let hole = document["features"]
                    .as_array_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap();
                parameter(hole, "majorDiameter")["expression"] = json!("#major");
                parameter(hole, "tappedDepth")["expression"] = json!("#threadDepth");
                parameter(hole, "endStyle")["value"] = json!(bore);
                parameter(hole, "isTappedThrough")["value"] = json!(tap_through);
                parameter(hole, "standardTappedOrClearance")["value"]["pitch"] =
                    json!("1.25 mm (M)");
                rows(hole).extend([
                    json!({"parameterId":"startStyle","value":"PLANE"}),
                    json!({"parameterId":"startBoundEntity","queries":[{"deterministicIds":["entry-plane"]}]}),
                    json!({"parameterId":"tipAngleStyle","value":"FLAT"}),
                ]);
                if bore == "THROUGH" {
                    rows(hole).retain(|parameter| parameter["parameterId"] != "tipAngle");
                }
            },
        );
        let result = decode(&bytes);
        let wire = wire(&result);
        let feature = wire["design"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|feature| {
                feature
                    .pointer("/definition/operation/value/thread")
                    .is_some()
            })
            .unwrap();
        let operation = &feature["definition"]["operation"]["value"];
        assert_eq!(
            operation["start"]["picks"]["value"][0]["native"]["value"]["value"],
            "entry-plane"
        );
        if bore == "BLIND" {
            assert_eq!(operation["shape"]["bottom"], json!({"kind":"flat"}));
        } else {
            assert!(operation["shape"].get("bottom").is_none());
        }
        assert_eq!(operation["thread"]["major_diameter"]["kind"], "parameter");
        assert_eq!(
            operation["thread"]["pitch"],
            json!({"kind":"literal","value":1.25})
        );
        assert_eq!(
            operation["thread"]["length"]["kind"],
            if full { "full" } else { "blind" }
        );
        if !full {
            assert_eq!(operation["thread"]["length"]["value"]["kind"], "parameter");
        }
        assert!(result
            .report()
            .losses
            .iter()
            .all(|loss| { loss.code.local_code() != "history.variable-evaluated" }));
        validate(result);
    }
}

#[test]
fn peer_booleans_use_the_stored_tool_order_without_a_target() {
    for (kind, expected) in [("UNION", "join"), ("INTERSECTION", "intersect")] {
        let bytes = part_studio("derived", |document| {
            document["features"][3] = feature(
                "Cut",
                "booleanBodies",
                json!([
                    {"parameterId":"operationType","value":kind},
                    {"parameterId":"tools","queries":[{"deterministicIds":["inserted","base"]}]},
                    {"parameterId":"keepTools","value":true},
                ]),
            );
        });
        let result = decode(&bytes);
        let operation = design_operation(result.ir(), "Cut");
        let record = &operation["value"];
        assert_eq!(operation["kind"], "combine");
        assert!(record.get("target").is_none());
        assert_eq!(record["op"]["kind"], expected);
        assert_eq!(record["tools_after"], "kept");
        let tools = record["tools"]["picks"]["value"].as_array().unwrap();
        assert_eq!(
            tools
                .iter()
                .map(|pick| pick["native"]["value"]["value"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["inserted", "base"]
        );
        validate(result);
    }
}

#[test]
fn an_up_to_face_extrude_extends_and_translates_its_limit() {
    for flip in [false, true] {
        let bytes = part_studio("plate-fillet", |document| {
            let extrude = &mut document["features"][1];
            parameter(extrude, "endBound")["value"] = json!("UP_TO_SURFACE");
            rows(extrude).extend([
                json!({"parameterId":"endBoundEntityFace","queries":[{"deterministicIds":["limit-face"]}]}),
                json!({"parameterId":"hasOffset","value":true}),
                json!({"parameterId":"offsetDistance","expression":"2 mm"}),
                json!({"parameterId":"offsetOppositeDirection","value":flip}),
            ]);
        });
        let result = decode(&bytes);
        let operation = design_operation(result.ir(), "Plate");
        let limit = &operation["value"]["span"]["end"];
        assert_eq!(limit["kind"], "to_geometry");
        assert_eq!(
            limit["value"]["extend"],
            json!({"kind":"literal","value":true})
        );
        assert_eq!(
            limit["value"]["offset"],
            json!({"kind":"translated","value":{
                "kind":"literal","value": if flip { 2.0 } else { -2.0 }
            }})
        );
        validate(result);
    }
}

#[test]
fn shell_sense_and_closed_hollow_use_the_source_controls() {
    for outward in [false, true] {
        let bytes = part_studio("plate-fillet", |document| {
            document["features"][2] = feature(
                "Wall",
                "shell",
                json!([
                    {"parameterId":"isHollow","value":true},
                    {"parameterId":"parts","queries":[{"deterministicIds":["body"]}]},
                    {"parameterId":"thickness","expression":"2 mm"},
                    {"parameterId":"oppositeDirection","value":outward},
                ]),
            );
        });
        let result = decode(&bytes);
        let operation = design_operation(result.ir(), "Wall");
        assert_eq!(operation["kind"], "shell");
        assert_eq!(
            operation["value"]["thickness"]["value"]["side"]["kind"],
            if outward { "outside" } else { "inside" }
        );
        assert_eq!(
            operation["value"]["bodies"]["picks"]["value"][0]["native"]["value"]["value"],
            "body"
        );
        assert_eq!(operation["value"]["removed"]["picks"]["value"], json!([]));
        validate(result);
    }
}

#[test]
fn a_sketch_quantity_keeps_raw_definition_and_stored_cache_independently() {
    for (expression, stored, units, kind, canonical, cache) in [
        (
            "(2 * inch)",
            Some(2.0),
            "in",
            "neutral",
            Some("2 * 1[in]"),
            Some(50.8),
        ),
        (
            "sqrt(4) * #wall",
            Some(20.0),
            "mm",
            "source",
            None,
            Some(20.0),
        ),
        ("1 in", None, "in", "neutral", Some("1[in]"), None),
    ] {
        let bytes = with_variables(
            &fixture("plate-fillet"),
            &[
                variable("inch", "inch-variable", "LENGTH", "10 mm"),
                variable("wall", "wall-variable", "LENGTH", "10 mm"),
            ],
            |document| {
                let constraint = &mut document["features"][0]["constraints"][1];
                let value = parameter(constraint, "length");
                value["expression"] = json!(expression);
                value["units"] = json!(units);
                if let Some(stored) = stored {
                    value["value"] = json!(stored);
                }
            },
        );
        let result = decode(&bytes);
        let wire = wire(&result);
        let parameter = parameter_wire(&wire, "Base sketch/length");
        let definition = &parameter["definition"]["value"];
        assert_eq!(definition["kind"], kind);
        assert_eq!(definition["value"]["text"].as_str(), canonical);
        assert_eq!(parameter["unit"][0]["symbol"], units);
        assert_eq!(parameter["value"]["value"].as_f64(), cache);
        assert_eq!(
            parameter["source_settings"]["onshape:source_expression"]["value"]["value"],
            expression
        );
        validate(result);
    }
}

#[test]
fn suppressed_variables_keep_their_owned_definition_and_derived_features_keep_the_pin() {
    let mut variable = variable("wall", "suppressed-wall", "LENGTH", "1 in");
    variable["suppressed"] = json!(true);
    let bytes = with_variables(&fixture("plate-fillet"), &[variable], |_| {});
    let result = decode(&bytes);
    let wire_value = wire(&result);
    let parameter = parameter_wire(&wire_value, "suppressed-wall");
    assert_eq!(
        parameter["owner"]["value"],
        id("feature", "suppressed-wall")
    );
    assert_eq!(parameter["definition"]["value"]["value"]["text"], "1[in]");
    validate(result);

    for suppressed in [false, true] {
        let bytes = part_studio("derived", |document| {
            document["features"][2]["suppressed"] = json!(suppressed);
            document["features"].as_array_mut().unwrap().pop();
        });
        let result = decode(&bytes);
        let operation = design_operation(result.ir(), "Derived");
        assert_eq!(operation["kind"], "stored");
        assert_eq!(operation["value"]["origin"]["kind"], "linked");
        let definition = &operation["value"]["origin"]["document"]["definition"];
        let (written, _) = cadir_round_trip(&result);
        let wire = cadmpeg_test_support::wire::in_memory_json(&written);
        let external = wire["model"]["product_definitions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["id"] == *definition)
            .unwrap();
        assert_eq!(
            external["storage"],
            json!({"kind":"external","document_id":"111111111111111111111111",
            "version":"222222222222222222222222","object":"444444444444444444444444"})
        );
        validate(result);
    }
}

#[test]
fn accounting_distinguishes_disabled_and_unsupported_fillet_controls() {
    for active in [false, true] {
        let bytes = part_studio("plate-fillet", |document| {
            let fillet = &mut document["features"][2];
            rows(fillet).extend([
                json!({"parameterId":"isVariable","value":active}),
                json!({"parameterId":"isPartial","value":false}),
                json!({"parameterId":"allowEdgeOverflow","value":false}),
                json!({"parameterId":"smoothCorners","value":false}),
                json!({"parameterId":"smoothTransition","value":false}),
                json!({"parameterId":"futureFlag","value":true}),
            ]);
        });
        let result = decode(&bytes);
        assert_eq!(
            design_operation(result.ir(), "Corner round")["kind"],
            "fillet"
        );
        assert_eq!(
            parameter_loss(&result, "isVariable", "Corner round"),
            active
        );
        for key in [
            "isPartial",
            "allowEdgeOverflow",
            "smoothCorners",
            "smoothTransition",
        ] {
            assert!(!parameter_loss(&result, key, "Corner round"), "{key}");
        }
        assert!(parameter_loss(&result, "futureFlag", "Corner round"));
        validate(result);
    }
}

#[test]
fn accounting_omits_disabled_extrusion_controls_and_charges_active_second_offset() {
    for offset in [false, true] {
        let bytes = part_studio("plate-fillet", |document| {
            rows(&mut document["features"][1]).extend([
                json!({"parameterId":"hasExtrudeDirection","value":false}),
                json!({"parameterId":"hasSecondDirection","value":true}),
                json!({"parameterId":"secondDirectionBound","value":"UP_TO_NEXT"}),
                json!({"parameterId":"hasSecondDirectionOffset","value":offset}),
                json!({"parameterId":"secondDirectionOffsetDistance","expression":"1 mm"}),
            ]);
        });
        let result = decode(&bytes);
        assert_eq!(design_operation(result.ir(), "Plate")["kind"], "extrude");
        assert_eq!(
            design_operation(result.ir(), "Plate")["value"]["span"].is_null(),
            offset
        );
        assert!(!parameter_loss(&result, "hasExtrudeDirection", "Plate"));
        assert_eq!(
            parameter_loss(&result, "hasSecondDirectionOffset", "Plate"),
            offset
        );
        assert_eq!(
            parameter_loss(&result, "secondDirectionOffsetDistance", "Plate"),
            offset
        );
        validate(result);
    }
}

#[test]
fn active_direction_and_next_offset_are_not_stated_as_default_values() {
    for control in ["direction", "offset"] {
        let bytes = part_studio("plate-fillet", |document| {
            let extrude = &mut document["features"][1];
            if control == "direction" {
                rows(extrude).extend([
                    json!({"parameterId":"hasExtrudeDirection","value":true}),
                    json!({"parameterId":"extrudeDirection","queries":[{"deterministicIds":["edge"]}]}),
                ]);
            } else {
                parameter(extrude, "endBound")["value"] = json!("UP_TO_NEXT");
                rows(extrude).extend([
                    json!({"parameterId":"hasOffset","value":true}),
                    json!({"parameterId":"offsetDistance","expression":"1 mm"}),
                ]);
            }
        });
        let result = decode(&bytes);
        let operation = design_operation(result.ir(), "Plate");
        let field = if control == "direction" {
            "direction"
        } else {
            "span"
        };
        assert!(operation["value"][field].is_null());
        let key = if control == "direction" {
            "hasExtrudeDirection"
        } else {
            "hasOffset"
        };
        assert!(parameter_loss(&result, key, "Plate"));
        validate(result);
    }
}

#[test]
fn shallow_part_studio_reports_omitted_design() {
    use cadmpeg_core::decode::DecodeDepth;
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let result = crate::test_support::codec()
        .decode(
            &mut std::io::Cursor::new(fixture("plate-fillet")),
            &DecodeOptions {
                depth: DecodeDepth::Geometry,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert!(result.ir().design.features.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "depth.not-delivered"));
    validate(result);
}

#[test]
fn accounting_treats_a_plain_hole_as_deliberately_unthreaded() {
    let bytes = part_studio("hole", |document| {
        let hole = document["features"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|feature| feature["featureType"] == "hole")
            .unwrap();
        rows(hole).retain(|row| {
            row["parameterId"] != "isTapped" && row["parameterId"] != "standardTappedOrClearance"
        });
        rows(hole).extend([
            json!({"parameterId":"isTapped","value":false}),
            json!({"parameterId":"standardTappedOrClearance","value":null}),
            json!({"parameterId":"hasClearance","value":false}),
            json!({"parameterId":"standardBlindInLast","value":false}),
        ]);
    });
    let result = decode(&bytes);
    let operation = design_operation(result.ir(), "Bore");
    assert_eq!(operation["kind"], "hole");
    assert!(operation["value"]["thread"].is_null());
    for key in [
        "isTapped",
        "standardTappedOrClearance",
        "hasClearance",
        "standardBlindInLast",
    ] {
        assert!(!parameter_loss(&result, key, "Bore"), "{key}");
    }
    validate(result);
}

#[test]
fn accounting_reports_a_formula_that_evaluates_to_a_flat_drill_bottom() {
    let bytes = part_studio("hole-forms", |document| {
        let hole = &mut document["features"][5];
        rows(hole).retain(|row| row["parameterId"] != "tipAngleStyle");
        rows(hole).push(json!({"parameterId":"tipAngleStyle","value":"CUSTOM"}));
        parameter(hole, "tipAngle")["expression"] = json!("90 deg + 90 deg");
    });
    let result = decode(&bytes);
    let operation = design_operation(result.ir(), "Tapped");
    assert_eq!(
        operation["value"]["shape"]["bottom"]["kind"], "flat",
        "{operation}"
    );
    assert!(parameter_loss(&result, "tipAngle", "Tapped"));
    validate(result);
}

#[test]
fn accounting_reports_a_suppressed_derived_selector_beyond_the_document_pin() {
    for part in ["part-one", "part-two"] {
        let bytes = part_studio("derived", |document| {
            document["features"].as_array_mut().unwrap().truncate(3);
            let derived = &mut document["features"][2];
            derived["suppressed"] = json!(true);
            rows(derived).retain(|row| row["parameterId"] != "newUI");
            rows(derived).push(json!({"parameterId":"newUI","value":true}));
            parameter(derived, "partStudio")["partQuery"] =
                json!({"queries":[{"deterministicIds":[part]}]});
        });
        let result = decode(&bytes);
        let operation = design_operation(result.ir(), "Derived");
        assert_eq!(operation["value"]["origin"]["kind"], "linked");
        assert!(!parameter_loss(&result, "newUI", "Derived"));
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .filter(|loss| {
                    loss.code.local_code() == "history.parameter-not-projected"
                        && loss.message.starts_with("parameter partStudio ")
                        && loss.message.contains("partQuery")
                        && loss.message.ends_with("; subject: Derived")
                })
                .count(),
            1
        );
        validate(result);
    }
}

#[test]
fn accounting_reports_legacy_derived_controls_beyond_a_stored_pin() {
    let bytes = part_studio("derived", |document| {
        document["features"].as_array_mut().unwrap().truncate(3);
        let derived = &mut document["features"][2];
        derived["suppressed"] = json!(true);
        rows(derived).retain(|row| row["parameterId"] != "newUI");
        rows(derived).push(json!({"parameterId":"newUI","value":false}));
    });
    let result = decode(&bytes);
    assert_eq!(
        design_operation(result.ir(), "Derived")["value"]["origin"]["kind"],
        "linked"
    );
    assert!(parameter_loss(&result, "newUI", "Derived"));
    validate(result);
}

#[test]
fn accounting_reports_an_unresolved_operation_once() {
    let bytes = part_studio("plate-fillet", |document| {
        parameter(&mut document["features"][1], "endBound")["value"] = json!("UP_TO_VERTEX");
    });
    let result = decode(&bytes);
    assert_eq!(design_operation(result.ir(), "Plate")["kind"], "unresolved");
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.local_code() == "history.design-operation-unresolved"
            && loss.message.ends_with("; subject: Plate")
    }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code.local_code() == "history.parameter-not-projected"
            && loss.message.ends_with("; subject: Plate")
    }));
    validate(result);
}

#[test]
fn accounting_accepts_inactive_revolve_offset_controls() {
    let bytes = part_studio("revolve", |document| {
        rows(&mut document["features"][1]).extend([
            json!({"parameterId":"endBoundHasOffset","value":false}),
            json!({"parameterId":"startBoundHasOffset","value":false}),
            json!({"parameterId":"startOppositeDirection","value":false}),
        ]);
    });
    let result = decode(&bytes);
    assert_eq!(design_operation(result.ir(), "Ring")["kind"], "revolve");
    for key in [
        "endBoundHasOffset",
        "startBoundHasOffset",
        "startOppositeDirection",
    ] {
        assert!(!parameter_loss(&result, key, "Ring"), "{key}");
    }
    validate(result);
}

#[test]
fn accounting_reports_active_sketch_imprinting_control() {
    let bytes = part_studio("plate-fillet", |document| {
        let sketch = &mut document["features"][0];
        rows(sketch).retain(|row| row["parameterId"] != "disableImprinting");
        rows(sketch).push(json!({"parameterId":"disableImprinting","value":true}));
    });
    let result = decode(&bytes);
    assert_eq!(
        design_operation(result.ir(), "Base sketch")["kind"],
        "sketch"
    );
    assert!(parameter_loss(&result, "disableImprinting", "Base sketch"));
    assert!(!parameter_loss(&result, "sketchPlane", "Base sketch"));
    validate(result);
}
