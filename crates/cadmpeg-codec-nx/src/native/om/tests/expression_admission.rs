use crate::container;
use crate::test_support::test_prt::prt_with_indexed_om_section;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn declaration_limit_error(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
    let file = prt_with_indexed_om_section();
    let scan_arena = DecodeArena::new();
    let scan_policy = DecodePolicy::service();
    let (scan_ctx, _) = DecodeContext::from_root_bytes(&file, &scan_arena, &scan_policy).unwrap();
    let container = container::scan_bytes(&scan_ctx, file.as_slice()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::expression_declarations(&ctx, &container).unwrap_err()
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
    let scan_arena = DecodeArena::new();
    let scan_policy = DecodePolicy::service();
    let (scan_ctx, _) = DecodeContext::from_root_bytes(&file, &scan_arena, &scan_policy).unwrap();
    let container = container::scan_bytes(&scan_ctx, file.as_slice()).unwrap();
    let declarations = super::super::expression_declarations(&scan_ctx, &container).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::expressions(&ctx, &container, &declarations).unwrap_err()
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
fn native_expression_unit_property_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let unit = super::super::ExpressionUnit::Native("custom/unit".to_string());
    let error = unit.property_name(&ctx).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}
