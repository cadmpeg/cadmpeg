// SPDX-License-Identifier: Apache-2.0
//! Evaluation of neutral expression trees on canonical values (§5.13).
//!
//! Every node has a quantity or is Boolean or Text, and every operator checks
//! it. A literal converts from its authored unit to the canonical unit of its
//! quantity; a reference takes its parameter's cache; a measure takes its
//! cached value. The walk keeps its own stack, so a tree of any depth
//! evaluates on a small thread.

use cadmpeg_core::convert::{f64_from_i64, truncate_f64_to_i32};

use crate::design::expr::{BinaryOp, Constant, Function, MeasureKind, Node, UnaryOp};
use crate::design::parameter::ParameterValue;
use crate::design::units::{Quantity, Unit, UnitSymbol, ValueKind};
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
const TIME: Quantity = Quantity {
    length: 0,
    mass: 0,
    angle: 0,
    time: 1,
};
const VOLUME: Quantity = Quantity {
    length: 3,
    mass: 0,
    angle: 0,
    time: 0,
};

/// The factor from a symbol to the canonical unit of its quantity, and that
/// quantity.
fn symbol(symbol: UnitSymbol) -> (f64, Quantity) {
    use std::f64::consts::PI;
    match symbol {
        UnitSymbol::Nm => (1.0 / 1_000_000.0, Quantity::LENGTH),
        UnitSymbol::Um => (1.0 / 1_000.0, Quantity::LENGTH),
        UnitSymbol::Mm => (1.0, Quantity::LENGTH),
        UnitSymbol::Cm => (10.0, Quantity::LENGTH),
        UnitSymbol::M => (1_000.0, Quantity::LENGTH),
        UnitSymbol::Km => (1_000_000.0, Quantity::LENGTH),
        UnitSymbol::In => (25.4, Quantity::LENGTH),
        UnitSymbol::Ft => (304.8, Quantity::LENGTH),
        UnitSymbol::Yd => (914.4, Quantity::LENGTH),
        UnitSymbol::Mi => (1_609_344.0, Quantity::LENGTH),
        UnitSymbol::Rad => (1.0, Quantity::ANGLE),
        UnitSymbol::Deg => (PI / 180.0, Quantity::ANGLE),
        UnitSymbol::Grad => (PI / 200.0, Quantity::ANGLE),
        UnitSymbol::Arcmin => (PI / 10_800.0, Quantity::ANGLE),
        UnitSymbol::Arcsec => (PI / 648_000.0, Quantity::ANGLE),
        UnitSymbol::Rev => (2.0 * PI, Quantity::ANGLE),
        UnitSymbol::S => (1.0, TIME),
        UnitSymbol::Min => (60.0, TIME),
        UnitSymbol::H => (3_600.0, TIME),
        UnitSymbol::G => (1.0 / 1_000.0, MASS),
        UnitSymbol::Kg => (1.0, MASS),
        UnitSymbol::Lb => (0.453_592_37, MASS),
    }
}

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
pub(super) fn unit_factor(unit: &Unit) -> Result<(f64, Quantity), EvalError> {
    let mut factor = 1.0;
    let mut quantity = DIMENSIONLESS;
    for item in &unit.0 {
        let (scale, of) = symbol(item.symbol);
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

fn binary<R>(
    op: BinaryOp,
    left: Value,
    right: Value,
    right_node: &Node<R>,
) -> Result<Value, EvalError> {
    match op {
        BinaryOp::Add | BinaryOp::Subtract => {
            let (left, a) = number(&left, "+ and -")?;
            let (right, b) = number(&right, "+ and -")?;
            let quantity = same(a, b, "+ and -")?;
            let value = if op == BinaryOp::Add {
                left + right
            } else {
                left - right
            };
            finite(value, quantity)
        }
        BinaryOp::Multiply => {
            let (left, a) = number(&left, "*")?;
            let (right, b) = number(&right, "*")?;
            finite(left * right, combined(a, b, 1)?)
        }
        BinaryOp::Divide => {
            let (left, a) = number(&left, "/")?;
            let (right, b) = number(&right, "/")?;
            if right == 0.0 {
                return Err(EvalError::Domain("division by zero"));
            }
            finite(left / right, combined(a, b, -1)?)
        }
        BinaryOp::Power => {
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
        BinaryOp::Equal
        | BinaryOp::NotEqual
        | BinaryOp::Less
        | BinaryOp::LessEqual
        | BinaryOp::Greater
        | BinaryOp::GreaterEqual => compare(op, left, right),
        BinaryOp::And => Ok(Value::Boolean(
            boolean(&left, "and")? & boolean(&right, "and")?,
        )),
        BinaryOp::Or => Ok(Value::Boolean(
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

enum Task<'a, R> {
    Visit(&'a Node<R>),
    Unary(UnaryOp),
    Binary(BinaryOp, &'a Node<R>),
    Call(Function, usize),
    Branch(&'a Node<R>, &'a Node<R>),
}

/// Evaluates a neutral tree on canonical values. `reference` gives a
/// reference's value, or `None` where it has none.
///
/// # Errors
///
/// Returns [`EvalError::Unbound`] where a reference, a measure, the law
/// parameter, or a bend variable has no value, [`EvalError::Type`] where an
/// operand does not have the quantity or kind its operator takes, and
/// [`EvalError::Domain`] for an operation outside its domain.
pub fn evaluate<R>(
    root: &Node<R>,
    reference: &mut dyn FnMut(&R) -> Option<Value>,
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
                Node::Binary { op, left, right } => {
                    tasks.push(Task::Binary(*op, right));
                    tasks.push(Task::Visit(right));
                    tasks.push(Task::Visit(left));
                }
                Node::Call { function, args } => {
                    tasks.push(Task::Call(*function, args.len()));
                    tasks.extend(args.iter().rev().map(Task::Visit));
                }
                Node::If(branches) => {
                    let [condition, then, otherwise] = branches.as_ref();
                    tasks.push(Task::Branch(then, otherwise));
                    tasks.push(Task::Visit(condition));
                }
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
            Task::Branch(then, otherwise) => {
                let taken = if boolean(&pop(&mut values)?, "if")? {
                    then
                } else {
                    otherwise
                };
                tasks.push(Task::Visit(taken));
            }
        }
    }
    pop(&mut values)
}

/// A cache as an expression value of the declared kind, or `None` where the
/// two do not agree.
pub(super) fn cache_value(kind: &ValueKind, cache: &ParameterValue) -> Option<Value> {
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
pub(super) fn kind_value(kind: &ValueKind, value: Value) -> Result<ParameterValue, String> {
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
