// SPDX-License-Identifier: Apache-2.0
//! Parameter-expression parser and arithmetic.

use cadmpeg_core::{decode::{DecodeContext, ScopedReservation}, CodecError};
use cadmpeg_ir::{
    features::{ParameterId, ParameterValue},
    scalar::{Angle, FiniteReal, Length},
};
use std::collections::HashMap;

use super::{copy_parameter_value, ParameterAliasView};
use crate::history::literals::parse_parameter_literal;

enum TokenText<'a, 'ctx> {
    Borrowed(&'a str),
    Owned { value: String, _reservation: ScopedReservation<'ctx> },
}

impl TokenText<'_, '_> {
    fn as_str(&self) -> &str {
        match self { Self::Borrowed(value) => value, Self::Owned { value, .. } => value }
    }
}

enum Token<'a, 'ctx> {
    Quoted(TokenText<'a, 'ctx>),
    Bare(TokenText<'a, 'ctx>),
}

enum ExpressionFailure {
    NoValue,
    Resource(CodecError),
}

impl From<CodecError> for ExpressionFailure {
    fn from(error: CodecError) -> Self { Self::Resource(error) }
}

pub(super) struct ParameterExpressionParser<'a, 'ctx, 'arena> {
    ctx: &'ctx DecodeContext<'arena>,
    input: &'a str,
    offset: usize,
    aliases: ParameterAliasMap<'a>,
    values: &'a HashMap<ParameterId, ParameterValue>,
}

enum ParameterAliasMap<'a> {
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

impl<'a, 'ctx, 'arena> ParameterExpressionParser<'a, 'ctx, 'arena> {
    pub(super) fn new(
        ctx: &'ctx DecodeContext<'arena>, input: &'a str,
        aliases: ParameterAliasView<'a>, values: &'a HashMap<ParameterId, ParameterValue>,
    ) -> Self {
        Self { ctx, input, offset: 0, aliases: ParameterAliasMap::Layered(aliases), values }
    }

    #[cfg(test)]
    pub(in crate::history) fn new_flat(
        ctx: &'ctx DecodeContext<'arena>, input: &'a str,
        aliases: &'a HashMap<String, Option<ParameterId>>, values: &'a HashMap<ParameterId, ParameterValue>,
    ) -> Self {
        Self { ctx, input, offset: 0, aliases: ParameterAliasMap::Flat(aliases), values }
    }

    pub(super) fn parse(mut self) -> Result<Option<ParameterValue>, CodecError> {
        match self.parse_value() {
            Ok(value) => Ok(Some(value)),
            Err(ExpressionFailure::NoValue) => Ok(None),
            Err(ExpressionFailure::Resource(error)) => Err(error),
        }
    }

    fn parse_value(&mut self) -> Result<ParameterValue, ExpressionFailure> {
        self.skip_space()?;
        self.take('=');
        self.skip_space()?;
        self.ctx.charge_work((self.input.len() - self.offset) as u64, "parse SLDPRT parameter literal")?;
        if let Some(value) = parse_parameter_literal(&self.input[self.offset..]) { return Ok(value); }
        let value = self.comparison()?;
        self.skip_space()?;
        if self.offset == self.input.len() { Ok(value) } else { Err(ExpressionFailure::NoValue) }
    }

    fn comparison(&mut self) -> Result<ParameterValue, ExpressionFailure> {
        let _depth = self.ctx.enter_nested("parse SLDPRT parameter comparison")?;
        self.ctx.charge_work(1, "parse SLDPRT parameter expression")?;
        let left = self.sum()?;
        self.skip_space()?;
        let operator = ["<=", ">=", "<>", "=", "<", ">"].into_iter()
            .find(|operator| self.input[self.offset..].starts_with(operator));
        let Some(operator) = operator else { return Ok(left); };
        self.offset += operator.len();
        compare_parameter_values(&left, &self.sum()?, operator)
            .map(ParameterValue::Boolean).ok_or(ExpressionFailure::NoValue)
    }

    fn sum(&mut self) -> Result<ParameterValue, ExpressionFailure> {
        let mut value = self.product()?;
        loop {
            self.ctx.charge_work(1, "parse SLDPRT parameter expression")?;
            self.skip_space()?;
            let Some(op) = self.take_one(&['+', '-']) else { return Ok(value); };
            value = add_parameter_values(value, self.product()?, op == '-').ok_or(ExpressionFailure::NoValue)?;
        }
    }

    fn product(&mut self) -> Result<ParameterValue, ExpressionFailure> {
        let mut value = self.unary()?;
        loop {
            self.ctx.charge_work(1, "parse SLDPRT parameter expression")?;
            self.skip_space()?;
            let Some(op) = self.take_one(&['*', '/']) else { return Ok(value); };
            value = multiply_parameter_values(value, self.unary()?, op == '/').ok_or(ExpressionFailure::NoValue)?;
        }
    }

    fn unary(&mut self) -> Result<ParameterValue, ExpressionFailure> {
        let _depth = self.ctx.enter_nested("parse SLDPRT parameter unary")?;
        self.ctx.charge_work(1, "parse SLDPRT parameter expression")?;
        self.skip_space()?;
        if self.take('-') { negate_parameter_value(&self.unary()?).ok_or(ExpressionFailure::NoValue) }
        else if self.take('+') { self.unary() }
        else { self.power() }
    }

    fn power(&mut self) -> Result<ParameterValue, ExpressionFailure> {
        let base = self.primary()?;
        self.skip_space()?;
        if self.take('^') { exponentiate_parameter_value(&base, &self.unary()?).ok_or(ExpressionFailure::NoValue) }
        else { Ok(base) }
    }

    fn primary(&mut self) -> Result<ParameterValue, ExpressionFailure> {
        self.skip_space()?;
        if self.take('(') {
            let value = self.comparison()?;
            self.skip_space()?;
            return if self.take(')') { Ok(value) } else { Err(ExpressionFailure::NoValue) };
        }
        let token = self.token()?;
        if let Token::Bare(token) = &token {
            self.skip_space()?;
            if self.take('(') {
                let function = ParameterFunction::parse(token.as_str()).ok_or(ExpressionFailure::NoValue)?;
                let mut arguments = Vec::with_capacity(function.argument_count());
                for index in 0..function.argument_count() {
                    if index != 0 {
                        self.skip_space()?;
                        if !self.take(',') { return Err(ExpressionFailure::NoValue); }
                    }
                    arguments.push(self.comparison()?);
                }
                self.skip_space()?;
                if !self.take(')') { return Err(ExpressionFailure::NoValue); }
                return function.apply(arguments).ok_or(ExpressionFailure::NoValue);
            }
            if token.as_str().eq_ignore_ascii_case("pi") {
                return FiniteReal::new(std::f64::consts::PI).map(ParameterValue::Real).ok_or(ExpressionFailure::NoValue);
            }
        }
        let referenced = |token: &str| -> Result<ParameterValue, ExpressionFailure> {
            let value = self.aliases.get(token).and_then(Option::as_ref)
                .and_then(|id| self.values.get(id)).ok_or(ExpressionFailure::NoValue)?;
            Ok(copy_parameter_value(self.ctx, value)?)
        };
        match token {
            Token::Quoted(token) => referenced(token.as_str()),
            Token::Bare(token) => match parse_parameter_literal(token.as_str()) {
                Some(value) => Ok(value), None => referenced(token.as_str()),
            },
        }
    }

    fn token(&mut self) -> Result<Token<'a, 'ctx>, ExpressionFailure> {
        let _depth = self.ctx.enter_nested("parse SLDPRT parameter token")?;
        self.ctx.charge_work(1, "parse SLDPRT parameter token")?;
        let rest = &self.input[self.offset..];
        if let Some((marker, prefix)) = [
            ("<MOD-DIAM>", "<MOD-DIAM>"), ("&lt;MOD-DIAM&gt;", "<MOD-DIAM>"),
            ("<MOD-RHO>", "R"), ("&lt;MOD-RHO&gt;", "R"),
        ].into_iter().find(|(marker, _)| rest.starts_with(marker)) {
            self.offset += marker.len();
            let Token::Bare(value) = self.token()? else { return Err(ExpressionFailure::NoValue); };
            let bytes = prefix.len().checked_add(value.as_str().len()).ok_or_else(|| self.ctx.refuse_codec_limit(
                "normalize SLDPRT parameter token", u64::MAX - 1, u64::MAX,
            ))?;
            let (mut text, reservation) = self.ctx.reserve_scoped_string(bytes, "normalize SLDPRT parameter token")?;
            text.push_str(prefix);
            text.push_str(value.as_str());
            return Ok(Token::Bare(TokenText::Owned { value: text, _reservation: reservation }));
        }
        if rest.starts_with('"') {
            self.offset += 1;
            let start = self.offset;
            let mut end = start;
            let mut closed = false;
            while end < self.input.len() {
                self.ctx.charge_work(1, "scan SLDPRT quoted parameter token")?;
                let rest = &self.input[end..];
                if rest.starts_with("\"\"") { end += 2; }
                else if rest.starts_with('"') { closed = true; break; }
                else { end += rest.chars().next().ok_or(ExpressionFailure::NoValue)?.len_utf8(); }
            }
            if !closed { self.offset = end; return Err(ExpressionFailure::NoValue); }
            let (mut value, reservation) = self.ctx.reserve_scoped_string(end - start, "retain SLDPRT quoted parameter token")?;
            self.ctx.charge_work((end - start) as u64, "copy SLDPRT quoted parameter token")?;
            let mut cursor = start;
            while cursor < end {
                let rest = &self.input[cursor..end];
                if rest.starts_with("\"\"") { value.push('"'); cursor += 2; }
                else {
                    let character = rest.chars().next().ok_or(ExpressionFailure::NoValue)?;
                    value.push(character); cursor += character.len_utf8();
                }
            }
            self.offset = end + 1;
            return Ok(Token::Quoted(TokenText::Owned { value, _reservation: reservation }));
        }
        let start = self.offset;
        let numeric = self.input[start..].chars().next()
            .is_some_and(|character| character.is_ascii_digit() || character == '.');
        while self.offset < self.input.len() {
            self.ctx.charge_work(1, "scan SLDPRT parameter token")?;
            let character = self.input[self.offset..].chars().next().ok_or(ExpressionFailure::NoValue)?;
            let exponent_sign = numeric && matches!(character, '+' | '-')
                && self.input[start..self.offset].ends_with(['e', 'E']);
            if character.is_whitespace() || (!exponent_sign && "+-*/^(),=<>".contains(character)) { break; }
            self.offset += character.len_utf8();
        }
        if self.offset == start { return Err(ExpressionFailure::NoValue); }
        Ok(Token::Bare(TokenText::Borrowed(&self.input[start..self.offset])))
    }

    fn skip_space(&mut self) -> Result<(), ExpressionFailure> {
        while let Some(character) = self.input[self.offset..].chars().next() {
            self.ctx.charge_work(1, "scan SLDPRT parameter whitespace")?;
            if !character.is_whitespace() { break; }
            self.offset += character.len_utf8();
        }
        Ok(())
    }

    fn take(&mut self, expected: char) -> bool {
        if self.input[self.offset..].starts_with(expected) { self.offset += expected.len_utf8(); true }
        else { false }
    }

    fn take_one(&mut self, expected: &[char]) -> Option<char> {
        let character = self.input[self.offset..].chars().next()?;
        expected.contains(&character).then(|| { self.offset += character.len_utf8(); character })
    }
}

fn negate_parameter_value(value: &ParameterValue) -> Option<ParameterValue> {
    Some(match value {
        ParameterValue::Length(value) => ParameterValue::Length(value.negated()),
        ParameterValue::Angle(value) => ParameterValue::Angle(value.negated()),
        ParameterValue::Real(value) => ParameterValue::Real(value.negated()),
        ParameterValue::Integer(value) => ParameterValue::Integer(value.checked_neg()?),
        ParameterValue::Boolean(_) | ParameterValue::String(_) => return None,
    })
}

fn add_parameter_values(
    left: ParameterValue,
    right: ParameterValue,
    subtract: bool,
) -> Option<ParameterValue> {
    let sign = if subtract { -1.0 } else { 1.0 };
    Some(match (left, right) {
        (ParameterValue::Length(left), ParameterValue::Length(right)) => {
            ParameterValue::Length(Length::new(left.get() + sign * right.get())?)
        }
        (ParameterValue::Angle(left), ParameterValue::Angle(right)) => {
            ParameterValue::Angle(Angle::new(left.get() + sign * right.get())?)
        }
        (ParameterValue::Integer(left), ParameterValue::Integer(right)) => {
            let right = if subtract {
                right.checked_neg()?
            } else {
                right
            };
            ParameterValue::Integer(left.checked_add(right)?)
        }
        (left, right) => ParameterValue::Real(FiniteReal::new(
            real_parameter_value(&left)? + sign * real_parameter_value(&right)?,
        )?),
    })
}

pub(super) fn compare_parameter_values(
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
        (ParameterValue::Length(left), ParameterValue::Length(right)) => left.partial_cmp(right)?,
        (ParameterValue::Angle(left), ParameterValue::Angle(right)) => left.partial_cmp(right)?,
        (ParameterValue::Real(left), ParameterValue::Real(right)) => left.partial_cmp(right)?,
        (ParameterValue::Integer(left), ParameterValue::Integer(right)) => left.cmp(right),
        (ParameterValue::Real(left), ParameterValue::Integer(right)) => {
            compare_integer_real(*right, left.get())?.reverse()
        }
        (ParameterValue::Integer(left), ParameterValue::Real(right)) => {
            compare_integer_real(*left, right.get())?
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

fn compare_integer_real(integer: i64, real: f64) -> Option<std::cmp::Ordering> {
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

fn conditional_parameter_value(
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
            Some(ParameterValue::Real(FiniteReal::new(
                real_parameter_value(if *condition { &when_true } else { &when_false })?,
            )?))
        }
        _ => None,
    }
}

fn multiply_parameter_values(
    left: ParameterValue,
    right: ParameterValue,
    divide: bool,
) -> Option<ParameterValue> {
    if divide && parameter_numeric_value(&right)? == 0.0 {
        return None;
    }
    match (left, right) {
        (ParameterValue::Length(left), ParameterValue::Length(right)) if divide => Some(
            ParameterValue::Real(FiniteReal::new(left.get() / right.get())?),
        ),
        (ParameterValue::Angle(left), ParameterValue::Angle(right)) if divide => Some(
            ParameterValue::Real(FiniteReal::new(left.get() / right.get())?),
        ),
        (ParameterValue::Length(left), right) => {
            Some(ParameterValue::Length(Length::new(if divide {
                left.get() / real_parameter_value(&right)?
            } else {
                left.get() * real_parameter_value(&right)?
            })?))
        }
        (ParameterValue::Angle(left), right) => {
            Some(ParameterValue::Angle(Angle::new(if divide {
                left.get() / real_parameter_value(&right)?
            } else {
                left.get() * real_parameter_value(&right)?
            })?))
        }
        (left, ParameterValue::Length(right)) if !divide => Some(ParameterValue::Length(
            Length::new(real_parameter_value(&left)? * right.get())?,
        )),
        (left, ParameterValue::Angle(right)) if !divide => Some(ParameterValue::Angle(Angle::new(
            real_parameter_value(&left)? * right.get(),
        )?)),
        (ParameterValue::Integer(left), ParameterValue::Integer(right)) if !divide => {
            Some(ParameterValue::Integer(left.checked_mul(right)?))
        }
        (left, right) => Some(ParameterValue::Real(FiniteReal::new(if divide {
            real_parameter_value(&left)? / real_parameter_value(&right)?
        } else {
            real_parameter_value(&left)? * real_parameter_value(&right)?
        })?)),
    }
}

pub(super) fn exponentiate_parameter_value(
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
        return Some(ParameterValue::Real(FiniteReal::new(integer_power_real(
            *base, *exponent,
        ))?));
    }

    let exponent = real_parameter_value(exponent)?;
    Some(match base {
        ParameterValue::Length(value) if exponent == 1.0 => ParameterValue::Length(*value),
        ParameterValue::Angle(value) if exponent == 1.0 => ParameterValue::Angle(*value),
        ParameterValue::Length(_) | ParameterValue::Angle(_) if exponent == 0.0 => {
            ParameterValue::Real(FiniteReal::ONE)
        }
        ParameterValue::Real(base) => {
            ParameterValue::Real(FiniteReal::new(base.get().powf(exponent))?)
        }
        ParameterValue::Integer(base) => {
            if exponent.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&exponent) {
                ParameterValue::Integer(base.checked_pow(exponent as u32)?)
            } else {
                ParameterValue::Real(FiniteReal::new((*base as f64).powf(exponent))?)
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

fn integer_power_real(base: i64, exponent: i64) -> f64 {
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

#[derive(Debug, Clone, Copy)]
pub(super) enum ParameterFunction {
    Iif,
    Abs,
    Sin,
    Cos,
    Tan,
    Sec,
    Cosec,
    Cotan,
    Arcsin,
    Arccos,
    Atn,
    Arcsec,
    Arccosec,
    Arccotan,
    Exp,
    Log,
    Sqr,
    Int,
    Sgn,
}

impl ParameterFunction {
    pub(super) fn parse(name: &str) -> Option<Self> {
        [
            ("iif", Self::Iif),
            ("abs", Self::Abs),
            ("sin", Self::Sin),
            ("cos", Self::Cos),
            ("tan", Self::Tan),
            ("sec", Self::Sec),
            ("cosec", Self::Cosec),
            ("cotan", Self::Cotan),
            ("arcsin", Self::Arcsin),
            ("arccos", Self::Arccos),
            ("atn", Self::Atn),
            ("arcsec", Self::Arcsec),
            ("arccosec", Self::Arccosec),
            ("arccotan", Self::Arccotan),
            ("exp", Self::Exp),
            ("log", Self::Log),
            ("sqr", Self::Sqr),
            ("int", Self::Int),
            ("sgn", Self::Sgn),
        ]
        .into_iter()
        .find_map(|(word, function)| name.eq_ignore_ascii_case(word).then_some(function))
    }

    fn argument_count(self) -> usize {
        match self {
            Self::Iif => 3,
            _ => 1,
        }
    }

    pub(super) fn apply(self, arguments: Vec<ParameterValue>) -> Option<ParameterValue> {
        if let Self::Iif = self {
            let [condition, when_true, when_false]: [ParameterValue; 3] =
                arguments.try_into().ok()?;
            return conditional_parameter_value(&condition, when_true, when_false);
        }
        let arguments = arguments.as_slice();
        let unary = || {
            let [argument] = arguments else {
                return None;
            };
            Some(argument)
        };
        let angle = || {
            let ParameterValue::Angle(value) = unary()? else {
                return None;
            };
            Some(value.get())
        };
        Some(match self {
            Self::Iif => return None,
            Self::Abs => match unary()? {
                ParameterValue::Length(value) => ParameterValue::Length(value.abs()),
                ParameterValue::Angle(value) => ParameterValue::Angle(value.abs()),
                ParameterValue::Real(value) => ParameterValue::Real(value.abs()),
                ParameterValue::Integer(value) => ParameterValue::Integer(value.checked_abs()?),
                ParameterValue::Boolean(_) | ParameterValue::String(_) => return None,
            },
            Self::Sin => ParameterValue::Real(FiniteReal::new(angle()?.sin())?),
            Self::Cos => ParameterValue::Real(FiniteReal::new(angle()?.cos())?),
            Self::Tan => ParameterValue::Real(FiniteReal::new(angle()?.tan())?),
            Self::Sec => ParameterValue::Real(FiniteReal::new(angle()?.cos().recip())?),
            Self::Cosec => ParameterValue::Real(FiniteReal::new(angle()?.sin().recip())?),
            Self::Cotan => ParameterValue::Real(FiniteReal::new(angle()?.tan().recip())?),
            Self::Arcsin => {
                ParameterValue::Angle(Angle::new(real_parameter_value(unary()?)?.asin())?)
            }
            Self::Arccos => {
                ParameterValue::Angle(Angle::new(real_parameter_value(unary()?)?.acos())?)
            }
            Self::Atn => ParameterValue::Angle(Angle::new(real_parameter_value(unary()?)?.atan())?),
            Self::Arcsec => {
                ParameterValue::Angle(Angle::new(real_parameter_value(unary()?)?.recip().acos())?)
            }
            Self::Arccosec => {
                ParameterValue::Angle(Angle::new(real_parameter_value(unary()?)?.recip().asin())?)
            }
            Self::Arccotan => {
                ParameterValue::Angle(Angle::new(real_parameter_value(unary()?)?.recip().atan())?)
            }
            Self::Exp => {
                ParameterValue::Real(FiniteReal::new(real_parameter_value(unary()?)?.exp())?)
            }
            Self::Log => {
                ParameterValue::Real(FiniteReal::new(real_parameter_value(unary()?)?.ln())?)
            }
            Self::Sqr => {
                ParameterValue::Real(FiniteReal::new(real_parameter_value(unary()?)?.sqrt())?)
            }
            Self::Int => match unary()? {
                ParameterValue::Integer(value) => ParameterValue::Integer(*value),
                ParameterValue::Real(value) => {
                    let value = value.get().trunc();
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
            Self::Sgn => {
                let value = parameter_numeric_value(unary()?)?;
                ParameterValue::Integer(match value.partial_cmp(&0.0)? {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                })
            }
        })
    }
}

fn real_parameter_value(value: &ParameterValue) -> Option<f64> {
    match value {
        ParameterValue::Real(value) => Some(value.get()),
        ParameterValue::Integer(value) => Some(*value as f64),
        _ => None,
    }
}

fn parameter_numeric_value(value: &ParameterValue) -> Option<f64> {
    match value {
        ParameterValue::Length(value) => Some(value.get()),
        ParameterValue::Angle(value) => Some(value.get()),
        ParameterValue::Real(value) => Some(value.get()),
        ParameterValue::Integer(value) => Some(*value as f64),
        ParameterValue::Boolean(_) | ParameterValue::String(_) => None,
    }
}

/// Convert a discrete integer to a native scalar without changing its value.
pub(crate) fn exact_integer_f64(value: i64) -> Option<f64> {
    let encoded = value as f64;
    ((encoded as i128) == i128::from(value)).then_some(encoded)
}

#[cfg(test)]
mod tests {
    use super::{
        add_parameter_values, exponentiate_parameter_value, multiply_parameter_values,
        ParameterFunction,
    };
    use cadmpeg_ir::{features::ParameterValue, scalar::FiniteReal};

    #[test]
    fn real_arithmetic_rejects_non_finite_results_at_construction() {
        let largest = ParameterValue::Real(FiniteReal::new(f64::MAX).unwrap());
        let two = ParameterValue::Integer(2);
        assert!(add_parameter_values(largest.clone(), largest.clone(), false).is_none());
        assert!(multiply_parameter_values(largest.clone(), two.clone(), false).is_none());
        assert!(exponentiate_parameter_value(&largest, &two).is_none());
        assert!(ParameterFunction::Exp.apply(vec![largest]).is_none());
        let negative = ParameterValue::Real(FiniteReal::new(-1.0).unwrap());
        assert!(ParameterFunction::Log
            .apply(vec![negative.clone()])
            .is_none());
        assert!(ParameterFunction::Sqr.apply(vec![negative]).is_none());
    }
}
