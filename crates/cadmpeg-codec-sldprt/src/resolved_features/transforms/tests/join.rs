//! Unique-translation profile join tests.

use super::super::*;
use super::marker;
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputKind, SketchInputLink, SketchRelationKind,
};
use cadmpeg_ir::features::{
    DesignParameter, DimensionDisplay, Feature, FeatureDefinition, FeatureId, Length, ParameterId,
    ParameterValue,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchCoordinateAxis, SketchEntity, SketchEntityId, SketchGeometry,
    SketchId, SketchLocus, SketchNativeOperand,
};
use std::collections::{BTreeMap, HashMap};

#[test]
fn unique_translation_joins_linked_endpoints_to_one_profile_entity() {
    let sketch = SketchId("sketch".into());
    let first = SketchEntityId("first".into());
    let second = SketchEntityId("second".into());
    let entities = vec![
        SketchEntity::new(
            first.clone(),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(10.0, 20.0),
                end: Point2::new(20.0, 20.0),
            },
        ),
        SketchEntity::new(
            second.clone(),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(20.0, 20.0),
                end: Point2::new(20.0, 30.0),
            },
        ),
    ];
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
        Some(SketchConstraintDefinition::SameCoordinate {
            first: cadmpeg_ir::sketches::SketchLocus::Start(first.clone()),
            second: cadmpeg_ir::sketches::SketchLocus::End(SketchEntityId("second".into())),
            axis: SketchCoordinateAxis::V,
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
        Some(SketchConstraintDefinition::SameCoordinate {
            first: cadmpeg_ir::sketches::SketchLocus::Start(first.clone()),
            second: cadmpeg_ir::sketches::SketchLocus::End(SketchEntityId("second".into())),
            axis: SketchCoordinateAxis::V,
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
    let circle_entity = SketchEntity::new(
        SketchEntityId("dimensioned-circle".into()),
        sketch_id.clone(),
        SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
        },
    );
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
            if entity == circle_entity.id().clone()
    ));
    let duplicate_circle = SketchEntity::new(
        SketchEntityId("duplicate-circle".into()),
        circle_entity.sketch.clone(),
        circle_entity.geometry.clone(),
    )
    .with_construction(circle_entity.construction)
    .with_native_ref(circle_entity.native_ref.clone())
    .with_geometry_ref(circle_entity.geometry_ref.clone())
    .with_endpoint_refs(circle_entity.endpoint_refs.clone());
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
        SketchEntity::new(
            line_ids[0].clone(),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(10.0, 0.0),
            },
        ),
        SketchEntity::new(
            line_ids[1].clone(),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(0.0, 20.0),
            },
        ),
        SketchEntity::new(
            line_ids[2].clone(),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(10.0, 3.0),
                end: Point2::new(20.0, 3.0),
            },
        ),
    ];
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
    let first = SketchEntity::new(
        first_id.clone(),
        sketch.clone(),
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    );
    let second = SketchEntity::new(
        second_id.clone(),
        sketch,
        SketchGeometry::Line {
            start: Point2::new(1.0, 0.0),
            end: Point2::new(1.0, 1.0),
        },
    );
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
    let entities = HashMap::from([(first.id(), &first), (second.id(), &second)]);

    assert_eq!(
        unique_linked_endpoint_locus(&point, &markers, &loci, &entities, 1.0e-8),
        Some(SketchLocus::End(first_id))
    );

    let mut ambiguous = second;
    ambiguous.geometry = SketchGeometry::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    };
    let entities = HashMap::from([(first.id(), &first), (ambiguous.id(), &ambiguous)]);
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
    let entity = SketchEntity::new(
        circle.clone(),
        sketch.clone(),
        SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(10.0),
        },
    );
    let points = [-10.0, 10.0].map(|u| {
        SketchEntity::new(
            SketchEntityId(format!("point-{u}")),
            sketch.clone(),
            SketchGeometry::Point {
                position: Point2::new(u, 0.0),
            },
        )
        .with_construction(true)
    });
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
