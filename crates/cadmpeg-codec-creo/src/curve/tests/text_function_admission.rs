// SPDX-License-Identifier: Apache-2.0
use crate::curve::{CurveExpressionValue, ExpressionValue, RelationEvaluationContext};
use std::collections::BTreeMap;

#[test]
fn relation_regex_preserves_service_match_and_invalid_pattern() {
    let values = BTreeMap::new();
    let matched = crate::decode::with_service_context(|ctx| {
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
    let invalid = crate::decode::with_service_context(|ctx| {
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
    assert_eq!(
        CurveExpressionValue::function_checked(
            CreoMathFunction::Pow,
            None,
            &arguments,
            RelationEvaluationContext::default()
        )
        .expect("invalid text needs no retained copy"),
        None
    );
    assert_eq!(
        crate::decode::with_service_context(|ctx| crate::curve::parse_relation_expression::<
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
            RelationEvaluationContext::default()
        )
        .expect("numeric power needs no retained copy"),
        Some(CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(8.0).expect("finite relation fixture")
        ))
    );
}

#[test]
fn relation_unit_symbols_match_ascii_case_insensitively() {
    for (expression, expected) in [
        ("2[cM]", Some(20.0)),
        ("1[mSeC]", Some(0.001)),
        ("0[c]", Some(273.15)),
        ("1[MpA]", Some(1_000.0)),
        ("1[Kelvin]", None),
        ("1[\u{212a}]", None),
    ] {
        assert_eq!(
            crate::decode::with_service_context(
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

#[test]
fn relation_itos_formats_a_negative_integer() {
    assert_eq!(
        crate::decode::with_service_context(|ctx| {
            crate::curve::parse_relation_expression::<CurveExpressionValue>(
                ctx,
                "itos(-1)",
                &BTreeMap::new(),
                RelationEvaluationContext::default(),
            )
        })
        .expect("argument and result storage")
        .expect("text value"),
        CurveExpressionValue::String("-1".to_owned())
    );
}
