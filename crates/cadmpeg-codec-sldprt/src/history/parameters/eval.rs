// SPDX-License-Identifier: Apache-2.0
//! Parameter-expression parser and arithmetic.

use cadmpeg_core::convert::{f64_from_i64, truncate_f64_to_i64, truncate_f64_to_u32};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{
    features::{DesignParameter, ParameterId, ParameterValue},
    scalar::{Angle, FiniteReal, Length},
};
use std::borrow::Cow;
use std::collections::HashMap;

use super::{ParameterAliasView, ParameterTokenText};
use crate::history::literals::{admit_literal, parse_parameter_literal};

enum Token<'a, 'ctx> {
    Quoted(ParameterTokenText<'a, 'ctx>),
    Bare(ParameterTokenText<'a, 'ctx>),
}

enum ExpressionFailure {
    NoValue,
    Resource(CodecError),
}

impl From<CodecError> for ExpressionFailure {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

pub(super) struct ParameterExpressionParser<'a, 'ctx, 'arena> {
    ctx: &'ctx DecodeContext<'arena>,
    input: &'a str,
    offset: usize,
    aliases: ParameterAliasMap<'a>,
    values: ParameterValues<'a>,
    blocked: Option<&'a ParameterId>,
}

/// Borrowed validation layers or positions in the model being evaluated.
pub(super) enum ParameterValues<'a> {
    #[cfg(test)]
    Stored(&'a HashMap<ParameterId, ParameterValue>),
    Validation {
        values: &'a super::ParameterValueIndex<'a>,
        configuration: Option<&'a cadmpeg_ir::features::DesignConfiguration>,
        excluded: &'a ParameterId,
    },
    Indexed {
        parameters: &'a [DesignParameter],
        positions: &'a HashMap<ParameterId, usize>,
    },
}

pub(super) enum ParameterEvaluation {
    Value(ParameterValue),
    Blocked(ParameterId),
    Invalid,
}

enum ParameterAliasMap<'a> {
    Layered(ParameterAliasView<'a>),
    #[cfg(test)]
    Flat(&'a HashMap<String, Option<ParameterId>>),
}

impl<'a> ParameterAliasMap<'a> {
    fn get(
        &self,
        ctx: &DecodeContext<'_>,
        alias: &str,
    ) -> Result<Option<&'a Option<ParameterId>>, CodecError> {
        match self {
            Self::Layered(aliases) => aliases.get(ctx, alias),
            #[cfg(test)]
            Self::Flat(aliases) => ctx.get_hash_map(aliases, alias, "look up SLDPRT hash key"),
        }
    }
}

impl<'a, 'ctx, 'arena> ParameterExpressionParser<'a, 'ctx, 'arena> {
    pub(super) fn new(
        ctx: &'ctx DecodeContext<'arena>,
        input: &'a str,
        aliases: ParameterAliasView<'a>,
        values: ParameterValues<'a>,
    ) -> Self {
        Self {
            ctx,
            input,
            offset: 0,
            aliases: ParameterAliasMap::Layered(aliases),
            values,
            blocked: None,
        }
    }

    #[cfg(test)]
    pub(in crate::history) fn new_flat(
        ctx: &'ctx DecodeContext<'arena>,
        input: &'a str,
        aliases: &'a HashMap<String, Option<ParameterId>>,
        values: &'a HashMap<ParameterId, ParameterValue>,
    ) -> Self {
        Self {
            ctx,
            input,
            offset: 0,
            aliases: ParameterAliasMap::Flat(aliases),
            values: ParameterValues::Stored(values),
            blocked: None,
        }
    }

    pub(super) fn parse(mut self) -> Result<Option<ParameterValue>, CodecError> {
        self.parse_borrowed()?
            .map(|value| self.retain_value(value))
            .transpose()
    }

    /// An evaluated result that borrows referenced values.
    pub(super) fn parse_borrowed(&mut self) -> Result<Option<Cow<'a, ParameterValue>>, CodecError> {
        match self.parse_value() {
            Ok(value) => Ok(Some(value)),
            Err(ExpressionFailure::NoValue) => Ok(None),
            Err(ExpressionFailure::Resource(error)) => Err(error),
        }
    }

    /// Evaluate once and name the first absent value that prevented evaluation.
    pub(super) fn evaluate(
        mut self,
        storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ) -> Result<ParameterEvaluation, CodecError> {
        match self.parse_value() {
            Ok(value) => Ok(ParameterEvaluation::Value(self.retain_value(value)?)),
            Err(ExpressionFailure::NoValue) => match self.blocked {
                Some(id) => {
                    let id = storage.with_storage(|| {
                        id.try_clone_for_decode(
                            self.ctx,
                            "index SLDPRT blocked parameter evaluations",
                        )
                    })?;
                    Ok(ParameterEvaluation::Blocked(id))
                }
                None => Ok(ParameterEvaluation::Invalid),
            },
            Err(ExpressionFailure::Resource(error)) => Err(error),
        }
    }

    fn retain_value(&self, value: Cow<'a, ParameterValue>) -> Result<ParameterValue, CodecError> {
        match value {
            Cow::Owned(value) => Ok(value),
            Cow::Borrowed(value) => {
                value.try_clone_for_decode(self.ctx, "retain SLDPRT parameter value text")
            }
        }
    }

    fn parse_value(&mut self) -> Result<Cow<'a, ParameterValue>, ExpressionFailure> {
        self.skip_space()?;
        self.take('=');
        self.skip_space()?;
        self.ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(self.input.len() - self.offset),
            "parse SLDPRT parameter literal",
        )?;
        if let Some(value) = parse_parameter_literal(&self.input[self.offset..]) {
            return Ok(Cow::Owned(value));
        }
        let value = self.comparison()?;
        self.skip_space()?;
        if self.offset == self.input.len() {
            Ok(value)
        } else {
            Err(ExpressionFailure::NoValue)
        }
    }

    fn comparison(&mut self) -> Result<Cow<'a, ParameterValue>, ExpressionFailure> {
        let _depth = self.ctx.enter_nested("parse SLDPRT parameter comparison")?;
        self.ctx
            .charge_work(1, "parse SLDPRT parameter expression")?;
        let left = self.sum()?;
        self.skip_space()?;
        let operator = ["<=", ">=", "<>", "=", "<", ">"]
            .into_iter()
            .find(|operator| self.input[self.offset..].starts_with(operator));
        let Some(operator) = operator else {
            return Ok(left);
        };
        self.offset += operator.len();
        compare_parameter_values(self.ctx, &left, self.sum()?.as_ref(), operator)?
            .map(ParameterValue::Boolean)
            .map(Cow::Owned)
            .ok_or(ExpressionFailure::NoValue)
    }

    fn sum(&mut self) -> Result<Cow<'a, ParameterValue>, ExpressionFailure> {
        let mut value = self.product()?;
        loop {
            self.ctx
                .charge_work(1, "parse SLDPRT parameter expression")?;
            self.skip_space()?;
            let Some(op) = self.take_one(&['+', '-']) else {
                return Ok(value);
            };
            value = add_parameter_values(&value, self.product()?.as_ref(), op == '-')
                .map(Cow::Owned)
                .ok_or(ExpressionFailure::NoValue)?;
        }
    }

    fn product(&mut self) -> Result<Cow<'a, ParameterValue>, ExpressionFailure> {
        let mut value = self.unary()?;
        loop {
            self.ctx
                .charge_work(1, "parse SLDPRT parameter expression")?;
            self.skip_space()?;
            let Some(op) = self.take_one(&['*', '/']) else {
                return Ok(value);
            };
            value = multiply_parameter_values(&value, self.unary()?.as_ref(), op == '/')
                .map(Cow::Owned)
                .ok_or(ExpressionFailure::NoValue)?;
        }
    }

    fn unary(&mut self) -> Result<Cow<'a, ParameterValue>, ExpressionFailure> {
        let _depth = self.ctx.enter_nested("parse SLDPRT parameter unary")?;
        self.ctx
            .charge_work(1, "parse SLDPRT parameter expression")?;
        self.skip_space()?;
        if self.take('-') {
            negate_parameter_value(self.unary()?.as_ref())
                .map(Cow::Owned)
                .ok_or(ExpressionFailure::NoValue)
        } else if self.take('+') {
            self.unary()
        } else {
            self.power()
        }
    }

    fn power(&mut self) -> Result<Cow<'a, ParameterValue>, ExpressionFailure> {
        let base = self.primary()?;
        self.skip_space()?;
        if self.take('^') {
            exponentiate_parameter_value(&base, self.unary()?.as_ref())
                .map(Cow::Owned)
                .ok_or(ExpressionFailure::NoValue)
        } else {
            Ok(base)
        }
    }

    fn primary(&mut self) -> Result<Cow<'a, ParameterValue>, ExpressionFailure> {
        self.skip_space()?;
        if self.take('(') {
            let value = self.comparison()?;
            self.skip_space()?;
            return if self.take(')') {
                Ok(value)
            } else {
                Err(ExpressionFailure::NoValue)
            };
        }
        let token = self.token()?;
        if let Token::Bare(token) = &token {
            self.skip_space()?;
            if self.take('(') {
                let function =
                    ParameterFunction::parse(token.as_str()).ok_or(ExpressionFailure::NoValue)?;
                let mut argument_storage = self
                    .ctx
                    .reserve_scoped(0, "SLDPRT parameter function arguments")?;
                let mut arguments = Vec::new();
                for index in 0..function.argument_count() {
                    if index != 0 {
                        self.skip_space()?;
                        if !self.take(',') {
                            return Err(ExpressionFailure::NoValue);
                        }
                    }
                    let value = self.comparison()?;
                    self.ctx.push_scoped_vec(
                        &mut argument_storage,
                        &mut arguments,
                        value,
                        "collect SLDPRT decoded vector items",
                    )?;
                }
                self.skip_space()?;
                if !self.take(')') {
                    return Err(ExpressionFailure::NoValue);
                }
                return function.apply(arguments).ok_or(ExpressionFailure::NoValue);
            }
            if token.as_str().eq_ignore_ascii_case("pi") {
                return FiniteReal::new(std::f64::consts::PI)
                    .map(ParameterValue::Real)
                    .map(Cow::Owned)
                    .ok_or(ExpressionFailure::NoValue);
            }
        }
        let mut referenced = |token: &str| -> Result<Cow<'a, ParameterValue>, ExpressionFailure> {
            let id = self
                .aliases
                .get(self.ctx, token)?
                .and_then(Option::as_ref)
                .ok_or(ExpressionFailure::NoValue)?;
            let value = match &self.values {
                #[cfg(test)]
                ParameterValues::Stored(values) => {
                    self.ctx
                        .get_hash_map(values, id, "look up SLDPRT hash key")?
                }
                ParameterValues::Validation {
                    values,
                    configuration,
                    excluded,
                } => {
                    if self
                        .ctx
                        .equal(id, *excluded, "check SLDPRT parameter evaluation")?
                    {
                        None
                    } else {
                        super::configuration_parameter_value(self.ctx, values, *configuration, id)?
                    }
                }
                ParameterValues::Indexed {
                    parameters,
                    positions,
                } => self
                    .ctx
                    .get_hash_map(positions, id, "look up SLDPRT hash key")?
                    .and_then(|index| parameters.get(*index))
                    .and_then(|parameter| parameter.value.as_ref()),
            };
            let Some(value) = value else {
                self.blocked = Some(id);
                return Err(ExpressionFailure::NoValue);
            };
            Ok(Cow::Borrowed(value))
        };
        match token {
            Token::Quoted(token) => referenced(token.as_str()),
            Token::Bare(token) => {
                admit_literal(self.ctx, token.as_str(), "parse SLDPRT parameter literal")?;
                match parse_parameter_literal(token.as_str()) {
                    Some(value) => Ok(Cow::Owned(value)),
                    None => referenced(token.as_str()),
                }
            }
        }
    }

    fn token(&mut self) -> Result<Token<'a, 'ctx>, ExpressionFailure> {
        let _depth = self.ctx.enter_nested("parse SLDPRT parameter token")?;
        self.ctx.charge_work(1, "parse SLDPRT parameter token")?;
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
            let Token::Bare(value) = self.token()? else {
                return Err(ExpressionFailure::NoValue);
            };
            let (text, reservation) = self.ctx.format_scoped(
                format_args!("{prefix}{}", value.as_str()),
                "normalize SLDPRT parameter token",
            )?;
            return Ok(Token::Bare(ParameterTokenText::Owned {
                value: text,
                _reservation: reservation,
            }));
        }
        if rest.starts_with('"') {
            self.offset += 1;
            let start = self.offset;
            let mut end = start;
            let mut closed = false;
            let mut characters = self.input[start..].char_indices();
            while let Some((at, character)) = self
                .ctx
                .next_charged(&mut characters, "scan SLDPRT quoted parameter token")?
            {
                end = start + at;
                if character == '"' {
                    if self.input[end + 1..].starts_with('"') {
                        self.ctx
                            .next_charged(&mut characters, "scan SLDPRT quoted parameter token")?;
                        end += 2;
                    } else {
                        closed = true;
                        break;
                    }
                } else {
                    end += character.len_utf8();
                }
            }
            if !closed {
                self.offset = end;
                return Err(ExpressionFailure::NoValue);
            }
            self.offset = end + 1;
            let identifier =
                super::ExpressionIdentifier::quoted(self.ctx, self.input, start - 1, self.offset)?
                    .ok_or(ExpressionFailure::NoValue)?;
            return Ok(Token::Quoted(identifier.value));
        }
        let start = self.offset;
        let numeric = self.input[start..]
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit() || character == '.');
        let mut characters = self.input[start..].char_indices();
        while let Some((at, character)) = self
            .ctx
            .next_charged(&mut characters, "scan SLDPRT parameter token")?
        {
            self.offset = start + at;
            let exponent_sign = numeric
                && matches!(character, '+' | '-')
                && self.input[start..self.offset].ends_with(['e', 'E']);
            if character.is_whitespace() || (!exponent_sign && "+-*/^(),=<>".contains(character)) {
                break;
            }
            self.offset += character.len_utf8();
        }
        if self.offset == start {
            return Err(ExpressionFailure::NoValue);
        }
        Ok(Token::Bare(ParameterTokenText::Borrowed(
            &self.input[start..self.offset],
        )))
    }

    fn skip_space(&mut self) -> Result<(), ExpressionFailure> {
        let rest = &self.input[self.offset..];
        let end = self
            .ctx
            .find_map(
                rest.char_indices(),
                |(at, character)| Ok((!character.is_whitespace()).then_some(at)),
                "scan SLDPRT parameter whitespace",
            )?
            .unwrap_or(rest.len());
        self.offset += end;
        Ok(())
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
    left: &ParameterValue,
    right: &ParameterValue,
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
                *right
            };
            ParameterValue::Integer(left.checked_add(right)?)
        }
        (left, right) => ParameterValue::Real(FiniteReal::new(
            real_parameter_value(left)? + sign * real_parameter_value(right)?,
        )?),
    })
}

pub(super) fn compare_parameter_values(
    ctx: &DecodeContext<'_>,
    left: &ParameterValue,
    right: &ParameterValue,
    operator: &str,
) -> Result<Option<bool>, CodecError> {
    if matches!(
        (left, right),
        (ParameterValue::Boolean(_), ParameterValue::Boolean(_))
    ) && !matches!(operator, "=" | "<>")
    {
        return Ok(None);
    }
    let ordering = match (left, right) {
        (ParameterValue::Length(left), ParameterValue::Length(right)) => left.partial_cmp(right),
        (ParameterValue::Angle(left), ParameterValue::Angle(right)) => left.partial_cmp(right),
        (ParameterValue::Real(left), ParameterValue::Real(right)) => left.partial_cmp(right),
        (ParameterValue::Integer(left), ParameterValue::Integer(right)) => Some(left.cmp(right)),
        (ParameterValue::Real(left), ParameterValue::Integer(right)) => {
            compare_integer_real(*right, left.get()).map(std::cmp::Ordering::reverse)
        }
        (ParameterValue::Integer(left), ParameterValue::Real(right)) => {
            compare_integer_real(*left, right.get())
        }
        (ParameterValue::Boolean(left), ParameterValue::Boolean(right)) => Some(left.cmp(right)),
        (ParameterValue::String(left), ParameterValue::String(right)) => Some(ctx.compare(
            left.as_str(),
            right.as_str(),
            "compare SLDPRT parameter text",
        )?),
        _ => return Ok(None),
    };
    let Some(ordering) = ordering else {
        return Ok(None);
    };
    Ok(Some(match operator {
        "=" => ordering.is_eq(),
        "<>" => !ordering.is_eq(),
        "<" => ordering.is_lt(),
        ">" => ordering.is_gt(),
        "<=" => !ordering.is_gt(),
        ">=" => !ordering.is_lt(),
        _ => return Ok(None),
    }))
}

fn compare_integer_real(integer: i64, real: f64) -> Option<std::cmp::Ordering> {
    if real.is_nan() {
        return None;
    }
    let Some(truncated) = truncate_f64_to_i64(real) else {
        // The truncation lies outside the `i64` range: below it for a negative
        // real, above it for a non-negative one.
        return Some(if real < 0.0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Less
        });
    };
    match integer.cmp(&truncated) {
        std::cmp::Ordering::Equal => 0.0f64.partial_cmp(&real.fract()),
        ordering => Some(ordering),
    }
}

fn multiply_parameter_values(
    left: &ParameterValue,
    right: &ParameterValue,
    divide: bool,
) -> Option<ParameterValue> {
    if divide && parameter_numeric_value(right)? == 0.0 {
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
                left.get() / real_parameter_value(right)?
            } else {
                left.get() * real_parameter_value(right)?
            })?))
        }
        (ParameterValue::Angle(left), right) => {
            Some(ParameterValue::Angle(Angle::new(if divide {
                left.get() / real_parameter_value(right)?
            } else {
                left.get() * real_parameter_value(right)?
            })?))
        }
        (left, ParameterValue::Length(right)) if !divide => Some(ParameterValue::Length(
            Length::new(real_parameter_value(left)? * right.get())?,
        )),
        (left, ParameterValue::Angle(right)) if !divide => Some(ParameterValue::Angle(Angle::new(
            real_parameter_value(left)? * right.get(),
        )?)),
        (ParameterValue::Integer(left), ParameterValue::Integer(right)) if !divide => {
            Some(ParameterValue::Integer(left.checked_mul(*right)?))
        }
        (left, right) => Some(ParameterValue::Real(FiniteReal::new(if divide {
            real_parameter_value(left)? / real_parameter_value(right)?
        } else {
            real_parameter_value(left)? * real_parameter_value(right)?
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
        )?)?));
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
            let integer_exponent = if exponent.fract() == 0.0 {
                truncate_f64_to_u32(exponent)
            } else {
                None
            };
            if let Some(power) = integer_exponent {
                ParameterValue::Integer(base.checked_pow(power)?)
            } else {
                ParameterValue::Real(FiniteReal::new(exact_integer_f64(*base)?.powf(exponent))?)
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

fn integer_power_real(base: i64, exponent: i64) -> Option<f64> {
    let mut exponent = exponent.unsigned_abs();
    let mut factor = exact_integer_f64(base)?;
    let mut value = 1.0;
    while exponent != 0 {
        if exponent & 1 != 0 {
            value *= factor;
        }
        exponent >>= 1;
        factor *= factor;
    }
    Some(value.recip())
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

    pub(super) fn apply<'a>(
        self,
        arguments: Vec<Cow<'a, ParameterValue>>,
    ) -> Option<Cow<'a, ParameterValue>> {
        if let Self::Iif = self {
            let [condition, when_true, when_false]: [Cow<'a, ParameterValue>; 3] =
                arguments.try_into().ok()?;
            let ParameterValue::Boolean(condition) = condition.as_ref() else {
                return None;
            };
            return match (when_true.as_ref(), when_false.as_ref()) {
                (ParameterValue::Length(_), ParameterValue::Length(_))
                | (ParameterValue::Angle(_), ParameterValue::Angle(_))
                | (ParameterValue::Real(_), ParameterValue::Real(_))
                | (ParameterValue::Integer(_), ParameterValue::Integer(_))
                | (ParameterValue::Boolean(_), ParameterValue::Boolean(_))
                | (ParameterValue::String(_), ParameterValue::String(_)) => {
                    Some(if *condition { when_true } else { when_false })
                }
                (ParameterValue::Real(_), ParameterValue::Integer(_))
                | (ParameterValue::Integer(_), ParameterValue::Real(_)) => Some(Cow::Owned(
                    ParameterValue::Real(FiniteReal::new(real_parameter_value(if *condition {
                        when_true.as_ref()
                    } else {
                        when_false.as_ref()
                    })?)?),
                )),
                _ => None,
            };
        }
        let arguments = arguments.as_slice();
        let unary = || {
            let [argument] = arguments else {
                return None;
            };
            Some(argument.as_ref())
        };
        let angle = || {
            let ParameterValue::Angle(value) = unary()? else {
                return None;
            };
            Some(value.get())
        };
        Some(Cow::Owned(match self {
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
                    ParameterValue::Integer(truncate_f64_to_i64(value)?)
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
        }))
    }
}

fn real_parameter_value(value: &ParameterValue) -> Option<f64> {
    match value {
        ParameterValue::Real(value) => Some(value.get()),
        ParameterValue::Integer(value) => exact_integer_f64(*value),
        _ => None,
    }
}

fn parameter_numeric_value(value: &ParameterValue) -> Option<f64> {
    match value {
        ParameterValue::Length(value) => Some(value.get()),
        ParameterValue::Angle(value) => Some(value.get()),
        ParameterValue::Real(value) => Some(value.get()),
        ParameterValue::Integer(value) => exact_integer_f64(*value),
        ParameterValue::Boolean(_) | ParameterValue::String(_) => None,
    }
}

/// Convert a discrete integer to a native scalar without changing its value.
pub(crate) fn exact_integer_f64(value: i64) -> Option<f64> {
    f64_from_i64(value)
}

#[cfg(test)]
mod tests {
    use super::{
        add_parameter_values, exponentiate_parameter_value, multiply_parameter_values,
        ParameterFunction,
    };
    use cadmpeg_ir::{features::ParameterValue, scalar::FiniteReal};
    use std::borrow::Cow;

    #[test]
    fn real_arithmetic_rejects_non_finite_results_at_construction() {
        let largest = ParameterValue::Real(FiniteReal::new(f64::MAX).unwrap());
        let two = ParameterValue::Integer(2);
        assert!(add_parameter_values(&largest, &largest, false).is_none());
        assert!(multiply_parameter_values(&largest, &two, false).is_none());
        assert!(exponentiate_parameter_value(&largest, &two).is_none());
        assert!(ParameterFunction::Exp
            .apply(vec![Cow::Owned(largest)])
            .is_none());
        let negative = ParameterValue::Real(FiniteReal::new(-1.0).unwrap());
        assert!(ParameterFunction::Log
            .apply(vec![Cow::Owned(negative.clone())])
            .is_none());
        assert!(ParameterFunction::Sqr
            .apply(vec![Cow::Owned(negative)])
            .is_none());
    }
}
