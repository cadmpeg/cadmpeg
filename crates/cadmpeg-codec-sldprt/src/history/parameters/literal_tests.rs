//! Parameter literal, unit and expression-evaluation tests.

use super::super::literals::{
    dimension_display, format_f64_literal, parse_length_mm, parse_parameter_literal,
};
use super::super::write::parameters::rewrite_parameter_expression;
use super::eval;
use super::eval::{
    compare_parameter_values, exact_integer_f64, exponentiate_parameter_value,
    ParameterExpressionParser,
};
use super::{bare_text_parameter_literal, formatted_text_dimension_literal};
use cadmpeg_ir::features::{DimensionDisplay, ParameterValue};

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
        let literal = format_f64_literal(real(value));
        let parsed = literal.parse::<f64>().expect("required invariant");
        assert_eq!(parsed.to_bits(), value.to_bits(), "{literal}");
    }
    assert_eq!(format_f64_literal(real(0.125)), "0.125");
    assert_eq!(
        format_f64_literal(real(7.745_183_829_698_638e-127)),
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
        parse_parameter_literal(
            &cadmpeg_test_support::service_decode_context(),
            "&lt;MOD-DIAM&gt;4.917"
        )
        .unwrap(),
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
        parse_parameter_literal(
            &cadmpeg_test_support::service_decode_context(),
            "<MOD-RHO>0.5"
        )
        .unwrap(),
        Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(0.5).unwrap()
        ))
    );
    assert_eq!(
        dimension_display(
            &cadmpeg_test_support::service_decode_context(),
            "&lt;MOD-RHO&gt;0.5"
        )
        .unwrap(),
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
            parse_parameter_literal(&cadmpeg_test_support::service_decode_context(), expression)
                .unwrap(),
            Some(ParameterValue::Length(
                cadmpeg_ir::scalar::Length::new(expected).unwrap()
            )),
            "{expression}"
        );
        assert_eq!(
            dimension_display(&cadmpeg_test_support::service_decode_context(), expression).unwrap(),
            Some(display),
            "{expression}"
        );
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
    assert_eq!(
        parse_parameter_literal(&cadmpeg_test_support::service_decode_context(), "x2").unwrap(),
        None
    );
    assert_eq!(
        parse_parameter_literal(&cadmpeg_test_support::service_decode_context(), "15mmH7").unwrap(),
        None
    );
    assert_eq!(
        parse_parameter_literal(
            &cadmpeg_test_support::service_decode_context(),
            "<MOD-DIAM>15H"
        )
        .unwrap(),
        None
    );
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
            eval::ParameterFunction::Sgn.apply(vec![ParameterValue::Integer(argument)]),
            Some(ParameterValue::Integer(expected))
        );
    }
}

#[test]
fn integer_function_preserves_discrete_integer_values() {
    for value in [i64::MIN, -(1_i64 << 53) - 1, (1_i64 << 53) + 1, i64::MAX] {
        assert_eq!(
            eval::ParameterFunction::Int.apply(vec![ParameterValue::Integer(value)]),
            Some(ParameterValue::Integer(value))
        );
    }
    assert_eq!(
        eval::ParameterFunction::Int.apply(vec![ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(-3.75).unwrap()
        )]),
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
        assert_eq!(
            parse_parameter_literal(&cadmpeg_test_support::service_decode_context(), literal)
                .unwrap(),
            None,
            "{literal}"
        );
    }
}

#[test]
fn bare_binary_digits_are_integer_parameters() {
    assert_eq!(
        parse_parameter_literal(&cadmpeg_test_support::service_decode_context(), "0").unwrap(),
        Some(ParameterValue::Integer(0))
    );
    assert_eq!(
        parse_parameter_literal(&cadmpeg_test_support::service_decode_context(), "1").unwrap(),
        Some(ParameterValue::Integer(1))
    );
    assert_eq!(
        parse_parameter_literal(&cadmpeg_test_support::service_decode_context(), "true").unwrap(),
        Some(ParameterValue::Boolean(true))
    );
    assert_eq!(
        parse_parameter_literal(&cadmpeg_test_support::service_decode_context(), "false").unwrap(),
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
        compare_parameter_values(&integer, &rounded_real, "="),
        Some(false)
    );
    assert_eq!(
        compare_parameter_values(&integer, &rounded_real, ">"),
        Some(true)
    );
    assert_eq!(
        compare_parameter_values(&rounded_real, &integer, "<"),
        Some(true)
    );

    assert_eq!(
        compare_parameter_values(
            &ParameterValue::Integer(-3),
            &ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::new(-3.5).unwrap()),
            ">",
        ),
        Some(true)
    );
    assert_eq!(
        compare_parameter_values(
            &ParameterValue::Integer(i64::MAX),
            &ParameterValue::Real(
                cadmpeg_ir::scalar::FiniteReal::new(
                    -cadmpeg_core::convert::f64_from_i64(i64::MIN).unwrap()
                )
                .unwrap()
            ),
            "<",
        ),
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
fn dimension_count_refusal_does_not_become_missing_literal() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = parse_parameter_literal(&ctx, "2X<MOD-DIAM>1.2").unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal");
    };
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
