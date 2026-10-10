use super::*;

#[test]
fn line_angles_retain_shallow_and_near_opposite_angles() {
    const ANGLE: f64 = 1e-8;
    let first = [[0.0, 0.0], [1.0, 0.0]];
    let second = [[0.0, 0.0], [ANGLE.cos(), ANGLE.sin()]];
    assert!(
        (super::super::line_line_angle(first, second).unwrap() - ANGLE).abs() <= f64::EPSILON * ANGLE
    );
    let opposite = [[0.0, 0.0], [-ANGLE.cos(), ANGLE.sin()]];
    assert!(
        (super::super::line_line_angle(first, opposite).unwrap() - (std::f64::consts::PI - ANGLE))
            .abs()
            <= f64::EPSILON * std::f64::consts::PI
    );
}

#[test]
fn numerical_audit_line_angles_are_finite_for_large_directions() {
    let x = [[0.0, 0.0], [1.0e200, 0.0]];
    let y = [[0.0, 0.0], [0.0, 1.0e200]];
    assert_eq!(super::super::line_line_angle(x, x), Some(0.0));
    assert_eq!(
        super::super::line_line_angle(x, y),
        Some(std::f64::consts::FRAC_PI_2)
    );
    assert_eq!(super::super::dynamic_line_line_angle(x, x), Some(0.0));
}

fn sketch_marker(
    id: &str,
    ordinal: u32,
    offset: u64,
    kind: crate::records::SketchInputKind,
    coordinates_m: Option<[f64; 2]>,
) -> crate::records::SketchInputEntity {
    let mut marker = crate::records::SketchInputEntity::new(id, "lane", ordinal, offset, kind);
    marker.feature_ref = Some("sketch".into());
    marker.coordinates_m = coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
    marker
}

fn dynamic_point_markers() -> Vec<crate::records::SketchInputEntity> {
    vec![
        sketch_marker(
            "relation",
            0,
            1,
            crate::records::SketchInputKind::Relation(
                crate::records::SketchRelationKind::Distance,
            ),
            None,
        ),
        sketch_marker(
            "p0",
            1,
            10,
            crate::records::SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        sketch_marker(
            "p1",
            2,
            20,
            crate::records::SketchInputKind::Point,
            Some([1.0, 0.0]),
        ),
        sketch_marker(
            "p2",
            3,
            30,
            crate::records::SketchInputKind::Point,
            Some([3.0, 0.0]),
        ),
    ]
}

#[test]
fn dynamic_point_relation_uses_unique_geometry_match_across_address_tiers() {
    let mut lane = lane(
        vec![class(10, "sgPntPntDist")],
        vec![dynamic_scalar(
            40,
            FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
            &[1, 2],
            2.0,
        )],
    );
    lane.sketch_entities = dynamic_point_markers();

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
        relation
            .operands
            .iter()
            .map(|operand| operand.entity_ref.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("p1"), Some("p2")]
    );
}

#[test]
fn dynamic_point_relation_preserves_explicit_driving_operand_reference() {
    let mut driving = dynamic_scalar(
        40,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[1, 2],
        2.0,
    );
    driving.operands[0].entity_ref = Some("p1".into());
    let mut lane = lane(vec![class(10, "sgPntPntDist")], vec![driving]);
    lane.sketch_entities = dynamic_point_markers();

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
        relation
            .operands
            .iter()
            .map(|operand| operand.entity_ref.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("p1"), Some("p2")]
    );
}

#[test]
fn dynamic_point_relation_falls_back_to_unique_geometry_after_address_miss() {
    let mut driving = dynamic_scalar(
        40,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[3, 1],
        1.0,
    );
    driving.operands[0].entity_ref = Some("p0".into());
    let mut lane = lane(vec![class(10, "sgPntPntDist")], vec![driving]);
    lane.sketch_entities = vec![
        sketch_marker(
            "relation",
            0,
            1,
            crate::records::SketchInputKind::Relation(
                crate::records::SketchRelationKind::Distance,
            ),
            None,
        ),
        sketch_marker(
            "p0",
            1,
            10,
            crate::records::SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        sketch_marker(
            "p1",
            2,
            20,
            crate::records::SketchInputKind::Point,
            Some([1.0, 1.0]),
        ),
        sketch_marker(
            "p2",
            3,
            30,
            crate::records::SketchInputKind::Point,
            Some([1.0, 0.0]),
        ),
    ];

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
        relation
            .operands
            .iter()
            .map(|operand| operand.entity_ref.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("p0"), Some("p2")]
    );
}

#[test]
fn relation_instance_keeps_first_scalar_operands_when_grouping_display_and_driver() {
    let mut display = dynamic_scalar(
        20,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[1, 2],
        2.0,
    );
    display.role = FeatureInputScalarRole::Display;
    display.operands[0].entity_ref = Some("p1".into());
    let mut driving = dynamic_scalar(
        30,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[1, 2],
        2.0,
    );
    driving.operands[0].entity_ref = Some("p0".into());
    let mut lane = lane(
        vec![class(10, "sgPntPntDist")],
        vec![display, driving.clone()],
    );
    lane.sketch_entities = dynamic_point_markers();

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one grouped relation instance");
    };
    assert_eq!(
        relation.parameter_scalar_ref().map(str::to_owned),
        Some(driving.id)
    );
    assert_eq!(
        relation
            .operands
            .iter()
            .map(|operand| operand.entity_ref.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("p1"), Some("p2")]
    );
}

#[test]
fn dynamic_display_relation_uses_display_value_without_driver() {
    let mut display = dynamic_scalar(
        40,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[1, 2],
        2.0,
    );
    display.role = FeatureInputScalarRole::Display;
    let mut lane = lane(vec![class(10, "sgPntPntDist")], vec![display]);
    lane.sketch_entities = dynamic_point_markers();

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one dynamically tagged relation");
    };
    assert!(relation.parameter_scalar_ref().is_none());
    assert_eq!(
        relation
            .operands
            .iter()
            .map(|operand| operand.entity_ref.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("p1"), Some("p2")]
    );
}

#[test]
fn dynamic_point_relation_with_ambiguous_geometry_stays_unbound() {
    let mut lane = lane(
        vec![class(10, "sgPntPntDist")],
        vec![dynamic_scalar(
            40,
            FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
            &[1, 2],
            1.0,
        )],
    );
    lane.sketch_entities = dynamic_point_markers();
    lane.sketch_entities[3].coordinates_m = cadmpeg_ir::units::FiniteVector::new([1.0, 0.0]);

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one dynamically tagged relation");
    };
    assert!(relation
        .operands
        .iter()
        .all(|operand| operand.entity_ref.is_none()));
}

#[test]
fn dynamic_point_line_relation_resolves_solver_line_by_exact_distance() {
    let mut driving = dynamic_scalar(
        40,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[0, 1],
        1.0,
    );
    driving.operands[1].entity_ref = Some("p0".into());
    let mut lane = lane(vec![class(10, "sgPntLineDist")], vec![driving]);
    lane.sketch_entities = vec![
        sketch_marker(
            "relation",
            0,
            1,
            crate::records::SketchInputKind::Relation(
                crate::records::SketchRelationKind::Distance,
            ),
            None,
        ),
        sketch_marker(
            "p0",
            1,
            10,
            crate::records::SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        sketch_marker(
            "p1",
            2,
            20,
            crate::records::SketchInputKind::Point,
            Some([1.0, 0.0]),
        ),
        sketch_marker(
            "p2",
            3,
            30,
            crate::records::SketchInputKind::Point,
            Some([0.0, 1.0]),
        ),
        sketch_marker(
            "p3",
            4,
            40,
            crate::records::SketchInputKind::Point,
            Some([1.0, 1.0]),
        ),
    ];

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one dynamically tagged relation");
    };
    assert_eq!(relation.operands[0].entity_ref.as_deref(), Some("p0"));
    assert!(relation.operands[1].entity_ref.is_none());
}

#[test]
fn dynamic_line_relation_validates_solver_line_pair() {
    let mut driving = dynamic_scalar(
        40,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[0, 1],
        1.0,
    );
    driving.operands[0].entity_ref = Some("p0".into());
    let mut lane = lane(vec![class(10, "sgLLDist")], vec![driving]);
    lane.sketch_entities = vec![
        sketch_marker(
            "p0",
            0,
            10,
            crate::records::SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        sketch_marker(
            "p1",
            1,
            20,
            crate::records::SketchInputKind::Point,
            Some([1.0, 0.0]),
        ),
        sketch_marker(
            "p2",
            2,
            30,
            crate::records::SketchInputKind::Point,
            Some([0.0, 1.0]),
        ),
        sketch_marker(
            "p3",
            3,
            40,
            crate::records::SketchInputKind::Point,
            Some([1.0, 1.0]),
        ),
    ];

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one dynamically tagged relation");
    };
    assert!(relation
        .operands
        .iter()
        .all(|operand| operand.entity_ref.is_none()));
}

#[test]
fn dynamic_angle_accepts_reversed_solver_line_direction() {
    let driving = dynamic_scalar(
        40,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[0, 1],
        std::f64::consts::FRAC_PI_4,
    );
    let mut lane = lane(vec![class(10, "sgAnglDim")], vec![driving]);
    lane.sketch_entities = vec![
        sketch_marker(
            "first-start",
            0,
            10,
            crate::records::SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        sketch_marker(
            "first-end",
            1,
            20,
            crate::records::SketchInputKind::Point,
            Some([1.0, 0.0]),
        ),
        sketch_marker(
            "second-start",
            2,
            30,
            crate::records::SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        sketch_marker(
            "second-end",
            3,
            40,
            crate::records::SketchInputKind::Point,
            Some([-1.0, 1.0]),
        ),
    ];

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one dynamically tagged angle relation");
    };
    assert!(relation
        .operands
        .iter()
        .all(|operand| operand.entity_ref.is_none()));
}

fn explicit_point_relation() -> (
    [crate::records::SketchInputEntity; 2],
    crate::records::FeatureInputRelationInstance,
) {
    let points = [
        sketch_marker("a", 0, 0, crate::records::SketchInputKind::Point, Some([0.0, 0.0])),
        sketch_marker("b", 1, 1, crate::records::SketchInputKind::Point, Some([1.0, 0.0])),
    ];
    let mut source = dynamic_scalar(2, FeatureInputOperandKind::D6, &[0, 1], 1.0);
    source.operands[0].entity_ref = Some("a".into());
    source.operands[1].entity_ref = Some("b".into());
    let relation = crate::records::FeatureInputRelationInstance {
        id: "relation".into(), parent: "lane".into(), ordinal: 0, offset: 0,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class".into(), feature_ref: "sketch".into(),
        scalars: crate::records::relation_scalars::RelationScalars::from_refs(
            vec![source.id], None, None,
        ).unwrap(),
        operands: source.operands,
    };
    (points, relation)
}

#[test]
fn strict_point_match_does_not_materialize_coordinate_fallback() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (points, expected) = explicit_point_relation();
    let roster = [&points[0], &points[1]];
    let mut policy = DecodePolicy::service();
    // Two four-slot borrowed candidate vectors: 32 + 32 bytes.
    // One four-slot (&str, &str) match vector: 128 bytes.
    policy.limits.max_materialized_bytes = 32 + 32 + 128;
    policy.limits.max_retained_bytes = 2 * 64;
    policy.limits.max_collection_items = 3 * 64;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let mut relation = expected.clone();
        super::super::bind_dynamic_point_relation(&ctx, &mut relation, &roster, 1.0, None).unwrap();
        assert_eq!(relation, expected);
        let released = ctx.reserve_scoped(192, "released strict point workspaces").unwrap();
        drop(released);
    }
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn strict_point_match_leaves_coordinate_probe_armed() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (points, expected) = explicit_point_relation();
    let roster = [&points[0], &points[1]];
    let mut relation = expected.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 192;
    policy.limits.max_retained_bytes = 2;
    policy.limits.max_collection_items = 3;
    policy.limits.max_work_units = u64::MAX;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits, "collect SLDPRT relation coordinate points", None,
    );
    super::super::bind_dynamic_point_relation(&ctx, &mut relation, &roster, 1.0, None).unwrap();
    assert_eq!(relation, expected);
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(limit)) =
        ctx.charge_work(1, "collect SLDPRT relation coordinate points")
    else { panic!("strict match must leave coordinate probe armed"); };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "collect SLDPRT relation coordinate points");
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.charge_work(0, "after unused coordinate probe"),
        Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    drop(probe);
}

#[test]
fn strict_point_match_refuses_exact_storage_boundaries() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (points, expected) = explicit_point_relation();
    let roster = [&points[0], &points[1]];
    for (dimension, cap, used, additional, operation) in [
        (ResourceDimension::MaterializedBytes, 191, 64, 128, "collect SLDPRT dynamic point matches"),
        (ResourceDimension::RetainedBytes, 1, 1, 1, "retain SLDPRT relation record identity"),
        (ResourceDimension::CollectionItems, 2, 2, 1, "collect SLDPRT dynamic point matches"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 192;
        policy.limits.max_retained_bytes = 2;
        policy.limits.max_collection_items = 3;
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            _ => panic!("strict point storage dimension"),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut relation = expected.clone();
        let Err(CodecError::ResourceLimit(limit)) =
            super::super::bind_dynamic_point_relation(&ctx, &mut relation, &roster, 1.0, None)
        else { panic!("strict point boundary must refuse"); };
        assert_eq!(relation, expected);
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.limit, cap);
        assert_eq!(limit.used, used);
        assert_eq!(limit.additional, additional);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.charge_work(0, "after strict point storage refusal"),
            Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
}

#[test]
fn equal_line_indices_skip_solver_scans_and_keep_native_references() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (points, mut expected) = explicit_point_relation();
    expected.operands[1].entity_index = expected.operands[0].entity_index;
    for operand in &mut expected.operands {
        operand.kind = FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap());
        operand.entity_ref = None;
    }
    let roster = [&points[0], &points[1]];
    for angle in [false, true] {
        expected.family = if angle { FeatureInputRelationFamily::Angle } else { FeatureInputRelationFamily::LineLineDistance };
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 2 * 64;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for _ in 0..64 {
            let mut relation = expected.clone();
            super::super::bind_dynamic_line_relation(&ctx, &mut relation, &roster, 1.0, angle).unwrap();
            assert_eq!(relation, expected);
        }
        assert_eq!(ctx.resource_refusal(), None);
    }
    let native = sketch_marker("a", 0, 0, crate::records::SketchInputKind::LineOrCircle, None);
    expected.operands[0].entity_ref = Some("a".into());
    for angle in [false, true] {
        expected.family = if angle { FeatureInputRelationFamily::Angle } else { FeatureInputRelationFamily::LineLineDistance };
        let mut policy = DecodePolicy::service();
        // One visited native marker and both one-byte reference operands.
        policy.limits.max_work_units = 3;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut relation = expected.clone();
        super::super::bind_dynamic_line_relation(&ctx, &mut relation, &[&native], 1.0, angle).unwrap();
        assert_eq!(relation, expected);
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn equal_line_indices_leave_solver_probe_armed() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (points, mut expected) = explicit_point_relation();
    expected.family = FeatureInputRelationFamily::LineLineDistance;
    expected.operands[1].entity_index = expected.operands[0].entity_index;
    for operand in &mut expected.operands {
        operand.kind = FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap());
        operand.entity_ref = None;
    }
    let roster = [&points[0], &points[1]];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits, "select SLDPRT relation line points", None,
    );
    let mut relation = expected.clone();
    super::super::bind_dynamic_line_relation(&ctx, &mut relation, &roster, 1.0, false).unwrap();
    assert_eq!(relation, expected);
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "select SLDPRT relation line points")
    else { panic!("equal line indices must leave solver probe armed"); };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "select SLDPRT relation line points");
    assert_eq!(limit.used, 2);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.charge_work(0, "after unused line solver probe"),
        Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    drop(probe);
}

#[test]
fn equal_line_index_clear_refusal_stays_fused() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (points, mut expected) = explicit_point_relation();
    expected.operands[1].entity_index = expected.operands[0].entity_index;
    for operand in &mut expected.operands {
        operand.kind = FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap());
        operand.entity_ref = None;
    }
    for angle in [false, true] {
        expected.family = if angle { FeatureInputRelationFamily::Angle } else { FeatureInputRelationFamily::LineLineDistance };
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut relation = expected.clone();
        let Err(CodecError::ResourceLimit(limit)) = super::super::bind_dynamic_line_relation(
            &ctx, &mut relation, &[&points[0], &points[1]], 1.0, angle,
        ) else { panic!("equal line index clearing must refuse at one unit"); };
        assert_eq!(relation, expected);
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "clear SLDPRT relation operands");
        assert_eq!(limit.limit, 1);
        assert_eq!(limit.used, 0);
        assert_eq!(limit.additional, 2);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.charge_work(0, "after equal line index refusal"),
            Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
}
