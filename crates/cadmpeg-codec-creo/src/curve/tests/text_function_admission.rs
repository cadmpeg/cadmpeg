// SPDX-License-Identifier: Apache-2.0
use crate::curve::{
    CurveExpressionValue, DimensionProbeValue, ExpressionValue, RelationEvaluationContext,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn refuse<V: ExpressionValue + std::fmt::Debug>(
    expression: &str,
    configure: impl FnOnce(&mut DecodePolicy),
) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    crate::curve::parse_relation_expression::<V>(
        &ctx,
        expression,
        &BTreeMap::new(),
        RelationEvaluationContext::default(),
    )
    .expect_err("string function allocation must refuse")
}

fn assert_work(error: &CodecError, operation: &str) {
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == operation));
}

#[test]
fn relation_search_refuses_scan_work() {
    assert_work(
        &crate::test_support::last_refusal_at(
            ResourceDimension::WorkUnits,
            "creo relation text search work",
            |ctx| {
                crate::curve::parse_relation_expression::<CurveExpressionValue>(
                    ctx,
                    "search('abc','b')",
                    &BTreeMap::new(),
                    RelationEvaluationContext::default(),
                )
            },
        ),
        "creo relation text search work",
    );
}

#[test]
fn relation_length_refuses_scan_work() {
    assert_work(
        &crate::test_support::last_refusal_at(
            ResourceDimension::WorkUnits,
            "creo relation text length work",
            |ctx| {
                crate::curve::parse_relation_expression::<CurveExpressionValue>(
                    ctx,
                    "string_length('abc')",
                    &BTreeMap::new(),
                    RelationEvaluationContext::default(),
                )
            },
        ),
        "creo relation text length work",
    );
}

#[test]
fn relation_prefix_refuses_comparison_work() {
    assert_work(
        &crate::test_support::last_refusal_at(
            ResourceDimension::WorkUnits,
            "creo relation text prefix work",
            |ctx| {
                crate::curve::parse_relation_expression::<CurveExpressionValue>(
                    ctx,
                    "string_starts('abc','a')",
                    &BTreeMap::new(),
                    RelationEvaluationContext::default(),
                )
            },
        ),
        "creo relation text prefix work",
    );
}

#[test]
fn relation_suffix_refuses_comparison_work() {
    assert_work(
        &crate::test_support::last_refusal_at(
            ResourceDimension::WorkUnits,
            "creo relation text suffix work",
            |ctx| {
                crate::curve::parse_relation_expression::<CurveExpressionValue>(
                    ctx,
                    "string_ends('abc','c')",
                    &BTreeMap::new(),
                    RelationEvaluationContext::default(),
                )
            },
        ),
        "creo relation text suffix work",
    );
}

#[test]
fn relation_match_refuses_comparison_work() {
    assert_work(
        &crate::test_support::last_refusal_at(
            ResourceDimension::WorkUnits,
            "creo relation text match work",
            |ctx| {
                crate::curve::parse_relation_expression::<CurveExpressionValue>(
                    ctx,
                    "string_match('abc','abc')",
                    &BTreeMap::new(),
                    RelationEvaluationContext::default(),
                )
            },
        ),
        "creo relation text match work",
    );
}

#[test]
fn relation_regex_refuses_compile_work() {
    assert_work(
        &crate::test_support::last_refusal_at(
            ResourceDimension::WorkUnits,
            "creo relation regex compile work",
            |ctx| {
                crate::curve::parse_relation_expression::<CurveExpressionValue>(
                    ctx,
                    "string_pattern('abc','a')",
                    &BTreeMap::new(),
                    RelationEvaluationContext::default(),
                )
            },
        ),
        "creo relation regex compile work",
    );
}

#[test]
fn relation_regex_refuses_source_text() {
    let error = refuse::<CurveExpressionValue>("string_pattern('abc','a')", |policy| {
        policy.limits.max_materialized_bytes = 0;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo relation regex source text"));
}

#[test]
fn relation_regex_refuses_compiler_scratch() {
    let error = refuse::<CurveExpressionValue>("string_pattern('abc','a')", |policy| {
        policy.limits.max_materialized_bytes = 9;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo relation regex compiler scratch"));
}

#[test]
fn relation_regex_refuses_match_work() {
    assert_work(
        &crate::test_support::last_refusal_at(
            ResourceDimension::WorkUnits,
            "creo relation regex match work",
            |ctx| {
                crate::curve::parse_relation_expression::<CurveExpressionValue>(
                    ctx,
                    "string_pattern('abc','a')",
                    &BTreeMap::new(),
                    RelationEvaluationContext::default(),
                )
            },
        ),
        "creo relation regex match work",
    );
}

#[test]
fn dimension_regex_refuses_compiler_scratch() {
    let error = refuse::<DimensionProbeValue>("string_pattern('abc','a')", |policy| {
        policy.limits.max_materialized_bytes = 9;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo relation regex compiler scratch"));
}

#[test]
fn relation_regex_preserves_service_match_and_invalid_pattern() {
    let values = BTreeMap::new();
    let matched = crate::decode::with_test_decode_ctx(|ctx| {
        crate::curve::parse_relation_expression::<CurveExpressionValue>(
            ctx,
            "string_pattern('abc','a.c')",
            &values,
            RelationEvaluationContext::default(),
        )
    })
    .expect("service pattern");
    assert_eq!(
        matched,
        Some(CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")
        ))
    );
    let invalid = crate::decode::with_test_decode_ctx(|ctx| {
        crate::curve::parse_relation_expression::<CurveExpressionValue>(
            ctx,
            "string_pattern('abc','[')",
            &values,
            RelationEvaluationContext::default(),
        )
    })
    .expect("service invalid pattern");
    assert_eq!(invalid, None);
}

#[test]
fn relation_power_rejects_text_without_copying_the_invalid_operand() {
    use crate::curve::CreoMathFunction;
    let arguments = [
        CurveExpressionValue::String("abc".to_owned()),
        CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture"),
        ),
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert_eq!(
        CurveExpressionValue::function_checked(
            CreoMathFunction::Pow,
            None,
            &arguments,
            RelationEvaluationContext::default(),
            &ctx,
        )
        .expect("invalid text needs no retained copy"),
        None
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| crate::curve::parse_relation_expression::<
            CurveExpressionValue,
        >(
            ctx,
            "pow('abc',2)",
            &BTreeMap::new(),
            RelationEvaluationContext::default(),
        ))
        .expect("service text expression admission"),
        None
    );
    assert_eq!(
        CurveExpressionValue::function_checked(
            CreoMathFunction::Pow,
            None,
            &[
                CurveExpressionValue::Number(
                    cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture")
                ),
                CurveExpressionValue::Number(
                    cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("finite relation fixture")
                )
            ],
            RelationEvaluationContext::default(),
            &ctx,
        )
        .expect("numeric power needs no retained copy"),
        Some(CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(8.0).expect("finite relation fixture")
        ))
    );
}

#[test]
fn relation_unit_symbols_match_borrowed_text_at_zero_byte_limits() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    for (expression, expected) in [
        ("2[cM]", Some(20.0)),
        ("1[mSeC]", Some(0.001)),
        ("0[c]", Some(273.15)),
        ("1[MpA]", Some(1_000.0)),
        ("1[Kelvin]", None),
        ("1[\u{212a}]", None),
    ] {
        assert_eq!(
            crate::curve::parse_relation_expression::<f64>(
                &ctx,
                expression,
                &BTreeMap::new(),
                RelationEvaluationContext::default(),
            )
            .expect("borrowed unit lookup needs no byte admission"),
            expected,
            "{expression}"
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(
                |ctx| crate::curve::parse_relation_expression::<f64>(
                    ctx,
                    expression,
                    &BTreeMap::new(),
                    RelationEvaluationContext::default(),
                )
            )
            .expect("service unit lookup"),
            expected,
            "{expression}"
        );
    }
}
