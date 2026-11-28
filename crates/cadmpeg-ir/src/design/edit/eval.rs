// SPDX-License-Identifier: Apache-2.0
//! Evaluation and type checking of neutral expression trees on canonical
//! values (§5.13).
//!
//! Every node has a quantity or is Boolean or Text, and every operator checks
//! it. A literal converts from its authored unit to the canonical unit of its
//! quantity; a reference takes its parameter's cache; a measure takes its
//! cached value. Evaluation visits only the taken branch of an `if`; the type
//! check visits every node. Each walk keeps its own stack, so a tree of any
//! depth evaluates on a small thread.

use cadmpeg_core::convert::{f64_from_i64, truncate_f64_to_i32};

use crate::design::dimension::ParameterRef;
use crate::design::expr::{
    AddOp, BendVariable, BinaryOp, Constant, Function, MeasureKind, MulOp, Node, UnaryOp,
};
use crate::design::parameter::ParameterValue;
use crate::design::units::{Quantity, Unit, ValueKind};
use crate::math::portable::Portable;
use crate::scalar::FiniteReal;

/// The value of an expression node.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A number in the canonical unit of its quantity.
    Number {
        /// The canonical value.
        value: f64,
        /// The quantity.
        quantity: Quantity,
    },
    /// A Boolean.
    Boolean(bool),
    /// A text.
    Text(String),
}

/// Why an expression has no value.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EvalError {
    /// A reference, a measure, the law parameter, or a bend variable has no
    /// value here.
    #[error("{0} has no value")]
    Unbound(&'static str),
    /// An operand does not have the quantity or kind its operator takes.
    #[error("{0}")]
    Type(String),
    /// The operation is outside its domain: division by zero, `sqrt` of a
    /// negative, a logarithm of a non-positive, `asin` or `acos` outside
    /// [-1, 1], `mod` by zero, or a result that is not finite.
    #[error("{0}")]
    Domain(&'static str),
}

const DIMENSIONLESS: Quantity = Quantity::REAL;
const MASS: Quantity = Quantity {
    length: 0,
    mass: 1,
    angle: 0,
    time: 0,
};
const VOLUME: Quantity = Quantity {
    length: 3,
    mass: 0,
    angle: 0,
    time: 0,
};

fn scaled(quantity: Quantity, by: i8) -> Option<Quantity> {
    Some(Quantity {
        length: quantity.length.checked_mul(by)?,
        mass: quantity.mass.checked_mul(by)?,
        angle: quantity.angle.checked_mul(by)?,
        time: quantity.time.checked_mul(by)?,
    })
}

fn combined(left: Quantity, right: Quantity, sign: i8) -> Result<Quantity, EvalError> {
    let right = scaled(right, sign).ok_or(EvalError::Domain("a quantity exponent overflows"))?;
    let sum = |a: i8, b: i8| {
        a.checked_add(b)
            .ok_or(EvalError::Domain("a quantity exponent overflows"))
    };
    Ok(Quantity {
        length: sum(left.length, right.length)?,
        mass: sum(left.mass, right.mass)?,
        angle: sum(left.angle, right.angle)?,
        time: sum(left.time, right.time)?,
    })
}

/// The factor from a unit to the canonical unit of its quantity, and that
/// quantity.
///
/// # Errors
///
/// Returns [`EvalError::Domain`] where an exponent overflows.
pub(crate) fn unit_factor(unit: &Unit) -> Result<(f64, Quantity), EvalError> {
    let mut factor = 1.0;
    let mut quantity = DIMENSIONLESS;
    for item in &unit.0 {
        let (scale, of) = item.symbol.canonical();
        let power = item.power.get();
        factor *= scale.powi(i32::from(power));
        let raised = scaled(of, power).ok_or(EvalError::Domain("a quantity exponent overflows"))?;
        quantity = combined(quantity, raised, 1)?;
    }
    Ok((factor, quantity))
}

const fn measure_quantity(kind: &MeasureKind) -> Quantity {
    match kind {
        MeasureKind::Length
        | MeasureKind::Distance
        | MeasureKind::Radius
        | MeasureKind::Coordinate(_) => Quantity::LENGTH,
        MeasureKind::Area => Quantity::AREA,
        MeasureKind::Volume => VOLUME,
        MeasureKind::Mass => MASS,
        MeasureKind::Angle => Quantity::ANGLE,
    }
}

fn type_error(text: impl Into<String>) -> EvalError {
    EvalError::Type(text.into())
}

fn number(value: &Value, what: &str) -> Result<(f64, Quantity), EvalError> {
    match value {
        Value::Number { value, quantity } => Ok((*value, *quantity)),
        _ => Err(type_error(format!("{what} takes a number"))),
    }
}

fn boolean(value: &Value, what: &str) -> Result<bool, EvalError> {
    match value {
        Value::Boolean(value) => Ok(*value),
        _ => Err(type_error(format!("{what} takes a Boolean"))),
    }
}

fn finite(value: f64, quantity: Quantity) -> Result<Value, EvalError> {
    if value.is_finite() {
        Ok(Value::Number {
            value: value + 0.0,
            quantity,
        })
    } else {
        Err(EvalError::Domain("the result is not finite"))
    }
}

fn same(left: Quantity, right: Quantity, what: &str) -> Result<Quantity, EvalError> {
    if left == right {
        Ok(left)
    } else {
        Err(type_error(format!("{what} takes operands of one quantity")))
    }
}

/// The value of a literal exponent: a number, or a negated number.
fn literal_exponent<R>(node: &Node<R>) -> Option<f64> {
    match node {
        Node::Number { value, unit } if unit.0.is_empty() => Some(value.get()),
        Node::Unary {
            op: UnaryOp::Negate,
            operand,
        } => match operand.as_ref() {
            Node::Number { value, unit } if unit.0.is_empty() => Some(-value.get()),
            _ => None,
        },
        _ => None,
    }
}

fn power(base: f64, exponent: f64) -> Result<f64, EvalError> {
    if exponent.fract() == 0.0 {
        if let Some(whole) = truncate_f64_to_i32(exponent) {
            if base == 0.0 && whole < 0 {
                return Err(EvalError::Domain("division by zero"));
            }
            return Ok(base.powi(whole));
        }
    }
    if base > 0.0 {
        Ok((exponent * base.portable_ln()).portable_exp())
    } else if base == 0.0 && exponent > 0.0 {
        Ok(0.0)
    } else if base == 0.0 {
        Err(EvalError::Domain("division by zero"))
    } else {
        Err(EvalError::Domain(
            "a negative base takes only a whole exponent",
        ))
    }
}

/// The quantity of a dimensioned base raised to a literal exponent `n`: every
/// product of `n` and a base exponent is an integer.
fn raised_quantity(quantity: Quantity, exponent: f64) -> Result<Quantity, EvalError> {
    let scale = |base: i8| -> Result<i8, EvalError> {
        let product = f64::from(base) * exponent;
        if product.fract() != 0.0 {
            return Err(type_error(
                "a dimensioned base takes an exponent that keeps its exponents whole",
            ));
        }
        truncate_f64_to_i32(product)
            .and_then(|whole| i8::try_from(whole).ok())
            .ok_or(EvalError::Domain("a quantity exponent overflows"))
    };
    Ok(Quantity {
        length: scale(quantity.length)?,
        mass: scale(quantity.mass)?,
        angle: scale(quantity.angle)?,
        time: scale(quantity.time)?,
    })
}

fn halved(quantity: Quantity) -> Result<Quantity, EvalError> {
    let half = |exponent: i8| {
        if exponent % 2 == 0 {
            Ok(exponent / 2)
        } else {
            Err(type_error("sqrt takes even exponents"))
        }
    };
    Ok(Quantity {
        length: half(quantity.length)?,
        mass: half(quantity.mass)?,
        angle: half(quantity.angle)?,
        time: half(quantity.time)?,
    })
}

fn unary(op: UnaryOp, operand: &Value) -> Result<Value, EvalError> {
    match op {
        UnaryOp::Negate => {
            let (value, quantity) = number(operand, "-")?;
            finite(-value, quantity)
        }
        UnaryOp::Not => Ok(Value::Boolean(!boolean(operand, "not")?)),
    }
}

fn compare(op: BinaryOp, left: Value, right: Value) -> Result<Value, EvalError> {
    let result = match (left, right) {
        (Value::Text(left), Value::Text(right)) => match op {
            BinaryOp::Equal => left == right,
            BinaryOp::NotEqual => left != right,
            _ => return Err(type_error("text compares only by == and !=")),
        },
        (left, right) => {
            let (left, a) = number(&left, "a comparison")?;
            let (right, b) = number(&right, "a comparison")?;
            same(a, b, "a comparison")?;
            match op {
                BinaryOp::Equal => left == right,
                BinaryOp::NotEqual => left != right,
                BinaryOp::Less => left < right,
                BinaryOp::LessEqual => left <= right,
                BinaryOp::Greater => left > right,
                _ => left >= right,
            }
        }
    };
    Ok(Value::Boolean(result))
}

/// One step of a left-to-right evaluation: a `Binary` node's operator, or
/// the operator between two operands of a chain.
#[derive(Clone, Copy)]
enum Pair {
    Binary(BinaryOp),
    Add(AddOp),
    Mul(MulOp),
    And,
    Or,
}

/// The first operand of a two-operand node or a chain; `None` for any other
/// node, and for a run of `and` or `or` with no operand.
fn run_first<R>(node: &Node<R>) -> Option<&Node<R>> {
    match node {
        Node::Binary { left, .. } => Some(left.as_ref()),
        Node::Sum { first, .. } | Node::Product { first, .. } => Some(first.as_ref()),
        Node::All(operands) | Node::Any(operands) => operands.first(),
        _ => None,
    }
}

/// Each later operand of a two-operand node or a chain with the step that
/// joins it, in order.
fn run_steps<R>(node: &Node<R>) -> Vec<(Pair, &Node<R>)> {
    match node {
        Node::Binary { op, right, .. } => vec![(Pair::Binary(*op), right.as_ref())],
        Node::Sum { rest, .. } => rest
            .iter()
            .map(|(op, operand)| (Pair::Add(*op), operand))
            .collect(),
        Node::Product { rest, .. } => rest
            .iter()
            .map(|(op, operand)| (Pair::Mul(*op), operand))
            .collect(),
        Node::All(operands) => operands
            .iter()
            .skip(1)
            .map(|operand| (Pair::And, operand))
            .collect(),
        Node::Any(operands) => operands
            .iter()
            .skip(1)
            .map(|operand| (Pair::Or, operand))
            .collect(),
        _ => Vec::new(),
    }
}

fn binary<R>(
    op: Pair,
    left: Value,
    right: Value,
    right_node: &Node<R>,
) -> Result<Value, EvalError> {
    match op {
        Pair::Add(op) => {
            let (left, a) = number(&left, "+ and -")?;
            let (right, b) = number(&right, "+ and -")?;
            let quantity = same(a, b, "+ and -")?;
            let value = match op {
                AddOp::Add => left + right,
                AddOp::Subtract => left - right,
            };
            finite(value, quantity)
        }
        Pair::Mul(MulOp::Multiply) => {
            let (left, a) = number(&left, "*")?;
            let (right, b) = number(&right, "*")?;
            finite(left * right, combined(a, b, 1)?)
        }
        Pair::Mul(MulOp::Divide) => {
            let (left, a) = number(&left, "/")?;
            let (right, b) = number(&right, "/")?;
            if right == 0.0 {
                return Err(EvalError::Domain("division by zero"));
            }
            finite(left / right, combined(a, b, -1)?)
        }
        Pair::Binary(BinaryOp::Power) => {
            let (base, a) = number(&left, "^")?;
            let (exponent, b) = number(&right, "^")?;
            same(b, DIMENSIONLESS, "an exponent")?;
            if a == DIMENSIONLESS {
                return finite(power(base, exponent)?, DIMENSIONLESS);
            }
            let literal = literal_exponent(right_node)
                .ok_or_else(|| type_error("a dimensioned base takes a literal exponent"))?;
            finite(power(base, literal)?, raised_quantity(a, literal)?)
        }
        Pair::Binary(
            op @ (BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::Less
            | BinaryOp::LessEqual
            | BinaryOp::Greater
            | BinaryOp::GreaterEqual),
        ) => compare(op, left, right),
        Pair::And => Ok(Value::Boolean(
            boolean(&left, "and")? & boolean(&right, "and")?,
        )),
        Pair::Or => Ok(Value::Boolean(
            boolean(&left, "or")? | boolean(&right, "or")?,
        )),
    }
}

fn one(function: Function, mut args: Vec<Value>) -> Result<(f64, Quantity), EvalError> {
    if args.len() != 1 {
        return Err(type_error(format!(
            "{} takes one argument",
            function.name()
        )));
    }
    number(&args.remove(0), function.name())
}

fn two(function: Function, mut args: Vec<Value>) -> Result<(f64, Quantity, f64), EvalError> {
    if args.len() != 2 {
        return Err(type_error(format!(
            "{} takes two arguments",
            function.name()
        )));
    }
    let (right, b) = number(&args.remove(1), function.name())?;
    let (left, a) = number(&args.remove(0), function.name())?;
    Ok((left, same(a, b, function.name())?, right))
}

fn dimensionless(function: Function, args: Vec<Value>) -> Result<f64, EvalError> {
    let (value, quantity) = one(function, args)?;
    same(quantity, DIMENSIONLESS, function.name())?;
    Ok(value)
}

fn angle(function: Function, args: Vec<Value>) -> Result<f64, EvalError> {
    let (value, quantity) = one(function, args)?;
    same(quantity, Quantity::ANGLE, function.name())?;
    Ok(value)
}

fn call(function: Function, args: Vec<Value>) -> Result<Value, EvalError> {
    match function {
        Function::Sin => finite(angle(function, args)?.portable_sin(), DIMENSIONLESS),
        Function::Cos => finite(angle(function, args)?.portable_cos(), DIMENSIONLESS),
        Function::Tan => finite(angle(function, args)?.portable_tan(), DIMENSIONLESS),
        Function::Asin | Function::Acos => {
            let value = dimensionless(function, args)?;
            if !(-1.0..=1.0).contains(&value) {
                return Err(EvalError::Domain("asin and acos take values in [-1, 1]"));
            }
            let result = if function == Function::Asin {
                value.portable_asin()
            } else {
                value.portable_acos()
            };
            finite(result, Quantity::ANGLE)
        }
        Function::Atan => finite(
            dimensionless(function, args)?.portable_atan(),
            Quantity::ANGLE,
        ),
        Function::Atan2 => {
            let (y, _, x) = two(function, args)?;
            finite(y.portable_atan2(x), Quantity::ANGLE)
        }
        Function::Sqrt => {
            let (value, quantity) = one(function, args)?;
            if value < 0.0 {
                return Err(EvalError::Domain("sqrt of a negative"));
            }
            finite(value.sqrt(), halved(quantity)?)
        }
        Function::Abs => {
            let (value, quantity) = one(function, args)?;
            finite(value.abs(), quantity)
        }
        Function::Exp => finite(dimensionless(function, args)?.portable_exp(), DIMENSIONLESS),
        Function::Ln | Function::Log10 => {
            let value = dimensionless(function, args)?;
            if value <= 0.0 {
                return Err(EvalError::Domain("a logarithm of a non-positive"));
            }
            let natural = value.portable_ln();
            let result = if function == Function::Ln {
                natural
            } else {
                natural / std::f64::consts::LN_10
            };
            finite(result, DIMENSIONLESS)
        }
        Function::Min | Function::Max => {
            let mut best: Option<(f64, Quantity)> = None;
            for arg in args {
                let (value, quantity) = number(&arg, function.name())?;
                best = Some(match best {
                    None => (value, quantity),
                    Some((held, of)) => {
                        same(of, quantity, function.name())?;
                        let keep = if function == Function::Min {
                            held <= value
                        } else {
                            held >= value
                        };
                        (if keep { held } else { value }, of)
                    }
                });
            }
            let (value, quantity) =
                best.ok_or_else(|| type_error(format!("{} takes an argument", function.name())))?;
            finite(value, quantity)
        }
        Function::Floor => finite(dimensionless(function, args)?.floor(), DIMENSIONLESS),
        Function::Ceil => finite(dimensionless(function, args)?.ceil(), DIMENSIONLESS),
        Function::Round => finite(dimensionless(function, args)?.round(), DIMENSIONLESS),
        Function::Trunc => finite(dimensionless(function, args)?.trunc(), DIMENSIONLESS),
        Function::Mod => {
            let (a, quantity, b) = two(function, args)?;
            if b == 0.0 {
                return Err(EvalError::Domain("mod by zero"));
            }
            finite(a - (a / b).trunc() * b, quantity)
        }
        Function::Sign => {
            let (value, _) = one(function, args)?;
            let sign = if value > 0.0 {
                1.0
            } else if value < 0.0 {
                -1.0
            } else {
                0.0
            };
            finite(sign, DIMENSIONLESS)
        }
    }
}

enum Task<'a> {
    Visit(&'a Node<ParameterRef>),
    Unary(UnaryOp),
    Binary(Pair, &'a Node<ParameterRef>),
    Call(Function, usize),
    /// Tests the ladder's first condition, or takes `otherwise` where no
    /// case is left.
    Ladder(
        &'a [(Node<ParameterRef>, Node<ParameterRef>)],
        &'a Node<ParameterRef>,
    ),
    /// Takes the value of the condition just tested, or tests the rest.
    Branch(
        &'a Node<ParameterRef>,
        &'a [(Node<ParameterRef>, Node<ParameterRef>)],
        &'a Node<ParameterRef>,
    ),
}

/// Evaluates a neutral design expression tree on canonical values.
/// `reference` gives a reference's value, or `None` where it has none.
///
/// # Errors
///
/// Returns [`EvalError::Unbound`] where a reference, a measure, the law
/// parameter, or a bend variable has no value, [`EvalError::Type`] where an
/// operand does not have the quantity or kind its operator takes, and
/// [`EvalError::Domain`] for an operation outside its domain.
pub fn evaluate(
    root: &Node<ParameterRef>,
    reference: &mut dyn FnMut(&ParameterRef) -> Option<Value>,
) -> Result<Value, EvalError> {
    let mut tasks = vec![Task::Visit(root)];
    let mut values: Vec<Value> = Vec::new();
    let pop = |values: &mut Vec<Value>| {
        values
            .pop()
            .ok_or_else(|| type_error("an operator has no operand"))
    };
    while let Some(task) = tasks.pop() {
        match task {
            Task::Visit(node) => match node {
                Node::Number { value, unit } => {
                    let (factor, quantity) = unit_factor(unit)?;
                    values.push(finite(value.get() * factor, quantity)?);
                }
                Node::Constant(Constant::Pi) => values.push(Value::Number {
                    value: std::f64::consts::PI,
                    quantity: DIMENSIONLESS,
                }),
                Node::Constant(Constant::E) => values.push(Value::Number {
                    value: std::f64::consts::E,
                    quantity: DIMENSIONLESS,
                }),
                Node::Boolean(value) => values.push(Value::Boolean(*value)),
                Node::Text(text) => values.push(Value::Text(text.clone())),
                Node::Reference(target) => {
                    values.push(reference(target).ok_or(EvalError::Unbound("a reference"))?);
                }
                Node::Measure(measure) => {
                    let cached = measure.cached.ok_or(EvalError::Unbound("a measure"))?;
                    values.push(finite(cached.get(), measure_quantity(&measure.kind))?);
                }
                Node::LawParameter => return Err(EvalError::Unbound("the law parameter")),
                Node::Bend(_) => return Err(EvalError::Unbound("a bend variable")),
                Node::Unary { op, operand } => {
                    tasks.push(Task::Unary(*op));
                    tasks.push(Task::Visit(operand));
                }
                Node::Binary { .. }
                | Node::Sum { .. }
                | Node::Product { .. }
                | Node::All(_)
                | Node::Any(_) => {
                    let first =
                        run_first(node).ok_or_else(|| type_error("a run states no operand"))?;
                    for (op, operand) in run_steps(node).into_iter().rev() {
                        tasks.push(Task::Binary(op, operand));
                        tasks.push(Task::Visit(operand));
                    }
                    tasks.push(Task::Visit(first));
                }
                Node::Call { function, args } => {
                    tasks.push(Task::Call(*function, args.len()));
                    tasks.extend(args.iter().rev().map(Task::Visit));
                }
                Node::If { cases, otherwise } => tasks.push(Task::Ladder(cases, otherwise)),
            },
            Task::Unary(op) => {
                let operand = pop(&mut values)?;
                values.push(unary(op, &operand)?);
            }
            Task::Binary(op, right_node) => {
                let right = pop(&mut values)?;
                let left = pop(&mut values)?;
                values.push(binary(op, left, right, right_node)?);
            }
            Task::Call(function, count) => {
                let start = values
                    .len()
                    .checked_sub(count)
                    .ok_or_else(|| type_error("a call has too few arguments"))?;
                let args = values.split_off(start);
                values.push(call(function, args)?);
            }
            Task::Ladder(cases, otherwise) => match cases.split_first() {
                Some(((condition, value), more)) => {
                    tasks.push(Task::Branch(value, more, otherwise));
                    tasks.push(Task::Visit(condition));
                }
                None => tasks.push(Task::Visit(otherwise)),
            },
            Task::Branch(value, more, otherwise) => {
                if boolean(&pop(&mut values)?, "if")? {
                    tasks.push(Task::Visit(value));
                } else {
                    tasks.push(Task::Ladder(more, otherwise));
                }
            }
        }
    }
    pop(&mut values)
}

/// The type of an expression node (§5.13).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Type {
    /// A number of a quantity.
    Number(Quantity),
    /// A Boolean.
    Boolean,
    /// A text.
    Text,
    /// A reference to a parameter of a document that was not read.
    Unknown,
}

impl Type {
    /// The type of a parameter of a kind, `None` for a `Native` kind, which
    /// has no §5.13 type.
    pub(crate) const fn of_kind(kind: &ValueKind) -> Option<Self> {
        match kind {
            ValueKind::Quantity(quantity) => Some(Self::Number(*quantity)),
            ValueKind::Integer => Some(Self::Number(DIMENSIONLESS)),
            ValueKind::Boolean => Some(Self::Boolean),
            ValueKind::Text => Some(Self::Text),
            ValueKind::Native(_) => None,
        }
    }
}

/// The quantity of a number operand, `None` where it is unknown.
fn quantity_of(operand: Type, what: &str) -> Result<Option<Quantity>, String> {
    match operand {
        Type::Number(quantity) => Ok(Some(quantity)),
        Type::Unknown => Ok(None),
        Type::Boolean | Type::Text => Err(format!("{what} takes a number")),
    }
}

fn boolean_type(operand: Type, what: &str) -> Result<(), String> {
    match operand {
        Type::Boolean | Type::Unknown => Ok(()),
        Type::Number(_) | Type::Text => Err(format!("{what} takes a Boolean")),
    }
}

/// The one quantity of operands that take the same quantity.
fn same_type(
    left: Option<Quantity>,
    right: Option<Quantity>,
    what: &str,
) -> Result<Option<Quantity>, String> {
    match (left, right) {
        (Some(left), Some(right)) => same(left, right, what)
            .map(Some)
            .map_err(|error| error.to_string()),
        (known, None) | (None, known) => Ok(known),
    }
}

fn number_type(quantity: Option<Quantity>) -> Type {
    quantity.map_or(Type::Unknown, Type::Number)
}

fn arity(function: Function, count: usize) -> Result<(), String> {
    let admitted = match function {
        Function::Atan2 | Function::Mod => count == 2,
        Function::Min | Function::Max => count >= 2,
        _ => count == 1,
    };
    if admitted {
        Ok(())
    } else {
        let takes = match function {
            Function::Atan2 | Function::Mod => "two arguments",
            Function::Min | Function::Max => "at least two arguments",
            _ => "one argument",
        };
        Err(format!("{} takes {takes}", function.name()))
    }
}

fn expect_quantity(operand: Type, expected: Quantity, what: &str) -> Result<(), String> {
    match quantity_of(operand, what)? {
        Some(quantity) if quantity != expected => Err(format!("{what} takes {expected:?}")),
        _ => Ok(()),
    }
}

fn binary_type<R>(op: Pair, left: Type, right: Type, right_node: &Node<R>) -> Result<Type, String> {
    let domain = |error: EvalError| error.to_string();
    Ok(match op {
        Pair::Add(_) => number_type(same_type(
            quantity_of(left, "+ and -")?,
            quantity_of(right, "+ and -")?,
            "+ and -",
        )?),
        Pair::Mul(op) => {
            let sign = if op == MulOp::Multiply { 1 } else { -1 };
            match (
                quantity_of(left, "* and /")?,
                quantity_of(right, "* and /")?,
            ) {
                (Some(a), Some(b)) => Type::Number(combined(a, b, sign).map_err(domain)?),
                _ => Type::Unknown,
            }
        }
        Pair::Binary(BinaryOp::Power) => {
            let base = quantity_of(left, "^")?;
            expect_quantity(right, DIMENSIONLESS, "an exponent")?;
            match base {
                Some(DIMENSIONLESS) => Type::Number(DIMENSIONLESS),
                Some(quantity) => {
                    let literal = literal_exponent(right_node)
                        .ok_or("a dimensioned base takes a literal exponent")?;
                    Type::Number(raised_quantity(quantity, literal).map_err(domain)?)
                }
                None => Type::Unknown,
            }
        }
        Pair::Binary(
            op @ (BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::Less
            | BinaryOp::LessEqual
            | BinaryOp::Greater
            | BinaryOp::GreaterEqual),
        ) => {
            let equality = matches!(op, BinaryOp::Equal | BinaryOp::NotEqual);
            match (left, right) {
                (Type::Text, Type::Text | Type::Unknown) | (Type::Unknown, Type::Text)
                    if equality => {}
                (Type::Text, _) | (_, Type::Text) => {
                    return Err("text compares only with text, by == and !=".into());
                }
                _ => {
                    same_type(
                        quantity_of(left, "a comparison")?,
                        quantity_of(right, "a comparison")?,
                        "a comparison",
                    )?;
                }
            }
            Type::Boolean
        }
        Pair::And | Pair::Or => {
            boolean_type(left, "and and or")?;
            boolean_type(right, "and and or")?;
            Type::Boolean
        }
    })
}

fn call_type(function: Function, args: &[Type]) -> Result<Type, String> {
    arity(function, args.len())?;
    let what = function.name();
    let first = args[0];
    Ok(match function {
        Function::Sin | Function::Cos | Function::Tan => {
            expect_quantity(first, Quantity::ANGLE, what)?;
            Type::Number(DIMENSIONLESS)
        }
        Function::Asin | Function::Acos | Function::Atan => {
            expect_quantity(first, DIMENSIONLESS, what)?;
            Type::Number(Quantity::ANGLE)
        }
        Function::Atan2 => {
            same_type(quantity_of(first, what)?, quantity_of(args[1], what)?, what)?;
            Type::Number(Quantity::ANGLE)
        }
        Function::Sqrt => match quantity_of(first, what)? {
            Some(quantity) => Type::Number(halved(quantity).map_err(|error| error.to_string())?),
            None => Type::Unknown,
        },
        Function::Abs => number_type(quantity_of(first, what)?),
        Function::Exp
        | Function::Ln
        | Function::Log10
        | Function::Floor
        | Function::Ceil
        | Function::Round
        | Function::Trunc => {
            expect_quantity(first, DIMENSIONLESS, what)?;
            Type::Number(DIMENSIONLESS)
        }
        Function::Min | Function::Max | Function::Mod => {
            let mut held = None;
            for arg in args {
                held = same_type(held, quantity_of(*arg, what)?, what)?;
            }
            number_type(held)
        }
        Function::Sign => {
            quantity_of(first, what)?;
            Type::Number(DIMENSIONLESS)
        }
    })
}

/// The one kind of the two branches of an `if`.
fn branch_type(then: Type, otherwise: Type) -> Result<Type, String> {
    match (then, otherwise) {
        (Type::Number(a), Type::Number(b)) => same(a, b, "if")
            .map(Type::Number)
            .map_err(|error| error.to_string()),
        (Type::Unknown, known) | (known, Type::Unknown) => Ok(known),
        (Type::Boolean, Type::Boolean) => Ok(Type::Boolean),
        (Type::Text, Type::Text) => Ok(Type::Text),
        _ => Err("if takes branches of one kind".into()),
    }
}

enum Check<'a, R = ParameterRef> {
    Visit(&'a Node<R>),
    Unary(UnaryOp),
    Binary(Pair, &'a Node<R>),
    Call(Function, usize),
    /// A ladder of this many cases, its conditions, values, and `otherwise`
    /// on the type stack.
    If(usize),
}

/// Where an expression occurs, which decides the variables in scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExpressionSite {
    /// A parameter's expression, an override's, or a program's: no law path
    /// and no bend is in scope.
    Parameter,
    /// An `AngleRange.value`: the bend variables are in scope.
    BendAllowance,
    /// A `Law::Expression`: the law parameter, dimensionless, is in scope.
    Law,
}

/// The quantity of a bend variable.
const fn bend_quantity(variable: BendVariable) -> Quantity {
    match variable {
        BendVariable::Angle => Quantity::ANGLE,
        BendVariable::InsideRadius | BendVariable::Thickness => Quantity::LENGTH,
    }
}

/// Type-checks a neutral expression tree by the semantics table of §5.13
/// and the function arities of part 10, every branch of an `if` included.
/// `reference` gives a reference's type, or why the reference has none. The
/// law parameter is refused outside a law, and the bend variables outside a
/// bend allowance.
///
/// # Errors
///
/// Returns why a node does not type-check.
pub(crate) fn check_types<R>(
    root: &Node<R>,
    site: ExpressionSite,
    reference: &dyn Fn(&R) -> Result<Type, String>,
) -> Result<Type, String> {
    let mut tasks = vec![Check::Visit(root)];
    let mut types: Vec<Type> = Vec::new();
    let pop = |types: &mut Vec<Type>| types.pop().ok_or("an operator has no operand");
    while let Some(task) = tasks.pop() {
        match task {
            Check::Visit(node) => match node {
                Node::Number { unit, .. } => types.push(Type::Number(
                    unit_factor(unit).map_err(|error| error.to_string())?.1,
                )),
                Node::Constant(_) => types.push(Type::Number(DIMENSIONLESS)),
                Node::Boolean(_) => types.push(Type::Boolean),
                Node::Text(_) => types.push(Type::Text),
                Node::Reference(target) => types.push(reference(target)?),
                Node::Measure(measure) => {
                    types.push(Type::Number(measure_quantity(&measure.kind)));
                }
                Node::LawParameter => match site {
                    ExpressionSite::Law => types.push(Type::Number(DIMENSIONLESS)),
                    ExpressionSite::Parameter | ExpressionSite::BendAllowance => {
                        return Err("the law parameter occurs only in a law expression".into());
                    }
                },
                Node::Bend(variable) => match site {
                    ExpressionSite::BendAllowance => {
                        types.push(Type::Number(bend_quantity(*variable)));
                    }
                    ExpressionSite::Parameter | ExpressionSite::Law => {
                        return Err("a bend variable occurs only in a bend allowance".into());
                    }
                },
                Node::Unary { op, operand } => {
                    tasks.push(Check::Unary(*op));
                    tasks.push(Check::Visit(operand));
                }
                Node::Binary { .. }
                | Node::Sum { .. }
                | Node::Product { .. }
                | Node::All(_)
                | Node::Any(_) => {
                    let first = run_first(node).ok_or("a run states no operand")?;
                    for (op, operand) in run_steps(node).into_iter().rev() {
                        tasks.push(Check::Binary(op, operand));
                        tasks.push(Check::Visit(operand));
                    }
                    tasks.push(Check::Visit(first));
                }
                Node::Call { function, args } => {
                    tasks.push(Check::Call(*function, args.len()));
                    tasks.extend(args.iter().rev().map(Check::Visit));
                }
                Node::If { cases, .. } => {
                    tasks.push(Check::If(cases.len()));
                    let operands: Vec<_> = node.operands().collect();
                    tasks.extend(operands.into_iter().rev().map(Check::Visit));
                }
            },
            Check::Unary(op) => {
                let operand = pop(&mut types)?;
                types.push(match op {
                    UnaryOp::Negate => number_type(quantity_of(operand, "-")?),
                    UnaryOp::Not => {
                        boolean_type(operand, "not")?;
                        Type::Boolean
                    }
                });
            }
            Check::Binary(op, right_node) => {
                let right = pop(&mut types)?;
                let left = pop(&mut types)?;
                types.push(binary_type(op, left, right, right_node)?);
            }
            Check::Call(function, count) => {
                let start = types
                    .len()
                    .checked_sub(count)
                    .ok_or("a call has too few arguments")?;
                let args = types.split_off(start);
                types.push(call_type(function, &args)?);
            }
            Check::If(cases) => {
                let mut taken = pop(&mut types)?;
                for _ in 0..cases {
                    let value = pop(&mut types)?;
                    boolean_type(pop(&mut types)?, "if")?;
                    taken = branch_type(value, taken)?;
                }
                types.push(taken);
            }
        }
    }
    Ok(pop(&mut types)?)
}

/// Whether a type is a value of the declared kind; an unknown type may be.
///
/// # Errors
///
/// Returns the difference where it is not.
pub(crate) fn kind_type(kind: &ValueKind, found: Type) -> Result<(), String> {
    match (kind, found) {
        (ValueKind::Native(name), _) => Err(format!("the kind {name} has no neutral value")),
        (_, Type::Unknown) => Ok(()),
        (kind, found) if Type::of_kind(kind) == Some(found) => Ok(()),
        (kind, found) => Err(format!("{found:?} is not a value of the kind {kind:?}")),
    }
}

/// A cache as an expression value of the declared kind, or `None` where the
/// two do not agree.
pub fn cache_value(kind: &ValueKind, cache: &ParameterValue) -> Option<Value> {
    match (kind, cache) {
        (ValueKind::Quantity(quantity), ParameterValue::Number(value)) => Some(Value::Number {
            value: value.get(),
            quantity: *quantity,
        }),
        (ValueKind::Integer, ParameterValue::Integer(value)) => Some(Value::Number {
            value: f64_from_i64(*value)?,
            quantity: DIMENSIONLESS,
        }),
        (ValueKind::Boolean, ParameterValue::Boolean(value)) => Some(Value::Boolean(*value)),
        (ValueKind::Text, ParameterValue::Text(value)) => Some(Value::Text(value.clone())),
        _ => None,
    }
}

/// An expression value as a cache of the declared kind.
///
/// # Errors
///
/// Returns the difference where the value does not have the kind: another
/// quantity, a fraction for an integer, another variant, or a `Native` kind.
pub fn kind_value(kind: &ValueKind, value: Value) -> Result<ParameterValue, String> {
    match (kind, value) {
        (ValueKind::Quantity(expected), Value::Number { value, quantity }) => {
            if *expected != quantity {
                return Err(format!(
                    "the value has the quantity {quantity:?}, not {expected:?}"
                ));
            }
            FiniteReal::new(value)
                .map(ParameterValue::Number)
                .ok_or_else(|| "the value is not finite".into())
        }
        (ValueKind::Integer, Value::Number { value, quantity }) => {
            if quantity != DIMENSIONLESS {
                return Err("an integer is dimensionless".into());
            }
            if value.fract() != 0.0 {
                return Err(format!("{value} is not a whole number"));
            }
            cadmpeg_core::convert::truncate_f64_to_i64(value)
                .map(ParameterValue::Integer)
                .ok_or_else(|| format!("{value} is outside the integer range"))
        }
        (ValueKind::Boolean, Value::Boolean(value)) => Ok(ParameterValue::Boolean(value)),
        (ValueKind::Text, Value::Text(value)) => Ok(ParameterValue::Text(value)),
        (ValueKind::Native(name), _) => Err(format!("the kind {name} has no neutral value")),
        (kind, value) => Err(format!("{value:?} is not a value of the kind {kind:?}")),
    }
}
