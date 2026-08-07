//! Tests for the `transforms` module.

use super::{
    binary_relation_matches_evaluated_geometry, bind_circle_dimension_centers,
    bind_circular_profile_by_dimension, bind_detached_relation_drivers, bind_pattern_inputs,
    bind_sweep_adjacent_profiles, closed_marker_profiles, compact_line_reference_direction,
    declared_line_reference_directions, dimensioned_circle_surface_transforms,
    dimensioned_circle_transform, doubled_profile_distance_loci, fitted_marker_circle,
    implicit_circle_marker, inferred_point_coordinates_by_index, input_owned_edge_selections,
    legacy_terminal_profile_indexed_endpoints, line_endpoint_markers, line_reference_direction,
    linear_pattern_display_directions, marker_entities, marker_owns_constraint, marker_point_locus,
    marker_relation_is_inactive, owned_relation_parameters, profile_loci_by_marker,
    project_compact_edge_selections, project_dimensioned_sketch_geometry,
    project_dissected_sketches, project_marker_backed_sketches, project_marker_dimensioned_circles,
    project_relation_bindings, project_relation_point_geometry,
    project_relation_solved_line_geometry, project_relation_solved_point_geometry,
    relation_constraint_is_inactive, relation_operand_loci, relation_operand_marker,
    relation_owner_markers, relation_parameter_by_display_name, resolve_connected_marker_arcs,
    resolved_marker_locus, select_marker_transforms_by_frame, single_marker_curve_entity,
    single_marker_line_entity, sketch_frame_marker_transform, type_display_relation_parameters,
    typed_marker_relation_definition, typed_marker_relation_definition_in_sketch,
    typed_relation_definition, unique_axis_aligned_linked_loci, unique_compatible_marker_transform,
    unique_linked_endpoint_locus, unique_marker_transform, unique_profile_axis_distance_locus,
    unique_profile_axis_distance_pair, unique_profile_distance_loci_pair,
    unique_profile_distance_locus, unique_profile_line_angle_entity,
    unique_profile_line_angle_pair, unique_profile_line_distance_entity,
    unique_profile_line_distance_pair, unique_profile_line_point_locus,
    unique_profile_point_line_entity, unique_profile_point_line_pair,
    unique_repaired_profile_line_angle_pair, unique_repaired_profile_line_distance_pair,
    unique_repaired_profile_point_line_pair, MarkerTransform, COMPACT_EDGE_VECTOR_MARKER,
    LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
};
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
    FeatureInputEdgeSelection, FeatureInputLane, FeatureInputName, FeatureInputOperand,
    FeatureInputOperandKind, FeatureInputRelationFamily, FeatureInputRelationInstance,
    FeatureInputScalar, FeatureInputScalarRole, SketchInputEntity, SketchInputKind,
    SketchInputLink, SketchRelationKind,
};
use cadmpeg_ir::features::{
    Angle, BooleanOp, DesignParameter, DimensionDisplay, EdgeSelection, ExtrudeExtent, ExtrudeSide,
    Feature, FeatureDefinition, FeatureId, Length, ParameterId, ParameterValue, PathRef,
    PatternKind, PatternSeed, ProfileRef, RadiusSpec, SweepMode, Termination,
};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchEntityUse,
    SketchGeometry, SketchId, SketchLocus, SketchNativeOperand,
};
use std::collections::{BTreeMap, HashMap, HashSet};

fn marker(id: &str, coordinates_m: Option<[f64; 2]>) -> SketchInputEntity {
    SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature-native".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    }
}

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
    let entities = vec![SketchEntity {
        id: line_id.clone(),
        sketch: sketch.clone(),
        construction: false,
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(5.0, 0.0),
        },
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        native_ref: Some(corner.id.clone()),
    }];

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
        parent: None,
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
        FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: None,
        },
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

    project_compact_edge_selections(&mut features, &[lane]);

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
fn compact_d6_operand_indexes_coordinate_handles_in_byte_order() {
    let mut first = marker("first", Some([0.0, 0.0]));
    first.offset = 10;
    first.kind = SketchInputKind::Arc;
    let mut second = marker("second", Some([1.0, 0.0]));
    second.offset = 20;
    second.kind = SketchInputKind::LineOrCircle;
    let markers = HashMap::from([(first.id.as_str(), &first), (second.id.as_str(), &second)]);
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
            entity_index: 1,
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
        Some("second")
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
        parent: None,
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
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
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
    let mut arc = marker("arc", Some([0.0, 0.0]));
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
    let triangle_point = |id: &str, ordinal, offset, coordinates_m| {
        let mut point = marker(id, Some(coordinates_m));
        point.feature_ref = Some("sketch-native".into());
        point.ordinal = ordinal;
        point.offset = offset;
        point
    };
    let triangle_points = [
        triangle_point("triangle-point-0", 6, 6, [0.010, 0.010]),
        triangle_point("triangle-point-1", 7, 7, [0.020, 0.010]),
        triangle_point("triangle-point-2", 8, 8, [0.015, 0.020]),
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
        triangle_line("triangle-0", 9, 9, "triangle-point-0", "triangle-point-1"),
        triangle_line("triangle-1", 10, 10, "triangle-point-1", "triangle-point-2"),
        triangle_line("triangle-2", 11, 11, "triangle-point-2", "triangle-point-0"),
    ];
    let mut display_handle = marker("display-handle", Some([0.030, 0.030]));
    display_handle.feature_ref = Some("sketch-native".into());
    display_handle.ordinal = 12;
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
        sketch_entities: vec![point, curve, endpoint, arc, arc_start, arc_end]
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
    assert_eq!(entities.len(), 12);
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
        FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(_),
            ..
        }
    ));
    let expected_sketch = sketches[0].id.clone();
    let mut configured_features = features.clone();
    configured_features[1].definition = FeatureDefinition::Sketch {
        space: cadmpeg_ir::features::SketchSpace::Planar,
        sketch: None,
    };
    project_marker_backed_sketches(
        &mut configured_features,
        &mut sketches,
        &mut entities,
        &histories,
        &lanes,
    );
    assert_eq!(sketches.len(), 1);
    assert_eq!(entities.len(), 12);
    assert!(matches!(
        &configured_features[1].definition,
        FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch),
            ..
        } if sketch == &expected_sketch
    ));

    let compact_id = SketchId("sldprt:model:sketch#compact:lane:7".into());
    let mut compact_sketch = sketches[0].clone();
    compact_sketch.id = compact_id.clone();
    compact_sketch.profiles.clear();
    let mut compact_entity = entities[0].clone();
    compact_entity.id = SketchEntityId("compact-entity".into());
    compact_entity.sketch = compact_id.clone();
    let mut replacement_features = features.clone();
    replacement_features[1].definition = FeatureDefinition::Sketch {
        space: cadmpeg_ir::features::SketchSpace::Planar,
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
    assert_eq!(replacement_entities.len(), 12);
    assert!(replacement_entities
        .iter()
        .all(|entity| entity.sketch == expected_sketch));
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
    let point = |id: &str, position| SketchEntity {
        id: SketchEntityId(format!("entity-{id}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let arc = |id: &str, start: &str, end: &str| SketchEntity {
        id: SketchEntityId(format!("entity-{id}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: vec![start.into(), end.into()],
        geometry: SketchGeometry::Native {
            native_kind: "sldprt:marker-geometry:2".into(),
        },
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
    entities.push(SketchEntity {
        id: SketchEntityId("entity-line".into()),
        sketch,
        construction: false,
        native_ref: Some("line".into()),
        geometry_ref: None,
        endpoint_refs: vec!["p2".into(), "p0".into()],
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 2.0),
            end: Point2::new(0.0, -2.0),
        },
    });
    assert_eq!(closed_marker_profiles(&entities)[0].len(), 3);
    entities
        .last_mut()
        .expect("required invariant")
        .construction = true;
    assert!(closed_marker_profiles(&entities).is_empty());
    entities.push(SketchEntity {
        id: SketchEntityId("entity-circle".into()),
        sketch: entities[0].sketch.clone(),
        construction: false,
        native_ref: Some("circle".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
        },
    });
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
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let mut sketches = vec![Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
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
    let mut center = marker("center", Some([0.0, 0.0]));
    center.kind = SketchInputKind::LineOrCircle;
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
    native_payload[56..60].copy_from_slice(&[1, 0, 1, 0]);
    native_payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    native_payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    native_payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    for at in (78..94).step_by(4) {
        native_payload[at..at + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
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
    let mut entities = vec![
        SketchEntity {
            id: SketchEntityId("first-entity".into()),
            sketch: sketch_id.clone(),
            construction: true,
            native_ref: Some("first".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(5.0, 0.0),
            },
        },
        SketchEntity {
            id: SketchEntityId("second-entity".into()),
            sketch: sketch_id.clone(),
            construction: true,
            native_ref: Some("second".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(0.0, 8.0),
            },
        },
    ];

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
        parent: None,
        dependencies,
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch,
        },
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
            parent: None,
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
                        termination: Termination::Blind {
                            length: Length(1.0),
                        },
                        draft: None,
                        offset: None,
                    },
                },
                op: BooleanOp::Join,
                direction_source: None,
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

#[test]
fn coordinate_curve_links_carry_reverse_constraint_incidence() {
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    let mut owner = marker("owner", Some([1.0, 2.0]));
    owner.kind = SketchInputKind::LineOrCircle;
    owner.object_index = Some(7);
    owner.offset = 1;
    owner.links = vec![SketchInputLink {
        local_id: 4,
        entity_ref: relation.id.clone(),
    }];
    let mut point = marker("point", Some([1.0, 2.0]));
    point.object_index = Some(8);
    point.offset = 2;
    point.links = owner.links.clone();
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (owner.id.as_str(), &owner),
        (point.id.as_str(), &point),
    ]);

    assert_eq!(
        relation_owner_markers(&relation, &markers),
        vec![&owner, &point]
    );
    let Some(SketchConstraintDefinition::Native { operands, .. }) =
        typed_marker_relation_definition(&relation, &markers, &HashMap::new())
    else {
        panic!("native relation");
    };
    assert_eq!(
        operands,
        vec![
            SketchNativeOperand {
                native_kind: "sldprt:marker-constraint-owner".into(),
                native_field: None,
                native_role: None,
                object_index: 7,
                native_ref: Some(owner.id),
            },
            SketchNativeOperand {
                native_kind: "sldprt:marker-constraint-owner".into(),
                native_field: None,
                native_role: None,
                object_index: 8,
                native_ref: Some(point.id),
            },
        ]
    );
}

#[test]
fn self_link_does_not_make_a_relation_operand_bearing() {
    let mut relation = marker("relation", Some([0.0, 0.0]));
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Perpendicular);
    relation.links = vec![SketchInputLink {
        local_id: 0,
        entity_ref: relation.id.clone(),
    }];
    let markers = HashMap::from([(relation.id.as_str(), &relation)]);

    assert!(!marker_owns_constraint(&relation, &markers));
    assert_eq!(
        typed_marker_relation_definition(&relation, &markers, &HashMap::new()),
        None
    );

    let mut collision = marker("collision", Some([1.0, 0.0]));
    collision.local_id = Some(7);
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Tangent);
    relation.local_id = Some(7);
    relation.object_index = Some(8);
    relation.links = vec![
        SketchInputLink {
            local_id: 7,
            entity_ref: collision.id.clone(),
        },
        SketchInputLink {
            local_id: 8,
            entity_ref: collision.id.clone(),
        },
    ];
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (collision.id.as_str(), &collision),
    ]);

    assert!(!marker_owns_constraint(&relation, &markers));
    assert_eq!(
        typed_marker_relation_definition(&relation, &markers, &HashMap::new()),
        None
    );
}

#[test]
fn self_identifying_forward_curve_link_is_excluded_from_arc_relation() {
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::ArcAngle90);
    relation.object_index = Some(7);
    relation.links = vec![
        SketchInputLink {
            local_id: 7,
            entity_ref: "ignored-arc".into(),
        },
        SketchInputLink {
            local_id: 9,
            entity_ref: "operand-arc".into(),
        },
    ];
    let mut ignored_arc = marker("ignored-arc", None);
    ignored_arc.kind = SketchInputKind::Arc;
    let mut operand_arc = marker("operand-arc", None);
    operand_arc.kind = SketchInputKind::Arc;
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (ignored_arc.id.as_str(), &ignored_arc),
        (operand_arc.id.as_str(), &operand_arc),
    ]);
    let loci = HashMap::from([
        (
            ignored_arc.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId("ignored-entity".into()))],
        ),
        (
            operand_arc.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId("operand-entity".into()))],
        ),
    ]);

    assert_eq!(
        typed_marker_relation_definition(&relation, &markers, &loci),
        Some(SketchConstraintDefinition::ArcAngle {
            entity: SketchEntityId("operand-entity".into()),
            angle: Angle(std::f64::consts::FRAC_PI_2),
        })
    );
}

#[test]
fn self_identifying_forward_link_is_not_a_relation_locus() {
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Vertical);
    relation.object_index = Some(1);
    relation.links = vec![SketchInputLink {
        local_id: 1,
        entity_ref: "center".into(),
    }];
    let mut center = marker("center", Some([0.0, 1.0]));
    center.kind = SketchInputKind::Arc;
    let mut first = marker("first", Some([-1.0, 0.0]));
    first.offset = 1;
    first.links = vec![SketchInputLink {
        local_id: 3,
        entity_ref: relation.id.clone(),
    }];
    let mut second = marker("second", Some([1.0, 0.0]));
    second.offset = 2;
    second.links = first.links.clone();
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (center.id.as_str(), &center),
        (first.id.as_str(), &first),
        (second.id.as_str(), &second),
    ]);
    let loci = HashMap::from([
        (
            center.id.clone(),
            vec![SketchLocus::Center(SketchEntityId("arc".into()))],
        ),
        (
            first.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId("first-point".into()))],
        ),
        (
            second.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId("second-point".into()))],
        ),
    ]);

    assert_eq!(
        relation_operand_loci(&relation, &markers, &loci),
        Some(vec![
            SketchLocus::Entity(SketchEntityId("first-point".into())),
            SketchLocus::Entity(SketchEntityId("second-point".into())),
        ])
    );
}

#[test]
fn native_fallback_entities_exclude_self_identity_collisions() {
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    relation.local_id = Some(3);
    relation.links = vec![
        SketchInputLink {
            local_id: 3,
            entity_ref: "collision".into(),
        },
        SketchInputLink {
            local_id: 4,
            entity_ref: "operand".into(),
        },
    ];
    let collision = marker("collision", Some([0.0, 0.0]));
    let operand = marker("operand", Some([1.0, 0.0]));
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (collision.id.as_str(), &collision),
        (operand.id.as_str(), &operand),
    ]);
    let loci = HashMap::from([
        (
            collision.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId("collision".into()))],
        ),
        (
            operand.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId("operand".into()))],
        ),
    ]);

    let Some(SketchConstraintDefinition::Native {
        entities, operands, ..
    }) = typed_marker_relation_definition(&relation, &markers, &loci)
    else {
        panic!("native fallback");
    };
    assert_eq!(entities, [SketchEntityId("operand".into())]);
    assert_eq!(operands.len(), 2);
}

#[test]
fn exact_curve_identity_precedes_incident_locus_expansion() {
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Vertical);
    relation.links = vec![SketchInputLink {
        local_id: 3,
        entity_ref: "curve-marker".into(),
    }];
    let mut curve = marker("curve-marker", Some([1.0, 1.0]));
    curve.kind = SketchInputKind::LineOrCircle;
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (curve.id.as_str(), &curve),
    ]);
    let exact = SketchEntityId("exact".into());
    let incident = SketchEntityId("incident".into());
    let loci = HashMap::from([(
        curve.id.clone(),
        vec![
            SketchLocus::Start(exact.clone()),
            SketchLocus::End(incident.clone()),
        ],
    )]);
    let entity = |id: SketchEntityId, native_ref: Option<&str>, start, end| SketchEntity {
        id,
        sketch: SketchId("sketch".into()),
        construction: false,
        native_ref: native_ref.map(str::to_string),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let entities = vec![
        entity(
            exact.clone(),
            Some(curve.id.as_str()),
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 2.0),
        ),
        entity(incident, None, Point2::new(0.0, 0.0), Point2::new(1.0, 2.0)),
    ];

    assert_eq!(
        typed_marker_relation_definition_in_sketch(
            &relation,
            &SketchId("sketch".into()),
            &entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Vertical { entity: exact })
    );
}

#[test]
fn fixed_relation_selects_one_geometry_operand_beside_auxiliary_relation_handles() {
    let mut relation = marker("fixed", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Fixed);
    relation.links = vec![
        SketchInputLink {
            local_id: 2,
            entity_ref: "point".into(),
        },
        SketchInputLink {
            local_id: 7,
            entity_ref: "radius".into(),
        },
    ];
    let mut point = marker("point", Some([1.0, 2.0]));
    point.kind = SketchInputKind::Point;
    let mut radius = marker("radius", None);
    radius.kind = SketchInputKind::Relation(SketchRelationKind::Radius);
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (point.id.as_str(), &point),
        (radius.id.as_str(), &radius),
    ]);
    let point_id = SketchEntityId("point-entity".into());
    let loci = HashMap::from([(
        point.id.clone(),
        vec![SketchLocus::Entity(point_id.clone())],
    )]);
    let point_entity = SketchEntity {
        id: point_id.clone(),
        sketch: SketchId("sketch".into()),
        construction: false,
        native_ref: Some(point.id.clone()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(1.0, 2.0),
        },
    };

    assert_eq!(
        typed_marker_relation_definition_in_sketch(
            &relation,
            &SketchId("sketch".into()),
            std::slice::from_ref(&point_entity),
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Fixed {
            entity: point_id.clone(),
        })
    );

    let mut second = marker("second", Some([3.0, 4.0]));
    second.kind = SketchInputKind::Point;
    relation.links.push(SketchInputLink {
        local_id: 8,
        entity_ref: second.id.clone(),
    });
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (point.id.as_str(), &point),
        (radius.id.as_str(), &radius),
        (second.id.as_str(), &second),
    ]);
    let loci = HashMap::from([
        (
            point.id.clone(),
            vec![SketchLocus::Entity(point_id.clone())],
        ),
        (
            second.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId("second-entity".into()))],
        ),
    ]);
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &relation,
            &SketchId("sketch".into()),
            &[point_entity],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Native { .. })
    ));
}

#[test]
fn resolved_wrong_family_relation_is_inactive() {
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::EllipseAngle180);
    let entity_id = SketchEntityId("line".into());
    let definition = SketchConstraintDefinition::Native {
        native_kind: "sldprt:marker-relation:34".into(),
        native_state: None,
        native_flags: None,
        native_properties: std::collections::BTreeMap::new(),
        entities: vec![entity_id.clone()],
        parameter: None,
        operands: Vec::new(),
    };
    let entities = vec![SketchEntity {
        id: entity_id,
        sketch: SketchId("sketch".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    }];

    assert!(marker_relation_is_inactive(
        &relation,
        &definition,
        &entities
    ));
}

#[test]
fn geometrically_contradicted_point_coincidence_is_inactive() {
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Coincident);
    let ids = [
        SketchEntityId("first".into()),
        SketchEntityId("second".into()),
    ];
    let definition = SketchConstraintDefinition::Native {
        native_kind: "sldprt:marker-relation:9".into(),
        native_state: None,
        native_flags: None,
        native_properties: std::collections::BTreeMap::new(),
        entities: ids.to_vec(),
        parameter: None,
        operands: Vec::new(),
    };
    let point = |id: SketchEntityId, position| SketchEntity {
        id,
        sketch: SketchId("sketch".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let first = point(ids[0].clone(), Point2::new(1.0, 2.0));
    let coincident = point(ids[1].clone(), Point2::new(1.0, 2.0));
    let distinct = point(ids[1].clone(), Point2::new(1.0, 3.0));

    assert!(!marker_relation_is_inactive(
        &relation,
        &definition,
        &[first.clone(), coincident],
    ));
    assert!(marker_relation_is_inactive(
        &relation,
        &definition,
        &[first, distinct],
    ));
}

#[test]
fn horizontal_relation_requires_one_line_or_two_points() {
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    let entity = |id: &str, geometry| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("sketch".into()),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let definition = |entities| SketchConstraintDefinition::Native {
        native_kind: "sldprt:marker-relation:4".into(),
        native_state: None,
        native_flags: None,
        native_properties: std::collections::BTreeMap::new(),
        entities,
        parameter: None,
        operands: Vec::new(),
    };
    let point = entity(
        "point",
        SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    );
    let line = entity(
        "line",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    );

    assert!(marker_relation_is_inactive(
        &relation,
        &definition(vec![point.id.clone()]),
        std::slice::from_ref(&point),
    ));
    assert!(!marker_relation_is_inactive(
        &relation,
        &definition(vec![line.id.clone()]),
        std::slice::from_ref(&line),
    ));
    assert!(!marker_relation_is_inactive(
        &relation,
        &definition(vec![point.id.clone(), SketchEntityId("second".into())]),
        &[
            point,
            entity(
                "second",
                SketchGeometry::Point {
                    position: Point2::new(1.0, 0.0),
                },
            ),
        ],
    ));
    assert!(marker_relation_is_inactive(
        &relation,
        &SketchConstraintDefinition::Native {
            native_kind: "sldprt:marker-relation:4".into(),
            native_state: None,
            native_flags: None,
            native_properties: std::collections::BTreeMap::new(),
            entities: Vec::new(),
            parameter: None,
            operands: vec![
                SketchNativeOperand {
                    native_kind: "sldprt:marker-local-id".into(),
                    native_field: None,
                    native_role: None,
                    object_index: 3,
                    native_ref: Some("same-marker".into()),
                },
                SketchNativeOperand {
                    native_kind: "sldprt:marker-local-id".into(),
                    native_field: None,
                    native_role: None,
                    object_index: 3,
                    native_ref: Some("same-marker".into()),
                },
            ],
        },
        &[],
    ));
}

#[test]
fn driving_point_distances_resolve_omitted_solver_points() {
    let mut origin = marker("origin", Some([0.0, 0.0]));
    origin.offset = 0;
    let mut negative = marker("negative", Some([-0.007, 0.0]));
    negative.offset = 1;
    let mut first_center = marker("first-center", Some([0.008, 0.0]));
    first_center.offset = 2;
    let mut second_center = marker("second-center", Some([0.0015, 0.0]));
    second_center.offset = 3;
    let operand = |index, marker: Option<&str>| FeatureInputOperand {
        offset: u64::from(index),
        reference_ref: format!("reference-{index}"),
        kind: FeatureInputOperandKind::Native(0x820f),
        entity_index: index,
        entity_ref: marker.map(str::to_string),
    };
    let scalar = |id: &str, value, operands| FeatureInputScalar {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature-native".into()),
        ordinal: 0,
        offset: 0,
        object_id: 0,
        name: "name".into(),
        value,
        role: FeatureInputScalarRole::Driving,
        entity_indices: Vec::new(),
        operands,
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: vec![
            scalar(
                "center-1",
                0.008,
                vec![operand(13, None), operand(3, Some("first-center"))],
            ),
            scalar(
                "center-2",
                0.0015,
                vec![operand(13, None), operand(4, Some("second-center"))],
            ),
            scalar(
                "terminal",
                0.007,
                vec![operand(12, None), operand(13, None)],
            ),
        ],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![origin, negative, first_center, second_center],
    };

    assert_eq!(
        inferred_point_coordinates_by_index(&lane, "feature-native"),
        HashMap::from([
            (3, [0.008, 0.0]),
            (4, [0.0015, 0.0]),
            (12, [-0.007, 0.0]),
            (13, [0.0, 0.0]),
        ])
    );
}

#[test]
fn ambiguous_driving_point_distance_does_not_assign_solver_points() {
    let mut first = marker("first", Some([0.0, 0.0]));
    first.offset = 0;
    let mut second = marker("second", Some([1.0, 0.0]));
    second.offset = 1;
    let mut third = marker("third", Some([2.0, 0.0]));
    third.offset = 2;
    let operand = |index| FeatureInputOperand {
        offset: u64::from(index),
        reference_ref: format!("reference-{index}"),
        kind: FeatureInputOperandKind::Native(0x820f),
        entity_index: index,
        entity_ref: None,
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: vec![FeatureInputScalar {
            id: "distance".into(),
            parent: "lane".into(),
            feature_ref: Some("feature-native".into()),
            ordinal: 0,
            offset: 0,
            object_id: 0,
            name: "name".into(),
            value: 1.0,
            role: FeatureInputScalarRole::Driving,
            entity_indices: Vec::new(),
            operands: vec![operand(12), operand(13)],
        }],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![first, second, third],
    };

    assert!(inferred_point_coordinates_by_index(&lane, "feature-native").is_empty());
}

#[test]
fn terminal_profile_curve_resolves_point_identity_endpoints() {
    let mut payload = vec![0; 92 + super::LEGACY_SKETCH_MARKER.len()];
    payload[..super::LEGACY_SKETCH_MARKER.len()].copy_from_slice(super::LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&15u16.to_le_bytes());
    payload[66..68].copy_from_slice(&16u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[92..].copy_from_slice(super::LEGACY_SKETCH_MARKER);
    let mut curve = marker("curve", None);
    curve.kind = SketchInputKind::LineOrCircle;
    let mut first = marker("first", Some([1.0, 0.0]));
    first.local_id = Some(15);
    first.object_index = Some(14);
    let mut second = marker("second", Some([2.0, 0.0]));
    second.object_index = Some(15);

    assert_eq!(
        legacy_terminal_profile_indexed_endpoints(&payload, &curve, &[&curve, &first, &second])
            .map(|endpoints| endpoints.map(|endpoint| endpoint.id.as_str())),
        Some(["first", "second"])
    );
}

#[test]
fn unary_relation_uses_one_resolved_reverse_curve_owner() {
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    relation.links = vec![SketchInputLink {
        local_id: 1,
        entity_ref: "point".into(),
    }];
    let mut owner = marker("owner", Some([1.0, 2.0]));
    owner.kind = SketchInputKind::LineOrCircle;
    owner.links = vec![SketchInputLink {
        local_id: 4,
        entity_ref: relation.id.clone(),
    }];
    let point = marker("point", None);
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (owner.id.as_str(), &owner),
        (point.id.as_str(), &point),
    ]);
    let line = SketchEntityId("line".into());
    let loci = HashMap::from([
        (owner.id.clone(), vec![SketchLocus::Entity(line.clone())]),
        (
            point.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId(
                "sldprt:model:sketch-entity#relation-point:1".into(),
            ))],
        ),
    ]);

    assert_eq!(
        typed_marker_relation_definition(&relation, &markers, &loci),
        Some(SketchConstraintDefinition::Horizontal {
            entity: line.clone(),
        })
    );
    let sketch = SketchId("sketch".into());
    let mut projected = SketchEntity {
        id: line,
        sketch: sketch.clone(),
        construction: true,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(0.0, 2.0),
        },
    };
    let definition = typed_marker_relation_definition_in_sketch(
        &relation,
        &sketch,
        std::slice::from_ref(&projected),
        &markers,
        &loci,
    )
    .expect("typed horizontal relation");
    assert!(matches!(
        definition,
        SketchConstraintDefinition::Horizontal { .. }
    ));
    assert!(marker_relation_is_inactive(
        &relation,
        &definition,
        std::slice::from_ref(&projected)
    ));
    projected.geometry = SketchGeometry::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 2.0),
    };
    let definition = typed_marker_relation_definition_in_sketch(
        &relation,
        &sketch,
        std::slice::from_ref(&projected),
        &markers,
        &loci,
    )
    .expect("typed horizontal relation");
    assert!(matches!(
        definition,
        SketchConstraintDefinition::Horizontal { .. }
    ));
    assert!(marker_relation_is_inactive(
        &relation,
        &definition,
        std::slice::from_ref(&projected)
    ));
}

#[test]
fn binary_relation_uses_two_resolved_reverse_curve_owners() {
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Parallel);
    let mut first_owner = marker("first-owner", Some([1.0, 2.0]));
    first_owner.kind = SketchInputKind::LineOrCircle;
    first_owner.offset = 1;
    first_owner.links = vec![SketchInputLink {
        local_id: 4,
        entity_ref: relation.id.clone(),
    }];
    let mut second_owner = marker("second-owner", Some([3.0, 4.0]));
    second_owner.kind = SketchInputKind::LineOrCircle;
    second_owner.offset = 2;
    second_owner.links = first_owner.links.clone();
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (first_owner.id.as_str(), &first_owner),
        (second_owner.id.as_str(), &second_owner),
    ]);
    let first = SketchEntityId("first".into());
    let second = SketchEntityId("second".into());
    let loci = HashMap::from([
        (
            first_owner.id.clone(),
            vec![SketchLocus::Entity(first.clone())],
        ),
        (
            second_owner.id.clone(),
            vec![SketchLocus::Entity(second.clone())],
        ),
    ]);

    assert_eq!(
        typed_marker_relation_definition(&relation, &markers, &loci),
        Some(SketchConstraintDefinition::Parallel {
            first: first.clone(),
            second: second.clone(),
        })
    );
    let sketch = SketchId("sketch".into());
    let line = |id, start, end| SketchEntity {
        id,
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let first_line = line(first, Point2::new(0.0, 0.0), Point2::new(4.0, 0.0));
    let mut second_line = line(second, Point2::new(0.0, 2.0), Point2::new(4.0, 2.0));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &relation,
            &sketch,
            &[first_line.clone(), second_line.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Parallel { .. })
    ));
    second_line.geometry = SketchGeometry::Line {
        start: Point2::new(0.0, 2.0),
        end: Point2::new(0.0, 6.0),
    };
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &relation,
            &sketch,
            &[first_line, second_line],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Native { .. })
    ));
}

#[test]
fn construction_line_endpoints_accept_reverse_incidence() {
    let mut line = marker("line", Some([0.5, 0.0]));
    line.kind = SketchInputKind::LineOrCircle;
    let mut first = marker("first", Some([0.0, 0.0]));
    first.offset = 1;
    first.links = vec![SketchInputLink {
        local_id: 4,
        entity_ref: line.id.clone(),
    }];
    let mut second = marker("second", Some([1.0, 0.0]));
    second.offset = 2;
    second.links = first.links.clone();
    let markers = HashMap::from([
        (line.id.as_str(), &line),
        (first.id.as_str(), &first),
        (second.id.as_str(), &second),
    ]);

    assert_eq!(
        line_endpoint_markers(&line, &markers),
        vec![&first, &second]
    );
}

#[test]
fn endpoint_incidence_binds_an_existing_profile_line() {
    let sketch_id = SketchId("sketch".into());
    let line_id = SketchEntityId("profile-line".into());
    let sketch = Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let entity = SketchEntity {
        id: line_id.clone(),
        sketch: sketch_id,
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    };
    let mut line = marker("line", Some([0.0005, 0.0]));
    line.kind = SketchInputKind::LineOrCircle;
    let mut first = marker("first", Some([0.0, 0.0]));
    first.offset = 1;
    first.links = vec![SketchInputLink {
        local_id: 4,
        entity_ref: line.id.clone(),
    }];
    let mut second = marker("second", Some([0.001, 0.0]));
    second.offset = 2;
    second.links = first.links.clone();
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
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![line, first, second],
    };

    assert_eq!(
        profile_loci_by_marker(&[feature], &[sketch], &[entity], &[lane])["line"],
        vec![SketchLocus::Entity(line_id)]
    );
}

#[test]
fn point_marker_materializing_a_circle_binds_its_center() {
    let sketch_id = SketchId("sketch".into());
    let circle_id = SketchEntityId("circle".into());
    let sketch = Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let entity = SketchEntity {
        id: circle_id.clone(),
        sketch: sketch_id,
        construction: false,
        native_ref: Some("circle-marker".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(1.0, 2.0),
            radius: Length(3.0),
        },
    };
    let mut circle_marker = marker("circle-marker", Some([1.0, 2.0]));
    circle_marker.kind = SketchInputKind::Point;
    circle_marker.feature_ref = Some("feature-native".into());
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
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![circle_marker],
    };

    assert_eq!(
        profile_loci_by_marker(&[feature], &[sketch], &[entity], &[lane])["circle-marker"],
        vec![SketchLocus::Center(circle_id)]
    );
}

#[test]
fn point_operand_canonicalizes_shared_endpoint_loci() {
    let sketch_id = SketchId("sketch".into());
    let sketch = Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let first_id = SketchEntityId("a-first".into());
    let second_id = SketchEntityId("z-second".into());
    let first = SketchEntity {
        id: first_id.clone(),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: vec!["first-start".into(), "shared".into()],
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    };
    let second = SketchEntity {
        id: second_id.clone(),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: vec!["shared".into(), "second-end".into()],
        geometry: SketchGeometry::Line {
            start: Point2::new(1.0, 0.0),
            end: Point2::new(1.0, 1.0),
        },
    };
    let mut first_start = marker("first-start", Some([0.0, 0.0]));
    first_start.offset = 1;
    let mut shared = marker("shared", Some([0.001, 0.0]));
    shared.offset = 2;
    let mut second_end = marker("second-end", Some([0.001, 0.001]));
    second_end.offset = 3;
    let relation = FeatureInputRelationInstance {
        id: "point-relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 4,
        family: FeatureInputRelationFamily::CircleDiameter,
        class_ref: "class".into(),
        feature_ref: "feature-native".into(),
        scalar_refs: Vec::new(),
        parameter_scalar_ref: None,
        display_scalar_ref: None,
        operands: vec![FeatureInputOperand {
            offset: 5,
            reference_ref: "shared-reference".into(),
            kind: FeatureInputOperandKind::Native(0x8ab6),
            entity_index: 0,
            entity_ref: Some("shared".into()),
        }],
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![relation],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![first_start, shared, second_end],
    };

    let loci = profile_loci_by_marker(
        &[feature],
        std::slice::from_ref(&sketch),
        &[first, second],
        std::slice::from_ref(&lane),
    );

    assert_eq!(
        loci["shared"],
        vec![SketchLocus::End(first_id)],
        "shared point markers use the canonical physical endpoint locus"
    );
}

#[test]
fn distance_fallback_requires_one_locus_in_the_complete_sketch() {
    let sketch = SketchId("sketch".into());
    let point = |id: &str, u: f64, v: f64| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(u, v),
        },
    };
    let known = point("known", 0.0, 0.0);
    let candidate = point("candidate", 3.0, 4.0);
    let parameter = DesignParameter {
        id: ParameterId("distance".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let known_locus = SketchLocus::Entity(known.id.clone());
    assert_eq!(
        unique_profile_distance_locus(
            &sketch,
            &known_locus,
            &parameter,
            &[known.clone(), candidate.clone()],
        ),
        Some(SketchLocus::Entity(candidate.id.clone()))
    );

    let ambiguous = point("ambiguous", -3.0, -4.0);
    assert_eq!(
        unique_profile_distance_locus(
            &sketch,
            &known_locus,
            &parameter,
            &[known, candidate, ambiguous],
        ),
        None
    );
}

#[test]
fn curve_operand_rejects_a_point_qualified_geometry_alias() {
    let sketch = SketchId("sketch".into());
    let point_id = SketchEntityId("point".into());
    let line_id = SketchEntityId("line".into());
    let circle_id = SketchEntityId("circle".into());
    let entities = vec![
        SketchEntity {
            id: point_id.clone(),
            sketch: sketch.clone(),
            construction: true,
            native_ref: None,
            geometry_ref: Some("curve-marker".into()),
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(0.5, 0.0),
            },
        },
        SketchEntity {
            id: line_id.clone(),
            sketch,
            construction: false,
            native_ref: Some("line-marker".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            },
        },
        SketchEntity {
            id: circle_id.clone(),
            sketch: SketchId("sketch".into()),
            construction: false,
            native_ref: Some("circle-marker".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length(1.0),
            },
        },
    ];
    let loci = HashMap::from([
        ("curve-marker".into(), vec![SketchLocus::Entity(point_id)]),
        (
            "line-marker".into(),
            vec![SketchLocus::Entity(line_id.clone())],
        ),
        (
            "circle-marker".into(),
            vec![SketchLocus::Entity(circle_id.clone())],
        ),
    ]);

    assert_eq!(
        single_marker_curve_entity("curve-marker", &HashMap::new(), &loci, &entities),
        None
    );
    assert_eq!(
        single_marker_curve_entity("line-marker", &HashMap::new(), &loci, &entities),
        Some(line_id)
    );
    assert_eq!(
        single_marker_line_entity("circle-marker", &HashMap::new(), &loci, &entities),
        None
    );
    assert_eq!(
        single_marker_curve_entity("circle-marker", &HashMap::new(), &loci, &entities),
        Some(circle_id)
    );
}

#[test]
fn line_operand_uses_linked_endpoint_incidence_beside_a_direct_point_locus() {
    let sketch = SketchId("sketch".into());
    let line_id = SketchEntityId("line".into());
    let misleading_line_id = SketchEntityId("misleading-line".into());
    let point_id = SketchEntityId("display-point".into());
    let first_point_id = SketchEntityId("first-point".into());
    let second_point_id = SketchEntityId("second-point".into());
    let entities = vec![
        SketchEntity {
            id: line_id.clone(),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            },
        },
        SketchEntity {
            id: point_id.clone(),
            sketch,
            construction: true,
            native_ref: Some("handle".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(0.5, 0.0),
            },
        },
        SketchEntity {
            id: misleading_line_id.clone(),
            sketch: SketchId("sketch".into()),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(0.0, 1.0),
                end: Point2::new(1.0, 1.0),
            },
        },
        SketchEntity {
            id: SketchEntityId("other-sketch-line".into()),
            sketch: SketchId("other-sketch".into()),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            },
        },
        SketchEntity {
            id: first_point_id.clone(),
            sketch: SketchId("sketch".into()),
            construction: true,
            native_ref: Some("first".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(0.25, 0.0),
            },
        },
        SketchEntity {
            id: second_point_id.clone(),
            sketch: SketchId("sketch".into()),
            construction: true,
            native_ref: Some("second".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(0.75, 0.0),
            },
        },
    ];
    let mut first = marker("first", Some([0.0, 0.0]));
    let second = marker("second", Some([0.001, 0.0]));
    let misleading = marker("misleading", None);
    first.links = vec![SketchInputLink {
        local_id: 3,
        entity_ref: misleading.id.clone(),
    }];
    let mut handle = marker("handle", Some([0.0005, 0.0]));
    handle.links = vec![
        SketchInputLink {
            local_id: 1,
            entity_ref: first.id.clone(),
        },
        SketchInputLink {
            local_id: 2,
            entity_ref: second.id.clone(),
        },
    ];
    let markers = HashMap::from([
        (first.id.as_str(), &first),
        (second.id.as_str(), &second),
        (handle.id.as_str(), &handle),
        (misleading.id.as_str(), &misleading),
    ]);
    let loci = HashMap::from([
        ("first".into(), vec![SketchLocus::Entity(first_point_id)]),
        ("second".into(), vec![SketchLocus::Entity(second_point_id)]),
        ("handle".into(), vec![SketchLocus::Entity(point_id)]),
        (
            "misleading".into(),
            vec![SketchLocus::Entity(misleading_line_id)],
        ),
    ]);

    assert_eq!(
        single_marker_line_entity("handle", &markers, &loci, &entities),
        Some(line_id)
    );
}

#[test]
fn line_operand_uses_the_unique_profile_line_through_a_point_handle() {
    let sketch = SketchId("sketch".into());
    let line_id = SketchEntityId("line".into());
    let point_id = SketchEntityId("point-entity".into());
    let entities = vec![
        SketchEntity {
            id: line_id.clone(),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(2.0, 0.0),
            },
        },
        SketchEntity {
            id: point_id.clone(),
            sketch,
            construction: true,
            native_ref: Some("point-handle".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(1.0, 0.0),
            },
        },
    ];
    let point = marker("point-handle", Some([1.0, 0.0]));
    let markers = HashMap::from([(point.id.as_str(), &point)]);
    let loci = HashMap::from([(point.id.clone(), vec![SketchLocus::Entity(point_id)])]);

    assert_eq!(
        single_marker_line_entity("point-handle", &markers, &loci, &entities),
        Some(line_id)
    );
}

#[test]
fn axis_relation_preserves_native_kind_and_reports_unsatisfied_geometry() {
    let sketch = SketchId("sketch".into());
    let first_id = SketchEntityId("first".into());
    let second_id = SketchEntityId("second".into());
    let line = |id: SketchEntityId, start: Point2, end: Point2| SketchEntity {
        id,
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let entities = vec![
        line(
            first_id.clone(),
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        ),
        line(
            second_id.clone(),
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 1.0),
        ),
    ];
    let first = marker("first-marker", None);
    let second = marker("second-marker", None);
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::HorizontalPoints);
    relation.links = vec![
        SketchInputLink {
            local_id: 1,
            entity_ref: first.id.clone(),
        },
        SketchInputLink {
            local_id: 2,
            entity_ref: second.id.clone(),
        },
    ];
    let markers = HashMap::from([
        (first.id.as_str(), &first),
        (second.id.as_str(), &second),
        (relation.id.as_str(), &relation),
    ]);
    let loci = HashMap::from([
        (first.id.clone(), vec![SketchLocus::Start(first_id)]),
        (second.id.clone(), vec![SketchLocus::End(second_id)]),
    ]);

    let definition =
        typed_marker_relation_definition_in_sketch(&relation, &sketch, &entities, &markers, &loci)
            .expect("typed horizontal-points relation");
    assert!(matches!(
        definition,
        SketchConstraintDefinition::HorizontalPoints { .. }
    ));
    assert!(marker_relation_is_inactive(
        &relation,
        &definition,
        &entities
    ));

    let mut swapped_relation = relation.clone();
    swapped_relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    let swapped_loci = HashMap::from([
        (
            first.id.clone(),
            vec![SketchLocus::End(SketchEntityId("first".into()))],
        ),
        (
            second.id.clone(),
            vec![SketchLocus::End(SketchEntityId("second".into()))],
        ),
    ]);
    let definition = typed_marker_relation_definition_in_sketch(
        &swapped_relation,
        &sketch,
        &entities,
        &markers,
        &swapped_loci,
    )
    .expect("typed legacy horizontal relation");
    assert!(matches!(
        definition,
        SketchConstraintDefinition::HorizontalPoints { .. }
    ));
    assert!(marker_relation_is_inactive(
        &swapped_relation,
        &definition,
        &entities
    ));

    let mut owner_relation = marker("owner-relation", None);
    owner_relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    let mut first_owner = marker("first-owner", Some([0.0, 0.0]));
    first_owner.kind = SketchInputKind::Point;
    first_owner.links = vec![SketchInputLink {
        local_id: 1,
        entity_ref: owner_relation.id.clone(),
    }];
    let mut second_owner = marker("second-owner", Some([0.0, 1.0]));
    second_owner.kind = SketchInputKind::Point;
    second_owner.links = first_owner.links.clone();
    let first_point = SketchEntityId("first-point".into());
    let second_point = SketchEntityId("second-point".into());
    let point = |id, position| SketchEntity {
        id,
        sketch: sketch.clone(),
        construction: true,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let owner_entities = [
        point(first_point.clone(), Point2::new(0.0, 0.0)),
        point(second_point.clone(), Point2::new(0.0, 1.0)),
    ];
    let owner_markers = HashMap::from([
        (owner_relation.id.as_str(), &owner_relation),
        (first_owner.id.as_str(), &first_owner),
        (second_owner.id.as_str(), &second_owner),
    ]);
    let owner_loci = HashMap::from([
        (
            first_owner.id.clone(),
            vec![SketchLocus::Entity(first_point)],
        ),
        (
            second_owner.id.clone(),
            vec![SketchLocus::Entity(second_point)],
        ),
    ]);
    let definition = typed_marker_relation_definition_in_sketch(
        &owner_relation,
        &sketch,
        &owner_entities,
        &owner_markers,
        &owner_loci,
    )
    .expect("typed owner horizontal relation");
    assert!(matches!(
        definition,
        SketchConstraintDefinition::HorizontalPoints { .. }
    ));
    assert!(marker_relation_is_inactive(
        &owner_relation,
        &definition,
        &owner_entities
    ));
}

#[test]
fn dimension_preserves_structurally_typed_operands_when_geometry_disagrees() {
    let sketch = SketchId("sketch".into());
    let entities = [
        SketchEntity {
            id: SketchEntityId("first".into()),
            sketch: sketch.clone(),
            construction: true,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(0.0, 0.0),
            },
        },
        SketchEntity {
            id: SketchEntityId("second".into()),
            sketch: sketch.clone(),
            construction: true,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(3.0, 4.0),
            },
        },
    ];
    let first = marker("first-marker", None);
    let second = marker("second-marker", None);
    let markers = HashMap::from([(first.id.as_str(), &first), (second.id.as_str(), &second)]);
    let loci = HashMap::from([
        (
            first.id.clone(),
            vec![SketchLocus::Entity(entities[0].id.clone())],
        ),
        (
            second.id.clone(),
            vec![SketchLocus::Entity(entities[1].id.clone())],
        ),
    ]);
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalar_refs: Vec::new(),
        parameter_scalar_ref: Some("scalar".into()),
        display_scalar_ref: None,
        operands: [&first, &second]
            .into_iter()
            .enumerate()
            .map(|(index, marker)| FeatureInputOperand {
                offset: index as u64,
                reference_ref: format!("reference-{index}"),
                kind: FeatureInputOperandKind::D6,
                entity_index: index as u16,
                entity_ref: Some(marker.id.clone()),
            })
            .collect(),
    };
    let parameter = DesignParameter {
        id: ParameterId("distance".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "4mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(4.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("scalar".into()),
    };

    let definition = typed_relation_definition(
        &relation,
        Some(&parameter),
        &sketch,
        &entities,
        &markers,
        &loci,
    )
    .expect("stored relation operands are authoritative");
    assert!(matches!(
        definition,
        SketchConstraintDefinition::DistanceLoci { .. }
    ));
    assert!(relation_constraint_is_inactive(
        Some(&parameter),
        &definition,
        &entities
    ));

    let mut exact_entities = entities.clone();
    exact_entities[0].native_ref = Some(first.id.clone());
    exact_entities[1].native_ref = Some(second.id.clone());
    assert!(matches!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &exact_entities,
            &markers,
            &HashMap::new(),
        ),
        Some(SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Entity(first),
            second: SketchLocus::Entity(second),
            ..
        }) if first == exact_entities[0].id && second == exact_entities[1].id
    ));
}

#[test]
fn line_distance_repairs_distinct_operands_collapsed_to_one_marker() {
    let sketch = SketchId("sketch".into());
    let line = |id: &str, v| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: true,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, v),
            end: Point2::new(10.0, v),
        },
    };
    let entities = [line("resolved", 0.0), line("unique-partner", 5.0)];
    let marker = marker("collapsed-marker", None);
    let markers = HashMap::from([(marker.id.as_str(), &marker)]);
    let loci = HashMap::from([(
        marker.id.clone(),
        vec![SketchLocus::Entity(entities[0].id.clone())],
    )]);
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::LineLineDistance,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalar_refs: Vec::new(),
        parameter_scalar_ref: Some("scalar".into()),
        display_scalar_ref: None,
        operands: [7, 10]
            .into_iter()
            .map(|entity_index| FeatureInputOperand {
                offset: u64::from(entity_index),
                reference_ref: format!("reference-{entity_index}"),
                kind: FeatureInputOperandKind::Native(0x8386),
                entity_index,
                entity_ref: Some(marker.id.clone()),
            })
            .collect(),
    };
    let parameter = DesignParameter {
        id: ParameterId("distance".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("scalar".into()),
    };

    assert!(matches!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Distance { entities: pair, .. })
            if pair == entities.iter().map(|entity| entity.id.clone()).collect::<Vec<_>>()
    ));
}

#[test]
fn line_distance_uses_an_addressed_point_to_select_the_missing_line() {
    let sketch = SketchId("sketch".into());
    let line = |id: &str, v| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, v),
            end: Point2::new(10.0, v),
        },
    };
    let known = line("known", 0.0);
    let intended = line("intended", 5.0);
    let distractor = line("distractor", -5.0);
    let point = SketchEntity {
        id: SketchEntityId("addressed-point".into()),
        sketch: sketch.clone(),
        construction: true,
        native_ref: Some("point-marker".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(3.0, 5.0),
        },
    };
    let known_marker = marker("known-marker", None);
    let mut point_marker = marker("point-marker", Some([0.003, 0.005]));
    point_marker.local_id = Some(13);
    let markers = HashMap::from([
        (known_marker.id.as_str(), &known_marker),
        (point_marker.id.as_str(), &point_marker),
    ]);
    let loci = HashMap::from([
        (
            known_marker.id.clone(),
            vec![SketchLocus::Entity(known.id.clone())],
        ),
        (
            point_marker.id.clone(),
            vec![SketchLocus::Entity(point.id.clone())],
        ),
    ]);
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::LineLineDistance,
        class_ref: "class".into(),
        feature_ref: "feature-native".into(),
        scalar_refs: Vec::new(),
        parameter_scalar_ref: Some("scalar".into()),
        display_scalar_ref: None,
        operands: vec![
            FeatureInputOperand {
                offset: 0,
                reference_ref: "missing-reference".into(),
                kind: FeatureInputOperandKind::Native(0x8386),
                entity_index: 13,
                entity_ref: None,
            },
            FeatureInputOperand {
                offset: 1,
                reference_ref: "known-reference".into(),
                kind: FeatureInputOperandKind::Native(0x8386),
                entity_index: 6,
                entity_ref: Some(known_marker.id.clone()),
            },
        ],
    };
    let parameter = DesignParameter {
        id: ParameterId("distance".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("scalar".into()),
    };
    let entities = [known.clone(), intended.clone(), distractor, point];

    assert!(matches!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Distance { entities: pair, .. })
            if pair == vec![intended.id, known.id]
    ));
}

#[test]
fn binary_relations_require_matching_evaluated_geometry() {
    use SketchRelationKind::{
        Collinear, Concentric, Coradial, Equal, Parallel, Perpendicular, Tangent,
    };
    let sketch = SketchId("sketch".into());
    let entity = |id: &str, geometry| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let horizontal = entity(
        "horizontal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(4.0, 0.0),
        },
    );
    let parallel = entity(
        "parallel",
        SketchGeometry::Line {
            start: Point2::new(0.0, 2.0),
            end: Point2::new(4.0, 2.0),
        },
    );
    let perpendicular = entity(
        "perpendicular",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(0.0, 4.0),
        },
    );
    let collinear = entity(
        "collinear",
        SketchGeometry::Line {
            start: Point2::new(6.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
    );
    let circle = |id: &str, u, v, radius| {
        entity(
            id,
            SketchGeometry::Circle {
                center: Point2::new(u, v),
                radius: Length(radius),
            },
        )
    };
    let first_circle = circle("first-circle", 0.0, 2.0, 2.0);
    let equal_circle = circle("equal-circle", 4.0, 2.0, 2.0);
    let concentric_circle = circle("concentric-circle", 0.0, 2.0, 1.0);
    let coradial_circle = circle("coradial-circle", 0.0, 2.0, 2.0);
    let unrelated_circle = circle("unrelated-circle", 8.0, 8.0, 3.0);

    for (kind, first, second) in [
        (Parallel, &horizontal, &parallel),
        (Perpendicular, &horizontal, &perpendicular),
        (Collinear, &horizontal, &collinear),
        (Equal, &first_circle, &equal_circle),
        (Concentric, &first_circle, &concentric_circle),
        (Coradial, &first_circle, &coradial_circle),
        (Tangent, &horizontal, &first_circle),
        (Tangent, &first_circle, &equal_circle),
    ] {
        assert!(binary_relation_matches_evaluated_geometry(
            kind, first, second
        ));
    }
    for kind in [
        Parallel,
        Perpendicular,
        Collinear,
        Equal,
        Concentric,
        Tangent,
        Coradial,
    ] {
        assert!(!binary_relation_matches_evaluated_geometry(
            kind,
            &horizontal,
            &unrelated_circle,
        ));
    }
}

#[test]
fn locus_relations_require_matching_evaluated_geometry() {
    let sketch = SketchId("sketch".into());
    let entity = |id: &str, geometry| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: true,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let mut first = entity(
        "first",
        SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    );
    let mut second = entity(
        "second",
        SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    );
    let line = entity(
        "line",
        SketchGeometry::Line {
            start: Point2::new(-2.0, 0.0),
            end: Point2::new(2.0, 0.0),
        },
    );
    let mut arc = entity(
        "arc",
        SketchGeometry::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length(1.0),
            start_angle: cadmpeg_ir::features::Angle(0.0),
            end_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
        },
    );
    let symmetric_first = entity(
        "symmetric-first",
        SketchGeometry::Point {
            position: Point2::new(-1.0, 2.0),
        },
    );
    let mut symmetric_second = entity(
        "symmetric-second",
        SketchGeometry::Point {
            position: Point2::new(1.0, 2.0),
        },
    );
    let symmetry_axis = entity(
        "symmetry-axis",
        SketchGeometry::Line {
            start: Point2::new(0.0, -3.0),
            end: Point2::new(0.0, 3.0),
        },
    );
    let mut first_marker = marker("first-marker", None);
    let second_marker = marker("second-marker", None);
    let mut line_marker = marker("line-marker", None);
    line_marker.kind = SketchInputKind::LineOrCircle;
    let mut arc_marker = marker("arc-marker", None);
    arc_marker.kind = SketchInputKind::Arc;
    let symmetric_first_marker = marker("symmetric-first-marker", None);
    let symmetric_second_marker = marker("symmetric-second-marker", None);
    let mut symmetry_axis_marker = marker("symmetry-axis-marker", None);
    symmetry_axis_marker.kind = SketchInputKind::LineOrCircle;
    let mut coincident = marker("coincident", None);
    coincident.kind = SketchInputKind::Relation(SketchRelationKind::Coincident);
    coincident.links = [(&first_marker, 1), (&second_marker, 2)]
        .map(|(marker, local_id)| SketchInputLink {
            local_id,
            entity_ref: marker.id.clone(),
        })
        .to_vec();
    let mut merge_points = coincident.clone();
    merge_points.id = "merge-points".into();
    merge_points.kind = SketchInputKind::Relation(SketchRelationKind::MergePoints);
    let mut midpoint = marker("midpoint", None);
    midpoint.kind = SketchInputKind::Relation(SketchRelationKind::Midpoint);
    midpoint.links = [(&first_marker, 1), (&line_marker, 3)]
        .map(|(marker, local_id)| SketchInputLink {
            local_id,
            entity_ref: marker.id.clone(),
        })
        .to_vec();
    let mut arc_angle = marker("arc-angle", None);
    arc_angle.kind = SketchInputKind::Relation(SketchRelationKind::ArcAngle90);
    arc_angle.links = vec![SketchInputLink {
        local_id: 4,
        entity_ref: arc_marker.id.clone(),
    }];
    let mut symmetric = marker("symmetric", None);
    symmetric.kind = SketchInputKind::Relation(SketchRelationKind::Symmetric);
    symmetric.links = [(&symmetric_first_marker, 5), (&symmetric_second_marker, 6)]
        .map(|(marker, local_id)| SketchInputLink {
            local_id,
            entity_ref: marker.id.clone(),
        })
        .to_vec();
    symmetry_axis_marker.links.push(SketchInputLink {
        local_id: 7,
        entity_ref: symmetric.id.clone(),
    });
    let mut at_intersection = marker("at-intersection", None);
    at_intersection.kind = SketchInputKind::Relation(SketchRelationKind::AtIntersection);
    at_intersection.links = [(&line_marker, 9), (&symmetry_axis_marker, 10)]
        .map(|(marker, local_id)| SketchInputLink {
            local_id,
            entity_ref: marker.id.clone(),
        })
        .to_vec();
    first_marker.links.push(SketchInputLink {
        local_id: 8,
        entity_ref: at_intersection.id.clone(),
    });
    let markers = HashMap::from([
        (first_marker.id.as_str(), &first_marker),
        (second_marker.id.as_str(), &second_marker),
        (line_marker.id.as_str(), &line_marker),
        (arc_marker.id.as_str(), &arc_marker),
        (symmetric_first_marker.id.as_str(), &symmetric_first_marker),
        (
            symmetric_second_marker.id.as_str(),
            &symmetric_second_marker,
        ),
        (symmetry_axis_marker.id.as_str(), &symmetry_axis_marker),
        (coincident.id.as_str(), &coincident),
        (merge_points.id.as_str(), &merge_points),
        (midpoint.id.as_str(), &midpoint),
        (arc_angle.id.as_str(), &arc_angle),
        (symmetric.id.as_str(), &symmetric),
        (at_intersection.id.as_str(), &at_intersection),
    ]);
    let loci = HashMap::from([
        (
            first_marker.id.clone(),
            vec![SketchLocus::Entity(first.id.clone())],
        ),
        (
            second_marker.id.clone(),
            vec![SketchLocus::Entity(second.id.clone())],
        ),
        (
            line_marker.id.clone(),
            vec![SketchLocus::Entity(line.id.clone())],
        ),
        (
            arc_marker.id.clone(),
            vec![SketchLocus::Entity(arc.id.clone())],
        ),
        (
            symmetric_first_marker.id.clone(),
            vec![SketchLocus::Entity(symmetric_first.id.clone())],
        ),
        (
            symmetric_second_marker.id.clone(),
            vec![SketchLocus::Entity(symmetric_second.id.clone())],
        ),
        (
            symmetry_axis_marker.id.clone(),
            vec![SketchLocus::Entity(symmetry_axis.id.clone())],
        ),
    ]);
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &coincident,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::CoincidentLoci { .. })
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &merge_points,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::CoincidentLoci { .. })
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &midpoint,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Midpoint { .. })
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &arc_angle,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::ArcAngle { .. })
    ));
    assert_eq!(
        typed_marker_relation_definition_in_sketch(
            &symmetric,
            &sketch,
            &[
                symmetric_first.clone(),
                symmetric_second.clone(),
                symmetry_axis.clone(),
            ],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Symmetric {
            first: SketchLocus::Entity(symmetric_first.id.clone()),
            second: SketchLocus::Entity(symmetric_second.id.clone()),
            axis: symmetry_axis.id.clone(),
        })
    );
    assert_eq!(
        typed_marker_relation_definition_in_sketch(
            &at_intersection,
            &sketch,
            &[first.clone(), line.clone(), symmetry_axis.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::AtIntersection {
            point: SketchLocus::Entity(first.id.clone()),
            first: line.id.clone(),
            second: symmetry_axis.id.clone(),
        })
    );

    second.geometry = SketchGeometry::Point {
        position: Point2::new(1.0, 0.0),
    };
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &coincident,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Native { .. })
    ));
    first.clone_from(&entity(
        "first",
        SketchGeometry::Point {
            position: Point2::new(1.0, 0.0),
        },
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &at_intersection,
            &sketch,
            &[first.clone(), line.clone(), symmetry_axis.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Native { .. })
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &midpoint,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Native { .. })
    ));
    arc.geometry = SketchGeometry::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length(1.0),
        start_angle: cadmpeg_ir::features::Angle(0.0),
        end_angle: cadmpeg_ir::features::Angle(std::f64::consts::PI),
    };
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &arc_angle,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Native { .. })
    ));
    symmetric_second.geometry = SketchGeometry::Point {
        position: Point2::new(2.0, 2.0),
    };
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &symmetric,
            &sketch,
            &[symmetric_first, symmetric_second, symmetry_axis],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Native { .. })
    ));
}

#[test]
fn distance_pair_fallback_requires_one_pair_in_the_complete_sketch() {
    let sketch = SketchId("sketch".into());
    let point = |id: &str, u: f64, v: f64| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(u, v),
        },
    };
    let parameter = DesignParameter {
        id: ParameterId("distance".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let first = point("first", 0.0, 0.0);
    let coincident_first = point("z-coincident-first", 0.0, 0.0);
    let second = point("second", 3.0, 4.0);
    let unrelated = point("unrelated", 20.0, 20.0);
    assert_eq!(
        unique_profile_distance_loci_pair(
            &sketch,
            &parameter,
            &[
                first.clone(),
                coincident_first,
                second.clone(),
                unrelated.clone(),
            ],
        ),
        Some((
            SketchLocus::Entity(first.id.clone()),
            SketchLocus::Entity(second.id.clone()),
        ))
    );

    let ambiguous = point("ambiguous", 23.0, 24.0);
    assert_eq!(
        unique_profile_distance_loci_pair(
            &sketch,
            &parameter,
            &[first, second, unrelated, ambiguous],
        ),
        None
    );
}

#[test]
fn axis_distance_fallback_requires_one_pair_in_the_complete_sketch() {
    let sketch = SketchId("sketch".into());
    let point = |id: &str, u: f64, v: f64| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(u, v),
        },
    };
    let first = point("first", 0.0, 0.0);
    let second = point("second", 5.0, 20.0);
    let unrelated = point("unrelated", 100.0, 100.0);
    let parameter = DesignParameter {
        id: ParameterId("distance".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let first_locus = SketchLocus::Entity(first.id.clone());
    let second_locus = SketchLocus::Entity(second.id.clone());
    let entities = [first.clone(), second.clone(), unrelated.clone()];
    assert_eq!(
        unique_profile_axis_distance_locus(&sketch, &first_locus, &parameter, &entities, true,),
        Some(second_locus.clone())
    );
    assert_eq!(
        unique_profile_axis_distance_pair(&sketch, &parameter, &entities, true),
        Some((first_locus, second_locus))
    );

    let ambiguous = point("ambiguous", 10.0, 30.0);
    assert_eq!(
        unique_profile_axis_distance_pair(
            &sketch,
            &parameter,
            &[first, second, unrelated, ambiguous],
            true,
        ),
        None
    );
}

#[test]
fn line_distance_fallback_requires_one_parallel_pair_in_the_complete_sketch() {
    let sketch = SketchId("sketch".into());
    let line = |id: &str, start: Point2, end: Point2| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let first = line("first", Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let second = line("second", Point2::new(0.0, 5.0), Point2::new(10.0, 5.0));
    let unrelated = line(
        "unrelated",
        Point2::new(20.0, 20.0),
        Point2::new(21.0, 21.0),
    );
    let parameter = DesignParameter {
        id: ParameterId("distance".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let entities = [first.clone(), second.clone(), unrelated.clone()];
    assert_eq!(
        unique_profile_line_distance_entity(&sketch, &first.id, &parameter, &entities),
        Some(second.id.clone())
    );
    assert_eq!(
        unique_profile_line_distance_pair(&sketch, &parameter, &entities),
        Some((first.id.clone(), second.id.clone()))
    );

    let wrong = line("wrong", Point2::new(0.0, 2.0), Point2::new(10.0, 2.0));
    assert_eq!(
        unique_repaired_profile_line_distance_pair(
            &sketch,
            &first.id,
            &wrong.id,
            &parameter,
            &[
                first.clone(),
                wrong.clone(),
                second.clone(),
                unrelated.clone(),
            ],
        ),
        Some((first.id.clone(), second.id.clone()))
    );

    let other_solved = line(
        "other-solved",
        Point2::new(0.0, -5.0),
        Point2::new(10.0, -5.0),
    );
    assert_eq!(
        unique_repaired_profile_line_distance_pair(
            &sketch,
            &first.id,
            &wrong.id,
            &parameter,
            &[first.clone(), wrong.clone(), second.clone(), other_solved,],
        ),
        None
    );

    let unrelated_first = line(
        "unrelated-first",
        Point2::new(20.0, 20.0),
        Point2::new(30.0, 20.0),
    );
    let unrelated_second = line(
        "unrelated-second",
        Point2::new(20.0, 25.0),
        Point2::new(30.0, 25.0),
    );
    assert_eq!(
        unique_repaired_profile_line_distance_pair(
            &sketch,
            &first.id,
            &wrong.id,
            &parameter,
            &[
                first.clone(),
                wrong.clone(),
                unrelated_first,
                unrelated_second,
            ],
        ),
        None
    );

    let ambiguous = line("ambiguous", Point2::new(0.0, 10.0), Point2::new(10.0, 10.0));
    assert_eq!(
        unique_profile_line_distance_pair(
            &sketch,
            &parameter,
            &[first, second, unrelated, ambiguous],
        ),
        None
    );
}

#[test]
fn line_angle_fallback_requires_one_pair_in_the_complete_sketch() {
    let sketch = SketchId("sketch".into());
    let line = |id: &str, start: Point2, end: Point2| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let horizontal = line("horizontal", Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let vertical = line("vertical", Point2::new(0.0, 0.0), Point2::new(0.0, 10.0));
    let diagonal = line("diagonal", Point2::new(20.0, 20.0), Point2::new(21.0, 21.0));
    let parameter = DesignParameter {
        id: ParameterId("angle".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "90deg".into(),
        display: None,
        value: Some(ParameterValue::Angle(Angle(std::f64::consts::FRAC_PI_2))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let entities = [horizontal.clone(), vertical.clone(), diagonal.clone()];
    assert_eq!(
        unique_profile_line_angle_entity(&sketch, &horizontal.id, &parameter, &entities),
        Some(vertical.id.clone())
    );
    assert_eq!(
        unique_profile_line_angle_pair(&sketch, &parameter, &entities),
        Some((horizontal.id.clone(), vertical.id.clone()))
    );

    let wrong = line(
        "wrong",
        Point2::new(0.0, 0.0),
        Point2::new(3.0_f64.sqrt(), 1.0),
    );
    assert_eq!(
        unique_repaired_profile_line_angle_pair(
            &sketch,
            &horizontal.id,
            &wrong.id,
            &parameter,
            &[
                horizontal.clone(),
                wrong.clone(),
                vertical.clone(),
                diagonal.clone(),
            ],
        ),
        Some((horizontal.id.clone(), vertical.id.clone()))
    );

    let ambiguous = line("ambiguous", Point2::new(5.0, 0.0), Point2::new(5.0, 10.0));
    assert_eq!(
        unique_repaired_profile_line_angle_pair(
            &sketch,
            &horizontal.id,
            &wrong.id,
            &parameter,
            &[
                horizontal.clone(),
                wrong.clone(),
                vertical.clone(),
                ambiguous.clone(),
            ],
        ),
        None
    );

    let unrelated_first = line(
        "unrelated-first",
        Point2::new(0.0, 0.0),
        Point2::new(0.5, 3.0_f64.sqrt() * 0.5),
    );
    let unrelated_second = line(
        "unrelated-second",
        Point2::new(0.0, 0.0),
        Point2::new(-3.0_f64.sqrt() * 0.5, 0.5),
    );
    assert_eq!(
        unique_repaired_profile_line_angle_pair(
            &sketch,
            &horizontal.id,
            &wrong.id,
            &parameter,
            &[
                horizontal.clone(),
                wrong.clone(),
                unrelated_first,
                unrelated_second,
            ],
        ),
        None
    );
    assert_eq!(
        unique_profile_line_angle_pair(
            &sketch,
            &parameter,
            &[horizontal, vertical, diagonal, ambiguous],
        ),
        None
    );
}

#[test]
fn point_line_fallback_requires_one_pair_in_the_complete_sketch() {
    let sketch = SketchId("sketch".into());
    let point = SketchEntity {
        id: SketchEntityId("point".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(0.0, 5.0),
        },
    };
    let line = |id: &str, start: Point2, end: Point2| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let horizontal = line("horizontal", Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let unrelated = line(
        "unrelated",
        Point2::new(100.0, 20.0),
        Point2::new(100.0, 30.0),
    );
    let parameter = DesignParameter {
        id: ParameterId("distance".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let point_locus = SketchLocus::Entity(point.id.clone());
    let entities = [point.clone(), horizontal.clone(), unrelated.clone()];
    assert_eq!(
        unique_profile_point_line_entity(&sketch, &point_locus, &parameter, &entities),
        Some(horizontal.id.clone())
    );
    assert_eq!(
        unique_profile_line_point_locus(&sketch, &horizontal.id, &parameter, &entities),
        Some(point_locus.clone())
    );
    assert_eq!(
        unique_profile_point_line_pair(&sketch, &parameter, &entities),
        Some((point_locus, horizontal.id.clone()))
    );

    let wrong = line("wrong", Point2::new(0.0, 2.0), Point2::new(10.0, 2.0));
    assert_eq!(
        unique_repaired_profile_point_line_pair(
            &sketch,
            &SketchLocus::Entity(point.id.clone()),
            &wrong.id,
            &parameter,
            &[
                point.clone(),
                wrong.clone(),
                horizontal.clone(),
                unrelated.clone(),
            ],
        ),
        Some((SketchLocus::Entity(point.id.clone()), horizontal.id.clone(),))
    );

    let ambiguous = line("ambiguous", Point2::new(0.0, 10.0), Point2::new(10.0, 10.0));
    assert_eq!(
        unique_repaired_profile_point_line_pair(
            &sketch,
            &SketchLocus::Entity(point.id.clone()),
            &wrong.id,
            &parameter,
            &[
                point.clone(),
                wrong.clone(),
                horizontal.clone(),
                ambiguous.clone(),
            ],
        ),
        None
    );

    let unrelated_point = SketchEntity {
        id: SketchEntityId("unrelated-point".into()),
        geometry: SketchGeometry::Point {
            position: Point2::new(20.0, 25.0),
        },
        ..point.clone()
    };
    let unrelated_line = line(
        "unrelated-line",
        Point2::new(20.0, 20.0),
        Point2::new(30.0, 20.0),
    );
    assert_eq!(
        unique_repaired_profile_point_line_pair(
            &sketch,
            &SketchLocus::Entity(point.id.clone()),
            &wrong.id,
            &parameter,
            &[
                point.clone(),
                wrong.clone(),
                unrelated_point,
                unrelated_line,
            ],
        ),
        None
    );
    assert_eq!(
        unique_profile_point_line_pair(
            &sketch,
            &parameter,
            &[point, horizontal, unrelated, ambiguous],
        ),
        None
    );
}

#[test]
fn axis_relation_fallback_requires_one_aligned_locus_in_the_complete_sketch() {
    let sketch = SketchId("sketch".into());
    let point = |id: &str, u: f64, v: f64| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(u, v),
        },
    };
    let first_entity = point("first-entity", 1.0, 2.0);
    let second_entity = point("second-entity", 4.0, 2.0);
    let unrelated = point("unrelated", 8.0, 9.0);
    let first = marker("first-marker", Some([0.001, 0.002]));
    let second = marker("second-marker", None);
    let collision = marker("collision-marker", Some([8.0, 9.0]));
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    relation.object_index = Some(7);
    relation.links = vec![
        SketchInputLink {
            local_id: 7,
            entity_ref: collision.id.clone(),
        },
        SketchInputLink {
            local_id: 1,
            entity_ref: first.id.clone(),
        },
        SketchInputLink {
            local_id: 2,
            entity_ref: second.id.clone(),
        },
    ];
    let markers = HashMap::from([
        (first.id.as_str(), &first),
        (second.id.as_str(), &second),
        (collision.id.as_str(), &collision),
    ]);
    let loci = HashMap::from([(
        first.id.clone(),
        vec![SketchLocus::Entity(first_entity.id.clone())],
    )]);
    assert_eq!(
        unique_axis_aligned_linked_loci(
            &relation,
            &sketch,
            &[
                first_entity.clone(),
                second_entity.clone(),
                unrelated.clone()
            ],
            &markers,
            &loci,
            true,
        ),
        Some(vec![
            SketchLocus::Entity(first_entity.id.clone()),
            SketchLocus::Entity(second_entity.id.clone()),
        ])
    );

    let ambiguous = point("ambiguous", 6.0, 2.0);
    assert_eq!(
        unique_axis_aligned_linked_loci(
            &relation,
            &sketch,
            &[first_entity, second_entity, unrelated, ambiguous],
            &markers,
            &loci,
            true,
        ),
        None
    );
}

#[test]
fn fixed_relation_ignores_self_identifying_geometry_link() {
    let mut relation = marker("fixed", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Fixed);
    relation.object_index = Some(7);
    relation.links = vec![
        SketchInputLink {
            local_id: 7,
            entity_ref: "collision".into(),
        },
        SketchInputLink {
            local_id: 2,
            entity_ref: "point".into(),
        },
    ];
    let mut collision = marker("collision", Some([3.0, 4.0]));
    collision.kind = SketchInputKind::Point;
    let mut point = marker("point", Some([1.0, 2.0]));
    point.kind = SketchInputKind::Point;
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (collision.id.as_str(), &collision),
        (point.id.as_str(), &point),
    ]);
    let point_id = SketchEntityId("point-entity".into());
    let loci = HashMap::from([(
        point.id.clone(),
        vec![SketchLocus::Entity(point_id.clone())],
    )]);
    let point_entity = SketchEntity {
        id: point_id.clone(),
        sketch: SketchId("sketch".into()),
        construction: false,
        native_ref: Some(point.id.clone()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(1.0, 2.0),
        },
    };

    assert_eq!(
        typed_marker_relation_definition_in_sketch(
            &relation,
            &SketchId("sketch".into()),
            std::slice::from_ref(&point_entity),
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Fixed { entity: point_id })
    );
}

#[test]
fn relation_line_identity_ignores_self_identifying_geometry_link() {
    let sketch = SketchId("sketch".into());
    let line_id = SketchEntityId("line".into());
    let first_id = SketchEntityId("first".into());
    let second_id = SketchEntityId("second".into());
    let line = SketchEntity {
        id: line_id.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        },
    };
    let point_entity = |id: SketchEntityId, position: Point2| SketchEntity {
        id,
        sketch: sketch.clone(),
        construction: true,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let first_entity = point_entity(first_id.clone(), Point2::new(0.0, 0.0));
    let second_entity = point_entity(second_id.clone(), Point2::new(2.0, 0.0));
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Distance);
    relation.object_index = Some(7);
    relation.links = vec![
        SketchInputLink {
            local_id: 7,
            entity_ref: "collision".into(),
        },
        SketchInputLink {
            local_id: 1,
            entity_ref: "first-marker".into(),
        },
        SketchInputLink {
            local_id: 2,
            entity_ref: "second-marker".into(),
        },
    ];
    let collision = marker("collision", Some([8.0, 9.0]));
    let first_marker = marker("first-marker", Some([0.0, 0.0]));
    let second_marker = marker("second-marker", Some([2.0, 0.0]));
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (collision.id.as_str(), &collision),
        (first_marker.id.as_str(), &first_marker),
        (second_marker.id.as_str(), &second_marker),
    ]);
    let loci = HashMap::from([
        (first_marker.id.clone(), vec![SketchLocus::Entity(first_id)]),
        (
            second_marker.id.clone(),
            vec![SketchLocus::Entity(second_id)],
        ),
    ]);

    assert_eq!(
        single_marker_line_entity(
            &relation.id,
            &markers,
            &loci,
            &[line, first_entity, second_entity],
        ),
        Some(line_id)
    );
}

#[test]
fn linked_locus_disambiguates_a_coordinate_collision() {
    let mut ambiguous = marker("ambiguous", None);
    ambiguous.links = vec![SketchInputLink {
        local_id: 2,
        entity_ref: "linked".into(),
    }];
    let linked = marker("linked", None);
    let markers = HashMap::from([
        (ambiguous.id.as_str(), &ambiguous),
        (linked.id.as_str(), &linked),
    ]);
    let expected = SketchLocus::Start(SketchEntityId("line-a".into()));
    let loci = HashMap::from([
        (
            ambiguous.id.clone(),
            vec![
                expected.clone(),
                SketchLocus::End(SketchEntityId("line-b".into())),
            ],
        ),
        (linked.id.clone(), vec![expected.clone()]),
    ]);

    assert_eq!(
        resolved_marker_locus(&ambiguous.id, &markers, &loci, &mut HashSet::new()),
        Some(expected)
    );
    assert_eq!(
        marker_entities(&ambiguous.id, &markers, &loci),
        vec![SketchEntityId("line-a".into())]
    );
}

#[test]
fn point_handle_does_not_inherit_a_constraint_sibling_locus() {
    let mut point = marker("point", None);
    point.links = vec![SketchInputLink {
        local_id: 0,
        entity_ref: "relation".into(),
    }];
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Distance);
    relation.links = vec![
        SketchInputLink {
            local_id: 1,
            entity_ref: point.id.clone(),
        },
        SketchInputLink {
            local_id: 3,
            entity_ref: "known".into(),
        },
    ];
    let known = marker("known", None);
    let markers = HashMap::from([
        (point.id.as_str(), &point),
        (relation.id.as_str(), &relation),
        (known.id.as_str(), &known),
    ]);
    let loci = HashMap::from([(
        known.id.clone(),
        vec![SketchLocus::Start(SketchEntityId("line".into()))],
    )]);

    assert_eq!(
        resolved_marker_locus(&point.id, &markers, &loci, &mut HashSet::new()),
        None
    );
}

#[test]
fn pattern_inputs_bind_adjacent_objects_and_line_reference_direction() {
    let native_feature = |id: &str, source_id: &str, name: &str| NativeFeature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source_id.into()),
        parent_source_id: None,
        ordinal: 0,
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
    let mut seed_native = native_feature("seed-native", "5", "SeedFeature");
    seed_native.input_class = Some("moExtrusion_c".into());
    let mut pattern_native = native_feature("pattern-native", "10", "Pattern1");
    pattern_native.input_class = Some("moCurvePattern_c".into());
    let mut path_native = native_feature("path-native", "20", "PathSketch");
    path_native.input_class = Some("moProfileFeature_c".into());
    let next_native = native_feature("next-native", "30", "NextFeature");
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![seed_native, pattern_native, path_native, next_native],
    };
    let name = |offset: u64, object_id: u32, value: &str| FeatureInputName {
        id: format!("name-{offset}"),
        parent: "lane".into(),
        ordinal: 0,
        offset,
        value: value.into(),
        object_id: Some(object_id),
    };
    let line_ref_offset = 120usize;
    let mut native_payload = vec![0; 400];
    native_payload[line_ref_offset + 136..line_ref_offset + 144]
        .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    native_payload[line_ref_offset + 148..line_ref_offset + 152]
        .copy_from_slice(&[0xf8, 0x2a, 0, 0]);
    for (index, value) in [-1.0f64, 0.0, 0.0].into_iter().enumerate() {
        let offset = line_ref_offset + 200 + index * 8;
        native_payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        line_reference_direction(&native_payload, line_ref_offset as u64),
        Some(Vector3::new(-1.0, 0.0, 0.0))
    );
    assert_eq!(
        compact_line_reference_direction(
            &native_payload,
            0,
            native_payload.len(),
            &[line_ref_offset + 136],
        ),
        None
    );
    let mut three_word_payload = vec![0; 400];
    three_word_payload[line_ref_offset + 144..line_ref_offset + 156].copy_from_slice(&[
        0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff,
    ]);
    three_word_payload[line_ref_offset + 160..line_ref_offset + 164]
        .copy_from_slice(&[0xf8, 0x2a, 0, 0]);
    for (index, value) in [0.0f64, 0.6, 0.8].into_iter().enumerate() {
        let offset = line_ref_offset + 220 + index * 8;
        three_word_payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        line_reference_direction(&three_word_payload, line_ref_offset as u64),
        Some(Vector3::new(0.0, 0.6, 0.8))
    );
    let mut declared_variants = vec![0; 280];
    let addressed_handles = 32;
    declared_variants[addressed_handles..addressed_handles + 8]
        .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    declared_variants[addressed_handles + 12..addressed_handles + 16]
        .copy_from_slice(&9000u32.to_le_bytes());
    for (index, value) in [0.1f64, 0.2, 0.3, 0.4, 0.0, 0.0, -1.0]
        .into_iter()
        .enumerate()
    {
        let offset = addressed_handles + 32 + index * 8;
        declared_variants[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        declared_line_reference_directions(&declared_variants, 0, declared_variants.len()),
        vec![Vector3::new(0.0, 0.0, -1.0)]
    );
    let mut display_payload = vec![0; 512];
    let display_names = [
        FeatureInputName {
            id: "display-d3".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 100,
            object_id: Some(u32::MAX),
            value: "D3".into(),
        },
        FeatureInputName {
            id: "display-d4".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 300,
            object_id: Some(u32::MAX),
            value: "D4".into(),
        },
    ];
    for (offset, spacing, direction) in [
        (100usize, 0.027f64, [0.6f64, 0.8, 0.0]),
        (300usize, 0.039f64, [0.0f64, 0.0, 1.0]),
    ] {
        display_payload[offset + 32..offset + 40].copy_from_slice(&spacing.to_le_bytes());
        for (index, value) in direction.into_iter().enumerate() {
            let scalar = offset + 161 + index * 8;
            display_payload[scalar..scalar + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    assert_eq!(
        linear_pattern_display_directions(
            &display_payload,
            50,
            display_payload.len(),
            &display_names,
            [Some(0.027), Some(0.039)],
        ),
        vec![Vector3::new(0.6, 0.8, 0.0), Vector3::new(0.0, 0.0, 1.0)]
    );
    assert_eq!(
        linear_pattern_display_directions(
            &display_payload,
            50,
            display_payload.len(),
            &display_names,
            [Some(0.028), Some(0.039)],
        ),
        vec![Vector3::new(0.0, 0.0, 1.0)]
    );
    assert_eq!(
        linear_pattern_display_directions(
            &display_payload,
            50,
            484,
            &display_names,
            [Some(0.027), Some(0.039)],
        ),
        vec![Vector3::new(0.6, 0.8, 0.0)]
    );
    let mut compact_longer_form = vec![0; 126];
    compact_longer_form[..8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    compact_longer_form[12..16].copy_from_slice(&9000u32.to_le_bytes());
    for (index, value) in [0.1f64, 0.2, 0.3, 0.4, 0.0, 0.0, 1.0, 0.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = 32 + index * 8;
        compact_longer_form[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_longer_form[104..112].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    compact_longer_form[124..126].copy_from_slice(&0x8001u16.to_le_bytes());
    assert!(
        declared_line_reference_directions(&compact_longer_form, 0, compact_longer_form.len())
            .is_empty()
    );
    assert_eq!(
        compact_line_reference_direction(&compact_longer_form, 0, compact_longer_form.len(), &[]),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );
    let mut compact_payload = vec![0; 400];
    let handles = 64;
    compact_payload[handles..handles + 8]
        .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    compact_payload[handles + 12..handles + 16].copy_from_slice(&5000u32.to_le_bytes());
    for (index, value) in [0.58, -0.0125, 0.023, -0.29, 0.0, 0.0, 0.0, -1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 32 + index * 8;
        compact_payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    compact_payload[handles + 104..handles + 112].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(
        compact_line_reference_direction(&compact_payload, 0, compact_payload.len(), &[]),
        Some(Vector3::new(0.0, -1.0, 0.0))
    );
    let mut six_scalar_payload = vec![0; 160];
    six_scalar_payload[..8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    six_scalar_payload[12..16].copy_from_slice(&8000u32.to_le_bytes());
    for (index, value) in [0.2, 0.27, -0.1, 0.0, 1.0, 0.0].into_iter().enumerate() {
        let offset = 40 + index * 8;
        six_scalar_payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    six_scalar_payload[104..112].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(
        compact_line_reference_direction(&six_scalar_payload, 0, six_scalar_payload.len(), &[],),
        Some(Vector3::new(0.0, 1.0, 0.0))
    );
    let mut token_terminated_payload = vec![0; 130];
    token_terminated_payload[..8]
        .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    token_terminated_payload[12..16].copy_from_slice(&9000u32.to_le_bytes());
    for (index, value) in [1.0_f64, 0.0, 0.25, 0.855, 0.0, 0.0, 1.0, 0.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = 32 + index * 8;
        token_terminated_payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    token_terminated_payload[104..112].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    token_terminated_payload[124..126].copy_from_slice(&0x82c0u16.to_le_bytes());
    assert_eq!(
        compact_line_reference_direction(
            &token_terminated_payload,
            0,
            token_terminated_payload.len(),
            &[],
        ),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );
    token_terminated_payload[124..126].copy_from_slice(&0x02c0u16.to_le_bytes());
    assert_eq!(
        compact_line_reference_direction(
            &token_terminated_payload,
            0,
            token_terminated_payload.len(),
            &[],
        ),
        None
    );
    let mut tagged_trailer_payload = vec![0; 144];
    tagged_trailer_payload[..8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    tagged_trailer_payload[12..16].copy_from_slice(&9100u32.to_le_bytes());
    for (index, value) in [0.07_f64, -0.046, 0.018, 0.012, 0.0, 0.0, 1.0]
        .into_iter()
        .enumerate()
    {
        let offset = 32 + index * 8;
        tagged_trailer_payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    tagged_trailer_payload[124..126].copy_from_slice(&0x8933u16.to_le_bytes());
    tagged_trailer_payload[142..144].copy_from_slice(&[0xff; 2]);
    assert_eq!(
        compact_line_reference_direction(
            &tagged_trailer_payload,
            0,
            tagged_trailer_payload.len(),
            &[],
        ),
        Some(Vector3::new(0.0, 0.0, 1.0))
    );
    tagged_trailer_payload[124..144].fill(0);
    tagged_trailer_payload[104..112].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    tagged_trailer_payload[122..124].copy_from_slice(&0x81b3u16.to_le_bytes());
    tagged_trailer_payload[140..142].copy_from_slice(&[0xff; 2]);
    assert_eq!(
        compact_line_reference_direction(
            &tagged_trailer_payload,
            0,
            tagged_trailer_payload.len(),
            &[],
        ),
        Some(Vector3::new(0.0, 0.0, 1.0))
    );
    tagged_trailer_payload[122..124].copy_from_slice(&0x01b3u16.to_le_bytes());
    assert_eq!(
        compact_line_reference_direction(
            &tagged_trailer_payload,
            0,
            tagged_trailer_payload.len(),
            &[],
        ),
        None
    );
    tagged_trailer_payload[122..144].fill(0);
    tagged_trailer_payload[124..126].copy_from_slice(&0x8204u16.to_le_bytes());
    tagged_trailer_payload[142..144].copy_from_slice(&[0xff; 2]);
    assert_eq!(
        compact_line_reference_direction(
            &tagged_trailer_payload,
            0,
            tagged_trailer_payload.len(),
            &[],
        ),
        Some(Vector3::new(0.0, 0.0, 1.0))
    );
    let short_handles = 200;
    compact_payload[short_handles..short_handles + 8]
        .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    compact_payload[short_handles + 12..short_handles + 16].copy_from_slice(&6000u32.to_le_bytes());
    for (index, value) in [0.056, -0.0415, 0.027, 0.018, 0.0, -1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = short_handles + 24 + index * 8;
        compact_payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    compact_payload[short_handles + 80..short_handles + 88]
        .copy_from_slice(&[0xb8, 0x85, 0xad, 0x80, 0xff, 0xfe, 0xff, 0x07]);
    let eight_scalar_handles = 300;
    compact_payload[eight_scalar_handles..eight_scalar_handles + 8]
        .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    compact_payload[eight_scalar_handles + 12..eight_scalar_handles + 16]
        .copy_from_slice(&7000u32.to_le_bytes());
    for (index, value) in [0.0, 0.988, 0.005, 0.494, 0.2215, 0.0, -1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = eight_scalar_handles + 24 + index * 8;
        compact_payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    compact_payload[eight_scalar_handles + 88..eight_scalar_handles + 96]
        .copy_from_slice(&f64::NAN.to_le_bytes());
    assert_eq!(
        compact_line_reference_direction(&compact_payload, 200, 288, &[]),
        Some(Vector3::new(0.0, -1.0, 0.0))
    );
    assert_eq!(
        compact_line_reference_direction(&compact_payload, 300, 400, &[]),
        Some(Vector3::new(0.0, -1.0, 0.0))
    );
    assert_eq!(
        compact_line_reference_direction(&compact_payload, 0, compact_payload.len(), &[]),
        Some(Vector3::new(0.0, -1.0, 0.0))
    );
    compact_payload[short_handles + 56..short_handles + 80].copy_from_slice(&[
        0, 0, 0, 0, 0, 0, 0xf0, 0x3f, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    assert_eq!(
        compact_line_reference_direction(&compact_payload, 0, compact_payload.len(), &[]),
        None
    );
    compact_payload[short_handles + 12..short_handles + 16].fill(0);
    compact_payload[eight_scalar_handles + 12..eight_scalar_handles + 16].fill(0);
    compact_payload[handles + 12..handles + 16].fill(0);
    assert_eq!(
        compact_line_reference_direction(&compact_payload, 0, compact_payload.len(), &[]),
        None
    );
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload,
        classes: vec![FeatureInputClass {
            id: "line-reference".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: line_ref_offset as u64,
            name: "moLineRef_w".into(),
            role: FeatureInputClassRole::Reference,
        }],
        names: vec![
            name(50, 5, "SeedFeature"),
            name(100, 10, "Pattern1"),
            name(500, 20, "PathSketch"),
            name(600, 30, "NextFeature"),
        ],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let model_feature = |id: &str, native_ref: &str, definition| Feature {
        id: FeatureId(id.into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: Some(native_ref.into()),
    };
    let sketch = SketchId("path-sketch".into());
    let mut features = vec![
        model_feature(
            "pattern",
            "pattern-native",
            FeatureDefinition::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::CurveDriven {
                    path: None,
                    spacing: Length(5.0),
                    count: 3,
                },
            },
        ),
        model_feature(
            "path",
            "path-native",
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        ),
        model_feature(
            "seed",
            "seed-native",
            FeatureDefinition::Native {
                kind: "Extrude".into(),
                parameters: BTreeMap::new(),
                properties: BTreeMap::new(),
            },
        ),
    ];

    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
    );

    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            ref seeds,
            pattern: PatternKind::CurveDriven { path: None, .. },
            ..
        } if seeds == &[PatternSeed::Feature(features[2].id.clone())]
    ));
    assert_eq!(features[0].dependencies, [features[2].id.clone()]);
    features[1].definition = FeatureDefinition::Sketch {
        space: cadmpeg_ir::features::SketchSpace::Planar,
        sketch: Some(sketch.clone()),
    };
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
    );

    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::CurveDriven {
                path: Some(PathRef::Sketch(ref path)),
                ..
            },
            ..
        } if path == &sketch
    ));
    assert_eq!(
        features[0].dependencies,
        [features[2].id.clone(), features[1].id.clone()]
    );
    let FeatureDefinition::Pattern { seeds, .. } = &features[0].definition else {
        panic!("expected pattern");
    };
    assert_eq!(seeds, &[PatternSeed::Feature(features[2].id.clone())]);

    let mut ambiguous_lane = lane.clone();
    ambiguous_lane.names.insert(2, name(450, 20, "PathSketch"));
    if let FeatureDefinition::Pattern {
        pattern: PatternKind::CurveDriven { path, .. },
        seeds,
        ..
    } = &mut features[0].definition
    {
        *path = None;
        seeds.clear();
    }
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&history),
        &[ambiguous_lane],
    );
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::CurveDriven { path: None, .. },
            ..
        }
    ));

    let mut linear_history = history.clone();
    linear_history.features[1].input_class = Some("moLPattern_c".into());
    features[0].dependencies.clear();
    features[0].definition = FeatureDefinition::Pattern {
        seeds: Vec::new(),
        pattern: PatternKind::Linear {
            direction: None,
            spacing: Length(5.0),
            count: 3,
            second: None,
        },
    };
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&linear_history),
        std::slice::from_ref(&lane),
    );
    let FeatureDefinition::Pattern { seeds, .. } = &features[0].definition else {
        panic!("expected pattern");
    };
    assert_eq!(seeds, &[PatternSeed::Feature(features[2].id.clone())]);
    assert_eq!(features[0].dependencies, [features[2].id.clone()]);
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Linear {
                direction: Some(Vector3 { x, y, z }),
                ..
            },
            ..
        } if x == -1.0 && y == 0.0 && z == 0.0
    ));

    let FeatureDefinition::Pattern {
        pattern: PatternKind::Linear { direction, .. },
        ..
    } = &mut features[0].definition
    else {
        panic!("expected linear pattern");
    };
    *direction = None;
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&linear_history),
        std::slice::from_ref(&lane),
    );
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            ref seeds,
            pattern: PatternKind::Linear {
                direction: Some(Vector3 { x, y, z }),
                ..
            },
        } if seeds == &[PatternSeed::Feature(features[2].id.clone())]
            && x == -1.0 && y == 0.0 && z == 0.0
    ));

    let mut derived_history = linear_history.clone();
    derived_history.features[0].input_class = Some("moCosmeticThread_c".into());
    derived_history.features[0].ordinal = 1;
    let mut decoy = native_feature("decoy-native", "6", "Decoy");
    decoy.input_class = Some("moProfileFeature_c".into());
    decoy.ordinal = 2;
    derived_history.features[1].ordinal = 3;
    derived_history.features[2].input_class = Some("moDerivedCosmeticThread_c".into());
    derived_history.features[2].ordinal = 4;
    derived_history.features[3].ordinal = 5;
    derived_history.features.insert(1, decoy);
    let mut derived_lane = lane.clone();
    derived_lane.names = vec![
        name(50, 5, "SeedFeature"),
        name(90, 6, "Decoy"),
        name(100, 10, "Pattern1"),
        name(150, 20, "PathSketch"),
        name(600, 30, "NextFeature"),
    ];
    features[0].dependencies.clear();
    features[0].definition = FeatureDefinition::Pattern {
        seeds: Vec::new(),
        pattern: PatternKind::Linear {
            direction: None,
            spacing: Length(5.0),
            count: 3,
            second: None,
        },
    };
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&derived_history),
        std::slice::from_ref(&derived_lane),
    );
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            ref seeds,
            pattern: PatternKind::Linear {
                direction: Some(Vector3 { x, y, z }),
                ..
            },
        } if seeds == &[PatternSeed::Feature(features[2].id.clone())]
            && x == -1.0 && y == 0.0 && z == 0.0
    ));
    derived_history.features[2].parameters =
        BTreeMap::from([("z".into(), "3".into()), ("e".into(), "19".into())]);
    derived_lane.classes.extend([
        FeatureInputClass {
            id: "count-dimension".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 200,
            name: "moNumberDim_c".into(),
            role: FeatureInputClassRole::Dimension,
        },
        FeatureInputClass {
            id: "spacing-dimension".into(),
            parent: "lane".into(),
            ordinal: 2,
            offset: 300,
            name: "ParallelPlaneDistanceDim_c".into(),
            role: FeatureInputClassRole::Dimension,
        },
    ]);
    derived_lane
        .names
        .extend([name(220, u32::MAX, "z"), name(420, u32::MAX, "e")]);
    features[0].dependencies.clear();
    features[0].definition = FeatureDefinition::Pattern {
        seeds: Vec::new(),
        pattern: PatternKind::Unresolved {
            form: Some(cadmpeg_ir::features::PatternForm::Linear),
        },
    };
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&derived_history),
        std::slice::from_ref(&derived_lane),
    );
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            ref seeds,
            pattern: PatternKind::Linear {
                direction: Some(Vector3 { x, y, z }),
                spacing: Length(19.0),
                count: 3,
                ..
            },
        } if seeds == &[PatternSeed::Feature(features[2].id.clone())]
            && x == -1.0 && y == 0.0 && z == 0.0
    ));

    let mut mirror_history = history.clone();
    mirror_history.features[1].input_class = Some("moMirrorPattern_c".into());
    mirror_history.features[2].input_class = Some("moDerivedCosmeticThread_c".into());
    let mut mirror_lane = lane.clone();
    mirror_lane.names[2].offset = 150;
    mirror_lane.native_payload.resize(700, 0);
    mirror_lane.native_payload.fill(0);
    mirror_lane.classes.clear();
    let frame = 160;
    for (relative, value) in [
        (0, 0.012_f64),
        (8, -0.025),
        (16, 0.0),
        (24, 0.0),
        (32, 1.0),
        (40, 0.0),
        (49, 1.0),
        (57, 0.0),
        (65, 0.0),
        (73, 0.0),
        (81, 0.0),
        (89, -1.0),
    ] {
        mirror_lane.native_payload[frame + relative..frame + relative + 8]
            .copy_from_slice(&value.to_le_bytes());
    }
    mirror_lane.native_payload[frame + 48] = 1;
    let seed_path = 300;
    mirror_lane.native_payload[seed_path - 12..seed_path - 8].copy_from_slice(&3u32.to_le_bytes());
    mirror_lane.native_payload[seed_path..seed_path + COMPACT_EDGE_VECTOR_MARKER.len()]
        .copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    for (index, source) in [5u32, 40].into_iter().enumerate() {
        let entry = seed_path + 18 + index * 20;
        mirror_lane.native_payload[entry..entry + 2]
            .copy_from_slice(&(0x8001 + index as u16).to_le_bytes());
        mirror_lane.native_payload[entry + 4..entry + 8].copy_from_slice(&[1, 0, 1, 0]);
        mirror_lane.native_payload[entry + 8..entry + 12].copy_from_slice(&source.to_le_bytes());
        mirror_lane.native_payload[entry + 12..entry + 16].copy_from_slice(&9000u32.to_le_bytes());
        mirror_lane.native_payload[entry + 16..entry + 20]
            .copy_from_slice(&(index as u32 + 1).to_le_bytes());
    }
    features[0].dependencies.clear();
    features[0].definition = FeatureDefinition::Pattern {
        seeds: Vec::new(),
        pattern: PatternKind::Unresolved {
            form: Some(cadmpeg_ir::features::PatternForm::Mirror),
        },
    };
    bind_pattern_inputs(
        &mut features,
        &[mirror_history],
        std::slice::from_ref(&mirror_lane),
    );
    assert_eq!(features[0].dependencies, [features[2].id.clone()]);
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            ref seeds,
            pattern: PatternKind::Mirror {
                plane_origin: Point3 { x, y, z },
                plane_normal: Vector3 { x: nx, y: ny, z: nz },
            },
        } if seeds == &[PatternSeed::Feature(features[2].id.clone())]
            && x == 12.0 && y == -25.0 && z == 0.0
            && nx == 0.0 && ny == 1.0 && nz == 0.0
    ));

    let mut sweep_history = history;
    sweep_history.features[0].input_class = Some("moProfileFeature_c".into());
    sweep_history.features[1].input_class = Some("moSweep_c".into());
    let path_sketch = SketchId("sweep-path".into());
    features[2].definition = FeatureDefinition::Sketch {
        space: cadmpeg_ir::features::SketchSpace::Planar,
        sketch: Some(path_sketch.clone()),
    };
    features[0].dependencies.clear();
    features[0].definition = FeatureDefinition::Sweep {
        section: cadmpeg_ir::features::SweepSection::Unresolved(None),
        sections: Vec::new(),
        path: Some(PathRef::Native("curve-reference".into())),
        mode: SweepMode::Solid {
            op: cadmpeg_ir::features::BooleanOp::Join,
        },
        orientation: None,
        transition: None,
        transformation: None,
        path_tangent: false,
        linearize: false,
        twist: None,
        path_extent: None,
        guide_rail: None,
        taper: None,
        scale: None,
        allow_multi_profile_faces: None,
    };
    bind_sweep_adjacent_profiles(&mut features, &[sweep_history], std::slice::from_ref(&lane));
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(
                cadmpeg_ir::features::ProfileRef::Sketch(ref profile),
            ),
            path: Some(PathRef::Sketch(ref path)),
            ..
        } if profile == &sketch && path == &path_sketch
    ));
    assert_eq!(
        features[0].dependencies,
        [features[1].id.clone(), features[2].id.clone()]
    );
}

#[test]
fn compact_line_reference_scalar_counts_follow_their_trailers() {
    const HANDLES: [u8; 8] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let write_scalars = |payload: &mut [u8], start: usize, values: &[f64]| {
        for (index, value) in values.iter().enumerate() {
            let offset = start + index * 8;
            payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
    };

    let mut shifted_nine = vec![0; 136];
    shifted_nine[..8].copy_from_slice(&HANDLES);
    shifted_nine[12..16].copy_from_slice(&8000u32.to_le_bytes());
    write_scalars(
        &mut shifted_nine,
        24,
        &[0.13, 0.01, -0.02, 0.05, 0.0, 0.0, -1.0, 0.0, 0.0],
    );
    shifted_nine[96..104].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    shifted_nine[116..118].copy_from_slice(&0x81a5u16.to_le_bytes());
    shifted_nine[134..136].fill(0xff);
    assert_eq!(
        compact_line_reference_direction(&shifted_nine, 0, shifted_nine.len(), &[]),
        Some(Vector3::new(-1.0, 0.0, 0.0))
    );

    let mut shifted_seven = vec![0; 136];
    shifted_seven[..8].copy_from_slice(&HANDLES);
    shifted_seven[12..16].copy_from_slice(&8000u32.to_le_bytes());
    write_scalars(
        &mut shifted_seven,
        24,
        &[0.01, 0.005, 0.022, 0.031, 1.0, 0.0, 0.0],
    );
    shifted_seven[116..118].copy_from_slice(&0x85deu16.to_le_bytes());
    shifted_seven[134..136].fill(0xff);
    assert_eq!(
        compact_line_reference_direction(&shifted_seven, 0, shifted_seven.len(), &[]),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );
    shifted_seven[80..136].fill(0);
    shifted_seven[80..88].copy_from_slice(&[120, 0, 0, 0, 10, 0, 0, 0]);
    assert_eq!(
        compact_line_reference_direction(&shifted_seven, 0, shifted_seven.len(), &[]),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );

    let mut unshifted_seven = vec![0; 96];
    unshifted_seven[..8].copy_from_slice(&HANDLES);
    unshifted_seven[12..16].copy_from_slice(&9000u32.to_le_bytes());
    write_scalars(
        &mut unshifted_seven,
        32,
        &[0.06, 0.03, 0.076, -0.03, 0.0, 0.0, 1.0],
    );
    unshifted_seven[88..96].fill(1);
    assert_eq!(
        compact_line_reference_direction(&unshifted_seven, 0, unshifted_seven.len(), &[]),
        Some(Vector3::new(0.0, 0.0, 1.0))
    );

    let mut addressless = vec![0; 84];
    addressless[..8].copy_from_slice(&HANDLES);
    write_scalars(
        &mut addressless,
        24,
        &[0.04, 0.01, 0.0, 0.0, 0.0, 0.0, -1.0],
    );
    assert_eq!(
        compact_line_reference_direction(&addressless, 0, addressless.len(), &[]),
        Some(Vector3::new(0.0, 0.0, -1.0))
    );
    addressless.truncate(80);
    addressless.extend([0, 0, 0, 0, 0xd8, 0x81]);
    assert_eq!(
        compact_line_reference_direction(&addressless, 0, addressless.len(), &[]),
        Some(Vector3::new(0.0, 0.0, -1.0))
    );

    let mut addressless_unshifted = vec![0; 136];
    addressless_unshifted[..8].copy_from_slice(&HANDLES);
    write_scalars(
        &mut addressless_unshifted,
        32,
        &[0.065, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    );
    addressless_unshifted[104..112].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(
        compact_line_reference_direction(
            &addressless_unshifted,
            0,
            addressless_unshifted.len(),
            &[],
        ),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );
}

#[test]
fn e1_line_distance_indices_address_coordinate_point_pairs() {
    let sketch = SketchId("sketch".into());
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let coordinates = [
        [0.002, -0.007],
        [0.018, -0.007],
        [0.002, -0.002],
        [0.002, -0.018],
        [0.018, -0.018],
        [0.018, -0.002],
        [0.002, -0.013],
        [0.018, -0.013],
        [0.002, -0.018],
        [0.018, -0.018],
        [0.018, -0.002],
        [0.002, -0.002],
    ];
    let markers = coordinates
        .into_iter()
        .enumerate()
        .map(|(index, coordinates)| {
            let mut point = marker(&format!("point-{index}"), Some(coordinates));
            point.offset = index as u64;
            point
        })
        .collect::<Vec<_>>();
    let mut entities = markers
        .iter()
        .take(3)
        .map(|marker| {
            let [u, v] = marker.coordinates_m.unwrap();
            SketchEntity {
                id: SketchEntityId(format!("bound-{}", marker.id)),
                sketch: sketch.clone(),
                construction: false,
                native_ref: Some(marker.id.clone()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: Point2::new(u * 1000.0, v * 1000.0),
                },
            }
        })
        .collect::<Vec<_>>();
    let operand = |offset: u64, index: u16| FeatureInputOperand {
        offset,
        reference_ref: format!("reference-{offset}"),
        kind: FeatureInputOperandKind::E1,
        entity_index: index,
        entity_ref: None,
    };
    let relation = |id: &str, offset: u64, first: u16, second: u16, scalar: &str| {
        FeatureInputRelationInstance {
            id: id.into(),
            parent: "lane".into(),
            ordinal: offset as u32,
            offset,
            family: FeatureInputRelationFamily::LineLineDistance,
            class_ref: "class".into(),
            feature_ref: "feature-native".into(),
            scalar_refs: vec![scalar.into()],
            parameter_scalar_ref: Some(scalar.into()),
            display_scalar_ref: None,
            operands: vec![operand(offset + 1, first), operand(offset + 2, second)],
        }
    };
    let lane = FeatureInputLane {
        id: "lane#test".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![
            relation("lower-distance", 100, 4, 3, "lower-scalar"),
            relation("upper-distance", 200, 5, 0, "upper-scalar"),
        ],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: markers,
    };
    let parameter = |id: &str, scalar: &str| DesignParameter {
        id: ParameterId(id.into()),
        owner: Some(feature.id.clone()),
        ordinal: 0,
        name: id.into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some(scalar.into()),
    };
    let parameters = vec![
        parameter("lower", "lower-scalar"),
        parameter("upper", "upper-scalar"),
    ];

    project_relation_solved_line_geometry(
        &mut entities,
        &[],
        std::slice::from_ref(&feature),
        &parameters,
        std::slice::from_ref(&lane),
    );

    let solver_lines = entities
        .iter()
        .filter(|entity| entity.id.0.contains("#solver-line:"))
        .collect::<Vec<_>>();
    assert_eq!(solver_lines.len(), 4);
    assert_eq!(
        solver_lines
            .iter()
            .filter_map(|entity| entity.geometry_ref.as_deref())
            .collect::<HashSet<_>>(),
        [
            "feature-native:solver-line:0",
            "feature-native:solver-line:3",
            "feature-native:solver-line:4",
            "feature-native:solver-line:5",
        ]
        .into_iter()
        .collect()
    );
    let mut constraints = Vec::new();
    project_relation_bindings(
        &mut constraints,
        &[],
        std::slice::from_ref(&feature),
        &entities,
        &parameters,
        std::slice::from_ref(&lane),
    );
    assert_eq!(constraints.len(), 2);
    assert!(constraints.iter().all(|constraint| matches!(
        &constraint.definition,
        SketchConstraintDefinition::Distance { entities, .. } if entities.len() == 2
    )));
}

#[test]
fn reused_point_handle_gets_one_solved_locus_per_dimension_relation() {
    let sketch = SketchId("sketch".into());
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let point = |id: &str, marker: Option<&str>, u: f64| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: marker.map(str::to_owned),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(u, 0.0),
        },
    };
    let mut entities = vec![
        point("origin", Some("known-a"), 0.0),
        point("middle", Some("known-b"), 5.0),
        point("far", None, 12.0),
    ];
    let known_a = marker("known-a", Some([0.0, 0.0]));
    let known_b = marker("known-b", Some([0.005, 0.0]));
    let missing = marker("missing", None);
    let operand = |index: usize, marker: &str| FeatureInputOperand {
        offset: index as u64,
        reference_ref: format!("reference-{index}"),
        kind: FeatureInputOperandKind::D6,
        entity_index: index as u16,
        entity_ref: Some(marker.into()),
    };
    let relation =
        |id: &str, offset: u64, family: FeatureInputRelationFamily, known: &str, scalar: &str| {
            FeatureInputRelationInstance {
                id: id.into(),
                parent: "lane".into(),
                ordinal: 0,
                offset,
                family,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalar_refs: vec![scalar.into()],
                parameter_scalar_ref: Some(scalar.into()),
                display_scalar_ref: None,
                operands: vec![operand(0, known), operand(1, "missing")],
            }
        };
    let relations = vec![
        relation(
            "relation-a",
            10,
            FeatureInputRelationFamily::PointPointDistance,
            "known-a",
            "scalar-a",
        ),
        relation(
            "relation-b",
            20,
            FeatureInputRelationFamily::PointPointDistance,
            "known-b",
            "scalar-b",
        ),
        relation(
            "relation-c",
            30,
            FeatureInputRelationFamily::PointPointHorizontalDistance,
            "known-b",
            "scalar-c",
        ),
    ];
    let lane = FeatureInputLane {
        id: "lane#test".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: relations.clone(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![known_a, known_b, missing],
    };
    let parameter = |id: &str, scalar: &str, distance: f64| DesignParameter {
        id: ParameterId(id.into()),
        owner: Some(feature.id.clone()),
        ordinal: 0,
        name: id.into(),
        expression: format!("{distance}mm"),
        display: None,
        value: Some(ParameterValue::Length(Length(distance))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some(scalar.into()),
    };
    let parameters = vec![
        parameter("distance-a", "scalar-a", 5.0),
        parameter("distance-b", "scalar-b", 7.0),
        parameter("distance-c", "scalar-c", 7.0),
    ];

    project_relation_point_geometry(
        &mut entities,
        &[],
        std::slice::from_ref(&feature),
        std::slice::from_ref(&lane),
    );
    project_relation_solved_point_geometry(
        &mut entities,
        &[],
        std::slice::from_ref(&feature),
        &parameters,
        std::slice::from_ref(&lane),
    );

    let solved = entities
        .iter()
        .filter(|entity| entity.id.0.contains("dimension-point:"))
        .collect::<Vec<_>>();
    assert_eq!(solved.len(), 3);
    assert!(matches!(
        solved[0].geometry,
        SketchGeometry::Point { position } if position == Point2::new(5.0, 0.0)
    ));
    assert!(matches!(
        solved[1].geometry,
        SketchGeometry::Point { position } if position == Point2::new(12.0, 0.0)
    ));
    assert!(matches!(
        solved[2].geometry,
        SketchGeometry::Point { position } if position == Point2::new(12.0, 0.0)
    ));
    assert_ne!(solved[0].geometry_ref, solved[1].geometry_ref);
    assert_ne!(solved[1].geometry_ref, solved[2].geometry_ref);

    let markers = lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let loci = profile_loci_by_marker(
        std::slice::from_ref(&feature),
        &[],
        &entities,
        std::slice::from_ref(&lane),
    );
    for (index, relation) in relations.iter().enumerate() {
        let definition = typed_relation_definition(
            relation,
            Some(&parameters[index]),
            &sketch,
            &entities,
            &markers,
            &loci,
        );
        let second = match definition {
            Some(
                SketchConstraintDefinition::DistanceLoci { second, .. }
                | SketchConstraintDefinition::HorizontalDistance { second, .. },
            ) => second,
            other => panic!("unexpected relation definition: {other:?}"),
        };
        assert_eq!(second, SketchLocus::Entity(solved[index].id.clone()));
    }
}

#[test]
fn circle_dimension_driver_supplies_the_center_operand() {
    let operand = |index, marker: &str| FeatureInputOperand {
        offset: u64::from(index),
        reference_ref: format!("reference-{index}"),
        kind: FeatureInputOperandKind::Native(0x929d),
        entity_index: index,
        entity_ref: Some(marker.into()),
    };
    let scalar = |id: &str, offset, operands| FeatureInputScalar {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_id: 1,
        name: "dimension-name".into(),
        value: 1.0,
        role: FeatureInputScalarRole::Native,
        entity_indices: Vec::new(),
        operands,
    };
    let display_operand = operand(2, "display-handle");
    let display = FeatureInputScalar {
        role: FeatureInputScalarRole::Display,
        ..scalar("display", 10, vec![display_operand.clone()])
    };
    let driver = scalar(
        "driver",
        20,
        vec![display_operand.clone(), operand(1, "center")],
    );
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "dimension-name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            value: "D1".into(),
            object_id: None,
        }],
        scalars: vec![display, driver],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let mut relations = vec![FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 10,
        family: FeatureInputRelationFamily::CircleDiameter,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalar_refs: vec!["display".into()],
        parameter_scalar_ref: None,
        display_scalar_ref: Some("display".into()),
        operands: vec![display_operand],
    }];

    bind_circle_dimension_centers(&mut relations, &lane);

    assert_eq!(relations[0].scalar_refs, ["display", "driver"]);
    assert_eq!(relations[0].operands.len(), 2);
    assert_eq!(
        relations[0].operands[1].entity_ref.as_deref(),
        Some("center")
    );
}

#[test]
fn point_distance_preserves_stored_operands_when_geometry_is_inconsistent() {
    let sketch = SketchId("sketch".into());
    let point = |id: &str, u: f64| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(u, 0.0),
        },
    };
    let entities = vec![
        point("hint-a", 0.0),
        point("hint-b", 2.0),
        point("solved", 5.0),
    ];
    let hint_a = marker("hint-a", Some([0.0, 0.0]));
    let hint_b = marker("hint-b", Some([0.002, 0.0]));
    let markers = HashMap::from([(hint_a.id.as_str(), &hint_a), (hint_b.id.as_str(), &hint_b)]);
    let mut loci = HashMap::from([
        (
            hint_a.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId("hint-a".into()))],
        ),
        (
            hint_b.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId("hint-b".into()))],
        ),
    ]);
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalar_refs: vec!["scalar".into()],
        parameter_scalar_ref: Some("scalar".into()),
        display_scalar_ref: None,
        operands: vec![
            FeatureInputOperand {
                offset: 1,
                reference_ref: "reference-a".into(),
                kind: FeatureInputOperandKind::D6,
                entity_index: 0,
                entity_ref: Some(hint_a.id.clone()),
            },
            FeatureInputOperand {
                offset: 2,
                reference_ref: "reference-b".into(),
                kind: FeatureInputOperandKind::D6,
                entity_index: 1,
                entity_ref: Some(hint_b.id.clone()),
            },
        ],
    };
    let parameter = DesignParameter {
        id: ParameterId("distance".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("scalar".into()),
    };

    assert!(matches!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Entity(first),
            second: SketchLocus::Entity(second),
            ..
        }) if [&first, &second].contains(&&SketchEntityId("hint-a".into()))
            && [&first, &second].contains(&&SketchEntityId("hint-b".into()))
    ));

    let mut horizontal_relation = relation.clone();
    horizontal_relation.family = FeatureInputRelationFamily::PointPointHorizontalDistance;
    assert!(matches!(
        typed_relation_definition(
            &horizontal_relation,
            Some(&parameter),
            &sketch,
            &entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::HorizontalDistance {
            first: SketchLocus::Entity(first),
            second: SketchLocus::Entity(second),
            ..
        }) if [&first, &second].contains(&&SketchEntityId("hint-a".into()))
            && [&first, &second].contains(&&SketchEntityId("hint-b".into()))
    ));

    let mut directional_entities = vec![point("hint-a", 0.0), point("hint-b", 1.0)];
    let mut projected_relation = relation.clone();
    for operand in &mut projected_relation.operands {
        operand.kind = FeatureInputOperandKind::Native(0xbc7c);
    }
    loci.insert(
        super::qualified_point_marker_key(&hint_a.id),
        vec![SketchLocus::Entity(SketchEntityId("hint-a".into()))],
    );
    loci.insert(
        super::qualified_point_marker_key(&hint_b.id),
        vec![SketchLocus::Entity(SketchEntityId("hint-b".into()))],
    );
    directional_entities[1].geometry = SketchGeometry::Point {
        position: Point2::new(1.0, 0.05),
    };
    let mut directional_parameter = parameter.clone();
    directional_parameter.value = Some(ParameterValue::Length(Length(1.0)));
    assert!(matches!(
        typed_relation_definition(
            &projected_relation,
            Some(&directional_parameter),
            &sketch,
            &directional_entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::HorizontalDistance { .. })
    ));
    directional_parameter.value = Some(ParameterValue::Length(Length(0.05)));
    assert!(matches!(
        typed_relation_definition(
            &projected_relation,
            Some(&directional_parameter),
            &sketch,
            &directional_entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::VerticalDistance { .. })
    ));
    directional_entities[1].geometry = SketchGeometry::Point {
        position: Point2::new(1.0, 1.0),
    };
    directional_parameter.value = Some(ParameterValue::Length(Length(1.0)));
    assert!(matches!(
        typed_relation_definition(
            &projected_relation,
            Some(&directional_parameter),
            &sketch,
            &directional_entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::DistanceLoci { .. })
    ));

    let mut ambiguous_entities = entities;
    ambiguous_entities.push(point("other-solved", -5.0));
    for candidate in [&relation, &horizontal_relation] {
        assert!(typed_relation_definition(
            candidate,
            Some(&parameter),
            &sketch,
            &ambiguous_entities,
            &markers,
            &loci,
        )
        .is_some());
    }

    let unrelated_entities = vec![
        point("hint-a", 0.0),
        point("hint-b", 2.0),
        point("unrelated-a", 10.0),
        point("unrelated-b", 15.0),
    ];
    for candidate in [&relation, &horizontal_relation] {
        assert!(typed_relation_definition(
            candidate,
            Some(&parameter),
            &sketch,
            &unrelated_entities,
            &markers,
            &loci,
        )
        .is_some());
    }
}

#[test]
fn display_scalar_name_resolves_one_unclaimed_owner_parameter() {
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: None,
        },
        native_ref: Some("native-feature".into()),
    };
    let parameter = DesignParameter {
        id: ParameterId("parameter".into()),
        owner: Some(feature.id.clone()),
        name: "D1".into(),
        ordinal: 0,
        expression: "12".into(),
        value: Some(ParameterValue::Length(Length(12.0))),
        display: None,
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("existing-driver".into()),
        dependencies: Vec::new(),
    };
    let scalar = FeatureInputScalar {
        id: "scalar".into(),
        parent: "lane".into(),
        feature_ref: Some("native-feature".into()),
        ordinal: 0,
        offset: 10,
        object_id: 1,
        name: "name".into(),
        value: 0.012,
        role: FeatureInputScalarRole::Display,
        entity_indices: Vec::new(),
        operands: Vec::new(),
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            value: "D1".into(),
            object_id: None,
        }],
        scalars: vec![scalar.clone()],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 10,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class".into(),
        feature_ref: "native-feature".into(),
        scalar_refs: vec!["scalar".into()],
        parameter_scalar_ref: None,
        display_scalar_ref: Some("scalar".into()),
        operands: Vec::new(),
    };
    assert_eq!(
        relation_parameter_by_display_name(
            &relation,
            &lane,
            std::slice::from_ref(&feature),
            std::slice::from_ref(&parameter),
        )
        .map(|parameter| &parameter.id),
        Some(&parameter.id)
    );
    assert_eq!(
        owned_relation_parameters(
            std::slice::from_ref(&feature),
            std::slice::from_ref(&parameter),
            std::slice::from_ref(&FeatureInputLane {
                relation_instances: vec![relation.clone()],
                ..lane.clone()
            }),
        )["relation"]
            .as_ref(),
        Some(&parameter.id)
    );
    let mut exact_parameter = parameter.clone();
    exact_parameter.native_ref = Some("scalar".into());
    let mut exact_lane = lane.clone();
    exact_lane.scalars[0].role = FeatureInputScalarRole::Native;
    exact_lane.relation_instances = vec![FeatureInputRelationInstance {
        display_scalar_ref: None,
        ..relation.clone()
    }];
    assert_eq!(
        owned_relation_parameters(
            std::slice::from_ref(&feature),
            std::slice::from_ref(&exact_parameter),
            std::slice::from_ref(&exact_lane),
        )["relation"]
            .as_ref(),
        Some(&exact_parameter.id)
    );
    let driving_relation = FeatureInputRelationInstance {
        id: "driving-relation".into(),
        parameter_scalar_ref: Some("existing-driver".into()),
        display_scalar_ref: None,
        scalar_refs: vec!["existing-driver".into()],
        ..relation.clone()
    };
    let ownership = owned_relation_parameters(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&FeatureInputLane {
            relation_instances: vec![relation.clone(), driving_relation],
            ..lane.clone()
        }),
    );
    assert_eq!(ownership.len(), 1);
    assert_eq!(ownership["driving-relation"].as_ref(), Some(&parameter.id));

    let mut driving_scalar = scalar.clone();
    driving_scalar.id = "driving-by-name".into();
    driving_scalar.ordinal = 1;
    driving_scalar.offset = 20;
    driving_scalar.object_id = 2;
    driving_scalar.name = "driving-name".into();
    driving_scalar.role = FeatureInputScalarRole::Driving;
    let driving_relation = FeatureInputRelationInstance {
        id: "driving-by-name-relation".into(),
        parameter_scalar_ref: Some(driving_scalar.id.clone()),
        display_scalar_ref: None,
        scalar_refs: vec![driving_scalar.id.clone()],
        ..relation.clone()
    };
    let driving_parameter = DesignParameter {
        id: ParameterId("driving-by-name-parameter".into()),
        name: "D".into(),
        native_ref: None,
        ..parameter.clone()
    };
    let mut driving_name = lane.names[0].clone();
    driving_name.id = driving_scalar.name.clone();
    driving_name.value = driving_parameter.name.clone();
    let ownership = owned_relation_parameters(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&driving_parameter),
        std::slice::from_ref(&FeatureInputLane {
            names: vec![lane.names[0].clone(), driving_name],
            scalars: vec![scalar.clone(), driving_scalar],
            relation_instances: vec![driving_relation],
            ..lane.clone()
        }),
    );
    assert_eq!(
        ownership["driving-by-name-relation"].as_ref(),
        Some(&driving_parameter.id)
    );

    let mut detached = scalar;
    detached.id = "driver".into();
    detached.role = FeatureInputScalarRole::Driving;
    detached.operands.clear();
    let mut detached_lane = lane.clone();
    detached_lane.scalars.push(detached);
    let mut detached_relation = vec![relation.clone()];
    bind_detached_relation_drivers(&mut detached_relation, &detached_lane);
    assert_eq!(
        detached_relation[0].parameter_scalar_ref.as_deref(),
        Some("driver")
    );
    assert_eq!(detached_relation[0].scalar_refs, ["scalar", "driver"]);

    let mut parameter = parameter;
    parameter.value = Some(ParameterValue::Integer(12));
    type_display_relation_parameters(
        std::slice::from_mut(&mut parameter),
        std::slice::from_ref(&feature),
        std::slice::from_ref(&FeatureInputLane {
            relation_instances: vec![FeatureInputRelationInstance {
                family: FeatureInputRelationFamily::CircleDiameter,
                ..relation.clone()
            }],
            ..lane.clone()
        }),
    );
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(12.0))));
    assert_eq!(parameter.expression, "<MOD-DIAM>12mm");
    assert_eq!(parameter.display, Some(DimensionDisplay::Diameter));

    parameter.value = Some(ParameterValue::Real(0.012));
    parameter.expression = "0.012".into();
    parameter.display = None;
    parameter.native_ref = Some("driver".into());
    type_display_relation_parameters(
        std::slice::from_mut(&mut parameter),
        std::slice::from_ref(&feature),
        std::slice::from_ref(&FeatureInputLane {
            relation_instances: vec![
                FeatureInputRelationInstance {
                    family: FeatureInputRelationFamily::PointPointDistance,
                    parameter_scalar_ref: Some("driver".into()),
                    ..relation.clone()
                },
                FeatureInputRelationInstance {
                    id: "other-relation".into(),
                    family: FeatureInputRelationFamily::Angle,
                    parameter_scalar_ref: Some("other-driver".into()),
                    ..relation
                },
            ],
            ..lane
        }),
    );
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(12.0))));
    assert_eq!(parameter.expression, "12mm");
}

#[test]
fn axis_aligned_sketch_frame_projects_native_plane_coordinates() {
    let sketch = Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(28.65, -35.0, 0.35),
            normal: Vector3::new(0.0, -1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, -1.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let transform = sketch_frame_marker_transform(&sketch, 1.0e-8).expect("axis frame");
    assert_eq!(
        transform.apply((2_865_000_000, -2_385_000_000)),
        Some((2_420_000_000, 0))
    );
    let other = MarkerTransform {
        u_sign: 1,
        ..transform
    };
    assert_eq!(
        select_marker_transforms_by_frame(&[other, transform], &sketch, 1.0e-8),
        vec![transform]
    );
    let translated = MarkerTransform {
        translation: (17, 23),
        ..transform
    };
    assert_eq!(
        select_marker_transforms_by_frame(&[other, translated], &sketch, 1.0e-8),
        vec![translated]
    );
    assert_eq!(
        select_marker_transforms_by_frame(&[other], &sketch, 1.0e-8),
        vec![other]
    );
    assert_eq!(
        select_marker_transforms_by_frame(&[], &sketch, 1.0e-8),
        vec![transform]
    );
}

#[test]
fn rotated_sketch_frame_projects_native_plane_coordinates() {
    let diagonal = std::f64::consts::FRAC_1_SQRT_2;
    let sketch = Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(10.0, 3.0, 20.0),
            normal: Vector3::new(0.0, -1.0, 0.0),
            u_axis: Vector3::new(diagonal, 0.0, -diagonal),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let transform = sketch_frame_marker_transform(&sketch, 1.0e-8).expect("rotated frame");

    assert!(transform.affine_matrix.is_some());
    assert_eq!(
        transform.apply((1_100_000_000, 1_900_000_000)),
        Some(((std::f64::consts::SQRT_2 / 1.0e-8).round() as i64, 0))
    );
}

#[test]
fn dimensioned_circle_materializes_from_an_alternate_handle_frame() {
    let sketch = SketchId("sketch".into());
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let mut entities = vec![
        SketchEntity {
            id: SketchEntityId("horizontal".into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(10.0, 20.0),
                end: Point2::new(30.0, 20.0),
            },
        },
        SketchEntity {
            id: SketchEntityId("vertical".into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(30.0, 20.0),
                end: Point2::new(30.0, 50.0),
            },
        },
    ];
    let mut horizontal = marker("horizontal-marker", Some([0.020, 0.020]));
    horizontal.kind = SketchInputKind::LineOrCircle;
    horizontal.offset = 0;
    let mut vertical = marker("vertical-marker", Some([0.035, 0.030]));
    vertical.kind = SketchInputKind::LineOrCircle;
    vertical.offset = 32;
    let mut center = marker("circle-center", Some([0.040, 0.015]));
    center.kind = SketchInputKind::LineOrCircle;
    center.offset = 64;
    let mut native_payload = vec![0; 96];
    for offset in [0, 32, 64] {
        native_payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    }
    let relation = FeatureInputRelationInstance {
        id: "circle-relation".into(),
        parent: "lane".into(),
        feature_ref: "feature-native".into(),
        ordinal: 0,
        offset: 80,
        family: FeatureInputRelationFamily::CircleDiameter,
        class_ref: "circle-class".into(),
        parameter_scalar_ref: Some("circle-scalar".into()),
        display_scalar_ref: None,
        operands: vec![FeatureInputOperand {
            offset: 81,
            reference_ref: "circle-reference".into(),
            kind: FeatureInputOperandKind::Native(0x8ab6),
            entity_index: 0,
            entity_ref: Some("circle-center".into()),
        }],
        scalar_refs: Vec::new(),
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload,
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![relation],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![horizontal, vertical, center],
    };
    let parameter = DesignParameter {
        id: ParameterId("diameter".into()),
        owner: Some(FeatureId("feature".into())),
        name: "D1".into(),
        ordinal: 0,
        expression: String::new(),
        value: Some(ParameterValue::Length(Length(8.0))),
        display: Some(DimensionDisplay::Diameter),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("circle-scalar".into()),
        dependencies: Vec::new(),
    };

    project_dimensioned_sketch_geometry(
        &mut entities,
        &[],
        &[],
        &[feature],
        &[parameter],
        std::slice::from_ref(&lane),
    );
    assert!(matches!(
        &entities[2].geometry,
        SketchGeometry::Circle { center, radius }
            if *center == Point2::new(15.0, 40.0) && *radius == Length(4.0)
    ));
    assert!(!entities[2].construction);

    let mut implicit_lane = lane;
    let mut implicit_center = marker("implicit-center", Some([0.010, 0.020]));
    implicit_center.local_id = Some(1);
    implicit_center.offset = 100;
    let mut implicit_radial = marker("implicit-radial", Some([0.013, 0.024]));
    implicit_radial.local_id = Some(2);
    implicit_radial.offset = 200;
    implicit_lane.sketch_entities = vec![implicit_center, implicit_radial];
    let (resolved, radius) = implicit_circle_marker(
        std::slice::from_ref(&implicit_lane),
        "feature-native",
        FeatureInputOperandKind::Native(0x83fe),
        0,
        5.0,
    )
    .expect("implicit circle pair");
    assert_eq!(resolved.id, "implicit-center");
    assert!((radius - 5.0).abs() < 1.0e-12);
}

#[test]
fn implicit_circle_uses_its_solver_relation_in_a_mixed_point_roster() {
    let mut unrelated = marker("unrelated", Some([0.0, 0.0]));
    unrelated.offset = 10;
    unrelated.object_index = Some(7);
    let mut center = marker("center", Some([0.010, 0.020]));
    center.offset = 20;
    center.object_index = Some(9);
    center.local_id = Some(11);
    let mut radial = marker("radial", Some([0.013, 0.024]));
    radial.offset = 30;
    radial.object_index = Some(8);
    radial.local_id = Some(12);
    let mut relation = marker("circle-owner", None);
    relation.offset = 40;
    relation.object_index = Some(1);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Distance);
    relation.links = vec![
        SketchInputLink {
            local_id: 11,
            entity_ref: center.id.clone(),
        },
        SketchInputLink {
            local_id: 11,
            entity_ref: center.id.clone(),
        },
    ];
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
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![unrelated, center, radial, relation],
    };

    let lanes = [lane];
    let (resolved, radius) = implicit_circle_marker(
        &lanes,
        "feature-native",
        FeatureInputOperandKind::Native(0x83fe),
        0,
        5.0,
    )
    .expect("solver-owned implicit circle");

    assert_eq!(resolved.id, "center");
    assert!((radius - 5.0).abs() < 1.0e-12);
}

#[test]
fn implicit_circle_uses_unique_terminal_radial_point() {
    let mut unrelated = marker("unrelated", Some([0.0, 0.0]));
    unrelated.offset = 10;
    unrelated.local_id = Some(1);
    let mut center = marker("center", Some([0.010, 0.010]));
    center.offset = 20;
    center.local_id = Some(2);
    let mut another = marker("another", Some([0.020, 0.020]));
    another.offset = 30;
    another.local_id = Some(3);
    let mut radial = marker("radial", Some([0.013, 0.014]));
    radial.offset = 40;
    radial.local_id = None;
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
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![unrelated, center, another, radial],
    };
    let lanes = [lane];

    let (resolved, radius) = implicit_circle_marker(
        &lanes,
        "feature-native",
        FeatureInputOperandKind::Native(0x83fe),
        0,
        5.0,
    )
    .expect("unique terminal radial pair");

    assert_eq!(resolved.id, "center");
    assert!((radius - 5.0).abs() < 1.0e-12);
}

#[test]
fn unique_translation_joins_linked_endpoints_to_one_profile_entity() {
    let sketch = SketchId("sketch".into());
    let first = SketchEntityId("first".into());
    let second = SketchEntityId("second".into());
    let entities = vec![
        SketchEntity {
            id: first.clone(),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(10.0, 20.0),
                end: Point2::new(20.0, 20.0),
            },
        },
        SketchEntity {
            id: second.clone(),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(20.0, 20.0),
                end: Point2::new(20.0, 30.0),
            },
        },
    ];
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch),
        },
        native_ref: Some("feature-native".into()),
    };
    let mut reference = marker("reference", None);
    reference.links = vec![
        SketchInputLink {
            local_id: 1,
            entity_ref: "marker-a".into(),
        },
        SketchInputLink {
            local_id: 2,
            entity_ref: "marker-b".into(),
        },
    ];
    reference.kind = SketchInputKind::Relation(SketchRelationKind::Vertical);
    reference.link_selector = Some(0);
    let mut native_payload = vec![0; 108];
    for offset in [0, 27, 54] {
        native_payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    }
    native_payload[81 + 23..81 + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    let mut marker_a = marker("marker-a", Some([0.0, 0.0]));
    marker_a.offset = 0;
    let mut marker_b = marker("marker-b", Some([0.01, 0.0]));
    marker_b.offset = 27;
    let mut marker_c = marker("marker-c", Some([0.01, 0.01]));
    marker_c.offset = 54;
    let mut display = marker("display", Some([0.1, 0.1]));
    display.offset = 81;
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
        sketch_entities: vec![marker_a, marker_b, marker_c, display, reference.clone()],
    };

    let joins = profile_loci_by_marker(&[feature], &[], &entities, std::slice::from_ref(&lane));
    assert!(joins.contains_key("marker-a"));
    assert!(joins.contains_key("marker-b"));
    assert!(joins.contains_key("marker-c"));
    assert_eq!(joins["marker-b"].len(), 2);
    assert!(!joins.contains_key("display"));
    let mut markers = lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        marker_entities("reference", &markers, &joins),
        vec![first.clone()]
    );
    let mut wrapper = marker("wrapper", None);
    wrapper.links = vec![SketchInputLink {
        local_id: 1,
        entity_ref: "marker-a".into(),
    }];
    let mut nested_reference = reference.clone();
    nested_reference.id = "nested-reference".into();
    nested_reference.links[0].entity_ref = wrapper.id.clone();
    markers.insert(wrapper.id.as_str(), &wrapper);
    markers.insert(nested_reference.id.as_str(), &nested_reference);
    assert_eq!(
        marker_entities("nested-reference", &markers, &joins),
        vec![first.clone()]
    );
    let mut cycle = marker("cycle", None);
    cycle.links = vec![SketchInputLink {
        local_id: 1,
        entity_ref: cycle.id.clone(),
    }];
    markers.insert(cycle.id.as_str(), &cycle);
    assert!(marker_entities("cycle", &markers, &joins).is_empty());
    assert_eq!(
        typed_marker_relation_definition(markers["reference"], &markers, &joins,),
        Some(SketchConstraintDefinition::Vertical {
            entity: first.clone(),
        })
    );
    let mut nested_horizontal = nested_reference.clone();
    nested_horizontal.kind = SketchInputKind::Relation(SketchRelationKind::HorizontalPoints);
    assert!(matches!(
        typed_marker_relation_definition(&nested_horizontal, &markers, &joins),
        Some(SketchConstraintDefinition::Native { ref native_kind, .. })
            if native_kind == "sldprt:marker-relation:25"
    ));
    let mut nested_native = nested_reference.clone();
    nested_native.kind = SketchInputKind::Native(28);
    assert_eq!(
        typed_marker_relation_definition(&nested_native, &markers, &joins),
        Some(SketchConstraintDefinition::Native {
            native_kind: "sldprt:marker-relation:28".into(),
            native_state: None,
            native_flags: None,
            native_properties: std::collections::BTreeMap::new(),
            entities: vec![first.clone(), second.clone()],
            parameter: None,
            operands: vec![
                SketchNativeOperand {
                    native_kind: "sldprt:marker-local-id".into(),
                    native_field: None,
                    native_role: None,
                    object_index: 1,
                    native_ref: Some("wrapper".into()),
                },
                SketchNativeOperand {
                    native_kind: "sldprt:marker-local-id".into(),
                    native_field: None,
                    native_role: None,
                    object_index: 2,
                    native_ref: Some("marker-b".into()),
                },
            ],
        })
    );
    let mut coordinate_horizontal = marker("coordinate-horizontal", Some([0.0, 0.0]));
    coordinate_horizontal.kind = SketchInputKind::from_native_code_and_layout(4, true);
    let mut coordinate_loci = joins.clone();
    coordinate_loci.insert(
        coordinate_horizontal.id.clone(),
        vec![cadmpeg_ir::sketches::SketchLocus::Start(first.clone())],
    );
    markers.insert(coordinate_horizontal.id.as_str(), &coordinate_horizontal);
    assert_eq!(
        typed_marker_relation_definition(&coordinate_horizontal, &markers, &coordinate_loci,),
        None
    );
    let relation_point = SketchEntityId("sldprt:model:sketch-entity#relation-point:lane:1".into());
    let point_handle = marker("point-handle", None);
    let mut point_horizontal = marker("point-horizontal", None);
    point_horizontal.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    point_horizontal.links = vec![SketchInputLink {
        local_id: 1,
        entity_ref: point_handle.id.clone(),
    }];
    let mut point_loci = joins.clone();
    point_loci.insert(
        point_handle.id.clone(),
        vec![SketchLocus::Entity(relation_point.clone())],
    );
    markers.insert(point_handle.id.as_str(), &point_handle);
    markers.insert(point_horizontal.id.as_str(), &point_horizontal);
    assert!(matches!(
        typed_marker_relation_definition(&point_horizontal, &markers, &point_loci),
        Some(SketchConstraintDefinition::Native { entities, .. })
            if entities == vec![relation_point]
    ));
    let mut operandless_vertical = marker("operandless-vertical", None);
    operandless_vertical.kind = SketchInputKind::Relation(SketchRelationKind::Vertical);
    assert_eq!(
        typed_marker_relation_definition(&operandless_vertical, &markers, &joins),
        None
    );
    operandless_vertical.coordinates_m = Some([0.01, 0.02]);
    assert_eq!(
        typed_marker_relation_definition(&operandless_vertical, &markers, &joins),
        None
    );
    let mut parallel = marker("parallel", None);
    parallel.kind = SketchInputKind::Relation(SketchRelationKind::Parallel);
    parallel.links = vec![
        SketchInputLink {
            local_id: 1,
            entity_ref: "marker-a".into(),
        },
        SketchInputLink {
            local_id: 3,
            entity_ref: "marker-c".into(),
        },
    ];
    markers.insert(parallel.id.as_str(), &parallel);
    assert_eq!(
        typed_marker_relation_definition(&parallel, &markers, &joins),
        Some(SketchConstraintDefinition::Parallel {
            first: first.clone(),
            second: SketchEntityId("second".into()),
        })
    );
    let mut symmetric = marker("symmetric", None);
    symmetric.kind = SketchInputKind::Relation(SketchRelationKind::Symmetric);
    symmetric.links = parallel.links.clone();
    markers.insert(symmetric.id.as_str(), &symmetric);
    assert_eq!(
        typed_marker_relation_definition(&symmetric, &markers, &joins),
        Some(SketchConstraintDefinition::Native {
            native_kind: "sldprt:marker-relation:11".into(),
            native_state: None,
            native_flags: None,
            native_properties: std::collections::BTreeMap::new(),
            entities: vec![first.clone(), SketchEntityId("second".into())],
            parameter: None,
            operands: vec![
                cadmpeg_ir::sketches::SketchNativeOperand {
                    native_kind: "sldprt:marker-local-id".into(),
                    native_field: None,
                    native_role: None,
                    object_index: 1,
                    native_ref: Some("marker-a".into()),
                },
                cadmpeg_ir::sketches::SketchNativeOperand {
                    native_kind: "sldprt:marker-local-id".into(),
                    native_field: None,
                    native_role: None,
                    object_index: 3,
                    native_ref: Some("marker-c".into()),
                },
            ],
        })
    );
    let mut coincident = marker("coincident", None);
    coincident.kind = SketchInputKind::Relation(SketchRelationKind::Coincident);
    coincident.links = parallel.links.clone();
    markers.insert(coincident.id.as_str(), &coincident);
    assert_eq!(
        typed_marker_relation_definition(&coincident, &markers, &joins),
        Some(SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                cadmpeg_ir::sketches::SketchLocus::Start(first.clone()),
                cadmpeg_ir::sketches::SketchLocus::End(SketchEntityId("second".into())),
            ],
        })
    );
    let mut horizontal_points = marker("horizontal-points", None);
    horizontal_points.kind = SketchInputKind::Relation(SketchRelationKind::HorizontalPoints);
    horizontal_points.links = parallel.links.clone();
    markers.insert(horizontal_points.id.as_str(), &horizontal_points);
    assert_eq!(
        typed_marker_relation_definition(&horizontal_points, &markers, &joins),
        Some(SketchConstraintDefinition::HorizontalPoints {
            first: cadmpeg_ir::sketches::SketchLocus::Start(first.clone()),
            second: cadmpeg_ir::sketches::SketchLocus::End(SketchEntityId("second".into())),
        })
    );
    let mut legacy_horizontal_points = marker("legacy-horizontal-points", None);
    legacy_horizontal_points.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    legacy_horizontal_points.links = parallel.links.clone();
    markers.insert(
        legacy_horizontal_points.id.as_str(),
        &legacy_horizontal_points,
    );
    assert_eq!(
        typed_marker_relation_definition(&legacy_horizontal_points, &markers, &joins),
        Some(SketchConstraintDefinition::HorizontalPoints {
            first: cadmpeg_ir::sketches::SketchLocus::Start(first.clone()),
            second: cadmpeg_ir::sketches::SketchLocus::End(SketchEntityId("second".into())),
        })
    );
    let mut entity_marker = marker("entity-marker", Some([0.01, 0.01]));
    entity_marker.kind = SketchInputKind::LineOrCircle;
    let mut midpoint = marker("midpoint", None);
    midpoint.kind = SketchInputKind::Relation(SketchRelationKind::Midpoint);
    midpoint.links = vec![
        SketchInputLink {
            local_id: 3,
            entity_ref: entity_marker.id.clone(),
        },
        SketchInputLink {
            local_id: 1,
            entity_ref: "marker-a".into(),
        },
    ];
    let mut midpoint_loci = joins.clone();
    midpoint_loci.insert(
        entity_marker.id.clone(),
        vec![cadmpeg_ir::sketches::SketchLocus::End(SketchEntityId(
            "second".into(),
        ))],
    );
    markers.insert(entity_marker.id.as_str(), &entity_marker);
    markers.insert(midpoint.id.as_str(), &midpoint);
    assert_eq!(
        typed_marker_relation_definition(&midpoint, &markers, &midpoint_loci),
        Some(SketchConstraintDefinition::Midpoint {
            point: cadmpeg_ir::sketches::SketchLocus::Start(first.clone()),
            entity: SketchEntityId("second".into()),
        })
    );
    let mut arc_marker = marker("arc-marker", None);
    arc_marker.kind = SketchInputKind::Arc;
    let mut arc_loci = midpoint_loci.clone();
    arc_loci.insert(
        arc_marker.id.clone(),
        vec![cadmpeg_ir::sketches::SketchLocus::Entity(SketchEntityId(
            "second".into(),
        ))],
    );
    markers.insert(arc_marker.id.as_str(), &arc_marker);
    for (kind, angle) in [
        (SketchRelationKind::ArcAngle90, std::f64::consts::FRAC_PI_2),
        (SketchRelationKind::ArcAngle180, std::f64::consts::PI),
        (
            SketchRelationKind::ArcAngle270,
            3.0 * std::f64::consts::FRAC_PI_2,
        ),
    ] {
        let mut arc_angle = marker("arc-angle", None);
        arc_angle.kind = SketchInputKind::Relation(kind);
        arc_angle.links = vec![SketchInputLink {
            local_id: 1,
            entity_ref: arc_marker.id.clone(),
        }];
        assert_eq!(
            typed_marker_relation_definition(&arc_angle, &markers, &arc_loci),
            Some(SketchConstraintDefinition::ArcAngle {
                entity: SketchEntityId("second".into()),
                angle: cadmpeg_ir::features::Angle(angle),
            })
        );
        arc_angle.links[0].entity_ref.clone_from(&entity_marker.id);
        assert!(matches!(
            typed_marker_relation_definition(&arc_angle, &markers, &arc_loci),
            Some(SketchConstraintDefinition::Native {
                native_kind,
                entities,
                parameter: None,
                operands,
            ..
            }) if native_kind == format!("sldprt:marker-relation:{}", kind.native_code())
                && entities == vec![SketchEntityId("second".into())]
                && operands.len() == 1
                && operands[0].object_index == 1
                && operands[0].native_ref.as_deref() == Some("entity-marker")
        ));
    }
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
        operands: ["marker-a", "marker-c"]
            .into_iter()
            .enumerate()
            .map(|(index, marker)| FeatureInputOperand {
                offset: index as u64,
                reference_ref: format!("reference-{index}"),
                kind: FeatureInputOperandKind::D6,
                entity_index: index as u16,
                entity_ref: Some(marker.into()),
            })
            .collect(),
    };
    let parameter = |id: &str, display| DesignParameter {
        id: ParameterId(id.into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: id.into(),
        expression: String::new(),
        display,
        value: Some(ParameterValue::Length(Length(2.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let sketch_id = SketchId("sketch".into());
    let distance = parameter("distance", None);
    assert!(matches!(
        typed_relation_definition(
            &relation,
            Some(&distance),
            &sketch_id,
            &[],
            &markers,
            &joins,
        ),
        Some(cadmpeg_ir::sketches::SketchConstraintDefinition::DistanceLoci {
            parameter,
            ..
        }) if parameter.0 == "distance"
    ));
    let same_locus_relation = FeatureInputRelationInstance {
        operands: relation
            .operands
            .iter()
            .cloned()
            .map(|mut operand| {
                operand.entity_ref = Some("marker-a".into());
                operand
            })
            .collect(),
        ..relation.clone()
    };
    assert_eq!(
        typed_relation_definition(
            &same_locus_relation,
            Some(&distance),
            &sketch_id,
            &[],
            &markers,
            &joins,
        ),
        None
    );
    let circle = FeatureInputRelationInstance {
        family: FeatureInputRelationFamily::CircleDiameter,
        operands: vec![FeatureInputOperand {
            offset: 0,
            reference_ref: "circle-reference".into(),
            kind: FeatureInputOperandKind::E1,
            entity_index: 0,
            entity_ref: Some("marker-a".into()),
        }],
        ..relation
    };
    let radius = parameter("circle", Some(DimensionDisplay::Radius));
    assert!(matches!(
        typed_relation_definition(
            &circle,
            Some(&radius),
            &sketch_id,
            &[],
            &markers,
            &joins,
        ),
        Some(SketchConstraintDefinition::Radius { parameter, .. })
            if parameter.0 == "circle"
    ));
    let diameter = parameter("circle", Some(DimensionDisplay::Diameter));
    assert!(matches!(
        typed_relation_definition(
            &circle,
            Some(&diameter),
            &sketch_id,
            &[],
            &markers,
            &joins,
        ),
        Some(SketchConstraintDefinition::Diameter { parameter, .. })
            if parameter.0 == "circle"
    ));
    let undisplayed = parameter("circle", None);
    assert_eq!(
        typed_relation_definition(
            &circle,
            Some(&undisplayed),
            &sketch_id,
            &[],
            &markers,
            &joins,
        ),
        None
    );
    let unresolved_circle = FeatureInputRelationInstance {
        operands: vec![FeatureInputOperand {
            entity_ref: None,
            ..circle.operands[0].clone()
        }],
        ..circle
    };
    let circle_entity = SketchEntity {
        id: SketchEntityId("dimensioned-circle".into()),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
        },
    };
    assert!(matches!(
        typed_relation_definition(
            &unresolved_circle,
            Some(&radius),
            &sketch_id,
            std::slice::from_ref(&circle_entity),
            &markers,
            &joins,
        ),
        Some(SketchConstraintDefinition::Radius { entity, .. })
            if entity == circle_entity.id
    ));
    let mut duplicate_circle = circle_entity.clone();
    duplicate_circle.id = SketchEntityId("duplicate-circle".into());
    assert_eq!(
        typed_relation_definition(
            &unresolved_circle,
            Some(&radius),
            &sketch_id,
            &[circle_entity, duplicate_circle],
            &markers,
            &joins,
        ),
        None
    );
}

#[test]
fn line_handle_interior_points_identify_profile_entities() {
    let sketch = SketchId("sketch".into());
    let line_ids = ["horizontal", "vertical", "offset"].map(|id| SketchEntityId(id.into()));
    let entities = vec![
        SketchEntity {
            id: line_ids[0].clone(),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(10.0, 0.0),
            },
        },
        SketchEntity {
            id: line_ids[1].clone(),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(0.0, 20.0),
            },
        },
        SketchEntity {
            id: line_ids[2].clone(),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(10.0, 3.0),
                end: Point2::new(20.0, 3.0),
            },
        },
    ];
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch),
        },
        native_ref: Some("feature-native".into()),
    };
    let mut native_payload = vec![0; 81];
    let mut markers = Vec::new();
    for (ordinal, (id, coordinates_m)) in [
        ("horizontal-marker", [0.0025, 0.0]),
        ("vertical-marker", [0.0, 0.010]),
        ("offset-marker", [0.015, 0.003]),
    ]
    .into_iter()
    .enumerate()
    {
        let offset = ordinal * 27;
        native_payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        let mut handle = marker(id, Some(coordinates_m));
        handle.ordinal = ordinal as u32;
        handle.offset = offset as u64;
        handle.kind = SketchInputKind::LineOrCircle;
        markers.push(handle);
    }
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
        sketch_entities: markers,
    };

    let joins = profile_loci_by_marker(&[feature], &[], &entities, std::slice::from_ref(&lane));
    for (marker, entity) in [
        ("horizontal-marker", &line_ids[0]),
        ("vertical-marker", &line_ids[1]),
        ("offset-marker", &line_ids[2]),
    ] {
        assert_eq!(
            joins[marker],
            vec![cadmpeg_ir::sketches::SketchLocus::Entity(entity.clone())]
        );
    }
}

#[test]
fn coordinate_less_point_handle_selects_one_shared_endpoint() {
    let sketch = SketchId("sketch".into());
    let first_id = SketchEntityId("first".into());
    let second_id = SketchEntityId("second".into());
    let first = SketchEntity {
        id: first_id.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    };
    let second = SketchEntity {
        id: second_id.clone(),
        sketch,
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(1.0, 0.0),
            end: Point2::new(1.0, 1.0),
        },
    };
    let mut first_marker = marker("first-marker", Some([0.0, 0.0]));
    first_marker.kind = SketchInputKind::LineOrCircle;
    let mut second_marker = marker("second-marker", Some([0.0, 0.0]));
    second_marker.kind = SketchInputKind::LineOrCircle;
    let mut point = marker("point", None);
    point.links = vec![
        SketchInputLink {
            local_id: 1,
            entity_ref: first_marker.id.clone(),
        },
        SketchInputLink {
            local_id: 2,
            entity_ref: second_marker.id.clone(),
        },
    ];
    let markers = HashMap::from([
        (first_marker.id.as_str(), &first_marker),
        (second_marker.id.as_str(), &second_marker),
        (point.id.as_str(), &point),
    ]);
    let loci = HashMap::from([
        (
            first_marker.id.clone(),
            vec![SketchLocus::Entity(first_id.clone())],
        ),
        (
            second_marker.id.clone(),
            vec![SketchLocus::Entity(second_id.clone())],
        ),
    ]);
    let entities = HashMap::from([(&first.id, &first), (&second.id, &second)]);

    assert_eq!(
        unique_linked_endpoint_locus(&point, &markers, &loci, &entities, 1.0e-8),
        Some(SketchLocus::End(first_id))
    );

    let mut ambiguous = second;
    ambiguous.geometry = SketchGeometry::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    };
    let entities = HashMap::from([(&first.id, &first), (&ambiguous.id, &ambiguous)]);
    assert_eq!(
        unique_linked_endpoint_locus(&point, &markers, &loci, &entities, 1.0e-8),
        None
    );
}

#[test]
fn curve_handles_reject_point_geometry() {
    let point = SketchGeometry::Point {
        position: Point2::new(0.0, 0.0),
    };
    let line = SketchGeometry::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    };
    let circle = SketchGeometry::Circle {
        center: Point2::new(0.0, 0.0),
        radius: Length(1.0),
    };

    assert!(!super::marker_accepts_locus(
        SketchInputKind::LineOrCircle,
        &point
    ));
    assert!(super::marker_accepts_locus(
        SketchInputKind::LineOrCircle,
        &line
    ));
    assert!(super::marker_accepts_locus(
        SketchInputKind::LineOrCircle,
        &circle
    ));
}

#[test]
fn symmetry_invariant_marker_identifies_profile_entity() {
    let sketch = SketchId("sketch".into());
    let circle = SketchEntityId("circle".into());
    let entity = SketchEntity {
        id: circle.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(10.0),
        },
    };
    let points = [-10.0, 10.0].map(|u| SketchEntity {
        id: SketchEntityId(format!("point-{u}")),
        sketch: sketch.clone(),
        construction: true,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(u, 0.0),
        },
    });
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch),
        },
        native_ref: Some("feature-native".into()),
    };
    let mut native_payload = vec![0; 54];
    for offset in [0, 27] {
        native_payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    }
    let mut handle = marker("circle-marker", Some([0.0, 0.0]));
    handle.kind = SketchInputKind::LineOrCircle;
    let mut point = marker("point-marker", Some([0.01, 0.0]));
    point.ordinal = 1;
    point.offset = 27;
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
        sketch_entities: vec![handle, point],
    };

    let mut entities = vec![entity];
    entities.extend(points);
    let joins = profile_loci_by_marker(&[feature], &[], &entities, &[lane]);
    assert_eq!(
        joins["circle-marker"],
        vec![cadmpeg_ir::sketches::SketchLocus::Entity(circle)]
    );
}

#[test]
fn unique_axis_swap_maps_marker_coordinates_to_profile_loci() {
    let markers = [(0, 0), (2, 1), (7, 4), (3, 9)].into_iter().collect();
    let loci = [(0, 0), (1, 2), (4, 7), (9, 3)].into_iter().collect();
    let transform = unique_marker_transform(&markers, &loci).expect("unique transform");
    assert!(transform.swap);
    assert_eq!(transform.u_sign, 1);
    assert_eq!(transform.v_sign, 1);
    assert!(markers
        .into_iter()
        .all(|point| loci.contains(&transform.apply(point).expect("required invariant"))));
}

#[test]
fn relation_point_materializes_under_one_proven_marker_transform() {
    let sketch = SketchId("sketch".into());
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let mut entities = [(0.0, 0.0), (1.0, 2.0), (4.0, 7.0)]
        .into_iter()
        .enumerate()
        .map(|(index, (u, v))| SketchEntity {
            id: SketchEntityId(format!("point-{index}")),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(u, v),
            },
        })
        .collect::<Vec<_>>();
    let mut markers = [[0.0, 0.0], [0.002, 0.001], [0.007, 0.004]]
        .into_iter()
        .enumerate()
        .map(|(index, coordinates)| {
            let mut value = marker(&format!("anchor-{index}"), Some(coordinates));
            value.offset = (index * 27) as u64;
            value
        })
        .collect::<Vec<_>>();
    let mut relation_point = marker("relation-point", Some([0.005, 0.006]));
    relation_point.offset = 81;
    markers.push(relation_point.clone());
    let mut endpoint_a = marker("endpoint-a", Some([0.002, 0.001]));
    endpoint_a.offset = 82;
    let mut endpoint_b = marker("endpoint-b", Some([0.007, 0.004]));
    endpoint_b.offset = 83;
    let mut relation_line = marker("relation-line", None);
    relation_line.offset = 84;
    relation_line.kind = SketchInputKind::Arc;
    let mut support_handle = marker("support-handle", None);
    support_handle.offset = 85;
    support_handle.links = vec![SketchInputLink {
        local_id: 3,
        entity_ref: relation_line.id.clone(),
    }];
    let mut qualified_curve = marker("qualified-curve", Some([0.0045, 0.0025]));
    qualified_curve.id = "sldprt:feature-input:sketch-entity#qualified-curve".into();
    qualified_curve.offset = 86;
    qualified_curve.kind = SketchInputKind::LineOrCircle;
    relation_line.links = vec![
        SketchInputLink {
            local_id: 1,
            entity_ref: endpoint_a.id.clone(),
        },
        SketchInputLink {
            local_id: 2,
            entity_ref: qualified_curve.id.clone(),
        },
    ];
    let mut coincident_point = marker("coincident-point", Some([0.002, 0.001]));
    coincident_point.offset = 87;
    let mut self_linked_curve = marker("self-linked-curve", Some([0.006, 0.005]));
    self_linked_curve.offset = 88;
    self_linked_curve.kind = SketchInputKind::Arc;
    self_linked_curve.links = vec![
        SketchInputLink {
            local_id: 8,
            entity_ref: self_linked_curve.id.clone(),
        },
        SketchInputLink {
            local_id: 9,
            entity_ref: endpoint_b.id.clone(),
        },
    ];
    let mut forward_linked_curve = marker("forward-linked-curve", Some([0.009, 0.009]));
    forward_linked_curve.offset = 89;
    forward_linked_curve.kind = SketchInputKind::Arc;
    forward_linked_curve.links = vec![
        SketchInputLink {
            local_id: 10,
            entity_ref: endpoint_a.id.clone(),
        },
        SketchInputLink {
            local_id: 11,
            entity_ref: endpoint_b.id.clone(),
        },
    ];
    markers.extend([
        endpoint_a,
        endpoint_b,
        relation_line.clone(),
        support_handle.clone(),
        qualified_curve.clone(),
        coincident_point.clone(),
        self_linked_curve.clone(),
        forward_linked_curve.clone(),
    ]);
    let mut native_payload = vec![0; 181];
    for offset in [0, 27, 54] {
        native_payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    }
    native_payload[84..84 + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    native_payload[89..97].fill(0xff);
    native_payload[97..101].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    native_payload[101..105].copy_from_slice(&2u32.to_le_bytes());
    native_payload[107..111].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    native_payload[111..113].copy_from_slice(&1u16.to_le_bytes());
    native_payload[115..123].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    native_payload[132..140].copy_from_slice(&1.0f64.to_le_bytes());
    native_payload[148..150].copy_from_slice(&4u16.to_le_bytes());
    native_payload[150..152].copy_from_slice(&6u16.to_le_bytes());
    native_payload[152..156].copy_from_slice(&1u32.to_le_bytes());
    native_payload[156..164].copy_from_slice(&(-1.0f64).to_le_bytes());
    native_payload[176..176 + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload,
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![
            FeatureInputRelationInstance {
                id: "relation".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 90,
                family: FeatureInputRelationFamily::CircleDiameter,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalar_refs: Vec::new(),
                parameter_scalar_ref: None,
                display_scalar_ref: None,
                operands: vec![FeatureInputOperand {
                    offset: 91,
                    reference_ref: "reference".into(),
                    kind: FeatureInputOperandKind::Native(0x929d),
                    entity_index: 0,
                    entity_ref: Some(relation_point.id.clone()),
                }],
            },
            FeatureInputRelationInstance {
                id: "qualified-point-relation".into(),
                parent: "lane".into(),
                ordinal: 2,
                offset: 94,
                family: FeatureInputRelationFamily::PointPointDistance,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalar_refs: Vec::new(),
                parameter_scalar_ref: None,
                display_scalar_ref: None,
                operands: vec![
                    FeatureInputOperand {
                        offset: 95,
                        reference_ref: "qualified-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x837b),
                        entity_index: 16,
                        entity_ref: Some(qualified_curve.id.clone()),
                    },
                    FeatureInputOperand {
                        offset: 96,
                        reference_ref: "point-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x837b),
                        entity_index: 17,
                        entity_ref: Some(relation_point.id.clone()),
                    },
                ],
            },
            FeatureInputRelationInstance {
                id: "line-relation".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 92,
                family: FeatureInputRelationFamily::LineLineDistance,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalar_refs: Vec::new(),
                parameter_scalar_ref: None,
                display_scalar_ref: None,
                operands: vec![FeatureInputOperand {
                    offset: 93,
                    reference_ref: "line-reference".into(),
                    kind: FeatureInputOperandKind::Native(0x8386),
                    entity_index: 0,
                    entity_ref: Some(support_handle.id.clone()),
                }],
            },
            FeatureInputRelationInstance {
                id: "coincident-point-relation".into(),
                parent: "lane".into(),
                ordinal: 3,
                offset: 97,
                family: FeatureInputRelationFamily::PointPointDistance,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalar_refs: Vec::new(),
                parameter_scalar_ref: None,
                display_scalar_ref: None,
                operands: vec![
                    FeatureInputOperand {
                        offset: 98,
                        reference_ref: "coincident-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x837b),
                        entity_index: 18,
                        entity_ref: Some(coincident_point.id.clone()),
                    },
                    FeatureInputOperand {
                        offset: 99,
                        reference_ref: "coincident-pair-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x837b),
                        entity_index: 17,
                        entity_ref: Some(relation_point.id.clone()),
                    },
                ],
            },
            FeatureInputRelationInstance {
                id: "self-linked-curve-relation".into(),
                parent: "lane".into(),
                ordinal: 4,
                offset: 100,
                family: FeatureInputRelationFamily::Angle,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalar_refs: Vec::new(),
                parameter_scalar_ref: None,
                display_scalar_ref: None,
                operands: vec![
                    FeatureInputOperand {
                        offset: 101,
                        reference_ref: "self-linked-curve-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x8386),
                        entity_index: 18,
                        entity_ref: Some(self_linked_curve.id.clone()),
                    },
                    FeatureInputOperand {
                        offset: 102,
                        reference_ref: "support-curve-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x8386),
                        entity_index: 19,
                        entity_ref: Some(support_handle.id.clone()),
                    },
                ],
            },
            FeatureInputRelationInstance {
                id: "forward-linked-curve-relation".into(),
                parent: "lane".into(),
                ordinal: 5,
                offset: 103,
                family: FeatureInputRelationFamily::LineLineDistance,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalar_refs: Vec::new(),
                parameter_scalar_ref: None,
                display_scalar_ref: None,
                operands: vec![
                    FeatureInputOperand {
                        offset: 104,
                        reference_ref: "forward-linked-curve-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x8386),
                        entity_index: 20,
                        entity_ref: Some(forward_linked_curve.id.clone()),
                    },
                    FeatureInputOperand {
                        offset: 105,
                        reference_ref: "forward-support-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x8386),
                        entity_index: 21,
                        entity_ref: Some(support_handle.id.clone()),
                    },
                ],
            },
        ],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: markers,
    };
    project_relation_point_geometry(
        &mut entities,
        &[],
        std::slice::from_ref(&feature),
        std::slice::from_ref(&lane),
    );
    assert!(entities.iter().any(|entity| {
        entity.construction
            && entity.native_ref.as_deref() == Some("relation-point")
            && matches!(
                entity.geometry,
                SketchGeometry::Point { position } if position == Point2::new(6.0, 5.0)
            )
    }));
    assert!(entities.iter().any(|entity| {
        entity.construction
            && entity.native_ref.as_deref() == Some("self-linked-curve")
            && entity.endpoint_refs == ["endpoint-b", "self-linked-curve"]
            && matches!(entity.geometry, SketchGeometry::Line { start, end }
                if start == Point2::new(4.0, 7.0) && end == Point2::new(5.0, 6.0))
    }));
    assert!(entities.iter().any(|entity| {
        entity.construction
            && entity.native_ref.as_deref() == Some("forward-linked-curve")
            && entity.endpoint_refs == ["endpoint-a", "endpoint-b"]
            && matches!(entity.geometry, SketchGeometry::Line { start, end }
                if start == Point2::new(1.0, 2.0) && end == Point2::new(4.0, 7.0))
    }));
    assert!(entities.iter().any(|entity| {
        entity.construction
            && entity.native_ref.as_deref() == Some("coincident-point")
            && matches!(
                entity.geometry,
                SketchGeometry::Point { position } if position == Point2::new(1.0, 2.0)
            )
    }));
    assert!(entities.iter().any(|entity| {
        entity.construction
            && entity.native_ref.is_none()
            && entity.geometry_ref.as_deref()
                == Some("sldprt:feature-input:sketch-entity#qualified-curve")
            && matches!(
                entity.geometry,
                SketchGeometry::Point { position } if position == Point2::new(2.5, 4.5)
            )
    }));
    assert!(entities.iter().any(|entity| {
        entity.construction
            && entity.native_ref.as_deref() == Some("relation-line")
            && entity.endpoint_refs
                == [
                    "endpoint-a",
                    "sldprt:feature-input:sketch-entity#qualified-curve",
                ]
            && matches!(entity.geometry, SketchGeometry::Line { start, end }
                if start == Point2::new(1.0, 2.0) && end == Point2::new(2.5, 4.5))
    }));
    let loci = profile_loci_by_marker(
        std::slice::from_ref(&feature),
        &[],
        &entities,
        std::slice::from_ref(&lane),
    );
    assert_eq!(
        loci["sldprt:feature-input:sketch-entity#qualified-curve:qualified-point"],
        vec![SketchLocus::End(SketchEntityId(
            "sldprt:model:sketch-entity#relation-line:lane:84".into(),
        ))]
    );
    let markers = lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        marker_point_locus(
            "sldprt:feature-input:sketch-entity#qualified-curve",
            &markers,
            &loci,
        ),
        Some(SketchLocus::End(SketchEntityId(
            "sldprt:model:sketch-entity#relation-line:lane:84".into(),
        )))
    );
}

#[test]
fn relation_point_uses_resolved_sketch_frame_when_marker_transform_is_ambiguous() {
    let sketch = SketchId("sketch".into());
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let sketch_record = Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let mut first_marker = marker("first-point", Some([-0.005, 0.002]));
    first_marker.offset = 1;
    let mut second_marker = marker("second-point", Some([0.005, 0.002]));
    second_marker.offset = 2;
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 3,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class".into(),
        feature_ref: "feature-native".into(),
        scalar_refs: vec!["distance".into()],
        parameter_scalar_ref: Some("distance".into()),
        display_scalar_ref: None,
        operands: vec![
            FeatureInputOperand {
                offset: 4,
                reference_ref: "first-reference".into(),
                kind: FeatureInputOperandKind::D6,
                entity_index: 0,
                entity_ref: Some(first_marker.id.clone()),
            },
            FeatureInputOperand {
                offset: 5,
                reference_ref: "second-reference".into(),
                kind: FeatureInputOperandKind::D6,
                entity_index: 1,
                entity_ref: Some(second_marker.id.clone()),
            },
        ],
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![relation],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![first_marker, second_marker],
    };
    let mut entities = Vec::new();

    project_relation_point_geometry(
        &mut entities,
        std::slice::from_ref(&sketch_record),
        std::slice::from_ref(&feature),
        std::slice::from_ref(&lane),
    );

    assert_eq!(entities.len(), 2);
    assert!(entities.iter().any(|entity| {
        entity.native_ref.as_deref() == Some("first-point")
            && matches!(
                entity.geometry,
                SketchGeometry::Point { position } if position == Point2::new(-5.0, 2.0)
            )
    }));
    assert!(entities.iter().any(|entity| {
        entity.native_ref.as_deref() == Some("second-point")
            && matches!(
                entity.geometry,
                SketchGeometry::Point { position } if position == Point2::new(5.0, 2.0)
            )
    }));
}

#[test]
fn unique_zero_translation_resolves_symmetric_axis_swaps() {
    let markers = [(0, 0), (48, 0), (48, 24), (0, 24)].into_iter().collect();
    let loci = [(0, 0), (24, 0), (24, 48), (0, 48)].into_iter().collect();
    assert_eq!(
        unique_marker_transform(&markers, &loci),
        Some(MarkerTransform {
            swap: true,
            u_sign: 1,
            v_sign: 1,
            affine_matrix: None,
            translation: (0, 0),
        })
    );
}

#[test]
fn marker_kinds_disambiguate_axis_swaps() {
    let compatible = HashMap::from([
        ((0, 0), HashSet::from([(10, 20)])),
        ((0, 2), HashSet::from([(12, 20)])),
        ((3, 1), HashSet::from([(11, 23)])),
    ]);
    let transform = unique_compatible_marker_transform(&compatible).expect("required invariant");
    assert!(transform.swap);
    assert_eq!(transform.u_sign, 1);
    assert_eq!(transform.v_sign, 1);
    assert_eq!(transform.translation, (10, 20));
}

#[test]
fn symmetric_frames_require_the_same_dimensioned_circle_set() {
    let identity = MarkerTransform {
        swap: false,
        u_sign: 1,
        v_sign: 1,
        affine_matrix: None,
        translation: (0, 0),
    };
    let swap = MarkerTransform {
        swap: true,
        ..identity
    };
    assert_eq!(
        dimensioned_circle_transform(&[swap, identity], &[((10, 20), 5), ((20, 10), 5)]),
        Some(identity)
    );
    assert_eq!(
        dimensioned_circle_transform(&[identity, swap], &[((10, 20), 5), ((20, 10), 7)]),
        None
    );
}

#[test]
fn cylinder_centers_resolve_dimensioned_circle_frame() {
    let sketch = Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(20.0, 20.0, 0.0),
            normal: Vector3::new(-1.0, 0.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let circles = [((6, 14), 3), ((14, 14), 3), ((14, 7), 3), ((6, 7), 3)];
    let surfaces = [(14.0, -6.0), (14.0, -14.0), (7.0, -14.0), (7.0, -6.0)]
        .into_iter()
        .enumerate()
        .map(|(index, (y, z))| Surface {
            id: SurfaceId(format!("cylinder-{index}")),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(19.5, y, z),
                axis: Vector3::new(1.0, 0.0, 0.0),
                ref_direction: Vector3::new(0.0, 1.0, 0.0),
                radius: 3.0,
            },
            source_object: None,
        })
        .collect::<Vec<_>>();
    let candidates = dimensioned_circle_surface_transforms(&sketch, &surfaces, &circles, 1.0);
    let transform =
        dimensioned_circle_transform(&candidates, &circles).expect("required invariant");
    let transformed = circles
        .iter()
        .map(|(center, _)| transform.apply(*center).expect("required invariant"))
        .collect::<HashSet<_>>();
    assert_eq!(
        transformed,
        HashSet::from([(-6, -6), (-14, -6), (-14, -13), (-6, -13)])
    );
}

#[test]
fn circular_profile_binds_by_unique_diameter_signature() {
    let sketch_id = SketchId("circle-profile".into());
    let entity_id = SketchEntityId("circle".into());
    let feature = |id: &str, name: &str, sketch| Feature {
        id: FeatureId(id.into()),
        ordinal: 0,
        name: Some(name.into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch,
        },
        native_ref: Some(format!("native-{id}")),
    };
    let mut features = vec![
        feature("first", "Sketch1", None),
        feature("second", "Sketch2", Some(sketch_id.clone())),
    ];
    let parameter = |id: &str, owner: &str, diameter: f64| DesignParameter {
        id: ParameterId(id.into()),
        owner: Some(FeatureId(owner.into())),
        ordinal: 0,
        name: "D1".into(),
        expression: format!("<MOD-DIAM>{diameter}"),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(Length(diameter))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let parameters = [
        parameter("first-diameter", "first", 4.0),
        parameter("second-diameter", "second", 5.0),
    ];
    let mut sketches = [Sketch {
        id: sketch_id.clone(),
        name: Some("Sketch2".into()),
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![vec![cadmpeg_ir::sketches::SketchEntityUse {
            entity: entity_id.clone(),
            reversed: false,
        }]],
        native_ref: None,
    }];
    let entities = [SketchEntity {
        id: entity_id,
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
        },
    }];

    bind_circular_profile_by_dimension(&mut features, &mut sketches, &entities, &parameters);

    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Sketch { sketch: Some(id), .. } if id == &sketch_id
    ));
    assert!(matches!(
        &features[1].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));
    assert_eq!(sketches[0].name.as_deref(), Some("Sketch1"));
}
