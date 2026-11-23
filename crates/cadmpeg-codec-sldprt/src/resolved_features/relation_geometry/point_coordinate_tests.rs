use super::{append_relation_point_notes, project_relation_point_geometry};
use crate::history::configuration::{
    align_configuration_parameter_kinds, project_configuration_design_states,
    project_configuration_sketch_states,
};
use crate::history::extrude_direction::settle_projected_configurations;
use crate::history::project::project_features;
use crate::history::write::configurations::prepare_configurations_for_write;
use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;
use crate::records::{
    Configuration, Feature as NativeFeature, FeatureHistory, FeatureInputLane, FeatureInputOperand,
    FeatureInputOperandKind, FeatureInputRelationFamily, FeatureInputRelationInstance,
    FeatureSource,
};
use crate::resolved_features::transforms::tests::marker;
use cadmpeg_ir::features::parameters::{
    ConfigurationEvaluation, ConfigurationFeatureState, DesignConfiguration,
};
use cadmpeg_ir::features::{
    ConfigurationId, Feature, FeatureDefinition, FeatureId, FeatureOperation, SketchFeatureBinding,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    SketchPlacement, SketchProfiles,
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
        &mut crate::resolved_features::compact_reference_planes::PlaneBindings::default(),
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

/// The writer projects the configuration states again when they differ from
/// the document's baseline. A relation point charge of that projection falls
/// on the decode's projection as well, so the write goes on and its report
/// names the marker whose point is not drawn.
#[test]
fn a_configuration_write_with_a_subnormal_marker_goes_on_and_reports_the_point() {
    let subnormal = f64::MIN_POSITIVE / 4.0;
    let (bound, entities, mut lane, marker) = fixture([subnormal, subnormal]);
    lane.configuration = Some("0".into());
    let FeatureDefinition::Operation(FeatureOperation::Sketch {
        sketch: SketchFeatureBinding::Planar(Some(sketch)),
    }) = bound.evaluation.definition().clone()
    else {
        panic!("the fixture binds its sketch");
    };
    let native = SldprtNative {
        feature_histories: vec![FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: vec![Configuration {
                id: "configuration-native".into(),
                parent: "history".into(),
                ordinal: 0,
                source_index: Some(0),
                name: "configuration".into(),
                material: None,
                properties: BTreeMap::new(),
            }],
            features: vec![NativeFeature {
                id: "feature-native".into(),
                parent: "history".into(),
                xml_tag: "Sketch".into(),
                tree_parent: None,
                source_id: FeatureSource::from_value(1),
                ordinal: 1,
                name: "Sketch1".into(),
                kind: "Sketch".into(),
                input_class: None,
                suppressed: false,
                parameters: BTreeMap::new(),
                dimension_properties: BTreeMap::new(),
                properties: BTreeMap::new(),
                text: None,
                content: Vec::new(),
            }],
        }],
        feature_input_lanes: vec![lane],
        pmi_dimensions: Vec::new(),
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features = project_features(&native.feature_histories).unwrap();
    ir.model.sketches.push(Sketch {
        id: sketch,
        name: Some("Sketch1".into()),
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::default(),
        native_ref: None,
    });
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
        feature_states: BTreeMap::new(),
        native_ref: Some("configuration-native".into()),
    });

    // The configuration states as the decode projects them from these
    // records; the document holds no baseline, so the write projects them
    // again.
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut decoded = ir.clone();
    let unprojected = project_configuration_design_states(
        &ctx,
        &mut decoded,
        &native.feature_histories,
        &native.feature_input_lanes,
        &[],
        None,
    )
    .unwrap();
    assert_eq!(unprojected, 0);
    settle_projected_configurations(&mut decoded);
    align_configuration_parameter_kinds(&mut decoded);
    let decode_losses = project_configuration_sketch_states(
        &ctx,
        &mut decoded,
        &native.feature_histories,
        &native.feature_input_lanes,
        &crate::resolved_features::profiles::ExactProfiles::default(),
        &mut cadmpeg_ir::Annotations::default(),
        &mut crate::resolved_features::compact_reference_planes::PlaneBindings::default(),
    )
    .unwrap();
    assert!(
        decode_losses.iter().any(|loss| loss.code
            == SldprtLossCode::SketchPointCoordinatesSubnormal.kind()
            && loss.message.contains(&marker)),
        "the configuration pass charges the marker: {decode_losses:?}"
    );
    ir.model.configurations = decoded.model.configurations;

    let losses = prepare_configurations_for_write(
        &ir,
        &mut Some(native),
        &cadmpeg_ir::Annotations::default(),
        |_| Ok(None),
    )
    .expect("a relation point charge does not refuse the write");
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert_eq!(
        loss.code,
        SldprtLossCode::SketchPointCoordinatesSubnormal.kind()
    );
    assert!(loss.message.contains(&marker), "{}", loss.message);
}

/// Projects the relation point of [`fixture`]'s marker stored at (3, 9) mm in
/// a sketch whose three anchors fit the marker transform
/// `(u, v) -> (v + 100, u + 200)`, which places the point at (109, 203), and
/// whose unresolved frame reads the pair as it stands, at (3, 9). A circle is
/// centred on each of `witnesses`.
fn project_between_readings(witnesses: &[Point2]) -> (Vec<SketchEntity>, Vec<LossNote>, String) {
    let (feature, entities, lane, marker) = fixture([0.003, 0.009]);
    let sketch = entities[0].sketch.clone();
    let mut entities = [(100.0, 200.0), (101.0, 202.0), (104.0, 207.0)]
        .into_iter()
        .enumerate()
        .map(|(index, (u, v))| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:id#shifted-anchor-{index}")).unwrap(),
                sketch.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(u, v),
                })
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    for (index, center) in witnesses.iter().enumerate() {
        entities.push(SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#witness-{index}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: *center,
                radius: Length::new(1.0).unwrap(),
            })
            .unwrap(),
        ));
    }
    let frame = Sketch {
        id: sketch,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: SketchProfiles::default(),
        native_ref: None,
    };
    let losses = project_relation_point_geometry(
        &mut entities,
        std::slice::from_ref(&frame),
        std::slice::from_ref(&feature),
        std::slice::from_ref(&lane),
    );
    (entities, losses, marker)
}

const FITTED: Point2 = Point2 { u: 109.0, v: 203.0 };
const FRAMED: Point2 = Point2 { u: 3.0, v: 9.0 };

fn drawn_at(entities: &[SketchEntity], marker: &str) -> Point2 {
    let points = relation_points(entities, marker);
    let [point] = points.as_slice() else {
        panic!("one relation point: {points:?}");
    };
    let SketchGeometryDefinition::Point { position } = *point.geometry.definition() else {
        panic!("a point");
    };
    position.get()
}

fn assert_near(actual: Point2, expected: Point2) {
    assert!(
        (actual.u - expected.u).abs() < 1.0e-9 && (actual.v - expected.v).abs() < 1.0e-9,
        "{actual:?} against {expected:?}"
    );
}

#[test]
fn a_curve_centre_on_the_fitted_place_draws_the_relation_point_there() {
    let (entities, losses, marker) = project_between_readings(&[FITTED]);
    assert_near(drawn_at(&entities, &marker), FITTED);
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn a_curve_centre_on_the_frame_place_draws_the_relation_point_there() {
    let (entities, losses, marker) = project_between_readings(&[FRAMED]);
    assert_near(drawn_at(&entities, &marker), FRAMED);
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn two_readings_that_no_curve_or_both_curves_mark_draw_no_relation_point() {
    for (witnesses, reason) in [
        (&[][..], "no curve end or centre"),
        (&[FITTED, FRAMED][..], "lies on each of them"),
    ] {
        let (entities, losses, marker) = project_between_readings(witnesses);
        assert!(relation_points(&entities, &marker).is_empty());
        let [loss] = losses.as_slice() else {
            panic!("one loss for {witnesses:?}: {losses:?}");
        };
        assert_eq!(
            loss.code,
            SldprtLossCode::SketchRelationPointPlaceUnsettled.kind()
        );
        assert!(loss.message.contains(&marker), "{}", loss.message);
        assert!(loss.message.contains(reason), "{}", loss.message);
    }
}
