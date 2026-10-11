// SPDX-License-Identifier: Apache-2.0

use super::{assert_projection_refusal, design_configuration, feature_input_lane};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    ConfigurationEvaluation, ConfigurationFeatureState, DistinctMembers, Feature, FeatureContent,
    FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation, FeatureSourceContent,
};
use std::collections::BTreeMap;

fn model() -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let id = FeatureId::mint("synthetic:test:id#copied-feature").unwrap();
    let dependency = FeatureId::mint("synthetic:test:id#copied-dependency").unwrap();
    let definition = FeatureDefinition::Operation(FeatureOperation::Native {
        kind: "Retained native operation".into(),
        parameters: BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("Native parameter"),
            "Parameter text".into(),
        )]),
    });
    let outputs: DistinctMembers<cadmpeg_ir::ids::BodyId> =
        cadmpeg_ir::features::DistinctMembers::try_from(
            vec![cadmpeg_ir::ids::BodyId::mint("synthetic:test:id#copied-body").unwrap()],
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap();
    let dependencies: DistinctMembers<FeatureId> = cadmpeg_ir::features::DistinctMembers::try_from(
        vec![dependency.clone()],
        &cadmpeg_test_support::service_decode_context(),
    )
    .unwrap();
    let state = ConfigurationFeatureState {
        definition: definition.clone(),
        dependencies: dependencies.clone(),
        evaluation: ConfigurationEvaluation::Active {
            outputs: outputs.clone(),
        },
    };
    ir.model.features.push(Feature {
        id: id.clone(),
        ordinal: 0,
        name: Some("Retained feature name".into()),
        suppressed: Some(false),
        dependencies,
        source_properties: BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("Source property"),
            "Property text".into(),
        )]),
        source_tag: Some("Source tag".into()),
        source_text: Some("Source text".into()),
        source_content: FeatureContent::try_from(vec![
            FeatureSourceContent::Text("Content text".into()),
            FeatureSourceContent::Feature(dependency),
        ])
        .unwrap(),
        evaluation: FeatureEvaluation::new(definition, outputs),
        native_ref: Some("Native feature reference".into()),
    });
    let mut configuration = design_configuration("copied-state", 0, Some(0), None);
    configuration.bodies = None;
    configuration.feature_states.insert(id, state);
    ir.model.configurations.push(configuration);
    ir
}

fn run_design(policy: &DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut ir = model();
    let features = ir.model.features.clone();
    super::super::project_configuration_design_states(
        &ctx,
        &mut ir,
        &[],
        &[feature_input_lane("lane", Some("0"))],
        &[],
        None,
    )?;
    assert_eq!(ir.model.features, features);
    assert!(ir.model.configurations[0].feature_states.is_empty());
    Ok(())
}

fn run_design_with_feature(policy: &DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut ir = model();
    ir.model.features[0].name = Some("x".repeat(32 * 1024));
    let base_features = ir.model.features.clone();
    let histories = [crate::records::FeatureHistory {
        id: "source".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![crate::history::tests::feature("local", None, 0)],
    }];
    super::super::project_configuration_design_states(
        &ctx,
        &mut ir,
        &histories,
        &[feature_input_lane("lane", Some("0"))],
        &[],
        None,
    )?;
    assert_eq!(ir.model.features, base_features);
    assert_eq!(ir.model.configurations[0].feature_states.len(), 1);
    Ok(())
}

fn run_design_with_projected_feature(policy: &DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut ir = model();
    let original_features = ir.model.features.clone();
    let mut projected_feature = crate::history::tests::feature("local", None, 0);
    projected_feature.name = "x".repeat(32 * 1024);
    let histories = [crate::records::FeatureHistory {
        id: "source".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![projected_feature],
    }];
    super::super::project_configuration_design_states(
        &ctx,
        &mut ir,
        &histories,
        &[feature_input_lane("lane", Some("0"))],
        &[],
        None,
    )?;
    assert_eq!(ir.model.features, original_features);
    assert_eq!(ir.model.configurations[0].feature_states.len(), 1);
    Ok(())
}

fn run_supplemental(policy: &DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut ir = model();
    let expected = ir.clone();
    super::super::project_configuration_supplemental_edge_selections(
        &ctx,
        &mut ir,
        &[feature_input_lane(
            "sldprt:feature-input:config-objects#0",
            Some("0"),
        )],
    )?;
    assert_eq!(ir, expected);
    Ok(())
}

fn run_topology(policy: &DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut ir = model();
    let expected = ir.clone();
    super::super::bind_configuration_topology_selections(
        &ctx,
        &mut ir,
        &[],
        &[feature_input_lane("lane", Some("0"))],
        &[],
    )?;
    assert_eq!(ir, expected);
    Ok(())
}

fn run_sketch(policy: &DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut ir = model();
    let expected = ir.clone();
    let losses = super::super::project_configuration_sketch_states(
        &ctx,
        &mut ir,
        &[],
        &[feature_input_lane("lane", Some("0"))],
        &mut cadmpeg_ir::Annotations::default(),
    )?;
    assert!(losses.is_empty());
    assert_eq!(ir, expected);
    Ok(())
}

#[test]
fn configuration_feature_copy_supplemental_refuses_collection_limit() {
    assert_projection_refusal(
        ResourceDimension::CollectionItems,
        "retain SLDPRT configuration features",
        run_supplemental_with_edges,
    );
}
#[test]
fn configuration_feature_copy_supplemental_refuses_retained_limit() {
    assert_projection_refusal(
        ResourceDimension::RetainedBytes,
        "retain SLDPRT supplemental configuration state",
        run_supplemental_with_edges,
    );
}
#[test]
fn configuration_feature_copy_supplemental_refuses_work_limit() {
    assert_projection_refusal(
        ResourceDimension::WorkUnits,
        "retain SLDPRT configuration features",
        run_supplemental_with_edges,
    );
}
#[test]
fn configuration_feature_copy_topology_refuses_collection_limit() {
    assert_projection_refusal(
        ResourceDimension::CollectionItems,
        "retain SLDPRT evaluated configuration features",
        run_topology,
    );
}
#[test]
fn configuration_feature_copy_topology_refuses_retained_limit() {
    assert_projection_refusal(
        ResourceDimension::RetainedBytes,
        "retain SLDPRT configuration feature state",
        run_topology,
    );
}
#[test]
fn configuration_feature_copy_topology_refuses_work_limit() {
    assert_projection_refusal(
        ResourceDimension::WorkUnits,
        "retain SLDPRT evaluated configuration features",
        run_topology,
    );
}
#[test]
fn configuration_feature_copy_sketch_refuses_collection_limit() {
    assert_projection_refusal(
        ResourceDimension::CollectionItems,
        "retain SLDPRT evaluated configuration features",
        run_sketch,
    );
}
#[test]
fn configuration_feature_copy_sketch_refuses_retained_limit() {
    assert_projection_refusal(
        ResourceDimension::RetainedBytes,
        "retain SLDPRT configuration feature state",
        run_sketch,
    );
}
#[test]
fn configuration_feature_copy_sketch_refuses_work_limit() {
    assert_projection_refusal(
        ResourceDimension::WorkUnits,
        "retain SLDPRT evaluated configuration features",
        run_sketch,
    );
}

#[test]
fn configuration_design_skips_base_features_without_projected_features() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    run_design(&policy).expect("empty feature projection must not copy the base model");

    policy.limits.max_materialized_bytes = u64::MAX;
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::MaterializedBytes,
        "retain SLDPRT configuration features",
        None,
    );
    run_design(&policy).expect("empty feature projection must not copy the base model");
}

#[test]
fn configuration_design_skips_base_features_without_inheritance_consumers() {
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::RetainedBytes,
        ResourceDimension::WorkUnits,
    ] {
        let mut policy = DecodePolicy::service();
        super::set_limit(&mut policy, dimension, u64::MAX);
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            dimension,
            "retain SLDPRT configuration features",
            None,
        );
        run_design_with_feature(&policy)
            .expect("custom local feature has no inheritance consumer");
    }
}

#[test]
fn configuration_design_projects_only_retained_feature_state() {
    assert_projection_refusal(
        ResourceDimension::MaterializedBytes,
        "copy SLDPRT projected feature text",
        run_design_with_projected_feature,
    );
    assert_projection_refusal(
        ResourceDimension::RetainedBytes,
        "retain SLDPRT configuration feature identity",
        run_design_with_projected_feature,
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::MAX;
    policy.limits.max_materialized_bytes = u64::MAX;
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::RetainedBytes,
        "copy SLDPRT projected feature text",
        None,
    );
    run_design_with_projected_feature(&policy)
        .expect("projected feature name is temporary and is not retained in state");
}

#[test]
fn unassigned_configuration_design_skips_base_features_and_clears_states() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    policy.limits.max_work_units = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = model();
    ir.model.features[0].name = Some("x".repeat(8192));
    let features = ir.model.features.clone();
    super::super::project_configuration_design_states(&ctx, &mut ir, &[], &[], &[], None).unwrap();
    assert_eq!(ir.model.features, features);
    assert!(ir.model.configurations[0].feature_states.is_empty());
    assert!(ir.model.configurations[0].parameter_values.is_empty());
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn configuration_parameter_metadata_uses_scoped_storage() {
    let mut owner = crate::history::tests::feature("owner", Some("1"), 0);
    owner
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Width"), "1".into());
    owner.dimension_properties.insert(
        "Width".into(),
        BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("Metadata"),
            "x".repeat(32 * 1024),
        )]),
    );
    let histories = [crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![owner],
    }];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The configuration keeps an integer and its identity, not the metadata text.
    policy.limits.max_retained_bytes = 16 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model
        .configurations
        .push(design_configuration("values", 0, Some(0), None));
    super::super::project_configuration_design_states(
        &ctx,
        &mut ir,
        &histories,
        &[feature_input_lane("lane", Some("0"))],
        &[],
        None,
    )
    .unwrap();
    let values = &ir.model.configurations[0].parameter_values;
    assert_eq!(values.len(), 1);
    assert_eq!(
        values.values().next(),
        Some(&cadmpeg_ir::features::ParameterValue::Integer(1))
    );
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn configuration_feature_metadata_is_scoped_in_each_projection() {
    for project in [run_topology, run_sketch, run_supplemental, run_supplemental_with_edges] {
        for operation in [
            "retain SLDPRT evaluated configuration features",
            "retain SLDPRT configuration features",
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = u64::MAX;
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                ResourceDimension::RetainedBytes,
                operation,
                None,
            );
            project(&policy)
                .expect("temporary feature metadata must not be retained");
        }
    }
}

#[test]
fn supplemental_edges_do_not_copy_unread_metadata_or_overlaid_model_definition() {
    let mut ir = model();
    let unused = "x".repeat(17 * 1024 * 1024);
    ir.model.features[0].name = Some(unused.clone());
    ir.model.features[0].source_properties.insert(
        cadmpeg_core::nonblank_literal!("Unused source property"),
        unused.clone(),
    );
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
            kind: "Overlaid model operation".into(),
            parameters: BTreeMap::from([(
                cadmpeg_core::nonblank_literal!("Unused model parameter"),
                unused,
            )]),
        }));
    let expected = ir.clone();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    super::super::project_configuration_supplemental_edge_selections(
        &ctx,
        &mut ir,
        &[feature_input_lane(
            "sldprt:feature-input:config-objects#0",
            Some("0"),
        )],
    )
    .unwrap();
    assert_eq!(ir, expected);
    ctx.finish_session().unwrap();
}

fn run_operation_base(policy: &DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut native = crate::history::tests::feature("extrusion", Some("1"), 0);
    native.kind = "Extrusion".into();
    native.xml_tag = "Extrusion".into();
    native
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Depth"), "1mm".into());
    let histories = [crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![native],
    }];
    let fixture_ctx = cadmpeg_test_support::service_decode_context();
    let local = crate::history::project::project_features(&fixture_ctx, &histories)?;
    assert!(matches!(
        local[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            op: cadmpeg_ir::features::BooleanOp::Unresolved,
            ..
        })
    ));
    let mut model = local.clone();
    model[0].name = Some("x".repeat(32 * 1024));
    let base =
        super::super::configuration_base_features(&ctx, &model, &local, &histories, &[], None)?;
    assert_eq!(base.features.len(), 1);
    assert_eq!(
        base.features[0].evaluation.definition(),
        model[0].evaluation.definition()
    );
    assert!(base.features[0].name.is_none());
    drop(base);
    ctx.finish_session()?;
    Ok(())
}

#[test]
fn configuration_operation_base_uses_scoped_storage() {
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
    ] {
        assert_projection_refusal(
            dimension,
            "retain SLDPRT configuration features",
            run_operation_base,
        );
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::MAX;
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::RetainedBytes,
        "retain SLDPRT configuration features",
        None,
    );
    run_operation_base(&policy).expect("operation base is temporary");
}

#[test]
fn configuration_duplicate_records_retain_only_last_valued_parameter_and_last_state() {
    let mut first = crate::history::tests::feature("duplicate", None, 0);
    first
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Value"), "1".into());
    let mut second = first.clone();
    second
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Value"), "2".into());
    let mut last = first.clone();
    last.parameters.insert(
        cadmpeg_core::nonblank_literal!("Value"),
        "Missing + 1".into(),
    );
    last.kind = "CustomLast".into();
    last.suppressed = true;
    let mut features = vec![first; 64];
    features.extend([second, last]);
    let histories = [crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features,
    }];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One configuration-state B-tree node holds the inline operation variants.
    // This cap admits that published node but excludes 66 retained state copies.
    policy.limits.max_retained_bytes = 16 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model
        .configurations
        .push(design_configuration("duplicates", 0, Some(0), None));
    super::super::project_configuration_design_states(
        &ctx,
        &mut ir,
        &histories,
        &[feature_input_lane("lane", Some("0"))],
        &[],
        None,
    )
    .unwrap();
    assert_eq!(ir.model.configurations[0].parameter_values.len(), 1);
    assert_eq!(
        ir.model.configurations[0].parameter_values.values().next(),
        Some(&cadmpeg_ir::features::ParameterValue::Integer(2))
    );
    assert_eq!(ir.model.configurations[0].feature_states.len(), 1);
    let state = ir.model.configurations[0]
        .feature_states
        .values()
        .next()
        .unwrap();
    assert!(
        matches!(&state.definition, FeatureDefinition::Operation(FeatureOperation::Native { kind, parameters }) if kind.as_str() == "CustomLast" && parameters["Value"] == "2")
    );
    assert!(matches!(
        state.evaluation,
        ConfigurationEvaluation::Suppressed {}
    ));
    ctx.finish_session().unwrap();
}

fn run_supplemental_with_edges(policy: &DecodePolicy) -> Result<(), CodecError> {
    use cadmpeg_ir::features::edge_treatments::{ChamferGroup, ChamferSpec};
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut ir = model();
    let id = ir.model.features[0].id.clone();
    let definition = FeatureDefinition::Operation(FeatureOperation::Chamfer {
        groups: cadmpeg_ir::features::NonEmptyMembers::one(ChamferGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
            spec: ChamferSpec::Distance {
                distance: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
            },
        }),
        flip_direction: false,
    });
    ir.model.configurations[0].feature_states.get_mut(&id).unwrap().definition = definition;
    let expected = ir.clone();
    let mut lane = feature_input_lane("sldprt:feature-input:config-objects#0", Some("0"));
    lane.edge_selections.push(crate::records::FeatureInputEdgeSelection {
        id: "selection".into(),
        parent: lane.id.clone(),
        ordinal: 0,
        offset: 100,
        object_name_ref: "name".into(),
        feature_ref: ir.model.features[0].native_ref.clone().unwrap(),
        local_edge_ids: vec![7],
        components: Vec::new(),
        references: Vec::new(),
        producer_feature_refs: Vec::new(),
        terminal_feature_ref: None,
    });
    super::super::project_configuration_supplemental_edge_selections(&ctx, &mut ir, &[lane])?;
    assert_eq!(ir.model.features, expected.model.features);
    let state = &ir.model.configurations[0].feature_states[&id];
    let original = &expected.model.configurations[0].feature_states[&id];
    assert_eq!(state.dependencies, original.dependencies);
    assert_eq!(state.evaluation, original.evaluation);
    assert!(matches!(&state.definition,
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
            if matches!(&groups[0].edges, cadmpeg_ir::features::EdgeSelection::Native(_))));
    ctx.finish_session()
}

#[test]
fn empty_supplemental_lanes_do_not_copy_or_publish_states() {
    for operation in [
        "retain SLDPRT configuration features",
        "retain SLDPRT configuration feature state",
        "retain SLDPRT supplemental configuration state",
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits, operation, None,
        );
        run_supplemental(&policy).expect("empty edge lane leaves every state unchanged");
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = model();
    let expected = ir.clone();
    let lanes = vec![feature_input_lane("sldprt:feature-input:config-objects#0", Some("0")); 64];
    super::super::project_configuration_supplemental_edge_selections(&ctx, &mut ir, &lanes).unwrap();
    assert_eq!(ir, expected);
    ctx.finish_session().unwrap();
}

#[test]
fn supplemental_edges_do_not_republish_unchanged_fields() {
    // Native operations cannot consume these edges. Projection must not copy an
    // identical definition or revalidate its already-distinct dependencies.
    let mut ir = model();
    let expected = ir.clone();
    let mut lane = feature_input_lane("sldprt:feature-input:config-objects#0", Some("0"));
    lane.edge_selections.push(crate::records::FeatureInputEdgeSelection {
        id: "selection".into(),
        parent: lane.id.clone(),
        ordinal: 0,
        offset: 100,
        object_name_ref: "name".into(),
        feature_ref: ir.model.features[0].native_ref.clone().unwrap(),
        local_edge_ids: vec![7],
        components: Vec::new(),
        references: Vec::new(),
        producer_feature_refs: Vec::new(),
        terminal_feature_ref: None,
    });
    for operation in ["retain SLDPRT supplemental configuration state", "validate distinct decoded members"] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits, operation, None,
        );
        super::super::project_configuration_supplemental_edge_selections(&ctx, &mut ir, std::slice::from_ref(&lane)).unwrap();
        assert_eq!(ir, expected);
        ctx.finish_session().unwrap();
    }
}
