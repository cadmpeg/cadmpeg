// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use super::{ONE_COMMENT, WITH_LOCAL_SYSTEM, parse, with_expression_policy};

#[test]
fn curve_expression_labels_refuse_before_vector_growth() {
    assert_eq!(
        parse(ONE_COMMENT, DecodePolicy::service())
            .expect("service profile admits expression")
            .len(),
        1
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo expression record labels"), |cap| {
        let mut trial = policy;
        trial.limits.max_collection_items = cap;
        parse(ONE_COMMENT, trial)
    });
    let error = parse(ONE_COMMENT, policy).expect_err("one label needs one collection item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo expression record labels"
    ));
}

#[test]
fn curve_expression_local_system_body_refuses_before_copy() {
    assert_eq!(
        parse(WITH_LOCAL_SYSTEM, DecodePolicy::service())
            .expect("service profile admits local system")
            .len(),
        1
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo expression local-system body"),
        |cap| {
            let mut trial_policy = policy;
            trial_policy.limits.max_retained_bytes = cap;
            parse(WITH_LOCAL_SYSTEM, trial_policy)
        },
    );
    let error =
        parse(WITH_LOCAL_SYSTEM, policy).expect_err("the local-system body needs retained bytes");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo expression local-system body"
    ));
}

#[test]
fn curve_expression_lines_refuse_before_vector_growth() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo expression record lines"), |cap| {
        let mut trial = policy;
        trial.limits.max_collection_items = cap;
        parse(ONE_COMMENT, trial)
    });
    let error = parse(ONE_COMMENT, policy).expect_err("the line follows one label");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo expression record lines"
    ));
}

#[test]
fn curve_expression_line_text_refuses_before_copy() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo expression record line text"),
        |cap| {
            let mut trial_policy = policy;
            trial_policy.limits.max_retained_bytes = cap;
            parse(ONE_COMMENT, trial_policy)
        },
    );
    let error = parse(ONE_COMMENT, policy).expect_err("the source line needs retained bytes");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo expression record line text"
    ));
}

#[test]
fn curve_expression_records_refuse_before_vector_growth() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo expression records"), |cap| {
        let mut trial = policy;
        trial.limits.max_collection_items = cap;
        parse(ONE_COMMENT, trial)
    });
    let error = parse(ONE_COMMENT, policy).expect_err("the record follows its label and line");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo expression records"
    ));
}

#[test]
fn curve_parameter_scalar_cache_refuses_before_unique_image_growth() {
    let payload = [0x46, 0x08, 0, 0, 0, 0, 0, 0];
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
            .expect("root input is admitted");
        crate::curve::parameter_records_with_face_ids(&ctx, &payload, None)
    };
    assert!(run(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, None, run)).expect("service admits scalar image").is_empty());
    let error = run(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo scalar cache unique images"), run)).expect_err("scalar image requires a set node");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo scalar cache unique images"
    ));
}

#[test]
fn depdb_curve_scalar_cache_refuses_before_unique_image_growth() {
    let payload = b"crv_array\0\xf2\xf8\x02crv_id\0\x06type\0\x08feat_id\0\x04topol_ref_data\0\x46\x08\0\0\0\0\0\0";
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
            .expect("root input is admitted");
        crate::curve::depdb_cross_section_rows(&ctx, payload)
    };
    assert!(run(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, None, run)).expect("service admits scalar image").is_empty());
    let error = run(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo scalar cache unique images"), run)).expect_err("scalar image requires a set node");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo scalar cache unique images"
    ));
}

fn scalar_lane_with_limits(
    body: &[u8],
    type_byte: u8,
    max_collection_items: u64,
    max_retained_bytes: u64,
) -> Result<crate::curve::CurveScalarLane, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    policy.limits.max_retained_bytes = max_retained_bytes;
    let (ctx, _) =
        DecodeContext::from_root_bytes(body, &arena, &policy).expect("root input is admitted");
    crate::curve::curve_scalar_lane(
        &ctx,
        body,
        type_byte,
        &crate::scalar::ScalarCache::default(),
    )
}

#[test]
fn curve_parameter_references_refuse_before_vector_growth() {
    let body = [0xf7, 1];
    let admitted = crate::decode::with_test_decode_ctx(|ctx| crate::curve::curve_scalar_lane(ctx, &body, 0, &crate::scalar::ScalarCache::default())).expect("service admits lane");
    assert_eq!(admitted.references.len(), 1);
    let error = crate::test_support::last_refusal_at(&[], ResourceDimension::CollectionItems, "creo curve parameter references", |ctx| crate::curve::curve_scalar_lane(ctx, &body, 0, &crate::scalar::ScalarCache::default()));
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "creo curve parameter references"));
}

#[test]
fn curve_parameter_scalars_refuse_before_vector_growth() {
    let body = [0x0e];
    let admitted = crate::decode::with_test_decode_ctx(|ctx| crate::curve::curve_scalar_lane(ctx, &body, 8, &crate::scalar::ScalarCache::default())).expect("service admits lane");
    assert_eq!(admitted.scalar_tokens.len(), 1);
    let error = crate::test_support::last_refusal_at(&[], ResourceDimension::CollectionItems, "creo curve parameter scalars", |ctx| crate::curve::curve_scalar_lane(ctx, &body, 8, &crate::scalar::ScalarCache::default()));
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "creo curve parameter scalars"));
}

#[test]
fn curve_scalar_raw_token_refuses_before_copy() {
    assert_eq!(
        scalar_lane_with_limits(
            &[0x0e],
            8,
            u64::MAX,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| scalar_lane_with_limits(&[0x0e], 8, u64::MAX, cap)
            )
        )
        .expect("one raw byte is admitted")
        .scalar_tokens[0]
            .raw,
        [0x0e]
    );
    let error = scalar_lane_with_limits(
        &[0x0e],
        8,
        2,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo curve scalar raw token"),
            |cap| scalar_lane_with_limits(&[0x0e], 8, u64::MAX, cap),
        ),
    )
    .expect_err("one raw byte needs retained admission");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo curve scalar raw token"));
}

#[test]
fn curve_zero_raw_token_refuses_before_copy() {
    assert_eq!(
        scalar_lane_with_limits(
            &[0x18],
            0,
            u64::MAX,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| scalar_lane_with_limits(&[0x18], 0, u64::MAX, cap)
            )
        )
        .expect("one zero byte is admitted")
        .scalar_tokens[0]
            .raw,
        [0x18]
    );
    let error = scalar_lane_with_limits(
        &[0x18],
        0,
        2,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo curve zero raw token"),
            |cap| scalar_lane_with_limits(&[0x18], 0, u64::MAX, cap),
        ),
    )
    .expect_err("one zero byte needs retained admission");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo curve zero raw token"));
}

#[test]
fn curve_opaque_spans_refuse_before_vector_growth() {
    let body = [0xff];
    let admitted = crate::decode::with_test_decode_ctx(|ctx| crate::curve::curve_scalar_lane(ctx, &body, 0, &crate::scalar::ScalarCache::default())).expect("service admits lane");
    assert_eq!(admitted.opaque_spans.len(), 1);
    let error = crate::test_support::last_refusal_at(&[], ResourceDimension::CollectionItems, "creo curve opaque spans", |ctx| crate::curve::curve_scalar_lane(ctx, &body, 0, &crate::scalar::ScalarCache::default()));
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "creo curve opaque spans"));
}

#[test]
fn curve_opaque_raw_span_refuses_before_copy() {
    assert_eq!(
        scalar_lane_with_limits(
            &[0xff],
            0,
            u64::MAX,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| scalar_lane_with_limits(&[0xff], 0, u64::MAX, cap)
            )
        )
        .expect("one opaque byte is admitted")
        .opaque_spans[0]
            .raw,
        [0xff]
    );
    let error = scalar_lane_with_limits(
        &[0xff],
        0,
        2,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo curve opaque raw span"),
            |cap| scalar_lane_with_limits(&[0xff], 0, u64::MAX, cap),
        ),
    )
    .expect_err("one opaque byte needs retained admission");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo curve opaque raw span"));
}

#[test]
fn curve_parameter_records_refuse_before_vector_growth() {
    let payload = b"topol_ref_data\0\x07\x08\x04\x01\xf6\xff\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3";
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
            .expect("root input is admitted");
        crate::curve::parameter_records_with_face_ids(&ctx, payload, None)
    };
    let admitted = crate::test_support::assert_refusal_order(
        ResourceDimension::CollectionItems,
        &["creo curve parameter records"],
        run,
    );
    assert_eq!(admitted.len(), 1);
    let error = run(crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo curve parameter records"),
        run,
    ))
    .expect_err("last collection item is the record");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo curve parameter records"));
}

#[test]
fn expression_conditional_validation_refuses_before_stack_growth() {
    let lines = [
        crate::curve::CurveExpressionLine {
            text: "if 1".to_string(),
            offset: 0,
        },
        crate::curve::CurveExpressionLine {
            text: "endif".to_string(),
            offset: 5,
        },
    ];
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        crate::curve::expression_program_control_is_valid(&ctx, &lines)
    };
    assert!(run(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, None, run)).expect("one conditional is admitted"));
    let error = run(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo expression conditional validation"), run)).expect_err("one conditional requires a validation slot");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo expression conditional validation"));
}

#[test]
fn expression_conditional_parent_refuses_before_stack_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo expression conditional parents"), |cap| {
        let mut trial = policy;
        trial.limits.max_collection_items = cap;
        with_expression_policy(trial, |ctx| {
            let mut stack = crate::curve::ConditionalStack::default();
            stack.push(ctx, crate::curve::ConditionalFrame { parent: crate::curve::CurveExpressionActivation::Active, condition: Some(true) })?;
            stack.push(ctx, crate::curve::ConditionalFrame { parent: crate::curve::CurveExpressionActivation::Active, condition: Some(true) })
        })
    });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let frame = || crate::curve::ConditionalFrame {
        parent: crate::curve::CurveExpressionActivation::Active,
        condition: Some(true),
    };
    let mut stack = crate::curve::ConditionalStack::default();
    stack
        .push(&ctx, frame())
        .expect("first frame is held inline");
    let error = stack
        .push(&ctx, frame())
        .expect_err("a nested frame needs a parent slot");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo expression conditional parents"));
    assert_eq!(stack.end(), crate::curve::CurveExpressionActivation::Active);
}

const ONE_DEPDB_CURVE_ROW: &[u8] = b"crv_array\0\xf2\xf8\x02crv_id\0\x06type\0\x08feat_id\0\x04topol_ref_data\0\x07\x08\x04\x01\xf6\xe4\xff\0\x09\x0a\0\xe1\xe0next_record\0";

fn depdb_rows_with_limit(limit: u64) -> Result<Vec<crate::curve::DepdbCurveRow>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(ONE_DEPDB_CURVE_ROW, &arena, &policy)
        .expect("root input is admitted");
    crate::curve::depdb_cross_section_rows(&ctx, ONE_DEPDB_CURVE_ROW)
}

#[test]
fn depdb_curve_rows_refuse_before_fallible_reservation() {
    assert_eq!(
        depdb_rows_with_limit(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, None, depdb_rows_with_limit))
            .expect("service admits one complete curve row")
            .len(),
        1
    );
    let error = depdb_rows_with_limit(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo cross-section curve rows"), depdb_rows_with_limit)).expect_err("one row requires one collection item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo cross-section curve rows"
    ));
}

#[test]
fn depdb_curve_boundaries_refuse_before_vec_growth() {
    let error = depdb_rows_with_limit(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo cross-section row boundaries"), depdb_rows_with_limit)).expect_err("boundary follows row reservation");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo cross-section row boundaries"
    ));
}

#[test]
fn depdb_segment_unique_candidates_need_no_collection_items() {
    let segment = [7, 8, 4, 1, 0xf6, 0, 9, 10, 0];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&segment, &arena, &policy).expect("root input is admitted");
    let row = crate::curve::parse_depdb_curve_segment(
        &ctx,
        &segment,
        0,
        &crate::scalar::ScalarCache::default(),
    )
    .expect("candidate scan has no collection")
    .expect("one prefix and one suffix");
    assert_eq!(row.id, 7);
    assert_eq!(row.suffix.x1, 9);
}

#[test]
fn depdb_segment_distinct_prefixes_remain_ambiguous() {
    let segment = [7, 8, 4, 1, 0xf6, 8, 8, 4, 1, 0xf6, 0, 9, 10, 0];
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        crate::curve::parse_depdb_curve_segment(
            ctx,
            &segment,
            0,
            &crate::scalar::ScalarCache::default(),
        )
    })
    .expect("ambiguous segment is parsed")
    .is_none());
}

#[test]
fn curve_prototype_vec_refuses_before_growth() {
    let payload = b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04";
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
            .expect("root input is admitted");
        crate::curve::prototypes(&ctx, payload)
    };
    assert_eq!(run(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, None, run)).expect("service admits one prototype").len(), 1);
    let error = run(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo curve prototypes"), run)).expect_err("one prototype requires a vector item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo curve prototypes"
    ));
}

