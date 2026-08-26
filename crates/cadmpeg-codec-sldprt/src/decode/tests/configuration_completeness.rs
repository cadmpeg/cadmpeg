// SPDX-License-Identifier: Apache-2.0
//! Configuration snapshot design-completeness tests.
#![allow(clippy::unwrap_used)]

use super::super::*;
use cadmpeg_ir::features::{
    Angle, BodyRetentionMode, BodySelection, BooleanOp, ConfigurationFeatureState, ConfigurationId,
    DesignConfiguration, DesignParameter, FaceSelection, Feature, FeatureDefinition, FeatureId,
    FeatureTreeNodeRole, HoleBottom, HoleKind, HolePlacement, Length, ParameterId, ParameterValue,
    PatternKind, PatternSeed, Termination,
};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::DecodeReport;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

#[test]
fn complete_parting_line_draft_does_not_require_an_outward_flag() {
    let faces = FaceSelection::Generated {
        faces: vec![cadmpeg_ir::features::GeneratedFaceRef {
            feature: FeatureId("producer".into()),
            local_id: "1".into(),
        }],
        native: "native".into(),
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.features.push(Feature {
        id: FeatureId("draft".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Draft {
            faces: faces.clone(),
            neutral_plane: FaceSelection::Unresolved,
            parting_tool: Some(faces),
            pull_direction: Some(Vector3::new(1.0, 0.0, 0.0)),
            pull_plane: None,
            angle: Some(Angle(0.1)),
            outward: None,
        },
        native_ref: None,
    });
    let mut report = DecodeReport {
        format: "sldprt".into(),
        container_only: false,
        geometry_transferred: true,
        coverage: BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses: Vec::new(),
        notes: Vec::new(),
    };

    append_design_losses(&ir, &mut report);

    assert!(report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("typed feature(s) retain native")));

    let FeatureDefinition::Draft {
        neutral_plane,
        parting_tool,
        ..
    } = &mut ir.model.features[0].definition
    else {
        unreachable!();
    };
    *neutral_plane = FaceSelection::Generated {
        faces: vec![cadmpeg_ir::features::GeneratedFaceRef {
            feature: FeatureId("producer".into()),
            local_id: "2".into(),
        }],
        native: "native".into(),
    };
    *parting_tool = None;
    let mut neutral_plane_report = DecodeReport {
        format: "sldprt".into(),
        container_only: false,
        geometry_transferred: true,
        coverage: BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses: Vec::new(),
        notes: Vec::new(),
    };

    append_design_losses(&ir, &mut neutral_plane_report);

    assert!(neutral_plane_report
        .losses
        .iter()
        .any(|loss| loss.message.contains("typed feature(s) retain native")));
}

#[test]
fn configuration_feature_states_drive_design_completeness_accounting() {
    let mut ir = CadIr::empty(Units::default());
    let feature_id = FeatureId("configured".into());
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::from([("Scope".into(), "Body1".into())]),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        },
        native_ref: None,
    });
    for (ordinal, definition) in [
        (
            0,
            FeatureDefinition::Native {
                kind: "Unprojected".into(),
                parameters: BTreeMap::new(),
                properties: BTreeMap::new(),
            },
        ),
        (
            1,
            FeatureDefinition::Combine {
                target: BodySelection::Native("target".into()),
                tools: BodySelection::Native("tools".into()),
                op: BooleanOp::Unresolved,
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
            id: ConfigurationId(format!("configuration-{ordinal}")),
            ordinal,
            active: (ordinal == 0).into(),
            source_index: Some(ordinal),
            name: format!("Configuration {ordinal}").into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::from([(
                feature_id.clone(),
                ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: (ordinal == 0)
                        .then(|| FeatureId("missing-dependency".into()))
                        .into_iter()
                        .collect(),
                    outputs: (ordinal == 0)
                        .then(|| BodyId("missing-output".into()))
                        .into_iter()
                        .collect(),
                    definition,
                },
            )]),
            native_ref: None,
        });
    }
    let mut report = DecodeReport {
        format: "sldprt".into(),
        container_only: false,
        geometry_transferred: true,
        coverage: std::collections::BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses: Vec::new(),
        notes: Vec::new(),
    };

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
fn active_configuration_inherits_late_feature_resolutions() {
    let mut ir = CadIr::empty(Units::default());
    let feature_id = FeatureId("mirror".into());
    let seed = PatternSeed::Feature(FeatureId("seed".into()));
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Pattern {
            seeds: vec![seed.clone()],
            pattern: PatternKind::Mirror {
                plane_origin: Point3::new(1.0, 2.0, 3.0),
                plane_normal: Vector3::new(0.0, 0.0, 1.0),
            },
        },
        native_ref: None,
    });
    let hole_id = FeatureId("hole".into());
    ir.model.features.push(Feature {
        id: hole_id.clone(),
        ordinal: 1,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: None,
            position: None,
            direction: None,
            placements: vec![HolePlacement::Axis {
                origin: Point3::new(1.0, 2.0, 3.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            }],
            kind: HoleKind::Simple,
            exit_kind: None,
            diameter: Some(Length(4.0)),
            extent: Some(Termination::Blind {
                length: Length(12.0),
            }),
            bottom: Some(HoleBottom::Flat),
            taper_angle: None,
            specification: None,
            allow_multi_profile_faces: None,
        },
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("configuration".into()),
        ordinal: 0,
        active: true.into(),
        source_index: Some(0),
        name: "Configuration".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::from([
            (
                feature_id.clone(),
                ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: Vec::new(),
                    outputs: Vec::new(),
                    definition: FeatureDefinition::Pattern {
                        seeds: vec![seed],
                        pattern: PatternKind::Unresolved { form: None },
                    },
                },
            ),
            (
                hole_id.clone(),
                ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: Vec::new(),
                    outputs: Vec::new(),
                    definition: FeatureDefinition::Hole {
                        profile: None,
                        profile_filter: None,
                        face: None,
                        position: None,
                        direction: None,
                        placements: Vec::new(),
                        kind: HoleKind::Simple,
                        exit_kind: None,
                        diameter: None,
                        extent: None,
                        bottom: None,
                        taper_angle: None,
                        specification: None,
                        allow_multi_profile_faces: None,
                    },
                },
            ),
        ]),
        native_ref: None,
    });

    sync_active_configuration_resolutions(&mut ir);

    assert!(matches!(
        ir.model.configurations[0].feature_states[&feature_id].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Mirror { .. },
            ..
        }
    ));
    assert!(matches!(
        &ir.model.configurations[0].feature_states[&hole_id].definition,
        FeatureDefinition::Hole {
            placements,
            diameter: Some(Length(4.0)),
            extent: Some(Termination::Blind {
                length: Length(12.0)
            }),
            bottom: Some(HoleBottom::Flat),
            ..
        } if placements.len() == 1
    ));

    let FeatureDefinition::Hole {
        placements,
        diameter,
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
    placements.clear();
    *diameter = Some(Length(8.0));
    *extent = Some(Termination::ThroughAll);
    *bottom = None;
    sync_active_configuration_resolutions(&mut ir);
    assert!(matches!(
        &ir.model.configurations[0].feature_states[&hole_id].definition,
        FeatureDefinition::Hole {
            placements,
            diameter: Some(Length(8.0)),
            extent: Some(Termination::ThroughAll),
            bottom: None,
            ..
        } if placements.len() == 1
    ));
}

#[test]
fn incomplete_configuration_snapshots_are_reported_as_design_losses() {
    let mut ir = CadIr::empty(Units::default());
    let feature_id = FeatureId("feature".into());
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        },
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId("parameter".into()),
        owner: Some(feature_id),
        ordinal: 0,
        name: "D1".into(),
        expression: "1".into(),
        display: None,
        value: Some(ParameterValue::Integer(1)),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId("unevaluated-parameter".into()),
        owner: None,
        ordinal: 1,
        name: "Text".into(),
        expression: "native text".into(),
        display: None,
        value: None,
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("configuration".into()),
        ordinal: 0,
        active: true.into(),
        source_index: Some(0),
        name: "Configuration".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    let mut report = DecodeReport {
        format: "sldprt".into(),
        container_only: false,
        geometry_transferred: true,
        coverage: std::collections::BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses: Vec::new(),
        notes: Vec::new(),
    };

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 configuration(s) lack a complete evaluated feature snapshot; 1 configuration(s) lack a complete evaluated parameter snapshot."
    }));

    ir.source = Some(cadmpeg_ir::document::SourceMeta {
        format: "sldprt".into(),
        attributes: BTreeMap::from([("sw_configuration_0_needs_update".into(), "YES".into())]),
    });
    report.losses.clear();
    append_design_losses(&ir, &mut report);
    assert!(!report
        .losses
        .iter()
        .any(|loss| { loss.message.contains("complete evaluated feature snapshot") }));
}

#[test]
fn active_configuration_snapshots_final_neutral_design_state() {
    let mut ir = CadIr::empty(Units::default());
    let feature_id = FeatureId("feature".into());
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(true),
        parent: None,
        dependencies: vec![FeatureId("dependency".into())],
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: vec![BodyId("body".into())],
        definition: FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        },
        native_ref: None,
    });
    let parameter_id = ParameterId("parameter".into());
    ir.model.parameters.push(DesignParameter {
        id: parameter_id.clone(),
        owner: Some(feature_id.clone()),
        ordinal: 0,
        name: "D1".into(),
        expression: "12mm".into(),
        value: Some(ParameterValue::Length(Length(12.0))),
        dependencies: Vec::new(),
        display: None,
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    for (ordinal, active) in [(0, true), (1, false)] {
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId(format!("configuration-{ordinal}")),
            ordinal,
            active: active.into(),
            source_index: Some(ordinal),
            name: format!("Configuration {ordinal}").into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        });
    }

    snapshot_active_configuration(&mut ir);

    assert_eq!(
        ir.model.configurations[0].parameter_values[&parameter_id],
        ParameterValue::Length(Length(12.0))
    );
    assert_eq!(
        ir.model.configurations[0].feature_states[&feature_id],
        ConfigurationFeatureState {
            suppressed: true,
            dependencies: vec![FeatureId("dependency".into())],
            outputs: vec![BodyId("body".into())],
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
        }
    );
    assert!(ir.model.configurations[1].parameter_values.is_empty());
    assert!(ir.model.configurations[1].feature_states.is_empty());

    ir.model.configurations[0]
        .parameter_values
        .insert(parameter_id.clone(), ParameterValue::Length(Length(25.0)));
    ir.model.configurations[0]
        .feature_states
        .get_mut(&feature_id)
        .expect("active feature state")
        .suppressed = false;
    snapshot_active_configuration(&mut ir);
    assert_eq!(
        ir.model.configurations[0].parameter_values[&parameter_id],
        ParameterValue::Length(Length(25.0))
    );
    assert!(!ir.model.configurations[0].feature_states[&feature_id].suppressed);
}

#[test]
fn resolved_configuration_snapshots_inherit_only_independent_parameter_values() {
    let mut ir = CadIr::empty(Units::default());
    let independent = ParameterId("independent".into());
    let overridden = ParameterId("overridden".into());
    let dependent = ParameterId("dependent".into());
    let parameter = |id: ParameterId, value, dependencies| DesignParameter {
        id,
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "12mm".into(),
        value: Some(value),
        dependencies,
        display: None,
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    ir.model.parameters = vec![
        parameter(
            independent.clone(),
            ParameterValue::Length(Length(12.0)),
            Vec::new(),
        ),
        parameter(
            overridden.clone(),
            ParameterValue::Length(Length(20.0)),
            Vec::new(),
        ),
        parameter(
            dependent.clone(),
            ParameterValue::Length(Length(24.0)),
            vec![independent.clone()],
        ),
    ];
    let configuration = |id: &str, parameter_values| DesignConfiguration {
        id: ConfigurationId(id.into()),
        ordinal: 0,
        active: false.into(),
        source_index: Some(0),
        name: id.into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
        parameter_values,
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    };
    ir.model.configurations = vec![
        configuration(
            "resolved",
            BTreeMap::from([(overridden.clone(), ParameterValue::Length(Length(25.0)))]),
        ),
        configuration("unresolved", BTreeMap::new()),
    ];

    complete_resolved_configuration_parameter_snapshots(&mut ir);

    assert_eq!(
        ir.model.configurations[0].parameter_values,
        BTreeMap::from([
            (independent, ParameterValue::Length(Length(12.0))),
            (overridden, ParameterValue::Length(Length(25.0))),
        ])
    );
    assert!(!ir.model.configurations[0]
        .parameter_values
        .contains_key(&dependent));
    assert!(ir.model.configurations[1].parameter_values.is_empty());
}
