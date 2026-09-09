use super::super::project_relation_point_dimensioned_circles;
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputClass, FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind,
    FeatureInputReference, FeatureInputRelationFamily, FeatureInputRelationInstance,
    SketchInputEntity, SketchInputKind,
};
use cadmpeg_ir::features::{
    DesignParameter, DimensionDisplay, Feature, FeatureDefinition, FeatureId, Length, ParameterId,
    ParameterValue,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometryDefinition, SketchId};
use std::collections::BTreeMap;

#[test]
fn explicit_point_circle_dimension_projects_with_declared_nonempty_lane() {
    let feature_id = FeatureId::mint("synthetic:test:id#feature").expect("identity grammar");
    let sketch_id = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 42,
        family: FeatureInputRelationFamily::CircleDiameter,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalars: crate::records::relation_scalars::RelationScalars::from_refs(
            vec!["scalar".into()],
            Some("scalar".into()),
            None,
        )
        .unwrap(),
        operands: vec![FeatureInputOperand {
            offset: 0,
            reference_ref: "reference".into(),
            kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_829A),
            entity_index: 0,
            entity_ref: Some("center".into()),
        }],
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0],
        classes: vec![FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 10,
            name: "sgEntHandle".into(),
        }],
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![relation],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: vec![FeatureInputReference {
            id: "reference".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: 20,
            kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_829A),
            class_ref: Some("class".into()),
            object_index: 0,
        }],
        sketch_entities: vec![{
            let marker_id: String = "center".into();
            let marker_parent: String = "lane".into();
            let mut constructed_marker =
                SketchInputEntity::new(marker_id, marker_parent, 0, 10, SketchInputKind::Point);
            constructed_marker.feature_ref = Some("feature".into());
            constructed_marker.object_index = Some(0);
            constructed_marker.local_id = Some(0);
            constructed_marker.state_value = Some(1.0);
            constructed_marker.coordinates_m = Some([0.001, 0.002]);
            constructed_marker.links = None;
            constructed_marker
        }],
    };
    let feature = Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id.clone())),
            },
        ),
        native_ref: Some("feature".into()),
    };
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
        owner: Some(feature_id),
        ordinal: 0,
        name: "D1".into(),
        expression: "<MOD-DIAM>4".into(),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(Length::new(4.0).unwrap())),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("scalar".into()),
    };
    let mut entities = vec![SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#center").unwrap(),
        sketch_id,
        cadmpeg_ir::sketches::SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(1.0, 2.0),
        })
        .unwrap(),
    )
    .with_construction(true)
    .with_native_ref(Some("center".into()))];

    project_relation_point_dimensioned_circles(
        &mut entities,
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&lane),
    )
    .unwrap();

    assert!(matches!(
        entities.get(1).map(|entity| entity.geometry.definition()),
        Some(SketchGeometryDefinition::Circle {
            center,
            radius: actual_radius
        }) if (*center == Point2::new(1.0, 2.0)) && actual_radius.get() == 2.0
    ));

    let mut classless_lane = lane.clone();
    classless_lane.references[0].class_ref = None;
    let mut classless_entities = vec![entities[0].clone()];
    project_relation_point_dimensioned_circles(
        &mut classless_entities,
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&classless_lane),
    )
    .unwrap();
    assert!(matches!(
        classless_entities.get(1).map(|entity| entity.geometry.definition()),
        Some(SketchGeometryDefinition::Circle {
            center,
            radius: actual_radius
        }) if (*center == Point2::new(1.0, 2.0)) && actual_radius.get() == 2.0
    ));

    let mut object_index_lane = lane.clone();
    object_index_lane.references[0].object_index = 1;
    object_index_lane.sketch_entities[0].object_index = Some(1);
    object_index_lane.sketch_entities[0].local_id = None;
    object_index_lane.relation_instances[0].operands[0].kind =
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_814C);
    object_index_lane.relation_instances[0].operands[0].entity_index = 1;
    let mut object_index_entities = vec![entities[0].clone()];
    project_relation_point_dimensioned_circles(
        &mut object_index_entities,
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&object_index_lane),
    )
    .unwrap();
    assert!(matches!(
        object_index_entities.get(1).map(|entity| entity.geometry.definition()),
        Some(SketchGeometryDefinition::Circle {
            center,
            radius: actual_radius
        }) if (*center == Point2::new(1.0, 2.0)) && actual_radius.get() == 2.0
    ));
}
