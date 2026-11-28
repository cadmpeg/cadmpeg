// SPDX-License-Identifier: Apache-2.0
//! Face-removal shell features map to the typed Shell operation.
use crate::test_support::{fixture, review};
use cadmpeg_core::json;
use serde_json::Value;

fn shell_bundle(edit: impl Fn(&mut Vec<Value>)) -> Vec<u8> {
    review::edit(&fixture("plate-fillet"), |path, value| {
        if path.ends_with("/bodydetails/2.json") {
            value["bodies"][0]["faces"] = json!([{
                "id": "opening", "type": "PLANE",
                "surface": {"type": "PLANE", "origin": {"x": 0, "y": 0, "z": 0.002},
                    "normal": {"x": 0.0, "y": 0.0, "z": 1.0}},
                "loops": [{"coedges": [{"edgeId": "upright"}]}]
            }]);
        }
        if path.ends_with("/features.json") {
            let feature = &mut value["features"][2];
            feature["featureType"] = json!("shell");
            feature["name"] = json!("Wall");
            let mut parameters = vec![
                json!({"parameterId": "entities", "queries": [
                    {"btType": "BTMIndividualQuery-138", "deterministicIds": ["opening"]}]}),
                json!({"parameterId": "thickness", "expression": "0.5 mm"}),
                json!({"parameterId": "oppositeDirection", "value": false}),
            ];
            edit(&mut parameters);
            feature["parameters"] = Value::Array(parameters);
        }
    })
}

fn last_feature(wire: &Value) -> &Value {
    wire["model"]["features"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
}

fn losses(result: &cadmpeg_ir::codec::DecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.to_string())
        .collect()
}

#[test]
fn an_inward_face_removal_shell_maps_with_its_thickness_and_dependency() {
    let result = review::decode(&shell_bundle(|_| {}));
    let wire = review::wire(&result);
    let feature = last_feature(&wire);
    assert_eq!(feature["definition"]["value"]["kind"], "shell");
    let shell = &feature["definition"]["value"]["value"];
    assert_eq!(shell["thickness"], json!(0.5));
    assert_eq!(shell["outward"], json!(false));
    assert_eq!(shell["removed_faces"]["kind"], "historical");
    assert_eq!(shell["removed_faces"]["value"]["faces"], json!(["opening"]));
    assert!(!losses(&result).iter().any(|code| code.contains("shell")));
    let design = review::design_operation(result.ir(), "Corner round");
    assert_eq!(design["kind"], "shell");
    assert_eq!(
        design["value"]["removed"]["picks"]["value"],
        json!([{"target":{"kind":"native","value":{}},
            "native":{"codec":"onshape","value":{"kind":"text","value":"opening"}}}])
    );
    assert_eq!(
        design["value"]["thickness"],
        json!({"kind":"one_side","value":{"value":{"kind":"literal","value":0.5},"side":{"kind":"inside"}}})
    );
    review::validate(result);
}

#[test]
fn shell_controls_outside_the_stored_forms_keep_the_feature_native_with_a_named_loss() {
    type Edit = Box<dyn Fn(&mut Vec<Value>)>;
    let cases: Vec<(&str, Edit)> = vec![
        (
            "hollow",
            Box::new(|p| p.push(json!({"parameterId": "isHollow", "value": true}))),
        ),
        (
            "part selection",
            Box::new(|p| p.push(json!({"parameterId": "parts", "queries": []}))),
        ),
        (
            "repeated part selection",
            Box::new(|p| {
                p.push(json!({"parameterId": "parts", "queries": []}));
                p.push(json!({"parameterId": "parts", "queries": []}));
            }),
        ),
        (
            "unlisted control",
            Box::new(|p| p.push(json!({"parameterId": "unlisted", "value": false}))),
        ),
    ];
    for (name, edit) in cases {
        let result = review::decode(&shell_bundle(edit));
        let wire = review::wire(&result);
        assert_eq!(
            last_feature(&wire)["definition"]["value"]["kind"],
            "native",
            "{name}"
        );
        let codes = losses(&result);
        // A repeated parameter is a malformed record, refused before shell support is assessed.
        let want = if name == "unlisted control" || name == "repeated part selection" {
            "onshape/history.not-supported"
        } else {
            "onshape/shell.not-supported"
        };
        assert!(codes.iter().any(|code| code == want), "{name}: {codes:?}");
    }
}

#[test]
fn a_shell_without_a_resolvable_removed_face_stays_native() {
    for ids in [json!(["absent"]), json!([])] {
        let result = review::decode(&shell_bundle(|p| {
            p[0] = json!({"parameterId": "entities", "queries": [
                {"btType": "BTMIndividualQuery-138", "deterministicIds": ids.clone()}]});
        }));
        let wire = review::wire(&result);
        assert_eq!(last_feature(&wire)["definition"]["value"]["kind"], "native");
        assert!(losses(&result)
            .iter()
            .any(|code| code == "onshape/history.not-supported"));
    }
}
