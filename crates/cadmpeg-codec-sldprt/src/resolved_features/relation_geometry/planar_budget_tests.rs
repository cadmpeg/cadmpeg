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

fn project_owned_loci_with_policy(policy: &DecodePolicy) -> Result<(), CodecError> {
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition};
    use crate::records::{SketchInputEntity, SketchInputKind, SketchInputLink, SketchInputLinks};
    let sketch = SketchId::mint("synthetic:test:id#owned-sketch").unwrap();
    let mut entities = Vec::new();
    let mut markers = Vec::new();
    for (name, start, end) in [
        ("first", Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)),
        ("second", Point2::new(1.0, 0.0), Point2::new(1.0, 1.0)),
        ("third", Point2::new(1.0, 0.0), Point2::new(1.000000001, 0.0)),
    ] {
        let mut entity = SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#owned-line-{name}-with-retained-identity")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        );
        entity.native_ref = Some(name.into());
        if name == "third" {
            entity.endpoint_refs = vec!["canonical-point".into(), "canonical-point".into()];
        }
        entities.push(entity);
        markers.push(SketchInputEntity::new(name, "lane", 0, 0, SketchInputKind::LineOrCircle));
    }
    let mut point = SketchInputEntity::new("linked-point", "lane", 0, 0, SketchInputKind::Point);
    point.links = SketchInputLinks::new(0, vec![
        SketchInputLink { local_id: 1, entity_ref: "first".into() },
        SketchInputLink { local_id: 2, entity_ref: "second".into() },
    ]);
    markers.push(point);
    markers.push(SketchInputEntity::new("canonical-point", "lane", 0, 0, SketchInputKind::Point));
    let mut lane = relation_lane();
    lane.relation_instances[0].operands = vec![FeatureInputOperand {
        offset: 0,
        reference_ref: "canonical-reference".into(),
        kind: FeatureInputOperandKind::D6,
        entity_index: 0,
        entity_ref: Some("canonical-point".into()),
    }];
    lane.relation_instances[0].feature_ref = "unowned-feature".into();
    markers[0].feature_ref = Some("feature".into());
    markers[0].coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.0, 0.0]);
    lane.sketch_entities = markers;
    let (mut owner_sketch, mut owner, _) = planar_fixture();
    owner_sketch.id = sketch.clone();
    owner.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Sketch { sketch: SketchFeatureBinding::Planar(Some(sketch)) },
    ));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut constraints = Vec::new();
    project_relation_bindings(&ctx, &mut constraints, &[owner_sketch], &[owner], &entities, &[], &[lane])?;
    assert!(constraints.is_empty());
    Ok(())
}

fn assert_owned_loci_refusal(dimension: ResourceDimension) {
    let set_limit = |policy: &mut DecodePolicy, limit: u64| match dimension {
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
        ResourceDimension::WorkUnits => policy.limits.max_work_units = limit,
        _ => panic!("unexpected owned locus budget dimension"),
    };
    project_owned_loci_with_policy(&DecodePolicy::service()).unwrap();
    let mut upper = 1u64;
    loop {
        let mut policy = DecodePolicy::service();
        set_limit(&mut policy, upper);
        match project_owned_loci_with_policy(&policy) {
            Ok(()) => break,
            Err(CodecError::ResourceLimit(limit)) => assert_eq!(limit.dimension, dimension),
            Err(error) => panic!("unexpected projection error: {error}"),
        }
        upper = upper.checked_mul(2).unwrap();
    }
    let mut lower = 0;
    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        let mut policy = DecodePolicy::service();
        set_limit(&mut policy, middle);
        match project_owned_loci_with_policy(&policy) {
            Ok(()) => upper = middle,
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, dimension);
                lower = middle + 1;
            }
            Err(error) => panic!("unexpected projection error: {error}"),
        }
    }
    assert!(upper > 0);
    let mut policy = DecodePolicy::service();
    set_limit(&mut policy, upper);
    project_owned_loci_with_policy(&policy).unwrap();
    set_limit(&mut policy, upper - 1);
    assert!(matches!(project_owned_loci_with_policy(&policy), Err(CodecError::ResourceLimit(limit)) if limit.dimension == dimension));
}

#[test]
fn planar_relation_owned_loci_refuse_collection_limit() {
    assert_owned_loci_refusal(ResourceDimension::CollectionItems);
}

#[test]
fn planar_relation_owned_loci_refuse_retained_limit() {
    assert_owned_loci_refusal(ResourceDimension::RetainedBytes);
}

#[test]
fn planar_relation_owned_loci_refuse_work_limit() {
    assert_owned_loci_refusal(ResourceDimension::WorkUnits);
}
