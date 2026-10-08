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

fn configuration_body<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    source: &BodyId,
) -> crate::decode::ConfigurationBodyIdentity<'ctx> {
    let (id, storage) = ctx
        .with_scoped_storage("retain SLDPRT body ID", || {
            source.try_clone_for_decode(ctx, "retain SLDPRT body ID")
        })
        .unwrap();
    crate::decode::ConfigurationBodyIdentity { id, storage }
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

    let ctx = cadmpeg_test_support::service_decode_context();
    assign_configuration_bodies(
        &ctx,
        &mut ir,
        vec![
            (7, vec![configuration_body(&ctx, &third)]),
            (5, vec![configuration_body(&ctx, &first)]),
            (5, vec![configuration_body(&ctx, &second)]),
        ],
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

    let ctx = cadmpeg_test_support::service_decode_context();
    assign_configuration_bodies(
        &ctx,
        &mut ir,
        vec![(5, vec![configuration_body(&ctx, &body)])],
    )
    .unwrap();

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

    let ctx = cadmpeg_test_support::service_decode_context();
    assign_configuration_bodies(
        &ctx,
        &mut ir,
        vec![(3, vec![configuration_body(&ctx, &body)])],
    )
    .unwrap();
    mark_active_configuration(&cadmpeg_test_support::service_decode_context(), &mut ir).unwrap();

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

    assign_configuration_bodies(
        &cadmpeg_test_support::service_decode_context(),
        &mut ir,
        Vec::new(),
    )
    .unwrap();
    mark_active_configuration(&cadmpeg_test_support::service_decode_context(), &mut ir).unwrap();

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
fn configuration_loss_counting_refuses_caller_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut ir = CadIr::empty();
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#configuration").expect("identity grammar"),
        ordinal: 0,
        active: true,
        source_index: Some(5),
        name: Some("Default".into()),
        material: None,
        properties: BTreeMap::new(),
        bodies: None,
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: Some("native:configuration".into()),
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
    let mut report = super::empty_report(true);
    let error = append_design_losses(&ctx, &ir, &mut report)
        .expect_err("configuration source index consumes one collection item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "count SLDPRT configuration source indices"
    ));
}

#[test]
fn design_loss_note_refuses_caller_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut ir = CadIr::empty();
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#inactive-configuration")
            .expect("identity grammar"),
        ordinal: 0,
        active: false,
        source_index: None,
        name: None,
        material: None,
        properties: BTreeMap::new(),
        bodies: None,
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
    let mut report = super::empty_report(true);
    let error = append_design_losses(&ctx, &ir, &mut report)
        .expect_err("the first design loss consumes one collection item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "append SLDPRT decode loss"
    ));
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
                cadmpeg_ir::features::DistinctMembers::try_from(
                    vec![BodyId::mint("test:model:entity#another-missing-body").unwrap()],
                    &cadmpeg_test_support::service_decode_context(),
                )
                .unwrap(),
            ),
        ),
        configuration(
            "synthetic:test:id#missing",
            1,
            Some(
                cadmpeg_ir::features::DistinctMembers::try_from(
                    vec![BodyId::mint("test:model:entity#missing-body").expect("identity grammar")],
                    &cadmpeg_test_support::service_decode_context(),
                )
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

#[cfg(target_pointer_width = "64")]
#[test]
fn out_of_range_partition_body_identity_is_not_retained() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let body = cadmpeg_ir::topology::Body {
        id: BodyId::mint("test:model:entity#body:unclaimed").unwrap(),
        kind: cadmpeg_ir::topology::BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let mut workspace = ctx
        .reserve_scoped(0, "configuration body copy test workspace")
        .unwrap();
    let identities = crate::decode::copy_body_ids(&ctx, &[body], &mut workspace).unwrap();
    assign_configuration_bodies(
        &ctx,
        &mut ir,
        vec![(usize::try_from(4_294_967_296_u64).unwrap(), identities)],
    )
    .unwrap();
    assert!(ir.model.configurations.is_empty());
    drop(workspace);
    ctx.finish_session().unwrap();
}

#[test]
fn duplicate_configuration_body_keys_release_lookup_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let body = BodyId::mint("test:model:entity#body:repeated").unwrap();
    let arena = DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "append SLDPRT partition configuration",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let bodies = (0..2).map(|_| configuration_body(&ctx, &body)).collect();
            ctx.with_scoped_storage("configuration output test workspace", || {
                assign_configuration_bodies(&ctx, &mut CadIr::empty(), vec![(5, bodies)])
            })
            .map(drop)
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("configuration output boundary");
    };
    // Only the partition map, its output vector and its first identity remain live.
    let map_node = 11 * (std::mem::size_of::<u32>() + std::mem::size_of::<Vec<BodyId>>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<Vec<BodyId>>();
    let output_vector = 4 * std::mem::size_of::<BodyId>();
    assert_eq!(
        limit.used,
        u64::try_from(map_node + output_vector + body.as_str().len()).unwrap()
    );
}
