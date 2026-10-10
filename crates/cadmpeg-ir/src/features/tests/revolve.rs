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

#[test]
fn revolve_setters_preserve_every_required_input_and_selection_state() {
    use crate::features::{PlanarProfileRef, RevolutionAxis, RevolveConstruction};
    let profile = serde_json::json!({"kind": "native", "value": "test:profile:é"});
    let replacement = serde_json::json!({"kind": "native", "value": "test:replacement:🦀"});
    let axis = serde_json::json!({
        "origin": {"x": -0.0, "y": 0.0, "z": 1.0},
        "direction": {"x": 0.0, "y": 0.0, "z": 1.0},
        "reference": {"kind": "native", "value": "test:axis:é"}
    });
    let extent = serde_json::json!({"kind": "two_sided",
        "first": {"kind": "to_face", "face": {"kind": "native", "value": "test:face:🦀"}},
        "second": {"kind": "angle", "angle": 1.25}
    });
    let wire = |profile: Option<&serde_json::Value>,
                axis: Option<&serde_json::Value>,
                extent: Option<&serde_json::Value>| {
        let mut wire = serde_json::json!({"solid": false,
            "face_maker_class": "Extension::FaceMakeré", "fuse_order": "feature_first",
            "allow_multi_profile_faces": true});
        wire["state"] =
            serde_json::json!(if profile.is_some() && axis.is_some() && extent.is_some() {
                "resolved"
            } else {
                "unresolved"
            });
        if profile.is_none() {
            wire["missing"] = serde_json::json!("profile");
        } else if axis.is_none() {
            wire["missing"] = serde_json::json!("axis");
        } else if extent.is_none() {
            wire["missing"] = serde_json::json!("extent");
        }
        for (key, value) in [("profile", profile), ("axis", axis), ("extent", extent)] {
            if let Some(value) = value {
                wire[key] = value.clone();
            }
        }
        wire
    };
    for mask in 0..8 {
        let original_profile = (mask & 1 != 0).then_some(&profile);
        let original_axis = (mask & 2 != 0).then_some(&axis);
        let original_extent = (mask & 4 != 0).then_some(&extent);
        let original = wire(original_profile, original_axis, original_extent);
        for present in [false, true] {
            let mut construction: RevolveConstruction =
                serde_json::from_value(original.clone()).unwrap();
            let new_profile = present.then_some(&replacement);
            construction.set_profile(
                new_profile.map(|value| {
                    serde_json::from_value::<PlanarProfileRef>(value.clone()).unwrap()
                }),
            );
            assert_eq!(
                serde_json::to_value(&construction).unwrap(),
                wire(new_profile, original_axis, original_extent)
            );
            let mut construction: RevolveConstruction =
                serde_json::from_value(original.clone()).unwrap();
            let new_axis = present.then_some(&axis);
            construction.set_axis(
                new_axis
                    .map(|value| serde_json::from_value::<RevolutionAxis>(value.clone()).unwrap()),
            );
            assert_eq!(
                serde_json::to_value(&construction).unwrap(),
                wire(original_profile, new_axis, original_extent)
            );
        }
    }
}

#[test]
fn revolve_setters_move_unchanged_native_backing() {
    use crate::features::{
        AngularTermination, FaceSelection, PathRef, PlanarProfileRef, RevolveConstruction,
        RevolveExtent,
    };
    let make = || {
        serde_json::from_value::<RevolveConstruction>(serde_json::json!({
            "state": "resolved",
            "profile": {"kind": "native", "value": "test:profile:é"},
            "axis": {
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "direction": {"x": 0.0, "y": 0.0, "z": 1.0},
                "reference": {"kind": "native", "value": "test:axis:é"}
            },
            "extent": {"kind": "one_sided", "termination": {
                "kind": "to_face", "face": {"kind": "native", "value": "test:face:🦀"}
            }},
            "face_maker_class": "Extension::FaceMakeré"
        }))
        .unwrap()
    };
    let extent_address = |construction: &RevolveConstruction| {
        let Some(RevolveExtent::OneSided {
            termination:
                AngularTermination::ToFace {
                    face: FaceSelection::Native(face),
                    ..
                },
        }) = construction.extent()
        else {
            panic!("native extent fixture");
        };
        face.as_str().as_ptr()
    };
    let mut construction = make();
    let face_maker = construction.face_maker().unwrap().as_str().as_ptr();
    let Some(PathRef::Native(axis)) = construction.axis().unwrap().reference.as_ref() else {
        panic!("native axis fixture");
    };
    let axis = axis.as_str().as_ptr();
    let extent = extent_address(&construction);
    construction.set_profile(None);
    assert_eq!(
        construction.face_maker().unwrap().as_str().as_ptr(),
        face_maker
    );
    let Some(PathRef::Native(after)) = construction.axis().unwrap().reference.as_ref() else {
        panic!("axis retained");
    };
    assert_eq!(after.as_str().as_ptr(), axis);
    assert_eq!(extent_address(&construction), extent);

    let mut construction = make();
    let face_maker = construction.face_maker().unwrap().as_str().as_ptr();
    let Some(PlanarProfileRef::Native(profile)) = construction.profile() else {
        panic!("native profile fixture");
    };
    let profile = profile.as_str().as_ptr();
    let extent = extent_address(&construction);
    construction.set_axis(None);
    assert_eq!(
        construction.face_maker().unwrap().as_str().as_ptr(),
        face_maker
    );
    let Some(PlanarProfileRef::Native(after)) = construction.profile() else {
        panic!("profile retained");
    };
    assert_eq!(after.as_str().as_ptr(), profile);
    assert_eq!(extent_address(&construction), extent);
}
