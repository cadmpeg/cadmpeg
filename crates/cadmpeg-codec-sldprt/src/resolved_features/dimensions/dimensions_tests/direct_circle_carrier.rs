use super::super::reconcile_direct_circle_dimension_carriers;
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind,
};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, Length};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
    SketchPlacement,
};

fn lane(feature: &str, marker: &str, relation: &str) -> FeatureInputLane {
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![FeatureInputRelationInstance {
            id: relation.into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            family: FeatureInputRelationFamily::CircleDiameter,
            class_ref: "circle-class".into(),
            feature_ref: feature.into(),
            scalar_refs: Vec::new(),
            parameter_scalar_ref: None,
            display_scalar_ref: None,
            operands: vec![FeatureInputOperand {
                offset: 0,
                reference_ref: "reference".into(),
                kind: FeatureInputOperandKind::Native(0x829a),
                entity_index: 0,
                entity_ref: Some(marker.into()),
            }],
        }],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![SketchInputEntity {
            id: marker.into(),
            parent: "lane".into(),
            feature_ref: Some(feature.into()),
            ordinal: 0,
            offset: 0,
            object_index: None,
            local_id: None,
            kind: SketchInputKind::LineOrCircle,
            state_value: Some(1.0),
            coordinates_m: Some([0.001, 0.002]),
            links: Vec::new(),
            link_selector: None,
        }],
    }
}

fn feature(feature_ref: &str, sketch: &SketchId) -> Feature {
    let mut feature = Feature::new(
        FeatureId("neutral-feature".into()),
        0,
        FeatureDefinition::Sketch {
            sketch: Some(sketch.clone()),
        },
    );
    feature.native_ref = Some(feature_ref.into());
    feature
}

fn sketch(sketch: &SketchId, profiles: Vec<Vec<SketchEntityUse>>) -> Sketch {
    Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved,
        profiles,
        native_ref: Some("lane".into()),
    }
}

fn native_entity(sketch: &SketchId, id: &str, native_ref: &str) -> SketchEntity {
    SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(native_ref.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Native {
            native_kind: "native-circle".into(),
        },
    }
}

#[test]
fn exact_direct_circle_dimension_replaces_only_its_native_carrier() {
    let feature_ref = "feature";
    let marker_ref = "circle-marker";
    let relation_ref = "circle-dimension";
    let sketch_id = SketchId("sketch".into());
    let typed_id = SketchEntityId("typed-circle".into());
    let native_id = SketchEntityId("native-circle".into());
    let other_id = SketchEntityId("other-native".into());
    let lane = lane(feature_ref, marker_ref, relation_ref);
    let feature = feature(feature_ref, &sketch_id);
    let typed = SketchEntity {
        id: typed_id.clone(),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: Some(marker_ref.into()),
        geometry_ref: Some(relation_ref.into()),
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(1.0, 2.0),
            radius: Length(2.0),
        },
    };
    let native = native_entity(&sketch_id, native_id.0.as_str(), marker_ref);
    let other = native_entity(&sketch_id, other_id.0.as_str(), "other-marker");
    let mut entities = vec![typed, native, other];
    let mut sketches = vec![sketch(
        &sketch_id,
        vec![
            vec![
                SketchEntityUse {
                    entity: native_id,
                    reversed: false,
                },
                SketchEntityUse {
                    entity: typed_id.clone(),
                    reversed: false,
                },
            ],
            vec![SketchEntityUse {
                entity: other_id,
                reversed: false,
            }],
        ],
    )];

    reconcile_direct_circle_dimension_carriers(
        &mut entities,
        &mut sketches,
        &sketch_id,
        feature.native_ref.as_deref().expect("feature reference"),
        std::slice::from_ref(&lane),
    );

    assert!(!entities
        .iter()
        .any(|entity| entity.id == SketchEntityId("native-circle".into())));
    assert!(entities.iter().any(|entity| entity.id == typed_id));
    assert!(entities
        .iter()
        .any(|entity| entity.id == SketchEntityId("other-native".into())));
    assert_eq!(sketches[0].profiles.len(), 2);
    assert_eq!(sketches[0].profiles[0].len(), 1);
    assert_eq!(
        sketches[0].profiles[0][0].entity,
        SketchEntityId("typed-circle".into())
    );
}

#[test]
fn direct_circle_dimension_without_typed_replacement_keeps_native_carrier() {
    let feature_ref = "feature";
    let marker_ref = "circle-marker";
    let sketch_id = SketchId("sketch".into());
    let lane = lane(feature_ref, marker_ref, "circle-dimension");
    let feature = feature(feature_ref, &sketch_id);
    let mut entities = vec![native_entity(&sketch_id, "native-circle", marker_ref)];
    let mut sketches = vec![sketch(&sketch_id, Vec::new())];

    reconcile_direct_circle_dimension_carriers(
        &mut entities,
        &mut sketches,
        &sketch_id,
        feature.native_ref.as_deref().expect("feature reference"),
        std::slice::from_ref(&lane),
    );

    assert_eq!(entities.len(), 1);
    assert!(matches!(
        entities[0].geometry,
        SketchGeometry::Native { .. }
    ));
}
