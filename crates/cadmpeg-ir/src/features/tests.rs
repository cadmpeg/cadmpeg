// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::features::{ConfigurationEvaluation, TrimCellSelection};
use crate::math::{Point3, Vector3};
use crate::validate::validate_neutral;
use crate::CadIr;

#[test]
fn native_feature_kind_preserves_the_source_spelling() {
    use crate::features::NativeFeatureKind;

    for (wire, expected) in [
        ("\"Canvas\"", NativeFeatureKind::Canvas),
        (
            "\"PartDesign::FeatureBase\"",
            NativeFeatureKind::Other("PartDesign::FeatureBase".into()),
        ),
    ] {
        let kind: NativeFeatureKind = serde_json::from_str(wire).unwrap();
        assert_eq!(kind, expected);
        assert_eq!(serde_json::to_string(&kind).unwrap(), wire);
    }
}

#[test]
fn configuration_body_membership_round_trips_and_validates() {
    use crate::features::{
        Angle, ConfigurationEvaluation, ConfigurationFeatureState, ConfigurationId,
        DesignConfiguration, DesignParameter, Feature, FeatureDefinition, FeatureId, Length,
        ParameterId, ParameterValue,
    };
    use crate::ids::BodyId;
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
        dependencies: Default::default(),
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
        ParameterValue::Real(crate::features::FiniteReal::new(1.0).unwrap()),
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
            definition: FeatureDefinition::DatumPoint {
                position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
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
            dependencies: Default::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),
            outputs: Vec::new(),
            definition: FeatureDefinition::DatumPoint {
                position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                construction: None,
            },
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
            definition: FeatureDefinition::DatumPoint {
                position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                construction: None,
            },
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
            evaluation: ConfigurationEvaluation::Suppressed,
            dependencies: Default::default(),
            definition: FeatureDefinition::DatumPoint {
                position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                construction: None,
            },
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
            definition: FeatureDefinition::DatumPoint {
                position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
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
            evaluation: ConfigurationEvaluation::Suppressed,
            dependencies: Default::default(),
            definition: FeatureDefinition::DatumPoint {
                position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
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
        .evaluation = ConfigurationEvaluation::Active {
        outputs: Default::default(),
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
        bodies: crate::features::ConfigurationBodies::Resolved(Default::default()),
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
fn configuration_suppression_is_derived_and_requires_agreeing_feature_states() {
    use crate::features::{
        ConfigurationBodies, ConfigurationFeatureState, ConfigurationId, DesignConfiguration,
        Feature, FeatureDefinition, FeatureId,
    };
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let feature = Feature::new(
        FeatureId::mint("synthetic:test:feature#suppressed").expect("identity grammar"),
        0,
        FeatureDefinition::DatumPoint {
            position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            construction: None,
        },
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
                evaluation: ConfigurationEvaluation::Suppressed,
                dependencies: feature.dependencies.clone(),
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
    let error = serde_json::from_value::<CadIr>(wire).unwrap_err();
    assert!(
        error.to_string().contains(&format!(
            "configuration suppressed feature `{}` has no configuration feature state",
            feature.id.0
        )),
        "{error}"
    );

    let mut invalid = serde_json::to_value(&ir).unwrap();
    invalid["model"]["configurations"][0]["feature_states"][feature.id.0.as_str()]["evaluation"]
        ["kind"] = serde_json::json!("active");
    let error = serde_json::from_value::<CadIr>(invalid).unwrap_err();
    assert!(error
        .to_string()
        .contains("configuration suppression disagrees with feature state"));
}

#[test]
fn datum_plane_reference_preserves_legacy_feature_ids_and_face_selections() {
    let feature = crate::features::DatumPlaneReference::Feature(
        crate::features::FeatureId::mint("test:model:feature#feature").expect("identity grammar"),
    );
    assert_eq!(
        serde_json::to_value(&feature).unwrap(),
        serde_json::json!("test:model:feature#feature")
    );
    assert_eq!(
        serde_json::from_value::<crate::features::DatumPlaneReference>(serde_json::json!(
            "test:model:feature#feature"
        ))
        .unwrap(),
        feature
    );

    let face =
        crate::features::DatumPlaneReference::Face(crate::features::FaceSelection::Faces(vec![
            crate::ids::FaceId::mint("test:model:face#face").expect("valid identity"),
        ]));
    assert_eq!(
        serde_json::to_value(&face).unwrap(),
        serde_json::json!({
            "face": {"kind": "faces", "value": ["test:model:face#face"]}
        })
    );
    assert_eq!(
        serde_json::from_value::<crate::features::DatumPlaneReference>(
            serde_json::to_value(&face).unwrap()
        )
        .unwrap(),
        face
    );
    let legacy_face_wire = serde_json::json!({
        "face": {"kind": "faces", "value": ["test:model:face#face"]},
        "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
        "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
    });
    assert_eq!(
        serde_json::from_value::<crate::features::DatumPlaneReference>(legacy_face_wire).unwrap(),
        face
    );

    let resolved = crate::features::DatumPlaneReference::ResolvedPlane {
        frame: crate::features::FeatureSupportPlaneFrame::new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    };
    let resolved_wire = serde_json::json!({
        "face": {"kind": "unresolved"},
        "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
        "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
    });
    assert_eq!(serde_json::to_value(&resolved).unwrap(), resolved_wire);
    assert_eq!(
        serde_json::from_value::<crate::features::DatumPlaneReference>(
            serde_json::to_value(&resolved).unwrap()
        )
        .unwrap(),
        resolved
    );

    let partial_legacy_wire = serde_json::json!({
        "face": {"kind": "faces", "value": ["test:model:face#face"]},
        "origin": {"x": 0.0, "y": 0.0, "z": 0.0}
    });
    assert!(
        serde_json::from_value::<crate::features::DatumPlaneReference>(partial_legacy_wire)
            .is_err()
    );
}

#[test]
fn feature_extents_round_trip_through_json() {
    use crate::features::{
        AngularTermination, ExtrudeExtent, ExtrudeSide, FaceSelection, Length, LinearTermination,
        RevolveExtent,
    };
    use crate::ids::FaceId;

    let extents = vec![
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: crate::features::NonZeroLength::new(12.5).unwrap(),
                },
                draft: Some(crate::features::SlopeAngle::new(0.1).unwrap()),
            },
        },
        ExtrudeExtent::Symmetric {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: crate::features::NonZeroLength::new(25.0).unwrap(),
                },
                draft: None,
            },
        },
        ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: crate::features::NonZeroLength::new(10.0).unwrap(),
                },
                draft: Some(crate::features::SlopeAngle::new(0.2).unwrap()),
            },
            second: ExtrudeSide {
                termination: LinearTermination::ToFace {
                    face: FaceSelection::Faces(vec![
                        FaceId::mint("synthetic:test:face#0").expect("valid identity")
                    ]),
                    offset: Some(Length::new(-2.0).unwrap()),
                },
                draft: None,
            },
        },
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::ThroughAll,
                draft: None,
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
            termination: AngularTermination::Angle {
                angle: crate::features::PositiveAngle::new(std::f64::consts::PI).unwrap(),
            },
        },
        RevolveExtent::Symmetric {
            termination: AngularTermination::Angle {
                angle: crate::features::PositiveAngle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            },
        },
        RevolveExtent::TwoSided {
            first: AngularTermination::Angle {
                angle: crate::features::PositiveAngle::new(0.25).unwrap(),
            },
            second: AngularTermination::Angle {
                angle: crate::features::PositiveAngle::new(0.75).unwrap(),
            },
        },
    ];
    let json = serde_json::to_string(&revolve_extents).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<RevolveExtent>>(&json).unwrap(),
        revolve_extents
    );
}

#[test]
fn termination_families_preserve_wire_and_reject_cross_family_variants() {
    use crate::features::{AngularTermination, LinearTermination};

    let blind_wire = serde_json::json!({"kind": "blind", "length": 12.5});
    let blind: LinearTermination = serde_json::from_value(blind_wire.clone()).unwrap();
    assert_eq!(
        blind,
        LinearTermination::Blind {
            length: crate::features::NonZeroLength::new(12.5).unwrap()
        }
    );
    assert_eq!(serde_json::to_value(blind).unwrap(), blind_wire);
    assert!(serde_json::from_value::<AngularTermination>(blind_wire).is_err());

    let angle_wire = serde_json::json!({"kind": "angle", "angle": 1.25});
    let angle: AngularTermination = serde_json::from_value(angle_wire.clone()).unwrap();
    assert_eq!(
        angle,
        AngularTermination::Angle {
            angle: crate::features::PositiveAngle::new(1.25).unwrap()
        }
    );
    assert_eq!(serde_json::to_value(angle).unwrap(), angle_wire);
    assert!(serde_json::from_value::<LinearTermination>(angle_wire).is_err());
}

#[test]
fn loft_sections_preserve_profile_shape() {
    use crate::features::{BooleanOp, FeatureDefinition, LoftSection, ProfileRef};

    let wire = serde_json::json!({
        "definition": "loft",
        "sections": [{"kind": "native", "value": "native:section"}],
        "guidance": {"kind": "guides", "path": []},
        "op": "new_body",
        "closed": false
    });
    let definition: FeatureDefinition = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        &definition,
        FeatureDefinition::Loft {
            sections,
            guidance: crate::features::LoftGuidance::Guides(guides),
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
        Feature, FeatureDefinition, FeatureId, GeneratedSweepSection, SweepMode, SweepSection,
    };

    let definition = FeatureDefinition::Sweep {
        section: SweepSection::Generated(GeneratedSweepSection::CircularRegion {
            outer_radius: crate::features::PositiveLength::new(3.0).unwrap(),
            wall_thickness: Some(crate::features::PositiveLength::new(1.0).unwrap()),
        }),
        sections: Vec::new(),
        path: None,
        mode: SweepMode::NewBody,
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
            id: FeatureId::mint("synthetic:test:feature#generated-sweep")
                .expect("identity grammar"),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            dependencies: Default::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
        ir.finalize();
        validate_neutral(&ir, Vec::new())
    };
    let report = validate_definition(definition.clone());
    assert!(report.is_ok(), "{report:#?}");

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
        id: FeatureId::mint("synthetic:test:feature#full-round").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: Some("Fillet".into()),
        source_text: None,
        source_content: Default::default(),
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
    use crate::features::{Angle, FlexMode, Length};

    let modes = vec![
        FlexMode::Bending {
            angle: Angle::new(0.5).unwrap(),
        },
        FlexMode::Twisting {
            angle: Angle::new(1.0).unwrap(),
        },
        FlexMode::Tapering {
            factor: crate::features::PositiveReal::new(1.5).unwrap(),
        },
        FlexMode::Stretching {
            distance: Length::new(12.0).unwrap(),
        },
    ];
    let json = serde_json::to_string(&modes).unwrap();
    assert_eq!(serde_json::from_str::<Vec<FlexMode>>(&json).unwrap(), modes);
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
            diameter: Some(crate::features::PositiveLength::new(10.0).unwrap()),
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
fn hole_construction_forms_preserve_the_flat_wire_layout() {
    use crate::features::{FeatureDefinition, HoleConstruction, HoleKind, HoleSpecification};

    let standard = serde_json::json!({
        "definition": "hole",
        "kind": {"kind": "simple"},
        "specification": {
            "standard": "ISO metric",
            "designation": "M8",
            "fit": "normal",
            "threaded": false,
            "modeled": false,
            "cosmetic": false,
            "hand": "right",
            "depth": {"kind": "hole_depth"}
        }
    });
    let definition: FeatureDefinition = serde_json::from_value(standard.clone()).unwrap();
    assert!(matches!(
        &definition,
        FeatureDefinition::Hole {
            construction: HoleConstruction::Form {
                kind: HoleKind::Simple,
                specification: Some(specification),
            },
            ..
        } if matches!(specification.as_ref(), HoleSpecification::Clearance { .. })
    ));
    assert_eq!(serde_json::to_value(definition).unwrap(), standard);

    let native_thread = serde_json::json!({
        "definition": "hole",
        "kind": {
            "kind": "threaded",
            "major_diameter": 8.0,
            "thread_depth": 12.0,
            "pitch": 1.25,
            "drill_point_angle": 2.0
        }
    });
    let definition: FeatureDefinition = serde_json::from_value(native_thread.clone()).unwrap();
    assert!(matches!(
        &definition,
        FeatureDefinition::Hole {
            construction: HoleConstruction::NativeThread { .. },
            ..
        }
    ));
    assert_eq!(serde_json::to_value(definition).unwrap(), native_thread);
}

#[test]
fn hole_wire_rejects_cross_form_thread_fields() {
    use crate::features::{FeatureDefinition, HoleSpecification};

    let specification = |threaded| {
        serde_json::json!({
            "standard": "ISO metric",
            "threaded": threaded,
            "modeled": false,
            "cosmetic": false,
            "hand": "right",
            "depth": {"kind": "hole_depth"}
        })
    };

    let mut clearance_with_class = specification(false);
    clearance_with_class["class"] = serde_json::json!("6H");
    assert!(serde_json::from_value::<HoleSpecification>(clearance_with_class).is_err());

    let mut thread_with_fit = specification(true);
    thread_with_fit["fit"] = serde_json::json!("normal");
    assert!(serde_json::from_value::<HoleSpecification>(thread_with_fit).is_err());

    let mut native_with_standard = serde_json::json!({
        "definition": "hole",
        "kind": {
            "kind": "threaded",
            "major_diameter": 8.0,
            "thread_depth": 12.0,
            "drill_point_angle": 2.0
        }
    });
    native_with_standard["specification"] = specification(true);
    assert!(serde_json::from_value::<FeatureDefinition>(native_with_standard).is_err());
}

#[test]
fn filled_surface_continuity_preserves_aggregate_and_component_wire_fields() {
    use crate::features::{
        EdgeSelection, FaceSelection, FeatureDefinition, SurfaceBoundary, SurfaceContinuity,
    };

    let definition = FeatureDefinition::FilledSurface {
        boundary: SurfaceBoundary::Edges(EdgeSelection::Unresolved),
        support_faces: FaceSelection::Faces(Vec::new()),
        continuity: crate::features::FilledSurfaceContinuityState::per_boundary(vec![
            SurfaceContinuity::Contact,
            SurfaceContinuity::Contact,
        ]),
        merge_result: Some(false),
    };
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["continuity"], serde_json::json!("contact"));
    assert_eq!(
        wire["boundary_continuities"],
        serde_json::json!(["contact", "contact"])
    );
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
        definition
    );

    let mut conflicting = wire;
    conflicting["continuity"] = serde_json::json!("curvature");
    assert!(serde_json::from_value::<FeatureDefinition>(conflicting).is_err());
}

#[test]
fn unresolved_filled_surface_continuity_omits_both_wire_fields() {
    use crate::features::{EdgeSelection, FaceSelection, FeatureDefinition, SurfaceBoundary};

    let definition = FeatureDefinition::FilledSurface {
        boundary: SurfaceBoundary::Edges(EdgeSelection::Unresolved),
        support_faces: FaceSelection::Faces(Vec::new()),
        continuity: crate::features::FilledSurfaceContinuityState::unresolved(),
        merge_result: None,
    };
    let wire = serde_json::to_value(&definition).unwrap();
    assert!(wire.get("continuity").is_none());
    assert!(wire.get("boundary_continuities").is_none());
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire).unwrap(),
        definition
    );
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
        EdgeSelection::Edges(vec![
            EdgeId::mint("synthetic:test:edge#0").expect("valid identity")
        ]),
        EdgeSelection::Resolved {
            edges: vec![EdgeId::mint("synthetic:test:edge#0").expect("valid identity")],
            native: "edge:10".into(),
        },
        EdgeSelection::historical(
            FeatureInputTopologyId::mint("synthetic:history-input:state#0")
                .expect("valid identity"),
            vec![HistoricalEdgeId::mint("synthetic:history-input:edge#0").expect("valid identity")],
            "edge:9".into(),
        )
        .unwrap(),
        EdgeSelection::historical_partial(
            FeatureInputTopologyId::mint("synthetic:history-input:state#0")
                .expect("valid identity"),
            vec![HistoricalEdgeId::mint("synthetic:history-input:edge#0").expect("valid identity")],
            vec!["native:edge-operand#1".into()],
            "edge:9".into(),
        )
        .unwrap(),
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

    let path = PathRef::historical_edges(
        FeatureInputTopologyId::mint("synthetic:history-input:state#0").expect("valid identity"),
        vec![
            HistoricalEdgeId::mint("synthetic:history-input:edge#0").expect("valid identity"),
            HistoricalEdgeId::mint("synthetic:history-input:edge#1").expect("valid identity"),
        ],
        "native:path#0".into(),
    )
    .unwrap();
    let json = serde_json::to_string(&path).unwrap();
    assert_eq!(serde_json::from_str::<PathRef>(&json).unwrap(), path);
}

#[test]
fn face_selections_round_trip_through_json() {
    use crate::features::FaceSelection;
    use crate::ids::{FaceId, FeatureInputTopologyId, HistoricalFaceId};

    let selections = vec![
        FaceSelection::Unresolved,
        FaceSelection::Faces(vec![
            FaceId::mint("synthetic:test:face#0").expect("valid identity")
        ]),
        FaceSelection::Resolved {
            faces: vec![FaceId::mint("synthetic:test:face#0").expect("valid identity")],
            native: "face:14".into(),
        },
        FaceSelection::historical(
            FeatureInputTopologyId::mint("synthetic:history-input:state#0")
                .expect("valid identity"),
            vec![HistoricalFaceId::mint("synthetic:history-input:face#0").expect("valid identity")],
            "face:13".into(),
        )
        .unwrap(),
        FaceSelection::historical_partial(
            FeatureInputTopologyId::mint("synthetic:history-input:state#0")
                .expect("valid identity"),
            vec![HistoricalFaceId::mint("synthetic:history-input:face#0").expect("valid identity")],
            vec!["native:face-operand#1".into()],
            "face:12".into(),
        )
        .unwrap(),
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

    let profile = ProfileRef::historical_faces(
        FeatureInputTopologyId::mint("synthetic:history-input:state#0").expect("valid identity"),
        vec![HistoricalFaceId::mint("synthetic:history-input:face#0").expect("valid identity")],
        vec!["native:profile-group#0".into()],
    )
    .unwrap();
    let json = serde_json::to_string(&profile).unwrap();
    assert_eq!(serde_json::from_str::<ProfileRef>(&json).unwrap(), profile);
}

#[test]
fn body_selections_round_trip_through_json() {
    use crate::features::BodySelection;
    use crate::ids::{BodyId, FeatureInputTopologyId, HistoricalBodyId};

    let selections = vec![
        BodySelection::Unresolved,
        BodySelection::Bodies(vec![
            BodyId::mint("synthetic:test:body#0").expect("valid identity")
        ]),
        BodySelection::Resolved {
            bodies: vec![BodyId::mint("synthetic:test:body#0").expect("valid identity")],
            native: "body:17".into(),
        },
        BodySelection::ResolvedSet {
            members: crate::features::BodyMembers::try_from_parts(
                vec![
                    BodyId::mint("synthetic:test:body#0").expect("valid identity"),
                    BodyId::mint("synthetic:test:body#1").expect("valid identity"),
                ],
                vec!["body:17".into(), "body:18".into()],
            )
            .expect("valid body selection rows"),
        },
        BodySelection::historical(
            FeatureInputTopologyId::mint("synthetic:history-input:state#0")
                .expect("valid identity"),
            vec![HistoricalBodyId::mint("synthetic:history-input:body#0").expect("valid identity")],
            "body:16".into(),
        )
        .unwrap(),
        BodySelection::HistoricalSet {
            state: FeatureInputTopologyId::mint("synthetic:history-input:state#0")
                .expect("valid identity"),
            members: crate::features::BodyMembers::try_from_parts(
                vec![
                    HistoricalBodyId::mint("synthetic:history-input:body#0")
                        .expect("valid identity"),
                    HistoricalBodyId::mint("synthetic:history-input:body#1")
                        .expect("valid identity"),
                ],
                vec!["body:16".into(), "body:17".into()],
            )
            .expect("valid historical body selection rows"),
        },
        BodySelection::HistoricalUnorderedSet {
            state: FeatureInputTopologyId::mint("synthetic:history-input:state#0")
                .expect("valid identity"),
            selection: crate::features::HistoricalUnorderedBodySelection::try_from_parts(
                vec![
                    HistoricalBodyId::mint("synthetic:history-input:body#0")
                        .expect("valid identity"),
                    HistoricalBodyId::mint("synthetic:history-input:body#1")
                        .expect("valid identity"),
                ],
                vec!["body:16".into(), "body:17".into()],
            )
            .expect("valid unordered historical body selection"),
        },
        BodySelection::Native("body:17,body:18".into()),
        BodySelection::NativeSet(vec!["body:17".into(), "body:18".into()].try_into().unwrap()),
    ];
    let json = serde_json::to_string(&selections).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<BodySelection>>(&json).unwrap(),
        selections
    );
}

#[test]
fn body_selection_members_reject_blank_native_rows() {
    use crate::features::{BodyMember, BodyMembers};
    use crate::ids::BodyId;

    let body = BodyId::mint("synthetic:test:body#blank").expect("identity grammar");
    assert!(BodyMember::new(body.clone(), " \t".into()).is_err());
    assert!(BodyMembers::try_from_parts(vec![body.clone()], vec!["\n".into()]).is_err());
    assert!(
        serde_json::from_value::<BodyMembers<BodyId>>(serde_json::json!([
            {"body": body, "native": " "}
        ]))
        .is_err()
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
fn loft_guidance_rejects_mixed_wire_members() {
    use crate::features::{LoftGuidance, PathRef};

    let guidance = LoftGuidance::Centerline(PathRef::Native("test:centerline".into()));
    let wire = serde_json::to_value(&guidance).unwrap();
    assert_eq!(
        serde_json::from_value::<LoftGuidance>(wire).unwrap(),
        guidance
    );
    assert!(serde_json::from_value::<LoftGuidance>(serde_json::json!({
        "kind": "guides",
        "path": [],
        "centerline": {"kind": "native", "value": "test:centerline"}
    }))
    .is_err());
}

#[test]
fn feature_result_topology_round_trips_without_current_model_bodies() {
    use crate::features::{FeatureId, FeatureResultTopology};
    use crate::ids::FeatureResultTopologyId;

    let state = FeatureResultTopology::new(
        FeatureResultTopologyId::mint("synthetic:history-result:state#0").expect("valid identity"),
        FeatureId::mint("synthetic:model:feature#0").expect("identity grammar"),
        vec!["body:17".into()],
        vec!["face:3".into()],
        vec!["edge:5".into()],
        vec!["vertex:8".into()],
        Some("native:result#0".into()),
    )
    .unwrap();
    let json = serde_json::to_string(&state).unwrap();
    assert_eq!(
        serde_json::from_str::<FeatureResultTopology>(&json).unwrap(),
        state
    );
}

#[test]
fn combine_omits_the_default_keep_tools_flag_from_json() {
    use crate::features::{BodySelection, BooleanKind, FeatureDefinition};

    let definition = FeatureDefinition::Combine {
        target: BodySelection::Native("body:17".into()),
        tools: BodySelection::Native("body:18".into()),
        op: BooleanKind::Join,
        keep_tools: false,
    };
    let json = serde_json::to_value(definition).unwrap();
    assert_eq!(json.get("keep_tools"), None);
}

#[test]
fn sweep_mode_preserves_the_solid_new_body_wire_form() {
    use crate::features::{BooleanKind, SweepMode};

    let wire = serde_json::json!({"mode": "solid", "op": "new_body"});
    assert_eq!(
        serde_json::from_value::<SweepMode>(wire.clone()).unwrap(),
        SweepMode::NewBody
    );
    assert_eq!(serde_json::to_value(SweepMode::NewBody).unwrap(), wire);
    assert!(serde_json::from_value::<SweepMode>(
        serde_json::json!({"mode": "solid", "op": "unresolved"})
    )
    .is_err());
    assert_eq!(
        serde_json::from_value::<SweepMode>(serde_json::json!({"mode": "solid", "op": "join"}))
            .unwrap(),
        SweepMode::Solid {
            op: BooleanKind::Join
        }
    );
}

#[test]
fn unresolved_feature_forms_preserve_the_legacy_wire_shape() {
    use crate::features::{ChamferSpec, PatternKind, RadiusSpec};

    for (wire, expected) in [
        (
            serde_json::json!({"kind": "unresolved"}),
            RadiusSpec::Unresolved,
        ),
        (
            serde_json::json!({"kind": "unresolved", "form": "constant"}),
            RadiusSpec::UnresolvedConstant,
        ),
        (
            serde_json::json!({"kind": "unresolved", "form": "variable"}),
            RadiusSpec::UnresolvedVariable,
        ),
    ] {
        assert_eq!(
            serde_json::from_value::<RadiusSpec>(wire.clone()).unwrap(),
            expected
        );
        assert_eq!(serde_json::to_value(expected).unwrap(), wire);
    }

    for (wire, expected) in [
        (
            serde_json::json!({"kind": "unresolved"}),
            ChamferSpec::Unresolved,
        ),
        (
            serde_json::json!({"kind": "unresolved", "form": "distance"}),
            ChamferSpec::UnresolvedDistance,
        ),
        (
            serde_json::json!({"kind": "unresolved", "form": "distance_angle"}),
            ChamferSpec::UnresolvedDistanceAngle,
        ),
    ] {
        assert_eq!(
            serde_json::from_value::<ChamferSpec>(wire.clone()).unwrap(),
            expected
        );
        assert_eq!(serde_json::to_value(expected).unwrap(), wire);
    }

    for (wire, expected) in [
        (
            serde_json::json!({"kind": "unresolved"}),
            PatternKind::UNRESOLVED,
        ),
        (
            serde_json::json!({"kind": "unresolved", "form": "linear"}),
            PatternKind::UNRESOLVED_LINEAR,
        ),
        (
            serde_json::json!({"kind": "unresolved", "form": "mirror"}),
            PatternKind::UNRESOLVED_MIRROR,
        ),
    ] {
        assert_eq!(
            serde_json::from_value::<PatternKind>(wire.clone()).unwrap(),
            expected
        );
        assert_eq!(serde_json::to_value(expected).unwrap(), wire);
    }
}

#[test]
fn face_maker_preserves_the_legacy_wire_and_rejects_split_discriminants() {
    use crate::features::FaceMaker;

    #[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
    struct ExtrusionCarrier {
        #[serde(default, with = "super::optional_extrusion_face_maker")]
        maker: Option<FaceMaker>,
    }

    assert_eq!(
        serde_json::from_value::<FaceMaker>(serde_json::json!("Part::FaceMakerUnified")).unwrap(),
        FaceMaker::Unified,
    );
    assert_eq!(
        serde_json::to_value(FaceMaker::Bullseye).unwrap(),
        serde_json::json!("Part::FaceMakerBullseye"),
    );
    assert!(serde_json::from_value::<FaceMaker>(serde_json::json!("")).is_err());

    let wire = serde_json::json!({
        "maker": {
            "class": "Part::FaceMakerBullseye",
            "mode": 3
        }
    });
    let carrier = serde_json::from_value::<ExtrusionCarrier>(wire.clone()).unwrap();
    assert_eq!(
        carrier,
        ExtrusionCarrier {
            maker: Some(FaceMaker::Bullseye),
        }
    );
    assert_eq!(serde_json::to_value(carrier).unwrap(), wire);

    let mismatch = serde_json::from_value::<ExtrusionCarrier>(serde_json::json!({
        "maker": {
            "class": "Part::FaceMakerBullseye",
            "mode": 4
        }
    }))
    .unwrap_err();
    assert!(mismatch.to_string().contains("face_maker.mode"));
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
    use crate::features::{FeatureDefinition, WrapMode};

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
            mode: WrapMode::Emboss { depth: actual_depth },
            ..
        } if actual_depth.get() == 2.5
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
    use crate::features::{FeatureDefinition, HelixShape};

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
        } if pitch.get() == 3.0
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
                radial_growth: actual_radial_growth
            },
            ..
        } if actual_radial_growth.get() == 1.5
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

#[test]
fn revolve_construction_preserves_the_flat_wire_shape() {
    use crate::features::{FeatureDefinition, PathRef, RevolveConstruction};

    let wire = serde_json::json!({
        "definition": "revolve",
        "construction": {
            "profile": {"kind": "sketch", "value": "test:model:sketch#profile"},
            "axis": {
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "direction": {"x": 0.0, "y": 0.0, "z": 1.0}
            },
            "extent": {
                "kind": "one_sided",
                "termination": {"kind": "angle", "angle": 1.25}
            },
            "axis_reference": {"kind": "native", "value": "test:axis"},
            "solid": true,
            "face_maker_class": "Part::FaceMakerBullseye"
        },
        "op": "new_body"
    });
    let definition: FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Revolve {
            construction: RevolveConstruction::Resolved { ref axis, .. },
            ..
        } if axis.reference == Some(PathRef::Native("test:axis".into()))
    ));
    assert_eq!(serde_json::to_value(definition).unwrap(), wire);
}

#[test]
fn revolve_construction_admits_only_typed_partial_states() {
    use crate::features::{FeatureDefinition, RevolveConstruction};

    let partial_wire = serde_json::json!({
        "definition": "revolve",
        "construction": {
            "profile": {"kind": "native", "value": "test:profile"},
            "solid": false
        },
        "op": "unresolved"
    });
    let partial: FeatureDefinition = serde_json::from_value(partial_wire.clone()).unwrap();
    assert!(matches!(
        partial,
        FeatureDefinition::Revolve {
            construction: RevolveConstruction::Unresolved(_),
            ..
        }
    ));
    assert_eq!(serde_json::to_value(partial).unwrap(), partial_wire);

    let orphan_reference = serde_json::json!({
        "definition": "revolve",
        "construction": {
            "axis_reference": {"kind": "native", "value": "test:axis"}
        },
        "op": "unresolved"
    });
    let error = serde_json::from_value::<FeatureDefinition>(orphan_reference)
        .unwrap_err()
        .to_string();
    assert!(error.contains("axis_reference requires a revolution axis"));
}

#[test]
fn extrude_direction_preserves_the_flat_source_wire_shape() {
    use crate::features::{ExtrudeDirection, ExtrusionDirectionSource, FeatureDefinition, PathRef};

    let wire = serde_json::json!({
        "definition": "extrude",
        "profile": {"kind": "native", "value": "test:profile"},
        "direction": {
            "kind": "explicit",
            "value": {"x": 0.0, "y": 1.0, "z": 0.0}
        },
        "start": {"kind": "profile_plane"},
        "extent": {
            "kind": "one_sided",
            "side": {
                "termination": {"kind": "blind", "length": 4.0}
            }
        },
        "op": "new_body",
        "direction_source": {
            "kind": "edge",
            "reference": {"kind": "native", "value": "test:direction-edge"}
        },
        "solid": true
    });
    let definition: FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::Explicit {
                source: Some(ExtrusionDirectionSource::Edge {
                    reference: PathRef::Native(ref reference),
                }),
                ..
            },
            ..
        } if reference == "test:direction-edge"
    ));
    assert_eq!(serde_json::to_value(definition).unwrap(), wire);
}

#[test]
fn extrude_direction_rejects_a_source_without_an_explicit_vector() {
    use crate::features::FeatureDefinition;

    let invalid = serde_json::json!({
        "definition": "extrude",
        "profile": {"kind": "native", "value": "test:profile"},
        "start": {"kind": "profile_plane"},
        "extent": {
            "kind": "one_sided",
            "side": {
                "termination": {"kind": "blind", "length": 4.0}
            }
        },
        "op": "new_body",
        "direction_source": {"kind": "custom"}
    });
    let error = serde_json::from_value::<FeatureDefinition>(invalid)
        .unwrap_err()
        .to_string();
    assert!(error.contains("direction_source requires an explicit extrusion direction"));
}

#[test]
fn per_edge_flange_widths_reject_empty_rosters_and_preserve_array_wire() {
    let wire = serde_json::json!({
        "kind": "two_sides_per_edge",
        "value": {"widths": [{"first": 3.0, "second": 1.5}, {"first": 2.0, "second": 4.0}]}
    });
    let width: super::SheetMetalFlangeWidth = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(width).unwrap(), wire);
    let mut empty = wire;
    empty["value"]["widths"] = serde_json::json!([]);
    let error = serde_json::from_value::<super::SheetMetalFlangeWidth>(empty).unwrap_err();
    assert!(error
        .to_string()
        .contains("widths must contain at least one pair"));
    assert!(super::SheetMetalFlangeEdgeWidths::new(Vec::new()).is_err());
}

#[test]
fn sketch_binding_preserves_known_planar_space_without_geometry() {
    for wire in [
        serde_json::json!({"definition": "sketch", "space": "unresolved"}),
        serde_json::json!({"definition": "sketch", "space": "planar"}),
        serde_json::json!({"definition": "sketch", "space": "planar", "sketch": "test:model:sketch#1"}),
    ] {
        let definition: super::FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(definition).unwrap(), wire);
    }
    for wire in [
        serde_json::json!({"definition": "sketch", "space": "unresolved", "sketch": "test:model:sketch#1"}),
        serde_json::json!({"definition": "sketch", "space": "spatial"}),
    ] {
        assert!(serde_json::from_value::<super::FeatureDefinition>(wire).is_err());
    }
    assert_ne!(
        super::SketchFeatureBinding::Unresolved,
        super::SketchFeatureBinding::Planar(None)
    );
}

mod body_selection;

#[test]
fn active_configuration_evaluation_can_have_no_body_outputs() {
    use crate::features::ConfigurationEvaluation;

    let active = ConfigurationEvaluation::Active {
        outputs: Default::default(),
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

#[test]
fn body_selection_admission_rejects_invalid_members() {
    use super::{BodySelection, GeneratedBodyRef, NativeSelections};
    use crate::ids::FeatureInputTopologyId;
    for names in [
        vec![],
        vec![" ".to_owned()],
        vec!["a".to_owned(), "a".to_owned()],
    ] {
        assert!(BodySelection::local(names.clone(), "native".into()).is_err());
        assert!(NativeSelections::try_from(names).is_err());
    }
    assert!(BodySelection::local(vec!["body".into()], " ".into()).is_err());
    let state = FeatureInputTopologyId::mint("test:input#1").unwrap();
    assert!(BodySelection::historical(state, vec![], "native".into()).is_err());
    assert!(GeneratedBodyRef::new(
        super::FeatureId::mint("test:feature#1").unwrap(),
        " ".into()
    )
    .is_err());
    for value in [
        serde_json::json!({"kind":"local","value":{"bodies":[],"native":"source"}}),
        serde_json::json!({"kind":"local","value":{"bodies":["a","a"],"native":"source"}}),
        serde_json::json!({"kind":"generated","value":{"bodies":[],"native":"source"}}),
        serde_json::json!({"kind":"native_set","value":[" "]}),
    ] {
        assert!(serde_json::from_value::<BodySelection>(value).is_err());
    }
}

#[test]
fn topology_membership_admission() {
    use super::{DistinctMembers, FeatureResultTopology};
    let id = crate::ids::FeatureResultTopologyId::mint("test:result#1").unwrap();
    let feature = super::FeatureId::mint("test:feature#1").unwrap();
    assert!(FeatureResultTopology::new(
        id.clone(),
        feature.clone(),
        vec![],
        vec![],
        vec![],
        vec![],
        None
    )
    .is_err());
    for bodies in [vec![" ".into()], vec!["a".into(), "a".into()]] {
        assert!(FeatureResultTopology::new(
            id.clone(),
            feature.clone(),
            bodies,
            vec![],
            vec![],
            vec![],
            None
        )
        .is_err());
    }
    assert!(DistinctMembers::<String>::try_from(vec!["a".into(), "a".into()]).is_err());
    assert!(
        serde_json::from_value::<super::ConfigurationBodies>(serde_json::json!([
            "test:body#1",
            "test:body#1"
        ]))
        .is_err()
    );
    assert!(serde_json::from_value::<FeatureResultTopology>(
        serde_json::json!({"id":"test:result#1","output_of":"test:feature#1"})
    )
    .is_err());
}

#[test]
fn feature_scalars_reject_nonfinite_constructor_and_serde_values() {
    use crate::features::{Angle, Length};
    use serde::de::value::{Error, F64Deserializer};

    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(Length::new(value).is_none());
        assert!(Angle::new(value).is_none());
        assert!(
            <Length as serde::Deserialize>::deserialize(F64Deserializer::<Error>::new(value))
                .is_err()
        );
        assert!(
            <Angle as serde::Deserialize>::deserialize(F64Deserializer::<Error>::new(value))
                .is_err()
        );
    }
    for value in [-10.0, 0.0, 10.0] {
        let length = Length::new(value).unwrap();
        let angle = Angle::new(value).unwrap();
        assert_eq!(length.get(), value);
        assert_eq!(angle.get(), value);
        assert_eq!(
            serde_json::to_value(length).unwrap(),
            serde_json::json!(value)
        );
        assert_eq!(
            serde_json::to_value(angle).unwrap(),
            serde_json::json!(value)
        );
        assert_eq!(
            serde_json::from_value::<Length>(serde_json::json!(value)).unwrap(),
            length
        );
        assert_eq!(
            serde_json::from_value::<Angle>(serde_json::json!(value)).unwrap(),
            angle
        );
    }
}

#[test]
fn positive_lengths_reject_zero_negative_and_nonfinite_values() {
    use crate::features::PositiveLength;
    use serde::de::value::{Error, F64Deserializer};

    for value in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(PositiveLength::new(value).is_none());
        assert!(<PositiveLength as serde::Deserialize>::deserialize(
            F64Deserializer::<Error>::new(value)
        )
        .is_err());
    }
    let length = PositiveLength::new(2.5).unwrap();
    assert_eq!(length.get(), 2.5);
    assert_eq!(
        serde_json::to_value(length).unwrap(),
        serde_json::json!(2.5)
    );
    assert_eq!(
        serde_json::from_str::<PositiveLength>("2.5").unwrap(),
        length
    );
}

#[test]
fn primitive_dimensions_are_checked_at_construction_and_deserialization() {
    use super::{PrimitiveSolid, PrimitiveSolidKind};
    use serde_json::{json, Value};

    let cases: [(Value, &[&str]); 8] = [
        (
            json!({"kind":"box","length":1.0,"width":2.0,"height":3.0}),
            &["length", "width", "height"],
        ),
        (
            json!({"kind":"cylinder","radius":1.0,"height":2.0,"angle":0.0}),
            &["radius", "height"],
        ),
        (
            json!({"kind":"cone","radius1":0.0,"radius2":1.0,"height":2.0,"angle":-1.0}),
            &["height"],
        ),
        (
            json!({"kind":"sphere","radius":1.0,"latitude1":-1.0,"latitude2":1.0,"longitude":0.0}),
            &["radius"],
        ),
        (
            json!({"kind":"ellipsoid","x_radius":1.0,"y_radius":2.0,"z_radius":3.0,"latitude1":-1.0,"latitude2":1.0,"longitude":-1.0}),
            &["x_radius", "y_radius", "z_radius"],
        ),
        (
            json!({"kind":"torus","major_radius":1.0,"minor_radius":2.0,"latitude1":-1.0,"latitude2":1.0,"longitude":0.0}),
            &["major_radius", "minor_radius"],
        ),
        (
            json!({"kind":"prism","sides":3,"circumradius":1.0,"height":2.0}),
            &["circumradius", "height"],
        ),
        (
            json!({"kind":"wedge","xmin":-1.0,"ymin":-1.0,"zmin":-1.0,"x2min":0.0,"z2min":0.0,"xmax":1.0,"ymax":1.0,"zmax":1.0,"x2max":0.0,"z2max":0.0}),
            &[],
        ),
    ];
    for (wire, positive_fields) in cases {
        let kind: PrimitiveSolidKind = serde_json::from_value(wire.clone()).unwrap();
        let solid = PrimitiveSolid::new(kind).unwrap();
        assert_eq!(serde_json::to_value(&solid).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<PrimitiveSolid>(wire.clone()).unwrap(),
            solid
        );
        let mut invalid = Vec::new();
        for field in positive_fields {
            for value in [0.0, -1.0] {
                let mut changed = wire.clone();
                changed[field] = json!(value);
                invalid.push(changed);
            }
        }
        match wire["kind"].as_str().unwrap() {
            "cone" => {
                let mut both_zero = wire.clone();
                both_zero["radius2"] = json!(0.0);
                invalid.push(both_zero);
                for field in ["radius1", "radius2"] {
                    let mut changed = wire.clone();
                    changed[field] = json!(-1.0);
                    invalid.push(changed);
                }
            }
            "sphere" | "ellipsoid" | "torus" => {
                for lower in [1.0, 2.0] {
                    let mut changed = wire.clone();
                    changed["latitude1"] = json!(lower);
                    invalid.push(changed);
                }
            }
            "prism" => {
                for sides in [0, 1, 2] {
                    let mut changed = wire.clone();
                    changed["sides"] = json!(sides);
                    invalid.push(changed);
                }
            }
            "wedge" => {
                for field in ["xmax", "ymax", "zmax", "x2max", "z2max"] {
                    let mut changed = wire.clone();
                    changed[field] = json!(-1.0);
                    invalid.push(changed);
                }
            }
            _ => {}
        }
        for changed in invalid {
            let kind: PrimitiveSolidKind = serde_json::from_value(changed.clone()).unwrap();
            assert!(PrimitiveSolid::new(kind).is_err(), "{changed}");
            assert!(
                serde_json::from_value::<PrimitiveSolid>(changed.clone()).is_err(),
                "{changed}"
            );
        }
    }
}

#[test]
fn bounded_feature_scalars_reject_out_of_domain_values_on_every_admission_route() {
    use crate::features::{
        FiniteReal, Fraction, InteriorAngle, NonNegativeLength, NonZeroLength, NonZeroReal,
        PositiveAngle, PositiveReal, SlopeAngle,
    };
    use serde::de::value::{Error, F64Deserializer};
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    macro_rules! check {
        ($ty:ty, valid: [$($valid:expr),*], invalid: [$($invalid:expr),*]) => {
            for value in [$($valid),*] {
                let admitted = <$ty>::new(value).unwrap();
                assert_eq!(admitted.get().to_bits(), value.to_bits());
                let wire = serde_json::to_string(&admitted).unwrap();
                let decoded: $ty = serde_json::from_str(&wire).unwrap();
                assert_eq!(decoded.get().to_bits(), value.to_bits());
            }
            for value in [$($invalid,)* f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                assert!(<$ty>::new(value).is_none());
                assert!(<$ty as serde::Deserialize>::deserialize(F64Deserializer::<Error>::new(value)).is_err());
            }
        };
    }

    check!(NonNegativeLength, valid: [-0.0, 0.0, 2.0], invalid: [-1.0]);
    check!(SlopeAngle, valid: [-1.0, -0.0, 0.0, 1.0], invalid: [-FRAC_PI_2, FRAC_PI_2, PI]);
    check!(InteriorAngle, valid: [0.5, FRAC_PI_2], invalid: [-1.0, 0.0, PI]);
    check!(PositiveAngle, valid: [1.0, TAU, 2.0 * TAU], invalid: [-1.0, -0.0, 0.0]);
    check!(FiniteReal, valid: [-10.0, -0.0, 0.0, 10.0], invalid: []);
    check!(PositiveReal, valid: [0.5, 10.0], invalid: [-1.0, -0.0, 0.0]);
    check!(NonZeroLength, valid: [-2.0, 0.5], invalid: [-0.0, 0.0]);
    check!(NonZeroReal, valid: [-2.0, 0.5], invalid: [-0.0, 0.0]);
    check!(Fraction, valid: [-0.0, 0.0, 0.5, 1.0], invalid: [-0.5, 1.5]);
}

#[test]
fn scale_factors_admit_only_finite_nonzero_components() {
    use crate::features::{NonZeroReal, ScaleFactors};
    for value in [0.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(NonZeroReal::new(value).is_none());
    }
    for value in [-2.0, 0.5] {
        let factors = ScaleFactors::Uniform(NonZeroReal::new(value).unwrap());
        let wire = serde_json::to_value(factors).unwrap();
        assert_eq!(wire, serde_json::json!({"uniform": value}));
        assert_eq!(
            serde_json::from_value::<ScaleFactors>(wire).unwrap(),
            factors
        );
    }
    let factors =
        ScaleFactors::PerAxis([-1.0, 2.0, -3.0].map(|value| NonZeroReal::new(value).unwrap()));
    let wire = serde_json::to_value(factors).unwrap();
    assert_eq!(wire, serde_json::json!({"x": -1.0, "y": 2.0, "z": -3.0}));
    assert_eq!(
        serde_json::from_value::<ScaleFactors>(wire).unwrap(),
        factors
    );
    for wire in [
        serde_json::json!({"uniform": 0.0}),
        serde_json::json!({"x": 0.0, "y": 1.0, "z": 1.0}),
        serde_json::json!({"x": 1.0, "y": 0.0, "z": 1.0}),
        serde_json::json!({"x": 1.0, "y": 1.0, "z": 0.0}),
        serde_json::json!({"x": 1.0, "y": 1.0}),
        serde_json::json!({"uniform": 1.0, "x": 1.0}),
    ] {
        assert!(serde_json::from_value::<ScaleFactors>(wire).is_err());
    }
}

#[test]
fn blind_and_coil_lengths_reject_zero_without_losing_signed_wire_values() {
    use crate::features::{CoilExtent, LinearTermination};
    for value in [-4.0, 4.0] {
        let wire = serde_json::json!({"kind": "blind", "length": value});
        let admitted: LinearTermination = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
    }
    for value in [-0.0, 0.0] {
        assert!(serde_json::from_value::<LinearTermination>(
            serde_json::json!({"kind":"blind","length":value})
        )
        .is_err());
        for wire in [
            serde_json::json!({"kind":"revolutions_pitch","revolutions":1.0,"pitch":value}),
            serde_json::json!({"kind":"height_pitch","height":value,"pitch":1.0}),
            serde_json::json!({"kind":"height_pitch","height":1.0,"pitch":value}),
            serde_json::json!({"kind":"spiral","revolutions":1.0,"radial_pitch":value}),
        ] {
            assert!(serde_json::from_value::<CoilExtent>(wire).is_err());
        }
    }
    let wire = serde_json::json!({"kind":"revolutions_height","revolutions":1.0,"height":0.0});
    let admitted: CoilExtent = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
}

#[test]
fn hole_profile_filters_admit_exactly_the_nonempty_family_sets() {
    use crate::features::HoleProfileFilter;
    let filters = [
        HoleProfileFilter::Points,
        HoleProfileFilter::Circles,
        HoleProfileFilter::PointsAndCircles,
        HoleProfileFilter::Arcs,
        HoleProfileFilter::PointsAndArcs,
        HoleProfileFilter::CirclesAndArcs,
        HoleProfileFilter::All,
    ];
    for (bits, filter) in (1..=7).zip(filters) {
        let wire = serde_json::json!({"points": bits & 1 != 0, "circles": bits & 2 != 0, "arcs": bits & 4 != 0});
        assert_eq!(serde_json::to_value(filter).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<HoleProfileFilter>(wire).unwrap(),
            filter
        );
    }
    assert!(serde_json::from_value::<HoleProfileFilter>(
        serde_json::json!({"points":false,"circles":false,"arcs":false})
    )
    .is_err());
}

#[test]
fn polygon_side_counts_reject_degenerate_polygons_at_admission() {
    use crate::features::PolygonSideCount;
    for value in [0, 1, 2] {
        assert!(PolygonSideCount::new(value).is_none());
        assert!(serde_json::from_value::<PolygonSideCount>(serde_json::json!(value)).is_err());
    }
    for value in [3, 7, u32::MAX] {
        let sides = PolygonSideCount::new(value).unwrap();
        assert_eq!(sides.get(), value);
        assert_eq!(
            serde_json::to_value(sides).unwrap(),
            serde_json::json!(value)
        );
        assert_eq!(
            serde_json::from_value::<PolygonSideCount>(serde_json::json!(value)).unwrap(),
            sides
        );
    }
}

#[test]
fn feature_geometry_admission_preserves_nonunit_directions_and_zero_displacements() {
    use crate::features::{FeatureDirection3, FinitePoint3, FiniteVector3};
    for point in [Point3::new(0.0, 0.0, 0.0), Point3::new(-1.0, 2.0, f64::MAX)] {
        let admitted = FinitePoint3::new(point).unwrap();
        let wire = serde_json::to_value(point).unwrap();
        assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FinitePoint3>(wire).unwrap(),
            admitted
        );
    }
    for vector in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(-1.0, 2.0, f64::MAX),
    ] {
        let admitted = FiniteVector3::new(vector).unwrap();
        let wire = serde_json::to_value(vector).unwrap();
        assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FiniteVector3>(wire).unwrap(),
            admitted
        );
    }
    for vector in [Vector3::new(0.0, 0.0, -2.0), Vector3::new(3.0, 4.0, 0.0)] {
        let admitted = FeatureDirection3::new(vector).unwrap();
        let wire = serde_json::to_value(vector).unwrap();
        assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FeatureDirection3>(wire).unwrap(),
            admitted
        );
    }
    for vector in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(f64::MAX, 0.0, 0.0),
    ] {
        assert!(FeatureDirection3::new(vector).is_none());
        assert!(
            serde_json::from_value::<FeatureDirection3>(serde_json::to_value(vector).unwrap())
                .is_err()
        );
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for axis in 0..3 {
            let mut components = [0.0; 3];
            components[axis] = invalid;
            assert!(FinitePoint3::new(Point3::from(components)).is_none());
            assert!(FiniteVector3::new(Vector3::from(components)).is_none());
            assert!(FeatureDirection3::new(Vector3::from(components)).is_none());
            let deserialize_point = || {
                serde::de::value::MapDeserializer::<_, serde::de::value::Error>::new(
                    [
                        ("x", components[0]),
                        ("y", components[1]),
                        ("z", components[2]),
                    ]
                    .into_iter(),
                )
            };
            assert!(
                <FinitePoint3 as serde::Deserialize>::deserialize(deserialize_point()).is_err()
            );
            assert!(
                <FiniteVector3 as serde::Deserialize>::deserialize(deserialize_point()).is_err()
            );
            assert!(
                <FeatureDirection3 as serde::Deserialize>::deserialize(deserialize_point())
                    .is_err()
            );
        }
    }
}

#[test]
fn feature_lines_and_polylines_close_geometry_bounds_without_changing_wire_fields() {
    use crate::features::{FeatureDefinition, FeatureLineSegment, FeaturePolyline};
    let first = Point3::new(0.0, 1.0, 2.0);
    let second = Point3::new(3.0, 4.0, 5.0);
    let segment = FeatureLineSegment::new(first, second).unwrap();
    let wire = serde_json::json!({"definition":"line_segment", "start":first, "end":second});
    let definition = FeatureDefinition::LineSegment { segment };
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire).unwrap(),
        definition
    );
    assert!(FeatureLineSegment::new(first, first).is_none());
    assert!(FeatureLineSegment::new(Point3::new(f64::NAN, 0.0, 0.0), second).is_none());
    assert!(serde_json::from_value::<FeatureDefinition>(
        serde_json::json!({"definition":"line_segment", "start":first, "end":first})
    )
    .is_err());

    for (points, closed) in [
        (vec![first, second], false),
        (vec![first, second, first], true),
        (vec![first, second, first], false),
    ] {
        let chain = FeaturePolyline::new(points.clone(), closed).unwrap();
        let wire = serde_json::json!({"definition":"polyline", "points":points, "closed":closed});
        let definition = FeatureDefinition::Polyline { chain };
        assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FeatureDefinition>(wire).unwrap(),
            definition
        );
    }
    for (points, closed) in [
        (vec![], false),
        (vec![first], false),
        (vec![first, second], true),
        (vec![first, first, second], false),
    ] {
        assert!(FeaturePolyline::new(points.clone(), closed).is_none());
        assert!(serde_json::from_value::<FeatureDefinition>(
            serde_json::json!({"definition":"polyline", "points":points, "closed":closed})
        )
        .is_err());
    }
    assert!(
        FeaturePolyline::new(vec![first, Point3::new(f64::INFINITY, 0.0, 0.0)], false).is_none()
    );
}

#[test]
fn equation_curve_admission_preserves_expression_text_and_requires_an_increasing_domain() {
    use crate::features::{FeatureDefinition, FeatureEquationCurve};
    let curve = FeatureEquationCurve::new(
        " t ".into(),
        " t*t ".into(),
        "0".into(),
        " -t ".into(),
        -2.0,
        3.0,
    )
    .unwrap();
    let definition = FeatureDefinition::EquationCurve { curve };
    let wire = serde_json::json!({"definition":"equation_curve", "parameter":" t ", "x_expression":" t*t ", "y_expression":"0", "z_expression":" -t ", "start":-2.0, "end":3.0});
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
        definition
    );
    for field in ["parameter", "x_expression", "y_expression", "z_expression"] {
        for blank in ["", " \t\n"] {
            let mut invalid = wire.clone();
            invalid[field] = serde_json::json!(blank);
            assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
        }
    }
    for [start, end] in [
        [1.0, 1.0],
        [2.0, 1.0],
        [f64::NAN, 2.0],
        [0.0, f64::INFINITY],
        [f64::NEG_INFINITY, 0.0],
    ] {
        assert!(FeatureEquationCurve::new(
            "t".into(),
            "t".into(),
            "0".into(),
            "0".into(),
            start,
            end
        )
        .is_none());
    }
    for [start, end] in [[1.0, 1.0], [2.0, 1.0]] {
        let mut invalid = wire.clone();
        invalid["start"] = serde_json::json!(start);
        invalid["end"] = serde_json::json!(end);
        assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
    }
}

#[test]
fn feature_arcs_preserve_directed_spans_and_admit_only_valid_frames_and_radii() {
    use crate::features::{
        FeatureCircularArc, FeatureDefinition, FeatureEllipticArc, PositiveLength,
    };
    use crate::geometry::DirectedParameterRange;
    let center = Point3::new(1.0, 2.0, 3.0);
    let normal = Vector3::new(0.0, 0.0, 2.0);
    let major_axis = Vector3::new(3.0, 0.0, 0.0);
    let major = PositiveLength::new(4.0).unwrap();
    let minor = PositiveLength::new(2.0).unwrap();
    for endpoints in [[0.0, std::f64::consts::TAU], [2.0, -1.0]] {
        let angles = DirectedParameterRange::new(endpoints).unwrap();
        let arc = FeatureCircularArc::new(center, normal, major, angles).unwrap();
        let definition = FeatureDefinition::CircularArc { arc };
        let wire = serde_json::json!({"definition":"circular_arc", "center":center, "normal":normal, "radius":4.0, "start_angle":endpoints[0], "end_angle":endpoints[1]});
        assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
            definition
        );
        let mut invalid = wire;
        invalid["end_angle"] = invalid["start_angle"].clone();
        assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());

        let arc =
            FeatureEllipticArc::new(center, normal, major_axis, [major, minor], angles).unwrap();
        let definition = FeatureDefinition::EllipticArc { arc };
        let wire = serde_json::json!({"definition":"elliptic_arc", "center":center, "normal":normal, "major_axis":major_axis, "major_radius":4.0, "minor_radius":2.0, "start_angle":endpoints[0], "end_angle":endpoints[1]});
        assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
            definition
        );
        for (field, value) in [
            ("minor_radius", serde_json::json!(5.0)),
            ("major_axis", serde_json::json!({"x":0.0,"y":0.0,"z":1.0})),
            ("end_angle", serde_json::json!(endpoints[0])),
        ] {
            let mut invalid = wire.clone();
            invalid[field] = value;
            assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
        }
    }
    let angles = DirectedParameterRange::new([0.0, 1.0]).unwrap();
    assert!(FeatureCircularArc::new(center, Vector3::new(0.0, 0.0, 0.0), major, angles).is_none());
    assert!(FeatureEllipticArc::new(center, normal, major_axis, [minor, major], angles).is_none());
    assert!(FeatureEllipticArc::new(center, normal, major_axis, [major, major], angles).is_some());
    assert!(FeatureEllipticArc::new(center, normal, normal, [major, minor], angles).is_none());
    assert!(FeatureEllipticArc::new(
        center,
        normal,
        Vector3::new(1.0, 0.0, super::EPS_FEATURE_ELLIPSE_AXES_ORTHO / 2.0),
        [major, minor],
        angles
    )
    .is_some());
    assert!(FeatureEllipticArc::new(
        center,
        normal,
        Vector3::new(1.0, 0.0, super::EPS_FEATURE_ELLIPSE_AXES_ORTHO),
        [major, minor],
        angles
    )
    .is_none());
}

#[test]
fn block_placement_admission_requires_a_right_handed_rigid_transform() {
    use crate::features::FeatureRigidPlacement;
    use crate::transform::Transform;
    for rows in [
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        [
            [0.0, -1.0, 0.0, 3.0],
            [1.0, 0.0, 0.0, -2.0],
            [0.0, 0.0, 1.0, 5.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    ] {
        let transform = Transform::from_rows(rows).unwrap();
        let placement = FeatureRigidPlacement::new(transform).unwrap();
        let wire = serde_json::to_value(transform).unwrap();
        assert_eq!(serde_json::to_value(placement).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FeatureRigidPlacement>(wire).unwrap(),
            placement
        );
    }
    for rows in [
        [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        [
            [1.0, 0.25, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    ] {
        let transform = Transform::from_rows(rows).unwrap();
        assert!(FeatureRigidPlacement::new(transform).is_none());
        assert!(serde_json::from_value::<FeatureRigidPlacement>(
            serde_json::to_value(transform).unwrap()
        )
        .is_err());
    }
}

#[test]
fn helical_sweep_travel_preserves_signed_and_planar_values_but_rejects_zero_travel() {
    use crate::features::{HelicalSweepTravel, Length};
    for [height, radial_growth] in [[-2.0, 0.0], [0.0, -3.0], [2.0, -3.0]] {
        let travel = HelicalSweepTravel::new(
            Length::new(height).unwrap(),
            Length::new(radial_growth).unwrap(),
        )
        .unwrap();
        let wire = serde_json::json!({"height":height,"radial_growth":radial_growth});
        assert_eq!(serde_json::to_value(travel).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<HelicalSweepTravel>(wire).unwrap(),
            travel
        );
    }
    assert!(HelicalSweepTravel::new(Length::ZERO, Length::ZERO).is_none());
    assert!(serde_json::from_value::<HelicalSweepTravel>(
        serde_json::json!({"height":0.0,"radial_growth":0.0})
    )
    .is_err());
}

#[test]
fn feature_coordinate_frame_admission_preserves_wire_and_handedness_bound() {
    use crate::features::{FeatureCoordinateFrame, FeatureDefinition, EPS_FEATURE_UNIT_FRAME};
    use crate::math::{Point3, Vector3};
    let origin = Point3::new(1.0, 2.0, 3.0);
    let x = Vector3::new(1.0, 0.0, 0.0);
    let y = Vector3::new(0.0, 1.0, 0.0);
    let z = Vector3::new(0.0, 0.0, 1.0);
    let frame = FeatureCoordinateFrame::new(origin, x, y, z).unwrap();
    let wire = serde_json::json!({"definition":"datum_coordinate_system","origin":origin,"x_axis":x,"y_axis":y,"z_axis":z});
    let definition = FeatureDefinition::DatumCoordinateSystem { frame };
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
        definition
    );
    for (key, value) in [
        ("x_axis", y),
        ("z_axis", Vector3::new(0.0, 0.0, -1.0)),
        ("x_axis", Vector3::new(2.0, 0.0, 0.0)),
    ] {
        let mut invalid = wire.clone();
        invalid[key] = serde_json::to_value(value).unwrap();
        assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
    }
    assert!(FeatureCoordinateFrame::new(Point3::new(f64::NAN, 0.0, 0.0), x, y, z).is_none());
    let expanded = 1.0 + EPS_FEATURE_UNIT_FRAME * 0.75;
    let frame = FeatureCoordinateFrame::new(
        origin,
        Vector3::new(expanded, 0.0, 0.0),
        Vector3::new(0.0, expanded, 0.0),
        Vector3::new(0.0, 0.0, expanded),
    )
    .unwrap();
    assert!(
        frame.x_axis().cross(frame.y_axis()).dot(frame.z_axis()) > 1.0 + EPS_FEATURE_UNIT_FRAME
    );
}

#[test]
fn feature_unit_plane_and_image_bounds_reject_degenerate_geometry() {
    use crate::features::{FeatureImageBounds, FeatureUnitPlaneFrame, EPS_FEATURE_UNIT_FRAME};
    use crate::math::{Point2, Point3, Vector3};
    let origin = Point3::new(0.0, 0.0, 0.0);
    let x = Vector3::new(1.0, 0.0, 0.0);
    for other in [
        Vector3::new(0.0, 0.0, 0.0),
        x,
        Vector3::new(f64::INFINITY, 0.0, 0.0),
    ] {
        assert!(FeatureUnitPlaneFrame::new(origin, x, other).is_none());
    }
    assert!(FeatureUnitPlaneFrame::new(
        origin,
        x,
        Vector3::new(EPS_FEATURE_UNIT_FRAME * 0.5, 1.0, 0.0)
    )
    .is_some());
    assert!(FeatureUnitPlaneFrame::new(
        origin,
        x,
        Vector3::new(EPS_FEATURE_UNIT_FRAME * 2.0, 1.0, 0.0)
    )
    .is_none());
    assert!(serde_json::from_value::<FeatureUnitPlaneFrame>(
        serde_json::json!({"origin":origin,"u_axis":x,"v_axis":x})
    )
    .is_err());
    for corners in [
        [Point2::new(2.0, 3.0), Point2::new(-2.0, -3.0)],
        [Point2::new(-2.0, 3.0), Point2::new(2.0, -3.0)],
    ] {
        let bounds = FeatureImageBounds::new(corners).unwrap();
        assert_eq!(bounds.corners(), corners);
        let wire = serde_json::to_value(corners).unwrap();
        assert_eq!(serde_json::to_value(bounds).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FeatureImageBounds>(wire).unwrap(),
            bounds
        );
    }
    for corners in [
        [Point2::new(0.0, 0.0), Point2::new(0.0, 1.0)],
        [Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
    ] {
        assert!(FeatureImageBounds::new(corners).is_none());
        assert!(serde_json::from_value::<FeatureImageBounds>(
            serde_json::to_value(corners).unwrap()
        )
        .is_err());
    }
    assert!(FeatureImageBounds::new([Point2::new(f64::NAN, 0.0), Point2::new(1.0, 1.0)]).is_none());
}

#[test]
fn reference_image_and_coil_frames_preserve_wire_fields_and_reject_invalid_frames() {
    use crate::features::{CoilPlacement, FeatureDefinition};
    let image = serde_json::json!({
        "definition":"reference_image", "asset":"synthetic:test:asset#frame",
        "visible":true, "mirror_u":false, "mirror_v":false,
        "origin":{"x":1.0,"y":2.0,"z":3.0},
        "u_axis":{"x":1.0,"y":0.0,"z":0.0},
        "v_axis":{"x":0.0,"y":1.0,"z":0.0},
        "bounds":[{"u":2.0,"v":3.0},{"u":-2.0,"v":-3.0}]
    });
    let decoded = serde_json::from_value::<FeatureDefinition>(image.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), image);
    let mut invalid = image.clone();
    invalid["v_axis"] = invalid["u_axis"].clone();
    assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
    let mut invalid = image;
    invalid["bounds"][1]["u"] = serde_json::json!(2.0);
    assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());

    let coil = serde_json::json!({"kind":"explicit",
        "origin":{"x":1.0,"y":2.0,"z":3.0},
        "axis":{"x":0.0,"y":0.0,"z":1.0},
        "radial":{"x":1.0,"y":0.0,"z":0.0}
    });
    let decoded = serde_json::from_value::<CoilPlacement>(coil.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), coil);
    let mut invalid = coil;
    invalid["radial"] = invalid["axis"].clone();
    assert!(serde_json::from_value::<CoilPlacement>(invalid).is_err());
    assert!(serde_json::from_value::<CoilPlacement>(
        serde_json::json!({"kind":"native","native_ref":" \t "})
    )
    .is_err());
    let native = serde_json::json!({"kind":"native","native_ref":" source:placement "});
    let decoded = serde_json::from_value::<CoilPlacement>(native.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), native);
}

#[test]
fn datum_and_support_plane_frames_preserve_nonunit_geometry_and_wire_fields() {
    use crate::features::{
        DatumPlaneReference, FeatureDatumPlaneFrame, FeatureDefinition, FeatureSupportPlaneFrame,
    };
    let origin = Point3::new(1.0, 2.0, 3.0);
    let normal = Vector3::new(0.0, 0.0, 2.0);
    let u_axis = Vector3::new(-3.0, 0.0, 0.0);
    let datum = FeatureDatumPlaneFrame::new(origin, normal, u_axis).unwrap();
    let support = FeatureSupportPlaneFrame::new(origin, normal, u_axis).unwrap();
    let geometry = serde_json::json!({"origin":origin,"normal":normal,"u_axis":u_axis});
    assert_eq!(serde_json::to_value(datum).unwrap(), geometry);
    assert_eq!(serde_json::to_value(support).unwrap(), geometry);
    let mut datum_wire = geometry.clone();
    datum_wire["definition"] = serde_json::json!("datum_plane");
    let definition = FeatureDefinition::DatumPlane { frame: datum };
    assert_eq!(serde_json::to_value(&definition).unwrap(), datum_wire);
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(datum_wire).unwrap(),
        definition
    );
    let resolved = DatumPlaneReference::ResolvedPlane { frame: support };
    assert_eq!(
        serde_json::from_value::<DatumPlaneReference>(geometry.clone()).unwrap(),
        resolved
    );
    let mut legacy = geometry;
    legacy["face"] = serde_json::json!({"kind":"unresolved"});
    assert_eq!(serde_json::to_value(&resolved).unwrap(), legacy);
    assert_eq!(
        serde_json::from_value::<DatumPlaneReference>(legacy).unwrap(),
        resolved
    );
}

#[test]
fn plane_frame_admission_rejects_nonfinite_degenerate_and_nonorthogonal_directions() {
    use crate::features::{FeatureDatumPlaneFrame, FeatureSupportPlaneFrame};
    let origin = Point3::new(0.0, 0.0, 0.0);
    let normal = Vector3::new(0.0, 0.0, 2.0);
    for u_axis in [Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 1.0)] {
        assert!(FeatureDatumPlaneFrame::new(origin, normal, u_axis).is_none());
        assert!(FeatureSupportPlaneFrame::new(origin, normal, u_axis).is_none());
        let wire = serde_json::json!({"origin":origin,"normal":normal,"u_axis":u_axis});
        assert!(serde_json::from_value::<FeatureDatumPlaneFrame>(wire.clone()).is_err());
        assert!(serde_json::from_value::<FeatureSupportPlaneFrame>(wire).is_err());
    }
    let u_axis = Vector3::new(3.0, 0.0, 0.0);
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            FeatureDatumPlaneFrame::new(Point3::new(value, 0.0, 0.0), normal, u_axis).is_none()
        );
        assert!(
            FeatureSupportPlaneFrame::new(origin, Vector3::new(0.0, 0.0, value), u_axis).is_none()
        );
    }
    let small = f64::EPSILON / 2.0;
    assert!(FeatureDatumPlaneFrame::new(origin, Vector3::new(0.0, 0.0, small), u_axis).is_some());
    assert!(FeatureSupportPlaneFrame::new(origin, normal, Vector3::new(small, 0.0, 0.0)).is_some());
}

#[test]
fn plane_frame_owners_preserve_their_distinct_floating_point_thresholds() {
    use crate::features::{
        FeatureDatumPlaneFrame, FeatureSupportPlaneFrame, EPS_FEATURE_PLANE_ORTHOGONAL,
    };
    let origin = Point3::new(0.0, 0.0, 0.0);
    let normal = Vector3::new(0.0, 0.0, 0.1);
    let u_axis = Vector3::new(0.7, 0.0, EPS_FEATURE_PLANE_ORTHOGONAL * 0.7);
    let dot = normal.dot(u_axis).abs();
    let datum_bound = EPS_FEATURE_PLANE_ORTHOGONAL * (normal.norm() * u_axis.norm());
    let support_bound = (EPS_FEATURE_PLANE_ORTHOGONAL * normal.norm()) * u_axis.norm();
    assert!(dot > datum_bound);
    assert!(dot <= support_bound);
    assert!(FeatureDatumPlaneFrame::new(origin, normal, u_axis).is_none());
    assert!(FeatureSupportPlaneFrame::new(origin, normal, u_axis).is_some());
}

#[test]
fn resolved_plane_serde_checks_used_geometry_and_preserves_ignored_legacy_geometry() {
    use crate::features::{DatumPlaneReference, FaceSelection};
    let geometry = serde_json::json!({
        "origin":{"x":0.0,"y":0.0,"z":0.0},
        "normal":{"x":0.0,"y":0.0,"z":0.0},
        "u_axis":{"x":1.0,"y":0.0,"z":0.0}
    });
    assert!(serde_json::from_value::<DatumPlaneReference>(geometry.clone()).is_err());
    let mut legacy = geometry;
    legacy["face"] = serde_json::json!({"kind":"unresolved"});
    assert!(serde_json::from_value::<DatumPlaneReference>(legacy.clone()).is_err());
    legacy["face"] = serde_json::json!({"kind":"native","value":"face:retained"});
    assert_eq!(
        serde_json::from_value::<DatumPlaneReference>(legacy).unwrap(),
        DatumPlaneReference::Face(FaceSelection::Native("face:retained".into()))
    );
}

mod edge_treatments;
mod patterns;

mod selections;

mod parameters;

mod configuration_states;

mod source_content;

mod profile_selections;
