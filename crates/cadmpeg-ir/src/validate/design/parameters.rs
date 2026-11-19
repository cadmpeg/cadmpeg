// SPDX-License-Identifier: Apache-2.0
//! A design parameter's Boolean value in a configuration (§5.11, §5.12), where
//! a stated value or in-IR arithmetic decides it.

use std::collections::BTreeSet;

use super::records::DesignRecords;
use crate::design::configuration::{DesignConfiguration, ParameterOverride};
use crate::design::dimension::{ParameterRef, RefTarget};
use crate::design::expr::{BinaryOp, Expression, ExpressionToken, Neutral, Node, UnaryOp};
use crate::design::parameter::{ParameterDefinition, ParameterValue};
use crate::features::ParameterId;

/// The Boolean value of `parameter` in the configuration whose chain,
/// nearest first, is `chain`. `None` where no stated value and no in-IR
/// arithmetic decides it.
///
/// The nearest override that states the parameter decides: its `value`, the
/// cache of its expression, else its `expression`. With no override, the
/// parameter's cached value holds where its expression reads no parameter
/// the chain overrides; otherwise its expression decides.
pub(super) fn boolean_in(
    index: &DesignRecords<'_>,
    chain: &[&DesignConfiguration],
    parameter: &ParameterId,
) -> Option<bool> {
    value_in(index, chain, parameter, true)
}

/// The value of `parameter` in the chain. With `evaluate` false, an
/// expression is not evaluated and leaves the value undecided.
fn value_in(
    index: &DesignRecords<'_>,
    chain: &[&DesignConfiguration],
    parameter: &ParameterId,
    evaluate: bool,
) -> Option<bool> {
    if let Some(stated) = nearest_override(chain, parameter) {
        return match (&stated.value, &stated.expression) {
            (Some(value), _) => boolean(value),
            (None, Some(expression)) if evaluate => evaluate_in(index, chain, expression),
            (None, _) => None,
        };
    }
    let record = index.parameters.get(parameter.as_str())?;
    let expression = match &record.definition {
        ParameterDefinition::Expression(expression) => Some(expression),
        ParameterDefinition::Driven | ParameterDefinition::Assigned(_) => None,
    };
    if !reads_override(index, chain, parameter) {
        if let Some(value) = &record.value {
            return boolean(value);
        }
    }
    expression
        .filter(|_| evaluate)
        .and_then(|expression| evaluate_in(index, chain, expression))
}

fn nearest_override<'a>(
    chain: &[&'a DesignConfiguration],
    parameter: &ParameterId,
) -> Option<&'a ParameterOverride> {
    chain
        .iter()
        .filter_map(|configuration| configuration.overrides.parameters.get(parameter))
        .find(|stated| stated.value.is_some() || stated.expression.is_some())
}

const fn boolean(value: &ParameterValue) -> Option<bool> {
    match value {
        ParameterValue::Boolean(value) => Some(*value),
        ParameterValue::Number(_) | ParameterValue::Integer(_) | ParameterValue::Text(_) => None,
    }
}

/// Whether the expression of `parameter`, through the expressions of the
/// parameters it reads, reads a parameter the chain overrides, or one whose
/// value the chain does not decide (another scope's, or an external one).
fn reads_override(
    index: &DesignRecords<'_>,
    chain: &[&DesignConfiguration],
    parameter: &ParameterId,
) -> bool {
    if chain
        .iter()
        .all(|configuration| configuration.overrides.parameters.is_empty())
    {
        return false;
    }
    let mut seen = BTreeSet::from([parameter]);
    let mut pending = vec![parameter];
    while let Some(parameter) = pending.pop() {
        let Some(ParameterDefinition::Expression(expression)) = index
            .parameters
            .get(parameter.as_str())
            .map(|record| &record.definition)
        else {
            continue;
        };
        for reference in references(expression) {
            let RefTarget::Parameter(target) = &reference.target else {
                return true;
            };
            if reference.through.is_some() || nearest_override(chain, target).is_some() {
                return true;
            }
            if seen.insert(target) {
                pending.push(target);
            }
        }
    }
    false
}

/// The parameter references of an expression, walked without recursion.
pub(super) fn references(expression: &Expression) -> Vec<&ParameterRef> {
    match expression {
        Expression::Neutral(Neutral(root)) => node_references(root),
        Expression::Source { tokens, .. } => tokens
            .iter()
            .filter_map(|token| match token {
                ExpressionToken::Reference(reference) => Some(reference),
                ExpressionToken::Text(_) => None,
            })
            .collect(),
    }
}

/// The parameter references of a neutral tree, walked without recursion.
pub(super) fn node_references<R>(root: &Node<R>) -> Vec<&R> {
    let mut found = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        match node {
            Node::Reference(reference) => found.push(reference),
            Node::Unary { operand, .. } => pending.push(operand),
            Node::Binary { left, right, .. } => pending.extend([&**left, &**right]),
            Node::Call { args, .. } => pending.extend(args),
            Node::If(branches) => pending.extend(branches.iter()),
            Node::Number { .. }
            | Node::Constant(_)
            | Node::Boolean(_)
            | Node::Text(_)
            | Node::Measure(_)
            | Node::LawParameter
            | Node::Bend(_) => {}
        }
    }
    found
}

/// The Boolean value of a neutral expression in the chain, walked without
/// recursion. A reference reads the referenced parameter's stated value.
fn evaluate_in(
    index: &DesignRecords<'_>,
    chain: &[&DesignConfiguration],
    expression: &Expression,
) -> Option<bool> {
    enum Task<'n> {
        Visit(&'n Node<ParameterRef>),
        Apply(&'n Node<ParameterRef>),
    }
    let Expression::Neutral(Neutral(root)) = expression else {
        return None;
    };
    let mut tasks = vec![Task::Visit(root)];
    let mut values: Vec<Option<bool>> = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Visit(node) => match node {
                Node::Boolean(value) => values.push(Some(*value)),
                Node::Reference(reference) => values.push(match &reference.target {
                    RefTarget::Parameter(target) if reference.through.is_none() => {
                        value_in(index, chain, target, false)
                    }
                    RefTarget::Parameter(_) | RefTarget::External { .. } => None,
                }),
                Node::Unary {
                    op: UnaryOp::Not,
                    operand,
                } => {
                    tasks.push(Task::Apply(node));
                    tasks.push(Task::Visit(operand));
                }
                Node::Binary {
                    op: BinaryOp::And | BinaryOp::Or | BinaryOp::Equal | BinaryOp::NotEqual,
                    left,
                    right,
                } => {
                    tasks.push(Task::Apply(node));
                    tasks.push(Task::Visit(right));
                    tasks.push(Task::Visit(left));
                }
                Node::If(branches) => {
                    tasks.push(Task::Apply(node));
                    tasks.extend(branches.iter().rev().map(Task::Visit));
                }
                _ => values.push(None),
            },
            Task::Apply(node) => {
                let value = match node {
                    Node::Unary { .. } => values.pop()?.map(|value| !value),
                    Node::Binary { op, .. } => {
                        let right = values.pop()?;
                        let left = values.pop()?;
                        combine(*op, left, right)
                    }
                    Node::If(_) => {
                        let otherwise = values.pop()?;
                        let then = values.pop()?;
                        match values.pop()? {
                            Some(true) => then,
                            Some(false) => otherwise,
                            None => None,
                        }
                    }
                    _ => None,
                };
                values.push(value);
            }
        }
    }
    values.pop().flatten()
}

/// A Boolean operation, decided only where both operands are Booleans with
/// a value: `and` and `or` evaluate both operands (§5.13), so an undecided
/// operand leaves the result undecided.
fn combine(op: BinaryOp, left: Option<bool>, right: Option<bool>) -> Option<bool> {
    let (left, right) = (left?, right?);
    match op {
        BinaryOp::And => Some(left && right),
        BinaryOp::Or => Some(left || right),
        BinaryOp::Equal => Some(left == right),
        BinaryOp::NotEqual => Some(left != right),
        BinaryOp::Add
        | BinaryOp::Subtract
        | BinaryOp::Multiply
        | BinaryOp::Divide
        | BinaryOp::Power
        | BinaryOp::Less
        | BinaryOp::LessEqual
        | BinaryOp::Greater
        | BinaryOp::GreaterEqual => None,
    }
}
