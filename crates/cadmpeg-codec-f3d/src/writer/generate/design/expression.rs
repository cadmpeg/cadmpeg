// SPDX-License-Identifier: Apache-2.0
//! Fusion parameter expression text for the subset of a neutral expression
//! tree whose Fusion form is established: literals in Fusion's length and
//! angle units, parameter references, negation, the four arithmetic
//! operations, and π.

use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{format_number, BinaryOp, Constant, ExpressionToken, Node, UnaryOp};
use cadmpeg_ir::design::units::{Quantity, Unit, UnitSymbol};
use cadmpeg_ir::features::ParameterId;

/// The unit symbols Fusion spells as `UnitSymbol::name` does.
const FUSION_UNITS: [UnitSymbol; 7] = [
    UnitSymbol::Mm,
    UnitSymbol::Cm,
    UnitSymbol::M,
    UnitSymbol::In,
    UnitSymbol::Ft,
    UnitSymbol::Deg,
    UnitSymbol::Rad,
];

/// The Fusion unit `unit` states for a parameter of `quantity`: one factor of
/// power 1 that Fusion spells.
pub(super) fn fusion_unit(unit: &Unit, quantity: Quantity) -> Option<UnitSymbol> {
    let [factor] = unit.0.as_slice() else {
        return None;
    };
    (factor.power.get() == 1
        && FUSION_UNITS.contains(&factor.symbol)
        && factor.symbol.canonical().1 == quantity)
        .then_some(factor.symbol)
}

/// The parameter a reference names in this document, read through no
/// occurrence path.
pub(super) fn referenced(reference: &ParameterRef) -> Option<&ParameterId> {
    match (&reference.target, &reference.through) {
        (RefTarget::Parameter(id), None) => Some(id),
        _ => None,
    }
}

/// Every parameter reference of `node`, in reading order.
pub(super) fn references<'a>(node: &'a Node<ParameterRef>, out: &mut Vec<&'a ParameterRef>) {
    match node {
        Node::Reference(reference) => out.push(reference),
        Node::Unary { operand, .. } => references(operand, out),
        Node::Binary { left, right, .. } => {
            references(left, out);
            references(right, out);
        }
        Node::Call { args, .. } => args.iter().for_each(|arg| references(arg, out)),
        Node::If(branches) => branches.iter().for_each(|branch| references(branch, out)),
        Node::Number { .. }
        | Node::Constant(_)
        | Node::Boolean(_)
        | Node::Text(_)
        | Node::Measure(_)
        | Node::LawParameter
        | Node::Bend(_) => {}
    }
}

/// The Fusion text of `node`, with each reference written as `name` gives
/// it. `None` where the tree holds a construct outside the subset or a
/// reference `name` does not give.
pub(super) fn print<'a>(
    node: &Node<ParameterRef>,
    name: &impl Fn(&ParameterId) -> Option<&'a str>,
) -> Option<String> {
    match node {
        Node::Number { value, unit } => literal(value.get(), unit),
        Node::Constant(Constant::Pi) => Some("PI".into()),
        Node::Reference(reference) => name(referenced(reference)?).map(str::to_owned),
        Node::Unary {
            op: UnaryOp::Negate,
            operand,
        } => Some(format!("-({})", print(operand, name)?)),
        Node::Binary { op, left, right } => {
            let op = arithmetic(*op)?;
            Some(format!(
                "({} {op} {})",
                print(left, name)?,
                print(right, name)?
            ))
        }
        _ => None,
    }
}

/// The value of `node` in canonical units, over the subset [`print`] writes,
/// with each reference's value as `value` gives it.
pub(super) fn evaluate(
    node: &Node<ParameterRef>,
    value: &impl Fn(&ParameterId) -> Option<f64>,
) -> Option<f64> {
    let result = match node {
        Node::Number { value, unit } => value.get() * canonical_factor(unit)?,
        Node::Constant(Constant::Pi) => std::f64::consts::PI,
        Node::Reference(reference) => value(referenced(reference)?)?,
        Node::Unary {
            op: UnaryOp::Negate,
            operand,
        } => -evaluate(operand, value)?,
        Node::Binary { op, left, right } => {
            let (left, right) = (evaluate(left, value)?, evaluate(right, value)?);
            match op {
                BinaryOp::Add => left + right,
                BinaryOp::Subtract => left - right,
                BinaryOp::Multiply => left * right,
                BinaryOp::Divide => left / right,
                _ => return None,
            }
        }
        _ => return None,
    };
    result.is_finite().then_some(result)
}

/// The Fusion text of source tokens in the Fusion dialect, with each
/// reference written as `name` gives it.
pub(super) fn print_tokens<'a>(
    tokens: &[ExpressionToken<ParameterRef>],
    name: &impl Fn(&ParameterId) -> Option<&'a str>,
) -> Option<String> {
    let mut text = String::new();
    for token in tokens {
        match token {
            ExpressionToken::Text(part) => text.push_str(part),
            ExpressionToken::Reference(reference) => text.push_str(name(referenced(reference)?)?),
        }
    }
    (!text.trim().is_empty()).then_some(text)
}

/// A literal of `value` canonical units of `quantity`, in `unit`.
pub(super) fn quantity_literal(value: f64, unit: Option<UnitSymbol>) -> Option<String> {
    let factor = unit.map_or(1.0, |unit| unit.canonical().0);
    let number = value / factor;
    let text = format_number(number.abs()).ok()?;
    let sign = if number < 0.0 { "-" } else { "" };
    Some(match unit {
        Some(unit) => format!("{sign}{text} {}", unit.name()),
        None => format!("{sign}{text}"),
    })
}

fn literal(value: f64, unit: &Unit) -> Option<String> {
    match unit.0.as_slice() {
        [] => format_number(value).ok(),
        [factor] if factor.power.get() == 1 => {
            let (scale, quantity) = factor.symbol.canonical();
            if FUSION_UNITS.contains(&factor.symbol) {
                Some(format!(
                    "{} {}",
                    format_number(value).ok()?,
                    factor.symbol.name()
                ))
            } else if quantity == Quantity::LENGTH {
                quantity_literal(value * scale, Some(UnitSymbol::Mm))
            } else if quantity == Quantity::ANGLE {
                quantity_literal(value * scale, Some(UnitSymbol::Rad))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The factor from `unit` to the canonical unit, for a dimensionless unit
/// or one factor of power 1.
fn canonical_factor(unit: &Unit) -> Option<f64> {
    match unit.0.as_slice() {
        [] => Some(1.0),
        [factor] if factor.power.get() == 1 => Some(factor.symbol.canonical().0),
        _ => None,
    }
}

fn arithmetic(op: BinaryOp) -> Option<&'static str> {
    match op {
        BinaryOp::Add => Some("+"),
        BinaryOp::Subtract => Some("-"),
        BinaryOp::Multiply => Some("*"),
        BinaryOp::Divide => Some("/"),
        _ => None,
    }
}
