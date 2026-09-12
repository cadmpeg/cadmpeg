// SPDX-License-Identifier: Apache-2.0
use crate::examples::unit_cube;
use crate::features::{ConfigurationEvaluation, DistinctMembers, FeatureContent};
use crate::math::Point3;
use crate::validate::validate_neutral;
use crate::CadIr;

#[test]
fn configuration_body_membership_round_trips_and_validates() {
    use crate::ids::BodyId;
    use crate::{
        features::{
            ConfigurationEvaluation, ConfigurationFeatureState, ConfigurationId,
            DesignConfiguration, DesignParameter, Feature, FeatureDefinition, FeatureId,
            FeatureOperation, ParameterId, ParameterValue,
        },
        scalar::{Angle, Length},
    };
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let configuration_id =
        ConfigurationId::mint("synthetic:test:configuration#0").expect("identity grammar");
    let parameter_id =
        ParameterId::mint("synthetic:test:parameter#width").expect("identity grammar");
    let body = ir.model.bodies[0].id.clone();
    ir.model.parameters.push(DesignParameter {
        id: parameter_id.clone(),
        owner: None,
        ordinal: 0,
        name: "width".into(),
        expression: "10 mm".into(),
        display: None,
        value: None,
        dependencies: DistinctMembers::default(),
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
        bodies: crate::features::ConfigurationBodies::Resolved(
            (vec![body.clone()]).try_into().unwrap(),
        ),
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
        ParameterId::mint("synthetic:test:parameter#missing").expect("identity grammar"),
        "30 mm".into(),
    )]);
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message.contains("configuration parameter override")
    }));
    ir.model.configurations[0].parameter_overrides.clear();

    ir.model.configurations[0].parameter_values = BTreeMap::from([(
        ParameterId::mint("synthetic:test:parameter#missing-value").expect("identity grammar"),
        ParameterValue::Real(crate::scalar::FiniteReal::new(1.0).unwrap()),
    )]);
    ir.model.configurations[0].feature_states = BTreeMap::from([(
        FeatureId::mint("synthetic:test:feature#missing-state").expect("identity grammar"),
        ConfigurationFeatureState {
            evaluation: ConfigurationEvaluation::Active {
                outputs: (vec![
                    BodyId::mint("synthetic:test:body#missing-output").expect("valid identity")
                ])
                .try_into()
                .unwrap(),
            },
            dependencies: (vec![FeatureId::mint("synthetic:test:feature#missing-dependency")
                .expect("identity grammar")])
            .try_into()
            .unwrap(),
            definition: FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                construction: None,
            }),
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

    ir.model.parameters[0].value = Some(ParameterValue::Length(Length::new(10.0).unwrap()));
    ir.model.configurations[0].parameter_values = BTreeMap::from([(
        parameter_id.clone(),
        ParameterValue::Angle(Angle::new(1.0).unwrap()),
    )]);
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message == "configuration parameter value is invalid"
    }));
    ir.model.configurations[0].parameter_values.clear();

    ir.model.parameters[0].value = None;

    let first_feature =
        FeatureId::mint("synthetic:test:feature#configuration-first").expect("identity grammar");
    let later_feature =
        FeatureId::mint("synthetic:test:feature#configuration-later").expect("identity grammar");
    for (ordinal, feature) in [first_feature.clone(), later_feature.clone()]
        .into_iter()
        .enumerate()
    {
        ir.model.features.push(Feature {
            id: feature,
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),

            evaluation: crate::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                    position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                        .unwrap(),
                    construction: None,
                }),
            ),
            native_ref: None,
        });
    }
    ir.model.configurations[0].feature_states = BTreeMap::from([(
        first_feature.clone(),
        ConfigurationFeatureState {
            evaluation: ConfigurationEvaluation::Active {
                outputs: (vec![body.clone()]).try_into().unwrap(),
            },
            dependencies: (vec![later_feature.clone()]).try_into().unwrap(),
            definition: FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                construction: None,
            }),
        },
    )]);
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message.contains("does not precede")
    }));
    ir.model.configurations[0].feature_states.clear();

    ir.model.configurations[0].feature_states = BTreeMap::from([(
        first_feature.clone(),
        ConfigurationFeatureState {
            evaluation: ConfigurationEvaluation::Suppressed {},
            dependencies: DistinctMembers::default(),
            definition: FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                construction: None,
            }),
        },
    )]);
    assert!(validate_neutral(&ir, Vec::new()).is_ok());
    ir.model.configurations[0].feature_states.clear();

    ir.model.configurations[0].active = true;
    ir.model.features[0].suppressed = Some(true);
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message
                == "active configuration suppression disagrees with current feature state"
    }));
    ir.model.configurations[0].active = false;
    ir.model.features[0].suppressed = Some(false);

    ir.model.configurations[0].feature_states = BTreeMap::from([(
        later_feature.clone(),
        ConfigurationFeatureState {
            evaluation: ConfigurationEvaluation::Active {
                outputs: (vec![body.clone()]).try_into().unwrap(),
            },
            dependencies: (vec![first_feature.clone()]).try_into().unwrap(),
            definition: FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                construction: None,
            }),
        },
    )]);
    // A dependency with no state in this configuration inherits its model-level
    // state; `feature_states` is allowed to be sparse, so that is not a finding.
    assert!(validate_neutral(&ir, Vec::new()).is_ok());
    ir.model.configurations[0].feature_states.insert(
        first_feature.clone(),
        ConfigurationFeatureState {
            evaluation: ConfigurationEvaluation::Suppressed {},
            dependencies: DistinctMembers::default(),
            definition: FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                construction: None,
            }),
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
        .evaluation = ConfigurationEvaluation::Active {
        outputs: DistinctMembers::default(),
    };
    assert!(validate_neutral(&ir, Vec::new()).is_ok());
    ir.model.configurations[0].feature_states.clear();

    ir.model.configurations[0].bodies = crate::features::ConfigurationBodies::Resolved(
        (vec![BodyId::mint("synthetic:test:body#missing").expect("valid identity")])
            .try_into()
            .unwrap(),
    );
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message.contains("missing configuration body")
    }));

    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:configuration#1").expect("identity grammar"),
        ordinal: 0,
        active: false,
        source_index: Some(7),
        name: "Alternate".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies: crate::features::ConfigurationBodies::Resolved(DistinctMembers::default()),
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    ir.model.configurations[0].active = true;
    ir.model.configurations[1].active = true;
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
fn configuration_suppression_is_read_from_feature_states_and_refuses_the_deleted_key() {
    use crate::features::{
        ConfigurationBodies, ConfigurationFeatureState, ConfigurationId, DesignConfiguration,
        Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let feature = Feature::new(
        FeatureId::mint("synthetic:test:feature#suppressed").expect("identity grammar"),
        0,
        FeatureDefinition::Operation(FeatureOperation::DatumPoint {
            position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            construction: None,
        }),
    );
    ir.model.features.push(feature.clone());
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:configuration#suppressed")
            .expect("identity grammar"),
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
                evaluation: ConfigurationEvaluation::Suppressed {},
                dependencies: feature.dependencies.clone(),
                definition: feature.evaluation.definition().clone(),
            },
        )]),
        native_ref: None,
    });

    let wire = serde_json::to_value(&ir).unwrap();
    let configuration = &wire["model"]["configurations"][0];
    assert!(configuration.get("suppressed_features").is_none());
    let round_trip = serde_json::from_value::<CadIr>(wire.clone()).unwrap();
    assert_eq!(
        round_trip.model.configurations[0]
            .suppressed_features()
            .collect::<Vec<_>>(),
        vec![&feature.id]
    );

    let mut restated = wire;
    restated["model"]["configurations"][0]
        .as_object_mut()
        .unwrap()
        .insert(
            "suppressed_features".into(),
            serde_json::json!([feature.id.0.clone()]),
        );
    let error = serde_json::from_value::<CadIr>(restated).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unknown field `suppressed_features`"),
        "{error}"
    );
}

#[test]
fn configuration_evaluation_wire_is_flat_and_strict() {
    use crate::features::ConfigurationEvaluation;
    use crate::ids::BodyId;

    let body = BodyId::mint("synthetic:test:body#evaluation").expect("identity grammar");
    let active = ConfigurationEvaluation::Active {
        outputs: (vec![body]).try_into().unwrap(),
    };
    let wire = serde_json::to_value(&active).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"kind": "active", "outputs": ["synthetic:test:body#evaluation"]})
    );
    assert_eq!(
        serde_json::from_value::<ConfigurationEvaluation>(wire).unwrap(),
        active
    );
    assert!(
        serde_json::from_value::<ConfigurationEvaluation>(serde_json::json!({
            "kind": "suppressed",
            "outputs": []
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<ConfigurationEvaluation>(serde_json::json!({
            "kind": "active",
            "outputs": [],
            "suppressed": true
        }))
        .is_err()
    );
}

#[test]
fn active_configuration_evaluation_can_have_no_body_outputs() {
    use crate::features::ConfigurationEvaluation;

    let active = ConfigurationEvaluation::Active {
        outputs: DistinctMembers::default(),
    };
    assert_eq!(
        serde_json::to_value(&active).unwrap(),
        serde_json::json!({"kind": "active"})
    );
    assert_eq!(
        serde_json::from_value::<ConfigurationEvaluation>(serde_json::json!({"kind": "active"}))
            .unwrap(),
        active,
    );
}
