use super::{assert_owned_loci_refusal, planar_fixture, project_relation_bindings};
use crate::records::{
    FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily, SketchInputEntity,
    SketchInputKind,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DesignParameter, DistinctMembers, FeatureDefinition, FeatureEvaluation, FeatureOperation,
    ParameterId, ParameterValue, SketchFeatureBinding,
};
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchLocus,
};
use std::collections::BTreeMap;

fn project_compact_operand_markers_with_policy(policy: &DecodePolicy) -> Result<(), CodecError> {
    let (mut sketch, mut feature, mut lane) = planar_fixture();
    sketch.id = SketchId::mint("sldprt:model:sketch#compact:lane:1").unwrap();
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(sketch.id.clone())),
        },
    ));
    lane.sketch_entities.clear();
    lane.relation_instances[0].family = FeatureInputRelationFamily::PointPointDistance;
    lane.relation_instances[0].operands = (0u16..2)
        .map(|index| FeatureInputOperand {
            offset: u64::from(index),
            reference_ref: format!("reference-{index}"),
            kind: FeatureInputOperandKind::D6,
            entity_index: index,
            entity_ref: None,
        })
        .collect();
    let mut entities = Vec::new();
    for index in 0u32..2 {
        let marker_id = format!("point-{index}");
        let mut marker = SketchInputEntity::new(
            marker_id.clone(),
            "lane",
            index,
            u64::from(index),
            SketchInputKind::Point,
        );
        marker.feature_ref = Some("feature".into());
        marker.coordinates_m =
            cadmpeg_ir::units::FiniteVector::new([f64::from(index) * 0.002, 0.0]);
        lane.sketch_entities.push(marker);
        entities.push(
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:id#compact-point-{index}")).unwrap(),
                sketch.id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: cadmpeg_ir::math::Point2::new(f64::from(index) * 2.0, 0.0),
                })
                .unwrap(),
            )
            .with_native_ref(Some(marker_id)),
        );
    }
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#compact-dimension").unwrap(),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "2mm".into(),
        display: None,
        value: Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(2.0).unwrap(),
        )),
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("scalar".into()),
    };
    let expected = SketchConstraintDefinitionInput::DistanceLoci {
        first: SketchLocus::Entity(entities[0].id().clone()),
        second: SketchLocus::Entity(entities[1].id().clone()),
        parameter: parameter.id.clone(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut constraints = Vec::new();
    project_relation_bindings(
        &ctx,
        &mut constraints,
        &[sketch],
        &[feature],
        &entities,
        &[parameter],
        &[lane],
    )?;
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].definition.kind(), &expected);
    Ok(())
}

#[test]
fn planar_compact_operand_markers_refuse_collection_limit() {
    assert_owned_loci_refusal(
        ResourceDimension::CollectionItems,
        project_compact_operand_markers_with_policy,
    );
}
#[test]
fn planar_compact_operand_markers_refuse_retained_limit() {
    assert_owned_loci_refusal(
        ResourceDimension::RetainedBytes,
        project_compact_operand_markers_with_policy,
    );
}
#[test]
fn planar_compact_operand_markers_refuse_work_limit() {
    assert_owned_loci_refusal(
        ResourceDimension::WorkUnits,
        project_compact_operand_markers_with_policy,
    );
}
