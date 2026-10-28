// SPDX-License-Identifier: Apache-2.0
//! Configuration partition identity and coherence tests.
#![allow(clippy::unwrap_used)]

use crate::decode::append_design_losses;
use crate::decode::assign_configuration_bodies;
use crate::decode::mark_active_configuration;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::CadIr;
use cadmpeg_ir::{
    features::{
        ConfigurationFeatureState, ConfigurationId, DesignConfiguration, DesignParameter, Feature,
        FeatureDefinition, FeatureId, FeatureOperation, FeatureTreeNodeRole, ParameterId,
        ParameterValue,
    },
    scalar::Length,
};
use std::collections::BTreeMap;

#[test]
fn missing_configuration_partition_does_not_establish_empty_membership() {
    let mut ir = CadIr::empty();
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("test:model:configuration#missing").expect("identity grammar"),
        ordinal: 0,
        active: false,
        source_index: Some(8),
        name: Some("Missing".to_string()),
        material: None,
        properties: BTreeMap::new(),
        bodies: None,
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    assign_configuration_bodies(&mut ir, Vec::new(), None).unwrap();
    assert!(ir.model.configurations[0].bodies.is_none());
}

fn manifest_configuration_source(keywords: &[u8]) -> Vec<u8> {
    use crate::test_support::container::{make_block, sldprt_with_colliding_sites};
    let mut source = sldprt_with_colliding_sites();
    source.extend(make_block(0x42, "Contents/Keywords", keywords));
    source.extend(make_block(0x43, "Contents/Features", br#"<swSolidWorks><swModel id="2" swConfigurationName="Default" swConfigurationId="0"/><swModel id="5" swConfigurationName="HI" swConfigurationId="1"/><swConfiguration swName="Default" swID="0" swModelRef="2" swMostRecentConfiguration="NO" swConfigurationNeedsUpdate="NO"/><swConfiguration swName="HI" swID="1" swModelRef="5" swMostRecentConfiguration="YES" swConfigurationNeedsUpdate="NO"/></swSolidWorks>"#));
    source
}

fn named_configuration_ir() -> CadIr {
    let mut ir = CadIr::empty();
    for (ordinal, name) in [(0, "Default"), (1, "HI")] {
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId::mint(format!("test:model:configuration#{name}"))
                .expect("identity grammar"),
            ordinal,
            active: false,
            source_index: None,
            name: Some(name.to_string()),
            material: None,
            properties: BTreeMap::new(),
            bodies: None,
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some(format!("native:{name}")),
        });
    }
    ir
}

#[test]
fn manifest_joins_bind_inactive_and_active_bodies_without_redundant_placeholders() {
    let source = manifest_configuration_source(
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="HI"/></Keywords>"#,
    );
    let scan = crate::test_support::container::scan(&source);
    let mut ir = named_configuration_ir();
    let a = BodyId::mint("test:model:body#inactive").expect("identity grammar");
    let b = BodyId::mint("test:model:body#active").expect("identity grammar");
    assign_configuration_bodies(
        &mut ir,
        vec![(0, vec![a.clone()]), (1, vec![b.clone()])],
        Some(&scan),
    )
    .unwrap();
    assert_eq!(ir.model.configurations.len(), 2);
    assert_eq!(ir.model.configurations[0].source_index, Some(0));
    assert_eq!(
        ir.model.configurations[0].bodies.as_deref(),
        Some([a].as_slice())
    );
    assert_eq!(ir.model.configurations[1].source_index, Some(1));
    assert_eq!(
        ir.model.configurations[1].bodies.as_deref(),
        Some([b].as_slice())
    );
}

#[test]
fn manifest_membership_distinguishes_missing_and_decoded_empty_snapshots() {
    let source = manifest_configuration_source(
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="HI"/></Keywords>"#,
    );
    let scan = crate::test_support::container::scan(&source);
    for decoded_empty in [false, true] {
        let mut ir = named_configuration_ir();
        let mut partitions = vec![(1, Vec::new())];
        if decoded_empty {
            partitions.push((0, Vec::new()));
        }
        assign_configuration_bodies(&mut ir, partitions, Some(&scan)).unwrap();
        assert_eq!(ir.model.configurations.len(), 2);
        assert_eq!(ir.model.configurations[0].bodies.is_some(), decoded_empty);
        assert_eq!(
            ir.model.configurations[0].source_index,
            decoded_empty.then_some(0)
        );
        assert!(ir.model.configurations[1]
            .bodies
            .as_deref()
            .is_some_and(<[_]>::is_empty));
    }
}

#[test]
fn conflicting_manifest_binding_cannot_be_resurrected_by_active_fallback() {
    let source = manifest_configuration_source(br#"<Keywords><Configuration Name="Default" SourceIndex="1"/><Configuration Name="HI"/></Keywords>"#);
    let scan = crate::test_support::container::scan(&source);
    let mut ir = named_configuration_ir();
    ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("sldprt:test"),
        )),
        BTreeMap::from([
            (
                cadmpeg_core::nonblank_literal!("active_parasolid_block"),
                "Contents/Config-0-Partition".into(),
            ),
            (
                cadmpeg_core::nonblank_literal!("sw_configuration_name"),
                "Default".into(),
            ),
        ]),
    ));
    let a = BodyId::mint("test:model:body#unbound").expect("identity grammar");
    assign_configuration_bodies(
        &mut ir,
        vec![(0, vec![a.clone()]), (1, Vec::new())],
        Some(&scan),
    )
    .unwrap();
    assert!(ir.model.configurations[0].source_index.is_none());
    assert!(ir.model.configurations[0].bodies.is_none());
    let placeholder = ir
        .model
        .configurations
        .iter()
        .find(|c| c.source_index == Some(0))
        .expect("unbound source partition");
    assert!(placeholder.native_ref.is_none());
    assert_eq!(placeholder.bodies.as_deref(), Some([a].as_slice()));
}

#[test]
fn configuration_partitions_require_explicit_source_identity() {
    let mut ir = CadIr::empty();
    let configuration = |id: &str, ordinal, source_index| DesignConfiguration {
        id: ConfigurationId::mint(id).expect("identity grammar"),
        ordinal,
        active: false,
        source_index,
        name: Some(id.to_string()),
        material: None,
        properties: BTreeMap::new(),
        bodies: None,
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: Some(format!("native:{id}")),
    };
    ir.model
        .configurations
        .push(configuration("synthetic:test:id#explicit", 0, Some(5)));
    ir.model
        .configurations
        .push(configuration("synthetic:test:id#inferred", 9, None));
    ir.model
        .configurations
        .push(configuration("synthetic:test:id#empty", 10, Some(8)));
    let first = BodyId::mint("test:model:entity#body:first").expect("identity grammar");
    let second = BodyId::mint("test:model:entity#body:second").expect("identity grammar");
    let third = BodyId::mint("test:model:entity#body:third").expect("identity grammar");

    assign_configuration_bodies(
        &mut ir,
        vec![
            (7, vec![third.clone()]),
            (5, vec![first.clone()]),
            (5, vec![second.clone()]),
            (8, Vec::new()),
        ],
        None,
    )
    .unwrap();

    assert_eq!(ir.model.configurations[0].source_index, Some(5));
    assert_eq!(
        ir.model.configurations[0].bodies.as_deref(),
        Some([first, second].as_slice())
    );
    assert_eq!(ir.model.configurations[1].source_index, None);
    assert!(ir.model.configurations[1].bodies.is_none());
    assert_eq!(ir.model.configurations[2].source_index, Some(8));
    assert!(ir.model.configurations[2]
        .bodies
        .as_deref()
        .is_some_and(<[_]>::is_empty));
    assert_eq!(ir.model.configurations[3].source_index, Some(7));
    assert_eq!(
        ir.model.configurations[3].bodies.as_deref(),
        Some([third].as_slice())
    );
    assert!(ir.model.configurations[3].native_ref.is_none());
}

#[test]
fn duplicate_configuration_source_identity_does_not_select_a_partition() {
    let mut ir = CadIr::empty();
    for ordinal in 0..2 {
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId::mint(format!("synthetic:test:id#configuration:{ordinal}"))
                .expect("identity grammar"),
            ordinal,
            active: false,
            source_index: Some(5),
            name: format!("Configuration {ordinal}").into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: None,
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some(format!("native:{ordinal}")),
        });
    }
    let body = BodyId::mint("test:model:entity#body:partition").expect("identity grammar");

    assign_configuration_bodies(&mut ir, vec![(5, vec![body.clone()])], None).unwrap();

    assert!(ir.model.configurations[0].bodies.is_none());
    assert!(ir.model.configurations[1].bodies.is_none());
    assert_eq!(ir.model.configurations[2].source_index, Some(5));
    assert_eq!(
        ir.model.configurations[2].bodies.as_deref(),
        Some([body].as_slice())
    );
    assert!(ir.model.configurations[2].native_ref.is_none());
}

#[test]
fn inferred_partition_does_not_fabricate_active_configuration_identity() {
    let mut ir = CadIr::empty();
    ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("sldprt:test"),
        )),
        BTreeMap::from([
            (
                cadmpeg_core::nonblank_literal!("active_parasolid_block"),
                "Contents/Config-3-Partition".into(),
            ),
            (
                cadmpeg_core::nonblank_literal!("sw_configuration_name"),
                "Default".into(),
            ),
        ]),
    ));
    let body = BodyId::mint("test:model:entity#body:active").expect("identity grammar");

    assign_configuration_bodies(&mut ir, vec![(3, vec![body.clone()])], None).unwrap();
    mark_active_configuration(&mut ir);

    assert_eq!(ir.model.configurations.len(), 1);
    let configuration = &ir.model.configurations[0];
    assert!(!configuration.active);
    assert_eq!(configuration.source_index, Some(3));
    assert_eq!(configuration.bodies.as_deref(), Some([body].as_slice()));

    let mut report = super::empty_report(true);
    append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .unwrap();
    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "active configuration identity is unresolved; 0 of 1 configuration records are active."
    }));
}

#[test]
fn active_configuration_name_binds_partition_without_fabricating_body_membership() {
    let mut ir = CadIr::empty();
    ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("sldprt:test"),
        )),
        BTreeMap::from([
            (
                cadmpeg_core::nonblank_literal!("active_parasolid_block"),
                "Contents/Config-3-Partition".into(),
            ),
            (
                cadmpeg_core::nonblank_literal!("sw_configuration_name"),
                "Default".into(),
            ),
        ]),
    ));
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#configuration").expect("identity grammar"),
        ordinal: 0,
        active: false,
        source_index: None,
        name: Some("Default".to_string()),
        material: None,
        properties: BTreeMap::new(),
        bodies: None,
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: Some("native:configuration".into()),
    });

    assign_configuration_bodies(&mut ir, Vec::new(), None).unwrap();
    mark_active_configuration(&mut ir);

    let configuration = &ir.model.configurations[0];
    assert_eq!(configuration.source_index, Some(3));
    assert!(configuration.bodies.is_none());
    assert!(configuration.active);

    let mut report = super::empty_report(false);
    append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .unwrap();
    assert!(!report.losses.iter().any(|loss| {
        loss.message
            == "active configuration identity does not resolve to active geometry partition 3."
    }));
}

#[test]
fn duplicate_configuration_partition_identities_are_reported() {
    let mut ir = CadIr::empty();
    for id in ["first", "second"] {
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId::mint(format!("synthetic:test:id#{id}")).expect("identity grammar"),
            ordinal: u32::try_from(ir.model.configurations.len()).unwrap(),
            active: false,
            source_index: Some(5),
            name: Some(id.to_string()),
            material: None,
            properties: BTreeMap::new(),
            bodies: Some(cadmpeg_ir::features::DistinctMembers::default()),
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some(format!("native:{id}")),
        });
    }
    let mut report = super::empty_report(true);

    append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .unwrap();

    assert!(report.losses.iter().any(|loss| {
        loss.message == "2 configuration record(s) share non-unique geometry partition identities."
    }));
}

#[test]
fn incomplete_configuration_names_are_reported() {
    let mut ir = CadIr::empty();
    for (position, (ordinal, name)) in [(0, ""), (1, "Shared"), (2, "Shared"), (2, "Unique")]
        .into_iter()
        .enumerate()
    {
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId::mint(format!("synthetic:test:id#configuration:{position}"))
                .expect("identity grammar"),
            ordinal,
            active: position == 1,
            source_index: Some(u32::try_from(position).unwrap()),
            name: Some(name.to_string()),
            material: None,
            properties: BTreeMap::new(),
            bodies: Some(cadmpeg_ir::features::DistinctMembers::default()),
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some(format!("native:{position}")),
        });
    }
    let mut report = super::empty_report(true);

    append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .unwrap();

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 configuration record(s) have empty names; 2 configuration record(s) share non-unique names; 2 configuration record(s) share regeneration ordinals."
    }));
}

#[test]
fn active_configuration_partition_disagreement_is_reported() {
    let mut ir = CadIr::empty();
    ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("sldprt:test"),
        )),
        BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("active_parasolid_block"),
            "Contents/Config-3-Partition".into(),
        )]),
    ));
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#configuration").expect("identity grammar"),
        ordinal: 0,
        active: true,
        source_index: Some(5),
        name: Some("Default".to_string()),
        material: None,
        properties: BTreeMap::new(),
        bodies: Some(cadmpeg_ir::features::DistinctMembers::default()),
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: Some("native:configuration".into()),
    });
    let mut report = super::empty_report(true);

    append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .unwrap();

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "active configuration identity does not resolve to active geometry partition 3."
    }));
}

#[test]
fn incoherent_configuration_bodies_are_reported() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let configuration = |id: &str, ordinal, bodies| DesignConfiguration {
        id: ConfigurationId::mint(id).expect("identity grammar"),
        ordinal,
        active: ordinal == 0,
        source_index: Some(ordinal),
        name: Some(id.to_string()),
        material: None,
        properties: BTreeMap::new(),
        bodies,
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: Some(format!("native:{id}")),
    };
    ir.model.configurations = vec![
        configuration(
            "synthetic:test:id#duplicate",
            0,
            Some(
                cadmpeg_ir::features::DistinctMembers::try_from(vec![BodyId::mint(
                    "test:model:entity#another-missing-body",
                )
                .unwrap()])
                .unwrap(),
            ),
        ),
        configuration(
            "synthetic:test:id#missing",
            1,
            Some(
                cadmpeg_ir::features::DistinctMembers::try_from(vec![BodyId::mint(
                    "test:model:entity#missing-body",
                )
                .expect("identity grammar")])
                .unwrap(),
            ),
        ),
        configuration("synthetic:test:id#unresolved", 2, None),
    ];
    let mut report = super::empty_report(true);

    append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .unwrap();

    assert!(report.losses.iter().any(|loss| {
        loss.message == "1 configuration record(s) have unresolved body membership; 2 configuration record(s) contain missing or repeated body references."
    }));
}

#[test]
fn configuration_values_complete_parameters_without_baseline_values() {
    let mut ir = CadIr::empty();
    let parameter =
        ParameterId::mint("synthetic:test:id#configured-parameter").expect("identity grammar");
    ir.model.parameters.push(DesignParameter {
        id: parameter.clone(),
        owner: None,
        ordinal: 0,
        name: "Configured".into(),
        expression: "12mm".into(),
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
        name: Some("Default".to_string()),
        material: None,
        properties: BTreeMap::new(),
        bodies: Some(cadmpeg_ir::features::DistinctMembers::default()),
        parameter_values: BTreeMap::from([(
            parameter,
            ParameterValue::Length(Length::new(12.0).unwrap()),
        )]),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: Some("native:configuration".into()),
    });
    let mut report = super::empty_report(true);

    append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .unwrap();

    assert!(!report.losses.iter().any(|loss| {
        loss.message
            .contains("complete evaluated parameter snapshot")
            || loss.message.contains("lack an evaluated scalar")
    }));
}

#[test]
fn configuration_suppression_and_override_references_are_coherent() {
    let mut ir = CadIr::empty();
    let feature = FeatureId::mint("synthetic:test:id#feature").expect("identity grammar");
    let definition = FeatureDefinition::Operation(FeatureOperation::TreeNode {
        role: FeatureTreeNodeRole::History,
        children: cadmpeg_ir::features::TreeChildren::default(),
    });
    ir.model.features.push(Feature {
        id: feature.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition.clone()),
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#configuration").expect("identity grammar"),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: Some("Default".to_string()),
        material: None,
        properties: BTreeMap::new(),
        bodies: Some(cadmpeg_ir::features::DistinctMembers::default()),
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::from([(
            ParameterId::mint("synthetic:test:id#missing").expect("identity grammar"),
            "1mm".into(),
        )]),
        feature_states: BTreeMap::from([(
            feature,
            ConfigurationFeatureState {
                evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Suppressed {},
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                definition,
            },
        )]),
        native_ref: Some("native:configuration".into()),
    });
    let mut report = super::empty_report(true);

    append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .unwrap();

    assert!(report.losses.iter().any(|loss| {
        loss.message == "1 configuration(s) have missing, repeated, or feature-state-inconsistent suppression members; 1 configuration(s) reference missing parameter overrides."
    }));
}
