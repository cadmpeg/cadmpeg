// SPDX-License-Identifier: Apache-2.0
//! Design-loss and geometry-report tests for SLDPRT decode.
#![allow(clippy::unwrap_used)]

use super::*;
use crate::container::{Block, CompoundStream, ContainerScan};
use crate::native::SldprtNative;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
    FeatureInputLane, FeatureInputName, FeatureInputRelationBinding, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind, SketchInputLink,
    SketchRelationKind,
};
use cadmpeg_ir::features::{
    Angle, BodyRetentionMode, BodySelection, BooleanOp, ConfigurationFeatureState, ConfigurationId,
    DesignConfiguration, DesignParameter, EdgeSelection, FaceSelection, Feature, FeatureDefinition,
    FeatureId, FeatureSourceContent, FeatureTreeNodeRole, HoleBottom, HoleKind, HolePlacement,
    Length, ParameterId, ParameterPmi, ParameterValue, PathRef, PatternKind, PatternSeed,
    PmiDimensionSubtype, RadiusSpec, RuledSurfaceMode, SurfaceContinuity, Termination,
};
use cadmpeg_ir::ids::{BodyId, EdgeId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::DecodeReport;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchConstraintId, SketchEntity, SketchEntityId, SketchGeometry,
    SketchId, SpatialSketchConstraint, SpatialSketchConstraintDefinition, SpatialSketchEntity,
    SpatialSketchEntityId, SpatialSketchGeometry, SpatialSketchId,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

#[test]
fn site_keys_use_outer_container_identity() {
    let first = Block {
        offset: 100,
        type_id: 0,
        comp_sz: 0,
        uncomp_sz: 0,
        section: Some("Contents/Config-0-Partition".into()),
        family: "parasolid",
        payload: Vec::new(),
        ps_stream: None,
        ps_streams: Vec::new(),
        ps_stream_offsets: Vec::new(),
    };
    let second = Block {
        offset: 200,
        section: first.section.clone(),
        ..first.clone()
    };
    assert_ne!(
        super::BodyOrigin::Block(&first).site_key(),
        super::BodyOrigin::Block(&second).site_key()
    );

    let compound = CompoundStream {
        path: "Contents/Config-0-Partition".into(),
        directory_id: 300,
        start_sector: 0,
        payload: Vec::new(),
        decoded_payload: None,
        ps_streams: Vec::new(),
        ps_stream_offsets: Vec::new(),
    };
    assert_eq!(
        super::BodyOrigin::Compound(&compound).site_key(),
        "compound@300"
    );
}

#[test]
fn sketch_constraint_completeness_distinguishes_neutral_and_native_semantics() {
    assert!(sketch_constraint_has_complete_neutral_semantics(
        &SketchConstraintDefinition::Disabled
    ));
    assert!(!sketch_constraint_has_complete_neutral_semantics(
        &SketchConstraintDefinition::Native {
            native_kind: "unresolved".into(),
            native_state: None,
            native_flags: None,
            native_properties: BTreeMap::new(),
            entities: Vec::new(),
            parameter: None,
            operands: Vec::new(),
        }
    ));
    assert!(spatial_sketch_constraint_has_complete_neutral_semantics(
        &SpatialSketchConstraintDefinition::Coincident {
            first: SpatialSketchEntityId("first".into()),
            second: SpatialSketchEntityId("second".into()),
        }
    ));
    assert!(!spatial_sketch_constraint_has_complete_neutral_semantics(
        &SpatialSketchConstraintDefinition::Native {
            native_kind: "unresolved".into(),
            native_state: None,
            parameter: None,
            operands: Vec::new(),
        }
    ));
}

#[test]
fn native_spatial_sketch_constraints_are_reported_as_design_losses() {
    let mut ir = CadIr::empty(Units::default());
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("native-spatial".into()),
            sketch: SpatialSketchId("spatial-sketch".into()),
            definition: SpatialSketchConstraintDefinition::Native {
                native_kind: "unresolved".into(),
                native_state: None,
                parameter: None,
                operands: Vec::new(),
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

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 planar or spatial sketch constraint(s) retain native relation kinds and operands without complete neutral geometric semantics."
    }));
}

#[test]
fn typed_native_operands_are_reported_as_design_losses() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.features.push(Feature {
        id: FeatureId("combine".into()),
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
        definition: FeatureDefinition::Combine {
            target: BodySelection::Native("target".into()),
            tools: BodySelection::Native("tools".into()),
            op: BooleanOp::Unresolved,
            keep_tools: false,
        },
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
            == "1 typed feature(s) retain native or unresolved required operation operands."
    }));
}

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

#[test]
fn design_completeness_rejects_unresolved_and_unaudited_typed_families() {
    let mut ir = CadIr::empty(Units::default());
    let feature = |id: &str, ordinal, definition| Feature {
        id: FeatureId(id.into()),
        ordinal,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: None,
    };
    ir.model.features.push(feature(
        "complete-helix",
        0,
        FeatureDefinition::Helix {
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_direction: Vector3::new(0.0, 0.0, 1.0),
            radius: Length(1.0),
            pitch: Length(2.0),
            revolutions: 3.0,
            start_angle: Angle(0.0),
            clockwise: false,
            radial_growth: None,
            cone_angle: None,
            segment_turns: None,
            construction_style: None,
        },
    ));
    ir.model.features.push(feature(
        "incomplete-dome",
        1,
        FeatureDefinition::Dome {
            faces: FaceSelection::Native("face".into()),
            height: None,
            elliptical: None,
            reverse: None,
        },
    ));
    ir.model.features.push(feature(
        "unresolved-plane",
        2,
        FeatureDefinition::DatumPlaneUnresolved,
    ));
    ir.model.features.push(feature(
        "unaudited-stored-geometry",
        3,
        FeatureDefinition::StoredGeometry,
    ));
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
            == "3 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn design_completeness_audits_direct_body_and_shape_families() {
    let mut ir = CadIr::empty(Units::default());
    let body = BodyId("body".into());
    let source = FeatureId("base".into());
    let mut push = |id: &str, ordinal, dependencies, outputs, definition| {
        ir.model.features.push(Feature {
            id: FeatureId(id.into()),
            ordinal,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies,
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition,
            native_ref: None,
        });
    };
    push(
        "base",
        0,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![body.clone()]),
        },
    );
    push(
        "stored",
        1,
        Vec::new(),
        vec![body.clone()],
        FeatureDefinition::StoredGeometry,
    );
    push(
        "derived",
        2,
        vec![source.clone()],
        Vec::new(),
        FeatureDefinition::DerivedGeometry { source },
    );
    push(
        "mirror",
        3,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::MirrorShape {
            source: BodySelection::Bodies(vec![body.clone()]),
            plane_origin: Point3::new(0.0, 0.0, 0.0),
            plane_normal: Vector3::new(0.0, 0.0, 1.0),
            plane_reference: Some(FaceSelection::Native("plane".into())),
        },
    );
    push(
        "sew",
        4,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::SewBodies {
            bodies: BodySelection::Bodies(vec![body.clone()]),
            gap_tolerance: None,
        },
    );
    push(
        "trim",
        5,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::TrimBodies {
            targets: BodySelection::Bodies(vec![body.clone()]),
            tools: BodySelection::Bodies(vec![body.clone()]),
            keep: cadmpeg_ir::features::BodyTrimSide::Unresolved,
        },
    );
    push(
        "import",
        6,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::ImportedGeometry {
            path: "  ".into(),
            format: cadmpeg_ir::features::GeometryImportFormat::Step,
        },
    );
    push(
        "section",
        7,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::SectionShape {
            first: BodySelection::Bodies(vec![body.clone()]),
            second: BodySelection::Bodies(vec![body]),
            approximate: None,
        },
    );
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

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "5 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn design_completeness_audits_typed_construction_families() {
    let mut ir = CadIr::empty(Units::default());
    let body = BodyId("body".into());
    let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());
    let face = FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId("face".into())]);
    let definitions = [
        FeatureDefinition::PointGeometry {
            position: Point3::new(0.0, 0.0, 0.0),
        },
        FeatureDefinition::Primitive {
            solid: cadmpeg_ir::features::PrimitiveSolid::Box {
                length: Length(1.0),
                width: Length(2.0),
                height: Length(3.0),
            },
            op: BooleanOp::NewBody,
        },
        FeatureDefinition::SheetMetalBaseFlange {
            profile: cadmpeg_ir::features::ProfileRef::Sketch(sketch),
            thickness: Length(1.0),
            side: cadmpeg_ir::features::SheetMetalThicknessSide::Symmetric,
        },
        FeatureDefinition::Polyline {
            points: vec![Point3::new(0.0, 0.0, 0.0)],
            closed: false,
        },
        FeatureDefinition::Block {
            dimensions: None,
            placement: None,
            op: BooleanOp::Unresolved,
        },
        FeatureDefinition::ProjectOnSurface {
            sources: PathRef::Native("sources".into()),
            support_face: face.clone(),
            direction: Vector3::new(0.0, 0.0, 1.0),
            mode: cadmpeg_ir::features::SurfaceProjectionMode::All,
            height: Length(0.0),
            offset: Length(0.0),
        },
        FeatureDefinition::Coil {
            construction: cadmpeg_ir::features::CoilConstruction {
                placement: cadmpeg_ir::features::CoilPlacement::Native {
                    native_ref: "placement".into(),
                },
                diameter: Length(10.0),
                extent: cadmpeg_ir::features::CoilExtent::RevolutionsHeight {
                    revolutions: 2.0,
                    height: Length(5.0),
                },
                section: cadmpeg_ir::features::CoilSection::Circular {
                    diameter: Length(1.0),
                },
                section_placement: cadmpeg_ir::features::CoilSectionPlacement::Center,
                clockwise: false,
                taper: Angle(0.0),
            },
            result: cadmpeg_ir::features::CoilResult::NewBody,
        },
        FeatureDefinition::Sphere {
            center: Point3::new(0.0, 0.0, 0.0),
            radius: Length(1.0),
            op: BooleanOp::Unresolved,
        },
        FeatureDefinition::FaceBlend {
            first_faces: face.clone(),
            second_faces: face.clone(),
            radius: RadiusSpec::Variable { points: Vec::new() },
        },
        FeatureDefinition::BoundaryFill {
            tools: BodySelection::Bodies(vec![body]),
            cells: Vec::new(),
        },
    ];
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("construction-{ordinal}")),
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
            definition,
            native_ref: None,
        });
    }
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

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "7 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn binder_completeness_requires_resolved_targets_and_shape_arity() {
    let mut ir = CadIr::empty(Units::default());
    let source = FeatureId("source".into());
    let feature = |id: &str, ordinal, dependencies, definition| Feature {
        id: FeatureId(id.into()),
        ordinal,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies,
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: None,
    };
    ir.model.features.push(feature(
        "source",
        0,
        Vec::new(),
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        },
    ));
    let shape = |sources| FeatureDefinition::Binder {
        sources,
        construction: cadmpeg_ir::features::BinderConstruction::Shape {
            trace_support: false,
        },
    };
    ir.model.features.push(feature(
        "complete",
        1,
        vec![source.clone()],
        shape(vec![cadmpeg_ir::features::BinderSource {
            target: cadmpeg_ir::features::BinderTarget::Feature {
                feature: source.clone(),
            },
            subelements: vec!["Face1".into()],
        }]),
    ));
    ir.model.features.push(feature(
        "native",
        2,
        Vec::new(),
        shape(vec![cadmpeg_ir::features::BinderSource {
            target: cadmpeg_ir::features::BinderTarget::Native {
                reference: "source".into(),
            },
            subelements: Vec::new(),
        }]),
    ));
    ir.model.features.push(feature(
        "multiple-shape-sources",
        3,
        Vec::new(),
        shape(vec![
            cadmpeg_ir::features::BinderSource {
                target: cadmpeg_ir::features::BinderTarget::External {
                    document: "a.FCStd".into(),
                    object: "Body".into(),
                },
                subelements: Vec::new(),
            },
            cadmpeg_ir::features::BinderSource {
                target: cadmpeg_ir::features::BinderTarget::External {
                    document: "b.FCStd".into(),
                    object: "Body".into(),
                },
                subelements: Vec::new(),
            },
        ]),
    ));
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

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "2 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn post_process_completeness_delegates_to_the_wrapped_operation() {
    let mut ir = CadIr::empty(Units::default());
    let post_process = |operation| FeatureDefinition::PostProcess {
        operation: Box::new(operation),
        refine: true,
        fuzzy_tolerance: cadmpeg_ir::features::FuzzyTolerance::KernelDefault,
    };
    for (ordinal, definition) in [
        post_process(FeatureDefinition::Helix {
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_direction: Vector3::new(0.0, 0.0, 1.0),
            radius: Length(1.0),
            pitch: Length(2.0),
            revolutions: 3.0,
            start_angle: Angle(0.0),
            clockwise: false,
            radial_growth: None,
            cone_angle: None,
            segment_turns: None,
            construction_style: None,
        }),
        post_process(post_process(FeatureDefinition::DatumPlaneUnresolved)),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(Feature {
            id: FeatureId(format!("post-process-{ordinal}")),
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
            definition,
            native_ref: None,
        });
    }
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

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn design_completeness_recurses_through_pattern_operands() {
    let mut ir = CadIr::empty(Units::default());
    let seed = cadmpeg_ir::features::PatternSeed::Feature(FeatureId("seed".into()));
    for (ordinal, pattern) in [
        (
            0,
            PatternKind::LinearOffsets {
                direction: None,
                offsets: vec![Length(0.0), Length(10.0)],
            },
        ),
        (
            1,
            PatternKind::CurveDriven {
                path: Some(PathRef::Native("path".into())),
                spacing: Length(10.0),
                count: 2,
            },
        ),
        (
            2,
            PatternKind::Scale {
                center: cadmpeg_ir::features::PatternScaleCenter::Native("center".into()),
                final_factor: 2.0,
                count: 2,
            },
        ),
        (
            3,
            PatternKind::Composite {
                stages: vec![cadmpeg_ir::features::PatternStage {
                    pattern: Box::new(PatternKind::CurveDriven {
                        path: None,
                        spacing: Length(10.0),
                        count: 2,
                    }),
                    combination: cadmpeg_ir::features::PatternStageCombination::Initialize,
                }],
            },
        ),
        (
            4,
            PatternKind::Circular {
                axis_origin: Point3::new(0.0, 0.0, 0.0),
                axis_dir: Vector3::new(0.0, 0.0, 1.0),
                angle: Angle(std::f64::consts::TAU),
                count: 4,
            },
        ),
    ] {
        ir.model.features.push(Feature {
            id: FeatureId(format!("pattern-{ordinal}")),
            ordinal,
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
                pattern,
            },
            native_ref: None,
        });
    }
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

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "4 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn design_completeness_checks_secondary_sweep_and_loft_paths() {
    let mut ir = CadIr::empty(Units::default());
    let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());
    let profile = cadmpeg_ir::features::ProfileRef::Sketch(sketch.clone());
    let path = PathRef::Sketch(sketch);
    let sweep = |sections, orientation| FeatureDefinition::Sweep {
        section: cadmpeg_ir::features::SweepSection::Profile(profile.clone()),
        sections,
        path: Some(path.clone()),
        mode: cadmpeg_ir::features::SweepMode::Surface,
        orientation,
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
    let definitions = [
        sweep(
            vec![cadmpeg_ir::features::SweepSection::Profile(
                cadmpeg_ir::features::ProfileRef::Native("section".into()),
            )],
            None,
        ),
        sweep(
            Vec::new(),
            Some(cadmpeg_ir::features::SweepOrientation::Auxiliary {
                path: PathRef::Native("auxiliary".into()),
                tangent: false,
                curvilinear: false,
            }),
        ),
        FeatureDefinition::Loft {
            sections: vec![
                cadmpeg_ir::features::LoftSection::Profile(profile.clone()),
                cadmpeg_ir::features::LoftSection::Profile(profile.clone()),
            ],
            guides: Vec::new(),
            centerline: Some(PathRef::Native("centerline".into())),
            op: BooleanOp::NewBody,
            closed: false,
            solid: false,
            ruled: false,
            max_degree: None,
            check_compatibility: None,
            allow_multi_profile_faces: None,
        },
        sweep(Vec::new(), None),
    ];
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("path-feature-{ordinal}")),
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
            definition,
            native_ref: None,
        });
    }
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

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "3 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn design_completeness_rejects_explicitly_unresolved_operation_fields() {
    let mut ir = CadIr::empty(Units::default());
    let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());
    let profile = cadmpeg_ir::features::ProfileRef::Sketch(sketch.clone());
    let path = PathRef::Sketch(sketch);
    let face = FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId("face".into())]);
    let extrude = |direction, termination| FeatureDefinition::Extrude {
        profile: profile.clone(),
        direction,
        start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
        extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
            side: cadmpeg_ir::features::ExtrudeSide {
                termination,
                draft: None,
                offset: None,
            },
        },
        op: BooleanOp::NewBody,
        direction_source: None,
        solid: Some(true),
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    };
    let definitions = [
        FeatureDefinition::ProjectedCurve {
            source: path.clone(),
            target_faces: face.clone(),
            direction: cadmpeg_ir::features::CurveProjectionDirection::State(
                cadmpeg_ir::features::CurveProjectionDirectionState::Unresolved,
            ),
            bidirectional: Some(false),
        },
        extrude(
            cadmpeg_ir::features::ExtrudeDirection::Unresolved,
            cadmpeg_ir::features::Termination::Blind {
                length: Length(10.0),
            },
        ),
        extrude(
            cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            cadmpeg_ir::features::Termination::ToVertex {
                vertex: cadmpeg_ir::features::VertexSelection::Native("vertex".into()),
            },
        ),
        FeatureDefinition::OffsetSurface {
            faces: face.clone(),
            distance: None,
        },
        FeatureDefinition::KnitSurface {
            faces: face.clone(),
            merge_entities: None,
            create_solid: None,
            gap_tolerance: None,
        },
        FeatureDefinition::ExtendSurface {
            faces: face.clone(),
            distance: Some(Length(10.0)),
            method: cadmpeg_ir::features::SurfaceExtension::Unresolved,
        },
        FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(path.clone()),
            support_faces: face.clone(),
            continuity: None,
            boundary_continuities: Vec::new(),
            merge_result: Some(false),
        },
        FeatureDefinition::TrimSurface {
            faces: face.clone(),
            tool: path.clone(),
            keep: cadmpeg_ir::features::TrimRegion::Unresolved,
        },
        FeatureDefinition::Draft {
            faces: face.clone(),
            neutral_plane: face.clone(),
            parting_tool: None,
            pull_direction: None,
            pull_plane: None,
            angle: None,
            outward: None,
        },
        FeatureDefinition::ProjectedCurve {
            source: path,
            target_faces: face,
            direction: cadmpeg_ir::features::CurveProjectionDirection::State(
                cadmpeg_ir::features::CurveProjectionDirectionState::TargetNormal,
            ),
            bidirectional: Some(false),
        },
    ];
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("operation-{ordinal}")),
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
            definition,
            native_ref: None,
        });
    }
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

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "9 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn empty_required_operands_are_incomplete_design_semantics() {
    let mut ir = CadIr::empty(Units::default());
    let feature = |ordinal, definition| Feature {
        id: FeatureId(format!("feature-{ordinal}")),
        ordinal,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: None,
    };
    ir.model.features.extend([
        feature(
            0,
            FeatureDefinition::Fillet {
                groups: vec![cadmpeg_ir::features::FilletGroup {
                    edges: EdgeSelection::Edges(Vec::new()),
                    radius: RadiusSpec::Constant {
                        radius: Length(1.0),
                    },
                    tangency_weight: None,
                }],
            },
        ),
        feature(
            1,
            FeatureDefinition::DeleteFace {
                faces: FaceSelection::Faces(Vec::new()),
                heal: false,
            },
        ),
        feature(
            2,
            FeatureDefinition::DeleteBody {
                bodies: BodySelection::Bodies(Vec::new()),
                mode: BodyRetentionMode::DeleteSelected,
            },
        ),
        feature(
            3,
            FeatureDefinition::CompositeCurve {
                segments: vec![PathRef::Edges(Vec::new())],
                closed: false,
            },
        ),
        feature(
            4,
            FeatureDefinition::Shell {
                bodies: None,
                removed_faces: FaceSelection::Faces(Vec::new()),
                thickness: Some(Length(1.0)),
                outward: Some(false),
                mode: None,
                join: None,
                resolve_intersections: None,
                allow_self_intersections: None,
            },
        ),
        feature(
            5,
            FeatureDefinition::FilledSurface {
                boundary: cadmpeg_ir::features::SurfaceBoundary::Edges(EdgeSelection::Edges(vec![
                    EdgeId("boundary".into()),
                ])),
                support_faces: FaceSelection::Faces(Vec::new()),
                continuity: Some(SurfaceContinuity::Contact),
                boundary_continuities: Vec::new(),
                merge_result: Some(false),
            },
        ),
        feature(
            6,
            FeatureDefinition::RuledSurface {
                edges: EdgeSelection::Edges(vec![EdgeId("boundary".into())]),
                support_faces: FaceSelection::Faces(Vec::new()),
                mode: RuledSurfaceMode::Direction {
                    direction: Vector3::new(0.0, 0.0, 1.0),
                    distance: Length(1.0),
                },
                angle: None,
                alternate_face: None,
                corner: None,
            },
        ),
        feature(
            7,
            FeatureDefinition::Fillet {
                groups: vec![cadmpeg_ir::features::FilletGroup {
                    edges: EdgeSelection::Edges(vec![EdgeId("edge".into())]),
                    radius: RadiusSpec::Variable { points: Vec::new() },
                    tangency_weight: None,
                }],
            },
        ),
    ]);
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
            == "6 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn hole_completeness_checks_optional_operands_when_present() {
    let mut ir = CadIr::empty(Units::default());
    let hole = |profile, exit_kind| FeatureDefinition::Hole {
        profile,
        profile_filter: None,
        face: None,
        position: None,
        direction: None,
        placements: vec![cadmpeg_ir::features::HolePlacement::Directed {
            position: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        }],
        kind: cadmpeg_ir::features::HoleKind::Simple,
        exit_kind,
        diameter: Some(Length(5.0)),
        extent: Some(cadmpeg_ir::features::Termination::ThroughAll),
        bottom: None,
        taper_angle: None,
        specification: None,
        allow_multi_profile_faces: None,
    };
    for (ordinal, definition) in [
        hole(
            Some(cadmpeg_ir::features::ProfileRef::Native("profile".into())),
            None,
        ),
        hole(
            None,
            Some(cadmpeg_ir::features::HoleKind::Unresolved {
                form: None,
                counterbore_diameter: None,
                counterbore_depth: None,
                countersink_diameter: None,
                countersink_angle: None,
            }),
        ),
        hole(None, None),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(Feature {
            id: FeatureId(format!("hole-{ordinal}")),
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
            definition,
            native_ref: None,
        });
    }
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

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "2 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn incomplete_parameter_semantics_are_reported_as_design_losses() {
    let mut ir = CadIr::empty(Units::default());
    let owner = FeatureId("owner".into());
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: Some("Boss-Extrude1".into()),
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
        id: ParameterId("base-parameter".into()),
        owner: Some(owner.clone()),
        ordinal: 0,
        name: "D0".into(),
        expression: "1mm".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Length(Length(1.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId("parameter".into()),
        owner: Some(owner.clone()),
        ordinal: 1,
        name: "D1".into(),
        expression: "\"D0@Boss-Extrude1\" + Missing@Sketch1".into(),
        display: None,
        value: None,
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId("bare-reference".into()),
        owner: Some(owner.clone()),
        ordinal: 2,
        name: "D2".into(),
        expression: "D99 + 1".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId("malformed-reference".into()),
        owner: Some(owner.clone()),
        ordinal: 3,
        name: "D3".into(),
        expression: "\"D0@Boss-Extrude1".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    let future = ParameterId("future".into());
    ir.model.parameters.push(DesignParameter {
        id: ParameterId("forward-reference".into()),
        owner: Some(owner.clone()),
        ordinal: 4,
        name: "D4".into(),
        expression: "D5".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Real(2.0)),
        dependencies: vec![future.clone()],
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: future,
        owner: Some(owner.clone()),
        ordinal: 5,
        name: "D5".into(),
        expression: "1".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId("omitted-dependency".into()),
        owner: Some(owner.clone()),
        ordinal: 6,
        name: "D6".into(),
        expression: "D0 + 1mm".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Length(Length(2.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId("cached-unsupported-expression".into()),
        owner: Some(owner.clone()),
        ordinal: 7,
        name: "D7".into(),
        expression: "unsupported(1)".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    for (id, ordinal, name) in [
        ("empty", 8, ""),
        ("shared-a", 9, "Shared"),
        ("shared-b", 10, "Shared"),
        ("ordinal", 10, "Unique"),
    ] {
        ir.model.parameters.push(DesignParameter {
            id: ParameterId(format!("identity:{id}")),
            owner: Some(owner.clone()),
            ordinal,
            name: name.into(),
            expression: "1".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
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

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 parameter(s) lack an evaluated scalar; 3 parameter expression(s) contain unresolved, ambiguous, or malformed parameter references; 4 parameter expression(s) cannot regenerate a finite typed value; 1 parameter record(s) contain missing or non-preceding dependency edges; 2 parameter record(s) have dependency edges inconsistent with their expressions; 1 dependency-driven parameter(s) disagree with their evaluated expressions."
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 parameter record(s) have empty names; 2 parameter record(s) share owner-local names; 2 parameter record(s) share owner-local ordinals."
    }));
}

#[test]
fn incoherent_feature_graph_is_reported_as_design_loss() {
    let mut ir = CadIr::empty(Units::default());
    let first = FeatureId("first".into());
    let second = FeatureId("second".into());
    let missing = FeatureId("missing".into());
    let feature = |id, ordinal, parent, dependencies| Feature {
        id,
        ordinal,
        name: None,
        suppressed: Some(false),
        parent,
        dependencies,
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
    };
    ir.model
        .features
        .push(feature(first.clone(), 0, None, vec![second.clone()]));
    ir.model
        .features
        .push(feature(second, 1, Some(first.clone()), vec![first]));
    ir.model.features.push(feature(
        FeatureId("third".into()),
        1,
        Some(missing),
        Vec::new(),
    ));
    ir.model.features[0].source_content = vec![
        FeatureSourceContent::Feature(FeatureId("second".into())),
        FeatureSourceContent::Feature(FeatureId("second".into())),
    ];
    ir.model.features[1].source_content =
        vec![FeatureSourceContent::Feature(FeatureId("third".into()))];
    ir.model.features[2].source_content = vec![FeatureSourceContent::Parameter(ParameterId(
        "missing-parameter".into(),
    ))];
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
            == "2 feature record(s) contain missing, repeated, or non-preceding parent/dependency edges; 2 feature record(s) share regeneration ordinals."
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "3 feature record(s) contain missing, repeated, misowned, or structurally inconsistent source-content references."
    }));
}

#[test]
fn incoherent_feature_outputs_are_reported_as_design_loss() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.features.clear();
    ir.model.parameters.clear();
    let body = ir.model.bodies[0].id.clone();
    let feature = |id: &str, ordinal: u64, outputs: Vec<BodyId>| Feature {
        id: FeatureId(id.into()),
        ordinal,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs,
        definition: FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        },
        native_ref: None,
    };
    ir.model
        .features
        .push(feature("duplicate", 0, vec![body.clone(), body]));
    ir.model
        .features
        .push(feature("missing", 1, vec![BodyId("missing-body".into())]));
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
        loss.message == "2 feature record(s) contain missing or repeated output body references."
    }));
}

#[test]
fn configuration_partitions_require_explicit_source_identity() {
    let mut ir = CadIr::empty(Units::default());
    let configuration = |id: &str, ordinal, source_index| DesignConfiguration {
        id: ConfigurationId(id.into()),
        ordinal,
        active: false.into(),
        source_index,
        name: id.into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: cadmpeg_ir::ConfigurationBodies::Unresolved,
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: Some(format!("native:{id}")),
    };
    ir.model
        .configurations
        .push(configuration("explicit", 0, Some(5)));
    ir.model
        .configurations
        .push(configuration("inferred", 9, None));
    ir.model
        .configurations
        .push(configuration("empty", 10, Some(8)));
    let first = BodyId("body:first".into());
    let second = BodyId("body:second".into());
    let third = BodyId("body:third".into());

    assign_configuration_bodies(
        &mut ir,
        &[
            (7, vec![third.clone()]),
            (5, vec![first.clone()]),
            (5, vec![second.clone()]),
        ],
    );

    assert_eq!(ir.model.configurations[0].source_index, Some(5));
    assert_eq!(ir.model.configurations[0].bodies, vec![first, second]);
    assert_eq!(ir.model.configurations[1].source_index, None);
    assert!(ir.model.configurations[1].bodies.is_unresolved());
    assert_eq!(ir.model.configurations[2].source_index, Some(8));
    assert!(ir.model.configurations[2].bodies.is_empty());
    assert_eq!(ir.model.configurations[3].source_index, Some(7));
    assert_eq!(ir.model.configurations[3].bodies, vec![third]);
    assert!(ir.model.configurations[3].native_ref.is_none());
}

#[test]
fn duplicate_configuration_source_identity_does_not_select_a_partition() {
    let mut ir = CadIr::empty(Units::default());
    for ordinal in 0..2 {
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId(format!("configuration:{ordinal}")),
            ordinal,
            active: false.into(),
            source_index: Some(5),
            name: format!("Configuration {ordinal}").into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Unresolved,
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some(format!("native:{ordinal}")),
        });
    }
    let body = BodyId("body:partition".into());

    assign_configuration_bodies(&mut ir, &[(5, vec![body.clone()])]);

    assert!(ir.model.configurations[0].bodies.is_unresolved());
    assert!(ir.model.configurations[1].bodies.is_unresolved());
    assert_eq!(ir.model.configurations[2].source_index, Some(5));
    assert_eq!(ir.model.configurations[2].bodies, vec![body]);
    assert!(ir.model.configurations[2].native_ref.is_none());
}

#[test]
fn inferred_partition_does_not_fabricate_active_configuration_identity() {
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(cadmpeg_ir::document::SourceMeta {
        attributes: BTreeMap::from([
            (
                "active_parasolid_block".into(),
                "Contents/Config-3-Partition".into(),
            ),
            ("sw_configuration_name".into(), "Default".into()),
        ]),
        ..Default::default()
    });
    let body = BodyId("body:active".into());

    assign_configuration_bodies(&mut ir, &[(3, vec![body.clone()])]);
    mark_active_configuration(&mut ir);

    assert_eq!(ir.model.configurations.len(), 1);
    let configuration = &ir.model.configurations[0];
    assert!(configuration.active.is_inactive());
    assert_eq!(configuration.source_index, Some(3));
    assert_eq!(configuration.bodies, vec![body]);

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
    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "active configuration identity is unresolved; 0 of 1 configuration records are active."
    }));
}

#[test]
fn duplicate_configuration_partition_identities_are_reported() {
    let mut ir = CadIr::empty(Units::default());
    for id in ["first", "second"] {
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId(id.into()),
            ordinal: ir.model.configurations.len() as u32,
            active: false.into(),
            source_index: Some(5),
            name: id.into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some(format!("native:{id}")),
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

    assert!(report.losses.iter().any(|loss| {
        loss.message == "2 configuration record(s) share non-unique geometry partition identities."
    }));
}

#[test]
fn incomplete_configuration_names_are_reported() {
    let mut ir = CadIr::empty(Units::default());
    for (position, (ordinal, name)) in [(0, ""), (1, "Shared"), (2, "Shared"), (2, "Unique")]
        .into_iter()
        .enumerate()
    {
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId(format!("configuration:{position}")),
            ordinal,
            active: (position == 1).into(),
            source_index: Some(position as u32),
            name: name.into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some(format!("native:{position}")),
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

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 configuration record(s) have empty names; 2 configuration record(s) share non-unique names; 2 configuration record(s) share regeneration ordinals."
    }));
}

#[test]
fn active_configuration_partition_disagreement_is_reported() {
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(cadmpeg_ir::document::SourceMeta {
        format: "sldprt".into(),
        attributes: BTreeMap::from([(
            "active_parasolid_block".into(),
            "Contents/Config-3-Partition".into(),
        )]),
    });
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("configuration".into()),
        ordinal: 0,
        active: true.into(),
        source_index: Some(5),
        name: "Default".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: Some("native:configuration".into()),
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
            == "active configuration identity does not resolve to active geometry partition 3."
    }));
}

#[test]
fn incoherent_configuration_bodies_are_reported() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let body = ir.model.bodies[0].id.clone();
    let configuration = |id: &str, ordinal, bodies| DesignConfiguration {
        id: ConfigurationId(id.into()),
        ordinal,
        active: (ordinal == 0).into(),
        source_index: Some(ordinal),
        name: id.into(),
        material: None,
        properties: BTreeMap::new(),
        bodies,
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: Some(format!("native:{id}")),
    };
    ir.model.configurations = vec![
        configuration(
            "duplicate",
            0,
            cadmpeg_ir::ConfigurationBodies::Resolved(vec![body.clone(), body]),
        ),
        configuration(
            "missing",
            1,
            cadmpeg_ir::ConfigurationBodies::Resolved(vec![BodyId("missing-body".into())]),
        ),
        configuration("unresolved", 2, cadmpeg_ir::ConfigurationBodies::Unresolved),
    ];
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
        loss.message == "1 configuration record(s) have unresolved body membership; 2 configuration record(s) contain missing or repeated body references."
    }));
}

#[test]
fn configuration_values_complete_parameters_without_baseline_values() {
    let mut ir = CadIr::empty(Units::default());
    let parameter = ParameterId("configured-parameter".into());
    ir.model.parameters.push(DesignParameter {
        id: parameter.clone(),
        owner: None,
        ordinal: 0,
        name: "Configured".into(),
        expression: "12mm".into(),
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
        name: "Default".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::from([(parameter, ParameterValue::Length(Length(12.0)))]),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: Some("native:configuration".into()),
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

    assert!(!report.losses.iter().any(|loss| {
        loss.message
            .contains("complete evaluated parameter snapshot")
            || loss.message.contains("lack an evaluated scalar")
    }));
}

#[test]
fn configuration_suppression_and_override_references_are_coherent() {
    let mut ir = CadIr::empty(Units::default());
    let feature = FeatureId("feature".into());
    let definition = FeatureDefinition::TreeNode {
        role: FeatureTreeNodeRole::History,
        children: Vec::new(),
        active_child: None,
    };
    ir.model.features.push(Feature {
        id: feature.clone(),
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
        definition: definition.clone(),
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("configuration".into()),
        ordinal: 0,
        active: true.into(),
        source_index: Some(0),
        name: "Default".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::from([(ParameterId("missing".into()), "1mm".into())]),
        feature_states: BTreeMap::from([(
            feature,
            ConfigurationFeatureState {
                suppressed: true,
                dependencies: Vec::new(),
                outputs: Vec::new(),
                definition,
            },
        )]),
        native_ref: Some("native:configuration".into()),
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

    assert!(report.losses.iter().any(|loss| {
        loss.message == "1 configuration(s) have missing, repeated, or feature-state-inconsistent suppression members; 1 configuration(s) reference missing parameter overrides."
    }));
}

#[test]
fn native_planar_and_spatial_sketch_geometry_is_reported() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.sketch_entities.push(SketchEntity {
        id: SketchEntityId("planar-entity".into()),
        sketch: SketchId("planar-sketch".into()),
        construction: false,
        native_ref: Some("native:planar".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Native {
            native_kind: "SplineHandle".into(),
        },
    });
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: SpatialSketchEntityId("spatial-entity".into()),
        sketch: SpatialSketchId("spatial-sketch".into()),
        construction: false,
        native_ref: Some("native:spatial".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Native {
            native_kind: "ReferenceCurve".into(),
        },
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
            == "2 sketch entity geometry record(s) retain native kinds without solved neutral geometry."
    }));
}

#[test]
fn only_sketch_owned_relation_records_without_constraints_are_counted() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.features.push(Feature {
        id: FeatureId("sketch-feature".into()),
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
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::default(),
            sketch: Some(SketchId("sketch".into())),
        },
        native_ref: Some("feature".into()),
    });
    ir.model.sketch_entities.push(SketchEntity {
        id: SketchEntityId("represented-geometry".into()),
        sketch: SketchId("sketch".into()),
        construction: false,
        native_ref: Some("geometry-marker".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Native {
            native_kind: "UnknownGeometry".into(),
        },
    });
    let marker = |id: &str, ordinal, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal,
        offset: u64::from(ordinal),
        object_index: None,
        local_id: None,
        kind,
        state_value: None,
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let relation = FeatureInputRelationInstance {
        id: "relation-instance".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalar_refs: vec!["scalar".into()],
        parameter_scalar_ref: Some("scalar".into()),
        display_scalar_ref: None,
        operands: Vec::new(),
    };
    let binding =
        |id: &str, class_ref: &str, scalar_ref: &str, ordinal| FeatureInputRelationBinding {
            id: id.into(),
            parent: "lane".into(),
            ordinal,
            offset: u64::from(ordinal),
            class_ref: class_ref.into(),
            family: FeatureInputRelationFamily::PointPointDistance,
            scalar_ref: scalar_ref.into(),
            feature_ref: Some("feature".into()),
        };
    let mut relation_marker = marker(
        "relation-marker",
        0,
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
    );
    relation_marker.links.push(SketchInputLink {
        local_id: 1,
        entity_ref: "geometry-marker".into(),
    });
    let native = SldprtNative {
        feature_input_lanes: vec![FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: vec![
                binding("grouped-binding", "class", "scalar", 0),
                binding("orphan-binding", "other-class", "other-scalar", 1),
            ],
            relation_instances: vec![relation],
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![
                relation_marker,
                marker(
                    "dimension-handle",
                    1,
                    SketchInputKind::Relation(SketchRelationKind::Distance),
                ),
                marker("geometry-marker", 2, SketchInputKind::Native(99)),
                marker(
                    "operandless-relation-marker",
                    3,
                    SketchInputKind::Relation(SketchRelationKind::Vertical),
                ),
            ],
        }],
        ..SldprtNative::default()
    };

    assert_eq!(unprojected_sketch_relation_records(&ir, &native), 3);

    ir.model.features[0].definition = FeatureDefinition::TreeNode {
        role: FeatureTreeNodeRole::History,
        children: Vec::new(),
        active_child: None,
    };
    assert_eq!(unprojected_sketch_relation_records(&ir, &native), 0);
}

#[test]
fn native_relation_records_have_at_most_one_neutral_owner() {
    let mut ir = CadIr::empty(Units::default());
    let entity = |id: &str, native_ref: &str| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("sketch".into()),
        construction: false,
        native_ref: Some(native_ref.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Native {
            native_kind: "UnknownGeometry".into(),
        },
    };
    ir.model.sketch_entities = vec![
        entity("first", "relation-marker"),
        entity("second", "relation-marker"),
        entity("profile", "profile-stream-record"),
    ];
    let native = SldprtNative {
        feature_input_lanes: vec![FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![
                SketchInputEntity {
                    id: "relation-marker".into(),
                    parent: "lane".into(),
                    feature_ref: Some("feature".into()),
                    ordinal: 0,
                    offset: 0,
                    object_index: None,
                    local_id: None,
                    kind: SketchInputKind::Relation(SketchRelationKind::Horizontal),
                    state_value: None,
                    coordinates_m: None,
                    links: vec![SketchInputLink {
                        local_id: 1,
                        entity_ref: "geometry-marker".into(),
                    }],
                    link_selector: None,
                },
                SketchInputEntity {
                    id: "geometry-marker".into(),
                    parent: "lane".into(),
                    feature_ref: Some("feature".into()),
                    ordinal: 1,
                    offset: 1,
                    object_index: None,
                    local_id: Some(1),
                    kind: SketchInputKind::Native(99),
                    state_value: None,
                    coordinates_m: None,
                    links: Vec::new(),
                    link_selector: None,
                },
            ],
        }],
        ..SldprtNative::default()
    };

    assert_eq!(multiply_projected_sketch_relation_records(&ir, &native), 1);
}

#[test]
fn direct_feature_input_operations_require_unique_history_bindings() {
    let class_name = "moExtrusion_c";
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: vec![FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 10,
            name: class_name.into(),
            role: FeatureInputClassRole::Feature,
        }],
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 10 + 6 + class_name.len() as u64,
            object_id: Some(42),
            value: "Boss".into(),
        }],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let mut native = SldprtNative {
        feature_input_lanes: vec![lane.clone()],
        ..SldprtNative::default()
    };
    assert_eq!(unbound_feature_input_operation_objects(&native), 1);

    native.feature_histories.push(FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![NativeFeature {
            id: "feature".into(),
            parent: "history".into(),
            xml_tag: "Extrusion".into(),
            tree_parent: None,
            source_id: Some("42".into()),
            parent_source_id: None,
            ordinal: 0,
            name: "Boss".into(),
            kind: "Extrusion".into(),
            input_class: Some(class_name.into()),
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    });
    assert_eq!(unbound_feature_input_operation_objects(&native), 0);
    native.feature_histories[0].features[0].input_class = Some("moSweep_c".into());
    assert_eq!(unbound_feature_input_operation_objects(&native), 1);
    native.feature_histories[0].features[0].input_class = Some(class_name.into());
    native.feature_histories[0].features[0].source_id = None;
    assert_eq!(unbound_feature_input_operation_objects(&native), 0);
    let mut duplicate = native.feature_histories[0].features[0].clone();
    duplicate.id = "duplicate-feature".into();
    native.feature_histories[0].features.push(duplicate);
    assert_eq!(unbound_feature_input_operation_objects(&native), 1);

    lane.names[0].offset += 1;
    native.feature_input_lanes = vec![lane];
    assert_eq!(unbound_feature_input_operation_objects(&native), 0);
}

#[test]
fn native_dimension_subtypes_are_reported() {
    let mut ir = CadIr::empty(Units::default());
    let owner = FeatureId("owner".into());
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: Some("Feature".into()),
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
        owner: Some(owner),
        ordinal: 0,
        name: "D1".into(),
        expression: "1".into(),
        display: None,
        value: Some(ParameterValue::Real(1.0)),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: Some(ParameterPmi {
            subtype: PmiDimensionSubtype::Native("Ordinate".into()),
            precision: 3,
            display_text: None,
            basic: false,
            inspection: false,
            reference_only: false,
            native_ref: "native:pmi".into(),
        }),
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
            == "0 semantic dimension record(s) are not bound to parameters; 1 parameter dimension(s) retain native subtypes."
    }));
}

#[test]
fn geometry_report_surfaces_ambiguous_pcurve_loss() {
    let scan = ContainerScan {
        source_image: &[],
        version: 0,
        blocks: Vec::new(),
        directory: Vec::new(),
        cache_cells: Vec::new(),
        compound_streams: Vec::new(),
    };
    let mut decoded = Brep::default();
    decoded.stats.ambiguous_pcurve_parameters = 2;

    let report = super::build_geometry_report(&scan, &decoded);
    assert!(report.losses.iter().any(|loss| {
        loss.code == crate::loss::SldprtLossCode::GeometryPcurveAmbiguous.kind()
            && loss.message.contains("2 pcurve(s)")
    }));
}
