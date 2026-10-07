//! Parameter literal, unit and expression-evaluation tests.

use super::super::literals::{
    dimension_display, format_parameter_value, parse_length_mm, parse_parameter_literal,
};
use super::super::write::parameters::rewrite_parameter_expression;
use super::eval;
use super::eval::{
    compare_parameter_values, exact_integer_f64, exponentiate_parameter_value,
    ParameterExpressionParser,
};
use super::{bare_text_parameter_literal, formatted_text_dimension_literal};
use cadmpeg_ir::features::{DimensionDisplay, ParameterValue};
use std::borrow::Cow;

/// A finite test value as the formatter's checked input.
fn real(value: f64) -> cadmpeg_ir::scalar::FiniteReal {
    cadmpeg_ir::scalar::FiniteReal::new(value).expect("a finite test value")
}

#[test]
fn native_scalar_literals_are_compact_and_bit_exact() {
    for value in [
        0.0,
        -0.0,
        0.125,
        -42.5,
        7.745_183_829_698_638e-127,
        -5.486_124_068_793_69e307,
    ] {
        let literal = format_parameter_value(&ParameterValue::Real(real(value)));
        let parsed = literal.parse::<f64>().expect("required invariant");
        assert_eq!(parsed.to_bits(), value.to_bits(), "{literal}");
    }
    assert_eq!(
        format_parameter_value(&ParameterValue::Real(real(0.125))),
        "0.125"
    );
    assert_eq!(
        format_parameter_value(&ParameterValue::Real(real(7.745_183_829_698_638e-127))),
        "7.745183829698638e-127"
    );
}

#[test]
fn solidworks_length_units_convert_to_millimeters() {
    for (literal, expected) in [
        ("1A", 1.0e-7),
        ("1Å", 1.0e-7),
        ("1nm", 1.0e-6),
        ("1um", 1.0e-3),
        ("1µm", 1.0e-3),
        ("1μm", 1.0e-3),
        ("1mm", 1.0),
        ("1cm", 10.0),
        ("1m", 1000.0),
        ("1uin", 25.4e-6),
        ("1mil", 0.0254),
        ("1in", 25.4),
        ("1ft", 304.8),
    ] {
        assert_eq!(
            parse_length_mm(literal).map(cadmpeg_ir::scalar::Length::get),
            Some(expected),
            "{literal}"
        );
    }
}

#[test]
fn diameter_display_literals_participate_in_expressions() {
    let aliases = std::collections::HashMap::new();
    let values = std::collections::HashMap::new();
    assert_eq!(
        ParameterExpressionParser::new_flat(
            &cadmpeg_test_support::service_decode_context(),
            "<MOD-DIAM>4mm / 2",
            &aliases,
            &values
        )
        .parse()
        .unwrap(),
        Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(2.0).unwrap()
        ))
    );
    assert_eq!(
        ParameterExpressionParser::new_flat(
            &cadmpeg_test_support::service_decode_context(),
            "<MOD-DIAM>4 + 1mm",
            &aliases,
            &values
        )
        .parse()
        .unwrap(),
        Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(5.0).unwrap()
        ))
    );
    assert_eq!(
        ParameterExpressionParser::new_flat(
            &cadmpeg_test_support::service_decode_context(),
            "&lt;MOD-DIAM&gt;4mm / 2",
            &aliases,
            &values,
        )
        .parse()
        .unwrap(),
        Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(2.0).unwrap()
        ))
    );
    assert_eq!(
        parse_parameter_literal("&lt;MOD-DIAM&gt;4.917"),
        Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(4.917).unwrap()
        ))
    );
}

#[test]
fn radius_display_literals_participate_in_expressions() {
    let aliases = std::collections::HashMap::new();
    let values = std::collections::HashMap::new();
    assert_eq!(
        ParameterExpressionParser::new_flat(
            &cadmpeg_test_support::service_decode_context(),
            "<MOD-RHO>4mm / 2",
            &aliases,
            &values
        )
        .parse()
        .unwrap(),
        Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(2.0).unwrap()
        ))
    );
    assert_eq!(
        ParameterExpressionParser::new_flat(
            &cadmpeg_test_support::service_decode_context(),
            "&lt;MOD-RHO&gt;4 + 1mm",
            &aliases,
            &values,
        )
        .parse()
        .unwrap(),
        Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(5.0).unwrap()
        ))
    );
    assert_eq!(
        parse_parameter_literal("<MOD-RHO>0.5"),
        Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(0.5).unwrap()
        ))
    );
    assert_eq!(
        dimension_display("&lt;MOD-RHO&gt;0.5"),
        Some(DimensionDisplay::Radius)
    );
}

#[test]
fn dimension_decorations_preserve_the_nominal_scalar() {
    let aliases = std::collections::HashMap::new();
    let values = std::collections::HashMap::new();
    for (expression, expected, display) in [
        ("2X<MOD-DIAM>1.2", 1.2, DimensionDisplay::Diameter),
        ("6XR2", 2.0, DimensionDisplay::Radius),
        ("<MOD-DIAM>15H7", 15.0, DimensionDisplay::Diameter),
        ("3x &lt;MOD-RHO&gt;0.5", 0.5, DimensionDisplay::Radius),
    ] {
        assert_eq!(
            parse_parameter_literal(expression),
            Some(ParameterValue::Length(
                cadmpeg_ir::scalar::Length::new(expected).unwrap()
            )),
            "{expression}"
        );
        assert_eq!(dimension_display(expression), Some(display), "{expression}");
        assert_eq!(
            ParameterExpressionParser::new_flat(
                &cadmpeg_test_support::service_decode_context(),
                expression,
                &aliases,
                &values
            )
            .parse()
            .unwrap(),
            Some(ParameterValue::Length(
                cadmpeg_ir::scalar::Length::new(expected).unwrap()
            )),
            "{expression}"
        );
    }
    assert_eq!(parse_parameter_literal("x2"), None);
    assert_eq!(parse_parameter_literal("15mmH7"), None);
    assert_eq!(parse_parameter_literal("<MOD-DIAM>15H"), None);
}

#[test]
fn bare_native_text_is_distinct_from_scalar_expressions_and_references() {
    for text in ["M16x2.0", "740四件等高", "plain text"] {
        assert_eq!(
            bare_text_parameter_literal(&cadmpeg_test_support::service_decode_context(), text)
                .unwrap(),
            Some(ParameterValue::String(text.into()))
        );
    }
    for expression in ["", "1 +", "width/2", "\"D1@Sketch1\"", "D12"] {
        assert_eq!(
            bare_text_parameter_literal(
                &cadmpeg_test_support::service_decode_context(),
                expression
            )
            .unwrap(),
            None,
            "{expression}"
        );
    }
}

#[test]
fn formatted_text_dimensions_are_strings_only_for_txd_parameters() {
    let text = "4X <MOD-DIAM> 12 <HOLE-DEPTH> 40<MOD-PM>.2";
    for (name, text) in [
        ("TXD5", text),
        ("TXD2", "30X <MOD-DIAM> 14<HOLE-SINK><MOD-DIAM> 20 X 90°"),
        ("TXD3", "<BORDER><MOD-DIAM>10 </BORDER>"),
        ("TXD7", "4X M12x1.75 <HOLE-DEPTH> 25<MOD-PM>.25"),
    ] {
        assert_eq!(
            formatted_text_dimension_literal(
                &cadmpeg_test_support::service_decode_context(),
                name,
                text
            )
            .unwrap(),
            Some(ParameterValue::String(text.into())),
            "{name}"
        );
    }
    for name in ["D5", "TXD", "TXD5-extra"] {
        assert_eq!(
            formatted_text_dimension_literal(
                &cadmpeg_test_support::service_decode_context(),
                name,
                text
            )
            .unwrap(),
            None,
            "{name}"
        );
    }
    for malformed in ["1 +", "<MOD-DIAM", "MOD-DIAM>", "<>", "< >", "<<TAG>"] {
        assert_eq!(
            formatted_text_dimension_literal(
                &cadmpeg_test_support::service_decode_context(),
                "TXD5",
                malformed
            )
            .unwrap(),
            None,
            "{malformed}"
        );
    }
}

#[test]
fn solidworks_sign_function_is_three_way() {
    for (argument, expected) in [(-2, -1), (0, 0), (2, 1)] {
        assert_eq!(
            eval::ParameterFunction::Sgn
                .apply(vec![Cow::Owned(ParameterValue::Integer(argument))])
                .map(Cow::into_owned),
            Some(ParameterValue::Integer(expected))
        );
    }
}

#[test]
fn integer_function_preserves_discrete_integer_values() {
    for value in [i64::MIN, -(1_i64 << 53) - 1, (1_i64 << 53) + 1, i64::MAX] {
        assert_eq!(
            eval::ParameterFunction::Int
                .apply(vec![Cow::Owned(ParameterValue::Integer(value))])
                .map(Cow::into_owned),
            Some(ParameterValue::Integer(value))
        );
    }
    assert_eq!(
        eval::ParameterFunction::Int
            .apply(vec![Cow::Owned(ParameterValue::Real(
                cadmpeg_ir::scalar::FiniteReal::new(-3.75).unwrap()
            ))])
            .map(Cow::into_owned),
        Some(ParameterValue::Integer(-3))
    );
}

#[test]
fn integer_powers_preserve_exact_exponent_parity() {
    let odd = ParameterValue::Integer((1_i64 << 53) + 1);
    assert_eq!(
        exponentiate_parameter_value(&ParameterValue::Integer(-1), &odd),
        Some(ParameterValue::Integer(-1))
    );
    assert_eq!(
        exponentiate_parameter_value(
            &ParameterValue::Integer(-1),
            &ParameterValue::Integer(-((1_i64 << 53) + 1)),
        ),
        Some(ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(-1.0).unwrap()
        ))
    );
    assert_eq!(
        exponentiate_parameter_value(&ParameterValue::Integer(2), &ParameterValue::Integer(-3),),
        Some(ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(0.125).unwrap()
        ))
    );
}

#[test]
fn non_finite_parameter_literals_have_no_evaluated_value() {
    for literal in ["NaN", "inf", "-inf"] {
        assert_eq!(parse_parameter_literal(literal), None, "{literal}");
    }
}

#[test]
fn bare_binary_digits_are_integer_parameters() {
    assert_eq!(
        parse_parameter_literal("0"),
        Some(ParameterValue::Integer(0))
    );
    assert_eq!(
        parse_parameter_literal("1"),
        Some(ParameterValue::Integer(1))
    );
    assert_eq!(
        parse_parameter_literal("true"),
        Some(ParameterValue::Boolean(true))
    );
    assert_eq!(
        parse_parameter_literal("false"),
        Some(ParameterValue::Boolean(false))
    );
}

#[test]
fn native_scalars_accept_only_exact_integer_values() {
    let largest_consecutive = 1_i64 << 53;
    assert_eq!(exact_integer_f64(largest_consecutive), Some(2_f64.powi(53)));
    assert_eq!(exact_integer_f64(largest_consecutive + 1), None);
    assert_eq!(
        exact_integer_f64(i64::MIN),
        Some(cadmpeg_core::convert::f64_from_i64(i64::MIN).unwrap())
    );
    assert_eq!(exact_integer_f64(i64::MAX), None);
}

#[test]
fn mixed_numeric_comparisons_preserve_integer_identity() {
    let integer = ParameterValue::Integer((1_i64 << 53) + 1);
    let rounded_real =
        ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::new(2_f64.powi(53)).unwrap());
    assert_eq!(
        compare_parameter_values(
            &cadmpeg_test_support::service_decode_context(),
            &integer,
            &rounded_real,
            "="
        )
        .unwrap(),
        Some(false)
    );
    assert_eq!(
        compare_parameter_values(
            &cadmpeg_test_support::service_decode_context(),
            &integer,
            &rounded_real,
            ">"
        )
        .unwrap(),
        Some(true)
    );
    assert_eq!(
        compare_parameter_values(
            &cadmpeg_test_support::service_decode_context(),
            &rounded_real,
            &integer,
            "<"
        )
        .unwrap(),
        Some(true)
    );

    assert_eq!(
        compare_parameter_values(
            &cadmpeg_test_support::service_decode_context(),
            &ParameterValue::Integer(-3),
            &ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::new(-3.5).unwrap()),
            ">",
        )
        .unwrap(),
        Some(true)
    );
    assert_eq!(
        compare_parameter_values(
            &cadmpeg_test_support::service_decode_context(),
            &ParameterValue::Integer(i64::MAX),
            &ParameterValue::Real(
                cadmpeg_ir::scalar::FiniteReal::new(
                    -cadmpeg_core::convert::f64_from_i64(i64::MIN).unwrap()
                )
                .unwrap()
            ),
            "<",
        )
        .unwrap(),
        Some(true)
    );
}

#[test]
fn expression_rewrite_quotes_hyphenated_identifiers() {
    let aliases = std::collections::HashMap::from([("Width".into(), "Wall-Gauge".into())]);
    assert_eq!(
        rewrite_parameter_expression(
            &cadmpeg_test_support::service_decode_context(),
            "Width * 2",
            &aliases
        )
        .unwrap()
        .as_deref(),
        Some("\"Wall-Gauge\" * 2")
    );
}

#[test]
fn integer_without_exact_real_value_has_no_real_arithmetic_result() {
    let inexact = ParameterValue::Integer((1_i64 << 53) + 1);
    let half = ParameterValue::Real(real(0.5));
    assert_eq!(exponentiate_parameter_value(&inexact, &half), None);
    assert_eq!(
        exponentiate_parameter_value(&inexact, &ParameterValue::Integer(-1)),
        None
    );
    let exact = ParameterValue::Integer(1_i64 << 53);
    assert!(exponentiate_parameter_value(&exact, &half).is_some());
    assert_eq!(
        exponentiate_parameter_value(&ParameterValue::Integer(2), &ParameterValue::Integer(-1)),
        Some(ParameterValue::Real(real(0.5)))
    );
}

#[test]
fn parameter_text_comparison_admits_both_operands() {
    let left = ParameterValue::String("abcdefgh".repeat(256));
    let right = left.clone();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        compare_parameter_values(&ctx, &left, &right, "=").unwrap(),
        Some(true)
    );
    let error = crate::test_support::work_refusal_at("compare SLDPRT parameter text", |ctx| {
        compare_parameter_values(ctx, &left, &right, "=")
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "compare SLDPRT parameter text")
    );
}

#[test]
fn parameter_identifier_storage_is_scoped_and_quotes_match() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 64 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (tokens, storage) =
        super::expression_identifier_tokens(&ctx, "Width + \"D1@a\" + \"a\"\"b\"").unwrap();
    let tokens = tokens.unwrap();
    assert_eq!(
        tokens
            .iter()
            .map(super::ExpressionIdentifier::value)
            .collect::<Vec<_>>(),
        ["Width", "D1@a", "a\"b"]
    );
    drop((tokens, storage));
    ctx.reserve_scoped(
        policy.limits.max_materialized_bytes,
        "released token storage",
    )
    .unwrap();
}

#[test]
fn xml_name_rejection_leaves_unvisited_suffix_unpaid() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let name = format!("-{}", "a".repeat(4096));
    assert!(!crate::history::write::xml::valid_xml_name(&ctx, &name).unwrap());
    assert!(ctx.resource_refusal().is_none());
}

fn string_operands() -> (
    std::collections::HashMap<String, Option<cadmpeg_ir::features::ParameterId>>,
    std::collections::HashMap<cadmpeg_ir::features::ParameterId, ParameterValue>,
) {
    let text = cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#text").unwrap();
    let other = cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#other").unwrap();
    (
        std::collections::HashMap::from([
            ("Text".into(), Some(text.clone())),
            ("Other".into(), Some(other.clone())),
        ]),
        std::collections::HashMap::from([
            (text, ParameterValue::String("a".repeat(4096))),
            (other, ParameterValue::String("b".repeat(8192))),
        ]),
    )
}

#[test]
fn failed_string_expression_does_not_retain_operands() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (aliases, values) = string_operands();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        ParameterExpressionParser::new_flat(&ctx, "Text + 1", &aliases, &values)
            .parse()
            .unwrap(),
        None
    );
    assert_eq!(
        ParameterExpressionParser::new_flat(&ctx, "Iif(true,Text,Other) + 1", &aliases, &values)
            .parse()
            .unwrap(),
        None
    );
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn string_comparison_retains_only_boolean_result() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (aliases, values) = string_operands();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        ParameterExpressionParser::new_flat(&ctx, "Text = Text", &aliases, &values)
            .parse()
            .unwrap(),
        Some(ParameterValue::Boolean(true))
    );
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn conditional_string_retains_only_selected_value() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (aliases, values) = string_operands();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        ParameterExpressionParser::new_flat(&ctx, "Iif(true,Text,Other)", &aliases, &values)
            .parse()
            .unwrap(),
        Some(ParameterValue::String("a".repeat(4096)))
    );
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn evaluation_value_index_borrows_large_stated_text() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId};
    let parameter = |name: &str, expression: &str, value| DesignParameter {
        id: ParameterId::mint(format!("synthetic:test:parameter#{name}")).unwrap(),
        owner: None,
        ordinal: 0,
        name: name.into(),
        expression: expression.into(),
        display: None,
        value,
        dependencies: DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let mut parameters = [
        parameter(
            "Text",
            "stated",
            Some(ParameterValue::String("a".repeat(128 * 1024))),
        ),
        parameter("Comparison", "Text = Text", None),
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 64 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::evaluate_parameter_expressions(
        &ctx,
        &mut parameters,
        &std::collections::HashMap::new(),
        &std::collections::HashSet::new(),
    )
    .unwrap();
    assert_eq!(
        parameters[0].value,
        Some(ParameterValue::String("a".repeat(128 * 1024)))
    );
    assert_eq!(parameters[1].value, Some(ParameterValue::Boolean(true)));
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn validation_views_borrow_large_values_and_apply_configuration_overrides() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId};
    let parameter = |name: &str, expression: &str, value| DesignParameter {
        id: ParameterId::mint(format!("synthetic:test:parameter#{name}")).unwrap(),
        owner: None, ordinal: 0, name: name.into(), expression: expression.into(),
        display: None, value, dependencies: DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(), pmi: None, native_ref: None,
    };
    let text = parameter("Text", "stated", Some(ParameterValue::String("a".repeat(128 * 1024))));
    let mut comparison = parameter("Comparison", "Text = Text", Some(ParameterValue::Boolean(true)));
    comparison.dependencies = DistinctMembers::try_from(vec![text.id.clone()], &cadmpeg_test_support::service_decode_context()).unwrap();
    let parameters = [text, comparison];
    let mut configuration = crate::history::tests::design_configuration("changed", 0, None, None);
    configuration.parameter_values.insert(parameters[0].id.clone(), ParameterValue::String("b".repeat(128 * 1024)));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 64 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (aliases, _storage) = super::ParameterAliases::scoped(&ctx, &parameters, &std::collections::HashMap::new(), &std::collections::HashSet::new()).unwrap();
    assert_eq!(super::parameters_with_unevaluable_expressions(&ctx, &parameters, &aliases, std::slice::from_ref(&configuration)).unwrap(), 0);
    assert_eq!(super::parameters_with_incoherent_evaluated_values(&ctx, &parameters, &aliases, std::slice::from_ref(&configuration)).unwrap(), 0);
    configuration.parameter_values.insert(parameters[1].id.clone(), ParameterValue::Boolean(false));
    assert_eq!(super::parameters_with_incoherent_evaluated_values(&ctx, &parameters, &aliases, std::slice::from_ref(&configuration)).unwrap(), 1);
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn validation_borrows_a_large_referenced_result() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId};
    let parameter = |name: &str, expression: &str| DesignParameter {
        id: ParameterId::mint(format!("synthetic:test:parameter#{name}")).unwrap(),
        owner: None, ordinal: 0, name: name.into(), expression: expression.into(),
        display: None, value: Some(ParameterValue::String("a".repeat(128 * 1024))),
        dependencies: DistinctMembers::default(), properties: std::collections::BTreeMap::new(),
        pmi: None, native_ref: None,
    };
    let text = parameter("Text", "stated");
    let mut alias = parameter("Alias", "Text");
    alias.dependencies = DistinctMembers::try_from(vec![text.id.clone()], &cadmpeg_test_support::service_decode_context()).unwrap();
    let parameters = [text, alias];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 64 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (aliases, _storage) = super::ParameterAliases::scoped(&ctx, &parameters, &std::collections::HashMap::new(), &std::collections::HashSet::new()).unwrap();
    assert_eq!(super::parameters_with_unevaluable_expressions(&ctx, &parameters, &aliases, &[]).unwrap(), 0);
    assert!(ctx.resource_refusal().is_none());
}
