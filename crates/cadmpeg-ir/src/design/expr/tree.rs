// SPDX-License-Identifier: Apache-2.0
//! Cloning, comparing, and dropping a tree on a work stack.
//!
//! A tree the expression ceiling admits is several times deeper than the
//! ceiling, since an operand of a two-operand chain or of a comparison nests
//! no level. These walk it without recursion, so its depth does not reach the
//! thread's stack.

use std::mem;

use super::Node;

impl<R> Node<R> {
    /// The node's operands, left to right.
    fn operands(&self) -> impl DoubleEndedIterator<Item = &Self> {
        let (pair, rest): ([Option<&Self>; 2], &[Self]) = match self {
            Self::Unary { operand, .. } => ([Some(operand.as_ref()), None], &[]),
            Self::Binary { left, right, .. } => ([Some(left.as_ref()), Some(right.as_ref())], &[]),
            Self::Call { args, .. } => ([None, None], args),
            Self::If(branches) => ([None, None], branches.as_slice()),
            Self::Number { .. }
            | Self::Constant(_)
            | Self::Boolean(_)
            | Self::Text(_)
            | Self::Reference(_)
            | Self::Measure(_)
            | Self::LawParameter
            | Self::Bend(_) => ([None, None], &[]),
        };
        pair.into_iter().flatten().chain(rest)
    }

    /// Moves the node's operands onto `out`, leaving a leaf in each place.
    fn take_operands(&mut self, out: &mut Vec<Self>) {
        let mut take = |node: &mut Self| out.push(mem::replace(node, Self::LawParameter));
        match self {
            Self::Unary { operand, .. } => take(operand.as_mut()),
            Self::Binary { left, right, .. } => {
                take(left.as_mut());
                take(right.as_mut());
            }
            Self::Call { args, .. } => out.append(args),
            Self::If(branches) => branches.iter_mut().for_each(take),
            Self::Number { .. }
            | Self::Constant(_)
            | Self::Boolean(_)
            | Self::Text(_)
            | Self::Reference(_)
            | Self::Measure(_)
            | Self::LawParameter
            | Self::Bend(_) => {}
        }
    }
}

impl<R: Clone> Node<R> {
    /// The node with `operands`, its operands' clones left to right, in
    /// place of its own.
    fn with_operands(&self, operands: Vec<Self>) -> Self {
        let mut operands = operands.into_iter();
        let mut next = || {
            operands
                .next()
                .expect("a clone is given one built operand for each operand")
        };
        match self {
            Self::Number { value, unit } => Self::Number {
                value: *value,
                unit: unit.clone(),
            },
            Self::Constant(constant) => Self::Constant(*constant),
            Self::Boolean(value) => Self::Boolean(*value),
            Self::Text(text) => Self::Text(text.clone()),
            Self::Reference(reference) => Self::Reference(reference.clone()),
            Self::Measure(measure) => Self::Measure(measure.clone()),
            Self::LawParameter => Self::LawParameter,
            Self::Bend(variable) => Self::Bend(*variable),
            Self::Unary { op, .. } => Self::Unary {
                op: *op,
                operand: Box::new(next()),
            },
            Self::Binary { op, .. } => {
                let left = Box::new(next());
                Self::Binary {
                    op: *op,
                    left,
                    right: Box::new(next()),
                }
            }
            Self::Call { function, .. } => Self::Call {
                function: *function,
                args: operands.collect(),
            },
            Self::If(_) => {
                let condition = next();
                let then = next();
                Self::If(Box::new([condition, then, next()]))
            }
        }
    }
}

impl<R: Clone> Clone for Node<R> {
    /// Each node is visited twice: first to clone its operands, then, with
    /// their clones built, to build its own.
    fn clone(&self) -> Self {
        let mut work = vec![(self, false)];
        let mut built = Vec::new();
        while let Some((node, operands_built)) = work.pop() {
            if operands_built {
                let at = built.len() - node.operands().count();
                let operands = built.split_off(at);
                built.push(node.with_operands(operands));
            } else {
                work.push((node, true));
                work.extend(node.operands().rev().map(|operand| (operand, false)));
            }
        }
        built
            .pop()
            .expect("a clone builds the root after its operands")
    }
}

impl<R: PartialEq> PartialEq for Node<R> {
    fn eq(&self, other: &Self) -> bool {
        let mut work = vec![(self, other)];
        while let Some((a, b)) = work.pop() {
            let same = match (a, b) {
                (
                    Self::Number { value, unit },
                    Self::Number {
                        value: value_b,
                        unit: unit_b,
                    },
                ) => value == value_b && unit == unit_b,
                (Self::Constant(a), Self::Constant(b)) => a == b,
                (Self::Boolean(a), Self::Boolean(b)) => a == b,
                (Self::Text(a), Self::Text(b)) => a == b,
                (Self::Reference(a), Self::Reference(b)) => a == b,
                (Self::Measure(a), Self::Measure(b)) => a == b,
                (Self::LawParameter, Self::LawParameter) => true,
                (Self::Bend(a), Self::Bend(b)) => a == b,
                (Self::Unary { op, .. }, Self::Unary { op: op_b, .. }) => op == op_b,
                (Self::Binary { op, .. }, Self::Binary { op: op_b, .. }) => op == op_b,
                (
                    Self::Call { function, args },
                    Self::Call {
                        function: function_b,
                        args: args_b,
                    },
                ) => function == function_b && args.len() == args_b.len(),
                (Self::If(_), Self::If(_)) => true,
                (
                    Self::Number { .. }
                    | Self::Constant(_)
                    | Self::Boolean(_)
                    | Self::Text(_)
                    | Self::Reference(_)
                    | Self::Measure(_)
                    | Self::LawParameter
                    | Self::Bend(_)
                    | Self::Unary { .. }
                    | Self::Binary { .. }
                    | Self::Call { .. }
                    | Self::If(_),
                    _,
                ) => false,
            };
            if !same {
                return false;
            }
            work.extend(a.operands().zip(b.operands()));
        }
        true
    }
}

impl<R> Drop for Node<R> {
    fn drop(&mut self) {
        let mut work = Vec::new();
        self.take_operands(&mut work);
        while let Some(mut node) = work.pop() {
            node.take_operands(&mut work);
        }
    }
}
