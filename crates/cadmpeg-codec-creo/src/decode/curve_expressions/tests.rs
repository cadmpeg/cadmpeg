// SPDX-License-Identifier: Apache-2.0

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

    assert!(crate::curve::expression_helix(&record).is_some());
    assert!(super::curve_expression_helix_definition(&record).is_some());
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

    let mut limited = service;
    limited.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &limited)
        .expect("root bytes are within the limit");
    let err = super::curve_expression_parameter_order(&ctx, &record, &unique_assignment_indices)
        .expect_err("two dependency rows exceed one collection item");
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

    let mut policy = service;
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("root bytes are within the limit");
    let err = super::curve_expression_parameter_order(&ctx, &record, &unique_assignment_indices)
        .expect_err("the dependency index follows two admitted outer rows");
    assert!(matches!(
        err,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo curve-expression dependency indices"
    ));
}

fn with_collection_limit<T>(
    limit: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"x", &arena, &policy).expect("small input is admitted");
    run(&ctx)
}

#[test]
fn curve_expression_pending_start_refuses_before_allocation() {
    let dependencies = [vec![1], vec![]];
    let error = with_collection_limit(0, |ctx| {
        super::expression_dependency_reaches(ctx, &dependencies, 0, 1)
    })
    .expect_err("the pending start needs one collection item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression pending dependency"
    ));
}

#[test]
fn curve_expression_visit_marks_refuse_before_allocation() {
    let dependencies = [vec![1], vec![]];
    let error = with_collection_limit(1, |ctx| {
        super::expression_dependency_reaches(ctx, &dependencies, 0, 1)
    })
    .expect_err("the visited lane needs two more collection items");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression visited dependencies"
    ));
}

#[test]
fn curve_expression_pending_growth_refuses_before_push() {
    let dependencies = [vec![1], vec![]];
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        super::expression_dependency_reaches(ctx, &dependencies, 0, 1)
    })
    .expect("service profile admits graph walk"));
    let error = with_collection_limit(3, |ctx| {
        super::expression_dependency_reaches(ctx, &dependencies, 0, 1)
    })
    .expect_err("the dependency edge needs a fourth collection item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression pending dependencies"
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
    let error = with_collection_limit(8, |ctx| {
        super::curve_expression_parameter_order(ctx, &record, &indices)
    })
    .expect_err("the first cyclic edge needs a ninth item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression cyclic edges"
    ));
}

#[test]
fn curve_expression_assigned_ordinals_refuse_before_allocation() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0b=2\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let indices = std::collections::BTreeMap::new();
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        super::curve_expression_parameter_order(ctx, &record, &indices)
    })
    .expect("service profile admits ordinal assignment")
    .is_some());
    let error = with_collection_limit(5, |ctx| {
        super::curve_expression_parameter_order(ctx, &record, &indices)
    })
    .expect_err("the assigned marks follow two dependency rows and two ordinals");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression assigned ordinals"
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
    let error = with_collection_limit(0, |ctx| {
        super::curve_expression_parameter_names(ctx, &record.assignments)
    })
    .expect_err("the first unique name needs one tree entry");
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
    let error = with_collection_limit(1, |ctx| {
        super::curve_expression_parameter_names(ctx, &record.assignments)
    })
    .expect_err("the occurrence follows one unique name");
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
    let error = with_collection_limit(1, |ctx| {
        super::curve_expression_parameter_names(ctx, &record.assignments)
    })
    .expect_err("the name slot follows one unique name");
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
    policy.limits.max_retained_bytes = 0;
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
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0a=2\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("input fits the root limit");
    let error = super::curve_expression_parameter_names(&ctx, &record.assignments)
        .expect_err("the first suffix needs two more retained bytes");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo curve-expression parameter suffix"
    ));
}

#[test]
fn curve_expression_parameter_name_refuses_before_text_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("input fits the root limit");
    let error = super::curve_expression_parameter_names(&ctx, &record.assignments)
        .expect_err("the output name follows two admitted key copies");
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
    })
    .expect("service profile admits assignment maps");
    assert_eq!(by_name.len(), 2);
    assert_eq!(unique.len(), 2);

    let error = with_collection_limit(0, |ctx| {
        super::curve_expression_assignment_indices(ctx, &record)
    })
    .expect_err("the first assignment needs a tree entry");
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
    let error = with_collection_limit(2, |ctx| {
        super::curve_expression_assignment_indices(ctx, &record)
    })
    .expect_err("the first unique index follows two assignment entries");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo curve-expression unique indices"
    ));
}

fn assignment_indices_with_retained_limit(
    record: &crate::curve::CurveExpressionRecord,
    retained_limit: u64,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"a", &arena, &policy).expect("root input fits the limit");
    super::curve_expression_assignment_indices(&ctx, record)
        .expect_err("the selected assignment key exceeds retained bytes")
}

#[test]
fn curve_expression_assignment_key_refuses_before_text_copy() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let error = assignment_indices_with_retained_limit(&record, 0);
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
    let error = assignment_indices_with_retained_limit(&record, 1);
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

    let error = with_collection_limit(1, |ctx| {
        super::curve_expression_emitted_ordinals(ctx, &record, &[0, 1])
    })
    .expect_err("two emitted indices exceed one collection item");
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
    let error = with_collection_limit(2, |ctx| {
        super::curve_expression_emitted_ordinals(ctx, &record, &[0, 1])
    })
    .expect_err("the first tree entry follows two emitted indices");
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
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
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
    limited.limits.max_collection_items = 9;
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
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
        .expect("root bytes fit the configured limit");
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.curves.expressions.push(record);
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let mut carriers = crate::decode::source_carriers::SourceUnitCarriers::new(None);
    super::transfer_curve_expression_features(
        &ctx,
        &scan,
        &mut ir,
        &mut annotations,
        dimension_parameters,
        &mut carriers,
    )
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
    limited.limits.max_materialized_bytes = 0;
    let error = transfer_with_limits(&["a=1", "b=a+1"], &dimensions, limited)
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
    limited.limits.max_collection_items = 24;
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
    limited.limits.max_collection_items = 25;
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
    limited.limits.max_collection_items = 11;
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
    limited.limits.max_collection_items = 10;
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
    limited.limits.max_materialized_bytes = 0;
    let error = transfer_with_limits(&["a=x+1"], &dimensions, limited)
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
    limited.limits.max_retained_bytes = 5;
    let error = transfer_with_limits(&["a=x+1"], &dimensions, limited)
        .expect_err("the dimension ID needs retained bytes");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo curve-expression dimension parameter id")
    );
}

#[test]
fn curve_expression_dependency_validation_charges_comparisons() {
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
    let mut limited = DecodePolicy::service();
    limited.limits.max_work_units = 2;
    let error = transfer_with_limits(&["a=1", "b=2", "c=a+b"], &dimensions, limited)
        .expect_err("validating two dependencies needs one comparison");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "validate Creo curve-expression dependency uniqueness")
    );
}
