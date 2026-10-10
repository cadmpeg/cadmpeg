use super::*;

fn circle_scalar(
    offset: u64,
    name: &str,
    role: FeatureInputScalarRole,
    entity_ref: Option<&str>,
) -> FeatureInputScalar {
    FeatureInputScalar {
        id: format!("scalar-{offset}"),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_id: 1,
        name: name.into(),
        value: cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite test scalar"),
        role,

        operands: vec![FeatureInputOperand {
            offset: offset + 1,
            reference_ref: format!("reference-{offset}"),
            kind: FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x1234).unwrap()),
            entity_index: 0,
            entity_ref: entity_ref.map(str::to_owned),
        }],
    }
}

#[test]
fn circle_handle_literal_does_not_add_comparison_work() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut display = circle_scalar(0, "", FeatureInputScalarRole::Display, None);
    display.id = "s".into();
    display.feature_ref = None;
    let input = lane(
        vec![class(1, "sgEntHandlf"), class(2, "sgEntHandlg"), class(3, "sgEntHandlh")],
        vec![display],
    );
    let relation = crate::records::FeatureInputRelationInstance {
        id: "relation".into(), parent: "lane".into(), ordinal: 0, offset: 0,
        family: FeatureInputRelationFamily::CircleDiameter,
        class_ref: "class".into(), feature_ref: "sketch".into(),
        scalars: crate::records::relation_scalars::RelationScalars::from_refs(
            vec!["s".into()], None, Some("s".into()),
        ).unwrap(),
        operands: Vec::new(),
    };
    // Current shared singleton-sort charge: 1 + 1 + (8 + 2*8)*2*8.
    // It includes an upper bound for comparisons and moves not executed.
    // Other work: scalar visit; lookup visit and two ID bytes;
    // three classes; driver visit. Fixed grammar adds no comparison charge.
    const WORK: u64 = 394;
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 32; // four borrowed scalar slots
    policy.limits.max_collection_items = 64;
    policy.limits.max_work_units = WORK * 64;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        assert!(circle_dimension_handle_driver(&ctx, &relation, &input).unwrap().is_none());
    }
    assert_eq!(ctx.resource_refusal(), None);

    // The core probe searches only budgets marked unlimited. Keep this
    // independent negative control separate from the finite-work proof.
    let mut probe_policy = policy.clone();
    probe_policy.limits.max_work_units = u64::MAX;
    let probe_arena = DecodeArena::new();
    let (probe_ctx, _) = DecodeContext::from_root_bytes(&[], &probe_arena, &probe_policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits, "match SLDPRT circle handle class", None,
    );
    assert!(circle_dimension_handle_driver(&probe_ctx, &relation, &input).unwrap().is_none());
    assert_eq!(probe_ctx.resource_refusal(), None);
    let CodecError::ResourceLimit(limit) = probe_ctx
        .charge_work(1, "match SLDPRT circle handle class").unwrap_err()
    else { panic!("fixed comparison must leave its probe armed"); };
    assert_eq!(limit.operation, "match SLDPRT circle handle class");
    assert_eq!(limit.used, WORK);
    assert_eq!(limit.additional, 1);
    assert_eq!(probe_ctx.resource_refusal(), Some(limit));
    drop(probe);

    policy.limits.max_work_units = WORK - 1;
    policy.limits.max_collection_items = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = match circle_dimension_handle_driver(&ctx, &relation, &input) {
        Err(error) => error,
        Ok(_) => panic!("one below executed source work must refuse"),
    };
    let CodecError::ResourceLimit(limit) = error else { panic!("resource refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "match SLDPRT circle handle driver");
    assert_eq!(limit.used, WORK - 1);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

fn detached_driver_with_duplicate_index_key(
    duplicate_scalar: bool,
) -> (
    FeatureInputLane,
    crate::records::FeatureInputRelationInstance,
) {
    let mut display = scalar(20, FeatureInputScalarRole::Display);
    display.name = "display-name".into();
    let mut driver = scalar(40, FeatureInputScalarRole::Driving);
    driver.name = "driver-name".into();
    driver.operands.clear();
    let mut last_display = display.clone();
    last_display.name = "last-display-name".into();
    let mut input = lane(
        Vec::new(),
        if duplicate_scalar {
            vec![display.clone(), last_display, driver]
        } else {
            vec![display.clone(), driver]
        },
    );
    let name = |id: &str, value: &str| FeatureInputName {
        id: id.into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        object_id: None,
        value: value.into(),
    };
    input.names = if duplicate_scalar {
        vec![
            name("display-name", "First"),
            name("last-display-name", "Last"),
            name("driver-name", "Last"),
        ]
    } else {
        vec![
            name("display-name", "First"),
            name("display-name", "Last"),
            name("driver-name", "Last"),
        ]
    };
    let relation = crate::records::FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::CircleDiameter,
        class_ref: "class".into(),
        feature_ref: "sketch".into(),
        scalars: crate::records::relation_scalars::RelationScalars::from_refs(
            vec![display.id.clone()],
            None,
            Some(display.id),
        )
        .unwrap(),
        operands: Vec::new(),
    };
    (input, relation)
}

#[test]
fn empty_relation_bindings_do_not_build_lane_indexes() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (input, _) = detached_driver_with_duplicate_index_key(false);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bind_circle_dimension_centers(&ctx, &mut [], &input).unwrap();
    super::super::bind_detached_relation_drivers(&ctx, &mut [], &input).unwrap();
    assert_eq!(ctx.resource_refusal(), None);

    policy.limits.max_work_units = u64::MAX;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits, "index SLDPRT relation records", None,
    );
    super::super::bind_circle_dimension_centers(&ctx, &mut [], &input).unwrap();
    super::super::bind_detached_relation_drivers(&ctx, &mut [], &input).unwrap();
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(limit)) =
        ctx.charge_work(1, "index SLDPRT relation records")
    else { panic!("unused indexes must leave their probe armed"); };
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 1);
    assert_eq!(limit.operation, "index SLDPRT relation records");
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    drop(probe);
}

#[test]
fn ineligible_relation_bindings_visit_only_relations() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (input, mut relation) = detached_driver_with_duplicate_index_key(false);
    relation.scalars = crate::records::relation_scalars::RelationScalars::from_refs(
        vec!["scalar-20".into(), "scalar-40".into()],
        Some("scalar-40".into()), Some("scalar-20".into()),
    ).unwrap();
    let before = relation.clone();
    let mut relations = [relation];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    // One relation visit in each binding pass, repeated sixty-four times.
    policy.limits.max_work_units = 2 * 64;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        super::super::bind_circle_dimension_centers(&ctx, &mut relations, &input).unwrap();
        super::super::bind_detached_relation_drivers(&ctx, &mut relations, &input).unwrap();
        assert_eq!(relations[0], before);
    }
    assert_eq!(ctx.resource_refusal(), None);
    policy.limits.max_work_units = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bind_circle_dimension_centers(&ctx, &mut relations, &input).unwrap();
    let Err(CodecError::ResourceLimit(limit)) =
        super::super::bind_detached_relation_drivers(&ctx, &mut relations, &input)
    else { panic!("second actual relation visit must refuse"); };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "match SLDPRT detached relation candidates");
    assert_eq!(limit.limit, 1);
    assert_eq!(limit.used, 1);
    assert_eq!(limit.additional, 1);
    assert_eq!(relations[0], before);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
}

#[test]
fn circle_center_missing_display_keeps_name_index_unbuilt() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (input, mut relation) = detached_driver_with_duplicate_index_key(false);
    relation.operands.push(input.scalars[0].operands[0].clone());
    relation.scalars = crate::records::relation_scalars::RelationScalars::from_refs(
        vec!["absent".into()], None, Some("absent".into()),
    ).unwrap();
    let before = relation.clone();
    let mut relations = [relation];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    // Four (&str, borrowed scalar) buckets plus alignment and controls.
    policy.limits.max_materialized_bytes = 4 * 24 + 15 + 20;
    policy.limits.max_collection_items = 2 * 64;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        super::super::bind_circle_dimension_centers(&ctx, &mut relations, &input).unwrap();
        assert_eq!(relations[0], before);
        let released = ctx.reserve_scoped(131, "released missing-display lookup").unwrap();
        drop(released);
    }
    assert_eq!(ctx.resource_refusal(), None);
    policy.limits.max_materialized_bytes = 130;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) =
        super::super::bind_circle_dimension_centers(&ctx, &mut relations, &input)
    else { panic!("scalar table backing must be admitted before growth"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "index SLDPRT relation records");
    assert_eq!(limit.limit, 130);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 131);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
}

#[test]
fn ambiguous_detached_candidates_keep_driver_index_probe_armed() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (input, relation) = detached_driver_with_duplicate_index_key(false);
    let mut other = relation.clone();
    other.id = "other-relation".into();
    let before = [relation, other];
    let mut relations = before.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits, "match SLDPRT detached relation drivers", None,
    );
    super::super::bind_detached_relation_drivers(&ctx, &mut relations, &input).unwrap();
    assert_eq!(relations, before);
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(limit)) =
        ctx.charge_work(1, "match SLDPRT detached relation drivers")
    else { panic!("ambiguous candidates must leave the driver probe armed"); };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "match SLDPRT detached relation drivers");
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    drop(probe);
}

#[test]
fn detached_relation_uses_last_duplicate_scalar_index_entry() {
    let (input, relation) = detached_driver_with_duplicate_index_key(true);
    let mut relations = [relation];
    super::super::bind_detached_relation_drivers(
        &cadmpeg_test_support::service_decode_context(),
        &mut relations,
        &input,
    )
    .unwrap();
    assert_eq!(relations[0].parameter_scalar_ref(), Some("scalar-40"));
}

#[test]
fn detached_relation_uses_last_duplicate_name_index_entry() {
    let (input, relation) = detached_driver_with_duplicate_index_key(false);
    let mut relations = [relation];
    super::super::bind_detached_relation_drivers(
        &cadmpeg_test_support::service_decode_context(),
        &mut relations,
        &input,
    )
    .unwrap();
    assert_eq!(relations[0].parameter_scalar_ref(), Some("scalar-40"));
}

#[test]
fn circle_dimension_binds_driver_inside_declared_entity_handle() {
    let first = circle_scalar(20, "name-first", FeatureInputScalarRole::Display, None);
    let nested_display =
        circle_scalar(40, "name-nested", FeatureInputScalarRole::Display, None);
    let nested_driver = circle_scalar(50, "name-nested", FeatureInputScalarRole::Driving, None);
    let mut lane = lane(
        vec![class(10, "sgCircleDim"), class(31, "sgEntHandle")],
        vec![first.clone(), nested_display.clone(), nested_driver.clone()],
    );
    lane.names = vec![
        FeatureInputName {
            id: "name-first".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 20,
            object_id: None,
            value: "D1".into(),
        },
        FeatureInputName {
            id: "name-nested".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 40,
            object_id: None,
            value: "D4".into(),
        },
    ];

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one circle relation");
    };
    assert_eq!(relation.scalar_refs(), vec![first.id]);
    assert_eq!(relation.display_scalar_ref(), Some("scalar-20"));
    assert!(relation.parameter_scalar_ref().is_none());
    assert_eq!(
        circle_dimension_handle_driver(
            &cadmpeg_test_support::service_decode_context(),
            relation,
            &lane
        )
        .unwrap()
        .map(|scalar| scalar.id.as_str()),
        Some("scalar-50")
    );
}

#[test]
fn circle_dimension_keeps_ambiguous_handle_drivers_unbound() {
    let first = circle_scalar(20, "name-first", FeatureInputScalarRole::Display, None);
    let d4_display = circle_scalar(40, "name-d4", FeatureInputScalarRole::Display, None);
    let d4_driver = circle_scalar(50, "name-d4", FeatureInputScalarRole::Driving, None);
    let d5_display = circle_scalar(60, "name-d5", FeatureInputScalarRole::Display, None);
    let d5_driver = circle_scalar(70, "name-d5", FeatureInputScalarRole::Driving, None);
    let mut lane = lane(
        vec![class(10, "sgCircleDim"), class(31, "sgEntHandle")],
        vec![first, d4_display, d4_driver, d5_display, d5_driver],
    );
    lane.names = [("name-first", "D1"), ("name-d4", "D4"), ("name-d5", "D5")]
        .into_iter()
        .enumerate()
        .map(|(ordinal, (id, value))| FeatureInputName {
            id: id.into(),
            parent: "lane".into(),
            ordinal: u32::try_from(ordinal).expect("test index fits u32"),
            offset: 20 + cadmpeg_core::decode::u64_from_index(ordinal) * 10,
            object_id: None,
            value: value.into(),
        })
        .collect();

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one circle relation");
    };
    assert_eq!(relation.scalar_refs().len(), 1);
    assert!(relation.parameter_scalar_ref().is_none());
    assert!(circle_dimension_handle_driver(
        &cadmpeg_test_support::service_decode_context(),
        relation,
        &lane
    )
    .unwrap()
    .is_none());
}

#[test]
fn circle_dimension_groups_repeated_display_scalars_by_name_and_entity() {
    let mut first = circle_scalar(20, "name-d1", FeatureInputScalarRole::Display, None);
    first.operands[0].entity_index = 0;
    let mut second = circle_scalar(30, "name-d1", FeatureInputScalarRole::Display, None);
    second.operands[0].entity_index = 1;
    let mut third = circle_scalar(40, "name-d1", FeatureInputScalarRole::Display, None);
    third.operands[0].entity_index = 2;
    let mut different_name =
        circle_scalar(50, "name-d2", FeatureInputScalarRole::Display, None);
    different_name.operands[0].entity_index = 3;
    let mut lane = lane(
        vec![class(10, "sgCircleDim")],
        vec![first.clone(), second, third, different_name],
    );
    lane.names = vec![
        FeatureInputName {
            id: "name-d1".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 20,
            object_id: None,
            value: "D1".into(),
        },
        FeatureInputName {
            id: "name-d2".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 50,
            object_id: None,
            value: "D2".into(),
        },
    ];

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one repeated circle relation");
    };
    assert_eq!(
        relation.scalar_refs(),
        vec!["scalar-20", "scalar-30", "scalar-40"]
    );
    assert!(relation.parameter_scalar_ref().is_none());
    assert!(relation.display_scalar_ref().is_none());
    assert_eq!(relation.operands, first.operands);
}
