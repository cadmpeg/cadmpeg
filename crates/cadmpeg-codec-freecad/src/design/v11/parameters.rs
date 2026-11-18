// SPDX-License-Identifier: Apache-2.0
//! The properties and sketch constraints of a Body's features, and the
//! properties of the Body, as v11 parameters.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{
    self, BinaryOp, Expression, ExpressionDialect, ExpressionToken, Neutral, Node, UnaryOp,
};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::parameter::{
    DesignParameter, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::units::{Quantity, ValueKind};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    DesignParameter as LegacyParameter, FeatureId, ParameterId, ParameterValue as LegacyValue,
};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::scalar::FiniteReal;

/// Relative agreement at which a stored angle's text is read in degrees or
/// in radians against its canonical value.
const ANGLE_TEXT_AGREEMENT: f64 = 1e-12;

/// Project each parameter the decoder read that a design feature or a Body
/// owns into the scope of `definition`. `containers` maps a Body's feature to
/// its container.
pub(super) fn project(
    ir: &mut CadIr,
    definition: &ProductDefinitionId,
    containers: &BTreeMap<FeatureId, ContainerId>,
) -> Result<(), CodecError> {
    let features = ir
        .design
        .features
        .iter()
        .map(|feature| &feature.id)
        .collect::<BTreeSet<_>>();
    let owners = ir
        .model
        .parameters
        .iter()
        .filter_map(|parameter| {
            let owner = parameter.owner.as_ref()?;
            let owner = containers.get(owner).map_or_else(
                || {
                    features
                        .contains(owner)
                        .then(|| ParameterOwner::Feature(owner.clone()))
                },
                |container| Some(ParameterOwner::Container(container.clone())),
            )?;
            Some((&parameter.id, owner))
        })
        .collect::<BTreeMap<_, _>>();
    let by_id = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| owners.contains_key(&parameter.id))
        .map(|parameter| (&parameter.id, parameter))
        .collect::<BTreeMap<_, _>>();
    let mut parameters = Vec::new();
    for (id, owner) in &owners {
        let parameter = by_id[id];
        let (kind, value) = kind_and_value(parameter.value.as_ref());
        let parameter_definition = if property(parameter, "is_driving") == Some("0") {
            ParameterDefinition::Driven
        } else {
            ParameterDefinition::Expression(expression(parameter, &by_id, &kind)?)
        };
        parameters.push(DesignParameter {
            id: parameter.id.clone(),
            scope: definition.clone(),
            owner: Some(owner.clone()),
            name: name(parameter),
            kind,
            unit: None,
            definition: parameter_definition,
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
    parameters.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.parameters = parameters;
    Ok(())
}

fn property<'a>(parameter: &'a LegacyParameter, key: &str) -> Option<&'a str> {
    parameter
        .properties
        .iter()
        .find(|(name, _)| name.as_str() == key)
        .map(|(_, value)| value.as_str())
}

/// The source's name of the parameter: a named constraint's name, or the
/// property's name.
fn name(parameter: &LegacyParameter) -> Option<NonBlankString> {
    let name = property(parameter, "source_name").unwrap_or(&parameter.name);
    NonBlankString::new(name.to_owned())
}

fn kind_and_value(value: Option<&LegacyValue>) -> (ValueKind, Option<ParameterValue>) {
    let quantity = |quantity, number: f64| {
        (
            ValueKind::Quantity(quantity),
            FiniteReal::new(number).map(ParameterValue::Number),
        )
    };
    match value {
        Some(LegacyValue::Length(value)) => quantity(Quantity::LENGTH, value.get()),
        Some(LegacyValue::Angle(value)) => quantity(Quantity::ANGLE, value.get()),
        Some(LegacyValue::Real(value)) => quantity(Quantity::REAL, value.get()),
        Some(LegacyValue::Integer(value)) => {
            (ValueKind::Integer, Some(ParameterValue::Integer(*value)))
        }
        Some(LegacyValue::Boolean(value)) => {
            (ValueKind::Boolean, Some(ParameterValue::Boolean(*value)))
        }
        Some(LegacyValue::String(value)) => {
            (ValueKind::Text, Some(ParameterValue::Text(value.clone())))
        }
        None => (ValueKind::Quantity(Quantity::REAL), None),
    }
}

/// The parameter's expression. A stored value is a one-literal expression in
/// the property's unit; a bound property holds the expression `FreeCAD` binds
/// to it.
fn expression(
    parameter: &LegacyParameter,
    parameters: &BTreeMap<&ParameterId, &LegacyParameter>,
    kind: &ValueKind,
) -> Result<Expression, CodecError> {
    let text = parameter.expression.as_str();
    if parameter.dependencies.is_empty() {
        if let Some(literal) = literal(text, parameter.value.as_ref()) {
            return Ok(Expression::Neutral(Neutral(literal)));
        }
    }
    translated(parameter, text, parameters, kind)
}

/// A stored value as a literal in the unit `FreeCAD` stores it in: millimetres
/// for a length, degrees or radians for an angle, whichever the stored text
/// agrees with, and no unit for a real.
fn literal(text: &str, value: Option<&LegacyValue>) -> Option<Node<ParameterRef>> {
    let stored = text
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())?;
    let unit = match value? {
        LegacyValue::Length(_) => "mm",
        LegacyValue::Angle(angle) => {
            let radians = angle.get();
            let tolerance = ANGLE_TEXT_AGREEMENT * radians.abs().max(1.0);
            if (stored.to_radians() - radians).abs() <= tolerance {
                "deg"
            } else if (stored - radians).abs() <= tolerance {
                "rad"
            } else {
                return None;
            }
        }
        LegacyValue::Real(_) => "",
        LegacyValue::Integer(_) | LegacyValue::Boolean(_) | LegacyValue::String(_) => return None,
    };
    let text = if unit.is_empty() {
        format!("{stored}")
    } else {
        format!("{stored}[{unit}]")
    };
    expr::parse(&text, &[], &[]).ok()
}

/// A `FreeCAD` expression as a neutral tree where every construct translates
/// and its quantity is the parameter's kind, and as the source's tokens with
/// each resolved reference otherwise.
fn translated(
    parameter: &LegacyParameter,
    text: &str,
    parameters: &BTreeMap<&ParameterId, &LegacyParameter>,
    kind: &ValueKind,
) -> Result<Expression, CodecError> {
    let targets = reference_paths(parameter, parameters);
    let mut tokens = Vec::new();
    let mut neutral = String::new();
    let mut refs: Vec<ParameterRef> = Vec::new();
    let mut translates = true;
    let mut text_start = 0;
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while cursor < text.len() {
        let start = cursor;
        let Some(ch) = text[cursor..].chars().next() else {
            break;
        };
        cursor += ch.len_utf8();
        if ch.is_ascii_digit() || (ch == '.' && bytes.get(cursor).is_some_and(u8::is_ascii_digit)) {
            while bytes
                .get(cursor)
                .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
            {
                cursor += 1;
            }
            if matches!(bytes.get(cursor), Some(b'e' | b'E'))
                && bytes
                    .get(cursor + 1)
                    .is_some_and(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-'))
            {
                cursor += 2;
                while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                    cursor += 1;
                }
            }
            let number = &text[start..cursor];
            let mut after = cursor;
            while bytes.get(after) == Some(&b' ') {
                after += 1;
            }
            let unit_end = text[after..]
                .char_indices()
                .find(|(_, ch)| !(ch.is_alphabetic() || *ch == '°' || *ch == 'µ'))
                .map_or(text.len(), |(at, _)| after + at);
            let word = &text[after..unit_end];
            if word.is_empty() {
                neutral.push_str(number);
            } else if let Some(symbol) = unit_symbol(word) {
                neutral.push_str(number);
                neutral.push('[');
                neutral.push_str(symbol);
                neutral.push(']');
                cursor = unit_end;
            } else {
                translates = false;
            }
        } else if ch.is_alphabetic() || ch == '_' {
            while let Some(next) = text[cursor..].chars().next() {
                if next.is_alphanumeric() || next == '_' || next == '.' {
                    cursor += next.len_utf8();
                } else {
                    break;
                }
            }
            let path = &text[start..cursor];
            if let Some(target) = targets.get(path) {
                let reference = ParameterRef {
                    target: RefTarget::Parameter((*target).clone()),
                    through: None,
                };
                if start > text_start {
                    tokens.push(ExpressionToken::Text(text[text_start..start].to_owned()));
                }
                tokens.push(ExpressionToken::Reference(reference.clone()));
                text_start = cursor;
                let index = refs
                    .iter()
                    .position(|known| *known == reference)
                    .unwrap_or_else(|| {
                        refs.push(reference);
                        refs.len() - 1
                    });
                neutral.push('$');
                neutral.push_str(&index.to_string());
            } else {
                translates = false;
            }
        } else if matches!(ch, '+' | '-' | '*' | '/' | '(' | ')') {
            neutral.push(ch);
        } else if !ch.is_whitespace() {
            translates = false;
        }
    }
    if text_start < text.len() {
        tokens.push(ExpressionToken::Text(text[text_start..].to_owned()));
    }
    if translates {
        if let Ok(tree) = expr::parse(&neutral, &refs, &[]) {
            if quantity(&tree, parameters)
                .map(ValueKind::Quantity)
                .as_ref()
                == Some(kind)
            {
                return Ok(Expression::Neutral(Neutral(tree)));
            }
        }
    }
    if tokens.is_empty() {
        return Err(CodecError::malformed(format!(
            "FCStd parameter {} has an empty expression",
            parameter.id.as_str()
        )));
    }
    Ok(Expression::Source {
        dialect: ExpressionDialect::FreeCad,
        tokens,
    })
}

/// The paths by which the expression names each parameter it depends on:
/// `Object.Property` and `Object.Constraints.name`.
fn reference_paths<'a>(
    parameter: &LegacyParameter,
    parameters: &BTreeMap<&ParameterId, &'a LegacyParameter>,
) -> BTreeMap<String, &'a ParameterId> {
    let mut paths = BTreeMap::new();
    for dependency in &parameter.dependencies {
        let Some(target) = parameters.get(dependency) else {
            continue;
        };
        let Some(object) = target
            .owner
            .as_ref()
            .and_then(|owner| owner.as_str().split_once('#'))
            .map(|(_, key)| key)
        else {
            continue;
        };
        let mut names = vec![target.name.clone()];
        if let Some(name) = property(target, "source_name") {
            names.push(format!("Constraints.{name}"));
        }
        for name in names {
            paths
                .entry(format!("{object}.{name}"))
                .or_insert(&target.id);
        }
    }
    paths
}

/// The `UnitSymbol` of a `FreeCAD` unit word.
fn unit_symbol(word: &str) -> Option<&'static str> {
    Some(match word {
        "nm" => "nm",
        "um" | "µm" => "um",
        "mm" => "mm",
        "cm" => "cm",
        "m" => "m",
        "km" => "km",
        "in" => "in",
        "ft" => "ft",
        "yd" => "yd",
        "mi" => "mi",
        "deg" | "°" => "deg",
        "rad" => "rad",
        "gon" => "grad",
        "s" => "s",
        "min" => "min",
        "h" => "h",
        "g" => "g",
        "kg" => "kg",
        "lb" => "lb",
        _ => return None,
    })
}

/// The quantity of a tree of literals, references, signs, and the four
/// arithmetic operators, or `None` where it has none.
fn quantity(
    tree: &Node<ParameterRef>,
    parameters: &BTreeMap<&ParameterId, &LegacyParameter>,
) -> Option<Quantity> {
    let mut pending = vec![(tree, false)];
    let mut values: Vec<Quantity> = Vec::new();
    while let Some((node, visited)) = pending.pop() {
        match (node, visited) {
            (Node::Number { unit, .. }, _) => {
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
                values.push(quantity);
            }
            (
                Node::Reference(ParameterRef {
                    target: RefTarget::Parameter(id),
                    ..
                }),
                _,
            ) => {
                let target = parameters.get(id)?;
                let (ValueKind::Quantity(quantity), _) = kind_and_value(target.value.as_ref())
                else {
                    return None;
                };
                target.value.as_ref()?;
                values.push(quantity);
            }
            (
                Node::Unary {
                    op: UnaryOp::Negate,
                    operand,
                },
                false,
            ) => pending.push((operand, false)),
            (Node::Binary { left, right, .. }, false) => {
                pending.push((node, true));
                pending.push((right, false));
                pending.push((left, false));
            }
            (Node::Binary { op, .. }, true) => {
                let right = values.pop()?;
                let left = values.pop()?;
                values.push(match op {
                    BinaryOp::Add | BinaryOp::Subtract if left == right => left,
                    BinaryOp::Multiply => Quantity {
                        length: left.length.checked_add(right.length)?,
                        mass: left.mass.checked_add(right.mass)?,
                        angle: left.angle.checked_add(right.angle)?,
                        time: left.time.checked_add(right.time)?,
                    },
                    BinaryOp::Divide => Quantity {
                        length: left.length.checked_sub(right.length)?,
                        mass: left.mass.checked_sub(right.mass)?,
                        angle: left.angle.checked_sub(right.angle)?,
                        time: left.time.checked_sub(right.time)?,
                    },
                    _ => return None,
                });
            }
            _ => return None,
        }
    }
    let [quantity] = values.as_slice() else {
        return None;
    };
    Some(*quantity)
}

#[cfg(test)]
mod tests;
