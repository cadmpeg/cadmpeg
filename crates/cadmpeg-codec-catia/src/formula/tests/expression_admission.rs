//! Resource refusal at recursive formula expression boundaries.

use std::collections::BTreeMap;

#[test]
fn formula_unary_descent_propagates_caller_depth_refusal() {
    crate::test_support::with_depth_limit(0, |ctx| {
        let result =
            super::super::evaluate_formula_expression_charged(ctx, "++++1", &BTreeMap::new());
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
        let result =
            super::super::evaluate_formula_expression_charged(ctx, "1**1**1", &BTreeMap::new());
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
fn formula_local_unary_depth_refuses_instead_of_returning_absence() {
    let expression = format!("{}1", "+".repeat(129));
    crate::test_support::with_depth_limit(256, |ctx| {
        let result =
            super::super::evaluate_formula_expression_charged(ctx, &expression, &BTreeMap::new());
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
            panic!("resource refusal required")
        };
        assert_eq!(limit.operation, "catia_formula_expression_local_depth");
        assert_eq!(limit.limit, 128);
        assert_eq!(limit.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn formula_bound_string_operations_admit_operand_work() {
    use super::super::{EvaluatedFormulaString, EvaluatedFormulaValue};
    for (source, operation, literal_bytes) in [
        ("#1_.Length()", "catia_formula_string_length", 0),
        ("#1_.Search(\"x\")", "catia_formula_string_search", 1),
        ("#1_.Extract(0,1)", "catia_formula_string_boundary", 0),
        ("#1_.ToReal()", "catia_formula_string_real", 0),
        (
            "ReplaceSubText(#1_,\"x\",\"y\")",
            "catia_formula_replace_work",
            2,
        ),
        ("ToUpper(#1_)", "catia_formula_case_work", 0),
    ] {
        let bindings = BTreeMap::from([(
            "#1_",
            EvaluatedFormulaValue::String(EvaluatedFormulaString::from_parts(
                "x".repeat(4096),
                true,
            )),
        )]);
        // The source scan, the evaluator's copy of the 4096-byte bound string, and each
        // literal's copy precede the operation's own operand work.
        crate::test_support::with_work_limit(
            u64::try_from(source.len()).expect("source length") + 4096 + literal_bytes,
            |ctx| {
                let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
                    super::super::evaluate_formula_expression_charged(ctx, source, &bindings)
                else {
                    panic!("operand work refusal required")
                };
                assert_eq!(limit.operation, operation);
                assert_eq!(ctx.resource_refusal(), Some(limit));
            },
        );
    }
}
