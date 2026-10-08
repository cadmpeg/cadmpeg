// SPDX-License-Identifier: Apache-2.0
//! Revolve-construction ownership transitions.

#[test]
fn revolve_set_axis_preserves_owned_profile_extent_and_selections() {
    use crate::features::{RevolutionAxis, RevolveConstruction};

    let axis = serde_json::json!({
        "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "direction": {"x": 0.0, "y": 0.0, "z": 1.0}
    });
    let mut construction: RevolveConstruction = serde_json::from_value(serde_json::json!({
        "state": "unresolved",
        "missing": "axis",
        "profile": {"kind": "native", "value": "test:profile"},
        "extent": {"kind": "one_sided", "termination": {"kind": "angle", "angle": 1.25}},
        "solid": true,
        "face_maker_class": "Part::FaceMakerBullseye"
    }))
    .unwrap();
    construction.set_axis(Some(
        serde_json::from_value::<RevolutionAxis>(axis.clone()).unwrap(),
    ));
    assert_eq!(
        serde_json::to_value(construction).unwrap(),
        serde_json::json!({
            "state": "resolved",
            "profile": {"kind": "native", "value": "test:profile"},
            "axis": axis,
            "extent": {"kind": "one_sided", "termination": {"kind": "angle", "angle": 1.25}},
            "solid": true,
            "face_maker_class": "Part::FaceMakerBullseye"
        })
    );
}

#[test]
fn revolve_construction_carries_its_axis_reference_inside_the_axis() {
    use crate::features::{FeatureDefinition, FeatureOperation, PathRef, RevolveConstruction};

    let wire = serde_json::json!({
        "definition": "revolve",
        "construction": {
            "state": "resolved",
            "profile": {"kind": "sketch", "value": "test:model:sketch#profile"},
            "axis": {
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "direction": {"x": 0.0, "y": 0.0, "z": 1.0},
                "reference": {"kind": "native", "value": "test:axis"}
            },
            "extent": {
                "kind": "one_sided",
                "termination": {"kind": "angle", "angle": 1.25}
            },
            "solid": true,
            "face_maker_class": "Part::FaceMakerBullseye"
        },
        "op": "new_body"
    });
    let definition: FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            construction: RevolveConstruction::Resolved { ref axis, .. },
            ..
        }) if axis.reference == Some(PathRef::Native("test:axis".into()))
    ));
    assert_eq!(serde_json::to_value(definition).unwrap(), wire);

    let mut sibling = wire;
    sibling["construction"].as_object_mut().unwrap().insert(
        "axis_reference".to_string(),
        serde_json::json!({"kind": "native", "value": "test:axis"}),
    );
    let error = serde_json::from_value::<FeatureOperation>(sibling)
        .unwrap_err()
        .to_string();
    assert!(error.contains("axis_reference"), "{error}");
}

#[test]
fn revolve_construction_admits_only_typed_partial_states() {
    use crate::features::{FeatureDefinition, FeatureOperation, RevolveConstruction};

    let partial_wire = serde_json::json!({
        "definition": "revolve",
        "construction": {
            "state": "unresolved",
            "missing": "axis",
            "profile": {"kind": "native", "value": "test:profile"},
            "solid": false
        },
        "op": "unresolved"
    });
    let partial: FeatureDefinition = serde_json::from_value(partial_wire.clone()).unwrap();
    assert!(matches!(
        partial,
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            construction: RevolveConstruction::Unresolved(_),
            ..
        })
    ));
    assert_eq!(serde_json::to_value(partial).unwrap(), partial_wire);

    let resolved_without_a_profile = serde_json::json!({
        "definition": "revolve",
        "construction": {
            "state": "resolved",
            "axis": {
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "direction": {"x": 0.0, "y": 0.0, "z": 1.0}
            },
            "extent": {
                "kind": "one_sided",
                "termination": {"kind": "angle", "angle": 1.25}
            }
        },
        "op": "unresolved"
    });
    let error = serde_json::from_value::<FeatureOperation>(resolved_without_a_profile)
        .unwrap_err()
        .to_string();
    assert!(error.contains("profile"), "{error}");
}

#[test]
fn an_unresolved_revolve_names_its_first_missing_operand() {
    use crate::features::{PartialRevolveConstruction, RevolveConstruction};

    let axis = serde_json::json!({
        "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "direction": {"x": 0.0, "y": 0.0, "z": 1.0}
    });
    let extent = serde_json::json!({
        "kind": "one_sided",
        "termination": {"kind": "angle", "angle": 1.25}
    });
    let profile = serde_json::json!({"kind": "native", "value": "test:profile"});

    let contradicting = serde_json::json!({
        "state": "unresolved",
        "missing": "axis",
        "profile": profile,
        "axis": axis,
    });
    let error = serde_json::from_value::<RevolveConstruction>(contradicting)
        .unwrap_err()
        .to_string();
    assert!(error.contains("axis"), "{error}");

    let untyped = serde_json::json!({
        "state": "unresolved",
        "profile": profile,
        "axis": axis,
        "extent": extent,
    });
    let error = serde_json::from_value::<RevolveConstruction>(untyped)
        .unwrap_err()
        .to_string();
    assert!(error.contains("missing"), "{error}");

    let named = serde_json::json!({
        "state": "unresolved",
        "missing": "profile",
        "axis": axis,
    });
    let construction: RevolveConstruction =
        serde_json::from_value(named.clone()).expect("reads the named partial state");
    assert!(matches!(
        construction,
        RevolveConstruction::Unresolved(PartialRevolveConstruction::Profile {
            axis: Some(_),
            extent: None,
            ..
        })
    ));
    assert_eq!(serde_json::to_value(&construction).unwrap(), named);
}
