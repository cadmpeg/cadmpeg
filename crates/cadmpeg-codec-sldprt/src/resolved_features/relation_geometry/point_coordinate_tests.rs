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
use crate::records::{SketchInputKind, SketchInputLink, SketchRelationKind};
use crate::resolved_features::stream_readings::SolidEdges;
use crate::resolved_features::transforms::tests::marker;
use cadmpeg_ir::features::parameters::{
    ConfigurationEvaluation, ConfigurationFeatureState, DesignConfiguration,
};
use cadmpeg_ir::features::{
    ConfigurationId, Feature, FeatureDefinition, FeatureId, FeatureOperation, SketchFeatureBinding,
};
use cadmpeg_ir::ids::{PointId, VertexId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    SketchPlacement, SketchProfiles,
};
use cadmpeg_ir::topology::{Point as BrepPoint, Vertex};
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
        &crate::resolved_features::stream_readings::SolidEdges::default(),
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

/// What reaches [`fixture`]'s point marker in [`project_between_readings`].
#[derive(Clone, Copy, Debug)]
enum Reach {
    /// The relation operand, as on main.
    Relation,
    /// Only the links of a list-read record whose stored code is a relation.
    ListedRecord,
}

/// What marks one of the two places in [`project_between_readings`].
#[derive(Clone, Copy, Debug)]
enum Mark {
    /// The centre of a circle of the sketch.
    Curve(Point2),
    /// A vertex of the solid, at the place on the sketch plane z = 0.
    Vertex(Point2),
}

/// Projects the relation point of [`fixture`]'s marker stored at (3, 9) mm in
/// a sketch whose three anchors fit the marker transform
/// `(u, v) -> (v + 100, u + 200)`, which places the point at (109, 203), and
/// whose frame (origin 0, normal +z, u along +x) reads the pair as it
/// stands, at (3, 9). A relation point drawn before the pass stands at each
/// of `drawn`.
fn project_between_readings(
    reach: Reach,
    marks: &[Mark],
    drawn: &[Point2],
) -> (Vec<SketchEntity>, Vec<LossNote>, String) {
    let (feature, entities, mut lane, marker_id) = fixture([0.003, 0.009]);
    if let Reach::ListedRecord = reach {
        lane.relation_instances.clear();
        let mut curve = marker("listed-curve", None);
        curve = curve.with_test_position(curve.ordinal(), 99);
        curve.reclassify(SketchInputKind::LineOrCircle);
        curve.listed_curve = Some(crate::records::ListedCurveOverride {
            form: "line",
            code: 4,
            stored: SketchInputKind::Relation(SketchRelationKind::Horizontal),
        });
        curve.links = crate::records::SketchInputLinks::new(
            0,
            vec![SketchInputLink {
                local_id: 1,
                entity_ref: marker_id.clone(),
            }],
        );
        lane.sketch_entities.push(curve);
    }
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
    for (index, position) in drawn.iter().enumerate() {
        entities.push(SketchEntity::new(
            SketchEntityId::mint(format!(
                "sldprt:model:sketch-entity#relation-point:lane:{}",
                900 + index
            ))
            .unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: *position,
            })
            .unwrap(),
        ));
    }
    let mut points = Vec::new();
    let mut vertices = Vec::new();
    for (index, mark) in marks.iter().enumerate() {
        match *mark {
            Mark::Curve(center) => entities.push(SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:id#witness-{index}")).unwrap(),
                sketch.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                    center,
                    radius: Length::new(1.0).unwrap(),
                })
                .unwrap(),
            )),
            Mark::Vertex(place) => {
                let point = PointId::mint(format!("synthetic:test:id#point-{index}")).unwrap();
                points.push(BrepPoint::new(
                    point.clone(),
                    cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(
                        place.u, place.v, 0.0,
                    ))
                    .unwrap(),
                    None,
                ));
                vertices.push(Vertex {
                    id: VertexId::mint(format!("synthetic:test:id#vertex-{index}")).unwrap(),
                    point,
                    tolerance: None,
                    native_ref: None,
                });
            }
        }
    }
    let frame = Sketch {
        id: sketch,
        name: None,
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
    };
    let losses = project_relation_point_geometry(
        &mut entities,
        std::slice::from_ref(&frame),
        std::slice::from_ref(&feature),
        std::slice::from_ref(&lane),
        &SolidEdges::new(&[], &vertices, &points, &[]),
    );
    (entities, losses, marker_id)
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
fn a_relation_reached_marker_with_two_readings_is_drawn_at_the_fitted_place() {
    let (entities, losses, marker) = project_between_readings(Reach::Relation, &[], &[]);
    assert_near(drawn_at(&entities, &marker), FITTED);
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn a_listed_record_marker_is_drawn_at_the_one_place_a_mark_witnesses() {
    for (mark, expected) in [
        (Mark::Vertex(FRAMED), FRAMED),
        (Mark::Vertex(FITTED), FITTED),
        (Mark::Curve(FITTED), FITTED),
        (Mark::Curve(FRAMED), FRAMED),
    ] {
        let (entities, losses, marker) =
            project_between_readings(Reach::ListedRecord, &[mark], &[]);
        assert_near(drawn_at(&entities, &marker), expected);
        assert!(losses.is_empty(), "{mark:?}: {losses:?}");
    }
}

#[test]
fn a_listed_record_marker_that_no_mark_or_both_marks_witness_is_named_and_not_drawn() {
    for (marks, reason) in [
        (&[][..], "no curve end or centre"),
        (
            &[Mark::Curve(FITTED), Mark::Vertex(FRAMED)][..],
            "lies on each of them",
        ),
    ] {
        let (entities, losses, marker) = project_between_readings(Reach::ListedRecord, marks, &[]);
        assert!(relation_points(&entities, &marker).is_empty());
        let [loss] = losses.as_slice() else {
            panic!("one loss for {marks:?}: {losses:?}");
        };
        assert_eq!(
            loss.code,
            SldprtLossCode::SketchRelationPointPlaceUnsettled.kind()
        );
        assert!(loss.message.contains(&marker), "{}", loss.message);
        assert!(loss.message.contains(reason), "{}", loss.message);
    }
}

#[test]
fn relation_points_drawn_before_do_not_vote_in_the_fit_of_an_added_point() {
    // Drawn at the frame's reading of the three anchor markers, these points
    // alone would fit the unswapped identity, which agrees with the frame.
    let drawn = [
        Point2::new(0.0, 0.0),
        Point2::new(2.0, 1.0),
        Point2::new(7.0, 4.0),
    ];
    let (entities, losses, marker) = project_between_readings(Reach::ListedRecord, &[], &drawn);
    assert!(relation_points(&entities, &marker).is_empty());
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert_eq!(
        loss.code,
        SldprtLossCode::SketchRelationPointPlaceUnsettled.kind()
    );
}
