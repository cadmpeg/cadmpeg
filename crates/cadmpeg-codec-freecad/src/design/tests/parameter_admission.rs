// SPDX-License-Identifier: Apache-2.0
//! Parameter dependency admission tests.

#[test]
fn design_parameter_object_name_index_refuses_at_collection_limit() {
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
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(super::super::bind_parameter_dependencies(
        &ctx, &mut Vec::new(), &[object], &std::collections::BTreeSet::default(),
    ), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "fcstd parameter dependency object names"));
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
    assert!(matches!(super::super::bind_parameter_dependencies(
        &ctx, &mut vec![parameter.clone()], &[], &std::collections::BTreeSet::default(),
    ), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "fcstd parameter dependency candidates"));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let result = super::super::order_parameters_by_dependencies(&ctx, &mut vec![parameter]);
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(ref limit))
        if limit.operation == "fcstd known parameter identities"),
        "{result:?}"
    );
}

#[test]
fn design_qualified_parameter_name_refuses_at_retained_limit() {
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
        expression: "1".into(),
        display: None,
        value: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: std::collections::BTreeMap::default(),
        pmi: None,
        native_ref: None,
    };
    crate::test_support::assert_retained_refusal_at(&[], "fcstd qualified candidate name", |ctx| {
        super::super::bind_parameter_dependencies(
            ctx,
            &mut vec![parameter.clone()],
            std::slice::from_ref(&object),
            &std::collections::BTreeSet::default(),
        )
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
    let (object, parameters) = parameter_dependency_fixture(false);
    for operation in [
        "fcstd parameter dependency candidates",
        "fcstd parameter candidate names",
        "fcstd local candidate keys",
        "fcstd local candidate identities",
        "fcstd qualified candidate keys",
        "fcstd qualified candidate identities",
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
            super::super::bind_parameter_dependencies(
                ctx,
                &mut parameters.clone(),
                std::slice::from_ref(&object),
                &std::collections::BTreeSet::default(),
            )
        });
    }
}

#[test]
fn design_parameter_cycle_owners_refuse_at_collection_limit() {
    let (object, parameters) = parameter_dependency_fixture(true);
    crate::test_support::assert_collection_refusal_at(&[], "fcstd parameter cycle owners", |ctx| {
        super::super::bind_parameter_dependencies(
            ctx,
            &mut parameters.clone(),
            std::slice::from_ref(&object),
            &std::collections::BTreeSet::default(),
        )
    });
}
