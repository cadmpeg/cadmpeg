// SPDX-License-Identifier: Apache-2.0
use crate::curve::{CurveExpressionValue, DimensionProbeValue, ExpressionValue, RelationEvaluationContext};
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

fn assert_work(error: CodecError, operation: &str) {
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == operation));
}

#[test]
fn relation_search_refuses_scan_work() {
    assert_work(refuse::<CurveExpressionValue>("search('abc','b')", |policy| {
        policy.limits.max_work_units = 1;
    }), "creo relation text search work");
}

#[test]
fn relation_length_refuses_scan_work() {
    assert_work(refuse::<CurveExpressionValue>("string_length('abc')", |policy| {
        policy.limits.max_work_units = 1;
    }), "creo relation text length work");
}

#[test]
fn relation_prefix_refuses_comparison_work() {
    assert_work(refuse::<CurveExpressionValue>("string_starts('abc','a')", |policy| {
        policy.limits.max_work_units = 1;
    }), "creo relation text prefix work");
}

#[test]
fn relation_suffix_refuses_comparison_work() {
    assert_work(refuse::<CurveExpressionValue>("string_ends('abc','c')", |policy| {
        policy.limits.max_work_units = 1;
    }), "creo relation text suffix work");
}

#[test]
fn relation_match_refuses_comparison_work() {
    assert_work(refuse::<CurveExpressionValue>("string_match('abc','abc')", |policy| {
        policy.limits.max_work_units = 1;
    }), "creo relation text match work");
}

#[test]
fn relation_regex_refuses_compile_work() {
    assert_work(refuse::<CurveExpressionValue>("string_pattern('abc','a')", |policy| {
        policy.limits.max_work_units = 1;
    }), "creo relation regex compile work");
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
    assert_work(refuse::<CurveExpressionValue>("string_pattern('abc','a')", |policy| {
        policy.limits.max_work_units = 2;
    }), "creo relation regex match work");
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
            ctx, "string_pattern('abc','a.c')", &values, RelationEvaluationContext::default(),
        )
    }).expect("service pattern");
    assert_eq!(matched, Some(CurveExpressionValue::Number(1.0)));
    let invalid = crate::decode::with_test_decode_ctx(|ctx| {
        crate::curve::parse_relation_expression::<CurveExpressionValue>(
            ctx, "string_pattern('abc','[')", &values, RelationEvaluationContext::default(),
        )
    }).expect("service invalid pattern");
    assert_eq!(invalid, None);
}
