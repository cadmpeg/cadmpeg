// SPDX-License-Identifier: Apache-2.0
//! Printing a tree as canonical expression text.

use super::super::units::Unit;
use super::{
    AddOp, BendVariable, BinaryOp, Constant, ExprError, Measure, MulOp, NeutralWire, Node, UnaryOp,
};

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

const fn precedence<R>(node: &Node<R>) -> u8 {
    match node {
        Node::Any(_) => 1,
        Node::All(_) => 2,
        Node::Sum { .. } => 5,
        Node::Product { .. } => 6,
        Node::Binary { op, .. } => binary_precedence(*op),
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

const fn binary_precedence(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Equal
        | BinaryOp::NotEqual
        | BinaryOp::Less
        | BinaryOp::LessEqual
        | BinaryOp::Greater
        | BinaryOp::GreaterEqual => 4,
        BinaryOp::Power => 8,
    }
}

const fn add_mark(op: AddOp) -> &'static str {
    match op {
        AddOp::Add => "+",
        AddOp::Subtract => "-",
    }
}

const fn mul_mark(op: MulOp) -> &'static str {
    match op {
        MulOp::Multiply => "*",
        MulOp::Divide => "/",
    }
}

const fn binary_mark(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Equal => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::Less => "<",
        BinaryOp::LessEqual => "<=",
        BinaryOp::Greater => ">",
        BinaryOp::GreaterEqual => ">=",
        BinaryOp::Power => "^",
    }
}

/// What is left to print, taken from the end of the printer's work stack.
enum Step<'a, N> {
    /// Prints a node where its context binds at the precedence, in
    /// parentheses if it binds looser.
    Show(&'a N, u8),
    /// Prints a node.
    Raw(&'a N),
    /// Writes fixed text.
    Text(&'static str),
    /// Writes an operator between spaces.
    Mark(&'static str),
    /// Prints the remaining operations of a left-associative chain whose
    /// operators bind at `at`, the next one last.
    Chain {
        at: u8,
        operations: Vec<(&'static str, &'a N)>,
    },
    /// Writes `if(` and the cases of a ladder from its first, then its
    /// `otherwise`, each later case as an `if` nested in the last argument
    /// of the one before, as the text spells it.
    Ladder(&'a [(N, N)], &'a N),
}

struct Printer<R> {
    text: String,
    refs: Vec<R>,
    measures: Vec<Measure>,
}

/// The printer walks the tree on a work stack, so the depth of the tree does
/// not reach the thread's stack.
impl<R: PartialEq + Clone> Printer<R> {
    fn run(&mut self, root: &Node<R>) -> Result<(), ExprError> {
        let mut work = vec![Step::Raw(root)];
        while let Some(step) = work.pop() {
            self.step(step, &mut work)?;
        }
        Ok(())
    }

    fn step<'a>(
        &mut self,
        step: Step<'a, Node<R>>,
        work: &mut Vec<Step<'a, Node<R>>>,
    ) -> Result<(), ExprError> {
        match step {
            Step::Show(node, need) => {
                if precedence(node) < need {
                    self.text.push('(');
                    work.push(Step::Text(")"));
                }
                work.push(Step::Raw(node));
            }
            Step::Raw(node) => self.raw(node, work)?,
            Step::Text(text) => self.text.push_str(text),
            Step::Mark(mark) => self.mark(mark),
            Step::Chain { at, mut operations } => {
                if let Some((mark, operand)) = operations.pop() {
                    self.mark(mark);
                    work.push(Step::Chain { at, operations });
                    work.push(Step::Show(operand, at + 1));
                }
            }
            Step::Ladder(cases, otherwise) => {
                let Some(((condition, value), more)) = cases.split_first() else {
                    return malformed(
                        "an if states no case; canonical text writes an if with at least one",
                    );
                };
                self.text.push_str("if(");
                work.push(Step::Text(")"));
                if more.is_empty() {
                    work.push(Step::Show(otherwise, 0));
                } else {
                    work.push(Step::Ladder(more, otherwise));
                }
                work.push(Step::Text(", "));
                work.push(Step::Show(value, 0));
                work.push(Step::Text(", "));
                work.push(Step::Show(condition, 0));
            }
        }
        Ok(())
    }

    fn mark(&mut self, mark: &str) {
        self.text.push(' ');
        self.text.push_str(mark);
        self.text.push(' ');
    }

    /// Writes a leaf, or the text a node writes before its first operand and
    /// the steps that print the rest.
    fn raw<'a>(
        &mut self,
        node: &'a Node<R>,
        work: &mut Vec<Step<'a, Node<R>>>,
    ) -> Result<(), ExprError> {
        match node {
            Node::Number { value, unit } => {
                self.text.push_str(&format_number(value.get())?);
                if !unit.0.is_empty() {
                    self.text.push('[');
                    self.text.push_str(&format_unit(unit));
                    self.text.push(']');
                }
            }
            Node::Constant(Constant::Pi) => self.text.push_str("pi"),
            Node::Constant(Constant::E) => self.text.push('e'),
            Node::Boolean(value) => self.text.push_str(if *value { "true" } else { "false" }),
            Node::Text(text) => {
                self.text.push('"');
                self.text
                    .push_str(&text.replace('\\', "\\\\").replace('"', "\\\""));
                self.text.push('"');
            }
            Node::Reference(reference) => {
                let index = if let Some(index) = self.refs.iter().position(|r| r == reference) {
                    index
                } else {
                    self.refs.push(reference.clone());
                    self.refs.len() - 1
                };
                self.text.push('$');
                self.text.push_str(&index.to_string());
            }
            Node::Measure(measure) => {
                let index =
                    if let Some(index) = self.measures.iter().position(|m| m == measure.as_ref()) {
                        index
                    } else {
                        self.measures.push(measure.as_ref().clone());
                        self.measures.len() - 1
                    };
                self.text.push('@');
                self.text.push_str(&index.to_string());
            }
            Node::LawParameter => self.text.push('t'),
            Node::Bend(BendVariable::Angle) => self.text.push_str("bendangle"),
            Node::Bend(BendVariable::InsideRadius) => self.text.push_str("bendradius"),
            Node::Bend(BendVariable::Thickness) => self.text.push_str("bendthickness"),
            Node::Unary {
                op: UnaryOp::Negate,
                operand,
            } => {
                self.text.push('-');
                work.push(Step::Show(operand, 7));
            }
            Node::Unary {
                op: UnaryOp::Not,
                operand,
            } => {
                self.text.push_str("not ");
                work.push(Step::Show(operand, 3));
            }
            Node::Binary { op, left, right } => match *op {
                BinaryOp::Power => {
                    work.push(Step::Show(right, 7));
                    work.push(Step::Text("^"));
                    work.push(Step::Show(left, 9));
                }
                op @ (BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::Less
                | BinaryOp::LessEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual) => {
                    work.push(Step::Show(right, 5));
                    work.push(Step::Mark(binary_mark(op)));
                    work.push(Step::Show(left, 5));
                }
            },
            Node::Sum { first, rest } => chain(
                5,
                first.as_ref(),
                rest.iter()
                    .rev()
                    .map(|(op, operand)| (add_mark(*op), operand))
                    .collect(),
                work,
            ),
            Node::Product { first, rest } => chain(
                6,
                first.as_ref(),
                rest.iter()
                    .rev()
                    .map(|(op, operand)| (mul_mark(*op), operand))
                    .collect(),
                work,
            ),
            Node::All(operands) | Node::Any(operands) => {
                let (at, mark) = if matches!(node, Node::All(_)) {
                    (2, "and")
                } else {
                    (1, "or")
                };
                let Some((first, rest)) = operands.split_first() else {
                    return malformed(format!(
                        "an {mark} run states no operand; canonical text writes one with at \
                         least two"
                    ));
                };
                chain(
                    at,
                    first,
                    rest.iter().rev().map(|operand| (mark, operand)).collect(),
                    work,
                );
            }
            Node::Call { function, args } => {
                if args.is_empty() {
                    return malformed(format!(
                        "a call of {} states no argument; canonical text calls a function with \
                         at least one",
                        function.name()
                    ));
                }
                self.text.push_str(function.name());
                self.text.push('(');
                arguments(args, work);
            }
            Node::If { cases, otherwise } => work.push(Step::Ladder(cases, otherwise)),
        }
        Ok(())
    }
}

/// Pushes the steps that print a run of one left-associative level: its
/// first operand, then `operations`, given last first, each operator before
/// its operand.
fn chain<'a, N>(
    at: u8,
    first: &'a N,
    operations: Vec<(&'static str, &'a N)>,
    work: &mut Vec<Step<'a, N>>,
) {
    work.push(Step::Chain { at, operations });
    work.push(Step::Show(first, at));
}

/// Pushes the steps that print `args`, separated by commas, and the closing
/// parenthesis.
fn arguments<'a, R>(args: &'a [Node<R>], work: &mut Vec<Step<'a, Node<R>>>) {
    work.push(Step::Text(")"));
    for (i, arg) in args.iter().enumerate().rev() {
        work.push(Step::Show(arg, 0));
        if i > 0 {
            work.push(Step::Text(", "));
        }
    }
}

/// Prints a tree as canonical text, its references and measures in order of
/// first use.
///
/// # Errors
///
/// Returns [`ExprError::Malformed`] for a number with no canonical text or a
/// call with no argument. A tree of any depth prints; reading its text back
/// refuses a tree deeper than an expression's bound (§2.5).
pub fn print<R: PartialEq + Clone>(node: &Node<R>) -> Result<NeutralWire<R>, ExprError> {
    let mut printer = Printer {
        text: String::new(),
        refs: Vec::new(),
        measures: Vec::new(),
    };
    printer.run(node)?;
    Ok(NeutralWire {
        text: printer.text,
        refs: printer.refs,
        measures: printer.measures,
    })
}
