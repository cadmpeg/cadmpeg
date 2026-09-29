use super::ownership_tests::relation_lane;
use super::{
    project_relation_bindings, project_relation_point_geometry, project_relation_solved_line_geometry,
    project_relation_solved_point_geometry,
};
use crate::records::{FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation, SketchFeatureBinding,
};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement, SketchProfiles};
use std::collections::BTreeMap;

fn planar_fixture() -> (Sketch, Feature, FeatureInputLane) {
    let sketch = Sketch {
        id: SketchId::mint("synthetic:test:id#sketch").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: SketchProfiles::default(),
        native_ref: Some("lane".into()),
    };
    let feature = Feature {
        id: FeatureId::mint("synthetic:test:id#feature").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch.id.clone())),
            },
        )),
        native_ref: Some("feature".into()),
    };
    let mut lane = relation_lane();
    lane.relation_instances[0].operands = vec![FeatureInputOperand {
        offset: 0,
        reference_ref: "reference".into(),
        kind: FeatureInputOperandKind::D6,
        entity_index: 0,
        entity_ref: None,
    }];
    (sketch, feature, lane)
}

fn project_with_policy(policy: DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"planar relation", &arena, &policy)?;
    let (sketch, feature, lane) = planar_fixture();
    let mut constraints = Vec::new();
    project_relation_bindings(
        &ctx,
        &mut constraints,
        &[sketch],
        &[feature],
        &[],
        &[],
        &[lane],
    )?;
    assert_eq!(constraints.len(), 1);
    Ok(())
}

fn project_solved_point_with_policy(policy: DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"solved point", &arena, &policy)?;
    let (sketch, feature, lane) = planar_fixture();
    let mut entities = Vec::new();
    project_relation_solved_point_geometry(
        &ctx,
        &mut entities,
        &[sketch],
        &[feature],
        &[],
        &[lane],
    )?;
    assert!(entities.is_empty());
    Ok(())
}

fn project_solved_line_with_policy(policy: DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"solved line", &arena, &policy)?;
    let (sketch, feature, lane) = planar_fixture();
    let mut entities = Vec::new();
    project_relation_solved_line_geometry(
        &ctx,
        &mut entities,
        &[sketch],
        &[feature],
        &[],
        &[lane],
    )?;
    assert!(entities.is_empty());
    Ok(())
}

fn project_relation_point_with_policy(policy: DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"relation point", &arena, &policy)?;
    let (sketch, feature, lane) = planar_fixture();
    let mut entities = Vec::new();
    project_relation_point_geometry(&ctx, &mut entities, &[sketch], &[feature], &[lane])?;
    assert!(entities.is_empty());
    Ok(())
}

#[test]
fn planar_relation_projection_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = project_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT planar relation sketches"));
}

#[test]
fn planar_relation_projection_refuses_retained_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = "relation".len() as u64;
    let error = project_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "format SLDPRT planar relation constraint identity"));
}

#[test]
fn planar_relation_projection_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error = project_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT planar relation sketches"));
}

#[test]
fn solved_point_projection_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = project_solved_point_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT solved-point sketches"));
}

#[test]
fn solved_point_projection_refuses_retained_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let error = project_solved_point_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "copy SLDPRT relation identity"));
}

#[test]
fn solved_point_projection_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error = project_solved_point_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT solved-point sketches"));
}

#[test]
fn solved_line_projection_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = project_solved_line_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT solved-line sketches"));
}

#[test]
fn solved_line_projection_refuses_retained_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let error = project_solved_line_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "copy SLDPRT planar sketch identity"));
}

#[test]
fn solved_line_projection_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error = project_solved_line_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT solved-line sketches"));
}

#[test]
fn relation_point_projection_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = project_relation_point_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT relation-point sketches"));
}

#[test]
fn relation_point_projection_refuses_retained_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let error = project_relation_point_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "copy SLDPRT planar sketch identity"));
}

#[test]
fn relation_point_projection_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error = project_relation_point_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT relation-point sketches"));
}
