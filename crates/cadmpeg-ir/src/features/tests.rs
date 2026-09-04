// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::features::TrimCellSelection;
use crate::math::{Point3, Vector3};
use crate::validate::validate_neutral;
use crate::CadIr;

#[test]
fn configuration_body_membership_round_trips_and_validates() {
    use crate::features::{
        Angle, ConfigurationFeatureState, ConfigurationId, DesignConfiguration, DesignParameter,
        Feature, FeatureDefinition, FeatureId, Length, ParameterId, ParameterValue,
    };
    use crate::ids::BodyId;
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let configuration_id = ConfigurationId("synthetic:test:configuration#0".into());
    let parameter_id = ParameterId("synthetic:test:parameter#width".into());
    let body = ir.model.bodies[0].id.clone();
    ir.model.parameters.push(DesignParameter {
        id: parameter_id.clone(),
        owner: None,
        ordinal: 0,
        name: "width".into(),
        expression: "10 mm".into(),
        display: None,
        value: None,
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: configuration_id.clone(),
        ordinal: 0,
        active: false,
        source_index: Some(7),
        name: "Default".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::from([(parameter_id.clone(), "25 mm".into())]),
        bodies: crate::features::ConfigurationBodies::Resolved(vec![body.clone()]),
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    ir.finalize();
    assert!(validate_neutral(&ir, Vec::new()).is_ok());
    let round_trip = CadIr::from_json(&serde_json::to_string(&ir).unwrap()).unwrap();
    assert_eq!(
        round_trip.model.configurations[0].bodies,
        vec![body.clone()]
    );
    assert_eq!(
        round_trip.model.configurations[0].parameter_overrides[&parameter_id],
        "25 mm"
    );

    ir.model.configurations[0].parameter_overrides = BTreeMap::from([(
        ParameterId("synthetic:test:parameter#missing".into()),
        "30 mm".into(),
    )]);
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message.contains("configuration parameter override")
    }));
    ir.model.configurations[0].parameter_overrides.clear();

    ir.model.configurations[0].parameter_values = BTreeMap::from([(
        ParameterId("synthetic:test:parameter#missing-value".into()),
        ParameterValue::Real(1.0),
    )]);
    ir.model.configurations[0].feature_states = BTreeMap::from([(
        FeatureId("synthetic:test:feature#missing-state".into()),
        ConfigurationFeatureState {
            suppressed: false,
            dependencies: vec![FeatureId(
                "synthetic:test:feature#missing-dependency".into(),
            )],
            outputs: vec![BodyId("synthetic:test:body#missing-output".into())],
            definition: FeatureDefinition::DatumPoint {
                position: Point3::new(0.0, 0.0, 0.0),
                construction: None,
            },
        },
    )]);
    let report = validate_neutral(&ir, Vec::new());
    for reference in [
        "configuration parameter value",
        "configuration feature state",
        "configuration feature dependency",
        "configuration feature output",
    ] {
        assert!(report.findings.iter().any(|finding| {
            finding.entity.as_deref() == Some(configuration_id.0.as_str())
                && finding.message.contains(reference)
        }));
    }
    ir.model.configurations[0].parameter_values.clear();
    ir.model.configurations[0].feature_states.clear();

    ir.model.parameters[0].value = Some(ParameterValue::Length(Length(10.0)));
    ir.model.configurations[0].parameter_values =
        BTreeMap::from([(parameter_id.clone(), ParameterValue::Angle(Angle(1.0)))]);
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message == "configuration parameter value is invalid"
    }));
    ir.model.configurations[0].parameter_values.clear();

    ir.model.parameters[0].value = Some(ParameterValue::Real(f64::NAN));
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(parameter_id.0.as_str())
            && finding.message == "parameter value is invalid"
    }));
    ir.model.parameters[0].value = None;

    let first_feature = FeatureId("synthetic:test:feature#configuration-first".into());
    let later_feature = FeatureId("synthetic:test:feature#configuration-later".into());
    for (ordinal, feature) in [first_feature.clone(), later_feature.clone()]
        .into_iter()
        .enumerate()
    {
        ir.model.features.push(Feature {
            id: feature,
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::DatumPoint {
                position: Point3::new(0.0, 0.0, 0.0),
                construction: None,
            },
            native_ref: None,
        });
    }
    ir.model.configurations[0].feature_states = BTreeMap::from([(
        first_feature.clone(),
        ConfigurationFeatureState {
            suppressed: false,
            dependencies: vec![later_feature.clone(), later_feature.clone()],
            outputs: vec![body.clone(), body.clone()],
            definition: FeatureDefinition::DatumPoint {
                position: Point3::new(0.0, 0.0, 0.0),
                construction: None,
            },
        },
    )]);
    let report = validate_neutral(&ir, Vec::new());
    for message in [
        "does not precede",
        "repeats dependency",
        "repeats output body",
    ] {
        assert!(report.findings.iter().any(|finding| {
            finding.entity.as_deref() == Some(configuration_id.0.as_str())
                && finding.message.contains(message)
        }));
    }
    ir.model.configurations[0].feature_states.clear();

    ir.model.configurations[0].feature_states = BTreeMap::from([(
        first_feature.clone(),
        ConfigurationFeatureState {
            suppressed: true,
            dependencies: Vec::new(),
            outputs: vec![body.clone()],
            definition: FeatureDefinition::DatumPoint {
                position: Point3::new(0.0, 0.0, 0.0),
                construction: None,
            },
        },
    )]);
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message == "suppressed configuration feature state has output bodies"
    }));
    ir.model.configurations[0].feature_states.clear();

    ir.model.configurations[0].active = true.into();
    ir.model.features[0].suppressed = Some(true);
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message
                == "active configuration suppression disagrees with current feature state"
    }));
    ir.model.configurations[0].active = false.into();
    ir.model.features[0].suppressed = Some(false);

    ir.model.configurations[0].feature_states = BTreeMap::from([(
        later_feature.clone(),
        ConfigurationFeatureState {
            suppressed: false,
            dependencies: vec![first_feature.clone()],
            outputs: vec![body.clone()],
            definition: FeatureDefinition::DatumPoint {
                position: Point3::new(0.0, 0.0, 0.0),
                construction: None,
            },
        },
    )]);
    // A dependency with no state in this configuration inherits its model-level
    // state; `feature_states` is allowed to be sparse, so that is not a finding.
    assert!(validate_neutral(&ir, Vec::new()).is_ok());
    ir.model.configurations[0].feature_states.insert(
        first_feature.clone(),
        ConfigurationFeatureState {
            suppressed: true,
            dependencies: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::DatumPoint {
                position: Point3::new(0.0, 0.0, 0.0),
                construction: None,
            },
        },
    );
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message
                == format!(
                    "configuration state closure uses suppressed dependency state `{}`",
                    first_feature.0
                )
    }));
    ir.model.configurations[0]
        .feature_states
        .get_mut(&first_feature)
        .expect("dependency state")
        .suppressed = false;
    assert!(validate_neutral(&ir, Vec::new()).is_ok());
    ir.model.configurations[0].feature_states.clear();

    ir.model.configurations[0].bodies = crate::features::ConfigurationBodies::Resolved(vec![
        BodyId("synthetic:test:body#missing".into()),
        BodyId("synthetic:test:body#missing".into()),
    ]);
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message.contains("missing configuration body")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message.contains("repeats body")
    }));

    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("synthetic:test:configuration#1".into()),
        ordinal: 0,
        active: false,
        source_index: Some(7),
        name: "Alternate".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies: crate::features::ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    ir.model.configurations[0].active = true.into();
    ir.model.configurations[1].active = true.into();
    ir.finalize();
    let report = validate_neutral(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("repeats configuration ordinal")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("multiple active configurations")));
    assert!(report.findings.iter().any(|finding| finding
        .message
        .contains("repeats configuration source index")));
}

#[test]
fn configuration_name_preserves_resolution_state() {
    use crate::features::{ConfigurationName, DesignConfiguration};

    let configuration: DesignConfiguration = serde_json::from_value(serde_json::json!({
        "id": "synthetic:test:configuration#0"
    }))
    .expect("legacy configuration");
    assert_eq!(configuration.name, ConfigurationName::Unresolved);
    assert!(!configuration.active);

    let encoded = serde_json::to_value(&configuration).expect("unresolved configuration");
    assert!(encoded.get("name").is_none());
    assert!(encoded.get("active").is_none());
    let round_trip: DesignConfiguration =
        serde_json::from_value(encoded).expect("round-trip unresolved configuration");
    assert_eq!(round_trip.name, ConfigurationName::Unresolved);
    assert!(!round_trip.active);
}

#[test]
fn configuration_suppression_is_derived_and_legacy_lists_migrate_at_the_model_boundary() {
    use crate::features::{
        ConfigurationBodies, ConfigurationFeatureState, ConfigurationId, DesignConfiguration,
        Feature, FeatureDefinition, FeatureId,
    };
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let feature = Feature::new(
        FeatureId("synthetic:test:feature#suppressed".into()),
        0,
        FeatureDefinition::DatumPoint {
            position: Point3::new(0.0, 0.0, 0.0),
            construction: None,
        },
    );
    ir.model.features.push(feature.clone());
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("synthetic:test:configuration#suppressed".into()),
        ordinal: 0,
        active: false,
        source_index: None,
        name: "Suppressed".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies: ConfigurationBodies::Unresolved,
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::from([(
            feature.id.clone(),
            ConfigurationFeatureState {
                suppressed: true,
                dependencies: feature.dependencies.clone(),
                outputs: Vec::new(),
                definition: feature.definition.clone(),
            },
        )]),
        native_ref: None,
    });

    let mut wire = serde_json::to_value(&ir).unwrap();
    let configuration = &mut wire["model"]["configurations"][0];
    assert_eq!(
        configuration["suppressed_features"],
        serde_json::json!([feature.id.0.clone()])
    );
    configuration
        .as_object_mut()
        .unwrap()
        .remove("feature_states");
    let migrated = serde_json::from_value::<CadIr>(wire).unwrap();
    let state = &migrated.model.configurations[0].feature_states[&feature.id];
    assert!(state.suppressed);
    assert_eq!(state.definition, feature.definition);
    assert!(state.outputs.is_empty());

    let mut invalid = serde_json::to_value(&ir).unwrap();
    invalid["model"]["configurations"][0]["feature_states"][feature.id.0.as_str()]["suppressed"] =
        serde_json::json!(false);
    let error = serde_json::from_value::<CadIr>(invalid).unwrap_err();
    assert!(error
        .to_string()
        .contains("configuration suppression disagrees with feature state"));
}

#[test]
fn datum_plane_reference_preserves_legacy_feature_ids_and_face_selections() {
    let feature =
        crate::features::DatumPlaneReference::Feature(crate::features::FeatureId("feature".into()));
    assert_eq!(
        serde_json::to_value(&feature).unwrap(),
        serde_json::json!("feature")
    );
    assert_eq!(
        serde_json::from_value::<crate::features::DatumPlaneReference>(serde_json::json!(
            "feature"
        ))
        .unwrap(),
        feature
    );

    let face = crate::features::DatumPlaneReference::Face {
        face: crate::features::FaceSelection::Faces(vec![crate::ids::FaceId("face".into())]),
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    assert_eq!(
        serde_json::from_value::<crate::features::DatumPlaneReference>(
            serde_json::to_value(&face).unwrap()
        )
        .unwrap(),
        face
    );
}

#[test]
fn feature_extents_round_trip_through_json() {
    use crate::features::{
        Angle, ExtrudeExtent, ExtrudeSide, FaceSelection, Length, RevolveExtent, Termination,
    };
    use crate::ids::FaceId;

    let extents = vec![
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(12.5),
                },
                draft: Some(Angle(0.1)),
                offset: None,
            },
        },
        ExtrudeExtent::Symmetric {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(25.0),
                },
                draft: None,
                offset: None,
            },
        },
        ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(10.0),
                },
                draft: Some(Angle(0.2)),
                offset: Some(Length(1.0)),
            },
            second: ExtrudeSide {
                termination: Termination::ToFace {
                    face: FaceSelection::Faces(vec![FaceId("synthetic:test:face#0".into())]),
                    offset: None,
                },
                draft: None,
                offset: Some(Length(-2.0)),
            },
        },
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::ThroughAll,
                draft: None,
                offset: None,
            },
        },
    ];
    let json = serde_json::to_string(&extents).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<ExtrudeExtent>>(&json).unwrap(),
        extents
    );

    let revolve_extents = vec![
        RevolveExtent::OneSided {
            termination: Termination::Angle {
                angle: Angle(std::f64::consts::PI),
            },
        },
        RevolveExtent::Symmetric {
            termination: Termination::Angle {
                angle: Angle(std::f64::consts::FRAC_PI_2),
            },
        },
        RevolveExtent::TwoSided {
            first: Termination::Angle { angle: Angle(0.25) },
            second: Termination::Angle { angle: Angle(0.75) },
        },
    ];
    let json = serde_json::to_string(&revolve_extents).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<RevolveExtent>>(&json).unwrap(),
        revolve_extents
    );
}

#[test]
fn loft_sections_accept_legacy_profiles_and_preserve_profile_shape() {
    use crate::features::{BooleanOp, FeatureDefinition, LoftSection, ProfileRef};

    let legacy = serde_json::json!({
        "definition": "loft",
        "profiles": [{"kind": "native", "value": "native:section"}],
        "op": "new_body",
        "closed": false
    });
    let definition: FeatureDefinition = serde_json::from_value(legacy).unwrap();
    assert!(matches!(
        &definition,
        FeatureDefinition::Loft {
            sections,
            guides,
            centerline: None,
            op: BooleanOp::NewBody,
            closed: false,
            ..
        } if sections == &vec![LoftSection::Profile(ProfileRef::Native("native:section".into()))]
            && guides.is_empty()
    ));
    let encoded = serde_json::to_value(definition).unwrap();
    assert_eq!(
        encoded["sections"][0],
        serde_json::json!({"kind": "native", "value": "native:section"})
    );
}

#[test]
fn generated_sweep_sections_round_trip_and_validate() {
    use crate::features::{
        BooleanOp, Feature, FeatureDefinition, FeatureId, GeneratedSweepSection, Length, SweepMode,
        SweepSection,
    };

    let definition = FeatureDefinition::Sweep {
        section: SweepSection::Generated(GeneratedSweepSection::CircularRegion {
            outer_radius: Length(3.0),
            wall_thickness: Some(Length(1.0)),
        }),
        sections: Vec::new(),
        path: None,
        mode: SweepMode::Solid {
            op: BooleanOp::NewBody,
        },
        orientation: None,
        transition: None,
        transformation: None,
        path_tangent: false,
        linearize: false,
        twist: None,
        path_extent: None,
        guide_rail: None,
        taper: None,
        scale: None,
        allow_multi_profile_faces: None,
    };
    let json = serde_json::to_string(&definition).unwrap();
    assert!(json.contains("\"kind\":\"generated\""));
    assert!(json.contains("\"shape\":\"circular_region\""));
    assert_eq!(
        serde_json::from_str::<FeatureDefinition>(&json).unwrap(),
        definition
    );

    let validate_definition = |definition| {
        let mut ir = unit_cube();
        ir.model.features.push(Feature {
            id: FeatureId("synthetic:test:feature#generated-sweep".into()),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
        ir.finalize();
        validate_neutral(&ir, Vec::new())
    };
    assert!(validate_definition(definition.clone()).is_ok());

    let mut invalid_wall = definition.clone();
    let FeatureDefinition::Sweep { section, .. } = &mut invalid_wall else {
        unreachable!();
    };
    let SweepSection::Generated(GeneratedSweepSection::CircularRegion {
        outer_radius,
        wall_thickness,
    }) = section
    else {
        unreachable!();
    };
    *wall_thickness = Some(*outer_radius);
    assert!(validate_definition(invalid_wall)
        .findings
        .iter()
        .any(|finding| { finding.message == "sweep magnitude is invalid" }));

    let mut invalid_mode = definition;
    let FeatureDefinition::Sweep { mode, .. } = &mut invalid_mode else {
        unreachable!();
    };
    *mode = SweepMode::Surface;
    assert!(validate_definition(invalid_mode)
        .findings
        .iter()
        .any(|finding| { finding.message == "sweep magnitude is invalid" }));
}

#[test]
fn full_round_fillet_keeps_automatic_side_semantics() {
    use crate::features::{
        FaceSelection, Feature, FeatureDefinition, FeatureId, FullRoundFilletGroup,
        FullRoundSideSelection,
    };

    let mut ir = unit_cube();
    let center = ir.model.faces[0].id.clone();
    let feature_index = ir.model.features.len();
    let definition = FeatureDefinition::FullRoundFillet {
        groups: vec![FullRoundFilletGroup {
            center_faces: FaceSelection::Faces(vec![center.clone()]),
            side_one_faces: FullRoundSideSelection::Automatic,
            side_two_faces: FullRoundSideSelection::Automatic,
        }],
    };
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(serde_json::to_value(&definition).unwrap())
            .unwrap(),
        definition
    );
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#full-round".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: Some("Fillet".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: None,
    });
    assert!(!validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.entity.as_deref() == Some("synthetic:test:feature#full-round")
                && finding.message == "full-round fillet face sets are invalid"
        }));

    if let FeatureDefinition::FullRoundFillet { groups } =
        &mut ir.model.features[feature_index].definition
    {
        groups[0].side_one_faces =
            FullRoundSideSelection::Explicit(FaceSelection::Faces(vec![center]));
    } else {
        unreachable!("test feature is a full-round fillet");
    }
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.entity.as_deref() == Some("synthetic:test:feature#full-round")
                && finding.message == "full-round fillet face sets are invalid"
        }));
}

#[test]
fn flex_modes_round_trip_and_validate() {
    use crate::features::{Angle, Feature, FeatureDefinition, FeatureId, FlexMode, Length};

    let modes = vec![
        FlexMode::Bending { angle: Angle(0.5) },
        FlexMode::Twisting { angle: Angle(1.0) },
        FlexMode::Tapering { factor: 1.5 },
        FlexMode::Stretching {
            distance: Length(12.0),
        },
    ];
    let json = serde_json::to_string(&modes).unwrap();
    assert_eq!(serde_json::from_str::<Vec<FlexMode>>(&json).unwrap(), modes);

    let mut ir = unit_cube();
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#flex".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Flex {
            axis: Some(Vector3::new(0.0, 0.0, 0.0)),
            mode: FlexMode::Tapering { factor: 0.0 },
        },
        native_ref: None,
    });
    let findings = validate_neutral(&ir, Vec::new()).findings;
    assert!(findings
        .iter()
        .any(|finding| finding.message == "flex axis is degenerate"));
    assert!(findings
        .iter()
        .any(|finding| finding.message == "flex magnitude is invalid"));
}

#[test]
fn unresolved_hole_and_flex_wire_forms_preserve_the_legacy_layout() {
    use crate::features::{FlexMode, HoleKind};

    let counterbore = serde_json::json!({
        "kind": "unresolved",
        "form": "counterbore",
        "counterbore_diameter": 10.0
    });
    let kind: HoleKind = serde_json::from_value(counterbore.clone()).unwrap();
    assert_eq!(
        kind,
        HoleKind::PartialCounterbore {
            diameter: Some(crate::features::Length(10.0)),
            depth: None,
        }
    );
    assert_eq!(serde_json::to_value(kind).unwrap(), counterbore);

    let flex = serde_json::json!({"kind": "unresolved", "form": "twisting"});
    let mode: FlexMode = serde_json::from_value(flex.clone()).unwrap();
    assert_eq!(
        mode,
        FlexMode::Unresolved(Some(crate::features::FlexForm::Twisting))
    );
    assert_eq!(serde_json::to_value(mode).unwrap(), flex);
}

#[test]
fn unresolved_hole_and_flex_wire_forms_reject_cross_family_payloads() {
    use crate::features::{FlexMode, HoleKind};

    assert!(serde_json::from_value::<HoleKind>(serde_json::json!({
        "kind": "unresolved",
        "form": "counterbore",
        "countersink_angle": 0.5
    }))
    .is_err());
    assert!(serde_json::from_value::<FlexMode>(serde_json::json!({
        "kind": "unresolved",
        "form": "twisting",
        "factor": 2.0
    }))
    .is_err());
}

#[test]
fn scale_factor_forms_preserve_the_legacy_wire_layout() {
    use crate::features::ScaleFactors;

    for wire in [
        serde_json::json!({}),
        serde_json::json!({"uniform": 2.0}),
        serde_json::json!({"x": 1.0, "y": 2.0, "z": 3.0}),
    ] {
        let factors: ScaleFactors = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(factors).unwrap(), wire);
    }
}

#[test]
fn scale_factor_wire_rejects_mixed_and_partial_forms() {
    use crate::features::ScaleFactors;

    for wire in [
        serde_json::json!({"uniform": 2.0, "x": 1.0}),
        serde_json::json!({"x": 1.0, "z": 3.0}),
    ] {
        assert!(serde_json::from_value::<ScaleFactors>(wire).is_err());
    }
}

#[test]
fn edge_selections_round_trip_through_json() {
    use crate::features::EdgeSelection;
    use crate::ids::{EdgeId, FeatureInputTopologyId, HistoricalEdgeId};

    let selections = vec![
        EdgeSelection::Unresolved,
        EdgeSelection::Edges(vec![EdgeId("synthetic:test:edge#0".into())]),
        EdgeSelection::Resolved {
            edges: vec![EdgeId("synthetic:test:edge#0".into())],
            native: "edge:10".into(),
        },
        EdgeSelection::Historical {
            state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
            edges: vec![HistoricalEdgeId("synthetic:history-input:edge#0".into())],
            native: "edge:9".into(),
        },
        EdgeSelection::HistoricalPartial {
            state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
            edges: vec![HistoricalEdgeId("synthetic:history-input:edge#0".into())],
            unresolved: vec!["native:edge-operand#1".into()],
            native: "edge:9".into(),
        },
        EdgeSelection::Native("sldprt:history:feature#10:0".into()),
    ];
    let json = serde_json::to_string(&selections).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<EdgeSelection>>(&json).unwrap(),
        selections
    );
}

#[test]
fn historical_edge_paths_round_trip_through_json() {
    use crate::features::PathRef;
    use crate::ids::{FeatureInputTopologyId, HistoricalEdgeId};

    let path = PathRef::HistoricalEdges {
        state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
        edges: vec![
            HistoricalEdgeId("synthetic:history-input:edge#0".into()),
            HistoricalEdgeId("synthetic:history-input:edge#1".into()),
        ],
        native: "native:path#0".into(),
    };
    let json = serde_json::to_string(&path).unwrap();
    assert_eq!(serde_json::from_str::<PathRef>(&json).unwrap(), path);
}

#[test]
fn face_selections_round_trip_through_json() {
    use crate::features::FaceSelection;
    use crate::ids::{FaceId, FeatureInputTopologyId, HistoricalFaceId};

    let selections = vec![
        FaceSelection::Unresolved,
        FaceSelection::Faces(vec![FaceId("synthetic:test:face#0".into())]),
        FaceSelection::Resolved {
            faces: vec![FaceId("synthetic:test:face#0".into())],
            native: "face:14".into(),
        },
        FaceSelection::Historical {
            state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
            faces: vec![HistoricalFaceId("synthetic:history-input:face#0".into())],
            native: "face:13".into(),
        },
        FaceSelection::HistoricalPartial {
            state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
            faces: vec![HistoricalFaceId("synthetic:history-input:face#0".into())],
            unresolved: vec!["native:face-operand#1".into()],
            native: "face:12".into(),
        },
        FaceSelection::Native("sldprt:history:feature#14:0".into()),
    ];
    let json = serde_json::to_string(&selections).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<FaceSelection>>(&json).unwrap(),
        selections
    );
}

#[test]
fn historical_face_profiles_round_trip_through_json() {
    use crate::features::ProfileRef;
    use crate::ids::{FeatureInputTopologyId, HistoricalFaceId};

    let profile = ProfileRef::HistoricalFaces {
        state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
        faces: vec![HistoricalFaceId("synthetic:history-input:face#0".into())],
        native: vec!["native:profile-group#0".into()],
    };
    let json = serde_json::to_string(&profile).unwrap();
    assert_eq!(serde_json::from_str::<ProfileRef>(&json).unwrap(), profile);
}

#[test]
fn body_selections_round_trip_through_json() {
    use crate::features::BodySelection;
    use crate::ids::{BodyId, FeatureInputTopologyId, HistoricalBodyId};

    let selections = vec![
        BodySelection::Unresolved,
        BodySelection::Bodies(vec![BodyId("synthetic:test:body#0".into())]),
        BodySelection::Resolved {
            bodies: vec![BodyId("synthetic:test:body#0".into())],
            native: "body:17".into(),
        },
        BodySelection::ResolvedSet {
            bodies: vec![
                BodyId("synthetic:test:body#0".into()),
                BodyId("synthetic:test:body#1".into()),
            ],
            native: vec!["body:17".into(), "body:18".into()],
        },
        BodySelection::Historical {
            state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
            bodies: vec![HistoricalBodyId("synthetic:history-input:body#0".into())],
            native: "body:16".into(),
        },
        BodySelection::HistoricalSet {
            state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
            bodies: vec![
                HistoricalBodyId("synthetic:history-input:body#0".into()),
                HistoricalBodyId("synthetic:history-input:body#1".into()),
            ],
            native: vec!["body:16".into(), "body:17".into()],
        },
        BodySelection::HistoricalUnorderedSet {
            state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
            bodies: vec![
                HistoricalBodyId("synthetic:history-input:body#0".into()),
                HistoricalBodyId("synthetic:history-input:body#1".into()),
            ],
            native: vec!["body:16".into(), "body:17".into()],
        },
        BodySelection::Native("body:17,body:18".into()),
        BodySelection::NativeSet(vec!["body:17".into(), "body:18".into()]),
    ];
    let json = serde_json::to_string(&selections).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<BodySelection>>(&json).unwrap(),
        selections
    );
}

#[test]
fn feature_result_topology_round_trips_without_current_model_bodies() {
    use crate::features::{FeatureId, FeatureResultTopology};
    use crate::ids::FeatureResultTopologyId;

    let state = FeatureResultTopology {
        id: FeatureResultTopologyId("synthetic:history-result:state#0".into()),
        output_of: FeatureId("synthetic:feature#0".into()),
        bodies: vec!["body:17".into()],
        faces: vec!["face:3".into()],
        edges: vec!["edge:5".into()],
        vertices: vec!["vertex:8".into()],
        native_ref: Some("native:result#0".into()),
    };
    let json = serde_json::to_string(&state).unwrap();
    assert_eq!(
        serde_json::from_str::<FeatureResultTopology>(&json).unwrap(),
        state
    );
}

#[test]
fn combine_omits_the_default_keep_tools_flag_from_json() {
    use crate::features::{BodySelection, BooleanOp, FeatureDefinition};

    let definition = FeatureDefinition::Combine {
        target: BodySelection::Native("body:17".into()),
        tools: BodySelection::Native("body:18".into()),
        op: BooleanOp::Join,
        keep_tools: false,
    };
    let json = serde_json::to_value(definition).unwrap();
    assert_eq!(json.get("keep_tools"), None);
}

#[test]
fn draft_anchor_round_trips_through_the_flat_wire_shape() {
    use crate::features::{DraftAnchor, FeatureDefinition};

    let wire = serde_json::json!({
        "definition": "draft",
        "faces": {"kind": "native", "value": "draft:faces"},
        "neutral_plane": {"kind": "unresolved"},
        "parting_tool": {"kind": "native", "value": "draft:parting-tool"},
        "pull_direction": {"x": 0.0, "y": 0.0, "z": 1.0},
        "pull_plane": "draft:pull-plane",
        "angle": 0.1,
        "outward": false
    });
    let definition: FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(
        &definition,
        FeatureDefinition::Draft {
            anchor: DraftAnchor::PartingLine { .. },
            ..
        }
    ));
    assert_eq!(serde_json::to_value(definition).unwrap(), wire);
}

#[test]
fn draft_anchor_rejects_split_or_conflicting_wire_fields() {
    use crate::features::FeatureDefinition;

    let base = serde_json::json!({
        "definition": "draft",
        "faces": {"kind": "unresolved"},
        "neutral_plane": {"kind": "unresolved"},
        "pull_direction": null,
        "angle": null,
        "outward": null
    });
    for invalid in [
        {
            let mut value = base.clone();
            value["pull_plane"] = serde_json::json!("draft:pull-plane");
            value
        },
        {
            let mut value = base.clone();
            value["parting_tool"] =
                serde_json::json!({"kind": "native", "value": "draft:parting-tool"});
            value
        },
        {
            let mut value = base.clone();
            value["neutral_plane"] =
                serde_json::json!({"kind": "native", "value": "draft:neutral-plane"});
            value["parting_tool"] =
                serde_json::json!({"kind": "native", "value": "draft:parting-tool"});
            value["pull_direction"] = serde_json::json!({"x": 0.0, "y": 0.0, "z": 1.0});
            value
        },
    ] {
        assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
    }
}

#[test]
fn wrap_mode_round_trips_through_the_flat_wire_shape() {
    use crate::features::{FeatureDefinition, Length, WrapMode};

    let wire = serde_json::json!({
        "definition": "wrap",
        "profile": {"kind": "native", "value": "wrap:profile"},
        "face": {"kind": "native", "value": "wrap:face"},
        "mode": "emboss",
        "depth": 2.5
    });
    let definition: FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(
        &definition,
        FeatureDefinition::Wrap {
            mode: WrapMode::Emboss { depth: Length(2.5) },
            ..
        }
    ));
    assert_eq!(serde_json::to_value(definition).unwrap(), wire);

    let scribe = FeatureDefinition::Wrap {
        profile: crate::features::ProfileRef::Native("wrap:profile".into()),
        face: crate::features::FaceSelection::Native("wrap:face".into()),
        mode: WrapMode::Scribe,
    };
    let encoded = serde_json::to_value(scribe).unwrap();
    assert_eq!(encoded.get("mode"), Some(&serde_json::json!("scribe")));
    assert_eq!(encoded.get("depth"), None);
}

#[test]
fn wrap_mode_rejects_a_missing_or_forbidden_depth() {
    use crate::features::FeatureDefinition;

    for invalid in [
        serde_json::json!({
            "definition": "wrap",
            "profile": {"kind": "native", "value": "wrap:profile"},
            "face": {"kind": "native", "value": "wrap:face"},
            "mode": "emboss"
        }),
        serde_json::json!({
            "definition": "wrap",
            "profile": {"kind": "native", "value": "wrap:profile"},
            "face": {"kind": "native", "value": "wrap:face"},
            "mode": "scribe",
            "depth": 1.0
        }),
    ] {
        assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
    }
}

#[test]
fn helix_shape_round_trips_through_the_flat_wire_shape() {
    use crate::features::{FeatureDefinition, HelixShape, Length};

    let conical_wire = serde_json::json!({
        "definition": "helix",
        "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "axis_direction": {"x": 0.0, "y": 0.0, "z": 1.0},
        "radius": 2.0,
        "pitch": 3.0,
        "revolutions": 4.0,
        "start_angle": 0.0,
        "clockwise": false,
        "cone_angle": 0.2
    });
    let conical: FeatureDefinition = serde_json::from_value(conical_wire.clone()).unwrap();
    assert!(matches!(
        &conical,
        FeatureDefinition::Helix {
            shape: HelixShape::Conical { pitch, .. },
            ..
        } if pitch.get() == Length(3.0)
    ));
    assert_eq!(serde_json::to_value(conical).unwrap(), conical_wire);

    let spiral_wire = serde_json::json!({
        "definition": "helix",
        "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "axis_direction": {"x": 0.0, "y": 0.0, "z": 1.0},
        "radius": 2.0,
        "pitch": 0.0,
        "revolutions": 4.0,
        "start_angle": 0.0,
        "clockwise": false,
        "radial_growth": 1.5
    });
    let spiral: FeatureDefinition = serde_json::from_value(spiral_wire.clone()).unwrap();
    assert!(matches!(
        &spiral,
        FeatureDefinition::Helix {
            shape: HelixShape::Spiral {
                radial_growth: Length(1.5)
            },
            ..
        }
    ));
    assert_eq!(serde_json::to_value(spiral).unwrap(), spiral_wire);
}

#[test]
fn helix_shape_rejects_sentinel_and_conflicting_wire_fields() {
    use crate::features::FeatureDefinition;

    let base = serde_json::json!({
        "definition": "helix",
        "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "axis_direction": {"x": 0.0, "y": 0.0, "z": 1.0},
        "radius": 2.0,
        "pitch": 0.0,
        "revolutions": 4.0,
        "start_angle": 0.0,
        "clockwise": false
    });
    for invalid in [
        base.clone(),
        {
            let mut value = base.clone();
            value["pitch"] = serde_json::json!(3.0);
            value["radial_growth"] = serde_json::json!(1.5);
            value
        },
        {
            let mut value = base.clone();
            value["radial_growth"] = serde_json::json!(1.5);
            value["cone_angle"] = serde_json::json!(0.2);
            value
        },
        {
            let mut value = base.clone();
            value["cone_angle"] = serde_json::json!(0.2);
            value
        },
    ] {
        assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
    }
}

#[test]
fn trim_cell_selection_requires_unique_in_range_ordinals() {
    let valid = TrimCellSelection::new(vec![1, 4], 5).unwrap();
    assert_eq!(valid.removed(), &[1, 4]);
    assert_eq!(valid.total(), 5);
    assert!(TrimCellSelection::new(vec![1, 1], 5).is_none());
    assert!(TrimCellSelection::new(vec![6], 5).is_none());
}

#[test]
fn trim_cells_preserve_the_flat_wire_fields_and_reject_invalid_input() {
    use crate::features::{FaceSelection, FeatureDefinition, PathRef, TrimRegion};

    let definition = FeatureDefinition::TrimSurface {
        faces: FaceSelection::Unresolved,
        tool: PathRef::Unresolved("test:trim-tool".into()),
        keep: TrimRegion::Cells(TrimCellSelection::new(vec![1, 4], 5).unwrap()),
    };
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["definition"], "trim_surface");
    assert_eq!(wire["keep"], "unresolved");
    assert_eq!(wire["cell_selection"]["removed"], serde_json::json!([1, 4]));
    assert_eq!(wire["cell_selection"]["total"], 5);
    let decoded: FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(
        decoded,
        FeatureDefinition::TrimSurface {
            keep: TrimRegion::Cells(ref selection),
            ..
        } if selection.removed() == [1, 4] && selection.total() == 5
    ));

    let mut conflicting = wire.clone();
    conflicting["keep"] = serde_json::json!("inside");
    assert!(serde_json::from_value::<FeatureDefinition>(conflicting).is_err());

    let mut invalid = wire;
    invalid["cell_selection"]["removed"] = serde_json::json!([6]);
    assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
}
