use super::{append_relation_point_notes, project_relation_point_geometry};
use crate::history::configuration::project_configuration_sketch_states;
use crate::loss::SldprtLossCode;
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance,
};
use crate::resolved_features::transforms::tests::marker;
use cadmpeg_ir::features::parameters::{
    ConfigurationEvaluation, ConfigurationFeatureState, DesignConfiguration,
};
use cadmpeg_ir::features::{
    ConfigurationId, Feature, FeatureDefinition, FeatureId, FeatureOperation, SketchFeatureBinding,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};
use std::collections::BTreeMap;

/// A sketch feature that binds its sketch, the sketch's entities and the lane
/// of one point marker stored at `coordinates_m`, whose three anchor markers
/// fix the marker transform `(u, v) -> (1000 v, 1000 u)` and whose marker
/// carries a dimension circle, and the marker's id.
fn fixture(coordinates_m: [f64; 2]) -> (Feature, Vec<SketchEntity>, FeatureInputLane, String) {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let feature = Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#feature").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch.clone())),
            }),
        ),
        native_ref: Some("feature-native".into()),
    };
    let mut entities = [(0.0, 0.0), (1.0, 2.0), (4.0, 7.0)]
        .into_iter()
        .enumerate()
        .map(|(index, (u, v))| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:id#anchor-{index}")).unwrap(),
                sketch.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(u, v),
                })
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let mut markers = [[0.0, 0.0], [0.002, 0.001], [0.007, 0.004]]
        .into_iter()
        .enumerate()
        .map(|(index, coordinates)| {
            let value = marker(&format!("anchor-{index}"), Some(coordinates));
            value.with_test_position(
                value.ordinal(),
                cadmpeg_core::decode::u64_from_index(index * 27),
            )
        })
        .collect::<Vec<_>>();
    let point_marker = marker("relation-point", Some(coordinates_m));
    let point_marker = point_marker.with_test_position(point_marker.ordinal(), 81);
    let point_id = point_marker.id().to_string();
    markers.push(point_marker);
    entities.push(
        SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#dimension-carrier").unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(5.0, 6.0),
                radius: Length::new(10.0).unwrap(),
            })
            .unwrap(),
        )
        .with_construction(true)
        .with_native_ref(Some(point_id.clone())),
    );
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new().into(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![FeatureInputRelationInstance {
            id: "relation".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 90,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class".into(),
            feature_ref: "feature-native".into(),
            scalars: crate::records::relation_scalars::RelationScalars::from_refs(
                vec!["sldprt:test:scalar#unselected-7".into()],
                None,
                None,
            )
            .unwrap(),
            operands: vec![FeatureInputOperand {
                offset: 91,
                reference_ref: "reference".into(),
                kind: FeatureInputOperandKind::D6,
                entity_index: 0,
                entity_ref: Some(point_id.clone()),
            }],
        }],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: markers,
    };
    (feature, entities, lane, point_id)
}

/// Projects the relation point of the marker of [`fixture`].
fn project_point(coordinates_m: [f64; 2]) -> (Vec<SketchEntity>, Vec<LossNote>, String) {
    let (feature, mut entities, lane, point_id) = fixture(coordinates_m);
    let losses = project_relation_point_geometry(
        &mut entities,
        &[],
        std::slice::from_ref(&feature),
        std::slice::from_ref(&lane),
    );
    (entities, losses, point_id)
}

fn relation_points<'a>(entities: &'a [SketchEntity], marker: &str) -> Vec<&'a SketchEntity> {
    entities
        .iter()
        .filter(|entity| {
            entity.native_ref.as_deref() == Some(marker)
                && matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                )
        })
        .collect()
}

#[test]
fn a_relation_point_marker_with_normal_coordinates_is_drawn() {
    let (entities, losses, marker) = project_point([0.005, 0.006]);
    let points = relation_points(&entities, &marker);
    let [point] = points.as_slice() else {
        panic!("one relation point: {points:?}");
    };
    assert!(
        matches!(
            *point.geometry.definition(),
            SketchGeometryDefinition::Point { position } if position == Point2::new(6.0, 5.0)
        ),
        "{:?}",
        point.geometry.definition()
    );
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn a_relation_point_marker_with_subnormal_coordinates_is_charged_and_not_drawn() {
    let subnormal = f64::MIN_POSITIVE / 4.0;
    assert!(subnormal != 0.0 && !subnormal.is_normal());
    for coordinates in [[subnormal, subnormal], [0.005, subnormal]] {
        let (entities, losses, marker) = project_point(coordinates);
        assert!(relation_points(&entities, &marker).is_empty());
        let [loss] = losses.as_slice() else {
            panic!("one loss for {coordinates:?}: {losses:?}");
        };
        assert_eq!(
            loss.code,
            SldprtLossCode::SketchPointCoordinatesSubnormal.kind()
        );
        assert!(loss.message.contains(&marker), "{}", loss.message);
    }
}

#[test]
fn a_configuration_pass_charges_a_subnormal_marker_the_model_pass_does_not_reach() {
    let subnormal = f64::MIN_POSITIVE / 4.0;
    let (bound, entities, mut lane, marker) = fixture([subnormal, subnormal]);
    lane.configuration = Some("0".into());
    // The model feature leaves the sketch unbound; only the configuration
    // state binds it, so only the configuration pass reaches the marker.
    let mut unbound = bound.clone();
    unbound.evaluation = cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(None),
        }),
    );
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(unbound);
    ir.model.sketch_entities = entities;
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#configuration").unwrap(),
        ordinal: 0,
        active: false,
        source_index: Some(0),
        name: Some("configuration".into()),
        material: None,
        properties: BTreeMap::new(),
        bodies: None,
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::from([(
            bound.id.clone(),
            ConfigurationFeatureState {
                definition: bound.evaluation.definition().clone(),
                dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
                evaluation: ConfigurationEvaluation::Active {
                    outputs: cadmpeg_ir::features::selections::DistinctMembers::default(),
                },
            },
        )]),
        native_ref: None,
    });
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let losses = project_configuration_sketch_states(
        &ctx,
        &mut ir,
        &[],
        std::slice::from_ref(&lane),
        &crate::resolved_features::profiles::ExactProfiles::default(),
        &mut cadmpeg_ir::Annotations::default(),
    )
    .unwrap();
    assert!(relation_points(&ir.model.sketch_entities, &marker).is_empty());
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert_eq!(
        loss.code,
        SldprtLossCode::SketchPointCoordinatesSubnormal.kind()
    );
    assert!(loss.message.contains(&marker), "{}", loss.message);

    // Where the model pass has charged the marker, the configuration pass's
    // note joins it as one note.
    let mut merged = losses.clone();
    append_relation_point_notes(&mut merged, losses.clone());
    assert_eq!(merged, losses);
}
