//! Unary, binary, axis, and line operand resolution tests.

use super::super::*;
use super::marker;
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputKind, SketchInputLink, SketchRelationKind,
};
use cadmpeg_ir::features::{
    DesignParameter, Feature, FeatureDefinition, FeatureId, Length, ParameterId, ParameterValue,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraintDefinition, SketchCoordinateAxis, SketchEntity, SketchEntityId,
    SketchGeometry, SketchId, SketchLocus,
};
use std::collections::{BTreeMap, HashMap};

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
fn point_relation_ignores_auxiliary_relation_links() {
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    relation.links = vec![SketchInputLink {
        local_id: 1,
        entity_ref: "radius".into(),
    }];
    let mut radius = marker("radius", None);
    radius.kind = SketchInputKind::Relation(SketchRelationKind::Radius);
    let mut first = marker("first", Some([0.0, 1.0]));
    first.offset = 1;
    first.links = vec![SketchInputLink {
        local_id: 4,
        entity_ref: relation.id.clone(),
    }];
    let mut second = marker("second", Some([1.0, 1.0]));
    second.offset = 2;
    second.links = first.links.clone();
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (radius.id.as_str(), &radius),
        (first.id.as_str(), &first),
        (second.id.as_str(), &second),
    ]);
    let loci = HashMap::from([
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
        typed_marker_relation_definition(&relation, &markers, &loci),
        Some(SketchConstraintDefinition::SameCoordinate {
            first: SketchLocus::Entity(SketchEntityId("first-point".into())),
            second: SketchLocus::Entity(SketchEntityId("second-point".into())),
            axis: SketchCoordinateAxis::V,
        })
    );
}

#[test]
fn axis_relation_expands_intermediate_relation_handle() {
    let mut first = marker("first-point", Some([0.0, 1.0]));
    first.offset = 1;
    let mut second = marker("second-point", Some([2.0, 1.0]));
    second.offset = 2;
    let mut distance = marker("distance-handle", None);
    distance.kind = SketchInputKind::Relation(SketchRelationKind::Distance);
    distance.local_id = Some(5);
    distance.object_index = Some(4);
    distance.links = vec![
        SketchInputLink {
            local_id: 5,
            entity_ref: distance.id.clone(),
        },
        SketchInputLink {
            local_id: 7,
            entity_ref: second.id.clone(),
        },
    ];
    let mut reverse_owner = marker("reverse-owner", Some([3.0, 4.0]));
    reverse_owner.offset = 3;
    reverse_owner.links = vec![SketchInputLink {
        local_id: 9,
        entity_ref: distance.id.clone(),
    }];
    let mut horizontal = marker("horizontal", None);
    horizontal.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    horizontal.local_id = Some(13);
    horizontal.object_index = Some(12);
    horizontal.links = vec![
        SketchInputLink {
            local_id: 8,
            entity_ref: first.id.clone(),
        },
        SketchInputLink {
            local_id: 5,
            entity_ref: distance.id.clone(),
        },
    ];
    let markers = HashMap::from([
        (first.id.as_str(), &first),
        (second.id.as_str(), &second),
        (distance.id.as_str(), &distance),
        (reverse_owner.id.as_str(), &reverse_owner),
        (horizontal.id.as_str(), &horizontal),
    ]);
    let loci = HashMap::from([
        (
            first.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId("first-point".into()))],
        ),
        (
            second.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId("second-point".into()))],
        ),
        (
            reverse_owner.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId(
                "reverse-owner-point".into(),
            ))],
        ),
    ]);

    assert_eq!(
        typed_marker_relation_definition(&horizontal, &markers, &loci),
        Some(SketchConstraintDefinition::SameCoordinate {
            first: SketchLocus::Entity(SketchEntityId("first-point".into())),
            second: SketchLocus::Entity(SketchEntityId("second-point".into())),
            axis: SketchCoordinateAxis::V,
        })
    );

    let sketch = SketchId("axis-sketch".into());
    let entities = vec![
        SketchEntity {
            id: SketchEntityId("first-entity".into()),
            sketch: sketch.clone(),
            construction: true,
            native_ref: Some(first.id.clone()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(0.0, 1.0),
            },
        },
        SketchEntity {
            id: SketchEntityId("second-entity".into()),
            sketch: sketch.clone(),
            construction: true,
            native_ref: Some(second.id.clone()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(2.0, 1.0),
            },
        },
    ];
    assert_eq!(
        typed_marker_relation_definition_in_sketch(
            &horizontal,
            &sketch,
            &entities,
            &markers,
            &HashMap::new(),
        ),
        Some(SketchConstraintDefinition::SameCoordinate {
            first: SketchLocus::Entity(SketchEntityId("first-entity".into())),
            second: SketchLocus::Entity(SketchEntityId("second-entity".into())),
            axis: SketchCoordinateAxis::V,
        })
    );
    let mut ambiguous_entities = entities.clone();
    ambiguous_entities.push(SketchEntity {
        id: SketchEntityId("second-duplicate".into()),
        ..ambiguous_entities[1].clone()
    });
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &horizontal,
            &sketch,
            &ambiguous_entities,
            &markers,
            &HashMap::new(),
        ),
        Some(SketchConstraintDefinition::Native { .. })
    ));
}

#[test]
fn axis_relation_prefers_forward_points_over_reverse_owners() {
    let sketch = SketchId("axis-sketch".into());
    let first = marker("first-point", Some([0.0, 1.0]));
    let mut second = marker("second-point", Some([2.0, 1.0]));
    second.offset = 1;
    let mut horizontal = marker("horizontal", None);
    horizontal.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    horizontal.offset = 2;
    horizontal.links = vec![
        SketchInputLink {
            local_id: 8,
            entity_ref: first.id.clone(),
        },
        SketchInputLink {
            local_id: 9,
            entity_ref: second.id.clone(),
        },
    ];
    let mut reverse_first = marker("reverse-first", Some([3.0, 4.0]));
    reverse_first.kind = SketchInputKind::Point;
    reverse_first.offset = 3;
    reverse_first.links = vec![SketchInputLink {
        local_id: 10,
        entity_ref: horizontal.id.clone(),
    }];
    let mut reverse_second = marker("reverse-second", Some([5.0, 6.0]));
    reverse_second.kind = SketchInputKind::Point;
    reverse_second.offset = 4;
    reverse_second.links = reverse_first.links.clone();
    let markers = HashMap::from([
        (first.id.as_str(), &first),
        (second.id.as_str(), &second),
        (horizontal.id.as_str(), &horizontal),
        (reverse_first.id.as_str(), &reverse_first),
        (reverse_second.id.as_str(), &reverse_second),
    ]);
    let first_entity = SketchEntity {
        id: SketchEntityId("first-entity".into()),
        sketch: sketch.clone(),
        construction: true,
        native_ref: Some(first.id.clone()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(0.0, 1.0),
        },
    };
    let second_entity = SketchEntity {
        id: SketchEntityId("second-entity".into()),
        sketch: sketch.clone(),
        construction: true,
        native_ref: Some(second.id.clone()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(2.0, 1.0),
        },
    };
    let entities = [first_entity.clone(), second_entity.clone()];
    let loci = HashMap::from([
        (
            first.id.clone(),
            vec![SketchLocus::Entity(first_entity.id.clone())],
        ),
        (
            second.id.clone(),
            vec![SketchLocus::Entity(second_entity.id.clone())],
        ),
    ]);

    assert_eq!(
        typed_marker_relation_definition_in_sketch(
            &horizontal,
            &sketch,
            &entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::SameCoordinate {
            first: SketchLocus::Entity(first_entity.id),
            second: SketchLocus::Entity(second_entity.id),
            axis: SketchCoordinateAxis::V,
        })
    );
}

#[test]
fn axis_relation_resolves_a_point_proxy_despite_an_index_collision() {
    let sketch = SketchId("sketch".into());
    let first_id = SketchEntityId("first-entity".into());
    let second_id = SketchEntityId("second-entity".into());
    let mut first = marker("first", Some([0.0, 0.0]));
    first.kind = SketchInputKind::Point;
    let mut proxy = marker("proxy", None);
    proxy.kind = SketchInputKind::Point;
    let mut relation = marker("horizontal", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    relation.object_index = Some(4);
    relation.links = vec![
        SketchInputLink {
            local_id: 4,
            entity_ref: first.id.clone(),
        },
        SketchInputLink {
            local_id: 1,
            entity_ref: proxy.id.clone(),
        },
    ];
    let markers = HashMap::from([
        (first.id.as_str(), &first),
        (proxy.id.as_str(), &proxy),
        (relation.id.as_str(), &relation),
    ]);
    let second_locus = SketchLocus::Entity(second_id.clone());
    let loci = HashMap::from([(proxy.id.clone(), vec![second_locus.clone()])]);
    let point = |id, native_ref, position| SketchEntity {
        id,
        sketch: sketch.clone(),
        construction: true,
        native_ref,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let entities = vec![
        point(
            first_id.clone(),
            Some(first.id.clone()),
            Point2::new(0.0, 0.0),
        ),
        point(second_id, None, Point2::new(1.0, 2.0)),
    ];

    let definition =
        typed_marker_relation_definition_in_sketch(&relation, &sketch, &entities, &markers, &loci)
            .expect("typed horizontal point relation");

    assert_eq!(
        definition,
        SketchConstraintDefinition::SameCoordinate {
            first: SketchLocus::Entity(first_id),
            second: second_locus,
            axis: SketchCoordinateAxis::V,
        }
    );
    assert!(marker_relation_is_inactive(
        &relation,
        &definition,
        &entities,
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
    let entities = [first_line, second_line];
    let definition =
        typed_marker_relation_definition_in_sketch(&relation, &sketch, &entities, &markers, &loci)
            .expect("typed parallel relation");
    assert!(matches!(
        definition,
        SketchConstraintDefinition::Parallel { .. }
    ));
    assert!(marker_relation_is_inactive(
        &relation,
        &definition,
        &entities,
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
        visible: None,
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
        visible: None,
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
        visible: None,
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
fn line_operand_rejects_a_circular_geometry_alias() {
    let sketch = SketchId("sketch".into());
    let line_id = SketchEntityId("line".into());
    let circle_id = SketchEntityId("circle".into());
    let entities = vec![
        SketchEntity {
            id: line_id.clone(),
            sketch: sketch.clone(),
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
        single_marker_line_entity("circle-marker", &HashMap::new(), &loci, &entities),
        None
    );
    assert_eq!(
        single_marker_line_entity("line-marker", &HashMap::new(), &loci, &entities),
        Some(line_id)
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
        SketchConstraintDefinition::SameCoordinate {
            axis: SketchCoordinateAxis::V,
            ..
        }
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
        SketchConstraintDefinition::SameCoordinate {
            axis: SketchCoordinateAxis::V,
            ..
        }
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
        SketchConstraintDefinition::SameCoordinate {
            axis: SketchCoordinateAxis::V,
            ..
        }
    ));
    assert!(marker_relation_is_inactive(
        &owner_relation,
        &definition,
        &owner_entities
    ));
}

#[test]
fn axis_relation_uses_unique_point_native_identity_when_loci_are_ambiguous() {
    let sketch = SketchId("sketch".into());
    let mut first = marker("first-point", Some([0.0, 0.01]));
    first.kind = SketchInputKind::Point;
    let mut second = marker("second-point", Some([0.02, 0.01]));
    second.kind = SketchInputKind::Point;
    let mut relation = marker("horizontal", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    relation.local_id = Some(7);
    relation.object_index = Some(6);
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
    let first_entity = SketchEntity {
        id: SketchEntityId("first-entity".into()),
        sketch: sketch.clone(),
        construction: true,
        native_ref: Some(first.id.clone()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(0.0, 10.0),
        },
    };
    let second_entity = SketchEntity {
        id: SketchEntityId("second-entity".into()),
        sketch: sketch.clone(),
        construction: true,
        native_ref: Some(second.id.clone()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(20.0, 10.0),
        },
    };
    let entities = vec![first_entity.clone(), second_entity.clone()];
    let definition = typed_marker_relation_definition_in_sketch(
        &relation,
        &sketch,
        &entities,
        &markers,
        &HashMap::new(),
    )
    .expect("typed horizontal point relation");
    assert_eq!(
        definition,
        SketchConstraintDefinition::SameCoordinate {
            first: SketchLocus::Entity(first_entity.id.clone()),
            second: SketchLocus::Entity(second_entity.id.clone()),
            axis: SketchCoordinateAxis::V,
        }
    );
    assert!(!marker_relation_is_inactive(
        &relation,
        &definition,
        &entities
    ));

    let mut ambiguous_entities = entities.clone();
    ambiguous_entities.push(first_entity);
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &relation,
            &sketch,
            &ambiguous_entities,
            &markers,
            &HashMap::new(),
        ),
        Some(SketchConstraintDefinition::Native { .. })
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
