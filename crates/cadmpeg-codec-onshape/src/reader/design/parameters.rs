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
) -> (Vec<DesignParameter>, Vec<ParameterId>) {
    let variables: BTreeMap<&str, &ParameterId> = parameters
        .iter()
        .filter(|parameter| parameter.owner.is_none())
        .map(|parameter| (parameter.name.as_str(), &parameter.id))
        .collect();
    let mut texts = Vec::new();
    let projected = parameters
        .iter()
        .map(|parameter| {
            let tree = neutral(&parameter.expression, &variables);
            let unit = tree.as_ref().and_then(declared);
            let definition = match tree {
                Some(tree) => Expression::Neutral(Neutral(tree)),
                None => {
                    texts.push(parameter.id.clone());
                    source(parameter)
                }
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
                None => (ValueKind::Quantity(Quantity::REAL), None),
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
                comment: None,
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
                .collect(),
                native_ref: parameter.native_ref.clone(),
            }
        })
        .collect();
    (projected, texts)
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
fn source(parameter: &ModelParameter) -> Expression {
    let text = parameter
        .properties
        .iter()
        .find(|(key, _)| key.as_str() == "source_expression")
        .map_or(&parameter.expression, |(_, text)| text);
    let mut tokens = vec![ExpressionToken::Text(text.clone())];
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

/// The neutral tree of an expression the reader rendered: a number with an
/// optional unit, a variable name, `(-e)`, or `(a op b)`.
fn neutral(text: &str, variables: &BTreeMap<&str, &ParameterId>) -> Option<Node<ParameterRef>> {
    let mut parser = Rendered {
        rest: text,
        variables,
        depth: 0,
    };
    let tree = parser.node()?;
    parser.rest.trim().is_empty().then_some(tree)
}

/// A nesting bound past every expression the reader admits.
const MAX_DEPTH: usize = 256;

struct Rendered<'t, 'v> {
    rest: &'t str,
    variables: &'v BTreeMap<&'v str, &'v ParameterId>,
    depth: usize,
}

impl<'t> Rendered<'t, '_> {
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

    fn node(&mut self) -> Option<Node<ParameterRef>> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return None;
        }
        let node = if self.take("(-") {
            let operand = self.node()?;
            Node::Unary {
                op: UnaryOp::Negate,
                operand: Box::new(operand),
            }
        } else if self.take("(") {
            let left = self.node()?;
            if self.take("+") {
                Node::add(left, AddOp::Add, self.node()?)
            } else if self.take("-") {
                Node::add(left, AddOp::Subtract, self.node()?)
            } else if self.take("*") {
                Node::mul(left, MulOp::Multiply, self.node()?)
            } else if self.take("/") {
                Node::mul(left, MulOp::Divide, self.node()?)
            } else {
                return None;
            }
        } else if self
            .rest
            .trim_start()
            .starts_with(|c: char| c.is_ascii_digit() || c == '.')
        {
            self.number()?
        } else {
            let name = self.word()?;
            Node::Reference(ParameterRef {
                target: RefTarget::Parameter((*self.variables.get(name)?).clone()),
                through: None,
            })
        };
        if matches!(
            node,
            Node::Unary { .. } | Node::Sum { .. } | Node::Product { .. }
        ) && !self.take(")")
        {
            return None;
        }
        self.depth -= 1;
        Some(node)
    }

    fn number(&mut self) -> Option<Node<ParameterRef>> {
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
        let unit = if self
            .rest
            .trim_start()
            .starts_with(|c: char| c.is_ascii_alphabetic())
        {
            let symbol = match self.word()? {
                "mm" => UnitSymbol::Mm,
                "cm" => UnitSymbol::Cm,
                "m" => UnitSymbol::M,
                "in" => UnitSymbol::In,
                "ft" => UnitSymbol::Ft,
                "rad" => UnitSymbol::Rad,
                "deg" => UnitSymbol::Deg,
                _ => return None,
            };
            Unit(vec![UnitFactor {
                symbol,
                power: NonZeroI8::new(1)?,
            }])
        } else {
            Unit(Vec::new())
        };
        Some(Node::Number { value, unit })
    }
}

#[cfg(test)]
mod tests;
