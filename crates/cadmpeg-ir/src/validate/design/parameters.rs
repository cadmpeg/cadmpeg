// SPDX-License-Identifier: Apache-2.0
//! The parameter references an expression reads (§5.12, §5.13).

use crate::design::dimension::ParameterRef;
use crate::design::expr::{Expression, ExpressionToken, Neutral, Node};

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
