// SPDX-License-Identifier: Apache-2.0
//! Cloning, comparing, and dropping a tree on a work stack.
//!
//! A tree the expression ceiling admits is several times deeper than the
//! ceiling, since an operand of a two-operand chain or of a comparison nests
//! no level. These walk it without recursion, so its depth does not reach the
//! thread's stack.

use std::mem;

use super::{AddOp, MulOp, Node};

impl<R> Node<R> {
    /// The node's operands, left to right: a ladder's conditions and values
    /// in order, then its `otherwise`.
    pub fn operands(&self) -> impl Iterator<Item = &Self> {
        let mut lead: [Option<&Self>; 2] = [None, None];
        let mut list: &[Self] = &[];
        let mut adds: &[(AddOp, Self)] = &[];
        let mut muls: &[(MulOp, Self)] = &[];
        let mut cases: &[(Self, Self)] = &[];
        let mut last = None;
        match self {
            Self::Unary { operand, .. } => lead = [Some(operand.as_ref()), None],
            Self::Binary { left, right, .. } => lead = [Some(left.as_ref()), Some(right.as_ref())],
            Self::Sum { first, rest } => {
                lead = [Some(first.as_ref()), None];
                adds = rest.as_slice();
            }
            Self::Product { first, rest } => {
                lead = [Some(first.as_ref()), None];
                muls = rest.as_slice();
            }
            Self::All(operands) | Self::Any(operands) | Self::Call { args: operands, .. } => {
                list = operands.as_slice();
            }
            Self::If {
                cases: ladder,
                otherwise,
            } => {
                cases = ladder.as_slice();
                last = Some(otherwise.as_ref());
            }
            Self::Number { .. }
            | Self::Constant(_)
            | Self::Boolean(_)
            | Self::Text(_)
            | Self::Reference(_)
            | Self::Measure(_)
            | Self::LawParameter
            | Self::Bend(_) => {}
        }
        lead.into_iter()
            .flatten()
            .chain(list)
            .chain(adds.iter().map(|(_, operand)| operand))
            .chain(muls.iter().map(|(_, operand)| operand))
            .chain(
                cases
                    .iter()
                    .flat_map(|(condition, value)| [condition, value]),
            )
            .chain(last)
    }

    /// The node's operands, left to right, to change in place.
    pub(crate) fn operands_mut(&mut self) -> impl Iterator<Item = &mut Self> {
        let mut lead: [Option<&mut Self>; 2] = [None, None];
        let mut list: &mut [Self] = &mut [];
        let mut adds: &mut [(AddOp, Self)] = &mut [];
        let mut muls: &mut [(MulOp, Self)] = &mut [];
        let mut cases: &mut [(Self, Self)] = &mut [];
        let mut last = None;
        match self {
            Self::Unary { operand, .. } => lead = [Some(operand.as_mut()), None],
            Self::Binary { left, right, .. } => lead = [Some(left.as_mut()), Some(right.as_mut())],
            Self::Sum { first, rest } => {
                lead = [Some(first.as_mut()), None];
                adds = rest.as_mut_slice();
            }
            Self::Product { first, rest } => {
                lead = [Some(first.as_mut()), None];
                muls = rest.as_mut_slice();
            }
            Self::All(operands) | Self::Any(operands) | Self::Call { args: operands, .. } => {
                list = operands.as_mut_slice();
            }
            Self::If {
                cases: ladder,
                otherwise,
            } => {
                cases = ladder.as_mut_slice();
                last = Some(otherwise.as_mut());
            }
            Self::Number { .. }
            | Self::Constant(_)
            | Self::Boolean(_)
            | Self::Text(_)
            | Self::Reference(_)
            | Self::Measure(_)
            | Self::LawParameter
            | Self::Bend(_) => {}
        }
        lead.into_iter()
            .flatten()
            .chain(list.iter_mut())
            .chain(adds.iter_mut().map(|(_, operand)| operand))
            .chain(muls.iter_mut().map(|(_, operand)| operand))
            .chain(
                cases
                    .iter_mut()
                    .flat_map(|(condition, value)| [condition, value]),
            )
            .chain(last)
    }
}

/// A leaf that stands in an operand's place until the operand is cloned in,
/// or after it is moved out to be dropped.
const fn placeholder<R>() -> Node<R> {
    Node::LawParameter
}

impl<R: Clone> Node<R> {
    /// The node with a placeholder in place of each operand.
    fn shallow(&self) -> Self {
        let operand = || Box::new(placeholder());
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
                operand: operand(),
            },
            Self::Binary { op, .. } => Self::Binary {
                op: *op,
                left: operand(),
                right: operand(),
            },
            Self::Sum { rest, .. } => Self::Sum {
                first: operand(),
                rest: rest.iter().map(|(op, _)| (*op, placeholder())).collect(),
            },
            Self::Product { rest, .. } => Self::Product {
                first: operand(),
                rest: rest.iter().map(|(op, _)| (*op, placeholder())).collect(),
            },
            Self::All(operands) => Self::All(operands.iter().map(|_| placeholder()).collect()),
            Self::Any(operands) => Self::Any(operands.iter().map(|_| placeholder()).collect()),
            Self::Call { function, args } => Self::Call {
                function: *function,
                args: args.iter().map(|_| placeholder()).collect(),
            },
            Self::If { cases, .. } => Self::If {
                cases: cases
                    .iter()
                    .map(|_| (placeholder(), placeholder()))
                    .collect(),
                otherwise: operand(),
            },
        }
    }
}

impl<R: Clone> Clone for Node<R> {
    /// Clones each node with placeholder operands, then clones each operand
    /// into its placeholder.
    fn clone(&self) -> Self {
        let mut root = self.shallow();
        let mut work: Vec<_> = self.operands().zip(root.operands_mut()).collect();
        while let Some((from, to)) = work.pop() {
            *to = from.shallow();
            work.extend(from.operands().zip(to.operands_mut()));
        }
        root
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
                (Self::Sum { rest, .. }, Self::Sum { rest: rest_b, .. }) => {
                    rest.len() == rest_b.len()
                        && rest.iter().zip(rest_b).all(|((a, _), (b, _))| a == b)
                }
                (Self::Product { rest, .. }, Self::Product { rest: rest_b, .. }) => {
                    rest.len() == rest_b.len()
                        && rest.iter().zip(rest_b).all(|((a, _), (b, _))| a == b)
                }
                (Self::All(a), Self::All(b)) | (Self::Any(a), Self::Any(b)) => a.len() == b.len(),
                (Self::If { cases, .. }, Self::If { cases: cases_b, .. }) => {
                    cases.len() == cases_b.len()
                }
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
                    | Self::Sum { .. }
                    | Self::Product { .. }
                    | Self::All(_)
                    | Self::Any(_)
                    | Self::Call { .. }
                    | Self::If { .. },
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
    /// Moves each operand out to a work stack before the node drops, so no
    /// node drops with an operand that is not a leaf.
    fn drop(&mut self) {
        let mut work: Vec<_> = self
            .operands_mut()
            .map(|operand| mem::replace(operand, placeholder()))
            .collect();
        while let Some(mut node) = work.pop() {
            work.extend(
                node.operands_mut()
                    .map(|operand| mem::replace(operand, placeholder())),
            );
        }
    }
}
