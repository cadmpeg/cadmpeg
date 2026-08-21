// SPDX-License-Identifier: Apache-2.0
//! Parameter-expression parser and arithmetic.

use cadmpeg_ir::features::{Angle, Length, ParameterId, ParameterValue};
use std::collections::HashMap;

use super::ParameterAliasView;
use crate::history::literals::parse_parameter_literal;

pub(crate) struct ParameterExpressionParser<'a> {
    input: &'a str,
    offset: usize,
    aliases: ParameterAliasMap<'a>,
    values: &'a HashMap<ParameterId, ParameterValue>,
}

pub(crate) enum ParameterAliasMap<'a> {
    Layered(ParameterAliasView<'a>),
    #[cfg(test)]
    Flat(&'a HashMap<String, Option<ParameterId>>),
}

impl ParameterAliasMap<'_> {
    fn get(&self, alias: &str) -> Option<&Option<ParameterId>> {
        match self {
            Self::Layered(aliases) => aliases.get(alias),
            #[cfg(test)]
            Self::Flat(aliases) => aliases.get(alias),
        }
    }
}

impl<'a> ParameterExpressionParser<'a> {
    pub(crate) fn new(
        input: &'a str,
        aliases: ParameterAliasView<'a>,
        values: &'a HashMap<ParameterId, ParameterValue>,
    ) -> Self {
        Self {
            input,
            offset: 0,
            aliases: ParameterAliasMap::Layered(aliases),
            values,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_flat(
        input: &'a str,
        aliases: &'a HashMap<String, Option<ParameterId>>,
        values: &'a HashMap<ParameterId, ParameterValue>,
    ) -> Self {
        Self {
            input,
            offset: 0,
            aliases: ParameterAliasMap::Flat(aliases),
            values,
        }
    }

    pub(crate) fn parse(mut self) -> Option<ParameterValue> {
        self.skip_space();
        self.take('=');
        self.skip_space();
        if let Some(value) = parse_parameter_literal(&self.input[self.offset..]) {
            return Some(value);
        }
        let value = self.comparison()?;
        self.skip_space();
        (self.offset == self.input.len()).then_some(value)
    }

    fn comparison(&mut self) -> Option<ParameterValue> {
        let left = self.sum()?;
        self.skip_space();
        let operator = ["<=", ">=", "<>", "=", "<", ">"]
            .into_iter()
            .find(|operator| self.input[self.offset..].starts_with(operator));
        let Some(operator) = operator else {
            return Some(left);
        };
        self.offset += operator.len();
        compare_parameter_values(&left, &self.sum()?, operator).map(ParameterValue::Boolean)
    }

    fn sum(&mut self) -> Option<ParameterValue> {
        let mut value = self.product()?;
        loop {
            self.skip_space();
            let op = self.take_one(&['+', '-']);
            let Some(op) = op else { return Some(value) };
            value = add_parameter_values(value, self.product()?, op == '-')?;
        }
    }

    fn product(&mut self) -> Option<ParameterValue> {
        let mut value = self.unary()?;
        loop {
            self.skip_space();
            let op = self.take_one(&['*', '/']);
            let Some(op) = op else { return Some(value) };
            value = multiply_parameter_values(value, self.unary()?, op == '/')?;
        }
    }

    fn unary(&mut self) -> Option<ParameterValue> {
        self.skip_space();
        if self.take('-') {
            negate_parameter_value(&self.unary()?)
        } else if self.take('+') {
            self.unary()
        } else {
            self.power()
        }
    }

    fn power(&mut self) -> Option<ParameterValue> {
        let base = self.primary()?;
        self.skip_space();
        if self.take('^') {
            exponentiate_parameter_value(&base, &self.unary()?)
        } else {
            Some(base)
        }
    }

    fn primary(&mut self) -> Option<ParameterValue> {
        self.skip_space();
        if self.take('(') {
            let value = self.comparison()?;
            self.skip_space();
            return self.take(')').then_some(value);
        }
        let (token, quoted) = self.token()?;
        if !quoted {
            self.skip_space();
            if self.take('(') {
                if token.eq_ignore_ascii_case("iif") {
                    let condition = self.comparison()?;
                    self.skip_space();
                    if !self.take(',') {
                        return None;
                    }
                    let when_true = self.comparison()?;
                    self.skip_space();
                    if !self.take(',') {
                        return None;
                    }
                    let when_false = self.comparison()?;
                    self.skip_space();
                    if !self.take(')') {
                        return None;
                    }
                    return conditional_parameter_value(&condition, when_true, when_false);
                }
                let argument = self.comparison()?;
                self.skip_space();
                if !self.take(')') {
                    return None;
                }
                return apply_parameter_function(&token, &argument)
                    .filter(parameter_value_is_finite);
            }
            if token.eq_ignore_ascii_case("pi") {
                return Some(ParameterValue::Real(std::f64::consts::PI));
            }
        }
        let referenced = || {
            self.aliases
                .get(&token)
                .and_then(Clone::clone)
                .and_then(|id| self.values.get(&id).cloned())
        };
        if quoted {
            referenced()
        } else {
            parse_parameter_literal(&token).or_else(referenced)
        }
    }

    fn token(&mut self) -> Option<(String, bool)> {
        let rest = &self.input[self.offset..];
        if let Some((marker, prefix)) = [
            ("<MOD-DIAM>", "<MOD-DIAM>"),
            ("&lt;MOD-DIAM&gt;", "<MOD-DIAM>"),
            ("<MOD-RHO>", "R"),
            ("&lt;MOD-RHO&gt;", "R"),
        ]
        .into_iter()
        .find(|(marker, _)| rest.starts_with(marker))
        {
            self.offset += marker.len();
            let (value, quoted) = self.token()?;
            return (!quoted).then(|| (format!("{prefix}{value}"), false));
        }
        if rest.starts_with('"') {
            self.offset += 1;
            let mut value = String::new();
            while self.offset < self.input.len() {
                let rest = &self.input[self.offset..];
                if rest.starts_with("\"\"") {
                    value.push('"');
                    self.offset += 2;
                } else if rest.starts_with('"') {
                    self.offset += 1;
                    return Some((value, true));
                } else {
                    let character = rest.chars().next()?;
                    value.push(character);
                    self.offset += character.len_utf8();
                }
            }
            return None;
        }
        let start = self.offset;
        let numeric = self.input[start..]
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit() || character == '.');
        while self.offset < self.input.len() {
            let character = self.input[self.offset..].chars().next()?;
            let exponent_sign = numeric
                && matches!(character, '+' | '-')
                && self.input[start..self.offset].ends_with(['e', 'E']);
            if character.is_whitespace() || (!exponent_sign && "+-*/^(),=<>".contains(character)) {
                break;
            }
            self.offset += character.len_utf8();
        }
        (self.offset > start).then(|| (self.input[start..self.offset].to_string(), false))
    }

    fn skip_space(&mut self) {
        while let Some(character) = self.input[self.offset..].chars().next() {
            if !character.is_whitespace() {
                break;
            }
            self.offset += character.len_utf8();
        }
    }

    fn take(&mut self, expected: char) -> bool {
        if self.input[self.offset..].starts_with(expected) {
            self.offset += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn take_one(&mut self, expected: &[char]) -> Option<char> {
        let character = self.input[self.offset..].chars().next()?;
        expected.contains(&character).then(|| {
            self.offset += character.len_utf8();
            character
        })
    }
}

pub(crate) fn negate_parameter_value(value: &ParameterValue) -> Option<ParameterValue> {
    Some(match value {
        ParameterValue::Length(Length(value)) => ParameterValue::Length(Length(-*value)),
        ParameterValue::Angle(Angle(value)) => ParameterValue::Angle(Angle(-*value)),
        ParameterValue::Real(value) => ParameterValue::Real(-*value),
        ParameterValue::Integer(value) => ParameterValue::Integer(value.checked_neg()?),
        ParameterValue::Boolean(_) | ParameterValue::String(_) => return None,
    })
}

pub(crate) fn add_parameter_values(
    left: ParameterValue,
    right: ParameterValue,
    subtract: bool,
) -> Option<ParameterValue> {
    let sign = if subtract { -1.0 } else { 1.0 };
    Some(match (left, right) {
        (ParameterValue::Length(Length(left)), ParameterValue::Length(Length(right))) => {
            ParameterValue::Length(Length(left + sign * right))
        }
        (ParameterValue::Angle(Angle(left)), ParameterValue::Angle(Angle(right))) => {
            ParameterValue::Angle(Angle(left + sign * right))
        }
        (ParameterValue::Integer(left), ParameterValue::Integer(right)) => {
            let right = if subtract {
                right.checked_neg()?
            } else {
                right
            };
            ParameterValue::Integer(left.checked_add(right)?)
        }
        (left, right) => ParameterValue::Real(
            real_parameter_value(&left)? + sign * real_parameter_value(&right)?,
        ),
    })
}

pub(crate) fn compare_parameter_values(
    left: &ParameterValue,
    right: &ParameterValue,
    operator: &str,
) -> Option<bool> {
    if matches!(
        (left, right),
        (ParameterValue::Boolean(_), ParameterValue::Boolean(_))
    ) && !matches!(operator, "=" | "<>")
    {
        return None;
    }
    let ordering = match (left, right) {
        (ParameterValue::Length(Length(left)), ParameterValue::Length(Length(right)))
        | (ParameterValue::Angle(Angle(left)), ParameterValue::Angle(Angle(right)))
        | (ParameterValue::Real(left), ParameterValue::Real(right)) => left.partial_cmp(right)?,
        (ParameterValue::Integer(left), ParameterValue::Integer(right)) => left.cmp(right),
        (ParameterValue::Real(left), ParameterValue::Integer(right)) => {
            compare_integer_real(*right, *left)?.reverse()
        }
        (ParameterValue::Integer(left), ParameterValue::Real(right)) => {
            compare_integer_real(*left, *right)?
        }
        (ParameterValue::Boolean(left), ParameterValue::Boolean(right)) => left.cmp(right),
        (ParameterValue::String(left), ParameterValue::String(right)) => left.cmp(right),
        _ => return None,
    };
    Some(match operator {
        "=" => ordering.is_eq(),
        "<>" => !ordering.is_eq(),
        "<" => ordering.is_lt(),
        ">" => ordering.is_gt(),
        "<=" => !ordering.is_gt(),
        ">=" => !ordering.is_lt(),
        _ => return None,
    })
}

pub(crate) fn compare_integer_real(integer: i64, real: f64) -> Option<std::cmp::Ordering> {
    if real.is_nan() {
        return None;
    }
    if real < i64::MIN as f64 {
        return Some(std::cmp::Ordering::Greater);
    }
    if real >= -(i64::MIN as f64) {
        return Some(std::cmp::Ordering::Less);
    }

    let truncated = real as i64;
    match integer.cmp(&truncated) {
        std::cmp::Ordering::Equal => 0.0f64.partial_cmp(&real.fract()),
        ordering => Some(ordering),
    }
}

pub(crate) fn conditional_parameter_value(
    condition: &ParameterValue,
    when_true: ParameterValue,
    when_false: ParameterValue,
) -> Option<ParameterValue> {
    let ParameterValue::Boolean(condition) = condition else {
        return None;
    };
    match (&when_true, &when_false) {
        (ParameterValue::Length(_), ParameterValue::Length(_))
        | (ParameterValue::Angle(_), ParameterValue::Angle(_))
        | (ParameterValue::Real(_), ParameterValue::Real(_))
        | (ParameterValue::Integer(_), ParameterValue::Integer(_))
        | (ParameterValue::Boolean(_), ParameterValue::Boolean(_))
        | (ParameterValue::String(_), ParameterValue::String(_)) => {
            Some(if *condition { when_true } else { when_false })
        }
        (ParameterValue::Real(_), ParameterValue::Integer(_))
        | (ParameterValue::Integer(_), ParameterValue::Real(_)) => {
            Some(ParameterValue::Real(real_parameter_value(if *condition {
                &when_true
            } else {
                &when_false
            })?))
        }
        _ => None,
    }
}

pub(crate) fn multiply_parameter_values(
    left: ParameterValue,
    right: ParameterValue,
    divide: bool,
) -> Option<ParameterValue> {
    if divide && parameter_numeric_value(&right)? == 0.0 {
        return None;
    }
    match (left, right) {
        (ParameterValue::Length(Length(left)), ParameterValue::Length(Length(right))) if divide => {
            Some(ParameterValue::Real(left / right))
        }
        (ParameterValue::Angle(Angle(left)), ParameterValue::Angle(Angle(right))) if divide => {
            Some(ParameterValue::Real(left / right))
        }
        (ParameterValue::Length(Length(left)), right) => {
            Some(ParameterValue::Length(Length(if divide {
                left / real_parameter_value(&right)?
            } else {
                left * real_parameter_value(&right)?
            })))
        }
        (ParameterValue::Angle(Angle(left)), right) => {
            Some(ParameterValue::Angle(Angle(if divide {
                left / real_parameter_value(&right)?
            } else {
                left * real_parameter_value(&right)?
            })))
        }
        (left, ParameterValue::Length(Length(right))) if !divide => Some(ParameterValue::Length(
            Length(real_parameter_value(&left)? * right),
        )),
        (left, ParameterValue::Angle(Angle(right))) if !divide => Some(ParameterValue::Angle(
            Angle(real_parameter_value(&left)? * right),
        )),
        (ParameterValue::Integer(left), ParameterValue::Integer(right)) if !divide => {
            Some(ParameterValue::Integer(left.checked_mul(right)?))
        }
        (left, right) => Some(ParameterValue::Real(if divide {
            real_parameter_value(&left)? / real_parameter_value(&right)?
        } else {
            real_parameter_value(&left)? * real_parameter_value(&right)?
        })),
    }
}

pub(crate) fn exponentiate_parameter_value(
    base: &ParameterValue,
    exponent: &ParameterValue,
) -> Option<ParameterValue> {
    if let (ParameterValue::Integer(base), ParameterValue::Integer(exponent)) = (base, exponent) {
        if let Ok(exponent) = u32::try_from(*exponent) {
            return base.checked_pow(exponent).map(ParameterValue::Integer);
        }
        if *exponent >= 0 {
            return match base {
                0 => Some(ParameterValue::Integer(0)),
                1 => Some(ParameterValue::Integer(1)),
                -1 => Some(ParameterValue::Integer(if exponent % 2 == 0 {
                    1
                } else {
                    -1
                })),
                _ => None,
            };
        }
        return Some(ParameterValue::Real(integer_power_real(*base, *exponent)));
    }

    let exponent = real_parameter_value(exponent)?;
    Some(match base {
        ParameterValue::Length(value) if exponent == 1.0 => ParameterValue::Length(*value),
        ParameterValue::Angle(value) if exponent == 1.0 => ParameterValue::Angle(*value),
        ParameterValue::Length(_) | ParameterValue::Angle(_) if exponent == 0.0 => {
            ParameterValue::Real(1.0)
        }
        ParameterValue::Real(base) => ParameterValue::Real(base.powf(exponent)),
        ParameterValue::Integer(base) => {
            if exponent.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&exponent) {
                ParameterValue::Integer(base.checked_pow(exponent as u32)?)
            } else {
                ParameterValue::Real((*base as f64).powf(exponent))
            }
        }
        ParameterValue::Length(_)
        | ParameterValue::Angle(_)
        | ParameterValue::Boolean(_)
        | ParameterValue::String(_) => {
            return None;
        }
    })
}

pub(crate) fn integer_power_real(base: i64, exponent: i64) -> f64 {
    let mut exponent = exponent.unsigned_abs();
    let mut factor = base as f64;
    let mut value = 1.0;
    while exponent != 0 {
        if exponent & 1 != 0 {
            value *= factor;
        }
        exponent >>= 1;
        factor *= factor;
    }
    value.recip()
}

pub(crate) fn apply_parameter_function(
    name: &str,
    argument: &ParameterValue,
) -> Option<ParameterValue> {
    let name = name.to_ascii_lowercase();
    Some(match name.as_str() {
        "abs" => match argument {
            ParameterValue::Length(Length(value)) => ParameterValue::Length(Length(value.abs())),
            ParameterValue::Angle(Angle(value)) => ParameterValue::Angle(Angle(value.abs())),
            ParameterValue::Real(value) => ParameterValue::Real(value.abs()),
            ParameterValue::Integer(value) => ParameterValue::Integer(value.checked_abs()?),
            ParameterValue::Boolean(_) | ParameterValue::String(_) => return None,
        },
        "sin" | "cos" | "tan" | "sec" | "cosec" | "cotan" => {
            let ParameterValue::Angle(Angle(angle)) = argument else {
                return None;
            };
            ParameterValue::Real(match name.as_str() {
                "sin" => angle.sin(),
                "cos" => angle.cos(),
                "tan" => angle.tan(),
                "sec" => angle.cos().recip(),
                "cosec" => angle.sin().recip(),
                "cotan" => angle.tan().recip(),
                _ => unreachable!(),
            })
        }
        "arcsin" | "arccos" | "atn" | "arcsec" | "arccosec" | "arccotan" => {
            let value = real_parameter_value(argument)?;
            ParameterValue::Angle(Angle(match name.as_str() {
                "arcsin" => value.asin(),
                "arccos" => value.acos(),
                "atn" => value.atan(),
                "arcsec" => value.recip().acos(),
                "arccosec" => value.recip().asin(),
                "arccotan" => value.recip().atan(),
                _ => unreachable!(),
            }))
        }
        "exp" => ParameterValue::Real(real_parameter_value(argument)?.exp()),
        "log" => ParameterValue::Real(real_parameter_value(argument)?.ln()),
        "sqr" => ParameterValue::Real(real_parameter_value(argument)?.sqrt()),
        "int" => match argument {
            ParameterValue::Integer(value) => ParameterValue::Integer(*value),
            ParameterValue::Real(value) => {
                let value = value.trunc();
                if value < i64::MIN as f64 || value >= -(i64::MIN as f64) {
                    return None;
                }
                ParameterValue::Integer(value as i64)
            }
            ParameterValue::Length(_)
            | ParameterValue::Angle(_)
            | ParameterValue::Boolean(_)
            | ParameterValue::String(_) => {
                return None;
            }
        },
        "sgn" => {
            let value = parameter_numeric_value(argument)?;
            if !value.is_finite() {
                return None;
            }
            ParameterValue::Integer(match value.partial_cmp(&0.0)? {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        }
        _ => return None,
    })
}

pub(crate) fn real_parameter_value(value: &ParameterValue) -> Option<f64> {
    match value {
        ParameterValue::Real(value) => Some(*value),
        ParameterValue::Integer(value) => Some(*value as f64),
        _ => None,
    }
}

pub(crate) fn parameter_numeric_value(value: &ParameterValue) -> Option<f64> {
    match value {
        ParameterValue::Length(Length(value))
        | ParameterValue::Angle(Angle(value))
        | ParameterValue::Real(value) => Some(*value),
        ParameterValue::Integer(value) => Some(*value as f64),
        ParameterValue::Boolean(_) | ParameterValue::String(_) => None,
    }
}

/// Convert a discrete integer to a native scalar without changing its value.
pub(crate) fn exact_integer_f64(value: i64) -> Option<f64> {
    let encoded = value as f64;
    ((encoded as i128) == i128::from(value)).then_some(encoded)
}

pub(crate) fn parameter_value_is_finite(value: &ParameterValue) -> bool {
    parameter_numeric_value(value).is_none_or(f64::is_finite)
}
