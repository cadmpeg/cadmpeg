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
