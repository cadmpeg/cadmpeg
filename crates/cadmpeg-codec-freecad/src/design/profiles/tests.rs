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
            super::EndpointIndex::new(ctx, &profile_entities, &entities).map(drop)
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
        super::endpoint_candidates(ctx, source, &available, &relations, &[], &index).map(drop)
    });
}

fn entity(id: &str, geometry: SketchGeometry) -> SketchEntity {
    SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint(id).unwrap(),
        SketchId::mint("test:test:sketch#curved").unwrap(),
        geometry,
    )
}

fn line_entity(ordinal: u32) -> SketchEntity {
    let start = f64::from(ordinal) * 10.0;
    entity(
        &format!("test:test:entity#profile-work-{ordinal}"),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(start, 0.0),
            end: Point2::new(start + 1.0, 0.0),
        })
        .expect("line geometry"),
    )
}

fn coincident_constraint(id: &str, sketch: SketchId, loci: Vec<SketchLocus>) -> SketchConstraint {
    SketchConstraint {
        id: SketchConstraintId::mint(id).unwrap(),
        sketch,
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::CoincidentLoci { loci },
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
    }
}

fn successful_work_used(ctx: &cadmpeg_core::decode::DecodeContext<'_>) -> u64 {
    let error = ctx
        .charge_work(u64::MAX, "FCStd profile short-prefix work oracle")
        .unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("work overflow probe must return a resource refusal")
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    limit.used
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

#[test]
fn empty_profile_relations_release_entity_index_storage() {
    let entities = [entity(
        "test:test:entity#profile-storage",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .expect("line geometry"),
    )];
    let marker = "probe".repeat(4096);
    let operation = "test after profile relation construction";
    let error = crate::test_support::refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        &[],
        operation,
        |ctx| {
            let (_storage, relations);
            (relations, _storage) = super::explicit_endpoint_relations(
                ctx,
                &std::collections::BTreeSet::new(),
                &entities,
                &[],
            )?;
            assert!(relations.is_empty());
            ctx.format_scoped(format_args!("{marker}"), operation)
                .map(|result| {
                    let (_format_storage, text);
                    (text, _format_storage) = result;
                    text
                })
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("materialized refusal required");
    };
    assert_eq!(
        limit.used, 0,
        "empty relations hold no temporary index storage"
    );
}

#[test]
fn profile_endpoint_index_failure_does_not_precharge_entity_suffix() {
    let first_entity = line_entity(0);
    let short_profile_entities = std::collections::BTreeSet::from([0]);
    let short_work = {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("profile oracle context");
        let mut source = short_profile_entities.iter();
        let Some(index) = ctx
            .next_charged(&mut source, "FCStd profile endpoint extraction")
            .expect("short index source step")
        else {
            panic!("short index source contains its first entity")
        };
        let (start, _) = super::endpoints(&first_entity).expect("line endpoints");
        let mut by_scale = std::collections::BTreeMap::new();
        ctx.push_btree_group(
            &mut by_scale,
            super::endpoint_scale_bucket(start),
            super::IndexedEndpoint {
                locus: super::EndpointLocus {
                    entity: *index,
                    start: true,
                },
                point: start,
            },
            "FCStd profile endpoint buckets",
            "FCStd profile endpoint index",
        )
        .expect("first indexed endpoint");
        drop(by_scale);
        successful_work_used(&ctx)
    };
    let entity_count = usize::try_from(short_work)
        .expect("short profile work fits a test input")
        .checked_add(1)
        .expect("profile entity suffix length fits")
        .max(256);
    let entity_count = u32::try_from(entity_count).expect("profile entity suffix fits u32");
    let entities = (0..entity_count).map(line_entity).collect::<Vec<_>>();
    let profile_entities = (0..entities.len()).collect::<std::collections::BTreeSet<_>>();

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = short_work;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("profile replay context");
    let error = super::EndpointIndex::new(&ctx, &profile_entities, &entities)
        .err()
        .expect("the second endpoint index visit must exceed the short-prefix cap");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("endpoint index work refusal required")
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.operation, "FCStd profile endpoint buckets");
    assert_eq!(limit.used, short_work);
    assert!(limit.additional > 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn explicit_profile_locus_failure_does_not_precharge_locus_suffix() {
    let entities = [line_entity(0)];
    let profile_entities = std::collections::BTreeSet::from([0]);
    let first_locus = SketchLocus::Start(entities[0].id().clone());
    let second_locus = SketchLocus::End(entities[0].id().clone());
    let short_constraint = coincident_constraint(
        "test:test:constraint#profile-short-prefix",
        entities[0].sketch.clone(),
        vec![first_locus.clone(), second_locus.clone()],
    );
    let short_work = {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("profile oracle context");
        let (storage, relations);
        (relations, storage) = super::explicit_endpoint_relations(
            &ctx,
            &profile_entities,
            &entities,
            std::slice::from_ref(&short_constraint),
        )
        .expect("short explicit relation prefix");
        let start = super::EndpointLocus {
            entity: 0,
            start: true,
        };
        let end = super::EndpointLocus {
            entity: 0,
            start: false,
        };
        assert!(
            relations
                == std::collections::BTreeMap::from([
                    (start, std::collections::BTreeSet::from([end])),
                    (end, std::collections::BTreeSet::from([start])),
                ])
        );
        drop(relations);
        drop(storage);
        successful_work_used(&ctx)
    };
    let mut loci = vec![first_locus, second_locus];
    loci.extend((0..(short_work + 256)).map(|_| SketchLocus::Start(entities[0].id().clone())));
    let long_locus_count = loci.len();
    let long_constraint = coincident_constraint(
        "test:test:constraint#profile-long-suffix",
        entities[0].sketch.clone(),
        loci,
    );

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = short_work
        .checked_add(1)
        .expect("short profile work cap fits");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("profile replay context");
    let error = super::explicit_endpoint_relations(
        &ctx,
        &profile_entities,
        &entities,
        std::slice::from_ref(&long_constraint),
    )
    .err()
    .expect("second nested locus lookup must exceed the short-prefix cap");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("profile locus work refusal required")
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert!([
        "FCStd explicit profile loci",
        "FCStd profile entity index",
        "FCStd eligible profile entity lookup",
    ]
    .contains(&limit.operation));
    assert!(limit.additional < u64::try_from(long_locus_count).expect("locus bound fits u64"));
    assert!(limit.used <= policy.limits.max_work_units);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn empty_profile_routes_preserve_prior_resource_refusal() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("profile test context");
    let chain_storage = ctx
        .reserve_scoped(0, "FCStd profile uses")
        .expect("empty profile storage");
    let cadmpeg_core::CodecError::ResourceLimit(first) = ctx
        .charge_work(1, "test prior profile work refusal")
        .expect_err("work cap fuses the context")
    else {
        panic!("work refusal required")
    };

    assert!(matches!(
        super::build_profiles(&ctx, &[], &[]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == first
    ));
    assert!(matches!(
        super::EndpointIndex::new(
            &ctx,
            &std::collections::BTreeSet::new(),
            &[],
        ),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == first
    ));
    let source = super::EndpointLocus {
        entity: 0,
        start: true,
    };
    let explicit = std::collections::BTreeMap::from([(source, std::collections::BTreeSet::new())]);
    let endpoint_index = super::EndpointIndex {
        by_scale: std::collections::BTreeMap::new(),
    };
    assert!(matches!(
        super::endpoint_candidates(
            &ctx,
            source,
            &std::collections::BTreeSet::new(),
            &explicit,
            &[],
            &endpoint_index,
        ),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == first
    ));
    assert!(matches!(
        super::explicit_endpoint_relations(
            &ctx,
            &std::collections::BTreeSet::new(),
            &[],
            &[],
        ),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == first
    ));
    assert!(matches!(
        super::finish_profile_chain(
            &ctx,
            chain_storage,
            std::collections::VecDeque::new(),
            &[],
        ),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == first
    ));
    assert_eq!(ctx.resource_refusal(), Some(first));
}
