// SPDX-License-Identifier: Apache-2.0
//! Profile endpoint and chain admission tests.

use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntity,
    SketchEntityUse, SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus,
};

use super::endpoints_match_by_roundoff;

fn build_profiles(
    entities: &[SketchEntity],
    constraints: &[SketchConstraint],
) -> Vec<Vec<SketchEntityUse>> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("profile test context");
    super::build_profiles(&ctx, entities, constraints).expect("profile projection")
}

#[test]
fn x64_profile_index_refuses_exhausted_work_before_search() {
    let entities = [entity(
        "test:test:entity#work",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .expect("line geometry"),
    )];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("profile test context");
    let error = super::build_profiles(&ctx, &entities, &[])
        .expect_err("profile construction work must be admitted before scanning");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
    ));
}

#[test]
fn profile_endpoint_buckets_refuse_at_matching_collection_limits() {
    let entities = [entity(
        "test:test:entity#bucket",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .expect("line geometry"),
    )];
    let profile_entities = std::collections::BTreeSet::from([0]);
    for operation in [
        "FCStd profile endpoint buckets",
        "FCStd profile endpoint index",
    ] {
        crate::test_support::assert_collection_refusal_at(&[], operation, |ctx| {
            super::EndpointIndex::new(ctx, &profile_entities, &entities)
                .map(|(_storage, index)| index)
        });
    }
}

#[test]
fn explicit_profile_candidates_refuse_at_matching_collection_limit() {
    let source = super::EndpointLocus {
        entity: 0,
        start: true,
    };
    let target = super::EndpointLocus {
        entity: 1,
        start: true,
    };
    let available = std::collections::BTreeSet::from([1]);
    let relations =
        std::collections::BTreeMap::from([(source, std::collections::BTreeSet::from([target]))]);
    let index = super::EndpointIndex {
        by_scale: std::collections::BTreeMap::new(),
    };
    crate::test_support::assert_collection_refusal_at(&[], "FCStd profile candidates", |ctx| {
        super::endpoint_candidates(ctx, source, &available, &relations, &[], &index)
            .map(|(_storage, candidates)| candidates)
    });
}

fn entity(id: &str, geometry: SketchGeometry) -> SketchEntity {
    SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint(id).unwrap(),
        SketchId::mint("test:test:sketch#curved").unwrap(),
        geometry,
    )
}

#[test]
fn profile_chain_refuses_before_use_growth_and_identity_copy() {
    let entities = [entity(
        "test:test:entity#profile-line",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .expect("valid line"),
    )];
    crate::test_support::assert_collection_refusal_at(&[], "FCStd profile uses", |ctx| {
        super::build_profiles(ctx, &entities, &[])
    });
    crate::test_support::assert_retained_refusal_at(&[], "FCStd profile use identity", |ctx| {
        super::build_profiles(ctx, &entities, &[])
    });
    crate::test_support::assert_collection_refusal_at(&[], "FCStd profile chains", |ctx| {
        super::build_profiles(ctx, &entities, &[])
    });
}

#[test]
fn curved_segments_chain_by_their_evaluated_endpoints() {
    let entities = [
        entity(
            "test:test:entity#line",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(-1.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .unwrap(),
        ),
        entity(
            "test:test:entity#arc",
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: Point2::new(0.0, 0.0),
                radius: Length::new(1.0).unwrap(),
                start_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
                end_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            })
            .unwrap(),
        ),
        entity(
            "test:test:entity#line-after-arc",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 1.0),
                end: Point2::new(1.0, 1.0),
            })
            .unwrap(),
        ),
    ];
    let profiles = build_profiles(&entities, &[]);
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].len(), 3);
}

#[test]
fn disconnected_profile_seeds_follow_persisted_entity_order() {
    let entities = (1..=11)
        .map(|ordinal| {
            entity(
                &format!("test:test:entity#{ordinal}"),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(f64::from(ordinal) * 10.0, 0.0),
                    end: Point2::new(f64::from(ordinal) * 10.0 + 1.0, 0.0),
                })
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();

    let profiles = build_profiles(&entities, &[]);

    assert_eq!(profiles.len(), entities.len());
    assert!(profiles
        .iter()
        .zip(&entities)
        .all(|(profile, entity)| profile[0].entity == entity.id().clone()));
}

#[test]
fn disconnected_profile_seeds_skip_construction_in_persisted_order() {
    let mut construction = entity(
        "test:test:entity#1",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(-1.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    );
    construction.construction = true;
    let entities = vec![
        construction,
        entity(
            "test:test:entity#2",
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length::new(2.0).unwrap(),
            })
            .unwrap(),
        ),
        entity(
            "test:test:entity#3",
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(10.0, 0.0),
                radius: Length::new(2.0).unwrap(),
            })
            .unwrap(),
        ),
    ];

    let profiles = build_profiles(&entities, &[]);

    assert_eq!(
        profiles
            .iter()
            .map(|profile| profile[0].entity.clone())
            .collect::<Vec<_>>(),
        vec![entities[1].id().clone(), entities[2].id().clone()]
    );
}

#[test]
fn coincident_constraint_connects_numerically_separate_endpoints() {
    let entities = [
        entity(
            "test:test:entity#1",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .unwrap(),
        ),
        entity(
            "test:test:entity#2",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(2.0, 0.0),
                end: Point2::new(3.0, 0.0),
            })
            .unwrap(),
        ),
    ];
    let constraint = SketchConstraint {
        id: SketchConstraintId::mint("test:test:constraint#1").unwrap(),
        sketch: entities[0].sketch.clone(),
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::CoincidentLoci {
                loci: vec![
                    SketchLocus::End(entities[0].id().clone()),
                    SketchLocus::Start(entities[1].id().clone()),
                ],
            },
        )
        .unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    };

    let profiles = build_profiles(&entities, &[constraint]);

    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].len(), 2);
}

#[test]
fn explicit_endpoint_relations_precede_nearby_geometry() {
    let entities = [
        entity(
            "test:test:entity#anchor",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(-1.0, 0.0),
                end: Point2::new(0.0, 0.0),
            })
            .unwrap(),
        ),
        entity(
            "test:test:entity#nearby",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(32.0 * f64::EPSILON, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .unwrap(),
        ),
        entity(
            "test:test:entity#constrained",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(2.0, 0.0),
                end: Point2::new(3.0, 0.0),
            })
            .unwrap(),
        ),
    ];
    let constraint = SketchConstraint {
        id: SketchConstraintId::mint("test:test:constraint#explicit-precedence").unwrap(),
        sketch: entities[0].sketch.clone(),
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::CoincidentLoci {
                loci: vec![
                    SketchLocus::End(entities[0].id().clone()),
                    SketchLocus::Start(entities[2].id().clone()),
                ],
            },
        )
        .unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    };

    let profiles = build_profiles(&entities, &[constraint]);

    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].len(), 2);
    assert_eq!(profiles[0][0].entity, entities[0].id().clone());
    assert_eq!(profiles[0][1].entity, entities[2].id().clone());
    assert_eq!(
        profiles[1],
        vec![SketchEntityUse {
            entity: entities[1].id().clone(),
            reversed: false,
        }]
    );
}

#[test]
fn profile_junction_uses_the_cadir_roundoff_boundary() {
    let entities = |gap| {
        [
            entity(
                "test:test:entity#anchor",
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(1.0, 0.0),
                })
                .unwrap(),
            ),
            entity(
                "test:test:entity#continuation",
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(1.0 + gap, 0.0),
                    end: Point2::new(2.0, 0.0),
                })
                .unwrap(),
            ),
        ]
    };

    let inside = entities(32.0 * f64::EPSILON);
    let inside_profiles = build_profiles(&inside, &[]);
    assert_eq!(inside_profiles.len(), 1);
    assert_eq!(inside_profiles[0].len(), 2);

    let outside = entities(128.0 * f64::EPSILON);
    let outside_profiles = build_profiles(&outside, &[]);
    assert_eq!(outside_profiles.len(), 2);
    assert!(outside_profiles.iter().all(|profile| profile.len() == 1));
}

#[test]
fn multiple_explicit_coincident_continuations_remain_separate_seeds() {
    let entities = [
        entity(
            "test:test:entity#anchor",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(-1.0, 0.0),
                end: Point2::new(0.0, 0.0),
            })
            .unwrap(),
        ),
        entity(
            "test:test:entity#first-continuation",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .unwrap(),
        ),
        entity(
            "test:test:entity#second-continuation",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(0.0, 1.0),
            })
            .unwrap(),
        ),
    ];
    let constraint = SketchConstraint {
        id: SketchConstraintId::mint("test:test:constraint#ambiguous-explicit").unwrap(),
        sketch: entities[0].sketch.clone(),
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::CoincidentLoci {
                loci: vec![
                    SketchLocus::End(entities[0].id().clone()),
                    SketchLocus::Start(entities[1].id().clone()),
                    SketchLocus::Start(entities[2].id().clone()),
                ],
            },
        )
        .unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    };

    let profiles = build_profiles(&entities, &[constraint]);

    assert_eq!(profiles.len(), 3);
    assert!(profiles.iter().all(|profile| profile.len() == 1));
}

#[test]
fn endpoint_roundoff_uses_a_bounded_scale() {
    let exact = Point2::new(1.0, 0.0);
    let inside = Point2::new(1.0 + 32.0 * f64::EPSILON, 0.0);
    let outside = Point2::new(1.0 + 128.0 * f64::EPSILON, 0.0);

    assert!(endpoints_match_by_roundoff(exact, inside));
    assert!(!endpoints_match_by_roundoff(exact, outside));
}

#[test]
fn ambiguous_profile_junctions_remain_separate() {
    let entities = (0..3)
        .map(|ordinal| {
            entity(
                &format!("test:test:entity#{}", ordinal + 1),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(f64::from(ordinal) + 1.0, 1.0),
                })
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();

    let profiles = build_profiles(&entities, &[]);

    assert_eq!(profiles.len(), 3);
    assert!(profiles.iter().all(|profile| profile.len() == 1));
}
