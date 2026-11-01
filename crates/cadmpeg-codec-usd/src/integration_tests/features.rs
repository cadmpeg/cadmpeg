// SPDX-License-Identifier: Apache-2.0
//! Features survive a write as `CadFeature` prims (spec §18.7).

use cadmpeg_ir::features::{DesignConfiguration, Feature};

use crate::test_support::{decode, encode};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/golden/fixtures/{name}.usda",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture")
}

fn feature(value: serde_json::Value) -> Feature {
    serde_json::from_value(value).expect("a feature")
}

/// The sketch fixture with a datum plane, a sketch feature on its sketch, a
/// tree node over both, and a parameter that the sketch feature owns.
fn model() -> cadmpeg_ir::CadIr {
    let source = decode(&fixture("sketch_curved"));
    let mut ir = source.ir().clone();
    let sketch = ir.model.sketches[0].id.to_string();
    ir.model.features = vec![
        feature(serde_json::json!({
            "id": "test:design:feature#Body",
            "ordinal": 0,
            "name": "Body",
            "suppressed": null,
            "definition": {
                "definition": "tree_node",
                "role": "solid_bodies",
                "children": {
                    "active_child": "test:design:feature#Sketch",
                    "children": ["test:design:feature#Plane", "test:design:feature#Sketch"]
                }
            }
        })),
        feature(serde_json::json!({
            "id": "test:design:feature#Plane",
            "ordinal": 1,
            "name": "Plane",
            "suppressed": false,
            "source_properties": {"MapMode": "5"},
            "source_tag": "PartDesign::Plane",
            "definition": {
                "definition": "datum_plane",
                "frame": {
                    "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
                    "origin": {"x": 0.0, "y": 0.0, "z": 3.0},
                    "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
                }
            }
        })),
        feature(serde_json::json!({
            "id": "test:design:feature#Sketch",
            "ordinal": 2,
            "name": "Sketch",
            "suppressed": null,
            "dependencies": ["test:design:feature#Plane"],
            "definition": {
                "definition": "sketch",
                "sketch": {"sketch": sketch, "space": "planar"}
            }
        })),
    ];
    let owned = ir.model.parameters[0].id.clone();
    if let Some(parameter) = ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == owned)
    {
        parameter.owner = Some(ir.model.features[2].id.clone());
    }
    let configuration = |key: &str, value: serde_json::Value| {
        let mut wire = serde_json::json!({"id": format!("test:design:configuration#{key}")});
        if let (serde_json::Value::Object(wire), serde_json::Value::Object(value)) =
            (&mut wire, value)
        {
            wire.extend(value);
        }
        serde_json::from_value::<DesignConfiguration>(wire).expect("a configuration")
    };
    ir.model.configurations = vec![
        configuration(
            "Default",
            serde_json::json!({"ordinal": 0, "active": true, "name": "Default"}),
        ),
        configuration(
            "Long",
            serde_json::json!({
                "ordinal": 1,
                "name": "Long",
                "source_index": 4,
                "material": "6061-T6",
                "properties": {"Description": "long variant"},
                "parameter_overrides": {owned.to_string(): "= 40mm"},
                "parameter_values": {owned.to_string(): {"kind": "length", "value": 40.0}},
                "feature_states": {
                    "test:design:feature#Plane": {
                        "evaluation": {"kind": "suppressed"},
                        "dependencies": [],
                        "definition": {
                            "definition": "datum_plane",
                            "frame": {
                                "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
                                "origin": {"x": 0.0, "y": 0.0, "z": 5.0},
                                "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
                            }
                        }
                    }
                }
            }),
        ),
    ];
    ir
}

#[test]
fn features_survive_a_write() {
    let ir = model();
    let (text, report) = encode(&ir);
    assert!(text.contains("def CadFeature"), "{text}");
    let unwritten = crate::loss::UsdLossCode::ExportFeatureNotWritten.kind();
    assert!(
        report.losses.iter().all(|loss| loss.code != unwritten),
        "{:?}",
        report.losses
    );
    let read = decode(&text);
    let after = &read.ir().model;
    assert_eq!(after.features, ir.model.features);
    assert_eq!(after.configurations, ir.model.configurations);
    let owner = |model: &cadmpeg_ir::document::Model| {
        model
            .parameters
            .iter()
            .map(|parameter| (parameter.id.to_string(), parameter.owner.clone()))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    assert_eq!(owner(after), owner(&ir.model));
}

#[test]
fn a_feature_that_names_a_rejected_feature_is_not_transferred() {
    let ir = model();
    let (text, _) = encode(&ir);
    let broken = text.replacen(
        "uniform string cad:feature:payload = \"{\\\"definition\\\":{\\\"definition\\\":\\\"datum_plane\\\"",
        "uniform string cad:feature:payload = \"{\\\"definition\\\":{\\\"definition\\\":\\\"no_such_operation\\\"",
        1,
    );
    assert_ne!(broken, text);
    let read = decode(&broken);
    // The plane does not deserialize; the sketch depends on it and the tree
    // node names it, so neither is transferred.
    assert!(read.ir().model.features.is_empty());
}
