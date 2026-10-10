// SPDX-License-Identifier: Apache-2.0
//! Parameter dependency admission tests.

#[test]
fn design_empty_parameters_skip_object_names_without_work_or_storage() {
    let object = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Feature".into(),
            "Feature".into(),
        )
        .expect("object identity"),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: std::collections::BTreeMap::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let (cycles, _storage) = super::super::ordering::bind_parameter_dependencies(
        &ctx,
        &mut Vec::new(),
        &vec![object; 4097],
        &std::collections::BTreeSet::new(),
    )
    .expect("empty parameters skip names");
    assert!(cycles.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn design_parameter_candidates_refuse_at_collection_limit() {
    let parameter = cadmpeg_ir::features::DesignParameter {
        id: cadmpeg_ir::features::ParameterId::mint("fcstd:design:parameter#Feature:Length")
            .expect("valid parameter identity"),
        owner: None,
        ordinal: 0,
        name: "Length".into(),
        expression: "1".into(),
        display: None,
        value: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: std::collections::BTreeMap::default(),
        pmi: None,
        native_ref: None,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(
        matches!(super::super::ordering::bind_parameter_dependencies(
        &ctx, &mut vec![parameter.clone()], &[], &std::collections::BTreeSet::default(),
    ), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let result =
        super::super::ordering::order_parameters_by_dependencies(&ctx, &mut vec![parameter]);
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(ref limit))
        if limit.operation == "fcstd known parameter identities"),
        "{result:?}"
    );
}

#[test]
fn design_qualified_parameter_name_refuses_at_materialized_limit() {
    let object = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Feature".into(),
            "Feature".into(),
        )
        .expect("object identity"),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: std::collections::BTreeMap::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let parameter = cadmpeg_ir::features::DesignParameter {
        id: cadmpeg_ir::features::ParameterId::mint("fcstd:design:parameter#Feature:Length")
            .expect("valid parameter identity"),
        owner: Some(
            cadmpeg_ir::features::FeatureId::mint("fcstd:design:feature#Feature")
                .expect("valid feature identity"),
        ),
        ordinal: 0,
        name: "Length".into(),
        expression: "Feature.Length".into(),
        display: None,
        value: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: std::collections::BTreeMap::default(),
        pmi: None,
        native_ref: None,
    };
    let _error =
        crate::test_support::materialized_refusal_at("fcstd qualified candidate name", |ctx| {
            super::super::ordering::bind_parameter_dependencies(
                ctx,
                &mut vec![parameter.clone()],
                std::slice::from_ref(&object),
                &std::collections::BTreeSet::default(),
            )
            .map(|(features, _storage)| features)
        });
}

fn parameter_dependency_fixture(
    cycle: bool,
) -> (
    crate::native::ObjectRecord,
    Vec<cadmpeg_ir::features::DesignParameter>,
) {
    let object = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Feature".into(),
            "Feature".into(),
        )
        .expect("object identity"),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: std::collections::BTreeMap::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let parameters = [
        ("Length", "Width"),
        ("Width", if cycle { "Length" } else { "1" }),
    ]
    .into_iter()
    .enumerate()
    .map(
        |(ordinal, (name, expression))| cadmpeg_ir::features::DesignParameter {
            id: cadmpeg_ir::features::ParameterId::mint(format!(
                "fcstd:design:parameter#Feature:{name}"
            ))
            .expect("valid parameter identity"),
            owner: Some(
                cadmpeg_ir::features::FeatureId::mint("fcstd:design:feature#Feature")
                    .expect("valid feature identity"),
            ),
            ordinal: u32::try_from(ordinal).expect("fixture value fits u32"),
            name: name.into(),
            expression: expression.into(),
            display: None,
            value: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties: std::collections::BTreeMap::default(),
            pmi: None,
            native_ref: None,
        },
    )
    .collect();
    (object, parameters)
}

#[test]
fn design_parameter_dependency_stages_refuse_at_collection_limits() {
    let (object, mut parameters) = parameter_dependency_fixture(false);
    parameters[0].expression = "Feature.Width".into();
    for operation in [
        "fcstd unique local candidates",
        "fcstd unique qualified candidates",
        "fcstd parameter dependencies",
        "fcstd parameter dependency members",
        "validate distinct decoded members",
        "fcstd ordinal owner groups",
        "fcstd owner ordinals",
        "fcstd known parameter identities",
        "fcstd emitted parameter identities",
        "fcstd reordered parameters",
        "fcstd next ordinal owners",
    ] {
        crate::test_support::assert_collection_refusal_at(&[], operation, |ctx| {
            super::super::ordering::bind_parameter_dependencies(
                ctx,
                &mut parameters.clone(),
                std::slice::from_ref(&object),
                &std::collections::BTreeSet::default(),
            )
            .map(|(features, _storage)| features)
        });
    }
}

#[test]
fn design_parameter_cycle_owners_refuse_at_collection_limit() {
    let (object, parameters) = parameter_dependency_fixture(true);
    crate::test_support::assert_collection_refusal_at(&[], "fcstd parameter cycle owners", |ctx| {
        super::super::ordering::bind_parameter_dependencies(
            ctx,
            &mut parameters.clone(),
            std::slice::from_ref(&object),
            &std::collections::BTreeSet::default(),
        )
        .map(|(features, _storage)| features)
    });
}

#[test]
fn parameter_candidates_preserve_alias_and_duplicate_ambiguity() {
    for (source, duplicate, expected) in [
        ("Length", false, 1),
        ("Alias", false, 1),
        ("Length", true, 0),
        ("Alias", true, 0),
    ] {
        let (object, mut parameters) = parameter_dependency_fixture(false);
        parameters[0].expression = "1".into();
        parameters[0].properties.insert(
            cadmpeg_core::nonblank_literal!("source_name"),
            source.into(),
        );
        parameters[1].expression = source.into();
        let consumer = parameters[1].id.clone();
        let target = parameters[0].id.clone();
        if duplicate {
            parameters.push(parameters[0].clone());
        }
        crate::test_support::with_service_context(&[], |ctx| {
            let (_cycles, _storage) = super::super::ordering::bind_parameter_dependencies(
                ctx,
                &mut parameters,
                std::slice::from_ref(&object),
                &std::collections::BTreeSet::new(),
            )
            .expect("dependency binding");
            let dependencies = &parameters
                .iter()
                .find(|parameter| parameter.id == consumer)
                .expect("consumer")
                .dependencies;
            assert_eq!(dependencies.len(), expected);
            if expected == 1 {
                assert!(dependencies.contains(&target));
            }
        });
    }
}

#[test]
fn parameter_qualified_collisions_across_owners_remain_ambiguous() {
    let (mut first_object, first_parameters) = parameter_dependency_fixture(false);
    let mut second_object = first_object.clone();
    first_object.identity = crate::native::object_identity::ObjectIdentity::try_new(
        "fcstd:native:object#A.B".into(),
        "A.B".into(),
    )
    .expect("first identity");
    second_object.identity = crate::native::object_identity::ObjectIdentity::try_new(
        "fcstd:native:object#A".into(),
        "A".into(),
    )
    .expect("second identity");
    let mut first = first_parameters[0].clone();
    first.id = cadmpeg_ir::features::ParameterId::mint("fcstd:design:parameter#A.B:C")
        .expect("first parameter");
    first.owner = Some(
        cadmpeg_ir::features::FeatureId::mint("fcstd:design:feature#A.B").expect("first owner"),
    );
    first.name = "C".into();
    first.expression = "1".into();
    let mut second = first.clone();
    second.id = cadmpeg_ir::features::ParameterId::mint("fcstd:design:parameter#A:B.C")
        .expect("second parameter");
    second.owner = Some(
        cadmpeg_ir::features::FeatureId::mint("fcstd:design:feature#A").expect("second owner"),
    );
    second.name = "B.C".into();
    let mut consumer = first_parameters[1].clone();
    consumer.owner = None;
    consumer.expression = "A.B.C".into();
    let consumer_id = consumer.id.clone();
    let first_id = first.id.clone();
    for collision in [false, true] {
        let mut parameters = vec![first.clone(), consumer.clone()];
        if collision {
            parameters.push(second.clone());
        }
        crate::test_support::with_service_context(&[], |ctx| {
            let (_cycles, _storage) = super::super::ordering::bind_parameter_dependencies(
                ctx,
                &mut parameters,
                &[first_object.clone(), second_object.clone()],
                &std::collections::BTreeSet::new(),
            )
            .expect("dependency binding");
            let dependencies = &parameters
                .iter()
                .find(|parameter| parameter.id == consumer_id)
                .expect("consumer")
                .dependencies;
            if collision {
                assert!(dependencies.is_empty());
            } else {
                assert_eq!(dependencies.len(), 1);
                assert!(dependencies.contains(&first_id));
            }
        });
    }
}

#[test]
fn design_cycle_objects_use_scoped_storage_across_repeated_fallbacks() {
    let (template, _) = parameter_dependency_fixture(false);
    let objects = (0..4)
        .map(|index| {
            let mut object = template.clone();
            let name = format!("Cycle{index}");
            object.identity = crate::native::object_identity::ObjectIdentity::try_new(
                format!("fcstd:native:object#{name}"),
                name,
            )
            .expect("cycle identity");
            object.dependencies = vec![object.id().clone()];
            object.order = index;
            object
        })
        .collect::<Vec<_>>();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    let ordering = super::super::ordering::feature_ordinals(
        &ctx,
        &objects,
        &std::collections::BTreeMap::new(),
        &std::collections::HashMap::new(),
    )
    .expect("cycle scratch is not retained");
    assert_eq!(ordering.cycle_affected.len(), objects.len());
    for object in &objects {
        assert!(ordering.cycle_affected.contains(object.id().as_str()));
    }
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn parameter_cycle_consumers_skip_all_candidate_indexes() {
    let (object, mut parameters) = parameter_dependency_fixture(false);
    parameters.truncate(1);
    let owner = parameters[0].owner.clone().expect("owned parameter");
    let objects = (0..4097).map(|index| {
        let mut object = object.clone();
        let name = format!("Unused{index}");
        object.identity = crate::native::object_identity::ObjectIdentity::try_new(
            format!("fcstd:native:object#{name}"), name,
        ).expect("object identity");
        object
    }).collect::<Vec<_>>();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 64;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let (_cycles, _storage) = super::super::ordering::bind_parameter_dependencies(
        &ctx, &mut parameters, &objects, &std::collections::BTreeSet::from([owner]),
    ).expect("cycle expressions do not consume candidate indexes");
    assert!(parameters[0].dependencies.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn constant_parameter_expressions_skip_qualified_object_names() {
    let (object, mut parameters) = parameter_dependency_fixture(false);
    for parameter in &mut parameters { parameter.expression = "1".into(); }
    let objects = vec![object; 4097];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 64;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let (_cycles, _storage) = super::super::ordering::bind_parameter_dependencies(
        &ctx, &mut parameters, &objects, &std::collections::BTreeSet::new(),
    ).expect("constant expressions do not consume qualified names");
    assert!(parameters.iter().all(|parameter| parameter.dependencies.is_empty()));
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn eligible_parameter_keeps_cycle_owned_candidates() {
    let (object, mut parameters) = parameter_dependency_fixture(false);
    let target = parameters[0].id.clone();
    let cyclic_owner = parameters[0].owner.clone().expect("candidate owner");
    parameters[1].owner = None;
    parameters[1].expression = "Feature.Length".into();
    let consumer = parameters[1].id.clone();
    crate::test_support::with_service_context(&[], |ctx| {
        let (_cycles, _storage) = super::super::ordering::bind_parameter_dependencies(
            ctx, &mut parameters, &[object], &std::collections::BTreeSet::from([cyclic_owner]),
        ).expect("eligible consumer resolves cycle-owned source");
        let parameter = parameters.iter().find(|parameter| parameter.id == consumer).expect("consumer");
        assert!(parameter.dependencies.contains(&target));
        assert_eq!(parameter.dependencies.len(), 1);
    });
}
