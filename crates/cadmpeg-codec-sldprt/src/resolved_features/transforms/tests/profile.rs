//! Marker-backed profile sketch and owned-edge tests.

use super::super::*;
use super::marker;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputEdgeSelection, FeatureInputLane,
    FeatureInputName, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputKind, SketchInputLink, SketchRelationKind,
};
use cadmpeg_ir::features::{
    Angle, BooleanOp, DesignParameter, DimensionDisplay, EdgeSelection, ExtrudeExtent, ExtrudeSide,
    Feature, FeatureDefinition, FeatureId, Length, LinearTermination, ParameterId, ParameterValue,
    ProfileRef, RadiusSpec,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus,
    SketchPlacement,
};
use std::collections::{BTreeMap, HashMap};

#[test]
fn doubled_point_distance_constrains_the_owned_profile_line() {
    let mut corner = marker("corner", Some([0.005, 0.005]));
    corner.object_index = Some(4);
    let mut center = marker("center", Some([0.0025, 0.0025]));
    center.object_index = Some(1);
    let mut distance_handle = marker("distance-handle", None);
    distance_handle.kind = SketchInputKind::Relation(SketchRelationKind::Distance);
    distance_handle.links = vec![SketchInputLink {
        local_id: 2,
        entity_ref: center.id.clone(),
    }];
    let markers = [&corner, &center, &distance_handle]
        .into_iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let relation = FeatureInputRelationInstance {
        id: "dimension".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class".into(),
        feature_ref: "feature-native".into(),
        scalar_refs: Vec::new(),
        parameter_scalar_ref: None,
        display_scalar_ref: None,
        operands: ["corner", "center"]
            .into_iter()
            .enumerate()
            .map(|(index, marker)| FeatureInputOperand {
                offset: index as u64,
                reference_ref: format!("reference-{index}"),
                kind: FeatureInputOperandKind::Native(0xbc7c),
                entity_index: index as u16,
                entity_ref: Some(marker.into()),
            })
            .collect(),
    };
    let parameter = DesignParameter {
        id: ParameterId("width".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "width".into(),
        expression: "5".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let sketch = SketchId("sketch".into());
    let line_id = SketchEntityId("line".into());
    let entities = vec![SketchEntity::new(
        line_id.clone(),
        sketch.clone(),
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(5.0, 0.0),
        },
    )
    .with_native_ref(Some(corner.id.clone()))];

    assert_eq!(
        doubled_profile_distance_loci(&relation, 0, 1, &sketch, &parameter, &entities, &markers,),
        Some((
            SketchLocus::Start(line_id.clone()),
            SketchLocus::End(line_id.clone()),
        ))
    );

    let markers_without_handle = [&corner, &center]
        .into_iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        doubled_profile_distance_loci(
            &relation,
            0,
            1,
            &sketch,
            &parameter,
            &entities,
            &markers_without_handle,
        ),
        None
    );
}

#[test]
fn repeated_native_edge_vectors_project_one_neutral_edge_each() {
    let feature = |id: &str, native_ref: &str, definition| Feature {
        id: FeatureId(id.into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: Some(native_ref.into()),
    };
    let producer = feature(
        "producer",
        "producer-native",
        FeatureDefinition::Sketch { sketch: None },
    );
    let target = feature(
        "target",
        "target-native",
        FeatureDefinition::Fillet {
            groups: vec![cadmpeg_ir::features::FilletGroup {
                edges: EdgeSelection::Unresolved,
                radius: RadiusSpec::Constant {
                    radius: Length(1.0),
                },
                tangency_weight: None,
            }],
        },
    );
    let selection = |ordinal, offset, local_edge_ids| FeatureInputEdgeSelection {
        id: format!("selection-{ordinal}"),
        parent: "lane".into(),
        ordinal,
        offset,
        object_name_ref: "name".into(),
        feature_ref: "target-native".into(),
        local_edge_ids,
        components: Vec::new(),
        references: Vec::new(),
        producer_feature_refs: vec!["producer-native".into()],
        terminal_feature_ref: Some("producer-native".into()),
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: vec![
            selection(0, 10, vec![1, 2]),
            selection(1, 20, vec![3, 4]),
            selection(2, 30, vec![1, 2]),
        ],
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let mut features = vec![producer, target];

    project_compact_edge_selections(&mut features, &[], &[lane]);

    let FeatureDefinition::Fillet { groups } = &features[1].definition else {
        panic!("generated edge selection");
    };
    let [cadmpeg_ir::features::FilletGroup {
        edges: EdgeSelection::Generated { edges, native },
        ..
    }] = groups.as_slice()
    else {
        panic!("generated edge selection group");
    };
    assert_eq!(
        edges
            .iter()
            .map(|edge| edge.local_id.as_str())
            .collect::<Vec<_>>(),
        vec!["1,2", "3,4"]
    );
    assert_eq!(
        native,
        "sldprt:feature-input:edge-selection-vectors:1,2;3,4;1,2"
    );
    assert_eq!(features[1].dependencies, vec![FeatureId("producer".into())]);
}

#[test]
fn input_owned_edge_vectors_exclude_future_owned_cache_records() {
    let selection = |ordinal, producer: Option<&str>| FeatureInputEdgeSelection {
        id: format!("selection-{ordinal}"),
        parent: "lane".into(),
        ordinal,
        offset: u64::from(ordinal),
        object_name_ref: "name".into(),
        feature_ref: "consumer".into(),
        local_edge_ids: vec![ordinal],
        components: Vec::new(),
        references: Vec::new(),
        producer_feature_refs: producer.into_iter().map(str::to_string).collect(),
        terminal_feature_ref: producer.map(str::to_string),
    };
    let retained = input_owned_edge_selections(vec![
        selection(0, Some("input")),
        selection(1, None),
        selection(2, Some("input")),
    ]);
    assert_eq!(
        retained
            .iter()
            .map(|selection| selection.ordinal)
            .collect::<Vec<_>>(),
        vec![0, 2]
    );

    let retained = input_owned_edge_selections(vec![selection(3, None), selection(4, None)]);
    assert_eq!(retained.len(), 2);
}

#[test]
fn compact_d6_operand_indexes_point_handles_in_byte_order() {
    let mut first = marker("arc", Some([0.0, 0.0]));
    first.offset = 10;
    first.kind = SketchInputKind::Arc;
    let mut second = marker("point", Some([1.0, 0.0]));
    second.offset = 20;
    let mut third = marker("line", Some([2.0, 0.0]));
    third.offset = 30;
    third.kind = SketchInputKind::LineOrCircle;
    let mut fourth = marker("constrained-point", Some([3.0, 0.0]));
    fourth.offset = 40;
    fourth.kind = SketchInputKind::ConstrainedPoint;
    let markers = HashMap::from([
        (first.id.as_str(), &first),
        (second.id.as_str(), &second),
        (third.id.as_str(), &third),
        (fourth.id.as_str(), &fourth),
    ]);
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class".into(),
        feature_ref: "feature-native".into(),
        scalar_refs: Vec::new(),
        parameter_scalar_ref: None,
        display_scalar_ref: None,
        operands: vec![FeatureInputOperand {
            offset: 0,
            reference_ref: "reference".into(),
            kind: FeatureInputOperandKind::D6,
            entity_index: 0,
            entity_ref: Some("stored-marker".into()),
        }],
    };

    assert_eq!(
        relation_operand_marker(
            &relation,
            0,
            &SketchId("sldprt:model:sketch#compact:lane:1".into()),
            &markers,
        ),
        Some("point")
    );
    let mut constrained_operand = relation.operands[0].clone();
    constrained_operand.entity_index = 1;
    let constrained_relation = FeatureInputRelationInstance {
        operands: vec![constrained_operand],
        ..relation.clone()
    };
    assert_eq!(
        relation_operand_marker(
            &constrained_relation,
            0,
            &SketchId("sldprt:model:sketch#compact:lane:1".into()),
            &markers,
        ),
        Some("constrained-point")
    );
    assert_eq!(
        relation_operand_marker(&relation, 0, &SketchId("sketch".into()), &markers),
        Some("stored-marker")
    );
}

#[test]
fn marker_backed_sketch_projects_endpoint_backed_lines_and_minor_arcs() {
    let native_feature = |id: &str, source_id: &str, name: &str| NativeFeature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source_id.into()),
        parent_source_id: None,
        ordinal: source_id.parse().expect("required invariant"),
        name: name.into(),
        kind: String::new(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native_feature("plane-native", "2", "Front Plane"),
            native_feature("sketch-native", "7", "Sketch1"),
        ],
    };
    let feature = |id: &str, native_ref: &str, ordinal, definition| Feature {
        id: FeatureId(id.into()),
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: Some(native_ref.into()),
    };
    let mut features = vec![
        feature(
            "plane",
            "plane-native",
            0,
            FeatureDefinition::DatumPrincipalPlane {
                plane: cadmpeg_ir::features::PrincipalPlane::Front,
            },
        ),
        feature(
            "sketch",
            "sketch-native",
            1,
            FeatureDefinition::Sketch { sketch: None },
        ),
    ];
    let mut payload = vec![0; 100];
    payload.extend_from_slice(b"moCompRefPlane_c");
    payload.extend([0; 12]);
    payload.extend(2u32.to_le_bytes());
    payload.extend(0x6554_f1b8_u32.to_le_bytes());
    payload.extend([0, 0, 3, 0]);
    payload.extend([0; 27]);
    payload.extend(1.0f64.to_le_bytes());
    payload.extend([
        0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xf9, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x65,
    ]);
    payload.extend([0; 32]);
    let mut point = marker("point", Some([0.001, 0.002]));
    point.feature_ref = Some("sketch-native".into());
    point.links = vec![SketchInputLink {
        local_id: 2,
        entity_ref: "curve".into(),
    }];
    let mut curve = marker("curve", Some([0.003, 0.004]));
    curve.feature_ref = Some("sketch-native".into());
    curve.ordinal = 1;
    curve.offset = 200;
    curve.kind = SketchInputKind::LineOrCircle;
    let mut endpoint = marker("endpoint", Some([0.005, 0.006]));
    endpoint.feature_ref = Some("sketch-native".into());
    endpoint.ordinal = 2;
    endpoint.offset = 2;
    endpoint.links = point.links.clone();
    let mut arc = marker("arc", None);
    arc.feature_ref = Some("sketch-native".into());
    arc.ordinal = 3;
    arc.offset = 3;
    arc.kind = SketchInputKind::Arc;
    let mut arc_start = marker("arc-start", Some([0.001, 0.0]));
    arc_start.feature_ref = Some("sketch-native".into());
    arc_start.ordinal = 4;
    arc_start.offset = 4;
    arc_start.links = vec![SketchInputLink {
        local_id: 4,
        entity_ref: arc.id.clone(),
    }];
    let mut arc_end = marker("arc-end", Some([0.0, 0.001]));
    arc_end.feature_ref = Some("sketch-native".into());
    arc_end.ordinal = 5;
    arc_end.offset = 5;
    arc_end.links = arc_start.links.clone();
    let mut arc_center = marker("arc-center", Some([0.0, 0.0]));
    arc_center.feature_ref = Some("sketch-native".into());
    arc_center.ordinal = 6;
    arc_center.offset = 6;
    let triangle_point = |id: &str, ordinal, offset, coordinates_m| {
        let mut point = marker(id, Some(coordinates_m));
        point.feature_ref = Some("sketch-native".into());
        point.ordinal = ordinal;
        point.offset = offset;
        point
    };
    let triangle_points = [
        triangle_point("triangle-point-0", 7, 7, [0.010, 0.011]),
        triangle_point("triangle-point-1", 8, 8, [0.020, 0.010]),
        triangle_point("triangle-point-2", 9, 9, [0.015, 0.020]),
    ];
    let triangle_line = |id: &str, ordinal, offset, first: &str, second: &str| {
        let mut line = marker(id, Some([0.0, 0.0]));
        line.feature_ref = Some("sketch-native".into());
        line.ordinal = ordinal;
        line.offset = offset;
        line.kind = SketchInputKind::LineOrCircle;
        line.links = vec![
            SketchInputLink {
                local_id: 10,
                entity_ref: first.into(),
            },
            SketchInputLink {
                local_id: 11,
                entity_ref: second.into(),
            },
        ];
        line
    };
    let triangle = [
        triangle_line("triangle-0", 10, 10, "triangle-point-0", "triangle-point-1"),
        triangle_line("triangle-1", 11, 11, "triangle-point-1", "triangle-point-2"),
        triangle_line("triangle-2", 12, 12, "triangle-point-2", "triangle-point-0"),
    ];
    let mut display_handle = marker("display-handle", Some([0.030, 0.030]));
    display_handle.feature_ref = Some("sketch-native".into());
    display_handle.ordinal = 13;
    display_handle.offset = 300;
    display_handle.kind = SketchInputKind::Arc;
    payload.resize(400, 0);
    let axis = 200;
    payload[axis..axis + 5].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[axis + 5..axis + 13].fill(0xff);
    payload[axis + 13..axis + 17].copy_from_slice(&(-1.0f32).to_le_bytes());
    payload[axis + 17..axis + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[axis + 23..axis + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[axis + 27..axis + 29].copy_from_slice(&2u16.to_le_bytes());
    payload[axis + 31..axis + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[axis + 48..axis + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[axis + 56..axis + 58].copy_from_slice(&2u16.to_le_bytes());
    payload[axis + 58..axis + 60].copy_from_slice(&3u16.to_le_bytes());
    payload[axis + 64..axis + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[axis + 74..axis + 76].copy_from_slice(&2u16.to_le_bytes());
    payload[axis + 84..axis + 89].copy_from_slice(LEGACY_SKETCH_MARKER);
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "plane-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                value: "Front Plane".into(),
                object_id: Some(2),
            },
            FeatureInputName {
                id: "sketch-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 100,
                value: "Sketch1".into(),
                object_id: Some(7),
            },
        ],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![point, curve, endpoint, arc, arc_start, arc_end, arc_center]
            .into_iter()
            .chain(triangle_points)
            .chain(triangle)
            .chain([display_handle])
            .collect(),
    };
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let histories = vec![history];
    let lanes = vec![lane];
    project_marker_backed_sketches(
        &mut features,
        &mut sketches,
        &mut entities,
        &histories,
        &lanes,
    );

    assert_eq!(sketches.len(), 1);
    assert_eq!(entities.len(), 13);
    assert!(matches!(
        entities[0].geometry,
        SketchGeometry::Point { position }
            if position == Point2::new(-2.0, 1.0)
    ));
    assert!(matches!(
        entities[1].geometry,
        SketchGeometry::Line { start, end }
            if start == Point2::new(-2.0, 1.0)
                && end == Point2::new(-6.0, 5.0)
    ));
    assert!(!entities[0].construction);
    assert!(entities[1].construction);
    assert!(entities[2..].iter().all(|entity| !entity.construction));
    assert!(matches!(
        entities[3].geometry,
        SketchGeometry::Arc {
            center,
            radius: Length(radius),
            start_angle: Angle(start_angle),
            end_angle: Angle(end_angle),
        } if center == Point2::new(0.0, 0.0)
            && radius == 1.0
            && start_angle == std::f64::consts::FRAC_PI_2
            && end_angle == std::f64::consts::PI
    ));
    assert_eq!(sketches[0].profiles.len(), 1);
    assert_eq!(sketches[0].profiles[0].len(), 3);
    assert!(matches!(
        features[1].definition,
        FeatureDefinition::Sketch { sketch: Some(_) }
    ));
    let expected_sketch = sketches[0].id.clone();
    let mut configured_features = features.clone();
    configured_features[1].definition = FeatureDefinition::Sketch { sketch: None };
    project_marker_backed_sketches(
        &mut configured_features,
        &mut sketches,
        &mut entities,
        &histories,
        &lanes,
    );
    assert_eq!(sketches.len(), 1);
    assert_eq!(entities.len(), 13);
    assert!(matches!(
        &configured_features[1].definition,
        FeatureDefinition::Sketch {
            sketch: Some(sketch),
        } if sketch == &expected_sketch
    ));

    let compact_id = SketchId("sldprt:model:sketch#compact:lane:7".into());
    let mut compact_sketch = sketches[0].clone();
    compact_sketch.id = compact_id.clone();
    compact_sketch.profiles.clear();
    let source_entity = &entities[0];
    let compact_entity = SketchEntity::new(
        SketchEntityId("compact-entity".into()),
        compact_id.clone(),
        source_entity.geometry.clone(),
    )
    .with_construction(source_entity.construction)
    .with_native_ref(source_entity.native_ref.clone())
    .with_geometry_ref(source_entity.geometry_ref.clone())
    .with_endpoint_refs(source_entity.endpoint_refs.clone());
    let mut replacement_features = features.clone();
    replacement_features[1].definition = FeatureDefinition::Sketch {
        sketch: Some(compact_id),
    };
    let mut replacement_sketches = vec![compact_sketch];
    let mut replacement_entities = vec![compact_entity];
    project_marker_backed_sketches(
        &mut replacement_features,
        &mut replacement_sketches,
        &mut replacement_entities,
        &histories,
        &lanes,
    );
    assert_eq!(replacement_sketches.len(), 1);
    assert_eq!(replacement_sketches[0].id, expected_sketch);
    assert_eq!(replacement_entities.len(), 13);
    assert!(replacement_entities
        .iter()
        .all(|entity| entity.sketch == expected_sketch));
}

#[test]
fn marker_backed_sketch_preserves_geometry_when_placement_is_unresolved() {
    let native_feature = NativeFeature {
        id: "feature-native".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: Some("1".into()),
        parent_source_id: None,
        ordinal: 1,
        name: "generated-profile".into(),
        kind: String::new(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![native_feature],
    }];
    let mut features = vec![Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: Some("generated-profile".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch { sketch: None },
        native_ref: Some("feature-native".into()),
    }];
    let lanes = vec![FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0],
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![marker("point", Some([0.001, 0.002]))],
    }];
    let mut sketches = Vec::new();
    let mut entities = Vec::new();

    project_marker_backed_sketches(
        &mut features,
        &mut sketches,
        &mut entities,
        &histories,
        &lanes,
    );

    assert_eq!(sketches.len(), 1);
    assert_eq!(sketches[0].placement, SketchPlacement::Unresolved);
    assert!(matches!(
        entities.as_slice(),
        [SketchEntity {
            geometry: SketchGeometry::Point { position },
            ..
        }] if *position == Point2::new(1.0, 2.0)
    ));
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Sketch { sketch: Some(sketch), .. }
            if sketch == &sketches[0].id
    ));
}

#[test]
fn marker_circle_fit_requires_one_circle_through_every_endpoint() {
    let points = [
        Point2::new(-2.0, 0.0),
        Point2::new(0.0, 2.0),
        Point2::new(2.0, 0.0),
        Point2::new(0.0, -2.0),
    ];
    assert_eq!(
        fitted_marker_circle(&points, 1.0e-8),
        Some((Point2::new(0.0, 0.0), 2.0))
    );
    let mut inconsistent = points;
    inconsistent[3] = Point2::new(0.0, -3.0);
    assert_eq!(fitted_marker_circle(&inconsistent, 1.0e-8), None);
    assert_eq!(fitted_marker_circle(&points[..2], 1.0e-8), None);
}

#[test]
fn connected_marker_arcs_use_their_shared_endpoint_circle() {
    let sketch = SketchId("sketch".into());
    let point = |id: &str, position| {
        SketchEntity::new(
            SketchEntityId(format!("entity-{id}")),
            sketch.clone(),
            SketchGeometry::Point { position },
        )
        .with_native_ref(Some(id.into()))
    };
    let arc = |id: &str, start: &str, end: &str| {
        SketchEntity::new(
            SketchEntityId(format!("entity-{id}")),
            sketch.clone(),
            SketchGeometry::Native {
                native_kind: "sldprt:marker-geometry:2".into(),
            },
        )
        .with_native_ref(Some(id.into()))
        .with_endpoint_refs(vec![start.into(), end.into()])
    };
    let mut entities = vec![
        point("p0", Point2::new(0.0, -2.0)),
        point("p1", Point2::new(2.0, 0.0)),
        point("p2", Point2::new(0.0, 2.0)),
        arc("a0", "p0", "p1"),
        arc("a1", "p1", "p2"),
    ];

    resolve_connected_marker_arcs(&mut entities, 1.0e-8);

    for entity in &entities[3..] {
        assert!(matches!(
            entity.geometry,
            SketchGeometry::Arc {
                center,
                radius: Length(2.0),
                ..
            } if center == Point2::new(0.0, 0.0)
        ));
    }
    for entity in &mut entities[3..] {
        entity.endpoint_refs.reverse();
        entity.geometry = SketchGeometry::Native {
            native_kind: "sldprt:marker-geometry:2".into(),
        };
    }
    resolve_connected_marker_arcs(&mut entities, 1.0e-8);
    assert!(entities[3..]
        .iter()
        .all(|entity| matches!(entity.geometry, SketchGeometry::Arc { .. })));
    assert_eq!(entities[3].endpoint_refs, ["p0", "p1"]);
    assert_eq!(entities[4].endpoint_refs, ["p1", "p2"]);
    entities.push(
        SketchEntity::new(
            SketchEntityId("entity-line".into()),
            sketch,
            SketchGeometry::Line {
                start: Point2::new(0.0, 2.0),
                end: Point2::new(0.0, -2.0),
            },
        )
        .with_native_ref(Some("line".into()))
        .with_endpoint_refs(vec!["p2".into(), "p0".into()]),
    );
    assert_eq!(closed_marker_profiles(&entities)[0].len(), 3);
    entities
        .last_mut()
        .expect("required invariant")
        .construction = true;
    assert!(closed_marker_profiles(&entities).is_empty());
    entities.push(
        SketchEntity::new(
            SketchEntityId("entity-circle".into()),
            entities[0].sketch.clone(),
            SketchGeometry::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length(2.0),
            },
        )
        .with_native_ref(Some("circle".into())),
    );
    assert_eq!(
        closed_marker_profiles(&entities),
        vec![vec![SketchEntityUse {
            entity: SketchEntityId("entity-circle".into()),
            reversed: false,
        }]]
    );
}

#[test]
fn unowned_radial_records_do_not_override_complete_diameter_circles() {
    let sketch_id = SketchId("sketch".into());
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let mut sketches = vec![Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: Some("lane".into()),
    }];
    let parameter = |ordinal: u32, diameter: f64| DesignParameter {
        id: ParameterId(format!("parameter-{ordinal}")),
        owner: Some(feature.id.clone()),
        name: format!("D{}", ordinal + 1),
        ordinal,
        expression: diameter.to_string(),
        value: Some(ParameterValue::Length(Length(diameter))),
        display: Some(DimensionDisplay::Diameter),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some(format!("scalar-{ordinal}")),
        dependencies: Vec::new(),
    };
    let center = marker("center", Some([0.0, 0.0]));
    let mut first = marker("first", Some([0.005, 0.0]));
    first.offset = 100;
    let mut second = marker("second", Some([0.0, 0.008]));
    second.offset = 200;
    let mut native_payload = vec![0; 102];
    native_payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    native_payload[5..13].fill(0xff);
    native_payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    native_payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    native_payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    native_payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    native_payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    native_payload[64..66].copy_from_slice(&1u16.to_le_bytes());
    native_payload[66..68].copy_from_slice(&1u16.to_le_bytes());
    native_payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    native_payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    native_payload[80..84].copy_from_slice(&1u32.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload,
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![center, first, second],
    };
    let carrier = SketchEntity::new(
        SketchEntityId("carrier".into()),
        sketch_id.clone(),
        SketchGeometry::Native {
            native_kind: "sldprt:marker-geometry:0".into(),
        },
    )
    .with_native_ref(Some("center".into()));
    let mut entities = vec![
        carrier,
        SketchEntity::new(
            SketchEntityId("first-entity".into()),
            sketch_id.clone(),
            SketchGeometry::Point {
                position: Point2::new(5.0, 0.0),
            },
        )
        .with_construction(true)
        .with_native_ref(Some("first".into())),
        SketchEntity::new(
            SketchEntityId("second-entity".into()),
            sketch_id.clone(),
            SketchGeometry::Point {
                position: Point2::new(0.0, 8.0),
            },
        )
        .with_construction(true)
        .with_native_ref(Some("second".into())),
    ];
    assert_eq!(
        crate::resolved_features::dimensions::extended_radial_circle_index(&lane.native_payload, 0,),
        Some(1)
    );

    let mut invalid_lane = lane.clone();
    let mut extra = marker("unowned-radial", Some([0.011, 0.0]));
    extra.offset = 300;
    invalid_lane.sketch_entities.push(extra);
    let mut invalid_entities = entities.clone();
    let mut invalid_sketches = sketches.clone();
    project_marker_dimensioned_circles(
        &mut invalid_entities,
        &mut invalid_sketches,
        std::slice::from_ref(&feature),
        &[parameter(0, 10.0), parameter(1, 16.0)],
        std::slice::from_ref(&invalid_lane),
    );
    assert_eq!(invalid_entities.len(), 3);
    assert!(invalid_sketches[0].profiles.is_empty());

    project_marker_dimensioned_circles(
        &mut entities,
        &mut sketches,
        std::slice::from_ref(&feature),
        &[parameter(0, 10.0), parameter(1, 16.0)],
        std::slice::from_ref(&lane),
    );

    assert_eq!(entities.len(), 4);
    assert_eq!(sketches[0].profiles.len(), 2);
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SketchGeometry::Circle {
            center,
            radius: Length(5.0)
        } if center == Point2::new(0.0, 0.0)
    )));
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SketchGeometry::Circle {
            center,
            radius: Length(8.0)
        } if center == Point2::new(0.0, 0.0)
    )));
}

#[test]
fn dissected_child_classification_does_not_imply_profile_alias() {
    let native_feature = |id: &str, name: &str, description: Option<&str>| NativeFeature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 0,
        name: name.into(),
        kind: String::new(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: description
            .map(|description| BTreeMap::from([("Description".into(), description.into())]))
            .unwrap_or_default(),
        text: None,
        content: Vec::new(),
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native_feature("owner-native", "Sketch", None),
            native_feature("child-native", "Sketch<3>", Some("Sketch<3>")),
            native_feature("multi-child-native", "Sketch<5>", Some("Sketch<5>")),
        ],
    };
    let feature = |id: &str, native_ref: &str, dependencies, sketch| Feature {
        id: FeatureId(id.into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies,
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch { sketch },
        native_ref: Some(native_ref.into()),
    };
    let single = SketchId("single".into());
    let multiple = SketchId("multiple".into());
    let mut features = vec![
        feature("owner", "owner-native", Vec::new(), Some(single.clone())),
        feature(
            "child",
            "child-native",
            vec![FeatureId("owner".into())],
            None,
        ),
        feature(
            "multi-owner",
            "owner-native",
            Vec::new(),
            Some(multiple.clone()),
        ),
        feature(
            "multi-child",
            "multi-child-native",
            vec![FeatureId("multi-owner".into())],
            None,
        ),
        Feature {
            id: FeatureId("consumer".into()),
            ordinal: 3,
            name: None,
            suppressed: Some(false),
            dependencies: vec![FeatureId("child".into())],
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Extrude {
                profile: ProfileRef::Feature(FeatureId("child".into())),
                direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
                start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: Length(1.0),
                        },
                        draft: None,
                    },
                },
                op: BooleanOp::Join,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
            native_ref: Some("consumer-native".into()),
        },
    ];
    let mut multi_consumer = features[4].clone();
    multi_consumer.id = FeatureId("multi-consumer".into());
    multi_consumer.ordinal = 4;
    multi_consumer.dependencies = vec![FeatureId("multi-child".into())];
    let FeatureDefinition::Extrude { profile, .. } = &mut multi_consumer.definition else {
        unreachable!();
    };
    *profile = ProfileRef::Feature(FeatureId("multi-child".into()));
    features.push(multi_consumer);
    let sketch = |id: SketchId, profile_count: usize| Sketch {
        id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: (0..profile_count)
            .map(|index| {
                vec![SketchEntityUse {
                    entity: SketchEntityId(format!("entity-{index}")),
                    reversed: false,
                }]
            })
            .collect(),
        native_ref: None,
    };
    let sketches = vec![sketch(single.clone(), 1), sketch(multiple, 2)];

    project_dissected_sketches(&mut features, &sketches, std::slice::from_ref(&history));

    assert!(matches!(
        &features[1].definition,
        FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::DissectedProfile,
            ..
        }
    ));
    assert!(matches!(
        features[3].definition,
        FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::DissectedProfile,
            ..
        }
    ));
    assert!(matches!(
        &features[4].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(sketch),
            ..
        } if sketch == &single
    ));
    assert_eq!(features[4].dependencies, [FeatureId("owner".into())]);
    assert!(matches!(
        &features[5].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(feature),
            ..
        } if feature == &FeatureId("multi-child".into())
    ));
    assert_eq!(features[5].dependencies, [FeatureId("multi-child".into())]);
}
