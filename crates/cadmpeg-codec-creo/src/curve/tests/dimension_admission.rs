// SPDX-License-Identifier: Apache-2.0
use crate::curve::{DimensionProbeKind, DimensionProbeValue, RelationEvaluationContext};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

fn refuse(
    expression: &str,
    values: &BTreeMap<String, DimensionProbeValue>,
    configure: impl FnOnce(&mut DecodePolicy),
) -> CodecError {
    refuse_with_context(
        expression,
        values,
        RelationEvaluationContext::default(),
        configure,
    )
}

fn refuse_with_context(
    expression: &str,
    values: &BTreeMap<String, DimensionProbeValue>,
    context: RelationEvaluationContext<'_>,
    configure: impl FnOnce(&mut DecodePolicy),
) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    crate::curve::parse_relation_expression(&ctx, expression, values, context)
        .expect_err("dimension expression allocation must refuse")
}

#[test]
fn dimension_integer_function_refuses_retained_text() {
    let error = refuse("itos(2)", &BTreeMap::new(), |policy| {
        policy.limits.max_retained_bytes = 0;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension integer text"));
}

#[test]
fn dimension_real_function_refuses_retained_text() {
    let error = refuse("rtos(1.25,2)", &BTreeMap::new(), |policy| {
        policy.limits.max_retained_bytes = 0;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo relation real text"));
}

#[test]
fn dimension_model_name_refuses_retained_copy() {
    let error = refuse_with_context(
        "rel_model_name()",
        &BTreeMap::new(),
        RelationEvaluationContext {
            model_name: Some("widget"),
            ..RelationEvaluationContext::default()
        },
        |policy| policy.limits.max_retained_bytes = 0,
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension model name text"));
}

#[test]
fn dimension_model_type_refuses_retained_copy() {
    let error = refuse("rel_model_type()", &BTreeMap::new(), |policy| {
        policy.limits.max_retained_bytes = 0;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension model type text"));
}

#[test]
fn dimension_exists_refuses_scoped_lookup_key() {
    let symbols = BTreeSet::from(["driver".to_owned()]);
    let error = refuse_with_context(
        "exists('driver')",
        &BTreeMap::new(),
        RelationEvaluationContext {
            existing_symbols: Some(&symbols),
            ..RelationEvaluationContext::default()
        },
        |policy| policy.limits.max_materialized_bytes = 0,
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo dimension exists lookup key"));
}

#[test]
fn dimension_search_refuses_scan_work() {
    let error = refuse("search('abc','b')", &BTreeMap::new(), |policy| {
        policy.limits.max_work_units = 1;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo dimension text search work"));
}

#[test]
fn dimension_extract_refuses_control_constraint_growth() {
    assert_collection(
        &refuse("extract('abc',2,1)", &BTreeMap::new(), |policy| {
            policy.limits.max_collection_items = 3;
        }),
        "creo dimension constraint growth",
    );
}

#[test]
fn dimension_extract_refuses_scan_work() {
    let error = refuse("extract('abc',2,1)", &BTreeMap::new(), |policy| {
        policy.limits.max_work_units = 1;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo dimension text extract work"));
}

#[test]
fn dimension_extract_refuses_retained_text() {
    let error = refuse("extract('abc',2,1)", &BTreeMap::new(), |policy| {
        policy.limits.max_retained_bytes = 3;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension extracted text"));
}

#[test]
fn dimension_length_refuses_scan_work() {
    let error = refuse("string_length('abc')", &BTreeMap::new(), |policy| {
        policy.limits.max_work_units = 1;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo dimension text length work"));
}

#[test]
fn dimension_conditional_refuses_retained_text() {
    let error = refuse("if(1,'a','b')", &BTreeMap::new(), |policy| {
        policy.limits.max_retained_bytes = 2;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension conditional text"));
}

#[test]
fn dimension_numeric_function_refuses_constraint_growth() {
    assert_collection(
        &refuse("sin(1)", &BTreeMap::new(), |policy| {
            policy.limits.max_collection_items = 1;
        }),
        "creo dimension operation constraints",
    );
}

#[test]
fn dimension_round_control_refuses_constraint_growth() {
    assert_collection(
        &refuse("ceil(1,2)", &BTreeMap::new(), |policy| {
            policy.limits.max_collection_items = 2;
        }),
        "creo dimension constraint growth",
    );
}

fn variables() -> BTreeMap<String, DimensionProbeValue> {
    crate::decode::with_test_decode_ctx(|ctx| -> Result<_, CodecError> {
        Ok(BTreeMap::from([
            (
                "driver".to_owned(),
                DimensionProbeValue::variable(ctx, "x")?,
            ),
            ("other".to_owned(), DimensionProbeValue::variable(ctx, "y")?),
        ]))
    })
    .expect("service variables")
}

fn assert_collection(error: &CodecError, operation: &str) {
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation));
}

#[test]
fn dimension_unit_refuses_constraint_growth() {
    assert_collection(
        &refuse("1[mm]", &BTreeMap::new(), |policy| {
            policy.limits.max_collection_items = 0;
        }),
        "creo dimension unit constraints",
    );
}

#[test]
fn dimension_sum_refuses_constraint_growth() {
    assert_collection(
        &refuse("1+2", &BTreeMap::new(), |policy| {
            policy.limits.max_collection_items = 0;
        }),
        "creo dimension operation constraints",
    );
}

#[test]
fn dimension_difference_refuses_constraint_growth() {
    assert_collection(
        &refuse("1-2", &BTreeMap::new(), |policy| {
            policy.limits.max_collection_items = 0;
        }),
        "creo dimension operation constraints",
    );
}

#[test]
fn dimension_comparison_refuses_constraint_growth() {
    assert_collection(
        &refuse("1==2", &BTreeMap::new(), |policy| {
            policy.limits.max_collection_items = 0;
        }),
        "creo dimension operation constraints",
    );
}

#[test]
fn dimension_logical_and_refuses_constraint_growth() {
    assert_collection(
        &refuse("1&2", &BTreeMap::new(), |policy| {
            policy.limits.max_collection_items = 0;
        }),
        "creo dimension operation constraints",
    );
}

#[test]
fn dimension_logical_or_refuses_constraint_growth() {
    assert_collection(
        &refuse("1|2", &BTreeMap::new(), |policy| {
            policy.limits.max_collection_items = 0;
        }),
        "creo dimension operation constraints",
    );
}

#[test]
fn dimension_logical_not_refuses_constraint_growth() {
    assert_collection(
        &refuse("!1", &BTreeMap::new(), |policy| {
            policy.limits.max_collection_items = 0;
        }),
        "creo dimension negation constraints",
    );
}

#[test]
fn dimension_power_refuses_constraint_growth() {
    assert_collection(
        &refuse("2^2", &BTreeMap::new(), |policy| {
            policy.limits.max_collection_items = 0;
        }),
        "creo dimension operation constraints",
    );
}

#[test]
fn dimension_existing_constraints_refuse_merge_copy() {
    assert_collection(
        &refuse("1[mm]+2", &BTreeMap::new(), |policy| {
            policy.limits.max_collection_items = 1;
        }),
        "creo dimension merged constraints",
    );
}

#[test]
fn dimension_multiplication_refuses_new_variable_node() {
    assert_collection(
        &refuse("driver*other", &variables(), |policy| {
            policy.limits.max_collection_items = 10;
        }),
        "creo dimension difference variable nodes",
    );
}

#[test]
fn dimension_division_refuses_new_variable_node() {
    assert_collection(
        &refuse("driver/other", &variables(), |policy| {
            policy.limits.max_collection_items = 10;
        }),
        "creo dimension difference variable nodes",
    );
}

#[test]
fn dimension_text_sum_refuses_left_copy() {
    let error = refuse("'a'+'b'", &BTreeMap::new(), |policy| {
        policy.limits.max_retained_bytes = 2;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension text sum left"));
}

#[test]
fn dimension_text_sum_refuses_right_growth() {
    let error = refuse("'a'+'b'", &BTreeMap::new(), |policy| {
        policy.limits.max_retained_bytes = 3;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension text sum right"));
}

#[test]
fn dimension_arithmetic_preserves_service_values() {
    let values = BTreeMap::new();
    let result = crate::decode::with_test_decode_ctx(|ctx| {
        crate::curve::parse_relation_expression::<DimensionProbeValue>(
            ctx,
            "2+3*4",
            &values,
            RelationEvaluationContext::default(),
        )
    })
    .expect("service expression")
    .expect("numeric result");
    assert!(matches!(
        result.kind,
        DimensionProbeKind::Numeric(Some(14.0))
    ));
    assert_eq!(result.constraints.len(), 1);
}

fn dimension_conversion_result(
    name: &str,
) -> Result<Option<Vec<crate::curve::RelationDimension>>, CodecError> {
    use crate::curve::{
        CurveExpressionEquation, CurveExpressionSolveBlock, CurveExpressionValue, SolveUnknown,
    };
    let mut expression = name.to_owned();
    for _ in 0..8 {
        expression = format!("({expression}^127)");
    }
    let block = CurveExpressionSolveBlock {
        equations: vec![CurveExpressionEquation {
            left: expression,
            right: "1".to_owned(),
            dependencies: vec![name.to_owned()],
            offset: 0,
        }],
        assignments: Vec::new(),
        unknowns: vec![SolveUnknown {
            name: "x".to_owned(),
            solution: None,
        }],
        offset: 0,
        for_offset: 1,
    };
    let values = BTreeMap::from([("length".to_owned(), CurveExpressionValue::Length(1.0))]);
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::curve::infer_solve_variable_dimensions(
            ctx,
            &block,
            &values,
            &[None],
            RelationEvaluationContext::default(),
        )
    })
}

#[test]
fn dimension_inference_refuses_inexact_integer_coefficient() {
    let error =
        dimension_conversion_result("x").expect_err("127 to the eighth power is not exact in f64");
    assert!(matches!(error, CodecError::Malformed(_)));
}

#[test]
fn dimension_inference_refuses_inexact_integer_constant() {
    let error = dimension_conversion_result("length")
        .expect_err("127 to the eighth power is not exact in f64");
    assert!(matches!(error, CodecError::Malformed(_)));
}
