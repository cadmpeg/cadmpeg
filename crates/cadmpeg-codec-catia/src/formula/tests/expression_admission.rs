//! Resource refusal at recursive formula expression boundaries.

use std::collections::BTreeMap;

#[test]
fn formula_unary_descent_propagates_caller_depth_refusal() {
    crate::test_support::with_depth_limit(0, |ctx| {
        let result = super::super::parse_formula_expression(ctx, "++++1", &BTreeMap::new());
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
            panic!("resource refusal required")
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::RecursionDepth
        );
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn formula_power_descent_propagates_caller_depth_refusal() {
    crate::test_support::with_depth_limit(0, |ctx| {
        let result = super::super::parse_formula_expression(ctx, "1**1**1", &BTreeMap::new());
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
            panic!("resource refusal required")
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::RecursionDepth
        );
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn formula_local_unary_depth_returns_absence_without_fusing_the_session() {
    let expression = format!("{}1", "+".repeat(129));
    crate::test_support::with_depth_limit(256, |ctx| {
        let result = super::super::parse_formula_expression(ctx, &expression, &BTreeMap::new());
        assert!(result
            .expect("the local depth ceiling is not a session refusal")
            .is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
}
