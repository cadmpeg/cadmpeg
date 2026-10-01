//! Resource refusal at recursive formula expression boundaries.

use std::collections::BTreeMap;

#[test]
fn formula_unary_descent_propagates_caller_depth_refusal() {
    crate::test_support::with_depth_limit(0, |ctx| {
        let result = super::super::evaluate_formula_expression_charged(ctx, "++++1", &BTreeMap::new());
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result
            else { panic!("resource refusal required") };
        assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::RecursionDepth);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn formula_power_descent_propagates_caller_depth_refusal() {
    crate::test_support::with_depth_limit(0, |ctx| {
        let result = super::super::evaluate_formula_expression_charged(ctx, "1**1**1", &BTreeMap::new());
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result
            else { panic!("resource refusal required") };
        assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::RecursionDepth);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn formula_local_unary_depth_refuses_instead_of_returning_absence() {
    let expression = format!("{}1", "+".repeat(129));
    crate::test_support::with_depth_limit(256, |ctx| {
        let result = super::super::evaluate_formula_expression_charged(ctx, &expression, &BTreeMap::new());
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result
            else { panic!("resource refusal required") };
        assert_eq!(limit.operation, "catia_formula_expression_local_depth");
        assert_eq!(limit.limit, 128);
        assert_eq!(limit.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}
