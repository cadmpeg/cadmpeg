// SPDX-License-Identifier: Apache-2.0
//! Formula expression evaluation tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn decode_evaluates_formula_precedence_and_parentheses() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 12.0)],
                "LENGTH",
                Some(30.0),
                "(#1_ /2+3mm)*2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode parenthesized formula");

    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("validated formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(30.0)
        ))
    );
}

#[test]
fn decode_rejects_a_constant_formula_that_disagrees_with_its_stored_result() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(13.0),
                "10mm+2mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode mismatched constant formula");

    assert!(decoded.ir().model.parameters.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT),
        0
    );
}

#[test]
fn decode_converts_degree_literals_to_radians() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "Integer", "Count", "#1_ /2", 4.0)],
                "ANGLE",
                Some(std::f64::consts::FRAC_PI_2),
                "360.0*1 deg/#1_ /2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode degree formula");

    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("degree formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2)
        ))
    );
}

#[test]
fn decode_evaluates_the_dimensionless_pi_constant_in_an_angle_expression() {
    let output_value = std::f64::consts::PI - 1.0;
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "ANGLE", "Angle", "#1_ /2", 1.0)],
                "ANGLE",
                Some(output_value),
                "PI*1rad-#1_ /2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode formula with PI");

    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("PI formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(output_value)
        ))
    );
}

#[test]
fn decode_evaluates_dimensionless_trigonometric_arguments_as_radians() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(0.0),
                "sin(0)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode scalar-radian trigonometric formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("scalar-radian trigonometric formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(0.0))
    );
}

#[test]
fn decode_evaluates_dimension_checked_trigonometric_calls() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[(
                    "#1_",
                    "ANGLE",
                    "Sweep",
                    "#1_ /2",
                    std::f64::consts::FRAC_PI_2,
                )],
                "Real",
                Some(1.0),
                "sin(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode trigonometric formula");

    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("trigonometric formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(1.0))
    );
}

#[test]
fn decode_evaluates_nested_logarithm_and_extrema_calls() {
    let output_value = -(4.0_f64.log10()) / 100.0_f64.log10() / 2.0;
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "Real", "Gain", "#1_ /2", 2.0),
                    ("#2_", "Real", "Reference", "#2_ /3", 10.0),
                ],
                "Real",
                Some(output_value),
                "-log(min(100,max(20*#1_ /2,#2_ /3)/#2_ /3))/log(100)/2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode logarithmic formula");

    let [first, second, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("logarithmic formula parameters")
    };
    assert_eq!(output.dependencies, [first.id.clone(), second.id.clone()]);
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(output_value))
    );
}

#[test]
fn decode_distinguishes_common_and_natural_logarithms() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(3.0),
                "log(100)+ln(E)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode logarithm formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("logarithm formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(3.0))
    );
}

#[test]
fn decode_normalizes_every_admitted_formula_length_unit_to_millimetres() {
    let expected = 0.001 + 1_609_344.0 + 914.4 + 1.0 + 10.0 + 1_000_000.0 + 304.8 + 25.4 + 1_000.0;
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(expected),
                "1micron+1mile+1yard+1mm+1cm+1km+1ft+1in+1m",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode complete length-unit formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("length-unit formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(expected)
        ))
    );
}

#[test]
fn decode_normalizes_every_admitted_formula_angle_unit_to_radians() {
    let expected = 1.0 + std::f64::consts::PI / 200.0 + std::f64::consts::PI / 180.0;
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "ANGLE",
                Some(expected),
                "1rad+1grad+1deg",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode complete angle-unit formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("angle-unit formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(expected)
        ))
    );
}

#[test]
fn decode_evaluates_exponential_and_hyperbolic_functions() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(2.0),
                "exp(0)+sinh(0)+cosh(0)+tanh(0)+asinh(0)+acosh(1)+atanh(0)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode exponential and hyperbolic formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("exponential and hyperbolic formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(2.0))
    );
}

#[test]
fn decode_evaluates_scalar_rounding_functions() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(8.0),
                "ceil(1.2)+floor(1.8)+int(-1.8)+round(2.5)+round(3.5)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode scalar rounding formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("scalar rounding formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(8.0))
    );
}

#[test]
fn decode_evaluates_dimensioned_rounding_in_the_selected_unit() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(1_230.0),
                "round(1234mm,\"cm\",0)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensioned rounding formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("dimensioned rounding formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(1_230.0)
        ))
    );
}

#[test]
fn decode_evaluates_integer_part_as_an_integer_result() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Integer",
                Some(-1.0),
                "int(-1.8)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode integer-part formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("integer-part formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Integer(-1))
    );
}

#[test]
fn decode_evaluates_variadic_extrema_and_integer_part_remainder() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(9.0),
                "min(8,5,7,3)+max(1,4,2)+mod(7.8,3)+max(1)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode variadic extrema and remainder formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("variadic extrema and remainder formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(9.0))
    );
}

#[test]
fn decode_evaluates_remainder_of_a_negative_real_dividend_integer_part() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(-1.0),
                "mod(-7.5,3)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode negative real remainder formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("negative real remainder formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(-1.0))
    );
}

#[test]
fn decode_evaluates_a_square_root_of_a_dimensioned_product() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "LENGTH", "Width", "#1_ /2", 3.0),
                    ("#2_", "LENGTH", "Height", "#2_ /3", 4.0),
                ],
                "LENGTH",
                Some(5.0),
                "sqrt(#1_ /2*#1_ /2+#2_ /3*#2_ /3)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensioned square root");

    let [first, second, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("square-root formula parameters")
    };
    assert_eq!(output.dependencies, [first.id.clone(), second.id.clone()]);
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(5.0)
        ))
    );
}

#[test]
fn decode_evaluates_right_associative_exponentiation_above_unary_signs() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(-512.0),
                "-2**3**2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode exponent formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("exponent formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(-512.0))
    );
}

#[test]
fn decode_evaluates_an_integral_power_of_a_dimensioned_value() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 3.0)],
                "LENGTH",
                Some(3.0),
                "sqrt((#1_ /2)**2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensioned exponent formula");

    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("dimensioned exponent formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(3.0)
        ))
    );
}

#[test]
fn decode_evaluates_inverse_trigonometric_calls_as_angles() {
    let output_value = 0.5_f64.asin() + 0.5_f64.acos() + 1.0_f64.atan();
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "ANGLE",
                Some(output_value),
                "asin(0.5)+acos(0.5)+atan(1)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode inverse trigonometric formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("inverse trigonometric formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(output_value)
        ))
    );
}

#[test]
fn decode_evaluates_dimension_safe_absolute_and_tangent_calls() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "LENGTH", "Offset", "#1_ /2", -2.0),
                    ("#2_", "ANGLE", "Slope", "#2_ /3", 0.0),
                ],
                "LENGTH",
                Some(2.0),
                "abs(#1_ /2)*(1+tan(#2_ /3))",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode absolute and tangent formula");

    let [first, second, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("absolute and tangent formula parameters")
    };
    assert_eq!(output.dependencies, [first.id.clone(), second.id.clone()]);
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(2.0)
        ))
    );
}

#[test]
fn decode_rejects_a_square_root_with_an_odd_dimension_exponent() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "AreaLike", "#1_ /2", 4.0)],
                "LENGTH",
                Some(2.0),
                "sqrt(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid square root");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "AreaLike");
}

#[test]
fn decode_rejects_a_fractional_power_of_a_dimensioned_value() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 4.0)],
                "LENGTH",
                Some(2.0),
                "(#1_ /2)**0.5",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid exponent formula");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
}

#[test]
fn decode_rejects_dimension_exponent_overflow() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 1.0)],
                "LENGTH",
                Some(1.0),
                "((#1_ /2)**2147483647)**2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode exponent-overflow formula");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
}

#[test]
fn decode_rejects_inverse_trigonometry_outside_its_scalar_domain() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 1.0)],
                "ANGLE",
                Some(std::f64::consts::FRAC_PI_4),
                "atan(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid inverse trigonometric formula");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
}

#[test]
fn decode_rejects_inverse_trigonometry_outside_its_numeric_domain() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "ANGLE",
                Some(0.0),
                "asin(2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode out-of-domain inverse trigonometric formula");

    assert!(decoded.ir().model.parameters.is_empty());
}

#[test]
fn decode_rejects_scalar_functions_with_dimensioned_arguments() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 1.0)],
                "Real",
                Some(1.0),
                "exp(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid exponential formula");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
}

#[test]
fn decode_rejects_invalid_inverse_hyperbolic_domains() {
    for expression in ["acosh(0.5)", "atanh(1)"] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                    3,
                    false,
                    &[],
                    "Real",
                    Some(0.0),
                    expression,
                )),
                &DecodeOptions::default(),
            )
            .expect("decode out-of-domain inverse hyperbolic formula");

        assert!(decoded.ir().model.parameters.is_empty(), "{expression}");
    }
}

#[test]
fn decode_rejects_nonfinite_exponential_results() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(0.0),
                "exp(1000)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode overflowing exponential formula");

    assert!(decoded.ir().model.parameters.is_empty());
}

#[test]
fn decode_rejects_invalid_remainder_divisors() {
    for expression in ["mod(7,0)", "mod(7,2.5)", "mod(7,1mm)"] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                    3,
                    false,
                    &[],
                    "Real",
                    Some(0.0),
                    expression,
                )),
                &DecodeOptions::default(),
            )
            .expect("decode invalid remainder formula");

        assert!(decoded.ir().model.parameters.is_empty(), "{expression}");
    }
}

#[test]
fn decode_rejects_a_logarithm_outside_its_dimensionless_positive_domain() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "Real", "Ratio", "#1_ /2", 0.0)],
                "Real",
                Some(0.0),
                "log(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode out-of-domain logarithm");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Ratio");
}

#[test]
fn decode_rejects_a_dimensioned_cubic_interpolation_fraction() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "Real", "Start", "#1_ /2", 2.0),
                    ("#2_", "Real", "End", "#2_ /3", 10.0),
                    ("#3_", "LENGTH", "Fraction", "#3_ /4", 0.25),
                ],
                "Real",
                Some(3.25),
                "CubicInterpolation(#1_ /2,#2_ /3,#3_ /4)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid cubic interpolation");

    assert_eq!(decoded.ir().model.parameters.len(), 3);
}

#[test]
fn decode_converts_metric_length_literals_to_millimetres() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(1_023.0),
                "1m+2cm+3mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode metric length formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("metric length formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(1_023.0)
        ))
    );
}

#[test]
fn decode_rejects_mixed_dimension_linear_interpolation_endpoints() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "LENGTH", "Start", "#1_ /2", 2.0),
                    ("#2_", "Real", "End", "#2_ /3", 10.0),
                    ("#3_", "Real", "Fraction", "#3_ /4", 0.25),
                ],
                "Real",
                Some(4.0),
                "LinearInterpolation(#1_ /2,#2_ /3,#3_ /4)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid linear interpolation");

    assert_eq!(decoded.ir().model.parameters.len(), 3);
}

#[test]
fn decode_rejects_extrema_between_different_dimensions() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "LENGTH", "Length", "#1_ /2", 2.0),
                    ("#2_", "ANGLE", "Angle", "#2_ /3", 1.0),
                ],
                "LENGTH",
                Some(2.0),
                "max(#1_ /2,#2_ /3)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid maximum");

    let [first, second] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed inputs")
    };
    assert_eq!(first.name, "Length");
    assert_eq!(second.name, "Angle");
}

#[test]
fn decode_rejects_trigonometric_calls_with_length_arguments() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Offset", "#1_ /2", 0.0)],
                "Real",
                Some(0.0),
                "sin(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid trigonometric formula");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Offset");
    assert!(input.dependencies.is_empty());
}

#[test]
fn decode_rejects_dimensionally_invalid_formula_output() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 12.0)],
                "LENGTH",
                Some(12.0),
                "#1_ /2+1rad",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid formula");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
    assert!(input.dependencies.is_empty());
}

#[test]
fn decode_rejects_a_conditional_with_different_branch_dimensions() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(5.0),
                "true ? 5mm ; 1rad",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid conditional formula");

    assert!(decoded.ir().model.parameters.is_empty());
}
