// SPDX-License-Identifier: Apache-2.0

#[test]
fn curve_expression_source_section_refuses_before_retained_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01value=5\0"
        .to_vec();
    let data = crate::test_support::build_prt("c", &[("FeatDefs", payload)]);
    let scan = crate::container::scan_bytes_ok(data);
    let offset = scan.curves.expressions[0].offset;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        ResourceDimension::RetainedBytes,
        Some("creo expression source section"),
        |cap| {
            let mut trial = policy;
            trial.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &trial).expect("root");
            crate::decode::coverage::source_section(&ctx, &scan, offset)
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = crate::decode::coverage::source_section(&ctx, &scan, offset)
        .expect_err("eight source-section bytes exceed retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RetainedBytes
            && refusal.operation == "creo expression source section")
    );
}

#[test]
fn curve_expression_frame_admits_only_finite_local_origin() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x02local_sys\0\xf9\x04\x03\xe4\x0f\x0f\x0f\x0f\x0f\x18\xe5\x0f\x0f\x0f\
        \xe0\x0aexpression\0\xf8\x03r=5\0theta=0-t*360\0z=-2+10*t\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let slots = record
        .local_system
        .as_ref()
        .expect("local system")
        .explicit_slots
        .expect("explicit slots");
    let mut nonfinite = slots.get();
    nonfinite[9] = f64::NAN;
    assert!(cadmpeg_ir::units::FiniteVector::new(nonfinite).is_none());

    let helix =
        crate::decode::with_test_decode_ctx(|ctx| crate::curve::expression_helix(ctx, &record))
            .expect("service helix admission")
            .expect("affine helix");
    assert!(super::curve_expression_helix_definition(&record, &helix).is_some());
}

#[test]
fn curve_expression_dependency_rows_refuse_before_allocation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0b=a+1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    assert_eq!(record.assignments.len(), 2);
    let unique_assignment_indices = std::collections::BTreeMap::new();
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &service)
        .expect("service profile admits input");
    assert!(
        super::curve_expression_parameter_order(&ctx, &record, &unique_assignment_indices)
            .expect("service profile admits dependency rows")
            .is_some()
    );

    let err = crate::test_support::last_refusal_at(
        payload,
        ResourceDimension::CollectionItems,
        "creo curve-expression dependency rows",
        |ctx| {
            super::curve_expression_parameter_order(ctx, &record, &unique_assignment_indices)
                .map(|result| result.map(|order| (order.ordinals, order.cyclic_edges)))
        },
    );
    assert!(matches!(
        err,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo curve-expression dependency rows"
    ));
}

#[test]
fn curve_expression_dependency_indices_refuse_before_inner_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0b=a+1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let unique_assignment_indices = std::collections::BTreeMap::from([("a".to_owned(), 0)]);
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &service)
        .expect("service profile admits input");
    assert!(
        super::curve_expression_parameter_order(&ctx, &record, &unique_assignment_indices)
            .expect("service profile admits dependency indices")
            .is_some()
    );

    let err = crate::test_support::last_refusal_at(
        payload,
        ResourceDimension::CollectionItems,
        "creo curve-expression dependency indices",
        |ctx| {
            super::curve_expression_parameter_order(ctx, &record, &unique_assignment_indices)
                .map(|result| result.map(|order| (order.ordinals, order.cyclic_edges)))
        },
    );
    assert!(matches!(
        err,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo curve-expression dependency indices"
    ));
}

#[test]
fn curve_expression_ordering_lookup_refuses_before_temporary_text() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0b=a+1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let indices = std::collections::BTreeMap::from([("a".to_string(), 0)]);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &DecodePolicy::service())
        .expect("service input fits");
    assert!(
        super::curve_expression_parameter_order(&ctx, &record, &indices)
            .expect("service profile admits lookup")
            .is_some()
    );

    let error = crate::test_support::last_refusal_at(
        payload,
        ResourceDimension::MaterializedBytes,
        "creo curve-expression ordering lookup",
        |ctx| {
            super::curve_expression_parameter_order(ctx, &record, &indices)
                .map(|result| result.map(|order| (order.ordinals, order.cyclic_edges)))
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "creo curve-expression ordering lookup"
    ));
}

#[test]
fn curve_expression_cyclic_edges_refuse_before_insert() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=b\0b=a\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let indices = std::collections::BTreeMap::from([("a".to_owned(), 0), ("b".to_owned(), 1)]);
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        super::curve_expression_parameter_order(ctx, &record, &indices)
    })
    .expect("service profile admits cyclic dependencies")
    .is_some());
    let error = crate::test_support::last_refusal_at(
        payload,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo curve-expression cyclic edges",
        |ctx| {
            super::curve_expression_parameter_order(ctx, &record, &indices)
                .map(|result| result.map(|order| (order.ordinals, order.cyclic_edges)))
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression cyclic edges"
    ));
}

#[test]
fn curve_expression_unique_name_refuses_before_tree_insert() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let names = crate::decode::with_test_decode_ctx(|ctx| {
        super::curve_expression_parameter_names(ctx, &record.assignments)
    })
    .expect("service profile admits names");
    assert_eq!(names, vec![Some("a".to_owned())]);
    let error = crate::test_support::last_refusal_at(
        payload,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo curve-expression unique names",
        |ctx| super::curve_expression_parameter_names(ctx, &record.assignments),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression unique names"
    ));
}

#[test]
fn curve_expression_occurrence_refuses_before_tree_insert() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0a=2\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let names = crate::decode::with_test_decode_ctx(|ctx| {
        super::curve_expression_parameter_names(ctx, &record.assignments)
    })
    .expect("service profile admits duplicate names");
    assert_eq!(names, vec![Some("a#1".to_owned()), Some("a#2".to_owned())]);
    let error = crate::test_support::last_refusal_at(
        payload,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo curve-expression occurrences",
        |ctx| super::curve_expression_parameter_names(ctx, &record.assignments),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression occurrences"
    ));
}

#[test]
fn curve_expression_name_slots_refuse_before_vector_growth() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let error = crate::test_support::last_refusal_at(
        payload,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo curve-expression parameter name slots",
        |ctx| super::curve_expression_parameter_names(ctx, &record.assignments),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression parameter name slots"
    ));
}

#[test]
fn curve_expression_name_key_refuses_before_text_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        ResourceDimension::RetainedBytes,
        Some("creo curve-expression name key"),
        |cap| {
            let mut trial = policy;
            trial.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &trial).expect("root");
            super::curve_expression_parameter_names(&ctx, &record.assignments)
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("input fits the root limit");
    let error = super::curve_expression_parameter_names(&ctx, &record.assignments)
        .expect_err("the first name needs one retained byte");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo curve-expression name key"
    ));
}

#[test]
fn curve_expression_name_suffix_refuses_before_text_growth() {
    use cadmpeg_core::decode::ResourceDimension;

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0a=2\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let error = crate::test_support::last_refusal_at(
        payload,
        ResourceDimension::RetainedBytes,
        "creo curve-expression parameter suffix",
        |ctx| super::curve_expression_parameter_names(ctx, &record.assignments),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo curve-expression parameter suffix"
    ));
}

#[test]
fn curve_expression_parameter_name_refuses_before_text_copy() {
    use cadmpeg_core::decode::ResourceDimension;

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let error = crate::test_support::last_refusal_at(
        payload,
        ResourceDimension::RetainedBytes,
        "creo curve-expression parameter name",
        |ctx| super::curve_expression_parameter_names(ctx, &record.assignments),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo curve-expression parameter name"
    ));
}

#[test]
fn curve_expression_assignment_index_refuses_before_tree_insert() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0b=2\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let (by_name, unique) = crate::decode::with_test_decode_ctx(|ctx| {
        super::curve_expression_assignment_indices(ctx, &record)
            .map(|indices| (indices.by_name, indices.unique))
    })
    .expect("service profile admits assignment maps");
    assert_eq!(by_name.len(), 2);
    assert_eq!(unique.len(), 2);

    let error = crate::test_support::last_refusal_at(
        payload,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo curve-expression assignment indices",
        |ctx| {
            super::curve_expression_assignment_indices(ctx, &record)
                .map(|indices| (indices.by_name, indices.unique))
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression assignment indices"
    ));
}

#[test]
fn curve_expression_unique_index_refuses_before_tree_insert() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0b=2\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let error = crate::test_support::last_refusal_at(
        payload,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo curve-expression unique indices",
        |ctx| {
            super::curve_expression_assignment_indices(ctx, &record)
                .map(|indices| (indices.by_name, indices.unique))
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression unique indices"
    ));
}

fn assignment_indices_retained_refusal(
    record: &crate::curve::CurveExpressionRecord,
    operation: &'static str,
) -> cadmpeg_core::CodecError {
    crate::test_support::last_refusal_at(
        b"a",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        operation,
        |ctx| {
            super::curve_expression_assignment_indices(ctx, record)
                .map(|indices| (indices.by_name, indices.unique))
        },
    )
}

#[test]
fn curve_expression_assignment_key_refuses_before_text_copy() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let error =
        assignment_indices_retained_refusal(&record, "creo curve-expression assignment key");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression assignment key"
    ));
}

#[test]
fn curve_expression_unique_key_refuses_before_text_copy() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let error = assignment_indices_retained_refusal(&record, "creo curve-expression unique key");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression unique key"
    ));
}

#[test]
fn curve_expression_emitted_indices_refuse_before_vector_reserve() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0b=2\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let emitted = crate::decode::with_test_decode_ctx(|ctx| {
        super::curve_expression_emitted_ordinals(ctx, &record, &[0, 1])
    })
    .expect("service profile admits emitted ordinals");
    assert_eq!(emitted.get(&0), Some(&0));
    assert_eq!(emitted.get(&1), Some(&1));

    let error = crate::test_support::last_refusal_at(
        payload,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo curve-expression emitted indices",
        |ctx| super::curve_expression_emitted_ordinals(ctx, &record, &[0, 1]),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression emitted indices"
    ));
}

#[test]
fn curve_expression_emitted_ordinals_refuse_before_tree_insert() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0b=2\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let error = crate::test_support::last_refusal_at(
        payload,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo curve-expression emitted ordinals",
        |ctx| super::curve_expression_emitted_ordinals(ctx, &record, &[0, 1]),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression emitted ordinals"
    ));
}

#[test]
fn curve_expression_source_content_refuses_before_vector_reserve() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let run = |policy: DecodePolicy| {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
            .expect("root input fits the limit");
        let mut scan = crate::test_support::empty_container_scan();
        scan.curves.expressions.push(record.clone());
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let mut carriers = crate::decode::source_carriers::SourceUnitCarriers::new(None);
        super::transfer_curve_expression_features(
            &ctx,
            &scan,
            &mut ir,
            &mut annotations,
            &std::collections::BTreeMap::new(),
            &mut carriers,
        )
    };
    assert_eq!(
        run(DecodePolicy::service()).expect("service profile admits the feature"),
        1
    );

    let mut limited = DecodePolicy::service();
    limited.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo curve-expression source content"),
        |cap| {
            let mut trial = limited;
            trial.limits.max_collection_items = cap;
            run(trial)
        },
    );
    let error = run(limited).expect_err("source content follows nine admitted items");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression source content"
    ));
}

fn transfer_with_limits(
    expression_lines: &[&str],
    dimension_parameters: &std::collections::BTreeMap<String, cadmpeg_ir::features::ParameterId>,
    policy: cadmpeg_core::decode::DecodePolicy,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8"
        .to_vec();
    payload.push(u8::try_from(expression_lines.len()).expect("test line count fits byte"));
    for line in expression_lines {
        payload.extend_from_slice(line.as_bytes());
        payload.push(0);
    }
    let record = crate::curve::expression_records(&payload)
        .pop()
        .expect("complete curve expression");
    transfer_record_with_limits(&payload, record, dimension_parameters, policy)
}

fn transfer_record_with_limits(
    payload: &[u8],
    record: crate::curve::CurveExpressionRecord,
    dimension_parameters: &std::collections::BTreeMap<String, cadmpeg_ir::features::ParameterId>,
    policy: cadmpeg_core::decode::DecodePolicy,
) -> Result<usize, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("root bytes fit the configured limit");
    transfer_record_in_context(&ctx, record, dimension_parameters)
}

fn transfer_record_in_context(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: crate::curve::CurveExpressionRecord,
    dimension_parameters: &std::collections::BTreeMap<String, cadmpeg_ir::features::ParameterId>,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.curves.expressions.push(record);
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let mut carriers = crate::decode::source_carriers::SourceUnitCarriers::new(None);
    super::transfer_curve_expression_features(
        ctx,
        &scan,
        &mut ir,
        &mut annotations,
        dimension_parameters,
        &mut carriers,
    )
}

fn assert_retained_transfer_boundary(
    mut run: impl FnMut(cadmpeg_core::decode::DecodePolicy) -> Result<usize, cadmpeg_core::CodecError>,
    operation: &'static str,
) {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    assert!(
        run(DecodePolicy::service()).is_ok(),
        "service transfer must succeed"
    );
    let mut limit = 0_u64;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = limit;
        match run(policy) {
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.operation == operation =>
            {
                assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
                assert!(refusal.limit < refusal.used + refusal.additional);
                return;
            }
            Err(cadmpeg_core::CodecError::ResourceLimit(resource)) => {
                assert_eq!(resource.dimension, ResourceDimension::RetainedBytes);
                let need = resource
                    .used
                    .checked_add(resource.additional)
                    .expect("retained need");
                assert!(need > limit);
                limit = need;
            }
            other => panic!("{operation} was not reached before {other:?}"),
        }
    }
    panic!("{operation} was not reached within the retained-byte test range");
}

#[test]
fn curve_expression_identities_refuse_retained_limit_at_each_copy() {
    let dimensions = std::collections::BTreeMap::new();
    for operation in [
        "creo curve-expression feature identity",
        "creo curve-expression parameter identity",
        "creo curve-expression IR parameter ID copy",
        "creo curve-expression owner ID copy",
        "creo curve-expression source parameter ID copy",
    ] {
        assert_retained_transfer_boundary(
            |policy| transfer_with_limits(&["a=1"], &dimensions, policy),
            operation,
        );
    }
    assert_retained_transfer_boundary(
        |policy| transfer_with_limits(&["a=1", "b=a+1"], &dimensions, policy),
        "creo curve-expression dependency identity",
    );
}

#[test]
fn curve_expression_helix_identities_refuse_retained_limit_at_each_copy() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x02local_sys\0\xf9\x04\x03\xe4\x0f\x0f\x0f\x0f\x0f\x18\xe5\x0f\x0f\x0f\
        \xe0\x0aexpression\0\xf8\x03r=5\0theta=0-t*360\0z=-2+10*t\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let dimensions = std::collections::BTreeMap::new();
    for operation in [
        "creo curve-expression curve identity",
        "creo curve-expression procedural identity",
        "creo curve-expression IR curve ID copy",
    ] {
        assert_retained_transfer_boundary(
            |policy| transfer_record_with_limits(payload, record.clone(), &dimensions, policy),
            operation,
        );
    }
}

fn dependency_keys_with_limits(
    expression_lines: &[&str],
    dimension_parameters: &std::collections::BTreeMap<String, cadmpeg_ir::features::ParameterId>,
    policy: cadmpeg_core::decode::DecodePolicy,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};

    let mut payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8"
        .to_vec();
    payload.push(u8::try_from(expression_lines.len()).expect("test line count fits byte"));
    for line in expression_lines {
        payload.extend_from_slice(line.as_bytes());
        payload.push(0);
    }
    let record = crate::curve::expression_records(&payload)
        .pop()
        .expect("complete curve expression");
    let (by_name, unique) = crate::decode::with_test_decode_ctx(|ctx| {
        super::curve_expression_assignment_indices(ctx, &record)
            .map(|indices| (indices.by_name, indices.unique))
    })
    .expect("service setup admits assignment maps");
    let cyclic_edges = crate::decode::with_test_decode_ctx(|ctx| {
        super::curve_expression_parameter_order(ctx, &record, &unique)
            .map(|result| result.map(|order| (order.ordinals, order.cyclic_edges)))
    })
    .expect("service setup admits ordering")
    .expect("executable assignments")
    .1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
        .expect("root bytes fit the configured limit");
    for ordinal in 0..record.assignments.len() {
        drop(super::curve_expression_parameter_dependencies(
            &ctx,
            &record,
            ordinal,
            &by_name,
            &unique,
            &cyclic_edges,
            dimension_parameters,
        )?);
    }
    Ok(())
}

#[test]
fn curve_expression_dependency_keys_refuse_before_text_copy() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let dimensions = std::collections::BTreeMap::new();
    assert_eq!(
        transfer_with_limits(&["a=1", "b=a+1"], &dimensions, DecodePolicy::service())
            .expect("service profile admits dependencies"),
        2
    );
    let mut limited = DecodePolicy::service();
    limited.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo curve-expression dependency key"),
        |cap| {
            let mut trial = limited;
            trial.limits.max_materialized_bytes = cap;
            dependency_keys_with_limits(&["a=1", "b=a+1"], &dimensions, trial)
        },
    );
    let error = dependency_keys_with_limits(&["a=1", "b=a+1"], &dimensions, limited)
        .expect_err("the dependency lookup key needs another byte");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "creo curve-expression dependency key")
    );
}

#[test]
fn curve_expression_seen_dependencies_refuse_before_tree_insert() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let dimensions = std::collections::BTreeMap::new();
    let mut limited = DecodePolicy::service();
    limited.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo curve-expression seen dependencies"),
        |cap| {
            let mut trial = limited;
            trial.limits.max_collection_items = cap;
            transfer_with_limits(&["a=1", "b=a+1"], &dimensions, trial)
        },
    );
    let error = transfer_with_limits(&["a=1", "b=a+1"], &dimensions, limited)
        .expect_err("the dependency index needs one tree item");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo curve-expression seen dependencies")
    );
}

#[test]
fn curve_expression_parameter_dependencies_refuse_before_vector_growth() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let dimensions = std::collections::BTreeMap::new();
    let mut limited = DecodePolicy::service();
    limited.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo curve-expression parameter dependencies"),
        |cap| {
            let mut trial = limited;
            trial.limits.max_collection_items = cap;
            transfer_with_limits(&["a=1", "b=a+1"], &dimensions, trial)
        },
    );
    let error = transfer_with_limits(&["a=1", "b=a+1"], &dimensions, limited)
        .expect_err("the dependency parameter needs one vector item");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo curve-expression parameter dependencies")
    );
}

#[test]
fn curve_expression_dimension_dependencies_refuse_before_vector_growth() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let dimensions = std::collections::BTreeMap::from([(
        "x".to_owned(),
        cadmpeg_ir::features::ParameterId::mint("test:test:parameter#x")
            .expect("valid dimension ID"),
    )]);
    assert_eq!(
        transfer_with_limits(&["a=x+1"], &dimensions, DecodePolicy::service())
            .expect("service profile admits dimension dependency"),
        1
    );
    let mut limited = DecodePolicy::service();
    limited.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo curve-expression dimension dependencies"),
        |cap| {
            let mut trial = limited;
            trial.limits.max_collection_items = cap;
            transfer_with_limits(&["a=x+1"], &dimensions, trial)
        },
    );
    let error = transfer_with_limits(&["a=x+1"], &dimensions, limited)
        .expect_err("the dimension parameter needs one vector item");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo curve-expression dimension dependencies")
    );
}

#[test]
fn curve_expression_dimension_candidates_refuse_before_vector_growth() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let dimensions = std::collections::BTreeMap::from([(
        "x".to_owned(),
        cadmpeg_ir::features::ParameterId::mint("test:test:parameter#x")
            .expect("valid dimension ID"),
    )]);
    let mut limited = DecodePolicy::service();
    limited.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo curve-expression dimension candidates"),
        |cap| {
            let mut trial = limited;
            trial.limits.max_collection_items = cap;
            transfer_with_limits(&["a=x+1"], &dimensions, trial)
        },
    );
    let error = transfer_with_limits(&["a=x+1"], &dimensions, limited)
        .expect_err("the dimension candidate needs one vector item");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo curve-expression dimension candidates")
    );
}

#[test]
fn curve_expression_dimension_key_refuses_before_text_copy() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let dimensions = std::collections::BTreeMap::from([(
        "x".to_owned(),
        cadmpeg_ir::features::ParameterId::mint("test:test:parameter#x")
            .expect("valid dimension ID"),
    )]);
    let mut limited = DecodePolicy::service();
    limited.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo curve-expression dependency key"),
        |cap| {
            let mut trial = limited;
            trial.limits.max_materialized_bytes = cap;
            dependency_keys_with_limits(&["a=x+1"], &dimensions, trial)
        },
    );
    let error = dependency_keys_with_limits(&["a=x+1"], &dimensions, limited)
        .expect_err("the dimension dependency lookup needs one temporary byte");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "creo curve-expression dependency key")
    );
}

#[test]
fn curve_expression_dimension_parameter_id_refuses_before_copy() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let dimensions = std::collections::BTreeMap::from([(
        "x".to_owned(),
        cadmpeg_ir::features::ParameterId::mint("test:test:parameter#x")
            .expect("valid dimension ID"),
    )]);
    let mut limited = DecodePolicy::service();
    limited.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        ResourceDimension::RetainedBytes,
        Some("creo curve-expression dimension parameter id"),
        |cap| {
            let mut trial = limited;
            trial.limits.max_retained_bytes = cap;
            transfer_with_limits(&["a=x+1"], &dimensions, trial)
        },
    );
    let error = transfer_with_limits(&["a=x+1"], &dimensions, limited)
        .expect_err("the dimension ID needs retained bytes");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo curve-expression dimension parameter id")
    );
}

#[test]
fn curve_expression_dependency_validation_charges_index_work() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let dimensions = std::collections::BTreeMap::new();
    assert_eq!(
        transfer_with_limits(
            &["a=1", "b=2", "c=a+b"],
            &dimensions,
            DecodePolicy::service(),
        )
        .expect("service profile admits two dependencies"),
        3
    );
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\xe0\x0aexpression\0\xf8\x03a=1\0b=2\0c=a+b\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "validate distinct decoded members",
        |ctx| transfer_record_in_context(ctx, record.clone(), &dimensions),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "validate distinct decoded members")
    );
}

fn retained_refusal<T>(
    operation: &'static str,
    run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> Result<T, cadmpeg_core::CodecError> {
    Err(crate::test_support::last_refusal_at(
        b"x",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        operation,
        run,
    ))
}

#[test]
fn curve_expression_external_dependency_text_refuses_before_copy() {
    let names = vec!["outside".to_string()];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::joined_dependency_names(
            ctx,
            &names,
            |_| Ok(true),
            "creo curve-expression external dependency text",
        ))
        .expect("service profile admits the property"),
        Some("outside".to_string())
    );
    let error = retained_refusal("creo curve-expression external dependency text", |ctx| {
        super::joined_dependency_names(
            ctx,
            &names,
            |_| Ok(true),
            "creo curve-expression external dependency text",
        )
    })
    .expect_err("seven property bytes exceed the limit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression external dependency text"
    ));
}

#[test]
fn curve_expression_ambiguous_dependency_text_refuses_before_copy() {
    let names = vec!["ambiguous".to_string()];
    let error = retained_refusal("creo curve-expression ambiguous dependency text", |ctx| {
        super::joined_dependency_names(
            ctx,
            &names,
            |_| Ok(true),
            "creo curve-expression ambiguous dependency text",
        )
    })
    .expect_err("nine property bytes exceed the limit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression ambiguous dependency text"
    ));
}

#[test]
fn curve_expression_intrinsic_dependency_text_refuses_before_copy() {
    let names = vec!["t".to_string(), "T".to_string()];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::joined_dependency_names(
            ctx,
            &names,
            |name| Ok(name.eq_ignore_ascii_case("t")),
            "creo curve-expression intrinsic dependency text",
        ))
        .expect("service profile admits the property"),
        Some("t,T".to_string())
    );
    let error = retained_refusal("creo curve-expression intrinsic dependency text", |ctx| {
        super::joined_dependency_names(
            ctx,
            &names,
            |_| Ok(true),
            "creo curve-expression intrinsic dependency text",
        )
    })
    .expect_err("the separator is a retained byte");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression intrinsic dependency text"
    ));
}

#[test]
fn curve_expression_cyclic_dependency_text_refuses_before_copy() {
    let names = ["alpha", "beta"];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::join_cyclic_dependency_names(ctx, &names))
            .expect("service profile admits the joined text"),
        "alpha,beta"
    );
    let error = retained_refusal("creo curve-expression cyclic dependency text", |ctx| {
        super::join_cyclic_dependency_names(ctx, &names)
    })
    .expect_err("the joined names need ten bytes");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression cyclic dependency text"
    ));
}

#[test]
fn curve_expression_property_node_refuses_before_tree_insert() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo curve-expression property nodes",
        |ctx| {
            let mut properties = std::collections::BTreeMap::new();
            super::insert_curve_expression_property(ctx, &mut properties, "source_name", "x".into())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression property nodes"
    ));
}

fn quantity_property_result(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<
    std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    cadmpeg_core::CodecError,
> {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let mut record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    record.assignments[0].value = Some(crate::curve::CurveExpressionValue::Quantity(
        crate::curve::CurveExpressionQuantity::new(3.5, [1, 2, 0, 0, 0])
            .expect("valid residual dimension fixture"),
    ));
    let parameter_id = cadmpeg_ir::features::ParameterId::mint("test:test:parameter#a")
        .expect("valid parameter ID");
    super::curve_expression_properties(
        ctx,
        &record.assignments[0],
        0,
        ("a", &parameter_id),
        (
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
        ),
        &std::collections::BTreeMap::new(),
        &std::collections::HashSet::new(),
    )
}

#[test]
fn curve_expression_source_ordinal_value_refuses_before_text() {
    let error = retained_refusal(
        "creo curve-expression source ordinal value",
        quantity_property_result,
    )
    .expect_err("ordinal value needs one retained byte");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "creo curve-expression source ordinal value")
    );
}

#[test]
fn curve_expression_activation_value_refuses_before_text() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo curve-expression activation value",
        quantity_property_result,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "creo curve-expression activation value")
    );
}

#[test]
fn curve_expression_canonical_value_refuses_before_text() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo curve-expression canonical value",
        quantity_property_result,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "creo curve-expression canonical value")
    );
}

#[test]
fn curve_expression_dimension_value_refuses_before_text() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo curve-expression dimension value",
        quantity_property_result,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "creo curve-expression dimension value")
    );
}

#[test]
fn curve_expression_quantity_properties_keep_value_and_dimension() {
    let properties = crate::decode::with_test_decode_ctx(quantity_property_result)
        .expect("service resources admit quantity properties");
    assert_eq!(properties.len(), 4);
    assert_eq!(
        properties[&cadmpeg_core::nonblank_literal!("evaluated_canonical_value")],
        "3.5"
    );
    assert_eq!(
        properties[&cadmpeg_core::nonblank_literal!("evaluated_dimension")],
        "length:1,mass:2,time:0,angle:0,temperature:0"
    );
}

#[test]
fn curve_expression_native_kind_refuses_before_text() {
    let error = retained_refusal("creo curve-expression native kind", |ctx| {
        super::native_curve_expression_definition(ctx, 7, 1)
    })
    .expect_err("native kind needs retained text");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "creo curve-expression native kind")
    );
}

#[test]
fn curve_expression_native_entity_value_refuses_before_text() {
    let error = retained_refusal("creo curve-expression native entity value", |ctx| {
        super::native_curve_expression_definition(ctx, 7, 1)
    })
    .expect_err("entity value needs one more retained byte");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "creo curve-expression native entity value")
    );
}

#[test]
fn curve_expression_native_assignment_count_refuses_before_text() {
    let error = retained_refusal("creo curve-expression native assignment count", |ctx| {
        super::native_curve_expression_definition(ctx, 7, 1)
    })
    .expect_err("assignment count needs one more retained byte");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "creo curve-expression native assignment count")
    );
}

#[test]
fn curve_expression_native_fallback_keeps_kind_and_values() {
    let definition = crate::decode::with_test_decode_ctx(|ctx| {
        super::native_curve_expression_definition(ctx, 7, 1)
    })
    .expect("service resources admit native definition");
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Native { kind, parameters },
    ) = definition
    else {
        panic!("native fallback definition");
    };
    assert_eq!(kind.as_str(), "CurveFromEquation");
    assert_eq!(
        parameters[&cadmpeg_core::nonblank_literal!("entity_id")],
        "7"
    );
    assert_eq!(
        parameters[&cadmpeg_core::nonblank_literal!("assignment_count")],
        "1"
    );
}

#[test]
fn curve_expression_feature_name_refuses_before_text() {
    let error = retained_refusal("creo curve-expression feature name", |ctx| {
        super::curve_expression_feature_labels(ctx, 7)
    })
    .expect_err("feature name needs retained text");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "creo curve-expression feature name")
    );
}

#[test]
fn curve_expression_feature_source_tag_refuses_before_text() {
    let error = retained_refusal("creo curve-expression feature source tag", |ctx| {
        super::curve_expression_feature_labels(ctx, 7)
    })
    .expect_err("source tag needs retained text");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "creo curve-expression feature source tag")
    );
}

#[test]
fn curve_expression_feature_labels_keep_source_spelling() {
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::curve_expression_feature_labels(ctx, 7))
            .expect("service resources admit labels"),
        ("Curve Equation 7".to_string(), "crv_fr_eqn".to_string())
    );
}

#[test]
fn curve_expression_source_text_refuses_before_join() {
    let lines = [
        crate::curve::CurveExpressionLine {
            text: "alpha".to_string(),
            offset: 0,
        },
        crate::curve::CurveExpressionLine {
            text: "beta".to_string(),
            offset: 6,
        },
    ];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::curve_expression_source_text(ctx, &lines))
            .expect("service profile admits source text"),
        "alpha\nbeta"
    );
    let error = retained_refusal("creo curve-expression feature source text", |ctx| {
        super::curve_expression_source_text(ctx, &lines)
    })
    .expect_err("the joined lines need ten bytes");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression feature source text"
    ));
}

#[test]
fn curve_expression_named_properties_refuse_before_second_tree() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let assignment = &record.assignments[0];
    let parameter_id = cadmpeg_ir::features::ParameterId::mint("test:test:parameter#a")
        .expect("valid parameter id");
    let assignment_indices = std::collections::BTreeMap::new();
    let unique_indices = std::collections::BTreeMap::new();
    let dimensions = std::collections::BTreeMap::new();
    let edges = std::collections::HashSet::new();
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::curve_expression_properties(
            ctx,
            assignment,
            0,
            ("a", &parameter_id),
            (&assignment_indices, &unique_indices),
            &dimensions,
            &edges,
        ))
        .expect("service profile admits both property maps")
        .len(),
        2
    );
    let error = crate::test_support::last_refusal_at(
        payload,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "named entry map nodes",
        |ctx| {
            super::curve_expression_properties(
                ctx,
                assignment,
                0,
                ("a", &parameter_id),
                (&assignment_indices, &unique_indices),
                &dimensions,
                &edges,
            )
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "named entry map nodes"
    ));
}

#[test]
fn curve_expression_native_parameters_refuse_before_tree_creation() {
    let dimensions = std::collections::BTreeMap::new();
    assert_eq!(
        transfer_with_limits(
            &["a=1"],
            &dimensions,
            cadmpeg_core::decode::DecodePolicy::service()
        )
        .expect("service profile admits native feature fallback"),
        1
    );
    let mut limited = cadmpeg_core::decode::DecodePolicy::service();
    limited.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo curve-expression native parameters"),
        |cap| {
            let mut trial = limited;
            trial.limits.max_collection_items = cap;
            transfer_with_limits(&["a=1"], &dimensions, trial)
        },
    );
    let error = transfer_with_limits(&["a=1"], &dimensions, limited)
        .expect_err("the native parameter tree needs two more items");
    assert!(
        matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "creo curve-expression native parameters"
        ),
        "{error:?}"
    );
}

#[test]
fn curve_expression_native_parameter_keys_refuse_retained_copy() {
    let initial = "CurveFromEquation".len() + "7".len() + "1".len();
    for (_cap, operation) in [
        (
            initial + "entity_id".len() - 1,
            "creo curve-expression native entity key",
        ),
        (
            initial + "entity_id".len() + "assignment_count".len() - 1,
            "creo curve-expression native assignment key",
        ),
    ] {
        let error = crate::test_support::last_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |ctx| super::native_curve_expression_definition(ctx, 7, 1),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource) if resource.operation == operation)
        );
    }
}

#[test]
fn curve_expression_components_match_reachability_for_small_graphs() {
    for edges in 0u32..(1 << 9) {
        let mut dependencies = vec![Vec::new(); 3];
        let mut reverse = vec![Vec::new(); 3];
        let mut reachable = [[false; 3]; 3];
        for from in 0..3 {
            reachable[from][from] = true;
            for to in 0..3 {
                if edges & (1 << (from * 3 + to)) != 0 {
                    dependencies[from].push(to);
                    reverse[to].push(from);
                    reachable[from][to] = true;
                }
            }
        }
        for via in 0..3 {
            for from in 0..3 {
                for to in 0..3 {
                    reachable[from][to] |= reachable[from][via] && reachable[via][to];
                }
            }
        }
        let components = crate::decode::with_test_decode_ctx(|ctx| {
            super::expression_dependency_components(ctx, &dependencies, &reverse)
        })
        .expect("small graph fits service limits");
        for from in 0..3 {
            for to in 0..3 {
                assert_eq!(
                    components[from] == components[to],
                    reachable[from][to] && reachable[to][from],
                    "edges={edges}, from={from}, to={to}"
                );
            }
        }
    }
}

#[test]
fn dimension_dependency_aliases_keep_first_identity_and_assignment_order() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_ir::features::ParameterId;
    use std::collections::BTreeMap;

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\xe0\x0aexpression\0\xf8\x02a=1\0b=x+a+y+z\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete expression");
    assert_eq!(record.offset, 2);
    let dimensions = BTreeMap::from([
        (
            "x".to_owned(),
            ParameterId::mint("test:test:parameter#dimension").expect("dimension"),
        ),
        (
            "y".to_owned(),
            ParameterId::mint("test:test:parameter#dimension").expect("dimension alias"),
        ),
        (
            "z".to_owned(),
            ParameterId::mint("creo:depdb:curve_expression_parameter#7-2-0")
                .expect("assignment alias"),
        ),
    ]);
    let indices = crate::decode::with_test_decode_ctx(|ctx| {
        super::curve_expression_assignment_indices(ctx, &record)
    })
    .expect("indices");
    let cyclic = std::collections::HashSet::new();
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::curve_expression_parameter_dependencies(
            ctx,
            &record,
            1,
            &indices.by_name,
            &indices.unique,
            &cyclic,
            &dimensions,
        )
    };
    let dependencies = crate::decode::with_test_decode_ctx(run).expect("dependencies");
    assert_eq!(
        dependencies
            .iter()
            .map(ParameterId::as_str)
            .collect::<Vec<_>>(),
        [
            "creo:depdb:curve_expression_parameter#7-2-0",
            "test:test:parameter#dimension"
        ]
    );
    for operation in [
        "creo curve-expression dependency identity lookup",
        "creo curve-expression dimension identity lookup",
    ] {
        let error = crate::test_support::last_refusal_at(
            payload,
            ResourceDimension::WorkUnits,
            operation,
            run,
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource) if resource.dimension == ResourceDimension::WorkUnits && resource.operation == operation)
        );
    }
}
