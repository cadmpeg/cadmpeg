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

/// Agreement of a typed feature value after a write, per unit of its
/// magnitude.
const VALUE_TOLERANCE: f64 = 1e-12;

/// Whether `left` and `right` agree, each number within `VALUE_TOLERANCE`
/// per unit of its magnitude.
fn agree(left: &serde_json::Value, right: &serde_json::Value) -> bool {
    match (left, right) {
        (serde_json::Value::Number(a), serde_json::Value::Number(b)) => {
            let (a, b) = (
                a.as_f64().unwrap_or(f64::NAN),
                b.as_f64().unwrap_or(f64::NAN),
            );
            (a - b).abs() <= VALUE_TOLERANCE * a.abs().max(1.0)
        }
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| agree(a, b))
        }
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| agree(a, b)))
        }
        (a, b) => a == b,
    }
}

#[test]
fn typed_feature_families_survive_a_write() {
    let mut ir = model();
    let definitions = [
        serde_json::json!({"definition": "extrude", "direction": {"kind": "explicit", "source": {"kind": "custom"}, "vector": {"x": 0.0, "y": 0.0, "z": 1.0}}, "extent": {"kind": "symmetric", "side": {"draft": std::f64::consts::PI / 60.0, "termination": {"kind": "blind", "length": 12.0}}}, "inner_wire_taper": "inverted", "op": "new_body", "profile": {"kind": "native", "value": "test:native:property#Profile"}, "solid": true, "start": {"kind": "profile_plane"}}),
        serde_json::json!({"definition": "extrude", "direction": {"kind": "profile_normal"}, "extent": {"kind": "two_sided", "first": {"termination": {"kind": "blind", "length": 5.0}}, "second": {"termination": {"kind": "through_all"}}}, "op": "cut", "profile": {"kind": "unresolved", "value": "test:model:feature#8053"}, "start": {"kind": "profile_plane"}}),
        serde_json::json!({"definition": "post_process", "refine": true, "fuzzy_tolerance": {"kind": "kernel_default"}, "operation": {"definition": "extrude", "allow_multi_profile_faces": true, "length_along_profile_normal": true, "direction": {"kind": "explicit", "source": {"kind": "profile_normal"}, "vector": {"x": 0.0, "y": 0.0, "z": 1.0}}, "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 3.5}}}, "op": "join", "profile": {"kind": "unresolved", "value": "x"}, "start": {"kind": "profile_plane"}}}),
        serde_json::json!({"construction": {"axis": {"direction": {"x": 0.0, "y": 1.0, "z": 0.0}, "origin": {"x": 1.0, "y": 2.0, "z": 3.0}}, "extent": {"kind": "symmetric", "termination": {"angle": 1.25 * std::f64::consts::PI, "kind": "angle"}}, "face_maker_class": "Part::FaceMakerBullseye", "profile": {"kind": "unresolved", "value": "test:model:sketch#R"}, "solid": false, "state": "resolved"}, "definition": "revolve", "op": "new_body"}),
        serde_json::json!({"definition": "fillet", "groups": [{"edges": {"kind": "unresolved"}, "radius": {"kind": "constant", "radius": 2.0}}, {"edges": {"kind": "unresolved"}, "radius": {"kind": "unresolved"}}]}),
        serde_json::json!({"definition": "chamfer", "flip_direction": false, "groups": [{"edges": {"kind": "native", "value": "test:native:property#Base"}, "spec": {"first": 1.5, "kind": "two_distances", "second": 2.5}}]}),
        serde_json::json!({"definition": "pattern", "pattern": {"count": 4, "direction": {"x": -0.0, "y": -0.0, "z": -1.0}, "kind": "linear", "spacing": 8.0}, "seeds": [{"kind": "feature", "value": "test:design:feature#Plane"}]}),
        serde_json::json!({"allow_multi_profile_faces": true, "bottom": {"depth_to_tip": false, "included_angle": 2.0, "kind": "angled"}, "definition": "hole", "direction": {"x": -0.0, "y": -0.0, "z": -1.0}, "extent": {"kind": "blind", "length": 7.0}, "profile": {"kind": "unresolved", "value": "test:model:sketch#HoleProfile"}, "profile_filter": "all", "shape": {"construction": {"construction": "form", "kind": {"angle": 1.5, "diameter": 10.0, "kind": "countersink"}}, "diameter": 6.0}}),
        serde_json::json!({"allow_self_intersections": false, "definition": "shell", "join": "arc", "mode": "skin", "outward": true, "removed_faces": {"kind": "resolved", "value": {"faces": ["test:model:face#32"], "native": "test:native:property#Faces"}}, "resolve_intersections": true, "thickness": 1.25}),
        serde_json::json!({"anchor": {"kind": "neutral_plane", "plane": {"kind": "native", "value": "test:native:property#NeutralPlane"}, "pull": {"direction": {"x": 0.0, "y": 0.0, "z": 1.0}}}, "angle": -0.25, "definition": "draft", "faces": {"kind": "resolved", "value": {"faces": ["test:model:face#10", "test:model:face#20"], "native": "test:native:property#Base"}}, "outward": true}),
        serde_json::json!({"definition": "mirror_shape", "plane_normal": {"x": 1.0, "y": 0.0, "z": 0.0}, "plane_origin": {"x": 0.0, "y": 0.0, "z": 0.0}, "source": {"kind": "native", "value": "test:native:property#Source"}}),
        serde_json::json!({"definition": "combine", "op": "cut", "operands": {"target": {"kind": "native", "value": "test:native:property#Base"}, "tools": {"kind": "native", "value": "test:native:property#Tool"}}}),
        serde_json::json!({"definition": "primitive", "op": "new_body", "placement": [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]], "solid": {"height": 8.0, "kind": "box", "length": 25.0, "width": 15.0}}),
        serde_json::json!({"definition": "datum_axis", "direction": {"x": 0.0, "y": 0.0, "z": 1.0}, "origin": {"x": 2.0, "y": 3.0, "z": 0.0}}),
        serde_json::json!({"definition": "datum_coordinate_system", "frame": {"origin": {"x": 7.0, "y": 8.0, "z": 9.0}, "x_axis": {"x": 1.0, "y": 0.0, "z": 0.0}, "y_axis": {"x": 0.0, "y": 1.0, "z": 0.0}, "z_axis": {"x": 0.0, "y": 0.0, "z": 1.0}}}),
        serde_json::json!({"definition": "datum_point", "position": {"x": 5.0, "y": -7.0, "z": 3.0}}),
    ];
    for (index, definition) in definitions.into_iter().enumerate() {
        let ordinal = u64::try_from(index).expect("an ordinal") + 3;
        let parsed: Result<Feature, _> = serde_json::from_value(serde_json::json!({
            "id": format!("test:design:feature#typed{index}"),
            "ordinal": ordinal,
            "suppressed": null,
            "definition": definition,
        }));
        match parsed {
            Ok(feature) => ir.model.features.push(feature),
            Err(error) => panic!("definition {index}: {error}"),
        }
    }
    let (text, _) = encode(&ir);
    for prim_type in [
        "def CadExtrudeFeature",
        "def CadRevolveFeature",
        "def CadFilletFeature",
        "def CadChamferFeature",
        "def CadPatternFeature",
        "def CadHoleFeature",
        "def CadShellFeature",
        "def CadDraftFeature",
        "def CadMirrorFeature",
        "def CadCombineFeature",
        "def CadPrimitiveFeature",
        "def CadDatumAxisFeature",
        "def CadDatumFrameFeature",
        "def CadDatumPointFeature",
        "def CadDatumPlaneFeature",
    ] {
        assert!(text.contains(prim_type), "{prim_type}\n{text}");
    }
    assert!(
        text.contains("cad:extrude:extent:side:termination:length = 0.012"),
        "{text}"
    );
    let read = decode(&text);
    let wire = |features: &[Feature]| {
        let mut sorted: Vec<&Feature> = features.iter().collect();
        sorted.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        serde_json::to_value(sorted).expect("features")
    };
    let (before, after) = (wire(&ir.model.features), wire(&read.ir().model.features));
    assert!(agree(&before, &after), "{before:#}\n{after:#}\n{text}");
}
