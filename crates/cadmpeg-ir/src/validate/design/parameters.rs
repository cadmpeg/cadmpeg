// SPDX-License-Identifier: Apache-2.0
//! A design parameter's Boolean value in a configuration (§5.11, §5.12), where
//! a stated value or in-IR arithmetic decides it.

use super::records::DesignRecords;
use crate::design::configuration::{
    own_value_holds, DesignConfiguration, ParameterOverride, Stated,
};
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
/// parameter's cached value holds where its definition reads nothing that
/// may differ in the chain's configuration (§5.13); otherwise its expression
/// decides.
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
        ParameterDefinition::Driven
        | ParameterDefinition::Assigned(_)
        | ParameterDefinition::Unset => None,
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

/// Whether the cached value of `parameter` may differ in the chain's
/// configuration from the base (§5.13, [`own_value_holds`]).
fn reads_override(
    index: &DesignRecords<'_>,
    chain: &[&DesignConfiguration],
    parameter: &ParameterId,
) -> bool {
    let Some(record) = index.parameters.get(parameter.as_str()) else {
        return true;
    };
    !own_value_holds(
        record,
        Stated::of(chain),
        &|target| nearest_override(chain, target).is_some(),
        &|target| index.parameters.get(target.as_str()).copied(),
        &|program| {
            index
                .ir
                .design
                .programs
                .iter()
                .find(|candidate| &candidate.id == program)
        },
    )
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
            Node::Unary { .. }
            | Node::Binary { .. }
            | Node::Sum { .. }
            | Node::Product { .. }
            | Node::All(_)
            | Node::Any(_)
            | Node::Call { .. }
            | Node::If { .. } => pending.extend(node.operands()),
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
                    op: BinaryOp::Equal | BinaryOp::NotEqual,
                    ..
                }
                | Node::All(_)
                | Node::Any(_)
                | Node::If { .. } => {
                    tasks.push(Task::Apply(node));
                    let operands: Vec<_> = node.operands().collect();
                    tasks.extend(operands.into_iter().rev().map(Task::Visit));
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
                    Node::All(operands) | Node::Any(operands) => {
                        let start = values.len().checked_sub(operands.len())?;
                        let all = matches!(node, Node::All(_));
                        values
                            .split_off(start)
                            .into_iter()
                            .try_fold(all, |held, value| {
                                value.map(|value| if all { held && value } else { held || value })
                            })
                    }
                    Node::If { cases, .. } => {
                        let start = values.len().checked_sub(2 * cases.len() + 1)?;
                        let ladder = values.split_off(start);
                        let (otherwise, cases) = ladder.split_last()?;
                        let mut taken = *otherwise;
                        for case in cases.chunks(2) {
                            match case {
                                [Some(true), value] => {
                                    taken = *value;
                                    break;
                                }
                                [Some(false), _] => {}
                                _ => {
                                    taken = None;
                                    break;
                                }
                            }
                        }
                        taken
                    }
                    _ => None,
                };
                values.push(value);
            }
        }
    }
    values.pop().flatten()
}

/// A Boolean comparison, decided only where both operands are Booleans with
/// a value. `and` and `or` evaluate every operand (§5.13), so an undecided
/// operand of a run leaves the run undecided as well.
fn combine(op: BinaryOp, left: Option<bool>, right: Option<bool>) -> Option<bool> {
    let (left, right) = (left?, right?);
    match op {
        BinaryOp::Equal => Some(left == right),
        BinaryOp::NotEqual => Some(left != right),
        BinaryOp::Power
        | BinaryOp::Less
        | BinaryOp::LessEqual
        | BinaryOp::Greater
        | BinaryOp::GreaterEqual => None,
    }
}
