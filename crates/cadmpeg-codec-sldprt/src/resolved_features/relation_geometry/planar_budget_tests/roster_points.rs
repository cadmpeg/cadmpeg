use super::{assert_owned_loci_refusal, planar_fixture, project_relation_bindings};
use crate::records::{FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId, ParameterValue};
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchLocus};
use std::collections::BTreeMap;

fn project_roster_points_with_policy(policy: &DecodePolicy) -> Result<(), CodecError> {
    let (sketch, feature, mut lane) = planar_fixture();
    lane.relation_instances[0].family = FeatureInputRelationFamily::PointPointDistance;
    lane.relation_instances[0].operands = (0u16..2).map(|index| FeatureInputOperand {
        offset: u64::from(index), reference_ref: format!("reference-{index}"),
        kind: FeatureInputOperandKind::Native(crate::records::operand_tag::NativeOperandTag::try_from(0x812a).unwrap()),
        entity_index: index, entity_ref: None,
    }).collect();
    let entities: Vec<_> = (0..2).map(|index| SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#roster-point-{index}")).unwrap(), sketch.id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: cadmpeg_ir::math::Point2::new(f64::from(index) * 2.0, 0.0),
        }).unwrap(),
    )).collect();
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#roster-dimension").unwrap(), owner: None, ordinal: 0,
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
fn planar_roster_points_refuse_collection_limit() {
    assert_owned_loci_refusal(ResourceDimension::CollectionItems, project_roster_points_with_policy);
}
#[test]
fn planar_roster_points_refuse_retained_limit() {
    assert_owned_loci_refusal(ResourceDimension::RetainedBytes, project_roster_points_with_policy);
}
#[test]
fn planar_roster_points_refuse_work_limit() {
    assert_owned_loci_refusal(ResourceDimension::WorkUnits, project_roster_points_with_policy);
}
