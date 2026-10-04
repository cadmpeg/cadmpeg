// SPDX-License-Identifier: Apache-2.0
//! Scalar sweep-law values, derivatives and admitted text projections.

use super::depth::ModelEvaluationDepthGuard;
use super::{admission, decode, EvaluationFailure};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::LawExpression;
use crate::math::sum::{ExactSignedSum, ScaledValue};
use crate::math::Vector3;
use crate::scalar::{FiniteReal, NonZeroReal, PositiveReal};
use cadmpeg_core::decode::u64_from_index;

/// A scalar law's finite value with its derivative, which states its own
/// outcome: a value can exist where its derivative has no value, or where
/// its derivative left the finite range.
#[derive(Clone, Copy)]
pub(super) struct ScalarSweepDifferential {
    pub(super) value: FiniteReal,
    pub(super) derivative: Result<FiniteReal, EvaluationFailure<()>>,
}

/// A law value or derivative computed from finite operands: one outside the
/// finite range leaves the law there.
pub(super) fn law_real(value: f64) -> Result<FiniteReal, EvaluationFailure<()>> {
    FiniteReal::new(value).ok_or(EvaluationFailure::NonFinite(()))
}

/// A constant law whose value is finite by its type.
fn constant_sweep_differential(value: FiniteReal) -> ScalarSweepDifferential {
    ScalarSweepDifferential {
        value,
        derivative: Ok(FiniteReal::ZERO),
    }
}

/// The derivatives of two operands, or the failure of the first that has
/// none.
fn operand_derivatives(
    first: ScalarSweepDifferential,
    second: ScalarSweepDifferential,
) -> Result<(FiniteReal, FiniteReal), EvaluationFailure<()>> {
    Ok((first.derivative?, second.derivative?))
}

/// Remove Unicode whitespace while holding the spelling in scoped scratch.
pub(super) fn compact_sweep_text(
    scratch: &decode::Scratch<'_, '_>,
    text: &str,
) -> Result<String, EvaluationFailure<()>> {
    let mut characters = text.chars();
    let mut bytes = Vec::new();
    while !characters.as_str().is_empty() {
        scratch
            .work(1, "IR sweep law text scan")
            .ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
        let Some(character) = characters.next() else {
            break;
        };
        if character.is_whitespace() {
            continue;
        }
        scratch
            .reserve(
                &mut bytes,
                character.len_utf8(),
                "IR sweep law text storage",
            )
            .ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
        let mut encoded = [0; 4];
        for byte in character.encode_utf8(&mut encoded).bytes() {
            scratch
                .work(1, "IR sweep law text copy")
                .ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
            bytes.push(byte);
        }
    }
    scratch
        .work(bytes.len(), "IR sweep law UTF-8 validation")
        .ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
    String::from_utf8(bytes).map_err(|_| EvaluationFailure::NoValue)
}

/// Admit each edge character before testing whether it is whitespace.
fn trim_sweep_text<'text>(
    admission: admission::EvaluationAdmission<'_, '_>,
    mut text: &'text str,
) -> Result<&'text str, EvaluationFailure<()>> {
    while !text.is_empty() {
        admission
            .work(1, "IR sweep law text whitespace")
            .map_err(EvaluationFailure::ResourceLimit)?;
        let Some(character) = text.chars().next() else {
            break;
        };
        if !character.is_whitespace() {
            break;
        }
        text = &text[character.len_utf8()..];
    }
    while !text.is_empty() {
        admission
            .work(1, "IR sweep law text whitespace")
            .map_err(EvaluationFailure::ResourceLimit)?;
        let Some(character) = text.chars().next_back() else {
            break;
        };
        if !character.is_whitespace() {
            break;
        }
        text = &text[..text.len() - character.len_utf8()];
    }
    Ok(text)
}

/// Admit delimiter visits and stop at the first matching byte.
fn split_sweep_text<'text>(
    admission: admission::EvaluationAdmission<'_, '_>,
    text: &'text str,
    delimiter: u8,
) -> Result<Option<(&'text str, &'text str)>, EvaluationFailure<()>> {
    for (index, byte) in text.as_bytes().iter().enumerate() {
        admission
            .work(1, "IR sweep law field scan")
            .map_err(EvaluationFailure::ResourceLimit)?;
        if *byte == delimiter {
            return Ok(Some((&text[..index], &text[index + 1..])));
        }
    }
    Ok(None)
}

/// Admit the numeric spelling before the standard floating-point parser reads it.
fn parse_sweep_number(
    admission: admission::EvaluationAdmission<'_, '_>,
    text: &str,
) -> Result<Option<f64>, EvaluationFailure<()>> {
    admission
        .work(u64_from_index(text.len()), "IR sweep law number parse")
        .map_err(EvaluationFailure::ResourceLimit)?;
    Ok(text.parse().ok())
}

/// Read exactly the fixed number of comma-delimited formula coordinates.
pub(super) fn sweep_number_fields<const N: usize>(
    admission: admission::EvaluationAdmission<'_, '_>,
    text: &str,
) -> Result<Option<[f64; N]>, EvaluationFailure<()>> {
    let mut remaining = Some(text);
    let mut values = [0.0; N];
    for value in &mut values {
        let Some(text) = remaining.take() else {
            return Ok(None);
        };
        let field = match split_sweep_text(admission, text, b',')? {
            Some((field, tail)) => {
                remaining = Some(tail);
                field
            }
            None => text,
        };
        let Some(number) = parse_sweep_number(admission, field)? else {
            return Ok(None);
        };
        *value = number;
    }
    Ok(remaining.is_none().then_some(values))
}

/// A scalar sweep law's value and derivative at `parameter`, or why it has
/// no value.
///
/// A form the evaluator does not read, a text that is not a finite number or
/// a finite multiple of `X`, an operand outside its operator's domain and a
/// division by zero state no value; a value that overflows leaves the law
/// outside the finite range. The derivative states its own outcome in the
/// same terms.
pub(super) fn scalar_sweep_law_differential(
    admission: admission::EvaluationAdmission<'_, '_>,
    expression: &LawExpression<FiniteReal, FiniteVector3, FinitePoint3>,
    parameter: FiniteReal,
) -> Result<ScalarSweepDifferential, EvaluationFailure<()>> {
    admission.within_model(|admission| {
        let _depth = ModelEvaluationDepthGuard::enter(admission.work_slice())
            .map_err(EvaluationFailure::ResourceLimit)?;
        admission.model_step()?;
        let no_value = EvaluationFailure::NoValue;
        match expression {
            LawExpression::Null {} => Ok(constant_sweep_differential(FiniteReal::ZERO)),
            LawExpression::Integer { value } => Ok(constant_sweep_differential(
                FiniteReal::from_integer(*value).ok_or(no_value)?,
            )),
            LawExpression::Double { value } => Ok(constant_sweep_differential(*value)),
            LawExpression::Text { value } => {
                let value = trim_sweep_text(admission, value.as_str())?;
                if value == "X" {
                    return Ok(ScalarSweepDifferential {
                        value: parameter,
                        derivative: Ok(FiniteReal::ONE),
                    });
                }
                // A text constant that is not finite states no value.
                if let Some(constant) = parse_sweep_number(admission, value)? {
                    return FiniteReal::new(constant)
                        .map(constant_sweep_differential)
                        .ok_or(no_value);
                }
                let (left, right) = split_sweep_text(admission, value, b'*')?.ok_or(no_value)?;
                let coefficient = if trim_sweep_text(admission, right)? == "X" {
                    parse_sweep_number(admission, trim_sweep_text(admission, left)?)?
                } else if trim_sweep_text(admission, left)? == "X" {
                    parse_sweep_number(admission, trim_sweep_text(admission, right)?)?
                } else {
                    return Err(no_value);
                };
                let coefficient = coefficient.and_then(FiniteReal::new).ok_or(no_value)?;
                Ok(ScalarSweepDifferential {
                    value: law_real(coefficient.get() * parameter.get())?,
                    derivative: Ok(coefficient),
                })
            }
            LawExpression::Algebraic { operator, operands } => {
                if let [operand] = operands.as_slice() {
                    let operand = scalar_sweep_law_differential(admission, operand, parameter)?;
                    return scalar_unary_sweep_law_differential(operator, operand);
                }
                if operator == "O" {
                    let [outer, inner] = operands.as_slice() else {
                        return Err(no_value);
                    };
                    let inner = scalar_sweep_law_differential(admission, inner, parameter)?;
                    let outer = scalar_sweep_law_differential(admission, outer, inner.value)?;
                    return Ok(ScalarSweepDifferential {
                        value: outer.value,
                        derivative: operand_derivatives(outer, inner)
                            .and_then(|(outer, inner)| law_real(outer.get() * inner.get())),
                    });
                }
                let [left, right] = operands.as_slice() else {
                    return Err(no_value);
                };
                let left = scalar_sweep_law_differential(admission, left, parameter)?;
                let right = scalar_sweep_law_differential(admission, right, parameter)?;
                let (x, y) = (left.value.get(), right.value.get());
                let derivatives = operand_derivatives(left, right);
                match operator.as_str() {
                    "ADD" => Ok(ScalarSweepDifferential {
                        value: law_real(x + y)?,
                        derivative: derivatives.and_then(|(dx, dy)| law_real(dx.get() + dy.get())),
                    }),
                    "SUB" => Ok(ScalarSweepDifferential {
                        value: law_real(x - y)?,
                        derivative: derivatives.and_then(|(dx, dy)| law_real(dx.get() - dy.get())),
                    }),
                    "MUL" => Ok(ScalarSweepDifferential {
                        value: law_real(x * y)?,
                        derivative: derivatives
                            .and_then(|(dx, dy)| law_real(dx.get() * y + x * dy.get())),
                    }),
                    "DIV" => {
                        let divisor = NonZeroReal::new(y).ok_or(no_value)?;
                        Ok(ScalarSweepDifferential {
                            value: law_real(x / y)?,
                            derivative: derivatives.and_then(|(dx, dy)| {
                                let mut numerator = ExactSignedSum::default();
                                numerator.add_product(dx.get(), y);
                                numerator.add_product(-x, dy.get());
                                numerator.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                                    sweep_quotient(
                                        value,
                                        crate::math::sum::ScaledValue::product_of_nonzero([
                                            divisor, divisor,
                                        ])
                                        .ok_or(EvaluationFailure::NoValue)?,
                                    )
                                })
                            }),
                        })
                    }
                    _ => Err(no_value),
                }
            }
            LawExpression::Point { .. }
            | LawExpression::Vector { .. }
            | LawExpression::Transform { .. }
            | LawExpression::TransformVec { .. }
            | LawExpression::Edge { .. }
            | LawExpression::Spline { .. } => Err(no_value),
        }
    })
}

/// `numerator / denominator` for a law derivative. A quotient that overflows
/// leaves the derivative outside the finite range.
fn sweep_quotient(
    numerator: crate::math::sum::ScaledValue,
    denominator: crate::math::sum::ScaledValue,
) -> Result<FiniteReal, EvaluationFailure<()>> {
    numerator
        .quotient(denominator)
        .map_err(|_| EvaluationFailure::NonFinite(()))
}

/// The derivative `factor * derivative` of a unary law whose operand has
/// derivative `derivative`, or the first failure among the two.
fn chain_derivative(
    factor: Result<f64, EvaluationFailure<()>>,
    derivative: Result<FiniteReal, EvaluationFailure<()>>,
) -> Result<FiniteReal, EvaluationFailure<()>> {
    let derivative = derivative?;
    law_real(factor? * derivative.get())
}

/// A unary law operator applied to its operand's value and derivative. An
/// operand outside the domain of the operator's value, and an operator the
/// evaluator does not read, state no value; a value that overflows leaves
/// the law outside the finite range. The derivative states its own outcome:
/// an operand outside the domain of the operator's derivative states no
/// derivative, and a derivative that overflows left the finite range.
pub(super) fn scalar_unary_sweep_law_differential(
    operator: &str,
    operand: ScalarSweepDifferential,
) -> Result<ScalarSweepDifferential, EvaluationFailure<()>> {
    let no_value = EvaluationFailure::NoValue;
    let non_finite = EvaluationFailure::NonFinite(());
    let x = operand.value.get();
    let law = |value: f64,
               derivative: Result<FiniteReal, EvaluationFailure<()>>|
     -> Result<ScalarSweepDifferential, EvaluationFailure<()>> {
        Ok(ScalarSweepDifferential {
            value: law_real(value)?,
            derivative,
        })
    };
    match operator {
        "LN" => {
            if x <= 0.0 {
                return Err(no_value);
            }
            return law(
                x.ln(),
                operand
                    .derivative
                    .and_then(|derivative| law_real(derivative.get() / x)),
            );
        }
        "EXP" => {
            let value = x.exp();
            let half = (0.5 * x).exp();
            return law(
                value,
                operand.derivative.and_then(|derivative| {
                    let mut product = ExactSignedSum::default();
                    product.add_factors([half, half, derivative.get()]);
                    product.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                        value.finite().map_err(|_| non_finite)
                    })
                }),
            );
        }
        "COT" | "CSC" | "TAN" | "SEC" | "ARCSECH" => {
            let (value, factor, denominator) = match operator {
                "COT" | "CSC" => {
                    let sine = NonZeroReal::new(x.sin()).ok_or(no_value)?;
                    let denominator =
                        crate::math::sum::ScaledValue::product_of_nonzero([sine, sine]);
                    if operator == "COT" {
                        (1.0 / x.tan(), -1.0, denominator.ok_or(no_value))
                    } else {
                        (1.0 / sine.get(), -x.cos(), denominator.ok_or(no_value))
                    }
                }
                "TAN" | "SEC" => {
                    let cosine = NonZeroReal::new(x.cos()).ok_or(no_value)?;
                    let denominator =
                        crate::math::sum::ScaledValue::product_of_nonzero([cosine, cosine]);
                    if operator == "TAN" {
                        (x.tan(), 1.0, denominator.ok_or(no_value))
                    } else {
                        (1.0 / cosine.get(), x.sin(), denominator.ok_or(no_value))
                    }
                }
                _ => {
                    // The value is defined on (0, 1]; at 1 the root is zero
                    // and the derivative has no value.
                    let positive = PositiveReal::new(x).ok_or(no_value)?;
                    if x > 1.0 {
                        return Err(no_value);
                    }
                    let root = positive.unit_complement_root();
                    (
                        (1.0 + root.map_or(0.0, PositiveReal::get)).ln() - x.ln(),
                        -1.0,
                        root.and_then(|root| {
                            crate::math::sum::ScaledValue::product_of_nonzero([
                                positive.into(),
                                root.into(),
                            ])
                        })
                        .ok_or(no_value),
                    )
                }
            };
            return law(
                value,
                operand.derivative.and_then(|derivative| {
                    let denominator = denominator?;
                    let mut numerator = ExactSignedSum::default();
                    numerator.add_product(factor, derivative.get());
                    numerator.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                        sweep_quotient(value, denominator)
                    })
                }),
            );
        }
        "ARCTAN" | "ARCOT" | "ARCSEC" | "ARCCSC" | "ARCCSCH" => {
            let (value, sign, denominator) = match operator {
                "ARCTAN" | "ARCOT" => {
                    let hypotenuse = operand.value.hypot_one_nonzero();
                    let denominator = ScaledValue::product_of_nonzero([hypotenuse, hypotenuse]);
                    if operator == "ARCTAN" {
                        (x.atan(), 1.0, denominator)
                    } else {
                        (std::f64::consts::FRAC_PI_2 - x.atan(), -1.0, denominator)
                    }
                }
                "ARCSEC" | "ARCCSC" => {
                    if x.abs() < 1.0 {
                        return Err(no_value);
                    }
                    let denominator = operand.value.beyond_unit().and_then(|beyond| {
                        let magnitude = beyond.magnitude();
                        let factor = beyond.arcsec_factor();
                        ScaledValue::product_of_nonzero([magnitude, magnitude, factor])
                    });
                    if operator == "ARCSEC" {
                        ((1.0 / x).acos(), 1.0, denominator)
                    } else {
                        ((1.0 / x).asin(), -1.0, denominator)
                    }
                }
                _ => {
                    let magnitude = NonZeroReal::new(x).ok_or(no_value)?.magnitude();
                    let hypotenuse = operand.value.hypot_one_nonzero();
                    let denominator = ScaledValue::product_of_nonzero([magnitude, hypotenuse]);
                    let inverse = 1.0 / x;
                    let value = if inverse.is_finite() {
                        inverse.asinh()
                    } else {
                        (std::f64::consts::LN_2 - x.abs().ln()).copysign(x)
                    };
                    (value, -1.0, denominator)
                }
            };
            return law(
                value,
                operand.derivative.and_then(|derivative| {
                    let denominator = denominator.ok_or(no_value)?;
                    // The operand derivative is finite, so it has a scaled
                    // form exactly when it is not zero.
                    match crate::math::sum::scaled_finite(derivative.get()) {
                        Some(numerator) => {
                            let quotient = sweep_quotient(numerator, denominator)?;
                            Ok(if sign < 0.0 {
                                quotient.negated()
                            } else {
                                quotient
                            })
                        }
                        None => Ok(FiniteReal::ZERO),
                    }
                }),
            );
        }
        "COTH" | "SECH" | "CSCH" => {
            let (tail, unit_sum) = operand.value.hyperbolic_tail_unit_sum();
            let (value, numerator_factors, denominator) = match operator {
                "COTH" => {
                    let sinh = NonZeroReal::new(x)
                        .ok_or(no_value)?
                        .hyperbolic_sinh_denominator();
                    (
                        1.0 / x.tanh(),
                        [-4.0, tail, tail],
                        ScaledValue::product_of_nonzero([sinh, sinh]),
                    )
                }
                "SECH" => {
                    let half_tail = (-0.5 * x.abs()).exp();
                    (
                        2.0 * tail / (1.0 + tail * tail),
                        [-2.0 * x.tanh(), half_tail, half_tail],
                        Some(ScaledValue::of_nonzero(unit_sum)),
                    )
                }
                _ => {
                    let half_tail = (-0.5 * x.abs()).exp();
                    let sinh = NonZeroReal::new(x)
                        .ok_or(no_value)?
                        .hyperbolic_sinh_denominator();
                    (
                        (2.0 * tail / sinh.get()).copysign(x),
                        [-2.0 * (1.0 + tail * tail), half_tail, half_tail],
                        ScaledValue::product_of_nonzero([sinh, sinh]),
                    )
                }
            };
            return law(
                value,
                operand.derivative.and_then(|derivative| {
                    let [first, second, third] = numerator_factors;
                    let denominator = denominator.ok_or(no_value)?;
                    let mut numerator = ExactSignedSum::default();
                    numerator.add_factors([first, second, third, derivative.get()]);
                    match numerator.finish() {
                        Some(value) => sweep_quotient(value, denominator),
                        None => Ok(FiniteReal::ZERO),
                    }
                }),
            );
        }
        "TANH" => {
            let (tail, denominator) = operand.value.hyperbolic_tail();
            return law(
                x.tanh(),
                operand.derivative.and_then(|derivative| {
                    let mut numerator = ExactSignedSum::default();
                    numerator.add_factors([4.0, tail, tail, derivative.get()]);
                    match numerator.finish() {
                        Some(value) => sweep_quotient(
                            value,
                            crate::math::sum::ScaledValue::of_nonzero(denominator),
                        ),
                        None => Ok(FiniteReal::ZERO),
                    }
                }),
            );
        }
        "ARCSINH" => {
            return law(
                x.asinh(),
                operand
                    .derivative
                    .and_then(|derivative| law_real(derivative.get() / x.hypot(1.0))),
            )
        }
        "ARCCOSH" => {
            if x < 1.0 {
                return Err(no_value);
            }
            return law(
                x.acosh(),
                operand.derivative.and_then(|derivative| {
                    if x == 1.0 {
                        return Err(no_value);
                    }
                    let denominator = if x < 2.0 {
                        ((x - 1.0) * (x + 1.0)).sqrt()
                    } else {
                        x * (1.0 - (1.0 / x).powi(2)).sqrt()
                    };
                    law_real(derivative.get() / denominator)
                }),
            );
        }
        "ARCOTH" => {
            let beyond = operand.value.beyond_unit().ok_or(no_value)?;
            return Ok(ScalarSweepDifferential {
                value: beyond.arcoth(),
                derivative: operand.derivative.and_then(|derivative| {
                    // (derivative / x) * (-1 / x) over 1 - 1/x², formed as one
                    // exact product and one quotient.
                    let mut numerator = ExactSignedSum::default();
                    numerator.add_product(
                        beyond.quotient(derivative).get(),
                        beyond.quotient(FiniteReal::ONE).negated().get(),
                    );
                    numerator.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                        sweep_quotient(
                            value,
                            crate::math::sum::ScaledValue::of_nonzero(beyond.square_complement()),
                        )
                    })
                }),
            });
        }
        _ => {}
    }

    let (value, factor) = match operator {
        "SIN" => (x.sin(), Ok(x.cos())),
        "COS" => (x.cos(), Ok(-x.sin())),
        "COSH" => (x.cosh(), Ok(x.sinh())),
        "SINH" => (x.sinh(), Ok(x.cosh())),
        "ARCCOS" | "ARCSIN" => {
            if x.abs() > 1.0 {
                return Err(no_value);
            }
            let denominator = (1.0 - x * x).sqrt();
            let factor = (denominator > 0.0)
                .then_some(1.0 / denominator)
                .ok_or(no_value);
            if operator == "ARCCOS" {
                (x.acos(), factor.map(|factor| -factor))
            } else {
                (x.asin(), factor)
            }
        }
        "ARCTANH" => {
            if x.abs() >= 1.0 {
                return Err(no_value);
            }
            (x.atanh(), Ok(1.0 / (1.0 - x * x)))
        }
        "ABS" => (
            x.abs(),
            if x > 0.0 {
                Ok(1.0)
            } else if x < 0.0 {
                Ok(-1.0)
            } else {
                Err(no_value)
            },
        ),
        "SIGN" => {
            if x == 0.0 {
                return Err(no_value);
            }
            (x.signum(), Ok(0.0))
        }
        "SQRT" => {
            if x < 0.0 {
                return Err(no_value);
            }
            (x.sqrt(), (x > 0.0).then(|| 0.5 / x.sqrt()).ok_or(no_value))
        }
        _ => return Err(no_value),
    };
    law(value, chain_derivative(factor, operand.derivative))
}

pub(super) fn sweep_scale(
    admission: admission::EvaluationAdmission<'_, '_>,
    expression: &LawExpression<FiniteReal, FiniteVector3, FinitePoint3>,
) -> Result<Option<Vector3>, EvaluationFailure<()>> {
    let scratch = decode::Scratch::new(admission);
    let result = (|| {
        Ok(match expression {
            LawExpression::Null {} => Some(Vector3::new(1.0, 1.0, 1.0)),
            LawExpression::Text { value } => {
                let value = compact_sweep_text(&scratch, value.as_str())?;
                let Some(values) = value
                    .strip_prefix("VEC(")
                    .and_then(|value| value.strip_suffix(')'))
                else {
                    return Ok(None);
                };
                sweep_number_fields::<3>(admission, values)?.map(|[x, y, z]| Vector3::new(x, y, z))
            }
            LawExpression::Vector { value } => Some(value.get()),
            _ => None,
        })
    })();
    scratch.settle(result)
}
