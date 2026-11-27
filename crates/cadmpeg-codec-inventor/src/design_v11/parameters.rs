// SPDX-License-Identifier: Apache-2.0
//! Part parameters and their stored expressions.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{unique_index, DecodeContext};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{
    self, Expression, ExpressionDialect, ExpressionToken, Neutral, Node,
};
use cadmpeg_ir::design::parameter::{
    DesignParameter, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::units::{Quantity, Unit, ValueKind};
use cadmpeg_ir::features::parameters::{
    DesignParameter as LegacyParameter, ParameterValue as LegacyValue,
};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::CadIr;

use crate::design::{PmDcParameter, PmDcUnit, PmDcUnitDimension, PmDcUnitKind};

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    parameters: &[PmDcParameter],
    unit_records: &[PmDcUnit],
) -> Result<(), CodecError> {
    let Some(part) = ir.model.product_definitions.first() else {
        return Ok(());
    };
    let units = unique_index(unit_records.iter().map(|unit| {
        (
            (
                unit.identity.segment_token.as_str(),
                unit.identity.record_ordinal,
            ),
            unit,
        )
    }));
    let raw = unique_index(
        parameters
            .iter()
            .map(|parameter| (parameter.id(), parameter)),
    );
    let by_id = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter))
        .collect::<BTreeMap<_, _>>();
    let kinds = ir
        .model
        .parameters
        .iter()
        .map(|parameter| {
            (
                parameter.id.clone(),
                value(parameter.value.as_ref(), None).0,
            )
        })
        .collect();
    for parameter in &ir.model.parameters {
        ctx.charge_entities(1, "project Inventor v11 parameter")?;
        let declared = parameter
            .native_ref
            .as_ref()
            .and_then(|id| raw.get(id))
            .and_then(Option::as_ref)
            .and_then(|parameter| {
                let token = parameter.identity.segment_token.as_str();
                let definition = units
                    .get(&(token, parameter.unit.record_ordinal()?))?
                    .as_ref()?;
                let PmDcUnitKind::Definition {
                    numerators,
                    denominators,
                    derived,
                    ..
                } = &definition.kind
                else {
                    return None;
                };
                let [base] = numerators.references() else {
                    return None;
                };
                if !denominators.references().is_empty() || derived.index() != 0 {
                    return None;
                }
                let base = units.get(&(token, base.record_ordinal()?))?.as_ref()?;
                let PmDcUnitKind::Base {
                    dimension, symbol, ..
                } = &base.kind
                else {
                    return None;
                };
                Some((*dimension, symbol.as_str()))
            });
        let (kind, value) = value(
            parameter.value.as_ref(),
            declared.map(|(dimension, _)| dimension),
        );
        let unit = declared.and_then(|(_, symbol)| unit(symbol));
        let definition =
            ParameterDefinition::Expression(expression(parameter, &by_id, &kinds, &kind));
        ir.design.parameters.push(DesignParameter {
            id: parameter.id.clone(),
            scope: part.id.clone(),
            owner: parameter.owner.clone().map(ParameterOwner::Feature),
            position: None,
            name: NonBlankString::new(parameter.name.clone()),
            kind,
            unit,
            definition,
            value,
            display: None,
            tolerance: None,
            decimals: None,
            comment: None,
            native_options: BTreeMap::default(),
            source_settings: BTreeMap::default(),
            native_ref: parameter.native_ref.clone(),
        });
    }
    ir.design.parameters.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}

fn unit(symbol: &str) -> Option<Unit> {
    if symbol.is_empty() || symbol == "ul" {
        return Some(Unit::default());
    }
    match expr::parse::<ParameterRef>(&format!("1[{symbol}]"), &[], &[]).ok()? {
        Node::Number { ref unit, .. } => Some(unit.clone()),
        _ => None,
    }
}

fn value(
    value: Option<&LegacyValue>,
    declared: Option<PmDcUnitDimension>,
) -> (ValueKind, Option<ParameterValue>) {
    let (quantity, number) = match value {
        Some(LegacyValue::Length(value)) => (Quantity::LENGTH, FiniteReal::new(value.get())),
        Some(LegacyValue::Angle(value)) => (Quantity::ANGLE, FiniteReal::new(value.get())),
        Some(LegacyValue::Real(value)) => (Quantity::REAL, Some(*value)),
        Some(LegacyValue::Integer(value)) => {
            return (ValueKind::Integer, Some(ParameterValue::Integer(*value)))
        }
        Some(LegacyValue::Boolean(value)) => {
            return (ValueKind::Boolean, Some(ParameterValue::Boolean(*value)))
        }
        Some(LegacyValue::String(value)) => {
            return (ValueKind::Text, Some(ParameterValue::Text(value.clone())))
        }
        None => (
            match declared {
                Some(PmDcUnitDimension::Length) => Quantity::LENGTH,
                Some(PmDcUnitDimension::Angle) => Quantity::ANGLE,
                _ => Quantity::REAL,
            },
            None,
        ),
    };
    (
        ValueKind::Quantity(quantity),
        number.map(ParameterValue::Number),
    )
}

fn expression(
    parameter: &LegacyParameter,
    parameters: &BTreeMap<cadmpeg_ir::features::ParameterId, &LegacyParameter>,
    kinds: &BTreeMap<cadmpeg_ir::features::ParameterId, ValueKind>,
    kind: &ValueKind,
) -> Expression {
    let names = unique_index(
        parameter
            .dependencies
            .iter()
            .filter_map(|id| parameters.get(id))
            .map(|target| (target.name.as_str(), *target)),
    );
    let text = parameter.expression.as_str();
    let mut tokens = Vec::new();
    let mut neutral = String::new();
    let mut refs = Vec::new();
    let mut cursor = 0;
    let mut text_start = 0;
    let mut previous_number = false;
    while cursor < text.len() {
        let start = cursor;
        let Some(ch) = text[cursor..].chars().next() else {
            break;
        };
        cursor += ch.len_utf8();
        if ch.is_ascii_digit() || ch == '.' {
            while cursor < text.len()
                && (text.as_bytes()[cursor].is_ascii_digit() || text.as_bytes()[cursor] == b'.')
            {
                cursor += 1;
            }
            if matches!(text.as_bytes().get(cursor), Some(b'e' | b'E')) {
                cursor += 1;
                if matches!(text.as_bytes().get(cursor), Some(b'+' | b'-')) {
                    cursor += 1;
                }
                while text.as_bytes().get(cursor).is_some_and(u8::is_ascii_digit) {
                    cursor += 1;
                }
            }
            neutral.push_str(&text[start..cursor]);
            previous_number = true;
        } else if ch.is_alphabetic() || ch == '_' {
            while cursor < text.len() {
                let Some(ch) = text[cursor..].chars().next() else {
                    break;
                };
                if !ch.is_alphanumeric() && ch != '_' {
                    break;
                }
                cursor += ch.len_utf8();
            }
            let word = &text[start..cursor];
            if let Some(Some(target)) = names.get(word) {
                let reference = ParameterRef {
                    target: RefTarget::Parameter(target.id.clone()),
                    through: None,
                };
                if start > text_start {
                    tokens.push(ExpressionToken::Text(text[text_start..start].into()));
                }
                tokens.push(ExpressionToken::Reference(reference.clone()));
                text_start = cursor;
                neutral.push('$');
                neutral.push_str(&refs.len().to_string());
                refs.push(reference);
            } else if previous_number && unit(word).is_some() {
                if word != "ul" {
                    neutral.push('[');
                    neutral.push_str(word);
                    neutral.push(']');
                }
            } else {
                neutral.push_str(word);
            }
            previous_number = false;
        } else {
            neutral.push(ch);
            if !ch.is_whitespace() {
                previous_number = false;
            }
        }
    }
    if text_start < text.len() {
        tokens.push(ExpressionToken::Text(text[text_start..].into()));
    }
    if let Ok(mut tree) = expr::parse(&neutral, &refs, &[]) {
        if normalize(&mut tree, kinds).as_ref() == Some(kind) {
            return Expression::Neutral(Neutral(tree));
        }
    }
    Expression::Source {
        dialect: ExpressionDialect::Inventor,
        tokens,
    }
}

fn normalize(
    tree: &mut Node<ParameterRef>,
    kinds: &BTreeMap<cadmpeg_ir::features::ParameterId, ValueKind>,
) -> Option<ValueKind> {
    use cadmpeg_ir::design::expr::{BinaryOp, Function, MulOp, UnaryOp};
    match tree {
        Node::Number { unit, .. } => {
            let mut quantity = Quantity::REAL;
            for factor in &unit.0 {
                let slot = match factor.symbol.name() {
                    "rad" | "deg" | "grad" | "arcmin" | "arcsec" | "rev" => &mut quantity.angle,
                    "s" | "min" | "h" => &mut quantity.time,
                    "g" | "kg" | "lb" => &mut quantity.mass,
                    _ => &mut quantity.length,
                };
                *slot = slot.checked_add(factor.power.get())?;
            }
            Some(ValueKind::Quantity(quantity))
        }
        Node::Reference(ParameterRef {
            target: RefTarget::Parameter(id),
            ..
        }) => kinds.get(id).cloned(),
        Node::Constant(_) => Some(ValueKind::Quantity(Quantity::REAL)),
        Node::Unary {
            op: UnaryOp::Negate,
            operand,
        } => normalize(operand, kinds),
        Node::Binary { op, left, right } => {
            let ValueKind::Quantity(a) = normalize(left, kinds)? else {
                return None;
            };
            let ValueKind::Quantity(b) = normalize(right, kinds)? else {
                return None;
            };
            let quantity = match op {
                BinaryOp::Power if a == Quantity::REAL && b == Quantity::REAL => Quantity::REAL,
                BinaryOp::Power if b == Quantity::REAL => {
                    let exponent = literal_number(right)?;
                    let scale = |power: i8| {
                        let scaled = f64::from(power) * exponent;
                        if scaled.fract() != 0.0 {
                            return None;
                        }
                        i8::try_from(cadmpeg_core::convert::truncate_f64_to_i32(scaled)?).ok()
                    };
                    Quantity {
                        length: scale(a.length)?,
                        mass: scale(a.mass)?,
                        angle: scale(a.angle)?,
                        time: scale(a.time)?,
                    }
                }
                _ => return None,
            };
            Some(ValueKind::Quantity(quantity))
        }
        Node::Sum { first, rest } => {
            let ValueKind::Quantity(a) = normalize(first, kinds)? else {
                return None;
            };
            for (_, operand) in rest.iter_mut() {
                let ValueKind::Quantity(b) = normalize(operand, kinds)? else {
                    return None;
                };
                (a == b).then_some(())?;
            }
            Some(ValueKind::Quantity(a))
        }
        Node::Product { first, rest } => {
            let ValueKind::Quantity(mut held) = normalize(first, kinds)? else {
                return None;
            };
            for (op, operand) in rest.iter_mut() {
                let ValueKind::Quantity(b) = normalize(operand, kinds)? else {
                    return None;
                };
                let combine = |a: i8, b: i8| match op {
                    MulOp::Multiply => a.checked_add(b),
                    MulOp::Divide => a.checked_sub(b),
                };
                held = Quantity {
                    length: combine(held.length, b.length)?,
                    mass: combine(held.mass, b.mass)?,
                    angle: combine(held.angle, b.angle)?,
                    time: combine(held.time, b.time)?,
                };
            }
            Some(ValueKind::Quantity(held))
        }
        Node::Call {
            function: Function::Sin | Function::Cos | Function::Tan,
            args,
        } => {
            let [argument] = args.as_mut_slice() else {
                return None;
            };
            let quantity = normalize(argument, kinds)?;
            if quantity != ValueKind::Quantity(Quantity::ANGLE) {
                return None;
            }
            Some(ValueKind::Quantity(Quantity::REAL))
        }
        _ => None,
    }
}

fn literal_number(tree: &Node<ParameterRef>) -> Option<f64> {
    match tree {
        Node::Number { value, unit } if unit.0.is_empty() => Some(value.get()),
        Node::Unary {
            op: expr::UnaryOp::Negate,
            operand,
        } => literal_number(operand).map(|value| -value),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
