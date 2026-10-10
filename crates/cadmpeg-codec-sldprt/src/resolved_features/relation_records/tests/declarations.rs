use super::*;

#[test]
fn nonrelation_classes_leave_declaration_index_probe_armed() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let classes = [class(0, "sgEntHandle"), class(1, "other")];
    let scalars = [scalar(20, FeatureInputScalarRole::Driving)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        super::super::unique_relation_declaration_candidates_charged(&ctx, &[], &scalars, &[])
            .unwrap()
            .is_empty()
    );
    assert_eq!(ctx.resource_refusal(), None);
    policy.limits.max_work_units = 2;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        super::super::unique_relation_declaration_candidates_charged(&ctx, &classes, &scalars, &[])
            .unwrap()
            .is_empty()
    );
    assert_eq!(ctx.resource_refusal(), None);
    policy.limits.max_work_units = u64::MAX;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "index SLDPRT relation declarations",
        None,
    );
    assert!(
        super::super::unique_relation_declaration_candidates_charged(&ctx, &classes, &scalars, &[])
            .unwrap()
            .is_empty()
    );
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(limit)) =
        ctx.charge_work(1, "index SLDPRT relation declarations")
    else {
        panic!("nonrelation classes must leave index probe armed");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "index SLDPRT relation declarations");
    assert_eq!(limit.used, 2);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
    drop(probe);
}

#[test]
fn selected_declaration_candidates_release_discarded_workspace() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    assert_eq!(
        std::mem::size_of::<(
            &FeatureInputClass,
            &FeatureInputScalar,
            FeatureInputRelationFamily
        )>(),
        24
    );
    let classes = [class(0, "sgPntPntHorDist")];
    let scalars = [scalar(1, FeatureInputScalarRole::Driving)];
    // All-candidate Vec: four 24-byte slots. Count table: four 24-byte
    // buckets + fifteen alignment bytes + twenty controls. Standalone
    // selected output uses retained storage: scratch peaks at 96 + 131.
    // In a caller scope its 96 bytes add to that peak, giving 323.
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 96;
    policy.limits.max_materialized_bytes = 227;
    policy.limits.max_collection_items = 4;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let selected =
        super::super::unique_relation_declaration_candidates_charged(&ctx, &classes, &scalars, &[])
            .unwrap();
    assert_eq!(selected.len(), 1);
    assert!(std::ptr::eq(selected[0].0, &raw const classes[0]));
    assert!(std::ptr::eq(selected[0].1, &raw const scalars[0]));
    assert_eq!(
        selected[0].2,
        FeatureInputRelationFamily::PointPointHorizontalDistance
    );
    let released = ctx
        .reserve_scoped(227, "released declaration workspaces")
        .unwrap();
    drop(released);
    assert_eq!(ctx.resource_refusal(), None);
    drop(selected);

    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 323;
    policy.limits.max_collection_items = 4 * 64;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let storage;
        let selected;
        (selected, storage) = ctx
            .with_scoped_storage("selected declaration candidates", || {
                super::super::unique_relation_declaration_candidates_charged(
                    &ctx,
                    &classes,
                    &scalars,
                    &[],
                )
            })
            .unwrap();
        assert_eq!(selected.len(), 1);
        assert!(std::ptr::eq(selected[0].0, &raw const classes[0]));
        assert!(std::ptr::eq(selected[0].1, &raw const scalars[0]));
        assert_eq!(
            selected[0].2,
            FeatureInputRelationFamily::PointPointHorizontalDistance
        );
        let released = ctx
            .reserve_scoped(227, "released discarded declaration candidates")
            .unwrap();
        drop(released);
        drop(selected);
        drop(storage);
        let released = ctx
            .reserve_scoped(323, "released all declaration candidates")
            .unwrap();
        drop(released);
    }
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn selected_declaration_candidates_refuse_exact_storage_boundaries() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let classes = [class(0, "sgPntPntHorDist")];
    let scalars = [scalar(1, FeatureInputScalarRole::Driving)];
    for (dimension, cap, used, additional, operation, scoped) in [
        (
            ResourceDimension::RetainedBytes,
            95,
            0,
            96,
            "collect SLDPRT unique relations",
            false,
        ),
        (
            ResourceDimension::MaterializedBytes,
            226,
            96,
            131,
            "index SLDPRT relation candidates",
            false,
        ),
        (
            ResourceDimension::MaterializedBytes,
            322,
            227,
            96,
            "collect SLDPRT unique relations",
            true,
        ),
        (
            ResourceDimension::CollectionItems,
            3,
            3,
            1,
            "collect SLDPRT unique relations",
            false,
        ),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = if scoped { 0 } else { 96 };
        policy.limits.max_materialized_bytes = if scoped { 323 } else { 227 };
        policy.limits.max_collection_items = 4;
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            _ => panic!("declaration candidate storage dimension"),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = if scoped {
            ctx.with_scoped_storage("selected declaration boundary", || {
                super::super::unique_relation_declaration_candidates_charged(
                    &ctx,
                    &classes,
                    &scalars,
                    &[],
                )
            })
            .map(|(selected, storage)| {
                drop(selected);
                drop(storage);
            })
        } else {
            super::super::unique_relation_declaration_candidates_charged(
                &ctx,
                &classes,
                &scalars,
                &[],
            )
            .map(drop)
        };
        let Err(CodecError::ResourceLimit(limit)) = result else {
            panic!("selected candidate boundary must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.limit, cap);
        assert_eq!(limit.used, used);
        assert_eq!(limit.additional, additional);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(
            matches!(ctx.charge_work(0, "after declaration boundary refusal"),
            Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
}

#[test]
fn declaration_class_workspace_ends_before_scalar_sort_scratch() {
    const ITEMS: u64 = 1 + 1 + 1 + 21 + 2 * 21;
    const MATERIALIZED: u64 = 195 + 8 + 32 * 8 + 2 * 21 * 8;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let classes = [class(0, "sgPntPntHorDist")];
    let scalars = (1..=21)
        .rev()
        .map(|offset| scalar(offset, FeatureInputScalarRole::Driving))
        .collect::<Vec<_>>();
    let intervals = [(0, None, "sketch".to_owned())];
    // One hash group: four 40-byte buckets + 15-byte alignment allowance
    // + four control bytes + 16 trailing bytes, then one 8-byte offset.
    // Scalar Vec: 32 reference slots. Stable sort: two 21-usize arrays.
    // The consumed four-slot (u64, &str) class Vec is no longer live.
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = MATERIALIZED;
    policy.limits.max_collection_items = ITEMS * 64;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let storage;
        let index;
        (index, storage) = ctx
            .with_scoped_storage("index SLDPRT relation declarations", || {
                super::super::DeclarationIndex::new(&ctx, &classes, &scalars, &intervals)
            })
            .unwrap();
        assert_eq!(
            index
                .scalars
                .iter()
                .map(|value| value.offset)
                .collect::<Vec<_>>(),
            (1..=21).collect::<Vec<_>>()
        );
        assert!(std::ptr::eq(index.scalars[0], &raw const scalars[20]));
        assert_eq!(index.relation_class_offsets.get("sketch").unwrap(), &[0]);
        drop(index);
        drop(storage);
    }
    assert_eq!(ctx.resource_refusal(), None);
    ctx.reserve_scoped(MATERIALIZED, "released relation declaration index")
        .unwrap();

    policy.limits.max_materialized_bytes = MATERIALIZED - 1;
    policy.limits.max_collection_items = ITEMS;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(error) = ctx.with_scoped_storage("index SLDPRT relation declarations", || {
        super::super::DeclarationIndex::new(&ctx, &classes, &scalars, &intervals)
    }) else {
        panic!("one below the live index and sort scratch must refuse");
    };
    let CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal");
    };
    assert_eq!(limit.operation, "index SLDPRT relation declarations");
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.used, 195 + 8 + 32 * 8 + 21 * 8);
    assert_eq!(limit.additional, 21 * 8);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn sketch_case_fold_keeps_relation_identity_and_admits_comparison() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    for tag in ["Sketch", "sKeTcH", "SKETCH"] {
        let mut history = sketch_history();
        history[0].features[0].xml_tag = tag.into();
        let driving = scalar(20, FeatureInputScalarRole::Driving);
        let input = lane(vec![class(10, "sgPntPntHorDist")], vec![driving]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let instances = relation_instances(&ctx, &history, &input).unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one declared relation");
        };
        assert_eq!(
            relation.family,
            FeatureInputRelationFamily::PointPointHorizontalDistance
        );
        assert_eq!(relation.scalar_refs(), ["scalar-20"]);
        assert_eq!(relation.feature_ref, "sketch");
        assert_eq!(ctx.resource_refusal(), None);
        let CodecError::ResourceLimit(limit) =
            crate::test_support::work_refusal_at("match SLDPRT sketch feature tags", |ctx| {
                relation_instances(ctx, &history, &input)
            })
        else {
            panic!("case-fold comparison must refuse");
        };
        assert_eq!(limit.operation, "match SLDPRT sketch feature tags");
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.additional, 6);
    }
}

#[test]
fn native_relation_tags_are_scoped_to_the_declared_family() {
    let operand_pair = |kind| {
        [0_u16, 1]
            .into_iter()
            .enumerate()
            .map(|(ordinal, entity_index)| FeatureInputOperand {
                offset: cadmpeg_core::decode::u64_from_index(ordinal),
                reference_ref: format!("reference-{ordinal}"),
                kind,
                entity_index,
                entity_ref: None,
            })
            .collect::<Vec<_>>()
    };
    let point_tagged = operand_pair(FeatureInputOperandKind::Native(NativeOperandTag::TAG_80D5));
    let line_tagged = operand_pair(FeatureInputOperandKind::Native(NativeOperandTag::TAG_810F));
    let roster_point_tagged =
        operand_pair(FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD));
    let roster_line_tagged =
        operand_pair(FeatureInputOperandKind::Native(NativeOperandTag::TAG_81E7));

    assert!(relation_signature(
        FeatureInputRelationFamily::PointPointDistance,
        &point_tagged
    ));
    assert!(relation_signature(
        FeatureInputRelationFamily::PointPointHorizontalDistance,
        &point_tagged
    ));
    assert!(relation_signature(
        FeatureInputRelationFamily::PointPointVerticalDistance,
        &point_tagged
    ));
    assert!(relation_signature(
        FeatureInputRelationFamily::Angle,
        &point_tagged
    ));
    assert!(relation_signature(
        FeatureInputRelationFamily::LineLineDistance,
        &line_tagged
    ));
    assert!(relation_signature(
        FeatureInputRelationFamily::PointPointDistance,
        &roster_point_tagged
    ));
    assert!(relation_signature(
        FeatureInputRelationFamily::LineLineDistance,
        &roster_line_tagged
    ));
    assert!(relation_signature(
        FeatureInputRelationFamily::PointLineDistance,
        &[
            FeatureInputOperand {
                kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD),
                ..roster_point_tagged[0].clone()
            },
            FeatureInputOperand {
                kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_81E7),
                ..roster_line_tagged[1].clone()
            },
        ]
    ));

    for tag in [0x8138, 0x80ac] {
        let point_distance_tagged =
            operand_pair(FeatureInputOperandKind::Native(tag.try_into().unwrap()));
        assert!(relation_signature(
            FeatureInputRelationFamily::PointPointDistance,
            &point_distance_tagged
        ));
        assert!(!relation_signature(
            FeatureInputRelationFamily::PointPointHorizontalDistance,
            &point_distance_tagged
        ));
        assert!(!relation_signature(
            FeatureInputRelationFamily::PointPointVerticalDistance,
            &point_distance_tagged
        ));
        assert!(!relation_signature(
            FeatureInputRelationFamily::Angle,
            &point_distance_tagged
        ));
    }

    assert!(!relation_signature(
        FeatureInputRelationFamily::PointPointDistance,
        &line_tagged
    ));
    assert!(!relation_signature(
        FeatureInputRelationFamily::LineLineDistance,
        &point_tagged
    ));
    assert!(!relation_signature(
        FeatureInputRelationFamily::PointLineDistance,
        &point_tagged
    ));

    for tag in [0x8100, 0x820f] {
        let solver_point_tagged =
            operand_pair(FeatureInputOperandKind::Native(tag.try_into().unwrap()));
        assert!(relation_signature(
            FeatureInputRelationFamily::PointPointDistance,
            &solver_point_tagged
        ));
        assert!(!relation_signature(
            FeatureInputRelationFamily::PointPointHorizontalDistance,
            &solver_point_tagged
        ));
        assert!(solver_point_tagged
            .iter()
            .all(|operand| is_solver_point_operand(operand.kind)));
    }
}

/// A class whose unknown-feature span no `u64` can name states no scope,
/// so it declares no relation. The control states the same shape at an
/// offset the span can name and declares one.
#[test]
fn a_class_whose_unknown_feature_span_is_unstatable_declares_no_relation() {
    let unstatable = class(u64::MAX - 100, "sgPntPntHorDist");
    let following = scalar(u64::MAX - 50, FeatureInputScalarRole::Driving);
    assert!(relation_declaration_candidates(
        &cadmpeg_test_support::service_decode_context(),
        std::slice::from_ref(&unstatable),
        std::slice::from_ref(&following),
        &[],
    )
    .unwrap()
    .is_empty());

    let stated = class(1_000, "sgPntPntHorDist");
    let within = scalar(1_050, FeatureInputScalarRole::Driving);
    assert_eq!(
        relation_declaration_candidates(
            &cadmpeg_test_support::service_decode_context(),
            std::slice::from_ref(&stated),
            std::slice::from_ref(&within),
            &[],
        )
        .unwrap()
        .len(),
        1
    );
}

#[test]
fn declaration_skips_nearer_incompatible_scalar() {
    let relation_class = class(10, "sgPntPntHorDist");
    let mut native = scalar(20, FeatureInputScalarRole::Native);
    native.operands.clear();

    let driving = scalar(40, FeatureInputScalarRole::Driving);
    let lane = lane(vec![relation_class], vec![native, driving.clone()]);

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one relation instance");
    };
    assert_eq!(
        relation.parameter_scalar_ref().map(str::to_owned),
        Some(driving.id.clone())
    );
    assert_eq!(relation.scalar_refs(), vec![driving.id]);
}

#[test]
fn declaration_reaches_compatible_scalar_after_auxiliary_records() {
    let relation_class = class(10, "sgPntPntHorDist");
    let driving = scalar(200, FeatureInputScalarRole::Driving);
    let mut lane = lane(vec![relation_class], vec![driving.clone()]);
    lane.names.push(FeatureInputName {
        id: "name-sketch".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        object_id: None,
        value: "Sketch".into(),
    });

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one relation instance");
    };
    assert_eq!(
        relation.parameter_scalar_ref().map(str::to_owned),
        Some(driving.id.clone())
    );
    assert_eq!(relation.scalar_refs(), vec![driving.id]);
}

#[test]
fn declaration_stops_before_the_next_relation_class() {
    let first = class(10, "sgPntPntHorDist");
    let second = class(100, "sgPntPntVertDist");
    let driving = scalar(200, FeatureInputScalarRole::Driving);
    let mut lane = lane(vec![first, second.clone()], vec![driving]);
    lane.names.push(FeatureInputName {
        id: "name-sketch".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        object_id: None,
        value: "Sketch".into(),
    });

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one relation instance");
    };
    assert_eq!(relation.class_ref, second.id);
    assert_eq!(
        relation.family,
        FeatureInputRelationFamily::PointPointVerticalDistance
    );
}

#[test]
fn display_scalar_joins_driving_scalar_after_class_declaration() {
    let vertical = class(10, "sgPntPntVertDist");
    let horizontal = class(30, "sgPntPntHorDist");
    let display = scalar(20, FeatureInputScalarRole::Display);
    let driving = scalar(40, FeatureInputScalarRole::Driving);
    let lane = lane(
        vec![vertical, horizontal.clone()],
        vec![display.clone(), driving.clone()],
    );

    let history = sketch_history();
    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &history,
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one relation instance");
    };
    assert_eq!(
        relation.family,
        FeatureInputRelationFamily::PointPointHorizontalDistance
    );
    assert_eq!(relation.class_ref, horizontal.id);
    assert_eq!(
        relation.scalar_refs(),
        vec![display.id.clone(), driving.id.clone()]
    );
    assert_eq!(
        relation.display_scalar_ref().map(str::to_owned),
        Some(display.id)
    );
    assert_eq!(
        relation.parameter_scalar_ref().map(str::to_owned),
        Some(driving.id)
    );
}

#[test]
fn display_scalars_do_not_cross_a_class_declaration() {
    let vertical = class(10, "sgPntPntVertDist");
    let horizontal = class(30, "sgPntPntHorDist");
    let first = scalar(20, FeatureInputScalarRole::Display);
    let second = scalar(40, FeatureInputScalarRole::Display);
    let lane = lane(vec![vertical, horizontal], vec![first, second]);

    let relations = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    assert_eq!(relations.len(), 2);
    assert_eq!(
        relations
            .iter()
            .map(|relation| relation.family)
            .collect::<Vec<_>>(),
        vec![
            FeatureInputRelationFamily::PointPointVerticalDistance,
            FeatureInputRelationFamily::PointPointHorizontalDistance,
        ]
    );
}

#[test]
fn ambiguous_relation_declarations_leave_scalar_unbound() {
    let distance = class(10, "sgPntPntDist");
    let vertical = class(20, "sgPntPntVertDist");
    let lane = lane(
        vec![distance, vertical],
        vec![scalar(30, FeatureInputScalarRole::Driving)],
    );

    assert!(relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane
    )
    .unwrap()
    .is_empty());
}

/// Two features, so the first interval ends at the second's start. A class
/// inside the first interval reaches a scalar past its own
/// unknown-feature span but inside the interval: the scope the class takes
/// is the interval's end, not the span.
#[test]
fn a_class_inside_an_interval_takes_the_interval_end_not_the_unknown_span() {
    let mut history = sketch_history();
    history[0].features[0].id = "first".into();
    history[0].features[0].name = "First".into();
    let mut second = history[0].features[0].clone();
    second.id = "second".into();
    second.name = "Second".into();
    second.ordinal = 1;
    history[0].features.push(second);

    let mut relation_scalar = scalar(150, FeatureInputScalarRole::Driving);
    relation_scalar.feature_ref = Some("first".into());
    let mut lane = lane(
        vec![class(10, "sgPntPntHorDist")],
        vec![relation_scalar.clone()],
    );
    lane.names = vec![
        FeatureInputName {
            id: "name-first".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: None,
            value: "First".into(),
        },
        FeatureInputName {
            id: "name-second".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 200,
            object_id: None,
            value: "Second".into(),
        },
    ];

    let intervals = feature_intervals(
        &cadmpeg_test_support::service_decode_context(),
        &history,
        &lane,
    )
    .unwrap();
    assert_eq!(
        intervals,
        vec![
            (0, Some(200), "first".to_owned()),
            (200, None, "second".to_owned()),
        ]
    );
    // 150 is past 10 + UNKNOWN_FEATURE_SPAN and inside the interval.
    assert!(relation_scalar.offset > 10 + UNKNOWN_FEATURE_SPAN);
    assert_eq!(
        relation_declaration_candidates(
            &cadmpeg_test_support::service_decode_context(),
            &lane.classes,
            &lane.scalars,
            &intervals
        )
        .unwrap()
        .len(),
        1
    );
}

/// The last feature interval is open, so a class inside it is bounded by
/// nothing and reaches a scalar at the highest offset a `u64` states.
#[test]
fn a_class_in_the_open_last_interval_reaches_the_highest_scalar_offset() {
    let history = sketch_history();
    let mut relation_scalar = scalar(10, FeatureInputScalarRole::Driving);
    relation_scalar.offset = u64::MAX;
    let mut lane = lane(
        vec![class(u64::MAX - 1, "sgPntPntHorDist")],
        vec![relation_scalar],
    );
    lane.names = vec![FeatureInputName {
        id: "name-sketch".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        object_id: None,
        value: "Sketch".into(),
    }];

    let intervals = feature_intervals(
        &cadmpeg_test_support::service_decode_context(),
        &history,
        &lane,
    )
    .unwrap();
    assert_eq!(
        relation_declaration_candidates(
            &cadmpeg_test_support::service_decode_context(),
            &lane.classes,
            &lane.scalars,
            &intervals
        )
        .unwrap()
        .len(),
        1,
        "the open last interval bounds no scalar offset"
    );
    assert_eq!(intervals, vec![(0, None, "sketch".to_owned())]);
}

#[test]
fn relation_declarations_do_not_cross_feature_intervals() {
    let mut history = sketch_history();
    history[0].features[0].id = "first".into();
    history[0].features[0].name = "First".into();
    let mut second = history[0].features[0].clone();
    second.id = "second".into();
    second.name = "Second".into();
    second.ordinal = 1;
    history[0].features.push(second);

    let mut relation_scalar = scalar(120, FeatureInputScalarRole::Driving);
    relation_scalar.feature_ref = Some("second".into());
    let mut lane = lane(vec![class(10, "sgPntPntDist")], vec![relation_scalar]);
    lane.names = vec![
        FeatureInputName {
            id: "name-first".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: None,
            value: "First".into(),
        },
        FeatureInputName {
            id: "name-second".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 100,
            object_id: None,
            value: "Second".into(),
        },
    ];

    assert!(relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &history,
        &lane
    )
    .unwrap()
    .is_empty());
}

#[test]
fn metadata_records_do_not_split_relation_feature_intervals() {
    let mut history = sketch_history();
    let mut metadata = history[0].features[0].clone();
    metadata.id = "attribute-definition".into();
    metadata.xml_tag = "Feature".into();
    metadata.source_id = Some(FeatureSource::Reserved);
    metadata.ordinal = 1;
    metadata.name = "Attribute-Definition".into();
    metadata.kind = "Attribute-Definition".into();
    metadata.input_class = None;
    history[0].features.push(metadata);

    let mut relation_scalar = scalar(120, FeatureInputScalarRole::Driving);
    relation_scalar.feature_ref = Some("sketch".into());
    let mut lane = lane(vec![class(110, "sgPntPntDist")], vec![relation_scalar]);
    lane.names = vec![
        FeatureInputName {
            id: "name-sketch".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: None,
            value: "Sketch".into(),
        },
        FeatureInputName {
            id: "name-attribute".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 100,
            object_id: None,
            value: "Attribute-Definition".into(),
        },
    ];

    let relations = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &history,
        &lane,
    )
    .unwrap();
    let [relation] = relations.as_slice() else {
        panic!("metadata must not terminate the sketch interval");
    };
    assert_eq!(relation.feature_ref, "sketch");
}

#[test]
fn repeated_driving_scalar_starts_another_anchored_relation() {
    let lane = lane(
        vec![class(10, "sgPntPntDist")],
        vec![
            scalar(20, FeatureInputScalarRole::Display),
            scalar(30, FeatureInputScalarRole::Driving),
            scalar(40, FeatureInputScalarRole::Driving),
        ],
    );

    let relations = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    assert_eq!(relations.len(), 2);
    assert_eq!(
        relations
            .iter()
            .map(|relation| relation.scalar_refs().len())
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert!(relations.iter().all(|relation| {
        relation.family == FeatureInputRelationFamily::PointPointDistance
            && relation.class_ref == "class-10"
    }));
}

#[test]
fn unclaimed_relation_binding_scalar_becomes_an_instance() {
    let dynamic = dynamic_scalar(
        20,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[1, 2],
        2.0,
    );
    let mut bound = dynamic_scalar(30, FeatureInputOperandKind::E1, &[3, 4], 3.0);
    bound.role = FeatureInputScalarRole::Display;
    let mut lane = lane(vec![class(10, "sgLLDist")], vec![dynamic, bound.clone()]);
    lane.relation_bindings = vec![FeatureInputRelationBinding {
        id: "binding-10".into(),
        parent: lane.id.clone(),
        ordinal: 0,
        offset: 10,
        class_ref: "class-10".into(),
        family: FeatureInputRelationFamily::LineLineDistance,
        scalar_ref: bound.id,
        feature_ref: Some("sketch".into()),
    }];
    lane.relation_bindings.push(FeatureInputRelationBinding {
        id: "binding-11".into(),
        parent: lane.id.clone(),
        ordinal: 1,
        offset: 11,
        class_ref: "class-11".into(),
        family: FeatureInputRelationFamily::LineLineDistance,
        scalar_ref: "scalar-30".into(),
        feature_ref: Some("sketch".into()),
    });

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    assert_eq!(instances.len(), 2);
    assert_eq!(instances[1].offset, 30);
    assert_eq!(
        instances[1].family,
        FeatureInputRelationFamily::LineLineDistance
    );
    assert_eq!(instances[1].scalar_refs(), vec!["scalar-30"]);
    assert_eq!(instances[1].display_scalar_ref(), Some("scalar-30"));
}

#[test]
fn dynamic_relation_tags_are_admitted_by_family_scope() {
    let lane = lane(
        vec![class(10, "sgPntPntDist")],
        vec![dynamic_scalar(
            40,
            FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
            &[0, 1],
            2.0,
        )],
    );

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one dynamically tagged relation");
    };
    assert_eq!(
        relation.family,
        FeatureInputRelationFamily::PointPointDistance
    );
    assert!(relation_uses_dynamic_operands(relation));
}

#[test]
fn declaration_signature_rejection_leaves_feature_probe_armed() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let classes = [class(10, "sgPntPntDist")];
    let mut candidate = scalar(20, FeatureInputScalarRole::Driving);
    for operand in &mut candidate.operands {
        operand.kind = FeatureInputOperandKind::E1;
    }
    let scalars = [candidate];
    let intervals = [(0, None, "sketch".to_owned())];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let index = super::super::DeclarationIndex::new(&ctx, &classes, &scalars, &intervals).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "compare SLDPRT relation declaration feature references",
        None,
    );
    assert!(index
        .declaration(
            &ctx,
            &classes[0],
            super::super::feature_at_offset(&ctx, classes[0].offset, &intervals).unwrap(),
            false
        )
        .unwrap()
        .is_none());
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(limit)) =
        ctx.charge_work(1, "compare SLDPRT relation declaration feature references")
    else {
        panic!("wrong signature must leave feature comparison probe armed");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(
        limit.operation,
        "compare SLDPRT relation declaration feature references"
    );
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.charge_work(0, "after unused declaration feature"),
        Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
    drop(probe);
}

#[test]
fn relation_group_fixed_rejection_skips_operand_signature() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    for trigger in 0..3 {
        let first = scalar(20, FeatureInputScalarRole::Display);
        let mut second = scalar(40, FeatureInputScalarRole::Driving);
        let mut input = lane(vec![class(10, "sgPntPntDist")], vec![first.clone()]);
        match trigger {
            0 => {
                let mut intervening = scalar(30, FeatureInputScalarRole::Native);
                intervening.feature_ref = None;
                input.scalars.push(intervening);
            }
            1 => second.role = FeatureInputScalarRole::Native,
            2 => input.classes.push(class(30, "sgCircleDim")),
            _ => unreachable!(),
        }
        input.scalars.push(second);
        let expected = crate::records::FeatureInputRelationInstance {
            id: "sldprt:feature-input:relation-instance#lane:20".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 20,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class-10".into(),
            feature_ref: "sketch".into(),
            scalars: crate::records::relation_scalars::RelationScalars::from_refs(
                vec![first.id.clone()],
                None,
                Some(first.id.clone()),
            )
            .unwrap(),
            operands: first.operands.clone(),
        };
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "compare SLDPRT relation operand signatures",
            None,
        );
        let relations = relation_instances(&ctx, &sketch_history(), &input).unwrap();
        assert_eq!(relations, [expected]);
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(limit)) =
            ctx.charge_work(1, "compare SLDPRT relation operand signatures")
        else {
            panic!("fixed group rejection must leave signature probe armed");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(
            limit.operation,
            "compare SLDPRT relation operand signatures"
        );
        assert_eq!(limit.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.charge_work(0, "after unused group signature"),
            Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
        drop(probe);
    }
}

#[test]
fn promoted_group_reuses_proved_class_difference() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let first = scalar(20, FeatureInputScalarRole::Display);
    let second = scalar(40, FeatureInputScalarRole::Driving);
    let input = lane(
        vec![class(10, "sgPntPntDist"), class(30, "sgPntPntDist")],
        vec![first.clone(), second.clone()],
    );
    let expected = crate::records::FeatureInputRelationInstance {
        id: "sldprt:feature-input:relation-instance#lane:20".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 20,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class-30".into(),
        feature_ref: "sketch".into(),
        scalars: crate::records::relation_scalars::RelationScalars::from_refs(
            vec![first.id.clone(), second.id.clone()],
            Some(second.id.clone()),
            Some(first.id.clone()),
        )
        .unwrap(),
        operands: first.operands.clone(),
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "compare SLDPRT relation group classes",
        None,
    );
    assert_eq!(
        relation_instances(&ctx, &sketch_history(), &input).unwrap(),
        [expected]
    );
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(limit)) =
        ctx.charge_work(1, "compare SLDPRT relation group classes")
    else {
        panic!("proved promotion must leave duplicate class comparison probe armed");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "compare SLDPRT relation group classes");
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.charge_work(0, "after unused promoted class comparison"),
        Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
    drop(probe);
}

#[test]
fn unstatable_declaration_does_not_demand_an_index() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let classes = [class(u64::MAX - 100, "sgPntPntHorDist")];
    let scalars = [scalar(u64::MAX - 50, FeatureInputScalarRole::Driving)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    // One admitted class visit per call; the empty interval search visits nothing.
    policy.limits.max_work_units = 64;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        assert!(
            super::super::unique_relation_declaration_candidates_charged(
                &ctx,
                &classes,
                &scalars,
                &[]
            )
            .unwrap()
            .is_empty()
        );
    }
    assert_eq!(ctx.resource_refusal(), None);

    policy.limits.max_work_units = u64::MAX;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "index SLDPRT relation declarations",
        None,
    );
    assert!(
        super::super::unique_relation_declaration_candidates_charged(&ctx, &classes, &scalars, &[])
            .unwrap()
            .is_empty()
    );
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(limit)) =
        ctx.charge_work(1, "index SLDPRT relation declarations")
    else {
        panic!("unstatable scope must leave index probe armed");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "index SLDPRT relation declarations");
    assert_eq!(limit.used, 1);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.charge_work(0, "after unused declaration index"),
        Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
    drop(probe);

    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) =
        super::super::unique_relation_declaration_candidates_charged(&ctx, &classes, &scalars, &[])
    else {
        panic!("unstatable class source must still be admitted");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "scan SLDPRT relation declarations");
    assert_eq!(limit.limit, 0);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.charge_work(0, "after unstatable class visit refusal"),
        Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}

#[test]
fn open_interval_still_demands_the_declaration_index_at_high_offsets() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let classes = [class(u64::MAX - 100, "sgPntPntHorDist")];
    let scalars = [scalar(u64::MAX - 50, FeatureInputScalarRole::Driving)];
    let intervals = [(0, None, "sketch".to_owned())];
    let service = cadmpeg_test_support::service_decode_context();
    let selected = super::super::unique_relation_declaration_candidates_charged(
        &service, &classes, &scalars, &intervals,
    )
    .unwrap();
    assert_eq!(selected.len(), 1);
    assert!(std::ptr::eq(selected[0].0, &raw const classes[0]));
    assert!(std::ptr::eq(selected[0].1, &raw const scalars[0]));
    assert_eq!(
        selected[0].2,
        FeatureInputRelationFamily::PointPointHorizontalDistance
    );
    assert_eq!(service.resource_refusal(), None);

    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "index SLDPRT relation declarations",
        None,
    );
    let Err(CodecError::ResourceLimit(limit)) =
        super::super::unique_relation_declaration_candidates_charged(
            &ctx, &classes, &scalars, &intervals,
        )
    else {
        panic!("open interval must demand its valid declaration index");
    };
    // Outer class visit and one actual interval probe precede the index's class visit.
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "index SLDPRT relation declarations");
    assert_eq!(limit.used, 2);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.charge_work(0, "after required declaration index"),
        Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
    drop(probe);
}

#[test]
fn rejected_unknown_span_does_not_suppress_a_later_valid_declaration() {
    let classes = [
        class(u64::MAX - 100, "sgPntPntHorDist"),
        class(1000, "sgPntPntHorDist"),
    ];
    let scalars = [
        scalar(u64::MAX - 50, FeatureInputScalarRole::Driving),
        scalar(1050, FeatureInputScalarRole::Driving),
    ];
    let ctx = cadmpeg_test_support::service_decode_context();
    let selected =
        super::super::unique_relation_declaration_candidates_charged(&ctx, &classes, &scalars, &[])
            .unwrap();
    assert_eq!(selected.len(), 1);
    assert!(std::ptr::eq(selected[0].0, &raw const classes[1]));
    assert!(std::ptr::eq(selected[0].1, &raw const scalars[1]));
    assert_eq!(
        selected[0].2,
        FeatureInputRelationFamily::PointPointHorizontalDistance
    );
    assert_eq!(ctx.resource_refusal(), None);
}

fn singleton_group_fixture() -> (
    FeatureInputLane,
    crate::records::FeatureInputRelationInstance,
) {
    let source = scalar(20, FeatureInputScalarRole::Driving);
    let expected = crate::records::FeatureInputRelationInstance {
        id: "sldprt:feature-input:relation-instance#lane:20".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 20,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class-10".into(),
        feature_ref: "sketch".into(),
        scalars: crate::records::relation_scalars::RelationScalars::from_refs(
            vec![source.id.clone()],
            Some(source.id.clone()),
            None,
        )
        .unwrap(),
        operands: source.operands.clone(),
    };
    (
        lane(vec![class(10, "sgPntPntDist")], vec![source]),
        expected,
    )
}

#[test]
fn relation_group_phases_release_indexes_before_output_and_bindings() {
    const MATERIALIZED: u64 = 99 + 32 + 131 + 227 + 4 * 80 + 16;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    assert_eq!(std::mem::size_of::<super::super::RelationGroup<'_>>(), 80);
    assert_eq!(
        std::mem::size_of::<(&str, (u64, FeatureInputRelationFamily, &str))>(),
        48
    );
    assert_eq!(std::mem::size_of::<(usize, &FeatureInputScalar)>(), 16);
    let (input, expected) = singleton_group_fixture();
    let histories = sketch_history();
    let mut policy = DecodePolicy::service();
    // Sketch set99, class offsets32, counts131, declarations227,
    // four80-byte group slots and one exact16-byte scalar slot.
    policy.limits.max_materialized_bytes = MATERIALIZED;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let output = relation_instances(&ctx, &histories, &input).unwrap();
        assert_eq!(output.as_slice(), std::slice::from_ref(&expected));
        let released = ctx
            .reserve_scoped(MATERIALIZED, "released relation grouping phases")
            .unwrap();
        drop(released);
    }
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn relation_group_phases_refuse_actual_first_scalar_slot() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (input, _) = singleton_group_fixture();
    let histories = sketch_history();
    for (dimension, cap, used, additional) in [
        (ResourceDimension::MaterializedBytes, 824, 809, 16),
        (ResourceDimension::CollectionItems, 7, 7, 1),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 825;
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            _ => panic!("grouping resource dimension"),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) = relation_instances(&ctx, &histories, &input)
        else {
            panic!("group scalar backing must be admitted before publication");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, "collect SLDPRT relation scalar groups");
        assert_eq!(limit.limit, cap);
        assert_eq!(limit.used, used);
        assert_eq!(limit.additional, additional);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(
            matches!(ctx.charge_work(0, "after grouping slot refusal"), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
}
