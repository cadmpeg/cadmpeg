// SPDX-License-Identifier: Apache-2.0
//! Part parameters and their formulas (§5.12, §5.13).
//!
//! The decode report names, in one `design.parameter-reference-as-text`
//! note, each parameter whose expression holds no reference to an input the
//! decoder resolved for it, with the inputs it loses.

use std::collections::BTreeMap;
use std::num::NonZeroI8;

use cadmpeg_core::decode::{unique_index, DecodeContext};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::edit::{evaluate, Value};
use cadmpeg_ir::design::expr::{
    self, Expression, ExpressionDialect, ExpressionToken, Neutral, Node, UnaryOp,
};
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
use cadmpeg_ir::design::projection::fields::{ModelParameterField, ParameterField};
use cadmpeg_ir::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, ValueKind};
use cadmpeg_ir::features::parameters::{
    DesignParameter as LegacyParameter, ParameterValue as LegacyValue,
};
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};
use cadmpeg_ir::CadIr;

use crate::loss::{identity_statement, CatiaLossCode};

/// The relative difference within which a formula's tree reproduces the
/// value the source stores for the formula.
const STORED_VALUE_AGREEMENT: f64 = 1e-9;

/// Restate every user parameter the decoder transferred as a parameter of
/// `scope`, and add to `losses` the note naming those that drop an input.
/// The decoder files no CATIA parameter under a feature; one it did would
/// need its owner stated, and is left out.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    scope: &ProductDefinitionId,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let user = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.is_none())
        .map(|parameter| (&parameter.id, parameter))
        .collect::<BTreeMap<_, _>>();
    let mut projected = Vec::with_capacity(user.len());
    let mut dropped = Vec::new();
    for parameter in user.values() {
        ctx.charge_entities(1, "project a CATIA v11 parameter")?;
        let kind = kind(parameter)?;
        let expression = expression(parameter, &user, &kind);
        let lost = dropped_inputs(parameter, &expression);
        if !lost.is_empty() {
            dropped.push(format!(
                "{} ({})",
                parameter.id,
                lost.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        projected.push(DesignParameter {
            id: parameter.id.clone(),
            scope: scope.clone(),
            owner: None,
            name: NonBlankString::new(parameter.name.clone()),
            value: value(parameter.value.as_ref(), &kind),
            definition: ParameterDefinition::Expression(expression),
            kind,
            unit: None,
            display: None,
            tolerance: None,
            decimals: None,
            comment: None,
            native_options: BTreeMap::default(),
            source_settings: BTreeMap::default(),
            native_ref: parameter.native_ref.clone(),
        });
    }
    if !dropped.is_empty() {
        losses.push(CatiaLossCode::DesignParameterReferenceAsText.note(format!(
            "{} parameters hold an input parameter the decoder resolved as text, or omit it, \
             and state no dependency on that parameter (each with the inputs it loses): {}",
            dropped.len(),
            identity_statement(&dropped)
        )));
    }
    ir.design.parameters.extend(projected);
    ir.design.parameters.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}

/// The inputs the decoder resolved for `parameter` that `expression` holds no
/// reference to.
fn dropped_inputs<'a>(
    parameter: &'a LegacyParameter,
    expression: &Expression,
) -> Vec<&'a ParameterId> {
    let held = match expression {
        Expression::Neutral(Neutral(tree)) => {
            expr::print(tree).map_or_else(|_| Vec::new(), |wire| wire.refs)
        }
        Expression::Source { tokens, .. } => tokens
            .iter()
            .filter_map(|token| match token {
                ExpressionToken::Reference(reference) => Some(reference.clone()),
                ExpressionToken::Text(_) => None,
            })
            .collect(),
    };
    parameter
        .dependencies
        .iter()
        .filter(|input| {
            !held.iter().any(
                |reference| matches!(&reference.target, RefTarget::Parameter(id) if id == *input),
            )
        })
        .collect()
}

/// The model parameter fields [`project`] reads to build `field`. The kind
/// is the signature's `value_type` property; a literal's tree is its stored
/// value where the stored expression restates that value.
#[deny(clippy::wildcard_enum_match_arm)]
pub(super) fn inputs(field: ParameterField) -> &'static [ModelParameterField] {
    match field {
        ParameterField::Name => &[ModelParameterField::Name],
        ParameterField::Kind => &[ModelParameterField::Properties],
        ParameterField::Definition => &[
            ModelParameterField::ExpressionText,
            ModelParameterField::Dependencies,
            ModelParameterField::Value,
            ModelParameterField::Properties,
        ],
        ParameterField::Value => &[ModelParameterField::Value, ModelParameterField::Properties],
        ParameterField::NativeRef => &[ModelParameterField::NativeRef],
        ParameterField::Scope
        | ParameterField::Owner
        | ParameterField::Unit
        | ParameterField::Display
        | ParameterField::Tolerance
        | ParameterField::Decimals
        | ParameterField::Comment
        | ParameterField::NativeOptions
        | ParameterField::SourceSettings => &[],
    }
}

/// The projected parameters whose signature declares no kind.
pub(super) fn defaulted_kinds(ir: &CadIr) -> Vec<&ParameterId> {
    let projected = ir
        .design
        .parameters
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    ir.model
        .parameters
        .iter()
        .filter(|parameter| {
            projected.contains(parameter.id.as_str()) && property(parameter, "value_type").is_none()
        })
        .map(|parameter| &parameter.id)
        .collect()
}

fn property<'a>(parameter: &'a LegacyParameter, key: &str) -> Option<&'a str> {
    parameter
        .properties
        .iter()
        .find(|(name, _)| name.as_str() == key)
        .map(|(_, value)| value.as_str())
}

/// The quantity the parameter's CATIA signature declares.
fn kind(parameter: &LegacyParameter) -> Result<ValueKind, CodecError> {
    Ok(match property(parameter, "value_type") {
        Some("LENGTH") => ValueKind::Quantity(Quantity::LENGTH),
        Some("ANGLE") => ValueKind::Quantity(Quantity::ANGLE),
        Some("Real" | "R") | None => ValueKind::Quantity(Quantity::REAL),
        Some("Integer" | "I") => ValueKind::Integer,
        Some("Boolean") => ValueKind::Boolean,
        Some("String") => ValueKind::Text,
        Some(other) => ValueKind::Native(
            NonBlankString::new(format!("catia:{other}"))
                .ok_or_else(|| CodecError::malformed("CATIA v11 parameter kind is blank"))?,
        ),
    })
}

/// The stored value, where its variant is the one the kind declares.
fn value(value: Option<&LegacyValue>, kind: &ValueKind) -> Option<ParameterValue> {
    match (kind, value?) {
        (ValueKind::Quantity(quantity), LegacyValue::Length(value))
            if *quantity == Quantity::LENGTH =>
        {
            FiniteReal::new(value.get()).map(ParameterValue::Number)
        }
        (ValueKind::Quantity(quantity), LegacyValue::Angle(value))
            if *quantity == Quantity::ANGLE =>
        {
            FiniteReal::new(value.get()).map(ParameterValue::Number)
        }
        (ValueKind::Quantity(quantity), LegacyValue::Real(value))
            if *quantity == Quantity::REAL =>
        {
            Some(ParameterValue::Number(*value))
        }
        (ValueKind::Integer, LegacyValue::Integer(value)) => Some(ParameterValue::Integer(*value)),
        (ValueKind::Boolean, LegacyValue::Boolean(value)) => Some(ParameterValue::Boolean(*value)),
        (ValueKind::Text, LegacyValue::String(value)) => Some(ParameterValue::Text(value.clone())),
        _ => None,
    }
}

/// A literal parameter holds its stored value as a one-literal expression;
/// a formula output holds its formula text.
fn expression(
    parameter: &LegacyParameter,
    parameters: &BTreeMap<&ParameterId, &LegacyParameter>,
    kind: &ValueKind,
) -> Expression {
    let stored = parameter.value.as_ref();
    if parameter.dependencies.is_empty() {
        if let Some(value) = stored {
            if crate::formula::parameter_expression(value) == parameter.expression {
                if let Some(literal) = literal(value) {
                    return Expression::Neutral(Neutral(literal));
                }
            }
        }
    }
    formula(parameter, parameters, kind)
}

fn literal(value: &LegacyValue) -> Option<Node<ParameterRef>> {
    match value {
        LegacyValue::Length(value) => number(value.get(), Some(UnitSymbol::Mm)),
        LegacyValue::Angle(value) => number(value.get(), Some(UnitSymbol::Rad)),
        LegacyValue::Real(value) => number(value.get(), None),
        LegacyValue::Integer(value) => number(f64::from(i32::try_from(*value).ok()?), None),
        LegacyValue::Boolean(value) => Some(Node::Boolean(*value)),
        LegacyValue::String(value) => Some(Node::Text(value.clone())),
    }
}

fn number(value: f64, unit: Option<UnitSymbol>) -> Option<Node<ParameterRef>> {
    let power = NonZeroI8::new(1)?;
    let node = Node::Number {
        value: NonNegativeReal::new(value.abs())?,
        unit: Unit(
            unit.map(|symbol| UnitFactor { symbol, power })
                .into_iter()
                .collect(),
        ),
    };
    Some(if value < 0.0 {
        Node::Unary {
            op: UnaryOp::Negate,
            operand: Box::new(node),
        }
    } else {
        node
    })
}

/// The neutral unit symbol of a CATIA formula unit word.
fn unit_symbol(word: &str) -> Option<&'static str> {
    Some(match word {
        "micron" => "um",
        "mm" => "mm",
        "cm" => "cm",
        "m" => "m",
        "km" => "km",
        "in" => "in",
        "ft" => "ft",
        "yard" => "yd",
        "mile" => "mi",
        "rad" => "rad",
        "grad" => "grad",
        "deg" => "deg",
        _ => return None,
    })
}

/// The end of a `#N_` binding symbol that starts at `start`.
fn symbol_end(bytes: &[u8], start: usize) -> Option<usize> {
    (bytes.get(start) == Some(&b'#')).then_some(())?;
    let digits = start + 1;
    let mut at = digits;
    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    (at > digits && bytes.get(at) == Some(&b'_')).then_some(at + 1)
}

/// The end of a binding's optional ` /N` ordinal suffix after `name_end`.
fn ordinal_end(bytes: &[u8], name_end: usize) -> usize {
    let mut at = name_end;
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    if bytes.get(at) != Some(&b'/') {
        return name_end;
    }
    let digits = at + 1;
    let mut end = digits;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if end > digits {
        end
    } else {
        name_end
    }
}

/// The end of a number literal that starts at `start`, with its exponent.
fn number_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut at = start;
    let mut digit = false;
    while let Some(byte) = bytes.get(at) {
        match byte {
            b'0'..=b'9' => digit = true,
            b'.' => {}
            _ => break,
        }
        at += 1;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let exponent = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        (at > exponent).then_some(())?;
    }
    digit.then_some(at)
}

/// A formula's text with each binding of an input the decoder resolved as a
/// reference. It is neutral where its constructs are numbers, units, the four
/// arithmetic operators, and parentheses, and the tree evaluates to the
/// declared kind and to the stored value where every input stores one.
fn formula(
    parameter: &LegacyParameter,
    parameters: &BTreeMap<&ParameterId, &LegacyParameter>,
    kind: &ValueKind,
) -> Expression {
    let inputs = parameter
        .dependencies
        .iter()
        .filter_map(|id| parameters.get(id).copied())
        .collect::<Vec<_>>();
    let symbols = unique_index(inputs.iter().filter_map(|input| {
        let binding = property(input, "catia_binding")?;
        let end = symbol_end(binding.as_bytes(), 0)?;
        Some((binding.get(..end)?, *input))
    }));
    let text = parameter.expression.as_str();
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut refs = Vec::new();
    let mut neutral = Some(String::new());
    let mut text_start = 0;
    let mut at = 0;
    while at < bytes.len() {
        if let Some(name_end) = symbol_end(bytes, at) {
            let end = ordinal_end(bytes, name_end);
            if let Some(Some(input)) = text.get(at..name_end).and_then(|name| symbols.get(name)) {
                if let Some(before) = text.get(text_start..at).filter(|before| !before.is_empty()) {
                    tokens.push(ExpressionToken::Text(before.to_owned()));
                }
                let reference = ParameterRef {
                    target: RefTarget::Parameter(input.id.clone()),
                    through: None,
                };
                tokens.push(ExpressionToken::Reference(reference.clone()));
                if let Some(neutral) = neutral.as_mut() {
                    neutral.push('$');
                    neutral.push_str(&refs.len().to_string());
                }
                refs.push(reference);
                text_start = end;
            } else {
                neutral = None;
            }
            at = end;
            continue;
        }
        let byte = bytes[at];
        if byte.is_ascii_digit() || byte == b'.' {
            let Some(end) = number_end(bytes, at) else {
                neutral = None;
                at += 1;
                continue;
            };
            let mut word_start = end;
            while bytes.get(word_start).is_some_and(u8::is_ascii_whitespace) {
                word_start += 1;
            }
            let mut word_end = word_start;
            while bytes.get(word_end).is_some_and(u8::is_ascii_alphabetic) {
                word_end += 1;
            }
            let value = text
                .get(at..end)
                .and_then(|digits| digits.parse::<f64>().ok());
            let unit = text
                .get(word_start..word_end)
                .filter(|word| !word.is_empty());
            match (neutral.as_mut(), value) {
                (Some(text), Some(value)) if value.is_finite() => {
                    text.push_str(&value.to_string());
                    match unit {
                        None => {}
                        Some(word) => match unit_symbol(word) {
                            Some(symbol) => {
                                text.push('[');
                                text.push_str(symbol);
                                text.push(']');
                            }
                            None => neutral = None,
                        },
                    }
                }
                _ => neutral = None,
            }
            at = if unit.is_some() { word_end } else { end };
            continue;
        }
        match (byte, neutral.as_mut()) {
            (b'+' | b'-' | b'*' | b'/' | b'(' | b')' | b' ' | b'\t', Some(text)) => {
                text.push(char::from(byte));
            }
            _ => neutral = None,
        }
        at += 1;
    }
    if let Some(rest) = text.get(text_start..).filter(|rest| !rest.is_empty()) {
        tokens.push(ExpressionToken::Text(rest.to_owned()));
    }
    if let Some(tree) = neutral
        .and_then(|neutral| expr::parse(&neutral, &refs, &[]).ok())
        .filter(|tree| agrees(tree, parameter, parameters, kind))
    {
        return Expression::Neutral(Neutral(tree));
    }
    if let [ExpressionToken::Reference(reference)] = tokens.as_slice() {
        return whole_reference(reference, text, kind, parameters);
    }
    Expression::Source {
        dialect: ExpressionDialect::Catia,
        tokens,
    }
}

/// A formula whose whole text is one resolved input of the same `Boolean`,
/// `Integer` or `Text` kind, which [`agrees`] cannot evaluate, is that
/// reference (§5.13: an integer expression is dimensionless and evaluates to
/// an integer). Any other such formula keeps its text whole: a `Source`
/// expression holds a `Text` token (part 10), and [`dropped_inputs`] names
/// the parameter.
fn whole_reference(
    reference: &ParameterRef,
    text: &str,
    kind: &ValueKind,
    parameters: &BTreeMap<&ParameterId, &LegacyParameter>,
) -> Expression {
    let input_kind = match &reference.target {
        RefTarget::Parameter(id) => parameters.get(id).copied().and_then(kind_of),
        RefTarget::External { .. } => None,
    };
    if matches!(
        kind,
        ValueKind::Boolean | ValueKind::Integer | ValueKind::Text
    ) && input_kind.as_ref() == Some(kind)
    {
        return Expression::Neutral(Neutral(Node::Reference(reference.clone())));
    }
    Expression::Source {
        dialect: ExpressionDialect::Catia,
        tokens: vec![ExpressionToken::Text(text.to_owned())],
    }
}

/// Whether `tree` evaluates to `kind`, and to the parameter's stored value
/// where every input stores a value.
fn agrees(
    tree: &Node<ParameterRef>,
    parameter: &LegacyParameter,
    parameters: &BTreeMap<&ParameterId, &LegacyParameter>,
    kind: &ValueKind,
) -> bool {
    let mut complete = true;
    let mut lookup = |reference: &ParameterRef| {
        let RefTarget::Parameter(id) = &reference.target else {
            return None;
        };
        let input = parameters.get(id)?;
        let kind = kind_of(input)?;
        let stored = value(input.value.as_ref(), &kind);
        if stored.is_none() {
            complete = false;
        }
        Some(match (kind, stored) {
            (ValueKind::Quantity(quantity), Some(ParameterValue::Number(value))) => Value::Number {
                value: value.get(),
                quantity,
            },
            (ValueKind::Quantity(quantity), _) => Value::Number {
                value: 1.0,
                quantity,
            },
            (ValueKind::Integer, Some(ParameterValue::Integer(value))) => Value::Number {
                value: f64::from(i32::try_from(value).ok()?),
                quantity: Quantity::REAL,
            },
            (ValueKind::Integer, _) => Value::Number {
                value: 1.0,
                quantity: Quantity::REAL,
            },
            _ => return None,
        })
    };
    let Ok(result) = evaluate(tree, &mut lookup) else {
        return false;
    };
    let Value::Number {
        value: result,
        quantity,
    } = result
    else {
        return false;
    };
    let ValueKind::Quantity(declared) = kind else {
        return false;
    };
    if quantity != *declared {
        return false;
    }
    match value(parameter.value.as_ref(), kind) {
        Some(ParameterValue::Number(stored)) if complete => {
            (result - stored.get()).abs() <= STORED_VALUE_AGREEMENT * stored.get().abs().max(1.0)
        }
        _ => true,
    }
}

fn kind_of(parameter: &LegacyParameter) -> Option<ValueKind> {
    kind(parameter).ok()
}

#[cfg(test)]
mod tests;
