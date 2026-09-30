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
    let mut translated = SketchInputEntity::new("translated-point", "lane", 0, 0, SketchInputKind::Point);
    translated.feature_ref = Some("feature".into());
    translated.coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.001, 0.0]);
    markers.push(translated);
    for (name, coordinates) in [("pair-start", [0.0, 0.0]), ("pair-end", [0.001, 0.0])] {
        let mut endpoint = SketchInputEntity::new(name, "lane", 0, 0, SketchInputKind::Point);
        endpoint.coordinates_m = cadmpeg_ir::units::FiniteVector::new(coordinates);
        markers.push(endpoint);
    }
    let mut handle = SketchInputEntity::new("pair-line-handle", "lane", 0, 0, SketchInputKind::LineOrCircle);
    handle.feature_ref = Some("feature".into());
    handle.links = SketchInputLinks::new(0, vec![
        SketchInputLink { local_id: 1, entity_ref: "pair-start".into() },
        SketchInputLink { local_id: 2, entity_ref: "pair-end".into() },
    ]);
    markers.push(handle);
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

fn assert_owned_loci_refusal(dimension: ResourceDimension, project: impl Fn(&DecodePolicy) -> Result<(), CodecError>) {
    let set_limit = |policy: &mut DecodePolicy, limit: u64| match dimension {
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
        ResourceDimension::WorkUnits => policy.limits.max_work_units = limit,
        ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = limit,
        _ => panic!("unexpected owned locus budget dimension"),
    };
    project(&DecodePolicy::service()).unwrap();
    let mut upper = 1u64;
    loop {
        let mut policy = DecodePolicy::service();
        set_limit(&mut policy, upper);
        match project(&policy) {
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
        match project(&policy) {
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
    project(&policy).unwrap();
    set_limit(&mut policy, upper - 1);
    assert!(matches!(project(&policy), Err(CodecError::ResourceLimit(limit)) if limit.dimension == dimension));
}

#[test]
fn planar_relation_owned_loci_refuse_collection_limit() {
    assert_owned_loci_refusal(ResourceDimension::CollectionItems, project_owned_loci_with_policy);
}

#[test]
fn planar_relation_owned_loci_refuse_retained_limit() {
    assert_owned_loci_refusal(ResourceDimension::RetainedBytes, project_owned_loci_with_policy);
}

#[test]
fn planar_relation_owned_loci_refuse_work_limit() {
    assert_owned_loci_refusal(ResourceDimension::WorkUnits, project_owned_loci_with_policy);
}

fn project_dimensional_with_policy(policy: &DecodePolicy) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{DesignParameter, DimensionDisplay, ParameterId, ParameterValue};
    use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition};
    let (sketch, feature, lane) = planar_fixture();
    let entity = SketchEntity::new(SketchEntityId::mint("synthetic:test:id#dimension-circle").unwrap(), sketch.id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: cadmpeg_ir::math::Point2::new(0.0, 0.0), radius: cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
        }).unwrap()).with_geometry_ref(Some("relation".into()));
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#dimension").unwrap(), owner: None, ordinal: 0,
        name: "D1".into(), expression: "2mm".into(), display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(cadmpeg_ir::scalar::Length::new(2.0).unwrap())),
        dependencies: DistinctMembers::default(), properties: BTreeMap::new(), pmi: None, native_ref: Some("scalar".into()),
    };
    let expected = SketchConstraintDefinitionInput::Diameter { entity: entity.id().clone(), parameter: parameter.id.clone() };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut constraints = Vec::new();
    project_relation_bindings(&ctx, &mut constraints, &[sketch], &[feature], &[entity], &[parameter], &[lane])?;
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].definition.kind(), &expected);
    Ok(())
}

#[test]
fn planar_dimensional_relation_refuses_collection_limit() {
    assert_owned_loci_refusal(ResourceDimension::CollectionItems, project_dimensional_with_policy);
}
#[test]
fn planar_dimensional_relation_refuses_retained_limit() {
    assert_owned_loci_refusal(ResourceDimension::RetainedBytes, project_dimensional_with_policy);
}
#[test]
fn planar_dimensional_relation_refuses_work_limit() {
    assert_owned_loci_refusal(ResourceDimension::WorkUnits, project_dimensional_with_policy);
}

fn project_dimensional_points_with_policy(policy: &DecodePolicy) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{DesignParameter, ParameterId, ParameterValue};
    use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchLocus};
    let (sketch, feature, mut lane) = planar_fixture();
    lane.relation_instances[0].family = crate::records::FeatureInputRelationFamily::PointPointDistance;
    lane.relation_instances[0].operands = (0u16..2).map(|index| FeatureInputOperand {
        offset: u64::from(index), reference_ref: format!("reference-{index}"), kind: FeatureInputOperandKind::D6,
        entity_index: index, entity_ref: None,
    }).collect();
    let entities: Vec<_> = (0..2).map(|index| {
        SketchEntity::new(SketchEntityId::mint(format!("synthetic:test:id#dimension-point-{index}")).unwrap(), sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position: cadmpeg_ir::math::Point2::new(f64::from(index) * 2.0, 0.0) }).unwrap())
            .with_geometry_ref(Some(format!("relation:operand:{index}")))
    }).collect();
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#point-dimension").unwrap(), owner: None, ordinal: 0,
        name: "D1".into(), expression: "2mm".into(), display: None,
        value: Some(ParameterValue::Length(cadmpeg_ir::scalar::Length::new(2.0).unwrap())),
        dependencies: DistinctMembers::default(), properties: BTreeMap::new(), pmi: None, native_ref: Some("scalar".into()),
    };
    let expected = SketchConstraintDefinitionInput::DistanceLoci {
        first: SketchLocus::Entity(entities[0].id().clone()), second: SketchLocus::Entity(entities[1].id().clone()), parameter: parameter.id.clone(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut constraints = Vec::new();
    project_relation_bindings(&ctx, &mut constraints, &[sketch], &[feature], &entities, &[parameter], &[lane])?;
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].definition.kind(), &expected);
    Ok(())
}

#[test]
fn planar_dimensional_points_refuse_collection_limit() {
    assert_owned_loci_refusal(ResourceDimension::CollectionItems, project_dimensional_points_with_policy);
}
#[test]
fn planar_dimensional_points_refuse_retained_limit() {
    assert_owned_loci_refusal(ResourceDimension::RetainedBytes, project_dimensional_points_with_policy);
}
#[test]
fn planar_dimensional_points_refuse_work_limit() {
    assert_owned_loci_refusal(ResourceDimension::WorkUnits, project_dimensional_points_with_policy);
}

fn project_dimensional_lines_with_policy(policy: &DecodePolicy) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{DesignParameter, ParameterId, ParameterValue};
    use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition};
    let (sketch, feature, mut lane) = planar_fixture();
    lane.relation_instances[0].family = crate::records::FeatureInputRelationFamily::LineLineDistance;
    lane.relation_instances[0].operands = (0u16..2).map(|index| FeatureInputOperand {
        offset: u64::from(index), reference_ref: format!("reference-{index}"), kind: FeatureInputOperandKind::D6,
        entity_index: index, entity_ref: None,
    }).collect();
    let entities: Vec<_> = (0..2).map(|index| {
        SketchEntity::new(SketchEntityId::mint(format!("synthetic:test:id#dimension-line-{index}")).unwrap(), sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start: cadmpeg_ir::math::Point2::new(0.0, f64::from(index) * 2.0), end: cadmpeg_ir::math::Point2::new(1.0, f64::from(index) * 2.0) }).unwrap())
            .with_geometry_ref(None)
    }).collect();
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#line-dimension").unwrap(), owner: None, ordinal: 0,
        name: "D1".into(), expression: "2mm".into(), display: None,
        value: Some(ParameterValue::Length(cadmpeg_ir::scalar::Length::new(2.0).unwrap())),
        dependencies: DistinctMembers::default(), properties: BTreeMap::new(), pmi: None, native_ref: Some("scalar".into()),
    };
    let expected = SketchConstraintDefinitionInput::Distance {
        entities: vec![entities[0].id().clone(), entities[1].id().clone()], parameter: parameter.id.clone(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut constraints = Vec::new();
    project_relation_bindings(&ctx, &mut constraints, &[sketch], &[feature], &entities, &[parameter], &[lane])?;
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].definition.kind(), &expected);
    Ok(())
}


#[test]
fn planar_dimensional_lines_refuse_collection_limit() {
    assert_owned_loci_refusal(ResourceDimension::CollectionItems, project_dimensional_lines_with_policy);
}

#[test]
fn planar_dimensional_lines_refuse_retained_limit() {
    assert_owned_loci_refusal(ResourceDimension::RetainedBytes, project_dimensional_lines_with_policy);
}

#[test]
fn planar_dimensional_lines_refuse_work_limit() {
    assert_owned_loci_refusal(ResourceDimension::WorkUnits, project_dimensional_lines_with_policy);
}

fn project_repeated_circles_with_policy(policy: &DecodePolicy) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{DesignParameter, DimensionDisplay, ParameterId, ParameterValue};
    use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition};
    let (sketch, feature, mut lane) = planar_fixture();
    lane.relation_instances[0].scalars = crate::records::relation_scalars::RelationScalars::from_refs(
        vec!["scalar".into(), "scalar-other".into()], Some("scalar".into()), None).unwrap();
    let entities: Vec<_> = (0..2).map(|index| SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#repeated-circle-{index}")).unwrap(), sketch.id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: cadmpeg_ir::math::Point2::new(f64::from(index) * 3.0, 0.0), radius: cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
        }).unwrap()).with_geometry_ref(Some("scalar".into()))).collect();
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#dimension").unwrap(), owner: None, ordinal: 0,
        name: "D1".into(), expression: "2mm".into(), display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(cadmpeg_ir::scalar::Length::new(2.0).unwrap())),
        dependencies: DistinctMembers::default(), properties: BTreeMap::new(), pmi: None, native_ref: Some("scalar".into()),
    };
    let expected = SketchConstraintDefinitionInput::RepeatedDiameter { entities: entities.iter().map(|entity| entity.id().clone()).collect(), parameter: parameter.id.clone() };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut constraints = Vec::new();
    project_relation_bindings(&ctx, &mut constraints, &[sketch], &[feature], &entities, &[parameter], &[lane])?;
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].definition.kind(), &expected);
    Ok(())
}


#[test]
fn planar_repeated_circles_refuse_collection_limit() {
    assert_owned_loci_refusal(ResourceDimension::CollectionItems, project_repeated_circles_with_policy);
}

#[test]
fn planar_repeated_circles_refuse_retained_limit() {
    assert_owned_loci_refusal(ResourceDimension::RetainedBytes, project_repeated_circles_with_policy);
}

#[test]
fn planar_repeated_circles_refuse_work_limit() {
    assert_owned_loci_refusal(ResourceDimension::WorkUnits, project_repeated_circles_with_policy);
}

fn project_dynamic_lines_with_policy(policy: &DecodePolicy) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{DesignParameter, ParameterId, ParameterValue};
    use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition};
    let (sketch, feature, mut lane) = planar_fixture();
    lane.relation_instances[0].family = crate::records::FeatureInputRelationFamily::LineLineDistance;
    lane.relation_instances[0].operands = (0u16..2).map(|index| FeatureInputOperand {
        offset: u64::from(index), reference_ref: format!("reference-{index}"), kind: FeatureInputOperandKind::Native(crate::records::operand_tag::NativeOperandTag::try_from(0x812a).unwrap()),
        entity_index: index, entity_ref: None,
    }).collect();
    let entities: Vec<_> = (0..2).map(|index| {
        SketchEntity::new(SketchEntityId::mint(format!("synthetic:test:id#dimension-line-{index}")).unwrap(), sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start: cadmpeg_ir::math::Point2::new(0.0, f64::from(index) * 2.0), end: cadmpeg_ir::math::Point2::new(1.0, f64::from(index) * 2.0) }).unwrap())
            .with_geometry_ref(Some(format!("feature:solver-line:{index}")))
    }).collect();
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#line-dimension").unwrap(), owner: None, ordinal: 0,
        name: "D1".into(), expression: "2mm".into(), display: None,
        value: Some(ParameterValue::Length(cadmpeg_ir::scalar::Length::new(2.0).unwrap())),
        dependencies: DistinctMembers::default(), properties: BTreeMap::new(), pmi: None, native_ref: Some("scalar".into()),
    };
    let expected = SketchConstraintDefinitionInput::Distance {
        entities: vec![entities[0].id().clone(), entities[1].id().clone()], parameter: parameter.id.clone(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut constraints = Vec::new();
    project_relation_bindings(&ctx, &mut constraints, &[sketch], &[feature], &entities, &[parameter], &[lane])?;
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].definition.kind(), &expected);
    Ok(())
}



#[test]
fn planar_dynamic_lines_refuse_collection_limit() {
    assert_owned_loci_refusal(ResourceDimension::CollectionItems, project_dynamic_lines_with_policy);
}

#[test]
fn planar_dynamic_lines_refuse_retained_limit() {
    assert_owned_loci_refusal(ResourceDimension::RetainedBytes, project_dynamic_lines_with_policy);
}

#[test]
fn planar_dynamic_lines_refuse_work_limit() {
    assert_owned_loci_refusal(ResourceDimension::WorkUnits, project_dynamic_lines_with_policy);
}

fn project_dynamic_marker_points_with_policy(policy: &DecodePolicy) -> Result<(), CodecError> {
    use crate::records::{SketchInputEntity, SketchInputKind, SketchInputLink, SketchInputLinks};
    use cadmpeg_ir::features::{DesignParameter, ParameterId, ParameterValue};
    use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchLocus};
    let (sketch, feature, mut lane) = planar_fixture();
    lane.relation_instances[0].family = crate::records::FeatureInputRelationFamily::PointPointDistance;
    lane.relation_instances[0].operands = (0u16..2).map(|index| FeatureInputOperand {
        offset: u64::from(index), reference_ref: format!("reference-{index}"),
        kind: FeatureInputOperandKind::Native(crate::records::operand_tag::NativeOperandTag::try_from(0x812a).unwrap()),
        entity_index: index, entity_ref: Some(format!("root-{index}")),
    }).collect();
    let mut entities = Vec::new();
    for index in 0u32..2 {
        let mut root = SketchInputEntity::new(format!("root-{index}"), "lane", index, u64::from(index), SketchInputKind::Point);
        root.feature_ref = Some("feature".into());
        root.links = SketchInputLinks::new(0, vec![SketchInputLink { local_id: 1, entity_ref: format!("child-{index}") }]);
        let child = SketchInputEntity::new(format!("child-{index}"), "lane", index + 2, u64::from(index) + 2, SketchInputKind::Point);
        lane.sketch_entities.extend([root, child]);
        let mut entity = SketchEntity::new(SketchEntityId::mint(format!("synthetic:test:id#marker-point-{index}")).unwrap(), sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position: cadmpeg_ir::math::Point2::new(f64::from(index) * 2.0, 0.0) }).unwrap());
        entity.native_ref = Some(format!("child-{index}"));
        entities.push(entity);
    }
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#marker-dimension").unwrap(), owner: None, ordinal: 0,
        name: "D1".into(), expression: "2mm".into(), display: None,
        value: Some(ParameterValue::Length(cadmpeg_ir::scalar::Length::new(2.0).unwrap())),
        dependencies: DistinctMembers::default(), properties: BTreeMap::new(), pmi: None, native_ref: Some("scalar".into()),
    };
    let expected = SketchConstraintDefinitionInput::DistanceLoci {
        first: SketchLocus::Entity(entities[0].id().clone()), second: SketchLocus::Entity(entities[1].id().clone()), parameter: parameter.id.clone(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut constraints = Vec::new();
    project_relation_bindings(&ctx, &mut constraints, &[sketch], &[feature], &entities, &[parameter], &[lane])?;
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].definition.kind(), &expected);
    Ok(())
}

#[test]
fn planar_dynamic_marker_points_refuse_collection_limit() {
    assert_owned_loci_refusal(ResourceDimension::CollectionItems, project_dynamic_marker_points_with_policy);
}
#[test]
fn planar_dynamic_marker_points_refuse_retained_limit() {
    assert_owned_loci_refusal(ResourceDimension::RetainedBytes, project_dynamic_marker_points_with_policy);
}
#[test]
fn planar_dynamic_marker_points_refuse_work_limit() {
    assert_owned_loci_refusal(ResourceDimension::WorkUnits, project_dynamic_marker_points_with_policy);
}
#[test]
fn planar_dynamic_marker_points_refuse_nesting_limit() {
    assert_owned_loci_refusal(ResourceDimension::RecursionDepth, project_dynamic_marker_points_with_policy);
}

fn project_native_marker_with_policy(policy: &DecodePolicy) -> Result<(), CodecError> {
    use crate::records::{SketchInputEntity, SketchInputKind, SketchInputLink, SketchInputLinks, SketchRelationKind};
    let (sketch, feature, mut lane) = planar_fixture();
    lane.relation_instances.clear();
    let mut relation = SketchInputEntity::new("opaque-relation", "lane", 0, 0, SketchInputKind::Relation(SketchRelationKind::OffsetEdge));
    relation.feature_ref = Some("feature".into());
    relation.links = SketchInputLinks::new(0, vec![SketchInputLink { local_id: 3, entity_ref: "opaque-point".into() }]);
    lane.sketch_entities = vec![relation, SketchInputEntity::new("opaque-point", "lane", 1, 1, SketchInputKind::Point)];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut constraints = Vec::new();
    project_relation_bindings(&ctx, &mut constraints, &[sketch], &[feature], &[], &[], &[lane])?;
    assert_eq!(constraints.len(), 1);
    let cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Native { entities, operands, .. } = constraints[0].definition.kind() else { panic!("native marker relation"); };
    assert!(entities.is_empty());
    assert_eq!(operands, &vec![cadmpeg_ir::sketches::SketchNativeOperand {
        native_kind: cadmpeg_core::nonblank_literal!("sldprt:marker-local-id"), field: None,
        object_index: Some(3), native_ref: Some("opaque-point".into()),
    }]);
    Ok(())
}
#[test]
fn planar_native_marker_refuses_collection_limit() {
    assert_owned_loci_refusal(ResourceDimension::CollectionItems, project_native_marker_with_policy);
}
#[test]
fn planar_native_marker_refuses_retained_limit() {
    assert_owned_loci_refusal(ResourceDimension::RetainedBytes, project_native_marker_with_policy);
}
#[test]
fn planar_native_marker_refuses_work_limit() {
    assert_owned_loci_refusal(ResourceDimension::WorkUnits, project_native_marker_with_policy);
}

mod roster_points;

mod address_markers;

mod axis_markers;

mod native_markers;
