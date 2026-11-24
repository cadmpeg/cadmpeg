// SPDX-License-Identifier: Apache-2.0
//! Derived Part Studio references and the Booleans that consume their bodies.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::bundle::{changed, kinds, read, timeline};
use crate::test_support::fixture;
use serde_json::{json, Value};

const HOST: &str = "partstudios/studio/features.json";
const SOURCE_STUDIO: &str =
    "references/111111111111111111111111/222222222222222222222222/444444444444444444444444/";

fn has_unsupported(report: &crate::test_support::Report) -> bool {
    report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some("onshape/history.not-supported"))
}

fn design_reference(wire: &Value) -> Value {
    serde_json::from_str(
        wire["model"]["features"][2]["source_properties"]["design_reference"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn derived_studio_keeps_neutral_reference_and_generated_boolean_operands() {
    let (model, report) = read(&fixture("derived")).unwrap();
    assert_eq!(
        kinds(&model),
        ["sketch", "extrude", "stored_geometry", "combine"],
        "{:?}",
        report.losses()
    );
    let wire = timeline(&model);
    let reference = design_reference(&wire);
    assert_eq!(reference["schema"], "dutchman.design-reference/1");
    assert_eq!(reference["bodies"][0]["name"], "Tool");
    assert_eq!(
        reference["ir"]["model"]["features"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(reference["step"]
        .as_str()
        .unwrap()
        .starts_with("ISO-10303-21;"));
    assert_eq!(
        wire["model"]["feature_result_topologies"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let operands = &wire["model"]["features"][3]["definition"]["value"]["value"]["operands"];
    assert_eq!(operands["target"]["kind"], "generated");
    assert_eq!(operands["tools"]["kind"], "generated");
    let dependencies = wire["model"]["features"][3]["dependencies"]
        .as_array()
        .unwrap();
    assert!(dependencies.contains(&wire["model"]["features"][1]["id"]));
    assert!(dependencies.contains(&wire["model"]["features"][2]["id"]));
    assert_eq!(
        model.ir.geometry.bodies.len(),
        1,
        "intermediate bodies must not enter final geometry"
    );
    assert!(!has_unsupported(&report), "{:?}", report.losses());
}

#[test]
fn derived_resolved_parts_can_select_the_entire_referenced_studio() {
    let bytes = changed(&fixture("derived"), |path, value| {
        if path == HOST {
            value["features"][2]["parameters"]
                .as_array_mut()
                .unwrap()
                .push(json!({"parameterId":"parts", "queries":[{"deterministicIds":["tool"]}]}));
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["sketch", "extrude", "stored_geometry", "combine"],
        "{:?}",
        report.losses()
    );
}

#[test]
fn derived_reference_mismatches_and_unresolved_boolean_bodies_stop_history() {
    for failure in [
        "missing",
        "version",
        "microversion",
        "source-final-parts",
        "configuration",
        "placement",
        "subset",
        "source-query",
        "tools",
        "duplicate",
        "body-name",
        "replaced-body",
        "custom",
        "boolean-option",
        "changed-producer",
        "boolean-query-namespace",
        "unknown-source-body",
        "inconsistent",
    ] {
        let bytes = changed(&fixture("derived"), |path, value| {
            if path == HOST {
                match failure {
                    "custom" => value["features"][2]["namespace"] = json!("custom"),
                    "boolean-query-namespace" => {
                        value["features"][3]["parameters"][2]["queries"][0]["namespace"] =
                            json!("custom");
                    }
                    "boolean-option" => value["features"][3]["parameters"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"parameterId":"offset","value":true})),
                    "missing" => {
                        value["features"][2]["parameters"][0]["value"]["elementId"] =
                            json!("555555555555555555555555");
                    }
                    "version" => {
                        value["features"][2]["parameters"][0]["value"]["versionId"] =
                            json!("666666666666666666666666");
                    }
                    "configuration" => {
                        value["features"][2]["parameters"][0]["configuration"] =
                            json!("size=large");
                    }
                    "placement" => value["features"][2]["parameters"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"parameterId":"mateConnector","value":true})),
                    "subset" => {
                        value["features"][2]["parameters"][0]["queries"] =
                            json!([{"deterministicIds":["another-source-part"]}]);
                    }
                    "source-query" => {
                        value["features"][2]["parameters"][0]["queries"] = json!([{"deterministicIds":["tool"],"queryString":"opaque source selection"}]);
                    }
                    "tools" => {
                        value["features"][3]["parameters"][2]["queries"][0]["deterministicIds"] =
                            json!(["absent"]);
                    }
                    "duplicate" => {
                        value["features"][3]["parameters"][2]["queries"][0]["deterministicIds"] =
                            json!(["inserted", "inserted"]);
                    }
                    _ => {}
                }
            }
            if path == "partstudios/studio/bodydetails/3.json" {
                if failure == "changed-producer" {
                    value["bodies"][0]["properties"]["changed"] = json!(true);
                }
                if failure == "body-name" {
                    value["bodies"][1]["properties"]["name"] = json!("Unmatched");
                }
                if failure == "replaced-body" {
                    value["bodies"].as_array_mut().unwrap().remove(0);
                }
            }
            if failure == "unknown-source-body"
                && path.starts_with("references/")
                && path.ends_with("bodydetails/2.json")
            {
                value["bodies"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"id":"unknown"}));
            }
            if failure == "microversion"
                && path.starts_with("references/")
                && path.ends_with("features.json")
            {
                value["sourceMicroversion"] = json!("other");
            }
            if failure == "source-final-parts"
                && path.starts_with("references/")
                && path.ends_with("bodydetails/index.json")
            {
                *value = json!([0, 1]);
            }
            if failure == "inconsistent" && path == format!("{SOURCE_STUDIO}manifest.json") {
                value["consistent"] = json!(false);
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert!(kinds(&model).contains(&"native".into()), "{failure}");
        assert!(has_unsupported(&report), "{failure}: {:?}", report.losses());
        assert!(model.source_step.is_some());
    }
}

#[test]
fn derived_booleans_keep_operation_and_tools_choice_without_guessing_body_order() {
    for (source, op, keep) in [
        ("UNION", "join", false),
        ("INTERSECTION", "intersect", true),
        ("SUBTRACTION", "cut", true),
    ] {
        let bytes = changed(&fixture("derived"), |path, value| {
            if path == HOST {
                value["features"][3]["parameters"][0]["value"] = json!(source);
                value["features"][3]["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|p| p["parameterId"] == "keepTools")
                    .unwrap()["value"] = json!(keep);
            }
        });
        let (model, report) = read(&bytes).unwrap();
        let wire = timeline(&model);
        let operation = &wire["model"]["features"][3]["definition"]["value"]["value"];
        assert_eq!(operation["op"], op, "{:?}", report.losses());
        assert_eq!(operation["keep_tools"], keep);
    }
}

#[test]
fn derived_studios_validate_their_own_pin_and_continue_after_a_bad_reference_record() {
    for bad_record in [false, true] {
        let bytes = changed(&fixture("derived"), |path, value| {
            if !path.starts_with("references/") {
                return;
            }
            if path.ends_with("manifest.json") {
                value["document"]["microversion"] = json!("referenced-microversion");
            }
            for key in ["sourceMicroversion", "microversionId"] {
                if value.get(key).is_some() {
                    value[key] = json!("referenced-microversion");
                }
            }
            if bad_record && path.ends_with("features.json") {
                value["features"][1]
                    .as_object_mut()
                    .unwrap()
                    .remove("featureId");
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(
            kinds(&model),
            ["sketch", "extrude", "stored_geometry", "combine"],
            "{:?}",
            report.losses()
        );
        let captured = design_reference(&timeline(&model));
        assert_eq!(
            captured["ir"]["model"]["features"]
                .as_array()
                .unwrap()
                .len(),
            if bad_record { 1 } else { 2 }
        );
        assert!(!has_unsupported(&report), "{:?}", report.losses());
        assert_eq!(
            report.losses().iter().any(|loss| loss.code.as_deref()
                == Some("onshape/record.malformed")
                && loss.detail.contains("featureId is missing")),
            bad_record,
            "{:?}",
            report.losses()
        );
    }
}

#[test]
fn derived_boolean_requires_a_stored_keep_tools_choice_and_keeps_later_records() {
    for choice in [Value::Null, json!("false")] {
        let bytes = changed(&fixture("derived"), |path, value| {
            if path == HOST {
                let parameters = value["features"][3]["parameters"].as_array_mut().unwrap();
                if choice.is_null() {
                    parameters.retain(|p| p["parameterId"] != "keepTools");
                } else {
                    parameters
                        .iter_mut()
                        .find(|p| p["parameterId"] == "keepTools")
                        .unwrap()["value"] = choice.clone();
                }
                let mut later = value["features"][0].clone();
                later["featureId"] = json!("later-sketch");
                value["features"].as_array_mut().unwrap().push(later);
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(
            kinds(&model),
            ["sketch", "extrude", "stored_geometry", "native", "sketch"],
            "{:?}",
            report.losses()
        );
        assert!(
            report.losses().iter().any(|loss| loss.code.as_deref()
                == Some("onshape/history.not-supported")
                && (loss.detail.contains("keepTools") || loss.detail.contains("keep-tools"))),
            "{:?}",
            report.losses()
        );
        assert!(model.source_step.is_some());
    }
}

#[test]
fn refused_host_creator_cannot_prove_a_derived_boolean_operand() {
    let bytes = changed(&fixture("derived"), |path, value| {
        if path == HOST {
            value["features"][1]["parameters"]
                .as_array_mut()
                .unwrap()
                .retain(|p| p["parameterId"] != "depth");
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["sketch", "native", "stored_geometry", "native"],
        "{:?}",
        report.losses()
    );
    assert!(model.source_step.is_some());
}

#[test]
fn derived_bundle_admits_the_selected_root_when_it_is_not_the_first_element() {
    let bytes = changed(&fixture("derived"), |path, value| {
        if path == "manifest.json" {
            value["elements"]
                .as_array_mut()
                .unwrap()
                .insert(0, json!({"id":"unrelated","type":"PARTSTUDIO"}));
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["sketch", "extrude", "stored_geometry", "combine"],
        "{:?}",
        report.losses()
    );
}

#[test]
fn derived_boolean_dependencies_follow_stored_operand_order() {
    let bytes = changed(&fixture("derived"), |path, value| {
        if path == HOST {
            value["features"][3]["parameters"]
                .as_array_mut()
                .unwrap()
                .swap(1, 2);
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["sketch", "extrude", "stored_geometry", "combine"],
        "{:?}",
        report.losses()
    );
    let wire = timeline(&model);
    let features = wire["model"]["features"].as_array().unwrap();
    assert_eq!(
        features[3]["dependencies"],
        json!([features[2]["id"], features[1]["id"]])
    );
}
