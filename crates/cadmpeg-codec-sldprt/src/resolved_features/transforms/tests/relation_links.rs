//! Relation link identity and inactive-constraint tests.

use super::super::*;
use super::marker;
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputScalar,
    FeatureInputScalarRole, SketchInputKind, SketchInputLink, SketchRelationKind,
};
use cadmpeg_ir::features::Angle;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchCoordinateAxis, SketchEntity, SketchEntityId, SketchGeometry,
    SketchId, SketchLocus, SketchNativeOperand,
};
use std::collections::HashMap;

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
            local_id: 7,
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
fn axis_relation_accepts_two_forward_points_through_identity_collisions() {
    let sketch = SketchId("sketch".into());
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    relation.local_id = Some(7);
    relation.object_index = Some(8);
    relation.links = vec![
        SketchInputLink {
            local_id: 7,
            entity_ref: "first-point".into(),
        },
        SketchInputLink {
            local_id: 8,
            entity_ref: "second-point".into(),
        },
    ];
    let first = marker("first-point", Some([0.0, 0.0]));
    let second = marker("second-point", Some([1.0, 0.0]));
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (first.id.as_str(), &first),
        (second.id.as_str(), &second),
    ]);
    let first_entity = SketchEntity {
        id: SketchEntityId("first-entity".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(first.id.clone()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    };
    let second_entity = SketchEntity {
        id: SketchEntityId("second-entity".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(second.id.clone()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(1.0, 0.0),
        },
    };
    let entities = vec![first_entity.clone(), second_entity.clone()];

    assert!(marker_owns_constraint(&relation, &markers));
    assert_eq!(
        typed_marker_relation_definition_in_sketch(
            &relation,
            &sketch,
            &entities,
            &markers,
            &HashMap::new(),
        ),
        Some(SketchConstraintDefinition::SameCoordinate {
            first: SketchLocus::Entity(first_entity.id),
            second: SketchLocus::Entity(second_entity.id),
            axis: SketchCoordinateAxis::V,
        })
    );
}

#[test]
fn object_index_collision_remains_a_forward_curve_operand() {
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    relation.local_id = Some(2);
    relation.object_index = Some(1);
    relation.links = vec![SketchInputLink {
        local_id: 1,
        entity_ref: "line".into(),
    }];
    let mut line = marker("line", None);
    line.kind = SketchInputKind::LineOrCircle;
    line.local_id = Some(1);
    let markers = HashMap::from([(relation.id.as_str(), &relation), (line.id.as_str(), &line)]);
    let loci = HashMap::from([(
        line.id.clone(),
        vec![SketchLocus::Entity(SketchEntityId("line-entity".into()))],
    )]);

    assert_eq!(
        typed_marker_relation_definition(&relation, &markers, &loci),
        Some(SketchConstraintDefinition::Horizontal {
            entity: SketchEntityId("line-entity".into()),
        })
    );
}

#[test]
fn self_identifying_forward_curve_link_is_excluded_from_arc_relation() {
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::ArcAngle90);
    relation.local_id = Some(7);
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
    relation.local_id = Some(1);
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
    for tag in [0x8100, 0x820f] {
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
            kind: FeatureInputOperandKind::Native(tag),
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
        kind: FeatureInputOperandKind::Native(0x8100),
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
