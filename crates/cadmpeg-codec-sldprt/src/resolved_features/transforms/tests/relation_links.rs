//! Relation link identity and inactive-constraint tests.

use super::super::*;
use super::marker;
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputScalar,
    FeatureInputScalarRole, SketchInputKind, SketchInputLink, SketchRelationKind,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::Angle;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntity, SketchEntityId,
    SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus, SketchNativeOperand,
};
use std::collections::HashMap;

#[test]
fn coordinate_curve_links_carry_reverse_constraint_incidence() {
    let mut relation = marker("relation", None);
    relation.reclassify(SketchInputKind::Relation(SketchRelationKind::Horizontal));
    let mut owner = marker("owner", Some([1.0, 2.0]));
    owner.reclassify(SketchInputKind::LineOrCircle);
    owner = owner.with_test_identity(Some(7), owner.local_id());
    owner = owner.with_test_position(owner.ordinal(), 1);
    owner.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 4,
            entity_ref: relation.id().to_string(),
        }],
    );
    let mut point = marker("point", Some([1.0, 2.0]));
    point = point.with_test_identity(Some(8), point.local_id());
    point = point.with_test_position(point.ordinal(), 2);
    point.links = owner.links.clone();
    let markers = HashMap::from([
        (relation.id(), &relation),
        (owner.id(), &owner),
        (point.id(), &point),
    ]);

    assert_eq!(
        relation_owner_markers(&relation, &markers),
        vec![&owner, &point]
    );
    let Some(SketchConstraintDefinitionInput::Native { operands, .. }) =
        typed_marker_relation_definition(&relation, &markers, &HashMap::new())
    else {
        panic!("native relation");
    };
    assert_eq!(
        operands,
        vec![
            SketchNativeOperand {
                native_kind: cadmpeg_ir::products::NonBlankString::new(
                    "sldprt:marker-constraint-owner"
                )
                .expect("source operand kind is nonempty"),
                field: None,
                object_index: Some(7),
                native_ref: Some(owner.id().to_string()),
            },
            SketchNativeOperand {
                native_kind: cadmpeg_ir::products::NonBlankString::new(
                    "sldprt:marker-constraint-owner"
                )
                .expect("source operand kind is nonempty"),
                field: None,
                object_index: Some(8),
                native_ref: Some(point.id().to_string()),
            },
        ]
    );
}

#[test]
fn self_link_does_not_make_a_relation_operand_bearing() {
    let mut relation = marker("relation", Some([0.0, 0.0]));
    relation.reclassify(SketchInputKind::Relation(SketchRelationKind::Perpendicular));
    relation.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 0,
            entity_ref: relation.id().to_string(),
        }],
    );
    let markers = HashMap::from([(relation.id(), &relation)]);

    assert!(!marker_owns_constraint(&relation, &markers));
    assert_eq!(
        typed_marker_relation_definition(&relation, &markers, &HashMap::new()),
        None
    );

    let mut collision = marker("collision", Some([1.0, 0.0]));
    collision = collision.with_test_identity(collision.object_index(), Some(7));
    let mut relation = marker("relation", None);
    relation.reclassify(SketchInputKind::Relation(SketchRelationKind::Tangent));
    relation = relation.with_test_identity(relation.object_index(), Some(7));
    relation = relation.with_test_identity(Some(8), relation.local_id());
    relation.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 7,
                entity_ref: collision.id().to_string(),
            },
            SketchInputLink {
                local_id: 7,
                entity_ref: collision.id().to_string(),
            },
        ],
    );
    let markers = HashMap::from([(relation.id(), &relation), (collision.id(), &collision)]);

    assert!(!marker_owns_constraint(&relation, &markers));
    assert_eq!(
        typed_marker_relation_definition(&relation, &markers, &HashMap::new()),
        None
    );
}

#[test]
fn axis_relation_accepts_two_forward_points_through_identity_collisions() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let mut relation = marker("relation", None);
    relation.reclassify(SketchInputKind::Relation(SketchRelationKind::Horizontal));
    relation = relation.with_test_identity(relation.object_index(), Some(7));
    relation = relation.with_test_identity(Some(8), relation.local_id());
    relation.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 7,
                entity_ref: "first-point".into(),
            },
            SketchInputLink {
                local_id: 8,
                entity_ref: "second-point".into(),
            },
        ],
    );
    let first = marker("first-point", Some([0.0, 0.0]));
    let second = marker("second-point", Some([1.0, 0.0]));
    let markers = HashMap::from([
        (relation.id(), &relation),
        (first.id(), &first),
        (second.id(), &second),
    ]);
    let first_entity = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#first-entity").unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    )
    .with_native_ref(Some(first.id().to_string()));
    let second_entity = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#second-entity").unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    )
    .with_native_ref(Some(second.id().to_string()));
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
        Some(SketchConstraintDefinitionInput::SameCoordinate {
            relation: cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                SketchLocus::Entity(first_entity.id().clone()),
                SketchLocus::Entity(second_entity.id().clone()),
                SketchCoordinateAxis::V
            )
            .unwrap()
        })
    );
}

#[test]
fn object_index_collision_remains_a_forward_curve_operand() {
    let mut relation = marker("relation", None);
    relation.reclassify(SketchInputKind::Relation(SketchRelationKind::Horizontal));
    relation = relation.with_test_identity(relation.object_index(), Some(2));
    relation = relation.with_test_identity(Some(1), relation.local_id());
    relation.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 1,
            entity_ref: "line".into(),
        }],
    );
    let mut line = marker("line", None);
    line.reclassify(SketchInputKind::LineOrCircle);
    line = line.with_test_identity(line.object_index(), Some(1));
    let markers = HashMap::from([(relation.id(), &relation), (line.id(), &line)]);
    let loci = HashMap::from([(
        line.id().to_string(),
        vec![SketchLocus::Entity(
            SketchEntityId::mint("synthetic:test:id#line-entity").unwrap(),
        )],
    )]);

    assert_eq!(
        typed_marker_relation_definition(&relation, &markers, &loci),
        Some(SketchConstraintDefinitionInput::Horizontal {
            entity: SketchEntityId::mint("synthetic:test:id#line-entity").unwrap(),
        })
    );
}

#[test]
fn self_identifying_forward_curve_link_is_excluded_from_arc_relation() {
    let mut relation = marker("relation", None);
    relation.reclassify(SketchInputKind::Relation(SketchRelationKind::ArcAngle90));
    relation = relation.with_test_identity(relation.object_index(), Some(7));
    relation = relation.with_test_identity(Some(7), relation.local_id());
    relation.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 7,
                entity_ref: "ignored-arc".into(),
            },
            SketchInputLink {
                local_id: 9,
                entity_ref: "operand-arc".into(),
            },
        ],
    );
    let mut ignored_arc = marker("ignored-arc", None);
    ignored_arc.reclassify(SketchInputKind::Arc);
    let mut operand_arc = marker("operand-arc", None);
    operand_arc.reclassify(SketchInputKind::Arc);
    let markers = HashMap::from([
        (relation.id(), &relation),
        (ignored_arc.id(), &ignored_arc),
        (operand_arc.id(), &operand_arc),
    ]);
    let loci = HashMap::from([
        (
            ignored_arc.id().to_string(),
            vec![SketchLocus::Entity(
                SketchEntityId::mint("synthetic:test:id#ignored-entity").unwrap(),
            )],
        ),
        (
            operand_arc.id().to_string(),
            vec![SketchLocus::Entity(
                SketchEntityId::mint("synthetic:test:id#operand-entity").unwrap(),
            )],
        ),
    ]);

    assert_eq!(
        typed_marker_relation_definition(&relation, &markers, &loci),
        Some(SketchConstraintDefinitionInput::ArcAngle {
            entity: SketchEntityId::mint("synthetic:test:id#operand-entity").unwrap(),
            angle: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        })
    );
}

#[test]
fn self_identifying_forward_link_is_not_a_relation_locus() {
    let mut relation = marker("relation", None);
    relation.reclassify(SketchInputKind::Relation(SketchRelationKind::Vertical));
    relation = relation.with_test_identity(relation.object_index(), Some(1));
    relation = relation.with_test_identity(Some(1), relation.local_id());
    relation.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 1,
            entity_ref: "center".into(),
        }],
    );
    let mut center = marker("center", Some([0.0, 1.0]));
    center.reclassify(SketchInputKind::Arc);
    let mut first = marker("first", Some([-1.0, 0.0]));
    first = first.with_test_position(first.ordinal(), 1);
    first.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 3,
            entity_ref: relation.id().to_string(),
        }],
    );
    let mut second = marker("second", Some([1.0, 0.0]));
    second = second.with_test_position(second.ordinal(), 2);
    second.links = first.links.clone();
    let markers = HashMap::from([
        (relation.id(), &relation),
        (center.id(), &center),
        (first.id(), &first),
        (second.id(), &second),
    ]);
    let loci = HashMap::from([
        (
            center.id().to_string(),
            vec![SketchLocus::Center(
                SketchEntityId::mint("synthetic:test:id#arc").unwrap(),
            )],
        ),
        (
            first.id().to_string(),
            vec![SketchLocus::Entity(
                SketchEntityId::mint("synthetic:test:id#first-point").unwrap(),
            )],
        ),
        (
            second.id().to_string(),
            vec![SketchLocus::Entity(
                SketchEntityId::mint("synthetic:test:id#second-point").unwrap(),
            )],
        ),
    ]);

    assert_eq!(
        relation_operand_loci(&relation, &markers, &loci),
        Some(vec![
            SketchLocus::Entity(SketchEntityId::mint("synthetic:test:id#first-point").unwrap()),
            SketchLocus::Entity(SketchEntityId::mint("synthetic:test:id#second-point").unwrap()),
        ])
    );
}

#[test]
fn native_fallback_entities_exclude_self_identity_collisions() {
    let mut relation = marker("relation", None);
    relation.reclassify(SketchInputKind::Relation(SketchRelationKind::Horizontal));
    relation = relation.with_test_identity(relation.object_index(), Some(3));
    relation.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 3,
                entity_ref: "collision".into(),
            },
            SketchInputLink {
                local_id: 4,
                entity_ref: "operand".into(),
            },
        ],
    );
    let collision = marker("collision", Some([0.0, 0.0]));
    let operand = marker("operand", Some([1.0, 0.0]));
    let markers = HashMap::from([
        (relation.id(), &relation),
        (collision.id(), &collision),
        (operand.id(), &operand),
    ]);
    let loci = HashMap::from([
        (
            collision.id().to_string(),
            vec![SketchLocus::Entity(
                SketchEntityId::mint("synthetic:test:id#collision").unwrap(),
            )],
        ),
        (
            operand.id().to_string(),
            vec![SketchLocus::Entity(
                SketchEntityId::mint("synthetic:test:id#operand").unwrap(),
            )],
        ),
    ]);

    let Some(SketchConstraintDefinitionInput::Native {
        entities, operands, ..
    }) = typed_marker_relation_definition(&relation, &markers, &loci)
    else {
        panic!("native fallback");
    };
    assert_eq!(
        entities,
        [SketchEntityId::mint("synthetic:test:id#operand").unwrap()]
    );
    assert_eq!(operands.len(), 2);
}

#[test]
fn exact_curve_identity_precedes_incident_locus_expansion() {
    let mut relation = marker("relation", None);
    relation.reclassify(SketchInputKind::Relation(SketchRelationKind::Vertical));
    relation.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 3,
            entity_ref: "curve-marker".into(),
        }],
    );
    let mut curve = marker("curve-marker", Some([1.0, 1.0]));
    curve.reclassify(SketchInputKind::LineOrCircle);
    let markers = HashMap::from([(relation.id(), &relation), (curve.id(), &curve)]);
    let exact = SketchEntityId::mint("synthetic:test:id#exact").unwrap();
    let incident = SketchEntityId::mint("synthetic:test:id#incident").unwrap();
    let loci = HashMap::from([(
        curve.id().to_string(),
        vec![
            SketchLocus::Start(exact.clone()),
            SketchLocus::End(incident.clone()),
        ],
    )]);
    let entity = |id: SketchEntityId, native_ref: Option<&str>, start, end| {
        SketchEntity::new(
            id,
            SketchId::mint("synthetic:test:id#sketch").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
        .with_native_ref(native_ref.map(str::to_string))
    };
    let entities = vec![
        entity(
            exact.clone(),
            Some(curve.id()),
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 2.0),
        ),
        entity(incident, None, Point2::new(0.0, 0.0), Point2::new(1.0, 2.0)),
    ];

    assert_eq!(
        typed_marker_relation_definition_in_sketch(
            &relation,
            &SketchId::mint("synthetic:test:id#sketch").unwrap(),
            &entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinitionInput::Vertical { entity: exact })
    );
}

#[test]
fn fixed_relation_selects_one_geometry_operand_beside_auxiliary_relation_handles() {
    let mut relation = marker("fixed", None);
    relation.reclassify(SketchInputKind::Relation(SketchRelationKind::Fixed));
    relation.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 2,
                entity_ref: "point".into(),
            },
            SketchInputLink {
                local_id: 7,
                entity_ref: "radius".into(),
            },
        ],
    );
    let mut point = marker("point", Some([1.0, 2.0]));
    point.reclassify(SketchInputKind::Point);
    let mut radius = marker("radius", None);
    radius.reclassify(SketchInputKind::Relation(SketchRelationKind::Radius));
    let markers = HashMap::from([
        (relation.id(), &relation),
        (point.id(), &point),
        (radius.id(), &radius),
    ]);
    let point_id = SketchEntityId::mint("synthetic:test:id#point-entity").unwrap();
    let loci = HashMap::from([(
        point.id().to_string(),
        vec![SketchLocus::Entity(point_id.clone())],
    )]);
    let point_entity = SketchEntity::new(
        point_id.clone(),
        SketchId::mint("synthetic:test:id#sketch").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(1.0, 2.0),
        })
        .unwrap(),
    )
    .with_native_ref(Some(point.id().to_string()));

    assert_eq!(
        typed_marker_relation_definition_in_sketch(
            &relation,
            &SketchId::mint("synthetic:test:id#sketch").unwrap(),
            std::slice::from_ref(&point_entity),
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinitionInput::Fixed {
            entity: point_id.clone(),
        })
    );

    let mut second = marker("second", Some([3.0, 4.0]));
    second.reclassify(SketchInputKind::Point);
    relation.links = crate::records::SketchInputLinks::new(
        0,
        relation
            .links()
            .iter()
            .cloned()
            .chain(std::iter::once(SketchInputLink {
                local_id: 8,
                entity_ref: second.id().to_string(),
            }))
            .collect(),
    );
    let markers = HashMap::from([
        (relation.id(), &relation),
        (point.id(), &point),
        (radius.id(), &radius),
        (second.id(), &second),
    ]);
    let loci = HashMap::from([
        (
            point.id().to_string(),
            vec![SketchLocus::Entity(point_id.clone())],
        ),
        (
            second.id().to_string(),
            vec![SketchLocus::Entity(
                SketchEntityId::mint("synthetic:test:id#second-entity").unwrap(),
            )],
        ),
    ]);
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &relation,
            &SketchId::mint("synthetic:test:id#sketch").unwrap(),
            &[point_entity],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinitionInput::Native { .. })
    ));
}

#[test]
fn resolved_wrong_family_relation_is_inactive() {
    let mut relation = marker("relation", None);
    relation.reclassify(SketchInputKind::Relation(
        SketchRelationKind::EllipseAngle180,
    ));
    let entity_id = SketchEntityId::mint("synthetic:test:id#line").unwrap();
    let definition = SketchConstraintDefinitionInput::Native {
        native_kind: cadmpeg_ir::products::NonBlankString::new("sldprt:marker-relation:34")
            .unwrap(),
        native_state: None,
        native_flags: None,
        native_properties: std::collections::BTreeMap::new(),
        entities: vec![entity_id.clone()],
        parameter: None,
        operands: Vec::new(),
    };
    let entities = vec![SketchEntity::new(
        entity_id,
        SketchId::mint("synthetic:test:id#sketch").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    )];

    assert!(marker_relation_is_inactive(
        &relation,
        &definition,
        &entities
    ));
}

#[test]
fn geometrically_contradicted_point_coincidence_is_inactive() {
    let mut relation = marker("relation", None);
    relation.reclassify(SketchInputKind::Relation(SketchRelationKind::Coincident));
    let ids = [
        SketchEntityId::mint("synthetic:test:id#first").unwrap(),
        SketchEntityId::mint("synthetic:test:id#second").unwrap(),
    ];
    let definition = SketchConstraintDefinitionInput::Native {
        native_kind: cadmpeg_ir::products::NonBlankString::new("sldprt:marker-relation:9").unwrap(),
        native_state: None,
        native_flags: None,
        native_properties: std::collections::BTreeMap::new(),
        entities: ids.to_vec(),
        parameter: None,
        operands: Vec::new(),
    };
    let point = |id: SketchEntityId, position| {
        SketchEntity::new(
            id,
            SketchId::mint("synthetic:test:id#sketch").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
        )
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
    relation.reclassify(SketchInputKind::Relation(SketchRelationKind::Horizontal));
    let entity = |id: &str, geometry| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("synthetic:test:id#sketch").unwrap(),
            geometry,
        )
    };
    let definition = |entities| SketchConstraintDefinitionInput::Native {
        native_kind: cadmpeg_ir::products::NonBlankString::new("sldprt:marker-relation:4").unwrap(),
        native_state: None,
        native_flags: None,
        native_properties: std::collections::BTreeMap::new(),
        entities,
        parameter: None,
        operands: Vec::new(),
    };
    let point = entity(
        "synthetic:test:id#point",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    );
    let line = entity(
        "synthetic:test:id#line",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    );

    assert!(marker_relation_is_inactive(
        &relation,
        &definition(vec![point.id().clone()]),
        std::slice::from_ref(&point),
    ));
    assert!(!marker_relation_is_inactive(
        &relation,
        &definition(vec![line.id().clone()]),
        std::slice::from_ref(&line),
    ));
    assert!(!marker_relation_is_inactive(
        &relation,
        &definition(vec![
            point.id().clone(),
            SketchEntityId::mint("synthetic:test:id#second").unwrap()
        ]),
        &[
            point,
            entity(
                "synthetic:test:id#second",
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(1.0, 0.0),
                })
                .unwrap(),
            ),
        ],
    ));
    assert!(marker_relation_is_inactive(
        &relation,
        &SketchConstraintDefinitionInput::Native {
            native_kind: cadmpeg_ir::products::NonBlankString::new("sldprt:marker-relation:4")
                .unwrap(),
            native_state: None,
            native_flags: None,
            native_properties: std::collections::BTreeMap::new(),
            entities: Vec::new(),
            parameter: None,
            operands: vec![
                SketchNativeOperand {
                    native_kind: cadmpeg_ir::products::NonBlankString::new(
                        "sldprt:marker-local-id"
                    )
                    .expect("source operand kind is nonempty"),
                    field: None,
                    object_index: Some(3),
                    native_ref: Some("same-marker".into()),
                },
                SketchNativeOperand {
                    native_kind: cadmpeg_ir::products::NonBlankString::new(
                        "sldprt:marker-local-id"
                    )
                    .expect("source operand kind is nonempty"),
                    field: None,
                    object_index: Some(3),
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
        origin = origin.with_test_position(origin.ordinal(), 0);
        let mut negative = marker("negative", Some([-0.007, 0.0]));
        negative = negative.with_test_position(negative.ordinal(), 1);
        let mut first_center = marker("first-center", Some([0.008, 0.0]));
        first_center = first_center.with_test_position(first_center.ordinal(), 2);
        let mut second_center = marker("second-center", Some([0.0015, 0.0]));
        second_center = second_center.with_test_position(second_center.ordinal(), 3);
        let operand = |index, marker: Option<&str>| FeatureInputOperand {
            offset: u64::from(index),
            reference_ref: format!("reference-{index}"),
            kind: FeatureInputOperandKind::Native(tag.try_into().unwrap()),
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
    first = first.with_test_position(first.ordinal(), 0);
    let mut second = marker("second", Some([1.0, 0.0]));
    second = second.with_test_position(second.ordinal(), 1);
    let mut third = marker("third", Some([2.0, 0.0]));
    third = third.with_test_position(third.ordinal(), 2);
    let operand = |index| FeatureInputOperand {
        offset: u64::from(index),
        reference_ref: format!("reference-{index}"),
        kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_8100),
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
    curve.reclassify(SketchInputKind::LineOrCircle);
    let mut first = marker("first", Some([1.0, 0.0]));
    first = first.with_test_identity(first.object_index(), Some(15));
    first = first.with_test_identity(Some(14), first.local_id());
    let mut second = marker("second", Some([2.0, 0.0]));
    second = second.with_test_identity(Some(15), second.local_id());

    assert_eq!(
        legacy_terminal_profile_indexed_endpoints(&payload, &curve, &[&curve, &first, &second])
            .map(|endpoints| endpoints.map(crate::records::SketchInputEntity::id)),
        Some(["first", "second"])
    );
}
