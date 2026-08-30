// SPDX-License-Identifier: Apache-2.0
//! Configuration partition identity and coherence tests.
#![allow(clippy::unwrap_used)]

use super::super::*;
use cadmpeg_ir::features::{
    ConfigurationFeatureState, ConfigurationId, DesignConfiguration, DesignParameter, Feature,
    FeatureDefinition, FeatureId, FeatureTreeNodeRole, Length, ParameterId, ParameterValue,
};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

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
    ir.source = Some(cadmpeg_ir::document::SourceMeta::unclassified(
        "",
        BTreeMap::from([
            (
                "active_parasolid_block".into(),
                "Contents/Config-3-Partition".into(),
            ),
            ("sw_configuration_name".into(), "Default".into()),
        ]),
    ));
    let body = BodyId("body:active".into());

    assign_configuration_bodies(&mut ir, &[(3, vec![body.clone()])]);
    mark_active_configuration(&mut ir);

    assert_eq!(ir.model.configurations.len(), 1);
    let configuration = &ir.model.configurations[0];
    assert!(configuration.active.is_inactive());
    assert_eq!(configuration.source_index, Some(3));
    assert_eq!(configuration.bodies, vec![body]);

    let mut report = super::empty_report(true);
    append_design_losses(&ir, &mut report);
    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "active configuration identity is unresolved; 0 of 1 configuration records are active."
    }));
}

#[test]
fn active_configuration_name_binds_partition_without_fabricating_body_membership() {
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(cadmpeg_ir::document::SourceMeta::unclassified(
        "",
        BTreeMap::from([
            (
                "active_parasolid_block".into(),
                "Contents/Config-3-Partition".into(),
            ),
            ("sw_configuration_name".into(), "Default".into()),
        ]),
    ));
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("configuration".into()),
        ordinal: 0,
        active: false.into(),
        source_index: None,
        name: "Default".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: cadmpeg_ir::ConfigurationBodies::Unresolved,
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: Some("native:configuration".into()),
    });

    assign_configuration_bodies(&mut ir, &[]);
    mark_active_configuration(&mut ir);

    let configuration = &ir.model.configurations[0];
    assert_eq!(configuration.source_index, Some(3));
    assert!(configuration.bodies.is_unresolved());
    assert!(configuration.active.is_active());

    let mut report = super::empty_report(false);
    append_design_losses(&ir, &mut report);
    assert!(!report.losses.iter().any(|loss| {
        loss.message
            == "active configuration identity does not resolve to active geometry partition 3."
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
    let mut report = super::empty_report(true);

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
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 configuration record(s) have empty names; 2 configuration record(s) share non-unique names; 2 configuration record(s) share regeneration ordinals."
    }));
}

#[test]
fn active_configuration_partition_disagreement_is_reported() {
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(cadmpeg_ir::document::SourceMeta::unclassified(
        "sldprt",
        BTreeMap::from([(
            "active_parasolid_block".into(),
            "Contents/Config-3-Partition".into(),
        )]),
    ));
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
    let mut report = super::empty_report(true);

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
    let mut report = super::empty_report(true);

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
    let mut report = super::empty_report(true);

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
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message == "1 configuration(s) have missing, repeated, or feature-state-inconsistent suppression members; 1 configuration(s) reference missing parameter overrides."
    }));
}
