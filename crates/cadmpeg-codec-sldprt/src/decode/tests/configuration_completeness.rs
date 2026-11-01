// SPDX-License-Identifier: Apache-2.0
//! Configuration snapshot design-completeness tests.
#![allow(clippy::unwrap_used)]

use crate::decode::append_design_losses;
use crate::history::configuration::normalize_configuration_differences;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::CadIr;
use cadmpeg_ir::{
    features::{
        holes::{HoleBottom, HoleKind, HolePlacement},
        patterns::{PatternKind, PatternSeed, PatternTransform},
        BodyRetentionMode, BodySelection, ConfigurationFeatureState, ConfigurationId,
        DesignConfiguration, DesignParameter, FaceSelection, Feature, FeatureDefinition, FeatureId,
        FeatureOperation, FeatureTreeNodeRole, LinearTermination, ParameterId, ParameterValue,
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
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#draft").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        needs_recompute: false,
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Draft {
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
                angle: Some(cadmpeg_ir::Dimension::Literal(
                    cadmpeg_ir::scalar::SlopeAngle::new(0.1).unwrap(),
                )),
                outward: None,
            }),
        ),
        native_ref: None,
    });
    let mut report = super::empty_report(true);

    append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .unwrap();

    assert!(report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("typed feature(s) retain native")));

    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Draft { anchor, .. }) = definition
        else {
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
    });
    let mut neutral_plane_report = super::empty_report(true);

    append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &mut neutral_plane_report,
    )
    .unwrap();

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
        product_definition: None,
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("Scope"),
            "Body1".into(),
        )]),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        needs_recompute: false,
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: cadmpeg_ir::features::TreeChildren::default(),
            }),
        ),
        native_ref: None,
    });
    for (ordinal, definition) in [
        (
            0,
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Unprojected".into(),
                parameters: BTreeMap::new(),
            }),
        ),
        (
            1,
            FeatureDefinition::Operation(FeatureOperation::Combine {
                operands: cadmpeg_ir::features::CombineOperands::new(
                    BodySelection::Native("target".into()),
                    BodySelection::Native("tools".into()),
                )
                .unwrap(),
                op: cadmpeg_ir::features::BooleanKind::Join,
                keep_tools: false,
            }),
        ),
        (
            2,
            FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                bodies: BodySelection::Native("bodies".into()),
                mode: BodyRetentionMode::Unresolved,
            }),
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
            bodies: Some(cadmpeg_ir::features::DistinctMembers::default()),
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::from([(
                feature_id.clone(),
                ConfigurationFeatureState::Definition {
                    definition: Box::new(definition),
                },
            )]),
            native_ref: None,
        });
    }
    let mut report = super::empty_report(true);

    append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .unwrap();

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
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#metadata-only").expect("identity grammar"),
        ordinal: 0,
        name: Some("Localized tree item".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some("Feature".into()),
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        needs_recompute: false,
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Localized tree item".into(),
                parameters: BTreeMap::new(),
            }),
        ),
        native_ref: None,
    });
    let mut report = super::empty_report(true);

    append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .unwrap();

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
        product_definition: None,
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        needs_recompute: false,
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: vec![seed.clone()],
                pattern: PatternKind::new(PatternTransform::Mirror {
                    plane_origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                        1.0, 2.0, 3.0,
                    ))
                    .unwrap(),
                    plane_normal: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                        0.0, 0.0, 1.0,
                    ))
                    .unwrap(),
                })
                .unwrap(),
            }),
        ),
        native_ref: None,
    });
    let hole_id = FeatureId::mint("synthetic:test:id#hole").expect("identity grammar");
    ir.model.features.push(Feature {
        product_definition: None,
        id: hole_id.clone(),
        ordinal: 1,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        needs_recompute: false,
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Hole {
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
                shape: cadmpeg_ir::features::holes::HoleShape::new(
                    cadmpeg_ir::features::holes::HoleConstruction::form(HoleKind::Simple),
                    None,
                    Some(cadmpeg_ir::Dimension::Literal(
                        cadmpeg_ir::scalar::PositiveLength::new(4.0).unwrap(),
                    )),
                )
                .unwrap(),
                extent: Some(LinearTermination::Blind {
                    length: cadmpeg_ir::Dimension::Literal(
                        cadmpeg_ir::scalar::NonZeroLength::new(12.0).unwrap(),
                    ),
                }),
                bottom: Some(HoleBottom::Flat),
                taper_angle: None,
                allow_multi_profile_faces: None,
            }),
        ),
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#configuration").expect("identity grammar"),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: Some("Configuration".to_string()),
        material: None,
        properties: BTreeMap::new(),
        bodies: Some(cadmpeg_ir::features::DistinctMembers::default()),
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::from([
            (
                feature_id.clone(),
                ConfigurationFeatureState::Definition {
                    definition: Box::new(FeatureDefinition::Operation(FeatureOperation::Pattern {
                        seeds: vec![seed],
                        pattern: PatternKind::UNRESOLVED,
                    })),
                },
            ),
            (
                hole_id.clone(),
                ConfigurationFeatureState::Definition {
                    definition: Box::new(FeatureDefinition::Operation(FeatureOperation::Hole {
                        profile: None,
                        profile_filter: None,
                        face: None,
                        direction: None,
                        placements: None,
                        shape: cadmpeg_ir::features::holes::HoleShape::new(
                            cadmpeg_ir::features::holes::HoleConstruction::form(HoleKind::Simple),
                            None,
                            None,
                        )
                        .unwrap(),
                        extent: None,
                        bottom: None,
                        taper_angle: None,
                        allow_multi_profile_faces: None,
                    })),
                },
            ),
        ]),
        native_ref: None,
    });

    normalize_configuration_differences(&cadmpeg_test_support::service_decode_context(), &mut ir)
        .unwrap();
    assert!(ir.model.configurations[0].feature_states.is_empty());
    assert!(
        matches!(ir.model.features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) if matches!(pattern.definition(), PatternTransform::Mirror { .. }))
    );
    assert!(
        matches!(ir.model.features[1].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole { placements, shape, extent: Some(LinearTermination::Blind { length }), bottom: Some(HoleBottom::Flat), .. }) if placements.as_ref().is_some_and(|placements| placements.len() == 1) && ir.dimension_value(shape.diameter().unwrap()).unwrap().get() == 4.0 && ir.dimension_value(length).unwrap().get() == 12.0)
    );
    let id = ir.model.features[1].id.clone();
    ir.set_dimension(
        &id,
        "/shape/diameter",
        cadmpeg_ir::scalar::PositiveLength::new(8.0).unwrap(),
    )
    .unwrap();
    assert!(ir.model.features[1].needs_recompute);
    assert!(ir.model.configurations[0].feature_states.is_empty());
    assert!(
        matches!(ir.model.features[1].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole { shape, .. }) if ir.dimension_value(shape.diameter().unwrap()).unwrap().get() == 8.0)
    );
}

#[test]
fn sparse_configuration_maps_do_not_imply_missing_source_content() {
    let mut ir = CadIr::empty();
    let feature_id = FeatureId::mint("synthetic:test:id#feature").expect("identity grammar");
    ir.model.features.push(Feature {
        product_definition: None,
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        needs_recompute: false,
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: cadmpeg_ir::features::TreeChildren::default(),
            }),
        ),
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        driving: true,
        id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
        owner: Some(feature_id),
        ordinal: 0,
        name: "D1".into(),
        expression: "1".into(),
        display: None,
        value: Some(ParameterValue::Integer(1)),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        driving: true,
        id: ParameterId::mint("synthetic:test:id#unevaluated-parameter").expect("identity grammar"),
        owner: None,
        ordinal: 1,
        name: "Text".into(),
        expression: "native text".into(),
        display: None,
        value: None,
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#configuration").expect("identity grammar"),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: Some("Configuration".to_string()),
        material: None,
        properties: BTreeMap::new(),
        bodies: Some(cadmpeg_ir::features::DistinctMembers::default()),
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    let mut report = super::empty_report(true);

    append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .unwrap();

    assert!(!report.losses.iter().any(|loss| loss
        .message
        .contains("complete evaluated feature snapshot")
        || loss
            .message
            .contains("complete evaluated parameter snapshot")));

    ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("sldprt:test"),
        )),
        BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("sw_configuration_0_needs_update"),
            "YES".into(),
        )]),
    ));
    report.losses.clear();
    append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .unwrap();
    assert!(!report
        .losses
        .iter()
        .any(|loss| { loss.message.contains("complete evaluated feature snapshot") }));
}

#[test]
fn active_configuration_uses_only_the_base_neutral_design_state() {
    let mut ir = CadIr::empty();
    let feature_id = FeatureId::mint("synthetic:test:id#feature").expect("identity grammar");
    ir.model.features.push(Feature {
        product_definition: None,
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(true),
        dependencies: cadmpeg_ir::features::DistinctMembers::try_from(vec![FeatureId::mint(
            "synthetic:test:id#dependency",
        )
        .expect("identity grammar")])
        .unwrap(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        needs_recompute: false,
        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: cadmpeg_ir::features::TreeChildren::default(),
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(vec![BodyId::mint(
                "test:model:entity#body",
            )
            .expect("identity grammar")])
            .unwrap(),
        ),
        native_ref: None,
    });
    let parameter_id = ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar");
    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        driving: true,
        id: parameter_id.clone(),
        owner: Some(feature_id.clone()),
        ordinal: 0,
        name: "D1".into(),
        expression: "12mm".into(),
        value: Some(ParameterValue::Length(Length::new(12.0).unwrap())),
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
            bodies: Some(cadmpeg_ir::features::DistinctMembers::default()),
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        });
    }

    normalize_configuration_differences(&cadmpeg_test_support::service_decode_context(), &mut ir)
        .unwrap();
    assert!(ir.model.configurations[0].parameter_values.is_empty());
    assert!(ir.model.configurations[0].parameter_overrides.is_empty());
    assert!(ir.model.configurations[0].feature_states.is_empty());
    assert_eq!(ir.model.features[0].suppressed, Some(true));
    assert_eq!(
        ir.model.parameters[0].value,
        Some(ParameterValue::Length(Length::new(12.0).unwrap()))
    );
    ir.model.configurations[1].parameter_values.insert(
        parameter_id.clone(),
        ParameterValue::Length(Length::new(25.0).unwrap()),
    );
    ir.model.configurations[1]
        .feature_states
        .insert(feature_id.clone(), ConfigurationFeatureState::Active {});
    normalize_configuration_differences(&cadmpeg_test_support::service_decode_context(), &mut ir)
        .unwrap();
    assert!(ir.model.configurations[0].parameter_values.is_empty());
    assert!(ir.model.configurations[0].feature_states.is_empty());
    assert_eq!(
        ir.model.configurations[1].parameter_values[&parameter_id],
        ParameterValue::Length(Length::new(25.0).unwrap())
    );
    assert!(!ir.model.configurations[1].feature_states[&feature_id].is_suppressed());
    assert_eq!(ir.model.features[0].suppressed, Some(true));
    assert_eq!(
        ir.model.parameters[0].value,
        Some(ParameterValue::Length(Length::new(12.0).unwrap()))
    );
}

#[test]
fn sparse_configuration_values_inherit_without_copied_base_caches() {
    let mut ir = CadIr::empty();
    let independent = ParameterId::mint("synthetic:test:id#independent").expect("identity grammar");
    let overridden = ParameterId::mint("synthetic:test:id#overridden").expect("identity grammar");
    let dependent = ParameterId::mint("synthetic:test:id#dependent").expect("identity grammar");
    let parameter = |id: ParameterId,
                     value: cadmpeg_ir::features::ParameterValue,
                     dependencies: Vec<ParameterId>| DesignParameter {
        product_definition: None,
        driving: true,
        id,
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: if let Some(dependency) = dependencies.into_iter().next() {
            cadmpeg_ir::features::Expression {
                tokens: vec![
                    cadmpeg_ir::features::ExpressionToken::Parameter(dependency),
                    cadmpeg_ir::features::ExpressionToken::Text(" * 2".into()),
                ],
            }
        } else {
            value.literal_expression()
        },
        value: Some(value),
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
        name: Some(id.to_string()),
        material: None,
        properties: BTreeMap::new(),
        bodies: Some(cadmpeg_ir::features::DistinctMembers::default()),
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

    normalize_configuration_differences(&cadmpeg_test_support::service_decode_context(), &mut ir)
        .unwrap();

    assert_eq!(
        ir.model.configurations[0].parameter_values,
        BTreeMap::from([(
            overridden.clone(),
            ParameterValue::Length(Length::new(25.0).unwrap())
        )])
    );
    assert!(!ir.model.configurations[0]
        .parameter_values
        .contains_key(&dependent));
    assert!(ir.model.configurations[1].parameter_values.is_empty());
    assert_eq!(
        ir.model.configurations[0].parameter_value(&ir, &independent),
        ir.model.parameters[0].value.as_ref()
    );
    ir.model.configurations[0].parameter_overrides.insert(
        dependent.clone(),
        cadmpeg_ir::features::Expression {
            tokens: vec![
                cadmpeg_ir::features::ExpressionToken::Parameter(overridden),
                cadmpeg_ir::features::ExpressionToken::Text(" * 2".into()),
            ],
        },
    );
    assert_eq!(
        ir.model.configurations[0].parameter_value(&ir, &dependent),
        None
    );
}
