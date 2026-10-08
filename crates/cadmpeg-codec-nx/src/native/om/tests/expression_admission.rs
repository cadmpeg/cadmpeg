use crate::container;
use crate::test_support::test_prt::prt_with_indexed_om_section;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn declaration_limit_error(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
    let file = prt_with_indexed_om_section();

    crate::test_support::with_decode_context_over(
        &file,
        |_| {},
        |scan_ctx| {
            let container = container::scan_bytes(scan_ctx, file.as_slice()).unwrap();

            crate::test_support::with_decode_context_over(
                &[],
                |policy| {
                    configure(policy);
                },
                |ctx| super::super::expression_declarations(ctx, &container).unwrap_err(),
            )
        },
    )
}

#[test]
fn expression_declaration_route_refuses_collection_limit() {
    let error = declaration_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn expression_declaration_route_refuses_retained_limit() {
    let error = declaration_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn expression_declaration_route_refuses_work_limit() {
    let error = declaration_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits));
}

fn expression_limit_error(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
    let file = prt_with_indexed_om_section();

    crate::test_support::with_decode_context_over(
        &file,
        |_| {},
        |scan_ctx| {
            let container = container::scan_bytes(scan_ctx, file.as_slice()).unwrap();
            let declarations = super::super::expression_declarations(scan_ctx, &container).unwrap();

            crate::test_support::with_decode_context_over(
                &[],
                |policy| {
                    configure(policy);
                },
                |ctx| super::super::expressions(ctx, &container, &declarations).unwrap_err(),
            )
        },
    )
}

#[test]
fn native_expression_route_refuses_collection_limit() {
    let error = expression_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn native_expression_route_refuses_retained_limit() {
    let error = expression_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn native_expression_route_refuses_scoped_limit() {
    let error = expression_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes));
}

#[test]
fn native_expression_route_refuses_work_limit() {
    let error = expression_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits));
}

#[test]
fn native_expression_record_directory_ordinal_parse_refuses_named_work() {
    let file = prt_with_indexed_om_section();
    let container = crate::test_support::with_decode_context(|ctx| {
        let container = container::scan_bytes(ctx, file.as_slice())?;
        // Build the section cache once so every walk step charges the same route.
        container.indexed_om_sections(ctx).map(|(sections, _storage)| sections)?;
        Ok::<_, CodecError>(container)
    })
    .expect("indexed expression container");
    let declarations = crate::test_support::with_decode_context(|ctx| {
        super::super::expression_declarations(ctx, &container)
    })
    .expect("expression declarations");
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "parse NX expression record-directory ordinal",
        |ctx| super::super::expressions(ctx, &container, &declarations).map(|_| ()),
    );
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "parse NX expression record-directory ordinal"
    ));
}

#[test]
fn native_expression_malformed_record_directory_ordinal_skips_declaration() {
    let file = prt_with_indexed_om_section();
    let container = crate::test_support::with_decode_context(|ctx| {
        let container = container::scan_bytes(ctx, file.as_slice())?;
        // Build the section cache once so every walk step charges the same route.
        container.indexed_om_sections(ctx).map(|(sections, _storage)| sections)?;
        Ok::<_, CodecError>(container)
    })
    .expect("indexed expression container");
    let mut declarations = crate::test_support::with_decode_context(|ctx| {
        super::super::expression_declarations(ctx, &container)
    })
    .expect("expression declarations");
    assert_eq!(declarations.len(), 1);
    declarations[0].record = "nx:om-record-directory-invalid:entry#0".to_owned();
    let expressions = crate::test_support::with_decode_context(|ctx| {
        super::super::expressions(ctx, &container, &declarations)
    })
    .expect("malformed directory reference remains an unmatched declaration");
    assert_eq!(expressions.len(), 1);
    assert!(expressions[0].declaration.is_none());
}

#[test]
fn native_expression_unit_property_refuses_retained_limit() {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            let unit = super::super::ExpressionUnit::Native("custom/unit".to_string());
            let error = unit.property_name(ctx).unwrap_err();
            assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
        },
    );
}

fn expression_search_refusal(operation: &str, declarations_needed: bool) {
    let file = prt_with_indexed_om_section();
    let container = crate::test_support::with_decode_context(|ctx| {
        let container = container::scan_bytes(ctx, file.as_slice())?;
        // Build the section cache once so every walk step charges the same route.
        container.indexed_om_sections(ctx).map(|(sections, _storage)| sections)?;
        Ok::<_, CodecError>(container)
    })
    .expect("indexed expression container");
    let declarations = crate::test_support::with_decode_context(|ctx| {
        super::super::expression_declarations(ctx, &container)
    })
    .expect("expression declarations");
    assert_eq!(declarations.len(), 1);
    let error = crate::test_support::resource_refusal_at(
        container.data.as_ref(),
        ResourceDimension::WorkUnits,
        operation,
        |ctx| {
            if declarations_needed {
                super::super::expressions(ctx, &container, &declarations).map(|_| ())
            } else {
                super::super::expression_declarations(ctx, &container).map(|_| ())
            }
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn expression_registry_search_preserves_work_refusal() {
    expression_search_refusal("NX expression declaration registry types", false);
}

#[test]
fn expression_candidate_search_preserves_work_refusal() {
    expression_search_refusal("NX expression declaration candidates", true);
}

#[test]
fn expression_record_split_preserves_work_refusal() {
    expression_search_refusal("NX expression declaration record split", true);
}
