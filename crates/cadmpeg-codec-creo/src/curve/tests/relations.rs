// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
const EPS_RELATION_VALUE: f64 = 1.0e-12;

use crate::curve::curve_equation_prohibited_constructs;
use crate::curve::evaluate_creo_math_function;
use crate::curve::evaluate_creo_numeric_relation_function;
use crate::curve::expression_records;
use crate::curve::reevaluate_expression_records;
use crate::curve::relation_round;
use crate::curve::tests::evaluate_expression_program;
use crate::curve::CreoMathFunction;
use crate::curve::CurveExpressionActivation;
use crate::curve::CurveExpressionLine;
use crate::curve::CurveExpressionQuantity;
use crate::curve::CurveExpressionSystemSymbolFamily;
use crate::curve::CurveExpressionTarget;
use crate::curve::CurveExpressionValue;
use crate::curve::DimensionEquality;
use crate::curve::DimensionProbeKind;
use crate::curve::DimensionProbeValue;
use crate::curve::ExpressionValue;
use crate::curve::ExternalRelationSymbols;
use crate::curve::RelationDimension;
use crate::curve::RelationEvaluationContext;
use crate::curve::SimultaneousAffineValue;
use crate::curve::SymbolicRelationDimension;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn evaluate_expression(expression: &str, values: &BTreeMap<String, f64>) -> Option<f64> {
    parse_relation_expression(expression, values, RelationEvaluationContext::default())
}

fn parse_relation_expression<V: ExpressionValue>(
    expression: &str,
    values: &BTreeMap<String, V>,
    context: RelationEvaluationContext<'_>,
) -> Option<V> {
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::curve::parse_relation_expression(ctx, expression, values, context)
    })
    .expect("service profile admits relation expression")
}

fn relation_parse_limit_error<V: ExpressionValue + std::fmt::Debug>(
    expression: &str,
    values: &BTreeMap<String, V>,
    configure: impl FnOnce(&mut DecodePolicy),
) -> CodecError {
    relation_parse_limit_error_with_context(
        expression,
        values,
        RelationEvaluationContext::default(),
        configure,
    )
}

fn relation_parse_limit_error_with_context<V: ExpressionValue + std::fmt::Debug>(
    expression: &str,
    values: &BTreeMap<String, V>,
    context: RelationEvaluationContext<'_>,
    configure: impl FnOnce(&mut DecodePolicy),
) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    crate::curve::parse_relation_expression(&ctx, expression, values, context)
        .expect_err("relation parser allocation must refuse")
}

#[test]
fn relation_integer_format_refuses_retained_text() {
    let error = relation_parse_limit_error(
        "itos(2)",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| policy.limits.max_retained_bytes = 0,
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo relation integer text"));
}

#[test]
fn relation_real_format_refuses_retained_text() {
    let error = relation_parse_limit_error(
        "rtos(1.234,2)",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| policy.limits.max_retained_bytes = 0,
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo relation real text"));
}

#[test]
fn relation_scientific_format_refuses_scratch_text() {
    let error = relation_parse_limit_error(
        "rtos(0.01234,2,TRUE)",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| policy.limits.max_materialized_bytes = 0,
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo relation scientific scratch"));
}

#[test]
fn relation_scientific_format_refuses_retained_text() {
    let error = relation_parse_limit_error(
        "rtos(0.01234,2,TRUE)",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| policy.limits.max_retained_bytes = 0,
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo relation real text"));
}

#[test]
fn relation_model_name_refuses_retained_copy() {
    let error = relation_parse_limit_error_with_context(
        "rel_model_name()",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        RelationEvaluationContext {
            model_name: Some("widget"),
            ..RelationEvaluationContext::default()
        },
        |policy| policy.limits.max_retained_bytes = 0,
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo relation model name value"));
}

#[test]
fn relation_model_type_refuses_retained_copy() {
    let error = relation_parse_limit_error(
        "rel_model_type()",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| policy.limits.max_retained_bytes = 0,
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo relation model type value"));
}

#[test]
fn relation_extract_refuses_scanning_work() {
    let error = relation_parse_limit_error(
        "extract('abc',2,1)",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| policy.limits.max_work_units = 1,
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo relation extract scan"));
}

#[test]
fn relation_extract_refuses_retained_substring() {
    let error = relation_parse_limit_error(
        "extract('abc',2,1)",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| policy.limits.max_retained_bytes = 3,
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo relation extracted text"));
}

#[test]
fn relation_extract_preserves_utf8_character_boundaries() {
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "extract('é漢x',2,1)",
            &BTreeMap::new(),
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::String("漢".to_owned()))
    );
}

#[test]
fn relation_conditional_string_refuses_retained_clone() {
    let error = relation_parse_limit_error(
        "if(1,'abc','def')",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| policy.limits.max_retained_bytes = 6,
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo relation conditional string"));
}

#[test]
fn relation_exists_refuses_scoped_lookup_key() {
    let symbols = std::collections::BTreeSet::from(["driver".to_owned()]);
    let error = relation_parse_limit_error_with_context(
        "exists('driver')",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        RelationEvaluationContext {
            existing_symbols: Some(&symbols),
            ..RelationEvaluationContext::default()
        },
        |policy| policy.limits.max_materialized_bytes = 0,
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo relation exists lookup key"));
}

#[test]
fn relation_referenced_string_refuses_retained_clone() {
    let values = BTreeMap::from([(
        "driver".to_owned(),
        CurveExpressionValue::String("abc".to_owned()),
    )]);
    let error = relation_parse_limit_error("driver", &values, |policy| {
        policy.limits.max_retained_bytes = 2;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo relation referenced string value"));
}

#[test]
fn relation_string_concatenation_refuses_retained_growth() {
    let error = relation_parse_limit_error(
        "'a'+'b'",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| policy.limits.max_retained_bytes = 2,
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo relation string concatenation"));
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "'a'+'b'",
            &BTreeMap::new(),
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::String("ab".to_owned()))
    );
}

fn affine_probe_values() -> BTreeMap<String, SimultaneousAffineValue> {
    BTreeMap::from([(
        "driver".to_owned(),
        SimultaneousAffineValue {
            dimension: RelationDimension::default(),
            constant: 0.0,
            coefficients: BTreeMap::from([("x".to_owned(), 1.0)]),
        },
    )])
}

fn distinct_affine_probe_values() -> BTreeMap<String, SimultaneousAffineValue> {
    let mut values = affine_probe_values();
    values.insert(
        "other".to_owned(),
        SimultaneousAffineValue {
            dimension: RelationDimension::default(),
            constant: 0.0,
            coefficients: BTreeMap::from([("y".to_owned(), 1.0)]),
        },
    );
    values
}

#[test]
fn relation_affine_sum_refuses_new_coefficient_node() {
    let error =
        relation_parse_limit_error("driver+other", &distinct_affine_probe_values(), |policy| {
            policy.limits.max_collection_items = 2;
        });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo affine combined coefficient nodes"));
}

#[test]
fn relation_affine_difference_refuses_new_coefficient_node() {
    let error =
        relation_parse_limit_error("driver-other", &distinct_affine_probe_values(), |policy| {
            policy.limits.max_collection_items = 2;
        });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo affine combined coefficient nodes"));
}

#[test]
fn relation_affine_comparison_refuses_coefficient_work() {
    let error = relation_parse_limit_error("driver==driver", &affine_probe_values(), |policy| {
        policy.limits.max_work_units = 0;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo affine coefficient comparison work"));
}

#[test]
fn relation_affine_function_refuses_selected_value_clone() {
    let error = relation_parse_limit_error(
        "if(1,driver,other)",
        &distinct_affine_probe_values(),
        |policy| {
            policy.limits.max_collection_items = 5;
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo relation affine clone coefficient nodes"));
}

#[test]
fn relation_affine_function_refuses_numeric_argument_vector() {
    let error = relation_parse_limit_error(
        "sin(1)",
        &BTreeMap::<String, SimultaneousAffineValue>::new(),
        |policy| {
            policy.limits.max_collection_items = 1;
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo affine function numeric arguments"));
}

fn dimension_probe_variable(name: &str) -> DimensionProbeValue {
    crate::decode::with_test_decode_ctx(|ctx| DimensionProbeValue::variable(ctx, name))
        .expect("service profile admits dimension variable")
}

#[test]
fn relation_affine_reference_refuses_coefficient_node() {
    let error = relation_parse_limit_error("driver", &affine_probe_values(), |policy| {
        policy.limits.max_collection_items = 0;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo relation affine clone coefficient nodes"));
}

#[test]
fn relation_affine_reference_refuses_coefficient_name() {
    let error = relation_parse_limit_error("driver", &affine_probe_values(), |policy| {
        policy.limits.max_retained_bytes = 0;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo relation affine clone coefficient names"));
}

#[test]
fn relation_dimension_reference_refuses_variable_node() {
    let values = BTreeMap::from([("driver".to_owned(), dimension_probe_variable("x"))]);
    let error = relation_parse_limit_error("driver", &values, |policy| {
        policy.limits.max_collection_items = 0;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo relation dimension clone variable nodes"));
}

#[test]
fn relation_dimension_reference_refuses_variable_name() {
    let values = BTreeMap::from([("driver".to_owned(), dimension_probe_variable("x"))]);
    let error = relation_parse_limit_error("driver", &values, |policy| {
        policy.limits.max_retained_bytes = 0;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo relation dimension clone variable names"));
}

#[test]
fn relation_dimension_reference_refuses_text_clone() {
    let values = BTreeMap::from([(
        "driver".to_owned(),
        DimensionProbeValue::text(Some("abc".to_owned())),
    )]);
    let error = relation_parse_limit_error("driver", &values, |policy| {
        policy.limits.max_retained_bytes = 2;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo relation dimension clone text"));
}

#[test]
fn relation_dimension_reference_refuses_constraint_vector() {
    let values = BTreeMap::from([(
        "driver".to_owned(),
        DimensionProbeValue {
            dimension: SymbolicRelationDimension::default(),
            kind: DimensionProbeKind::Numeric(None),
            constraints: vec![DimensionEquality {
                left: SymbolicRelationDimension::default(),
                right: SymbolicRelationDimension::default(),
            }],
        },
    )]);
    let error = relation_parse_limit_error("driver", &values, |policy| {
        policy.limits.max_collection_items = 0;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo relation dimension clone constraints"));
}

#[test]
fn relation_literal_text_refuses_retained_copy() {
    let error = relation_parse_limit_error(
        "'abc'",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| {
            policy.limits.max_retained_bytes = 2;
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo relation literal text"));
}

#[test]
fn relation_lookup_refuses_scoped_key_copy() {
    let values = BTreeMap::from([("driver".to_owned(), CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture")))]);
    let error = relation_parse_limit_error("DRIVER", &values, |policy| {
        policy.limits.max_materialized_bytes = 5;
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo relation lookup key"));
}

#[test]
fn relation_function_refuses_argument_vector() {
    let error = relation_parse_limit_error(
        "sin(1)",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| {
            policy.limits.max_collection_items = 0;
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo relation function arguments"));
}

#[test]
fn relation_function_refuses_recursive_step() {
    let error = relation_parse_limit_error(
        "sin(1)",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| {
            policy.limits.max_recursion_depth = 0;
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RecursionDepth
            && resource.operation == "creo relation function depth"));
}

#[test]
fn relation_group_refuses_recursive_step() {
    let error = relation_parse_limit_error(
        "(1)",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| {
            policy.limits.max_recursion_depth = 0;
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RecursionDepth
            && resource.operation == "creo relation group depth"));
}

#[test]
fn relation_function_refuses_work() {
    let error = relation_parse_limit_error(
        "sin(1)",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| {
            policy.limits.max_work_units = 0;
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo relation function work"));
}

#[test]
fn relation_exponent_refuses_recursive_step() {
    let error = relation_parse_limit_error(
        "2^3",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| {
            policy.limits.max_recursion_depth = 0;
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RecursionDepth
            && resource.operation == "creo relation exponent depth"));
}

#[test]
fn relation_exponent_refuses_work() {
    let error = relation_parse_limit_error(
        "2^3",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| {
            policy.limits.max_work_units = 0;
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo relation exponent work"));
}

#[test]
fn relation_group_refuses_work() {
    let error = relation_parse_limit_error(
        "(1)",
        &BTreeMap::<String, CurveExpressionValue>::new(),
        |policy| {
            policy.limits.max_work_units = 0;
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo relation group work"));
}

fn numeric_value(value: Option<&CurveExpressionValue>) -> f64 {
    let Some(CurveExpressionValue::Number(value)) = value else {
        panic!("expected evaluated numeric value")
    };
    value.get()
}

fn observe_relation_symbol(
    symbols: &mut ExternalRelationSymbols,
    name: &str,
    value: Option<CurveExpressionValue>,
) {
    crate::decode::with_test_decode_ctx(|ctx| symbols.observe(ctx, name.to_owned(), value))
        .expect("service profile admits relation symbol");
}

fn relation_symbol_error(
    max_collection_items: u64,
    max_retained_bytes: u64,
    max_materialized_bytes: u64,
) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    policy.limits.max_retained_bytes = max_retained_bytes;
    policy.limits.max_materialized_bytes = max_materialized_bytes;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut symbols = ExternalRelationSymbols::default();
    let result = (|| -> Result<(), CodecError> {
        let (name, _reservation) = ctx.format_scoped(
            format_args!("d{}", 42),
            "creo relation dimension symbol formatting",
        )?;
        symbols.observe(&ctx, name, Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture"))))
    })();
    result.expect_err("one relation symbol exceeds limit")
}

#[test]
fn relation_dimension_symbol_refuses_temporary_name_bytes() {
    let error = relation_symbol_error(1, 3, 0);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo relation dimension symbol formatting"));
}

#[test]
fn relation_dimension_symbol_refuses_retained_name_bytes() {
    let error = relation_symbol_error(1, 0, 3);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo external relation symbol names"));
}

#[test]
fn relation_dimension_symbol_refuses_map_node() {
    let error = relation_symbol_error(0, 3, 3);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo external relation symbol nodes"));
}

#[test]
fn declares_units_only_on_new_relation_parameters() {
    let lines = [
        "span[inch]=2",
        "copy=span+25.4[mm]",
        "span[mm]=50.8[mm]",
        "bad[degree]=1[mm]",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();
    let assignments =
        evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        assignments[0].parameter_target(),
        Some(("span", Some("inch")))
    );
    assert_eq!(
        assignments[0].value,
        Some(CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(50.8).expect("finite relation fixture")))
    );
    let Some(CurveExpressionValue::Length(copy)) = &assignments[1].value else {
        panic!("dimensioned copy");
    };
    assert!((copy.get() - 76.2).abs() < EPS_RELATION_VALUE);
    assert_eq!(
        assignments[2].parameter_target(),
        Some(("span", Some("mm")))
    );
    assert_eq!(assignments[2].value, None);
    assert_eq!(assignments[3].value, None);
}

#[test]
fn evaluates_creo_math_functions_without_treating_function_names_as_dependencies() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x05a=SIN(30)\0b=pow(a,2)+sqrt(9)\0\
        c=bound(12,0,10)+dead(3,1,2)\0d=custom(a)\0e=1e3\0";
    let records = expression_records(payload);
    let assignments = &records[0].assignments;

    assert!(assignments[0].dependencies.is_empty());
    assert!((numeric_value(assignments[0].value.as_ref()) - 0.5).abs() < EPS_RELATION_VALUE);
    assert_eq!(assignments[1].dependencies, ["a"]);
    assert!((numeric_value(assignments[1].value.as_ref()) - 3.25).abs() < EPS_RELATION_VALUE);
    assert!(assignments[2].dependencies.is_empty());
    assert_eq!(
        assignments[2].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(11.0).expect("finite relation fixture")))
    );
    assert_eq!(assignments[3].dependencies, ["custom", "a"]);
    assert_eq!(assignments[3].value, None);
    assert!(assignments[4].dependencies.is_empty());
    assert_eq!(
        assignments[4].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(1000.0).expect("finite relation fixture")))
    );

    let values = BTreeMap::new();
    let cases = [
        ("cos(60)", 0.5),
        ("tan(45)", 1.0),
        ("asin(1)", 90.0),
        ("acos(0)", 90.0),
        ("atan(1)", 45.0),
        ("atan2(1,0)", 90.0),
        ("sinh(0)", 0.0),
        ("cosh(0)", 1.0),
        ("tanh(0)", 0.0),
        ("sign(-2,-1)", -2.0),
        ("sign(-2,-0)", 2.0),
        ("mod(-5,3)", -2.0),
        ("if(0,2,3)", 3.0),
        ("near(2,2.1,0.2)", 1.0),
        ("min(2,3)+max(2,3)", 5.0),
        ("log(100)", 2.0),
        ("ln(exp(1))", 1.0),
        ("abs(-2)", 2.0),
        ("ceil(2.1)+floor(2.9)", 5.0),
        ("ceil(10.255,2)", 10.26),
        ("ceil(10.255,2.9)", 10.26),
        ("floor(10.255,1)", 10.2),
        ("floor(-10.255,2)", -10.26),
        ("ceil(12.5,-1)", 20.0),
        ("ceil(12.5,-1.9)", 20.0),
        ("floor(12.5,-1)", 10.0),
        ("floor(-12.5,-1)", -20.0),
        ("ceil(10.255,9)", 10.255),
        ("floor(10.255,9)", 10.255),
        ("dbl_in_tol(2,2.1,0.2)", 1.0),
        ("2^3^2", 512.0),
        ("-2^2", -4.0),
        ("(-2)^2", 4.0),
        ("2^-2", 0.25),
        ("2+3*4==14", 1.0),
        ("2>=2 & 3<>4", 1.0),
        ("2<1 | 3~=4", 1.0),
        ("!(2<=3)", 0.0),
        ("~-1", 0.0),
        ("if(2^3==8,5,6)", 5.0),
    ];
    for (expression, expected) in cases {
        let actual = evaluate_expression(expression, &values).expect(expression);
        assert!((actual - expected).abs() < EPS_RELATION_VALUE, "{expression}");
    }
    assert_eq!(evaluate_expression("sqrt(-1)", &values), None);
    assert_eq!(evaluate_expression("tan(90)", &values), None);
    assert_eq!(evaluate_expression("tan(-90)", &values), None);
    assert_eq!(evaluate_expression("atan2(0,0)", &values), None);
    let sinh_86 = evaluate_expression("sinh(86)", &values).expect("finite hyperbolic result");
    assert!(sinh_86.is_finite() && sinh_86 > 1.0e30);
    assert_eq!(evaluate_expression("sinh(1000)", &values), None);
    assert_eq!(evaluate_expression("bound(1,2,1)", &values), None);
    assert_eq!(evaluate_expression("sin()", &values), None);
    assert_eq!(evaluate_expression("1<2<3", &values), None);
    for expression in [
        "1e309==1e309",
        "1e308*1e308>0",
        "if(1,2,1e308*1e308)",
        "0 & (1/0)",
    ] {
        assert_eq!(
            evaluate_expression(expression, &values),
            None,
            "{expression}"
        );
    }
    assert!(evaluate_expression("min(0,-0)", &values)
        .expect("minimum tie")
        .is_sign_negative());
    assert!(evaluate_expression("max(-0,0)", &values)
        .expect("maximum tie")
        .is_sign_positive());
    let Some(CurveExpressionValue::Length(minimum)) =
        parse_relation_expression::<CurveExpressionValue>(
            "min(0[mm],-0[mm])",
            &BTreeMap::new(),
            RelationEvaluationContext::default(),
        )
    else {
        panic!("dimensioned minimum tie")
    };
    assert!(minimum.get().is_sign_negative());
    let maximum = parse_relation_expression::<crate::curve::AffineValue>(
        "max(-0,0)",
        &BTreeMap::new(),
        RelationEvaluationContext::default(),
    )
    .expect("affine maximum tie");
    assert!(maximum.constant.is_sign_positive());
    assert_eq!(maximum.linear, 0.0);
    assert_eq!(relation_round(f64::MAX, 8.0, true), Some(f64::MAX));
    assert_eq!(relation_round(f64::MAX, 8.0, false), Some(f64::MAX));
    assert_eq!(relation_round(-f64::MAX, 8.0, true), Some(-f64::MAX));
    assert_eq!(relation_round(-f64::MAX, 8.0, false), Some(-f64::MAX));
    let tiny_divisor = f64::MIN_POSITIVE * 3.0;
    let expected_remainder = f64::MIN_POSITIVE * 2.0;
    assert_eq!(
        evaluate_creo_math_function(CreoMathFunction::Mod, &[f64::MAX, tiny_divisor]),
        Some(expected_remainder)
    );
    assert_eq!(
        evaluate_creo_math_function(CreoMathFunction::Mod, &[-f64::MAX, tiny_divisor]),
        Some(-expected_remainder)
    );
    assert_eq!(
        evaluate_creo_numeric_relation_function(
            CreoMathFunction::Mod,
            &[
                CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(f64::MAX).expect("finite relation fixture")),
                CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(tiny_divisor).expect("finite relation fixture")),
            ],
        ),
        Some(CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(expected_remainder).expect("finite relation fixture")))
    );
    let excessive_power_depth = format!("{}2", "2^".repeat(129));
    assert!(crate::decode::with_test_decode_ctx(|ctx| crate::curve::parse_relation_expression::<f64>(ctx, &excessive_power_depth, &values, Default::default())).is_err());
    let long_unary_chain = format!("{}1", "-".repeat(1024));
    assert_eq!(evaluate_expression(&long_unary_chain, &values), Some(1.0));
}

#[test]
fn retains_context_function_arguments_without_function_dependencies() {
    let expressions = [
        "a=cable_len(\"c\",start_id,end_id)",
        "b=cable_thick(\"c\",location_id)",
        "c=cbl_logical_file()",
        "d=eang(first_entity,second_entity)",
        "e=elen(first_entity)",
        "f=edistk(first_entity,second_entity)",
        "g=ecoordx(first_entity)",
        "h=ecoordy(first_entity)",
        "i=evalgraph(\"graph\",driver)",
        "j=trajpar_of_pnt(\"trajectory\",\"point\")",
        "k=massprop_param(property_name)",
        "l=material_param(parameter_name,material_name)",
        "m=mp_mass(model_path)",
        "n=mp_assigned_mass(model_path)",
        "o=mp_surf_area(model_path)",
        "p=mp_volume(model_path)",
        "q=mp_cg_x(model_path,coordinate_system,component_path)",
        "r=mp_cg_y(model_path,coordinate_system,component_path)",
        "s=mp_cg_z(model_path,coordinate_system,component_path)",
        "t=has_value(table_parameter,needle,column)",
        "u=match_value(table_parameter,needle,column)",
        "v=average(table_parameter)",
        "w=value_by_argument(table_parameter,argument,interpolation_order)",
        "x=weighted_average(table_parameter)",
        "y=value(table_parameter,row,column)",
        "z=count_rows(table_parameter)",
        "aa=min(table_parameter)",
        "ab=max(table_parameter)",
    ];
    let lines = expressions
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let assignments =
        evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(assignments.len(), expressions.len());
    assert_eq!(assignments[0].dependencies, ["start_id", "end_id"]);
    assert_eq!(assignments[1].dependencies, ["location_id"]);
    assert!(assignments[2].dependencies.is_empty());
    assert_eq!(
        assignments[3].dependencies,
        ["first_entity", "second_entity"]
    );
    assert_eq!(assignments[4].dependencies, ["first_entity"]);
    assert_eq!(
        assignments[5].dependencies,
        ["first_entity", "second_entity"]
    );
    assert_eq!(assignments[6].dependencies, ["first_entity"]);
    assert_eq!(assignments[7].dependencies, ["first_entity"]);
    assert_eq!(assignments[8].dependencies, ["driver"]);
    assert!(assignments[9].dependencies.is_empty());
    assert_eq!(assignments[10].dependencies, ["property_name"]);
    assert_eq!(
        assignments[11].dependencies,
        ["parameter_name", "material_name"]
    );
    for assignment in &assignments[12..16] {
        assert_eq!(assignment.dependencies, ["model_path"]);
    }
    for assignment in &assignments[16..19] {
        assert_eq!(
            assignment.dependencies,
            ["model_path", "coordinate_system", "component_path"]
        );
    }
    for assignment in &assignments[19..21] {
        assert_eq!(
            assignment.dependencies,
            ["table_parameter", "needle", "column"]
        );
    }
    assert_eq!(assignments[21].dependencies, ["table_parameter"]);
    assert_eq!(
        assignments[22].dependencies,
        ["table_parameter", "argument", "interpolation_order"]
    );
    assert_eq!(assignments[23].dependencies, ["table_parameter"]);
    assert_eq!(
        assignments[24].dependencies,
        ["table_parameter", "row", "column"]
    );
    assert_eq!(assignments[25].dependencies, ["table_parameter"]);
    assert_eq!(assignments[26].dependencies, ["table_parameter"]);
    assert_eq!(assignments[27].dependencies, ["table_parameter"]);
    assert!(assignments
        .iter()
        .all(|assignment| assignment.value.is_none()));
}

#[test]
fn retains_scoped_model_name_calls_without_parameter_dependencies() {
    let lines = ["component_name=rel_model_name:27()", "copy=component_name"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let assignments = evaluate_expression_program(
        &lines,
        Some("current_part"),
        &ExternalRelationSymbols::default(),
    );

    assert_eq!(assignments.len(), 2);
    assert!(assignments[0].dependencies.is_empty());
    assert_eq!(assignments[0].value, None);
    assert_eq!(assignments[1].dependencies, ["component_name"]);
    assert_eq!(assignments[1].value, None);
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "rel_model_name:27()",
            &BTreeMap::new(),
            RelationEvaluationContext {
                model_name: Some("current_part"),
                ..RelationEvaluationContext::default()
            },
        ),
        None
    );
}

#[test]
fn evaluates_scoped_symbol_targets_without_declaring_local_parameters() {
    let lines = [
        "d7:0=driver+1",
        "width:fid_25:cid_12=5",
        "copy=d7:0*2",
        "present=exists('width:fid_25:cid_12')",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();
    let mut external_symbols = ExternalRelationSymbols::default();
    observe_relation_symbol(
        &mut external_symbols,
        "driver",
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture"))),
    );

    let assignments = evaluate_expression_program(&lines, None, &external_symbols);

    assert_eq!(assignments.len(), 4);
    assert_eq!(
        assignments[0].target,
        CurveExpressionTarget::ScopedSymbol {
            name: "d7:0".to_owned(),
        }
    );
    assert_eq!(assignments[0].dependencies, ["driver"]);
    assert_eq!(
        assignments[0].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[1].target,
        CurveExpressionTarget::ScopedSymbol {
            name: "width:fid_25:cid_12".to_owned(),
        }
    );
    assert_eq!(assignments[2].dependencies, ["d7:0"]);
    assert_eq!(
        assignments[2].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(6.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[3].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")))
    );
    assert!(assignments[..2]
        .iter()
        .all(|assignment| assignment.parameter_target().is_none()));
}

#[test]
fn classifies_and_evaluates_unscoped_system_symbol_targets() {
    use CurveExpressionSystemSymbolFamily::{
        Dimension, DrivenDimension, KnownDimension, PatternCount, ReferenceDimension,
        SectionDimension, SectionReferenceDimension, Tolerance,
    };

    let targets = [
        ("d7", Dimension),
        ("sd8", SectionDimension),
        ("rd9", ReferenceDimension),
        ("rsd10", SectionReferenceDimension),
        ("kd11", KnownDimension),
        ("Ad12", DrivenDimension),
        ("p13", PatternCount),
        ("tpm14", Tolerance),
        ("tp15", Tolerance),
        ("tm16", Tolerance),
    ];
    let mut sources = targets
        .iter()
        .enumerate()
        .map(|(index, (name, _))| format!("{name}={}", index + 1))
        .collect::<Vec<_>>();
    sources.push("sum=d7+sd8+rd9+rsd10+kd11+Ad12+p13+tpm14+tp15+tm16".to_owned());
    let lines = sources
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine { text, offset })
        .collect::<Vec<_>>();

    let assignments =
        evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(assignments.len(), targets.len() + 1);
    for (assignment, (name, family)) in assignments.iter().zip(targets) {
        assert_eq!(
            assignment.target,
            CurveExpressionTarget::SystemSymbol {
                name: name.to_owned(),
                family,
            }
        );
        assert!(assignment.parameter_target().is_none());
    }
    assert_eq!(
        assignments
            .last()
            .and_then(|assignment| assignment.value.clone()),
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(55.0).expect("finite relation fixture")))
    );
}

#[test]
fn retains_registered_function_write_targets_and_argument_dependencies() {
    let lines = [
        "store_value(component,if(row==1,2,3),column,\"literal\")=driver*2",
        "notify_regeneration()=5",
        "result=driver",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let assignments =
        evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(assignments.len(), 3);
    assert_eq!(
        assignments[0].target,
        CurveExpressionTarget::FunctionWrite {
            name: "store_value".to_owned(),
            arguments: vec![
                "component".to_owned(),
                "if(row==1,2,3)".to_owned(),
                "column".to_owned(),
                "\"literal\"".to_owned(),
            ],
        }
    );
    assert_eq!(
        assignments[0].dependencies,
        ["component", "row", "column", "driver"]
    );
    assert_eq!(
        assignments[1].target,
        CurveExpressionTarget::FunctionWrite {
            name: "notify_regeneration".to_owned(),
            arguments: Vec::new(),
        }
    );
    assert!(assignments[1].dependencies.is_empty());
    assert!(assignments[..2]
        .iter()
        .all(|assignment| assignment.value.is_none()));
    assert_eq!(assignments[2].parameter_target(), Some(("result", None)));
}

#[test]
fn retains_table_cell_targets_without_defining_scalar_parameters() {
    let lines = [
        "value(samples,row_index,column_index)=driver*2",
        "VALUE(series,2)=5",
        "after=value(samples,row_index,column_index)",
        "value(samples,if(row_index==1,2,3),column_index)=driver",
        "value (spaced,row_index)=driver",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let assignments =
        evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(assignments.len(), 5);
    assert_eq!(
        assignments[0].target,
        CurveExpressionTarget::TableCell {
            parameter: "samples".to_owned(),
            row: "row_index".to_owned(),
            column: Some("column_index".to_owned()),
        }
    );
    assert_eq!(
        assignments[0].dependencies,
        ["samples", "row_index", "column_index", "driver"]
    );
    assert_eq!(
        assignments[1].target,
        CurveExpressionTarget::TableCell {
            parameter: "series".to_owned(),
            row: "2".to_owned(),
            column: None,
        }
    );
    assert_eq!(assignments[1].dependencies, ["series"]);
    assert_eq!(assignments[2].parameter_target(), Some(("after", None)));
    assert_eq!(
        assignments[2].dependencies,
        ["samples", "row_index", "column_index"]
    );
    assert_eq!(
        assignments[3].target,
        CurveExpressionTarget::TableCell {
            parameter: "samples".to_owned(),
            row: "if(row_index==1,2,3)".to_owned(),
            column: Some("column_index".to_owned()),
        }
    );
    assert_eq!(
        assignments[3].dependencies,
        ["samples", "row_index", "column_index", "driver"]
    );
    assert_eq!(
        assignments[4].target,
        CurveExpressionTarget::TableCell {
            parameter: "spaced".to_owned(),
            row: "row_index".to_owned(),
            column: None,
        }
    );
    assert!(assignments
        .iter()
        .all(|assignment| assignment.value.is_none()));
}

#[test]
fn evaluates_string_relations_and_ignores_literal_contents_in_dependencies() {
    let sources = [
        "material='steel'",
        "label=material+\"-\"+itos(2.4)",
        "where=search(label,'eel')",
        "piece=extract(label,2,3)",
        "length=string_length(piece)",
        "starts=string_starts(label,'ste')",
        "ends=string_ends(label,'-2')",
        "same=piece=='tee'",
        "matches=string_match(label,'steel-2')",
        "pattern=string_pattern(label,'steel-[0-9]*')",
        "not_pattern=string_pattern(label,'steel-[A-Z]*')",
        "zero=itos(0)",
        "bad=-'text'",
        "bad_pattern=string_pattern(label,'[')",
    ];
    let lines = sources
        .iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: (*text).to_owned(),
            offset,
        })
        .collect::<Vec<_>>();
    let assignments =
        evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());

    assert!(assignments[0].dependencies.is_empty());
    assert_eq!(
        assignments[0].value,
        Some(CurveExpressionValue::String("steel".into()))
    );
    assert_eq!(assignments[1].dependencies, ["material"]);
    assert_eq!(
        assignments[1].value,
        Some(CurveExpressionValue::String("steel-2".into()))
    );
    assert_eq!(
        assignments[2].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[3].value,
        Some(CurveExpressionValue::String("tee".into()))
    );
    assert_eq!(
        assignments[4].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[5].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[6].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[7].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[8].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[9].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[10].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(0.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[11].value,
        Some(CurveExpressionValue::String(String::new()))
    );
    assert_eq!(assignments[12].value, None);
    assert_eq!(assignments[13].value, None);

    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "extract('abc',1e308,1)",
            &BTreeMap::new(),
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::String(String::new()))
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "extract('abc',2,1e308)",
            &BTreeMap::new(),
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::String("bc".into()))
    );
}

#[test]
fn bracketed_relation_units_are_not_dependencies() {
    let lines = [
        CurveExpressionLine {
            text: "length=5[mm]+offset[inch]".to_owned(),
            offset: 0,
        },
        CurveExpressionLine {
            text: "compound=pressure[N/mm^2]".to_owned(),
            offset: 1,
        },
        CurveExpressionLine {
            text: "fall=G*2[s]^2".to_owned(),
            offset: 2,
        },
    ];
    let assignments =
        evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(assignments[0].dependencies, ["offset"]);
    assert_eq!(assignments[0].value, None);
    assert_eq!(assignments[1].dependencies, ["pressure"]);
    assert_eq!(assignments[1].value, None);
    assert!(assignments[2].dependencies.is_empty());
    assert_eq!(
        assignments[2].value,
        Some(CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(39_200.0).expect("finite relation fixture")))
    );

    let values = BTreeMap::new();
    let cases = [
        ("5[mm]+.2[cm]", CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(7.0).expect("finite relation fixture"))),
        ("1[inch]", CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(25.4).expect("finite relation fixture"))),
        ("PI[rad]", CurveExpressionValue::Angle(cadmpeg_ir::scalar::FiniteReal::new(180.0).expect("finite relation fixture"))),
        ("sin(PI[rad]/2)", CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture"))),
        ("1[mm]*2", CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture"))),
        ("1[mm]/.1[cm]", CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture"))),
    ];
    for (expression, expected) in cases {
        let actual = parse_relation_expression::<CurveExpressionValue>(
            expression,
            &values,
            RelationEvaluationContext::default(),
        )
        .expect(expression);
        match (actual, expected) {
            (CurveExpressionValue::Number(actual), CurveExpressionValue::Number(expected))
            | (CurveExpressionValue::Length(actual), CurveExpressionValue::Length(expected))
            | (CurveExpressionValue::Angle(actual), CurveExpressionValue::Angle(expected)) => {
                assert!((actual.get() - expected.get()).abs() < EPS_RELATION_VALUE, "{expression}");
            }
            _ => panic!("unexpected value kind for {expression}"),
        }
    }
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "1[mm]+1[deg]",
            &values,
            RelationEvaluationContext::default(),
        ),
        None
    );

    let pressure = parse_relation_expression::<CurveExpressionValue>(
        "1[N/mm^2]",
        &values,
        RelationEvaluationContext::default(),
    );
    assert_eq!(
        pressure,
        Some(CurveExpressionValue::Quantity(CurveExpressionQuantity::new(1_000.0, [-1, 1, -2, 0, 0]).expect("valid residual dimension fixture")))
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "1[(N/mm^2)]",
            &values,
            RelationEvaluationContext::default(),
        ),
        pressure
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "1[N/mm^2]/1[N/mm^2]",
            &values,
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")))
    );
    for expression in [
        "1[sq_in]/1[in]^2",
        "1[cu_ft]/1[ft]^3",
        "1[joule]/(1[N]*1[m])",
        "1[kW]/(1000[joule]/1[s])",
        "1[MPa]/1[N/mm^2]",
        "1[ton]/(1000[kg]*9.80665[m/s^2])",
    ] {
        let Some(CurveExpressionValue::Number(value)) =
            parse_relation_expression::<CurveExpressionValue>(
                expression,
                &values,
                RelationEvaluationContext::default(),
            )
        else {
            panic!("unexpected value kind for {expression}");
        };
        assert!((value.get() - 1.0).abs() < EPS_RELATION_VALUE, "{expression}");
    }
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "1[psi]/1[Pa]",
            &values,
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(6_894.757_293_168_361).expect("finite relation fixture")))
    );
    for (expression, expected_kelvin) in [
        ("0[C]", 273.15),
        ("32[F]", 273.15),
        ("273.15[K]", 273.15),
        ("491.67[R]", 273.15),
    ] {
        let Some(CurveExpressionValue::Quantity(value)) =
            parse_relation_expression::<CurveExpressionValue>(
                expression,
                &values,
                RelationEvaluationContext::default(),
            )
        else {
            panic!("unexpected value kind for {expression}");
        };
        assert!(
            (value.value() - expected_kelvin).abs() < EPS_RELATION_VALUE,
            "{expression}"
        );
        assert_eq!(value.powers()[4], 1, "{expression}");
        assert_eq!(
            [
                value.powers()[0],
                value.powers()[1],
                value.powers()[2],
                value.powers()[3],
            ],
            [0; 4],
            "{expression}"
        );
    }
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "1[C/s]",
            &values,
            RelationEvaluationContext::default(),
        ),
        None
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "2[mm]^2",
            &values,
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::Quantity(CurveExpressionQuantity::new(4.0, [2, 0, 0, 0, 0]).expect("valid residual dimension fixture")))
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "sqrt(4[mm^2])",
            &values,
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture")))
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "min(abs(-2[cm]),30[mm])",
            &values,
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(20.0).expect("finite relation fixture")))
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "near(1[inch],25[mm],1[mm])",
            &values,
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")))
    );
    let dimensioned_cases = [
        ("if(1,2[cm],1[inch])", CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(20.0).expect("finite relation fixture"))),
        (
            "bound(30[mm],1[cm],2[cm])",
            CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(20.0).expect("finite relation fixture")),
        ),
        (
            "dead(25[mm],1[cm],2[cm])",
            CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(5.0).expect("finite relation fixture")),
        ),
        ("mod(25[mm],1[cm])", CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(5.0).expect("finite relation fixture"))),
        ("sign(2[cm],-1[s])", CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(-20.0).expect("finite relation fixture"))),
        ("ceil(2.1[mm])", CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("finite relation fixture"))),
        ("ceil(12.5[mm],-1)", CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(20.0).expect("finite relation fixture"))),
        ("floor(2.19[cm],1)", CurveExpressionValue::Length(cadmpeg_ir::scalar::FiniteReal::new(21.9).expect("finite relation fixture"))),
        ("atan(1)", CurveExpressionValue::Angle(cadmpeg_ir::scalar::FiniteReal::new(45.0).expect("finite relation fixture"))),
    ];
    for (expression, expected) in dimensioned_cases {
        assert_eq!(
            parse_relation_expression::<CurveExpressionValue>(
                expression,
                &values,
                RelationEvaluationContext::default(),
            ),
            Some(expected),
            "{expression}"
        );
    }
    let Some(CurveExpressionValue::Angle(angle)) = parse_relation_expression::<CurveExpressionValue>(
        "atan2(1[cm],5[mm])",
        &values,
        RelationEvaluationContext::default(),
    ) else {
        panic!("dimensioned atan2 angle");
    };
    assert!((angle.get() - 2.0f64.atan().to_degrees()).abs() < EPS_RELATION_VALUE);
    for incompatible in [
        "if(1,1[mm],1[s])",
        "bound(1[mm],0[s],2[mm])",
        "mod(1[mm],1[s])",
        "atan2(1[mm],1[s])",
    ] {
        assert_eq!(
            parse_relation_expression::<CurveExpressionValue>(
                incompatible,
                &values,
                RelationEvaluationContext::default(),
            ),
            None,
            "{incompatible}"
        );
    }
    let force_ratio = parse_relation_expression::<CurveExpressionValue>(
        "1[lbf]/1[N]",
        &values,
        RelationEvaluationContext::default(),
    );
    let Some(CurveExpressionValue::Number(force_ratio)) = force_ratio else {
        panic!("force ratio");
    };
    assert!((force_ratio.get() - 4.448_221_615_260_5).abs() < EPS_RELATION_VALUE);
    for malformed in ["1[N/mm^]", "1[N//mm]", "1[N^128]"] {
        assert_eq!(
            parse_relation_expression::<CurveExpressionValue>(
                malformed,
                &values,
                RelationEvaluationContext::default(),
            ),
            None,
            "{malformed}"
        );
    }
}

#[test]
fn formats_relation_reals_with_creo_rtos_conventions() {
    let values = BTreeMap::new();
    let cases = [
        ("rtos(123.456789)", "123.456789"),
        ("rtos(123.456789,3)", "123.457"),
        ("rtos(123.456789,4,YES)", "1.2346e02"),
        ("rtos(0)", ""),
        ("rtos(-0,3,YES)", ""),
        ("rtos(0.01234,2,TRUE)", "1.23e-02"),
        ("rtos(25.4[mm],1)", "25.4"),
        ("rtos(0.5[rad],3)", "28.648"),
        ("rtos(2[N],0)", "2000"),
        ("rel_model_type()", "part"),
        ("itos(1.6[mm])", "2"),
        ("itos(1e20)", "100000000000000000000"),
        ("itos(-1e20)", "-100000000000000000000"),
    ];
    for (expression, expected) in cases {
        assert_eq!(
            parse_relation_expression::<CurveExpressionValue>(
                expression,
                &values,
                RelationEvaluationContext::default()
            ),
            Some(CurveExpressionValue::String(expected.to_owned())),
            "{expression}"
        );
    }
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "rtos(1,-1)",
            &values,
            RelationEvaluationContext::default()
        ),
        None
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "rtos(1,1.5)",
            &values,
            RelationEvaluationContext::default()
        ),
        None
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "rtos(1,129)",
            &values,
            RelationEvaluationContext::default()
        ),
        None
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "rtos(1,2,YES,NO)",
            &values,
            RelationEvaluationContext::default()
        ),
        None
    );
    for expression in ["rtos('one')", "rtos(1,2[mm])", "itos('one')"] {
        assert_eq!(
            parse_relation_expression::<CurveExpressionValue>(
                expression,
                &values,
                RelationEvaluationContext::default()
            ),
            None,
            "{expression}"
        );
    }
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "rel_model_name()",
            &values,
            RelationEvaluationContext {
                model_name: Some("widget"),
                ..RelationEvaluationContext::default()
            },
        ),
        Some(CurveExpressionValue::String("widget".to_owned()))
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "rel_model_name()",
            &values,
            RelationEvaluationContext::default(),
        ),
        None
    );
}

#[test]
fn proves_exists_for_local_and_external_relation_symbols() {
    let sources = [
        "IF exists('later')",
        "selected=1",
        "ELSE",
        "selected=2",
        "ENDIF",
        "later=5",
        "IF exists('d42')",
        "dimension=3",
        "ENDIF",
        "IF exists('external')",
        "unknown=1",
        "ENDIF",
    ];
    let lines = sources
        .iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: (*text).to_owned(),
            offset,
        })
        .collect::<Vec<_>>();
    let mut external_symbols = ExternalRelationSymbols::default();
    observe_relation_symbol(&mut external_symbols, "d42", None);
    let assignments = evaluate_expression_program(&lines, None, &external_symbols);

    assert_eq!(assignments.len(), 5);
    assert!(assignments[0].dependencies.is_empty());
    assert_eq!(assignments[0].activation, CurveExpressionActivation::Active);
    assert_eq!(
        assignments[0].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[1].activation,
        CurveExpressionActivation::Inactive
    );
    assert_eq!(assignments[1].value, None);
    assert_eq!(
        assignments[2].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(5.0).expect("finite relation fixture")))
    );
    assert_eq!(assignments[3].activation, CurveExpressionActivation::Active);
    assert_eq!(
        assignments[3].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[4].activation,
        CurveExpressionActivation::Conditional
    );
    assert_eq!(assignments[4].value, None);
}

#[test]
fn reevaluates_expression_records_after_external_symbols_are_decoded() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x03IF exists('d42')\0selected=1\0ENDIF\0";
    let mut records = expression_records(payload);

    assert_eq!(
        records[0].assignments[0].activation,
        CurveExpressionActivation::Conditional
    );
    let mut external_symbols = ExternalRelationSymbols::default();
    observe_relation_symbol(&mut external_symbols, "d42", None);
    crate::decode::with_test_decode_ctx(|ctx| {
        reevaluate_expression_records(ctx, &mut records, None, &external_symbols)
    })
    .expect("test curve expression reevaluation");
    assert_eq!(
        records[0].assignments[0].activation,
        CurveExpressionActivation::Active
    );
    assert_eq!(records[0].assignments[0].value, None);
}

#[test]
fn external_symbol_values_require_agreeing_observations() {
    let lines = [CurveExpressionLine {
        text: "value=d42+1".to_owned(),
        offset: 0,
    }];
    let mut external_symbols = ExternalRelationSymbols::default();
    observe_relation_symbol(
        &mut external_symbols,
        "D42",
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture"))),
    );
    observe_relation_symbol(
        &mut external_symbols,
        "d42",
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture"))),
    );
    assert_eq!(
        evaluate_expression_program(&lines, None, &external_symbols)[0].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("finite relation fixture")))
    );

    observe_relation_symbol(
        &mut external_symbols,
        "d42",
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(4.0).expect("finite relation fixture"))),
    );
    assert_eq!(
        evaluate_expression_program(&lines, None, &external_symbols)[0].value,
        None
    );
}

#[test]
fn binds_relation_symbols_case_insensitively_and_preserves_scoped_dependencies() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x04Radius=5\0q=radius+PI\0\
        external=d1:2+PARAM:FID_20\0RADIUS=7\0";
    let assignments = &expression_records(payload)[0].assignments;

    assert_eq!(assignments[1].dependencies, ["radius"]);
    assert_eq!(
        assignments[1].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(5.0 + std::f64::consts::PI).expect("finite relation fixture")))
    );
    assert_eq!(assignments[2].dependencies, ["d1:2", "PARAM:FID_20"]);
    assert_eq!(assignments[2].value, None);
    assert_eq!(
        assignments[3].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(7.0).expect("finite relation fixture")))
    );
    assert_eq!(
        evaluate_expression("pi", &BTreeMap::new()),
        Some(std::f64::consts::PI)
    );
}

#[test]
fn skipped_solve_conditionals_do_not_underflow_execution_stack() {
    for suffix in ["ENDIF\0x=1\0", "ELSE\0x=1\0ENDIF\0"] {
        let mut payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
            \xe0\x0aexpression\0\xf8"
            .to_vec();
        payload.push(if suffix.starts_with("ELSE") { 6 } else { 5 });
        payload.extend_from_slice(b"SOLVE\0IF YES\0FOR x\0");
        payload.extend_from_slice(suffix.as_bytes());
        let records = expression_records(&payload);
        let assignments = evaluate_expression_program(
            &records[0].lines,
            None,
            &ExternalRelationSymbols::default(),
        );
        assert_eq!(assignments.len(), 1);
        assert_eq!(
            assignments[0].activation,
            CurveExpressionActivation::Conditional
        );
        assert_eq!(assignments[0].value, None);
    }
}

#[test]
fn evaluates_nested_relation_conditionals_in_source_order() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x0eA=0\0IF a==0\0b=5\0IF NO\0c=1\0\
        ELSE\0c=b+1\0ENDIF\0ELSE\0b=10\0ENDIF\0a=5\0d=B\0iffy=9\0";
    let record = &expression_records(payload)[0];
    assert_eq!(record.prohibited_constructs, ["else", "endif", "if"]);
    let assignments =
        evaluate_expression_program(&record.lines, None, &ExternalRelationSymbols::default());

    assert_eq!(assignments.len(), 8);
    assert_eq!(
        assignments[0].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(0.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[1].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(5.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[2].activation,
        CurveExpressionActivation::Inactive
    );
    assert_eq!(assignments[2].value, None);
    assert_eq!(
        assignments[3].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(6.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[4].activation,
        CurveExpressionActivation::Inactive
    );
    assert_eq!(
        assignments[5].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(5.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[6].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(5.0).expect("finite relation fixture")))
    );
    assert_eq!(assignments[7].parameter_target(), Some(("iffy", None)));
    assert_eq!(
        assignments[7].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(9.0).expect("finite relation fixture")))
    );
}

#[test]
fn curve_equations_retain_but_do_not_evaluate_prohibited_constructs() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x05/* search('ignored') */\0a=abs(-2)\0label='ceil(1)'\0b=sqrt(4)\0c=IF(1,2,3)\0";
    let mut records = expression_records(payload);
    let record = &records[0];

    assert_eq!(record.prohibited_constructs, ["abs", "if"]);
    assert!(record
        .assignments
        .iter()
        .all(|assignment| assignment.value.is_none()));
    let mut symbols = ExternalRelationSymbols::default();
    observe_relation_symbol(
        &mut symbols,
        "external",
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(5.0).expect("finite relation fixture"))),
    );
    crate::decode::with_test_decode_ctx(|ctx| {
        reevaluate_expression_records(ctx, &mut records, None, &symbols)
    })
    .expect("test curve expression reevaluation");
    assert!(records[0]
        .assignments
        .iter()
        .all(|assignment| assignment.value.is_none()));
}

fn prohibited_construct_error(
    text: &str,
    max_collection_items: u64,
    max_retained_bytes: u64,
) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    policy.limits.max_retained_bytes = max_retained_bytes;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    curve_equation_prohibited_constructs(
        &ctx,
        &[CurveExpressionLine {
            text: text.to_owned(),
            offset: 0,
        }],
    )
    .expect_err("one prohibited construct exceeds limit")
}

#[test]
fn prohibited_relation_keyword_refuses_node() {
    let error = prohibited_construct_error("IF a", 0, 32);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo prohibited construct nodes"));
}

#[test]
fn prohibited_function_refuses_node() {
    let error = prohibited_construct_error("r=AbS(1)", 0, 32);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo prohibited construct nodes"));
}

#[test]
fn prohibited_function_refuses_retained_name() {
    let error = prohibited_construct_error("r=AbS(1)", 2, 0);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo prohibited construct names"));
}

#[test]
fn prohibited_relation_keyword_refuses_output_vector() {
    let error = prohibited_construct_error("IF a", 1, 32);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo prohibited construct records"));
}

#[test]
fn unresolved_and_malformed_conditionals_do_not_choose_a_branch() {
    let unresolved = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x06IF external\0x=1\0ELSE\0x=2\0ENDIF\0y=x+1\0";
    let record = &expression_records(unresolved)[0];
    let assignments =
        evaluate_expression_program(&record.lines, None, &ExternalRelationSymbols::default());
    assert_eq!(assignments.len(), 3);
    assert!(assignments[..2]
        .iter()
        .all(|assignment| assignment.activation == CurveExpressionActivation::Conditional));
    assert_eq!(assignments[2].activation, CurveExpressionActivation::Active);
    assert_eq!(assignments[2].value, None);

    let malformed = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x08\
        \xe0\x0aexpression\0\xf8\x04IF YES\0x=1\0ELSE trailing\0ENDIF\0";
    let record = &expression_records(malformed)[0];
    let assignments =
        evaluate_expression_program(&record.lines, None, &ExternalRelationSymbols::default());
    assert_eq!(
        assignments[0].activation,
        CurveExpressionActivation::Conditional
    );
    assert_eq!(assignments[0].value, None);

    let overflow = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x06IF 1e308*1e308>0\0x=1\0ELSE\0x=2\0ENDIF\0y=x+1\0";
    let record = &expression_records(overflow)[0];
    let assignments =
        evaluate_expression_program(&record.lines, None, &ExternalRelationSymbols::default());
    assert!(assignments[..2]
        .iter()
        .all(|assignment| assignment.activation == CurveExpressionActivation::Conditional));
    assert_eq!(assignments[2].activation, CurveExpressionActivation::Active);
    assert_eq!(assignments[2].value, None);
}

#[test]
fn unresolved_reassignment_invalidates_the_previous_scalar_value() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x04a=5\0b=a+1\0a=external\0c=a+1\0";
    let records = expression_records(payload);
    let assignments = &records[0].assignments;

    assert_eq!(
        assignments[0].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(5.0).expect("finite relation fixture")))
    );
    assert_eq!(
        assignments[1].value,
        Some(CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::new(6.0).expect("finite relation fixture")))
    );
    assert_eq!(assignments[2].value, None);
    assert_eq!(assignments[3].value, None);
}
