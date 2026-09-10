// SPDX-License-Identifier: Apache-2.0
//! Configuration snapshot design-completeness tests.
#![allow(clippy::unwrap_used)]

use super::super::*;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::CadIr;
use cadmpeg_ir::{
    features::{
        BodyRetentionMode, BodySelection, ConfigurationFeatureState, ConfigurationId,
        DesignConfiguration, DesignParameter, FaceSelection, Feature, FeatureDefinition, FeatureId,
        FeatureTreeNodeRole, HoleBottom, HoleKind, HolePlacement, LinearTermination, ParameterId,
        ParameterValue, PatternKind, PatternSeed, PatternTransform,
    },
    scalar::Length,
};
use std::collections::BTreeMap;

#[test]
fn complete_parting_line_draft_does_not_require_an_outward_flag() {
    let faces = FaceSelection::generated(
        vec![cadmpeg_ir::features::GeneratedFaceRef::new(
            FeatureId::mint("synthetic:test:id#producer").expect("identity grammar"),
            "1".into(),
        )
        .unwrap()],
        "native".into(),
    )
    .unwrap();
    let mut ir = CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#draft").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Draft {
                faces: faces.clone(),
                anchor: cadmpeg_ir::features::DraftAnchor::PartingLine {
                    tool: faces,
                    pull: cadmpeg_ir::features::DraftPull {
                        direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                            1.0, 0.0, 0.0,
                        ))
                        .unwrap(),
                        plane: None,
                    },
                },
                angle: Some(cadmpeg_ir::scalar::SlopeAngle::new(0.1).unwrap()),
                outward: None,
            },
        ),
        native_ref: None,
    });
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("typed feature(s) retain native")));

    ir.model.features[0]
        .evaluation
        .try_edit(|definition, _| {
            let FeatureDefinition::Draft { anchor, .. } = definition else {
                unreachable!();
            };
            *anchor = cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                plane: FaceSelection::generated(
                    vec![cadmpeg_ir::features::GeneratedFaceRef::new(
                        FeatureId::mint("synthetic:test:id#producer").expect("identity grammar"),
                        "2".into(),
                    )
                    .unwrap()],
                    "native".into(),
                )
                .unwrap(),
                pull: Some(cadmpeg_ir::features::DraftPull {
                    direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                        1.0, 0.0, 0.0,
                    ))
                    .unwrap(),
                    plane: None,
                }),
            };
        })
        .unwrap();
    let mut neutral_plane_report = super::empty_report(true);

    append_design_losses(&ir, &mut neutral_plane_report);

    assert!(neutral_plane_report
        .losses
        .iter()
        .any(|loss| loss.message.contains("typed feature(s) retain native")));
}

#[test]
fn configuration_feature_states_drive_design_completeness_accounting() {
    let mut ir = CadIr::empty();
    let feature_id = FeatureId::mint("synthetic:test:id#configured").expect("identity grammar");
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::from([("Scope".into(), "Body1".into())]),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: cadmpeg_ir::features::TreeChildren::default(),
            },
        ),
        native_ref: None,
    });
    for (ordinal, definition) in [
        (
            0,
            FeatureDefinition::Native {
                kind: "Unprojected".into(),
                parameters: BTreeMap::new(),
            },
        ),
        (
            1,
            FeatureDefinition::Combine {
                operands: cadmpeg_ir::features::CombineOperands::new(
                    BodySelection::Native("target".into()),
                    BodySelection::Native("tools".into()),
                )
                .unwrap(),

                op: cadmpeg_ir::features::BooleanKind::Join,
                keep_tools: false,
            },
        ),
        (
            2,
            FeatureDefinition::DeleteBody {
                bodies: BodySelection::Native("bodies".into()),
                mode: BodyRetentionMode::Unresolved,
            },
        ),
    ] {
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId::mint(format!("synthetic:test:id#configuration-{ordinal}"))
                .expect("identity grammar"),
            ordinal,
            active: ordinal == 0,
            source_index: Some(ordinal),
            name: format!("Configuration {ordinal}").into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(
                cadmpeg_ir::features::DistinctMembers::default(),
            ),
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::from([(
                feature_id.clone(),
                ConfigurationFeatureState {
                    evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Active {
                        outputs: ((ordinal == 0)
                            .then(|| {
                                BodyId::mint("test:model:entity#missing-output")
                                    .expect("identity grammar")
                            })
                            .into_iter()
                            .collect::<Vec<_>>())
                        .try_into()
                        .unwrap(),
                    },
                    dependencies: ((ordinal == 0)
                        .then(|| {
                            FeatureId::mint("synthetic:test:id#missing-dependency")
                                .expect("identity grammar")
                        })
                        .into_iter()
                        .collect::<Vec<_>>())
                    .try_into()
                    .unwrap(),
                    definition,
                },
            )]),
            native_ref: None,
        });
    }
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    for expected in [
        "1 feature record(s) contain missing, repeated, or non-preceding parent/dependency edges; 0 feature record(s) share regeneration ordinals.",
        "2 feature(s) retain non-empty native output scopes that do not resolve to model bodies.",
        "1 feature record(s) contain missing or repeated output body references.",
        "1 feature(s) retain their native kind without a complete neutral operation definition.",
        "2 typed feature(s) retain native or unresolved required operation operands.",
        "1 body delete/keep feature(s) retain selected native body identities without a decoded retention mode.",
    ] {
        assert!(report.losses.iter().any(|loss| loss.message == expected));
    }
}

#[test]
fn metadata_only_native_feature_does_not_report_missing_operation() {
    let mut ir = CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#metadata-only").expect("identity grammar"),
        ordinal: 0,
        name: Some("Localized tree item".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some("Feature".into()),
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Native {
                kind: "Localized tree item".into(),
                parameters: BTreeMap::new(),
            },
        ),
        native_ref: None,
    });
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("retain their native kind")));
}

#[test]
fn active_configuration_inherits_late_feature_resolutions() {
    let mut ir = CadIr::empty();
    let feature_id = FeatureId::mint("synthetic:test:id#mirror").expect("identity grammar");
    let seed =
        PatternSeed::Feature(FeatureId::mint("synthetic:test:id#seed").expect("identity grammar"));
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Pattern {
                seeds: vec![seed.clone()],
                pattern: PatternKind::new(PatternTransform::Mirror {
                    plane_origin: Point3::new(1.0, 2.0, 3.0),
                    plane_normal: Vector3::new(0.0, 0.0, 1.0),
                })
                .unwrap(),
            },
        ),
        native_ref: None,
    });
    let hole_id = FeatureId::mint("synthetic:test:id#hole").expect("identity grammar");
    ir.model.features.push(Feature {
        id: hole_id.clone(),
        ordinal: 1,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Hole {
                profile: None,
                profile_filter: None,
                face: None,
                direction: None,
                placements: Some(vec![HolePlacement::Axis {
                    origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
                        .unwrap(),
                    axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                        .unwrap(),
                }]),
                shape: cadmpeg_ir::features::HoleShape::new(
                    cadmpeg_ir::features::HoleConstruction::form(HoleKind::Simple),
                    None,
                    Some(cadmpeg_ir::scalar::PositiveLength::new(4.0).unwrap()),
                )
                .unwrap(),

                extent: Some(LinearTermination::Blind {
                    length: cadmpeg_ir::scalar::NonZeroLength::new(12.0).unwrap(),
                }),
                bottom: Some(HoleBottom::Flat),
                taper_angle: None,
                allow_multi_profile_faces: None,
            },
        ),
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#configuration").expect("identity grammar"),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: "Configuration".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: cadmpeg_ir::ConfigurationBodies::Resolved(
            cadmpeg_ir::features::DistinctMembers::default(),
        ),
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::from([
            (
                feature_id.clone(),
                ConfigurationFeatureState {
                    evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Active {
                        outputs: cadmpeg_ir::features::DistinctMembers::default(),
                    },
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    definition: FeatureDefinition::Pattern {
                        seeds: vec![seed],
                        pattern: PatternKind::UNRESOLVED,
                    },
                },
            ),
            (
                hole_id.clone(),
                ConfigurationFeatureState {
                    evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Active {
                        outputs: cadmpeg_ir::features::DistinctMembers::default(),
                    },
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    definition: FeatureDefinition::Hole {
                        profile: None,
                        profile_filter: None,
                        face: None,
                        direction: None,
                        placements: None,
                        shape: cadmpeg_ir::features::HoleShape::new(
                            cadmpeg_ir::features::HoleConstruction::form(HoleKind::Simple),
                            None,
                            None,
                        )
                        .unwrap(),

                        extent: None,
                        bottom: None,
                        taper_angle: None,
                        allow_multi_profile_faces: None,
                    },
                },
            ),
        ]),
        native_ref: None,
    });

    sync_active_configuration_resolutions(&mut ir).unwrap();

    assert!(
        matches!(&(ir.model.configurations[0].feature_states[&feature_id].definition),
            FeatureDefinition::Pattern {
                pattern: admitted_pattern,
                ..
            } if matches!(admitted_pattern.definition(), PatternTransform::Mirror { .. })
        )
    );
    assert!(matches!(
        &ir.model.configurations[0].feature_states[&hole_id].definition, FeatureDefinition::Hole {
            placements,
            shape,
            extent: Some(LinearTermination::Blind {
                length: actual_length
            }),
            bottom: Some(HoleBottom::Flat),
            ..
        } if matches!((&shape.diameter(),), (Some(actual_diameter),) if (placements.as_ref().is_some_and(|placements| placements.len() == 1)) && actual_diameter.get() == 4.0 && actual_length.get() == 12.0)));

    let FeatureDefinition::Hole {
        placements,
        shape,
        extent,
        bottom,
        ..
    } = &mut ir.model.configurations[0]
        .feature_states
        .get_mut(&hole_id)
        .expect("hole state")
        .definition
    else {
        unreachable!();
    };
    let mut edited_diameter = shape.diameter();
    let diameter = &mut edited_diameter;
    *placements = None;
    *diameter = Some(cadmpeg_ir::scalar::PositiveLength::new(8.0).unwrap());
    *extent = Some(LinearTermination::ThroughAll {});
    *bottom = None;

    *shape = cadmpeg_ir::features::HoleShape::new(
        shape.construction().clone(),
        *shape.exit_kind(),
        edited_diameter,
    )
    .unwrap();
    sync_active_configuration_resolutions(&mut ir).unwrap();
    assert!(matches!(
        &ir.model.configurations[0].feature_states[&hole_id].definition, FeatureDefinition::Hole {
            placements,
            shape,
            extent: Some(LinearTermination::ThroughAll {}),
            bottom: None,
            ..
        } if matches!((&shape.diameter(),), (Some(actual_diameter),) if (placements.as_ref().is_some_and(|placements| placements.len() == 1)) && actual_diameter.get() == 8.0)));
}

#[test]
fn incomplete_configuration_snapshots_are_reported_as_design_losses() {
    let mut ir = CadIr::empty();
    let feature_id = FeatureId::mint("synthetic:test:id#feature").expect("identity grammar");
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: cadmpeg_ir::features::TreeChildren::default(),
            },
        ),
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
        owner: Some(feature_id),
        ordinal: 0,
        name: "D1".into(),
        expression: "1".into(),
        display: None,
        value: Some(ParameterValue::Integer(1)),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("synthetic:test:id#unevaluated-parameter").expect("identity grammar"),
        owner: None,
        ordinal: 1,
        name: "Text".into(),
        expression: "native text".into(),
        display: None,
        value: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#configuration").expect("identity grammar"),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: "Configuration".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: cadmpeg_ir::ConfigurationBodies::Resolved(
            cadmpeg_ir::features::DistinctMembers::default(),
        ),
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 configuration(s) lack a complete evaluated feature snapshot; 1 configuration(s) lack a complete evaluated parameter snapshot."
    }));

    ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect::DialectId::pinned("sldprt:test"),
        )),
        BTreeMap::from([("sw_configuration_0_needs_update".into(), "YES".into())]),
    ));
    report.losses.clear();
    append_design_losses(&ir, &mut report);
    assert!(!report
        .losses
        .iter()
        .any(|loss| { loss.message.contains("complete evaluated feature snapshot") }));
}

#[test]
fn active_configuration_snapshots_final_neutral_design_state() {
    let mut ir = CadIr::empty();
    let feature_id = FeatureId::mint("synthetic:test:id#feature").expect("identity grammar");
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(true),
        dependencies: (vec![
            FeatureId::mint("synthetic:test:id#dependency").expect("identity grammar")
        ])
        .try_into()
        .unwrap(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: cadmpeg_ir::features::TreeChildren::default(),
            },
            vec![BodyId::mint("test:model:entity#body").expect("identity grammar")],
        )
        .unwrap(),
        native_ref: None,
    });
    let parameter_id = ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar");
    ir.model.parameters.push(DesignParameter {
        id: parameter_id.clone(),
        owner: Some(feature_id.clone()),
        ordinal: 0,
        name: "D1".into(),
        expression: "12mm".into(),
        value: Some(ParameterValue::Length(Length::new(12.0).unwrap())),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        display: None,
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    for (ordinal, active) in [(0, true), (1, false)] {
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId::mint(format!("synthetic:test:id#configuration-{ordinal}"))
                .expect("identity grammar"),
            ordinal,
            active,
            source_index: Some(ordinal),
            name: format!("Configuration {ordinal}").into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(
                cadmpeg_ir::features::DistinctMembers::default(),
            ),
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        });
    }

    snapshot_active_configuration(&mut ir);

    assert_eq!(
        ir.model.configurations[0].parameter_values[&parameter_id],
        ParameterValue::Length(Length::new(12.0).unwrap())
    );
    assert_eq!(
        ir.model.configurations[0].feature_states[&feature_id],
        ConfigurationFeatureState {
            evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Suppressed,
            dependencies: (vec![
                FeatureId::mint("synthetic:test:id#dependency").expect("identity grammar")
            ])
            .try_into()
            .unwrap(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: cadmpeg_ir::features::TreeChildren::default(),
            },
        }
    );
    assert!(ir.model.configurations[1].parameter_values.is_empty());
    assert!(ir.model.configurations[1].feature_states.is_empty());

    ir.model.configurations[0].parameter_values.insert(
        parameter_id.clone(),
        ParameterValue::Length(Length::new(25.0).unwrap()),
    );
    ir.model.configurations[0]
        .feature_states
        .get_mut(&feature_id)
        .expect("active feature state")
        .evaluation = cadmpeg_ir::features::ConfigurationEvaluation::Active {
        outputs: cadmpeg_ir::features::DistinctMembers::default(),
    };
    snapshot_active_configuration(&mut ir);
    assert_eq!(
        ir.model.configurations[0].parameter_values[&parameter_id],
        ParameterValue::Length(Length::new(25.0).unwrap())
    );
    assert!(!ir.model.configurations[0].feature_states[&feature_id]
        .evaluation
        .is_suppressed());
}

#[test]
fn resolved_configuration_snapshots_inherit_only_independent_parameter_values() {
    let mut ir = CadIr::empty();
    let independent = ParameterId::mint("synthetic:test:id#independent").expect("identity grammar");
    let overridden = ParameterId::mint("synthetic:test:id#overridden").expect("identity grammar");
    let dependent = ParameterId::mint("synthetic:test:id#dependent").expect("identity grammar");
    let parameter = |id: ParameterId, value, dependencies: Vec<ParameterId>| DesignParameter {
        id,
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "12mm".into(),
        value: Some(value),
        dependencies: (dependencies).try_into().unwrap(),
        display: None,
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    ir.model.parameters = vec![
        parameter(
            independent.clone(),
            ParameterValue::Length(Length::new(12.0).unwrap()),
            Vec::new(),
        ),
        parameter(
            overridden.clone(),
            ParameterValue::Length(Length::new(20.0).unwrap()),
            Vec::new(),
        ),
        parameter(
            dependent.clone(),
            ParameterValue::Length(Length::new(24.0).unwrap()),
            vec![independent.clone()],
        ),
    ];
    let configuration = |id: &str, parameter_values| DesignConfiguration {
        id: ConfigurationId::mint(id).expect("identity grammar"),
        ordinal: 0,
        active: false,
        source_index: Some(0),
        name: id.into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: cadmpeg_ir::ConfigurationBodies::Resolved(
            cadmpeg_ir::features::DistinctMembers::default(),
        ),
        parameter_values,
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    };
    ir.model.configurations = vec![
        configuration(
            "synthetic:test:id#resolved",
            BTreeMap::from([(
                overridden.clone(),
                ParameterValue::Length(Length::new(25.0).unwrap()),
            )]),
        ),
        configuration("synthetic:test:id#unresolved", BTreeMap::new()),
    ];

    complete_resolved_configuration_parameter_snapshots(&mut ir);

    assert_eq!(
        ir.model.configurations[0].parameter_values,
        BTreeMap::from([
            (
                independent,
                ParameterValue::Length(Length::new(12.0).unwrap())
            ),
            (
                overridden,
                ParameterValue::Length(Length::new(25.0).unwrap())
            ),
        ])
    );
    assert!(!ir.model.configurations[0]
        .parameter_values
        .contains_key(&dependent));
    assert!(ir.model.configurations[1].parameter_values.is_empty());
}
