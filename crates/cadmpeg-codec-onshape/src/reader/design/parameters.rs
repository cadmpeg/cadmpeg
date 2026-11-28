// SPDX-License-Identifier: Apache-2.0
//! The design parameters of a Part Studio (§5.12, §5.13).
//!
//! Each model parameter is a design parameter under its own id: a variable
//! is a user parameter of the scope, and a feature's or a sketch's dimension
//! is owned by that feature. The reader admits only expressions of numbers
//! in a length or angle unit, variables, `+ - * /`, unary minus, and
//! parentheses, each with its arithmetic meaning, so every admitted
//! expression is a neutral tree (§5.13).

use std::collections::BTreeMap;
use std::num::NonZeroI8;

use serde_json::Value;

use super::{expression, Sources};
use crate::reader::{features, id};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::dimension::{Dimension, ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{
    AddOp, Expression, ExpressionDialect, ExpressionToken, MulOp, Neutral, Node, UnaryOp,
};
use cadmpeg_ir::design::options::NativeOption;
use cadmpeg_ir::design::parameter::{
    DesignParameter, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, ValueKind};
use cadmpeg_ir::features::parameters::{
    DesignParameter as ModelParameter, ParameterValue as ModelValue,
};
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};

/// The design parameter of each model parameter of `parameters`, in the
/// scope `scope`, and the ids of those whose expression stays source text.
pub(super) fn project(
    parameters: &[ModelParameter],
    scope: &ProductDefinitionId,
    sources: &Sources<'_>,
) -> (Vec<DesignParameter>, Vec<ParameterId>) {
    let rows = quantity_rows(sources);
    let variables = variables(sources, parameters);
    let mut texts = Vec::new();
    let all = || {
        parameters
            .iter()
            .chain(sources.values().flat_map(|source| &source.parameters))
    };
    let mut lists = BTreeMap::new();
    for parameter in all() {
        if let Some(row) = rows.get(parameter.id.as_str()) {
            lists
                .entry(parameter.owner.as_ref())
                .and_modify(|list| {
                    if *list != Some(row.list) {
                        *list = None;
                    }
                })
                .or_insert(Some(row.list));
        }
    }
    let projected = all()
        .map(|parameter| {
            let property_value = |key: &str| {
                parameter
                    .properties
                    .iter()
                    .find(|(name, _)| name.as_str() == key)
                    .map(|(_, value)| value.as_str())
            };
            let source = rows.get(parameter.id.as_str());
            let row = source.map(|source| source.row);
            let raw = row
                .and_then(|row| row.get("expression"))
                .and_then(Value::as_str)
                .or_else(|| property_value("source_expression"));
            let ordinal = source.map(|source| source.feature_index);
            let resolve = |name: &str| {
                let bindings = variables.get(name)?;
                let end =
                    bindings.partition_point(|(at, _)| ordinal.is_none_or(|ordinal| *at < ordinal));
                bindings.get(end.checked_sub(1)?)?.1
            };
            let (kind, value) = match parameter.value.as_ref() {
                Some(ModelValue::Length(length)) => (
                    ValueKind::Quantity(Quantity::LENGTH),
                    FiniteReal::new(length.get()).map(ParameterValue::Number),
                ),
                Some(ModelValue::Angle(angle)) => (
                    ValueKind::Quantity(Quantity::ANGLE),
                    FiniteReal::new(angle.get()).map(ParameterValue::Number),
                ),
                Some(ModelValue::Real(real)) => (
                    ValueKind::Quantity(Quantity::REAL),
                    Some(ParameterValue::Number(*real)),
                ),
                Some(ModelValue::Integer(integer)) => {
                    (ValueKind::Integer, Some(ParameterValue::Integer(*integer)))
                }
                Some(ModelValue::Boolean(flag)) => {
                    (ValueKind::Boolean, Some(ParameterValue::Boolean(*flag)))
                }
                Some(ModelValue::String(text)) => {
                    (ValueKind::Text, Some(ParameterValue::Text(text.clone())))
                }
                None => (
                    ValueKind::Quantity(match property_value("quantity_kind") {
                        Some("length") => Quantity::LENGTH,
                        Some("angle") => Quantity::ANGLE,
                        _ => Quantity::REAL,
                    }),
                    None,
                ),
            };
            let expected = match kind {
                ValueKind::Quantity(Quantity::LENGTH) => Some(expression::Dimension::Length),
                ValueKind::Quantity(Quantity::ANGLE) => Some(expression::Dimension::Angle),
                ValueKind::Quantity(Quantity::REAL) => Some(expression::Dimension::Scalar),
                _ => None,
            };
            let tree = expected
                .and_then(|kind| neutral(raw.unwrap_or(&parameter.expression), kind, &resolve));
            let unit = row
                .and_then(|row| row.get("units"))
                .and_then(Value::as_str)
                .and_then(|unit| {
                    expression::unit(unit).and_then(|(_, _, canonical)| symbol(canonical))
                })
                .and_then(unit)
                .or_else(|| tree.as_ref().and_then(declared));
            let definition = match tree {
                Some(tree) => Expression::Neutral(Neutral(tree)),
                None => {
                    texts.push(parameter.id.clone());
                    self::source(parameter, raw)
                }
            };
            let value = match (expected, row) {
                (Some(dimension), Some(row)) => expression::stored(row, dimension)
                    .and_then(FiniteReal::new)
                    .map(ParameterValue::Number),
                _ => value,
            };
            let property = |key: &str| {
                parameter
                    .properties
                    .iter()
                    .find(|(name, _)| name.as_str() == key)
                    .and_then(|(_, value)| NonBlankString::new(value.clone()))
                    .map(|value| NativeOption::Text(Dimension::Literal(value)))
            };
            let option = |key: &'static str, name| property(key).map(|value| (name, value));
            DesignParameter {
                id: parameter.id.clone(),
                scope: scope.clone(),
                owner: parameter.owner.clone().map(ParameterOwner::Feature),
                position: source
                    .filter(|row| lists[&parameter.owner.as_ref()] == Some(row.list))
                    .and_then(|row| u32::try_from(row.index).ok()),
                name: NonBlankString::new(parameter.name.clone()),
                unit: unit.filter(|unit| {
                    matches!(
                        (&kind, unit.0.first().map(|factor| factor.symbol)),
                        (
                            ValueKind::Quantity(Quantity::LENGTH),
                            Some(
                                UnitSymbol::Mm
                                    | UnitSymbol::Cm
                                    | UnitSymbol::M
                                    | UnitSymbol::In
                                    | UnitSymbol::Ft
                            )
                        ) | (
                            ValueKind::Quantity(Quantity::ANGLE),
                            Some(UnitSymbol::Rad | UnitSymbol::Deg)
                        )
                    )
                }),
                kind,
                definition: ParameterDefinition::Expression(definition),
                value,
                display: None,
                tolerance: None,
                decimals: None,
                comment: ordinal
                    .and_then(|ordinal| {
                        sources.values().find(|source| {
                            source.ordinal == ordinal && source.kind() == Some("assignVariable")
                        })
                    })
                    .and_then(|source| source.tag("description"))
                    .map(str::to_owned),
                native_options: option(
                    "source_kind",
                    cadmpeg_ir::codec_name!("onshape:source_kind"),
                )
                .into_iter()
                .collect(),
                source_settings: option(
                    "display_unit",
                    cadmpeg_ir::codec_name!("onshape:display_unit"),
                )
                .into_iter()
                .chain(raw.and_then(NonBlankString::new).map(|raw| {
                    (
                        cadmpeg_ir::codec_name!("onshape:source_expression"),
                        NativeOption::Text(Dimension::Literal(raw)),
                    )
                }))
                .collect(),
                native_ref: parameter.native_ref.clone(),
            }
        })
        .collect();
    (projected, texts)
}

type Variable<'a> = (&'a ParameterId, expression::Dimension);
type Bindings<'a> = BTreeMap<&'a str, Vec<(usize, Option<Variable<'a>>)>>;

fn variables<'a>(sources: &'a Sources<'_>, parameters: &'a [ModelParameter]) -> Bindings<'a> {
    let by_id: BTreeMap<&str, &ModelParameter> = parameters
        .iter()
        .filter(|parameter| parameter.owner.is_none())
        .map(|parameter| (parameter.id.as_str(), parameter))
        .collect();
    let mut bindings = Bindings::new();
    for source in sources
        .values()
        .filter(|source| source.kind() == Some("assignVariable"))
    {
        if super::super::suppression(source.record) != Ok(false) {
            continue;
        }
        let Some(name) = source.tag("name") else {
            continue;
        };
        let Some(feature) = source.record.get("featureId").and_then(Value::as_str) else {
            continue;
        };
        let binding = by_id
            .get(id("parameter", feature).as_str())
            .and_then(|parameter| {
                let dimension = match source.tag("variableType") {
                    Some("LENGTH") => expression::Dimension::Length,
                    Some("ANGLE") => expression::Dimension::Angle,
                    Some("NUMBER") => expression::Dimension::Scalar,
                    _ => return None,
                };
                Some((&parameter.id, dimension))
            });
        bindings
            .entry(name)
            .or_default()
            .push((source.ordinal, binding));
    }
    for bindings in bindings.values_mut() {
        bindings.sort_unstable_by_key(|(ordinal, _)| *ordinal);
    }
    bindings
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ParameterList {
    Variables,
    FeatureParameters(usize),
    SketchConstraints(usize),
}

struct QuantityRow<'a> {
    row: &'a Value,
    feature_index: usize,
    list: ParameterList,
    index: usize,
}

fn quantity_rows<'a>(sources: &Sources<'a>) -> BTreeMap<String, QuantityRow<'a>> {
    let mut result = BTreeMap::new();
    for source in sources.values() {
        let row = source.record;
        let Some(feature) = row.get("featureId").and_then(Value::as_str) else {
            continue;
        };
        for (index, parameter) in row
            .get("parameters")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let Some(key) = parameter.get("parameterId").and_then(Value::as_str) else {
                continue;
            };
            result.insert(
                id("parameter", &format!("{feature}/{key}")),
                QuantityRow {
                    row: parameter,
                    feature_index: source.ordinal,
                    list: ParameterList::FeatureParameters(source.ordinal),
                    index,
                },
            );
        }
        if source.kind() == Some("assignVariable") {
            let key = match source.tag("variableType") {
                Some("LENGTH") => "lengthValue",
                Some("ANGLE") => "angleValue",
                Some("NUMBER") => "numberValue",
                _ => continue,
            };
            if let Ok(parameter) = features::parameter(row, key) {
                result.insert(
                    id("parameter", feature),
                    QuantityRow {
                        row: parameter,
                        feature_index: source.ordinal,
                        list: ParameterList::Variables,
                        index: source.ordinal,
                    },
                );
            }
        }
        if source.kind() == Some("newSketch") {
            for (index, constraint) in row
                .get("constraints")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                let Some(name) = constraint.get("entityId").and_then(Value::as_str) else {
                    continue;
                };
                let Some(key) = constraint.get("constraintType").and_then(Value::as_str) else {
                    continue;
                };
                let parameter = match key {
                    "LENGTH" => features::parameter(constraint, "length"),
                    "DIAMETER" => features::parameter(constraint, "length")
                        .or_else(|_| features::parameter(constraint, "diameter")),
                    "DISTANCE" => features::parameter(constraint, "distance"),
                    "RADIUS" => features::parameter(constraint, "radius"),
                    "ANGLE" => features::parameter(constraint, "angle"),
                    _ => continue,
                };
                if let Ok(parameter) = parameter {
                    result.insert(
                        id("parameter", &format!("{feature}/{name}")),
                        QuantityRow {
                            row: parameter,
                            feature_index: source.ordinal,
                            list: ParameterList::SketchConstraints(source.ordinal),
                            index,
                        },
                    );
                }
            }
        }
    }
    result
}

fn symbol(unit: &str) -> Option<UnitSymbol> {
    Some(match unit {
        "mm" => UnitSymbol::Mm,
        "cm" => UnitSymbol::Cm,
        "m" => UnitSymbol::M,
        "in" => UnitSymbol::In,
        "ft" => UnitSymbol::Ft,
        "rad" => UnitSymbol::Rad,
        "deg" => UnitSymbol::Deg,
        _ => return None,
    })
}

/// The unit every literal of `tree` states, where its literals state one
/// unit.
fn declared(tree: &Node<ParameterRef>) -> Option<Unit> {
    fn literals<'n>(node: &'n Node<ParameterRef>, units: &mut Vec<&'n Unit>) {
        match node {
            Node::Number { unit, .. } if !unit.0.is_empty() => units.push(unit),
            Node::Unary { operand, .. } => literals(operand, units),
            Node::Sum { first, rest } => {
                literals(first, units);
                for (_, node) in rest {
                    literals(node, units);
                }
            }
            Node::Product { first, rest } => {
                literals(first, units);
                for (_, node) in rest {
                    literals(node, units);
                }
            }
            _ => {}
        }
    }
    let mut units = Vec::new();
    literals(tree, &mut units);
    let (first, rest) = units.split_first()?;
    rest.iter()
        .all(|unit| unit == first)
        .then(|| (*first).clone())
}

/// The source text of `parameter`, with each variable it reads a
/// reference.
fn source(parameter: &ModelParameter, raw: Option<&str>) -> Expression {
    let mut tokens = vec![ExpressionToken::Text(
        raw.unwrap_or(&parameter.expression).to_owned(),
    )];
    tokens.extend(parameter.dependencies.iter().map(|id| {
        ExpressionToken::Reference(ParameterRef {
            target: RefTarget::Parameter(id.clone()),
            through: None,
        })
    }));
    Expression::Source {
        dialect: ExpressionDialect::Native(cadmpeg_core::nonblank_const!("onshape")),
        tokens,
    }
}

pub(super) fn unit(symbol: UnitSymbol) -> Option<Unit> {
    NonZeroI8::new(1).map(|power| Unit(vec![UnitFactor { symbol, power }]))
}

const MAX_DEPTH: usize = 64;

/// Translate the syntax and dimensions of a quantity independently of its evaluation.
fn neutral<'a>(
    text: &str,
    expected: expression::Dimension,
    resolve: &impl Fn(&str) -> Option<Variable<'a>>,
) -> Option<Node<ParameterRef>> {
    let mut parser = Parser {
        rest: text,
        resolve,
        nodes: 0,
    };
    let (tree, dimension) = parser.sum(0)?;
    (dimension == expected && parser.rest.trim().is_empty()).then_some(tree)
}

type Term = (Node<ParameterRef>, expression::Dimension);

struct Parser<'t, 'r, R> {
    rest: &'t str,
    resolve: &'r R,
    nodes: usize,
}

impl<'a, 't, R: Fn(&str) -> Option<Variable<'a>>> Parser<'t, '_, R> {
    fn take(&mut self, prefix: &str) -> bool {
        self.rest = self.rest.trim_start();
        match self.rest.strip_prefix(prefix) {
            Some(rest) => {
                self.rest = rest;
                true
            }
            None => false,
        }
    }

    fn word(&mut self) -> Option<&'t str> {
        self.rest = self.rest.trim_start();
        let end = self
            .rest
            .char_indices()
            .find(|(index, c)| {
                !(c.is_ascii_alphabetic() || *c == '_' || (*index > 0 && c.is_ascii_digit()))
            })
            .map_or(self.rest.len(), |(index, _)| index);
        if end == 0 {
            return None;
        }
        let (word, rest) = self.rest.split_at(end);
        self.rest = rest;
        Some(word)
    }

    fn sum(&mut self, depth: usize) -> Option<Term> {
        let (mut node, dimension) = self.product(depth)?;
        loop {
            let op = if self.take("+") {
                AddOp::Add
            } else if self.take("-") {
                AddOp::Subtract
            } else {
                return Some((node, dimension));
            };
            let (next, actual) = self.product(depth)?;
            if actual != dimension {
                return None;
            }
            node = Node::add(node, op, next);
        }
    }

    fn product(&mut self, depth: usize) -> Option<Term> {
        use expression::Dimension::Scalar;
        let (mut node, mut dimension) = self.atom(depth)?;
        loop {
            let op = if self.take("*") {
                MulOp::Multiply
            } else if self.take("/") {
                MulOp::Divide
            } else {
                return Some((node, dimension));
            };
            let (next, actual) = self.atom(depth)?;
            dimension = match (op, dimension, actual) {
                (MulOp::Multiply, Scalar, other) | (_, other, Scalar) => other,
                (MulOp::Divide, a, b) if a == b => Scalar,
                _ => return None,
            };
            node = Node::mul(node, op, next);
        }
    }

    fn atom(&mut self, depth: usize) -> Option<Term> {
        self.nodes += 1;
        if depth > MAX_DEPTH || self.nodes > super::super::ROWS {
            return None;
        }
        if self.take("+") {
            return self.atom(depth + 1);
        }
        if self.take("-") {
            let (operand, dimension) = self.atom(depth + 1)?;
            return Some((
                Node::Unary {
                    op: UnaryOp::Negate,
                    operand: Box::new(operand),
                },
                dimension,
            ));
        }
        if self.take("(") {
            let term = self.sum(depth + 1)?;
            return self.take(")").then_some(term);
        }
        if self.take("#") {
            let (id, dimension) = (self.resolve)(self.word()?)?;
            return Some((
                Node::Reference(ParameterRef {
                    target: RefTarget::Parameter(id.clone()),
                    through: None,
                }),
                dimension,
            ));
        }
        if self
            .rest
            .starts_with(|c: char| c.is_ascii_digit() || c == '.')
        {
            return self.number();
        }
        let (_, dimension, canonical) = expression::unit(self.word()?)?;
        Some((
            Node::Number {
                value: NonNegativeReal::new(1.0)?,
                unit: unit(symbol(canonical)?)?,
            },
            dimension,
        ))
    }

    fn number(&mut self) -> Option<Term> {
        self.rest = self.rest.trim_start();
        let bytes = self.rest.as_bytes();
        let mut end = 0;
        while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
            end += 1;
        }
        if end < bytes.len() && matches!(bytes[end], b'e' | b'E') {
            end += 1;
            if end < bytes.len() && matches!(bytes[end], b'+' | b'-') {
                end += 1;
            }
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
        }
        let (digits, rest) = self.rest.split_at(end);
        self.rest = rest;
        let value = NonNegativeReal::new(digits.parse::<f64>().ok()?)?;
        let (unit, dimension) = if self
            .rest
            .trim_start()
            .starts_with(|c: char| c.is_ascii_alphabetic())
        {
            let (_, dimension, canonical) = expression::unit(self.word()?)?;
            (unit(symbol(canonical)?)?, dimension)
        } else {
            (Unit(Vec::new()), expression::Dimension::Scalar)
        };
        Some((Node::Number { value, unit }, dimension))
    }
}

#[cfg(test)]
mod tests;
