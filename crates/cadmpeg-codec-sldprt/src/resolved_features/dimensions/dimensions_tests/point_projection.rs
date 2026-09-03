use super::super::project_relation_point_dimensioned_circles;
use crate::records::{
    FeatureInputClass, FeatureInputClassRole, FeatureInputLane, FeatureInputOperand,
    FeatureInputOperandKind, FeatureInputReference, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind,
};
use cadmpeg_ir::features::{
    DesignParameter, DimensionDisplay, Feature, FeatureDefinition, FeatureId, Length, ParameterId,
    ParameterValue,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchId};
use std::collections::BTreeMap;

#[test]
fn explicit_point_circle_dimension_projects_with_declared_nonempty_lane() {
    let feature_id = FeatureId("feature".into());
    let sketch_id = SketchId("sketch".into());
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 42,
        family: FeatureInputRelationFamily::CircleDiameter,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalar_refs: vec!["scalar".into()],
        parameter_scalar_ref: Some("scalar".into()),
        display_scalar_ref: None,
        operands: vec![FeatureInputOperand {
            offset: 0,
            reference_ref: "reference".into(),
            kind: FeatureInputOperandKind::Native(0x829a),
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
            role: FeatureInputClassRole::SketchEntity,
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
            kind: FeatureInputOperandKind::Native(0x829a),
            class_ref: Some("class".into()),
            object_index: 0,
        }],
        sketch_entities: vec![SketchInputEntity {
            id: "center".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: 10,
            object_index: Some(0),
            local_id: Some(0),
            kind: SketchInputKind::Point,
            state_value: Some(1.0),
            coordinates_m: Some([0.001, 0.002]),
            links: Vec::new(),
            link_selector: None,
        }],
    };
    let feature = Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some("feature".into()),
    };
    let parameter = DesignParameter {
        id: ParameterId("parameter".into()),
        owner: Some(feature_id),
        ordinal: 0,
        name: "D1".into(),
        expression: "<MOD-DIAM>4".into(),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(Length(4.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("scalar".into()),
    };
    let mut entities = vec![SketchEntity {
        id: SketchEntityId("center".into()),
        sketch: sketch_id,
        construction: true,
        native_ref: Some("center".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(1.0, 2.0),
        },
    }];

    project_relation_point_dimensioned_circles(
        &mut entities,
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&lane),
    );

    assert!(matches!(
        entities.get(1).map(|entity| &entity.geometry),
        Some(SketchGeometry::Circle {
            center,
            radius: Length(2.0)
        }) if *center == Point2::new(1.0, 2.0)
    ));

    let mut classless_lane = lane.clone();
    classless_lane.references[0].class_ref = None;
    let mut classless_entities = vec![entities[0].clone()];
    project_relation_point_dimensioned_circles(
        &mut classless_entities,
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&classless_lane),
    );
    assert!(matches!(
        classless_entities.get(1).map(|entity| &entity.geometry),
        Some(SketchGeometry::Circle {
            center,
            radius: Length(2.0)
        }) if *center == Point2::new(1.0, 2.0)
    ));

    let mut object_index_lane = lane.clone();
    object_index_lane.references[0].object_index = 1;
    object_index_lane.sketch_entities[0].object_index = Some(1);
    object_index_lane.sketch_entities[0].local_id = None;
    object_index_lane.relation_instances[0].operands[0].kind =
        FeatureInputOperandKind::Native(0x814c);
    object_index_lane.relation_instances[0].operands[0].entity_index = 1;
    let mut object_index_entities = vec![entities[0].clone()];
    project_relation_point_dimensioned_circles(
        &mut object_index_entities,
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&object_index_lane),
    );
    assert!(matches!(
        object_index_entities.get(1).map(|entity| &entity.geometry),
        Some(SketchGeometry::Circle {
            center,
            radius: Length(2.0)
        }) if *center == Point2::new(1.0, 2.0)
    ));
}
