//! Unique-translation profile join tests.

use super::super::*;
use super::marker;
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputKind, SketchInputLink, SketchRelationKind,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntity, SketchEntityId,
    SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus, SketchNativeOperand,
};
use cadmpeg_ir::{
    features::{
        DesignParameter, DimensionDisplay, Feature, FeatureDefinition, FeatureId, FeatureOperation,
        ParameterId, ParameterValue,
    },
    scalar::Length,
};
use std::collections::{BTreeMap, HashMap};

#[test]
fn unique_translation_joins_linked_endpoints_to_one_profile_entity() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let first = SketchEntityId::mint("synthetic:test:id#first").unwrap();
    let second = SketchEntityId::mint("synthetic:test:id#second").unwrap();
    let entities = vec![
        SketchEntity::new(
            first.clone(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(10.0, 20.0),
                end: Point2::new(20.0, 20.0),
            })
            .unwrap(),
        ),
        SketchEntity::new(
            second.clone(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(20.0, 20.0),
                end: Point2::new(20.0, 30.0),
            })
            .unwrap(),
        ),
    ];
    let feature = Feature {
        id: FeatureId::mint("synthetic:test:id#feature").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }),
        ),
        native_ref: Some("feature-native".into()),
    };
    let mut reference = marker("reference", None);
    reference.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 1,
                entity_ref: "marker-a".into(),
            },
            SketchInputLink {
                local_id: 2,
                entity_ref: "marker-b".into(),
            },
        ],
    );
    reference.reclassify(SketchInputKind::Relation(SketchRelationKind::Vertical));
    let mut native_payload = vec![0; 108];
    for offset in [0, 27, 54] {
        native_payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    }
    native_payload[81 + 23..81 + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    let mut marker_a = marker("marker-a", Some([0.0, 0.0]));
    marker_a = marker_a.with_test_position(marker_a.ordinal(), 0);
    let mut marker_b = marker("marker-b", Some([0.01, 0.0]));
    marker_b = marker_b.with_test_position(marker_b.ordinal(), 27);
    let mut marker_c = marker("marker-c", Some([0.01, 0.01]));
    marker_c = marker_c.with_test_position(marker_c.ordinal(), 54);
    let mut display = marker("display", Some([0.1, 0.1]));
    display = display.with_test_position(display.ordinal(), 81);
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
        .map(|marker| (marker.id(), marker))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        marker_entities("reference", &markers, &joins),
        vec![first.clone()]
    );
    let mut wrapper = marker("wrapper", None);
    wrapper.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 1,
            entity_ref: "marker-a".into(),
        }],
    );
    let mut nested_reference = reference.clone();
    nested_reference.set_test_id("nested-reference");
    nested_reference.links.as_mut().unwrap().entries_mut()[0].entity_ref = wrapper.id().to_string();
    markers.insert(wrapper.id(), &wrapper);
    markers.insert(nested_reference.id(), &nested_reference);
    assert_eq!(
        marker_entities("nested-reference", &markers, &joins),
        vec![first.clone()]
    );
    let mut cycle = marker("cycle", None);
    cycle.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 1,
            entity_ref: cycle.id().to_string(),
        }],
    );
    markers.insert(cycle.id(), &cycle);
    assert!(marker_entities("cycle", &markers, &joins).is_empty());
    assert_eq!(
        typed_marker_relation_definition(markers["reference"], &markers, &joins,),
        Some(SketchConstraintDefinitionInput::Vertical {
            entity: first.clone(),
        })
    );
    let mut nested_horizontal = nested_reference.clone();
    nested_horizontal.reclassify(SketchInputKind::Relation(
        SketchRelationKind::HorizontalPoints,
    ));
    assert!(matches!(
        typed_marker_relation_definition(&nested_horizontal, &markers, &joins),
        Some(SketchConstraintDefinitionInput::Native { ref native_kind, .. })
            if native_kind == "sldprt:marker-relation:25"
    ));
    let mut nested_native = nested_reference.clone();
    nested_native.reclassify(SketchInputKind::from_native_code(28));
    assert_eq!(
        typed_marker_relation_definition(&nested_native, &markers, &joins),
        Some(SketchConstraintDefinitionInput::Native {
            native_kind: cadmpeg_ir::products::NonEmptyString::new("sldprt:marker-relation:28")
                .unwrap(),
            native_state: None,
            native_flags: None,
            native_properties: std::collections::BTreeMap::new(),
            entities: vec![first.clone(), second.clone()],
            parameter: None,
            operands: vec![
                SketchNativeOperand {
                    native_kind: cadmpeg_ir::products::NonEmptyString::new(
                        "sldprt:marker-local-id"
                    )
                    .expect("source operand kind is nonempty"),
                    field: None,
                    object_index: Some(1),
                    native_ref: Some("wrapper".into()),
                },
                SketchNativeOperand {
                    native_kind: cadmpeg_ir::products::NonEmptyString::new(
                        "sldprt:marker-local-id"
                    )
                    .expect("source operand kind is nonempty"),
                    field: None,
                    object_index: Some(2),
                    native_ref: Some("marker-b".into()),
                },
            ],
        })
    );
    let mut coordinate_horizontal = marker("coordinate-horizontal", Some([0.0, 0.0]));
    coordinate_horizontal.reclassify(SketchInputKind::from_native_code_and_layout(4, true));
    let mut coordinate_loci = joins.clone();
    coordinate_loci.insert(
        coordinate_horizontal.id().to_string(),
        vec![cadmpeg_ir::sketches::SketchLocus::Start(first.clone())],
    );
    markers.insert(coordinate_horizontal.id(), &coordinate_horizontal);
    assert_eq!(
        typed_marker_relation_definition(&coordinate_horizontal, &markers, &coordinate_loci,),
        None
    );
    let relation_point =
        SketchEntityId::mint("sldprt:model:sketch-entity#relation-point:lane:1").unwrap();
    let point_handle = marker("point-handle", None);
    let mut point_horizontal = marker("point-horizontal", None);
    point_horizontal.reclassify(SketchInputKind::Relation(SketchRelationKind::Horizontal));
    point_horizontal.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 1,
            entity_ref: point_handle.id().to_string(),
        }],
    );
    let mut point_loci = joins.clone();
    point_loci.insert(
        point_handle.id().to_string(),
        vec![SketchLocus::Entity(relation_point.clone())],
    );
    markers.insert(point_handle.id(), &point_handle);
    markers.insert(point_horizontal.id(), &point_horizontal);
    assert!(matches!(
        typed_marker_relation_definition(&point_horizontal, &markers, &point_loci),
        Some(SketchConstraintDefinitionInput::Native { entities, .. })
            if entities == vec![relation_point]
    ));

    let mut operandless_vertical = marker("operandless-vertical", None);
    operandless_vertical.reclassify(SketchInputKind::Relation(SketchRelationKind::Vertical));
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
    parallel.reclassify(SketchInputKind::Relation(SketchRelationKind::Parallel));
    parallel.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 1,
                entity_ref: "marker-a".into(),
            },
            SketchInputLink {
                local_id: 3,
                entity_ref: "marker-c".into(),
            },
        ],
    );
    markers.insert(parallel.id(), &parallel);
    assert_eq!(
        typed_marker_relation_definition(&parallel, &markers, &joins),
        Some(SketchConstraintDefinitionInput::Parallel {
            first: first.clone(),
            second: SketchEntityId::mint("synthetic:test:id#second").unwrap(),
        })
    );
    let mut symmetric = marker("symmetric", None);
    symmetric.reclassify(SketchInputKind::Relation(SketchRelationKind::Symmetric));
    symmetric.links = parallel.links.clone();
    markers.insert(symmetric.id(), &symmetric);
    assert_eq!(
        typed_marker_relation_definition(&symmetric, &markers, &joins),
        Some(SketchConstraintDefinitionInput::Native {
            native_kind: cadmpeg_ir::products::NonEmptyString::new("sldprt:marker-relation:11")
                .unwrap(),
            native_state: None,
            native_flags: None,
            native_properties: std::collections::BTreeMap::new(),
            entities: vec![
                first.clone(),
                SketchEntityId::mint("synthetic:test:id#second").unwrap()
            ],
            parameter: None,
            operands: vec![
                cadmpeg_ir::sketches::SketchNativeOperand {
                    native_kind: cadmpeg_ir::products::NonEmptyString::new(
                        "sldprt:marker-local-id"
                    )
                    .expect("source operand kind is nonempty"),
                    field: None,
                    object_index: Some(1),
                    native_ref: Some("marker-a".into()),
                },
                cadmpeg_ir::sketches::SketchNativeOperand {
                    native_kind: cadmpeg_ir::products::NonEmptyString::new(
                        "sldprt:marker-local-id"
                    )
                    .expect("source operand kind is nonempty"),
                    field: None,
                    object_index: Some(3),
                    native_ref: Some("marker-c".into()),
                },
            ],
        })
    );
    let mut coincident = marker("coincident", None);
    coincident.reclassify(SketchInputKind::Relation(SketchRelationKind::Coincident));
    coincident.links = parallel.links.clone();
    markers.insert(coincident.id(), &coincident);
    assert_eq!(
        typed_marker_relation_definition(&coincident, &markers, &joins),
        Some(SketchConstraintDefinitionInput::CoincidentLoci {
            loci: vec![
                cadmpeg_ir::sketches::SketchLocus::Start(first.clone()),
                cadmpeg_ir::sketches::SketchLocus::End(
                    SketchEntityId::mint("synthetic:test:id#second").unwrap()
                ),
            ],
        })
    );
    let mut horizontal_points = marker("horizontal-points", None);
    horizontal_points.reclassify(SketchInputKind::Relation(
        SketchRelationKind::HorizontalPoints,
    ));
    horizontal_points.links = parallel.links.clone();
    markers.insert(horizontal_points.id(), &horizontal_points);
    assert_eq!(
        typed_marker_relation_definition(&horizontal_points, &markers, &joins),
        Some(SketchConstraintDefinitionInput::SameCoordinate {
            relation: cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                cadmpeg_ir::sketches::SketchLocus::Start(first.clone()),
                cadmpeg_ir::sketches::SketchLocus::End(
                    SketchEntityId::mint("synthetic:test:id#second").unwrap()
                ),
                SketchCoordinateAxis::V
            )
            .unwrap()
        })
    );
    let mut legacy_horizontal_points = marker("legacy-horizontal-points", None);
    legacy_horizontal_points.reclassify(SketchInputKind::Relation(SketchRelationKind::Horizontal));
    legacy_horizontal_points.links = parallel.links.clone();
    markers.insert(legacy_horizontal_points.id(), &legacy_horizontal_points);
    assert_eq!(
        typed_marker_relation_definition(&legacy_horizontal_points, &markers, &joins),
        Some(SketchConstraintDefinitionInput::SameCoordinate {
            relation: cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                cadmpeg_ir::sketches::SketchLocus::Start(first.clone()),
                cadmpeg_ir::sketches::SketchLocus::End(
                    SketchEntityId::mint("synthetic:test:id#second").unwrap()
                ),
                SketchCoordinateAxis::V
            )
            .unwrap()
        })
    );
    let mut entity_marker = marker("entity-marker", Some([0.01, 0.01]));
    entity_marker.reclassify(SketchInputKind::LineOrCircle);
    let mut midpoint = marker("midpoint", None);
    midpoint.reclassify(SketchInputKind::Relation(SketchRelationKind::Midpoint));
    midpoint.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 3,
                entity_ref: entity_marker.id().to_string(),
            },
            SketchInputLink {
                local_id: 1,
                entity_ref: "marker-a".into(),
            },
        ],
    );
    let mut midpoint_loci = joins.clone();
    midpoint_loci.insert(
        entity_marker.id().to_string(),
        vec![cadmpeg_ir::sketches::SketchLocus::End(
            SketchEntityId::mint("synthetic:test:id#second").unwrap(),
        )],
    );
    markers.insert(entity_marker.id(), &entity_marker);
    markers.insert(midpoint.id(), &midpoint);
    assert_eq!(
        typed_marker_relation_definition(&midpoint, &markers, &midpoint_loci),
        Some(SketchConstraintDefinitionInput::Midpoint {
            point: cadmpeg_ir::sketches::SketchLocus::Start(first.clone()),
            entity: SketchEntityId::mint("synthetic:test:id#second").unwrap(),
        })
    );
    let mut arc_marker = marker("arc-marker", None);
    arc_marker.reclassify(SketchInputKind::Arc);
    let mut arc_loci = midpoint_loci.clone();
    arc_loci.insert(
        arc_marker.id().to_string(),
        vec![cadmpeg_ir::sketches::SketchLocus::Entity(
            SketchEntityId::mint("synthetic:test:id#second").unwrap(),
        )],
    );
    markers.insert(arc_marker.id(), &arc_marker);
    for (kind, angle) in [
        (SketchRelationKind::ArcAngle90, std::f64::consts::FRAC_PI_2),
        (SketchRelationKind::ArcAngle180, std::f64::consts::PI),
        (
            SketchRelationKind::ArcAngle270,
            3.0 * std::f64::consts::FRAC_PI_2,
        ),
    ] {
        let mut arc_angle = marker("arc-angle", None);
        arc_angle.reclassify(SketchInputKind::Relation(kind));
        arc_angle.links = crate::records::SketchInputLinks::new(
            0,
            vec![SketchInputLink {
                local_id: 1,
                entity_ref: arc_marker.id().to_string(),
            }],
        );
        assert_eq!(
            typed_marker_relation_definition(&arc_angle, &markers, &arc_loci),
            Some(SketchConstraintDefinitionInput::ArcAngle {
                entity: SketchEntityId::mint("synthetic:test:id#second").unwrap(),
                angle: cadmpeg_ir::scalar::Angle::new(angle).unwrap(),
            })
        );
        arc_angle.links.as_mut().unwrap().entries_mut()[0].entity_ref =
            entity_marker.id().to_string();
        assert!(matches!(
            typed_marker_relation_definition(&arc_angle, &markers, &arc_loci),
            Some(SketchConstraintDefinitionInput::Native {
                native_kind,
                entities,
                parameter: None,
                operands,
            ..
            }) if native_kind.as_str() == format!("sldprt:marker-relation:{}", kind.native_code())
                && entities == vec![SketchEntityId::mint("synthetic:test:id#second").unwrap()]
                && operands.len() == 1
                && operands[0].object_index == Some(1)
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
        scalars: crate::records::relation_scalars::RelationScalars::from_refs(
            Vec::new(),
            None,
            None,
        )
        .unwrap(),
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
        id: ParameterId::mint(id).expect("identity grammar"),
        owner: Some(FeatureId::mint("synthetic:test:id#feature").expect("identity grammar")),
        ordinal: 0,
        name: id.into(),
        expression: String::new(),
        display,
        value: Some(ParameterValue::Length(Length::new(2.0).unwrap())),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let sketch_id = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let distance = parameter("synthetic:test:id#distance", None);
    assert!(matches!(
        typed_relation_definition(
            &relation,
            Some(&distance),
            &sketch_id,
            &[],
            &markers,
            &joins,
        ),
        Some(cadmpeg_ir::sketches::SketchConstraintDefinitionInput::DistanceLoci {
            parameter,
            ..
        }) if parameter.as_str() == "synthetic:test:id#distance"
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
    let radius = parameter("synthetic:test:id#circle", Some(DimensionDisplay::Radius));
    assert!(matches!(
        typed_relation_definition(
            &circle,
            Some(&radius),
            &sketch_id,
            &[],
            &markers,
            &joins,
        ),
        Some(SketchConstraintDefinitionInput::Radius { parameter, .. })
            if parameter.as_str() == "synthetic:test:id#circle"
    ));
    let diameter = parameter("synthetic:test:id#circle", Some(DimensionDisplay::Diameter));
    assert!(matches!(
        typed_relation_definition(
            &circle,
            Some(&diameter),
            &sketch_id,
            &[],
            &markers,
            &joins,
        ),
        Some(SketchConstraintDefinitionInput::Diameter { parameter, .. })
            if parameter.as_str() == "synthetic:test:id#circle"
    ));
    let undisplayed = parameter("synthetic:test:id#circle", None);
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
        SketchEntityId::mint("synthetic:test:id#dimensioned-circle").unwrap(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(2.0).unwrap(),
        })
        .unwrap(),
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
        Some(SketchConstraintDefinitionInput::Radius { entity, .. })
            if entity == circle_entity.id().clone()
    ));
    let duplicate_circle = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#duplicate-circle").unwrap(),
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
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let line_ids = [
        "synthetic:test:id#horizontal",
        "synthetic:test:id#vertical",
        "synthetic:test:id#offset",
    ]
    .map(|id| SketchEntityId::mint(id).unwrap());
    let entities = vec![
        SketchEntity::new(
            line_ids[0].clone(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(10.0, 0.0),
            })
            .unwrap(),
        ),
        SketchEntity::new(
            line_ids[1].clone(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(0.0, 20.0),
            })
            .unwrap(),
        ),
        SketchEntity::new(
            line_ids[2].clone(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(10.0, 3.0),
                end: Point2::new(20.0, 3.0),
            })
            .unwrap(),
        ),
    ];
    let feature = Feature {
        id: FeatureId::mint("synthetic:test:id#feature").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }),
        ),
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
        handle = handle.with_test_position(ordinal as u32, handle.offset());
        handle = handle.with_test_position(handle.ordinal(), offset as u64);
        handle.reclassify(SketchInputKind::LineOrCircle);
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
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let first_id = SketchEntityId::mint("synthetic:test:id#first").unwrap();
    let second_id = SketchEntityId::mint("synthetic:test:id#second").unwrap();
    let first = SketchEntity::new(
        first_id.clone(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    );
    let second = SketchEntity::new(
        second_id.clone(),
        sketch,
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(1.0, 0.0),
            end: Point2::new(1.0, 1.0),
        })
        .unwrap(),
    );
    let mut first_marker = marker("first-marker", Some([0.0, 0.0]));
    first_marker.reclassify(SketchInputKind::LineOrCircle);
    let mut second_marker = marker("second-marker", Some([0.0, 0.0]));
    second_marker.reclassify(SketchInputKind::LineOrCircle);
    let mut point = marker("point", None);
    point.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 1,
                entity_ref: first_marker.id().to_string(),
            },
            SketchInputLink {
                local_id: 2,
                entity_ref: second_marker.id().to_string(),
            },
        ],
    );
    let markers = HashMap::from([
        (first_marker.id(), &first_marker),
        (second_marker.id(), &second_marker),
        (point.id(), &point),
    ]);
    let loci = HashMap::from([
        (
            first_marker.id().to_string(),
            vec![SketchLocus::Entity(first_id.clone())],
        ),
        (
            second_marker.id().to_string(),
            vec![SketchLocus::Entity(second_id.clone())],
        ),
    ]);
    let entities = HashMap::from([(first.id(), &first), (second.id(), &second)]);

    assert_eq!(
        unique_linked_endpoint_locus(&point, &markers, &loci, &entities, 1.0e-8),
        Some(SketchLocus::End(first_id))
    );

    let mut ambiguous = second;
    ambiguous.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .unwrap();
    let entities = HashMap::from([(first.id(), &first), (ambiguous.id(), &ambiguous)]);
    assert_eq!(
        unique_linked_endpoint_locus(&point, &markers, &loci, &entities, 1.0e-8),
        None
    );
}

#[test]
fn curve_handles_reject_point_geometry() {
    let point = SketchGeometry::try_from(SketchGeometryDefinition::Point {
        position: Point2::new(0.0, 0.0),
    })
    .unwrap();
    let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .unwrap();
    let circle = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(0.0, 0.0),
        radius: Length::new(1.0).unwrap(),
    })
    .unwrap();

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
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let circle = SketchEntityId::mint("synthetic:test:id#circle").unwrap();
    let entity = SketchEntity::new(
        circle.clone(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(10.0).unwrap(),
        })
        .unwrap(),
    );
    let points = [-10.0, 10.0].map(|u| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#point-{u}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, 0.0),
            })
            .unwrap(),
        )
        .with_construction(true)
    });
    let feature = Feature {
        id: FeatureId::mint("synthetic:test:id#feature").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }),
        ),
        native_ref: Some("feature-native".into()),
    };
    let mut native_payload = vec![0; 54];
    for offset in [0, 27] {
        native_payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    }
    let mut handle = marker("circle-marker", Some([0.0, 0.0]));
    handle.reclassify(SketchInputKind::LineOrCircle);
    let mut point = marker("point-marker", Some([0.01, 0.0]));
    point = point.with_test_position(1, point.offset());
    point = point.with_test_position(point.ordinal(), 27);
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
