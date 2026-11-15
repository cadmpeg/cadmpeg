// SPDX-License-Identifier: Apache-2.0
//! Printing a tree as canonical expression text.

use super::super::units::Unit;
use super::{BendVariable, BinaryOp, Constant, ExprError, Measure, NeutralWire, Node, UnaryOp};

fn malformed<T>(message: impl Into<String>) -> Result<T, ExprError> {
    Err(ExprError::Malformed(message.into()))
}

fn index(value: impl TryInto<usize>) -> Result<usize, ExprError> {
    value
        .try_into()
        .map_err(|_| ExprError::Malformed("a digit position is out of range".into()))
}

/// Prints a number as the shortest digit string that reads back to the same
/// value, in the form of ECMAScript's `Number.prototype.toString`: fixed
/// notation from 10⁻⁶ up to and excluding 10²¹, and exponent notation with a
/// signed exponent outside it.
///
/// # Errors
///
/// Returns [`ExprError::Malformed`] for a value that is not finite or is
/// negative: a number has no sign of its own.
pub fn format_number(value: f64) -> Result<String, ExprError> {
    if !value.is_finite() {
        return malformed("a number is finite");
    }
    if value < 0.0 {
        return malformed(format!(
            "{value} has no canonical text: a number has no sign of its own, and `-` applied to \
             it reads back as Unary(Negate, Number)"
        ));
    }
    if value == 0.0 {
        return Ok("0".into());
    }
    let scientific = format!("{value:e}");
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return malformed(format!("{scientific} states no exponent"));
    };
    let exponent: i64 = exponent
        .parse()
        .map_err(|_| ExprError::Malformed(format!("{scientific} states no integer exponent")))?;
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = i64::try_from(digits.len())
        .map_err(|_| ExprError::Malformed("a number has too many digits".into()))?;
    let n = exponent + 1;
    Ok(if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat(index(n - k)?))
    } else if 0 < n && n <= 21 {
        let (whole, fraction) = digits.split_at(index(n)?);
        format!("{whole}.{fraction}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat(index(-n)?))
    } else {
        let e = n - 1;
        let (first, rest) = digits.split_at(1);
        let mantissa = if rest.is_empty() {
            first.to_owned()
        } else {
            format!("{first}.{rest}")
        };
        format!("{mantissa}e{}{}", if e > 0 { '+' } else { '-' }, e.abs())
    })
}

fn format_unit(unit: &Unit) -> String {
    let mut out = String::new();
    for (i, factor) in unit.0.iter().enumerate() {
        let power = i32::from(factor.power.get());
        let magnitude = if i == 0 {
            power
        } else {
            out.push(if power > 0 { '*' } else { '/' });
            power.abs()
        };
        out.push_str(factor.symbol.name());
        if magnitude != 1 {
            out.push('^');
            out.push_str(&magnitude.to_string());
        }
    }
    out
}

const fn level<R>(node: &Node<R>) -> u8 {
    match node {
        Node::Binary { op, .. } => binary_level(*op),
        Node::Unary {
            op: UnaryOp::Not, ..
        } => 3,
        Node::Unary {
            op: UnaryOp::Negate,
            ..
        } => 7,
        _ => 9,
    }
}

const fn binary_level(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Or => 1,
        BinaryOp::And => 2,
        BinaryOp::Equal
        | BinaryOp::NotEqual
        | BinaryOp::Less
        | BinaryOp::LessEqual
        | BinaryOp::Greater
        | BinaryOp::GreaterEqual => 4,
        BinaryOp::Add | BinaryOp::Subtract => 5,
        BinaryOp::Multiply | BinaryOp::Divide => 6,
        BinaryOp::Power => 8,
    }
}

const fn binary_mark(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Or => "or",
        BinaryOp::And => "and",
        BinaryOp::Equal => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::Less => "<",
        BinaryOp::LessEqual => "<=",
        BinaryOp::Greater => ">",
        BinaryOp::GreaterEqual => ">=",
        BinaryOp::Add => "+",
        BinaryOp::Subtract => "-",
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::Power => "^",
    }
}

struct Printer<R> {
    refs: Vec<R>,
    measures: Vec<Measure>,
}

impl<R: PartialEq + Clone> Printer<R> {
    fn show(&mut self, node: &Node<R>, need: u8) -> Result<String, ExprError> {
        let text = self.raw(node)?;
        Ok(if level(node) < need {
            format!("({text})")
        } else {
            text
        })
    }

    fn raw(&mut self, node: &Node<R>) -> Result<String, ExprError> {
        Ok(match node {
            Node::Number { value, unit } => {
                let number = format_number(value.get())?;
                if unit.0.is_empty() {
                    number
                } else {
                    format!("{number}[{}]", format_unit(unit))
                }
            }
            Node::Constant(Constant::Pi) => "pi".into(),
            Node::Constant(Constant::E) => "e".into(),
            Node::Boolean(value) => if *value { "true" } else { "false" }.into(),
            Node::Text(text) => format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"")),
            Node::Reference(reference) => {
                let index = if let Some(index) = self.refs.iter().position(|r| r == reference) {
                    index
                } else {
                    self.refs.push(reference.clone());
                    self.refs.len() - 1
                };
                format!("${index}")
            }
            Node::Measure(measure) => {
                let index =
                    if let Some(index) = self.measures.iter().position(|m| m == measure.as_ref()) {
                        index
                    } else {
                        self.measures.push(measure.as_ref().clone());
                        self.measures.len() - 1
                    };
                format!("@{index}")
            }
            Node::LawParameter => "t".into(),
            Node::Bend(BendVariable::Angle) => "bendangle".into(),
            Node::Bend(BendVariable::InsideRadius) => "bendradius".into(),
            Node::Bend(BendVariable::Thickness) => "bendthickness".into(),
            Node::Unary {
                op: UnaryOp::Negate,
                operand,
            } => format!("-{}", self.show(operand, 7)?),
            Node::Unary {
                op: UnaryOp::Not,
                operand,
            } => format!("not {}", self.show(operand, 3)?),
            Node::Binary { op, left, right } => {
                let op = *op;
                let at = binary_level(op);
                let mark = binary_mark(op);
                if op == BinaryOp::Power {
                    format!("{}^{}", self.show(left, 9)?, self.show(right, 7)?)
                } else if at == 4 {
                    format!("{} {mark} {}", self.show(left, 5)?, self.show(right, 5)?)
                } else {
                    format!(
                        "{} {mark} {}",
                        self.show(left, at)?,
                        self.show(right, at + 1)?
                    )
                }
            }
            Node::Call { function, args } => {
                let parts = args
                    .iter()
                    .map(|arg| self.show(arg, 0))
                    .collect::<Result<Vec<_>, _>>()?;
                format!("{}({})", function.name(), parts.join(", "))
            }
            Node::If(branches) => {
                let [condition, then, otherwise] = branches.as_ref();
                format!(
                    "if({}, {}, {})",
                    self.show(condition, 0)?,
                    self.show(then, 0)?,
                    self.show(otherwise, 0)?
                )
            }
        })
    }
}

/// Prints a tree as canonical text, its references and measures in order of
/// first use.
///
/// # Errors
///
/// Returns [`ExprError::Malformed`] for a number with no canonical text.
pub fn print<R: PartialEq + Clone>(node: &Node<R>) -> Result<NeutralWire<R>, ExprError> {
    let mut printer = Printer {
        refs: Vec::new(),
        measures: Vec::new(),
    };
    let text = printer.raw(node)?;
    Ok(NeutralWire {
        text,
        refs: printer.refs,
        measures: printer.measures,
    })
}
