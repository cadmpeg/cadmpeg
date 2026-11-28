// SPDX-License-Identifier: Apache-2.0
//! A formula as `FreeCAD`'s `ExpressionParser` reads it, and the kind of its
//! result as `FreeCAD` computes it: the unit rules of its operators and
//! functions over the kinds of the properties it references.

use cadmpeg_ir::design::units::Quantity;

use super::{unit_word, Cursor, Unit};

/// The most tokens of a formula the reader parses.
const MAX_TOKENS: usize = 1024;

/// The deepest nesting of parentheses, calls, and signs the reader parses.
const MAX_NESTING: usize = 64;

/// The distance from an integer within which `FreeCAD` takes an exponent
/// of a unit as that integer.
const INTEGRAL: f64 = 1e-9;

/// The magnitude below which an integral number is a Python `int`
/// (`essentiallyInteger`, the range of a C `long`).
const WHOLE: f64 = 9.223_372_036_854_776e18;

/// A formula the reader does not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::design) enum Unparsed {
    /// `FreeCAD`'s parser also refuses the text.
    Syntax,
    /// The text is larger or deeper than the reader parses.
    Limit,
}

/// The kind of a formula's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::design) enum Kind {
    Quantity(Quantity),
    /// A Python `bool`: a comparison, `True`, `False`, or a `Bool` property.
    Boolean,
    Text,
    /// A kind that the file does not determine.
    Unknown,
}

/// A node of a parsed formula.
#[derive(Debug)]
pub(in crate::design) enum Node<'a> {
    /// A number as written, with the unit that follows it.
    Number {
        text: &'a str,
        unit: Option<Unit>,
    },
    /// A unit with no number: a whole formula, or the divisor of `x / unit`.
    Unit(Unit),
    /// `pi`, `e`, `True`, `False`, or `None`.
    Constant {
        name: &'a str,
        value: f64,
    },
    /// A `<<…>>` string.
    Text,
    Path(Path),
    Negate(Box<Node<'a>>),
    Plus(Box<Node<'a>>),
    Binary {
        op: Operator,
        left: Box<Node<'a>>,
        right: Box<Node<'a>>,
    },
    /// `condition ? then : otherwise`.
    Conditional {
        condition: Box<Node<'a>>,
        then: Box<Node<'a>>,
        otherwise: Box<Node<'a>>,
    },
    Call {
        name: &'a str,
        args: Vec<Argument<'a>>,
    },
    Group(Box<Node<'a>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::design) enum Operator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Power,
    /// `==` or `!=`.
    Equal,
    /// `<`, `<=`, `>`, or `>=`.
    Order,
}

/// The Python type of a value as `FreeCAD` hands it to an operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::design) enum Python {
    /// An `int` or a `bool`.
    Int,
    Float,
    /// A plain number whose value the file does not store: an `int` where
    /// the value is whole, and a `float` otherwise.
    Unsettled,
    /// A `QuantityPy`, a `str`, or another object.
    Object,
    /// `None`, a `tuple`, or a `set`: a value that is no number, which
    /// `FreeCAD`'s operators refuse and an aggregate passes over.
    Refused,
    /// A value whose evaluation raises.
    Raises,
    /// A value of a type the file does not state, which `FreeCAD`'s
    /// operators can refuse: a Python object property, or a conditional
    /// with one branch refused or raising.
    Opaque,
}

impl Python {
    /// The value is one that `FreeCAD`'s operators can refuse.
    const fn refused(self) -> bool {
        matches!(self, Self::Refused | Self::Raises | Self::Opaque)
    }

    /// The type of the plain number `value`: an `int` where it is whole
    /// (`pyFromQuantity`, `essentiallyInteger`).
    pub(in crate::design) fn of(value: f64) -> Self {
        if value.fract() == 0.0 && value.abs() < WHOLE {
            Self::Int
        } else {
            Self::Float
        }
    }
}

/// The kind of a value, and its Python type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::design) struct Value {
    pub(in crate::design) kind: Kind,
    pub(in crate::design) python: Python,
}

impl Value {
    pub(in crate::design) const fn new(kind: Kind, python: Python) -> Self {
        Self { kind, python }
    }
}

const REAL: Kind = Kind::Quantity(Quantity::REAL);

/// An argument of a call: a value, or a range of cells `from:to`.
#[derive(Debug)]
pub(in crate::design) enum Argument<'a> {
    Value(Node<'a>),
    Range(&'a str, &'a str),
}

/// A reference to a property, or to a cell of a sheet.
#[derive(Debug)]
pub(in crate::design) struct Path {
    /// The byte range of the path in the formula.
    pub(in crate::design) span: std::ops::Range<usize>,
    pub(in crate::design) origin: Origin,
    pub(in crate::design) components: Vec<String>,
    /// The path has an indexer.
    pub(in crate::design) indexed: bool,
}

/// Where a path starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::design) enum Origin {
    /// An object name, or the formula's own object where the path has one
    /// component.
    Named,
    /// `.`: the formula's own object.
    Own,
    /// A `<<label>>` of an object.
    Label,
    /// Another document, or a sub-object.
    Outside,
}

/// A reference of a formula: a path, or a range of cells.
#[derive(Debug, Clone, Copy)]
pub(in crate::design) enum Reference<'n, 'a> {
    Path(&'n Path),
    Range(&'a str, &'a str),
}

/// What the references of a formula name.
pub(in crate::design) trait Scope {
    /// The value of the property or cell that `path` names.
    fn path(&mut self, path: &Path) -> Value;
    /// The values of the used cells from `from` to `to` of the formula's
    /// sheet, where the range resolves.
    fn range(&mut self, from: &str, to: &str) -> Option<Vec<Value>>;
    /// The formula's kind takes a plain number whose value the file does not
    /// store as an `int`.
    fn assume_integer(&mut self);
}

/// The tree of `text`, a formula without its `=`.
pub(in crate::design) fn parse(text: &str) -> Result<Node<'_>, Unparsed> {
    let lexemes = lex(text).ok_or(Unparsed::Syntax)?;
    if lexemes.len() > MAX_TOKENS {
        return Err(Unparsed::Limit);
    }
    let mut parser = Parser {
        lexemes,
        at: 0,
        nesting: 0,
    };
    if let Some(unit) = parser.attempt(Parser::unit_expression) {
        if parser.done() {
            return Ok(Node::Unit(unit));
        }
        parser.at = 0;
    }
    let node = parser.expression()?;
    if parser.done() {
        Ok(node)
    } else {
        Err(Unparsed::Syntax)
    }
}

impl<'a> Node<'a> {
    /// The kind of the node's result.
    pub(in crate::design) fn kind(&self, scope: &mut dyn Scope) -> Kind {
        self.value(scope).kind
    }

    /// The kind of the node's result, and the Python type `FreeCAD` hands to
    /// an operator: a plain number goes through `pyFromQuantity`, which
    /// makes a whole dimensionless value an `int`, and every function other
    /// than an aggregate returns a `QuantityPy`.
    pub(in crate::design) fn value(&self, scope: &mut dyn Scope) -> Value {
        match self {
            Node::Number { text, unit: None } => Value::new(
                REAL,
                text.replace(',', ".")
                    .parse::<f64>()
                    .map_or(Python::Object, Python::of),
            ),
            Node::Number {
                unit: Some(unit), ..
            }
            | Node::Unit(unit) => Value::new(unit_kind(unit), Python::Object),
            Node::Constant { name, .. } => match *name {
                "None" => Value::new(Kind::Unknown, Python::Refused),
                "True" | "False" => Value::new(Kind::Boolean, Python::Int),
                _ => Value::new(REAL, Python::Float),
            },
            Node::Text => Value::new(Kind::Text, Python::Object),
            Node::Path(path) => scope.path(path),
            Node::Negate(operand) | Node::Plus(operand) => {
                let operand = operand.value(scope);
                match numeric(operand.kind) {
                    Some(quantity) => Value::new(Kind::Quantity(quantity), operand.python),
                    None if operand.kind == Kind::Unknown
                        && !matches!(operand.python, Python::Refused | Python::Raises) =>
                    {
                        operand
                    }
                    None => Value::new(Kind::Unknown, Python::Raises),
                }
            }
            Node::Group(inner) => inner.value(scope),
            Node::Binary { op, left, right } => {
                let exponent = right.constant();
                binary(*op, left.value(scope), right.value(scope), exponent, scope)
            }
            Node::Conditional {
                then, otherwise, ..
            } => {
                let (then, otherwise) = (then.value(scope), otherwise.value(scope));
                let kind = if then.kind == otherwise.kind {
                    then.kind
                } else {
                    Kind::Unknown
                };
                let python = match (then.python, otherwise.python) {
                    (then, otherwise) if then == otherwise => then,
                    (then, otherwise) if then.refused() || otherwise.refused() => Python::Opaque,
                    (Python::Object, _) | (_, Python::Object) => Python::Object,
                    _ => Python::Unsettled,
                };
                Value::new(kind, python)
            }
            Node::Call { name, args } => call(name, args, scope),
        }
    }

    /// The Python type of a cell whose formula is the node and whose result
    /// is `value` (`Sheet::updateProperty`): a bool, and a dimensionless
    /// number where it is whole, are `int` values.
    pub(in crate::design) fn stored(&self, value: Value) -> Python {
        match value.kind {
            Kind::Boolean => Python::Int,
            Kind::Quantity(Quantity::REAL) => match (self.constant(), value.python) {
                (Some(constant), _) => Python::of(constant),
                (None, Python::Int) => Python::Int,
                _ => Python::Unsettled,
            },
            Kind::Unknown if value.python.refused() => value.python,
            Kind::Unknown => Python::Unsettled,
            Kind::Quantity(_) | Kind::Text => Python::Object,
        }
    }

    /// Each reference of the node, in source order.
    pub(in crate::design) fn references(&self) -> Vec<Reference<'_, 'a>> {
        let mut found = Vec::new();
        self.collect(&mut found);
        found
    }

    fn collect<'n>(&'n self, found: &mut Vec<Reference<'n, 'a>>) {
        match self {
            Node::Path(path) => found.push(Reference::Path(path)),
            Node::Negate(inner) | Node::Plus(inner) | Node::Group(inner) => inner.collect(found),
            Node::Binary { left, right, .. } => {
                left.collect(found);
                right.collect(found);
            }
            Node::Conditional {
                condition,
                then,
                otherwise,
            } => {
                condition.collect(found);
                then.collect(found);
                otherwise.collect(found);
            }
            Node::Call { args, .. } => {
                for arg in args {
                    match arg {
                        Argument::Value(node) => node.collect(found),
                        Argument::Range(from, to) => found.push(Reference::Range(from, to)),
                    }
                }
            }
            Node::Number { .. } | Node::Unit(_) | Node::Constant { .. } | Node::Text => {}
        }
    }

    /// The value of a node of numbers and constants only.
    fn constant(&self) -> Option<f64> {
        match self {
            Node::Number { text, unit: None } => text.replace(',', ".").parse().ok(),
            Node::Constant { name, value } => (*name != "None").then_some(*value),
            Node::Call { name, args } => fold(name, args),
            Node::Negate(operand) => operand.constant().map(|value| -value),
            Node::Plus(operand) | Node::Group(operand) => operand.constant(),
            Node::Binary { op, left, right } => {
                let (left, right) = (left.constant()?, right.constant()?);
                match op {
                    Operator::Add => Some(left + right),
                    Operator::Subtract => Some(left - right),
                    Operator::Multiply => Some(left * right),
                    Operator::Divide => Some(left / right),
                    Operator::Power => Some(left.powf(right)),
                    Operator::Remainder | Operator::Equal | Operator::Order => None,
                }
            }
            _ => None,
        }
    }
}

fn unit_kind(unit: &Unit) -> Kind {
    if unit.native {
        Kind::Unknown
    } else {
        Kind::Quantity(unit.quantity())
    }
}

/// `left op right` (`OperatorExpression`, `calc`, Python's number
/// protocol). `calc` refuses a `None`, `tuple`, or `set` operand, except the
/// right operand of a string's `%`. A bool is an `int`. `==` and `!=` give a
/// bool for any two other values; an ordering gives a bool for two texts,
/// two numbers of one quantity, a bool and a number, and a quantity and a
/// plain number. A string times an `int` repeats the string. A quantity's
/// `%` keeps its unit, and a plain number's `%` of a quantity raises. An
/// operator of two values of known kinds whose result has no kind raises.
fn binary(
    op: Operator,
    left: Value,
    right: Value,
    exponent: Option<f64>,
    scope: &mut dyn Scope,
) -> Value {
    let quantity = |quantity: Option<Quantity>| quantity.map_or(Kind::Unknown, Kind::Quantity);
    let plain = |python: Python| matches!(python, Python::Int | Python::Float | Python::Unsettled);
    if let (Operator::Remainder, Kind::Text) = (op, left.kind) {
        return Value::new(Kind::Text, Python::Object);
    }
    let raising = |python: Python| matches!(python, Python::Refused | Python::Raises);
    if raising(left.python) || raising(right.python) {
        return Value::new(Kind::Unknown, Python::Raises);
    }
    if op == Operator::Equal && (left.python.refused() || right.python.refused()) {
        return Value::new(Kind::Unknown, Python::Opaque);
    }
    let kind = match (op, left.kind, right.kind) {
        (Operator::Add, Kind::Text, Kind::Text) => Kind::Text,
        (Operator::Multiply, Kind::Text, REAL | Kind::Boolean) => repeated(right.python, scope),
        (Operator::Multiply, REAL | Kind::Boolean, Kind::Text) => repeated(left.python, scope),
        (Operator::Equal, _, _)
        | (Operator::Order, Kind::Text, Kind::Text)
        | (Operator::Order, Kind::Boolean, Kind::Boolean | Kind::Quantity(_))
        | (Operator::Order, Kind::Quantity(_), Kind::Boolean) => Kind::Boolean,
        (Operator::Order, Kind::Quantity(left_quantity), Kind::Quantity(right_quantity))
            if left_quantity == right_quantity
                || (left_quantity == Quantity::REAL && plain(left.python))
                || (right_quantity == Quantity::REAL && plain(right.python)) =>
        {
            Kind::Boolean
        }
        (Operator::Order, _, _) => Kind::Unknown,
        _ => match (numeric(left.kind), numeric(right.kind)) {
            (Some(left_quantity), Some(right_quantity)) => match op {
                Operator::Add | Operator::Subtract => {
                    quantity((left_quantity == right_quantity).then_some(left_quantity))
                }
                Operator::Multiply => quantity(combine(left_quantity, right_quantity, 1)),
                Operator::Divide => quantity(combine(left_quantity, right_quantity, -1)),
                Operator::Remainder => quantity(
                    (left.python == Python::Object || right.python != Python::Object)
                        .then_some(left_quantity),
                ),
                Operator::Power => power(left_quantity, right_quantity, exponent),
                Operator::Equal | Operator::Order => Kind::Unknown,
            },
            _ => Kind::Unknown,
        },
    };
    // A power of a quantity by a dimensionless value the file does not store
    // takes its unit at run time.
    let deferred = op == Operator::Power && exponent.is_none() && right.kind == REAL;
    let python = match kind {
        Kind::Text => Python::Object,
        Kind::Boolean => Python::Int,
        Kind::Unknown if left.kind != Kind::Unknown && right.kind != Kind::Unknown && !deferred => {
            Python::Raises
        }
        Kind::Quantity(_) | Kind::Unknown => arithmetic(op, left.python, right.python, exponent),
    };
    Value::new(kind, python)
}

/// The kind of a string times a plain number of Python type `python`: a
/// string where the number is an `int`. A number of unstated value is taken
/// as an `int`, the one value with which the formula evaluates.
fn repeated(python: Python, scope: &mut dyn Scope) -> Kind {
    match python {
        Python::Int => Kind::Text,
        Python::Unsettled => {
            scope.assume_integer();
            Kind::Text
        }
        Python::Float | Python::Object | Python::Refused | Python::Raises | Python::Opaque => {
            Kind::Unknown
        }
    }
}

/// The Python type of `left op right` for numbers (Python's number protocol):
/// `/` gives a `float`, and `**` of `int` values a `float` for a negative
/// exponent.
fn arithmetic(op: Operator, left: Python, right: Python, exponent: Option<f64>) -> Python {
    let both = match (left, right) {
        (Python::Refused | Python::Raises, _) | (_, Python::Refused | Python::Raises) => {
            Python::Raises
        }
        (Python::Opaque, _) | (_, Python::Opaque) => Python::Opaque,
        (Python::Object, _) | (_, Python::Object) => Python::Object,
        (Python::Float, _) | (_, Python::Float) => Python::Float,
        (Python::Int, Python::Int) => Python::Int,
        _ => Python::Unsettled,
    };
    match (op, both) {
        (_, Python::Raises | Python::Opaque | Python::Object) => both,
        (Operator::Divide, _) => Python::Float,
        (Operator::Power, Python::Int) => match exponent {
            Some(exponent) if exponent < 0.0 => Python::Float,
            Some(_) => Python::Int,
            None => Python::Unsettled,
        },
        (Operator::Equal | Operator::Order, _) => Python::Int,
        _ => both,
    }
}

/// The quantity of a number: a bool is a dimensionless `int`.
fn numeric(kind: Kind) -> Option<Quantity> {
    match kind {
        Kind::Quantity(quantity) => Some(quantity),
        Kind::Boolean => Some(Quantity::REAL),
        Kind::Text | Kind::Unknown => None,
    }
}

/// `base ^ exponent`: the exponent is dimensionless, and with a unit it is
/// a constant whose product with each exponent of the unit is an integer.
fn power(base: Quantity, exponent: Quantity, value: Option<f64>) -> Kind {
    if exponent != Quantity::REAL {
        return Kind::Unknown;
    }
    if base == Quantity::REAL {
        return Kind::Quantity(Quantity::REAL);
    }
    value
        .and_then(|value| scaled(base, value))
        .map_or(Kind::Unknown, Kind::Quantity)
}

fn combine(left: Quantity, right: Quantity, sign: i8) -> Option<Quantity> {
    let one = |left: i8, right: i8| left.checked_add(right.checked_mul(sign)?);
    Some(Quantity::of(
        one(left.length, right.length)?,
        one(left.mass, right.mass)?,
        one(left.angle, right.angle)?,
        one(left.time, right.time)?,
    ))
}

/// Each exponent of `quantity` times `factor`, where each product is an
/// integer.
fn scaled(quantity: Quantity, factor: f64) -> Option<Quantity> {
    let one = |exponent: i8| {
        let product = f64::from(exponent) * factor;
        (i8::MIN..=i8::MAX).find(|integer| (f64::from(*integer) - product).abs() < INTEGRAL)
    };
    Some(Quantity::of(
        one(quantity.length)?,
        one(quantity.mass)?,
        one(quantity.angle)?,
        one(quantity.time)?,
    ))
}

/// The functions that take each of their first three arguments as a number
/// (`pyToQuantity`) and check its unit (`FunctionExpression::evaluate`).
const NUMERIC: [&str; 25] = [
    "cos", "sin", "tan", "acos", "asin", "atan", "exp", "log", "log10", "sinh", "tanh", "cosh",
    "round", "trunc", "ceil", "floor", "abs", "sqrt", "cbrt", "atan2", "mod", "pow", "hypot",
    "cath", "not",
];

/// The value of a call, by `FunctionExpression`'s unit rules: an aggregate
/// and `count`, `and`, and `or` go through `pyFromQuantity`, `tuple` gives
/// a tuple, and every other function returns a `QuantityPy`. A numeric
/// function raises on an argument that is no number and on a unit it does
/// not take.
fn call(name: &str, args: &[Argument<'_>], scope: &mut dyn Scope) -> Value {
    const ANGLE: Kind = Kind::Quantity(Quantity::ANGLE);
    const LENGTH: Kind = Kind::Quantity(Quantity::LENGTH);
    if matches!(name, "sum" | "min" | "max" | "average" | "stddev") {
        return aggregate(name, args, scope);
    }
    let values = args
        .iter()
        .map(|arg| match arg {
            Argument::Value(node) => node.value(scope),
            Argument::Range(..) => Value::new(Kind::Unknown, Python::Object),
        })
        .collect::<Vec<_>>();
    let value = |index: usize| {
        values
            .get(index)
            .copied()
            .unwrap_or(Value::new(Kind::Unknown, Python::Object))
    };
    let first = value(0);
    if matches!(name, "href" | "hiddenref") {
        return first;
    }
    if name == "tuple" {
        return Value::new(Kind::Unknown, Python::Refused);
    }
    let first_quantity = numeric(first.kind);
    let kind = match name {
        "cos" | "sin" | "tan" => {
            if matches!(first_quantity, Some(Quantity::REAL | Quantity::ANGLE)) {
                REAL
            } else {
                Kind::Unknown
            }
        }
        "acos" | "asin" | "atan" => {
            if first_quantity == Some(Quantity::REAL) {
                ANGLE
            } else {
                Kind::Unknown
            }
        }
        "exp" | "log" | "log10" | "sinh" | "tanh" | "cosh" => {
            if first_quantity == Some(Quantity::REAL) {
                REAL
            } else {
                Kind::Unknown
            }
        }
        "round" | "trunc" | "ceil" | "floor" | "abs" => {
            first_quantity.map_or(Kind::Unknown, Kind::Quantity)
        }
        "sqrt" => first_quantity
            .and_then(|quantity| scaled(quantity, 0.5))
            .map_or(Kind::Unknown, Kind::Quantity),
        "cbrt" => first_quantity
            .and_then(|quantity| scaled(quantity, 1.0 / 3.0))
            .map_or(Kind::Unknown, Kind::Quantity),
        "atan2" => {
            if first_quantity.is_some() && numeric(value(1).kind) == first_quantity {
                ANGLE
            } else {
                Kind::Unknown
            }
        }
        "mod" => match (first_quantity, numeric(value(1).kind)) {
            (Some(left), Some(right))
                if left == right || left == Quantity::REAL || right == Quantity::REAL =>
            {
                Kind::Quantity(left)
            }
            _ => Kind::Unknown,
        },
        "pow" => match (first_quantity, numeric(value(1).kind), args.get(1)) {
            (Some(base), Some(exponent), Some(Argument::Value(node))) => {
                power(base, exponent, node.constant())
            }
            _ => Kind::Unknown,
        },
        "hypot" | "cath" => {
            let rest =
                (1..args.len().max(2)).all(|index| numeric(value(index).kind) == first_quantity);
            if let (Some(quantity), true) = (first_quantity, rest) {
                Kind::Quantity(quantity)
            } else {
                Kind::Unknown
            }
        }
        "not" | "and" | "or" | "count" | "vdot" => REAL,
        "str" | "address" => Kind::Text,
        "vangle" => ANGLE,
        "vlinedist" | "vplanedist" => LENGTH,
        _ => Kind::Unknown,
    };
    if NUMERIC.contains(&name) {
        let checked = &values[..values.len().min(3)];
        // A power of a quantity by a dimensionless value the file does not
        // store takes its unit at run time.
        let deferred = name == "pow"
            && value(1).kind == REAL
            && matches!(args.get(1), Some(Argument::Value(node)) if node.constant().is_none());
        let raises = checked.iter().any(|value| {
            value.kind == Kind::Text || matches!(value.python, Python::Refused | Python::Raises)
        }) || (kind == Kind::Unknown
            && !deferred
            && checked.iter().all(|value| value.kind != Kind::Unknown));
        if raises {
            return Value::new(Kind::Unknown, Python::Raises);
        }
    }
    let python = match name {
        "count" | "and" | "or" => Python::Int,
        _ => Python::Object,
    };
    Value::new(kind, python)
}

/// The value of an aggregate: the quantity its values share. It passes over
/// a value that is no number (`pyToQuantity`) and counts a bool as a
/// dimensionless `int`; a text, bool, or `None` cell of a range has no
/// quantity, and `FreeCAD` raises on it, on an argument that raises, and on
/// values of different quantities. A dimensionless `sum`, `min`, or `max` of
/// `int` values is an `int`, and an aggregate of constants is its value.
fn aggregate(name: &str, args: &[Argument<'_>], scope: &mut dyn Scope) -> Value {
    let raises = Value::new(Kind::Unknown, Python::Raises);
    let unknown = Value::new(Kind::Unknown, Python::Object);
    let mut found: Option<Quantity> = None;
    let mut whole = true;
    for arg in args {
        let values = match arg {
            Argument::Value(node) => {
                let value = node.value(scope);
                match (value.kind, value.python) {
                    (_, Python::Raises) => return raises,
                    (Kind::Text, _) | (_, Python::Refused) => continue,
                    (Kind::Boolean, _) => vec![Value::new(REAL, Python::Int)],
                    _ => vec![value],
                }
            }
            Argument::Range(from, to) => {
                let Some(values) = scope.range(from, to) else {
                    return unknown;
                };
                values
            }
        };
        for value in values {
            let quantity = match value.kind {
                Kind::Quantity(quantity) => quantity,
                Kind::Unknown if !matches!(value.python, Python::Refused | Python::Raises) => {
                    return unknown;
                }
                Kind::Unknown | Kind::Text | Kind::Boolean => return raises,
            };
            if found.is_some_and(|found| found != quantity) {
                return raises;
            }
            found = Some(quantity);
            whole &= value.python == Python::Int;
        }
    }
    let quantity = found.unwrap_or(Quantity::REAL);
    let python = if quantity != Quantity::REAL {
        Python::Object
    } else if let Some(value) = fold(name, args) {
        Python::of(value)
    } else if whole && matches!(name, "sum" | "min" | "max") {
        Python::Int
    } else {
        Python::Unsettled
    };
    Value::new(Kind::Quantity(quantity), python)
}

/// The value of `sum`, `min`, `max`, or `average` of constants
/// (`SumCollector`, `MinCollector`, `MaxCollector`, `AverageCollector`).
fn fold(name: &str, args: &[Argument<'_>]) -> Option<f64> {
    let values = args
        .iter()
        .map(|arg| match arg {
            Argument::Value(node) => node.constant(),
            Argument::Range(..) => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let (first, rest) = values.split_first()?;
    let sum = || values.iter().sum::<f64>();
    match name {
        "sum" => Some(sum()),
        "average" => Some(sum() / f64::from(u32::try_from(values.len()).ok()?)),
        "min" => Some(rest.iter().fold(*first, |least, value| least.min(*value))),
        "max" => Some(rest.iter().fold(*first, |most, value| most.max(*value))),
        _ => None,
    }
}

#[derive(Debug, Clone)]
enum Token<'a> {
    Number {
        text: &'a str,
        integer: Option<i8>,
    },
    Unit(Unit),
    /// `'` or `"`.
    UsUnit(Unit),
    Constant {
        name: &'a str,
        value: f64,
    },
    Text(&'a str),
    Identifier(&'a str),
    Cell(&'a str),
    /// A function name with its `(`.
    Function(&'a str),
    Minus,
    Compare(Operator),
    Symbol(char),
}

#[derive(Debug, Clone)]
struct Lexeme<'a> {
    token: Token<'a>,
    start: usize,
    end: usize,
}

/// The tokens of `text` as `FreeCAD`'s lexer reads them: the longest match,
/// and on a tie the earlier rule.
fn lex(text: &str) -> Option<Vec<Lexeme<'_>>> {
    let mut lexemes = Vec::new();
    let mut at = 0;
    while let Some(ch) = text[at..].chars().next() {
        if matches!(ch, ' ' | '\t' | '\n') {
            at += 1;
            continue;
        }
        let (token, length) = lexeme(&text[at..])?;
        lexemes.push(Lexeme {
            token,
            start: at,
            end: at + length,
        });
        at += length;
    }
    Some(lexemes)
}

fn lexeme(rest: &str) -> Option<(Token<'_>, usize)> {
    let ch = rest.chars().next()?;
    if rest.starts_with("<<") {
        if let Some(length) = string(rest) {
            return Some((Token::Text(&rest[..length]), length));
        }
    }
    if ["==", "!="].iter().any(|symbol| rest.starts_with(symbol)) {
        return Some((Token::Compare(Operator::Equal), 2));
    }
    if [">=", "<="].iter().any(|symbol| rest.starts_with(symbol)) {
        return Some((Token::Compare(Operator::Order), 2));
    }
    match ch {
        '<' | '>' => Some((Token::Compare(Operator::Order), 1)),
        '-' | '\u{2212}' => Some((Token::Minus, ch.len_utf8())),
        '\'' | '"' => Some((Token::UsUnit(unit_word(&rest[..1])?), 1)),
        '°' | '′' | '″' => Some((
            Token::Unit(unit_word(&rest[..ch.len_utf8()])?),
            ch.len_utf8(),
        )),
        '$' => cell_address(rest).map(|length| (Token::Cell(&rest[..length]), length)),
        '.' | ',' | '0'..='9' => {
            number(rest).or_else(|| (!ch.is_ascii_digit()).then_some((Token::Symbol(ch), 1)))
        }
        '%' | '+' | '(' | ')' | '=' | '/' | '*' | '^' | '{' | '}' | '[' | ']' | ':' | ';' | '@'
        | '?' | '#' => Some((Token::Symbol(ch), 1)),
        ch if ch.is_alphabetic() || ch == '_' => Some(word(rest)),
        _ => None,
    }
}

/// The length of a `<<…>>` string at the start of `rest`.
fn string(rest: &str) -> Option<usize> {
    let mut chars = rest.char_indices().skip(2);
    while let Some((at, ch)) = chars.next() {
        match ch {
            '\\' => {
                chars.next()?;
            }
            '>' => return rest[at..].starts_with(">>").then_some(at + 2),
            '\n' => return None,
            _ => {}
        }
    }
    None
}

fn number(rest: &str) -> Option<(Token<'_>, usize)> {
    let mut cursor = Cursor { text: rest, at: 0 };
    cursor.number()?;
    let text = &rest[..cursor.at];
    let integer = text
        .bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| text.parse().ok())
        .flatten();
    Some((Token::Number { text, integer }, cursor.at))
}

/// The length of `$A$1`, `$A1`, or `A$1` at the start of `rest`.
fn cell_address(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut at = usize::from(bytes.first() == Some(&b'$'));
    let letters = bytes[at..]
        .iter()
        .take_while(|byte| byte.is_ascii_alphabetic())
        .count();
    if !(1..=2).contains(&letters) {
        return None;
    }
    at += letters;
    let dollar = bytes.get(at) == Some(&b'$');
    if !dollar && bytes.first() != Some(&b'$') {
        return None;
    }
    at += usize::from(dollar);
    let digits = bytes[at..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    (digits > 0).then_some(at + digits)
}

/// A token that starts with a letter or `_`: a unit, a constant, a cell
/// address, a function name, or an identifier.
fn word(rest: &str) -> (Token<'_>, usize) {
    let identifier = rest
        .char_indices()
        .find(|&(at, ch)| !(ch.is_alphanumeric() || ch == '_' || (at > 0 && ch == '@')))
        .map_or(rest.len(), |(at, _)| at);
    let run = rest
        .char_indices()
        .find(|(_, ch)| !(ch.is_alphanumeric() || *ch == '_'))
        .map_or(rest.len(), |(at, _)| at);
    let function = rest.starts_with(char::is_alphabetic).then(|| {
        let after = rest[run..].trim_start_matches('\t');
        after
            .starts_with('(')
            .then_some(rest.len() - after.len() + 1)
    });
    let word = &rest[..identifier];
    let constant = match word {
        "pi" => Some(std::f64::consts::PI),
        "e" => Some(std::f64::consts::E),
        "True" | "true" => Some(1.0),
        "None" | "False" | "false" => Some(0.0),
        _ => None,
    };
    let candidates = [
        unit_word(word).map(|unit| (Token::Unit(unit), identifier)),
        constant.map(|value| (Token::Constant { name: word, value }, identifier)),
        cell_address(rest).map(|length| (Token::Cell(&rest[..length]), length)),
        function
            .flatten()
            .map(|length| (Token::Function(&rest[..run]), length)),
        Some((Token::Identifier(word), identifier)),
    ];
    let mut best: Option<(Token<'_>, usize)> = None;
    for (token, length) in candidates.into_iter().flatten() {
        if best.as_ref().is_none_or(|(_, longest)| length > *longest) {
            best = Some((token, length));
        }
    }
    best.unwrap_or((Token::Identifier(word), identifier))
}

struct Parser<'a> {
    lexemes: Vec<Lexeme<'a>>,
    at: usize,
    nesting: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token<'a>> {
        self.peek_at(0)
    }

    fn peek_at(&self, offset: usize) -> Option<&Token<'a>> {
        self.lexemes
            .get(self.at + offset)
            .map(|lexeme| &lexeme.token)
    }

    fn done(&self) -> bool {
        self.at == self.lexemes.len()
    }

    fn symbol(&self, expected: char) -> bool {
        matches!(self.peek(), Some(Token::Symbol(found)) if *found == expected)
    }

    fn eat(&mut self, expected: char) -> bool {
        let found = self.symbol(expected);
        if found {
            self.at += 1;
        }
        found
    }

    fn require(&mut self, expected: char) -> Result<(), Unparsed> {
        if self.eat(expected) {
            Ok(())
        } else {
            Err(Unparsed::Syntax)
        }
    }

    /// `rule` from the current token, or no token consumed where it fails.
    fn attempt<T>(&mut self, rule: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        let at = self.at;
        let found = rule(self);
        if found.is_none() {
            self.at = at;
        }
        found
    }

    /// `rule` one level deeper.
    fn nested(
        &mut self,
        rule: impl FnOnce(&mut Self) -> Result<Node<'a>, Unparsed>,
    ) -> Result<Node<'a>, Unparsed> {
        if self.nesting == MAX_NESTING {
            return Err(Unparsed::Limit);
        }
        self.nesting += 1;
        let node = rule(self);
        self.nesting -= 1;
        node
    }

    fn expression(&mut self) -> Result<Node<'a>, Unparsed> {
        self.nested(Self::conditional)
    }

    /// `?:`, right-associative and below every other operator.
    fn conditional(&mut self) -> Result<Node<'a>, Unparsed> {
        let condition = self.binary(1)?;
        if !self.eat('?') {
            return Ok(condition);
        }
        let then = self.expression()?;
        self.require(':')?;
        let otherwise = self.expression()?;
        Ok(Node::Conditional {
            condition: Box::new(condition),
            then: Box::new(then),
            otherwise: Box::new(otherwise),
        })
    }

    /// The left-associative operators by level: comparisons, then `+ -`,
    /// then `* / %`, then `^`.
    fn binary(&mut self, level: u8) -> Result<Node<'a>, Unparsed> {
        if level > 4 {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        loop {
            let op = match (level, self.peek()) {
                (1, Some(Token::Compare(op))) => *op,
                (2, Some(Token::Symbol('+'))) => Operator::Add,
                (2, Some(Token::Minus)) => Operator::Subtract,
                (3, Some(Token::Symbol('*'))) => Operator::Multiply,
                (3, Some(Token::Symbol('/'))) => Operator::Divide,
                (3, Some(Token::Symbol('%'))) => Operator::Remainder,
                (4, Some(Token::Symbol('^'))) => Operator::Power,
                _ => return Ok(left),
            };
            self.at += 1;
            let right = match op {
                Operator::Divide => match self.attempt(Self::unit_suffix) {
                    Some((unit, _)) => Node::Unit(unit),
                    None => self.binary(level + 1)?,
                },
                _ => self.binary(level + 1)?,
            };
            left = Node::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
    }

    /// A sign binds tighter than every binary operator.
    fn unary(&mut self) -> Result<Node<'a>, Unparsed> {
        match self.peek() {
            Some(Token::Minus) => {
                self.at += 1;
                Ok(Node::Negate(Box::new(self.nested(Self::unary)?)))
            }
            Some(Token::Symbol('+')) => {
                self.at += 1;
                Ok(Node::Plus(Box::new(self.nested(Self::unary)?)))
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Node<'a>, Unparsed> {
        let token = self.peek().cloned().ok_or(Unparsed::Syntax)?;
        match token {
            Token::Number { text, .. } => {
                self.at += 1;
                Ok(self.number(text))
            }
            Token::Constant { name, value } => {
                self.at += 1;
                Ok(Node::Constant { name, value })
            }
            Token::Text(_) if !matches!(self.peek_at(1), Some(Token::Symbol('.' | '#'))) => {
                self.at += 1;
                Ok(Node::Text)
            }
            Token::Text(_) | Token::Identifier(_) | Token::Cell(_) | Token::Symbol('.') => {
                self.path()
            }
            Token::Function(name) => {
                self.at += 1;
                self.nested(|parser| parser.call(name))
            }
            Token::Symbol('(') => {
                self.at += 1;
                let inner = self.expression()?;
                self.require(')')?;
                Ok(Node::Group(Box::new(inner)))
            }
            _ => Err(Unparsed::Syntax),
        }
    }

    /// A number with the unit that follows it, and `1' 6"`, a sum of two
    /// numbers with `'` or `"`.
    fn number(&mut self, text: &'a str) -> Node<'a> {
        let Some((unit, bare)) = self.attempt(Self::unit_suffix) else {
            return Node::Number { text, unit: None };
        };
        let first = Node::Number {
            text,
            unit: Some(unit),
        };
        if let (true, Some(Token::Number { text: second, .. }), Some(Token::UsUnit(unit))) =
            (bare, self.peek_at(0), self.peek_at(1))
        {
            let second = Node::Number {
                text: second,
                unit: Some(unit.clone()),
            };
            self.at += 2;
            return Node::Binary {
                op: Operator::Add,
                left: Box::new(first),
                right: Box::new(second),
            };
        }
        first
    }

    /// The unit after a number or a `/`: a unit word, or a parenthesized
    /// unit expression, with integer powers. It is bare where it is a lone
    /// `'` or `"`.
    fn unit_suffix(&mut self) -> Option<(Unit, bool)> {
        let (mut unit, mut bare) = match self.peek()? {
            Token::Unit(unit) => (unit.clone(), false),
            Token::UsUnit(unit) => (unit.clone(), true),
            Token::Symbol('(') => {
                self.at += 1;
                let unit = self.unit_expression()?;
                if !self.symbol(')') {
                    return None;
                }
                (unit, false)
            }
            _ => return None,
        };
        self.at += 1;
        while self.eat('^') {
            let negative = matches!(self.peek(), Some(Token::Minus));
            self.at += usize::from(negative);
            let Some(Token::Number {
                integer: Some(power),
                ..
            }) = self.peek()
            else {
                return None;
            };
            let power = if negative {
                power.checked_neg()?
            } else {
                *power
            };
            self.at += 1;
            unit = unit.power(power)?;
            bare = false;
        }
        Some((unit, bare))
    }

    /// Units joined by `*` and `/`.
    fn unit_expression(&mut self) -> Option<Unit> {
        let mut unit = self.unit_suffix()?.0;
        loop {
            let sign = if self.eat('*') {
                1
            } else if self.eat('/') {
                -1
            } else {
                return Some(unit);
            };
            unit = unit.times(self.unit_suffix()?.0, sign)?;
        }
    }

    fn call(&mut self, name: &'a str) -> Result<Node<'a>, Unparsed> {
        let mut args = Vec::new();
        loop {
            let range = match (self.peek_at(0), self.peek_at(1), self.peek_at(2)) {
                (
                    Some(Token::Identifier(from) | Token::Cell(from)),
                    Some(Token::Symbol(':')),
                    Some(Token::Identifier(to) | Token::Cell(to)),
                ) => Some((*from, *to)),
                _ => None,
            };
            if let Some((from, to)) = range {
                self.at += 3;
                args.push(Argument::Range(from, to));
            } else {
                args.push(Argument::Value(self.expression()?));
            }
            if self.eat(',') || self.eat(';') {
                continue;
            }
            self.require(')')?;
            return Ok(Node::Call { name, args });
        }
    }

    /// A path: `.prop`, `object.prop`, `doc#object.prop`, each with an
    /// optional `.<<sub>>` sub-object, then further `.name` components and
    /// indexers.
    fn path(&mut self) -> Result<Node<'a>, Unparsed> {
        let start = self.lexemes.get(self.at).map_or(0, |lexeme| lexeme.start);
        let mut path = Path {
            span: start..start,
            origin: Origin::Named,
            components: Vec::new(),
            indexed: false,
        };
        if self.eat('.') {
            path.origin = Origin::Own;
            self.sub_object(&mut path);
            path.components.push(self.id_or_cell()?);
        } else {
            if matches!(self.peek_at(1), Some(Token::Symbol('#'))) {
                if !matches!(self.peek(), Some(Token::Text(_) | Token::Identifier(_))) {
                    return Err(Unparsed::Syntax);
                }
                self.at += 2;
                path.origin = Origin::Outside;
            }
            let label = matches!(self.peek(), Some(Token::Text(_)));
            self.object(&mut path)?;
            let named = matches!(
                self.peek_at(1),
                Some(Token::Text(_) | Token::Identifier(_) | Token::Cell(_))
            );
            if label || path.origin == Origin::Outside || (self.symbol('.') && named) {
                self.require('.')?;
                self.sub_object(&mut path);
                path.components.push(self.id_or_cell()?);
            }
        }
        loop {
            if self.symbol('.') && matches!(self.peek_at(1), Some(Token::Identifier(_))) {
                self.at += 1;
                path.components.push(self.id_or_cell()?);
            } else if self.eat('[') {
                path.indexed = true;
                while !self.eat(']') {
                    if !self.eat(':') {
                        self.expression()?;
                    }
                }
            } else {
                break;
            }
        }
        path.span.end = self
            .at
            .checked_sub(1)
            .and_then(|at| self.lexemes.get(at))
            .map_or(start, |lexeme| lexeme.end);
        Ok(Node::Path(path))
    }

    /// The object of a path: a `<<label>>`, or a name.
    fn object(&mut self, path: &mut Path) -> Result<(), Unparsed> {
        let name = match self.peek() {
            Some(Token::Text(quoted)) => {
                if path.origin != Origin::Outside {
                    path.origin = Origin::Label;
                }
                label(quoted)
            }
            Some(Token::Identifier(name) | Token::Cell(name)) => (*name).to_owned(),
            _ => return Err(Unparsed::Syntax),
        };
        self.at += 1;
        path.components.push(name);
        Ok(())
    }

    /// A `.<<sub>>` sub-object, which the path then names.
    fn sub_object(&mut self, path: &mut Path) {
        if matches!(self.peek(), Some(Token::Text(_)))
            && matches!(self.peek_at(1), Some(Token::Symbol('.')))
        {
            self.at += 2;
            path.origin = Origin::Outside;
        }
    }

    fn id_or_cell(&mut self) -> Result<String, Unparsed> {
        match self.peek() {
            Some(Token::Identifier(name) | Token::Cell(name)) => {
                let name = (*name).to_owned();
                self.at += 1;
                Ok(name)
            }
            _ => Err(Unparsed::Syntax),
        }
    }
}

/// The text of a `<<…>>` string, with `FreeCAD`'s escapes resolved.
fn label(quoted: &str) -> String {
    let inner = quoted
        .strip_prefix("<<")
        .and_then(|inner| inner.strip_suffix(">>"))
        .unwrap_or(quoted);
    let mut text = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            text.push(ch);
            continue;
        }
        match chars.next() {
            Some('t') => text.push('\t'),
            Some('n') => text.push('\n'),
            Some('r') => text.push('\r'),
            Some(escaped @ ('\\' | '\'' | '"')) => text.push(escaped),
            _ => {}
        }
    }
    text
}

#[cfg(test)]
mod tests;
