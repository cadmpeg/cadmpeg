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
        &refuse::<CurveExpressionValue>("search('abc','b')", |policy| {
            policy.limits.max_work_units = 1 + 4;
        }),
        "creo relation text search work",
    );
}

#[test]
fn relation_length_refuses_scan_work() {
    assert_work(
        &refuse::<CurveExpressionValue>("string_length('abc')", |policy| {
            policy.limits.max_work_units = 1 + 3;
        }),
        "creo relation text length work",
    );
}

#[test]
fn relation_prefix_refuses_comparison_work() {
    assert_work(
        &refuse::<CurveExpressionValue>("string_starts('abc','a')", |policy| {
            policy.limits.max_work_units = 1 + 4;
        }),
        "creo relation text prefix work",
    );
}

#[test]
fn relation_suffix_refuses_comparison_work() {
    assert_work(
        &refuse::<CurveExpressionValue>("string_ends('abc','c')", |policy| {
            policy.limits.max_work_units = 1 + 4;
        }),
        "creo relation text suffix work",
    );
}

#[test]
fn relation_match_refuses_comparison_work() {
    assert_work(
        &refuse::<CurveExpressionValue>("string_match('abc','abc')", |policy| {
            policy.limits.max_work_units = 1 + 6;
        }),
        "creo relation text match work",
    );
}

#[test]
fn relation_regex_refuses_compile_work() {
    assert_work(
        &refuse::<CurveExpressionValue>("string_pattern('abc','a')", |policy| {
            policy.limits.max_work_units = 1 + 4;
        }),
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
        &refuse::<CurveExpressionValue>("string_pattern('abc','a')", |policy| {
            policy.limits.max_work_units =
                2 + 4 + 2 * cadmpeg_core::decode::u64_from_index(r"\A(?:a)\z".len());
        }),
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
    assert_eq!(matched, Some(CurveExpressionValue::Number(1.0)));
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
        CurveExpressionValue::Number(2.0),
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
                CurveExpressionValue::Number(2.0),
                CurveExpressionValue::Number(3.0)
            ],
            RelationEvaluationContext::default(),
            &ctx,
        )
        .expect("numeric power needs no retained copy"),
        Some(CurveExpressionValue::Number(8.0))
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
