// SPDX-License-Identifier: Apache-2.0
//! The properties and sketch constraints of a Body's features, and the
//! properties of the Body, as v11 parameters.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::dimension::{Dimension, ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{
    self, Expression, ExpressionDialect, ExpressionToken, MulOp, Neutral, Node, UnaryOp,
};
use cadmpeg_ir::design::feature::Feature;
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::names::CodecName;
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::parameter::{
    DesignParameter, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::units::{Quantity, ValueKind};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::parameters::{
    DesignParameter as LegacyParameter, ParameterValue as LegacyValue,
};
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;

use crate::loss::FreecadLossCode;

/// The property type of a sketch's constraints, whose values `FreeCAD` stores
/// in millimetres and radians.
const CONSTRAINT_LIST: &str = "Sketcher::PropertyConstraintList";

/// The property type of a spreadsheet's cells, one parameter each (§5.12).
const SHEET: &str = "Spreadsheet::PropertySheet";

/// The display attributes of a cell, each kept as `fcstd:<attribute>`.
const CELL_DISPLAY: [&str; 5] = [
    "alignment",
    "style",
    "foregroundColor",
    "backgroundColor",
    "displayUnit",
];

/// Each parameter the decoder read that one of `features` or a Body owns, in
/// the scope of `definition`. `containers` maps a Body's feature to its
/// container, and `carriers` a property record to its type.
pub(super) fn project(
    ir: &CadIr,
    definition: &ProductDefinitionId,
    containers: &BTreeMap<FeatureId, ContainerId>,
    features: &[Feature],
    carriers: &HashMap<&str, &str>,
    losses: &mut Vec<LossNote>,
) -> Vec<DesignParameter> {
    let features = features
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
    // The legacy parameters follow their dependencies, so a quantity found
    // for a parameter with no stored value is known before an expression
    // that references it.
    let mut known = BTreeMap::new();
    let mut parameters = Vec::new();
    for parameter in &ir.model.parameters {
        let Some(owner) = owners.get(&parameter.id) else {
            continue;
        };
        if !std::ptr::eq(by_id[&parameter.id], parameter) {
            continue;
        }
        let carrier = parameter
            .native_ref
            .as_deref()
            .and_then(|property| carriers.get(property).copied());
        let (kind, value, parameter_definition) = if carrier == Some(SHEET) {
            let (kind, value, expression) = cell(parameter, &by_id, &known, carriers, losses);
            (kind, value, ParameterDefinition::Expression(expression))
        } else {
            let (kind, value) = kind_and_value(parameter.value.as_ref());
            let parameter_definition = if property(parameter, "is_driving") == Some("0") {
                ParameterDefinition::Driven
            } else {
                ParameterDefinition::Expression(expression(
                    parameter, &by_id, &known, &kind, carriers, losses,
                ))
            };
            (kind, value, parameter_definition)
        };
        if let ValueKind::Quantity(quantity) = kind {
            let found = carrier == Some(SHEET)
                && matches!(
                    parameter_definition,
                    ParameterDefinition::Expression(Expression::Neutral(_))
                );
            if value.is_some() || found {
                known.insert(&parameter.id, quantity);
            }
        }
        let (name, source_settings) = if carrier == Some(SHEET) {
            (
                property(parameter, "alias")
                    .and_then(|alias| NonBlankString::new(alias.to_owned())),
                cell_settings(parameter),
            )
        } else {
            (name(parameter), BTreeMap::default())
        };
        parameters.push(DesignParameter {
            id: parameter.id.clone(),
            scope: definition.clone(),
            owner: Some(owner.clone()),
            name,
            kind,
            unit: None,
            definition: parameter_definition,
            value,
            display: None,
            tolerance: None,
            decimals: None,
            comment: None,
            native_options: BTreeMap::default(),
            source_settings,
            native_ref: parameter.native_ref.clone(),
        });
    }
    parameters.sort_by(|a, b| a.id.cmp(&b.id));
    parameters
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

/// A cell's kind, value, and expression (§5.12). `FreeCAD` reads an `=`
/// content as a formula, a `'` content as the text after the mark, a
/// content that starts with a number as a number or a quantity, and any
/// other content as text. A formula with no stored value has the quantity
/// of its neutral tree.
fn cell(
    parameter: &LegacyParameter,
    parameters: &BTreeMap<&ParameterId, &LegacyParameter>,
    known: &BTreeMap<&ParameterId, Quantity>,
    carriers: &HashMap<&str, &str>,
    losses: &mut Vec<LossNote>,
) -> (ValueKind, Option<ParameterValue>, Expression) {
    let content = parameter.expression.as_str();
    let text = |text: &str| {
        (
            ValueKind::Text,
            Some(ParameterValue::Text(text.to_owned())),
            Expression::Neutral(Neutral(Node::Text(text.to_owned()))),
        )
    };
    if let Some(quoted) = content.strip_prefix('\'') {
        return text(quoted);
    }
    let formula = match content.strip_prefix('=') {
        Some(formula) => formula,
        None if parameter.value.is_some() => {
            let (kind, value) = kind_and_value(parameter.value.as_ref());
            let expression = expression(parameter, parameters, known, &kind, carriers, losses);
            return (kind, value, expression);
        }
        None if content.starts_with(|ch: char| ch.is_ascii_digit() || ".+-".contains(ch)) => {
            content
        }
        None => return text(content),
    };
    if parameter.value.is_some() {
        let (kind, value) = kind_and_value(parameter.value.as_ref());
        let expression = translated(parameter, formula, parameters, known, Some(&kind));
        return (kind, value, expression);
    }
    let expression = translated(parameter, formula, parameters, known, None);
    let kind = match &expression {
        Expression::Neutral(Neutral(tree)) => quantity(tree, parameters, known),
        Expression::Source { .. } => None,
    }
    .map_or(ValueKind::Quantity(Quantity::REAL), ValueKind::Quantity);
    (kind, None, expression)
}

/// A cell's display attributes, which only a `FreeCAD` writer writes (Rule N
/// case (b)).
fn cell_settings(parameter: &LegacyParameter) -> NativeOptions {
    CELL_DISPLAY
        .iter()
        .filter_map(|attribute| {
            let value = NonBlankString::new(property(parameter, attribute)?.to_owned())?;
            let key = CodecName::new(&cadmpeg_ir::codec_id!("fcstd"), attribute)?;
            Some((key, NativeOption::Text(Dimension::Literal(value))))
        })
        .collect()
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
/// to it. A parameter whose expression text is empty is stated with its
/// stored value and charged a loss.
fn expression(
    parameter: &LegacyParameter,
    parameters: &BTreeMap<&ParameterId, &LegacyParameter>,
    known: &BTreeMap<&ParameterId, Quantity>,
    kind: &ValueKind,
    carriers: &HashMap<&str, &str>,
    losses: &mut Vec<LossNote>,
) -> Expression {
    let text = parameter.expression.as_str();
    let carrier = parameter
        .native_ref
        .as_deref()
        .and_then(|property| carriers.get(property).copied());
    if text.trim().is_empty() {
        return empty_expression(parameter, carrier, losses);
    }
    if parameter.dependencies.is_empty() {
        if let Some(literal) = literal(text, parameter.value.as_ref(), carrier) {
            return Expression::Neutral(Neutral(literal));
        }
    }
    translated(parameter, text, parameters, known, Some(kind))
}

/// The expression of a parameter that stores no expression text: its stored
/// value as a one-literal expression where it is a quantity in a known
/// carrier unit, and otherwise a source expression with no token, the
/// spelling of a text the decoder did not read (interim, gap 20261119-X3-1).
fn empty_expression(
    parameter: &LegacyParameter,
    carrier: Option<&str>,
    losses: &mut Vec<LossNote>,
) -> Expression {
    let literal = parameter
        .value
        .as_ref()
        .and_then(|value| stored_literal(value, carrier));
    let stated = if literal.is_some() {
        "its stored value is stated as its expression"
    } else {
        "its expression is stated with no text"
    };
    losses.push(FreecadLossCode::ParameterExpressionEmpty.note(format!(
        "FCStd parameter {} has an empty expression; {stated}",
        parameter.id.as_str()
    )));
    literal.map_or_else(
        || Expression::Source {
            dialect: ExpressionDialect::FreeCad,
            tokens: Vec::new(),
        },
        |literal| Expression::Neutral(Neutral(literal)),
    )
}

/// The unit a carrier stores a value in (§5.12): millimetres for a length,
/// degrees for an angle property, radians for a sketch constraint's angle,
/// and no unit for a real. An angle whose carrier the decoder does not know,
/// and a value that is no quantity, has none.
struct CarrierUnit {
    symbol: &'static str,
    /// A number in the unit, in the canonical unit.
    to_canonical: fn(f64) -> f64,
    /// A number in the canonical unit, in the unit.
    from_canonical: fn(f64) -> f64,
}

fn carrier_unit(value: &LegacyValue, carrier: Option<&str>) -> Option<CarrierUnit> {
    let same = |value| value;
    match value {
        LegacyValue::Length(_) => Some(CarrierUnit {
            symbol: "mm",
            to_canonical: same,
            from_canonical: same,
        }),
        LegacyValue::Angle(_) if carrier == Some(CONSTRAINT_LIST) => Some(CarrierUnit {
            symbol: "rad",
            to_canonical: same,
            from_canonical: same,
        }),
        LegacyValue::Angle(_) if carrier.is_some_and(|carrier| carrier.contains("Angle")) => {
            Some(CarrierUnit {
                symbol: "deg",
                to_canonical: f64::to_radians,
                from_canonical: f64::to_degrees,
            })
        }
        LegacyValue::Real(_) => Some(CarrierUnit {
            symbol: "",
            to_canonical: same,
            from_canonical: same,
        }),
        LegacyValue::Angle(_)
        | LegacyValue::Integer(_)
        | LegacyValue::Boolean(_)
        | LegacyValue::String(_) => None,
    }
}

/// A stored value's text as a literal in the unit its carrier stores it in.
fn literal(
    text: &str,
    value: Option<&LegacyValue>,
    carrier: Option<&str>,
) -> Option<Node<ParameterRef>> {
    let stored = text
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())?;
    literal_in(stored, &carrier_unit(value?, carrier)?)
}

/// A decoded value as a literal in the unit its carrier stores it in.
fn stored_literal(value: &LegacyValue, carrier: Option<&str>) -> Option<Node<ParameterRef>> {
    let unit = carrier_unit(value, carrier)?;
    let canonical = match value {
        LegacyValue::Length(value) => value.get(),
        LegacyValue::Angle(value) => value.get(),
        LegacyValue::Real(value) => value.get(),
        LegacyValue::Integer(_) | LegacyValue::Boolean(_) | LegacyValue::String(_) => return None,
    };
    literal_in((unit.from_canonical)(canonical), &unit)
}

fn literal_in(stored: f64, unit: &CarrierUnit) -> Option<Node<ParameterRef>> {
    let number = decimal(stored, unit.to_canonical);
    let text = if unit.symbol.is_empty() {
        number
    } else {
        format!("{number}[{}]", unit.symbol)
    };
    expr::parse(&text, &[], &[]).ok()
}

/// The shortest decimal whose value in the canonical unit equals that of
/// `stored` to 15 significant digits (§5.12). `canonical` converts a number
/// in the literal's unit to the canonical unit.
fn decimal(stored: f64, canonical: fn(f64) -> f64) -> String {
    let fifteen = |value: f64| format!("{:.14e}", canonical(value));
    let target = fifteen(stored);
    (0..17)
        .filter_map(|places| format!("{stored:.places$e}").parse::<f64>().ok())
        .find(|candidate| fifteen(*candidate) == target)
        .map_or_else(|| format!("{stored}"), |candidate| format!("{candidate}"))
}

/// A `FreeCAD` expression as a neutral tree where every construct translates
/// and its quantity is `kind`, or any quantity where `kind` is `None`, and as
/// the source's tokens with each resolved reference otherwise.
fn translated(
    parameter: &LegacyParameter,
    text: &str,
    parameters: &BTreeMap<&ParameterId, &LegacyParameter>,
    known: &BTreeMap<&ParameterId, Quantity>,
    kind: Option<&ValueKind>,
) -> Expression {
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
            let stated = quantity(&tree, parameters, known);
            let Some(kind) = kind else {
                if stated.is_some() {
                    return Expression::Neutral(Neutral(tree));
                }
                return source(text, tokens);
            };
            if stated.map(ValueKind::Quantity).as_ref() == Some(kind) {
                return Expression::Neutral(Neutral(tree));
            }
            // FreeCAD takes a dimensionless result in the property's
            // internal unit (§5.12, Kind).
            let internal = match kind {
                ValueKind::Quantity(Quantity::LENGTH) => Some("mm"),
                ValueKind::Quantity(Quantity::ANGLE) => Some("deg"),
                _ => None,
            };
            if let (Some(Quantity::REAL), Some(unit)) = (stated, internal) {
                if let Ok(tree) = expr::parse(&format!("({neutral}) * 1[{unit}]"), &refs, &[]) {
                    return Expression::Neutral(Neutral(tree));
                }
            }
        }
    }
    source(text, tokens)
}

/// The source's tokens of `text`. A source token list holds a text token
/// (part 10): a lone reference stays the source's text.
fn source(text: &str, mut tokens: Vec<ExpressionToken<ParameterRef>>) -> Expression {
    if !text.is_empty()
        && !tokens
            .iter()
            .any(|token| matches!(token, ExpressionToken::Text(_)))
    {
        tokens = vec![ExpressionToken::Text(text.to_owned())];
    }
    Expression::Source {
        dialect: ExpressionDialect::FreeCad,
        tokens,
    }
}

/// The paths by which the expression names each parameter it depends on:
/// `Object.Property`, `Object.Constraints.name`, and a cell's
/// `Object.alias` and `Object.address`. A parameter of the expression's own
/// owner is also named without its object (a cell's `=A1 * 2`).
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
        if let Some(address) = property(target, "address") {
            names.push(address.to_owned());
        }
        let local = target.owner == parameter.owner;
        for name in names {
            if local {
                paths.entry(name.clone()).or_insert(&target.id);
            }
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
    known: &BTreeMap<&ParameterId, Quantity>,
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
                if let Some(quantity) = known.get(id) {
                    values.push(*quantity);
                    continue;
                }
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
            (Node::Sum { .. } | Node::Product { .. }, false) => {
                pending.push((node, true));
                let operands: Vec<_> = node.operands().collect();
                pending.extend(operands.into_iter().rev().map(|operand| (operand, false)));
            }
            (Node::Sum { rest, .. }, true) => {
                let start = values.len().checked_sub(rest.len() + 1)?;
                let run = values.split_off(start);
                let (&first, rest) = run.split_first()?;
                rest.iter()
                    .all(|quantity| *quantity == first)
                    .then_some(())?;
                values.push(first);
            }
            (Node::Product { rest, .. }, true) => {
                let start = values.len().checked_sub(rest.len() + 1)?;
                let run = values.split_off(start);
                let (&first, operands) = run.split_first()?;
                let mut held = first;
                for ((op, _), right) in rest.iter().zip(operands) {
                    let combine = |a: i8, b: i8| match op {
                        MulOp::Multiply => a.checked_add(b),
                        MulOp::Divide => a.checked_sub(b),
                    };
                    held = Quantity {
                        length: combine(held.length, right.length)?,
                        mass: combine(held.mass, right.mass)?,
                        angle: combine(held.angle, right.angle)?,
                        time: combine(held.time, right.time)?,
                    };
                }
                values.push(held);
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
