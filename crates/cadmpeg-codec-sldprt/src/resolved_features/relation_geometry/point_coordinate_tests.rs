use super::{project_relation_point_geometry, RelationPointNotes};
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
    )
    .into_notes()
    .collect();
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
    let (losses, decisions) = project_configuration_sketch_states(
        &ctx,
        &mut ir,
        &[],
        std::slice::from_ref(&lane),
        &crate::resolved_features::profiles::ExactProfiles::default(),
        &mut cadmpeg_ir::Annotations::default(),
        &mut crate::resolved_features::compact_reference_planes::PlaneBindings::default(),
    )
    .unwrap();
    assert!(losses.is_empty(), "{losses:?}");
    assert!(relation_points(&ir.model.sketch_entities, &marker).is_empty());
    let losses = decisions.into_notes().collect::<Vec<_>>();
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert_eq!(
        loss.code,
        SldprtLossCode::SketchPointCoordinatesSubnormal.kind()
    );
    assert!(loss.message.contains(&marker), "{}", loss.message);
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
        None,
    )
    .unwrap();
    assert_eq!(unprojected, 0);
    settle_projected_configurations(&mut decoded);
    align_configuration_parameter_kinds(&mut decoded);
    let (_, decode_points) = project_configuration_sketch_states(
        &ctx,
        &mut decoded,
        &native.feature_histories,
        &native.feature_input_lanes,
        &crate::resolved_features::profiles::ExactProfiles::default(),
        &mut cadmpeg_ir::Annotations::default(),
        &mut crate::resolved_features::compact_reference_planes::PlaneBindings::default(),
    )
    .unwrap();
    let decode_notes = decode_points.into_notes().collect::<Vec<_>>();
    assert!(
        decode_notes.iter().any(|loss| loss.code
            == SldprtLossCode::SketchPointCoordinatesSubnormal.kind()
            && loss.message.contains(&marker)),
        "the configuration pass charges the marker: {decode_notes:?}"
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

/// What reaches [`fixture`]'s point marker in [`Readings`].
#[derive(Clone, Copy, Debug)]
enum Reach {
    /// The relation operand, as on main.
    Relation,
    /// Only the links of a list-read record whose stored code is a relation.
    ListedRecord,
}

/// What marks one of the two places in [`Readings`].
#[derive(Clone, Copy, Debug)]
enum Mark {
    /// The centre of a circle of the sketch.
    Curve(Point2),
    /// A vertex of the solid, at the place on the sketch plane z = 0.
    Vertex(Point2),
}

/// The class of a relation point drawn before the pass in [`Readings`].
#[derive(Clone, Copy, Debug)]
enum Drawn {
    /// Drawn for a marker that a relation reaches as on main.
    Main,
    /// Drawn for a marker that only a list-read record reaches.
    Listed,
}

/// The anchors that fit the marker transform `(u, v) -> (v + 100, u + 200)`.
const SWAPPED_ANCHORS: [Point2; 3] = [
    Point2 { u: 100.0, v: 200.0 },
    Point2 { u: 101.0, v: 202.0 },
    Point2 { u: 104.0, v: 207.0 },
];

/// [`fixture`]'s marker stored at (3, 9) mm in a sketch whose three anchor
/// entities stand at `anchors`, and whose frame (origin 0, normal +z, u along
/// +x) reads the pair as it stands, at (3, 9). With [`SWAPPED_ANCHORS`] the
/// fit places the point at (109, 203).
struct Readings {
    feature: Feature,
    frame: Sketch,
    lane: FeatureInputLane,
    entities: Vec<SketchEntity>,
    solid: SolidEdges,
    marker: String,
}

impl Readings {
    /// A relation point drawn before the pass, of class `drawn.1`, stands at
    /// each `drawn.0`; its marker belongs to a lane the pass is not given.
    fn new(reach: Reach, anchors: [Point2; 3], marks: &[Mark], drawn: &[(Point2, Drawn)]) -> Self {
        let (feature, entities, mut lane, marker) = fixture([0.003, 0.009]);
        if let Reach::ListedRecord = reach {
            lane.relation_instances.clear();
            lane.sketch_entities
                .push(listed_curve("listed-curve", &marker));
        }
        let sketch = entities[0].sketch.clone();
        let mut entities = anchors
            .into_iter()
            .enumerate()
            .map(|(index, position)| {
                SketchEntity::new(
                    SketchEntityId::mint(format!("synthetic:test:id#placed-anchor-{index}"))
                        .unwrap(),
                    sketch.clone(),
                    SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        for (index, (position, class)) in drawn.iter().enumerate() {
            let class = match class {
                Drawn::Main => "",
                Drawn::Listed => "listed:",
            };
            entities.push(
                SketchEntity::new(
                    SketchEntityId::mint(format!(
                        "sldprt:model:sketch-entity#relation-point:{class}other-lane:{}",
                        900 + index
                    ))
                    .unwrap(),
                    sketch.clone(),
                    SketchGeometry::try_from(SketchGeometryDefinition::Point {
                        position: *position,
                    })
                    .unwrap(),
                )
                .with_native_ref(Some(format!("other-lane-marker-{index}"))),
            );
        }
        let mut points = Vec::new();
        let mut vertices = Vec::new();
        for (index, mark) in marks.iter().enumerate() {
            match *mark {
                Mark::Curve(center) => entities.push(circle(&sketch, index, center)),
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
        Self {
            feature,
            frame: framed_sketch(sketch, None),
            lane,
            entities,
            solid: SolidEdges::new(&[], &vertices, &points, &[]),
            marker,
        }
    }

    fn project(&mut self) -> RelationPointNotes {
        project_relation_point_geometry(
            &mut self.entities,
            std::slice::from_ref(&self.frame),
            std::slice::from_ref(&self.feature),
            std::slice::from_ref(&self.lane),
            &self.solid,
        )
    }
}

/// A list-read curve record whose stored code is the horizontal relation,
/// linked to `point`.
fn listed_curve(id: &str, point: &str) -> crate::records::SketchInputEntity {
    let mut curve = marker(id, None);
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
            entity_ref: point.to_owned(),
        }],
    );
    curve
}

/// A sketch whose frame has origin 0, normal +z and u along +x.
fn framed_sketch(id: SketchId, lane: Option<&str>) -> Sketch {
    Sketch {
        id,
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
        native_ref: lane.map(str::to_owned),
    }
}

/// A circle of radius 1 mm about `center`.
fn circle(sketch: &SketchId, index: usize, center: Point2) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#witness-{index}")).unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center,
            radius: Length::new(1.0).unwrap(),
        })
        .unwrap(),
    )
}

/// Projects the relation point of [`Readings`] with [`SWAPPED_ANCHORS`].
fn project_between_readings(
    reach: Reach,
    marks: &[Mark],
    drawn: &[(Point2, Drawn)],
) -> (Vec<SketchEntity>, Vec<LossNote>, String) {
    let mut readings = Readings::new(reach, SWAPPED_ANCHORS, marks, drawn);
    let losses = readings.project().into_notes().collect();
    (readings.entities, losses, readings.marker)
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

/// The frame's readings of the three anchor markers: relation points drawn
/// there alone fit the unswapped identity, which agrees with the frame.
const IDENTITY_READINGS: [Point2; 3] = [
    Point2 { u: 0.0, v: 0.0 },
    Point2 { u: 2.0, v: 1.0 },
    Point2 { u: 7.0, v: 4.0 },
];

fn drawn(class: Drawn) -> Vec<(Point2, Drawn)> {
    IDENTITY_READINGS
        .into_iter()
        .map(|position| (position, class))
        .collect()
}

#[test]
fn relation_points_drawn_before_do_not_vote_in_the_fit_of_an_added_point() {
    // Main's relation points, which vote in the fit of main's geometry.
    let (entities, losses, marker) =
        project_between_readings(Reach::ListedRecord, &[], &drawn(Drawn::Main));
    assert!(relation_points(&entities, &marker).is_empty());
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert_eq!(
        loss.code,
        SldprtLossCode::SketchRelationPointPlaceUnsettled.kind()
    );
    assert!(loss.message.contains("(109, 203)"), "{}", loss.message);
}

#[test]
fn added_relation_points_do_not_move_a_point_main_places() {
    let (entities, losses, marker) =
        project_between_readings(Reach::Relation, &[], &drawn(Drawn::Listed));
    assert_near(drawn_at(&entities, &marker), FITTED);
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn a_point_main_draws_for_a_marker_of_a_lane_the_pass_is_not_given_votes_as_on_main() {
    // The drawn points' markers belong to a lane the pass is not given. As on
    // main, they vote with the anchors: two transforms, so the frame places
    // the point.
    let (entities, losses, marker) =
        project_between_readings(Reach::Relation, &[], &drawn(Drawn::Main));
    assert_near(drawn_at(&entities, &marker), FRAMED);
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn readings_within_the_linear_resolution_place_the_point_together() {
    let anchors = IDENTITY_READINGS.map(|anchor| Point2::new(anchor.u + 1.0e-8, anchor.v));
    let mut readings = Readings::new(Reach::ListedRecord, anchors, &[], &[]);
    let losses = readings.project().into_notes().collect::<Vec<_>>();
    let position = drawn_at(&readings.entities, &readings.marker);
    assert!(
        (position.u - FRAMED.u).hypot(position.v - FRAMED.v) < 1.0e-6,
        "{position:?}"
    );
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn a_later_pass_that_draws_a_named_marker_withdraws_the_note() {
    let mut readings = Readings::new(Reach::ListedRecord, SWAPPED_ANCHORS, &[], &[]);
    let mut decisions = readings.project();
    let sketch = readings.entities[0].sketch.clone();
    // A curve that a later pass draws ends on the fitted place.
    readings.entities.push(circle(&sketch, 0, FITTED));
    decisions.extend(readings.project());
    assert_near(drawn_at(&readings.entities, &readings.marker), FITTED);
    let losses = decisions.into_notes().collect::<Vec<_>>();
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn a_later_pass_that_names_a_named_marker_again_keeps_one_note() {
    let mut readings = Readings::new(Reach::ListedRecord, SWAPPED_ANCHORS, &[], &[]);
    let mut decisions = readings.project();
    let sketch = readings.entities[0].sketch.clone();
    readings.entities.push(circle(&sketch, 0, FITTED));
    readings.entities.push(circle(&sketch, 1, FRAMED));
    decisions.extend(readings.project());
    assert!(relation_points(&readings.entities, &readings.marker).is_empty());
    let losses = decisions.into_notes().collect::<Vec<_>>();
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert!(
        loss.message.contains("lies on each of them"),
        "{}",
        loss.message
    );
}

/// Two lanes name one feature. The document feature binds the sketch read
/// from the first lane; the second lane's configuration binds the sketch read
/// from the second. Each lane holds a point marker at (3, 9) mm that only a
/// list-read record reaches. Runs the model pass over both lanes, checks it,
/// and then the second lane's configuration pass, whose sketch holds a point
/// bound to the second lane's marker where `held`. Returns the entities, the
/// decisions of both passes, the second lane's marker and the two sketches.
fn second_lane_passes(held: bool) -> (Vec<SketchEntity>, RelationPointNotes, String, [Sketch; 2]) {
    let (feature, _, template, _) = fixture([0.003, 0.009]);
    let lane = |lane_id: &str, prefix: &str| {
        let point = marker(&format!("{prefix}relation-point"), Some([0.003, 0.009]));
        let point = point.with_test_position(point.ordinal(), 81);
        let point_id = point.id().to_owned();
        let mut lane = template.clone();
        lane.id = lane_id.into();
        lane.relation_instances.clear();
        lane.sketch_entities = vec![
            point,
            listed_curve(&format!("{prefix}listed-curve"), &point_id),
        ];
        (lane, point_id)
    };
    let (first_lane, first_marker) = lane("first-lane", "first-");
    let (second_lane, second_marker) = lane("second-lane", "second-");
    let sketches = [
        framed_sketch(
            SketchId::mint("synthetic:test:id#first-sketch").unwrap(),
            Some("first-lane"),
        ),
        framed_sketch(
            SketchId::mint("synthetic:test:id#second-sketch").unwrap(),
            Some("second-lane"),
        ),
    ];
    let binding = |sketch: &Sketch| {
        let mut bound = feature.clone();
        bound.evaluation = cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch.id.clone())),
            }),
        );
        bound
    };
    let mut entities = Vec::new();
    let mut decisions = project_relation_point_geometry(
        &mut entities,
        &sketches,
        &[binding(&sketches[0])],
        &[first_lane, second_lane.clone()],
        &SolidEdges::default(),
    );
    assert_near(drawn_at(&entities, &first_marker), FRAMED);
    assert!(relation_points(&entities, &second_marker).is_empty());
    if held {
        entities.push(
            SketchEntity::new(
                SketchEntityId::mint("synthetic:test:id#second-lane-point").unwrap(),
                sketches[1].id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point { position: FRAMED })
                    .unwrap(),
            )
            .with_native_ref(Some(second_marker.clone())),
        );
    }
    decisions.extend(project_relation_point_geometry(
        &mut entities,
        &sketches,
        &[binding(&sketches[1])],
        std::slice::from_ref(&second_lane),
        &SolidEdges::default(),
    ));
    (entities, decisions, second_marker, sketches)
}

#[test]
fn an_added_point_of_a_second_lane_is_drawn_only_in_the_sketch_of_that_lane() {
    let (entities, decisions, marker, sketches) = second_lane_passes(false);
    // A relation row of the second lane reads the first entity bound to each
    // of its operand markers.
    let operand = entities
        .iter()
        .find(|entity| entity.native_ref.as_deref() == Some(marker.as_str()))
        .expect("the second lane's point is drawn");
    assert_eq!(operand.sketch, sketches[1].id);
    assert_eq!(relation_points(&entities, &marker).len(), 1);
    let losses = decisions.into_notes().collect::<Vec<_>>();
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn a_point_the_sketch_of_its_own_lane_already_holds_withdraws_the_model_pass_note() {
    let (entities, decisions, marker, sketches) = second_lane_passes(true);
    let points = relation_points(&entities, &marker);
    let [point] = points.as_slice() else {
        panic!("one point: {points:?}");
    };
    assert_eq!(point.sketch, sketches[1].id);
    let losses = decisions.into_notes().collect::<Vec<_>>();
    assert!(losses.is_empty(), "{losses:?}");
}
