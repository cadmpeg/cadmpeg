// SPDX-License-Identifier: Apache-2.0
//! Model parameters: section dimensions and relation assignments.
//!
//! A section dimension or a relation assignment keeps its Creo text, with
//! each parameter the decoder resolved as a reference, except a literal in a
//! unit the file states that the stored value confirms, which is a
//! one-literal expression, and a relation whose whole text is one resolved
//! parameter of the same kind, which is that reference. The decode report
//! names, in one `design.parameter-reference-as-text` note, each parameter
//! whose expression holds no reference to a dependency the decoder resolved
//! for it, or to a name the decoder left out of its dependencies because it
//! closes a cycle, with what it loses.

use std::collections::BTreeMap;
use std::num::NonZeroI8;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::{Dimension, ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{
    self, Expression, ExpressionDialect, ExpressionToken, Neutral, Node, UnaryOp,
};
use cadmpeg_ir::design::ops::sketch::{RadialMeasure, SketchRelation};
use cadmpeg_ir::design::parameter::{
    DesignParameter, DimensionDisplay, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, ValueKind};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    DesignParameter as LegacyParameter, DimensionDisplay as LegacyDisplay, ParameterId,
    ParameterValue as LegacyValue,
};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};

use crate::loss::{CreoLossCode, NAMED_INSTANCES_PER_NOTE};

const FIRST_POWER: NonZeroI8 = match NonZeroI8::new(1) {
    Some(power) => power,
    None => NonZeroI8::MAX,
};

/// Relative agreement a literal in its unit needs with the stored value.
const LITERAL_AGREEMENT: f64 = 1e-12;

/// Millimetres in one nanometre.
const MILLIMETRES_PER_NANOMETRE: f64 = 1e-6;

/// States every model parameter as a parameter of `definition`, and adds to
/// `losses` the note naming those that drop a dependency.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    definition: &ProductDefinitionId,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let names = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let kinds = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), kind_and_value(parameter).0))
        .collect::<BTreeMap<_, _>>();
    let mut projected = Vec::new();
    let mut dropped = Vec::new();
    for parameter in &ir.model.parameters {
        ctx.charge_entities(1, "project Creo v11 parameter")?;
        let design = project_parameter(parameter, definition, &names, &kinds);
        if let ParameterDefinition::Expression(expression) = &design.definition {
            let lost = dropped_dependencies(parameter, expression, &names);
            if !lost.is_empty() {
                dropped.push(format!("{} ({})", parameter.id, lost.join(", ")));
            }
        }
        projected.push(design);
    }
    if !dropped.is_empty() {
        let more = if dropped.len() > NAMED_INSTANCES_PER_NOTE {
            ", ..."
        } else {
            ""
        };
        losses.push(CreoLossCode::DesignParameterReferenceAsText.note(format!(
            "{} parameters hold a parameter the decoder resolved as their dependency as text, \
             or omit it, and state no dependency on it (each with the dependencies it loses): \
             {}{more}",
            dropped.len(),
            dropped
                .iter()
                .take(NAMED_INSTANCES_PER_NOTE)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    ir.design.parameters.extend(projected);
    ir.design.parameters.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}

/// Clears the presentation of a parameter whose binding radial dimension
/// already measures it (§5.12): a parameter a diameter binds has no
/// `display`, and one a radius binds states only a diameter presentation.
pub(super) fn omit_measured_display(ir: &mut CadIr) {
    let mut measured = BTreeMap::<&ParameterId, RadialMeasure>::new();
    for constraint in &ir.design.sketch_constraints {
        let SketchRelation::Radial {
            measure,
            value: Dimension::Parameter(parameter) | Dimension::Negated(parameter),
            ..
        } = &constraint.relation
        else {
            continue;
        };
        let entry = measured.entry(parameter).or_insert(*measure);
        if *measure == RadialMeasure::Diameter {
            *entry = RadialMeasure::Diameter;
        }
    }
    for parameter in &mut ir.design.parameters {
        let omitted = match measured.get(&parameter.id) {
            Some(RadialMeasure::Diameter) => true,
            Some(RadialMeasure::Radius) => parameter.display == Some(DimensionDisplay::Radius),
            _ => false,
        };
        if omitted {
            parameter.display = None;
        }
    }
}

fn project_parameter(
    parameter: &LegacyParameter,
    scope: &ProductDefinitionId,
    names: &BTreeMap<ParameterId, &str>,
    kinds: &BTreeMap<ParameterId, ValueKind>,
) -> DesignParameter {
    let (kind, value) = kind_and_value(parameter);
    let unit = declared_unit(parameter).filter(|unit| quantity(unit) == quantity_of(&kind));
    let definition = ParameterDefinition::Expression(expression(
        parameter,
        names,
        kinds,
        &kind,
        value.as_ref(),
        unit.as_ref(),
    ));
    DesignParameter {
        id: parameter.id.clone(),
        scope: scope.clone(),
        owner: parameter.owner.clone().map(ParameterOwner::Feature),
        name: NonBlankString::new(parameter.name.clone()),
        kind,
        unit,
        definition,
        value,
        display: parameter.display.map(|display| match display {
            LegacyDisplay::Diameter => DimensionDisplay::Diameter,
            LegacyDisplay::Radius => DimensionDisplay::Radius,
        }),
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: parameter.native_ref.clone(),
    }
}

/// The parameter's kind and cache. A relation value of a derived quantity
/// is stated from the canonical value and exponents the decoder evaluated;
/// a temperature has no quantity of the vocabulary.
fn kind_and_value(parameter: &LegacyParameter) -> (ValueKind, Option<ParameterValue>) {
    let number = |quantity, value: f64| {
        (
            ValueKind::Quantity(quantity),
            FiniteReal::new(value).map(ParameterValue::Number),
        )
    };
    match &parameter.value {
        Some(LegacyValue::Length(value)) => number(Quantity::LENGTH, value.get()),
        Some(LegacyValue::Angle(value)) => number(Quantity::ANGLE, value.get()),
        Some(LegacyValue::Real(value)) => number(Quantity::REAL, value.get()),
        Some(LegacyValue::Integer(value)) => {
            (ValueKind::Integer, Some(ParameterValue::Integer(*value)))
        }
        Some(LegacyValue::Boolean(value)) => {
            (ValueKind::Boolean, Some(ParameterValue::Boolean(*value)))
        }
        Some(LegacyValue::String(value)) => {
            (ValueKind::Text, Some(ParameterValue::Text(value.clone())))
        }
        None => evaluated(parameter),
    }
}

fn property<'a>(parameter: &'a LegacyParameter, key: &str) -> Option<&'a str> {
    parameter
        .properties
        .iter()
        .find(|(name, _)| name.as_str() == key)
        .map(|(_, value)| value.as_str())
}

fn evaluated(parameter: &LegacyParameter) -> (ValueKind, Option<ParameterValue>) {
    let Some(exponents) = property(parameter, "evaluated_dimension") else {
        return (ValueKind::Quantity(Quantity::REAL), None);
    };
    let mut quantity = Quantity::REAL;
    for entry in exponents.split(',') {
        let Some((name, power)) = entry.split_once(':') else {
            return native_kind("creo:quantity");
        };
        let Ok(power) = power.parse::<i8>() else {
            return native_kind("creo:quantity");
        };
        match name {
            "length" => quantity.length = power,
            "mass" => quantity.mass = power,
            "angle" => quantity.angle = power,
            "time" => quantity.time = power,
            "temperature" if power == 0 => {}
            "temperature" => return native_kind("creo:temperature"),
            _ => return native_kind("creo:quantity"),
        }
    }
    let value = property(parameter, "evaluated_canonical_value")
        .and_then(|value| value.parse::<f64>().ok())
        .and_then(FiniteReal::new)
        .map(ParameterValue::Number);
    (ValueKind::Quantity(quantity), value)
}

fn native_kind(kind: &str) -> (ValueKind, Option<ParameterValue>) {
    (
        NonBlankString::new(kind.to_owned()).map_or(ValueKind::Quantity(Quantity::REAL), |kind| {
            ValueKind::Native(kind)
        }),
        None,
    )
}

fn quantity_of(kind: &ValueKind) -> Option<Quantity> {
    match kind {
        ValueKind::Quantity(quantity) => Some(*quantity),
        ValueKind::Integer | ValueKind::Boolean | ValueKind::Text | ValueKind::Native(_) => None,
    }
}

/// The unit a relation declares for its target, where it is one symbol of
/// the vocabulary.
fn declared_unit(parameter: &LegacyParameter) -> Option<Unit> {
    property(parameter, "declared_unit")
        .and_then(creo_symbol)
        .map(single)
}

fn single(symbol: UnitSymbol) -> Unit {
    Unit(vec![UnitFactor {
        symbol,
        power: FIRST_POWER,
    }])
}

/// A Creo unit name of one vocabulary symbol.
fn creo_symbol(name: &str) -> Option<UnitSymbol> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "nm" => UnitSymbol::Nm,
        "um" | "micron" => UnitSymbol::Um,
        "mm" => UnitSymbol::Mm,
        "cm" => UnitSymbol::Cm,
        "m" => UnitSymbol::M,
        "km" => UnitSymbol::Km,
        "in" | "inch" => UnitSymbol::In,
        "ft" | "foot" => UnitSymbol::Ft,
        "yd" => UnitSymbol::Yd,
        "mi" => UnitSymbol::Mi,
        "rad" | "radian" => UnitSymbol::Rad,
        "deg" | "degree" => UnitSymbol::Deg,
        "s" | "sec" => UnitSymbol::S,
        "min" => UnitSymbol::Min,
        "h" | "hr" => UnitSymbol::H,
        "g" => UnitSymbol::G,
        "kg" => UnitSymbol::Kg,
        "lbm" => UnitSymbol::Lb,
        _ => return None,
    })
}

fn quantity(unit: &Unit) -> Option<Quantity> {
    let mut quantity = Quantity::REAL;
    for factor in &unit.0 {
        let slot = match factor.symbol {
            UnitSymbol::Rad
            | UnitSymbol::Deg
            | UnitSymbol::Grad
            | UnitSymbol::Arcmin
            | UnitSymbol::Arcsec
            | UnitSymbol::Rev => &mut quantity.angle,
            UnitSymbol::S | UnitSymbol::Min | UnitSymbol::H => &mut quantity.time,
            UnitSymbol::G | UnitSymbol::Kg | UnitSymbol::Lb => &mut quantity.mass,
            UnitSymbol::Nm
            | UnitSymbol::Um
            | UnitSymbol::Mm
            | UnitSymbol::Cm
            | UnitSymbol::M
            | UnitSymbol::Km
            | UnitSymbol::In
            | UnitSymbol::Ft
            | UnitSymbol::Yd
            | UnitSymbol::Mi => &mut quantity.length,
        };
        *slot = slot.checked_add(factor.power.get())?;
    }
    Some(quantity)
}

/// The canonical value of one unit of a length or angle symbol.
fn canonical_factor(symbol: UnitSymbol) -> Option<f64> {
    Some(match symbol {
        UnitSymbol::Nm => MILLIMETRES_PER_NANOMETRE,
        UnitSymbol::Um => 1e-3,
        UnitSymbol::Mm | UnitSymbol::Rad => 1.0,
        UnitSymbol::Cm => 10.0,
        UnitSymbol::M => 1000.0,
        UnitSymbol::Km => 1e6,
        UnitSymbol::In => 25.4,
        UnitSymbol::Ft => 304.8,
        UnitSymbol::Yd => 914.4,
        UnitSymbol::Mi => 1_609_344.0,
        UnitSymbol::Deg => std::f64::consts::PI / 180.0,
        _ => return None,
    })
}

fn expression(
    parameter: &LegacyParameter,
    names: &BTreeMap<ParameterId, &str>,
    kinds: &BTreeMap<ParameterId, ValueKind>,
    kind: &ValueKind,
    value: Option<&ParameterValue>,
    declared: Option<&Unit>,
) -> Expression {
    let text = parameter.expression.trim();
    if let Some(literal) = literal(text, kind, value, declared) {
        return Expression::Neutral(Neutral(literal));
    }
    let tokens = tokens(&parameter.expression, parameter, names);
    if let [ExpressionToken::Reference(reference)] = tokens.as_slice() {
        return whole_reference(reference, &parameter.expression, kind, kinds);
    }
    Expression::Source {
        dialect: ExpressionDialect::Creo,
        tokens,
    }
}

/// A relation whose whole text is one resolved parameter is that reference
/// where the parameter has the same kind, one with a neutral value. Otherwise
/// the name stays text: a `Source` expression holds a `Text` token (part 10),
/// and [`drops_dependency`] names the parameter.
fn whole_reference(
    reference: &ParameterRef,
    text: &str,
    kind: &ValueKind,
    kinds: &BTreeMap<ParameterId, ValueKind>,
) -> Expression {
    let typed = match &reference.target {
        RefTarget::Parameter(target) => kinds.get(target) == Some(kind),
        RefTarget::External { .. } => false,
    };
    if typed && !matches!(kind, ValueKind::Native(_)) {
        return Expression::Neutral(Neutral(Node::Reference(reference.clone())));
    }
    Expression::Source {
        dialect: ExpressionDialect::Creo,
        tokens: vec![ExpressionToken::Text(text.to_owned())],
    }
}

/// The dependencies the decoder resolved for `parameter` that `expression`
/// holds no reference to, by id, and each name of its `cyclic_dependencies`
/// (an edge the decoder left out of `dependencies` because it closes a
/// cycle) that `expression` holds no reference to, as `cyclic <name>`.
fn dropped_dependencies(
    parameter: &LegacyParameter,
    expression: &Expression,
    names: &BTreeMap<ParameterId, &str>,
) -> Vec<String> {
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
    let held = held
        .iter()
        .filter_map(|reference| match &reference.target {
            RefTarget::Parameter(id) => Some(id),
            RefTarget::External { .. } => None,
        })
        .collect::<Vec<_>>();
    let resolved = parameter
        .dependencies
        .iter()
        .filter(|dependency| !held.contains(dependency))
        .map(ToString::to_string);
    let cyclic = property(parameter, "cyclic_dependencies")
        .into_iter()
        .flat_map(|list| list.split(','))
        .filter(|name| {
            !held.iter().any(|id| {
                names
                    .get(*id)
                    .is_some_and(|target| target.eq_ignore_ascii_case(name))
            })
        })
        .map(|name| format!("cyclic {name}"));
    resolved.chain(cyclic).collect()
}

/// A literal whose unit the file states, written in brackets or declared
/// for the relation's target, and whose value in that unit is the stored
/// value. A bare literal with no declared unit stays source text: which
/// unit Creo reads it in is under review (interim, errata H3).
fn literal(
    text: &str,
    kind: &ValueKind,
    value: Option<&ParameterValue>,
    declared: Option<&Unit>,
) -> Option<Node<ParameterRef>> {
    let Some(ParameterValue::Number(stored)) = value else {
        return None;
    };
    let quantity = quantity_of(kind)?;
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, text),
    };
    let (number, written) = match unsigned.split_once('[') {
        Some((number, unit)) => (
            number.trim_end(),
            Some(creo_symbol(unit.strip_suffix(']')?)?),
        ),
        None => (unsigned, None),
    };
    let leads = number
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_digit() || byte == b'.');
    let numeric = number
        .bytes()
        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-'));
    if !leads || !numeric {
        return None;
    }
    let magnitude = number.parse::<f64>().ok()?;
    let signed = if negative { -magnitude } else { magnitude };
    let unit = match written {
        Some(symbol) => single(symbol),
        None => declared?.clone(),
    };
    if self::quantity(&unit) != Some(quantity)
        || !canonical(signed, &unit).is_some_and(|canonical| agrees(canonical, stored.get()))
    {
        return None;
    }
    let node = Node::Number {
        value: NonNegativeReal::new(magnitude)?,
        unit,
    };
    Some(if negative {
        Node::Unary {
            op: UnaryOp::Negate,
            operand: Box::new(node),
        }
    } else {
        node
    })
}

fn canonical(value: f64, unit: &Unit) -> Option<f64> {
    match unit.0.as_slice() {
        [] => Some(value),
        [factor] if factor.power.get() == 1 => Some(value * canonical_factor(factor.symbol)?),
        _ => None,
    }
}

fn agrees(literal: f64, stored: f64) -> bool {
    (literal - stored).abs() <= LITERAL_AGREEMENT * literal.abs().max(stored.abs())
}

/// The relation text as tokens: each name of a parameter the decoder
/// resolved as this parameter's dependency is a reference, matched without
/// case as Creo matches names; every other run is text.
fn tokens(
    text: &str,
    parameter: &LegacyParameter,
    names: &BTreeMap<ParameterId, &str>,
) -> Vec<ExpressionToken<ParameterRef>> {
    let mut targets = BTreeMap::<String, Option<&ParameterId>>::new();
    for dependency in &parameter.dependencies {
        if let Some(name) = names.get(dependency) {
            targets
                .entry(name.to_ascii_lowercase())
                .and_modify(|target| *target = None)
                .or_insert(Some(dependency));
        }
    }
    let mut tokens = Vec::new();
    let mut pending = String::new();
    let mut bracket = false;
    let mut rest = text;
    while let Some(ch) = rest.chars().next() {
        if ch.is_ascii_alphabetic() || ch == '_' {
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            let (word, tail) = rest.split_at(end);
            let qualified = tail.starts_with(':');
            let target = (!bracket && !qualified)
                .then(|| targets.get(&word.to_ascii_lowercase()).copied().flatten())
                .flatten();
            match target {
                Some(target) => {
                    if !pending.is_empty() {
                        tokens.push(ExpressionToken::Text(std::mem::take(&mut pending)));
                    }
                    tokens.push(ExpressionToken::Reference(ParameterRef {
                        target: RefTarget::Parameter(target.clone()),
                        through: None,
                    }));
                }
                None => pending.push_str(word),
            }
            rest = tail;
            continue;
        }
        match ch {
            '[' => bracket = true,
            ']' => bracket = false,
            _ => {}
        }
        pending.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    if !pending.is_empty() {
        tokens.push(ExpressionToken::Text(pending));
    }
    tokens
}

#[cfg(test)]
mod tests;
