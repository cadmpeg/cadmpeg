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
fn configuration_feature_copy_design_refuses_collection_limit() {
    assert_projection_refusal(ResourceDimension::CollectionItems, run_design);
}
#[test]
fn configuration_feature_copy_design_refuses_retained_limit() {
    assert_projection_refusal(ResourceDimension::RetainedBytes, run_design);
}
#[test]
fn configuration_feature_copy_design_refuses_work_limit() {
    assert_projection_refusal(ResourceDimension::WorkUnits, run_design);
}
#[test]
fn configuration_feature_copy_supplemental_refuses_collection_limit() {
    assert_projection_refusal(ResourceDimension::CollectionItems, run_supplemental);
}
#[test]
fn configuration_feature_copy_supplemental_refuses_retained_limit() {
    assert_projection_refusal(ResourceDimension::RetainedBytes, run_supplemental);
}
#[test]
fn configuration_feature_copy_supplemental_refuses_work_limit() {
    assert_projection_refusal(ResourceDimension::WorkUnits, run_supplemental);
}
#[test]
fn configuration_feature_copy_topology_refuses_collection_limit() {
    assert_projection_refusal(ResourceDimension::CollectionItems, run_topology);
}
#[test]
fn configuration_feature_copy_topology_refuses_retained_limit() {
    assert_projection_refusal(ResourceDimension::RetainedBytes, run_topology);
}
#[test]
fn configuration_feature_copy_topology_refuses_work_limit() {
    assert_projection_refusal(ResourceDimension::WorkUnits, run_topology);
}
#[test]
fn configuration_feature_copy_sketch_refuses_collection_limit() {
    assert_projection_refusal(ResourceDimension::CollectionItems, run_sketch);
}
#[test]
fn configuration_feature_copy_sketch_refuses_retained_limit() {
    assert_projection_refusal(ResourceDimension::RetainedBytes, run_sketch);
}
#[test]
fn configuration_feature_copy_sketch_refuses_work_limit() {
    assert_projection_refusal(ResourceDimension::WorkUnits, run_sketch);
}
